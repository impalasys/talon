load("@aspect_rules_js//js:defs.bzl", "js_library", "js_run_binary", "js_test")
load("@aspect_rules_js//npm:defs.bzl", "npm_package")
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

js_test(
    name = "unit_tests",
    args = [
        "--test",
        "$(rootpath :build)/index.test.js",
    ],
    data = [
        ":build",
        ":node_modules",
    ],
    entry_point = "//tools/bazel:node_tests_entry",
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
