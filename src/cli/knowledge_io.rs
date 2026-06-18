fn knowledge_resource_name(path: &str) -> String {
    path.to_string()
}

fn build_knowledge(namespace: &str, path: &str, content: String) -> Knowledge {
    Knowledge {
        metadata: Some(ObjectMeta {
            name: knowledge_resource_name(path),
            namespace: namespace.to_string(),
            labels: HashMap::new(),
            annotations: HashMap::new(),
            owner_references: Vec::new(),
            finalizers: Vec::new(),
            generation: 0,
            resource_version: String::new(),
            uid: String::new(),
            deletion_timestamp: None,
        }),
        spec: Some(KnowledgeSpec {
            path: path.to_string(),
            content,
        }),
        status: Some(resource_model::common_status(String::new())),
    }
}

fn knowledge_resource_manifest_proto(
    knowledge: &Knowledge,
) -> Result<resources_proto::ResourceManifest> {
    Ok(resources_proto::ResourceManifest {
        api_version: "talon.impalasys.com/v1".to_string(),
        kind: "Knowledge".to_string(),
        metadata: knowledge.metadata.clone(),
        spec: Some(resources_proto::ResourceSpec {
            kind: Some(resources_proto::resource_spec::Kind::Knowledge(
                knowledge.spec.clone().context("Knowledge missing spec")?,
            )),
        }),
    })
}

fn knowledge_from_resource_proto(resource: resources_proto::Resource) -> Option<Knowledge> {
    let spec = resource.spec.and_then(|spec| match spec.kind {
        Some(resources_proto::resource_spec::Kind::Knowledge(spec)) => Some(spec),
        _ => None,
    })?;
    let status = resource.status.and_then(|status| match status.kind {
        Some(resources_proto::resource_status::Kind::Knowledge(status)) => Some(status),
        _ => None,
    });
    Some(Knowledge {
        metadata: resource.metadata,
        spec: Some(spec),
        status,
    })
}

fn knowledge_resource_manifest_json(knowledge: &Knowledge) -> serde_json::Value {
    json!({
        "apiVersion": "talon.impalasys.com/v1",
        "kind": "Knowledge",
        "metadata": knowledge.metadata,
        "spec": {
            "knowledge": knowledge.spec,
        },
    })
}

fn knowledge_from_resource_json(resource: serde_json::Value) -> Result<Option<Knowledge>> {
    let metadata = resource.get("metadata").cloned();
    let spec = resource
        .get("spec")
        .and_then(|spec| spec.get("knowledge"))
        .cloned();
    let status = resource
        .get("status")
        .and_then(|status| status.get("knowledge"))
        .cloned();
    let Some(spec) = spec else {
        return Ok(None);
    };
    Ok(Some(Knowledge {
        metadata: metadata
            .map(serde_json::from_value)
            .transpose()
            .context("Failed to decode Knowledge metadata")?,
        spec: Some(serde_json::from_value(spec).context("Failed to decode Knowledge spec")?),
        status: status
            .map(serde_json::from_value)
            .transpose()
            .context("Failed to decode Knowledge status")?,
    }))
}

pub(super) fn read_knowledge_content(
    file: &Option<String>,
    content: &Option<String>,
) -> Result<String> {
    match (file, content) {
        (Some(path), None) => fs::read_to_string(path)
            .with_context(|| format!("Failed to read knowledge content from '{}'", path)),
        (None, Some(value)) => Ok(value.clone()),
        (Some(_), Some(_)) => anyhow::bail!("Specify only one of --file or --content"),
        (None, None) => anyhow::bail!("One of --file or --content is required"),
    }
}

fn relative_knowledge_path(root: &Path, file: &Path) -> Result<String> {
    let relative = file.strip_prefix(root).with_context(|| {
        format!(
            "Knowledge file '{}' is not inside '{}'",
            file.display(),
            root.display()
        )
    })?;
    let path = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/");
    if path.is_empty() {
        anyhow::bail!("Knowledge path cannot be empty for '{}'", file.display());
    }
    Ok(path)
}

fn collect_markdown_files(dir: &Path) -> Result<Vec<PathBuf>> {
    fn walk(current: &Path, acc: &mut Vec<PathBuf>) -> Result<()> {
        for entry in fs::read_dir(current)
            .with_context(|| format!("Failed to read directory '{}'", current.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                walk(&path, acc)?;
            } else if path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("md"))
                .unwrap_or(false)
            {
                acc.push(path);
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    walk(dir, &mut files)?;
    files.sort();
    Ok(files)
}

pub(super) async fn knowledge_get(
    cli: &Cli,
    namespace: &str,
    path: &str,
) -> Result<Option<Knowledge>> {
    let name = knowledge_resource_name(path);
    if cli.rest {
        let resp = rest_request_json(
            cli,
            reqwest::Method::GET,
            &format!(
                "/v2/ns/{}/resources/Knowledge/{}",
                urlencoding::encode(namespace),
                urlencoding::encode(&name)
            ),
            None,
        )
        .await?;
        let Some(resource) = resp.get("resource").cloned() else {
            return Ok(None);
        };
        Ok(knowledge_from_resource_json(resource)?)
    } else {
        let channel = tonic::transport::Channel::from_shared(cli.gateway.clone())
            .with_context(|| format!("Invalid gateway URL {}", cli.gateway))?
            .connect()
            .await
            .with_context(|| format!("Could not connect to gateway at {}", cli.gateway))?;
        let mut client = GatewayServiceClient::with_interceptor(channel, auth_interceptor(cli)?);
        let response = client
            .get_resource(GetResourceRequest {
                ns: namespace.to_string(),
                kind: "Knowledge".to_string(),
                name,
            })
            .await;
        match response {
            Ok(resp) => Ok(resp
                .into_inner()
                .resource
                .and_then(knowledge_from_resource_proto)),
            Err(status) if status.code() == tonic::Code::NotFound => Ok(None),
            Err(status) => Err(status).context(format!(
                "Failed to fetch Knowledge '{}/{}'",
                namespace, path
            )),
        }
    }
}

pub(super) async fn knowledge_set(
    cli: &Cli,
    namespace: &str,
    path: &str,
    content: String,
) -> Result<()> {
    let knowledge = build_knowledge(namespace, path, content);
    if cli.rest {
        rest_request_json(
            cli,
            reqwest::Method::POST,
            &format!("/v2/ns/{}/resources", urlencoding::encode(namespace)),
            Some(json!({
                "ns": namespace,
                "manifest": knowledge_resource_manifest_json(&knowledge),
            })),
        )
        .await
        .with_context(|| format!("Failed to write Knowledge '{}/{}'", namespace, path))?;
    } else {
        let channel = tonic::transport::Channel::from_shared(cli.gateway.clone())
            .with_context(|| format!("Invalid gateway URL {}", cli.gateway))?
            .connect()
            .await
            .with_context(|| format!("Could not connect to gateway at {}", cli.gateway))?;
        let mut client = GatewayServiceClient::with_interceptor(channel, auth_interceptor(cli)?);
        client
            .create_resource(CreateResourceRequest {
                ns: namespace.to_string(),
                manifest: Some(knowledge_resource_manifest_proto(&knowledge)?),
            })
            .await
            .with_context(|| format!("Failed to write Knowledge '{}/{}'", namespace, path))?;
    }
    Ok(())
}

pub(super) async fn knowledge_delete(cli: &Cli, namespace: &str, path: &str) -> Result<()> {
    let name = knowledge_resource_name(path);
    if cli.rest {
        rest_request_json(
            cli,
            reqwest::Method::DELETE,
            &format!(
                "/v2/ns/{}/resources/Knowledge/{}",
                urlencoding::encode(namespace),
                urlencoding::encode(&name)
            ),
            None,
        )
        .await
        .with_context(|| format!("Failed to delete Knowledge '{}/{}'", namespace, path))?;
    } else {
        let channel = tonic::transport::Channel::from_shared(cli.gateway.clone())
            .with_context(|| format!("Invalid gateway URL {}", cli.gateway))?
            .connect()
            .await
            .with_context(|| format!("Could not connect to gateway at {}", cli.gateway))?;
        let mut client = GatewayServiceClient::with_interceptor(channel, auth_interceptor(cli)?);
        client
            .delete_resource(DeleteResourceRequest {
                ns: namespace.to_string(),
                kind: "Knowledge".to_string(),
                name,
            })
            .await
            .with_context(|| format!("Failed to delete Knowledge '{}/{}'", namespace, path))?;
    }
    Ok(())
}

async fn knowledge_list(cli: &Cli, namespace: &str) -> Result<Vec<Knowledge>> {
    if cli.rest {
        let resp = rest_request_json(
            cli,
            reqwest::Method::GET,
            &format!(
                "/v2/ns/{}/resources?kind=Knowledge",
                urlencoding::encode(namespace)
            ),
            None,
        )
        .await?;
        let resources = resp
            .get("resources")
            .cloned()
            .unwrap_or_else(|| serde_json::Value::Array(Vec::new()));
        let resources = resources.as_array().cloned().unwrap_or_default();
        resources
            .into_iter()
            .map(knowledge_from_resource_json)
            .collect::<Result<Vec<_>>>()
            .map(|items| items.into_iter().flatten().collect())
    } else {
        let channel = tonic::transport::Channel::from_shared(cli.gateway.clone())
            .with_context(|| format!("Invalid gateway URL {}", cli.gateway))?
            .connect()
            .await
            .with_context(|| format!("Could not connect to gateway at {}", cli.gateway))?;
        let mut client = GatewayServiceClient::with_interceptor(channel, auth_interceptor(cli)?);
        Ok(client
            .list_resources(ListResourcesRequest {
                ns: namespace.to_string(),
                kind: Some("Knowledge".to_string()),
            })
            .await
            .with_context(|| format!("Failed to list Knowledge for '{}'", namespace))?
            .into_inner()
            .resources
            .into_iter()
            .filter_map(knowledge_from_resource_proto)
            .collect())
    }
}
