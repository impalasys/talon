#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

CONFIG_ARG="${1:-}"
if [[ -n "${CONFIG_ARG}" ]]; then
  export TALON_CF_DEV_CONFIG_YAML="${CONFIG_ARG}"
  export TALON_CF_PROD_CONFIG_YAML="${CONFIG_ARG}"
else
  export TALON_CF_DEV_CONFIG_YAML="${TALON_CF_DEV_CONFIG_YAML:-${TALON_CF_CONFIG_YAML:-${SCRIPT_DIR}/dev/talon.yaml}}"
  export TALON_CF_PROD_CONFIG_YAML="${TALON_CF_PROD_CONFIG_YAML:-${TALON_CF_CONFIG_YAML:-${SCRIPT_DIR}/talon.yaml}}"
fi
export TALON_CF_DEV_WRANGLER="${TALON_CF_DEV_WRANGLER:-${SCRIPT_DIR}/dev/wrangler.jsonc}"
export TALON_CF_PROD_WRANGLER="${TALON_CF_PROD_WRANGLER:-${SCRIPT_DIR}/worker/wrangler.jsonc}"

export TALON_CF_WORKER_NAME="${TALON_CF_WORKER_NAME:-talon-cloudflare}"
export TALON_CF_COMPATIBILITY_DATE="${TALON_CF_COMPATIBILITY_DATE:-2026-06-13}"
export TALON_CF_DEV_SCHEDULER_AUTH_TOKEN="${TALON_CF_DEV_SCHEDULER_AUTH_TOKEN:-cloudflare-local-scheduler-token}"

export TALON_CF_GATEWAY_CONTAINER_COUNT="${TALON_CF_GATEWAY_CONTAINER_COUNT:-1}"
export TALON_CF_WORKER_CONTAINER_COUNT="${TALON_CF_WORKER_CONTAINER_COUNT:-1}"

export TALON_CF_D1_BINDING="${TALON_CF_D1_BINDING:-TALON_D1}"
export TALON_CF_D1_DATABASE_NAME="${TALON_CF_D1_DATABASE_NAME:-talon-control-plane}"
export TALON_CF_D1_DATABASE_ID="${TALON_CF_D1_DATABASE_ID:-00000000-0000-0000-0000-000000000000}"

export TALON_CF_R2_BINDING="${TALON_CF_R2_BINDING:-TALON_R2}"
export TALON_CF_R2_BUCKET_NAME="${TALON_CF_R2_BUCKET_NAME:-talon-objects}"

export TALON_CF_SESSION_DISPATCH_QUEUE="${TALON_CF_SESSION_DISPATCH_QUEUE:-talon-session-dispatch}"
export TALON_CF_RESOURCE_LIFECYCLE_QUEUE="${TALON_CF_RESOURCE_LIFECYCLE_QUEUE:-talon-resource-lifecycle}"
export TALON_CF_SESSION_CONTROL_QUEUE="${TALON_CF_SESSION_CONTROL_QUEUE:-talon-session-control}"

export TALON_CF_SESSION_DISPATCH_BINDING="${TALON_CF_SESSION_DISPATCH_BINDING:-SESSION_DISPATCH_QUEUE}"
export TALON_CF_RESOURCE_LIFECYCLE_BINDING="${TALON_CF_RESOURCE_LIFECYCLE_BINDING:-RESOURCE_LIFECYCLE_QUEUE}"
export TALON_CF_SESSION_CONTROL_BINDING="${TALON_CF_SESSION_CONTROL_BINDING:-SESSION_CONTROL_QUEUE}"

export TALON_CF_DEV_MAIN="${TALON_CF_DEV_MAIN:-../worker/src/index.ts}"
export TALON_CF_PROD_MAIN="${TALON_CF_PROD_MAIN:-src/index.ts}"

export TALON_CF_DEV_EXTERNAL_CONTAINERS="${TALON_CF_DEV_EXTERNAL_CONTAINERS:-true}"
export TALON_CF_DEV_GATEWAY_URL="${TALON_CF_DEV_GATEWAY_URL:-http://gateway:50052}"
export TALON_CF_DEV_GATEWAY_GRPC_URL="${TALON_CF_DEV_GATEWAY_GRPC_URL:-http://gateway:50051}"
export TALON_CF_DEV_WORKER_URL="${TALON_CF_DEV_WORKER_URL:-http://worker:8081}"

export TALON_CF_DEV_RUNTIME_IMAGE="${TALON_CF_DEV_RUNTIME_IMAGE:-../../../dockerfiles/oss-runtime.Dockerfile}"
export TALON_CF_DEV_RUNTIME_BUILD_CONTEXT="${TALON_CF_DEV_RUNTIME_BUILD_CONTEXT:-../../../}"

export TALON_CF_PROD_IMAGE_TAG="${TALON_CF_PROD_IMAGE_TAG:-latest}"
export TALON_CF_PROD_RUNTIME_IMAGE="${TALON_CF_PROD_RUNTIME_IMAGE:-ghcr.io/impalasys/talon-runtime:${TALON_CF_PROD_IMAGE_TAG}}"

for config_path in "${TALON_CF_DEV_CONFIG_YAML}" "${TALON_CF_PROD_CONFIG_YAML}"; do
  if [[ ! -f "${config_path}" ]]; then
    echo "Talon Cloudflare config not found: ${config_path}" >&2
    exit 1
  fi
done

python3 - <<'PY'
import json
import os
import re
from pathlib import Path

dev_config_yaml = Path(os.environ["TALON_CF_DEV_CONFIG_YAML"]).read_text()
prod_config_yaml = Path(os.environ["TALON_CF_PROD_CONFIG_YAML"]).read_text()

def env(name: str) -> str:
    return os.environ[name]

def config_env_keys(config_yaml: str) -> list[str]:
    keys: list[str] = []
    lines = config_yaml.splitlines()
    for index, line in enumerate(lines):
        if line.strip() != "source: env":
            continue
        source_indent = len(line) - len(line.lstrip())
        for next_line in lines[index + 1:]:
            if not next_line.strip():
                continue
            next_indent = len(next_line) - len(next_line.lstrip())
            if next_indent < source_indent:
                break
            match = re.match(r"\s*key:\s*[\"']?([A-Za-z_][A-Za-z0-9_]*)[\"']?\s*$", next_line)
            if match:
                keys.append(match.group(1))
                break
    return sorted(set(keys))

def durable_object_bindings(include_container_classes: bool) -> list[dict]:
    bindings = [
        {"name": "SCHEDULE_SHARD", "class_name": "ScheduleShard"},
        {"name": "SESSION_STREAMS", "class_name": "SessionStreamShard"},
    ]
    if include_container_classes:
        bindings = [
            {"name": "GATEWAY_CONTAINER", "class_name": "GatewayContainer"},
            {"name": "WORKER_CONTAINER", "class_name": "WorkerContainer"},
            *bindings,
        ]
    return bindings

def durable_object_classes(include_container_classes: bool) -> list[str]:
    classes = ["ScheduleShard", "SessionStreamShard"]
    if include_container_classes:
        classes = ["GatewayContainer", "WorkerContainer", "EnvoyContainer", *classes]
    return classes

def base_config(
    main: str,
    config_yaml: str,
    scheduler_auth_token: str | None,
    include_container_classes: bool,
) -> dict:
    vars = {
        "TALON_CONFIG_INLINE_YAML": config_yaml,
        "TALON_GATEWAY_CONTAINER_COUNT": env("TALON_CF_GATEWAY_CONTAINER_COUNT"),
        "TALON_WORKER_CONTAINER_COUNT": env("TALON_CF_WORKER_CONTAINER_COUNT"),
    }
    if scheduler_auth_token:
        vars["TALON_SCHEDULER_AUTH_TOKEN"] = scheduler_auth_token

    return {
        "$schema": "https://workers.cloudflare.com/schema/wrangler.json",
        "name": env("TALON_CF_WORKER_NAME"),
        "main": main,
        "compatibility_date": env("TALON_CF_COMPATIBILITY_DATE"),
        "compatibility_flags": ["nodejs_compat"],
        "vars": vars,
        "d1_databases": [
            {
                "binding": env("TALON_CF_D1_BINDING"),
                "database_name": env("TALON_CF_D1_DATABASE_NAME"),
                "database_id": env("TALON_CF_D1_DATABASE_ID"),
            }
        ],
        "r2_buckets": [
            {
                "binding": env("TALON_CF_R2_BINDING"),
                "bucket_name": env("TALON_CF_R2_BUCKET_NAME"),
            }
        ],
        "queues": {
            "producers": [
                {
                    "binding": env("TALON_CF_SESSION_DISPATCH_BINDING"),
                    "queue": env("TALON_CF_SESSION_DISPATCH_QUEUE"),
                },
                {
                    "binding": env("TALON_CF_RESOURCE_LIFECYCLE_BINDING"),
                    "queue": env("TALON_CF_RESOURCE_LIFECYCLE_QUEUE"),
                },
                {
                    "binding": env("TALON_CF_SESSION_CONTROL_BINDING"),
                    "queue": env("TALON_CF_SESSION_CONTROL_QUEUE"),
                },
            ],
            "consumers": [
                {"queue": env("TALON_CF_SESSION_DISPATCH_QUEUE"), "max_batch_size": 10},
                {"queue": env("TALON_CF_RESOURCE_LIFECYCLE_QUEUE"), "max_batch_size": 10},
                {"queue": env("TALON_CF_SESSION_CONTROL_QUEUE"), "max_batch_size": 10},
            ],
        },
        "durable_objects": {
            "bindings": durable_object_bindings(include_container_classes)
        },
        "migrations": [
            {
                "tag": "v1",
                "new_sqlite_classes": durable_object_classes(include_container_classes),
            }
        ],
    }

def write_json(path: str, config: dict) -> None:
    target = Path(path)
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(json.dumps(config, indent=2) + "\n")
    print(f"wrote {target}")

dev_external_containers = env("TALON_CF_DEV_EXTERNAL_CONTAINERS").lower() == "true"

dev = base_config(
    env("TALON_CF_DEV_MAIN"),
    dev_config_yaml,
    env("TALON_CF_DEV_SCHEDULER_AUTH_TOKEN"),
    not dev_external_containers,
)
dev["vars"]["TALON_CF_DEV_EXTERNAL_CONTAINERS"] = str(dev_external_containers).lower()
dev["vars"]["TALON_CF_DEV_GATEWAY_URL"] = env("TALON_CF_DEV_GATEWAY_URL")
dev["vars"]["TALON_CF_DEV_GATEWAY_GRPC_URL"] = env("TALON_CF_DEV_GATEWAY_GRPC_URL")
dev["vars"]["TALON_CF_DEV_WORKER_URL"] = env("TALON_CF_DEV_WORKER_URL")
for key in config_env_keys(dev_config_yaml):
    dev["vars"].setdefault(key, os.environ.get(key, "local-cloudflare-e2e"))
if not dev_external_containers:
    dev["containers"] = [
        {
            "class_name": "GatewayContainer",
            "max_instances": 1,
            "image": env("TALON_CF_DEV_RUNTIME_IMAGE"),
            "image_build_context": env("TALON_CF_DEV_RUNTIME_BUILD_CONTEXT"),
        },
        {
            "class_name": "WorkerContainer",
            "max_instances": 1,
            "image": env("TALON_CF_DEV_RUNTIME_IMAGE"),
            "image_build_context": env("TALON_CF_DEV_RUNTIME_BUILD_CONTEXT"),
        },
    ]

prod = base_config(env("TALON_CF_PROD_MAIN"), prod_config_yaml, None, True)
prod["containers"] = [
    {
        "class_name": "GatewayContainer",
        "max_instances": 1,
        "image": env("TALON_CF_PROD_RUNTIME_IMAGE"),
    },
    {
        "class_name": "WorkerContainer",
        "max_instances": 1,
        "image": env("TALON_CF_PROD_RUNTIME_IMAGE"),
    },
]

write_json(env("TALON_CF_DEV_WRANGLER"), dev)
write_json(env("TALON_CF_PROD_WRANGLER"), prod)
PY
