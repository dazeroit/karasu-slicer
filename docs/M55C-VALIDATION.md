# M5.5-C drainage validation

## Result and implementation boundary

The bounded finite-cardinal drainage backend passes the declared software gates. It certifies
positive-area exterior-to-cavity access for one M5.5-B convex hollow source and evaluates actual
material by exact layer-domain difference. This is a prepared slicing scene, not a generic Boolean
triangle mesh. Oblique axes, through cuts ending outside the cavity, potentially overlapping cutter
boxes and uncertain/invalid cases fail explicitly. Geometric access does not certify physical resin
removal, venting or strength. See the [specification](M55C-DRAINAGE-SPEC.md).

Changes are based on `b298ccf1f164353c98a119a4e02aada7fea0f183`. Reference research records
behaviour only; no runtime algorithm claim or other program's implementation is imported. No
dependency, foreign geometry kernel or licensed reference asset is added. Raw evidence remains
outside the repository.

## Complete quality and native gates

Final Linux passes:

```console
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
cargo test --workspace --release
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --document-private-items
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps
```

Both complete profiles report **793 passed, zero failed, two ignored**. Results and full logs are
in `/tmp/karasu-m55c-validation/results-final.json` and `*-final.log`. Eight independent Python
reader tests also pass. Initial lint and historical cap-expectation failures remain in the original
logs; they are not counted as passes. The terminal-cap regression exposed the missing coherent
mixed cap/side ownership case; the narrow TD-039 rule resolves it while retaining both occurrences.

Updated existing normalization/graph/path tests distinguish a verified local cap perimeter from
unresolved incidence. Existing M3 defect fixtures can have a coherent section on a plane away from
their source defect; their expected local status now records that distinction. The source remains
unchanged, all events remain retained, and a complete local layer is not a global source-validity
certificate. Winding errors, duplicate incidence and degenerate caps still fail the new focused
regressions. C additionally relies on B's independent global source admission.

Native Windows uses the documented hash-verified snapshot procedure with reviewed untracked files:
243 files, source archive SHA-256
`3d65aad9dc892933bbe63943d14158f3729f2fb959fbc26dcd7dcf7e13af520d`.
Native fmt, strict all-target Clippy, complete Debug/Release suites, build, public/private rustdoc,
Release drainage example build and fixture execution all pass with **zero failed commands**.
Each complete profile reports **792 passed, zero failed, two ignored**; the Unix-only test explains
the count difference. Both before/after snapshot integrity checks pass. No application-control
launch block occurred during native validation; no persistent security policy was modified.

Input/runner provenance is in `/tmp/karasu-m55c-native`. The temporary copy of the generic runner
adds the Release drainage example/export commands; other validation commands are unchanged.
Evidence directory:
`%LOCALAPPDATA%\Temp\karasu-native-af3bf1c9a6b248d9bc5aaaa9bc902e1d\evidence-bb30f316476640418c0bff0d16ec509e`.
An initial sandboxed WSL interop invocation failed before launching validation; the authorized native
invocation succeeded. Later changes concern documentation/evidence narratives only: executable Rust,
Cargo and lock inputs match the snapshot. Newly completed validation documentation was not a compiled
snapshot input and is not presented as such.

## Focused geometry and independent artifact checks

New coverage comprises 12 public drainage integration tests, eight exact difference tests, three
layer-adapter tests, four mixed-cap regressions and five private cylinder tests. They cover all six
cardinal directions, analytic polygon area, independent material occupancy, hollow wall retention,
terminal-cap planes, source/plan immutability, zero/opened cavity diagnostics, edit/remove recompute,
missed/shallow/oversized/non-exterior cuts, oblique rejection, overlap, independent budgets,
cooperative cancellation, malicious rings and translated radial/axial bounds. Random rectangle
arrangements compare exact difference with independently enumerated cell area and occupancy.

The owned example constructs a 20 mm cube, 2 mm shell, and a diameter-2 mm cut from `(10,10,-1)`
towards positive Z for 4 mm. Requested approximation error is 0.001 mm. It evaluates actual drained
M3 layers and M4 binary center-sampled 200x200 masks over `[0,20]²`, without compensation or AA.
Its source stamp is a synthetic caller assertion, not a verified asset digest.

The independent standard-library Python reader uses exact rational pixel centers and squared
circular distance brackets. All 160,000 centers are checked in each fixture set; none fall in the
permitted radial uncertainty annulus. The Z=1 bottom-wall opening has 39,684 material pixels;
Z=2.5 and exact terminal cap Z=3 each have 14,400; the intact top wall Z=19 has 40,000.
All three final Linux repeats and the native export pass. The reader's eight tests deliberately
corrupt settings, masks, caps, intensities and error bounds. It supplements rather than replaces
Rust connectivity/cylinder proofs and does not validate physical drainage or arbitrary CSG topology.

Corresponding PGM bytes are identical between all repeats and Windows/Linux. Report fields also
match except variable `elapsed_ms`, including full canonical plan bytes and error bounds. The plan
record SHA-256 is `11308815673c58c3de9ebcc4b959b4aaa10241859dcc3894c4b4ae15eb671d56`.
The example uses 128 cylinder sides; reported radial error is about 0.000301181304 mm and axial
error about 4.441e-16 mm, with their outward-rounded sum below the requested tolerance.
Native comparison and independent reports are in `/tmp/karasu-m55c-validation`.

## Preliminary fixture performance

[Budgets](../tools/m55c-validate/PROFILING.md) were declared before measurement: at most 30 seconds
for preparation plus four layer evaluations/rasterizations and 512 MiB whole-process peak RSS.
Final Release results use three fresh processes and absent output directories:

| Run | Evaluation (ms) | Whole-process peak RSS (KiB) | Independent reader |
| --- | ---: | ---: | --- |
| 1 | 157.587 | 3968 | Pass |
| 2 | 142.771 | 4096 | Pass |
| 3 | 135.694 | 4096 | Pass |

All pass the declared fixture budgets. Timed evaluation excludes fixture/recipe/schedule creation,
serialization and publication; GNU time RSS includes the entire process. The earlier preliminary
measurement directory is retained separately; final results follow the last fallible prefix-allocation
change. Host scheduling/caches/frequency are uncontrolled, and compiler/testing/fuzz activity ran
concurrently. These are preliminary fixture measurements, not isolated throughput, universal
admitted-plan bounds or comparisons with commercial slicers. Native fixture export validates behavior,
not repeated native performance. Exact executable/environment hashes and complete reports are in
`/tmp/karasu-m55c-bench-final`.

## Fuzz and remaining acceptance

The bounded final-source Linux ASan campaign passes **204 executions in 21 reported seconds**,
peak RSS **76 MiB**, exit zero and no failure artifacts. Seed 5504, 256-byte maximum inputs,
1024 MiB RSS limit and 10-second per-input timeout are recorded. All core/harness/dependency hashes
match preflight. Fixed replay passes 15 inputs (seven accepted/eight expected rejections); final
retained corpus replay passes 30 (nine accepted/21 rejected). Typed rejection is expected.
Fuzz formatting, strict all-target Clippy and focused seed tests pass. See
[fuzz documentation](FUZZ-CAMPAIGNS.md) and `/tmp/karasu-m55c-fuzz/` for hashes, corpus and logs.
Leak detection is disabled for the established host ptrace limitation; no leak-clean or native
Windows sanitizer claim is made. This short campaign is not sustained arbitrary-geometry fuzzing.

M5.5-D manual support geometry and M5.5-E combined prepared-scene M3/M4/M5 acceptance remain required.
M6 supplies interactive placement/editing, M7 end-to-end/hardware acceptance. C's limited backend
must not be represented as arbitrary-mesh hollowing/cutting, an exportable Boolean triangle mesh,
a universal cross-platform bitwise trigonometric geometry guarantee or physical drainage validation.
