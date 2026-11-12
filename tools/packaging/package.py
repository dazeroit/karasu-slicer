"""Assemble karasu-slicer's packages from built executables (TD-141, packaging prototype).

Windows: a portable ZIP, and the staged folder `tools/packaging/karasu.iss` (Inno Setup) builds
the per-user installer from. Linux: a tar.gz with `install.sh` (into `~/.local` by default), and a
`.deb` built with `dpkg-deb`.

Every package carries the application (`karasu`), the command line (`karasu-cli`), a README and
the third-party notices for its platform (`notices.py`). The packages are reproducible for the
same inputs: entries are sorted and dated at `SOURCE_DATE_EPOCH` (default: the last commit's
time).

Usage:
  python3 tools/packaging/package.py windows <folder with karasu.exe and karasu-cli.exe> [<output>]
  python3 tools/packaging/package.py linux <folder with karasu and karasu-cli> [<output>]

The output folder defaults to `target/packages`.
"""
import gzip
import io
import os
import shutil
import subprocess
import sys
import tarfile
import tomllib
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(Path(__file__).resolve().parent))
import notices  # noqa: E402  (beside this script)

NAME = "karasu-slicer"
ICONS = ROOT / "assets" / "ui" / "icon"
# The application's Wayland app id and X11 class (eframe uses the application name).
APP_ID = NAME
PROJECT_MIME = "application/x-karasu-project"
# The model formats the application opens, as shared-mime-info names them.
MODEL_MIME = ("model/stl", "model/3mf", "model/obj")
ICON_SIZES = (16, 24, 32, 48, 64, 128, 256, 512)

README = """karasu-slicer {version}
{rule}

A native MSLA/LCD resin slicer. This is a packaging prototype (M6.7).

- karasu{exe}: the desktop application. Open a project (.karasu) or a model (STL, 3MF, OBJ) by
  passing it on the command line, dropping it on the window or from the File menu.
- karasu-cli{exe}: the command line (karasu-cli --help).

Settings, profiles and the recovery copy live in the user's application data folder, not here.

Copyright (C) 2026 Ivan Maruca. karasu-slicer is free software under the GNU Affero General
Public License, version 3 or any later version (LICENSE.txt), without any warranty. Its source
is at https://github.com/dazeroit/karasu-slicer. The name karasu-slicer and its logo are
trademarks, not licensed with the code (TRADEMARKS.txt).

THIRD-PARTY-NOTICES.txt lists the open-source crates compiled into the executables, with their
licences.
"""

DESKTOP = """[Desktop Entry]
Type=Application
Name={name}
GenericName=Resin slicer
Comment=MSLA/LCD resin slicer
Exec={exec} %F
Icon={name}
Terminal=false
Categories=Graphics;3DGraphics;Engineering;
Keywords=3D;printing;resin;MSLA;LCD;slicer;
MimeType={mime};
StartupWMClass={app_id}
"""

MIME_XML = """<?xml version="1.0" encoding="UTF-8"?>
<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="{mime}">
    <comment>karasu-slicer project</comment>
    <sub-class-of type="application/zip"/>
    <glob pattern="*.karasu"/>
    <icon name="{name}"/>
  </mime-type>
</mime-info>
"""

INSTALL_SH = r"""#!/bin/sh
# Install karasu-slicer for the current user (default prefix ~/.local), or remove it with
# --uninstall. Usage: ./install.sh [--prefix <folder>] [--uninstall]
set -eu
prefix="$HOME/.local"
action=install
while [ $# -gt 0 ]; do
    case "$1" in
        --prefix) prefix="$2"; shift 2 ;;
        --uninstall) action=uninstall; shift ;;
        *) echo "usage: $0 [--prefix <folder>] [--uninstall]" >&2; exit 2 ;;
    esac
done
[ -n "$prefix" ] || { echo "the prefix is empty" >&2; exit 2; }
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
manifest="$prefix/share/karasu-slicer/installed-files"

refresh() {
    command -v update-mime-database >/dev/null 2>&1 && update-mime-database "$prefix/share/mime" || true
    command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$prefix/share/applications" || true
    command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t "$prefix/share/icons/hicolor" || true
}

if [ "$action" = uninstall ]; then
    [ -f "$manifest" ] || { echo "karasu-slicer is not installed under $prefix" >&2; exit 1; }
    while IFS= read -r file; do
        [ -n "$file" ] && rm -f -- "$prefix/$file"
    done < "$manifest"
    rm -f -- "$manifest"
    rmdir -- "$prefix/share/karasu-slicer" "$prefix/share/doc/karasu-slicer" 2>/dev/null || true
    refresh
    echo "karasu-slicer removed from $prefix"
    exit 0
fi

mkdir -p "$prefix/share/karasu-slicer"
: > "$manifest"
(cd "$here" && find bin share -type f | sort) | while IFS= read -r file; do
    mkdir -p "$prefix/$(dirname -- "$file")"
    cp -- "$here/$file" "$prefix/$file"
    echo "$file" >> "$manifest"
done
chmod 755 "$prefix/bin/karasu" "$prefix/bin/karasu-cli"
# The menu entry starts the installed executable by its full path: the prefix's bin folder may
# not be on PATH.
desktop="$prefix/share/applications/karasu-slicer.desktop"
sed "s|^Exec=karasu |Exec=\"$prefix/bin/karasu\" |" "$desktop" > "$desktop.new"
mv -- "$desktop.new" "$desktop"
refresh
echo "karasu-slicer installed under $prefix; remove it with: $0 --prefix \"$prefix\" --uninstall"
"""


def version() -> str:
    with open(ROOT / "Cargo.toml", "rb") as file:
        return tomllib.load(file)["workspace"]["package"]["version"]


def epoch() -> int:
    if "SOURCE_DATE_EPOCH" in os.environ:
        return int(os.environ["SOURCE_DATE_EPOCH"])
    out = subprocess.run(
        ["git", "log", "-1", "--format=%ct"], cwd=ROOT, check=True, capture_output=True, text=True
    )
    return int(out.stdout.strip())


def readme(exe: str) -> bytes:
    title = f"karasu-slicer {version()}"
    return README.format(version=version(), rule="=" * len(title), exe=exe).encode()


def third_party(target: str) -> bytes:
    """The notices for `target`, through notices.py into a temporary file."""
    path = Path(os.environ.get("TMPDIR", "/tmp")) / f"karasu-notices-{target}.txt"
    notices.write(target, path)
    data = path.read_bytes()
    path.unlink()
    return data


def executable(folder: Path, name: str) -> bytes:
    path = folder / name
    if not path.is_file():
        raise SystemExit(f"{path} is missing: build the release executables first")
    return path.read_bytes()


def windows(binaries: Path, output: Path) -> None:
    stem = f"{NAME}-{version()}-windows-x64"
    files = {
        "karasu.exe": executable(binaries, "karasu.exe"),
        "karasu-cli.exe": executable(binaries, "karasu-cli.exe"),
        "README.txt": readme(".exe").replace(b"\n", b"\r\n"),
        "THIRD-PARTY-NOTICES.txt": third_party("x86_64-pc-windows-msvc").replace(b"\n", b"\r\n"),
        "LICENSE.txt": (ROOT / "LICENSE").read_bytes().replace(b"\n", b"\r\n"),
        "TRADEMARKS.txt": (ROOT / "TRADEMARKS.md").read_bytes().replace(b"\n", b"\r\n"),
    }
    # The staged folder Inno Setup reads.
    stage = output / stem
    if stage.exists():
        shutil.rmtree(stage)
    stage.mkdir(parents=True)
    for name, data in files.items():
        (stage / name).write_bytes(data)
    stamp = zip_time(epoch())
    archive = output / f"{stem}.zip"
    with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as zip_file:
        for name in sorted(files):
            info = zipfile.ZipInfo(f"{stem}/{name}", stamp)
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o644 << 16
            zip_file.writestr(info, files[name])
    print(f"{archive} ({archive.stat().st_size} bytes); installer input {stage}")


def zip_time(seconds: int) -> tuple:
    import time
    t = time.gmtime(max(seconds, 315532800))  # ZIP dates start in 1980.
    return (t.tm_year, t.tm_mon, t.tm_mday, t.tm_hour, t.tm_min, t.tm_sec)


def linux_tree(binaries: Path, exec_name: str, doc: str) -> dict[str, tuple[bytes, int]]:
    """The installed files under a prefix, with their modes."""
    mime = ";".join((PROJECT_MIME,) + MODEL_MIME)
    tree = {
        "bin/karasu": (executable(binaries, "karasu"), 0o755),
        "bin/karasu-cli": (executable(binaries, "karasu-cli"), 0o755),
        f"share/applications/{NAME}.desktop": (
            DESKTOP.format(name=NAME, exec=exec_name, mime=mime, app_id=APP_ID).encode(), 0o644),
        f"share/mime/packages/{NAME}.xml": (
            MIME_XML.format(mime=PROJECT_MIME, name=NAME).encode(), 0o644),
        f"{doc}/README.txt": (readme(""), 0o644),
        f"{doc}/THIRD-PARTY-NOTICES.txt": (third_party("x86_64-unknown-linux-gnu"), 0o644),
        f"{doc}/LICENSE.txt": ((ROOT / "LICENSE").read_bytes(), 0o644),
        f"{doc}/TRADEMARKS.txt": ((ROOT / "TRADEMARKS.md").read_bytes(), 0o644),
    }
    for size in ICON_SIZES:
        tree[f"share/icons/hicolor/{size}x{size}/apps/{NAME}.png"] = (
            (ICONS / f"karasu-{size}.png").read_bytes(), 0o644)
    return tree


def add(tar: tarfile.TarFile, name: str, data: bytes, mode: int, mtime: int) -> None:
    info = tarfile.TarInfo(name)
    info.size, info.mode, info.mtime = len(data), mode, mtime
    info.uid = info.gid = 0
    info.uname = info.gname = "root"
    tar.addfile(info, io.BytesIO(data))


def add_dirs(tar: tarfile.TarFile, names: list[str], mtime: int, prefix: str = "") -> None:
    """The folders above `names`, each once, parents first."""
    folders = sorted({str(Path(n).parents[i]) for n in names for i in range(len(Path(n).parts) - 1)})
    for folder in folders:
        info = tarfile.TarInfo(prefix + folder)
        info.type, info.mode, info.mtime = tarfile.DIRTYPE, 0o755, mtime
        info.uname = info.gname = "root"
        tar.addfile(info)


def tar_gz(path: Path, entries: dict[str, tuple[bytes, int]], mtime: int, root: str) -> None:
    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode="w", format=tarfile.GNU_FORMAT) as tar:
        add_dirs(tar, [f"{root}/{n}" for n in entries], mtime)
        for name in sorted(entries):
            data, mode = entries[name]
            add(tar, f"{root}/{name}", data, mode, mtime)
    with open(path, "wb") as file, gzip.GzipFile(fileobj=file, mode="wb", mtime=mtime) as gz:
        gz.write(raw.getvalue())


def glibc(binary: bytes) -> str:
    """The newest GLIBC symbol version the executable needs, read from its bytes."""
    found = set()
    for part in binary.split(b"GLIBC_")[1:]:
        digits = part.split(b"\0", 1)[0]
        if digits and all(c in b"0123456789." for c in digits):
            found.add(tuple(int(x) for x in digits.decode().split(".") if x))
    return ".".join(map(str, max(found)))


def linux(binaries: Path, output: Path) -> None:
    mtime = epoch()
    stem = f"{NAME}-{version()}-linux-x86_64"
    # The tar.gz: the prefix tree beside install.sh.
    tree = linux_tree(binaries, "karasu", f"share/doc/{NAME}")
    tree["install.sh"] = (INSTALL_SH.encode(), 0o755)
    archive = output / f"{stem}.tar.gz"
    tar_gz(archive, tree, mtime, stem)
    print(f"{archive} ({archive.stat().st_size} bytes)")
    deb(binaries, output, mtime)


def deb(binaries: Path, output: Path, mtime: int) -> None:
    """A .deb of the tree under /usr, built by dpkg-deb from a staged folder."""
    tree = linux_tree(binaries, "karasu", f"share/doc/{NAME}")
    stage = output / f"{NAME}_{version()}_amd64"
    if stage.exists():
        shutil.rmtree(stage)
    for name, (data, mode) in tree.items():
        path = stage / "usr" / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
        path.chmod(mode)
    size_kib = sum((len(d) + 1023) // 1024 for d, _ in tree.values())
    newest = max(glibc(tree["bin/karasu"][0]), glibc(tree["bin/karasu-cli"][0]),
                 key=lambda v: tuple(map(int, v.split("."))))
    control = f"""Package: {NAME}
Version: {version()}
Architecture: amd64
Maintainer: karasu-slicer project <karasu-slicer@invalid>
Installed-Size: {size_kib}
Depends: libc6 (>= {newest}), libgcc-s1, libxkbcommon0, libvulkan1 | libegl1
Recommends: libwayland-client0, libxkbcommon-x11-0, libx11-6, libxcursor1, libxi6, libxrandr2, xdg-desktop-portal
Section: graphics
Priority: optional
Description: native MSLA/LCD resin slicer
 karasu-slicer prepares models for MSLA/LCD resin printers: supports, hollowing,
 drain holes and slicing into printer files. This is a packaging prototype.
"""
    (stage / "DEBIAN").mkdir()
    (stage / "DEBIAN" / "control").write_text(control)
    for path in stage.rglob("*"):
        os.utime(path, (mtime, mtime))
    package = output / f"{NAME}_{version()}_amd64.deb"
    env = dict(os.environ, SOURCE_DATE_EPOCH=str(mtime))
    subprocess.run(
        ["dpkg-deb", "--root-owner-group", "-Zxz", "--build", str(stage), str(package)],
        check=True, env=env, stdout=subprocess.DEVNULL,
    )
    shutil.rmtree(stage)
    print(f"{package} ({package.stat().st_size} bytes)")


def main() -> None:
    if len(sys.argv) not in (3, 4) or sys.argv[1] not in ("windows", "linux"):
        raise SystemExit(__doc__)
    binaries = Path(sys.argv[2])
    output = Path(sys.argv[3]) if len(sys.argv) == 4 else ROOT / "target" / "packages"
    output.mkdir(parents=True, exist_ok=True)
    (windows if sys.argv[1] == "windows" else linux)(binaries, output)


if __name__ == "__main__":
    main()
