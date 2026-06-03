use std::path::PathBuf;

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
                root.join("proto/models.proto"),
                root.join("proto/manifests.proto"),
                root.join("proto/events.proto"),
                root.join("proto/gateway.proto"),
            ],
            &[root.clone(), root.join("third_party/googleapis")],
        )
        .expect("generate rust SDK");
}
