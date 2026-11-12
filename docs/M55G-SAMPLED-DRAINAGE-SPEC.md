# M5.5-G drainage cuts for sampled (non-convex) cavities

## Purpose and status

`preparation::sampled::drain_model_sampled` (TD-047) cuts finite drainage holes into cavities made
by the sampled hollowing backend ([M5.5-F](M55F-SAMPLED-HOLLOWING-SPEC.md)). It works for convex
and non-convex models, nested shells and several bodies. Entries with a free straight escape need
nothing more; other entries require embedded, pairwise-disjoint source surfaces (checked; see
below).

It reuses M5.5-C ([convex drainage](M55C-DRAINAGE-SPEC.md)) wherever no convexity is involved:
- plan schema: one hollow operation followed by `Hole` operations;
- certified cardinal polygonal cylinders, with their radial and axial error bound;
- `DrainageError` and `OpeningIssue`, extended with `Overshoot` and `DrainageError::Cut`;
- the conservative cutter-separation policy.

Two things differ: the opening certificate, and how the drained solid is evaluated.

## Semantics

The drained solid is `I \ (C ∪ K1 ∪ … ∪ Kn)`, where:
- `I` is the source's even-odd interior;
- `C` is the reconstructed cavity;
- `Ki` are the represented cutter prisms.

The original source is never modified.

A cut drains the cavity region that contains its end cap. Cavity regions are reported as follows:
- **Region identity:** cavity surfaces are edge-connected components of the outward cavity mesh.
  A positive-volume component is the outer boundary of one region. A negative-volume component is
  an island (a preserved inner void with its wall), owned by the innermost outer surface around it.
  Ownership comes from one exact ray per island that counts only outer surfaces: subtrees of the
  surface index holding island triangles alone are skipped, so stacked islands do not make the
  rays quadratic, and parity is kept as a sparse list of odd components. Cancellation is checked
  per island.
- **Orientation:** component signed volumes are summed about each component's own first vertex,
  so rounding scales with the component's size, not its distance from the origin. This keeps the
  outer/island sign reliable even for tiny reconstruction pockets far from the origin; such
  pockets become their own (enclosed) regions.
- **Region data:** its volume is the outer volume minus its islands, together with its number of
  bounding surfaces and the operations that drain it, in plan order.
- **Regions with no certified opening** are counted by `enclosed_cavities`.
- **Not counted:** regions a prism happens to pass through without ending in them are not claimed
  drained. This is a conservative under-report.

## Opening certificate

Each cut is built as in M5.5-C. The cap centre lies in the cap: the axial coordinate of each ring
is the centre's own, and the certified inner radius is positive. For the certificate each cap is
covered by the wedges from its centre to its edges: a ray from the centre through any cap point
leaves the cap through an edge. (Should a centre not lie exactly in its cap plane, the cap is
fanned from its first vertex instead.) Small wedges keep each query local, whereas a fan from a
vertex makes long slivers whose boxes cover most of the cap.

**Entry cap.** It is certified to lie in the unbounded exterior:
1. No cap wedge meets any placed source triangle. This is an exact closed-set test
   (`contact::triangles_meet`, built only from exact `orient3d` signs and coordinate comparisons).
2. Every cap wedge is proven farther than `minimum_clearance_mm` from the source surface
   (see [Distances](#distances)).
3. **Escape.** A straight segment joins the cap centre to a point outside the source's bounding
   box and meets no source triangle (exact `contact::segment_meets_triangle` through the BVH).
   The 26 directions towards the neighbours of a unit cube are tried in a fixed order: away from
   the model (opposite to the cut) first, then the other axis directions, the 12 face diagonals
   and the 8 body diagonals. The outside of a box is connected, unbounded and free of the
   surface, so the centre lies in the unbounded component of the complement. The BVH prunes
   boxes with a conservative slab test: per axis, the parameters where the segment crosses the
   two slab planes are enclosed in outward-rounded intervals, so a touched box is never skipped
   and only the exact test decides.
4. **Fallback when every direction is blocked** (for example an entry deep in a winding recess):
   the cap centre has even parity with respect to every edge-connected source component
   separately (exact symbolic-perturbation ray parity per component label). This is sound only for
   an embedded source, so the embedded-source admission below runs first, at most once per call.
   A source that fails it refuses that hole with `SourceNotEmbedded`.

Because the cap is connected and disjoint from the surface, every point has the same complement
component (and the same per-component sides) as its centre. For embedded, pairwise-disjoint
surfaces, being outside each component individually means being in the unbounded complement. So
an entry inside a sealed void, which is "outside" by global parity, is refused. Edge connectivity
keeps shells that touch only at a vertex apart.

**Embedded-source admission (fallback only).** A BVH-against-BVH traversal checks every pair of
triangles whose boxes overlap, with exact predicates:
- triangles sharing no vertex must not meet;
- triangles sharing one vertex must not meet elsewhere (an opposite edge crossing the other
  triangle, or a coplanar incident edge entering its corner);
- triangles sharing an edge must not fold onto each other coplanarly; when both are collinear
  (zero area), they must not extend beyond the same endpoint of the edge;
- duplicated faces are rejected.

Without it, two touching or overlapping bodies (for example a lid resting on an open cup) can seal
a pocket that is outside every body individually, which parity would wrongly certify as exterior.
Point contacts (shared vertices only) are allowed: they cannot seal a volume.

Two exact shortcuts decide most pairs before the general rules:
- **Plane side.** If the unshared vertices of one triangle lie strictly on one side of the other's
  plane, the pair meets at most in its shared vertices or edge.
- **Coplanar pairs.** They are decided in a projection where the common plane keeps positive
  area. With one shared vertex `w`, the intersection is convex and contains `w`, so it contains
  another point exactly when the two corners at `w` overlap, that is, when one corner contains a
  boundary ray of the other. With no shared vertex, two closed convex polygons are disjoint
  exactly when one lies strictly outside an edge line of the other: the origin then lies strictly
  outside an edge of their Minkowski difference, and each such edge is parallel to an edge of
  either polygon.

The traversal polls cancellation throughout. Its cost is proportional to the number of
box-overlapping triangle pairs: near-linear for well-shaped meshes, but up to quadratic in the
triangle count when boxes overlap widely, for example around the apex of a fan-triangulated cap or
along long sliver strips on a planar face. This is why it only runs when no escape exists.

**End cap.** It is certified to lie inside one cavity region:
1. No cap wedge meets the cavity surface (exact test).
2. Every cap wedge is proven farther than the clearance from the cavity surface.
3. The cap centre has odd total parity over the cavity surface.

The region is the smallest-volume outer surface whose individual parity contains the centre. The
cavity is embedded by construction (marching tetrahedra).

**Connectivity argument.** The prism is a connected set removed from the material. One end lies in
the unbounded exterior and the other in the region, so the void "exterior ∪ prism ∪ region" is
connected. No convexity of the source or cavity is used. The statement is about the represented
geometry, which is exactly what is sliced. It does not prove that resin physically drains or vents.

**Failure categories** (`DrainageError::Opening`):

| Issue | Cause |
| --- | --- |
| `EntryNotExterior` | The entry cap meets the surface, is too close to it, or is not in the unbounded exterior. |
| `MissedModel` | The end-cap centre is outside the source. |
| `Shallow` | The end-cap centre is in the material or exactly on the source surface, and the cut axis never crossed the cavity. |
| `Overshoot` | The end-cap centre is in the material or exactly on the source surface after the cut axis crossed the cavity (too long). |
| `InsufficientClearance` | The end-cap centre is in the cavity or exactly on its surface, and the end cap touches or nears that surface. |
| `SourceNotEmbedded` | None of the 26 escape segments is free for the entry, and the source fails the embedded-source admission. |

Classification uses the cap centre computed from the plan (entry + unit axis · length), not a
ring vertex. Contact of the centre with either surface is decided first by an exact point query,
whatever the face orientation; parity only decides points off the surface. Overlapping cutters fail with `PotentialOverlap`, as in M5.5-C. A cylinder that
cannot be built (axis, size or error budget) fails with `DrainageError::Cut`, which names the
operation.

## Distances

Intersection is decided exactly. Clearances are certified lower bounds:
- **Candidate search:** the computed distance of each disjoint triangle pair is the minimum over
  vertex-to-triangle and edge-to-edge candidates. A vertex-to-triangle candidate is a vertex
  (first-order region tests), the nearest edge point, or the foot of the perpendicular when it
  lies in the triangle. The foot is a convex combination `a + β·ab + γ·ac` whose coordinates come
  from a Gram-Schmidt complement of `ab`: accurate for long thin triangles, and still a point of
  the triangle (up to rounding) when a near-degenerate triangle makes them arbitrary. Every
  candidate is therefore an upper bound up to rounding. These values drive branch and bound over
  the BVH, with inclusive boxes.
- **Box distances** are rigorous lower bounds: the per-axis gaps are scaled by the largest one
  before squaring, so no square underflows into the subnormal range (where a rounded square can
  be twice too large), the result is shrunk by a relative 10⁻¹², and a result that would itself
  be subnormal is replaced by the largest gap, which is exact there.
- **Certified lower bound:** for every examined pair, an interval-arithmetic separation along the
  computed closest direction and both triangle normals is a rigorous lower bound, because
  projection onto a unit direction is 1-Lipschitz.
- **Admission:** each cap wedge is queried with the required clearance as the threshold, so
  boxes farther away are pruned at once; cancellation is polled inside each query. Every pruned triangle is farther than the threshold or
  than some examined one, so the minimum of the examined bounds and the computed nearest
  distance bounds the whole surface even when a computed distance overestimates (thin triangles,
  nearly parallel edges). "Beyond the threshold" is decided from this certified value only,
  never from a computed distance alone.
- **Reported clearance** (`entry_clearance_mm`, `terminal_clearance_mm`): a certified lower bound
  for the whole cap, through the disk that encloses it (centre, axis and an interval upper bound
  on the vertex radius). The disk projects onto a direction `u` as `c·u ± r·|u'|`, with `u'` the
  part of `u` across the axis, so each bound below costs constant work whatever the number of
  sides. One best-first search orders nodes by a lower bound for their box: the distance from the
  cap's box and the projection gaps from the disk along the axis and towards the box centre (in
  space and within the cap plane). Each examined triangle contributes the largest of its box
  distance and its gaps along the axis, its own normal and the directions towards its centroid.
  At any point the minimum of the examined bounds and the next node key bounds the whole surface,
  so the search stops when the next key is no smaller, or after 16,384 node visits, and the value
  stays certified either way. It is at least the required clearance, which the admission proved
  strictly exceeded. Surfaces crossing the cap plane just outside the disk are bounded radially:
  for a tube 0.1 mm outside a 3 mm cap the report is 0.099 mm.
  A cap far inside a large cavity therefore costs a bounded search instead of a near-exhaustive
  one: for an end cap at the centre of the hollowed FX-11 sphere the bound is 26.0 mm (the true
  distance is about 26.5 mm).

The required clearance (default 1e-6 mm) is an admission margin, not a tolerance applied to
geometry.

**Exact arithmetic cost.** Coplanar and collinear configurations make exact signs zero, which no
interval filter can decide. `orient3d` therefore has a fixed-width exact path: dividing all twelve
coordinates by the smallest power of two among their odd-mantissa forms changes the determinant
by a positive factor only. When the scaled coordinates stay below 2¹²⁰, the determinant
(below 2³⁶⁶) is evaluated exactly in six 64-bit limbs; wider exponent spans use the general
6400-bit integers. Interval square roots verify their bounds by an exact 128-bit comparison of
squares. Both fast paths are compared against the general integers in randomized tests.

## Evaluating the drained solid

Exact per-layer polygon difference (M3.5-C) is unsuitable for marching-tetrahedra shells:
- its cost grows with the square of a layer's edge count;
- its common dyadic encoding needs a per-layer coordinate ratio below about 2⁸.

Measured on one layer: the equatorial layer of the hollowed FX-11 sphere (0.25 mm cells, placed
at X 50, Y 30) has 4,988 path edges. The construction's default input cap is 4,096 edges, and its
pair cap allows about 1,414. The layer's coordinate magnitudes span 1.4·10⁻⁴ to 80 mm, a ratio of
about 2¹⁹.

Instead the shell is sliced as one strict represented-coordinate stream (`body_layers`). The
cutters are evaluated analytically: `SampledDrainedModel` implements `raster::LayerCut`. For a
plane `z`, each separated prism contributes:
- **Vertical prism:** its certified ring, when `z` lies strictly between its caps.
- **Horizontal prism:** a rectangle, its axial extent times the ring's chord at height `z`, when
  that chord has positive length.

A plane exactly on a cap, or tangent to a prism edge, removes nothing (a zero-area boundary), so
no plane position can make a layer fail. Slicing the prisms as meshes would report such tangent
contacts as unresolved geometry.

`raster::stream::DifferenceRasterStream` (or `raster::rasterize_layer_minus_rings_with_cancellation`)
subtracts the rings from each body layer per raster sample. A sample is lit exactly when it is
inside the body and outside every cut ring under the even-odd rule, which equals sampling the
polygonal difference. The work is linear in the edges; no difference polygon is built.
- **Outside the panel:** cut vertices may lie outside the panel, because removing material there
  changes nothing. The body keeps its configured outside policy.
- **Antialiasing and compensation:** grayscale sampling and compensation operate on the subtracted
  coverage exactly as on a plain mask.
- **Payload accounting:** the raster preflight counts the per-edge span buffers as well, and both
  edge buffers at their full reserved capacity (every cut-ring vertex) before either is reserved.

## Limits and validation

*TD-066 lifts the cardinal-axis restriction: cuts may have any direction, or be perpendicular to
the surface and run into the cavity ([specification](HOLLOW-SIMPLIFY-DRAIN-SPEC.md)).*

`SampledDrainageLimits` defaults:
- hollowing limits;
- 32 holes;
- at most 4096 cylinder sides;
- 1e-6 mm clearance.

The two surface indexes (source and cavity) are admitted against the hollowing payload cap before
either is built, at a fixed per-face estimate of their construction peak (about 300 bytes). Every
buffer whose size grows with the input is reserved fallibly at its final size before it is filled
(the BVH reserves its exact node count, computed from the triangle count), and index construction
checks cancellation before splitting every large range. They are the main memory cost of drainage: for FX-11 with 0.1 mm
cells (8.8 million cavity triangles) the peak resident size is 2.6 GB, against 0.9 GB for
hollowing alone.

The plan's `distance_error_mm` is both the hollowing query margin and the cylinder approximation
budget. The CLI uses `min(1 µm, cell/32)` when holes are requested, which yields polygons of a few
hundred sides, and keeps 1e-6 mm for plain hollowing.

**Print resolution (CLI).** A certified opening is a property of the represented prism. The sliced
file samples it at pixel centres and layer planes, so the CLI refuses holes that could vanish in
the sampled output:
- **Vertical holes** need a diameter of at least two pixel pitches. The disk then contains a pixel
  centre on every crossed layer, giving a continuous column through the wall.
- **Horizontal holes** need at least two pitches and two layer heights. Some layer plane then lies
  within half a layer of the axis and cuts a chord wider than one pitch, giving a row of removed
  samples from outside into the cavity.
Checkpoints: `DrainageStart`, `DrainageWork` (also polled while building the surface indexes,
inside every clearance query and search, and inside the embedded-source traversal) and
`DrainagePublish`, plus the hollowing checkpoints. Sorting the edge keys of a surface is the
longest step without a poll: about 0.7 s for the 8.8 million cavity faces of FX-11 at 0.1 mm cells,
and 0.85 s for a cavity with 14,720 preserved voids. Nothing partial is published. Progress adds
the `SampledHollowStage::Drainage` stage after hollowing, with processed and requested cuts as
item counts.

Tests:
- **Exact arithmetic:** the fixed-width `orient3d` path against the general integers on 30,000
  random cases (dyadic grids, full mantissas, coplanar sets and exponent spans beyond the fixed
  width); the 128-bit square comparison against the general integers on 40,000 cases.
- **Exact contacts:** segment/triangle and triangle/triangle cases, including vertex, edge,
  coplanar and collinear configurations; the plane-side and coplanar shortcuts against the general
  rules on 40,000 random grid pairs with 0, 1 or 2 shared vertices, plus zero-area neighbours and
  collinear faces sharing an edge (review counterexamples).
- **Distances:** pairwise distances against closed forms; hollowing a rotated 10 mm cube whose top
  edge is split by a zero-area face keeps the nearest cavity vertex exactly one wall away for four
  rotations that dented or broke the cavity with a normal-projection face case.
- **BVH:** odd-parity labels follow nesting, and node masks skip whole subtrees while keeping the
  remaining parities exact; the clearance query reports `Within` for a pair 1e-11 mm apart whose computed distance
  is 3.5e-7 mm (review counterexample) at every threshold; the slab filter never rejects a box
  that an exact oracle finds touched (20,000 random segments and boxes); cap lower bounds are
  tight for parallel faces and never above computed distances; the self-pair search finds
  contacts and stops on cancellation.
- **Raster difference:** interval subtraction against a boolean oracle (2000 random cases);
  per-sample membership for 1 and 4 samples per axis; an empty cut reproducing the plain render;
  a cut ring with 20,000 repeated vertices refused by the payload preflight before reservation.
- **Drainage (`tests/sampled_drainage.rs`):**
  - cube with a bottom hole, rasterized volume within 1% of `8000 − 16³ − π·1.5²·2`;
  - non-convex torus drained from above;
  - two bodies with one hole, leaving one enclosed region;
  - entry inside a sealed void refused;
  - preserved inner void giving one region with an island, and a column of twelve stacked voids
    giving one region with all twelve islands;
  - every failure category, including `Overshoot`, centred end caps crossing either side wall,
    and end-cap centres exactly on a source face: top, bottom, far and near vertical walls, a
    wall plane reached along the wall, and a vertical edge; overlap; a non-cardinal axis (`Cut`);
    cancellation; out-of-contract limits (no holes allowed, 4097 or 7 sides, non-positive or
    non-finite clearance) and a plan without a hollow operation;
  - progress per hole, and cancellation at `DrainageWork`;
  - a 1.6e-162 mm gap refused against a 2e-162 mm requirement and reported no larger than the gap
    against a smaller one (review counterexample), and a post beside the entry cap bounded
    radially;
  - the reviewers' counterexamples: a lid sealing a cup pocket (`SourceNotEmbedded` through the
    fallback admission, while a hole into the same cup from below escapes and is certified), a tiny far-off
    pocket (classified, no error), and an X hole tangent to layer planes with its entry outside
    the panel (sliced, volume within 1%).
- **Prism sections:** strict cap rule, tangent planes, X and Y rectangles.
- **Contacts and distances:** improper-contact rules for vertex- and edge-sharing pairs; certified
  bounds below the true distance for a 100 mm × 26 µm sliver and nearly parallel segments; box
  distances never above the gap down to subnormal gaps; disk gaps along the disk axis, across it
  and along tilted directions; exact BVH node reservations; index
  construction stopping on cancellation; a disk bound near 0.1 mm for 40,000 slivers crossing the
  cap plane just outside the disk.
- **CLI:** drained volume, region report, a refused entry with a positional message, resolution
  refusal, a panel-edge tangent hole, cylinder errors by position, the 32-hole cap and argument
  validation.

Measured in Release on Linux, through the CLI:

| Model | Holes | Total time |
| --- | --- | --- |
| FX-11 sphere (327,680 triangles), 2 mm wall | 2 | 10–12 s (0.56 GB peak) for 1200 layers at 1000×600 |
| Same, one 3 mm hole ending at the cavity centre | 1 | 12.3 s |
| Same with 0.1 mm cells (8.8 million cavity triangles), centre hole | 1 | 26.8 s (hollowing alone: 22.1 s) |
| Non-convex torus (25,920 triangles) | 2 | 1.6 s |

Measured in Release through the library (`drain_model_sampled`, 2 mm wall, one vertical hole;
hollowing alone in parentheses). These are the review rounds' slow cases:

| Model | Before | After |
| --- | --- | --- |
| Cylinder with fan-triangulated caps, 1000 sides (3,996 triangles), 0.25 mm cells | 58.2 s | 0.19 s (0.12 s) |
| Same, 4000 sides (15,996 triangles) | — | 0.37 s (0.29 s) |
| 20 mm cube with 100 × 100 grids per face (120,000 triangles), 0.25 mm cells | 94 s | 0.23 s (0.15 s) |
| Sheared strip prism, 8,064 triangles, axis escapes blocked by five boxes, 0.5 mm cells | 40.3 s | 0.42 s |
| Same, 16,064 triangles | 143 s | 0.67 s |
| FX-11 end cap at the cavity centre, clearance search alone (0.25 / 0.1 mm cells) | 24.3 s / 137.6 s | bounded (whole runs above) |
| 20 mm cube with a 360,460-triangle tube crossing the entry plane 0.1 mm outside a 4096-sided cap, 0.5 mm cells | 30.2 s | 0.60 s (0.43 s) |
| Same tube with 1,441,804 triangles and a 128-sided cap | 5.9 s | 2.6 s (1.8 s) |

When every escape is blocked, the fallback admission on the fan cylinder and the grid cube takes
0.70 s and 1.26 s; on long sliver strips it can grow quadratically (see the admission above).

Native Windows has not been validated.

## Limitations

- **Axis-parallel holes only.** Oblique holes need a certified oblique cylinder.
- **Region credit:** holes are credited only to the region holding their end cap.
- **Fallback admission cost:** entries without a free straight escape need the embedded-source
  admission, whose work can grow quadratically with the triangle count (wide box overlaps); it is
  cancellable.
- **Memory:** the drainage surface indexes roughly triple the peak memory of hollowing on very
  fine cavities.
- **No pocket policies:** minimum volume and keep-largest are not implemented.
- **Supports:** M5.5-D does not yet accept a sampled drained model as its base.
- **No physical claim:** resin evacuation, venting and wall strength around openings are not
  certified.
