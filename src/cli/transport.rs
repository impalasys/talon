fn rest_grpc_error_details(headers: &reqwest::header::HeaderMap) -> String {
    let grpc_status = headers
        .get("grpc-status")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty());
    let grpc_message = headers
        .get("grpc-message")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .map(|value| {
            urlencoding::decode(value)
                .map(|decoded| decoded.into_owned())
                .unwrap_or_else(|_| value.to_string())
        });

    match (grpc_status, grpc_message.as_deref()) {
        (Some(status), Some(message)) => {
            format!(" grpc-status={} grpc-message={}", status, message)
        }
        (Some(status), None) => format!(" grpc-status={}", status),
        (None, Some(message)) => format!(" grpc-message={}", message),
        (None, None) => String::new(),
    }
}

pub(super) async fn rest_request_json(
    cli: &Cli,
    method: reqwest::Method,
    path: &str,
    body: Option<serde_json::Value>,
) -> Result<serde_json::Value> {
    let client = rest_client(cli)?;
    let url = format!("{}{}", cli.gateway.trim_end_matches('/'), path);
    let mut request = client.request(method, &url);
    if let Some(payload) = body {
        request = request
            .header(CONTENT_TYPE, "application/json")
            .json(&payload);
    }
    let response = request
        .send()
        .await
        .with_context(|| format!("Failed to call REST endpoint {}", url))?;
    let status = response.status();
    let headers = response.headers().clone();
    let text = response
        .text()
        .await
        .with_context(|| format!("Failed to read REST response body from {}", url))?;
    if !status.is_success() {
        anyhow::bail!(
            "REST {} {} failed: status={} body={}{}",
            path,
            url,
            status,
            text.trim(),
            rest_grpc_error_details(&headers)
        );
    }
    if text.trim().is_empty() {
        return Ok(serde_json::Value::Null);
    }
    serde_json::from_str(&text)
        .with_context(|| format!("Failed to parse REST response JSON from {}", url))
}
