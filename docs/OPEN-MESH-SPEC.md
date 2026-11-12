# Defective meshes: open, non-manifold and inconsistently oriented sources (specification)

**Status:** approved (TD-054) with the recommended defaults: diagnosis always on, section closure
on with a 2 mm bound, and `duplicates`/`orientation` repairs on by default. **Phases 1
(diagnosis and section closure), 2 (duplicate and orientation repairs) and 3 (winding-rule
hollowing and hole filling, approved into the MVP as D2) are implemented.**

## Implementation status (phase 1)

- **Diagnosis:**
  - `TriangleMesh::defect_report_with_cancellation` returns `mesh::topology::DefectReport`, with
    rim details capped at `MAX_RIM_DETAILS`.
  - Its edge counts equal the full topology report's on the corpus and on random meshes.
  - Inside-out (nesting) analysis belongs to phase 2.
- **Rims:** chained from one sorted array of directed uses.
  - At boundary pinches, the face-fan walk crosses consistently wound two-face edges only; elsewhere
    the rim ends as a chain.
  - A Möbius strip's boundary reverses direction at the orientation seam, so it reports two chains.
- **Closure:**
  - Configured by `slicing::winding::SectionClosure`, through `ParallelRaster::closure`.
  - `LayerKernel::with_closure` builds the rim table once.
  - The per-layer result is `WindingLayer::closure()` / `RasterImage::closure()`
    (`ClosureReport`: chords, longest chord, ambiguous rims).
  - Failures: `LayerFailure::OpenSectionGap` (chord above the bound), `Unbalanced` (imbalance off
    the rims, or leftover rim crossings) and `InvalidGapBound`.
- **CLI:**
  - `--close-holes <mm>` (default 2) and `--no-close-holes`.
  - The summary prints `Mesh defects: ...` and `Closed sections: ...`.
  - Union admission accepts boundary edges while closure is active, and still refuses winding
    conflicts (phase 2).
- **Validation:**
  - `tests/open_meshes.rs`: corpus diagnosis values; edge counts against the full report;
    star prisms missing side faces closing to the complete prism byte for byte at 1 and
    16 samples per pixel; a U-shaped hole choosing the shorter matching; the gap-bound failure;
    a non-orientable Möbius strip.
  - CLI tests: an open-top box admitted; two missing walls refused at 2 mm and identical to the
    complete box at 12 mm; `--no-close-holes` refusal naming the option.
  - Defect-free outputs (FX-11 drained, FX-12) are unchanged.

## Implementation status (phase 2)

- **API:** `TriangleMesh::repair_with_cancellation(RepairPolicy, limits, cancel)` returns the
  derived mesh (`None` when nothing changed) and a `mesh::repair::RepairReport`.
- **Duplicates (refines 5.1):** each group of faces with equal corner positions collapses to its
  net orientation. One face survives (the earliest with that orientation), or none when the
  orientations cancel. A stacked group (net count 2 or more) keeps its net count where
  collapsing it would unbalance an edge that was balanced, as a face two overlapping bodies share
  on the same side (TD-092 r9).
  - The winding number is unchanged wherever it was defined.
  - Opposite pairs (coincident double walls between touching bodies) disappear instead of being
    kept, which makes such meshes edge-manifold. FX-12 becomes one closed manifold and can now be
    hollowed.
- **Orientation (5.2):** union-find with parity over two-face edges.
  - The larger-area class is kept; on an exact tie, the class of the smallest face index.
  - Non-orientable components are left unchanged and counted.
- **Outward (refines 5.2):** only closed components whose lexicographically smallest vertex lies
  outside the bounding box of every other closed component are flipped (when their signed volume
  is negative beyond a forward error bound).
  - Components inside another's bounds turn with the outermost components around them when all
    of those turn, so an inside-out model keeps its cavities (TD-092 r9); otherwise they are left
    unchanged, and reversed ones are counted as nested, because they are usually cavities.
  - Exact nesting and overlap tests remain future work.
- **CLI:**
  - `--repair none|auto|<list>`, default `auto` (`duplicates,orientation`); `outward` is opt-in.
  - The summary prints `Repairs` and `Remaining defects`.
  - Admission uses the repaired mesh's defects.
  - A negative-winding failure names `--repair outward`.
  - Hollowing and slicing use the repaired mesh, which is a deterministic function of the bound
    source (phase 3 records the repair in the preparation plan).
- **Validation:**
  - `tests/mesh_repair.rs`: clean meshes untouched; FX-09 restored by one flip; the FX-08
    duplicate dropped; net-orientation groups and glued boxes; 200 random minority flips undone
    exactly; an inside-out box turned outward and the FX-04 cavity kept.
  - `tests/large_models.rs`: FX-12 repairs to one closed manifold.
  - CLI tests: a reversed face repaired to the clean box byte for byte; refusal with
    `--repair none`; an inside-out box failing with the hint and repaired with `outward`.

## Implementation status (phase 3)

- **Winding-rule hollowing (5.5):**
  - The field classifies a query point by its exact winding number along a vertical ray
    (`Bvh::vertical_winding`, signed by each stabbed face's exact orientation). Inside means a
    winding of exactly 1.
  - A winding of 2 or more (overlapping bodies) and a negative winding (an inside-out body or
    an unenclosed reversed shell) stop hollowing with an `Unsupported` error. This replaces the
    positive rule of 5.5: the wall distance is measured to every face, including faces inside
    an overlap, so a cavity in an overlap would leave internal walls. The refusal is sound
    because the winding is constant on each queried ball (the field already certifies that no
    face passes through it).
  - A query point on a stabbed face is reported as uncertain, never guessed.
  - Admission uses the lean defect report: no boundary edges, winding conflicts, unbalanced
    non-manifold edges or degenerate faces. Touching bodies (balanced non-manifold edges) are
    admitted and hollowed as one body.
- **Source repair in the plan (5.4):**
  - `OperationKind::Repair(preparation::repair::RepairSpec)` must be the plan's first operation.
    Its canonical encoding is tag 3, one flag byte (duplicates 1, orientation 2, outward 4, fill
    8, allow unfilled 16) and `max_hole_edges` (0 without filling); `max_hole_edges` must be at
    least 3.
  - `preparation::repair::repair_source` applies, in order: duplicates and orientation
    (`mesh::repair`), hole filling, then `outward`. The result is idempotent, so the plan
    reproduces it from the bound source. Hollowing and drainage apply the plan's repair before
    admission.
  - `RepairError::UnfilledHoles` reports the first refused hole unless `allow_unfilled` is set.
- **Hole filling (5.4):** `FillHoles { max_hole_edges }` (default 256), `FillReport` and
  `HoleRefusal`.
  - Weight: the largest fold (one minus the cosine between unit normals) between a cap triangle
    and its neighbours (the rim's mesh faces and the other cap triangles), then the total area.
    Ties keep the smallest split vertex.
  - Refusals: `OpenChain`, `TooLarge`, `NotSimple`, `Degenerate`, `ExistingEdge` (an internal cap
    edge already exists in the mesh) and `Intersects`.
  - The intersection check rebuilds a BVH over the source and the caps and looks for improper
    contacts involving a cap triangle. It removes the owning cap and repeats until no contact
    remains, so the accepted caps are exactly validated together.
- **CLI:**
  - `--repair` accepts `fill-holes` (opt-in).
  - For slicing, refused holes are reported and left to section closure.
  - For hollowing, the repair is recorded in the plan, refused holes fail the command, and an
    open model without `fill-holes` fails with a hint naming it.
  - The summary prints the holes filled (cap triangles, area, largest rim) and the holes left
    open.
- **Validation:**
  - `tests/hole_filling.rs`:
    - missing box faces are capped into the closed box (volume, idempotence);
    - a 48-gon hole gets a flat 46-triangle cap with no new vertex;
    - `TooLarge`, `OpenChain` (FX-10B) and `Intersects` (a body piercing the hole's plane) are
      refused;
    - FX-10A is hollowed through a plan repair (16 mm cavity);
    - a misplaced repair operation is rejected.
  - `tests/sampled_hollowing.rs`: touching bodies give two cavities with full walls; overlapping
    and inverted bodies are refused.
  - CLI tests: an open box filled is byte-identical to the closed box; hollowing an open box
    without `fill-holes` fails with the hint; with it, the output is identical to the closed
    box's.
  - Defect-free outputs (FX-11 drained, FX-12) are unchanged.

## 1. Purpose and principles

Downloaded models are often not closed, consistently oriented, edge-manifold surfaces. Thingi10K
measures:
- 11 % open;
- 22 % non-manifold;
- 10 % non-oriented;
- 45 % self-intersecting or overlapping (the last class is already handled by the winding union,
  TD-050).

Today Karasu refuses the others at admission or fails the first layer that crosses a defect. This
specification makes them sliceable under explicit policies, without silent repair. It keeps the
commitments of AGENTS.md:
- **The source is immutable.** Policies either act on section geometry only (nothing about the
  mesh changes) or build a **derived** mesh recorded as an explicit operation. The imported mesh is
  never modified.
- **Exact and index-based where possible.** Defects are defined on vertex indices and exact
  coordinates. The only tolerance is the explicit gap bound of section closure, given in
  millimetres.
- **Every change is reported** with counts, extremes and locations, in the CLI summary and through
  the core APIs. Anything a policy cannot resolve fails with the offending elements.
- **Deterministic:** identical output for every thread count; stable tie-breaking by indices.
- **Bounded:** fallible reservations, explicit limits, cooperative cancellation, and memory linear
  in the mesh with a few words per triangle (TD-053).

The policies apply to winding-union slicing (`LayerFill::Winding`). The strict pipeline keeps
refusing defects.

## 2. Baseline (before this specification)

| Defect | Import | Union admission (CLI) | Union layer | Hollowing |
| --- | --- | --- | --- | --- |
| Degenerate face | kept, diagnosed | excluded from counts, admitted | harmless | refused |
| Duplicate face, same orientation | kept, diagnosed | non-manifold edges, admitted | **fails** where crossed (unbalanced) | refused |
| Duplicate pair, opposite orientation | kept, diagnosed | non-manifold edges, admitted | balanced, contributes nothing | refused |
| Hole (boundary edges) | kept | **refused** | **fails** where crossed | refused |
| Non-manifold edge from touching bodies | kept | admitted | balanced, correct | refused |
| Non-manifold edge from an extra face (fin) | kept | admitted | **fails** where crossed | refused |
| Inconsistent faces in a component | kept | **refused** (winding conflicts) | **fails** where crossed | refused |
| Inside-out closed component | kept | admitted | **fails** (negative winding) unless nested as a cavity | parity treats it like any shell |
| Overlap / self-intersection | kept | admitted | correct union | parity: overlaps become **outside** |

## 3. Definitions

All definitions use the imported vertex indices; a face is *degenerate* under the existing
`DegenerateTriangle` rule and is ignored by every count below.

- **Directed use:** a face's edge `(a → b)` in its cyclic order.
- **Edge:** the unordered pair `{a, b}`. It is a *boundary edge* with one use, *two-face* with two,
  *non-manifold* with three or more.
- **Balanced edge:** equal numbers of uses in each direction.
- **Orientation conflict:** a two-face edge whose uses have the same direction.
- **Duplicate faces:** faces with the same vertex set.
  - A *same-orientation duplicate* is a later face with the same cyclic order as an earlier one.
  - An *opposite pair* is a face and a later face with the reversed order.
- **Rim:** a maximal chain of boundary half-edges, where each boundary edge is directed as its
  only use.
  - At a vertex with exactly one outgoing boundary half-edge, the chain continues there.
  - At a vertex with several (a pinch on the boundary), the successor of an incoming boundary
    half-edge `(u → v)` is found by walking the face fan around `v` from the face of `(u → v)`
    across two-face edges until a face with an outgoing boundary half-edge `(v → w)` is reached.
    The walk is the standard half-edge boundary traversal.
  - A rim is a **loop** when it returns to its first half-edge, otherwise a **chain** (it ends at a
    non-manifold edge).
  - Every boundary half-edge belongs to exactly one rim and has a position (index) along it.
- **Manifold component:** faces connected through two-face edges. Non-manifold edges do not connect
  components.
  - A component is **closed** when every edge of its faces, counted within the component, is
    balanced.
- **Nesting depth of a closed component C:** the number of other closed components containing a
  reference vertex of C. The reference vertex is C's lexicographically smallest `(x, y, z)` vertex,
  and containment is exact ray parity with simulation of simplicity (the method of
  `preparation::sampled`).
  - C *overlaps* another component when their surfaces intersect: an exact triangle-triangle
    contact over a BVH, excluding shared vertices.

## 4. Diagnosis (always on)

`TriangleMesh::defect_report_with_cancellation(limits, cancel) -> DefectReport`, a lean analysis in
the style of `edge_summary` (TD-053):
- **Memory:** sorted directed uses (8 bytes each) and per-face union-find with parity (8 bytes per
  face); about 40 bytes per triangle in total.
- **Counts:** degenerate faces; same-orientation duplicates; opposite pairs; edges by class;
  balanced and unbalanced non-manifold edges; orientation conflicts; rims (loops and chains, total
  and largest boundary length in mm, largest boundary edge count); manifold components; closed
  components; non-orientable components; and, when requested, inside-out closed components (needs
  nesting, so it is not computed by default).
- **Details:** at most 16 per class (edges as vertex pairs, rims as first vertex and length,
  components as smallest face index), plus the total for each class.
- **CLI:** prints a one-line summary per non-zero class before slicing (stdout summary, not stderr
  progress), and the policy each class falls under.

## 5. Policies

Policies are independent and composable. Each lists what it changes, its algorithm, what it reports
and how it fails.

### 5.1 `duplicates`: drop same-orientation duplicates (derived mesh)

- **Changes:** a derived face list without later same-orientation duplicates of a face, keeping the
  first by index. Opposite pairs are kept: under the winding rule they contribute exactly nothing
  (balanced, zero net winding), and dropping them would change nothing material.
- **Algorithm:** sort keys `(canonical sorted triple, rotation-normalized cyclic order, face index)`
  using the canonical vertex identities of TD-053, so equal-position vertices at different indices
  also match. Within a group, drop every face whose normalized cyclic order equals an earlier one.
  The cost is O(F log F) with 24 bytes per face.
- **Reports:** dropped faces (count and the first 16 indices).
- **Fails:** never; it only removes exact copies.

### 5.2 `orientation`: consistent orientation per manifold component (derived mesh)

- **Changes:** flips faces so that every two-face edge has opposite uses, within each manifold
  component, keeping the larger-area orientation class.
- **Algorithm:**
  1. **Constraints:** union-find with parity over faces. Every two-face edge requires its faces to
     have the *same* orientation class if their uses are opposite, else *opposite* classes.
     Non-manifold and boundary edges add no constraint.
  2. **Non-orientable components:** a parity contradiction makes the component non-orientable (a
     Möbius-like strip). The policy fails for that component, reporting the edge closing the
     contradiction; the other components proceed.
  3. **Class choice:** in each component, the class containing more original face area (f64 sum of
     face areas) keeps its orientation, and the other is flipped. On an exact area tie, the class of
     the smallest face index keeps it.
- **Inside-out closed components (sub-policy `orientation=outward`):** after step 3, every closed
  component that overlaps no other component gets the sign required by its nesting depth: outward
  (positive signed volume) at even depth, inward at odd depth (cavities).
  - The signed volume uses a compensated f64 sum with a forward error bound.
  - When |V| does not exceed the bound, the component is reported as ambiguous and left unchanged.
  - Overlapping components are required outward.
- **Reports:** flipped faces (count, area, number of components), components flipped whole by
  `outward`, ambiguous components, non-orientable components.
- **Fails:** only for non-orientable components (an error unless `--allow-topology-defects`, which
  leaves them unchanged and lets their layers fail where crossed).

### 5.3 `close-sections`: chord closure of open sections at holes (slicing only)

The central open-mesh policy. It changes no mesh. In each winding layer it closes every open
section chain at a hole with a straight chord across the hole, paired by the hole's rim.

**Precomputation (once per job, in `LayerKernel::build`):**
- extract the rims (section 3);
- build a table, sorted by edge key, of `(boundary edge key, rim id, position)`: 16 bytes per
  boundary edge;
- cost O(B log B) for B boundary edges, with cancellation.

**Per layer (in `winding::segments`, after the balance grouping):**
1. **Crossing points:** every unbalanced crossing point whose edges are all boundary edges is a
   *rim crossing*. It is an *end* (excess −1: an open chain arrives) or a *start* (excess +1: one
   leaves).
   - Vertex-avoiding planes guarantee that each lies strictly inside one boundary edge.
   - An unbalanced point on a non-boundary edge (same-orientation duplicates, unbalanced fins on
     non-manifold edges) is not a hole and keeps today's `Unbalanced` failure; section 5.1 removes
     the duplicate case.
2. **Grouping:** group the rim crossings by rim and order each group by position along the rim.
   - **Lemma:** a boundary half-edge directed upward (from below the plane to above) yields an end,
     and one directed downward yields a start. This follows from the combinatorial segment direction
     of TD-050 for both cases of the face's lone vertex.
   - So along a rim loop, ends and starts alternate. Along a rim chain they also alternate, and the
     unmatched first and last crossings are possible only at chain ends.
3. **Pairing:**
   - **Loop with one end and one start** (the common small hole): connect `end → start`.
   - **Loop with k > 1 pairs:** the two alternating cyclic matchings are both non-crossing along the
     rim:
     - (a) each end to the next start along the rim;
     - (b) each end to the previous start.

     Choose the one with the smaller total chord length; tie → (a). Count the rim as *ambiguous*.
   - **Chain:** pair each end with the next start along the chain. A leftover crossing fails the
     layer.
4. **Chords:** add each chord as a directed segment `end point → start point` (exact crossing
   points, world XY).
   - Chord length (Euclidean, mm) must not exceed `max_gap_mm`; otherwise the layer fails, reporting
     the rim, the chord's endpoints and its length.
   - Afterwards the layer must balance exactly. A residual imbalance is an internal failure reported
     as `Unbalanced`.
5. **Raster:** unchanged. Chords are ordinary segments, and the positive rule applies.

- **Meaning:** for a hole whose rim crosses the plane twice, the result is exactly the slice's own
  2D winding number thresholded at 0.5 (research OM-003). It is the section of a cap spanning the
  hole whose intersection with every plane is the chord between the rim crossings. Different holes
  are never joined; unrelated open chains are never bridged by proximity.
- **Reports:**
  - per layer: chords and the longest chord;
  - per job: layers with closures, chords, the longest chord with its layer and position, rims
    involved, ambiguous rims;
  - with antialiasing or not, the masks follow from the closed sections.
- **Fails:** chord longer than `max_gap_mm`, a leftover rim crossing, or imbalance off the rims. The
  failure lists the first 16 items with totals, as `LayerFailure::Unbalanced` does.
- **Sheets and surfaces** (models that are not solids): their rims are long, so the chords exceed
  any reasonable gap bound and the layer fails with that diagnosis rather than inventing a solid.

### 5.4 `fill-holes`: closed derived mesh for hollowing and supports (preparation operation)

Hollowing (TD-045/047) and future supports need a closed surface. This is a new
`OperationKind::Repair(RepairSpec)` in the preparation plan, before `Hollow`, bound to the source
digest like every operation. It is never applied by slicing alone.

- **Changes:** for every rim *loop* with at most `max_hole_edges` (default 256) boundary edges, add
  a triangulated cap.
- **Algorithm:**
  1. **Admission:** the loop must be simple (no repeated vertex). Otherwise the hole fails with its
     rim.
  2. **Triangulation:** minimum-weight triangulation by dynamic programming over the loop's vertex
     sequence (Liepa 2003), O(n³) time and O(n²) memory. The weight is lexicographic: first the
     largest dihedral angle with the neighbouring faces and with the other cap triangles (cosine
     computed in f64), then total area. Tie-breaking is by vertex positions in the loop. No new
     vertex is added, so no refinement or fairing happens.
  3. **Orientation:** cap triangles are oriented opposite to the rim's boundary half-edges, so every
     former boundary edge becomes a balanced two-face edge.
  4. **Validation:**
     - the result is closed and edge-manifold around the cap, with no degenerate cap triangle
       (existing rule);
     - no cap triangle intersects the mesh or another cap triangle except at shared rim vertices
       (exact triangle-triangle contact over the existing BVH).
     A failing hole is reported and left open, and the operation fails unless
     `allow_unfilled = true`.
- **Reports:** holes filled (count, cap area, largest loop), holes refused (reason, rim) and the
  caps' share of total area.
- **Fails:** refused holes, unless allowed. Rim chains (holes bounded by non-manifold edges) are
  always refused, because no cap is well-defined there.

### 5.5 Hollowing with the winding rule (prerequisite for overlapping or repaired sources)

The sampled hollowing classifies inside points by exact ray **parity**. For overlapping shells,
parity marks the overlap as outside, while the slicer's union fills it.
- **Change:** classify with the exact **signed** ray count (the winding number) under the positive
  rule, keeping simulation of simplicity.
- **Admission:** replace the closed edge-manifold requirement by:
  - balanced edges (every edge has equal uses in both directions, which admits touching bodies and
    overlaps);
  - no degenerate faces;
  - positive winding everywhere (no negative region).
- **Distances:** wall distance remains the exact distance to all faces, including inner overlap
  faces. The cavity surface is still certified closed. This is a separate work item with its own
  tests against the existing certificate.

## 6. Interfaces

**Core (`karasu_core`):**
- `mesh::topology::DefectReport` and `TriangleMesh::defect_report_with_cancellation` (section 4).
- `mesh::repair` (new module):
  - `resolve_duplicates(&TriangleMesh, cancel) -> Result<Repaired, RepairError>`;
  - `orient(&TriangleMesh, OrientPolicy { outward: bool }, cancel) -> Result<Repaired, RepairError>`.
  - `Repaired { mesh: TriangleMesh, report: RepairReport }` holds the derived mesh (vertices shared
    by index; a new face buffer only).
- `slicing::winding::SectionClosure { max_gap_mm: f64 }`:
  - added to `LayerKernel` / `ParallelRaster` as `closure: Option<SectionClosure>`;
  - `WindingLayer::closure() -> ClosureReport` (chords, longest chord);
  - new `LayerFailure` details for chords that are too long or leftover crossings.
- `preparation::OperationKind::Repair(RepairSpec { fill_holes: bool, max_hole_edges: u32,
  allow_unfilled: bool })` and its certificate (section 5.4).

**CLI (`slice`):**
- `--repair <set>`, with `none`, `auto` or a comma list of `duplicates`, `orientation`,
  `orientation=outward` and `fill-holes` (the last requires `--hollow`). The default is decision D1.
- `--close-holes <max gap mm>` / `--no-close-holes`. The default is decision D1; the suggested value
  is 2 mm (PrusaSlicer's bound).
- **Admission (union):**
  - boundary edges are admitted when closure is active;
  - orientation conflicts are admitted when `orientation` repair is active;
  - otherwise today's refusal applies, now naming the policy that would admit the model.
- **Summary:** the diagnosis lines, plus one line per applied policy:
  - `Repaired: N duplicate faces dropped`;
  - `Repaired: N faces flipped in M components`;
  - `Closed sections: L layers, C chords, longest X mm at layer K`;
  - `Filled holes: H`.

## 7. Limits, determinism and cost

- **Memory:**
  - diagnosis: about 40 bytes per triangle;
  - repairs: one derived face buffer (12 bytes per face) plus their sort keys;
  - closure table: 16 bytes per boundary edge;
  - per-layer closure scratch: proportional to that layer's rim crossings.
- **Time:** O(F log F) per mesh-level policy. Closure adds a binary search per rim crossing.
  `fill-holes` costs O(n³) per hole, bounded by `max_hole_edges`.
- **Determinism:** no hashing order, no thread-dependent reduction; ties are broken by indices.
  Parallel layers stay byte-identical (closure is a pure per-layer function of the immutable rim
  table).
- **Cancellation:** at every sort and sweep, as in TD-053.
- **Limits:** `RepairLimits { max_faces, max_boundary_edges, max_hole_edges, max_details }`, checked
  before allocation.

## 8. Validation

| ID | Fixture | Expectation |
| --- | --- | --- |
| V1 | FX-10A (cube without its top; the rim lies at the top height) | Admitted with closure active; no plane crosses the rim, so no chord is added and masks equal FX-01. A box with a 4 mm square side window (new): chords of 4 mm on the window's layers; with `--close-holes 2` those layers fail naming the rim and the 4 mm chord |
| V2 | Cylinder with a 1 mm side hole (new) | Chords ≤ 1 mm on the hole's layers; masks equal the closed cylinder's except inside the hole's own 1 mm chord, compared with the 2D winding-number oracle (0.5) |
| V3 | Sphere with several holes, one wavy rim crossing a plane 4 times (new) | Per-rim pairing; ambiguous rim reported; no chord joins two different holes |
| V4 | Open sheet or surface model (new) | Chord exceeds the bound; clear failure |
| V5 | FX-09 (one reversed face) | `orientation` flips exactly that face; masks equal FX-01 |
| V6 | Inside-out cube; nested cube-in-cavity with a reversed inner body (new) | `outward` flips by nesting; cavities stay inward; masks equal the reference |
| V7 | Möbius strip (new) | Non-orientable reported with the contradiction edge |
| V8 | FX-08 (duplicate face) and same/opposite duplicates (new) | `duplicates` drops same-orientation copies; opposite pairs unchanged; layers balance |
| V9 | FX-10B (extra fin face) | Reported as one rim chain plus an unbalanced non-manifold edge. Layers where the fin crosses only its boundary edges close into zero area; layers where it crosses its non-manifold edge fail as unbalanced (not a hole) |
| V10 | FX-12 | Diagnosis matches inspection (two duplicates, four non-manifold edges); slicing unchanged |
| V11 | `fill-holes` on V1/V2 | Caps validated, closed manifold; hollowing succeeds; refused hole (self-intersecting cap) reported |
| V12 | Overlapping hollowed bodies (section 5.5) | Winding-rule hollowing keeps the overlap solid; certificate holds |
| V13 | Randomized | Random closed bodies with random faces removed: closure output against the 2D winding-number oracle on single-crossing rims; repair results against brute-force orientation constraints |
| V14 | Determinism | 1, 2 and 8 threads give identical files; repeated runs identical |

Every policy keeps the existing outputs unchanged on defect-free inputs (FX-01–07, FX-11 and the CLI
corpus).

## 9. Phases

1. **Phase 1:** diagnosis (section 4) and `close-sections` (5.3), the open-mesh slicing path. It is
   the most valuable step and changes no mesh.
2. **Phase 2:** `duplicates` and `orientation` (5.1, 5.2), mesh-level and exact.
3. **Phase 3:** winding-rule hollowing (5.5), then `fill-holes` (5.4) for hollowing and supports on
   open sources.

Each phase is implemented, validated (section 8 subset) and documented on its own.

## 10. Decisions for the owner

- **D1, defaults.** Recommendation:
  - diagnosis always;
  - `--close-holes 2` on by default, since it is reported in the summary and bounded;
  - `--repair duplicates,orientation` on by default (exact, minimal, reported);
  - `outward` and `fill-holes` opt-in.

  The alternative keeps everything opt-in with the refusal naming the option. The commercial
  references close silently and without a bound (CHITUBOX) or warn but never block (Lychee).
- **D2, roadmap.** The roadmap lists "advanced mesh repair" as Post-MVP and M2 allows "only
  conservative explicitly requested repair". Phases 1–2 fit that rule as explicit policies;
  phase 3 needs approval to enter the MVP. **Decided (2026-10-18): phase 3 is in the MVP.**
- **D3, gap bound.** Is 2 mm the right default for MSLA, or should it scale with the model (for
  example a fraction of the bounding-box diagonal)? **Decided: a fixed 2 mm default.**
