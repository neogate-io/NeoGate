#!/usr/bin/env python3
"""Fetch Volcengine Ark / Doubao pricing from the public documentation API.

Volcengine's Ark pricing page renders rich-text tables from a public doc detail
API. The document stores table row zones, column zones, and cell zones separately;
cell zone IDs are built as "x" + row_id + "x" + column_id. This script reconstructs
the visible tables and emits normalized JSON while preserving the original row data.

No Volcengine account or API key is required.
"""

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

LIBRARY_ID = "82379"
DOCUMENT_ID = "1544106"
DEFAULT_SOURCE_URL = f"https://www.volcengine.com/docs/{LIBRARY_ID}/{DOCUMENT_ID}?lang=zh"
DEFAULT_API_URL = (
    "https://www.volcengine.com/api/doc/getDocDetail"
    f"?LibraryID={LIBRARY_ID}&DocumentID={DOCUMENT_ID}&newapi=1"
)
DEFAULT_OUTPUT = Path("frontend/public/model-pricing.json")
USER_AGENT = "Mozilla/5.0 (compatible; NeoGate pricing scraper; +https://github.com/neogate-io/NeoGate)"


class ScrapeError(RuntimeError):
    pass


def fetch_json(url: str) -> dict[str, Any]:
    request = urllib.request.Request(
        url,
        headers={
            "User-Agent": USER_AGENT,
            "Accept": "application/json, text/plain, */*",
            "Referer": DEFAULT_SOURCE_URL,
        },
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        charset = response.headers.get_content_charset() or "utf-8"
        return json.loads(response.read().decode(charset, errors="replace"))


def op_text(ops: list[dict[str, Any]]) -> str:
    parts: list[str] = []
    for op in ops:
        inserted = op.get("insert")
        if isinstance(inserted, str):
            if inserted == "*":
                continue
            parts.append(inserted)
    return re.sub(r"\n+", "\n", "".join(parts)).strip()


def zone_text(data: dict[str, Any], zone_id: str) -> str:
    zone = data.get(zone_id) or {}
    return op_text(zone.get("ops") or [])


def zone_ids(data: dict[str, Any], zone_id: str) -> list[str]:
    zone = data.get(zone_id) or {}
    ids: list[str] = []
    for op in zone.get("ops") or []:
        inserted = op.get("insert")
        if isinstance(inserted, dict) and inserted.get("id"):
            ids.append(str(inserted["id"]))
    return ids


def clean_key(text: str) -> str:
    compact = re.sub(r"\s+", "", text.replace("（", "(").replace("）", ")"))
    mapping = {
        "模型名称": "model",
        "模型": "model",
        "基础模型ID": "base_model",
        "精调模型对应的基础模型": "base_model",
        "条件千token": "condition",
        "条件(千token)": "condition",
        "条件": "condition",
        "计费方式": "billing_method",
        "输入(非音频)元/百万token": "input_text_cny_per_million_tokens",
        "输入（非音频）元/百万token": "input_text_cny_per_million_tokens",
        "输入(音频)元/百万token": "input_audio_cny_per_million_tokens",
        "输入（音频）元/百万token": "input_audio_cny_per_million_tokens",
        "缓存存储元/百万token/小时": "cache_storage_cny_per_million_tokens_hour",
        "缓存命中(非音频)元/百万token": "cache_hit_text_cny_per_million_tokens",
        "缓存命中（非音频）元/百万token": "cache_hit_text_cny_per_million_tokens",
        "缓存命中(音频)元/百万token": "cache_hit_audio_cny_per_million_tokens",
        "缓存命中（音频）元/百万token": "cache_hit_audio_cny_per_million_tokens",
        "输出元/百万token": "output_cny_per_million_tokens",
        "输入元/每10KTPM": "input_cny_per_10k_tpm",
        "输出元/每1KTPM": "output_cny_per_1k_tpm",
        "在线推理元/百万token": "online_inference_cny_per_million_tokens",
        "离线推理元/百万token": "offline_inference_cny_per_million_tokens",
        "单价元/张": "price_cny_per_image",
        "输出单价元/次": "output_cny_per_request",
        "文本输入元/百万token": "text_input_cny_per_million_tokens",
        "图片输入元/百万token": "image_input_cny_per_million_tokens",
        "LoRA精调元/百万token": "lora_tuning_cny_per_million_tokens",
        "全量精调元/百万token": "full_tuning_cny_per_million_tokens",
        "定价元/小时": "price_cny_per_hour",
        "定价元/个": "price_cny_per_unit",
        "价格元/千次": "price_cny_per_thousand_calls",
        "价格元/次": "price_cny_per_call",
        "价格": "price",
        "说明": "description",
        "服务项": "service_item",
        "机型": "unit_type",
        "算力规格": "compute_spec",
        "产物(3D文件)": "artifact",
    }
    if compact in mapping:
        return mapping[compact]
    key = re.sub(r"[^0-9A-Za-z\u4e00-\u9fff]+", "_", compact).strip("_")
    return key or "column"


def parse_decimal(value: Any) -> str | None:
    raw = "" if value is None else str(value).strip()
    if not raw or raw in {"-", "—", "暂不支持", "不支持"}:
        return None
    match = re.search(r"\d+(?:\.\d+)?", raw.replace(",", ""))
    if not match:
        return None
    try:
        return format(Decimal(match.group(0)).normalize(), "f")
    except InvalidOperation:
        return None


def normalize_price(value: Any, unit: str | None = None) -> dict[str, Any]:
    raw = "" if value is None else str(value).strip()
    return {"raw": raw, "amount_cny": parse_decimal(raw), "unit": unit}


def decimal_number(value: str | None) -> int | float | None:
    if value is None:
        return None
    decimal = Decimal(value)
    if decimal == decimal.to_integral_value():
        return int(decimal)
    return float(decimal)


def price_amount(price: dict[str, Any] | None) -> int | float | None:
    if not price:
        return None
    return decimal_number(price.get("amount_cny"))


def slug(value: str) -> str:
    return re.sub(r"[^0-9A-Za-z._-]+", "-", value.strip().lower()).strip("-")


def modalities_for_record(record: dict[str, Any]) -> dict[str, list[str]]:
    section = record.get("section") or ""
    model = record.get("model") or ""
    raw = record.get("raw") or {}
    inputs = ["text"]
    outputs = ["text"]
    if "视频" in section or "seedance" in model:
        inputs = ["text", "image", "video"]
        outputs = ["video"]
    elif "图片" in section or "seedream" in model:
        inputs = ["text", "image"]
        outputs = ["image"]
    elif "3D" in section or "seed3d" in model.lower() or raw.get("artifact"):
        inputs = ["text", "image"]
        outputs = ["3d"]
    elif "向量" in section or "embedding" in model:
        inputs = ["text", "image"]
        outputs = ["embedding"]
    return {"input": inputs, "output": outputs}


def new_model_entry(model_id: str, name: str, record: dict[str, Any]) -> dict[str, Any]:
    return {
        "id": model_id,
        "name": name,
        "description": "",
        "modalities": modalities_for_record(record),
        "open_weights": False,
        "metadata": {"currency": "CNY", "cost_unit": "1M tokens unless noted", "pricing": []},
    }


def merge_modalities(existing: dict[str, Any], record: dict[str, Any]) -> None:
    incoming = modalities_for_record(record)
    current = existing.setdefault("modalities", {"input": [], "output": []})
    for direction in ("input", "output"):
        merged = list(dict.fromkeys([*(current.get(direction) or []), *incoming[direction]]))
        current[direction] = merged


def cost_from_record(record: dict[str, Any]) -> dict[str, int | float]:
    prices = record.get("prices") or {}
    cost: dict[str, int | float] = {}
    mappings = (
        ("input_text_cny_per_million_tokens", "input"),
        ("output_cny_per_million_tokens", "output"),
        ("cache_hit_text_cny_per_million_tokens", "cache_read"),
        ("cache_storage_cny_per_million_tokens_hour", "cache_write"),
        ("online_inference_cny_per_million_tokens", "input"),
        ("text_input_cny_per_million_tokens", "input"),
        ("image_input_cny_per_million_tokens", "input_image"),
        ("price_cny_per_image", "output"),
        ("output_cny_per_request", "output"),
        ("price_cny_per_call", "output"),
    )
    for source, target in mappings:
        value = price_amount(prices.get(source))
        if value is not None and target not in cost:
            cost[target] = value
    if "input" in cost and "output" not in cost and record.get("section") not in {"向量模型"}:
        cost["output"] = cost["input"]
    return cost


def to_models_dev_payload(
    records: list[dict[str, Any]],
    tables: list[dict[str, Any]],
    source_url: str,
    api_url: str,
    result: dict[str, Any],
) -> dict[str, Any]:
    models: dict[str, dict[str, Any]] = {}
    for record in records:
        name = record.get("model")
        if not name:
            continue
        model_id = slug(str(name))
        entry = models.setdefault(model_id, new_model_entry(model_id, str(name), record))
        merge_modalities(entry, record)
        cost = cost_from_record(record)
        if cost and "cost" not in entry:
            entry["cost"] = cost
        entry["metadata"]["pricing"].append(
            {
                "section": record.get("section"),
                "subsection": record.get("subsection"),
                "category": record.get("category"),
                "condition": record.get("condition"),
                "cost": cost,
                "raw": record.get("raw"),
            }
        )

    return {
        "volcengine-ark": {
            "id": "volcengine-ark",
            "env": ["ARK_API_KEY"],
            "npm": "@ai-sdk/openai-compatible",
            "api": "https://ark.cn-beijing.volces.com/api/v3",
            "name": "火山方舟 / 豆包",
            "doc": source_url,
            "models": models,
            "metadata": {
                "currency": "CNY",
                "cost_unit": "1M tokens unless noted",
                "source_api_url": api_url,
                "document_id": result.get("DocumentID") or DOCUMENT_ID,
                "document_title": result.get("Title"),
                "document_updated_at": result.get("UpdatedTime"),
                "fetched_at": dt.datetime.now(dt.timezone.utc).isoformat(),
                "tables": tables,
            },
        }
    }


def current_headings(headings: list[tuple[int, str, str]], index: int) -> list[str]:
    selected: dict[str, str] = {}
    for pos, text, level in headings:
        if pos >= index:
            break
        selected[level] = text
        if level == "h1":
            selected.pop("h2", None)
            selected.pop("h3", None)
        elif level == "h2":
            selected.pop("h3", None)
    return [selected[level] for level in ("h1", "h2", "h3") if selected.get(level)]


def heading_refs(main_ops: list[dict[str, Any]]) -> list[tuple[int, str, str]]:
    headings: list[tuple[int, str, str]] = []
    pending: tuple[int, str] | None = None
    for index, op in enumerate(main_ops):
        attrs = op.get("attributes") or {}
        inserted = op.get("insert")
        if attrs.get("heading") and inserted == "*":
            pending = (index, str(attrs["heading"]))
            continue
        if pending and isinstance(inserted, str):
            text = inserted.strip()
            if text:
                headings.append((pending[0], text, pending[1]))
                pending = None
    return headings


def extract_table(data: dict[str, Any], ace_table: str) -> list[list[str]]:
    try:
        row_zone_id, col_zone_id = ace_table.split()[:2]
    except ValueError as exc:
        raise ScrapeError(f"invalid aceTable reference: {ace_table}") from exc

    row_ids = zone_ids(data, row_zone_id)
    col_ids = zone_ids(data, col_zone_id)
    rows: list[list[str]] = []
    for row_id in row_ids:
        row = [zone_text(data, f"x{row_id}x{col_id}") for col_id in col_ids]
        if any(cell.strip() for cell in row):
            rows.append(row)
    return rows


def table_records(data: dict[str, Any], table: list[list[str]], headings: list[str]) -> list[dict[str, Any]]:
    if len(table) < 2:
        return []
    headers = [clean_key(cell) for cell in table[0]]
    seen: dict[str, int] = {}
    unique_headers: list[str] = []
    for header in headers:
        count = seen.get(header, 0)
        seen[header] = count + 1
        unique_headers.append(header if count == 0 else f"{header}_{count + 1}")

    records: list[dict[str, Any]] = []
    current_model = ""
    for cells in table[1:]:
        cells = cells + [""] * (len(unique_headers) - len(cells))
        raw = {key: value.strip() for key, value in zip(unique_headers, cells)}
        model_key = "model" if "model" in raw else "base_model" if "base_model" in raw else None
        if model_key:
            if raw.get(model_key):
                current_model = raw[model_key]
            elif current_model:
                raw[model_key] = current_model
        if not any(raw.values()):
            continue

        prices = {
            key: normalize_price(value, price_unit_for_key(key))
            for key, value in raw.items()
            if is_price_key(key)
        }
        records.append(
            {
                "section": headings[0] if headings else None,
                "subsection": headings[1] if len(headings) > 1 else None,
                "category": headings[2] if len(headings) > 2 else None,
                "model": raw.get("model") or raw.get("base_model") or raw.get("service_item") or raw.get("unit_type"),
                "condition": raw.get("condition") or raw.get("billing_method"),
                "prices": prices,
                "raw": raw,
            }
        )
    return records


def is_price_key(key: str) -> bool:
    return "cny" in key or key == "price"


def price_unit_for_key(key: str) -> str | None:
    if "per_million_tokens_hour" in key:
        return "百万token/小时"
    if "per_million_tokens" in key:
        return "百万token"
    if "per_10k_tpm" in key:
        return "每10K TPM"
    if "per_1k_tpm" in key:
        return "每1K TPM"
    if "per_image" in key:
        return "张"
    if "per_request" in key or "per_call" in key:
        return "次"
    if "per_hour" in key:
        return "小时"
    if "per_unit" in key:
        return "个"
    if "per_thousand_calls" in key:
        return "千次"
    return None


def scrape(api_url: str, source_url: str) -> dict[str, Any]:
    response = fetch_json(api_url)
    result = response.get("Result") or {}
    content_raw = result.get("Content")
    if not content_raw:
        raise ScrapeError("Volcengine doc API returned no Content")
    content = json.loads(content_raw)
    data = content.get("data") or {}
    main_ops = (data.get("0") or {}).get("ops") or []
    headings = heading_refs(main_ops)

    tables: list[dict[str, Any]] = []
    records: list[dict[str, Any]] = []
    for index, op in enumerate(main_ops):
        attrs = op.get("attributes") or {}
        ace_table = attrs.get("aceTable")
        if not ace_table:
            continue
        context = current_headings(headings, index)
        table = extract_table(data, str(ace_table))
        if not table:
            continue
        parsed = table_records(data, table, context)
        tables.append(
            {
                "headings": context,
                "row_count": len(table),
                "column_count": len(table[0]) if table else 0,
                "headers": table[0] if table else [],
            }
        )
        records.extend(parsed)

    if not records:
        raise ScrapeError("extracted document but found no pricing records")

    return to_models_dev_payload(records, tables, source_url, api_url, result)


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
    parser = argparse.ArgumentParser(description="Fetch Volcengine Ark / Doubao public pricing into JSON")
    parser.add_argument("--api-url", default=DEFAULT_API_URL, help=f"doc API URL (default: {DEFAULT_API_URL})")
    parser.add_argument("--source-url", default=DEFAULT_SOURCE_URL, help=f"source doc URL (default: {DEFAULT_SOURCE_URL})")
    parser.add_argument("--output", default=str(DEFAULT_OUTPUT), help=f"merged JSON output path (default: {DEFAULT_OUTPUT})")
    parser.add_argument("--stdout", action="store_true", help="print JSON to stdout instead of writing a file")
    args = parser.parse_args()

    try:
        payload = scrape(args.api_url, args.source_url)
    except Exception as exc:  # noqa: BLE001 - CLI should return a readable failure
        print(f"error: {exc}", file=sys.stderr)
        return 1

    text = json.dumps(payload, ensure_ascii=False, indent=2) + "\n"
    if args.stdout:
        print(text, end="")
    else:
        output = Path(args.output)
        provider = payload["volcengine-ark"]
        table_count = len(provider["metadata"]["tables"])
        provider_count, model_count = update_provider_file(output, payload)
        print(
            f"updated volcengine-ark with {len(provider['models'])} models from {table_count} tables "
            f"in {output} ({provider_count} providers, {model_count} models total)"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
