"""SDK destination ownership, assembled and updated by upstream bazel_lib rules."""

load("@bazel_lib//lib:diff_test.bzl", "diff_test")
load("@bazel_lib//lib:directory_path.bzl", "directory_path")
load("@bazel_lib//lib:write_source_files.bzl", "write_source_files")
load("@rules_python//python:defs.bzl", "py_test")

# Only these entire directories are replaced. Mixed SDK roots are never owned.
_GENERATED_DIRECTORIES = [
    "sdk/go/talon-client/talon",
    "sdk/java/talon-client/src/main/java/talon",
    "sdk/js/talon-client/src/gen",
    "sdk/python/talon-client/src/talon_client/proto",
    "sdk/rust/talon-client/src/generated",
]
_GENERATED_FILES = [
    "sdk/go/talon-client/clientset_gen.go",
    "sdk/java/talon-client/src/main/java/systems/impala/talon/client/TalonClientset.java",
    "sdk/js/talon-client/src/clientset.ts",
] + [
    "sdk/python/talon-client/src/talon_client/" + name + ".py"
    for name in ["__init__", "config", "data", "events", "external", "harness", "resources", "v1", "clientset"]
]

def sdk_source_updates():
    """Update explicit destinations; compare declared file trees for freshness."""
    files = {}
    for i, destination in enumerate(_GENERATED_DIRECTORIES + _GENERATED_FILES):
        name = "update_input_%d" % i
        directory_path(
            name = name,
            directory = ":generated_with_metadata",
            path = destination,
        )
        files["//:" + destination] = ":" + name

    # Source-directory labels hide file additions/deletions from Bazel. Build a
    # tree from each package's file glob and compare that declared artifact.
    write_source_files(
        name = "update_generated",
        files = files,
        check_that_out_file_exists = False,
        diff_test = False,
    )
    diff_test(
        name = "generated_test",
        file1 = ":generated_with_metadata",
        file2 = ":checked_in_generated",
        failure_message = "Generated SDK files are stale. Run scripts/sdk/generate.sh",
    )
    py_test(
        name = "update_generated_test",
        srcs = ["update_generated_test.py"],
        args = ["$(rootpath :update_generated)"] + _GENERATED_DIRECTORIES + _GENERATED_FILES,
        data = [":update_generated", ":generated_with_metadata", ":checked_in_generated"],
        legacy_create_init = False,
        deps = ["@rules_python//python/runfiles"],
    )
