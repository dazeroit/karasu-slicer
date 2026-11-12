# Certified fast hollowing: implementation plan (M6.4e, TD-093)

**Status:** done, 2026-10-22 (h1 to h5; results in [Progress](#progress)). Decision: [TD-093](TECHNOLOGY-DECISIONS.md). Evidence and
alternatives: [HOLLOW-CAVITY-DESIGN-COMPARISON](HOLLOW-CAVITY-DESIGN-COMPARISON.md), sections 5.1,
5.2 and 9.

## 1. Goal

Hollow any wall the user asks for, quickly, with the requested wall guaranteed:
- **Guarantee:** a certified minimum wall that is always at least the requested wall, proven per
  cavity triangle.
- **Cell:** chosen before hollowing from the wall and the source's surface area. The cavity then
  grows as `1 / wall`, not `1 / wall²`.
- **Interfaces kept:** every consumer of the cavity mesh stays as it is: drain holes, regions,
  supports, display, caps, picking, cache.

**Targets** (measured by the experiment; the implementation must meet or beat them, on the same
host):

| Case | Target |
| --- | --- |
| `Base_01.stl`, 1.3 mm | certified ≥ 1.3 mm in one pass, about 7 s, under 0.5 GB, drain holes fit |
| `Base_01.stl`, 0.5 mm | certified ≥ 0.5 mm, about 12 s, under 1 GB |
| The 20-model campaign, 0.5–2 mm | every run certified in one pass, at most about 25 s and 1.4 GB |

## 2. The rule (TD-093)

| Quantity | Value |
| --- | --- |
| Accepted over-thickness `τ` | 0.05 mm |
| Triangle budget `N` | 10,000,000 cavity triangles |
| Cell | `max(√(τ · wall / 0.39), √(7.9 · A / N))`, below `(wall − 2 · error) / √3` |
| First offset | `wall + 0.40 · cell² / wall` |
| Shortfall correction | offset grows by the measured shortfall |
| Refinement | halve the cell after a refused pass, or after passes that do not converge |
| Bounds | a fixed number of passes, and the triangle budget |
| Stop | "nothing to hollow" ends the run at once |

`A` is the placed source's surface area: exact sum of triangle areas, scaled by the placement.
An explicit cell (`--hollow-cell`, plates' `hollow_cell_mm`) bypasses the cell rule; the offset
correction and the exact proof still apply.

## 3. Phases

| Phase | Content | Acceptance |
| --- | --- | --- |
| **h1 Exact certificate** | Per-triangle proof in the core; the loose lower bound and its refusal go | Certified minimum equals the brute-force minimum on small meshes; equals the vertex minimum within 1 µm on `Base_01.stl` |
| **h2 Rule and passes** | Cell from `τ`, `A`, `N`; predicted offset; fallback passes, inside the core | `Base_01.stl` 1.3 and 0.5 mm meet the targets; deterministic for any thread count |
| **h3 Pipeline and command line** | Plan record and cache, summary lines, refusals, comparison baseline | Unhollowed comparison cases byte-identical; hollowed ones changed as recorded |
| **h4 Interface** | Guaranteed wall and thick spots shown, warning under 1 mm, simplification removed, passes in progress | Every corpus model hollows from the interface; drain holes on `Base_01.stl` at 1.0 mm |
| **h5 Validation** | The 20-model campaign with the product code, fixtures, budgets, native Windows, documents | All targets of section 1 |

### h1 Exact certificate (core)

The sampled backend keeps its field, extraction, welding and validation. What changes:
- **Proof** (`certify`, `preparation/sampled.rs`):
  - every cavity triangle is proven farther than the vertex minimum less a small `δ`, with
    `Bvh::clearance` on the source index hollowing already built;
  - a triangle that fails gets its own `Bvh::lower_bound`;
  - the certified minimum is the smallest of these.
  - Triangles whose widened vertex bound already passes are skipped: today's enclosure as a fast
    filter.
- **Upper end:** the certified upper end stays the widened enclosure, which is conservative. The
  vertex maximum is reported as the thickest measured spot.
- **Refusal:** "cavity surface is not certified to be separated from the source" now depends only
  on the exact proof, i.e. a triangle that meets or touches the source.
- **Simplified faces** keep TD-066's proven wall floor.
- **Determinism:** fixed chunks and a minimum, whatever the thread count.
- **Tests:**
  - brute-force minimum over all source triangles on small meshes;
  - a sphere and a cube with known walls;
  - a model where the old certificate refused and the exact one accepts;
  - thread-count identity and cancellation inside the proof.

### h2 Rule and passes (core)

- **Request:** `HollowSpec` carries the requested wall. A new sampling mode carries `τ`, `N` and
  the coefficient, or the explicit cell. The canonical record encodes the mode and its parameters
  under a new backend identifier (`sampled-distance-tetra-v2`); `v1` records stay readable.
- **Passes:** the loop of section 2 runs inside `hollow_mesh_sampled`, so every caller gets the
  guarantee.
  - Progress reports each pass. Cancellation is checked between and inside passes.
  - Every pass is bounded by the existing limits.
- **Results:** the model reports the requested wall, the certified wall, the offset used, the
  cell, the passes and the thickest spot.
- **Refusals:**
  - *Budget:* when the triangle budget or the pass bound is reached, the refusal names the best
    certified wall and the cell that would be needed.
  - *Nothing to hollow:* stays as today.
- **Tests:**
  - the rule's arithmetic;
  - one pass on crafted cases;
  - a forced fallback (a coefficient set too low in a test) converging in two passes;
  - the budget refusal;
  - determinism.

### h3 Pipeline and command line (project, CLI)

- **Cell:** `pipeline::hollow_cell` and `DEFAULT_CELLS_PER_WALL` give way to the rule; the area
  comes from the placed source.
- **Cache:** the cavity cache key follows the new plan record. The hole probe, drilling and
  supports are unchanged.
- **Summary:**
  - *Kept:* `Hollow grid cell (mm)`.
  - *Changed:* `Certified wall range (mm)` now carries the exact minimum.
  - *New:* `Hollow wall guaranteed (mm)`, `Thickest wall at cavity vertices (mm)` and
    `Hollowing passes`.
  - *Removed:* `Measured wall at cavity vertices (mm)`, replaced by the two above.
- **Options:** `--hollow-simplify` stays; the help text describes the rule.
- **Comparison suite:**
  - *Unhollowed cases* stay byte-identical.
  - *Hollowed cases* (c02, c06, c10 and the plate cases) are compared before and after, and the
    differences are recorded under TD-093.
  - Then a new baseline is taken.

### h4 Interface (app)

- **Hollowing panel:**
  - shows "guaranteed wall X mm; up to Y mm thicker in places" and the passes;
  - warns under 1 mm;
  - drops the "Simplify the cavity" checkbox (the plan keeps simplification for plates and the
    command line).
- **Progress:** hollowing progress shows the pass.
- **Unchanged:** cavity display, caps and picking.
- **Acceptance:** the corpus test hollows every fixture from the interface. A test places a drain
  hole on `Base_01.stl` at 1.0 mm; it is local only, as the model is not in the repository.

### h5 Validation

- **The 20-model campaign** (`hollow-runtime/predicted/`) with the product code, at 2.0, 1.3,
  1.0 and 0.5 mm: passes, certified wall, thickest spot, triangles, time, memory, against the
  experiment.
- **Fixtures:** FX-11 at 2 mm (TD-066's reference), FX-12 and the Yorick.
- **Before and after:** today's backend on the same cases.
- **Documents:**
  - ARCHITECTURE, TEST-STRATEGY, README (command line), ROADMAP;
  - superseded notes in the M5.5-F specification and TD-045 (cell rule and certificate);
  - TD-093's results.
- **Native Windows:** `validate.ps1 -IncludeApp`.

## Progress

- **h1 (2026-10-22), exact certificate:** the lower end of `surface_distance_mm` is proven per
  cavity face. FX-11's 2 mm wall now certifies 1.999 mm instead of 1.571; `Base_01.stl` at 1.3 mm
  certifies 1.291 mm instead of 1.014.
  - *Brute force:* on an L-shaped prism at a coarse cell, where faces come 0.044 mm closer than
    their vertices, the certificate equals the brute-force minimum over every face pair.
  - *Command line:* only the "Certified wall range" line of the six hollowed comparison cases
    changes; every print file is identical (TD-093).
  - *Cost at today's `wall / 8` cell:* about 15 s for `Base_01.stl`'s 15 M cavity faces, and
    1.5 s more for FX-11. h2's cell brings the faces, and the cost, down.

- **h2 (2026-10-22), rule and passes:** `HollowSpec::guarantee` (`WallGuarantee`, its coefficient
  recorded under a new marker only when set) makes the sampled backend hollow at the predicted
  offset and certify; when the wall falls short it raises the offset (1.25 times the shortfall),
  and after a refused pass, or three short ones at one cell, it halves the cell.
  - *Limits:* at most six passes, then `HollowError::WallNotReached`.
  - *Shared work:* the passes share the source index and its plane index.
  - *Rule:* `guaranteed_cell` and `surface_area` give the rule's first cell; the certificate
    reports the offset and the passes.
  - *Measured* (`hollow_guarantee_budgets`, release, 16 threads), all in one pass:

    | Model | Wall | Certified wall | Cavity | Time |
    | --- | --- | --- | --- | --- |
    | `Base_01.stl` | 1.3 mm | 1.3042 mm | 2.35 M triangles | 4.85 s |
    | `Base_01.stl` | 0.5 mm | 0.5053 mm | 6.43 M | 10.65 s |
    | The Yorick | 0.5–2 mm | at or above each request | — | 0.7–1.1 s |
    | FX-12 | 0.5–2 mm | at or above each request | — | 3.6–4.2 s |

    The whole run peaked at 1.03 GB.

- **h3 (2026-10-22), pipeline and command line:** every hollow plan carries the guarantee, with
  the rule's cell from the source's surface area unless `--hollow-cell` (or a plate's
  `cell_mm`) gives one. Drain-hole cuts are sampled within a 32nd of the cell the shell used.
  - *Summary:* the summary adds `Hollow wall guaranteed (mm)`, `Hollow offset (mm)`,
    `Hollowing passes` and `Thickest wall at cavity vertices (mm)`, and drops
    `Measured wall at cavity vertices (mm)`.
  - *Progress and refusals:* progress names further passes, and `WallNotReached` adds "a thicker
    wall or a finer --hollow-cell may reach it".
  - *Comparison suite:* the six hollowed cases change (outputs and summaries) and every other file
    is identical.
    - The walls are at or above the request: 2.0503 mm for 2 mm, and 1.999998 mm with c10's
      simplification, whose floor is the wall less twice the query error.
    - The resin grows about 2.4 % (the 0.05 mm over-thickness).
    - Surface holes are 0.05 mm longer.
    - The times fall: c02 from 4.75 to 1.76 s, c10 from 11.48 to 3.14 s, c18 from 4.42 to
      1.69 s.

    The new output is the comparison baseline from now on.
  - *`Base_01.stl` with a surface drain hole on the P1:* 1.3 mm is guaranteed at 1.3042 mm and
    drilled, 10.2 s in all, 0.93 GB (refused before). 1.0 mm is guaranteed at 1.0039 mm and
    drilled, 8.1 s, 1.15 GB (hollowing itself was refused before).

- **h4 (2026-10-22), interface:** the hollowing panel shows the guaranteed wall, the thickest
  spot at the cavity's vertices and the passes; it warns under 1 mm, and the wall field now goes
  down to 0.1 mm. The "Simplify the cavity" checkbox is gone: a project that asks for
  simplification keeps it and the panel says so. The cell hint describes the rule. Pass progress
  comes from the pipeline (h3).
  - *Measured from the panel* (release, 16 threads, the app test of TEST-STRATEGY):

    | Model | Wall | Cell | Guaranteed | Thickest | Hollowed | Hole applied | Peak memory |
    | --- | --- | --- | --- | --- | --- | --- | --- |
    | `Base_01.stl` | 1.3 mm | 0.408 mm | 1.3042 mm | 1.62 mm | 10.6 s | 2.4 s | 1.71 GB |
    | `Base_01.stl` | 1.0 mm | 0.358 mm | 1.0039 mm | 1.25 mm | 11.8 s | 3.0 s | 2.06 GB |
    | FX-12 | 1.0 mm | 0.358 mm | 1.0042 mm | 1.24 mm | 15.5 s | 2.5 s | 1.71 GB |
    | The Yorick | 1.0 mm | 0.358 mm | 1.0036 mm | 1.24 mm | 4.2 s | 0.8 s | 0.48 GB |

    All in one pass. "Hollowed" counts from opening the file to the prepared plate.
  - *Contention, not in M6.4e's scope:* the app diagnoses an imported model on its own thread
    while the plate is prepared, so the two share the processor. FX-12's hollowing setup and
    topology check take about 8.5 s in the app against 4.4 s from the command line. Scheduling
    the diagnosis is left to the interface review.

- **h5 (2026-10-22), validation:** the twenty models of the experiment, hollowed by the product
  code (`hollow_guarantee_budgets`, release, 16 threads, one process per model and wall) at 2.0,
  1.3, 1.0 and 0.5 mm. Raw results: `hollow-runtime/product/` in the external evidence.
  - *Guarantee:* 79 of 80 runs are guaranteed in one pass. The 80th, `Hair_1.stl` at 2 mm, has
    nothing to hollow and is refused as such, as in the experiment. Every guaranteed wall is
    0.003–0.037 mm above the request; the thickest spots are 0.17–0.42 mm above it.
  - *Time:* 251 s for all runs, against 328 s in the experiment; no run is slower than its
    experiment run by more than 15 %. The longest is
    `obj_1_Gameboy_v2` at 0.5 mm: 22.9 s, 13.0 M cavity triangles.
  - *Memory:* at most 1.50 GB (`obj_1_Gameboy_v2` at 0.5 mm; 1.35 GB in the experiment), the
    only run above section 1's 1.4 GB target. Every other run stays under 1.25 GB.
  - *Targets:* `Base_01.stl` at 1.3 mm is guaranteed at 1.3042 mm in 4.9 s, 0.48 GB, and drained
    (h3, h4); at 0.5 mm at 0.5053 mm in 10.2 s, 0.99 GB.
  - *Fixtures:*

    | Model | 2.0 mm | 1.3 mm | 1.0 mm | 0.5 mm |
    | --- | --- | --- | --- | --- |
    | FX-11 | 2.0503 mm, 0.86 s | 1.3503 mm, 1.13 s | 1.0503 mm, 1.47 s | 0.5503 mm, 2.37 s |
    | FX-12 | 2.0046 mm, 3.79 s | 1.3047 mm, 3.88 s | 1.0042 mm, 3.97 s | 0.5054 mm, 4.53 s |
    | The Yorick | 2.0044 mm, 0.78 s | 1.3041 mm, 0.82 s | 1.0036 mm, 0.91 s | 0.5057 mm, 1.13 s |

    FX-11's flat walls are guaranteed at the offset itself: TD-066's 2 mm reference, certified at
    1.571 mm before h1, is now 2.0503 mm.
  - *Before and after* on `Base_01.stl` at 1.3 mm (the design comparison's measurements):

    | | Time | Cavity triangles | Certified wall | Drain holes |
    | --- | --- | --- | --- | --- |
    | TD-045's backend (`wall / 8`) | 10.5 s | 15 M | ≥ 1.014 mm | refused |
    | M6.4e | 4.9 s | 2.35 M | ≥ 1.3042 mm | drained |

    Under about 1.1 mm TD-045's backend refused hollowing `Base_01.stl` at all; M6.4e guarantees
    0.5 mm.
  - *Native Windows* (`validate.ps1 -IncludeApp`, code of e8225e2 with h5's documents): fmt,
    clippy, Debug and Release workspace tests (1,249 passed each, 0 failed, 7 ignored; Linux
    runs 2 more, Unix-only), build, rustdoc, and the application's three smoke runs all passed.

## 4. After M6.4e

- **Short-term items** of the comparison (section 7), done on 2026-10-22:
  - *A leaner hole index* (TD-094): 173 bytes per face instead of 308.
  - *A parallel face Z index:* the slicer's face index is sorted on every core, in the same
    order. On `Base_01.stl` the sort fell from 0.124 to 0.033 s at 1.3 mm (3.4 M faces), and
    from 0.61 to 0.20 s at 0.3 mm (16.5 M faces).
  - *Cached probes:* a hollowed object's hole probe is kept in the pipeline cache under its
    cavity and exact placement, so adding, moving or reshaping holes does not index it again.
    - Its hierarchy is built on every core: subtrees fill known slots, and a node's box is its
      children's joined.
    - The cavity's triangles are written once, in slot order, from the mesh, instead of copied
      and reordered.
    - The result is the same hierarchy, bit for bit.
    - On `Base_01.stl` at 0.3 mm (15.5 M cavity faces), measured from the panel:

      | Step | Before | After |
      | --- | --- | --- |
      | Applying a hole | 10.1–11.0 s | 1.0–1.1 s |
      | Moving the object | 9.2–9.6 s | 5.7 s |
      | Hollowing | 30.5–32.3 s | 28.9 s |

      The application peaked at 7.0 GB either way.
- **Adaptive extraction** (option A), for walls under about 0.5 mm on large models: an open
  decision, to take before manual supports (M6.4g, after the viewport and interface reviews). If built, it is a second mode beside the
  fixed grid, which stays ([design comparison](HOLLOW-CAVITY-DESIGN-COMPARISON.md), section 9,
  question 4).
- **The raster-side verifier.**
- **Then** manual supports (M6.4g) and the mandatory preparation scenarios (M6.4h), after the
  viewport review (M6.4f) and the interface review (M6.5), which the owner brought forward on
  2026-10-23.

## 5. Risks

- **The shortfall law** holds on 20 models. A model with sharper offset curvature exceeds it; the
  fallback pass covers that, at the cost of time.
- **The proof's cost** is about the hollowing's on dense models (FX-12). The skip filter of h1
  must keep it there.
- **Thick spots** (up to about 0.9 cells) cost resin. `τ` controls them, and a user-facing
  quality setting may come later.
- **Changed outputs:** every hollowed command-line output changes. TD-093 records it, and the
  comparison suite takes a new baseline.
