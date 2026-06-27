load("@aspect_rules_js//npm:defs.bzl", "npm_package")

filegroup(
    name = "data",
    srcs = glob(
        [
            "dist/**",
            "src/**",
            "package.json",
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
    visibility = ["//visibility:public"],
)
