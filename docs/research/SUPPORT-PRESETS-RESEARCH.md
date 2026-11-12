# Support presets: published guidance and the venom torso print (research, 2026-11-06)

**Purpose:** prepare the review of karasu-slicer's support presets (Light, Medium, Heavy), the
automatic tool's defaults and its braces, after the owner's print of the venom torso v4, whose
supports were hard to remove (ROADMAP, M6.6). This record is research only. It changes no value,
and it sets no default. The values come in a technology decision the owner approves, after the
test print proposed below.

**Method:** public documentation, manuals and articles found by web search on 2026-11-06 and read
as web pages. PrusaSlicer's command-line manual is documentation of its behaviour. Values are behavioural references, not copied designs.
Statements are marked **observed** (read in a source or measured in our files), **inferred**
(deduced from observations) or **hypothesis** (to be tested by a print).

## The print (owner's evidence)

**E-SUP-01, the venom torso v4** (owner, printed 2026-11-05, reported 2026-11-06; project
`venom_torso_2.karasu` saved 2026-11-05 15:53, sliced as `venom_torso_2_v4.pp1` 15:54):
- **Conditions (observed in the project):**
  - Photon P1, Anycubic Standard Resin, print setting "Normal 0.05 mm";
  - exposure 2.0 s, 5 bottom layers at 30 s;
  - stored PP1 (before TD-133);
  - anti-aliasing 16 on the model **and on the supports** (before TD-134), lowest grey 0;
  - the model hollowed and drained, lifted 5 mm.
- **Conditions (owner):** the printer's smart lift on (its smart power-off turned off); the
  supports were removed after washing and **before** the UV cure.
- **Supports (observed in the project):**
  - 1,973 supports in all: 1,964 automatic ones with the Heavy preset and 9 from the Islands tool;
  - Heavy's values: a cone contact of 0.6 mm sunk 0.3 mm (6 layers of 0.05 mm), fitted down to
    0.2 mm at edges (TD-116), a tip 1.4 mm wide where it meets the shaft and 3.0 mm long, a
    1.4 mm shaft, 7 mm pads;
  - the 9 islands supports: contact 0.15 mm, sunk 0.15 mm, a 0.4 mm shaft;
  - bases: 1,382 on another support (branches), 319 feet on the model, 272 on the plate;
  - 2,603 braces, 1.32 for each support.
- **Outcome (owner, photos):**
  - the model printed whole, the flat cavity ceiling included;
  - no failed layer and no support that came away;
  - the supports were hard to remove;
  - many small marks are left across the underside.
- **Not comparable (owner):** the reference slicer's file of the same torso
  (`venom_body_full_plate_1.pp1`) was prepared by someone else, at Z = 0 with its raft fused to
  the model and many supports fused underneath, so it is no baseline for removal.

## Sources

Accessed 2026-11-06. The confidence column rates the source for the values cited, not the
author.

| ID | Source | Kind | Confidence |
| --- | --- | --- | --- |
| S1 | [PrusaSlicer manual page (Debian)](https://manpages.debian.org/testing/prusa-slicer/prusa-slicer.1); [Ubuntu](https://manpages.ubuntu.com/manpages/focal/man1/prusa-slicer.1.html) | the slicer's own command-line documentation | high (defaults of that version) |
| S2 | [Prusa help: tree supports](https://help.prusa3d.com/article/tree-supports_1515) | vendor documentation | high, for its example only |
| S3 | [J3DTech guide, Introduction to Supporting (Mango3D docs)](https://doc.mango3d.io/doc/j3d-tech-s-guide-to-resin-printing/print-settings/introduction-to-supporting/) | a guide on the Lychee vendor's documentation site | medium (a guide's presets, not the slicer's shipped defaults) |
| S4 | [All3DP: J3DTech guide](https://all3dp.com/1/j3dtech-guide-to-resin-3d-printing/) | magazine, an older edition of S3 | low to medium (older values) |
| S5 | [Mango3D docs: manual supports](https://doc.mango3d.io/doc/resin-documentation/resin-support/manual-supports/) | vendor documentation | high for definitions, no values |
| S6 | [CHITUBOX docs: configure support parameters](https://docs.chitubox.com/en-US/chitubox-basic/latest/ui-and-features/configure-support-parameters) | vendor documentation | high for definitions |
| S7 | [CHITUBOX Academy: failed printing](https://docs.chitubox.com/en-US/academy/all-you-need-to-know-about-failed-printing) | vendor article | medium (Light and Medium defaults of one version) |
| S8 | [Formlabs: Supports V2](https://formlabs.com/support/what-are-supports-v2/); [PreForm 3.43](https://formlabs.com/blog/preform-3-43-update/); [PreForm 2.19](https://dental.formlabs.com/blog/preform-219-faster-surgical-guides-smaller-touchpoints/) | vendor documentation (an SLA printer, not MSLA) | high for that system, medium as a transfer |
| S9 | [AmeraLabs: support experiment](https://ameralabs.com/blog/experiment-3d-printing-supports/) | resin maker's controlled test, two printers | medium (approximate sizes, no counts) |
| S10 | [Mr Resin: supports guide](https://mrresin.es/en/blogs/mr-resin-3d-blog/supports-in-resin-3d-printing-definitive-guide) | resin seller's guide | low to medium (ranges, an inner inconsistency) |
| S11 | [Resin & Rolls: Lychee settings for miniatures](https://resinandrolls.com/guides/lychee-slicer-settings-miniatures/); [CHITUBOX settings for miniatures](https://resinandrolls.com/guides/chitubox-settings-miniatures/) | hobby guides for miniatures | low to medium |
| S12 | [Anycubic: supports hard to remove](https://store.anycubic.com/blogs/3d-printing-guides/3d-printing-supports-hard-to-remove-reasons-and-solutions) | printer maker's article | medium (reasons, no numbers) |
| S13 | [jewelsprint: support settings](https://www.jewelsprint.com/jewelry-3d-printing-support-settings/); [dreaming3d: support settings](https://dreaming3d.net/blogs/news/ultimate-resin-support-settings-guide-stop-failed-prints-2026); [3dprinterstuff: miniatures](https://www.3dprinterstuff.com/workshop/3d-printing-miniatures-resin-settings-perfect-detail) | seller and hobby guides | low (spacing rules of thumb) |
| S14 | [3D Printerly: anti-aliasing](https://3dprinterly.com/how-to-use-anti-aliasing-for-resin-3d-printing-best-settings/); [HeyGears: layer lines](https://store.heygears.com/blogs/blog/how-to-deal-with-layer-lines-in-resin-3d-prints) | hobby and maker articles | low to medium |
| S15 | [Grid bracing (Wikipedia)](https://en.wikipedia.org/wiki/Grid_bracing); [yidimu: supports](https://www.yidimu.com/news/340-en.html) | structural mathematics; a maker's guide | medium for the principle |
| S17 | [AmeraLabs: settings guide for beginners](https://ameralabs.com/blog/the-complete-resin-3d-printing-settings-guide-for-beginners/); [ThreeDimensionPrinters: hollow models and suction cups](https://threedimensionprinters.com/hollow-models-and-suction-cups-calculating-physics-of-resin-print-failures/) | resin maker's guide; a hobby article with a worked load example | low to medium |
| S16 | [Anycubic Photon Workshop manual (manuals.plus)](https://manuals.plus/m/a326f59a54739adc5c266e0f6cb04b40d9c88ac0f1fb3c3fcde0d54fd5af6a3e) | a copy of the vendor's manual | medium for definitions, no defaults |

## What the sources say

### Contact size and depth (observed)

| Source | Light | Medium | Heavy | Depth (penetration) |
| --- | --- | --- | --- | --- |
| karasu today | 0.3 mm | 0.4 mm | 0.6 mm | 0.15, 0.2, 0.3 mm |
| S3, J3D presets (Lychee lineage) | 0.3 mm, shaft 1.1 | 0.4 mm, shaft 1.3 | 0.6 mm, shaft 1.5 | not given |
| S3, Lychee Pro extras | Ultralight 0.24 mm, shaft 1.0; Medium-light 0.35 mm, shaft 1.2 | | | not given |
| S4, an older J3D edition | 0.24 mm | 0.3 mm | 0.4 mm | not given |
| S1, PrusaSlicer default | | 0.4 mm front, head width 1.0, pillar 1.0 | | 0.2 mm |
| S2, Prusa's example of thicker supports | | | 0.6 mm, pillar 1.3 | 0.6 mm |
| S7, CHITUBOX (one version) | upper diameter 0.3 mm | 0.4 mm | not given | contact depth 0.3 mm (Light), 0.4 mm (Medium) |
| S8, Formlabs Grey V5 | "finer" 0.2 mm | default 0.55 mm | | not given |
| S10, Mr Resin | 0.20–0.30 mm | 0.35–0.50 mm | 0.60–0.80 mm | 0.15–0.25, 0.25–0.35, 0.35–0.50 mm |
| S11, miniatures | 0.3 mm | | | 0.1 mm visible, 0.15 mm undersides |

- **Depth's meaning differs (observed):** S6 defines CHITUBOX's contact depth as how deep the tip
  goes into the model; S1 defines PrusaSlicer's penetration the same way. S7's Light and Medium
  give the same number to depth and diameter, so its depth may count to another reference point.
  Compare depths across slicers with care (inferred).
- **The trade-off (observed in S5, S6, S10, S11):** a wider or deeper contact holds better and
  leaves a larger mark, and is harder to break off; too small a contact fails during the print.
- **Size before density (observed in S9):** small tips (about 0.3 mm) failed on both printers
  even at the highest density; medium (0.4–0.5 mm) and large (0.5–0.6 mm) tips at high density
  succeeded. More supports do not make up for too small a tip.
- **Finer contacts need more of them (observed in S8):** Formlabs' finer touchpoints (0.2 mm
  instead of 0.55 mm) remove more easily and add about 40 % to the print time, with more
  touchpoints.

### Spacing and density (observed)

- **Flat undersides:** a contact every 2–3 mm (S13); one guide warns against more than 3 mm on
  large flat areas.
- **Curves and slopes:** 4–6 mm on organic curves, 6–10 mm on slopes of 30–45° (S13, one guide).
- **PrusaSlicer** (S1): relative density 100 %, at least 1 mm between support points, bridges up
  to 15 mm long, pillars joined "dynamically" (zigzag or cross).
- **karasu today:** 2.5 mm under a level surface, up to 5 mm on steep ones, density 100 %
  (TD-127). Within the published range (inferred).

### Braces and branches (observed)

- **Bracing** stiffens supports so they do not bend alone and holds a heavy model (S3, S15).
- **Over-bracing:** a grid braced in every cell uses many more braces than its rigidity needs,
  and a minimal set gives the same rigidity (S15, Wikipedia on grid bracing). The guides say to
  brace where the load or a long free length needs it, not everywhere (S15).
- **Branches:** tree supports spread the force and cut the number of contacts on the model, and
  suit organic models (S15).
- **PrusaSlicer** joins neighbouring pillars by bridges of at most 15 mm (S1).
- **Removal:** dense supports, complex supports and supports inside hollow parts are the
  reasons removal is hard (S12).

### Removal (observed)

- **When:** after washing and before the final UV cure (S10), while the resin is still a little
  flexible (S15).
- **Warm water:** a 15–30 s soak at 40–50 °C eases removal (S10); gentle warmth helps (S12).
- **Tools:** flush cutters rather than pulling (S12).

### Anti-aliasing on supports (observed)

- **Weaker supports:** a grey pixel passes less light and cures less (S14, HeyGears). One article
  reports anti-aliased supports failing during the print and advises turning anti-aliasing off on
  supports (S14, 3D Printerly), as TD-134 now does by default.

### Hollow models (observed)

- **Lighter supports suffice:** a hollow print weighs less and loads its supports less (S17).
  The risk moves to suction in sealed cavities, which drain holes solve (S17); the load to check
  is each tip's share of the peel and suction force, not the model's weight alone (S17, a worked
  example).

## What it means for karasu (inferred)

1. **The preset sizes are not outliers.** karasu's contacts (0.3, 0.4, 0.6 mm) are those of the
   J3D presets on Lychee's documentation site (S3), and Medium is PrusaSlicer's default (S1).
   The depths (0.15, 0.2, 0.3 mm) lie within the published values: PrusaSlicer's 0.2 mm, the
   miniature guides' 0.1–0.15 mm, and the seller's 0.15–0.50 mm.
2. **Heavy was a heavy choice for a hollow torso.** With 0.6 mm sunk 0.3 mm, each of 1,964
   contacts is Heavy on a model whose walls weigh a fraction of the solid. The published advice
   keeps Heavy for heavy or structural parts and Medium for most prints (S10: "about 80 %").
3. **The structure is the likeliest outlier.** 1.32 braces for each support and 70 % of the
   supports branching from another make a stiff lattice. The sources warn that dense and complex
   supports are hard to remove (S12), and that bracing every cell is more than rigidity needs
   (S15). No published value gives braces per support, so this is inferred, not measured
   against a reference.
4. **The anti-aliased supports did not make removal harder.** Grey edges cure less, so they make
   contacts weaker, not stronger (S14). Since TD-134 the supports print binary: the same
   contacts now cure whole, so they may hold **more** and be harder to remove than in v4. The
   presets are to be judged on binary supports (inferred).
5. **Removal was done at the right time.** The owner removed the supports before the UV cure,
   as the sources advise; a warm soak is the one step to add (S10).

## Measured on the test plate (observed, 2026-11-06)

The test plate below, sliced by `karasu-cli slice --project` for the Photon P1 (212 layers of
0.05 mm, supports binary), read back with the independent PP1 reader
(`tools/m5-validate/validate_pp1.py`):
- **Every tile stands on its tips:** under tile 1, layer 159 (the last below its underside)
  has 15,181 lit pixels and layer 160 the whole tile. UVtools 5 still lists each tile's first
  layer as an island: its check judges the lit area below against the layer's area, which 36
  contacts under a 15 mm tile do not reach. Its report is no defect here.
- **A contact's mark is wider than its diameter.** karasu's contact diameter is the cone's end
  inside the model, `penetration_mm` below the surface; the cone widens towards the tip's body,
  so where it meets the surface it is `d + (body - d) * penetration / tip_length` wide. The 36
  tips of 0.3 mm cover 3.07, 3.54 and 4.00 mm² at the surface for depths of 0.1, 0.2 and 0.3 mm
  (computed: 3.04, 3.52 and 4.17 mm²). Heavy's 0.6 mm sunk 0.3 mm meets the surface 0.68 mm wide:
  28 % more area than its diameter says (tile 18: 12.7 mm² measured, 13.1 computed). Whether
  other slicers measure their contact at the surface or at its end is not documented in the
  sources above; a preset compared with theirs should be compared at the surface.

## Hypotheses for the test print

- **H1:** fewer braces (a brace only where a shaft is long or carries others) make removal
  easier without a failure.
- **H2:** a shallower contact (0.15 mm instead of 0.3 mm) at the same diameter makes removal
  easier and still holds a hollow part.
- **H3:** Medium (0.4 mm) holds the torso's hollow walls as Heavy did.
- **H4:** with binary supports (TD-134), contacts hold as well as in v4 with a smaller diameter.
- **H5:** fewer branches (more supports to the plate, or merged trunks) leave fewer contacts and
  break off more easily.

**A test plate** made with karasu (`tools/support-test/make_support_test.py`, the owner's P1
profiles copied from the torso's project), about 35 to 40 minutes on the Photon P1: thin tiles
(15 × 15 × 2 mm, lifted 8 mm over a raft), each with one support combination and its number
embossed on its top.
- **Grid A, the contact:** diameter 0.3, 0.4, 0.5 and 0.6 mm against depth 0.1, 0.2 and 0.3 mm:
  12 tiles, spacing and the rest fixed.
- **Grid B, the structure:** the same contact with braces on and off, branches on and off, and
  spacing 2.5 and 3.5 mm: 4 to 6 tiles.
- **Recorded per tile:** whether it held; the effort to remove its supports, from 1 to 5;
  whether marks or holes are left. Removed after washing, before the UV cure, as v4 was.

## The test print (E-SUP-02, owner, 2026-11-06)

**Conditions (observed):** `support_test.pp1` as generated above (commit 3ea0b80), Photon P1,
Anycubic Standard Resin, "Normal 0.05 mm" (2.0 s), anti-aliasing 16 on the tiles, supports
binary; washed, then the supports removed before the UV cure without a warm soak. The owner's
record sheet and 27 photos (number, side view, underside of each tile that printed) are kept
outside the repository (`E:\3dprint\karasu_slicer_compare_test\karasu_test\support_test\`).

**Outcome (observed):** 9 of the 18 tiles printed; the 9 others came away at their contacts and
fell into the vat, their supports standing whole on the raft (plate photo). Every tile that
printed is bent, most at its corners, which no support holds (the outer tips stand 1.25 mm in
from each edge).

| Tile | Contact, depth | Width at the surface | Contact area / tile area | Held | Removal (1-5) | Marks |
| --- | --- | --- | --- | --- | --- | --- |
| 1, 5, 9 | 0.3 mm; 0.1, 0.2, 0.3 mm | 0.33, 0.36, 0.38 mm | 1.4, 1.6, 1.9 % | no | | |
| 2, 6 | 0.4 mm; 0.1, 0.2 mm | 0.42, 0.45 mm | 2.3, 2.5 % | no | | |
| 10 | 0.4 mm; 0.3 mm | 0.47 mm | 2.8 % | yes, more bent | 1 | pits |
| 3, 7, 11 | 0.5 mm; 0.1, 0.2, 0.3 mm | 0.52, 0.54, 0.56 mm | 3.4, 3.7, 3.9 % | yes, bent | 2, 4, 2 | pits |
| 4, 8, 12 | 0.6 mm; 0.1, 0.2, 0.3 mm | 0.62, 0.63, 0.65 mm | 4.8, 5.0, 5.3 % | yes, bent | 2, 4, 3 | dots and pits; pits |
| 13 to 16 | Medium (0.4 mm sunk 0.2): braced; 3.5 mm apart; 3.5 mm braced; branching | 0.45 mm | 2.5 % (1.1 % at 3.5 mm) | no | | |
| 18 | Heavy (0.6 mm sunk 0.3, tip 1.4 mm, shaft 1.4 mm) | 0.68 mm | 5.8 % | yes, bent | 5 | pits |
| 17 | Heavy, branching and braced, as the torso v4 | 0.68 mm | 5.8 % | yes, bent | could not be removed | |

The width at the surface is `d + (body - d) * depth / tip_length` (measured above); the area is
the 36 (or 16) contacts' at the surface over the tile's 225 mm².

**Findings (inferred from one tile per combination):**
1. **What holds is the contact area, not the diameter set.** Every tile with contacts covering
   2.8 % or more of its area held, every one with 2.5 % or less fell, whatever its depth,
   braces, spacing or branches. Depth helped only by widening the cone at the surface (tile 10
   held where 2 and 6 fell). This is a flat tile that peels its whole area at every layer: the
   worst case, the load each contact carries being the area it serves.
2. **Medium does not hold a solid flat part.** Medium's 0.4 mm sunk 0.2 fell on all five tiles
   (6, 13 to 16); neither braces nor branches saved it, as AmeraLabs found (S9).
3. **Light (0.3 mm) holds no flat area** at 2.5 mm spacing: it is for details, not undersides.
4. **Heavy's body, not its contact, makes it hard to remove.** Tile 12 (0.6 mm sunk 0.3 on
   Medium's 1.0 mm tip and shaft) scored 3; tile 18 (the same contact on Heavy's 1.4 mm tip and
   shaft, 3.0 mm long) scored 5, with almost the same width at the surface (0.65 against
   0.68 mm). A stiffer tip carries the force to the contact instead of snapping at it.
5. **Braces and branches made Heavy irremovable** (tile 17), as on the torso v4: this tile
   repeats the torso's supports and their outcome.
6. **Depth 0.2 mm scored worst** for 0.5 and 0.6 mm (4 against 2 and 2 to 3): not monotonic, so
   most likely the spread of one sample each; depth is to be measured again.
7. **Corners curl** where no tip reaches: the outline needs tips of its own, nearer the edge and
   at the corners, than an inner grid gives.

**Not shown by this print:** whether these thresholds hold on curved or sloped undersides, on
hollow parts (which peel only their walls' area), at other exposures and resins; whether a
second print repeats the removal scores.

**The second plate** (`make_support_test.py --round 2`, 2026-11-06; 20 tiles, 345 layers,
about 53 minutes estimated): two tiles of each of ten combinations, the second copy elsewhere
on the plate, to confirm the findings above and settle what one sample left open. Every tile
stands on its 36 (or 16) tips spread from edge to edge, the outer rows and the corners 0.4 mm
in, since finding 7 showed that edges and corners need their own tips; a tilted tile's lowest
edge, which prints first, stands on tips 0.2 mm in (owner, 2026-11-06: in the plate's first
draft its first layers grew in mid-air, 1.25 mm short of the first tips; UVtools listed their
first layer as an island, and lists nothing on the tilted tiles since).

| Combination (tiles) | Contact, depth | Width at the surface | Tests |
| --- | --- | --- | --- |
| 1 (1, 11) | 0.43, 0.1 mm | 0.45 mm | the threshold (finding 1) |
| 2 (2, 12) | 0.46, 0.1 mm | 0.48 mm | the threshold |
| 3 (3, 13) | 0.5, 0.1 mm | 0.52 mm | the reference, as the first plate's tile 3 |
| 4 (4, 14) | 0.5, 0.2 mm | 0.54 mm | depth (finding 6), as tile 7 |
| 5 (5, 15) | 0.6, 0.1 mm on a 1.0 mm tip and shaft, 2.5 mm long | 0.62 mm | Heavy's body (finding 4) |
| 6 (6, 16) | 0.6, 0.1 mm on Heavy's 1.4 mm tip and shaft, 3.0 mm long | 0.63 mm | Heavy's body |
| 7 (7, 17) | 0.6, 0.1 mm, 16 tips 4.7 mm apart (the record sheet says 3.5 mm: the spacing asked, before the insets spread four rows from edge to edge) | 0.62 mm, 2.1 % of the area | area or width (finding 1): a wide contact below the area threshold |
| 8 (8, 18) | 0.5, 0.1 mm, 18 braces (every other pair along the rows) | 0.52 mm | sparse braces |
| 9 (9, 19) | 0.5, 0.1 mm, tilted 30° about X | 0.52 mm | a sloped underside |
| 10 (10, 20) | 0.4, 0.2 mm (today's Medium), tilted 30° | 0.45 mm | Medium on a slope |

## The second print (E-SUP-03, owner, 2026-11-06)

**Conditions (observed):** `support_test_2.pp1` as generated above (commit 1cc1aab), the same
printer, resin, exposure and anti-aliasing as the first plate, supports binary; the vat cleared
of the first plate's fallen tiles and the resin filtered before printing; washed, then the
supports removed before the UV cure without a warm soak. The owner's record sheet and 60 photos
(the plate, then each tile's top, side and underside) are kept outside the repository
(`E:\3dprint\karasu_slicer_compare_test\karasu_test\support_test_2\`).

**Outcome (observed):** all 20 tiles printed, none fell. The record sheet notes a bend only on
combination 7 (in the centre) and on the four tilted tiles (at an edge, tile 10 less than
tile 9); the flat tiles held on 36 tips are recorded as held without a bend. Every tile's
supports left pits; none tore its underside.

| Combination (tiles) | Width at the surface | Contact area / tile area | Held | Removal (1-5) |
| --- | --- | --- | --- | --- |
| 1 (1, 11): 0.43, 0.1 mm | 0.45 mm | 2.6 % | yes, yes | 1, 1 |
| 2 (2, 12): 0.46, 0.1 mm | 0.48 mm | 2.9 % | yes, yes | 1, 1 |
| 3 (3, 13): 0.5, 0.1 mm | 0.52 mm | 3.4 % | yes, yes | 1.5, 1 |
| 4 (4, 14): 0.5, 0.2 mm | 0.54 mm | 3.7 % | yes, yes | 1.5, 1 |
| 5 (5, 15): 0.6, 0.1 mm, 1.0 mm tip and shaft | 0.62 mm | 4.8 % | yes, yes | 1.5, 2 |
| 6 (6, 16): 0.6, 0.1 mm, Heavy's 1.4 mm tip and shaft | 0.63 mm | 4.9 % | yes, yes | 3, 2.5 |
| 7 (7, 17): 0.6, 0.1 mm, 16 tips 4.7 mm apart | 0.62 mm | 2.1 % | bent in the centre, twice | 1, 1 |
| 8 (8, 18): 0.5, 0.1 mm, sparse braces | 0.52 mm | 3.4 % | yes, yes | 1, 1 |
| 9 (9, 19): 0.5, 0.1 mm, tilted 30° | 0.52 mm | 3.4 % | bent at an edge, twice | 1, 1 |
| 10 (10, 20): 0.4, 0.2 mm (Medium), tilted 30° | 0.45 mm | 2.5 % | bent at an edge, twice (10 less than 9) | 1, 1 |

**Findings (inferred from two tiles per combination and from the first plate):**
1. **The edges, not the contact area, decided whether a tile held.** The first plate's
   threshold (finding 1 of E-SUP-02: 2.8 % held, 2.5 % fell) does not survive: combination 1
   (2.6 %), Medium on a slope (2.5 %) and the wide contacts 4.7 mm apart (2.1 %) all held, on
   both copies. The same width at the surface as Medium's (0.45 mm) fell five times out of five
   on the first plate, whose outer tips stood 1.25 mm in from the edges, and held four times out
   of four here, with the outer tips 0.4 mm in. The likeliest reading: the film peels a layer
   from its outline inwards, and an outline 1.25 mm beyond the last tips levers the contacts
   off, where tips at the outline take the pull as it starts. Other differences between the two
   prints (the vat cleared and the resin filtered, a different day) are not ruled out; the edge
   inset is the one that matches every outcome.
2. **Edge and corner tips also keep a flat part flat.** Every tile of the first plate curled at
   its corners; the flat tiles here are recorded without a bend. Finding 7 of E-SUP-02 is
   confirmed: the outline needs its own tips, at the corners too.
3. **Spacing is bounded by the sag between tips, not by the hold.** 16 tips 4.7 mm apart held a
   2 mm tile but let its centre sag, on both copies; 2.84 mm apart (the other tiles) did not.
4. **Heavy's body makes it hard to remove; its contact does not.** The same 0.6 mm contact sunk
   0.1 mm scored 1.5 and 2 on a 1.0 mm tip and shaft 2.5 mm long, 3 and 2.5 on Heavy's 1.4 mm tip
   and shaft 3.0 mm long. Finding 4 of E-SUP-02 is confirmed; the wider 0.6 mm contact itself
   (combination 7: 1 and 1) is easy to remove.
5. **Depth made no difference to removal.** 0.5 mm sunk 0.1 and sunk 0.2 scored 1.5 and 1 both;
   the first plate's worse scores at 0.2 mm (finding 6 of E-SUP-02) were the spread of one
   sample.
6. **Sparse braces cost nothing at removal** (1 and 1 against 1.5 and 1 without), at the height
   these tiles stand. Whether braces were needed at all is not shown: every unbraced tile held.
7. **A sloped underside bends at its lowest edge.** The tilted tiles held but bent at their
   front edge, the lowest, which prints first on tips 0.2 mm in from it (owner, 2026-11-06), on
   both copies, Medium's contact less than 0.5 mm sunk 0.1. Tips under the edge alone hold the
   first layers' thin strip from below but not against the film's pull along the slope. The
   owner's remedy: an edge that stands on supports is also held on one of its sides, by tips on
   the face beside it (here the tile's front face, which faces down 30° from vertical), not only
   under it.
8. **Nothing failed**, so no lower bound is measured: below 0.45 mm at the surface, or below
   2.1 % of the area with tips at the edges, is untested on this plate. Light (0.3 mm sunk
   0.15, 0.34 mm at the surface) was tested only without edge tips (E-SUP-02: it fell).

**Not shown by these prints:** hollow parts, curved undersides, tall parts that need braces for
their shafts, other resins and exposures, branches with edge tips.

## Lychee's automatic supports on the test tiles (E-SUP-04, owner, 2026-11-06)

**Conditions (observed):** Lychee Slicer 8.0.001, its Light, Medium and Heavy presets at their
defaults (Light contact 0.24 mm, Medium 0.3, Heavy 0.4; tip length 2.5 mm; diameter 1.0, 1.1
and 1.3 mm), automatic supports "Classic" at density "Normal", Auto-Parent on, Auto-Bracing and
Auto-Lift off. The owner lifted each tile 8 mm by hand: the two tiles karasu generated for the
purpose (`tile_flat.stl`, 15 × 15 × 2 mm, and `tile_tilted_30.stl`, the same turned 30° about X
with its lowest edge at Y = 0), one of each per preset (Light at the back, Medium in the middle,
Heavy in front). Exported as the supported model (STL, 31,464 triangles) and as a PP1 for the
Photon P1 (348 layers, 2,618 s estimated); kept outside the repository
(`E:\3dprint\karasu_slicer_compare_test\karasu_test\lychee_tiles\`). Only the files were read,
by an independent script; nothing of the application was examined. Not printed.

**The tip (observed, from the STL):** an octagonal cone from a joint ball as wide as the shaft
to the contact, which ends in a near-hemispherical cap.

| Preset | Shaft and tip body | Width at the surface (octagon's corners) | Cap's depth into the tile | Tip, ball to cap |
| --- | --- | --- | --- | --- |
| Light | 0.99 mm | 0.243 mm | 0.12 mm | 3.12 mm |
| Medium | 1.09 mm | 0.304 mm | 0.15 mm | 3.20 mm |
| Heavy | 1.28 mm | 0.407 mm | 0.20 mm | 3.35 mm |

- The "tip diameter" Lychee shows is the contact's width **at the surface**, where karasu's is
  its end's inside the model. There is no separate depth: the cap sinks about the contact's
  radius. Lychee's Light is the "Ultralight" of S3 (0.24 mm on 1.0 mm), its Medium S3's Light
  (0.3 on 1.1 mm) and its Heavy S3's Medium (0.4 on 1.3 mm): the guide's presets shifted one
  step lighter.
- An octagon's area is 0.90 that of the circle through its corners: the contacts at the
  surface cover 0.042, 0.065 and 0.117 mm² each.

**The placement (observed)** is the same for the three presets; only the sizes change.
- **Flat tile:** 18 tips on 225 mm², not on a grid, about 3.5 mm apart; the outermost 0.6 to
  1.8 mm in from the edges; no tip given to a corner (the nearest 1.5 to 3 mm from it). Nine
  shafts stand on the tile's own raft (0.5 mm thick, 0.8 mm beyond the tile); the other nine
  tips hang from them on arms. Their contacts cover 0.33 % (Light), 0.52 % (Medium) and 0.94 %
  (Heavy) of the tile, where the least that held on karasu's second plate was 2.1 %.
- **Tilted tile:** 14 tips. The lowest edge takes five tips under it, 0.11 mm in from the edge,
  and three on the face beside it, 0.21 mm up from the edge, alternating along it; the rest of
  the underside takes six, from 5 mm up the slope. Every tip follows its face's normal (30° and
  60° from vertical).

**Findings (inferred):**
1. **The leading edge is held from below and from its side** in another slicer too, the
   owner's remedy (finding 7 of E-SUP-03, TD-136), with its tips much nearer the edge (0.11 mm
   under, 0.21 mm up the face) than karasu's (0.4 mm under; the side tip in the middle of the
   face, about 1 mm up).
2. **Thin contacts, sparse tips, stiff shafts:** the opposite of karasu's after TD-136. Whether
   they hold a flat part is not known: these tiles were not printed, and the owner notes that
   the automatic supports of Lychee, and of other slicers generally, are not the best
   (2026-11-06). Their placement is one slicer's choice, not evidence that it holds.
3. **A contact is to be compared at the surface:** Lychee's names are its widths there.

**Applied (owner, 2026-11-06):** tips close to a leading edge are common practice when supports
are placed by hand; TD-136 now puts the tips under it and the side tips the contact's radius
from the edge, alternating along it.

## Proposal and decision (TD-136)

From E-SUP-02 and E-SUP-03. **Approved by the owner on 2026-11-06** with two changes: Heavy's
shaft 1.2 mm, so that the presets' shafts differ by 0.2 mm (its tip keeps the tested 1.0 mm);
and a new brace's diameter 1.0 mm, an automatic brace the thinner of its shafts but at most
that. Implemented under TD-136 in `docs/TECHNOLOGY-DECISIONS.md`.

1. **Tips along the outline.** The automatic tool places tips along each underside's outline,
   0.4 mm in, and at its corners, before it fills the inside on the spacing's grid (findings 1
   and 2). Today the plan probes the footprint on a grid and takes a candidate only where no tip
   lies within its reach, so an edge or a corner can stand up to the reach (2.5 mm) from the
   nearest tip. This is the change the prints support most strongly.
   Where an underside's lowest edge stands on tips, the face beside that edge takes tips too,
   so that the edge is held from below and from one side (finding 7, the owner's remedy); such
   a face is often steeper than the critical angle, so the plan would not support it today.
2. **The presets (the contact sunk 0.1 mm, finding 5):**

   | Preset | Contact, depth | Tip body, length | Shaft | Width at the surface | Today |
   | --- | --- | --- | --- | --- | --- |
   | Light | 0.3, 0.1 mm | 0.8, 2.0 mm | 0.8 mm | 0.33 mm | 0.3 sunk 0.15 (0.34 mm) |
   | Medium | 0.5, 0.1 mm | 1.0, 2.5 mm | 1.0 mm | 0.52 mm | 0.4 sunk 0.2 (0.45 mm) |
   | Heavy | 0.6, 0.1 mm | 1.0, 2.5 mm | 1.2 mm (owner) | 0.62 mm | 0.6 sunk 0.3 on 1.4 mm, 3.0 mm long (0.68 mm) |

   Medium's 0.45 mm held with edge tips, but nothing failed to show its margin; 0.52 mm was
   no harder to remove (1 to 1.5). Heavy takes combination 5's body, which removal scored 1.5
   and 2 against 3 and 2.5. Heavy's pads and base cones stay. Light stays for details; its
   hold on a flat area with edge tips is untested.
   Saved projects keep their supports as they are: a document writes every support with its
   complete parameters (TD-130), so a preset's new values apply to new supports only.
3. **The automatic tool's defaults:** the Medium preset instead of Light; braces off (finding
   6: no tile needed them; they made Heavy irremovable on the first plate); the spacing kept at
   2.5 mm (finding 3). Branches stay on, untested with edge tips.
4. **The contact's width at the surface shown** in the support panel beside its diameter, which
   keeps its meaning (the cone's end inside the model) so that saved projects slice the same.
5. **Not proposed yet:** a narrower neck that breaks off cleanly, a warm soak suggested in the
   interface.
