"""The third-party notices a package of karasu-slicer ships (TD-141).

Lists every crate compiled into the packaged executables (`karasu-app`, `karasu-cli`) for one
target platform: the normal dependencies reachable from them in Cargo's resolve, the project's
own crates left out. Each crate is named with its version, its declared licence expression and
its repository, and the licence and notice files its package carries are reproduced, the same
text once. Development and build-time dependencies are not compiled into the executables and are
not listed.

Reads `cargo metadata` offline (`--locked`), so the crates must be in Cargo's cache.

Usage: python3 tools/packaging/notices.py <target triple> <output file>
"""
import json
import subprocess
import sys
from pathlib import Path

ROOTS = {"karasu-app", "karasu-cli"}

# The MIT licence's standard text, for crates that declare it and carry no file.
MIT = """
Permission is hereby granted, free of charge, to any person obtaining a copy of this software
and associated documentation files (the "Software"), to deal in the Software without
restriction, including without limitation the rights to use, copy, modify, merge, publish,
distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the
Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or
substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING
BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
"""
NOTICE_FILES = ("LICENSE", "LICENCE", "COPYING", "NOTICE", "COPYRIGHT", "UNLICENSE")


def metadata(target: str) -> dict:
    command = [
        "cargo", "metadata", "--format-version", "1", "--locked", "--offline",
        "--filter-platform", target,
    ]
    root = Path(__file__).resolve().parents[2]
    return json.loads(subprocess.run(command, check=True, capture_output=True, cwd=root).stdout)


def shipped(meta: dict) -> list[dict]:
    """The packages compiled into the roots' executables, without the workspace's own."""
    packages = {p["id"]: p for p in meta["packages"]}
    workspace = set(meta["workspace_members"])
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    roots = [p["id"] for p in meta["packages"] if p["name"] in ROOTS and p["id"] in workspace]
    seen, stack = set(), list(roots)
    while stack:
        node = nodes[stack.pop()]
        for dep in node["deps"]:
            normal = any(k["kind"] is None for k in dep["dep_kinds"])
            if normal and dep["pkg"] not in seen:
                seen.add(dep["pkg"])
                stack.append(dep["pkg"])
    return sorted(
        (packages[i] for i in seen if i not in workspace),
        key=lambda p: (p["name"], p["version"]),
    )


def notice_files(package: dict) -> list[Path]:
    folder = Path(package["manifest_path"]).parent
    return sorted(
        f for f in folder.iterdir()
        if f.is_file() and f.name.upper().startswith(NOTICE_FILES)
    )


def write(target: str, output: Path) -> None:
    """Write the notices for `target` into `output`."""
    packages = shipped(metadata(target))
    lines = [
        "karasu-slicer: third-party notices",
        "",
        f"The executables for {target} include the following crates. Each is used under the",
        "licence its authors declare; the licence and notice texts their packages carry follow",
        "the list, each text once.",
        "",
    ]
    texts: dict[str, list[str]] = {}
    for package in packages:
        licence = package.get("license") or "see its licence files"
        source = package.get("repository") or package.get("homepage") or ""
        lines.append(f"- {package['name']} {package['version']}: {licence}" + (f" ({source})" if source else ""))
        for path in notice_files(package):
            text = path.read_text(encoding="utf-8", errors="replace").strip()
            texts.setdefault(text, []).append(f"{package['name']} {package['version']} ({path.name})")
        if not notice_files(package) and package.get("license_file"):
            path = Path(package["manifest_path"]).parent / package["license_file"]
            if path.is_file():
                text = path.read_text(encoding="utf-8", errors="replace").strip()
                texts.setdefault(text, []).append(f"{package['name']} {package['version']} ({path.name})")
    lines += ["", f"{len(packages)} crates.", ""]
    for text, owners in texts.items():
        lines += ["=" * 78, "Used by: " + ", ".join(owners), "-" * 78, text, ""]
    # Crates whose packages carry no licence file: their authors and declared licence, then the
    # standard texts of the licences they name.
    missing = [p for p in packages if not notice_files(p) and not p.get("license_file")]
    if missing:
        lines += [
            "=" * 78,
            "Crates whose packages carry no licence file, with their authors and the licence they",
            "declare; the standard texts of those licences follow.",
            "-" * 78,
        ]
        for package in missing:
            authors = ", ".join(package.get("authors") or []) or "their authors"
            lines.append(f"- {package['name']} {package['version']}: {package.get('license')}; {authors}")
        lines.append("")
        named = " ".join(p.get("license") or "" for p in missing)
        if "MIT" in named:
            lines += ["-" * 78, "The MIT License", "-" * 78, MIT.strip(), ""]
        if "Apache-2.0" in named:
            apache = next(
                (t for t, owners in texts.items() if "Apache License" in t and "Version 2.0" in t
                 and any("APACHE" in o.upper() for o in owners)),
                None,
            )
            if apache is None:
                raise SystemExit("no Apache-2.0 text found among the packages")
            lines += ["-" * 78, "The Apache License, Version 2.0", "-" * 78, apache, ""]
    # Always LF, also under Python for Windows: package.py turns the lines into CRLF itself.
    output.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    print(f"{output.name}: {len(packages)} crates, {len(texts)} distinct texts, {len(missing)} without files")


def main() -> None:
    write(sys.argv[1], Path(sys.argv[2]))


if __name__ == "__main__":
    main()
