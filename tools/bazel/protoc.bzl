"""Expose the selected protobuf compiler to Cargo build scripts."""

_PROTO_TOOLCHAIN = "@protobuf//bazel/private:proto_toolchain_type"

def _protoc_binary_impl(ctx):
    compiler = ctx.toolchains[_PROTO_TOOLCHAIN].proto.proto_compiler
    executable = ctx.actions.declare_file(ctx.label.name)
    ctx.actions.symlink(output = executable, target_file = compiler.executable, is_executable = True)
    return [DefaultInfo(executable = executable, runfiles = ctx.runfiles(files = [compiler.executable]))]

protoc_binary = rule(
    implementation = _protoc_binary_impl,
    executable = True,
    toolchains = [_PROTO_TOOLCHAIN],
)
