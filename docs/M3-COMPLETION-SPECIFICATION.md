# M3 Completion Specification

## 1. Status

This document was the implementation-ready **proposal** for the remaining M3 work. TD-015 through
TD-025 remained authoritative when it was written. TD-026 and the finalized
[M3.5-A contract](M3-REGION-HIERARCHY-CONTRACT.md) now supersede its strict-region proposal;
TD-027 and the finalized [schedule contract](M3-LAYER-SCHEDULE-CONTRACT.md) supersede its schedule
proposal. TD-029 and the [layer engine contract](M3-LAYER-ENGINE-CONTRACT.md) supersede its
orchestration proposal. TD-030 and the
[constructive contract](M3-CONSTRUCTIVE-MATERIAL-CONTRACT.md) supersede the conditional exact
composition proposal. The [completion audit](M3-COMPLETION-AUDIT.md) separates implementation
from remaining acceptance. This document does not
change M3.1 intersection, M3.2 ownership, M3.3 graph/path, or M3.4-A/B/C contracts.

M3.5-A strict exact region hierarchy, M3.5-B explicit uniform scheduling and M3.6 owned layer
orchestration are implemented. The later owner-authorized M3.5-C exact constructive operation is
also implemented, with explicit source-material semantics rather than implicit union.


**Closure update:** M3 is now accepted under the
[final corpus/budget/native-profile record](M3-CLOSURE-ACCEPTANCE.md). TD-031 removes the mandatory
grid bottleneck through an explicit represented-coordinate mode; TD-032 resolves coherent opposite-
side shared-edge crossings discovered by the complete corpus. Earlier pending statements below
retain proposal history. Native functional acceptance uses the full MSVC Release suite, while
Debug host Code Integrity errors remain explicitly documented.

## 2. Completion boundary

M3 is complete only when the agreed corpus can be processed from immutable M2 geometry into ordered,
owned, deterministic layer geometry with explicit diagnostics. Completion still requires:

- disconnected material regions and nested holes;
- an explicit fill/ownership policy;
- open, ambiguous, tangent, and coplanar context represented separately from material;
- explicit first plane, step, terminal inclusion, and layer limits;
- cancellation, progress, and resource behavior for multi-layer work;
- contour/region fuzzing, performance/memory evidence, and native Linux/Windows validation.

M3 does not rasterize, select a printer, attach exposure/lift metadata, repair a mesh, or choose an
export format.

## 3. Reused immutable inputs

The remaining stages must borrow or consume only reviewed outputs:

- `MeshSection` and its full source-face/event provenance;
- `NormalizedSection` with every occurrence and unresolved group retained;
- `IncidenceGraph` and `SectionPaths` with complete accounting;
- `PlanarInspection` with local embedding and all cross-path contacts;
- `SectionAreas` for exact traversal orientation and area representation outcomes;
- `ExactGridView` plus its validated `BinaryGridSpec`.

No stage may mutate the source mesh, suppress raw events, reclassify ownership, close a path, weld
coordinates, or reverse upstream buffers. Derived final boundary views may select forward or reverse
iteration without altering the borrowed path.

## 4. Historical M3.5-A design basis

TD-026 and [M3-REGION-HIERARCHY-CONTRACT.md](M3-REGION-HIERARCHY-CONTRACT.md) are authoritative
for the implemented strict hierarchy. This section remains as the pre-implementation design record.

### 4.1 Input domain

The first region assembler accepts only a complete ExactOnly grid view and its exact matching planar
inspection and area report. It has no default grid. All graph vertices with agreed coordinates must
already have exact in-range integer outcomes under TD-025.

A path is initially eligible only when:

1. it is a M3.3 closed-cycle candidate;
2. M3.4-A classifies it as `SimpleCycle`;
3. M3.4-B provides a nonzero exact orientation outcome;
4. every referenced vertex has an exact grid coordinate;
5. no retained cross-path contact involves it.

Open chains, ambiguous trails, non-simple cycles, coordinate conflicts, off-grid/range failures,
and contact-involved cycles cannot become region boundaries. They receive one stable disposition;
they are not deleted.

### 4.2 Zero-dimensional and coplanar context

Raw tangent events remain non-boundary incidence. They do not create a filled point or a zero-area
region, and their counts/provenance remain available. Verified local coplanar patch boundaries may
participate through the existing graph. Verified internal patch edges remain non-boundary outcomes.
Any unresolved complete-only group prevents a claim that the whole layer is fully resolved; it does
not become a boundary through multiplicity or parity.

The result therefore has an explicit completeness state:

- `Complete` when every retained contribution has a reviewed nonmaterial or boundary outcome;
- `GeometricallyUnresolved` when representable ambiguity remains;
- operational `Err` for cancellation, invalid invariants, resource failure, or allocation failure.

`GeometricallyUnresolved` is a successful complete analysis, not a printable-layer certificate.

### 4.3 Exact containment convention

Containment operates on TD-025 integer coordinates. For an unordered pair of eligible cycles:

1. integer AABBs reject disjoint candidates;
2. M3.4-A already guarantees no boundary contact;
3. a canonical vertex of each cycle is tested against the other with a positive-X ray;
4. a half-open Y rule counts an edge only when one endpoint is at or below the query Y and the other
   is strictly above it (or the symmetric directed form);
5. side comparisons use exact signed integer cross products; there is no division and no epsilon.

The TD-025 `|q| <= 2^52` domain permits exact segment orientation products in signed `i128`. Every
arithmetic expression still requires a checked proof and boundary tests; debug overflow behavior is
not a contract.

Because boundary contact is excluded, the probe cannot be on the tested boundary. Each pair must be
classified exactly as disjoint, first-inside-second, or second-inside-first. A contradictory result is
a typed consistency error.

### 4.4 Forest and material rule

Containment among non-contacting simple cycles must be laminar. The depth of a cycle is the number of
strict containing cycles. Its immediate parent is the unique containing cycle at depth minus one.
This derives the forest without comparing rounded area magnitudes.

The first proposed fill policy is named `EvenOddStrict`:

- even-depth boundaries are material outers;
- their direct odd-depth children are holes;
- an even-depth island inside a hole starts another material region;
- separate depth-zero roots are disconnected regions.

The rule is explicit project configuration, not inferred from current traversal orientation or a
commercial default. It is valid only for the strict non-contacting input domain. It does not define
union semantics for crossing or coincident source shells.

Each material outer becomes one region. Regions are ordered by outer path ID; holes are ordered by
path ID. Each boundary retains its source path ID, depth, parent, current exact orientation, material
role, and a `reversed_view` flag needed to expose outer CCW/hole CW iteration. Upstream path order and
storage are never changed.

### 4.5 Proposed owned result

The public result should be owned so one layer can outlive temporary borrowing stages:

```text
RegionLayer
  plane: LayerPlane
  grid: BinaryGridSpec
  vertices: flat exact-grid/world-coordinate records
  boundaries: flat boundary records and vertex ranges
  regions: outer boundary plus hole range
  path_outcomes: one outcome per input path
  diagnostics: stable structured findings
  completeness: Complete | GeometricallyUnresolved
```

Names are provisional. Public Rust items require final documentation before implementation.
Coordinates must retain exact integer identity and the original represented world XY values already
proved equal to that grid point; no inverse decoder or reconstructed rounded coordinate is required.
Indices intended to cross serialization/platform boundaries use fixed-width integers or checked
conversion, not implicit `usize` bytes.

### 4.6 Limits, errors, cancellation, and complexity

Separate limits are required for input paths, cycle vertices, pair classifications, containment
relations, boundaries, regions, diagnostics, and aggregate requested capacity. Counts and bytes use
checked `usize`/`isize` arithmetic and fallible exact reservations.

The implementation uses direct point-in-cycle scans with an explicit ray-edge cap. Its documented
work is O(V + P + R), where V is copied eligible vertex references, P is actual ray-edge tests and
R is retained relationships, plus up to O(C^2) candidate pairs. Pair/ray work exceeding a configured
cap is an operational error, not partial geometry.

Extend TD-014 with entry, input, storage, bounds, pair, forest, output, and final-publication
checkpoints. Cancellation publishes no `RegionLayer`. Geometric ambiguity remains data and is not
replaced by a generic error.

## 5. Constructive polygon gate

**Subsequent status:** TD-030 resolves this gate for exact source-group parity/union under the new
contract. The following text retains the original pre-implementation requirements. Lossy conversion
and physical error budgets remain separate; strict reports still retain their original ambiguity.

The phrase “required 2D polygon operations” does not authorize implicit cleanup or union. M3.5-A
provides exact point-in-cycle, containment, nesting, and canonical boundary views. Before whole-M3
closure the owner must decide whether the agreed corpus also requires construction for crossing
closed shells.

If construction is required, it is M3.5-C and needs a separate approved contract containing:

- source-solid/material semantics for multiple mesh components;
- an explicit fill rule for union versus parity;
- exact proper-intersection construction and split ordering;
- a policy for endpoint touches, collinear overlap, coincidence, and multiplicity;
- proved bounds for every rational comparison and output conversion;
- topology-preserving output policy and provenance for constructed vertices;
- independent fuzz/property oracles and resource/cancellation limits.

TD-025 already proves that a proper intersection can require a reduced 156-bit numerator, so an
`i128`-only constructor is insufficient. The safe default is exact rational arrangement arithmetic
with reviewed fixed-limb bounds, or an explicitly authorized dependency after the full dependency
review. A lossy nearest-grid constructor is prohibited until a positive physical error, minimum-gap,
and topology-change budget is approved.

Until M3.5-C is approved, proper crossings, touches, overlaps, and coincidences remain explicit
unresolved region inputs. That is a documented degeneracy policy, not a repair failure.

## 6. Historical M3.5-B proposal: deterministic layer schedule

Implemented under TD-027; the finalized contract supersedes this design-stage wording.

### 6.1 Explicit configuration

The geometry core has no implicit first layer. A uniform schedule requires:

- finite `first_plane_z_mm`;
- finite strictly positive `layer_height_mm`;
- exactly one terminal rule;
- a caller-supplied maximum layer count.

Terminal rules are:

- `Count(n)`: emit exactly `n` planes, including an explicitly permitted empty schedule;
- `Before(z)`: emit mathematical planes strictly below finite `z`;
- `Through(z)`: emit mathematical planes at or below finite `z`.

No hidden epsilon, half-height offset, mesh-bound clamp, bottom-layer profile, or printer default is
allowed. Variable-height schedules require a later separate contract; no placeholder variant should
imply support.

### 6.2 Numerical rule

Plane `i` is the mathematical dyadic value

`z(i) = first_plane_z_mm + i * layer_height_mm`.

Generation must evaluate that dyadic expression exactly and round once to nearest-even binary64 for
publication. Terminal comparisons use the exact mathematical dyadic values, not repeated floating
addition or an epsilon. Each emitted value must be finite and strictly greater than its predecessor;
otherwise generation fails with a typed range/resolution error.

Before implementation, the fixed-limb capacity for finite binary64 inputs and the accepted layer
index domain must be proved and tested. A platform `usize` conversion cannot define schedule
semantics. Published layer indices are contiguous `u64` values starting at zero.

`LayerPlane` contains only index, Z in millimeters, and the uniform nominal height. Exposure, lift,
transition, burn-in, and machine metadata belong to M5 and link by exact layer index/Z under
SLICE-004.

### 6.3 EXP-03 boundary

EXP-03 is not required to implement this fully explicit schedule. It remains mandatory before Resin
Slicer selects or advertises a product-compatible default first plane, top inclusion rule, bottom
profile, or variable-layer transition behavior. An observation of another program is a test
hypothesis, not the project default.

## 7. Proposed M3.6: owned layer engine and progress

Implemented under TD-029; the finalized layer engine contract supersedes this design-stage wording.
The historical heading is retained for existing research links.

An M3 layer engine combines an immutable mesh/transform, explicit plane schedule, reviewed stage
limits, grid/fill configuration, and a borrowed cancellation predicate. It produces one owned
`RegionLayer` at a time. Intermediate raw/normalized/graph/path/inspection/area/grid values remain
temporary and are converted to the owned final layer without self-referential public structures.

The iterator contract is:

- ascending contiguous plane index;
- one complete atomic layer per successful item;
- fused after the first operational error or cancellation;
- previously yielded layers remain individually complete, but the stream is not a complete job
  until normal exhaustion;
- source geometry and transforms remain unchanged.

Progress is separate from cancellation. A synchronous observer receives monotonic events with a
named `M3LayerGeneration` stage, completed-layer count, total layer count when known, and current
layer index. Events are advisory observations and cannot alter geometry. A callback panic is outside
the library contract unless an explicit panic-catching policy is later approved. No thread, async
runtime, or dependency is required.

An optional atomic collector may return a complete owned vector only within explicit layer/output
byte caps. Cancellation or any error returns no successful complete collection. M5 remains
responsible for atomic file/job publication.

## 8. Determinism

For identical source bytes, transform, schedule, grid, policies, limits, and toolchain-supported
platform semantics:

- planes, paths, outcomes, containment, regions, holes, and diagnostics have fixed documented order;
- hash-map iteration cannot affect public order;
- parallel execution cannot be introduced until it preserves those results and cancellation rules;
- test encodings use fixed-width little-endian integers and `f64::to_bits`, not `Debug` output;
- face-order permutation tests compare geometric/provenance-equivalent results under documented ID
  remapping, not impossible source-ID equality.

## 9. Mandatory tests

### 9.1 Region unit and integration tests

- one square, two disconnected squares, nested shell/hole, and shell/hole/island;
- three or more nesting depths and opposite input traversal directions;
- exact-grid coordinates at positive/negative domain boundaries;
- horizontal ray vertices, half-open Y cases, and near-boundary adjacent integer points;
- open chain, ambiguous trail, self-contact, zero-area inconsistency, tangent-only layer;
- cross-path proper crossing, endpoint touch, positive overlap, and coincidence retained unresolved;
- verified coplanar patch boundary/internal ownership and unresolved coplanar group;
- source order/face permutation, repeated execution, limit boundaries, allocation failure, and every
  cancellation checkpoint;
- original mesh, raw events, normalization, graph, and paths byte/identity preservation.

Independent oracles use small integer/rational fixtures and exhaustive small-grid enumeration. The
production containment implementation cannot be its sole oracle.

### 9.2 Schedule tests

- exact count, empty count, negative/zero/positive first plane, and finite validation;
- `Before` versus `Through` at exact and adjacent binary64 boundaries;
- no accumulated drift over large indices;
- rounding ties, subnormal height, overflow, duplicate rounded planes, and cap precedence;
- identical plane bits on stable Linux and native Windows;
- cancellation before work, during layer stages, and at final publication;
- monotonic progress with no false complete event after failure.

### 9.3 Property/fuzz tests

Add bounded targets for path/inspection-to-region assembly and schedule generation. Properties include
forest acyclicity, one disposition per path, parent-depth consistency, role parity, boundary vertex
conservation, deterministic reruns, source immutability, no panic, and bounded allocation. Any future
constructor requires its own intersection/arrangement fuzz target.

### 9.4 Performance and memory

Measure, do not assume, representative clean cycles, deep nesting, many disconnected regions,
pair-cap boundaries, cube/cylinder layer sequences, and a large owned mesh. Record wall time,
requested capacities, and peak RSS separately. Establish a baseline before selecting acceleration;
large-input acceptance needs explicit preliminary budgets approved from measured data.

## 10. Documentation and platform gates

Each implemented increment must update architecture, technology decisions, roadmap, public API docs,
test strategy, examples, and known limitations. Standard stable gates, warnings-denied rustdoc,
focused Release tests, fuzz replay, `git diff --check`, and native Windows/Linux validation are
mandatory. WSL results do not imply native Windows acceptance.

## 11. Decision gates before implementation

The owner must approve these choices separately:

1. **Approved and implemented:** M3.5-A `EvenOddStrict` over exact non-contacting cycles and its
   global unresolved-context policy (TD-026).
2. **Approved and implemented:** Exact dyadic scheduling with explicit `Count`/`Before`/`Through`
   and no default first plane (TD-027; owner request 2026-10-17).
3. **Approved and implemented:** Explicit source-material exact parity/union (TD-030), with contact
   provenance, rational arithmetic and bounded cancellable arrangement work.
4. Preliminary M3 performance/memory budgets after baseline measurement.

Approval of one gate does not authorize later increments, dependencies, lossy conversion, raster
sampling, printer defaults, or commercial behavior cloning.

