# M3.1 Triangle–Horizontal-Plane Contract

**Status:** M3.0-B design accepted for the first M3.1 implementation increment; not implemented.
**Baseline:** `main`, `afc04f77fb169d643e187d0c61d21ea04c47f52a` (M2 closure).
**Decisions:** TD-015 through TD-017. This specification is the normative detailed contract.

## 1. Scope and mathematical object

The local primitive intersects the closed convex hull of three supplied represented points with
the closed horizontal plane `z = h`. All coordinates and `h` are finite binary64 (`f64`) values,
in millimeters and in the same caller-named coordinate space. A source triangle may have affine
dimension zero, one, or two. Input order records source winding; it is not a validity assertion.

M3.1 performs O(1) work with O(1) stack storage, fixed-size arrays, no heap allocation, and no
dependency. It validates its own inputs because existing `geometry::Point3` has public fields and
`Point3::new` does not enforce finiteness. It takes no mesh, transform, printer, tolerance, callback,
progress controller, cancellation controller, resource cap, layer index, or job configuration.
The bounded local work does not warrant cancellation; a later mesh traversal owns its checkpoints.

Excluded: traversal, plane generation/first-layer placement, shared-edge ownership, segment
multiplicity/suppression, mesh-wide deduplication, snapping/welding, contour/region construction,
hole containment, polygon scaling, rasterization, and repair. Passing three transformed M2 points
does not change this boundary. No geometry or metadata is mutated, on success or failure.

## 2. Classification is exact relative to the inputs

For each corner, use the finite represented numbers directly:

| Relation | Side |
| --- | --- |
| `z < h` | `Below` |
| `z == h` | `On` |
| `z > h` | `Above` |

The three relations are exhaustive for finite inputs. The default has **zero classification
tolerance**. Do not calculate `z - h` to choose the side: `MAX - (-MAX)` overflows even though
both operands are finite and their order is unambiguous. Direct comparisons work at every finite
magnitude, including subnormals. `-0.0 == +0.0`, so both are `On` at either represented zero plane.

An exact real value strictly between adjacent representable numbers is not an additional possible
`f64` input. Its conversion first rounds it to one representable `h`, and classification uses that
value. The kernel cannot recover the original real/decimal intent. Likewise, rounding introduced by
M2 transforms precedes this predicate; classification cannot undo it.

**Decision:** defer tolerance-band classification to a separately specified higher-level policy.
A band intentionally changes incidence/topology and would need units, projection rules, and
shared-feature consistency. No tolerance parameter or alternate band mode is added in M3.1.
Machine-rounding bounds in Section 5 are accuracy bounds, not classification tolerances.

## 3. Raw events and independent source context

Events encode source incidence as well as the geometric support. They make no positive-area or
printability assertion. The mutually exclusive variants are:

| Variant | Incidence condition | Payload and meaning |
| --- | --- | --- |
| `None` | All corners strictly on the same side | Empty intersection |
| `TangentPoint` | Exactly one `On`, other two on the same strict side | Exact on-plane corner; point intersection, including degenerate source inputs |
| `Segment` | Both strict sides occur, with zero or one `On` | Two endpoint records; strict-crossing source incidence, even if represented endpoints coincide |
| `CoplanarEdge` | Exactly two `On` | Complete local edge ID and its two exact source-corner endpoint records; may have zero length |
| `CoplanarTriangle` | Three `On` | All three normalized source-corner records; closed convex hull may be a point, line segment, or area |

`CoplanarTriangle` deliberately does not mean `CoplanarFace` or a filled region. A collinear or
repeated-position coplanar input is preserved as the same raw source incidence, never certified as
an ordinary positive-area face. Later consumers must not infer area from the variant name.

### Coincidence is not a separate intersection alternative

`Segment` and `CoplanarEdge` carry `EndpointCoincidence::{Distinct, CoincidentRepresented}` using
exact equality of the returned normalized coordinates. Both endpoint records survive coincidence.
This is not an exact-real rank test: a collinear source may have a point intersection, or distinct
exact intersections may round to one represented point; conversely independent constructions of
the same exact point may differ in their final rounding. The incidence variant does not change.

Thus `CollapsedSegment` is not a competing topological variant. It is the explicit
`CoincidentRepresented` condition on an incidence segment. Do not use a distance epsilon or silently
discard it. A crossing that rounds to a source endpoint still has `Edge` provenance.

### Degeneracy context

Return context separately from the event, including for `None`:

- Three exact equal-position flags in fixed edge order `01, 12, 20`, with signed zero equal.
  Equal coordinates do not erase corner or edge identity.
- `TriangleAreaObservation::{RepeatedPosition, ZeroComputedCross, NonzeroComputedCross,
  UnavailableRange}`. This is an observation, **not an exact affine-rank predicate**.

Compute the observation in canonical coordinate order, not source winding order. If any pair is
exactly equal, report `RepeatedPosition` (mathematically degenerate). Otherwise sort the three
points lexicographically by normalized X/Y/Z; subtract the first from each other point, scale each
edge independently by its largest absolute component, and form the three cross components with
separate products/subtractions in a fixed order. If an edge difference or normalization/cross
intermediate is non-finite, or a distinct edge cannot be normalized, return `UnavailableRange` as
context, not a failed intersection. An all-zero computed cross gives `ZeroComputedCross`; otherwise
`NonzeroComputedCross`. Do not compute a unit normal or area magnitude.

This deliberately follows the *meaning* of M2's computed-degeneracy observations, without calling
its private `mesh::diagnostics::normal`, claiming a certified rank, or changing M2. Subtractive
cancellation, underflow, and prior rounding can invalidate a rank inference from a floating-point cross product;
even a nonzero computed cross is not an exact non-collinearity certificate. Exact general
collinearity is outside this primitive. An unassessed exact rank is an explicit contract limit,
not permission for a downstream positive-area assumption.

| Source shape | M3.1 treatment |
| --- | --- |
| All three positions identical | All three equality flags true; `RepeatedPosition`; `None` or raw `CoplanarTriangle` |
| One repeated pair | Preserve both local corners/edges; `RepeatedPosition`; raw incidence event, possibly coincident endpoints |
| Three distinct collinear points | Preserve raw event and computed-cross observation; do not certify nonzero area or exact rank |
| Vertical collapsed triangle | Side table still applies; a crossing can have coincident endpoints and edge provenance |
| All Z equal but not `h` | `None`, with independent degeneracy context |
| All Z equal to `h` | Raw `CoplanarTriangle`, whether area, segment, or point |
| One `On`, one `Below`, one `Above` | `Segment` with exact corner and strict opposite-edge crossing; preserve coincidence if they agree |

## 4. All 27 ordered classifications

`B/O/A` mean `Below/On/Above`. `C` counts strict-crossing edges. `P` counts source edges whose
intersection with the plane is a singleton, **with multiplicity**; a source `On` corner contributes
on each incident noncoplanar edge. `E` counts entirely coplanar source edges, including collapsed
edges. `On corners` and `On edges` are local identities. These counts are combinatorial incidences,
not counts of distinct geometric positions. The kernel derives only the final bounded event, not
an intermediate dynamically allocated list of edge hits.

| Sides 0/1/2 | C | P | E | On corners | On edges | Raw event | Caveat |
| --- | ---: | ---: | ---: | --- | --- | --- | --- |
| BBB | 0 | 0 | 0 | — | — | None | D |
| BBO | 0 | 2 | 0 | 2 | — | TangentPoint | D |
| BBA | 2 | 2 | 0 | — | — | Segment | S |
| BOB | 0 | 2 | 0 | 1 | — | TangentPoint | D |
| BOO | 0 | 2 | 1 | 1,2 | 12 | CoplanarEdge | E |
| BOA | 1 | 3 | 0 | 1 | — | Segment | S |
| BAB | 2 | 2 | 0 | — | — | Segment | S |
| BAO | 1 | 3 | 0 | 2 | — | Segment | S |
| BAA | 2 | 2 | 0 | — | — | Segment | S |
| OBB | 0 | 2 | 0 | 0 | — | TangentPoint | D |
| OBO | 0 | 2 | 1 | 0,2 | 20 | CoplanarEdge | E |
| OBA | 1 | 3 | 0 | 0 | — | Segment | S |
| OOB | 0 | 2 | 1 | 0,1 | 01 | CoplanarEdge | E |
| OOO | 0 | 0 | 3 | 0,1,2 | 01,12,20 | CoplanarTriangle | T |
| OOA | 0 | 2 | 1 | 0,1 | 01 | CoplanarEdge | E |
| OAB | 1 | 3 | 0 | 0 | — | Segment | S |
| OAO | 0 | 2 | 1 | 0,2 | 20 | CoplanarEdge | E |
| OAA | 0 | 2 | 0 | 0 | — | TangentPoint | D |
| ABB | 2 | 2 | 0 | — | — | Segment | S |
| ABO | 1 | 3 | 0 | 2 | — | Segment | S |
| ABA | 2 | 2 | 0 | — | — | Segment | S |
| AOB | 1 | 3 | 0 | 1 | — | Segment | S |
| AOO | 0 | 2 | 1 | 1,2 | 12 | CoplanarEdge | E |
| AOA | 0 | 2 | 0 | 1 | — | TangentPoint | D |
| AAB | 2 | 2 | 0 | — | — | Segment | S |
| AAO | 0 | 2 | 0 | 2 | — | TangentPoint | D |
| AAA | 0 | 0 | 0 | — | — | None | D |

Caveats: D = retain source context even for a point/empty result; S = construction can fail, or
returned endpoints can coincide (Section 3); E = exact source endpoints may be identical; T = raw
coplanar support has no positive-area guarantee. The 27 cases partition as 2 empty, 6 tangent,
12 segment, 6 coplanar edge, and 1 coplanar triangle. Source degeneracy changes support dimension,
not these source-incidence counts. Classification cannot by itself certify a positive-length
segment: for example, distinct collinear vertices with `BBA` intersect the plane at one exact point.

## 5. Concrete construction algorithm and rejection domain

### Ideal intersection

For a strict-crossing edge, real arithmetic gives
`t = (h - a.z) / (b.z - a.z)` and `p.xy = (1-t)*a.xy + t*b.xy`, with `0 < t < 1`.
Set `p.z = h`. This ideal concerns represented inputs interpreted as real numbers. Most such
intersections are not representable exactly in binary64.

The literal formula is not the implementation contract: numerator or denominator can overflow;
the parameter can underflow or round to an endpoint; `b.x-a.x` can overflow although its convex
interpolant is finite. Subtracting nearly equal rounded transformed values also cannot recover
precision already lost. A finite answer under one association does not imply every formula works.

### Chosen bounded strategy

Use the following fixed operation sequence; every `fl` is a separate round-to-nearest, ties-to-even
binary64 operation with gradual underflow. Do not use fast-math, reassociation, implicit contraction,
or `mul_add` in this path. This operation contract must be verified on supported Windows/Linux
targets during implementation; arbitrary caller changes to the floating-point environment are not
supported.

1. Classify first. Reuse an `On` vertex exactly (apart from normalized zero); no parameter arithmetic
   is needed for tangencies/coplanar edges/triangles. Strict-crossing edges alone use steps 2–6.
2. Orient the edge by Z as `lo.z < h < hi.z`. Compute `d_lo = fl(h-lo.z)` and
   `d_hi = fl(hi.z-h)`. If either is non-finite, return `DistanceOverflow` for this local edge.
   Both must be strictly positive. A zero result contradicts the supported arithmetic assumptions
   for distinct represented finite inputs; return `UnrepresentableParameter` if encountered.
   Subnormal positive distances are allowed. When a binary64 subtraction's exact result is
   subnormal, it is an integer multiple of the least subnormal and representable exactly.
3. Choose `near = lo` when `d_lo <= d_hi` (fixed tie rule), otherwise `near = hi`. The other endpoint
   is `far`. Let `d_near` and `d_far` be those computed distances. Compute
   `r = fl(d_near/d_far)`, `den = fl(1+r)`, then `s = fl(r/den)`. Require `r` and `s` to be finite
   **normal positive** values and `s <= 0.5`. Otherwise return `UnrepresentableParameter`.
   This deliberately rejects extremely unbalanced strict crossings even if a weaker absolute
   construction guarantee might allow them. It preserves a relative bound on the small weight.
   Never compute `d_lo+d_hi`; that sum can overflow for a perfectly balanced crossing at zero.
4. For each axis X then Y, let `n = near[axis]`, `f = far[axis]`:
   - If `n == f`, copy that coordinate exactly.
   - If `n` and `f` have opposite strict signs, compute `w = fl(1-s)`,
     `q = fl(fl(w*n) + fl(s*f))`. Products are bounded in magnitude by their endpoint magnitudes;
     the sum has opposing signs and cannot overflow.
   - Otherwise compute `delta = fl(f-n)`, `q = fl(n + fl(s*delta))`. Same-sign (or zero) endpoint
     subtraction cannot overflow, unlike the unrestricted formula.
   Check each evaluated intermediate/result is finite; otherwise `CoordinateOverflow`.
   Project the finite `q` to `[min(n,f), max(n,f)]` if rounding strays outside that closed interval.
   This explicitly documented numerical range clamp cannot increase distance from the ideal
   convex interpolant; it is not endpoint welding or a classification change.
5. Assign constructed Z directly to normalized `h`. Normalize every returned numerical zero to
   `+0.0`. XY subnormal rounding and underflow to zero are allowed under the absolute bound below.
   Output coincidence is explicitly recorded; no exact-interior or positive-length promise follows.
6. Retain `Edge(local_edge)` provenance even if coordinates now equal a corner or another result.
   Do not replace it with `Vertex` provenance or erase one of two coincident endpoint records.

This strategy is intentionally narrower than all finite-input construction, with a precise
data-dependent rejection domain. It accepts opposite-sign extreme Z at `h=0` when each distance is
finite, even if the standard denominator overflows. It rejects, for example, `lo.z=-MAX`,
`hi.z=MAX`, `h=MAX/2` because one distance overflows. Exact endpoint/coplanar events still succeed
at these extremes; unrelated source-area observation failure does not override them.

### Accuracy contract and derivation

Let `u = 2^-53` (unit roundoff, half `f64::EPSILON`) and `eta = 2^-1074` (least positive
subnormal). For a successful crossing coordinate, let `M = max(abs(n), abs(f))` for that axis and
`q*` be the exact real edge interpolant. The contract is the absolute forward bound

`abs(q - q*) <= 64*u*M + 16*eta` millimeters.

This bound is evaluated mathematically (or with a scaled/exact test oracle), not by overflowing
`64*M` in `f64`. It is not a geometric tolerance, a relative error guarantee near zero, a mesh
accuracy target, or a license to merge points. There is no positive lower bound on the returned
segment length.

Justification: each finite normal distance subtraction has relative error at most `u`; subnormal
distance subtraction is exact as above. Let `R = D_near/D_far` for the exact positive distances
of the chosen anchor, and `S = R/(1+R)`. Distance subtraction and the first division give

`(1-u)^2/(1+u) <= r/R <= (1+u)^2/(1-u)`.

For positive `R,r`, the ratio of `r/(1+r)` to `R/(1+R)` lies between one and `r/R`. Accounting
also for the rounded denominator addition and final division therefore bounds the computed weight:

`(1-u)^3/(1+u)^2 <= s/S <= (1+u)^3/(1-u)^2`.

At binary64's `u`, each endpoint of this interval differs from one by less than `6u` (the
inequalities follow directly by multiplying the positive denominators). Since `0 < S < 1`,
`abs(s-S) < 6u < 8u`, without requiring the rounded choice of nearer endpoint to be exact.
Perturbing the ideal weight thus changes one XY coordinate by less than `16u*M`.

For fixed computed `s`, the sign-specific interpolation has at most a complement, a difference,
two products and a sum, with pre-rounding magnitudes bounded by `2*M` plus earlier rounding error.
Applying `abs(fl(x)-x) <= u*abs(x) + eta/2` at each applicable operation bounds that part by
`16u*M + 8*eta` (including products of the rounding factors at this fixed `u`). A same-sign
difference is at most `M`; opposite-sign products are at most `M` and their sum cannot grow by
reinforcing signs. Together these bounds are below the specified `64u*M + 16*eta`; the extra
margin covers all higher-order terms and boundary rounding without changing classification.
Range projection cannot enlarge the forward error. Implementation tests must check this bound
against independent oracles; relaxing it requires a revised argument and decision, not a wider
convenience epsilon.

No claim is made that coordinates are correctly rounded exact intersections or have bounded
relative error in cancellation-dominated XY. Z assignment and on-plane vertex copies are exact
after signed-zero normalization. A source equality never requires approximation.

### Error precedence and failure contract

Validation order is corner 0/1/2, each axis X/Y/Z, then `plane_z`; return the first non-finite
input error. All finite inputs classify. Only necessary strict-crossing constructions can fail.
Process active crossing edges by their canonical geometric edge key (Section 6), then X/Y,
so arithmetic failure selection is stable under relabeling, except tied indistinguishable edges
use local IDs. Return no successful partial event. Do not fall back to a tolerance or silently
substitute an endpoint on error.

| Error | Exact trigger |
| --- | --- |
| `NonFiniteVertex { corner, axis }` | Supplied corner coordinate NaN or infinity |
| `NonFinitePlane` | Supplied plane NaN or infinity |
| `DistanceOverflow { edge }` | One required `h-lo.z` or `hi.z-h` subtraction is non-finite |
| `UnrepresentableParameter { edge }` | Required positive distances fail positivity, or `r/s` fail the normal-positive/finite/range checks above |
| `CoordinateOverflow { edge, axis }` | A required XY interpolation intermediate or output is non-finite |

These are numerical construction errors, not malformed STL, mesh invalidity, resource exhaustion,
or cancellation. Do not wrap them in `TransformError` or `MeshError`. The supported arithmetic
model should make coordinate overflow unreachable for the chosen sign branches; retain its typed
guard and test the internal finite check rather than fabricate a reachable public failure fixture.

## 6. Provenance, ordering, and permutation law

Corners are `Corner0`, `Corner1`, `Corner2`, corresponding to the input array. Edges are
`Edge01`, `Edge12`, `Edge20`, matching M2's local edge order `0->1, 1->2, 2->0`. Source winding
is recoverable from those identities and the original corner sequence; canonical output order
is not a winding direction. No global mesh triangle or vertex ID is accepted by this primitive.
A later caller maps each local corner to its mesh vertex and attaches its source face ID.

An endpoint carries either `Vertex(CornerId)` for an exact on-plane source corner, or
`Edge(LocalEdge)` for a strict crossing. A coplanar edge additionally carries its complete local
edge ID; a raw coplanar triangle retains all three `{corner, position}` records. Equal positions
with different IDs are not merged. Original local sides and equality flags remain in source order.

For finite normalized points use numeric lexicographic `(X,Y,Z)` order, equivalent to
`total_cmp` after zero normalization. A geometric edge key is its two endpoint coordinates sorted
lexicographically; local edge ID breaks exact coordinate-key ties. Construction always uses the
lower/higher-Z rule in Section 5, independent of edge traversal direction or corner IDs.

Sort returned segment/coplanar-edge endpoints by coordinates, then provenance geometric key:
vertex before edge, edge's sorted source endpoint coordinates, then local provenance ID. Sort
coplanar triangle records by coordinates then corner ID. Endpoint sorting never changes the
incidence variant, source sides, or source IDs. Repeated evaluation of identical inputs yields
bit-identical coordinate payloads and equal provenance/context on the supported arithmetic model;
Rust `Debug` text, struct padding, and memory layout are not serialization formats.

For a corner permutation `pi` with `new[i] = old[pi(i)]`, remap new corner IDs by `pi` and edge
IDs by their unordered mapped corner pairs. Compare numeric geometry after canonical sorting;
it must be bit-identical, including context computed in coordinate order. Compare provenance as
the remapped set/multiset of endpoint/corner records, not raw enum ordinal order after tied
coordinate reindexing. Source-side arrays/equality flags obey the same permutation. Canonical
tie breaks may change record order among geometrically indistinguishable records; they may never
lose their multiplicity. Invalid-input errors need only follow the documented validation order.

## 7. Minimal public API proposal

Introduce `karasu_core::geometry::section` in M3.1 only, alongside its real implementation/tests.
Future file: `crates/karasu-core/src/geometry/section.rs`, exported by `geometry.rs`. Reuse existing
`Point3`. Keep error and small result types in this module; no top-level slicing workspace or
empty future modules. The following is a design signature and type inventory, not executable code:

```text
intersect_triangle_z(vertices: [Point3; 3], plane_z: f64)
    -> Result<TrianglePlaneIntersection, TrianglePlaneError>

VertexPlaneSide = Below | On | Above
CornerId = Corner0 | Corner1 | Corner2
LocalEdge = Edge01 | Edge12 | Edge20
EndpointOrigin = Vertex(CornerId) | Edge(LocalEdge)
SectionEndpoint { position: Point3, origin: EndpointOrigin }
SourceCorner { corner: CornerId, position: Point3 }
EndpointCoincidence = Distinct | CoincidentRepresented
TriangleAreaObservation = RepeatedPosition | ZeroComputedCross |
                          NonzeroComputedCross | UnavailableRange
TriangleContext {
    equal_corner_pairs: [bool; 3],
    area_observation: TriangleAreaObservation
}
TrianglePlaneEvent =
    None |
    TangentPoint { endpoint: SectionEndpoint } |
    Segment { endpoints: [SectionEndpoint; 2], coincidence: EndpointCoincidence } |
    CoplanarEdge { edge: LocalEdge, endpoints: [SectionEndpoint; 2],
                   coincidence: EndpointCoincidence } |
    CoplanarTriangle { corners: [SourceCorner; 3] }
TrianglePlaneIntersection {
    sides: [VertexPlaneSide; 3], context: TriangleContext, event: TrianglePlaneEvent
}
TrianglePlaneError = NonFiniteVertex { corner: CornerId, axis: usize } |
    NonFinitePlane | DistanceOverflow { edge: LocalEdge } |
    UnrepresentableParameter { edge: LocalEdge } |
    CoordinateOverflow { edge: LocalEdge, axis: usize }
```

The single public operation validates all coordinates once and uses a private already-finite
scalar classifier. Its returned side array exposes the classification without a second public
validation API or an artificial source-corner identity for a standalone scalar.

Use `Clone, Copy, Debug, PartialEq` on finite-valued records and `Eq` additionally on discrete
types/error variants. Follow existing error conventions with `Display`, `std::error::Error`, and
`#[non_exhaustive]` on the public error enum. Coordinates/private paths are not copied into
diagnostics. No `Default` numeric policy, configurable epsilon, `Vec`, trait hierarchy, allocation
error, precision flag with unspecified meaning, or mesh-owned source ID is needed. Result records
may use public documented fields, as existing `Point3` does; guarantees apply to kernel-produced
results, not arbitrary caller-fabricated values.

Private helpers: validate finite triangle/plane; classify finite scalar; canonical geometric keys;
observe source degeneracy; construct one strict-crossing endpoint; interpolate one XY axis;
normalize numerical zero; sort two endpoints/three corners. No public normal or exact-rank API.

Compatibility: M2 source point/index buffers, `MeshError`, `Degeneracy`, and transform signatures
stay unchanged. A caller chooses three points in source or world space and passes the plane in
that same space. Transforms and their failures remain an outer explicit step. M2 source-topology
diagnostics are additional context; they cannot substitute for post-transform observations after
a zero scale or numerical collapse.

## 8. Coplanar and later-stage decisions

Detection (M3.1), ownership among adjacent faces (M3.2), and contribution to a filled region
(later contours/polygons) are distinct decisions. Keep all original local sides, corner/edge IDs,
coordinate-identical multiplicity, and coplanar source incidence so later stages can select a
half-open or other proved rule. A local coplanar-edge event cannot inspect its neighboring face.

EXP-04's pending tangent/coplanar observations remain compatibility research; no commercial rule
is frozen. EXP-03 remains a dependency for final layer scheduling/first and terminal inclusion,
not for evaluating one supplied `h`. Integer polygon scale remains gated before M3.4. No M1 or
whole-M3 acceptance is implied by this local contract.

## 9. Implementation-ready test specification

All tests below are planned, not yet run. No existing test or fixture changes are made in M3.0-B.
Use dependency-free deterministic enumeration, arrays, bit construction, and bounded integer/rational
oracles, consistent with M2 tests. New canonical STL files are unnecessary.

| Test family | Inputs and exact oracle | Required assertion |
| --- | --- | --- |
| Truth table | All 27 words at `h=0`, Z `-1/0/1`, fixed noncollinear XY `(0,0),(4,0),(0,4)` | Exact sides, variant, local corner/edge IDs, C/P/E specification counts |
| Permutations | Each word, all six corner permutations (162 invocations), plus degenerate fixtures | Section 6 bitwise geometry and remapped provenance/multiplicity law; full endpoint arrays checked |
| Analytic proper crossings | `(0,0,-1),(4,0,1),(0,4,1)`, `h=0` | Exact endpoints `(0,2,0)` on 20 and `(2,0,0)` on 01; no tolerance |
| Exact on-corner crossing | `(0,0,0),(4,0,-1),(0,4,1)` | Exact corner 0 plus `(2,2,0)` on 12 |
| Tangencies | `OBB` and `OAA`, every corner and all permutations | Source corner copied exactly; no interpolation or fake positive-length segment |
| Coplanar edge | `(0,0,0),(4,0,0),(0,4,1)`, also third Z `-1` | Exact complete edge 01 and endpoints, third side retained |
| Coplanar triangle | All Z zero, both windings, all permutations | All three corner records preserved, no region/ring or positive-area claim |
| Repeated positions | Three equal, one equal pair, coplanar/noncoplanar, distinct local IDs | Exact equality flags, `RepeatedPosition`, correct incidence and coincident endpoint records |
| Collinear | `(0,0,-1),(0,0,1),(0,0,2)`, `h=0`; also oblique dyadic collinearity | `Segment`, coincident represented endpoints with two edge origins, `ZeroComputedCross` for these exact simple cases |
| On-vertex collinear | `(0,0,0),(0,0,-1),(0,0,1)` | Segment incidence retained with Vertex(0) and Edge12 at the same point |
| Coplanar rank loss | `(0,0,0),(1,0,0),(2,0,0)`; all equal; repeated pair | Raw `CoplanarTriangle` with degeneracy context, never an asserted area face |
| Source observation limits | Distinct finite corners whose edge subtraction overflows, all on extreme plane | Successful raw event with `UnavailableRange`, not fabricated nonzero area or construction failure |
| Signed zero | All vertex/plane sign combinations of zero | Exact `On`, normalized `+0` in all returned point payloads, original input bits unchanged |
| Adjacent values | `h=1`, immediate predecessor and successor built from bits | Strict sides; no hidden band; representable-plane contract |
| Small subnormals | `eta`, `2*eta`, `3*eta` Z configurations, `h=2*eta` | Positive subnormal distances allowed; balanced parameter succeeds; exact axis-constant coordinates |
| XY gradual underflow | Crossing midpoint of X `0` and `eta`, simple normal Z | Correct rounded zero allowed; forward bound holds; source/edge origin unchanged |
| Very large accepted inputs | X/Y `+/-MAX` at balanced Z and vice versa; `h=0` | Finite convex construction, exact symmetric midpoint where operations are exact |
| Denominator overflow avoidance | Edge Z `-MAX,+MAX`, `h=0` | Success, although `hi.z-lo.z` would overflow; no summed distances |
| Required distance overflow | Edge Z `-MAX,+MAX`, `h=MAX/2`, with a valid third corner | Exact sides; intersection returns `DistanceOverflow` on the canonical failing edge |
| Parameter rejection | Edge Z `0,MAX`, `h=eta` | `UnrepresentableParameter`, not tangent; ratio cannot be normal |
| Normal-weight boundary | Cases just around normal/subnormal `r` and `s` using powers of two and adjacent bits | Success/rejection follows exact step-3 checks; no tolerance |
| Rounded endpoint coincidence | Sub-ULP changes to large common X/Y offsets with ordinary Z | Preserve both origins and `CoincidentRepresented`; no branch reclassification |
| Non-finite input | NaN, both infinities at every corner/axis and plane | Exact typed location/error precedence, zero partial results, no panic |
| Immutability/repeats | Reuse arrays, compare `to_bits()` before/after success and error | Inputs unmodified; normalized coordinate payloads and discrete records repeat exactly |

For inexact constructed coordinates, use the forward bound in Section 5 against an independent
oracle, not a second evaluation of the production formula. For ordinary dyadic/rational fixtures,
derive exact intersections with bounded `i128` numerator/denominator arithmetic and compare with
an outward error interval; ensure all oracle operations fit. Represent finite extreme floats as
signed integer significands times powers of two for exact side/endpoint/zero oracles. Exact
midpoint, constant-axis, on-plane copy, and zero-normalization cases require bitwise equality.
Do not approximate extreme expected values by parsing rounded decimal strings.

If a test oracle cannot evaluate `64*u*M + 16*eta` safely, compare scaled quantities or exact
dyadic rationals; do not drop the underflow allowance or overflow the error computation. For
ordinary nonzero expected values bounded away from zero, additionally constrain ULP error using
the derived absolute bound divided by local spacing; no universal ULP count is claimed at zero.

Properties must state their floating-point preconditions:

- Exact powers-of-two scaling preserves side words when input conversions are lossless. Coordinate
  scale equivariance is tested only where all selected operations scale exactly and neither test
  crosses subnormal/overflow/rejection boundaries; otherwise check analytic error bounds separately.
- Common XY translation preserves exact ideal events only if the input additions preserve all
  differences. Common Z translation must include the plane and preserve represented Z ordering/
  differences. Even then do not demand general bitwise construction equivariance: evaluate each
  translated ideal against its own bound. Use small integer/dyadic grids for exact equality tests.
- Corner permutations preserve the coordinate/provenance law under the same arithmetic domain.
  Arbitrary source winding reversal does not change the intersection set; orientation information
  resides in source identities, not the endpoint order.
- Returned points have exactly normalized `z=h` and coordinates inside each source edge's closed
  axis intervals. This is a bounding-box containment property, not exact collinearity of rounded XY.

Do not run new fuzzing/benchmarks in M3.0-B. M3.1 implementation must run stable format, strict
Clippy, workspace test/build and rustdoc, and subsequent native Windows validation. M2's accepted
counts stay historical; no unimplemented test family is reported as passed.

## 10. Research-to-decision traceability

Every choice in this contract follows from the mathematical deductions of Sections 2–6: exact
represented-f64 sides with zero tolerance (a finite scalar order needs no determinant or
subtraction); a checked small-weight construction with explicit rejection (a strict crossing is a
convex interpolant, and the literal floating formula can overflow); no imported constants,
welding or polygon scale (classification, construction error and mesh-wide identification have
distinct purposes); raw coplanar events with preserved context and multiplicity (on-plane
incidence alone cannot assign ownership or establish rank); a local geometry API without a
printer or layer schedule; and cancellation left to later mesh and layer orchestration (a
fixed-size primitive has no long-running traversal). Open compatibility questions remain
EXP-03/04.

## 11. Independent mathematical references

These references inform mathematical principles, not dependency adoption or source translation.
URLs were checked on 2026-10-16.

1. David Goldberg, “What Every Computer Scientist Should Know About Floating-Point Arithmetic,”
   *ACM Computing Surveys* 23(1), 5–48, March 1991. Edited reprint in Oracle's *Numerical
   Computation Guide*: [rounding, cancellation, exact rounding and gradual underflow](https://docs.oracle.com/cd/E19957-01/806-3568/ncg_goldberg.html).
   Supplies the rounding/error framework. The construction bound here is independently derived
   for this operation sequence; it is not a theorem quoted from that paper.
2. CGAL manual, [“Predicates and Constructions,” Chapter 4, release 3.1](https://doc.cgal.org/Manual/3.1/doc_html/cgal_manual/Kernel_d/Chapter_predicates_constructions_d.html).
   A predicate decides a relation; a construction creates new numerical values subject to
   representation error. Citing this distinction does not select CGAL or any C++/FFI dependency.
3. Jonathan Richard Shewchuk, “Adaptive Precision Floating-Point Arithmetic and Fast Robust
   Geometric Predicates,” *Discrete & Computational Geometry* 18(3), 305–363, 1997; technical
   report CMU-CS-96-140, May 1996.
   [Author's publication page](https://www.cs.cmu.edu/~quake/robust.html).
   Adaptive determinant predicates address a different problem than ordering two finite scalar
   coordinates. No adaptive predicate dependency is justified for M3.1's side test. A later exact
   rank/orientation/nesting requirement needs its own algorithm/dependency review.

## 12. First implementation increment and gate

Implement only `geometry::section` and the types/helpers in Section 7, with the Sections 2–6
semantics and Section 9 deterministic tests. Add its public export, executable API documentation,
new focused integration tests (expected `crates/karasu-core/tests/triangle_plane.rs`), and unit tests
for private arithmetic/ordering helpers. Update documentation from planned to implemented only
after these checks succeed. No CLI command, mesh traversal, fixture generator, canonical STL,
new dependency, layer iterator, progress, or cancellation addition belongs to that increment.

M3.0-B acceptance is **PASS for the documented local contract**: scope, exact sides, incidence
events, all 27 states, source context, bounded numerical strategy/errors, deterministic provenance,
independent references and test plan are specified. The source-area observation remains explicitly
inexact; the contract never consumes it as a rank certificate. This is a defined limitation, not an
unanswered positive-area predicate requirement for this raw local API.

**Recommendation:** M3.1 is design-ready for this exact increment, subject to a separate implementation
request. This task does not start implementation. Later coplanar ownership, contour topology, final
layer scheduling and integer scaling remain gated; M1 remains open and M3 remains incomplete.
