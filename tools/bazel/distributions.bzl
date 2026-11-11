"""Package language SDK artifacts after their Bazel builds."""

def _npm_archive_impl(ctx):
    output = ctx.actions.declare_file(ctx.label.name + ".tgz")
    ctx.actions.run(
        executable = ctx.executable._tool,
        arguments = [ctx.file.package.path, output.path, "required" if ctx.attr.require_binary else "optional"],
        inputs = [ctx.file.package],
        outputs = [output],
        mnemonic = "NpmArchive",
    )
    return [DefaultInfo(files = depset([output]))]

npm_archive = rule(
    implementation = _npm_archive_impl,
    attrs = {
        "package": attr.label(allow_single_file = True, mandatory = True),
        "require_binary": attr.bool(default = False),
        "_tool": attr.label(default = "//tools/bazel:npm_archiver", executable = True, cfg = "exec"),
    },
)

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
