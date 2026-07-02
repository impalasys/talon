package talonserver

import (
	"context"
	"crypto/rand"
	"crypto/rsa"
	"encoding/base64"
	"encoding/json"
	"math/big"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/golang-jwt/jwt/v5"
)

func TestVerifyMCPJWT(t *testing.T) {
	privateKey := testRSAKey(t)
	kid := "test-key"
	jwksServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/.well-known/jwks.json" {
			http.NotFound(w, r)
			return
		}
		_ = json.NewEncoder(w).Encode(testJWKS(&privateKey.PublicKey, kid))
	}))
	t.Cleanup(jwksServer.Close)

	verifier, err := NewMCPJWTVerifier(JWTVerifierConfig{
		Issuer:  "https://talon.example.com",
		JWKSURL: jwksServer.URL + "/.well-known/jwks.json",
		Leeway:  time.Second,
	})
	if err != nil {
		t.Fatal(err)
	}
	token := signMCPToken(t, privateKey, kid, "https://talon.example.com", MCPAudience)

	claims, err := verifier.VerifyMCP(context.Background(), token)
	if err != nil {
		t.Fatal(err)
	}
	if claims.Namespace != "Tenant:conic:Customers:11" {
		t.Fatalf("unexpected namespace %q", claims.Namespace)
	}
	if claims.MCPServer != "conic" {
		t.Fatalf("unexpected MCP server %q", claims.MCPServer)
	}
	if claims.Agent != "cmo" {
		t.Fatalf("unexpected agent %q", claims.Agent)
	}
}

func TestMCPJWTVerifierRejectsWrongAudience(t *testing.T) {
	privateKey := testRSAKey(t)
	kid := "test-key"
	jwksServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(testJWKS(&privateKey.PublicKey, kid))
	}))
	t.Cleanup(jwksServer.Close)

	verifier, err := NewMCPJWTVerifier(JWTVerifierConfig{
		Issuer:  "https://talon.example.com",
		JWKSURL: jwksServer.URL,
	})
	if err != nil {
		t.Fatal(err)
	}
	token := signMCPToken(t, privateKey, kid, "https://talon.example.com", TalonGatewayAudience)

	if _, err := verifier.VerifyMCP(context.Background(), token); err == nil {
		t.Fatal("expected wrong audience to be rejected")
	}
}

func TestMCPJWTVerifierRejectsHS256(t *testing.T) {
	verifier, err := NewMCPJWTVerifier(JWTVerifierConfig{
		Issuer:  "https://talon.example.com",
		JWKSURL: "http://127.0.0.1/.well-known/jwks.json",
	})
	if err != nil {
		t.Fatal(err)
	}
	claims := jwt.MapClaims{
		"iss":              "https://talon.example.com",
		"sub":              "talon-mcp-client",
		"aud":              MCPAudience,
		"exp":              time.Now().Add(time.Minute).Unix(),
		"iat":              time.Now().Unix(),
		"talon:ns":         "Tenant:conic:Customers:11",
		"talon:mcp_server": "conic",
	}
	token, err := jwt.NewWithClaims(jwt.SigningMethodHS256, claims).SignedString([]byte("secret"))
	if err != nil {
		t.Fatal(err)
	}

	if _, err := verifier.VerifyMCP(context.Background(), token); err == nil || !strings.Contains(err.Error(), "signing method") {
		t.Fatalf("expected signing method error, got %v", err)
	}
}

func TestMCPJWTVerifierRequiresMCPAudience(t *testing.T) {
	_, err := NewMCPJWTVerifier(JWTVerifierConfig{Audience: TalonGatewayAudience})
	if err == nil {
		t.Fatal("expected audience mismatch error")
	}
}

func testRSAKey(t *testing.T) *rsa.PrivateKey {
	t.Helper()
	key, err := rsa.GenerateKey(rand.Reader, minRSAModulusBits)
	if err != nil {
		t.Fatal(err)
	}
	return key
}

func signMCPToken(t *testing.T, key *rsa.PrivateKey, kid, issuer, audience string) string {
	t.Helper()
	now := time.Now()
	claims := MCPClaims{
		RegisteredClaims: jwt.RegisteredClaims{
			Issuer:    issuer,
			Subject:   "talon-mcp-client",
			Audience:  jwt.ClaimStrings{audience},
			ExpiresAt: jwt.NewNumericDate(now.Add(time.Minute)),
			IssuedAt:  jwt.NewNumericDate(now),
		},
		Namespace: "Tenant:conic:Customers:11",
		MCPServer: "conic",
		Agent:     "cmo",
	}
	token := jwt.NewWithClaims(jwt.SigningMethodRS256, claims)
	token.Header["kid"] = kid
	signed, err := token.SignedString(key)
	if err != nil {
		t.Fatal(err)
	}
	return signed
}

func testJWKS(key *rsa.PublicKey, kid string) jwksResponse {
	return jwksResponse{Keys: []jwkKey{{
		Kty: "RSA",
		Use: "sig",
		Kid: kid,
		Alg: jwt.SigningMethodRS256.Alg(),
		N:   base64.RawURLEncoding.EncodeToString(key.N.Bytes()),
		E:   base64.RawURLEncoding.EncodeToString(big.NewInt(int64(key.E)).Bytes()),
	}}}
}
