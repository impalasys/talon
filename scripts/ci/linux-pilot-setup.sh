#!/usr/bin/env bash
set -euo pipefail

# Pin Ubuntu's official snapshot archive so exact package versions in
# scripts/ci/linux-pilot-packages.txt stay installable after noble-updates or
# noble-security rotates newer versions into the live archive.
readonly UBUNTU_SNAPSHOT='20261116T000000Z'
readonly OS_RELEASE_PATH="${TALON_OS_RELEASE_PATH:-/etc/os-release}"

# Align the actual Ubuntu release package and native userspace, rather than
# weakening the comparison fingerprint or editing /etc/os-release.
source "$OS_RELEASE_PATH"
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
  # snapshot.ubuntu.com redirects HTTP to HTTPS, so bootstrap CA certificates
  # from the default archive first on minimal images that omit them.
  if [[ "$(dpkg-query -W -f='${Status}' ca-certificates 2>/dev/null || true)" != 'install ok installed' ]]; then
    sudo env DEBIAN_FRONTEND=noninteractive apt-get update
    sudo env DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends ca-certificates
  fi
  snapshot_dir="$(mktemp -d /tmp/talon-ubuntu-snapshot-XXXXXX)"
  trap 'rm -rf "$snapshot_dir"' EXIT
  mkdir -p "${snapshot_dir}/empty.d"
  snapshot_uri="https://snapshot.ubuntu.com/ubuntu/${UBUNTU_SNAPSHOT}"
  cat > "${snapshot_dir}/snapshot.list" <<EOF
deb [signed-by=/usr/share/keyrings/ubuntu-archive-keyring.gpg] ${snapshot_uri} noble main restricted universe multiverse
deb [signed-by=/usr/share/keyrings/ubuntu-archive-keyring.gpg] ${snapshot_uri} noble-updates main restricted universe multiverse
deb [signed-by=/usr/share/keyrings/ubuntu-archive-keyring.gpg] ${snapshot_uri} noble-security main restricted universe multiverse
EOF
  # Priority > 1000 allows apt to downgrade transitive dependencies (such as
  # libssl3t64 or libc-dev-bin) when a newer base image ships ahead of the pin.
  cat > "${snapshot_dir}/snapshot.pref" <<'EOF'
Package: *
Pin: release o=Ubuntu
Pin-Priority: 1001
EOF
  apt_opts=(
    -o "Dir::Etc::sourcelist=${snapshot_dir}/snapshot.list"
    -o "Dir::Etc::sourceparts=${snapshot_dir}/empty.d"
    -o "Dir::Etc::preferences=${snapshot_dir}/snapshot.pref"
    -o "Dir::Etc::preferencesparts=${snapshot_dir}/empty.d"
    -o 'Acquire::Check-Valid-Until=false'
  )
  sudo env DEBIAN_FRONTEND=noninteractive apt-get "${apt_opts[@]}" update
  sudo env DEBIAN_FRONTEND=noninteractive apt-get "${apt_opts[@]}" install -y \
    --allow-downgrades --allow-change-held-packages --no-install-recommends "${missing[@]}"
fi
source "$OS_RELEASE_PATH"
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
