package talonserver

import (
	"archive/tar"
	"bytes"
	"compress/gzip"
	"context"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"time"
)

const defaultVersion = "latest"

type Provider struct {
	Name    string
	BaseURL string
	Model   string
	APIKey  string
}

type Options struct {
	TalonNodePath  string
	ConfigPath     string
	Config         map[string]any
	DataDir        string
	Version        string
	GrpcPort       int
	UIPort         int
	KeepTempDir    bool
	Env            map[string]string
	StartupTimeout time.Duration
	Provider       *Provider
	JWTSecret      string
}

type JWTOptions struct {
	Subject   string
	TTL       time.Duration
	Namespace string
	Agent     string
	Session   string
	Channel   string
}

type Server struct {
	process    *exec.Cmd
	tempDir    string
	configPath string
	grpcPort   int
	uiPort     int
	keepTemp   bool
	logs       lockedBuffer
}

type lockedBuffer struct {
	mu   sync.Mutex
	data []byte
}

func (b *lockedBuffer) Write(p []byte) (int, error) {
	b.mu.Lock()
	defer b.mu.Unlock()
	b.data = append(b.data, p...)
	return len(p), nil
}

func (b *lockedBuffer) String() string {
	b.mu.Lock()
	defer b.mu.Unlock()
	return string(b.data)
}

func Start(ctx context.Context, opts Options) (*Server, error) {
	if opts.ConfigPath != "" && (opts.Config != nil || opts.DataDir != "" || opts.Provider != nil) {
		return nil, errors.New("config_path cannot be combined with config, data_dir, or provider; put those settings in the config file")
	}
	if opts.Config != nil && opts.Provider != nil {
		return nil, errors.New("config cannot be combined with provider; put providers in the config object")
	}
	if opts.StartupTimeout == 0 {
		opts.StartupTimeout = 30 * time.Second
	}
	if opts.Version == "" {
		opts.Version = defaultVersion
	}
	nodePath, err := resolveTalonNode(ctx, opts)
	if err != nil {
		return nil, err
	}
	grpcPort := opts.GrpcPort
	if grpcPort == 0 {
		grpcPort, err = freePort()
		if err != nil {
			return nil, err
		}
	}
	uiPort := opts.UIPort
	if uiPort == 0 {
		uiPort, err = freePort()
		if err != nil {
			return nil, err
		}
	}
	tempDir, err := os.MkdirTemp("", "talon-server-")
	if err != nil {
		return nil, err
	}
	configPath := opts.ConfigPath
	if configPath == "" {
		dataDir := opts.DataDir
		if dataDir != "" {
			dataDir, err = filepath.Abs(dataDir)
		}
		if err != nil {
			_ = os.RemoveAll(tempDir)
			return nil, err
		}
		var config map[string]any
		if opts.Config != nil {
			config, err = configWithDataDir(opts.Config, dataDir)
		} else {
			if dataDir == "" {
				dataDir = filepath.Join(tempDir, "data")
			}
			config = defaultConfig(opts.Provider, dataDir)
		}
		if err != nil {
			_ = os.RemoveAll(tempDir)
			return nil, err
		}
		if configDataDir := controlPlaneDataDir(config); configDataDir != "" {
			if !filepath.IsAbs(configDataDir) {
				configDataDir = filepath.Join(tempDir, configDataDir)
			}
			if err := os.MkdirAll(configDataDir, 0o755); err != nil {
				_ = os.RemoveAll(tempDir)
				return nil, err
			}
		}
		data, err := json.MarshalIndent(config, "", "  ")
		if err != nil {
			_ = os.RemoveAll(tempDir)
			return nil, err
		}
		data = append(data, '\n')
		configPath = filepath.Join(tempDir, "talon.json")
		if err := os.WriteFile(configPath, data, 0o600); err != nil {
			_ = os.RemoveAll(tempDir)
			return nil, err
		}
	} else if configPath, err = filepath.Abs(configPath); err != nil {
		_ = os.RemoveAll(tempDir)
		return nil, err
	}

	cmd := exec.CommandContext(ctx, nodePath)
	server := &Server{
		process:    cmd,
		tempDir:    tempDir,
		configPath: configPath,
		grpcPort:   grpcPort,
		uiPort:     uiPort,
		keepTemp:   opts.KeepTempDir,
	}
	cmd.Stdout = &server.logs
	cmd.Stderr = &server.logs
	cmd.Env = append(os.Environ(),
		"GRPC_ADDR=127.0.0.1:"+fmt.Sprint(grpcPort),
		"GATEWAY_UI_ADDR=127.0.0.1:"+fmt.Sprint(uiPort),
		"TALON_CONFIG_PATH="+configPath,
		"RUST_LOG=info",
	)
	if opts.JWTSecret != "" {
		cmd.Env = append(cmd.Env, "GATEWAY_JWT_SECRET="+opts.JWTSecret)
	}
	for k, v := range opts.Env {
		cmd.Env = append(cmd.Env, k+"="+v)
	}
	if err := cmd.Start(); err != nil {
		_ = os.RemoveAll(tempDir)
		return nil, err
	}
	if err := waitForPort(ctx, "127.0.0.1", grpcPort, opts.StartupTimeout); err != nil {
		_ = server.Stop()
		return nil, fmt.Errorf("talon-node did not become ready: %w; logs:\n%s", err, server.Logs())
	}
	return server, nil
}

func (s *Server) GrpcEndpoint() string { return "127.0.0.1:" + fmt.Sprint(s.grpcPort) }
func (s *Server) UIEndpoint() string   { return "http://127.0.0.1:" + fmt.Sprint(s.uiPort) }
func (s *Server) TempDir() string      { return s.tempDir }
func (s *Server) ConfigPath() string   { return s.configPath }
func (s *Server) Logs() string         { return s.logs.String() }

func MintJWT(secret string, opts JWTOptions) (string, error) {
	if secret == "" {
		return "", errors.New("secret is required")
	}
	if opts.Subject == "" {
		opts.Subject = "talon-sdk"
	}
	if strings.TrimSpace(opts.Subject) == "" {
		return "", errors.New("subject is required")
	}
	if opts.TTL == 0 {
		opts.TTL = time.Hour
	}
	if opts.TTL <= 0 {
		return "", errors.New("ttl must be positive")
	}
	if opts.Channel != "" && opts.Namespace == "" {
		return "", errors.New("channel-scoped JWTs require namespace")
	}
	claims := map[string]any{
		"sub": opts.Subject,
		"aud": "talon",
		"exp": time.Now().Add(opts.TTL).Unix(),
	}
	if err := addJWTClaim(claims, "talon:ns", opts.Namespace); err != nil {
		return "", err
	}
	if err := addJWTClaim(claims, "talon:agent", opts.Agent); err != nil {
		return "", err
	}
	if err := addJWTClaim(claims, "talon:session", opts.Session); err != nil {
		return "", err
	}
	if err := addJWTClaim(claims, "talon:channel", opts.Channel); err != nil {
		return "", err
	}

	header, err := jwtSegment(map[string]string{"alg": "HS256", "typ": "JWT"})
	if err != nil {
		return "", err
	}
	payload, err := jwtSegment(claims)
	if err != nil {
		return "", err
	}
	message := header + "." + payload
	mac := hmac.New(sha256.New, []byte(secret))
	_, _ = mac.Write([]byte(message))
	return message + "." + base64.RawURLEncoding.EncodeToString(mac.Sum(nil)), nil
}

func AuthorizationHeader(token string) (string, error) {
	if strings.TrimSpace(token) == "" {
		return "", errors.New("token is required")
	}
	return "Bearer " + token, nil
}

func addJWTClaim(claims map[string]any, key string, value string) error {
	if value == "" {
		return nil
	}
	if strings.TrimSpace(value) == "" {
		return fmt.Errorf("%s must not be empty", key)
	}
	claims[key] = value
	return nil
}

func jwtSegment(value any) (string, error) {
	data, err := json.Marshal(value)
	if err != nil {
		return "", err
	}
	return base64.RawURLEncoding.EncodeToString(data), nil
}

func (s *Server) Stop() error {
	var err error
	if s.process != nil && s.process.Process != nil {
		err = s.process.Process.Signal(os.Interrupt)
		done := make(chan error, 1)
		go func() { done <- s.process.Wait() }()
		select {
		case waitErr := <-done:
			if waitErr != nil && err == nil {
				err = waitErr
			}
		case <-time.After(2 * time.Second):
			_ = s.process.Process.Kill()
			err = <-done
		}
	}
	if !s.keepTemp {
		if rmErr := os.RemoveAll(s.tempDir); rmErr != nil && err == nil {
			err = rmErr
		}
	}
	return err
}

func resolveTalonNode(ctx context.Context, opts Options) (string, error) {
	if opts.TalonNodePath != "" {
		return opts.TalonNodePath, nil
	}
	if env := os.Getenv("TALON_NODE_PATH"); env != "" {
		return env, nil
	}
	return downloadTalonNode(ctx, opts.Version)
}

func downloadTalonNode(ctx context.Context, version string) (string, error) {
	platform, err := platformName()
	if err != nil {
		return "", err
	}
	cache, err := os.UserCacheDir()
	if err != nil {
		return "", err
	}
	targetDir := filepath.Join(cache, "talon", "node", version, platform)
	target := filepath.Join(targetDir, "talon-node")
	if _, err := os.Stat(target); err == nil {
		return target, nil
	}
	if err := os.MkdirAll(targetDir, 0o755); err != nil {
		return "", err
	}
	base := fmt.Sprintf("https://github.com/impalasys/talon/releases/%s/download", version)
	if version != "latest" {
		base = fmt.Sprintf("https://github.com/impalasys/talon/releases/download/%s", version)
	}
	archiveURL := fmt.Sprintf("%s/talon-node-%s.tar.gz", base, platform)
	checksumURL := archiveURL + ".sha256"
	archive, err := httpGet(ctx, archiveURL)
	if err != nil {
		return "", err
	}
	checksum, err := httpGet(ctx, checksumURL)
	if err != nil {
		return "", err
	}
	if err := verifySHA256(archive, string(checksum)); err != nil {
		return "", err
	}
	if err := extractTalonNode(archive, targetDir); err != nil {
		return "", err
	}
	return target, os.Chmod(target, 0o755)
}

func platformName() (string, error) {
	if runtime.GOOS == "linux" && runtime.GOARCH == "amd64" {
		return "linux-x64", nil
	}
	if runtime.GOOS == "darwin" && runtime.GOARCH == "arm64" {
		return "darwin-arm64", nil
	}
	return "", fmt.Errorf("unsupported talon-node platform: %s-%s", runtime.GOOS, runtime.GOARCH)
}

func httpGet(ctx context.Context, url string) ([]byte, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		return nil, err
	}
	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, fmt.Errorf("download %s failed: %s", url, resp.Status)
	}
	return io.ReadAll(resp.Body)
}

func verifySHA256(data []byte, expectedLine string) error {
	expected := strings.Fields(expectedLine)
	if len(expected) == 0 {
		return errors.New("empty checksum")
	}
	sum := sha256.Sum256(data)
	if hex.EncodeToString(sum[:]) != expected[0] {
		return errors.New("talon-node checksum mismatch")
	}
	return nil
}

func extractTalonNode(data []byte, targetDir string) error {
	gz, err := gzip.NewReader(bytes.NewReader(data))
	if err != nil {
		return err
	}
	defer gz.Close()
	tr := tar.NewReader(gz)
	for {
		header, err := tr.Next()
		if errors.Is(err, io.EOF) {
			break
		}
		if err != nil {
			return err
		}
		if filepath.Base(header.Name) != "talon-node" {
			continue
		}
		out, err := os.CreateTemp(targetDir, "talon-node-*")
		if err != nil {
			return err
		}
		tmpPath := out.Name()
		defer os.Remove(tmpPath)
		if err := out.Chmod(0o755); err != nil {
			_ = out.Close()
			return err
		}
		_, copyErr := io.Copy(out, tr)
		closeErr := out.Close()
		if copyErr != nil {
			return copyErr
		}
		if closeErr != nil {
			return closeErr
		}
		return os.Rename(tmpPath, filepath.Join(targetDir, "talon-node"))
	}
	return errors.New("talon-node not found in archive")
}

func freePort() (int, error) {
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		return 0, err
	}
	defer listener.Close()
	return listener.Addr().(*net.TCPAddr).Port, nil
}

func waitForPort(ctx context.Context, host string, port int, timeout time.Duration) error {
	deadline := time.Now().Add(timeout)
	for time.Now().Before(deadline) {
		conn, err := net.DialTimeout("tcp", net.JoinHostPort(host, fmt.Sprint(port)), 250*time.Millisecond)
		if err == nil {
			_ = conn.Close()
			return nil
		}
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-time.After(100 * time.Millisecond):
		}
	}
	return errors.New("timeout")
}

func defaultConfig(provider *Provider, dataDir string) map[string]any {
	config := map[string]any{
		"control_plane": map[string]any{
			"database": map[string]any{
				"driver":   "sqlite",
				"data_dir": dataDir,
			},
			"message_broker": map[string]any{
				"driver": "local_socket",
			},
		},
	}
	if provider != nil {
		name := provider.Name
		if name == "" {
			name = "mock"
		}
		config["providers"] = map[string]any{
			name: map[string]any{
				"type":     "openai_compatible",
				"base_url": provider.BaseURL,
				"model":    provider.Model,
				"api_key":  provider.APIKey,
			},
		}
		config["default_provider"] = name
	}
	return config
}

func configWithDataDir(config map[string]any, dataDir string) (map[string]any, error) {
	data, err := json.Marshal(config)
	if err != nil {
		return nil, err
	}
	var copy map[string]any
	if err := json.Unmarshal(data, &copy); err != nil {
		return nil, err
	}
	if dataDir == "" {
		return copy, nil
	}
	controlPlane := ensureMap(copy, "control_plane")
	database := ensureMap(controlPlane, "database")
	database["data_dir"] = dataDir
	return copy, nil
}

func ensureMap(target map[string]any, key string) map[string]any {
	if value, ok := target[key].(map[string]any); ok {
		return value
	}
	value := map[string]any{}
	target[key] = value
	return value
}

func controlPlaneDataDir(config map[string]any) string {
	controlPlane, ok := config["control_plane"].(map[string]any)
	if !ok {
		return ""
	}
	database, ok := controlPlane["database"].(map[string]any)
	if !ok {
		return ""
	}
	dataDir, ok := database["data_dir"].(string)
	if !ok || strings.TrimSpace(dataDir) == "" {
		return ""
	}
	return dataDir
}
