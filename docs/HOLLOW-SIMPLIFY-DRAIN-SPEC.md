# Cavity simplification and oriented drain holes (TD-066)

**Status:** implemented on 2026-10-19; completes hollowing before M6.

**Research and owner decisions:** research.

**Builds on:** [sampled hollowing](M55F-SAMPLED-HOLLOWING-SPEC.md) (TD-045) and
[sampled drainage](M55G-SAMPLED-DRAINAGE-SPEC.md) (TD-047).

**Code:**
- `preparation::sampled::simplify`;
- `preparation::drainage::cylinder::oblique_cylinder`;
- `preparation::sampled::drain` (oblique prisms, surface holes);
- `Bvh::{lower_bound, index_planes}`;
- the plan operations `Simplify` and `SurfaceHole`;
- the CLI and the plate document.

## Cavity simplification

### Request

- **Plan:** the operation `Simplify { tolerance_mm }` directly after the hollow (plan tag 4). Plans
  without it keep their bytes.
- **CLI:** `--hollow-simplify <mm|auto>` (requires `--hollow`). `auto` is half the smaller pixel
  pitch, at most half the calibrated layer height: below that, differences cannot reach the
  masks.
- **Plate document:** `"hollow": {"simplify_mm": <mm>|"auto"}`.
- **Limits:** the tolerance must be positive and below the wall thickness.

### Method

The extracted cavity is simplified by deterministic half-edge collapses, before certification:
- **Collapse:** a vertex `u` merges into a neighbour `v`. Surviving vertices are original
  iso-surface vertices, and the star of `u` (its faces) is replaced by a fan around `v` over the
  same boundary loop.
- **Choice:** among the neighbours, the nearest valid one wins (ties by index). Candidates are
  ordered by their deviation bound, then their index.

**Every accepted collapse proves the following.**

**Topology.** The link condition holds: the two vertices opposite edge `uv` are their only common
neighbours. The surface therefore stays a closed edge-manifold with the same components, which
is re-validated after simplification.

**Deviation (both directions).**
- The star and the fan both project injectively onto one plane: every face has an
  interval-proven positive orientation along the star's mean normal `n`.
- They are then two piecewise-linear height functions over the same polygon. At any point their
  difference is at most the spread of the star's vertex heights along `n` (outward intervals).
- Each face carries a bound `eps` on its distance from the original surface; a new face gets
  `eps = spread + max eps of the star`.
- The bound holds in both directions: every point of the simplified surface is within `eps` of
  the original surface, and every point of the original surface within `eps` of the simplified
  one.
- Only collapses with `eps ≤ tolerance` are accepted.

**Wall floor.**
- Every new face is proven exactly (`Bvh::clearance`: exact contact test, rigorous separation
  bounds) to be farther than the floor from the outer surface. The floor is
  `min(thickness, thinnest original cavity vertex) − 2 × query error`.
- The wall therefore never becomes thinner than the requested thickness, as reconstructed. Since
  cavity points are at least as far as the cavity surface, every cavity point is certified too.
- The floor is global, not pointwise. Where the original wall was thicker than the floor (near
  edges and recesses), the cavity may move outward by at most the tolerance. On FX-12 the cavity
  volume grew by 0.14 mm³ (`auto`) and 0.56 mm³ (0.02 mm), while the certified minimum wall
  rose.
- The owner accepted this behaviour; a pointwise "only thicker" constraint is to be reconsidered
  with the M6 user interface.
- The cavity also stays inside the solid: its vertices are interior and no face meets the
  surface.

**Embedding.** No new face meets any other cavity face except where shared vertices allow (exact
`improper_contact`).

**Face size.** No new edge is longer than 32 mean original edges, which keeps the embedding checks
local.

### Determinism and performance

**Rounds** (at most 64; they stop when nothing is applied):
1. Evaluate in parallel the candidates whose neighbourhood changed; the others stay cached.
2. Select an independent set whose stars are two rings apart.
3. Prove embedding against the faces at the start of the round, and the wall floor, in parallel.
4. Apply the collapses sequentially in order, checking embedding against the faces created
   earlier in the round.

Results are identical for every thread count.

**Fast rejection.** The clearance search first rejects nearby source triangles with a rigorous
projection gap along either normal, compared in squares without a square root. Simplification
also indexes the source triangles' planes (`Bvh::index_planes`).

**Measured on FX-11** (60 mm sphere, 327,680 triangles), 2 mm wall, 0.25 mm cell, Linux release,
16 threads:

| Tolerance | Cavity triangles | Certified deviation | Total time | Peak memory | Cavity volume change |
| --- | --- | --- | --- | --- | --- |
| none | 1,415,016 | — | 2.3 s | 0.43 GB | — |
| `auto`, 0.007 mm (16K printer) | 232,532 | 0.0070 mm | 11.6 s | 0.71 GB | −3.8 mm³ |
| 0.02 mm | 95,000 | 0.0200 mm | 14.1 s | 0.73 GB | −13.4 mm³ |

The volume change is the extra resin: 0.004 mL and 0.013 mL here. Simplification costs time and
transient memory once, during preparation; the cavity it leaves is smaller to keep, preview
and slice.

On flat walls the certified deviation is about 10⁻¹² mm: a 12 mm cube's cavity drops to a small
fraction of its triangles.

### Certificate

`SampledHollowCertificate::simplification` records:
- the tolerance and the largest certified face deviation used;
- the wall floor;
- the triangle counts before and after;
- the cavity volume before.

The a posteriori enclosure bounds simplified faces from their proofs: below by the wall floor,
above by the original enclosure plus their deviation. The a priori completeness band widens by
the tolerance.

**CLI summary line:**

```
Cavity simplification: tolerance (mm) T, certified deviation (mm) D, triangles A -> B,
volume before (mm^3) V, wall floor (mm) F
```

## Oriented drain holes

### Inclined holes

- **Request:** a `Hole` may have any direction.
  - The CLI's `--drain-hole` axis token accepts `dx,dy,dz` besides `±x ±y ±z`.
  - The plate document accepts `"direction": [dx, dy, dz]` instead of `"axis"`.
- **Cardinal directions** keep the exact construction (byte-identical outputs).
- **Other directions** use `oblique_cylinder`.
  - **Geometry:** a ring in the plane through the entry, spanned by an orthonormal basis across
    the unit axis; the end ring is the entry ring moved along the axis.
  - **Certificate:** in outward intervals, against the requested cylinder (axis through the entry
    along the raw direction):
    - **axial error:** the end-centre displacement;
    - **radial error:** vertex distances from the axis, the chord sagitta and the lateral skew;
    - **winding:** proven exactly once from positive, sub-right-angle steps whose sines sum
      below 8, so that the turning stays under two turns.
  - **Segments:** doubled from 8 up to 4096 until the radial and axial errors fit the plan's
    distance error.
- **Per-layer cut:** the convex hull of the plane's crossings with every ring and lateral edge,
  which is exactly the section of the convex prism. A plane through a single vertex removes
  nothing.
- **Clearances:** entry and end caps are admitted exactly (fan pieces), as before. The reported
  clearances of oblique caps come from a new certified best-first lower bound
  (`Bvh::lower_bound`).

### Surface holes

- **Request:** `SurfaceHole { surface_mm, diameter_mm, outside_mm, inside_mm }` (plan tag 5).
  - **CLI:** `--surface-drain-hole <x> <y> <z> <diameter>`, with both margins 1 mm.
  - **Plate document:** `{"surface_mm": [...], "diameter_mm": d, "outside_mm"?, "inside_mm"?}`.
- **Resolution during drainage** (deterministic, so the plan records only the request):
  - **Foot:** the nearest point of the outer surface to `surface_mm`.
  - **Axis:** the inward, area-weighted mean normal of every outer face within the hole's radius
    of the foot. This is smoother than the single-face normal of the references.
  - **Entry:** `outside_mm` outside the foot.
  - **Length:** to the first cavity surface along the axis, by bisection on the exact segment
    test, plus `inside_mm`.
- **Certification:** the resolved cut is certified like any other.
- **Refusals:** a missing cavity, an undefined normal or an end cap that does not fit refuses
  the hole, naming it.
- **Reporting:** the summary reports the built axis of every hole:
  `Drain hole N axis: entry (mm) x y z, direction dx dy dz, length (mm) L`.

### Print resolution

A hole must be at least two pixel pitches wide, and its diameter at least two layer heights
times the sine of its tilt from vertical (vertical: none; horizontal: two layer heights).
- Explicit holes are checked before hollowing.
- Surface holes are checked for the pixel pitch first and along their resolved axis after
  drainage.

### Supports and plates

Support clearance from drain holes, and plate footprints, use the holes as built (resolved surface
holes included).

## Not in this version

- Hole shapes: plugs, tapers and square holes.
- Automatic hole placement and suction-cup detection.
- Infill lattices; outer and bottom-through hollowing modes.
- Simplification without the wall floor (it would allow walls below the minimum).
- A pointwise "only thicker" constraint (to be decided with the M6 user interface).
