#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

export TALON_CF_CONFIG_YAML="${1:-${TALON_CF_CONFIG_YAML:-${SCRIPT_DIR}/talon.yaml}}"
export TALON_CF_DEV_WRANGLER="${TALON_CF_DEV_WRANGLER:-${SCRIPT_DIR}/dev/wrangler.jsonc}"
export TALON_CF_PROD_WRANGLER="${TALON_CF_PROD_WRANGLER:-${SCRIPT_DIR}/worker/wrangler.jsonc}"

export TALON_CF_WORKER_NAME="${TALON_CF_WORKER_NAME:-talon-cloudflare}"
export TALON_CF_COMPATIBILITY_DATE="${TALON_CF_COMPATIBILITY_DATE:-2026-06-13}"
export TALON_CF_SCHEDULER_AUTH_TOKEN="${TALON_CF_SCHEDULER_AUTH_TOKEN:-cloudflare-local-scheduler-token}"

export TALON_CF_GATEWAY_CONTAINER_COUNT="${TALON_CF_GATEWAY_CONTAINER_COUNT:-1}"
export TALON_CF_WORKER_CONTAINER_COUNT="${TALON_CF_WORKER_CONTAINER_COUNT:-1}"
export TALON_CF_ENVOY_CONTAINER_COUNT="${TALON_CF_ENVOY_CONTAINER_COUNT:-1}"

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

export TALON_CF_DEV_RUNTIME_IMAGE="${TALON_CF_DEV_RUNTIME_IMAGE:-../../../dockerfiles/oss-runtime.Dockerfile}"
export TALON_CF_DEV_RUNTIME_BUILD_CONTEXT="${TALON_CF_DEV_RUNTIME_BUILD_CONTEXT:-../../../}"
export TALON_CF_DEV_ENVOY_IMAGE="${TALON_CF_DEV_ENVOY_IMAGE:-../dockerfiles/cloudflare-envoy.Dockerfile}"
export TALON_CF_DEV_ENVOY_BUILD_CONTEXT="${TALON_CF_DEV_ENVOY_BUILD_CONTEXT:-../../../}"

export TALON_CF_PROD_RUNTIME_IMAGE="${TALON_CF_PROD_RUNTIME_IMAGE:-ghcr.io/impalasys/talon-runtime:latest}"
export TALON_CF_PROD_ENVOY_IMAGE="${TALON_CF_PROD_ENVOY_IMAGE:-ghcr.io/impalasys/talon-envoy-cloudflare:latest}"

if [[ ! -f "${TALON_CF_CONFIG_YAML}" ]]; then
  echo "Talon Cloudflare config not found: ${TALON_CF_CONFIG_YAML}" >&2
  exit 1
fi

python3 - <<'PY'
import json
import os
from pathlib import Path

config_yaml = Path(os.environ["TALON_CF_CONFIG_YAML"]).read_text()

def env(name: str) -> str:
    return os.environ[name]

def base_config(main: str) -> dict:
    return {
        "$schema": "https://workers.cloudflare.com/schema/wrangler.json",
        "name": env("TALON_CF_WORKER_NAME"),
        "main": main,
        "compatibility_date": env("TALON_CF_COMPATIBILITY_DATE"),
        "compatibility_flags": ["nodejs_compat"],
        "vars": {
            "TALON_CONFIG_INLINE_YAML": config_yaml,
            "TALON_SCHEDULER_AUTH_TOKEN": env("TALON_CF_SCHEDULER_AUTH_TOKEN"),
            "TALON_GATEWAY_CONTAINER_COUNT": env("TALON_CF_GATEWAY_CONTAINER_COUNT"),
            "TALON_WORKER_CONTAINER_COUNT": env("TALON_CF_WORKER_CONTAINER_COUNT"),
            "TALON_ENVOY_CONTAINER_COUNT": env("TALON_CF_ENVOY_CONTAINER_COUNT"),
        },
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
            "bindings": [
                {"name": "GATEWAY_CONTAINER", "class_name": "GatewayContainer"},
                {"name": "WORKER_CONTAINER", "class_name": "WorkerContainer"},
                {"name": "ENVOY_CONTAINER", "class_name": "EnvoyContainer"},
                {"name": "SCHEDULE_SHARD", "class_name": "ScheduleShard"},
            ]
        },
        "migrations": [
            {
                "tag": "v1",
                "new_sqlite_classes": [
                    "GatewayContainer",
                    "WorkerContainer",
                    "EnvoyContainer",
                    "ScheduleShard",
                ],
            }
        ],
    }

def write_json(path: str, config: dict) -> None:
    target = Path(path)
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(json.dumps(config, indent=2) + "\n")
    print(f"wrote {target}")

dev = base_config(env("TALON_CF_DEV_MAIN"))
dev["containers"] = [
    {
        "class_name": "GatewayContainer",
        "image": env("TALON_CF_DEV_RUNTIME_IMAGE"),
        "image_build_context": env("TALON_CF_DEV_RUNTIME_BUILD_CONTEXT"),
    },
    {
        "class_name": "WorkerContainer",
        "image": env("TALON_CF_DEV_RUNTIME_IMAGE"),
        "image_build_context": env("TALON_CF_DEV_RUNTIME_BUILD_CONTEXT"),
    },
    {
        "class_name": "EnvoyContainer",
        "image": env("TALON_CF_DEV_ENVOY_IMAGE"),
        "image_build_context": env("TALON_CF_DEV_ENVOY_BUILD_CONTEXT"),
    },
]

prod = base_config(env("TALON_CF_PROD_MAIN"))
prod["containers"] = [
    {
        "class_name": "GatewayContainer",
        "image": env("TALON_CF_PROD_RUNTIME_IMAGE"),
    },
    {
        "class_name": "WorkerContainer",
        "image": env("TALON_CF_PROD_RUNTIME_IMAGE"),
    },
    {
        "class_name": "EnvoyContainer",
        "image": env("TALON_CF_PROD_ENVOY_IMAGE"),
    },
]

write_json(env("TALON_CF_DEV_WRANGLER"), dev)
write_json(env("TALON_CF_PROD_WRANGLER"), prod)
PY
