load("@aspect_rules_js//js:defs.bzl", "js_library")
load("@aspect_rules_js//npm:defs.bzl", "npm_package")
load("//tools/bazel:distributions.bzl", "npm_archive")

npm_package(
    name = "pkg",
    srcs = glob([
        "bin/**",
        "index.js",
        "package.json",
    ]),
    visibility = ["//visibility:public"],
)

npm_archive(
    name = "archive",
    package = ":pkg",
    require_binary = True,
    tags = ["manual"],
    visibility = ["//visibility:public"],
)

exports_files(["package.json"])

js_library(
    name = "manifest",
    srcs = ["package.json"],
    visibility = ["//visibility:public"],
)
