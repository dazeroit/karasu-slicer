# Model export after a Boolean: how other tools write STL, OBJ and 3MF

**Date:** 2026-11-09. **For:** TD-139 (model export) and TD-140 (the mesh Boolean kernel).
**Method:** clean-room desk research. Only documentation, release notes, forum and issue prose,
specifications and papers were read. Each point is labelled *observation* (stated by the source),
*inference* or *hypothesis*.

## Why this was researched

karasu cuts drain holes into a model's mesh with its exact Boolean kernel (TD-140 phase 4). On
the owner's torso (2.3 million triangles, 13 holes):

| Precision | Crossing pairs | Collapsed triangles |
|---|---|---|
| Exact result, rounded to `f64` | 34 | 0 |
| The same, stored as `f32` (binary STL) | 5,156 | 382 |

About 95 % of the `f32` crossings lie where the sculpt's own overlapping bodies were united,
26 mm or more from any hole; about 226 lie on the holes' walls.

A prototype correction (2026-11-09) rounded the result to the kernel's grid, resolved the
crossings with the kernel itself, and moved the offending vertices to a grid coarsened in every
round. It cleaned a drilled sphere in one or two rounds. On the torso it diverged:
- on the 2⁻¹² mm grid, 55,800 → 63,484 → 34,126 → 51,350 crossings;
- on a 2⁻¹⁵ mm grid, 22,773 → 49,394 → 112,214;
- about 2 minutes per round.

## Findings

### Coordinate precision written

- *Observation.* lib3mf, the 3MF Consortium's reference library, writes positions as
  fixed-point decimals with a configurable number of digits after the point.
  https://lib3mf.readthedocs.io/en/latest/source/Cpp/lib3mf_Writer.html
- *Observation.* 3MF Core 1.4.0 §4.1.3.1: coordinates "can be recorded to arbitrary
  precision"; producers "SHOULD NOT use more precision than the error generated in their
  calculations, or the anticipated resolution of their consumer".
  https://github.com/3MFConsortium/spec_core
- *Observation.* An OpenSCAD mailing-list thread (2019) on rounding CGAL's exact rationals: it
  makes vertices coincide and creates self-intersections and degenerate triangles. STL cannot
  avoid this. 3MF keeps the topology when the vertex table is built before rounding, because
  manifoldness "applies to vertex IDs, not vertex values".
  https://lists.openscad.org/empathy/thread/3LCDMFFKLFM7VM5A6M3SIYDXYO3EH7IQ?page=3
- *Observation.* Manifold's documentation states that it guarantees manifold topology, not
  freedom from self-intersection.
  https://github.com/elalish/manifold/wiki/Manifold-Library
- *Not determined.* The number formats PrusaSlicer, OrcaSlicer, Bambu Studio and Cura use for
  3MF and OBJ: their documentation does not state them.

### Booleans in slicers, and what they export

- *Observation.* PrusaSlicer (SLA) drills drain holes into the hollowed mesh with OpenVDB and
  CGAL, during preview or slicing. If the input mesh is broken, the drilling "may refuse to
  proceed … in order to prevent program crashes or incorrect results". The hollowed and drilled
  model is exported with File › Export › Export plate as STL.
  https://help.prusa3d.com/article/hollowing_117285
- *Observation.* The rotation issue was not fixed. A maintainer wrote that "a nearly degenerate
  mesh degenerated during the rotation or scaling operation". A bot closed the issue.
  https://github.com/prusa3d/PrusaSlicer/issues/4063
- *Observation.* Exported hollowed models are reported as broken since 2.6.0: a duplicate outer
  shell, non-manifold, with duplicate faces. Lychee and Anycubic Workshop show broken output;
  Chitubox slices it. The reports are open, without a maintainer's answer.
  https://github.com/prusa3d/PrusaSlicer/issues/11460,
  https://github.com/prusa3d/PrusaSlicer/issues/13515
- *Observation.* Bambu Studio 1.7: mesh Booleans are based on MCUT and "may fail on some
  meshes". The implicit Boolean in "Export as STL" was disabled.
  https://github.com/bambulab/BambuStudio/releases/tag/v01.07.00.53
- *Inference.* No project documents a check for self-intersections that rounding makes on
  export. The responses found are "may fail", refusal, or issues left open.
- *Observation.* PrusaSlicer exports supports with the model (SLA, "Export Plate as STL/OBJ
  Including Supports"), every model of the plate in one STL.
  https://help.prusa3d.com/article/export_1771
- *Hypothesis.* The shells are concatenated, not united. Unconfirmed.

### What the 3MF specification requires

- *Observation, MUST.* Objects of type `model` and `solidsupport`: every edge shared by exactly
  two triangles (by vertex index), consistently oriented, facing out (§4.1). A triangle's three
  indices must be distinct (§4.1.4.1); a non-zero area is only a SHOULD.
- *Observation, SHOULD NOT.* "Producers SHOULD NOT produce self-intersections or overlapping
  objects"; "consumers MUST accept self-intersecting meshes and overlapping objects, but treat
  'model' and 'solidsupport' types as if filled with a Positive fill rule" (§4.1.1).
- *Observation.* Overlapping build items: devices "MUST unite the final geometry" (§3.4.3.1).
- *Inference.* A drilled mesh written by vertex index (closed, oriented, distinct indices) is a
  conforming 3MF `model` even with tiny crossings or collapsed triangles. They breach only a
  SHOULD, and a consumer must read them with the positive fill rule.

### How consumers treat small self-intersections

- *Observation.* PrusaSlicer slices by default with the positive fill rule ("Regular").
  https://help.prusa3d.com/article/slicing-mode_301501
- *Observation.* Cura has "Union overlapping volumes".
  https://support.ultimaker.com/articles/Knowledge/1667418045074
- *Observation.* Lychee's repair tool targets non-manifold edges and holes, not
  self-intersections. https://doc.mango3d.io/doc/resin-documentation/resin-layout/repair-tool/
- *Inference.* Crossings of nanometres inside one oriented closed shell print correctly
  wherever the positive rule (or a union) is used. How Chitubox and Lychee fill is not
  documented.

### Rounding exact results without crossings

Sources:
- the CGAL blog: https://www.cgal.org/2025/06/13/autorefine-and-snap/
- Valque and Lazard, "Resolving Self-intersections in 3D Meshes while Preserving Floating-point
  Coordinates", Computer Graphics Forum 44(5), SGP 2025: https://inria.hal.science/hal-04907149/document

Findings:
- *Observation.* The target is `double`.
- *Observation.* Only the vertices of intersecting triangles, with every vertex in the same
  cells, are snapped to the centres of a **fixed** grid. By default each axis's largest
  coordinate is scaled into [2²³, 2²⁴), the finest grid of exact floats.
- *Observation.* The new intersection points are rounded to doubles, at least 2²⁹ times finer
  than that grid. The paper argues that this gap makes flipped triangles unlikely; it is not a
  proof.
- *Observation.* CGAL defaults to 5 iterations and reports failure with a partial result.
- *Observation.* Every Thingi10K model was resolved but one. That model has about 5 million
  intersecting pairs. It failed on the default grid and was solved in one iteration (38 s) on
  a grid of [2¹³, 2¹⁴), a Hausdorff distance below 0.6 % of the model's size. Finer grids cost
  2 min, then 35 min, then more than an hour.
- *Observation.* "Rounding all coordinates to floats (after computing self-intersections)
  performs very poorly": 42 % of models failed, and 24 % still failed after an hour of
  iterations.
- *Observation.* Neither source treats `f32` output or STL.
- *Inference.* karasu's prototype departed from the method in three ways:
  1. it rounded every point the cut made to a grid as coarse as, or coarser than, `f32`;
  2. it coarsened the grid in every round instead of keeping it fixed;
  3. it moved only the offending vertices, not every vertex in their cells.

  Point 1 is what the paper reports performing very poorly. With an `f32` target, the 2²⁹ gap
  between the snap grid and the new points disappears.
- *Hypothesis.* The method might converge on the torso with all three of:
  - one fixed snap grid well coarser than `f32` (about 0.012 mm on a 200 mm model, the paper's
    scale for its hard case);
  - whole cells snapped;
  - new points stored as `f32`, keeping a gap of about 2¹⁰.

  That is still not a guarantee. It moves the offending vertices by up to about 6 µm, below a
  pixel, and is untested.

## Conclusions for karasu

1. *Inference.* None of the tools found guarantees an intersection-free `f32` output after a
   Boolean. The practice is closed, oriented topology with tolerated crossings, read with the
   positive fill rule, which 3MF makes normative.
2. *Inference.* karasu's drilled mesh is closed and oriented, and writing it by vertex index
   keeps it so whatever rounding does. It is a conforming 3MF; in STL, the format itself loses
   the topology.
3. *Inference.* Reporting the crossings in the file's precision, apart from refusal, is already
   more than the tools found do. Separating the crossings near the holes from those where the
   model's own bodies overlap tells the user which ones the cut made.
4. *Hypothesis.* A correction faithful to Valque and Lazard is a separate research step, with
   no promise of convergence on sculpts.

## Outcome (2026-11-09)

The owner chose a correction faithful to Valque and Lazard, tried on the torso:
- the kernel's grid refined to 2⁻¹⁵ mm, exact in `f32` within ±512 mm;
- made points rounded to it;
- a fixed snap grid of 15.6 µm, whole cells snapped;
- the mesh resolved by the kernel's self-union in each round.

The torso converged in 8 rounds (73.5 s in all) to no crossing and no flat triangle, vertices
moved at most 7.8 µm. With 7.8 µm cells, 5 rounds left 39 crossings. It was adopted as TD-140
phase 5. Remaining crossings, should rounds not suffice on another model, are reported.
