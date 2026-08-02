#!/usr/bin/env python3
"""Generate provider-specific Talon model catalogs.

The provider APIs are intentionally queried outside of Talon startup.  The
resulting YAML files are checked-in build inputs, so a temporary provider API
failure cannot make a running Talon instance depend on the network.

Examples:

  OPENAI_API_KEY=... python3 scripts/generate_model_catalogs.py --provider openai
  python3 scripts/generate_model_catalogs.py --all --output-dir models

Provider model APIs do not share a metadata contract.  This script normalizes
the fields Talon currently consumes and skips fields a provider does not
actually advertise.  Set PROVIDER_MODELS_URL to supply an endpoint for a
provider whose API is deployment-specific.
"""

from __future__ import annotations

import argparse
import html
import json
import os
import re
import sys
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_OUTPUT_DIR = ROOT / "models"


class CatalogError(RuntimeError):
    """A provider catalog could not be fetched or normalized."""


def first(mapping: dict[str, Any], *names: str) -> Any:
    for name in names:
        value = mapping.get(name)
        if value is not None:
            return value
    return None


def number(value: Any) -> int | float | None:
    if value is None or isinstance(value, bool):
        return None
    try:
        parsed = float(value)
    except (TypeError, ValueError):
        return None
    if parsed <= 0:
        return None
    return int(parsed) if parsed.is_integer() else parsed


def dollars_per_million(value: Any, *, per_token: bool = False) -> int | float | None:
    parsed = number(value)
    if parsed is None:
        return None
    return parsed * 1_000_000 if per_token else parsed


def model_items(payload: Any) -> list[dict[str, Any]]:
    if isinstance(payload, list):
        return [item for item in payload if isinstance(item, dict)]
    if not isinstance(payload, dict):
        raise CatalogError("model API returned neither an object nor a list")
    for key in ("data", "models", "items"):
        value = payload.get(key)
        if isinstance(value, list):
            return [item for item in value if isinstance(item, dict)]
    raise CatalogError("model API response has no data/models/items array")


@dataclass(frozen=True)
class Provider:
    name: str
    api_key_env: tuple[str, ...]
    url: str | None
    parser: Callable[[str, dict[str, Any]], dict[str, Any] | None]
    auth: str = "bearer"
    notes: str = ""

    def effective_url(self) -> str | None:
        return os.environ.get(f"{self.name.upper()}_MODELS_URL", self.url)

    def api_key(self) -> str | None:
        for env_name in self.api_key_env:
            if os.environ.get(env_name):
                return os.environ[env_name]
        return None


def generic_parser(provider: str, item: dict[str, Any]) -> dict[str, Any] | None:
    item_type = item.get("type")
    if item_type in {"image", "embedding", "moderation", "rerank"}:
        return None
    methods = item.get("supportedGenerationMethods")
    if isinstance(methods, list) and methods and not any(
        method in {"generateContent", "chat", "completion"} for method in methods
    ):
        return None
    model_id = first(item, "id", "name", "model")
    if not isinstance(model_id, str) or not model_id:
        return None
    record: dict[str, Any] = {"provider": provider}
    context = number(
        first(
            item,
            "contextLength",
            "context_length",
            "context_size",
            "inputTokenLimit",
            "max_context_length",
            "context_window",
        )
    )
    maximum = number(
        first(
            item,
            "maxOutputTokens",
            "max_output_tokens",
            "max_completion_tokens",
            "outputTokenLimit",
        )
    )
    top_provider = item.get("top_provider")
    if maximum is None and isinstance(top_provider, dict):
        maximum = number(top_provider.get("max_completion_tokens"))
    if context is not None:
        record["contextWindowTokens"] = context
    if maximum is not None:
        record["maxOutputTokens"] = maximum
    return {"id": model_id, "record": record}


def openrouter_parser(provider: str, item: dict[str, Any]) -> dict[str, Any] | None:
    result = generic_parser(provider, item)
    if result is None:
        return None
    pricing = item.get("pricing")
    if isinstance(pricing, dict):
        for source, destination in (
            ("prompt", "inputCostPerMillionTokens"),
            ("completion", "outputCostPerMillionTokens"),
            ("input_cache_read", "cacheReadCostPerMillionTokens"),
        ):
            value = dollars_per_million(pricing.get(source), per_token=True)
            if value is not None:
                result["record"][destination] = value
    return result


def novita_parser(provider: str, item: dict[str, Any]) -> dict[str, Any] | None:
    if not novita_supports_tool_calling(item):
        return None
    result = generic_parser(provider, item)
    if result is None:
        return None
    # Novita reports prices in ten-thousandths of a USD per million tokens:
    # e.g. 1400 means $0.14/M.  Talon's catalog convention is USD/M.
    for source, destination in (
        ("input_token_price_per_m", "inputCostPerMillionTokens"),
        ("output_token_price_per_m", "outputCostPerMillionTokens"),
    ):
        value = number(item.get(source))
        if value is not None:
            result["record"][destination] = value / 10_000
    return result


def novita_supports_tool_calling(item: dict[str, Any]) -> bool:
    """Return true only when Novita explicitly advertises tool calling.

    Novita's catalog metadata has used both boolean capability fields and
    feature/capability lists.  Do not infer support from a model family: an
    unsupported model causes the OpenAI-compatible endpoint to reject the
    entire request when tools are present.
    """
    boolean_fields = (
        "function_calling",
        "functionCalling",
        "supports_function_calling",
        "supportsFunctionCalling",
        "tool_calling",
        "toolCalling",
        "supports_tool_calling",
        "supportsToolCalling",
    )
    for container in (item, item.get("capabilities"), item.get("features")):
        if not isinstance(container, dict):
            continue
        if any(container.get(field) is True for field in boolean_fields):
            return True

    tool_features = {
        "function_calling",
        "function-calling",
        "functioncalling",
        "tool_calling",
        "tool-calling",
        "toolcalling",
        "tools",
    }
    for field in ("capabilities", "features", "supported_features", "supportedFeatures"):
        value = item.get(field)
        if isinstance(value, list) and any(
            isinstance(feature, str) and feature.strip().lower() in tool_features
            for feature in value
        ):
            return True
    return False


def fireworks_parser(provider: str, item: dict[str, Any]) -> dict[str, Any] | None:
    if item.get("supportsServerless") is not True:
        return None
    model_name = str(item.get("name", "")).lower()
    if "embedding" in model_name or "rerank" in model_name:
        return None
    result = generic_parser(provider, item)
    if result is None:
        return None
    name = result["id"]
    if not name.startswith("accounts/"):
        account = os.environ.get("FIREWORKS_ACCOUNT_ID", "fireworks")
        name = f"accounts/{account}/models/{name}"
    result["id"] = name
    return result


def fireworks_pricing() -> dict[str, dict[str, Any]]:
    """Parse Fireworks' public standard serverless pricing table.

    The authenticated model API does not expose prices.  The pricing page is
    an HTML table, so only rows with an explicit model URL and three numeric
    standard-tier prices are accepted.  Fast/US variants are skipped because
    they use a different route or tier.
    """
    url = os.environ.get(
        "FIREWORKS_PRICING_URL", "https://docs.fireworks.ai/serverless/pricing"
    )
    request = urllib.request.Request(
        url,
        headers={"Accept": "text/html", "User-Agent": "talon-model-catalog-generator/1"},
    )
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            page = response.read().decode("utf-8", "replace")
    except (urllib.error.HTTPError, urllib.error.URLError) as error:
        raise CatalogError(f"pricing page unavailable: {error}") from error

    prices: dict[str, dict[str, Any]] = {}
    rows = re.findall(r"<tr>(.*?)</tr>", page, re.DOTALL)
    for row in rows:
        match = re.search(
            r'href="https://app\.fireworks\.ai/models/fireworks/([^"?]+)"', row
        )
        if match is None:
            continue
        label = html.unescape(re.sub(r"<[^>]+>", " ", row))
        if " Fast" in label or " US" in label:
            continue
        cells = re.findall(
            r'<td[^>]*data-numeric="true"[^>]*>\$([^<]+)</td>', row
        )
        if not cells or match.group(1) in prices:
            continue
        values = [number(part.replace("$", "").strip()) for part in cells[0].split("/")]
        if len(values) != 3 or any(value is None for value in values):
            continue
        prices[match.group(1)] = {
            "inputCostPerMillionTokens": values[0],
            "cacheReadCostPerMillionTokens": values[1],
            "outputCostPerMillionTokens": values[2],
        }
    return prices


def fireworks_pricing_for_items(items: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    """Return explicit prices plus Fireworks' documented base-model tiers."""
    prices = fireworks_pricing()
    for item in items:
        if not item.get("supportsServerless"):
            continue
        name = item.get("name")
        if not isinstance(name, str):
            continue
        model_id = name.rsplit("/", 1)[-1]
        if model_id in prices:
            continue
        details = item.get("baseModelDetails")
        if not isinstance(details, dict):
            continue
        model_type = str(details.get("modelType", "")).lower()
        if "embed" in model_type or "rerank" in model_type:
            continue
        parameter_count = number(details.get("parameterCount"))
        if parameter_count is None:
            continue
        parameter_count /= 1_000_000_000
        if details.get("moe"):
            rate = 0.50 if parameter_count <= 56 else 1.20 if parameter_count <= 176 else 0.90
        elif parameter_count < 4:
            rate = 0.10
        elif parameter_count <= 16:
            rate = 0.20
        else:
            rate = 0.90
        prices[model_id] = {
            "inputCostPerMillionTokens": rate,
            "outputCostPerMillionTokens": rate,
        }
    return prices


def fireworks_page_context(model_id: str) -> int | None:
    """Recover context for custom models whose API metadata reports zero."""
    url = f"https://fireworks.ai/models/fireworks/{urllib.parse.quote(model_id, safe='') }"
    request = urllib.request.Request(url, headers={"User-Agent": "talon-model-catalog-generator/1"})
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            page = response.read().decode("utf-8", "replace")
    except (urllib.error.HTTPError, urllib.error.URLError):
        return None
    match = re.search(
        r"Context Length</span>.*?>([0-9,.]+)\s*([kKmM])\s+tokens",
        page,
        re.DOTALL,
    )
    if match is None:
        return None
    multiplier = 1_000 if match.group(2).lower() == "k" else 1_000_000
    return int(float(match.group(1).replace(",", "")) * multiplier)


def google_parser(provider: str, item: dict[str, Any]) -> dict[str, Any] | None:
    result = generic_parser(provider, item)
    if result is None:
        return None
    # Generation requests use baseModelId rather than the resource name.
    base_id = item.get("baseModelId")
    if isinstance(base_id, str) and base_id:
        result["id"] = base_id.removeprefix("models/")
    elif result["id"].startswith("models/"):
        result["id"] = result["id"][len("models/") :]
    return result


def google_pricing() -> dict[str, dict[str, Any]]:
    """Parse unambiguous Standard paid-tier token prices from Google's page."""
    url = os.environ.get("GOOGLE_PRICING_URL", "https://ai.google.dev/gemini-api/docs/pricing")
    request = urllib.request.Request(url, headers={"User-Agent": "talon-model-catalog-generator/1"})
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            page = response.read().decode("utf-8", "replace")
    except (urllib.error.HTTPError, urllib.error.URLError) as error:
        raise CatalogError(f"pricing page unavailable: {error}") from error

    prices: dict[str, dict[str, Any]] = {}
    for block in re.split(r'<div class="models-section">', page)[1:]:
        model = re.search(r'<em><code[^>]*>([^<]+)</code></em>', block)
        if model is None:
            continue
        standard = re.search(r'<h3[^>]*>Standard</h3>(.*?</table>)', block, re.DOTALL)
        if standard is None:
            continue
        values: dict[str, Any] = {}
        for row in re.findall(r"<tr>(.*?)</tr>", standard.group(1), re.DOTALL):
            text = html.unescape(re.sub(r"<[^>]+>", " ", row))
            text = re.sub(r"\s+", " ", text).strip()
            match = re.search(r"(Input price|Output price|Context caching price).*?\$([0-9]+(?:\.[0-9]+)?)", text)
            if match is None or "Not available" in text:
                continue
            field = {
                "Input price": "inputCostPerMillionTokens",
                "Output price": "outputCostPerMillionTokens",
                "Context caching price": "cacheReadCostPerMillionTokens",
            }[match.group(1)]
            values[field] = number(match.group(2))
        if "inputCostPerMillionTokens" in values and "outputCostPerMillionTokens" in values:
            prices[model.group(1)] = values
    return prices


def xai_parser(provider: str, item: dict[str, Any]) -> dict[str, Any] | None:
    result = generic_parser(provider, item)
    if result is None:
        return None
    # xAI documents these fields as USD cents per 100 million tokens.  Convert
    # to Talon's USD per million-token units.
    for source, destination in (
        ("prompt_text_token_price", "inputCostPerMillionTokens"),
        ("completion_text_token_price", "outputCostPerMillionTokens"),
        ("cached_prompt_text_token_price", "cacheReadCostPerMillionTokens"),
    ):
        value = number(item.get(source))
        if value is not None:
            result["record"][destination] = value / 100
    return result


def anthropic_parser(provider: str, item: dict[str, Any]) -> dict[str, Any] | None:
    # Anthropic's Models API currently exposes identity/lifecycle metadata but
    # not context or pricing limits.
    return generic_parser(provider, item)


PROVIDERS: dict[str, Provider] = {
    "anthropic": Provider(
        "anthropic", ("ANTHROPIC_API_KEY",), "https://api.anthropic.com/v1/models", anthropic_parser, auth="anthropic"
    ),
    "baichuan": Provider(
        "baichuan", ("BAICHUAN_API_KEY",), None, generic_parser,
        notes="Baichuan does not publish a stable public LLM model-list URL; set BAICHUAN_MODELS_URL.",
    ),
    "deepseek": Provider(
        "deepseek", ("DEEPSEEK_API_KEY",), "https://api.deepseek.com/models", generic_parser
    ),
    "fireworks": Provider(
        "fireworks", ("FIREWORKS_API_KEY",), "https://api.fireworks.ai/v1/accounts/{account}/models?filter=supports_serverless%3Dtrue&pageSize=200", fireworks_parser,
        notes="Uses the Fireworks management API; set FIREWORKS_ACCOUNT_ID when needed.",
    ),
    "google": Provider(
        "google", ("GEMINI_API_KEY", "GOOGLE_API_KEY"), "https://generativelanguage.googleapis.com/v1beta/models", google_parser, auth="google"
    ),
    "groq": Provider(
        "groq", ("GROQ_API_KEY",), "https://api.groq.com/openai/v1/models", generic_parser
    ),
    "meta": Provider(
        "meta", ("META_API_KEY",), None, generic_parser,
        notes="Meta does not expose a hosted public model-list API; set META_MODELS_URL for a configured route.",
    ),
    "minimax": Provider(
        "minimax", ("MINIMAX_API_KEY",), None, generic_parser,
        notes="MiniMax's OpenAI-compatible route does not consistently expose /models; set MINIMAX_MODELS_URL.",
    ),
    "mistral": Provider(
        "mistral", ("MISTRAL_API_KEY",), "https://api.mistral.ai/v1/models", generic_parser
    ),
    "moonshot": Provider(
        "moonshot", ("MOONSHOT_API_KEY",), "https://api.moonshot.ai/v1/models", generic_parser
    ),
    "novita": Provider(
        "novita", ("NOVITA_API_KEY",), "https://api.novita.ai/openai/v1/models", novita_parser
    ),
    "openai": Provider(
        "openai", ("OPENAI_API_KEY",), "https://api.openai.com/v1/models", generic_parser
    ),
    "openrouter": Provider(
        "openrouter", ("OPENROUTER_API_KEY",), "https://openrouter.ai/api/v1/models", openrouter_parser
    ),
    "qwen": Provider(
        "qwen", ("DASHSCOPE_API_KEY", "QWEN_API_KEY"), None, generic_parser,
        notes="DashScope model discovery is region/deployment-specific; set QWEN_MODELS_URL.",
    ),
    "siliconflow": Provider(
        "siliconflow", ("SILICONFLOW_API_KEY",), "https://api.siliconflow.cn/v1/models", generic_parser
    ),
    "together": Provider(
        "together", ("TOGETHER_API_KEY",), "https://api.together.xyz/v1/models", generic_parser
    ),
    "volcengine": Provider(
        "volcengine", ("VOLCENGINE_API_KEY",), None, generic_parser,
        notes="Volcengine model availability is endpoint/account scoped; set VOLCENGINE_MODELS_URL.",
    ),
    "xai": Provider(
        "xai", ("XAI_API_KEY",), "https://api.x.ai/v1/language-models", xai_parser
    ),
    "zhipu": Provider(
        "zhipu", ("ZHIPU_API_KEY",), None, generic_parser,
        notes="Zhipu does not document a stable public model-list URL; set ZHIPU_MODELS_URL.",
    ),
}


def add_query(url: str, name: str, value: str) -> str:
    parts = urllib.parse.urlsplit(url)
    query = urllib.parse.parse_qsl(parts.query, keep_blank_values=True)
    query.append((name, value))
    return urllib.parse.urlunsplit(parts._replace(query=urllib.parse.urlencode(query)))


def set_query(url: str, name: str, value: str) -> str:
    """Set one query parameter, replacing any value from an earlier page."""
    parts = urllib.parse.urlsplit(url)
    query = [(key, item) for key, item in urllib.parse.parse_qsl(parts.query, keep_blank_values=True) if key != name]
    query.append((name, value))
    return urllib.parse.urlunsplit(parts._replace(query=urllib.parse.urlencode(query)))


def fetch(provider: Provider) -> list[dict[str, Any]]:
    url = provider.effective_url()
    if url is None:
        raise CatalogError(provider.notes or "no model-list endpoint configured")
    if "{account}" in url:
        account = os.environ.get("FIREWORKS_ACCOUNT_ID")
        if not account:
            raise CatalogError("FIREWORKS_ACCOUNT_ID is required for the management API")
        url = url.format(account=urllib.parse.quote(account, safe=""))
    api_key = provider.api_key()
    if not api_key:
        raise CatalogError(f"missing one of: {', '.join(provider.api_key_env)}")
    headers = {"Accept": "application/json", "User-Agent": "talon-model-catalog-generator/1"}
    if provider.auth == "anthropic":
        headers.update({"x-api-key": api_key, "anthropic-version": "2023-06-01"})
    elif provider.auth == "google":
        url = add_query(url, "key", api_key)
    else:
        headers["Authorization"] = f"Bearer {api_key}"

    all_items: list[dict[str, Any]] = []
    while True:
        request = urllib.request.Request(url, headers=headers)
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                payload = json.load(response)
        except urllib.error.HTTPError as error:
            body = error.read().decode("utf-8", "replace")[:500]
            raise CatalogError(f"HTTP {error.code}: {body}") from error
        except urllib.error.URLError as error:
            raise CatalogError(str(error.reason)) from error
        except json.JSONDecodeError as error:
            raise CatalogError(f"invalid JSON response: {error}") from error

        all_items.extend(model_items(payload))
        if not isinstance(payload, dict):
            break
        page_token = payload.get("nextPageToken") or payload.get("next_page_token")
        if not page_token:
            break
        url = set_query(url, "pageToken", str(page_token))
    return all_items


def yaml_string(value: str) -> str:
    return json.dumps(value, ensure_ascii=False)


def write_yaml(provider: Provider, records: list[dict[str, Any]], output: Path, source: str) -> None:
    output.parent.mkdir(parents=True, exist_ok=True)
    lines = [
        "# Generated by scripts/generate_model_catalogs.py; do not edit by hand.",
        f"# Source: {source}",
        "models:" if records else "models: {}",
    ]
    for record in sorted(records, key=lambda item: item["id"]):
        key = f"{provider.name}/{record['id']}"
        lines.append(f"  {yaml_string(key)}:")
        for field in (
            "provider",
            "contextWindowTokens",
            "maxOutputTokens",
            "inputCostPerMillionTokens",
            "outputCostPerMillionTokens",
            "cacheReadCostPerMillionTokens",
        ):
            if field in record["record"]:
                lines.append(f"    {field}: {record['record'][field]}")
    output.write_text("\n".join(lines) + "\n", encoding="utf-8")


def parse_catalog(path: Path) -> dict[str, list[dict[str, Any]]]:
    """Read Talon's deliberately small model-catalog YAML subset.

    This bootstrap path avoids adding PyYAML just to split the checked-in
    catalog. API-generated refreshes do not use this parser.
    """
    providers: dict[str, list[dict[str, Any]]] = {}
    current_key: str | None = None
    current: dict[str, Any] | None = None

    def flush() -> None:
        if current is None:
            return
        provider = current["record"].get("provider")
        if not isinstance(provider, str) or provider not in PROVIDERS:
            return
        model_id = current["id"]
        prefix = f"{provider}/"
        if model_id.startswith(prefix):
            model_id = model_id[len(prefix) :]
        providers.setdefault(provider, []).append(
            {"id": model_id, "record": dict(current["record"])}
        )

    for raw_line in path.read_text(encoding="utf-8").splitlines():
        if raw_line.startswith("  ") and not raw_line.startswith("    ") and raw_line.endswith(":"):
            flush()
            current_key = raw_line[2:-1].strip().strip('"')
            current = {"id": current_key, "record": {}}
            continue
        if current is None or not raw_line.startswith("    ") or ":" not in raw_line:
            continue
        field, raw_value = raw_line.strip().split(":", 1)
        raw_value = raw_value.strip()
        if raw_value:
            try:
                value: Any = json.loads(raw_value)
            except json.JSONDecodeError:
                value = raw_value.strip('"')
            current["record"][field] = value
    flush()
    return providers


def bootstrap_catalog(path: Path, output_dir: Path) -> int:
    provider_records = parse_catalog(path)
    for provider_name, provider in PROVIDERS.items():
        write_yaml(
            provider,
            provider_records.get(provider_name, []),
            output_dir / f"{provider_name}.yaml",
            f"bootstrap from {path}",
        )
        print(f"{provider_name}: wrote {len(provider_records.get(provider_name, []))} bootstrap models")
    unknown = set(provider_records) - set(PROVIDERS)
    if unknown:
        print(f"warning: catalog contains unknown providers: {', '.join(sorted(unknown))}", file=sys.stderr)
    return 0


def generate(provider: Provider, output_dir: Path, write_empty: bool) -> bool:
    try:
        items = fetch(provider)
        records = []
        seen: set[str] = set()
        for item in items:
            parsed = provider.parser(provider.name, item)
            if parsed is None or parsed["id"] in seen:
                continue
            seen.add(parsed["id"])
            records.append(parsed)
        pricing_count = 0
        if provider.name == "fireworks":
            pricing = fireworks_pricing_for_items(items)
            for parsed in records:
                model_id = parsed["id"].rsplit("/", 1)[-1]
                if "contextWindowTokens" not in parsed["record"]:
                    context = fireworks_page_context(model_id)
                    if context is not None:
                        parsed["record"]["contextWindowTokens"] = context
                # Fireworks documents that, absent a model-specific lower
                # ceiling, serverless models may generate up to the full
                # context window.  This is a maximum, not the 2K default
                # request value.  Keep it explicit for Talon's compactor.
                if (
                    "contextWindowTokens" in parsed["record"]
                    and "maxOutputTokens" not in parsed["record"]
                ):
                    parsed["record"]["maxOutputTokens"] = parsed["record"][
                        "contextWindowTokens"
                    ]
                if model_id in pricing:
                    parsed["record"].update(pricing[model_id])
                    pricing_count += 1
        elif provider.name == "google":
            pricing = google_pricing()
            for parsed in records:
                model_id = parsed["id"]
                if model_id in pricing:
                    parsed["record"].update(pricing[model_id])
                    pricing_count += 1
        output = output_dir / f"{provider.name}.yaml"
        source = provider.effective_url() or "provider API"
        if provider.name == "fireworks":
            source += "; pricing: https://docs.fireworks.ai/serverless/pricing (explicit + base-model tiers)"
        elif provider.name == "google":
            source += "; pricing: https://ai.google.dev/gemini-api/docs/pricing (standard paid tier)"
        write_yaml(provider, records, output, source)
        suffix = f", {pricing_count} with pricing" if provider.name in {"fireworks", "google"} else ""
        print(f"{provider.name}: wrote {len(records)} models to {output}{suffix}")
        return True
    except CatalogError as error:
        print(f"{provider.name}: skipped: {error}", file=sys.stderr)
        if write_empty:
            write_yaml(provider, [], output_dir / f"{provider.name}.yaml", "unavailable")
        return False


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group()
    group.add_argument("--all", action="store_true", help="generate every configured provider catalog")
    group.add_argument("--provider", choices=sorted(PROVIDERS), help="generate one provider catalog")
    parser.add_argument("--output-dir", type=Path, default=DEFAULT_OUTPUT_DIR)
    parser.add_argument("--write-empty", action="store_true", help="write empty YAML for unavailable providers")
    parser.add_argument(
        "--from-catalog",
        type=Path,
        help="bootstrap provider files by splitting an existing Talon models.yaml",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.from_catalog:
        return bootstrap_catalog(args.from_catalog, args.output_dir)
    if not args.all and not args.provider:
        raise SystemExit("one of --all, --provider, or --from-catalog is required")
    providers = PROVIDERS.values() if args.all else [PROVIDERS[args.provider]]
    failures = sum(not generate(provider, args.output_dir, args.write_empty) for provider in providers)
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
