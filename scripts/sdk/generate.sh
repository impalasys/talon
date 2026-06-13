#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

PROTO_SRCS=(
  proto/config.proto
  proto/models.proto
  proto/manifests.proto
  proto/events.proto
  proto/gateway.proto
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
  "--go_opt=Mproto/models.proto=${GO_MODULE}/talon/models"
  "--go_opt=Mproto/manifests.proto=${GO_MODULE}/talon/manifests"
  "--go_opt=Mproto/events.proto=${GO_MODULE}/talon/events"
  "--go_opt=Mproto/gateway.proto=${GO_MODULE}/talon/gateway"
  "--go-grpc_opt=Mproto/config.proto=${GO_MODULE}/talon/config"
  "--go-grpc_opt=Mproto/models.proto=${GO_MODULE}/talon/models"
  "--go-grpc_opt=Mproto/manifests.proto=${GO_MODULE}/talon/manifests"
  "--go-grpc_opt=Mproto/events.proto=${GO_MODULE}/talon/events"
  "--go-grpc_opt=Mproto/gateway.proto=${GO_MODULE}/talon/gateway"
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
  "${PROTO_SRCS[@]}" \
  google/api/http.proto \
  google/api/annotations.proto
find sdk/java/talon-client/src/main/java -name '*.java' -exec perl -pi -e 's/[ \t]+$//' {} +

python3 - <<'PY'
from pathlib import Path

path = Path("sdk/java/talon-client/src/main/java/talon/gateway/Gateway.java")
text = path.read_text()


def strip_region(source: str, start_marker: str, end_marker: str) -> str:
    start = source.index(start_marker)
    end = source.index(end_marker, start)
    region = source[start:end]
    stripped = "\n".join(line.rstrip() for line in region.splitlines())
    if region.endswith("\n"):
        stripped += "\n"
    return source[:start] + stripped + source[end:]


text = strip_region(
    text,
    "  public static final class ClearSessionRequest",
    "  public interface CreateChannelRequestOrBuilder",
)
text = strip_region(
    text,
    "  private static final com.google.protobuf.Descriptors.Descriptor\n"
    "    internal_static_talon_gateway_ClearSessionRequest_descriptor;",
    "  private static final com.google.protobuf.Descriptors.Descriptor\n"
    "    internal_static_talon_gateway_CreateChannelRequest_descriptor;",
)
path.write_text(text)
PY

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
  "${PROTO_SRCS[@]}" \
  google/api/http.proto \
  google/api/annotations.proto

PYTHON_CODEGEN="${PYTHON_CODEGEN:-python3}"
PY_TOOLS="$ROOT/.tools/python-codegen"
"$PYTHON_CODEGEN" -m pip install --quiet --target "$PY_TOOLS" grpcio-tools==1.76.0
PYTHONPATH="$PY_TOOLS${PYTHONPATH:+:$PYTHONPATH}" "$PYTHON_CODEGEN" -m grpc_tools.protoc -I. -Ithird_party/googleapis \
  --experimental_allow_proto3_optional \
  --python_out=sdk/python/talon-client/src/talon_client \
  --grpc_python_out=sdk/python/talon-client/src/talon_client \
  "${PROTO_SRCS[@]}" \
  google/api/http.proto \
  google/api/annotations.proto

find sdk/python/talon-client/src/talon_client -type d -exec sh -c 'touch "$0/__init__.py"' {} \;
python3 - <<'PY'
from pathlib import Path

root = Path("sdk/python/talon-client/src/talon_client")
for path in root.rglob("*_pb2*.py"):
    text = path.read_text()
    text = text.replace("from proto import ", "from talon_client.proto import ")
    text = text.replace("from google.api import ", "from talon_client.google.api import ")
    path.write_text(text)
PY

cargo run --quiet --manifest-path sdk/rust/tools/codegen/Cargo.toml
