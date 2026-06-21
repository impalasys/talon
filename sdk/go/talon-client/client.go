package talonclient

import (
	"context"
	"crypto/tls"
	"errors"
	"net/http"
	"strings"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials"
	"google.golang.org/grpc/credentials/insecure"
)

const (
	defaultConnectTimeout = 5 * time.Second
	defaultRequestTimeout = 30 * time.Second
)

type GatewayTransport string

const (
	GatewayTransportGRPC    GatewayTransport = "grpc"
	GatewayTransportGRPCWeb GatewayTransport = "grpc-web"
)

type GatewayClientOptions struct {
	Endpoint       string
	Transport      GatewayTransport
	Authorization  string
	ConnectTimeout time.Duration
	RequestTimeout time.Duration
	HTTPClient     *http.Client
	DialOptions    []grpc.DialOption
}

type GatewayClientOption func(*GatewayClientOptions)

func Connect(ctx context.Context, endpoint string, options ...GatewayClientOption) (*Clientset, error) {
	return ConnectWithOptions(ctx, endpoint, options...)
}

func ConnectGRPCWeb(ctx context.Context, endpoint string, options ...GatewayClientOption) (*Clientset, error) {
	return ConnectWithOptions(ctx, endpoint, append(options, WithGRPCWeb())...)
}

func ConnectWithOptions(ctx context.Context, endpoint string, options ...GatewayClientOption) (*Clientset, error) {
	opts := GatewayClientOptions{Endpoint: endpoint}
	for _, option := range options {
		if option != nil {
			option(&opts)
		}
	}
	if strings.TrimSpace(opts.Endpoint) == "" {
		return nil, errors.New("talon endpoint is required")
	}
	opts = opts.withDefaults()
	switch opts.Transport {
	case "", GatewayTransportGRPC:
		return connectNative(ctx, opts)
	case GatewayTransportGRPCWeb:
		return connectGRPCWeb(opts)
	default:
		return nil, errors.New("unsupported talon transport: " + string(opts.Transport))
	}
}

func WithGRPC() GatewayClientOption {
	return func(opts *GatewayClientOptions) {
		opts.Transport = GatewayTransportGRPC
	}
}

func WithGRPCWeb() GatewayClientOption {
	return func(opts *GatewayClientOptions) {
		opts.Transport = GatewayTransportGRPCWeb
	}
}

func WithAuthorization(authorization string) GatewayClientOption {
	return func(opts *GatewayClientOptions) {
		opts.Authorization = authorization
	}
}

func WithConnectTimeout(timeout time.Duration) GatewayClientOption {
	return func(opts *GatewayClientOptions) {
		opts.ConnectTimeout = timeout
	}
}

func WithRequestTimeout(timeout time.Duration) GatewayClientOption {
	return func(opts *GatewayClientOptions) {
		opts.RequestTimeout = timeout
	}
}

func WithHTTPClient(client *http.Client) GatewayClientOption {
	return func(opts *GatewayClientOptions) {
		opts.HTTPClient = client
	}
}

func WithDialOptions(dialOptions ...grpc.DialOption) GatewayClientOption {
	return func(opts *GatewayClientOptions) {
		opts.DialOptions = append(opts.DialOptions, dialOptions...)
	}
}

func (c *Clientset) Close() error {
	if c == nil || c.close == nil {
		return nil
	}
	return c.close()
}

func (opts GatewayClientOptions) withDefaults() GatewayClientOptions {
	if opts.Transport == "" {
		opts.Transport = GatewayTransportGRPC
	}
	if opts.ConnectTimeout == 0 {
		opts.ConnectTimeout = defaultConnectTimeout
	}
	if opts.RequestTimeout == 0 {
		opts.RequestTimeout = defaultRequestTimeout
	}
	return opts
}

func connectNative(ctx context.Context, opts GatewayClientOptions) (*Clientset, error) {
	target, secure := nativeTarget(opts.Endpoint)
	dialOptions := make([]grpc.DialOption, 0, len(opts.DialOptions)+3)
	if secure {
		dialOptions = append(dialOptions, grpc.WithTransportCredentials(credentials.NewTLS(&tls.Config{
			MinVersion: tls.VersionTLS13,
		})))
	} else {
		if opts.Authorization != "" {
			return nil, errors.New("authorization requires a TLS native gRPC endpoint; use https:// or omit WithAuthorization")
		}
		dialOptions = append(dialOptions, grpc.WithTransportCredentials(insecure.NewCredentials()))
	}
	if opts.Authorization != "" {
		dialOptions = append(dialOptions, grpc.WithPerRPCCredentials(authorizationCredentials(opts.Authorization)))
	}
	dialOptions = append(dialOptions, opts.DialOptions...)
	dialOptions = append(dialOptions, grpc.WithBlock())

	if opts.ConnectTimeout > 0 {
		var cancel context.CancelFunc
		ctx, cancel = context.WithTimeout(ctx, opts.ConnectTimeout)
		defer cancel()
	}
	conn, err := grpc.DialContext(ctx, target, dialOptions...)
	if err != nil {
		return nil, err
	}
	return newClientset(conn, conn.Close), nil
}

func connectGRPCWeb(opts GatewayClientOptions) (*Clientset, error) {
	conn, err := newGRPCWebConn(opts)
	if err != nil {
		return nil, err
	}
	return newClientset(conn, conn.Close), nil
}

func nativeTarget(endpoint string) (target string, secure bool) {
	endpoint = strings.TrimSpace(endpoint)
	if strings.HasPrefix(endpoint, "https://") {
		return strings.TrimPrefix(endpoint, "https://"), true
	}
	if strings.HasPrefix(endpoint, "http://") {
		return strings.TrimPrefix(endpoint, "http://"), false
	}
	return endpoint, true
}

type authorizationCredentials string

func (c authorizationCredentials) GetRequestMetadata(context.Context, ...string) (map[string]string, error) {
	return map[string]string{"authorization": string(c)}, nil
}

func (authorizationCredentials) RequireTransportSecurity() bool {
	return true
}
