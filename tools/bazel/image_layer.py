"""Write an OCI layer without Docker or executing any target-platform code."""
import io
import json
from pathlib import Path
import sys
import tarfile
import zipfile

manifest = json.loads(Path(sys.argv[1]).read_text())
entries = {}
for source, destination, mode in manifest['entries']:
    source = Path(source)
    if source.is_dir():
        for file in sorted(source.rglob('*')):
            if file.is_file():
                entries[destination + '/' + file.relative_to(source).as_posix()] = (file, int(mode, 8))
    else:
        entries[destination] = (source, int(mode, 8))
with tarfile.open(sys.argv[2], 'w', format=tarfile.PAX_FORMAT) as tar:
    for destination, (source, mode) in sorted(entries.items()):
        info = tarfile.TarInfo(destination)
        info.mode = mode
        info.size = source.stat().st_size
        with source.open('rb') as file:
            tar.addfile(info, file)
    if manifest['python']:
        with zipfile.ZipFile(manifest['monty']) as wheel:
            name = next(name for name in wheel.namelist() if name.endswith('/scripts/monty'))
            binary = wheel.read(name)
        info = tarfile.TarInfo('usr/local/bin/monty')
        info.mode = 0o755
        info.size = len(binary)
        tar.addfile(info, io.BytesIO(binary))
        for name, target in [('opt/talon-e2e-venv/bin/python', '/usr/local/bin/python3'), ('opt/talon-e2e-venv/bin/python3', '/usr/local/bin/python3'), ('opt/talon-monty-venv/bin/monty', '/usr/local/bin/monty')]:
            info = tarfile.TarInfo(name)
            info.type = tarfile.SYMTYPE
            info.linkname = target
            info.mode = 0o755
            tar.addfile(info)
        script = b'#!/usr/local/bin/python3\nimport pytest\nraise SystemExit(pytest.console_main())\n'
        info = tarfile.TarInfo('opt/talon-e2e-venv/bin/pytest')
        info.mode = 0o755
        info.size = len(script)
        tar.addfile(info, io.BytesIO(script))
