"""SDK regeneration preserves helpers and removes obsolete generated sources."""
from pathlib import Path
import tempfile
import unittest

from sync_generated import sync


class GeneratedSDKTest(unittest.TestCase):
    def test_update_preserves_helpers_and_removes_obsolete_generated_files(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp) / 'checkout'
            source = Path(temp) / 'generated'
            helper = root / 'sdk/go/talon-client/client.go'
            metadata = root / 'sdk/go/talon-client/talon/v1/BUILD.bazel'
            stale = root / 'sdk/go/talon-client/talon/v1/removed.pb.go'
            fresh = source / 'sdk/go/talon-client/talon/v1/auth.pb.go'
            for path, content in [(metadata, 'build metadata'), (helper, 'hand written'), (stale, 'old'), (fresh, 'generated')]:
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(content)
            self.assertTrue(sync(source, root, False))
            sync(source, root, True)
            self.assertEqual(helper.read_text(), 'hand written')
            self.assertFalse(stale.exists())
            self.assertEqual(metadata.read_text(), 'build metadata')
            self.assertEqual((root / fresh.relative_to(source)).read_text(), 'generated')
            self.assertEqual(sync(source, root, False), [])


if __name__ == '__main__':
    unittest.main()
