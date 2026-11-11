"""Check or update generated files without removing hand-written SDK helpers."""
import argparse
import os
from pathlib import Path
import shutil

from python.runfiles import runfiles

GENERATED_TREES = [
    "sdk/go/talon-client/talon",
    "sdk/java/talon-client/src/main/java/talon",
    "sdk/java/talon-client/src/main/java/com/google/api",
    "sdk/js/talon-client/src/gen",
    "sdk/python/talon-client/src/talon_client/proto",
    "sdk/python/talon-client/src/talon_client/google",
    "sdk/rust/talon-client/src/generated",
]


def sync(source: Path, root: Path, update: bool) -> list[str]:
    changed = []
    for tree in GENERATED_TREES:
        destination = root / tree
        if destination.is_dir():
            for current in destination.rglob("*"):
                # Build metadata and placeholders are maintained by developers.
                if not current.is_file() or current.suffix not in [".go", ".java", ".ts", ".py", ".pyi", ".rs"]:
                    continue
                if not (source / current.relative_to(root)).is_file():
                    if update:
                        current.unlink()
                    else:
                        changed.append(str(current.relative_to(root)) + " (removed)")
    for generated in sorted(source.rglob("*")):
        if not generated.is_file():
            continue
        relative = generated.relative_to(source)
        target = root / relative
        if update:
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(generated, target)
        elif not target.is_file() or target.read_bytes() != generated.read_bytes():
            changed.append(str(relative))
    return changed


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--update', action='store_true')
    args = parser.parse_args()
    files = runfiles.Create()
    source = Path(files.Rlocation('_main/sdk/generated'))
    root = Path(os.environ['BUILD_WORKSPACE_DIRECTORY']) if args.update else Path(files.Rlocation('_main/sdk/VERSION')).parent.parent
    changed = sync(source, root, args.update)
    if changed:
        raise SystemExit('Generated SDK files are stale. Run scripts/sdk/generate.sh:\n' + '\n'.join(changed))


if __name__ == '__main__':
    main()
