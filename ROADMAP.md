# karasu-slicer Roadmap

The roadmap is milestone-based and intentionally has no fixed dates. A milestone advances only when
its acceptance criteria and Definition of Done are satisfied. Advanced work must not weaken the
correctness or evidence required by earlier stages.

## Product identity

The product is `karasu-slicer` (TD-033). Packages/source directories are `karasu-cli` and
`karasu-core`; all future crates use `karasu-<module>`. The previous CLI executable name remains a
compatibility alias. Historical validation/research paths and version-1 corpus identities retain
their original spelling and hashes. This naming refactor does not start M4 or reopen M3 scope.

## Expanded MVP scope decision

Owner-approved revision, 2026-10-16 (TD-020): parametric hollowing, manually placed/editable resin
drainage holes and manually placed/editable 3D supports are **mandatory MVP deliverables**, not
exclusively Post-MVP work. They belong to the new **M5.5 — Model Preparation Engine**, followed by
M6 interactive integration and M7 end-to-end acceptance. M5.5-A contracts, M5.5-B bounded convex
hollowing, M5.5-C finite cardinal drainage and M5.5-D parametric manual supports are implemented.
M5.5-E completes bounded headless integration and native acceptance; see
[closure](docs/M55-CLOSURE-ACCEPTANCE.md). M6/M7 remain mandatory; requirements PREP-001–009
guide them.

Milestone implementation order remains M0–M5, then M5.5, M6 and M7. Runtime data flow is different:
source/placement -> preparation -> M3 slicing -> M4 rasterization -> M5 export. M5.5 integrates
against the established M2–M5 contracts; its position does not put hollowing after slicing at runtime
or create a dependency from the headless engine onto the M6 GUI. No existing milestone is renumbered.
M2 and M3 are accepted for their documented contracts/corpora. M1 compatibility research remains
open; the M3 closure below records native-profile and environment limitations.
This planning decision does not authorize implementing preparation during ongoing M3 work.

The expanded MVP increases geometric and integration effort: robust shell/offset construction,
cavity/hole connectivity, required intersection/subtraction, support contacts/collisions, bounded
memory/time, deterministic edit/recompute and native-platform validation need their own gates.
No calendar date or completion estimate is implied. Geometry tests cannot by themselves establish
physical drainage, structural support strength or universal printability.

## M0 — Project Foundation

**Scope:** Inspect the repository/environment; establish a minimal Cargo workspace, permanent
engineering conventions, architecture and numerical foundations, a research policy, testing strategy,
and the staged roadmap. Do not implement slicing or select a GUI.

**Deliverables:** `karasu-core`, `karasu-cli`, workspace quality configuration, baseline tests,
`README.md`, `AGENTS.md`, this roadmap, and the four design documents under `docs/`.

**Dependencies:** Existing Git repository and a stable Rust toolchain for executable validation.

**Technical risks:** The initial WSL environment has no Rust toolchain; hand-authored Cargo metadata
cannot be compiled until one is available. Premature APIs could constrain later geometric design.

**Acceptance criteria:** Repository content is preserved; artifacts are English; crates remain small
and dependency-free; the CLI exposes only identity/help/version; numerical and dependency policies are
explicit; required Cargo checks pass once stable Rust is available.

**Automated tests:** Core application identity and CLI parsing/help behavior. Formatting, Clippy,
workspace test, and workspace build gates.

**Reference research:** Define legal/ethical separation, evidence records, and the M1 protocol only;
do not run an extensive campaign.

**Definition of Done:** All deliverables are reviewed, Rust availability and each validation command
are reported truthfully, open decisions are recorded, and no commit/push/tool installation occurs
without authorization. Compilation remains a blocker until the toolchain gate runs successfully.

## M1 — Reference Research

**Current research scope (TD-036, 2026-10-17):** EXP-03 and EXP-04 are **optional compatibility
research, unperformed**. They do not gate M1, M3 or later implementation/acceptance, or independently
specified Karasu defaults. Existing protocols, fixtures and evidence are retained; commercial parity
remains unverified. This decision supersedes earlier EXP-03/04 gate references in staged historical
notes below. Independent correctness tests and all other milestone acceptance criteria remain.
M1 is not marked complete by this reclassification.


**Scope:** Establish evidence-backed functional requirements by running controlled, lawful
behavioural experiments with other resin slicers (Lychee Slicer 8, CHITUBOX) and an independently
owned test corpus.

**Deliverables:** The programs' versions; an evidence register; controlled model specifications and scenario matrix; coordinate/layer/raster/output observations; prioritized
behavioral requirements; reproducible test specifications.

**Dependencies:** M0 research policy; lawful local access to reference installations; hashing and
inspection tools; public specifications; owner-approved storage for raw evidence.

**Technical risks:** Mistaking an observation for the program's actual rule; version drift; hidden defaults; nondeterministic output; proprietary artifacts leaking into source; one
product's behavior being treated as correctness.

**Acceptance criteria:** Both installed products are identified from evidence; observations,
interpretations, and hypotheses are separate; each requirement cites evidence and defines tolerance/
validation; settings and input hashes make scenarios repeatable.

**Automated tests:** Generators/validators for owned reference models where feasible; evidence-schema
validation; independent readers for any publicly documented structures selected for study.

**Reference research:** This is the primary milestone. Start with read-only inventories, then vary one
observable parameter at a time. Do not bypass protections or translate proprietary implementation.

**Definition of Done:** The most important M2–M5 questions have requirements and replayable scenarios;
uncertainty is visible; no proprietary code/assets enter production; research is sufficient to guide
implementation without dictating it.

## M2 — Mesh Engine

**Status: COMPLETE.** M2.1 mesh/STL foundation, M2.2 transforms/world bounds, M2.3 topology, and
Closures A–C are implemented. The accepted production baseline is
`d8816303bbc4c2ba26213645ddf278db6574342b`. Stable Linux/WSL validation passes 224 tests with zero
failures and two optional tests ignored. Final native Windows validation of the same commit passes
223 tests with zero failures and two ignored; the one-test difference is the intentionally Unix-only
non-Unicode-path regression. Formatting, strict Clippy, build, warnings-denied rustdoc, cancellation,
CLI inspection, transform round trips, representative native CLI runs, and fixture/source integrity
all pass on Windows with no platform defect or source change. The archived evidence integrity index
is SHA-256 `1b3adb055adcecd842a5e3cbfd8fa1e4b31d0a540d556c9b1aab7ec6f085956a`.

Closure A supplies synchronous cooperative mesh cancellation with deterministic phase checkpoints and
documented blocking-I/O/sorting limits (TD-014). Closure B supplies read-only
`inspect --assume-mm <path>` with bounded, deterministic source-space/diagnostic/topology output.
Closure C supplies explicit invertible-transform round trips, independent scalar/rational oracles,
well-conditioned floating-point budgets, canonical source preservation, and singular-case coverage.
M2.4-B completed the initial bounded four-target fuzzing campaign with 6,226,119 executions and no
reported failure. M2.4-C establishes the repeated WSL/Linux Release timing and whole-process peak-RSS
baseline; see `docs/PERFORMANCE-BASELINE.md`. Neither campaign proves defect absence, worst-case
memory, or native Windows performance. M1 research remains independently open; M2 completion does not
select proprietary conventions or mark M1 complete. M3 is the next implementation milestone, subject
to its own entry requirements and authorization.

**M2.1 delivered:** Immutable `f64` source vertices and indexed triangles; explicit unit policy;
bounded byte/reader/path import; deterministic exact-coordinate deduplication; source bounds;
retained normal/attribute metadata; structured failures; non-destructive degeneracy/duplicate
diagnostics; owned-corpus and malformed-input regressions. The CLI was help/version only at M2.1;
Closure B subsequently adds the documented read-only inspection path.
See `docs/ARCHITECTURE.md` for supported grammar, detection precedence, numeric limitations, and caps.

**M2.2 delivered:** Native `f64` unit-quaternion rotations, checked affine composition, explicit
local-pivot object transforms, uniform/non-uniform/zero/negative scale policies, point/direction
mapping, and tight transformed-vertex world bounds without source copies or mutation. No automatic
plate placement, printer dependence, inverse, normal conversion, or baking is included. See TD-012.

**M2.3 delivered:** Deterministic source-index directed edge incidence; boundary, two-face,
non-manifold and local-winding findings; edge-connected and vertex-connected triangle partitions;
simple-path/cycle vertex links including vertex-only shell contacts; explicit degenerate exclusions
and isolated vertices; retained duplicate incidences; bounded, fallible derived storage. Closed
combinatorial classification is not a printable-solid claim. See TD-013 and the architecture.

**Validation and deferred work:** Final native Windows functional verification, the initial bounded
topology/parser fuzzing campaign, and representative timing/peak-memory profiling are complete.
Sustained fuzzing, precise allocator attribution, native Windows performance measurement, optional
FX-11 Windows execution, and further structural/numerical validation remain future work. Cancellation
has no job orchestration, progress API, or strict wall-clock latency guarantee. Self-intersection and
printability analysis and any repair operation require separately approved specifications. No
slicing/rasterization is introduced by M2 or authorized by its completion.

**Scope:** Implement binary and ASCII STL import, internal mesh representation, transformations,
topology analysis, invalid-geometry diagnostics, and only conservative explicitly requested repair.
Define import limits and cancellation behavior.

**Deliverables:** Mesh/vertex/face types; STL detector/parser; source-to-world transforms; finite/range
validation; adjacency/topology metadata; boundary/non-manifold/degenerate diagnostics; corpus fixtures;
CLI inspection path.

**Dependencies:** M0 conventions and toolchain gate; M1 unit/orientation requirements; approved
native-Rust dependencies, if any.

**Technical risks:** Ambiguous ASCII/binary detection, hostile sizes, numeric overflow, duplicated or
degenerate faces, excessive adjacency memory, unit assumptions, and repairs that change topology.

**Acceptance criteria:** Known valid STL files import with correct bounds/orientation; malformed and
resource-exhausting inputs fail safely with context; transforms are explicit; diagnostics are stable;
original geometry is not silently changed.

**Automated tests:** Parser unit/integration tests, truncated/corrupt inputs, round-trip transforms,
topology fixtures, property tests, parser fuzzing, deterministic diagnostics, and representative
memory benchmarks.

**Reference research:** Validate observable STL unit/orientation and placement behavior using M1
scenarios; use public STL conventions as the primary format basis.

**Definition of Done:** Public mesh APIs are documented; corpus provenance is recorded; checks pass on
Linux and native Windows; fuzzing finds no known crash/hang; known limitations are explicit.

## M3 — Geometry and Slicing Engine

**Current status: COMPLETE for the documented corpus and numerical/geometry contracts (2026-10-17).**
The [closure record](docs/M3-CLOSURE-ACCEPTANCE.md) is authoritative over historical pending statuses
below. Whole-corpus replay found and fixed coherent on-plane shared-edge crossings (TD-032),
including equators of the 327,680-triangle sphere. Source occurrences remain preserved; ambiguity
is still explicit. All four large-input workloads pass the predeclared preliminary budgets.
Stable Linux passes **611/0/2**; complete native MSVC **Release** passes **610/0/2**, with required
native format/Clippy/build/rustdoc checks. Native Debug executable launch remains blocked by host
Code Integrity (4551), retained explicitly rather than reported as success. The full Release run
is native functional acceptance; no tests or security policy were bypassed. Bounded fuzz campaigns
and final retained-corpus replay pass. M1 compatibility defaults, native Debug host remediation and
future expanded robustness monitoring remain separate. This M3 closure introduced no M4 features;
the subsequent M4 implementation and its acceptance are recorded below.

The following dated entries retain the development history, not additional current closure blockers.

**Entry status:** M3.0-A targeted static research passed at M2 closure HEAD
`afc04f77fb169d643e187d0c61d21ea04c47f52a`; its raw evidence remains outside Git. M3.0-B now
records the independent local kernel contract in TD-015–017 and
[M3-TRIANGLE-PLANE-CONTRACT.md](docs/M3-TRIANGLE-PLANE-CONTRACT.md). The first M3.1 increment is
**implemented and validated on stable Linux/WSL**: one finite-f64 triangle and one explicit plane,
exact scalar classification, checked approximate construction, raw incidence events, degeneracy context,
deterministic local provenance, and the exhaustive 27-state/permutation test specification.
M3.1 follows the committed contract unchanged, with independent analytic/rational tests, fixed-size
results and typed errors. Native Windows validation of committed M3.1 HEAD
`5f12ebcdf6b75366479ddf974555d6afa1feda24` passes 256 tests, zero failed and two ignored, with
28 focused tests in both Debug/Release and four private kernel tests; the single difference from
WSL's 257 is the Unix-only CLI test. The accepted M2 Windows result is not reused.
M3.0-C's comparative study passed outside Git and recommended this
independent contract unchanged; no proprietary implementation or numerical constants are adopted.

No traversal, layer schedule, welding, contour, polygon, raster, CLI, progress or cancellation
controller is part of that local increment. **M3.2-A is implemented and stable Linux/WSL validated**:
the `slicing` module performs a deterministic full source-face scan at one supplied world Z with
explicit identity/object/affine mapping, preserved raw incidence/provenance/multiplicity, post-transform
degeneracy context, M3-specific face/event/byte limits and cooperative atomic cancellation (TD-018).
The unchanged M3.1 kernel is the local operation. There is no candidate filtering, source geometry
copy/cache, ownership, welding or normalization. The new increment passes 298 workspace tests,
zero failed and two optional tests ignored, including 39 focused Debug/Release tests, one private
overflow test and one executable API example. Native Windows validation was pending at that
implementation checkpoint; the subsequent consolidated campaign below validates the changed
traversal/cancellation code independently of the earlier M3.1 campaign.

**M3.2-B is implemented and stable Linux/WSL validated** at the normalization scope in TD-019:
source-feature endpoint/segment groups, separate exact-coordinate observations, complete occurrence
multiplicity, explicit finite-coordinate conflicts and conservative verified local coplanar patch
ownership. Internal edges remain present; mixed/on-edge, duplicate, non-manifold, collapsed and other
ambiguous contributions are retained as structured unresolved groups, never silently filled or repaired.
An independent fixed-storage exact XY sign verifies coplanar nondegeneracy/opposite half-planes without
changing M3.1, quantizing coordinates or adding dependencies. Tests pass **336/0/2**, including
31 new focused integration tests and six private checks in both Debug and Release, plus one executable
example. Combined native Windows validation of M3.2-A/B was intentionally pending at that checkpoint
and subsequently passed in the consolidated campaign below; neither all of M3.2 nor M3 was thereby
closed. At the normalization checkpoint M3.3 still required branch/open-path, duplicate/ambiguous-incidence and contour
orientation policies; these are not selected by normalization. EXP-03 plane-placement and EXP-04
tangent/coplanar observations are now optional compatibility research under TD-036; neither gates
independent layer scheduling or selects M3.1's raw-event policy. TD-019 defines only local coplanar patch ownership, not
a commercial or universal filled-boundary rule. The original integer polygon representation/scale
gate before M3.4 is retained, with only the explicit detection-only TD-023 exception below.
These staged entry gates do not remove
the original dependencies or acceptance requirements below. M1 remains independently open; M2
acceptance is unchanged; the whole M3 milestone is not yet implemented, validated or complete.

**M3.3-A is implemented and stable Linux/WSL validated** at the incidence-graph scope of TD-021:
canonical source-feature vertices and complete feature-pair group records, borrowed lossless raw
occurrence references, separate complete/locally eligible adjacency, explicit degree/multiplicity
semantics, ordered ambiguity/conflict and potential endpoint/branch diagnostics, bounded fallible
storage and cooperative atomic cancellation. Internal patch edges remain inspectable; unresolved
groups cannot enter candidate adjacency. No welding, intersection splitting, path extraction,
automatic closure, routing, orientation or filled-region acceptance is introduced. The 336/0/2
baseline grows to 386/0/2, including 45 graph integration tests, four private tests and one executable
example; focused integration/private tests also pass in Release. Native Windows validation of
M3.2-A/B and M3.3-A was pending at that checkpoint and subsequently passed in the consolidated
campaign below. Path extraction and ambiguous/branch/open-path policy were deferred to M3.3-B below;
geometric orientation remains M3.4 work. M3.3 and M3 remain incomplete; M1,
EXP-03/04, integer-scale gates and the mandatory expanded M5.5/M6/M7 commitments are unchanged.

**M3.3-B is implemented and stable Linux/WSL validated** at TD-022's conservative path scope:
borrowed graph input, exact candidate-group accounting, candidate-connected components, maximal
nonbranching trails stopped at branches/uncertainty, canonical open chains and provisional graph-simple
cycles. Complete-only groups, numerical conflicts and raw multiplicity remain inspectable; no
arbitrary routing, automatic closure or geometric validity is inferred. The 386/0/2 baseline grows
to 441/0/2 with 45 integration tests, nine private tests (including two test-only graph-corruption
cases) and one executable example; focused new tests also pass in Release. M3.4 still owns XY
simplicity/intersection/overlap/area, geometric orientation, integer polygon scale, nesting and regions;
EXP-03/04 and M1 remain open. Consolidated native Windows validation of M3.2-A/B and M3.3-A/B
subsequently passed at committed HEAD `c4865117e18d998fe80d48d8fac1022b5731ebc8`: 440/0/2 versus
Linux/WSL 441/0/2, with the Unix-only CLI test accounting for the difference. All four focused
integration suites passed in Debug/Release; the 39-test `mesh_section` Release run is owner-supplied
interactive evidence, with reported rather than independently machine-captured exit 0. Earlier
Code Integrity launch failures remain archived. See the
[consolidated validation record](docs/TEST-STRATEGY.md#subsequent-consolidated-native-windows-m32m33-validation)
for source provenance, integrity and limitations. This closes that native campaign, not M3.3 or M3;
their geometric/research and remaining acceptance gates above remain open. M5.5 and expanded M6/M7
obligations are unchanged.

**M3.4 entry research/design is complete; the owner approved detection-only staging under TD-023.**
The comparative static study and
[independent design](docs/M3-PLANAR-VALIDITY-DESIGN.md) specify detection-only represented-XY contact
inspection over immutable M3.3 paths. Approval followed research commit `b1b58ba` on 2026-10-16.
M3.4-A implements exact contacts/collapse, local cycle embedding, separate cross-path diagnostics,
unassessed context, deterministic bounded pair work/storage and atomic TD-014 cancellation, without
new dependencies or upstream mathematical changes. Stable Linux/WSL validation passes 459/0/2,
including 12 new integration tests, five private tests and one executable example; focused new
integration/private and shared exact tests also pass Release. Native validation is recorded in the
test strategy. The integer conversion/construction/region gate remains OPEN. Area/orientation,
construction, nesting/material, scheduling, M1/EXP-03/04 and
whole-M3 acceptance remain outstanding. Reference runtime performance and global validity are
unknown. M3.4-A supplies path-embedding observations, not material acceptance or whole-M3 closure.

**M3.4-B cycle area and traversal orientation is implemented under owner-approved TD-024.** The
targeted comparative research and
[implementation contract](docs/M3-AREA-ORIENTATION-CONTRACT.md) separate historical evidence from
the independent Rust design. The immutable area pass measures already-simple represented-XY cycles
with exact orientation and a correctly rounded, explicitly ranged mm^2 magnitude. It adds no
geometry, material roles, rewinding, grid, nesting, scheduling or dependency. The proposal explicitly
compares exact accumulation with rigorously bounded compensated/filter alternatives. It does not
retroactively expand TD-023 or close the retained conversion/construction/region gate. Actual
Linux/native results are recorded in the test strategy. No hole/material roles, summed material
area or whole-M3 acceptance is established.

Scoped M3.4-B validation passes Linux/WSL **481/0/2** and native Windows **480/0/2**, including 14
new integration tests, seven private arithmetic/consistency tests and one working API example.
Focused new and unchanged predicate/validity checks also pass Release. The test strategy retains
the initial native capture error and host-policy-blocked doctest separately from the final unchanged
passing replay. This is no new performance/fuzz acceptance, physical grid or M1/M3 closure.

M3.4-B was subsequently committed at `1c4f52e` on 2026-10-16 at the owner's request, before the
next research increment. **M3.4-C ExactOnly mapping is implemented after owner approval and contract
commit `75acecd`.** The
comparative research remains a historical research
checkpoint; the [finalized contract](docs/M3-INTEGER-GRID-CONTRACT.md) specifies validated metadata,
one outcome per graph vertex, exact divisibility/range arithmetic, resource caps, TD-014 checkpoints,
structured errors and deterministic acceptance tests. The owner subsequently authorized only that
implementation; a later owner request authorizes committing it, not further geometry.
`slicing::grid` borrows the inspection and
maps every agreed feature exactly, rejecting off-grid/range points atomically while retaining
conflicts and complete-only context. No physical step, positive loss allowance or universal printer
envelope is selected. TD-025 records this boundary; actual Linux/native results are in the test
strategy. Lossy conversion, construction, topology-change/material acceptance and regions remain
OPEN. No repair, dependency, integer area, inverse decoder, layer policy or constructor is added.
Scoped acceptance passes Linux/WSL **510/0/2** and native Windows final replay **509/0/2**, adding
16 integration tests, 12 private tests and one API example. Stable gates and focused Release checks
pass on both. The test strategy retains the initial Windows CLI application-control failure,
successful unchanged replays and source provenance separately. This is not whole-M3, performance,
fuzz, construction/material or M1 acceptance. The owner subsequently requested committing the
validated implementation.
EXP-03/04, M1, remaining M3 and mandatory M5.5/M6/M7 commitments are unchanged.

**M3.5-A strict exact region hierarchy is implemented under TD-026.** The
[independent completion specification](docs/M3-COMPLETION-SPECIFICATION.md) traces the remaining
region, construction, schedule, progress, fuzz and performance gates from the M3.4-C baseline.
Raw commercial evidence remains outside Git. `slicing::regions` accepts only matching M3.4-A/B/C
reports, produces a nonconstructive exact containment forest and applies `EvenOddStrict` to
non-contacting eligible `SimpleCycle` paths. Open, ambiguous, contacting and unresolved inputs stay
structured outcomes; no repair, Boolean construction or source-solid claim is implied. The finalized
[contract](docs/M3-REGION-HIERARCHY-CONTRACT.md) defines exact half-open containment, limits and
atomic cancellation.

Scoped stable Linux/WSL validation passes format checking, strict workspace Clippy, build,
warnings-denied rustdoc, `git diff --check` and **518/0/2** workspace tests; the five focused
hierarchy tests also pass in Release. Native Windows validation, fuzzing/performance evidence and
whole-M3 acceptance remain separate work and are not implied by this checkpoint.

**M3.5-B explicit uniform layer scheduling is implemented under TD-027.** On 2026-10-17 the owner
authorized reference research, autonomous independent specification and implementation of this
increment. The research retains unknown commercial scheduling rules. The
[finalized contract](docs/M3-LAYER-SCHEDULE-CONTRACT.md) and `slicing::schedule` require finite first
Z, positive height, explicit `Count`/`Before`/`Through`, and caller-selected count/payload caps.
Fixed 2176-bit signed arithmetic derives exact terminal membership over the u64 index domain and
publishes once-rounded nearest-even finite planes with strictly increasing Z. Range/resolution,
capacity/allocation and cancellation failures publish no successful partial schedule. An exact
integer recurrence avoids cumulative floating drift and per-output multiplication; no measured
speed claim, default physical policy, new dependency, geometry orchestration or CLI is added.

Scoped stable Linux/WSL gates pass **554/0/2** workspace tests, format checking, strict all-target
Clippy, build and warnings-denied rustdoc. The 26 schedule integration and nine private arithmetic
tests also pass Release; one new executable API example passes. An independent Python Fraction
oracle agrees on 8,000 deterministic full-domain-input cases. Bounded fuzz evidence and limitations
are recorded in the test strategy. Native Windows validation remains pending for this increment;
earlier native results are not reused.

**M3.6 owned layer composition, progress and atomic collection is implemented under TD-029.** The
owner's subsequent 2026-10-17 request authorizes focused reference research and independent
implementation. The research retains unknown progress/stop-latency/publication behaviour. The
[contract](docs/M3-LAYER-ENGINE-CONTRACT.md) composes the eight existing cancellation-aware stages
serially over an immutable source/transform and validated schedule. Each yielded layer owns flat
geometry, raw provenance, hierarchy, ambiguity outcomes and diagnostics. Errors preserve scheduled
plane and underlying stage context and fuse the stream. Exact completed-layer progress is separate
from cancellation; normal exhaustion is required for complete-stream status. A fresh-engine atomic
collector adds explicit count/header/aggregate payload limits and defers completion until its final
publication check. No dependency, constructor, raster, export, printer default or new CLI is added.

An independent nested-layer fixture exposed an existing strict-containment implementation defect:
the ray test omitted the strict upper-Y endpoint bound. The corrected half-open straddle now
preserves shell/hole/island nesting under collinear subdivision, with direct analytic regressions.
This enforces TD-026's existing contract and changes region code, so prior native results do not
validate this revision automatically.

Scoped stable Linux/WSL validation passes format checking, strict all-target Clippy, build,
warnings-denied rustdoc and **571/0/2** workspace tests. Twelve engine integration tests, three
owned-projection private tests, three containment predicate tests and five existing hierarchy tests
also pass Release. Bounded fuzz evidence is recorded separately in the test strategy. Native Windows
validation remains pending: the current session has no registered WSL interop bridge.

**M3.5-C exact source-material composition is implemented under TD-030.** The owner's subsequent
remaining-M3 request authorized the [gap audit](docs/M3-COMPLETION-AUDIT.md),
reference research, independent specification and code.
The [contract](docs/M3-CONSTRUCTIVE-MATERIAL-CONTRACT.md) defines explicit parity within material
sources and union across sources. Original integer segments are split at exact rational crossings,
touches and overlap endpoints; all atomic supports and left/right outcomes remain inspectable.
Material-left boundary walks produce contact-aware cycles and exact outer/hole hierarchy. Checked
512-bit signed arithmetic covers the proved domain, without dependencies or floating reconstruction.

`LayerEngine::with_material_union` requires an explicit source ID per original face and consistent
ownership across each eligible path. It retains the complete strict report and attaches separate
constructed material before atomic layer publication. Open/ambiguous/non-simple paths, unresolved
ownership and off-grid input remain typed failures. Existing strict APIs/configuration remain
compatible. No silent repair, lossy conversion, universal grid or printer default is introduced.

Scoped stable Linux/WSL gates pass **592/0/2** workspace tests, formatting, strict all-target Clippy,
build and warnings-denied rustdoc. Eight constructive tests, four engine-adapter tests and eight
private arithmetic tests also pass Release. Dedicated ASan campaigns pass 6,713 constructive and
1,796 strict-region executions; complete final corpora replay without findings. The new
[Release profiling protocol](tools/m3-profile/README.md) records six independently validated
layer/region/constructor/count-cap workloads, timing, retained payload and separate process RSS.
These are bounded initial observations, not universal performance acceptance.

Remaining M3 acceptance includes native Windows on this revision, agreed-corpus/numerical-domain
review, sustained/large-input validation and adoption of preliminary performance/memory budgets.
The current session has no WSL interop registration. ExactOnly remains deliberately restrictive:
approximate upstream intersections can be off-grid even for an analytic shape/plane combination.
No physical snapping tolerance is inferred to bypass that error. Under TD-036, EXP-03/04 are
optional comparisons; commercial-default parity claims still require appropriate evidence. M1 and whole M3 are not marked complete; mandatory M5.5/M6/M7 commitments remain.

**Scope:** Implement robust triangle-plane classification/intersection, deterministic segment
normalization, closed contour reconstruction, holes/disconnected regions, required 2D polygon
operations, slice planes, and valid layer-region generation. Add spatial acceleration only with
measured benefit.

**Deliverables:** Explicit slicing configuration/tolerances; robust predicates; intersection event
model; canonical segment/contour/region structures; layer iterator; diagnostics for open/ambiguous
results; analytic and regression corpus; optional proven acceleration index.

**Dependencies:** Validated M2 mesh model; M1 plane/tangency observations; decision on predicates and
integer polygon scaling; mathematical references and independently derived algorithms.

**Technical risks:** Coplanar faces, vertex/edge touches, non-manifold input, endpoint snapping that
changes topology, polygon overflow, incorrect hole nesting, nondeterminism under parallel execution,
and combinatorial cost.

**Acceptance criteria:** Analytic primitives yield correct topology, bounds, and areas within stated
tolerances; degeneracies follow documented policies; ambiguous/open contours are reported; layer
ordering and bytes are deterministic; large inputs meet preliminary memory goals.

**Automated tests:** Exhaustive local classifications, analytic slices, winding/nesting tests,
permutation properties, tolerance-boundary cases, regressions, fuzzing for contour/polygon stages,
and performance/memory benchmarks.

**Reference research:** Compare first-plane placement, tangent/coplanar behavior, region topology, and
invalid-input outcomes through behavioral scenarios. Reference products are comparison oracles, not
implementation sources.

**Definition of Done:** The engine creates valid deterministic regions for the agreed corpus; every
degenerate class has a policy/test; no silent topology repair occurs; public APIs and complexity are
documented; native Windows/Linux gates pass.


**M3 represented-coordinate completion is implemented under TD-031**, by the
[independent contract](docs/M3-REPRESENTED-COORDINATE-CONTRACT.md). Callers can now explicitly
retain f64 millimeter contours and assemble strict regions with exact dyadic predicates, bypassing
mandatory `ExactOnly` conversion. Optional material union uses lossless common dyadic encoding and
retains the exact physical scale; unsupported >60-bit encoding ranges fail explicitly. Decimal
contours and quarter-plane sections now have end-to-end regression coverage, alongside cancellation,
limits, provenance and 60-bit rational topology. No source code or dependency was imported.
The old exact-grid default/API remains compatible. This supersedes the earlier mandatory-grid
limitation, not the native Windows, corpus, reference and preliminary-budget acceptance gates;
whole M3 remains open for those gates rather than a known missing implementation module.

**Native Windows acceptance subsequently passes at `382fb62` on 2026-10-17.** The
[campaign](docs/M3-NATIVE-WINDOWS-VALIDATION.md) records native MSVC **600 passed, 0 failed, 2 ignored**,
required quality gates, warnings-denied rustdoc, seven focused Release suites and before/after
SHA-256 verification of all 150 exported files. No production fix or security-policy change is
needed. Sandbox/PowerShell wrapper restrictions explain the access obstacles; Windows itself and
the installed native toolchain work. This supersedes the native pending status above for this
revision. Agreed-corpus, adopted performance/memory budgets and remaining reference gates stay open.


## M4 — LCD Layer Engine

**Current status (2026-10-17): COMPLETE for the documented engine contracts and validation corpus.**
Platform acceptance (Linux 645/0/2; native Windows Debug and final Release 644/0/2),
including the retained initial Windows launch failure and successful unchanged retries, is recorded in
[M4-VALIDATION.md](docs/M4-VALIDATION.md). The dependency-free safe-Rust `raster` module includes
validated LCD profiles and inverse projection, strict/constructed M3 material adapters, active-edge
midpoint sampling, deterministic grayscale quantization, discrete rectangular grayscale compensation,
no-cache fused streaming, preview metadata and bounded deterministic grayscale8 PNG export.
The [implementation contract](docs/M4-LCD-IMPLEMENTATION.md) is authoritative for supported numeric
ranges, error semantics, clipping, operation order, work/storage bounds and cancellation.

Reference research precedes independent design. The controls other programs offer are
evidence-backed; exact runtime orientation, proprietary
AA/gray/compensation kernels and compatible defaults remain unmeasured M1 EXP-05/06 questions.
No commercial parity, optical calibration, Euclidean contour offset or conservative thin-feature
coverage is claimed. These unknowns do not select hidden implementation defaults.

Linux verification includes analytic masks, all orientation combinations, sampling/gray levels,
independent point/morphology oracles, constructed rational boundaries, cancellation/errors/resource
caps and all owned reference fixtures including generated FX11. PNG decoding and pixel comparison
use an independent Python zlib reader. Benchmark tools and predeclared budgets are in
[`tools/m4-profile`](tools/m4-profile/README.md). Native Windows outcomes remain separate from Linux.
M5 job/export and M5.5 preparation are not started by this increment.

**Scope:** Map physical regions to LCD space, rasterize binary/grayscale masks, implement configurable
antialiasing and pixel compensation, preview layers, bound memory use, and export reference PNGs.

**Deliverables:** Validated LCD geometry/profile subset; physical-to-pixel transform; deterministic
coverage rasterizer; grayscale/antialias configuration; compensation operation; layer stream/cache;
PNG reference export; preview metadata.

**Dependencies:** M3 regions; M1 raster orientation and sampling evidence; printer dimensions/pitch;
approved native-Rust image dependency or a justified local implementation.

**Technical risks:** Half-pixel/origin mistakes, vertical flips, overflow at image bounds, grayscale
semantics, non-conservative antialiasing, unbounded layer storage, and platform-dependent images.

**Acceptance criteria:** Analytic regions map to expected pixels and physical bounds; orientation and
dimensions are validated; antialiasing/compensation are explicit and reproducible; output streams
within a defined memory budget; PNGs decode with an independent reader.

**Automated tests:** Pixel-boundary unit tests, golden masks with justified updates, transform
round trips, coverage properties, out-of-bounds cases, independent PNG decoding, deterministic hashes,
and memory/throughput benchmarks.

**Reference research:** Measure image origin, flips/rotations, sampling convention, grayscale levels,
antialias modes, and pixel compensation on controlled shapes.

**Definition of Done:** Corpus layers render deterministically with documented error bounds;
independent readers validate exports; resource budgets are met; native Windows/Linux tests pass.

## M5 — Print Jobs and Export

**Status: COMPLETE for the documented headless job/profile and exporter contracts.** The M5.5-A
validation snapshot subsequently passes complete native Windows Debug and Release suites (735/0/2),
including existing M5 tests, with the exporter implementation unchanged. This resolves the prior M5
Release acceptance blocker for that snapshot; initial 4551 failures remain preserved in the
[validation record](docs/M5-VALIDATION.md). No permanent host-policy repair, physical printer,
dual-material P1 or encrypted-only CTB firmware acceptance is claimed.

**CLI access (TD-043, 2026-10-18):** `karasu-cli slice` exposes the existing M2→M3→M4→M5 pipeline
for one explicitly placed STL. It adds no geometry policy and no M5.5/M6 scope. TD-044 adds stderr
progress and cooperative Ctrl-C cancellation through the `ctrlc` dependency. Native Windows
validation of the command is pending.

**Implementation:** Owner-expanded CTB/GOO/Anycubic scope is implemented through versioned exporter
IDs, with Photon P1 as the first Anycubic target. See [M5 specifications](docs/M5-IMPLEMENTATION-SPEC.md)
and [validation](docs/M5-VALIDATION.md). This is a headless implementation; physical printer,
dual-material P1 calibration and encrypted-only CTB firmware compatibility are not established.


**Scope:** Define printer/resin profiles, layer/exposure/motion metadata, validated print jobs, PNG
sequence plus JSON manifest, then select and independently implement one machine-specific format.

**Deliverables:** Versioned profile/job schemas; validation rules; deterministic manifest/reference
export; exporter interface; first machine-format writer and independent reader/validator; sample jobs.

**Dependencies:** M4 layer stream; M1/M5 lawful format evidence; project-owner choice of first printer
format; dependency/license review; representative hardware or trusted independent validator.

**Technical risks:** Undocumented/proprietary formats, checksum/compression errors, unsafe motion or
exposure values, format-version drift, corrupted partial output, and false confidence without printer
validation.

**Acceptance criteria:** Invalid profiles/jobs fail before export; reference output is deterministic;
machine output is accepted by an independent reader and, when safely authorized, representative
device/workflow; partial files are not presented as complete.

**Automated tests:** Schema and boundary tests, compatibility fixtures, exporter/reader round trips,
checksums, corrupt/truncated inputs, deterministic hashes, cancellation/atomic-finalization tests,
and large-job memory benchmarks.

**Reference research:** Document public specifications and observable container/metadata semantics.
Do not bypass encryption/signing or copy vendor SDK code/assets.

**Definition of Done:** Profiles and jobs are versioned/documented; reference and first machine export
are validated independently; safety ranges and limitations are prominent; both platforms pass.

## M5.5 — Model Preparation Engine

**Status:** M5.5-A contracts, M5.5-B convex hollowing, M5.5-C bounded finite drainage cuts and
M5.5-D parametric manual support geometry are implemented. Validation records cover
[A](docs/M55-VALIDATION.md), [B](docs/M55B-VALIDATION.md), [C](docs/M55C-VALIDATION.md)
and [D](docs/M55D-VALIDATION.md). **E is complete within the bounded headless domain**;
[E specification](docs/M55E-INTEGRATION-SPEC.md) and [closure acceptance](docs/M55-CLOSURE-ACCEPTANCE.md)
record lifecycle/progress integration, independent combined output validation and full native gates. Earlier M5 Windows Release failures are retained;
subsequent complete native passes resolve that software gate without modifying host policy. This is a headless native
Rust subsystem, not a GUI-owned algorithm collection. Traceability: PREP-001–007; PREP-008/009
define its mandatory M6/M7 handoff. Original M0–M7 engineering obligations still apply.

**Scope:** Reversible, non-destructive ordered project operations for configurable hollowing,
manually placed/editable drainage holes and manually placed/editable 3D support structures. Preserve
original meshes/STLs and explicit transforms; generate validated derived geometry or prepared-scene
components suitable for M3, with reproducible settings/history, diagnostics, limits and cancellation.
Automatic island detection, automatic support placement and support optimization are not required.

**Dependencies:** Validated M2 geometry/transforms and completed M3/M4/M5 slicing, raster and export
interfaces; independent mathematical design, owned analytic fixtures, reviewed native-Rust algorithms/
dependencies and a declared supported input domain. M2 topology findings are not self-intersection,
solid, thickness or printability proofs. No new foreign geometry kernel is permitted.

### M5.5-A — Model Preparation Requirements and Mathematical Design

**Implementation:** the [complete A contract/design](docs/M55-IMPLEMENTATION-SPEC.md) and
reference research precede the independent safe-Rust `preparation` plan/history code. Canonical content records, full-prefix support bindings, explicit
world-frame parameters and bounded transactional undo/redo are implemented and tested. Source hashes
are caller assertions; no hollowing/cut/support geometry, solid proof or prepared-scene evaluator is
exposed. B–E retain their geometry/proof/integration acceptance obligations. No new dependency is added.


**Deliverables:** Specify immutable source versus derived assets, ordered operations, provenance,
coordinate frames, transform/anchor behavior, wall/cut/contact units, numeric tolerances and composed
approximation/error budgets. Define mesh validity prerequisites and explicit rejection/indeterminate
handling for open, non-manifold or self-intersecting inputs. Define reversible history/replay and
versioned source/settings identity; resource budgets, phase progress and cooperative cancellation;
prepared-scene/material-combination contracts; analytic fixtures and test specifications.

**Acceptance gate:** Review independently justified offset/shell, voxel, signed-distance and Boolean
or equivalent alternatives against robustness, approximation, licensing, native Rust and platform
constraints before choosing any. Derive the supported operating envelope and test budgets; do not
assume arbitrary meshes can be prepared. Record only implemented needs, not speculative APIs or
dependencies. The A design selects an implicit world-distance/cell reconstruction direction for B–D, with
explicit oracle, validity and topology obligations before implementation acceptance; no geometry
backend is implemented by A. See the specification for alternatives, provisional budgets and fixtures.

### M5.5-B — Hollowing

**Implementation:** Independent safe-Rust `preparation::hollow` constructs an inward cavity and
sliceable shell for one certified closed outward convex polyhedral source. See
[contract and certificate](docs/M55B-HOLLOWING-SPEC.md),
research, and [validation](docs/M55B-VALIDATION.md).
Exact source/hull predicates, interval erosion/volume brackets, preserved placed exterior, cavity
preview geometry, bounded work/storage, advisory progress and atomic cancellation are implemented.
Concave/multiple/invalid/ambiguous inputs fail explicitly; arbitrary STL hollowing is not claimed.
The provisional A voxel choice is superseded for this first backend by convex supporting-plane
erosion (TD-038); no general voxel/nonconvex backend or new dependency is introduced.
The bounded-domain gate passes Linux 761/0/2 and native Windows Debug and Release 760/0/2,
independent fixture readers, a short ASan campaign and declared preliminary fixture budgets.
See the validation record for provenance and limits.


**Deliverables:** Parametric positive finite wall thickness in mm in a named evaluation space;
deterministic internal cavity generation; derived geometry, cavity preview data, thickness/boundary
diagnostics and reproducible operation settings. Preserve original source and make approximation
resolution/error visible when exact construction is infeasible.

**Acceptance gate:** Supported analytic solids meet the declared shell/thickness/volume error budget;
thin features, excessive thickness, invalid/unsupported/indeterminate input, numeric range failure
and memory exhaustion produce explicit diagnostics or typed failure, never a fabricated valid shell.
Cancellation publishes no completed partial geometry. Arbitrary-mesh hollowing success is not promised.

### M5.5-C — Drainage Holes

**Implementation:** Safe-Rust `preparation::drainage` admits finite cardinal cylinder cuts of one
convex hollow source with certified whole-cap exterior/cavity clearance. Exact layer-domain
body-minus-union construction supplies actual material to M3/M4; no generic Boolean mesh is claimed.
Independent polygon radial/axial error bounds, canonical operation provenance, enclosed-cavity
diagnostics, conservative overlap rejection and atomic cancellation are implemented. Oblique axes,
through cuts ending outside the cavity and ambiguous/unsupported geometry fail explicitly. See
[specification](docs/M55C-DRAINAGE-SPEC.md), reference research
and [validation](docs/M55C-VALIDATION.md).
The bounded-domain gate passes complete Linux Debug/Release 793/0/2 and native Windows
Debug/Release 792/0/2, independent mask readers, final-source ASan and declared fixture budgets.

**Deliverables:** Manually place, edit and remove cuts with explicit position, diameter (mm),
orientation and defined cut extent. Subtract geometry or use an independently justified equivalent
construction; verify hole-to-cavity geometric connectivity and report failed/missed cuts, insufficient
wall clearance and possible undrained/enclosed void regions. Preserve operation order and provenance.

**Acceptance gate:** Analytic valid openings and missed/shallow/overlapping/insufficient-clearance
cases have independently verified geometry and connectivity results under stated tolerances;
identical inputs/settings reproduce results. Unknown connectivity is not reported as connected.
Geometric connectivity does not guarantee physical drainage, venting or resin removal.

### M5.5-D — Manual 3D Supports

**Implementation:** Safe-Rust parametric manual recipes generate explicit closed convex-loft
components from independently shaped/oriented sections. Rectangle, elliptical/regular/custom
polygon and terminal-point profiles support tapered, inclined, bent, horizontal and returning paths.
Immutable KSUP1 identity and bounded edit/undo/redo preserve KPREP1 compatibility; exact source keys
reject stale contacts. Conservative certificates validate platform/path connection, one intended
exterior-facet tip penetration and separation from unintended model/cavity/cutter contact. Explicit
cross-support union and exact prepared-material composition feed M3/M4/M5 without double exposure.
Sources remain one certified convex solid or the implemented bounded hollow/drain domain. Unknown
separation fails explicitly; arbitrary source meshes, smooth-curve/strength promises and final Boolean
mesh export are excluded. See the [specification](docs/M55D-SUPPORTS-SPEC.md) and the
[validation](docs/M55D-VALIDATION.md).
Complete Linux Debug/Release 828/0/2 and native Windows Debug/Release 827/0/2 pass, alongside
independent geometry/mask/machine readers, final-source ASan and declared preliminary fixture budgets.

**Deliverables:** Manually choose contact points; create, add, remove and edit support tip, shaft and
base parameters; define source/model/support/platform frames and stable edit/transform relationships.
Generate explicit support geometry with intended-contact versus collision diagnostics, connection
validation and a prepared-scene combination suitable for slicing supports together with the model.

**Acceptance gate:** Analytic contacts and platform connections, disconnected tips/bases, unintended
penetration, collision, invalid parameters and stale anchors are diagnosed deterministically. Model
and support material reaches M3/M4/M5 without losing parts, double exposure or unintended holes.
Neither contact geometry nor a successful slice certifies load-bearing strength. No automatic island
detection, automatic support generation/placement or optimization is needed for MVP acceptance.

### M5.5-E — Validation and Integration

**Status: COMPLETE for the documented bounded headless contract (TD-041).** Linux Debug/Release
852/0/2 and native Windows MSVC Debug/Release 851/0/2; independent combined/stress readers, four
ASan campaigns and declared fixture budgets pass. See [closure](docs/M55-CLOSURE-ACCEPTANCE.md).
M6 interactive and M7 release/hardware acceptance remain open.

**Deliverables:** Independent analytic models, geometry/topology invariants and error-budget checks;
canonical deterministic geometry/settings/diagnostic records; resource/cancellation, stress and
malformed-input tests; native Linux and Windows build/runtime evidence. Validate the prepared scene
through M3 slicing, M4 rasterization and M5 export, including combined hollowed/drained/supported
objects. Hand off headless editing, previews, diagnostics and progress/cancellation to M6.

**Acceptance gate:** PREP-001–007 and the headless portions of the planned preparation scenarios in
`docs/TEST-STRATEGY.md` pass on both native platforms within documented limits; source/history remain
intact after failure, cancellation and edits. Independent readers/analytic slices validate final output; unresolved geometry
is visible and cannot masquerade as complete validated output. Preserve or restore the last completed
revision without presenting it as current after an edit. No GUI is required for engine acceptance.

**Technical risks:** Robust offset/shell construction and cavity collapse; cavity/hole connectivity;
intersection/Boolean accuracy; model/support contact correctness; large intermediate geometry and
memory/time; reproducible scene editing and anchor invalidation; native-platform integration;
unvalidated physical drainage, strength and exposure behavior. Mitigation and remaining unsupported
cases must be explicit rather than delegated to automatic repair.

**Reference research:** Optional separately scoped behavioral comparison may inform named questions;
the owner decision and independent geometry define this requirement, not invented vendor evidence.
Existing M1 findings are retained; EXP-03/04 are optional compatibility research under TD-036.

### M5.5-F — Sampled-distance hollowing for non-convex solids

**Status (2026-10-18): implemented for the documented headless hollowing contract (TD-045).**
`preparation::sampled` hollows arbitrary closed edge-manifold solids, convex or not, including nested
shells and multiple bodies:
- exact distance with a BVH, exact even-odd parity, Lipschitz-pruned adaptive sampling,
  watertight marching tetrahedra, and a certified wall band;
- deterministic multithreading;
- exposed through `karasu-cli slice --hollow`.

See the [specification](docs/M55F-SAMPLED-HOLLOWING-SPEC.md).

TD-046 makes slicing of large meshes, including hollowed shells, result-identical and much faster:
face Z index, box-pruned planar contacts, filtered exact orientation.

TD-050 (2026-10-18, approved and implemented) adds winding-union layers, the CLI default, which
slice touching, overlapping and self-intersecting bodies without repair and equal the strict masks
on every strict-accepted input, plus byte-identical parallel layer generation and encoding on all
cores, span rasterization and exact sample classification:
[specification](docs/ENGINE-UNION-PARALLEL-SPEC.md).

TD-066 (2026-10-19, owner request) finishes hollowing before M6
([spec](docs/HOLLOW-SIMPLIFY-DRAIN-SPEC.md)):
- `--hollow-simplify <mm|auto>`: certified cavity simplification (deviation bound, walls never
  thinner);
- drain holes in any direction, and `--surface-drain-hole` perpendicular to the surface into the
  cavity.

TD-065 (2026-10-19, owner request) adds several models per plate ([spec](docs/PLATE-SPEC.md)):
- `--plate` documents of fixed and arranged objects, each with its own model, placement,
  hollowing, drain holes, supports and copies;
- `--copies`, `--arrange` and `--spacing` (default 2 mm) for a single model;
- deterministic shelf packing of support-inclusive footprints, union of overlapping objects.

TD-064 (2026-10-19, owner request) adds variable and adaptive layer heights
([spec](docs/LAYER-PLAN-SPEC.md)):
- `--layer-plan` Z ranges with fixed heights and explicit exposures;
- adaptive heights from the model's slopes, with exposures from a height → exposure table;
- exact integer-nanometre positions; calibrated bottom layers; SL1 refuses variable layers.

TD-063 (2026-10-19, owner request) adds output formats ([spec](docs/OUTPUT-FORMATS-SPEC.md)):
- plain CTB v3, v4 and v5 (encrypted CTB excluded by owner decision);
- the Anycubic Photon Workshop binary family (21 extensions, revisions 1–518, including `.pws`
  bit-planes);
- Prusa SL1 and SL1S;
- each one validated by an independent reader, and hardware unverified.

TD-062 (2026-10-19, owner request) adds manual supports on any model ([spec](docs/SUPPORTS-SPEC.md)):
- individually parameterised supports from presets, with tips, bends, and plate, model or
  support feet;
- braces and a raft;
- proven clearance, anchor, piercing, drain-hole and volume checks;
- interior supports in hollowed models;
- union with the model by winding, through `--supports`.

TD-061 (2026-10-19, owner request) exposes core features in the CLI:
- placement: `--rotate` (deterministic degrees), `--scale`, `--mirror`, exact `--center` and
  `--drop`;
- XY compensation in pixels, with a separate bottom-layer value against elephant's foot;
- resin volume, mass and cost in the summary.
Manual supports on real models need a core extension (TD-062).

TD-060 (2026-10-19, owner request) adds an opt-in minimum gray for antialiased edges
(`RasterConfig::minimum_gray`, `--min-gray`):
- partial coverage maps linearly onto `minimum..255` before level rounding;
- empty and full pixels are unchanged;
- outputs without the option are byte-identical.

TD-059 (2026-10-19, owner request) stores rendered model previews in every machine format:
- GOO thumbnails, CTB and Photon RLE15 previews, and PP1 RGB PNGs, in place of black, absent or
  gray mask projections;
- a deterministic headless CPU renderer of the sliced surface;
- layer data unchanged, at about 0.2 s per FX-12 job.

TD-058 (2026-10-19) parallelizes model import with exact first-occurrence welding on all cores,
with every output unchanged:
- FX-12 imports in 0.33 s from STL, 0.54–0.60 s from OBJ and 1.36–1.40 s from 3MF;
- the whole 16K job takes 2.44 s.

TD-057 (2026-10-19, owner request) brings OBJ and 3MF import forward from the STL-only MVP scope:
- one extensible format registry with explicit unit rules;
- OBJ ear-clipping triangulation;
- specification-conformant 3MF (components, mirroring, object types, required extensions,
  Production parts);
- one mesh across formats (research).

TD-056 (2026-10-19) parallelizes topology analysis with deterministic sorts and runs diagnosis
and repair concurrently:
- FX-12 at 16K takes 3.0 s, down from 4.8 s;
- FX-11 takes 0.35 s;
- every output is unchanged.

TD-055 (2026-10-19) holds every layer as nonzero row spans instead of a dense image, with
byte-identical output:
- FX-11 at 16K drops from 38 s and 1.5 GB to 0.5 s and 61 MB;
- karasu is now faster than the open-source slicers it was measured against on every case of
  that benchmark except binary FX-12.

Native Windows validation of TD-043–TD-054 passed on 2026-10-19: all gates pass, and CLI and
example outputs are byte-identical to Linux, except the PP1 `os` field
([record](docs/CLI-ENGINE-NATIVE-WINDOWS-VALIDATION.md)).

TD-054 (2026-10-18, approved; phases 1–3 implemented) specifies defective-mesh handling: lean diagnosis, rim-paired chord
closure of open sections under an explicit gap bound, exact duplicate and orientation repairs as
reported policies, hole filling for hollowing, and winding-rule hollowing
([specification](docs/OPEN-MESH-SPEC.md), research).
Phases 1 and 2 are in place:
- every slice reports mesh defects;
- holes are closed in each layer up to `--close-holes` (default 2 mm);
- duplicate faces and inconsistent orientation are repaired on a derived mesh (`--repair`,
  default `auto`).
FX-09 and FX-08 now slice like the clean cube, and FX-12 can be hollowed. Phase 3, approved
into the MVP, adds:
- winding-rule hollowing, which admits touching bodies and refuses overlaps;
- opt-in `--repair fill-holes`, recorded as the first operation of the preparation plan, so open
  models can be hollowed.

TD-053 (2026-10-18) supports large models: default import limits of 25 million triangles and
1.28 GiB, lean union admission, and duplicate detection and buffers sized for scale. A 21-million-triangle
model slices at 16K in 34 s with a 2.7 GB peak; the owner's FX-12 real model is a local test fixture.

TD-052 (2026-10-18) adds `--antialias 1|2|4|8|16` to the CLI: exact N × N supersampling with grey
at each format's native depth, the level recorded in GOO, CTB and PP1 metadata, and almost no cost
at 16K (research).

TD-051 (2026-10-18) measures PP1 scene metrics on the parallel encoder workers with run-based
labelling: a 1200-layer 16K PP1 export drops from 200 s to 18 s with identical bytes
(research).

TD-049 (2026-10-18) fixes the reduced code audit's findings
([audit](docs/CODE-AUDIT-2026-10-18.md)): P1 raster limits, swapped-panel headers, the placement
hint, publication on FAT32/exFAT media and smaller CLI/export items.

TD-048 (2026-10-18) fixes a silent layer defect found by a code audit: a layer plane exactly on a
horizontal step face returned the face instead of material. The CLI now samples such layers just
below the face, in the middle of the vertex-free gap within the same slab.

**M5.5-G (2026-10-18, TD-047):** drainage holes for sampled cavities are implemented.
- Openings are certified by exact contact tests and an exact straight escape segment (26
  directions) to the outside of the source's bounding box, so no convexity is needed. Entries without a free escape fall back to
  per-component parity after an embedded-source check, so an entry inside a sealed void is
  refused.
- Cavity regions are reported, including which ones remain enclosed.
- The drained solid is rasterized as shell minus cutters per sample.
- Exposed as `slice --drain-hole`.
- See the [specification](docs/M55G-SAMPLED-DRAINAGE-SPEC.md).

Not covered on 2026-10-18 within M5.5/M6, and what became of it (reviewed 2026-11-11):
- **Supports on sampled cavities:** done. Supports stand in a hollowed model's cavity, their
  tips on its ceiling and their feet on its floor (TD-108), with the cavity view and the
  cavity's own preset and density (TD-137); a tip on the ceiling over a drain hole warns
  (TD-131).
- **Oblique holes:** done. Drain holes take any direction (TD-066), turned with a handle
  (TD-083) or about one axis at a time (TD-120).
- **Certified cavity simplification:** done in the core and the command line (TD-066), and
  removed from the interface by TD-093, which certifies the wall directly.
- **Native Windows validation:** done, first with TD-066 on 2026-10-19 (every output identical
  to Linux), then by every native validation since (the last of 907f450).
- **Pocket policies:** still open. Minimum volume and keep-largest are not implemented: every
  cavity region, the smallest pockets included, is kept and reported. The regions that trap
  resin are located and shown (TD-101), and a drain-hole problem is a warning (TD-131). Open
  until the owner asks for one.

The convex backend and the A–E acceptance are unchanged.

**Definition of Done:** A–E satisfy the agreed input domain, error budgets, replay/immutability,
bounded execution and diagnostic contracts; required Linux/native Windows evidence passes and M3–M5
integration is verified. Algorithms remain callable without the GUI. M6 integration and M7 validation
are still mandatory before the expanded MVP is feature-complete or releasable.

## M6 — Desktop Application

**Status (2026-11-11):** M6.0 to M6.6 complete for their recorded scope (the foundations,
project and pipeline, viewport, core workflow, preparation with hollowing, drain holes, repair,
manual and automatic supports, suction cups and orientation, the layout and style, plates and
the project file). M6.7 (hardening and packaging) closed for now on 2026-11-10, its open items
listed under it; the licence was chosen since (TD-145). M6.8 (the visual review) in progress,
on the owner's points. Last native Windows validation: 907f450 (2026-11-11).

**Plan (TD-067, 2026-10-19):** [M6 plan](docs/M6-PLAN.md).
- **Phases:** foundations, project and shared pipeline, viewport, core workflow, preparation,
  layout and style, plates and project file, hardening and packaging.
- **Owner decisions:** the project → plates data model from the start; `egui_dock`; native file
  dialogs for now; embedded models in the project file (format open); the OpenGL fallback;
  code-signing feasibility to be studied.
- **M6.0 (2026-10-19, TD-068):** the desktop dependencies were reviewed and adopted. `karasu-app`
  opens the themed window with deterministic adapter selection and the OpenGL fallback;
  smoke-tested on Linux and native Windows.
- **M6.1 (2026-10-20, TD-070 to TD-072):** the `karasu-project` crate holds:
  - the slicing pipeline shared by the command line and the application;
  - the project model (embedded models, plates with their own printer, resin and settings,
    revisions) and background jobs;
  - supports that follow their object on the plate.

  CLI outputs are byte-identical.
- **M6.2 (2026-10-20, TD-073):** the wgpu viewport (plate, build volume, multisampled flat
  shading), opening by drop or command line, picking, and move, rotate and scale gizmos. FX-12
  (2.7 million triangles) orbits at 60 Hz on native Windows, on Vulkan and on OpenGL.
- **M6.3 (2026-10-20, TD-077, TD-078):** the core workflow: printer and resin presets and the
  user library, slicing with progress and Cancel, the layer panel, and export. The refinement
  after the owner's test adds the Repair menu, a diagnosis of every opened model, and messages
  with details and fixes in place of command-line errors. TD-079 tolerates, within an explicit
  bound per layer, the reversed slivers that self-intersecting folds leave.
- **M6.4a (2026-10-20, TD-081, TD-082):** undo and redo of plate edits, and live preparation
  previews: every object prepared in the background as slicing prepares it, with support
  findings kept for display.
- **M6.4b (2026-10-20, TD-083):** hollowing and drain holes from the interface: the preparation
  panel with what was certified, a section view of the cavity, and drain holes placed by clicking
  the model, which follow it when it moves.
- **M6.4b2 (2026-10-20, TD-084, TD-085):** drain holes are solids checked while they are placed
  (green or red, following the pointer), dragged along the surface by their solid, and widened,
  deepened and tilted with handles.
- **M6.4b4 (2026-10-20, TD-087, TD-088):** the owner's viewport review: the plate's
  front tab, layers shown as the plate, and the plate fading out from below with a border.
- **M6.4c (2026-10-21, TD-089, TD-090):** hollowing and drain holes
  rebuilt as operations on an untouched model, in its model frame: moving, turning or scaling
  never hollows again, slicing reuses the cavity, holes are independent, may overlap and are
  checked along their whole length, and each object keeps its operation list for the timeline.
  The interface applies and removes hollowing, places holes as drafts to apply, from the outer
  surface or the cavity, and shows applied holes as openings cut on the GPU.
- **M6.4d (2026-10-21, TD-092; r1 to r9):** mesh repair. Every repair, including aggressive ones up to
  rebuilding the surface; exact first, then tolerances the user sets; never silent, with every
  change and every remaining defect reported; tolerant hollowing; a guided flow from import to a
  printable model. See the [repair plan](docs/REPAIR-PLAN.md). r1 laid the foundations: the
  located diagnosis, self-intersections on request, located repair changes, the `Off` and `Exact`
  presets, the printability state, per-object repair operations, `--repair off|exact` and
  `inspect --diagnosis`. r2 added welding, degenerate-face repair and tolerant cracks, and the
  `Standard` preset. r3 made hollowing tolerant of zero-area defects and overlapping bodies. r4
  repairs non-manifold edges (fins) and adds the `Aggressive` preset. r5 closes large, irregular
  and non-simple holes with refined, faired caps. r6 removes small and inner shells and orients
  open parts by visibility. r7 rebuilds a model as a solid, the last resort. r8 brings it to the
  interface: the Repair section, auto-repair on import, markers and repair fixes. r9 validated it
  on a crafted corpus, performance budgets, the owner's models and native Windows, fixing four
  faults it found.
- **Next (owner, 2026-10-23):**
  1. **M6.4f:** a viewport review, on the owner's list of changes. First list: zoom towards
     the pointer, a deeper zoom with panning that keeps its pace, and a view cube with a home
     button. Then a selection outline, kept in the section view, with its colour and switch
     among the appearance settings.
  2. **M6.5:** the layout, style and interface review, brought forward before manual supports
     because the interface has grown cluttered. Started 2026-10-23: the
     [interface inventory](docs/UI-INVENTORY.md) and the actions layer with configurable
     shortcuts (TD-097, every user setting to persist); then the owner's layout (TD-098): a
     menu bar, a workflow bar of steps and tools, fixed panels at the viewport's right, and a
     closable left sidebar with the printer, the resin and the objects. Before going on
     (owner, 2026-10-26): the printer and resin profile review, with the catalog by vendor, the
     user's own printers and resins, and print settings separate from the resin (TD-106,
     accepted, implemented on 2026-10-26: the catalog in `assets/catalog`, the user's library
     with a one-time conversion, print settings on plates, `--print-setting`, and the
     interface of the approved mockup; parameter comparison).
     Then the settings store (u3, TD-097): every setting kept across restarts in a versioned
     file, the window as it closed, dialogs and tool windows at the size and place they were
     left; then the settings dialog (colours, shortcuts, repair on import, window, language,
     the file), with `--dev` for the pseudo-locale; then the live log window (TD-096).
     **Closed with reservations (owner, 2026-10-26):** the layout, the style, the panels, the
     profiles and the settings are done; reopened later for the sidebar's review for multiple
     plates and the models' state icons, the Slice and Layers panels if the owner wants them
     again, and the backlog (a model with defects tinted in the viewport, the CPU shared by
     diagnosis and preparation, the profile data still to decide, the orientation of printers
     the owner does not have).
  3. **M6.4g:** manual supports, preceded by the overhang analysis (owner, 2026-10-26): shown
     on the models, guiding manual supports, and later the base of automatic orientation and
     supports, whose scope (M6 or Post-MVP) is decided after the manual supports. The critical
     angle is 45° by default and a user setting. Adaptive cavity extraction is deferred after
     the MVP (its gain is performance, not the print). Research:
     overhangs and islands; decision TD-107 (slope
     tint on the model first; islands in the Supports panel, on demand).
     **The Supports step** (owner, mockup approved 2026-10-26, revised 2026-10-27; TD-108):
     supports placed with a preview and applied at once, edited afterwards; supports with
     errors print with a warning; supports kept in the model's frame so they follow it. Four
     tools: Supports (placing, editing, presets of the user's own), Islands, Braces and Raft
     (with the lift). The stages are all done (2026-10-30):
     - the core and pipeline, and the Supports panel;
     - the mockup of the four tools, and the handles;
     - the presets and the automatic foot;
     - Islands, Raft and Braces;
     - branches and trees of supports, with Merge trunks and Join a support.

     Native Windows validation passed on 2026-10-30 for bcbb38d (TD-108).
  4. **M6.4h:** the mandatory preparation scenarios, run from the interface (TD-111,
     [record](docs/M64H-SCENARIOS.md)). Done on 2026-10-30: the automated scenarios pass on Linux and
     native Windows with byte-identical exports, and the owner's interactive checklist passed on
     native Windows. Saving and reopening
     a project is not applicable until M6.6, so replay is checked through undo and redo.
  5. **M6.4i:** automatic supports, the essential version: overhangs covered as well as
     islands, tips spread over the downward surface by a density, and a closing check that adds
     tips where anything is left unsupported. It is built on the manual supports' planning,
     routing, trees and braces (TD-110). Implemented on 2026-10-30 as the Automatic tool
     (TD-112). The closing check is geometric: slicing again cannot tell coverage. Weight by
     load is a later phase, after M6.4j.
     **Mini supports** (owner, 2026-10-30, brought in before M6.4j; TD-113): a thin rod with a
     ball at each end, standing on the model or on another support, for the places a support
     does not fit. Placed by hand with the Manual tool's Mini button, and used by the Islands and
     Automatic tools where a standard support is refused. Implemented on 2026-10-30; on the
     owner's `HillGiant_32mm.stl` the places Automatic leaves go from 56 to 14.
  6. **M6.4j:** suction-cup detection, layer by layer in the print orientation (the owner's
     proposal of 2026-10-25, below), on the same per-layer analysis (TD-114,
     [specification](docs/SUCTION-CUP-SPEC.md)).
     - Owner's decisions (2026-10-31): the cups belong to the Drain holes tool; the smallest
       vent is an area the user sets.
     - Done on 2026-10-31:
       - j1, the research ([record](docs/research/SUCTION-CUP-RESEARCH.md)): on ten shapes,
         UVtools finds cups on the same layers;
       - j2, the core analysis on the exported masks and `slice --suction-cups [--min-vent]`.
     - j3 and j4, implemented on 2026-10-31 from the owner's mockup, await the owner's trial:
       - j3, the Drain holes tool: search, display filter, markers, navigator, list tab and the
         Slice panel's line;
       - j4, Add a vent here: under the cap towards the plate, or sideways on the plate;
       - after the owner's first trial, a cup's parts: one vent per lowest point.
     - j5: native Windows validation passed on 2026-11-02 (snapshot of c528394,
       `validate.ps1 -IncludeApp`): no failed command; Debug and Release each pass 1,541 tests
       with 14 ignored (Linux 1,543, the two Unix-only path tests); the three application smoke
       runs pass (Vulkan 12.1 ms, OpenGL 12.8 ms, FX-03 16.5 ms mean). Next: a physical print.
     - **Hollowing blockers** (owner, 2026-11-02, brought in before j5; TD-115): boxes whose
       bases may differ and spheres, kept solid by the hollowing, placed freely in the model
       frame from the Hollow tool and applied with Apply hollowing, with a Solid base shortcut.
       Implemented on 2026-11-02 (c528394), validated natively on Windows with j5; the mockup's
       size handles followed on 2026-11-03. Awaits the owner's trial.
     - **Native Windows validation of f78352a** (2026-11-03, `validate.ps1 -IncludeApp`): no
       failed command; Debug and Release each pass 1,551 tests with 14 ignored; the three
       application smoke runs pass (Vulkan 11.7 ms, OpenGL 13.3 ms, FX-03 16.4 ms mean). It
       covers the fitted contacts, the blockers' size handles, the PP1 fix and the resin colour
       and heating.
     - **PP1 files the P1 accepts** (2026-11-03, TD-117): the P1 refused karasu's first file
       sent to it, after its MD5 integrity check, and Photon Workshop refused it as corrupt.
       Tests in Photon Workshop found five pieces of `anycubic_photon_resins.pwsp` the readers
       require; the writer now records them, the firmware constants from Anycubic's sample by
       the owner's authorization. Photon Workshop opens the files and the P1 printed one to the
       end on 2026-11-03, resumed after a power cut; its time cannot check an estimate.
     - **Resin colour and heating temperature** (owner, 2026-11-03, TD-118): a resin's colour
       and swatch, so each colour has its own print settings, and a print setting's heating
       temperature, written to PP1 files.
     - **Drain holes cut only the model** (owner, 2026-11-03, TD-123): supports keep their
       material where they cross a hole, and are refused only when they enter its channel
       through the model, which they would plug. Settled on 2026-11-05 (owner, TD-131): that
       is a warning, so the automatic tools hold the islands a hole makes; braces stay out of
       holes.
     - **Tips kept off the model beside their contact** (owner, 2026-11-03, TD-124): a contact's
       cone must not cross the surface away from where it touches, routes lean aside and
       branches join lower to avoid it, and tips follow the surface's mean normal.
     - **Automatic supports never enter the model** (owner, 2026-11-04): they try tilted tips
       before going aside (TD-125), and mini supports they place keep off the model or are left
       out (TD-126), while manual ones stay tolerant.
     - **The lift moves to the Automatic tool** (owner, 2026-11-04, TD-128): 5 mm by default,
       applied by Generate; the Raft tool keeps the raft alone.
     - **Support density** (owner, 2026-11-04, TD-127): the Automatic tool's density (tips per
       area) and closer tips under wide undersides; the defaults await tuning by prints.
     - **Fixed time per layer** (owner, 2026-11-03, TD-122): an optional printer field added to
       the time estimate once per layer; the catalog's P1 takes 0.45 s (revision 2). To be
       checked against a print without interruption.
     - **Native Windows validation of b179c8c** (2026-11-04, `validate.ps1 -IncludeApp`): no
       failed command; Debug and Release each pass 1,579 tests with 16 ignored; the three
       application smoke runs pass (Vulkan 11.8 ms, OpenGL 12.8 ms, FX-03 16.5 ms mean). It
       covers drain holes cutting only the model, tips kept off the model, tilted and clear
       automatic supports, support density, the lift in the Automatic tool and the fixed time
       per layer.
     - **A route checks the tip as built** (owner, 2026-11-04, TD-129): with the tip towards the
       shaft, the straight and on-model ways check a tip straight down, within the axis limit,
       so a support on a sideways face is turned or goes aside instead of being built in error.
       Open: a pad on the plate beside a model resting on it is not in the route's clearance.
     - **Owner's trial** (2026-11-04): the automatic supports keep off the model, tilted or as
       minis, and the lift in the Automatic tool, the density with closer tips under wide
       undersides and the minis-only view work as intended.
     - **Contacts fitted to the surface** (owner, 2026-11-02, brought in before M6.4k; TD-116):
       an option of the Supports, Islands and Automatic tools that narrows a contact, with its
       penetration, down to a smallest diameter where it would hang over an edge or pass
       through a thin wall; it never widens. Implemented on 2026-11-02; awaits the owner's trial
       and a physical print. The Islands and Automatic tools also choose the contact's shape,
       Cone or Sphere, beside their preset.
  7. **M6.4k:** automatic orientation, scored with the overhangs, the suction cups and each
     layer's area. Implemented on 2026-11-03 from the owner's mockup (TD-119): the Orient tool in
     the Prepare step, constraints (tilt axes, most tilt from now, protected points), a rough
     pass over every allowed tilt, the best six checked as a slice would, five candidates
     previewed and applied as one step. Awaits the owner's trial.
     - **Drain holes turned about one axis** (owner, 2026-11-03; TD-120): the selected hole's
       turn gizmo, rings about the world's axes or the surface's, beside the free tilt handle.
       Implemented on 2026-11-03; awaits the owner's trial. A new diameter no longer turns a
       hole (TD-121).
     - **Native Windows validation of 3a6c39d** (2026-11-03, `validate.ps1 -IncludeApp`): no
       failed command; Debug and Release each pass 1,565 tests with 16 ignored; the three
       application smoke runs pass (Vulkan 12.8 ms, OpenGL 14.0 ms, FX-03 16.5 ms mean). It
       covers the Orient tool, the drain holes' turn gizmo and the kept axis on resize.
  8. **M6.6** plates and project file, then **M6.7** hardening and distribution, with the
     printer catalog widened alongside.
     - **The project file first** (owner, 2026-11-04, TD-130): a `.karasu` ZIP package with the
       project document and the models byte for byte; reopened, it recomputes cavities and
       previews; saved atomically, with a recovery copy every few minutes; sliced by the
       command line too. Then multiple plates in the application, from a mockup. The format,
       its checks and `slice --project` were committed on 2026-11-04 (25a648b); the
       application's New, Open, Open recent, Save, Save as, unsaved-changes question, title and
       recovery copy followed the same day, awaiting the owner's trial.
     - **Supports and drain holes, islands from supports** (owner, 2026-11-05): a support in a
       drain hole is a warning, and no automatic tip or foot stands where a hole cuts the model
       away (TD-131); "Hide islands under" hides islands from the list and the counts too; a
       model foot points up towards its shaft, so its shaft's bottom no longer hangs off it and
       starts in mid-air (TD-132). Awaiting the owner's trial.
     - **Smaller PP1 files and the plate's anti-aliasing** (owner, 2026-11-05): PP1 members
       deflated in the ZIP, as Photon Workshop and Lychee write them (TD-133: the venom torso
       413.6 → 252.5 MB); the Slice tool overriding the print setting's anti-aliasing and lowest
       grey for the plate, even turning anti-aliasing off, and binary supports by default
       (TD-134: 171.5 MB, sliced in 16.8 s instead of 29.4 s). Implemented on 2026-11-05;
       Photon Workshop opens the deflated file (owner, same day); the owner printed a deflated
       PP1 on the P1 on 2026-11-10.
     - **Print of the venom torso, v4** (owner, 2026-11-05, Photon P1, stored PP1, smart lift
       on): the hollowed, drained model printed whole, the flat cavity ceiling included; no
       failed layer, no detached support. The printer showed 6:21:07 against the 4 h 51 min
       karasu estimated: the time model leaves out about 2.5 s a layer on this printer (to be
       studied with its smart lift). The supports were hard to remove and left many marks: 1,964
       automatic Heavy supports (contact 0.6 mm sunk 0.3 mm, tip 1.4 mm, 1,378 of them branching
       from another support) and 2,603 braces, with nine islands supports of 0.15 mm. The
       presets' sizes, the density and the braces are to be reviewed from this print and test
       prints of our own: the reference slicer's file of the same torso was prepared by someone
       else, at Z = 0 with its raft fused to the model, and is not comparable (owner,
       2026-11-06). The published guidance is gathered in the
       [support presets research](docs/research/SUPPORT-PRESETS-RESEARCH.md) (2026-11-06): the
       preset sizes are within it; the braces (1.32 a support), the branches and Heavy on a
       hollow part are the likelier causes; a test plate is proposed.
     - **Native Windows validation of 7156099** (2026-11-05, `validate.ps1 -IncludeApp`): no
       failed command; Debug and Release each pass 1,611 tests with 17 ignored; the three
       application smoke runs pass (Vulkan 13.2 ms, OpenGL 14.4 ms, FX-03 16.4 ms mean). It
       covers the tip towards its shaft, the project file in the command line and the
       application, supports and drain holes, model feet towards their shaft, deflated PP1
       members and the plate's anti-aliasing with binary supports (TD-129 to TD-134).
     - **Several plates in the application** (owner, 2026-11-05, TD-135): from the approved
       mockup, a collapsible plates bar at the bottom of the viewport with top-view thumbnails, a Plate
       menu, the overview window, moving and copying models between plates, Export every
       plate, and Arrange with the overflow to new plates; undo across plates. In five phases.
       Phase 1 (the plates bar with thumbnails, the Plate menu, showing another plate and undo
       across plates) implemented on 2026-11-05, awaiting the owner's trial. Phase 2, the
       overview window (All plates…: a card per plate with a larger thumbnail, its profiles, its
       models' footprint and its last slice's layers, height, time and resin, with Open,
       Duplicate, Export and Delete, and the plates' totals), implemented the same day. Phase 3,
       moving and copying models between plates (the model row's menu, or the row dropped on a
       plate, Ctrl copying), implemented on 2026-11-06. Phases 4 (Export every plate, Slice every
       plate) and 5 (Arrange with the overflow), put off on 2026-11-06, implemented on
       2026-11-07; validated on native Windows with 779cc48.
     - **Native Windows validation of 599a9c6** (2026-11-06, `validate.ps1 -IncludeApp`): no
       failed command; Debug and Release each pass 1,624 tests with 17 ignored; the three
       application smoke runs pass (Vulkan 12.3 ms, OpenGL 14.1 ms, FX-03 16.5 ms mean). It
       covers TD-135's phases 1 to 3: the plates bar, the Plate menu, undo across plates, the
       overview, and moving and copying models between plates.
     - **Support presets from test prints** (owner, 2026-11-06, TD-136): two test plates of
       flat tiles on the Photon P1 (E-SUP-02, E-SUP-03 in the research). With the outer tips
       1.25 mm in, half the tiles fell and all curled; 0.4 mm in from the edges and at the
       corners, all twenty held flat, down to 2.1 % of their area in contacts. Decided: every
       contact sunk 0.1 mm, Medium 0.5 mm, Heavy on a 1.0 mm tip and a 1.2 mm shaft, the width
       at the surface shown, automatic braces at most 1.0 mm, the Automatic tool on Medium
       without braces, and its tips along each underside's outline, at its corners, and close
       to a leading edge, under it and on the face beside it, alternating (as supports placed
       by hand, and Lychee 8.0's, E-SUP-04). Implemented the same day; the next print is to
       show them on a real model.
     - **Placing aids and the cavity's supports** (owner, 2026-11-06, TD-137): a line around the
       model at the contact's height while a support is placed or its tip dragged, its colour a
       setting; Shift holds that height; the Automatic tool's cavity takes its own preset and
       density. Implemented the same day.
     - **Supports on a grid** (owner, 2026-11-06, TD-138): a Grid mode in the Supports tool; a
       click picks a flat underside, tips on a regular grid in its plane with the outline's
       tips of TD-136, straight to the plate or with branches as an option (off by default), the
       rows along the longest edge by default (to be tried in print). Implemented on
       2026-11-06 (the strip's Grid, B), with a rectangle or a lasso drawn on the screen as
       the button's other options.
     - **Native Windows validation of 975ffe5** (2026-11-07, `validate.ps1 -IncludeApp`, a clean
       snapshot of 692 files): no failed command; Debug and Release each pass 1,645 tests with
       18 ignored; the three application smoke runs pass (Vulkan 12.5 ms, OpenGL 12.9 ms, FX-03
       16.5 ms mean). It covers the support presets and the outline's tips (TD-136), the
       contact's height line and the cavity's own supports (TD-137), and supports on a grid with
       the rectangle and the lasso (TD-138).
     - **Native Windows validation of 779cc48** (2026-11-07, `validate.ps1 -IncludeApp`, a clean
       snapshot of 694 files): no failed command; Debug and Release each pass 1,656 tests with
       18 ignored; the three application smoke runs pass (Vulkan 13.7 ms, OpenGL 12.9 ms, FX-03
       16.4 ms mean). It covers TD-135's phases 4 and 5 (Arrange, Slice and Export every plate)
       and the slices' files kept per session. The run of 838563d before it failed one Debug
       test: three tests shared the ledge fixture's directory, which each removed on ending;
       779cc48 gives each its own.
     - **Mesh Booleans** (owner, 2026-11-07): a general, robust Boolean kernel in the core
       (difference, union, intersection of closed meshes), researched and decided first. Its
       first use is cutting the drain holes into exported models; cutting a model, joining two
       and lettering reuse it later, each from its own mockup. Researched on 2026-11-07; TD-140 (accepted 2026-11-07, design A) is a
       localized integer plane-based arrangement with winding-number vectors. Phases 1 to 4 are
       implemented (2026-11-07): the arithmetic, the arrangement, the closed output mesh, and
       the drain holes cut into a model's mesh (`HoleCuts::drill`), validated against slicing on
       a hollow sphere and the owner's torso (2.3 million triangles and 13 holes in 26.5 s).
       The output was written in `f64` (owner, 2026-11-07); phase 5 (owner, 2026-11-09) puts
       it on a grid of 2⁻¹⁵ mm, exact in every model file, rounding's crossings resolved in
       rounds (the torso: 8 rounds, nothing left, 73.5 s, vertices moved at most 7.8 µm).
     - **Model export** (owner, 2026-11-04; TD-139 proposed 2026-11-07 with its mockup): the
       model, the plate or every plate as binary STL, 3MF or OBJ, one file per plate or per
       model, supports and raft by choice, hollowed with the drain holes cut. Implemented on
       2026-11-09: the cut (TD-140 phases 4 and 5), the writers in the core, the assembly in
       `karasu-project`, `karasu-cli export-models`, and the dialog (File › Export models…, a
       model row's Export model…). Validated on native Windows with 7dddcf2 (2026-11-09).
       **Open (owner, 2026-11-09):** Lychee reports "structural issues" it cannot repair for the
       supports of an exported file (Bambu Studio reads them clean): 8,378 edges on the torso
       where two closed support parts touch along a ring of vertices. Accepted for now; the
       remedy studied is uniting each support's parts with the Boolean kernel (one shell per
       support), its export cost to be measured first (TD-139).
     - **Native Windows validation of 7dddcf2** (2026-11-09, `validate.ps1 -IncludeApp`, a clean
       snapshot of 713 files; rustc 1.99.0 MSVC): no failed command; Debug and Release each pass
       1,718 tests with 23 ignored (Linux 1,720: the Unix-only path tests); the three
       application smoke runs pass (Vulkan 13.7 ms, OpenGL 13.5 ms, FX-03 16.3 ms mean). It
       covers the mesh Boolean kernel (TD-140 phases 1 to 5, its memory, threads and
       cancellation) and the model export (TD-139), from 99a79c6 to 7dddcf2.
     - **Packaging prototype** (owner, 2026-11-09; TD-141): the application's executable is
       `karasu`; the owner's icon (window, shortcuts, menus) and splash artwork (the name now in
       the artwork, 720 × 405); a portable ZIP and a per-user Inno Setup installer for Windows
       (Start menu, uninstaller, `.karasu` association), a tar.gz with `install.sh` and a `.deb`
       for Linux, each with the third-party notices (`tools/packaging/`). Unsigned; the
       project's licence was chosen on 2026-11-11 (TD-145). The Linux tar.gz was installed into a test prefix and
       removed; the Windows installer (Inno Setup 6.7.3) was installed and uninstalled silently
       on native Windows. The owner's icon master v2 and splash artwork; the masters stay with
       the owner. The icon is in `karasu.exe` since 2026-11-09, a resource file the build script
       writes itself (no resource compiler, no new dependency).
     - **Native Windows validation of 856f712** (2026-11-09, `validate.ps1 -IncludeApp`, a clean
       snapshot of 728 files; rustc 1.99.0 MSVC): no failed command; Debug and Release each pass
       1,719 tests with 23 ignored (Linux 1,721: the Unix-only path tests); the three
       application smoke runs pass (Vulkan 12.4 ms, OpenGL 14.3 ms, FX-03 16.3 ms mean; splash
       1.51 s, opaque). It covers the packaging prototype (TD-141): the `karasu` executable, the
       window icon and the opaque splash.
     - **Accessibility** (owner, 2026-11-10; TD-142): analysed first (keyboard, focus, names,
       colour-only cues, contrast), then parts A1, A2 and A6 implemented: every control named
       for screen readers, one Tab stop per value field, sliders on the keyboard, a 2-point
       accent focus ring shown while the keyboard is in use, focused controls scrolled into
       view, notifications out of the Tab cycle under a modal, and an accessibility check over
       every tool and dialog in the tests (6a6d904). Then A3, A4 and A5: the 3D view out of the
       Tab cycle (no camera keys, owner), the tool strip before the panel, an opened tool window
       taking the focus; levels, states and refusals said with icons and words, not colour
       alone; the status colours configurable in Settings, with a warning for poor contrast.
       Then, by the owner on 2026-11-10: the controls' outline stays as it is (a lighter
       `#6E6A84` was proposed), and the manual screen-reader pass (A7) found no problem.
     - **Code-signing feasibility** (2026-11-10,
       [CODE-SIGNING-RESEARCH](docs/research/CODE-SIGNING-RESEARCH.md)): Windows releases should
       be signed before M7 (Smart App Control may block unsigned executables; EV no longer helps
       with SmartScreen); Linux releases get a signed `SHA256SUMS`. The route depends on who
       publishes (Azure Artifact Signing is open to EU organizations, not EU individuals) and on
       the licence (open-source routes are free or about €49). Decided: an individual, no signing
       for now; with the AGPL and a CLA, the open-source route is Certum Open Source (TD-145).
     - **Printer catalog, phase 1** (owner, 2026-11-10): 14 Anycubic printers whose formats
       karasu-slicer already wrote (Photon Zero, Mono, Ultra, M3, M3 Max, M3 Plus, M3 Premium,
       Mono X 6K, Mono X 6Ks, Mono X2, Mono 2, Mono 4, D2, Mono M5s Pro), from the official
       manuals, with Anycubic's Standard Resin settings for 13 of them (the Zero has none); 23
       printers in all. Orientation and printing not verified on the printers; the Photon X
       and Mono SE wait for readable official specifications. A test checks each Anycubic
       printer's format names it.
     - **Printer catalog, phase 2** (owner, 2026-11-10): 11 ELEGOO printers that read GOO (Mars 4,
       Mars 4 Max, Mars 4 Ultra, Mars 5, Mars 5 Ultra, Saturn 3, Saturn 3 Ultra, Saturn 4,
       Saturn 4 Ultra, Jupiter 2, Jupiter SE), from the official manuals; GOO stated for seven
       (official .goo test files or pages) and inferred for four from ELEGOO SatelLite, which
       writes only GOO. ELEGOO Standard Resin settings for ten (starting points from the middles
       of ELEGOO's ranges, the Saturn 3's official), two-stage lifts as ELEGOO's tables give
       them. 34 printers in all. Orientation and printing not verified; the Mars 4 DLP and the
       first Jupiter left out.
     - **ELEGOO orientation from ELEGOO's test files** (owner, 2026-11-10; TD-143): the same
       `ROOK.stl` exported by karasu-slicer against ELEGOO's `ROOK.goo` for four printers. None
       was mirrored; the Mars 4, Mars 4 Ultra and Saturn 3 were turned 180° and now mirror in
       X and Y (revision 2), the Saturn 3 Ultra already matched; all four verified.
     - **Left out of the catalog (owner, 2026-11-10):** the Prusa SL1 and SL1S until a PrusaSlicer
       export of the same model verifies their portrait, mirrored panel; UniFormation printers
       (GKtwo, GK3 Pro, GK3 Ultra), which by users' reports read only encrypted CTB and check
       files' integrity: not supported without a published format or UniFormation's
       permission. Their users can export prepared models and slice them elsewhere.
     - **Resin catalog** (owner, 2026-11-11; TD-146,
       [RESIN-CATALOG-RESEARCH](docs/research/RESIN-CATALOG-RESEARCH.md)): one entry per
       product, with densities. Anycubic's 16 resins sold, with the settings of its resin page
       (official, anti-aliasing included); ELEGOO's 23 resins of its table without the fast
       modes (starting points from the middles of its ranges, the manuals' bottom layers); SUNLU's
       16 resins as materials only, SUNLU publishing no settings for any printer. 55 resins and
       389 print settings. Adding a catalog printer offers its resins as a checklist with only the
       vendor's standard resin ticked. Anycubic's product pages, read only in archived captures,
       are to be re-read live.
     - **Native Windows validation of f1ba1b8** (2026-11-10, `validate.ps1 -IncludeApp`, a clean
       snapshot of 733 files; rustc 1.99.0 MSVC): no failed command; Debug and Release each pass
       1,729 tests with 23 ignored (Linux 1,731: the Unix-only path tests); the three
       application smoke runs pass (Vulkan 12.6 ms, OpenGL 12.5 ms, FX-03 16.5 ms mean; splash
       1.51 s, opaque). It covers the icon in `karasu.exe` (7e2ebf3) and the accessibility work
       (TD-142, 6a6d904 and f1ba1b8).
     - **Native Windows validation of c0890ff** (2026-11-10, `validate.ps1 -IncludeApp`, a clean
       snapshot of 792 files; rustc 1.99.0 MSVC): no failed command; Debug and Release each pass
       1,731 tests with 23 ignored (Linux 1,733: the Unix-only path tests); the three
       application smoke runs pass (Vulkan 13.2 ms, OpenGL 13.9 ms, FX-03 16.7 ms mean; splash
       1.51 s, opaque). It covers everything since f1ba1b8: the catalog's two phases and
       TD-143's ELEGOO orientations, the code-signing study, the documentation's reorganisation
       and `tools/goo-orientation`.
     - **M6.7 closed for now (owner, 2026-11-10)** with this validation. Decided since (owner,
       2026-11-10): no code signing for now, and an individual as the publisher when it comes;
       the controls' outline kept; the screen-reader pass done. Left open, to take up when their
       inputs arrive: the project's licence (decided 2026-11-11, TD-145); printer
       verifications (Photon Zero and Photon against Photon Workshop files of one model,
       the six ELEGOO printers without test files, the Prusa SL1/SL1S through PrusaSlicer);
       Anycubic formats not written yet (Mono 4 Ultra, Mono M7 series, possibly the P1 Max); the
       DragonFruit resin data decision.
  9. **M6.8** a further visual review of the interface (owner, 2026-11-05), before the release
     milestone (M7): the whole application looked at again, screen by screen, to optimise the
     layout, spacing, hierarchy, colours and icons of what M6 built, with the deferred UI/UX
     points gathered since M6.4d (a model with repair problems shown in the viewport, the
     layer viewer's size and lists, the wall map's colours). It changes how the interface looks
     and reads, not what it does. **No mockup (owner, 2026-11-10):** the interface is broadly
     right; the review takes the points the owner names, one by one.
     - **Owner points R1–R9 (2026-11-10):** no ellipsis on actions (R1); switches aligned with
       their labels (R2); panel sections as groups, the settings that depend on a switch hidden
       while it is off (R3); amber gizmo handles, a Settings colour (R4); tooltips that explain
       the parameter only (R5); the closed sidebar's margin and the plates' fold arrow always on
       the right (R6); the toast close button lit in its toast's colour (R7); a Mini filter in
       the supports list (R8); the supports' plate bases and the rafts fading out with the plate
       when the view is below it, the feet on the model staying (R9). Pushed in 535747a and
       8b35899; rules in [UI-DESIGN](docs/UI-DESIGN.md).
     - **Native Windows validation of 8b35899** (2026-11-10, `validate.ps1 -IncludeApp`, a clean
       snapshot of 792 files): no failed command; Debug and Release each pass 1,736 tests with
       23 ignored (Linux 1,738: the Unix-only path tests); the three application smoke runs pass
       (Vulkan 12.9 ms, OpenGL 13.3 ms, FX-03 16.7 ms mean; splash 1.51 s, opaque).
     - **Raft shapes (owner, 2026-11-10, TD-144):** in two phases. Phase 1, Skate (a1da402):
       the slab with its sides sloping in towards the plate (45° when chosen), the build failing
       with the foot's place when nothing would be left at the plate. Phase 2, Web: a disc under
       each foot and low bars between neighbours forming triangles without crossings (1,600
       feet: the support build 0.24 s against 0.12 s with a slab).
     - **Native Windows validation of 3bbc700** (2026-11-10, `validate.ps1 -IncludeApp`, a clean
       snapshot of 793 files): no failed command; Debug and Release each pass 1,741 tests with
       24 ignored (Linux 1,743: the Unix-only path tests); the three application smoke runs pass
       (Vulkan 13.2 ms, OpenGL 14.4 ms, FX-03 16.5 ms mean; splash 1.51 s, opaque). It covers
       the Skate and Web rafts.
     - **R10 (owner, 2026-11-10):** the section's keys (arrows, Page Up and Down, Home, End)
       move its active handle without the slider focused, while no other control holds the
       keyboard, and are set in Settings › Keyboard shortcuts › Section.
     - **Shortcuts settings (owner, 2026-11-10):** groups as the other settings' sections, names
       aligned left, and every action listed: the plates' (Ctrl+Page Up and Down among them),
       New project, Save, Export models and the tool strip's commands were missing.
     - **Move's key is T** (owner, 2026-11-10), as in other slicers and CAD programs; Tips only,
       which had T, takes Y; M is free.
     - **A model's menu in the viewport** (owner, 2026-11-10): a right click on a model selects it
       and opens the sidebar row's menu at the pointer.
     - **Clone** (owner, 2026-11-10): the model's menu adds 1 to 100 copies onto the plate shown,
       another or a new one, arranged around the models there (the overflow to new plates) or
       in the middle of the plate; on its own plate the model is arranged with its copies.
     - **Arrange from the middle** (owner, 2026-11-10): every arrangement (Arrange models,
       Clone, a model moved or copied to a plate, the command line's copies) places each model
       nearest the plate's middle beside those already there, the group growing outwards in a
       grid; shelves remain the fallback when they fit more (400 footprints in 82 ms).
     - **Checked models** (owner, 2026-11-10): check boxes on the sidebar's model rows, a bar
       to check or clear them all, and the checked models hidden, shown or deleted together.
     - **CTB named unencrypted (owner, 2026-11-11, 84dac6f):** the CTB formats read "CTB version
       2, unencrypted" to "CTB version 5, unencrypted" wherever a format is named, since
       karasu-slicer writes no encrypted CTB. An encrypted CTB that Lychee exported for a
       UniFormation GK3 (magic `0x12FD0107`) is not read: decrypting it would bypass its
       encryption.
     - **Native Windows validation of 84dac6f** (2026-11-11, `validate.ps1 -IncludeApp`, a clean
       snapshot of 794 files): no failed command; Debug and Release each pass 1,749 tests with
       25 ignored (the two Unix-only path tests run on Linux only); the three application smoke
       runs pass (Vulkan 13.5 ms, OpenGL 14.3 ms, FX-03 16.4 ms mean; splash 1.50 s, opaque). It
       covers everything since 3bbc700: R10, the shortcuts settings, the viewport's model menu,
       Clone, arranging from the middle, checked models and the CTB names.
     - **Licence (owner, 2026-11-11, TD-145):** AGPL-3.0-or-later, copyright Ivan Maruca; the
       name and logo are trademarks (`TRADEMARKS.md`); contributions under a CLA (draft); Help ›
       About karasu-slicer and `karasu-cli --version` show the notices; the packages carry the
       licence.
     - **Long profile menus (owner, 2026-11-11):** the sidebar's printer, resin and print setting
       menus scroll their list past 320 points, keep their actions (add, manage) below it and in
       view, and show a search field, focused as they open, once they list more than ten
       entries.
     - **Native Windows validation of 907f450** (2026-11-11, `validate.ps1 -IncludeApp`, a clean
       snapshot of 1,215 files; rustc 1.99.0 MSVC): no failed command; Debug and Release each
       pass 1,757 tests with 25 ignored (Linux 1,759: the two Unix-only path tests); the three
       application smoke runs pass (Vulkan 12.9 ms, OpenGL 12.4 ms, FX-03 16.4 ms mean; splash
       1.51 s, opaque). It covers everything since 84dac6f: the licence and Help › About
       (664337b), the long profile menus (1313a77) and the resin catalog (TD-146, 907f450).
     - **Printer thumbnails, deferred (owner, 2026-11-10):** a picture on each printer's card in
       the catalog and the library. Never the manufacturers' photos (proprietary assets). The
       options studied: a schematic drawn from the profile's own data (the plate to scale, the
       height, the panel), original illustrations per kind of printer, or photos of the owner's
       own printers; a mockup first.

  Moved into the MVP by the owner on 2026-10-30 (TD-110). Each feature needs physical print
  evidence before any claim that it prints.

  See [M6-PLAN](docs/M6-PLAN.md).
- **Persistent session logs (2026-10-23, TD-096):**
  - the application keeps one log file per session (the 10 newest, up to 20 MB each) for users
    to send with a problem;
  - panics are recorded with their backtrace;
  - the command line logs with `--log <file>`;
  - a ring of recent records serves the live log window (Windows › Log, 2026-10-26), with the
    logs folder and a diagnostic report that can hide the home folder.

  See the [plan](docs/LOGGING-PLAN.md).
- **Raster-side wall verifier (2026-10-23, TD-095, phase 1):** on demand
  (`slice --verify-walls`, or "Verify walls" in the layer panel), it measures the printed wall
  on the layer masks, finds breaches and undrained pockets, and maps them on the layers.
  - On `Base_01.stl` from 2.0 to 0.3 mm every printed wall is within one voxel of the
    certificate, and verifying adds 23 to 29 s.
  - Other slicers' files are a later phase. See the [plan](docs/RASTER-VERIFIER-PLAN.md).
- **M6.4e follow-up (2026-10-22, TD-094):** hollowing limits known in advance and sized to the
  machine. The interface warns below the predicted minimum wall, keeps the hollow and disables
  new holes when a cavity cannot be indexed, and sizes the memory budget from the installed
  memory. A leaner hole index makes every cavity that can be hollowed drillable. `Base_01.stl`
  now hollows and drills at 0.3 mm.
- **M6.4e (2026-10-22, TD-093, h1 to h5):** certified fast hollowing. The cell is chosen before
  hollowing from the wall and the surface area, the offset is raised by the predicted shortfall,
  and every cavity triangle's distance is proven exactly, so the requested wall is guaranteed,
  from the command line and the interface. Measured on 20 models from 0.5 to 2 mm: every
  hollowable run guaranteed in one pass, in at most 23 s; `Base_01.stl` at 1.3 mm in 4.9 s instead
  of 10.5 s, and drained. See
  the [plan](docs/HOLLOW-CERTIFIED-PLAN.md) and the
  [design comparison](docs/HOLLOW-CAVITY-DESIGN-COMPARISON.md). Manual supports follow (now M6.4g, after
  the viewport review M6.4f and the interface review M6.5), then the mandatory preparation
  scenarios (M6.4h).
- **M6.4b3 (2026-10-20, TD-086):** a section panel always at the viewport's right, with two
  handles snapped to layers; cut solids are capped, so a hollowed model shows its wall; hole
  solids are hidden behind the model and shown there as a faded ghost.
- **M6.3b (2026-10-20, TD-080):** the interface is prepared for localisation with Project
  Fluent: every text is in the `en-US` catalog, with a pseudo-locale and tests for catalogs,
  arguments and glyphs. Translations and the language setting come later.

**Stack decision (TD-042):** Rust with **egui**, **eframe** and **wgpu**, selected by the owner.
Dependency adoption, viewport prototyping and native validation remain M6 work. A focused custom
wgpu viewport sharing eframe resources is recommended for evaluation, not yet implemented.
Apply the owner-approved [color palette](docs/UI-DESIGN.md) through semantic theme tokens;
validate actual contrast, focus and non-color diagnostic cues during M6.

**Scope:** Build the cross-platform desktop application with the selected stack: model import, viewport,
transforms, settings, job progress/cancellation, layer preview, and export around the established core.
Include the mandatory M5.5 preparation workflow: hollowing settings/cavity preview; manual drainage
hole placement/editing; manual support placement/editing/removal; prepared geometry and diagnostics;
non-destructive edit/recompute and slice previews for the current prepared revision.

**Deliverables:** Implement and validate TD-042; native Windows/Linux application crate(s); accessible main
workflow; interactive 3D viewport; settings validation; support for core diagnostics, progress,
cancellation, preview, and export; packaging prototype. Add preparation controls and diagnostic/
selection overlays backed by headless core operations, with responsive background execution,
cancellation and stage progress. The GUI must not own the preparation algorithms or hide stale results.

**Dependencies:** Stable M2–M5 APIs and mandatory M5.5-A–E capabilities/handoff; PREP-008;
representative GPU/driver matrix; UX requirements; native build and signing environments;
audited GUI/rendering dependency tree.

**Technical risks:** Framework longevity, non-Rust native transitive components conflicting with
policy scope, GPU compatibility, UI/core coupling, large-scene responsiveness, accessibility, and
packaging differences. Preparation adds expensive recomputation, stable contact/hole selection,
history/anchor invalidation, consistent preview/slice revisions and background-job lifecycle risks.

**Acceptance criteria:** A user can import, inspect, position, configure, slice, preview, and export
through the GUI; operations remain cancellable/responsive; core stays usable headlessly; errors retain
actionable detail; native builds run on supported Windows/Linux systems. Users can hollow, place/edit
holes and add/edit/remove manual supports without changing the original STL, inspect cavity/contact/
clearance diagnostics, cancel recomputation, and slice/export the same prepared revision displayed.
Algorithms remain headless; background results cannot overwrite newer edits. If project persistence
exists, save/reopen includes preparation settings/history and source identity; no file format is
selected here and unconditional project persistence is not added by this revision.

**Automated tests:** Core remains covered independently; view-model/state tests; workflow integration
tests; snapshot tests used sparingly; renderer smoke tests; accessibility checks; packaging/install
smoke tests on both platforms. Add M5.5-to-GUI editing, revision invalidation, preview/diagnostic,
background cancellation/progress and conditional project reopen/recompute tests (PREP-006/008).

**Reference research:** Use reference applications only for workflow comparison and terminology where
lawful; design original UX and assets.

**Definition of Done:** Framework choice is justified; primary workflow functions on both platforms;
headless compatibility and deterministic processing remain intact; known GPU/UI limitations are
documented. Mandatory preparation workflows pass interactively on both native platforms; M5.5 is
not considered integrated merely because headless operations or isolated controls exist.

## M7 — MVP Validation

**Scope:** Validate correctness, reproducibility, interoperability, performance, memory, and native
cross-platform operation; close critical gaps and publish known limitations.

**Deliverables:** Validation matrix/report; corpus results; reproducibility hashes; CPU/peak-memory
profiles; printer-format compatibility evidence; native Windows/Linux build/runtime records; security
and dependency audit; release checklist; limitations.

**Dependencies:** Feature-complete M2–M6 MVP **including mandatory M5.5-A–E and its M6 integration**;
PREP-009; stable corpus and research scenarios; representative hardware; the distribution
decisions needed for release (the release model and code signing; the licence is decided:
AGPL-3.0-or-later, TD-145).

**Technical risks:** Late platform differences, corpus overfitting, insufficient hardware coverage,
performance regressions, dependency/license blockers, and unresolved malformed-input behavior.
Include shell/cut/contact failure, closed cavities, insufficient clearance, stale preparation state,
memory-heavy recomputation and limits on physical drainage/strength/print validation claims.

**Acceptance criteria:** All release-blocking tests pass; geometric deviations are within justified
bounds; identical inputs reproduce outputs; resource use meets declared budgets; target formats pass
independent validation; Windows/Linux workflows are demonstrated natively. M7 cannot pass with
M5.5 or its M6 workflow missing, deferred to Post-MVP, or demonstrated only by UI placeholders.
The following are mandatory end-to-end scenarios on native Windows and Linux:

- Import a solid STL, hollow it, manually add drainage holes, slice and export.
- Import a model, manually add/edit/remove supports, slice and export.
- Prepare and export a combined hollowed, drained and supported model.
- Edit preparation parameters and recompute without modifying the original STL/source mesh.
- Reopen/recompute saved preparation settings where project persistence is available; otherwise
  explicitly record that conditional case as not applicable and still test deterministic replay.
- Verify reproducible prepared geometry and sliced/exported output from pinned inputs/settings.
- Exercise invalid geometry, closed cavities, insufficient clearance and support connection failures;
  show actionable diagnostics and refuse unjustified success claims.

Physical print claims require separately authorized representative trials and recorded conditions;
analytic/geometric connectivity alone proves neither drainage nor support strength/printability.

**Automated tests:** Full unit/integration/property/regression suites, sustained fuzz campaigns,
benchmark comparisons, reproducibility tests, clean-machine builds, dependency/advisory checks, and
installer/application smoke tests. Include the preparation scenario matrix, source-preservation,
edit/cancel/retry, prepared-scene-to-M3/M4/M5 and M6 revision-consistency regressions. Conditional
persistence tests do not permit skipping in-memory history/replay or the other mandatory workflows.

**Reference research:** Re-run the controlled M1 scenarios on pinned reference versions and explain
meaningful differences. Do not define success solely as matching a commercial product.

**Definition of Done:** Evidence supports every MVP claim; no critical known correctness/data-loss/
safety defect remains; release constraints and unsupported cases are visible; owner approves the
distribution model and release decision. M5.5 engine and M6 preparation integration are included in
that evidence, with unsupported inputs and physical validation limitations prominently documented.

## Post-MVP — Advanced Geometry and Automation

**Scope:** Support optimization beyond M6.4i's essential automatic supports (load and peel
models, protected surfaces), orientation beyond M6.4k's, advanced mesh repair, generalized robust 3D Boolean workflows beyond
M5.5's required operations, advanced compensation beyond M4's baseline, additional machine formats
and other separately approved advanced functionality.
Each capability should be a separately gated milestone rather than one undifferentiated release.

Parametric hollowing, manual drainage holes and editable manual 3D supports are no longer exclusively
Post-MVP: they are mandatory in M5.5 and M6/M7. Offset/shell, intersection, subtraction or equivalent
geometry capabilities essential to those operations must be supplied and validated within M5.5,
even if a broader user-facing Boolean/repair workflow is deferred. Post-MVP cannot be used to defer
an internal MVP prerequisite. This revision does not require automatic island detection or support
placement, nor does it authorize silently repairing unsupported preparation input.

**Proposals recorded by the owner (not scheduled):**
- **Suction-cup detection** (2026-10-25; scheduled as M6.4j on 2026-10-30, TD-110; analysis
  and command line done on 2026-10-31, TD-114). A suction cup is a print-time hazard that depends on
  the orientation: at some layer, uncured resin is enclosed by the cured cross-section and capped
  towards the build plate by what is already printed, so the peel from the film pulls a vacuum.
  It differs from an undrained cavity region (TD-101), a property of the finished part: an
  undrained cavity is always a suction cup while printing, a drained one still is when its
  openings are only on the vat side, and an open concave shape facing the vat is one with no
  cavity at all. The analysis would run layer by layer in the print orientation, report each cup
  with its layers and the vent it lacks (an opening near the build-plate side of the cup), and
  locate it in the viewport with markers and Show, as the cavity regions are. It needs its own
  specification, research and print evidence; `docs/HOLLOW-SIMPLIFY-DRAIN-SPEC.md` and
  `docs/M55C-DRAINAGE-SPEC.md` keep it out of the drainage statement.
- **Pixel editor** (2026-10-31; Chitubox and UVtools have one, as behavioural references only).
  This edits the sliced layers, the last step before export, where the print is seen exactly as
  the printer will cure it. It would offer:
  - drawing or erasing pixels on one layer or a range of layers, with brushes of several shapes
    and sizes;
  - pixel supports: a column drawn from a layer down to the plate or to material below, to close
    an island left after the supports;
  - pixel drain holes and small fixes, such as burrs or pinholes.

  The proposed design, which needs its own specification and decision:
  - **A separate overlay:** the edits are added and removed pixels per layer, kept compactly and
    applied at export. Slicing stays deterministic, and the edits can be undone, reviewed and
    saved with the project (M6.6).
  - **Bound to one slice:** a new slice after a change to the model, its supports or the
    settings declares the edits stale. They are carried over only where the geometry is
    unchanged, and are never applied silently to a different raster.
  - **In the Layers tool (TD-103),** which already shows the layer images.
  - **Checked:** the island analysis runs again on the edited layers, so a closed island shows
    as closed.
  - **Measured:** a Photon P1 layer is 13312 × 5120 pixels (about 68 MB), so edits are kept
    sparse and only the layers touched are loaded.

**Deliverables:** Feature-specific algorithms and interfaces, safety/diagnostic UX, extended owned
corpus, format integrations, and updated resource budgets.

**Dependencies:** Validated MVP pipeline; feature-specific mathematical research; real print evidence;
explicit priorities; audited dependencies.

**Technical risks:** Topology-changing repairs, trapped resin, structurally insufficient supports,
unprintable cavities, Boolean robustness, heuristic unpredictability, and misleading automation.

**Acceptance criteria:** Every operation previews/reports topology-changing effects; automated choices
are reproducible and explainable enough to diagnose; manual overrides remain possible; print trials
and analytic/regression tests support claims.

**Automated tests:** Feature-specific analytic/property/regression/fuzz suites, structural heuristics,
collision and drainage cases, deterministic optimization, printer-format compatibility, and resource
benchmarks.

**Reference research:** Investigate externally observable behavior only for named requirements;
combine it with publications, computational-geometry literature, and original experimental design.

**Definition of Done:** A feature meets its independent specification and safety/correctness gate,
integrates without weakening MVP guarantees, is documented on both platforms, and has no unresolved
critical failure in its intended operating envelope.

