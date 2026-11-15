load("@aspect_rules_js//js:defs.bzl", "js_library", "js_run_binary", "js_test")
load("@aspect_rules_js//npm:defs.bzl", "npm_package")
load("@bazel_lib//lib:directory_path.bzl", "directory_path")
load("@npm//:defs.bzl", "npm_link_all_packages")
load("@npm//sdk/js/talon-server:typescript/package_json.bzl", typescript_bin = "bin")

npm_link_all_packages(name = "node_modules")

filegroup(
    name = "srcs",
    srcs = glob([
        "package.json",
        "src/**/*.ts",
        "tsconfig.json",
    ]),
    visibility = ["//visibility:private"],
)

typescript_bin.tsc_binary(
    name = "tsc",
    data = [":node_modules"],
    visibility = ["//visibility:private"],
)

js_run_binary(
    name = "build",
    srcs = [
        ":node_modules",
        ":srcs",
    ],
    args = [
        "-p",
        "tsconfig.json",
    ],
    chdir = package_name(),
    out_dirs = ["dist"],
    tool = ":tsc",
    visibility = ["//visibility:public"],
)

filegroup(
    name = "data",
    srcs = [":srcs"],
    visibility = ["//visibility:public"],
)

npm_package(
    name = "pkg",
    srcs = [
        "package.json",
        ":build",
        ":data",
    ],
    packable = True,
    visibility = ["//visibility:public"],
)

directory_path(
    name = "unit_test_entry",
    directory = ":build",
    path = "index.test.js",
)

js_test(
    name = "unit_tests",
    data = [
        ":build",
        ":node_modules",
    ],
    entry_point = ":unit_test_entry",
    node_options = ["--test"],
)

alias(
    name = "archive",
    actual = ":pkg.pack",
    visibility = ["//visibility:public"],
)

exports_files(["package.json"])

js_library(
    name = "manifest",
    srcs = ["package.json"],
    visibility = ["//visibility:public"],
)
