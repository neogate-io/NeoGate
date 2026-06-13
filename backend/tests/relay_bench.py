#!/usr/bin/env python3
"""Run a small NeoGate relay benchmark with wrk.

This script expects NeoGate to be running and configured with the mock upstream
from relay_bench_mock.py. It keeps dependencies to the standard library plus
the external wrk binary.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from dataclasses import dataclass
from typing import Iterable


@dataclass
class ResourceSample:
    rss_kib: int
    cpu_percent: float


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--url",
        default=os.environ.get(
            "NEOGATE_BENCH_URL", "http://127.0.0.1:8080/v1/chat/completions"
        ),
    )
    parser.add_argument("--api-key", default=os.environ.get("NEOGATE_API_KEY"))
    parser.add_argument("--model", default=os.environ.get("NEOGATE_BENCH_MODEL", "bench-model"))
    parser.add_argument("--duration", default=os.environ.get("NEOGATE_BENCH_DURATION", "30s"))
    parser.add_argument("--threads", type=int, default=int(os.environ.get("NEOGATE_BENCH_THREADS", "4")))
    parser.add_argument(
        "--connections",
        type=int,
        default=int(os.environ.get("NEOGATE_BENCH_CONNECTIONS", "128")),
    )
    parser.add_argument("--max-tokens", type=int, default=16)
    parser.add_argument("--stream", action="store_true")
    parser.add_argument("--pid", type=int, default=pid_from_env())
    parser.add_argument("--sample-interval", type=float, default=1.0)
    return parser.parse_args()


def pid_from_env() -> int | None:
    value = os.environ.get("NEOGATE_PID")
    if not value:
        return None
    try:
        return int(value)
    except ValueError:
        return None


def request_body(model: str, max_tokens: int, stream: bool) -> str:
    payload: dict[str, object] = {
        "model": model,
        "messages": [{"role": "user", "content": "ping"}],
        "max_tokens": max_tokens,
    }
    if stream:
        payload["stream"] = True
    return json.dumps(payload, separators=(",", ":"))


def lua_string(value: str) -> str:
    return value.replace("\\", "\\\\").replace("'", "\\'")


def wrk_script(body: str, api_key: str) -> str:
    return (
        f"wrk.method = 'POST'\n"
        f"wrk.body = '{lua_string(body)}'\n"
        f"wrk.headers['authorization'] = 'Bearer {lua_string(api_key)}'\n"
        "wrk.headers['content-type'] = 'application/json'\n"
    )


def sample_process(pid: int, interval: float, stop: threading.Event, samples: list[ResourceSample]) -> None:
    while not stop.is_set():
        sample = read_process_sample(pid)
        if sample is not None:
            samples.append(sample)
        stop.wait(interval)


def read_process_sample(pid: int) -> ResourceSample | None:
    try:
        result = subprocess.run(
            ["ps", "-o", "rss=", "-o", "%cpu=", "-p", str(pid)],
            check=False,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
        )
    except OSError:
        return None
    line = result.stdout.strip()
    if not line:
        return None
    parts = line.split()
    if len(parts) < 2:
        return None
    try:
        return ResourceSample(rss_kib=int(float(parts[0])), cpu_percent=float(parts[1]))
    except ValueError:
        return None


def summarize_samples(samples: Iterable[ResourceSample]) -> str:
    samples = list(samples)
    if not samples:
        return "resource samples: unavailable"
    rss_values = [sample.rss_kib for sample in samples]
    cpu_values = [sample.cpu_percent for sample in samples]
    return (
        "resource samples: "
        f"rss_max={max(rss_values) / 1024:.1f}MiB "
        f"rss_avg={sum(rss_values) / len(rss_values) / 1024:.1f}MiB "
        f"cpu_max={max(cpu_values):.1f}% "
        f"cpu_avg={sum(cpu_values) / len(cpu_values):.1f}% "
        f"samples={len(samples)}"
    )


def main() -> int:
    args = parse_args()
    if not args.api_key:
        print("NEOGATE_API_KEY or --api-key is required", file=sys.stderr)
        return 2
    if shutil.which("wrk") is None:
        print("wrk is required but was not found in PATH", file=sys.stderr)
        return 2

    body = request_body(args.model, args.max_tokens, args.stream)
    samples: list[ResourceSample] = []
    stop = threading.Event()
    sampler: threading.Thread | None = None
    if args.pid:
        sampler = threading.Thread(
            target=sample_process,
            args=(args.pid, args.sample_interval, stop, samples),
            daemon=True,
        )
        sampler.start()

    script_path: str | None = None
    try:
        with tempfile.NamedTemporaryFile("w", suffix=".lua", delete=False) as handle:
            handle.write(wrk_script(body, args.api_key))
            script_path = handle.name
        command = [
            "wrk",
            f"-t{args.threads}",
            f"-c{args.connections}",
            f"-d{args.duration}",
            "--latency",
            "-s",
            script_path,
            args.url,
        ]
        print("mode:", "stream" if args.stream else "json")
        print("command:", " ".join(command[:-1] + [args.url]))
        started = time.monotonic()
        result = subprocess.run(command, check=False, text=True)
        elapsed = time.monotonic() - started
    finally:
        stop.set()
        if sampler is not None:
            sampler.join(timeout=2)
        if script_path is not None:
            try:
                os.unlink(script_path)
            except Exception:
                pass

    print(f"elapsed_seconds={elapsed:.3f}")
    print(summarize_samples(samples))
    return result.returncode


if __name__ == "__main__":
    raise SystemExit(main())
