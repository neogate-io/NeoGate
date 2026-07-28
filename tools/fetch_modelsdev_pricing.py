#!/usr/bin/env python3
"""Fetch OpenAI / Anthropic USD pricing from models.dev, convert to CNY at the
live exchange rate, and merge into frontend/public/model-pricing.json.

models.dev publishes model prices in USD per 1M tokens. This script pulls the
specified providers (default: openai, anthropic), converts every numeric cost
field to CNY using a live USD->CNY exchange rate fetched from the European
Central Bank reference rates (frankfurter.dev), and writes the result into the
local CNY pricing catalog so it can be consumed by the NeoGate backend in CNY
billing mode.

The exchange rate source is free and requires no API key:
  - Primary: https://api.frankfurter.dev  (ECB reference rates)
  - Fallback: https://open.er-api.com      (ExchangeRate-API free tier)

No API key is required. All network access uses Python stdlib (urllib).
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import sys
import urllib.request
from pathlib import Path
from typing import Any

DEFAULT_PROVIDERS = "openai,anthropic"
DEFAULT_MODELS_DEV_URL = "https://models.dev/api.json"
DEFAULT_OUTPUT = Path("frontend/public/model-pricing.json")
USER_AGENT = "Mozilla/5.0 (compatible; NeoGate pricing scraper; +https://github.com/neogate-io/NeoGate)"

# USD->CNY exchange rate sources (free, no API key).
FX_SOURCE_FRANKFURTER = "frankfurter (ECB reference rates)"
FX_SOURCE_ER_API = "open.er-api.com (ExchangeRate-API)"
FX_URL_FRANKFURTER = "https://api.frankfurter.dev/v1/latest?base=USD&symbols=CNY"
FX_URL_ER_API = "https://open.er-api.com/v6/latest/USD"

# Cost fields that carry a per-1M-token USD price and must be converted.
COST_FIELDS = ("input", "output", "cache_read", "cache_write")
# Rounding precision: 6 decimal places is enough for micro-unit accuracy
# (backend multiplies by 1_000_000 to store micros).
ROUND_DIGITS = 2


class ScrapeError(RuntimeError):
    pass


def fetch_json(url: str) -> Any:
    """Fetch a URL and parse JSON, following redirects."""
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(req, timeout=30) as resp:
        return json.loads(resp.read().decode("utf-8"))


def fetch_fx_rate() -> tuple[float, str, str]:
    """Fetch the live USD->CNY exchange rate.

    Returns (rate, source_label, rate_date). Tries the ECB-backed
    frankfurter.dev first, then falls back to open.er-api.com.
    """
    # Primary: frankfurter.dev (ECB reference rates)
    try:
        data = fetch_json(FX_URL_FRANKFURTER)
        rate = data["rates"]["CNY"]
        if not isinstance(rate, (int, float)) or rate <= 0:
            raise ScrapeError(f"invalid CNY rate from frankfurter: {rate!r}")
        rate_date = data.get("date", "")
        return float(rate), FX_SOURCE_FRANKFURTER, rate_date
    except Exception as exc:
        print(f"warning: frankfurter FX fetch failed ({exc}), falling back to er-api", file=sys.stderr)

    # Fallback: open.er-api.com
    try:
        data = fetch_json(FX_URL_ER_API)
        rate = data["rates"]["CNY"]
        if not isinstance(rate, (int, float)) or rate <= 0:
            raise ScrapeError(f"invalid CNY rate from er-api: {rate!r}")
        rate_date = data.get("time_last_update_utc", "")
        return float(rate), FX_SOURCE_ER_API, rate_date
    except Exception as exc:
        raise ScrapeError(f"all FX sources unavailable: {exc}") from exc


def convert_cost_value(value: Any, fx_rate: float) -> Any:
    """Convert nested models.dev cost values while preserving their shape."""
    if isinstance(value, dict):
        return {
            key: (
                round(item * fx_rate, ROUND_DIGITS)
                if key in COST_FIELDS
                and isinstance(item, (int, float))
                and not isinstance(item, bool)
                else convert_cost_value(item, fx_rate)
            )
            for key, item in value.items()
        }
    if isinstance(value, list):
        return [convert_cost_value(item, fx_rate) for item in value]
    return value


def convert_cost(cost: dict[str, Any], fx_rate: float) -> dict[str, Any]:
    """Convert USD cost fields to CNY, including nested pricing tiers."""
    return convert_cost_value(cost, fx_rate)


def convert_provider(
    provider_key: str,
    provider_data: dict[str, Any],
    fx_rate: float,
) -> dict[str, Any]:
    """Convert all model costs in a provider from USD to CNY."""
    models = provider_data.get("models", {})
    converted_models: dict[str, Any] = {}
    skipped = 0

    for model_id, model in models.items():
        model = dict(model)  # shallow copy so we don't mutate upstream data
        cost = model.get("cost")
        if not isinstance(cost, dict) or not any(
            isinstance(cost.get(f), (int, float)) and not isinstance(cost.get(f), bool)
            for f in COST_FIELDS
        ):
            skipped += 1
            continue
        model["cost"] = convert_cost(cost, fx_rate)
        converted_models[model_id] = model

    result = dict(provider_data)
    result["models"] = converted_models
    result["metadata"] = {
        "currency": "CNY",
        "cost_unit": "1M tokens",
        "source": "models.dev (USD converted to CNY)",
        "fx_rate": fx_rate,
        "fx_date": dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%d"),
        "fetched_at": dt.datetime.now(dt.timezone.utc).isoformat(),
    }
    return result, skipped


def update_provider_file(
    output: Path, provider_payloads: dict[str, Any]
) -> tuple[int, int]:
    """Merge converted providers into the existing model-pricing.json."""
    merged: dict[str, Any] = {}
    if output.exists():
        try:
            existing = json.loads(output.read_text(encoding="utf-8"))
        except json.JSONDecodeError as exc:
            raise ScrapeError(f"invalid existing JSON in {output}: {exc}") from exc
        if not isinstance(existing, dict):
            raise ScrapeError(f"existing {output} must contain a provider map object")
        merged.update(existing)
    merged.update(provider_payloads)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(
        json.dumps(dict(sorted(merged.items())), ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    model_count = sum(
        len(provider.get("models", {}))
        for provider in merged.values()
        if isinstance(provider, dict)
    )
    return len(merged), model_count


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Fetch OpenAI/Anthropic pricing from models.dev, convert USD->CNY at live rate, merge into model-pricing.json"
    )
    parser.add_argument(
        "--providers",
        default=DEFAULT_PROVIDERS,
        help=f"comma-separated models.dev provider keys to convert (default: {DEFAULT_PROVIDERS})",
    )
    parser.add_argument(
        "--models-dev-url",
        default=DEFAULT_MODELS_DEV_URL,
        help=f"models.dev API URL (default: {DEFAULT_MODELS_DEV_URL})",
    )
    parser.add_argument(
        "--output",
        default=str(DEFAULT_OUTPUT),
        help=f"merged JSON output path (default: {DEFAULT_OUTPUT})",
    )
    parser.add_argument(
        "--fx-rate",
        type=float,
        default=None,
        help="override the live FX rate (use this to pin a specific USD->CNY rate)",
    )
    parser.add_argument(
        "--stdout",
        action="store_true",
        help="print converted provider JSON to stdout instead of writing a file",
    )
    args = parser.parse_args()

    provider_keys = [k.strip() for k in args.providers.split(",") if k.strip()]
    if not provider_keys:
        print("error: at least one provider must be specified", file=sys.stderr)
        return 1

    # Determine exchange rate
    if args.fx_rate is not None:
        if args.fx_rate <= 0:
            print("error: --fx-rate must be a positive number", file=sys.stderr)
            return 1
        fx_rate = args.fx_rate
        fx_source = "manual override"
        fx_date = dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%d")
        print(f"using manual FX rate: 1 USD = {fx_rate} CNY")
    else:
        try:
            fx_rate, fx_source, fx_date = fetch_fx_rate()
        except ScrapeError as exc:
            print(f"error: {exc}", file=sys.stderr)
            return 1
        print(f"live FX rate: 1 USD = {fx_rate} CNY (source: {fx_source}, date: {fx_date})")

    # Fetch models.dev data
    try:
        models_dev = fetch_json(args.models_dev_url)
    except Exception as exc:
        print(f"error: failed to fetch models.dev pricing: {exc}", file=sys.stderr)
        return 1

    # Convert each requested provider
    payloads: dict[str, Any] = {}
    total_skipped = 0
    for key in provider_keys:
        provider_data = models_dev.get(key)
        if not isinstance(provider_data, dict):
            print(f"error: provider '{key}' not found in models.dev data", file=sys.stderr)
            return 1
        converted, skipped = convert_provider(key, provider_data, fx_rate)
        payloads[key] = converted
        model_count = len(converted.get("models", {}))
        print(f"converted {key}: {model_count} models ({skipped} skipped, no USD cost)")
        total_skipped += skipped

    if args.stdout:
        print(json.dumps(payloads, ensure_ascii=False, indent=2))
    else:
        output = Path(args.output)
        provider_count, total_models = update_provider_file(output, payloads)
        print(
            f"updated {len(provider_keys)} provider(s) in {output} "
            f"({provider_count} providers, {total_models} models total, {total_skipped} skipped)"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
