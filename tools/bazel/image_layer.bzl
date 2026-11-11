"""Create deterministic OCI filesystem layers from declared Bazel outputs."""

def _image_layer_impl(ctx):
    output = ctx.actions.declare_file(ctx.label.name + ".tar")
    entries = []
    inputs = []
    for file in ctx.files.binaries:
        entries.append([file.path, "usr/local/bin/" + file.basename, "755"])
        inputs.append(file)
    for file in ctx.files.configs:
        relative = "models/" + file.basename if file.dirname.endswith("/models") else file.basename
        entries.append([file.path, "data/talon/" + relative, "644"])
        inputs.append(file)
    for label, destination in ctx.attr.files.items():
        for file in label[DefaultInfo].files.to_list():
            entries.append([file.path, destination, "755" if destination.endswith("/docker") else "644"])
            inputs.append(file)
    for file in ctx.files.web:
        entries.append([file.path, "usr/share/nginx/html", "644"])
        inputs.append(file)
    for dep in ctx.attr.python_packages:
        for file in dep[DefaultInfo].default_runfiles.files.to_list():
            if "site-packages/" in file.short_path:
                destination = "usr/local/lib/python3.13/site-packages/" + file.short_path.split("site-packages/", 1)[1]
            elif file.basename == "monty":
                destination = "usr/local/bin/monty"
            else:
                continue
            entries.append([file.path, destination, "755" if file.basename == "monty" else "644"])
            inputs.append(file)
    manifest = ctx.actions.declare_file(ctx.label.name + ".json")
    ctx.actions.write(manifest, json.encode({"entries": entries, "python": bool(ctx.attr.python_packages), "monty": ctx.file.monty_whl.path if ctx.file.monty_whl else ""}))
    ctx.actions.run(executable = ctx.executable._tool, arguments = [manifest.path, output.path], inputs = depset(inputs + [manifest] + ctx.files.monty_whl), outputs = [output], mnemonic = "TalonOCILayer")
    return [DefaultInfo(files = depset([output]))]

image_layer = rule(implementation = _image_layer_impl, attrs = {
    "monty_whl": attr.label(allow_single_file = True),
    "binaries": attr.label(),
    "configs": attr.label(),
    "files": attr.label_keyed_string_dict(allow_files = True),
    "web": attr.label(),
    "python_packages": attr.label_list(),
    "_tool": attr.label(default = "//tools/bazel:image_layer", executable = True, cfg = "exec"),
})

def _debian_layer_impl(ctx):
    output = ctx.actions.declare_file(ctx.label.name + ".tar")
    ctx.actions.run(executable = ctx.executable._tool, arguments = [output.path] + [file.path for file in ctx.files.packages], inputs = ctx.files.packages, outputs = [output], mnemonic = "TalonDebianLayer")
    return [DefaultInfo(files = depset([output]))]

debian_layer = rule(implementation = _debian_layer_impl, attrs = {"packages": attr.label(mandatory = True), "_tool": attr.label(default = "//tools/bazel:merge_layers", executable = True, cfg = "exec")})
