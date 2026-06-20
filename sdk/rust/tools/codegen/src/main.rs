// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use std::path::PathBuf;

const SOURCE_HEADER: &str = "// Copyright (C) 2026 Impala Systems, Inc.\n// SPDX-License-Identifier: AGPL-3.0-only\n\n";

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .expect("repo root")
        .to_path_buf();
    let out_dir = root.join("sdk/rust/talon-client/src/generated");
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
            &[
                root.join("proto/config.proto"),
                root.join("proto/resources/common.proto"),
                root.join("proto/resources/agents.proto"),
                root.join("proto/resources/mcp.proto"),
                root.join("proto/resources/knowledge.proto"),
                root.join("proto/resources/namespaces.proto"),
                root.join("proto/resources/channels.proto"),
                root.join("proto/resources/schedules.proto"),
                root.join("proto/resources/workflows.proto"),
                root.join("proto/resources/deployments.proto"),
                root.join("proto/resources/sandboxes.proto"),
                root.join("proto/resources/sessions.proto"),
                root.join("proto/resources/skills.proto"),
                root.join("proto/resources/usage.proto"),
                root.join("proto/resources/workers.proto"),
                root.join("proto/resources/resource.proto"),
                root.join("proto/harness/llm.proto"),
                root.join("proto/data/data.proto"),
                root.join("proto/data/session_submission.proto"),
                root.join("proto/data/session_journal_entry.proto"),
                root.join("proto/events.proto"),
                root.join("proto/gateway.proto"),
            ],
            &[root.clone(), root.join("third_party/googleapis")],
        )
        .expect("generate rust SDK");

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
