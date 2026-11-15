"""SHA256 manifests for Talon's release layout and downloadable node archive."""
import hashlib
from pathlib import Path
import sys
import tarfile

mode, source, output = sys.argv[1:]
if mode == "contents":
    with tarfile.open(source) as archive:
        sums = [(entry.name.removeprefix("./"), hashlib.sha256(archive.extractfile(entry).read()).hexdigest())
                for entry in archive if entry.isfile()]
else:
    sums = [(Path(source).name, hashlib.sha256(Path(source).read_bytes()).hexdigest())]
Path(output).write_text("".join(f"{digest}  {name}\n" for name, digest in sorted(sums)))
