"""Apply the SDK's published whitespace, package-init and Python import policy."""
from pathlib import Path
import sys

language, source, destination = sys.argv[1:]
source, root = Path(source), Path(destination)
root.mkdir(parents=True, exist_ok=True)
for path in source.rglob("*"):
    if not path.is_file():
        continue
    text = path.read_text()
    if language == "python":
        for package in ["", ".data", ".external", ".harness", ".resources", ".talon", ".talon.v1"]:
            text = text.replace(f"from proto{package} import ", f"from talon_client.proto{package} import ")
        text = text.replace("import proto.", "import talon_client.proto.")
    else:
        text = "\n".join(line.rstrip() for line in text.splitlines())
        text = (text if language == "java" else text.rstrip()) + "\n"
    output = root / path.relative_to(source)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(text)
if language == "python":
    for directory in (path for path in root.rglob("*") if path.is_dir()):
        (directory / "__init__.py").touch()
