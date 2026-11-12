# Output formats, priority 4 (TD-063)

**Status:** implemented on 2026-10-19.

**Research and owner decisions:** research.

**Validation:**
- `tools/m5-validate/validate_machine.py` (CTB v3–v5);
- `validate_workshop.py` (Anycubic);
- `validate_sl1.py` (SL1/SL1S).

**Common rules:**
- **Selection:** formats are chosen by an explicit identifier listed in the printer profile, never
  by the file extension.
- **Status:** every new format is "file-format valid, hardware unverified" until the physical print
  campaign.
- **Determinism:** writers record no clock time, host OS or random value, so outputs are
  byte-identical across platforms. Masks reach every writer already in the printer's physical
  orientation, set by the printer profile.

## CTB v3, v4 and v5, plain (`ctb-v3-plain`, `ctb-v4-plain`, `ctb-v5-plain`)

- **What does not change from v2:** all values are little-endian. Layers use the v2 7-bit RLE and
  previews the RLE15 encoding. The XOR seed (header +100) is 0, so images are plain. There are no
  checksums.
- **Encryption:** encrypted CTB (`0x12FD0107`) is never written.

**Magic and version:**

| Version | Magic | Version field |
| --- | --- | --- |
| v3 | `0x12FD0086` | 3 |
| v4 | `0x12FD0106` | 4 |
| v5 | `0x12FD0106` | 5 |

**Section order:**
- **v3:** header (112), print parameters (60), slicer information (76), machine name, previews
  (optional), layer table (36 × N), then for each layer its `LayerDefEx` (84) and image.
- **v4/v5:** header, previews (always written; black when none were rendered), print parameters,
  slicer information, machine name, notice (320), v4 block (464), v5 only resin parameters, layer
  table, then `LayerDefEx` and image for each layer.

**Header fields** (as in v2, except):
- +44: light-off (v3), or 0 (v4/v5, which use rest times);
- +84: print-parameters offset;
- +92: antialiasing level sets, 1;
- +96/+98: normal/bottom PWM;
- +104: slicer-information offset;
- +28: total height and print parameters +20: encoded volume, both patched at the end.

**Print parameters (60):**
- +0/+8: bottom/normal lift distance, **total of both stages**;
- +4/+12: stage-1 lift speeds (mm/min);
- +16: normal stage-1 retract speed;
- +20: volume (mL);
- +32/+36: light-off (v3), or 0;
- +40: bottom count.

**Slicer information (76):**

| Offset | Field |
| --- | --- |
| +0…+0x14 | v4/v5 only: bottom and normal stage-2 lift distance and speed, normal stage-2 retract distance and speed |
| +0x18 | v4/v5 only: normal rest after lift |
| +0x1c, +0x20 | Machine-name offset and length |
| +0x24 | Antialiasing flag: 0x0F when supersampled, else 0x07 |
| +0x27 | Per-layer byte: 0x20 (v3), 0x40 (v4, inferred), 0x50 (v5) |
| +0x2c | Antialiasing samples per axis |
| +0x30 | Writer version `0xMMmmpp00` |
| +0x34, +0x38 | v4/v5 only: normal rest after retract, rest after lift |
| +0x3c | Transition-layer count |
| +0x40 | v4/v5 only: v4-block offset |

**Notice (320, v4/v5):** karasu's own ASCII statement, NUL-padded, by owner decision. The vendor
notice is not copied.

**v4 block (464):**
- +0/+4: bottom retract speeds (stages 1 and 2);
- +0x0c and +0x14: 4.0, a constant observed in reference files whose meaning is not established;
- +0x18/+0x1c/+0x20: normal rest after retract / after lift / before lift;
- +0x24: bottom stage-2 retract distance;
- +0x28: total stationary time (exposures and waits), patched;
- +0x34: layer count;
- +0x48/+0x4c: notice offset and length;
- +0x50: resin-parameters offset (v5) or 0.

**Resin parameters (v5):**
- 40 bytes: colour 0, then three offset/length string pairs (resin, resin, machine, as observed),
  and the density at +0x20;
- then the strings, end to end.

**Layer table row (36):**
- Z, exposure, light-off (v3; 0 for v4/v5);
- the image offset, pointing past the `LayerDefEx`;
- image length, page 0, then 84.

**`LayerDefEx` (84):**
- a copy of the row;
- 84 + image length;
- total lift, stage-1 lift speed, stage-2 lift distance and speed;
- stage-1 retract speed, stage-2 retract distance and speed;
- rest before lift, after lift and after retract (v4/v5);
- PWM as f32.

**Capabilities:**

| Version | What it can store |
| --- | --- |
| v3 | Per-layer exposure, lift, retract and PWM, one motion stage, light-off |
| v4/v5 | Two-stage motion and the three rest times, per layer; a machine name is required |

## Anycubic Photon Workshop binary (`anycubic-<extension>-v<revision>`)

**Targets:** 21, one per extension, at the revision karasu writes. Where the references disagree,
it is Lychee's minimal revision.

| Revision | Extensions |
| --- | --- |
| 1 | `pws` (bit-planes), `pw0`, `pwx`, `pwmo`, `pwms`, `pwmx` |
| 515 | `pmsq`, `dlp` |
| 516 | `pwma`, `pm3`, `pm3m`, `pwmb`, `pm3r`, `dl2p`, `pmx2` |
| 517 | `pm3n`, `pm5`, `px6s`, `pm4n` |
| 518 | `pm5s`, `m5sp` |

**File mark:**
- `ANYCUBIC` padded to 12 bytes, the revision and the area count;
- then u32 addresses: HEADER, SOFTWARE, PREVIEW, colour table, LAYERDEF, EXTRA, MACHINE, images,
  MODEL, SUBIMGS, PREVIEW2;
- mark size and areas: 48/4 (rev 1), 48/5 (515), 52/8 (516), 56/9 (517), 64/11 (518);
- revisions 1 and 515 have no image slot. The MACHINE slot, and in 515 also the EXTRA slot, hold
  the image address.

**Sections** (names NUL-padded to 12 bytes; little-endian; no checksum):
- **HEADER:**
  - size 96/96/100/108/112 by revision, and its length field is the size minus 16;
  - pixel size (µm, the larger axis), layer height, exposure, light-off, bottom exposure, bottom
    count **as f32**;
  - total lift, stage-1 lift speed, final retract speed (mm/s);
  - volume and weight, both patched;
  - antialiasing count, resolution X and Y, print time;
  - from 516: the transition count;
  - from 517: u16 antialiasing and blur.
- **PREVIEW:** 224 × 168 words with blue in bits 15–11, green in 10–5 and red in 4–0, row-major
  from the top left; header w·h·2+12, 224, 42, 168.
- **Colour table (515 and later):** 0, 16, then the bytes 0x0F … 0xFF, then 0.
- **LAYERDEF:** N·32+4 and N, then per layer:
  - image address and length;
  - total lift and stage-1 lift speed;
  - exposure;
  - thickness (Δz);
  - lit pixels (summed over planes for `.pws`).
- **EXTRA (516 and later):** 24, then for bottom and for normal layers: 2, lift-1 distance and
  speed, retract-1 speed, lift-2 distance and speed, retract-2 speed (mm/s).
- **MACHINE (516 and later):** 156 bytes (224 for 518) and that length.
  - Machine name [96] and image format [16] (`pw0Img` or `pwsimg`).
  - Two words, 16 and 15, observed in reference files; their meaning is not established.
  - Build X, Y and Z, the revision, then 0.
  - 518 adds the pixel size X and Y (µm), ten words with index 8 set to 1, and the 16-bit
    resolution.
- **SOFTWARE (517 and later):** `karasu-slicer`, 164, the version, an empty OS field (platform
  independence) and `karasu-slicer`.
- **MODEL (517 and later):** the lit-pixel bounding box in panel millimetres, with layer Z.
- **SUBIMGS (518):** one record per layer (address, length, lit pixels).

**Two-stage motion:**
- Lifting covers lift-1, then lift-2.
- Retracting covers lift-2's distance at retract-1 speed, then lift-1's at retract-2 speed. A job's
  retract stages must mirror its lift stages.

**Images:**
- PW0, shared with PP1: 16 exact levels.
- `.pws`: N = antialiasing bit-planes of 1-bit runs (bit 7 lit, runs of up to 127, across rows).
  With c lit planes the stored gray is c·256/N − 1, so grays round to the nearest
  (`RoundToFormat`).

**Capabilities:**
- light-off only, the same for bottom and normal layers;
- no rest times and no PWM;
- two stages from revision 516; distinct bottom motion from 516;
- per-layer exposure only.

## Prusa SL1 and SL1S (`prusa-sl1-firmware-motion`, `prusa-sl1s-firmware-motion`)

**Archive:** a stored ZIP of:
- `<job>00000.png` and following: compressed 8-bit grayscale PNGs (zlib level 1, `miniz_oxide`);
- `thumbnail/thumbnail400x400.png` and `thumbnail/thumbnail800x480.png`: RGBA, when previews are
  rendered;
- `config.ini`, last, because it holds totals known only after the layers. Members are found by
  name.

**`config.ini`** has 16 keys in byte order, written as `key = value`:
- `action = print`;
- `expTime` and `expTimeFirst`, the shortest round-trip text;
- `expUserProfile = 0`;
- `fileCreationTimestamp = 1970-01-01 at 00:00:00 UTC`, a fixed value for determinism;
- `hollow = 0`;
- `jobDir`, the job ID: 1–64 ASCII letters, digits, `_` or `-`;
- `layerHeight`;
- `numFade` = the transition count;
- `numFast` and `numSlow`: a layer is slow when its lit area exceeds 50 % of the display;
- `printTime`, with 6 decimals;
- `printerModel` `SL1` or `SL1S`;
- `printerVariant = default`;
- `prusaSlicerVersion = karasu-slicer-<version>`;
- `usedMaterial` (mL, 6 decimals).

**Capabilities:**
- exactly one bottom layer and 3–20 transition layers, which the firmware fades;
- no other per-layer exposure, no waits, full PWM;
- motion is managed by the firmware: job motion is used only for estimates.

**Orientation:** set by the printer profile.
- SL1S: 1620 × 2560 portrait, 128 × 81 mm, so `swap_xy` is set.
- SL1: 1440 × 2560.
- Column 0 is bed Y = 0, and the top row is the maximum bed X (unverified on hardware).
