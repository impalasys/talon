"""Deterministic runtime distributions built from Bazel binaries."""

def _release_distribution_impl(ctx):
    output = ctx.actions.declare_directory(ctx.label.name)
    args = ctx.actions.args()
    args.add("--output", output.path)
    args.add("--platform", ctx.attr.platform)
    args.add("--sdk-platform", ctx.attr.sdk_platform)
    for file in ctx.files.binaries:
        args.add("--binary", file.path)
    for file in ctx.files.configs:
        args.add("--config", file.path)
    ctx.actions.run(
        executable = ctx.executable._packager,
        arguments = [args],
        inputs = ctx.files.binaries + ctx.files.configs,
        outputs = [output],
        mnemonic = "TalonReleaseDistribution",
        progress_message = "Packaging Talon release binaries",
    )
    return [DefaultInfo(files = depset([output]))]

release_distribution = rule(
    implementation = _release_distribution_impl,
    attrs = {
        "binaries": attr.label(mandatory = True),
        "configs": attr.label(mandatory = True),
        "platform": attr.string(mandatory = True),
        "sdk_platform": attr.string(mandatory = True),
        "_packager": attr.label(default = "//tools/bazel:release_packager", executable = True, cfg = "exec"),
    },
)

def _container_context_impl(ctx):
    output = ctx.actions.declare_directory(ctx.label.name)
    args = ctx.actions.args()
    args.add("--output", output.path)
    args.add("--dockerfile", ctx.file.dockerfile.path)
    for file in ctx.files.binaries:
        args.add("--binary", file.path)
    for file in ctx.files.configs:
        args.add("--config", file.path)
    if ctx.file.requirements:
        args.add("--requirements", ctx.file.requirements.path)
    for file in ctx.files.web:
        args.add("--web", file.path)
    ctx.actions.run(
        executable = ctx.executable._builder,
        arguments = [args],
        inputs = ctx.files.binaries + ctx.files.configs + ctx.files.web + ctx.files.requirements + [ctx.file.dockerfile],
        outputs = [output],
        mnemonic = "TalonContainerContext",
    )
    return [DefaultInfo(files = depset([output]))]

container_context = rule(
    implementation = _container_context_impl,
    attrs = {
        "binaries": attr.label(),
        "configs": attr.label(),
        "web": attr.label(),
        "dockerfile": attr.label(allow_single_file = True, mandatory = True),
        "requirements": attr.label(allow_single_file = True),
        "_builder": attr.label(default = "//tools/bazel:context_builder", executable = True, cfg = "exec"),
    },
)
