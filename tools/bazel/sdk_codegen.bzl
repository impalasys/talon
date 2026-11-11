"""Generate the checked-in SDK sources with pinned Bazel toolchains."""

def _sdk_sources_impl(ctx):
    output = ctx.actions.declare_directory(ctx.label.name)
    args = ctx.actions.args()
    args.add("--output", output.path)
    args.add("--config", ctx.file.config.path)
    for name in ["protoc", "go", "go_grpc", "java", "es", "connect", "rust", "clientsets", "gofmt"]:
        args.add("--" + name.replace("_", "-"), getattr(ctx.executable, name).path)
    ctx.actions.run(
        executable = ctx.executable._driver,
        arguments = [args],
        inputs = ctx.files.protos + [ctx.file.config],
        tools = [getattr(ctx.attr, name)[DefaultInfo].files_to_run for name in ["protoc", "go", "go_grpc", "java", "es", "connect", "rust", "clientsets", "gofmt"]],
        outputs = [output],
        mnemonic = "TalonSDKCodegen",
    )
    return [DefaultInfo(files = depset([output]))]

sdk_sources = rule(implementation = _sdk_sources_impl, attrs = dict(
    {name: attr.label(executable = True, cfg = "exec", allow_files = True, mandatory = True) for name in ["protoc", "go", "go_grpc", "java", "es", "connect", "rust", "clientsets", "gofmt"]},
    protos = attr.label(mandatory = True),
    config = attr.label(allow_single_file = True, mandatory = True),
    _driver = attr.label(default = "//tools/bazel:sdk_codegen", executable = True, cfg = "exec"),
))
