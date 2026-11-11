"""Create an npm-compatible archive without invoking another build."""
import gzip
import io
import json
from pathlib import Path
import sys
import tarfile

root, output = map(Path, sys.argv[1:3])
manifest = json.loads((root / "package.json").read_text())
for section in ["dependencies", "devDependencies", "optionalDependencies", "peerDependencies"]:
    for name, value in manifest.get(section, {}).items():
        if value.startswith("workspace:"):
            manifest[section][name] = value.removeprefix("workspace:")
if sys.argv[3] == "required" and not (root / "bin/talon-node").is_file():
    raise SystemExit("A Bazel-built talon-node release binary must be staged before packaging")
allowed = manifest.get("files", [])
with output.open("wb") as file, gzip.GzipFile(fileobj=file, mode="wb", mtime=0, filename="") as compressed:
    with tarfile.open(fileobj=compressed, mode="w") as tar:
        for path in sorted(root.rglob("*")):
            if not path.is_file():
                continue
            relative = path.relative_to(root).as_posix()
            if relative != "package.json" and not path.name.lower().startswith(("readme", "license")) and not any(relative == item or relative.startswith(item.rstrip("/") + "/") for item in allowed):
                continue
            content = (json.dumps(manifest, indent=2) + "\n").encode() if relative == "package.json" else path.read_bytes()
            info = tarfile.TarInfo("package/" + relative)
            info.size = len(content)
            info.mode = 0o755 if relative == "bin/talon-node" else 0o644
            tar.addfile(info, io.BytesIO(content))
