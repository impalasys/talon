"""Package language SDK artifacts after their Bazel builds."""

def _python_distribution_impl(ctx):
    output = ctx.actions.declare_directory(ctx.label.name)
    ctx.actions.run(
        executable = ctx.executable._tool,
        arguments = [ctx.file.manifest.dirname, output.path],
        inputs = ctx.files.sources + [ctx.file.manifest],
        outputs = [output],
        mnemonic = "PythonDistribution",
    )
    return [DefaultInfo(files = depset([output]))]

python_distribution = rule(
    implementation = _python_distribution_impl,
    attrs = {
        "sources": attr.label(mandatory = True),
        "manifest": attr.label(allow_single_file = True, mandatory = True),
        "_tool": attr.label(default = "//tools/bazel:python_packager", executable = True, cfg = "exec"),
    },
)
