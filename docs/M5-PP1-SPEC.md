# Photon P1 export contract

## Sources and implementation boundary

The requested first Anycubic target is Photon P1, extension `.pp1`. This is a ZIP-based dialect,
not legacy Photon RLE under a different suffix. The format is verified against files that
Anycubic's slicer, Photon Workshop, writes, and by prints on the Photon P1.

This contract describes interoperability structures, not a printer firmware specification.
Karasu implements its own bounded Rust encoder from the protocol requirements below; no other
program's code, image or geometry engine, vendor assets, or default resin calibration is
incorporated.
Public reader acceptance and native cross-platform tests do not establish physical print safety.

The official public `TEST.pp1` is pinned by SHA-256
`d48e98b7dca6116d034ecba6a64e64f7bc7f9e92758b64798b54be7b80a7c83a` (4,811,835 bytes),
downloaded normally from the [P1 wiki](https://wiki.anycubic.com/en/resin-3d-printer/anycubic-photon-p1).
It contains 292 PW0 layers and revision-3 scene metadata. Raw bytes remain outside Git at
`/tmp/karasu-m5-commercial/TEST.pp1`; no models, previews, presets, or resin settings are reused.

## Container

Use a deterministic ZIP archive whose entries are deflated (method 8) at the crate's level 3,
or stored (method 0) when deflating does not make them smaller (TD-133), with standard CRC-32 and
sizes of the uncompressed bytes, fixed timestamps, UTF-8 entry names, and no ZIP64. Photon
Workshop and Lychee write deflated entries too. Each layer is deflated on the encoding worker
that encodes it, so the file is the same for every thread count. Reject excessive entry counts, member lengths,
offsets, total output bytes, or retained metadata before integer conversion/allocation. Every entry
is either complete or fails the containing export transaction; a failed job must not publish a
partial final file. Entry names are encoder-generated and cannot contain user-controlled paths.

Required semantic members:

| Member | Meaning |
| --- | --- |
| `anycubic_photon_resins.pwsp` | JSON machine geometry/identity and active user resin/settings. |
| `layers_controller.conf` | JSON `count` and `paras` array of effective per-layer settings. |
| `lcd_function.json` | Explicitly disabled calibration/RERF function and truthful software/model processing metadata. |
| `print_info.json` | Layer count and explicitly computed estimates; do not invent resin cost/density. |
| `software_info.conf` | Karasu identity and version, without impersonating Photon Workshop or a GPU renderer. |
| `scene.slice` | Little-endian binary scene/header and per-layer metadata. |
| `layer_images/layer_{index}.pw0Img` | PW0 raster for the zero-based layer index. |

Preview images use `preview_images/preview_{index}.png` and contain independently generated
224 × 168 and 336 × 252 grayscale maximum projections of quantized masks, never vendor assets.
The first projection samples pixel centers with nearest-neighbor selection; the second resizes it.
These are mask summaries, not shaded model renders. `calc_layer_volumes.data` is an optional auxiliary
calculation table in the public reader; do not invent its empirical raw-volume conversion. Any
hardware-required auxiliary member must be corroborated against an official P1 sample before
claiming firmware compatibility. The official sample contains no such auxiliary table.

## PW0 raster

Pixels are visited in the M4 mask's declared row-major LCD order without an additional undocumented
flip. A pixel is represented by a 4-bit level `q`, decoded to `17 * q` in eight-bit grayscale.
Black/white runs use two bytes: upper nibble is level 0/15 and the remaining 12 bits hold a run
length from 1 through 4095, most-significant byte first. Intermediate levels 1 through 14 use one
byte: upper nibble is the level and lower nibble holds a run length from 1 through 15. Runs may
cross row boundaries. Longer runs split into valid adjacent records. No zero-length run, implicit
trailing fill, or proprietary decoder tolerance is required by the encoder.

`ExactOnly` accepts only pixels in `{0, 17, ..., 255}`. `RoundToFormat` explicitly selects nearest
representable level (integer `(pixel + 8) / 17`); quantization is a recorded export choice. It is
not silently inherited from an unrelated format's AA setting. Check cancellation while scanning
large masks, and bound worst-case encoded size before allocating. Independent decoder tests must
cover all 256 input values, both policies, lengths around 15 and 4095, cross-row runs, and mixed
asymmetric images.

**Reader acceptance (TD-117).** The P1 and Photon Workshop refuse a file whose
`anycubic_photon_resins.pwsp` lacks any of these:
- `machine_type`:
  - `key_image_format` `pwszImg`, the only one Photon Workshop allows for the P1, whatever the
    members' `.pw0Img` suffix;
  - `max_file_version`, `property` and `raster_segments_capacity`;
  - the preview colours;
- `machine_extern`:
  - `picture`, which may be empty;
  - the P1's `firmware_calc_print_time_paras` and `firmware_calc_exp_time_paras`;
  - a complete resin `property`;
  - `max_acceleration` in its `slice_extpara`.

The firmware constants and `max_acceleration` come from Anycubic's official sample, by the
owner's authorization: Lychee and CHITUBOX write the same values. Previews are opaque RGBA, as
every reader's own are.

## Settings and units

The machine document uses a versioned `machine_type` and `machine_extern` object. Identity and
suffix must identify the actual requested target (`pp1`); machine settings retain independent X/Y
pixel dimensions, physical millimeters, and micron pixel pitches. Child-screen layout and any
P1-specific controller properties require independent confirmation; a generic single-screen model
must not silently masquerade as the production P1 profile.

The pinned official sample confirms anisotropic pixel geometry and includes two child-screen
rectangles. Its first single-material raster lights pixels between those rectangles; they must not
be unconditionally imposed as clipping bounds. This adapter emits one job-owned full-frame child
screen and zero extra rotation, following the public generic interoperability writer. Explicit
orientation belongs to the supplied profile/M4 mask. Hardware calibration and dual-material
partition mapping remain unverified; the size fixture is clearly marked uncalibrated.

The active user resin supplies `slicepara` and `slice_extpara`. Required concepts include nominal
layer height, bottom-layer count, normal/bottom exposure, rest before exposure (after retract),
rest after exposure (before lift), rest after lift, transition count, and staged motion. Durations
are seconds, dimensions millimeters, and the PP1 JSON speed unit is millimeters per second. Convert
from the core's explicitly declared unit exactly once. `multi_state_paras` keys identify
bottom/normal stages (`bott_0`, `bott_1`, `normal_0`, `normal_1`), each with height, upward speed,
and downward speed.

Do not infer calibration coefficients, heating targets, firmware acceleration parameters, light
power, or cost from a library's defaults. Unsupported nondefault job settings require an actionable
capability error. A future adapter revision may add a corroborated representation rather than
drop fields silently.

The implemented admission requires full PWM (255) and retract stages that mirror the lift stages
exactly, because one PP1 stage height is shared by both directions. Bands run from the plate's
lowest position up; retracting covers them in reverse, so band 0's down speed is the last retract
stage's (TD-105). It rejects per-layer motion/rest/PWM overrides; exposure and printer Z may
vary. `use_indivi_layerpara` is 1, and every controller entry carries effective exposure so
transitions are explicit. `zup_height` in the
profile/controller is total staged travel, as observed in the official sample; individual stage
distances remain in `multi_state_paras`. Automatic heating, compensation, firmware calibration,
cost, and resin density are not invented.

`layers_controller.conf` has `count` and `paras`. Each entry supplies zero-based `layer_index`,
`layer_minheight`, `layer_thickness`, `exposure_time`, `zup_height`, and `zup_speed`. The public
reader exposes no equivalent per-layer override fields here for arbitrary retract, rests, or PWM.
Such overrides must therefore be rejected unless another verified wire representation exists.
The materialized layer Z/thickness and count must agree across masks, controller JSON, and scene.

The resin's `slicepara.anti_count` records the masks' supersampling per axis (1 for binary masks),
as Lychee 8.0.0 records its antialiasing level there (TD-052); `blur_level` and `gray_level` are
zero. The machine `raster_antialiasing` capability field is unchanged.

## Scene binary

All numeric fields use little-endian 32-bit integers or IEEE-754 binary32, with checked finite
conversion from core values. Fixed strings are NUL-padded ASCII. The header begins with the
16-byte `ANYCUBIC-PWSZ` magic field and 64-byte Karasu software field. It then contains binary
type, scene version, slice type, model unit, point ratio, layer count, six bounds coordinates,
model status flags, 64 reserved integers, marker `<---`, and layer-definition count.

Each 64-byte layer record contains height, area, centered XY bounds (four floats), object count,
maximum contour area, and eight reserved integers. The final marker is `--->`. Bounds use
millimeters relative to the physical display center. Empty-mask bounds and area are explicit zeros.
Raster-derived support area is not a substitute for unavailable geometric contour topology:
object count and maximum-contour-area fields need trustworthy region metadata or a clearly
documented independently computed raster-component interpretation.

Karasu uses scene revision 3. Area is nonzero quantized pixel count times physical pixel area,
object count is the number of eight-neighbor connected components, and maximum area is the
largest component's pixel area. Bounds use outer pixel edges relative to display center. These
independent metrics deliberately differ from the commercial writers: CHITUBOX 1.4.0 writes only a
raster area (zero bounds, one object, maximum area equal to the area) and Lychee 8.0.0 only
placeholders (research).

Metrics are a pure function of each layer's mask and are measured on the parallel encoder workers
with the PW0 payload (TD-051); the writer only appends the record in layer order. Components are
labelled per run of pixels above the zero bin: each run joins the previous row's runs within one
pixel, and a component is finished when no run continues it. Scratch stays proportional to the
width, and uniform stretches are skipped eight pixels at a time. Automated tests check all 512
3 × 3 masks and 600 random masks (several widths and zero bins) against an independent flood fill.

## Admission and acceptance

An adapter revision must declare the exact settings/capabilities it supports and must pass bounded
validation before output publication. Tests must include stored ZIP checks with an independent
reader, member CRC verification, decoded pixel equality under the declared quantization, effective
layer metadata consistency, invalid dimensions/counts/settings, output/resource bounds,
cancellation, writer failure, deterministic bytes, and native Windows/Linux comparison.

Retained metadata admission reserves 640 bytes per layer, 512 KiB for preview/bounded JSON scratch,
and 80 bytes per raster column for component scratch. This aggregate must fit the explicit metadata
budget before initial output. Encoded masks are individually bounded and streamed without retaining
an image stack. Cancellation is polled during run/component scans, metadata loops, ZIP CRC/output,
and preview encoding.

The conservative metadata allowance covers simultaneous controller JSON, retained wire/scene rows,
ZIP directory capacity and member names. Full-height P1 jobs may require a larger explicit metadata
budget than the 8 MiB default; admission reports that limit before output instead of undercounting.
`print_info.json` volume is accumulated from encoded four-bit pixel intensities, physical pixel
area, and actual per-layer Z thickness, converted from cubic millimeters to milliliters. It is an
encoded coverage estimate, distinct from geometric volume. Cost and weight are omitted because
the wire snapshot does not supply verified price/density; unknown values are not replaced by zero.

`tools/m5-validate/validate_pp1.py` is an independent Python ZIP/PW0/PNG/scene reader. With
`--reference-dir`, selected masks and effective exposure/Z are compared to reference PNG/JSON.
`--commercial --sample-layers 1` validates the official sample's layout/previews and first
68,157,440-pixel mask without asserting Karasu's scene-area semantics or decoding its whole stack.
Tests reject PW0 zero/truncated/overrun/underrun records and check independent component fixtures.

The first release must distinguish tested software interoperability from physical P1 firmware
acceptance. Actual P1 sample readback and a user-authorized native/printer validation are needed
before claiming that the new adapter is confirmed on hardware.
