# M5.5 bounded headless closure acceptance

## Decision and scope

**Accepted on 2026-10-18: M5.5-A–E are complete for the documented bounded headless domain.**
M6 interactive integration and M7 release/hardware acceptance remain mandatory. This is not expanded
MVP completion, arbitrary-mesh hollowing, a universal Boolean engine or a print-success claim.
See [E specification](M55E-INTEGRATION-SPEC.md), [B](M55B-HOLLOWING-SPEC.md),
[C](M55C-DRAINAGE-SPEC.md) and [D](M55D-SUPPORTS-SPEC.md) for exact contracts.
The supported source remains one closed outward embedded convex polyhedron, certified inward
halfspace hollowing, admitted finite cardinal cuts and explicitly placed polygonal-station supports.
Unsupported nonconvex/multiple-source, uncertain contact/separation and invalid geometry fail explicitly.

Reference behaviour is recorded in reference research. Vendor runtime rollback, numerical
accuracy and stale-output guarantees remain unproven; independent requirements and tests establish
Karasu behavior. No proprietary implementation, new dependency or foreign geometry kernel is used.

## Implemented acceptance matrix

| Scenario / requirement | Executed headless evidence and scope |
| --- | --- |
| S01 / PREP-001,006 | A–D immutable source/canonical history tests plus actual owned STL import, source-byte digest, edits/undo/redo, failed admission, deterministic replay and retained original-plan preview. |
| S02 / PREP-002,005 | Existing B analytic cube and convex polyhedral sphere fixtures, volume/inclusion certificates, collapse and thickness diagnostics. Sphere evidence concerns represented polyhedra and declared approximation; arbitrary smooth-sphere output is not claimed. |
| S03 / PREP-003,005 | Existing C finite cap/extent and cavity-connection tests, closed-cavity count, misses/shallow cuts/clearance and cutter separation. Multiple cavities/sources are unsupported, not silently connected. Connection is geometric, not physical drainage. |
| S04 / PREP-004,005 | Existing D parametric profile/contact/platform/separation tests, including horizontal contacts, plus combined and eight-support cases. Explicit rebind is required after upstream contact changes. |
| S05 / PREP-002–005,009 headless | Same imported source hollowed/drained/supported through M3/M4, reference PNG and four machine formats; independent geometry, exact represented volumes and 1,600,000 analytic pixels. |
| S06 / PREP-001–005,009 headless | Existing invalid/non-manifold/nonconvex/degenerate, collapsed/uncertain/clearance/contact cases plus session failed geometry and limit admission. No repair or partial validated-solid publication. Closed cavity remains an explicit geometric state. |
| S07 / PREP-006,008 headless | Actual worker thread across edit; foreign owner, ABA, undo/redo, same-generation supersession, stale Complete, newest-only acceptance and current-only scene access. GUI interaction remains M6; saved-project replay is not applicable because no project persistence exists. |
| S08 / PREP-001,007 | Existing exact/one-over limits and nested cancellation, deterministic progress, failed/cancelled publication retaining preview, 20 atomic export cancellation cases, eight-support stress and four ASan campaigns. Capacity limits do not guarantee process RSS; caller limits concurrent tasks. |
| S09 / PREP-008,009 headless | Full Linux and native Windows Debug/Release suites and actual STL→combined preparation→M3/M4/M5 examples; independently decoded outputs and cross-platform comparisons. Interactive workflows and representative printer trials remain M6/M7. |

PREP-005/006/007 headless integration is implemented. PREP-008/009 and conditional persistence are
not marked complete by this engine gate. The composed numerical budget is the component-wise
contract in the E specification, not a universal summed Hausdorff or wall-thickness bound.

## Functional tests and independent readers

New Rust suites: `preparation_session` 13, `preparation_progress` 5,
`preparation_integration` 4 and `preparation_stress` 2 tests. All 24 pass in full Debug/Release
runs on both platforms. Integration includes stale contact after cut edit and explicit rebind;
20 export cancellation cases cover four checkpoints across reference, CTB, GOO, Photon and PP1.
Progress tests distinguish computation Complete from session acceptance.

The owned ASCII STL at `tools/m55e-validate/source.stl` has SHA-256
`f8fe73215c28859baff91db874d51fbbe0a6c62ef206a80eb5923df7bec8bbc6`.
M2 imports it in millimeters, preserving original mesh data. The independent Python reader hashes
emitted bytes and checks imported vertices/facets against that source. The general API still treats
source digest/decoded-mesh correspondence as a caller/asset-loader assertion.

The 20 mm cube is translated +10 mm Z, hollowed 2 mm, drained at (5,5), and supported at (12,8)
with three rectangular loft components. Twelve boundary PGMs contribute 480,000 checked pixels;
28 reference PNGs contribute 1,120,000. No sample falls in the declared radial uncertainty annulus.
Exact rational checks cover represented component closure/orientation/volume and source/cavity
structure. The existing independent M5 readers decode CTB v2 plain, GOO v3, Photon v1 and Anycubic
PP1 v3 and compare their masks to the analytically validated reference. This does not certify firmware
or hardware acceptance. Synthetic 0.5 mm layers exercise boundaries, not a recommended print preset.

The stress fixture has eight mutually separated supports, 24 components, one cavity and one cut.
The independent stress reader checks 80,000 pixels: 3,200 base-material pixels and 39,684 wall-material
pixels. It also checks recipe header/source identity and report counts; it is not a complete KSUP1
parser or a substitute for Rust geometry certificates. Both reader suites pass **12 Python tests**,
including deliberately corrupted masks, identity and counts.

## Full platform gates and provenance

| Gate | Linux x86_64, stable Rust | Native Windows MSVC, stable Rust |
| --- | --- | --- |
| Formatting, strict workspace/all-target Clippy, workspace build | Pass | Pass |
| Complete Debug tests | 852 passed / 0 failed / 2 ignored | 851 passed / 0 failed / 2 ignored |
| Complete Release tests | 852 / 0 / 2 | 851 / 0 / 2 |
| Warnings-denied public and private rustdoc | Pass | Pass |
| Combined and stress examples, independent output readers | Pass | Pass |

The single test difference is Unix-specific CLI coverage; WSL results are not called Windows
validation. Linux runs on WSL2 kernel 6.6.87.2, Rust 1.99.0, AMD Ryzen 7 7700. Full gate logs are
`/tmp/karasu-m55e-validation`; native launch metadata/readers are `/tmp/karasu-m55e-native`.

Native execution follows `tools/windows-native/README.md` using the hash-verified snapshot runner,
with two additional example build/run commands. Snapshot base is dirty
`5ca7b267c7f1211020c91c2d4734d704e4823de1`, 279 files, archive SHA-256
`4231ef66b4888b480bcdfc23f576a637194779234f1720ed86475ec4126a6453`.
Before/after integrity checks pass all 279 files; all runner commands exit zero. Evidence lives at
`%LOCALAPPDATA%\Temp\karasu-native-3f89f787ff3247e8b4f086326383ad2a\evidence-14bdad219dc54a13a3b584e1d25b32d0`.
No source/security change or focused-retry substitution was needed. The Code Integrity query was
unavailable and is retained as such; successful actual command exits establish this run's outcome.
Later closure documentation and the standalone stress Python reader are outside that snapshot;
production Rust, Rust tests/examples and owned STL remain unchanged.

Across three fresh Linux Release runs per fixture, all geometry/source/mask/reference and machine
bytes repeat exactly; only the report timer varies. Comparing Windows outputs, combined PP1 also
changes only `software_info.conf.os` from `linux` to `windows`; every other ZIP member matches.
All other artifacts match except report timers. There are 49 combined and three stress files per run.
Comparisons and per-file SHA-256 manifests are `/tmp/karasu-m55e-bench`.

## Declared budgets and measured results

Budgets were recorded in `tools/m55e-validate/PROFILING.md` before timing frozen Release examples:
10 seconds for each named geometry timer and 512 MiB whole-process peak RSS. GNU time measured
three fresh processes per fixture; readers ran separately. Measured geometry times, in ms:

| Fixture | Run 1 | Run 2 | Run 3 | Peak RSS KiB | Whole-process wall seconds |
| --- | --- | --- | --- | --- | --- |
| Combined | 29.299463 | 26.384513 | 27.126224 | 4864 / 4864 / 4984 | 1.72 / 1.68 / 1.67 |
| Eight supports | 125.385207 | 124.946642 | 126.015850 | 4096 / 4096 / 4096 | 0.16 / 0.16 / 0.16 |

Combined geometry timer excludes source/plan/schedule setup and all masks/export; stress includes
source decode and plan admission, excluding masks/output. Whole-process measurements include the
respective emitted artifacts. All declared budgets pass for these fixtures. Scheduling/caches/CPU
frequency are uncontrolled; these are preliminary bounded-case measurements, not isolated throughput,
a universal supported-plan performance guarantee or a commercial comparison. Windows evidence here
is functional, not a repeated native Windows performance campaign.

## ASan and retained replay

| Target | Executions | Reported seconds | Peak RSS MiB | Retained accepted / expected rejected |
| --- | --- | --- | --- | --- |
| preparation_session | 184 | 62 | 111 | 38 / 12 |
| hollow_mesh | 932 | 31 | 108 | 27 / 40 |
| drainage | 300 | 31 | 93 | 11 / 20 |
| supports | 157 | 31 | 75 | 26 / 21 |

All four exit zero with zero artifacts, matching frozen instrumented-source hashes. All **195**
retained inputs replay successfully; rejections are structured expected outcomes, not ignored crashes.
Evidence: `/tmp/karasu-m55e-fuzz/summary.json` and per-target logs/corpora. Initial orchestration
reported missing old temporary B/C corpora; that error is preserved. Fresh documented seed families
were used rather than claiming old corpus reuse. Leak detection is disabled for the host ptrace
constraint; no Windows ASan or universal bug-free claim is made.

After campaigns, rustfmt wrapped one standalone replay-tool match arm (outside workspace fmt).
Pre-format bytes and the cosmetic boundary are archived; instrumented harness/production/tests/examples
are unchanged. Rebuilt formatted replay passes all four corpora; final fuzz formatting and strict
all-target Clippy pass. No instrumented campaign is represented as running changed harness code.

## Remaining work

M6 is next: select and validate desktop technology, provide real non-destructive preparation controls,
preview/diagnostic overlays, bounded background scheduling, cancellation and current-revision export
on Windows and Linux. Project persistence requires verified source/recipe reopen if introduced.
M7 retains native release workflows, distribution/license decisions, interoperability, representative
printer/resin trials and physical claim boundaries. General concave preparation remains an explicit
unsupported-domain extension, not a silently completed capability.
