package talonserver

import (
	"context"
	"crypto/rsa"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"math/big"
	"net/http"
	"strings"
	"sync"
	"time"

	"github.com/golang-jwt/jwt/v5"
)

const (
	DefaultJWTIssuer        = "https://talon.impala.systems"
	DefaultJWKSURL          = DefaultJWTIssuer + "/.well-known/jwks.json"
	TalonGatewayAudience    = "talon.impala.systems"
	MCPAudience             = "mcps.talon.impala.systems"
	defaultJWKSRefreshAfter = 5 * time.Minute
	defaultJWKSHTTPTimeout  = 10 * time.Second
	maxJWKSResponseBytes    = 1 << 20
	minRSAModulusBits       = 2048
)

// JWTVerifierConfig configures verification of Talon-issued platform JWTs.
type JWTVerifierConfig struct {
	Issuer       string
	JWKSURL      string
	Audience     string
	HTTPClient   *http.Client
	RefreshAfter time.Duration
	Leeway       time.Duration
}

// JWTVerifier verifies Talon-issued RS256 platform JWTs against Talon's JWKS.
type JWTVerifier struct {
	issuer       string
	jwksURL      string
	audience     string
	httpClient   *http.Client
	refreshAfter time.Duration
	leeway       time.Duration

	mu        sync.Mutex
	keys      map[string]*rsa.PublicKey
	fetchedAt time.Time
}

// MCPClaims are the claims Talon signs when calling MCPs.
type MCPClaims struct {
	jwt.RegisteredClaims
	Namespace string `json:"talon:ns"`
	MCPServer string `json:"talon:mcp_server"`
	Agent     string `json:"talon:agent,omitempty"`
}

// NewJWTVerifier returns a verifier for a specific Talon platform JWT audience.
func NewJWTVerifier(config JWTVerifierConfig) (*JWTVerifier, error) {
	issuer := strings.TrimSpace(config.Issuer)
	if issuer == "" {
		issuer = DefaultJWTIssuer
	}
	jwksURL := strings.TrimSpace(config.JWKSURL)
	if jwksURL == "" {
		jwksURL = DefaultJWKSURL
	}
	audience := strings.TrimSpace(config.Audience)
	if audience == "" {
		return nil, errors.New("talon JWT audience is required")
	}
	httpClient := config.HTTPClient
	if httpClient == nil {
		httpClient = &http.Client{Timeout: defaultJWKSHTTPTimeout}
	}
	refreshAfter := config.RefreshAfter
	if refreshAfter == 0 {
		refreshAfter = defaultJWKSRefreshAfter
	}
	if refreshAfter < 0 {
		return nil, errors.New("talon JWT refresh_after must not be negative")
	}
	if config.Leeway < 0 {
		return nil, errors.New("talon JWT leeway must not be negative")
	}
	return &JWTVerifier{
		issuer:       issuer,
		jwksURL:      jwksURL,
		audience:     audience,
		httpClient:   httpClient,
		refreshAfter: refreshAfter,
		leeway:       config.Leeway,
		keys:         map[string]*rsa.PublicKey{},
	}, nil
}

// NewMCPJWTVerifier returns a verifier for Talon MCP assertions.
func NewMCPJWTVerifier(config JWTVerifierConfig) (*JWTVerifier, error) {
	if strings.TrimSpace(config.Audience) != "" && strings.TrimSpace(config.Audience) != MCPAudience {
		return nil, fmt.Errorf("MCP verifier requires audience %q", MCPAudience)
	}
	config.Audience = MCPAudience
	return NewJWTVerifier(config)
}

// Verify verifies rawToken into claims.
func (v *JWTVerifier) Verify(ctx context.Context, rawToken string, claims jwt.Claims) error {
	if v == nil {
		return errors.New("talon JWT verifier is nil")
	}
	if strings.TrimSpace(rawToken) == "" {
		return errors.New("talon JWT is required")
	}
	if claims == nil {
		return errors.New("talon JWT claims are required")
	}
	parserOptions := []jwt.ParserOption{
		jwt.WithValidMethods([]string{jwt.SigningMethodRS256.Alg()}),
		jwt.WithIssuer(v.issuer),
		jwt.WithAudience(v.audience),
		jwt.WithExpirationRequired(),
		jwt.WithIssuedAt(),
	}
	if v.leeway > 0 {
		parserOptions = append(parserOptions, jwt.WithLeeway(v.leeway))
	}
	token, err := jwt.ParseWithClaims(rawToken, claims, v.keyfunc(ctx), parserOptions...)
	if err != nil {
		return err
	}
	if token == nil || !token.Valid {
		return errors.New("talon JWT is invalid")
	}
	return nil
}

// VerifyMCP verifies a Talon MCP assertion and returns typed claims.
func (v *JWTVerifier) VerifyMCP(ctx context.Context, rawToken string) (*MCPClaims, error) {
	claims := &MCPClaims{}
	if err := v.Verify(ctx, rawToken, claims); err != nil {
		return nil, err
	}
	if strings.TrimSpace(claims.Namespace) == "" {
		return nil, errors.New("talon MCP JWT is missing talon:ns")
	}
	if strings.TrimSpace(claims.MCPServer) == "" {
		return nil, errors.New("talon MCP JWT is missing talon:mcp_server")
	}
	return claims, nil
}

func (v *JWTVerifier) keyfunc(ctx context.Context) jwt.Keyfunc {
	return func(token *jwt.Token) (any, error) {
		if token.Method.Alg() != jwt.SigningMethodRS256.Alg() {
			return nil, fmt.Errorf("unexpected talon JWT signing method %s", token.Method.Alg())
		}
		kid, _ := token.Header["kid"].(string)
		kid = strings.TrimSpace(kid)
		if kid == "" {
			return nil, errors.New("talon JWT kid is required")
		}
		return v.key(ctx, kid)
	}
}

func (v *JWTVerifier) key(ctx context.Context, kid string) (*rsa.PublicKey, error) {
	v.mu.Lock()
	defer v.mu.Unlock()
	if key := v.keys[kid]; key != nil && !v.shouldRefreshLocked() {
		return key, nil
	}
	if err := v.refreshLocked(ctx); err != nil {
		return nil, err
	}
	if key := v.keys[kid]; key != nil {
		return key, nil
	}
	return nil, fmt.Errorf("talon JWKS does not contain kid %q", kid)
}

func (v *JWTVerifier) shouldRefreshLocked() bool {
	return v.fetchedAt.IsZero() || (v.refreshAfter > 0 && time.Since(v.fetchedAt) >= v.refreshAfter)
}

func (v *JWTVerifier) refreshLocked(ctx context.Context) error {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, v.jwksURL, nil)
	if err != nil {
		return err
	}
	resp, err := v.httpClient.Do(req)
	if err != nil {
		return fmt.Errorf("fetch talon JWKS: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return fmt.Errorf("fetch talon JWKS: unexpected status %s", resp.Status)
	}
	var jwks jwksResponse
	if err := json.NewDecoder(io.LimitReader(resp.Body, maxJWKSResponseBytes)).Decode(&jwks); err != nil {
		return fmt.Errorf("decode talon JWKS: %w", err)
	}
	keys := make(map[string]*rsa.PublicKey, len(jwks.Keys))
	for _, jwk := range jwks.Keys {
		key, err := jwk.publicKey()
		if err != nil {
			return err
		}
		keys[strings.TrimSpace(jwk.Kid)] = key
	}
	if len(keys) == 0 {
		return errors.New("talon JWKS did not contain any usable keys")
	}
	v.keys = keys
	v.fetchedAt = time.Now()
	return nil
}

type jwksResponse struct {
	Keys []jwkKey `json:"keys"`
}

type jwkKey struct {
	Kty string `json:"kty"`
	Use string `json:"use,omitempty"`
	Kid string `json:"kid"`
	Alg string `json:"alg,omitempty"`
	N   string `json:"n"`
	E   string `json:"e"`
}

func (j jwkKey) publicKey() (*rsa.PublicKey, error) {
	if j.Kty != "RSA" {
		return nil, fmt.Errorf("talon JWKS key %q has unsupported kty %q", j.Kid, j.Kty)
	}
	if j.Use != "" && j.Use != "sig" {
		return nil, fmt.Errorf("talon JWKS key %q has unsupported use %q", j.Kid, j.Use)
	}
	if j.Alg != "" && j.Alg != jwt.SigningMethodRS256.Alg() {
		return nil, fmt.Errorf("talon JWKS key %q has unsupported alg %q", j.Kid, j.Alg)
	}
	if strings.TrimSpace(j.Kid) == "" {
		return nil, errors.New("talon JWKS key is missing kid")
	}
	n, err := decodeBase64URLUInt(j.N)
	if err != nil {
		return nil, fmt.Errorf("talon JWKS key %q has invalid modulus: %w", j.Kid, err)
	}
	if n.BitLen() < minRSAModulusBits {
		return nil, fmt.Errorf("talon JWKS key %q modulus is too small: %d bits", j.Kid, n.BitLen())
	}
	e, err := decodeBase64URLUInt(j.E)
	if err != nil {
		return nil, fmt.Errorf("talon JWKS key %q has invalid exponent: %w", j.Kid, err)
	}
	if !e.IsInt64() || e.Sign() <= 0 {
		return nil, fmt.Errorf("talon JWKS key %q has invalid exponent", j.Kid)
	}
	return &rsa.PublicKey{N: n, E: int(e.Int64())}, nil
}

func decodeBase64URLUInt(value string) (*big.Int, error) {
	value = strings.TrimSpace(value)
	if value == "" {
		return nil, errors.New("value is empty")
	}
	decoded, err := base64.RawURLEncoding.DecodeString(value)
	if err != nil {
		return nil, err
	}
	integer := new(big.Int).SetBytes(decoded)
	if integer.Sign() <= 0 {
		return nil, errors.New("value must be positive")
	}
	return integer, nil
}
