FROM envoyproxy/envoy:v1.30-latest

RUN apt-get update \
    && apt-get install -y --no-install-recommends protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

COPY proto ./proto
COPY third_party/googleapis ./third_party/googleapis

RUN protoc -I. -Iproto -Ithird_party/googleapis \
    --include_imports \
    --include_source_info \
    --experimental_allow_proto3_optional \
    --descriptor_set_out=/etc/envoy/talon_gateway_proto-descriptor-set.proto.bin \
    proto/gateway.proto

COPY envoy.yaml /etc/envoy/envoy.yaml
