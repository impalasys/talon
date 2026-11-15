#!/usr/bin/env bash
set -euo pipefail

# Both providers use Ubuntu 24.04's native LLVM 18 toolchain. Check package
# state instead of a stamp so a recycled VM only installs missing packages.
source /etc/os-release
[[ "$ID" == ubuntu && "$VERSION_ID" == 24.04 && "$(uname -m)" == x86_64 ]] || {
  echo 'The Linux pilot requires Ubuntu 24.04 x86_64.' >&2
  exit 1
}
packages=(build-essential ca-certificates git python3 pkg-config libssl-dev clang-18 libclang-18-dev cmake)
missing=()
for package in "${packages[@]}"; do
  if [[ "$(dpkg-query -W -f='${Status}' "$package" 2>/dev/null || true)" != 'install ok installed' ]]; then
    missing+=("$package")
  fi
done
if (( ${#missing[@]} )); then
  sudo apt-get update
  sudo apt-get install -y --no-install-recommends "${missing[@]}"
fi
