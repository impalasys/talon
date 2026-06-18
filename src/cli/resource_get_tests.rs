#[cfg(test)]
mod get_list_tests {
    use super::*;

    #[test]
    fn resource_list_target_supports_kubectl_style_aliases() {
        let namespace = "customers:acme".to_string();
        assert_eq!(
            resource_list_target("agents", Some(&namespace)).unwrap(),
            ResourceListTarget::Resources {
                ns: "customers:acme".to_string(),
                kind: Some("Agent".to_string()),
            }
        );
        assert_eq!(
            resource_list_target("sandbox-policies", Some(&namespace)).unwrap(),
            ResourceListTarget::Resources {
                ns: "customers:acme".to_string(),
                kind: Some("SandboxPolicy".to_string()),
            }
        );
        assert_eq!(
            resource_list_target("sandboxclasses", Some(&namespace)).unwrap(),
            ResourceListTarget::Resources {
                ns: "customers:acme".to_string(),
                kind: Some("SandboxClass".to_string()),
            }
        );
        assert_eq!(
            resource_list_target("resources", Some(&namespace)).unwrap(),
            ResourceListTarget::Resources {
                ns: "customers:acme".to_string(),
                kind: None,
            }
        );
        assert_eq!(
            resource_list_target("namespaces", Some(&namespace)).unwrap(),
            ResourceListTarget::Namespaces {
                parent: Some("customers:acme".to_string()),
            }
        );
    }

    #[test]
    fn single_template_lookup_honors_explicit_namespace() {
        let namespace = "customers:source".to_string();

        assert_eq!(
            resource_lookup_target("template", "coding-sandbox-policy", Some(&namespace)).unwrap(),
            (
                "customers:source".to_string(),
                "Template".to_string(),
                "coding-sandbox-policy".to_string(),
            )
        );
        assert_eq!(
            rest_delete_path("template", "coding-sandbox-policy", Some(&namespace)).unwrap(),
            "/v2/ns/customers%3Asource/resources/Template/coding-sandbox-policy"
        );
    }

    #[test]
    fn single_sandbox_class_lookup_honors_explicit_namespace() {
        let namespace = "Example".to_string();

        assert_eq!(
            resource_lookup_target("sandboxclass", "docker-codex", Some(&namespace)).unwrap(),
            (
                "Example".to_string(),
                "SandboxClass".to_string(),
                "docker-codex".to_string(),
            )
        );
        assert_eq!(
            rest_delete_path("sandboxclass", "docker-codex", Some(&namespace)).unwrap(),
            "/v2/ns/Example/resources/SandboxClass/docker-codex"
        );
    }

    #[test]
    fn render_resource_list_table_includes_kind_namespace_name_and_phase() {
        let resources = vec![resources_proto::Resource {
            api_version: "talon.impalasys.com/v1".to_string(),
            kind: "Agent".to_string(),
            metadata: Some(resources_proto::ResourceMeta {
                name: "coding".to_string(),
                namespace: "customers:acme".to_string(),
                labels: HashMap::new(),
                annotations: HashMap::new(),
                owner_references: Vec::new(),
                finalizers: Vec::new(),
                generation: 1,
                resource_version: "1".to_string(),
                uid: "uid".to_string(),
                deletion_timestamp: None,
            }),
            spec: None,
            status: Some(resources_proto::ResourceStatus {
                kind: Some(resources_proto::resource_status::Kind::Agent(
                    resources_proto::AgentStatus {
                        observed_generation: 1,
                        phase: "Ready".to_string(),
                        conditions: Vec::new(),
                        last_session_id: None,
                    },
                )),
            }),
        }];

        let table = render_resource_list_table(&resources);

        assert!(table.contains("KIND"));
        assert!(table.contains("NAMESPACE"));
        assert!(table.contains("Agent"));
        assert!(table.contains("customers:acme"));
        assert!(table.contains("coding"));
        assert!(table.contains("Ready"));
    }
}
