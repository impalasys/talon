package talonserver

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"strings"
	"testing"
	"time"
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

func TestMintJWT(t *testing.T) {
	token, err := MintJWT("secret", JWTOptions{
		Subject:   "browser-demo",
		TTL:       time.Minute,
		Namespace: "demo",
		Agent:     "copilot",
		Channel:   "chat",
	})
	if err != nil {
		t.Fatal(err)
	}
	segments := strings.Split(token, ".")
	if len(segments) != 3 {
		t.Fatalf("expected three JWT segments, got %d", len(segments))
	}

	var header map[string]string
	decodeSegment(t, segments[0], &header)
	if header["alg"] != "HS256" || header["typ"] != "JWT" {
		t.Fatalf("unexpected header: %#v", header)
	}

	var claims map[string]any
	decodeSegment(t, segments[1], &claims)
	if claims["sub"] != "browser-demo" || claims["aud"] != "talon" {
		t.Fatalf("unexpected claims: %#v", claims)
	}
	if claims["talon:ns"] != "demo" || claims["talon:agent"] != "copilot" || claims["talon:channel"] != "chat" {
		t.Fatalf("unexpected scoped claims: %#v", claims)
	}
	headerValue, err := AuthorizationHeader(token)
	if err != nil {
		t.Fatal(err)
	}
	if headerValue != "Bearer "+token {
		t.Fatalf("unexpected authorization header: %s", headerValue)
	}
}

func TestMintJWTRequiresNamespaceForChannel(t *testing.T) {
	if _, err := MintJWT("secret", JWTOptions{Channel: "chat"}); err == nil {
		t.Fatal("expected error")
	}
}

func decodeSegment(t *testing.T, segment string, target any) {
	t.Helper()
	data, err := base64.RawURLEncoding.DecodeString(segment)
	if err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(data, target); err != nil {
		t.Fatal(err)
	}
}
