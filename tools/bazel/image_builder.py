"""Run Docker packaging after Bazel has built the complete image context."""
import argparse
from pathlib import Path
import subprocess

from python.runfiles import runfiles

parser = argparse.ArgumentParser()
parser.add_argument("--dockerfile", required=True)
parser.add_argument("--target", required=True)
parser.add_argument("--tag", action="append", default=[])
parser.add_argument("--push", action="store_true")
args, extra = parser.parse_known_args()
files = runfiles.Create()
dockerfile = Path(files.Rlocation("_main/" + args.dockerfile))
context = dockerfile.parent
command = ["docker", "buildx", "build", "--file", str(dockerfile), "--target", args.target]
for tag in args.tag:
    command += ["--tag", tag]
command += ["--push" if args.push else "--load", *extra, str(context)]
subprocess.run(command, check=True)
