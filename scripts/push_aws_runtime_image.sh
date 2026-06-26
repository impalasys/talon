#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Build the internal Talon AWS runtime image in Linux Docker and push it to ECR.

Defaults target talon-prd in us-west-2:

  talon/scripts/push_aws_runtime_image.sh

Useful overrides:

  AWS_PROFILE=osprey-prd \
  AWS_REGION=us-west-2 \
  ECR_REPOSITORY=talon-prd/runtime \
  IMAGE_TAG="$(git rev-parse --short=12 HEAD)" \
  talon/scripts/push_aws_runtime_image.sh

The script preserves Bazel caches under:

  ~/.cache/talon/aws-runtime-bazel-output
  ~/.cache/talon/aws-runtime-bazel-repository
  ~/.cache/talon/bazel-bins
EOF
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

if ! command -v docker >/dev/null 2>&1; then
  echo "docker is required" >&2
  exit 1
fi

if ! command -v aws >/dev/null 2>&1; then
  echo "aws CLI is required" >&2
  exit 1
fi

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"

AWS_REGION="${AWS_REGION:-us-west-2}"
AWS_PROFILE="${AWS_PROFILE:-osprey-prd}"
AWS_ACCOUNT_ID="${AWS_ACCOUNT_ID:-}"
ECR_REPOSITORY="${ECR_REPOSITORY:-talon-prd/runtime}"
BAZEL_TARGET="${BAZEL_TARGET:-//talon:osprey_aws_runtime_image_tarball}"
BAZEL_VERSION="${BAZEL_VERSION:-9.1.1}"
DOCKER_PLATFORM="${DOCKER_PLATFORM:-linux/arm64}"
IMAGE_TAG="${IMAGE_TAG:-$(git -C "${REPO_ROOT}" rev-parse --short=12 HEAD)}"
PUSH_LATEST="${PUSH_LATEST:-1}"

CACHE_ROOT="${TALON_AWS_IMAGE_CACHE_ROOT:-${HOME}/.cache/talon}"
BAZEL_OUTPUT_CACHE="${BAZEL_OUTPUT_CACHE:-${CACHE_ROOT}/aws-runtime-bazel-output}"
BAZEL_REPOSITORY_CACHE="${BAZEL_REPOSITORY_CACHE:-${CACHE_ROOT}/aws-runtime-bazel-repository}"
BAZEL_BIN_CACHE="${BAZEL_BIN_CACHE:-${CACHE_ROOT}/bazel-bins}"
ARTIFACT_DIR="${ARTIFACT_DIR:-${CACHE_ROOT}/aws-runtime-artifacts}"
ARTIFACT_TAR="${ARTIFACT_DIR}/talon-runtime-image.tar"

case "${DOCKER_PLATFORM}" in
  linux/arm64 | linux/arm64/v8)
    BAZEL_ARCH="arm64"
    ;;
  *)
    echo "DOCKER_PLATFORM=${DOCKER_PLATFORM} is not supported for talon-prd." >&2
    echo "ECS is configured for ARM64, so this script defaults to linux/arm64." >&2
    exit 1
    ;;
esac

mkdir -p \
  "${BAZEL_OUTPUT_CACHE}" \
  "${BAZEL_REPOSITORY_CACHE}" \
  "${BAZEL_BIN_CACHE}" \
  "${ARTIFACT_DIR}"

if [[ -z "${AWS_ACCOUNT_ID}" ]]; then
  AWS_ACCOUNT_ID="$(
    AWS_PROFILE="${AWS_PROFILE}" AWS_REGION="${AWS_REGION}" \
      aws sts get-caller-identity --query Account --output text
  )"
fi

ECR_REGISTRY="${AWS_ACCOUNT_ID}.dkr.ecr.${AWS_REGION}.amazonaws.com"
ECR_IMAGE="${ECR_REGISTRY}/${ECR_REPOSITORY}"

echo "Building ${BAZEL_TARGET} inside Docker (${DOCKER_PLATFORM})..."
docker run --rm \
  --platform "${DOCKER_PLATFORM}" \
  --volume "${REPO_ROOT}:/workspace" \
  --volume "${BAZEL_OUTPUT_CACHE}:/bazel-output-cache" \
  --volume "${BAZEL_REPOSITORY_CACHE}:/bazel-repository-cache" \
  --volume "${BAZEL_BIN_CACHE}:/bazel-bin-cache" \
  --volume "${ARTIFACT_DIR}:/artifacts" \
  --env BAZEL_ARCH="${BAZEL_ARCH}" \
  --env BAZEL_TARGET="${BAZEL_TARGET}" \
  --env BAZEL_VERSION="${BAZEL_VERSION}" \
  --env CARGO_NET_GIT_FETCH_WITH_CLI=true \
  ubuntu:24.04 \
  bash -lc '
    set -euo pipefail

    export DEBIAN_FRONTEND=noninteractive
    apt-get update >/dev/null
    apt-get install -y --no-install-recommends \
      build-essential \
      ca-certificates \
      clang \
      curl \
      git \
      libssl-dev \
      lld \
      pkg-config \
      protobuf-compiler \
      python3 \
      unzip \
      xz-utils >/dev/null

    bazel_name="bazel-${BAZEL_VERSION}-linux-${BAZEL_ARCH}"
    bazel_bin="/bazel-bin-cache/${bazel_name}"
    if [[ ! -x "${bazel_bin}" ]]; then
      echo "Downloading ${bazel_name}..."
      curl -fsSL \
        "https://github.com/bazelbuild/bazel/releases/download/${BAZEL_VERSION}/${bazel_name}" \
        -o "${bazel_bin}"
      chmod +x "${bazel_bin}"
    fi

    ln -sf "${bazel_bin}" /usr/local/bin/bazel

    cd /workspace
    bazel \
      --output_user_root=/bazel-output-cache \
      --repository_cache=/bazel-repository-cache \
      build "${BAZEL_TARGET}"

    tarball="$(
      bazel \
        --output_user_root=/bazel-output-cache \
        --repository_cache=/bazel-repository-cache \
        cquery --output=files "${BAZEL_TARGET}" |
        tail -n 1
    )"
    cp "${tarball}" /artifacts/talon-runtime-image.tar
  '

echo "Loading image tarball ${ARTIFACT_TAR}..."
loaded_image="$(
  docker load --input "${ARTIFACT_TAR}" |
    awk -F": " "/Loaded image:/ { image = \$2 } /Loaded image ID:/ { image = \$2 } END { print image }"
)"

if [[ -z "${loaded_image}" ]]; then
  echo "docker load did not report a loadable image reference" >&2
  exit 1
fi

echo "Logging into ${ECR_REGISTRY}..."
AWS_PROFILE="${AWS_PROFILE}" AWS_REGION="${AWS_REGION}" \
  aws ecr get-login-password --region "${AWS_REGION}" |
  docker login --username AWS --password-stdin "${ECR_REGISTRY}" >/dev/null

echo "Verifying ECR repository ${ECR_REPOSITORY} exists..."
AWS_PROFILE="${AWS_PROFILE}" AWS_REGION="${AWS_REGION}" \
  aws ecr describe-repositories \
    --region "${AWS_REGION}" \
    --repository-names "${ECR_REPOSITORY}" >/dev/null

echo "Tagging ${loaded_image} as ${ECR_IMAGE}:${IMAGE_TAG}..."
docker tag "${loaded_image}" "${ECR_IMAGE}:${IMAGE_TAG}"
docker push "${ECR_IMAGE}:${IMAGE_TAG}"

if [[ "${PUSH_LATEST}" == "1" ]]; then
  echo "Tagging ${loaded_image} as ${ECR_IMAGE}:latest..."
  docker tag "${loaded_image}" "${ECR_IMAGE}:latest"
  docker push "${ECR_IMAGE}:latest"
fi

echo "Pushed ${ECR_IMAGE}:${IMAGE_TAG}"
if [[ "${PUSH_LATEST}" == "1" ]]; then
  echo "Pushed ${ECR_IMAGE}:latest"
fi
