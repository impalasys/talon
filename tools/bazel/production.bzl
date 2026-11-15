"""Production feature and optimization policy applied by upstream with_cfg."""

load("@with_cfg.bzl", "with_cfg")

# Production artifacts always include both storage backends and optimized code.
production_filegroup, _production_filegroup = (
    with_cfg(native.filegroup)
        .set(Label("//:with_aws"), True)
        .set(Label("//:with_rocksdb"), True)
        .set("compilation_mode", "opt")
        .build()
)
