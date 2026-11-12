# Printer and resin profile sources (research, 2026-10-26)

**Purpose:** decide whether open printer and resin datasets can feed karasu-slicer's presets
(`tools/assets/make_presets.py`), which today hold 7 printers and 2 resin families taken by hand
from manufacturer pages. This record is research only: nothing is imported yet, and adopting
third-party data is the owner's licensing decision (see "Decisions for the owner").

**Method:** the repositories were cloned outside the project, and only their data files were
read, as data. No code in them was opened: karasu-slicer's writers stay an independent
implementation. Statements are marked **observed** (read in the
files), **inferred** (deduced from observations) or **hypothesis** (to be tested).

## Sources

| Repository | Commit | Date | License | Content used |
| --- | --- | --- | --- | --- |
| `github.com/Open-Resin-Alliance/df-plugin-elegoo` | `be59b1597ac5f5551ddbdae97145f584f05be056` | 2026-10-18 | MIT | `printers/*.json` (20 printers) |
| `github.com/Open-Resin-Alliance/df-plugin-anycubic` | `e555f6488ba18db4b1ce6df0a7015c3523d5f4ef` | 2026-10-20 | MIT | `printers/*.json` (27 printers) |
| `github.com/Open-Resin-Alliance/df-plugin-sirayatech` | `c4a19f9ab4d24dddf4ac5a8fb1af833b8b7280f6` | 2026-08-31 | MIT | `materials/**/*.json` (50 printers, 1 790 presets) |

The three `LICENSE` files are identical MIT texts ("Copyright (c) 2026 Open Resin Alliance",
sha256 `6b7df03f…7ed9b`). Data file hashes (sha256):

| File | sha256 |
| --- | --- |
| `df-plugin-elegoo/printers/jupiter-series.json` | `48fb126f5a9b3063dff70579c404eb52bb758bb1773e9e1a585d2e9c6f212b35` |
| `df-plugin-elegoo/printers/mars-series.json` | `b60291a949d07b3265d4a4f6da724399f368c2f104fa41384651403be05f3006` |
| `df-plugin-elegoo/printers/saturn-series.json` | `2964241bdeb09e04db89577a5858883cfebb3f163cd680a4155623e69168c5ee` |
| `df-plugin-anycubic/printers/photon-m-series.json` | `77cb5721102725933690978d92d8b2fab5862f7fb33c9748ba6c5d30832879b5` |
| `df-plugin-anycubic/printers/photon-mono-m-series.json` | `97b4bf7544f874870989205e064fa6252bc5dffbf90da0b47d05fcee9ae1743a` |
| `df-plugin-anycubic/printers/photon-mono-series.json` | `b691ddfb4561425cc6ae2e769a9e69f3011db5801e4731af617d4949ac564301` |
| `df-plugin-anycubic/printers/photon-mono-x-series.json` | `975155be1bca798c7eb1eb000b5cd52631abfa33ce71e22b929e7f528d0618c8` |
| `df-plugin-anycubic/printers/photon-p-series.json` | `304fa31990dbdd9500854b39f2fd4901783fb7106948d3d03ed945b7d6f694a4` |
| `df-plugin-anycubic/printers/photon-series.json` | `00583a602b5e94292b8e02195f695959ff79f58993fca9ee70473ff761e08262` |
| `df-plugin-sirayatech/dragonfruit-plugin.json` | `db670c81770d48f901a0b24e0857c1dc9900f284506f62458fe1bb3a0b681914` |

**Not usable (observed):** the printer images (`printers/assets/*.png`) were taken from the
manufacturers' sites, and the plugins' READMEs say permission is still being sought. They are
excluded.

## What the printer data holds (observed)

Each printer record: `presetId`, `profileVersion`, `manufacturer`, `family`, `name`,
`buildVolumeMm` (`width`, `depth`, `height`; width and depth absent for most ELEGOO and the P1
entries), `pixelSize` (µm, x and y), `display` (`resolutionX`, `resolutionY`, `outputFormat`,
`formatVersion`, `mirrorX`, `mirrorY`, `settingsMode`: simple, twostage or tilting),
`antiAliasing`, `bitDepth` (grey levels the panel renders), `hasCamera`, `networkSupport`.

Fields karasu-slicer's printer profile could take: resolution, pixel pitch (so the build area),
build height, output format (where karasu-slicer writes it), and, as hints only, the mirroring,
the motion mode (two-stage, tilting) and the grey levels.

## Comparison with karasu-slicer's presets (observed)

| Preset | karasu-slicer | Dataset | Note |
| --- | --- | --- | --- |
| Photon Mono SQ | 2560 × 2400, 128 × 120 × 200 | same | agrees |
| Photon Mono X | 3840 × 2400, 192 × 120 × 245 | same | agrees; dataset mirrors X |
| Photon Mono 4K | 3840 × 2400, 134.4 × 84 × 165 | same | agrees; dataset mirrors X |
| Photon Mono M5, M5s | 11520 × 5120, 218.88 × 122.88 × 200 | pixel 19 × 19 µm (97.28 mm deep), build 218.88 × 122.88 | dataset contradicts itself: its build depth matches ours (24 µm pitch), its pixel size does not |
| Photon S | 2560 × 1440, 47 µm (120.32 × 67.68) | 47.25 µm (120.96 × 68.04) | differs; to check against Anycubic |
| Photon (original) | present | absent | — |
| Saturn 4 Ultra 16K | 15120 × 6230, 211.68 × 118.37 × 220, GOO | same dimensions; output CTB v5 (encrypted) | dimensions agree |

## Mirroring (observed, then inferred)

**Observed:** the flags are per printer and vary inside families (Mars 2: X; Mars 2 Pro: none;
Saturn 3 Ultra: X; Saturn 3: Y). The ELEGOO README says they were "reconciled against the
supplied `Elegoo_officialPlatform.ini` catalog", copied independently per model; Mars 2 Pro
and Saturn 2 14K are not in that catalog and are "not treated as verified". The Anycubic README
says P1, Photon X and Mono SQ are unverified, and that the catalog contradicts itself for the
Mono X 6K / M3 Plus. Records changed after corrections (`profileVersion` raised), so the flags
are not stable over time. For the Saturn 4 Ultra 16K the dataset now says `mirrorX: false,
mirrorY: true`, for CTB output.

**Observed (TD-104):** for the Saturn 4 Ultra 16K's GOO output, karasu-slicer matches ChiTuBox's
stored image only with its own mirror X and mirror Y, and ChiTuBox's GOO header declares X 1,
Y 0.

**Inferred:** the dataset's flags follow the catalog's (ChiTuBox-style machine settings) meaning,
for the dataset's output format, against its own raster convention. None of the three is
karasu-slicer's: its raster puts the plate's front in the first row, and the same printer's
CTB and GOO files may differ. No fixed conversion from the dataset's flags to karasu-slicer's is
established: for the Saturn 4 Ultra 16K, (false, true) in the dataset corresponds to (true, true)
in karasu-slicer for GOO; for the Photon P1 (dataset: X), karasu-slicer's unmirrored PP1
equals Lychee's and is turned 180° from ChiTuBox's (TD-104).

**Consequence:** mirroring must not be imported as verified. Each printer and output format
needs a comparison with a reference file (SCN-08), as done for the Saturn 4 Ultra 16K.

## Resin data (observed)

Siraya Tech's presets are ported from Siraya Tech's own downloadable print settings (CBX 50 µm
`.cfg`/`.cfgx`), "co-authored by Siraya Tech and the Open Resin Alliance, and officially
supported by Siraya Tech". Each preset gives, per printer: layer height, normal and bottom
exposure, bottom layers, transition layers, two-stage lift and retract distances and speeds for
bottom and normal layers, waits before and after cure and after lift, light-off delays, PWM,
the minimum anti-aliasing alpha, the bottle price and capacity, and settings per output format.
These map onto karasu-slicer's resin calibration fields (`bottom` and `normal` layer settings
with lift and retract stages, waits, PWM; bottom and transition counts). When this record was
written, the anti-aliasing alpha and the price had no field.

**Update (2026-11-11):** TD-106, accepted the same day as this record, gave both a field:

- **Minimum anti-aliasing alpha:** the print setting's `ImageSettings::min_gray` (0–255), which
  a plate may override (TD-106 point 8; TD-060 for its meaning). Before an import, the unit of
  Siraya Tech's value (percentage, 0–255 or other) must be checked, so that it converts to
  `min_gray` exactly. **Inferred** from the field name; not yet observed.
- **Bottle price and capacity:** the resin's `cost_per_liter` and `currency`, the price divided
  by the capacity in litres.

Settings per output format still have no counterpart: a karasu-slicer print setting applies to
every format the printer writes.

## Findings

1. The dataset is a credible starting point for printer dimensions (resolution and pitch agree
   with ours for 5 of the 7 shared presets) and covers 47 printers.
2. It has errors (M5, M5s pitch) and disagreements (Photon S pitch): every value must still be
   checked against a manufacturer source before it becomes a preset, as the presets README
   already requires.
3. Mirroring cannot be taken from it (see above).
4. Siraya Tech's resin settings are the only openly licensed, manufacturer-backed calibrations
   found: they could give real resins for many printers, where karasu-slicer has two
   "starting point" resins.

## Official specifications (collected 2026-10-26)

**Method:** the manufacturers' own sources only: product pages, the ELEGOO wiki
(`wiki.elegoo.com`), the Anycubic wiki (`wiki.anycubic.com`) and the English PDF manuals it links
(hosted on the wiki or on Anycubic's store file storage, `cdn.shopify.com/s/files/1/0245/5519/2380/`).
Third-party sites only helped to find official pages. Both stores (`store.anycubic.com`,
`*.elegoo.com`) answered HTTP 429 to automated reads for most of the session, so the source of
each row is marked:
- **Direct:** the page or manual was read (manual pages without text were rendered and read).
- **Excerpt:** a search-engine excerpt of an official page, restricted to the manufacturer's site.
  To check in a browser before use.
- **Unverified:** only a search summary of an official page; not to be used until read.

Spot-checked by hand: the ELEGOO wiki's Mars table (Mars 5 "143.43 x 89.6 x 150 mm³", "35x35μm
(4098x2560)") and the Photon M3 Premium manual ("28.5 µm 7680*4320 (8K)", "218.88 mm(L)
*123.12 mm(W) *250 mm(H)").

"Derived" pitches are the stated build divided by the stated pixel count, not official figures.
Build volumes are X × Y × Z; stores that write height first (HWD) were reordered.

### ELEGOO (20)

| Printer | Resolution | Pixel (µm) | Build (mm) | Source | Against the dataset |
| --- | --- | --- | --- | --- | --- |
| Jupiter 2 | 15120 × 6230 | 20 × 26 | 302.40 × 161.98 × 300 | Direct (wiki FAQ) + excerpt | agrees; exports `.goo` only (wiki) |
| Jupiter SE | 5448 × 3064 | 51 | 277.848 × 156.264 × 300 | Excerpt | agrees (dataset lacks X, Y) |
| Jupiter | 5448 × 3064 | 51 | 277.848 × 156.264 × 300 | Excerpt | agrees; a launch post gives Y 156.06 (typo) |
| Mars 5 Ultra | 8520 × 4320 | 18 × 18 | 153.36 × 77.76 × 165 | Direct (wiki) | agrees |
| Mars 5 | 4098 × 2560 | 35 × 35 | 143.43 × 89.6 × **150** | Direct (wiki) | **height 150, dataset 165** |
| Mars 4 Ultra | 8520 × 4320 | 18 × 18 | 153.36 × 77.76 × 165 | Direct (wiki) | agrees |
| Mars 4 Max | 5760 × 3600 | 34 | 195.84 × 122.4 × 150 | Direct (wiki) | agrees |
| Mars 4 DLP | 2560 × 1440 | **50** (wiki), **52** (product page) | 132.8 × 74.7 × 150 | Direct (wiki) + excerpt | **ELEGOO never states 51.875** (the dataset's build ÷ pixels) |
| Mars 4 | 8520 × 4320 | 18 | 153.36 × 77.76 × 175 | Direct (wiki) + excerpt | agrees |
| Mars 3 Pro | 4098 × 2560 | 35 | 143.43 × 89.6 × 175 | Excerpt | agrees |
| Mars 3 | 4098 × 2560 | 35 | 143 × 89 × 175 (rounded) | Excerpt | agrees |
| Mars 2 Pro | 1620 × 2560 (as written) | **50** | **129 × 80 × 160** | Excerpt | **dataset 51 µm, 130.56 × 82.62**; ELEGOO's own figures disagree (2560 × 50 µm = 128) |
| Mars 2 | 1620 × 2560 | **50** | 129 × 80 × 150 | Excerpt | **dataset 51 µm** |
| Saturn 4 Ultra 16K | 15120 × 6230 | 14 × 19 | 211.68 × 118.37 × 220 | Excerpt | agrees (and karasu-slicer's preset) |
| Saturn 4 Ultra | 11520 × 5120 | 19 × 24 | 218.88 × 122.88 × 220 | Excerpt | agrees |
| Saturn 4 | 11520 × 5120 | 19 × 24 | 218.88 × 122.88 × 220 | Excerpt | agrees |
| Saturn 3 Ultra | 11520 × 5120 | 19 × 24 | 218.88 × 122.88 × 260 | Excerpt | agrees |
| Saturn 3 | 11520 × 5120 | 19 × 24 | 218.88 × 122.88 × **250** | Excerpt | **height 250, dataset 249.7**; its page names the GOO format |
| Saturn 2 14K | — | — | — | not found | **no ELEGOO product of that name** (a third-party upgrade kit) |
| Saturn 2 | 7680 × 4320 | 28.5 | 218.88 × 123.12 × 250 | Excerpt | agrees (product page rounds to 219 × 123) |

### Anycubic (27)

| Printer | Resolution | Pixel (µm) | Build (mm) | Source | Against the dataset |
| --- | --- | --- | --- | --- | --- |
| Photon M3 Max | 6480 × 3600 | 46 | **298 × 164** × 300 | Direct (manual) | **build 298 × 164, dataset 298.08 × 165.6**; 3600 × 46 µm ≠ 164 |
| Photon M3 Premium | **7680 × 4320** | **28.5** | 218.88 × 123.12 × 250 | Direct (manual), checked by hand | **dataset 5760 × 3240 at 38 µm is wrong** |
| Photon M3 | 4098 × 2560 (as printed) | — (derived ≈ 40) | 164 × 102 × 180 | Direct (store) | rounded build; "4,098" likely a typo for 4096 |
| Photon Mono M7 Max | 6480 × 3600 | — (derived ≈ 45.9 × 45.6) | 297.5 × 164 × 300 | Direct (manual) | build agrees; the dataset's 46 × 46 does not give it |
| Photon Mono M7 Pro | 13312 × 5120 | — (derived 16.80 × 24.70) | 223.64 × 126.48 × 230 | Direct (manual) | build agrees; **dataset Y 24.8 µm gives 126.98** |
| Photon Mono M7 | 13312 × 5120 | — (derived 16.80 × 24.70) | 223.64 × 126.48 × 230 | Direct (manual) | as the M7 Pro |
| Photon Mono M5s Pro | 13312 × 5120 | — (derived 16.81 × 24.68) | **223.78 × 126.38** × 200 | Direct (manual) | **build differs; dataset pixel 16.8 × 16.8 cannot be right in Y** |
| Photon Mono M5s | 11520 × 5120 | — (derived 19.0 × 24.0) | 218.88 × 122.88 × 200 | Direct (manual) | build agrees (and karasu-slicer's preset); **dataset 19 × 19 wrong in Y** |
| Photon Mono M5 | 11520 × 5120 | 19 "horizontal" | 218 × 123 × 200 (rounded) | Direct (store) | **19 µm is X only**; Y ≈ 24 (karasu-slicer's preset agrees) |
| Photon Mono 4 Ultra | 9024 × 5120 | — (derived 17.0) | 153.4 × 87 × 165 | Direct (manual) | agrees |
| Photon Mono 4 | 9024 × 5120 | — (derived 17.0) | 153.4 × 87 × 165 | Direct (manual) | agrees |
| Photon Mono 2 | 4096 × 2560 | — (derived 35.0 × 34.8) | 143.36 × **89.1** × 165 | Direct (manual) | **Y 89.1, dataset 89.6** |
| Photon Mono 4K | 3840 × 2400 | 35 "horizontal" | **132 × 80** × 165 | Direct (store) | page contradicts itself (3840 × 35 µm = 134.4); karasu-slicer's README already lists it |
| Photon Mono | 2560 × 1620 | 51 | 130 × 80 × 165 | Unverified | — |
| Photon Mono SE | 2560 × 1620 | 51 | **130 × 78** × 160 | Direct (store) | page contradicts itself (1620 × 51 µm = 82.62) |
| Photon Mono SQ | 2560 × 2400 | 50 | 128 × 120 × 200 | Unverified | agrees if confirmed (and karasu-slicer's preset) |
| Photon Mono X 6Ks | 5760 × 3600 | — (derived ≈ 33.9) | 195 × 122 × 200 | Direct (manual) | rounded or slightly different build; 34 not stated |
| Photon Mono X 6K / M3 Plus | 5760 × 3600 | 34 (store), 34.4 (manual) | **197 × 122** × 245 | Direct (store, manual) | official sources disagree on the pitch; neither gives 197 × 122 exactly |
| Photon Mono X2 | 4096 × 2560 | 48 | 196.7 × 122.8 × 200 | Direct (manual) | agrees (rounded) |
| Photon Mono X | 3840 × 2400 | 50 | 192 × 120 × 245 | Unverified | agrees if confirmed (and karasu-slicer's preset) |
| Photon P1 Max | 11520 × 8640 | — (derived ≈ 24.8) | 285.5 × 214 × 300 | Direct (manual) | agrees (dataset lacks X, Y) |
| Photon P1 | 13312 × 5120 | — (derived 16.75 × 24.61) | 223 × 126 × 230 | Direct (manual) | agrees; 24.8 µm Y gives 126.98 |
| Photon D2 | 2560 × 1440 | — (derived 51.0) | 130.56 × 73.44 × 165 | Direct (store) | agrees (DLP) |
| Photon Ultra | 1280 × 720 | 80 | — | Unverified | — |
| Photon X | — | — | — | not found | no official page or manual found |
| Photon S | — | 47 | 115 × 65 × 165 | Unverified | the printable area karasu-slicer's README already notes |
| Photon Zero | 854 × 480 | 115.5 | 97 × 54 × 150 | Unverified | 854 × 115.5 µm = 98.64 ≠ 97 |

**Findings from the official figures:**
1. The dataset has clear errors: the Photon M3 Premium's panel (8K, 28.5 µm), the Y pitch of the
   11520 × 5120 and 13312 × 5120 Anycubic panels (about 24 µm, never square 19 or 16.8 µm), two
   ELEGOO heights (Mars 5: 150; Saturn 3: 250) and the Mars 2 / 2 Pro and Mars 4 DLP pitches.
2. Manufacturers often state a rounded build, a printable area smaller than the panel, or one
   "horizontal" pitch for non-square pixels; several pages contradict themselves. A preset
   therefore needs the panel's pixel count and the pitch along each axis, and the source's
   figures may have to be reconciled (as karasu-slicer's README already does for the M5).
3. 39 of 47 printers have an official source: the 20 Anycubic rows read directly, and the 19
   ELEGOO rows read directly (7) or from excerpts (12) to check in a browser. 6 Anycubic rows
   are unverified until the store can be read; the Photon X has no official source, and the
   Saturn 2 14K is not an ELEGOO product.
4. No official page states a file format per model except the Jupiter 2 (GOO only) and the
   Saturn 3 (GOO). The format still has to come from the manufacturer's slicer or a reference
   file.

## Photon P1 files from four slicers (observed 2026-10-26)

The owner exported Base_01 (ChiTuBox, Lychee) and HillGiant (Photon Workshop 4.2.1) for the
Photon P1; karasu-slicer sliced both models with the same settings. Each PP1's machine settings
(`anycubic_photon_resins.pwsp`, `machine_type`) and stored layers were compared:

| Slicer | Stored image against karasu-slicer's unmirrored | `rotate_z` | `xy_pixel` × `xy_pixel_y` (µm) | `print_xsize` × `print_ysize` (mm) | `property` |
| --- | --- | --- | --- | --- | --- |
| Photon Workshop 4.2.1 | turned 180° (0.93–0.95) | 180 | 16.8 × 24.8 | 223.6416 × 125.976 | 119 |
| ChiTuBox | turned 180° (0.39 on a hollowed Base_01 layer) | 180 | 16.80 × 24.605 | 223.642 × 125.976 | 119 |
| Lychee | same (0.65) | absent | 16.8 × 24.8 | 223.6 × 126.48 | 119 |
| karasu-slicer (test profile) | — | 0 | 16.80 × 24.605 | 223.642 × 125.976 | absent |

None of the four agrees with another on the P1's pitch and area together, and none is fully
consistent: 5120 × 24.8 µm is 126.976 mm, not 125.976 (Photon Workshop) nor 126.48 (Lychee);
the manual gives 223 × 126 mm and no pitch. 125.976 mm (24.605 µm) agrees best with the manual.
`property` 119 is an undocumented integer. The orientation decision is in TD-104.

## Photon Workshop's resin settings for the P1 (observed 2026-10-26)

Photon Workshop's PP1 (SHA-256
`12974683d1c3f1b0608e4eb5ac2c0793ba2391d46a2054166bb689aab398b334`) carries, besides the active
profile, the slicer's factory resin profiles for the printer (`machine_extern.factory_resins`):
42 profiles for 22 Anycubic resins, each named `Anycubic@<resin>@NFEP@<setting>`, mostly in a
"high_quality_print" (0.03 mm) and a "normal_print" (0.05 mm) variant. The active profile is
Standard Resin, normal print, edited by the owner; the file stores the edit over the factory
profile too. The owner read Photon Workshop's unedited values in its interface:

| Setting | Standard Resin, normal print (default) | Exported file |
| --- | --- | --- |
| Layer height | 0.05 mm | 0.05 mm |
| Exposure | 2.0 s | 1.5 s |
| Bottom layers × exposure | 1 × 20 s | 4 × 20 s |
| Transition layers | 15 | 0 |
| Off time (normal, bottom) | 2 s, 30 s | 2 s, 30 s |
| Waits before and after lift | 0 s | 0 s |
| Normal motion, bands 0 and 1 | 4 mm at 3 mm/s up and down; 5 mm at 8 mm/s up and down | same |
| Bottom motion, bands 0 and 1 | 5 mm at 3 mm/s; 5 mm at 3 mm/s | same |

The other factory profiles share the off times, the waits and the motion; they differ in exposure
(1.1–4.4 s), bottom exposure (15–35 s) and layer height. Reading the bands showed that
karasu-slicer's PP1 adapter paired the retract stages with the bands in the wrong order (TD-105).

## Proposed adoption (not done; for the profiles review)

- The catalog generator (`make_catalog.py`, formerly `make_presets.py`) would read a pinned copy of the needed fields only (no code, no images),
  each preset recording its source record and the dataset commit, with the MIT notice in the
  presets README.
- Dimensions are accepted only where a manufacturer page agrees; disagreements are listed as
  known limits, as today.
- Mirroring stays off and "unverified" in the interface until a reference file is compared.
- Resin calibrations from Siraya Tech are imported per printer, marked as the manufacturer's
  settings rather than karasu-slicer's.

## Decisions for the owner

1. Adopt MIT-licensed third-party data in the presets (with its notice)?
2. Which printers and resins: all 47 printers, or the ones karasu-slicer writes a format for?
3. Siraya Tech resins: import now, or with the resin profile review?
4. Photon P1: use Photon Workshop's Standard Resin defaults above as the P1's calibration in the
   Anycubic Standard Resin preset, recorded as the manufacturer's slicer defaults rather than a
   public web table? The other factory profiles would wait for the resin profile review.
   **Decided by the owner on 2026-10-26: yes.** The Photon P1 printer preset and its Standard
   Resin calibration are in the catalog (`assets/catalog`, source A11); a HillGiant PP1 sliced with them
   carries Photon Workshop's motion bands, off times and orientation.
