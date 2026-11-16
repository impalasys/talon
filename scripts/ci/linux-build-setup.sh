#!/usr/bin/env bash
set -euo pipefail
bash scripts/ci/linux-pilot-setup.sh
missing=()
command -v docker >/dev/null || missing+=(docker.io)
command -v unzip >/dev/null || missing+=(unzip)
if (( ${#missing[@]} )); then
  sudo apt-get update
  sudo apt-get install -y --no-install-recommends "${missing[@]}"
fi
if ! sudo docker info >/dev/null 2>&1; then
  sudo sh -c 'nohup dockerd --host=unix:///var/run/docker.sock > /var/log/talon-ci-docker.log 2>&1 < /dev/null &'
fi
for attempt in {1..60}; do
  if [[ -S /var/run/docker.sock ]]; then
    sudo chgrp "$(id -g)" /var/run/docker.sock
    sudo chmod 660 /var/run/docker.sock
  fi
  if docker info >/dev/null 2>&1; then
    docker version
    exit 0
  fi
  sleep 1
done
sudo tail -n 100 /var/log/talon-ci-docker.log || true
echo 'Docker daemon did not become ready' >&2
exit 1
