# M3.4-C Contract: ExactOnly Integer Coordinates and the Retained Construction Gate

**Subsequent status (2026-10-17):** TD-030 implements exact constructive composition within this
integer domain under [M3-CONSTRUCTIVE-MATERIAL-CONTRACT.md](M3-CONSTRUCTIVE-MATERIAL-CONTRACT.md).
The original authorization/gate wording below is historical. Positive-error conversion, physical
scale defaults and topology-changing snapping remain unapproved/unimplemented.

Date: 2026-10-16. Baseline: `main`, `1c4f52eac1af3cf4ba5fb9632f2995015c38c04b`.
**Status:** On 2026-10-16 the owner approved the binary-grid/ExactOnly direction/domain and design
finalization, then explicitly authorized implementation after committing the contract. Commit
`75acecd` records that design before code. Sections 8–11 are now implemented; scoped validation
is recorded separately below and in the test strategy. A later owner request authorizes committing
the validated implementation. No positive loss allowance or polygon constructor is authorized.

Trace: ROADMAP M3; GEO-001, SLICE-002/003, APP-001; TD-005/014/017/019/022/023/024/025.
The comparative research supplies bounded static evidence,
independent proofs/models and unresolved decisions, not an implementation source.

## 1. Required decision versus recommendations

TD-023/024's full lossy-conversion/topology-change/all-intermediate construction gate stays **OPEN**.
TD-025 now fixes the zero-displacement first-view direction/domain, with its detailed contract below
implemented at the limited scope below, subject to its actual validation record. A universal numeric
scale is not mandated:
the existing design explicitly compares unchanged represented f64, a fixed mm grid and a recorded
per-job grid.
A configuration type alone does not settle that gate.

No approved positive polygon quantization allowance, universal XY envelope, minimum feature/gap
policy or construction algorithm exists. Do not turn RP-001, display precision, pixel pitch or
commercial constants into those decisions. Photon P1 observations remain Lychee
223.642 x 126.48 x 230 mm versus CHITUBOX 223.642 x 125.976 x 230 mm; Lychee screen X=223.6 mm
is separate. The 0.504 mm depth discrepancy is unresolved, not a source-domain constraint.

**Approved first-view direction:** use explicit job-level metadata rather than one hidden universal
scale, with an **ExactOnly integer coordinate view**: accept a point only when the recorded
grid represents the original binary64 value exactly and its integer coordinate fits the approved
domain. Reject off-grid/range input without coarsening, clipping, recentering or repair. This is a
limited useful domain, not a promise to accept every finite mesh or arbitrary M3.1 intersection.
It selects no positive physical error budget. A later BoundedNearest policy requires a separate
owner-approved error/topology contract; it is analyzed below, not authorized alongside ExactOnly.

No constructor, intersection point, contour rewinding, containment/hole assignment, Boolean region,
material total, raster grid, printer setting, layer schedule or GUI is part of this first candidate.
EXP-03 final placement and EXP-04 compatibility remain open; this does not close M1 or M3.

## 2. Representation, origin, parity and input ownership

An exact uniform world-XY grid has origin o, positive step h in mm and signed integer q:

```text
r = o + h*q                    exact represented grid point, not an f64 evaluation
q = RN_even((x-o)/h)            candidate nearest mapping of represented finite x
ExactOnly accepts iff r = x    per axis, by exact arithmetic
```

Version-1 metadata specifies a **binary step h=2^k**, explicit finite binary64 origin
per axis interpreted exactly, named world XY/mm frame, ExactOnly policy, integer bound,
parameter/count domain and immutable source associations. No automatic bbox-origin/scale selection
is included. Origin is conversion metadata, not movement of the model or a change of world origin.
Signed zeros are equal numerically; preserve original source bits and define canonical metadata
zero separately. No serialization format is introduced. The nearest-even mapping above describes
the researched lossy alternative; ExactOnly performs no rounding and rejects every nonzero residual.

Origin shifts by an odd integer number of grid cells can change physical nearest-even choices at
ties even though the grid location set is identical. Example: x=1/2,h=1 rounds to r=0 with o=0,
but to r=1 with o=1. Metadata must retain the exact origin/parity convention. Comparing or combining
q values from different grids is forbidden without a separately explicit exact transform/regrid.
Even nested grids do not make repeated rounding equivalent: 5/8 -> 1/2 -> 0, but direct unit-grid
rounding gives 1. Recompute from the immutable source, not an already quantized intermediate.

The first view must borrow `PlanarInspection`, not a required area report; retain graph/path/raw
provenance. Emit one outcome per original graph feature in feature-ID order. An agreed point may
be converted; a conflicting feature remains explicitly unresolved with no representative chosen.
Complete-only/isolated context stays accessible. Equal q values never weld source features.
The successful value is a complete **coordinate view**, not a `GridPolygon`, valid material
or a partial geometry export. All operational/off-grid/range errors publish no successful view.
Source-unresolved outcomes are complete retained context, not permission for later acceptance.

## 3. Exact forward conversion, not a floating cast

Let eta=2^-1074. Decode finite binary64 x=X*eta and o=O*eta: exponent field E=0 gives magnitude
F; otherwise `(2^52+F)*2^(E-1)`. Both zero signs have integer zero; reject E=2047.
Compute exact signed D=X-O, not rounded f64 subtraction. |D|<2^2099.

For the researched nearest alternative at h=2^k, RN_even acts on D*2^(-1074-k). For a right shift
t>0, divide unsigned |D| into quotient a and remainder z; increment iff z exceeds half, or equals
half with odd a; restore sign.
For ExactOnly require the discarded remainder to be zero. Check the exact rounded quotient against
the coordinate domain before narrowing. For B>=0 the nearest bound at |scaled|=B+1/2 accepts the
tie only if B is even; negative cases are symmetric. ExactOnly additionally requires zero residual.
Guard metadata/exponent/bit lengths before shifts; do not allocate an unbounded integer from k.

Neither `(x-o)/h`, `round(x*scale)`, `round_ties_even()` after a rounded expression nor `as i64`
defines this exact conversion. Rust casts truncate/saturate and map NaN to zero; they are not
range validation. Two independent counterexamples:

- Represented x=0.15, exact decimal h=1/10: exact nearest q=1, but floating x*10 becomes 1.5
  and its nearest-even integer is 2.
- x=3,o=2^-53,h=2: exact nearest q=1, but floating x-o rounds to 3 and subsequent rounding gives 2.

An exact decimal step is a legitimate alternative, but requires bounded rational metadata and
quotient/remainder arithmetic with powers of five; a decimal-looking f64 is not an exact decimal.
Do not add that arithmetic merely to reserve a future option.

## 4. Storage is not the arithmetic domain

For |q|<=B, differences are <=2B, each difference product <=4B^2 and the elementary orientation
bound is <=8B^2. The sharper final determinant bound is 4B^2 (maximum triangle in the coordinate
square); using it requires its correlated evaluation proof, not an unqualified independent-product
argument. Promote before subtracting; guard without `abs(MIN)` or overflow inside the bound check.

| Full coordinate storage | Signed bits needed for a full-domain determinant | Consequence |
| --- | ---: | --- |
| i32 | 65 | i64 is not universally sufficient; i128 predicates can suffice |
| i64 | 129 | i128 does not universally suffice |
| i128 | 257 | Larger coordinates alone do not solve arithmetic |

For N closing edges, every product-by-product twice-area partial state is <=2*N*B^2. A signed
W-bit accumulator needs that bound <=2^(W-1)-1, not merely a bounded final simple-polygon area.
For example B=2^62-1 fits the elementary i128 orientation bound but its conservative i128 area
bound permits only N<=4. Checked count limits or a separately proved wider accumulator are needed.

**Owner-approved first-view coordinate domain:** i64 storage, |q|<=2^52. The accompanying
per-cycle N<=3,000,000 illustration bounds i128 orientation and integer area: both 8B^2 and 2NB^2
are <i128::MAX. This is arithmetic headroom,
not a chosen mm scale, world extent, physical accuracy or M3 throughput promise. Callers cannot
raise the coordinate domain without a new bound/proof. The first view computes neither integer
predicates nor area and issues no such certificate; the N bound belongs to that later operation's
schedule, not a new path scan/rejection in this coordinate pass. Existing TD-024 APIs are unchanged.
The elementary every-state area bound permits at most 4,194,303 closing terms for this B; the
illustrative 3,000,000 cap is lower. Cross-cycle totals, indexing, allocation and total work still
need independent limits; this does not authorize an unbounded material-area accumulator.

For this domain, binary k in [-1074,1023] spans finite nonzero binary64 power-of-two steps.
Exact forward difference magnitude fits 66 u32 limbs (2112 bits). If an inverse is later needed,
R=O+q*2^(1074+k) has |R|<2^2150 in eta units, fitting 68 limbs (2176 bits), plus sign. These are
separate private bounds, not reuse of the old 132/134-limb proofs or a general arithmetic API.
The approved parameter domain still requires implementation tests. Section 8 specifies the smaller
ExactOnly forward helper; the inverse analysis does not authorize an inverse implementation.
This document adds no source code or dependency.

## 5. Constructions remain a separate gate

For a,b,c,d bounded by B, intersection uses r=b-a,s=d-c and
`a + r*((c-a) cross s)/(r cross s)`. Elementary determinant bounds are quadratic; straightforward
coordinate numerators need <=24B^3 and rational comparisons can require degree-five products.
An exact segment intersection need not be an integer point. Final coordinates inside endpoint
bounds do not bound their rational construction numerators/intermediate products.

Example: `(0,0)->(B,B-1)` and `(0,B)->(B-1,0)` intersect at
`x=B^2*(B-1)/(B^2+(B-1)^2)`, already reduced. At B=2^62-1 its numerator needs 186 bits and its
denominator 125. Safe i128 orientation therefore does not make even this single exact construction
fit i128. Later Boolean inputs may already be rational, with further growth.
Even the narrower worked B=2^52 candidate produces a reduced 156-bit numerator and 105-bit
denominator in this family. The first-view predicate/area bounds do not certify construction.

Do not declare the complete construction/region gate satisfied by the candidate above. Before a
constructor is approved, fix its operation sequence, rational normalization/comparison, input and
operation-depth domain, output rounding, topology policy, every-intermediate bound and resource
failure semantics. Narrower domains, fixed bounded limbs, reviewed arbitrary precision or a
different proven design are alternatives, not selected dependencies or an implicit i256 commitment.

## 6. Inverse representation and physical error

The authoritative inverse is exact r=o+h*q. If a future caller needs f64 y, require one specified
nearest-even rounding of exact r. Do not assume `f64(q)*h+o` is equivalent. With o=2^100,h=2^40,
q=-2^60+1 (outside the worked candidate), exact r=2^40 while naive reconstruction gives zero.
Within |q|<=2^52, q is exactly f64 and binary scaling is exact when finite, but can overflow before
cancellation: k=1023,q=2,o=-f64::MAX has exact inverse 2^971 while the floating product is infinity.
The bounded exact-expression strategy avoids that intermediate overflow. It is sufficient, not
the only possible independently proved decoder. Within the candidate k domain every inverse is a
multiple of eta, so a nonzero inverse cannot round to zero. A finite scalar alone is no proof.
Even a bounded q can produce nonfinite inverse or decoded coordinate collisions at a large origin.
Report/reject those losses; never return infinity coordinates or merge integer points by a display
approximation. ExactOnly can reuse its original agreed f64 point after proving equality; no new
f64 decoder is needed in the first view.

For a future nearest conversion, exact per-axis quantization error is |x-r|<=h/2. If y is consumed,
also bound e_decode=|y-r| and certify |y-x|<=e_quant+e_decode. Store exact dyadic/rational bounds or
outward-rounded certificates; a bound smaller than eta must not silently become zero. Upstream
STL/transform/M3.1 construction error is separate. This is not manufacturing accuracy.

For R=max|x-o| over an entire proposed envelope and B>0, h>=R/B is a sufficient range condition;
it is not necessary for individual finite grid-aligned points. A worst-case per-axis allowance a>0
also needs h<=2a (Euclidean allowance p instead gives h<=sqrt(2)*p). Step admissibility, inverse
error and upstream budgets further constrain feasibility. Check exact inequalities/actual quotient
ties, not rounded log/sqrt estimates. An impossible range/resolution combination must fail rather
than silently coarsen or reposition. No numeric a,p,R or positive error allowance is selected here.

## 7. Topology and policy alternatives

ExactOnly preserves the represented positions of every converted agreed feature, hence introduces
no new geometric change there. It does not resolve original ambiguity, complete-only context,
material/nesting or ideal pre-rounding source geometry. Off-grid geometry is rejected, not repaired.

If BoundedNearest is later approved, point displacement is <=delta=h/sqrt(2). Corresponding segments
have Hausdorff distance <=delta by convex interpolation. Those metric bounds do **not** establish
topology. Unit-grid examples, scalable to any h>0:

- `(0,0),(1/4,0),(0,1/4)` collapses completely.
- `(0,0),(2,7/16),(3,9/16)` reverses determinant from -3/16 to +2 without vertex collapse.
- A square with bounds 1/4 and 15/4 inside a square with bounds 0 and 4 becomes coincident with it.
- The simple quadrilateral `(7/8,1),(-5/8,-11/8),(3/2,13/8),(1/2,15/8)` gains forbidden contacts.

A source segment clearance >2delta certifies continued separation of those particular nonincident
segments. It does not solve incident folding, containment or whole-polygon isotopy. Matching
before/after local simplicity and all pair-contact categories alone does not certify unchanged
nesting/material; contact answers do not encode containment or intermediate motion. Do not add a
continuous-motion solver, containment classifier or repair under a conversion task by implication.

| Proposed policy | Allowed success / limitation |
| --- | --- |
| ExactOnly, recommended first | Complete coordinate view only when every required agreed position is exactly representable and in range; original unresolved context stays explicit |
| BoundedNearest, not approved | Requires explicit error/domain policy, changed-coordinate/collapse/new-and-lost-contact/sign diagnostics and a separate downstream acceptance contract; never a preservation claim by default |
| Certified topology preservation | Needs an independently proved adequate certificate, not h alone; scope/algorithm/owner assumptions unresolved |
| Accept or repair topology changes | Not authorized; requires separate explicit material/change policy and provenance/history |

No old SimpleCycle, orientation or future hole certificate may be transferred to moved coordinates
without its required new proof. ExactOnly preserves the represented geometry, not global material
acceptance. A later source-versus-grid audit must budget all tested pairs/findings and record lost
as well as new contacts, without silently truncating. It is not part of the first ExactOnly pass.

## 8. ExactOnly implementation contract: metadata, API and arithmetic

The names below specify the implemented first view. It adds only a headless
`slicing::grid` module; no CLI command, serialization, dependency or general big-integer API.
Version 1 has one uniform XY step, one explicit finite origin per axis and only ExactOnly behavior.
No default origin, default k, automatic bounds fit, rounding-policy enum or future placeholder.

### Validated metadata

`BinaryGridSpec::try_new(origin_xy_mm: [f64; 2], step_exponent: i16)` returns
`Result<BinaryGridSpec, GridSpecError>`. Private fields prevent bypassing validation. Check origin X,
then Y for finiteness, then -1074<=k<=1023. Normalize only metadata signed zeros to +0; preserve
the original immutable input. Getters are `origin_xy_mm() -> [f64; 2]`,
`step_exponent() -> i16`, `version() -> u32` (1) and `max_abs_coordinate() -> i64` (2^52).
The last bound is fixed, inclusive and not caller-configurable. No f64 step/inverse getter.

`GridAxis::{X,Y}` defines error/traversal order. `GridSpecError` distinguishes
`NonFiniteOrigin { axis }` and `StepExponentOutOfRange { exponent: i16 }`, with English Display
and standard Error. This O(1), allocation-free constructor is not cancellable. It is a separate
call: a cancellation request at conversion cannot retroactively override an earlier constructor
error. Logical metadata equality uses version, canonical origin values/bits, k and fixed policy/
domain, not an approximate comparison. Equal index pairs from different specs are not comparable
world coordinates. No combine/regrid operation is exposed, so no fictitious mixed-grid guard API.
`BinaryGridSpec` is Clone/Copy/Debug/PartialEq, not Default. GridAxis, GridVertexOutcome, GridLimits
and GridSummary are Clone/Copy/Debug/PartialEq/Eq; GridLimits additionally implements Default with
the stated caps. ExactGridView is Debug/PartialEq, not Clone or publicly constructible. Error types
are Debug/PartialEq/Eq; GridSpecError and GridError are non-exhaustive.

### Borrowed result and public entry points

Signatures, with the same transitive lifetime convention as `SectionAreas`:

```text
map_exact_grid_vertices<'i, 'p, 'g, 'n, 'r>(
    &'i PlanarInspection<'p, 'g, 'n, 'r>, BinaryGridSpec, GridLimits
) -> Result<ExactGridView<'i, 'p, 'g, 'n, 'r>, GridError>

map_exact_grid_vertices_with_cancellation<'i, 'p, 'g, 'n, 'r>(
    &'i PlanarInspection<'p, 'g, 'n, 'r>, BinaryGridSpec, GridLimits, &mut Cancellation<'_>
) -> Result<ExactGridView<'i, 'p, 'g, 'n, 'r>, GridError>
```

The ordinary wrapper supplies `Cancellation::never()`. `ExactGridView` has private construction
and borrows the inspection, not an area report. Accessors are `inspection()`, copied `spec()`,
`outcomes() -> &[GridVertexOutcome]`, copied `vertex(id) -> Option<GridVertexOutcome>` and copied
`summary() -> GridSummary`. An out-of-range lookup returns None, not a panic or fabricated result.

One outcome for **every** graph vertex/normalized feature, in that original ID order:

| `GridVertexOutcome` | Exact meaning |
| --- | --- |
| `Exact { xy: [i64; 2] }` | Both agreed represented XY coordinates exactly equal o+q*2^k, with each abs(q)<=2^52 |
| `UnresolvedCoordinates` | Upstream CoordinateAgreement::Conflict; all observations remain borrowed and no representative, index pair or success certificate is chosen |

Include isolated, complete-only/internal and unresolved-edge-adjacent agreed vertices, not merely
eligible path vertices. An off-grid agreed point in such context still rejects the whole view;
there is no silent subset policy. A conflict is a complete retained outcome, not an off-grid error
or an invented common point. Its observations are not individually quantized or expanded. A view
may succeed with all vertices unresolved, but cannot then be consumed as all-exact geometry.
Even if conflicting observations happened to share XY, retain the upstream full-coordinate conflict.
Equal XY pairs never connect distinct source IDs. Resolve source identity and original positions
through `inspection().paths().graph().normalized()`; no duplicate source/provenance buffer.
Z stays the borrowed explicit plane in mm; no Z quantization or layer sampling occurs.

`GridSummary` fields are `vertices`, `exact_vertices`, `unresolved_vertices`, `axis_conversions`
and `requested_payload_bytes`, all usize. On success V=C+U, A=2*C, outcomes.len()=V. Input metadata
and all context remain accessible through the borrowed inspection; do not recalculate contact or
material summaries. Plain copied outcomes do not constitute a detached validity certificate.
Determinism means identical logical fields/IDs, not struct padding bytes or a new wire format.

### Minimal exact forward helper

Use a narrowly private sign/magnitude helper with two reusable `[u32; 66]` arrays. Decode x and o
exactly into eta units as in Section 3; subtraction/addition overwrites the x array with D=X-O.
Canonical zero has positive sign. At most 2099 magnitude bits suffice, below 2112 available.
Split the <=53-bit mantissa into u32 fragments before shifted insertion; do not shift it by up
to 31 inside a u64 and assume it fits. Carry/borrow operations use proved u64 fragments.
No heap arithmetic, dependency or change to the existing exact predicate/area helpers.

Let t=1074+k, so 0<=t<=2097. Test all bits of |D| below t: if any are set, return OffGrid. This
comes **before** quotient range classification, even if both conditions could fail. Otherwise the
quotient is exact. For nonzero D, inspect bit_length(|D|)-t without unchecked subtraction: lengths
<=52 fit, length 53 fits only at exactly 2^52, larger lengths fail. Do not narrow/truncate before
this test. Extract at most 53 quotient bits into u64, validate <=2^52, then safely apply sign into
i64. D=0 gives q=0 for every valid k. Negative boundaries are symmetric; no abs(i64::MIN).
Each limb pass visits at most 66 words and extraction at most 53 bits; guard all shifts/indices.

Both exact axes must succeed before pushing their one outcome. Accepted coordinate displacement
and conversion-rounding error are exactly zero relative to the agreed represented inputs. Every
nonzero residual, including either nearest-even midpoint, is rejected, not rounded. There is no
need to implement nearest-even rounding, integer orientation/area, inverse conversion, a 68-limb
inverse helper or source-versus-grid pair audit in this first pass. Section 3's lossy formulas and
Sections 4–7's construction/inverse analysis remain future design constraints, not additional code.

## 9. Resource limits, storage and deterministic preflight

`GridLimits` has public usize fields; defaults are safety policy, not measured job acceptance:

| Field | Default | Charged work/data |
| --- | ---: | --- |
| `max_vertices` | 3,000,000 | Every graph vertex, including unresolved and isolated context |
| `max_axis_conversions` | 6,000,000 | Two planned exact conversions per agreed feature, including complete-only points |
| `max_payload_bytes` | 512 MiB | One requested flat outcome vector, V*size_of::<GridVertexOutcome>() |

`GridResource::{Vertices,AxisConversions,PayloadBytes}` identifies cap failures. Zero limits are
valid: empty input can succeed with all zero; nonempty all-conflict input may succeed with zero
axis budget but still consumes vertices/bytes. Raising resource caps never changes k or |q| bounds.
No per-path count/area cap is imposed: this operation scans no path walks and assesses no cycles.
The N<=3,000,000 area illustration cannot be advertised as a certificate issued by this view.

Preflight order is fixed after GridStart:

1. Read graph vertex count V; check max_vertices before scanning metadata or allocating.
2. Require normalized feature count equal V; require a finite borrowed plane, including empty input.
3. Scan graph IDs ascending with GridInput polls. Require vertex.feature_group==ID, strictly
   ascending SourceFeature identities and a nonempty feature observation range within feature_order.
   For Agreed points require all XYZ finite and z equal to the borrowed plane; count C with checked
   arithmetic. Conflicts retain their upstream tag without scanning alternate observations.
4. Compute A=2*C with checked arithmetic and check max_axis_conversions. Compute V*record-size with
   checked usize arithmetic and <=isize::MAX; check max_payload_bytes. No heap request precedes these.
5. For V>0, one `Vec::new()` plus fallible `try_reserve_exact(V)`, with GridStorage checks on both
   sides. Empty input has no reservation/storage checks. Reserve once, then push at most V records.
6. Convert/emit in graph ID order, X then Y. The one-outcome-per-vertex loop and checked counters
   must yield the stated V=C+U/A=2*C identities; test these publication invariants. Poll GridComplete before
   publishing the complete borrowed view. No successful partial vector escapes any error.

These defensive checks cover the metadata actually read. They do not revalidate every observation,
edge, path walk, diagnostic or privately built upstream certificate. Do not turn this into an
untrusted deserializer or add an O(E^2) geometry pass. Only constant-time range metadata is read;
referenced observation arrays and complete adjacency stay borrowed. Private malformed-input tests
exercise the checks without exposing a public forgeable inspection constructor.
Test a private preflight helper on borrowed metadata slices (cloned from valid graph/feature records
and deliberately altered in the test), rather than adding a constructor for forged PlanarInspection
values or changing upstream visibility. Production entry points still accept only the real report.

Requested bytes are exactly the outcome reservation's requested payload; no additional heap
scratch is allowed. The two arithmetic arrays total 528 bytes of fixed magnitude storage plus
signs/scalars/compiler stack overhead, excluded from that payload budget. The inline spec/summary,
borrowed upstream storage, allocator metadata/over-allocation and cleanup are also excluded; no
hard stack or process-RSS guarantee. Record layout/size is determined by the actual target build,
not hard-coded as an ABI. Counters, sizes and byte products use checked usize/isize arithmetic.
Cost is O(V+66*A), one O(V) output and fixed scratch. No sort, hash table, pair scan, per-observation
expansion, path/edge copy, inverse decoder or new threading is part of the implementation.

## 10. Cancellation and structured errors

Add only these TD-014 checkpoint variants during implementation; existing names/cadence stay intact:

| Checkpoint | Contract |
| --- | --- |
| GridStart | Once on entry, before counts/caps/input work |
| GridInput | Preflight graph-feature IDs 0,256,512,... |
| GridStorage | Immediately before and after the one nonempty fallible reservation |
| GridRecords | Emission IDs 0,256,512,..., including conflict-only stretches |
| GridCoordinates | Before axis conversions 0,256,512,... using one global axis counter, X then Y, not reset per vertex |
| GridComplete | Once before publication, including empty and all-conflict input |

One fixed per-axis arithmetic operation, allocation and cleanup are indivisible. This is bounded
cooperative work, not time-to-cancel, progress, asynchronous work or a thread guarantee. A prompt
non-panicking predicate runs on the caller's thread. Exact checkpoint counts are testable on this
build, not an API promise across releases. A request observed at a checkpoint wins there; an error
before the next checkpoint stays that error. Requests after GridComplete need not prevent success.

`GridInputIssue` is a public Clone/Copy/Debug/PartialEq/Eq enum containing the six consistency
reasons listed below. `GridError` is structured, non-exhaustive, with English Display and standard Error:

| Variant | Meaning/context |
| --- | --- |
| `Cancelled(Cancelled)` | Sourced TD-014 cancellation, with the actual checkpoint |
| `LimitExceeded { resource: GridResource, limit: usize }` | First configured cap failure in the stated preflight order |
| `SizeOverflow` | Checked count/size/isize arithmetic failure, not a geometry or memory-limit finding |
| `Allocation` | Fallible outcome reservation failed |
| `InvalidInspection { vertex: Option<usize>, reason: GridInputIssue }` | Inconsistent read metadata; reasons FeatureCount, Plane, Association, FeatureOrder, ObservationRange or Coordinates |
| `OffGrid { vertex: usize, axis: GridAxis }` | Exact nonzero residual; no nearest result is returned |
| `CoordinateOutOfRange { vertex: usize, axis: GridAxis }` | Exact integer quotient has abs(q)>2^52 |
| `ArithmeticOverflow { vertex: usize, axis: GridAxis }` | Defensive bounded-helper capacity failure; unreachable for the proved domain if correctly implemented, never wrap or saturate |

GridSpecError is separate because conversion receives an already validated private-field spec.
The first operational failure in phase/vertex/axis order wins. Metadata/cap errors precede the
reservation; therefore allocation can precede a later OffGrid/CoordinateOutOfRange diagnosis.
Within an axis, bounded arithmetic failure precedes residual classification, which precedes range.
No error is a warning-only success or carries a partially successful view. Source feature/plane
and raw observation provenance remain resolvable from the caller's unchanged inspection and IDs.
Error::source exposes the underlying Cancelled; there is no invented subordinate error cause.

## 11. Deterministic implementation acceptance matrix

These are the implementation acceptance obligations. Actual executed coverage/results are recorded
separately in the test strategy; the matrix alone is not proof of passing. Bounded deterministic
enumeration supplies property coverage without a new property-test dependency.

| Family | Required independent assertions |
| --- | --- |
| Spec | Every valid k endpoint/domain; nonfinite X/Y, invalid exponents, constructor error order, signed-zero metadata, explicit origin/no defaults and getters |
| Exact arithmetic | Decode all exponent fields, smallest/largest finite values, signed zero/subnormals; independent bit/Fraction or bounded integer oracles, carry/borrow/sign/cancellation and shifted mantissa fragments at every alignment residue |
| Accepted quotient | Positive/negative/zero q, +/-2^52 exactly, neighboring in/out values, maximum opposite finite origin subtraction and narrow cancellation; check exact equality, not floating reconstruction |
| Off-grid precedence | Nonzero residuals on both sides and signed midpoint/neighbor cases; when residual and range both fail return OffGrid; X wins before Y and first graph ID wins |
| No rounded subtraction | x=3,o=2^-53,h=2 must be OffGrid, not a rounded integer; extreme differences must classify without f64 subtraction overflow |
| Public pipeline | Actual mesh -> section -> normalization -> graph -> paths -> inspection -> view for aligned analytic cases and independently predicted off-grid sections; no production helper as expected-value oracle |
| Context/provenance | All agreed complete-only/internal/isolated points are checked; their off-grid/range errors are not skipped; conflicts yield one unresolved outcome with every observation/context borrowed; equal q never merges distinct IDs |
| Topology boundary | Open, ambiguous, crossing, collapsed, coincident and nested context remains unchanged; conversion does not change local embedding/area eligibility, assign roles or accept material |
| Metadata and determinism | Different origins/scales retain distinct specs; lossless aligned origin reindexing and source permutations use analytically mapped IDs, not false numeric-ID invariance; repeat/never-cancelled wrapper agree; no combine/regrid/serialization API |
| Limits/storage | Exact/one-below independent vertex, axis and payload caps; all-conflict/empty zero cases, caller-raised caps, checked usize/isize overflow, no reservation before failing preflight; do not fake actual allocator exhaustion |
| Cancellation | Exact documented trace with deterministic predicates at every reachable checkpoint; global axis and conflict-loop cadence beyond 256, both reservation sides, empty/final checks, error precedence, retry/source immutability; no sleeps or latency assertions |
| Defensive input | Private malformed feature count/order/association/ranges, plane and finite coordinates return contextual typed errors; bounded-helper capacity tests do not forge a valid public certificate |
| Regression/platform | All stable quality gates, focused Debug/Release, working rustdoc example and genuine native Windows builds/tests; compare logical outcomes and target-sized payload formulas, never padding bytes |

Tests preserve the canonical STL generator/files and the M3.1 kernel, existing exact signs, area
rounder, upstream APIs and their cancellation cadence. No integer orientation/area API, inverse
rounder, mixed-grid operation, fuzz campaign or benchmark is added under this first-view increment.
Existing owned analytic fixtures may be used without changing canonical evidence. Later constructor,
region, fuzz and measured performance/memory acceptance remains under the original roadmap.

Implementation files: `crates/karasu-core/src/slicing/grid.rs`, narrowly private `grid/tests.rs`,
`crates/karasu-core/tests/exact_grid.rs`, additive module/checkpoint declarations in
`slicing.rs`/`cancellation.rs`, and relevant English architecture/test/roadmap/TD documentation.
No existing kernel, predicate, area arithmetic, generator or canonical fixture was changed.

## 12. Executed research checks, limitations and implementation approval gate

The independent exact model passes, exit 0: 134,004 exhaustive small-box orientation triples,
185,500 partial area states, 14,291 finite binary64 decodings/conversion-oracle cases, 33,475
rational rounding cases, 30,562 one-round inverse cases and 51,000 segment-error samples, plus
explicit range/construction/parity/double-rounding/topology counterexamples. These are finite models,
not production conversion tests, platform validation, performance or a complete topology proof.
The supplemental candidate model also passes 73,430 inverse shift/add cases across every proposed
k and 24 exact signed boundary cases, corroborating the separate symbolic limb/range bounds.
At that research checkpoint, stable workspace regression validated unchanged M3.4-B, not a converter.

Independent notes, model, logs and the final report stay outside the repository. That historical checkpoint contained independently authored documentation only; no converter then.

The owner subsequently approved the first-view direction/domain and authorized the design-only
finalization above. Prior model results remain historical evidence; no new reference research is
claimed by contract finalization. Current design verification logs stay outside the repository.
The new independent forward schedule model passes 20,470 decode cases and 90,353 conversions
against a separate Fraction oracle: 15,541 exact, 37,515 off-grid and 37,297 out-of-range model
classifications. Coverage includes 18,572 finite aligned boundary cases and 8,388 exact half-step
cases. Its 135 abstract resource/cadence checks exercise formulas, not a Rust allocator or callback.
These are finite design checks; none is a tested production conversion/cancellation result.

The owner subsequently authorized implementing Sections 8–11 after contract commit `75acecd`.
Implementation validation evidence stays outside the repository.
Research model results above are not substituted for the Rust/native acceptance results.
Scoped Rust acceptance passes Linux/WSL 510/0/2 and native Windows final replay 509/0/2, with 16
new integration tests, 12 private tests and one executable example. Quality gates and focused
Release checks pass; the test strategy preserves the initial Windows CLI host-policy error 4551,
the unchanged isolated/full successful replays and exact snapshot provenance. The owner subsequently
requested committing the validated implementation; no performance/fuzz/material acceptance is implied.

**Retained next gate:** before any construction, define its independently justified algorithm,
rational intermediate growth, output-grid/error/topology policy and resource contract. ExactOnly
does not close that gate. Lossy conversion, material/regions, final layer sampling, EXP-03/04, M1
and remaining M3/later milestones remain OPEN. No next increment is authorized by the commit request.
