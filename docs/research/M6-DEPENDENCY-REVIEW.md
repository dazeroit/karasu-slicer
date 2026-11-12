# M6.0 desktop dependency review (TD-068)

**Date:** 2026-10-19, for M6.0 ([plan](../M6-PLAN.md), TD-067). It covers the checklist that
`AGENTS.md` requires before any adoption:
- native-Rust status and source availability;
- FFI and build scripts;
- the transitive tree and maintenance;
- tests and robustness evidence;
- performance and memory;
- licences, security and Windows/Linux support.

## Selected versions

| Crate | Version | Released | Licence | Role | Adopted |
| --- | --- | --- | --- | --- | --- |
| `eframe` | 0.36.2 | 2026-09-08 | MIT OR Apache-2.0 | Application and window lifecycle (TD-042) | **yes**, M6.0 |
| `egui` / `egui-wgpu` / `egui-winit` | 0.36.2 | 2026-09-08 | MIT OR Apache-2.0 | Widgets, wgpu renderer, window integration (through eframe) | **yes**, M6.0 |
| `wgpu` | 30.0.1 | 2026-08-22 | MIT OR Apache-2.0 | GPU API (through eframe; direct use from M6.2) | **yes**, M6.0 |
| `winit` | 0.30.13 | 2026-03-02 | Apache-2.0 | Windows and events (through eframe) | **yes**, M6.0 |
| `png` | 0.18.1 | 2026-02-14 | MIT OR Apache-2.0 | Decoding the bundled interface artwork (splash, TD-069) | **yes**, M6.0 splash |
| `egui_dock` | 0.21.1 | 2026-08-06 | MIT | Docking layout (owner decision) | reviewed; added when the layout is built (M6.5) |
| `rfd` | 0.17.2 | 2026-01-12 | MIT | Native file dialogs (owner decision: native for now) | **yes**, M6.3 (TD-077) |
| `fluent-bundle` | 0.16.0 | 2025-05-22 | Apache-2.0 OR MIT | Interface localisation (owner decision, see below) | **yes**, M6.3b (TD-080) |
| `unic-langid` | 0.9.6 | 2025-05-09 | MIT OR Apache-2.0 | Language identifiers for Fluent | **yes**, M6.3b (TD-080) |

**Why 0.36.2 and not 0.37.0:**
- egui and eframe 0.37.0 were published on 2026-10-15, four days before this review.
- `egui_dock` 0.21.1, the latest, requires egui 0.36.
- 0.36.2 is the current maintenance release of the series `egui_dock` supports.

Moving to 0.37 waits for an `egui_dock` release that supports it.

**Reviewed now, added later.** `egui_dock` and `rfd` are reviewed but not added yet: `AGENTS.md`
forbids adding a library before it is used. Each is added with its first use, re-checking its
version.

**`rfd` (added for M6.3, TD-077):**
- With its default features (`xdg-portal`, `wayland`), the lock file gains only `rfd` and
  `pollster` 0.4. `pollster` is a small MIT OR Apache-2.0 executor that blocks on the portal's
  future; 1.0 was already in the tree through wgpu. Everything else was already present.
- Windows uses the common item dialog through `windows-sys`, and Linux uses XDG desktop portals.
  No C or C++ is compiled.

**`png` (added for the splash, TD-069):**
- **Origin:** the image-rs PNG decoder, pure Rust, very widely used (about 270 million downloads).
- **Dependencies:** `bitflags`, `crc32fast`, `fdeflate`, `flate2` and `miniz_oxide`, all already in
  the tree through egui/epaint, so adopting it adds this one crate.
- **Exposure:** it decodes only artwork bundled into the executable at build time (trusted
  input).

## Features enabled

**eframe:** `default-features = false`, with only:
- `accesskit`: accessibility;
- `default_fonts`: bundled fonts;
- `x11` and `wayland`: Linux window systems;
- `wgpu`: rendering.

**Left out:**
- `glow` and `glutin` as eframe renderers: TD-042 selects wgpu, and the OpenGL fallback is wgpu's
  own GL backend;
- `persistence`: added with layout persistence;
- `clipboard`, `links` and `system_fonts`.

**wgpu:** the default backends, through `egui-wgpu`, are Vulkan, DX12, Metal and GLES/GL, plus the
WGSL front end. The GL backend provides the owner-requested OpenGL fallback (TD-067).

## Transitive tree (normal and build dependencies of `karasu-app`)

| Target | Third-party crates | Build scripts | `-sys` crates | Proc macros |
| --- | --- | --- | --- | --- |
| `x86_64-unknown-linux-gnu` | about 250 | 40 | `glutin_egl_sys`, `glutin_glx_sys`, `linux-raw-sys`, `renderdoc-sys`, `wayland-sys` | 19 |
| `x86_64-pc-windows-msvc` | about 165 | 30 | `glutin_egl_sys`, `glutin_wgl_sys`, `renderdoc-sys`, `windows-sys` | 10 |

The workspace lock file records about 380 packages for all platforms.

**Native-Rust status, FFI and build scripts:**
- **No C or C++ code is compiled.** `cc` appears only as a build dependency of `wayland-backend`.
  It is used only by that crate's optional `log` feature, which is off: the enabled features are
  `client_system`, `dlopen` and `default`.
- **No geometry kernel and no foreign algorithm library.** Every crate is Rust source from
  crates.io.
- **FFI is limited to operating-system and driver interfaces:**
  - window systems (X11 through `x11-dl`, Wayland through `wayland-sys`, both loaded at run time;
    Win32 through `windows-sys`);
  - GPU APIs (the Vulkan loader through `ash`; EGL, GLX and WGL through the glutin `-sys` crates;
    DX12 through the `windows` bindings);
  - accessibility (UI Automation on Windows; AT-SPI over D-Bus through the pure-Rust `zbus` stack
    on Linux).

  This is the scope TD-042 anticipated; it does not conflict with the TD-001 ban, which concerns
  geometry kernels.
- **Build scripts** generate code or configuration (naga, khronos API lists, cfg detection). None
  downloads anything.
- **Unsafe code** lives inside these dependencies (graphics and OS bindings). The workspace's
  `unsafe_code = "forbid"` still applies to every karasu crate, `karasu-app` included.

## Localisation: Project Fluent (reviewed and adopted 2026-10-20, TD-080)

The owner asked to prepare the interface for localisation before M6.4 (the interface stays in
English for now) and to evaluate Fluent before adopting it. After this review the owner approved
it; `karasu-app` adds `fluent-bundle` and `unic-langid`, and `fluent-syntax` (already in the tree)
for tests.

**Candidates:** `fluent-bundle` 0.16.0 (2025-05-22), the runtime of
[Project Fluent](https://www.projectfluent.org), and `unic-langid` 0.9.6 (2025-05-09) for
language identifiers. The `fluent` umbrella crate (0.17.0) is not needed. `fluent-pseudo`
0.3.3 is not taken either: it pulls `regex` into the tree, and a pseudo-locale is a short
transform function that `FluentBundle::set_transform` accepts.

**What adding the two to `karasu-app` changes in the lock file**, measured on a copy of the
workspace: eleven new crates, everything else already present.

| Crate | Version | Licence | Origin |
| --- | --- | --- | --- |
| `fluent-bundle`, `fluent-syntax`, `intl-memoizer` | 0.16.0, 0.12.0, 0.5.3 | Apache-2.0 OR MIT | projectfluent/fluent-rs (Mozilla) |
| `fluent-langneg` | 0.13.1 | Apache-2.0 OR MIT | projectfluent/fluent-langneg-rs |
| `intl_pluralrules` | 7.0.2 (2022) | Apache-2.0/MIT | CLDR plural rules |
| `unic-langid`, `unic-langid-impl` | 0.9.6 | MIT OR Apache-2.0 | language identifiers |
| `tinystr`, `zerovec`, `zerofrom` | 0.8.4, 0.11.8, 0.1.8 | Unicode-3.0 | unicode-org/icu4x |
| `displaydoc` | 0.2.7 | MIT OR Apache-2.0 | compile-time derive macro |

`self_cell`, `type-map`, `smallvec`, `rustc-hash` 2, `thiserror` 2, `memchr` and `syn` 3 are
already in the tree through egui and wgpu.

**Checklist:**
- **Native Rust and source:** all pure Rust, sources on crates.io and GitHub; no FFI, no C or C++,
  no build scripts among the new crates. `displaydoc` is a proc macro run at compile time.
- **Licences:** permissive (MIT, Apache-2.0, Unicode-3.0). `self_cell`, already present, is
  Apache-2.0 OR GPL-2.0-only, used under Apache-2.0. Compatible with any licence the owner may
  choose; no project licence is selected yet.
- **Maintenance:** fluent-rs is the reference implementation Firefox uses, about 21 million
  downloads; its last release was in May 2025, a mature and slow-moving project rather than an
  abandoned one. `intl_pluralrules` was last released in 2022 and follows CLDR data that changes
  rarely. The icu4x crates are actively maintained (August 2026).
- **`unsafe`:** the new crates use it in a few narrow places (unchecked construction of
  validated language subtags in `unic-langid-impl`, constant plural-rule tables in
  `intl_pluralrules`). The workspace's own `unsafe` ban is unaffected; dependencies such as
  `smallvec` and `memchr` already use it.
- **Security:** `cargo audit` (1345 advisories) reports no vulnerability and no new warning; only
  the three existing `zbus`/`zvariant` warnings remain.
- **Size:** the Windows Release application grew by about 0.25 MB (18.20 to 18.46 MB) with
  Fluent and the catalog.
- **Windows and Linux:** platform-independent code.
- **Functional probe:** English and Italian messages with CLDR plurals format correctly
  ("1 closed body is inside out", "3 corpi chiusi sono rovesciati").

**Findings that shape the design:**
- Fluent wraps every placeable in Unicode isolation marks (U+2068, U+2069) by default, which
  egui does not need and may draw as boxes; the bundle must disable them. Right-to-left scripts
  are not a goal (egui does not lay them out).
- `fluent-bundle` formats numbers without locale conventions (Italian still gets "0.1").
  Decimal separators and digit grouping must be formatted by the application, with the numeric
  value still passed for plural selection.
- egui's default fonts cover Latin, Greek and Cyrillic; Chinese, Japanese or Korean would need a
  bundled font (a later, separate decision).

**Recommendation:** adopt `fluent-bundle` 0.16.0 and `unic-langid` 0.9.6 in `karasu-app` only.
The headless crates and the command line stay in English.

## Installed memory: `sysinfo` (reviewed and adopted 2026-10-22, TD-094)

TD-094 sizes the interface's hollowing budget from the installed memory, which the owner asked
to be read automatically. Linux exposes it in `/proc/meminfo`. Windows exposes it only through
the `GlobalMemoryStatusEx` system call, which needs `unsafe`, and the workspace forbids it. The
owner chose a reviewed dependency over a local `unsafe` exception, or over a fixed budget on
Windows.

**Candidate:** `sysinfo` 0.39.6 (MIT, [GuillaumeGomez/sysinfo](https://github.com/GuillaumeGomez/sysinfo)),
with default features off and only `system`. `karasu-app` reads one value:
`System::new_with_specifics` with the RAM refresh only, then `total_memory`.

**What adding it to `karasu-app` changes in the lock file:**

| Crate | Version | Licence | Targets |
| --- | --- | --- | --- |
| `sysinfo` | 0.39.6 | MIT | all |
| `ntapi` | 0.4.3 | Apache-2.0 OR MIT | Windows |
| `winapi` | 0.3.9 | MIT OR Apache-2.0 | Windows (through `ntapi`) |
| `winapi-i686-pc-windows-gnu`, `winapi-x86_64-pc-windows-gnu` | 0.4.0 | MIT OR Apache-2.0 | Windows GNU only; import libraries, unused with MSVC |
| `objc2-io-kit` | 0.3.2 | Zlib OR Apache-2.0 OR MIT | macOS only |

`windows` 0.62.2, `libc` and `memchr` are already in the tree through eframe and wgpu. On Linux
`sysinfo` adds itself only.

**Checklist:**
- **Native Rust and source:** pure Rust, sources on crates.io and GitHub. It calls the operating
  system's APIs (Win32 and NT through `windows` and `ntapi`, `/proc` on Linux). No C or C++ is
  compiled, and nothing touches geometry.
- **Build scripts:** `sysinfo` has none. `ntapi`'s and `winapi`'s only emit link and `cfg`
  directives: they choose import libraries and compile nothing.
- **Licences:** permissive (MIT, Apache-2.0, Zlib), compatible with any licence the owner may
  choose.
- **Maintenance:** actively maintained, last release July 2026, about 228 million downloads (55
  million recent). `winapi` 0.3.9 is old but stable and ubiquitous. `ntapi` is maintained for
  `sysinfo`'s use.
- **`unsafe`:** inside the crates only (about 19 sites in the Windows system module, 12 in
  Linux's). The workspace's own `unsafe` ban is unaffected.
- **Tests:** the crate has its own test suites per area (system, processes, disks).
  `karasu-app`'s tests run the memory read on Linux, and native Windows validation runs it on
  Windows.
- **Security:** `cargo audit` (1348 advisories) reports no vulnerability and no new warning. The
  existing `zbus`/`zvariant` maintenance warnings and a yanked `tokio` remain.
- **Performance and memory:** one read at start-up, refreshing the RAM figures only. On Linux it
  reads one `/proc` file.
- **Size:** the Windows Release application grew by about 60 kB (19,552,256 to 19,612,160
  bytes).
- **Windows and Linux:** both supported, and the reason for the choice.

**Recommendation:** adopt `sysinfo` 0.39.6, `system` feature only, in `karasu-app` only. The
headless crates and the command line keep a fixed default budget.

## Settings file: `serde` and `serde_json` (adopted in `karasu-app` 2026-10-26, TD-097)

The settings store writes one JSON file. `karasu-app` now depends directly on `serde` 1.0.228
(with `derive`) and `serde_json` 1.0.150, the exact versions `karasu-core` and `karasu-project`
already use: the lock file gains no crate, and the review of those crates for the headless
core applies (pure Rust, no build script compiling foreign code, MIT OR Apache-2.0, widely
maintained). egui's own `persistence` feature was not enabled: it would add `ron`, a crate not
yet in the tree, to serialize egui's memory, which the store does not need.

## Maintenance, tests and robustness

- **egui/eframe, wgpu and winit:**
  - widely used, actively maintained projects with regular releases (egui about monthly; wgpu
    major releases about quarterly with patch releases);
  - large test suites and continuous integration on Windows, Linux and macOS.
- **egui_dock:** maintained, released alongside egui versions.
- **rfd:** maintained; uses native dialogs (XDG portals on Linux, the common item dialog on
  Windows).
- **Robustness evidence in this repository:**
  - the M6.0 smoke test opens the real window, renders frames through wgpu and reports the
    chosen adapter;
  - it was exercised on Linux (WSLg) with Vulkan and with the OpenGL fallback;
  - on native Windows (AMD Radeon RX 9070 XT) the automatic choice used Vulkan on the discrete
    GPU, and the forced OpenGL fallback also rendered (validation record).

## Security (RustSec, `cargo audit`, 2026-10-19, 1338 advisories)

**No vulnerability** is reported for the lock file. There are four *unmaintained* warnings:

| Advisory | Crate | Pulled in by | Assessment |
| --- | --- | --- | --- |
| RUSTSEC-2024-0436 | `paste` 1.0.15 | `egui_dock` (not yet added) | Compile-time proc macro only; no runtime code; acceptable, re-checked when `egui_dock` is added |
| RUSTSEC-2026-0370/0371/0372 | `zvariant`, `zvariant_derive`, `zbus_names` | `accesskit_unix` → `atspi`/`zbus` (Linux accessibility) | Flagged on 2026-10-16; D-Bus serialization for screen-reader support. Kept for accessibility; replacement tracked upstream (AccessKit); re-checked at every update |

## Performance and memory (first measurements)

These are measurements of the M6.0 window, not budgets:
- **Release binary:** about 26 MB on Linux (unstripped) and 13.8 MB on Windows.
- **Smoke test, Linux (WSLg, llvmpipe software rendering):** 10 frames in 0.33 s, 153 MB peak
  resident memory, most of it the software rasterizer and font atlases.
- **Real budgets** for large meshes and the panel blur are measured in M6.2 and M6.5 on
  representative GPUs.

## Platform support

- **Windows:** native window, DX12 and Vulkan, and OpenGL (WGL) fallback.
- **Linux:** X11 and Wayland, Vulkan, and OpenGL (EGL/GLX) fallback.
- **Validated on:**
  - Linux: WSLg, where both Vulkan and GL run on Mesa llvmpipe;
  - native Windows: automatic adapter selection and the forced OpenGL fallback.
- **Drivers:** behaviour on real GPUs and older drivers is part of the M6 GPU matrix.

## Conclusion

Adopt `eframe` 0.36.2, with the reduced feature set, as the M6 desktop stack. It brings egui,
egui-wgpu, egui-winit, wgpu 30 and winit 0.30. `egui_dock` 0.21.1 and `rfd` 0.17.2 pass the review
and are added when first used.
