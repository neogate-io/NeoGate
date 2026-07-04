#!/usr/bin/env python3
"""Fetch Alibaba Model Studio / Qwen pricing from the public pricing page.

The Alibaba Cloud help page contains server-rendered HTML tables for Qwen model
pricing. This script parses those tables with Python's stdlib, converts the rows
to the same provider-map shape used by frontend/public/model-pricing.json, and
updates only the DashScope provider while preserving existing providers.

No Alibaba Cloud account or API key is required.
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

DEFAULT_URL = "https://help.aliyun.com/zh/model-studio/model-pricing"
DEFAULT_OUTPUT = Path("frontend/public/model-pricing.json")
USER_AGENT = "Mozilla/5.0 (compatible; NeoGate pricing scraper; +https://github.com/neogate-io/NeoGate)"


class ScrapeError(RuntimeError):
    pass


class PricingHTMLParser(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.elements: list[tuple[str, Any]] = []
        self._table_depth = 0
        self._current_table: dict[str, Any] | None = None
        self._current_row: list[dict[str, Any]] | None = None
        self._current_cell: dict[str, Any] | None = None
        self._heading_level: str | None = None
        self._heading_text: list[str] = []

    def handle_starttag(self, tag: str, attrs_list: list[tuple[str, str | None]]) -> None:
        attrs = {key: value or "" for key, value in attrs_list}
        if tag == "table":
            self._table_depth += 1
            if self._table_depth == 1:
                self._current_table = {"attrs": attrs, "rows": []}
        elif self._table_depth and tag == "tr":
            self._current_row = []
        elif self._table_depth and tag in {"td", "th"}:
            self._current_cell = {
                "text": [],
                "rowspan": int(attrs.get("rowspan") or 1),
                "colspan": int(attrs.get("colspan") or 1),
            }
        elif not self._table_depth and tag in {"h1", "h2", "h3", "h4"}:
            self._heading_level = tag
            self._heading_text = []
        elif self._current_cell is not None and tag == "br":
            self._current_cell["text"].append("\n")

    def handle_data(self, data: str) -> None:
        if self._current_cell is not None:
            self._current_cell["text"].append(data)
        elif self._heading_level is not None:
            self._heading_text.append(data)

    def handle_endtag(self, tag: str) -> None:
        if self._table_depth and tag in {"td", "th"} and self._current_cell is not None:
            text = normalize_text("".join(self._current_cell["text"]))
            self._current_cell["text"] = text
            if self._current_row is not None:
                self._current_row.append(self._current_cell)
            self._current_cell = None
        elif self._table_depth and tag == "tr" and self._current_row is not None:
            if self._current_table is not None:
                self._current_table["rows"].append(self._current_row)
            self._current_row = None
        elif tag == "table" and self._table_depth:
            self._table_depth -= 1
            if self._table_depth == 0 and self._current_table is not None:
                self.elements.append(("table", self._current_table))
                self._current_table = None
        elif tag == self._heading_level:
            text = normalize_text("".join(self._heading_text))
            if text:
                self.elements.append(("heading", (self._heading_level, text)))
            self._heading_level = None
            self._heading_text = []


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


def current_headings(headings: dict[str, str]) -> list[str]:
    return [headings[level] for level in ("h1", "h2", "h3", "h4") if headings.get(level)]


def clean_key(value: str) -> str:
    compact = re.sub(r"\s+", "", value.replace("（", "(").replace("）", ")"))
    mapping = {
        "模型ID(ModelID)": "model",
        "模型ID": "model",
        "服务部署范围": "deployment_scope",
        "模式": "mode",
        "单次请求的输入Token数": "condition",
        "单次请求的输入Token范围": "condition",
        "输入Token范围": "condition",
        "输入单价(每百万Token)": "input_cny_per_million_tokens",
        "输出单价(每百万Token)思维链+回答": "output_cny_per_million_tokens",
        "输出单价(每百万Token)": "output_cny_per_million_tokens",
        "免费额度(注)有效期:阿里云百炼开通后90天内": "free_quota",
        "免费额度（注）有效期:阿里云百炼开通后90天内": "free_quota",
        "上下文缓存写入价格(每百万Token)": "cache_write_cny_per_million_tokens",
        "上下文缓存命中价格(每百万Token)": "cache_read_cny_per_million_tokens",
        "单价": "price",
        "价格": "price",
    }
    if compact in mapping:
        return mapping[compact]
    if "输入单价" in compact and "百万" in compact:
        return "input_cny_per_million_tokens"
    if "输出单价" in compact and "百万" in compact:
        return "output_cny_per_million_tokens"
    if "缓存" in compact and "写入" in compact:
        return "cache_write_cny_per_million_tokens"
    if "缓存" in compact and "命中" in compact:
        return "cache_read_cny_per_million_tokens"
    return re.sub(r"[^0-9A-Za-z\u4e00-\u9fff]+", "_", compact).strip("_") or "column"


def parse_decimal(value: Any) -> str | None:
    raw = "" if value is None else str(value).strip()
    if not raw or raw in {"-", "—", "免费", "限时免费", "无"}:
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


def price_amount(raw: Any) -> int | float | None:
    return decimal_number(parse_decimal(raw))


def slug(value: str) -> str:
    return re.sub(r"[^0-9A-Za-z._-]+", "-", value.strip().lower()).strip("-")


def model_id_from_cell(value: str) -> str | None:
    match = re.search(r"\bqwen[0-9A-Za-z._-]*\b", value)
    return match.group(0) if match else None


def is_qwen_table(table: dict[str, Any], grid: list[list[str]]) -> bool:
    attrs = table.get("attrs") or {}
    class_name = attrs.get("class") or ""
    text = " ".join(" ".join(row) for row in grid[:6])
    return "qwen" in class_name or "qwen" in text.lower()


def table_records(grid: list[list[str]], headings: list[str]) -> list[dict[str, Any]]:
    if len(grid) < 2:
        return []
    headers = [clean_key(cell) for cell in grid[0]]
    seen: dict[str, int] = {}
    unique_headers: list[str] = []
    for header in headers:
        count = seen.get(header, 0)
        seen[header] = count + 1
        unique_headers.append(header if count == 0 else f"{header}_{count + 1}")

    records: list[dict[str, Any]] = []
    for cells in grid[1:]:
        cells = cells + [""] * (len(unique_headers) - len(cells))
        raw = {key: value.strip() for key, value in zip(unique_headers, cells)}
        model = model_id_from_cell(raw.get("model", ""))
        if not model:
            continue
        prices = {
            "input_cny_per_million_tokens": normalize_price(raw.get("input_cny_per_million_tokens"), "百万Token"),
            "output_cny_per_million_tokens": normalize_price(raw.get("output_cny_per_million_tokens"), "百万Token"),
            "cache_write_cny_per_million_tokens": normalize_price(raw.get("cache_write_cny_per_million_tokens"), "百万Token"),
            "cache_read_cny_per_million_tokens": normalize_price(raw.get("cache_read_cny_per_million_tokens"), "百万Token"),
        }
        records.append(
            {
                "section": headings[0] if headings else None,
                "subsection": headings[1] if len(headings) > 1 else None,
                "category": headings[2] if len(headings) > 2 else None,
                "region": raw.get("deployment_scope"),
                "model": model,
                "condition": raw.get("condition"),
                "mode": raw.get("mode"),
                "prices": {key: value for key, value in prices.items() if value["raw"]},
                "raw": raw,
            }
        )
    return records


def normalize_price(raw: Any, unit: str | None) -> dict[str, Any]:
    value = "" if raw is None else str(raw).strip()
    return {"raw": value, "amount_cny": parse_decimal(value), "unit": unit}


def cost_from_record(record: dict[str, Any]) -> dict[str, int | float]:
    prices = record.get("prices") or {}
    cost: dict[str, int | float] = {}
    for source, target in (
        ("input_cny_per_million_tokens", "input"),
        ("output_cny_per_million_tokens", "output"),
        ("cache_read_cny_per_million_tokens", "cache_read"),
        ("cache_write_cny_per_million_tokens", "cache_write"),
    ):
        value = price_amount((prices.get(source) or {}).get("amount_cny"))
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


def to_provider_payload(records: list[dict[str, Any]], tables: list[dict[str, Any]], source_url: str) -> dict[str, Any]:
    models: dict[str, dict[str, Any]] = {}
    for record in records:
        model = str(record["model"])
        model_id = slug(model)
        entry = models.setdefault(model_id, new_model_entry(model_id, model))
        cost = cost_from_record(record)
        if cost and "cost" not in entry:
            entry["cost"] = cost
        entry["metadata"]["pricing"].append(
            {
                "section": record.get("section"),
                "subsection": record.get("subsection"),
                "category": record.get("category"),
                "region": record.get("region"),
                "mode": record.get("mode"),
                "condition": record.get("condition"),
                "cost": cost,
                "raw": record.get("raw"),
            }
        )

    return {
        "dashscope": {
            "id": "dashscope",
            "env": ["DASHSCOPE_API_KEY"],
            "npm": "@ai-sdk/openai-compatible",
            "api": "https://dashscope.aliyuncs.com/compatible-mode/v1",
            "name": "阿里云百炼 / 通义千问",
            "doc": source_url,
            "models": models,
            "metadata": {
                "currency": "CNY",
                "cost_unit": "1M tokens",
                "source_url": source_url,
                "fetched_at": dt.datetime.now(dt.timezone.utc).isoformat(),
                "tables": tables,
            },
        }
    }


def scrape(source_url: str) -> dict[str, Any]:
    parser = PricingHTMLParser()
    parser.feed(fetch_text(source_url))
    headings: dict[str, str] = {}
    records: list[dict[str, Any]] = []
    tables: list[dict[str, Any]] = []

    for kind, value in parser.elements:
        if kind == "heading":
            level, text = value
            headings[level] = text
            if level == "h1":
                headings.pop("h2", None)
                headings.pop("h3", None)
                headings.pop("h4", None)
            elif level == "h2":
                headings.pop("h3", None)
                headings.pop("h4", None)
            elif level == "h3":
                headings.pop("h4", None)
            continue

        table = value
        grid = expand_table(table["rows"])
        if not grid or not is_qwen_table(table, grid):
            continue
        parsed = table_records(grid, current_headings(headings))
        if not parsed:
            continue
        tables.append(
            {
                "headings": current_headings(headings),
                "row_count": len(grid),
                "column_count": max(len(row) for row in grid),
                "headers": grid[0],
            }
        )
        records.extend(parsed)

    if not records:
        raise ScrapeError("extracted page but found no Qwen pricing records")
    return to_provider_payload(records, tables, source_url)


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
    parser = argparse.ArgumentParser(description="Fetch Alibaba Model Studio / Qwen public pricing into JSON")
    parser.add_argument("--url", default=DEFAULT_URL, help=f"pricing page URL (default: {DEFAULT_URL})")
    parser.add_argument("--output", default=str(DEFAULT_OUTPUT), help=f"merged JSON output path (default: {DEFAULT_OUTPUT})")
    parser.add_argument("--stdout", action="store_true", help="print provider JSON to stdout instead of writing a file")
    args = parser.parse_args()

    try:
        payload = scrape(args.url)
    except Exception as exc:  # noqa: BLE001 - CLI should return a readable failure
        print(f"error: {exc}", file=sys.stderr)
        return 1

    if args.stdout:
        print(json.dumps(payload, ensure_ascii=False, indent=2))
    else:
        output = Path(args.output)
        provider = payload["dashscope"]
        table_count = len(provider["metadata"]["tables"])
        provider_count, model_count = update_provider_file(output, payload)
        print(
            f"updated dashscope with {len(provider['models'])} models from {table_count} tables "
            f"in {output} ({provider_count} providers, {model_count} models total)"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
