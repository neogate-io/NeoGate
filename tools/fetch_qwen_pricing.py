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
        "输出视频模式": "mode",
        "输出视频类型": "mode",
        "单次请求的输入Token数": "condition",
        "单次请求的输入Token范围": "condition",
        "输入Token范围": "condition",
        "输出视频分辨率": "resolution",
        "输入单价(每百万Token)": "input_cny_per_million_tokens",
        "输出单价(每百万Token)思维链+回答": "output_cny_per_million_tokens",
        "输出单价(每百万Token)": "output_cny_per_million_tokens",
        "免费额度(注)有效期:阿里云百炼开通后90天内": "free_quota",
        "免费额度（注）有效期:阿里云百炼开通后90天内": "free_quota",
        "上下文缓存写入价格(每百万Token)": "cache_write_cny_per_million_tokens",
        "上下文缓存命中价格(每百万Token)": "cache_read_cny_per_million_tokens",
        "输入单价(每万字符)": "input_cny_per_10k_char",
        "输出单价(每秒)": "output_cny_per_second",
        "单价": "price",
        "价格": "price",
        "输出单价": "output_price",
    }
    if compact in mapping:
        return mapping[compact]
    if "输入单价" in compact and "百万" in compact:
        return "input_cny_per_million_tokens"
    if "输出单价" in compact and "百万" in compact:
        return "output_cny_per_million_tokens"
    if "输入单价" in compact and "万字符" in compact:
        return "input_cny_per_10k_char"
    if ("输出单价" in compact or "单价" in compact) and "秒" in compact:
        return "output_cny_per_second"
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
    # 支持 qwen/wan/z-image/happyhorse/fun-music/qwen3-tts 等前缀
    for pattern in (
        r"\bqwen[\w.-]*\b",
        r"\bwan[\w.-]*\b",
        r"\bz-image[\w.-]*\b",
        r"\bhappyhorse[\w.-]*\b",
        r"\bfun-music[\w.-]*\b",
        r"\bcosyvoice[\w.-]*\b",
    ):
        match = re.search(pattern, value, re.IGNORECASE)
        if match:
            return match.group(0).lower()
    return None


def is_qwen_table(table: dict[str, Any], grid: list[list[str]]) -> bool:
    # 放宽:只要表头含价格列(单价/价格/百万Token/万字符/秒/张)即认为是定价表
    header_text = " ".join(grid[0]) if grid else ""
    has_price = any(kw in header_text for kw in ("单价", "价格", "百万Token", "万字符", "元/秒", "元/张"))
    return bool(has_price)


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
        # 非 token 口径:output_price 根据文本单位分流(元/张 vs 元/秒)
        output_price_raw = raw.get("output_price") or ""
        if output_price_raw:
            if "秒" in output_price_raw:
                prices["per_second"] = normalize_price(output_price_raw, "元/秒")
            else:
                prices["per_image"] = normalize_price(output_price_raw, "元/张")
        if raw.get("output_cny_per_second"):
            prices["per_second"] = normalize_price(raw.get("output_cny_per_second"), "元/秒")
        if raw.get("input_cny_per_10k_char"):
            prices["per_10k_token_input"] = normalize_price(raw.get("input_cny_per_10k_char"), "元/万字符")
        # 元/张 的价格文本可能形如 "0.5元/张" 或 "关闭提示词改写:0.1元/张",normalize_price 已取首数字
        records.append(
            {
                "section": headings[0] if headings else None,
                "subsection": headings[1] if len(headings) > 1 else None,
                "category": headings[2] if len(headings) > 2 else None,
                "region": raw.get("deployment_scope"),
                "model": model,
                "condition": raw.get("condition") or raw.get("resolution"),
                "mode": raw.get("mode"),
                "prices": {key: value for key, value in prices.items() if value["raw"]},
                "raw": raw,
            }
        )
    return records


def normalize_price(raw: Any, unit: str | None) -> dict[str, Any]:
    value = "" if raw is None else str(raw).strip()
    return {"raw": value, "amount_cny": parse_decimal(value), "unit": unit}


def infer_basis(record: dict[str, Any], prices: dict[str, Any]) -> str:
    if prices.get("per_image", {}).get("amount_cny"):
        return "image"
    if prices.get("per_second", {}).get("amount_cny"):
        return "second"
    if prices.get("per_10k_token_input", {}).get("amount_cny"):
        return "per_10k_token"
    return "token"


def cost_from_record(record: dict[str, Any]) -> dict[str, Any]:
    prices = record.get("prices") or {}
    cost: dict[str, Any] = {}
    basis = infer_basis(record, prices)
    if basis == "image":
        value = price_amount(prices.get("per_image", {}).get("amount_cny"))
        if value is not None:
            cost["per_image"] = value
    elif basis == "second":
        value = price_amount(prices.get("per_second", {}).get("amount_cny"))
        if value is not None:
            cost["per_second"] = value
    elif basis == "per_10k_token":
        value = price_amount(prices.get("per_10k_token_input", {}).get("amount_cny"))
        if value is not None:
            cost["per_10k_token_input"] = value
            cost["per_10k_token_output"] = value
    else:  # token
        for source, target in (
            ("input_cny_per_million_tokens", "input"),
            ("output_cny_per_million_tokens", "output"),
            ("cache_read_cny_per_million_tokens", "cache_read"),
            ("cache_write_cny_per_million_tokens", "cache_write"),
        ):
            value = price_amount(prices.get(source, {}).get("amount_cny"))
            if value is not None:
                cost[target] = value
    cost["basis"] = basis
    return cost


def modalities_for_section(section: str | None, subsection: str | None = None) -> dict[str, list[str]]:
    text = f"{section or ''} {subsection or ''}"
    if "图像" in text:
        return {"input": ["text", "image"], "output": ["image"]}
    if "视频" in text:
        return {"input": ["text", "image", "video"], "output": ["video"]}
    if "语音" in text or "音乐" in text:
        return {"input": ["text"], "output": ["audio"]}
    if "向量" in text or "排序" in text:
        return {"input": ["text"], "output": ["embedding"]}
    if "3D" in text:
        return {"input": ["text", "image"], "output": ["3d"]}
    return {"input": ["text"], "output": ["text"]}


def new_model_entry(model_id: str, name: str, record: dict[str, Any]) -> dict[str, Any]:
    return {
        "id": model_id,
        "name": name,
        "description": "",
        "modalities": modalities_for_section(record.get("section"), record.get("subsection")),
        "open_weights": False,
        "metadata": {"currency": "CNY", "cost_unit": "1M tokens unless noted", "pricing": []},
    }


def video_tier_label(record: dict[str, Any], resolution: str) -> str:
    parts = [
        str(record.get("region") or "").strip(),
        str(record.get("mode") or "").strip(),
        resolution.strip(),
    ]
    return " · ".join(part for part in parts if part)


def to_provider_payload(records: list[dict[str, Any]], tables: list[dict[str, Any]], source_url: str) -> dict[str, Any]:
    models: dict[str, dict[str, Any]] = {}
    # 视频多档聚合:同模型同 section 多档(分辨率)合并
    video_tiers_by_model: dict[str, list[dict[str, Any]]] = {}
    for record in records:
        model = str(record["model"])
        model_id = slug(model)
        entry = models.setdefault(model_id, new_model_entry(model_id, model, record))
        cost = cost_from_record(record)
        section = record.get("section") or ""
        subsection = record.get("subsection") or ""
        if subsection == "视频生成" and cost.get("basis") == "second":
            res = record.get("condition") or ""
            tier = {"resolution": res, "unit": "per_second", "tiers": {"price": cost.get("per_second")}}
            label = video_tier_label(record, res)
            if label and label != res:
                tier["label"] = label
            video_tiers_by_model.setdefault(model_id, []).append(tier)
            continue
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

    # 视频多档:取最低档作为代表档,写入 video_tiers(单位元/秒)
    for model_id, tiers in video_tiers_by_model.items():
        entry = models.get(model_id)
        if not entry:
            continue
        valid = []
        seen: set[tuple[str, str, str]] = set()
        for tier in tiers:
            if tier["tiers"].get("price") is None:
                continue
            key = (
                str(tier.get("label") or ""),
                str(tier.get("resolution") or ""),
                str(tier["tiers"].get("price")),
            )
            if key in seen:
                continue
            seen.add(key)
            valid.append(tier)
        if not valid:
            continue
        prices_list = [t["tiers"]["price"] for t in valid]
        representative = min(prices_list)
        entry["cost"] = {
            "input": representative,
            "output": representative,
            "video_tiers": valid,
            "basis": "multi_tier_video",
        }
        entry["metadata"]["pricing"].append({
            "section": "视频生成",
            "cost": entry["cost"],
        })

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
