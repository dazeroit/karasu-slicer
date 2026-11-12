# Command line: model formats, inspection and headless slicing

Moved from the repository README on 2026-11-10 (owner): the README keeps what a reader needs to
build and use the software, and points here for the rest.

## Model formats (TD-057)

STL, OBJ and 3MF models are read through one extensible format registry. The format is chosen by
the file extension (`.stl`, `.obj`, `.3mf`, ignoring case), never guessed from content. New formats
implement `karasu_core::mesh::formats::ModelFormat` and are added with `FormatRegistry::register`.

- **Units:** coordinates are converted to millimetres.
  - 3MF declares its unit (micron, millimeter, centimeter, inch, foot or meter), and that unit is
    used.
  - STL and OBJ declare none, so the unit must be given with `--units <mm|cm|m|um|in|ft>`, or
    with `--assume-mm`.
  - A given unit that contradicts a declared one is an error.
  - Millimetre coordinates are never rescaled.
- **Welding:** equal positions share one vertex by exact coordinate bits in every format, so the
  same geometry gives the same mesh and the same print file.
- **OBJ:**
  - Vertices (`v`, with an optional `w` of 1 or an ignored colour) and faces in every index form
    are read, including negative indices and forward references.
  - Polygons are split by ear clipping in their own plane. Convex polygons give the usual fan, and
    concave polygons stay inside their outline.
  - Texture coordinates, normals, groups, smoothing, materials, lines and points are ignored and
    reported. All objects form one mesh.
  - Free-form curves and surfaces, and unknown statements, are errors naming their line.
- **3MF:**
  - The model part is found through the package relationships.
  - Components and build-item transforms are applied, and mirrored instances keep their outward
    winding.
  - Objects of type `support` and `surface` are not solids, so they are skipped and reported. An
    `other` object in the build is an error.
  - Unknown required extensions are refused. The Production extension (objects in other model
    parts) and Materials (colours ignored) are supported.
  - Document type declarations, non-UTF-8 parts, CRC or size mismatches and unsafe archives are
    errors.

## Read-only model inspection

```console
cargo run -p karasu-cli -- inspect --assume-mm fixtures/reference/stl/fx-01-cube.stl
target/debug/karasu-cli inspect --assume-mm "/absolute/path/model with spaces.stl"
```

On native Windows the built executable is `target\debug\karasu-cli.exe`. Syntax is exactly
`karasu-cli inspect --assume-mm <path>`: the subcommand, unit flag, and one nonempty file path in that
order. Relative and absolute paths work. Quote shell paths containing spaces. A filename beginning
with `-` needs a relative prefix such as `./-model.stl` so it is not mistaken for an option. No stdin,
interactive session, or implicit unit default exists.

`karasu-cli inspect --model <path> [--units <unit>]` inspects any supported format. For STL in
millimetres it prints the STL report above; otherwise it prints the model report:
- the format, the unit (declared or assumed), the objects placed and the import notes;
- the same bounds, diagnostics and topology sections, in millimetres.
Help remains available through `--help`/`-h` and no arguments; `--version`/`-V` retain their behavior.

STL has no standard physical-unit declaration. `--assume-mm` explicitly interprets one coordinate
unit as one millimeter; it does not infer units from a filename, printer, header or GUI. Public
`karasu-core` import and topology APIs perform all geometry work. The command opens input read-only,
preserves file bytes/vertices/indices/winding/metadata, and does not repair, center, ground, scale,
rotate or place the mesh. Both binary and the core's documented single-solid ASCII grammar are
automatically detected; failed selected parsers do not silently fall back to another grammar.

### Report contract

Output is English text on stdout, in this fixed order:

1. Header, input path, detected STL format, explicit assumed units, and source-space interpretation.
   Paths are quoted/escaped using native OS-string debug representation, including control characters
   and non-Unicode names where the OS supports them. The path is the supplied spelling, not a
   canonicalized filename; different relative/absolute arguments can therefore yield different text.
2. Triangle and indexed vertex counts; source minimum, maximum and dimensions in XYZ order, in mm.
   Numeric values use Rust's locale-independent shortest round-trippable `f64` decimal display,
   not two-decimal GUI rounding. Empty meshes explicitly print `absent (empty mesh)` for all bounds
   and dimensions, never a fabricated zero box.
3. Import diagnostic total and counts for degenerate/duplicate triangles, non-finite/zero/opposing
   stored normals, and opaque nonzero attributes, followed by ordered details and omitted count.
4. Complete topology summary: usable/excluded faces, total/boundary/two-face/non-manifold edges,
   winding conflicts, both edge/vertex-connected component counts, and interior/boundary/non-manifold/
   indeterminate/isolated vertex counts. Separate edge-manifold, vertex-manifold and local-winding
   checks use `satisfied`, `failed`, `indeterminate` or `not applicable`. Closed-combinatorial status
   is separate; ordered issue details and omitted count follow.
5. An explicit statement that printability is not assessed. Closed combinatorics does not establish
   outwardness, self-intersection freedom, valid shell nesting, thickness, or a printable solid.
6. Only with `--diagnosis` (M6.4d r1, TD-092), after a blank line, the extended diagnosis:
   - needle and cap faces, cracks, pinch vertices, components under 1 mm² and the smallest
     component's area;
   - self-intersecting face pairs (exact contact test, searched up to 100,000 pairs; the line
     says when the bound stopped the search);
   - where each defect is: at most 16 positions per class, in source millimetres;
   - the printability state as is (union filling, holes closed in layers): `ready`, `ready with
     notes`, `slices; hollowing needs repair` or `needs repair`;
   - a note when self-intersections can leave reversed slivers that layers refuse;
   - what the `exact`, `standard` and `aggressive` repair presets would change, where, and the
     state after each. The file and the model are not changed.
   Everything is computed before anything is written, so a failure leaves no partial report.

`slice ... --unsupported [--overhang-angle <deg>]` (TD-107) adds the print's unsupported regions
to the slice summary, found on the sliced layers: islands, which start in mid-air, and overhangs,
which hang out from material that holds them, beyond the self-supporting distance of the layer
height over the tangent of the critical angle (degrees from the plate, 45 by default). Supports
in the job count as material; it reports and never fails the job.

`slice ... --suction-cups [--min-vent <mm2>]` (TD-114) adds the print's suction cups to the slice
summary, found on the sliced layers: resin a layer encloses while it is peeled from the film,
capped towards the plate (by the print, its raft or the plate) and not vented. An opening vents
only where a circle of the minimum vent area (1 mm² by default; 0 for any empty pixel) fits
through it, less one pixel. Each cup gives its layers, the centre of its cap (where a vent would
relieve it), its widest area and volume, whether it closes over trapped resin, and how many
parts it has: a cavity with several lowest points is one cup, each part listed after it, since
each needs its own vent. The 20 widest are listed. It reports and never fails the job.

`inspect --minima [--rotate <x> <y> <z>]` (TD-107) is a report of its own: the model turned by the
angles (degrees, X then Y then Z) and lowered onto the plate, then its local minima, the points,
level edges and faces where material starts with nothing below it. Each is listed (at most 100)
with its position, how many vertices it groups and whether it rests on the plate.

Each detail section shows at most **100** core-ordered records. Total counts always cover the full
analysis; shown/omitted counts are explicit even when zero. No diagnostics are silently truncated
or reclassified as healthy. Triangle/vertex identifiers are zero-based imported source-mesh IDs;
edge identifiers index the deterministic derived report and include their source endpoint indices.
STL first-occurrence deduplication IDs need not equal fixture-generator buffer IDs. Excluded faces
stay present in the mesh but do not contribute ordinary topology incidences; vertices touching them
are indeterminate. A zero count differs from a `not applicable` check on empty input.

For example, FX-01 reports:

```text
Triangles: 12
Indexed vertices: 8
Source minimum XYZ (mm): -10 -10 0
Source maximum XYZ (mm): 10 10 20
Source dimensions XYZ (mm): 20 20 20
```

For the same input bytes, argument spelling and build/platform, repeated reports have stable field
and diagnostic ordering with no timestamps or runtime metrics. Text is not a versioned JSON schema;
future contract changes require documentation/tests. Native path rendering and floating-point
classification can differ between platforms within the core's documented limitations.

### Errors, status and limits

- **0:** inspection completed, including successfully imported defective meshes. Findings on stdout
  do not constitute execution failure or an automatic repair decision.
- **2:** invalid usage, missing `--assume-mm`/path, unsupported commands/options or extra arguments.
- **1:** file access, import, topology analysis or output-write/flush failure.

Errors go to stderr with stage/path context; ordinary invalid input does not print a panic trace.
Import and topology must both succeed before any inspection report is written, so their failures
leave stdout empty. A later stdout I/O failure can leave already-written bytes, but returns failure
rather than presenting a successful complete report. Diagnostic findings alone leave stderr empty.

Existing core default resource limits apply (1,280 MiB encoded STL, 25,000,000 faces, 75,000,000
distinct vertices, plus ASCII limits); these are not total-memory or elapsed-time guarantees.
Topology retains derived buffers: the full report needs about 300 bytes per triangle (about 6 GB
for 20 million), while `slice` admits union models through the lean edge summary (TD-053). Detail caps limit output, not analysis work. The CLI remains synchronous and uses
ordinary public APIs; it adds no cancellation controller, signal handling, worker or job system.
STL, OBJ and 3MF are supported (see Model formats). No mesh repair or scene transformation
command is added.

## Headless slicing

```console
karasu-cli slice --assume-mm model.stl --printer printer.json --resin resin.json \
  --format ctb-v2-plain --output model.ctb --translate 50 30 0
```

`slice` connects existing core contracts without adding geometry policy (TD-043):
model import → optional topology gate → explicit placement → uniform layer schedule → M3 layer engine
→ M4 binary LCD masks → M5 create-only export. Options may appear in any order; each at most once, except the repeatable `--drain-hole`.

- `--model <path>` with `--units <unit>`, or `--assume-mm <path>`:
  - **The model:** STL, OBJ or 3MF, chosen by extension (see Model formats).
  - **Units:** `--units` gives the unit for STL and OBJ; 3MF declares its own.
  - **`--assume-mm`:** equals `--model <path> --units mm`. For compatibility it reads a file
    without a known model extension as STL.
  - **Summary:** the `Model` line gives the format, unit, origin and objects, and `Import notes`
    lists what was ignored or converted.
- `--printer <file>`, `--resin <file>`: typed profile JSON documents, exactly as written by
  `ProfileDocument::to_json` (`{"kind": "printer", "profile": {...}}` / `"kind": "resin"`).
  Unknown fields are rejected. The resin must contain a calibration whose `printer_id` equals the
  printer `id`; its `layer_height_mm`, bottom/transition counts and exposures define the job.
- `--print-setting <file>` (TD-106): a library print setting (`"kind": "print_setting"`); `--resin`
  is then a library resin (`"kind": "material"`), and the setting supplies the calibration, the
  anti-aliasing, the minimum grey and the XY compensation, so those options are refused beside
  it.
- `--format <id>`: one of these identifiers. The output extension must match it.
  - `ctb-v2-plain`, `ctb-v3-plain`, `ctb-v4-plain` and `ctb-v5-plain` (TD-063), all `.ctb`.
    These are plain CTB: encrypted CTB, which some firmware requires, is not supported.
  - `goo-v3` (`.goo`) and `anycubic-pp1-v3` (`.pp1`).
  - `photon-v1-firmware-motion` (`.photon`).
  - `anycubic-<ext>-v<revision>` for the Anycubic Photon Workshop family (TD-063). Examples are
    `anycubic-pwmx-v1`, `anycubic-pwma-v516`, `anycubic-pm5-v517`, `anycubic-pm5s-v518` and
    `anycubic-pws-v1`; the help lists all 21.
  - `prusa-sl1-firmware-motion` (`.sl1`) and `prusa-sl1s-firmware-motion` (`.sl1s`) (TD-063).
    They need one bottom layer and 3–20 transition layers, which the firmware fades, and use the
    job ID as the layer-name prefix.
  - `reference`: a new directory with numbered PNGs, `manifest.json` and `layers.jsonl`.

  Machine identifiers must also be listed in the printer's `format_ids`. The dialect is never
  guessed from the extension. The new formats are validated by independent readers, but printer
  acceptance is unverified until physical tests.
- **Previews:** machine files store rendered model thumbnails at the format's sizes (TD-059):
  the sliced surface, meaning the repaired model or the hollowed shell, seen from the front right
  and above.
- `--output <path>`: must not exist. Publication is atomic and create-only; failures remove their
  own staging data and never replace an existing file.
- **Placement (TD-061):**
  - `--rotate <x> <y> <z>`: degrees about fixed X, then Y, then Z axes.
  - `--scale <x> <y> <z>`: positive factors.
  - `--mirror <axes>`: such as `x` or `xz`.
  - All three act about the source bounding-box centre. Mirroring cannot be combined with
    `--hollow`.
- `--translate <x> <y> <z>` (default `0 0 0`), or `--center` and/or `--drop`:
  - `--center` centres the model on the plate, and `--drop` rests it exactly on Z = 0, after any
    rotation.
  - `--translate` cannot be combined with them.
  - The placed model's minimum Z must be **exactly 0**, and its XY extent must lie inside
    `[origin, origin + build size]`. Otherwise the command fails and, for Z, suggests `--drop`
    or the complete `--translate` that rests the model on Z = 0. Values printed by `inspect`
    round-trip.
  - The model is never clipped.
  - The summary prints the applied `Translation (mm)` and any `Transform`.
- `--xy-compensation <px>` and `--bottom-xy-compensation <px>` (−64 to 64, default none):
  - Grow (positive) or shrink (negative) the masks by whole pixels. Holes change the opposite way.
  - The bottom value replaces the general one on the resin calibration's bottom layers, for
    example `-1` against elephant's foot.
  - The summary gives the millimetre equivalents, and reference records carry each layer's
    compensation.
- **Manual supports (TD-062, [specification](SUPPORTS-SPEC.md)):**
  - `--supports <file.json>` reads a `karasu-supports` document.
  - Each support has:
    - a tip point on the model;
    - an axis along the surface normal, straight towards the shaft, or explicit;
    - optional bend points;
    - a base: `{"platform": [x, y]}`, `{"vertical": true}`, `{"model": [x, y, z]}` on the
      model, or `{"support": id, "z_mm": z}` on another support's shaft.
  - Parameters come from the `light`, `medium` (default) or `heavy` preset, then document
    `defaults`, then the support's own `parameters` (diameters, lengths, penetration, sides, pad,
    cone and model-foot sizes).
  - Braces and a raft (thickness, margin, slope) are optional.
  - Every contact must lie on the model surface, outer or cavity, so supports also stand inside
    hollowed models.
  - Clearance to the model is proven. Collisions, piercing, drain-hole crossings and
    out-of-volume parts fail the command with each finding. `--accept-support-collisions`, or a
    support's `accept_collision`, accepts collisions, which stay listed.
  - `--drop --lift <mm>` raises the model for supports. Previews show the supports, and the
    summary counts them.
  - **Binding:** `binding.translation_mm` alone requires exactly that placement. With the pose
    the supports were placed for (`rotate_deg`, `scale`, `mirror`, TD-072), they follow the model
    when it is moved on the plate or turned about the vertical axis. A tilt, scale, mirroring or
    height change is refused. The summary then reports `Supports follow the model: …`.

  ```json
  {"format": "karasu-supports", "version": 1, "preset": "medium",
   "supports": [{"id": 1, "tip": {"point": [15, 25, 5]}, "base": {"vertical": true}}]}
  ```
- **Variable and adaptive layer heights (TD-064, [specification](LAYER-PLAN-SPEC.md)):**
  - `--layer-plan <file.json>` reads a `karasu-layers` document of Z ranges from the plate up.
  - Each range has a fixed `height_mm`, optionally with its own `exposure_s`, or an `adaptive`
    rule: heights from the model's slopes, `c / |n_z|` with a maximum deviation, minimum and
    maximum heights, and a growth ratio.
  - Layers whose thickness differs from the calibration take their range's exposure or the
    document's height → exposure table, interpolated only between its points. A thickness
    without either is refused; nothing is extrapolated or scaled.
  - All heights are whole multiples of the Z step (default 0.001 mm) and positions are exact.
  - Bottom and transition layers keep the calibrated height.
  - SL1 and SL1S store one layer height and refuse layer plans. The other formats store per-layer
    positions.
  - The summary prints `Layer heights (mm): <thinnest> to <thickest>, <n> layers with explicit
    settings`.

  ```json
  {"format": "karasu-layers", "version": 1,
   "ranges": [{"from_mm": 0, "height_mm": 0.05},
              {"from_mm": 5, "adaptive": {"min_height_mm": 0.02, "max_height_mm": 0.1,
                                          "max_deviation_mm": 0.02}}],
   "exposures": [{"height_mm": 0.02, "exposure_s": 1.2}, {"height_mm": 0.1, "exposure_s": 3.5}]}
  ```
- **Several models and copies (TD-065, [specification](PLATE-SPEC.md)):**
  - `--copies <n>` prints n instances of the model, arranged automatically; `--arrange` arranges
    even one. `--spacing <mm>` (default 2) sets the gap between footprints.
  - `--plate <plate.json>` reads a `karasu-plate` document instead of `--model`. Each object has
    its own model, units, rotation, scale, mirroring, placement, hollowing, drain holes, supports
    and copies, and is either fixed (`"arrange": false`) or arranged (the default).
  - Footprints include supports, raft and drain holes. They are packed in deterministic shelves,
    never rotated, centred when no object is fixed, and packed around fixed objects otherwise.
    Instances that do not fit fail the command.
  - Each object is prepared once; its copies are rigid moves of it. Overlapping objects print as
    their union and are listed. Drain holes must not reach other objects.
  - Version 2 documents (TD-089) also take drain holes at the object level, without hollowing,
    and surface holes from the cavity or with a fixed depth; version 1 is still read.
  - Layer heights, exposure and the other print settings stay plate-wide.

  ```json
  {"format": "karasu-plate", "version": 1,
   "objects": [{"model": "part.stl", "units": "mm", "drop": true, "copies": 4},
               {"model": "bust.stl", "units": "mm", "translate_mm": [60, 40, 5],
                "arrange": false, "supports": "bust-supports.json"}]}
  ```
- **Resin:** the summary reports `Resin volume (ml)` from the sampled mask volume. It adds
  `Resin mass (g)` and `Resin cost (<currency>)` when the resin profile has a density and a cost.
- `--job-id <id>` (default `slice`): portable job identity recorded in exported metadata.
- `--fill <union|strict>` (default `union`): how layer sections become material (TD-050).
  - `union`: a point is material where the winding number of the oriented surface is positive.
    Overlapping, touching and self-intersecting bodies print as their union, and reversed shells
    (such as hollowing cavities) stay empty. Nothing is repaired: a layer whose section does not
    close (open boundary or inconsistently oriented faces) fails with the offending mesh edges,
    and an inside-out model fails instead of printing empty layers. The summary reports how many
    layers merged overlapping material.
  - `strict`: the strict even-odd pipeline; every layer must be an embedded set of contours.
  On every model `strict` accepts, both produce identical files.
- `--antialias <1|2|4|8|16>` (default `1`, binary masks): samples every pixel on an N × N grid
  of centres and stores the covered fraction as grey, linearly, with no gamma; `--min-gray` adds
  an explicit floor (TD-052, research).
  - Depth: GOO keeps 8-bit greys and PP1 its 16 native levels, both exactly. CTB rounds each grey
    to its nearest 7-bit level.
  - `photon-v1-firmware-motion` stores binary masks only and is refused.
  - The level is recorded in the file: the GOO antialiasing factor, the CTB antialiasing flag and
    the PP1 `anti_count`.
  - The summary reports the samples per pixel, and the sampled volume includes partial pixels.
- `--min-gray <0-255>` (with `--antialias` above 1; default 0): the smallest gray of a partially
  covered pixel, for resins that do not cure faint grays (TD-060).
  - Coverage `c` becomes `G + (255 − G)·c` before the format's rounding.
  - Empty pixels stay 0 and full pixels 255, so the same pixels are lit, with brighter edges.
  - The summary prints `Minimum edge gray (0-255)`, and reference records carry `minimum_gray`.
  - There is no default value, because the right one is a resin calibration.
- `--close-holes <mm>` (default `2`) / `--no-close-holes`: with `union`, closes open sections at
  holes in each layer (TD-054, [specification](OPEN-MESH-SPEC.md)).
  - Each open chain is joined across its hole by a straight chord between the hole rim's crossings.
    Crossings are paired along the rim, never by proximity, and the mesh itself is unchanged.
  - A chord longer than the bound fails the layer with its position. This is the case for large
    holes and for open surfaces that are not solids.
  - Every slice prints `Mesh defects` (boundary edges and hole rims, non-manifold edges, winding
    conflicts, duplicate and degenerate faces) and `Closed sections` (layers, chords, longest
    chord).
  - `--no-close-holes` refuses meshes with holes.
- `--repair <none|off|auto|exact|standard|aggressive|rebuild|list>` (default `auto` = `duplicates,orientation`): explicit repairs on a
  derived mesh (TD-054 phases 2 and 3), applied in the order duplicates and orientation, hole
  filling, outward. The source file is never changed, and the summary prints
  `Repairs` and `Remaining defects`.
  - `duplicates`: faces with the same corners collapse to their net orientation, so cancelling
    double walls between touching bodies disappear.
  - `orientation`: faces are flipped so each connected component is consistently oriented,
    keeping the orientation covering more area. Non-orientable components are left unchanged.
  - `fill-holes` (opt-in): every hole whose rim is a simple closed loop of at most 256 edges is
    capped by the triangulation of its rim with the smallest maximum fold against the neighbouring
    faces (then the smallest area), using no new vertices. A hole is left open and reported when
    its rim is an open chain (it ends at a non-manifold edge), is too long, revisits a vertex, has
    only degenerate caps, or when its cap would duplicate an existing edge or cut through the
    mesh. The summary prints the holes filled, their cap triangles and area, and the holes left
    open. Closing sections in each layer remains the fallback for holes that are not filled.
  - `outward` (opt-in, implies `orientation`): closed bodies outside every other body's bounds and
    enclosing negative volume are turned outward. Nested reversed shells (cavities) are left
    unchanged.
  - `weld[=mm]` (TD-092, R1, first): vertices on open edges within `mm` of each other merge into
    groups no wider than `mm`, onto the group's first vertex; without a value, only equal
    positions. Vertices of closed regions never move. Faces whose corners merge are removed.
  - `degenerate[=mm]` (R2, after welding): faces with a repeated vertex are removed; with `mm`,
    edges up to `mm` collapse (when the surface stays manifold) and faces whose apex is within
    `mm` of their longest edge are flipped with their neighbour. Faces left within `mm` are
    counted.
  - `cracks=mm` (R4, with `fill-holes`): cracks whose rim lies within `mm` of a line are closed
    by splitting, like exact cracks; the largest offset is reported.
  - `fins` (R5, after duplicates and orientation): an open part that makes an edge non-manifold
    and unbalanced (a flap folded onto a face, a sheet hanging from an edge) is removed whole,
    when removing it makes no edge more unbalanced and it is not the largest part on that edge;
    the body it hangs from is never removed. `cut-non-manifold` also removes every face on an
    edge still unbalanced, for `fill-holes` to close.
  - With `fill-holes` (R7, TD-092 r5): `split-rims` caps a rim through a vertex twice (at a
    pinch vertex) as its simple loops; `refine-holes` refines caps to the size of the faces
    around the hole and fairs the new vertices as a thin plate that continues the surface's
    slope (Liepa 2003, Kobbelt 1997), falling back to the unrefined cap when the refined one
    meets the mesh; rims beyond 512 edges need it. `unbounded-holes` lifts the 256-edge bound.
    Every cap is still validated against the mesh.
  - `visibility` (R6, before hole filling): an open part (a sheet, a surface with holes) is
    turned when rays from its faces reach the outside of the model's box unobstructed more
    often against its normals than along them; a tie leaves it.
  - `small-shells=<mm²>` and `small-volume=<mm³>` (R8, after hole filling) remove shells (faces
    joined through edges) with less surface, or closed shells with less volume; `inner-shells`
    removes a closed outward shell that crosses no other shell and lies inside another body
    (exact ray parity per shell). Cavities (reversed shells) and bodies floating in a cavity are
    never removed.
  - `rebuild[=mm]` (R9, last; default 0.05 mm, from 0.001 to 10): the model is rebuilt as the
    solid its winding number defines (the union of its bodies), sampled on a grid of that cell
    with the hollowing core's extraction: closed, manifold, oriented and free of
    self-intersections. The summary gives both deviations (the rebuilt surface from the source,
    certified; the source's vertices from the rebuilt surface, where thin features are lost) and
    the volumes. It needs a winding number: holes and fins must be repaired first (the `rebuild`
    preset is `aggressive` plus `rebuild`). The output is dense: about 10 million triangles for
    the Yorick at 0.05 mm, and a rebuild beyond 20 million triangles is refused.
  - `non-finite` (TD-092 r9): an STL whose facets have a NaN or infinite coordinate is sliced
    without them instead of being refused. They have no position, so whatever they formed is
    lost; the summary's import notes give how many were left out and in which runs of facets.
  - Tolerances are from 0 to 1 mm, and the summary reports the largest vertex move.
  - `exact` (TD-092) is the preset of every repair that moves no vertex and needs no tolerance:
    `non-finite,weld,degenerate,duplicates,fill-holes,split-rims,outward`, also closing zero-area cracks
    (TD-091); holes it cannot fill stay open and are reported instead of failing the repair.
    `standard` adds `weld`, `degenerate` and `cracks` at 0.001 mm, `fins` and `refine-holes`;
    `aggressive` adds `cut-non-manifold`, `unbounded-holes`, `visibility` and `small-shells=1`.
    `off` is `none`.
  - With `--repair none`, orientation conflicts are refused.
- `--allow-topology-defects`: by default, a mesh is refused before slicing when, with `union`, it
  has boundary edges or winding conflicts, or, with `strict`, it is not a closed combinatorial
  surface (boundary/non-manifold edges or vertices, excluded degenerate faces, isolated
  vertices). Union accepts non-manifold contacts between touching bodies, and holes while section
  closure is enabled. This flag skips the
  gate only; every sampled layer must still close (`union`) or resolve to complete strict even-odd
  regions (`strict`), or the export fails.
- `--progress` / `--no-progress` (mutually exclusive): progress is advisory and goes to stderr
  only. By default it is shown as one rewritten line when stderr is a terminal and omitted
  otherwise. `--progress` forces it; off a terminal it prints one `progress: ...` line per phase
  and per completed percentage of layers, with a linear remaining-time estimate. `--no-progress`
  disables it. Progress never changes stdout.
- `--hollow <wall mm>` and `--hollow-cell <mm>` hollow the placed model before slicing, using the
  sampled-distance backend (TD-045, [specification](M55F-SAMPLED-HOLLOWING-SPEC.md)).
  - What is removed: every interior point deeper than the wall. Convex and non-convex models,
    nested shells and multiple bodies are all handled.
  - The interior is decided by the winding number of the oriented surface: a point is inside
    where it is positive. Touching and overlapping bodies are hollowed as their union (TD-092
    H2): every surface keeps its wall, so overlapping bodies get one void per region, and a void
    inside two bodies is emptied in both. Inside-out bodies (negative winding) are refused.
  - The source must be closed and consistently oriented after `--repair`: no boundary edges,
    winding conflicts or unbalanced non-manifold edges. Faces of zero area and the zero-area
    cracks they leave at T-junctions are left out first (TD-092 H1: they change no point's inside
    or outside) and reported as `Hollowing left out as zero measure`. An open model needs
    `fill-holes` in `--repair`, and every hole must then be filled; the hollow fails otherwise.
  - With overlapping bodies stdout adds `Cavities inside overlapping bodies`.
    The repair is recorded as the first operation of the preparation plan, so the hollowed result
    is reproducible from the original file.
  - The wall is guaranteed (TD-093). The cavity is extracted at a raised offset,
    `wall + 0.40 · cell² / wall`, and every cavity face is proven, by exact tests, at least the
    wall away from the surface. When a pass falls short, the offset is raised and, if needed, the
    cell halved, at most six passes; otherwise the run fails with the thickest certified wall.
  - The cell defaults to the larger of `sqrt(0.05 · wall / 0.39)` mm (a 0.05 mm over-thickness)
    and the cell that keeps the cavity near 10 million triangles for the model's surface, below
    `(wall − 2e) / sqrt(3)`. `--hollow-cell` sets it instead and must satisfy
    `cell · sqrt(3) + 2e < wall`, where `e` is 1e-6 mm, or `min(1e-3, cell/32)` mm for drain holes
    (it is then also the hole polygon's approximation budget). A coarser cell is refused, never
    enlarged silently.
  - stdout additionally reports the guaranteed wall, the offset, the passes, the cell, the
    certified wall range, the thickest wall at cavity vertices, the cavity volume and its surface
    count.
  - Without drain holes the summary states that the enclosed cavity will trap uncured resin.
- `--verify-walls` checks the sliced layers after slicing (TD-095,
  [plan](RASTER-VERIFIER-PLAN.md)). It reads the exported masks, with the drain holes' cuts
  and the cavities rasterized alone, on the print's pixels and layers (a pixel is material when
  half of its samples are covered).
  - *Printed wall:* the thinnest wall between the cavity and the outside, by exact Euclidean
    distance between voxel centres, measured (less both voxels' extents along the line) and
    guaranteed (less two half-diagonals), with its layer and position.
  - *Breaches:* cavity that reaches the outside other than through a drain hole.
  - *Undrained pockets:* empty regions that reach the outside by no path, with drain holes open;
    voids of the model itself and reversed slivers left empty are pockets too.
  - stdout adds `Printed wall (mm)`, `Thinnest printed wall at`, `Printed wall thinner than … mm
    (cavity voxels)`, `Wall breaches`, `Undrained pockets` and `Wall verification`.
  - A breach, or a printed wall under the certified one by more than one voxel diagonal, fails
    the job with exit status 1 after its output is written, so it can be inspected.
  - Without hollowing, walls are compared with 1 mm. It takes time in proportion to the model's
    volume at print resolution: 26 s more for `Base_01.stl` at 1.3 mm on a 13,312 × 5,120 panel.
- `--log <file>` writes the job's log to the file (TD-096, [plan](LOGGING-PLAN.md)). The file
  is replaced, and capped at 20 MB. It holds the command line, the session's facts, the phases
  with their durations, imports, hollows, drain holes, the verification, the summary and any
  error. Without it nothing is written to disk, and outputs are the same either way. The desktop
  application keeps its own session logs (the 10 newest, in the cache root's `logs` folder).
- `--drain-hole <x> <y> <z> <axis> <length> <diameter>` (repeatable up to 32 times, requires `--hollow`) cuts a
  cylindrical drain hole. It starts at the world point `x y z` (after placement) and runs along
  `axis` (`+x -x +y -y +z -z`) for `length` mm, with the given diameter (TD-047,
  [specification](M55G-SAMPLED-DRAINAGE-SPEC.md)).
  - What is certified: the entry cap lies entirely outside the model (not in a sealed void), and
    the end cap entirely inside one cavity region. An entry is outside when a straight path (along
    an axis or a face or body diagonal) leads from it past the model's bounding box without
    touching the model. If every such path is blocked (an entry deep in a winding recess), the
    model's bodies must not touch, overlap or self-intersect, since that could seal the entry.
    Otherwise the command fails, naming the hole by its position and explaining the problem
    (entry not outside, no straight path out of a touching or self-intersecting model, ends
    before the cavity, passes through the cavity into or onto the far wall, misses the model,
    end cap not inside the cavity).
  - The reported clearances are certified lower bounds on the distance from each cap to the
    nearest surface, computed with bounded work; they can be somewhat below the true distance,
    but never above it and never below the required clearance.
  - Print resolution: a vertical hole needs a diameter of at least two pixel pitches, a horizontal
    one at least two pitches and two layer heights. Smaller holes are refused because they would
    not open in the sliced file.
  - What is reported: every hole with its region and clearances, the number of cavity regions, and
    whether any region is still enclosed.
  - Example: `--hollow 2 --drain-hole 50 30 -1 +z 4 3` drills a 3 mm hole up through the 2 mm
    floor of a model resting on the plate at X 50, Y 30.
  - **Any direction** (TD-066, [specification](HOLLOW-SIMPLIFY-DRAIN-SPEC.md)): the axis may
    be a vector `dx,dy,dz`, as in `--drain-hole 6 30 6 1,0,1 12 4`, a 45° hole.
  - `--surface-drain-hole <x> <y> <z> <diameter>` drills perpendicular to the surface nearest the
    point:
    - the axis is the area-weighted normal over the hole's radius;
    - the entry is 1 mm outside, and the end 1 mm past the first cavity surface along the axis;
    - the cut is then certified like any other.
    The summary prints every hole's built axis (`Drain hole N axis`).
  - Options after a `--surface-drain-hole` shape it (TD-089, M6.4c c5): `--hole-depth <mm>` sets
    a fixed depth from the surface, which needs no hollowing; `--hole-from cavity` starts it on
    the cavity's surface, cutting out through the wall; `--hole-tilt <angle> <direction>` leans
    its axis by up to 60° towards a direction measured around the normal. Example:
    `--surface-drain-hole 20 30 20 4 --hole-depth 5 --hole-tilt 30 0`.
  - A tilted hole must also be at least two layer heights times the sine of its tilt wide.
- `--hollow-simplify <mm|auto>` (with `--hollow`, TD-066) simplifies the cavity surface.
  - **Bounds:** the surface moves at most the tolerance, and the walls never get thinner than the
    thinnest original wall, proven exactly for every new face.
  - **`auto`:** half the smaller pixel pitch, at most half the layer height.
  - **Effect:** flat walls lose almost all their triangles. A 60 mm sphere's cavity drops from
    1.4 million to 0.23 million triangles at 0.007 mm.
  - **Summary:** the tolerance, the certified deviation, the triangle counts and the wall floor.
  - A model with no point deeper than the wall fails with "nothing to hollow".

Layer *i* covers `[i·h, (i+1)·h]` and is sampled at its midpoint; planes run through the model's
top Z. When a midpoint lies exactly at the height of a model vertex (for example on the top face of
a plate carrying a boss), the layer is sampled instead in the middle of the vertex-free interval
just below it, within the same slab, so it holds the material just below that face (TD-048). Masks are binary (one sample per pixel, no antialiasing or compensation) and use the
printer's explicit origin/orientation.

Slicing uses a per-face Z index, so each layer evaluates only the faces that cross it (TD-046).
Layers are generated and machine files encoded on all available cores, with output identical to
sequential processing (TD-050). Each layer is held as nonzero row spans, never as a dense image
(TD-055), so time and memory follow the model's outline rather than the panel size. FX-11 (1200
layers) slices at 15120 × 6230 in about 0.5 s with a 61 MB peak. Layers in flight are bounded by
two per thread and by the 1 GiB raster and 512 MiB encoding windows over their retained span bytes.
Per-layer stage limits are 2,000,000 evaluated faces per plane and 40,000,000 box-overlapping
planar-contact tests. Denser layers fail explicitly. On success stdout reports layers, output bytes, sampled mask volume
(mm³) and estimated print time; exit status is 0. Usage errors exit 2; profile, geometry,
placement and export failures exit 1 with stdout empty.

**Ctrl-C.** The first Ctrl-C (SIGINT on Unix, the console control event on Windows) requests
cooperative cancellation through the core checkpoints of import, topology, placement, job planning,
layer generation, rasterization and export. The command then removes its own staging data, publishes
nothing, prints `error: interrupted by Ctrl-C; no output was published` and exits **130**. Latency
depends on the current uninterruptible step (for example a sort or one I/O call), not a wall-clock
guarantee. A request after the final publication checkpoint can still complete successfully. A
second Ctrl-C exits immediately with status 130 and may leave a hidden staging entry
(`.<name>.karasu-partial-…`) next to the output; other termination signals are not intercepted.
An error after the output is published (for example a closed standard output) is reported as
itself, not as an interruption. Machine files are published atomically by hard link; on media
without hard links (FAT32/exFAT USB sticks and SD cards) they are copied into an exclusively created
destination, which never replaces an existing file but is not atomic.

Supports are not exposed through the CLI.
Exported files are format-reader validated, not certified on physical printers.
