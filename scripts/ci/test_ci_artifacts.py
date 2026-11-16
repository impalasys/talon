import hashlib
import io
import json
import tarfile
import tempfile
from pathlib import Path
import unittest
from unittest.mock import patch
import fetch_ci_artifacts as ci


class HandoffTest(unittest.TestCase):
    def test_stale_revision_or_affected_main_is_rejected(self):
        manifest = {'schema': 2, 'repository': 'impalasys/talon', 'head': 'h', 'tree': 't',
                    'base': 'b', 'success': True, 'files': {}}
        ci.validate_manifest(manifest, 'h', 'b', 't')
        with self.assertRaisesRegex(RuntimeError, 'revision/tree'):
            ci.validate_manifest(manifest, 'new-head', 'b', 't')
        manifest['base'] = None
        manifest['mode'] = 'affected'
        with self.assertRaisesRegex(RuntimeError, 'full monorepo'):
            ci.validate_manifest(manifest, 'h', None, 't')

    def test_pending_retry_does_not_accept_older_success(self):
        status = {'context': ci.CONTEXT, 'state': 'pending',
                  'target_url': 'https://impalasys.buildbuddy.io/invocation/' + 'a' * 36,
                  'creator': {'login': 'buildbuddy-io[bot]'}}
        rows = [[status, {**status, 'state': 'success'}]]
        with patch.object(ci.subprocess, 'check_output', return_value=json.dumps(rows)), \
                patch.object(ci.time, 'monotonic', return_value=1):
            with self.assertRaisesRegex(RuntimeError, 'Timed out'):
                ci.wait_status('impalasys/talon', 'a' * 40, 0)

    def test_spoofed_publisher_is_rejected(self):
        status = {'context': ci.CONTEXT, 'state': 'success',
                  'target_url': 'https://impalasys.buildbuddy.io/invocation/' + 'a' * 36,
                  'creator': {'login': 'another-user'}}
        with patch.object(ci.subprocess, 'check_output', return_value=json.dumps([[status]])):
            with self.assertRaisesRegex(RuntimeError, 'publisher'):
                ci.wait_status('impalasys/talon', 'a' * 40, 0)

    def test_changed_pr_is_rejected_before_artifact_access(self):
        event = {'number': 1, 'pull_request': {'head': {'sha': 'a' * 40}, 'base': {'sha': 'b' * 40}}}
        current = {'head': {'sha': 'c' * 40}, 'base': {'sha': 'b' * 40}}
        with patch.object(ci, 'github', return_value=current):
            with self.assertRaisesRegex(RuntimeError, 'revision changed'):
                ci.revision(event, 'impalasys/talon')

    def test_relay_uses_the_current_base_then_rechecks_it_after_transfer(self):
        event = {'number': 1, 'pull_request': {'head': {'sha': 'a' * 40}, 'base': {'sha': 'b' * 40}}}
        current = {'number': 1, 'head': {'sha': 'a' * 40, 'ref': 'feature', 'repo': {'full_name': 'someone/talon'}},
                   'base': {'sha': 'c' * 40}, 'merge_commit_sha': 'd' * 40}
        with patch.object(ci, 'github', side_effect=[current, {'tree': {'sha': 'e' * 40}}]):
            revision = ci.revision(event, 'impalasys/talon')
        self.assertEqual(revision[:3], ('a' * 40, 'c' * 40, 'e' * 40))

    def test_links_and_path_traversal_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            archive = Path(temporary) / 'bundle'
            with tarfile.open(archive, 'w') as stream:
                link = tarfile.TarInfo('images/link')
                link.type = tarfile.SYMTYPE
                link.linkname = '/tmp/elsewhere'
                stream.addfile(link)
            with self.assertRaisesRegex(RuntimeError, 'linked'):
                ci.unpack(archive, Path(temporary) / 'out', {'images/link': {'size': 0}})
        manifest = {'schema': 2, 'repository': 'impalasys/talon', 'head': 'h', 'tree': 't',
                    'base': 'b', 'success': True, 'files': {'images/../../escape': {'size': 0, 'sha256': 'a' * 64}}}
        with self.assertRaisesRegex(RuntimeError, 'Unsafe'):
            ci.validate_manifest(manifest, 'h', 'b', 't')

    def test_complete_bundle_is_checked_against_manifest(self):
        data = b'archive bytes'
        inventory = {'images/runtime.tar': {'size': len(data), 'sha256': hashlib.sha256(data).hexdigest()}}
        with tempfile.TemporaryDirectory() as temporary:
            archive = Path(temporary) / 'bundle'
            with tarfile.open(archive, 'w') as stream:
                file = tarfile.TarInfo('images/runtime.tar')
                file.size = len(data)
                stream.addfile(file, io.BytesIO(data))
            ci.unpack(archive, Path(temporary) / 'out', inventory)
            self.assertEqual((Path(temporary) / 'out/images/runtime.tar').read_bytes(), data)
            inventory['images/runtime.tar']['sha256'] = '0' * 64
            with self.assertRaisesRegex(RuntimeError, 'checksum'):
                ci.unpack(archive, Path(temporary) / 'out', inventory)


if __name__ == '__main__':
    unittest.main()
