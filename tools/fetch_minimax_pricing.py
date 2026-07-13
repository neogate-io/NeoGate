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
    # 取首个数字,避免折扣文本(如 "~~4.20~~ 2.10" 取 4.20、限免 "1.0(限免)" 取 1.0)
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


def clean_cell(cell: str) -> str:
    cell = re.sub(r"<br\s*/?>", " ", cell)
    cell = re.sub(r"<[^>]+>", " ", cell)
    cell = cell.replace("\\*", "*")
    cell = re.sub(r"\*\*|__|`", "", cell)
    cell = re.sub(r"\s+", " ", cell).strip()
    return cell


def slug(value: str) -> str:
    return re.sub(r"[^0-9A-Za-z._-]+", "-", value.strip().lower()).strip("-")


# 各 section 的默认计费口径与 modality
SECTION_BASIS = {
    "语言模型": ("token", {"input": ["text"], "output": ["text"]}),
    "语音": ("per_10k_token", {"input": ["text"], "output": ["audio"]}),
    "视频": ("multi_tier_video", {"input": ["text", "image"], "output": ["video"]}),
    "音乐": ("call", {"input": ["text"], "output": ["audio"]}),
    "图像": ("image", {"input": ["text", "image"], "output": ["image"]}),
    "MCP": ("call", {"input": ["text", "image"], "output": ["text"]}),
}


def modalities_for_record(section: str) -> dict[str, list[str]]:
    return SECTION_BASIS.get(section, (None, {"input": ["text"], "output": ["text"]}))[1]


# 单位关键词 -> 价格字段名(非 token 口径)
UNIT_PRICE_KEY = {
    "元/张": "per_image",
    "元/视频": "per_call",
    "元/首": "per_call",
    "元/次": "per_call",
    "元/音色": "per_call",
    "元/万字符": "per_10k_token",
}


def detect_unit(header_cells: list[str]) -> str | None:
    """从表头识别计费单位,返回 '百万tokens' 或 '元/张' 等。"""
    joined = " ".join(header_cells)
    for unit in UNIT_PRICE_KEY:
        if unit in joined:
            return unit
    if "百万 tokens" in joined or "百万tokens" in joined:
        return "百万tokens"
    return None


def model_name(raw: str) -> str | None:
    match = re.search(r"\bMiniMax-[0-9A-Za-z._-]+\b", raw)
    if match:
        return match.group(0)
    # 视频/音乐/图像/MCP 模型名不带 MiniMax- 前缀
    for pattern in (r"\bMiniMax-[0-9A-Za-z._-]+\b", r"\bimage-[\w-]+\b", r"\bAPI-[\w-]+\b"):
        match = re.search(pattern, raw)
        if match:
            return match.group(0)
    # speech/Music/Hailuo 系列
    match = re.search(r"\b(speech-[\w.-]+|Music-[\w.+]+|MiniMax-Hailuo-[\w.-]+)", raw)
    if match:
        return match.group(0)
    return None


def condition_from_model_cell(raw: str) -> str | None:
    for pattern in (r"[≤<>].*?tokens\\?\*?", r"输入 tokens\\?\*?", r"\d+P\s*\d+s", r"图生视频.*", r"文生视频.*"):
        match = re.search(pattern, raw)
        if match:
            return match.group(0).replace("\\*", "*").strip()
    return None


def descriptive_condition(row: list[str], name_col: int, price_col: int) -> str | None:
    for idx, cell in enumerate(row):
        if idx in {name_col, price_col}:
            continue
        value = cell.strip()
        if value:
            return value
    return None


def video_resolution_from_condition(condition: str | None) -> str:
    if not condition:
        return ""
    match = re.search(r"\b\d{3,4}P\b", condition, flags=re.IGNORECASE)
    return match.group(0).upper() if match else ""


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
        unit = detect_unit(header)
        if unit is None:
            table = []
            return
        # 数据行从分隔行之后开始
        data_rows = table[2:] if len(table) > 2 and re.match(r"^:?-+", table[1][0]) else table[1:]
        for row in data_rows:
            if len(row) < 3:
                continue
            # 模型名可能在任意列(语言模型在 row[0],语音/MCP 在 row[1])
            name = None
            name_col = 0
            for idx, cell in enumerate(row):
                name = model_name(cell)
                if name:
                    name_col = idx
                    break
            if not name:
                continue
            # 确定价格列(最后一列)
            price_col = len(row) - 1
            price_raw = row[price_col]
            # 视频/音乐/图像/MCP 表:功能/说明列(含分辨率/时长)作为 condition
            if unit == "百万tokens":
                condition = condition_from_model_cell(row[0]) or None
            else:
                condition = descriptive_condition(row, name_col, price_col)
            record = {
                "section": section,
                "model": name,
                "condition": condition,
                "unit": unit,
                "prices": {},
                "raw": {"model": row[0], "price": price_raw},
            }
            if unit == "百万tokens":
                # 语言模型:输入/输出/缓存读/缓存写
                record["prices"]["input_cny_per_million_tokens"] = {"raw": row[1], "amount_cny": parse_decimal(row[1]), "unit": unit}
                record["prices"]["output_cny_per_million_tokens"] = {"raw": row[2], "amount_cny": parse_decimal(row[2]), "unit": unit}
                record["raw"]["input"] = row[1]
                record["raw"]["output"] = row[2]
                if len(row) > 3:
                    record["prices"]["cache_read_cny_per_million_tokens"] = {"raw": row[3], "amount_cny": parse_decimal(row[3]), "unit": unit}
                    record["raw"]["cache_read"] = row[3]
                if len(row) > 4:
                    record["prices"]["cache_write_cny_per_million_tokens"] = {"raw": row[4], "amount_cny": parse_decimal(row[4]), "unit": unit}
                    record["raw"]["cache_write"] = row[4]
            else:
                # 非 token 口径:单价在最后一列
                key = UNIT_PRICE_KEY[unit]
                record["prices"][key] = {"raw": price_raw, "amount_cny": parse_decimal(price_raw), "unit": unit}
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

    # 视频模型多档聚合:同模型多行合并为 multi_tier_video
    return records


def cost_from_record(record: dict[str, Any]) -> dict[str, Any]:
    prices = record["prices"]
    cost: dict[str, Any] = {}
    section = record.get("section") or ""
    unit = record.get("unit")

    if unit == "百万tokens":
        for source, target in (
            ("input_cny_per_million_tokens", "input"),
            ("output_cny_per_million_tokens", "output"),
            ("cache_read_cny_per_million_tokens", "cache_read"),
            ("cache_write_cny_per_million_tokens", "cache_write"),
        ):
            value = amount((prices.get(source) or {}).get("amount_cny"))
            if value is not None:
                cost[target] = value
        cost["basis"] = "token"
    elif unit in UNIT_PRICE_KEY:
        key = UNIT_PRICE_KEY[unit]
        value = amount((prices.get(key) or {}).get("amount_cny"))
        if value is not None:
            if key == "per_image":
                cost["per_image"] = value
                cost["basis"] = "image"
            elif key == "per_10k_token":
                cost["per_10k_token_input"] = value
                cost["per_10k_token_output"] = value
                cost["basis"] = "per_10k_token"
            else:  # per_call (元/视频/元/首/元/次/元/音色)
                cost["per_call"] = value
                cost["basis"] = "call"
    return cost


def new_model_entry(model_id: str, name: str, record: dict[str, Any]) -> dict[str, Any]:
    return {
        "id": model_id,
        "name": name,
        "description": "",
        "modalities": modalities_for_record(record.get("section") or ""),
        "open_weights": False,
        "metadata": {"currency": "CNY", "cost_unit": "1M tokens unless noted", "pricing": []},
    }


def to_provider_payload(records: list[dict[str, Any]], source_url: str) -> dict[str, Any]:
    models: dict[str, Any] = {}
    # 视频模型同模型多档先聚合
    video_tiers_by_model: dict[str, list[dict[str, Any]]] = {}
    for record in records:
        name = record["model"]
        model_id = slug(name)
        entry = models.setdefault(model_id, new_model_entry(model_id, name, record))
        cost = cost_from_record(record)
        if record.get("section") == "视频" and cost.get("basis") == "call":
            # 视频按次计费但有多档(分辨率/时长),收集档位
            condition = record.get("condition") or ""
            tier = {
                "resolution": video_resolution_from_condition(condition),
                "unit": "per_video",
                "tiers": {"price": cost.get("per_call")},
            }
            if condition and condition != tier["resolution"]:
                tier["label"] = condition
            video_tiers_by_model.setdefault(model_id, []).append(tier)
            continue
        if cost and "cost" not in entry:
            entry["cost"] = cost
        entry["metadata"]["pricing"].append({
            "section": record.get("section"),
            "condition": record.get("condition"),
            "cost": cost,
            "raw": record.get("raw"),
        })
    # 视频多档:取最低档作为代表档,写入 video_tiers
    for model_id, tiers in video_tiers_by_model.items():
        entry = models.get(model_id)
        if not entry:
            continue
        valid = [t for t in tiers if t["tiers"].get("price") is not None]
        if not valid:
            continue
        prices = [t["tiers"]["price"] for t in valid]
        representative = min(prices)
        entry["cost"] = {
            "input": representative,
            "output": representative,
            "video_tiers": valid,
            "basis": "multi_tier_video",
        }
        entry["metadata"]["pricing"].append({
            "section": "视频",
            "cost": entry["cost"],
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
