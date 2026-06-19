pub(super) async fn grpc_delete_resource(
    cli: &Cli,
    kind: &str,
    name: &str,
    namespace: Option<&String>,
) -> Result<String> {
    let channel = tonic::transport::Channel::from_shared(cli.gateway.clone())
        .with_context(|| format!("Invalid gateway URL {}", cli.gateway))?
        .connect()
        .await
        .with_context(|| format!("Could not connect to gateway at {}", cli.gateway))?;
    let mut client = GatewayServiceClient::with_interceptor(channel, auth_interceptor(cli)?);

    let (ns, kind, name) = resource_lookup_target(kind, name, namespace)?;
    client
        .delete_resource(DeleteResourceRequest {
            ns: ns.clone(),
            kind: kind.clone(),
            name: name.clone(),
        })
        .await
        .with_context(|| format!("Failed to delete {} '{}/{}'", kind, ns, name))?;
    Ok(format!(
        "✓ {} '{}/{}' deleted successfully.",
        kind, ns, name
    ))
}

pub(super) async fn rest_delete_resource(
    cli: &Cli,
    kind: &str,
    name: &str,
    namespace: Option<&String>,
) -> Result<String> {
    let path = rest_delete_path(kind, name, namespace)?;
    rest_request_json(cli, reqwest::Method::DELETE, &path, None)
        .await
        .with_context(|| format!("Failed to delete {} '{}'", kind, name))?;
    Ok(format!("✓ {} '{}' deleted successfully.", kind, name))
}
