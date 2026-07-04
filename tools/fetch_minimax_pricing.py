#!/usr/bin/env python3
"""Fetch MiniMax pay-as-you-go model pricing into frontend/public/model-pricing.json."""

from __future__ import annotations

import argparse
import datetime as dt
import json
import re
import sys
import urllib.request
from decimal import Decimal, InvalidOperation
from pathlib import Path
from typing import Any

DEFAULT_URL = "https://platform.minimaxi.com/docs/guides/pricing-paygo.md"
DEFAULT_OUTPUT = Path("frontend/public/model-pricing.json")
USER_AGENT = "Mozilla/5.0 (compatible; NeoGate pricing scraper; +https://github.com/neogate-io/NeoGate)"


class ScrapeError(RuntimeError):
    pass


def fetch_text(url: str) -> str:
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT, "Accept": "text/markdown,text/plain,*/*"})
    with urllib.request.urlopen(req, timeout=40) as response:
        charset = response.headers.get_content_charset() or "utf-8"
        return response.read().decode(charset, errors="replace")


def parse_decimal(value: Any) -> str | None:
    raw = "" if value is None else str(value).strip()
    if not raw or raw in {"-", "—", "免费", "暂不支持", "不支持"}:
        return None
    numbers = re.findall(r"\d+(?:\.\d+)?", raw.replace(",", ""))
    if not numbers:
        return None
    try:
        return format(Decimal(numbers[-1]).normalize(), "f")
    except InvalidOperation:
        return None


def decimal_number(value: str | None) -> int | float | None:
    if value is None:
        return None
    decimal = Decimal(value)
    if decimal == decimal.to_integral_value():
        return int(decimal)
    return float(decimal)


def amount(raw: Any) -> int | float | None:
    return decimal_number(parse_decimal(raw))


def clean_cell(cell: str) -> str:
    cell = re.sub(r"<br\s*/?>", " ", cell)
    cell = re.sub(r"<[^>]+>", " ", cell)
    cell = cell.replace("\\*", "*")
    cell = re.sub(r"\*\*|__|`", "", cell)
    cell = re.sub(r"\s+", " ", cell).strip()
    return cell


def slug(value: str) -> str:
    return re.sub(r"[^0-9A-Za-z._-]+", "-", value.strip().lower()).strip("-")


def model_name(raw: str) -> str | None:
    match = re.search(r"\bMiniMax-[0-9A-Za-z._-]+\b", raw)
    return match.group(0) if match else None


def condition_from_model_cell(raw: str) -> str | None:
    for pattern in (r"[≤<>].*?tokens\\?\*?", r"输入 tokens\\?\*?"):
        match = re.search(pattern, raw)
        if match:
            return match.group(0).replace("\\*", "*").strip()
    return None


def parse_markdown_tables(text: str) -> list[dict[str, Any]]:
    records: list[dict[str, Any]] = []
    section = ""
    table: list[list[str]] = []

    def flush() -> None:
        nonlocal table
        if len(table) < 2:
            table = []
            return
        header = table[0]
        has_language_price = any("输入价格" in h for h in header) and any("输出价格" in h for h in header)
        if not has_language_price:
            table = []
            return
        for row in table[2:] if len(table) > 2 and re.match(r"^:?-+", table[1][0]) else table[1:]:
            if len(row) < 3:
                continue
            name = model_name(row[0])
            if not name:
                continue
            condition = condition_from_model_cell(row[0])
            record = {
                "section": section,
                "model": name,
                "condition": condition,
                "prices": {
                    "input_cny_per_million_tokens": {"raw": row[1], "amount_cny": parse_decimal(row[1]), "unit": "百万tokens"},
                    "output_cny_per_million_tokens": {"raw": row[2], "amount_cny": parse_decimal(row[2]), "unit": "百万tokens"},
                },
                "raw": {"model": row[0], "input": row[1], "output": row[2]},
            }
            if len(row) > 3:
                record["prices"]["cache_read_cny_per_million_tokens"] = {"raw": row[3], "amount_cny": parse_decimal(row[3]), "unit": "百万tokens"}
                record["raw"]["cache_read"] = row[3]
            if len(row) > 4:
                record["prices"]["cache_write_cny_per_million_tokens"] = {"raw": row[4], "amount_cny": parse_decimal(row[4]), "unit": "百万tokens"}
                record["raw"]["cache_write"] = row[4]
            records.append(record)
        table = []

    for line in text.splitlines():
        stripped = line.strip()
        if stripped.startswith("## "):
            flush()
            section = stripped.lstrip("# ").strip()
            continue
        if "|" in stripped and stripped.startswith("|"):
            table.append([clean_cell(cell) for cell in stripped.strip("|").split("|")])
        else:
            flush()
    flush()
    return records


def cost_from_record(record: dict[str, Any]) -> dict[str, int | float]:
    prices = record["prices"]
    cost: dict[str, int | float] = {}
    mapping = (
        ("input_cny_per_million_tokens", "input"),
        ("output_cny_per_million_tokens", "output"),
        ("cache_read_cny_per_million_tokens", "cache_read"),
        ("cache_write_cny_per_million_tokens", "cache_write"),
    )
    for source, target in mapping:
        value = amount((prices.get(source) or {}).get("amount_cny"))
        if value is not None:
            cost[target] = value
    return cost


def new_model_entry(model_id: str, name: str) -> dict[str, Any]:
    return {
        "id": model_id,
        "name": name,
        "description": "",
        "modalities": {"input": ["text"], "output": ["text"]},
        "open_weights": False,
        "metadata": {"currency": "CNY", "cost_unit": "1M tokens", "pricing": []},
    }


def to_provider_payload(records: list[dict[str, Any]], source_url: str) -> dict[str, Any]:
    models: dict[str, Any] = {}
    for record in records:
        name = record["model"]
        model_id = slug(name)
        entry = models.setdefault(model_id, new_model_entry(model_id, name))
        cost = cost_from_record(record)
        if cost and "cost" not in entry:
            entry["cost"] = cost
        entry["metadata"]["pricing"].append({
            "section": record.get("section"),
            "condition": record.get("condition"),
            "cost": cost,
            "raw": record.get("raw"),
        })
    return {
        "minimax": {
            "id": "minimax",
            "env": ["MINIMAX_API_KEY"],
            "npm": "@ai-sdk/openai-compatible",
            "api": "https://api.minimax.chat/v1",
            "name": "MiniMax",
            "doc": source_url,
            "models": models,
            "metadata": {"currency": "CNY", "cost_unit": "1M tokens", "source_url": source_url, "fetched_at": dt.datetime.now(dt.timezone.utc).isoformat()},
        }
    }


def scrape(source_url: str) -> dict[str, Any]:
    records = parse_markdown_tables(fetch_text(source_url))
    if not records:
        raise ScrapeError("extracted page but found no MiniMax pricing records")
    return to_provider_payload(records, source_url)


def update_provider_file(output: Path, provider_payload: dict[str, Any]) -> tuple[int, int]:
    merged: dict[str, Any] = {}
    if output.exists():
        try:
            existing = json.loads(output.read_text(encoding="utf-8"))
        except json.JSONDecodeError as exc:
            raise ScrapeError(f"invalid existing JSON in {output}: {exc}") from exc
        if not isinstance(existing, dict):
            raise ScrapeError(f"existing {output} must contain a provider map object")
        merged.update(existing)
    merged.update(provider_payload)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(dict(sorted(merged.items())), ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    model_count = sum(len(provider.get("models", {})) for provider in merged.values() if isinstance(provider, dict))
    return len(merged), model_count


def main() -> int:
    parser = argparse.ArgumentParser(description="Fetch MiniMax public pricing into JSON")
    parser.add_argument("--url", default=DEFAULT_URL, help=f"pricing markdown URL (default: {DEFAULT_URL})")
    parser.add_argument("--output", default=str(DEFAULT_OUTPUT), help=f"merged JSON output path (default: {DEFAULT_OUTPUT})")
    parser.add_argument("--stdout", action="store_true", help="print provider JSON to stdout instead of writing a file")
    args = parser.parse_args()
    try:
        payload = scrape(args.url)
    except Exception as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1
    if args.stdout:
        print(json.dumps(payload, ensure_ascii=False, indent=2))
    else:
        output = Path(args.output)
        provider = payload["minimax"]
        provider_count, model_count = update_provider_file(output, payload)
        print(f"updated minimax with {len(provider['models'])} models in {output} ({provider_count} providers, {model_count} models total)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
