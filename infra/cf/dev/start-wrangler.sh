#!/usr/bin/env sh
set -eu

if [ "$(id -u)" = "0" ]; then
  if [ -S /var/run/docker.sock ]; then
    docker_gid="$(stat -c '%g' /var/run/docker.sock)"
    groupadd -f -o -g "$docker_gid" docker-host
    usermod -aG docker-host node
  fi
  mkdir -p \
    /repo/.wrangler \
    /repo/infra/cf/dev/.wrangler \
    /repo/infra/cf/worker/.wrangler \
    /repo/infra/cf/worker/node_modules
  chown -R node:node \
    /repo/.wrangler \
    /repo/infra/cf/dev/.wrangler \
    /repo/infra/cf/worker/.wrangler \
    /repo/infra/cf/worker/node_modules
  exec su node -s /bin/sh -c "cd /repo/infra/cf/worker && if [ ! -x node_modules/.bin/wrangler ]; then npm ci; fi; npm run dev"
fi

exec sh -c "if [ ! -x node_modules/.bin/wrangler ]; then npm ci; fi; npm run dev"
