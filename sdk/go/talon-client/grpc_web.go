package talonclient

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/binary"
	"errors"
	"io"
	"net/http"
	"net/url"
	"strconv"
	"strings"

	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"
	"google.golang.org/protobuf/proto"
)

const (
	defaultMaxReceiveMessageSize = 4 * 1024 * 1024
	defaultMaxTrailerFrameSize   = 4 * 1024 * 1024
	grpcWebContentType           = "application/grpc-web+proto"
	grpcWebUserAgent             = "talon-client-go"
)

type grpcWebConn struct {
	endpoint      string
	authorization string
	client        *http.Client
	ownedClient   bool
}

func newGRPCWebConn(opts GatewayClientOptions) (*grpcWebConn, error) {
	endpoint := strings.TrimRight(strings.TrimSpace(opts.Endpoint), "/")
	if endpoint == "" {
		return nil, errors.New("talon gateway endpoint is required")
	}
	client := opts.HTTPClient
	ownedClient := false
	if client == nil {
		client = &http.Client{Timeout: opts.RequestTimeout}
		ownedClient = true
	}
	return &grpcWebConn{
		endpoint:      endpoint,
		authorization: opts.Authorization,
		client:        client,
		ownedClient:   ownedClient,
	}, nil
}

type grpcWebCallConfig struct {
	maxReceiveMessageSize int
	headerAddr            *metadata.MD
	trailerAddr           *metadata.MD
	onFinish              []func(error)
}

func (c *grpcWebConn) Invoke(ctx context.Context, method string, args any, reply any, opts ...grpc.CallOption) (err error) {
	config := parseGRPCWebCallOptions(opts)
	defer func() {
		for _, onFinish := range config.onFinish {
			onFinish(err)
		}
	}()

	request, err := marshalMessage(args)
	if err != nil {
		return err
	}
	resp, err := c.do(ctx, method, request)
	if err != nil {
		return err
	}
	defer resp.Body.Close()

	headers := headersToMetadata(resp.Header)
	if config.headerAddr != nil {
		*config.headerAddr = headers.Copy()
	}
	reader := grpcWebFrameReader{
		reader:                resp.Body,
		maxReceiveMessageSize: config.maxReceiveMessageSize,
	}
	var trailers metadata.MD
	for {
		frame, err := reader.Next()
		if errors.Is(err, io.EOF) {
			if config.trailerAddr != nil {
				*config.trailerAddr = trailers.Copy()
			}
			return requireStatus(metadata.Join(responseStatusMetadata(resp), trailers))
		}
		if err != nil {
			return err
		}
		if frame.trailer {
			trailers = metadata.Join(trailers, parseTrailerMetadata(frame.payload))
			if config.trailerAddr != nil {
				*config.trailerAddr = trailers.Copy()
			}
			if err := requireStatus(metadata.Join(responseStatusMetadata(resp), trailers)); err != nil {
				return err
			}
			return nil
		}
		if err := unmarshalMessage(frame.payload, reply); err != nil {
			return err
		}
	}
}

func (c *grpcWebConn) NewStream(ctx context.Context, desc *grpc.StreamDesc, method string, opts ...grpc.CallOption) (grpc.ClientStream, error) {
	if desc.ClientStreams {
		return nil, status.Error(codes.Unimplemented, "gRPC-Web transport does not support client-streaming or bidirectional-streaming RPCs")
	}
	return &grpcWebClientStream{
		conn:   c,
		config: parseGRPCWebCallOptions(opts),
		ctx:    ctx,
		method: method,
	}, nil
}

func (c *grpcWebConn) Close() error {
	if c.ownedClient {
		if transport, ok := c.client.Transport.(interface{ CloseIdleConnections() }); ok {
			transport.CloseIdleConnections()
		}
	}
	return nil
}

func (c *grpcWebConn) do(ctx context.Context, method string, message []byte) (*http.Response, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.endpoint+method, bytes.NewReader(encodeDataFrame(message)))
	if err != nil {
		return nil, err
	}
	req.Header.Set("content-type", grpcWebContentType)
	req.Header.Set("accept", grpcWebContentType)
	req.Header.Set("x-grpc-web", "1")
	req.Header.Set("x-user-agent", grpcWebUserAgent)
	if c.authorization != "" {
		req.Header.Set("authorization", c.authorization)
	}
	addOutgoingMetadata(req.Header, ctx)

	resp, err := c.client.Do(req)
	if err != nil {
		return nil, err
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		defer resp.Body.Close()
		return nil, status.Errorf(codes.Unavailable, "gRPC-Web request failed: %s", resp.Status)
	}
	if found, err := statusFromMetadata(responseStatusMetadata(resp)); found && err != nil {
		defer resp.Body.Close()
		return nil, err
	}
	return resp, nil
}

type grpcWebClientStream struct {
	conn    *grpcWebConn
	config  grpcWebCallConfig
	ctx     context.Context
	method  string
	request []byte
	resp    *http.Response
	reader  *grpcWebFrameReader
	header  metadata.MD
	trailer metadata.MD
	started bool
	done    bool
}

func (s *grpcWebClientStream) Header() (metadata.MD, error) {
	if err := s.start(); err != nil {
		return nil, err
	}
	return s.header.Copy(), nil
}

func (s *grpcWebClientStream) Trailer() metadata.MD {
	return s.trailer.Copy()
}

func (s *grpcWebClientStream) CloseSend() error {
	return nil
}

func (s *grpcWebClientStream) Context() context.Context {
	return s.ctx
}

func (s *grpcWebClientStream) SendMsg(m any) error {
	if s.started {
		return errors.New("cannot send gRPC-Web request after stream has started")
	}
	if s.request != nil {
		return errors.New("gRPC-Web server-streaming requests support exactly one request message")
	}
	message, err := marshalMessage(m)
	if err != nil {
		return err
	}
	s.request = message
	return nil
}

func (s *grpcWebClientStream) RecvMsg(m any) error {
	if s.done {
		return io.EOF
	}
	if err := s.start(); err != nil {
		return err
	}
	for {
		frame, err := s.reader.Next()
		if errors.Is(err, io.EOF) {
			s.done = true
			if s.resp != nil {
				_ = s.resp.Body.Close()
			}
			if err := requireStatus(metadata.Join(responseStatusMetadata(s.resp), s.trailer)); err != nil {
				s.finish(err)
				return err
			}
			s.finish(nil)
			return io.EOF
		}
		if err != nil {
			s.done = true
			if s.resp != nil {
				_ = s.resp.Body.Close()
			}
			s.finish(err)
			return err
		}
		if frame.trailer {
			s.trailer = metadata.Join(s.trailer, parseTrailerMetadata(frame.payload))
			if s.config.trailerAddr != nil {
				*s.config.trailerAddr = s.trailer.Copy()
			}
			s.done = true
			if s.resp != nil {
				_ = s.resp.Body.Close()
			}
			if err := requireStatus(metadata.Join(responseStatusMetadata(s.resp), s.trailer)); err != nil {
				s.finish(err)
				return err
			}
			s.finish(nil)
			return io.EOF
		}
		return unmarshalMessage(frame.payload, m)
	}
}

func (s *grpcWebClientStream) start() error {
	if s.started {
		return nil
	}
	resp, err := s.conn.do(s.ctx, s.method, s.request)
	if err != nil {
		return err
	}
	s.resp = resp
	reader := grpcWebFrameReader{
		reader:                resp.Body,
		maxReceiveMessageSize: s.config.maxReceiveMessageSize,
	}
	s.reader = &reader
	s.header = headersToMetadata(resp.Header)
	if s.config.headerAddr != nil {
		*s.config.headerAddr = s.header.Copy()
	}
	s.started = true
	return nil
}

func (s *grpcWebClientStream) finish(err error) {
	for _, onFinish := range s.config.onFinish {
		onFinish(err)
	}
	s.config.onFinish = nil
}

type grpcWebFrame struct {
	trailer bool
	payload []byte
}

type grpcWebFrameReader struct {
	reader                io.Reader
	maxReceiveMessageSize int
}

func (r *grpcWebFrameReader) Next() (grpcWebFrame, error) {
	var header [5]byte
	if _, err := io.ReadFull(r.reader, header[:]); err != nil {
		if errors.Is(err, io.EOF) {
			return grpcWebFrame{}, io.EOF
		}
		if errors.Is(err, io.ErrUnexpectedEOF) {
			return grpcWebFrame{}, status.Error(codes.Unavailable, "incomplete gRPC-Web frame header")
		}
		return grpcWebFrame{}, err
	}
	compressed := header[0]&0x01 != 0
	if compressed {
		return grpcWebFrame{}, status.Error(codes.Unimplemented, "compressed gRPC-Web frames are not supported")
	}
	trailer := header[0]&0x80 != 0
	length := binary.BigEndian.Uint32(header[1:])
	maxFrameSize := r.maxReceiveMessageSize
	frameKind := "message"
	if trailer {
		maxFrameSize = defaultMaxTrailerFrameSize
		frameKind = "trailer"
	}
	if maxFrameSize >= 0 && uint64(length) > uint64(maxFrameSize) {
		return grpcWebFrame{}, status.Errorf(codes.ResourceExhausted, "gRPC-Web %s frame length %d exceeds maximum %d", frameKind, length, maxFrameSize)
	}
	payload := make([]byte, int(length))
	if _, err := io.ReadFull(r.reader, payload); err != nil {
		if errors.Is(err, io.EOF) || errors.Is(err, io.ErrUnexpectedEOF) {
			return grpcWebFrame{}, status.Error(codes.Unavailable, "incomplete gRPC-Web frame payload")
		}
		return grpcWebFrame{}, err
	}
	return grpcWebFrame{
		trailer: trailer,
		payload: payload,
	}, nil
}

func encodeDataFrame(message []byte) []byte {
	frame := make([]byte, 5+len(message))
	binary.BigEndian.PutUint32(frame[1:5], uint32(len(message)))
	copy(frame[5:], message)
	return frame
}

func marshalMessage(v any) ([]byte, error) {
	message, ok := v.(proto.Message)
	if !ok {
		return nil, status.Errorf(codes.Internal, "gRPC-Web request %T does not implement proto.Message", v)
	}
	return proto.Marshal(message)
}

func unmarshalMessage(data []byte, v any) error {
	message, ok := v.(proto.Message)
	if !ok {
		return status.Errorf(codes.Internal, "gRPC-Web response %T does not implement proto.Message", v)
	}
	return proto.Unmarshal(data, message)
}

func parseGRPCWebCallOptions(opts []grpc.CallOption) grpcWebCallConfig {
	config := grpcWebCallConfig{maxReceiveMessageSize: defaultMaxReceiveMessageSize}
	for _, opt := range opts {
		switch typed := opt.(type) {
		case grpc.HeaderCallOption:
			config.headerAddr = typed.HeaderAddr
		case grpc.TrailerCallOption:
			config.trailerAddr = typed.TrailerAddr
		case grpc.MaxRecvMsgSizeCallOption:
			config.maxReceiveMessageSize = typed.MaxRecvMsgSize
		case grpc.OnFinishCallOption:
			if typed.OnFinish != nil {
				config.onFinish = append(config.onFinish, typed.OnFinish)
			}
		}
	}
	return config
}

func addOutgoingMetadata(headers http.Header, ctx context.Context) {
	md, ok := metadata.FromOutgoingContext(ctx)
	if !ok {
		return
	}
	for key, values := range md {
		key = strings.ToLower(key)
		if key == "" || strings.HasPrefix(key, ":") || strings.HasPrefix(key, "grpc-") {
			continue
		}
		for _, value := range values {
			if strings.HasSuffix(key, "-bin") {
				value = base64.StdEncoding.EncodeToString([]byte(value))
			}
			headers.Add(key, value)
		}
	}
}

func headersToMetadata(headers http.Header) metadata.MD {
	md := metadata.MD{}
	for key, values := range headers {
		key = strings.ToLower(key)
		if key == "" || key == "content-type" {
			continue
		}
		for _, value := range values {
			md.Append(key, value)
		}
	}
	return md
}

func responseStatusMetadata(resp *http.Response) metadata.MD {
	if resp == nil {
		return nil
	}
	md := metadata.MD{}
	for _, key := range []string{"grpc-status", "grpc-message"} {
		for _, value := range resp.Header.Values(key) {
			md.Append(key, value)
		}
	}
	return md
}

func parseTrailerMetadata(payload []byte) metadata.MD {
	md := metadata.MD{}
	for _, line := range strings.Split(string(payload), "\r\n") {
		if line == "" {
			continue
		}
		key, value, ok := strings.Cut(line, ":")
		if !ok {
			continue
		}
		md.Append(strings.ToLower(strings.TrimSpace(key)), strings.TrimSpace(value))
	}
	return md
}

func requireStatus(md metadata.MD) error {
	found, err := statusFromMetadata(md)
	if !found {
		return status.Error(codes.Unavailable, "missing gRPC status in gRPC-Web response")
	}
	return err
}

func statusFromMetadata(md metadata.MD) (bool, error) {
	values := md.Get("grpc-status")
	if len(values) == 0 {
		return false, nil
	}
	codeNumber, err := strconv.Atoi(values[len(values)-1])
	if err != nil {
		return true, status.Errorf(codes.Internal, "invalid grpc-status %q", values[len(values)-1])
	}
	code := codes.Code(codeNumber)
	if code == codes.OK {
		return true, nil
	}
	message := ""
	if messages := md.Get("grpc-message"); len(messages) > 0 {
		message = decodeGRPCMessage(messages[len(messages)-1])
	}
	return true, status.Error(code, message)
}

func decodeGRPCMessage(message string) string {
	decoded, err := url.PathUnescape(message)
	if err != nil {
		return message
	}
	return decoded
}
