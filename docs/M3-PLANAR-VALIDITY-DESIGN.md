# M3.4 Entry: Planar Validity and Numerical Representation

**Status:** Detection-only staging/scope approved by the owner on 2026-10-16 after research commit
`b1b58ba`; M3.4-A is implemented at the narrow scope below. No TD-015–022 mathematical behavior or
dependency is replaced. The integer-polygon conversion/construction gate below remains open.

Sections 1–9 retain the research rationale and candidate alternatives; Section 10 records the actual
approved implementation contract. Unselected area/grid/acceleration alternatives remain proposals.

**Baseline:** `main`, `c4865117e18d998fe80d48d8fac1022b5731ebc8`.
**Trace:** ROADMAP M3; GEO-001; SLICE-002/003; TD-005/014/019/021/022.

## 1. Question and scope boundary

What independently justified arithmetic and contact policy can assess the represented XY embedding
of existing paths without silently changing connectivity, source observations or material topology?

This proposal separates four claims:

1. Graph continuity: already supplied by M3.3-B, including explicit open and ambiguous trails.
2. Per-path planar embedding: noncollapsed edges and permitted versus forbidden contacts.
3. Compatibility between paths: distinct simple rings may still touch, cross, overlap or coincide.
4. Filled material: ownership, orientation roles, containment, holes and Boolean interpretation.

The approved first implementation scope is detection only (claims 2 and contact data for 3), not
claim 4. Implementation and validation details are recorded in Section 10, not whole-M3 acceptance.
No area, grid conversion, intersection construction, ring rewinding, splitting, welding, repair,
automatic closure, region, layer, raster or CLI feature is implemented by this document.

Existing `ClosedCycleCandidate` remains graph-simple, not a valid polygon. Never modify its
canonical direction to imitate clockwise/counterclockwise ordering. Existing complete-group outcomes
and all source observations, multiplicity and provenance remain accessible and unchanged.

## 2. Inputs and interpretation

Proposed input is one immutable, privately constructed `SectionPaths`, borrowing its graph,
normalization and raw section, plus explicit resource limits and optional TD-014 cancellation.
Reuse the graph's finite agreed world-space positions in millimeters and the supplied plane; do not
recompute source intersections, resolve conflicts by choosing a representative, or rescan the mesh.

All proposed exact geometric decisions concern **the represented binary64 XY values**. Exact
predicates cannot restore ideal geometry lost during STL encoding, transformation or M3.1
construction. Disjoint represented segments do not certify a gap in the ideal unrounded source
section; touching represented segments do not prove intended source contact.

Source-feature identity and coordinate equality remain different. Equal XY at different graph IDs
is a geometric contact observation, not permission to connect the features. Normalize signed zero
for numeric comparison only; retain all source data. No classification epsilon or proximity band.

Inspection includes edges of open/ambiguous paths as well as cycle candidates, so contacts with them
are not silently omitted. A path with graph uncertainty is never promoted to an eligible ring even
if its endpoint positions coincide. Complete-only unresolved/internal groups and isolated tangent
points remain inherited, explicitly unassessed context, not certified absent geometry. The result
must disclose that this is a path-embedding analysis, not a full section/material-boundary proof.

## 3. Proposed segment-contact taxonomy

Apply collapsed-segment handling before nondegenerate segment logic. Contact classification and
collapse flags are separate, so a collapsed segment may also lie on another segment. No intersection
coordinate construction is needed for this detection stage.

| Relation | Mathematical meaning on represented coordinates |
| --- | --- |
| Disjoint | Empty intersection of the two closed segments |
| Proper crossing | Both segments are noncollapsed and their intersection lies strictly inside both, with noncollinear supporting lines |
| Endpoint contact | A single noncollinear contact at an endpoint of each segment |
| Endpoint–interior contact | A single noncollinear contact at an endpoint of one segment and in the other's strict interior |
| Collinear point contact | Collinear closed segments meet at exactly one point; retain endpoint/interior roles separately |
| Positive overlap | Collinear closed intersection has positive length |
| Coincident segments | Positive overlap with equal unordered endpoint coordinates; a specialization, not a second duplicate finding |
| Collapsed contact | A point–segment or point–point contact; retain which inputs collapsed and whether the point is an endpoint/interior |

For noncollapsed AB and CD, exact signs of orient(A,B,C), orient(A,B,D), orient(C,D,A),
orient(C,D,B) decide strict crossings. Zero signs require explicit closed-interval membership.
For fully collinear segments, project onto a nonconstant coordinate (X if available, otherwise Y)
and compare closed interval endpoints directly, without subtraction. Distinguish empty, singleton
and positive-length intersection. Use exact endpoint equality for the coincident specialization.
Collapsed cases use exact equality and point-on-segment orientation/closed bounds.

Within a single path, consecutive edges may meet **only at their prescribed common graph endpoint**.
For a closed cycle, first/last edges are consecutive; there is no other adjacency exemption. Still
classify those pairs: collinear backtracking/overlap is invalid, not excused by adjacency. Any
nonadjacent contact is incompatible with simple embedding. Different source IDs at identical XY
do not receive an identity-based exemption.

Between paths, every contact remains a separate compatibility diagnostic. Even an endpoint shared
by branch-stopped trails is recorded, not routed through. Contact alone does not assign material,
union/intersection, hole roles or a repair instruction. A per-path simple result does not mean the
whole set is mutually disjoint, correctly nested or ready to fill.

## 4. Independent arithmetic alternatives

| Candidate | Correctness and range | Cost and scope | Recommendation |
| --- | --- | --- | --- |
| Reuse/generalize the existing fixed-limb exact XY sign | Exact determinant sign for finite represented binary64 inputs; no grid | Bounded stack arithmetic, no dependency; review private-module extraction and preserve every old test/API | Preferred candidate for the first detection reference, subject to implementation approval |
| Filtered/adaptive exact sign | Exact only with a proved error filter and complete exact fallback | Potentially faster ordinary cases; more branches, proof and portability testing | Compare later against the correctness reference; do not add a dependency now |
| Naive f64 determinant plus epsilon | Can misclassify cancellation/extremes; a tolerance also changes geometry | Small code, insufficient contract | Reject for exact represented-value contact claims |
| Quantize then use checked integer signs | Exact for the chosen grid, not the original represented geometry | Requires scale/origin/range/rounding and changed-topology policy | Blocked until Section 5 is resolved |

The existing exact sign uses every f64 as an integer times eta=2^-1074. Magnitudes need at most
2098 bits, differences 2099, products 4198 and determinant 4199; 132 base-2^32 limbs (4224 bits)
suffice. These are arithmetic units, **not** a physical polygon quantization step. Reusing the
implementation requires a separately approved production change, not copying it into a second
private implementation. Its finite-input precondition must remain explicit and validated.

Exact orientation plus numeric coordinate ordering suffices for segment-contact classification;
there is no reason to construct and round an intersection point merely to detect one. This does
not settle the later representation of constructed polygon Boolean results.

## 5. Integer representation/scale gate: precise unresolved decision

**OPEN for construction/lossy conversion — do not declare the roadmap gate satisfied.** The
subsequent ExactOnly coordinate contract below does not close those gates. No approved positive
physical quantization error,
supported polygon extent, minimum feature/gap acceptance policy or polygon-construction algorithm
currently determines a justified scale. RP-001 is a research control, not a universal range limit.
Printer naming and commercial constants cannot fill these missing inputs.

Compare retaining represented f64 with exact predicates, a fixed mm grid, and a recorded per-job
origin/scale. A reviewed conversion must define:

- coordinate space and origin; finite extent domain and range rejection;
- step/scale, rounding including ties and negative values, and conversion arithmetic;
- independently derived quantization plus conversion-rounding error in mm;
- collapsed edges, new/lost contacts, changed winding and other topology-change diagnostics;
- storage bounds and **all intermediate** determinant, construction and area-sum bounds;
- reproducible metadata and rules for combining differently scaled inputs.

For ideal exact nearest rounding to step h, each axis error is at most h/2 and Euclidean error
at most h/sqrt(2). The implementation of conversion needs its own floating arithmetic analysis;
the ideal bound is not automatically a bound for `round(x*scale)`. No positive h preserves all
arbitrarily narrow gaps/features. Distinct represented points may map to one grid point.

If integer coordinates obey |q|<=B, differences obey |dq|<=2B and the elementary orientation
bound is |det|<=8B^2. Twice shoelace area over N vertices has elementary bound <=2*N*B^2.
Storage width alone does not prove either sum/product safe. Bounds must be evaluated with checked
or wider arithmetic, not overflow while testing their own safety.

**Approved owner staging decision (2026-10-16):** allow only a detection-only, unquantized represented-f64
reference increment under Section 3, while explicitly retaining the integer-conversion gate before
polygon construction/region work. This explicit TD-023 staging exception permits M3.4-A only;
it does not satisfy or waive the physical conversion gate. No dependency is introduced.

Subsequent M3.4-C research and
[integer-grid contract](M3-INTEGER-GRID-CONTRACT.md) compare the alternatives. The owner subsequently
approved TD-025's per-job binary/ExactOnly direction and arithmetic domain, authorizing design-only
finalization. After contract commit `75acecd`, the owner authorized the bounded implementation;
its actual validation is recorded separately in the test strategy. It supplies no positive loss
budget and does not close the construction/region
requirements above. The original research checkpoint remains historical, not blanket grid approval.

## 6. Area, orientation, containment and material: later contracts

Area is measured in mm^2. Signed shoelace area is algebraic area; nonzero sign does not prove
simplicity, and zero sign alone does not describe the cause. Do not use area sign as a substitute
for contact testing. The existing architectural outer-CCW/hole-CW convention applies only after
valid geometry and material/containment roles are established.

An exact sum over finite binary64 points is possible in dyadic arithmetic, but its range proof is
not the fixed three-point predicate proof. With coordinate integers of magnitude <2^2098, an
N-edge twice-area sum has magnitude <N*2^4197, so a sufficient magnitude bit budget is
4197+ceil(log2(max(1,N))) (with a separately handled sign). Checked count/domain limits are needed.
An exact sign and a finite f64 area magnitude are different outputs; conversion may overflow or
underflow. Compare exact sign/accumulation against a checked compensated estimate with a rigorous
uncertainty bound before implementing an area API. No accumulator or width is selected now.

The subsequent [M3.4-B contract proposal](M3-AREA-ORIENTATION-CONTRACT.md) performs that comparison
and proposes a full-domain 134-limb exact reference with one correctly rounded magnitude conversion.
The owner subsequently approved TD-024 and the measurement-only implementation on 2026-10-16;
this section's historical detection-only checkpoint is not retroactively changed into area approval.

Nesting later requires boundary-aware containment, explicit touch/overlap policy and outer/hole/
island semantics, not a centroid heuristic or winding guessed from source-ID path direction.
Parity and nonzero winding can interpret self-intersecting/multiple boundaries differently. Do not
choose a fill rule for ambiguous input here. Do not infer global material from local coplanar
ownership or from an absence of path-contact findings.

EXP-03 still gates final plane scheduling; EXP-04 remains compatibility research. This explicit-XY
contract neither specifies layer inclusion nor completes either experiment or M1.

## 7. Proposed reference algorithm, deterministic output and bounds

Prefer a straightforward reference scan before a sweep implementation. Enumerate path edge
references by (path ID, edge offset), classify each edge's collapse, then each unordered pair once
in lexicographic reference order, including cross-path pairs and adjacent pairs. Bounding-box
rejection is permitted only if proved conservative on represented endpoints and it does not
change pair accounting, diagnostic order or error precedence. No geometric splitting is performed.

For E retained path edges, pair count is E*(E-1)/2, evaluated with overflow-safe checked arithmetic
(divide an even factor before multiplication). Charge every unordered pair, including AABB rejects
and allowed adjacent contacts, against an explicit comparison cap. Report actual examined counts;
never return complete success after stopping at a cap. This O(E^2) reference is a bounded
correctness baseline, not a claimed large-scene solution. A sweep/index may later improve cost but
needs independent proofs/tests for vertical/coincident edges, event ties and overlap reporting.

Per-path statuses should distinguish ineligible open/ambiguous input, non-simple/collapsed embedding
and simple cycle embedding. Cross-path contacts are separate records and cannot silently change
these local statuses or imply a valid region. Preserve original path IDs, offsets, graph groups
and endpoint feature IDs through borrowed access; retain complete-only uncertainty separately.
There is no global `printable` or `valid_material` shortcut.

Proposed public entry points are ordinary and cancellation-aware inspection over `SectionPaths`,
with a separately owned/borrowed immutable report following existing conventions. Exact Rust type
names/signatures are proposals until implementation approval; no placeholder API is added now.

Limit input vertices/groups/paths/edge references, potential pair work, actual diagnostic records
and aggregate requested buffer capacity. Check usize/isize counts/byte arithmetic before fallible
allocation. Borrowed input, allocator overhead and exact-predicate stack are excluded explicitly
from requested-byte accounting. Numeric safety defaults must be documented as configurable policy,
not performance measurements or a hard RSS promise. No all-pairs matrix/per-pair heap allocation
is necessary; scan pairs and emit only findings. Retained storage is O(P+D) plus any explicitly
budgeted O(E) reference index; E is not squared in storage.

Typed operational failures should distinguish cancellation, resource/count/byte exhaustion,
allocation/overflow and inconsistent privately constructed input. Geometric invalidity is retained
diagnostic data, not a fabricated successful polygon and not disguised as an allocation error.
Too many findings must fail atomically rather than silently truncate an apparently complete report.

TD-014 polling is required at entry, validation, bounded inner pair loops, allocation boundaries,
diagnostic assembly and final publication, including empty input. Use deterministic tests, not
sleeps. Individual fixed-size exact predicates, allocations, bulk operations and cleanup remain
indivisible. Polls are not a progress API or strict wall-clock guarantee. Errors before a later
checkpoint keep their ordinary precedence; cancellation publishes no successful partial report.

## 8. Independent deterministic test specification — not production tests run

| Family | Independent expected property |
| --- | --- |
| Convex/concave rings | Simple embedding; no inferred hole/material role |
| Adjacent collinearity | Forward subdivision allowed; reverse/backtracking overlap rejected |
| Bowtie and nonzero-area crossing ring | Both non-simple regardless of area sign |
| Nonadjacent equal XY/different IDs | Contact recorded without welding |
| Point, T and collinear contacts | Exact relation and endpoint/interior roles; no constructed intersection |
| Partial/complete/reversed overlap | Positive overlap and exact coincidence specialization without duplicate records |
| Collapsed segments | Point–point/point–segment roles plus collapse; no silent deletion |
| Two individually simple rings | Disjoint, touching, crossing, overlapping and coincident relationships remain distinct |
| Near-collinear/one-ULP gaps | Exact represented geometry, not a proximity threshold |
| MAX, subnormal, signed-zero cases | Correct signs/closed comparisons without overflowing f64 differences/products |
| Reversal/cyclic/permutation | Same remapped contacts, canonical diagnostic order for identical IDs |
| Analytic/rational oracle | Independent bounded integer/rational arithmetic, not a second call to the production predicate |
| Dyadic metamorphic cases | Translation/scaling only under explicitly lossless representation/domain conditions |
| Complete-only graph context | Unresolved/internal/tangent data preserved; no whole-section material claim |
| Resource/cancellation/retry | Exact/one-below caps, inner-loop and final cancellation, no partial publication, unchanged inputs |
| Quantization counterexamples | Grid-induced collapse/new contacts exposed; not treated as unchanged topology |

Later area/nesting tests must add known rational area, cancellation-sensitive sums, outer/hole/island,
touch versus strict containment, boundary queries and concave rings with an exterior centroid.
Preserve all existing owned STL files; small analytic indexed inputs are sufficient for most tests.
Run Debug/Release, full stable quality gates and subsequent native Windows tests only after a
separately authorized implementation exists. M3 contour/polygon fuzzing and representative timing/
memory measurements remain milestone work, not results of this design document.

### Executed independent mathematical checks

An external, dependency-free Python `Fraction` model compares four-sign/interval detection against
an independently solved parametric line/segment intersection oracle. On 6,561 ordered small-grid
segment pairs, all **26,244** base/reversal/swap assertions pass: 3,128 disjoint, 2,601 singleton
contacts, 560 proper crossings and 272 positive overlaps in the base inventory. Another **24** exact
dyadic transformations pass. Separate assertions cover signed zero, extreme/subnormal orientation,
zero- and nonzero-area self-crossings, quantization collapse and count-dependent accumulator bounds.
The singleton model does not separately test every proposed endpoint/interior diagnostic field.

These checks ran successfully with exit 0 during this research increment. They test mathematical
models and counterexamples, not Rust code, allocator/cancellation behavior, commercial behavior,
native-platform portability or worst-case performance. Finite enumerations support but do not
replace the stated arithmetic arguments. Raw model, output and independent note remain external.

## 9. Independent sources and review gate

Conceptual mathematical sources, not adopted source code or dependencies:

- [Shewchuk: adaptive precision and robust predicates](https://www.cs.cmu.edu/~quake/robust.html).
- [CGAL: predicates versus constructions](https://doc.cgal.org/Manual/3.1/doc_html/cgal_manual/Kernel_d/Chapter_predicates_constructions_d.html).
- [CGAL: polygon simplicity and bounded-side concepts](https://doc.cgal.org/latest/Polygon/group__PkgPolygon2Functions.html).
- [Haines: point-in-polygon strategies](https://erich.realtimerendering.com/ptinpoly/).

The owner approved the staged numeric exception in Section 5 and the first detection-only scope.
Any later scale, dependency, fill/repair rule, exact-area representation or acceleration
needs its own justified decision. M3, M1 and preparation/GUI milestones are not completed here.

## 10. Implemented M3.4-A contract

`slicing::validity::{inspect_planar_paths, inspect_planar_paths_with_cancellation}` borrow
`SectionPaths` and take `ValidityLimits`. `PlanarInspection` retains the input reference, one
`PathEmbedding` per original path, ordered `PlanarDiagnostic` records and `ValiditySummary`.
`PathEdgeRef { path, offset }` resolves unchanged graph-group/source-feature/raw provenance.

`ContactKind` distinguishes proper crossing, noncollinear endpoint/endpoint and endpoint/interior,
collinear singleton, positive overlap, coincident unordered endpoints and collapsed contact.
Singleton fields name Start/End/Interior/Collapsed on each oriented path edge. Disjoint and
permitted consecutive singleton pairs have no diagnostic but still count as examined pairs.
Collapse findings precede pair findings; both phases use lexicographic path/offset ordering.
First/last adjacency exists only for a graph-closed path; overlapping/backtracking adjacency is
not exempt. Cross-path contacts never downgrade an individually simple cycle or assign material.

`EmbeddingStatus` is Ineligible for all open/ambiguous paths, NonSimpleCycle for cycle candidates
with collapse/forbidden self-contact, otherwise SimpleCycle (represented path embedding only).
Complete-only internal/unresolved groups and isolated vertices are counted as unassessed; complete
upstream diagnostics remain borrowed, not copied or certified away. Finite agreed positions are
required only for retained path endpoints; no conflicting representative is selected.

Defaults are configurable safety policy: 3,000,000 graph vertices/groups/paths/path edges,
6,000,000 path vertex references, 1,000,000 unordered comparisons and actual diagnostics, and
512 MiB aggregate requested buffer capacity. They are not measured throughput or accepted job size.
Checked potential-pair count rejects excess before allocation. Actual diagnostics grow fallibly,
with geometric capacity growth clipped to count/byte caps. Requested bytes include edge scratch
and path records plus charged diagnostic capacity, not borrowed input, allocator over-allocation,
stack or process RSS. All-pairs storage is never allocated.

For V graph vertices, G complete groups, P paths, E retained edges and D findings, the implementation
uses O(V+G+P+E^2) work and O(P+E+D) requested storage. Strict finite endpoint AABB rejection is
conservative and does not alter pair accounting or canonical order. Fixed-limb exact orientation
is moved without arithmetic changes to shared private `slicing::exact`; old sign tests are retained.

Errors distinguish Cancelled (with source), LimitExceeded(resource,limit), SizeOverflow, Allocation
and InvalidPaths(path,graph edge when known). Entry polling precedes limits; coarse identity/count
checks precede reservation, finite endpoint checks occur while indexing, actual finding caps precede
insertion, and final polling precedes publication. There is no successful partial report.
ValidityStart/Input/Storage/Pairs/Diagnostics/Complete extend TD-014. Loops poll every 256 from zero,
including the global inner-pair count; each finding and reservation boundary also polls. Fixed-size
predicates, allocations and cleanup remain indivisible, without progress or latency guarantees.

The test strategy records actual Debug/Release, independent-oracle, fixture, cancellation/resource
and platform results. No area/orientation, scale conversion, intersection construction, nesting,
region, raster, layer, repair or CLI API is implemented. Future constructive work still needs its
numeric contract and approval.
