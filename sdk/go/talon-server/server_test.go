package talonserver

import (
	"strings"
	"testing"
)

func TestConfigYAMLUsesSQLiteAndLocalSocket(t *testing.T) {
	config := configYAML(nil)
	if !strings.Contains(config, "driver: sqlite") || !strings.Contains(config, "driver: local_socket") {
		t.Fatalf("unexpected config:\n%s", config)
	}
}
