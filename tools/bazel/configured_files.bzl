"""Build runtime artifacts with explicit, cacheable production configurations."""

def _runtime_transition_impl(settings, attr):
    return {
        "//:with_aws": True,
        "//:with_rocksdb": True,
        "//command_line_option:compilation_mode": "opt" if attr.release else "fastbuild",
    }

_runtime_transition = transition(
    implementation = _runtime_transition_impl,
    inputs = [],
    outputs = ["//:with_aws", "//:with_rocksdb", "//command_line_option:compilation_mode"],
)

def _configured_files_impl(ctx):
    dependencies = ctx.attr.targets
    files = depset(transitive = [dep[DefaultInfo].files for dep in dependencies])
    runfiles = ctx.runfiles(transitive_files = files)
    for dep in dependencies:
        runfiles = runfiles.merge(dep[DefaultInfo].default_runfiles)
    return [DefaultInfo(files = files, runfiles = runfiles)]

configured_files = rule(
    implementation = _configured_files_impl,
    attrs = {
        "targets": attr.label_list(cfg = _runtime_transition),
        "release": attr.bool(default = False),
        "_allowlist_function_transition": attr.label(default = "@bazel_tools//tools/allowlists/function_transition_allowlist"),
    },
)
