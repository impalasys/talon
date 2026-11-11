# syntax=docker/dockerfile:1.7
# Vite emits static assets; no compiler or package manager is needed at runtime.
FROM nginx:1.28-alpine AS ui-base
COPY <<'CONFIG' /etc/nginx/conf.d/default.conf
server {
    listen 3000;
    root /usr/share/nginx/html;
    location / { try_files $uri $uri/ /index.html; }
}
CONFIG
EXPOSE 3000

FROM ui-base AS ui-from-bazel
COPY dist/ /usr/share/nginx/html/

FROM gcr.io/bazel-public/bazel:9.3.2 AS bazel-builder
USER root
WORKDIR /workspace
COPY . .
ARG NEXT_PUBLIC_GATEWAY_URL=""
RUN --mount=type=cache,target=/root/.cache/bazel \
    bazel --output_user_root=/root/.cache/bazel build \
      --define="GATEWAY_URL=${NEXT_PUBLIC_GATEWAY_URL}" //ui:dist \
    && cp -rL bazel-bin/ui/dist /out

FROM ui-base AS ui
COPY --from=bazel-builder /out/ /usr/share/nginx/html/
