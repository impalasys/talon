"""Stage the ACP tooling recipe for its separate Docker build."""
import argparse
from pathlib import Path
import shutil

parser = argparse.ArgumentParser()
parser.add_argument("--output", type=Path, required=True)
parser.add_argument("--dockerfile", type=Path, required=True)
args = parser.parse_args()
args.output.mkdir(parents=True, exist_ok=True)
shutil.copyfile(args.dockerfile, args.output / "Dockerfile")
