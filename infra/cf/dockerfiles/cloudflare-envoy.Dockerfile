FROM debian:trixie-slim AS descriptor

WORKDIR /src

RUN apt-get update && apt-get install -y --no-install-recommends \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

COPY proto ./proto
COPY third_party/googleapis ./third_party/googleapis

RUN protoc \
    -I. \
    -Iproto \
    -Ithird_party/googleapis \
    --include_imports \
    --include_source_info \
    --experimental_allow_proto3_optional \
    --descriptor_set_out=/tmp/talon_gateway_proto-descriptor-set.proto.bin \
    proto/gateway.proto

FROM envoyproxy/envoy:v1.33-latest@sha256:f91c972d5c99bc133233a079b5663b903e7d56b3b0b0216398924f7b80d09e47

COPY --from=descriptor /tmp/talon_gateway_proto-descriptor-set.proto.bin /etc/envoy/talon_gateway_proto-descriptor-set.proto.bin
COPY infra/cf/envoy.yaml /etc/envoy/envoy.yaml

EXPOSE 8081
