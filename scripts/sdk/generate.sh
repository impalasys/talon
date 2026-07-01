#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

V1_PROTO_SRCS=(
  proto/talon/v1/auth.proto
  proto/talon/v1/channels.proto
  proto/talon/v1/connectors.proto
  proto/talon/v1/knowledge.proto
  proto/talon/v1/namespaces.proto
  proto/talon/v1/resources.proto
  proto/talon/v1/search.proto
  proto/talon/v1/sessions.proto
  proto/talon/v1/workflows.proto
)

PROTO_SRCS=(
  proto/config.proto
  proto/resources/common.proto
  proto/resources/agents.proto
  proto/resources/mcp.proto
  proto/resources/knowledge.proto
  proto/resources/namespaces.proto
  proto/resources/channels.proto
  proto/resources/routing.proto
  proto/resources/connectors.proto
  proto/resources/schedules.proto
  proto/resources/workflows.proto
  proto/resources/deployments.proto
  proto/resources/sandboxes.proto
  proto/resources/sessions.proto
  proto/resources/skills.proto
  proto/resources/usage.proto
  proto/resources/workers.proto
  proto/resources/resource.proto
  proto/harness/llm.proto
  proto/data/api_keys.proto
  proto/data/connectors.proto
  proto/data/data.proto
  proto/data/search.proto
  proto/data/session_submission.proto
  proto/data/session_journal_entry.proto
  proto/events.proto
  "${V1_PROTO_SRCS[@]}"
)

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64)
    PROTOC_PLATFORM="osx-aarch_64"
    JAVA_GRPC_PLATFORM="osx-aarch_64"
    ;;
  Darwin-x86_64)
    PROTOC_PLATFORM="osx-x86_64"
    JAVA_GRPC_PLATFORM="osx-x86_64"
    ;;
  Linux-x86_64)
    PROTOC_PLATFORM="linux-x86_64"
    JAVA_GRPC_PLATFORM="linux-x86_64"
    ;;
  Linux-aarch64|Linux-arm64)
    PROTOC_PLATFORM="linux-aarch_64"
    JAVA_GRPC_PLATFORM="linux-aarch_64"
    ;;
  *)
    echo "Unsupported platform for SDK codegen: $(uname -s)-$(uname -m)" >&2
    exit 1
    ;;
esac

PROTOC_VERSION="${PROTOC_VERSION:-34.1}"
PROTOC_ROOT="$ROOT/.tools/protoc/protoc-${PROTOC_VERSION}-${PROTOC_PLATFORM}"
PROTOC="$PROTOC_ROOT/bin/protoc"
if [[ ! -x "$PROTOC" ]]; then
  mkdir -p "$PROTOC_ROOT"
  PROTOC_ZIP="$ROOT/.tools/protoc/protoc-${PROTOC_VERSION}-${PROTOC_PLATFORM}.zip"
  curl -fL -o "$PROTOC_ZIP" \
    "https://github.com/protocolbuffers/protobuf/releases/download/v${PROTOC_VERSION}/protoc-${PROTOC_VERSION}-${PROTOC_PLATFORM}.zip"
  unzip -q -o "$PROTOC_ZIP" -d "$PROTOC_ROOT"
fi

GO_MODULE="github.com/impalasys/talon/sdk/go/talon-client"
GO_OPTS=(
  "--go_opt=module=${GO_MODULE}"
  "--go-grpc_opt=module=${GO_MODULE}"
  "--go_opt=Mproto/config.proto=${GO_MODULE}/talon/config"
  "--go_opt=Mproto/resources/common.proto=${GO_MODULE}/talon/resources"
  "--go_opt=Mproto/resources/agents.proto=${GO_MODULE}/talon/resources"
  "--go_opt=Mproto/resources/mcp.proto=${GO_MODULE}/talon/resources"
  "--go_opt=Mproto/resources/knowledge.proto=${GO_MODULE}/talon/resources"
  "--go_opt=Mproto/resources/namespaces.proto=${GO_MODULE}/talon/resources"
  "--go_opt=Mproto/resources/channels.proto=${GO_MODULE}/talon/resources"
  "--go_opt=Mproto/resources/routing.proto=${GO_MODULE}/talon/resources"
  "--go_opt=Mproto/resources/connectors.proto=${GO_MODULE}/talon/resources"
  "--go_opt=Mproto/resources/schedules.proto=${GO_MODULE}/talon/resources"
  "--go_opt=Mproto/resources/workflows.proto=${GO_MODULE}/talon/resources"
  "--go_opt=Mproto/resources/deployments.proto=${GO_MODULE}/talon/resources"
  "--go_opt=Mproto/resources/sandboxes.proto=${GO_MODULE}/talon/resources"
  "--go_opt=Mproto/resources/sessions.proto=${GO_MODULE}/talon/resources"
  "--go_opt=Mproto/resources/skills.proto=${GO_MODULE}/talon/resources"
  "--go_opt=Mproto/resources/usage.proto=${GO_MODULE}/talon/resources"
  "--go_opt=Mproto/resources/workers.proto=${GO_MODULE}/talon/resources"
  "--go_opt=Mproto/resources/resource.proto=${GO_MODULE}/talon/resources"
  "--go_opt=Mproto/harness/llm.proto=${GO_MODULE}/talon/harness"
  "--go_opt=Mproto/data/api_keys.proto=${GO_MODULE}/talon/data"
  "--go_opt=Mproto/data/connectors.proto=${GO_MODULE}/talon/data"
  "--go_opt=Mproto/data/data.proto=${GO_MODULE}/talon/data"
  "--go_opt=Mproto/data/search.proto=${GO_MODULE}/talon/data"
  "--go_opt=Mproto/data/session_submission.proto=${GO_MODULE}/talon/data"
  "--go_opt=Mproto/data/session_journal_entry.proto=${GO_MODULE}/talon/data"
  "--go_opt=Mproto/events.proto=${GO_MODULE}/talon/events"
  "--go_opt=Mproto/talon/v1/auth.proto=${GO_MODULE}/talon/v1"
  "--go_opt=Mproto/talon/v1/channels.proto=${GO_MODULE}/talon/v1"
  "--go_opt=Mproto/talon/v1/connectors.proto=${GO_MODULE}/talon/v1"
  "--go_opt=Mproto/talon/v1/knowledge.proto=${GO_MODULE}/talon/v1"
  "--go_opt=Mproto/talon/v1/namespaces.proto=${GO_MODULE}/talon/v1"
  "--go_opt=Mproto/talon/v1/resources.proto=${GO_MODULE}/talon/v1"
  "--go_opt=Mproto/talon/v1/search.proto=${GO_MODULE}/talon/v1"
  "--go_opt=Mproto/talon/v1/sessions.proto=${GO_MODULE}/talon/v1"
  "--go_opt=Mproto/talon/v1/workflows.proto=${GO_MODULE}/talon/v1"
  "--go-grpc_opt=Mproto/config.proto=${GO_MODULE}/talon/config"
  "--go-grpc_opt=Mproto/resources/common.proto=${GO_MODULE}/talon/resources"
  "--go-grpc_opt=Mproto/resources/agents.proto=${GO_MODULE}/talon/resources"
  "--go-grpc_opt=Mproto/resources/mcp.proto=${GO_MODULE}/talon/resources"
  "--go-grpc_opt=Mproto/resources/knowledge.proto=${GO_MODULE}/talon/resources"
  "--go-grpc_opt=Mproto/resources/namespaces.proto=${GO_MODULE}/talon/resources"
  "--go-grpc_opt=Mproto/resources/channels.proto=${GO_MODULE}/talon/resources"
  "--go-grpc_opt=Mproto/resources/routing.proto=${GO_MODULE}/talon/resources"
  "--go-grpc_opt=Mproto/resources/connectors.proto=${GO_MODULE}/talon/resources"
  "--go-grpc_opt=Mproto/resources/schedules.proto=${GO_MODULE}/talon/resources"
  "--go-grpc_opt=Mproto/resources/workflows.proto=${GO_MODULE}/talon/resources"
  "--go-grpc_opt=Mproto/resources/deployments.proto=${GO_MODULE}/talon/resources"
  "--go-grpc_opt=Mproto/resources/sandboxes.proto=${GO_MODULE}/talon/resources"
  "--go-grpc_opt=Mproto/resources/sessions.proto=${GO_MODULE}/talon/resources"
  "--go-grpc_opt=Mproto/resources/skills.proto=${GO_MODULE}/talon/resources"
  "--go-grpc_opt=Mproto/resources/usage.proto=${GO_MODULE}/talon/resources"
  "--go-grpc_opt=Mproto/resources/workers.proto=${GO_MODULE}/talon/resources"
  "--go-grpc_opt=Mproto/resources/resource.proto=${GO_MODULE}/talon/resources"
  "--go-grpc_opt=Mproto/harness/llm.proto=${GO_MODULE}/talon/harness"
  "--go-grpc_opt=Mproto/data/api_keys.proto=${GO_MODULE}/talon/data"
  "--go-grpc_opt=Mproto/data/connectors.proto=${GO_MODULE}/talon/data"
  "--go-grpc_opt=Mproto/data/data.proto=${GO_MODULE}/talon/data"
  "--go-grpc_opt=Mproto/data/search.proto=${GO_MODULE}/talon/data"
  "--go-grpc_opt=Mproto/data/session_submission.proto=${GO_MODULE}/talon/data"
  "--go-grpc_opt=Mproto/data/session_journal_entry.proto=${GO_MODULE}/talon/data"
  "--go-grpc_opt=Mproto/events.proto=${GO_MODULE}/talon/events"
  "--go-grpc_opt=Mproto/talon/v1/auth.proto=${GO_MODULE}/talon/v1"
  "--go-grpc_opt=Mproto/talon/v1/channels.proto=${GO_MODULE}/talon/v1"
  "--go-grpc_opt=Mproto/talon/v1/connectors.proto=${GO_MODULE}/talon/v1"
  "--go-grpc_opt=Mproto/talon/v1/knowledge.proto=${GO_MODULE}/talon/v1"
  "--go-grpc_opt=Mproto/talon/v1/namespaces.proto=${GO_MODULE}/talon/v1"
  "--go-grpc_opt=Mproto/talon/v1/resources.proto=${GO_MODULE}/talon/v1"
  "--go-grpc_opt=Mproto/talon/v1/search.proto=${GO_MODULE}/talon/v1"
  "--go-grpc_opt=Mproto/talon/v1/sessions.proto=${GO_MODULE}/talon/v1"
  "--go-grpc_opt=Mproto/talon/v1/workflows.proto=${GO_MODULE}/talon/v1"
)

mkdir -p sdk/go/talon-client sdk/java/talon-client/src/main/java sdk/js/talon-client/src/gen sdk/python/talon-client/src/talon_client
rm -rf sdk/go/talon-client/talon sdk/java/talon-client/src/main/java/talon sdk/java/talon-client/src/main/java/com/google/api sdk/js/talon-client/src/gen sdk/python/talon-client/src/talon_client/proto sdk/python/talon-client/src/talon_client/google
mkdir -p sdk/js/talon-client/src/gen sdk/python/talon-client/src/talon_client

export PATH="$(go env GOPATH)/bin:$PATH"

"$PROTOC" -I. -Ithird_party/googleapis \
  --experimental_allow_proto3_optional \
  --go_out=sdk/go/talon-client \
  --go-grpc_out=sdk/go/talon-client \
  "${GO_OPTS[@]}" \
  "${PROTO_SRCS[@]}"

JAVA_GRPC_VERSION="${JAVA_GRPC_VERSION:-1.76.0}"
JAVA_GRPC_PLUGIN="$ROOT/.tools/protoc-gen-grpc-java/protoc-gen-grpc-java-${JAVA_GRPC_VERSION}-${JAVA_GRPC_PLATFORM}.exe"
if [[ ! -x "$JAVA_GRPC_PLUGIN" ]]; then
  mkdir -p "$(dirname "$JAVA_GRPC_PLUGIN")"
  curl -fL -o "$JAVA_GRPC_PLUGIN" \
    "https://repo1.maven.org/maven2/io/grpc/protoc-gen-grpc-java/${JAVA_GRPC_VERSION}/protoc-gen-grpc-java-${JAVA_GRPC_VERSION}-${JAVA_GRPC_PLATFORM}.exe"
  chmod +x "$JAVA_GRPC_PLUGIN"
fi

"$PROTOC" -I. -Ithird_party/googleapis \
  --experimental_allow_proto3_optional \
  --java_out=sdk/java/talon-client/src/main/java \
  "--grpc-java_out=sdk/java/talon-client/src/main/java" \
  "--plugin=protoc-gen-grpc-java=${JAVA_GRPC_PLUGIN}" \
  "${PROTO_SRCS[@]}"
find sdk/java/talon-client/src/main/java -name '*.java' -exec perl -pi -e 's/[ \t]+$//' {} +

python3 - <<'PY'
from pathlib import Path

root = Path("sdk/java/talon-client/src/main/java")
for path in root.rglob("*.java"):
    text = path.read_text()
    stripped = "\n".join(line.rstrip() for line in text.splitlines())
    if text.endswith("\n"):
        stripped += "\n"
    path.write_text(stripped)
PY

NPM_BIN="$ROOT/.tools/npm-bin"
mkdir -p "$NPM_BIN"
cat > "$NPM_BIN/protoc-gen-es" <<'EOF'
#!/usr/bin/env bash
exec npm exec --yes --package @bufbuild/protoc-gen-es@1.10.1 -- protoc-gen-es "$@"
EOF
cat > "$NPM_BIN/protoc-gen-connect-es" <<'EOF'
#!/usr/bin/env bash
exec npm exec --yes --package @connectrpc/protoc-gen-connect-es@1.7.0 -- protoc-gen-connect-es "$@"
EOF
chmod +x "$NPM_BIN/protoc-gen-es" "$NPM_BIN/protoc-gen-connect-es"

PATH="$NPM_BIN:$PATH" "$PROTOC" -I. -Ithird_party/googleapis \
  --experimental_allow_proto3_optional \
  --es_out=sdk/js/talon-client/src/gen \
  --es_opt=target=ts,import_extension=.js \
  --connect-es_out=sdk/js/talon-client/src/gen \
  --connect-es_opt=target=ts,import_extension=.js \
  "${PROTO_SRCS[@]}"
python3 - <<'PY'
from pathlib import Path

root = Path("sdk/js/talon-client/src/gen")
for path in root.rglob("*.ts"):
    source = path.read_text()
    lines = [line.rstrip() for line in source.splitlines()]
    while lines and lines[-1] == "":
        lines.pop()
    stripped = "\n".join(lines)
    path.write_text(stripped + "\n")
PY

PYTHON_CODEGEN="${PYTHON_CODEGEN:-python3}"
PY_TOOLS="$ROOT/.tools/python-codegen"
"$PYTHON_CODEGEN" -m pip install --quiet --upgrade --target "$PY_TOOLS" grpcio-tools==1.76.0
PYTHONPATH="$PY_TOOLS${PYTHONPATH:+:$PYTHONPATH}" "$PYTHON_CODEGEN" -m grpc_tools.protoc -I. -Ithird_party/googleapis \
  --experimental_allow_proto3_optional \
  --python_out=sdk/python/talon-client/src/talon_client \
  --pyi_out=sdk/python/talon-client/src/talon_client \
  --grpc_python_out=sdk/python/talon-client/src/talon_client \
  "${PROTO_SRCS[@]}"

find sdk/python/talon-client/src/talon_client -type d -exec sh -c 'touch "$0/__init__.py"' {} \;
python3 - <<'PY'
from pathlib import Path

root = Path("sdk/python/talon-client/src/talon_client")
for path in [
    *root.rglob("*_pb2.py"),
    *root.rglob("*_pb2.pyi"),
    *root.rglob("*_pb2_grpc.py"),
]:
    text = path.read_text()
    text = text.replace("from proto import ", "from talon_client.proto import ")
    text = text.replace("from proto.data import ", "from talon_client.proto.data import ")
    text = text.replace("from proto.harness import ", "from talon_client.proto.harness import ")
    text = text.replace("from proto.resources import ", "from talon_client.proto.resources import ")
    text = text.replace("from proto.talon import ", "from talon_client.proto.talon import ")
    text = text.replace("from proto.talon.v1 import ", "from talon_client.proto.talon.v1 import ")
    text = text.replace("import proto.events_pb2 as ", "import talon_client.proto.events_pb2 as ")
    text = text.replace("import proto.data.data_pb2 as ", "import talon_client.proto.data.data_pb2 as ")
    text = text.replace("import proto.resources.", "import talon_client.proto.resources.")
    text = text.replace("import proto.talon.", "import talon_client.proto.talon.")
    path.write_text(text)
PY
python3 scripts/sdk/generate_clientsets.py
gofmt -w sdk/go/talon-client/clientset_gen.go

run_with_retries() {
  local attempts="${SDK_CARGO_RETRY_ATTEMPTS:-4}"
  local delay="${SDK_CARGO_RETRY_DELAY_SECONDS:-5}"
  local attempt=1

  while true; do
    if "$@"; then
      return 0
    fi

    if (( attempt >= attempts )); then
      return 1
    fi

    echo "Command failed; retrying in ${delay}s (${attempt}/${attempts}): $*" >&2
    sleep "$delay"
    attempt=$((attempt + 1))
    delay=$((delay * 2))
  done
}

export CARGO_NET_RETRY="${CARGO_NET_RETRY:-10}"
run_with_retries cargo run --quiet --manifest-path sdk/rust/tools/codegen/Cargo.toml
