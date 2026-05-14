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

COPY ui/app ./app
COPY ui/components ./components
COPY ui/lib ./lib
COPY ui/proto ./proto
COPY ui/public ./public
COPY ui/utils ./utils
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
ENV PATH="$PNPM_HOME:$PATH"
ENV NODE_ENV=production
ENV NEXT_PUBLIC_GATEWAY_URL=""

RUN corepack enable

WORKDIR /app/ui

COPY --from=builder /repo/ui/package.json ./package.json
COPY --from=builder /repo/ui/.next ./.next
COPY --from=builder /repo/ui/node_modules ./node_modules
COPY --from=builder /repo/ui/public ./public
COPY --from=builder /repo/ui/next.config.mjs ./next.config.mjs

EXPOSE 3000

CMD ["pnpm", "start"]
