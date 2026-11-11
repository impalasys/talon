"""Run Docker packaging after Bazel has built the complete image context."""
import argparse
from pathlib import Path
import subprocess

from python.runfiles import runfiles

parser = argparse.ArgumentParser()
parser.add_argument("--context", required=True)
parser.add_argument("--target", required=True)
parser.add_argument("--tag", action="append", default=[])
parser.add_argument("--push", action="store_true")
parser.add_argument("--archive", type=Path)
args, extra = parser.parse_known_args()
files = runfiles.Create()
context = Path(files.Rlocation("_main/" + args.context))
command = ["docker", "buildx", "build", "--file", str(context / "Dockerfile"), "--target", args.target]
for tag in args.tag:
    command += ["--tag", tag]
command += ["--push" if args.push else "--load", *extra, str(context)]
subprocess.run(command, check=True)
if args.archive:
    if not args.tag or args.push:
        parser.error("--archive requires a local tagged image")
    args.archive.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(["docker", "save", "--output", str(args.archive), args.tag[0]], check=True)
