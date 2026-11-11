"""Check OCI layers preserve files and contain no duplicate paths."""
import hashlib
import io
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest


class DebianLayerTest(unittest.TestCase):
    def test_merge_normalizes_duplicate_directories_and_replaced_files(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            for number in [1, 2]:
                with tarfile.open(root / f'{number}.tar.xz', 'w:xz') as package:
                    directory = tarfile.TarInfo('./usr/bin')
                    directory.type = tarfile.DIRTYPE
                    package.addfile(directory)
                    item = tarfile.TarInfo('./usr/bin/tool')
                    item.mode = 0o755
                    item.size = 1
                    package.addfile(item, io.BytesIO(str(number).encode()))
            hashes = []
            for output in ['first.tar', 'second.tar']:
                subprocess.run([sys.executable, str(Path(__file__).with_name('merge_layers.py')), str(root/output), str(root/'1.tar.xz'), str(root/'2.tar.xz')], check=True)
                hashes.append(hashlib.sha256((root/output).read_bytes()).digest())
            self.assertEqual(*hashes)
            with tarfile.open(root/'first.tar') as layer:
                self.assertEqual(layer.getnames(), ['usr/bin', 'usr/bin/tool'])
                self.assertEqual(layer.extractfile('usr/bin/tool').read(), b'2')
                self.assertEqual(layer.getmember('usr/bin/tool').mode, 0o755)


if __name__ == '__main__':
    unittest.main()
