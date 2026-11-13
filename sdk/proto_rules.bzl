"""Bind upstream proto compilation to this repository's pinned plugins."""

load("@rules_proto_grpc//:defs.bzl", "ProtoPluginInfo", "proto_compile_attrs", "proto_compile_impl", "proto_compile_toolchains")

# The upstream API exposes an implementation and attributes for custom plugins.
# Keep all action creation, toolchain resolution and output merging upstream.
sdk_proto_compile = rule(
    implementation = proto_compile_impl,
    attrs = dict(
        proto_compile_attrs,
        _plugins = attr.label_list(providers = [ProtoPluginInfo], cfg = "exec"),
    ),
    toolchains = proto_compile_toolchains,
)
