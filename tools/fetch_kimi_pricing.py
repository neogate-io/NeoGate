#!/usr/bin/env python3
"""Fetch Kimi / Moonshot model pricing into frontend/public/model-pricing.json."""

from __future__ import annotations

import argparse
import ast
import datetime as dt
import json
import re
import sys
import urllib.request
from decimal import Decimal, InvalidOperation
from pathlib import Path
from typing import Any

DEFAULT_URLS = [
    "https://platform.kimi.com/docs/pricing/chat-k27-code.md",
    "https://platform.kimi.com/docs/pricing/chat-k26.md",
    "https://platform.kimi.com/docs/pricing/chat-v1.md",
]
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
    match = re.search(r"\d+(?:\.\d+)?", raw.replace(",", ""))
    if not match:
        return None
    try:
        return format(Decimal(match.group(0)).normalize(), "f")
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


def slug(value: str) -> str:
    return re.sub(r"[^0-9A-Za-z._-]+", "-", value.strip().lower()).strip("-")


def extract_title(markdown: str) -> str:
    match = re.search(r"^#\s+(.+)$", markdown, re.M)
    return match.group(1).strip() if match else "模型推理价格"


def extract_rows(markdown: str) -> list[list[str]]:
    match = re.search(r"rows=\{\[(.*?)\]\}", markdown, re.S)
    if not match:
        raise ScrapeError("could not find rows={[...]} in Kimi pricing markdown")
    source = "[" + match.group(1).strip().rstrip(",") + "]"
    try:
        rows = ast.literal_eval(source)
    except Exception as exc:
        raise ScrapeError(f"could not parse Kimi pricing rows: {exc}") from exc
    if not isinstance(rows, list):
        raise ScrapeError("Kimi pricing rows are not a list")
    return rows


def context_limit(value: str) -> int | None:
    match = re.search(r"\d+(?:,\d{3})*", value or "")
    return int(match.group(0).replace(",", "")) if match else None


def record_from_row(row: list[str], title: str, source_url: str) -> dict[str, Any] | None:
    if len(row) == 6:
        model, unit, cache_hit, cache_miss, output, context = row
        prices = {
            "cache_read_cny_per_million_tokens": {"raw": cache_hit, "amount_cny": parse_decimal(cache_hit), "unit": unit},
            "input_cny_per_million_tokens": {"raw": cache_miss, "amount_cny": parse_decimal(cache_miss), "unit": unit},
            "output_cny_per_million_tokens": {"raw": output, "amount_cny": parse_decimal(output), "unit": unit},
        }
    elif len(row) == 5:
        model, unit, input_price, output, context = row
        prices = {
            "input_cny_per_million_tokens": {"raw": input_price, "amount_cny": parse_decimal(input_price), "unit": unit},
            "output_cny_per_million_tokens": {"raw": output, "amount_cny": parse_decimal(output), "unit": unit},
        }
    else:
        return None
    return {
        "section": title,
        "model": model,
        "context": context,
        "limit": context_limit(context),
        "prices": prices,
        "source_url": source_url,
        "raw": row,
    }


def scrape_one(url: str) -> list[dict[str, Any]]:
    markdown = fetch_text(url)
    title = extract_title(markdown)
    rows = extract_rows(markdown)
    records = [record_from_row(row, title, url) for row in rows]
    return [record for record in records if record]


def cost_from_record(record: dict[str, Any]) -> dict[str, int | float]:
    prices = record["prices"]
    cost: dict[str, int | float] = {}
    for source, target in (
        ("input_cny_per_million_tokens", "input"),
        ("output_cny_per_million_tokens", "output"),
        ("cache_read_cny_per_million_tokens", "cache_read"),
    ):
        value = amount((prices.get(source) or {}).get("amount_cny"))
        if value is not None:
            cost[target] = value
    return cost


def new_model_entry(model_id: str, name: str, record: dict[str, Any]) -> dict[str, Any]:
    modalities = {"input": ["text"], "output": ["text"]}
    if "vision" in name or name.startswith("kimi-k2.6") or name.startswith("kimi-k2.7"):
        modalities["input"] = ["text", "image"]
    return {
        "id": model_id,
        "name": name,
        "description": "",
        "modalities": modalities,
        "open_weights": False,
        "metadata": {"currency": "CNY", "cost_unit": "1M tokens", "pricing": []},
    }


def to_provider_payload(records: list[dict[str, Any]], source_urls: list[str]) -> dict[str, Any]:
    models: dict[str, Any] = {}
    for record in records:
        name = record["model"]
        model_id = slug(name)
        entry = models.setdefault(model_id, new_model_entry(model_id, name, record))
        if record.get("limit") and "limit" not in entry:
            entry["limit"] = {"context": record["limit"]}
        cost = cost_from_record(record)
        if cost and "cost" not in entry:
            entry["cost"] = cost
        entry["metadata"]["pricing"].append({
            "section": record.get("section"),
            "context": record.get("context"),
            "cost": cost,
            "raw": record.get("raw"),
            "source_url": record.get("source_url"),
        })
    return {
        "moonshot": {
            "id": "moonshot",
            "env": ["MOONSHOT_API_KEY"],
            "npm": "@ai-sdk/openai-compatible",
            "api": "https://api.moonshot.cn/v1",
            "name": "Kimi / Moonshot",
            "doc": "https://platform.kimi.com/docs/pricing/chat",
            "models": models,
            "metadata": {"currency": "CNY", "cost_unit": "1M tokens", "source_urls": source_urls, "fetched_at": dt.datetime.now(dt.timezone.utc).isoformat()},
        }
    }


def scrape(urls: list[str]) -> dict[str, Any]:
    records: list[dict[str, Any]] = []
    for url in urls:
        records.extend(scrape_one(url))
    if not records:
        raise ScrapeError("extracted pages but found no Kimi pricing records")
    return to_provider_payload(records, urls)


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
    parser = argparse.ArgumentParser(description="Fetch Kimi / Moonshot public pricing into JSON")
    parser.add_argument("--url", action="append", dest="urls", help="pricing markdown URL; can be repeated")
    parser.add_argument("--output", default=str(DEFAULT_OUTPUT), help=f"merged JSON output path (default: {DEFAULT_OUTPUT})")
    parser.add_argument("--stdout", action="store_true", help="print provider JSON to stdout instead of writing a file")
    args = parser.parse_args()
    urls = args.urls or DEFAULT_URLS
    try:
        payload = scrape(urls)
    except Exception as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1
    if args.stdout:
        print(json.dumps(payload, ensure_ascii=False, indent=2))
    else:
        output = Path(args.output)
        provider = payload["moonshot"]
        provider_count, model_count = update_provider_file(output, payload)
        print(f"updated moonshot with {len(provider['models'])} models in {output} ({provider_count} providers, {model_count} models total)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
