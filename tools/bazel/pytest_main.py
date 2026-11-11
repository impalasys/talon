"""Run pytest inside the declared Bazel runfiles tree."""

import os
from pathlib import Path
import sys

import pytest


if __name__ == "__main__":
    output = os.environ.get("TEST_UNDECLARED_OUTPUTS_DIR", os.environ.get("TEST_TMPDIR", "/tmp"))
    os.environ.setdefault("SIGHTLINE_SCREENSHOT_DIR", output)
    raise SystemExit(pytest.main(["-q", "-o", "cache_dir=" + str(Path(output) / "pytest-cache"), *sys.argv[1:]]))
