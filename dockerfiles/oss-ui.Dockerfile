# syntax=docker/dockerfile:1.7

FROM node:22-slim AS deps

ENV PNPM_HOME="/pnpm"
ENV PATH="$PNPM_HOME:$PATH"
RUN corepack enable

WORKDIR /repo/ui

COPY pnpm-workspace.yaml /repo/pnpm-workspace.yaml
COPY pnpm-lock.yaml /repo/pnpm-lock.yaml
COPY ui/package.json ./
COPY packages/copilot/package.json /repo/packages/copilot/package.json
RUN --mount=type=cache,target=/pnpm/store \
    pnpm install --frozen-lockfile --config.node-linker=hoisted

FROM deps AS builder

ARG NEXT_PUBLIC_GATEWAY_URL=""
ENV NEXT_PUBLIC_GATEWAY_URL=$NEXT_PUBLIC_GATEWAY_URL

COPY ui/app ./app
COPY ui/components ./components
COPY ui/lib ./lib
COPY ui/proto ./proto
COPY ui/utils ./utils
COPY packages/copilot/src /repo/packages/copilot/src
COPY packages/copilot/README.md /repo/packages/copilot/README.md
COPY packages/copilot/tsup.config.ts /repo/packages/copilot/tsup.config.ts
COPY ui/global.d.ts ./global.d.ts
COPY ui/next-env.d.ts ./next-env.d.ts
COPY ui/next.config.mjs ./next.config.mjs
COPY ui/postcss.config.mjs ./postcss.config.mjs
COPY ui/tailgrids.config.json ./tailgrids.config.json
COPY ui/tsconfig.json ./tsconfig.json
COPY ui/types.d.ts ./types.d.ts
RUN pnpm run build

FROM node:22-slim AS runner

ENV PNPM_HOME="/pnpm"
ENV PATH="/app/node_modules/.bin:/app/ui/node_modules/.bin:$PNPM_HOME:$PATH"
ENV NODE_ENV=production
ENV NEXT_PUBLIC_GATEWAY_URL=""

RUN corepack enable

WORKDIR /app

COPY --from=builder /repo/node_modules ./node_modules
COPY --from=builder /repo/ui ./ui

WORKDIR /app/ui

EXPOSE 3000

CMD ["pnpm", "start"]
