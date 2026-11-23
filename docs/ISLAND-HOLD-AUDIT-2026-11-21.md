# Island hold audit, 2026-11-21

## Scope and method

This is a read-only audit of the workspace at commit `3753ce1` (0.2.0-dev after the third beta),
requested by the owner. It checks two conditions, plus a related one:

- **C1.** An island must never be reported as held when it is not really held. "Reported" covers
  every count, list, marker, notification and summary line in the command line and the
  application. "Really held" means that in the printed layers (model plus supports as material) the
  island's first layer cures onto material connected down to the plate, and the connection does
  not run through another island that is itself not held.
- **C2.** Standard and mini supports (with their branches, merged trunks, braces, feet on the model
  and rafts) must never create islands. No support material may start in mid-air in the sliced
  layers, and no support may stand on material that is itself not held.
- **C3.** Islands the print will have that no tool finds or reports. An island that is never found
  is never reported as not held.

The audit ran in five steps. No repository file was changed. The scratch harness, the synthetic
models and the run outputs were kept outside the repository.

1. **Pixel truth.** A scratch harness calls the repository's own pipeline:
   - It plans automatic supports as `karasu-cli slice --auto-supports` does.
   - It slices the model alone and the final print.
   - It follows the final print's layers from the plate up. A piece (8-connected material) is
     plate-connected when one of its pixels lies within the self-supporting reach of a
     plate-connected supporting pixel of the layer below; layer 0 rests on the plate.
   - It judges every island the planner reports held against those layers.
   - It lists every piece made only of support pixels that starts in mid-air.
2. **Code review.** Six independent read-only lenses:
   - the hold rule;
   - the planners' counts;
   - the application's Islands tool;
   - the support geometry;
   - minis, braces and rafts;
   - detection and the printed file.
3. **Adversarial verification.** Every finding went to two independent verifiers, each asked to
   refute it: one by re-tracing the code, one on reachability and physical effect. Most findings
   were reproduced with the release CLI or on synthetic models.
4. **Completeness review.** It opened two follow-up lenses:
   - hollowed and drilled models: no new mechanism;
   - supported models moved, turned or scaled: three new findings.
5. **Solution design.** For each of seven problem clusters, three independent designs:
   - the smallest correct change;
   - the strongest guarantee;
   - the user's and the owner's view.

   A judge per cluster scored them and synthesized the alternatives, and one integrated plan joins
   the clusters. Several costs were measured on scratch prototypes; the others are marked as
   estimates.

33 findings were raised:

- **1 was refuted:** final-check-last-round-unchecked. The final check's third round cannot add
  supports.
- **2 were narrowed:**
  - The final-print analysis does absorb an island held only through an unheld island into that
    island's floating body. The floating body is still reported at its lowest start, so this is not
    a detection gap.
  - A piece split into several starts over-reports; it never produces a false hold.
- **The others were confirmed.** Some are consequences of recorded decisions (TD-108, TD-112,
  TD-131), and are marked so.

## Summary

| Condition | Result | In short |
| --- | --- | --- |
| C1 | **Not guaranteed** | See below. |
| C2 | **Violated with the defaults** | See below. |
| C3 | **Gaps confirmed** | See below. |

**C1: not guaranteed.**

- With the defaults, no measured real model had a false hold. That covers 11 configurations
  (table below) and 15 runs on hollowed and drilled models.
- Fifteen confirmed mechanisms can still report an unheld island as held.
- Reachable with the defaults:
  - the bounding-box and height-window rule;
  - other pieces' tips;
  - supports standing on unheld islands;
  - the application's tip-only holder;
  - unchecked kept supports;
  - out-of-date searches.
- Reachable only with other values or after edits:
  - the join distance (52 to 120 false holds on HillGiant);
  - negative XY compensation;
  - rebuild repair;
  - pose and scale changes of a supported model.

**C2: violated with the defaults.**

- Every automatic plan creates **11 to 24 support-made island starts**, 14 on HillGiant. All of
  them are mini support foot balls on steep walls.
- The verifiers also reproduced:
  - standard model-foot lips on faces steeper than about 70 degrees;
  - supports standing on unheld islands;
  - feet in drain holes printed by the application;
  - rising paths after hand edits or pose changes.
- Planner-made standard supports, automatic braces and rafts created no island.

**C3: gaps confirmed.**

- The printed layers (model plus supports) are never analysed by the application, and only on
  demand (`--unsupported`) by the CLI, with no reconciliation against the planner's counts.
  Support-made islands therefore go unseen.
- A separate piece that starts within reach of the layer below, but not on it, is never an island.

## Measurements

Pixel truth for HillGiant (`HillGiant_32mm.stl`) on the Anycubic Photon P1 (PP1), turned
80 degrees about X, automatic supports:

| Case | Planner held | Really false held | Support-made starts |
| --- | --- | --- | --- |
| Defaults | 263 / 307 | **0** | **14** |
| Join distance 1 mm | 276 / 307 | **52** | 12 |
| Join distance 3 mm | 293 / 307 | **120** | 8 |
| HillGiant upright | 475 / 612 | 0 | 17 |
| Saturn 4 Ultra 16K (GOO) | 276 / 323 | 0 | 24 |
| Second miniature, upright | 74 / 123 | 0 | 1 |
| Second miniature at 30 degrees | 57 / 88 | 0 | 3 |
| Heavy preset | 248 / 307 | 0 | 15 |
| Light preset | 269 / 307 | 0 | 18 |
| Mini density 25 % | 263 / 307 | 0 | 11 |
| **Minis off** | 126 / 307 | 0 | **0** |
| Zigzag or cross braces (245 braces) | as defaults | 0 | 14 (unchanged) |
| Hollowed, with and without 20 drain holes | 264 / 266 | 0 | 14 / 12 (all exterior mini feet) |

With the defaults the final print has 57 island starts. 43 are model islands the planner lists
as left. **14 are made only of support material**, and no tool lists them.

Other cases measured by the verifiers:

- **Rebuild repair at 0.2 mm:** 12 supports are left out of the print, and 3 islands are reported
  held that are not.
- **A lowered supported document:** 13 islands are reported held that are not.
- **A synthetic L next to a block, islands only:** the CLI reports "3 of 3" held; the print has
  the L's island.
- **Negative XY compensation:**
  - the Medium contact is lost at -6 px on the P1 and at -3 px on 50 µm panels;
  - Light mini rods break at -5 px;
  - every catalog setting uses 0.
- **An adaptive layer plan on a two-model plate:** the search found 5 starts and the print had 14.
  The plan reported "5 of 5" held, and 10 starts in the print had nothing under them.

## Findings

"Default" means reachable with default settings and ordinary actions. "Other" means it needs
non-default values, hand edits or rare states. File references are at `3753ce1`.

### C1: islands reported held when they are not

| # | Finding | Reach | Where | Effect |
| --- | --- | --- | --- | --- |
| H1 | **Bounding box and height window.** `holds()` accepts any tip inside the axis-aligned box of the start's first-layer pixels, in a z window `[under - layer, first + layer + join]`. With planes at slab centres that is `[c(k-2), c(k+1)]`, a whole layer above the start. `start_tips` probes the centroid first. For an L, C, ring or crescent the centroid lies off the pixels, so the probe hits the next layer's underside and the rule accepts it. | Default | `support_plan/islands/check.rs:74-82`, `islands.rs:346-384`, `found.rs:94-104` | The island's own support stands beside it. The CLI reports "1 of 1" held and the print keeps the island. The only unit test uses a square start, where box and pixels coincide. |
| H2 | **Other pieces' tips.** `held()` accepts any tip of the plan inside the box and window: another island's tip, an overhang tip nudged up to 1.25 mm, a mini, or a kept support. An island credited this way never gets a support of its own. | Default | `check.rs:84-88`, `islands.rs:815-833, 884-892`, `auto.rs:897-903, 1123-1144` | An island counts as held by material that never touches it. |
| H3 | **Held through an unheld island.** The hold rule looks only at the tip, never at what the support stands on. Standard model feet, mini feet and tree branches can stand on an island the same plan leaves unheld. | Default | `core supports/route.rs:517-540, 690-711`, `islands.rs:545, 647-697` | The upper island is counted held while its only way to the plate runs through floating material. |
| H4 | **Application holder by tip position only.** `holder()` returns the first document support whose world tip passes `holds()`. It ignores whether the support builds, whether the print leaves it out, and whether its tip is in a drain hole. | Default | `karasu-app/src/islands_ui.rs:985-993` | Green markers, "Held by #N", list rows, the free filter, the footer, the search notification, and the islands Support the islands skips (`islands_ui/plan.rs:90`). |
| H5 | **Kept supports in the Automatic tool.** Islands that a kept support "holds" are left out of "held N of M" and of the left list. The exclusion is by design (TD-112 point 3), but the credit passes no build or drain-hole check. | Default | `auto.rs:907-917, 1123-1144` | A wrong credit by a kept support is invisible. |
| H6 | **Out-of-date searches.** After a move, turn, lift, scale, hollow or drill, plans, the plan footer, Apply and Support it here use the old islands without saying so. Stale display is by design (TD-108 point 3), but these actions are not. | Default | `islands_ui.rs:96-103, 494-518, 556-563, 600-606`; `islands_ui/panel.rs:493-513, 603-605` | Rows turn green for islands that no longer describe the plate. |
| H7 | **Plans checked against the previous drain holes.** `PreparationUi::latest` does not compare operations, so right after a hole edit a plan is checked against the old cylinders and can be applied. | Default, short window | `preparation_ui.rs:498-532`, `check.rs:132-140` | A tip or foot in the new hole can be applied and counted. |
| H8 | **Join distance above zero.** The box widens by `join` and the window's top rises by `join`, and `groups()` shares one support per group. Members are credited without being touched. | Other: the default is 0. Before settings schema 2 the default was 1 mm, and the migration rewrites only an exact 1.0. | `check.rs:74-82`, `islands.rs:389-412`, `auto.rs:913, 1132` | 52 false holds at 1 mm and 120 at 3 mm on HillGiant. At 1 mm the really held islands fall from 264 to 222. |
| H9 | **Negative XY compensation.** The whole mask, supports included, is eroded after rendering. The search and the hold rule never see it. | Other | `core raster/render.rs:836-841`, `pipeline.rs:2030` | Contacts vanish under counted holds; broken rods also violate C2. |
| H10 | **Final check on the imported mesh.** The planners' final check builds supports on the imported mesh, while the job builds them on the repaired one. | Other: rebuild repair | `generate.rs:139, 179-199`, `check.rs:121`, `pipeline.rs:1784, 3077-3081` | At rebuild 0.2 mm, 12 supports are left out and 3 counted islands start in mid-air. |
| H11 | **Window against layer plans and penetration.** With variable layers the window covers more than one thin layer. A tip at the window's bottom holds only if its contact sinks to `c(k-1)`. Fitted contacts shrink their penetration, and so do thick layers. | Other | `check.rs:76-79`, `core supports/fit.rs:78-86` | Reproduced with fitted contacts under about 0.09 mm at 0.05 mm layers, and with 0.1 mm layers. |
| H12 | **Hidden islands.** With "Hide islands under" above zero, the footer says "Every island is supported" while hidden islands may have no support, even when every island is hidden. | Other | `islands_ui/panel.rs:616-633` | A universal statement over a subset. |
| H13 | **Islands filtered out of the Automatic counts.** Islands under the skip-under area, in the other place, or misclassified as cavity or outside at their centroid are dropped before counting and never listed. | Other | `auto.rs:907-917`, `found.rs:66-70, 91, 103` | "N of M" silently excludes them. |
| H14 | **Final check skipped without a word.** When the build fails as a whole, for example an impossible raft, the check returns without checking. The preparation-missing and no-model branches cannot be reached. | Other, rare | `check.rs:121-129, 195-197` | Unchecked supports are counted. |
| H15 | **Supported model moved, turned about X or Y, or scaled.** `SupportDocument::placed` moves tips, bends and feet but keeps junction heights in the world (TD-108). Junctions fall outside their parent's moved shaft, and branches, merged stems and mini branches are left out of the print. | Default: ordinary actions | `core supports/document.rs:1143-1174`, `branches.rs:62, 134-170` | Their islands stay listed as held (holder H4, kept tips H5). 13 false holds on a lowered HillGiant document. |

A minor gap that does not concern islands: overhang side tips "thinned" by a mini that the final
check later removes stay counted as covered (`auto.rs:1094-1117, 1145-1161`). The CLI then
misses a "Left unsupported: overhang" line.

### C2: islands made by supports

| # | Finding | Reach | Where | Effect |
| --- | --- | --- | --- | --- |
| S1 | **Mini foot ball on a steep wall.** `find_mini_foot` accepts any hit whose mean normal has a positive z. The ball's centre is placed at `point + n (r - p)`, and the build adds a full sphere. On a planar wall the pole leaves the model when `n_z < 1 - p/r`: walls steeper than 60 degrees for Medium (0.4 mm, 0.1), 48.2 for Light and 66.4 for Heavy. TD-132 fixed this effect for standard feet only. | Default | `core supports/route.rs:686-711`, `support_plan/route.rs:313-323`, `core supports/build.rs:795-803, 914-915` | **The only support-made source found in real runs.** Specks of 1 to 132 px, almost always a single layer, 0.145 to 0.193 mm below the foot point. Independently reproduced on wedges at 80 and 88 degrees. |
| S2 | **Standard foot lip.** On a face steeper than the axis limit, TD-132 turns the foot axis up by at most 59.5 degrees. The base disc stays tilted, and its downhill edge lies outside the material. | Default | `core supports/route.rs:524-534, 588-599`, `build.rs:632-673` | Through the planner with defaults: about 1, 5 and 7 of 18 planned Medium feet on 70, 80 and 85 degree faces leave a speck. |
| S3 | **Supports standing on unheld material.** Standard feet, minis and their branches can stand on an island the plan leaves unheld. This is the C2 side of H3. | Default | as H3; `islands/minis.rs:208-279`, `check.rs:228-251` | The supports and the island above them hang from a floating island. The build reports nothing. |
| S4 | **Feet in drain holes printed by the application.** FootInHole is an error, but under TD-108 the application slices supports with errors and warns. Drain cutters leave support material (TD-123). | Default: by design for the printing; not for the hold claim | `core supports/build.rs:1308-1324`, `project.rs:1243-1244`, `pipeline.rs:3317-3333` | The support starts in mid-air, and its island stays listed as held. |
| S5 | **Rising paths.** Only a plate base is checked against the path above it. A branch junction dragged above the branch's tip join, or a support base on a support, may rise. The steep-shaft test uses the absolute rise. | Other: hand edits | `build.rs:560-604, 675-687, 711-722`, `karasu-app/src/supports_ui/handles.rs:338-353` | The join ball starts in mid-air with no finding. |
| S6 | **Thick own-diameter braces.** A brace with its own diameter ignores its shafts. On a slanted shaft segment its lower end ball's south pole falls outside the shaft when `r sin(phi) > r_shaft`. | Other | `core supports/document.rs:730-739`, `build.rs:955-991` | A speck under the lowest brace ball. |
| S7 | **Wide junction balls.** Merge trunks and Join a support hang a wider support on a thinner slanted shaft. The no-thicker-than-trunk rule applies only to hand branches, panel edits and junction drags. | Other | `build.rs:557, 675-686`, `karasu-app/src/supports_ui/merging.rs:49-76` | The junction ball's bottom cap prints in mid-air. |
| S8 | **Pose changes with world junction heights.** As H15: when a supported model is lowered, turned or scaled, branches and merged stems rise to junctions that stayed in place. | Default: ordinary actions | `core supports/document.rs:1170`, `branches.rs:156-158` | Tip-join or bend balls start in mid-air (0.03 to 0.7 mm² over 5 to 7 layers). They build with no error and print in the application and in `slice --project`. |
| S9 | **Scaled-down models.** Tip and foot lengths are world values, so supports standing on the model rise when the model is scaled down. | Other: 70 % or less on HillGiant | `core supports/document.rs:1138-1139, 1166-1174`, `build.rs:494, 648` | A mid-air start with no finding. |

### C3: islands no tool finds

| # | Finding | Reach | Where | Effect |
| --- | --- | --- | --- | --- |
| D1 | **The printed layers are never analysed.** The tools see only the model sliced alone. The application has no check of the print; the CLI's `--unsupported` is opt-in, does not attribute starts to supports, and is not reconciled with the planner's counts. | Default | `generate.rs:377-403`, `islands_ui.rs:4-11, 955-967`, `auto.rs:42-47` | HillGiant's 14 support-made starts appear nowhere. |
| D2 | **Proximity anchoring.** `out_of_reach` anchors a piece as soon as any of its pixels lies within the reach disc of supporting pixels below, whichever piece those belong to. There is no overlap or connectivity requirement. | Default | `core analysis/unsupported.rs:287-313`, `runs.rs:56-70` | Pieces that start beside material are never islands. On real models such hidden pieces were left with no contact below: 4 of 5 on the second miniature, 4 of 4 on HillGiant at 30 degrees, 2 of 2 and 2 of 3 on the 16K. |
| D3 | **Faint or corner links.** Pieces join through any material pixel (gray 1 and up) and at corners. A faint anti-aliased pixel or a diagonal touch can link a piece to held material. Partly by design under TD-107. | Default, with anti-aliasing | `pipeline/unsupported.rs:148-151`, `core analysis/unsupported.rs:303-308`, `runs.rs:162-164` | On HillGiant, 11 of 12 such hidden starts are pieces joined in their own layer only through pixels under half coverage. |
| D4 | **One piece, several starts.** Depending on row order, one piece can become several starts, each with a partial box. | Default | `core analysis/unsupported.rs:226-251` | Over-reporting only: duplicate "left" lines and extra supports. Never a false hold. |
| D5 | **Adaptive plans on multi-model plates.** The search keeps the plate's adaptive plan but slices one model, so its planes are not the print's. | Other | `pipeline.rs:1355-1361`, `generate.rs:384-392` | See the two-model plate in Measurements. |

### Checked without findings

These claims were checked and found correct. A fix must keep them true.

- **The hold rule with the defaults.** With join 0 the rule is TD-167's strict one. On HillGiant every island the planner counts held is really held at its first layer, chains included, and every island it leaves is reported with a reason.
- **Frames and heights.**
  - Every call of `holds`/`held` compares world tips with world islands.
  - The pixel-to-world conversion is exact under mirror and swap.
  - Planes are slab centres, and the heights are consistent.
- **Search and print agree.** The search uses the print's placement, lift, layer plan, format, hollowing and drain holes. Dropping the plate's other objects hides no island.
- **Counting and the final check.**
  - Counting runs after the last final check.
  - The final check removes errored planned supports, tips in drain holes and everything hanging from them.
  - Its three rounds leave nothing unchecked.
- **Drain holes.** Planned tips and feet in holes are refused (TD-131).
- **Planner geometry.**
  - Planner paths always descend.
  - Junction balls of planner branches start inside the parent shaft.
  - Mini tip balls and rods do not hang.
  - Mini branches stay one level deep.
- **Braces.** At their automatic diameter, braces never create islands, and their levels never go below a shaft.
- **Plate-side parts.** Pads, cones, raft fillets and every raft shape (slab, skate, web) start on the plate.
- **Rasterization.**
  - Bottom-layer XY compensation cannot disconnect supports.
  - Binary support rasterization (TD-134) keeps thin contacts connected.
  - Supports are always united with the model.
- **Hollowed and drilled HillGiant, 15 runs.** No false hold, no held island floating, no support on floating material. Interior supports create no start.
- **Other rules that hold.**
  - Grid mode makes no hold claim of its own.
  - Supports of another model never count.
  - A plan is never applied to a changed plate.
  - Out-of-date searches are flagged where counts are given.
- **The CLI in model mode.** It never prints a support with errors, nor silently leaves one out.

## Proposed solutions

### One definition of "held"

Today each part of the program uses its own proxy: a box, a window, a reach. The proposal is **one
rule, implemented once in `karasu-core`**. The island search, the hold test, grounding and the
print check all call it.

- **Material:** gray >= 1.
- **Cured:** gray >= the analysis's half-coverage threshold, `minimum + ceil((255 - minimum) / 2)`
  (`pipeline/unsupported.rs:151`).
- **Pieces:** 8-connected cured pixels. A faint pixel belongs to a cured piece it touches but
  never joins two.
- **Attachment at layer k:** a cured pixel on, or touching by an edge or a corner, a cured pixel
  of a grounded piece one layer below. It is capped by today's reach disc, so the rule is never
  looser than TD-107.
- **Grounded:** layer-0 pieces, and pieces attached to a grounded piece one layer below.
- **Held:** the island's first-layer cured pixels belong to a grounded piece of the printed layers
  (model, supports, raft, after compensation and quantisation) at the same plane.
- **Never held:** a plane that does not exist in the print, material that is missing, an
  incomplete or unverifiable check, eroded contacts, and starts joined only through partly lit
  pixels.

This rule alone closes D2. D3 needs a test print to settle whether partly lit pixels bond
(owner decision 1).

### A. The hold test

Findings: H1, H2, H5, H8, H9, H11.

| Alternative | Pros | Cons |
| --- | --- | --- |
| **A1, recommended. Pixel hold test in the slice's terms.** See the description below. | See below. | See below. |
| A2. A1 delivered reporting first, then planners | The first commit of A1; about half the code (900-1,100 lines). | Until the planner half lands, more islands are reported left instead of supported. The proxy survives inside the planners. |
| A3. A conservative analytic contact rule, no section raster | 600-900 lines; microseconds per pair. | A second geometric model of the supports. A later change to `build.rs` can silently break C1. Recommended only as candidate ordering. |

**A1 in detail.**

- **What it tests.** The support pixels the print will contain, computed from the built scene's
  convex parts with the rasterizer's own rules:
  - the TD-134 centre-most sample;
  - strictly inside at both z and z - h/4, which covers a vertex-avoiding plane move;
  - whole pixels when supports are anti-aliased;
  - compensation applied to the support pixels alone, which is sound because erode(S) is a subset
    of erode(M u S).
- **Verdict.** The support holds the start when its pixels at layer k-1 attach to the start's own
  first-layer piece. Outcomes are Held, NotReached, Eroded and Unverified; only Held counts.
- **Probing and credit.**
  - `start_tips` probes only the piece's own pixels.
  - Kept supports pass the same test and are counted separately ("holding K islands").
  - Braces and rafts never count as holders.

**A1, pros.**

- Exact where it matters, conservative where it cannot be.
- No extra slice.
- Measured on HillGiant: 13-32 ms for 307 islands, 28-60 ms upright.
- `start_tips` on own pixels takes 4.5 ms against 128 ms today.
- Measured on scratch prototypes: it matches pixel truth on HillGiant and on the second miniature,
  and refuses all 54 false credits at join 1 mm.

**A1, cons.**

- 1,600-2,000 lines.
- The sampler must stay in step with `render.rs`, guarded by a differential property test.
- Plans change where the box over-credited.

**The join distance** should be retired. With a physical credit it shares nothing, and at 1 mm it
lowers real holds from 264 to 222.

### B. Grounding

Findings: H3, S3.

| Alternative | Pros | Cons |
| --- | --- | --- |
| **B1, recommended. Piece graph and grounded settle.** See the description below. | Exact on every measured case. 0.5-2.5 ms per plan, no extra slice. Default plans stay byte-identical where nothing floats. | About 850 lines. On interleaved models (hair, chains) box ambiguity can refuse valid feet: fewer holds, never false ones. Not yet measured there. |
| B2. B1 plus a mandatory printed-layer check | The strongest guarantee. | Overlaps cluster D, which provides the check. |
| B3. Grounded reports only, plans unchanged | Lowest risk; byte-identical files. | Leaves C2 open: supports keep standing on unheld islands. |

**B1 in detail.**

- **The graph.** The search records every piece's parents and bounds: about +0.1 s and 0.3 MB on
  HillGiant.
- **The evaluator.** It grounds from the plate up:
  - parents propagate grounding;
  - a start the hold test holds grounds its piece;
  - a foot grounds only when every candidate piece under it is grounded and a drain-aware ray down
    stays in the model.
- **The settle.** After the plan's final check, ungrounded planned supports are removed with what
  hangs from them. The plan then regrows once, lowest first, with grounded-only feet, followed by
  a remove-only settle.
- **Reporting.** An island that can only be held through an unheld one is reported with its root
  ("#12, on island 5, not held"). Hand-made and kept supports on unheld material are flagged and
  never credited, but not removed.

### C. Support geometry

Findings: S1, S2, S4, S5, S6, S7.

| Alternative | Pros | Cons |
| --- | --- | --- |
| **C1, recommended. Rest by construction, a "starts in mid-air" error, editor guards.** See the description below. | See below. | See below. |
| C2. The same check, keeping the ball and adding a frustum sole or seat under hanging mini balls | Existing primitives only for minis. Sliced as model geometry it gave 0 starts in 720 wedge cases and took HillGiant from 57 to 43 starts. | Wider marks: 0.42-0.44 mm. Standard feet still need the loft. |
| C3. Re-place, refuse or sink hanging feet | The least code (200-300 lines). | Refusing loses 44-61 of HillGiant's 163 mini feet, up to 61 held islands. Sinking makes pits of 0.185-0.277 mm (standard feet 0.35-0.4 mm), against the owner's preference for nubs over pits. |

**C1 in detail.**

- **Feet.** A two-ring loft replaces the foot only where today's shape would hang: mini balls whose
  pole leaves the model, and standard feet turned by TD-132.
- **Braces.** Own-diameter brace cylinders are inscribed in their end balls.
- **The error.** One general "starts in mid-air" error that `accept_collision` cannot accept. It is
  checked part by part, in build order. A part must rest on:
  - the model as sliced, where drained material counts as empty;
  - the plate or the raft;
  - or a part already shown to rest.

  The tolerance is half the printer's smaller pixel pitch. Paths must strictly descend.
- **Editor guards.** The junction and knee handles are clamped. The no-thicker-than-trunk rule is
  extended to Merge trunks, Join a support and planner branches.

**C1, pros.**

- No deeper penetration.
- The smallest marks:
  - a Medium mini about 0.37 mm wide on vertical walls, against 0.346 today;
  - turned standard feet 0.200 mm deep instead of 0.274.
- Plans barely change: 159 of 163 minis kept in place on HillGiant.
- The check costs about 13 ms on HillGiant.

**C1, cons.**

- A new hull primitive, and mixed mini foot shapes.
- The check must raise no false alarm on tessellation slivers beside joint balls: zero findings on
  planner output across presets and printers before release.
- The lofts have not yet been sliced as binary support parts. A pixel sweep and a marks test print
  are needed.

**TD-108 and TD-131.** The proposal is that a support that starts in mid-air or stands in a drain
hole (FootInHole), and what stands on it, refuses the slice in the application and in CLI project
mode, naming the supports. Other error kinds keep print-and-warn. The TD-131 rim refinement must
ship with it: a foot near a hole's rim that still rests on material is not an error.

### D. Verification on the printed layers

Finding: D1. It also backs every C1 and C2 finding.

| Alternative | Pros | Cons |
| --- | --- | --- |
| **D1, recommended. A print check in every slice with supports.** See the description below. | See below. | See below. |
| D2. D1 plus a verification slice in the tools | Truth in the Supports step before export. | +2.1-3.5 s per Find islands or Generate on HillGiant; more on large models. |
| D3. D2 plus a closed loop in the planners | C2 guaranteed for plans whatever geometry bug remains. | Until C lands, it removes 14-20 minis per HillGiant plan that hold real islands. One or two more slices per plan. Keep in reserve. |

**D1 in detail.**

- **Where it runs.** On the exported masks the job produces anyway, on its own thread.
- **How it checks.** It grounds the printed layers from the plate up with the one definition. It
  gives each island found a verdict: Held, StartsInMidAir, HangsFrom(root), NotMaterial,
  NoSuchPlane or Incomplete; only Held counts.
- **Attribution.** It attributes every support-made start to its support and part.
- **The tools.** They say "planned, not checked" unless the revision has been checked.

**D1, pros.**

- No extra slice.
- 0.25-0.55 s of one core on HillGiant, overlapped with the export: wall +0 to 0.37 s.
- Peak memory of the search and `--unsupported` falls by about 260 MB, because no pass keeps the
  masks any more.
- Print files stay byte-identical.
- Every CLI number becomes verified, whatever the mechanism behind an error.

**D1, cons.**

- In the application, the green state comes only after a Slice, unless A, B and E land too.
- C2 defects are reported, not prevented.
- It needs a third marker state.

**The CLI** keeps `Automatic supports held: islands N of M` with verified numbers. New lines
appear only when non-zero:

- failed holds, with the reason;
- islands left by the plan but held in the print;
- support-made starts and supports on unheld material, with their ids.

`--no-print-check` turns it off, and `--require-held` makes contradictions fail the job, keeping
the file. Once C brings support-made starts to zero on the test corpus, those starts and supports
on unheld material become CLI errors (file kept, as TD-095 point 5).

**In the application,** the check's warnings go to the Slice results, which open the Layers tool
with the start ringed. When the last Slice matches the plate's revision, the Islands and
Automatic tools show its verdicts.

### E. Honest reports in the application and the CLI

Findings: H4, H5, H6, H7, H10, H12, H13, H14, the side-tip gap, and the hold claim of S4.

| Alternative | Pros | Cons |
| --- | --- | --- |
| **E1, recommended. One headless hold ledger.** See the description below. | Closes all ten findings. Removes the per-frame O(islands x supports) scans (about 36 ms per frame at 3,000 islands and 5,000 supports). Default CLI output stays byte-identical. | 900-1,300 lines, plus strings and translations. Short neutral Checking states after edits. |
| E2. Patch each statement in place | About 200 lines. | Fails C1 for stale searches, filtered counts and the wrong surface. Two copies of the rule can drift. |

**E1 in detail.**

- **Where it is computed.** Off the UI thread, from the support build the application already
  makes.
- **States.**
  - Held (by #N; kept or not);
  - Free (with the reason);
  - NotCovered (skip area, other place);
  - PartlyLit, Checking, OutOfDate, Unverified;
  - Hidden, as a flag.
- **Invariant.** found = held + free + not covered + neutral.
- **Currency.**
  - Searches and plans are keyed per object and to the exact preparation, operations included.
  - Out-of-date islands are neutral, and U and Support it here are disabled until Find runs again.
  - The final check, the builds and the ledger use the mesh the job slices.

**Small fixes worth doing first:**

- "Every island shown is supported; K hidden have no support";
- Automatic counts that name skipped, other-place and kept-credited islands;
- "Automatic supports final check: not run: <reason>";
- a recount of thinned side tips after the final check;
- plan keys that carry the search serial and the preparation generation.

### F. Island detection

Findings: D2, D3, D4, D5.

| Alternative | Pros | Cons |
| --- | --- | --- |
| **F1, recommended. Capped contact, one start per piece, plate planes.** See the description below. | Never looser than today on any printer, layer height or angle, by construction. Measured: +0/+1 starts per model alone on the P1 and +0/+3 on the 16K, all 0.065 mm² or less. Cost-neutral. | One switch and one state in the interface. Counts change slightly everywhere. |
| F2. Partly lit pixels never bridge, and are planned like any island | The strongest detection guarantee, no new UI. | On anti-aliased settings (P1, D2, Photon Ultra) +6 to +48 starts per model: single-layer lobes, each a mark if supported. |
| F3. Capped contact and one start per piece only | Core only; ships soonest. | Leaves a measured ambiguity unreported. |

**F1 in detail.**

- **Anchoring.** Through cured pixels, capped by the reach disc (the one definition).
- **Starts.** One start per 8-connected piece.
- **Planes.** On adaptive multi-model plates the search uses the plate's planes.
- **Partly lit pixels.** Starts joined only through partly lit pixels form a listed, unplanned
  class until a test print settles whether they bond.

### G. Supported models moved, turned or scaled

Findings: H15, S8, S9.

| Alternative | Pros | Cons |
| --- | --- | --- |
| **G1, recommended. Junction heights carried, descent rule, built-only holds.** See the description below. | About 55 lines plus tests. No format change, deterministic: 5e-14 mm over ten drag steps. Exact for lifts and uniform scales. | On tilts of 20-45 degrees it leaves out 3-18 of 455 supports more than G2, all reported. It amends TD-108's "heights on shafts are heights above the plate" for junctions. |
| G2. Junctions stored as model-frame points | Fewest supports lost on large tilts. | A format change: 0.2.0-beta.3 refuses new documents that hold a branch. About 3-4 times the code. |
| G3. G1 plus a prompt before tilting a supported model, like the Orient tool (TD-119) | The user decides before supports are lost. | Friction on small corrections. |
| G4. Checks only; heights stay in the world | About 100 lines. | Ordinary actions lose many supports on HillGiant: 92 when lowered 2 mm, 122 at 85 % scale, 87 of 258 when Generate re-lifts an Islands-tool plan. |

**G1 in detail.**

- **The carry.** Junction heights are carried in `Workspace::apply` with a scaled drop:
  z' = to(r).z - sigma (from(r).z - z), with sigma = |T_lin ẑ|.
- **The descent rule.** Every standard and mini path must strictly descend. Otherwise the build
  reports InvalidPath, builds nothing, and its children fail with InvalidParent.
- **Holds.** Only built supports hold.

### Integrated plan

The recommended path is about 7,500 lines plus tests, in nine phases (P0 to P8). Each phase is one
pull request with native Windows validation. Much of the value arrives early, so the phases fall
into three stages.

**Stage 1: no unverified "held", and supports stop creating islands.** About 4,000 lines.

- **P0. Decisions, a test print and decision records.**
  - The owner decisions below.
  - A P1 test print at anti-aliasing 16 with two plates:
    - lobes beside a wall joined by one pixel of gray 17/51/85/119/136, at 0/1/2 pixels from the
      wall, and diagonal-only contacts;
    - today's mini balls, lofts and soles, and turned standard feet, on 60/75/85/90 degree walls,
      to compare marks.
  - Decision records, numbered when written:
    - what "held" means;
    - the print check;
    - every support part rests;
    - island reports.
  - Fixes for the documentation drift: `UI-DESIGN.md:962-963` (the pre-TD-167 hold rule),
    `TECHNOLOGY-DECISIONS.md:2081` (hidden islands "stay counted"), and the join entry in
    `CLI.md`.
- **P1. Quick wins** (500-700 lines, default CLI output byte-identical).
  - Retire the join distance; `--auto-settings` refuses a non-zero value.
  - True universal statements and complete Automatic counts.
  - The final check returns a Result, with a "not run" line.
  - The side-tip recount.
  - Plan keys with the search serial and the preparation generation; Generate waits for the exact
    preparation.
  - Inscribed own-diameter braces.
  - Editor guards.
  - Closes H7, H8, H12, H13, H14 and the side-tip gap.
- **P2. Core foundation and the one rule** (800-900 lines).
  - First, with output unchanged: a streaming `find_unsupported` that keeps two layers, the start
    pieces (an index on `IslandStart`, which stays Copy) and the piece graph.
  - Then the one definition, with the partly lit class.
  - Peak search memory on HillGiant falls from about 390 MB to 130 MB.
  - Closes D2, and D3 in the core.
- **P3. The print check** (1,300-1,600 lines). See D1. Closes D1.
- **P4. Supports rest by construction** (950-1,100 lines). See C1 and the TD-108 refusal. Gate: 0
  support-made starts over the corpus and layer-phase sweeps (P1 and 16K, 45-89 degree wedges,
  braces 1.5-3 mm). Closes S1, S2, S4, S5, S6, S7.
- **G1. Junction heights carried and the descent rule.** Closes S8 and S9; H15 together with
  P5/E1.

After stage 1, every CLI number is verified on the printed layers. The application states truth
after a Slice and "planned" before it. Automatic plans create no island.

**Stage 2: truth before slicing, and better plans.**

- **P5. The hold ledger with the pixel hold test** (1,500-2,000 lines, the largest phase).
  - The job's mesh and per-object currency.
  - Closes H1, H2, H4, H5, H6, H9, H10, H11.
- **P6. Grounding** (about 850 lines). Closes H3 and S3.
- **P7. Planners plan against the true rule.**
  - One start per piece; the TD-167 tables measured again.
  - Falsely credited islands get supports instead of only being reported left.
  - Closes D4.

**Stage 3: rare cases.**

- **P8.** Plate planes for adaptive multi-model searches, and the partly lit default from the
  test print. Closes D5.

**Runtime.**

- Tens of milliseconds per plan.
- 0.25-0.55 s of overlapped CPU per HillGiant slice.
- About 260 MB less peak memory in searches.

**Outside the plan's guarantee:**

- peel forces, cure depth and resin behaviour;
- the short window after an edit, when no claim is made;
- adaptive plans on the P1, whose firmware ignores per-layer heights (issue #35): the file is
  verified, not the physical layer heights.

## Owner decisions

Recommendations in parentheses.

1. **What "held" means.**
   - Attachment between layers: (a) today's reach; (b) capped contact; (c) overlap only.
   - Partly lit pixels: they bridge, they never bridge, or they form their own listed class.
   - (b), with partly lit starts listed and not planned until the test print settles them. One
     judge preferred (c) as the literal reading of "cured onto".
2. **TD-108 for supports that start in mid-air or stand in a drain hole.**
   - Refuse the slice and name them; leave them out with a warning; or print them with a warning,
     which breaks C2.
   - (Refuse, with the TD-131 rim refinement. Other errors keep print-and-warn.)
3. **Print check policy.**
   - Scope: jobs with supports, or every job.
   - A CLI switch to turn it off.
   - Report only, or fail.
   - Whether the tools slice for verification.
   - (Every job with supports and every application Slice, with `--no-print-check`, and no tool
     slices. Report only, with `--require-held`, until P4's gate; then support-made starts fail
     the CLI job, file kept.)
4. **Before slicing.**
   - (a) The tools say "planned" and turn green after a Slice.
   - (b) The ledger gives a trustworthy green at once.
   - ((a) in stage 1, (b) in stage 2.)
5. **Join distance.**
   - Retire it; keep it as planning order only; or later turn it into "one contact touching
     several islands".
   - (Retire it.)
6. **Foot shapes and the rest rule.**
   - Mini feet: a loft only where the ball would hang; a loft on every mini foot; a sole or seat;
     or a sunk ball.
   - Turned standard feet: a loft, refusal or sinking.
   - The rest rule's tolerance: half the printer's smaller pitch, or a fixed value.
   - (A loft only where needed, a loft for turned standard feet, on every document, with half a
     pitch. Confirm the marks with the test print.)
   - **Decided for mini feet on 2026-11-23 ([TD-169](TECHNOLOGY-DECISIONS.md)):**
     - The tools take a foot only where its ball rests: re-placed in the fan, or refused with a
       reason. The shape does not change.
     - A mini placed by hand whose foot hangs is reported with a warning.
     - Closes S1 for planned supports. Standard foot lips (S2) and the general check stay open.
7. **Grounding policy.**
   - Planned supports on unheld material: remove and regrow, or report only.
   - Hand-made and kept supports there: flag and never credit them, or refuse them.
   - Grid-mode feet on the model.
   - (Remove and regrow. Flag hand-made and kept supports, never credit them, never remove them.
     Grid mode runs the model-alone search when it places feet on the model, +2-5 s on HillGiant.)
8. **Currency and display.**
   - Out of date: neutral with U and Support it here disabled, or an automatic re-search.
   - A neutral Checking state.
   - Islands held through an unheld island: red with the reason, or a third colour.
   - (Neutral and disabled; Checking in a configurable neutral colour; red with the reason, and
     "Without a support" renamed "Not held".)
9. **CLI wording.**
   - Keep `Automatic supports held: islands N of M` with true numbers, or rename it, which breaks
     scripts and `karasu-cli/tests/slice.rs`.
   - (Keep it; kept holds counted separately.)
10. **Errors that keep the material.** Collision, piercing and axis deviation: do they still
    count as holding? (Yes. OutsideVolume gets no credit and stays printed.)
11. **Negative XY compensation.**
    - Report Eroded and make supports thinner than the erosion a build error.
    - Later, wider contacts at island starts.
    - Exempt supports from erosion, which changes TD-062.
    - (The first now, the second if users need it.)
12. **Pose and scale changes of a supported model.**
    - Carry junction heights, which amends TD-108; store model-frame points, a format change; or
      ask first before tilts.
    - Should Fit to plate and Drop to plate keep the lift of a supported model?
    - (Carry, with the descent rule. The tilt prompt is the owner's call.)
13. **Delivery.** (One PR per phase with native Windows validation, P1 first; P3 and P4 can be
    developed in parallel.)

Minor, with the designers' recommendation:

- checks on the job's repaired mesh, routes on the imported one;
- plate planes for adaptive multi-model plates;
- one start per piece;
- unverified outcomes never count as held.

## Measurement plan

**How to measure.**

- Release build, one heavy job at a time on an idle machine, three runs, median reported.
- Final numbers repeated on native Windows with the hash-verified snapshot runner.
- Correctness is judged by the audit's pixel truth until P3, then by the product's own print
  check, cross-validated once against the pixel truth.

**Corpus.** The Measurements table above, plus:

- rebuild repair;
- a lowered document;
- negative compensation;
- synthetic L, ring and wedge cases;
- feet in drain holes;
- a stress plate of four copies.

**Performance.**

- CLI `--auto-supports` time;
- search and analysis time;
- peak RSS of searches and `--unsupported`;
- frame time with the Islands tool open on many islands.

**Physical.** The P0 test print for partly lit pixels and foot marks.

## Open risks

- **The definition is still a model of curing.** Whether one-pixel contacts and partly lit pixels
  bond depends on the resin; only the test print settles it.
- **The pre-slice states depend on the sampler.** The hold test's sampler must stay in step with
  `render.rs`. A drift would make pre-slice green states wrong; the print check would then
  contradict them.
- **The rest check can be wrong both ways.** It may flag harmless tessellation slivers, or miss
  slivers below its tolerance. The new foot shapes still need pixel sweeps and a print.
- **Old projects can be refused.** Refusing mid-air and in-hole supports blocks old projects with
  rising paths, thick braces or feet near hole rims until they are edited. Projects saved with
  stale junctions cannot be recovered: they open with those supports reported as left out.
- **Plans and counts change** where the old rule over-credited. That needs release notes.
- **Extreme inputs are not measured yet:** thousands of islands, 16K plates of 3,000 layers,
  rebuilt meshes of 14 M triangles, and interleaved models.
