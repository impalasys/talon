"""Package already-built binaries; archive metadata is reproducible."""

import argparse
import gzip
import hashlib
from pathlib import Path
import shutil
import tarfile


def archive(source: Path, destination: Path) -> None:
    with destination.open("wb") as output, gzip.GzipFile(fileobj=output, mode="wb", mtime=0, filename="") as compressed:
        with tarfile.open(fileobj=compressed, mode="w") as tar:
            for path in [source, *sorted(source.rglob("*"))]:
                info = tar.gettarinfo(str(path), arcname=str(Path(source.name) / path.relative_to(source)))
                info.uid = info.gid = info.mtime = 0
                info.uname = info.gname = ""
                info.mode = 0o755 if path.is_dir() or path.name.startswith("talon-") else 0o644
                if path.is_file():
                    with path.open("rb") as content:
                        tar.addfile(info, content)
                else:
                    tar.addfile(info)


def package(output: Path, platform: str, sdk_platform: str, binaries: list[Path], configs: list[Path]) -> None:
    output.mkdir(parents=True, exist_ok=True)
    runtime = output / ("talon-" + platform)
    runtime.mkdir()
    for binary in binaries:
        shutil.copyfile(binary, runtime / binary.name)
        (runtime / binary.name).chmod(0o755)
    for config in configs:
        # Config labels preserve their repository-relative paths.
        relative = Path("models") / config.name if config.parent.name == "models" else Path(config.name)
        destination = runtime / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(config, destination)
    checksums = "".join(
        f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.relative_to(runtime).as_posix()}\n"
        for path in sorted(runtime.rglob("*")) if path.is_file()
    )
    (runtime / "SHA256SUMS").write_text(checksums)
    archive(runtime, output / (runtime.name + ".tar.gz"))
    node = output / ("talon-node-" + sdk_platform)
    node.mkdir()
    shutil.copyfile(runtime / "talon-node", node / "talon-node")
    (node / "talon-node").chmod(0o755)
    node_archive = output / (node.name + ".tar.gz")
    archive(node, node_archive)
    (output / (node_archive.name + ".sha256")).write_text(f"{hashlib.sha256(node_archive.read_bytes()).hexdigest()}  {node_archive.name}\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--platform", required=True)
    parser.add_argument("--sdk-platform", required=True)
    parser.add_argument("--binary", type=Path, action="append", default=[])
    parser.add_argument("--config", type=Path, action="append", default=[])
    args = parser.parse_args()
    package(args.output, args.platform, args.sdk_platform, args.binary, args.config)
