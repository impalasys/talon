import hashlib
import io
import tempfile
from pathlib import Path
import unittest
from unittest.mock import patch
import buildbuddy_api as api


class ArtifactTest(unittest.TestCase):
    def test_cas_digest_is_verified_and_corrupt_download_is_removed(self):
        data = b'build output' * 200000
        digest = hashlib.sha256(data).hexdigest()
        file = {'uri': f'bytestream://remote.buildbuddy.io/ci/blobs/{digest}/{len(data)}'}
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / 'output'
            class Response(io.BytesIO):
                def __init__(self, data):
                    super().__init__(data)
                    self.reads = []

                def read(self, size=-1):
                    self.reads.append(size)
                    return super().read(size)

            response = Response(data)
            with patch.object(api, 'open_request', return_value=response) as request:
                api.download(file, output)
            request.assert_called_once_with('GetFile', {'uri': file['uri']})
            self.assertGreater(len(response.reads), 2)
            self.assertTrue(all(0 < size <= 1024**2 for size in response.reads))
            self.assertEqual(output.read_bytes(), data)
            with patch.object(api, 'open_request', return_value=io.BytesIO(b'x' * len(data))):
                with self.assertRaisesRegex(RuntimeError, 'checksum'):
                    api.download(file, output)
            self.assertFalse(output.exists())

    def test_truncated_oversized_and_interrupted_streams_are_removed(self):
        data = b'output'
        digest = hashlib.sha256(data).hexdigest()
        file = {'uri': f'bytestream://remote.buildbuddy.io/ci/blobs/{digest}/{len(data)}'}
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / 'output'
            for actual, error in [(data[:-1], 'Truncated'), (data + b'extra', 'Oversized')]:
                with self.subTest(error=error), patch.object(api, 'open_request', return_value=io.BytesIO(actual)):
                    with self.assertRaisesRegex(RuntimeError, error):
                        api.download(file, output)
                    self.assertFalse(output.exists())
            class Interrupted(io.BytesIO):
                def read(self, size=-1):
                    raise OSError('connection interrupted')
            with patch.object(api, 'open_request', return_value=Interrupted()):
                with self.assertRaisesRegex(OSError, 'interrupted'):
                    api.download(file, output)
            self.assertFalse(output.exists())

    def test_untrusted_uri_and_ambiguous_artifact_are_rejected(self):
        with self.assertRaisesRegex(ValueError, 'URI'):
            api.download({'uri': 'bytestream://example.com/blobs/' + 'a' * 64 + '/1'}, Path('unused'))
        with self.assertRaisesRegex(RuntimeError, 'one artifact'):
            api.artifact({'artifacts': [{'name': 'output'}, {'name': 'command-0/output'}]}, 'output')


if __name__ == '__main__':
    unittest.main()
