load("@aspect_rules_js//npm:defs.bzl", "npm_package")

filegroup(
    name = "data",
    srcs = glob(
        [
            "dist/**",
            "src/**",
            "package.json",
            "tsup.config.ts",
            "tsconfig.json",
        ],
        allow_empty = True,
    ),
    visibility = ["//visibility:public"],
)

npm_package(
    name = "pkg",
    srcs = [
        "package.json",
        ":data",
    ],
    data = ["//talon/sdk/js/talon-client:pkg"],
    visibility = ["//visibility:public"],
)
