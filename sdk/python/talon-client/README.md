# talon-client

Generated Python protobuf and gRPC bindings for the Talon gateway.

```python
import grpc
from talon_client.proto import gateway_pb2, gateway_pb2_grpc

channel = grpc.insecure_channel("127.0.0.1:50051")
client = gateway_pb2_grpc.GatewayServiceStub(channel)
client.ListNamespaces(gateway_pb2.ListNamespacesRequest())
```
