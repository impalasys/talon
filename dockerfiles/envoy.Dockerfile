FROM envoyproxy/envoy:v1.30-latest

COPY talon_gateway_proto-descriptor-set.proto.bin /etc/envoy/talon_gateway_proto-descriptor-set.proto.bin
COPY envoy.yaml /etc/envoy/envoy.yaml
