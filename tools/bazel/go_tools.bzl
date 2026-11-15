"""Expose gofmt from the selected Go toolchain."""

def _gofmt_impl(ctx):
    sdk = ctx.toolchains["@rules_go//go:toolchain"].sdk
    tool = [f for f in sdk.tools.to_list() if f.basename == "gofmt"][0]
    output = ctx.actions.declare_file(ctx.label.name)
    ctx.actions.symlink(output = output, target_file = tool, is_executable = True)
    return [DefaultInfo(executable = output, runfiles = ctx.runfiles(files = [tool]))]

gofmt_binary = rule(implementation = _gofmt_impl, executable = True, toolchains = ["@rules_go//go:toolchain"])
