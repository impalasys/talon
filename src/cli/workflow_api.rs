pub(super) fn read_json_arg(value: &Option<String>, file: &Option<String>) -> Result<String> {
    let raw = if let Some(file) = file {
        fs::read_to_string(file).with_context(|| format!("Failed to read JSON file '{}'", file))?
    } else {
        value.clone().unwrap_or_else(|| "{}".to_string())
    };
    let parsed: serde_json::Value =
        serde_json::from_str(&raw).context("Argument must be valid JSON")?;
    serde_json::to_string(&parsed).context("Failed to normalize JSON argument")
}

pub(super) async fn workflow_run_create(
    cli: &Cli,
    namespace: &str,
    workflow: &str,
    input_json: String,
) -> Result<serde_json::Value> {
    if cli.rest {
        return rest_request_json(
            cli,
            reqwest::Method::POST,
            &format!(
                "/v1/ns/{}/workflows/{}/runs",
                urlencoding::encode(namespace),
                urlencoding::encode(workflow)
            ),
            Some(json!({ "ns": namespace, "workflow": workflow, "inputJson": input_json })),
        )
        .await;
    }
    let mut client = connect_gateway(cli).await?;
    let response = client
        .create_workflow_run(CreateWorkflowRunRequest {
            ns: namespace.to_string(),
            workflow: workflow.to_string(),
            input_json,
            labels: HashMap::new(),
        })
        .await
        .context("Failed to create workflow run")?
        .into_inner();
    let run = response.run.context("Workflow run missing from response")?;
    Ok(workflow_run_json(&run, &response.steps))
}

pub(super) async fn workflow_run_get(
    cli: &Cli,
    namespace: &str,
    workflow: &str,
    run_id: &str,
) -> Result<serde_json::Value> {
    if cli.rest {
        return rest_request_json(
            cli,
            reqwest::Method::GET,
            &format!(
                "/v1/ns/{}/workflows/{}/runs/{}",
                urlencoding::encode(namespace),
                urlencoding::encode(workflow),
                urlencoding::encode(run_id)
            ),
            None,
        )
        .await;
    }
    let mut client = connect_gateway(cli).await?;
    let response = client
        .get_workflow_run(GetWorkflowRunRequest {
            ns: namespace.to_string(),
            workflow: workflow.to_string(),
            run_id: run_id.to_string(),
        })
        .await
        .context("Failed to get workflow run")?
        .into_inner();
    let run = response.run.context("Workflow run missing from response")?;
    Ok(workflow_run_json(&run, &response.steps))
}

pub(super) async fn workflow_run_list(
    cli: &Cli,
    namespace: &str,
    workflow: &str,
    page_size: i32,
    before_run_id: &str,
) -> Result<serde_json::Value> {
    if cli.rest {
        let mut query = Vec::new();
        if page_size != 0 {
            query.push(format!("page_size={page_size}"));
        }
        if !before_run_id.is_empty() {
            query.push(format!(
                "before_run_id={}",
                urlencoding::encode(before_run_id)
            ));
        }
        let query = if query.is_empty() {
            String::new()
        } else {
            format!("?{}", query.join("&"))
        };
        return rest_request_json(
            cli,
            reqwest::Method::GET,
            &format!(
                "/v1/ns/{}/workflows/{}/runs{}",
                urlencoding::encode(namespace),
                urlencoding::encode(workflow),
                query
            ),
            None,
        )
        .await;
    }
    let mut client = connect_gateway(cli).await?;
    let response = client
        .list_workflow_runs(ListWorkflowRunsRequest {
            ns: namespace.to_string(),
            workflow: workflow.to_string(),
            page_size,
            before_run_id: before_run_id.to_string(),
        })
        .await
        .context("Failed to list workflow runs")?
        .into_inner();
    Ok(json!({
        "runs": response.runs.iter().map(|run| workflow_run_json(run, &[])).collect::<Vec<_>>(),
        "hasMore": response.has_more,
        "nextBeforeRunId": response.next_before_run_id,
    }))
}

pub(super) async fn workflow_run_resume(
    cli: &Cli,
    namespace: &str,
    workflow: &str,
    run_id: &str,
    step_id: &str,
    resume_json: String,
) -> Result<serde_json::Value> {
    if cli.rest {
        return rest_request_json(
            cli,
            reqwest::Method::POST,
            &format!(
                "/v1/ns/{}/workflows/{}/runs/{}:resume",
                urlencoding::encode(namespace),
                urlencoding::encode(workflow),
                urlencoding::encode(run_id)
            ),
            Some(json!({
                "ns": namespace,
                "workflow": workflow,
                "runId": run_id,
                "stepId": step_id,
                "resumeJson": resume_json,
            })),
        )
        .await;
    }
    let mut client = connect_gateway(cli).await?;
    let response = client
        .resume_workflow_run(ResumeWorkflowRunRequest {
            ns: namespace.to_string(),
            workflow: workflow.to_string(),
            run_id: run_id.to_string(),
            step_id: step_id.to_string(),
            resume_json,
        })
        .await
        .context("Failed to resume workflow run")?
        .into_inner();
    let run = response.run.context("Workflow run missing from response")?;
    Ok(workflow_run_json(&run, &response.steps))
}

pub(super) async fn workflow_run_cancel(
    cli: &Cli,
    namespace: &str,
    workflow: &str,
    run_id: &str,
) -> Result<serde_json::Value> {
    if cli.rest {
        return rest_request_json(
            cli,
            reqwest::Method::POST,
            &format!(
                "/v1/ns/{}/workflows/{}/runs/{}:cancel",
                urlencoding::encode(namespace),
                urlencoding::encode(workflow),
                urlencoding::encode(run_id)
            ),
            Some(json!({ "ns": namespace, "workflow": workflow, "runId": run_id })),
        )
        .await;
    }
    let mut client = connect_gateway(cli).await?;
    let response = client
        .cancel_workflow_run(CancelWorkflowRunRequest {
            ns: namespace.to_string(),
            workflow: workflow.to_string(),
            run_id: run_id.to_string(),
        })
        .await
        .context("Failed to cancel workflow run")?
        .into_inner();
    let run = response.run.context("Workflow run missing from response")?;
    Ok(workflow_run_json(&run, &response.steps))
}

pub(super) async fn workflow_run_events(
    cli: &Cli,
    namespace: &str,
    workflow: &str,
    run_id: &str,
) -> Result<()> {
    if cli.rest {
        rest_stream_workflow_events(cli, namespace, workflow, run_id).await?;
        return Ok(());
    }
    let mut client = connect_gateway(cli).await?;
    let mut stream = client
        .stream_workflow_events(StreamWorkflowEventsRequest {
            ns: namespace.to_string(),
            workflow: workflow.to_string(),
            run_id: run_id.to_string(),
        })
        .await
        .context("Failed to stream workflow events")?
        .into_inner();
    while let Some(event) = stream
        .message()
        .await
        .context("Failed to read workflow event")?
    {
        println!("{}", serde_json::to_string(&workflow_event_json(&event))?);
    }
    Ok(())
}

async fn rest_stream_workflow_events(
    cli: &Cli,
    namespace: &str,
    workflow: &str,
    run_id: &str,
) -> Result<()> {
    let client = rest_client(cli)?;
    let path = format!(
        "/v1/ns/{}/workflows/{}/runs/{}/stream",
        urlencoding::encode(namespace),
        urlencoding::encode(workflow),
        urlencoding::encode(run_id)
    );
    let url = format!("{}{}", cli.gateway.trim_end_matches('/'), path);
    let response = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("Failed to call REST endpoint {}", url))?;
    let status = response.status();
    let headers = response.headers().clone();
    if !status.is_success() {
        let text = response
            .text()
            .await
            .with_context(|| format!("Failed to read REST response body from {}", url))?;
        anyhow::bail!(
            "REST {} {} failed: status={} body={}{}",
            path,
            url,
            status,
            text.trim(),
            rest_grpc_error_details(&headers)
        );
    }

    let mut stream = response.bytes_stream();
    let mut buffer = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.with_context(|| format!("Failed to read REST stream from {}", url))?;
        buffer.extend_from_slice(&chunk);
        ensure_rest_stream_buffer_within_limit(buffer.len())?;
        let mut last_index = 0;
        while let Some(newline) = buffer[last_index..].iter().position(|byte| *byte == b'\n') {
            let absolute_newline = last_index + newline;
            let line = String::from_utf8_lossy(&buffer[last_index..absolute_newline]);
            print_stream_event_line(line.trim_end_matches('\r'))?;
            last_index = absolute_newline + 1;
        }
        if last_index > 0 {
            buffer.drain(..last_index);
        }
    }
    if !buffer.is_empty() {
        let line = String::from_utf8_lossy(&buffer);
        print_stream_event_line(line.trim_end_matches('\r'))?;
    }
    Ok(())
}

fn ensure_rest_stream_buffer_within_limit(buffer_len: usize) -> Result<()> {
    if buffer_len > MAX_REST_STREAM_BUFFER_BYTES {
        anyhow::bail!(
            "REST stream exceeded maximum buffer limit of {} bytes without a newline",
            MAX_REST_STREAM_BUFFER_BYTES
        );
    }
    Ok(())
}

fn print_stream_event_line(line: &str) -> Result<()> {
    let line = line.trim();
    if line.is_empty() || line.starts_with(':') || line.starts_with("event:") {
        return Ok(());
    }
    let payload = line.strip_prefix("data:").map(str::trim).unwrap_or(line);
    if payload.is_empty() || payload == "[DONE]" {
        return Ok(());
    }
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(payload) {
        println!("{}", serde_json::to_string(&json)?);
    } else {
        println!("{}", payload);
    }
    Ok(())
}
