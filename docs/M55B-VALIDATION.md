# M5.5-B validation

## Scope and provenance

This increment implements bounded convex-polyhedral inward hollowing, not arbitrary-STL preparation.
Reference behaviour precedes independent safe native-Rust implementation. No other program's
code, asset, foreign geometry kernel or new dependency is imported. See the
[contract](M55B-HOLLOWING-SPEC.md). Source SHA correspondence remains the
asset loader's assertion; the example uses an asserted synthetic fixture identity.

The working changes are based on commit `14fc04d85e38272664443be61dc1e125f54f062b`.
Raw logs and generated fixtures are outside production source under `/tmp/karasu-m55b-*`.
They are local evidence, not promised permanent repository artifacts.

## Functional and platform gates

Final Linux checks pass `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`,
`cargo build --workspace` and warning-denied private/public documentation generation.
The complete suite reports **761 passed, zero failed, two ignored**. Focused Release integration
and private hollow tests also pass. Logs: `/tmp/karasu-m55b-validation/`, including
`results-final.json` and the `*-final.log` files.

The hash-verified native Windows snapshot uses the documented snapshot runner, including untracked
files: 227 files, archive SHA-256
`a2a47ec52473d6a7db4026b3e11083af6374a11d44caadad7e7e56fc1e4d0fca`.
Before/after integrity checks pass. Native fmt, strict Clippy, complete Debug and Release suites,
build, public/private documentation, focused hollow tests in both profiles, Release private tests,
example build and all four fixture exports pass with **zero failed commands**. Each complete native
profile reports **760 passed, zero failed, two ignored**; one Unix-specific test explains the platform
count difference. No launch block occurred in this campaign and no security policy was changed.

Native evidence directory:
`%LOCALAPPDATA%\Temp\karasu-native-9b5039e3ddac456bb3afa7fde757435d\evidence-5396075c0f5b48e185468cba3ce01bdb`.
Input/runner provenance: `/tmp/karasu-m55b-native/`. Subsequent edits to status documentation and the
fuzz evidence narrative are outside that snapshot; executable Rust and Cargo inputs are unchanged.

The 13 public integration tests cover analytic cube and icosahedron volumes, source/exterior
preservation, world-space nonuniform placement, transformed placement, closed oriented cavity and
two-component shell, determinism, collapse, invalid/nonconvex/multiple sources, unsupported recipes,
resource admission, cancellation/progress and M3 slicing into a material region with a hole.
An 80-face sphere regression retains the requested distance error after the combined-plane
certificate improvement. Twelve private tests cover exact predicates, interval square roots,
convex hull reconstruction, boundary admission and numerical extremes.

## Independent artifact checks

The Python reader uses exact rational arithmetic on represented coordinates and imports no Karasu
code. Eight reader unit tests pass. All four final Linux fixtures and all four native Windows fixtures
pass reader checks: oriented closed edge/component counts, convexity, exact preserved source exterior
and reversed cavity assembly, exact represented volume within reported intervals and the squared
source-halfspace wall bound. The cube additionally checks analytic cavity/material volumes 4096/3904
cubic millimeters and cavity bounds 2..18 mm.

The reader does not independently prove reverse erosion inclusion, vertex links or every coplanar
contact; it supplements the Rust certificate and topology validation. It proves no drainage,
printability or physical strength. Native and Linux `geometry.json`, `shell.stl` and `cavity.stl`
are byte-identical for all four cases. Reports match except intentionally variable `elapsed_ms`.
Evidence: `/tmp/karasu-m55b-validation/native-comparison.json` and independent-reader JSON files.

## Preliminary measured budgets

[Budgets and procedure](../tools/m55b-validate/PROFILING.md) were declared before measurement:
up to five seconds evaluation and 512 MiB whole-process peak RSS on these owned fixtures.
Release results comprise three fresh processes per case; all twelve pass:

| Source | Triangles | Evaluation range (ms) | Maximum process RSS (KiB) | Certified erosion error (mm) |
| --- | ---: | ---: | ---: | ---: |
| Cube | 12 | 7.826–12.126 | 2560 | 1.843e-13 |
| Sphere subdivision 0 | 20 | 13.230–21.941 | 2560 | 4.233e-8 |
| Sphere subdivision 1 | 80 | 125.940–151.239 | 2688 | 9.357e-7 |
| Sphere subdivision 2 | 320 | 1051.984–1607.529 | 3328 | 8.226e-7 |

All use a requested error of 1e-6 mm, without automatically widening it. Derived geometry/STL bytes
repeat exactly across runs; report fields repeat except time. The initial single-plane-only
certificate correctly rejected denser spheres as uncertain. Those failures remain in
`/tmp/karasu-m55b-bench`; the final independently bounded nonnegative plane-combination certificate
passes them, with results/environment/executable hashes in `/tmp/karasu-m55b-bench-final`.
Only a preparation module documentation header changed after those measurements.

Evaluation timing excludes fixture creation and serialization. GNU time RSS includes the whole
process. Host scheduling, caches and frequency are uncontrolled; Linux compilation/testing overlapped
some runs. These are preliminary fixture budgets, not isolated throughput, universal admitted-input
bounds or a performance comparison with commercial slicers. Requested capacity increments are
separate from allocator traffic and RSS. Native fixture exports validate behavior, not a repeated
native performance budget.

## Fuzz evidence and remaining boundaries

The final-source bounded ASan campaign passes **469 executions in 21 reported seconds**, peak RSS
96 MiB, zero failures/artifacts. All core, harness and dependency hashes match preflight.
Twelve fixed seeds replay (four accepted/eight rejected); 177 retained inputs replay (112 accepted,
65 rejected). Typed rejection is an expected outcome. Fuzz formatting, strict all-target Clippy and
focused seed tests pass. See [fuzz procedure](FUZZ-CAMPAIGNS.md) and
`/tmp/karasu-m55b-fuzz/final/`. The earlier 501-execution campaign is retained separately; its
preflight differs only in a later module documentation header. Leak detection was disabled for the
host ptrace constraint; neither leak cleanliness nor Windows sanitizer coverage is claimed.

This is a short campaign, not sustained fuzz acceptance of arbitrary meshes. The backend supports
one embedded closed outward convex polyhedral solid within documented limits. Concave/multiple,
invalid, numerically ambiguous, collapsed or over-budget inputs fail explicitly. Its certificate
bounds the erosion parameter, not a Hausdorff or physical wall-strength guarantee. Drainage cuts,
generated supports, combined preparation and interactive workflows remain M5.5-C–E and M6/M7.
