# Packaging

The tools that turn release executables into karasu-slicer's packages. The packages are a
prototype and are not signed. Each carries karasu-slicer's licence (`LICENSE.txt`, AGPL-3.0 or
later), its trademark note (`TRADEMARKS.txt`) and the third-party notices. The design
and its measurements are in TD-141 ([docs/TECHNOLOGY-DECISIONS.md](../../docs/TECHNOLOGY-DECISIONS.md)).

| Platform | Package | Contents |
| --- | --- | --- |
| Windows x64 | `karasu-slicer-<version>-windows-x64.zip` | Portable: `karasu.exe`, `karasu-cli.exe`, README, notices |
| Windows x64 | `karasu-slicer-<version>-windows-x64-setup.exe` | Inno Setup installer, per user, no administrator rights |
| Linux x86-64 | `karasu-slicer-<version>-linux-x86_64.tar.gz` | The same tree as the `.deb` under a prefix, with `install.sh` |
| Linux x86-64 (Debian, Ubuntu) | `karasu-slicer_<version>_amd64.deb` | Under `/usr` |

Every asset the application uses (icons, the splash, the interface catalogs, the printer catalog)
is compiled into the executables, so a package is the executables and their documents.

## Brand assets

The owner keeps the masters (the brand folder); the repository holds only the images the
application and the packages use.

- **Application icon:** from the owner's master `karasu_icon_master_v2.png` (1254 ×
  1254, transparent background), `make_icons.ps1 -Master <file>` (Windows, System.Drawing) centres
  the emblem so it spans 96 % of the square, resamples each size from 16 to 512 pixels with
  high-quality bicubic filtering, and sharpens the sizes up to 48 pixels with an unsharp mask;
  `make_ico.py` packs 16 to 256 into `karasu.ico`, which the build puts into `karasu.exe` (a
  resource file written by `crates/karasu-app/build.rs`, linked by MSVC; no resource compiler). `-Outline <fraction>` adds a contour around
  the emblem (none by default). The window uses the 256 pixel PNG; the Linux packages install
  each size into the hicolor theme.
- **Splash artwork:** from the owner's artwork, which carries the name (1672 × 941),
  `tools/assets/make_splash.ps1 -Master <file>` writes `assets/ui/images/splash.png` (see
  `docs/UI-DESIGN.md`, Startup splash): resampled to 1440 × 810, square and opaque.

## Releases

Releases are built by `.github/workflows/release.yml` (TD-150) on GitHub's runners: a tag
`vX.Y.Z` builds every package below from the tagged commit and opens a draft release with them,
`SHA256SUMS`, a build provenance attestation and `release-notes.md` (the download notes; `{version}`
is replaced). Pull requests that change this folder or the workflow build the same packages as
workflow artifacts. [docs/RELEASE-PROCESS.md](../../docs/RELEASE-PROCESS.md) describes the
steps.

## Building by hand

1. Build the release executables on each platform:

   ```console
   cargo build --release -p karasu-app -p karasu-cli --locked
   ```

2. Assemble the packages (Python 3.11 or newer; the crates must be in Cargo's cache, since the
   notices read `cargo metadata --offline`). The output folder defaults to `target/packages`.

   ```console
   python3 tools/packaging/package.py linux target/release
   python3 tools/packaging/package.py windows <Windows target\release folder>
   ```

   The Linux `.deb` needs `dpkg-deb`. The Windows command can run under WSL against the native
   build's folder; it writes the ZIP and stages its files in `target/packages/<stem>/`.

3. Build the Windows installer with Inno Setup 6 (6.7.3 tested; `winget install
   JRSoftware.InnoSetup --scope user` puts `ISCC.exe` under `%LOCALAPPDATA%\Programs\Inno Setup 6`)
   from the staged folder:

   ```console
   ISCC.exe /DVersion=<version> /DSource=<staged folder> /DOutput=<output folder> tools\packaging\karasu.iss
   ```

The packages are reproducible for the same executables: entries are sorted and dated at
`SOURCE_DATE_EPOCH`, by default the last commit's time.

## What each package does

- **Windows installer:** installs under `%LOCALAPPDATA%\Programs\karasu-slicer`; a Start menu
  entry, an optional desktop one; `.karasu` projects open in karasu (per user, under
  `HKEY_CURRENT_USER\Software\Classes`); an uninstaller in Settings > Apps. The application's
  settings and profiles live in the user's application data and are not removed.
- **Linux tar.gz:** `./install.sh` copies the tree into `~/.local` (or `--prefix <folder>`),
  points the menu entry at the installed executable and refreshes the MIME, menu and icon caches
  when their tools exist; `./install.sh --uninstall` removes the files it recorded.
- **Linux .deb:** the same files under `/usr`. The system's triggers refresh the caches.
  Dependencies: the C library and `libgcc-s1`, `libxkbcommon0`, and Vulkan or EGL; the windowing
  libraries and the desktop portal (file dialogs) are recommended.

## Third-party notices

`notices.py <target triple> <output>` lists every crate compiled into the executables for a
platform (normal dependencies reachable from `karasu-app` and `karasu-cli`), with its version,
licence and repository, followed by the licence and notice texts their packages carry, each text
once. Crates whose packages carry no licence file are listed with their authors and declared
licence, followed by the standard texts of those licences.

## Known limits

- `karasu-cli.exe` carries no icon; `karasu.exe` does.
- No signing (Windows Authenticode, Linux package signatures) until the public response
  justifies a certificate (TD-150): see
  [docs/research/CODE-SIGNING-RESEARCH.md](../../docs/research/CODE-SIGNING-RESEARCH.md).
  Releases carry `SHA256SUMS` and a build provenance attestation instead.
- The command line is not added to `PATH` by the Windows installer.
- The `resin-cli` compatibility alias is not packaged.
