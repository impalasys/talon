load("@aspect_rules_js//js:defs.bzl", "js_library", "js_run_binary", "js_test")
load("@npm//:defs.bzl", "npm_link_all_packages")
load("@npm//ui:@playwright/test/package_json.bzl", playwright_bin = "bin")
load("@npm//ui:jest/package_json.bzl", jest_bin = "bin")
load("@npm//ui:typescript/package_json.bzl", typescript_bin = "bin")
load("@npm//ui:vite/package_json.bzl", vite_bin = "bin")

npm_link_all_packages(name = "node_modules")

js_library(
    name = "srcs",
    srcs = glob(
        [
            "**/*.css",
            "**/*.d.ts",
            "**/*.html",
            "**/*.json",
            "**/*.js",
            "**/*.mts",
            "**/*.ts",
            "**/*.tsx",
            "package.json",
            "tsconfig.json",
            "vite.config.ts",
        ],
        allow_empty = True,
        exclude = ["node_modules/**"],
    ),
    visibility = ["//visibility:private"],
)

typescript_bin.tsc_test(
    name = "typecheck",
    args = [
        "--noEmit",
        "--incremental",
        "false",
        "-p",
        package_name() + "/tsconfig.json",
    ],
    data = [
        ":node_modules",
        ":srcs",
        "//packages/talon-chat:src",
        "//sdk/js/talon-client:src",
    ],
    # Canonicalize shared React declarations across the runfiles symlinks.
    patch_node_fs = False,
    tags = ["pre-push"],
)

vite_bin.vite_binary(
    name = "vite_js_binary",
    data = [
        ":node_modules",
        ":srcs",
        "//packages/talon-chat:src",
        "//sdk/js/talon-client:src",
    ],
    visibility = ["//visibility:private"],
)

js_run_binary(
    name = "vite_client",
    srcs = [
        ":node_modules",
        ":srcs",
    ],
    args = ["build"],
    chdir = package_name(),
    env = {"VITE_GATEWAY_URL": "$(GATEWAY_URL)"},
    out_dirs = ["dist"],
    tool = ":vite_js_binary",
    visibility = ["//visibility:public"],
)

alias(
    name = "dist",
    actual = ":vite_client",
    visibility = ["//visibility:public"],
)

jest_bin.jest_test(
    name = "unit_tests",
    size = "medium",
    args = [
        "--config",
        "jest.config.js",
        "--runInBand",
    ],
    chdir = package_name(),
    data = [
        ":node_modules",
        ":srcs",
        "//packages/talon-chat:all_sources",
        "//sdk/js/talon-client:pkg",
    ],
)

exports_files(["package.json"])

vite_bin.vite_binary(
    name = "preview",
    args = [
        "preview",
        "--host",
        "0.0.0.0",
        "--port",
        "3000",
        "--strictPort",
    ],
    chdir = package_name(),
    data = [
        ":dist",
        ":node_modules",
        ":srcs",
        "//packages/talon-chat:src",
        "//sdk/js/talon-client:pkg",
    ],
    visibility = ["//visibility:public"],
)

playwright_bin.playwright_binary(
    name = "playwright",
    data = [
        ":node_modules",
        ":srcs",
    ],
    visibility = ["//visibility:public"],
)

js_test(
    name = "e2e",
    size = "large",
    args = [
        "$(rootpath //tests:e2e_stack)",
        "$(rootpath :preview)",
        "$(rootpath :playwright)",
    ],
    data = [
        ":node_modules",
        ":playwright",
        ":preview",
        ":srcs",
        "//tests:e2e_stack",
    ],
    entry_point = "bazel-e2e.mjs",
    env_inherit = [
        "PLAYWRIGHT_BROWSERS_PATH",
        "HOME",
        "DOCKER_HOST",
    ],
    no_copy_to_bin = ["//tests:e2e_stack"],
    tags = [
        "exclusive",
        "manual",
        "no-cache",
        "no-remote-exec",
        "requires-docker",
    ],
)
