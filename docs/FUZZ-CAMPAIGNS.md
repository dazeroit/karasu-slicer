# Fuzzing: target protocols and campaign records

Moved from `fuzz/README.md` on 2026-11-10 (owner): the folder's README says how to run the
fuzz targets; this document keeps, unchanged, each target's protocol and the record of every
bounded campaign (dates, revisions, seeds, executions, peak memory, replays and limits).
Commands below run from the repository root unless they say otherwise.

## M5.5-B bounded convex hollowing target

`hollow_mesh` decodes at most 2,048 bytes into either an independently authored cube/tetrahedron
with small nonuniform positive scaling and translation, malformed variants, or at most 16 raw
vertices and 32 indexed triangles. Raw coordinates include arbitrary binary64 values, nonfinite
values, subnormals and finite extremes. Malformed indices and invalid/nonpositive thickness are
expected rejections. Hollowing admission uses a 200,000-unit work cap, 2 MiB cumulative requested
payload cap, 256 output vertices, 512 output triangles and a requested erosion-bracket error of
0.00001 mm. The source digest in generated plans remains a harness assertion, as in the public API.

The oracle checks source/plan immutability on both success and typed failure. Successful results
must bind the exact plan record, retain the placed exterior, contain two closed combinatorial
surfaces and an independently positive oriented cavity, and provide finite positive volume bounds
and an erosion-bracket error no larger than requested. An independently accumulated signed-volume
identity checks exterior minus cavity against shell material. The harness does not reproduce the
clipping/hull algorithm or claim arbitrary mesh support. Ordinary replay is available through the
`hollow_mesh` mode of `tools/replay.rs`; its counts are not coverage-guided fuzz executions.

The 2026-10-17 Linux nightly ASan campaign, seed 5502, passes **501 executions in 21 reported
seconds**, 98 MiB peak RSS, exit 0 and zero failure artifacts. During that run only the
`preparation.rs` module documentation header changed; all hollowing and harness hashes remained
unchanged. Its complete logs and preflight hashes remain in `/tmp/karasu-m55b-fuzz/`.
A final-source follow-up, seed 5503, passes **469 executions in 21 reported seconds**, 96 MiB peak
RSS, exit 0 and zero failure artifacts; its records are retained in the `final/` subdirectory.
All core/harness/dependency hashes match that follow-up's preflight record. Final ordinary replay
passes 12 fixed seeds (4 accepted, 8 expected rejections) and 177 retained inputs (112 accepted,
65 expected rejections). The fuzz workspace also passes formatting, strict all-target Clippy and
the focused seed unit test.
Both runs request 20 seconds, 1,024 MiB RSS and a 10-second per-input timeout. Leak detection is
disabled for the previously established host ptrace restriction; address instrumentation is active,
but leak validation and native Windows fuzzing are not claimed. These short bounded campaigns do
not establish defect absence or exhaustive numeric/topological coverage.

```console
CARGO_NET_OFFLINE=true ASAN_OPTIONS=detect_leaks=0 cargo +nightly fuzz run hollow_mesh /tmp/karasu-m55b-fuzz/final/corpus --target-dir /tmp/karasu-m55b-fuzz/target -- -max_total_time=20 -max_len=2048 -rss_limit_mb=1024 -timeout=10 -seed=5503 -print_final_stats=1 -artifact_prefix=/tmp/karasu-m55b-fuzz/final/artifacts/
```

## M5.5-D bounded parametric support target

`supports` accepts at most 256 bytes and exercises a separately authored 8 mm cube elevated
4 mm above the platform, either solid or hollowed by 1 mm. Ordered rectangle, elliptical polygon,
regular polygon, custom convex polygon and terminal point sections vary center positions and
three-axis orientation. It includes nonfinite/invalid centers, forbidden intermediate point
sections, unsupported profile counts, stale anchors, invalid contact/penetration, publication
cancellation and explicit reject/union intersection policies for two overlapping supports.

Per-segment limits are 200,000 charged work units and 2 MiB requested capacity; upstream drainage
has independent 100,000-unit/4 MiB limits. The support admission allowance is 1,200,000 units and
16 MiB for one recipe, or 2,000,000 units and 24 MiB for the two-recipe case. Plan sizes and exact
planar construction have additional small independent caps. The oracle checks immutable source
geometry and canonical upstream/support bytes on success and failure, finite bounded contact
certificates on success, and a real prepared-material section below the model. A successful
section must retain additive support material when the body is absent; typed section failures are
permitted under the independent small construction budget. The oracle does not replicate the
convex-hull, contact or planar arrangement algorithms and makes no mechanical strength claim.
Ordinary seed replay covers 33 fixed inputs: 13 accepted and 20 expected rejections. Replay and
unit-test counts are distinct from coverage-guided executions.

The final-source 2026-10-18 Linux nightly ASan campaign, seed 5505, passes **103 executions in
21 reported seconds**, 68 MiB peak RSS, exit 0 and zero failure artifacts. Every recorded application
Rust, test/example, harness and Cargo manifest/lockfile hash matches the postflight record. Final
ordinary replay passes the 33 fixed inputs (13 accepted, 20 expected rejections) and all 40 retained
inputs (20 accepted, 20 expected rejections). The
fuzz workspace passes formatting, strict all-target Clippy and its focused seed unit test.
Evidence, complete logs, fixed-seed hashes and preflight/postflight records remain in
`/tmp/karasu-m55d-fuzz/`. This short bounded run requests 20 seconds, 1,024 MiB RSS and a
10-second per-input timeout. Address instrumentation is enabled; leak detection is disabled for
the established host ptrace limitation. Leak validation and native Windows fuzzing are not claimed,
and the short campaign does not establish absence of defects or exhaustive geometric coverage.

```console
CARGO_NET_OFFLINE=true ASAN_OPTIONS=detect_leaks=0 cargo +nightly fuzz run supports /tmp/karasu-m55d-fuzz/corpus --target-dir /tmp/karasu-m55d-fuzz/target -- -max_total_time=20 -max_len=256 -rss_limit_mb=1024 -timeout=10 -seed=5505 -print_final_stats=1 -artifact_prefix=/tmp/karasu-m55d-fuzz/artifacts/
```


## M5.5-C bounded drainage target

`drainage` accepts at most 256 bytes and exercises a fixed independently authored 8 mm cube,
1 mm hollowing, up to two finite cardinal cuts and one actual prepared material-difference layer.
It varies all six cardinal directions, entry position, length and section height, and includes
shallow, missed, noncardinal, nonfinite, oversized, overlapping, cancellation and resource cases.
Construction admits at most 64 sides per cut, 200,000 charged work units and a 4 MiB aggregate
requested allocation allowance; subordinate hollowing and planar construction have independent
small limits. Typed geometry, numeric, unsupported and resource failures are expected outcomes.

The oracle checks immutable source geometry and canonical plan bytes on success and failure;
successful scenes retain the original plan, finite bounded combined radial/axial errors, strict
terminal clearance and the documented single-cavity enclosure state. A bounded layer evaluation
must either return its exact material result or a typed stream failure. It is not an independent
reimplementation of the Boolean algorithm. Fixed ordinary replay covers 15 inputs: seven accepted
(six directions and an uncut enclosed cavity) and eight expected failures. Ordinary replay and
unit-test counts are not coverage-guided fuzz executions.

The final-source 2026-10-17 Linux nightly ASan campaign, seed 5504, passes **204 executions in
21 reported seconds**, 76 MiB peak RSS, exit 0 and zero failure artifacts. All core Rust, harness
Rust and Cargo manifest/lockfile hashes match the preflight record. Final ordinary replay passes
15 fixed inputs (7 accepted, 8 expected rejections) and all 30 retained corpus inputs (9 accepted,
21 expected rejections). Evidence, seed manifest, complete campaign/replay logs and preflight hashes
remain in `/tmp/karasu-m55c-fuzz/`. This campaign requests 20 seconds, a 1,024 MiB RSS limit and a
10-second per-input timeout. Leak detection is disabled for the established host ptrace restriction;
address instrumentation is active, but leak validation and native Windows fuzzing are not claimed.
This bounded short campaign is regression evidence, not exhaustive geometric or numeric coverage.

```console
CARGO_NET_OFFLINE=true ASAN_OPTIONS=detect_leaks=0 cargo +nightly fuzz run drainage /tmp/karasu-m55c-fuzz/corpus --target-dir /tmp/karasu-m55c-fuzz/target -- -max_total_time=20 -max_len=256 -rss_limit_mb=1024 -timeout=10 -seed=5504 -print_final_stats=1 -artifact_prefix=/tmp/karasu-m55c-fuzz/artifacts/
```

## M5 bounded profile JSON target

`profile_json` passes at most 64 KiB of untrusted bytes to the typed, versioned public profile
decoder. Accepted profiles must validate, serialize deterministically, reload with identical physical
values, and serialize identically again. It performs no filesystem operations. Structured parser,
schema and range errors are expected outcomes. Serde JSON `float_roundtrip` is enabled to preserve
binary64 calibration values exactly across portable profile transfers.

The initial seed corpus is independently authored printer/resin JSON plus malformed schema,
duplicate/unknown fields, excessive nesting and invalid UTF-8. External evidence lives in
`/tmp/karasu-m5-profile-fuzz/`. On 2026-10-17, the actual nightly ASan coverage-guided campaign
passes **2,660,659 executions in 26 reported seconds** for the requested 25-second limit, seed 503,
527 MiB peak RSS, exit 0 and zero failure artifacts. Exact final-harness replay passes all 10 fixed
seeds (2 accepted / 8 expected rejections) and 2,638 retained corpus files (62 accepted / 2,576
expected rejections). Profile/parser/harness/dependency source hashes are unchanged from preflight.
Earlier PP1 integration build failures are retained separately and do not count as fuzz executions.
Leak detection remains disabled because of the host's previously established ptrace restriction;
ASan instrumentation remains enabled. Native Windows fuzzing and leak validation were not performed.
A short bounded campaign does not establish complete coverage or absence of defects.

```console
CARGO_NET_OFFLINE=true ASAN_OPTIONS=detect_leaks=0 cargo +nightly fuzz run profile_json /tmp/karasu-m5-profile-fuzz/corpus --target-dir /tmp/karasu-m5-profile-fuzz/target -- -max_total_time=25 -max_len=65536 -rss_limit_mb=1024 -timeout=5 -seed=503 -print_final_stats=1 -artifact_prefix=/tmp/karasu-m5-profile-fuzz/artifacts/
```

## M3.6 bounded owned-layer engine target

`layer_engine` exercises the complete strict pipeline over tiny generated meshes (at most 16 faces)
and explicit schedules of at most four planes. Cases include random indexed triangles, duplicates,
closed ribbons, coplanar faces, tangents and open/contacting paths. Reviewed per-stage caps and a
1 MiB owned-output cap bound generated work; off-grid and other typed operational errors are
expected outcomes. The oracle compares deterministic reruns, direct iteration against atomic
collection, source preservation, raw/flat reference conservation and individually finite contiguous
planes. A cancellation mode checks pre-publication failure and fused iteration.

Seed and retained-corpus replay use the `layer_engine` mode in `tools/replay.rs`. Replays are ordinary
deterministic executions, not coverage-guided evidence. Current command/seed/source/campaign records
are outside Git in `/tmp/resin-m36-fuzz/`; the
[test record](TEST-STRATEGY.md#implemented-m36-owned-layer-engine-coverage) reports results
and limitations. The target belongs to the existing isolated fuzz workspace and adds no production
dependency or foreign geometry implementation. Full-M3 region/large-input/platform campaigns remain
separate acceptance work.

On 2026-10-17 the final current-source ASan campaign passes 7,997 executions in 21 reported
seconds, seed 20261018, 431 MiB peak RSS and zero artifacts. An earlier run passed 8,730 executions;
the final run strengthens unresolved/summary/reference invariants and includes the corrected
containment code. Final replay passes 128 fixed seeds and 1,093 retained corpus inputs. Both
campaigns disable leak detection (`ASAN_OPTIONS=detect_leaks=0`) because this host cannot provide
LeakSanitizer's ptrace operation; address instrumentation and debug assertions remain enabled.
No leak, native Windows, large-input performance or whole-M3 acceptance claim follows from this
short bounded campaign.

## M3.5-B bounded layer-schedule target

The isolated workspace also includes `layer_schedule`; the existing four mesh targets and their
corpora are unchanged. `src/schedule.rs` decodes raw binary64 Count/Before/Through, zero-first
Count and bounded dyadic terminal modes under a 64-plane output cap. It checks deterministic
reruns, finite strictly increasing planes, contiguous indices, unchanged nominal height,
independent native-FMA rounding for small exact indices, integer terminal membership and cap errors.
`tools/replay.rs` accepts `layer_schedule` for uninstrumented replay, which is not coverage-guided
validation by itself. No production dependency or foreign geometry implementation is added.

On 2026-10-17, offline nightly ASan fuzzing with seed 3502 passes 1,256,859 executions in 21
reported seconds for a requested 20-second cap; exit 0, 419 MB peak RSS, no failure artifacts.
Final replay passes 712 corpus files (572 accepted / 140 expected rejections). The first campaign
completed 1,222,599 runs but exited 1 on LeakSanitizer's unavailable ptrace operation during
teardown; its empty artifact replays normally. The successful run sets
`ASAN_OPTIONS=detect_leaks=0`, retaining ASan instrumentation without claiming leak validation.
Logs, commands, source/seed hashes and corpus are in `/tmp/resin-m35b-fuzz/`.

Reproduce in a caller-created external corpus/artifact directory with the already installed
nightly toolchain and isolated cached dependencies:

```console
CARGO_NET_OFFLINE=true ASAN_OPTIONS=detect_leaks=0 cargo +nightly fuzz run layer_schedule /tmp/resin-m35b-fuzz/corpus --target-dir /tmp/resin-m35b-fuzz/target -- -max_total_time=20 -max_len=40 -rss_limit_mb=1024 -timeout=5 -seed=3502 -print_final_stats=1 -artifact_prefix=/tmp/resin-m35b-fuzz/artifacts-no-leaks/
```

The full [test record](TEST-STRATEGY.md#implemented-m35-b-uniform-schedule-coverage) retains
the independent Fraction oracle and stable/Release gates. This bounded pass does not close M3,
establish Windows behavior, demonstrate a speed improvement or prove defect absence.

This independent development workspace now defines six libFuzzer targets over public `karasu-core`
APIs: the four original mesh targets documented below and the schedule/engine targets above. It is excluded
from ordinary workspace builds by its own `[workspace]`; root manifests,
lockfile, stable defaults, production dependencies, and geometry implementations are unchanged.

## Resumed campaign: PASS for the initial bounded scope

On 2026-10-15, all four existing targets compiled and completed coverage-guided libFuzzer runs
under WSL/Linux with AddressSanitizer and debug assertions. HEAD remains
`3c6ed2e84888b98194a564786f26228acd9783a7`. The resumed checkout contained the 13 expected
uncommitted preparation files; these were preserved. No production code, oracle assertion,
canonical fixture, or production dependency required correction.

Versions: rustc 1.101.0-nightly (`7df81381d`, 2026-10-14), Cargo nightly 1.101.0-nightly
(`3960fa69b`, 2026-10-12), LLVM 23.1.3, cargo-fuzz 0.13.2, libfuzzer-sys 0.4.13, and GCC C++
13.3.0 on `x86_64-unknown-linux-gnu`. Stable Rust/Cargo 1.99.0 remains the default. The owner
installed the tools; the agent downloaded only the approved isolated Cargo testing dependencies.

New evidence, separate from the unchanged preparation archive:

`<evidence>/m2/m2-4b/20261015-3c6ed2e-resume-R4uSnn/`

| Target | Executions | Coverage INITED → DONE | Features INITED → DONE | Active corpus INITED → DONE | Reported seconds | Peak RSS MB |
| --- | ---: | --- | --- | --- | ---: | ---: |
| binary_stl | 453,022 | 414 → 456 | 3,190 → 5,886 | 18 → 800 | 121 | 432 |
| ascii_stl | 5,377,414 | 530 → 672 | 3,342 → 7,956 | 17 → 1,829 | 121 | 468 |
| auto_stl | 359,506 | 660 → 772 | 4,937 → 9,043 | 35 → 1,456 | 121 | 469 |
| topology | 36,177 | 2,583 → 2,812 | 13,525 → 29,386 | 22 → 1,200 | 121 | 497 |

All builds and campaigns exited 0: 6,226,119 total executions. No sanitizer finding, panic,
crash, timeout, resource-limit failure, watchdog, unsupported option, or failure artifact was
reported. No production regression test or fix was needed. Coverage/feature counters belong to
each instrumented target; they are not whole-program percentages or interchangeable across binaries.
ASan and libFuzzer runtime/coverage symbols were recorded from all four executables.

Each command used `-max_total_time=120`; libFuzzer reported 121 seconds at its integer-second
termination check. Wrapper times including any rebuild were 126.509/122.124/123.649/123.679 seconds.
Initial seed directories contain 19/18/38/22 files; the active `INITED`/`DONE` corpus reflects
libFuzzer's selection/reduction and can differ from raw filesystem counts. New discovery directories
ended with 779/1,780/1,410/1,165 files (13,024,661/1,798,997/11,612,285/3,109,835 bytes). All 97
seed copies retained their hashes. RSS is resource-gate telemetry, not M2.4-C profiling or a
controlled performance comparison.

`instrumented-builds.json`, `campaign.json`, and `logs/` preserve exact commands, exits, corpus
metrics, runtime/statistics, and outputs. Tool versions/source hashes are in `prerequisites.json`.
Cargo/libFuzzer help logs and `option-verification.json` establish supported options. The resolved
testing lockfile, Linux dependency metadata, source/seed provenance, final report, and artifact hash
index are retained. `stable-workspace/` records passing format, strict Clippy, tests/build,
warnings-denied rustdoc, and whitespace checks. Tests remain 156 passed, 0 failed, 2 ignored.

Resumption changed only infrastructure reporting: the existing runner now records requested corpus,
runtime, failure, and nightly-version details; the preparation checker uses
`NOT_PERFORMED_BY_PREPARATION_CHECK` instead of its obsolete hardcoded missing-tool label. No
assertions were weakened. An unfiltered offline metadata query requested an uncached non-Linux
package; Linux-filtered metadata succeeded offline. Both outputs are retained. This was a query
issue rather than a target build or engine failure.

PASS applies to the initial bounded campaign. Longer runs, more input/coordinate distributions,
retained-corpus minimization/replay, and continued regression monitoring remain necessary.
Arbitrary `f64`/transform fuzzing, Windows fuzzing, and M2.4-C profiling were not performed.
A short clean campaign does not establish absence of defects or complete coverage.

## Historical preparation: execution was initially blocked

On 2026-10-15, baseline `3c6ed2e84888b98194a564786f26228acd9783a7` on `main` was clean.
WSL provides stable Rust/Cargo 1.99.0 for `x86_64-unknown-linux-gnu`, GCC C++ 13.3.0, and Python
3.12.3. At that earlier stage there was no nightly toolchain or `cargo-fuzz` executable. Clang++
was also absent, but GCC met the C++ prerequisite. The libFuzzer build had not been verified and
no tools or dependencies had been downloaded/installed during preparation.

The archived outcome was therefore **BLOCKED**, with zero coverage-guided executions and no
coverage or sanitizer results. It remains unchanged as historical evidence; current acceptance
uses the resumed campaign above. Ordinary seed replay alone never established fuzzing validation.

External preparation evidence:

`<evidence>/m2/m2-4b/20261015-3c6ed2e-preparation-yZvuTT/`

`prerequisites.json` records the initial tool probe; `final-preflight/prerequisites.json` records
the final runner's command results, available memory, and source/harness hashes. Both associated
`campaign.json` files record the blocker. `seed-manifest.json` identifies every seed's source, size,
and SHA-256.
`preparation-check/commands.json` and per-command logs record the separate stable preparation
checks and ordinary workspace gates. Generated inputs, builds, logs, and failures stay outside Git.

## Targets and assertions

| Target | Public API | Main checks |
| --- | --- | --- |
| `binary_stl` | `import_stl_bytes`, explicit `StlFormat::Binary` | finite vertices, index ranges, counts, exact source-bound extrema, executable diagnostics and metadata alignment |
| `ascii_stl` | `import_stl_bytes`, explicit `StlFormat::Ascii` | same successful-import checks; strict grammar/numeric failures may return structured errors |
| `auto_stl` | `import_stl_bytes`, `StlFormat::Auto` | same checks plus agreement with only the actual successfully selected representation |
| `topology` | `TriangleMesh::try_new`, `analyze_topology_with_limits` | edge multiplicities and directed source references; complete valid component memberships; ordered repeatability; unchanged vertices/triangles/bounds/diagnostics |

The principal parser receives the entire input, without consuming a control prefix. A second
attempt uses tighter caps derived from input words to reach zero/exact-size/resource boundaries.
Non-finite stored normals are legal metadata: comparisons accept corresponding NaNs rather than
incorrectly require reflexive floating-point equality. No imported mesh is assumed manifold.

Topology uses a documented synthetic encoding, not STL bytes:

1. Little-endian `u16` counts: V modulo 513 and F modulo 1025; V=0 implies F=0.
2. V triples of little-endian `i16`, converted exactly to `f64` positions by division by 16 mm.
3. F triples of little-endian `u16`, each reduced modulo V for valid source indices.

Missing bytes read as zero. At most 9,220 bytes are consumed; any remaining suffix is unused.
Positions lie between -2048 and 2047.9375 mm. This deliberately bounds geometry, indices, and work
while permitting repeated indices, coincident manual vertices, duplicates, branches, conflicts,
isolates, and excluded faces. It does not exercise arbitrary floating-point topology coordinates,
large production resource caps, or transformed/singular geometry. The oracle checks public
invariants without reproducing component or vertex-link classification algorithms. Synthetic
source meshes have no STL metadata; parser targets separately check retained file metadata.

## Seeds and preparation

`tools/prepare_corpus.py` verifies all 11 committed small STL hashes against the authoritative
manifest before copying any input. FX-11 is never read or regenerated. It uses fresh external seed
directories and refuses to overwrite an existing seed set. Corpus outputs cannot be inside this
repository or an ancestor. Keep each preparation/campaign in a new external directory.

- Binary: seven unchanged FX-01/02/05/08/09/10A/10B copies plus 12 synthetic boundary/normal/
  attribute inputs: 19 files.
- ASCII: six independently encoded fixture counterparts (omitting the larger cylinder encoding)
  plus 12 syntax/numeric/whitespace inputs: 18 files.
- Auto: seven binary and six ASCII fixture counterparts, both synthetic groups, and one ambiguous
  malformed header: 38 files. Includes `solid` binary headers, empty/short/truncated input, and
  valid/invalid text.
- Topology: seven encoded fixture counterparts plus 15 synthetic cases: 22 files. These include
  disconnected tetrahedra, vertex-only contacts, a bow tie, collinearity, duplicate/reversed faces,
  a 128-face high-incidence edge, and maximum-count zero-padded input. Fixture coordinates are
  quantized to the topology encoding; these are documented derived seeds, not canonical STL copies.

Seeds include valid complete parser structures so replay reaches beyond the opener; long-line,
long-token, scientific, overflow, non-finite vertex/normal, truncation, and trailing-data cases are
separate. The seed manifest retains both provenance and exact bytes by SHA-256. Seed directories
remain separate from generated mutation corpora and failure artifacts.

From the repository root, substitute a fresh absolute external directory for `$run_dir`:

```bash
run_dir=<evidence>/m2/m2-4b/NEW-RUN
python3 -B fuzz/tools/prepare_corpus.py --output "$run_dir"
python3 -B fuzz/tools/check_preparation.py --output "$run_dir"
```

`check_preparation.py` runs ordinary stable workspace gates, compiles the shared oracle with
`rustc`, lints it with `clippy-driver -W clippy::all -D warnings`, and replays each external seed
directory through that exact oracle. It needs no fuzz dependency or nightly instrumentation.
This check is ordinary fixed-input validation and must never be reported as coverage-guided fuzzing.
On the initial preparation, all 97 seeds passed assertions: binary 12 accepted/7 rejected;
ASCII 10/8; auto 22/16; topology 22 constructor/analyzer checks. This found no production defect.

## Bounded campaign after prerequisites are provided

The task authorizes no tool installation. Once an owner-provided nightly and `cargo-fuzz` exist,
select the installed dated nightly explicitly without changing the stable default. On a fresh
prepared run directory:

```bash
python3 -B fuzz/tools/run_campaign.py --output "$run_dir" --toolchain nightly-YYYY-MM-DD
```

`run_campaign.py` checks installed names before invoking nightly, records the selected toolchain
and command versions, and exits 2 with `BLOCKED` when prerequisites are absent. A named toolchain
must already be listed by rustup, preventing implicit installation by `cargo +...`. It preserves
the first campaign record rather than overwriting a prior result. It is WSL/Linux tooling.

The resumed campaign executed this command form for each target, using resolved absolute paths and
`nightly-x86_64-unknown-linux-gnu`. `campaign.json` retains the exact argument arrays:

```bash
cargo +INSTALLED_NIGHTLY fuzz run TARGET \
  --fuzz-dir /develop/dazero/karasu-slicer/fuzz \
  --target-dir "$run_dir/target" --sanitizer address --debug-assertions \
  "$run_dir/corpus/TARGET" "$run_dir/seeds/TARGET" -- \
  -max_len=65536 -timeout=3 -rss_limit_mb=1024 -malloc_limit_mb=256 \
  -max_total_time=120 -seed=20261015 -print_final_stats=1 -use_value_profile=1 \
  -artifact_prefix="$run_dir/artifacts/TARGET/"
```

Targets execute sequentially; seeds are 20261015/20261016/20261017/20261018 in binary/ASCII/auto/
topology order. The first corpus directory receives discoveries; the second supplies preserved
seeds. The 120-second libFuzzer budget applies to target execution, with a separate 900-second
watchdog including compilation. Parser caps are 64 KiB input, 1,024 triangles, 3,072 vertices,
4,096 bytes/line and 128 bytes/numeric token. Topology caps are 512 vertices/1,024 faces. The planned
RSS cap is 1,024 MB and maximum single allocation cap 256 MB, with 3 seconds per input. These are
configured limits; the table above separately reports observed RSS. ASan virtual address reservation is not constrained
with `ulimit -v`. Do not raise resource limits merely to conceal a slow or memory-heavy input.

Record and inspect the installed tool's `run --help` and libFuzzer flags when resuming; options
may vary by version. The runner preserves logs and exit codes, copies the resolved fuzz lockfile
to evidence, and records execution/coverage values only when actually reported. It requires a
libFuzzer `DONE` record, execution statistics, successful exit, and no failure artifacts before
marking a bounded target run complete. Review the complete results before acceptance.
No percentages, whole-program completeness, or Windows fuzzing claims follow from these counters.

## Dependency boundary and reproducibility

The owner-approved development-only `libfuzzer-sys = "0.4"` follows the official cargo-fuzz
template. Its LLVM C++ fuzzing runtime/build support belongs exclusively to the isolated testing
package; no foreign geometry kernel enters the Rust production core. Public target/oracle code
contains no unsafe Rust. The resumed Linux tree resolved libfuzzer-sys 0.4.13, arbitrary 1.5.0,
cc 1.8.0, find-msvc-tools 0.1.14, jobserver 0.1.35, libc 0.2.190, and shlex 2.0.1. Exact checksums
and additional unbuilt platform dependencies are in the saved `fuzz-Cargo.lock`; third-party source
is available in the registry cache. libfuzzer-sys builds the C++ testing runtime; libc has a
configuration build script, and cc/jobserver/find-msvc-tools/shlex support the testing build. None
is a production geometry kernel or a dependency of `karasu-core`. Third-party packages declare
MIT OR Apache-2.0, with libfuzzer-sys additionally declaring NCSA. This records dependency notices
without choosing the project's distribution license. Linux build/runtime correctness was exercised;
Windows fuzz-runtime support, sustained robustness, comparative performance, and an advisory scan
are not established. The dependency tree and `resolved-testing-dependencies.json` preserve exact
release metadata and build-script paths. Root Cargo files remain unchanged.

Repeatability requires the same source/harness hashes, dated toolchain, cargo-fuzz and compiler
versions, test dependency lockfile, seed hashes, limits, and random seed. A wall-clock budget can
produce different execution counts/discoveries; preserve generated corpora as well as initial seeds.
The safety defaults, assertions, and proposed limits do not prove absence of production defects.

## Failures, minimization, and regression

The runner stops at the first failed/incomplete target, preserving the complete command, output,
exit status, watchdog state, seeds, and artifacts. A process failure with no libFuzzer report is
an environment/build failure until investigated, not automatically a production crash. Original
inputs must never be overwritten by minimized copies. Hash and preserve them before replay.

After the same nightly/runtime build is available, reproduce one artifact without mutation:

```bash
cargo +INSTALLED_NIGHTLY fuzz run TARGET \
  --fuzz-dir fuzz --target-dir "$run_dir/target" --sanitizer address --debug-assertions \
  /absolute/path/to/preserved-artifact -- -runs=1 -timeout=3 -rss_limit_mb=1024
```

For bounded minimization using libFuzzer's crash minimizer, select a new output name:

```bash
cargo +INSTALLED_NIGHTLY fuzz run TARGET \
  --fuzz-dir fuzz --target-dir "$run_dir/target" --sanitizer address --debug-assertions \
  /absolute/path/to/preserved-artifact -- -minimize_crash=1 -max_total_time=60 \
  -timeout=3 -rss_limit_mb=1024 \
  -exact_artifact_path="$run_dir/artifacts/TARGET/minimized-INPUT_SHA"
```

Check the installed version's options. Preserve output beside, not over, the original.
The independently compiled `preparation-check/replay TARGET INPUT_FILE` also reproduces oracle
assertions outside the mutation loop, but does not replace sanitizer reproduction.

Classify a failure as a production defect, harness error, or environment failure from a reproducible
case. Add a minimal deterministic regression and narrowly justified production correction only for
a confirmed production defect; rerun that regression and the affected target. Never suppress an
assertion or discard an artifact to obtain a passing run. None of these failure workflows has been
needed for production code during preparation or the resumed bounded campaigns. Sustained campaigns, more
coordinate distributions, and longer adversarial exploration remain future work; M2.4-C profiling
is outside this task.

Protocol references (checked 2026-10-15): [Rust Fuzz Book setup](https://rust-fuzz.github.io/book/cargo-fuzz/setup.html),
[cargo-fuzz templates](https://github.com/rust-fuzz/cargo-fuzz/blob/main/src/templates.rs),
[cargo-fuzz run options](https://github.com/rust-fuzz/cargo-fuzz/blob/main/src/options/run.rs),
[libFuzzer options and metrics](https://llvm.org/docs/LibFuzzer.html).

## Remaining M3 constructive and strict-region campaigns

`construct_material` decodes two to four small simple rectangles/triangles with explicit material
labels. It checks independent off-boundary occupancy (parity per material, union across materials),
exact-result determinism, source-edge support indices, result references, resource failures and
cancellation. Its floating occupancy oracle uses only small values and excludes probes within
1e-9 of an edge; it is not an arbitrary-coordinate exact arithmetic oracle.

`strict_regions` builds two to six nested, disconnected or contacting ribbons at exact dyadic
planes. It compares the owned engine's region output with direct reviewed stage composition,
checks analytic depth/role parity, explicit unresolved contacts and provenance preservation.
Both targets have ordinary `tools/replay.rs` modes, which do not establish coverage-guided fuzzing.

The final 2026-10-17 ASan campaigns retain source hashes and logs outside Git at
`/tmp/resin-m3-completion-fuzz/`. Production geometry and both oracle hashes are unchanged across
these campaigns. With `ASAN_OPTIONS=detect_leaks=0`, `construct_material` passes 6,713 executions in
21 reported seconds (seed 20261019, 406 MiB peak RSS), and `strict_regions` passes 1,796 executions
in 21 reported seconds (seed 20261020, 387 MiB peak RSS). Both produce zero failure artifacts.
Final ordinary replay accepts all 425 retained constructor inputs and all 153 region inputs.
The fuzz workspace passes offline strict all-target Clippy and formatting.

Leak validation remains unavailable because this host rejects LeakSanitizer's ptrace operation.
An earlier 2,000-run rectangle-only precursor completed its geometry oracle but failed during
LeakSanitizer teardown; its empty artifact is environmental evidence, not a constructor regression.
Final campaigns exclude that precursor's target and use the dedicated targets above. One replay
started during corpus mutation and encountered a disappearing file; final replay after campaign
exhaustion passes. These bounded studies neither prove defect absence, arbitrary arrangement
coverage, native Windows behavior nor whole-M3 acceptance.

## TD-031 represented-coordinate region coverage

`strict_regions` now selects exact-grid or represented-coordinate mode using the fourth input
byte. Represented fixtures translate XY by 0.1 mm and use quarter-spaced planes; nesting,
disconnection/contact reporting, deterministic reruns, low-level/owned projection equivalence and
source provenance remain checked. No production or fuzz dependency was added.

On 2026-10-17 an offline nightly ASan run with seed 351031 and 20 requested seconds passes
**2,014 executions in 21 reported seconds**, 390 MiB peak RSS, with no failure artifacts.
Source, corpus, target and logs remain in `/tmp/resin-m3-represented-fuzz/`. This host requires
`ASAN_OPTIONS=detect_leaks=0`; address instrumentation is active but leak validation is not claimed.
The campaign is bounded, Linux-only evidence and does not prove defect absence.


## M3 closure campaign

The [closure record](M3-CLOSURE-ACCEPTANCE.md) reports the extended and exact-final-source
ASan campaigns and their source boundaries. Extended runs total 2,947,478 executions over the four
M3 targets; exact-final-source follow-ups total 926,854, with zero failure artifacts. The final
stable-library replay passes 687 constructor, 66 region, 1,021 engine and 627 schedule inputs.
Expected typed rejections are retained and counted, not fuzz failures. Campaigns use 1,536 MiB RSS
and 10-second per-input limits, fixed recorded seeds and `ASAN_OPTIONS=detect_leaks=0`; leak coverage
is not claimed. Logs/corpora/hashes are external in `/tmp/resin-m3-closure/`. These bounded campaigns
satisfy the preliminary M3 campaign scope; sustained/adversarial expansion remains maintenance.

## Combined preparation session lifecycle target

`preparation_session` accepts at most 256 bytes. Its owned immutable 8 mm cube is
placed 4 mm above the platform, hollowed by 1 mm, bottom-drained and supported by
four rectangular stations. The harness uses synthetic caller-asserted source
identity and never treats it as verified asset-byte correspondence. Inputs vary
cut diameter, layer selection and explicit lifecycle families: invalid/stale
recipes, cancelled evaluation/publication/edit, foreign outcomes, newer worker
requests, A-to-B-to-A revision changes, undo/redo, failed geometry budgets and
retained-preview versus current-only access. Only accepted current geometry may
produce a material layer. Fixed source coordinates, facets and original plan
bytes must remain unchanged. No opaque outcome is constructed or inspected
outside its public API.

The per-input bounds include 1.5 million requested geometry work units, 24 MiB
requested geometry capacity, three history snapshots and 64 KiB history capacity.
Component and drainage bounds remain independently enforced; these are not CPU
or RSS guarantees. A fixed 33-input replay covers 27 successful lifecycle paths
and six intentional rejections. Ordinary replay is not coverage-guided fuzzing.
Campaign evidence and source epoch hashes are stored outside Git under
`/tmp/karasu-m55e-fuzz/`. ASan campaigns disable leak detection for the host's
ptrace limitation; no leak-clean or native Windows fuzzing claim follows.

Final-source Linux nightly ASan runs on 2026-10-18 pass:

| Target | Seed | Executions | Reported seconds | Peak RSS MiB | Final replay accepted / rejected |
| --- | --- | ---: | ---: | ---: | ---: |
| `preparation_session` | 5506 | 184 | 62 | 111 | 38 / 12 |
| `hollow_mesh` | 5507 | 932 | 31 | 108 | 27 / 40 |
| `drainage` | 5508 | 300 | 31 | 93 | 11 / 20 |
| `supports` | 5509 | 157 | 31 | 75 | 26 / 21 |

All runs exit zero with zero failure artifacts. Full Rust source, tests,
examples, harness, Cargo manifest/lockfile and owned STL epoch hashes match
before and after every campaign and its final replay. Subsequently, only the
standalone `fuzz/tools/replay.rs` match-arm line wrapping was formatted; it is
not compiled into the instrumented targets. The exact cosmetic boundary and
all successful replays rebuilt from that formatted tool are recorded in
`post-campaign-format-boundary.json`. Production, tests, examples and target
harness hashes remain unchanged. Source records, commands,
complete logs, seed/corpus manifests and final replay results are retained in
`/tmp/karasu-m55e-fuzz/summary.json` and its target subdirectories. Ordinary
final replay checks 195 retained inputs in total. The session fixed seeds pass
27 successful paths and six intentional rejections. The support regression
starts from the available M5.5-D corpus. Historical B/C temporary directories
were absent, so those regressions use fresh documented fixed harness seed
families (12 and 15 inputs); this does not recreate or extend the unavailable
historical corpora. The initial orchestration path failure is preserved in
`orchestration-initial-error.txt` and did not indicate a code failure.

```console
CARGO_NET_OFFLINE=true ASAN_OPTIONS=detect_leaks=0 cargo +nightly fuzz run preparation_session /tmp/karasu-m55e-fuzz/preparation_session/corpus --target-dir /tmp/karasu-m55e-fuzz/target -- -max_total_time=60 -max_len=256 -rss_limit_mb=1024 -timeout=10 -seed=5506 -print_final_stats=1 -artifact_prefix=/tmp/karasu-m55e-fuzz/preparation_session/artifacts/
```

These bounded Linux campaigns provide regression evidence, not exhaustive
coverage, proof of defect absence, leak validation or native Windows fuzzing.
