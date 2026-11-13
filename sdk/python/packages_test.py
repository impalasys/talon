"""Check the distributions we publish, including rebuilding wheels from sdists."""

import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest
import zipfile


HATCHLING = str(Path(sys.argv.pop(1)).resolve())
DISTRIBUTIONS = [Path(path).resolve() for path in sys.argv[1:]]
del sys.argv[1:]


class PackagesTest(unittest.TestCase):
    def test_distributions(self):
        for distribution in DISTRIBUTIONS:
            with self.subTest(distribution=distribution), tempfile.TemporaryDirectory() as temporary:
                wheel, = distribution.glob("*.whl")
                sdist, = distribution.glob("*.tar.gz")
                with tarfile.open(sdist) as archive:
                    # Checkout/build metadata must never leak into published sources.
                    for member in archive.getmembers():
                        relative = Path(member.name).parts[1:]
                        self.assertTrue(relative)
                        self.assertIn(relative[0], {"src", "README.md", "pyproject.toml", "PKG-INFO", ".gitignore"})
                    archive.extractall(temporary, filter="data")
                project, = Path(temporary).iterdir()
                with zipfile.ZipFile(wheel) as original:
                    metadata, = [name for name in original.namelist() if name.endswith(".dist-info/METADATA")]
                    self.assertEqual(original.read(metadata), (project / "PKG-INFO").read_bytes())
                # A consumer installing the source distribution must get the same wheel.
                output = Path(temporary) / "rebuilt"
                subprocess.run(
                    [HATCHLING, "build", "-t", "wheel", "-d", str(output)],
                    cwd=project,
                    env={**os.environ, "SOURCE_DATE_EPOCH": "315532800"},
                    check=True,
                )
                self.assertEqual(wheel.read_bytes(), (output / wheel.name).read_bytes())


if __name__ == "__main__":
    unittest.main()
