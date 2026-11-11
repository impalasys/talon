"""Flatten pinned Debian data archives into a deterministic OCI layer."""
import sys
import tarfile
import io

entries = {}
for archive in sys.argv[2:]:
    with tarfile.open(archive, 'r:*') as package:
        for entry in package:
            entry.name = entry.name.removeprefix('./')
            if not entry.name or entry.name == '.':
                continue
            entry.uid = entry.gid = entry.mtime = 0
            entry.uname = entry.gname = ''
            entry.pax_headers = {}
            content = package.extractfile(entry).read() if entry.isfile() else None
            entries[entry.name] = (entry, content)
with tarfile.open(sys.argv[1], 'w', format=tarfile.PAX_FORMAT) as output:
    for entry, content in entries.values():
        output.addfile(entry, io.BytesIO(content) if content is not None else None)
