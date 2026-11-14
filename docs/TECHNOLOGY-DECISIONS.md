# Technology Decisions

This document records confirmed project decisions, their consequences, and unresolved choices. A decision
may be revised only with written rationale and migration impact.

Current research scope: TD-036 supersedes earlier references to EXP-03/04 as mandatory gates.
Their protocols and historical evidence remain retained; neither experiment is marked completed.

## Confirmed decisions

### TD-001: Native Rust processing core

**Status:** Accepted

The application core and geometry/slicing engine use native Rust. C/C++ geometry kernels, wrappers
around them, and bundled foreign kernels built by Cargo scripts are not allowed. This preserves
memory-safety goals and a coherent cross-platform codebase but may require implementing specialized
algorithms directly.

### TD-002: Small Cargo workspace

**Status:** Accepted

Start with `karasu-core` and `karasu-cli`. Logical modules stay in the core crate until independent
versioning, dependency isolation, compile-time cost, or ownership provides concrete evidence for a
new crate. This limits abstraction and build overhead.

### TD-003: Headless core and deferred GUI

**Status:** Accepted

The core has no GUI dependency. GUI implementation and dependency installation belong to M6,
after processing interfaces are validated through the CLI and tests. TD-042 records the subsequent
owner-selected Rust/egui/eframe/wgpu stack; the core remains independent.

### TD-004: Stable Rust and safe code by default

**Status:** Accepted

Use stable Rust, Cargo, rustfmt, Clippy, and standard tests. Workspace lints currently forbid
`unsafe`. A future need for `unsafe` requires an explicit decision, documented invariants, a small
auditable boundary, and targeted tests.

### TD-005: Geometric baseline

**Status:** Accepted for initial design; validate in M1–M3

Use millimeters, right-handed world coordinates, Z upward from the build plate, `f64` principal
geometry, purpose-specific tolerances, right-hand-rule triangle orientation, and deterministic
ordering. Integer polygon and LCD coordinates are distinct from continuous geometry. Detailed
conventions are in `ARCHITECTURE.md`.

### TD-006: Correctness before performance

**Status:** Accepted

Prioritize geometric correctness, robustness, memory safety, determinism, maintainability, and only
then measured performance. Optimizations require benchmarks and regression evidence.

### TD-007: Independent reference research

**Status:** Accepted

Other slicers can inform behavioural specifications and interoperability tests, in clean room
(AGENTS.md, "Clean-room development"). Raw evidence remains outside the repository.

### TD-008: English-only repository

**Status:** Accepted

All project artifacts, code, documentation, tests, diagnostics, and version-control text use English.

### TD-009: M2.1 immutable indexed source mesh

**Status:** Accepted for M2.1

Use native standard-library `f64` point/vector values, one vertex buffer, `u32` triangle indices,
immutable geometry access, optional cached source bounds, and separate STL metadata. Manual
construction validates without welding; STL deduplicates exact finite coordinate bits in source
order, normalizing signed zero only. No proximity merge or geometric repair occurs. Reject
unrepresentable bound extents rather than publish infinite derived dimensions. Distinct point/vector
types and minimal implemented operations suffice; no linear-algebra dependency is needed.

STL shared indices describe exact positional identity, not proof of manifold connectivity. M2.2
transforms/world bounds follow TD-012 below; full topology remains M2.3. Original normals and
opaque attributes cost a separate metadata record per facet, preserving evidence without retaining
a redundant triangle-soup geometry copy. Revisit storage only with measured workloads.

### TD-010: Explicit bounded STL import contract

**Status:** Accepted for M2.1

Require an explicit mm-assumption policy for unitless STL; never infer units/placement from a header,
printer, or GUI. Auto detection prefers exact binary count/length agreement, then a textual `solid`
opener, then binary-looking input; explicit format selection resolves caller intent. Never hide a
failed parser with speculative fallback. Reject binary trailing data, preserve opaque attribute
bits, allow explicit empty meshes, and support a strict documented single-solid, line-oriented ASCII
grammar. Retain bad stored normals as diagnosed metadata, but reject non-finite vertex geometry.

Resource caps and fallible allocations are part of the public import contract. Default input cap
64 MiB and one-million-facet cap cover the owned corpus without permitting unbounded file counts;
callers may tighten them. These are application safety defaults, not mathematical geometry limits or
a total-RSS guarantee. No third-party production or test dependency is added.

### TD-011: Bounded claims for M2.1 diagnostics

**Status:** Accepted for initial diagnostics; robust predicates remain open

Retain all degenerate/duplicate faces and preserve winding. Exact repeated-position detection is
distinct from a zero computed cross product. The latter is explicitly a floating-point observation,
not a robust exact collinearity predicate or a near-degeneracy test. Source-order diagnostics and
first-occurrence duplicate IDs are deterministic independent of hash-table iteration. No validity,
watertightness, or manifold claim follows from an empty diagnostic list. Edge incidence, winding
consistency, and more robust numerical classification require their own M2.3/M3 design/tests.

### TD-012: M2.2 explicit pivoted transforms and column-vector composition

**Status:** Accepted for M2.2; independent project convention, not a proprietary-behavior inference

Preserve M2.1 source meshes. Use local pivot `c`, local-axis scale `S`, active right-handed rotation
`R`, and last-applied world translation `t`: `p_world = t + c + R*(S*(p_local-c))`. Euler convenience
accepts radians only and uses fixed-axis extrinsic X then Y then Z, `Rz*Ry*Rx`. Store validated
normalized unit quaternions, with maximum-component norm scaling and renormalized compositions.
Non-finite and exactly zero quaternion/axis inputs are errors; no near-zero epsilon is introduced.

Use a compact 3×3-plus-translation affine type rather than a general linear-algebra framework.
For column vectors, chronological `a.then(b)` represents matrix `B*A`. Store coefficients row-major,
evaluate multiplication in a fixed order, and retain general affine results rather than discard
shear or guess an object decomposition. Object flattening is explicit; direct pivot evaluation and
affine evaluation can differ in rounding/range. No automatic fallback, inverse, or baking is offered.

All finite scale factors, including zero/negative, are explicit supported input. Zero can collapse
geometry; an odd number of negative nonzero factors reverses orientation. Source winding/normals
remain unchanged. Reject non-finite inputs, arithmetic intermediates, outputs, and overflowing bound
extents; accept ordinary finite rounding/underflow without claiming exact classification. No global
tolerance changes mathematical transform intent. Direction transformations omit translation/pivot
and are not inverse-transpose normal transformations.

Compute tight bounds over actual transformed vertices in O(n) time and constant scratch space,
including unused manual vertices consistently with source bounds. An eight-corner source-AABB
transform is not used as tight mesh bounds. Preserve metadata, indices, source bounds/coordinates,
and source diagnostics even when a derived operation fails. Error context includes the original
vertex index. Placement/printer limits, normal conversion, inversion, topology, and native Windows
validation remain separate work. No third-party dependency or proprietary algorithm is introduced.

### TD-013: Derived source-index topology with complete vertex links

**Status:** Accepted for M2.3; independent native-Rust design, no dependency added

Keep immutable source mesh storage and M2.2 transforms unchanged. Build a separate, bounded,
fallibly allocated topology report with sorted undirected edge keys and original directed face uses.
Index identity is authoritative; no proximity matching, additional welding, or automatic repair.
Retain duplicate face incidences and existing coordinate-based diagnostics. Exclude existing
computed-degenerate faces explicitly from surface incidences, preserve their source-face incidence,
and mark all touched vertices indeterminate rather than infer a healthy partial neighborhood.

Expose both edge-connected and vertex-connected usable triangle partitions. Edge incidence alone
cannot recognize vertex-only closed-shell contacts; use complete simple path/cycle links with
multiplicity to establish vertex-manifoldness. Iterative disjoint sets and sorted flat membership
avoid recursive traversal and pairwise high-degree adjacency. Reports have source-correlated,
deterministic ordering and O(V+F+E) storage. Default source caps track STL import defaults; checked
capacity arithmetic and fallible allocations are not a hard peak-RSS budget.

Keep no-boundary, edge-manifold, vertex-manifold, supplied local winding, and closed-combinatorial
findings separate. Known failures override indeterminate checks; empty checks are not applicable.
Isolated vertices preclude whole-mesh vertex-manifoldness. Closed combinatorial surfaces need not
have consistent supplied winding or one component; they are not verified embedded/oriented solids.
No global reorientation/orientability solver, outwardness, self-intersection, nesting, volume,
thickness, printability, or repair behavior is implied. Numerical exclusion continues TD-011's
floating-point limitations. Sustained fuzzing, peak-memory profiling, cancellation, and native
Windows validation remain independent work. See architecture and tests for exact contracts.

### TD-014: Borrowed synchronous mesh cancellation with atomic results

**Status:** Accepted for M2 Closure A

Add cancellable entry points without changing existing signatures/options, immutable geometry, or
dependencies. A `Cancellation` controller exclusively borrows a phase-tagged `FnMut(Checkpoint) -> bool`
predicate on the invoking thread. No internal synchronization, threading, async jobs, signals, or
progress API is introduced. Caller-owned flags and synchronization remain caller responsibilities.
Ordinary entry points use a no-predicate controller. Predicates must return promptly without panic.

Poll before work, on documented record/chunk/phase boundaries, and before success, including empty
results. Return a typed cancellation variant with checkpoint context and no successful partial value;
flatten subordinate cancellation into the outer cancellation variant. Preserve resource/error causes
and ordinary behavior when cancellation is never requested. A request observed at a checkpoint wins
there; errors already encountered before another poll are not replaced. Post-read polls precede
inspection of the read result. A request after the final observed check need not prevent success.

Use 256-record polling and at-most-8-KiB byte chunks, including high-incidence inner loops. Retain
existing standard-library sorts/deduplication with before/after checks; do not claim interruption
inside a sort, blocking read/open, allocation/initialization, hash operation, string scan, bulk copy,
or cleanup. These are explicit phase-level limitations, not strict elapsed-time latency guarantees.
`Interrupted` remains retryable. Reader consumption cannot be rolled back. Deterministic tests use
checkpoint predicates/counters, never sleeps; checkpoint counts are not stable across releases.
See architecture for the full contract and source/metadata-preservation guarantees.

At the Closure A decision point, M2 remained open for CLI inspection, transform round-trip coverage,
and applicable cross-platform validation. Those items subsequently passed at accepted M2 baseline
`d8816303bbc4c2ba26213645ddf278db6574342b`. M3–M5 scheduling/progress is not pulled into this
decision. No profiling/fuzz campaign was rerun for Closure A; potential callback/poll overhead is
unmeasured.

### TD-015: Exact represented-value horizontal-plane classification

**Status:** Accepted in M3.0-B; implemented in M3.1, stable Linux/WSL and native Windows validated

Use direct `<`, `==`, and `>` on validated finite `f64` vertex Z and explicit finite plane Z.
Classification has zero tolerance, works for finite extremes/subnormals without subtraction, and
treats signed zeros as equal. It is exact for stored values, not pre-conversion real intent or
unrounded transformed geometry. Defer caller-supplied tolerance bands to a separately specified
higher-level policy. No vendor constant or general orientation-predicate dependency is adopted.

The normative scope, truth table, API and tests are in
[M3-TRIANGLE-PLANE-CONTRACT.md](M3-TRIANGLE-PLANE-CONTRACT.md). The f64 decision is independent.
EXP-03/04 remain open.

### TD-016: Checked small-weight intersection construction

**Status:** Accepted in M3.0-B; implemented in M3.1, stable Linux/WSL and native Windows validated

Copy exact on-plane vertices. For strict crossings, orient by Z, form checked positive plane
distances, and derive the smaller interpolation weight using a distance ratio and denominator
between one and two. Avoid the full edge-Z difference or distance sum, which can overflow.
Reject non-finite distances and parameters outside the contract's normal-positive domain with
typed errors. Use the specified sign-dependent XY convex interpolation, finite checks and explicit
closed-endpoint-interval range projection; set Z to the requested plane and normalize zero.

The detailed operation sequence and independently derived absolute forward bound
`64 * 2^-53 * max(|endpoint_axis|) + 16 * 2^-1074` mm are normative in the specification. XY
underflow/coincident represented endpoints remain explicit possibilities. This is an approximate
construction with a bounded rejection domain, not an exact-real result or universal relative-error
claim. No silent endpoint fallback, tolerance, proximity welding, adaptive precision, or new
dependency is introduced. Implementation must validate the arithmetic argument against independent
oracles and supported Windows/Linux targets.

### TD-017: Raw local incidence events with preserved provenance

**Status:** Accepted in M3.0-B; implemented in M3.1, stable Linux/WSL and native Windows validated

Intersect one supplied three-point closed convex hull with one supplied horizontal plane in O(1)
time/stack space. Expose `geometry::section::intersect_triangle_z([Point3; 3], f64)` alongside its
implementation/tests. Distinguish no intersection, tangent point, strict-crossing segment, coplanar
edge and raw coplanar triangle. Keep endpoint coincidence and source degeneracy observations
independent of the incidence variant; retain both endpoint origins when their coordinates agree.
A coplanar triangle is not a certified positive-area face. Exact repeated-position flags and a
qualified computed-cross observation preserve degeneracy context without inventing an exact-rank
guarantee that M2 does not provide.

Retain local corner/edge IDs and sides. Canonical coordinate construction/order is invariant under
corner permutations; provenance comparisons remap local identities and retain multiplicity. No
normalization may erase source identity. Return owned fixed-size records and structured numerical
errors; do not add a traversal, cancellation/progress/resource controller, mesh repair, CLI feature,
layer schedule, contour, polygon or raster API to this constant-size primitive.

Shared coplanar ownership and segment multiplicity belong to M3.2; final layer scheduling remains
dependent on EXP-03 and tangent/coplanar compatibility on EXP-04. Integer polygon representation and
scale remain a separate decision before M3.4. These decisions establish only the first M3.1 design
gate and do not close M1 or complete M3.

### TD-018: Raw supplied-world-plane full scan with source incidence and atomic cancellation

**Status:** Implemented for M3.2-A; stable Linux/WSL and subsequent consolidated native Windows
validation passed. See the [validation record](TEST-STRATEGY.md#subsequent-consolidated-native-windows-m32m33-validation)
for owner-supplied Release evidence provenance.

Expose one core-only raw mesh section in `slicing`, using the unchanged M3.1 local kernel. Borrow the
immutable `TriangleMesh`, require a finite explicit world Z and identity/object/affine mapping, and
scan every face in source order without filtering, caches or topology exclusions. Direct object
evaluation never implicitly flattens to affine. Negative and singular transforms remain supported
M2 mappings; preserve original winding and inspect degeneracy after checked world-point computation.
Unused source vertices are not section input. Transform and kernel failures remain separate contextual
typed causes, not repaired geometry. Source mesh storage, M2 APIs and TD-015–017 semantics do not change.

Retain every non-None raw event with its source face and original index triple. Resolve local
corners/edges into mesh indices on demand, preserving local identity, collapsed pairs, coincident
disconnected features and distinct face multiplicity. Omit only None storage while counting all
faces and qualified area observations. Do not let canonical coordinate order imply winding,
shared-feature ownership, a positive area, an embedded solid or a closed/filled contour.

M3-specific face/event/requested-payload-byte caps, checked arithmetic and fallible geometric event
buffer growth bound work/storage requests without a topology-cap dependency or worst-case upfront
allocation. Requested payload is not total RSS; allocator overhead, transient reallocation and
uninterruptible allocation/copy/drop phases remain explicit. Fixed summary counters replace unbounded
diagnostic logs. Extend TD-014 with start, 256-face, storage-growth and final publication checkpoints;
errors/cancellation never publish a successful partial section. No thread/dependency/progress API.

This intentionally defers ownership, deduplication, welding, contours, polygon scaling and layer
sampling to their own increments. Existing M3.0-C evidence supports contrasting coordinate-based and
source-incidence connectivity and motivates adversarial tests; no proprietary constants, routines,
filters or repair behavior are adopted. The full scan is a future optimization correctness reference,
not an accelerated or benchmarked slicer. EXP-03/04 and M1 remain open; this does not close M3.2/M3.

### TD-019: Non-destructive source-feature normalization and conservative local patch ownership

**Status:** Design recorded before M3.2-B algorithm implementation; now implemented and stable
Linux/WSL validated. Combined native Windows validation subsequently passed in the
[consolidated campaign](TEST-STRATEGY.md#subsequent-consolidated-native-windows-m32m33-validation).

Normalize a borrowed complete M3.2-A `MeshSection`; retain `raw()` unchanged. Endpoint identity is
`Vertex(index)` or `Edge(min_index,max_index)`, never a coordinate bin. A transformed collapse does
not change its source feature. Preserve every coordinate observation, raw event index and local
origin. Group observations by feature; exact normalized coordinate disagreement is a structured
conflict, with no chosen representative. Independently group exact coordinates to expose coincidence
of different features; proximity and general geometric overlap are not connectivity.

Each crossing/coplanar-edge event contributes one segment occurrence; a coplanar triangle contributes
its three original directed edges and three shared corner observations; tangents contribute only
point observations. Canonical segment keys are sorted feature pairs. Keep all occurrences, source
face/event references and reversal relative to the incoming payload, including duplicates and zero
length. Sorted index arrays supply groups/ranges without per-occurrence heap allocations. Raw source
winding remains authoritative, not canonical endpoint ordering. No even/odd cancellation occurs.

A source-topological edge is locally internal to a coplanar patch only for exactly two occurrences
from distinct, nonduplicate indexed triangles, with distinct edge/third indices, agreed vertex
coordinates, opposite directed edge uses, and exact nonzero XY orientations of the two third vertices
on opposite sides of the shared edge. The exact sign uses independently authored fixed-size integer
arithmetic on represented finite binary64 dyadics, with no coordinate quantization or dependency.
This certifies the local two-triangle patch only, not global embedding/outwardness/printability.
For coplanar triangles the exact represented-coordinate sign independently verifies nondegeneracy;
it may certify a patch even when M3.1's qualified computed-cross observation is zero/unavailable.
The raw context is preserved, not revised or treated as an exact rank certificate. Crossing/on-edge
events with degenerate or unassessed raw context remain unresolved candidates.
Keep both internal occurrences; classify them out of the local patch-boundary candidate class only.
One verified nondegenerate coplanar face yields a local patch-boundary candidate. Mixed coplanar/
noncoplanar incidences, on-plane noncoplanar edges, duplicates/opposite duplicates, more than two
incident faces, inconsistent winding, overlapping half-planes, collapse, coordinate conflict and
unassessed/degenerate context remain structured unresolved groups, not assumed filled boundaries.

Use independent normalization event/incidence/aggregate-requested-byte caps, checked sizes and
fallible flat buffers. Poll entry, both event passes, group inner loops, before/after each sort and
allocation, and final publication through additive TD-014 checkpoints. Sort/allocation/drop and
bounded exact-sign calculations are indivisible; no latency, RSS, progress or threading guarantee.
Malformed provenance and nonfinite/off-plane observations are typed fatal errors; finite conflicting
observations and geometric ambiguities are retained as distinct machine-readable diagnostics.
M3.0-C motivates source identity and adversarial tests, not vendor routines, scales or cleanup.
EXP-04 compatibility, M3.3 branch/contour interpretation and global overlap analysis remain open.

### TD-020: Owner-approved expanded MVP and M5.5 headless preparation gate

**Status:** Accepted scope decision, 2026-10-16; preparation capabilities are planned, not implemented.
**Source:** Owner's “Expanded MVP Roadmap Revision,” not new commercial observations or M1 evidence.

Parametric hollowing, manually placed/editable drainage holes and manually placed/editable 3D supports
are mandatory MVP capabilities. Add M5.5 — Model Preparation Engine between existing M5 and M6;
retain M0–M7, accepted M2, current M3.2-B evidence/status, unresolved EXP-03/04 and M1 acceptance.
M6 must integrate preparation interactively; M7 is not feature-complete without M5.5 and that M6
workflow. The new requirements are PREP-001–009 in the functional-requirements document.

M5.5-A defines immutable source/derived semantics, operation order, frames/anchors, validity domains,
numeric/approximation budgets, replay/versioning, resource/cancellation/progress contracts, algorithm
evaluation and analytic tests. B implements hollowing, C manual drainage, D manual supports, and E
validates headless native Windows/Linux and M3/M4/M5 integration before functional handoff to M6.
Development order is not runtime order: prepared geometry feeds slicing, raster and export.
Non-destructive reversibility means source-preserving history/recompute, not a geometry inverse.
In-memory operation history/replay is mandatory; saved-project reopen/recompute is required where
a persistence mechanism exists, without selecting a file format or a new persistence subsystem now.

No voxel, SDF, offset-surface, Boolean, mesh-fusion or multi-component strategy is preselected.
Evaluate independent native-Rust alternatives and any dependencies under the existing review policy.
Necessary shell/intersection/subtraction or equivalent construction/material-combination work belongs
to M5.5; generalized Boolean/repair workflows may stay Post-MVP but cannot hide an essential MVP
dependency. Automatic supports, advanced island/overhang optimization and auto-orientation stay
Post-MVP. MVP still includes M4's existing raster compensation; advanced extensions alone are deferred.

The engine remains headless. M6 owns responsive background orchestration, editing/preview/status,
not preparation algorithms. Prepared/source identities and revisions, support-to-model/platform
relationships, collision versus intended contact, cavity/hole connectivity and diagnostics must be
specified and tested. Geometric connectivity is not physical drainage; connection is not strength;
closed combinatorics is not an embedded/printable solid. Unsupported/indeterminate inputs and
construction failure need explicit outcomes, not universal success or implicit repair claims.

This expands geometric complexity, memory/time and integration/validation scope without a calendar
estimate. Existing M3 geometry contracts, engineering priorities and native platform gates are not
weakened. Implementation requires its separately scoped milestone work; this decision adds no code,
dependency, toolchain or new reference experiment and does not mark any preparation increment complete.

### TD-021: Borrowed source-feature incidence graph with separate candidate adjacency

**Status:** Implemented for M3.3-A; stable Linux/WSL and subsequent consolidated native Windows
validation passed. See the [validation record](TEST-STRATEGY.md#subsequent-consolidated-native-windows-m32m33-validation).

Build only from an immutable, privately constructed M3.2-B `NormalizedSection`. Canonical vertex
IDs equal lexicographic feature-group indices; exact On corners use indexed Vertex identity and
strict crossings use unordered Edge identity, even when rounding coincides with a vertex. There is
no additional vertex/edge equivalence, coordinate/proximity welding or interior-intersection splitting.
Conflicting observations have no representative; original groups, issue bits and raw provenance stay
borrowed and unchanged. No mesh rescan or new geometric predicate is needed.

One complete graph record per canonical segment group retains all parallel raw occurrence IDs through
the borrowed normalization index range. This is grouped incidence, not destructive edge deduplication.
Complete adjacency retains internal/unresolved/self-loop ends. Candidate adjacency includes only
TD-019 CrossingCandidate/CoplanarPatchBoundary groups, not verified material boundaries. Internal
coplanar groups and all unresolved groups are excluded from that view but remain inspectable.
Local degree two cannot establish an unambiguous contour, especially with unresolved adjacent groups.

Use five flat fallibly reserved buffers: vertices, edges, complete adjacency, candidate adjacency,
diagnostics. Sorted `(vertex, edge, endpoint-slot)` entries and per-vertex ranges give CSR-like
traversal without one allocation per incidence. Unique group-edge degree, eligible incidence-end
degree and raw occurrence-end multiplicity are separate; self-loops have one unique edge and two
ends. Diagnostic order is feature-coordinate conflicts, unresolved groups, then candidate degree
one/greater-than-two vertices, each in canonical ID order. These are potential endpoints/branches,
not path routing or loop acceptance. Face-order permutations remap raw face/occurrence IDs rather
than promising unchanged source IDs. Debug formatting is not a serialization contract.

Cap vertices, groups, borrowed occurrence references, aggregate adjacency ends, actual diagnostics,
and requested buffer bytes. Diagnostic capacity is conservatively min(2V+unresolved groups, cap);
counts/bytes use checked usize/isize arithmetic. Extend TD-014 with graph entry, record, storage,
sort, adjacency, diagnostic and final checkpoints; do not publish partial success. Finite conflicts
and representable ambiguity are structured data, distinct from resource/allocation/cancellation errors.
Allocation/sort/drop phases remain indivisible. No runtime, dependency, performance gain, RSS limit
or progress/latency guarantee is introduced. At this graph-only checkpoint, path/branch/open/ambiguity
interpretation was deferred to M3.3-B (TD-022 below), while geometric orientation remains M3.4 work.
The then-pending M3.2-A/B and M3.3-A native gates subsequently passed in the consolidated campaign.

### TD-022: Conservative graph-path decomposition with canonical identity traversal

**Status:** Implemented for M3.3-B; stable Linux/WSL and subsequent consolidated native Windows
validation passed. See the [validation record](TEST-STRATEGY.md#subsequent-consolidated-native-windows-m32m33-validation).

Consume the existing immutable M3.3-A graph, not a new connectivity construction. A candidate group
is consumed exactly once, independent of raw occurrence multiplicity. Each complete group has exactly
one final outcome: path/offset, local internal coplanar exclusion, or unresolved. All original graph,
normalization and raw source references stay borrowed; no coordinate buffer, occurrence suppression,
welding or ownership reclassification is introduced. Any unconsumed candidate on final accounting
is an operational inconsistency, not quietly omitted output.

Stop continuation at candidate degree !=2, finite coordinate conflict or any adjacent complete-only
unresolved group. Branches seed independent maximal nonbranching trails, not routes through the
junction. Discover components on candidate adjacency only, ordered by minimum vertex; complete-only
connections do not join components. Ordinary clean degree-one paths are OpenChain; wholly clean
degree-two cycles with at least three distinct graph vertices are ClosedCycleCandidate; boundary-
touched trails are AmbiguousTrail, including actual graph-closed returns to a branch/uncertain vertex.
Internal patch diagonals alone do not block continuity and never become path edges. Existing loops,
collapsed/duplicate/parallel occurrences stay unresolved under TD-019/021. Separate parallel candidate
groups/self-loop candidates contradict the current graph contract; defensive checks reject fabricated
reclassifications rather than bypass ownership to make those cases ordinary paths.

Canonical open starts use the smaller endpoint ID. Closed walks omit the repeated terminal vertex,
rotate to their unique minimum graph vertex and select the smaller outgoing edge ID; graph-simple
trail construction guarantees that minimum is unique. Path records sort by (first vertex, first edge),
a unique key because every group is consumed once. This is canonical identity direction, not geometric
winding. A closed ambiguous walk may be rotated away from its original boundary start; its issues
and explicit vertex diagnostics still identify the unresolved junction, not a resolved route.

No graph cycle is certified as a simple polygon or filled boundary. XY self-intersection, overlap,
zero area, outwardness, region nesting/holes and printability remain unassessed; M3.4 must define
those claims and numerical policy separately. EXP-04 compatibility and EXP-03 layer sampling stay
open; neither commercial heuristics nor signed area select a path direction here.

Use flat path/vertex/edge/component/outcome/diagnostic buffers and bounded per-vertex state/DFS stack.
Checked aggregate requested bytes include all eight buffers, even scratch later dropped. Actual path,
reference, component and diagnostic caps are separate from conservative reservation bounds. Poll
input validation, component/high-degree scans, traversal, canonicalization, allocation/sort boundaries,
diagnostics and final publication using TD-014; failures/cancellation publish no partial success.
Counts/capacity/index inconsistencies are typed, while representable uncertainty stays diagnostic.
No dependencies, threads, async runtime, polygon engine, RSS guarantee or benchmark claim is added.

## Approved M3.4 detection-only staging decision

### TD-023: Detection-only represented-coordinate validity and retained polygon-grid gate

**Status:** Owner approved on 2026-10-16, after research commit `b1b58ba`. Implemented at the
M3.4-A detection-only scope; stable Linux/WSL validated. Native Windows validation is recorded
separately in the test strategy. This is a narrow staging exception, not a new polygon-grid contract.

The independent [planar-validity design](M3-PLANAR-VALIDITY-DESIGN.md) separates path embedding,
cross-path contact and material-region acceptance. Use a bounded pairwise reference over
borrowed `SectionPaths`, using exact signs on represented finite f64 XY, direct interval comparisons
and explicit collapsed/contact/overlap diagnostics, without constructing new intersections. Preserve
source-feature identity, canonical path order, raw multiplicity, complete-only uncertainty and
immutable upstream results. Open/ambiguous trails cannot become rings through coordinate equality.

The existing private fixed-limb sign is shared by normalization and detection without changing its
arithmetic or tests; this is not a general-purpose arithmetic abstraction. Area/sign, geometric winding, nesting,
Boolean constructions, layer policy and a physical integer grid remain separate decisions. Exact
predicates on rounded endpoints do not recover ideal unrounded source intersections. No global
epsilon, repair, snapping, splitting, automatic closure, thread or new dependency is selected.

The roadmap's integer polygon representation/scale gate remains **OPEN**. No authorized physical
error/range/topology-change budget presently justifies a grid. The owner explicitly approved the
narrow unquantized detection reference before quantized polygon construction/regions, while
retaining the full grid/rounding/intermediate-range gate for those stages. This supersedes only the
timing of the original pre-M3.4 gate for detection-only work, not its requirements for conversion,
construction or regions. The supplied-plane kernel's mathematical contract remains byte-identical.

Compare exact-predicate and integer alternatives, pairwise and sweep/index approaches, and later
exact versus error-bounded area strategies using the proposal's proofs and independent tests.
Return distinct geometric findings and operational errors; budget pair work and diagnostics as well
as requested storage, with TD-014 cancellation and no successful partial report. Timing, RSS,
progress, full-region correctness and native portability require their own implementation evidence.

`slicing::validity` exposes ordinary/cancellation-aware `inspect_planar_paths` and an immutable
`PlanarInspection`. Canonical edge references retain path/offset and resolve graph/source provenance
through the borrowed input. Exact singleton roles, overlaps/coincidence and collapse are distinct.
Within-path adjacency permits only the prescribed endpoint; all cross-path contacts are retained
without changing local cycle simplicity. Open/ambiguous paths remain ineligible. Unassessed internal,
unresolved and isolated context is counted explicitly. No printable/material shortcut is exposed.

M1/EXP-03/04, original M3 acceptance and mandatory M5.5/M6/M7 preparation commitments are unchanged.

## Approved M3.4 measurement-only staging decision

### TD-024: Exact cycle area and traversal orientation, without material acceptance

**Status:** Owner approved on 2026-10-16 after the research/design report. Implemented as M3.4-B;
platform validation is recorded separately in the test strategy. The owner subsequently requested
the implementation commit `1c4f52e` before the next research/design increment; that commit does
not approve a physical grid or further implementation.

The research and
[contract](M3-AREA-ORIENTATION-CONTRACT.md) specify measuring only existing SimpleCycle embeddings
through an immutable borrowed inspection. Preserve canonical IDs/traversal, all provenance,
cross-path contacts and complete-only uncertainty. Skip open, ambiguous and non-simple paths
explicitly; do not derive material roles, repair or reverse geometry, sum union area, or repeat
contact detection. Exact-zero area for a claimed SimpleCycle is a typed consistency failure.

Independently encode finite represented f64 coordinates as dyadic integers. A narrowly private
134-u32-limb magnitude plus sign covers every partial shoelace sum through checked N<=u64::MAX;
retain the existing orientation predicate unchanged. Native u128 mantissa products are split into
bounded limb fragments. Exact sign determines current traversal orientation. One integer-derived
nearest-even binary64 conversion returns positive finite mm^2, RoundedToZero or Overflow; the last
two retain orientation and are complete representation outcomes, not successful zero/infinity
scalars or operational errors. This does not recover ideal source geometry before rounding.

This explicitly settles the earlier design's requested exact-versus-checked-compensated comparison
as approved: a rigorous estimate must also bound product/range error and certify rounding or fall
back. Ordinary compensation is insufficient; exact full-domain accumulation provides the initial
correctness reference without an unmeasured filter/fallback. A later proven, measured filter remains
possible. No dependency, physical quantization or general-purpose big-integer API is selected.

Approval extends staging only to **nonconstructive measurement** before a physical grid is
chosen. It does not waive the retained integer representation/scale, physical error, range and
topology-change gate before conversion, polygon construction or regions. EXP-03 final sampling,
EXP-04 compatibility, material/nesting, remaining M3 acceptance and M1 are unchanged.

Public APIs, count/work/byte caps, TD-014 atomic cancellation, structured errors, independent
rounding/rational tests and separate stable/native validation are specified in the contract. They
are scoped acceptance conditions, not claims of material validity or performance. Existing upstream
contracts, exact orientation arithmetic and cancellation cadence remain unchanged. No further
increment, physical grid, construction, material role or region acceptance is authorized.

## M3.4 integer-coordinate decision — approved ExactOnly implementation

### TD-025: Explicit job-configured binary grid and ExactOnly first coordinate view

**Status:** Owner approved the explicit binary-grid/ExactOnly first-view direction and worked
arithmetic domain on 2026-10-16, then authorized design finalization. The owner subsequently
authorized implementing the bounded contract after design commit `75acecd`. The first view is
implemented; actual platform acceptance is recorded separately in the test strategy. A later owner
request authorizes committing the validated implementation. TD-023/024's retained construction/material gate remains binding;
no positive physical loss allowance is selected.

The [independent contract](M3-INTEGER-GRID-CONTRACT.md) and
historical comparative evidence specify explicit origin/step/domain
metadata for each job rather than a hidden universal scale. An initial ExactOnly coordinate view
would accept an agreed represented-world-XY point only if exact arithmetic proves equality with
the recorded grid point and its integer coordinate is in range. Off-grid/range/operational errors
publish no view. Borrow the immutable planar inspection, retain feature IDs, unresolved conflicts
and source provenance, and do not weld, repair or imply polygon/material validity.

The approved coordinate domain uses h=2^k mm, -1074<=k<=1023, finite represented-f64 origins and
i64 indices |q|<=2^52. The accompanying N<=3,000,000 illustration concerns later per-cycle arithmetic,
not a new scan/area certificate in the coordinate view. Independent bounds cover exact
forward differences in 66 u32 magnitude limbs plus sign, a possible later exact inverse in 68,
i128 orientation and every product-by-product area partial state. These are separate arithmetic
proofs, not existing Rust code or permission to reuse a predicate's private capacity unchecked.
The same domain still admits a proper intersection with a reduced 156-bit numerator: rational
construction safety is NOT established. No step k, mm envelope, positive error budget, minimum
gap/feature policy, constructor, dependency, topology certificate or material rule is selected.

Sections 8–11 define the implemented API: `slicing::grid::BinaryGridSpec`,
ordinary/cancellable `map_exact_grid_vertices`, a borrowed `ExactGridView` and one exact-or-unresolved
outcome per original graph vertex. The spec has private validated fields, canonical metadata zero,
no default origin/k and no lossy mode. A private two-array 66-u32-limb forward helper checks exact
divisibility before quotient range; no nearest-even rounder, integer area/predicate or inverse
helper is added. The pass borrows complete context without quantizing conflict observations.

`GridLimits` defaults are 3,000,000 input vertices, 6,000,000 planned agreed-axis conversions and
512 MiB requested outcome payload. One flat fallible reservation follows metadata/count/byte
preflight. Fixed arithmetic stack and borrowed input/allocator overhead are outside requested
bytes; no RSS promise. GridStart/Input/Storage/Records/Coordinates/Complete checkpoints follow TD-014,
with 256-record/global-axis cadence and no successful partial result. Off-grid/range errors identify
the original vertex and X/Y axis; invalid metadata, consistency, count/capacity, allocation,
defensive arithmetic and sourced cancellation failures are distinct. The contract fixes precedence.

Keep the first view O(V+66*A) for V vertices/A agreed axes; no pair rescan or inverse decoder.
ExactOnly can retain its original agreed f64 after exact equality; no moved geometry exists.
BoundedNearest conversion requires a separate positive physical/error and changed-topology policy.
Mixed-grid operations, inverse decoding and constructions require their own metadata/arithmetic/
acceptance proofs. Metric error alone does not certify contact, nesting, holes or isotopy.

Implementation acceptance requires independent exact conversion/boundary oracles, retained unresolved
context/IDs, immutable deterministic output, checked count/work/byte caps, atomic TD-014 cancellation
and actual Linux/native Windows validation. The executed finite research models validate the named
mathematical cases only; separate Rust integration/private/doctest results validate the implementation.
M1/EXP-03/04, M3 regions/scheduling and all later milestone commitments remain open. Construction
needs its own reviewed algorithm/growth/output policy; the subsequent implementation commit request
does not authorize the next increment.

## M3.5-A accepted decision and later completion proposals

The comparative research and
[completion specification](M3-COMPLETION-SPECIFICATION.md) originally defined three independent
owner gates. The owner approved and authorized the first narrow implementation under TD-026;
the 2026-10-17 request separately authorizes scheduling under TD-027. Constructive overlay remains
a proposal. A subsequent 2026-10-17 request authorizes M3.6 composition under TD-029, without
authorizing overlay.

### TD-026: Strict exact region hierarchy

**Status:** Owner approved and implemented as M3.5-A. The detailed public contract is
[M3-REGION-HIERARCHY-CONTRACT.md](M3-REGION-HIERARCHY-CONTRACT.md).

Build an exact containment forest only over M3.3 `ClosedCycleCandidate` paths whose M3.4-A embedding
is `SimpleCycle`, whose M3.4-B outcome is measured, whose referenced TD-025 coordinates are exact,
and which have no retained cross-path contact. Use an inclusive-AABB rejection followed by a
positive-X half-open-Y integer ray with exact `i128` side products. The approved `EvenOddStrict`
rule assigns even-depth cycles as material outers and their direct odd-depth children as holes.

The result preserves one deterministic outcome per original path, path provenance, current
orientation, parent/depth/role and a nonmutating reverse-view flag. Open, ambiguous, non-simple,
coordinate-conflicted and contact-involved paths remain explicit non-boundaries. Unresolved
complete-only context, grid conflicts and contacts yield successful `GeometricallyUnresolved` data,
not repair or a material/printability certificate. No closure, welding, construction, source-solid
claim, dependency or thread is introduced. TD-014 adds the eight `Regions*` checkpoints.

### TD-027: Explicit exact-dyadic uniform scheduling

**Status:** Owner authorized autonomous research, independent specification and M3.5-B implementation
on 2026-10-17. This supersedes P-M3-027. See the
[finalized contract](M3-LAYER-SCHEDULE-CONTRACT.md).

Require a caller-supplied finite first plane and positive finite height plus exactly one `Count`,
`Before` or `Through` terminal rule. Evaluate `first + index * height` exactly, compare terminal
boundaries before rounding, publish once-rounded nearest-even binary64 Z and contiguous `u64`
indices, and reject nonfinite or duplicate outputs atomically. No first-plane/profile default is
selected; EXP-03 remains the compatibility-default gate. Neither reference product establishes
a portable ordinary scheduler contract through the bounded static study.

An independent 34-u64-limb signed magnitude uses eta=2^-1074 units. Finite inputs have magnitude
<2^2098; multiplication by a u64 index plus signed first plane stays <2^2163, within 2176 bits.
At most 65 exact probes derive terminal count; generation uses exact integer addition, avoiding
per-plane multiplication without accumulated floating error. Direct floating evaluation and FMA
alone fail the full-u64 index domain; dynamic big integers are unnecessary. No dependency is added.

The bounded owned result deliberately validates every plane before publication rather than exposing
a lazy stream that can fail after earlier planes escape. Explicit layer and requested-payload caps,
checked host capacities and one fallible reservation bound output. M3.6 still owns geometry streaming,
progress, job lifecycle and aggregate collection. Five `Schedule*` cancellation checkpoints follow
TD-014. Native Windows, whole-M3 performance budgets and sustained fuzz acceptance remain separate.

### P-M3-028: Conditional exact constructive overlay

**Subsequent status:** Resolved by the owner-authorized remaining-M3 request under TD-030 below.
The paragraph below preserves the original decision gate.

Before construction, decide whether crossing closed shells belong to the agreed M3 corpus and define
source-material union versus parity. If required, use a separately reviewed exact arrangement or an
approved dependency with full audit. Proper intersections already exceed i128-only rational storage
under TD-025's domain. Endpoint touch, collinear overlap and coincidence remain typed ambiguity until
their ownership policy is approved; lossy grid movement remains prohibited without a positive
physical/topology budget.

### TD-029: Owned strict layer stream, progress and atomic collection

**Status:** Owner authorized reference research, independent design and M3.6 implementation
on 2026-10-17. See the [contract](M3-LAYER-ENGINE-CONTRACT.md). P-M3-028 remains a separate unapproved overlay gate.

Compose the existing eight cancellation-aware stages serially over an immutable source/transform,
validated TD-027 schedule, explicit exact XY grid/fill rule and reviewed stage caps. Publish one
independent owned `RegionLayer` per successful item. Preserve raw events, all observations and
occurrences, source-feature identities, complete group ownership/multiplicity, path and region
outcomes, diagnostics and stage summaries. Flat owned buffers avoid self references and per-path
allocations; derived adjacency and coordinate lookup indexes are not copied. Successful geometric
ambiguity remains `GeometricallyUnresolved`, not an operational failure or accepted material.

An operational failure yields one contextual typed error and fuses the stream. Earlier items remain
individually complete; normal exhaustion and its final cancellation check establish stream
completion. Advisory synchronous named-stage progress counts completed layers exactly, separately
from cancellation and without imported commercial percentage weights. No callback panic catching,
async runtime, thread, foreign kernel, dependency or geometry cache is introduced.

The optional atomic collector consumes a fresh engine only. It preflights known count and header
storage, charges owned layer payload against an aggregate cap, and defers Completed notification
until exhaustion and final collection publication checks succeed. A late failure drops partial
collection output. Per-stage caps, projection payload and collection payload are distinct from total
RSS; intermediate reports overlap projection and a generated item can precede aggregate rejection.
Nine additional orchestration/projection/collection checkpoints follow TD-014. M5 owns file atomicity.

Independent M3.6 nested-layer tests exposed an existing TD-026 implementation defect: the ray
eligibility condition checked the lower Y endpoint but omitted the strict upper endpoint. Subdivided
collinear sides could therefore toggle parity twice and lose an island. The correction enforces the
already specified half-open Y straddle, without changing the accepted fill or coordinate contract.
Analytic subdivided rectangles (both orientations/rotations) and multilayer shell/hole/island tests
retain the regression. Historical validation counts remain historical; changed region code requires
fresh native-platform validation.

### TD-030: Explicit source-material exact arrangement

**Status:** Owner authorized remaining-M3 audit, reference research, independent specification and
implementation on 2026-10-17. This resolves P-M3-028 for the required exact composition operation.
See the [audit](M3-COMPLETION-AUDIT.md), [contract](M3-CONSTRUCTIVE-MATERIAL-CONTRACT.md) and
research.

Accept individually simple closed integer rings in the TD-025 domain, each with a unique source
path and explicit material-source ID. Apply parity within a source and union across sources.
Intersect original segments only, split at crossings/contact/overlap endpoints, retain all atomic
supports, and classify exact rational midpoints with symbolic infinitesimal left/right normals.
No floating offset, rounding, snapping, inferred ownership or silent cleanup is permitted.

Eight-u64-limb signed arithmetic covers original homogeneous intersections, rational comparison,
midpoint and symbolic supporting-line predicates, conservatively below 512 bits in the approved
domain. Checked overflow remains typed. Output coordinates remain rational grid values without
f64 reconstruction; homogeneous representations need not be gcd-reduced. Do not sum unrestricted
rational polygon areas. Material-left halfedge walks are split at repeated vertices into
edge-disjoint cycles; exact local turns and symbolic geometric-interior containment derive roles
and the parent forest, allowing shared contact vertices explicitly.

All arrangement atoms retain source path/edge support and explicit left/right material disposition,
including internal/cancelled atoms. Work/count/capacity budgets and TD-014 checkpoints make errors
atomic. The initial all-pairs/linear-index implementation is bounded and independently tested;
optimization requires profiling. No production dependency, foreign kernel or unsafe code is added.

The engine opt-in requires one source ID per original face and consistent ownership across each
eligible path. It preserves the complete strict layer and attaches separate constructed material
before publication, charging retained bytes against the existing per-layer/collector caps. Open,
ambiguous, self-contacting, off-grid and unresolved-source cases are rejected rather than repaired.
Strict configuration literals and previous behavior remain compatible. General Boolean command
families, lossy error budgets and physical defaults are not selected by this decision.

## TD-031 — Explicit represented-coordinate regions and lossless dyadic composition

**Status:** Accepted for implementation by the owner's 2026-10-17 request to resolve `ExactOnly`.
This introduces no dependency.

Ordinary approximate intersections need not land on an integer lattice, which motivates
decoupling contour topology from integer conversion. The [contract](M3-REPRESENTED-COORDINATE-CONTRACT.md) specifies an explicit
engine builder, honest per-vertex outcomes/mode metadata, shared exact represented-coordinate
containment, and lossless common dyadic encoding for material union. The old integer API and its
2^52 bound stay intact; a private 2^60 constructor domain has a <496-bit symbolic-expression proof.
Unsupported dynamic range fails explicitly. No snapping, tolerance, repair, foreign code or
proprietary reconstruction is added. Existing physical defaults and M3 acceptance gates remain open.

## TD-032 — Coherent shared-source-edge plane crossings and M3 acceptance corpus

**Status:** Accepted under the owner's 2026-10-17 instruction to finish M3. The initial whole-corpus
replay exposes unresolved equators in both sphere resolutions, leading to the independent local
classification documented in [M3 closure acceptance](M3-CLOSURE-ACCEPTANCE.md).

Exactly two nondegenerate coherent on-edge contributions from distinct/nonduplicate faces with
opposite strict third-vertex Z signs can be one `OnPlaneCrossingCandidate`. Preserve both raw
supports and multiplicity two; graph/path eligibility accepts that explicitly named ownership.
All existing conflict, tangent, duplicate, nonmanifold and mixed-coplanar uncertainty stays visible.
This does not repair input, infer solid validity, introduce a tolerance or change section geometry.
The decision supersedes TD-019's blanket unresolved policy only for this proved local case.
The same acceptance record defines the bounded full corpus and preliminary large-input budgets.
No dependency, unsafe code, framework or later-milestone operation is added.

## TD-033 — Karasu product and package identity

**Status:** Owner-requested naming refactor after M3 acceptance (2026-10-17).

Product identity becomes `karasu-slicer`. Rename the application packages/directories to
`karasu-cli` and `karasu-core`, Rust imports to `karasu_core`, and the existing isolated fuzz
package/helper to `karasu-mesh-fuzz`/`karasu_mesh_fuzz`. Future crates must use `karasu-<module>`.
Update manifests/lockfiles, executable identity, tests, doctests, active documentation and profiling/
fuzz tooling together. No dependency version, geometry API/algorithm or format behavior changes.

Keep the former `resin-cli` executable as a thin compatibility entry point using the same command
implementation. The primary default-run target is `karasu-cli`; no retained legacy package or
parallel CLI implementation is added. Explicit Cargo dependency aliases let source consumers retain
old import spelling while selecting the newly named package. Cargo package selectors intentionally
use the owner-requested new names. CLI branding changes are intentional; argument/output structure,
exit statuses and inspection semantics remain unchanged.

Do not rewrite historical evidence paths, pinned source identities or hash-bearing corpus metadata.
The existing `reference-fixtures` package and its version-1 STL headers/dataset identifiers stay
unchanged; changing them would invalidate the independently owned corpus and historical hashes.
This existing-tool exception does not relax the prefix rule for future crates. The workspace folder
and Git history/remotes are unchanged. Native Windows acceptance must be rerun for renamed binaries;
earlier M3 source validation does not automatically validate this refactor.

## Dependency adoption policy

Every proposed direct dependency requires a short review committed with the change or recorded in
this document. Geometry and file-format dependencies receive heightened scrutiny. Review:

1. **Implementation language:** confirm relevant algorithms are native Rust; locate any FFI,
   generated bindings, native system library, or build script.
2. **Source availability and provenance:** identify canonical source, release process, package owner,
   checksum/lockfile behavior, and ability to audit the exact version.
3. **Transitive dependencies:** inspect the complete feature-resolved tree, including target-specific
   and build dependencies. Disabled default features must be recorded.
4. **Maintenance:** assess recent releases, issue response, maintainer concentration, security policy,
   and signs of abandonment. Popularity alone is not evidence of suitability.
5. **Correctness and tests:** inspect test depth, fuzzing, property tests, known edge cases, panic
   behavior, and evidence for geometric robustness.
6. **Numerical model:** understand precision, predicates, tolerance assumptions, degeneracy behavior,
   coordinate/range limits, determinism, and repair policy.
7. **Performance and memory:** benchmark representative project workloads rather than accepting
   generic claims; inspect allocation and streaming behavior.
8. **Licensing:** identify direct and transitive licenses, notices, redistribution/source obligations,
   patent clauses, and compatibility with both possible open and closed distribution. Escalate
   ambiguity to the project owner; do not choose the project license by implication.
9. **Cross-platform support:** validate native Windows and Linux builds on supported target triples,
   including target-specific dependencies and path/file behavior.
10. **Security and supply chain:** check advisories, unsafe code, parser limits, dependency pinning,
    and exposure to untrusted model/print files.
11. **Alternatives and exit cost:** compare a standard-library or local implementation, document why
    the dependency is justified, and assess replacement/file-format lock-in.

Approval is for a specific version and feature set. Re-evaluate material upgrades. `Cargo.lock` is
committed for reproducible application builds. No third-party dependency is added in M0.

## TD-034 — Independent bounded LCD rasterization and grayscale PNG

**Decision:** Implement M4 inside the safe, dependency-free `karasu-core::raster` module. Separate
validated physical LCD metadata, M3 material eligibility, lossy projection, deterministic midpoint
sampling, quantization, optional discrete rectangular grayscale compensation, preview metadata and
bounded export. Retain exact M3 source/constructed geometry unchanged. Require explicit orientation,
outside policy and sampling; do not select commercial defaults from static controls or names.

**Rationale:** other slicers offer separate physical, resolution and raster controls. An active
edge sweep and one-row integer counts avoid an enlarged AA framebuffer. Separable monotone extrema
provide linear-time rectangular dilation/erosion. This improves asymptotic work/storage design;
measured performance claims remain limited to the recorded workloads.

**Output/dependencies:** Local PNG stored-DEFLATE framing follows public PNG/zlib specifications,
uses bounded fallible scratch and independently validated checksums/decoding. It intentionally trades
compression ratio for determinism and auditability. No dependency adoption/license choice is needed.
No timestamp, gamma, assets, runtime libraries or foreign geometry engines enter the core.

**Consequences:** AA may miss subcell features and compensation is discrete rectangular morphology,
not a Euclidean contour offset or calibrated optical correction. The documented finite arithmetic
domain can reject otherwise valid M3 geometry rather than lose precision silently. Streaming owns no
cache; callers control retention. Source failures and cancellation fuse output. Arbitrary writers may
retain partial PNG bytes on failure; transactional files are an application responsibility. Full
commercial behavior parity remains unverified. Specifications, research and validation are linked
from `docs/M4-LCD-IMPLEMENTATION.md` and `docs/M4-VALIDATION.md`.

## Open decisions

| Decision | Earliest milestone | Evidence needed |
| --- | --- | --- |
| Minimum supported Rust version | M1/M2 | Available Windows/Linux toolchains and dependency needs |
| Project distribution license | Decided 2026-11-11 (TD-145): AGPL-3.0-or-later | — |
| Transform inversion, normal conversion, and scene ownership beyond TD-012 | Later M2/application work | Concrete caller needs, singular/ill-conditioned policy, normal tests, and ownership evidence |
| Larger mesh-storage changes beyond TD-009/TD-013 | Later M2 | Measured source/report peak memory and concrete caller needs |
| Robust predicates beyond M3.1 scalar Z classification | M3.2 onward | TD-015–017 settle the local contract; exact rank, 2D orientation/nesting and any dependency need their own corpus and review |
| Integer polygon representation/scale | M3 | TD-025 ExactOnly mapping and TD-030 exact rational construction implemented; positive-error conversion/physical domain defaults remain OPEN; native acceptance separate |
| Region fill and source-material interpretation | M3 | TD-026 strict `EvenOddStrict`; TD-030 explicit source-group parity/union with caller face ownership; no inferred source identity or universal material default |
| Uniform layer schedule | M3 | TD-027 accepts explicit exact-dyadic scheduling; EXP-03 is optional compatibility research under TD-036; compatibility claims still require appropriate evidence |
| Constructive polygon overlay | M3 | TD-030 implements required explicit exact composition; native/corpus acceptance remains; generalized Boolean command families require separately scoped need |
| M3 preliminary performance/memory budgets | M3 closure | Measured region/layer baselines on agreed large fixtures before choosing acceleration or a hard acceptance threshold |
| Parallel execution framework | M3/M4 | Profiling, determinism design, cancellation needs |
| Image/PNG implementation | M4 | TD-034 selects grayscale8 and local bounded stored-DEFLATE PNG; compression improvements require measured need and separate review |
| First machine-specific format | M5 | User priority, public documentation, lawful interoperability evidence |
| Preparation algorithms and supported validity domain | M5.5-A | Independent offset/shell, voxel/SDF and subtraction/Boolean-equivalent evaluation; analytic/error/resource budgets and native-Rust dependency review |
| Prepared-scene/material combination, frames and anchors | M5.5-A | Source/derived identity, model/support/platform contact, scale/thickness behavior, connectivity and M3/M4/M5 integration tests |
| Preparation replay/versioning and conditional project persistence integration | M5.5-A/M6 | Deterministic operation records, source hashes, revision invalidation and supported save/reopen behavior if a project mechanism exists; no format selected |
| Preparation physical claim envelope | M5.5-E/M7 | Explicit drainage/strength limitations and separately authorized representative print trials before making physical success claims |
| Desktop integration and 3D viewport | M6 / TD-042 | Rust/egui/eframe/wgpu selected by owner; viewport design, dependency audit, UX, accessibility and native GPU validation remain |
| Packaging and update model | M6/M7 | Distribution strategy, signing, platform requirements |

No open choice should be preempted by adding an unused dependency.


## TD-035 — Versioned headless jobs, extensible machine export and user profile persistence

**Decision:** Implement immutable schema-one print jobs and validated printer/resin snapshots in
safe native Rust. Register exact versioned exporter IDs through a public session interface. Initially
support CTB v2 plain, GOO v3, firmware-motion Photon v1 and single-material Anycubic Photon P1 PP1 v3.
Pixel loss requires an explicit policy; unsupported settings fail rather than disappearing.

Use bounded local codec/container implementations, one-layer streaming and create-only transactional
machine publication. Distinguish source coverage from encoded quantized volume. Do not infer printer
firmware acceptance, physical orientation or calibrated resin settings from reader acceptance.

Adopt pinned serde 1.0.228 and serde_json 1.0.150 with float_roundtrip after the recorded native-Rust,
transitive, build-script, license and security review. Store editable profiles in platform user data,
configuration separately and cache locally; serialize strict bounded schema-one JSON. Use exclusive
library locking, optimistic successor revisions, immutable history and atomic complete-file updates.

Public protocol facts and reference behaviour guide independent algorithms. Other programs' code,
assets and presets are excluded. See the [implementation specification](M5-IMPLEMENTATION-SPEC.md)
and the [validation](M5-VALIDATION.md).

## TD-036 — Optional EXP-03/04 compatibility research

**Status:** Accepted by the project owner, 2026-10-17.

**Decision:** Reclassify EXP-03 (reference first-plane/layer-count rules) and EXP-04 (reference plane
degeneracy/invalid-mesh behavior) as optional compatibility research. They are no longer prerequisites
for implementation or acceptance of M1, M3 or subsequent milestones, nor for independently specified
Karasu defaults. This supersedes earlier mandatory EXP-03/04 gates throughout the roadmap and
historical research/decision records; it does not rewrite their observations.

**Rationale:** M3–M5 now have independently defined and tested contracts for explicit slice schedules,
sampling versus printer Z, boundary/degeneracy handling, invalid inputs and export metadata. Matching
commercial behavior is a separate product choice, not the correctness oracle for those contracts.

**Consequences:** Keep experiment IDs, fixtures, protocols, evidence and unresolved commercial
questions. Record status as optional and unperformed, not completed or validated. No Lychee/CHITUBOX
parity is inferred; a future compatibility claim requires scoped behavioral evidence, whether through
these protocols or an appropriately documented alternative. Do not silently select commercial
first-plane, tolerance, repair or inclusion defaults.

**Migration and validation:** Documentation/status changes only; no runtime/API/schema change or
weakened tests. Analytic/property/regression coverage for first/last layers, exact contacts,
coplanarity, invalid meshes, diagnostics, determinism and resource bounds remains mandatory.
Other M1 acceptance gaps, remaining M3 acceptance, M5 native Release host-policy blockers and all
mandatory M5.5/M6/M7 scope are unchanged. Reclassification alone does not mark any milestone complete.

## TD-037 — Immutable preparation plans before geometry evaluation

**Status:** Accepted implementation scope for the owner's next-roadmap-step request, 2026-10-17.

Implement M5.5-A's plan/history contracts in the existing safe-Rust headless core without dependencies.
Bind source digest assertions, canonical affine placement and explicit world-mm parameters in a
version-one canonical binary record. Support keys compare exact full upstream model bytes; edits do
not silently reproject/rebind contacts. Record raw finite cut directions and derive unit directions
with scaled arithmetic, preserving replay identity without repeated-normalization drift.

Validate explicit hollow -> cuts -> supports order, IDs, dimensions and independent admission limits.
Retain immutable snapshots with transactional bounded undo/redo; owner-local monotonic generations
prevent ABA background publication. Validation certifies parameters only, not embedded solids,
geometric connectivity, load-bearing strength or physical printability. No fake evaluate API exists.

Choose signed-distance/implicit CSG as the mathematical direction and bounded cells as the initial
reconstruction design for B–D, subject to explicit exact/interval source-sign, global validity,
reconstruction/topology and error-budget obligations. Do not adopt nearest-normal signing, implicit
clamps or foreign/C++ Boolean kernels from the references. No geometry implementation is supplied in
A and no backend success is inferred from this decision. See [specification](M55-IMPLEMENTATION-SPEC.md),
research and [validation](M55-VALIDATION.md).

The owner authorized this next development increment while M5 Windows Release remains environment-
blocked. That acceptance gate, B–E geometry work, M6 interaction and M7 hardware/end-to-end validation
remain required; advancing design/contracts does not close them.

**Subsequent validation:** [M5.5-A validation](M55-VALIDATION.md) passes full Linux736/0/2 and native
Windows735/0/2 Debug/Release on the final snapshot. This also resolves the M5 software Release gate
for unchanged exporters. Initial host blocks remain evidence; no permanent policy remediation or
B–E geometric acceptance is inferred.

## TD-038 — Certified convex supporting-halfspace hollowing

**Decision:** Implement the first M5.5-B geometry backend as `convex-halfspaces-v1`, superseding
TD-037/A's provisional voxel reconstruction choice for this bounded domain. Closed embedded outward
convex polyhedral sources admit Euclidean erosion as an intersection of inward supporting planes.
This avoids adopting an unproved general-solid sign oracle or hidden voxel/chamfer approximation.
Nonconvex, multiple or unresolved geometry is explicitly unsupported; no arbitrary-STL promise is made.

Preserve the placed exterior and generate an independently certified cavity, reversing it in the
sliceable shell. Use authored native-Rust fixed-integer orient3d with interval filters, source contact
proofs and exact derived hull topology. Approximate clipping/snapping is only a candidate construction;
interval checks must prove nonempty convex erosion inclusion at t±delta within the caller's budget.
Nonnegative plane-combination certificates include the entire residual; floating solve proposals
are never treated as exact normal-cone membership. Keep represented volume bounds, source tessellation,
coordinate placement, erosion parameter and physical-print claims distinct.

No new dependency or reference implementation is imported. All core geometry remains safe Rust.
The algorithm result pins backend identity and exact plan content. Synchronous phase progress is
separate from cancellation; failed/uncertain/cancelled work returns no successful partial geometry.
This implements B only for its documented domain; C drainage, D supports and E combined pipeline
acceptance remain required. See the [specification](M55B-HOLLOWING-SPEC.md) and the
[validation](M55B-VALIDATION.md).

## TD-039 — Certified finite drainage scenes with exact layer subtraction

**Decision:** Implement the first M5.5-C backend as a prepared CSG slicing scene rather than a
fabricated general Boolean triangle mesh. The positive convex shell is source zero; independently
certified polygonal cutters are subtracted as a union using exact represented-coordinate planar
arrangements. Preserve existing material-union behavior and diagnostics. No new dependency or
commercial/reference geometry implementation is adopted.

Initial supported cuts are finite and parallel to world axes. A whole entry cap beyond one exterior
support plane and a whole terminal cap strictly inside every cavity support plane establish a
positive-area geometric passage. Explicit clearance and bounded polygon/cylinder error are required.
Potentially overlapping cutter boxes, oblique axes, ambiguous boundary conditions and over-budget
construction fail transactionally. This conservative domain is intentional; no physical drainage,
arbitrary STL, generic cut-mesh export or support generation is claimed.

Immutable plan records retain operation IDs and ordering after edits/removal; old results remain
bound to their original record. A zero-cut scene reports its sole enclosed cavity. M4 consumes the
constructed material difference through existing layer interfaces. Full combined preparation/export
acceptance remains M5.5-E, interactive editing M6 and end-to-end acceptance M7. See
[specification](M55C-DRAINAGE-SPEC.md) and reference research.

The necessary M3 integration admits one nondegenerate coplanar cap edge plus one coherent
noncoplanar side occurrence as a local patch perimeter, preserving both records. Opposite source
edge uses, distinct faces, strict side rank, agreed coordinates and no independent issue are
required. Other mixed cases remain unresolved. This local ownership change is neither source
repair nor a global solid-validity certificate; regression and complete platform suites cover it.

## TD-040 — Parametric support stations and exact prepared-material composition

**Decision:** Preserve historical `SupportSpec`/`KPREP1` and introduce a separately versioned `KSUP1`
immutable support plan. Each manual support is an ordered sequence of world-positioned, fully
oriented convex polygonal profiles. Elliptical polygons, rectangles, regular/custom convex polygons
and terminal points supply independent shape, dimensions, tilt, roll, taper and path controls.
Adjacent-ring convex hulls define explicit closed components; they do not imply an analytic curved
sweep, automatic tree generation or a final Boolean boundary mesh.

Use the existing exact predicate/hull/interval machinery to validate components and positive-area
shared-section connectivity. Certify one intended source-facet tip contact with bounded penetration,
platform attachment, and separation of all unintended model/cavity/drainage interactions. Uncertain
separation fails explicitly. Cross-support union is an explicit policy; no geometric acceptance
certifies load capacity, adhesion, physical drainage or printing calibration. Support sources are
certified convex solids or the existing bounded hollow/drain domain; arbitrary concave input remains
unsupported. Rounded world-profile rank is independently checked to exclude point/edge connections.

Extend explicit exact planar construction with `(body minus cutters) union additive components`;
body parity is ID0, cutter parity uses positive IDs below bit63, additive component parity uses
high-bit IDs. This avoids accidentally cutting supports or double exposing intentional overlaps.
Existing union/difference APIs retain their behavior. M3 represented section coordinates feed exact
rational construction, M4 masks and extensible M5 export without a foreign geometry dependency.

Full recipe identity records source/model revision, ordered parameters, quaternion/matrix placement,
shapes, intended contacts and semantic tolerances. Changed upstream content rejects stale anchors;
world-fixed support edits/rebinding remain explicit. Bounded support history provides transactional
edit/remove/undo/redo and cancellation. Conservative work/storage admissions include subordinate
processing and copied geometry; capacities and RSS remain distinct. See
[specification](M55D-SUPPORTS-SPEC.md), research and
[validation](M55D-VALIDATION.md). Comprehensive combined-scene acceptance remains M5.5-E and M6/M7.

## TD-041 — Revision-owned preparation tasks and bounded headless acceptance

**Status:** Accepted, 2026-10-18.

**Decision:** Complete M5.5-E with a framework-independent session around existing immutable
A–D plans/scenes. Full canonical bytes, generation, monotonic request and opaque owner identity
jointly govern current publication. Preview of a retained completed result includes its own plan
and cannot directly supply a layer engine. Cancellation/failure preserves that preview without
marking it current. Named progress stages/counts describe computation, not accepted revision status.
No worker runtime, project format, geometry dependency or new foreign kernel is introduced.

**Reason:** other programs show whether a prepared model can be exported, which does not prove
rollback or stale-result safety. Independent Karasu requirements need stronger exact revision
ownership, ABA/out-of-order rejection and transactional publication. Existing low-level immutable APIs remain compatible.

**Validation:** Full Linux Debug/Release 852/0/2 and native Windows MSVC 851/0/2, independent combined
STL/geometry/1.6-million-pixel/four-format readers, eight-support stress, 12 Python reader tests,
four ASan campaigns and 195 retained replays pass. Declared bounded fixture timing/RSS budgets pass.
See [specification](M55E-INTEGRATION-SPEC.md), research and
[closure acceptance](M55-CLOSURE-ACCEPTANCE.md) for identities, evidence and limitations.

**Consequences:** M5.5 closes within its explicit convex/cardinal/polygonal domain. Source digests
remain loader assertions; concurrency memory is caller-owned; cancellation is cooperative. M6 must
integrate real editing/preview/scheduling/current-only export; M7 retains release and physical
acceptance. No arbitrary-mesh, physical drainage/strength or expanded-MVP completion claim follows.

## TD-042 — Rust desktop interface with egui, eframe and wgpu

**Status:** Accepted stack selection by the project owner, 2026-10-18. Implementation pending M6.

**Decision:** Implement the desktop application in Rust, using `egui` for interface widgets/layout,
`eframe` for the application/window lifecycle and `wgpu` for GPU rendering. Use eframe's wgpu
backend explicitly. This supersedes the deferred framework selection in TD-003 without changing
its headless-core boundary. Target native Windows and Linux under the existing M6 acceptance gate.
A future application crate must use the `karasu-` prefix; no empty crate or dependency is added now.
The owner-approved base color palette and semantic roles are recorded in [UI design](UI-DESIGN.md);
M6 must implement and validate the theme alongside the selected stack.

`karasu-core` continues to own authoritative f64 geometry, preparation, slicing and raster/export
semantics. Presentation-specific GPU buffers, cameras, picking feedback and widget state belong
outside the core. Display approximations or GPU previews must never become authoritative export
geometry/masks. Background work must preserve TD-041 revision ownership and current-only export.

**Recommended viewport design, pending M6 prototype:** A focused custom 3D renderer on the same
wgpu device/queue as eframe, integrated through `egui_wgpu` callbacks. A render-to-texture path can
be selected where independent depth, MSAA or multipass composition makes it useful. Prototype the
integration before fixing the pass structure. This recommendation is not adoption of another engine.

Prioritize indexed mesh buffers and reuse across camera/UI changes, explicit model transforms,
depth-tested solid rendering, selection/diagnostic overlays, build-platform/grid display, clipping
planes and cavity inspection, support/contact editing and layer previews. Start with simple useful
lighting; require measurements before adding complex shading or acceleration. Picking must map
back to original entity/triangle identities and validate against the current authoritative revision;
clipped/hidden geometry must obey an explicit selection policy. CPU ray tests and optional GPU ID
picking should be compared on representative meshes before choosing an implementation.

Preserve f64 source coordinates; any f32 GPU conversion needs an explicit local origin, finite/range
checks and a measured display error budget. Retained preview uses its original plan/transforms.
Presentation buffers must not mutate or reinterpret source meshes. Upload work, memory retention
and background jobs require explicit budgets, revision-aware replacement and native stress evidence.

**Alternatives and tradeoffs:** A general 3D engine can supply scene management, materials and
picking, but adds integration, dependency and lifecycle decisions to this eframe-based application.
Evaluate one only if the focused viewport prototype exposes a concrete unmet requirement or measured
maintenance/performance benefit. A custom renderer requires ownership of cameras, picking, clipping,
transparency, resource recovery and tests; it is not automatically faster or smaller. No alternative
engine is selected by this decision. Transparency/cavity presentation must not imply exact material
or physical printability guarantees.

**Adoption gates:** Before adding Cargo dependencies, audit exact mutually compatible versions and
features, stable-Rust/MSRV fit, source/licenses, maintenance/security, transitive dependencies,
FFI/build scripts and platform support under AGENTS.md. Selecting Rust libraries does not certify
their entire transitive/platform implementation as Rust-only or unsafe-free. Preserve the workspace
unsafe prohibition for application code and the ban on foreign geometry kernels. Inspect eframe's
wgpu re-exports and compatible integration versions to avoid duplicate incompatible GPU stacks.
No version or GPU backend default is frozen by this documentation-only selection.

M6 must validate large meshes, support overlays, clipping/picking, DPI/resizing, accessibility,
idle redraw behavior, cancellation, stale-result rejection and resource/device/surface failures.
Record actual Windows/Linux adapters, drivers and selected backends; surface unavailable-GPU errors
clearly. Backend fallback policy and support minimums need evidence, not assumed universal coverage.
This selection does not mark M6 implemented or replace native interactive/packaging tests.

**Primary references checked on 2026-10-18:**

- [eframe documentation](https://docs.rs/eframe/latest/eframe/) describes the egui application
  framework, wgpu backend and graphics re-exports.
- [egui-wgpu CallbackTrait](https://docs.rs/egui-wgpu/latest/egui_wgpu/trait.CallbackTrait.html)
  documents custom GPU rendering integration.
- [wgpu project](https://wgpu.rs/) documents its portable graphics role.
- [Bevy mesh picking example](https://bevy.org/examples/picking/custom-hit-data/) illustrates
  functionality available in a broader engine; it does not establish suitability or superiority here.

## TD-043 — Headless `slice` CLI command over existing core contracts

**Status:** Accepted at the owner's request, 2026-10-18.

**Decision:** Add `karasu-cli slice` so a user can turn one STL into a validated machine file or
reference PNG directory without writing Rust. The command composes only accepted M2–M5 public APIs
and selects these explicit adapter policies:

- Units are asserted with `--assume-mm`; profiles are typed `ProfileDocument` JSON files.
- Placement is an explicit translation only. The placed minimum Z must equal 0 exactly and the XY
  extent must lie in the LCD area; failures report the exact correction instead of grounding,
  centering or clipping automatically. Translating by the negated `inspect` minimum yields +0.
- Meshes that are not closed combinatorial surfaces are refused unless
  `--allow-topology-defects` is given. Waiving the gate never relaxes per-layer strictness:
  unresolved, open or contacting sections still fail through `RasterError::UnresolvedGeometry`.
- Layers use the resin calibration height, midpoint samples `(i + 1/2)·h` through the top Z,
  represented binary64 coordinates and `EvenOddStrict` regions. Masks are binary with no
  antialiasing or compensation. `PixelPolicy::Exact` rejects any format loss.
- Section face and planar-pair caps reuse the M3 closure's validated large-input budgets
  (2,000,000 and 40,000,000); other stage, raster and export limits keep core defaults.
- `--format` must name a built-in dialect or `reference`; it is validated at parse time (exit 2).

**Rationale:** The engine was reachable only through examples and tests. A minimal adapter makes the
pipeline usable and gives M6 a headless reference path while preserving the AGENTS.md rule against
silent reinterpretation. The closed-surface gate catches open meshes whose defects would otherwise
be invisible when no sampled plane crosses them.

**Evidence:** Eleven process-level tests and two parser unit tests cover all four machine formats,
reference output, exact box volume, translation, LCD/Z envelope refusal, create-only publication,
the topology gate and its waiver, profile mismatches, usage errors and the `resin-cli` alias.
Manual Linux Release runs over the owned FX-01–FX-11 corpus produced analytic volumes (FX-01
8000 mm³; FX-11 113,093.3 mm³ versus 113,097.3 mm³ for the ideal sphere) and refused FX-10A/B
by default.

**Limitations:** No hollowing/drainage/support options, rotation/scale or antialiasing. Progress
and Ctrl-C cancellation were added subsequently by TD-044. Raster/export is single-threaded; FX-11 at 1000 × 600 px took
about 64 s for 1200 layers in Release, so full-resolution panels need measured optimization before
M7 performance acceptance. Native Windows validation passed on 2026-10-19 ([native Windows validation](CLI-ENGINE-NATIVE-WINDOWS-VALIDATION.md)).

## TD-044 — `ctrlc` for cooperative Ctrl-C cancellation in `karasu-cli`

**Status:** Accepted at the owner's request for CLI progress and Ctrl-C support, 2026-10-18.

**Decision:** Add `ctrlc = "=3.5.2"` to `karasu-cli` only. Its handler thread sets a process-wide
`AtomicBool`; every `slice` stage receives a `Cancellation` predicate reading it, so the existing
core checkpoints stop work and transactional export removes its staging data. A second Ctrl-C exits
immediately (status 130). `karasu-core` remains free of signal, thread and terminal concerns.
Progress uses only `std` (`IsTerminal`, `Instant`) and needs no dependency.

**Why a dependency:** Receiving SIGINT or a Windows console control event requires OS FFI
(`sigaction`/self-pipe on Unix, `SetConsoleCtrlHandler` on Windows). The workspace forbids
`unsafe`, so a safe wrapper is required. Without a handler the default action terminates the
process before transactional cleanup can run.

**Review record (2026-10-18):**

- *Native Rust / FFI:* Pure Rust crates that call operating-system APIs. They are not geometry
  code and do not bundle or compile C/C++; no geometry kernel is involved.
- *Transitive tree (normal edges):* Linux/Unix: `ctrlc 3.5.2 → nix 0.31.3 → {bitflags 2.13.2,
  cfg-if 1.0.5, libc 0.2.190}`. Windows: `ctrlc → windows-sys 0.61.2 → windows-link 0.2.1`.
  Apple targets add `dispatch2`/`objc2` entries to `Cargo.lock` only; they are not supported
  targets and are not built on Linux or Windows.
- *Build scripts:* `nix` (via build-dependency `cfg_aliases 0.2.2`) and `libc` emit `cfg` flags;
  `libc` runs `rustc --version` only. No native compilation, downloads or bundled binaries.
  `ctrlc` and `windows-sys` have no build script.
- *Versions and age:* `ctrlc` 3.5.2 was published 2026-02-10. `libc` is locked to 0.2.190
  (2026-10-02) rather than the two-day-old 0.2.191. The other locked crates were published between
  2025-10-06 and 2026-09-16. None is yanked. Exact `ctrlc` pin; versions change only by review.
- *Licenses:* `ctrlc`, `libc`, `bitflags`, `cfg-if`, `windows-sys`, `windows-link`: MIT or
  Apache-2.0; `nix`, `cfg_aliases`: MIT. All are permissive and compatible with any later
  project license choice. No project license is selected by this decision.
- *Maintenance / security:* `ctrlc` is the widely used cross-platform handler (Detegr/rust-ctrlc),
  with active 2025–2026 releases. `nix`, `libc` and `windows-sys` are core ecosystem crates (the
  latter maintained by Microsoft). Its surface is one handler registration; the handler thread only
  stores an atomic and writes one stderr line.
- *MSRV / platforms:* Highest declared `rust-version` is 1.71, below the stable toolchain used.
  Linux is validated here. Windows support is documented upstream through `SetConsoleCtrlHandler`.
  The command builds and runs natively
  ([native Windows validation](CLI-ENGINE-NATIVE-WINDOWS-VALIDATION.md)), but Windows console
  Ctrl-C has no automated test yet (the SIGINT test is Unix-only).
- *Performance and memory:* One registration and one handler thread blocked on a pipe or event;
  each predicate call is one atomic load at existing checkpoints.

**Tests:** A Unix integration test sends SIGINT during a 4,500-layer slice and checks exit status
130, the final message, empty stdout, no output and no staging entry; it passed 20/20 repeated
runs. Manual Linux Release cancellation of FX-11 returned within 12 ms. Progress tests cover
phase order, completed-layer counts, `--no-progress` silence and unchanged stdout; unit tests cover
flag exclusivity and remaining-time arithmetic.

**Alternatives rejected:** `signal-hook` is Unix-only and would still need a Windows path.
Hand-written FFI violates the `unsafe` prohibition. Leaving SIGINT unhandled cannot remove staging
data.

## TD-045 — Sampled-distance hollowing backend for non-convex solids

**Status:** Accepted at the owner's request, 2026-10-18. Specification:
[M5.5-F](M55F-SAMPLED-HOLLOWING-SPEC.md). Research: M5.5-F reference.

**Decision:** Add `preparation::sampled` (backend `sampled-distance-tetra-v1`) beside the convex
backend. It has six parts:
1. **Distance:** exact binary64 point-to-triangle distance through a deterministic BVH.
2. **Sign:** exact even-odd ray parity with symbolic perturbation.
3. **Evaluation:** Lipschitz-pruned adaptive blocks, so only leaves straddling `d = t` are sampled.
4. **Extraction:** marching tetrahedra on the Kuhn subdivision, with edge-keyed welding.
5. **Validation:** an independent closed-oriented-manifold check.
6. **Certificate:** an a priori `cell · sqrt(3) + 2 · error` band and an a posteriori surface
   enclosure.

The plan's reserved `SamplingSpec` fields now have their intended grid meaning. The CLI exposes
the backend as `slice --hollow <wall> [--hollow-cell <mm>]`, with default cell `min(wall/8, 0.5 mm)`.

*Superseded in part by [TD-093](#td-093--hollowing-with-a-predicted-cell-and-an-exact-wall-certificate) (M6.4e):* the default cell
follows TD-093's rule from the wall and the surface area, the certificate's lower end is proven
per cavity face, and every hollow plan of the command line and the interface guarantees the
requested wall. The backend's field, extraction, welding and validation are unchanged.

Supporting additions:
- `karasu_core::digest::sha256`, independently implemented from FIPS 180-4 and tested with its
  vectors, binds the plan to the source bytes.
- Work runs on `std` scoped threads, with results identical for every thread count.

**Rationale:**
- M5.5-B certified only convex sources, while real models are non-convex.
- Other slicers' hollowed results show walls quantized or coarsened without a report.
- Exact distance and parity match the slicer's own semantics.
- The tetrahedral extraction guarantees a closed embedded cavity, so strict even-odd slicing of
  exterior plus reversed cavity is valid.
- Work proportional to the cavity area makes fine cells practical.

**Alternatives rejected:**
- Dense voxel or chamfer fields: approximate metric, cubic memory.
- Flood-fill sign: leaks through holes.
- Marching cubes with position welding: ambiguous cases and cracks.
- Per-layer 2D offsets: not a 3D wall.
- OpenVDB or libigl via FFI: foreign kernels are prohibited.
- Smoothing the cavity: it would move the wall without re-certification.

**Limitations:**
- Drainage, supports and pocket policies on sampled cavities are future scope.
- The output mesh is large; certified simplification is future work.
- The a posteriori bound is conservative.
- Native Windows validation passed on 2026-10-19 ([native Windows validation](CLI-ENGINE-NATIVE-WINDOWS-VALIDATION.md)).

## TD-046 — Slicing throughput: face Z index, box-pruned contacts and filtered orientation

**Status:** Accepted at the owner's request, 2026-10-18.

**Decision.** Three changes, each proven result-identical, remove the per-layer costs that
dominated large-model slicing.

1. **Face Z index.** `LayerEngine::with_face_z_index(max_bytes)` is opt-in.
   - It computes each face's world Z range once, with exactly the transform the full scan uses,
     then evaluates only faces whose closed range contains the plane, in ascending face order.
   - `SectionSummary` gains `prefiltered_faces`; area observations count evaluated faces only.
   - `SectionResource::IndexBytes` bounds the index.
   - With the index enabled, `max_faces` bounds the candidates evaluated per plane.
   - Transform failures surface when the index is built.
2. **Box-pruned planar contacts.**
   - Exact contact tests in `inspect_planar_paths` are restricted to segment pairs whose closed
     XY boxes overlap. Pairs whose boxes are disjoint cannot touch.
   - Candidates come from a sweep sorted by minimum X and are processed in lexicographic pair
     order, so diagnostics are identical.
   - `examined_pairs` still counts every pair.
   - The new `tested_pairs` counts exact tests, and `max_pairs` now bounds `tested_pairs`
     (an intentional semantic change: the old all-pairs cap made work and limits quadratic).
3. **Filtered exact orientation.**
   - The 2D orientation predicate returns zero immediately when a point coincides with an
     endpoint.
   - Otherwise it first evaluates an outward-rounded interval determinant, falling back to the
     existing exact integer evaluation only when the interval contains zero or NaN.

**Evidence:**
- `tests/face_z_index.rs` compares indexed and full layers over the corpus, including planes at
  vertex and face heights and a transformed 327,680-face mesh.
- The orientation filter agrees with exact evaluation on 60,000 near-degenerate, huge, tiny and
  subnormal cases.
- The CLI's GOO output for FX-11 is byte-identical before and after.
- The FX-11 slice (1,200 layers) improved as follows:

  | Panel | Before | After |
  | --- | --- | --- |
  | Tiny panel (geometry only) | 49.7 s | 2.1 s |
  | 1000 × 600 panel | 55.2 s | 7.7 s |

- Per-layer validity at the equator went from 25.8 ms to 0.17 ms.

**Remaining cost:** rasterization and encoding now dominate. Full-resolution panels still need
measured raster/encoder optimization before M7.

## TD-047 — Drainage of sampled cavities and per-sample raster difference

**Status:** Accepted at the owner's request, 2026-10-18. Revised the same day after five
adversarial multi-agent review rounds (17, 8, 5, 5 and 5 confirmed findings, the last ones
reducing to three distinct issues; the unverified ones were addressed as well).
Specification: [M5.5-G](M55G-SAMPLED-DRAINAGE-SPEC.md).

**Decision:** Add `preparation::sampled::drain_model_sampled` for hollow-then-holes plans on the
sampled backend. The M5.5-C cylinders and separation policy are reused. Openings are certified
without convexity:
- **Entry cap:** exactly disjoint from the source surface and certified clear of it. Its centre
  is joined to the outside of the source's bounding box by a straight segment, in one of 26
  directions, that meets no source triangle (exact), which places it in the unbounded exterior.
- **Fallback admission:** only when every such segment is blocked, the entry must be outside every
  edge-connected source component individually. This is sound only for an embedded source with
  pairwise-disjoint bodies, so an exact, cancellable BVH self-pair check runs first, at most once
  per call; a failure refuses that hole (`OpeningIssue::SourceNotEmbedded`). Vertex-only contacts
  are allowed.
- **End cap:** exactly disjoint from the cavity surface, certified clear of it, with odd cavity
  parity at its centre.

Each cut drains the innermost region containing its end; regions are reported with volumes,
islands and draining operations.
- `OpeningIssue::Overshoot` separates too-long cuts from too-short ones.
- `DrainageError::Cut` names the operation whose cylinder cannot be built.

Supporting additions:
- exact closed segment/triangle, triangle/triangle and improper-mesh-contact predicates, with
  exact plane-side and coplanar (corner overlap, separating edge) shortcuts;
- face distances from a Gram-Schmidt foot that is always a convex combination of the corners,
  taken together with the edge candidates, so a near-degenerate face never shortens a distance;
- interval-arithmetic separation lower bounds: caps are admitted per wedge from their centre
  against the required clearance (deciding "beyond" from the certified bound only, with rigorous
  box distances down to subnormal gaps), and the reported clearances come from one bounded
  best-first search per cap over its enclosing disk, certified even when cut short;
- sparse labeled parity (odd components only) with subtree masks, so island ownership rays skip
  island-only subtrees; exact segment queries with a conservative interval slab filter, and
  branch-and-bound clearance in the BVH; BVH construction reserves its exact node count and every
  other input-sized buffer fallibly, polls cancellation, and the drainage surface indexes are
  admitted against the hollowing payload cap;
- exact point queries for classifying end caps that stop on a surface, whatever its orientation;
- a fixed-width exact `orient3d` path (common-exponent scaling, six 64-bit limbs) and an exact
  128-bit square comparison for interval square roots, both checked against the general integers.

**Evaluation:**
- The shell is one strict layer stream.
- Cutter sections are computed analytically per plane (`raster::LayerCut`): the ring for vertical
  prisms, an axial rectangle times the ring's chord for horizontal ones.
- Sections are subtracted per sample: `raster::rasterize_layer_minus_rings_with_cancellation`,
  `raster::stream::DifferenceRasterStream`.
- Cut vertices may lie outside the panel.
- The render core keeps one scanline per edge set; the plain path reproduces the previous masks.
- The preflight counts span buffers, and both edge buffers at full reserved capacity before
  either is reserved.

**CLI:**
- `slice --drain-hole <x> <y> <z> <axis> <length> <diameter>`, repeatable up to 32 times, requires
  `--hollow`.
- Holes below two pixel pitches (vertical) or two pitches and two layers (horizontal) are refused,
  since they could not open in the sampled file.
- Plan error is `min(1 µm, cell/32)` with holes and 1e-6 mm without.
- Failures name holes by position. The summary reports certified openings and regions without a
  certified opening.

**Rationale:**
- **Why not the convex certificate:** M5.5-C relies on supporting half-spaces, which non-convex
  cavities lack.
- **Why an escape segment first:** it proves exterior connectivity directly, for any source,
  including touching or self-intersecting bodies. Most entries (holes from below, from the sides
  or into open recesses) have one.
- **Why the admission is only a fallback:** per-component parity proves connectivity for embedded,
  disjoint surfaces only, and the admission enforces exactly that precondition. Its pair count
  follows box overlaps and can be quadratic in the triangle count (fans, sliver strips): it took
  54.6 s on a 1000-sided fan-capped cylinder (hollowing: 0.1 s), 91 s on a cube with 100 × 100
  face grids, and 40–143 s on sheared sliver-strip prisms whose axis escapes were blocked. The
  26-direction escape avoids it in all of these (0.2–0.7 s).
- **Why a bounded best-first clearance report:** the certificate only needs "farther than the
  required clearance", which prunes almost everything. Searching for the exact minimum instead
  examined nearly a whole equidistant cavity per fan triangle (24 s, and 138 s with 0.1 mm cells,
  for an end cap at the centre of a sphere), without polling cancellation.
- **Why wedges from the centre:** fan triangles from a cap vertex are slivers whose boxes cover
  most of the cap, so any surface crossing the cap plane near the rim was examined once per
  sliver (30 s for a dense tube around a 4096-sided cap; 0.6 s with wedges).
- **Why faster exact arithmetic:** zero signs on coplanar input always reach the exact path. With
  the fixed-width path and the coplanar shortcuts, those two admissions take 0.7 s and 1.3 s.
- **Why not exact layer difference:** it is quadratic and limited to a per-layer dynamic range of
  about 2⁸. Measured: 4,988 edges and a 2¹⁹ coordinate ratio on one shell layer.
- **Why analytic cutter sections:** slicing cutter meshes made tangent planes unresolved and
  rejected out-of-panel cutters. Analytic sections remove both failure modes and equal the
  represented prism.

**Alternatives rejected:**
- Raising the exact construction limits.
- Boolean mesh subtraction (no robust in-house kernel; foreign kernels prohibited).
- Crediting regions a prism merely crosses (needs a separate proof; under-reporting is
  conservative).
- Slicing cutters as meshes.

**Limitations:** axis-parallel holes only; no pocket policies; supports on sampled drained models
are not supported yet; the fallback admission can be quadratic in the triangle count; the
drainage surface indexes roughly triple peak memory on very fine cavities. Native Windows output
is byte-identical to Linux ([native Windows validation](CLI-ENGINE-NATIVE-WINDOWS-VALIDATION.md)).

## TD-048 — Layer planes avoid vertex heights

**Status:** Accepted at the owner's request, 2026-10-18, after a code audit found the defect.

**Problem:** the CLI samples each slab at its midpoint. When that plane coincides exactly with a
horizontal face, the M3 section returns the face itself: a coplanar cap edge next to a side wall is
admitted as a patch perimeter (TD-039) whether the wall continues below or above the plane. For a
step, such as the top of a plate carrying a boss, that face is the plate minus the boss footprint,
which is neither the material below nor above. The layer was silently left without material under
the boss (verified: 30,000 lit pixels instead of 40,000 for a 20 mm plate under a 10 mm boss at
0.05 mm layers). This happens whenever a horizontal face lies exactly on a midpoint, for example
faces at 0.25 + 0.5 k mm with 0.1 mm layers, which binary STL coordinates represent exactly.

**Decision:** `LayerEngine::with_vertex_avoiding_planes`, enabled by the CLI, samples each layer on
a plane that contains no mesh vertex:
- A scheduled plane that equals the world Z of a face corner moves to the middle of the
  vertex-free interval just below it. The interval reaches down to the next lower vertex height or
  to the bottom of the slab (half a nominal height), whichever is higher.
- No vertex lies between the two planes, so the layer is exactly the section just below the
  scheduled plane. With no vertex on the plane, every face is missed or crossed strictly, so no
  coplanar or on-plane case remains.
- Staying mid-interval keeps crossing points well separated from vertices. A plane one unit in the
  last place below was rejected: its crossings land within rounding of the vertices and make the
  layer geometrically unresolved.
- Only if the interval holds no binary64 value strictly inside does the plane step down one value
  at a time. A moved plane that would not stay above the previous layer fails explicitly
  (`LayerFailure::PlaneSeparation`); it cannot happen with the slab bound.
- The layer keeps its scheduled `plane()` identity, which print jobs and exporters verify;
  `RegionLayer::sampled_z_mm` reports the sectioned Z, and analytic drainage cutter sections use
  it too.

**Rationale:**
- **Why "just below":** it keeps a plane through a top face non-empty and equal to the material
  under it, as before, and gives every step a consistent side. The material above starts on the
  next layer.
- **Why move the plane rather than change the M3 kernel:** M3's exact on-plane semantics and its
  contracts stay unchanged for direct callers. The engine option only chooses where to sample,
  inside the same slab.
- **Why the middle of the gap:** a plane at the exact midpoint is still preferred whenever it meets
  no vertex, so ordinary layers are unchanged.

**Evidence:**
- An engine test covers a plane on an upright and an upside-down step: one region with no hole,
  the full plate or the stem, scheduled identity kept, and only that plane moved.
- A CLI test of a step on a midpoint gives the volume 58 mm³; the unfixed engine gave 54 mm³.
- Unit tests cover the plane choice.

**Limitations:** the moved layer samples at most a quarter of a nominal layer below its midpoint.
Bodies that touch along a vertical face still make layers unresolved, as before.

## TD-049 — Code-audit fixes: P1 limits, swapped panels, placement hint and removable media

**Status:** Accepted at the owner's request, 2026-10-18. Audit:
[CODE-AUDIT-2026-10-18](CODE-AUDIT-2026-10-18.md).

**Decisions:**
- **P1 raster limits (A1).** `RasterLimits::default()` now admits 128 MiPx, matching
  `ExportLimits`, and 2 billion work units. The CLI previously failed on the first layer of the
  13312 × 5120 Anycubic Photon P1.
- **Swapped panels (A2).** Machine writers pair each panel-axis length with that axis's pixel
  count. With `swap_xy`, the world build lengths now swap into panel order before encoding, so the
  CTB/Photon bed sizes, GOO display sizes and PP1 pixel pitch/size describe square pixels.
- **Placement hint (A3).** A model not resting on Z = 0 gets the complete
  `--translate <x> <y> <z>`: the given X/Y and the negated source minimum Z, which yields an exact
  zero. It replaces the given translation, instead of an amount that ignored the given Z.
- **Removable media (A4).** Publication links atomically when possible. Where the link fails
  (FAT32/exFAT), it creates the destination exclusively, copies, syncs, and on failure removes
  only that new file. An existing destination is still never replaced; the fallback is not atomic.
- **Smaller fixes:**
  - **B1:** an error after publication is reported as itself, not as a Ctrl-C interruption.
  - **B2:** staging entries start with a dot.
  - **B3:** the default export metadata budget is 64 MiB, about 100,000 PP1 layers.
  - **B5:** generated support hulls are validated under the output plane cap, like generated
    cavities.

**Not changed:** a non-ASCII byte or bare CR in an ASCII STL is still rejected (B4); only the
message is misleading. A 256-vertex twisted support loft also needs a component payload above the
4 MiB default.

**Evidence:** for every fix except B1 and B2, a regression test fails before the fix and passes
after it:
- a P1-sized layer under default limits;
- CTB header lengths of a swapped panel;
- the suggested translation, applied successfully;
- copy fallback publication and its no-clobber/cleanup behaviour, with a hard link that is refused;
- a 20,000-layer PP1 admission;
- a twisted loft above the plane cap.

## TD-099 — Toast notifications

**Status:** Accepted on 2026-10-23, owner's design (M6.5); mockup in the layout canvas.

**Context:** the tool panels may no longer hold messages (TD-098's design rule), and the
interface's messages appeared only as one line in the status bar and in the Messages window.

**Decision:**
1. **Every message is also a toast** (`toasts`): a fixed-width card at the bottom centre of the
   viewport, a margin above its bottom, drawn over the scene. Its fill is its severity's
   (information azure, success green, warning orange, error red, all dark enough for the
   primary text); its title bar holds the severity's icon, its name and a close button; the
   message follows, and a link opens the Messages window when the message has details or
   fixes.
2. **Timed or not, per message.** A message's `Lasting` sets its toast's time: by default 5 s
   for information, 4 s for success, 8 s for a warning, and until closed for an error. A timed
   toast shows a thin bar running from full to empty, right to left, and closes when it ends;
   the pointer on it pauses its time.
3. **At most three** toasts show at once, the newest lowest.
4. **Every toast stays in the Messages window**, which keeps the session's messages with their
   details and fixes; the status bar keeps its line of short messages.
5. **A success level** joins information, warning and error: a finished slice, an export and a
   passed wall verification are successes.

**Consequences:** panels can hand their messages to toasts, tool by tool; nothing is drawn or
repainted without toasts, and a timed toast repaints about 30 times a second while it shows.

## TD-100 — OpenGL loaded only as the fallback

**Status:** Implemented on 2026-10-25 at the owner's request; it refines how TD-067's GPU baseline
(modern backends plus the OpenGL fallback) is applied.

**Context:** the automatic backend choice enumerated Vulkan, DX12, Metal and OpenGL together and
then preferred the modern adapters, so the OpenGL driver was loaded even when Vulkan drew. On the
owner's Windows machine (AMD Radeon RX 9070 XT, driver 32.0.32015.7029) the Vulkan smoke test
crashed at exit twice in four native validations, after rendering its frames: the Windows event
log names the AMD OpenGL driver `atio6axx.dll` with 0xC0000409 at the same offset both times. It
did not reproduce in 62 focused runs.

**Decision:**
1. The automatic choice first creates an instance of the modern backends alone and asks it for
   adapters. If it has any, the application's instance enumerates the modern backends only; if
   it has none, or cannot tell at once, OpenGL joins them as before.
2. The adapter selection (modern before OpenGL, then real devices before software ones) is
   unchanged, so on a machine with a modern adapter the same adapter is chosen as before: a
   modern software adapter (WARP, lavapipe) already ranked before an OpenGL GPU.
3. One case changes: a machine whose modern adapters all fail to present to the window. It now
   fails with a message that names `--gpu-backend gl`, which still selects OpenGL alone.

**Consequences:** the OpenGL driver is not loaded when it is not used. Start-up asks one more
instance for its adapters before the window: on the owner's Windows machine the probe takes
0.29–0.31 s, and the adapter is chosen 0.57–0.58 s after start instead of 0.38 s, so the splash
shows about 0.19 s later (session logs, release build, 2026-10-25). The owner accepted this for
now. The `--gpu-backend gl` smoke test keeps exercising the fallback.

**Proposed improvement (not done):** skip the probe where a modern adapter always exists. On
Windows the DX12 backend also lists WARP, the software adapter, which the selection already
ranked before any OpenGL GPU, so OpenGL was never chosen automatically there; on macOS Metal is
always present. The automatic choice could then use the modern backends alone on both, at no
cost, and probe only on Linux, where an older machine may lack Vulkan. To be verified first:
that wgpu lists WARP on Windows. A cached probe result is the alternative, with more state.

## TD-101 — Locating the cavity regions that trap resin

**Status:** Implemented on 2026-10-25 at the owner's request, from the owner-approved mockup
("Cavity regions" in the layout canvas).

**Context:** after hollowing and drain holes, the Drain holes panel reported regions such as
"Cavity region 3 (2 mm³) has no drain hole" and "4 enclosed pockets under 1 mm³", but nothing
showed where they were, and a cavity region lies inside the model.

**Decision:**
1. **The core locates each region.** `CavityRegion` gains `point_mm`, a vertex of the region's
   outer surface (a point certainly on the region), and `bounds_mm`, that surface's exact
   coordinate extrema, which bound its nested islands too. Both are in the cavity mesh's
   coordinates (world millimetres as prepared), gathered while the components' volumes are
   measured; nothing else about drainage changes.
2. **Markers in the viewport.** With the Drain holes tool on the selected model, every region no
   hole drains has an orange marker at its point, drawn through the model like the holes' ghosts,
   a constant size on screen, smaller for pockets under 1 mm³. The pointer on one says what it
   marks. A switch in the Drainage section hides them, with a legend.
3. **Show.** In the Drainage section each undrained region has a Show button; the pockets are
   counted, and listed on demand (at most 20, the rest counted), each with Show. Show, or a click
   on a marker, frames the region (at least 12 mm across, the eye at least 35° above it) and cuts
   the models through the middle of its height, so the cavity shows from above; the region's row
   is highlighted and its marker larger. "Back to the whole model", at the viewport's top left,
   returns to the view and the section before.

**Consequences:** finding where resin is trapped needs no search inside the model. A marker is a
draw of one small uploaded mesh; nothing is computed per frame beyond the regions' list. Showing
a region's whole surface (its faces highlighted) is left for later, when the markers are tried.

## TD-102 — Layer heights from the section bar, and its typed layers

**Status:** Implemented on 2026-10-25 at the owner's request, from the owner-approved mockup
("Section panel and layer heights" in the layout canvas). It gives TD-064's variable and adaptive
layer heights an interface, and completes TD-098 point 4 (the section bar's typed layer and
keys).

**Decision:**
1. **The section bar** is 104 points wide. Each handle's layer number and height are value
   fields, typed or stepped. The handle last dragged, clicked near or typed into is the active
   one, outlined; with the bar focused, Up and Down move it a layer, Page Up and Page Down ten,
   Home and End to the plate and the top. Since R10 (owner, 2026-11-10) these are shortcuts,
   `Action::Section`, configurable, which work without the bar focused while no other control
   holds the keyboard.
2. **Planned layers.** The handles snap to the plate's planned layers, numbered from the plate.
   The core's planner computes them, as slicing does. They are exact for the calibrated height
   and for fixed ranges: a test slices the plate and finds the planned count. Adaptive ranges
   are planned from the models as opened, placed on the plate, while slicing reads the repaired
   models, so the Slice panel says "about" there. Planning runs on its own thread when it reads
   slopes, again only after a plate edit, and the last result stays in use meanwhile.
3. **Ranges from the bar.** A right click offers "Add adaptive layers from here…" and "Add
   fixed layers from here…" at the layer clicked, "Show everything", and on a range's band
   "Edit…" and "Remove". Ranges are bands on the bar, violet for adaptive and teal for fixed.
4. **The range window** has the start (a layer or a height, filled from the click), where the
   range ends (the next range or the top), the adaptive rule (thinnest and thickest layer,
   largest stair step, growth per layer) or the fixed height and an optional own exposure, and
   the exposure table. It refuses to apply, saying why:
   - a start within the bottom and transition layers;
   - a start another range already has;
   - heights outside the printer's limits;
   - a table that is not increasing or exceeds the printer's exposure;
   - a table that does not cover the range's heights (it is never extrapolated).

   It warns of a format that stores one height (SL1) and of a range above the models.
5. **The plan is the plate's** `karasu-layers` document: the first range from the plate with
   the calibrated height, then the user's ranges. Each change is one undoable plate edit, and a
   slice made before it is out of date; without ranges the plate has no plan. The exposure table
   is kept with the plate, provisionally, until the resin profiles hold it.
6. **The Slice panel** shows the heights in a line ("0.050 mm; Adaptive 0.020–0.100 mm from
   20.00 mm") and the planned layers' count, with the reason when the plan cannot be used.

**Consequences:**
- A plan made with one resin keeps that resin's calibrated first range. After a change of
  resin, slicing refuses it with the planner's reason, until the ranges are set again; nothing
  rewrites the plan silently.
- Tinting the models by range in the viewport, shown in the mockup, is not done: it needs the
  shaders to colour by height.

## TD-103 — The Layers tool

**Status:** Implemented on 2026-10-25 at the owner's request, from the owner-approved mockup
("Layers panel" in the layout canvas).

**Decision:**
1. **A tool of the Export step**, beside Slice and export, with a panel like the others (fixed
   width, a scrolling body, a fixed footer). It replaces the layer column at the window's right,
   which took room whenever a slice existed.
2. **The layer image:** the exported mask, reduced and seen from above with the plate's front at
   the bottom. Its material stays white, as exported, also under the verification's marks (owner:
   no longer dimmed). The wheel zooms at the pointer, a drag moves a zoomed view, buttons zoom
   out and in, and a double click or the enlarge button opens a larger, resizable window with
   its own zoom. With the image focused, the arrows, Page Up and Page Down, Home and End browse
   the layers.
3. **Follow in the viewport** (on by default): the section cuts the models at the shown layer's
   top; leaving the tool or switching Follow off puts the section's handles back.
4. **This layer:** the height, thickness, exposure and kind (bottom, transition, normal, or from
   the layer plan) as the print job resolved them: `SliceOutcome::layers` now carries each
   layer's `PrintedLayer` from `PrintJob::layer`. Then the exposed area, each pixel weighed by
   its intensity, and the islands, runs joined across rows where they touch (corners included),
   both counted from the mask once per layer shown.
5. **The footer:** first, previous, the typed layer "of n", next and last, a slider, and the
   layer shown with its height, or that the slice is out of date.
   The larger window has the same slider between previous and next.
5b. **Printed walls in Layers:** a verified slice lists its findings in the Layers panel too,
   each showing its layer with the finding ringed; in the Slice panel a finding opens the
   Layers tool on its layer.
6. Two icons join the generated set: layers (a frame with a section) and enlarge (four
   corners).

**Consequences:** a slice keeps a few numbers per layer more (its top, thickness, phase and
exposure); counting islands walks the shown layer's runs once.

## TD-104 — Panel orientation verified for the Saturn 4 Ultra 16K, and GOO mirror flags

**Status:** Implemented on 2026-10-26 after the owner found a GOO export that looked mirrored in
UVTools. The Saturn 4 Ultra 16K's orientation is superseded by TD-153 (2026-11-14), which follows
ELEGOO SatelLite instead of ChiTuBox.

**Observation (2026-10-26):** the same model (HillGiant_32mm.stl, turned 90° about X, centred,
hollowed with 1.2 mm walls) was exported for the ELEGOO Saturn 4 Ultra 16K by karasu-slicer
(`export_test_1.goo`) and by ChiTuBox (`chitubox_export_test_1.goo`). Their stored layers were
decoded and compared on a 40-pixel grid at 5, 20, 40 and 60 mm:

| karasu-slicer profile | Overlap with ChiTuBox, as stored | Best other overlap |
| --- | --- | --- |
| no mirroring (revision 1) | 0.00–0.16 | 0.89–0.93 turned 180° |
| mirror X only | 0.00–0.25 | 0.89–0.93 mirrored in Y |
| mirror X and Y | **0.97–0.98** | under 0.24 |

So the first export was turned 180° against ChiTuBox, not mirrored: the printed part keeps its
handedness but faces the printer's back. A mirror in X alone, which the ELEGOO entries of an
open profile dataset suggest, would have printed a mirrored part. ChiTuBox also writes the GOO
header's mirror bytes, after the resolution, as X 1 and Y 0, which UVTools reads to show the
layer flipped back; karasu-slicer wrote 0 and 0 whatever the profile said. Its raster puts the
plate's front in the first row, where these files have the plate's back.

**Decision:**
1. The Saturn 4 Ultra 16K preset mirrors in X and Y (revision 2), generated by
   `tools/assets/make_presets.py`. A new export matches ChiTuBox's stored image (0.97–0.98) and
   its mirror bytes.
2. The GOO writer declares the stored image's mirroring against the view from above with the
   plate's back first: X as the profile's X mirror, Y as the opposite of its Y mirror, since
   karasu-slicer's raster puts the plate's front first. This holds for every GOO profile
   (owner, 2026-10-26): a profile without mirroring stores Lychee's image and declares X 0, Y 1,
   as Lychee does, where it used to declare 0, 0, which was false.
3. The other presets (all Anycubic) keep no mirroring. For the Photon P1, an earlier
   comparison of Base_01 found karasu-slicer's layer equal to Lychee's and turned 180° from
   ChiTuBox's, so the references disagree and nothing changes until a print or Photon
   Workshop settles it.

**Lychee (2026-10-26):** Lychee's export of the same model for the same printer stores ChiTuBox's
image turned 180° (overlap 0.90–0.93 once turned, 0.00–0.05 as stored), which is
karasu-slicer's image without mirroring, and declares X 0, Y 1. Each of the two references
declares its own image truthfully; they differ only by which side of the plate is its front, so
a part never prints mirrored from either, if the firmware honours the flags or not. The
revision-1 file had Lychee's image but declared 0, 0: with a firmware that honours the flags it
would have printed mirrored.

**Photon P1 (2026-10-26):** for HillGiant on the Photon P1, Photon Workshop 4.2.1 (Anycubic's own
slicer) stores the image karasu-slicer stores unmirrored turned 180° (overlap 0.93–0.95 once
turned, 0.00–0.19 otherwise) and declares `rotate_z` 180 in the PP1 machine settings; ChiTuBox
does the same for Base_01, while Lychee stores karasu-slicer's orientation and writes no
`rotate_z`. ChiTuBox names its setting for both printers "LCD_mirror".

**Decision, continued (owner, 2026-10-26):** karasu-slicer follows the manufacturer's slicer and
ChiTuBox, the LCD_mirror convention, since a part turned on the plate costs nothing while a
mirrored one is lost. PP1 files declare `rotate_z` from the stored image: 0 for an unmirrored
profile, 180 for one mirroring in X and Y. A profile mirroring along one axis cannot be declared
by a turn, so a PP1 export from it is refused rather than written with a false `rotate_z`.

**Consequences:** Saturn 4 Ultra 16K exports change, and every GOO header of an unmirrored profile
now declares its Y mirroring; projects and user copies of the preset keep their own
orientation. The profile's flags and the header's differ in Y because karasu-slicer's raster
puts the plate's front first; making the raster put its back first would make them equal, but
would change every printer's output, and is left for the profiles review. Each other printer
needs a reference file before its orientation is trusted (SCN-08).

## TD-105 — PP1 motion bands follow the job's motion order

**Status:** Implemented on 2026-10-26, found while reading Photon Workshop's P1 settings for the
owner.

**Context:** a job's retract stages are listed in motion order: a two-stage retract covers the
second lift stage's distance first, then the first's (OUTPUT-FORMATS-SPEC, two-stage motion). PP1
describes motion as bands from the plate's lowest position up (`normal_0`, `normal_1`), each with a
height, an up speed and a down speed. The PP1 adapter paired retract stage *i* with band *i*. So it
refused Photon Workshop's standard P1 motion (4 mm at 3 mm/s, then 5 mm at 8 mm/s), whose stages
mirror rather than match. With two equal stages it would have swapped the down speeds, bringing the
plate down onto the film at the fast speed.

**Decision:** a PP1 band's down speed is that of the retract stage covering it: the last retract
stage covers band 0. A PP1 export requires the retract stages to mirror the lift stages, as Photon
Workshop files (`.pwmx` and related) already do. A single stage is unchanged.

**Consequences:** PP1 exports of two-stage profiles now write the speeds Photon Workshop writes
for the same settings, as the Photon P1 preset's do. Single-stage profiles are unchanged.

**Confirmed (2026-10-26):** Photon Workshop's manual names Step[0] the slow stage near the curing
face, and Lychee's P1 export writes its last, slower retract speed on band 0.

## TD-106 — Printer and resin catalog, the user's library and print settings

**Status:** Accepted on 2026-10-26 (owner), with the proposals of points 4, 5 and 8 and the name
"print setting"; for the profile review the owner placed before the interface review (M6.5).
Implemented on 2026-10-26 after the owner approved the mockup; see "Implementation" below.

**Context:**
- The nine printer presets sit in one folder and the printer menu lists all of them, although a
  user owns one or a few printers. The catalog could reach hundreds of printers.
- A resin profile holds the material (name, density, cost) and one calibration per printer
  identity. A resin from one maker cannot be offered for another maker's printer without
  duplicating the material, and resins from makers of no printer (Siraya, Sunlu, Phrozen…) have
  nowhere natural to go.
- Copying a printer leaves it without resin calibrations, so a copied printer cannot slice.
- A comparison of slicers found that every slicer separates
  a read-only catalog from the user's printers. CHITUBOX and Photon Workshop list only the printers
  the user added. Photon Workshop and PrusaSlicer separate the material from the print settings
  for a printer and layer height, and Photon Workshop names them by resin, release film and
  quality.

**Decision:**

1. **Three kinds of profile:**
   - **Printer:** the machine, as today, plus `vendor` (a catalog vendor identity, optional for a
     custom printer) and `model`. Exposure never belongs to it.
   - **Resin:** the material only: `vendor`, name, kind (standard, ABS-like, water-washable…),
     density, cost and currency. No printer reference.
   - **Print setting:** exposure and motion for one printer and one resin: name (for example
     "Normal 0.05 mm"), printer identity, resin identity, release film (optional), the existing
     `PrintSettings` unchanged (layer height, bottom and transition layers, exposures, rests,
     motion), its source and a status: **official** (a manufacturer's value), **starting point**
     (copied from another resin or printer, to be calibrated) or **user** (edited). Several print
     settings may exist for one printer and resin (layer heights, films, qualities).
2. **Catalog** (built-in, read-only, compiled into the executable):
   ```
   assets/catalog/
     vendors.json                                   identity, display name, website
     printers/<vendor>/<model>.json
     resins/<vendor>/<resin>.json
     settings/<printer vendor>/<printer model>/<resin>[-<variant>].json
   ```
   - Identities stay global and stable (`anycubic-photon-p1`); a published identity is never
     reused, and a corrected entry is a new revision (as TD-104's Saturn).
   - `tools/assets/make_presets.py` becomes the catalog generator: it writes the files, the
     provenance README and a generated list of the files to compile in; a test checks that the
     list matches the folder, that identities are unique and references resolve, and that every
     catalog printer slices in each of its formats.
   - Each entry records its sources; a printer records what has been verified (dimensions,
     orientation against a reference file, release type), so the interface can say
     "orientation not verified".
3. **The user's library** (the core's `ProfileStore`, three kinds):
   - **Adding a printer** opens the catalog (filtered by vendor, searchable) or starts a custom
     printer. The chosen entry is copied as a user printer with `based_on` (catalog identity and
     revision). The printer menu lists only user printers, then "Add printer…" and "Manage…".
   - **Adding a resin** opens the resin catalog; resins with an official print setting for the
     plate's printer come first. The resin is copied as a user resin, and the catalog's print
     settings for that printer and resin are copied as user print settings linked to the user's
     printer and resin.
   - **The resin menu** lists the user's resins that have a print setting for the plate's
     printer; a print setting menu appears when there is more than one.
   - **Copying a printer** copies its print settings too.
   - **Several printers of one model:** a catalog entry can be added any number of times, each
     copy with its own identity. Each printer in the library has a unique name, which identifies
     it to the user: a second copy is offered as "Photon P1 (2)", and a name already in use is
     refused. Renaming changes only the name, never the identity.
   - **First start:** an empty library invites the user to add a printer.
4. **A printer and resin without a print setting:** allowed only by an explicit "start from…"
   that copies another print setting for the same printer, marked *starting point* until the
   user edits it; never offered silently (owner). Refusing until the user enters every value was
   the alternative.
5. **Catalog revisions:** when a user copy's `based_on` revision is older than the catalog's, the
   interface notes "update available" and shows the differences; the user applies them as a new
   revision or dismisses them. Nothing changes on its own (owner).
6. **Projects** (the project file is M6.6): a plate keeps complete snapshots of its printer, resin
   and print setting, as `PlateContent` does today, so a project slices identically whatever the
   library becomes. Opening a project whose profiles differ from the library's copies of the same
   identity says so and keeps the project's.
7. **Schema version 2 and migration.**
   - Printer, resin and print-setting documents gain `schema_version` 2.
   - A version-1 resin with N calibrations becomes one resin and N print settings, identified
     `<resin>@<printer>`, with status *user*; version-1 printers keep their identity, without a
     vendor.
   - The store migrates on first opening, keeps the original files in a dated backup folder,
     and reports what it migrated; a file it cannot migrate is reported, not dropped.
   - The command line keeps reading version-1 printer and resin files, converting them in memory,
     and gains a `--settings` file for version 2.
8. **Plate settings that other slicers keep with the resin:** anti-aliasing, minimum grey and XY
   compensation are plate settings today; CHITUBOX, Photon Workshop and Lychee keep them (or some
   of them) with the print setting. They move into the print setting, where a manufacturer's
   default can carry them (Photon Workshop's P1 default is anti-aliasing 16); the plate's repair
   and hole options stay on the plate (owner).
9. **Physical printers (owner, 2026-10-26), later:** a library printer is a *configuration*
   (model, panel, formats, print settings), and slicing depends only on it. When printing over
   the network arrives, a separate *physical printer* will name each machine (a unique name,
   its configuration, network address, serial number, notes, and possibly its own corrections
   such as an exposure offset). Sending a sliced plate then picks one or more physical printers
   whose configuration matches the plate's: in a print farm, twenty Photon P1 share one
   configuration and its print settings, and each is still identified by name. Exporting to a
   file needs no physical printer. Nothing in this decision may tie a configuration to a single
   machine; a user who calibrates machines differently keeps one configuration per machine.
10. **Out of scope:** downloading catalog updates, importing third-party data (the decisions in
   PRINTER-PROFILE-SOURCES-RESEARCH), and new parameters such as shrinkage compensation, which
   need their own decisions.

**Order:** the interface mockup (the printer and resin selectors, the catalog, the library
manager); the schema, store and migration in the core; the catalog layout and generator; the
interface; then catalog content.

**Implementation (2026-10-26):**
- **Core (`profiles`):** `ResinMaterial` (document kind `material`), `PrintSettingProfile` (kind
  `print_setting`) with `ImageSettings` (anti-aliasing, minimum grey, XY compensation), and
  `CatalogRef`; printers gain optional `vendor`, `model` and `based_on`, written only when set,
  so a version-one printer keeps its bytes. Printers read schema 1 or 2; calibrated resins
  (`ResinProfile`) stay version one and remain what a print job takes:
  `PrintSettingProfile::job_resin` composes one from a resin and its setting. `profiles::catalog`
  holds the catalog index and `differences`, which lists a revision's changes field by field.
- **Store:** `ProfileStore::library` is `profiles/v2`, beside the untouched `profiles/v1`;
  `remove` deletes a profile into the revision history.
- **Catalog:** `assets/catalog/` with `catalog.json` (vendors and the formats their printers
  read, files, release type, orientation verification, sources), `printers/<vendor>/`, `materials/<vendor>/` and
  `print-settings/<vendor>/<printer>/`; `tools/assets/make_catalog.py` (the former preset
  generator) writes it, its sources (in `docs/CATALOG-SOURCES.md` since 2026-11-10, with a short
  README in the folder) and the list compiled into the executable. The ELEGOO
  setting is marked a starting point: its bottom layers and motion are karasu-slicer's choices.
- **Library (`karasu-project::library`):** the catalog checked whole; the user's printers,
  resins and settings; adding from the catalog or custom, unique names, starting points,
  revisions, duplicating with settings, removal with dependents, catalog updates offered,
  applied or dismissed, and imports (a version-one resin becomes a resin and its settings).
  The first opening converts `profiles/v1` through a staging folder published whole, and
  reports what it converted and what it left out.
- **Plates:** `PlateContent` holds the resin and the print setting; a setting counts only for
  the plate's printer and resin; `Project::request` takes the job's resin and image settings
  from it. `PlateSettings` no longer holds anti-aliasing and compensation.
- **Command line:** `--print-setting <file>` with a library resin as `--resin`; the image
  options are refused beside it; version-one files work as before.
- **Interface (`karasu-app::profiles_ui`):** the sidebar's printer, resin and print setting
  menus, the windows of the mockup (adding a printer or a resin, setting one up, the print
  setting editor, printer and resin forms, the library, a catalog update), opened profile files
  imported, and the Slice panel showing the setting's values with a link to its editor. The
  windows follow the owner's window rule of 2026-10-26 (`docs/UI-DESIGN.md`, "Windows"): modal
  dialogs drawn by `karasu-app::dialog`, stacked when one opens another, sized as the rule's
  table says. A resizable dialog lays out its title, body and footer by hand and claims exactly
  its own area: `egui::Resize` grows to whatever its content claims, so a footer a few pixels
  taller than planned made the dialog grow at every repaint. Formats are shown by readable name (`karasu-project::request::format_names`), and
  the reference format is no printer's: it is offered only under the Slice panel's Advanced.
- **Not yet:** greying out the settings a printer's format cannot store (the mockup's format
  switch): an export still refuses them, with its reason. Physical printers (point 9) come with
  network printing. No dependency was added.

**Alternatives considered:**
- **Keep resins with per-printer calibrations and add a vendor field:** fixes the listing, not
  the duplication of materials across printers or resins from makers without printers.
- **Exposure settings inside the printer, as CHITUBOX does:** a printer file then carries every
  resin's settings, and a resin used on two printers is two unrelated entries.
- **Library entries that reference the catalog instead of copying it:** a catalog revision would
  change a user's settings without their action.

## TD-107 — Overhang analysis: slope on the model, local minima, unsupported regions per layer

**Status:** Accepted on 2026-10-26 (owner), before the manual supports (M6.4g). Research:
overhangs and islands.

**Context:**
- The manual supports, and later automatic orientation and automatic supports, need to know which
  parts of a model need support in its current pose. karasu-slicer computes nothing of the kind
  today.
- Other slicers analyse either the model's surface (overhangs by slope, minima) or the sliced
  layers (islands). The surface analysis misses the points where material starts in mid-air (a
  downward cone's tip); the layer analysis is the reference but needs a slice.
- At the default 45° and 0.05 mm layers, a layer may stand out from the one below by 0.05 mm,
  one to three pixels: a layer comparison on pixels is dominated by rasterisation.

**Decision:**
1. **The critical angle** is measured from the plate's plane: a surface facing the plate whose
   slope above level is under the angle needs support. 45° by default; the user sets it (other
   slicers keep it fixed or per printer), and it is kept with the settings (TD-097).
2. **Three levels:**
   1. *Slope*, per face from its normal in world coordinates, computed in the viewport's shader
      against the angle as a uniform: the model is tinted live as it turns, with nothing computed
      on the processor and nothing uploaded again. A gradient runs from the angle's limit to a
      level surface.
   2. *Local minima*: vertices, and level edges, lower than all their neighbours, where an
      island starts; computed off the interface thread when the pose changes, in the headless
      core.
   3. *Unsupported regions per layer*, the reference: each layer's sections minus the previous
      layer's grown by the self-supporting distance `h / tan(angle)`, on the slicer's **exact
      integer contours** (owner, option A), so the result does not depend on a pixel grid. An
      island is a region with nothing below it within that distance; an overhang, a region beyond
      it. Slivers found layer after layer are joined across layers into regions in space.
      Each region keeps where its islands start (`IslandStart`, 2026-10-30, TD-112): pieces
      starting in mid-air at different layers may grow into one region, and each needs its
      support where it starts. The summary says how many (`Island starts`).
      Measured on `Base_01.stl` before it is relied on; pixels at the printer's resolution with a
      conservative rule are the fallback, never a coarser scan.
3. **Nothing hidden by default:** no minimum island area (a display filter at most). Islands,
   overhang regions and minima are reported separately; Photon Workshop's "connected but
   possibly hanging" class is left out for now.
4. **Showing (owner):**
   - now, the slope tint on the model only;
   - islands, overhang regions and minima only in the future Supports panel, switched on and off
     there, so the scene is not loaded when they are not needed.
5. **Headless and deterministic:** levels 2 and 3 live in `karasu-core` (`f64`, deterministic
   order, explicit tolerances), with a command-line report for tests and comparisons; level 1 is
   rendering only (`f32` in the shader).
6. **Verification:** once supports are placed, the model sliced with its supports and analysed
   again must show no unsupported island, as the wall verifier checks walls (TD-095).

**Consequences:**
- The manual supports start from the analysis; automatic orientation (an area-weighted histogram
  of normals scored per direction) and automatic supports can be built on it if the owner brings
  them into M6.
- A test corpus is needed: inverted cone, bridge, ramps at 30°, 44°, 46° and 60°, a hollowed
  cube's ceiling, a floating sphere, a flat torus, and the owner's models.

**Implementation of level 1 (2026-10-26):**
- **Shader.** The scene's mesh shader takes the face's normal from screen-space derivatives,
  turned towards the eye (the side seen: the outside of a closed model), once per fragment
  before any branch. On a model's draw (only models carry the outside filter) a face whose
  `-normal.z` exceeds the cosine of the critical angle, above 0.01 mm (a base resting on the
  plate is no overhang), takes a colour from the limit's (yellow) to the level one's (red),
  mixed by its opacity; the outside filter then applies over it.
- **Uniforms.** The frame's block grows from five vectors to eight: the angle's cosine, the
  switch and the plate's contact height, then the two colours. Nothing is uploaded when the
  model turns or the angle changes.
- **Settings.** View › Show overhangs (O) and the Settings dialog's Supports section (the
  critical angle, 1–89°, 45° by default, and the switch); the two colours in Appearance; all
  kept across restarts (TD-097). The switch is on by default (owner, 2026-10-26).
- **Measured** (native Windows, Vulkan, AMD Radeon RX 9070 XT, the 2,702,172-triangle raven,
  the smoke test's 90-frame orbit, two runs each): 16.29 and 16.34 ms per frame with overhangs
  off, 16.38 and 16.37 ms with them on. The frames are held at the display's 60 Hz, so the
  shading's cost stays under it.

**Implementation of level 2 (2026-10-26):** `karasu_core::analysis::minima::local_minima`.
- **Groups.** Vertices joined by edges whose ends are level within `LEVEL_TOLERANCE_MM` (1e-6 mm:
  a rotation leaves a level face's vertices differing by rounding, about 1e-12 of their size)
  form one group: a point, a level edge, a level loop (a flat torus) or a level face. A group is
  a minimum when every edge leaving it climbs.
- **Material above.** The faces around the group must face down, by their area-weighted
  normals in the placement (the mesh oriented outward; a mirroring placement is accounted for),
  so a cavity's floor, lowest in its own surface but with material below, is no minimum.
- **On the plate.** A minimum at or under 0.01 mm is marked as resting on the plate.
- **Report.** `inspect --minima [--rotate <x> <y> <z>]` turns the model, lowers it onto the
  plate and lists the minima (at most 100, with the total).
- **Measured** on the 2,702,172-triangle raven (Linux, release): 0.71 s for the whole report,
  most of it importing the 135 MB STL, 430 MB resident. It finds 988 minima, 946 in mid-air: a
  sculpted surface has a minimum at every small dip, most of which a layer covers at once. Telling
  those from real islands is level 3's work (a minimum whose surroundings rise less than the
  self-supporting distance within a layer is no island); level 2 alone is not a list of supports
  to place.

**Level 3 on pixels (owner, 2026-10-26), superseding option A for it:**
- **Why.** The exact composition (`slicing::construct`) tests every pair of a layer's segments,
  capped at 4096 edges by default: it was built for the preparation's small parts. Measured
  sections: `Base_01.stl` 1,755 to 2,869 segments, the raven 715 to 6,816. The difference alone
  is about 17 million pair tests per layer on `Base_01` (2,097 layers), and growing the layer
  under it edge by edge makes 30,000 to 110,000 edges: hours to days per model. Option A needs a
  new sweep-line exact Boolean and offset kernel first. The owner chose the fallback the
  decision already named: pixels at the printer's resolution.
- **Definition.** On the exported masks (supports in the job are material): a pixel of layer `k`
  touched at all (gray 1 or more) is material, so no island smaller than a pixel is lost; a
  pixel of layer `k - 1` half covered supports, as the wall verifier's material. A material
  pixel is supported when a supporting pixel's centre lies within `Δz / tan(angle)`, measured
  exactly on the lattice with each axis' pitch, a relative 1e-9 absorbing the rounding of the
  tangent and of the planes (a slope at exactly the angle holds). Pieces join pixels by edges
  and corners (8-neighbours). A piece with nothing in reach is an island; the unsupported part
  of a piece that rests elsewhere is an overhang. Parts of consecutive layers within reach join
  into regions, by lowest layer then position. The first layer rests on the plate.
- **Implementation.** `karasu_core::analysis::unsupported::find_unsupported`, on runs: work and
  memory follow the masks' runs, two layers at a time. `slice --unsupported [--overhang-angle
  <deg>]` keeps the masks as the wall verification does and adds the counts, the areas and the
  first twenty regions (islands first, lowest first) to the summary; it never fails the job.
- **Measured** (Linux, release, Photon P1 at 0.05 mm, exact repair):
  - `Base_01.stl`: 3.80 s without, 4.29 s with the analysis; 26 islands and 2,795 overhang
    regions, against 4,250 local minima in mid-air. Its largest island starts at layer 1: the
    sculpted base touches the plate at one vertex and the rest of its underside lies 0.1 mm
    and more above it (the maker ships a supported version).
  - The raven: 5.06 s without, 5.20 s with; 350 islands and 4,511 overhang regions, against 946
    minima in mid-air.
  - Keeping the masks costs their memory, as for the wall verification (`Base_01`: 243 MB
    resident without, 574 MB with).
  - Many overhang regions are slivers along slopes near the angle, where rasterisation decides;
    the Supports panel will show them on demand, with their areas.
- **In the interface** (2026-10-29): the Islands tool (TD-108) finds the islands of the selected
  model, sliced alone without its supports, and shows the local minima on demand.

**Alternatives considered:**
- **Pixels at the printer's resolution:** cheaper to write on the existing masks, but the 1–3
  pixel band makes the result depend on rasterisation; kept as the fallback.
- **A coarser scan (Lychee):** faster, but the island count changes with the scan.
- **Slope alone:** misses the minima, as PreForm's "Support All Minima" and the literature show.

## TD-108 — The Supports tool

**Status:** Accepted on 2026-10-26 (owner, from the mockup "Supports workflow (M6.4g)");
implemented in stages (M6.4g) through 2026-10-30, with native Windows validation passed the same
day.

**Context:**
- The engine for manual supports exists (TD-062): a support document, deterministic part meshes,
  validation against the sliced surface (anchors, axis, piercing, proven clearance, drain holes,
  build volume) and the union with the model. The overhang analysis exists too (TD-107). The
  interface offers neither: the Supports step is disabled.
- A support document is written in world coordinates for one placement; the command line binds it
  to that placement and lets it follow moves on the plate and turns about the vertical axis only
  (TD-072). In the interface the user moves, turns and scales a model at any time.

**Decision (owner):**
1. **One tool, Supports,** in the Supports step, in a tool panel that follows the panel rules
   (TD-098): Advanced switch in the title bar, a scrolling body, a fixed footer.
   - *What needs support:* switches for the overhang tint (the same as View › Show overhangs),
     the local minima and the islands. **Find islands** slices the model with its supports in the
     background, as Slice does, and finds the islands and overhang regions (TD-107 level 3); a
     navigator steps through the islands (layers, area, whether a support holds it) and **Support
     it here** adds a draft support under the island's lowest point. Advanced: a display filter
     that hides the markers of islands under an area (they stay found and counted).
   - *New supports:* Supports or Braces; **Place supports** (as Place holes); the weight (Light,
     Medium, Heavy: TD-062's presets); contact diameter, tip length, shaft diameter and
     penetration; the foot, **Automatic** (on the plate, or on the model when the model is below
     the tip) or **Plate only**. Advanced: the tip's direction, the contact's shape, the pad, a
     tapered shaft and the sides.
   - *The list:* one row per support (new, inside a cavity, a finding's icon, a bin); a row opens
     that support's values, which change it alone. A colliding support offers **Accept the
     collision**: it prints, and the finding stays reported.
   - *Base:* the lift and the raft (thickness; Advanced: margin and side slope).
   - *Checks* (Advanced): the clearance and the tip angle limit.
   - *Footer:* **Apply supports (N)** and **Discard**; Apply waits while a collision is not
     accepted. The state: placing, an error, islands not searched, out of date, islands without
     support, or every island supported.
2. **Drafts, then Apply,** as drain holes: new supports are drafts until applied; applying them is
   one undoable step.
3. **Islands found before a change are out of date** until found again: the panel says so; it
   does not slice again by itself (about 4 s on `Base_01`).
4. **The automatic foot** stands on the model when the model is below the tip.
5. **In the scene,** the selected support shows two handles, its tip and its foot. Bends, moving a
   foot along a shaft and editing several supports at once come later.

**Revised by the owner on 2026-10-27, after trying stage 2:**
- **Applied at once:** no drafts, no Apply or Discard. A click adds the support to the model as
  one undoable step, even when it shows an error: it is drawn red, kept, and changed afterwards
  (moved, resized, its tip turned). This replaces point 2 above.
- **Errors do not stop the print:** other slicers print red supports, so the interface slices
  supports with error findings as they are built and warns (the Slice tool's message, the
  status bar, the panel's state); a support that cannot be built at all is left out and
  reported. The command line keeps refusing them. This is an explicit, reported policy of the
  interface's requests (`SliceRequest::slice_support_errors`), not a silent repair.
- **Separate tools:** the Supports step gets four tools, each with its own panel: *Supports*
  (placing and editing, the weights and the user's own presets, the checks under Advanced),
  *Islands* (finding them, stepping through them, supporting them; later automatic supports
  from them), *Braces* (manual, later automatic) and *Raft* (the raft and the lift, later the
  raft's type).
- **The user's presets:** besides Light, Medium and Heavy, presets the user adds, renames and
  removes, each with every value, kept with the settings. Done on 2026-10-29: a menu of the
  presets in the panel with save, rename and delete; the Settings dialog chooses the preset new
  supports start from, built in or the user's own (kept by name).
- The handles (tip, foot, the tip's direction) come before the islands. A fourth, the knee
  (owner, 2026-10-29), raises or lowers the top of the vertical shaft along Z only: the shaft
  stays where it stands (so it never moves towards its neighbours), the tip points at the knee,
  a bend of the support (TD-062), at most 45° from vertical. Moving the foot moves the knee with
  it, keeping the shaft vertical, unless the switch "Knee follows the foot" (on by default) is
  off. A small axis gizmo (owner, 2026-10-29) moves the knee sideways too: once pressed, the
  knee shows X, Y and Z arrows and a square for its level plane, a foot on the plate X and Y
  arrows; an arrow holds the drag to that world axis (the closest point of the pointer's ray to
  the axis, as the Move tool's arrows), so a move stays deliberate and the support never drifts
  towards its neighbours by accident. The tip, its direction and a foot on the model stay free:
  they slide on the surface, which an axis would leave.
- **Tips only** (owner, 2026-10-29): a strip switch that, with the Supports tool in use, draws
  only the supports' contact parts (their tips) by leaving the other parts' triangle ranges out
  of the draws, nothing uploaded again; picking then tests only the tips' triangles, so a hidden
  shaft never takes a click.
- **The Raft tool** (owner, 2026-10-29, canvas "Raft tool with the lift"): the selected model's
  lift (its pose's resting height, the Transform panel's Z) and the raft of its support document
  (thickness; margin and side slope under Advanced), both the model's values, each change one
  undoable step. A new raft takes the defaults set in Settings › Supports; values outside the
  document's ranges in the settings file fall back to the built-in raft (1 mm, 1 mm, 60°). The
  raft is built and drawn with the model's supports; the panel shows its footprint from the
  latest build. Revised the same day (owner, after trying it: the raft added a slab under the
  pads instead of becoming the feet): with a raft, plate supports have no pads; each shaft stands
  on the raft through a fillet (a frustum sunk half the raft's thickness into it, its diameter
  and height in the raft spec), and the raft is the hull of the fillets. Rafts lie one under
  every foot or one per group of feet closer than a distance (single-linkage over a grid of that
  size, deterministic). The document gains three optional raft fields, read as the defaults when
  absent; only the Slab type exists for now.
- **List windows** (owner, 2026-10-29, canvas "List windows"): a model may have hundreds of
  supports, which a list in the panel buried the values under. The supports, and for
  consistency the drain holes, are listed in a tool window opened from the tool strip (L):
  not modal, moved, resized and reopened where it was left; it follows the selection both ways
  and filters the items with a problem. The panel keeps the selected item's values. Only the
  rows in view are laid out (`ScrollArea::show_rows`), measured at 0.03 ms a frame for 10 to
  10,000 rows. The Islands and Braces tools will list theirs the same way.
- **The cavity view** (owner, 2026-10-29): outer geometry hid the cavity while supports were
  placed in it. A tool strip toggle shows the selected hollowed model's outside as a ghost
  (translucent, no depth written, drawn after the opaque scene) and its cavity from inside
  (its faces turned towards the eye culled by winding, the cavity mesh being outward-oriented
  as a solid of the void), with new supports going into the cavity (owner's choice of the
  ghost over hiding the outside entirely, and of switching the surface with the view). It is a
  viewport `Look` per draw: pipeline states only, nothing uploaded again; the section does not
  cap such draws. The view is not kept across sessions: it belongs to the model at hand.
- **Supports with errors stay visible** (owner report, 2026-10-29): supports in the cavity with
  an error disappeared, because the builder dropped the parts of a support whose axis exceeded
  the limit or whose penetration pierced a surface, against the specification's "validation
  never drops anything". Such a support is now built and reported: drawn red, selectable, and
  printed with a warning as the other red supports are (the CLI still refuses errors). Supports
  without a defined shape (an anchor off the surface, an invalid path) still have no parts.
- **A foot on the model turns as a tip does** (owner, 2026-10-29): a support from the model to
  the model has two tips, so its foot shows the tip's handles: a dot that slides on the surface
  and a direction ring beyond the foot's join along its axis, which sets the foot's axis
  (`BaseSpec::Model`'s `AxisSpec::Direction`, in the model's frame) at most the document's axis
  limit from the surface's normal there, as the build requires. Moving the foot puts its axis
  back along the normal.
- **Placing tip then foot** (owner, 2026-10-29): an option of the tool strip's Place button,
  beside one click. The first click places the tip; the new support's foot then follows the
  pointer by the same rule as the foot handle (the plate, or the model below facing up, the
  shaft vertical under a knee at most 45° from the tip), built alone as a preview, and the
  second click adds the whole support as one undoable step. It is held as a drag that is not
  in the document yet, so Escape, a change of mode or surface, or leaving the tool drops it
  without an undo step, and the camera still turns between the clicks.
- **Supports inside the cavity** (owner, 2026-10-29): the panel's Surface choice (Outside or
  Cavity) picks the cavity's ceiling where the section shows it, as drain holes do; the route
  is always found there (the foot on the cavity's floor, which the hollowed shell's surface
  gives), and the build classifies the contact as interior (TD-062). Handles of such a support
  move on the cavity's surface.
- **Supports placed clear of the model** (owner, 2026-10-29): where a shaft straight down would
  go through more of the model, a new support goes aside first (the nearest clear vertical
  column, the tip leaning to a knee at most 45° from vertical and within the axis limit), then
  stands on the model below, then goes straight down with its collision reported. The panel's
  switch "Avoid the model" (on by default) turns it off. `karasu_core::supports::find_route`
  checks capsules (the support's widest radius plus the clearance) around the path's straight
  parts against a uniform grid of the surface, exactly per candidate triangle after a
  bounding-sphere test; columns are tried ring by ring (every 0.5 mm, up to 15 mm, 16
  directions), so the result is deterministic. It guides placing; the build still proves the
  clearance. Measured on `Base_01.stl` (1,009,544 triangles, Linux release): the grid in
  110–180 ms, once per preparation and placement, off the interface thread; 244 routes from
  tips under the model: 0.63 ms on average, 32 ms at worst (a tip with no way, every column
  tried); 219 straight, 16 aside, 2 on the model, 7 none.
- **The mockup of the four tools** (canvas row "Supports step: four tools", 2026-10-27): the
  Supports panel keeps its three placing choices (Place: one click or tip then foot; Foot:
  automatic or plate only; Surface: outer or cavity) always in view for now, the owner to
  confirm after trying them; the user's presets are kept with the settings, for every
  project.
- **Tools open with their defaults** (owner, 2026-10-27, for every tool): the values a tool's
  next operation uses (the hollowing's wall and cell, the drain hole's diameter, depth and
  surface, the support's weight and values) start from defaults set in the Settings dialog
  only (its Hollowing and Supports sections); every time a tool opens it shows them, and what
  is changed in its panel is for the model at hand and never kept. The user's presets are where
  support values are kept. TD-097's "every setting persists" applies to settings and to the
  interface's state (switches, layout), not to values being worked on.
  **Revised by the owner on 2026-11-02:**
  - a tool's values stay while the user works, when the tool closes and opens again, and are
    never saved as defaults;
  - they go back to the defaults at start-up, with Edit › Restore all default values and, once
    M6.6 brings it, with a new project;
  - a value that differs from its default shows a reset button before its field (in the panels
    only, not in Settings), and for a support's values the default is its preset's;
  - a default changed in Settings carries to the values still at the old default, and those the
    user changed stay (`window::tool_values`).

**Implementation choices:**
- **Supports follow the model.** The interface keeps an object's supports in its model frame, as
  it keeps drain holes (TD-089): tips, bends, feet on the model and explicit axes follow every
  move, turn, scale and mirror; feet on the plate stand below the last point of their shaft.
  Heights on shafts (braces, supports standing on supports) are heights above the plate. The
  pipeline places the document with the object's placement before building it; the command line
  and plate documents keep world coordinates.
- **The supports are built where they are edited.** Every object's supports are built against
  its prepared surface (the hollowed shell, or the placed model) off the interface thread, the
  selected object's first; the scene draws the parts on the GPU, uploaded once per build, each
  support in its colour from one upload (`ScenePart` ranges). The preparation leaves supports
  out of its request and keeps its result when only supports change, so adding a support never
  prepares the model again; slicing builds them again from the plate, as for the command line.
  A build whose model moves is carried along until it is built again.

**Stages:**
1. The core and the pipeline: the document placed from the model frame, each support's
   triangles in the built mesh, the request's support coordinates. Done (7aed20a).
2. The panel, placing with its preview, the list, the build off the interface thread, drawing.
   Done (7aed20a); revised on 2026-10-27: applied at once, errors print with a warning, every
   object's supports built and drawn by the tool, the preparation kept when only supports
   change.
3. The mockup of the four tools (done, 2026-10-27), then the handles (tip, foot, the tip's
   direction) and the values under Advanced (done, 2026-10-27).
4. The user's presets and the automatic foot.
5. Islands, Raft (with the lift) and Braces, then the native Windows validation. Raft done
   (2026-10-29). Braces done (2026-10-29, owner mockup revised the same day): placed one by
   one or in a chain, as clicked or level, ends dragged along their shafts, and automatic
   braces between neighbouring supports, planned in the core (`plan_braces`) on a thread of
   their own and applied as one step; skipped braces are counted with their reasons.
   Branches placed by hand (2026-10-30, owner: phase 1 of branching; automatic branches in
   Support the islands and trees of several levels to come): a support standing on another's
   shaft, which the document already held (TD-062), placed in two clicks, its junction a
   handle; branches are deleted with their trunk, and are no thicker than it. Phase 2 the same
   day: Support the islands grows trunks and branches (tips within 5 mm, four branches a
   trunk by default); on `HillGiant_32mm.stl` 299 of 544 supports are branches (planned in
   0.32 s), 400 of 707 hollowed (0.67 s). Phase 3 the same day: trunks on the plate within
   10 mm merge, three at a time, two levels, the main trunk thicker below; on `HillGiant_32mm`
   with Light, 132 supports stand on the plate or the model instead of about 245 (planned in
   0.44 s), 193 of 707 hollowed (0.72 s). Trunks on the model merge as well; in the
   Supports tool, "Merge trunks" merges any supports standing on their own (a mode, applied as
   one step), and "Join a support" joins the selected one to a shaft clicked, both below what
   hangs from the shaft (branches and braces); the merging is shared (`supports_ui::trees`).
   Islands done (2026-10-29): the selected model alone, without its supports, sliced in the
   background with the unsupported regions (TD-107; `SliceRequest::allow_floating` slices a
   lifted model where it stands, for an analysis only), held islands told by the supports'
   tips, local minima on demand, "Support it here". Support the islands (owner mockup revised
   the same day): supports planned for every island without one in the background (joined
   islands, several on large ones, routed straight, aside or on the model, in the cavity ceiling
   to floor, checked to build), previewed and applied as one step. Measured on the owner's
   `HillGiant_32mm.stl` (500,510 triangles, lifted 5 mm, Linux release, 16 threads): solid,
   161 islands found in 0.41 s, 544 supports for 155 islands planned in 0.12 s; hollowed at
   2 mm, 166 islands (5 in the cavity) in 0.67 s, 702 supports for 158 islands in 0.61 s. The
   plan routes every tip on every core (5.3 s on one when hollowed: a tip in a cavity tries
   every column aside before standing on its floor), with the same result whatever the cores.
   An island's place (outside or in the cavity) is told as its tip is: the shell's surface
   straight above it.
   **Native Windows validation passed on 2026-10-30** (hash-verified snapshot of bcbb38d, 627
   files, `validate.ps1 -IncludeApp`, rustc 1.99.0 MSVC): no failed command. fmt, clippy,
   build and the public and private rustdoc pass. The Debug and Release workspace tests each
   pass 1,481 with 0 failed and 9 ignored; Linux runs 1,483, and the two extra are the Unix-only
   path tests. The application's three smoke runs pass:
   - Vulkan on the discrete adapter: 12.4 ms mean frame time;
   - the OpenGL fallback: 13.2 ms;
   - FX-03 in the viewport: 16.5 ms mean, 32.9 ms max.

   The snapshot's integrity check passed after the run.

## TD-109 — The tool strip

**Status:** Accepted on 2026-10-29 (owner, from the canvas mockup "Tool strip"); implemented the
same day.

**Context:** the Supports panel filled up with modes and switches beside its values; the Drain
holes panel had the same mix on a smaller scale.

**Decision:** a second, contextual panel: a narrow strip of icon buttons anchored at the
viewport's left, opaque but light (the panels' colour, a little translucent, no blur; the
frosted glass dropped in TD-098 stays dropped). Toggles are lit while on; a button with options
shows a corner triangle that opens them (and a right click). Every button is an action with a
configurable shortcut (TD-097): Place (P), Outside or cavity (C), Avoid the model (A), Knee
follows the foot (K), acting on the tool in use. Each tool chooses its strip: Supports (place,
surface, avoid, knee, overhangs, delete) and Drain holes (place, start from, delete). The panels
keep the values; their placing buttons and switches moved to the strip.

**Consequences:** `tool_strip::show` draws any tool's strip from its items; new icons for the
surfaces, avoiding the model, the knee and the overhangs. Place has two options, one click
or tip then foot (added the same day); the cavity view (V) joined the Supports strip the same
day (TD-108).

## TD-110 — Automatic supports, suction cups and automatic orientation move into the MVP

**Status:** Accepted on 2026-10-30 (owner), after the manual supports (M6.4g) closed.

**Context:**
- The ROADMAP kept automatic supports, automatic orientation and suction-cup detection after the
  MVP. With the manual supports done, most of what automatic supports need exists:
  - the per-layer unsupported regions, islands and overhangs (TD-107);
  - planning in the background, routing that avoids the model, cavity supports, trees, braces;
  - the core's checks.
- Without these three features the preparation workflow is behind CHITUBOX's and Lychee's.
  Competitiveness also rests on proven automatic supports, the printer catalog, the project file
  and distribution.

**Decision (owner):** the order after M6.4g is:
1. **M6.4h**, the mandatory preparation scenarios.
2. **M6.4i**, automatic supports in an essential version:
   - overhang regions supported as well as islands (today Support the islands uses only the
     islands);
   - tips spread over the downward surface by a density, denser where it is closer to level,
     instead of a grid over an island's bounds;
   - a closing check: the analysis runs again with the supports, and tips are added where
     anything is still unsupported.
3. **M6.4j**, suction-cup detection, as proposed on 2026-10-25 (ROADMAP, Post-MVP proposals).
4. **M6.4k**, automatic orientation, scored with the overhangs, the suction cups and the layers'
   areas. It comes last because it needs the other two to score a pose.
5. **M6.6** plates and project file, then **M6.7** hardening and distribution. The printer
   catalog is widened alongside.

**Consequences:**
- Post-MVP keeps support optimisation beyond the essential version: load and peel models,
  protected surfaces, and orientation beyond M6.4k's.
- No feature may claim it prints without physical print evidence under recorded conditions.
- Two candidates are noted, not scheduled: cutting large models, and macOS.

## TD-111 — The mandatory preparation scenarios, run from the interface (M6.4h)

**Status:** Accepted and done on 2026-10-30. The automated scenarios pass on Linux and native
Windows, with byte-identical exports. The owner's interactive run on native Windows passed.
Record:
[M6.4h scenarios](M64H-SCENARIOS.md).

**Context:** the ROADMAP's M7 requires seven preparation scenarios end to end, on native Windows
and Linux. The core and the command line already passed their headless versions (M5.5-E). The
desktop application had tests for each tool, but none for whole flows through it.

**Decision:**
1. **Automated scenarios** (`karasu-app`, `scenarios.rs`) drive the interface's own state:
   - the tools' hovers and clicks;
   - the panels' edits;
   - undo and redo;
   - the Slice panel's job, and the export (`SlicingUi::export_to`, which the save dialog now
     calls as well).

   Each scenario:
   - exports in the printer's own format;
   - checks that the STL file and the imported mesh never change;
   - compares exported bytes where the result must be the same.

   They run with every `cargo test`, so with every native validation too.
   `KARASU_SCENARIOS_OUT` keeps the exports for comparing platforms and for the independent
   readers.
2. **Saving and reopening a project is not applicable** until the project file (M6.6). Replay
   is checked in memory instead: undoing every step and redoing it gives the same bytes.
3. **Two diagnostics found by the scenarios:**
   - a slice whose summary says a cavity traps resin now also shows a warning, instead of only a
     success;
   - a slice refused because no support stands on the plate now lists the supports that could
     not be built, and why.
4. **An interactive checklist** for the owner covers the same scenarios on the real interface.

**Consequences:** physical printing stays out of scope: the scenarios prove geometry, files and
diagnostics, not drainage or support strength.

## TD-112 — The Automatic tool: every support planned at once (M6.4i)

**Status:** Accepted on 2026-10-30 (owner, from the mockup "Automatic supports tool"); implemented
the same day. Weight by load is a later phase, after M6.4j.

**Context:**
- TD-110 moved automatic supports into the MVP, in an essential version: overhangs covered as
  well as islands, tips spread by a density, and a closing check.
- The pieces existed in the Supports step: the per-layer analysis (TD-107), Support the islands'
  plan, routing clear of the model, trees and merging, and the Braces tool's automatic braces.

**Decision (owner, 2026-10-30):**
1. **A tool of its own, Automatic,** first in the Supports step, with its own panel. It is not
   the manual Supports panel. The Islands tool keeps Support the islands for now; the button may
   leave later. Feet on the model are on by default.
2. **What it plans:**
   - **Islands** are supported as Support the islands supports them; its planning is shared,
     not copied (`islands_ui::plan::grow` and `merge`).
   - **Overhangs:** each region found is probed on a grid half the tip spacing apart. A point is
     a candidate when its surface faces the plate less steeply than the critical angle, and it
     wants a tip within its reach. The reach is the tip spacing under a level surface and grows
     in proportion to the slope, up to the spacing at the critical angle.
   - **Placing:** candidates are taken lowest first. One becomes a tip when no tip is within its
     reach yet; kept supports' tips count.
3. **Kept and replaced.** Planned supports carry a new origin, `automatic`, in the support
   document. Generating again starts from the document without them and replaces only them.
   Supports placed by hand or by the Islands tool stay, and an island one of them holds is left
   to it.
4. **The closing check is geometric.** TD-110 proposed slicing again with the supports. That
   cannot tell coverage: the per-layer analysis judges each layer's self-support (a 0.05 mm
   reach at 45° and 0.05 mm layers), and the gap between any two tips exceeds it, so an overhang
   held by tips would still show as unsupported. Instead:
   - a candidate is covered when a kept support's tip is within its reach;
   - where a support finds no way down, the next uncovered candidates are tried, pass after
     pass, never a point tried twice, up to the passes asked for (3) or until a pass adds
     nothing;
   - what remains is joined into places left unsupported, each with the reason of the nearest
     failed tip. They are listed in the panel, marked in the viewport and said in the footer and
     the notification.
5. **The final check** (owner, 2026-10-30): the supports are built as a slice builds them,
   against the model, with the kept ones. A planned support built with an error goes: a tip
   piercing a thin part, a part too close to the model, a part outside the build volume, a path
   the build refuses. What hung from it (branches and merged trunks) is grown again on supports
   of its own and checked again, up to three rounds. The place is reported as left, with why.
   The plan never offers a support that a slice would build with an error.
6. **Apply** replaces the automatic supports with the planned ones, braces included, as one
   undoable step. A plan made for an earlier plate is out of date and cannot be applied.

**Consequences:**
- The support document's `origin` gains `automatic`. "Delete the automatic supports" removes
  them and the branches and braces on them.
- The tool's values start from Settings › Supports › Automatic supports each time it opens, and
  its user preset is kept by name, as the Islands tool's is.
- **The contact's shape** (owner, 2026-11-02): beside the preset, the Islands and Automatic
  tools choose Cone or Sphere for their supports' contacts, whatever the preset holds (Cone by
  default, with its default in Settings › Supports); the ball is the preset's. On HillGiant, the
  Automatic tool plans the same 701 supports with balls, built without a finding in 0.10 to
  0.14 s.
- **Measured** on the owner's `HillGiant_32mm.stl` (500,510 triangles, lifted 5 mm, Linux
  release, 16 threads, default values, search included):

  | Model | Cover | Supports | Braces | Overhang points covered | Places left | Time |
  | --- | --- | --- | --- | --- | --- | --- |
  | Solid | Islands and overhangs | 768 (539 branches and merged trunks) | 135 | 6,657 of 6,909 | 27 | 2.7 s |
  | Hollowed at 2 mm | Islands and overhangs | 990 (690) | 130 | 9,203 of 9,324 | 29 | 3.7 s |

  Built as a slice builds them, the applied supports have no finding at all: none left out,
  none steeper than 45°, no error. Before the final check, seven tips pierced a thin part and
  two supports collided; these tips came from the Islands tool's rules as well. Nearly every
  place left is "no way down clear of the model".
- **Revised on 2026-10-30, after the owner's trial** (long diagonals hanging across the model):
  - *A merged trunk leans at most the branch distance aside* (owner: option a; the default
    branch distance without branches). Second-level merges had joined main trunks up to 14 mm
    apart. The rule holds for the Islands tool and the Supports tool's Merge trunks, but not for
    Join a support, where the user picks the shaft.
  - *A merged trunk's shaft ends at its bend.* A bend above a routed knee used to be added after
    it, so the shaft went down to the knee and back up. The bends below the new one are now
    dropped. This was also wrong in Merge trunks and Join a support.
  - *A junction stays a step below its bend.* Two shafts routed aside to the same column made
    them coincide: a degenerate path, and its branches left without a trunk.
  - *Routes plan their slanted parts at 44.5° at most* (`find_route`), and a foot on the model
    only within that slant from the tip's join. Knees planned at exactly 45° read as steeper
    after rounding, and feet were checked only for being lower. On `HillGiant_32mm.stl` the steep
    shafts went from 27 to none.
  - *A raft is checked against drain holes by its bounds first* (core `build_supports`). Each
    part is proven clear of a drain hole through a capsule around it. For a raft, that capsule is
    a ball as wide as the raft. Every hole in a lifted model's bottom was reported as "crossing"
    the raft, and the raft drew red (owner's trial). A part whose box lies apart from the hole's
    is clear; otherwise the capsule decides, as before.
- **The Islands tool, revised the same day** (owner's trial: "Support it here" on a foot's
  sole put 14 supports over the whole sole, some red because a drain hole cut them):
  - *One support where each island starts* (owner, after a second trial: "a single support,
    always where the island starts").
    - **An island is where one piece starts in mid-air.** The core analysis now keeps, in each
      region, every piece that starts in mid-air (`IslandStart`: layer, centroid, first-layer
      bounds), because several may grow into one region. Each start is an island of its own, so
      the Islands tool shows them all. On `HillGiant_32mm.stl` turned 80° and hollowed, the
      islands go from 202 regions to 305 starts, and 434 of the 501 local minima are at a start
      (354 before).
    - **One support, where it starts:** under the centroid of its first layer. If that is
      refused (no way down, or the final check), the next point of its first layer at least
      2 mm from every refused one is tried, three at most. "Large islands: one every" is gone.
      Islands within the join distance share a support; an island of a chain that support does
      not reach gets its own.
    - **Held only where it starts:** a tip on its first layer's footprint, widened by 0.6 mm and
      the join distance, within 1 mm above it. A support beside it no longer turns its marker
      green.
    - **Measured** on `HillGiant_32mm.stl` (lifted 5 mm): 568 islands. "Islands only" holds 546
      of them with 285 supports in 1.7 s; "Islands and overhangs" holds 557 with 410 supports in
      3.4 s. Every island left has no way down clear of the model, and the supports build
      without a finding. "Support it here" on the turned model's first island puts one support,
      exactly where it starts.
  - *Too close to the plate is said with numbers* (owner, after a third trial with the model
    lifted 3 mm). A support needs room under its tip for the tip, the base's cone and the base
    (`room_needed`): 3.2 mm with Light, 4.0 with Medium, 4.8 with Heavy. An island starting
    lower is left as "too close to the plate" (`Skip::TooLow`), and Support it here says how
    many millimetres it has, how many the preset needs and how much more to lift the model.
    Supports are not shortened to fit: the owner chose the message.
  - *The islands list filters by place:* Everywhere, Outside or In the cavity. An island on the
    cavity's ceiling cures in the vat as one outside does, and needs its support as much.
  - *The final check is shared* (`islands_ui::plan::check`). Support the islands and Support it
    here build their supports as a slice would and drop those with errors, a drain hole cutting
    them included (`Skip::DrainHole`). The notification says why an island got no support.
  - *Where on a hollowed model* (owner, 2026-10-30): "Everywhere", "Outside" or "Cavity only"
    replaces the "Inside the cavity" switch (`Place`). Islands and overhang candidates elsewhere
    are left out, and Generate replaces only the automatic supports of the place covered: a
    support's place is the surface its tip touches, probed straight up within 1 mm (none
    counts as outside). Saved settings with "inside" read as Everywhere (on) or Outside (off).
    On a model that is not hollowed the choice is off and the plan covers everywhere.
  - *Names on the strip* (owner, 2026-10-30): the Supports tool is now named Manual, and it
    swapped icons with the Automatic tool: the plain support for Automatic, the support with a
    plus for Manual.
  - *Islands no longer share a support by default* (owner trials, 2026-10-30). "Join islands
    closer than" defaults to 0 mm in the Islands and Automatic tools and moves under Advanced:
    the owner's best results came with every island held where it starts. Settings schema 2
    rewrites a saved 1 mm, the old default, to 0 once; any other saved value is kept.
- **Where the code lives.** The planning stays in `karasu-app`, beside Support the islands' and
  the trees', which it shares. Moving these plans into the headless core is due when the command
  line gets automatic supports.
- Physical printing stays out of scope: nothing here claims a support holds.

## TD-113 — Mini supports

**Status:** Accepted on 2026-10-30 (owner's proposal, brought before M6.4j); implemented the
same day.

**Context:**
- The Islands and Automatic tools left places unsupported where a support finds too little room
  under its tip (a tip, a cone and a base, or a foot, do not fit) or no way down clear of the
  model. On the owner's `HillGiant_32mm.stl`, UVtools 2.2.0 (`print-issues -i`, used as a
  black box) still found 74 islands in an Automatic export.
- Lychee Slicer offers "mini supports" (owner's screenshot, used as a behavioural reference
  only): a thin rod with rounded ends, without a tip cone or a foot, standing on the model and
  able to branch. Karasu's own design follows; nothing was taken from Lychee's implementation.
- The owner also observed that Lychee's default supports have no penetration, and asked that a
  mini support can have none from the start.

**Decision:**
1. **A kind of support, not a preset.** The support document gains `kind`: `standard` (the
   default) or `mini`. A mini support keeps the tip, bends, base, branches, braces, origin and
   collision acceptance of any support, so placing, handles, undo, the list and the move with
   its model work unchanged.
2. **Geometry** (specification, section 4): one diameter `d = 2r` along its whole path, a ball
   at each end centred `r − p` out of the surface along its axis (so it reaches the mini
   penetration `p`), cylinders between, balls at bends. On another support, its lower end is a
   ball at the junction. No tip cone, no foot, no pad.
3. **Values:** `mini_diameter_mm` (0.1–2 mm; presets 0.3, 0.4, 0.5) and `mini_penetration_mm`
   (0–1 mm; presets 0.05, 0.1, 0.15), the penetration less than the radius. Zero is accepted:
   the balls then touch the surface. The presets' values are karasu's choice, not proven print
   settings.
4. **Where it stands:** on the model or on another support. On the plate it is refused: it
   would have no pad. Its foot must lie below its tip: the foot prints first and holds it.
5. **Checks:**
   - **It may touch the model** (owner, 2026-10-30, after his trial: "the error is too strict;
     Lychee is far more permissive"). No part of it keeps the clearance, and touching or
     entering the model is not an error: the resin there is one. Where its axis crosses the
     model, a warning says so, once per support; the support stays as placed, not red.
   - Within `2(r + clearance)` of an end on the model the rod is that end's part (Contact or
     Foot), so Tips only shows and picks it as a tip.
   - Every other check is a support's: anchors, axes, piercing, drain holes, the build volume,
     steep segments.
   - The Braces tool's planned braces leave mini supports out.
6. **Placing (Manual tool):** a "Mini supports" button on the tool strip (owner's choice among
   three), kept with the settings. New supports are then mini supports in any of the three
   placing ways:
   - one click: the foot on the first surface straight below the tip's ball (`find_mini_foot`):
     it must face up and lie within 45° of vertical. Without one the footer says so and nothing
     is added;
   - tip, then foot: the foot follows the pointer on the model only;
   - branch onto a support: the highest junction within 45° of the tip.

   The panel then shows the mini support's two values. A selected mini support has only its tip
   and foot (or junction) handles. A branch that is a mini support is no thicker than its trunk.
7. **Islands and Automatic: a fallback, on by default** ("Mini supports where a support does not
   fit"):
   - Where a standard support is refused while routed alone (no way down, too low, out of
     reach, unbuildable), a mini support is tried on the model straight below the tip.
   - A standard support the final check removes leaves a mini support at its tip.
   - A mini support is never a trunk, never merged, and never braced.

   The Automatic panel says how many mini supports a plan holds.

**Consequences:**
- The CLI reads `kind` and its summary adds "mini N" when there are any.
- **Measured** on `HillGiant_32mm.stl` (500,510 triangles, lifted 5 mm, Linux release, 16 threads,
  Automatic defaults, search included):

  | Model | Cover | Mini supports | Islands held | Places left | Time |
  | --- | --- | --- | --- | --- | --- |
  | Solid | Islands only | off → 30 | 534 → 558 of 568 | 34 → 10 | 2.28 → 2.18 s |
  | Solid | Islands and overhangs | off → 72 | 536 → 559 of 568 | 56 → 14 | 4.31 → 3.35 s |
  | Hollowed at 2 mm | Islands only | off → 34 | 539 → 568 of 579 | 40 → 11 | 2.92 → 2.91 s |
  | Hollowed at 2 mm | Islands and overhangs | off → 81 | 541 → 569 of 579 | 67 → 14 | 5.36 → 3.91 s |

  Built as a slice builds them, the applied supports have no finding. Most places still left
  are tips that pierce a thin part with nothing below to stand on. Fewer closing passes make the
  full plans faster.
- **Revised the same day** (owner's trial: mini supports placed tip then foot were often red,
  "crosses the model"). Probed on `HillGiant_32mm.stl` with feet clicked 1, 2 and 4 mm aside of
  each underside tip, 21 %, 59 % and 78 % of them had an error under the clearance rule; a 0.1 mm
  clearance changed little (59 % to 54 %), since their rods really run along or through the
  model. With mini supports free of the clearance none has an error; 9 to 26 % carry the
  crossing warning. `find_mini_foot` and mini branches no longer ask for clearance either.
  Automatic then leaves 13 places on the solid model (76 mini supports, 3.22 s) and 14 on the
  hollowed one (86, 3.67 s); Islands only, 8 and 9.
- **Limits:**
  - A mini support's foot stands on whatever faces up below. Whether that part is itself
    held at that layer is not checked: an island there gets its own support.
  - It marks the surface it stands on.
  - Physical prints must show whether these diameters and penetrations hold. Nothing here claims
    a mini support prints.

## TD-114 — Suction-cup detection on the layer masks

**Status:** Accepted on 2026-10-31 (owner: the cups belong to the Drain holes tool, and the
smallest vent is a setting, an area). The core analysis and the command line are implemented; the
interface follows from a mockup. Specification: [SUCTION-CUP-SPEC](SUCTION-CUP-SPEC.md). Research:
[suction cups](research/SUCTION-CUP-RESEARCH.md).

**Context:**
- M6.4j (TD-110) detects suction cups: resin a layer encloses while it is peeled from the film,
  capped towards the plate. Automatic orientation (M6.4k) is to score poses with them.
- A cup depends on the pose and on the print as it grows. It differs from TD-101's cavity
  regions, a property of the finished part:
  - an undrained cavity is a cup while it prints;
  - a drained one still is when its holes are on the vat side;
  - an open shape facing the vat is one with no cavity at all.
- UVtools, used as a black box on ten shapes, places its cups on exactly the layers this
  definition gives. Any opening of its pixels vents, and a minimum area and height hide the small
  ones.

**Decision:**
1. **Definition.** At layer `k`, an empty region that does not reach the outside through layers
   `0..=k` is a cup. The plate caps the first layer.
   - A cup spans the layers it is open to the film at.
   - It ends **then open** (a vent, or past the rim) or **closes** over trapped resin.
   - Cups whose empty regions join below are one: their resin is one.
2. **On the exported masks**, with supports and raft: they can cap a cup, and only what the print
   leaves open vents it. A pixel cured at all seals (gray 1).
3. **Minimum vent (owner):** an area in mm², the circle that must fit through an opening.
   - The empty pixels are eroded by its radius less one pixel and joined again.
   - Cups only the narrow openings vent are reported with openings too small.
   - Under a pixel, any whole empty pixel vents.
   - Default 1 mm² on the command line; the interface's default comes with its mockup.
4. **Algorithm:** one sweep from the plate up on the whole empty pixels and one on the eroded
   ones.
   - Runs are labelled per layer, and one union-find node is kept per component, with the
     outside as node 0. Two layers are held at a time.
   - Pieces are recorded when their layer is joined and grouped into cups.
   - The per-layer runs are computed on every core a batch at a time, with the same report for
     any thread count.
5. **Nothing hidden** (TD-107): the core and the command line report every cup. The interface
   filters what it draws by width and height, and counts the rest.
6. **The command line:** `slice --suction-cups [--min-vent <mm2>]` adds the count and the 20
   widest cups to the summary, each with:
   - its layers;
   - its cap in world millimetres, where a vent relieves it;
   - its widest area and its volume;
   - its opening and its end.

   It reports and never fails the job.

**Alternatives rejected:**
- **Any opening vents (UVtools):** the owner rejected it, since a pixel would still be a cup
  physically.
- **The minimum vent as a pixel count or a width:** the owner chose an area, the setting users
  know from drain holes.
- **Splitting a closed cup into a trap and a cup:** one resin, one report, with whether it closes.
- **Thresholds in the core:** they would hide cups from the command line and from M6.4k's
  scoring.

**Consequences:**
- The shared run helpers of the analyses (runs, lattice disc, union-find) moved to
  `analysis::runs`. The unsupported regions' tests pass unchanged.
- `parallel::fill_costly_with` maps a few costly items (layers) on every core.
- On the owner's hollowed `HillGiant_32mm.stl` (774 layers at 13312 × 5120):
  - 1319 cups, 9 of them at least 1 mm² wide and 0.5 mm tall;
  - the cavity is one cup, layers 79–688, 1238.5 mm², and it closes;
  - 0.75 s on 16 cores.
- **Interface (2026-10-31, from the owner's mockup):** the Drain holes tool has:
  - a Suction cups section: the minimum vent from Settings › Hollowing, and a display filter
    (1 mm² and 0.5 mm) kept with the settings;
  - a footer search that slices the model as it will be exported;
  - markers, a navigator that frames each cup at its widest layer, and a list tab;
  - Add a vent here: a new hole straight towards the plate from the cap, or sideways when the
    plate closes that way;
  - parts (owner, 2026-10-31, after the trial on the turned `HillGiant_32mm.stl`, where 7 cups
    became 10 once one was vented). The core reports each part of a cup that starts on its own:
    its cap, its size until it meets an older part, and where a circle of the minimum vent first
    fits in it, above a narrow tip. The panel counts the vents needed and steps through the
    parts, with a vent for one or each. A vent takes the best of straight down, down at 45° and
    level, its hole turned along the way. Venting each part of that cup leaves, after the first
    round, the one part joined only through a narrow passage, which a second search shows;
  - after the second trial (2026-11-02): the shortest way wins (straight down counts three
    quarters of its length), so a crevice of the surface is vented through its own wall rather
    than through the whole model; the hole is turned with the exact normal drainage resolves
    (`surface_hole_outward`, new in the core); the section cuts the layer where the vent goes.
    On the turned HillGiant, the owner's crevice, a second crevice and a closed pocket are each
    vented by one hole;
  - a line in the Slice panel.

  The list window gained tabs.
- **j5, native Windows:** passed on 2026-11-02 (hash-verified snapshot of c528394, 657
  files, `validate.ps1 -IncludeApp`, rustc 1.99.0 MSVC): no failed command. fmt, clippy, build
  and the public and private rustdoc pass. The Debug and Release workspace tests each pass 1,541
  with 0 failed and 14 ignored; Linux runs 1,543, and the two extra are the Unix-only path tests.
  The application's three smoke runs pass:
  - Vulkan on the discrete adapter: 12.1 ms mean frame time;
  - the OpenGL fallback: 12.8 ms;
  - FX-03 in the viewport: 16.5 ms mean, 32.1 ms max.

  The snapshot's integrity check passed after the run.
- **Next:** physical prints, before any claim that the analysis predicts print failures.

## TD-115 — Hollowing blockers

**Status:** Accepted on 2026-11-02 (owner: boxes whose two bases can differ, and spheres; free
placement, centred on the point clicked, in the model frame, applied with Apply hollowing; a
Solid base shortcut). Brought in before M6.4j's j5 and M6.4k.

**Context:**
- Hollowing removes everything deeper than the wall. Some regions must stay solid: a base that
  takes the peel force, a thin limb, a part that is cut or drilled later.
- The sampled hollowing (TD-045) cuts the cavity where the field `d(p) - t` is negative, and
  proves the wall with a 1-Lipschitz bound on each block of its grid.

**Decision:**
1. **Shapes, in the model frame** (`request::Blocker`, an operation of the object's timeline):
   - a frustum: two rectangular bases, parallel and centred on one axis, a height apart (a box
     when they are alike, a truncated pyramid otherwise, a pyramid with a zero upper base),
     turned about the model's fixed X, Y and Z axes;
   - a sphere, by its diameter.
   They follow every move, turn, scale and mirror of the object, as drain holes do.
2. **In the core** (`preparation::HollowBlocker`, `HollowSpec::blockers`, at most 1024): the
   field becomes `min(d(p) - t, s_B(p))`, where `s_B` is the least signed bound of the blockers,
   negative inside one.
   - A sphere's bound is exact. A frustum's is the largest of its six faces' plane distances:
     exact inside and along each face, under the true distance outside near its edges.
   - The blockers are given in the plan's source frame. A world point is taken back by the
     placement's inverse and the bound scaled by its least singular value (closed form, shrunk by
     a millionth), so it stays 1-Lipschitz and the grid's pruning and wall proof are unchanged.
   - A block wholly inside a blocker has no cavity; one with no blocker within reach keeps the
     shortcut it had. Blockers only take cavity away, so the wall stays certified.
   - The plan's canonical record encodes them, so the cavity cache tells hollowings apart.
   - Invalid blockers are refused (positions, sizes, a negative upper base, axes not
     orthonormal). A blocker over the whole cavity leaves nothing to hollow, reported as a model
     too thin to hollow. The exact backend refuses blockers.
3. **In the interface** (`hollowing_ui::blockers`), the Hollow tool:
   - Place blockers (P) puts one centred on the point clicked on the model, unturned in the
     model frame, with the New blockers' values;
   - a click selects one; the move and turn gizmos (M, R) place it along the world's or its own
     axes, and the scale gizmo (S, owner 2026-11-02) sizes it about its centre: a box along its
     own width, depth and height (both bases alike) or uniformly, a sphere uniformly; its fields
     set its centre, turn and sizes;
   - size handles (the mockup's, owner 2026-11-03), dots shown on the selected blocker whatever
     the gizmo: at the middle of each side of a box's two bases, sizing that base alone evenly
     about the axis; at the middle of each base, moving that base alone, the other staying; at
     the ends of a sphere's three axes, setting its diameter. They pass through the gizmo's
     handles as `manipulate::Kind::Size` and drag along their outward direction, in model
     millimetres however the model is scaled;
   - **Solid base up to** adds a box over the footprint, from below the lowest point up to that
     height, square with the world;
   - the blockers are a draft of the model's list, applied with the wall by Apply hollowing as
     one plate edit (Remove hollowing removes them too);
   - each is drawn translucent through the model: teal applied, violet new or changed, magenta
     when selected (owner, 2026-11-02: more visible than the mockup's grey and amber; settings
     schema 3 moves colours still at the first defaults), each colour set in Settings › Colours ›
     Hollowing blockers; one mesh per shape is uploaded and placed by transforms;
   - a list window, and the New blockers' defaults in Settings › Hollowing (design rule 6).

**Alternatives rejected:**
- **Blockers as a mesh Boolean after hollowing:** a second geometry pass, with its own failure
  modes, where one more bound in the field does the same.
- **Blockers in world coordinates:** they would stay behind when the model moves.
- **Exact distances to a frustum:** costlier, and a lower bound keeps every proof valid.
- **Applying each blocker as it is edited:** each would cut the cavity again, seconds on a large
  model.

**Consequences:**
- `HollowSpec` is no longer `Copy`.
- **Measured** on the owner's `HillGiant_32mm.stl` (500510 triangles), hollowed at 2 mm with an
  automatic cell, release build, 16 cores: 1.47 s before and 1.49 s after without blockers (the
  same cavity, 20774.6 mm³); 1.50 s with 21 blockers (20 spheres and a turned frustum), whose
  cavity is 20099.9 mm³.
- The applied blockers change the cavity: drainage and suction cups follow from it. A blocker may
  split the cavity, and each part then needs its own hole.
- **Native Windows:** validated with the suction cups on 2026-11-02 (c528394, TD-114).
- **Next:** the owner's trial.

## TD-116 — Contacts fitted to the surface

**Status:** Accepted on 2026-11-02 (owner: the contact only narrows, never widens; the largest is
the preset's contact and the smallest is the user's; an option to switch on; done at once, before
M6.4k). For the Supports, Islands and Automatic tools alike.

**Context:**
- Every support had the fixed contact of its preset or its own values. A preset's 0.3 to 0.6 mm
  contact hangs over the edge of a thin feature (a finger, a strand, a cloth's edge) and goes
  through a wall thinner than its penetration, where the build refuses it (piercing) or the
  automatic tools leave the place unsupported.
- The opposite, a wider contact on wide flat undersides, was declined by the owner: it holds
  better but marks more. Sizing a contact by the load it carries is the later "weight by load"
  phase (TD-112).

**Decision:**
1. **A parameter of the support** (`contact_fit_min_mm`, absent by default, 0.05 to 5 mm, at most
   the contact, soft range from 0.2 mm). With it, the core builds the first contact from the
   support's own down by 0.05 mm steps to this value whose footprint has room, the ball and the
   penetration narrowing by the same factor ([specification](SUPPORTS-SPEC.md), section 4).
2. **Room** is the surface's, in the core at each build (`supports::fit`):
   - measured along the area-weighted mean normal of the triangles within the widest footprint
     that face the anchor's way, so a tip on a vertex of a tessellated curve sees the curve;
   - the point half the penetration deep (at least `2τ`) is inside the model, the column below it
     to the penetration crosses no surface, and 16 spokes of the footprint's radius at that depth
     and at the penetration cross none: exact orientation predicates, on the support index's
     grid;
   - so an edge, a thin fin, a border or a thin wall narrow the contact, and a concave corner does
     not.
3. **No room at the smallest:** the smallest is built, with a `ContactFit` warning on the contact.
4. **The built diameter** is reported with each built support (`BuiltSupport::contact_diameter_mm`)
   and shown in the selected support's values ("Contact as built").
5. **In the interface,** a switch "Fit contact to the surface" and, while on, "Smallest contact"
   (0.2 mm when switched on):
   - in the Supports tool's values, for the next support and the selected one; it is the tool's
     choice, kept whatever preset is chosen, and a preset is still recognised with it;
   - in the Islands and Automatic tools' values, applied to their preset;
   - each with its default in Settings › Supports, off by default.

**Alternatives rejected:**
- **Fitting in the interface when a support is placed:** the stored diameter would go stale as
  the support or the model's hollowing changed, and the command line would not have it.
- **Measuring along the tip's axis or the anchor facet's normal:** a leaning tip, or a tip on a
  vertex, would narrow on a flat or gently curved surface.
- **Narrowing the contact alone:** a small contact with the large penetration would still pierce
  thin walls.

**Consequences:**
- `SupportParameters` and `BuiltSupport` gain a field; the document format gains an optional
  parameter, so version 1 documents read as before.
- **Measured** on the owner's `HillGiant_32mm.stl` (500,510 triangles, lifted 5 mm, Linux release,
  Light contacts of 0.3 mm), the Automatic tool covering everything:
  - building the 711 planned supports takes 0.13 to 0.15 s with fitting down to 0.2 mm, against
    0.11 to 0.13 s without; planning takes 3.0 s with the search, as before;
  - 219 contacts narrow and 67 have no room even at 0.2 mm (mostly sharp points facing down,
    which the contact encloses); the Islands tool, fitting, supports every one of the 568 islands
    instead of 560.
- Fitting is geometric: it says nothing about whether the narrowed contact holds in print. A
  physical print is needed before any claim that it does.
- **Native Windows:** validated on 2026-11-03 with f78352a (`validate.ps1 -IncludeApp`, a clean
  snapshot of 658 files): no failed command; Debug and Release each pass 1,551 tests with 14
  ignored; the three application smoke runs pass (Vulkan 11.7 ms, OpenGL 13.3 ms, FX-03 16.4 ms
  mean).

## TD-117 — PP1 files that Photon Workshop and the P1 accept

**Status:** Accepted on 2026-11-03. The owner authorized writing the P1's firmware constants taken
from Anycubic's files. The fix awaits the owner's check in Photon Workshop and a print on the P1.

**Context:**
- The P1 refused karasu's `hillgiant_test_v4.pp1` after its "MD5 integrity check" and went back
  to the main screen. Photon Workshop 4.2 refused the same file, and every variant of it, with
  "File Corruption". It opened files by Lychee 8.0.0 and CHITUBOX 1.2.3 and Anycubic's own.
- No reader stores an MD5 in the file. The check rejects missing metadata.

**Evidence** (owner's tests in Photon Workshop, 2026-11-03; files kept outside Git):
- Hybrids of a Lychee file that opens, with one member from karasu's file:
  - only the karasu `anycubic_photon_resins.pwsp` was refused;
  - previews, `print_info.json`, `software_info.conf`, `lcd_function.json`, layer images and a
    stored (uncompressed) ZIP were accepted;
  - karasu's `raster_antialiasing` of 1 was accepted.
- Variants of the official `TEST.pp1` (SHA-256 `d48e98b7dca6116d034ecba6a64e64f7bc7f9e92758b64798b54be7b80a7c83a`, Photon Workshop
  4.1.0), each without one field karasu lacked. Refused without:
  - the preview colours of `machine_type`;
  - `max_acceleration` in the resin's `slice_extpara`;
  - a complete resin `property`;
  - `firmware_calc_print_time_paras` and `firmware_calc_exp_time_paras`, even with
    `firmware_calc_print_time` set to 0;
  - `picture`.
- Accepted:
  - a single `child_screen`;
  - one platform height;
  - no depth curve and no factory resins;
  - an empty `picture`;
  - preview colours of our own;
  - a resin with zero price and density;
  - `firmware_calc_print_time` 0.
- The firmware constants and `max_acceleration` (2) are identical in Anycubic's sample, a Photon
  Workshop 4.2 export, Lychee 8.0.0 and CHITUBOX 1.2.3 (up to float formatting).
- Photon Workshop's P1 profile allows only the image format `pwszImg`; all three readers name
  their layer members `.pw0Img` all the same.

**Decision:** the PP1 writer records:
1. `key_image_format` `pwszImg`;
2. `max_file_version` 518, `property` 119 and `raster_segments_capacity` 100000;
3. preview colours and the cloud preview size of its own;
4. an empty `picture`;
5. the P1's firmware constants, and `max_acceleration` 2 in the resin;
6. a complete resin `property`:
   - named `karasu@<resin>@NFEP@karasu`;
   - the resin profile's density;
   - zero price and volume;
   - the 25 °C target every reader writes.

The previews are opaque RGBA, like every reader's own. The rest is unchanged: binary
`raster_antialiasing` 1, `lcd_function.json`, a stored ZIP.

**Alternatives rejected:**
- **Copying a whole Anycubic or Lychee resin file:** only the machine's constants are needed;
  resin names, prices and the factory resin list are not.
- **Omitting the constants with `firmware_calc_print_time` 0:** Photon Workshop still refuses
  the file.

**Consequences:**
- PP1 files carry machine constants from Anycubic's files (owner's authorization, recorded here).
  If a firmware update changes them, they must be read again from a new official sample.
- Photon Workshop opening a file is evidence of format acceptance, not of print safety. The P1
  print stays pending.
- **Owner's check, 2026-11-03:** Photon Workshop opens `hillgiant_test_v4_fixed.pp1`. That file is
  the refused v4 with only its `anycubic_photon_resins.pwsp` rewritten as this writer now writes it
  and its previews made RGBA; its layers, controller and scene are byte for byte the v4's.
  `3dBenchy_no_support.pp1`, exported by the fixed writer itself (9c3c406), opens as well, with
  the 28 °C its print setting states in the resin's `target_temperature`. Photon Workshop shows no
  heating target for any file; the P1 shows it.
- **On the P1, 2026-11-03:** the same file passes the printer's integrity check and prints. The
  P1 shows a heating target of 25 °C, the resin's `target_temperature`: it showed 20 °C for the
  refused file, which had none. Its own estimate is 4 h 11 min 47 s, against the file's 3 h 41 min
  and UVtools' 5 h 45 min, with the printer's defaults (smart lift, smart light-off, error
  detection, base compensation).
- **The print's outcome (owner, 2026-11-03):** the file printed to the end. A power cut stopped
  it near layer 387 and the P1 resumed it; at that height (19.35 mm above the plate, 0.05 mm
  layers) only the two hollow legs and the club's head were being cured, and the defects the
  owner found on the feet are most likely from the resumption. Otherwise:
  - the fine detail printed (face, beard, ropes, chains, the wheel on the back, spear tips);
  - the cavity drained through its holes;
  - the light supports held; their shafts came off cleanly, some tips stayed on the model with
    the usual marks (the owner will try a penetration of 0 and the medium preset); the mini
    supports worked;
  - nothing was noticed at layer 708, where a small unsupported island was reported;
  - some cured residue may have stayed in the vat, which the power cut may explain.
  The P1 reported 5 h 07 min 47 s in all. The time includes the power cut and the resumption, of
  unknown length, and perhaps the smart light-off: it cannot check an estimate. A print without
  interruption is needed for that.
- **Native Windows:** validated on 2026-11-03 with f78352a (`validate.ps1 -IncludeApp`, a clean
  snapshot of 658 files): no failed command; Debug and Release each pass 1,551 tests with 14
  ignored; the three application smoke runs pass (Vulkan 11.7 ms, OpenGL 13.3 ms, FX-03 16.4 ms
  mean).

## TD-118 — A resin's colour and a print setting's heating temperature

**Status:** Accepted on 2026-11-03 (owner): the colour on the resin, the heating temperature on
the print setting, and the model not drawn in the resin's colour.

**Context:**
- A resin's colour changes its exposure. Resins of one kind in different colours need their own
  print settings; without a colour, the user tells them apart by name alone, or edits the
  settings each time.
- The P1 heats its vat to the resin's `target_temperature` in the file. It showed 20 °C for a
  file that stated none, and 25 °C, the value every reader writes, once karasu wrote it
  (TD-117).

**Decision:**
1. **Colour on the resin** (`ResinMaterial`):
   - `colour`, the maker's name for it ("Grey"), and `swatch`, `#RRGGBB`, both optional;
   - each colour of a kind is its own resin, with its own print settings;
   - the swatch is drawn beside the resin's name in the sidebar's resin menu and the library;
     the colour is added to the library row's facts.
2. **Heating temperature on the print setting** (`PrintSettings::heating_temperature_c`):
   - optional, 10 to 60 °C; a new setting starts at 25 °C;
   - a setting that states none, such as a catalog setting, heats to 25 °C;
   - the PP1 writer records it as the resin's `target_temperature`.
3. **Compatibility:** both fields are written only when set, so the library keeps schema
   version 2. Files saved before them read unchanged and keep their bytes; older builds refuse
   a profile that sets them.
4. **Not done** (owner): the model is not drawn in the resin's colour. Markers, gizmos and
   overlays have fixed colours, which a model of any colour could clash with.

**Consequences:**
- The other formats do not record a heating temperature yet. They gain it when one is
  confirmed to carry it.
- **Native Windows:** validated on 2026-11-03 with f78352a (`validate.ps1 -IncludeApp`, a clean
  snapshot of 658 files): no failed command; Debug and Release each pass 1,551 tests with 14
  ignored; the three application smoke runs pass (Vulkan 11.7 ms, OpenGL 13.3 ms, FX-03 16.4 ms
  mean).

## TD-119 — Automatic orientation: tilts proposed under the user's constraints (M6.4k)

**Status:** Accepted on 2026-11-03. The owner chose all three constraints, a list of
candidates in the panel, the tilt limit on at 30° by default, and a confirmation before
supports are removed (mockup "Automatic orientation in the Orient tool").

**Context:**
- An orientation decides where supports go. Users tilt a figure so its face looks up and away
  from the plate, so the supports land on its back.
- An algorithm cannot know which surfaces matter. The user must be able to say so, and to
  choose among good poses rather than take one.
- TD-110 placed M6.4k last because it scores a pose with what M6.4i and M6.4j built: the
  unsupported regions (TD-107) and the suction cups (TD-114). TD-110 kept painted protected
  surfaces after the MVP; protected points are their simple form.

**Decision:**
1. **Rotation only, and only a tilt.**
   - What faces the plate is a direction of the model, two degrees of freedom. A turn about the
     vertical axis changes neither supports nor cups, so the search tilts the model about a
     horizontal axis and keeps the user's turn about the vertical one.
   - The model turns about its centre and rests where it rested, at its lift; its XY place
     stays.
2. **Constraints** (all optional, combined):
   - **Tilt about:** any horizontal axis, the plate's X only, or its Y only.
   - **At most from now:** the tilt within so many degrees of the pose now; on at 30° by default,
     so the user orients roughly and the search refines.
   - **Protected points:** clicked on the model and kept in its frame. A tilt is refused when a
     protected point's surface would face the plate past the critical angle. Surface facing the
     plate and lowest points within a distance of a point (5 mm by default) weigh heavily in the
     score.
3. **Two passes** (core `analysis::orientation`):
   - **Rough:** each allowed tilt is scored from the prepared mesh, the hollowed shell when there
     is one:
     - the area facing the plate past the critical angle, faces resting on the plate left out;
     - the lowest points not on the plate, where islands start, counted only where the faces
       around them face down;
     - the height;
     - near protected points, both again.
     The tilts are sampled evenly: about 5° apart on the cap the limit allows for "any", 1°
     apart along the arc for one axis. They are scored on every core.
   - **Precise:** the best few distinct tilts (at least 8° apart), the pose now among them, are
     checked as a slice would:
     - the model alone is sliced in that pose, as the Islands tool's search does, for its
       islands and, hollowed, its suction cups (those the Drain holes tool shows);
     - the largest layer's area comes from sections of the mesh every 0.5 mm.
4. **Score:** each measure is divided by its largest among the candidates and weighed; the
   lowest sum ranks first.
   - Weights: surface facing the plate, islands, suction cups, height, largest layer, and
     protected surfaces.
   - Priority presets: Fewer supports, Balanced, Faster. Advanced shows the weights.
5. **The Orient tool** in the Prepare step, after Transform:
   - constraints, priority, then up to five candidates with their measures;
   - hovering a candidate previews the model in that pose, overhangs tinted; Apply turns it to
     the chosen one as one undoable step;
   - a model with supports asks first: they are removed with the turn, since they were placed
     for the pose before;
   - the search runs on its own thread, with progress and Stop.

**Alternatives rejected:**
- **Locking rotation axes alone:** an indirect way to say "not on the face"; a model already
  turned makes the right axis unclear.
- **Applying the best pose directly:** the measures conflict (fewer supports or fewer layers),
  and the choice is the user's.
- **Slicing every tilt:** hundreds of slices; the rough pass ranks them in a fraction of a
  second.

**Consequences:**
- Protected points live with the tool while it works; with the project file (M6.6) they are
  kept with the model.
- **Measured** on the owner's `HillGiant_32mm.stl` (500,510 triangles, lifted 5 mm, Linux release,
  16 threads, critical angle 45°, Balanced):
  - rough pass alone (core): 112 tilts within 30° in 0.12 s, 1,652 tilts unlimited in 1.2 s,
    360 about one axis in 0.29 s; the sections of a candidate in about 3 ms;
  - the whole search, six candidates sliced: 4.6 s within 30° and 5.6 s unlimited for the solid
    model; 14.6 s and 17.2 s hollowed at 2 mm, its cavity and suction cups checked in each pose;
  - within 30° the best candidates sit near the limit (the figure needs much support upright);
    unlimited, they lie on its back or side, near 90° to 110°, with about half the surface
    facing the plate and the islands.
- While a candidate is previewed, the model's own drawing turns; its drain holes and cavity
  overlays keep the pose now until Apply.
- An orientation is geometric advice; it makes no claim that a pose prints better without
  physical evidence.
- **Native Windows:** validated on 2026-11-03 with 3a6c39d (`validate.ps1 -IncludeApp`, a clean
  snapshot of 667 files): no failed command; Debug and Release each pass 1,565 tests with 16
  ignored; the three application smoke runs pass (Vulkan 12.8 ms, OpenGL 14.0 ms, FX-03 16.5 ms
  mean).

## TD-120 — Drain holes turned about one axis at a time

**Status:** Implemented on 2026-11-03, at the owner's request: the free tilt handle (TD-083) is
not precise enough to set a hole's lean.

**Decision:**
- The selected drain hole shows a turn gizmo: three rings centred at the foot of its axis on the
  surface, a fixed size on screen, drawn and picked as the objects' rings
  (`manipulate::rings`). A ring turns the hole's axis about its own axis only, by the pointer's
  angle about the foot; Shift snaps to 15°.
- **Axes:** the tool strip's axes switch (the transform's Global/Local, shared by every gizmo)
  chooses the world's X, Y and Z, or the surface's own: the hole's two reference tangents and
  the surface normal. About a tangent the hole leans across the surface by exactly the angle
  turned; about the normal its lean keeps its angle and changes direction.
- The turn is measured from the axis the hole had when the drag began, so the result depends
  only on the pointer. The new axis becomes the hole's tilt (`axis_tilt`), carried into the
  object's model frame as the tilt handle's is. A lean past the limit (60°) stops at it, still
  facing the way the ring turned it.
- The rim, end and tilt handles stay, and are taken first where they overlap a ring; the hole's
  solid is taken last. The panel's tilt and direction fields remain the way to type values.

**Alternatives rejected:**
- **Rings about the hole's own axis:** turning a cylinder about its axis changes nothing, and
  its two other axes would turn with every drag.
- **Replacing the tilt handle:** a free lean is still the quickest when no exact angle matters.

**Validation:** a unit test turns a hole on a side face about world Z (the axis stays in the
horizontal plane), with Shift (50° snaps to 45°), past the limit (stops at 60°), and about the
surface normal (an upright hole stays upright).
- **Native Windows:** validated on 2026-11-03 with 3a6c39d (`validate.ps1 -IncludeApp`, a clean
  snapshot of 667 files): no failed command; Debug and Release each pass 1,565 tests with 16
  ignored; the three application smoke runs pass (Vulkan 12.8 ms, OpenGL 14.0 ms, FX-03 16.5 ms
  mean).

## TD-121 — A drain hole's diameter does not turn it

**Status:** Implemented on 2026-11-03, after the owner noticed that resizing an applied hole
also turned it.

**Context:** a surface hole's tilt is measured from the mean normal of the faces within its
radius of its foot (TD-066), so on a sculpted surface a new diameter moves the normal, and the
axis with it, even for a hole without a tilt. Averaging over the radius is right when a hole is
placed: one stray facet does not set its direction. It is wrong when only the size is edited,
the more so since holes are turned precisely (TD-120).

**Decision:**
- When the diameter of a hole changes in the interface (its rim handle, or the panel's field),
  its tilt is recomputed from the normal at the new radius so that its axis stays the one it
  had, in the world and so in the model frame. A lean the new normal would push past 60° stops
  at the limit, facing the same way.
- The normal comes from the object's hole probe, as the tilt's carrying into the world does:
  `DrainHole::outward` resolves a short upright hole, and `with_world_tilt` now uses it too.
- The project format and the core's resolution are unchanged: only what the interface writes
  into the tilt changes. Moving a hole along the surface still keeps its lean from the surface.

**Alternatives rejected:** a normal over a fixed radius, independent of the diameter: it would
change how existing projects' holes resolve and where new ones point.

**Validation:**
- On a 20 mm cube, a hole 2 mm from an edge: widened from 3 to 6 mm as it is, its axis turns;
  widened keeping its axis, its new tilt brings it back onto the old axis within 1e-9.
- **Measured** on `HillGiant_32mm.stl` hollowed at 2 mm (Linux release): the normal takes
  0.007 ms for a 2 mm hole, 0.026 ms for 5 mm and 0.13 ms for 10 mm, beside about 1 ms for the
  verdict each step of a rim drag already makes.
- **Native Windows:** validated on 2026-11-03 with 3a6c39d (`validate.ps1 -IncludeApp`, a clean
  snapshot of 667 files): no failed command; Debug and Release each pass 1,565 tests with 16
  ignored; the three application smoke runs pass (Vulkan 12.8 ms, OpenGL 14.0 ms, FX-03 16.5 ms
  mean).

## TD-122 — A printer's fixed time per layer in the time estimate

**Status:** Accepted on 2026-11-03 (owner, after an analysis of the rests, to be made after the
HillGiant print); implemented the same day.

**Context:**
- karasu estimates a print as the sum, over its layers, of the exposure, the rests and the time
  of each lift and retract stage at its speed. That is additive, as Photon Workshop and CHITUBOX
  are for the P1.
- Photon Workshop's own P1 estimates match the same sum plus 0.45 s per layer within 0.2 % (on
  Anycubic's `Base_01` sample files). The P1's firmware constants, which PP1 files carry
  (TD-117), state it: `MACHINE_GENERATE_FRAME_TIME` is 450 ms. The printer spends it on every
  layer, preparing the next image.
- The owner pointed out that it is the P1's, not every printer's.

**Decision:**
- Printer profiles gain an optional `fixed_layer_time_s`, 0 to 10 s, written only when set (the
  library schema stays 2). The time estimate adds it once per layer. Formats that record the
  estimate (PP1, CTB, Photon Workshop, the machine formats) record it with that time.
- The catalog's Photon P1 takes 0.45 s, as revision 2: a copy of revision 1 is offered the
  update, as every catalog change (TD-106). Other printers state none until one is measured.
- The printer editor has the field under **Timing** ("Fixed time per layer"), in seconds.

**Consequences:**
- The HillGiant file the P1 printed on 2026-11-03 (1,612 layers) is estimated 12 min longer:
  3 h 53 min instead of the 3 h 41 min it records (13,265 s), against the P1's own 4 h 11 min
  47 s. The P1's smart lift and smart
  light-off, which its defaults turn on, may lengthen a print beyond any such estimate.
- The print's actual time (5 h 07 min 47 s) cannot check it: a power cut interrupted it (TD-117).
  A print without interruption, with the smart functions noted, is needed.

**Validation:** a core test adds 7 × 0.45 s to a seven-layer job and refuses values outside the
range; a library test offers revision 2 of the P1 to a copy of revision 1, with only the new
field changed.
- **Native Windows:** validated on 2026-11-04 with b179c8c (`validate.ps1 -IncludeApp`, a clean
  snapshot of 668 files): no failed command; Debug and Release each pass 1,579 tests with 16
  ignored; the three application smoke runs pass (Vulkan 11.8 ms, OpenGL 12.8 ms, FX-03 16.5 ms
  mean).

## TD-123 — Drain holes cut only the model

**Status:** Accepted on 2026-11-03 (owner): "drain holes should cut only the model, not the
supports; what matters is that the hole stays free to drain."

**Context:**
- Slicing joined the model and its supports into one mesh, sectioned it, and removed each drain
  hole's cylinder from every layer (TD-089). A support crossing the cylinder anywhere was cut,
  so any support part meeting it was an error ("crosses a drain hole, which would cut it").
- The cylinder starts 1 mm outside the surface, the entry margin a surface hole needs, and a
  hole through a wall into a cavity ends 1 mm past the wall. A hole also makes new overhangs at
  its own rim: the owner placed a support on an island the hole had made, and it was refused
  because its top crossed the margin under the surface, where the hole ghost showed it.

**Decision:**
1. **Slicing:** a drain hole removes the model's material only. The faces of the supports are
   known in the sliced mesh (the supports follow the model's faces in each object, and objects
   follow one another on a plate), and every winding layer lists their segments apart. A sample
   is material when it is inside the layer and outside the cut, or inside a support: the cut
   loses the supports' spans before it is subtracted from the layer's. It stays exact per
   sample, as the cut was. Strict layers cannot tell supports apart and are refused with kept
   faces; supported models slice as winding layers anyway.
2. **The check:** a support part is an error only when it enters a hole's *channel*: the
   cylinder over each stretch of the hole's axis inside the material, from where the axis
   enters the surface to where it leaves it, or to the hole's end. Crossings come from the
   outward surface's orientation, so a cavity the hole crosses, the entry margin and the margin
   past a cavity wall are no channel. Material there would plug the hole; elsewhere it plugs
   nothing. Braces follow the same rule.
3. **Tapered parts:** a part is tested by its axis in eight pieces, each as wide as the part at
   that piece's wider end, instead of one capsule as wide as its widest end. A cone's thin
   contact, touching the surface at a hole's rim, is tested as thin.
4. **Words:** the finding is "enters drain hole k through the model, which it would plug"; the
   lists say "Plugs a drain hole". (TD-131 made it a warning with new words.)

**Alternatives rejected:**
- **Keep cutting the supports and shorten the cylinder:** a cavity hole's inner margin and any
  stretch outside a sculpted surface would still cut a support, and the rule would stay unclear.
- **A warning when only a tip's penetration enters the channel:** not needed so far; the
  tapered test already keeps a tip on the rim clear. To be revisited with real cases.

**Consequences:**
- A support standing under a hole's opening, or crossing it outside the model, prints whole.
- The masks of models without supports, or whose supports cross no hole, are byte for byte as
  before.
- The wall verification (TD-095) fills the holes in again, and a support in a hole's margin is
  material either way: it changes nothing there.
- **Measured** on `HillGiant_32mm.stl` (500,510 triangles) hollowed at 2 mm, with two surface
  holes and 371 vertical supports, sliced to CTB on a 3840 × 2400 panel (Linux release, 16
  threads), three runs each: 2.25 to 2.33 s before, 2.28 to 2.34 s after, CPU time 2 to 3 %
  higher, the same memory, and the same file byte for byte. With a 372nd support whose tip
  crosses a hole's entry margin under the surface, the earlier build refuses the job ("crosses
  drain hole 1, which would cut it"); this one slices it whole.

**Validation:**
- A winding layer lists the segments of kept faces apart (core unit test).
- A strip cut across a model and an overlapping support box, with 1 and 4 threads: the model is
  cut, the support's material stays, nothing changes outside the strip or without a cut; strict
  layers with kept faces are refused.
- Supports: a support on a hole's rim, its shaft through the margin, is clear; one whose tip
  reaches into the hole plugs it; a shaft crossing a hole's axis inside the cavity is clear, a
  tip on the cavity ceiling over the hole plugs the wall's channel; holes beside the model plug
  nothing; braces under a hole through a slab are kept as without it.

**Open decision (owner, 2026-11-03), settled by TD-131 on 2026-11-05:** a support entering a
channel is a warning, no longer an error, so the automatic tools can hold the islands a hole
makes.
- **Native Windows:** validated on 2026-11-04 with b179c8c (`validate.ps1 -IncludeApp`, a clean
  snapshot of 668 files): no failed command; Debug and Release each pass 1,579 tests with 16
  ignored; the three application smoke runs pass (Vulkan 11.8 ms, OpenGL 12.8 ms, FX-03 16.5 ms
  mean).

## TD-124 — A support's tip keeps off the model beside its contact

**Status:** Accepted on 2026-11-03 (owner chose corrections 1, 2 and 3 of the analysis), after
the owner found automatic supports crossing the model without an error.

**Context:**
- A contact's cone, from the contact to its join with the shaft (2 to 3 mm), touches the model
  by design, so the build exempted it from the clearance check (TD-062), and the search for a
  route (TD-108) started at the join. On a crease, the cone could run into the surface beside
  its contact, and nothing said so: only the axis limit and the penetration segment were
  checked at a tip.
- A tip that follows the normal, and the axis limit, took the normal of the single facet under
  the contact. On a sculpted surface one facet can point anywhere, often at the neighbouring
  bulge in a crease.

**Decision:**
1. **The reach:** a contact's or model foot's cone must not enter the model away from where it
   touches. From where its side rises twice the anchor tolerance above the contact's tangent
   plane to its join, the cone must meet no surface triangle, by the separating-axis proof the
   clearance uses, with no clearance. Within three footprint radii of the contact, the cone is
   narrowed by the contact's penetration first: the bumps of a sculpted surface around a contact,
   no deeper than the contact's own intrusion, are not counted. Farther, the whole cone counts:
   a fold it grazes there is another part of the model (owner, 2026-11-04: "it was enough to tilt
   it slightly"). Otherwise the contact part has a collision error, which a support's or the
   job's collision acceptance can accept like any other.
2. **Routes and branches:** every way a route finds (straight, aside, on the model) also needs
   the tip's reach clear, and so does every junction a branch is offered on its trunk. A tip
   that would cross the surface beside it leans aside, or joins its trunk lower, which leans it
   another way; with no way, the support is built straight and its build reports it, so the
   Islands and Automatic tools leave the place with the reason.
3. **The normal:** at a contact, the area-weighted mean of the normals of the triangles within
   the contact's footprint radius that face the anchor triangle's way, as fitted contacts measure
   it (TD-116); at a model foot, over the foot's contact. A tip that follows the normal follows
   it, and the axis limit is measured from it. The interface plans branches, trunks and previews
   with the same normal, so a plan has the shape the build gives it.
- `TipCone` describes a tip once for the build and the route search, so the two cannot differ.

**Alternatives rejected:**
- **Lowering the axis limit:** fewer cases, not none: a tip along the normal in a narrow crease
  still meets the other side.
- **The whole cone against the surface:** it touches its own contact by design; and without the
  penetration's allowance, 16 of 370 test supports on HillGiant were flagged, 12 by bumps within
  0.6 mm of their contact.
- **The allowance along the whole cone** (the first version, 2026-11-03): it let a cone graze
  a fold away from its contact by up to the penetration. On the owner's model a branch's tip
  visibly crossed a fold with no error.

**Consequences:**
- Supports that follow the normal may turn by a few degrees from before on faceted surfaces, so
  an existing project's supports can change shape slightly, and one in a narrow place can
  newly pierce or collide: its build says so.
- **Measured** on `HillGiant_32mm.stl` (500,510 triangles, lifted 5 mm, Linux release, 16
  threads), the Automatic tool's plan as its test makes it:
  - islands only: 565 of 568 islands supported instead of 560, 3 places left instead of 8;
  - all: 7 places left instead of 13, 6,903 of 6,909 overhang points instead of 6,902, in 2.48 s
    instead of 2.92 s;
  - the built supports have no finding either way; before the interface planned with the same
    normal, the new build found 21 to 28 shafts steeper than 45°, planned on the facet's normal.
- On 370 vertical supports placed by a script under the same model, the reach flags 7, each
  meeting the surface 0.7 to 2.1 mm from its contact, none of the bumps closer; slicing them
  takes the same time within noise (2.33 s against 2.28 to 2.33 s).
- On the owner's `obj_1_quant_1.stl` (1,864,166 triangles; turned -8°, -21.4°, 1.5°; scaled
  200 %; Light), the Automatic tool covering everything: without the check at junctions the
  build refuses 3 branches whose tip crosses a fold, which the first version passed, and 9
  places are left; with it, 6 places are left and no built support has a finding. Planning
  takes 7.1 s either way.

**Validation:** core tests: the reach of a cone along and across the normal, and narrowed; a
shallow intrusion counted only away from the contact; the mean normal of a ridge; a tip leaning into a bulge is a collision of its contact, leaning away
it is not, and a bump shallower than the penetration is not; a tip on a ridge measures its axis
from the mean; a route beside a bulge leans away from it, and goes straight down away from it.
- **Native Windows:** validated on 2026-11-04 with b179c8c (`validate.ps1 -IncludeApp`, a clean
  snapshot of 668 files): no failed command; Debug and Release each pass 1,579 tests with 16
  ignored; the three application smoke runs pass (Vulkan 11.8 ms, OpenGL 12.8 ms, FX-03 16.5 ms
  mean).

## TD-125 — Automatic supports try tilted tips before going aside

**Status:** Accepted on 2026-11-04 (owner): "if we tilt the normal supports we can probably do
without a mini support there". It starts the owner's rule for automatic supports: they must
never enter the model, they are left out when they would, and they must try more ways than
before to find room. Manual supports stay as tolerant as they are.

**Decision:**
- A route (TD-108) tries, after the straight way and before a knee aside, a tip turned from the
  surface's mean normal (TD-124): ring by ring every 5° up to the axis limit, 16 directions on
  each, steepest first, its shaft straight down from its join. The first whose shaft and tip
  reach are clear is taken: `Route::Tilted`, an explicit tip direction.
- The interface carries the direction into the model's frame with the model's placement, as it
  does points, so the tip turns with the model afterwards.
- Manual placement takes the same way, since it is a better shape than a knee for the same tip:
  it changes nothing a user accepted.

**Consequences:**
- **Measured** with the Automatic tool covering everything (Linux release, 16 threads):
  - the owner's `obj_1_quant_1.stl` (1,864,166 triangles; turned -8°, -21.4°, 1.5°; scaled
    200 %; Light): 1 place left instead of 6, 3,537 of 3,538 overhang points instead of 3,528,
    197 standing supports instead of 188 and 75 mini instead of 76; 7.9 s instead of 7.1 s;
  - `HillGiant_32mm.stl`: 6 places left instead of 7, 567 of 568 islands instead of 565, 79 mini
    supports instead of 84; 2.6 s instead of 2.5 s;
  - the built supports have no finding either way.
- Mini supports are still placed where a standard one does not fit for other reasons; their
  rods still touch the model in about one in eight (10 of 75 and 10 of 79), the next step.

**Validation:** a route 1 mm inside a block's edge turns its tip by whole steps within the limit
until the shaft below clears the block; 2 mm inside, no tilt is enough and it goes aside; a tip
beside a bulge leans away from it; the tip's direction turns into the model's frame.
- **Native Windows:** validated on 2026-11-04 with b179c8c (`validate.ps1 -IncludeApp`, a clean
  snapshot of 668 files): no failed command; Debug and Release each pass 1,579 tests with 16
  ignored; the three application smoke runs pass (Vulkan 11.8 ms, OpenGL 12.8 ms, FX-03 16.5 ms
  mean).

## TD-126 — Automatic mini supports keep off the model

**Status:** Accepted on 2026-11-04 (owner): "in automatic supports the supports must never
penetrate the model; if they do, we do not place them, but we must look for more ways than
today to place them". Manual placement stays as tolerant as before (TD-113): there, accepting a
collision is the user's informed choice.

**Context:** a mini support's foot was found only straight below its tip, and its rod could
touch or enter the model anywhere on its way (TD-113). On the Automatic tool's plans, about one
mini support in eight had its rod touching the model away from its balls, one in twenty by more
than half its radius.

**Decision:**
- Placed by the Islands or Automatic tool, a mini support's foot is looked for in a fan of
  directions from the tip's ball: straight down first, then rings every 5° up to 45° from
  vertical, 16 directions on each. The first foot that faces up, keeps the rod within 45° of
  vertical and keeps the rod off the model away from its balls is taken. With none, no mini
  support goes there: the place is left with the reason its standard support had.
- "Away from its balls": farther than the rod's radius and 0.1 mm from either ball's centre
  along the rod; nearer, the rod leaving a ball sitting on the surface still grazes it. The rod
  must not touch the model there at all, with no clearance: mini supports are for tight places.
- By hand, a mini support still stands straight below its tip, and may touch the model.
- Both ways place the balls along the surface's mean normals at the tip and the foot, as the
  build does (TD-124).
- A segment is followed in pieces of at most 2 mm to find what it meets first, so a long
  slanted ray never gathers the triangles of its whole bounding box.

**Consequences:**
- **Measured** with the Automatic tool covering everything (Linux release, 16 threads), against
  TD-125:
  - the owner's `obj_1_quant_1.stl` (1,864,166 triangles; turned, scaled 200 %, Light): no
    mini rod touches the model instead of 10 of 75; no place left instead of 1, all 3,538
    overhang points; 77 mini supports; 7.4 s instead of 7.9 s;
  - `HillGiant_32mm.stl`: no mini rod touching instead of 10 of 79; 1 place left instead of 6,
    all 568 islands instead of 567; 86 mini supports; 2.2 s instead of 2.6 s.
  More places are supported than before: a tilted rod often finds a foot where the one
  straight down met a surface facing down or slanted past 45°.
- The test of the Automatic tool on a local model reports how many mini rods touch the model.

**Validation:** a mini support 0.1 mm beside a block: by hand it stands straight down, its rod
touching the block; placed by a tool it stands elsewhere with its rod clear; clear of
everything, both ways go straight down.
- **Native Windows:** validated on 2026-11-04 with b179c8c (`validate.ps1 -IncludeApp`, a clean
  snapshot of 668 files): no failed command; Debug and Release each pass 1,579 tests with 16
  ignored; the three application smoke runs pass (Vulkan 11.8 ms, OpenGL 12.8 ms, FX-03 16.5 ms
  mean).

## TD-127 — The density of automatic supports, and closer tips under wide undersides

**Status:** Accepted on 2026-11-04 (owner, options 1 and 2 of the analysis, from the mockup
"Support density in the Automatic tool").

**Context:** the Automatic tool spreads tips over an overhang so that every point of it has one
within its reach: the tip spacing under a level surface, growing with the slope to the spacing at
the critical angle (M6.4i). The spacing is one value for the whole model. A wide surface, which
pulls on the film and sags more than a small one, got the same spacing; and the critical angle
changes the density only as a side effect.

**Decision:**
1. **Density**, 50 to 200 %, 100 % by default: the tips per area as a percentage of what the
   spacings give. Each candidate's reach, and the grid an overhang is probed on, scale by
   `sqrt(100 / density)`; the spacings stay in Advanced. The panel shows the spacings it gives.
2. **Closer under wide undersides**, on by default: a region whose underside spans more than
   "Wide from" (20 mm²) takes its tips closer by `sqrt(from / area)`, no closer than "As close
   as" (60 % of the spacing). The area is the region's over its layers, as the analysis reports
   it: what the supports under it hold. The mockup spoke of a layer widened at once; a gentle
   slope spreads a wide surface over many layers, so the underside's area is the measure. An
   island keeps its tip where it starts; its region is probed like an overhang's, so it gets the
   closer tips too.
- The values live with the tool's others: in the panel, in Settings › Supports as defaults, kept
  across restarts; settings without them read the defaults.
- The search for neighbours uses cells as wide as the farthest reach the density allows.

**Alternatives rejected:**
- **Density as a scale of the spacing** (200 % halving it): four times the tips for "twice the
  density" would surprise.
- **Areas drawn by the user** and **density by load:** later, if needed (the owner's options 3
  and 4).

**Consequences:**
- **Measured** with the Automatic tool covering everything and the defaults (Linux release, 16
  threads), against TD-126:
  - the owner's `obj_1_quant_1.stl` (turned, scaled 200 %, Light): 422 supports instead of 370,
    4,031 of 4,032 overhang points (the finer grid probes more), 1 place left instead of none;
    8.1 s instead of 7.4 s;
  - `HillGiant_32mm.stl`: 835 supports instead of 715, all 568 islands, 2 places left instead of
    1; 2.6 s instead of 2.2 s;
  - no mini rod touches the model, no built support has a finding.
- The defaults (20 mm², 60 %) are a start: physical prints should tune them.

**Validation:** the density's scale and the wide factor's three cases; out-of-range values fall
back to the defaults; under a 320 mm² overhang, twice the density gives about twice the tips, no
two closer than the scaled spacing, and the wide underside's 60 % brings them closer still.
- **Native Windows:** validated on 2026-11-04 with b179c8c (`validate.ps1 -IncludeApp`, a clean
  snapshot of 668 files): no failed command; Debug and Release each pass 1,579 tests with 16
  ignored; the three application smoke runs pass (Vulkan 11.8 ms, OpenGL 12.8 ms, FX-03 16.5 ms
  mean).

## TD-128 — The model's lift is the Automatic tool's

**Status:** Accepted on 2026-11-04 (owner): "add the automatic lift to the Automatic supports
tool and take it out of the Raft tool; 5 mm by default is fine". The owner chose that Generate
applies it, and that it is set only there.

**Decision:**
- The Automatic tool has a **Lift**, 0 to 50 mm, 5 mm by default, kept with its other values and
  in Settings › Supports. Generate first rests the model with its lowest point at that height
  above the plate when it stands elsewhere, as one undoable step, then plans on the model where
  it then stands: the plan waits for the model's surface there, as it always waits for it.
- The Raft tool keeps the raft alone; its footer still says how high the model stands.
- The Islands tool's and the Automatic tool's "too close to the plate" reasons point to the
  Automatic tool's Lift. A model can still be placed by hand with the Transform panel's Z.

**Consequences:** supports placed by hand on a model resting on the plate need the model lifted
first, in the Automatic tool or by the Transform panel; the Islands tool says how much more lift
it needs.

**Validation:** Generate lifts a model standing 6 mm high to 5 mm as one step that Undo takes
back, and leaves a model already at the lift untouched.
- **Native Windows:** validated on 2026-11-04 with b179c8c (`validate.ps1 -IncludeApp`, a clean
  snapshot of 668 files): no failed command; Debug and Release each pass 1,579 tests with 16
  ignored; the three application smoke runs pass (Vulkan 11.8 ms, OpenGL 12.8 ms, FX-03 16.5 ms
  mean).

## TD-129 — A route checks the tip as it is built

**Status:** Accepted on 2026-11-04 (owner): the open item "the axis a route checks is not the one
built when the tip points towards the shaft", to be corrected with a decision and a test.

**Context:** the Supports tool's tip direction "towards the shaft" (`AxisSpec::Straight`) builds
the tip pointing at the next point of its path: straight down to the plate below it, or to the
foot straight below it on the model. The route search (TD-108, TD-124, TD-125) always checked a
tip along the surface's normal, with its shaft straight down from that tip's join. Straight down
and on the model, it therefore proved a support other than the one built: one beside a wall could
be accepted while its real tip, straight down, ran along the wall or turned more than the axis
limit from the surface (90° on a side face), and the build then reported it.

**Decision:**
- `RouteQuery::towards_shaft` says which tip is built. With it, the straight-down way and the way
  onto the model check a tip pointing straight down: within the axis limit from the surface's
  mean normal, its cone kept off the model away from its contact, and its shaft from that tip's
  join. Without it, they check the tip along the normal, as before.
- The tilted way and the aside way give the tip's direction themselves (an explicit axis, or the
  knee it points at), so both kinds of tip already checked what is built there; they are
  unchanged. A tip that cannot point straight down within the limit is therefore turned or
  routed aside instead, as one along the normal is.
- The Islands and Automatic tools build their tips from the preset, along the normal, and are
  unchanged; so are mini supports, whose foot search has its own direction (TD-126).

**Consequences:** with "towards the shaft", a support on a steep or sideways face now gets a tilted
tip or goes aside instead of a tip straight down that its build would mark in error.

**Not covered (seen while testing):** the route's clearance is the widest radius of the tip and the
shaft; a support's pad and base cone on the plate are wider, so a column close beside a model
resting on the plate can still meet it there. The build reports it; a model lifted by the
Automatic tool's lift (TD-128) stands clear of it.

**Validation:**
- A core test on a slab: under it, straight down either way; on its side, which faces +X, a tip
  along the normal goes straight down from its join, while a tip towards the shaft is turned
  within 60° of the normal; over a wide block, the foot stands straight below the tip.
- An interface test places supports under a ledge and on its side with each tip direction, and
  builds them without an error; with the old route, the side's support kept a tip straight down.
- **Measured** on the owner's `obj_1_quant_1.stl` (rotated -8, -21.4, 1.5°, scaled 200 %, Light,
  Linux release): the Automatic tool's plans are identical before and after (170 supports for
  the islands, 422 for everything, 1 place left), in 4.61 and 7.85 s before, 4.69 and 8.03 s
  after, within run-to-run noise; the change adds one dot product per route.
- **Native Windows:** validated on 2026-11-05 with 7156099 (`validate.ps1 -IncludeApp`, a clean
  snapshot of 674 files): no failed command; Debug and Release each pass 1,611 tests with 17
  ignored; the three application smoke runs pass (Vulkan 13.2 ms, OpenGL 14.4 ms, FX-03 16.4 ms
  mean).

## TD-130 — The project file: a karasu ZIP package, `.karasu`

**Status:** Accepted on 2026-11-04 (owner), the first part of M6.6. The owner chose a ZIP package
of karasu's own (not 3MF), the `.karasu` extension, cavities recomputed on reopening, the
project file before multiple plates in the application, and a recovery copy saved
automatically. Exporting models as STL or 3MF (single models, a plate, several plates) follows
later, as its own step (owner, 2026-11-04).

**Context:**
- The project model (TD-071) holds every plate in memory: embedded models identified by the
  SHA-256 of their bytes, and plates with snapshots of their printer, resin and print setting,
  their settings and their objects (pose, operations, supports, copies). Nothing persists it,
  so the M7 scenario "reopen and recompute the saved preparation" is not applicable yet.
- Earlier decisions already bind the file: a plate keeps complete snapshots of its profiles, and
  opening a project whose profiles differ from the library's copies of the same identity says so
  and keeps the project's (TD-106); a new project returns the tools' values to their defaults
  (TD-097 revision of 2026-11-02); the Orient tool's protected points are kept with their model
  once the file exists (TD-119); a cavity is stored or recomputed deterministically from its key
  (TD-089).
- **Reference behaviour** (public documentation only; their files were not examined): Lychee's
  `.lys` scene embeds the models with their supports and hollowing, to move work between
  computers, and its `.lyt` only refers to files on disk; Chitubox's `.chitubox` project reopens
  with its supports and hollowing editable. Both keep the models inside one file, as decided
  here. `.ksp`, considered first, is already used by KeyShot packages and KISSlicer projects;
  no format is known to use `.karasu`.

**Decision:**
1. **Package.** A ZIP archive with the `.karasu` extension, recognised by its content, not by
   its name:
   - `karasu-project.json`, the first entry: the project document;
   - `models/<sha256>.<ext>`: each model file the project's plates use, byte for byte as it was
     opened, `<ext>` the lower-case extension of its original name. Models no plate uses are not
     written.
   - Entries are deflated; ZIP64 records are written when an entry, the archive or the entry
     count exceeds the classic limits, so a project is not bounded at 4 GB.
   - **Deterministic:** fixed timestamps, the document first and then the models in the
     project's order, fields in a fixed order, numbers in shortest round-trip form. Saving the
     same project twice gives the same bytes, and so does saving the project it opens to.
   - The writer is independently designed beside the bounded ZIP reader in the core
     (`karasu_core::archive`, which also gives the reader a public face); the SL1 writer's
     stored ZIP is left as it is, so its output stays byte-identical. Entries are deflated at
     level 3, stored when that does not make them smaller.
   - The document is written with `serde` and `serde_json`, the core's own JSON stack, now also
     a direct dependency of `karasu-project`; no new crate enters the tree.
2. **Document.** `{"format": "karasu-project", "version": 1, ...}` with the project's name and
   its plates in order. A plate keeps its identity, name, printer, resin and print setting (the
   profile documents' own JSON, as snapshots), its settings (format, job identity, fill, hole
   closing, reversed tolerance, repairs, topology waiver, layer plan, spacing) and its objects.
   An object keeps its name, model identity and original file name, units, translation, pose,
   operations with their identities and switches (repair, hollowing, drain holes, blockers),
   supports (the support document's own JSON), collision acceptance, copies, arrangement, and
   the Orient tool's protected points (each a point and an outward normal in the model's frame),
   which become part of the object, edited as one undoable step like the rest.
   - Support documents and layer plans gain writers that their existing readers read back
     exactly.
   - Unknown fields are refused. A document of a newer version is refused with a message saying
     a newer karasu-slicer wrote it; an older one is migrated as it is read, when there is one.
3. **Not saved, recomputed or reset on opening:** each plate's undo history (a reopened plate
   starts at a new revision with none); diagnoses, cavities, hole indexes, placements, previews
   and slices, recomputed in the background as when a model is opened (a cavity from its key,
   TD-089); the view (camera, section, visibility, selection); the tools' values, which return
   to their defaults as with a new project, and the user's settings, which stay the user's.
4. **Opening is checked, never repaired.**
   - The archive is read with the bounded reader's limits on entries and sizes; the document
     has its own size limit.
   - Each model's bytes must hash to the identity its name and the document give; a model
     missing, unused or altered refuses the file with the entry named.
   - Every value is validated as an edit validates it (profiles, poses, operations, support
     documents, layer plans); the first invalid one refuses the file with its plate, object and
     field. Nothing is extracted to disk, and entry names never become paths.
   - A plate's profile that differs from the library's copy of the same identity is kept and
     noted (TD-106).
5. **Saving is atomic and off the interface thread.** The project is taken as it stands (its
   plates' revisions recorded), written to a temporary file beside the target and renamed over
   it, so a failed or interrupted save leaves the previous file whole; the failure is reported
   with its cause. The project is unmodified while every plate keeps the revision it was saved
   at and no plate was added or removed.
6. **The application:** File › New project, Open, Open recent (the last ten, kept in the
   settings; one no longer found is removed with a notification), Save and Save as, as actions
   with configurable shortcuts (TD-097). A `.karasu` file dropped on the window or given on the
   command line opens; a model file is added to the plate as now. The window's title shows the
   project's name and a mark while it has unsaved changes. Closing, New and Open with unsaved
   changes ask, in a modal dialog, to save, discard or cancel.
7. **The recovery copy.** While the project has unsaved changes, it is written every few
   minutes (a setting, 3 by default, 0 to turn it off) to a folder of the session under the
   application's data: the document rewritten each time, each model written once by its
   identity, off the interface thread. It is deleted when the project is saved or the
   application closes normally. At start, the copies of sessions that did not end so are
   offered: restore one as an unsaved project, or discard them.
8. **The command line** slices a project: `slice --project <file.karasu> [--project-plate
   <number or name>] --output <path>` through the shared pipeline, so "save, reopen, recompute"
   is testable without the interface. `--plate` already names a plate document (TD-065), hence
   the new options; a project of one plate needs no `--project-plate`, and every option the
   project holds is refused beside it.

**Consequences:** a project file holds its models whole, so it is about as large as they are once
deflated, and reopening a hollowed model hollows it again (about 1 to 23 s on the models
measured in TD-093). Files are readable only by karasu-slicer; models leave it as STL or 3MF
through the export that follows. Associating `.karasu` with the application is packaging work
(M6.7).

**Validation:**
- The core's archive writer and reader round-trip deflated and stored entries, with and without
  ZIP64 records, deterministically; names readers could not tell apart and damaged packages are
  refused. Support documents and layer plans written read back equal.
- A project with two plates (one without profiles), every operation, units, mirroring, copies,
  protected points, supports and a layer plan reads back equal, at revision 1 with no history,
  keeps its plate identities and next identity, leaves out an unused model, and saves to the
  same bytes, as does the project read.
- **Save, reopen, recompute:** a block hollowed, drilled and lifted on two supports, saved and
  reopened, slices to a GOO file byte-identical to the one sliced before saving; the command
  line's `slice --project` writes the same bytes as the library for each of two plates, chosen
  by number or name.
- A foreign file, a package without a document, a truncated one, another format, a newer
  version, an unknown field, a model altered or missing, an entry not listed, a scale of zero
  (reported with its plate, object and field), two plates with one identity and supports the
  core refuses are each refused. ZIP64 records, written for every value as a package beyond
  4 GiB has them, read back the same project.
- A save replaces the file whole; a target that cannot be replaced, or a missing folder, is
  reported and leaves no temporary file.
- **Measured** (Linux release, `a_large_model_saves_and_opens_in_measured_time`): the owner's
  `obj_1_quant_1.stl` (93.2 MB) saves to 48.2 MB in 1.6 s and opens in 0.72 s, at most 416 MB
  resident for the test (the model, its copy in the project, the package and the one read);
  `HillGiant_32mm.stl` (25.0 MB) saves to 18.7 MB in 0.55 s and opens in 0.19 s. Deflate level 3
  gives the same sizes as level 6 in about 70 % of its time.
- **The application** (`window::project_file` tests): a project with a model is unsaved until
  saved, then named after its file and listed first among the recent files; an edit is unsaved
  again; New asks and Escape keeps the project; Open asks, Enter saves first and then opens
  what was saved, at revision 1 with its edit; New with nothing unsaved does not ask. A recovery
  copy is written once due, outlives a crash (its lock released, its folder kept), is the one
  offered at the next start (a copy a running session holds is not, and an empty folder is
  removed) and restores the project unsaved, its folder removed. The Orient tool's points are
  plate edits, undone and redone with the plate.
- **Opened all at once** (owner, 2026-11-04: the model, then its hollowing, then its supports
  appeared one after another): a dialog shows what is being done until the models are
  imported, the preparation has a result for the plate and every support that can be built is,
  and the scene draws nothing until then. A model reopened is placed once its mesh arrives,
  though the plate's revision stays (the owner's reopened marshal showed its cavity alone).
- **Interface cost:** the unsaved check, the title and the recovery schedule allocate nothing
  per frame: the state is compared in place, the title set only when it changes, and the state
  taken only when a copy is due. Opening recreates the viewport once, with the tools' states.
- **Native Windows:** validated on 2026-11-05 with 7156099 (`validate.ps1 -IncludeApp`, a clean
  snapshot of 674 files): no failed command; Debug and Release each pass 1,611 tests with 17
  ignored; the three application smoke runs pass (Vulkan 13.2 ms, OpenGL 14.4 ms, FX-03 16.4 ms
  mean).

## TD-131 — A support in a drain hole is a warning

**Status:** Accepted on 2026-11-05 (owner): "make 'Plugs a drain hole' permissive, so automatic
supports can also place a support near a drain hole; a drain hole often makes an island that
was not there before, so islands come first." It settles TD-123's open decision. Points 3 and
4 were added the same day after the owner's trials.

**Context:**
- TD-123 made a support part entering a drain hole's channel through the model an error. The
  Automatic and Islands tools drop every planned support a slice would build with an error, so
  they never placed a support in a hole: an island starting where a hole is was left
  ("Its support would plug a drain hole"), or held from a place at least 2 mm aside.
- A hole up through a model's underside makes overhangs of its own: the end of a blind hole,
  and the rim where the hole meets a cavity or a sculpted surface. Those are islands the model
  did not have, and they need a support where they start.

**Decision:**
1. **The check:** a part entering a hole's channel is a warning, no longer an error: "enters
   drain hole k through the model and may partly plug it". The channel is unchanged (TD-123).
   Slicing proceeds, and the warning stays reported.
2. **Automatic and Islands:** a support in a hole is kept, as any support with only warnings.
3. **No tip where a hole cuts the material away** (owner, 2026-11-05, on the first trial: "the
   automatic supports are placed to support the hole, where there is nothing to support"). The
   surface for routes is the model's, without the holes, so a probe under a hole still meets
   the surface the hole removes. A point strictly inside a hole's cylinder
   (`DrainCylinder::removes`; the rim is not inside) is therefore:
   - no overhang candidate for the Automatic tool: there is nothing to hold, and it is not
     counted as a place left;
   - a refusal in the final check for a support whose tip stands there ("A drain hole cuts the
     material away there: nothing to hold"), tried again at least 2 mm aside as other
     refusals are, with no mini support in its place.
4. **No foot where a hole cuts the model away** (owner, 2026-11-05, on the second trial:
   supports in the cavity stood on the cavity's floor right over a hole, on nothing). A support
   whose base stands on the model at a point a hole removes is an error for every support, by
   hand too: `FootInHole`, "stands at p where drain hole k cuts the model away: nothing is under
   it"; the lists say "Foot in a drain hole". The Automatic and Islands tools' final check
   removes such a support ("Its support's foot would stand where a drain hole cuts the model
   away"), and a mini support may take its place, its foot found in its own fan of directions.
5. **The Supports tool's list:** a support in a hole says "In a drain hole" in the warning
   colour, its tooltip the full finding; it is drawn as a valid support.
6. **Braces stay out of holes:** a brace holds no island, so the brace plan still skips one
   entering a channel (`Skipped::DrainHole`).

**Alternatives rejected:**
- **An error only when a support blocks a large share of the opening:** a share needs the
  opening's area at each height and the part's section there, for a judgment the owner makes
  better by looking; a thin tip in a hole is already permitted by this decision, and the list
  shows it.
- **Removing the check:** a support in a hole would then go unseen.

**Consequences:**
- An island a hole made gets its support where it starts; the hole stays open around it. A
  slice keeps the support's material in the hole (TD-123).
- Plans of models without holes, or whose supports enter none, are unchanged. The added work
  is one point-in-cylinder test per probe and per planned tip, for each hole: nothing
  measurable beside the probes' ray casts.
- A blind hole's end inside the material is a new overhang the route surface does not have;
  it is not supported automatically (to be revisited with real cases).

**Validation:**
- Core: a support whose tip reaches into a hole is a drain-hole warning and the scene has no
  error; one on the rim, one crossing a cavity on the hole's axis and holes beside the model
  give no finding, as before; a tip on a cavity's ceiling over the hole warns.
- Core: a point strictly inside a hole's cylinder is removed, one on the rim, before its start
  or past its end is not, along a tilted axis too, and a degenerate axis removes nothing.
- Core: in a hollow slab with a hole through both walls, a support from the ceiling over the
  hole to the floor over it has the foot error (and the drain-hole warning); one whose foot is
  2 mm from the hole's axis has no error.
- Automatic: under a slab's overhang with a 4 mm hole, no candidate lies inside the hole and
  the others are those without the hole.
- Islands: an island starting at a point over a hole is skipped as cut away; one over the
  whole underside around the hole is held by one support at least 2 mm aside; applied, it
  builds without an error.
- **Native Windows:** validated on 2026-11-05 with 7156099 (`validate.ps1 -IncludeApp`, a clean
  snapshot of 674 files): no failed command; Debug and Release each pass 1,611 tests with 17
  ignored; the three application smoke runs pass (Vulkan 13.2 ms, OpenGL 14.4 ms, FX-03 16.4 ms
  mean).

## TD-132 — A model foot points up towards its shaft

**Status:** Accepted on 2026-11-05 (owner report: "many islands were introduced by the supports
themselves"; screenshots of supports whose shaft's bottom hangs past the foot).

**Context:**
- UVtools reported 84 islands on the owner's `venom_torso_2.pp1` (Photon P1, 2004 automatic
  supports). Many lie at the bottom of supports standing on the model.
- A route that stands a support on the model gave the foot the surface's normal
  (`AxisSpec::Normal`). Feet stand on surfaces facing up, but on a sculpted model many are steep:
  a foot 68° from vertical leaves almost level.
- Every path point of a standard support carries a ball of the shaft's radius there, the foot's
  join too. A merged trunk's shaft widens to twice its diameter at its bottom (`wider`). The
  lowest point of that ball then lies outside the foot, below the foot's side: the ball's bottom
  is a separate piece of the layer, which starts in mid-air.

**Decision:**
1. **The foot's axis:** a route on the model gives the foot `foot_axis(mean normal, limit)`:
   - straight up when that is within the axis limit of the surface's mean normal, measured
     within the foot's contact radius as the build measures it;
   - otherwise the normal turned up by the limit less 0.5°.
   The foot's join, the shaft's clearance and its slant are checked with that axis. The support
   records it as an explicit direction (`AxisSpec::Direction`), which follows the model's
   placement as any direction does.
2. **Nothing else changes:** supports placed by hand keep the axis their user gives; the Islands
   and Automatic tools route their supports this way.

**Alternatives rejected:**
- **Judging the way with the foot along the normal, then turning it up where the shaft allows**
  (asked by the owner to keep the same supports on the model): measured, it does not keep them.
  The plan's shape changes anyway, because a foot pointing up ends the shaft about 1.5 mm higher
  on a steep surface, which leaves branches fewer heights to join the trunk; and where the
  turned foot has no room, the ball still hangs (7 to 11 on the torso).
- **A wider foot under a wider trunk:** it helps a vertical foot only; a level foot still leaves
  the ball's bottom outside.

**Consequences, measured** on `Torso_2.stl` (1,182,264 triangles, rotated 12.70°, 4.36°,
63.71°, hollowed 2 mm, lifted 5 mm; the Automatic tool with its defaults, everything covered;
Linux release):

| | Before | After |
| --- | --- | --- |
| Feet on the model | 189 | 372 |
| Their shaft's bottom ball outside the foot | 64 | 0 |
| Supports standing / branches | 437 / 1,564 | 692 / 1,309 |
| Braces | 448 | 1,901 |
| Mini supports | 41 | 104 |
| Places left | 6 | 6 |
| Planning time with the search | 12.5 s | 13.9 s |

- The same places are held and the same are left. More tips stand on their own feet instead of
  joining a trunk: the shorter shafts above upright feet leave branches fewer heights to join.
  More stand on the model, and the standing supports take more braces between them.
- On the cavity's floor, which faces up, the foot was nearly upright already.
- A wide trunk's ball may now come too close to a steep surface beside the foot; the build says
  so as a collision, and the automatic tools' final check removes such a support.

**Validation:**
- `foot_axis`: up within the limit, the normal turned up by 59.5° beyond it, and the normal kept
  for a surface facing straight down.
- On a slab over a ramp 68° from level, a support's bottom ball twice the shaft's diameter lies
  outside a foot along the normal and inside one turned up; built so, the support has no error.
- The route's foot on a block's top points straight up.
- **Native Windows:** validated on 2026-11-05 with 7156099 (`validate.ps1 -IncludeApp`, a clean
  snapshot of 674 files): no failed command; Debug and Release each pass 1,611 tests with 17
  ignored; the three application smoke runs pass (Vulkan 13.2 ms, OpenGL 14.4 ms, FX-03 16.4 ms
  mean).

## TD-133 — PP1 layers deflated in the ZIP

**Status:** Accepted and implemented on 2026-11-05 (owner: "the karasu PP1 weighs a lot next to
Lychee's"). Photon Workshop opens a deflated file (owner, 2026-11-05: `venom_torso_2_v5.pp1`);
awaiting a print of one on the P1.

**Context:**
- The PP1 writer stores every ZIP member uncompressed (method 0, `M5-PP1-SPEC.md`). That was
  chosen in M5, before the crate had a deflate encoder.
- The owner's `venom_torso_2_v4.pp1` (one torso, 1,973 supports, anti-aliasing 16, 2,151
  layers) is 413.6 MB, nearly all of it PW0 layer images (412.3 MB). Lychee's
  `venom_body_full_plate_1.pp1` (the torso, the head and the tongue, 2,201 layers) is 160.8 MB:
  295.2 MB of layers, deflated.
- Every PP1 the owner's references wrote is deflated (method 8): Photon Workshop's
  (`AC-PC`) and Lychee 8.0.0's, in `anycubic_export_test` and `lychee_export_test` [O]. The
  P1 prints Lychee's files, so its firmware reads deflated layers [I].
- The crate already deflates the project package (TD-130) with the pinned
  `miniz_oxide 0.9.1`, a native-Rust, safe encoder; level 3 is measured there.
- PW0 is a run-length code. Deflate still finds repetition across its runs (run bytes repeat
  along edges, row after row).

**Measured** (Python's zlib on every hundredth layer of each file, 22 and 23 layers):

| File | Layers stored | Deflate 3 | Deflate 6 |
| --- | --- | --- | --- |
| karasu v4 | 412.3 MB | 0.613 → about 253 MB | 0.597 → about 246 MB |
| Lychee | 295.2 MB raw | 0.559 → about 165 MB | 0.537 → about 159 MB (its file: 160.4 MB) |

Level 3 took 0.2 s for the 22 karasu layers: about 20 s of CPU for the whole file, spread over
the export's encoding workers. The rest of the gap with Lychee is the layers' content (more
supports and edges, 16 gray levels against 12), not the container.

**Decision:**
1. **Members deflated:** each PP1 member is deflated at level 3 (`archive::DEFLATE_LEVEL`) and
   written with method 8, or stored when deflating does not make it smaller, as the project
   package does. CRC-32 and sizes are those of the uncompressed bytes, as APPNOTE requires.
2. **In the encoding workers:** a layer's PW0 bytes are deflated, and their CRC taken, in
   `Pp1Session::encode`, which already runs on the export's worker threads; the writer then
   only copies the result in order. The metadata, scene and previews are deflated on the
   writing thread; a member that does not shrink is stored.
3. **The container:** the bounded writer used by PP1 and SL1 (`ClassicZip`, formerly
   `StoredZip`) takes members prepared beforehand (`Member`); SL1 keeps storing (its layers are
   PNGs, already deflated). No ZIP64: a PP1 stays under 4 GiB, and the
   existing limits still refuse anything larger.
4. **Deterministic:** the same bytes for the same job on every run and thread count (each
   member is deflated alone, with a pinned encoder and level).
5. **Readers:** the independent `tools/m5-validate` reader (Python's `zipfile`) already reads
   deflated members; its stored-only checks change to accept both methods.

**Alternatives considered:**
- **Level 6:** 2.6 % smaller than level 3 on these layers, in about 1.4 times the time.
- **Keeping stored members:** no reader needs them; every reference writes deflated files.
- **Fewer gray levels:** a print setting already chooses the anti-aliasing (see the owner's
  request for a per-plate override, to be decided separately).

**Measured** on the owner's `venom_torso_2.karasu` (`slice --project`, Linux release, 16
threads):

| | Before (stored) | After (deflated) |
| --- | --- | --- |
| PP1 file | 413.6 MB | 252.5 MB (−39 %) |
| Wall time, slicing and export | 26.6 s | 28.0 s |
| CPU time | 321 s | 340 s |
| Peak resident memory | 1.21 GB | 1.21 GB |

**Validation:**
- Unit: a deflated member reads back with its CRC-32 and size, a member of noise is stored, the
  same bytes give the same member; every PP1 test now reads members inflated and checks each
  one's CRC and size.
- The independent reader (`tools/m5-validate/validate_pp1.py`, Python's `zipfile`) reads the
  deflated venom file: the first 40 layers' masks equal the stored file's, and every member's
  CRC checks.
- The export's thread-count tests keep the file the same for every thread count.
- **External, by the owner:** Photon Workshop opens the deflated `venom_torso_2_v5.pp1`
  (2026-11-05; SHA-256 `eea6df2d…57ad`, 171.5 MB, binary supports). The owner printed a
  deflated PP1 on the P1 on 2026-11-10, so the P1's firmware reads the deflated layers karasu
  writes, not only Lychee's.
- **Native Windows:** validated on 2026-11-05 with 7156099 (`validate.ps1 -IncludeApp`, a clean
  snapshot of 674 files): no failed command; Debug and Release each pass 1,611 tests with 17
  ignored; the three application smoke runs pass (Vulkan 13.2 ms, OpenGL 14.4 ms, FX-03 16.4 ms
  mean).

## TD-134 — The plate overrides its print setting's anti-aliasing; binary supports

**Status:** Accepted and implemented on 2026-11-05 (owner: "in the Slice tool it should be
possible to override the anti-aliasing settings, and even to turn it off"; then, asked whether
the supports are anti-aliased too: "yes, include it in TD-134"). Awaiting the owner's trial.

**Context:**
- A print setting holds the masks' anti-aliasing (`image.antialias`: 1, 2, 4, 8 or 16 samples
  per pixel axis, TD-052) and the smallest gray of a partly covered pixel (`image.min_gray`).
  A plate keeps a copy of its print setting; the Slice panel shows both values read-only.
- Changing them today means editing the print setting, which changes every plate that uses it.
- Anti-aliasing 16 samples each pixel on a 16 × 16 grid; 1 gives binary masks. Fewer samples
  slice faster and give fewer gray runs to encode, so smaller files (see TD-133).

**Decision (owner, 2026-11-05: on the plate; anti-aliasing, the smallest gray, and the
supports):**
1. **The plate's override:** `PlateSettings.image` is an `ImageOverride`,
   `{ antialias: Option<u8>, min_gray: Option<u8>, antialias_supports: bool }`. Each value
   given replaces the print setting's for this plate only; otherwise the print setting's
   applies. A value chosen equal to the print setting's is stored as none, so the plate keeps
   following its print setting.
2. **Consistency:** anti-aliasing 1 (binary masks) makes the smallest gray 0, whatever either
   source says, as the print setting editor does; a smallest gray needs anti-aliasing 2 or more.
   A format that stores binary masks only refuses anti-aliasing above 1, as today, with the
   same message.
3. **The Slice panel:** an **Anti-aliasing** row (Off, 2, 4, 8 or 16), a **Lowest grey** row
   (0 to 255, enabled only with anti-aliasing on) and an **Anti-alias the supports** switch,
   each showing the value the plate slices with. A value that differs from the print setting's
   has a reset button beside it, its tooltip naming the print setting's. Levels the plate's
   format cannot store are disabled, with a tooltip. The print setting's editor and facts name
   the levels the same way (owner, 2026-11-05: "let us make them uniform"): Off for one sample
   per pixel side, binary masks, then 2, 4, 8 or 16.
4. **An edit of the plate:** choosing a value is one undoable plate edit; the slice of the plate
   as it was becomes out of date, as any plate edit makes it.
5. **The project file:** the override is written in the plate's settings only when present, so a
   project without one is written exactly as before and older builds still read it. The format
   version stays 1; an older build refuses a project with an override as an unknown field.
6. **The command line:** `slice --project` slices with the plate's override, because the request
   is taken from the plate. `--antialias` and `--min-gray` keep their meaning for a model or a
   plate document; `--antialias-supports` anti-aliases its supports.
7. **Binary supports (the default):** the supports' material is sampled apart from the model's.
   The model alone (`WindingLayer::model_segments`: the segments of the faces that are not
   supports, closure chords included) is anti-aliased as asked, minus any drain-hole cut; a pixel whose centre-most sample
   (column and sub-row `n / 2` of `n`) lies in a support is full. The two are united per sample
   row, so no pixel counts more than full. With anti-aliasing off nothing changes. A support's
   edge then has no gray, so a thin tip cures whole, and its many short runs no longer split
   into gray levels. `RasterConfig.kept` (`KeptSampling::Binary` or `Antialiased`) carries it
   to the kernel.

**Alternatives considered:**
- **An application preference instead of the plate:** a project reopened elsewhere would slice
  differently, against the rule that a project holds what it prints.
- **Binary supports in the print setting:** the setting is the resin's calibration; whether
  supports are smooth is the plate's choice, as their layout is.
- **Sampling every support pixel at its centre without separating the model:** where a tip
  enters the model, the model's own samples would be lost and leave a gray ring inside it.
- **A copy of the print setting edited for the plate:** the plate would no longer follow its
  print setting's other changes.

**Measured** on the owner's `venom_torso_2.karasu` (`slice --project`, PP1 deflated as TD-133
writes it, Linux release, 16 threads; peak memory 1.2 GB in every case):

| Plate's masks | Wall time | CPU time | PP1 file |
| --- | --- | --- | --- |
| Anti-aliasing 16, supports anti-aliased (as before) | 29.4 s | 347 s | 252.5 MB |
| Anti-aliasing 16, binary supports (the default) | 16.8 s | 170 s | 171.5 MB |
| Anti-aliasing 4, binary supports | 15.9 s | 120 s | 168.7 MB |
| Off | 13.7 s | 105 s | 133.9 MB |

Binary supports leave the supports out of the 16 × 16 sampling, so slicing is faster, not
slower. A first version that cancelled the supports out of the whole layer with reversed edges
wrote the same bytes in 38.0 s: the two ways agree, and the model's own segments are the faster.
The file with anti-aliased supports is byte for byte the file before TD-134.

**Validation:**
- Kernel: a winding layer lists the model's segments apart from the supports' (the 2 × 3 box
  beside a kept 1 × 1 box).
- Renderer: a model and two supports, beside it and across its edge, with 2, 4 and 16 samples:
  every pixel equals a per-sample oracle (full where its centre-most sample is in a support,
  otherwise the model's own coverage); anti-aliased, the same layer has gray at the supports'
  edges.
- Command line: a slab a quarter pixel off the grid on two supports, at anti-aliasing 4: below the
  slab the support layers have no gray, with `--antialias-supports` they have; the slab's own
  layers keep their gray and are the same either way.
- Project: the override's anti-aliasing, smallest gray (Off clears it) and supports reach the
  request; it reads back equal, is written in the plate's settings only when present, and a level
  the masks cannot take is refused with where.
- Interface: the rows show, drawing changes nothing, an override is an undoable edit that moves
  the plate's revision, and a gray typed digit by digit is one step.
- **Native Windows:** validated on 2026-11-05 with 7156099 (`validate.ps1 -IncludeApp`, a clean
  snapshot of 674 files): no failed command; Debug and Release each pass 1,611 tests with 17
  ignored; the three application smoke runs pass (Vulkan 13.2 ms, OpenGL 14.4 ms, FX-03 16.4 ms
  mean).

## TD-135 — Several plates in the application

**Status:** Accepted on 2026-11-05 from the owner-approved mockup ("karasu multiple plates
mockup": the plates bar at the bottom, top-view thumbnails, the overflow to a new plate and a
Plate menu). Phases 1 and 2 implemented the same day, phase 3 on 2026-11-06; phases 4 and 5,
put off on 2026-11-06, implemented on 2026-11-07 (owner: "terminare fase 4 e 5").

**Context:**
- The project holds several plates since M6.1: each its own printer, resin, print setting,
  slicing settings, objects and history (`PlateContent`, `History` per plate), and the project
  file saves them all (TD-130). The application shows one plate and offers no way to another.
- Undo is per plate: adding, removing or reordering plates, and moving an object from one plate
  to another, change two plates or the list itself, which no plate's history holds.
- The command line arranges copies with a deterministic shelf packer and refuses what does not
  fit; the application has no Arrange yet.

**Decision (owner, 2026-11-05):**
1. **The plates bar:** a strip at the bottom of the viewport, one card per plate in project
   order: a top-view thumbnail, the name, the number of models and the state of its slice
   (sliced, out of date, not sliced, failed); the current plate lit. Then **New** and **All
   plates…**. A click opens a plate; cards reorder by dragging; a model row dropped on a card
   moves it there, with Ctrl copies it.
   **Collapsible** (owner, 2026-11-05: "otherwise it takes room from the viewport"): its button
   folds it to a row about 30 px tall of tabs, one per plate with its name and a state dot,
   between Previous and Next plate, then New and All plates…; it opens, moves and takes dropped
   models as the cards do. Folded or not is remembered across restarts (a persistent setting,
   TD-097) and toggled by Plate › Show plates bar. Folded, no thumbnail is drawn.
2. **Thumbnails from above:** each plate's models (with supports and raft) drawn from above by
   the GPU into a small texture, on the plate's own outline; drawn again only when that plate's
   revision changes, never per frame, and never for a plate whose models are still loading.
3. **The Plate menu:** New plate, Duplicate plate, Rename plate (F2), Delete plate; Previous and
   Next plate (Ctrl+Page Up, Ctrl+Page Down); Show plates bar; Arrange models…; All plates…;
   Export every plate…. Every entry is an action (TD-097), its shortcut configurable.
4. **The overview:** a tool window of cards with a larger thumbnail, the plate's profiles and
   its last slice's layers, time and resin, with Open, Duplicate, Export and Delete, and
   footer totals with **Slice every plate** and **Export every plate…**.
5. **Moving and copying a model:** its row's menu (Move to plate, Copy to plate, each listing the
   plates and New plate) or a drop on a card. The object moves whole: source, transform and its
   whole preparation (repair, hollowing, drain holes, blockers, supports, protected points,
   copies). It lands at the same place when it fits there clear of the others, otherwise beside
   them as Arrange places it; where it fits nowhere it stays, and a notification says so. A
   target with another printer keeps its own profiles; a model outside its volume shows so.
6. **Export every plate:** a modal dialog with the folder, a file-name pattern
   (`{project} {number} {plate}`), the plates with a check box, file, format and state, and a
   switch to slice first the plates not sliced or out of date (off: they are left out and
   listed). Plates slice one after another off the interface thread; an existing file is never
   replaced without asking; each file is written whole or not at all.
7. **Arrange with the overflow:** Arrange models… arranges the plate's models with the shelf
   packer (footprints with supports and rafts, a spacing). Models that do not fit go to a new
   plate with the same profiles and settings, arranged the same way, as many plates as needed;
   or, by choice, stay where they are. One undoable step.
8. **Undo across plates:** the project gains a history of its structure beside the plates':
   adding, duplicating, deleting, renaming and reordering plates, moving and copying objects
   between plates and Arrange's new plates are each one step, holding the plates it changed.
   Ctrl+Z takes the latest step of the current plate or of the structure, whichever is newer.
9. **The project file** needs no change: it already holds every plate in order (TD-130).

**Phases:**
1. The current plate, the plates bar with thumbnails, the Plate menu, and the structure's undo.
2. The overview window.
3. Moving and copying models between plates.
4. Export every plate.
5. Arrange, with the overflow to new plates.

**Performance:** a plate's thumbnail is one offscreen render at its revision; switching plates
uploads nothing a loaded model already has (meshes are kept per model, as today), and the
inactive plates' preparation is recomputed only when they are opened or sliced.

**Phase 1, as implemented (2026-11-05):**
- **Revisions unique across the project:** every plate edit, undo and redo takes the project's
  next revision, a plate added or read from a file too. The interfaces' caches keyed by a
  revision (placements, preparation, islands, plans, slices) can therefore never take one
  plate's result for another's.
- **The structure's history** in `karasu-project`: `insert_plate`, `delete_plate` and
  `move_plate` are steps; each plate's steps and the structure's carry the revision they were
  made at, so `Project::undo` takes the newest of the plate shown and of the structure, and
  `redo` the one undone last. A plate taken out by a step is kept in it as it is then, its own
  history included, so putting it back restores it whole. Renaming is an edit of the plate.
- **Showing another plate** rebuilds the per-plate interfaces as opening a project does, keeps
  the tools' values for the next operation (`tool_values`, owner 2026-11-02) and the settings,
  and keeps each plate's `SlicingUi`, polled while another plate is shown. The viewport and its
  uploaded meshes stay.
- **Thumbnails** (`Viewport::render_thumbnail`): the viewport's own pipeline, meshes and slots,
  with the targets of the thumbnail swapped in for the call, an orthographic view from above
  of the printer's plate, 192 × 108 pixels, without the plate, lines, front word, section,
  holes or outline. At most one a frame; a plate without a printer or with a model still
  loading waits; the plate shown at most every 500 ms while it changes.
- **Not yet in phase 1:** thumbnails show the models without their supports and raft (those
  are built only for the plate shown); the overview, moving and copying models, Export every
  plate and Arrange are phases 2 to 5. Each kept plate's slicing holds its layers' masks in
  memory, as the plate shown's always has: to be measured with several large plates.

**Phase 2, as implemented (2026-11-05):**
- **The overview** is a tool window (not modal; it opens in the middle of the application
  every time, owner 2026-11-05, and as large as it was left, 940 × 640 points the first time),
  opened and closed by **All plates…** in the plates bar, open or folded, and in the Plate menu
  (an action, TD-097, with no default shortcut). Its facts are rows of one small line, so the
  card's actions never cover them.
- **Names and deletion** (owner, 2026-11-05, from the first trial): a new plate is named
  "Plate n" with the first number from the plates' count up that no plate is named with
  (deleting Plate 1 and adding a plate no longer gives two "Plate 2"); deleting a plate with
  models asks first, from the menu, the bar or the overview, and an empty plate goes at once;
  Undo brings either back.
- **One card per plate** in project order, as many side by side as fit, then a dashed card
  that adds a plate. A card: the thumbnail from above, 256 × 144 points (512 × 288 pixels),
  with the slice's state at its top right and the box the models cover ("Models cover 96 × 71
  mm", the models alone, from their placements) at its bottom left; the plate's number and
  name; its printer, resin and print setting; its models; its last slice's layers and height,
  print time and resin, faded when the plate has changed since. The plate shown is lit as in
  the bar.
- **Actions:** Open (disabled for the plate shown), Duplicate (the copy after its plate, shown),
  Export (the plate's last slice, offered under the plate's name; only while the slice is of
  the plate as it is, as the Slice panel's Export) and Delete (undoable; disabled on the last
  plate). A card's image dragged onto another's moves the plate there; a double click shows
  it. They act on the card's plate, not on the plate shown: deleting another plate leaves the
  plate shown.
- **The footer** totals the plates: the models, how many plates are sliced as they are, and
  those slices' print time and resin. **Slice every plate** and **Export every plate…** come
  with phase 4, which slices plates not shown.
- **The other plates' slicing** reports in the messages as the plate shown's does: a slice
  ending, an export written.
- **Showing another plate from the bar or the overview** takes effect in the same frame: what
  is drawn after them (the tools' panels, the overview) is already of the plate shown.
- **Cost:** the overview's thumbnails are a second size kept beside the bar's, drawn only while
  the overview is open, one a frame at most, again only when their plate's revision changes;
  the footprint comes with them, from the placements the thumbnail already computes.

**Phase 3, as implemented (2026-11-06):**
- **From the models' list:** a row's menu has Move to plate and Copy to plate, each listing the
  other plates ("2 · Torso") and New plate; a row dragged onto a plate's card or tab in the
  plates bar, or its image in the overview, moves the model there, with Ctrl held copies it. The
  plate under the pointer is outlined while it can take the model, and a line beside the pointer
  says what dropping does. The plate shown stays shown; a notification says where the model went.
- **Whole:** the object goes with its source, transform and whole preparation (repair, hollowing,
  drain holes, blockers, supports, protected points, copies). A new plate goes after the plate
  shown with its profiles and settings, named as New plate names one.
- **Where it lands:** its footprint is the placed model with its supports' tips, bends, feet and
  pads, and the raft's fillet, margin and slope beyond them (drain-hole cutters lie within the
  model). It stays where it is when that footprint lies on the target's plate clear of the
  models there by the target's spacing; otherwise the shelf packer (the command line's, TD-065)
  places it beside them, centred on an empty plate, and the object is no longer centred by its
  pose. With no room, it stays on its plate and a warning says so; with a model still loading,
  nothing moves. A target with another printer keeps its own profiles.
- **Undo:** the project's structure has a step that replaces several plates' contents at once,
  and one that is several steps (the move and the new plate): a move or a copy is one step,
  undone and redone whole. A plate it changed and edited since is undone first, newest first
  (and shown), so the step finds the contents it left; an edit of such a plate after the step
  is undone leaves the step nothing to redo. As for every plate, a plate's own undone edits are
  redone when that plate is shown.

**Phases 4 and 5, as implemented (2026-11-07):**
- **Actions** (TD-097, no default shortcut): Arrange models… (enabled while the plate shown has
  models and a printer), Slice every plate and Export every plate… (disabled while a batch
  runs), in the Plate menu; Slice every plate and Export every plate… also in the overview's
  footer, right of the totals.
- **A batch** takes its plates one after another in project order, one slice at a time, each
  by the plate's own `SlicingUi` (the plate shown's, or the one kept for it; a plate never
  sliced gets one writing where the plate shown's writes), off the interface thread, polled
  every frame as before. A plate whose last slice is of the plate as it is is not sliced again.
  A slice already running for a plate (the Slice panel's) is awaited, not started twice. A
  plate deleted meanwhile, a slice that fails, or a plate edited while it slices is reported in
  the batch's notification and never exported; the batch goes on with the next plate.
- **Messages:** a batch's slice names its plate in its messages ("Torso: …"); its plain
  success and its cancellation are left to the batch's single notification at the end: how
  many plates were sliced or exported and where, then the plates left out and the failures with
  their reasons, or, stopped, how many were done and how many not reached. While a plate slices,
  the status bar and the overview's footer say "Slicing plate 2 of 3 · Torso · 42 %"; the
  overview's footer then offers Stop.
- **Export every plate…** is a modal, resizable dialog: the folder (typed, or Choose… for the
  system's folder picker; created when missing), the file names' pattern with `{project}` (the
  project file's name, else the project's, else "Untitled"), `{number}` and `{plate}`, the
  extension each plate's format's (none for the reference format's folder); names are made
  safe on every system (no separator, reserved or control character, no dot or space at the
  end) and never repeated (" (2)"). One row per plate: its check box, "1 · Head", the file, the
  format and the state ("Exported · 54 MB", "Slicing…", "Waiting", "Left out", "Failed", or
  where its slice stands). The switch slices first the plates not sliced, or out of date; off,
  they are left out and listed. Files already in the folder are named in a confirmation before
  anything is written; each file is published whole through a staging file, as a single export
  is. While it works the controls are disabled, the footer shows the progress, and Cancel (or
  Escape) stops the batch: the slice running is cancelled, files written stay. The folder, the
  pattern and the switch are settings kept across restarts (`PlateSettings`).
- **Arrange models…** is a modal dialog titled "Arrange models · Arms": the spacing, the
  plate's own to start with (not kept: it is the plate's setting); "All on the plate"; the
  choice "Go to a new plate" or "Stay where they are" (kept across restarts) with its help
  line. The shelf packer (TD-065) places every model's footprint (supports and raft included,
  as a move between plates measures it) on the printer's plate; when some do not fit, the
  packer's leftovers are taken out and the rest packed again until all that remain fit. The
  overflow goes to new plates after the plate shown, named "Arms (2)", "Arms (3)" (a name taken
  is skipped), each with the plate's profiles and settings and packed the same way, as many as
  needed; or stays where it is. Placed models are no longer centred by their pose. The whole
  arrangement is one step of the project's structure (contents and new plates together); the
  plate shown stays. A notification: "Arranged: 4 models on Arms", detail "2 did not fit and
  went to a new plate, Arms (2). Ctrl+Z undoes the whole arrangement." (a warning when models
  stayed).
- **From the owner's first trial (2026-11-07):**
  - **The format:** Slice every plate failed on two Photon P1 plates, sliced as GOO, which the
    printer does not take. A plate stores a format its printer lists only once the Slice panel
    has been open on it. A slice now uses the format the panel would settle on (the plate's own
    when its printer writes it, else the printer's first; the reference format, chosen in
    Advanced, stays), and so do the dialog's file names and formats.
  - **The slices' files** (a fault since phase 1): every plate's slicing numbered its files
    from 1 in the shared cache folder, and each new slicing, made when a plate is first shown,
    emptied that folder. One plate's slice could replace or delete another's file, and that
    plate's export then failed or wrote the other plate. Files are now numbered across the
    process, in a folder of the session's own.
- **The slices' files on disk (owner's question, 2026-11-07):** each session writes into its
  own folder, `slices/<process>`, holding a lock file there while it runs (as the recovery
  copies do, TD-130). A session starting removes the folders whose lock is free, and loose
  files an earlier version left; another application running beside it keeps its slices
  (before, the second application emptied the folder). A slice ending removes its plate's
  previous file, as before; a plate's slicing let go (its plate deleted, a new plate undone,
  another project opened) removes its file, and a running job's output should it be there; the
  application closing removes the session's files, the next session the folder. So the disk
  holds one file per plate sliced at most while the application runs.
- **Another project (a fault since phase 1):** plate identities and revisions start over in a
  project made or opened, but the last project's plates' slicings and thumbnails were kept: its
  plate 1's slice became the new plate 1's, possibly as Sliced and exportable. Making or
  opening a project now forgets them (the thumbnails freed from the renderer first, a running
  batch dropped); Arrange's and Export every plate's choices stay.
- **Cost:** Arrange computes each model's footprint from its placement once and packs a few
  rectangles; a batch adds a scan of its rows per frame and nothing to the frame while idle.
  Each plate sliced keeps its layers' masks in memory as before (phase 1's note).

**Validation (phase 1):**
- Project: unique revisions (a new plate's is above every other's); insert, move and delete
  undone and redone with the plates' own edits in order, from the plate shown as the
  application follows it; a plate's own redo kept through its removal and return; plates read
  from a file at new revisions with no history.
- Workspace: plates added, duplicated, shown, stepped with wrapping, moved and deleted, then
  undone newest first; the last plate stays.
- Window: a new plate starts with no selection and no slice, the tools' values kept; the first
  plate's failed slice is there again when it is shown; undoing the new plate drops its
  slicing; folding the bar is a kept setting. The bar draws every plate's name, models and
  state, open or folded.

**Validation (phase 2):**
- Workspace: a plate not shown is deleted and the plate shown stays; undone, it is back; a
  plate not shown is duplicated after itself and its copy shown.
- Window: with a sliced plate and a new one, the overview draws its title, both cards' number
  and name, the profiles, the states Sliced and Not sliced, the slice's layers and height, the
  New plate card and the totals (1 model, 1 of 2 sliced, the time and the resin); the sliced
  plate can be exported and the new one cannot; an edit leaves the slice out of date, no
  longer counted (0 of 2 sliced); All plates… closes it again.
- The cards side by side: as many as fit, one at least.
- Window: a new plate after a deletion takes a free name, and skips a name already taken; an
  empty plate is deleted at once; a plate with a model waits for the answer, and Escape keeps
  it.

**Validation (phase 3):**
- Project: a move between two plates is one step at new revisions for both; an unknown plate
  changes nothing; the target edited after the move is undone first, then the move; redo
  replays the move, the target's edit when that plate is shown; a copy to a new plate is undone
  whole; undone and then edited, the move is not redone.
- Workspace: a copy onto a plate whose cube is in the same place lands beside it, clear by the
  spacing and on the plate, its pose kept but no longer centred; a move to a new plate keeps its
  place, the plate shown stays, and undo brings it back; a model wider than the free room
  stays, with nothing changed; the plate shown is no target. A footprint reaches a support's
  pad on the plate 20 mm beyond the model.
- Window: a copy keeps the selection, a move clears it; the notification names the model and
  the plate.
- Drop (owner's trial, 2026-11-06: models dropped on the bar did nothing): egui's
  `dnd_release_payload` takes whatever is carried and drops it when it is of another type, so a
  card's check for a dropped plate threw a dropped model away. Each check now takes only its
  own type; a model released on a card that first checks for a plate still reaches the model's
  check (and fails without the guard).

- **Native Windows (phases 1 to 3):** validated on 2026-11-06 with 599a9c6 (`validate.ps1
  -IncludeApp`, a clean snapshot of 680 files): no failed command; Debug and Release each pass
  1,624 tests with 17 ignored; the three application smoke runs pass (Vulkan 12.3 ms, OpenGL
  14.1 ms, FX-03 16.5 ms mean).

**Validation (phases 4 and 5):**
- Project: two new plates and the arranged contents are one step, undone and redone whole.
- Workspace: six 90 × 50 mm slabs and one 250 × 50 mm on a Photon Mono X: four arranged on the
  plate inside its area and the spacing apart, two to "Arms (2)", the huge one left; the plate
  shown stays; undo restores the seven; with Stay, four arranged and three left.
- Window: Arrange opens with the plate's spacing and draws its title and choices; the
  notification says how many models and that Ctrl+Z undoes it; the choice is a kept setting;
  an empty plate cannot be arranged. Slice every plate slices both plates, the one not shown
  too, with the status line, then says there is nothing left; Export every plate slices the
  edited plate first and writes both files, asks before replacing them, and, the switch off,
  leaves out the plate out of date and lists it. A batch stopped cancels its slice, without the
  slice's own message, and counts the plates not reached. The dialog draws its fields, both
  plates' files and states and "Export 2 plates". File names follow the pattern and are safe.
- Regressions: a plate holding GOO for a printer that does not write it is sliced in the
  printer's format and its file named so; two plates' slicings in one folder keep their own
  files, a slicing made afterwards empties nothing, and each let go removes its own file only;
  only the folders of sessions that ended, one without a lock and loose files are removed, a
  folder whose lock is held stays; a new project keeps no slicing of the last one's plates and
  its file is gone (the test fails without the reset: the old plate 1's slice became the new
  plate 1's).
- **Native Windows (phases 4 and 5):** the run of 838563d (`validate.ps1 -IncludeApp`, 694
  files) failed one command: in Debug, a support routing test panicked as its model did not
  open, for three tests shared the ledge fixture's directory and each removed it on ending
  (Release passed 1,656 tests, the smoke runs passed). 779cc48 gives each call its own
  directory. Its run (2026-11-07, a clean snapshot of 694 files): no failed command; Debug and
  Release each pass 1,656 tests with 18 ignored (Linux 1,658: the Unix-only path tests); the
  three application smoke runs pass (Vulkan 13.7 ms, OpenGL 12.9 ms, FX-03 16.4 ms mean).

## TD-136 — Support presets and the outline's tips from the test prints

**Status:** Accepted on 2026-11-06 by the owner, from the support preset review
(`docs/research/SUPPORT-PRESETS-RESEARCH.md`: published guidance, the venom torso v4 print
E-SUP-01, and the two test plates E-SUP-02 and E-SUP-03). Implemented the same day.

**Context:**
- The venom torso v4 printed well, but its Heavy supports, braced and branching, were hard to
  remove (E-SUP-01). The presets were chosen within published ranges, not proven by a print.
- The first test plate (18 flat tiles, one combination each) held only tiles whose contacts
  covered 2.8 % or more of their area; every tile curled at its corners, where no tip reached
  (the outer tips 1.25 mm in). Heavy's stiff 1.4 mm tip made its supports hardest to remove,
  and braced and branching, impossible.
- The second plate (20 tiles, ten combinations twice, every outer tip 0.4 mm in from the edges
  and the corners) held every tile, down to 2.1 % of the area: the edges, not the contact area,
  decided the hold. The flat tiles stayed flat; 4.7 mm between tips let a 2 mm tile sag; depth
  0.1 or 0.2 mm made no difference to removal; Heavy's contact on a 1.0 mm tip scored 1.5 and
  2, on its own 1.4 mm tip 3 and 2.5. Tiles tilted by 30° bent at their lowest edge, which
  prints first, though tips stood 0.2 mm in from it.
- karasu measures a cone contact's diameter at its end inside the model; its mark at the
  surface is wider (measured on the first plate's layers).
- The piercing check started only past twice the anchor tolerance (0.1 mm), so a contact sunk
  exactly 0.1 mm was never checked, though the specification's segment is not empty there.

**Decision (owner, 2026-11-06):**
1. **The presets** (`Preset::parameters`): every contact sunk 0.1 mm; Light 0.3 mm on its
   0.8 mm tip and shaft; Medium 0.5 mm (0.4 before) on its 1.0 mm tip and shaft; Heavy 0.6 mm
   on a 1.0 mm tip 2.5 mm long and a 1.2 mm shaft (owner: 0.2 mm between the presets' shafts).
   Feet, pads, cones and mini supports keep their values. Saved projects are unchanged: a
   document writes every support's complete parameters (TD-130).
2. **The contact's width at the surface** (`SupportParameters::surface_width_mm`) is shown in
   the support panel beside the contact ("At the surface"): Light 0.33, Medium 0.52, Heavy
   0.62 mm. The diameter keeps its meaning, so documents read as before.
3. **Braces** (owner): a new brace's diameter defaults to 1.0 mm (0.8 before); an automatic
   brace is the thinner of its two shafts, at most that diameter (`Brace::max_diameter_mm`), so
   two Heavy shafts are braced at 1.0 mm, not 1.2 mm. A document without it builds its
   automatic braces as before.
4. **The Automatic tool** starts from Medium (Light before) and without braces (on before);
   the tip spacing stays 2.5 mm. Settings schema 4 turns the old defaults to the new ones; a
   Light preset or braces chosen before cannot be told from them and are turned too.
5. **Tips along the outline** (`auto_ui::plan::edges`): where an underside ends (nothing beyond
   it, a drain hole, a step over 0.5 mm or a turn over 30°), found by bisection between the
   probe grid's points, its outline takes candidates 0.4 mm in from the edge along the grid's
   axes, and from both edges at a corner. Corners are taken first, then edges, then the inside.
   A smooth surface growing steeper than the critical angle has no edge.
6. **A leading edge is held from below and from its side** (owner: an edge standing on
   supports is also held on one side, and its tips go close to it, as supports placed by hand
   are): where the underside rises away from an edge by more than 10°, its edge tips stand the
   contact's radius at the surface in from it along the slope (Medium 0.26 mm), so that their
   marks reach the edge; the face beside it, found outward up to 2 mm, rising up to 3 mm and no
   steeper than 75°, takes side tips the same radius up from the edge where the face is at
   least the contact's width. Side tips go between the tips taken, half their reach from them
   and their reach from each other, so that the two alternate along the edge as Lychee's do
   (E-SUP-04: 0.11 mm under, 0.21 mm up the face); one that finds no support is a place left
   unsupported.
7. **The piercing check** runs whenever its segment is not empty (`depth > τ`).

**Alternatives rejected:**
- **A contact-area threshold** (2.8 %, from the first plate): the second plate held 2.1 % with
  tips at the edges.
- **Redefining the contact diameter at the surface:** it would change what every saved project
  builds; showing the width is enough.
- **Heavy's tip as thick as its 1.2 mm shaft:** untested, and the stiff tip was what made Heavy
  hard to remove; its tip keeps the tested 1.0 mm. A shaft changed in the panel's basic view
  takes the tip with it only where they were alike.
- **Braces by default:** no test tile needed them, and on the first plate they made Heavy
  irremovable.

**Consequences:**
- The automatic tool plans more tips near the edges: a corner and an edge tip every spacing
  along each sharp edge, and side tips along leading edges.
- Medium and Heavy contacts are shallower; on a wall thinner than about 0.15 mm the piercing
  check now reports them.
- **Measured** with the Automatic tool covering everything, as its local-model test makes the
  plan (Linux release, 16 threads), before (Light with braces, the old defaults) and after
  (Medium without braces, the edges' tips):
  - `HillGiant_32mm.stl` (500,510 triangles, lifted 5 mm): 883 supports (339 standing, 92
    mini) instead of 835 (292, 89), no braces instead of 39, all 12,753 overhang points covered
    instead of 8,563 of 8,566, no place left instead of 2; 2.92 s instead of 2.89 s;
  - the owner's `obj_1_quant_1.stl` (1,864,166 triangles; turned -8°, -21.4°, 1.5°; scaled
    200 %): 419 supports (217 standing, 72 mini) instead of 422 (203, 75), no braces instead of
    291, 6,020 of 6,022 overhang points instead of 4,031 of 4,032, 1 place left either way;
    6.74 s instead of 8.56 s;
  - the built supports have no finding either way; islands only, the plans hold every island
    (1.68 s instead of 2.00 s; 3.62 s instead of 5.21 s, which planned braces);
  - the two changes are measured together: the presets' and the edges' shares are not split.
    With the side tips in the middle of the face (the first version), HillGiant took 934
    supports in 3.33 s; the timings vary by a few tenths of a second between runs.
- Not shown by the prints: hollow parts, curved undersides, tall parts that need braces, other
  resins and exposures; Light on a flat area with edge tips; which edge inset is the least that
  holds.

## TD-137 — The contact's height line, the held height and the cavity's own supports

**Status:** Accepted on 2026-11-06 by the owner, from three proposals of the same day (the
third, supports on a grid, is TD-138). Implemented the same day.

**Context:**
- Supports placed by hand are often kept in a row at one height, along an edge or around a
  part; other slicers show a line around the model at the contact's height while one is placed
  (the owner's screenshot, Lychee 8.0), and the owner asked for a key to hold that height.
- A hollowed model's cavity is planned with the outside's preset and density. Its supports are
  not seen and hold a lighter part (S17 in the support presets research), and the Automatic
  tool already plans the cavity apart (Place), in two runs with values changed between them.

**Decision (owner, 2026-11-06):**
1. **The contact's height line:** while a support is placed or its tip dragged in the Supports
   tool, the models' shader draws a line around them at the height under the pointer: two
   values in the frame's uniforms (the height, its half width of 0.75 px, its colour), so a
   pointer move changes a uniform and rebuilds nothing. Its colour is a setting (dark by
   default, #24222A, to stand out on both model colours), and a switch hides it.
2. **Shift holds the height:** pressed while the line shows, the height stays. The surface's
   contour at that height (the edges of the triangles crossing it that a support can stand
   under) is found once, and each pointer move takes its point nearest the surface under the
   pointer, or off the model the one the screen shows nearest the pointer, the nearer to the
   eye among ties; a point under the pointer already on that height (a level face there) is
   kept. A section that hides the height leaves nowhere to go. Let go, the tip is free again.
3. **The cavity's own values:** the Automatic tool's "Own values in the cavity" (off by
   default) gives a hollowed model's cavity its own preset and density, starting from the
   outside's when switched on. A plan of the cavity alone uses them; a plan of everywhere is
   the outside's plan and the cavity's joined, with braces planned over both. The cavity's user
   preset is kept with the settings by name, as the outside's is.

**Alternatives rejected:**
- **Snapping to the height by a typed value:** the line and Shift do it from the pointer, as
  the owner asked; a typed height can come later.
- **The line drawn as geometry (a contour mesh) each frame:** the shader draws it for nothing.
- **Splitting every value for the cavity:** the owner asked for the preset and the density.

**Consequences:**
- **Measured** (Linux release) on the owner's `obj_1_quant_1.stl` (1,864,166 triangles; turned
  -8°, -21.4°, 1.5°; scaled 200 %): the contour at a quarter, half and three quarters of its
  height takes 8.0 to 9.0 ms, once when Shift is pressed (1,144 to 2,600 edges), and each
  pointer move then 0.03 to 0.15 ms.
- With the cavity's own values, a plan of everywhere probes the regions twice, once for each
  place.
- The frame's uniform block grows from nine vectors to eleven.

**Validation:**
- Viewport: the line's uniforms (height, switch, half width, colour; none off), the frame's
  block of eleven vectors; the shader parses and validates.
- Height: a box's contour at a height is its four walls, nothing above it, at a level face's
  height or where left out; the point nearest the pointer is on the front wall seen from the
  front, on the right wall aimed beside it, the front wall's below the surface under the
  pointer, and none behind the eye. On a lifted cube: no height held until the pointer is on
  the model; held at the bottom, the pointer on the front wall 3 mm higher puts the tip on the
  wall at the bottom's height under the pointer; the bottom elsewhere keeps its own point; a
  section hiding the height leaves nowhere; let go, free; placing stopped, no line.
- Automatic: on a hollowed cube, off plans the cavity with Medium as the outside; on, with
  Light at 50 %, the outside's supports are the same and the cavity's are Light and fewer, every
  overhang point covered; the cavity alone takes the cavity's values and the outside alone the
  outside's.
- Settings: the cavity's values, its preset by name, the line's colour and switch come back as
  they were set.

## TD-138 — Supports on a grid

**Status:** Accepted on 2026-11-06 by the owner from the mockup "Supports on a grid" (canvas
"Karasu Layout Mockup"). Implemented the same day, at the owner's go.

**Context:**
- A flat underside, such as a statue's base printed tilted, is held best by regular rows of
  supports at a spacing the user sets, easy to remove; the Automatic tool places tips freely.
- The test prints (TD-136) showed that a grid alone, its outer tips 1.25 mm in, lets the
  corners curl and the part fall: the outline needs tips of its own.

**Decision (owner, 2026-11-06):**
1. **Where:** a Grid mode in the Supports tool's strip, beside placing (its key is B: G is the
   Braces tool's automatic braces, and a key has one action; it can be changed in Settings).
2. **What is gridded:** a click on the model picks a flat underside: the faces joined to the
   one clicked that turn from it by less than "Flat within" (5°, under Advanced). **A rectangle
   or a lasso** (owner, the same day: the Grid button's options, as other tools' buttons have
   theirs) dragged on the screen picks instead the triangles facing down that it touches, seen
   through the model, and its tips are kept to the shape; a border of the shape across a face
   that goes on is no edge. No lift of its own (owner: not needed).
3. **The grid** lies in the picked face's plane, so a tilted base is gridded as a level one:
   spacing along the rows and between them; the rows along the face's longest edge by default
   (to be tried in print), or at an angle typed.
4. **Edges first** (TD-136): the outline takes its corner and edge tips the inset in (0.4 mm,
   under Advanced, with a switch), a tilted face's lowest edge its leading-edge tips; the grid
   fills the inside, leaving out a grid point nearer an edge tip than half the spacing.
5. **The supports** are made with the preset chosen and go straight down to the plate, as
   placing makes them (feet on the model and mini supports as placing does); Apply adds them
   as one undoable step, Discard drops them, and picking another face replaces the preview.
6. **Branches, as an option** (owner, the same day): a "Branches" switch, off by default, with
   the branch distance and the branches per trunk (their defaults in Settings › Supports): the
   grid's tips then share trunks as the Automatic tool grows them; a tip whose branch finds no
   way gets a support of its own. Off by default until a print shows branches holding with
   the outline's tips (the first test plate's branching tiles fell, without them).

**Alternatives rejected:**
- **A grid without the outline's tips:** what failed on the first test plate.
- **Branches always:** straight to the plate stays the default; branches are a choice.

**Implementation (`grid_ui`):**
- **The shapes** (`plan::select`, `plan::inside_polygon`): while the option is Rectangle or
  Lasso, a primary drag draws the shape (the view does not turn; Escape drops it), drawn over
  the viewport as a line; released, it is kept with the eye it was drawn through and the
  model's frame then. A triangle facing down is touched when a corner or its centre lies in the
  shape, or the shape's first point in it; the face is those triangles, its tips those whose
  point, carried back to where the model stood, the shape holds. Once planned the shape is kept
  as its triangles, so a value changed or the model moved plans them again; a surface prepared
  again (another triangle count) asks for the shape again. The option is kept with the
  settings (`grid_pick`).
- **Edges:** a border is an edge where no triangle lies beside it or the one beside turns from
  the face by more than 30° (as TD-136's outline), for every pick; only edges take corner and
  edge tips and keep the grid away.
- **The face** (`plan::pick`): the clicked point is kept in its model's frame; the triangle
  under it on the surface for routes is found by a short vertical probe, then every triangle
  of the same side (outside or cavity) turned from it by less than "Flat within" and joined to
  it edge to edge. One pass over the surface's triangles, on the plan's thread.
- **The tips** (`plan::tips`): the frame's rows along the direction the outline's length runs
  most (1° bins, a 5° window), or the angle typed; the outline's loops cut at corners turning
  more than 30°; corner tips the inset in from both sides, side tips every spacing along each
  side; a side the face rises away from by more than 10° (a leading edge) takes its tips the
  contact's radius in and, between each two, a side tip on the triangle beside it (facing down
  no steeper than 75°) the radius from the edge; the grid centred on the face, each point on
  the face, clear of the outline by the inset (the radius with the edges off) and of its tips by
  half the smaller spacing.
- **The supports** (`auto_ui::plan::plan_tips`): the tips grown as the Automatic tool grows its
  own (no merging of trunks, no braces), placed supports (`SupportOrigin::Placed`), with the
  Automatic tool's defaults for feet on the model and mini supports; then built against the
  model, those with an error going; a tip left without a support is counted as left.
- **Values:** in the settings (`grid`, `grid_preset` by name), the tools' values starting from
  them; a value changed in the panel, or the plate's revision, plans the picked face again.

**Consequences:**
- Branches need room under the face: on a face less than about 8 mm above the plate, the
  tips that cannot join a trunk take supports of their own (the test slab at 6 mm took none).
- **The surface for routes is asked for by the tool panel** each frame (the owner's trial,
  2026-11-06: a click sometimes planned nothing until placing was switched on and off): the
  panel asked for it for the Islands and Automatic tools only, after the Grid mode's own
  request, so the grid waited for a surface that only placing, or one already made, gave. The
  panel now asks for it while the Grid mode is on; the window test runs the panel each frame.
- **The Grid mode does not lift the model** (the Automatic tool's Generate does, TD-128): a face
  resting on the plate, or too close to it, is picked but grows no support (the owner's trial,
  2026-11-06: `HillGiant_32mm_Base.stl` on the plate, 523 tips all too low). Such a plan says so
  in a notification with the most common reason (too low: lift the model), not only in the
  footer.
- **Measured** (Linux release, `a_local_model_s_base_is_gridded`, the widest level triangle
  facing down clicked, the built-in values), all on the plan's thread:
  - `HillGiant_32mm.stl` (500,510 triangles): a face of 18 triangles (2 mm², tilted 4.7°)
    picked in 5.5 ms; 4 tips in 19 ms; 4 supports grown and checked in 58 ms;
  - the owner's `obj_1_quant_1.stl` (1,864,166 triangles): a face of 944 triangles (50 mm²)
    picked in 21 ms; 83 tips (7 corners, 74 along its edges, 2 on the grid) in 96 ms; 83
    supports grown and checked, none left, in 217 ms;
  - the face and the side tips each scan the surface's triangles once: on these models, under
    0.1 s; neither model has a wide flat base, so a statue's base is still to be timed.

**Validation:**
- Plan: a level bottom is picked whole (two triangles, 200 mm²), a top not at all, and only
  triangles kept by the side; the bottom takes four corners the inset in from two edges, edge
  tips the inset in from one, and a grid in rows along its longest edge, clear of the outline
  and of its tips; without edges, no outline tips and a grid the radius clear; rows at 45°
  neighbour at 45°; a face tilted 30° takes its lowest edge's tips the radius up the slope and
  side tips the radius up the face beside it, at 60°; values out of range are the built-in ones.
- Shapes: a rectangle over a slab's bottom seen from below plans it as a click does and is
  kept as its triangles, planned again when a value changes; a shape too small, or a click,
  picks nothing; a lasso round the bottom's left half plans fewer supports, every tip inside
  it, none along its border across the bottom; on synthetic slabs the screen selection keeps
  the triangles facing down the shape touches, and one of a bottom's two triangles takes edge
  tips on its two outer sides only and one corner.
- Mode: a slab resting on the plate is picked, plans no support, and a warning says why;
  off, a click is not the mode's; a top is said and not picked; the bottom of a slab
  lifted 15 mm is planned (four corners, no sides, every tip a support, none left, no
  branches); wider spacings plan fewer; branches join some tips; Apply adds them all as one
  step, placed, with a success notification, and nothing is planned again until another
  click; Undo removes them; Discard and the mode off drop the face.

- **Native Windows (TD-136 to TD-138):** validated on 2026-11-07 with 975ffe5 (`validate.ps1
  -IncludeApp`, a clean snapshot of 692 files): no failed command; Debug and Release each pass
  1,645 tests with 18 ignored (Linux 1,647: the Unix-only path tests); the three application
  smoke runs pass (Vulkan 12.5 ms, OpenGL 12.9 ms, FX-03 16.5 ms mean).

## TD-139 — Exporting models as STL, 3MF and OBJ, their drain holes cut

**Status:** Accepted on 2026-11-07, with the owner's answers of the same day; mockup "karasu
model export mockup". Implemented on 2026-11-09 (steps 1 to 4 below). It waited for general
mesh Booleans (owner, 2026-11-07): an export without
the drain holes is of no use, so none ships before the holes can be cut, and the cut is to be
the first use of a general Boolean kernel, which cutting, joining and lettering models will
reuse. The cut is implemented on that kernel (TD-140 phase 4, 2026-11-07:
`HoleCuts::drill`), validated against slicing; the writers, the assembly and the dialog (steps 3
and 4 below) are next. The drilled mesh comes out on the kernel's grid, rounding's crossings
resolved (TD-140 phase 5, adopted by the owner on 2026-11-09): every format stores it exactly,
and what the rounds could not resolve is reported, never hidden. The writers are in the core
(`export::models`), the assembly in `karasu-project` (`pipeline::export_models`, and
`jobs::ModelExportJob` off the interface thread), the command line's `export-models`, and the
dialog in the application, all on 2026-11-09 (see "As implemented" below).

**Context:**
- The ROADMAP (owner, 2026-11-04) asks for model export after the project file: single models,
  a plate or several plates, as STL or 3MF. The core reads STL, OBJ and 3MF (TD-057) and writes
  only print formats; its ZIP writer (`print_job::zip`, deterministic, deflated, TD-133) can
  frame a 3MF package.
- A prepared model is not one mesh. The hollowed shell is a mesh (the placed exterior and the
  reversed cavity, TD-045), and the supports and the raft are closed shells that overlap the
  model, united by positive winding (TD-062). Drain holes are not in any mesh: certified
  polygonal prisms (TD-047; entry cap in the exterior, end cap in the cavity, both clear of
  every surface; prisms clear of each other) whose material is removed layer by layer.
  Exporting a drilled model therefore needs a mesh difference, which karasu does not have.

**Decision (owner's answers, 2026-11-07):**
1. **Content:** every model as it stands on the plate, in millimetres (turned, scaled,
   mirrored, hollowed), its drain holes cut. **Supports** and **Raft** are switches in the
   dialog, both on by default.
2. **Drain holes are cut into the mesh now**, not left out: no export writes a hollowed model
   without the holes it is printed with.
3. **Scope and files:** the model chosen (from its row's menu), the plate shown, or every
   plate; one file per plate or one file per model, chosen in the dialog. With one file per
   model each model takes its own supports; the raft, shared by a plate's models, goes only
   with one file per plate.
4. **Formats:** binary STL, 3MF and OBJ. STL and OBJ hold one solid: the model, supports and
   raft concatenated (overlapping closed shells, as other slicers export them). 3MF holds each
   model, its supports and the raft as objects of their own (type `model`, so a slicer reading
   them prints them), each a build item, unit millimetre.
5. **The dialog** (mockup): Export (the model, the plate, every plate), Format, Files, the two
   switches and a line saying what the file holds. One file: Export… asks where, offering the
   project's and plate's name. Several: a folder, a name pattern (`{project}`, `{number}`,
   `{plate}`, `{model}`) and a table of the files with their state, as Export every plate
   (TD-135): never replacing a file without asking, each file written whole or not at all,
   off the interface thread, with progress and Cancel. Entry points: File › Export models…
   and a model row's Export model…; each an action (TD-097). The choices are kept settings.
6. **The command line** gains the same export (`karasu-cli export-models`), the processing
   being headless (`karasu-project`).

**The drain-hole cut (first design, before the decision for a general kernel):** the result
is the solid `M \ ∪ C`,
`M` the shell's positive-winding solid and `C` the certified prisms.
- **What crosses:** the certificate keeps both caps clear of every surface, so only the
  prisms' side faces meet the shell. For each prism, the shell triangles near it come from a
  BVH query; each is clipped against the infinite prism by its side planes (a convex polygon,
  decided with exact orientation predicates on the input coordinates); a piece inside the
  prism and between its caps is removed, the triangle minus that convex polygon is
  triangulated, and a piece beyond a cap (in the cavity's far wall or outside) is kept.
- **The hole's wall:** each side face keeps the part of it inside `M`, reversed. Its boundary
  is the segments the clipped triangles leave on that face's plane and the lateral edges'
  intervals inside `M`, found from the entry cap (outside `M`) by the signed crossings of the
  shell along the edge (the winding number). The face's region is a planar polygon set,
  triangulated in the face's plane.
- **No cracks by construction:** every new vertex is symbolic and computed once: a shell edge
  meeting a side plane (shared by the edge's two triangles), or a lateral edge piercing a
  shell triangle (shared by the two side faces at that edge). Topology is decided exactly;
  only these coordinates are rounded.
- **Refusals, never repairs:** a degenerate configuration the exact tests cannot settle (a
  shell vertex on a side plane, a lateral edge through a shell edge, overlapping shells
  crossing inside a hole's wall) refuses the export of that model with the hole named, rather
  than writing a doubtful mesh. Simulation of simplicity, as the sampled field uses, is the
  first remedy to try for the vertex-on-plane case.
- **Validation:** the output is closed and edge-manifold where the input was, and consistently
  oriented (the core's diagnostics); its volume is the shell's minus each prism's part inside
  it; and slicing the exported mesh gives the same layers, pixel for pixel but for the
  boundary pixels, as slicing the model with its cutters (the drilled model as printed).
  Property tests over random hole placements on spheres, cubes and a sampled hollow; fuzzing of
  the clip and the face triangulation; the measured cost on FX models with several holes.

**Hollowing needs no Boolean:** the certified cavity lies wholly inside the source, clear of
it by the minimum wall (TD-093), so the shell is two disjoint closed surfaces written one after
the other. Overlapping bodies, and supports entering the model, are united by positive
winding, as other resin slicers read them; a general kernel can also write their true union.

**Order (owner, 2026-11-07):**
1. The general mesh Boolean kernel: research, its own decision, then the core (a separate
   decision record).
2. The drain-hole cut on that kernel, with the validation above.
3. Writers in the core (binary STL, OBJ, 3MF on the ZIP writer), each read back by karasu's
   importers in tests; the export's assembly in `karasu-project` and the command line.
4. The dialog and the menus in the application, from the approved mockup. No export ships
   before step 2.

**Cost and memory:** the cut touches only the triangles near each hole (a BVH query per
prism), so it costs little beside hollowing; the writers stream triangles and hold no second
copy of a mesh, except 3MF's XML, deflated member by member.

**As implemented (2026-11-09).**
- **Writers** (`karasu_core::export::models`): binary STL, 3MF (the ZIP writer, deflated) and
  OBJ, for named bodies, each a closed mesh moved by its instance's offset.
  - Every format stores coordinates as `f32`. 3MF and OBJ write each value as the shortest
    decimal of that `f32` taken as an `f64`, so readers parsing either width get it exactly.
  - The same bodies give the same bytes.
  - A file is written to a hidden staging file and published whole, never over an existing
    file. A replacement the user confirmed renames the complete staging file over the old one,
    so a failure still leaves the old file.
  - 3MF requires two triangles per edge by vertex index (Core §4.1). Where the union left four
    faces on an edge (bodies of a sculpt touching along a line), the faces are paired by their
    exact angle about the edge, each pair bounding a wedge of solid, and each sheet gets its
    own vertex copies. Where the sheets still meet around both ends of the edge, the other
    pairing (wedges of empty space) is used.
  - On the owner's torso the 91 such edges all came out used twice: 3,475,404 edges, each by
    two triangles. Any left are counted in the file's summary.
- **Assembly** (`karasu_project::pipeline::export_models`):
  - Objects are prepared exactly as slicing prepares them and arranged the same way.
  - Each instance's XY move is rounded to the kernel's grid, so a drilled model stays exact in
    the file (at most 2⁻¹⁶ mm from where slicing places it).
  - An object with holes is drilled once, whatever its copies.
  - Its supports and raft are separated from the supported mesh (the raft by its parts).
  - One file per plate holds every chosen object's instances; one file per model holds one
    object's.
  - A file that fails (an existing file, a model whose holes cannot be cut) is reported in its
    own result while the others are written. A cancellation stops the export.
  - **The raft goes with its model** in a file per model too. Decision 3 assumed a raft shared
    by the plate, but each model's supports build their own raft.
- **Command line:** `karasu-cli export-models --project <file> --format stl|3mf|obj`.
  - `--project-plate` or `--every-plate`; `--model <n>`, `--per-model`.
  - `--no-supports`, `--no-raft`, `--replace`.
  - `--output <file>` for one file, or `--folder <folder> --names <pattern>` for several.
  - Two files with the same name are refused before anything is written.
  - The command reports each file and what drilling left; exit status 1 when a file was not
    written.
- **Application:** File › Export models… and a model row's Export model…, each an action
  (TD-097).
  - The dialog (export, format, files, the two switches, the content line) follows the mockup.
  - A single file asks where, offering the project's and the plate's (or model's) name.
  - Several files go into a folder, named by the pattern, with a table of each file's state.
    The user is asked before any file is replaced.
  - The export runs off the interface thread, with its step and Cancel in the footer, and a
    notification ends it: the files, each drilled model's report, and the failures.
  - The choices are kept settings.
- **Files other programs read** (the owner's trial, 2026-11-09):
  - **Chitubox** places any 3MF wrong, ours or another program's: tiny, and multi-object files
    scattered. It is not a reference for 3MF.
  - **Lychee** rests each 3MF object on the plate on its own, so the model's supports, as an
    object of their own, moved away from it. One object per model reads right.
  - **Bambu Studio** reads every variant right; its notice of "an invalid configuration" comes
    from a 3MF without its own project files and is harmless.
  - Neither number format (17 or 9 significant digits), local coordinates with item transforms,
    nor components changed what Lychee or Chitubox did.
- **Decision 4 changed (owner, 2026-11-09): objects as an option.** 3MF and OBJ hold each model
  with its supports and raft as one object by default, or, chosen in the dialog
  ("Objects: one per model | model, supports and raft apart") or with `--separate-objects`, the
  three apart. STL is one solid either way.
- **Supports made fit for a file** (`export::models::tidy_shells`), found comparing the torso's
  file with Lychee's own:
  - **Repeated parts.** 1,143,184 of the supports' 2,853,336 faces were repeated parts: a joint
    sphere for every segment and brace meeting at a node, up to eight at one point. Each identical
    shell is now written once.
  - **Caps back to back.** Two consecutive segments' caps facing each other are both left out.
  - **Comparison.** Shells are compared as the file stores them (`f32`): one point reached two
    ways differed by units in the last place of `f64`.
  - **Joint spheres** are turned half a side, so their rings never share a vertex with a shaft's
    (`supports::shapes::sphere`).
  - **Result.** The supports went to 1,692,792 faces, no duplicate, from 934,978 edges of four
    faces to 8,378. Bambu Studio reports nothing for them.
- **The owner's trial after these fixes** (2026-11-09):
  - **Bambu Studio** opens the torso's STL and 3MF with no error, the model alone and the
    supports alone too.
  - **Lychee** opens the drilled model with no error. It still reports "structural issues" for
    our supports and cannot repair them ("too complex topology"), though its own supports have
    edges of four faces and duplicates of the same kind.
  - **What remains** in our supports: 8,378 edges where two closed parts touch along a ring of
    vertices (a cone standing on its pad). Read by positive winding, as every slicer reads
    overlapping shells, they print as one solid.
  - **Accepted by the owner for now.** If Lychee's acceptance is wanted later, uniting each
    support's parts with the kernel would leave one shell per support, with no contact and no
    inner faces; its cost on export is to be measured.
- **Native Windows** (2026-11-09, `validate.ps1 -IncludeApp`, a clean snapshot of 7dddcf2, 713
  files): no failed command; Debug and Release each pass 1,718 tests with 23 ignored (Linux
  1,720: the Unix-only path tests); the three application smoke runs pass (Vulkan 13.7 ms,
  OpenGL 13.5 ms, FX-03 16.3 ms mean). With TD-140's phases, from 99a79c6 on.
- **Measured** on the owner's torso (2.3 million triangles, 13 holes, supports and raft;
  release, 16 threads): 5,170,412 triangles in three bodies.
  - STL: 77.5 s, 259 MB.
  - 3MF: 85.4 s, 61 MB.
  - Peak resident memory 3.2 GB.
- **Tests:**
  - every format read back by karasu's importers;
  - a 3MF where four faces share an edge, split into its two cubes;
  - a confirmed replacement;
  - the assembly per plate and per model, with or without supports and raft, one model alone,
    an existing file, a cancellation, file names;
  - the job, cancelled too;
  - the command line's options;
  - the dialog: drawn, a folder export, asked before replacing, a model's single file.

## TD-140 — A general mesh Boolean kernel in the core

**Status:** Accepted on 2026-11-07 by the owner (design A), from published research (owner,
2026-11-07: general Booleans first, the drain holes their first use). The grid was 2⁻¹² mm, and
is 2⁻¹⁵ mm since phase 5. Fragments are classified by exact tracing from reference points, never
by propagating winding numbers radially around intersection curves. What a file for other slicers does when rounding left a crossing is decided
with the export (TD-139). Phase 1 started the same day; phases 1 to 4 are implemented
(2026-11-07). The output was written in `f64` rather than on the grid (phase 4; owner,
2026-11-07). Memory, threads and cancellation, left open at phase 4, done on 2026-11-09. Phase 5
(owner, 2026-11-09): the output on a grid of 2⁻¹⁵ mm, rounding's crossings resolved by rounds of
the kernel itself (Valque and Lazard), for model files.

**Context:**
- Model export needs the drain holes cut into the mesh (TD-139); cutting a model, joining two
  and lettering will need the same operations. Slicing needs none (winding union and per-layer
  hole subtraction stay as they are).
- karasu has exact `orient3d` for every `f64`, exact contact tests, exact ray winding with
  simulation of simplicity, a deterministic BVH and the certificate pattern; it has no 3D
  intersection construction and no exact 3D point.
- The published methods split into rational arrangements (exact, slow), exact predicates on
  implicit points over `f64` inputs (Cherchi, Attene, Lévy), localized integer plane-based
  arrangements (EMBER, the fastest exact one reported) and consistent floating point (Manifold,
  not exact). Rounding an exact result to floats can create tiny self-intersections; the
  literature keeps results exact through chains and rounds once.

**Proposal (design A of the research):**
1. **Semantics:** solids are PWN meshes (closed, oriented, overlaps and self-intersections
   allowed), inside where the winding number is positive. An operation is an indicator on the
   vector of winding numbers, one per input, so union, difference, intersection and n-ary
   combinations (a model with all its supports and raft in one pass) are one algorithm.
2. **Representation:** every input vertex snapped once to a fixed global grid of 2⁻¹² mm
   (0.24 µm; displacement at most 0.12 µm, stated in the result); coordinates within the admitted
   ±10,000 mm are integers below 2²⁶. Polygons are planes with integer coefficients, vertices
   homogeneous intersections of three planes; fixed-width signed integer arithmetic (at most
   about 256 bits, the bounds proved for each primitive), no heap, no floating point in any
   decision.
3. **Algorithm:** a localized arrangement: an adaptive subdivision of the region where inputs
   meet (the BVH's pairs), a reference point per region with its winding vector (found with the
   exact ray winding, then traced exactly), polygons split by local BSPs and classified, regions
   that cannot change the result skipped. Parallel over independent regions; the output sorted
   canonically, so it is the same for any thread count.
4. **Output:** a conforming triangle mesh (exact vertex identity, T-junctions resolved, convex
   fans), rounded to `f64` on the grid; closed and consistently oriented by construction. An
   exact self-intersection check reports any crossing the rounding made.
5. **Refusals, never repairs:** an input that is not closed and oriented, coordinates outside
   the range, and any limit of work or memory exceeded are structured errors naming the input and
   where; nothing is silently mended.
6. **Validation:** exact volume identities as property tests; a slow rational oracle on small
   inputs; slicing equivalence for the drain holes; closedness, orientation and determinism
   checks; fuzz targets for the arithmetic, the splitting and the output; measurements on the FX
   models (hollowed, millions of triangles, several holes, hundreds of supports).

**Alternatives:** design B (exact predicates on implicit points over the untouched `f64`
inputs) keeps untouched vertices bit-identical, but needs a large set of hand-derived
predicates, heavier exact fallbacks and is reported about ten times slower; it remains the
fallback should design A's output stage fail. Floating-point methods are
not exact and mishandle overlapping inputs; rational ones are too slow except as a test oracle.

**Open decisions for the owner:**
1. Design A, and the grid of 2⁻¹² mm.
2. A file for other slicers: refused when the rounding left a self-intersection, or snapped
   within a bounded loop (Valque and Lazard), or written closed and oriented with the crossings
   reported. **Decided (owner, 2026-11-09): snapped within a bounded loop** (phase 5), what
   remains reported. Writing and reporting was decided first, the same day, on the count in
   `f64` (34 crossings on the torso); stored as `f32`, as binary STL must be, the torso had
   5,156 crossings and 382 collapsed triangles, and the owner chose to resolve them. The research
   on how other tools export is `docs/research/MODEL-EXPORT-RESEARCH.md`.

**Phases (each validated before the next):**
1. **Arithmetic:** the grid and its admission, fixed-width integers, planes, homogeneous points
   and the predicates, with proved bit bounds and exhaustive tests at the extremes.
2. **Arrangement:** the subdivision, the reference winding vectors, the local splitting and the
   classification, for n inputs and any indicator.
3. **Output:** the conforming triangle mesh, the rounding and its check.
4. **First use:** the drain holes cut into the hollowed shell (TD-139), validated against
   slicing; then the union of model, supports and raft for export.

**Cost:** measured from phase 2 on; the work is near the intersections only, so subtracting a
few holes from a hollowed shell of millions of triangles should cost a fraction of the
hollowing itself (the target set once phase 2 is measured).

**Phase 1, as implemented (2026-11-07):** the private module `karasu_core::boolean`, used only
by its tests until phase 2.
- **The grid** (`grid`): a point snapped to the nearest step of 2⁻¹² mm, ties to even; a
  coordinate not finite or beyond ±10,000 mm refused with its axis and value; grid coordinates of
  magnitude at most 40,960,000 < 2²⁶, exact in `f64` both ways.
- **The integers** (`int`): sign and magnitude in `L` 64-bit limbs, exact sums, differences,
  products and comparisons; a result that does not fit is a violated bound and panics.
- **Planes and points** (`plane`): a supporting plane through three grid points (normal below
  2⁵⁵, offset below 2⁸³) or an extrusion plane through two along a coordinate axis (the edge
  planes); a vertex as the exact meeting of three planes by Cramer's rule (`w` below 2¹⁶⁸,
  coordinates below 2¹⁹⁶, `w > 0`); the side of a plane a point lies on (below 2²⁵⁴), equality of
  two points (below 2³⁶⁴), a point's order against a grid coordinate or another point on an axis.
  The bounds are derived in the module's documentation.
- **Tests:** the integers against `i128` and by the ring laws far beyond it, and at their
  extremes (one beyond overflows); the grid's rounding, ties, range and errors; a plane holding
  its points and its sides agreeing with karasu's exact `orient3d` on 20,000 random cases; three
  planes meeting on all three, the same point in any order, parallel planes meeting nowhere; a
  vertex made from planes through a grid point being that point; every bound held at the corners
  of the admitted cube, and approached within a few bits (`w` above 150 bits, coordinates above
  175), so the bounds are tight.
- **Cost** (release, one core, 1,000,000 random cases): a meeting point 233 ns, a side 28 ns, an
  equality 11 ns.

**Phase 2, as implemented (2026-11-07):** the arrangement (`boolean::arrange`, with
`boolean::polygon` and `boolean::trace`), still used only by its tests.
- **Inputs and operation:** closed oriented triangle meshes on the grid, vertices welded by grid
  position; a point is in an input where its winding number is positive (overlapping bodies
  count once); the operation is any function of the memberships, so union, intersection,
  difference and several inputs at once are one algorithm. The result is a list of convex faces
  facing out of it.
- **Polygons** are a support, one plane per edge (a triangle's edges extruded along the axis its
  normal leans on most) and their exact vertices. A cut by another polygon's support or by a cell
  wall adds only that plane, so every vertex stays within phase 1's bounds however often a
  polygon is cut.
- **Contacts:** a deterministic BVH pairs overlapping boxes; a triangle is touched when it meets a
  triangle of another input, or one of its own input other than along shared vertices and edges.
  Quick exact tests on the grid integers (`i128` orientations; coplanar pairs decided in the
  plane by the same rules as the general test) settle most pairs; the general exact tests
  (`improper_contact`, `triangles_meet`) the rest, and agree with the quick ones on 40,000 random
  pairs. Untouched triangles have the same winding numbers all over each side: classified once,
  kept whole.
- **Cells:** halved at odd numbers of half-steps (no grid point and no input plane in a wall),
  touched triangles clipped, untouched ones whole in every half they reach; a leaf at 16 polygons
  or one step. A cell whose operation cannot change given the absent inputs' memberships (the
  difference outside the first input, the union inside one) is dropped.
- **Winding numbers** at each cell's perturbed corner, by walks along the axes from its parent's
  corner; a part's or an untouched triangle's by a walk to the centroid of three of its vertices
  (strictly inside it), then across every coplanar part covering that point. Coplanar parts
  covering the same region make one face. Nothing propagates winding numbers around intersection
  curves.
- **Filters:** a side or a crossing is first decided in floating point when its value exceeds
  `2⁻⁴⁰` of its terms' magnitudes (the error is below `2⁻⁴⁵`, derived in `trace`); otherwise in
  exact integers. A polygon's widened box skips walks that pass far from it.
- **Parallel and deterministic:** cells are halved level by level, in place, until there are 64
  per core, then each is finished on its own thread taking the next as it is free, and the faces
  joined in cell order: the same faces in the same order for any thread count (tested by
  finishing from one cell and from 4,096).
- **Tests:** an independent oracle (an upward ray through the inputs' triangles in plain
  integers) gives each input's membership at random points of eighths of a step; the faces'
  winding number there must be 1 inside the result and 0 outside: two overlapping boxes (union,
  intersection, difference), boxes sharing a face, the same box twice, coplanar faces partly
  overlapping, an input overlapping itself, 40 rounds of three random tetrahedra, and a hollow box
  drilled through its wall. Breaking the coplanar deduplication or the crossing's sign makes them
  fail.
- **Cost** (release, 16 threads, a hollow box of 540,048 triangles finely meshed, four square
  holes through its wall): 28.7 s first, then 17.6 s keeping untouched triangles whole, 10.8 s
  with the filters, 2.7 s with the quick contact tests, 2.0 s in parallel: 0.37 s making and
  welding the polygons, 0.71 s finding the contacts (still on one thread), 0.27 s the cells;
  540,585 faces.

**Phase 3, as implemented (2026-11-07):** the result as a closed mesh (`boolean::output`) and the
entry point `boolean::solve`, still used only by its tests.
- **`solve`** takes meshes in millimetres and a function of the memberships: each vertex is
  snapped to the grid (a coordinate beyond ±10,000 mm or not finite refused with its input and
  vertex), each input checked closed on its snapped, welded vertices (refused with the number of
  edges not used as often in each direction), then arranged and meshed. It reports the input
  vertices snapped, the result's vertices moved and triangles dropped or left flat by rounding,
  and the crossings rounding made.
- **One vertex per point:** the faces' vertices grouped by nearest grid point and compared
  exactly in each group; points made as grid points compare by their coordinates.
- **Cuts undone:** two pieces of one input triangle with the same side out and an edge between
  them (a cell wall, or a plane that crossed it where no surface does) are joined again while
  their union is convex; where the two shared a run of edges, the out-and-back is closed up, and
  a union that would repeat a vertex is refused. Whole untouched triangles are left as they are.
- **T-junctions:** the vertices of cut faces (the only ones that can lie inside another face's
  edge) are hashed in cells of a millimetre and coarse regions of 16 cells; each edge reaching a
  region is sampled every half cell, and each candidate inserted when it lies exactly on the
  edge's two planes, strictly between its ends (ordered along an axis where the ends differ
  exactly). Then a vertex on a straight side of every face holding it is left out of them all.
- **Triangles** by clipping strictly convex corners, those next to a vertex on a side first;
  every edge is then used once in each direction (the exact triangulation is checked closed).
- **Rounding** to the nearest grid point, ties to even, exact: a triangle whose corners round to
  fewer than three points is dropped (every edge stays balanced), a flat one counted. The pairs
  of triangles with a moved vertex that meet other than along shared vertices are counted exactly
  (the same quick integer tests, then the general one): what the export decides on (TD-139).
- **What remains of the cells:** a vertex where a cell wall crossed a cut and no convex union
  rejoined the pieces stays, rounded to the grid (a drilled 1 mm block: 18 such vertices and
  the four where the drill's diagonals cross its faces, 32 to 80 triangles where 32 would do).
- **Tests:** boxes combined into closed meshes whose membership the independent oracle confirms
  at every sample, coplanar faces partly overlapping, a hollow box drilled by a square hole and a
  slanted spike (closed; at most 3 of 600 samples differ, within rounding's reach), 40 rounds of
  random tetrahedra (closed; at most 1 % of samples within rounding's reach), exact rounding of
  points with ties, and `solve`'s refusals (an open input, a coordinate beyond the range), its
  snapping of an input a third of a step off the grid, and a drilled block with no crossing.
- **Cost** (release, 16 threads, the fine hollow box of 540,048 triangles drilled by four holes):
  arranged in 2.0 s, meshed in 0.9 s (0.26 s identifying points, 0.09 s rejoining, 0.41 s the
  T-junctions, 0.17 s flat vertices), 3.6 s from `solve`'s start to its end with the snapping,
  the closed checks and the crossing check: 540,808 triangles, 298 vertices moved, 2 dropped, no
  flat triangle, no crossing.

**Phase 4, as implemented (2026-11-07):** the first use, drain holes cut into a model's mesh
(`HoleCuts::drill` in `preparation::sampled`, public; `PreparedHoles::cuts` in `karasu-project`
gives an object's cuts). The result is the model's positive-winding solid minus the union of the
holes' polygonal cylinders (the cylinders slicing cuts), one closed mesh; supports and raft are
not cut and stay separate shells (TD-139, decision 4), so the union of model, supports and raft is
not needed for the export. Errors name the mesh concerned (the model or the holes): a vertex
beyond ±10,000 mm, a mesh that is not closed, cancellation, exhausted memory.
- **The output leaves the grid (owner, 2026-11-07).** The points the Boolean makes are
  written as `f64` coordinates, exactly where they are whole grid steps and otherwise within a few
  units in the last place; input vertices stay snapped to the grid. Rounding to the grid moved a
  point up to 2⁻¹³ mm, enough to fold the slivers where a slanted hole grazes a curved surface:
  a hollow sphere drilled by six holes had 67 crossing pairs and 16 dropped triangles on the grid,
  none in `f64`. A later Boolean snaps such a result again, as any input. The crossing check is
  exact on the `f64` coordinates.
- **Fewer vertices to round.** Faces in one plane facing one way are joined whatever input
  triangle they come from (two halves of a cylinder's side, a block's face), again after the
  T-junctions are resolved; a union that turns back on itself (a spike along a run of unmatched
  vertices) is refused. Then a point the Boolean made whose faces all lie in one plane, or in two
  planes not parallel either side of a straight line through it, is taken out and its polygon
  triangulated again in that plane (exact ear clipping): the surface is the same and the point,
  where a cell wall met a face around a hole, is never rounded. A block drilled through: 32
  triangles, none rounded (phase 3: 32 to 80, 4 to 23 moved).
- **Cells in two phases.** Halving now stops when three halvings in a row did not thin a cell (a
  hole's end cap fans out from one point, and its cells were halved to one step: 34,000 faces for
  a block and one cylinder). A cell is halved while it holds more than 16 touched parts or 1,024
  polygons; then its parts' faces are made, and it is halved again for its untouched triangles
  alone, while more than 64, the parts carried whole as obstacles for the walks and never cut
  again. Before, a dense surface cut each long touched triangle (a cylinder's side) into as many
  parts as there were cells: 900,000 parts from 13,202 touched triangles on the model below. The
  first cells, made before the parallel finish, take the same steps, so the faces are the same
  for any thread count.
- **T-junctions** are searched only on edges used more often one way than the other, among the
  vertices at their ends, paired through the deterministic BVH (a cluster of 22,000 vertices within
  62 µm of one point made the coarse grid's search scan 7.4 billion candidates). The joins use a
  persistent map of edges and a queue instead of repeated passes.
- **Tests** (`karasu-core/tests/drilled_models.rs`): a hollow sphere drilled by six holes (into the
  cavity, overlapping, through both walls, slanted, ending in the wall, beside the model) is
  closed, nothing dropped, flat or crossing, its volume reduced, and its layers (260 at 0.1 mm,
  pixels of 0.1 mm) are those of the shell sliced with the holes' cuts but for pixels on a
  boundary; a block takes a blind and a through hole, nothing rounded, its corners kept, the
  removed volume within 1 % of the cylinders', the same mesh twice; holes beside a model leave it
  as it was; an open model, a coordinate beyond the range and a cancellation are refused.
- **Measured on the owner's `venom_torso_2.karasu`** (`karasu-project/tests/drilled_project.rs`,
  ignored; release, 16 threads): the hollowed torso, 2,319,476 triangles, and its 13 holes cut in
  26.5 s (370 s at first): 2,345,954 triangles, closed, 27,025 points rounded, nothing dropped or
  flat. Its 1,873 layers at 0.05 mm (2,382 × 1,887 pixels of 0.05 mm) against the shell sliced
  with its cuts: 22,537 of 566,880,565 material pixels differ, all on a boundary but one, whose
  sample lies 0.06 µm from the model's surface. 34 crossing pairs and 33 non-manifold edges
  remain, all 26 mm or more from any hole, where the sculpt's own bodies intersect and the union
  makes nanometre slivers. Peak memory 5.3 GB with the preparation (the preparation alone about
  1.2 GB): every input triangle is held as an exact polygon. The fine hollow box of phase 3:
  2.69 s from start to end (arranged 1.53 s, meshed 0.68 s), 539,710 triangles, nothing rounded,
  0.98 GB.
- **Left for later** at phase 4: the memory of the exact polygons; the contact search and the
  crossing check on one thread; cancellation inside the parallel cells. What an export does with
  the crossings was decided with TD-139 (written and reported; owner, 2026-11-09).

**Memory, threads and cancellation (2026-11-09):** the three points left at phase 4.
- **Memory.** Every input triangle is kept as an `InputTriangle`: its support, its three edge
  planes and its grid corners, about 330 bytes and no allocation, where an exact polygon of
  three vertices took about 800; only the touched triangles become polygons to cut. A face that
  is a part is boxed, so the faces, mostly whole triangles, take a few bytes each. Each stage lets
  go of its input once the next holds what it needs: the snapped inputs after the arrangement,
  the arrangement once the points are identified, the exact triangulation once the `f64` mesh is
  made. The triangles are collected in place, never held twice.
- **Threads.** A BVH pair search on every core (`Bvh::pairs`): the tree's node pairs split level
  by level into 4,096 tasks, each searched as the serial search does, the pairs joined in task
  order, so the result depends on the tree alone. It finds the contacts, the T-junction
  candidates and the crossings. Faces in different planes never join, so the joining of cut
  faces runs plane by plane on every core, with the same joins as one queue over every face; the
  rings are triangulated in runs on every core, joined in ring order. Identifying the points (a
  hash of grid points, about 2 s on the torso) stays on one thread: computing the keys is 0.27 s
  of it, the rest is memory traffic, and a faster hasher saved nothing.
- **Cancellation.** A cancellable parallel fill (`parallel::fill_balanced_cancellable`): while
  the workers run, the caller's thread polls the predicate every 20 ms, and on cancellation raises
  a flag the work checks, so a cell being finished stops at its next step and a pair search
  within 256 node pairs. The serial stages of the output poll it too, at a new checkpoint
  `BooleanMesh`. Tests cancel at each of the three checkpoints and stop items already running.
- **Measured on the owner's torso** (release, 16 threads, the same run conditions): 14.9 s
  where `46fffc1` took 27.6 s, the result identical bit for bit (the same hash of its
  coordinates and triangles); the Boolean's own peak 2.3 GB above the preparation's, where it was
  4.4 GB (the process's peak resident memory 3.1 GB, from 5.1 GB measured the same way). Contacts 3.3 s to 0.8 s, crossings
  2.3 s to 1.0 s, joining 3.3 s to 0.7 s; the cells 2.6 s, identifying 2.1 s, T-junctions 1.9 s,
  triangulation 1.0 s.

**Phase 5, as implemented (2026-11-09): the output on the grid, rounding's crossings resolved**
(owner, 2026-11-09).
- **Why.** A model file stores coordinates as `f32` (binary STL can hold nothing else, and
  slicers read 3MF and OBJ so). On the owner's torso, the `f64` result stored as `f32` had 5,156
  crossing pairs and 382 collapsed triangles. About 95 % lay where the sculpt's own overlapping
  bodies were united, 26 mm or more from any hole; about 226 lay on the holes' walls.
- **The finer grid.** The kernel's grid is 2⁻¹⁵ mm, and the admitted range is ±2,000 mm (it was
  2⁻¹² mm and ±10,000 mm). The bounds of `plane` are in grid steps (`G < 2²⁶`), so every proof
  holds unchanged. A grid point within ±512 mm is an exact `f32` (at most 24 significant bits)
  and an exact `f64`, so a mesh on the grid is stored exactly by every format. Snapping moves an
  input vertex by at most 2⁻¹⁶ mm.
- **The rounds** (`boolean::solve`): Valque and Lazard's method (SGP 2025), with the grid as the
  target precision.
  1. The exact result is rounded to the grid.
  2. The pairs of triangles that then cross, and the triangles left without area, are found
     exactly.
  3. Every vertex in a cell of a fixed snap grid (2⁹ steps, 15.6 µm) that holds a vertex of such
     a triangle moves to the cell's centre. A vertex moves by at most 7.8 µm on each axis, below
     any resin pixel; slivers collapse, and the triangles that collapse are left out.
  4. The mesh is united with itself by the kernel, which resolves whatever still crosses, and
     the new result is rounded again.

  After 12 rounds, what remains is counted and reported. Whatever the rounds do, a result that
  is not closed is refused (`NotClosed`), never written.
- **Specks** (2026-11-09, found comparing the export with the source). Rounds can leave a
  separate body no larger than a snap cell: on the torso, a 0.3 µm tetrahedron and a 15.6 µm
  bipyramid with vertices at cells' centres; the source is one body. A separate closed body
  whose extent is at most a snap cell (15.6 µm) on every axis is left out and counted
  (`specks`), within the move the rounds already allow and below any pixel. Removing a whole
  closed body keeps the rest closed, and a result that is only such a body is kept. The torso's
  drilled model is one body, its two specks reported.
- **Touching bodies moved apart** (2026-11-09, the owner's trial in Bambu Studio and Lychee).
  - **Why.** Where the sculpt's bodies touch along a line, the union leaves edges of four faces
    (91 on the torso). Writing them by vertex index keeps 3MF valid, but STL has no indices: every
    reader welds equal points and finds them again. Bambu Studio reported "31 non-manifold edges"
    for the torso's STL.
  - **What.** As the kernel's last step, each such vertex gets a copy per sheet. The faces about
    the edge are paired by their exact angle, each pair bounding a wedge of solid, as the 3MF
    writer pairs them. Each copy moves two grid steps (about 0.06 µm) into its own body, along
    its faces' mean inward normal.
  - **Edges still shared.** An edge whose sheets still meet around both ends, or whose copies
    could not move, is split instead: each pair of faces gets its own point in the middle, moved
    the same way.
  - **Result.** No reader finds an edge of four faces. On the torso: 698 points, every edge used
    by two faces once welded; Bambu Studio found 1 non-manifold edge before the split pass.
  - **The cost.** 1,750 pairs of triangles cross by nanometres along the seams. Moving by 1 µm
    left 2,096: they come from the seams' slivers turning, whatever the distance. They are
    reported, read by the positive fill rule, and below any pixel.
- **What did not work** (the research record has the numbers):
  - Rounding to the 2⁻¹² mm grid, coarsening the snap in every round and moving only the
    offending vertices diverged on the torso.
  - So did the same loop on the 2⁻¹⁵ mm grid.
  - Valque and Lazard keep the snap grid fixed, snap whole cells, and keep the new points far
    finer than the cells. With those three, the torso converged.
- **Measured on the owner's torso** (release, 16 threads): 13 holes cut in 73.5 s in all.
  - 8 rounds after the cut; 14,816 vertices moved to cells' centres, the farthest by 7.81 µm.
  - 2,316,936 triangles, closed, nothing flat or crossing. Peak resident memory 3.4 GB with the
    preparation.
  - Its 1,873 layers against the shell sliced with its cuts: 33,729 of 566,880,565 material
    pixels differ, all on a boundary but 10. Those 10 lie within 4.6 µm of the model's surface
    or a hole's wall, inside the reported move.
  - With 7.8 µm cells, 5 rounds left 39 crossings.
- **Known defect, contained.** On the torso, one round's exact result once came out with 3 edges
  unbalanced: a T-junction on a seam where two sheets touch along a line was not resolved. The
  next round, welding by position, closed it. It is not yet reproduced in a small case. The
  final closure check refuses any such result rather than write it.
- **Non-manifold edges.** Where the sculpt's bodies touch along a line, the union leaves edges
  of four triangles (91 on the torso). 3MF requires two triangles per edge by vertex index, so
  its writer must give each sheet its own vertices there (TD-139).
- **Tests.**
  - A block drilled on the grid needs no round.
  - The drilled sphere is resolved in one or two rounds, nothing left, every vertex an exact
    `f32`.
  - Grid points within ±512 mm are `f32` values.
  - The torso's layer comparison allows each differing pixel the reported move.

## TD-141 — Packaging prototype: executables, icon, splash, packages and notices

**Status:** Implemented on 2026-11-09 at the owner's request (M6.7 packaging prototype), with the
owner's choices of the same day. The Windows installer was built with Inno Setup 6.7.3 and
installed, checked and uninstalled silently on native Windows (see "Tests"). Native Windows
validation of 856f712 (2026-11-09, `validate.ps1 -IncludeApp`): no failed command, 1,719 tests
in Debug and in Release, the three application smoke runs pass with the opaque splash.

**Decision:**
- **Executable names:** the application's executable is `karasu` (`karasu.exe`), from a `[[bin]]`
  of the `karasu-app` crate, which keeps its `karasu-<module>` name; the command line stays
  `karasu-cli`. The `resin-cli` compatibility alias is built but not packaged.
- **Icon:** the owner's master `karasu_icon_master_v2.png` (2026-11-09, 1254 × 1254), which
  replaced a first master whose emblem filled only three quarters of the square and blurred on a
  dark taskbar. The emblem is centred to span 96 % of the side, resampled into 16 to 512 pixel
  PNGs (high-quality bicubic), and the sizes up to 48 pixels are sharpened with an unsharp mask
  (0.6, colour only) (`tools/packaging/make_icons.ps1`, which can also draw an outline; the owner
  chose none); a Windows `karasu.ico` holds PNG entries 16 to 256 (`make_ico.py`). The owner
  keeps the masters; the repository holds only the images in use (owner, 2026-11-09). The
  window's icon is the 256 pixel PNG, compiled in.
- **The icon in `karasu.exe`** (owner, 2026-11-09): the build script `crates/karasu-app/build.rs`
  turns `karasu.ico` into a 32-bit Windows resource file (`.res`) in `OUT_DIR` and passes it to
  the MSVC linker of the `karasu` executable (`cargo::rustc-link-arg-bin`), which takes resource
  files as inputs. The `.res` format is documented and simple (an empty first entry, one
  `RT_ICON` per image, an `RT_GROUP_ICON` naming them), so `src/icon_resource.rs` writes it with
  the standard library: no resource compiler, no crate, nothing to review as a dependency. Other
  targets skip it. Explorer, the taskbar, the shortcuts and the `.karasu` association show the
  icon from the executable, so the packages no longer carry `karasu.ico`; the installer uses it
  for the setup program only. `karasu-cli.exe` has no icon. The executable grows by 146 KB.
- **Splash:** the owner's artwork (1672 × 941, kept by the owner), which
  carries the product name, replaces the generated one of TD-069. The splash is 720 × 405 points
  (16:9); `tools/assets/make_splash.ps1` resamples the master to 1440 × 810 pixels. It is square
  and opaque, and the window is no longer transparent (owner, 2026-11-09): the rounded corners
  showed dark on native Windows anyway. The application no longer draws the name over it (only
  in the fallback panel), and lines its subtitle, step, bar and version up with the artwork's
  title, clear of the raven. The artwork is about 2 MB of the executable.
- **Packages** (`tools/packaging/package.py`), each with `karasu`, `karasu-cli`, a README and the
  third-party notices; reproducible for the same executables (sorted entries dated at
  `SOURCE_DATE_EPOCH`, by default the last commit's time):
  - **Windows:** a portable ZIP, and an Inno Setup 6 installer (`karasu.iss`) per user, without
    administrator rights (`%LOCALAPPDATA%\Programs\karasu-slicer`): a Start menu entry, an
    optional desktop one, an uninstaller, and the `.karasu` association under
    `HKEY_CURRENT_USER\Software\Classes`. The owner chose Inno Setup 6.7.3, its latest release.
  - **Linux:** a tar.gz whose `install.sh` copies the tree into `~/.local` (or a prefix),
    records what it copied for `--uninstall`, and points the menu entry at the installed
    executable; and a `.deb` under `/usr` built with `dpkg-deb`. Both carry a desktop entry
    (`karasu-slicer.desktop`, matching the window's app id), the hicolor icons and the
    `application/x-karasu-project` MIME type for `*.karasu`. The `.deb` depends on the C library
    at the version the build needs (2.39 when built on Ubuntu 24.04), `libgcc-s1`,
    `libxkbcommon0`, and Vulkan or EGL, and recommends the windowing libraries and the desktop
    portal.
- **Third-party notices** (`tools/packaging/notices.py`): every crate compiled into the
  executables for the platform, from `cargo metadata` (normal dependencies reachable from the two
  executables' crates), with version, licence and repository, then each licence or notice text
  their packages carry, once. Crates whose packages carry no licence file (18, the egui family
  among them) are listed with their authors and declared licence, followed by the standard MIT
  and Apache-2.0 texts. Windows: 175 crates, 113 texts; Linux: 253 crates, 150 texts.
- **Not in the prototype:** signing (studied in M6.7), `PATH` for the command line, stripped
  Linux executables (`karasu` is 42 MB with its symbol table, 33 MB
  without; kept for readable panic backtraces), and the project's own licence, which only the
  owner chooses.

**Measured (2026-11-09):** Windows ZIP 15.2 MB (`karasu.exe` 29.1 MB, `karasu-cli.exe` 6.2 MB),
installer 13.4 MB (compiled in 11 s); Linux tar.gz 19.0 MB, `.deb` 14.4 MB; assembling the Linux
packages takes 10 s, most of it compressing. Building the same Linux packages twice gives
identical bytes.

**Tests:** the icon's resource file read back (the empty first entry, each image as an
`RT_ICON` named in order, the group's directory naming them), and files that are not icons
refused; on native Windows, the icon Explorer extracts from `karasu.exe` is the application's
(the previous build's showed the generic one), and the installer's shortcut and association
point at the executable; the window icon decodes as a square RGBA image; the splash artwork test checks the
new size and format, opaque everywhere. The Linux tar.gz was installed into a test prefix and
removed again (`install.sh`, `--uninstall`), the desktop entry pointing at the installed
executable; both executables ran from it. The Windows installer, run silently into a test
folder for the current user: exit status 0; the five files and the uninstaller in place; the
`.karasu` association and its open command, the Start menu entry and the Apps entry with its
icon registered under `HKEY_CURRENT_USER`; `karasu-cli --version` ran from the installed folder;
the silent uninstall removed the files, the association and the Start menu entry. Inno Setup
installs per user (`winget install JRSoftware.InnoSetup --scope user`, under
`%LOCALAPPDATA%\Programs\Inno Setup 6`). Not yet tried by hand through the wizard.

## TD-142 — Accessibility: names, keyboard focus ring and the accessibility check

**Status:** Implemented on 2026-11-10 at the owner's request (M6.7 accessibility): parts A1, A2
and A6 of the analysis of the same day (6a6d904), with the owner's choice of a 2-point accent
focus ring; then parts A3, A4 and A5 with the owner's choices of the same day: the status
colours configurable, no keys for the camera in the 3D view. Native Windows validation of
f1ba1b8 (2026-11-10, `validate.ps1 -IncludeApp`): no failed command, 1,729 tests in Debug and in
Release, the three application smoke runs pass. Not yet tried with a screen reader.

**Analysis (2026-11-10, before any change):** the window was drawn headless with AccessKit on,
in every tool and dialog, and its tree read back:
- every function was reachable with the keyboard, menus opened with Enter and moved with the
  arrows, modal dialogs kept the Tab cycle and closed with Escape;
- about half the controls had no name for a screen reader (with a model selected 13 of 37, in
  the Supports tool 43 of 88, in Settings 91 of 122): value fields, switches, the application's
  sliders, colour pickers, section arrows, the plates' cards, the 3D view;
- each value field took two Tab stops, its unit segment being one that only a drag uses (of
  zero width for counts, an invisible stop);
- the focus looked like a pressed control, its fill hardly apart from the hover fill (1.37:1),
  and the application's own controls (icon buttons, tool strip, choices, sliders) showed none;
- egui does not scroll to a control the Tab key reaches, so the focus could land on a control
  hidden in a scrolled panel;
- notifications stayed in the Tab cycle under a modal dialog.
The colour-only cues and the contrast of the theme (A4, A5) are recorded for the next parts.

**Decision:**
- **Names** (`widgets::name`, `widgets::name_id`): a control whose visible label is drawn apart
  is given that label as its AccessKit name. The rows name their field (`value_row`,
  `default_row`, `check_value_row`, `auto_row`, `default_switch`, the profile forms'
  `form_row`), and so do the controls drawn alone (section fields and slider, transform axes,
  colour pickers and their hexadecimal fields, combo boxes, search fields, the 3D view, plate
  cards and tabs, section arrows, the tool strip's option triangles, a dialog's resize corner).
  A choice's options are radio buttons; the application's sliders report a slider with its
  value; a plate's card says its slice state. Naming is free while no assistive technology is
  attached (AccessKit builds no tree then).
- **Keyboard:** a value field's unit segment takes no focus (one Tab stop per field, typed and
  stepped with Up and Down); the application's sliders take the arrows (one step), Page Up and
  Page Down (a tenth) and Home and End while focused.
- **Focus ring** (`focus.rs`, owner 2026-11-10): a 2-point outline in the accent colour
  (`#C9B2FF`), 1 point from the focused control, on the control's own layer, drawn last in the
  frame. It shows while the keyboard is in use (a Tab or an arrow turns it on, a pointer press
  off), so clicks leave no ring. A control drawn in parts names the rectangle to ring (a value
  field: the whole field).
- **In view:** the areas that scroll are shown through `focus::scrolled`, which scrolls a control
  the keyboard just focused into view (and the areas around it), at once and with a repaint so
  it does not wait for the next input.
- **Under a modal dialog,** notifications go under its backdrop and take no focus.
- **The accessibility check** (`accessibility_check.rs`, tests): what a draw shows is drawn with
  AccessKit on; every reachable control must have a name, and the Tab cycle is walked: every
  stop must be visible once scrolled to, named, and inside the modal dialog when one is open.
  egui's own drag targets (a window's title bar and resize corner) and scroll bars are left
  out. It runs on the main window empty and with a model in all twelve tools, the four list
  windows, every dialog (settings on each of its eight pages, profiles, plates overview,
  arrangement, export of models and of every plate, renaming and deleting a plate, deleting a
  model, log, messages), every profile dialog and the plate's printer, resin and setting
  selectors.

- **A3, keyboard order and windows:**
  - The 3D view takes no keyboard focus (owner, 2026-11-10: the camera has no keys); it is named,
    and clicks and drags work as before.
  - The tool strip is drawn before the tool panel, so the Tab key reaches the strip at the left
    before the panel at the right. The status bar's Messages button stays before the sidebar:
    egui lays the bottom bar out before the side panels.
  - A tool window that opens takes the focus on its first control (`dialog::tool_window`, its
    content's `remember_tool_window`): egui has no call for it, so on the window's first shown
    frame a focus entry at the start of its content has the focus and passes it on as the Tab
    key does; it exists on that frame only and is never a Tab stop. A modal dialog keeps the
    focus inside it from the first Tab.
- **A4, meaning not in colour alone:**
  - the Messages window shows each message with its level's icon and name; the status bar's
    button names the worst unseen level ("Messages (2 new, Error)"); the status bar's line
    starts a warning or an error with its level;
  - a plate's tooltip names its slice state;
  - while placing a support, the panel says when the support would have an error (outside the
    build volume, or no reach to the plate), not only its red preview;
  - a printer name already in the library is said under the field;
  - the layer range dialog's errors and warnings each have their icon;
  - a drain hole with notes reads "OK, with notes" in its list.
  The viewport's tints (overhangs, islands, supports and holes in error, repair markers) keep
  their colours; their lists, findings and hover texts say the same in words.
- **A5, status colours:** the warning, error and success colours and the notifications' four
  fills are the user's, in Settings › Appearance › Status colours, saved with the other colours
  (`Appearance::status`). The window hands them to `theme::set_status` each frame and the
  status tokens answer with them on the interface's thread (a thread-local value, so tests on
  other threads keep the defaults). Under the group, a line names any status colour that reads
  under 4.5:1: a text colour on the panels, or a fill behind the primary text. The defaults are
  unchanged: red and green are close for deuteranopia (simulated: #B5A767 and #C3B680), so the
  icons and words of A4 carry the meaning.
- **The controls' outline:** `BORDER`, 1.88:1 on the panels, under the 3:1 WCAG asks of control
  boundaries, is an approved palette colour. A lighter outline (`#6E6A84`, 3.28:1) was proposed;
  the owner keeps the current one (2026-11-10). Controls stay recognisable by their fill, their
  place in a row and their names.

**A7, manual screen-reader pass:** done by the owner on 2026-11-10, with no problem reported.

**Tests:** the accessibility check above (four tests); a tool window that opens taking the
focus and the 3D view taking none; the focus ring following the keyboard and leaving with a
click; the status colours being the user's on the interface's thread only, and the contrast
measure; a warning or an error named in words in the status line; and the existing widget
tests.

## TD-143 — Panel orientation of four ELEGOO printers, verified from ELEGOO's test files

**Status:** Implemented on 2026-11-10 at the owner's request, after the catalog's second phase
added twelve ELEGOO printers whose orientation was unverified. The Mars 4, Mars 4 Ultra and
Saturn 3 Ultra are superseded by TD-153 (2026-11-14), which follows ELEGOO SatelLite.

**Evidence:** ELEGOO's download page (`https://www.elegoo.com/pages/download`) offers a test archive
per printer; four of the catalog's printers come with the model and ELEGOO's sliced file side by
side (`ROOK.stl`, `ROOK.goo`). Read on 2026-11-10, kept outside the repository:

| Printer | Archive (SHA-256) | `ROOK.goo` (SHA-256) | Written by | Declared mirror X, Y |
| --- | --- | --- | --- | --- |
| Mars 4 | `de45391a…6ca` | `629b8fe3…a148` | Voxeldance Tango 2.11.99.70 | 1, 0 |
| Mars 4 Ultra | `a09d9f2b…2c63` | `0d550a69…f7f0` | CHITUBOX 1.9.5 | 1, 0 |
| Saturn 3 | `c4c4d12f…061d` | `4a7c2b01…ad9c` | Voxeldance Tango 2.11.99.68 | 1, 0 |
| Saturn 3 Ultra | `e1e7366d…9260` | `de62ef6f…ef52` | Voxeldance Tango 2.11.99.69 | 0, 1 |

The Mars 4 Max's archive holds a sliced file only (`010bb85e…9da0`, Voxeldance Tango, declaring
0, 1); the Mars 5, Mars 5 Ultra, Saturn 4, Saturn 4 Ultra, Jupiter 2 and Jupiter SE have no test
file there.

**Method (as TD-104):** each `ROOK.stl` was exported by karasu-slicer for its catalog printer,
centred and on the plate as ELEGOO's file places it, at the same 0.05 mm layers, and the stored
images compared at layers 100, 400, 600 and 900 (the rook's spiral stair makes the middle layers
asymmetric) with `tools/goo-orientation/goo_compare.py`: intersection over union of lit 4 × 4 cells,
as stored, mirrored along columns or rows, and turned 180 degrees.

| Printer | karasu-slicer before (no mirroring) | After |
| --- | --- | --- |
| Mars 4 | turned 180°: 0.98–1.00; as stored 0.68–0.92; mirrored 0.29–0.92 | as stored 1.00 |
| Mars 4 Ultra | turned 180°: 0.98–1.00; as stored 0.65–0.92; mirrored 0.26–0.97 | as stored 1.00 |
| Saturn 3 | turned 180°: 0.98–1.00; as stored 0.68–0.92; mirrored 0.46–0.92 | as stored 1.00 |
| Saturn 3 Ultra | as stored 1.00; turned 180° 0.67–0.92; mirrored 0.55–0.92 | unchanged |

**Findings:** no printer was mirrored: karasu-slicer's parts kept their handedness everywhere.
Three faced the plate's back where ELEGOO's face its front, as the Saturn 4 Ultra 16K before
TD-104. Each header's declared mirroring matches its image (a declaration of 1, 0 stores
karasu-slicer's unmirrored image turned 180°), from both Voxeldance Tango and CHITUBOX.

**Decision:**
1. The Mars 4, Mars 4 Ultra and Saturn 3 mirror in X and Y (revision 2); their exports now match
   ELEGOO's stored images (1.00 at every compared layer) and declare 1, 0 as ELEGOO's files do.
   The Saturn 3 Ultra keeps no mirroring. All four are marked verified in the catalog and the
   README's table of supported printers.
2. The Mars 4 Max stays unverified: its official file declares the image karasu-slicer already
   writes for it, but no model beside it allows the comparison. The six printers without a test
   file stay unverified too.
3. The scripts are kept in `tools/goo-orientation/`; ELEGOO's files stay outside the repository.

**Native Windows:** validated with c0890ff on 2026-11-10 (no failed command, 1,731 tests in
Debug and in Release).

**Limits:** this verifies the stored image against ELEGOO's own slicers, not a print. Whether a
printer's firmware applies the header's declaration was not needed: karasu-slicer's image and
declaration are now both ELEGOO's.

## TD-145 — Licence: AGPL-3.0-or-later, the name and logo as trademarks

**Status:** decided by the owner on 2026-11-11, after a comparison of GPL-3.0, AGPL-3.0,
MPL-2.0, MIT/Apache-2.0 and a proprietary licence.

**Decision:**
1. **Licence:** the GNU Affero General Public License, version 3 or any later version
   (`AGPL-3.0-or-later`). `LICENSE` is the FSF's text verbatim (`agpl-3.0.txt`, SHA-256
   `0d96a4ff…abcb0`, 661 lines). Every package of the workspace and the fuzz package declare it
   (`license.workspace`); `tools/reference-fixtures` stays unchanged by AGENTS.md and is covered
   by the repository's licence.
2. **Why the AGPL:** the core's work (exact Booleans, supports, rafts, slicing, formats) stays
   open in programs that distribute it and in services that run it over a network, as cloud
   slicing would; PrusaSlicer and UVtools are AGPL too. Every dependency is permissive (MIT,
   Apache-2.0, BSD, Zlib, ISC, Unicode, BSL, fonts under OFL), all compatible with AGPL-3.0.
3. **Copyright:** "Copyright (C) 2026 Ivan Maruca" (`karasu_core::COPYRIGHT`), the owner as an
   individual, as for code signing.
4. **Trademarks:** the names karasu-slicer and karasu and the logo (`assets/ui/icon/`,
   `assets/ui/images/splash.png`) are trademarks, not licensed with the code
   ([TRADEMARKS.md](../TRADEMARKS.md)): forks take another name and other artwork. The generated
   interface icons are code, licensed with it.
5. **Contributions:** accepted under a Contributor Licence Agreement ([CLA.md](../CLA.md),
   revised by the owner on 2026-11-12) that leaves the copyright with the contributor and lets the owner
   also offer other licences, commercial ones included; [CONTRIBUTING.md](../CONTRIBUTING.md)
   says how to agree.
6. **Notices:** the desktop application has Help › About karasu-slicer with the copyright, the
   licence's notice (no warranty), links to the licence and the source
   (<https://github.com/dazeroit/karasu-slicer>, to be public) and the trademark note: the
   AGPL's "Appropriate Legal Notices" for an interactive interface. `karasu-cli --version`
   prints the same. The packages carry `LICENSE.txt` and `TRADEMARKS.txt` beside the
   third-party notices; the Windows installer shows the licence as information and names the
   owner as publisher.
7. **Code signing:** SignPath Foundation excludes projects with commercial dual licensing, which
   the CLA keeps open; the open-source route left is Certum Open Source (about €49 a year), or
   an OV certificate.

**Tests:** the licence file is the AGPL and the manifest names it; the core's notice says
"WITHOUT ANY WARRANTY"; the About dialog shows the copyright, the notice and the source and
closes on Escape, and passes the accessibility check; `--version` prints the copyright and the
licence; every action, About included, is listed among the shortcuts.

## TD-144 — Raft shapes: Skate, then Web

**Status:** implemented on 2026-11-10 at the owner's request, in two phases: Skate, then Web.

**Request (owner, 2026-11-10):** two raft types besides the slab. *Skate*: the slab with its
45° bevel the other way, over the whole thickness. A second type joining every plate foot to its
neighbours by low bars that form triangles, from a reference screenshot of another slicer used
as a behavioural reference only; the owner named it *Web*.

**Decisions:**
1. **The shape is a raft value** (`RaftSpec::shape`, `RaftShape`), written in the support
   document as `"shape"` only when it is not a slab: documents written before read as slabs, and
   earlier builds still read a slab's document. An earlier build refuses a document with another
   shape as an unknown field, with that error; the document version stays 1.
2. **Skate:** the slab's own construction, the hull of the feet's fillet rings widened by the
   margin at the top, with the ring at the plate narrowed by the slope's spread (thickness over
   the slope's tangent) instead of widened. Choosing Skate sets the slope to 45°, which stays
   editable; going back to Slab keeps the slope.
3. **No silent repair:** under a foot whose ring at the plate would be narrower than
   `SKATE_MIN_PLATE_RADIUS_MM` (0.1 mm as a radius, two pixels of a 50 µm panel) the build fails
   with the foot's place and what to change, rather than turning the raft inside out or
   clamping it. The Raft footer says so in red as soon as the narrowest foot (a fillet of the
   raft's own diameter) has nothing left at the plate.
4. **Web:** under each plate foot a disc, the slab of that foot alone (a frustum: the fillet
   widened by the margin at the top, the slope's spread wider at the plate). Bars between feet
   are chosen shortest first (ties by the feet's places): a pair is kept when its segment
   crosses no kept bar and passes through no other foot's disc, crossings decided with exact
   orientation signs (`turn_2d`, shared with the slab's hull), so the result is a
   deterministic set of segments that never cross: the greedy triangulation of the feet, cut
   at "Longest bar". Feet whose discs overlap at their tops get no bar. Bars are straight
   prisms from centre to centre with a trapezoid section (the width at the top, widened to the
   plate by the raft's slope over the bar's height), their ends inside the discs; discs and
   bars stay separate closed convex parts, overlapping as every support part does. Defaults
   (owner): bar width 1.0 mm, bar height 0.5 mm, longest bar 20 mm, and a new raft 0.5 mm
   thick instead of 1 mm (owner, 2026-11-10); in the document
   `bar_width_mm` 0.2–10, `bar_height_mm` 0.2–5, `bar_longest_mm` 1–200, written for a web only.
   The Base choice (single or groups) is hidden for Web: the longest bar groups the feet. The
   scene counts a web's pieces (discs joined by bars or overlapping at the plate) as its
   rafts. More than 2^22 candidate bars is refused as a resource limit.
5. **Search:** candidates come from a grid as wide as the longest bar; the disc and crossing
   tests look only at the cells of a finer grid (at least the widest disc) along the segment,
   walked from its start and stopped at the first disc met, since most long candidates are
   blocked near their start.

**Measured** (Linux, release, `raft_shapes_under_many_feet_are_timed`, 1,600 plate supports
2.3 mm apart under a 100 mm slab): the whole support build takes 0.12 s with a slab raft,
0.10 s with a skate and 0.24 s with a web (1,600 discs, 4,641 bars, one piece; 1.16 million
triangles in all). Before the search walked the cells and stopped at the first disc, choosing
the bars alone took 0.42 s.

**Tests:** a skate's width at the plate against its top (narrower by twice the spread, a slab
wider by as much), a closed mesh without findings, the shape written only for a skate and read
back, an unknown shape refused, the build failing on a skate too thick for its margin; in the
application, the Type menu's choice setting 45°, a skate built under a support, the footer's
error, and the shape kept with the settings. For the web: a square of feet taking its four
sides and one diagonal, touching discs taking none, bars on a jittered grid of 400 feet that
never cross nor pass through a disc and repeat exactly, a bar a closed prism wound outward; a
web built under four feet (seven parts, two pieces, bars 0.8 mm high, closed), its values
written and read back and refused out of range; in the application, two feet joined into one
piece and kept apart by a shorter longest bar.

## TD-146 — The resin catalog: Anycubic, ELEGOO and SUNLU resins, one entry per product

**Status:** implemented on 2026-11-11 at the owner's request ("can we add Anycubic, ELEGOO and
SUNLU resins to the catalog?"), after the research in
[RESIN-CATALOG-RESEARCH.md](research/RESIN-CATALOG-RESEARCH.md).

**Decision (owner, 2026-11-11):**
1. **One catalog resin per product**, not per colour (TD-118 still holds for the user's own
   resins). Where a product's colours have different settings, the entry names the colour whose
   settings it holds (ELEGOO's first colour, Grey for most); CATALOG-SOURCES.md lists the other
   colours that differ, which users adjust in a copy. Anycubic's 16K Standard holds the colours
   other than Orange Red.
2. **Anycubic:** the 16 resins sold on 2026-11-11, with the settings of Anycubic's resin page
   (A8), single values, "official", anti-aliasing included (1 where the page gives none or
   cannot be taken as printed). The Photon P1's Standard Resin keeps Photon Workshop's default
   (A11); A8's other P1 columns are used. The product pages, read only in Internet Archive
   captures, disagree with A8 in many values; A8 is used and the differences are recorded.
   Legacy resins (no longer sold) are left out, and so are A8's Standard Resin V2 columns of the
   M5, M5s and M5s Pro (a missing layer, and 0.1 mm layers with the 0.05 mm exposures: an
   inferred copy error).
3. **ELEGOO:** the 23 resins of ELEGOO's table (E2) on the catalog printers, in the normal modes;
   the fast-printing modes and their Rapid Standard resin are left out. Each printer's bottom
   layers come from its manual (E3). A range gives its middle and makes the setting a starting
   point; a setting of single values only is "official". The tilting printers have no Z motion in
   the table: karasu-slicer's 5 mm lift at 3 mm/s stands in, so their settings are starting
   points. The Mars 4 Max has none: its manual shows another printer's values.
4. **SUNLU:** its 16 resins as materials only. SUNLU publishes no setting for any printer, only
   wide ranges by screen type; a user starts from another setting (TD-106's "start from").
5. **Densities:** the manufacturer's single published value, in g/ml: ELEGOO's safety data
   sheets (1.05 for each resin that has one), SUNLU's technical data sheets (the liquid value,
   tolerance dropped), Anycubic's Standard Resin page (1.13). A range gives none, and so does a
   data sheet that describes another resin. Costs stay unset.
6. **Adding a printer from the catalog** offers its resins as a checklist, with only the vendor's
   standard resin ticked (`Vendor::standard_resin` in the catalog index), and an "All" box over
   the list that ticks every resin or clears them, mixed while only some are, as the sidebar's
   box for models; it replaces one box that added every resin with a setting for it.

**Implementation:**
- `tools/assets/extract_resin_tables.py` (standard library only) reads the saved A8 page and the
  E2 spreadsheet, which stay outside the repository, and writes
  `tools/assets/catalog-data/anycubic-a8.tsv` and `elegoo-e2.tsv` with every value as
  published, each headed by its source's SHA-256. Both match an independent conversion made
  during the research.
- `tools/assets/make_catalog.py` reads them, stops on any value it cannot read (only "/" means a
  value not given), and writes the catalog, the index's vendors with their standard resins, and
  CATALOG-SOURCES.md with a table of resins, a count of settings per printer, what was left out
  and the colours that differ. Setting identities stay `<printer>-<resin>`; one ELEGOO slug is
  shortened (`8k-washable-abs-like-resin`, as ELEGOO's safety data sheet names it) to keep the
  identity within 64 bytes, which the generator now checks.
- Revisions: the Photon Ultra's and D2's Standard Resin settings (anti-aliasing 4 and 16) and the
  Anycubic and ELEGOO Standard Resins (density, ELEGOO's colour) are revision 2, so copies are
  offered the update (TD-106). The tilting ELEGOO printers' source text is corrected (bottom
  layers from the manuals) without a new revision: no value changed.
- `Catalog::resins_for` and `Catalog::standard_resin_for`; `Library::add_printer_from_catalog`
  takes the resins to copy. The catalog check refuses a vendor whose standard resin is not one of
  its catalog resins.
- Catalog: 55 resins (16 Anycubic, 23 ELEGOO, 16 SUNLU) and 389 print settings (141 Anycubic
  official; 248 ELEGOO, 1 official), from 2 and 31.

**Measured (Linux, release):** `Catalog::built_in`, run once when the library opens, 0.20 ms
before and 1.9 ms after; the catalog's JSON compiled in grows from 84 KB to 763 KB, a test
executable from 1.39 to 2.14 MB.

**Tests:** the catalog holds 16, 23 and 16 resins of the three makers, densities within
0.9–1.4 g/ml, no SUNLU setting, ELEGOO's standard resin offered for the Saturn 3 among more than
twenty, none for the first Photon or the Mars 4 Max, no 8K resin on the Jupiter SE, the 16K's
setting a starting point at the middle of ELEGOO's range; the dialog ticks only the standard resin
at first and adds the resins ticked, all of them or none, a resin's copy shared between
printers; the dialog with a
printer's resins passes the accessibility check.

## TD-147 — Clean-room development: behaviour only

**Status:** decided by the owner on 2026-11-12, before the repository was published.

**Decision:**
1. karasu-slicer is developed in clean room, as AGENTS.md's "Clean-room development" states: other
   programs, commercial or open source, may inform what karasu-slicer does, never how it is
   written. Research uses public specifications and literature, manufacturers' published
   documentation, programs' behaviour as an ordinary user sees it, the files they write from
   inputs prepared for the purpose, and data files under a compatible licence read as data.
2. No other program's source code or binaries are examined, open source included and whatever
   its licence.
3. No other program's code, in any form, nor a description of its internals ever enters the
   repository; documents mention other programs only for observed behaviour and published facts.
   Legal assessments (licences, patents) are not recorded in the repository.
4. Third-party code enters the product only as a reviewed dependency (AGENTS.md, Dependencies).
5. CONTRIBUTING.md and the CLA ask the same of contributors.

**Consequence:** before publication the documents were reviewed so that they mention other
programs only for observed behaviour and published facts; the catalog's extensions of the
Photon Mono M5 and M5s are left without a source until an official one confirms them.

## TD-148 — Accepting the CLA in pull requests

**Status:** decided by the owner on 2026-11-12.

**Decision:** contributors accept the [CLA](../CLA.md) by posting a fixed sentence as a comment
in their pull request ([CONTRIBUTING.md](../CONTRIBUTING.md)); a check named **CLA** stays failed
until every author of the pull request's commits has accepted it. Acceptances are recorded in the
private repository `dazeroit/karasu-slicer-cla` (`signatures/cla-v1.json`): GitHub login and id,
the name and e-mail address of the author's commits, the CLA's version and blob identity, the pull
request, the comment and its date. The owner's account and bots need no acceptance. A merged pull
request is locked, so that the comments recording acceptances stay as they were.

**Implementation:** `.github/workflows/cla.yml` runs `.github/scripts/cla.js` through
`actions/github-script`, both GitHub's own actions pinned to commits (`actions/checkout` v7.0.1,
`actions/github-script` v9.0.0). The workflow runs on `pull_request_target` and pull request
comments, always from the default branch; it never checks out or runs the pull request's code and
reads it only through the API. It writes the acceptances with the `CLA_SIGNATURES_TOKEN` secret, a
fine-grained token limited to the private repository's contents.

**Alternatives:** the hosted CLA Assistant keeps the acceptances in its own service and the text
in a Gist to keep identical to `CLA.md`; its GitHub Action edition was archived in 2026 and gets
no security fixes, while it would run on every pull request with a write token. The earlier rule,
a sentence with name and e-mail address in the pull request's description, was not checked.

**Tests:** `node --test .github/scripts/cla.test.js` (Node 24) against an in-memory GitHub: an
author who has not accepted fails the check and is asked to; the exact sentence records the author
once and passes it; nobody else can accept for an author, and no other sentence counts; every
author must accept; the owner and bots need nothing; a commit without a GitHub account fails it;
`recheck` records nothing; a merged pull request is locked and a closed one is not; a missing
token is an error, never a pass; comments on issues are ignored.

**Setup (owner):** create the private repository `dazeroit/karasu-slicer-cla` with a `main`
branch (the first acceptance creates the file); create a fine-grained token for that repository
alone with Contents read and write; add it to karasu-slicer as the Actions secret
`CLA_SIGNATURES_TOKEN`; optionally require the **CLA** check on `main`.

## TD-149 — Branches, versions and releases

**Status:** decided by the owner on 2026-11-12.

**Context:** until now every change went straight to `main`. karasu-slicer is a desktop
application: users keep a version installed and need its defects fixed without taking a new major
version with unfinished work. The project has one maintainer, and each maintained line costs every
fix twice and a native Windows validation per release.

**Decision:** [RELEASE-PROCESS.md](RELEASE-PROCESS.md) describes the process; in short:
1. `main` develops the next major version. Every change, the owner's included, enters it from a
   short topic branch through a pull request that passes CI and the CLA check and is
   squash-merged. The owner may bypass the rules in an emergency.
2. A release branch `release/N.x` per major version is created when N.0 is frozen, from release
   candidates `vN.0.0-rc.K` to `vN.0.0`; `main` then moves to `(N+1).0.0-dev`. The branch receives
   fixes (patch versions) and additions that change no file format, command line or default
   behaviour, such as catalog printers and resins (minor versions).
3. Fixes go to `main` first and are carried to the release branch with `git cherry-pick -x`.
4. Semantic Versioning, judged by what users saved or automated: a major version breaks files,
   the library, the command line, or removes a printer or an output format. Annotated tags
   `vX.Y.Z` are never moved. Before 1.0, `0.x` previews are tagged on `main` and receive no fixes
   of their own.
5. The latest series receives every fix; the previous one critical and security fixes for three
   months after the next major version.
6. GitHub rulesets enforce it: no deletion or force push of `main` and `release/*`, linear
   history, pull requests only, squash merge only, the checks **Linux**, **Windows** and **CLA**
   on an up-to-date branch; only the owner creates `v*` tags, which are never updated or deleted.

**Implementation:** `.github/workflows/ci.yml` runs the README's checks on pull requests and pushes
to `main` and `release/*`: on Linux formatting, Clippy, tests, the build, both documentation
builds and the CLA script's tests; on Windows (MSVC) Clippy, tests and the build. It uses only
GitHub's own actions pinned to commits (`actions/checkout` v7.0.1, `actions/cache` v6.1.0,
`actions/setup-node` v7.1.0) and installs the stable toolchain with `rustup`; it runs with a
read-only token and no secrets. Pushes to `main` and `release/*` save the Cargo registry and the
dependencies' build artifacts in the cache; pull requests only read it. The CTB writer version
packs only the numeric part of the version, so a pre-release suffix such as `-rc.1` does not
change it.

**Alternatives:** one release branch per minor version, with minor versions released from `main`,
brings features to users sooner but multiplies the branches to maintain. A single `main` cannot
fix a released version without shipping unfinished work. A permanent `develop` branch beside
`main` adds a merge step that gives one maintainer nothing.

**Consequences:** CI on GitHub's Windows runners executes the tests natively on every pull
request, but does not replace the complete native Windows validation and a run on real hardware
before a release. Agents work on topic branches and never push to `main` or `release/*`.

**Tests:** the CTB writer version of `1.2.3`, `1.2.3-rc.1` and `2.0.0-dev`, and its limit of 255
per part.

## TD-150 — Release builds and distribution

**Status:** decided by the owner on 2026-11-12. The first preview, 0.1.0, waits for the owner's
go-ahead.

**Context:** the packages of TD-141 were built by hand on the owner's machines. A release made
that way cannot show which sources it came from, and depends on one machine's state.

**Decision:**
1. A tag `vX.Y.Z` starts `.github/workflows/release.yml`. It fails unless the tag is `v` followed
   by the version in `Cargo.toml`; it builds the release executables and the packages of
   `tools/packaging` from the tagged commit on GitHub's runners (Windows Server 2025 with its Inno
   Setup 6.7; Ubuntu 22.04, so the Linux executables need glibc 2.35 at most), with no cache;
   it writes `SHA256SUMS`, records a build provenance attestation for every file and opens a
   **draft** release with the files, the download notes (`tools/packaging/release-notes.md`) and
   GitHub's list of merged pull requests.
2. The owner installs the draft's packages and runs the application on real hardware before
   publishing it. A draft that fails is not fixed by moving the tag: the next version follows.
   Versions `0.x` and those with a pre-release suffix are published as pre-releases.
3. GitHub Releases is the only channel for now. winget and Flathub are considered after 1.0, if
   users ask for them; the Microsoft Store and an APT repository only on demand.
4. No code-signing certificate until the public response justifies its cost (owner, 2026-11-12):
   the Windows files stay unsigned and the release notes explain Windows' warning. Certum Open
   Source Code Signing remains the route when it is wanted
   ([research](research/CODE-SIGNING-RESEARCH.md)); its key cannot be used unattended, so the
   owner would sign the executables and the installer on Windows between the build and the
   draft.
5. No automatic update. A check for a newer release, off until the user turns it on, is a later
   change of the application.

**Implementation:** the build jobs run with a read-only token and no secrets, like CI; only the
draft job, for tags, may write releases and attestations. Pull requests that change
`release.yml` or `tools/packaging`, and a manual run, build the packages as workflow artifacts
(kept 7 days) without releasing anything. Actions: GitHub's own, pinned to commits
(`actions/checkout` v7.0.1, `actions/setup-python` v7.1.0, `actions/upload-artifact` v7.0.2,
`actions/download-artifact` v8.0.2, `actions/attest-build-provenance` v4.2.2). Users check a
download with `sha256sum --check` and `gh attestation verify`.

**Alternatives:** building and uploading the packages by hand leaves no record of their sources;
publishing straight from the tag skips the trial on real hardware; a signing certificate now
costs money before anyone has asked for the software.

**Tests:** the workflow's pull request run builds every package from the branch; the artifacts
are installed by hand before the first tag.

## TD-151 — Beta releases and version constraints

**Status:** decided by the owner on 2026-11-12.

**Decision:**
1. Experimental versions are beta releases: tags `vX.Y.Z-alpha.N`, `-beta.N` and `-rc.N`, built and
   drafted by the release workflow like any release and published as pre-releases
   ([RELEASE-PROCESS.md](RELEASE-PROCESS.md#beta-releases)). Alphas and betas of the next major
   are tagged on `main`, betas of a series' next minor and release candidates on `release/N.x`.
   They use the stable version's settings and profile library.
2. After every release the branch moves at once to the next version with `-dev`, so that builds
   from source never carry a released version.
3. A version has at most 23 characters: the GOO header keeps it in a 24-byte field with its
   terminating zero (Photon Workshop in 32 bytes). A compile-time assertion in
   `karasu_core::print_job` fails the build for a longer version, instead of every GOO export
   failing at run time.
4. The `.deb` carries the version with `~` in place of `-` (`1.0.0~rc.1`): dpkg reads a hyphen as
   the start of a Debian revision and would order `1.0.0-rc.1` after `1.0.0`, so the final
   release would not upgrade a release candidate. File names keep the Semantic Versioning form.
5. Nightly builds are not published for now (owner): publishing one every night in this
   repository would crowd its releases and notify its watchers every night, and the owner does
   not want a separate repository. If they come, they need a short version
   (`0.2.0-nightly.20261112` fits; the commit goes elsewhere), portable packages only, and their
   own settings and profile folders, so that they cannot change a stable installation's data.
6. Until the software is declared stable, it warns that it is beta software, to use with care
   (owner, 2026-11-13): a failed print can leave cured resin in the vat that tears the release
   film or cracks the screen. The top of every pre-release's notes
   (`tools/packaging/beta-warning.md`) and its packages' README carry the warning; a
   pre-release is a 0.x version or one with a suffix, the rule that already makes it a GitHub
   pre-release, so the release workflow and `package.py` add it from the version alone and a
   stable version carries none. The repository's README carries it until the first stable
   release, which removes it by hand.

**Tests:** the GOO version field holds 23 characters and refuses 24, and the longest planned
pre-release forms fit; a 31-character version fails the build (checked by hand);
`tools/packaging/test_package.py`, run by CI, checks the Debian versions and that `dpkg` orders
alpha, beta, rc, release, patch, `-dev` and the next release as Semantic Versioning does, which
versions are pre-releases, and that the packages' README carries the beta warning only before a
stable version.

## TD-152 — Dependency audit before the first release

**Status:** done on 2026-11-12, at the owner's request, in the review before the first release.

**Findings** (`cargo audit`, RustSec database of 2026-11-12; "shipped" means compiled into
`karasu` or `karasu-cli` for Windows x64 or Linux x86-64, checked with `cargo tree --target`):

| Advisory | Crate | Shipped | Action |
|---|---|---|---|
| RUSTSEC-2026-0384, vulnerability (race in `time::Interval`) | `tokio` 1.54.0, also yanked | no: only `wasm-bindgen-futures` for the web target | 1.54.1 |
| RUSTSEC-2026-0486, unsound (`xattr` with an empty buffer) | `rustix` 1.1.5 | no | 1.1.6 |
| yanked | `clipboard-win` 5.4.1 (egui's clipboard) | Windows | 5.4.3 |
| yanked | `async-executor` 1.14.0 (Linux accessibility) | Linux | 1.14.1 |
| RUSTSEC-2026-0370/0371/0372, unmaintained | `zvariant`, `zvariant_derive`, `zbus_names` | Linux (screen-reader support through AccessKit) | kept, as in the M6.0 review; no replacement upstream yet |

**Decision:** the four crates are moved to the patch versions above with
`cargo update --precise`; no crate is added or removed (401 packages), and Cargo resolves `errno`
and `rustix` to the `windows-sys` 0.52 already in the lock file. After the update `cargo audit`
reports no vulnerability, only the three unmaintained warnings. Dependabot reported none of these
findings, so `cargo audit` stays part of the review before every release.

**Tests:** the complete checks on Linux and CI on Linux and Windows build and test the updated
lock file.

## TD-153 — Panel orientation verified against the manufacturers' slicers

**Status:** Implemented on 2026-11-14 at the owner's request, after ELEGOO SatelLite showed
karasu-slicer's Saturn 4 Ultra 16K file mirrored.

**Observation (2026-11-14):** the owner opened karasu-slicer 0.1.0's GOO for the Saturn 4 Ultra
16K (HillGiant) in ELEGOO SatelLite 1.0.2.43, which showed the model mirrored in X. SatelLite
showed ChiTuBox's file for the same printer (TD-104), whose stored image and mirror bytes are
karasu-slicer's, mirrored too, and its own export of HillGiant, opened again, turned 180 degrees:
what SatelLite shows of an opened GOO file is no guide to the printed part. Its own export
(SHA-256 `27f48b45…53f5`) declares mirror X 0, Y 1 and stores Lychee's image, karasu-slicer's
turned 180 degrees: the slicers compared all keep the part's handedness and differ in which side
of the plate it faces and in the declaration. How each firmware reads that declaration is not
known, so the owner decided that each printer follows its manufacturer's slicer exactly.

**Method:** a chiral test model, an "F" 20 × 30 × 2 mm (`tools/orientation-check/make_f.py`),
exported by the owner unrotated, centred, without supports, at 0.05 mm and without
anti-aliasing from ELEGOO SatelLite 1.0.2.43 for its twelve printers and from Anycubic Photon
Workshop 4.2.1.20 for its twenty-two, and by karasu-slicer from the catalog
(`export_karasu.py`). `compare.py` scores one layer's silhouettes under the four panel flips, in
place and centred (TD-143's method, every format). The files stay outside the repository; their
SHA-256 begin as listed.

| Printer | Manufacturer's file | Declared X, Y | karasu-slicer before | Change |
| --- | --- | --- | --- | --- |
| Jupiter SE, Mars 5, Mars 5 Ultra, Saturn 4, Saturn 4 Ultra | `059ff71f…`, `d6edbcd1…`, `78911984…`, `5b38bcfe…`, `273ae220…` | 0, 1 | same image (0.97–1.00), 0, 1 | none |
| Mars 4, Mars 4 Ultra, Saturn 4 Ultra 16K | `12e4617a…`, `a7d7767d…`, `ad5e69e0…` | 0, 1 | turned 180° (1.00), 1, 0 | no mirroring (revision 3) |
| Saturn 3 Ultra, Mars 4 Max | `d3a33a4e…`, `db74f104…` | 1, 0 | turned 180° (0.98–0.99), 0, 1 | mirror X and Y (revision 2) |
| Saturn 3 | `9d8fc1f6…` (encrypted CTB) | — | not compared | none (TD-143) |
| Jupiter 2 | `dbef15eb…` (GOO V5.2) | — | not compared | none, unverified |
| 16 Anycubic printers¹ | `4b703b46…` and others | — | same image (0.99–1.00) | none |
| Photon, Photon S, Photon Zero, Photon Mono, Photon Mono SQ | `1b43cad5…`, `38547ea5…`, `3b909a0a…`, `0f2a2381…`, `bfe32f61…` | — | same image on an upright panel (0.98–1.00) | upright panel (revision 2) |
| Photon P1 | `5ca4ec0c…` (vector layers) | `rotate_z` 180 | not compared | none (TD-104) |

¹ D2, M3, M3 Max, M3 Plus, M3 Premium, Mono 2, Mono 4, Mono 4K, Mono M5, M5s, M5s Pro, Mono X,
Mono X 6K, Mono X 6Ks, Mono X2, Ultra. The other flips scored 0.59 at most everywhere.

**Findings:**
- No printer was mirrored. ELEGOO's own sources disagree by 180 degrees: SatelLite turns the
  Mars 4 and Mars 4 Ultra the other way from ELEGOO's test files written with Voxeldance Tango and
  CHITUBOX (TD-143), and the Mars 4 Max the other way from its test file's declaration; each file
  declares its own image truthfully.
- Photon Workshop declares five older panels upright: the image's width is the short side
  (1440 × 2560 for the Photon and Photon S, 480 × 854 for the Zero, 1620 × 2560 for the Mono,
  2400 × 2560 for the Mono SQ) and the plate's X lies along it, the part keeping its orientation
  in the image. karasu-slicer declared them as the manufacturers list the panels, long side
  first.
- Photon Workshop 4.2.1.20 writes the Photon P1's layers (and the Photon P1 Max's, a `.pp1m`
  not in the catalog: 11520 × 8640 pixels, 285.5 × 214 × 300 mm, `rotate_z` 180) as `pwszImg`
  members whose first bytes are the part's outline box in millimetres, not as images. They were
  not compared; the P1 stays verified by TD-104 and its print (TD-117).
- SatelLite writes the Saturn 3's file as encrypted CTB (magic `0x12FD0107`), which is not read
  (its encryption is not bypassed), and the Jupiter 2's as GOO version 5.2, whose layout
  karasu-slicer neither reads nor writes. Whether the Jupiter 2 reads the GOO V3 files
  karasu-slicer writes is not established.
- Photon Workshop writes newer format revisions than karasu-slicer for seven printers (517 for
  karasu-slicer's 516: D2, M3 Plus, M3 Premium, Mono X 6K, Mono X2; 516 for the Mono X's 1; 515
  for the Mono's 1). Whether their firmware reads the older revisions is not established
  (TD-063).

**Decision (owner, 2026-11-14):**
1. Each printer's panel orientation follows its manufacturer's slicer: the stored image and the
   declaration (GOO mirror bytes, PP1 `rotate_z`), so that its files print as that slicer's do
   however the firmware reads the declaration. ChiTuBox, Lychee and test files written by other
   slicers no longer decide.
2. The Mars 4, Mars 4 Ultra and Saturn 4 Ultra 16K keep no mirroring (revision 3); the Saturn 3
   Ultra and Mars 4 Max mirror in X and Y (revision 2); the Photon, Photon S, Photon Zero, Photon
   Mono and Photon Mono SQ exchange their panel's sides and the plate's X and Y (`UPRIGHT` in
   `tools/assets/make_catalog.py`, revision 2). A new export of each matches the manufacturer's
   file as stored (0.97–1.00), declaration included.
3. 33 of the 34 printers are verified (the Saturn 3 by TD-143, the Photon P1 by TD-104); the
   Jupiter 2 is not. The README's table says so.
4. `tools/orientation-check/` is kept for the next printers and slicer versions; CI runs its
   tests.

**Consequences:** exports for the ten changed printers change. Library copies of their earlier
revisions keep their orientation and are offered the new revision (TD-106), never changed on
their own. The five upright plates show their short side along X. A manufacturer's file shows
what that slicer writes, not what the printer does: a print on each printer remains the proof,
and the README's table records the printers printed.

**Confirmed in SatelLite (2026-11-14):** the test "F" exported by karasu-slicer from the merged
catalog (d264786) for the Saturn 4 Ultra 16K declares mirror X 0, Y 1 and matches SatelLite's
file as stored (1.00). The owner opened it in SatelLite 1.0.2.43, which showed it turned 180
degrees exactly as it shows its own file, no longer mirrored.

## TD-154 — The Photon P1 Max and its `.pp1m` archive

**Status:** Implemented on 2026-11-14 at the owner's request; the printer stays "not verified".

**Observation:** Photon Workshop 4.2.1.20's files for the same test model (TD-153), for the
Photon P1 (SHA-256 `5ca4ec0c…`) and the Photon P1 Max (`25af4e3d…`), are the same archive: the
same members, settings schema, compression, `key_image_format` `pwszImg`, `max_file_version`
518, `property` 119, `raster_antialiasing` 4, `rotate_z` 180, and identical firmware constants.
The P1 Max's differs in `key_suffix` (`pp1m`), the machine name ("Anycubic Photon P1 Max"),
the panel (11520 × 8640 pixels of 24.8 µm), the sizes (285.5 × 214 × 300 mm), one
`child_screen` over the whole panel and one platform height (karasu-slicer already writes both
for the P1), `print_exposure_calibration_func` 3 (the P1's file has 1; karasu-slicer writes
neither), and its resins' values, which are Photon Workshop's presets and not used. Both files
hold each layer as an outline (`pwszImg` members), not as an image, so neither orientation
could be compared. Anycubic publishes the specifications (A29); its resin table (A8, read again
on 2026-11-14) does not list the P1 Max.

**Decision (owner, 2026-11-14):**
1. A format `anycubic-pp1m-v3` writes the Photon P1's archive under the P1 Max's machine name
   and suffix, with PW0 layer images as for the P1 (`Pp1Model` in `print_job::pp1`).
2. The catalog lists the Photon P1 Max (`anycubic-photon-p1-max`): 11520 × 8640 pixels of
   24.8 µm, the panel's 285.696 × 214.272 mm (the official 285.5 × 214 mm is smaller), 300 mm
   high, without print settings.
3. Its orientation is not verified. The profile mirrors in X and Y as the P1's, which stores
   what `rotate_z` 180 declares, the value Photon Workshop's file declares
   (`ASSUMED_ORIENTATION` in `tools/assets/make_catalog.py`).

**Known limits:** whether the P1 Max's firmware reads PW0 layer images, as the P1's does, and
whether it needs `print_exposure_calibration_func`, is not established; a print on the printer
will tell, and the printer report form asks for one.

**Tests:** an export for the P1 Max records the suffix `pp1m`, the machine name and PW0 layer
members; the format is listed with the others, and the README's table lists the printer.

## TD-098 — The application layout: menu bar, workflow bar, sidebar and fixed panels

**Status:** Accepted on 2026-10-23, owner decisions (M6.5 u2). The points marked open are
settled step by step as the interface moves into the layout.

**Context:**
- **A cluttered interface.** The [inventory](UI-INVENTORY.md) found one top row of about 20
  items, a preparation window fixed under the view cube that can run past the screen, and the
  printer, resin and format mixed in with the tools.
- **The earlier plan** (UI-DESIGN, M6-PLAN) had user-arranged docking panels (`egui_dock`) with
  frosted glass. The owner dropped the glass (2026-10-23) and now wants panels that cannot be
  moved, so the interface stays orderly.

**Decision:**
1. **Menu bar** at the top, the usual kind (File, Edit, windows, and so on). Its entries are to
   be decided; they go through the actions (TD-097).
2. **Workflow bar** under it, taller than today's bar:
   - the workflow's steps are buttons that stay lit when chosen, like today's Move, Rotate and
     Scale (for example Prepare, Hollowing, Supports, Export);
   - the chosen step shows its tools in the bar itself (for Prepare, for example transform,
     copy, mirror, auto-orientation; the tools are to be defined);
   - choosing a tool shows its panel;
   - the bar may collapse, but it must still show which step and which operation the user is
     in.
3. **Sidebar** at the left of the viewport: fixed, neither a panel nor a window, and closed
   sideways by the user to widen the viewport. It holds the printer (its facts and its choice),
   the resin's choice, and the list of the scene's objects.
4. **Viewport** in the centre, as today, with the view cube and the section bar on its right.
   The section bar gains a typed layer and the Up and Down arrows when it has the focus.
5. **Panels** are docked at the viewport's right, below the view cube, and cannot be moved. They
   appear for the tool chosen in the workflow bar. Design rule (owner, 2026-10-23): a panel
   shows all its content, scrolling within the viewport's height; it keeps a fixed width, cutting
   or wrapping long titles, texts and translations; its title is the tool's name only, never a
   file or object name; and it holds no messages, which go to a notification system. It keeps
   the same structure with or without a selection, its object controls disabled without one;
   a tool's values (for its next operation) are kept as user settings, while an object's
   values show only while that object is selected (UI-DESIGN, "Tool panels").
6. **Windows** keep today's behaviour: they move, close and collapse. They belong mostly to the
   menu bar, for example the user settings.
7. **Status bar** at the bottom, where it is, with fewer and better-ordered facts.
8. **Style:** as today, with larger buttons (more padding). Panels and windows are opaque, with
   no blur. The steps' buttons are pills; the tools' buttons are equal squares, an icon in the
   middle and the label in small capitals below.
10. **Icons** are original, drawn by us (owner, 2026-10-23): `tools/assets/make_icons.py` draws
    each as a few strokes on a 24-unit grid and writes its SVG source and a 96-pixel white PNG
    to `assets/ui/icons/`; the application tints the PNG with the theme's colours, so no SVG
    renderer is added. Interface artwork lives in `assets/ui/images/`.
9. **No docking crate.** Panels are fixed, so `egui_dock` is not adopted; the layout is our own.

**Open (recommendations given, owner to confirm):**
- **The collapsed workflow bar:** the steps' row always shown, with the active step lit; the
  tools' row collapses, and the active tool's name then shows beside the steps.
- **A "select" tool:** decided (owner, 2026-10-23): Prepare starts on Select, a neutral tool
  that shows no handles and no panel (and closes the previous one). Selecting works as with
  every tool, by a click on the model or in the sidebar.
- **Collapsible panels:** recommended, with the arrow as today, plus a scroll area and a height
  bounded by the viewport.
- **The menus' entries:** a first draft is File, Edit, View, Windows, Help.
- **The slicing and layers panel** shown after slicing: to be reviewed by the owner later.

**Consequences:**
- **Orderly by construction.** Each control has one place; panels follow the workflow.
- **Steps and tools are actions.** The workflow bar, the menus and the shortcuts invoke the
  same actions.
- **Supersedes** the user-arranged docking of the earlier plan.

## TD-097 — Interface actions, configurable shortcuts and persistent user settings

**Status:** Accepted on 2026-10-23, owner decisions, at the start of the interface review (M6.5).

**Context:**
- **Direct calls everywhere.** Each control of the interface calls the workspace directly. The
  layout review will put the same action in several places (a toolbar, a menu, a shortcut, a
  context menu), and each would repeat its enabling, its tooltip and its effect.
- **Fixed shortcuts.** The keyboard shortcuts are written in the window's code, and some keys
  fired with any modifier held (Ctrl+M switched to the move mode).
- **Nothing the user sets survives a restart** except saved profiles: the auto-repair choice,
  the appearance colours and the panels' states are lost.

**Decision:**
1. **Actions.** `karasu-app::actions` names what the user can do (`Action`), with its label and
   hint. The window decides when an action is enabled and performs it in one place; buttons,
   toggles, shortcuts and the view cube's home button all go through it, and each action taken
   is logged (`app`, at `debug`). More actions join as the layout review moves controls.
2. **Undo stays snapshot-based.** Actions are not commands that undo themselves: the plate's
   history keeps the content before each edit (TD-081).
3. **Shortcuts are data.** A `Keymap` binds shortcuts to actions, more specific shortcuts first,
   so Ctrl+Shift+Z is never taken by Ctrl+Z. The defaults are the window's shortcuts so far. A
   bare key no longer fires with Ctrl held. Shortcuts fire only while no widget holds the
   keyboard focus (a text field or a value being typed).
4. **Configurable shortcuts.** The settings panel will let the user change the keymap.
5. **Every user setting persists.** Whatever the user sets (shortcuts, colours and their
   switches, auto-repair on import, units, panel and layout states, and later settings) is
   kept across restarts, in the application's preferences root (`ProfilePaths::config`), in a
   versioned file. The store comes with the settings panel (both done 2026-10-26, below).

**Implementation of the store (2026-10-26, M6.5 u3, before the settings panel):**
- **File.** `settings.json` in `ProfilePaths::config`, JSON with `schema_version` 1
  (`karasu-app::settings`). Every field has a default, so an earlier file reads with what it
  lacks at its defaults. A save writes `settings.json.partial` beside it and renames it over.
- **What it holds.** The colours and the selection outline's switch; the repair on import; the
  main window's state (windowed, maximized, fullscreen) and, as a window, its size and place;
  the sidebar, the tool panel, the workflow bar's tools row and the sidebar's sections; the
  resizable dialogs' sizes and the tool windows' places; the tools' settings (Transform's axes,
  locks and uniform scale, the Advanced switches, the hollowing's wall and cell, the next drain
  hole's diameter, depth and surface, the walls' verification, the Layers tool's following, and
  the editor's speed unit); and the printer, resin and print setting the plate last used (owner,
  2026-10-26), by their library identities. A new session starts with them while the printer is
  still in the library, the resin still set up for it and the setting still one of theirs (else
  the printer's first resin and setting), and the plate's history is cleared after, so Undo
  does not take them away. Shortcuts join with the panel that changes them: stored now, they
  would freeze today's defaults.
- **Never silently replaced.** A file that cannot be read, or a newer version's, is reported
  (a notification, with the path and the reason) and left as it is; the session runs on the
  defaults, or on what a newer file's known fields hold, and saves nothing over it. A failed
  save is reported once and tried again.
- **When.** Applied as the window is created; compared four times a second (not every frame)
  and saved one second after a change rests, and at once when the window closes. A smoke test
  neither reads nor writes the user's settings. `--window` overrides the kept state.
- **Window placement.** The main window reopens at its kept size and place when a title bar's
  worth of it shows on the monitor, else centred; a monitor's position is not known to egui,
  so a window left on a secondary monitor comes back centred.
- **egui's own memory is not serialized:** it would need egui's `persistence` feature and the
  `ron` crate. The dialogs and tool windows read and write their sizes and places in a small
  record kept in the egui context (`dialog::Remembered`), which the store saves.
- **Dependencies.** `serde` and `serde_json` in `karasu-app`, at the versions the headless
  crates already use: nothing enters the dependency tree.

**The settings dialog (2026-10-26, owner mockup):** File › Settings… (Ctrl+,), a resizable
dialog with Appearance, Keyboard shortcuts, Opening models, Window, Language and Settings file
(`docs/UI-DESIGN.md`, "Settings"). It replaces the temporary colours window and the File menu's
repair-on-import entry. Shortcuts are kept by each action's stable identity (`Action::id`), only
where they differ from the defaults, so a later change of a default reaches users who kept it;
an unknown action or key in the file is left out and reported. The language is a setting
(`--locale` overrides it for one start); the pseudo-locale is offered only with `--dev`, the
developer mode. "Start over" keeps an unreadable or newer file beside the new one, as
`settings.json.unreadable` or `.newer`.
**Consequences:**
- **One place per action.** Moving a control in the layout review moves only where it is drawn.
- **Testable without a window.** The keymap's matching and precedence are tested headless.
- **Settings to store.** The settings panel must load and save every setting and report a file
  it cannot read rather than silently start over.

## TD-096 — Persistent session logs

**Status:** Accepted on 2026-10-23, owner decisions. Plan: [LOGGING-PLAN](LOGGING-PLAN.md).

**Context:**
- **Users need something to send.** When a user meets a problem, we need a record of what
  happened.
- **Nothing persists today:**
  - the application's messages are kept in memory only;
  - its standard error is lost on Windows, where the Release build has no console;
  - a panic leaves no trace, which is how a GPU buffer over wgpu's limit at 0.2 mm (TD-094)
    had to be diagnosed by inference.
- **A live view is planned.** The owner also wants a terminal-like window that shows the log
  live, built later with the interface review.

**Decision:**
1. **Facade and logger.** The `log` crate is the logging facade, and our own logger in
   `karasu_project::logging` implements it. It also collects egui's, wgpu's and winit's records,
   which are already in the tree. The geometry core does not log.
2. **Session file.** The application writes one file per session to `logs/` in its cache root.
   - Retention: the 10 newest sessions are kept.
   - Size: each file stops at 20 MB, with a final line saying so.
   - Writing: buffered, flushed on warnings, errors and panics, and otherwise every second.
3. **Ring.** The last 10,000 records are kept in memory, with sequence numbers, for the live
   window to read without per-frame cost. The window came with the interface review
   (2026-10-26: Windows › Log, with the logs folder and a diagnostic report; LOGGING-PLAN,
   section 5).
4. **Command line.** It logs to a file only with `--log <file>`.
5. **Levels.** Our records are `info` and above by default, third-party records `warn` and
   above; `KARASU_LOG` changes our level.
6. **Panics.** A panic hook records the message, the place and a backtrace before the default
   hook runs.
7. **Format.** Records are English plain-text lines, with the UTC time, the time since start,
   the level, the area and the message.
8. **Behaviour.** Logging never changes results or outputs, and nothing is logged per frame.
9. **Privacy.** Logs stay on the computer. They hold the opened files' paths and the hardware's
   names, and sending them is the user's choice.

**`log` 0.4.34, reviewed:**
- **Source and build:** pure Rust, MIT OR Apache-2.0, no build script.
- **Dependencies:** none by default, and none of its optional ones is enabled.
- **In the tree already:** through egui, wgpu and winit, so no crate is added to the lock file
  (only our three crates' dependency lists name it).
- **Maintenance:** maintained by the Rust project, released in August 2026.
- **Platforms:** Windows and Linux.

**Consequences:**
- **Support:** a user can send the log of the session where a problem happened. A panic is
  recorded with its backtrace.
- **New dependency:** `karasu-project` takes `log` as a direct dependency.

**Results (done, 2026-10-23, g1 to g4):**
- **Application:** it keeps a session file per run. On Windows these are in
  `%LOCALAPPDATA%\karasu-slicer\logs`, with the session's facts, the adapter, the memory budget,
  the work done and the messages shown; other libraries' warnings are recorded too.
- **Panics:** they are recorded with a backtrace, which is tested on Linux and on Windows.
- **Command line:** `slice --log <file>` writes the job's log, and outputs are byte-identical
  with and without it (507 comparison files).
- **Cost:** logging costs no measurable time on `Base_01.stl`.
- **Details:** [LOGGING-PLAN](LOGGING-PLAN.md#progress).

## TD-095 — A raster-side wall verifier, on demand

**Status:** Accepted on 2026-10-22, owner decisions. Plan:
[RASTER-VERIFIER-PLAN](RASTER-VERIFIER-PLAN.md).

**Context:**
- *What is certified today:* TD-093 certifies the hollowed mesh, the wall between the cavity and
  the source surface.
- *What prints:* the layer masks, after slicing, rasterization, antialiasing and the drain-hole
  cuts.
- *Earlier checks:* the commercial comparison checked exports with research scripts, and found
  CHITUBOX's 0.1 mm wall breached in four layers without a message. Those scripts were 2D, per
  layer, and outside the product.

**Decision:**
1. **What it checks.** A verifier reads only layer masks. It measures the thinnest printed wall
   between the cavity and the outside by an exact Euclidean distance on the print lattice:
   - a separable in-plane transform;
   - an exact combination across layers within the wall;
   - variable layer heights included.
2. **What it reports:**
   - the thinnest printed wall (measured, a guaranteed lower bound, and where);
   - breaches, meaning expected cavity that reaches the outside other than through a drain hole;
   - undrained pockets;
   - a map of all three on the layers.
3. **Phase 1 checks Karasu's own layers.** The drain holes' cuts and the expected cavity are
   rasterized alone beside the print, so a breach is told from a hole. Layer files from other
   slicers (starting with PP1) are a second phase and a separate decision.
4. **On demand only:**
   - `slice --verify-walls` on the command line;
   - a "Verify walls" action on the sliced result in the interface; since M6.5 the Slice
     panel's "Verify the printed walls" switch, a setting of the tool, off by default.

   It never slows ordinary slicing.
5. **Failure policy.** A breach, or a printed wall under the certificate by more than the
   lattice bound, contradicts the certificate. The file is still written so it can be
   inspected; the command line exits with a nonzero status, and the interface shows the
   finding as an error.
6. **Material convention.** A pixel is material when it covers at least half of its samples. On
   the lattice, `Base_01.stl` on the P1 has voxels of 0.017 × 0.025 × 0.05 mm.

**Consequences:**
- Without the option, outputs and timings are unchanged.
- The verifier also prepares Phase 2: the same measurement for any layer file.

**Results (phase 1 done, 2026-10-23):**
- **`Base_01.stl` from 2.0 to 0.3 mm:**
  - every printed wall is within one voxel diagonal of the certified one;
  - there is no breach;
  - a front drain hole drains the cavity, leaving only the repair's empty slivers as pockets;
  - verifying adds 23 to 29 s.
- **FX-11, FX-12 and the Yorick** at 2.0, 1.0 and 0.5 mm pass the same way.
- **A wall thinner than a pixel** (0.3 mm on 1 mm pixels) is found as a breach and fails the
  job.
- **Details:** [RASTER-VERIFIER-PLAN](RASTER-VERIFIER-PLAN.md#progress), v4.
- Its cost follows the model's volume at print resolution. On `Base_01.stl` on the P1 that is
  about 27 billion voxels, estimated at tens of seconds.

## TD-094 — Hollowing and drain-hole limits known in advance and sized to the machine

**Status:** Accepted on 2026-10-22, owner decisions after testing M6.4e in the interface.

**Context:** the owner hollowed `Base_01.stl` from the interface.
- **Thin walls:** walls under 0.3 mm were refused only after the whole hollowing ran ("hollowing
  resource unavailable: cavity triangles").
- **Holes:** at 0.3 mm a drain hole was refused ("cannot index … for drain holes"), and the
  refusal also removed the cavity already shown.
- **Why:** under about 0.6 mm the grid must stay under `wall / √3`, so the cavity grows as
  `1 / wall²`. The hollowing allowed 20 M cavity triangles, but the hole index, at about 308
  bytes per face under a 4 GiB cap, only 13.9 M: from about 0.27 to 0.32 mm the model could be
  hollowed and not drilled.

**Decision:**
1. **Holes unavailable are known at once.**
   - A preview checks, from the cavity's size alone, whether a hole index can be built, and
     reports why not (`ObjectPreparation::drainage_unavailable`).
   - Such an object, or one whose index fails, stays prepared and hollowed, without holes.
     Slicing it with holes is still refused.
   - The interface disables "Add drain holes" and says why.
2. **A leaner hole index.** The index's triangles are reordered in place instead of copied, and
   its peak is the larger of its two phases (component labelling, then the hierarchy) instead
   of their sum: 173 bytes per face instead of 308, so 24.8 M faces fit 4 GiB.
3. **One memory budget** (`SliceRequest::memory_budget_bytes`):
   - It caps the hollowing's payload and every hole index.
   - It caps the cavity at the faces an index can take, so a model that can be hollowed can be
     drilled.
   - Without a budget, the command line keeps today's limits: 20 M cavity triangles, 4 GiB.
4. **The interface sizes the budget from the installed memory.**
   - A preparation may use half of it.
   - The application's footprint was measured as 0.75 GiB plus 410 bytes per cavity triangle
     (`Base_01.stl` hollowed and drilled at 0.5, 0.35 and 0.3 mm: 3.2, 5.5 and 7.0 GB).
   - The budget is the index payload of the largest cavity that fits that half, and never fewer
     than 1 M cavity triangles.
   - The memory is read with `sysinfo`, see
     [the dependency review](research/M6-DEPENDENCY-REVIEW.md). User-set limits wait for the
     application's settings.
5. **The minimum wall is predicted, and warned about, not imposed.**
   - From the model's surface area, TD-093's law gives each wall's cavity; the panel warns, in
     red, below the thinnest wall whose cavity fits the budget, or when an explicit cell is too
     fine.
   - The prediction has a 10 % margin: the law fell up to 7 % short on its campaign. Without it,
     `Base_01.stl` at 0.19 mm was not warned about and was refused after hollowing.
   - Applying stays possible: the law over-predicts models whose thin parts leave no cavity.
     `Katana 14 handle.stl` at 0.5 mm was predicted at 22 M triangles and made 6.2 M.

6. **Large meshes are drawn in parts.** The viewport's device keeps wgpu's default limits, whose
   largest buffer is 256 MiB. A mesh whose vertex or index buffer exceeds the device's limit is
   split into parts of whole triangles, each with its own vertices, each buffer within the limit.
   - Before this, a cavity of more than about 22 M triangles made a buffer over the limit.
   - wgpu reports that as a validation error and ends the application: `Base_01.stl` at
     0.2 mm closed the application once hollowed.

**Consequences:**
- **Command-line outputs** are unchanged: the 507 comparison files are identical.
- **`Base_01.stl` at 0.3 mm** now hollows and drills from the interface.
- **The thinnest wall** the interface predicts for `Base_01.stl` is about 0.20 mm on a 31 GB
  computer, and 0.28 mm under the command line's default.
- **Native Windows** (`validate.ps1 -IncludeApp`, first with `-Online` to fetch the new crates)
  passed every gate: 1,257 tests in Debug and in Release, and the application's smoke runs.
  - One earlier run failed an application test in Debug.
  - The cause was a race between tests that shared a temporary model file; each test now has its
    own.

## TD-093 — Hollowing with a predicted cell and an exact wall certificate

**Status:** Accepted on 2026-10-22, owner decision, after the comparison in
[HOLLOW-CAVITY-DESIGN-COMPARISON](HOLLOW-CAVITY-DESIGN-COMPARISON.md) (sections 5.1, 5.2 and 9).
Planned as M6.4e; manual supports move to M6.4f and the mandatory preparation scenarios to M6.4g
(TD-092's placement and older references keep their wording). Plan:
[HOLLOW-CERTIFIED-PLAN](HOLLOW-CERTIFIED-PLAN.md).

**Context:**
- **Today's backend** (TD-045) hollows on a cell of `wall / 8`. The cavity grows as `1 / wall²`:
  - drain holes are refused on `Base_01.stl` under about 1.33 mm;
  - hollowing itself is refused under about 1.1 mm;
  - a 0.1 mm wall would need billions of triangles.
- **Its certificate is loose.** It widens vertex distances by each triangle's size. At 1.3 mm it
  proves 1.014 mm where the real wall is 1.292 mm, and on large models at 1 mm it proves
  0.01–0.05 mm.
- **The references** (CHITUBOX v1.4.0, Lychee 8.0.0) are fast, but they guarantee nothing, and
  CHITUBOX breached a 0.1 mm wall in four layers without a message.

**Evidence (measured, worktree experiment, never committed; raw results in
`<reference>/evidence/hollow-runtime/`):**
- **Exact certificate:**
  - every cavity triangle's distance to the source can be proven with the existing exact tests
    (`Bvh::clearance`, `Bvh::lower_bound`);
  - the real minimum equals the vertex minimum within 1 µm;
  - the proof costs about 0.3 to 1.5 times the hollowing.
- **Shortfall law:** the first-pass shortfall of the minimum wall follows
  `0.39 · cell² / wall` on 20 different models. It is the sagitta of a tetrahedron's chord on the
  rounded offset next to a reflex edge, `(cell · √3)² / (8 · wall)`, so it can be predicted before
  hollowing.
- **Campaign:** with the cell from an accepted over-thickness and a triangle budget, and the
  offset raised by the predicted shortfall, 80 runs on 20 models from 0.5 to 2 mm were all
  guaranteed in one pass. The only run without a pass had nothing to hollow, which is correct.
  - *Time and memory:* 0.1–23 s, at most 1.35 GB.
  - *Cavity size:* at most 13 M triangles, under the drain-hole index.
  - *`Base_01.stl` at 1.3 mm:* 6.7 s and certified 1.311 mm, against today's 19.4 s, certified
    1.014 mm and refused drain holes.
  - *`Base_01.stl` at 0.5 mm:* certified 0.5125 mm in 11.8 s.

**Decision (owner, the comparison's suggestions):**
1. **Approach:** keep the sampled backend and every consumer of the cavity mesh.
   - Replace the loose certificate by an exact per-triangle proof.
   - Choose the cell and the offset by the predicted rule.
   - Adaptive extraction (the comparison's option A) comes later, for thin walls on large models.
   - Print-grid hollowing (option R) is not adopted.
2. **Cell:** `max(√(τ · wall / 0.39), √(7.9 · A / N))`, below `(wall − 2 · error) / √3`, with:
   - `τ = 0.05 mm`, the accepted over-thickness;
   - `A`, the source's surface area;
   - `N = 10,000,000` cavity triangles, under the 14.3 M drain-hole index.

   An explicit `--hollow-cell` still overrides it.
3. **Offset:** `wall + 0.40 · cell² / wall`. The coefficient was never exceeded in the campaign
   (largest measured ratio 0.37); the fallback below covers models that would exceed it.
4. **Guarantee:** the certified wall is always at least the requested wall.
   - *Fallback:* when a pass falls short, the offset grows by the shortfall. When a pass is
     refused or does not converge, the cell is halved.
   - *Stop:* "nothing to hollow" ends the run at once.
   - *Bounds:* at most a fixed number of passes and the triangle budget; then the run is refused,
     naming the best certified wall reached.
   - *Report:* the certified wall and the thickest spot.
5. **Minimum wall:** no fixed limit. The interface warns under 1 mm; refusals give their reason.
6. **Simplification (TD-066):** removed from the interface (no longer needed for drain holes);
   kept in the command line.
7. **Later:** the raster-side verifier, as a separate feature.

**Consequences:**
- **Command-line outputs of hollowed cases change** (cavity, volume, summary lines). This TD
  records the change. A new comparison baseline is taken once the implementation is measured.
- **Hollow plan records** carry the rule's version and parameters, so caches and bound documents
  never mix the two backends' results.
- **Command-line limits:** `--hollow` accepts any wall the rule can certify, within the budget.

**Progress:** see [HOLLOW-CERTIFIED-PLAN](HOLLOW-CERTIFIED-PLAN.md#progress). Command-line change
recorded at h3:
- **Six hollowed cases change** (c02, c06, c10, c13, c18, c20; every other file is identical):
  - *Cavity:* it is extracted at the guarantee's offset on the rule's cell (0.506 mm for a 2 mm
    wall).
  - *Summary:* it adds the guaranteed wall, the offset, the passes and the thickest wall at the
    cavity's vertices.
  - *Holes:* surface holes reach the offset.
- **Walls, resin and time:** each guaranteed wall is at or above the request, the resin grows
  about 2.4 %, and the hollowed cases run 1.5 to 3.7 times faster.

**Results (M6.4e done, 2026-10-22):**
- **The twenty models** of the experiment, hollowed by the product code at 0.5–2 mm: 79 of 80
  runs guaranteed in one pass. The 80th has nothing to hollow and is refused as such.
- **Over-thickness:** guaranteed walls are 0.003–0.037 mm above the request, and the thickest
  spots 0.17–0.42 mm above it.
- **Cost:** 251 s for all runs (328 s in the experiment). The longest run is 22.9 s. Peak memory
  is at most 1.50 GB, the one run over the plan's 1.4 GB target.
- **`Base_01.stl`:**
  - at 1.3 mm, 4.9 s and 2.35 M cavity triangles instead of 10.5 s and 15 M; guaranteed at
    1.3042 mm where 1.014 mm was certified; drained from the command line and the interface
    where holes were refused;
  - at 1.0 and 0.5 mm, both refused before, it is now guaranteed.
- **Interface:** every reference fixture hollows from the panel at 1 mm, except FX-10b, which is
  refused as non-manifold.
- **Native Windows:** every gate passed, the application included (`validate.ps1 -IncludeApp`).
- **Details:** [HOLLOW-CERTIFIED-PLAN](HOLLOW-CERTIFIED-PLAN.md#progress), h5.

## TD-092 — Repair everything, never silently, under the user's control

**Status:** Accepted on 2026-10-21, owner decision; the plan is in [REPAIR-PLAN](REPAIR-PLAN.md).
The owner approved its placement as M6.4d (manual supports become M6.4e and the mandatory
scenarios M6.4f), the defaults (auto-repair on import `Exact`, the command line unchanged) and the
presets.

**Context:** a model a user cannot print with Karasu is a usability failure. The
repair comparison shows that commercial slicers repair
silently with fixed tolerances (CHITUBOX snaps every vertex to 1 µm and deletes small shells) and
report partial fills as success, while Karasu repairs only exactly and refuses what it cannot
close, as with `Base_01.stl` (TD-091).

**Decision (owner):**
- Implement every repair, including aggressive ones, up to rebuilding the surface.
- Never repair silently: every change is counted, located and shown, and every remaining defect is
  named with its effect.
- The user controls automatic repair and its tolerances: exact repairs first, tolerances (as
  CHITUBOX's welding) when needed, set by the user and reported. This degree of freedom is one no
  reference offers.
- The interface leads the user to a printable model: a stronger repair or an explicit, recorded
  bypass of a small defect, never a false "everything is fine".

**Consequences:** the repair stages, presets, report, printability states and guided flow of the
plan; tolerant hollowing for measure-zero defects and overlapping shells; a crafted defect corpus.
The command line's default stays as it is, so earlier outputs do not change.

**r1, foundations (2026-10-21):**
- **Diagnosis (R0):** the defect report tells needles from caps, finds cracks, pinch vertices and
  components under 1 mm², and keeps up to 16 positions per class. Self-intersections are a
  separate search on request (exact contact test over a BVH, bounded), since it costs several
  times the report.
- **Report:** every change of duplicate collapse, orientation and hole filling is located (up to
  16 positions per kind), refusals included; the fill report gives the largest cap.
- **Presets:** `Off` and `Exact` (every repair that moves no vertex: duplicates, orientation,
  crack closing, hole filling with refused holes reported, outward). Presets with tolerances come
  with their stages, so no preset names a repair that does not exist.
- **Printability:** `Ready`, `ReadyWithNotes`, `NeedsRepairForHollowing`, `NeedsRepair`, derived
  from the checks slicing, rasterizing and hollowing apply, for the mesh as diagnosed. Supports
  are not part of the state: they do not depend on the model's topology today.
- **Per object:** a repair operation first in the object's timeline replaces the plate's repairs
  for that object. Plate documents keep the job's repairs (a later document version).
- **Command line:** `--repair off|exact`; `inspect --diagnosis` appends the extended diagnosis,
  the state, and the exact preset's preview with its positions and the state after it. The
  default and every existing output are unchanged.
- **Measured** (Linux release, 16 threads, one run after a warm-up):

  | Model (faces) | Defect report | Diagnosis (report and repair preview) | Self-intersections | Exact repair |
  | --- | --- | --- | --- | --- |
  | Yorick (706,822) | 0.15 s | 0.38 s | 1.95 s (8 pairs) | 0.48 s |
  | `Base_01.stl` (1,009,544) | 0.22 s | 0.51 s | 3.80 s (83,424 pairs) | 1.46 s |

  `inspect --diagnosis` takes 3.5 s on the Yorick (1.8 s without) and 7.6 s on `Base_01.stl`
  (2.0 s without). The self-intersection search is single-threaded; its parallel traversal
  belongs to the r9 budgets.
- **Finding:** `Base_01.stl` has 2,696 pinch vertices and 83,424 self-intersecting face pairs:
  it is made of overlapping shells, which is what tolerant hollowing (H2, r3) must handle.

**r2, welding, degenerate faces and cracks (2026-10-21):**
- **R1 welding** merges vertices within a tolerance into clusters in which every two members are
  within it, onto the cluster's smallest index: no vertex moves farther than the tolerance and no
  two points farther apart merge (property test). It merges **only vertices on open edges**.
  Welding every vertex at 1 µm, as CHITUBOX snaps, gave the clean Yorick miniature three
  non-manifold edges and a pinch vertex where two parts pass within 1 µm; welding closes gaps,
  and only open edges have them. Faces whose corners merge are removed and reported.
- **R2 degenerate faces:** faces with a repeated vertex are removed (they are segments; nothing
  else changes). With a tolerance, short edges collapse only under the link condition and when no
  other face turns over, and flat faces flip with their neighbour only when both new faces are
  well shaped, so nothing flips back; faces left within the tolerance are counted.
- **R3** needs no new code: duplicate collapse runs after R1 and R2 and cancels the coincident
  reversed faces they leave.
- **R4:** a crack tolerance zips rims within it of a line, splitting at the vertices within it of
  each rim edge, with the largest offset reported. And crack closing (TD-091) now accepts any
  result whose edges around the crack are used equally in both directions, instead of exactly one
  face per side: splitting keeps every face's region and removes only zero-area faces, so the
  winding number cannot change, and a balanced surface defines it everywhere. This closes the
  fourth crack of `Base_01.stl`, in a knot where the split's new edge already existed: it becomes
  one balanced non-manifold edge, as where bodies touch.
- **Presets:** `Exact` gains exact welding and needle removal; `Standard` adds welding,
  degenerate faces and cracks at 1 µm. Tolerances range from 0 to 1 mm and are recorded in the
  plan only when set, so earlier records are unchanged. Command line: `--repair standard`, list
  items `weld[=mm]`, `degenerate[=mm]`, `cracks=mm`; `inspect --diagnosis` previews both presets.
- **Results** (Linux release):

  | Model | Exact | Standard | After |
  | --- | --- | --- | --- |
  | Yorick | 0.51 s, no change | 0.83 s: 255 flat faces flipped, 28 short edges collapsed, 65 left | closed, ready (as before) |
  | `Base_01.stl` | 1.58 s: 4 cracks closed | 1.86 s: 2,732 flat faces flipped (one closing a crack), 5,267 left, 3 cracks closed | closed; 1 balanced non-manifold edge, 2,695 pinch vertices |

  `Base_01.stl` is hollowed (2 mm wall, cavity 281,539 mm³) and sliced after `--repair
  standard` with `--reversed-tolerance 0.05`: self-intersecting folds leave reversed slivers of
  0.0017 mm² in five layers, which rasterizing refuses even with the default repair. The
  printability state cannot see them; `inspect --diagnosis` notes when self-intersections exist.

**r3, tolerant hollowing (2026-10-21):**
- **H1, defects of zero measure:** admission closes exact zero-area cracks (TD-091's split, in a
  crack-only mode of hole filling) and leaves out faces of zero computed area, then requires a
  balanced surface as before. Neither changes any point's inside or outside state, nor encloses
  material. Both are reported (`Hollowing left out as zero measure`); the shell's exterior is the
  source without them. The pipeline asks for `fill-holes` only for holes other than cracks.
- **H2, overlapping bodies:** the interior is a positive winding number instead of exactly one.
  Distances stay those to every source face, so every surface keeps its wall: overlapping bodies
  get one void per region (the overlap's void separated from the others by the walls of the
  surfaces crossing it), and no void contains a source face, so each has a constant winding
  number. The assembly subtracts each void that many times (its reversed surface repeated), so
  the shell's winding number is zero in every void, and a void inside one body gives the same
  shell as before. Hollowing the union's outer boundary only, without the inner walls, would need
  a Boolean union of the bodies, which the core does not have; the walls are reported through the
  cavity surface count, and each void needs its own drain hole.
- **S1:** drain-hole certification works on such shells unchanged (test: the overlap's void is
  drained); manual supports never depended on the model's topology.
- **Printability:** zero-area faces and cracks no longer need repair for hollowing; they are
  notes.
- **Result:** `Base_01.stl` is hollowed (2 mm wall, cavity 281,539 mm³) with the default repair
  and with none: 5 zero-area faces and 4 cracks left out. The Yorick hollows to the same bytes in
  the same time (1.6 s); the comparison suite is byte-identical.

**r4, non-manifold edges (2026-10-21):**
- **Rule (changed from the plan):** the plan kept, at an unbalanced edge, "the face pair closest
  to flat". FX-10B shows why that fails: its flap is folded onto the cube's front face, as flat as
  the face itself, so the rule could keep the flap and remove the cube's face. Instead, faces are
  grouped into parts joined through two-face edges; a closed part balances every edge, so an
  imbalance comes from an open part. An open part is removed as a **fin** when it is surplus at an
  unbalanced non-manifold edge (its own uses there point the way the edge has too many), its
  removal makes no edge more unbalanced and some edge less, and it is not the largest part on
  that edge. Smallest parts go first and the balance is updated after each removal. A flap or a
  hanging sheet goes whole; the body it hangs from, which balances its own uses, never qualifies.
- **Aggressive:** every face on an edge still unbalanced is removed, for hole filling to close.
- **Presets:** `Standard` gains fins; `Aggressive` (new) is `Standard` with the aggressive cut,
  and will gain the aggressive stages of r5 and r6. Command line: `--repair aggressive`, list
  items `fins` and `cut-non-manifold`; the plan records the mode under marker 0xF6.
- **Diagnosis:** an unbalanced non-manifold edge makes layers fail (FX-10B does not slice with
  the default repair), so the printability state is `NeedsRepair` rather than "slices"; the
  project's diagnosis reports fins left after duplicate collapse, so a doubled face, which that
  collapse resolves, is not taken for one.
- **Pinch vertices:** splitting them moves to r5, where hole filling needs it.
- **Result:** FX-10B slices and hollows (2 mm wall) after `--repair standard`: one fin of 100 mm²
  removed. The stage takes 0.12 s on the Yorick and 0.18 s on `Base_01.stl`, and changes neither.

**r5, holes (2026-10-21):**
- **Non-simple rims:** a rim through a vertex twice is split there into simple loops, each capped,
  and refused as a whole if one loop is. Capping both loops of a pinch restores the vertex's fan,
  so the pinch-vertex split the plan placed in R5 is not needed. No vertex is added, so `Exact`
  includes it (`split_rims`).
- **Refinement and fairing** (`refine`, new vertices): centroid splits where a triangle is larger
  than the edges around its corners suggest, with Delaunay edge swaps (Liepa 2003), then a
  membrane and a discrete thin plate (Kobbelt 1997) whose rim umbrella includes the mesh
  neighbours outside the cap. A membrane alone spans the rim flat: on the Yorick, a 108-edge hole
  on its convex top was refused for intersections until the thin plate, which continues the
  surface's slope, replaced it.
- **Large holes:** the minimum-weight triangulation costs O(n³); past 512 edges a refined cap
  starts from a centroid fan instead. `unbounded-holes` lifts the 256-edge bound.
- **Fallback:** a refined cap that meets the mesh is replaced by its unrefined triangulation, and
  the hole is refused only if that one meets it too. Reported (`unrefined_caps`).
- **Presets:** `Exact` splits rims; `Standard` adds refinement; `Aggressive` adds the unbounded
  bound. Command line: `split-rims`, `refine-holes`, `unbounded-holes` with `fill-holes`; the
  plan records them under marker 0xF7.
- **Measured** (Yorick, holes cut around its top; whole exact or standard repair, Linux release):
  1.5 mm (32 edges) 1.1 s exact, 1.5 s standard (8 vertices added); 3 mm (108 edges) 1.1 s and
  1.6 s (242 vertices); 4.5 mm (two rims) 1.1 s and 1.5 s; none adds a self-intersection. At 6
  mm, six interlocked rims: the 127-edge one and three small ones meet the mesh and are refused
  by every preset (4.1 s with refinement, each refusal rebuilding the contact index).

**r6, shells and orientation (2026-10-21):**
- **Shells** are faces joined through shared edges, closed when every edge balances.
- **Small shells (R8):** less surface than an area bound, or (closed) less volume than a volume
  bound: removed and reported. CHITUBOX deletes small shells silently (CHI-MR-13); here the bound
  is the user's and every removal is located.
- **Inner shells (R8, on request):** a closed outward shell is removed only when no other shell
  crosses it (exact contact pairs) and the other closed shells' winding number at one of its
  vertices is at least one (exact ray parity per shell, weighted by each shell's orientation). A
  vertex on another shell or inside an open one decides nothing, and the shell stays. So a
  cavity, a body floating in a cavity (winding zero around it) and crossing bodies are kept; of
  bodies nested inside one another, all but the outermost go.
- **Visibility (R6):** for open parts only, as Lychee orients them: rays from up to 32 sample
  faces along and against their normals; the part turns when the reverse reaches the outside of
  the model's box unobstructed more often. A tie leaves it. Closed parts keep the exact outward
  rule.
- **Presets:** `Aggressive` gains visibility and small shells under 1 mm² (the diagnosis's small
  component); inner shells only on request. Command line: `visibility`, `small-shells=mm2`,
  `small-volume=mm3`, `inner-shells`; plan markers 0xF8 and 0xF9.
- **Measured:** visibility 0.14 s on the Yorick and 0.70 s on `Base_01.stl` (one open part, not
  turned); shells with the inner test 0.14 s and 0.20 s. Neither changes any corpus model.

**r7, rebuild as a solid (2026-10-21):**
- **Method:** the hollowing core's field in a rebuild mode: the distance to the source signed by
  the exact vertical-ray winding number (positive inside, so the union of the bodies), sampled
  on a grid of the requested cell only in blocks the surface may cross, its zero level extracted
  by the same marching tetrahedra, welded and validated. Closed, edge-manifold, oriented outward
  and free of self-intersections by construction.
- **Rays near the surface only:** a grid sample takes its sign from an already signed neighbour
  when their distances sum to more than their spacing (the distance is 1-Lipschitz, so the
  surface cannot pass between them). Same result (identical triangles and volumes), 21.8 s to
  14.0 s on the Yorick at 0.05 mm.
- **Deviation, both ways:** the rebuilt surface lies within a certified bound of the source
  (exact vertex distances widened per triangle): about two cells at sharp oblique edges (FX-05:
  1.02 mm at 0.5 mm), not the one cell the plan expected. The source's vertices are measured
  against the rebuilt triangles (a cell index searched in rings, exact, on every core): a large
  value, located, means a feature thinner than the cell was lost.
- **Admission:** zero-area faces and cracks left out as hollowing does; the rest must be
  balanced, so holes and fins come first: the `Rebuild` preset is `Aggressive` then R9 at
  0.05 mm (an interface derives the cell from the printer). Command line: `rebuild[=mm]`, preset
  `rebuild`; plan marker 0xFA.
- **Measured** (Linux release, whole stage):

  | Model | Cell | Time | Peak memory | Result |
  | --- | --- | --- | --- | --- |
  | Yorick (706,822 faces) | 0.2 mm | 2.3 s | 0.3 GB | 0.56 M triangles, deviation 0.42 / 0.48 mm |
  | Yorick | 0.1 mm | 3.7 s | 0.4 GB | 2.4 M triangles, 0.22 / 0.37 mm |
  | Yorick | 0.05 mm | about 8.5 s | 1.5 GB | 10.0 M triangles, 0.11 / 0.19 mm; within the 10 s budget |
  | `Base_01.stl` (1,009,544 faces) | 0.2 mm | about 15 s | 1.8 GB | 10.6 M triangles, 0.41 / 1.24 mm |
  | `Base_01.stl` | 0.1 mm | refused | | beyond 20 M triangles |

  `Base_01.stl` rebuilt at 0.2 mm slices with the default rules: its self-intersection slivers,
  which otherwise need `--reversed-tolerance`, are gone with the union. The rebuilt surface is
  dense; a certified simplification of it (as TD-066 simplifies cavities) belongs to r9's
  budgets.

**r8, interface (2026-10-21):**
- **Auto-repair on import:** every opened model gets the chosen preset as its own repair
  (default Exact, the owner's decision); "Diagnose only" leaves it to the plate. The import
  message judges the defects against those repairs and names them. The setting lasts the
  session: the application has no saved settings yet.
- **Repair section** (first in the object panel): the printability state of the object as it
  will be sliced (from the preview's diagnosis of the repaired mesh, or the import diagnosis
  meanwhile), each defect with its effect and "Show", the repairs (plate, preset, Rebuild with
  its cell, custom stages with tolerances), the report, "Try" the next stronger preset when not
  ready, and "Remove repairs". Every change is one plate edit, undone like any other.
- **Preview:** the pipeline reports each object's `RepairOutcome`, its full diagnosis computed
  once per repairs and kept in the cache; a preview no longer stops at the topology check, which
  slicing still applies. The app prepares every object once profiles are chosen (a solid object
  costs a cached import and repair).
- **Markers:** the selected object's defects and repairs, up to 16 per kind, as an octahedron
  uploaded once and placed per marker on the GPU, sized from each marker's distance to the eye
  (so it keeps its size on screen wherever the camera looks) and drawn through the model as a
  ghost, like drain holes. Colours follow the severity (owner's request after the first trial,
  where every defect of `Base_01.stl` showed red although slicing handles them): red where
  slicing fails, orange where only hollowing is blocked, grey where slicing copes, green where a
  repair changed the model; a hole left open stays orange. Positions do not tell balanced
  non-manifold edges from unbalanced ones, so all show red while any is unbalanced. The pointer
  over a marker names it and its effect; the section has a legend and a switch.
- **Failed slices:** a layer that does not close (a fin, or a hole wider than layer closure) now
  carries `Remedy::RepairDefects`; the command line adds "; repair the model's holes and
  non-manifold edges, for example with --repair standard" to that error (a recorded change of
  that message; no comparison case has it). The interface offers "Repair: Standard" first for it
  and for the topology refusals, and "Repair: Rebuild as a solid" last for reversed slivers.
- **Acceptance:** every tracked fixture reaches slicing from the interface, FX-10B after the
  offered "Repair: Standard" (test).
- **Found while testing on Windows (`Base_01.stl`):**
  - The drain-hole probe and drainage admitted the sum of the exterior's and the cavity's index
    sizes, although they are built one after the other; each is now admitted on its own. A
    cavity still too large to index (15.0 M triangles at a 1.3 mm wall with the default cell)
    says how many triangles it has and that a thicker wall or a coarser cell makes fewer. At
    1.5 mm (11.1 M) holes are placed; the preview then peaks near 4 GB, a budget for r9.
  - A preview no longer requires an object to rest on the plate: a lifted object is hollowed
    and drilled where it is while it is moved, and slicing refuses it with its remedy.
- **Not done:** the deviation colour map of a rebuilt surface (the report locates the largest
  deviation), export metadata (the print formats carry no free text; the summary and the
  messages say how the model was prepared), saved settings, and a trial of the next preset on a
  background copy (the next preset is suggested instead).

**r9, validation (2026-10-21):**
- **Corpus:** `repair_corpus` generates every defect class of the plan (26 cases) and repairs
  each with Exact, Standard and Aggressive, checking closure, that every change is recorded and
  located, the winding number, determinism and the weld bound (TEST-STRATEGY). Its table:

  | Case | Exact | Standard | Aggressive |
  | --- | --- | --- | --- |
  | clean box | unchanged | unchanged | unchanged |
  | inside-out box | turned outward | turned outward | turned outward |
  | one face reversed, duplicated face, split vertex, needle | closed | closed | closed |
  | flat cap (0.1 µm) | kept | cap flipped | cap flipped |
  | fold | kept (closed) | kept | kept |
  | T-junction crack | crack closed | crack closed | crack closed |
  | crack 0.5 µm off the line | capped | crack closed | crack closed |
  | crack 10 µm off the line | capped | capped | capped |
  | two halves 0.8 µm apart across a 1 µm cell border | each capped | welded | welded |
  | fin on 3 faces | hole refused, open | fin removed | fin removed |
  | shared edge on 4 faces, pinch vertex | kept (balanced) | kept | kept |
  | non-simple hole, hole of 48 edges | capped | capped | capped |
  | hole of 300 edges | refused, open | refused, open | capped |
  | model and cavity inside out | turned outward with its cavity | same | same |
  | cavity, overlapping shells, nested shells | kept | kept | kept |
  | shells sharing a face | kept (balanced) | kept | kept |
  | small shell (0.54 mm²) | kept | kept | removed |
  | open sheet | refused, open | refused, open | refused, open |
  | open box inside out | turned, capped | turned, capped | turned by visibility, capped |

- **Found by the corpus and the owner's models, and fixed:**
  - *A model turned inside out with its cavity* was turned outward without the cavity, which
    became a doubly covered body (winding 2). A nested shell now turns with the outermost shells
    around it when all of them turn (TD-054's outward repair).
  - *A planar rim at a crease* (a missing box face or cylinder top) was faired into a dome:
    5.3 mm on a 48-sided prism of radius 10 mm, 13 mm on a 300-sided one, as the thin plate
    continues the walls' slope. A planar rim (within 10⁻⁴ of its size) that the surface leaves at
    45° or more on average keeps the flat membrane; the thin plate's cap is kept as the first
    alternative when the flat one meets the mesh (without it, two holes of a build-plate model
    were refused that r8 filled), then the unrefined one. When two caps meet, the one with an
    alternative is replaced first. A planar rim on a gently curved surface (a sphere's polar cap)
    is still faired (test).
  - *Stacked duplicates:* a group of identical faces with a net count of 2 or more (a face two
    overlapping bodies share on the same side) was collapsed to one face, unbalancing its edges:
    the 17-shell 3DBenchy was ready as it was and needed repair after Exact. Such a group now
    collapses to one face only where that leaves balanced every edge that was balanced (a stray
    duplicate), and otherwise keeps its net count; collapses are undone one by one until none
    unbalances a balanced edge. The winding number is unchanged wherever it was defined.
  - *Contact validation* of caps indexed every mesh face (1.04 s of `Base_01.stl`'s 1.25 s exact
    fill); it now indexes the caps and the faces whose box meets a cap's, and is skipped without
    caps: 0.27 s.
- **Performance budgets** (`repair_budgets`, Linux release, each stage alone; budget per million
  triangles 0.5 s exact, 2 s tolerant):

  | Stage | Yorick (707 k) | `Base_01.stl` (1.01 M) | FX-12 (2.68 M) |
  | --- | --- | --- | --- |
  | R1 weld, exact / 1 µm | 0.021 / 0.011 | 0.036 / 0.016 | 0.145 / 0.140 |
  | R2 degenerate, exact / 1 µm | 0.001 / 0.349 | 0.001 / 0.469 | 0.008 / 0.994 |
  | R3+R6 duplicates, orientation | 0.134 | 0.189 | 0.547 |
  | R5 fins | 0.117 | 0.172 | 0.464 |
  | R6 visibility | 0.133 | 0.717 | 0.550 |
  | R4+R7 cracks and holes, exact | 0.171 | 0.328 | 0.556 |
  | R4+R7 1 µm, refined | 0.170 | 0.287 | 0.564 |
  | R8 shells | 0.143 | 0.186 | 0.517 |
  | R6 outward | 0.318 | 0.426 | 1.072 |
  | Preset Exact / Standard / Aggressive | 0.48 / 0.97 / 1.25 | 0.79 / 1.45 / 2.56 | 1.92 / 3.01 / 3.99 |

  Every stage is within its budget (FX-12's are 2.68 times the per-million budget). The edge
  balance of stacked duplicates is computed only when a stacked group exists: computed always,
  it doubled the duplicates stage on FX-12 (1.08 s). Rebuilding the Yorick at 0.05 mm takes
  8.70 s (budget 10 s).
- **Memory** (peak resident size of the budget run, the import included): Yorick 154 MB
  (223 bytes per triangle), `Base_01.stl` 332 MB (336), FX-12 497 MB (190): linear in the mesh.
  Rebuilding the Yorick at 0.05 mm peaks at 1.25 GB, from the field's grid rather than the mesh.
  The hollowing preview's 4 GB peak on `Base_01.stl` with drain holes at a 1.5 mm wall (r8) is
  the cavity's size (11.1 M triangles), outside repair, and stays open.
- **The owner's models:** `Base_01.stl` and the Yorick as above, and a fixed sample of 40 of the
  1,356 STL files of the owner's library (every 34th by path; 4.7 M triangles, at most 749 k),
  diagnosed with every preset's preview (`inspect --diagnosis`; raw results kept outside the
  repository). 25 are ready as they are and stay ready, 11 are ready with notes under every
  preset, the 3DBenchy of 17 shells was ready with notes and needed repair after any preset
  until stacked duplicates were fixed (now ready with notes under every preset), a printer's
  build-plate model with 1,099 self-intersections slices but needs repair for hollowing under
  every preset (16 of 24 holes capped; the rest meet the mesh: Rebuild is the next step), and
  one file is refused at import for a non-finite coordinate. Against r8, only the 3DBenchy's
  results differ.
- **Native Windows** (hash-verified snapshot of the final tree, `validate.ps1 -IncludeApp`):
  fmt, clippy, Debug and Release workspace tests (1,237 passed, 0 failed, 5 ignored each; Linux
  runs 1,239, the difference being the Unix-only path tests), build, rustdoc, and the app's
  three smoke runs (Vulkan on the discrete adapter, the OpenGL fallback, FX-03 in the viewport at
  16.5 ms mean frame time) all passed.
- **Black-box comparison:** one runtime case so far, below (facets without a position).
  `KARASU_CORPUS_EXPORT` writes the
  crafted corpus as STL files for the owner to open in them, with the observations to be recorded
  there.

**r9, facets without a position (2026-10-21, owner's approval):** one file of the owner's sample
has 1,440 facets with only NaN coordinates (9 runs of 160); Karasu refused it at import. The
owner ran it in CHITUBOX and Lychee: both slice it without those facets, CHITUBOX silently, Lychee
with a misleading "1453 holes" warning (see the repair comparison).
- **Decision:** a `non-finite` repair (`RepairSpec::non_finite`), part of `Exact` and the
  stronger presets: the import leaves those facets out (`NonFinitePolicy::Drop`) and records how
  many and in which runs; the summary's import notes and the Repair section name them; the
  diagnosis has a `DroppedFacets` finding, and the state is at best "ready with notes", since
  part of the file is lost and where it was is unknown.
- **Without it** a job is refused as before, with the same message and now a remedy (`--repair
  non-finite`, or `Remedy::DropNonFinite`, offered in the interface as "Repair: Exact"). The
  command line's default does not include it, so its outputs and refusals are unchanged except
  for that added hint; `inspect` still refuses such files.
- **Measured:** `Dragon_6_SUP.stl` slices with `--repair exact` in 5.9 s (122.904 ml, the same as
  the file without the NaN facets, within 0.01 % of CHITUBOX's layers).

## TD-091 — Closing zero-area cracks when filling holes

**Status:** Implemented on 2026-10-21, from the owner's report on `Base_01.stl` (1,009,544
triangles): with every repair on, hollowing was refused because "4 hole(s) could not be filled
... Degenerate".

**Context:** the model has 5 zero-area faces: three distinct points on a line, a few micrometres
long, as exporters leave at T-junctions. Topology leaves them out, so their edges become 13
boundary edges in 4 rims (two of the faces are adjacent). Every cap of such a rim has zero area,
so hole filling (TD-054) refused them, and hollowing, whose admission needs balanced edges and
no degenerate faces, could not run. Slicing was unaffected: layer closure handles the gaps.

**Decision:** hole filling (`preparation::sampled::fill`, part of the `fill-holes` repair) closes
such cracks instead of refusing them.
- **When:** a rim every cap triangle of which has zero computed area, so its vertices lie on one
  line.
- **How:** each face along the rim is split at the rim vertices strictly inside its rim edge,
  in order along the edge. The new faces cover the region of the face they replace; no vertex
  is added or moved. The zero-area faces whose vertices all lie on the closed rim are removed.
- **Folds:** where a split triangle coincides with an existing face of opposite orientation (a
  zero-volume fold, as two of the model's cracks have), both are removed, like cancelling
  duplicate faces.
- **Validation:** around the crack, every edge must then have exactly one face on each side.
  A crack that does not close exactly this way, or touches faces another closed crack changed,
  stays refused as degenerate. A rim with any cap of area is capped as before.
- **Reported:** cracks closed, faces split, zero-area and folded faces removed, in the repair
  report and the summary's "Repairs" line, which is unchanged for models without cracks.

**Result on `Base_01.stl`:** 3 of the 4 cracks close (4 faces split, 4 zero-area and 2 folded
faces removed). The fourth lies in a knot under 5 µm where coplanar faces overlap with opposite
normals around one vertex: closing it would give an edge four faces, so it stays refused, and
hollowing is still refused. Treating zero-area cracks as closed for hollowing, since they do not
change which points are inside, would be a new admission policy of the core; it is left to the
owner's decision. (Amended by TD-092 r2: a crack closes when the edges around it balance, which
closes the fourth crack into a balanced non-manifold edge.)

**Tests:** cracks at a T-junction closed with and without the zero-area face, with the same
vertices and volume; a crack whose middle vertex is off the line capped as a hole instead. The
comparison suite is byte-identical (507 files).

## TD-090 — The object timeline

**Status:** Accepted on 2026-10-20, owner proposal. The data model was built on 2026-10-21 (M6.4c
c3, recorded in TD-089); the timeline panel comes with the interface review (M6.5).

**Context:** with TD-089, an object is its untouched source mesh plus the operations applied to
it. The owner proposed showing them as a timeline, as parametric CAD applications do (the
CAD applications show), so that the object can be "rewound", an earlier operation changed,
and the later ones applied again in sequence.

**Decision:**
- **One timeline per object**, not per plate: the source, then its operations in the order they
  were applied (hollowing, hollowing blockers, drain holes, later manual supports).
- **Each operation has a stable identity and an enabled flag.** A rewind marker suspends the
  operations after it without deleting them. A single operation can be suspended. Editing an
  earlier operation evaluates the later ones again.
- **Placement is not in the timeline.** Moves, rotations, scales and mirroring change
  constantly in a slicer and stay in undo and redo (TD-081). The operations follow the placement
  (TD-089).
- **Operations refer to the source, never to an earlier operation's result.** A drain hole is
  anchored on the source's surface, not on the hollowed shell. Changing or suspending the
  hollowing therefore never breaks a later hole: there is no "topological naming" problem.
  - The only dependencies are findings, such as whether a hole reaches a cavity, and they are
    evaluated again.
- **Replaying is cheap:** the only heavy operation, generating a cavity, is kept with its inputs
  (TD-089). Rewinding and replaying without changing parameters computes nothing heavy.
- **Undo and redo stay:** they step through edits in time, while the timeline shows how the
  object is built. A timeline edit, such as a suspension, a reorder or a change of parameters,
  is itself one undoable plate edit.
- **Order:** operations are evaluated in timeline order. Today's operations commute:
  - the material is the source minus the cavity minus the holes;
  - blockers act on the cavity only.
  The order matters for display and for future operations that do not commute.

**Consequences:**
- M6.4c stores each object's operations as an ordered list with identities and enabled flags,
  and the plate document gains them.
- M6.5 adds the timeline panel: the list, the rewind marker, suspension and selection.
- A single command model behind every action is evaluated with the interface review, because it
  keeps the timeline and undo coherent.

## TD-089 — Hollowing and drain holes as operations on an untouched model

**Status:** Accepted on 2026-10-20, owner decision. Implemented in M6.4c, completed on
2026-10-21; it supersedes parts of TD-083, TD-084 and TD-085. Phases c1 (model-frame hollowing),
c2 (independent drain holes), c3 (operations and cache), c4 (interface), c5 (command line, formats
and records) and c6 (validation) have their records at the end of this decision.

**Context:** the owner found the hollowing and drain-hole workflow unacceptable on real models:
- Every move, rotation, scale and hole edit prepares the object again. On a 700,000-triangle
  miniature that takes 25 to 30 s, and the cavity and holes are hidden meanwhile.
- Slicing hollows the model once more.

An analysis of the code showed why:
- Hollowing runs on the placed model in world coordinates. The placement is one of its inputs, so
  any transform changes its plan.
- Adding the first hole changes the sampling precision of the cavity.
- A refused hole makes the preview hollow a second time without holes.
- Slicing shares nothing with the preview.
- Nothing checks the side of a hole's cylinder. Only its entry, which must be outside, and its
  end, which must be in a cavity, are certified. A hole that runs inside a wall, or crosses the
  cavity and the opposite wall, is accepted, and it is subtracted from every layer it spans. The
  owner saw such a hole cross a whole figure.

The owner proposed treating hollowing and drain holes as reversible Boolean operations on the
mesh:
- subtract the cavity or the hole and keep its "ghost" mesh;
- undo by uniting the ghost back;
- holes independent of hollowing, possibly overlapping, applied with an explicit button.

**Analysis of reversibility by union:** (A − B) ∪ B equals A ∪ B, not A.
- **Holes:** a drain hole starts outside the model, so uniting it back would add its outer stub
  as material.
- **Overlapping holes:** removing one hole would refill the volume it shares with another.
- **Hollowing:** in exact arithmetic the union restores the solid. With floating point, every
  Boolean re-triangulates, so repeated apply, edit and remove cycles degrade the mesh.
- **Robustness and cost:** robust 3D Booleans are a major project. There is no mature native-Rust
  kernel, and C and C++ kernels are excluded (`AGENTS.md`). Miniatures often carry folds and
  self-intersections on which they fail. A 700,000-triangle model with a cavity of millions of
  triangles would take seconds to minutes per operation.
- **Roadmap:** general 3D Booleans are post-MVP (ROADMAP).

**Decision:** the owner's workflow, implemented as operations on an untouched model.
- **The source mesh is never modified.** An object is its source plus an ordered list of
  operations (TD-090), each in the object's model frame. Removing an operation drops it from the
  list, so returning to the original is always exact.
- **Hollowing:**
  - **Model frame:** hollowing is computed on the source in the object's model frame: units
    applied, before placement. The resulting cavity, the "ghost", follows every move, rotation,
    scale and mirror through the object's transform. Moving or turning a hollowed object computes
    nothing.
  - **Explicit apply:** the wall thickness, grid and simplification are edited first, and
    "Apply hollowing" generates the cavity. Editing the parameters of applied hollowing marks it
    as changed until it is applied again, which replaces the cavity. "Remove hollowing" drops the
    operation.
  - **Selected object only:** hollowing applies to the selected object, never to the whole plate.
  - **Scaling scales the cavity, and the wall with it.** At twice the size a 2 mm wall becomes
    4 mm, as in the reference slicers. Unlike them, the application states the resulting wall
    range, from the smallest to the largest scale factor, and offers to apply the hollowing
    again at the set thickness. Mirroring is accepted, which lifts today's refusal.
  - **Kept once computed:** the cavity is kept with the key of its inputs:
    - the source digest, units and repairs;
    - the wall, the grid and the resolved simplification;
    - the core's hollowing version.
    Previews and slicing use the same cavity, so slicing never hollows again. When the project
    file exists (M6.6), it stores the cavity or recomputes it deterministically from the key.
- **Drain holes:**
  - **Independent of hollowing:** a hole can be placed with or without a cavity. It is stored in
    the model frame: the anchor point, the direction (normal and tilt), the diameter and a
    depth set by the user. The default depth is the wall thickness plus a margin when the
    object is hollowed. The interface no longer searches for the cavity.
  - **From outside or from inside (owner, 2026-10-21):** the anchor is on the source's outer
    surface, cutting inward, or on the cavity's surface, cutting outward through the wall, as
    the reference slicers allow once an object is hollowed. Seen from inside, through the
    section, the wall's thickness is easier to judge.
  - **Overlapping holes are allowed:** the material is the source minus the cavity minus the
    union of the holes.
  - **Explicit apply:** holes are placed with today's live preview, which follows the pointer,
    snaps to the surface and turns red on a problem. "Apply holes" commits them; they stay
    editable and removable afterwards.
  - **The user decides the depth (owner, 2026-10-21):** a hole may pass through the whole
    object, for example to drain at the bottom and vent at the top with one hole. Nothing about
    its path is refused.
  - **Checked along their whole length, reported as findings:**
    - **Refused (red, cannot be applied):** only a diameter below the printable minimum
      (TD-066), and parameters that are not finite or not positive.
    - **Shown on the hole, applied anyway:**
      - it passes through the object and opens on both sides;
      - it reaches no cavity;
      - it ends in material, for example inside the opposite wall;
      - it does not open to the outside;
      - a cavity region stays enclosed and traps resin.
    The findings come from the cylinder's whole length against the source and cavity surfaces,
    not only from its ends.
  - **No small hole limit (owner, 2026-10-21):** the 32-hole limit of the drainage core was a
    resource bound, with no other reason recorded (M5.5-G). Its costs were the pairwise
    separation check between holes, which overlapping holes no longer need, and each layer
    subtracting the holes it crosses. With the holes indexed by height, a layer costs only the
    holes that cross it. A large explicit bound stays, as `AGENTS.md` requires bounded work. It
    is set from a measurement of many holes on a large model, at 1024 per object or more, and
    exceeding it is reported, not silent.
- **How the result is made:** layer by layer at slicing, never as a 3D mesh Boolean.
  - The source and its cavity are sliced as today, the shell being the exterior plus the
    reversed cavity.
  - The holes' exact analytic sections are subtracted from each layer, as drain cuts already
    are (TD-066).
  - Overlaps, and later hollowing blockers (zones the cavity must not reach, as one reference
    slicer offers), become unions and differences of plane sections, which are exact and
    robust.
- **Display:** holes are shown as real openings in the model's surface by GPU constructive solid
  geometry: depth and stencil passes subtract the convex cylinders from the model on screen,
  without changing its mesh. This is the technique the section caps use (TD-086). The ghosts stay
  visible in the drain-hole context. While an operation is being computed, the previous cavity
  and holes stay visible, dimmed.
- **3D mesh Booleans** remain a separate, later decision, for example to export a hollowed STL.
  The stored operations would be their input.

**Consequences:**
- **Core:** hollowing moves to the model frame, and holes gain whole-length checks and fixed
  depths. The CLI's outputs for hollowed models change slightly, because the sampling grid moves
  with the model frame. The comparison baseline is regenerated for those cases and recorded, and
  every case without hollowing must stay byte-identical. The existing CLI options are kept: world
  holes in the design placement are converted to the model frame exactly.
- **Project and plate document:** objects gain their operation list (TD-090), and the document's
  version changes.
- **Interface:** apply and remove actions, the pending state, the scale message, the hole apply
  workflow and GPU-subtracted holes.

**c1 record (2026-10-21): model-frame hollowing.**
- **Core:** `SampledHollowedModel::placed` maps a model-frame shell by a placement, and
  `drain_hollowed_sampled` drains an existing shell. The certificate gains `placement_scale`.
- **Pipeline:**
  - The hollow plan is bound to the identity placement and always samples with the hollowing
    query error (1e-6 mm). Holes no longer change the cavity: before, a plan with holes sampled
    with the coarser drainage error.
  - Holes are certified on the placed shell. A preview whose holes are refused no longer hollows
    a second time.
  - Mirrored objects can be hollowed: the refusal in the request and in the plate document is
    lifted.
  - The summary adds "Hollow placement scale" when the placement scales the shell.
- **Tests:**
  - a rigid placement maps every vertex exactly and keeps the faces, distances and plan record;
  - the exterior is bit-identical to hollowing the placed source;
  - a mirror reverses the faces and keeps positive volumes;
  - a scale multiplies the wall bounds and the volume;
  - a placed shell cannot be placed again, nor collapsed by a zero scale;
  - draining an existing hollow gives the same openings, cutters and regions as hollowing and
    draining together;
  - the plate pipeline gives a moved copy the same cavity faces and a mirrored copy reversed ones,
    and the plate slices;
  - the CLI hollows a mirrored slab with the same cavity volume to rounding.
- **CLI comparison suite:**
  - 473 of the 479 files are byte-identical. They include the print files of c02, c06, c13 and
    c20, which are hollowed; only their summaries differ, in the last digits of the wall range
    and the cavity volume.
  - c02, which has drain holes, now reports a tighter certified wall (1.5709 to 2.4291 mm, before
    1.5699 to 2.4301 mm) because its cavity is sampled with the finer error.
  - The plate case c13 now reports exactly the values of the same model sliced alone (c06).
  - Only c10 (hollowed, simplified, with a surface hole) changes its print file: 10 bytes
    shorter, and a sampled volume 0.0037 mm³ larger out of 21,145 mm³. The simplification keeps
    4 triangles fewer (109,658 against 109,662).
  - The error suite stays byte-identical (28 files).
- **Timing** (same machine, release, the suite's cases): c10 13.4 s (14.6 s in the earlier
  baseline record) and c02 3.0 s (2.8 s). The interactive gain comes with phase c3, when previews
  and slicing reuse the cavity.

**c2 record (2026-10-21): independent drain holes.**
- **Core, `preparation::sampled::holes`:**
  - `HoleProbe` indexes a hollowed shell (exterior and cavity) or a solid surface. It resolves a
    hole anchored on the outer surface (cutting inward) or on the cavity's surface (cutting out
    through the wall), with a fixed depth or, for an outer hole of a hollowed model, the
    automatic depth to the cavity (the command line's surface holes).
  - Its findings come from the hole's axis and eight lines on its side: crossings with both
    surfaces located by bisection on the exact segment test, and each stretch classified by
    exact parity. They are advice, not a certificate: a feature thinner than the bisection step
    on all nine lines can be missed.
  - `HoleCuts` builds the cylinders and serves their exact sections per layer, indexed by
    height. `MAX_HOLES` is 1024 per object.
  - Nothing about a path is refused. Only invalid parameters and the hole bound are.
- **Raster:** cut rings are walked counter-clockwise and removed where their winding is not
  zero, so overlapping holes remove their union. For disjoint rings this is the even-odd area
  of before, so every earlier output is unchanged.
- **Pipeline:**
  - Holes are cut into hollowed and solid objects alike. `--drain-hole` no longer needs
    `--hollow`; `--surface-drain-hole`, whose depth reaches the cavity, still does.
  - The only refusal left is a diameter below the printable minimum along the resolved axis.
  - A preview keeps the object without its holes when they cannot be cut, with the reason.
  - The summary states each hole's findings and the cavity regions it opens to the outside.
  - `DrainHole::Surface` gains the surface it starts from.
- **Interface (minimal until c4):** a hole is refused only when it cannot be resolved; its
  findings are listed under it as notes. Overlapping holes are accepted.
- **Tests:**
  - outer holes into the cavity, only into the wall, into the opposite wall and through the
    whole cube, with their findings and drained regions;
  - holes from the cavity, going out and stopping short;
  - holes on a solid, and the automatic depth only with a cavity;
  - cuts indexed by height and bounded in number;
  - overlapping raster rings removing their union, and ring orientation;
  - CLI: holes in a solid block, overlapping holes removing their union, a through hole.
- **CLI comparison suite** (against c1):
  - Every print file of every case is byte-identical, including the drained cases c02, c10
    and c13.
  - Their summaries now state findings instead of clearances.
  - c18, a hole entirely above the model that drainage refused ("the cut ends outside the
    model"), now slices: the summary says it does not open to the outside and reaches no
    cavity, and the cavity stays enclosed.
  - The error suite is byte-identical (28 files).
- **Measurement:** 1024 vertical holes of 1.5 mm through a 100 x 60 x 20 mm solid block take
  0.90 s and 167 MB for the whole slice. The removed volume, 36,136 mm³, matches the 36,191 mm³
  of the cylinders.

**c3 record (2026-10-21): operations and cache.**
- **Where the 25 to 30 s went** (measured on the 706,822-triangle Yorick miniature, Linux,
  release):
  - reading the file 0.14 s, importing it 0.29 s, its digest 0.09 s;
  - hollowing with the 2 mm wall and 0.25 mm grid 0.62 s, placing the shell 8 ms;
  - indexing it for holes 0.45 s, and resolving and checking a hole 1.7 ms;
  - the whole slice 1.78 s.

  The old drain-hole probe ran the embedded-source pair check, which can be quadratic on folded
  meshes, and previews with a refused hole hollowed twice. Both went with c2. On native Windows,
  after c2, a moved hollowed Yorick was prepared again within about a second.
- **Project (TD-090):**
  - `PlateObject::operations` is the object's timeline of `ObjectOperation`s, each with an
    identity, an enabled flag and a kind (`Hollow` or `DrainHole`).
  - Helpers find the enabled hollowing and holes, push an operation with a fresh identity,
    apply or remove the hollowing in place, and remove an operation.
  - A request refuses an object hollowed more than once.
  - `Hollowing` no longer holds the holes.
- **Holes in the model frame:**
  - Project holes are stored in the object's model frame, and requests say which coordinates
    their holes are in (`Coordinates`). The pipeline places model-frame holes before cutting
    them: anchors and entries mapped, explicit directions turned, diameters and depths kept in
    millimetres.
  - The interface edits holes in world coordinates through `Workspace::hollow_edit`, which
    converts them both ways. Holes no longer need to be carried along when an object moves.
  - Known limit: a tilted hole's tilt direction is measured from world tangents, so turning the
    object does not turn the tilt with it. The interface review (c4) handles it.
- **Cache:** `HollowCache` keeps model-frame cavities by the hollow plan's canonical record,
  the model's format and its unit, least recently used first beyond 1 GiB of meshes.
  - `prepare_with`, `slice_with` and the jobs' `start_with` share it. The interface owns one for
    previews and slicing.
  - A cached cavity is the one a new computation gives, so outputs never change. A test slices
    with and without the cache and compares the files.
- **Continuity in the interface:** after a move, the plate is prepared again (import, checks,
  placement, the cavity taken from the cache), about half a second on the Yorick. Its status
  showed "Hollowing: cavity reused", but the earlier cavity and holes were hidden meanwhile, so
  the owner saw them disappear and come back as if hollowed again. Now, while the object's model,
  hollowing, repairs, printer and holes are unchanged, the last cavity and holes are drawn with
  the map from the placement they were prepared at to the current one. They follow the object
  at once, during a drag too, until the new result replaces them. Supports still wait for it.
  On native Windows, twelve captures over the two seconds after a move all show the cavity in
  place.
- **Plate document version 2:** object-level drain holes without hollowing, and a surface hole's
  starting surface and fixed depth ([plate specification](PLATE-SPEC.md)). Version 1 is still
  read.
- **Tests:**
  - operation identities, order, suspension and in-place hollowing;
  - an object hollowed twice refused;
  - model-frame holes following a moved and a mirrored object;
  - previews and slices sharing a cavity with byte-identical outputs;
  - another wall computing another cavity;
  - removing hollowing giving the never-hollowed output byte for byte;
  - version 2 documents and their refusals.
- **CLI comparison suite:** byte-identical to c2 (480 files), and the error suite (28 files).
- **Native Windows:** on the Yorick, a hole placed, the object moved with the hole following
  and prepared again at once, and the plate sliced (375 layers, 2.5 ml).

**c4 record (2026-10-21): interface.**
- **Hollowing panel:**
  - The wall, grid and simplification are a draft. "Apply hollowing" stores them as the
    object's hollowing operation, "Remove hollowing" drops it and keeps the holes. The panel
    says "Applied", "Changed, not applied yet" or "Not hollowed". The draft starts again when
    the object or its applied hollowing changes, by apply, removal or undo.
  - A scaled hollowed object shows the wall it really has, from its smallest to its largest
    scale factor. **Deviation:** the decision's offer to apply the hollowing again at the set
    thickness is not a button. The stored wall is in the model frame, so a button would offer a
    thicker wall again after each press. The user sets the thickness and applies it.
- **Drain holes:**
  - Holes are placed on solid objects too. While holes are being placed on an object, the
    preview prepares a hole probe for it even without operations (`prepare_with`'s `probes`).
  - A click adds a *new* hole: it is drawn green, with what it would do, and kept in the
    object's model frame, so it follows the object. "Apply holes (n)" appends the new holes to
    the timeline as one edit; "Discard" drops them.
  - New holes have a fixed depth: past the wall plus 1 mm on a hollowed object (at its largest
    scale), 3 mm into a solid one, or the depth the user sets.
  - "Start from: Cavity" places holes on the cavity's surface, where the section shows it: only
    cavity faces seen from inside the void, within the section's heights and not behind the
    model, are picked. The outer surface is also picked only where the section shows it.
  - Applied holes are edited and removed in place, keeping their identities.
  - **New holes are shaped before they are applied (owner, 2026-10-21):** a placed hole is
    selected at once and takes the same handles, fields and drags as an applied one, with the
    same live checks, while it stays new; the owner found it counter-intuitive that a hole
    could be shaped only after being applied. Handles also work while placing; a click still
    places.
- **Tilts turn with the object:** a tilt is measured about the surface normal from tangents
  fixed per frame, so it is now carried between frames through its axis. `map_tilt` and
  `axis_tilt` (core) convert it, and `DrainHole::with_world_tilt` (project) places a model-frame
  tilt with the probe's normal. The pipeline uses it for model-frame holes; the command line's
  world holes are unchanged. The interface converts a tilt dragged in world coordinates back to
  the model frame.
- **Holes as openings (GPU):** the applied holes are cylinders in a storage buffer (a data texture
  since c6). Each model
  and cavity draw discards its fragments inside its object's holes, and so do the section caps.
  A hole's bore is the far side of its cylinder, drawn only in material: a stencil bit counts
  the parity of the object's surfaces beyond the bore along the line of sight, as the section
  caps do. A hole that runs on into the cavity or the air shows the cavity or the background
  there. Ghosts are now drawn only where something hides them. New and candidate holes stay
  solids. Where holes overlap on the screen, each pixel shows the bore of the hole its line of
  sight enters first.
- **Out of the build volume (owner, 2026-10-21):** previews prepare an object beside the LCD
  area or above the build height all the same, so its cavity and holes stay visible while it is
  moved or scaled back; slicing still refuses it, naming the problem.
- **Panel:** anchored at the viewport's top right, clear of the section panel. Enclosed cavity
  regions under 1 mm³ are counted in one line instead of one line each.
- **Findings on overlapping bodies:** a hole's stretches are classified by the winding number
  (positive is material), not by parity. Sculpted models often overlap bodies, and parity called
  their overlap empty, which reported a hole ending in an overlap as passing through.
- **Not done here:** the earlier cavity and holes follow a moved object at full colour (c3), not
  dimmed. The re-preparation after a move (about half a second on the Yorick, the cavity taken
  from the cache) remains; caching the imported mesh would remove most of it.
- **Tests:**
  - core: tilts carried between frames and inverted; overlapping bodies counted as material;
  - project: a tilted hole leaning the same way on the turned object; previews probing a solid
    object on request; an object out of the build volume hollowed in the preview and refused by
    slicing;
  - app: new holes waiting to be applied and discarded, shaped and moved while new; applied
    holes dragged back when
    refused; holes from the cavity where the section shows it; holes on a solid object; a
    world tilt kept in the model frame; the hollowing draft; the shaders validated with naga.
- **CLI comparison suite:** byte-identical to c3 (480 files), and the error suite (28 files). No
  case has overlapping bodies or model-frame tilts.
- **Native Windows:** on the cube hollowed at 2 mm, the hollowing applied, a hole placed, applied
  and shown as an opening into the cavity, the section's cap open where the hole crosses it, and
  a hole started from the cavity.

**c5 record (2026-10-21): command line, formats and records.**
- **Command line:** its options are kept. World holes are cut where they are given, so no
  conversion is needed; the interface's model-frame holes are placed by the pipeline (c3, c4).
  Options after a `--surface-drain-hole` now shape it as the interface does:
  - `--hole-depth <mm>`: a fixed depth from the surface; such a hole needs no hollowing;
  - `--hole-from outer|cavity`: a hole from the cavity's surface, out through the wall (needs
    `--hollow`);
  - `--hole-tilt <angle> <direction>`: the axis leans by up to 60 degrees.
  Each applies to the `--surface-drain-hole` just before it, at most once per hole. A surface
  hole without a depth still needs `--hollow`; its message now names `--hole-depth` too.
- **Plate document version 2:** a surface hole takes `"tilt": {"angle_deg", "azimuth_deg"}`,
  measured in world coordinates like the document's holes ([plate specification](PLATE-SPEC.md)).
  A version 1 document with a tilt is refused.
- **Comparison suite in the repository:** the suite used since c1 lived outside the repository
  and was lost with a WSL restart. It was recreated from the session record, with the same cases,
  profiles and inputs, in [`tools/cli-comparison`](../tools/cli-comparison/README.md). Its new
  baseline is the c4 build (`c8399b6`), 507 files. **c5 is byte-identical to it** (507 files):
  the new options change nothing for runs without them.
- **Benchmark before and after M6.4c** (Linux/WSL, AMD Ryzen 7 7700, release, three runs each,
  medians; the 4K test printer at 0.05 mm layers). "Before" is the build of `904bccb`, the plan of
  M6.4c, before c1:

  | Case | Before | After | Peak memory, before / after |
  | --- | ---: | ---: | ---: |
  | Yorick (706,822 triangles), solid | 0.63 s | 0.65 s | 179 / 179 MB |
  | Yorick hollowed at 2 mm | 1.41 s | 1.42 s | 241 / 241 MB |
  | Yorick hollowed, a hole up through the base | refused, 1.49 s | 1.84 s | 257 / 245 MB |
  | FX-11 hollowed, two holes (c02) | 2.86 s | 2.93 s | 530 / 544 MB |
  | FX-11 hollowed, simplified, a surface hole (c10) | 12.51 s | 12.43 s | 704 / 704 MB |
  | FX-11 solid, antialiased (c03) | 0.38 s | 0.38 s | 106 / 107 MB |
  | open model repaired and hollowed (c06) | 0.13 s | 0.14 s | 46 / 47 MB |
  | plate document (c13) | 0.21 s | 0.22 s | 64 / 66 MB |
  | cube hollowed at 3 mm (c20) | 0.07 s | 0.06 s | 21 / 21 MB |

  Slicing from the command line costs the same as before; the M6.4c gains are interactive. The
  hole up through the Yorick's base passes through a thin floor beside the cavity: before, it
  was refused ("the cut ends outside the model"); now it is cut and reported. A surface hole
  from the base's centre was refused before ("ends in or on the far wall") and is now cut,
  reaching the cavity and ending in material, in 1.75 s.
- **Interactive measurement** (the project pipeline as the interface runs it, the Yorick
  hollowed at 2 mm, two runs): a cold preview 1.45 s; after a move and a 45-degree turn, the
  preview takes the cavity from the cache in 0.71 s (25 to 30 s before c1, as measured in c3);
  slicing with the preview's cavity 0.69 s, against 1.29 s without the cache.
- **Tests:** the plate document's tilt and its refusals; the command line's fixed depth, hole
  from the cavity, tilt and every misuse of the new options.

**c6 record (2026-10-21): validation.**
- **Native Windows, first run** (snapshot of `d6e7f8b`, clean): every gate passed (format,
  clippy, Debug and Release tests 1180/0/3, build, rustdoc, application build), except the
  application's smoke test on the OpenGL fallback, which panicked at start: the hole openings
  (c4) read the holes from a storage buffer in fragment shaders, and the downlevel OpenGL device
  allows none. **Fixed:** the holes are now a data texture of `f32` texels read with
  `textureLoad`, which every backend supports, 256 holes to a row, written only when they change.
- **Cross-platform determinism:** the Yorick hollowed at 2 mm, with an axis hole and a tilted
  fixed-depth surface hole, sliced to the same print file on Linux and native Windows
  (SHA-256 `06878f37…`). One summary line differed in the last digit of the tilted hole's axis:
  `tilted_axis` used the platform's `sin_cos`. **Fixed:** it now uses the fixed-operation
  `sin_cos_degrees` of TD-061, and both summaries are identical. The comparison suite is
  unchanged by it (507 files).
- **Native Windows, second run** (snapshot of `d6e7f8b` with the two fixes, recorded as a dirty
  tree): `FAILED_COMMANDS 0` out of 13 commands; tests 1180/0/3 in Debug and Release (1182 on
  Linux, the difference being the two Unix-only tests); the smoke tests on Vulkan, on OpenGL and
  with a model all render. No antivirus or application-control block occurred.
- **The owner's scenarios on the Yorick** (native Windows Release application, Photon Mono X,
  screen captures):
  - opened outside the build volume and hollowed there at once (TD-089 c4: previews prepare out
    of the volume); the certified wall 1.58–2.50 mm and the cavity, 484 mm³, within about
    1.5 s of "Apply hollowing";
  - moved onto the plate: the warning went away, the cavity followed;
  - a hole placed on the outer surface at the default depth reported that it reaches no cavity;
    deepened in the panel to 8 mm before being applied, it reported passing through the model;
    with the automatic depth it was refused in red ("the hole axis does not reach the cavity");
    it was discarded;
  - with the section at 8 mm, a hole started from the cavity's floor drained it; applied, the
    panel reported "Cavity region 1 (484 mm³) drains through hole 1" and one enclosed pocket
    under 1 mm³;
  - moved and turned about Z: the hole, the cavity and the section caps followed within 100 ms;
  - scaled by 1.5 along Z: "Scaled with the object, the wall is 2.00–3.00 mm thick", the certified
    wall 1.58–3.75 mm, the cavity 726 mm³, still drained by the hole;
  - sliced: 564 layers, 1:27:54, 3.8 ml of resin, within 4 s; the preparation panel moved aside
    for the layer panel;
  - on the OpenGL backend, a cube's applied hole showed as an opening into its cavity.
- **Known limits, recorded for later phases:**
  - Model-frame tilts (the interface's holes) are carried to the world through `atan2`, from the
    platform library, so their last bit may differ between platforms; command-line and plate
    document tilts do not go through it.
  - Support documents turn their bases with the platform's `sin_cos` (`turn_deg`); M6.4e, which
    reworks supports, should move it to `sin_cos_degrees`.
  - The interface's re-preparation after a move (0.71 s on the Yorick) still imports the model
    again; caching the imported mesh would remove most of it. Done the same day, below.

**Follow-up (2026-10-21): imported models kept.**
- `HollowCache` becomes `PipelineCache`. Besides cavities, it keeps each model held in memory
  (a project's) as imported, with its defects, its digest once computed and its repairs for
  each repair setting asked for. The key is the identity of the model's bytes, which the entry
  keeps alive, with its file name and unit choices: no hash is computed to find it. Changing the
  repairs repairs the kept import again; nothing else is redone. Models read from files are not
  kept, so the command line is unchanged. The least recently used models are dropped beyond
  1 GiB of their meshes.
- Every step kept is deterministic, so outputs do not change: a test slices with and without the
  cache after a move and after a change of repairs, and compares the files; the comparison suite
  is byte-identical (507 files).
- **Measured** on the Yorick hollowed at 2 mm (project pipeline, Linux, release, two runs):

  | Step | Before | After |
  | --- | ---: | ---: |
  | Preview after a move and a turn | 0.77 s | 0.47 s |
  | Slice after the preview | 0.72 s | 0.40 s |
  | Cold preview | 1.62 s | 1.58 s |

  Of the 0.47 s left, 0.46 s built the hole probe over the placed shell, which a preview made for
  every hollowed object.
- **The hole probe only when needed** (the owner's choice, the same day): a preview builds an
  object's probe only when it has drain holes, which need it, or when the interface asks for it
  (`prepare_with`'s `probes`): while holes are placed on the object, and while it has new holes,
  which are checked again as they change or the object moves. Starting to place holes on an
  object without any costs one preparation with the probe; the hole under the pointer is grey
  until then. Measured on the Yorick:

  | Step | With the model cache | And the probe on demand |
  | --- | ---: | ---: |
  | Preview after a move and a turn | 0.47 s | 0.02 s |
  | Cold preview | 1.58 s | 1.06 s |
  | Preview when holes start being placed | — | 0.45 s |

  On native Windows, the hole under the pointer was green within 1.2 s of turning placing on.
- **Memory:** the interface keeps the model twice, once for display and once in the cache (about
  25 MB for the Yorick, more when repairs change it).

## TD-088 — The plate fades when seen from below

**Status:** Implemented on 2026-10-20 (M6.4b4), from the owner's review of the viewport.

**Context:** seen from below, the plate's surface and grid lie over the models' undersides,
which is where drain holes and, later, supports are placed. The owner compared two reference
behaviours (observed only):
- one slicer hides the surface;
- another fades out both the surface and the grid, and keeps a thick border so the plate's
  limits stay visible.

The owner chose the second.

**Decision:**
- While the camera is below the plate (eye below Z = 0), the plate's surface and grid fade out
  over 0.25 s, and fade back in when it returns above. The fade is drawn by the shaders from
  one opacity in the frame uniforms, so no geometry is rebuilt.
- A **border** around the plate, outside its edge, stays at all times. It is about 1 % of the
  plate's larger side, from 0.8 to 3 mm, in its own configurable colour (`plate_border`).
  - The front tab now starts beyond the border. It stays visible, and from below its word reads
    mirrored, as a mark on the plate's top would.
  - The build volume's edges and the plate's outline also stay.
- The scene's line and surface data are split into what fades (grid, surface) and what stays
  (edges, border, tab).

**Validation:**
- **Application tests:** the border surrounds the plate at the expected width; the tab lies
  beyond it; the grid and the edges are separate.
- **Native Windows** (screen captures):
  - from above, the plate with its grid, border and tab;
  - from below, the border, the tab and the volume's edges around a model whose underside is
    unobstructed.

## TD-087 — The plate's front, and layers seen as the plate

**Status:** Implemented on 2026-10-20 (M6.4b4), from the owner's review of the viewport.

**Context:** the owner found that:
- nothing showed which side of the plate is the front, as every slicer does;
- the layer panel did not match the model's place on the plate.

The layer panel showed each mask as written to the printer's panel: row 0, the plate's front
(lowest world Y), at the top. Each printer's axis swap and mirrors turned it further.

**Decision:**
- **Front tab:** a trapezoid on the plate's plane, outside the front edge (the lowest Y,
  `docs/ARCHITECTURE.md`) and centred on it, carrying the word "Front" (`plate-front`,
  translatable). It is sized to the plate's width: 36 % of it up to 120 mm along the edge.
  Its colours (`front_tab`, `front_label`) join the configurable defaults.
  - The word lies on the plate and is drawn in the scene with depth, so models in front of it
    hide it. Its glyphs come from egui's font atlas: the text is laid out at 64 points and the
    atlas is copied into a viewport texture right away. Later changes to egui's atlas never
    affect the copy, and no font dependency is added.
  - The tab and the word are rebuilt when the printer changes.
- **Layers seen as the plate:** the layer panel undoes the printer's panel orientation (mirrors,
  then the axis swap, the reverse of the raster's order) and shows the plate from above, with
  the front at the bottom and X to the right. The exported file is unchanged; only the preview
  turns.

**Validation:**
- **Application tests:** the tab lies outside the front edge, centred, with the word inside it on
  the plate's plane; a lit panel pixel lands where the plate puts it for each mirror and the
  axis swap.
- **Native Windows** (Yorick 32 mm miniature on a 4K plate, screen captures):
  - the tab and the word on the front edge;
  - layer 150 shows the figure as the top view does: head to the back, legs to the front, sword
    to the left.

## TD-086 — Section panel, capped cuts and hole depth cues

**Status:** Implemented on 2026-10-20 (M6.4b3), after the owner's third review. It replaces
TD-083's section view in the preparation panel and TD-085's holes drawn only over the model.

**Context:** the owner found hollowing and drain holes hard to read:
- a hole's solid, drawn over the model, did not show whether it was in front of or behind it as
  the view turned;
- a hollowed model looked like a thin shell around another solid: the wall's thickness was not
  visible;
- the section view belonged in a fixed, narrow panel at the viewport's right, always available,
  with a vertical slider of two handles to move through the layers from both ends.

**Decision:**
- **Section panel** (`section_ui`): a 78-point column at the viewport's right, always shown.
  - A vertical slider with two handles: the lower one cuts the models below its height, the
    upper one above. Each shows its layer number and height.
  - Heights snap to the plate's layers (the resin calibration's layer height for the printer,
    0.05 mm without one). The slider spans the highest model, not the build volume, so a small
    model uses the whole slider.
  - Handles keep their place on the slider when a model is turned, scaled or moved: each is
    stored as a fraction of the models' height, and the cut follows the model, snapped to a
    layer. Handles are drawn at their fractions, not at the snapped heights, so they hold
    still while the model turns. A handle at its end cuts nothing, whatever the models' height. Keeping an absolute
    height instead moved the handles by themselves when the slider's scale changed, which the
    owner reported as a defect.
  - Double-click shows everything again. It is only a view: the plate does not change, and
    nothing is recorded in undo.
- **Cut solids are capped:** where a plane cuts a closed solid, the cut is filled in a section
  colour (`#C28452`, a configurable default), so a hollowed model shows its wall as a ring
  around the open cavity.
  - The viewport uses a depth-stencil target (`Depth24PlusStencil8`). For each active plane, the
    capped meshes are drawn again with only that plane's clip, inverting one stencil bit per
    fragment behind the plane. Pixels with an odd count are inside a solid, and a quad on the
    plane is drawn there with depth testing.
  - Models and cavities are capped (`Draw::cap`), so the source mesh and the cavity mesh
    together leave the wall's cross-section. Holes and the plate are not.
  - *Supports are capped too* (owner, 2026-10-30), in a colour of their own (`#2E5BD9` by
    default, under Settings › Colours › Supports). A support is a union of overlapping closed
    convex parts, where parity would open the cap at every overlap. Its draws (`Cap::Support`)
    count their winding in the stencil's four low bits instead: front faces add one, back faces
    take one away, and a pixel is in a support where the count is not zero. The two planes
    share the counter: each plane's support cap clears it where it is drawn or hidden. Support
    caps go down before the models' caps, which win where a tip enters the model, and holes
    never open them. Without a section nothing more is drawn; with one, each support draw is
    drawn once more per plane into the stencil only, and nothing is uploaded.
  - The shader clips with two planes (lowest and highest kept height), set per object, so the
    counting passes can clip one plane at a time.
  - The caps' two quads are uploaded once, with each plane's index in place of its height; the
    vertex shader reads the heights from the frame uniforms. They are uploaded again only when
    the cap colour changes. The first version built a new buffer every frame, which the
    performance directive in `AGENTS.md` rules out.
- **Holes have depth cues:** each hole's solid is drawn twice. The solid is depth-tested like a
  model, so it is hidden where the model is in front. A ghost at 30 % of its opacity is drawn
  over everything, so a hole behind the model still shows, faded. The placing candidate is drawn
  the same way. Handles stay on top so they can always be grabbed.

**Validation:**
- **Application:** snapping, layer numbers and which planes cut, in unit tests; the workspace
  gates.
- **Native Windows** (FX-01 cube hollowed at 2 mm, synthesized input, screen captures; Vulkan,
  and the GL backend in the smoke test):
  - the upper handle at layer 207 (10.35 mm) shows the wall's ring around the open cavity;
  - a hole on a side face is solid blue in front of the model and a faded ghost when the view
    turns it behind.
  - A slider drag released over another part of the screen first jumped to the pointer's
    release position. The slider now moves only while the button is held.

## TD-085 — Drain holes as solids with handles

**Status:** Implemented on 2026-10-20 (M6.4b2), after the owner's second review. It replaces
TD-084's ring markers and reasons written next to the pointer. TD-086 draws the solids with depth
and a ghost instead of only over the model.

**Context:** the owner found the reasons beside the pointer confusing and the rings unintuitive.
Users expect to click and drag the hole itself. The owner asked:
- to always show the hole's solid while placing, coloured red when it cannot be made;
- for handles on the hole to widen, lengthen and turn it, with the panel's fields kept;
- for placed holes in a light blue or blue.

**Decision:**
- **Holes are solids**, drawn whole over the model through a new overlay pass of the viewport
  (`Draw::overlay`: no depth test or write, drawn last, 80 % opaque). Depth and tilt are
  visible through the wall.
  - Placed holes are blue (`#3D8BF2`), the selected one light blue (`#9CCBFF`), a refused one
    red. The placing candidate is green or red, and grey until the object has a probe.
  - All colours are configurable defaults.
- **No text over the model:** a refused candidate's reason is shown in the panel only, under the
  placing hint.
- **The core gives a refused hole's shape:** `DrainageProbe::check` returns a `RefusedHole` with
  the error and as much of the hole as was resolved: the cut, its `HoleFrame` (foot and outward
  normal) and its cutter.
  - A surface hole whose automatic depth finds no cavity gets a nominal 5 mm depth for display.
  - A certified hole also gives its cut and frame.
  - Drainage's own results are unchanged: the comparison suite still matches in 479 and 28
    files.
- **The solid is the grip:** a click on a hole's solid selects it, and a drag of the solid slides
  the hole along the surface. A hole released where it is refused returns where it was.
- **Handles of the selected hole:**
  - **Rim (dot):** the diameter, the distance from the pointer's ray to the axis, 0.5 to 20 mm.
  - **End (square):** a fixed depth, the pointer's position along the axis, 0.1 to 100 mm.
  - **Axis (ring), 4 mm outside the surface on a line from the hole:** the tilt. The ring moves
    on the plane at that height, and the axis runs from it through the foot (angle up to 60
    degrees, direction around the normal, as the core's `tilted_axis` defines them).
  - Handle drags are checked like moves, all the way, and released only where certified.
  - The panel's fields edit the same values.
- **Core helpers made public for the handles:** `hole_tangents` and `tilted_axis`, so the
  interface measures tilt exactly as the core applies it.

**Validation:**
- **Core:** a probed hole gives its frame; refused holes give their shape (an edge hole, an
  overlap, a fixed depth in the wall).
- **Application:**
  - refused candidates keep a solid and a reason;
  - a click selects a hole by its solid and not beside it;
  - solid drags return on refusal and move on certification, in one undo step;
  - the end, rim, tilt and body grips give the expected depth, diameter, tilt and surface
    point.
- **Native Windows** (synthesized input, screen captures): a green solid at a face's centre, a
  red solid near an edge with the reason in the panel only, a placed hole selected with its
  three handles.

## TD-084 — Drain holes checked while they are placed

**Status:** Implemented on 2026-10-20 (M6.4b2), after the owner's review of M6.4b. Its rings
and pointer-side reasons were replaced by TD-085.

**Context:** the owner asked for:
- a preview of the hole that follows the pointer and snaps to the surface;
- red where a hole cannot be made, and nothing placed there;
- holes that can be moved, turned and given a depth after they are placed.

Until now a hole was certified only by the background preparation, after it was placed, and one
refused hole failed the whole object's drainage.

**Decision:**
- **Core** (`preparation::sampled::DrainageProbe`):
  - The probe indexes a hollowed model's shell and cavity once, and decides once whether the
    source is embedded.
  - It then certifies one hole at a time exactly as `drain_model_sampled` does: same resolution,
    cutter, certificate and separation from given cutter boxes. A check is a few index queries.
  - Drainage and the probe share the per-hole evaluation (`evaluate_one`).
  - A test checks that a probed hole is the opening drainage certified, cutter included.
- **Surface holes gain options** (`SurfaceHoleSpec`):
  - `tilt`: angle up to 60 degrees from the inward normal, and a direction around it from a
    reference tangent;
  - `depth_mm`: a fixed depth from the surface, which certification still refuses when it does
    not end inside the cavity.
  - The plan records them only when given, under markers no operation tag uses, so existing plan
    records and command-line outputs are unchanged.
- **Faster resolution:** a surface hole's normal is summed over the faces within its radius found
  through the shell's index, in source order, instead of a scan of every face. The sum is
  bit-identical, and the comparison suite confirms it (479 and 28 files unchanged).
- **Previews keep the cavity:** when a hole is refused, preparation for display hollows the object
  without its holes, sampled exactly as drainage samples, and reports the refusal apart
  (`ObjectPreparation::drainage_error`). Every hole then still has a verdict and can be moved.
  Slicing still refuses the object.
- **Application:**
  - With "Add drain holes" on, the candidate hole follows the pointer on the selected object.
    Its certified cutter is drawn in green; a refused one shows a red ring and its reason in the
    interface's words. A click places only a certified hole.
  - A placed hole is selected by its marker or its row, and dragged along the surface, checked
    all the way. It returns where it was when released where it is refused. Delete removes it,
    and Escape ends a drag or placing, then releases the selection.
  - The selected hole's diameter, depth (automatic or fixed) and tilt (angle and direction) are
    edited in the panel. Every hole's verdict colours its marker and row.
  - The probe of the latest preparation is reused while the object's model, placement, hollowing
    settings, repairs and printer are unchanged, so adding or moving holes is checked at once.
- **Approximation, documented:** an object prepared without any hole is hollowed with the
  hollowing-only sampling, so the first hole's check may differ marginally from the preparation
  that follows. The preparation decides, and its verdicts replace the probe's.

**Not yet:** the command line's `--surface-drain-hole` and plate documents keep perpendicular
holes with automatic depth.

**Validation:**
- **Core:** tilted holes follow the stated direction; a fixed depth in the wall is refused and
  one past it drains at that length; a tilt beyond 60 degrees is refused; the record changes
  only by the tilt's bytes; the probe matches drainage and refuses an edge hole and an
  overlapping one.
- **Project:** a refused hole keeps the cavity and the probe, and slicing is still refused.
- **Application, headless:** placing on a certified point, refusing an edge, an immediate check
  after adding, a drag back on refusal, a drag to a certified point in one undo step, Delete and
  Escape.
- **Native Windows** (synthesized input, screen captures): green preview at a face's centre, red
  with its reason near an edge, placing, and dragging a hole to a new certified place.

## TD-083 — Hollowing and drain holes in the interface

**Status:** Implemented on 2026-10-20 (M6.4b). Its hollowing and drain-hole workflow is to be
replaced by TD-089 (M6.4c).

**Decision:**
- **Preparation panel** (`hollowing_ui`): the selected object's panel, at the viewport's top
  right.
  - **Hollowing:** wall thickness (default 2 mm), grid cell (automatic by default) and cavity
    simplification. Each value changed repeatedly is one undo step.
  - **What was certified:** the panel shows what the background preparation (TD-082) certified:
    - the wall range, the cavity and material volumes;
    - every cavity region with the holes that drain it;
    - a region no hole reaches, reported as trapping uncured resin. Nothing is claimed about
      resin flow.
    - It shows the object's refusal with its reason, a solid model as solid, and asks for a
      printer and resin when they are missing.
- **Drain holes:**
  - With "Add drain holes" on, each click on the selected object adds a surface hole at the
    point hit, of the chosen diameter (3 mm by default). Its margins are the command line's
    (`SURFACE_HOLE_MARGIN_MM`). The core orients and certifies it while preparing.
  - Holes are listed with their diameter and a remove button. Escape stops placing.
  - Every surface hole is marked and numbered in the viewport, so a refused hole shows where it
    is. The certified cutting cylinders are drawn in the drain-hole colour.
- **Holes follow their object:** holes are stored in world coordinates at the object's placement,
  the plate document convention. Every interface edit that moves, turns or scales an object maps
  its surface holes from the old placement to the new one, in the same undo step, so a hole
  stays at the same point of the model.
  - An object is followed only when the edit kept its model and its holes, so removing or
    reordering objects never moves another object's holes.
  - Explicit-axis holes, which the interface does not create, are not moved.
- **Section view:** meshes are cut away above a chosen height (in the shader; the plate and lines
  are not cut), so the cavity shows inside the model, in the cavity colour. It ends when the
  panel closes. TD-086 replaced it with the section panel.
- **Colours:** cavity and drain-hole colours join the configurable viewport defaults.
- **Glyphs:** a test now checks that the font draws every symbol written in the interface code,
  not only the catalogs. It found the layer panel's arrows and the applied-fix mark drawn as boxes
  since M6.3; they were replaced.

**Observed on native Windows** (FX-01 cube, synthesized input, screen captures):
- Hollowed at 2 mm with the automatic 0.25 mm grid, the wall certified at 1.81 to 2.22 mm, with a
  cavity of 4095 mm³.
- A hole placed near an edge was refused with the core's reason ("the end cap is not entirely
  inside the cavity"). One at the face's centre drained the cavity.
- The section view showed the cavity. Slicing gave 400 layers and 3.9 ml, matching the certified
  3905 mm³ of material.

**Known limits, for M6.5:**
- The panel floats over the layer panel when both are shown.
- Number fields keep the dot as decimal separator.
- Moving a hollowed object hollows it again after the pause; nothing is cached yet.

## TD-082 — Preparation previews

**Status:** Implemented on 2026-10-20 (M6.4a).

**Context:** M6.4 shows hollowing, drain holes and manual supports while they are edited. Until
now, only slicing ran the preparation stages. It refused supports with an error finding, so a
preview could not show what was wrong.

**Decision:**
- **Headless:** `pipeline::prepare(request, observer, stop)` runs import, repair, placement,
  hollowing, drain holes and supports exactly as `slice` does, without slicing or writing.
  - It returns one result per plate object (`ObjectPreparation`), so one refused object does not
    hide the others.
  - Each object gives its hollowed model (shell, cavity and wall certificate), its drained model
    (openings, cavity regions and cutters) and its built support scene with every finding.
  - Two refusals of slicing become data for display: support findings of error severity, and a
    model above the plate with no support on it. Slicing still refuses both.
- **Background:** `jobs::PreparationJob` runs it on its own thread, with phases, cancellation and
  the plate revision it describes.
- **Application** (`preparation_ui`):
  - A plate with preparation and a printer and resin is prepared 300 ms after its last edit, so
    a drag starts one preparation when it pauses. An edit cancels the preparation in progress.
  - A preparation is drawn only while the plate still has its revision.
  - Supports are drawn per object in the support colour, or in the error colour when a finding
    is an error. Both colours are configurable defaults.
  - The status bar shows the progress, objects that cannot be prepared (the reason on hover) and
    the count of support errors.
- **Picking:** a hit now gives the surface's outward normal in world coordinates, turned back
  outward under a reflecting placement. Drain holes and support tips are placed with it
  (M6.4b; supports in M6.4d, renumbered by TD-089).

**Validation:**
- A three-object plate prepares the supported slab with a support in the air as an error
  finding, and the hollowed and drained block with a certified wall and no enclosed cavity. It
  refuses the lifted block alone, with its remedy; slicing the same plate is refused.
- The background job finishes with its revision and can be cancelled.
- The application prepares a hollowed plate only after the quiet pause, and hides the
  preparation after an edit.
- The world normal is tested, including under mirroring.
- The 20-case CLI suite matches in 479 files, and the refusal cases in 28.

## TD-081 — Undo and redo of plate edits

**Status:** Implemented on 2026-10-20 (M6.4a): the non-destructive edits of M6.4 need them.

**Decision:**
- **Headless history** in `karasu-project`:
  - Each plate keeps the contents before each edit step, up to 200 steps.
  - `undo_plate` and `redo_plate` restore them as **new revisions**: results computed before stay
    stale even when the content matches them again.
  - An edit that changes nothing is a revision but no step. A new edit clears the redo steps.
- **Merged steps:** `edit_plate_merging(key)` merges consecutive edits with the same key into one
  step, as a drag or a dragged value is one change for the user.
  - Any other edit, an undo or a redo ends the step.
  - A merged step that comes back to its starting content is dropped.
- **Application:**
  - Undo and Redo buttons, and Ctrl+Z, Ctrl+Shift+Z and Ctrl+Y (Command on macOS), except while
    a text field has the focus.
  - Each handle drag, including its settling on the plate, is one step.
  - Each Repair menu setting is one step while it is changed repeatedly.
- **Scope:** plate content only. The model library and profiles are not undone. A model removed
  with its object stays in the library, so undoing the removal restores the object.

**Validation:**
- Tests cover undo and redo as new revisions, stale results after an undo, cleared redo steps,
  merged drags, a dropped no-op step, the history limit and an unknown plate.

## TD-080 — Interface localisation with Project Fluent

**Status:** Implemented on 2026-10-20 (M6.3b). The owner chose to prepare localisation before
M6.4, to keep the interface in English for now, and approved Fluent after its dependency review.

**Decision:**
- **Catalogs:** every interface text is a [Fluent](https://www.projectfluent.org) message in
  `assets/locales/<locale>/*.ftl`. Code names messages only by identifier (`tr!("id", name =
  value)`); plurals, word order and grammar belong to each catalog. `en-US` is the reference: it
  holds every message, and a message missing from another language falls back to it.
- **Sources:** bundled catalogs are compiled into the executable, so the interface always works.
  `--locale-dir <folder>` adds `<folder>/<locale>/*.ftl` on top, overriding messages with the same
  identifier, so a translator sees a change without rebuilding. `--locale <tag>` chooses the
  language (default `en-US`); the settings will offer it in M6.5. Packaging (M6.7) may later ship
  catalogs beside the executable.
- **Scope:** the desktop interface only. The command line and the headless crates stay in
  English. Technical details shown inside messages also stay in English for now: the pipeline's
  reason, the slice summary, file errors and the pipeline's progress phases.
- **Arguments:** counts are integers that also select plurals; other values are text already
  formatted. Fluent formats numbers without locale conventions, so decimals go through
  `i18n::decimal`, which uses the catalog's `number-decimal-separator`. Number fields
  (`DragValue`) keep the dot until the settings and layout work of M6.5.
- **Fluent options:** Unicode isolation marks are disabled (egui lays out left-to-right text
  only; right-to-left scripts are not a goal).
- **Pseudo-locale:** `--locale pseudo` shows the reference texts with accented look-alike letters,
  about 35 % longer and bracketed. It reveals texts that bypass the catalogs and layouts that cannot
  take longer translations.
- **Dependencies:** `fluent-bundle` 0.16.0 and `unic-langid` 0.9.6 in `karasu-app` only, with
  `fluent-syntax` 0.12.0 for tests ([review](research/M6-DEPENDENCY-REVIEW.md)). `fluent-pseudo` is
  not used: it would add `regex`.
- **Policy:** `AGENTS.md` now allows the translation catalogs, and only them, to be written in
  their own language.

**Findings from the pseudo-locale on native Windows:**
- Six of the first pseudo letters had no glyph in egui's default fonts and showed as boxes. A test
  now checks that the font draws every character of every catalog and of the pseudo-locale.
- The workflow bar has no room for 35 % longer texts: the move, rotate and scale buttons leave
  the window. The bar is redesigned in M6.5.

**Validation:**
- Every `tr!` identifier exists in the reference catalog with exactly the arguments its message
  uses, and every message is used.
- Every catalog parses, is bundled and uses only reference messages with the reference variables.
- English plurals, fallback to the reference, folder overrides, an added language, the decimal
  separator and the pseudo-locale are tested.
- Interface calls given an English literal fail a test (a heuristic for single-line calls).
- The font draws every catalog and pseudo character.
- On native Windows the application runs with `--locale pseudo`, and a window capture shows
  pseudo texts everywhere except data (profile and format names).

## TD-079 — Tolerated reversed slivers

**Status:** Implemented on 2026-10-20; the owner chose the policy and its defaults.

**Context:**
- The owner's model (`Base_01.stl`, SHA-256 `bc0d0276…572a`, 1,009,544 triangles) failed to
  slice with "negative winding number", even with every repair enabled.
- It is not inside out: its signed volume is positive (about 376 cm³) and every face agrees with
  its stored normal.
- In 2 of its 2097 layers (indices 308 and 2007, at z = 15.425 and 100.375 mm), a
  self-intersecting fold leaves a reversed sliver: winding −1 over 3 samples and over 1 sample
  of one row, about 0.001 mm² in all.
- The positive rule refused any negative sample, so one pixel failed the whole job, and the error
  did not say where.

**Decision:**
- **Core:** `WindingRule::PositiveBounded { max_negative_samples }` fills like the positive rule.
  Up to that many negative samples per layer stay empty; more fail the layer.
  - `WindingRange` counts each layer's negative samples.
  - `RasterError::NegativeWinding` now names the layer and the first pixel.
  - `RasterError::NegativeWindingAboveBound` also gives the count and the bound.
- **Pipeline:** `reversed_tolerance_mm2` (union fill only, finite, positive, at most 100 mm²)
  becomes the bound in whole samples: the area over one sample's area (pixel pitch squared over
  the antialiasing samples), rounded down.
  - When it is set, the summary adds "Reversed slivers left empty": layers, total and largest
    area, its layer and the bound.
  - A negative-winding failure offers `Remedy::TolerateReversed` when no tolerance is set, beside
    `RepairOutward` when bodies are not turned outward.
- **Command line:** `--reversed-tolerance <mm2>`, with no default.
  - Summaries are unchanged unless it is given.
  - The negative-winding error now names the layer, the pixel and both remedies.
- **Application:** new plates tolerate 0.1 mm² per layer
  (`DEFAULT_REVERSED_TOLERANCE_MM2`).
  - The Repair menu shows and edits the tolerance.
  - A failed slice offers "Leave small reversed slivers empty", and a finished slice reports the
    slivers left empty.

**Why bounded:** a body that is inside out as a whole has a negative section of square
millimetres or more, so it still fails and the outward repair still applies. A fold's slivers
are a few pixels. 0.1 mm² is about 380 pixels at 14 × 19 µm, or 40 at 50 µm.

**Validation:**
- Core tests: the bound tolerates exactly its count and fails one below it; an outward source
  gives the positive rule's masks; the error names the layer and first pixel.
- Pipeline test: a 2 × 2 mm inside-out box beside a box is refused without a tolerance with both
  remedies. With a 3.5 mm² tolerance it fails on 4 samples over a bound of 3. With 4 mm² it
  slices to the plain box's volume, and the summary line reports the slivers.
- CLI tests: parsing and bounds.
- The 20-case suite still matches in 479 files. The refusal cases differ only in the inside-out
  error's new location and hint.
- The owner's model slices to 2097 layers with 0.001064 mm² left empty in 2 layers.
- Native Windows validation of TD-077 to TD-079 passed on 2026-10-20
  ([native Windows validation](CLI-ENGINE-NATIVE-WINDOWS-VALIDATION.md)).

## TD-078 — Repairs, diagnosis and messages in the interface

**Status:** Implemented on 2026-10-20 (M6.3 refinement, after the owner's test).

**Context:** the owner sliced an inside-out STL from the interface. The status bar showed the
command line's error, which named `--repair outward`: too long for one line, and an option the
interface did not have.

**Decision:**
- **Structured pipeline errors** (`karasu-project::pipeline`):
  - `PipelineError` keeps the command line's `message`, and adds the `reason` without
    command-line options and the `remedies` (`Remedy`) those options stand for: turn bodies
    outward, make orientation consistent, fill holes, close holes per layer, strict fill, allow
    topology defects, rest on the plate, move onto the plate, give or match units.
  - Each refusal composes its message as reason plus hint, so command-line output is unchanged.
- **Diagnosis at import** (`karasu-project::diagnosis`):
  - `diagnose` reads a model without changing it: the core's defect report, and what duplicate
    collapse, consistent orientation and outward orientation would change.
  - Findings: inside-out bodies, holes, faces pointing the wrong way, duplicate faces, touching
    bodies (non-manifold edges), non-orientable or flat bodies, no usable faces. Nested reversed
    bodies are cavities, not findings.
  - Each finding is judged against the plate's settings: handled, harmless under union filling,
    or unhandled.
  - The application diagnoses every opened model on a background thread after showing it
    (FX-12, 2.7 million triangles: about 1.3 s in a release build).
- **Repair menu** (`karasu-app::repair_ui`): merge duplicate faces, make orientation
  consistent, turn inside-out bodies outward (which needs consistent orientation), fill holes in
  the model, close holes in each layer with its gap, slice despite mesh defects. Each switch is a
  plate setting the command line already has; changing one makes an earlier slice out of date.
- **Messages** (`karasu-app::messages`):
  - The status bar shows the latest message in one line, truncated, and a Messages button that
    counts new messages, coloured by the worst of them.
  - The Messages window lists every message, newest first, with its details (the pipeline's
    reason, the slice summary, the diagnosis) and the fixes it offers.
  - A fix is applied only when clicked. A failed slice's fix also slices again.
  - Status colours outside the brand palette: amber warnings `#F2B33D` and red errors
    `#FF6B6B`, both above 4.5:1 on the panels. They become configurable with the settings
    (M6.5).
- **Repairs summary:** a finished slice adds a message with its summary, and another listing the
  repairs applied when any were.

**Not changed:** nothing is repaired unless the plate's settings ask for it, and the defaults stay
those of the command line (duplicates and orientation, layer closure up to 2 mm).

**Validation:**
- Pipeline tests check the remedies and CLI-free reasons of an inside-out body, a lifted model
  and a model beside the plate, and that the remedy for the inside-out body works.
- Diagnosis tests cover a clean box, an inside-out box, a hole, a flipped face, a duplicate face,
  a cavity and the handling rules.
- App tests cover the failed-slice wording and fixes, the settings each fix changes and the import
  message.
- The 20-case CLI comparison suite matched in all 479 files. Nine more cases matched the CLI built
  before the change in all 28 files: refusals of an inside-out body, a model off the plate, a
  missing unit, hollowing with holes, strict fill of an open model and an orientation conflict,
  two option conflicts, and the outward repair succeeding.

## TD-077 — The core workflow: profiles, slicing, export and layers

**Status:** Implemented on 2026-10-20 (M6.3). The presets, the profile library and the
selection were replaced by TD-106's catalog, library and print settings (2026-10-26).

**Decision:**
- **Profile library** (`karasu-project::library`):
  - Built-in presets are the JSON documents under `assets/presets`, compiled into the
    executable, read-only. A test lists the folder, so a preset cannot be left out, and checks
    that every preset is valid, that identities are unique and that each resin calibrates a
    preset printer.
  - The user's profiles live in the core's `ProfileStore`, in the platform data folder.
  - Saving a preset makes a user copy under a new identity (`<id>-custom`, then `-2`, `-3`…);
    presets are never changed.
  - Saving a user profile writes its next revision, and a stale revision is refused.
- **Presets** (generated by `tools/assets/make_presets.py`; provenance in
  `assets/presets/README.md`):
  - eight printers from public manufacturer specifications: the Anycubic Photon, Photon S,
    Photon Mono X, SQ, 4K, M5 and M5s, and the Elegoo Saturn 4 Ultra 16K;
  - the build X and Y are the panel's area, which fixes the pixel pitch;
  - resin starting points from the manufacturers' tables: Anycubic Standard on six Anycubic
    printers, ELEGOO Standard on the Saturn;
  - karasu-slicer's own choices and the known limits are listed beside the sources;
  - the Prusa SL1S waits until its portrait, mirrored export is verified;
  - a test slices a small box on every preset printer in each of its formats.
- **Selection and editing** (`karasu-app::profiles_ui`):
  - the plate's printer is chosen from the library, and its resin from the resins calibrated
    for that printer;
  - preset resins are labelled "starting point";
  - a basic editor covers the printer's build volume, panel and output flags, and the resin's
    calibration for the plate's printer (layer height, exposures, bottom and transition layers);
  - the core validates every profile before it is stored.
- **Files:** `rfd` 0.17.2 opens models and profiles in the native dialog and saves exports. It
  adds `rfd` and `pollster` 0.4 to the lock file (dependency review).
- **Slicing** (`karasu-app::slicing_ui`):
  - Slice runs the plate's request as a background job into the application's cache folder,
    which is emptied at start.
  - The format is chosen among the printer's supported formats.
  - A progress card with phase, layers and Cancel appears after 150 ms, so quick jobs never
    flash it.
  - The status bar shows layers, estimated time and resin. A result older than the plate's last
    edit is marked out of date and cannot be exported.
- **Export:** the native save dialog chooses the destination. The file, or the reference format's
  folder, is copied to a temporary sibling and renamed into place, so a failure leaves nothing
  partial.
- **Layers:**
  - the pipeline's `Observer` gains `mask`, which receives each exported layer's mask exactly as
    the exporter gets it;
  - jobs started with `keep_masks` return them, as compact row spans;
  - the layer panel shows the chosen layer, reduced to at most 1024 pixels by taking the
    brightest covered pixel, so thin features stay visible;
  - the exported file is never decoded or recomputed for display.

**Validation:**
- Tests cover the presets (valid, unique, complete, each printer slicing in each format) and the
  user library (save, revise, stale refusal, import, free identities, presets only).
- Jobs keep the exported masks in order with the right coverage.
- Also tested: the mask reduction, atomic publishing and the time format.
- CLI outputs are unchanged.

## TD-076 — Colours with alpha

**Status:** Implemented on 2026-10-20 at the owner's request.

**Decision:**
- **Every configurable colour carries alpha:**
  - the palette tokens (`theme::Token`, now `rgba`);
  - the viewport and handle colours (`appearance::Rgba`).

  Values are sRGB red, green and blue with an alpha that is not premultiplied (255 is opaque).
  Every default is opaque except the plate surface, 80 % transparent at the owner's request.
- **Where translucency works now:** the plate surface, grid, outline and volume edges, the
  background and the handles. Models also blend, but in plate order with depth writes, so a
  translucent model can hide one behind it.
- **Where it waits:** correct translucent models need a separate pass, sorted far to near,
  without depth writes. It comes with the hollowing view of M6.4, which needs to see the cavity
  through the walls.
- **Compositing:**
  - The viewport image is premultiplied for egui: the background is cleared premultiplied, and
    alpha blending over it keeps every pixel premultiplied.
  - Blending happens on sRGB-encoded values, which is exact for opaque colours and close enough
    for light overlays.
- **Contrast checks:** they composite a translucent foreground over its background. The tests
  require the approved palette to stay opaque.

**Validation:** tests cover the linear conversion with alpha, the encoded channels, egui's
unpremultiplied input and the palette's opacity. On native Windows the default colours read
exactly as before: model `#fcbf87`, background `#0b0b12`, plate `#1c1c22`, panels `#1a1c24`.

## TD-075 — Window placement and start mode

**Status:** Implemented on 2026-10-20 at the owner's request.

**Decision:**
- **Centring:** the splash and the main window always appear centred on the monitor they open
  on, whatever position the windowing system first chooses.
  - The splash is placed on its first frame, from the monitor size.
  - The main window is placed when it replaces the splash.
  - A windowed main window is centred again two frames later by its actual outer size (title
    bar and borders included).
- **Start mode:** the main window opens windowed, maximized, or fullscreen (no title bar,
  covering the monitor). The default is **maximized**. A maximized window is first centred, so
  restoring it gives a centred window.
- **Configuration:** the mode is a setting: the window reopens as it closed, and as a window at
  its kept size and place (TD-097). `--window windowed|maximized|fullscreen` overrides it.
- **Limits:** positions use the size of the monitor the window opens on, as the windowing system
  reports it. A secondary monitor whose origin is not the desktop's origin is not yet
  accounted for (egui reports the monitor's size, not its position).

**Validation:** native Windows, 1920 × 1080, positions read with `GetWindowRect`:
- the splash is 720 × 420 at (600, 330), centred exactly, in every mode;
- windowed, the main window's centre is (960, 540.5) with its decorations;
- maximized, `IsZoomed` is true;
- fullscreen, the window covers 1920 × 1080 from (0, 0).

Headless tests cover the option parsing and the centring arithmetic.

## TD-074 — Interface colours and complete object transforms

**Status:** Implemented on 2026-10-20 after the owner's review of M6.2. It supersedes TD-073 on
colours, moving and scaling.

**Owner decisions:**
- **The palette is the brand identity.** The interface also uses other colours. Every interface
  colour must be configurable in the settings.
- **The gizmo uses the conventional red, green and blue axes**, configurable later.
- **Models default to `#fcbf87`**, also configurable.
- **Objects move freely, and along X, Y and Z, without supports.** The workflow is: place the
  object (move, rotate, scale), then hollow and drill drain holes, then add supports.
- **Scaling works per axis as well as uniformly.**
- **The graphics as a whole will be revisited later.**

**Decision:**
- **Colours:** `karasu-app::appearance::Appearance` holds every viewport colour:
  - models, selection, outside the volume;
  - background, plate surface, plate grid (every 10 and every 50 mm), plate outline and build
    volume edges;
  - the three axes, uniform handles, the hovered handle.

  The defaults apply until the settings exist; the brand palette stays the interface chrome.
  Defaults: models `#fcbf87`, selection `#ffe3c7`, outside `#6e6a7a`; greys for the plate
  (owner request): surface `#1c1c22` at 80 % transparency (alpha `0x33`), grid `#4a465c`, grid every 50 mm `#6b6876`, outline and
  volume edges `#3a3a42`;
  axes X `#e5484d`,
  Y `#46c25a`, Z `#3e7bfa`; free-move square `#f2b800` (`#ffe066` under the pointer); uniform
  scale handle `#f5f4f8`; other handles under the pointer `#ffd14a`. Labels X, Y and Z
  remain, so the axes do not rely on colour alone.
- **Moving:**
  - The handles follow the object while it is dragged.
  - Arrows move along world X, Y and Z and end in arrowheads pointing along their axis.
  - A filled yellow square at the gizmo's origin moves freely on the plate's plane, and turns a
    lighter yellow under the pointer.
  - Only handles move objects (owner decision): clicking an object selects it, and dragging
    across it orbits the view, so it is never moved by accident.
- **Height:**
  - Moving along Z sets an explicit height and ends resting on the plate.
  - An object never goes below the plate: after every edit, a lifted object whose lowest point
    is below Z = 0 is raised onto the plate.
  - "Drop to plate" rests it again.
  - The status bar names objects above the plate, because slicing still refuses a model that
    floats without supports. That check stays where it is: a floating model would print
    detached from the plate.
- **Scaling:** handles along the object's own X, Y and Z axes (they turn with it) scale one
  axis, and a uniform handle scales all three. Scale applies in the object's frame before its
  rotation, as placements define it.

- **Exact colours on screen:** egui shows native textures as sRGB-encoded values, so the
  viewport's target stores encoded bytes. The shaders light in linear values and encode their
  output, and the light follows the view. A face turned towards the viewer therefore shows
  exactly the configured colour. Native Windows pixels read `#fcbf87` on the model,
  `#0b0b12` for the background and `#1c1c22` for the plate. An earlier sRGB target had shown the
  linear values, so every viewport colour looked darker.

**Validation:** headless tests cover the colour conversion and the default model colour; the
plate's surface, grid and edges; the
move handles (X, Y, Z, plane); free, axis and Z moves; turns and snapping; per-axis and uniform
scaling; the free-move square hit anywhere inside; settling onto the plate, floating detection,
dropping, and the point a ray hits.

## TD-073 — The 3D viewport

**Status:** Implemented on 2026-10-20 (M6.2).

**Decision:**
- **Rendering:** the scene renders on eframe's own wgpu device into an offscreen sRGB colour
  target with a 32-bit depth buffer, with 4x multisampling where the adapter supports it, and
  egui shows the resolved texture. The 3D pass stays independent of egui's render pass, and the
  same code runs on Vulkan, DX12, Metal and the OpenGL fallback. No dependency was added.
- **Large meshes:** each library model is uploaded once, as indexed `f32` positions relative to
  its source bounding-box centre (sub-micrometre precision at printer scale).
  - A draw places it with a model matrix, so moving, turning or scaling never uploads vertices
    again.
  - Flat shading comes from screen-space derivatives, so no per-face normals or duplicated
    vertices are stored. Lighting is two-sided, so mirrored placements shade correctly.
- **Placement:** objects are drawn exactly where they will print. The pipeline's placement moved
  into a public `karasu-project::placement` module that both use. Placements are recomputed only
  when the plate's revision changes.
- **Camera:** an orbit camera in the core's world frame (mm, Z up) with depth 0 to 1:
  - primary drag orbits, secondary or middle drag pans, scroll zooms;
  - "Fit view" frames the build volume and the objects;
  - *the target follows what is seen* (owner report, 2026-10-30: after a while orbiting and
    panning grew too fast until Home). Orbit and pan work at the target's depth, which panning
    and turning had left far from the surface in view. A drag now first brings the target along
    the line of sight, the view unchanged, to the depth of the surface seen: in the middle of
    the view (else under the pointer) for an orbit, under the pointer for a pan, so the point
    grabbed follows it. Over no surface the target stays. The surface is found by the same ray
    test as the wheel's zoom, once per drag: 8.8 ms on a 500,510-triangle model (release, WSL).
- **Opening files:** dropping files on the window, or naming them on the command line:
  - profiles (JSON) become the plate's printer or resin;
  - models import on a background thread and are embedded in the project;
  - a format that declares no unit (STL) is read in millimetres, and a notice says so;
  - new objects are centred (with a printer) and dropped onto the plate.

  Native file dialogs follow with import and export (M6.3).
- **Selection and gizmos:** clicking picks the nearest object by a ray test in each mesh's
  source frame, so the vertices are not transformed. The selected object shows handles at its
  anchor (its source bounding-box centre, placed), a fixed size on screen:
  - **move** along X and Y: objects rest on the plate, and lifting needs supports;
  - **rotate** about X, Y and Z: a Z turn adds to the Z angle exactly; X and Y turns compose with
    the rotation and decompose to the placement's fixed-axis angles;
  - **scale** uniformly;
  - each drag edits the object from its values when the drag began, so the result depends only
    on the pointer; Shift snaps turns to 15 degrees.
- **Colours and cues:** the palette only. Objects are Soft Lilac, the selection Electric Violet,
  and objects outside the build volume Beak Slate. Axes are labelled X, Y and Z, and the status
  bar names objects outside the volume, so nothing relies on colour alone.
- **Plates:** a plate may exist before its printer and resin are chosen; slicing refuses it
  until both are set.

**Validation:**
- Headless tests cover the camera (projection, rays, controls, fitting), picking and the drag
  geometry, the Euler composition, the gizmo drags, and opening, placing, picking and drawing in
  the workspace.
- Native Windows (AMD Radeon RX 9070 XT), FX-12 with 2,675,038 triangles, 90-frame measured
  orbit:
  - Vulkan: mean frame interval 16.3 ms, at most 21.4 ms (the 60 Hz refresh);
  - OpenGL: 16.4 ms mean, at most 18.5 ms;
  - opening, import and splash took about 5 s.
- Selection, the gizmo handles and a move drag were exercised on native Windows with synthesized
  input.

## TD-072 — Supports that follow their object on the plate

**Status:** Implemented on 2026-10-20 as the last step of M6.1 (M6.1c).

**Context:**
- The M6 plan asks for supports that follow their object through moves, rotations and copies to
  other plates.
- Until now a support document was bound to one world placement, and any other placement was
  refused.
- A document mixes two kinds of geometry:
  - **contact points** (tips, model anchors, axes), which belong to the object;
  - **routing** (bends, plate bases, junction and brace heights, the raft), which belongs to the
    plate, because shafts are vertical and stand on it.

**Decision:**
- **Recorded pose.** A binding may record the pose the supports were placed for: `rotate_deg`,
  `scale` and `mirror`, together with `translation_mm`.
- **Which changes the supports follow.** A placement that differs from the recorded one only by a
  move on the plate and a turn about the vertical axis keeps every contact on the same surface
  point and every shaft vertical. The whole document then moves rigidly with the object:
  - `PlanarMotion` is a turn about a vertical axis plus a move;
  - `SupportDocument::moved` applies it to tips, bends, bases and axes;
  - junction and brace heights, the raft and the parameters are unchanged.

  This covers moving an object, turning it on the plate, arranging it, and copying it to other
  plates.
- **Which changes are refused.** A different rotation about X or Y, scale or mirroring would need
  new routing, so it is refused with both poses named; the user re-places the supports
  (interactive editing in M6.4). So is a height change beyond the document's own
  `anchor_tolerance_mm`. The tolerance only absorbs the rounding of composed rotations; within
  it, every anchor is still checked on the actual surface.
- **How the motion is computed.** The model's source bounding-box centre is placed under the
  recorded and the current placement, which gives the move. The turn is the difference of the
  Z rotations, because the placement applies the Z rotation last. Whole quarter turns use exact
  sines and cosines.
- **Unchanged behaviour.** Documents without a recorded pose behave exactly as before: an exact
  translation match, or no binding at all. The command line's outputs are unchanged.
- **Reporting.** The summary reports a motion it applied (`Supports follow the model: …`).

**Validation:**
- Core tests: binding parsing and its refusals, and an exact quarter-turn motion of every
  coordinate kind. The moved document builds on the moved model with the same parts, braces and
  mesh size.
- A project plate moved and turned a quarter keeps its supports, layers and volume (within one
  percent of pixel sampling). Tilt, scale and height changes are refused.
- A CLI document with a pose follows `--translate`, while a translation-only binding is still
  refused.

## TD-071 — Project model and background jobs

**Status:** Implemented on 2026-10-20 as the second step of M6.1 (M6.1b). Persistence follows in
M6.6.

**Decision:**
- **Inputs from memory or files.** The pipeline request names its inputs through sources:
  - `Input<T>` for profiles, support documents and layer plans: a file read when the job runs,
    or a value already loaded, with a display name;
  - `ModelSource` for models: a file, or bytes embedded in a project (shared, never copied);
  - `Subject` for what is printed: one model, a plate document file, or a plate held in memory
    (`PlateSpec`, a list of `ObjectSpec`).

  The command line uses files exactly as before. Summaries name an embedded model with
  `Input name`/`Model name` and an in-memory plate with `Plate`, instead of the file `… path` lines.
- **Project model** (`project`, in memory):
  - a library of model files, each stored once and identified by the SHA-256 of its bytes;
    adding the same bytes again returns the existing identity, and a model still placed on a
    plate cannot be removed;
  - plates, each with its own printer, resin, job-wide settings (format, masks, fill, repair,
    layer plan, spacing) and objects (a library model, its placement, hollowing and drain holes,
    supports, copies and arrangement);
  - plate identities are never reused.
- **Revisions:**
  - every plate edit goes through `Project::edit_plate`, which increments the plate's revision,
    even when the content ends up equal: results are recomputed rather than trusted;
  - `Project::freshness(plate, revision)` says whether a result is `Current`, `Stale` (the plate
    was edited) or `Removed`;
  - an older result never replaces a newer edit (TD-041).
- **Plate requests:** `Project::request` turns a plate into a pipeline request over embedded data,
  with the revision it describes. It checks the settings and every object as the pipeline will,
  so the interface can refuse an invalid plate before starting a job.
- **Background jobs** (`jobs`):
  - `Job::start` runs a request on its own thread; the job owns its request, so the project can
    be edited meanwhile;
  - `poll` returns progress events (phases, layers) and the final result without blocking, and
    `wait` blocks;
  - `cancel` (and dropping the job) stops it at the next checkpoint. A failure after a
    cancellation request is reported as `Cancelled`, and nothing is published.

**Validation:**
- The comparison suite stays byte-identical (479 of 479 files) after the input change.
- New tests cover:
  - library identity and models in use;
  - revisions and freshness, including a plate edited while its job runs;
  - plate requests and their refusals (empty plate, missing model, invalid settings or object);
  - an embedded plate whose output file is byte-identical to the same plate given as files,
    with summaries that differ only in the input and output names;
  - background progress, and a cancelled background job that publishes nothing.

**Next (M6.1c):** supports in the object frame.

## TD-070 — Shared slicing pipeline in `karasu-project`

**Status:** Implemented on 2026-10-20 as the first step of M6.1 (M6.1a).

**Context:** the M6 plan requires the command line and the desktop application to run the same
orchestration, with CLI outputs unchanged. Until now the orchestration lived inside `karasu-cli`
(about 2,500 lines of `slice`), behind command-line types, a process-wide Ctrl-C flag and stderr
progress.

**Decision:**
- **New crate `karasu-project`** (headless, depends only on `karasu-core`):
  - `request`: the validated job description, `SliceRequest`, with its drain-hole,
    simplification, fill and pose types, the format registry and the option checks;
  - `pipeline`: `slice(request, observer, stop)` runs the whole job: import, repair, placement,
    hollowing, drain holes, supports, plate arrangement, layer planning, rasterization, export.
    It returns the published output's summary text, export summary and resin usage.
- **Progress** goes to an `Observer` trait: named phases and completed layers, purely advisory.
- **Cancellation** is the caller's predicate (`Fn(Checkpoint) -> bool + Sync`), passed to every
  core checkpoint and worker; nothing in the pipeline is process-wide. A failed job has published
  nothing, because publishing is its last fallible step.
- **The CLI keeps** option parsing, terminal progress (`Progress` implements `Observer`), the
  Ctrl-C handler and exit statuses, and prints the summary the pipeline returns.
- **Errors stay text messages** with the CLI's option names, which are today the only wording
  users see. The desktop application maps them to its controls when it builds requests (M6.3).
  Structured error kinds are added where the application needs to react to them, not
  speculatively.

**Validation:**
- A comparison suite was run with the CLI built before and after the move: 20 cases covering
  every pipeline path, including two expected failures:
  - formats: reference, GOO, CTB v4 and v5, PP1 on a 13312 × 5120 panel, Photon, PWMA, PM5S;
  - geometry: FX-11, FX-12, open, reversed and cavity models, rotation, scale and mirroring;
  - hollowing with axis and surface drain holes, simplification and an explicit cell;
  - antialiasing, minimum gray, XY and bottom compensation, strict fill;
  - copies, a plate document, an adaptive layer plan, supports.
- All 479 files were byte-identical: outputs, stdout summaries, stderr and exit codes.
- The workspace tests are unchanged, and new library tests cover the outcome, progress
  reporting, cancellation without publication, and request validation.

**Next (M6.1b, M6.1c):** the project model (project → plates → instances, embedded models) builds
requests from in-memory data; background jobs with revisions; supports in the object frame.

## TD-069 — Startup splash screen

**Status:** Implemented on 2026-10-20 at the owner's request (M6.0). Native Windows validation is in
the [validation record](CLI-ENGINE-NATIVE-WINDOWS-VALIDATION.md). The artwork, its size and the
name were changed by TD-141 (2026-11-09): the owner's artwork, which carries the name, at 720 × 405,
square and opaque in an opaque window.

**Decision:**
- **Window:** the desktop window starts as a borderless, transparent, centred, fixed-size splash
  (720 × 420 points). When startup is done it becomes the decorated, resizable main window: the
  same window, reconfigured through viewport commands.
- **Artwork:** `assets/ui/images/splash.png` (in `assets/ui/` until M6.5), an original image generated deterministically by
  `tools/assets/make_splash.py` from the palette:
  - a dark gradient, a purple glow and a sphere drawn as stacked layer slices;
  - rounded corners whose outside is fully transparent;
  - made at twice the splash size for high-DPI displays.
  It is the first content of the planned `assets/` folder (M6 plan). The name, the version and
  the progress are drawn by the application, so the image never goes stale.
- **Progress:** the bar is the fraction of real startup steps completed (artwork loaded, graphics
  device presented a frame, workspace ready). It never animates on time alone; later phases add
  real steps (presets, recent projects).
  - The splash stays at least 1.5 s so it does not flash.
- **Transparency:**
  - The rounded corners are transparent where the chosen surface composes alpha (observed on
    Linux with Vulkan).
  - Otherwise the window clears with the background colour, so the corners are a dark square
    edge that blends with the artwork. This is the case on native Windows with Vulkan and
    OpenGL, and the smoke test reports `transparent`.
  - **Windows, observed 2026-10-20:**
    - DX12 presenting through Direct Composition does compose alpha, but the transparent corners
      showed white. The probable cause (a hypothesis, not verified) is the frame Windows draws
      for the undecorated-window drop shadow, which eframe 0.36 always requests and does not let
      the application disable.
    - Rounded corners through the window manager would need an option eframe does not expose,
      or `unsafe` platform calls the workspace forbids.
    - So the backend choice is not changed for the splash, and Windows uses the opaque fallback.
    - Revisit when eframe exposes the shadow, redirection-bitmap or corner options.
- **Decoding:** `png` 0.18.1, reviewed in the
  [dependency review](research/M6-DEPENDENCY-REVIEW.md).

## TD-068 — Desktop dependencies adopted for M6.0

**Status:** Accepted 2026-10-19 (M6.0). Native Windows validation is recorded in the
[validation record](CLI-ENGINE-NATIVE-WINDOWS-VALIDATION.md).

**Review:** [M6 dependency review](research/M6-DEPENDENCY-REVIEW.md).

**Decision:**
- **Adopted:** `eframe` 0.36.2 (with egui, egui-wgpu, egui-winit 0.36.2, wgpu 30 and winit 0.30)
  in the new `karasu-app` crate, with only the `accesskit`, `default_fonts`, `x11`, `wayland`
  and `wgpu` features.
  - **Rendering:** wgpu with its default Vulkan, DX12, Metal and GL backends. GL is the OpenGL
    fallback of TD-067.
  - **Why not 0.37:** egui 0.37 is four days old and not yet supported by `egui_dock`.
- **Reviewed, not yet added:** `egui_dock` 0.21.1 and `rfd` 0.17.2 pass the review and are added
  with their first use (M6.5 and M6.3).
- **Native code:** no C or C++ code is compiled; FFI is limited to window-system, GPU-driver and
  accessibility interfaces.
- **Security:** `cargo audit` reports no vulnerability. The `paste` (via `egui_dock`) and
  `zbus`/`zvariant` (Linux accessibility) crates are flagged as unmaintained, which is accepted
  and recorded.
- **Window:** `karasu-app` opens the window themed with the palette tokens, chooses the adapter
  deterministically (modern backends first, OpenGL as the fallback) and has a `--smoke-test`
  mode.
- **Windows validation:** `tools/windows-native/validate.ps1 -IncludeApp` builds the application
  and runs the smoke test with the automatic choice and with OpenGL.

## TD-067 — M6 plan and initial desktop decisions

**Status:** Accepted by the owner on 2026-10-19 as the M6 plan; implementation pending.

**Plan:** [M6-PLAN](M6-PLAN.md).

**Decision:**
- **Crates:** two new crates.
  - `karasu-project` is headless: the project model, the shared per-plate pipeline (the CLI moves
    to it with byte-identical outputs), background jobs with revisions, and persistence.
  - `karasu-app` is the eframe/egui application with the custom wgpu viewport (TD-042).
- **Data model:** "project → plates, each with its own printer and resin" from the first phase;
  models in a project library, preparation per plate. Supports move to the object's frame.
- **Docking:** `egui_dock`, subject to the M6.0 dependency review.
- **File dialogs:** native, for now (final choice open).
- **Project file:** models are embedded. The format is open; a 3MF-conformant package with
  karasu JSON parts is recommended.
- **GPU baseline:** modern backends plus the OpenGL fallback; features the fallback cannot
  provide degrade to the opaque style.
- **Code signing:** desirable; feasibility and costs are studied in M6.7.

**Phases:**

| Phase | Content |
| --- | --- |
| M6.0 | Foundations |
| M6.1 | Project and pipeline |
| M6.2 | Viewport |
| M6.3 | Core workflow |
| M6.4 | Preparation |
| M6.5 | Layout and style |
| M6.6 | Plates and project file |
| M6.7 | Hardening and packaging |

## TD-066 — Certified cavity simplification and oriented drain holes

**Status:** Implemented on 2026-10-19 at the owner's request, to finish hollowing before M6.
Native Windows validation passed the same day: every output identical to Linux
([record](CLI-ENGINE-NATIVE-WINDOWS-VALIDATION.md)).

**Research:** clean room, from the behaviour of other resin slicers. No reference bounds its cavity
simplification in millimetres or checks the wall afterwards. Every reference takes a drain
hole's axis from one picked triangle and does not check that the hole reaches the cavity.

**Specification:** [HOLLOW-SIMPLIFY-DRAIN-SPEC](HOLLOW-SIMPLIFY-DRAIN-SPEC.md).

**Owner decisions:**
- the simplification tolerance is in millimetres, with an `auto` default from the printer;
- the minimum wall must stay valid: no point closer to the outside than the thinnest original
  wall; thicker walls may lose at most the tolerance. This was accepted after the FX-12
  comparison and is to be revisited with the M6 user interface;
- holes may be inclined or perpendicular to the surface.

**Decision:**
- **Simplification:** a `Simplify` plan operation after the hollow (`--hollow-simplify <mm|auto>`,
  `simplify_mm` in plates), implemented as deterministic, parallel half-edge collapses.
  - **Topology:** the link condition preserves it.
  - **Deviation:** a two-sided bound from the projection argument, accumulated per face, at most
    the tolerance.
  - **Wall floor:** an exact clearance proof that every new face stays farther than the thinnest
    original wall.
  - **Embedding:** exact contact checks.
  - **Certificate and summary:** they report the tolerance, the certified deviation, the triangle
    counts, the volume before and the wall floor.
- **Inclined holes:** any cut direction (`dx,dy,dz` on the CLI, `direction` in plates).
  - Cardinal axes keep the exact construction.
  - Others use a certified oblique polygonal cylinder whose per-layer cut is the convex section
    of the represented prism.
- **Surface holes:** a `SurfaceHole` plan operation (`--surface-drain-hole`, `surface_mm` in
  plates), resolved during drainage:
  - axis: the area-weighted normal over the hole's radius;
  - entry: 1 mm outside;
  - end: 1 mm past the first cavity surface;
  - then certified like any cut.
  The built axis of every hole is reported.
- **Print resolution:** generalized to the tilt: twice the pixel pitch, and twice the layer
  height times the sine of the tilt.
- **Faster clearance:** `Bvh::clearance` rejects nearby triangles first with a rigorous squared
  projection gap; simplification also indexes source planes.

**Validation:**
- **Core tests:**
  - cube cavity: flat walls to a fraction of their triangles at about 10⁻¹² mm, the wall floor
    certified;
  - curved cavity: a tighter tolerance keeps more triangles; identical for 1 and 3 threads;
  - plan order and tolerance refusals;
  - oblique cylinders: closed, outward, within the error budget and deterministic for four
    directions;
  - oblique prism sections;
  - a 45° hole through a cube wall removing the analytic plug volume;
  - surface holes on a cube face and a torus, and the refusal of a cavity too thin for the end
    cap.
- **CLI test:**
  - simplification by value and `auto` (tolerance, triangle counts, volume);
  - the built axis of a surface hole and of a 45° hole;
  - option refusals.
- **FX-11, 2 mm wall, 16K printer:**
  - 1,415,016 cavity triangles become 232,532 (`auto`, 0.007 mm, 11.6 s in all instead of
    2.3 s) or 95,000 (0.02 mm, 14.1 s), with 0.004 mL and 0.013 mL more resin;
  - a surface hole on the lower flank is resolved at 45° and certified.
- **Regression:** the 16 cross-platform cases and the 11 TD-064/TD-065 cases are byte-identical to
  the previous build.

**Open:**
- hole plugs, tapers and shapes;
- automatic hole placement;
- infill and the outer and bottom-through modes;
- faster simplification of very large cavities;
- a pointwise "only thicker" simplification constraint, to be decided with the M6 user
  interface.

**FX-12 comparison** (137 supports, two surface holes, 2 mm wall, 16K printer, 1090 layers):

| | Total time | Cavity triangles | Cavity volume |
| --- | --- | --- | --- |
| No simplification | 9.3 s | 475,880 | 6559.68 mm³ |
| `auto` | 13.2 s (simplification 4.1 s) | 250,416 | 6559.82 mm³ |
| 0.02 mm | 15.3 s (simplification 6.4 s) | 175,472 | 6560.23 mm³ |

- **Masks:** they differ only at cavity edges, and resin changes by less than 0.001 mL.
- **Unchanged phases:** drainage (2.1 s), supports and slicing (1.15 s) do not change: they depend
  on the 2.6-million-triangle outer surface.

## TD-065 — Multiple models per plate and arranged copies

**Status:** Implemented on 2026-10-19 at the owner's request (priority 5 of the
engine assessment, second part).
Native Windows validation passed the same day: every output identical to Linux
([record](CLI-ENGINE-NATIVE-WINDOWS-VALIDATION.md)).

**Research:** clean room, from the behaviour of other resin slicers.

**Specification:** [PLATE-SPEC](PLATE-SPEC.md).

**Owner decisions:**
1. **Plate document:** a JSON plate document (`--plate`). `--copies` and `--arrange` remain for a
   single model.
2. **Spacing:** the default spacing is 2 mm. Supports and raft are always part of the footprint.

**Decision:**
- **Core (`karasu_core::plate`):**
  - the versioned `karasu-plate` document;
  - `arrange`: deterministic packing of axis-aligned footprints around fixed obstacles,
    reporting what does not fit. Next-fit shelves, centred without obstacles, until
    2026-11-10; since then (owner) each footprint goes nearest the plate's middle among the
    candidates beside those already placed, the group growing outwards in a grid, with the
    shelves as a fallback when they fit more (see [PLATE-SPEC](PLATE-SPEC.md), "Packer"). A
    shelf the obstacles leave empty now starts again above them;
  - `overlapping_pairs`.
- **CLI, preparation:**
  - every object is prepared once at its design placement by the single-model pipeline;
  - instances are rigid XY moves of that geometry, concatenated into one world mesh and united
    by the positive winding rule;
  - drain cutters move with their instance, and footprints with drain holes may not overlap;
  - arranged footprints keep one pixel pitch from the plate edges.
- **CLI, single model:** without plate options it keeps its exact placement transform, so
  existing outputs do not change.
- **Global settings:** per-object exposure and every other plate-global setting stay global
  (MM-R2).

**Validation:**
- **Core unit tests:**
  - document defaults, every refusal and the instance limit;
  - shelf positions and centring, spacing between all footprints, bitwise repetition;
  - obstacles and overflow reports; overlap pairs.
- **CLI tests:**
  - `--copies 3`: exact translations and volume;
  - twenty copies fit and the twenty-first is refused;
  - option conflicts;
  - a plate of fixed, arranged and overlapping objects: translations, union volume, the overlap
    line; refusals of model options with `--plate`, invalid objects and missing models.
- **Manual runs on the 16K printer:**
  - three FX-11 copies;
  - a supported FX-11 fixed in the middle with six rotated cubes and two drained spheres
    arranged around it;
  - drain holes present in every copy;
  - an adaptive layer plan on the plate, decoded by the GOO validator;
  - the overlapping-drain refusal.
- **Regression:** the 16 cross-platform cases are byte-identical to the previous build.

**Open:**
- per-object resin volume in the summary (MM-R8; plate totals come from the raster);
- rotation in packing and convex-hull footprints;
- whole-pixel offsets, so that copies sample alike;
- per-object exposure;
- several plates per job.

## TD-064 — Variable and adaptive layer heights

**Status:** Implemented on 2026-10-19 at the owner's request (priority 5 of the
engine assessment, first part).
Native Windows validation passed the same day: every output identical to Linux
([record](CLI-ENGINE-NATIVE-WINDOWS-VALIDATION.md)).

**Research:** clean room, from the behaviour of other resin slicers. No reference implements
adaptive resin layers or an exposure-vs-thickness rule.

**Specification:** [LAYER-PLAN-SPEC](LAYER-PLAN-SPEC.md).

**Owner decision:** manual Z ranges with explicit exposure, plus an independently designed adaptive
mode whose exposure comes from a user height → exposure table, interpolated only between its
points.

**Decision:**
- **Document:** `karasu_core::layer_plan` reads a versioned `karasu-layers` document:
  - a Z step;
  - ordered ranges, fixed (with optional exposure) or adaptive;
  - an exposure table.
- **Planner:** `plan_layers` stacks heights in integer nanometres:
  - **carry boundaries:** a layer takes the range of its bottom;
  - **termination:** centres at or below the top;
  - **calibrated bottom and transition layers;**
  - complete `LayerOverride`s for every layer whose thickness or exposure differs;
  - refusal of a missing exposure and of a reached range that would be skipped.
- **Adaptive heights:** `c / max |n_z|` over the faces near the layer, with a look-ahead of one
  maximum layer, from a Z-bucket index of the placed model's sloped faces. Growth is limited by a
  ratio, decreases are immediate, and heights are floored to the step and clamped.
- **Schedule:** `build_slab_schedule` publishes stacked slabs sampled at their centres, with each
  position one correctly rounded division of an integer. `LayerSchedule::spec()` becomes
  `Option`.
- **Print job:**
  - `PrintJob` takes positions and thicknesses from the slabs.
  - Uncalibrated thicknesses need an override, must stay inside the printer's layer height
    limits, and are refused on bottom and transition layers.
- **Writers:** GOO, CTB, Photon, PP1, Anycubic and the reference format store per-layer positions
  or thicknesses. SL1/SL1S refuse non-uniform jobs: the CLI before rasterizing, and the encoder per
  layer.
- **CLI:** `--layer-plan <file.json>` and a `Layer heights (mm)` summary line.

**Validation:**
- **Core unit and integration tests:**
  - **Fixed ranges:** carry, exposures and table interpolation.
  - **Adaptive slopes:**
    - a 45° cone gives 0.028 mm at `c` = 0.02 mm;
    - a vertical column grows by 1.5× to the maximum;
    - horizontal faces impose nothing.
  - **Document errors.**
  - **Slab schedules:** 30 000 alternating layers end exactly at 600 mm.
  - **Print-job rules.**
  - **SL1 refusal.**
- **CLI test:** heights, positions and exposures in the reference records; GOO output; the SL1,
  missing-exposure and malformed-document refusals.
- **FX-11, fixed and adaptive plans:**
  - GOO, CTB v5, Anycubic `.pm5s` and `.pws`, PP1, Photon and the reference format, each decoded
    by its validator, with per-layer thicknesses as planned and no gaps;
  - an adaptive plan of 1572 layers between 0.02 and 0.1 mm.
- **Regression:** the 16 cross-platform cases are byte-identical to the previous build.

**Open:**
- printer acceptance of variable layers (physical campaign);
- per-range motion, waits and PWM;
- boundary snapping;
- an opt-in exposure model from resin constants;
- per-object layer plans.

## TD-063 — Output formats: plain CTB v3 to v5, Anycubic Photon Workshop, Prusa SL1

**Status:** Implemented on 2026-10-19 at the owner's request (priority 4 of the
engine assessment).
Native Windows validation passed the same day: every output identical to Linux. The example
programs ran once the owner paused a heuristic antivirus false positive
([record](CLI-ENGINE-NATIVE-WINDOWS-VALIDATION.md)).

**Research:** clean room, from the behaviour of other resin slicers.

**Specification:** [OUTPUT-FORMATS-SPEC](OUTPUT-FORMATS-SPEC.md).

**Owner decisions:**
1. **Plain CTB only.** Encrypted CTB is a vendor-keyed, licensed access control, and extracted keys
   are forbidden. GOO stays the preferred route for Elegoo printers that accept it.
2. **CTB v4/v5 notice section:** it carries karasu's own neutral text, not the vendor's copyright
   notice.
3. **Scope:** NanoDLP/Athena and the ZIP + G-code dialects are deferred.

**Decision:**
- **CTB v3, v4 and v5 (`print_job::ctb`):**
  - **What they share with v2:** encoding, RLE15 previews and XOR seed 0.
  - **Per layer:** each layer gains its 84-byte `LayerDefEx`, which is authoritative.
  - **v4 and v5:**
    - add two-stage motion and three rest times, per layer;
    - add a notice and a v4 block, and v5 adds resin parameters (name and density);
    - always write previews, black when none were rendered;
    - require a machine name.
  - **v3:** keeps one motion stage and the light-off delay.
- **Anycubic Photon Workshop (`print_job::photon_workshop`, `AnycubicWorkshop`):**
  - 21 extensions at revisions 1, 515, 516, 517 and 518, with one exporter identifier each,
    `anycubic-<extension>-v<revision>`.
  - Sections are written up front. LAYERDEF and SUBIMGS records are patched as layers stream, and
    volume, weight and model bounds at the end.
  - Images are PW0 (shared with PP1). `.pws` gets a new bit-plane encoder, with consecutive
    same-state runs merged.
  - Two-stage motion requires mirrored retract stages. Rest times and PWM are refused.
- **Prusa SL1 and SL1S (`print_job::sl1`):**
  - A ZIP of compressed grayscale PNG layers and RGBA thumbnails, with `config.ini` written last.
  - Layer PNGs use zlib level 1 through the already reviewed `miniz_oxide` (TD-057); the existing
    stored encoders keep their bytes.
  - The firmware manages motion, so the identifiers say so (`prusa-sl1-firmware-motion`,
    `prusa-sl1s-firmware-motion`).
  - The fade must be exactly one bottom layer and 3–20 transition layers.
- **No platform dependence:** the writers record no clock time, host OS or random value.
- **CLI:**
  - Every identifier is selectable.
  - PW0 targets rasterize at 16 levels.
  - `.pws` and CTB round grays to the format; the others are exact.

**Validation:**
- **Codec unit tests:**
  - CTB v3–v5: magic and version, plain seed, table rows and `LayerDefEx`, two-stage values and
    rest times, notice and v4 block, stationary time, v5 resin strings, the patched volume, and
    images identical to v2. Refusal of two stages on v3 and of unnamed v4 files.
  - Anycubic: six revisions with every section, contiguous images, records, header values, model
    bounds and bit-plane bytes; the capability refusals; bit-plane rounding and exactness.
  - SL1: members, PNG content, sorted `config.ini` and fade validation.
- **Independent validators,** each with hand-built fixtures and corruption tests:
  - `validate_machine.py`: CTB v3–v5;
  - `validate_workshop.py`: Anycubic;
  - `validate_sl1.py`: SL1/SL1S.
- **CLI tests:**
  - every new identifier publishes a complete file with shaded previews;
  - SL1 archives, and the refusal of an unsupported fade.
- **FX-11 at 100 × 60 pixels, binary and 4× antialiasing:**
  - every CTB version decodes to the v2 masks and volume;
  - every Anycubic revision validates, and its binary masks equal CTB's.
- **FX-11 as SL1S at 1620 × 2560:** validated, with 36 MB for 1200 layers in 0.66 s. On a cube,
  every SL1 layer equals the reference-format mask.
- **Regression:** the 16 cross-platform cases are byte-identical to the previous build.

**Open:**
- printer acceptance of every new format (physical campaign);
- per-printer revision choices where the references disagree;
- the meaning of the CTB v4-block constants and of the Anycubic MACHINE words;
- SL1 firmware checks;
- the Anycubic revision-517 raster orientation;
- NanoDLP/Athena and ZIP + G-code (deferred).

## TD-062 — Manual supports on any model

**Status:** Implemented on 2026-10-19 at the owner's request, as the last part of priority 3 of the
engine assessment.
Native Windows validation passed the same day: every support output identical to Linux
([record](CLI-ENGINE-NATIVE-WINDOWS-VALIDATION.md)).

**Research:** clean room, from the behaviour of other resin slicers.

**Specification:** [SUPPORTS-SPEC](SUPPORTS-SPEC.md).

**Context:**
- The KSUP1 supports of M5.5-D certify contacts against a convex model and compose exactly
  (quadratic cost, 4096 edges per layer), so they cannot serve real models.
- The owner asked for supports that:
  - compete with commercial slicers;
  - can be edited one by one (sizes, inclination);
  - can stand on the plate, on the model or on another support;
  - can stand inside hollowed models.

**Decision:**
- **Document:** a versioned `karasu-supports` JSON document (`supports::SupportDocument`).
  - Parameters: each support owns a complete parameter set: built-in light, medium or heavy
    preset, then document defaults, then its own overrides.
  - Validation:
    - hard ranges are errors and soft ranges are warnings;
    - coupled values are validated;
    - IDs, references and cycles are checked;
    - nothing is clamped or quantised.
  - Bases: plate (given XY or vertical), model surface, or another support's shaft at a height.
  - Also bends (knees), braces, a raft and an optional binding to the model hash and translation.
- **Geometry (`supports::build_supports`):**
  - Every part is a closed, outward, convex mesh: frustums and latitude–longitude spheres.
  - Parts are the contact (cone or sphere, including the penetration), joints, shaft segments
    tapering by arc length, pad and transition cone, model foot, junction on a parent, braces,
    and the raft. The raft is the convex hull of the pads plus margin, widened downwards by its
    slope.
  - Tessellation depends only on each support's `sides`, and the degree trigonometry of TD-061
    makes it bit-identical everywhere.
  - Preview, slicing and export use the same geometry.
- **Validation against the actual sliced surface** (the placed model, or the hollowed shell):
  - **Anchors:** must be on the surface within a tolerance. Interior and exterior come from
    whether the anchor triangle belongs to the cavity.
  - **Axis:** the deviation from the normal is limited.
  - **Piercing:** penetration must not reach another surface, checked by exact orientation tests.
  - **Collision:** clearance is proven conservatively:
    - a uniform grid finds the candidate triangles;
    - then an axis-capsule distance test;
    - then separating axes (part face normals, triangle normal, edge cross products).
    A failed proof is a collision. Collisions are errors unless the document or caller accepts
    them, and accepted ones stay reported.
  - **Drain holes:** a support crossing a drain hole would be cut, so it is an error.
  - **Build volume:** checked part by part.
- **Union:**
  - Supports are concatenated with the surface and sliced under the positive winding rule.
  - Penetrating tips, overlapping supports and the model therefore merge.
  - Supports inside a cavity (winding zero) become material there.
  - Antialiasing, minimum gray, XY compensation and drain cuts apply to the whole layer.
- **CLI:**
  - New options: `--supports <file.json>`, `--accept-support-collisions`, and `--lift <mm>`
    (with `--drop`).
  - A supported model may stand above the plate, but some support or the raft must reach it.
  - Previews include the supports.
  - The summary lists supports by base, interior contacts, braces, raft, parts and findings.
  - The document is read first, and the model is hashed only when a binding asks for it.
- **KSUP1 (M5.5-D)** is unchanged for preparation sessions.

**Validation:**
- **Unit tests:**
  - closed, outward primitives whose volumes converge to the analytic ones;
  - right-handed deterministic frames;
  - nearest triangle, exact crossings, and sound but tight clearance proofs.
- **Integration tests (`tests/manual_supports.rs`):**
  - plate, vertical, sphere and bent supports under a floating slab;
  - tessellation independent of other supports;
  - off-surface anchors, axis deviation and thin-wall piercing;
  - collisions proven, refused and accepted;
  - a foot on a lower step;
  - a branch on a support, and a brace;
  - an interior pillar from cavity ceiling to floor;
  - the raft footprint;
  - drain holes and the build volume;
  - document resolution and every schema error.
- **CLI tests:**
  - a lifted slab, with `--lift` equal to the explicit placement;
  - collisions refused and accepted, globally or per support;
  - off-surface contacts;
  - strict fill and acceptance without a document refused;
  - a pillar inside a hollowed cube.
- **The 16 cross-platform CLI cases** without supports are byte-identical to the previous build.
- **Real models at 16K** (supports placed under downward-facing surfaces on a 3 mm grid):
  - FX-11 with 240 supports: 0.65 s (0.31 s unsupported); with a raft, 0.64 s;
  - FX-12 with 137 supports: about 3.0 s (2.9 s unsupported);
  - support build and validation on FX-12: 0.18 s;
  - no findings, and previews show the supports.

**Open:**
- automatic placement (later work);
- tree and branch helpers, other raft types (grid, wall), zero-elevation pads and supports
  exported as separate meshes;
- support transfer under model transforms. Documents are written for one placement, and the
  surface checks reject stale ones.

## TD-061 — Placement, XY compensation and resin usage in the CLI

**Status:** Implemented on 2026-10-19 at the owner's request (priority 3 of the
engine assessment, all parts except manual supports, which
need a core extension, TD-062). Outputs of earlier command lines are unchanged; summaries gain
lines.
Native Windows validation passed the same day: every output identical to Linux, including rotated,
mirrored, hollowed and compensated cases ([record](CLI-ENGINE-NATIVE-WINDOWS-VALIDATION.md)).

**Context:** the core already had object transforms, pixel compensation and resin material
properties, but the CLI offered only an explicit translation, binary or antialiased masks without
compensation, and a mask volume.

**Decision:**
- **Rotation in degrees (`Rotation3::from_euler_xyz_degrees`):**
  - Fixed X, then Y, then Z axes, as the radian constructor.
  - The radian constructors use the platform's `sin_cos`, whose last bit can differ between
    platforms. This one evaluates the half-angle sine and cosine with fixed IEEE operations only:
    an exact reduction to [0°, 45°] by the remainder and Sterbenz-exact subtractions, then Taylor
    polynomials. Rotations are therefore bit-identical on every platform.
  - Multiples of 180° are exact; elsewhere it is within about one ulp of the platform functions.
- **Placement (`--rotate`, `--scale`, `--mirror`):**
  - Rotation, positive per-axis scale and mirroring (negative scale) act about the source
    bounding-box centre, so they do not move the model away.
  - Without them the transform stays the exact translation-only evaluation, so earlier outputs
    are unchanged.
  - Mirroring reverses orientation. The winding layers already reverse section directions for a
    negative determinant, so a mirrored model prints the right way out.
  - Hollowing plans accept positive scales only, so `--mirror` with `--hollow` is a usage error.
- **Computed translation (`--center`, `--drop`):**
  - Both start from the bounds placed without translation.
  - `--center` centres the XY extent on the plate.
  - `--drop` translates by the negated lowest Z. Adding a value's negation gives exactly zero,
    and rounding is monotone, so the model rests exactly on Z = 0 with no vertex below it, after
    any rotation.
  - `--translate` is explicit and cannot be combined with them.
  - The plate-contact error now suggests `--drop`.
  - The summary prints the applied `Translation (mm)` and, when given, the `Transform`.
- **Bottom raster configuration (`ParallelRaster::bottom`, `BottomRaster`):** the first N layers
  can use their own `RasterConfig`. Both are validated and preflighted before any layer.
- **XY compensation (`--xy-compensation <px>`, `--bottom-xy-compensation <px>`):**
  - Signed whole pixels, from −64 to 64, map to the core's gray dilation (positive) or erosion
    (negative), in X and Y.
  - The bottom value replaces the general one on the resin calibration's bottom layers. That is
    the elephant-foot correction. Asking for it with no bottom layers is an error.
  - The summary prints both, with their millimetre equivalents per axis.
  - Pixels rather than millimetres, because the morphology works in whole pixels; converting
    millimetres would round silently.
  - Holes always change the opposite way to outlines: pixel morphology cannot tell them apart.
- **Resin usage (`ResinProfile::usage`):**
  - Resin volume in millilitres from the sampled mask volume (gray pixels by intensity).
  - Mass when the profile has a density, and cost in its currency when it has one.
  - Waste and residue are not included.

**Validation:**
- **Degree trigonometry:** exact at multiples of 90°, within 10⁻¹⁵ of the platform functions
  from −720° to 720°; the degree Euler rotation matches the radian convention.
- **Placement CLI tests:**
  - `--center --drop` equals the explicit translation byte for byte.
  - A box mirrored in X and Z, or turned 180°, equals the plain box apart from previews.
  - A quarter turn and a scale keep the expected volumes.
  - A tilt is refused with the `--drop` hint and accepted with `--drop`.
  - Invalid values and `--mirror` with `--hollow` are usage errors.
- **Compensation:**
  - Parallel stream tests: the bottom layers equal a sequential stream with the bottom
    configuration, the others the plain one, for several thread counts; an invalid bottom
    configuration fails before any layer.
  - CLI tests give the exact volumes for growing, shrinking, the bottom-only correction and both
    together, and the per-layer compensation recorded in reference records.
- **Resin usage:** unit and CLI tests for volume, mass and cost.
- **FX-12 at 16K:** rotated 45° and centred, a compound rotation scaled 0.8 (volume × 0.512),
  mirrored in Y, and rotated and hollowed all slice in 2.8 s (6.4 s hollowed).

**Open:** compensation that tells holes from outlines needs contour offsetting rather than pixel
morphology.

## TD-060 — Minimum gray for antialiased edges

**Status:** Implemented on 2026-10-19 at the owner's request (the remaining half of priority 2 in
the engine assessment). It is opt-in, and every output without
it is unchanged.
Native Windows validation passed the same day: outputs identical to Linux, with and without the
option ([record](CLI-ENGINE-NATIVE-WINDOWS-VALIDATION.md)).

**Context:**
- Antialiased masks store coverage linearly (TD-052).
- Resin does not cure below a threshold dose, so the faintest edge grays have no effect and
  coverage differences among them are lost.
- Other slicers therefore offer a minimum gray (a grey offset or minimum alpha setting); CHITUBOX
  files carry a minimum-gray header key whose semantics were not resolved.
- AA-001 requires any such floor to be an explicit, recorded option.

**Decision:**
- **Core:** `RasterConfig::minimum_gray`, a gray value from 0 to 255 (0 is the previous
  behaviour).
  - A pixel with coverage `c` strictly between 0 and 1 gets the intensity
    `minimum_gray + (255 − minimum_gray)·c`, before rounding to `grayscale_levels`. Rounding is
    to the nearest level, with ties up.
  - Empty pixels stay 0 and full pixels 255.
  - The arithmetic is exact rational integer arithmetic, so the result does not depend on the
    platform. With 0 it reproduces the previous quantizer bit for bit.
- **Why a linear remap without a cut-off:**
  - Every covered sample still adds material, so edge ordering and monotonicity are kept.
  - A cut-off would silently discard partially covered material.
- **Rounding before format depth:** the remap happens before rounding to levels, so PP1's 16
  levels and GOO's 256 stay exact. CTB keeps its explicit nearest 7-bit policy.
- **CLI:** `--min-gray <0-255>`.
  - It requires `--antialias` above 1; with binary masks it is a usage error rather than a
    silent no-op.
  - The summary prints `Minimum edge gray (0-255)`, and the sampled volume includes the raised
    grays.
- **Recording:**
  - Reference records carry `minimum_gray` when nonzero; the key is omitted at 0, so earlier
    records are unchanged.
  - Machine headers keep their gray-level fields at 0, because their meaning to firmware is not
    established.

**Validation:**
- **Quantizer:** compared with an independent exhaustive nearest-level oracle over every sample
  count, for 1–16 samples per axis, 2–256 levels and minimum grays 0–255.
- **Quantizer properties:** monotone and bounded, with no nonzero value more than half a level
  below the minimum.
- **Randomized raster tests:** the span rasterizer still equals the dense reference, now with
  random minimum grays.
- **Exact value:** a half-covered pixel at minimum 100 gives 178.
- **CLI tests:**
  - Option parsing and its dependency on `--antialias`.
  - The same pixels are lit, the partial ones brighter, and the volume larger.
  - At 255, every lit pixel is full.
  - The reference record and the summary line are present.
- **Validator:** the reference validator checks the recorded value and that no nonzero pixel lies
  below it.
- **Regression:** the 16 cross-platform CLI cases without the option are byte-identical to the
  previous build.
- **FX-11 at 4 × 4, minimum 77 (about 30 %):**
  - GOO at 16K, CTB, PP1 and the reference format all pass their independent validators.
  - Edge grays start at 88: one sample of 16 gives 77 + 178/16.
  - There are 17 distinct values per layer.
  - The sampled volume grows by 0.02 % at 16K and 0.11 % on the coarser panel.

**Open:** the right value per resin and exposure is a calibration, so the option has no default.
Physical validation belongs to the print campaign.

## TD-059 — Rendered print-file previews

**Status:** Implemented on 2026-10-19 at the owner's request (priority 2 of the
engine assessment). Layer data is unchanged.
Native Windows validation passed the same day: outputs identical to Linux, including every preview,
and FX-12 sliced at 16K in about 2.0 s ([record](CLI-ENGINE-NATIVE-WINDOWS-VALIDATION.md)).

**Context:** printers and their file browsers show the thumbnails stored in the print file.
Until now:
- GOO thumbnails were black;
- CTB and Photon previews were absent (zero descriptor offsets);
- PP1 previews showed a gray maximum projection of the masks.

**Decision:**
- **Renderer (`karasu_core::preview`):**
  - A headless CPU renderer, with no new dependency.
  - Orthographic camera from the front right and above (direction (1, −1.3, 0.9)).
  - The model's projected outline is fitted with an 8% margin, preserving the aspect ratio.
  - 3 × 3 supersampling with an `f64` depth buffer.
  - Two-sided Lambert shading under one fixed light, on a fixed palette.
  - Determinism: only IEEE arithmetic and square roots, in input order, so the same mesh gives the
    same pixels on every platform and thread count. Ties in depth keep the earlier triangle.
  - The image depends only on the shape, not on its position on the plate.
- **What is rendered:** the surface that is sliced, so either the repaired model or the hollowed
  shell. A model whose open sections are closed only per layer (`--close-holes`) shows its open
  source surface, which tells the user what the file was made from.
- **Encodings:**

  | Format | Small | Large | Encoding |
  | --- | --- | --- | --- |
  | GOO v3 | 116 × 116 | 290 × 290 | RGB565 big-endian in the fixed header |
  | CTB v2, Photon v1 | 200 × 125 | 400 × 300 | RLE15 after a 32-byte descriptor, between the header and the layer table |
  | PP1 | 224 × 168 | 336 × 252 | RGB PNG members |

  - RLE15 words hold red in bits 15–11, green in 10–6 and blue in 4–0. Bit 5 marks a run whose
    length follows as `0x3000 | (count − 1)`, for runs of 2 to 4096 pixels.
- **API:**
  - `BuiltinExporter::preview_sizes` gives the `[small, large]` sizes, and `with_previews` stores
    `Previews`.
  - Encoders check the sizes before writing anything.
  - Without previews every format writes exactly what it wrote before.
- **CLI:**
  - Machine formats render both sizes concurrently, in a `Rendering previews` phase before
    planning the layers.
  - The reference format stores no previews.

**Alternatives rejected:**
- **GPU or OpenGL rendering:** a new dependency and platform surface, and pixel results that
  depend on the driver.
- **Previews from the layer masks:** they carry no shading. PP1 keeps the gray projection only as
  the fallback without rendered previews.

**Validation:**
- **Codec unit tests:**
  - GOO, CTB and Photon store the given images exactly. CTB and Photon RLE15 is decoded
    independently, including white runs longer than 4096 pixels.
  - Layers are unchanged, only moved.
  - Wrong sizes fail before output.
  - PP1 writes RGB PNGs, and gray PNGs without previews.
- **Renderer unit tests:** a cube shows background corners and at least three face shades. The
  render is deterministic and independent of translation. An empty mesh and invalid sizes are
  covered.
- **CLI tests:**
  - Every machine format stores shaded previews.
  - STL, OBJ and 3MF still give byte-identical files.
  - Repaired and filled-hole hollow models still equal their closed references.
- **Independent validator:** `tools/m5-validate` decodes GOO and RLE15 previews, and reports
  PP1 PNG channels.
- **The 16 cross-platform CLI cases, compared with the previous build:**
  - GOO files differ only inside the thumbnail bytes.
  - CTB and Photon layers are byte-identical, moved by the preview size.
  - PP1 differs only in its two preview members.
  - The decoded previews look right (FX-12 raven, FX-11 sphere, boxes).

**Cost:** rendering FX-12 (2.7 M triangles) takes 0.14–0.18 s per image. The two images render
concurrently, so the 16K job grows by about 0.2 s.

**Not verified:** how physical printers and firmware display the previews. That belongs to the
physical print campaign.

## TD-058 — Parallel model import

**Status:** Implemented on 2026-10-19 at the owner's request. Every mesh, report and output is
unchanged.
Native Windows validation passed the same day: outputs identical to Linux, and FX-12 sliced at
16K in 1.9 s ([record](CLI-ENGINE-NATIVE-WINDOWS-VALIDATION.md)).

**Context:** after TD-056, importing FX-12 (2.7 M triangles) was the largest single-threaded
phase.

| Phase | Time |
| --- | --- |
| STL record decoding with welding through a SipHash map | 0.49 s |
| Geometry diagnostics | 0.27 s |
| Stored-normal diagnostics | about 0.04 s |
| OBJ import, total | 0.86 s |
| 3MF import, total (0.65 s inflating, the rest streaming XML) | 2.74 s |

**Decision:**
- **Exact parallel welding (`mesh::weld`):**
  - Corners are partitioned into 64 shards by a hash of their exact coordinate key.
  - Each shard walks its corners in increasing order with its own map and records each corner's
    first equal corner.
  - One sequential pass then numbers first corners in order of occurrence and maps every corner to
    its vertex.
  - The result equals the sequential first-occurrence builder for any thread count.
  - Maps use a fast multiply-mix hash seeded per process, so crafted collisions cannot be
    predicted.
  - STL, OBJ and 3MF all weld through it.
- **STL binary:**
  - Records are decoded in parallel, with the first non-finite coordinate found by a parallel
    smallest-index search.
  - Corner positions are decoded again from the input while welding instead of being copied.
  - Stored-normal findings are one code byte per facet, computed in parallel.
- **Geometry diagnostics:** degeneracy and duplicate-face keys are computed in parallel. The
  findings keep their order.
- **OBJ:**
  - The text is split into chunks at line boundaries that do not cut a continued line, and the
    chunks are parsed in parallel with their line offsets.
  - Negative indices are resolved after merging, relative to the vertices of earlier chunks.
  - The reported error is the one on the earliest line, as in a sequential pass. Resource limits
    are checked after parsing.
  - Faces are triangulated in parallel groups, and logical lines borrow the text unless continued.
- **3MF:**
  - Model parts are inflated on a second thread, in 1 MiB chunks through a bounded channel,
    overlapping inflation with XML parsing.
  - Vertex and triangle attributes are read in one pass, borrowing values that need no
    normalization.

**Validation:**
- A welding unit test compares the result with a sequential first-occurrence map over random
  inputs, including signed zeros and the vertex limit.
- A parallel smallest-index test.
- A multi-megabyte OBJ grid parsed in parallel chunks, with mixed index forms, negative indices
  and continued lines: it gives the expected triangles, and an error deep in the file names its
  exact line.
- The 16 cross-platform CLI cases (1217 files) are byte-identical to the previous build.
- FX-12 sliced from OBJ and from 3MF gives the same file as from STL.

**Measured** (Release, 16 threads):

| Case | Before | After |
| --- | --- | --- |
| FX-12 STL import | 0.79 s | 0.33 s |
| FX-12 OBJ import | 0.86 s | 0.54–0.60 s |
| FX-12 3MF import | 2.74 s | 1.36–1.40 s |
| FX-12 slice at 16K, whole process | 2.96 s | 2.44 s |
| FX-11 slice at 16K, whole process | 0.35 s | 0.31 s |

**Remaining:** 3MF is bound by streaming XML tokenization of a single part, about 1.3 s for
4 million elements. Parsing mesh sections in parallel would need the whole part inflated in
memory and is not done.

## TD-057 — Extensible model import: OBJ and 3MF

**Status:** Implemented on 2026-10-19 at the owner's request. The MVP had specified STL only; the
owner approved bringing OBJ and 3MF forward and chose the dependencies below.
Native Windows validation passed the same day, with outputs identical to Linux
([record](CLI-ENGINE-NATIVE-WINDOWS-VALIDATION.md)).

**Decision:**
- **Interface:** `mesh::formats::ModelFormat` with an id, a name, extensions, whether units are
  declared, and `parse` into a file-unit mesh with notes and diagnostics.
  - `FormatRegistry` selects a format by extension and refuses duplicate or invalid ids and
    extensions.
  - STL, OBJ and 3MF are built in, and external formats register the same way.
- **Units:**
  - The registry resolves units for every format.
  - A declared unit is used; a unitless format needs an assumed unit, and a contradiction is an
    error.
  - Millimetre meshes are unchanged; other units multiply world coordinates after all transforms.
- **Shared builder:** vertices are welded by exact coordinate bits, as in STL import. FX-01,
  FX-04 and FX-06 give identical meshes as STL, OBJ and 3MF.
- **OBJ** (standard library only):
  - `v`/`f` geometry in every index form, including negative and forward indices.
  - Polygons are split by ear clipping in the Newell plane, with the convex fan as the first ear.
  - Non-geometric statements are reported; free-form geometry and unknown statements are line
    errors.
- **3MF (Core 1.4 with Production):**
  - The package is read by an in-house bounded ZIP reader.
  - Parts are parsed as streaming XML, with DTDs and non-UTF-8 parts refused.
  - The start part is resolved through `_rels/.rels`.
  - Components and items use the row-vector transforms, and mirroring is re-wound outward.
  - `support` and `surface` objects are skipped and reported; `other` objects in the build are
    refused.
  - Required extensions other than Production and Materials are refused.
  - Objects must be defined before use, and an object holding both a mesh and components is
    refused.
  - Depth (32) and expanded triangles are capped before construction.
- **CLI:**
  - `--model <path>` with `--units <unit>`, for both `slice` and `inspect`.
  - `--assume-mm` stays as `--units mm`, still reading unknown extensions as STL.
  - The summary reports the model's format, unit, origin and objects, and the import notes.
- **Shared code:** `digest::Crc32` is the incremental CRC-32 used by the ZIP reader.

**Dependencies (owner-approved, exact pins; recorded per the dependency policy):**

| Item | `miniz_oxide =0.9.1` (with `adler2 2.0.1`) | `quick-xml =0.42.0` (with `memchr 2.8.0`, already present) |
| --- | --- | --- |
| Purpose | raw DEFLATE decompression of 3MF entries | streaming, namespace-aware XML |
| Native Rust / FFI / build script | pure Rust, `#![forbid(unsafe_code)]`; no FFI or build script | pure Rust, `#![forbid(unsafe_code)]`; no FFI or build script (`memchr` uses audited SIMD) |
| Licence | MIT / Zlib / Apache-2.0 | MIT |
| Maintenance | active; 0.9.1 released 2026-03-13; the inflate core of `flate2` and `std` backtraces | active; 0.42.0 released 2026-08-22 |
| Robustness | fuzzed in OSS-Fuzz; no open RustSec advisory | fuzzed in OSS-Fuzz; RUSTSEC-2026-0194/0195 (denial of service) fixed from 0.41 |
| Memory / performance | streaming state, about 43 KiB, no full-output buffer | event streaming; memory proportional to one event, not the document |
| Platforms | Windows and Linux, no target-specific code | Windows and Linux, no target-specific code |

Rejected alternatives:
- **`zip`:** large default feature set, advisories in its extraction code, and fast major churn.
  An in-house reader of about 500 lines fits our in-memory, non-extracting use.
- **`roxmltree`:** builds the whole document tree at 6–8 times the part size, which is too much
  for 25-million-triangle models.
- **An in-house inflate decoder:** high correctness risk for no benefit.

**Validation:**
- ZIP tests: stored, deflated, streamed and ZIP64 entries; CRC, size, limit, duplicate-name and
  truncation failures.
- `tests/model_formats.rs`:
  - registry selection, an externally registered format, and duplicate refusals;
  - one mesh across formats for three corpus models;
  - unit rules and every 3MF unit;
  - OBJ index forms, ignored statements, line continuation, NUL padding and every line error;
  - concave and reversed polygons, and the self-intersecting bow-tie;
  - 3MF components with mirroring under row-vector transforms;
  - support, `other`, required extensions, materials and Production parts;
  - eleven invalid models and package failures, including a CRC-only corruption.
- CLI tests: the same box in OBJ (mm and cm) and 3MF (mm and inch-declared) slices
  byte-identically to the STL; unit errors name the remedy; unknown extensions list the supported
  ones; `inspect --model` reports a 3MF.
- The 14 cross-platform STL cases (1215 files) are byte-identical to the previous build.

**Limits and future work:**
- Self-intersecting OBJ polygons that still have ears are split without detection.
- OBJ up-axis conventions are not converted.
- Thumbnails and metadata are not read.
- Real-world producer files are not yet part of the corpus.

## TD-056 — Parallel topology analysis

**Status:** Implemented on 2026-10-19 at the owner's request, after the CHITUBOX comparison
(benchmark). Every report, repair and output is unchanged.
Native Windows validation passed the same day, with outputs identical to Linux
([record](CLI-ENGINE-NATIVE-WINDOWS-VALIDATION.md)).

**Context:**
- After TD-055, FX-12 (2.7 M triangles) took 4.8 s at 16K. The single-threaded topology phase
  took 2.5 s of that, in three steps of about 0.8 s each:
  - the defect report;
  - the repair (duplicates and orientation);
  - the defect report of the repaired mesh.
- Inside each step, the time went to sorting about 8 M directed edge uses, sorting the vertices
  for canonical identities, sorting the face keys for duplicates, and computing face normals,
  twice per report.
- CHITUBOX slices FX-12 in about 2.2 s and performs no such checks.

**Decision:**
- **Parallel helpers:** a private `parallel` module provides a deterministic parallel sort:
  - chunks are sorted on scoped threads;
  - bottom-up merges follow, with each merge split by co-ranking so every round uses all cores;
  - ties go to the left run, so the result equals `sort_unstable_by_key` whenever equal keys mean
    equal elements;
  - when the merge buffer cannot be allocated, it sorts sequentially.

  The module also provides an index-exact parallel fill. Slices below 32,768 items per thread
  stay sequential.
- **Where it is used:** the defect report, the repair, the edge summary, the full topology report
  and the import duplicate check.
  - Every sort it replaces has unique keys (edge uses `(undirected, directed, face)`, vertices
    `(position key, index)`, repair keys ending in the face) or keys covering the whole element.
  - Face usability (a defined normal) is computed once per report, in parallel, and shared with
    duplicate counting.
- **CLI:** the defect report and the source repair of the imported mesh run concurrently, since
  both only read it. A diagnosis failure is still reported before a repair failure.
- **Cancellation:** it is checked before and after each parallel step; a step itself is bounded,
  about 0.1 s on FX-12.

**Validation:**
- Parallel sort tests compare it with the sequential sort, across sizes around the threshold,
  few and many distinct keys, and presorted and reversed input. Co-rank ties and the parallel
  fill have their own tests.
- All existing topology, repair and large-model tests pass.
- The 14 cross-platform CLI cases (1215 files) and their summaries, including defect and repair
  counts, are byte-identical to the previous build.

**Measured** (Release, 16 threads, 15120 × 6230, GOO; medians of three runs):

| Case | TD-055 | TD-056 |
| --- | --- | --- |
| Defect report, FX-12 | 0.83 s | 0.43 s |
| Repair, FX-12 | 0.81 s | 0.48 s |
| Topology phase, FX-12 (CLI) | 2.5 s | 1.07 s |
| FX-12, whole process | 4.84 s, 356 MB | 2.96 s, 521 MB |
| FX-12, 4× antialiasing | 4.98 s | 3.06 s |
| FX-12 hollowed | 8.86 s | 6.34 s |
| FX-11, whole process | 0.51 s | 0.35 s |

**Trade-off:** running the report and the repair concurrently raises the FX-12 peak by about
165 MB.

**Remaining:** FX-12's 3 s now splits about evenly into:
- import, 1.15 s: STL parsing and hash welding, single-threaded;
- topology, 1.07 s;
- slicing and export, 1.15 s.

## TD-055 — Span-mask layer pipeline

**Status:** Implemented on 2026-10-19 at the owner's request, after the open-source slicer
benchmark (record). All outputs are byte-identical. Native
Windows validation passed the same day, with outputs identical to Linux
([record](CLI-ENGINE-NATIVE-WINDOWS-VALIDATION.md)).

**Context:**
- At 16K (15120 × 6230, 94 Mpx), karasu needed 38 s and 1.5 GB for FX-11 (1200 layers). The
  open-source slicers measured were much faster: 3.7 s on one core and 5.6 s on 16.
- The geometry alone cost 2.2 s on one core (the same model on a panel with 10× larger pixels).
  The rest was per-pixel work. The rasterizer already produced row spans, but it wrote them into a
  dense 94 MB image per layer, and that image was then:
  - zero-filled at allocation, about 70 s of kernel page-fault time per job;
  - scanned pixel by pixel by the run-length encoder, the intensity sum and the PP1 metrics.
- The cost therefore scaled with the panel, not the model: FX-12, 7× smaller in volume, cost the
  same, and 4× antialiasing cost nothing extra.

**Decision:**
- **Representation:** `RasterImage` holds a `raster::mask::SpanMask`: per row, the sorted,
  disjoint, maximal runs of one nonzero value.
  - The form is canonical, so mask equality is image equality.
  - The rasterizer pushes its runs straight into it.
  - Compensation (morphology) still runs on a dense copy and converts back.
- **Encoders:** GOO, CTB, Photon V1 and PP1 PW0 encode from the mask's row-major runs, coalesced
  across rows and split at each format's run limit. Format quantization maps each run's value
  before coalescing. Intensity sums are taken per run.
- **Other consumers:**
  - PP1 scene metrics label components from the row spans.
  - The PP1 projection samples pixels by binary search in a row.
  - The reference PNG and CRC-32 stream one dense row at a time.
- **API change:**
  - `RasterImage::pixels()` is replaced by `mask()` and the fallible `to_pixels()`.
  - `SpanMask` offers `row`, `rows`, `pixel`, `from_pixels`, `to_pixels` and `retained_bytes`.
- **Budgets and windows:**
  - Raster preflight no longer reserves the dense image, except for compensation.
  - Retained spans are charged against `RasterLimits::max_payload_bytes` as they are produced.
  - `requested_payload_bytes` is the preflight scratch plus the retained spans.
  - The parallel raster and encoding windows hold at most two layers per thread. Finished layers
    wait only while their retained mask bytes stay within the window budget, and the next layer
    in order is always admitted.
- **Cancellation:** codecs poll per mask run instead of per pixel, since a run is constant work.

**Alternatives rejected:**
- **Reusing dense buffers:** this avoids page faults but keeps the per-pixel scans.
- **A global run list without rows:** PP1 labelling, previews and row streaming need row access.
- **Encoding directly inside the rasterizer:** this would couple formats to sampling and break the
  shared image used by previews, statistics and every format.

**Validation:**
- **Oracles:** the previous dense encoders (GOO, CTB, Photon, PW0) are kept as test-only oracles
  and match the run encoders byte for byte on 300 random multi-row masks each, including value
  mapping.
- **Mask round trip:** the dense round trip, canonical form, pixel lookup and cross-row runs are
  checked on random masks.
- **Existing tests:** dense rasterization references and PP1 flood-fill comparisons run through
  the masks.
- **Real outputs:** the CLI outputs below and the `print_reference` example outputs are
  byte-identical to the previous build:
  - FX-11 drained GOO;
  - FX-11 PP1 at 13312 × 5120;
  - FX-11 CTB with 4× antialiasing;
  - FX-10A closed, filled and hollowed;
  - FX-09 repaired;
  - FX-12;
  - FX-11 and FX-12 at 16K, binary and 4×, solid and hollowed.

**Measured** (Release, 16 threads, 15120 × 6230, 0.05 mm layers, GOO; medians of three runs):

| Case | Before | After |
| --- | --- | --- |
| FX-11 (327,680 triangles, 1200 layers) | 38.0 s, 1.52 GB | 0.51 s, 61 MB |
| FX-11, 4× antialiasing | 38.0 s | 0.67 s |
| FX-11, one core | 203.7 s | 1.29 s |
| FX-12 (2.7 M triangles, 990 layers) | 35.4 s, 1.66 GB | 4.84 s, 356 MB |
| FX-12, 4× antialiasing | 36.8 s | 4.98 s |
| FX-11 hollowed with a drain hole | 42.9 s | 3.21 s |
| FX-12 hollowed | 41.6 s, 1.83 GB | 8.86 s, 803 MB |

**Remaining costs:**
- Of FX-12's 4.8 s, import takes 1.0 s and the single-threaded topology phase 2.5 s: diagnosis,
  repair and diagnosis of the repaired mesh.
- Hollowing time is now dominated by the distance field.

## TD-054 — Defective meshes: diagnosis, section closure and explicit repair

**Status:** Approved on 2026-10-18 with the recommended defaults: diagnosis always, section closure
on with a 2 mm bound (fixed, D3), and `duplicates`/`orientation` repairs on by default. Phase 3
was approved into the MVP (D2) the same day. **All three phases are implemented:**
- phase 1: lean diagnosis and rim-paired section closure;
- phase 2: `mesh::repair` (duplicate collapse to net orientation, consistent orientation,
  opt-in outward for outermost shells; `--repair`, default `auto`);
- phase 3: winding-rule hollowing (inside at winding 1, overlaps and negative winding refused),
  and opt-in `fill-holes` as the plan's first operation, `OperationKind::Repair`.
 Specification: [OPEN-MESH-SPEC](OPEN-MESH-SPEC.md).

**Context:**
- Union slicing (TD-050) refuses meshes with holes or orientation conflicts, and fails layers
  crossing same-orientation duplicate faces or fins.
- Hollowing refuses anything that is not a closed edge-manifold surface (FX-12 for two duplicate
  faces).
- Thingi10K measures 11 % open, 22 % non-manifold and 10 % non-oriented models.
- Other slicers close or drop open section chains silently, some without any distance bound;
  others warn but never block, or refuse.

**Proposal:**
- **Diagnosis:** always on and lean: every defect class counted exactly, with rims (boundary
  loops and chains) and manifold components.
- **`close-sections`:** in each winding layer, open chains at a hole are closed by the chord
  across the hole.
  - Rim crossings are paired along the hole's rim, never by proximity, and bounded by an explicit
    gap in millimetres.
  - The result equals the slice's own winding number thresholded at 0.5 for simple holes.
  - It changes no mesh and is reported per layer.
- **Explicit repairs (derived meshes, reported):**
  - `duplicates`: drop same-orientation duplicate faces;
  - `orientation`: consistent orientation per manifold component by union-find with parity,
    keeping the larger-area class;
  - `orientation=outward`: inside-out closed components flipped by exact nesting depth.
- **`fill-holes`:** a preparation operation (minimum-weight triangulation of simple rims up to 256
  edges, validated closed and non-intersecting) for hollowing.
- **Hollowing:** classifies with the winding rule (signed ray count) instead of parity, so it agrees
  with the union fill on overlapping sources.

**Phase 3 refinements (implemented):**
- **Hollowing:** a point is inside at winding exactly 1. Winding 2 or more (overlapping bodies)
  and negative winding are refused instead of hollowed. The wall distance includes the faces
  inside an overlap, so a cavity there would keep internal walls. Admission requires balanced
  edges and no degenerate faces, so touching bodies are hollowed as one.
- **Repair in the plan:** `RepairSpec` (duplicates, orientation, outward, fill, allow-unfilled)
  is encoded in the canonical record and applied before admission. The repair is idempotent, so
  the plan reproduces the derived mesh from the bound source.
- **Fill weight:** the dihedral criterion is the largest fold (one minus the cosine between unit
  normals), then area. Caps that would duplicate a mesh edge or meet the mesh improperly are
  refused, one hole at a time, until the accepted set validates together.
- **Slicing:** refused holes stay open and are handled by section closure. In a hollowing plan,
  every hole must be filled.

**Alternatives rejected:**
- **Proximity gap closing:** it can join unrelated chains and is
  unbounded or silent.
- **Per-pixel 3D generalized winding numbers:** too costly at LCD resolution; the fast variant has
  no error bound.
- **Hole filling or voxel remeshing at import:** it changes topology silently, and MeshFix-like
  repair deletes components.
- **Heuristic row repair in the raster:** the result depends on neighbouring rows.

## TD-053 — Large-model support

**Status:** Implemented on 2026-10-18 at the owner's request, validated with the owner-supplied
FX-12 real model and a synthetic 21-million-triangle sphere.

**Context:**
- The default import limits (64 MiB, 1,000,000 triangles) refused FX-12: 2.7 million triangles,
  128 MiB.
- With the limits raised, FX-12 sliced correctly. Its 990 layers at 13312 × 5120 took 20.5 s, but
  model handling needed about 400 bytes per triangle:
  - duplicate-triangle detection hashed nine coordinates per face, twice (at import and again in
    topology), about 90 bytes per triangle;
  - CLI admission ran the full topology report (about 270 bytes per triangle) to read four counts;
  - the CLI kept the file bytes, facet metadata and per-facet diagnostics until the end;
  - vertex heights were gathered per face corner and kept at full capacity.
  Scaled to 20 million triangles this is about 8 GB.

**Decision:**
- **Limits:** default import limits are 1,280 MiB, 25,000,000 triangles and 75,000,000 vertices.
  Topology defaults follow them.
- **Duplicate detection:** canonical vertex identities (sorted vertex keys) plus sorted
  canonical corner triples, with identical diagnostics.
- **Lean admission:** `TriangleMesh::edge_summary_with_cancellation` returns the report's edge
  counts from one sorted word per directed use. The CLI admits union models with it; strict
  admission keeps the full report, because it needs vertex manifoldness.
- **Early release:** the CLI computes the source digest only when hollowing and releases the file
  bytes, facet metadata and diagnostics right after import.
- **Vertex heights:** each vertex is evaluated once, at its first use in face order (same error
  context), and the deduplicated heights are shrunk.
- **Binary STL:** triangle and facet buffers are reserved exactly from the validated record count.

All outputs are unchanged. Tests compare duplicate detection with the coordinate-hash algorithm, and
the edge summary with the full report on the corpus and random meshes.

**Measured** (Release, 16 cores):

| Model | Import | Admission | Slicing at 13312 × 5120 | Peak |
| --- | --- | --- | --- | --- |
| FX-12, 2.7 M triangles | 1.0 s (was 1.6 s) | 0.4 s (was 2.3 s, +720 MB) | 990 layers, about 20 s | 0.36 GB on a 1000 × 600 panel (was 1.1 GB) |
| Sphere, 21.0 M triangles, 1 GB STL | 8.8 s | 3.2 s | 1200 layers, 21 s; 34 s in total | 2.7 GB |

**Remaining limits:**
- The importer holds the file bytes, facet metadata and per-facet diagnostics until import ends,
  about 120 bytes per triangle above the file itself.
- The full topology report (`inspect`, strict admission) needs about 6 GB at 20 million triangles.
- Hollowing keeps its own 3,000,000-vertex source limit. It required an edge-manifold source, so
  FX-12 (two duplicated faces) was refused explicitly; since TD-054 the default repair makes it
  admissible.
- Import deduplication is single-threaded.

## TD-052 — Antialiasing in the CLI

**Status:** Implemented on 2026-10-18 at the owner's request.

**Context:**
- The M4 raster already supports exact N×N centre supersampling with linear coverage and grey
  quantization, but the CLI produced binary masks only.
- Format headers recorded fixed antialiasing values: GOO factor 1, CTB flag 0x07, PP1
  `anti_count` 1.

**Research:** from the files other slicers write.
- Lychee 8.0.0 offers anti-aliasing with an optional grey floor, off by default, and writes the
  level only to PP1 `anti_count`.

**Decision:**
- **Option:** `--antialias <1|2|4|8|16>`, default 1 (binary). It uses the M4 supersampling
  unchanged: no gamma, no grey floor and no blur, because those are calibrations that would need
  their own explicit options.
- **Depth:** masks are generated at native depth: GOO 256 levels and PP1 16 levels, both stored
  exactly. CTB uses the explicit nearest 7-bit level policy (`PixelPolicy::RoundToFormat`).
  Legacy Photon is refused rather than thresholded.
- **Declaration:** `BuiltinExporter::with_antialiasing` declares the level, and every mask must
  match it or the export fails.
- **Recording:**
  - GOO: antialiasing factor N, grey and blur levels 0.
  - CTB: slicer-information flag 0x0F when N > 1; the header's antialiasing planes stay 1,
    because CTB stores grey per pixel.
  - PP1: `anti_count` N.
  - Photon: none.

**Measured:** FX-11 (1200 layers) at 13312 × 5120 as GOO takes 21.2 s at 2×2, 23.6 s at 4×4 and
21.2 s at 8×8, against 20.1 s for binary masks. The span rasterizer charges per span endpoint, so
supersampling multiplies only the sub-rows. The sampled volume changes by under 0.001 %.

**Open:** whether firmware reads these fields, and P1 `anti_count` semantics (level per axis or
sample count), remain for hardware validation (M7).

## TD-051 — PP1 scene metrics on encoder workers

**Status:** Implemented on 2026-10-18 at the owner's request, after the 16K benchmark showed PP1
export taking 200 s for 1200 layers against about 20 s for GOO and CTB.

**Context:** the P1 `scene.slice` record of every layer (area, bounds, 8-connected components and
the largest one) was measured on the in-order writer thread. The labelling scanned every pixel and
made several full-width passes per row, even empty ones: about 0.15 s per 68-Mpx layer, serially.

**Research:** from the files other slicers write. CHITUBOX 1.4.0 writes only a raster area and
placeholders for the rest; Lychee 8.0.0 writes placeholders only.

**Decision:**
- Measure the metrics with the PW0 payload in the pure per-layer encode step, on the parallel
  encoder workers (`EncodedPayload::scene`). The writer appends the record in order and keeps the
  same checks.
- Label components per run instead of per pixel, with word-at-a-time skipping of uniform stretches
  and scratch proportional to the width.
- Keep Karasu's real metric definitions unchanged; the output bytes are identical.

**Measured:** FX-11 (1200 layers) at 13312 × 5120 as PP1 went from 199.8 s to 17.7 s, with a
byte-identical file, in line with GOO (20.1 s) and CTB (19.5 s).

**Alternatives rejected:**
- **Writing placeholders like the commercial writers:** it discards information that now costs
  almost nothing, and nothing shows the firmware needs placeholders.
- **Parallel workers with the pixel algorithm:** it would still spend about 0.15 s of CPU per layer
  for little benefit.

## TD-050 — Winding-union layers and layer parallelism

**Status:** Approved by the owner on 2026-10-18, with three decisions: the CLI default fill becomes
`union` once its validation passes, the CLI uses all cores, and parallelism comes first. All parts
are implemented, and `union` is the CLI default. Specification:
[ENGINE-UNION-PARALLEL-SPEC](ENGINE-UNION-PARALLEL-SPEC.md).

**Implemented (winding-union layers):** `slicing::winding`, `raster::parallel::LayerFill`.
- Combinatorially oriented segments on vertex-avoiding planes; an exact balance check by crossing
  position fails open or inconsistently oriented sections with the offending edges.
- Signed-crossing raster under `Positive`, `NonZero` or `EvenOdd`; negative winding under
  `Positive` fails the layer instead of printing an inside-out model as empty.
- Raster samples are now classified exactly against each pixel-space edge (exact orientation sign
  from an interpolated estimate), for strict and union layers alike: masks no longer depend on edge
  direction or interpolation rounding, so union masks equal strict masks on every strict-accepted
  input by construction. Existing outputs were unchanged.
- U1–U6 pass (corpus and FX-11 identical to strict; overlapping, touching, kitbash,
  self-intersecting and random bodies equal a solid-angle winding-number oracle).
- CLI `--fill union|strict` (default `union`); union admission accepts non-manifold contacts but
  refuses boundary edges and winding conflicts without `--allow-topology-defects`. The summary
  reports layers that merged overlapping material.

**Implemented (parallel layer stream):** `raster::parallel::with_parallel_raster`.
- **Determinism:** workers run the engine's own per-layer kernel over a shared immutable face Z
  index with per-worker cursors, so output is byte-identical for every thread count.
- **Ordering:** an in-order reorder window bounded by `min(2 × threads, 1 GiB / per-layer
  preflight)`; the first error in layer order wins.
- **Cancellation:** forwarded to every worker.
- **Measured:**
  - 400 layers at 13312 × 5120: from 164 s to 36 s;
  - FX-11 hollow and drained at 1000 × 600: from 12.5 s to 4.5 s;
  - 16 cores; identical outputs.
- **Parallel encoding:**
  - machine codecs split into precheck, pure encode and emit;
  - `ExportSession::layer_encoder` and `write_encoded` have defaults;
  - `export_machine_parallel` pulls and checks in order, encodes in parallel within a 512 MiB
    window and writes in order;
  - files, summaries and first failures are identical to `export_machine`;
  - 400 layers at 13312 × 5120 now take 20.5 s (164 s sequential).
- **Span rasterization:** rows are written as constant-coverage runs from the sorted sub-row span
  endpoints; uncovered pixels are never visited and statistics come from the runs when no
  compensation is applied. Masks are byte-identical; 400 layers at 13312 × 5120 take 8.4 s.

**Context:** compared with commercial slicers, the engine leads on correctness: exact predicates,
certified walls and openings, determinism, bounded resources and cancellation. It trails on input
tolerance (layers with touching, overlapping or self-intersecting bodies are refused) and on
throughput: 0.41 s per layer on one core at 13312 × 5120, so about 8 minutes for 1200 layers on a
16-core host.

**Proposal:**
- **Winding-union layer mode** (opt-in at first):
  - per-layer oriented section segments on vertex-avoiding planes (TD-048);
  - an exact balance check by canonical edge identity (open or inconsistently oriented layers fail
    with the edges involved);
  - a signed-crossing raster under an explicit `Positive`, `NonZero` or `EvenOdd` rule.
  It must reproduce the strict masks byte for byte on every input the strict pipeline accepts.
- **Parallel layer stream:**
  - an immutable Z stabbing index;
  - workers over contiguous layer chunks;
  - an in-order reorder window bounded by a memory budget;
  - consumer-thread cancellation forwarded to workers;
  - the first error in index order wins;
  - byte-identical output for every thread count;
  - then per-layer parallel encoding.

**Alternatives rejected:**
- **Extending the exact constructive union (TD-030) to whole layers:** it is quadratic, needs simple
  closed rings per body and has 4096-edge defaults.
- **Silent mesh repair or voxel remeshing:** forbidden, and it changes geometry.
- **GPU rasterization:** a new dependency and platform surface; CPU parallelism should come first.

**Open decisions:** the open-mesh policy.

