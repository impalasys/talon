# syntax=docker/dockerfile:1.7

FROM node:22-slim AS deps

ENV PNPM_HOME="/pnpm"
ENV PATH="$PNPM_HOME:$PATH"
RUN corepack enable

WORKDIR /repo/ui

COPY ui/package.json ./
COPY ui/pnpm-lock.yaml ./pnpm-lock.yaml
RUN pnpm install --frozen-lockfile --config.node-linker=hoisted

FROM deps AS builder

ARG NEXT_PUBLIC_GATEWAY_URL=""
ENV NEXT_PUBLIC_GATEWAY_URL=$NEXT_PUBLIC_GATEWAY_URL

COPY ui ./
RUN pnpm run build

FROM node:22-slim AS runner

ENV PNPM_HOME="/pnpm"
ENV PATH="$PNPM_HOME:$PATH"
ENV NODE_ENV=production
ENV NEXT_PUBLIC_GATEWAY_URL=""

RUN corepack enable

WORKDIR /app/ui

COPY --from=builder /repo/ui/package.json ./package.json
COPY --from=builder /repo/ui/.next ./.next
COPY --from=builder /repo/ui/node_modules ./node_modules
COPY --from=builder /repo/ui/public ./public
COPY --from=builder /repo/ui/next.config.ts ./next.config.ts

EXPOSE 3000

CMD ["pnpm", "start"]
