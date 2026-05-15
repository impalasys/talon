# Talon

Talon is a Rust-based agent runtime and control plane with a gateway API, worker runtime, namespace-scoped resources, and an optional Next.js UI.

This repository is the open-source home for Talon runtime and UI code. It intentionally excludes Impala's private marketing site and internal deployment plumbing.

## Included

- Rust runtime, worker, and CLI
- Proto definitions
- Resource manifests
- UI under `ui/`
- Dockerfiles for open-source validation

## Not included

- Internal production deploy configuration
- `site/` marketing/docs site from the private monorepo

## Local development

### Rust

```bash
cargo metadata --locked
cargo build --locked --bins
cargo test --locked
```

### UI

```bash
cd ui
pnpm install --no-frozen-lockfile
pnpm build
```

### Docker validation

```bash
docker build -f dockerfiles/oss-runtime.Dockerfile .
docker build -f dockerfiles/oss-ui.Dockerfile .
```

To validate the Envoy image:

```bash
protoc -I. -Iproto -Ithird_party/googleapis \
  --include_imports \
  --include_source_info \
  --experimental_allow_proto3_optional \
  --descriptor_set_out=talon_gateway_proto-descriptor-set.proto.bin \
  proto/gateway.proto

docker build -f dockerfiles/envoy-cloudrun.Dockerfile .
```

## License

This repository is licensed under the GNU Affero General Public License v3.0. See `LICENSE`.

## Contributions

External contributions are accepted under the Talon Contributor License
Agreement in `CLA.md`. Rust source files use the repository's standard copyright
and SPDX header, enforced in CI.
