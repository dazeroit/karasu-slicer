# Raster-side wall verifier: implementation plan (TD-095)

**Status:** phase 1 done, 2026-10-23 (v1 to v4; results in [Progress](#progress)). Phase 2 is a
separate decision. Decision: [TD-095](TECHNOLOGY-DECISIONS.md). Origin: the
[hollowing design comparison](HOLLOW-CAVITY-DESIGN-COMPARISON.md), section 7, item 4, and
section 9, question 6.

## 1. Goal

Check what the printer will cure, independently of how the cavity was made. The check reads only
the layer masks, and it runs on demand after slicing. It reports:

- **The thinnest printed wall:**
  - its value;
  - where it is (layer and plate position);
  - how close it is to the requested wall.
- **Breaches:** places where the cavity meets the outside other than through a drain hole. This
  is the defect CHITUBOX left in a 0.1 mm wall without a message.
- **Undrained pockets:** cavity regions that no drain hole reaches, found on the voxels. This
  independently confirms the mesh-side "trapped resin" findings.
- **A map:** in the application, the thin spots, breaches and pockets are drawn on the sliced
  layers.

**Scope:**
- *Phase 1:* layers sliced by Karasu. They come with their cavity and drain holes, so a breach
  can be told from a hole.
- *Phase 2* (a separate decision): layer files from other slicers, starting with PP1. There, the
  holes are not known.

## 2. Definitions

**Voxels.** A voxel is one pixel of one layer: X and Y follow the panel pitch, Z the layer's
slab. Layer heights may vary (TD-064).

**Material.** A pixel is material when it covers at least half of its samples:
`gray ≥ min_gray + (255 − min_gray) / 2`. This is the rule the commercial comparisons used
(gray ≥ 128 at `min_gray = 0`). Binary masks are unchanged by it.

**Stacks**, all streamed in layer order. Each has a bounded window of layers in memory.

| Stack | Content | Source |
| --- | --- | --- |
| `M` | the exported masks | the slicing itself |
| `H` | the drain holes' cuts, alone | the same analytic rings that are subtracted from `M`, rasterized with no body |
| `K` | the expected cavity, alone | the cavity mesh rasterized with the same profile |

**Classification:**
- *Closed stack* `M ∪ H`: the print with every drain hole filled in.
- *Exterior* `E`: the empty voxels of the closed stack that are 6-connected to the outside of
  the stack's box.
- *Cavity* `C`: the other empty voxels.

**Wall.** At a cavity voxel `c`, the wall is the exact Euclidean distance from `c`'s centre to the
nearest exterior voxel centre, less one voxel step in that direction. That step is the thickness
of one voxel along the line, which the printed material lacks at both ends. Two bounds are
reported:
- the measured minimum;
- a guaranteed lower bound, which subtracts the voxel's half-diagonal on top.

A cavity voxel more than the requested wall away from every exterior voxel is not visited.

**Breach.** An expected-cavity voxel (`K`) that is exterior (`E`) in the closed stack.

**Pocket.** A connected component of the empty voxels of `M` itself (holes open) that does not
reach the outside: resin that cannot drain. Its volume and centroid are reported.

## 3. Algorithm

1. **Components (first pass).** A run-based union-find across layers.
   - Empty runs of each row are joined to the overlapping runs of the previous row and of the
     previous layer (6-connectivity).
   - Runs touching the box's border, the first layer or the last layer are joined to the
     outside.
   - This runs on the closed stack, to find `E` and `C`, and on `M`, to find pockets.
   - Memory follows the runs, not the voxels: about 10,000 runs per layer on `Base_01.stl`.
2. **In-plane distance.** For each layer, the exact squared distance from every pixel to the
   nearest exterior pixel of that layer:
   - a separable Euclidean distance transform, along rows and then columns (Felzenszwalb and
     Huttenlocher);
   - capped at the wall, on the anisotropic pitch;
   - run in parallel per row and per column.
3. **Across layers.** The 3D distance at a cavity voxel is the minimum of `dz² + d²xy`:
   - `d²xy` is the in-plane squared distance at the same pixel in a nearby layer;
   - `dz` is the height between the two layers;
   - only layers closer than the wall count. This is exact, because the transform is separable.
   - The window is `2 · ceil(wall / layer) + 1` layers of in-plane distances, cropped to the
     model's box.
   - Only cavity voxels next to material are evaluated: the minimum lies there.
4. **Breaches and the map.**
   - `K` is streamed alongside, and every `K ∩ E` voxel is a breach. Breach voxels are grouped
     into components, each with its extent.
   - Thin spots (wall under the requested value) are kept per layer as runs for the map, up to a
     bound, with counts beyond it.
5. **Determinism.** The result is independent of the thread count:
   - integer lattice;
   - fixed order;
   - sorted reports.

**Cost (estimate, `Base_01.stl` on the P1):**
- *Voxels:* about 9,722 × 1,325 × 2,097, cropped to the model's box. That is 27 billion, visited
  as runs in pass 1 and as pixels in pass 2.
- *Time:* tens of seconds on 16 cores.
- *Memory:* the window of in-plane distances is about 1.4 GB at 1.3 mm, or 0.4 GB with 32-bit
  values on tiles. The masks of `M` are already held by the application.

## 4. Phases

| Phase | Content | Acceptance |
| --- | --- | --- |
| **v1 Core** | The verifier in `karasu-core` | Equals a brute-force 3D check on small synthetic stacks |
| **v2 Pipeline and command line** | Masks, holes and cavity streamed to the verifier; summary lines; an option | Every comparison output unchanged without the option |
| **v3 Interface** | The result panel and the map | The verifier runs from the interface and shows its findings |
| **v4 Validation** | Measurements and native Windows | The campaign and documents |

**v1 Core** (`karasu-core`, a new `raster::verify` module):
- **Inputs:** layer masks with their planes, an optional hole stack and an optional
  expected-cavity stack, the profile and the requested wall.
- **Output:** a report with:
  - the walls (measured, guaranteed and where);
  - breaches (count, voxels, extents);
  - pockets (volume, centroid, extent);
  - per-layer runs for the map.
- **Limits and errors:**
  - the usual limits, with structured errors;
  - cancellation, checked by the caller's thread.
- **Tests** against brute force on synthetic stacks:
  - a box with a void, at walls of 1 to 10 voxels;
  - an oblique wall, and a horizontal wall that only the pass across layers sees;
  - variable layer heights;
  - a breach;
  - a drained void, an undrained one, and nested voids;
  - antialiased masks at the material threshold;
  - thread-count identity.

**v2 Pipeline and command line:**
- **Option:** `slice --verify-walls`, after slicing. It renders `H` and `K` and runs the verifier.
- **Summary:**
  - `Printed wall (mm)`: the measured value and the guaranteed bound;
  - `Thinnest printed wall at`: the layer and the position;
  - `Wall breaches`;
  - `Undrained pockets`.
- **Failure policy:** a breach, or a printed wall under the certified wall by more than the
  lattice bound, contradicts the certificate. The job still writes its file, so it can be
  inspected, but it exits with a nonzero status and the reason.
- **Unchanged:** without the option nothing changes, and the comparison suite stays
  byte-identical.

**v3 Interface:**
- **Running it:** a "Verify walls" button with the sliced result. It runs in the background,
  with progress, and can be cancelled.
- **Result:** a panel with the walls, breaches and pockets, each clickable to go to its layer.
- **Map:**
  - the layer viewer overlays the thin spots, breaches and pockets on the layer;
  - colours come from the theme;
  - the overlay is drawn on the GPU texture of the layer image, rebuilt only when the layer
    changes.

**v4 Validation:**
- **Measurements:**
  - `Base_01.stl` at 2.0, 1.3, 1.0, 0.5, 0.3 and 0.2 mm, with and without drain holes;
  - FX-11, FX-12 and the Yorick;
  - for each, time and memory, and the printed wall against the certificate.
- **Breach check:** a crafted breach, made by cutting a slit in the wall, is found.
- **Native Windows:** `validate.ps1 -IncludeApp`.
- **Documents:** ARCHITECTURE, TEST-STRATEGY, README and ROADMAP.

## Progress

- **v1 (2026-10-22), core:** `raster::verify` (`verify_walls`, `VerifyInput`, `WallReport`) with
  new cancellation checkpoints (`VerifyStart`, `VerifyComponents`, `VerifyDistances`).
  - *Design points:*
    - Empty runs are labelled by union-find across rows and layers.
    - The in-plane transform runs along rows by two sweeps, then down columns by a lower
      envelope, in parallel.
    - Distances are combined across layers in a moving window, only at cavity voxels next to
      material.
    - The empty space beyond the first and last layers counts as exterior.
  - *Tests:* the smallest cavity-to-exterior distance and the cavity count equal a brute-force
    3D search. The cases are:
    - a box with walls of 2 columns, 3 rows and 3 layers;
    - a floor on the first layer;
    - twelve random ellipsoidal or box shells with off-centre voids, pillars and varied layer
      heights.

    Also covered: a slit breaches the expected cavity while the same opening as a drain hole
    does not; a closed void is a pocket; the material threshold; invalid input.
  - *Measured:* an ellipsoidal shell at print scale (4,000 × 1,300 pixels × 1,000 layers on the
    P1's pitch, 5.2 billion voxels, 1.6 billion of them cavity) is verified in 7.0 s on 16
    threads. `Base_01.stl` (about 27 billion voxels) would take about 35 s.

- **v2 (2026-10-22), pipeline and command line:** `slice --verify-walls`
  (`SliceRequest::verify_walls`). The job keeps its exported masks and rasterizes:
  - the cavities alone, with the nonzero rule and no compensation (a plate unites its instances'
    cavities);
  - the drain holes' cut rings alone (`raster::rasterize_rings_with_cancellation`).

  The findings go to the summary and to `SliceOutcome::verification`. A breach, or a printed wall
  under the certified wall by more than one voxel diagonal, fails the job after its output is
  written.
  - *Found on `Base_01.stl`:* its repair leaves reversed slivers empty, and the verifier counted
    them as cavity, with a zero wall where one touched the outside diagonally. With an expected
    cavity, walls are now measured from it only; other enclosed voids are pockets.
  - *Speed:* the column pass now runs only over the rows within reach of a pixel that is read,
    and keeps integer offsets (the distance is computed where it is read). The print-scale shell
    takes 4.1 s instead of 7.0.
  - *Measured:* `Base_01.stl` at 1.3 mm on the comparison's 13,312 × 5,120 printer, without a
    hole:

    | | Total | Peak memory |
    | --- | --- | --- |
    | Sliced | 8.5 s | 0.71 GB |
    | Sliced and verified | 35.1 s | 1.42 GB |

    - Printed wall: 1.259 mm measured, at least 1.255 mm guaranteed, against 1.3042 mm
      certified, within one voxel (0.056 mm). The cavity's floor is quantized to the 0.05 mm
      layers.
    - Undrained pockets: 14 (the cavity and 13 empty slivers).
    - FX-11's cavity, closed, is one pocket of 91,429.61 mm³, against 91,429.60 mm³ on the mesh.

- **v3 (2026-10-22), interface:** the layer panel has a "Verify walls" button.
  - *What it runs:* the plate is sliced again with the verifier on. The file is the same,
    since slicing is deterministic, and the pipeline cache spares the preparation. The layer shown
    stays the same.
  - *What the panel shows:* the verdict, then the thinnest wall (measured and guaranteed),
    voxels under the requested wall, breaches and undrained pockets (the first eight listed).
    Each finding is a link to its layer.
  - *The map:* the layer image paints pockets (violet), thin walls (amber) and breaches (red),
    with a legend. The marks are baked into the layer's image when it is built, so a layer
    costs nothing more to draw.
  - *Messages:* "Walls verified", or "Wall verification failed" as an error with the reason.
  - *Owner's test:* the first link of a pocket led to its lowest voxel, where `Base_01.stl`'s
    cavity is a speck. At the layer of another, tiny pocket, the cavity's own violet hid it.
    - Each pocket is now shown where it is widest (`Pocket::widest`).
    - The pocket chosen in the panel is painted in the selection colour, a status cyan
      (`theme::SELECTION`). It was first white, the material's own colour, so the chosen
      cavity made its layer look solid.
    - Under a map, the material is dimmed to 45 % of its gray.
    - A chosen finding (wall, breach or pocket) gets a ring and a cross on its layer, so a
      finding of a few pixels is found at any reduction.

- **v4 (2026-10-23), validation.** `slice --verify-walls` against plain slicing, on the
  comparison's 13,312 × 5,120 printer (`printer-big.json`). Raw results are in
  `wall-verification/` in the external evidence.
  - *`Base_01.stl`* (the drain hole is a surface hole on the front face):

    | Wall | Certified | Printed (measured / guaranteed) | Sliced | Sliced and verified | Peak memory |
    | --- | --- | --- | --- | --- | --- |
    | 2.0 mm | 2.0035 | 1.958 / 1.955 | 5.7 s | 34.5 s | 0.56 → 1.38 GB |
    | 1.3 mm | 1.3042 | 1.259 / 1.255 | 9.3 s | 32.2 s | 0.73 → 1.42 GB |
    | 1.0 mm | 1.0039 | 0.961 / 0.955 | 10.2 s | 31.7 s | 0.87 → 1.48 GB |
    | 0.5 mm | 0.5053 | 0.461 / 0.459 | 13.0 s | 36.1 s | 1.56 → 1.70 GB |
    | 0.3 mm | 0.3038 | 0.260 / 0.255 | 30.1 s | 53.3 s | 3.44 → 3.45 GB |

    - Every printed wall is within one voxel diagonal (0.0555 mm) of the certified one, and
      there is no breach.
    - Without the hole, the cavity is one undrained pocket among the repair's empty slivers
      (11 to 14 pockets). With it, only the slivers remain: 0.0002 to 0.05 mm³ in all.
    - Verification costs 23 to 29 s on this model, roughly constant in the wall, as its cost
      follows the volume at print resolution.
  - *Fixtures, without holes:*

    | Model | Walls | Printed against certified | Sliced | Sliced and verified |
    | --- | --- | --- | --- | --- |
    | FX-11 | 2.0, 1.0, 0.5 mm | 0.050–0.054 mm under, one pocket each | 1.4–3.3 s | 10.9–14.0 s |
    | FX-12 | 2.0, 1.0, 0.5 mm | 0.042–0.045 mm under, 82–182 pockets | 8.4–9.8 s | 13.0–15.8 s |
    | The Yorick | 2.0, 1.0, 0.5 mm | 0.042–0.046 mm under, 480–532 pockets | 2.1–4.1 s | 3.3–3.7 s |

    FX-11's single pocket is its cavity, at the mesh's volume. FX-12's and the Yorick's are
    their cavities' many regions with the models' own enclosed voids. Every run passes.
  - *A real breach:* a 6 mm cube hollowed at 0.3 mm on 1 mm pixels has no pixel centre in its
    side walls, so its cavity reaches the outside. The job writes its file, reports the breach
    and fails with exit status 1 (`verify_walls_reports_the_printed_wall_breaches_and_pockets`).

  - *Native Windows* (`validate.ps1 -IncludeApp`): every gate passed, with 1,277 tests in
    Debug and in Release, and the application's smoke runs.

## 5. Phase 2 (separate decision)

Layer files from other slicers, starting with PP1, decoded independently from the format
research. Without their holes, every cavity voxel that reaches the outside counts as exterior.
- **What it still reports:**
  - the printed wall;
  - undrained pockets;
  - breaches as "the enclosed cavity shrinks against a solid slicing", the measure the commercial
    comparison used.
- **What it needs:** a model or a solid slicing of it, to tell a breach from a drain hole.

## 6. Risks

- **Volume at print resolution.** A 16K panel has 4 to 5 times more pixels for the same model.
  - The window and tiles bound the memory.
  - The time grows with the volume.
- **Antialiasing.** The half-coverage rule is a convention. A wall one pixel from the threshold
  may read one pixel thinner or thicker; the lattice bound covers it.
- **Thin features that are not walls** (hair, blades) are not cavity walls, so they are never
  measured. Only the distance from the cavity to the outside is measured.
