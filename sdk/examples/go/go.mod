module github.com/impalasys/talon/sdk/examples/go

go 1.25.0

require (
	github.com/impalasys/talon/sdk/go/talon-client v0.0.0
	github.com/impalasys/talon/sdk/go/talon-server v0.0.0
)

require (
	github.com/golang-jwt/jwt/v5 v5.3.1 // indirect
	golang.org/x/net v0.55.0 // indirect
	golang.org/x/sys v0.45.0 // indirect
	golang.org/x/text v0.37.0 // indirect
	google.golang.org/genproto/googleapis/rpc v0.0.0-20260523011958-0a33c5d7ca68 // indirect
	google.golang.org/grpc v1.79.3 // indirect
	google.golang.org/protobuf v1.36.11 // indirect
)

replace github.com/impalasys/talon/sdk/go/talon-client => ../../go/talon-client

replace github.com/impalasys/talon/sdk/go/talon-server => ../../go/talon-server
