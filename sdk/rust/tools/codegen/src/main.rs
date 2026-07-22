// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use std::path::PathBuf;

const SOURCE_HEADER: &str =
    "// Copyright (C) 2026 Impala Systems, Inc.\n// SPDX-License-Identifier: AGPL-3.0-only\n\n";

#[derive(Debug)]
struct Service {
    name: String,
    field_name: String,
    client_mod: String,
    client_name: String,
    methods: Vec<Method>,
}

#[derive(Debug)]
struct Method {
    rpc_name: String,
    method_name: String,
    request: String,
    response: String,
    server_streaming: bool,
}

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .expect("repo root")
        .to_path_buf();
    let out_dir = root.join("sdk/rust/talon-client/src/generated");
    let v1_protos = talon_v1_protos(&root);
    let mut protos = vec![
        root.join("proto/config.proto"),
        root.join("proto/resources/common.proto"),
        root.join("proto/resources/agents.proto"),
        root.join("proto/resources/mcp.proto"),
        root.join("proto/resources/knowledge.proto"),
        root.join("proto/resources/namespaces.proto"),
        root.join("proto/resources/channels.proto"),
        root.join("proto/resources/schedules.proto"),
        root.join("proto/resources/secrets.proto"),
        root.join("proto/resources/workflows.proto"),
        root.join("proto/resources/deployments.proto"),
        root.join("proto/resources/files.proto"),
        root.join("proto/resources/sandboxes.proto"),
        root.join("proto/resources/sessions.proto"),
        root.join("proto/resources/skills.proto"),
        root.join("proto/resources/tasks.proto"),
        root.join("proto/resources/usage.proto"),
        root.join("proto/resources/workers.proto"),
        root.join("proto/resources/resource.proto"),
        root.join("proto/harness/llm.proto"),
        root.join("proto/data/api_keys.proto"),
        root.join("proto/data/connectors.proto"),
        root.join("proto/data/data.proto"),
        root.join("proto/data/routing.proto"),
        root.join("proto/data/session_submission.proto"),
        root.join("proto/data/session_journal_entry.proto"),
        root.join("proto/events.proto"),
        root.join("proto/external/connectors.proto"),
    ];
    protos.extend(v1_protos.iter().cloned());
    std::fs::create_dir_all(&out_dir).expect("create generated dir");
    for entry in std::fs::read_dir(&out_dir).expect("read generated dir") {
        let entry = entry.expect("generated entry");
        if entry.path().extension().is_some_and(|ext| ext == "rs") {
            std::fs::remove_file(entry.path()).expect("remove old generated file");
        }
    }
    tonic_build::configure()
        .build_client(true)
        .build_server(false)
        .out_dir(&out_dir)
        .protoc_arg("--experimental_allow_proto3_optional")
        .compile_protos(
            &protos,
            &[root.clone(), root.join("third_party/googleapis")],
        )
        .expect("generate rust SDK");

    let services = parse_talon_v1_services(&v1_protos);
    std::fs::write(out_dir.join("clientset.rs"), generate_clientset(&services))
        .expect("write generated clientset");

    for entry in std::fs::read_dir(&out_dir).expect("read generated dir") {
        let path = entry.expect("generated entry").path();
        if !path.extension().is_some_and(|ext| ext == "rs") {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("read generated source");
        if source.starts_with(SOURCE_HEADER) {
            continue;
        }
        std::fs::write(&path, format!("{SOURCE_HEADER}{source}")).expect("write generated source");
    }
}

fn talon_v1_protos(root: &std::path::Path) -> Vec<PathBuf> {
    [
        "namespaces.proto",
        "resources.proto",
        "sessions.proto",
        "channels.proto",
        "workflows.proto",
        "knowledge.proto",
        "files.proto",
        "search.proto",
        "auth.proto",
        "connectors.proto",
    ]
    .into_iter()
    .map(|file| root.join("proto/talon/v1").join(file))
    .collect()
}

fn parse_talon_v1_services(paths: &[PathBuf]) -> Vec<Service> {
    let mut services = Vec::new();
    let mut current: Option<Service> = None;

    for path in paths {
        let source = std::fs::read_to_string(path).expect("read talon v1 proto");
        let mut pending_rpc: Option<String> = None;
        for raw_line in source.lines() {
            let line = raw_line.trim();
            if let Some(name) = line
                .strip_prefix("service ")
                .and_then(|rest| rest.split_whitespace().next())
            {
                let service_domain = name.strip_suffix("Service").unwrap_or(name);
                current = Some(Service {
                    name: name.to_string(),
                    field_name: service_field_name(service_domain),
                    client_mod: format!("{}_client", to_snake_case(name)),
                    client_name: format!("{name}Client"),
                    methods: Vec::new(),
                });
                continue;
            }

            if line == "}" {
                if let Some(service) = current.take() {
                    services.push(service);
                }
                continue;
            }

            let Some(service) = current.as_mut() else {
                continue;
            };
            let rpc_line = if let Some(pending) = pending_rpc.as_mut() {
                pending.push(' ');
                pending.push_str(line);
                if !line.ends_with(';') {
                    continue;
                }
                pending_rpc.take().expect("pending rpc")
            } else {
                let Some(_) = line.strip_prefix("rpc ") else {
                    continue;
                };
                if !line.ends_with(';') {
                    pending_rpc = Some(line.to_string());
                    continue;
                }
                line.to_string()
            };
            if let Some(method) = parse_rpc_method(service, &rpc_line) {
                service.methods.push(method);
            }
        }
    }

    services
}

fn parse_rpc_method(service: &Service, line: &str) -> Option<Method> {
    let signature = line.strip_prefix("rpc ")?;
    let (rpc_name, rest) = signature.split_once('(')?;
    let (request, rest) = rest.split_once(") returns (")?;
    let response = rest.strip_suffix(");")?;
    let (server_streaming, response) = response
        .strip_prefix("stream ")
        .map(|response| (true, response))
        .unwrap_or((false, response));
    Some(Method {
        rpc_name: rpc_name.trim().to_string(),
        method_name: delegate_method_name(&service.name, rpc_name.trim()),
        request: rust_type_path(request.trim()),
        response: rust_type_path(response.trim()),
        server_streaming,
    })
}

fn generate_clientset(services: &[Service]) -> String {
    let mut source = String::new();
    source.push_str(SOURCE_HEADER);
    source.push_str("use crate::v1::{\n");
    for service in services {
        source.push_str(&format!(
            "    {}::{},\n",
            service.client_mod, service.client_name
        ));
    }
    source.push_str("};\n\n");

    source.push_str("#[derive(Debug)]\n");
    source.push_str("pub struct TalonClientset<T> {\n");
    for service in services {
        source.push_str(&format!(
            "    pub {}: {}<T>,\n",
            service.field_name, service.client_name
        ));
    }
    source.push_str("}\n\n");

    source.push_str(
        "impl<T> TalonClientset<T>\nwhere\n    T: tonic::client::GrpcService<tonic::body::BoxBody> + Clone,\n    T::Error: Into<tonic::codegen::StdError>,\n    T::ResponseBody: tonic::codegen::Body<Data = tonic::codegen::Bytes> + Send + 'static,\n    <T::ResponseBody as tonic::codegen::Body>::Error: Into<tonic::codegen::StdError> + Send,\n{\n",
    );
    source.push_str("    pub fn from_service(service: T) -> Self {\n");
    source.push_str("        Self {\n");
    for service in services {
        source.push_str(&format!(
            "            {}: {}::new(service.clone()),\n",
            service.field_name, service.client_name
        ));
    }
    source.push_str("        }\n");
    source.push_str("    }\n");
    source.push_str("}\n\n");

    source.push_str(
        "macro_rules! delegate_dynamic_unary_rpc {\n    ($name:ident, $field:ident, $method:ident, $request:ty, $response:ty $(,)?) => {\n        pub async fn $name(\n            &mut self,\n            request: $request,\n        ) -> Result<tonic::Response<$response>, tonic::Status> {\n            match self {\n                crate::TalonClient::Native(client) => client.$field.$method(request).await,\n                crate::TalonClient::GrpcWeb(client) => client.$field.$method(request).await,\n            }\n        }\n    };\n}\n\n",
    );
    source.push_str(
        "macro_rules! delegate_dynamic_server_streaming_rpc {\n    ($name:ident, $field:ident, $method:ident, $request:ty, $response:ty $(,)?) => {\n        pub async fn $name(\n            &mut self,\n            request: $request,\n        ) -> Result<tonic::Response<tonic::codec::Streaming<$response>>, tonic::Status> {\n            match self {\n                crate::TalonClient::Native(client) => client.$field.$method(request).await,\n                crate::TalonClient::GrpcWeb(client) => client.$field.$method(request).await,\n            }\n        }\n    };\n}\n\n",
    );

    source.push_str("impl crate::TalonClient {\n");
    for service in services {
        for method in &service.methods {
            let macro_name = if method.server_streaming {
                "delegate_dynamic_server_streaming_rpc"
            } else {
                "delegate_dynamic_unary_rpc"
            };
            source.push_str(&format!(
                "    {}!(\n        {},\n        {},\n        {},\n        {},\n        {},\n    );\n",
                macro_name,
                method.method_name,
                service.field_name,
                to_snake_case(&method.rpc_name),
                method.request,
                method.response
            ));
        }
    }
    source.push_str("}\n");

    source
}

fn service_field_name(service_domain: &str) -> String {
    match service_domain {
        "Auth" => "auth".to_string(),
        "Knowledge" => "knowledge".to_string(),
        domain => pluralize(&to_snake_case(domain)),
    }
}

fn delegate_method_name(service_name: &str, rpc_name: &str) -> String {
    match (service_name, rpc_name) {
        ("AuthService", _) => to_snake_case(rpc_name),
        ("SessionService", "SendMessage") => "send_message".to_string(),
        ("SessionService", "AnswerPermission") => "answer_session_permission".to_string(),
        ("SessionService", "StopGeneration") => "stop_session_generation".to_string(),
        _ => {
            let domain = service_name
                .strip_suffix("Service")
                .map(to_snake_case)
                .unwrap_or_else(|| to_snake_case(service_name));
            let rpc = to_snake_case(rpc_name);
            if rpc == "list" {
                format!("list_{}", pluralize(&domain))
            } else if matches!(rpc.as_str(), "create" | "get" | "delete" | "clear") {
                format!("{rpc}_{domain}")
            } else if rpc.contains(&domain) {
                rpc
            } else if let Some((verb, rest)) = rpc.split_once('_') {
                format!("{verb}_{domain}_{rest}")
            } else {
                format!("{rpc}_{domain}")
            }
        }
    }
}

fn rust_type_path(proto_type: &str) -> String {
    let proto_type = proto_type.trim_start_matches('.');
    if let Some(rest) = proto_type.strip_prefix("talon.v1.") {
        format!("crate::v1::{rest}")
    } else if let Some(rest) = proto_type.strip_prefix("talon.data.") {
        format!("crate::data::{rest}")
    } else if let Some(rest) = proto_type.strip_prefix("talon.events.") {
        format!("crate::events::{rest}")
    } else if let Some(rest) = proto_type.strip_prefix("talon.external.") {
        format!("crate::external::{rest}")
    } else if let Some(rest) = proto_type.strip_prefix("talon.resources.") {
        format!("crate::resources::{rest}")
    } else {
        format!("crate::v1::{proto_type}")
    }
}

fn pluralize(value: &str) -> String {
    if let Some(stem) = value.strip_suffix('y') {
        format!("{stem}ies")
    } else if value.ends_with('s')
        || value.ends_with('x')
        || value.ends_with("ch")
        || value.ends_with("sh")
    {
        format!("{value}es")
    } else {
        format!("{value}s")
    }
}

fn to_snake_case(value: &str) -> String {
    let mut out = String::new();
    let mut prev_was_lower_or_digit = false;
    for ch in value.chars() {
        if ch.is_ascii_uppercase() {
            if prev_was_lower_or_digit {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
            prev_was_lower_or_digit = false;
        } else {
            out.push(ch);
            prev_was_lower_or_digit = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        }
    }
    out
}
