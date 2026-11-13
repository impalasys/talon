"""Talon release layout; archive construction is provided by rules_pkg."""

load("@bazel_skylib//rules:select_file.bzl", "select_file")
load("@rules_pkg//pkg:mappings.bzl", "pkg_attributes", "pkg_files", "strip_prefix")
load("@rules_pkg//pkg:tar.bzl", "pkg_tar")
load("@with_cfg.bzl", "with_cfg")

# Production artifacts always include both storage backends and optimized code.
production_filegroup, _production_filegroup = (
    with_cfg(native.filegroup)
        .set(Label("//:with_aws"), True)
        .set(Label("//:with_rocksdb"), True)
        .set("compilation_mode", "opt")
        .build()
)

def release_distribution(name, binaries, configs, platform, sdk_platform):
    pkg_files(
        name = name + "_binaries",
        srcs = [binaries],
        strip_prefix = strip_prefix.files_only(),
        attributes = pkg_attributes(mode = "0755"),
    )
    pkg_files(
        name = name + "_configs",
        srcs = [configs],
        strip_prefix = strip_prefix.from_root(),
        attributes = pkg_attributes(mode = "0644"),
    )
    pkg_tar(name = name + "_payload", srcs = [":" + name + "_binaries", ":" + name + "_configs"])
    native.genrule(
        name = name + "_checksums",
        srcs = [":" + name + "_payload"],
        outs = [name + "/SHA256SUMS"],
        tools = ["//tools/bazel:checksums"],
        cmd = "$(execpath //tools/bazel:checksums) contents $(location :" + name + "_payload) $@",
    )
    pkg_tar(
        name = name + "_runtime",
        deps = [":" + name + "_payload"],
        srcs = [":" + name + "_checksums"],
        package_dir = "talon-" + platform,
        package_file_name = name + "/talon-" + platform + ".tar.gz",
        extension = "tar.gz",
        mode = "0644",
    )
    select_file(name = name + "_node_binary", srcs = binaries, subpath = "talon-node")
    pkg_files(
        name = name + "_node_files",
        srcs = [":" + name + "_node_binary"],
        strip_prefix = strip_prefix.files_only(),
        attributes = pkg_attributes(mode = "0755"),
    )
    pkg_tar(
        name = name + "_node",
        srcs = [":" + name + "_node_files"],
        package_dir = "talon-node-" + sdk_platform,
        package_file_name = name + "/talon-node-" + sdk_platform + ".tar.gz",
        extension = "tar.gz",
        mode = "0644",
    )
    native.genrule(
        name = name + "_node_checksum",
        srcs = [":" + name + "_node"],
        outs = [name + "/node.sha256"],
        tools = ["//tools/bazel:checksums"],
        cmd = "$(execpath //tools/bazel:checksums) archive $(location :" + name + "_node) $@",
    )
    native.filegroup(name = name, srcs = [":" + name + suffix for suffix in ["_runtime", "_node", "_node_checksum", "_checksums"]])
