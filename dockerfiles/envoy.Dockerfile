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

FROM envoyproxy/envoy:v1.33-latest

COPY --from=descriptor /tmp/talon_gateway_proto-descriptor-set.proto.bin /etc/envoy/talon_gateway_proto-descriptor-set.proto.bin
COPY envoy.yaml /etc/envoy/envoy.yaml
