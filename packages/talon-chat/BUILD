load("@aspect_rules_js//js:defs.bzl", "js_library", "js_run_binary", "js_test")
load("@aspect_rules_js//npm:defs.bzl", "npm_package")
load("@npm//:defs.bzl", "npm_link_all_packages")
load("@npm//packages/talon-chat:@playwright/test/package_json.bzl", playwright_bin = "bin")
load("@npm//packages/talon-chat:storybook/package_json.bzl", storybook_bin = "bin")
load("@npm//packages/talon-chat:tsup/package_json.bzl", tsup_bin = "bin")
load("//tools/bazel:distributions.bzl", "npm_archive")

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
            "tsup.config.ts",
            "README.md",
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
    data = ["//sdk/js/talon-client:pkg"],
    visibility = ["//visibility:public"],
    deps = [":node_modules"],
)

tsup_bin.tsup_binary(
    name = "tsup",
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
        "--config",
        "tsup.config.ts",
    ],
    chdir = package_name(),
    out_dirs = ["dist"],
    tool = ":tsup",
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
    data = ["//sdk/js/talon-client:pkg"],
    visibility = ["//visibility:public"],
)

js_library(
    name = "all_sources",
    srcs = glob(["src/**"]),
    data = ["//sdk/js/talon-client:pkg"],
    visibility = ["//visibility:public"],
    deps = [":node_modules"],
)

js_test(
    name = "unit_tests",
    args = [
        "--experimental-strip-types",
        "--test",
    ] + glob(["src/**/*.test.ts"]),
    chdir = package_name(),
    data = [
        ":all_sources",
        ":node_modules",
        "//sdk/js/talon-client:pkg",
    ],
    entry_point = "//tools/bazel:node_tests_entry",
)

npm_archive(
    name = "archive",
    package = ":pkg",
    visibility = ["//visibility:public"],
)

js_test(
    name = "package_test",
    args = ["$(rootpath :archive)"],
    data = [
        ":archive",
        ":node_modules",
        "//sdk/js/talon-client:pkg",
    ],
    entry_point = "scripts/check-package.mjs",
)

exports_files(["package.json"])

js_library(
    name = "storybook_sources",
    srcs = glob([
        ".storybook/**",
        "src/**",
        "package.json",
        "tsconfig.json",
    ]),
    visibility = ["//visibility:public"],
)

storybook_bin.storybook_binary(
    name = "storybook_cli",
    data = [
        ":node_modules",
        ":storybook_sources",
        "//sdk/js/talon-client:pkg",
    ],
)

js_run_binary(
    name = "storybook",
    srcs = [
        ":node_modules",
        ":storybook_sources",
        "//sdk/js/talon-client:pkg",
    ],
    args = [
        "build",
        "--disable-telemetry",
        "--output-dir",
        "storybook-static",
    ],
    chdir = package_name(),
    env = {"STORYBOOK_DISABLE_TELEMETRY": "1"},
    out_dirs = ["storybook-static"],
    tool = ":storybook_cli",
    visibility = ["//visibility:public"],
)

playwright_bin.playwright_binary(
    name = "playwright",
    data = [":node_modules"],
    visibility = ["//visibility:public"],
)

js_test(
    name = "screenshots",
    args = ["$(rootpath :storybook)"],
    data = [
        ":node_modules",
        ":storybook",
    ],
    entry_point = "scripts/capture-storybook-screenshots.mjs",
    env_inherit = [
        "PLAYWRIGHT_BROWSERS_PATH",
        "HOME",
    ],
    tags = [
        "manual",
        "no-cache",
        "no-remote-exec",
    ],
)
