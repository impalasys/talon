// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::{Cli, RunOutcome};
use crate::cli::{auth_interceptor, rest_request_json};
use crate::control::resource_model;
use crate::gateway::rpc::manifests::{Knowledge, KnowledgeSpec, ObjectMeta};
use crate::gateway::rpc::proto::gateway_service_client::GatewayServiceClient;
use crate::gateway::rpc::proto::{
    CreateResourceRequest, DeleteResourceRequest, GetResourceRequest, ListResourcesRequest,
};
use crate::gateway::rpc::resources_proto;

#[derive(Args)]
pub(crate) struct KnowledgeCommand {
    #[command(subcommand)]
    command: KnowledgeCommands,
}

#[derive(Subcommand)]
enum KnowledgeCommands {
    /// Read a knowledge artifact by path.
    Get {
        #[arg(short, long)]
        namespace: String,
        #[arg(long)]
        path: String,
    },
    /// Write a knowledge artifact from inline content or a file.
    Set {
        #[arg(short, long)]
        namespace: String,
        #[arg(long)]
        path: String,
        #[arg(long, conflicts_with = "content")]
        file: Option<String>,
        #[arg(long, conflicts_with = "file")]
        content: Option<String>,
    },
    /// Delete a knowledge artifact by path.
    Delete {
        #[arg(short, long)]
        namespace: String,
        #[arg(long)]
        path: String,
    },
    /// Sync all markdown files in a directory into namespace knowledge.
    Sync {
        #[arg(short, long)]
        namespace: String,
        #[arg(long)]
        dir: String,
    },
}

pub(super) async fn run(cli: &Cli, command: &KnowledgeCommand) -> Result<RunOutcome> {
    match &command.command {
        KnowledgeCommands::Get { namespace, path } => {
            let knowledge = knowledge_get(cli, namespace, path).await?;
            let Some(knowledge) = knowledge else {
                eprintln!("Knowledge '{}/{}' not found.", namespace, path);
                return Ok(RunOutcome { exit_code: Some(1) });
            };
            let content = knowledge
                .spec
                .as_ref()
                .map(|spec| spec.content.clone())
                .unwrap_or_default();
            print!("{}", content);
            if !content.ends_with('\n') {
                println!();
            }
        }
        KnowledgeCommands::Set {
            namespace,
            path,
            file,
            content,
        } => {
            let value = read_knowledge_content(file, content)?;
            knowledge_set(cli, namespace, path, value).await?;
            println!("✓ Knowledge '{}/{}' written successfully.", namespace, path);
        }
        KnowledgeCommands::Delete { namespace, path } => {
            knowledge_delete(cli, namespace, path).await?;
            println!("✓ Knowledge '{}/{}' deleted successfully.", namespace, path);
        }
        KnowledgeCommands::Sync { namespace, dir } => {
            let root = Path::new(dir);
            let (synced_count, unsynced_existing) = sync_knowledge_dir(cli, namespace, dir).await?;
            println!(
                "✓ Synced {} knowledge artifact(s) into '{}'.",
                synced_count, namespace
            );
            if !unsynced_existing.is_empty() {
                eprintln!(
                    "Note: {} existing knowledge artifact(s) in '{}' were left untouched because they are not present in '{}'.",
                    unsynced_existing.len(),
                    namespace,
                    root.display()
                );
            }
        }
    }
    Ok(RunOutcome { exit_code: None })
}

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

fn read_knowledge_content(file: &Option<String>, content: &Option<String>) -> Result<String> {
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

async fn knowledge_get(cli: &Cli, namespace: &str, path: &str) -> Result<Option<Knowledge>> {
    let name = knowledge_resource_name(path);
    if cli.rest {
        let resp = rest_request_json(
            cli,
            reqwest::Method::GET,
            &format!(
                "/v1/ns/{}/resources/Knowledge/{}",
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

async fn knowledge_set(cli: &Cli, namespace: &str, path: &str, content: String) -> Result<()> {
    let knowledge = build_knowledge(namespace, path, content);
    if cli.rest {
        rest_request_json(
            cli,
            reqwest::Method::POST,
            &format!("/v1/ns/{}/resources", urlencoding::encode(namespace)),
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

async fn knowledge_delete(cli: &Cli, namespace: &str, path: &str) -> Result<()> {
    let name = knowledge_resource_name(path);
    if cli.rest {
        rest_request_json(
            cli,
            reqwest::Method::DELETE,
            &format!(
                "/v1/ns/{}/resources/Knowledge/{}",
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
                "/v1/ns/{}/resources?kind=Knowledge",
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

async fn sync_knowledge_dir(
    cli: &Cli,
    namespace: &str,
    dir: &str,
) -> Result<(usize, Vec<String>)> {
    let root = Path::new(dir);
    let files = collect_markdown_files(root)?;
    let existing: Vec<Knowledge> = knowledge_list(cli, namespace).await?;
    let existing_paths = existing
        .into_iter()
        .filter_map(|knowledge| knowledge.spec.map(|spec| spec.path))
        .collect::<std::collections::HashSet<_>>();
    let mut synced_paths = Vec::new();

    for file in files {
        let knowledge_path = relative_knowledge_path(root, &file)?;
        let content = fs::read_to_string(&file)
            .with_context(|| format!("Failed to read knowledge file '{}'", file.display()))?;
        knowledge_set(cli, namespace, &knowledge_path, content).await?;
        synced_paths.push(knowledge_path);
    }

    let unsynced_existing = existing_paths
        .into_iter()
        .filter(|path| !synced_paths.iter().any(|synced| synced == path))
        .collect::<Vec<_>>();

    Ok((synced_paths.len(), unsynced_existing))
}
