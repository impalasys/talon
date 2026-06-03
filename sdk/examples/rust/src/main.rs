use talon_client::gateway::{
    gateway_service_client::GatewayServiceClient, CreateNamespaceRequest, ListNamespacesRequest,
};
use talon_server::{Options, Server};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let server = Server::start(Options::default())?;
    let mut client =
        GatewayServiceClient::connect(format!("http://{}", server.grpc_endpoint())).await?;

    client
        .create_namespace(CreateNamespaceRequest {
            name: "example-app".to_string(),
            ..Default::default()
        })
        .await?;

    let response = client
        .list_namespaces(ListNamespacesRequest::default())
        .await?
        .into_inner();
    println!(
        "Talon is running at {} with {} namespace(s)",
        server.grpc_endpoint(),
        response.namespaces.len()
    );

    server.stop()?;
    Ok(())
}

