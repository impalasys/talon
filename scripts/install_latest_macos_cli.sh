#!/usr/bin/env bash
set -euo pipefail

REPO="${TALON_REPO:-impalasys/talon}"
BRANCH="${TALON_BRANCH:-main}"
WORKFLOW="${TALON_WORKFLOW:-ci.yml}"
INSTALL_DIR="${TALON_INSTALL_DIR:-/usr/local/bin}"
INSTALL_NAME="${TALON_INSTALL_NAME:-talon}"
SIGNING_IDENTIFIER="${TALON_SIGNING_IDENTIFIER:-com.impalasys.talon.cli}"
SHA=""
USE_LATEST_SUCCESSFUL="false"

usage() {
  cat <<'EOF'
Usage: scripts/install_latest_macos_cli.sh [options]

Downloads talon-cli from the macOS arm64 artifact produced by GitHub CI,
installs it locally, and applies an ad-hoc macOS signature.

Options:
  --repo OWNER/REPO        GitHub repository to download from (default: impalasys/talon)
  --branch BRANCH         Branch whose latest commit should be used (default: main)
  --sha SHA               Commit SHA whose successful CI artifact should be used
  --workflow FILE         Workflow file name (default: ci.yml)
  --install-dir DIR       Destination directory (default: /usr/local/bin)
  --install-name NAME     Installed binary name (default: talon)
  --latest-successful     Use the latest successful branch run instead of the branch HEAD
  -h, --help              Show this help text

Environment overrides:
  TALON_REPO, TALON_BRANCH, TALON_WORKFLOW, TALON_INSTALL_DIR,
  TALON_INSTALL_NAME, TALON_SIGNING_IDENTIFIER

Requirements:
  macOS arm64, gh, codesign, tar, shasum, sudo if the install dir is not writable
EOF
}

die() {
  echo "error: $*" >&2
  exit 1
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo)
      REPO="${2:-}"
      shift 2
      ;;
    --branch)
      BRANCH="${2:-}"
      shift 2
      ;;
    --sha)
      SHA="${2:-}"
      shift 2
      ;;
    --workflow)
      WORKFLOW="${2:-}"
      shift 2
      ;;
    --install-dir)
      INSTALL_DIR="${2:-}"
      shift 2
      ;;
    --install-name)
      INSTALL_NAME="${2:-}"
      shift 2
      ;;
    --latest-successful)
      USE_LATEST_SUCCESSFUL="true"
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "unknown option: $1"
      ;;
  esac
done

[[ -n "$REPO" ]] || die "--repo must not be empty"
[[ -n "$BRANCH" ]] || die "--branch must not be empty"
[[ -n "$WORKFLOW" ]] || die "--workflow must not be empty"
[[ -n "$INSTALL_DIR" ]] || die "--install-dir must not be empty"
[[ -n "$INSTALL_NAME" ]] || die "--install-name must not be empty"

[[ "$(uname -s)" == "Darwin" ]] || die "this installer only supports macOS"
[[ "$(uname -m)" == "arm64" ]] || die "CI currently publishes only the darwin-arm64 talon-cli artifact"

for cmd in gh codesign tar shasum; do
  command -v "$cmd" >/dev/null 2>&1 || die "missing required command: $cmd"
done

gh auth status --hostname github.com >/dev/null 2>&1 || die "gh is not authenticated; run: gh auth login"

if [[ -z "$SHA" && "$USE_LATEST_SUCCESSFUL" != "true" ]]; then
  echo "Resolving ${REPO}@${BRANCH}..."
  SHA="$(gh api "repos/${REPO}/commits/${BRANCH}" --jq .sha)"
fi

if [[ "$USE_LATEST_SUCCESSFUL" == "true" ]]; then
  echo "Finding latest successful ${WORKFLOW} run on ${REPO}:${BRANCH}..."
  RUN_INFO="$(gh run list \
    --repo "$REPO" \
    --workflow "$WORKFLOW" \
    --branch "$BRANCH" \
    --status success \
    --limit 1 \
    --json databaseId,headSha \
    --jq '.[0] | select(. != null) | [.databaseId, .headSha] | @tsv')"
else
  echo "Finding successful ${WORKFLOW} run for ${SHA}..."
  RUN_INFO="$(gh run list \
    --repo "$REPO" \
    --workflow "$WORKFLOW" \
    --commit "$SHA" \
    --status success \
    --limit 1 \
    --json databaseId,headSha \
    --jq '.[0] | select(. != null) | [.databaseId, .headSha] | @tsv')"
fi

[[ -n "$RUN_INFO" ]] || die "no successful ${WORKFLOW} run found; CI may still be running or the artifact may have expired"

IFS=$'\t' read -r RUN_ID RUN_SHA <<< "$RUN_INFO"
[[ -n "$RUN_ID" ]] || die "could not parse workflow run id"
[[ -n "$RUN_SHA" ]] || die "could not parse workflow run sha"

ARTIFACT_NAME="talon-darwin-arm64-${RUN_SHA}"
ARCHIVE_NAME="${ARTIFACT_NAME}.tar.gz"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

echo "Downloading artifact ${ARTIFACT_NAME} from run ${RUN_ID}..."
gh run download "$RUN_ID" \
  --repo "$REPO" \
  --name "$ARTIFACT_NAME" \
  --dir "$TMP_DIR"

[[ -s "${TMP_DIR}/${ARCHIVE_NAME}" ]] || die "downloaded artifact did not contain ${ARCHIVE_NAME}"

mkdir -p "${TMP_DIR}/extract"
tar -xzf "${TMP_DIR}/${ARCHIVE_NAME}" -C "${TMP_DIR}/extract"

SOURCE_BIN="${TMP_DIR}/extract/talon-darwin-arm64/talon-cli"
CHECKSUMS="${TMP_DIR}/extract/talon-darwin-arm64/SHA256SUMS"

[[ -x "$SOURCE_BIN" ]] || die "artifact did not contain an executable talon-cli"
[[ -s "$CHECKSUMS" ]] || die "artifact did not contain SHA256SUMS"

(
  cd "$(dirname "$SOURCE_BIN")"
  shasum -a 256 -c SHA256SUMS --ignore-missing
)

SIGNED_BIN="${TMP_DIR}/${INSTALL_NAME}"
cp "$SOURCE_BIN" "$SIGNED_BIN"
chmod 755 "$SIGNED_BIN"

echo "Applying local ad-hoc signature..."
codesign \
  --force \
  --sign - \
  --identifier "$SIGNING_IDENTIFIER" \
  --options runtime \
  --timestamp=none \
  "$SIGNED_BIN"

codesign --verify --strict --verbose=2 "$SIGNED_BIN"

INSTALL_PATH="${INSTALL_DIR%/}/${INSTALL_NAME}"
if [[ -d "$INSTALL_DIR" && -w "$INSTALL_DIR" ]]; then
  install -m 755 "$SIGNED_BIN" "$INSTALL_PATH"
else
  command -v sudo >/dev/null 2>&1 || die "install dir is not writable and sudo is unavailable: $INSTALL_DIR"
  sudo mkdir -p "$INSTALL_DIR"
  sudo install -m 755 "$SIGNED_BIN" "$INSTALL_PATH"
fi

codesign --verify --strict --verbose=2 "$INSTALL_PATH"
echo "Installed ${INSTALL_PATH}"
echo "Run: ${INSTALL_PATH} --help"
