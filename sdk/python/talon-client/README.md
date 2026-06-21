# talon-client

Generated Python protobuf and gRPC bindings plus a small Talon clientset facade.

```python
import grpc
from talon_client import TalonClient
from talon_client.proto.talon.v1 import api_pb2

channel = grpc.insecure_channel("127.0.0.1:50051")
client = TalonClient(channel)
client.namespaces.List(api_pb2.ListNamespacesRequest())
```
