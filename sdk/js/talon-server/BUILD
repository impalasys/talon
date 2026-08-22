load("@aspect_rules_js//js:defs.bzl", "js_run_binary")
load("@aspect_rules_js//npm:defs.bzl", "npm_package")
load("@npm//:defs.bzl", "npm_link_all_packages")
load("@npm//talon/sdk/js/talon-server:typescript/package_json.bzl", typescript_bin = "bin")

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
        ":data",
        ":build",
    ],
    visibility = ["//visibility:public"],
)
