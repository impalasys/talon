package talonclient_test

import (
	"testing"

	"github.com/impalasys/talon/sdk/go/talon-client/talon/gateway"
)

func TestGeneratedGatewayTypesAreAvailable(t *testing.T) {
	req := &gateway.ListAgentsRequest{Ns: "default"}
	if req.GetNs() != "default" {
		t.Fatalf("unexpected namespace: %q", req.GetNs())
	}
}
