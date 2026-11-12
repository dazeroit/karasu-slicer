# M3 Closure Acceptance

## Authority, corpus and preliminary budgets

The owner requested completion of the remaining M3 acceptance work on 2026-10-17. This authorizes
selection and execution of a bounded milestone corpus and preliminary engineering budgets; these
are not universal throughput promises or physical printer defaults. The production baseline is
`23d2cd1`, with the local shared-edge correction specified below. No later milestone is started.

The accepted corpus combines all independently owned FX-01 through FX-11 STL files (both FX-10
variants), the existing analytic contour/material fixtures, explicit malformed/ambiguous cases and
quarter-plane decimal regressions. `tests/m3_acceptance.rs` runs valid solids through STL import,
explicit schedules and represented-coordinate owned layers, reruns deterministically, and checks
independent topology/area envelopes. Cube/hollow/disconnected areas are exact; the 128-gon cylinder
uses its analytic polygon area with 0.0001 mm^2 allowance for binary32 fixture coordinates. The
inscribed spheres/cavity use declared ideal-surface envelopes: 2% at subdivision 3, 6% for the
subdivision-2 cavity and 0.1% at subdivision 7. These cover fixture tessellation, not a new kernel
epsilon. The rotated prism must produce a positive finite single-region section inside its area
bound. FX-11 has 327,680 triangles and is sliced at 15, 30 and 45 mm with a 16-million pair safety
cap. These test cases are ordinary automated tests, not ignored performance placeholders.

Defect cases verify unchanged source geometry, complete face accounting and deterministic diagnostic
outcomes at five planes. A valid section of an open/reversed-face mesh is not a global watertightness
or outwardness certificate. Mixed coplanar cap context at endpoint planes remains explicitly
unresolved under the existing conservative policy. Source-labelled crossing/overlap/coincidence,
parity/cavities, exact rational construction, cancellation and resource failures retain their
existing dedicated public tests and fuzz oracles.

Before the large-input measurements, this closure selects the following deliberately preliminary
Linux Release budgets on the documented Ryzen 7 7700 host. Each operation must pass its independent
result checks, take at most **5 seconds**, retain at most **64 MiB** of output, and remain below
**512 MiB whole-process peak RSS** in each of three fresh-process memory runs:

| Worker case | Acceptance workload |
| --- | --- |
| `reference-large` | Full 327,680-triangle FX-11 import/setup, then three owned represented-coordinate sphere layers; expected one region, no holes and 0.1% ideal-area envelope |
| `face-million` | 1,000,008 scanned source faces per plane, three layers; deliberately duplicated nonintersecting faces retained for scan/accounting stress |
| `regions-large` | 1,024 disconnected square regions, 8,192 source faces and three complete layers |
| `construct-large` | Union of 64 independently labelled overlapping rectangles; one connected outer and no holes |

Input setup remains outside timing and inside memory measurement. Timing follows the existing
three-warmup/ten-batch protocol at one repetition. Report min/median/max and compare the maximum
with the budget; memory includes source/setup/working/output/validation, not just retained payload.
The profiling worker uses a two-million face cap and a 40-million validity-pair cap for large disjoint sections; this is
explicit workload policy, not a change to library defaults. The 64-source constructor uses an
explicit 100-million work cap; the default 20-million cap correctly rejects that workload. Counts, resource errors and bounded
constructor limits remain part of acceptance. No spatial/parallel optimization is justified merely
by the presence of a large input. Native Windows functional gates are required separately;
Windows performance and process-memory attribution remain future monitoring rather than an
unfulfilled M3 budget promise.

## TD-032: exact shared-edge crossings found by corpus replay

Initial corpus replay failed the expected resolved result at the equators of FX-03 and FX-11.
TD-019 conservatively treated all noncoplanar on-plane edge pairs as unresolved, including coherent
faces on opposite strict Z sides. This was a genuine missing local classification, not a tolerance
or performance issue. The initial failing log is retained externally.

Accept one `OnPlaneCrossingCandidate` only when exactly two contributions use the same noncollapsed
source edge, source faces are distinct/nonduplicate, ordinary finite coordinate/rank checks pass,
original directed edge uses oppose, and their third vertices have opposite strict M3.1 plane signs.
The intersection of this local two-triangle surface with the plane is exactly the shared edge.
Retain both original occurrences and all provenance; emit one candidate graph edge with raw
multiplicity two. Path validation explicitly accepts that multiplicity only for this ownership.
Downstream degree, contact, area and strict-region checks remain authoritative. Same-side tangency,
conflicting coordinates/winding, duplicate faces, nonmanifold multiplicity and mixed coplanar
context stay unresolved. No source or numerical coordinate is changed and no global solid claim is
inferred. Complexity, storage, caps and cancellation cadence are unchanged.

Tests cover both third-side choices, reversed winding, duplicate/nonmanifold uses, existing source
occurrence conservation and an equatorial octahedron with area exactly 2 mm^2, four contour edges
and all eight source contributions. The sphere corpus reproduces the real-world failure at two
mesh resolutions. Existing permissive single-occurrence test assumptions are updated to require
exactly two supports only for the new named ownership.

## Research boundary

M3 research already compares scheduling, tangency/coplanar context, invalid input and polygon output;
see the retained completion and gap studies. EXP-03/04 and the six commercial runtime
unknowns are preserved in M1. The original completion research explicitly states that EXP-03 does
not block APIs requiring caller-selected first plane/step/terminal rules, and EXP-04 does not
supersede the project's reviewed degeneracy policy. Consequently M3 closure does not require
claiming commercial default equivalence or resolving unrelated M1 research. No proprietary source,
application security policy or installed reference application is modified.

## Execution evidence

Final command results, performance rows, fuzz runs, corpus replay and native source identities follow. Raw evidence is external in `/tmp/resin-m3-closure/`; hashes and
scope distinguish successful final runs from retained initial failures.


### Large-input results

All four cases pass every analytic check, all ten timed batches and all three independent memory
runs. Timing is for the complete operation described above, not one layer. The commands ran
sequentially after coverage-guided campaigns and native test execution ended; CPU affinity and
frequency were not controlled. Fixture import/setup is excluded from timing and included in RSS.

| Case | Min / median / max, ms | Retained bytes | Three peak RSS observations, KiB | Budget |
| --- | ---: | ---: | --- | --- |
| `reference-large` | 102.069 / 102.301 / 103.257 | 2,625,120 | 78228 / 78240 / 78204 | PASS |
| `face-million` | 123.537 / 123.674 / 124.054 | 18,240 | 14208 / 14208 / 14208 | PASS |
| `regions-large` | 427.422 / 427.803 / 428.301 | 14,111,088 | 23100 / 23096 / 23096 | PASS |
| `construct-large` | 104.550 / 104.720 / 104.971 | 598,088 | 3328 / 3200 / 3200 | PASS |

The first face-million attempt correctly hit the library's one-million-face default; the workload
contains 1,000,008 faces, so the worker now states a two-million cap. The first 64-source constructor
attempt correctly hit the default 20-million work cap; its caller budget is explicitly 100 million.
Neither library default nor elapsed-time/RSS acceptance threshold changed. Initial failed runner
logs are retained; the final table reruns every workload with one final worker configuration.
No acceleration or concurrency framework was needed to meet the preliminary budgets.

### Fuzzing and final corpus replay

Extended ASan campaigns run with explicit 10-second per-input timeout and 1,536 MiB process limit.
Leak detection remains unavailable on this host (`ASAN_OPTIONS=detect_leaks=0`); address
instrumentation and assertions are enabled. The extended runs include the local shared-edge
classification before a behavior-preserving helper extraction/documentation update. A second,
exact-final-production-source campaign follows, then complete retained-corpus replay against the
final stable library. No successful partial output, invariant violation, crash, timeout or sanitizer
finding is reported; every artifact directory is empty.

| Target | Extended executions / reported seconds | Exact-final-source executions / reported seconds |
| --- | ---: | ---: |
| `construct_material` | 16,069 / 61 | 5,129 / 21 |
| `strict_regions` | 9,750 / 121 | 1,732 / 21 |
| `layer_engine` | 115,606 / 121 | 11,238 / 21 |
| `layer_schedule` | 2,806,053 / 61 | 908,755 / 21 |

Extended peak RSS is 393–447 MiB; exact-final-source peak RSS is 387–431 MiB. These are ASan fuzz
process observations, not the normal Release performance-budget measurements. The constructor
oracle probes independent occupancy and source support, regions check analytic nesting/contact
policy, the owned engine checks lifecycle/provenance and the schedule checks exact recurrence.
Final ordinary replay passes all 687 constructor, 66 strict-region, 1,021 engine and 627 schedule
inputs (2,401 total). Expected typed engine/schedule rejections are counted separately in raw logs.
Replay is not counted as coverage-guided execution. These bounded campaigns are milestone evidence,
not a proof of defect absence; ongoing adversarial/fuzz monitoring remains future maintenance.

### Linux and native Windows gates

Stable Linux passes format checking, strict all-target Clippy, build, warnings-denied public/private
rustdoc and **611 passed, 0 failed, 2 ignored** workspace tests. Counts include five existing fixture
generator tests reused by the self-contained acceptance harness; they are not five additional core
algorithms. The new acceptance harness has four end-to-end cases plus those five shared checks.

Windows 11 Pro build 26200, stable Rust/Cargo 1.99.0, native `x86_64-pc-windows-msvc`, passes the
**entire Release workspace: 610 passed, 0 failed, 2 ignored**, including CLI processes, cancellation,
new corpus tests and all doctests. The difference is the existing Unix-only CLI path test. Format,
strict all-target Clippy, build and warnings-denied public/private rustdoc pass. Ten focused Release
suites also pass. Full native Release is the native functional acceptance gate for this closure;
tests are neither removed nor filtered to obtain it.

Native Debug is a retained environment failure, **not** reported as PASS. The default workspace
and isolated retry cannot launch `cancellation-ece58cdd98e63827.exe` (OS 4551); the explicit-MSVC Debug
run passes 257 tests before Code Integrity blocks the acceptance executable. Scoped Windows event
IDs 3033/3077 identify enterprise signing/code-integrity requirements, policy
`{0283ac0f-fff1-49ae-ada1-8a933130cad6}`. No signing policy, application control, executable bytes or
installation was modified to defeat the block. Complete native Release execution supplies actual
cross-platform functional evidence; native Debug launch reliability remains an environment follow-up,
not a passing gate or an omitted software failure. This profile distinction is part of acceptance.

The initial native copy also exposed a genuine test setup problem: FX-11 was excluded from Git and
not present. The acceptance test now reuses the owned Rust generator/serializer in memory on every
platform; fresh checkouts need no local FX-11 file and do not write fixtures. The failed first export
is retained separately from the corrected snapshot.

The corrected native source snapshot is based on `23d2cd1`, with 153 files, archive SHA-256
`840a0b112706cd08275e782c9d5e75333f0e54453f5a200d1f423a5ac68ed04b`. Native hashes verify every
exported file before/after commands and replays. The isolated source directory is:

`%LOCALAPPDATA%\Temp\resin-slicer-m3-closure-28d99d3980a8441baaa7dc0f2aa3667e\source`

A subsequent **profiling example only** delta states the larger caller caps; its before/after source
and SHA-256 are retained separately, and final native fmt/Clippy/build/large-union validation checks
that delta. Production core and test bytes remain exactly those of the fully native-tested snapshot.
Documentation updated after execution is not misrepresented as part of that export.

### Definition of Done disposition

| Criterion | Evidence | Disposition |
| --- | --- | --- |
| Valid deterministic regions for the agreed corpus | All FX families; analytic area/topology envelopes; repeated owned output; source-labelled composition and rational occupancy tests | PASS within the declared numerical/geometry domain |
| Degeneracy policy/test coverage | Local triangle classifications; tangent/coplanar retention; new coherent shared-edge case; open/branch/nonmanifold/conflict/self-contact reports; explicit range/resource failures | PASS, ambiguity stays explicit |
| No silent repair or ownership inference | Immutable source/provenance tests, explicit fill/source IDs, no rounding fallback | PASS |
| Schedule, ordering, progress, cancellation and atomicity | Exact schedule, engine tests and corpus replay; full native Release includes cancellation tests | PASS |
| Contour/polygon fuzz/property evidence | Extended and exact-final-source campaigns plus complete final corpus replay | PASS for the bounded milestone campaign |
| Preliminary large-input performance/memory goals | Four predeclared workloads, ten batches each, three process-RSS runs each, unchanged 5 s / 64 MiB / 512 MiB budgets | PASS |
| Documented interfaces, complexity and failure modes | TD-015–032, stage contracts, architecture and this closure record | PASS |
| Linux/native Windows functional validation | Full stable Linux workspace and full native MSVC Release workspace, native quality gates; explicit Debug host limitation retained | PASS with the native profile distinction above |

**M3 is complete for this explicit corpus and contract.** This accepts a headless slicing engine,
not a printer application or a universal printable-mesh certificate. M1 EXP-03/04/commercial defaults,
native Debug host-policy remediation, sustained future fuzz monitoring, general Boolean command
families, offsets/preparation, rasterization, export and GUI remain separate work. M4 is next;
this closure does not implement or implicitly start it.

### Evidence seal and final source reconciliation

The compact external archive is `/tmp/resin-m3-closure-20261017-evidence.tar.gz`, SHA-256
`06e1de00605b8d1633d6c55cb0339f43b18d6e196bba8b4c61a7d8bc51eef0f6` (6,874,825 bytes).
It includes source exports/manifests, raw command results, failed initial exports/retries, final
native results and profiling delta, performance samples/RSS, fuzz logs/corpora, stable replay logs
and a per-file SHA-256 index. Build-target directories are excluded. The final reconciliation
verifies **101 Rust/Cargo/fixture files** against native-tested source plus the separately tested
profiling-example delta. Later status documentation is outside that production-byte claim.
The `/tmp` archive is a retained session artifact, not a promise of durable backup or a replacement
for versioning the independently reproducible test/profiling protocol.
