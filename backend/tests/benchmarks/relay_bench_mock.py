#!/usr/bin/env python3
"""Small OpenAI-compatible mock upstream for local relay benchmarks."""

from __future__ import annotations

import argparse
import json
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any


class RelayBenchHandler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    server_version = "NeoGateRelayBenchMock/1.0"

    def do_POST(self) -> None:
        length = int(self.headers.get("content-length", "0") or "0")
        raw_body = self.rfile.read(length) if length else b"{}"
        try:
            request = json.loads(raw_body)
        except json.JSONDecodeError:
            self.send_json(400, {"error": {"message": "invalid json"}})
            return

        delay_ms = self.server.delay_ms  # type: ignore[attr-defined]
        if delay_ms > 0:
            time.sleep(delay_ms / 1000)

        if request.get("stream"):
            self.send_stream(request)
        else:
            self.send_json(200, self.json_response(request))

    def do_GET(self) -> None:
        self.send_json(200, {"ok": True})

    def log_message(self, fmt: str, *args: Any) -> None:
        if self.server.quiet:  # type: ignore[attr-defined]
            return
        super().log_message(fmt, *args)

    def json_response(self, request: dict[str, Any]) -> dict[str, Any]:
        model = str(request.get("model") or "bench-model")
        output_bytes = self.server.output_bytes  # type: ignore[attr-defined]
        content = "x" * output_bytes
        return {
            "id": "chatcmpl-bench",
            "object": "chat.completion",
            "created": int(time.time()),
            "model": model,
            "choices": [
                {
                    "index": 0,
                    "message": {"role": "assistant", "content": content},
                    "finish_reason": "stop",
                }
            ],
            "usage": {
                "prompt_tokens": 8,
                "completion_tokens": max(1, output_bytes // 4),
                "total_tokens": 8 + max(1, output_bytes // 4),
            },
        }

    def send_stream(self, request: dict[str, Any]) -> None:
        model = str(request.get("model") or "bench-model")
        output_bytes = self.server.output_bytes  # type: ignore[attr-defined]
        chunks = max(1, self.server.stream_chunks)  # type: ignore[attr-defined]
        chunk_content = "x" * max(1, output_bytes // chunks)

        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.send_header("cache-control", "no-cache")
        self.send_header("connection", "close")
        self.end_headers()

        for index in range(chunks):
            event = {
                "id": "chatcmpl-bench",
                "object": "chat.completion.chunk",
                "created": int(time.time()),
                "model": model,
                "choices": [
                    {
                        "index": 0,
                        "delta": {"content": chunk_content},
                        "finish_reason": None,
                    }
                ],
            }
            self.wfile.write(f"data: {json.dumps(event, separators=(',', ':'))}\n\n".encode())
            self.wfile.flush()

        usage_event = {
            "id": "chatcmpl-bench",
            "object": "chat.completion.chunk",
            "created": int(time.time()),
            "model": model,
            "choices": [],
            "usage": {
                "prompt_tokens": 8,
                "completion_tokens": max(1, output_bytes // 4),
                "total_tokens": 8 + max(1, output_bytes // 4),
            },
        }
        self.wfile.write(f"data: {json.dumps(usage_event, separators=(',', ':'))}\n\n".encode())
        self.wfile.write(b"data: [DONE]\n\n")
        self.wfile.flush()

    def send_json(self, status: int, payload: dict[str, Any]) -> None:
        body = json.dumps(payload, separators=(",", ":")).encode()
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=18080)
    parser.add_argument("--delay-ms", type=float, default=0.0)
    parser.add_argument("--output-bytes", type=int, default=64)
    parser.add_argument("--stream-chunks", type=int, default=4)
    parser.add_argument("--quiet", action="store_true")
    args = parser.parse_args()

    server = ThreadingHTTPServer((args.host, args.port), RelayBenchHandler)
    server.delay_ms = args.delay_ms
    server.output_bytes = max(0, args.output_bytes)
    server.stream_chunks = max(1, args.stream_chunks)
    server.quiet = args.quiet
    print(f"mock upstream listening on http://{args.host}:{args.port}", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
