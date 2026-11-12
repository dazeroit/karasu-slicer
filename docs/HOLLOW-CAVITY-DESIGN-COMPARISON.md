# Hollowing at any wall: print-grid hollowing, an exact certificate, or adaptive extraction

**Status:** design comparison, 2026-10-21. **Decided on 2026-10-22 (TD-093):** option C with the
predicted cell and offset (section 5.2), with the suggestions of section 9; planned as M6.4e in
[HOLLOW-CERTIFIED-PLAN](HOLLOW-CERTIFIED-PLAN.md). Figures marked *measured* come from recorded runs; figures marked *estimate* are calculations
from them and must be confirmed by a prototype. Raw results of every Karasu run stay outside the
repository.

**Two wall figures, not to be confused.** Karasu's summary reports two ranges:
- *Measured wall at cavity vertices:* exact distances at the cavity's vertices, a diagnostic.
- *Certified wall range:* the certificate. It widens each vertex distance by its triangle's size
  to cover the triangles' interiors.

Only the second is a guarantee. Earlier drafts of this comparison and of the hollowing research
called the first "certified"; that is corrected here.

## 1. The problem

Karasu's sampled hollowing (TD-045, backend `sampled-distance-tetra-v1`) extracts the cavity as a
closed triangle mesh on a uniform grid whose cell is tied to the wall: `cell = min(wall / 8,
0.5 mm)`. The field is evaluated only near the iso-surface `d = wall` (Lipschitz-pruned blocks),
but the output has one triangle size everywhere, so the cavity's triangle count grows as the
inverse square of the wall:

| `Base_01.stl` (1.0 M faces, 50,300 mm² of surface), P1 printer | Cavity triangles | Result |
| --- | --- | --- |
| 1.5 mm (cell 0.1875 mm), *measured* | 11.1 M | hollowed, drilled, sliced in 19.5 s, 3.3 GB |
| 1.3 mm (cell 0.1625 mm), *measured* | 14.97 M | refused once a drain hole is placed |
| 1.3 mm with certified simplification (0.0084 mm), *measured* | 14.97 M → 3.31 M | 127.8 s, 5.8 GB |
| 0.1 mm (cell 0.0125 mm), *estimate* | about 2.5 billion | beyond every limit by 4×–190× |

Every consumer of the cavity pays for its size:
- **Hole probe and findings:** an index of about 300 bytes per cavity face, capped at 4 GiB, so
  about 14.3 M faces.
- **Slicing:** a serial face Z index before the first layer, 4.9 s of the 6.1 s slicing time at
  1.3 mm *measured*.
- **The rest:** the supports' index, the GPU upload, CPU picking and the cache.

**What the references do (measured on `Base_01.stl`):**
- *Practical walls:* Lychee (about 3 s at 1.3 mm) and CHITUBOX (about 35 s) produce the
  requested wall within about one pixel at 0.5 mm and above, as Karasu does (11.4 s).
- *Extreme walls:* Lychee refuses walls under 0.5 mm. CHITUBOX hollows at 0.1 mm in minutes, but
  its wall is breached in four layers without a message.
- *How they size the cell (black-box, A.4):*
  - CHITUBOX shrinks the cell with the wall, but more slowly (about `wall^0.55`), and divides it
    by its precision setting. Its cavity grows about as `1 / wall`, and at 0.1 mm the cell is as
    large as the wall.
  - Lychee bounds the cell by the model's size (250 cells on the longest side).
- *Gap:* neither guarantees the wall; Karasu does, and stops instead. Karasu's guarantee is
  also loose: at 1.3 mm its certificate proves only 1.014 mm, although every vertex and, as
  section 5 shows, every triangle lies at 1.292 mm or more.

**Goal:**
- **Guarantee:** keep Karasu's guarantee, a certified minimum wall and a cavity that never
  reaches the outside.
- **Resources:** make time and memory depend on the model's geometry, not on `1 / wall²`.
- **Range:** cover every wall a user can ask for, down to 0.1 mm on `Base_01.stl`.

## 2. Constraints

From AGENTS.md and the recorded decisions:
- **No foreign kernels** (no OpenVDB or libigl). Use `f64` for principal geometry, deterministic
  results for any thread count, and no silent change of geometry.
- **Measured performance:** heavy work stays off the UI thread, and the scene is drawn on the GPU.
- **TD-045 already rejected two of the ideas below:**
  - "Dense voxel or chamfer fields: approximate metric, cubic memory."
  - "Per-layer 2D offsets: not a 3D wall."
  Any print-grid design must answer both objections.
- **TD-089 hollows in the model frame:**
  - Moving, turning or scaling never hollows again, and the cavity is cached by the plan record.
  - Drain holes are free operations.
  - The app shows the cavity, caps sections with it and places holes on it.
- **Command-line outputs** stay byte-identical unless a recorded decision changes them. Either
  option changes hollowed outputs and needs one.

## 3. Option R: hollowing on the print grid

**Idea:** decide material per print voxel instead of extracting a surface. A voxel is one
pixel, or one antialiasing sample, of one layer. A material voxel becomes cavity when its exact
Euclidean distance to the nearest non-material voxel exceeds the wall.

**Algorithm:**
- The distance comes from a separable exact distance transform: per row, per column, then
  across layers through a lower envelope of parabolas (Saito–Toriwaki, Felzenszwalb–
  Huttenlocher), on the anisotropic lattice (16.8 × 24.7 × 50 µm on the P1).
- Only `distance > wall` matters, so the transform can be capped at the wall and stream through
  a window of `2·ceil(wall / layer) + 1` layers.
- Cavity regions are 3D connected components of cavity voxels, by streaming union-find across
  layers.
- Drain holes are already subtracted per raster sample (`HoleCuts`, `LayerCut`). A region is
  drained when its voxels connect to the outside through a hole's voxels.

**What it answers in TD-045:**
- *Metric:* the metric is exact on the lattice, not chamfer. Its error against the continuous
  wall is bounded by the voxel's half-diagonal (about 0.03 mm on the P1 at 0.05 mm layers).
- *Memory:* it is bounded by the layer window and XY tiles, not by the whole box.
- *Per-layer offsets:* the third pass across layers makes it a true 3D wall, not a per-layer
  offset.
- *Breaches:* no cavity voxel can touch an exterior voxel once `wall ≥` the largest lattice
  step, so the breaches seen in CHITUBOX are impossible by construction.

**Cost (*estimate*):**
- **Voxels:** the box of `Base_01.stl` on the P1 is 9,722 × 1,325 × 2,097 ≈ 27 billion voxels at
  pixel resolution, and 16 times more on 4 × 4 antialiasing samples.
- **Time:** three linear passes at a few nanoseconds per voxel give minutes on one core, so
  tens of seconds on 16 cores. That is roughly constant in the wall, because the cost follows
  the model's volume at print resolution.
- **Memory:** the window of capped squared distances is about 1.4 GB at 1.3 mm (53 layers, 16-bit
  values, uncropped tiles) and about 130 MB at 0.1 mm (5 layers). XY tiles reduce both.
- **Larger models:** a 16K printer has 4–5 times more pixels than the P1 for the same model.

**What it breaks or replaces** (every consumer that reads the cavity mesh today):

| Consumer | Today | Under option R |
| --- | --- | --- |
| Moving, rotating, scaling (TD-089) | cavity re-placed, never re-hollowed | hollowed again on every placement or printer change: the print grid is in world coordinates |
| Cache | cavity by plan record, in the model frame | keyed by placement and printer too; little reuse while editing |
| Hole probe, findings, surface holes on the cavity | BVH of the cavity mesh | voxel queries; a hole "from the cavity" needs a surface the voxels do not have |
| Supports inside the cavity | anchored on cavity faces | no faces; interior supports need another anchor model |
| App: cavity display, section caps, cavity picking | GPU mesh, stencil caps, CPU ray picking | a display mesh extracted from voxels, or none |
| Certificate and summary | wall range from exact vertex distances widened per triangle | wall bound from the lattice: `wall − half-diagonal` |
| Previews (thumbnails) | render the shell mesh | shell without the cavity, or a voxel render |

**Strengths:**
- It is the printed result itself: the guarantee is about the voxels the printer cures.
- Its cost does not grow at thin walls, so 0.1 mm is as cheap as 1 mm.
- It is simple and deterministic: integer lattice, linear passes, no mesh validity to prove.
- It also gives an independent check of any layer stack, Karasu's included. This is the
  measurement used for the commercial exports, made exact.

**Weaknesses:**
- Hollowing depends on placement and printer, which undoes TD-089's main property and the
  interactive flow built on it.
- About half of the hollowing-related features need a second design, because there is no
  cavity surface for holes, supports or display.
- The cost follows the volume at print resolution, which grows fastest with the printer
  generation.
- Variable layer heights (TD-064) make the lattice irregular in Z, which the envelope pass
  handles at some complexity.

## 4. Option A: adaptive cavity extraction

**Idea:** keep the cavity as a certified closed triangle mesh, but extract it with cells sized
by need instead of by the wall.

**Algorithm:**
- **Refinement:** cells are refined where the cavity surface bends, or where the certificate
  is not yet met, and stay coarse on flat or gently curved stretches.
- **Conforming tetrahedra:** the current extraction already runs marching tetrahedra on the
  Kuhn (Freudenthal) subdivision. Kuhn simplices refine conformingly by newest-vertex bisection
  (Maubach 1995; Traxler 1997), so neighbouring cells of different sizes share faces without
  cracks. The cavity therefore stays closed and manifold, as the validation requires, without
  dual-contouring heuristics.
- **Certificate-driven refinement:** extract coarsely, certify each triangle exactly (option C),
  and refine only the cells whose triangles are not proven within `[wall − ε, wall + ε']` of the
  source.
  - The a priori band (`cell · √3 + 2 · error`) no longer bounds coarse cells. The exact
    per-triangle certificate of option C carries the guarantee.
  - The certificate's terms stay the same: a certified minimum wall, and a cavity that never
    meets the source.
- **Determinism:** the refinement order is a fixed function of cell keys, as the field's work
  items are today.

**Cost (*estimate*):**
- **Triangles:** they follow the complexity of the offset surface, not `1 / wall²`.
  - *1.3 mm:* certified simplification of the 1.3 mm cavity of `Base_01.stl` at 0.0084 mm already
    shows that about 3.3 M triangles carry it. An extraction aiming at the same tolerance should
    land in that order, without first producing 15 M.
  - *0.1 mm:* the cavity is nearly the source surface moved inward. Its detail is the source's
    (1.0 M faces), so the order is a few million triangles, not billions.
- **Time:** it follows the number of evaluated cells, which is proportional to the output, plus
  the refinement rounds.
- **Memory:** every consumer's memory falls with the triangle count:
  - hole probe index 300 bytes per face;
  - slicing face Z index 24 bytes per face;
  - supports index, GPU upload, picking and cache.

**What it keeps:** every consumer listed under option R works unchanged, because the cavity is
still a closed, oriented mesh in the model frame. That covers:
- TD-089 placement and caching;
- holes from the cavity;
- interior supports;
- caps;
- picking;
- the summary's certificate.

**Strengths:**
- The output size follows the geometry at any wall, and the guarantee is unchanged.
- The change is confined to the field, extraction and certification of `preparation::sampled`.
  The plan model, consumers, app and CLI keep their interfaces.
- It makes TD-066's slow simplification unnecessary as a default; it stays as an option.

**Weaknesses:**
- It is algorithmically harder:
  - conforming adaptive refinement with closed-manifold output;
  - refinement criteria that converge;
  - a certificate that holds where coarse cells meet fine ones.
- Its benefit depends on the surface. A finely sculpted model refines everywhere, and the
  triangle count then approaches the source's complexity times a constant, not less.
- At very thin walls the field still needs many exact distance queries near the surface. Their
  count follows the output, but each is a BVH query on the source.

**A second lever, orthogonal to adaptivity:**
- **The observation:** at the same 0.26 mm cell on `Base_01.stl` at 1.3 mm, CHITUBOX's cavity has
  1.63 M triangles and Karasu's 5.82 M, 3.6 times more.
- **The likely cause (I):** a dual surface has about one vertex per crossed cell, while marching
  tetrahedra on Kuhn cubes emit more.
- **What it would take:** an extraction with fewer triangles per cell would help every option. It
  must keep the closed, embedded, manifold output that TD-045 chose marching tetrahedra for; its
  rejection of marching cubes with position welding stands.

## 5. Option C: an exact certificate with coarser cells

**Idea:** keep today's backend, but replace the loose certificate by an exact one per triangle.
Then choose a coarser cell, because the guarantee no longer depends on the cell's size. If needed,
hollow a second time with the wall corrected by the measured shortfall.

**Today's certificate is the loose part, not the geometry (*measured*).**
- **Method:** an experiment on a separate worktree, never committed. Every cavity triangle of
  `Base_01.stl` at 1.3 mm is certified against the source with two functions Karasu already has:
  - `Bvh::lower_bound`: a certified lower bound on the triangle's distance;
  - `Bvh::clearance`: an exact proof that the triangle is farther than a threshold, the test the
    TD-066 simplification uses for its wall floor.
- **Setup:** 16 threads, identity placement, no holes.

| Cell | Cavity triangles | Hollowing | Certificate today (min) | Vertices (min) | Exact minimum over triangles | Exact minimum, cost | Proof "every triangle ≥ vertex minimum − 1 µm", cost |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 0.653 mm (Lychee's rule) | 0.91 M | 2.05 s | 0.166 mm | 1.1733 mm | 1.1725 mm | 9.1 s | 0 unproven, **0.60 s** |
| 0.26 mm (`wall / 5`) | 5.82 M | 4.84 s | 0.844 mm | 1.2805 mm | 1.2804 mm | 41.0 s | 0 unproven, **5.8 s** |
| 0.1625 mm (`wall / 8`, today) | 14.97 M | 10.48 s | 1.014 mm | 1.2924 mm | 1.2924 mm | 91.1 s | 0 unproven, **15.7 s** |

- **The real minimum is the vertex minimum.** The exact minimum over triangles equals it within
  1 µm in every case (11, 22 and 58 triangles fall below it, by less than 1 µm).
- **What drives the old certificate:** it widens each vertex distance by the triangle's size,
  which is why it falls with the cell (65–78 % of the wall). The true minimum barely moves.
- **A proof is enough.** Proving every triangle beyond a threshold costs 6–15 times less than
  computing each triangle's minimum: the exact test prunes everything far from the threshold.
  The statement is the same, "certified wall ≥ X". The proof costs about 30 % of the hollowing
  time with coarse cells and 150 % at `wall / 8`.

**Coarser cells, measured with today's command line** (P1, surface drain hole, `Base_01.stl`).
Triangle counts in parentheses are *estimates* from `7.9 · A / cell²`.

| Cell rule | Wall | Cavity triangles | Hole | Time | Peak memory | Vertices | Certificate today |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `wall / 8` | 1.3 mm | 14.97 M | refused | 19.4 s without hole | 3.0 GB | 1.292–1.408 | 1.014–1.639 |
| `wall / 8` | 1.0 mm | over 20 M | hollowing refused | — | — | — | — |
| `wall / 6` | 1.3 mm | (8.4 M) | ok | 15.4 s | 2.5 GB | 1.286–1.431 | 0.916–1.751 |
| `wall / 6` | 1.0 mm | 14.53 M | refused | — | — | — | — |
| `wall / 5` | 1.3 mm | (5.9 M) | ok | 10.9 s | 2.0 GB | 1.280–1.471 | 0.844–1.822 |
| `wall / 5` | 1.0 mm | (9.9 M) | ok | 20.1 s | 3.1 GB | 0.985–1.133 | 0.644–1.404 |
| `wall / 5` | 0.9 mm | (12.2 M) | ok | 21.1 s | 3.8 GB | 0.886–1.016 | 0.580–1.259 |
| `wall / 5` | 0.8 mm | 15.94 M | refused | — | — | — | — |
| Lychee's (0.653 mm) | 2.0 mm | (0.9 M) | ok | 5.8 s | 0.48 GB | 1.919–2.374 | 0.858–3.272 |
| Lychee's (0.653 mm) | 1.3 mm | 0.91 M | ok | **3.2 s** | 0.50 GB | 1.173–1.700 | 0.166–2.585 |
| Lychee's (0.653 mm) | 1.2 mm | (0.9 M) | ok | 5.7 s | 0.50 GB | 1.063–1.604 | 0.054–2.509 |
| Lychee's (0.653 mm) | 1.15 mm | — | refused: the loose certificate cannot prove separation | — | — | — | — |
| Lychee's (0.653 mm) | 1.1 mm | — | refused: `cell · √3 ≥ wall` | — | — | — | — |

**Reading the cell rules:**
- **Triangle counts** follow `(rule / 8)²` within 2–3 % of the prediction.
- **Real walls** degrade slowly: the vertex minimum is 0.6 % under the request at `wall / 8`,
  1.0 % at `/ 6`, 1.5 % at `/ 5` and 10 % with Lychee's rule. Coarse cells interpolate badly
  near the source's features.
- **The hole limit on `Base_01.stl`** falls from about 1.33 mm (`/ 8`) to about 1.0 mm (`/ 6`)
  and about 0.83 mm (`/ 5`).
- **Lychee's rule** gives Lychee's speed: 3.2 s at 1.3 mm, against about 3 s measured in Lychee.
- **Small cavity pockets** appear or vanish with the cell: 5, 4 and 8 cavity surfaces at 1.3 mm
  with `/ 8`, `/ 6` and `/ 5`.

**Combined, hollowing plus the proof (1.3 mm, *measured* parts added):**

| | Time | Cavity triangles | Certified wall | Drain holes |
| --- | --- | --- | --- | --- |
| Today (`wall / 8`) | 10.5 s | 15 M | ≥ 1.014 mm | refused |
| `wall / 8` + proof | 26 s | 15 M | ≥ 1.291 mm | refused |
| **`wall / 5` + proof** | **10.6 s** | **5.8 M** | **≥ 1.279 mm** | **ok** |
| Lychee's cell + proof | 2.7 s | 0.9 M | ≥ 1.172 mm (10 % under) | ok |

**A sub-proportional cell rule (not yet tried):** CHITUBOX's cell shrinks about as
`wall^0.55` (A.4). Its cavity then grows about as `1 / wall` instead of `1 / wall²`, at the price
of coarse cells at thin walls. With an exact certificate the same rule becomes honest: the
reported wall is the real one, and where no positive wall can be proven the run is refused,
or, under option A, refined locally. Thin walls with such cells need the `cell · √3 < wall` rule
revisited, as below.

### 5.1 Successive approximations (measured)

With coarse cells, the proof tells the real shortfall, so the wall can be corrected and proven
again. The experiment ran this loop on a detached worktree; the code is kept outside the
repository with the results (`hollow-runtime/successive/`).

**The loop:**
1. **Cell:** `max(wall / 8, longest side / 250)`, today's precision for small models and
   Lychee's bound for large ones, lowered if needed to keep `cell · √3 < wall`.
2. **Pass:** hollow at the current offset, then compute the certified minimum wall `m`. Every
   cavity triangle is proven beyond the vertex minimum less 1 µm; a triangle that fails gets its
   own certified lower bound.
3. **Stop:** when `m ≥ wall`.
4. **Correct:** otherwise the offset grows by 1.25 times the shortfall.
5. **Refine:** after three passes at one cell, or when hollowing refuses the cell, halve the cell
   and restart from the requested wall.
6. **Bounds:** at most eight passes.

**Results:**
- **59 runs:** 20 models of the owner's library and the fixtures, each at 2.0, 1.3, 1.0 mm and
  more.
  - *Models:* miniatures, action-figure bases and torsos, FDM parts, the Yorick and FX-12.
  - *Sizes:* longest side from 27 to 232 mm, 12 k to 2.7 M faces.
- **Every run ends with a certified wall at or above the request.**
- **Passes:** 58 runs needed two passes. One run needed three: Venom's `Torso_P2` (170 mm) at
  1.0 mm, where hollowing refused the coarse cell and the loop halved it.

Runs are grouped by the cell the rule chose. A small model gets `wall / 8` at thick walls, and
`side / 250` once the wall is thin enough.

| Cell chosen (runs) | First pass, shortfall | Final certified wall | Thickest wall | Time | Peak memory |
| --- | --- | --- | --- | --- | --- |
| `wall / 8`, today's (14) | 0.4–0.7 % | +0.1 to +0.2 % | +7 to +10 % | 0.3–6.5 s | 0.01–0.33 GB |
| `side / 250` or the `√3` limit (41) | 0.8–11.9 % | +0.2 to +5.0 % | +9 to +56 % | 0.5–12.7 s | 0.03–0.44 GB |
| FX-12, 2.7 M dense faces (3) | 0.6–1.6 % | +0.2 to +0.4 % | +7 to +13 % | 18–20 s | 0.9 GB |
| `Torso_P2` at 1.0 mm, refined, three passes (1) | 2.9 % at the halved cell | +0.8 % | +21 % | 27.8 s | 1.0 GB |

**`Base_01.stl`:**

| Wall | Loop, cell 0.65 mm | Today, `wall / 8` |
| --- | --- | --- |
| 1.3 mm | certified 1.347 mm, 0.90 M triangles, 7.5 s, 0.37 GB; drain holes fit | certified 1.014 mm, 15 M triangles, 19.4 s, 3.0 GB; drain holes refused |
| 1.0 mm | certified 1.045 mm in 8.6 s | hollowing refused (over 20 M triangles) |

**Reading the results:**
- **Today's certificate on large models is meaningless.** At 1.0 mm it would state 0.01–0.05 mm
  on four of the large models, where the loop proves 1.04–1.05 mm.
- **Cavity size:** every cavity stays under 3.1 M triangles (the 232 mm Gameboy cartridge at
  1.0 mm), far below the 14.3 M drain-hole index cap.
- **Cost:**
  - *Thick spots:* on large models some regions are much thicker than requested, up to +56 %. That
    costs more resin and a smaller cavity, and must be reported with the certified minimum. The
    25 % correction margin also overshoots the minimum by up to 5 %; a smaller margin trades that
    for an occasional third pass.
  - *The proof's cost:* on dense models such as FX-12 it is about twice the hollowing, so the loop
    (18–20 s) is slower there than today's single pass (about 3 s, certified only 0.95 mm).
- **The refusal of the coarse cell came from today's built-in check.** It requires the loose
  certificate to prove separation; the exact proof would have accepted the cell. With an exact
  certificate that check must be replaced.

### 5.2 A predicted cell and offset (measured)

**The first-pass shortfall follows one law.** Across the 58 first passes of section 5.1, on 20
different models, the shortfall is about

`shortfall ≈ 0.39 · cell² / wall`

- **Spread:** the ratio of the measured shortfall to `cell² / wall` has a median of 0.390, a 10th
  percentile of 0.377 and a 90th of 0.423, with a maximum of 0.442. A free fit gives
  `0.367 · cell^1.92 / wall^1.18`.
- **Below the law:** only smooth models fall under it: the Katana handle at 2 mm (0.09), the
  Tent, `Hair_1` and the LEGO base (about 0.28).
- **Geometric reading (I):** next to a reflex edge of the source, which almost every real model
  has, the level set `d = wall` is a rounded surface of radius `wall`. A tetrahedron of diameter
  `cell · √3` cuts it with a chord whose sagitta is `(cell · √3)² / (8 · wall) = 0.375 · cell² /
  wall`, the measured coefficient.
- **Consequence:** the minimum-wall error does not depend on a model's "complexity" but on the
  cell and the wall. It can be predicted before hollowing.

**The rule derived from it, with everything computed before hollowing:**
- **Cell:** the larger of two values, kept under `wall / √3`:
  - `√(τ · wall / 0.39)`, where `τ` is the accepted over-thickness (0.05 mm here);
  - `√(7.9 · A / N)`, where `A` is the source's surface area and `N` a triangle budget
    (10 M here).
- **Offset:** `wall + 0.45 · cell² / wall`.
- **Proof and fallback:** the exact per-triangle proof, then the loop of section 5.1 if needed.
- **Growth with the wall:** the cell grows as `√wall`, so the cavity grows as `1 / wall`, not
  `1 / wall²`. This is the shape measured for CHITUBOX (A.4), here with a guarantee.

**Campaign:** the same 20 models at 2.0, 1.3, 1.0 and 0.5 mm, 80 runs. Raw results:
`hollow-runtime/predicted/`.

| Wall | Guaranteed | Passes | Time | Peak memory | Largest cavity |
| --- | --- | --- | --- | --- | --- |
| 2.0 mm | 19 / 20 | 1 | 0.1–7.1 s | ≤ 0.82 GB | 3.3 M triangles |
| 1.3 mm | 20 / 20 | 1 | 0.3–10.5 s | ≤ 0.83 GB | 5.6 M |
| 1.0 mm | 20 / 20 | 1 | 0.4–13.5 s | ≤ 0.85 GB | 7.4 M |
| 0.5 mm | 20 / 20 | 1 | 0.8–22.6 s | ≤ 1.35 GB | 13.0 M (the 232 mm Gameboy cartridge) |

The missing run is correct: `Hair_1` has no interior point deeper than 2 mm, so there is nothing
to hollow. The experiment's loop kept refining there; it must stop on that answer.

**The predictions:**
- **Shortfall:** the measured one is always below the prediction (median 80 %, at most 83 %), so
  every run closed in one pass. The certified wall exceeds the request by a median 0.0115 mm and
  at most 0.045 mm (smooth models). A coefficient of 0.40 instead of 0.45 would about halve that.
- **Triangles from the area:** close for large models (`Base_01.stl` 2.35 M against 2.38 M
  predicted; others within 5–10 %), and an upper bound for thin or small ones, whose cavity has
  less area than the source. Never more than 7 % under.

**Thick spots:**
- *Size:* the thickest point exceeds the request by a median 0.29 mm and at most 0.42 mm, about
  0.7–0.9 cells, near the source's edges where the interpolated cavity lags.
- *Not uniform:* the background over-thickness stays near `τ` plus about 0.01 mm.
- *Control and reporting:* `τ` is the control, and the thick spots must be reported with the
  certified minimum.

**`Base_01.stl`:**

| | Today (`wall / 8`) | Section 5.1 loop | This rule |
| --- | --- | --- | --- |
| 1.3 mm: passes, time | 1, 19.4 s | 2, 7.5 s | **1, 6.7 s** |
| 1.3 mm: certified wall | 1.014 mm | 1.347 mm | **1.311 mm** |
| 1.3 mm: thickest point | +0.11 mm | +0.52 mm | **+0.33 mm** |
| 1.3 mm: cavity, drain holes | 15 M, refused | 0.9 M, fit | 2.35 M, fit |
| 0.5 mm | impossible (about 100 M triangles) | — | **0.5125 mm certified, 6.4 M triangles, 11.8 s, 0.93 GB** |

**Limits:**
- **Area growth stays.** The cell is still uniform, so cost keeps growing with the model's area
  over `cell²`. A budget per model (as Lychee's 250 cells) bounds it, but then thin walls on large
  models need cells that coarse interpolation makes inaccurate, or that `cell · √3 < wall` forbids.
- **The `cell · √3 < wall` rule** is the a priori band's requirement for the evaluation's leaves.
  With an exact certificate it could be relaxed, which needs its own proof that the field's
  pruning stays sound.

**Strengths:**
- It changes the certificate and a constant, nothing in the consumers, plan model or interfaces.
- It is measured today, not estimated.
- It makes Karasu's guarantee tight, so certified walls are what users get.
- **Foundation for option A:** option A needs exactly this per-triangle proof.

**Weaknesses:**
- It does not reach 0.1 mm on large models: below about `side / 250 · √3` the cell must shrink
  and the cost grows again, as `Torso_P2` at 1.0 mm already shows.
- The proof costs as much as the hollowing or more on dense models. Known optimizations:
  - start from an offset that already includes the typical shortfall, often ending in one pass;
  - skip triangles whose widened vertex bound already passes;
  - reuse the source index across passes.

## 6. Side by side

| Criterion | Today | R: print grid | C: exact certificate, coarser cells | A: adaptive extraction |
| --- | --- | --- | --- | --- |
| Guarantee | certified wall, loose (1.014 at 1.3 mm) | wall to the voxel half-diagonal, no breach | tight certified wall (1.279 at 1.3 mm, `/ 5`) | tight certified wall (with C's proof) |
| Drain holes on `Base_01.stl` down to | about 1.33 mm | any wall (*estimate*) | 0.5 mm or less with the predicted rule (*measured*: 0.5125 mm certified, 6.4 M triangles) | any wall (*estimate*) |
| 0.1 mm on `Base_01.stl` | impossible (billions of triangles) | feasible, cost as at 1 mm (*estimate*) | about 130 M triangles: the `√3` limit forces a 0.055 mm cell (*estimate*), over the caps | feasible, a few million triangles (*estimate*) |
| Cost grows with | `area / wall²` | model volume at print resolution | `area / cell²`, cell coarser | offset-surface complexity |
| Re-hollow on move or printer change | no | yes | no | no |
| Consumers to redesign | — | holes, regions, supports, app display, caps, picking, cache, previews | none | none |
| Main risk | — | breaks TD-089; a second design for half the features | the proof's cost at fine cells | conforming refinement and its proof |
| Code touched | — | raster stage, pipeline, app, supports, drainage | the certificate and the default cell | `preparation::sampled` (field, extraction, certificate) |
| Time, `Base_01.stl` at 1.3 mm | 19.4 s, 3.0 GB | — | 6.7 s, 0.48 GB (predicted rule, one pass) | — |
| Evidence | measured | estimates only | measured (experiment, CLI, 59-run loop on 20 models) | estimates only |

## 7. Independent of the choice

These help either way and can go first, each with measurements before and after:
1. **A leaner cavity index for holes:** about 300 bytes per face to about 150. Store triangles as
   indices or `f32` offsets, and skip the edge labelling where only crossings are needed. This
   doubles today's limit without any geometric compromise.
2. **Parallel face Z index:** the serial sort of every face before the first layer, 4.9 s of 6.1 s
   for the 1.3 mm hollowed `Base_01.stl`. Use the workspace's parallel sort, as topology already
   does (TD-056).
3. **Cached probes:** the hole probe is rebuilt on every preparation. Caching it beside the
   cavity, under the same key, makes repeated hole edits cheaper.
4. **A raster-side verifier:** option R's distance pass, run on the final layers only as a
   check. It confirms the printed wall and the absence of breaches for any backend, or for an
   imported layer file, without hollowing on the print grid.

## 8. Recommendation

**Option C first, then option A.**
1. **Option C with a predicted cell and offset (section 5.2):**
   - an exact per-triangle certificate (the clearance proof, with its cost optimized);
   - the cell from the over-thickness `τ` and the triangle budget, both computed from the wall
     and the surface area before hollowing;
   - the offset raised by the predicted shortfall;
   - the loop of section 5.1 as the fallback when a pass is refused or not proven.

   In 80 runs on 20 models from 0.5 to 2 mm it guaranteed the requested wall in one pass, in
   0.1–23 s and at most 1.35 GB, on small and large models. It keeps every cavity under the
   drain-hole cap and touches no consumer. Every number behind it is measured.
2. **Option A,** adaptive extraction driven by C's proof, with newest-vertex bisection of the
   existing Kuhn simplices. It is the structural answer for thin walls on large models and stays
   inside the hollowing core. It is still only estimated: a time-boxed prototype comes first.

**Option R** is not recommended as the hollowing backend. It would make hollowing depend on
placement and printer (against TD-089) and require a second design for holes, supports and the
cavity view. Its distance pass is worth keeping as the verifier of section 7, item 4.

**Order:**
1. Option C, with items 1 and 2 of section 7.
2. A time-boxed prototype of option A, measured on the cases below.
3. Each adoption as a technology decision, with its CLI output change recorded.

**Prototype acceptance cases:**
- `Base_01.stl` at 1.5, 1.3, 1.0, 0.5 and 0.1 mm;
- FX-11 at 2 mm (TD-066's reference);
- FX-12;
- the Yorick.

For each, record the cavity triangles, time, peak memory, the certified wall range, and the
regions against today's backend. The thread-count determinism tests carry over.

## 9. Decisions for the owner

1. Whether option C with the predicted cell and offset (section 5.2) goes first, and its
   parameters:
   - the over-thickness `τ` (0.05 mm measured), fixed or set by the user as a quality control;
   - the triangle budget (10 M measured, under the 14.3 M drain-hole index);
   - the shortfall coefficient (0.45 measured; 0.40 would halve the excess and has never been
     exceeded).
2. The guarantee's target:
   - always reach the requested wall (as measured), or accept a reported certified wall
     slightly under it;
   - whether the thickest spots (up to 0.42 mm over at `τ = 0.05`) must be bounded too, at the
     cost of finer cells.
3. Whether a minimum wall is needed at all, or only a warning as in Lychee (under 1 mm), with the
   certificate reporting what was achieved. The appendix gives a bound that depends only on the
   printer: at least 8 pixels for today's relative precision, √3 pixels as the absolute minimum.
4. Whether the prototype of option A starts after option C, or waits.
   - *Owner, 2026-10-26: decided, deferred after the MVP.* The slicer copes with today's fixed
     grid, and option A's gain is the machine's performance, not the print.
   - *Owner, 2026-10-22: to decide before manual supports (M6.4g since 2026-10-23).* Option A's gain is
     performance (memory, slicing, display), not the print: the wall is guaranteed either way,
     and the inside of an aesthetic model is never seen. If it is built, it sits beside the
     fixed grid as a second mode the user chooses, and the fixed grid (Auto or an explicit cell)
     stays.
   - *Proposed:* a time-boxed prototype measured on `Base_01.stl` at 1.3, 0.5, 0.3 and 0.2 mm
     before integration. A run with a low memory budget would show how far an 8 GB computer
     goes today.
5. Whether TD-066's certified simplification stays in the interface. Under the loop every
   measured cavity is under 3.1 M triangles and fits the drain-hole index, so simplification is
   no longer needed to make holes possible. It would still make a cavity lighter (FX-11: 1.4 M
   to 232 k triangles), at a cost of seconds to minutes, mainly to speed slicing and display.
6. Whether the raster-side verifier (section 7, item 4) is wanted as a feature, for example to
   check layer files from other slicers.
   - *Owner, 2026-10-22 (TD-095):* yes, on demand, Karasu's own layers first, and other slicers'
     files as a second phase ([plan](RASTER-VERIFIER-PLAN.md)).

## Appendix: the grid cell

### A.1 What `cell = wall / 8` gives

The cavity is the level set `d = wall` of the exact distance `d` to the source surface. It is
sampled on a grid and extracted by linear interpolation in the six Kuhn tetrahedra of each cube.

- **A priori band:** `d` changes by at most the distance moved (it is 1-Lipschitz), and a
  tetrahedron is no longer than the cube's diagonal. So every cavity point is within
  `band = cell · √3 + 2 · error` of the requested wall.
  - Soundness: every cavity point has `d ≥ wall − band`.
  - Completeness: every point deeper than `wall + band` is hollow.
- **Relative precision:** with `cell = wall / 8` the band is about 22 % of the wall, at any wall.
  This is what the rule buys: the same relative precision everywhere. No other rationale for the
  factor 8 is recorded; it is TD-045's default, overridable with `--hollow-cell`.
- **Leaf blocks:** the evaluation's leaf blocks need `L · cell · √3 + 2 · error < wall`. With
  `wall / 8`, `L = 4`.
- **Cap:** the 0.5 mm cap on the cell applies only to walls over 4 mm.
- **A posteriori certificate:** after extraction, exact distances at every cavity vertex, widened
  per triangle by `min(circumradius, longest edge)`, give the certified wall.
  - *Measured:* on `Base_01.stl` at 1.3 mm it proves 1.014–1.639 mm, hardly tighter than the a
    priori 1.02 mm, because the widening is of the cell's order.
  - *The real wall:* the vertices measure 1.292–1.408 mm, and an exact per-triangle certificate
    confirms 1.292 (section 5). On flat walls, vertices lie within about 10⁻³ mm of the request.

Cavity triangles grow as `area / cell²`, about `7.9 · A / cell²`. The coefficient is calibrated
on `Base_01.stl`, where `A` is at most about 50,300 mm² for thin walls. With `wall / 8` this is
about `503 · A / wall²`.

| Wall on `Base_01.stl` | Cell | A priori band | A priori wall | Cavity triangles |
| --- | --- | --- | --- | --- |
| 2.0 mm | 0.25 mm | 0.43 mm | ≥ 1.57 mm | about 6 M (*estimate*) |
| 1.3 mm | 0.1625 mm | 0.28 mm | ≥ 1.02 mm | 15.0 M (*measured*) |
| 1.0 mm | 0.125 mm | 0.22 mm | ≥ 0.78 mm | about 25 M (*estimate*) |
| 0.5 mm | 0.0625 mm | 0.11 mm | ≥ 0.39 mm | about 100 M (*estimate*) |
| 0.1 mm | 0.0125 mm | 0.022 mm | ≥ 0.078 mm | about 2.5 billion (*estimate*) |

**Coarser fixed cells do not fix this:**
- A fixed coarse cell, for example 0.2 mm at a 0.1 mm wall, has a band (0.35 mm) wider than the
  wall. The a priori guarantee is then void, and the run is refused unless the a posteriori
  certificate proves a positive wall.
- A coarser rule such as `wall / 5` or `wall / 6` gives fewer triangles for a wider a priori
  band. With an exact certificate the reported wall stays close to the request (section 5). It
  is a measured compromise, but the `1 / wall²` growth remains.

### A.2 A cell fixed at the printer pixel, with today's algorithm

Pixels on the presets and the owner's printer:
- Anycubic Photon P1: 16.8 × 24.7 µm;
- Elegoo Saturn 4 Ultra 16K: 14 × 19 µm;
- Anycubic Photon Mono M5 and M5s: 19 × 24 µm;
- Anycubic Photon Mono 4K: 35 µm;
- Anycubic Photon Mono X and SQ: 50 µm.

The cell is a cube, so on rectangular pixels one side must be chosen. With a fixed cell, the
cavity's triangle count no longer depends on the wall, only on the cavity's area:

| Cell | `Base_01.stl` cavity (*estimate*) | Against the 20 M cavity cap | Largest cavity area within the cap | Cube of that area |
| --- | --- | --- | --- | --- |
| 14 µm (16K, X) | about 2.0 billion triangles | ×100 | about 500 mm² | about 9 mm |
| 16.8 µm (P1, X) | about 1.4 billion | ×70 | about 720 mm² | about 11 mm |
| 24.7 µm (P1, Y) | about 650 M | ×32 | about 1,550 mm² | about 16 mm |
| 50 µm (a layer, or a Mono X pixel) | about 160 M | ×8 | about 6,400 mm² | about 33 mm |
| *today at 1.3 mm: 162.5 µm* | *15 M (measured)* | *within* | — | — |

- **Grid limit:** at 16.8 µm the grid over `Base_01.stl`'s box alone has about 1.2 · 10¹¹ cells,
  over the 2³⁶ ≈ 6.9 · 10¹⁰ cell limit. The run is refused before evaluating anything.
- **Time and memory:** scaled from 1.3 mm (15 M triangles in 11.4 s of hollowing), about
  1.4 billion triangles mean on the order of 15–20 minutes and hundreds of GB (*estimate*).
- **Worse than today for practical walls:** today's `wall / 8` cell is already coarser than a
  pixel for any wall over 8 pixels (about 0.13–0.2 mm), so a pixel cell multiplies the triangles
  exactly for the walls that are used. It is finer only below about 0.13 mm, where it still
  needs over a billion triangles.
- **Conclusion:** a pixel-sized cell fits option R, where the pixel is the voxel and nothing is
  meshed, or option A, where cells that fine are used only where needed, as the refinement's
  smallest size.

### A.3 A minimum wall from the pixel alone

Four different minima, only two of which depend on the pixel alone:

| Minimum | Depends on | Rule | P1 (16.8 / 24.7 µm) | 16K (14 / 19 µm) |
| --- | --- | --- | --- | --- |
| Admitted by the algorithm (`cell · √3 < wall`, one-cell leaves) | the pixel | wall > √3 · pixel | > 0.029 / 0.043 mm | > 0.024 / 0.033 mm |
| At today's relative precision (a priori band ≤ 22 %) | the pixel | wall ≥ 8 · pixel | ≥ 0.13 / 0.20 mm | ≥ 0.11 / 0.15 mm |
| Within memory and time | the cavity's area and the cell | `7.9 · A / cell²` ≤ the caps | per model (A.2) | per model |
| Physically printable | resin, exposure, geometry | not computable from the pixel | — | — |

**Reading the four minima:**
- **The first** is the true geometric limit. At that minimum the a priori band is almost the
  whole wall, and only the a posteriori certificate can still prove a positive wall.
- **The second** is today's rule read backwards: the thinnest wall worth asking for at
  today's precision is about 8 pixels, 0.13–0.2 mm. That is the order of CHITUBOX's 0.1 mm,
  which it breached.
- **Use:** a printer-only bound such as "at least 8 pixels", with √3 pixels as the absolute floor,
  can be shown before any hollowing, under either option.
- **What it does not cover:** it does not bound resources, which depend on the model's area, and
  it says nothing about what a resin can print.

### A.4 How the references size the cell (measured)

Black-box counts of CHITUBOX v1.4.0's hollowed `Base_01.stl` (vertices and triangles shown by the
application, less the source's) and Lychee's preview.
Cells are inferred from `1 / cell²` and from vertex counts; the two agree within 5 %.

| | Cell at 1.3 mm | Cell at 0.1 mm | Cavity at 1.3 mm | Cavity at 0.1 mm | Growth |
| --- | --- | --- | --- | --- | --- |
| CHITUBOX, low | about 0.43 mm (wall / 3) | about 0.10 mm (wall / 1) | 0.57 M | 9.9 M | ×17.5 |
| CHITUBOX, middle | about 0.26 mm (wall / 5) | — | 1.63 M | — | — |
| CHITUBOX, high | about 0.22 mm (wall / 6) | about 0.053 mm (wall / 2) | 2.37 M | 39.9 M | ×16.8 |
| Lychee, quality ≥ 2 | 0.65 mm (250-cell bound) | — (minimum 0.5 mm) | — | — | bounded |
| Karasu, `wall / 8` | 0.1625 mm | 0.0125 mm | 14.97 M | about 2.5 billion | ×169 |

**What the counts show:**
- **The cell follows the wall sub-proportionally.** From 1.3 to 0.1 mm the wall is 13 times
  thinner, but CHITUBOX's cell is only 4.1–4.2 times smaller. Its cavity therefore grows about 17
  times where Karasu's grows 169 times.
- **Thin walls get coarse cells.** At 0.1 mm CHITUBOX's cell is the wall or half of it, which
  fits the breached 0.1 mm wall measured at middle.
- **CHITUBOX emits fewer triangles per cell** (section 4, second lever).

