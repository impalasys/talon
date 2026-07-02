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
