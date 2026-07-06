#!/usr/bin/env python3
"""Fetch DeepSeek public pricing into frontend/public/model-pricing.json.

DeepSeek publishes model pricing on a Docusaurus docs page as a single HTML
table. The table uses rowspan/colspan to group rows (e.g. a rowspan=3 "价格"
cell covers cache-hit / cache-miss / output lines, and colspan=2 rows carry
shared metadata). This script parses that table into a normalized provider-map
JSON that matches the shape used by the other fetch_*_pricing.py scripts.

No DeepSeek API key is required.
"""

from __future__ import annotations

import argparse
import datetime as dt
import html
import json
import re
import sys
import urllib.request
from decimal import Decimal, InvalidOperation
from html.parser import HTMLParser
from pathlib import Path
from typing import Any

DEFAULT_URL = "https://api-docs.deepseek.com/zh-cn/quick_start/pricing"
DEFAULT_OUTPUT = Path("frontend/public/model-pricing.json")
USER_AGENT = "Mozilla/5.0 (compatible; NeoGate pricing scraper; +https://github.com/neogate-io/NeoGate)"


class ScrapeError(RuntimeError):
    pass


class PricingHTMLParser(HTMLParser):
    """Collect top-level tables as grids of cells with rowspan/colspan."""

    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.tables: list[list[list[dict[str, Any]]]] = []
        self._table_depth = 0
        self._current_table: list[list[dict[str, Any]]] | None = None
        self._current_row: list[dict[str, Any]] | None = None
        self._current_cell: dict[str, Any] | None = None

    def handle_starttag(self, tag: str, attrs_list: list[tuple[str, str | None]]) -> None:
        attrs = {key: value or "" for key, value in attrs_list}
        if tag == "table":
            self._table_depth += 1
            if self._table_depth == 1:
                self._current_table = []
        elif self._table_depth and tag == "tr":
            self._current_row = []
        elif self._table_depth and tag in {"td", "th"}:
            self._current_cell = {
                "text": [],
                "rowspan": int(attrs.get("rowspan") or 1),
                "colspan": int(attrs.get("colspan") or 1),
            }
        elif self._current_cell is not None and tag == "br":
            self._current_cell["text"].append("\n")

    def handle_data(self, data: str) -> None:
        if self._current_cell is not None:
            self._current_cell["text"].append(data)

    def handle_endtag(self, tag: str) -> None:
        if self._table_depth and tag in {"td", "th"} and self._current_cell is not None:
            self._current_cell["text"] = normalize_text("".join(self._current_cell["text"]))
            if self._current_row is not None:
                self._current_row.append(self._current_cell)
            self._current_cell = None
        elif self._table_depth and tag == "tr" and self._current_row is not None:
            if self._current_table is not None:
                self._current_table.append(self._current_row)
            self._current_row = None
        elif tag == "table" and self._table_depth:
            self._table_depth -= 1
            if self._table_depth == 0 and self._current_table is not None:
                self.tables.append(self._current_table)
                self._current_table = None


def normalize_text(value: str) -> str:
    value = html.unescape(value).replace("\u200b", " ").replace("\xa0", " ")
    return re.sub(r"\s+", " ", value).strip()


def fetch_text(url: str) -> str:
    request = urllib.request.Request(
        url,
        headers={
            "User-Agent": USER_AGENT,
            "Accept": "text/html,application/xhtml+xml",
            "Accept-Language": "zh-CN,zh;q=0.9",
        },
    )
    with urllib.request.urlopen(request, timeout=40) as response:
        charset = response.headers.get_content_charset() or "utf-8"
        return response.read().decode(charset, errors="replace")


def expand_table(rows: list[list[dict[str, Any]]]) -> list[list[str]]:
    """Expand rowspan/colspan into a plain string grid."""
    grid: list[list[str]] = []
    spans: dict[tuple[int, int], tuple[str, int]] = {}
    for row_index, row_cells in enumerate(rows):
        row: list[str] = []
        col_index = 0
        while (row_index, col_index) in spans:
            text, remaining = spans.pop((row_index, col_index))
            row.append(text)
            if remaining > 1:
                spans[(row_index + 1, col_index)] = (text, remaining - 1)
            col_index += 1
        for cell in row_cells:
            while (row_index, col_index) in spans:
                text, remaining = spans.pop((row_index, col_index))
                row.append(text)
                if remaining > 1:
                    spans[(row_index + 1, col_index)] = (text, remaining - 1)
                col_index += 1
            text = str(cell["text"])
            rowspan = int(cell.get("rowspan") or 1)
            colspan = int(cell.get("colspan") or 1)
            for offset in range(colspan):
                row.append(text)
                if rowspan > 1:
                    spans[(row_index + 1, col_index + offset)] = (text, rowspan - 1)
            col_index += colspan
        grid.append(row)
    return grid


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


def context_tokens(value: str) -> int | None:
    """'1M' -> 1_000_000, '384K' -> 384_000."""
    raw = (value or "").strip()
    match = re.search(r"(\d+(?:\.\d+)?)\s*([KkMm])", raw)
    if not match:
        digits = re.search(r"\d+(?:,\d{3})*", raw)
        return int(digits.group(0).replace(",", "")) if digits else None
    number = Decimal(match.group(1))
    unit = match.group(2).lower()
    multiplier = {"k": 1_000, "m": 1_000_000}[unit]
    total = (number * multiplier).to_integral_value()
    return int(total)


def extract_model_columns(grid: list[list[str]]) -> list[str]:
    """First row holds the model ids after a colspan=2 label cell."""
    if not grid:
        raise ScrapeError("DeepSeek pricing table is empty")
    header = grid[0]
    # The header is [label(label), label(expanded), model1, model2, ...].
    # Skip the duplicated label cells and any empty placeholders.
    candidates = [cell for cell in header[2:] if cell.strip()]
    cleaned: list[str] = []
    for cell in candidates:
        match = re.search(r"deepseek-[0-9A-Za-z._-]+", cell)
        cleaned.append(match.group(0) if match else cell.strip())
    if not cleaned:
        raise ScrapeError("could not find model columns in DeepSeek pricing table")
    return cleaned


def find_pricing_rows(grid: list[list[str]]) -> list[list[str]]:
    """Locate the three pricing rows (cache hit / cache miss / output).

    Each pricing row has a rowspan-3 "价格" label in column 0 and a sublabel
    containing "百万tokens" in column 1. Matching on the "百万tokens" qualifier
    avoids colliding with metadata rows such as "输出长度".
    """
    keys = ("缓存命中", "缓存未命中", "输出")
    found: list[list[str]] = []
    for row in grid:
        label = row[1] if len(row) > 1 else ""
        if "百万tokens" in label and any(k in label for k in keys):
            found.append(row)
    return found


def record_from_grid(grid: list[list[str]], source_url: str) -> list[dict[str, Any]]:
    models = extract_model_columns(grid)
    pricing_rows = find_pricing_rows(grid)
    if len(pricing_rows) < 3:
        raise ScrapeError("could not locate the three DeepSeek pricing rows")

    # Map by keyword
    cache_hit_row = next((r for r in pricing_rows if "缓存命中" in (r[1] if len(r) > 1 else "")), pricing_rows[0])
    cache_miss_row = next((r for r in pricing_rows if "缓存未命中" in (r[1] if len(r) > 1 else "")), pricing_rows[1])
    output_row = next((r for r in pricing_rows if "输出" in (r[1] if len(r) > 1 else "")), pricing_rows[2])

    context = metadata_value(grid, "上下文长度")
    output_len = metadata_value(grid, "输出长度")

    records: list[dict[str, Any]] = []
    for index, model in enumerate(models):
        cache_hit = index_value(cache_hit_row, index)
        cache_miss = index_value(cache_miss_row, index)
        output = index_value(output_row, index)
        records.append({
            "section": "模型推理价格",
            "model": model,
            "context": context,
            "output_length": output_len,
            "limit": context_tokens(context),
            "prices": {
                "cache_read_cny_per_million_tokens": cache_hit,
                "input_cny_per_million_tokens": cache_miss,
                "output_cny_per_million_tokens": output,
            },
            "source_url": source_url,
        })
    return records


def metadata_value(grid: list[list[str]], key: str) -> str | None:
    for row in grid:
        if key in (row[0] if row else ""):
            # Row layout: [label, label(expanded), value_for_model1, value_for_model2]
            # Both model columns carry the same metadata; take the first.
            values = [cell for cell in row[2:] if cell.strip()]
            return values[0] if values else None
    return None


def index_value(row: list[str], index: int) -> str | None:
    values = [cell for cell in row[2:] if cell.strip() != ""]
    if index < len(values):
        return values[index]
    return None


def cost_from_record(record: dict[str, Any]) -> dict[str, int | float]:
    prices = record["prices"]
    cost: dict[str, int | float] = {}
    cache_read = amount(prices.get("cache_read_cny_per_million_tokens"))
    input_price = amount(prices.get("input_cny_per_million_tokens"))
    output_price = amount(prices.get("output_cny_per_million_tokens"))
    if cache_read is not None:
        cost["cache_read"] = cache_read
    if input_price is not None:
        cost["input"] = input_price
    if output_price is not None:
        cost["output"] = output_price
    # DeepSeek does not charge for cache writes separately on the pricing page.
    cost["basis"] = "token"
    return cost


def new_model_entry(model_id: str, name: str, record: dict[str, Any]) -> dict[str, Any]:
    modalities = {"input": ["text"], "output": ["text"]}
    entry = {
        "id": model_id,
        "name": name,
        "description": "",
        "modalities": modalities,
        "open_weights": False,
        "metadata": {"currency": "CNY", "cost_unit": "1M tokens", "pricing": []},
    }
    if record.get("limit"):
        entry["limit"] = {"context": record["limit"]}
    return entry


def to_provider_payload(records: list[dict[str, Any]], source_url: str) -> dict[str, Any]:
    models: dict[str, Any] = {}
    for record in records:
        name = record["model"]
        model_id = slug(name)
        entry = models.setdefault(model_id, new_model_entry(model_id, name, record))
        cost = cost_from_record(record)
        if cost and "cost" not in entry:
            entry["cost"] = cost
        entry["metadata"]["pricing"].append({
            "section": record.get("section"),
            "context": record.get("context"),
            "output_length": record.get("output_length"),
            "cost": cost,
            "raw": {
                "cache_read_cny_per_million_tokens": record["prices"].get("cache_read_cny_per_million_tokens"),
                "input_cny_per_million_tokens": record["prices"].get("input_cny_per_million_tokens"),
                "output_cny_per_million_tokens": record["prices"].get("output_cny_per_million_tokens"),
            },
            "source_url": record.get("source_url"),
        })
    return {
        "deepseek": {
            "id": "deepseek",
            "env": ["DEEPSEEK_API_KEY"],
            "npm": "@ai-sdk/openai-compatible",
            "api": "https://api.deepseek.com",
            "name": "DeepSeek",
            "doc": source_url,
            "models": models,
            "metadata": {
                "currency": "CNY",
                "cost_unit": "1M tokens",
                "source_url": source_url,
                "fetched_at": dt.datetime.now(dt.timezone.utc).isoformat(),
            },
        }
    }


def scrape(source_url: str) -> dict[str, Any]:
    html_text = fetch_text(source_url)
    parser = PricingHTMLParser()
    parser.feed(html_text)
    if not parser.tables:
        raise ScrapeError("could not find any pricing table on DeepSeek pricing page")
    grid = expand_table(parser.tables[0])
    records = record_from_grid(grid, source_url)
    if not records:
        raise ScrapeError("extracted table but found no DeepSeek pricing records")
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
    parser = argparse.ArgumentParser(description="Fetch DeepSeek public pricing into JSON")
    parser.add_argument("--url", default=DEFAULT_URL, help=f"pricing page URL (default: {DEFAULT_URL})")
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
        provider = payload["deepseek"]
        provider_count, model_count = update_provider_file(output, payload)
        print(f"updated deepseek with {len(provider['models'])} models in {output} ({provider_count} providers, {model_count} models total)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
