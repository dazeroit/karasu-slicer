#!/usr/bin/env python3
"""Prepare a hash-verified source snapshot for native Windows validation."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import zipfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path, help="New evidence input directory outside the repository")
    parser.add_argument("--include-untracked", action="store_true", help="Include nonignored untracked files after reviewing git status")
    args = parser.parse_args()
    root = Path(subprocess.check_output(["git", "rev-parse", "--show-toplevel"], text=True).strip())
    destination = args.destination.resolve()
    if destination == root or root in destination.parents:
        parser.error("snapshot destination must be outside the repository")
    command = ["git", "ls-files", "--cached", "-z"]
    if args.include_untracked:
        command += ["--others", "--exclude-standard"]
    paths = sorted(set(subprocess.check_output(command, cwd=root).decode("utf-8").split("\0")) - {""})
    destination.mkdir(parents=True, exist_ok=False)
    manifest = []
    with zipfile.ZipFile(destination / "source.zip", "w", zipfile.ZIP_DEFLATED) as archive:
        for name in paths:
            source = root / name
            if source.is_symlink():
                raise ValueError(f"snapshot does not support symbolic links: {name}")
            data = source.read_bytes()
            archive.writestr(name, data)
            manifest.append({"path": name, "sha256": hashlib.sha256(data).hexdigest()})
    (destination / "manifest.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    metadata = {
        "commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip(),
        "status": subprocess.check_output(["git", "status", "--short"], cwd=root, text=True),
        "include_untracked": args.include_untracked,
        "files": len(manifest),
        "archive_sha256": hashlib.sha256((destination / "source.zip").read_bytes()).hexdigest(),
    }
    (destination / "snapshot.json").write_text(json.dumps(metadata, indent=2), encoding="utf-8")
    print(json.dumps(metadata, indent=2))


if __name__ == "__main__":
    main()
