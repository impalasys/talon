"""Exercise the real upstream SDK updater in an isolated checkout."""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

from python.runfiles import runfiles


class SDKUpdaterTest(unittest.TestCase):
    def test_update_preserves_metadata_removes_stale_files_and_is_idempotent(self):
        files = runfiles.Create()
        updater = Path(files.Rlocation("_main/" + sys.argv[1]))
        expected_tree = Path(files.Rlocation("_main/sdk/generated_with_metadata"))
        checked_in_tree = Path(files.Rlocation("_main/sdk/checked_in_generated"))
        expected = self.contents(expected_tree)
        destinations = sys.argv[2:]
        with tempfile.TemporaryDirectory() as temp:
            checkout = Path(temp) / "checkout"
            shutil.copytree(checked_in_tree, checkout, copy_function=shutil.copyfile)
            helpers = {
                "sdk/go/talon-client/client.go": b"handwritten Go helper\n",
                "sdk/java/talon-client/src/main/java/systems/impala/talon/client/Client.java": b"handwritten Java helper\n",
                "sdk/js/talon-client/src/index.ts": b"handwritten JS helper\n",
                "sdk/python/talon-client/src/talon_client/client.py": b"handwritten Python helper\n",
                "sdk/rust/talon-client/src/lib.rs": b"handwritten Rust helper\n",
            }
            for path, content in helpers.items():
                self.write(checkout / path, content)
            # Damage every owned destination, including additions and removals.
            for destination in destinations:
                path = checkout / destination
                if path.is_dir():
                    self.write(path / "obsolete" / "stale.generated", b"stale\n")
                    member = next(p for p in path.rglob("*") if p.is_file() and p.suffix in [".go", ".java", ".ts", ".py", ".rs"])
                    member.unlink()
                else:
                    path.write_bytes(b"outdated\n")
            env = dict(os.environ, BUILD_WORKSPACE_DIRECTORY=str(checkout))
            # Upstream scripts resolve their inputs and child updaters from the
            # runfiles workspace, exactly as they do under bazel run.
            workspace = updater.parent.parent
            for _ in range(2):
                subprocess.run([str(updater)], cwd=workspace, env=env, check=True)
                self.assertEqual(self.contents(checkout), expected | helpers)
                self.assertFalse(any(p.is_symlink() for p in checkout.rglob("*")))

    @staticmethod
    def contents(root):
        return {str(p.relative_to(root)): p.read_bytes() for p in root.rglob("*") if p.is_file()}

    @staticmethod
    def write(path, content):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)


if __name__ == "__main__":
    unittest.main(argv=[sys.argv[0]])
