"""Shared wiring for independently compiled internal Rust workspace crates."""

load("@crates//:defs.bzl", "aliases", "all_crate_deps")
load("@rules_rust//rust:defs.bzl", "rust_library", "rust_test")

def talon_crate(name, deps = [], test_deps = [], unit_tests = True, compile_data = [], **kwargs):
    native.exports_files(["Cargo.toml"])
    rust_library(
        name = name,
        srcs = native.glob(["src/**/*.rs"]),
        crate_root = "src/lib.rs",
        edition = "2021",
        aliases = aliases(),
        proc_macro_deps = all_crate_deps(proc_macro = True),
        deps = all_crate_deps(normal = True) + deps,
        compile_data = native.glob(["src/**"], exclude = ["src/**/*.rs"], allow_empty = True) + compile_data,
        visibility = ["//visibility:public"],
        **kwargs
    )
    if unit_tests:
        rust_test(
            name = "unit_tests",
            crate = ":" + name,
            edition = "2021",
            aliases = aliases(normal_dev = True),
            proc_macro_deps = all_crate_deps(proc_macro_dev = True),
            deps = all_crate_deps(normal_dev = True) + test_deps,
        )
