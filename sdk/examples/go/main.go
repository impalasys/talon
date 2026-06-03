package main

import (
	"context"
	"fmt"
	"log"
	"time"

	"github.com/impalasys/talon/sdk/go/talon-client/talon/gateway"
	talonserver "github.com/impalasys/talon/sdk/go/talon-server"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
)

func main() {
	ctx, cancel := context.WithTimeout(context.Background(), 45*time.Second)
	defer cancel()

	server, err := talonserver.Start(ctx, talonserver.Options{})
	if err != nil {
		log.Fatalf("start Talon server: %v", err)
	}
	defer func() {
		if err := server.Stop(); err != nil {
			log.Printf("stop Talon server: %v", err)
		}
	}()

	conn, err := grpc.NewClient(server.GrpcEndpoint(), grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		log.Fatalf("connect to Talon gateway: %v", err)
	}
	defer conn.Close()

	client := gateway.NewGatewayServiceClient(conn)
	if _, err := client.CreateNamespace(ctx, &gateway.CreateNamespaceRequest{Name: "example-app"}); err != nil {
		log.Fatalf("create namespace: %v", err)
	}

	resp, err := client.ListNamespaces(ctx, &gateway.ListNamespacesRequest{})
	if err != nil {
		log.Fatalf("list namespaces: %v", err)
	}
	fmt.Printf("Talon is running at %s with %d namespace(s)\n", server.GrpcEndpoint(), len(resp.GetNamespaces()))
}

