#!/usr/bin/env bash
# Build OCI images in a native Linux environment, then load them into Docker.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
image_names=("$@")
if [[ ${#image_names[@]} == 0 ]]; then image_names=(runtime ui); fi
image_targets=()
for image in "${image_names[@]}"; do
  case "$image" in runtime|ui|benchmark) ;; *) echo "Expected runtime, ui, or benchmark" >&2; exit 2 ;; esac
  image_targets+=("//dockerfiles:${image}_image")
done
bazel_version="$(cat "$repo_root/.bazelversion")"
if [[ "$bazel_version" != 9.3.2 ]]; then
  echo "Update the pinned Linux Bazel checksums in scripts/build-images.sh" >&2
  exit 2
fi
case "$(docker info --format '{{.Architecture}}')" in
  x86_64|amd64) builder_arch=x86_64; bazel_sha=2e14c55c298752d71e8a0bbd762665ffce5966c9ad00ec5d1a8b82a58e7bfa06 ;;
  aarch64|arm64) builder_arch=arm64; bazel_sha=13d5677509ac5e52f28139d76b1343e8a4aa494451558222599abbb86bff92cb ;;
  *) echo "Unsupported Docker CPU architecture" >&2; exit 2 ;;
esac
builder_name="talon-bazel-images-${builder_arch}-$$"
builder_output="$(mktemp -d)"
cleanup() {
  docker rm -f "$builder_name" >/dev/null 2>&1 || true
  rm -rf "$builder_output"
}
trap cleanup EXIT
# The persistent volume keeps Bazel's repository, output, and action caches.
docker run --interactive --name "$builder_name" \
  --volume "$repo_root:/src:ro" \
  --volume "talon-bazel-linux-${builder_arch}:/cache" \
  --env "TALON_BAZEL_VERSION=$bazel_version" \
  --env "TALON_BAZEL_ARCH=$builder_arch" \
  --env "TALON_BAZEL_SHA=$bazel_sha" \
  --env "TALON_BAZEL_JOBS=${TALON_BAZEL_JOBS:-4}" \
  ubuntu@sha256:fe81560990fae86dc4743202b2cb4d3df98d0bcf949173cecef15ce1127ef852 \
  bash -s -- "${image_targets[@]}" <<'BUILD'
set -euo pipefail
apt-get update -qq
DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
  build-essential ca-certificates clang cmake curl git libclang-dev libssl-dev \
  pkg-config python3 unzip xz-utils zip >/dev/null
curl -fsSL "https://github.com/bazelbuild/bazel/releases/download/${TALON_BAZEL_VERSION}/bazel-${TALON_BAZEL_VERSION}-linux-${TALON_BAZEL_ARCH}" -o /usr/local/bin/bazel
printf '%s  /usr/local/bin/bazel\n' "$TALON_BAZEL_SHA" | sha256sum --check --status
chmod +x /usr/local/bin/bazel
mkdir /workspace
# Copy source inputs without host build outputs or local credentials.
tar -C /src --exclude='./.git' --exclude='./bazel-*' --exclude='node_modules' \
  --exclude='target' --exclude='dist' --exclude='storybook-static' --exclude='.tools' \
  --exclude='coverage' --exclude='playwright-report' --exclude='test-results' --exclude='storybook-screenshots' \
  --exclude='.venv*' --exclude='__pycache__' --exclude='.env' --exclude='.env.*' \
  --exclude='./.bazelrc.local' -cf - . | tar -C /workspace -xf -
cd /workspace
bazel --output_user_root=/cache/output build --disk_cache=/cache/disk \
  --repository_cache=/cache/repositories --jobs="$TALON_BAZEL_JOBS" \
  --output_groups=tarball "$@"
BUILD
for image in "${image_names[@]}"; do
  docker cp -L "$builder_name:/workspace/bazel-bin/dockerfiles/${image}_image/tarball.tar" "$builder_output/${image}.tar"
  docker load --input "$builder_output/${image}.tar"
done
