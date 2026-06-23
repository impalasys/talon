package talonclient_test

import (
	"context"
	"encoding/binary"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"strings"
	"testing"
	"time"

	talonclient "github.com/impalasys/talon/sdk/go/talon-client"
	"github.com/impalasys/talon/sdk/go/talon-client/talon/events"
	talonv1 "github.com/impalasys/talon/sdk/go/talon-client/talon/v1"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"
	"google.golang.org/protobuf/proto"
)

func TestGeneratedTalonV1TypesAreAvailable(t *testing.T) {
	req := &talonv1.ListResourcesRequest{Ns: "default", Kind: proto.String("Agent")}
	if req.GetNs() != "default" {
		t.Fatalf("unexpected namespace: %q", req.GetNs())
	}
	if req.GetKind() != "Agent" {
		t.Fatalf("unexpected kind: %q", req.GetKind())
	}
}

func TestClientsetExposesGeneratedServiceClients(t *testing.T) {
	var clientset talonclient.Clientset
	var _ talonv1.NamespaceServiceClient = clientset.Namespaces()
	var _ talonv1.ResourceServiceClient = clientset.Resources()
	var _ talonv1.SessionServiceClient = clientset.Sessions()
	var _ talonv1.ChannelServiceClient = clientset.Channels()
	var _ talonv1.WorkflowServiceClient = clientset.Workflows()
	var _ talonv1.KnowledgeServiceClient = clientset.Knowledge()
	var _ talonv1.AuthServiceClient = clientset.Auth()
}

func TestConnectNativeRejectsPlaintextAuthorization(t *testing.T) {
	_, err := talonclient.ConnectWithOptions(
		context.Background(),
		"http://127.0.0.1:1",
		talonclient.WithAuthorization("Bearer test-token"),
	)
	if err == nil {
		t.Fatalf("expected plaintext authorization to fail")
	}
	if !strings.Contains(err.Error(), "authorization requires a TLS native gRPC endpoint") {
		t.Fatalf("unexpected error: %v", err)
	}
}

func TestConnectGRPCWebDoesNotMutateCallerOptionBackingArray(t *testing.T) {
	sentinel := talonclient.GatewayClientOption(func(opts *talonclient.GatewayClientOptions) {
		opts.Authorization = "sentinel"
	})
	backing := make([]talonclient.GatewayClientOption, 2)
	backing[0] = talonclient.WithRequestTimeout(time.Second)
	backing[1] = sentinel
	options := backing[:1]

	client, err := talonclient.ConnectGRPCWeb(context.Background(), "http://127.0.0.1:1", options...)
	if err != nil {
		t.Fatalf("connect grpc-web: %v", err)
	}
	t.Cleanup(func() {
		if err := client.Close(); err != nil {
			t.Fatalf("close client: %v", err)
		}
	})

	var opts talonclient.GatewayClientOptions
	backing[1](&opts)
	if opts.Authorization != "sentinel" {
		t.Fatalf("ConnectGRPCWeb mutated caller option backing array")
	}
}

func TestSDKGenerationKeepsRootHelpers(t *testing.T) {
	data, err := os.ReadFile("../../../scripts/sdk/generate.sh")
	if err != nil {
		t.Fatalf("read SDK generator: %v", err)
	}
	script := string(data)
	if !strings.Contains(script, "rm -rf sdk/go/talon-client/talon") {
		t.Fatalf("SDK generator no longer removes only the generated Go talon tree")
	}
	if strings.Contains(script, "rm -rf sdk/go/talon-client ") || strings.Contains(script, "rm -rf sdk/go/talon-client\n") {
		t.Fatalf("SDK generator appears to remove the hand-written Go client root")
	}
}

func TestConnectGRPCWebUnaryUsesHTTP1GRPCWeb(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.ProtoMajor != 1 {
			t.Fatalf("expected HTTP/1.x request, got %s", r.Proto)
		}
		if r.Method != http.MethodPost {
			t.Fatalf("expected POST, got %s", r.Method)
		}
		if r.URL.Path != talonv1.NamespaceService_List_FullMethodName {
			t.Fatalf("unexpected path: %s", r.URL.Path)
		}
		if got := r.Header.Get("content-type"); got != "application/grpc-web+proto" {
			t.Fatalf("unexpected content-type: %q", got)
		}
		if got := r.Header.Get("x-grpc-web"); got != "1" {
			t.Fatalf("unexpected x-grpc-web: %q", got)
		}
		if got := r.Header.Get("x-user-agent"); got != "talon-client-go" {
			t.Fatalf("unexpected x-user-agent: %q", got)
		}
		if got := r.Header.Get("authorization"); got != "Bearer test-token" {
			t.Fatalf("authorization header was not forwarded: %q", got)
		}
		if got := r.Header.Get("x-talon-test"); got != "metadata" {
			t.Fatalf("outgoing metadata was not forwarded: %q", got)
		}

		body, err := io.ReadAll(r.Body)
		if err != nil {
			t.Fatalf("read request body: %v", err)
		}
		var request talonv1.ListNamespacesRequest
		decodeRequestFrame(t, body, &request)

		w.Header().Set("content-type", "application/grpc-web+proto")
		w.Header().Set("x-test-header", "header-value")
		_, _ = w.Write(grpcWebBody(
			&talonv1.ListNamespacesResponse{
				Namespaces: []*talonv1.NamespaceResponse{{Name: "from-grpc-web"}},
			},
			"x-test-trailer: trailer-value",
			"grpc-status: 0",
		))
	}))
	defer server.Close()

	ctx := metadata.AppendToOutgoingContext(context.Background(), "x-talon-test", "metadata")
	client, err := talonclient.ConnectWithOptions(
		ctx,
		server.URL,
		talonclient.WithGRPCWeb(),
		talonclient.WithAuthorization("Bearer test-token"),
	)
	if err != nil {
		t.Fatalf("connect gRPC-Web client: %v", err)
	}
	defer client.Close()

	var header metadata.MD
	var trailer metadata.MD
	var finished error
	resp, err := client.Namespaces().List(
		ctx,
		&talonv1.ListNamespacesRequest{},
		grpc.Header(&header),
		grpc.Trailer(&trailer),
		grpc.OnFinish(func(err error) {
			finished = err
		}),
	)
	if err != nil {
		t.Fatalf("list namespaces: %v", err)
	}
	if got := resp.GetNamespaces()[0].GetName(); got != "from-grpc-web" {
		t.Fatalf("unexpected namespace: %q", got)
	}
	if got := header.Get("x-test-header"); len(got) != 1 || got[0] != "header-value" {
		t.Fatalf("unexpected header metadata: %v", header)
	}
	if got := trailer.Get("x-test-trailer"); len(got) != 1 || got[0] != "trailer-value" {
		t.Fatalf("unexpected trailer metadata: %v", trailer)
	}
	if got := trailer.Get("grpc-status"); len(got) != 1 || got[0] != "0" {
		t.Fatalf("unexpected grpc-status trailer: %v", trailer)
	}
	if finished != nil {
		t.Fatalf("unexpected OnFinish error: %v", finished)
	}
}

func TestConnectGRPCWebUnaryMapsTrailerStatus(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("content-type", "application/grpc-web+proto")
		_, _ = w.Write(grpcWebBody(nil, "grpc-status: 7", "grpc-message: permission%20denied"))
	}))
	defer server.Close()

	client, err := talonclient.ConnectGRPCWeb(context.Background(), server.URL)
	if err != nil {
		t.Fatalf("connect gRPC-Web client: %v", err)
	}
	defer client.Close()

	_, err = client.Namespaces().List(context.Background(), &talonv1.ListNamespacesRequest{})
	if status.Code(err) != codes.PermissionDenied {
		t.Fatalf("expected permission denied, got %v (%v)", status.Code(err), err)
	}
	if got := status.Convert(err).Message(); got != "permission denied" {
		t.Fatalf("unexpected status message: %q", got)
	}
}

func TestConnectGRPCWebUnaryRequiresFinalStatus(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("content-type", "application/grpc-web+proto")
		message, err := proto.Marshal(&talonv1.ListNamespacesResponse{})
		if err != nil {
			t.Fatalf("marshal response: %v", err)
		}
		_, _ = w.Write(grpcWebDataFrame(message))
	}))
	defer server.Close()

	client, err := talonclient.ConnectGRPCWeb(context.Background(), server.URL)
	if err != nil {
		t.Fatalf("connect gRPC-Web client: %v", err)
	}
	defer client.Close()

	_, err = client.Namespaces().List(context.Background(), &talonv1.ListNamespacesRequest{})
	if status.Code(err) != codes.Unavailable {
		t.Fatalf("expected unavailable for missing grpc-status, got %v (%v)", status.Code(err), err)
	}
	if !strings.Contains(status.Convert(err).Message(), "missing gRPC status") {
		t.Fatalf("unexpected status message: %q", status.Convert(err).Message())
	}
}

func TestConnectGRPCWebUnaryRejectsTruncatedFrame(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("content-type", "application/grpc-web+proto")
		_, _ = w.Write([]byte{0, 0})
	}))
	defer server.Close()

	client, err := talonclient.ConnectGRPCWeb(context.Background(), server.URL)
	if err != nil {
		t.Fatalf("connect gRPC-Web client: %v", err)
	}
	defer client.Close()

	_, err = client.Namespaces().List(context.Background(), &talonv1.ListNamespacesRequest{})
	if status.Code(err) != codes.Unavailable {
		t.Fatalf("expected unavailable for truncated frame, got %v (%v)", status.Code(err), err)
	}
	if !strings.Contains(status.Convert(err).Message(), "incomplete gRPC-Web frame header") {
		t.Fatalf("unexpected status message: %q", status.Convert(err).Message())
	}
}

func TestConnectGRPCWebUnaryRejectsOversizedMessageFrame(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("content-type", "application/grpc-web+proto")
		_, _ = w.Write(grpcWebDataFrame([]byte("too-large")))
		_, _ = w.Write(grpcWebTrailerFrame("grpc-status: 0"))
	}))
	defer server.Close()

	client, err := talonclient.ConnectGRPCWeb(context.Background(), server.URL)
	if err != nil {
		t.Fatalf("connect gRPC-Web client: %v", err)
	}
	defer client.Close()

	_, err = client.Namespaces().List(context.Background(), &talonv1.ListNamespacesRequest{}, grpc.MaxCallRecvMsgSize(4))
	if status.Code(err) != codes.ResourceExhausted {
		t.Fatalf("expected resource exhausted for oversized frame, got %v (%v)", status.Code(err), err)
	}
}

func TestConnectGRPCWebServerStreamingUsesGeneratedClient(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != talonv1.ChannelService_StreamEvents_FullMethodName {
			t.Fatalf("unexpected path: %s", r.URL.Path)
		}
		body, err := io.ReadAll(r.Body)
		if err != nil {
			t.Fatalf("read request body: %v", err)
		}
		var request talonv1.StreamChannelEventsRequest
		decodeRequestFrame(t, body, &request)
		if request.GetNs() != "default" || request.GetChannel() != "chat" {
			t.Fatalf("unexpected stream request: %+v", request)
		}

		w.Header().Set("content-type", "application/grpc-web+proto")
		_, _ = w.Write(grpcWebBody(
			&events.ChannelEvent{
				Ns:      "default",
				Channel: "chat",
				Kind:    events.ChannelEventKind_CHANNEL_EVENT_KIND_MESSAGE_CREATED,
			},
			&events.ChannelEvent{
				Ns:      "default",
				Channel: "chat",
				Kind:    events.ChannelEventKind_CHANNEL_EVENT_KIND_SESSION_ROUTED,
			},
			"grpc-status: 0",
		))
	}))
	defer server.Close()

	client, err := talonclient.ConnectGRPCWeb(context.Background(), server.URL)
	if err != nil {
		t.Fatalf("connect gRPC-Web client: %v", err)
	}
	defer client.Close()

	stream, err := client.Channels().StreamEvents(context.Background(), &talonv1.StreamChannelEventsRequest{
		Ns:      "default",
		Channel: "chat",
	})
	if err != nil {
		t.Fatalf("stream channel events: %v", err)
	}
	first, err := stream.Recv()
	if err != nil {
		t.Fatalf("receive first event: %v", err)
	}
	if first.GetKind() != events.ChannelEventKind_CHANNEL_EVENT_KIND_MESSAGE_CREATED {
		t.Fatalf("unexpected first event: %v", first.GetKind())
	}
	second, err := stream.Recv()
	if err != nil {
		t.Fatalf("receive second event: %v", err)
	}
	if second.GetKind() != events.ChannelEventKind_CHANNEL_EVENT_KIND_SESSION_ROUTED {
		t.Fatalf("unexpected second event: %v", second.GetKind())
	}
	if _, err := stream.Recv(); err != io.EOF {
		t.Fatalf("expected EOF, got %v", err)
	}
}

func TestConnectGRPCWebServerStreamingRequiresFinalStatus(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("content-type", "application/grpc-web+proto")
		message, err := proto.Marshal(&events.ChannelEvent{
			Ns:      "default",
			Channel: "chat",
			Kind:    events.ChannelEventKind_CHANNEL_EVENT_KIND_MESSAGE_CREATED,
		})
		if err != nil {
			t.Fatalf("marshal event: %v", err)
		}
		_, _ = w.Write(grpcWebDataFrame(message))
	}))
	defer server.Close()

	client, err := talonclient.ConnectGRPCWeb(context.Background(), server.URL)
	if err != nil {
		t.Fatalf("connect gRPC-Web client: %v", err)
	}
	defer client.Close()

	stream, err := client.Channels().StreamEvents(context.Background(), &talonv1.StreamChannelEventsRequest{
		Ns:      "default",
		Channel: "chat",
	})
	if err != nil {
		t.Fatalf("stream channel events: %v", err)
	}
	if _, err := stream.Recv(); err != nil {
		t.Fatalf("receive event: %v", err)
	}
	_, err = stream.Recv()
	if status.Code(err) != codes.Unavailable {
		t.Fatalf("expected unavailable for missing grpc-status, got %v (%v)", status.Code(err), err)
	}
}

func decodeRequestFrame(t *testing.T, body []byte, message proto.Message) {
	t.Helper()
	if len(body) < 5 {
		t.Fatalf("request body too short: %d", len(body))
	}
	if body[0] != 0 {
		t.Fatalf("unexpected request frame flags: 0x%x", body[0])
	}
	length := int(binary.BigEndian.Uint32(body[1:5]))
	if len(body[5:]) != length {
		t.Fatalf("unexpected request frame length: got %d want %d", len(body[5:]), length)
	}
	if err := proto.Unmarshal(body[5:], message); err != nil {
		t.Fatalf("decode request: %v", err)
	}
}

func grpcWebBody(messagesAndTrailer ...any) []byte {
	var body []byte
	for _, item := range messagesAndTrailer {
		switch value := item.(type) {
		case proto.Message:
			message, err := proto.Marshal(value)
			if err != nil {
				panic(err)
			}
			body = append(body, grpcWebDataFrame(message)...)
		case string:
			body = append(body, grpcWebTrailerFrame(messagesAndTrailer...)...)
			return body
		case nil:
		default:
			panic("unsupported gRPC-Web body item")
		}
	}
	return append(body, grpcWebTrailerFrame("grpc-status: 0")...)
}

func grpcWebDataFrame(message []byte) []byte {
	frame := make([]byte, 5+len(message))
	binary.BigEndian.PutUint32(frame[1:5], uint32(len(message)))
	copy(frame[5:], message)
	return frame
}

func grpcWebTrailerFrame(items ...any) []byte {
	var lines []string
	for _, item := range items {
		line, ok := item.(string)
		if ok {
			lines = append(lines, line)
		}
	}
	payload := []byte(strings.Join(lines, "\r\n") + "\r\n")
	frame := make([]byte, 5+len(payload))
	frame[0] = 0x80
	binary.BigEndian.PutUint32(frame[1:5], uint32(len(payload)))
	copy(frame[5:], payload)
	return frame
}
