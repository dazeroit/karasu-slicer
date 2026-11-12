# Suction cups in resin printing: reference behaviour

## Scope and method

M6.4j (TD-110) adds suction-cup detection. Before the analysis was designed, the owner decided
(2026-10-31):
- the cups are shown and handled in the Drain holes tool, since a hole is their remedy;
- the smallest opening that vents a cup is a setting, an area: "a pixel would not do; it would
  still be a suction cup physically".

This record covers two things:
- how the one reference that detects cups from the command line, UVtools, classifies a small
  set of shapes;
- what it reports on the owner's models.

It is research for TD-114. It is not a source of the algorithm: karasu's own definition and
design are in `docs/SUCTION-CUP-SPEC.md`.

**Evidence and its limits.** UVtools (AGPL-3.0) was used as a black box only:
- its command-line tool `UVtoolsCmd print-issues <file> -r -s` (resin traps and suction cups)
  was run on print files that karasu exported or that the owner made;
- its current settings were read from the user's settings file and its interface.

Its code was not read. The installed applications were not modified.

| Artifact | Version | SHA-256 |
| --- | --- | --- |
| `UVtools.exe` | 7.0.3+9db87262 | — |
| `UVtoolsCmd.exe` | 2.2.0+9db87262 | `2ea84f79a3dca7e746f67c1dc77add343e4ee96d49c292e9c594bd657f83c9ce` |

Labels: **[O]** observation, **[I]** inference, **[H]** hypothesis.

## UVtools' settings

The user's `usersettings.xml` holds these values, all defaults the owner never changed [O]:

| Setting | Value | Meaning (shipped documentation) |
| --- | --- | --- |
| `SuctionCupRequiredAreaToConsider` | 10000 | "Required minimum area to be considered a suction cup" |
| `SuctionCupRequiredHeightToConsider` | 0.5 | "Required minimum height (in mm) to be considered a suction cup" |
| `ResinTrapRequiredAreaToProcessCheck` | 17 | — |
| `ResinTrapRequiredBlackPixelsToDrain` | 10 | — |
| `ResinTrapMaximumPixelBrightnessToDrain` | 30 | — |
| `SuctionCupsVentHole` | 16 | the repair operation's vent |

UVtools reports a suction cup's area in px² [O]. The values are far above one layer's pixels,
so it is probably summed over the cup's layers [H]. With both thresholds, a cup must be at
least 0.5 mm tall and large enough to count [I].

## Experiment 1: ten shapes

**Conditions.**
- The models are built from 1 mm voxels (generator kept outside the repository, in the session's
  scratch directory):
  - a box 12 × 12 × 12 mm with 2 mm walls, enclosing a cavity of 8 × 8 × 8 mm;
  - where named, lifted 3 mm on four 2 × 2 mm legs.
- They were sliced by `karasu-cli slice` for the Anycubic Photon P1 catalog profile:
  - 13312 × 5120 px, 0.0168 × 0.0246 mm pixels, 0.05 mm layers;
  - catalog standard resin and print setting, `--center`, PP1 v3, binary masks.
- UVtools then read each file.
- Karasu's figures are its own analysis (`--suction-cups`, default 1 mm² vent) of the same
  masks, added after it was built.

| Shape | UVtools [O] | Karasu |
| --- | --- | --- |
| a. Closed box on the plate | ResinTrap, layers 40–199 | cup, layers 40–199, no opening, closes over trapped resin |
| b. Box on legs, 2 × 2 mm hole in the floor | none | none |
| c. Box on legs, 2 × 2 mm hole in the roof | SuctionCup, layers 100–299 | cup, layers 100–299, no opening, then open |
| d. Box on legs, 2 × 2 mm hole low in a side wall | none | none |
| e. Box on legs, 2 × 2 mm hole high in a side wall | SuctionCup, layers 100–219 | cup, layers 100–219, then open |
| f. Ring 4 mm tall on the plate | SuctionCup, layers 0–79 | cup, layers 0–79, then open |
| g. The same ring on legs | none | none |
| h. Bowl open towards the vat, on legs | SuctionCup, layers 100–179 | cup, layers 100–179, then open |
| i. Bowl open towards the plate, on legs | none | none |
| j. Box on legs, 1 × 1 mm hole in the floor | none | cup, layers 100–259, openings too small, closes |

**Findings.**
- [O] Where both find a cup, the layers agree exactly. They agree too where neither finds one.
- [I] UVtools follows the same model as karasu:
  - a cup lasts while its void is capped towards the plate and has no way out;
  - it ends at the vent's first layer, or at the rim of an open shape.
- [O] UVtools reports the closed box as a resin trap, not a cup. [I] It separates a void that
  ends enclosed (a trap) from one that ends open (a cup). Karasu reports one cup and says it
  closes over trapped resin: while its layers print, the closed box is a suction cup too.
- [O] Any opening vents in UVtools: shape j's 1 mm hole does. Karasu's minimum vent is the
  owner's setting. A 1 × 1 mm square passes a circle 1 mm across, 0.79 mm²: under the default
  1 mm², so the box is still a cup there.

## Experiment 2: the owner's models

**Conditions.**
- `HillGiant_32mm.stl` (the owner's model) was sliced by `karasu-cli` with:
  - `--center --drop --hollow 2`;
  - no drain holes and no supports;
  - the P1 profile as above, 774 layers.
- UVtools read the export. Karasu ran `--suction-cups` on the same masks.

| | UVtools [O] | Karasu |
| --- | --- | --- |
| Suction cups | 21 | 1319 (all, no threshold) |
| Resin traps | 279 | — (karasu marks a cup that closes) |
| The cavity | several resin traps (layers 312–478, 476–688, 243–414, …) | one cup, layers 79–688, 1238.5 mm² at its widest, 20.7 cm³, closes |
| The base's underside | SuctionCup, layers 59–85 | cup, layers 59–85, 23.0 mm², then open |
| Others alike | 575–584; 516–531; 585–596 | 575–584; 516–533; 585–599 |

Karasu's 1319 cups, by size (widest area, and layers spanned):

| Shown from | Any height | At least 5 layers | At least 10 layers (0.5 mm) |
| --- | --- | --- | --- |
| Any area | 1319 | — | — |
| 0.5 mm² | 608 | 27 | 13 |
| 1 mm² | 72 | 19 | 9 |
| 2 mm² | 22 | 15 | 8 |
| 4.1 mm² (10000 px of one layer) | 11 | 10 | 5 |

**Findings.**
- [O] 1099 of karasu's cups span one layer, and 570 have no opening and are under 0.05 mm²:
  pixel-scale dimples on the sculpted surface.
- [O] 586 are reported with openings too small. A circle of 1 mm² less a pixel takes about
  0.9 mm² as it sweeps, so each of these is at least that wide.
- [I] They are crevices of the surface, wider inside than at their mouth. Each is real by the
  definition and negligible in force.
- [I] UVtools' 21 are its thresholds at work: 0.5 mm tall and a minimum area. With 0.5 mm and
  1 mm², karasu shows 9. The largest coincide by their layers.
- [O] UVtools splits the enclosed cavity into several traps by layer ranges. Karasu keeps it as
  one cup, since its resin is one.
- [O] On two of the owner's exports UVtools found 25 cups (karasu's earlier hollowed HillGiant)
  and 15 (`full_test_hollowing_drain_holes_auto_supports_raft.pp1`, karasu, with drain holes).
  They also hold hundreds of resin traps of a few pixels, between supports and the model. [I]
  Drain holes alone do not prevent cups: they must be on the cup's plate side.

**Cost.** Karasu's analysis of the 774 layers took 0.75 s on 16 cores (0.23 s with no minimum
vent, which skips the erosion). The slice kept its masks for it, as `--unsupported` does: the
process peaked at 446 MB in all.

## Conclusions for the design

1. The definition both tools share holds: a void capped towards the plate, with no way out, is a
   cup at every layer it is open to the film. Karasu also reports whether it ends trapped.
2. The minimum vent is karasu's own addition at the owner's request. An opening counts where a
   circle of that area fits through it.
3. The core reports every cup, by TD-107's rule that nothing is hidden. A sculpted model has
   hundreds of negligible ones, so the interface needs a display filter (a minimum area and a
   minimum height) with the rest counted. Its defaults are an owner decision for M6.4j's
   interface; 1 mm² and 0.5 mm leave 9 on the hollowed HillGiant.
4. Physical prints must show which cups matter. Nothing here claims an effect on a print.
