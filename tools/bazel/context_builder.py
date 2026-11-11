"""Stage a Docker context from declared, precompiled Bazel artifacts."""
import argparse
from pathlib import Path
import shutil

parser = argparse.ArgumentParser()
parser.add_argument("--output", type=Path, required=True)
parser.add_argument("--dockerfile", type=Path, required=True)
parser.add_argument("--binary", type=Path, action="append", default=[])
parser.add_argument("--config", type=Path, action="append", default=[])
parser.add_argument("--web", type=Path, action="append", default=[])
parser.add_argument("--requirements", type=Path)
args = parser.parse_args()
args.output.mkdir(parents=True, exist_ok=True)
shutil.copyfile(args.dockerfile, args.output / "Dockerfile")
for binary in args.binary:
    (args.output / "bin").mkdir(exist_ok=True)
    destination = args.output / "bin" / binary.name
    shutil.copyfile(binary, destination)
    destination.chmod(0o755)
for config in args.config:
    relative = Path("models") / config.name if config.parent.name == "models" else Path(config.name)
    destination = args.output / relative
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(config, destination)
if args.requirements:
    destination = args.output / "tools/bazel/requirements.lock"
    destination.parent.mkdir(parents=True)
    shutil.copyfile(args.requirements, destination)
for web in args.web:
    shutil.copytree(web, args.output / "dist", dirs_exist_ok=True)
