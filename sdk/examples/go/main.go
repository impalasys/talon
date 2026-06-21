package main

import (
	"context"
	"fmt"
	"log"
	"time"

	talonclient "github.com/impalasys/talon/sdk/go/talon-client"
	talonv1 "github.com/impalasys/talon/sdk/go/talon-client/talon/v1"
	talonserver "github.com/impalasys/talon/sdk/go/talon-server"
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

	client, err := talonclient.Connect(ctx, server.GrpcEndpoint())
	if err != nil {
		log.Fatalf("connect to Talon gateway: %v", err)
	}
	defer client.Close()

	if _, err := client.Namespaces().Create(ctx, &talonv1.CreateNamespaceRequest{Name: "example-app"}); err != nil {
		log.Fatalf("create namespace: %v", err)
	}

	resp, err := client.Namespaces().List(ctx, &talonv1.ListNamespacesRequest{})
	if err != nil {
		log.Fatalf("list namespaces: %v", err)
	}
	fmt.Printf("Talon is running at %s with %d namespace(s)\n", server.GrpcEndpoint(), len(resp.GetNamespaces()))
}
