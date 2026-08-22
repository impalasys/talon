load("@aspect_rules_js//js:defs.bzl", "js_run_binary")
load("@npm//:defs.bzl", "npm_link_all_packages")
load("@npm//talon/ui:typescript/package_json.bzl", typescript_bin = "bin")
load("@npm//talon/ui:vite/package_json.bzl", vite_bin = "bin")

npm_link_all_packages(name = "node_modules")

filegroup(
    name = "srcs",
    srcs = glob([
        "**/*.css",
        "**/*.d.ts",
        "**/*.html",
        "**/*.json",
        "**/*.mts",
        "**/*.ts",
        "**/*.tsx",
        "package.json",
        "tsconfig.json",
        "vite.config.ts",
    ], exclude = ["node_modules/**"], allow_empty = True),
    visibility = ["//visibility:private"],
)

typescript_bin.tsc_test(
    name = "typecheck",
    args = [
        "--noEmit",
        "-p",
        package_name() + "/tsconfig.json",
    ],
    data = [
        ":node_modules",
        ":srcs",
        "//talon/packages/talon-chat:src",
        "//talon/sdk/js/talon-client:src",
    ],
    tags = ["pre-push"],
)

vite_bin.vite_binary(
    name = "vite_js_binary",
    data = [
        ":node_modules",
        ":srcs",
        "//talon/packages/talon-chat:src",
        "//talon/sdk/js/talon-client:src",
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
    out_dirs = ["dist"],
    tool = ":vite_js_binary",
    visibility = ["//visibility:public"],
)

alias(
    name = "dist",
    actual = ":vite_client",
    visibility = ["//visibility:public"],
)
