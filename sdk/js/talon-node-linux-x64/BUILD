load("@aspect_rules_js//js:defs.bzl", "js_library")
load("@aspect_rules_js//npm:defs.bzl", "npm_package")

npm_package(
    name = "pkg",
    srcs = glob([
        "bin/**",
        "index.js",
        "package.json",
    ]),
    packable = True,
    visibility = ["//visibility:public"],
)

alias(
    name = "archive",
    actual = ":pkg.pack",
    tags = ["manual"],
    visibility = ["//visibility:public"],
)

exports_files(["package.json"])

js_library(
    name = "manifest",
    srcs = ["package.json"],
    visibility = ["//visibility:public"],
)
