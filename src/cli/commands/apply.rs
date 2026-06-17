// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{Context, Result};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::control::ns;
use crate::control::resource_model::TypedResource;
use crate::gateway::rpc::proto::gateway_service_client::GatewayServiceClient;
use crate::gateway::rpc::proto::{CreateNamespaceRequest, CreateResourceRequest};
use crate::gateway::rpc::resources_proto;

use super::Cli;
use crate::cli::{auth_interceptor, parse_raw_manifest, render_manifest_file, rest_request_json};

#[derive(clap::Args)]
pub(crate) struct ApplyCommand {
    /// Manifest file or directory. Repeat to apply multiple paths.
    #[arg(short, long, required = true)]
    pub(crate) file: Vec<String>,
    /// Template variables in KEY=VALUE form.
    #[arg(long = "var", value_name = "KEY=VALUE")]
    pub(crate) vars: Vec<String>,
}

pub(super) async fn run(cli: &Cli, command: &ApplyCommand) -> Result<()> {
    let files = collect_apply_files(&command.file)?;
    for file in files {
        let file = file.to_string_lossy().into_owned();
        let content = render_manifest_file(&file, &command.vars)?;

        if cli.rest {
            rest_ensure_manifest_namespace(cli, &content).await?;
            println!("{}", rest_apply_manifest(cli, &content).await?);
        } else {
            println!("{}", grpc_apply_manifest(cli, &content).await?);
        }
    }
    Ok(())
}

fn collect_apply_files(inputs: &[String]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for input in inputs {
        collect_apply_path(Path::new(input), &mut files)?;
    }
    files.sort();
    Ok(files)
}

fn collect_apply_path(path: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    if path.is_file() {
        files.push(path.to_path_buf());
        return Ok(());
    }
    if path.is_dir() {
        for entry in fs::read_dir(path)
            .with_context(|| format!("Failed to read directory '{}'", path.display()))?
        {
            let entry = entry?;
            let child = entry.path();
            if child.is_dir() {
                collect_apply_path(&child, files)?;
            } else if is_yaml_file(&child) {
                files.push(child);
            }
        }
        return Ok(());
    }
    anyhow::bail!("Apply path '{}' does not exist", path.display())
}

fn is_yaml_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            extension.eq_ignore_ascii_case("yaml") || extension.eq_ignore_ascii_case("yml")
        })
        .unwrap_or(false)
}

fn namespace_to_ensure(content: &str) -> Result<Option<String>> {
    let raw = parse_raw_manifest(content)?;
    if matches!(raw.kind.as_str(), "Namespace" | "MCPServer" | "McpServer") {
        return Ok(None);
    }
    Ok(raw
        .metadata
        .get("namespace")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|namespace| !namespace.is_empty())
        .map(str::to_string))
}

async fn rest_ensure_manifest_namespace(cli: &Cli, content: &str) -> Result<()> {
    let Some(namespace) = namespace_to_ensure(content)? else {
        return Ok(());
    };
    rest_request_json(
        cli,
        reqwest::Method::POST,
        &format!("/v1/namespaces/{}", urlencoding::encode(&namespace)),
        Some(json!({
            "name": namespace.clone(),
            "recursive": true,
            "labels": {},
        })),
    )
    .await
    .with_context(|| format!("Gateway rejected implicit Namespace '{}'", namespace))?;
    Ok(())
}

fn is_generic_resource_kind(kind: &str) -> bool {
    matches!(
        kind,
        "Agent"
            | "McpServer"
            | "MCPServer"
            | "McpServerBinding"
            | "Knowledge"
            | "Channel"
            | "ChannelSubscription"
            | "Schedule"
            | "Workflow"
            | "Template"
            | "Deployment"
            | "DeploymentReplica"
            | "SandboxClass"
            | "SandboxPolicy"
            | "Sandbox"
    )
}

fn resource_manifest_from_manifest(
    content: &str,
) -> Result<(String, String, String, resources_proto::ResourceManifest)> {
    use resources_proto::resource_spec::Kind as SpecKind;

    let raw = parse_raw_manifest(content)?;
    reject_status_field(content)?;
    let mut manifest = match raw.kind.as_str() {
        "MCPServer" | "McpServer" => {
            let mut server = crate::control::manifest::parse_mcp_server(content)?;
            let meta = server
                .metadata
                .as_mut()
                .context("MCPServer missing metadata")?;
            if meta.namespace.is_empty() {
                meta.namespace = ns::TALON_SYSTEM.to_string();
            }
            resources_proto::ResourceManifest {
                api_version: "talon.impalasys.com/v1".to_string(),
                kind: "McpServer".to_string(),
                metadata: server.metadata.clone(),
                spec: Some(resources_proto::ResourceSpec {
                    kind: Some(SpecKind::McpServer(
                        server.spec.clone().context("MCPServer missing spec")?,
                    )),
                }),
            }
        }
        "Agent" => {
            let agent = crate::control::manifest::parse_agent(content)?;
            resources_proto::ResourceManifest {
                api_version: "talon.impalasys.com/v1".to_string(),
                kind: "Agent".to_string(),
                metadata: agent.metadata.clone(),
                spec: Some(resources_proto::ResourceSpec {
                    kind: Some(SpecKind::Agent(
                        agent.spec.clone().context("Agent missing spec")?,
                    )),
                }),
            }
        }
        "McpServerBinding" => {
            let binding = crate::control::manifest::parse_mcp_server_binding(content)?;
            resources_proto::ResourceManifest {
                api_version: "talon.impalasys.com/v1".to_string(),
                kind: "McpServerBinding".to_string(),
                metadata: binding.metadata.clone(),
                spec: Some(resources_proto::ResourceSpec {
                    kind: Some(SpecKind::McpServerBinding(
                        binding
                            .spec
                            .clone()
                            .context("McpServerBinding missing spec")?,
                    )),
                }),
            }
        }
        "Knowledge" => {
            let knowledge = crate::control::manifest::parse_knowledge(content)?;
            resources_proto::ResourceManifest {
                api_version: "talon.impalasys.com/v1".to_string(),
                kind: "Knowledge".to_string(),
                metadata: knowledge.metadata.clone(),
                spec: Some(resources_proto::ResourceSpec {
                    kind: Some(SpecKind::Knowledge(
                        knowledge.spec.clone().context("Knowledge missing spec")?,
                    )),
                }),
            }
        }
        "Channel" => {
            let channel = crate::control::manifest::parse_channel(content)?;
            resources_proto::ResourceManifest {
                api_version: "talon.impalasys.com/v1".to_string(),
                kind: "Channel".to_string(),
                metadata: channel.metadata.clone(),
                spec: Some(resources_proto::ResourceSpec {
                    kind: Some(SpecKind::Channel(
                        channel.spec.clone().context("Channel missing spec")?,
                    )),
                }),
            }
        }
        "ChannelSubscription" => {
            let subscription = crate::control::manifest::parse_channel_subscription(content)?;
            resources_proto::ResourceManifest {
                api_version: "talon.impalasys.com/v1".to_string(),
                kind: "ChannelSubscription".to_string(),
                metadata: subscription.metadata.clone(),
                spec: Some(resources_proto::ResourceSpec {
                    kind: Some(SpecKind::ChannelSubscription(
                        subscription
                            .spec
                            .clone()
                            .context("ChannelSubscription missing spec")?,
                    )),
                }),
            }
        }
        "Workflow" => {
            let workflow = crate::control::manifest::parse_workflow(content)?;
            resources_proto::ResourceManifest {
                api_version: "talon.impalasys.com/v1".to_string(),
                kind: "Workflow".to_string(),
                metadata: workflow.metadata.clone(),
                spec: Some(resources_proto::ResourceSpec {
                    kind: Some(SpecKind::Workflow(
                        workflow.spec.clone().context("Workflow missing spec")?,
                    )),
                }),
            }
        }
        kind if is_generic_resource_kind(kind) => {
            crate::control::manifest::parse_resource_manifest(content)?
        }
        other => anyhow::bail!("Unsupported manifest kind '{}'", other),
    };
    let meta = manifest
        .metadata
        .as_mut()
        .context("resource missing metadata")?;
    if meta.namespace.is_empty() {
        meta.namespace = ns::TALON_SYSTEM.to_string();
    }
    Ok((
        meta.namespace.clone(),
        manifest.kind.clone(),
        meta.name.clone(),
        manifest,
    ))
}

fn reject_status_field(content: &str) -> Result<()> {
    let value: serde_yaml::Value =
        serde_yaml::from_str(content).context("Failed to parse resource manifest YAML")?;
    let has_status = value
        .as_mapping()
        .map(|mapping| mapping.contains_key(serde_yaml::Value::String("status".to_string())))
        .unwrap_or(false);
    if has_status {
        anyhow::bail!("Resource manifests cannot set status; status is controller-owned");
    }
    Ok(())
}

fn resource_manifest_proto_json(resource: &resources_proto::ResourceManifest) -> serde_json::Value {
    json!({
        "apiVersion": resource.api_version,
        "kind": resource.kind,
        "metadata": resource.metadata.as_ref().map(resource_meta_proto_json),
        "spec": resource.spec.as_ref().map(resource_spec_proto_json),
    })
}

fn resource_meta_proto_json(meta: &resources_proto::ResourceMeta) -> serde_json::Value {
    json!({
        "name": meta.name,
        "namespace": meta.namespace,
        "labels": meta.labels,
        "annotations": meta.annotations,
        "ownerReferences": meta.owner_references.iter().map(owner_reference_proto_json).collect::<Vec<_>>(),
        "finalizers": meta.finalizers,
        "generation": meta.generation,
        "resourceVersion": meta.resource_version,
        "uid": meta.uid,
        "deletionTimestamp": meta.deletion_timestamp,
    })
}

fn owner_reference_proto_json(reference: &resources_proto::OwnerReference) -> serde_json::Value {
    json!({
        "apiVersion": reference.api_version,
        "kind": reference.kind,
        "namespace": reference.namespace,
        "name": reference.name,
        "uid": reference.uid,
        "controller": reference.controller,
        "blockOwnerDeletion": reference.block_owner_deletion,
    })
}

fn resource_ref_proto_json(reference: &resources_proto::ResourceRef) -> serde_json::Value {
    json!({
        "namespace": reference.namespace,
        "name": reference.name,
    })
}

fn resource_spec_proto_json(spec: &resources_proto::ResourceSpec) -> serde_json::Value {
    use resources_proto::resource_spec::Kind;

    match spec.kind.as_ref() {
        Some(Kind::Template(spec)) => json!({
            "template": {
                "kind": spec.kind,
                "metadata": spec.metadata.as_ref().map(resource_meta_proto_json),
                "specJson": spec.spec_json,
            }
        }),
        Some(Kind::Deployment(spec)) => json!({
            "deployment": {
                "placement": spec.placement.as_ref().map(|placement| json!({
                    "namespaceSelector": placement.namespace_selector.as_ref().map(|selector| json!({
                        "parent": selector.parent,
                        "matchLabels": selector.match_labels,
                    })),
                })),
                "templates": spec.templates,
            }
        }),
        Some(Kind::DeploymentReplica(spec)) => json!({
            "deploymentReplica": {
                "deploymentRef": spec.deployment_ref.as_ref().map(resource_ref_proto_json),
                "targetNamespace": spec.target_namespace,
            }
        }),
        Some(Kind::SandboxClass(spec)) => json!({
            "sandboxClass": {
                "provider": spec.provider,
                "providerConfigJson": spec.provider_config_json,
                "credentialsJson": spec.credentials_json,
            }
        }),
        Some(Kind::SandboxPolicy(spec)) => json!({
            "sandboxPolicy": {
                "classRef": spec.class_ref.as_ref().map(resource_ref_proto_json),
                "template": spec.template.as_ref().map(sandbox_runtime_template_proto_json),
                "maxConcurrent": spec.max_concurrent,
            }
        }),
        Some(Kind::Sandbox(spec)) => json!({
            "sandbox": {
                "policyRef": spec.policy_ref,
                "classRef": spec.class_ref.as_ref().map(resource_ref_proto_json),
                "runtimeTemplate": spec.runtime_template.as_ref().map(sandbox_runtime_template_proto_json),
            }
        }),
        Some(Kind::PermissionRequest(spec)) => json!({
            "permissionRequest": {
                "agent": spec.agent,
                "sessionId": spec.session_id,
                "action": spec.action,
                "prompt": spec.prompt,
                "payloadJson": spec.payload_json,
            }
        }),
        Some(Kind::Raw(spec)) => json!({ "raw": { "json": spec.json } }),
        Some(Kind::Agent(_))
        | Some(Kind::Workflow(_))
        | Some(Kind::Schedule(_))
        | Some(Kind::Channel(_))
        | Some(Kind::ChannelSubscription(_))
        | Some(Kind::McpServer(_))
        | Some(Kind::McpServerBinding(_))
        | Some(Kind::Knowledge(_))
        | Some(Kind::Namespace(_))
        | Some(Kind::Session(_))
        | None => json!({}),
    }
}

fn sandbox_runtime_template_proto_json(
    spec: &resources_proto::SandboxRuntimeTemplateSpec,
) -> serde_json::Value {
    json!({
        "image": spec.image,
        "workspace": spec.workspace.as_ref().map(|workspace| json!({
            "mode": workspace.mode,
            "mountPath": workspace.mount_path,
        })),
        "setup": spec.setup.as_ref().map(|setup| json!({
            "packages": setup.packages,
            "commands": setup.commands,
        })),
        "network": spec.network.as_ref().map(|network| json!({
            "mode": network.mode,
        })),
        "filesystem": spec.filesystem.as_ref().map(|filesystem| json!({
            "writable": filesystem.writable,
            "readonly": filesystem.readonly,
        })),
        "leasePolicy": spec.lease_policy.as_ref().map(|lease_policy| json!({
            "mode": lease_policy.mode,
        })),
    })
}

pub(super) async fn rest_apply_manifest(cli: &Cli, content: &str) -> Result<String> {
    let plan = build_rest_apply_plan(content)?;
    rest_request_json(cli, plan.method, &plan.path, Some(plan.payload))
        .await
        .with_context(|| format!("Gateway rejected {}", plan.success_label))?;
    Ok(format!("✓ {} applied successfully.", plan.success_label))
}

#[derive(Debug)]
struct RestApplyPlan {
    method: reqwest::Method,
    path: String,
    payload: serde_json::Value,
    success_label: String,
}

#[derive(Debug)]
enum GrpcApplyPlan {
    Namespace {
        name: String,
        labels: HashMap<String, String>,
    },
    Resource {
        ns: String,
        kind: String,
        name: String,
        manifest: resources_proto::ResourceManifest,
    },
}

fn build_rest_apply_plan(content: &str) -> Result<RestApplyPlan> {
    let raw = parse_raw_manifest(content)?;
    if raw.kind == "Namespace" {
        let namespace = crate::control::manifest::parse_namespace(content)?;
        let name = namespace.name();
        return Ok(RestApplyPlan {
            method: reqwest::Method::POST,
            path: format!("/v1/namespaces/{}", urlencoding::encode(&name)),
            payload: json!({
                "name": name,
                "recursive": true,
                "labels": namespace.labels(),
            }),
            success_label: format!("Namespace '{}'", name),
        });
    }
    let (ns, kind, name, manifest) = resource_manifest_from_manifest(content)?;
    Ok(RestApplyPlan {
        method: reqwest::Method::POST,
        path: format!("/v2/ns/{}/resources", urlencoding::encode(&ns)),
        payload: json!({
            "ns": ns,
            "manifest": resource_manifest_proto_json(&manifest),
        }),
        success_label: format!("{} '{}/{}'", kind, ns, name),
    })
}

fn build_grpc_apply_plan(content: &str) -> Result<GrpcApplyPlan> {
    match parse_raw_manifest(content)?.kind.as_str() {
        "Namespace" => {
            let namespace = crate::control::manifest::parse_namespace(content)?;
            Ok(GrpcApplyPlan::Namespace {
                name: namespace.name().to_string(),
                labels: namespace.labels().clone(),
            })
        }
        _ => {
            let (ns, kind, name, manifest) = resource_manifest_from_manifest(content)?;
            Ok(GrpcApplyPlan::Resource {
                ns,
                kind,
                name,
                manifest,
            })
        }
    }
}

fn grpc_plan_namespace_to_ensure(plan: &GrpcApplyPlan) -> Option<&str> {
    match plan {
        GrpcApplyPlan::Resource { ns, .. } => Some(ns.as_str())
            .filter(|ns| !ns.is_empty())
            .filter(|ns| *ns != crate::control::ns::TALON_SYSTEM),
        GrpcApplyPlan::Namespace { .. } => None,
    }
}

pub(super) async fn grpc_apply_manifest(cli: &Cli, content: &str) -> Result<String> {
    let plan = build_grpc_apply_plan(content)?;
    let channel = tonic::transport::Channel::from_shared(cli.gateway.clone())
        .with_context(|| format!("Invalid gateway URL {}", cli.gateway))?
        .connect()
        .await
        .with_context(|| format!("Could not connect to gateway at {}", cli.gateway))?;
    let mut client = GatewayServiceClient::with_interceptor(channel, auth_interceptor(cli)?);
    if let Some(namespace) = grpc_plan_namespace_to_ensure(&plan) {
        client
            .create_namespace(CreateNamespaceRequest {
                name: namespace.to_string(),
                recursive: true,
                labels: HashMap::new(),
            })
            .await
            .with_context(|| format!("Gateway rejected implicit Namespace '{}'", namespace))?;
    }

    match plan {
        GrpcApplyPlan::Namespace { name, labels } => {
            client
                .create_namespace(CreateNamespaceRequest {
                    name: name.clone(),
                    recursive: true,
                    labels,
                })
                .await
                .with_context(|| format!("Gateway rejected Namespace '{}'", name))?;
            Ok(format!("✓ Namespace '{}' applied successfully.", name))
        }
        GrpcApplyPlan::Resource {
            ns,
            kind,
            name,
            manifest,
        } => {
            client
                .create_resource(CreateResourceRequest {
                    ns: ns.clone(),
                    manifest: Some(manifest),
                })
                .await
                .with_context(|| format!("Gateway rejected {} '{}/{}'", kind, ns, name))?;
            Ok(format!(
                "✓ {} '{}/{}' applied successfully.",
                kind, ns, name
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    fn write_file(path: &Path, content: &str) {
        let mut file = fs::File::create(path).expect("create test file");
        file.write_all(content.as_bytes()).expect("write test file");
    }

    #[test]
    fn collect_apply_files_accepts_multiple_files_and_directories() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        let nested = root.join("nested");
        fs::create_dir(&nested).expect("create nested dir");

        let b = root.join("b.yaml");
        let a = nested.join("a.yml");
        let ignored = root.join("notes.txt");
        let explicit = root.join("explicit.txt");
        write_file(&b, "kind: Namespace\nmetadata:\n  name: b\n");
        write_file(&a, "kind: Namespace\nmetadata:\n  name: a\n");
        write_file(&ignored, "ignored");
        write_file(&explicit, "kind: Namespace\nmetadata:\n  name: explicit\n");

        let files = collect_apply_files(&[
            root.to_string_lossy().into_owned(),
            explicit.to_string_lossy().into_owned(),
        ])
        .expect("collect files");

        assert_eq!(files, vec![b, explicit, a]);
    }

    #[test]
    fn collect_apply_files_rejects_missing_paths() {
        let err = collect_apply_files(&["/definitely/not/a/talon/manifest.yaml".to_string()])
            .expect_err("missing path should fail");

        assert!(err.to_string().contains("does not exist"));
    }
}
