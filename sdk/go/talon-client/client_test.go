package talonclient_test

import (
	"testing"

	"github.com/impalasys/talon/sdk/go/talon-client/talon/gateway"
	"google.golang.org/protobuf/proto"
)

func TestGeneratedGatewayTypesAreAvailable(t *testing.T) {
	req := &gateway.ListResourcesRequest{Ns: "default", Kind: proto.String("Agent")}
	if req.GetNs() != "default" {
		t.Fatalf("unexpected namespace: %q", req.GetNs())
	}
	if req.GetKind() != "Agent" {
		t.Fatalf("unexpected kind: %q", req.GetKind())
	}
}
