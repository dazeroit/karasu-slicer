# Variable and adaptive layer heights (TD-064)

**Status:** implemented on 2026-10-19 (priority 5 of the
engine assessment, first part).

**Research and owner decisions:** research.

**Code:**
- `karasu_core::layer_plan`: the document, the planner and the adaptive rule;
- `slicing::schedule::build_slab_schedule`: stacked slabs;
- `PrintJob`: per-layer positions and thicknesses, and the validation rules below;
- the CLI's `--layer-plan <file.json>`.

## Document (`karasu-layers`, version 1)

```json
{"format": "karasu-layers", "version": 1, "z_step_mm": 0.001,
 "ranges": [
   {"from_mm": 0, "height_mm": 0.05},
   {"from_mm": 5, "height_mm": 0.025, "exposure_s": 1.6},
   {"from_mm": 20, "adaptive": {"min_height_mm": 0.02, "max_height_mm": 0.1,
                                "max_deviation_mm": 0.02, "max_growth_ratio": 1.5}}],
 "exposures": [{"height_mm": 0.02, "exposure_s": 1.2},
               {"height_mm": 0.05, "exposure_s": 2.0},
               {"height_mm": 0.1, "exposure_s": 3.5}]}
```

**General rules:**
- Unknown fields are refused, as is any document over 1 MiB.

**`z_step_mm`** (default 0.001):
- A positive whole number of nanometres.
- Every height must be a multiple of it.
- Off-step heights are refused, never snapped.

**`ranges`** (1–1024):
- Start at `from_mm` and run to the next range's start; the first range starts at 0 and starts
  strictly increase.
- Each range has exactly one of:
  - `height_mm`: a fixed height, positive, at most 1 mm, a multiple of the step;
  - `adaptive`:
    - `min_height_mm` ≤ `max_height_mm`, both on the step;
    - `max_deviation_mm` > 0: the largest stair-step deviation;
    - `max_growth_ratio` ≥ 1 (default 1.5).
- `exposure_s` (fixed ranges only) sets the exposure of every layer the range starts.

**`exposures`** (up to 256 points, heights strictly increasing, positive times):
- A layer thickness on a point takes its time.
- A thickness between two points is interpolated linearly.
- A thickness outside the table has no exposure: there is no extrapolation and no scaling.

## Planning (`plan_layers`)

All positions are integer nanometres, so the stack never accumulates rounding:
- **Stacking:** layer *k* covers `[bottom_k, bottom_k + h_k]`, starting from 0.
- **Range choice (carry policy):** a layer takes the range that contains its **bottom**. The first
  layer of a new range therefore starts at the first slab bottom at or above the boundary, and a
  lower range's last layer may end past the boundary.
- **No silent skips:** a range that the print reaches but in which no layer starts (one layer
  crosses all of it) is refused; ranges above the print are unused.
- **Termination:** layers are added while their centre lies at or below the model top (ceiled to
  the nanometre). This is the uniform plan's rule.
- **Bottom and transition layers** (`bottom_layer_count + transition_layer_count`) must fall in
  ranges with the calibrated height and no explicit exposure; otherwise the plan is refused. Their
  exposures stay the calibration's bottom and faded values.
- **Explicit settings:** every other layer whose thickness differs from the calibration, or whose
  range gives an exposure, receives a complete `LayerOverride`:
  - the calibration's normal settings (motion, waits, PWM);
  - with the range's exposure or the table's.
  - A layer without either is refused with its index and thickness.

**Adaptive rule:**
- **Slopes:** for every non-degenerate face that is not exactly horizontal (three equal vertex
  heights), `|n_z|` is taken from its unit normal.
  - A Z-bucket index keeps the largest `|n_z|` per bucket. The bucket size is the smallest adaptive
    maximum height, and the index does not depend on the deviation.
  - Vertical faces have `|n_z| = 0` and impose nothing.
- **Bound:** the bound of a layer starting at `b` is `c / max |n_z|` over the buckets touched by
  `[b, b + max_height]`, a look-ahead of one maximum layer. Without sloped faces it is unbounded.
- **Height:** the height is the smallest of:
  - the bound;
  - the previous layer's height × `max_growth_ratio`;
  - `max_height`.
  It is floored to the Z step, then clamped to `[min_height, max_height]`.
  - Decreases are immediate.
  - When `min_height` exceeds the bound, the deviation can exceed `c`.
- **Input surface:** only the placed model's surface, never the supports, and never the hollowing
  cavity.
- **Exposure:** adaptive layers always use the exposure table.

## Schedule and print job

`build_slab_schedule(heights_nm, limits, cancel)` publishes:
- one `LayerPlane` per slab at its centre, with the slab height as its nominal height;
- the `Slab` list (bottom, top, height);
- `spec()` is `None` for slab schedules.

Positions are `nm / 10⁶`, each one correctly rounded division. Stacks above 1 m and zero heights
are refused.

`PrintJob` uses each slab's top as the layer position and its height as the thickness. Layers whose
height differs from the calibration:
- must not be bottom or transition layers;
- must lie within the printer's `min_layer_height_mm`…`max_layer_height_mm`;
- must have an explicit override.

## Output formats

Writers take thickness from consecutive positions (`z_i − z_{i−1}`), so the following store
variable layers. Their header layer height stays the calibration's.

| Format | What it stores per layer |
| --- | --- |
| GOO | Position Z |
| CTB v2–v5 | Position Z |
| Photon | Position Z |
| PP1 | Position Z and `layer_thickness` |
| Anycubic Photon Workshop | LAYERDEF thickness |
| Reference | Position and thickness |

**SL1/SL1S:**
- `config.ini` has one `layerHeight`, so a non-uniform job is refused:
  - by the CLI before rasterizing (any explicit layer settings);
  - by the encoder per layer.
- Per-layer exposure is already refused by its fade rule.

## CLI

- **Option:** `--layer-plan <file.json>` reads the document before the model.
- **Summary:** the summary adds `Layer heights (mm): <thinnest> to <thickest>, <n> layers with
  explicit settings`.
- **Errors (nonzero exit):** document errors, a missing exposure, uncalibrated bottom layers and
  printer limit violations.

## Not in this version

- Per-range lift, wait or PWM changes (only exposure).
- Snapping range boundaries.
- An exposure model (for example Beer–Lambert) from resin constants.
- Per-object layer plans.
