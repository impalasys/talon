"""Materialize Bazel sandbox inputs, then use the upstream Hatchling backend."""
import os
from pathlib import Path
import shutil
import sys
import tempfile

from hatchling.build import build_sdist, build_wheel

manifest, output = (Path(path).absolute() for path in sys.argv[1:3])
os.environ["SOURCE_DATE_EPOCH"] = "315532800"
with tempfile.TemporaryDirectory() as directory:
    project = Path(directory) / "project"
    shutil.copytree(manifest.parent, project)
    os.chdir(project)
    build_wheel(str(output))
    build_sdist(str(output))
