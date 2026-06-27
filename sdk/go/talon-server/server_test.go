package talonserver

import (
	"context"
	"testing"
)

func TestConfigYAMLUsesSQLiteAndLocalSocket(t *testing.T) {
	config := defaultConfig(nil, "/tmp/talon-data")
	controlPlane := config["control_plane"].(map[string]any)
	database := controlPlane["database"].(map[string]any)
	messageBroker := controlPlane["message_broker"].(map[string]any)
	if database["driver"] != "sqlite" || database["data_dir"] != "/tmp/talon-data" || messageBroker["driver"] != "local_socket" {
		t.Fatalf("unexpected config:\n%#v", config)
	}
}

func TestConfigCanSpecifyGeneralTalonSettings(t *testing.T) {
	config, err := configWithDataDir(map[string]any{
		"workspace_dir":    "/tmp/workspace",
		"default_provider": "openai",
		"control_plane": map[string]any{
			"database":       map[string]any{"driver": "sqlite"},
			"message_broker": map[string]any{"driver": "local_socket"},
		},
	}, "")
	if err != nil {
		t.Fatal(err)
	}
	if config["workspace_dir"] != "/tmp/workspace" || config["default_provider"] != "openai" {
		t.Fatalf("unexpected config: %#v", config)
	}
}

func TestStartRejectsAmbiguousConfigOptions(t *testing.T) {
	if _, err := Start(context.Background(), Options{ConfigPath: "talon.yaml", Config: map[string]any{"workspace_dir": "."}}); err == nil {
		t.Fatal("expected error")
	}
}

func TestAuthorizationHeader(t *testing.T) {
	token := "test-token"
	headerValue, err := AuthorizationHeader(token)
	if err != nil {
		t.Fatal(err)
	}
	if headerValue != "Bearer "+token {
		t.Fatalf("unexpected authorization header: %s", headerValue)
	}
}

func TestAuthorizationHeaderRequiresToken(t *testing.T) {
	if _, err := AuthorizationHeader(" "); err == nil {
		t.Fatal("expected error")
	}
}
