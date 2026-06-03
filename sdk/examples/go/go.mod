module github.com/impalasys/talon/sdk/examples/go

go 1.25.0

require (
	github.com/impalasys/talon/sdk/go/talon-client v0.0.0
	github.com/impalasys/talon/sdk/go/talon-server v0.0.0
	google.golang.org/grpc v1.79.3
)

require (
	golang.org/x/net v0.48.0 // indirect
	golang.org/x/sys v0.39.0 // indirect
	golang.org/x/text v0.32.0 // indirect
	google.golang.org/genproto/googleapis/api v0.0.0-20260526163538-3dc84a4a5aaa // indirect
	google.golang.org/genproto/googleapis/rpc v0.0.0-20260523011958-0a33c5d7ca68 // indirect
	google.golang.org/protobuf v1.36.11 // indirect
)

replace github.com/impalasys/talon/sdk/go/talon-client => ../../go/talon-client

replace github.com/impalasys/talon/sdk/go/talon-server => ../../go/talon-server
