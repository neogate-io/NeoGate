#!/usr/bin/env python3
"""Fetch BigModel (Zhipu AI) pricing data from the public pricing page.

The BigModel pricing page is a Vue SPA. The public HTML loads a hashed app JS
bundle, and the pricing tables are currently embedded in that bundle as a
locale/module object. This script extracts that object and emits a normalized
JSON file that keeps both raw display prices and machine-friendly numeric values.

No BigModel API key is required.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import re
import shutil
import subprocess
import sys
import tempfile
import urllib.request
from decimal import Decimal, InvalidOperation
from pathlib import Path
from typing import Any

DEFAULT_URL = "https://bigmodel.cn/pricing"
DEFAULT_OUTPUT = Path("frontend/public/model-pricing.json")
USER_AGENT = "Mozilla/5.0 (compatible; NeoGate pricing scraper; +https://github.com/neogate-io/NeoGate)"


class ScrapeError(RuntimeError):
    pass


def fetch_text(url: str) -> str:
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(request, timeout=30) as response:
        charset = response.headers.get_content_charset() or "utf-8"
        return response.read().decode(charset, errors="replace")


def find_app_bundle_url(page_url: str, html: str) -> str:
    matches = re.findall(r'<script[^>]+src=["\']([^"\']*/js/app\.[^"\']+\.js)["\']', html)
    if not matches:
        raise ScrapeError("could not find /js/app.<hash>.js in pricing page HTML")
    src = matches[-1]
    if src.startswith("http://") or src.startswith("https://"):
        return src
    origin = re.match(r"^(https?://[^/]+)", page_url)
    if not origin:
        raise ScrapeError(f"invalid page URL: {page_url}")
    return origin.group(1) + src


def extract_pricing_object_js(bundle: str) -> str:
    marker = 'productPrice:"产品价格",callPrice:"调用单价"'
    marker_index = bundle.find(marker)
    if marker_index < 0:
        raise ScrapeError("could not locate BigModel pricing locale object in app bundle")

    module_start = bundle.rfind('t["default"]=', 0, marker_index)
    if module_start < 0:
        raise ScrapeError("could not locate t[\"default\"] assignment before pricing object")

    object_start = bundle.find("{", module_start)
    if object_start < 0:
        raise ScrapeError("could not locate pricing object opening brace")

    depth = 0
    in_string: str | None = None
    escaped = False
    for index in range(object_start, len(bundle)):
        char = bundle[index]
        if in_string:
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == in_string:
                in_string = None
            continue
        if char in ('"', "'"):
            in_string = char
        elif char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return bundle[object_start : index + 1]

    raise ScrapeError("unterminated pricing object in app bundle")


def evaluate_js_object(js_object: str) -> dict[str, Any]:
    node = shutil.which("node")
    if not node:
        raise ScrapeError("node is required to evaluate BigModel's JavaScript pricing object")

    script = """
const obj = __OBJECT__;
process.stdout.write(JSON.stringify(obj));
""".replace("__OBJECT__", js_object)

    with tempfile.NamedTemporaryFile("w", suffix=".mjs", encoding="utf-8", delete=False) as handle:
        handle.write(script)
        script_path = Path(handle.name)
    try:
        result = subprocess.run(
            [node, str(script_path)],
            check=False,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=20,
        )
    finally:
        script_path.unlink(missing_ok=True)

    if result.returncode != 0:
        raise ScrapeError(f"node failed to evaluate pricing object: {result.stderr.strip()}")
    return json.loads(result.stdout)


def parse_decimal(text: Any) -> str | None:
    if text is None:
        return None
    raw = str(text).strip()
    if not raw or any(word in raw for word in ("免费", "限时免费", "不支持", "数十万", "百万", "千万", "十万")):
        return None
    match = re.search(r"\d+(?:\.\d+)?", raw.replace(",", ""))
    if not match:
        return None
    try:
        return format(Decimal(match.group(0)).normalize(), "f")
    except InvalidOperation:
        return None


def normalize_price(value: Any, unit: Any) -> dict[str, Any]:
    raw = None if value is None else str(value).strip()
    numeric = parse_decimal(raw)
    return {
        "raw": raw,
        "amount_cny": numeric,
        "unit": None if unit is None else str(unit).strip(),
    }


def decimal_number(value: str | None) -> int | float | None:
    if value is None:
        return None
    decimal = Decimal(value)
    if decimal == decimal.to_integral_value():
        return int(decimal)
    return float(decimal)


def price_per_million(price: dict[str, Any]) -> int | float | None:
    amount = price.get("amount_cny")
    if amount is None:
        return None
    decimal = Decimal(amount)
    if "千" in str(price.get("unit") or ""):
        decimal *= Decimal(1000)
    return decimal_number(format(decimal.normalize(), "f"))


def slug(value: str) -> str:
    return re.sub(r"[^0-9A-Za-z._-]+", "-", value.strip().lower()).strip("-")


def context_limit(value: Any) -> int | None:
    if value is None:
        return None
    text = str(value).upper().strip()
    match = re.search(r"\d+(?:\.\d+)?", text)
    if not match:
        return None
    number = Decimal(match.group(0))
    if "M" in text:
        number *= Decimal(1_000_000)
    elif "K" in text:
        number *= Decimal(1_000)
    return int(number)


def new_model_entry(model_id: str, name: str, description: str | None = None) -> dict[str, Any]:
    return {
        "id": model_id,
        "name": name,
        "description": description or "",
        "modalities": {"input": ["text"], "output": ["text"]},
        "open_weights": False,
        "metadata": {"currency": "CNY", "cost_unit": "1M tokens", "pricing": []},
    }


def to_models_dev_payload(records: list[dict[str, Any]], page_url: str, app_bundle_url: str) -> dict[str, Any]:
    models: dict[str, dict[str, Any]] = {}
    for record in records:
        name = record.get("name")
        if not name:
            continue
        model_id = slug(str(name))
        entry = models.setdefault(model_id, new_model_entry(model_id, str(name), record.get("description")))
        context = context_limit(record.get("context") or (record.get("context_or_condition") or [None])[0])
        if context and "limit" not in entry:
            entry["limit"] = {"context": context}

        cost: dict[str, int | float] = {}
        pricing_item = {
            "section": record.get("section"),
            "category": record.get("category"),
            "condition": record.get("context_or_condition") or record.get("context"),
            "type": record.get("pricing_type"),
            "raw": record.get("raw"),
        }
        if record.get("pricing_type") == "input_output":
            for source, target in (
                ("input_price", "input"),
                ("output_price", "output"),
                ("cache_hit_price", "cache_read"),
                ("cache_storage_price", "cache_write"),
            ):
                value = price_per_million(record[source])
                if value is not None:
                    cost[target] = value
        else:
            value = price_per_million(record["price"])
            if value is not None:
                cost["input"] = value
                cost["output"] = value
            batch = price_per_million(record["batch_price"])
            if batch is not None:
                pricing_item["batch_cost"] = batch
        if cost and "cost" not in entry:
            entry["cost"] = cost
        pricing_item["cost"] = cost
        entry["metadata"]["pricing"].append(pricing_item)

    return {
        "bigmodel": {
            "id": "bigmodel",
            "env": ["BIGMODEL_API_KEY"],
            "npm": "@ai-sdk/openai-compatible",
            "api": "https://open.bigmodel.cn/api/paas/v4",
            "name": "智谱 BigModel",
            "doc": page_url,
            "models": models,
            "metadata": {
                "currency": "CNY",
                "cost_unit": "1M tokens",
                "source_bundle_url": app_bundle_url,
                "fetched_at": dt.datetime.now(dt.timezone.utc).isoformat(),
            },
        }
    }


def iter_latest_tables(pricing: dict[str, Any]) -> list[dict[str, Any]]:
    latest = pricing.get("latestModel") or {}
    groups = []
    for section_key, history in (("latest", latest.get("model") or []), ("history", latest.get("historyModel") or [])):
        for section in history:
            table = section.get("table") or {}
            for row in table.get("tbody") or []:
                groups.append(
                    {
                        "section": section_key,
                        "category": section.get("name") or section.get("label"),
                        "name": row.get("label"),
                        "description": row.get("desc"),
                        "context": row.get("Context") or row.get("Resolution"),
                        "pricing_type": "single_price",
                        "price": normalize_price(row.get("costPrice"), row.get("unit")),
                        "batch_price": normalize_price(row.get("batchPrice"), row.get("unit")),
                        "raw": row,
                    }
                )
    return groups


def iter_flagship_tables(pricing: dict[str, Any]) -> list[dict[str, Any]]:
    new_model = pricing.get("newModel") or {}
    records = []
    for category in new_model.get("model") or []:
        current_name = None
        for row in category.get("modelList") or []:
            if row.get("name"):
                current_name = row.get("name")
            name = row.get("name") or current_name
            if not name:
                continue
            units = {
                "input_output": category.get("unit1"),
                "cache_storage": category.get("unit2"),
                "cache_hit": category.get("unit2"),
                "decode": category.get("unit4"),
            }
            records.append(
                {
                    "section": "flagship",
                    "category": category.get("modelName"),
                    "name": name,
                    "description": row.get("intro") or category.get("desc"),
                    "context_or_condition": row.get("upDownText"),
                    "pricing_type": "input_output",
                    "input_price": normalize_price((row.get("inPrice") or [None])[0], units["input_output"]),
                    "output_price": normalize_price((row.get("outPrice") or [None])[0], units["input_output"]),
                    "cache_storage_price": normalize_price(row.get("storage"), units["cache_storage"]),
                    "cache_hit_price": normalize_price((row.get("hit") or [None])[0], units["cache_hit"]),
                    "decode_speed": None if row.get("decode") is None else str(row.get("decode")),
                    "decode_unit": units["decode"],
                    "raw": row,
                }
            )
    return records


def scrape(page_url: str) -> dict[str, Any]:
    html = fetch_text(page_url)
    app_bundle_url = find_app_bundle_url(page_url, html)
    bundle = fetch_text(app_bundle_url)
    pricing = evaluate_js_object(extract_pricing_object_js(bundle))
    records = iter_flagship_tables(pricing) + iter_latest_tables(pricing)
    if not records:
        raise ScrapeError("extracted pricing object but found no model records")
    return to_models_dev_payload(records, page_url, app_bundle_url)


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
    parser = argparse.ArgumentParser(description="Fetch BigModel public pricing into JSON")
    parser.add_argument("--url", default=DEFAULT_URL, help=f"pricing page URL (default: {DEFAULT_URL})")
    parser.add_argument("--output", default=str(DEFAULT_OUTPUT), help=f"merged JSON output path (default: {DEFAULT_OUTPUT})")
    parser.add_argument("--stdout", action="store_true", help="print JSON to stdout instead of writing a file")
    args = parser.parse_args()

    try:
        payload = scrape(args.url)
    except Exception as exc:  # noqa: BLE001 - CLI should return a readable failure
        print(f"error: {exc}", file=sys.stderr)
        return 1

    text = json.dumps(payload, ensure_ascii=False, indent=2) + "\n"
    if args.stdout:
        print(text, end="")
    else:
        output = Path(args.output)
        provider = payload["bigmodel"]
        provider_count, model_count = update_provider_file(output, payload)
        print(f"updated bigmodel with {len(provider['models'])} models in {output} ({provider_count} providers, {model_count} models total)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
