load("@aspect_rules_js//js:defs.bzl", "js_library", "js_run_binary")
load("@aspect_rules_js//npm:defs.bzl", "npm_package")
load("@npm//:defs.bzl", "npm_link_all_packages")
load("@npm//talon/packages/talon-chat:typescript/package_json.bzl", typescript_bin = "bin")

npm_link_all_packages(name = "node_modules")

filegroup(
    name = "srcs",
    srcs = glob(
        [
            "package.json",
            "src/**/*.d.ts",
            "src/**/*.ts",
            "src/**/*.tsx",
            "tsconfig.json",
        ],
        exclude = [
            "src/**/*.stories.tsx",
            "src/**/*.test.ts",
            "src/**/*.test.tsx",
        ],
    ),
    visibility = ["//visibility:private"],
)

js_library(
    name = "src",
    srcs = [":srcs"],
    visibility = ["//visibility:public"],
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
    data = ["//talon/sdk/js/talon-client:pkg"],
    visibility = ["//visibility:public"],
)
