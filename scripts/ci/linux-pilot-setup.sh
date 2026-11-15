#!/usr/bin/env bash
set -euo pipefail

# Align the actual Ubuntu release package and native userspace, rather than
# weakening the comparison fingerprint or editing /etc/os-release.
source /etc/os-release
[[ "$ID" == ubuntu && "$VERSION_ID" == 24.04 && "$(uname -m)" == x86_64 ]] || {
  echo 'The Linux pilot requires Ubuntu 24.04 x86_64.' >&2
  exit 1
}
packages=(ca-certificates git python3 pkg-config)
missing=()
for package in "${packages[@]}"; do
  if [[ "$(dpkg-query -W -f='${Status}' "$package" 2>/dev/null || true)" != 'install ok installed' ]]; then
    missing+=("$package")
  fi
done
while IFS= read -r specification; do
  [[ -z "$specification" || "$specification" == \#* ]] && continue
  package="${specification%%=*}"
  version="${specification#*=}"
  if [[ "$(dpkg-query -W -f='${Version}' "$package" 2>/dev/null || true)" != "$version" ]]; then
    missing+=("$specification")
  fi
done < scripts/ci/linux-pilot-packages.txt
if (( ${#missing[@]} )); then
  sudo apt-get update
  sudo apt-get install -y --no-install-recommends "${missing[@]}"
fi
source /etc/os-release
[[ "$PRETTY_NAME" == 'Ubuntu 24.04.5 LTS' ]] || {
  echo "Unexpected pilot release after package alignment: $PRETTY_NAME" >&2
  exit 1
}
# BuildBuddy's runner cleanup also invokes Bazel. Keep startup options in its
# normal rc lookup so workload and cleanup can read the same heap setting.
heap_option='startup --host_jvm_args=-Xmx4g'
if ! grep -Fxq -- "$heap_option" .bazelrc.local 2>/dev/null; then
  umask 077
  printf '%s\n' "$heap_option" >> .bazelrc.local
fi
