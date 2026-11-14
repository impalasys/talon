"""Validate the release contract against the archives produced by rules_pkg."""
import hashlib
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest

import yaml

RUNTIME, NODE = sys.argv[1:]
sys.argv[1:] = []


class ReleaseArchivesTest(unittest.TestCase):
    def test_runtime_contents_checksums_and_executable(self):
        with tarfile.open(RUNTIME) as archive:
            files = {member.name.removeprefix("./"): member for member in archive if member.isfile()}
            root = next(iter(files)).split("/")[0]
            self.assertTrue(root.startswith("talon-"))
            for name in ["talon-server", "talon-worker", "talon-cli", "talon-node"]:
                self.assertEqual(files[f"{root}/{name}"].mode, 0o755)
            sums = archive.extractfile(files[f"{root}/SHA256SUMS"]).read().decode().splitlines()
            self.assertEqual(len(sums), len(files) - 1)
            for line in sums:
                digest, name = line.split("  ", 1)
                self.assertEqual(digest, hashlib.sha256(archive.extractfile(files[f"{root}/{name}"]).read()).hexdigest())
            configs = {name: yaml.safe_load(archive.extractfile(member)) for name, member in files.items() if name.endswith(".yaml")}
            self.assertIn(f"{root}/models.yaml", configs)
            for name, config in configs.items():
                extends = config.get("extends", [])
                for relative in [extends] if isinstance(extends, str) else extends:
                    self.assertIn(str(Path(name).parent / relative), configs)
            with tempfile.TemporaryDirectory() as directory:
                archive.extractall(directory, filter="data")
                subprocess.run([str(Path(directory) / root / "talon-cli"), "--help"], cwd=Path(directory) / root, check=True, stdout=subprocess.DEVNULL)

    def test_node_archive_layout(self):
        with tarfile.open(NODE) as archive:
            files = [member for member in archive if member.isfile()]
            self.assertEqual(len(files), 1)
            self.assertTrue(files[0].name.removeprefix("./").startswith("talon-node-"))
            self.assertTrue(files[0].name.endswith("/talon-node"))
            self.assertEqual(files[0].mode, 0o755)


if __name__ == "__main__":
    unittest.main()
