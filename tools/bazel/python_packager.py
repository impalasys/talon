"""Build wheel and sdist in an isolated source copy with Bazel's Python deps."""
import os
from pathlib import Path
import shutil
import sys
import tempfile

from build import ProjectBuilder

manifest, output = map(lambda path: Path(path).absolute(), sys.argv[1:3])
source = manifest.parent
output.mkdir(parents=True, exist_ok=True)
os.environ["SOURCE_DATE_EPOCH"] = "315532800"
with tempfile.TemporaryDirectory() as directory:
    project = Path(directory) / "project"
    shutil.copytree(source, project)
    os.environ["PYTHONPATH"] = os.pathsep.join(sys.path)
    builder = ProjectBuilder(project)
    builder.build("wheel", output)
    builder.build("sdist", output)
