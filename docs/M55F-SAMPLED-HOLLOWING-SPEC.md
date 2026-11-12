# M5.5-F sampled-distance hollowing for non-convex solids

## Purpose and status

`preparation::sampled` implements backend `sampled-distance-tetra-v1` (TD-045). It hollows
arbitrary closed source solids, convex or not, and also handles nested shells and multiple bodies.
It lifts M5.5-B's convex-only restriction for hollowing.

- **Inputs:** the existing preparation plan (TD-020/TD-041). The sampling `cell_mm`,
  `distance_error_mm` and `max_cells` that M5.5-A reserved for a grid backend get their intended
  meaning here.
- **Not covered:** supports (M5.5-D) still require the convex backend's `HollowedModel`. Drainage
  cuts into sampled cavities are [M5.5-G](M55G-SAMPLED-DRAINAGE-SPEC.md) (TD-047).
- **Basis:** the reference research and independent
  design. No code, assets or foreign kernel was imported, and no dependency was added.

## Semantics

- `S` is the placed source surface and `I` its even-odd interior.
- Parity is the same rule the slicer applies, so nested inner voids and multiple bodies behave
  consistently between hollowing and slicing.
- `d(p)` is the Euclidean distance from `p` to `S`, and `t` the requested wall thickness.
- The ideal cavity is `C = {p in I : d(p) > t}`, i.e. Euclidean inward erosion.
- The shell published for slicing is `S` followed by the reversed boundary of the reconstructed
  cavity. Even-odd slicing of this shell yields `I \ C`.
- Cavity components are counted and reported; none is filled or discarded.

## Admission

> **Superseded in part.** TD-054 admits balanced surfaces, touching bodies included, with the
> interior decided by the winding number. TD-092 r3 leaves zero-area faces and exact zero-area
> cracks out of the source (H1) and hollows overlapping bodies as their union (H2); see
> `docs/TECHNOLOGY-DECISIONS.md`. The rules below are the original M5.5-F admission.

- The plan must contain exactly one `Hollow` operation; `t`, `cell` and `error` must be finite
  and positive.
- **Source:**
  - Placed with the plan's affine transform; every placed coordinate within ±10000 mm.
  - Must have no boundary edges, no non-manifold edges and no M2-excluded degenerate triangles.
  - Non-manifold vertices are admitted: parity and distance are well defined for them.
  - Winding is irrelevant to even-odd parity.
  - Anything else fails with a structured `Unsupported` error. Nothing is repaired.
- **Query error:** `error` must be at least the binary64 query budget
  `2^-40 · (1 + M)`, where `M` bounds the coordinate magnitudes.
  - The budget assumes fewer than 40 dependent, correctly rounded operations per candidate point,
    each within 2^-47 relative error, so it leaves a margin of more than 100×.
  - Tests check it against closed-form cases.
  - It is an analytic budget, not an interval proof. Every candidate point is a point of the
    triangle up to rounding, so a misjudged region only overestimates. A near-degenerate face
    (which admission can let through after a rotation) never underestimates: its face candidate
    is a verified convex combination of the corners, taken together with the three edges (TD-047).
- **Cell size:** `L · cell · sqrt(3) + 2 · error < t` must hold for a leaf size `L` in {8, 4, 2, 1}.
  The largest such `L` is used; if none exists the request is rejected as "cell too coarse".
- **Grid:**
  - Its origin is the source minimum minus `(2 + 0.381966…) · cell` on each axis. The fixed
    fractional offset keeps grid planes away from layer planes and axis-aligned faces; a
    coincidence would be reported by the slicer as ambiguous, never silently accepted.
  - At least two outside cells pad every side.
  - Padded cells must not exceed `min(limits.max_grid_cells, plan.max_cells)`.

## Field evaluation

**Exact queries:**
- A BVH (deterministic median split, inclusive boxes) answers nearest-distance queries with
  point-to-triangle distance: vertex regions from first-order tests, otherwise the nearest of the
  three edge points and the Gram-Schmidt face foot when it lies in the triangle.
- Inside/outside comes from the parity of the vertical ray, using exact `orient3d`/`orient2d`.
  The query point is symbolically perturbed by `(e, e²)`, so projected edges and vertices are
  owned by exactly one triangle and zero-area projections are never counted.
- A query point exactly on the surface returns `OnSurface`, which is an explicit uncertain error.

**Adaptive blocks.** The grid is tiled into work items of `8L` cells and evaluated as an octree.
Each block gets one exact distance `d(c)` at its centre `c`; its ball radius is
`r = side · sqrt(3) / 2`. With margin `m = error`:
- If `d(c) + r + m < t`, every point is within `t` of `S`: no cavity in the block.
- If `d(c) > r + m`, the block cannot touch `S`, so it is one-sided. One parity query decides the
  side (it is inherited by children). Then:
  - outside: no cavity;
  - inside and `d(c) − r − m > t`: entirely cavity, no surface crosses it;
  - otherwise it is subdivided down to a leaf.
- A block that might touch `S` and is not already decided is subdivided. The leaf diameter
  satisfies `2r + 2m < t`, so such a block always resolves to material before reaching leaf size.
- Leaves are therefore always inside and straddle the iso-level. All `(L+1)^3` vertices get exact
  distances and `F = d − t`; a value of exactly zero counts as material.

Work is therefore proportional to the cavity surface area, not to the bounding volume.

**Threads and determinism.**
- Items run on `std` scoped threads, pulling indices from an atomic counter.
- Results are stored by item index and merged in index order, so the output is identical for every
  thread count; tests compare 1, 2, 3 and 8 threads.
- Cancellation is polled on the caller's thread between items and stops other threads at their
  next block.
- Resource counters are atomic. Exceeding a cap fails the run, and the lowest-indexed failure is
  reported.

## Extraction

Each cell is split into six positively oriented Kuhn tetrahedra sharing the main diagonal. The
split is the same in every cell, so the tetrahedra form one conforming triangulation.

On each tetrahedron the field is linear, and its zero set is a planar triangle or quadrilateral
strictly inside the tetrahedron:
- No vertex value is zero by construction.
- Edge parameters are clamped to `[2^-20, 1 − 2^-20]`. This keeps distinct iso-vertices distinct
  after rounding; every point of the edge still satisfies the a priori band.

Polygons of different tetrahedra meet only along shared iso-edges on common faces. The union is
therefore closed, edge-manifold and embedded. In exact arithmetic no polygon triangle is collinear;
the per-pattern tests check this.

Orientation is combinatorial:
- Even permutations put the positive vertices first.
- Normals point from the cavity towards the material.

Iso-vertices are keyed by grid edge (`lower vertex · 8 + direction`) and welded exactly; no
position welding is used.

After welding, an independent `O(T log T)` check verifies the construction:
- Every undirected edge occurs exactly twice, once in each direction.
- No triangle has a zero computed cross product.

Union-find over vertices then counts the cavity surface components.

## Certificate

`SampledHollowCertificate` reports the following. With `band = cell · sqrt(3) + 2 · error`:

- **A priori, for every point of the cavity:** it lies in `I` with `d ≥ t − band`.
  - The piecewise-linear field is positive at a point only if its tetrahedron has a vertex with
    `d > t`. The tetrahedron diameter is at most `cell · sqrt(3)` and `d` is 1-Lipschitz.
  - Cells that contain cavity surface lie in one-sided inside leaves, and fully positive blocks
    are inside by their parity query.
- **A priori, completeness:** every point of `I` with `d > t + band` lies in the cavity, because
  all vertices of its tetrahedron are positive.
- **`surface_distance_mm`** is an a posteriori enclosure of `d` over the whole emitted surface:
  - start from exact vertex distances, widened per triangle by `min(circumradius, longest edge)`
    (every point of a triangle is that close to one of its vertices);
  - then widen by `error`.
  - Its lower end is the certified minimum wall of the represented shell and must be positive.
  - *Superseded in part by TD-093 (M6.4e h1):* the lower end is now proven per cavity face, by
    `Bvh::clearance` and, for a face that fails it, `Bvh::lower_bound`. The widened enclosure is
    kept only as a filter that skips faces already proven, and as the upper end. The refusal
    "not certified to be separated" fires only when a face meets the source.
- **`vertex_distance_mm`** is the observed range at cavity vertices (diagnostic). On flat and
  smooth walls it is within about 10⁻³ mm of `t`.
- **Wall guarantee (TD-093, M6.4e h2):** with `HollowSpec::guarantee`, the cavity is extracted at a
  raised offset and re-extracted until the proven lower end reaches `t − 2 · error`;
  `offset_mm` and `passes` report it. The cell then follows TD-093's rule instead of `t / 8`.
- **Volumes:** binary64 divergence sums for the source and cavity, and the surface component count.

The certificate concerns represented geometry. Physical wall strength, drainage, cure behaviour and
printability are outside its scope.

## Limits, errors and progress

`SampledHollowLimits` defaults:

| Limit | Default |
| --- | --- |
| Source vertices | 3,000,000 |
| Source triangles | 4,000,000 |
| Padded grid cells | 2^36 |
| Distance queries | 2·10^9 |
| Cavity triangles | 2·10^7 |
| Requested payload (BVH, fragments, merged cavity; allocator overhead excluded) | 4 GiB |
| Threads | 0 (available parallelism) |

Errors reuse `HollowError` and publish nothing partial:
- `InvalidInput`: parameters, coarse cell.
- `Unsupported`: topology, range, query budget, "nothing to hollow".
- `Resource`: limits, allocation.
- `Uncertain`: on-surface query, failed post-validation, unseparated cavity.
- `Mesh`, `Topology`, and `Cancelled`, checked at `HollowStart`, `HollowWork`,
  `HollowCertificate` and `HollowPublish`.

Progress stages are Source, Field (with completed and total items), Extraction, Certification,
Assembly and Published.

## Validation and measured behaviour

**Tests:**
- `tests/sampled_hollowing.rs` covers:
  - analytic cube (cavity side 16 from 20 at `t = 2`, volume error < 1%);
  - a non-convex torus (tube radius `r − t`, < 3% including tessellation);
  - sphere convergence;
  - a preserved inner void (0.2% of the rounded-offset analytic volume);
  - thread-count determinism;
  - "nothing to hollow";
  - open sources and coarse cells;
  - limits and every cancellation checkpoint;
  - stage order.
- Unit tests cover the BVH against brute force, exact parity at projected vertices and edges with
  nested shells, closed-form distances and degenerate triangles, and Kuhn tetrahedra tiling.
- Every sign pattern is shown to give a closed, consistently oriented patch.
- CLI tests check sliced mask volume and certificate output.

**Release measurements**, Linux/WSL with 16 threads, for the 327,680-triangle FX-11 sphere
(R = 30 mm) at `t = 2`. The analytic cavity is about 91,952 mm³ (the tessellation shrinks it
slightly):

| Cell | Total time (16 threads) | Cavity triangles | Cavity volume | Vertex wall range |
| --- | --- | --- | --- | --- |
| 0.4 mm | 0.7 s | 0.55 M | 91,940 mm³ | [1.9999999, 2.0023] mm |
| 0.2 mm | 1.9 s (13.0 s on 1 thread) | 2.2 M | 91,947 mm³ | [1.9999999, 2.0009] mm |
| 0.1 mm | 7.2 s | 8.8 M | 91,948 mm³ | [1.9999999, 2.0004] mm |

Native Windows performance has not been measured.

**Comparison with commercial defaults** [I, from the research above]:
- Lychee: about 2.3 mm cells on a 100 mm model.
- CHITUBOX: 0.3 mm cells and voxel-quantized walls.

This backend reaches 0.1–0.25 mm cells in seconds and reports its wall error instead of hiding it.

## Known limitations

*TD-066 adds an opt-in certified simplification of the cavity
([specification](HOLLOW-SIMPLIFY-DRAIN-SPEC.md)), which addresses the mesh-size limitation below.*

- **Mesh size:** marching tetrahedra emit many triangles (about 2.2 M for a 60 mm sphere at
  0.2 mm). Certified simplification is future work. The face Z index (TD-046) keeps slicing such
  shells fast.
- **A posteriori bound:** it is conservative (triangle widening); actual vertex deviations are
  orders of magnitude smaller.
- **Not yet supported:**
  - supports and pocket policies (minimum volume, keep largest) on sampled cavities (drainage
    cuts are M5.5-G);
  - "outer" and "bottom-through" modes;
  - infill.
