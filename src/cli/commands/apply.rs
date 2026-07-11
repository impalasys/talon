// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::control::ns;
use crate::control::resource_model::TypedResource;
use crate::gateway::rpc::resources_proto;
use talon_client::v1::{CreateNamespaceRequest, CreateResourceRequest};

use super::Cli;
use crate::cli::{connect_gateway, render_manifest_file, to_sdk_resource_manifest};

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
        if content.trim().is_empty() {
            continue;
        }

        apply_manifest(cli, &content).await?;
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

fn is_generic_resource_kind(kind: &str) -> bool {
    matches!(
        kind,
        "Agent"
            | "McpServer"
            | "MCPServer"
            | "Knowledge"
            | "File"
            | "Task"
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
            | "UsagePolicy"
            | "ConnectorClass"
            | "Connector"
    )
}

fn resource_manifest_from_manifest(
    raw: &crate::control::manifest::RawManifest,
    content: &str,
) -> Result<(String, String, String, resources_proto::ResourceManifest)> {
    use resources_proto::resource_spec::Kind as SpecKind;

    let mut manifest = match raw.kind.as_str() {
        "MCPServer" | "McpServer" => {
            let server = crate::control::manifest::parse_mcp_server(content)?;
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
        "File" => {
            let file: resources_proto::File =
                serde_yaml::from_str(content).context("Failed to parse File manifest")?;
            resources_proto::ResourceManifest {
                api_version: "talon.impalasys.com/v1".to_string(),
                kind: "File".to_string(),
                metadata: file.metadata.clone(),
                spec: Some(resources_proto::ResourceSpec {
                    kind: Some(SpecKind::File(
                        file.spec.clone().context("File missing spec")?,
                    )),
                }),
            }
        }
        "Task" => {
            let task: resources_proto::Task =
                serde_yaml::from_str(content).context("Failed to parse Task manifest")?;
            resources_proto::ResourceManifest {
                api_version: "talon.impalasys.com/v1".to_string(),
                kind: "Task".to_string(),
                metadata: task.metadata.clone(),
                spec: Some(resources_proto::ResourceSpec {
                    kind: Some(SpecKind::Task(
                        task.spec.clone().context("Task missing spec")?,
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

fn reject_status_field_in_value(value: &serde_yaml::Value) -> Result<()> {
    let has_status = value
        .as_mapping()
        .map(|mapping| mapping.contains_key(serde_yaml::Value::String("status".to_string())))
        .unwrap_or(false);
    if has_status {
        anyhow::bail!("Resource manifests cannot set status; status is controller-owned");
    }
    Ok(())
}

#[derive(Debug)]
enum ApplyPlan {
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

fn build_apply_plan_from_value(value: serde_yaml::Value) -> Result<ApplyPlan> {
    reject_status_field_in_value(&value)?;
    let raw: crate::control::manifest::RawManifest =
        serde_yaml::from_value(value.clone()).context("Failed to parse manifest YAML")?;
    let document = serde_yaml::to_string(&value).context("Failed to serialize YAML document")?;
    match raw.kind.as_str() {
        "Namespace" => {
            let namespace = crate::control::manifest::parse_namespace(&document)?;
            Ok(ApplyPlan::Namespace {
                name: namespace.name().to_string(),
                labels: namespace.labels().clone(),
            })
        }
        _ => {
            let (ns, kind, name, manifest) = resource_manifest_from_manifest(&raw, &document)?;
            Ok(ApplyPlan::Resource {
                ns,
                kind,
                name,
                manifest,
            })
        }
    }
}

fn build_apply_plans(content: &str) -> Result<Vec<ApplyPlan>> {
    let mut plans = Vec::new();
    for document in serde_yaml::Deserializer::from_str(content) {
        let value = serde_yaml::Value::deserialize(document)
            .context("Failed to parse resource manifest YAML")?;
        if matches!(value, serde_yaml::Value::Null) {
            continue;
        }
        plans.push(build_apply_plan_from_value(value)?);
    }
    Ok(plans)
}

fn plan_namespace_to_ensure(plan: &ApplyPlan) -> Option<&str> {
    match plan {
        ApplyPlan::Resource { ns, .. } => Some(ns.as_str())
            .filter(|ns| !ns.is_empty())
            .filter(|ns| *ns != crate::control::ns::TALON_SYSTEM),
        ApplyPlan::Namespace { .. } => None,
    }
}

pub(super) async fn apply_manifest(cli: &Cli, content: &str) -> Result<()> {
    let plans = build_apply_plans(content)?;
    let mut client = connect_gateway(cli).await?;
    let mut ensured_namespaces = HashSet::new();
    for plan in plans {
        if let Some(namespace) = plan_namespace_to_ensure(&plan) {
            if ensured_namespaces.insert(namespace.to_string()) {
                client
                    .create_namespace(CreateNamespaceRequest {
                        name: namespace.to_string(),
                        recursive: true,
                        labels: HashMap::new(),
                    })
                    .await
                    .with_context(|| {
                        format!("Gateway rejected implicit Namespace '{}'", namespace)
                    })?;
            }
        }

        match plan {
            ApplyPlan::Namespace { name, labels } => {
                client
                    .create_namespace(CreateNamespaceRequest {
                        name: name.clone(),
                        recursive: true,
                        labels,
                    })
                    .await
                    .with_context(|| format!("Gateway rejected Namespace '{}'", name))?;
                println!("✓ Namespace '{}' applied successfully.", name);
            }
            ApplyPlan::Resource {
                ns,
                kind,
                name,
                manifest,
            } => {
                client
                    .create_resource(CreateResourceRequest {
                        ns: ns.clone(),
                        manifest: Some(to_sdk_resource_manifest(&manifest)?),
                    })
                    .await
                    .with_context(|| format!("Gateway rejected {} '{}/{}'", kind, ns, name))?;
                println!("✓ {} '{}/{}' applied successfully.", kind, ns, name);
            }
        }
    }
    Ok(())
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

    #[test]
    fn worker_manifest_is_not_user_authorable() {
        let err = build_apply_plans(
            "apiVersion: talon.impalasys.com/v1\nkind: Worker\nmetadata:\n  name: worker-a\n",
        )
        .expect_err("Worker manifests should not be accepted by apply");

        assert!(err.to_string().contains("Unsupported manifest kind"));
    }

    #[test]
    fn build_apply_plans_accepts_multi_document_yaml() {
        let plans = build_apply_plans(
            r#"
apiVersion: talon.impalasys.com/v1
kind: Namespace
metadata:
  name: demo
---
apiVersion: talon.impalasys.com/v1
kind: Knowledge
metadata:
  namespace: demo
  name: guide
spec:
  path: guide.md
  content: hello
---
"#,
        )
        .expect("multi-document YAML should plan");

        assert_eq!(plans.len(), 2);
        assert!(matches!(plans[0], ApplyPlan::Namespace { .. }));
        assert!(matches!(plans[1], ApplyPlan::Resource { .. }));
    }

    #[test]
    fn build_apply_plans_rejects_status_in_any_document() {
        let err = build_apply_plans(
            r#"
apiVersion: talon.impalasys.com/v1
kind: Namespace
metadata:
  name: demo
---
apiVersion: talon.impalasys.com/v1
kind: Knowledge
metadata:
  namespace: demo
  name: guide
spec:
  path: guide.md
  content: hello
status:
  phase: ready
"#,
        )
        .expect_err("status should be rejected in a YAML stream");

        assert!(err.to_string().contains("status is controller-owned"));
    }
}
