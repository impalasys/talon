import base64
import hashlib
import tempfile
from pathlib import Path
import unittest
from unittest.mock import patch
import buildbuddy_api as api


class ArtifactTest(unittest.TestCase):
    def test_cas_digest_is_verified_and_corrupt_download_is_removed(self):
        data = b'build output'
        digest = hashlib.sha256(data).hexdigest()
        file = {'uri': f'bytestream://remote.buildbuddy.io/ci/blobs/{digest}/{len(data)}'}
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / 'output'
            with patch.object(api, 'request', return_value={'data': base64.b64encode(data).decode()}):
                api.download(file, output)
            self.assertEqual(output.read_bytes(), data)
            with patch.object(api, 'request', return_value={'data': base64.b64encode(b'x' * len(data)).decode()}):
                with self.assertRaisesRegex(RuntimeError, 'checksum'):
                    api.download(file, output)
            self.assertFalse(output.exists())

    def test_untrusted_uri_and_ambiguous_artifact_are_rejected(self):
        with self.assertRaisesRegex(ValueError, 'URI'):
            api.download({'uri': 'bytestream://example.com/blobs/' + 'a' * 64 + '/1'}, Path('unused'))
        with self.assertRaisesRegex(RuntimeError, 'one artifact'):
            api.artifact({'artifacts': [{'name': 'output'}, {'name': 'command-0/output'}]}, 'output')


if __name__ == '__main__':
    unittest.main()
