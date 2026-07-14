// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

mod tests {
    use super::*;
    use crate::gateway::rpc::resources_proto::{resource_spec, resource_status};

    #[test]
    fn mcp_server_manifest_requires_namespace_and_parses_policy() {
        let missing_namespace = parse_resource_manifest(
            r#"
apiVersion: talon.impalasys.com/v1
kind: McpServer
metadata:
  name: github
spec:
  transport: http
  target: https://example.com/mcp
  args: []
  headers: {}
  disabled: false
"#,
        )
        .expect_err("McpServer without namespace should fail");
        assert!(missing_namespace
            .to_string()
            .contains("McpServer metadata.namespace is required"));

        let manifest = parse_resource_manifest(
            r#"
apiVersion: talon.impalasys.com/v1
kind: McpServer
metadata:
  name: github
  namespace: conic
spec:
  transport: http
  target: https://example.com/mcp
  args: []
  headers: {}
  disabled: false
  policy:
    tools:
      allowlist:
        - search
"#,
        )
        .expect("namespaced McpServer should parse");

        let Some(resource_spec::Kind::McpServer(spec)) = manifest.spec.and_then(|spec| spec.kind)
        else {
            panic!("expected McpServer spec");
        };
        assert_eq!(spec.policy.unwrap().tools.unwrap().allowlist, vec!["search"]);
    }

    #[test]
    fn mcp_server_binding_manifest_is_unsupported() {
        let error = parse_resource_manifest(
            r#"
apiVersion: talon.impalasys.com/v1
kind: McpServerBinding
metadata:
  name: github
  namespace: conic
spec:
  serverRef: github
"#,
        )
        .expect_err("McpServerBinding should be unsupported");

        assert!(error.to_string().contains("McpServerBinding manifests are unsupported"));
    }

    #[test]
    fn template_manifest_normalizes_nested_spec_to_json() {
        let manifest = parse_resource_manifest(
            r#"
apiVersion: talon.impalasys.com/v1
kind: Template
metadata:
  name: coding-agent
  namespace: customers
spec:
  kind: Agent
  metadata:
    name: coding
  spec:
    systemPrompt: hello
"#,
        )
        .expect("template manifest parses");

        let Some(resource_spec::Kind::Template(spec)) = manifest.spec.and_then(|spec| spec.kind)
        else {
            panic!("expected Template spec");
        };
        let rendered_spec: serde_json::Value =
            serde_json::from_str(&spec.spec_json).expect("template spec JSON");
        assert_eq!(rendered_spec["systemPrompt"], "hello");
    }

    #[test]
    fn sandbox_class_manifest_normalizes_config_maps_to_json() {
        let manifest = parse_resource_manifest(
            r#"
apiVersion: talon.impalasys.com/v1
kind: SandboxClass
metadata:
  name: docker-code
  namespace: system
spec:
  provider: docker
  providerConfig:
    image: talon-zed-codex-acp:local
  credentials:
    apiKey:
      source: env
      key: E2B_API_KEY
"#,
        )
        .expect("sandbox class manifest parses");

        let Some(resource_spec::Kind::SandboxClass(spec)) =
            manifest.spec.and_then(|spec| spec.kind)
        else {
            panic!("expected SandboxClass spec");
        };
        let provider_config: serde_json::Value =
            serde_json::from_str(&spec.provider_config_json).expect("provider config JSON");
        let credentials: serde_json::Value =
            serde_json::from_str(&spec.credentials_json).expect("credentials JSON");
        assert_eq!(provider_config["image"], "talon-zed-codex-acp:local");
        assert_eq!(credentials["apiKey"]["key"], "E2B_API_KEY");
    }

    #[test]
    fn connector_class_manifest_maps_resource_secret_ref_shape() {
        let manifest = parse_resource_manifest(
            r#"
apiVersion: talon.impalasys.com/v1
kind: ConnectorClass
metadata:
  name: slack
  namespace: customers
spec:
  platform: slack
  runtime:
    kind: externalService
    endpoint: https://slack-connector.example.com
  auth:
    kind: apiKey
    apiKey:
      env: TALON_SLACK_CONNECTOR_API_KEY
  matchIndexes:
    - name: workspace
      fields:
        - teamId
"#,
        )
        .expect("connector class manifest parses");

        let Some(resource_spec::Kind::ConnectorClass(spec)) =
            manifest.spec.clone().and_then(|spec| spec.kind)
        else {
            panic!("expected ConnectorClass spec");
        };
        let api_key = spec.auth.unwrap().api_key.unwrap();
        assert_eq!(api_key.env.as_deref(), Some("TALON_SLACK_CONNECTOR_API_KEY"));
        assert_eq!(api_key.plain, None);

        let rendered = render_resource_yaml(&resources_proto::Resource {
            api_version: manifest.api_version,
            kind: manifest.kind,
            metadata: manifest.metadata,
            spec: manifest.spec,
            status: Some(resources_proto::ResourceStatus {
                kind: Some(resource_status::Kind::ConnectorClass(Default::default())),
            }),
        })
        .expect("render connector class");
        let rendered_yaml: serde_yaml::Value =
            serde_yaml::from_str(&rendered).expect("rendered YAML parses");
        assert_eq!(
            rendered_yaml["spec"]["auth"]["apiKey"]["env"].as_str(),
            Some("TALON_SLACK_CONNECTOR_API_KEY")
        );
        assert!(rendered_yaml["spec"]["auth"]["apiKey"]
            .get("source")
            .is_none());
    }

    #[test]
    fn connector_manifest_maps_message_consumer_payload_shape() {
        let manifest = parse_resource_manifest(
            r#"
apiVersion: talon.impalasys.com/v1
kind: Connector
metadata:
  name: slack-main
  namespace: customers
spec:
  classRef:
    name: slack
  enabled: true
  matchFields:
    teamId: T123
  consumer:
    channel:
      channel:
        name: campaigns
      agent:
        name: marketing-agent
      continuity: reuse
      replyPolicy: thread
"#,
        )
        .expect("connector manifest parses");

        let Some(resource_spec::Kind::Connector(spec)) =
            manifest.spec.clone().and_then(|spec| spec.kind)
        else {
            panic!("expected Connector spec");
        };
        let consumer = spec.consumer.expect("consumer");
        assert!(consumer.session.is_none());
        let channel = consumer.channel.expect("channel consumer");
        assert_eq!(channel.channel.unwrap().name, "campaigns");
        assert_eq!(channel.agent.unwrap().name, "marketing-agent");
        assert_eq!(channel.reply_policy, "thread");

        let rendered = render_resource_yaml(&resources_proto::Resource {
            api_version: manifest.api_version,
            kind: manifest.kind,
            metadata: manifest.metadata,
            spec: manifest.spec,
            status: Some(resources_proto::ResourceStatus {
                kind: Some(resource_status::Kind::Connector(Default::default())),
            }),
        })
        .expect("render connector");
        let rendered_yaml: serde_yaml::Value =
            serde_yaml::from_str(&rendered).expect("rendered YAML parses");
        assert!(rendered_yaml["spec"]["consumer"].get("kind").is_none());
        assert_eq!(
            rendered_yaml["spec"]["consumer"]["channel"]["replyPolicy"].as_str(),
            Some("thread")
        );
    }

    #[test]
    fn connector_manifest_maps_workflow_consumer_payload_shape() {
        let manifest = parse_resource_manifest(
            r#"
apiVersion: talon.impalasys.com/v1
kind: Connector
metadata:
  name: slack-router
  namespace: customers
spec:
  classRef:
    name: slack
  enabled: true
  matchFields:
    teamId: T123
  consumer:
    workflow:
      name: message-router
      replyMode: thread
"#,
        )
        .expect("connector manifest parses");

        let Some(resource_spec::Kind::Connector(spec)) =
            manifest.spec.clone().and_then(|spec| spec.kind)
        else {
            panic!("expected Connector spec");
        };
        let consumer = spec.consumer.expect("consumer");
        assert!(consumer.session.is_none());
        assert!(consumer.channel.is_none());
        let workflow = consumer.workflow.expect("workflow consumer");
        assert_eq!(workflow.name, "message-router");
        assert_eq!(workflow.reply_mode, "thread");
    }

    #[test]
    fn connector_manifest_maps_session_consumer_reply_mode() {
        let manifest = parse_resource_manifest(
            r#"
apiVersion: talon.impalasys.com/v1
kind: Connector
metadata:
  name: nimbus-shukant
  namespace: Tenant:conic:Nimbus
spec:
  classRef:
    name: nimbus-imessage
  enabled: true
  matchFields:
    lineId: shared
    participantId: "+16146863949"
  consumer:
    session:
      agent:
        name: nimbus
      continuity: reuse
      replyMode: hold_for_review
"#,
        )
        .expect("connector manifest parses");

        let Some(resource_spec::Kind::Connector(spec)) =
            manifest.spec.clone().and_then(|spec| spec.kind)
        else {
            panic!("expected Connector spec");
        };
        let consumer = spec.consumer.expect("consumer");
        assert!(consumer.channel.is_none());
        assert!(consumer.workflow.is_none());
        let session = consumer.session.expect("session consumer");
        assert_eq!(session.agent.unwrap().name, "nimbus");
        assert_eq!(session.continuity, "reuse");
        assert_eq!(session.reply_mode, "hold_for_review");
    }

    #[test]
    fn agent_manifest_maps_a2a_target_payload_shape() {
        let manifest = parse_resource_manifest(
            r#"
apiVersion: talon.impalasys.com/v1
kind: Agent
metadata:
  name: planner
  namespace: customers
spec:
  systemPrompt: hello
  a2a:
    connections:
      - name: search
        target:
          external:
            agentCardUrl: https://example.com/.well-known/agent-card.json
"#,
        )
        .expect("agent manifest parses");

        let Some(resource_spec::Kind::Agent(spec)) = manifest.spec.and_then(|spec| spec.kind)
        else {
            panic!("expected Agent spec");
        };
        let connection = &spec.a2a.unwrap().connections[0];
        let target = connection.target.as_ref().expect("target");
        assert!(target.internal.is_none());
        assert_eq!(
            target.external.as_ref().unwrap().agent_card_url,
            "https://example.com/.well-known/agent-card.json"
        );
    }

    #[test]
    fn sandbox_policy_manifest_accepts_and_renders_template_spec_shape() {
        let manifest = parse_resource_manifest(
            r#"
apiVersion: talon.impalasys.com/v1
kind: SandboxPolicy
metadata:
  name: coding
  namespace: customers
spec:
  classRef:
    namespace: system
    name: docker-code
  template:
    spec:
      image: talon-zed-codex-acp:local
      workspace:
        mode: customer-repo
        mountPath: /workspace
      filesystem:
        writable:
          - /workspace
      leasePolicy:
        mode: exclusive
  quota:
    maxConcurrent: 5
"#,
        )
        .expect("sandbox policy manifest parses");

        let Some(resource_spec::Kind::SandboxPolicy(spec)) =
            manifest.spec.clone().and_then(|spec| spec.kind)
        else {
            panic!("expected SandboxPolicy spec");
        };
        assert_eq!(spec.max_concurrent, 5);
        let template = spec.template.expect("runtime template");
        assert_eq!(template.image, "talon-zed-codex-acp:local");
        assert_eq!(
            template.workspace.expect("workspace").mount_path,
            "/workspace"
        );

        let rendered = render_resource_yaml(&resources_proto::Resource {
            api_version: manifest.api_version,
            kind: manifest.kind,
            metadata: manifest.metadata,
            spec: manifest.spec,
            status: Some(resources_proto::ResourceStatus {
                kind: Some(resource_status::Kind::SandboxPolicy(Default::default())),
            }),
        })
        .expect("render sandbox policy");
        let rendered_yaml: serde_yaml::Value =
            serde_yaml::from_str(&rendered).expect("rendered YAML parses");
        assert_eq!(
            rendered_yaml["spec"]["template"]["image"].as_str(),
            Some("talon-zed-codex-acp:local")
        );
        assert_eq!(
            rendered_yaml["spec"]["template"]["workspace"]["mountPath"].as_str(),
            Some("/workspace")
        );
        assert!(rendered_yaml.get("status").is_none());
    }

    #[test]
    fn sandbox_policy_manifest_rejects_max_concurrent_overflow() {
        let error = parse_resource_manifest(
            r#"
apiVersion: talon.impalasys.com/v1
kind: SandboxPolicy
metadata:
  name: coding
  namespace: customers
spec:
  quota:
    maxConcurrent: 4294967296
"#,
        )
        .expect_err("maxConcurrent above u32 range should fail");
        assert!(error.to_string().contains("maxConcurrent"));
    }

    #[test]
    fn schedule_manifest_rejects_interval_seconds_overflow() {
        let error = parse_resource_manifest(
            r#"
apiVersion: talon.impalasys.com/v1
kind: Schedule
metadata:
  name: daily
  namespace: customers
spec:
  kind: every
  intervalSeconds: 4294967296
"#,
        )
        .expect_err("intervalSeconds above u32 range should fail");
        assert!(error.to_string().contains("intervalSeconds"));
    }

    #[test]
    fn resource_yaml_renders_common_status_conditions_when_present() {
        let rendered = render_resource_yaml(&resources_proto::Resource {
            api_version: "talon.impalasys.com/v1".to_string(),
            kind: "Agent".to_string(),
            metadata: Some(resources_proto::ResourceMeta {
                name: "coding".to_string(),
                namespace: "customers:acme".to_string(),
                ..Default::default()
            }),
            spec: Some(resources_proto::ResourceSpec {
                kind: Some(resource_spec::Kind::Agent(Default::default())),
            }),
            status: Some(resources_proto::ResourceStatus {
                kind: Some(resource_status::Kind::Agent(resources_proto::AgentStatus {
                    observed_generation: 3,
                    phase: "Ready".to_string(),
                    conditions: vec![resources_proto::ResourceCondition {
                        r#type: "Available".to_string(),
                        status: "True".to_string(),
                        reason: "RuntimeReady".to_string(),
                        message: "agent runtime is ready".to_string(),
                        last_transition_time: 42,
                        observed_generation: 3,
                    }],
                    last_session_id: None,
                })),
            }),
        })
        .expect("render resource YAML");

        let rendered_yaml: serde_yaml::Value =
            serde_yaml::from_str(&rendered).expect("rendered YAML parses");
        assert_eq!(
            rendered_yaml["status"]["observedGeneration"].as_u64(),
            Some(3)
        );
        assert_eq!(rendered_yaml["status"]["phase"].as_str(), Some("Ready"));
        assert_eq!(
            rendered_yaml["status"]["conditions"][0]["type"].as_str(),
            Some("Available")
        );
        assert_eq!(
            rendered_yaml["status"]["conditions"][0]["observedGeneration"].as_u64(),
            Some(3)
        );
    }

    #[test]
    fn deployment_status_replica_counts_round_trip_yaml() {
        let rendered = render_resource_yaml(&resources_proto::Resource {
            api_version: "talon.impalasys.com/v1".to_string(),
            kind: "Deployment".to_string(),
            metadata: Some(resources_proto::ResourceMeta {
                name: "company-builder".to_string(),
                namespace: "customers".to_string(),
                ..Default::default()
            }),
            spec: Some(resources_proto::ResourceSpec {
                kind: Some(resource_spec::Kind::Deployment(Default::default())),
            }),
            status: Some(resources_proto::ResourceStatus {
                kind: Some(resource_status::Kind::Deployment(
                    resources_proto::DeploymentStatus {
                        observed_generation: 4,
                        phase: "Ready".to_string(),
                        conditions: Vec::new(),
                        replicas: Vec::new(),
                        replica_counts: Some(resources_proto::DeploymentReplicaCounts {
                            desired: 1200,
                            updated: 1200,
                            ready: 1198,
                            pending: 0,
                            degraded: 2,
                        }),
                    },
                )),
            }),
        })
        .expect("render resource YAML");

        let rendered_yaml: serde_yaml::Value =
            serde_yaml::from_str(&rendered).expect("rendered YAML parses");
        assert!(rendered_yaml["status"]["replicas"].is_null());
        assert_eq!(
            rendered_yaml["status"]["replicaCounts"]["desired"].as_u64(),
            Some(1200)
        );
        assert_eq!(
            rendered_yaml["status"]["replicaCounts"]["ready"].as_u64(),
            Some(1198)
        );
        let parsed = parse_resource(&rendered).expect("parse rendered deployment");
        let Some(resource_status::Kind::Deployment(status)) =
            parsed.status.and_then(|status| status.kind)
        else {
            panic!("expected Deployment status");
        };
        let counts = status.replica_counts.expect("replica counts");
        assert_eq!(counts.updated, 1200);
        assert_eq!(counts.degraded, 2);
    }

    #[test]
    fn agent_spec_serde_preserves_capabilities() {
        let spec: resources_proto::AgentSpec = serde_json::from_value(serde_json::json!({
            "capabilities": {
                "schedules": ["inspect", "create"]
            }
        }))
        .expect("deserialize AgentSpec capabilities");
        assert_eq!(spec.capabilities.len(), 1);
        let rendered = serde_json::to_value(&spec).expect("serialize AgentSpec capabilities");
        assert_eq!(
            rendered["capabilities"]["schedules"],
            serde_json::json!(["inspect", "create"])
        );
    }

    #[test]
    fn agent_spec_serde_preserves_post_history_prompt() {
        let spec: resources_proto::AgentSpec = serde_json::from_value(serde_json::json!({
            "systemPrompt": "Be helpful.",
            "postHistoryPrompt": "Current time: {{ talon.now }}"
        }))
        .expect("deserialize AgentSpec postHistoryPrompt");

        assert_eq!(spec.system_prompt, "Be helpful.");
        assert_eq!(spec.post_history_prompt, "Current time: {{ talon.now }}");

        let rendered = serde_json::to_value(&spec).expect("serialize AgentSpec postHistoryPrompt");
        assert_eq!(rendered["postHistoryPrompt"], "Current time: {{ talon.now }}");
    }

    #[test]
    fn agent_spec_serde_defaults_post_history_prompt_to_empty() {
        let spec: resources_proto::AgentSpec = serde_json::from_value(serde_json::json!({}))
            .expect("deserialize empty AgentSpec");

        assert_eq!(spec.post_history_prompt, "");
    }

    #[test]
    fn sandbox_policy_manifest_rejects_system_mount_path() {
        let error = parse_resource_manifest(
            r#"
apiVersion: talon.impalasys.com/v1
kind: SandboxPolicy
metadata:
  name: coding
  namespace: customers
spec:
  classRef:
    namespace: system
    name: docker-code
  template:
    spec:
      workspace:
        mountPath: /etc
"#,
        )
        .expect_err("system mount path should fail");
        assert!(error.to_string().contains("mountPath"));
    }

    #[test]
    fn skill_manifest_uses_typed_spec_and_renders_flat_yaml() {
        let manifest = parse_resource_manifest(
            r#"
apiVersion: talon.impalasys.com/v1
kind: Skill
metadata:
  name: review
  namespace: customers
spec:
  description: Review code
  instructions: Look for regressions.
"#,
        )
        .expect("skill manifest should parse");
        let Some(resource_spec::Kind::Skill(spec)) =
            manifest.spec.clone().and_then(|spec| spec.kind)
        else {
            panic!("expected Skill spec");
        };
        assert_eq!(spec.instructions, "Look for regressions.");

        let rendered = render_resource_yaml(&resources_proto::Resource {
            api_version: manifest.api_version,
            kind: manifest.kind,
            metadata: manifest.metadata,
            spec: manifest.spec,
            status: Some(resources_proto::ResourceStatus {
                kind: Some(resource_status::Kind::Skill(Default::default())),
            }),
        })
        .expect("skill resource should render");

        assert!(rendered.contains("kind: Skill"));
        assert!(rendered.contains("description: Review code"));
        assert!(rendered.contains("instructions: Look for regressions."));
        assert!(!rendered.contains("raw:"));
    }

    #[test]
    fn file_manifest_accepts_and_renders_symbolic_enums() {
        let manifest = parse_resource_manifest(
            r#"
apiVersion: talon.impalasys.com/v1
kind: File
metadata:
  name: brand-guidelines-md-7f3a
  namespace: Tenant:acme:Workspace:main
spec:
  path: /memory/brand-guidelines.md
  mediaType: text/markdown
  purpose: MEMORY
  indexPolicy: RETRIEVAL
  retention: RETAINED
"#,
        )
        .expect("file manifest should parse symbolic enum values");
        let Some(resource_spec::Kind::File(spec)) =
            manifest.spec.clone().and_then(|spec| spec.kind)
        else {
            panic!("expected File spec");
        };
        assert_eq!(spec.purpose, resources_proto::FilePurpose::Memory as i32);
        assert_eq!(
            spec.index_policy,
            resources_proto::FileIndexPolicy::Retrieval as i32
        );
        assert_eq!(
            spec.retention,
            resources_proto::FileRetention::Retained as i32
        );

        let rendered = render_resource_yaml(&resources_proto::Resource {
            api_version: manifest.api_version,
            kind: manifest.kind,
            metadata: manifest.metadata,
            spec: manifest.spec,
            status: Some(resources_proto::ResourceStatus {
                kind: Some(resource_status::Kind::File(Default::default())),
            }),
        })
        .expect("file resource should render");

        assert!(rendered.contains("purpose: MEMORY"));
        assert!(rendered.contains("indexPolicy: RETRIEVAL"));
        assert!(rendered.contains("retention: RETAINED"));
    }

    #[test]
    fn task_manifest_accepts_string_type_and_renders_symbolic_phase() {
        let manifest = parse_resource_manifest(
            r#"
apiVersion: talon.impalasys.com/v1
kind: Task
metadata:
  name: launch-copy
  namespace: Tenant:acme:Workspace:main
spec:
  title: Launch copy
  description: Draft launch copy.
  type: agent_delegation
  owner:
    namespace: Tenant:acme:Workspace:main
    name: cmo
  delegate:
    namespace: Tenant:acme:Workspace:main
    name: writer
"#,
        )
        .expect("task manifest should parse string type");
        let Some(resource_spec::Kind::Task(spec)) =
            manifest.spec.clone().and_then(|spec| spec.kind)
        else {
            panic!("expected Task spec");
        };
        assert_eq!(spec.r#type, "agent_delegation");
        assert_eq!(spec.owner.as_ref().unwrap().name, "cmo");
        assert_eq!(spec.delegate.as_ref().unwrap().name, "writer");

        let rendered = render_resource_yaml(&resources_proto::Resource {
            api_version: manifest.api_version,
            kind: manifest.kind,
            metadata: manifest.metadata,
            spec: manifest.spec,
            status: Some(resources_proto::ResourceStatus {
                kind: Some(resource_status::Kind::Task(
                    resources_proto::TaskStatus {
                        phase: resources_proto::TaskPhase::NeedsReview as i32,
                        ..Default::default()
                    },
                )),
            }),
        })
        .expect("task resource should render");

        assert!(rendered.contains("type: agent_delegation"));
        assert!(rendered.contains("name: cmo"));
        assert!(rendered.contains("name: writer"));
        assert!(rendered.contains("phase: NEEDS_REVIEW"));
    }

    #[test]
    fn agent_manifest_rejects_invalid_acp_permission_policy() {
        let error = parse_resource_manifest(
            r#"
apiVersion: talon.impalasys.com/v1
kind: Agent
metadata:
  name: coding
  namespace: customers
spec:
  runtime:
    kind: acp
    acp:
      command: codex-acp
      sandboxPolicyRef: coding
      permissionPolicy:
        filesystemwrite: alllow
"#,
        )
        .expect_err("invalid permission policy should fail");
        assert!(error.to_string().contains("permissionPolicy"));
    }
}
