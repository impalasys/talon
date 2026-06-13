#!/usr/bin/env sh
set -eu

if [ "$(id -u)" = "0" ]; then
  if [ -S /var/run/docker.sock ]; then
    docker_gid="$(stat -c '%g' /var/run/docker.sock)"
    groupadd -f -o -g "$docker_gid" docker-host
    usermod -aG docker-host node
  fi
  mkdir -p /repo/infra/cf/worker/node_modules /repo/infra/cf/worker/.wrangler
  chown -R node:node /repo/infra/cf/worker/node_modules /repo/infra/cf/worker/.wrangler
  exec su node -s /bin/sh -c "cd /repo/infra/cf/worker && npm ci && npm run dev"
fi

exec sh -c "npm ci && npm run dev"
