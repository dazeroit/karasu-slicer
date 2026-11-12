# Native Windows validation of the CLI and engine work (TD-043–TD-054)

**Date:** 2026-10-19. **Result:** pass on native Windows MSVC after two test-only fixes. Every gate
passes, and all CLI and example outputs are byte-identical to Linux.

The previous native Windows pass closed M5.5 (TD-041, [closure](M55-CLOSURE-ACCEPTANCE.md)). Since
then, the following work had only been validated on Linux:
- the `slice` command (TD-043) and Ctrl-C (TD-044);
- sampled hollowing and drainage (TD-045–TD-048);
- parallel and winding-union layers (TD-050);
- PP1 metrics (TD-051);
- antialiasing (TD-052);
- large models (TD-053);
- defective meshes (TD-054).

## Procedure

Validation followed [tools/windows-native](../tools/windows-native/README.md): a hash-verified
snapshot, every gate run natively, and `-IncludeExamples`.

Host and toolchain:
- `rustc 1.99.0 (b940084d7 2026-09-28)`, host `x86_64-pc-windows-msvc`, `cargo 1.99.0`;
- the same Rust version on Linux (WSL2).

The Code Integrity query found no events for the run.

## Runs, in order (initial failures preserved)

| Run | Snapshot | Outcome | Classification |
| --- | --- | --- | --- |
| 1 | `7a78c4f`, clean, 319 files, archive `f32f7f82…616408b` | 9 commands failed: `ctrlc` is not in the offline Cargo cache | Environment prerequisite: TD-044 added the dependency after the last native run |
| 2 | Same snapshot, `-Online` (locked downloads only) | clippy, Debug tests and Release tests failed; build, rustdoc and examples passed | **Two test defects**, invisible on Linux (below) |
| 3 | `7a78c4f` plus the two fixes (dirty), 319 files, archive `8cea2d4ba2879e8b72c3a3081e712f1bcdb02fad300facf07114501c2cde6302` | All commands exit 0, offline; source integrity before and after passes | Pass |

Defects found by run 2:
1. **Unused imports on Windows:** `crates/karasu-cli/tests/slice.rs` imported I/O and process items
   at module level that only the Unix-only SIGINT test uses. Warnings-denied clippy failed on
   Windows. The imports now live inside that test.
2. **Non-hermetic test:** `crates/karasu-core/tests/face_z_index.rs` (TD-046) read FX-11 from
   disk. FX-11 is a reproducible but Git-ignored 16-MiB fixture, so the test passed only on
   checkouts that had generated it. It now generates FX-11 in memory from the shared fixture
   generator, as `raster_corpus`, `m3_acceptance` and `winding_layers` already do.

No production code changed. Every other test file that refers to FX-11 generates it. The FX-12
tests are `#[ignore]`d because that fixture is local only.

## Gates (run 3)

| Gate | Linux (WSL2) | Native Windows MSVC |
| --- | --- | --- |
| fmt, warnings-denied all-target clippy, workspace build | Pass | Pass |
| Debug tests | 1002 passed / 0 failed / 3 ignored | 1000 / 0 / 3 |
| Release tests | Pass | 1000 / 0 / 3 |
| Warnings-denied public and private rustdoc | Pass | Pass |
| `print_reference` example (small and P1) | Pass | Pass |

The two-test difference is the two `#[cfg(unix)]` CLI tests:
- the Unix path test in `tests/inspect.rs`;
- the SIGINT cancellation test in `tests/slice.rs`.

## Cross-platform output identity

The Windows binaries came from run 3. The Release `karasu-cli.exe` has SHA-256
`c197493eda6dcfbd27928539e8b771640cff1315c2e87cb78f8f454790bef432`. Inputs were the tracked corpus,
the local FX-11 and FX-12 files, and the same printer and resin profiles.

Every artifact was compared with its Linux counterpart. PP1 was compared ZIP member by member:
the only allowed difference is the `os` value in `software_info.conf` (`linux` or `windows`).

| Case | Covers | Result |
| --- | --- | --- |
| FX-11, `--hollow 2`, two drain holes, GOO | TD-045, TD-047, TD-048 | Identical |
| FX-11 on a 13312 × 5120 panel, PP1 (1208 members) | TD-050, TD-051 | Identical except `os` |
| FX-11, `--antialias 4`, CTB | TD-052 | Identical |
| FX-10A (open), per-layer closure, GOO | TD-054 phase 1 | Identical |
| FX-10A, `fill-holes`, GOO | TD-054 phase 3 | Identical |
| FX-10A, `fill-holes` and `--hollow 2`, GOO | TD-054 phase 3 | Identical |
| FX-09, default repair, GOO | TD-054 phase 2 | Identical |
| FX-12 (2.7 M triangles), GOO | TD-053, TD-054 | Identical |
| `print_reference` small and P1 (CTB, GOO, Photon, PP1, reference PNG/JSON) | M5 exporters | Identical except PP1 `os` |

The stdout summaries are identical apart from the input and output paths, and the PP1 output size
(2 bytes longer: `windows` versus `linux`).

## Remaining limits

- Ctrl-C on Windows (TD-044) is covered by build and run only. The cancellation test sends SIGINT
  and is Unix-only, so console control events still need a manual or dedicated test.
- These are single-host results. Printer and firmware acceptance remain M7 work.
- The raw evidence stays outside the repository:
  - Windows: `%LOCALAPPDATA%\Temp\karasu-native-d614a3b6cc0a47cabbe12401c31b44ed`;
  - Linux comparison outputs: the session scratch directory.

## Revalidation after TD-055 (span masks), 2026-10-19

TD-055 changed every layer image, encoder and parallel window, so the full native procedure was
repeated on its commit.

**Snapshot and run:**
- clean commit `ed553b9`, 322 files, archive
  `040c50e2649fd94e5f78c9b9d3d843872ea093066af5f1a6d4a22a52910f1c69`;
- offline with `-IncludeExamples`, on the same host and toolchain;
- every command exits 0 on the first run, and source integrity passes before and after.

**Gates:** fmt, warnings-denied clippy, workspace build, and public and private rustdoc pass.

| Test configuration | Linux | Native Windows MSVC |
| --- | --- | --- |
| Debug | 1005 / 0 / 3 | 1003 / 0 / 3 |
| Release | not run | 1003 / 0 / 3 |

The two-test difference is the two Unix-only tests.

**Cross-platform outputs:** the Release `karasu-cli.exe` has SHA-256
`02d2638fe65eccf3c3b8438ec56d3caa995299679f1b64dcca67495589f4d12a`. It ran 14 CLI cases, compared
with Linux on the same profiles and inputs. The cases cover every built-in format:
- FX-11 drained GOO;
- FX-11 CTB with 4× antialiasing;
- FX-11 Photon V1;
- FX-11 reference directory (PNG layers and manifest);
- FX-10A closed, and filled and hollowed;
- FX-09 repaired;
- FX-11 PP1 at 13312 × 5120;
- at 15120 × 6230:
  - FX-11 binary, with 4× antialiasing, and hollowed with a drain hole;
  - FX-12 binary;
  - FX-12 with 4× antialiasing (CTB);
  - FX-12 hollowed.

**Results:**
- 1214 of the 1215 output files are byte-identical.
- The PP1 (1208 ZIP members) differs only in the `software_info.conf` `os` value.
- The 15 `print_reference` example files are identical, again apart from the PP1 `os` value.
- The stdout summaries are identical apart from paths and the 2-byte longer PP1.

## Revalidation after TD-056 (parallel topology analysis), 2026-10-19

TD-056 added scoped-thread sorting to the topology analysis and concurrent diagnosis and repair
in the CLI. The full native procedure was repeated on its commit.

**Snapshot and run:**
- clean commit `3c13161`, 323 files, archive
  `6e6e27baf6ac8d5ad57fa63a2200be1efa012fbeb1fb8894c02bfc5c0e980495`;
- offline with `-IncludeExamples`;
- every command exits 0 on the first run, and source integrity passes before and after.

**Tests:** Debug and Release give 1006 passed, 0 failed, 3 ignored on Windows, against 1008 on
Linux. The difference is the two Unix-only tests.

**Outputs:** the Release `karasu-cli.exe` has SHA-256
`77330fc1d4b391770f7607d7bbcbe0ef81400a1af006a2979f749ae4237afbb0`. On the same 14 CLI cases
as the TD-055 revalidation:
- 1214 of 1215 files are byte-identical to Linux, and the PP1 differs only in its `os` value;
- the summaries are identical, including the defect and repair lines;
- the `print_reference` example files are identical apart from the PP1 `os` value.

## Revalidation after TD-057 (OBJ and 3MF import), 2026-10-19

TD-057 added the first new third-party crates since the M5.5 closure: `miniz_oxide`, `adler2` and
`quick-xml`. The run therefore used `-Online` from the start, which allows locked crates.io
downloads only.

**Snapshot and run:**
- clean commit `f53d629`, 329 files, archive
  `093a11d43d2fa6dff739831c055f5cee692f719c8ea435b2f874941a7ef96e3a`;
- every command exits 0 on the first run, and source integrity passes before and after.

**Tests:** Debug and Release give 1019 passed, 0 failed, 3 ignored on Windows, against 1021 on
Linux. The difference is the two Unix-only tests.

**Outputs:** the Release `karasu-cli.exe` has SHA-256
`78a5939dd136d4e98217d2b2269b9897fb81afb2d8222b7d7b9dfbf3a93410d2`. The 14 earlier cases were run
with two new ones: FX-11 from OBJ (binary GOO) and from deflated 3MF (CTB with 4× antialiasing),
both at 15120 × 6230.
- 1216 of 1217 files are byte-identical to Linux, and the PP1 differs only in its `os` value.
- The 16 summaries are identical apart from paths and sizes.
- The example files are identical apart from the PP1 `os` value.

## Revalidation after TD-058 (parallel model import), 2026-10-19

TD-058 added sharded parallel welding, chunked OBJ parsing and threaded 3MF inflation.

**Snapshot and run:**
- clean commit `71d91a3`, 331 files, archive
  `849debcdd435b2192a92b8dd2616e18cb02571bd84d0dd2f018c2637afb9bb3d`;
- offline (the locked crates were already cached); every command exits 0 on the first run, and
  source integrity passes before and after.

**Tests:** Debug and Release give 1022 passed, 0 failed, 3 ignored on Windows, against 1024 on
Linux. The difference is the two Unix-only tests.

**Outputs:** the Release `karasu-cli.exe` has SHA-256
`dfafa61ce1aa1a7bc0e517c0bc19e8c63d35be42ffa5ffa81b49339ff9b278af`.
- On the 16 CLI cases, 1216 of 1217 files are byte-identical to Linux, and the PP1 differs only in
  its `os` value.
- The 16 summaries are identical apart from paths and sizes.
- The example files are identical apart from the PP1 `os` value.

**FX-12 at 15120 × 6230, whole process on native Windows (three runs):**

| Input | Times |
| --- | --- |
| STL | 1.90, 1.97, 1.86 s (2.22 s before TD-058) |
| OBJ (140 MB) | 1.99, 1.97, 1.97 s |
| Deflated 3MF (55 MB) | 3.05, 3.04, 3.05 s |

All three give the Linux STL file byte for byte.


## Revalidation after TD-059 (rendered print-file previews), 2026-10-19

TD-059 added the deterministic preview renderer and stores its images in every machine format.

**Snapshot and run:**
- the uncommitted TD-059 working tree on `8a914a3`, including the new untracked
  `preview.rs`;
- 333 files, archive `de3f7c298d660329d498c780c9df5a7ce841f2c82d3d55e5f104b156fb0806a9`;
- offline (no new crates); every command exits 0 on the first run, and source integrity passes
  before and after.

**Tests:** Debug and Release give 1028 passed, 0 failed, 3 ignored on Windows, against 1030 on
Linux. The difference is the two Unix-only tests.

**Outputs:** the Release `karasu-cli.exe` has SHA-256
`6e3ba38224f4cb39a2883723688b4cd3c38970b9134c00d9d0e6ca17f768dbf0`.
- On the 16 CLI cases, 1216 of 1217 files are byte-identical to Linux, including every rendered
  preview, and the PP1 differs only in its `os` value.
- The 16 summaries are identical apart from paths.
- The example files are identical apart from the PP1 `os` value.

**FX-12 at 15120 × 6230, STL, whole process on native Windows:** 2.03, 1.99 and 2.02 s in three
runs, including preview rendering (1.86–1.97 s before TD-059).

## Revalidation after TD-060 (minimum edge gray), 2026-10-19

TD-060 added `RasterConfig::minimum_gray` and the CLI option `--min-gray`.

**Snapshot and run:**
- the uncommitted TD-060 working tree on `96c09dd`, with no untracked files;
- 333 files, archive `06b0c0e0b55ea1ad1b311c5b332a0188179970041e23617b6ff52fdea9a9e191`;
- offline (no new crates); every command exits 0 on the first run, and source integrity passes
  before and after.

**Tests:** Debug and Release give 1031 passed, 0 failed, 3 ignored on Windows, against 1033 on
Linux. The difference is the two Unix-only tests.

**Outputs:** the Release `karasu-cli.exe` has SHA-256
`534813d07ec5c0bb0714ad04589ee0b9241622ab48c71e8b0e59de29a3435232`.
- On the 16 CLI cases (without the option), 1216 of 1217 files are byte-identical to Linux, and
  the PP1 differs only in its `os` value.
- The example files are identical apart from the PP1 `os` value.
- **FX-11 at 4 × 4 with `--min-gray 77`:**
  - GOO at 16K, CTB and the 1200-layer reference directory are byte-identical to Linux.
  - The PP1 differs only in its `os` value.
  - Summaries are identical apart from paths and the PP1 size.

## Revalidation after TD-061 (placement, XY compensation and resin usage), 2026-10-19

TD-061 added deterministic degree rotations, `--rotate`, `--scale`, `--mirror`, `--center`,
`--drop`, bottom-layer raster configuration with `--xy-compensation` and
`--bottom-xy-compensation`, and resin usage in the summary.

**Snapshot and run:**
- clean commit `843156e`, 333 files, archive
  `b8b910023ccf70325ec199af3641d10cdfc1aeeb624cbdaf8016f6593a414eb0`;
- offline (no new crates); every command exits 0 on the first run, and source integrity passes
  before and after.

**Tests:** Debug and Release give 1039 passed, 0 failed, 3 ignored on Windows, against 1041 on
Linux. The difference is the two Unix-only tests.

**Outputs:** the Release `karasu-cli.exe` has SHA-256
`4e8496700fd49efd093715bed52cfa39c0f4774cd351491d743cf058aae470ef`.
- On the 16 CLI cases, 1216 of 1217 files are byte-identical to Linux, and the PP1 differs only
  in its `os` value. The summaries are identical apart from paths and sizes.
- **Seven new cases, using a resin profile with density and cost:**
  - FX-12 rotated 45° and centred and dropped (GOO, 16K);
  - FX-12 under a compound rotation (30°, −20°, 15°), scaled 0.8, with 4× antialiasing (CTB);
  - FX-11 mirrored in Y and turned 90° about Y (GOO);
  - FX-11 rotated (12.5°, 0°, 33°) and hollowed (GOO);
  - FX-11 with +1 px compensation and −2 px on the bottom layers (PP1);
  - FX-11 with antialiasing, minimum gray 60 and −1 px compensation (GOO);
  - FX-11 rotated 45° about Y with bottom compensation (reference format, 1200 layers).
- Every new-case file is byte-identical to Linux, except the PP1 `os` value. Summaries, including
  translations, transforms, resin volume, mass and cost, are identical apart from paths. Degree
  rotations are therefore platform-independent.

## Revalidation after TD-062 (manual supports), 2026-10-19

TD-062 added the `supports` core module (document, geometry, validation) and `--supports`,
`--accept-support-collisions` and `--lift`.

**Snapshot and run:**
- the uncommitted TD-062 working tree on `01ed3e0`, including the new untracked files;
- 341 files, archive `f64decd32d88c0dfe29816871199f785cdd5316668d73ad384cee4b23cfb5212`;
- offline (no new crates); every command exits 0 on the first run, and source integrity passes
  before and after.

**Tests:** Debug and Release give 1055 passed, 0 failed, 3 ignored on Windows, against 1057 on
Linux. The difference is the two Unix-only tests.

**Outputs:** the Release `karasu-cli.exe` has SHA-256
`da41a28be965830278b8706e12152c5dedabc4e411221aeeaa5ec5d9f50f91aa`.
- On the 16 CLI cases, 1216 of 1217 files are byte-identical to Linux, and the PP1 differs only
  in its `os` value. The summaries are identical apart from paths and sizes.
- **Five support cases**, with support documents generated on a 3 mm grid under
  downward-facing surfaces:
  - FX-11 with 240 supports (GOO, 16K);
  - FX-11 with 240 supports and a raft (PP1, 1308 members);
  - FX-12 with 137 supports, as GOO and as 4× antialiased CTB;
  - a hollowed cube with an interior pillar from the cavity ceiling to its floor.
- Every support-case file is byte-identical to Linux, except the PP1 `os` value. Summaries,
  including support counts and findings, are identical apart from paths and sizes. Support
  geometry and validation are therefore platform-independent.

## Revalidation after TD-063 (output formats), 2026-10-19

TD-063 added plain CTB v3–v5, the Anycubic Photon Workshop binary family and Prusa SL1/SL1S.

**Snapshot and run:**
- the uncommitted TD-063 working tree on `d8970a3`, including the new untracked files;
- 350 files, archive `e022b195c62e03dc8e97e4b59892845c39d9fd62f2c2c781c2cbbedae43d9591`;
- offline (no new crates); source integrity passes before and after.

**Gates:** fmt, clippy, Debug and Release tests, build, both rustdoc builds and the example build
exit 0.

**Tests:** 1061 passed, 0 failed, 3 ignored on Windows, against 1063 on Linux. The difference is
the two Unix-only tests.

**Example runs (`print-small`, `print-p1`): blocked by the environment, not failed.**
- Both runs, and one unchanged retry with `-ExistingRun` (evidence
  `evidence-196f2eac07c24e5786a944a5ce09a2f3` and `evidence-b0eb4e180ff8417c9fabf3979bff2a06`),
  failed to launch the freshly built `print_reference.exe` with "Access denied".
- Read-only diagnosis:
  - the file was held "in use by another process" while no `print_reference` process ran;
  - even `Get-FileHash` could not read it;
  - the matching CodeIntegrity events name the host's Kaspersky antivirus components, and no
    App Control block was recorded for the binary.
- The binary was neither modified nor excluded from scanning.
- **Cause (confirmed by the owner):** Kaspersky Premium reported the fresh executable as
  `VHO:Trojan-Ransom.Win32.Convagent.gen`. That is a heuristic, reputation-based detection; it is
  treated as a false positive for this locally built, hash-verified, network-free tool.
- **Resolution:**
  - the owner temporarily suspended the protection;
  - an unchanged `-ExistingRun` (evidence `evidence-4c55e6002b67463a8dda392217269611`) then
    passed every command, examples included, with `FAILED_COMMANDS 0`;
  - `print_reference.exe` has SHA-256
    `0b4e28bb5d58b6ef2c84b689358fa08ba44e081020ab7c2f6cc5b2f6b0ea988c`;
  - its sample and P1 outputs (15 files) are byte-identical to Linux, except the PP1 `os` value.

**Outputs:** the Release `karasu-cli.exe` has SHA-256
`9bdf735bd813fbf404c493b069a16d7bf043d7e35f0e3b53f176c7a0f952979c`.
- On the 16 CLI cases, every file is byte-identical to Linux, except the PP1 `os` value. Summaries
  are identical apart from paths and sizes.
- **Ten new-format cases on FX-11 with 4× antialiasing:**
  - CTB v3, v4 and v5;
  - Anycubic `pws`, `pwmx`, `pmsq`, `pwma`, `pm5` and `pm5s`;
  - Prusa SL1S at 1620 × 2560.
  Every file is byte-identical to Linux, including the SL1S archive, which records no OS or time.
  Summaries are identical apart from paths.

## Revalidation after TD-064 and TD-065 (layer plans and plates), 2026-10-19

TD-064 added variable and adaptive layer heights (`--layer-plan`); TD-065 added several models per
plate (`--plate`) and arranged copies (`--copies`, `--arrange`, `--spacing`).

**Snapshot and run:**
- the uncommitted TD-064/TD-065 working tree on `cc049ff`, including the new untracked files;
- 356 files, archive `924478644621859566cfcaac92c65388eaa9ec4b5cbeccbb2ff1d1fdb79bc88f`;
- offline (no new crates); source integrity passes before and after;
- evidence `evidence-73e3c433fb31498082e41ffe6182fc01`, `FAILED_COMMANDS 0`. No CodeIntegrity
  event matched the run.

**Gates:** fmt, clippy, Debug and Release tests, build, both rustdoc builds, the example build and
both example runs exit 0. No antivirus block occurred in this run.

**Tests:** 1074 passed, 0 failed, 3 ignored on Windows (Debug and Release), against 1076 on Linux.
The difference is the two Unix-only tests.

**Examples:** `print_reference.exe` (SHA-256
`d74b81dcc0c6afa18d4892800cb77b37a6b862b915cb62c4e3c73afbf85f866b`) writes sample and P1 outputs
(15 files) byte-identical to Linux, except the PP1 `os` value.

**Outputs:** the Release `karasu-cli.exe` has SHA-256
`880b39c3ddc891aa0adf1d28c0bc79c442ead92bd12a723ee26a78f50f21db42`.
- On the 16 CLI cases, every file is byte-identical to Linux, except the PP1 `os` value.
- **Eleven new cases on FX-11:**
  - fixed and adaptive layer plans in GOO (binary and 4× antialiasing at 16K), CTB v5, Anycubic
    `pm5s`, PP1 and the reference format;
  - three arranged copies at 16K;
  - two arranged copies of the model with 240 supports;
  - the mixed plate: the supported model fixed, six rotated cubes and two drained spheres
    arranged, alone and with an adaptive plan at 4× antialiasing;
  - three drained copies in the reference format.
  Every file (1985 files in all) is byte-identical to Linux, except the PP1 `os` value. Summaries
  are identical apart from paths.

## Revalidation after TD-066 (cavity simplification and oriented drain holes), 2026-10-19

TD-066 added certified cavity simplification (`--hollow-simplify`), drain holes in any direction
and drain holes perpendicular to the surface (`--surface-drain-hole`).

**Snapshot and run:**
- the uncommitted TD-066 working tree on `60e8edd`, including the new untracked files;
- 359 files, archive `de22fc2d07e5cca9f836ed7689c7ae7ee884845561485ed6f0caec6638e7dbad`;
- offline (no new crates); source integrity passes before and after;
- evidence `evidence-c12c04f52323451da5cf6b2943ce734b`, `FAILED_COMMANDS 0`. No CodeIntegrity
  event matched the run.

**Gates:** fmt, clippy, Debug and Release tests, build, both rustdoc builds, the example build and
both example runs exit 0. No antivirus block occurred.

**Tests:** 1082 passed, 0 failed, 3 ignored on Windows (Debug and Release), against 1084 on Linux.
The difference is the two Unix-only tests.

**Examples:** `print_reference.exe` (SHA-256
`55e1e279e5566031cd47e1e6c7eee29b495fe19150c25ef9201b40837dff2e14`) writes sample and P1 outputs
(15 files) byte-identical to Linux, except the PP1 `os` value.

**Outputs:** the Release `karasu-cli.exe` has SHA-256
`945038ab0e60e242c3932683507939aacf0556a8a723ddf78076c368f66a1f2f`.
- The 16 CLI cases and the 11 TD-064/TD-065 cases are byte-identical to Linux, except the PP1
  `os` value.
- **Four new cases:**
  - FX-11 hollowed at 16K with `auto` simplification (1,415,016 → 232,532 cavity triangles);
  - the same with 0.02 mm simplification and a surface drain hole resolved at 45°;
  - a 45° drain hole in the reference format;
  - a plate of two simplified, surface-drained spheres and a simplified cube with an oblique
    hole.
  Every file (1205 in all) and every summary are identical to Linux apart from paths.

## M6.0 desktop foundation (TD-068), 2026-10-20

The first native run of the desktop application crate `karasu-app`, with the new desktop
dependencies (eframe 0.36.2, egui, wgpu 30, winit 0.30).

**Snapshot and run:**
- the uncommitted M6.0 working tree on `4e00a93`, including the new untracked files;
- 365 files, archive `34201f8a2eeda64d8db6c18f7d63d3f47d859c893c7a006f24ea113c422ea402`;
- `-Online` (new crates fetched natively, locked versions), `-IncludeExamples` and the new
  `-IncludeApp`;
- source integrity passes before and after;
- evidence `evidence-50f58081cfe443a9a2e92b8afcf9c8ca`, `FAILED_COMMANDS 0`. No antivirus block
  occurred.

**Gates:** every command exits 0: fmt, clippy, Debug and Release tests, build, both rustdoc builds,
the example build and runs, the application Release build and both smoke tests.

**Tests:** 1088 passed, 0 failed, 3 ignored on Windows (Debug and Release), against 1090 on Linux;
the two Unix-only tests make the difference.

**Examples:** the sample and P1 outputs are byte-identical to Linux, except the PP1 `os` value.

**Desktop application:** `karasu-app.exe` (Release, 13.8 MB, SHA-256
`051d961c7989f2714bec52f031f0cf3783ca46289c8317b88f874923be2ad3d3`) opened its window from the
interactive session and rendered 10 frames in each smoke test.

| Run | Backend | Adapter | Device | Driver | Time |
| --- | --- | --- | --- | --- | --- |
| `--smoke-test` (automatic) | Vulkan | AMD Radeon RX 9070 XT | discrete GPU | AMD proprietary driver 26.9.2 (LLPC) | 1.6 s |
| `--smoke-test --gpu-backend gl` | OpenGL | AMD Radeon RX 9070 XT | other | 4.6.0 Compatibility Profile 26.9.2 | 0.7 s |

The automatic choice selected the discrete GPU on Vulkan. The forced run proves that the OpenGL
fallback renders on native Windows.

## Startup splash (TD-069), 2026-10-20

**Snapshot and run:**
- the uncommitted splash working tree on `694b077`: 368 files, archive
  `93fa28771a7e8088a7c490dc1a7a5c6741a6904f3c75ab1f152c041fc66973ef`;
- `-IncludeExamples -IncludeApp`; source integrity passes before and after;
- evidence `evidence-eece4b7e0fc9490ead45b34be31d35dc`, `FAILED_COMMANDS 0`. No antivirus block
  occurred.

**Gates:** every command exits 0. **Tests:** 1090 passed, 0 failed, 3 ignored on Windows, against
1092 on Linux (the two Unix-only tests).

**Splash smoke tests:**

| Run | Backend | Adapter | Splash time | `transparent` |
| --- | --- | --- | --- | --- |
| `--smoke-test` (automatic) | Vulkan | AMD Radeon RX 9070 XT (discrete GPU) | 1508 ms | false |
| `--smoke-test --gpu-backend gl` | OpenGL | AMD Radeon RX 9070 XT | 1502 ms | false |

**Transparency probe** (a focused build of the application, not a complete run):
- **DX12 through Direct Composition:** reported `transparent: true`, but a screen capture showed
  white corners outside the rounded artwork.
- **Vulkan (opaque surface):** a screen capture showed dark corners matching the artwork's edge.

The Direct Composition change was therefore withdrawn (TD-069), and the window now clears with the
background colour when the surface is opaque.

**Re-run on the final tree:**
- archive `b71222bc02ddce4f1463642cfe0e157dab94b4a7ea9667578b598ea8c1992605`, 368 files. Its
  code is identical to commit `536a1a4`, which differs only in this record.
- `-IncludeExamples -IncludeApp`; source integrity passes before and after.
- evidence `evidence-1ff2ca614da34679911743ca21d50fee`, `FAILED_COMMANDS 0`. No antivirus block
  occurred.
- **Tests:** 1090 passed, 0 failed, 3 ignored (Debug and Release).
- **Smoke tests:**
  - automatic: Vulkan on the discrete GPU, splash 1508 ms;
  - forced OpenGL: splash 1516 ms;
  - both report `transparent: false`, so the opaque fallback applies.
- **Executable:** `karasu-app.exe` (Release), 13.9 MB, SHA-256
  `c7534409c14ec5a6ffff774c4909bdbe20639922c4b69a5df294c655011428f7`.

## Shared pipeline (TD-070, M6.1a), 2026-10-20

**Snapshot and run:**
- the M6.1a working tree, identical to commit `002c7bc`: 374 files, archive
  `f49ee2361910d344b5fe666cb0c5303d234151c1be6e1c3391d70c818cb2c147`;
- `-IncludeExamples -IncludeApp`; source integrity passes before and after;
- evidence `evidence-8e51c2bb88fb43afa9905001b328ce6a`, `FAILED_COMMANDS 0`. No antivirus block
  occurred.

**Gates:** every command exits 0, including the new `karasu-project` crate's build, tests and
documentation.

**Tests:** 1093 passed, 0 failed, 3 ignored on Windows (Debug and Release), against 1095 on Linux
(the two Unix-only tests).

**Examples and smoke tests:** the example runs and both application smoke tests pass (Vulkan on
the discrete GPU and forced OpenGL, splash about 1.5 s).

## Project model and background jobs (TD-071, M6.1b), 2026-10-20

**Initial run (failure preserved):**
- Archive `339b149a9342898efe1b179d63687120f35e07f2d208253cdfd2170eecf98707`, 378 files.
- Evidence `evidence-cc87c2c4b4484617a4c2e704671c2a11`, `FAILED_COMMANDS 2` (Debug and Release
  tests).
- One test failed in both configurations: `a_cancelled_background_job_publishes_nothing` could not
  read FX-11. FX-11 and FX-12 are local fixtures, not tracked in Git, so the hash-verified snapshot
  does not contain them. The test passed on Linux only because the local file exists.
- **This was a test defect, not an engine failure:** the test now generates its own dense model,
  a binary STL box of about 200,000 triangles.

**Complete re-run on the corrected tree:**
- Archive `b6f8cfeb8e77b36a1e2c179cb451f187391635291dd228baa50ce143f91f965f`, 378 files;
  `-IncludeExamples -IncludeApp`; source integrity passes before and after.
- Evidence `evidence-b8869675b0554918a5c8971e91d1ab21`, `FAILED_COMMANDS 0`. No antivirus block
  occurred.
- **Tests:** 1099 passed, 0 failed, 3 ignored on Windows (Debug and Release), against 1101 on Linux
  (the two Unix-only tests).
- **Examples and smoke tests:** all pass (Vulkan and forced OpenGL).

## Supports that follow their object (TD-072, M6.1c), 2026-10-20

**Snapshot and run:**
- the M6.1c working tree: 378 files, archive
  `22f5f9311f6e90d72541e836b817bcbbdcb118c7980544bb77bcb92530c023fb`;
- `-IncludeExamples -IncludeApp`; source integrity passes before and after;
- evidence `evidence-2906c93ac73e447ab71eb9f23d4a4e78`, `FAILED_COMMANDS 0`. No antivirus block
  occurred.

**Tests:** 1103 passed, 0 failed, 3 ignored on Windows (Debug and Release), against 1105 on Linux
(the two Unix-only tests).

**Examples and smoke tests:** all pass (Vulkan and forced OpenGL).

## The 3D viewport (TD-073, M6.2), 2026-10-20

**Snapshot and run:**
- the M6.2 working tree: 386 files, archive
  `0786afb5327822fad9cb378571cf463b6562d42ba3bee92d4d6fec0e60ed6d12`;
- `-IncludeExamples -IncludeApp`, now with the `app-smoke-model` viewport run; source integrity
  passes before and after;
- evidence `evidence-2deff8a1090741cd9b235ebc4791eecd`, `FAILED_COMMANDS 0`. No antivirus block
  occurred.

**Tests:** 1113 passed, 0 failed, 3 ignored on Windows (Debug and Release), against 1115 on Linux
(the two Unix-only tests).

**Application smoke tests:**

| Run | Backend | Objects, triangles | Frame interval, mean and maximum |
| --- | --- | --- | --- |
| automatic | Vulkan, AMD Radeon RX 9070 XT | none | 12.8 ms, 28.7 ms (10 frames) |
| `--gpu-backend gl` | OpenGL | none | 12.8 ms, 17.2 ms (10 frames) |
| `app-smoke-model` (FX-03) | Vulkan | 1, 1,280 | 16.6 ms, 33.7 ms (90-frame orbit) |

**Large model (local FX-12, recorded in TD-073):** 2,675,038 triangles orbit at the 60 Hz refresh,
16.3 ms mean on Vulkan and 16.4 ms on OpenGL.

**Interactive checks:** selection, the move and rotate handles and a move drag were exercised with
synthesized input, and checked on screen captures.

## The core workflow, repairs and reversed slivers (TD-077 to TD-079, M6.3), 2026-10-20

**Snapshot and run:**
- commit `c66cfccee319c200a184ef00de4fbd4465a57c91`, clean tree: 409 files, archive
  `7f865e9d1c91e54a8184793d4ca836fdcd57eacc64e7c38701263ae942aee07f`. It also covers the M6.3
  workflow commit `065331b`, which had no run of its own;
- `-IncludeExamples -IncludeApp`; source integrity passes before and after;
- evidence `evidence-98e4411b029f4d6c8d678ac71cd2da26`, `FAILED_COMMANDS 0` out of 17 commands. No
  antivirus or application-control block occurred.

**Tests:** 1130 passed, 0 failed, 3 ignored on Windows (Debug and Release), against 1132 on Linux.
The test names match except for the two Unix-only tests (non-Unicode paths and SIGINT
cancellation).

**Application smoke tests:**

| Run | Backend | Objects, triangles | Frame interval, mean and maximum |
| --- | --- | --- | --- |
| automatic | Vulkan, AMD Radeon RX 9070 XT | none | 13.1 ms, 33.4 ms (10 frames) |
| `--gpu-backend gl` | OpenGL | none | 12.1 ms, 17.5 ms (10 frames) |
| `app-smoke-model` (FX-03) | Vulkan | 1, 1,280 | 16.6 ms, 32.5 ms (90-frame orbit) |

**Owner's model (TD-079):** outside the runner, the native Release CLI built from the same tree
sliced the owner's local `Base_01.stl` with `--reversed-tolerance 0.1` to 2097 layers, leaving
0.001064 mm² of reversed slivers empty in 2 layers, the same result as on Linux.

**Not covered:** the Messages window and its fix buttons were not exercised with synthesized
input; the owner checks them by hand.

## Hollowing and drain holes as operations (TD-089, M6.4c), 2026-10-21

**First run:**
- commit `d6e7f8b72887fb65771ccde84a28d8abd48d8435`, clean tree: 428 files, archive
  `8bf090cd4fea9077e27ab84659dfb5bcea0c6b0b3b781a8c27463230d90f6238`; `-IncludeApp`;
- evidence `evidence-3e4424d5032d4d2abf57ebb1a561ea25`, `FAILED_COMMANDS 1`: `app-smoke-gl`
  panicked creating the viewport's frame bind group ("Too many bindings of type StorageBuffers in
  Stage FRAGMENT, limit is 0"). Every other command passed, including tests 1180/0/3 in Debug and
  Release. This was a code defect of M6.4c c4, not an environment block; it is fixed (TD-089 c6
  record).

**Second run:**
- commit `d6e7f8b` with the fix and the tilt determinism change, a dirty tree of 428 files,
  archive `c14cd2768b0bc3445179f877179200c95a88a868de39b45abff72ae4457f1471`; `-IncludeApp`;
  source integrity passes before and after;
- evidence `evidence-d959a6297f6e4de1af9d0e41c39b0b79`, `FAILED_COMMANDS 0` out of 13 commands.
  No antivirus or application-control block occurred.

**Tests:** 1180 passed, 0 failed, 3 ignored on Windows (Debug and Release), against 1182 on Linux;
the difference is the two Unix-only tests.

**Application smoke tests:**

| Run | Backend | Objects, triangles | Frame interval, mean and maximum |
| --- | --- | --- | --- |
| automatic | Vulkan, AMD Radeon RX 9070 XT | none | 11.7 ms, 26.0 ms (10 frames) |
| `--gpu-backend gl` | OpenGL | none | 12.0 ms, 17.4 ms (10 frames) |
| `app-smoke-model` (FX-03) | Vulkan | 1, 1,280 | 16.6 ms, 33.4 ms (90-frame orbit) |

**Cross-platform output:** the native Release CLI sliced the owner's Yorick miniature (706,822
triangles) hollowed at 2 mm, with an axis drain hole and a tilted, fixed-depth surface hole, to the
same print file as Linux (SHA-256 `06878f37d71f16f438f903339d487ab7ed72bd6e25e944ee10e3c335a162341f`),
with identical summaries once the tilt used fixed IEEE operations.

**Interface scenarios:** hollowing, holes from the outer surface and from the cavity, move, turn,
scale and slice on the Yorick, and hole openings on the OpenGL backend, checked by screen capture
(TD-089 c6 record).

