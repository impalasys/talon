import hashlib
from pathlib import Path
import tarfile
import tempfile
import unittest

from release_packager import package


class ReleaseTest(unittest.TestCase):
    def test_distribution_is_reproducible_and_contains_model_catalog(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binaries = []
            for name in ["talon-server", "talon-worker", "talon-cli", "talon-node"]:
                path = root / name
                path.write_bytes((name + " binary").encode())
                binaries.append(path)
            (root / "models").mkdir()
            model = root / "models" / "provider.yaml"
            model.write_text("models: {}\n")
            for destination in [root / "first", root / "second"]:
                package(destination, "linux-x86_64", "linux-x64", binaries, [model])
            first = root / "first" / "talon-linux-x86_64.tar.gz"
            self.assertEqual(first.read_bytes(), (root / "second" / first.name).read_bytes())
            with tarfile.open(first) as tar:
                self.assertIn("talon-linux-x86_64/models/provider.yaml", tar.getnames())
                self.assertEqual(tar.getmember("talon-linux-x86_64/talon-node").mode, 0o755)
                sums = tar.extractfile("talon-linux-x86_64/SHA256SUMS").read().decode()
                self.assertIn(hashlib.sha256(model.read_bytes()).hexdigest(), sums)


if __name__ == "__main__":
    unittest.main()
