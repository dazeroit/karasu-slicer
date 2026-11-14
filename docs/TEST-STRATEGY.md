# Test Strategy

Current naming uses `karasu-slicer`, `karasu-cli` and `karasu-core` (TD-033). Historical command,
package and external evidence identities below retain their original spelling; translate package
selectors/imports using the migration notes in README when reproducing them against current source.


## Purpose

Testing provides evidence that Resin Slicer preserves topology, dimensional intent, numerical
classification, deterministic output, and format validity across Windows and Linux. Coverage numbers
alone are not acceptance evidence for geometric algorithms.

## Test layers

### Unit tests

Keep focused tests beside implementation modules. Cover invariants, boundaries, all classification
branches, invalid values, tolerance thresholds, and error context. M0 includes identity and CLI
argument tests only because no processing behavior exists yet.

### Integration tests

Exercise public crate APIs and complete stage boundaries: import-to-mesh, mesh-to-regions,
regions-to-mask, profile-to-job, and job-to-export. Verify diagnostics as well as successful data.
CLI integration tests should later verify streams and exit statuses without coupling domain tests to
presentation text.
`crates/karasu-cli/tests/slice.rs` (TD-043) runs the native executable end to end: generated
analytic boxes and schema-serialized profiles on a 1 mm-pitch panel give exact layer counts and
mask volumes for every machine format and the reference directory. It also checks placement and
envelope refusals, create-only publication without staging leaks, the topology gate and waiver,
profile/usage errors (exit 1/2, empty stdout) and byte-identical output from the `resin-cli` alias.
TD-044 adds progress-on-stderr checks and a Unix-only SIGINT test. The SIGINT test waits for the
first layer progress line, sends a real signal and asserts exit 130, an empty stdout and no
published or staging files.

TD-045/TD-046 add the following suites:
- `tests/sampled_hollowing.rs`: analytic cube, torus, sphere-convergence and inner-void cavities,
  with volume and certified wall checks; identical output for 1/2/3/8 threads; rejection of thin,
  open and coarse-cell inputs; limits; every cancellation checkpoint; stage order.
- Unit tests:
  - BVH against brute force;
  - exact parity with nested shells and projected-edge cases;
  - closed-form point-triangle distances;
  - Kuhn tiling, plus closed oriented patches for all 254 sign patterns;
  - FIPS 180-4 SHA-256 vectors;
  - interval-filtered against exact orientation on 60,000 adversarial cases.
- `tests/face_z_index.rs`: indexed against full layer generation over the corpus, at vertex/face
  heights and on a transformed dense mesh.
- CLI tests: `--hollow` mask volume, cavity volume, certificate output and request validation.

TD-050 parallel encoding adds export tests: byte-identical files, sizes and volumes against
sequential export for every built-in format and pixel policy, with several thread counts and a
one-layer window; identical failures for source errors, short and long sources; worker
cancellation.

TD-068 (M6.0) adds:
- **`karasu-app` unit tests:**
  - palette token values;
  - WCAG AA contrast of text on background, panels and primary fills;
  - visuals built only from tokens;
  - deterministic adapter selection (modern backends before OpenGL, device classes, ties, no
    presenting adapter);
  - backend and option parsing;
  - the splash artwork (format, size, transparent corners) and startup progress (real steps,
    idempotent completion, minimum display time) (TD-069).
- **A runtime smoke test** (`--smoke-test`): it opens the window, renders frames and reports the
  adapter. Linux CI runs it where a display exists; native Windows runs it through
  `validate.ps1 -IncludeApp`, with the automatic choice and with OpenGL. GUI rendering is not part
  of `cargo test`, which must also pass on machines without a display.

TD-070 (M6.1a) adds:
- **`karasu-project` library tests:**
  - a job run from the library publishes its output and returns the summary;
  - phases and layers reach the observer, in order;
  - outputs stay create-only;
  - a job cancelled at the first checkpoint or when layer generation starts publishes nothing;
  - request validation and the format registry.
- **CLI byte identity:** the CLI built before and after the move ran a 20-case comparison suite,
  and all 479 files (outputs, summaries, stderr, exit codes) matched. The workspace tests are
  unchanged.

The imported-model cache (TD-089 follow-up) adds a project test: a model sliced after a move is
not imported, checked or repaired again, a change of repairs repairs the kept import again, and
every cached slice writes the same bytes as an uncached one. The probe on demand adds a project
test (solid and hollowed objects get a probe only with holes or when asked) and an app test (the
objects the interface asks probes for).

TD-095 v1 adds the raster-side wall verifier (`raster::verify`), tested against a brute-force 3D
search. The smallest cavity-to-exterior distance and the cavity voxel count must be equal on:
- a box with walls of 2 columns, 3 rows and 3 layers, whose measured wall is exactly two
  columns;
- a floor resting on the first layer, measured against the empty space below it;
- twelve random ellipsoidal or box shells with off-centre voids, pillars and varied layer
  heights (at least six of them enclose a cavity, so the check is not vacuous).

It also checks:
- a slit that breaches the expected cavity, against the same opening given as a drain hole
  (no breach, no pocket, unchanged walls);
- an enclosed void reported as an undrained pocket;
- the material threshold on antialiased pixels;
- refused inputs.

An ignored test measures a print-scale shell. A layer wider than two of the column pass's former
blocks (140 columns) is checked against brute force too.

TD-095 v2 takes it to the command line:
- **`slice --verify-walls`:** a solid cube has no cavity; hollowed and closed, its cavity is one
  undrained pocket, with a printed wall within a pixel of the certified one; a drain hole
  through the top drains it and is not a breach.
- **Off by default:** the summary has no verification lines.
- **Option:** given only once.
- **Verdict:** a breach, or a wall more than one voxel under the certified one, fails; with no
  certified wall only a breach fails.

The icons (M6.5) are tested to decode as 96-pixel squares, white wherever they show (so tinting
gives the theme's colour exactly), with strokes drawn and empty corners.

Toasts (TD-099) are tested for their time running out, pausing under the pointer and errors
staying; the severities' default durations; at most three drawn, the newest; and every message
showing as a toast for its lasting while staying in the Messages window. The notification
fills pass 4.5:1 with the primary text.

The Transform panel (M6.5): typed values move the centre, set the lowest point's height (0
rests it), set angles and scale (uniformly or along one axis), and repeated typing of one value
is one undo step; Fit to plate takes the tightest axis and stays inside; locks belong to their
section; the panel fits its width with and without a model, in English and in the pseudo-locale.

Local axes (M6.5): an object turned 90 degrees about Z has its local X arrow along the world's
Y, a drag along it moves the object along Y, the free-move square stays on the plate's plane,
and a turn about the local X equals a turn about the world's Y.

Panel widths (M6.5): a row wider than a panel is cut and never widens it; the Repair panel with
every stage on fits its width in English and in the pseudo-locale (a horizontal row of the old
kind makes it 420 points wide, which the test catches). The value field is tested for its
parsing, fixed width and drawing.

The Repair panel (M6.5) is tested to keep its structure without a selection, its preset showing
what an opened model gets, and to change nothing then; and for its draft: a repair chosen
changes nothing until applied, applied it waits for its preparation to report, an edit before
then cancels the report, and a draft follows its model. The switch widget is tested to turn on
a click unless disabled, and the theme's roomier spacing against egui's.

The Hollow and Drain holes panels (M6.5) are tested to keep their structure without a
selection, buttons in their footers included, and to change nothing then; to fit their width
with every setting shown, in English and in the pseudo-locale, on a hollowed cube; for the
footers' state, most important first (a solid model, a thin draft wall, placing, a wall changed
by scaling, an enclosed cavity leading to the Drain holes tool); for the resin saved by the
cube's cavity; for applied hollowing reported once prepared and not after another edit; and for
a model never hollowed starting from the tool's hollowing. A sliding choice is tested to refuse
a click on an unavailable option.

Cavity regions (TD-101): the core test of two separate cubes checks that each region's point is
on it and its bounds are those of its cube less the wall, within a cell. The interface tests
number undrained regions among all regions and pockets among themselves, show a region once from
its row or its marker (found under the pointer, not elsewhere), keep a line with Show within the
panel's width in English and in the pseudo-locale, and cut the section at a height.

The Slice and export panel (M6.5) is tested to keep its structure without profiles, saying what
is missing and changing nothing, and to follow a slice of a hollowed cube: not sliced, a
failure said until a slice ends, running, done with the slice's facts, out of date after an
edit; with Advanced on it fits its width in English and in the pseudo-locale.

Layer heights (TD-102): ranges make a document the core plans and read back the same; a range
is checked for its start, its printer limits and an exposure table that covers it; a plan set on
a cube's plate is planned exactly without slopes and approximately on a thread with an adaptive
range, and a slice of a fixed range has the planned layers' count. The section bar snaps its
handles to variable layers, keeps them a layer apart, and steps the active one with the keys.

The Layers tool (TD-103) is tested without a slice (its structure and notice) and on a sliced
hollowed cube: the first layer a bottom one, browsing kept within the slice, Follow at the
layer's top, a middle layer one island with an area under the cube's, and the panel within its
width in English and in the pseudo-locale. The exposed area weighs pixels by intensity; islands
join runs across rows and corners; a zoom keeps the point under the pointer and stays in the
image. A slice's resolved layers carry a fixed range's thickness, exposure and phase. The track
slider of the footer and the larger window takes the first, the middle and the last value under
a click. A verified slice lists its printed walls' findings in the Layers panel,
the closed cube's cavity among them as a pocket.

Automatic orientation (TD-119): in the core, tilts turning their direction straight down about
a horizontal axis, tilts keeping to their axis and limit, a flat box needing nothing and on an
edge facing the plate, a floating box starting an island at its lowest corner, protected points
refusing tilts and weighing near them, sections with a cavity subtracted, and the ranking and
separation; `orientation_budget` (ignored, `KARASU_ORIENT_MODEL`) times the rough pass and the
sections. In the interface: values in range and priorities setting the weights, the turn's
pivot and lift, tilts in words, a flat box proposed as it lies and turned as one step (supports
asked first, undone), and a protected point following the model and refusing tilts; the ignored
`a_local_model_is_oriented` times a whole search on a local model, solid or hollowed.

A resin's colour and a setting's heating temperature (TD-118): profiles without them read and
keep their bytes, set they round-trip, and a bad swatch, a blank colour or a temperature out of
range is refused; the PP1 writer records the job's heating target; the resin form keeps the colour
and swatch and leaves them out when empty.

Printer and resin profiles (TD-106): the catalog is complete (every file under
`assets/catalog` compiled in and listed), valid, grouped by vendor, with settings for catalog
printers and resins within their limits, and every catalog printer slices a box in each of its
formats with its setting. The library adds catalog printers under unique names with their
official settings, reuses a resin copy, makes starting points only on request and from the same
printer, marks an edited setting as the user's, duplicates printers with their settings, removes
dependents with a printer or a resin (history kept), offers a newer catalog revision with its
changes and applies or dismisses it, converts a version-one library once (files kept, failures
reported), and imports profiles with free identities. Core tests cover the new documents' round
trips and refusals, the composed job resin, the separate store and its removal, and catalog
differences; the command line slices a library resin and setting exactly as the calibrated resin
with the same options. The app tests a plate's selection and its following of the library, and
draws every profile window.

The status bar's line of short messages (M6.5) is tested to keep to the width it is given,
cutting a long message.

M6.5 u4 (TD-098) adds `karasu-app` tests for the workflow bar: each step keeps its tool and a
tool chooses its step, a step without tools has no panel, every tool belongs to the step that
lists it, the collapsed bar names the step and tool, and the bar draws headless, open and
collapsed, without acting by itself.

M6.5 u1 (TD-097) adds `karasu-app` tests for the actions' keymap, headless: the default
shortcuts reach their actions, Ctrl+Shift+Z is redo and never undo whatever the bindings' order,
a bare key does not fire with Ctrl held, no two bindings share a shortcut, every action has a
label, and tooltips name the shortcut.

M6.4f (viewport review) adds `karasu-app` tests for:
- **Zoom at a point:** the point under the pointer stays under it while zooming in and out, the
  target moves towards it, and the distance stops at its new floor (0.05 mm), so miniatures can
  be filled with the view.
- **Looking from a direction:** the yaw and pitch for faces, edges and corners, and straight
  down with the plate's front at the bottom of the screen.
- **The view cube:** its faces are right-handed and its cells give the 26 views, with one cell
  per face of a view (two for an edge, three for a corner); the cell found under a point is the
  one drawn there; each face's name reads left to right and upright when seen square on; a turn
  eases to its view, the shorter way round. A headless egui run tessellates the cube for views
  with faces edge-on, with a view lit, and every vertex must stay by the cube: drawn as egui
  polygons, those faces' unlimited mitres threw a vertex 56,000 points away.
- **The selection outline:** its scissor covers the object's box on screen grown by the width,
  is clipped to the image, and is the whole image when the box reaches behind the eye; the
  naga test checks its shaders. The smoke test's measured orbit now has the first object
  selected, so its frame times include the outline.
- **The colours (now the settings dialog's Appearance):** egui's picker moves a translucent
  colour by one channel step each frame, which the former colours test window caught; colours
  are written back only when changed.
- **The overhang analysis (TD-107):** local minima on built shapes (a downward tip, a floating
  cube's level face, a cube turned onto a corner and a quarter turn with rounding, a flat
  torus's lowest loop, a hollow cube whose cavity floor is no minimum, a mirrored placement) and
  through `inspect --minima`; unsupported regions on hand-drawn masks (an island in mid-air, a
  slope exactly at the angle that holds and a flatter one that hangs out, a ceiling over a
  pillar, a corner contact, refused parameters) and through `slice --unsupported` (an island,
  and the same box bridged to its pillar, which leaves overhangs only); the slope tint's uniform
  packing and the shader's validation.
- **The suction cups (TD-114):** on drawn masks, a closed box (one cup through its cavity that
  closes, with exact area, volume, cap and bounds), a ring on the plate and a lifted one, a side
  vent wider and narrower than the minimum vent, a floor vent with and without a minimum, two
  chimneys over one chamber, two separate cups sorted, and refused parameters; through
  `slice --suction-cups`, a ring on the plate, a slotted ring vented without a minimum and too
  narrow at 20 mm², and misused options. The ten reference shapes of the research agree with
  UVtools layer for layer (recorded, not automated). In the interface, a hollowed cube resting
  on the plate is searched from the Drain holes tool: its cavity is one cup that closes, shown,
  framed at its widest layer, and vented sideways through a wall.
- **Hollowing blockers (TD-115):** a 20 mm cube hollowed at 2 mm against closed forms: a box
  keeping half the cavity solid, a slab splitting it in two, a sphere left solid inside it, a
  pyramid on its floor, blockers in the source frame of a scaled model, a blocker over the whole
  cavity refused as nothing to hollow, invalid blockers refused and records told apart; the
  project's pipeline with a blocker and the same blocker turned with swapped sides. In the
  interface: the meshes closed outwards at their volumes, rays hitting blockers in a scaled
  model's frame, gizmo drags moving and turning in the model frame, and a cube's blocker placed
  by a click, selected, dragged, applied with the hollowing, deleted, the Solid base's box, and
  Remove hollowing. The size handles: where a turned box's and a sphere's stand, one base sized
  alone, a base moved with the other staying, the ranges kept, and a drag on a scaled model
  measured in model millimetres.
- **Contacts fitted to the surface (TD-116):** in the core, on prisms: open surface and a
  concave corner keep the contact, a tip near an edge narrows to the first diameter whose
  footprint has room and to the smallest, reported, closer than that, a slab thinner than the
  penetration narrows the contact and its penetration, and a ball fits by its circle on the
  surface; `tests/manual_supports.rs` builds fitted Heavy contacts under a slab (kept, narrowed,
  at the smallest with a warning) and refuses a smallest out of range or wider than the
  contact. In the interface: the fields' coupling, the fitting kept across presets and still
  matching its preset, and the Islands and Automatic tools' supports fitted and, when asked,
  ending in a ball (`KARASU_INTERFACE_SPHERE=1` in the local-model measurement). The Automatic
  tool's local-model measurement fits down to `KARASU_INTERFACE_FIT` when set, and reports the
  build's time and the contacts narrowed.
- **Presets and the outline's tips (TD-136):** the core computes the presets' widths at the
  surface (and a contact sunk past its tip, flush, or ending in a ball); automatic braces take
  the thinner shaft up to their largest diameter, which a document writes, reads back, and
  refuses beside a diameter of its own. In the Automatic tool's plan: a level tile takes a
  corner tip the inset in from both edges at each corner and edge tips the inset in from each
  edge, kept in the plan none closer than the spacing; the slab's overhang has no edge where its
  bottom goes on over the pillar; a drain hole's rim is an edge; a tile tilted by 30° takes the
  tips under its lowest edge the contact's radius from it along the slope, and side tips the
  same radius up the face beside it, which the plan holds alternating with the tips under the
  edge; a level tile takes none. Settings
  of schema 3 take Medium, no braces and 1.0 mm braces where they kept the old defaults.
- **The contact's line and the cavity's values (TD-137):** the viewport packs the line's
  uniforms and the frame's eleven vectors; a box's contour at a height is its walls, and the
  point taken is the one nearest the surface under the pointer, or on the screen nearest the
  pointer, front before back; on a lifted cube Shift holds the bottom's height, a wall higher up
  puts the tip at that height, a level face keeps its own point, a section hiding the height
  leaves none, and letting go frees it. The Automatic tool plans a hollowed cube's cavity with
  its own preset and density, the outside as before, and each place alone with its own values;
  the settings bring the values, the cavity's preset and the line's colour back.
- **Supports on a grid (TD-138):** the face picked on synthetic slabs, the outline's corner,
  edge and side tips and the grid's rows (along the longest edge, or turned), level and tilted;
  the mode on a lifted slab from the click to Apply, Undo and Discard; the rectangle and the
  lasso from below (the shape kept as its triangles, the tips inside it, no edge tips along a
  border across the face), and a border across a face that goes on taking no edge tips. **Local models:**
  `a_local_model_s_base_is_gridded` (ignored; model in `KARASU_INTERFACE_MODEL`) lifts a model
  5 mm, clicks its widest level triangle facing down and times the face, the tips and the
  supports.
- **Arrange, Slice and Export every plate (TD-135, phases 4 and 5):** the workspace arranges
  six slabs and a plate-wide one on a Photon Mono X (four on the plate inside its area and the
  spacing apart, two on a new plate, the widest left; Stay leaves three), one undoable step,
  which the project's test of several plates added in one edit covers. The window's tests run a
  batch frame by frame as the window does: both plates of a hollowed cube sliced, the one not
  shown too; an edited plate sliced again before both files are written; existing files asked
  about first; a plate out of date left out and listed with the switch off; a batch stopped,
  its slice cancelled without a message of its own. The dialogs draw their fields and rows;
  the file names' pattern gives names every system accepts.
- **Mesh Booleans (TD-140):** the integers against `i128` and the ring laws, at their extremes;
  the grid's rounding and range; planes and meeting points against karasu's exact `orient3d`, at
  the corners of the admitted cube; cuts of random polygons; walks through boxes, corners and
  overlaps; the filters against the exact sides, zero cases included; the quick contact tests
  against the general ones on 40,000 random pairs. The arrangement is checked against an
  independent oracle (a vertical ray through the inputs' triangles in plain integers) at random
  points: the faces' winding number is the result's membership, for unions, intersections and
  differences of boxes, coplanar and identical faces, self-overlapping inputs, random tetrahedra
  and a drilled hollow box; and its faces do not depend on how the work is shared among threads.
  The result as a mesh: the exact triangulation and the rounded mesh are closed (every edge used
  once in each direction), the oracle confirms membership exactly where nothing rounds and within
  rounding's reach elsewhere, points round exactly with ties to even, and `solve` refuses open
  inputs and coordinates beyond the range and reports what snapping and rounding did.
  `a_fine_shell_drilled_by_four_holes` (ignored, three of them) measures 540,048 triangles. A
  block drilled through comes out with its 32 triangles and nothing rounded (the vertices cells
  made are taken out again).
- **Boolean threads and cancellation (TD-140, 2026-11-09):** the parallel BVH pair search finds
  the same pairs as the serial one, in the same order on every run, and stops when cancelled;
  the cancellable parallel fill fills in order and stops items already running; `solve` stops at
  each of its checkpoints (contacts, cells, mesh). On a real model, the drilled mesh's hash is
  compared with the previous commit's when the kernel is reorganised.
- **The Boolean's output on the grid (TD-140 phase 5):** a block drilled on the grid needs no
  round; a separate body no larger than a snap cell is left out and counted, one a cell longer
  is kept, and a result that is only a speck is kept; the drilled sphere's rounding crossings are resolved in one or two rounds, nothing left,
  every vertex an exact `f32`; grid points within ±512 mm are `f32` values. On a real model the
  layer comparison allows each differing pixel the reported move (`farthest_mm`, on three axes),
  measured to the model's surface and the holes' walls, and streams the layers.
- **Model files (TD-139):** every format (binary STL, 3MF, OBJ) reads back through karasu's
  importers as the stored `f32` corners, the same bodies give the same bytes, 3MF names and
  builds each body, and an existing file, a wrong extension, a coordinate beyond `f32` and a
  cancellation leave the folder as it was; a confirmed replacement takes the old file's place,
  never before the new one is whole; two cubes touching along an edge come out of 3MF as two
  sheets, each edge used by two triangles. `karasu-project/tests/model_export.rs` exports a
  plate (a slab on supports and a raft, a hollowed block with a hole) per plate and per model,
  with and without supports and raft, one model alone, over an existing file, cancelled, and as
  a background job; the command line's options and the dialog (drawn, a folder export, asked
  before replacing, a model's own file) have their tests.
- **Drain holes cut into a mesh (TD-140 phase 4):** `drilled_models` drills a hollow sphere with
  six holes (into the cavity, overlapping, through both walls, slanted, ending in the wall,
  beside the model): closed, nothing dropped, flat or crossing, volume reduced, and its layers
  equal to the shell's sliced with the holes' cuts but for boundary pixels (neighbours that
  differ); a block with a blind and a through hole (nothing rounded, corners kept, removed volume
  within 1 % of the cylinders', deterministic); holes beside a model (unchanged); and the
  refusals (an open model, a coordinate beyond the range, cancellation).
  `karasu-project/tests/drilled_project.rs` (ignored; `KARASU_DRILL_PROJECT=<project.karasu>`)
  drills every hollowed model of a local project with its holes, reports the counts and the
  topology, and compares its layers at 0.05 mm with the shell's sliced with the cuts: a pixel
  that differs away from a boundary must sample within 0.5 µm of the model's surface (a feature
  thinner than a pixel); `KARASU_DRILL_COMPARE=0` measures the cut alone.
- **The settings (TD-097):** the store's tests read, save only changes, read an earlier file
  with defaults, leave a damaged or newer file as it is, and start over keeping it beside; the
  window's settings applied and collected back are equal (a setting forgotten in either fails);
  every section of the settings dialog draws inside it; a new shortcut is taken from the keys,
  another action's asks first and Escape cancels; shortcuts keep only what differs from the
  defaults and read back; a resizable dialog reopens at its remembered size.
- **Surface filters:** the encoding a filter reaches the shader with, the uniform block's size,
  and the build volume the filter tests against (none without a printer). The per-fragment
  test itself is in the shaders the naga test validates; it was checked by eye on native
  Windows.

TD-096 g4 validates the logs:
- logging costs no measurable time on `Base_01.stl`;
- on native Windows the application's smoke runs leave session files holding the session's
  facts, the adapter, the file opened, the message shown and an egui warning.

TD-096 g3 adds `slice --log <file>`. The test checks:
- the output with the log is byte-identical to the one without;
- the log holds the session, the command line, the import, the hollow, the end and the
  summary;
- a refused job's error is logged;
- the option may be given only once, and an unwritable log fails the job.

TD-096 g2 records the application's work. A test installs the logger in memory and verifies a
hollowed, drained cube from the layer panel. The ring then describes each step: opening and
import, the slice asked for, the hollow, the drain-hole index, the verification, the phase
times, the end, the summary, and the message shown.

TD-096 g1 adds the session logger (`karasu_project::logging`). Unit tests cover:
- the UTC times, file names and one-line format;
- the levels of our areas and of other targets;
- the ring's order and bound;
- pruning old sessions without touching other files;
- the file cap with its last line, and buffering until a flush.

An integration test in its own process installs the logger. It checks the `log` facade's
records, filtering, a refused second installation, and a caught panic recorded with its
backtrace and flushed to the file.

TD-095 v4 validates it:
- **A real breach:** a 6 mm cube hollowed at 0.3 mm on 1 mm pixels has no printed side wall. The
  job writes its file, reports the breach and fails with exit status 1.
- **The campaign** (`wall-verification/` in the external evidence) covers `Base_01.stl` from
  2.0 to 0.3 mm, with and without a drain hole, and FX-11, FX-12 and the Yorick at 2.0, 1.0 and
  0.5 mm. It checks the printed walls against the certificate, the drained and undrained
  pockets, and the time and memory against plain slicing.

TD-095 v3 brings it to the interface:
- **The map:** pockets, thin walls and breaches are painted over the reduced layer image, each
  over the one before.
- **The panel:** a hollowed cube on a preset printer, sliced and then verified from the
  panel:
  - the result carries the verification, with one undrained pocket and one map entry per layer;
  - the layer shown is kept;
  - one message says the walls are verified.

TD-094 makes the hollowing's limits known in advance and sizes them to the machine:
- **Holes unavailable:**
  - a preview under a budget no hole index fits says so at once, with and without holes and
    while holes are placed, and keeps the object; slicing it with a hole is refused;
  - in the application the panel reads it before any hole is placed.
- **Budget:** a cavity over the budget is refused with its remedy; the default cap is the
  hollowing's 20 M triangles, which the default index takes, and a budget caps the cavity at what
  its index takes.
- **Prediction:** the predicted minimum wall fits the cap, with a 10 % margin, and 1 % less
  does not (a source the size of `Base_01.stl`: 0.26–0.29 mm by default); the panel warns below it, and for an explicit
  cell too fine.
- **Holes on the first move:** holes applied and prepared still follow the object, from their
  held verdicts, the first time it is moved; they were taken before the holes' preparation
  finished, and are now taken again with each finished preparation's probe.
- **Parallel hierarchies:** subtrees built on several threads give the nodes and order of one
  thread; the indexed build, which writes triangles once in slot order, gives the hierarchy,
  slots, labels and ids of the build that reorders a copy.
- **Probe cache:** adding a hole reuses a hollowed object's probe, moving the object indexes it
  again, and slicing from the cache writes what slicing without one writes.
- **Face Z index:** its integer key orders values as `f64::total_cmp`, signed zeros, infinities
  and NaNs included, so the parallel sort keeps the sequential order.
- **Viewport parts:** a mesh over the buffer limit is split into parts that keep every
  triangle, each part's vertices its own and distinct; within the limit, one part over every
  vertex.
- **Lean index:** the in-place reordering puts every triangle and label in its slot through
  cycles of every length; the command-line comparison stays byte-identical (507 files).
- **Measured:** `Base_01.stl` hollowed and drilled from the panel at 0.5, 0.35 and 0.3 mm
  (3.2, 5.5 and 7.0 GB), which sizes the application's budget.

M6.4e h5 (TD-093) validates the milestone with the product code: `hollow_guarantee_budgets`
over the experiment's twenty models and FX-11, FX-12 and the Yorick, at 2.0, 1.3, 1.0 and 0.5 mm,
one process per model and wall for its peak memory. Every hollowable run is guaranteed in one pass
(results in [HOLLOW-CERTIFIED-PLAN](HOLLOW-CERTIFIED-PLAN.md#progress), h5). Native Windows runs
the complete workspace and the desktop application (`validate.ps1 -IncludeApp`).

M6.4e h4 (TD-093) takes the guarantee to the interface:
- **Automatic cell:** a cube hollowed from the panel's defaults is hollowed on the rule's cell,
  coarser than the explicit default, with the guaranteed wall at least the one set.
- **Local models:** `a_local_model_is_hollowed_and_drained_from_the_panel` (ignored; model in
  `KARASU_INTERFACE_MODEL`, wall in `KARASU_INTERFACE_WALL`, default 1 mm) hollows a model from
  the panel, places a new hole from above where it drains the cavity, applies it and checks a
  region drains. At 1 mm every reference fixture but FX-10b (refused, non-manifold) hollows in
  one pass; every one but FX-04 (no draining place on its top) is drained.
- **Catalog:** the i18n test checks the new messages are used and the simplification ones gone.

The Islands tool (TD-108, 2026-10-29):
- **Local models:** `a_local_model_s_islands_are_found_and_supported` (ignored; model in
  `KARASU_INTERFACE_MODEL`, lift in `KARASU_INTERFACE_LIFT`, default 5 mm, hollowed when
  `KARASU_INTERFACE_WALL` is set) centres and lifts a model, finds its islands, grids its
  surface for routes and plans their supports with Light and Medium, printing each phase's time
  and the islands skipped by reason. Run it in release.

M6.4e h3 (TD-093) takes the guarantee to the command line:
- **Hollowing:** a 20 mm cube at 2 mm reports the rule's cell, the predicted offset, one pass, a
  guaranteed wall between 2 mm and the offset, and a cavity matching the cube eroded by the
  offset.
- **Drain holes:** a surface drain hole's length follows the offset (1 mm out, through the wall,
  1 mm past the cavity).
- **Comparison suite:** only the six hollowed cases change, as recorded under TD-093.

M6.4e h2 (TD-093) guarantees the requested wall:
- **The rule:** its arithmetic (over-thickness, budget and grid bounds; the cube's area).
- **One pass:** a box with a void reaches the requested 2 mm in one pass at the predicted offset,
  where the same cell without the guarantee falls short.
- **Correction:** a coefficient far too low falls short and is corrected in a second pass,
  identically for one, three and eight threads.
- **Plan:** the guarantee is recorded only when set; invalid coefficients are refused; the
  `WallNotReached` message names the requested and certified walls and the passes.
- **Measurements:** `hollow_guarantee_budgets` (ignored; models in `KARASU_GUARANTEE_MODELS`)
  prints passes, certified wall, thickest spot, cavity size and time per model and wall, and
  fails if a wall is not reached.

M6.4e h1 (TD-093) proves the hollow wall per cavity face:
- **Soundness and tightness:** a unit test hollows an L-shaped prism at a fine and a coarse cell,
  and checks the certified minimum wall against the brute-force minimum distance over every
  cavity–source face pair. It may never exceed it, and may fall below it by at most the proof's
  1 µm slack and the query error. At the coarse cell, faces come 0.044 mm closer than their
  vertices.
- **Flat walls:** a cube's certified wall stays within 1.1 µm of its thinnest vertex at 0.25 mm
  and 1 mm cells.
- **Unchanged:** the existing thread-count, simplification and placement tests still hold.

M6.4d r9 (TD-092) validates repair:
- **Corpus:** `repair_corpus` generates 25 defect classes (inside-out, reversed, duplicated and
  needle faces, split vertices, flat caps, folds, T-junction and near-collinear cracks at 0,
  0.5 and 10 µm, a seam across a 1 µm cell border, fins on 3 faces, shared edges on 4 faces,
  pinch vertices, non-simple holes, holes of 48 and 300 edges, a model and its cavity turned
  inside out, cavities, overlapping, nested and small shells, an open sheet, an open box turned
  inside out) and repairs each with Exact, Standard and Aggressive. For each it checks the
  expected closure, that a derived mesh comes exactly with a recorded and located change, the
  winding number (kept on a closed source unless the report records a turn or a removal, and
  the expected one at probe points once closed), and that a second run is identical; welding at
  0, 1 and 10 µm moves no vertex farther than its tolerance. `--nocapture` prints the corpus
  table; `KARASU_CORPUS_EXPORT=<dir>` writes the cases as STL for other slicers.
- **Fairing:** a planar rim at a crease stays flat; one on a gently curved surface is faired.
- **Facets without a position:** STL import refuses NaN or infinite coordinates by default and,
  with `NonFinitePolicy::Drop`, leaves those facets out with their runs counted (binary and
  ASCII, bounded run list, kept metadata aligned, the refusal reproduced word for word); the
  pipeline refuses such a model with the import's message and `Remedy::DropNonFinite` unless the
  repairs accept it, and then writes the same output as the model without those facets; the
  diagnosis notes them (ready with notes; handled by Exact); the interface offers "Repair:
  Exact" for the refusal.
- **Budgets:** `repair_budgets` (ignored; models in `KARASU_BUDGET_MODELS`, the rebuild model in
  `KARASU_REBUILD_MODEL`) times each stage alone in release against its budget. Results in the
  TD-092 r9 record.

M6.4d r8 (TD-092) adds, in the interface:
- **Acceptance:** `every_corpus_model_reaches_slicing_from_the_interface` opens every tracked
  fixture as a user does (profiles chosen, repaired on import with Exact), slices it with the
  pipeline, and on a failure applies the first fix its message offers: FX-01 to FX-10A slice
  directly, FX-10B after "Repair: Standard".
- the preset ladder; failed-slice messages offering the standard repair first and the rebuild
  last.
- **Project:** a lifted object is previewed with its cavity and hole, and slicing refuses it.
- **Comparison suite:** byte-identical to the c4 baseline.

M6.4d r7 (TD-092) adds `rebuild_solid` in the core:
- a box rebuilt closed and outward, every rebuilt vertex within the certified deviation of the
  box's exact surface;
- overlapping boxes rebuilt as one shell of the union's volume, without self-intersections;
- a plate thinner than the resolution lost and located by the source deviation;
- open surfaces and resolutions out of range refused;
- every tracked fixture (FX-01 to FX-10B) closed and manifold after the rebuild preset at 0.5 mm;
- identical results with one thread and eight.
- **CLI:** `rebuild` and `rebuild=mm`, and their refusals.
- **Comparison suite:** byte-identical to the c4 baseline.

M6.4d r6 (TD-092) adds `shell_repair` in the core:
- small shells removed by area (three specks) and by volume (with a 1 mm cube), none without a
  bound;
- inner shells: a body inside a body removed; a cavity, a body floating in a cavity and crossing
  bodies kept; of two nested bodies, only the inner ones removed;
- visibility: an open box facing inward turned outward, one facing outward left, a lone sheet
  undecided.
- **Real models:** the aggressive preset turns no part of the Yorick, `Base_01.stl` or the open
  fixtures and removes no shell from them.
- **CLI:** `visibility`, `small-shells`, `small-volume`, `inner-shells` and their refusals.
- **Comparison suite:** byte-identical to the c4 baseline.

M6.4d r5 (TD-092) adds `hole_refinement` in the core:
- a wavy, non-planar hole of more than 96 edges in a sphere: capped flat without new vertices, and
  refined (over 100 new vertices, cap edges near the surrounding ones), both closed, the refined
  one without self-intersections, inside the hole's span, and repeated exactly;
- a 520-edge hole refused as too large without refinement, closed with it;
- a bow-tie hole through a pinch vertex refused as not simple, and capped as its two loops with
  `split_rims`, the pinch resolved;
- a refined cap passing through a small box is replaced by the flat unrefined cap.
- **Real model:** holes cut into the Yorick (radius 1.5 to 4.5 mm around its top, up to 108 rim
  edges) close with every preset without adding self-intersections; at 6 mm, six interlocked
  rims are partly refused by every preset. Measured in the TD-092 r5 record.
- **CLI:** `split-rims`, `refine-holes`, `unbounded-holes` need `fill-holes`.
- **Comparison suite:** byte-identical to the c4 baseline.

M6.4d r4 (TD-092) adds `non_manifold_repair` in the core:
- FX-10B's flap removed and the cube closed; a two-face sheet hanging from a cube edge removed
  whole; on an open box the sheet goes and the box and its hole stay; a sheet larger than the box
  is not taken for a fin and the edge is reported; two cubes sharing an edge are left alone; the
  aggressive mode cuts the faces of the edge left unbalanced.
- **Project:** FX-10B reports fins and needs repair, handled by the standard preset; a doubled
  face is not taken for a fin.
- **CLI:** `aggressive`, `fins`, `cut-non-manifold` and their refusals; `inspect --diagnosis`
  shows the fin removed and FX-10B ready after the standard preset. FX-10B slices and hollows
  with `--repair standard`.
- **Comparison suite:** byte-identical to the c4 baseline.

M6.4d r3 (TD-092) adds:
- **Hollowing (H1):** a box with a zero-area face at a T-junction is hollowed without repair, the
  crack closed and the face left out, both reported.
- **Hollowing (H2):** overlapping boxes give three voids, one inside both boxes; the shell's
  generalized winding number (signed solid angles) is 0 in every void and 1 or 2 in the walls.
- **Drainage (S1):** a hole from the top drains the void inside both boxes; the two others stay
  enclosed.
- **Project:** a crack with its zero-area face is ready with notes (hollowing no longer needs it
  repaired).
- **Comparison suite:** byte-identical to the c4 baseline; the Yorick hollowed at 1.5 mm gives the
  same bytes before and after, in the same time (1.6 s).

M6.4d r2 (TD-092) adds `tolerant_repair` in the core:
- **Welding property:** soups of triangles whose corners scatter around shared centres, closer
  and farther than the tolerance, for four seeds: no corner moves farther than the tolerance,
  corners welded together were all within it of each other, and the result repeats exactly.
- **Welding cases:** a corner split 0.5 µm apart closes at 1 µm and stays open at 0.1 µm; equal
  positions merge exactly; a sliver whose corners merge is removed with its area reported; two
  closed cubes 0.5 µm apart are left alone.
- **Degenerate faces:** a face with a repeated vertex removed without a tolerance; a 0.1 µm edge
  collapsed back into the cube; a flat face flipped with its neighbour, no vertex moved.
- **Cracks:** a sliver hole capped exactly, zipped within 1 µm with its offset reported, and
  capped again under a smaller tolerance; a crack in a knot of touching faces (a tetrahedron
  touching the cube along the split's new edge) closed into one balanced non-manifold edge.
- **Settings:** tolerances out of range or a crack tolerance without hole filling refused; plan
  records grow by one marker and value per tolerance set.
- **CLI:** `--repair standard` and the `weld`, `degenerate` and `cracks` items with their
  refusals; `inspect --diagnosis` previews both presets.
- **Comparison suite:** byte-identical to the c4 baseline.

M6.4d r1 (TD-092) adds:
- **Core:** the extended diagnosis (needles and caps told apart, a crack with its cap, pinch
  vertices, small components with bounded positions); self-intersections (a clean box and boxes
  touching at a corner have none, crossing boxes have them where they cross, the bound stops the
  search, faces without a normal are left to the defect report); the presets as fixed specs, and
  every change of the exact preset located (duplicates, an outward turn, a filled and a refused
  hole, the largest cap).
- **Project:** the printability state against what slicing and hollowing refuse (a hole with and
  without layer closure, an inside-out box, touching corners); a crack with its cap needs repair
  for hollowing and is ready after the exact repair; an open object hollowed only with its own
  exact repair, the timeline order and the refusal of a second or misplaced repair.
- **CLI:** `--repair off|exact`, and `inspect --diagnosis` appending its section after the
  unchanged report.
- **Comparison suite:** byte-identical to the c4 baseline.

TD-089 c6 (M6.4c) records:
- **Native Windows:** a complete run of the snapshot runner with the application (`-IncludeApp`),
  after a first run exposed the OpenGL fallback's lack of fragment storage buffers; the owner's
  Yorick scenarios checked by screen capture.
- **Cross-platform:** the Yorick hollowed with an axis hole and a tilted surface hole slices to
  the same bytes, and the same summary, on Linux and Windows.
- **Shaders:** the naga validation test keeps checking the viewport's shaders; it does not
  model device limits, so new GPU resources need a native OpenGL smoke run.

TD-089 c5 (M6.4c) adds:
- **Core:** a surface hole's tilt in plate documents, and its refusals.
- **CLI:** `--hole-depth`, `--hole-from cavity` and `--hole-tilt` shaping a surface hole, and
  every misuse of them refused.
- **Comparison suite:** [`tools/cli-comparison`](../tools/cli-comparison/README.md), recreated in
  the repository; c5 byte-identical to c4 (507 files). A benchmark before and after M6.4c is
  recorded in TD-089.

TD-089 c4 (M6.4c) adds:
- **Core:** tilts carried between frames and inverted; overlapping bodies counted as material in
  hole findings.
- **Project:** a tilted model-frame hole leaning the same way on the turned object; previews
  probing a solid object on request; an object out of the build volume hollowed in the preview
  and refused by slicing.
- **App:** new holes waiting to be applied or discarded, and shaped and moved while new; applied
  holes dragged back when refused;
  holes from the cavity where the section shows it; holes on a solid object; a world tilt kept in
  the model frame; the hollowing draft; the viewport's shaders validated with naga.
- **Native Windows:** the hollowing applied, a hole applied and shown as an opening, the section
  cap opened by it, and a hole from the cavity, checked by screen capture.

TD-089 c3 (M6.4c) adds:
- **Project:** operation identities and order, suspension, in-place hollowing, an object hollowed
  twice refused, model-frame holes following moved and mirrored objects, cavities shared by
  previews and slices with byte-identical outputs, and removed hollowing giving the
  never-hollowed output.
- **Core:** plate document version 2 and its refusals.
- **App:** holes stored in the model frame follow their object.

TD-089 c2 (M6.4c) adds:
- **Core:** hole resolution on the outer and cavity surfaces, findings for holes that drain, stop
  in the wall, reach the opposite wall or pass through, solid models, automatic depth, height
  indexing, the hole bound, and union removal of overlapping raster rings.
- **CLI:** holes without hollowing, overlapping holes, a through hole, and the 1024-hole bound.
- **Comparison suite:** print files unchanged; summaries and the formerly refused c18 recorded in
  TD-089; a 1024-hole measurement.

TD-089 c1 (M6.4c) adds:
- **Core:** model-frame shells placed exactly (vertices, faces, distances, plan record), bit-identical
  exteriors, mirrored and scaled placements, refusals of a second placement and of a zero scale,
  and draining an existing hollow as hollow-and-drain does.
- **Project:** moved and mirrored copies of one hollowed model.
- **CLI:** a mirrored slab hollowed.
- **Comparison suite:** regenerated outputs recorded in TD-089; non-hollowed cases byte-identical.

TD-088 (M6.4b4) adds: the plate border's width and place, the tab beyond it, and the grid split
from the edges; the fade from below checked on native Windows by screen capture.

TD-087 (M6.4b4) adds: the front tab and its word placed in the tab; layer masks turned to the
plate for each mirror and the axis swap.

TD-086 (M6.4b3) adds: layer snapping, layer numbers and the planes that cut, in unit tests; the
capped wall and the ghost holes checked on native Windows by screen capture.

TD-085 (M6.4b2) adds: the probe's frames and refused shapes; selection by the hole's solid;
solid drags returning or moving; the end, rim, tilt and body grips.

TD-084 (M6.4b2) adds:
- **Core:** tilted and fixed-depth surface holes; the bounded tilt; records unchanged without the
  options; the probe matching drainage, and refusing an edge hole and an overlap.
- **Project:** a refused hole keeps the cavity and the probe for display, while slicing refuses.
- **App:** the placing, refusing, dragging, undo, Delete and Escape flow on a hollowed cube.
- **CLI byte identity:** 479 and 28 files unchanged after the normal search moved to the index.

TD-083 (M6.4b) adds:
- drain holes follow a moved and turned object onto the same face, do not move when another
  object is removed, and come back with one undo;
- one undo step per hollowing value and per hole;
- the font draws every symbol written in the interface code, not only the catalogs.

TD-081 and TD-082 (M6.4a) add:
- **History:** undo and redo as new revisions, stale results after an undo, cleared redo steps,
  merged steps, a dropped no-op step, the history limit.
- **Preparation:** one result per object, support errors kept as findings, a hollowed and
  drained block certified, a lifted block refused alone with its remedy, slicing still refusing
  the plate; the background job and its cancellation.
- **App:** preparation after the quiet pause and hidden after an edit; the world normal of a hit,
  including under mirroring; preview meshes placed from their centre.

TD-080 (M6.3b) adds localisation tests:
- every `tr!` identifier exists in the reference catalog with exactly its message's arguments, and
  every message is used;
- every catalog parses, is bundled, and uses only reference messages and variables;
- plurals, fallback, folder overrides, the decimal separator and the pseudo-locale;
- no interface call takes an English literal (a single-line heuristic);
- the interface font has a glyph for every catalog and pseudo character.

TD-079 adds:
- **Core:** the bounded positive rule tolerates exactly its sample count and fails one below it;
  outward sources keep the positive masks; negative-winding errors name the layer and first
  pixel.
- **Pipeline:** an inside-out box beside a box is refused with both remedies, fails above a
  tolerance, and within it slices to the plain box's volume with the summary line.
- **CLI:** `--reversed-tolerance` parsing and bounds.
- **App:** the fix that enables the tolerance.

TD-078 (M6.3 refinement) adds:
- **Pipeline errors:** an inside-out body, a lifted model and a model beside the plate are
  refused with their remedies and a reason free of command-line options; the remedy for the
  inside-out body slices it.
- **Diagnosis:** a clean box has no findings; an inside-out box, a hole, a flipped face and a
  duplicate face are found; a cavity is not a finding; handling follows the plate's settings.
- **App:** failed-slice wording and fixes, the settings each fix changes, and the message for an
  opened model.
- **CLI byte identity:** the 20-case suite (479 files) and nine cases of refusals and repairs
  (28 files) match the CLI built before the change.

TD-077 (M6.3) adds:
- **Presets:** every file under `assets/presets` is compiled in, valid and unique, every resin
  calibrates a preset printer, and every preset printer slices a small box in each of its
  formats.
- **User library:** save, revise, stale revisions refused, preset identities protected, import,
  free identities, and presets only without a user folder.
- **Jobs:** the exported masks are kept in order, with the expected coverage.
- **App:** the mask reduction keeps thin features; export publishing is atomic; durations format.

TD-076 adds tests of colours with alpha: the linear conversion keeps alpha, encoded channels,
egui's unpremultiplied input, and the palette stays opaque. The contrast check composites a
translucent foreground over its background.

TD-075 adds tests of the `--window` parsing and of the centring arithmetic. Window positions
are checked on native Windows with `GetWindowRect` (recorded in TD-075).

TD-074 adds `karasu-app` tests for:
- the default model colour and the sRGB-to-linear conversion;
- the move handles along X, Y and Z and the free-move square;
- axis, free and Z moves;
- per-axis and uniform scaling along the object's turned axes;
- settling a lifted object onto the plate, floating detection, dropping, and the point
  a ray hits; the free-move square hit anywhere inside.

TD-073 (M6.2) adds:
- **`karasu-app` headless tests:**
  - the camera: projection, rays through pixels, orbit, pan, zoom and fitting;
  - ray tests against boxes, triangles and placed meshes, and affine inverses;
  - drags along axes, on planes and by angle;
  - the Euler composition of world-axis turns;
  - the gizmo handles and their drags (move, turn with snapping, scale);
  - the workspace: opening a model, its notice and placement, drawing, picking and the anchor.
- **The viewport smoke test** with a tracked model (`app-smoke-model` in native validation):
  import, rendering and a measured orbit.
- FX-12 frame times are measured on native Windows with the local fixture, and recorded in
  TD-073.

TD-072 (M6.1c) adds:
- **Core:** pose bindings and their refusals; an exact quarter-turn motion of tips, bends,
  bases and axes, with heights, braces and parameters unchanged. The moved document builds on
  the moved model with the same parts and mesh size.
- **Project:** a plate object moved and turned a quarter keeps its supports, layer count and
  volume (within one percent of pixel sampling); a tilt, a new scale and a new height are
  refused with the reason.
- **CLI:** a pose-bound document follows `--translate`; a translation-only binding is still
  refused.

TD-071 (M6.1b) adds `karasu-project` tests for:
- **The library:** each model stored once, identified by SHA-256; models in use kept.
- **Revisions:** every edit advances the revision, older results become stale, removed plates
  are reported, identities are never reused.
- **Plate requests:** embedded data, and the refusals (empty plate, missing model, invalid
  settings, invalid object named by position and name).
- **Embedded equals files:** a hollowed plate of two copies sliced from embedded data gives the
  same output file, byte for byte, as the same plate given as a plate document and files. The
  summaries differ only in the lines naming the plate, the model and the output.
- **Background jobs:** progress events from the first phase to the last layer; the result of a
  plate edited while its job ran is stale; a job cancelled while importing publishes nothing.
- **CLI byte identity:** the comparison suite still matches on all 479 files.

TD-066 adds:
- **Simplification tests:**
  - flat-wall reduction with the wall floor;
  - curvature against tolerance;
  - thread-count identity;
  - plan and tolerance refusals.
- **Oblique cylinder unit tests:** closure, orientation, volume, error budget, determinism.
- **Oblique prism section tests.**
- **Sampled drainage tests:**
  - a 45° hole with its analytic plug volume;
  - surface holes on a cube and a torus;
  - the refusal of a cavity that is too thin;
  - the updated refusal of a tilted entry cap.
- **A CLI test** of `--hollow-simplify`, `--surface-drain-hole` and vector `--drain-hole` axes.

The 16 cross-platform cases and the TD-064/TD-065 cases are compared byte for byte with the
previous build.

TD-065 adds:
- `plate` unit tests: document defaults and refusals, positions from the middle outwards and
  the shelves' fallback, spacing, obstacles kept clear, overflow, bitwise repetition and overlap
  pairs; an ignored test times 50 to 400 footprints;
- CLI tests: arranged copies (translations, volume, overflow, option conflicts) and a plate of
  fixed, arranged and overlapping objects (union volume, overlap report, refusals).
Plates of real models (supports, drained copies, an adaptive layer plan) are decoded by the
format validators. The 16 cross-platform cases are compared byte for byte with the previous
build.

TD-064 adds:
- `layer_plan` unit tests: fixed ranges with carry boundaries, range and table exposures, adaptive
  slope bounds (cone, vertical column, horizontal faces), growth limits, document errors, skipped
  ranges and missing exposures;
- slab schedule tests: exact integer stacking over 30 000 layers, refusals and caps;
- print-job tests: slab positions and thicknesses, required overrides, calibrated bottom layers
  and printer limits;
- an SL1 encoder refusal test and a CLI test of `--layer-plan` (records, GOO output, refusals).
FX-11 fixed and adaptive plans are decoded by the format validators, with per-layer thicknesses
checked against the plan. The 16 cross-platform cases are compared byte for byte with the
previous build.

TD-063 adds:
- codec unit tests:
  - CTB v3–v5 records and sections;
  - Anycubic sections, records and bit-planes across six revisions, and capability refusals;
  - SL1 members, PNG content, `config.ini` and fade;
- independent Python validators for CTB v3–v5, Anycubic and SL1, with hand-built fixtures and
  corruption tests;
- CLI tests: every new format publishes a complete file with previews; SL1 fade refusal.
Real FX-11 files in every new format are decoded by the validators and compared with the CTB v2
masks. The 16 cross-platform cases are compared byte for byte with the previous build.

TD-062 adds:
- support primitive unit tests: closure, orientation, volume convergence, frames, determinism;
- index unit tests: nearest triangle, exact crossings, sound and tight clearance proofs;
- `tests/manual_supports.rs`:
  - plate, model, support and interior bases, braces, raft, bends and sphere contacts;
  - anchor, axis, piercing, collision, drain-hole and volume findings;
  - tessellation independence;
  - document resolution and schema errors;
- CLI tests: lifted models, `--lift`, collision refusal and acceptance, off-surface contacts, usage
  errors and an interior pillar.
The 16 cross-platform cases without supports are compared byte for byte with the previous build.

TD-061 adds:
- degree trigonometry and Euler rotation unit tests (exact right angles, agreement with the
  radian convention);
- CLI placement tests: computed versus explicit translation byte for byte, mirror and half-turn
  identities, volumes under rotation and scale, `--drop` after a tilt, usage errors;
- a parallel-stream test of the bottom raster configuration against sequential references;
- CLI compensation tests with exact volumes and recorded per-layer compensation;
- resin usage unit and CLI tests.

TD-060 adds:
- the quantizer against an exhaustive nearest-level oracle for every sample count, level count and
  a range of minimum grays, plus monotonicity and bound checks;
- randomized span-versus-dense raster tests with random minimum grays;
- CLI tests: `--min-gray` parsing and its `--antialias` requirement, brighter edges with the same
  lit pixels, full pixels at 255, and the recorded value;
- reference-validator vectors for the minimum gray.
The 16 cross-platform CLI cases without the option are compared byte for byte with the previous
build.

TD-059 adds:
- renderer unit tests: shaded cube, determinism, translation independence, size errors;
- codec tests: exact GOO RGB565 and independently decoded CTB/Photon RLE15 previews, unchanged
  layers, size checks before output, PP1 RGB PNGs and the gray fallback;
- a CLI test: shaded previews in every machine format;
- validator vectors for preview decoding and malformed runs.
The cross-platform CLI outputs are compared with the previous build: layer data identical, only
preview bytes changed.

TD-058 adds:
- a welding test against a sequential first-occurrence map, including signed zeros and the vertex
  limit;
- a parallel smallest-index test;
- a multi-megabyte OBJ grid parsed in parallel chunks: expected triangles, and an error deep in
  the file named at its exact line.
The cross-platform CLI outputs are compared byte for byte with the previous build.

TD-057 adds:
- ZIP reader unit tests;
- `tests/model_formats.rs`: the registry, units, cross-format mesh identity, OBJ syntax,
  triangulation and errors, 3MF transforms, object types, extensions, parts and invalid packages;
- a CLI test slicing one box from STL, OBJ (mm, cm) and 3MF (mm, inch) to byte-identical files.

TD-056 adds `parallel` unit tests:
- the parallel sort against `sort_unstable_by_key`, for sizes around the threading threshold,
  few and many distinct keys, and random, sorted and reversed input;
- co-rank tie handling;
- the index-exact parallel fill.

The cross-platform CLI cases and their defect and repair summaries are compared byte for byte
with the previous build.

TD-055 (span masks) keeps the previous dense GOO, CTB, Photon and PW0 encoders as test-only
oracles:
- the run encoders must match them byte for byte on 300 random multi-row masks each, including
  value mapping;
- `raster::mask` tests check the dense round trip, the canonical form, pixel lookup and runs
  crossing rows;
- the dense rasterization reference and the PP1 flood-fill comparisons run through the masks;
- codec cancellation is tested per run.

Every CLI and `print_reference` output is compared byte for byte with the build before TD-055.

TD-054 phase 3 adds `tests/hole_filling.rs`:
- missing box faces capped into the closed box, with exact volume and idempotence;
- a 48-gon hole capped flat by 46 triangles without new vertices;
- `TooLarge`, `OpenChain` (FX-10B) and `Intersects` refusals;
- zero-area cracks at a T-junction closed by splitting, with and without the zero-area face, and
  a crack off its line capped instead (TD-091);
- FX-10A hollowed through the plan's repair, and a misplaced repair rejected.
`tests/sampled_hollowing.rs` checks that touching bodies are hollowed with full walls and that
overlapping and inside-out bodies are refused. CLI tests cover `fill-holes` parsing, an open box
filled byte-identical to the closed box, the hollowing hint without `fill-holes`, and an open box
hollowed identically to the closed box.

TD-054 phase 2 adds `tests/mesh_repair.rs`:
- untouched clean meshes;
- FX-09 restored by exactly one flip, and the FX-08 duplicate dropped;
- duplicate groups collapsing to their net orientation, and glued boxes cancelling their double
  wall;
- 200 random minority flips undone exactly;
- `outward` turning an inside-out box and keeping the FX-04 cavity.
`tests/large_models.rs` checks that FX-12 repairs to one closed manifold. CLI tests cover
`--repair` parsing, a reversed face repaired to the clean box byte for byte, refusal without
repairs, and the inside-out hint and `outward` repair.

TD-054 phase 1 adds `tests/open_meshes.rs`:
- diagnosis values on the defect corpus;
- defect edge counts against the full topology report on random meshes;
- star prisms missing side faces, which close to the complete prism byte for byte (1 and 16
  samples per pixel);
- a U-shaped hole crossed four times, choosing the shorter matching;
- the gap-bound failure with its position;
- a non-orientable Möbius strip.
CLI tests cover `--close-holes` parsing, the open-top box, two missing walls (refused at 2 mm,
identical to the complete box at 12 mm) and `--no-close-holes` refusal.

TD-053 adds large-model tests:
- duplicate detection against the previous coordinate-hash algorithm on random meshes with
  repeated positions;
- the lean edge summary against the full topology report on every small corpus fixture and 300
  random meshes;
- `tests/large_models.rs` (ignored by default; run with `--release -- --ignored`): FX-12 identity,
  default-limit import, edge summary and ten union layers. It skips when the local file is absent.
Corpus scans exclude the local-only FX-11 and FX-12 files. A non-ignored test must never read
them from disk: FX-11 is generated in memory from the shared fixture generator, and FX-12 tests
are `#[ignore]`d. A test that relied on a local FX-11 failed on native Windows
([validation](CLI-ENGINE-NATIVE-WINDOWS-VALIDATION.md)).

TD-052 adds antialiasing tests:
- export: the GOO factor, CTB flag and PP1 `anti_count` for binary and supersampled masks; a mask
  whose supersampling differs from the declaration and an unsupported level both fail without
  publication;
- CLI: `--antialias` parsing, supersampled reference/GOO/CTB/PP1 output with the exact box
  volume and the recorded level, and refusal for legacy Photon and unsupported levels.
Grey masks are also checked with the independent `tools/m5-validate` readers against reference
masks (GOO exact, CTB with its 7-bit rounding).

TD-051 adds `pp1_scene` unit tests: run-based component labelling against an independent flood fill
on 600 random masks (widths 1 to 67, three zero bins) and word predicates against byte checks. The
existing PP1 parallel-encoding export tests prove byte-identical files.

TD-050 winding-union layers add `tests/winding_layers.rs`:
- U1: masks identical to strict under every rule on the closed corpus and FX-11 (1 and 16 samples);
- U2/U3/U6: overlapping, touching, kitbash, self-intersecting, nested-cavity, hollow and random
  bodies against an independent oracle, the generalized winding number (solid angles) at each
  pixel centre, skipping only numerically ambiguous pixels;
- U4: open and flipped surfaces fail with the offending edges; U5: planes on vertex heights;
- inside-out sources, mirrored panels and several thread counts.
Unit tests cover segment orientation, reflecting and singular transforms, balance failures and the
rules; `render.rs` tests compare the span writer with a dense per-pixel reference. CLI tests check
`--fill`, union admission, merged-overlap reporting and identical union/strict files.

TD-050 adds `tests/raster_parallel.rs`:
- images identical to the sequential engine and raster streams for 1, 2, 3 and 8 threads, a
  one-layer window, and cuts;
- the same first failure in layer order as the sequential stream;
- cancellation and early consumer stops that end without later layers or hanging workers.

TD-049 adds regressions for the audit fixes: a full P1 panel under default raster limits, CTB
header lengths for a swapped panel, the complete placement hint applied successfully, copy-fallback
publication with a refused hard link (exclusive creation, no replacement, cleanup), a 20,000-layer
PP1 admission and a twisted support loft above the plane cap.

TD-048 adds an engine regression (a plane exactly on a step face, upright and upside down, keeps
the material just below, preserves the scheduled identity and moves only that plane), a unit test of
the plane choice (gaps, slab bottom, adjacent binary64 heights) and a CLI volume test of a step on a
layer midpoint against an off-plane control.

TD-047 adds the following:
- `tests/sampled_drainage.rs`:
  - rasterized drained-cube volume against the analytic value; non-convex torus;
  - region accounting with two bodies and with a preserved island;
  - a sealed-void entry refused;
  - every opening failure category, including `Overshoot` and end-cap centres exactly on a
    source face of any orientation (horizontal, vertical, an edge);
  - overlap, `Cut` and cancellation errors; out-of-contract limits and a plan without a hollow
    operation; progress per hole;
  - a column of twelve stacked preserved voids owned by one region;
  - certified clearances down to subnormal gaps, and a radially bounded post beside the cap;
  - the review counterexamples: lid-sealed pocket refused (`SourceNotEmbedded`) while a hole
    into the same cup from below escapes and is certified, tiny far-off pocket classified, X hole tangent to layer
    planes and entered from outside the panel sliced correctly.
- `tests/sampled_hollowing.rs`: a rotated cube with a zero-area split face keeps its wall (four
  rotations that dented or broke the cavity with a normal-projection face case).
- `tests/raster.rs`: a cut ring with repeated vertices is refused by the payload preflight at its
  reserved capacity.
- Unit tests:
  - exact segment/triangle, triangle/triangle and improper-mesh-contact predicates; the exact
    plane-side and coplanar shortcuts against the general rules on random grid pairs; collinear
    faces sharing an edge;
  - odd-parity label lists and subtree masks; disk gaps along and across the axis;
  - the conservative segment slab filter against an exact oracle; certified cap lower bounds,
    including dense walls crossing the cap plane; box distances in the subnormal range; exact
    BVH node reservations and cancellable construction;
  - the fixed-width `orient3d` path and the 128-bit square comparison against the general
    integers;
  - closed-form distances;
  - certified separation bounds on thin-triangle and near-parallel cases; `Within`/`Beyond`
    decided from the certified bound only; self-pair search cancellation;
  - analytic prism sections;
  - span subtraction against a boolean oracle;
  - per-sample difference render at 1 and 4 samples per axis.
- CLI tests: `--drain-hole` volume, report, positional refusal and cylinder messages, the
  resolution check, the panel-edge tangent hole, the 32-hole cap and argument validation.

### Property-based tests

Generate constrained valid and invalid geometry. Candidate properties include transform round trips,
classification invariance under translation/rotation, area conservation within a derived tolerance,
ring normalization idempotence, deterministic permutation handling, and raster bounds safety. Store
minimal reproductions of failures as regression fixtures.

### Geometric regression tests

Version small, reviewable fixtures and expected structural results: layer counts, plane heights,
region/component counts, ring winding, areas, bounds, hashes, diagnostics, and selected mask images.
Prefer semantic assertions over fragile floating-point dumps. Any golden-file update requires an
explanation of the intended behavioral change.

### Numerical edge-case tests

Cover zero and near-zero area triangles, vertices/edges/faces on a slice plane, nearly parallel
intersections, large and small coordinate magnitudes, non-finite values, quantization boundaries,
coincident segments, repeated vertices, and tolerance transitions on both sides of the boundary.

### Fuzz testing

Fuzz file parsers, topology construction, triangle-plane classification, contour assembly, polygon
operations, rasterization, and format readers. Assertions include no undefined behavior, panics,
unbounded allocations, hangs, out-of-bounds access, or acceptance of structurally invalid output.
Seed corpora must contain both valid specimens and minimized historical failures. Fuzz tooling will
be selected only when the relevant parser exists.

### Benchmarks and profiling

Benchmark stage throughput and peak memory separately using fixed data sets and recorded hardware/
toolchain metadata. Track median and tail latency after warm-up. Performance changes must first pass
correctness tests. Use profilers to locate measured bottlenecks; do not encode timing thresholds in
ordinary unit tests. Establish explicit budgets when representative M2–M5 workloads exist.

### Cross-platform verification

Run formatting, linting, unit/integration tests, and builds on stable Rust under native Linux and
native Windows. Add platform-specific runtime tests for paths and atomic output. WSL is a useful Linux
environment but is not native Windows evidence. Record target triples and exact Rust versions.

CI (`.github/workflows/ci.yml`, TD-149) runs these checks on every pull request into `main` or a
release branch: on Linux formatting, Clippy, tests, the build, the documentation builds, the
CLA script's tests, the packaging script's tests and the orientation check's tests (TD-153); on
Windows (MSVC, GitHub's runner) Clippy, tests and the build, both without debug information. Its Windows job is native test execution for each pull request; a release still
needs the complete native validation of [tools/windows-native](../tools/windows-native/README.md)
in both profiles and a run of the application on real hardware
([RELEASE-PROCESS.md](RELEASE-PROCESS.md#tags-and-releases)).

## Planned geometric corpus

Each fixture will include provenance, units, expected validity, important dimensions, generation
method, and a license/ownership record.

| Fixture | Primary purpose | Initial expected assertions |
| --- | --- | --- |
| Cube | Baseline topology and axis-aligned slices | Closed manifold; square regions; exact bounds |
| Cylinder | Curved tessellation and tangencies | Stable ring; bounded area error; top/bottom cases |
| Sphere | Changing cross-sections and poles | Symmetry; pole degeneracy policy; monotonic area ranges |
| Hollow box | Nested shells and holes | Outer ring plus correctly wound inner ring |
| Inclined solid | Non-axis-aligned intersections | Stable topology at vertex/edge events |
| Mesh with holes | Boundary diagnostics | Boundary loops reported; no silent closure |
| Non-manifold mesh | Topology diagnostics | Non-manifold entities identified deterministically |
| Degenerate triangles | Classification robustness | Degenerates reported or skipped by explicit policy |
| Self-intersecting geometry | Validity diagnostics | Intersections reported; printability not assumed |
| Multi-component solid | Disconnected regions | Components preserved and canonically ordered |
| High-triangle-count model | Scaling and memory | Correct result under established resource budgets |

M0 does not add model fixtures because no importer or geometric oracle exists yet. M1 defines the
controlled test models; M2 begins the corpus with independently generated, redistributable files.

## Oracles and comparison

Use multiple oracles where practical: analytic results for primitives, independently implemented
readers, invariants, high-precision calculations, public format specifications, and behavioral
comparison with reference applications. A commercial application's output is an observation, not an
automatic definition of correctness.

Comparisons must state whether they require exact equality, topological equality, bounded geometric
error, bounded pixel disagreement, or merely compatible file structure. Every tolerance has units
and a rationale.

## Reproducibility and failure artifacts

Record software revision, target, toolchain, configuration, fixture hash, seed, tolerance set, and
command. Keep randomized seeds reproducible. On failure, retain the smallest non-sensitive input and
sufficient metadata to replay it. Do not commit proprietary input or raw reference-product evidence
to production fixture directories.

## M0 quality gate

The expanded MVP adds the planned M5.5/M6/M7 gate below; existing implemented milestone evidence
and Rust/platform obligations in this document are unchanged.

The baseline gate, also required for subsequent implementation changes, is:

```console
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
```

These commands could not run during the initial M0 session because no Rust toolchain was available in
the inspected WSL environment. They remain mandatory before the foundation is accepted as compiled.

## M5.5 preparation and expanded M6/M7 validation

**M6.4h (TD-111):** the application runs PREP-S01, S04, S05, S06, S07 and S09's scenarios
through its interface's own state (`crates/karasu-app/src/scenarios.rs`), in every
`cargo test`. The scenarios export the printer's format, compare bytes where results must be the
same, and check that the source never changes. The matrix, the Linux hashes and the owner's
interactive checklist are in the [M6.4h record](M64H-SCENARIOS.md).

**Status: M5.5-A–E headless portions are implemented and accepted within the bounded domain;
M6 interactive and M7 release portions remain open.** See [closure](M55-CLOSURE-ACCEPTANCE.md)
for the executed scenario matrix and explicit limitations. The following ledger preserves the
expanded acceptance obligations originally defined before implementation. TD-020 and PREP-001–009 make hollowing, manual drainage holes and editable manual
3D supports mandatory MVP capabilities. M5.5-A must fix a supported input domain, representation/
algorithm evaluation, mm/coordinate conventions, geometric approximation/error budgets, deterministic
canonicalization, resource limits and exact failure expectations before implementation tests are frozen.
Numeric budgets must be derived independently; no vendor threshold, global epsilon or unreviewed
voxel/SDF/Boolean library is selected by this test plan.

### Analytic and automated scenario ledger

| Scenario | Requirement trace | Independent inputs and expected validation |
| --- | --- | --- |
| PREP-S01 — Source/history preservation | PREP-001, PREP-006 | Pinned solid STL/source buffers with explicit placement and ordered edits; compare source bytes/indices/bounds before/after hollow/cut/support, edit/remove/undo/recompute and failure. Same source/settings/history produce the declared deterministic derived revision; singular operations need no mathematical inverse. |
| PREP-S02 — Hollowing | PREP-002, PREP-005 | Analytic cube and sphere with known inner/outer dimensions, material/cavity volume and face/radial thickness. A 20 mm cube/2 mm wall has a 16 mm cavity and 3,904 mm³ material under the specified sharp-corner contract; existing owned FX-04 supplies an independent reference, not proof hollowing is implemented. A sphere uses inner radius R-t. Check reviewed approximation/corner/error budgets and boundary diagnostics; invalid/excessive thickness is not healthy geometry. |
| PREP-S03 — Drainage cuts | PREP-003, PREP-005 | Owned hollow shell with a manually oriented cut reaching the cavity; independent cut extent/opening and void-connectivity oracle. Include misses, shallow/failed cuts, insufficient ligament/clearance, neighboring cuts and multiple/enclosed cavities. Unknown connectivity remains explicit and geometric connection never implies physical drainage. |
| PREP-S04 — Manual supports | PREP-004, PREP-005 | Elevated analytic model with manually specified tip/contact, shaft and platform base dimensions/frames; verify generated geometry, intended contact area/depth, connection and slice occupancy. Add/edit/remove/translate/rotate with stale-contact, disconnected-base and unintended-penetration counterexamples. No automatic contact placement or island detection is required. |
| PREP-S05 — Combined material integration | PREP-002, PREP-003, PREP-004, PREP-005, PREP-009 | Hollow, drain and support the same owned model; derive analytic slice material/void occupancy including intentional model/support overlap. M3/M4/M5 preserve cavities/openings/supports without double exposure or parity-created holes. Independently read reference PNG/JSON and the selected machine format under existing M5 acceptance. |
| PREP-S06 — Invalid/limited geometry | PREP-001, PREP-002, PREP-003, PREP-004, PREP-005, PREP-009 | Open/non-manifold/self-intersecting inputs, thin walls, closed cavities, invalid cut/contact parameters, insufficient clearance and collapsed transforms. Assert supported-domain rejection/diagnostic/indeterminate policies, original-entity/operation references and no silent repair or partial valid-solid claim. M2 closed combinatorics alone is not the oracle. |
| PREP-S07 — Interactive revisions and replay | PREP-006, PREP-008 | Repeated headless replay plus native M6 cavity/hole/support editing and preview. Upstream edits invalidate derived results; stale background jobs cannot replace current state. If persistence exists, save/reopen/recompute pinned settings/history; reject missing/mismatched source and incompatible versions. Otherwise record that conditional case as not applicable, while in-memory replay remains required. |
| PREP-S08 — Bounded execution | PREP-001, PREP-007 | Exact/one-over count/byte limits, numeric range failure, malformed inputs and bounded stress cases; deterministic entry/mid/final cooperative cancellation and retry. Assert distinct errors, bounded diagnostics, no completed partial geometry/export and preserved source/last completed revision. Record indivisible phases and measured intermediate/output/peak-memory conditions; no implicit RSS/latency promise. |
| PREP-S09 — Native end-to-end release | PREP-008, PREP-009 | On native Linux and Windows, execute solid STL -> hollow -> holes -> slice/export; model -> manual supports -> slice/export; combined preparation; edit/recompute; conditional saved replay; reproducible geometry/slices; invalid geometry/closed cavity/clearance/support-connection failures. Record source/config/version hashes, diagnostics and independent outputs from the current prepared revision. |

These are acceptance obligations; executed counts and supported-domain scope are in the closure record. Preserve
all existing fixture hashes/generator provenance; add independently owned analytic inputs only in
separately authorized implementation work. For geometry with approximation, compare to independent
analytic dimensions/volume, signed boundary/thickness and connectivity expectations under reviewed
error budgets, not the production algorithm's own reconstruction as its sole oracle.

### Delivery and platform gates

M5.5-B/C/D need focused unit/integration/property/numerical tests for their operations, with source
preservation and typed diagnostic assertions. M5.5-E needs the combined headless M3 slicing, M4
raster and M5 export gate, deterministic repeated geometry/settings/diagnostics, malformed/stress
coverage and resource/cancellation evidence. Model/support combination must be independently validated
whether implemented as fused geometry or separate material components; no representation is chosen
here. Record operation/schema/algorithm version, named frames, approximation settings and explicit
seeds if relevant so results can be reproduced rather than compared only visually.

Run normal stable Rust formatting, strict Clippy, full regression/build and warnings-denied rustdoc
when implementations exist; preserve original M2/M3 contracts/tests. Native Windows and Linux runs
must validate the same hash-identified production source, actual inventories/counts and fixture integrity;
WSL is not Windows evidence. Cross-platform deterministic-output differences require diagnosis, not
silent tolerance widening. Existing geometry accuracy allowances are not automatic shell/cut budgets.
Stress/timing/peak-memory campaigns must identify setup, transient geometry/live revisions and
allocator overhead; no calendar or performance target is invented by this revision.

M6 adds actual interactive preparation, diagnostic overlays, responsive background progress/cancel,
revision-consistent prepared slice preview and conditional project replay tests. The core must pass
headlessly; GUI controls alone are insufficient. M7 is not feature-complete until M5.5-A–E and its
M6 integration pass PREP-S01–S09 alongside the original release, interoperability, performance,
security/dependency and native workflow gates. Physical drainage/venting, support strength and print
success require separately authorized representative trials and explicit printer/resin/process
conditions; geometry alone supports no universal physical-success claim.

## Implemented M3.1 local intersection coverage

The normative executable test matrix is Section 9 of
[M3-TRIANGLE-PLANE-CONTRACT.md](M3-TRIANGLE-PLANE-CONTRACT.md); Section 4 enumerates all 27 ordered
side triples and their strict-crossing/singleton/coplanar edge counts. Enumerate each triple under
all six corner permutations (162 baseline invocations), checking exact side/variant incidence,
canonical coordinate bits, and provenance after remapping local corner/edge IDs. Include distinct
collinear, repeated-position, vertical collapsed and coplanar rank-loss inputs; a raw event or a
computed cross observation must not certify positive area or exact rank.

Require exact equality for on-plane source copies, exactly representable analytic intersections,
source identities, assigned Z and normalized zero. For inexact XY use independent bounded
integer/rational or exact-dyadic oracles and the contract's derived coordinate-wise absolute bound,
including its subnormal allowance. Exercise adjacent representable Z, accepted subnormal
distances, extreme balanced crossings, required-distance overflow, parameter normal/subnormal
boundaries, XY underflow, rounded endpoint coincidence, typed validation precedence, immutable
inputs and repeat determinism. Do not reproduce the production expression as the expected value
or introduce a universal ULP/geometric epsilon.

Translation/scaling properties require lossless represented inputs, preserved side/difference
relations and the same accepted arithmetic domain. General floating-point translation does not
promise bitwise construction equivariance; test exact small dyadic grids or compare each result
to its own analytic error bound. Permutation invariance compares remapped provenance as a multiset
when tied coordinates make local-ID ordering differ. Signed-zero input bits must remain unchanged
even though output point payloads normalize zero.

The local primitive has no cancellation controller, allocation, progress, mesh traversal or layer
policy. No corresponding local APIs/tests are added by analogy with mesh-wide operations.

`crates/resin-core/tests/triangle_plane.rs` implements the literal 27-state C/P/E truth table and
all six permutations, checking full coordinate-bit arrays and remapped provenance multisets.
It also covers repeated/collinear/coplanar rank loss, geometric-before-local-ID ordering, source
observation range failure, all 1,024 zero-sign assignments to nine coordinates and the plane,
strict zero-plane crossings, adjacent floats, subnormal distances/XY rounding, normal-weight domain
boundaries, balanced opposite MAX inputs, deterministic geometric error precedence, invalid-input
validation order, immutable M2 meshes/transforms, and repeated bitwise output.

Independent integer line-plane equations check 2,304 bounded triangles (two endpoints each), with
exact `i128` dyadic decoding of actual outputs. Error comparisons use checked integer arithmetic and
the actual per-axis M, proving the stricter normal-term bound where applicable; an eta-scaled exact
midpoint check separately includes the underflow allowance without rounding it away. Another 847
triangles cover seven normal binades, with lossless power-of-two rescaling against the same rational
oracle. Exact dyadic translations/scales and inexact ordinary permutations are separate constrained
properties. An inexact collinear example explicitly demonstrates that computed endpoint coincidence
is not exact-real rank; it is bounded against the independent exact-zero oracle, not snapped closed.
These are production-code tests, not reuse of the earlier M3.0-C Python model's pass count.

Private unit tests exercise the finite coordinate guard (public coordinate overflow is unreachable
under supported arithmetic), normal-positive parameter domain, extreme scalar comparisons, and
geometric edge-key ordering/ties. One executable public API example checks the analytic crossing.
The normative mathematical contract remains unchanged. Full stable Linux/WSL format, strict Clippy,
workspace test/build and warnings-denied public/private rustdoc gates, focused Debug/Release tests,
fixture hashes and whitespace checks are required before handoff. Native Windows validation remains
separate from Linux and the historical M2 gate; its later successful M3.1 run is recorded below.

On stable Rust/Cargo 1.99.0, `x86_64-unknown-linux-gnu` under WSL, the completed gate passes
**257 workspace tests, 0 failed, 2 optional tests ignored**: the unchanged 224-test M2 baseline plus
28 new integration tests, four private unit tests, and one documentation test. All 28 focused tests
also pass in Release. Emitted Linux Release LLVM section arithmetic has separate floating operations
without fast-math/contraction flags or FMA intrinsics; Debug/Release subnormal tests exercise gradual
underflow. This is build-specific evidence, not a universal compiler/platform proof. All 12 available
manifest-listed fixtures (including existing optional FX-11) retain their hashes before/after; no
optional measurement, fuzzing, benchmark, fixture generation or native Windows run occurs. Raw logs,
exit statuses, arithmetic inspection and source hashes are external under
`<evidence>/m3/m3-1/20261016-66132b9-vO0HpD/`.

### Subsequent native Windows M3.1 validation

Committed HEAD `5f12ebcdf6b75366479ddf974555d6afa1feda24` passed native stable Rust/Cargo 1.99.0,
`x86_64-pc-windows-msvc`, all standard gates and warnings-denied rustdoc. Counts are 256 passed,
zero failed and two optional tests ignored (the Unix-only CLI path test explains WSL's extra pass).
All 28 focused tests pass unchanged in Debug/Release; all four private kernel units pass. Eleven
committed fixture hashes and all 90 archived source files remained unchanged; optional FX-11 was
absent from that export. Archive SHA-256 is
`dc060ef8f2aa83003092feff423abfd4fd332d78f1606ebc227d6091ae073f87`. Evidence is external at
`%LOCALAPPDATA%\Temp\resin-slicer-m3-1-windows-validation\20261016-030159-5f12ebc\evidence`.
Its 27-entry manifest hash is `a257a86a189002681e162d1facdac2e87b5864f0bf40566016e5a9bac382fd2d`;
manifest and all entries were reverified during M3.2-A. No M3.2-A Windows pass is inferred from that
older campaign; the later consolidated validation is recorded below.

## Implemented M3.2-A raw mesh-section coverage

The clean starting point is M3.1 HEAD `5f12ebcdf6b75366479ddf974555d6afa1feda24` on `main`.
`crates/resin-core/tests/mesh_section.rs` adds 39 deterministic integration tests; one private unit
checks count/multiplication/isize-capacity overflow without allocating, and one executable example
uses the public section API. Existing M2/M3.1 tests, normative contract, fixture generator, manifest
and canonical STL bytes are unchanged. Stable Rust/Cargo 1.99.0 on `x86_64-unknown-linux-gnu` under
WSL passes **298 tests, zero failed, two optional tests ignored** (257 baseline plus 41 additions).
All 39 focused integration tests also pass in Release. Formatting application/check, strict
all-target Clippy, workspace tests/build, warnings-denied public/private rustdoc and whitespace pass.

Coverage includes empty/unused-only and above/below inputs, the 27 side words with expected event
totals 2/6/12/6/1, exact analytic crossing/indexed origin resolution, local collapsed corner/edge
identity, tangent vertices/edges, degenerate and reversed/duplicate/disconnected coincident faces,
no-hit context totals, stable source-face order and reordered-face multiplicity. No ownership,
duplicate suppression, topology exclusion or implicit solid interpretation is accepted.

Independent exact cube oracles assert eight midplane half-sides, square support coordinates and
boundary-plane totals (two None, four tangent, four coplanar edges, two coplanar triangles). Cylinder
midplane tests assert 256 wall segments spanning ring-vertex XY to neighboring chord midpoints;
cap tests retain 128 None/tangent/edge/triangle events of each kind. A separately constructed inclined
triangular prism asserts six exact half-sides on its analytic triangular support. FX-05 endpoints
are checked with independent scalar line equations on the actual serialized vertices, using
`128 * EPSILON * 50` mm for bounded coordinates below 50 mm. Object Y rotation/nonuniform scale/
non-origin pivot uses separate scalar trig/line equations with `256 * EPSILON * 32` mm. These are
short-chain comparison budgets, not traversal epsilons or universal transform error guarantees.

Other checks cover identity modes, exact world translation, an exact rotation swapping source/world
Z, affine shear, reflected/singular maps, source-incidence preservation under new rank loss, direct
object evaluation when affine flattening fails, unused-vertex overflow exclusion, contextual transform
and kernel parameter/distance failures, signed-zero normalization, subnormal distances and adjacent
represented Z. All eleven small fixtures at three explicit planes also compare every raw retained
record against direct M3.1 calls as a supplementary assembly oracle, not the sole geometry oracle.

Resource tests cover zero/exact/one-below face/event/byte limits, non-power-of-two growth, very large
configured limits without speculative allocation, and failure after partial work with unchanged
source and successful retry. Cancellation tests use deterministic predicates only: entry before
invalid plane/limits, the exact current 0/256/512/768 no-hit face-poll sequence, cancellation on the
third face poll, before/after storage growth, empty/no-hit final publication, and ordinary error
precedence. Poll sequences are version-specific tests, not a stable progress contract or time bound.
Repeated outputs compare coordinate bits and source records; complete imported mesh/metadata/
diagnostics and transforms remain equal to independent snapshots. No allocator OOM injection,
new fuzzing, performance campaign or native Windows execution is claimed.

All 12 available canonical STL hashes and sizes match the manifest before/after, including existing
optional FX-11, which is hashed only and never regenerated or exercised as a measurement. Raw logs,
statuses, fixture checks, prior Windows evidence verification and implementation handoff are external:
`<evidence>/m3/m3-2a/20261016-5f12ebc-ww4o7r/`.
M3.2-A acceptance covers raw traversal only. At that implementation checkpoint, native validation and
M3.2-B ownership/normalization remained separate gates; later sections record their implementation
and the consolidated Windows campaign. Polygon/layer work and EXP-03/04 remain separate open gates.

## Implemented M3.2-B normalization and ownership coverage

Baseline clean `main` is committed M3.2-A `2f17dbe494318fb2d637559aa39457c561a487d8` (298/0/2).
`tests/section_normalization.rs` adds 31 deterministic integration tests; normalization/private exact
modules add six unit tests and one executable API example. Existing M2/M3.1/M3.2-A tests and the
normative contract stay unchanged. Stable Linux/WSL Rust/Cargo 1.99.0 passes **336/0/2** (298+38).
All 31 focused integration and all six private tests pass in both Debug and Release. Formatting,
strict all-target Clippy, build, warnings-denied public/private rustdoc and whitespace checks pass.

Analytic planar square expectations are four explicit exterior source-edge keys and one internal
diagonal with two retained opposed occurrences: six occurrences total, not a five-edge destructive
deduplication. Single triangles, same/opposite duplicate faces, inconsistent winding, overlapping
same-side third vertices, mixed coplanar/noncoplanar neighbors and three-face non-manifold edges
check explicit local classifications and issue bits. Pure noncoplanar on-plane edges stay unresolved
on either side configuration; tangents contribute points only. Exact predicate checks include 2,048
independent bounded-integer determinant cases, MAX/subnormal/zero/range extremes, cross-binade
products and a determinant of `EPSILON^2` hidden by rounded products, scaled across four binades.
A patch regression independently certifies a tiny nondegenerate triangle whose raw computed cross
is zero, without changing that raw observation or the M3.1 contract.

Interior FX-01 asserts eight source-edge feature nodes, eight ordinary segment candidates, two raw
uses at each node and square-support coordinates without extracting rings. Cylinder/FX-05 sections
retain expected 256/eight source-edge incidences. All eleven small canonical fixtures at three planes
check complete observation/segment partitions, raw contribution counts and imported mesh/metadata/
diagnostic immutability. Other cases cover disconnected coincident coordinates, one-ULP/small XY
separations without welding, source-edge reversal, crossing multiplicity, repeated indices, collinear
collapse, branch-like source-vertex neighborhoods, reflections, singular transforms, signed zero,
face-order permutation with provenance remapping and repeated coordinate-bit equality.

Private synthetic mutation of a locally owned raw test value exercises otherwise unreachable
inconsistent shared-edge observations: no representative is selected, both positions survive and
affected groups are unresolved. Separate private corrupt-provenance/nonfinite-coordinate tests
verify distinct fatal errors; these do not alter old tests or production raw construction.
Safety coverage includes empty input, exact/zero/one-below byte and count budgets, huge configured
caps, checked arithmetic/isize bounds, pre-cancellation, final cancellation, all 16 allocation-boundary
and six sort-boundary polls, deterministic event/high-incidence inner-loop cancellation, retry,
unchanged raw data, never-cancelled wrapper equality and error precedence. No sleeps, allocator OOM
injection, timing claims, new fuzzing, benchmark or commercial execution occurs.

All 12 available canonical hashes/sizes match before/after; optional FX-11 is hashed only, not
regenerated or run as a measurement. External evidence and actual statuses are under
`<evidence>/m3/m3-2b/20261016-2f17dbe-wP6Zwx/`.
M3.2-B PASS covers explicit safe local classification and retained unresolved outputs, not universal
filled-boundary inference, global overlap validity or commercial compatibility. EXP-04 and geometric
contour interpretation remain separate requirements. Combined native Windows M3.2-A/B validation,
pending at the normalization checkpoint, subsequently passed in the consolidated campaign below.

## Implemented M3.3-A contour-incidence graph coverage

Clean baseline is `main` at `be161f01f87533b262a681366c7c4317d304f8d2`, with the expanded MVP and
M3.2-B committed (336 passed, zero failed, two ignored). `tests/section_graph.rs` adds 45 independent
integration tests; `slicing::graph` adds four private unit tests and one executable public example.
Stable Linux/WSL quality gates pass 386/0/2; all 45 integration and four private tests pass in both
Debug and Release. No existing suite, canonical STL, generator, dependency or normative contract is
changed. Native Windows M3.2-A/B and M3.3-A validation was pending at that implementation checkpoint;
the subsequent consolidated campaign below supplies the scoped native results.

Analytic structures assert exact canonical keys, adjacency symmetry, unique-edge and eligible-end
degrees, multiplicity, occurrence partitions and raw source references: empty, one crossing, four-edge
coplanar square with retained internal diagonal, two squares, open ribbon chain, closed ribbon cycle,
disconnected cycles, degree three/seven branches, self-loops and duplicate/parallel/reversed face
contributions. Shared source-edge interior intersections join across reversed local direction; exact
On corners share Vertex identity, not fabricated edge-interior identities. Distinct coincident or
arbitrarily close features never weld; crossing XY interiors do not split or join.

Canonical cube interior asserts eight half-side candidates and degree two; its exact face/vertex/
edge plane preserves unresolved mixed boundary contributions and the internal diagonal. Cylinder
midplane asserts 256 candidates; FX-05 asserts eight at supplied Z=17. Owned open/non-manifold,
degenerate and reversed-face fixtures retain all raw/normalized/source data at three planes. Other
tests cover tangent-only isolation, coplanar faces, singular collapse, zero-length crossing, mixed
ownership, non-manifold edge/vertex contact and degree two with additional unresolved incidence.
Private synthetic raw mutation covers finite disagreements on shared Vertex and Edge features,
retaining both positions, no chosen point and no eligible connection. These tests do not alter raw
production APIs or the normative M3.1/M3.2-B rules.

Repeated builds assert deterministic full graph data. Face/winding permutations preserve canonical
graph structure while remapping original source faces/occurrences, including duplicate multisets.
Resource tests exercise exact/one-below count caps, aggregate requested payload, actual diagnostics,
zero/huge configured limits, checked usize/isize arithmetic and impossible-capacity typed reservation
failure (not injected physical OOM). Cancellation covers entry precedence, every nonempty allocation
boundary, both adjacency sort boundaries, record/diagnostic loops, one high-degree vertex's inner
adjacency scan after both sorts, empty/nonempty final publication and successful retry.

Record full fmt application/check, strict all-target Clippy, workspace test/build, warnings-denied
private/public rustdoc, focused Debug/Release and whitespace outputs with actual exit codes. Hash
all available manifest-listed fixtures without regenerating optional FX-11 or running measurements.
Evidence stays outside Git under `<evidence>/m3/m3-3a/`; this graph-only
acceptance does not close M3.3/M3 or claim paths/regions, physical validity, benchmark results or
commercial equivalence. Expanded M5.5 and M6/M7 preparation acceptance remains unchanged.

## Implemented M3.3-B conservative ordered path coverage

Clean baseline is `main` at `e696414616911bec7bd42a8b67b937ee7245efe6` (386 passed, zero failed,
two optional ignored). `tests/contour_paths.rs` adds 45 integration tests; seven private path units
and two test-only malformed-graph tests in the graph owner module add nine units, plus one executable
path API example. Graph production construction is unchanged. Full stable Linux/WSL validation passes
441/0/2; all 45 integration and nine new private tests pass in Debug and Release. Native validation
of M3.2-A/B and M3.3-A/B was pending at that implementation checkpoint; the subsequent campaign
below supplies the scoped native results without reusing an older production baseline.

Independent expected sequences assert the four-edge coplanar square [0,1,2,3] with edge IDs
[0,3,4,2], retaining the two-occurrence internal diagonal separately; two squares; linear ribbon
chains; T and degree-four branches with one trail per arm; branch-attached cycles and two cycles
sharing a junction; unresolved neighbors stopping otherwise degree-two continuity; and two uncertain
cycle vertices splitting two explicitly expected trails. Every candidate group has one path/offset
outcome and every complete-only group a retained internal/unresolved outcome. Tests verify endpoint/
edge alignment, distinct graph vertices, canonical keys, component membership totals and exact raw
occurrence conservation, not merely agreement with another production traversal.

Cube interior gives one eight-edge candidate, cylinder 256 edges and FX-05 eight at supplied Z=17.
Exact cube tangencies retain ten raw occurrences with no fabricated path. FX-10A is an open cube
missing its top: its horizontal midplane legitimately remains a cycle, while an explicit exact axis
rotation gives a six-edge open vertical section. This distinction is tested rather than assuming a
closed section implies a closed/printable source mesh. Other owned fixtures cover open/non-manifold,
degenerate/reversed/cavity/multiple-volume/sphere input. Coincident disconnected loops, independent
crossing chain interiors, repeated indices/self-loops, parallel/duplicate raw incidences, singular
collapse and a private finite shared-feature conflict retain their original distinctions and data.
XY bow-tie and zero-area/overlapping embeddings deliberately remain provisional graph candidates,
not certified polygons; no area/nesting/routing heuristic or repair is tested as implemented.

Permutation and repeated-output checks preserve canonical identities with remapped source-face/
occurrence provenance, including bounded wire families. Private canonicalization enumerates cyclic
rotations and both directions against one literal expected sequence. Safety tests cover all 16
nonempty allocation-boundary polls and both sort polls, input/component/high-degree/traversal/
canonicalization/diagnostic phases, long walks/cycles, empty/nonempty final cancellation, retry,
immutability, exact/one-below actual count and byte caps, huge/zero caps, checked arithmetic/isize
boundaries, inconsistent adjacency/order/indices and fabricated ownership reclassification. Impossible
reservation tests do not claim physical OOM injection. Graph-corruption tests mutate locally owned
private test values only; no arbitrary externally mutable graph API is introduced.

Record fmt application/check, strict all-target Clippy, full test/build, warnings-denied public/private
rustdoc, focused Debug/Release, fixture integrity and whitespace actual exits. All available canonical
hashes/sizes remain unchanged, including optional FX-11 hashed only, never regenerated or benchmarked.
Evidence is external under `<evidence>/m3/m3-3b/`. M3.4 geometric validity/
orientation/regions, EXP-03/04 and M1 stay open; no M3.3/M3 closure is implied. The then-pending native
platform gate is recorded in the subsequent campaign below.
Existing M2/M3 tests, normative contract and expanded MVP preparation scenarios are preserved.

### Subsequent consolidated native Windows M3.2/M3.3 validation

Committed `main` HEAD `c4865117e18d998fe80d48d8fac1022b5731ebc8` passed the scoped consolidated
native Windows campaign on 2026-10-16. This is separate from historical M2/M3.1 validation and the
Linux/WSL implementation campaigns above. The original WSL checkout remained clean and unchanged;
no source patch or dependency change was needed. Native environment: Windows NT 10.0.26200.0 x64,
Rust/Cargo 1.99.0, `stable-x86_64-pc-windows-msvc`, MSVC linker 14.44.35229.0 and SDK 10.0.26100.0.
Actual native compilation, linking, tests, CLI execution and rustdoc succeeded; WSL was not used to
compile or execute the Windows tests.

Machine-captured exits are 0 for fmt check, strict all-target Clippy, workspace test/build and
warnings-denied public/private rustdoc. Windows workspace results are **440 passed, 0 failed,
2 optional ignored**, versus Linux/WSL 441/0/2; the only count difference is the Unix-only
non-Unicode CLI path test. Focused integration counts per Debug/Release profile are `mesh_section`
39, `section_normalization` 31, `section_graph` 45 and `contour_paths` 45. Private filters also pass:
overflow 1, normalization/exact arithmetic 6, graph 6 and paths 9 per profile; the graph/path filters
overlap on the two graph-owner tests and are not additive counts.

**Evidence qualification:** The `mesh_section` Release success is owner-supplied interactive native
terminal output, not a machine-captured run. Its transcript records all 39 tests passing and the test
executable name; exit 0 is owner-reported in a separate status record, not independently printed by
the supplied shell transcript. Three earlier automated attempts retain actual exit 101 because
enterprise Code Integrity blocked the executable before tests started (OS error 4551; events
3033/3077). Both the original policy failures and the successful supplied transcript are retained;
no failed command is relabelled as machine-captured success. The archived report accepts this
qualified evidence as campaign PASS; no Windows-specific Rust assertion defect was observed.

Native CLI help/version and `inspect --assume-mm` FX-01/FX-10B checks pass. Missing-file exit 1 and
usage-error exit 2 are expected. All eleven committed canonical STL files match recorded hashes and
sizes. Optional FX-11 is absent from the archive, remains ignored, and was not regenerated or made
an acceptance condition.

Source archive SHA-256 is `5d2928659f17b5c98b91ff7a68827fec7f3e1a1589d0c5ee7e0a64dce970d3d4`.
The exact current committed archive was reverified during M3.4 entry research: all 99 archived file
payloads match committed Git blobs, the clean working tree and pre/post source hashes/sizes.
The normative M3.1 contract remains byte-identical, SHA-256
`6d06e0ed97336b766065ebfbee87a8ee8fce2b1f2551a7526f3d7385acfceb08`.

External evidence is retained at
`<evidence>/m3/m3-3/windows-validation/20261016-112044-c486511/`;
the Windows evidence directory is
`%LOCALAPPDATA%\Temp\resin-slicer-m3-3-windows-validation\20261016-112044-c486511\evidence`.
All 41 manifest entries reverify; manifest SHA-256 is
`ec0bf981e474d33bd1daa1778a89c489e4b5a0bd1b1f38e88f65eedcc62b31e9`.
Raw logs, screenshots/commercial outputs and observation records remain outside Git. Integrity
verification does not claim independent recapture of the owner terminal run. This documentation
reconciliation does not repeat the Windows campaign or run commercial experiments, fuzzing or
benchmarking; the current documentation-only Linux quality gates are recorded separately below. Campaign
PASS closes this scoped native gate only; it does not establish planar validity, region/material
semantics, layer scheduling, M1/EXP-03/04 completion, whole-M3.3/M3 acceptance or later MVP preparation.

## Historical M3.4 entry research/design checks — before implementation approval

The research record and
[independent design](M3-PLANAR-VALIDITY-DESIGN.md) specify planned contact/degeneracy/adjacency,
cross-path, extreme-coordinate, deterministic oracle/metamorphic, cancellation and resource tests.
Those are implementation acceptance requirements **proposed for review**, not tests added or run
against a nonexistent M3.4 API. The integer representation/scale gate remains open under unaccepted
proposed TD-023; M1 and M3 are not closed.

The external exact `Fraction` model passed 26,244 reversal/swap checks over 6,561 ordered segment
pairs, 24 exact dyadic transformations and separate numeric/area/grid counterexamples, exit 0.
The model compares orientation/interval decisions with independently solved rational line equations;
it does not verify all proposed diagnostic fields, Rust behavior, allocator/cancellation paths,
native Windows behavior, global commercial correctness or performance.

On stable Rust/Cargo 1.99.0, Linux/WSL x86-64, the unchanged Rust workspace passes **441/0/2**.
Formatting check, strict all-target Clippy, workspace tests/build, warnings-denied public/private
rustdoc and whitespace check each return 0. These regressions are separate from earlier native
Windows validation. No new native Windows, fuzzing, benchmark or commercial experiment was run.
Raw scripts, output, actual exit codes, source protection and evidence-integrity results remain in
the external M3.4 entry directory named in the research record, not in production source.

## Implemented M3.4-A planar embedding coverage

The owner approved TD-023's explicit detection-only staging on 2026-10-16 after research commit
`b1b58ba`. No integer conversion/construction gate is waived. Public APIs are
`slicing::validity::{inspect_planar_paths, inspect_planar_paths_with_cancellation}`; the
[design contract](M3-PLANAR-VALIDITY-DESIGN.md#10-implemented-m34-a-contract) defines units,
represented-coordinate exactness, endpoint roles, canonical findings, unassessed context and caps.

Twelve new `tests/planar_validity.rs` integration tests cover convex/concave/subdivided cycles,
zero/nonzero-area self-crossings and backtracking, open/branch trails, disjoint/touch/cross/coincident
simple cycles with separate cross-path findings, tangent/duplicate/internal complete-only context,
all input and pair cap exact/one-below boundaries, actual diagnostic and minimum byte budgets,
deterministic cancellation/retry/source preservation, inner pair polling and every observed
reservation boundary, empty/final/precedence cases, face-order/winding invariance and all eleven
small canonical fixtures. Optional FX-11 is not executed or regenerated.

Five private tests cover full contact taxonomy and directed endpoint roles, a structurally different
parametric i128 rational oracle over 6,561 small-grid pairs with 26,244 swap/reversal assertions,
MAX/subnormal/one-ULP separation, lossless dyadic maps and checked pair/isize capacity bounds.
The shared exact-predicate extraction preserves its three original tests and arithmetic unchanged.
One new executable API example covers the empty headless pipeline. These checks do not establish
ideal pre-rounding source geometry, region/material semantics, arbitrary-mesh validity or performance.
Actual allocator exhaustion is not forced; checked rejection and allocation checkpoints are tested.

Stable Rust/Cargo 1.99.0 Linux/WSL passes **459/0/2**, up from 441/0/2: 12 integration, five private,
one example. Fmt, strict all-target Clippy, workspace test/build, warnings-denied public/private
rustdoc and whitespace check return 0. Focused integration 12, private validity 5 and unchanged
exact-sign 3 also pass Release. No dependency, production fixture or prior mathematical contract
changes.

Native Windows stable Rust/Cargo 1.99.0 `x86_64-pc-windows-msvc` independently passes **458/0/2**;
the existing Unix-only CLI path test explains the difference. Final actual exits are 0 for fmt,
strict Clippy, workspace test/build, warnings-denied private/public rustdoc and focused Release
integration/private/exact tests (12/5/3). Real native compilation/linking and execution establish
functional MSVC/SDK support. The isolated native-filesystem copy is a hashed **uncommitted working
snapshot based on b1b58ba**, not a claim that the implementation is committed. All exported source
hashes reverify after execution; no build artifacts were copied from Linux.

**Preserved limitations:** An initial `.ps1` launch was rejected by execution policy (driver exit 1).
Direct native Cargo invocation required no policy change. The first workspace output records an
existing transform doctest blocked by application-control OS error 4551; its Cargo exit was not
captured because the wrapper waited on a lingering task-owned console. Log filename escaping and
process-handle capture bugs were confined to external validation tooling, not production Rust.
A second capture attempt returned driver exit 2 for unavailable native exit metadata, not Rust test
failure. Both attempts and raw output remain archived. The final corrected capture passes on
unchanged Rust sources; no failed/unknown original command is relabelled as success. This establishes
the final scoped native result, not immunity from future machine application-control restrictions.

Evidence is external at `<evidence>/m3/m3-4a-20261016-RsKEpD/`; the native
copy is `%LOCALAPPDATA%\Temp\resin-slicer-m3-4a-validation\20261016-RsKEpD\source`.
No sustained fuzz/performance campaign or commercial execution occurred. Linux is not substituted
for Windows evidence. M1/EXP-03/04 and remaining M3 geometric/construction/region gates stay open.

## Historical M3.4-B research checks — before implementation approval

The [contract](M3-AREA-ORIENTATION-CONTRACT.md#8-deterministic-acceptance-tests) originally specified
future independent rational area/sign, limb/carry/borrow, every-binade rounding and midpoint,
real-pipeline tiny/huge, eligibility/cross-contact/provenance, lossless metamorphic, count/byte,
structured consistency and atomic cancellation tests. This section records the design-only
checkpoint, not current Rust coverage. The owner subsequently approved TD-024 and M3.4-B.

Research-only exact-model checks exited 0: 1,008 coordinate decodings, eight width cases, 504
ordered triangle/reversal/cyclic checks, 19,925 binary64 rounding comparisons, 69,633 exhaustive
toy-format nearest-neighbor comparisons and eight lossless dyadic maps, plus boundary assertions.
An external Rust probe of the **existing** public pipeline confirms that coplanar triangles with
side 2^-600 and 2^600 mm each reach SimpleCycle without findings, exit 0. Their analytic areas are
2^-1201 and 2^1199 mm^2. No new area API, fixtures or production tests were added or exercised.
The full proof and model limitations are in the contract; finite samples do not prove future Rust.

Raw models/probe and logs remain at
`<evidence>/m3/m3-4b-contract-20261016-KO7tWw/`.
Stable Rust/Cargo 1.99.0 Linux/WSL regression passes **459/0/2**, unchanged from M3.4-A. Actual
exits are 0 for fmt, strict all-target Clippy, workspace test/build, warnings-denied private/public
rustdoc and git diff whitespace checks. All 12 existing fixture hashes match, including the optional
FX-11 file (hash only, not executed or regenerated); 97 tracked files outside the six edited
pre-existing documents retain their original hashes. Both new files are research/design documents.
Logs are external. Historical native M3.4-A results above do not validate proposed area code;
no Windows run was performed here.
The implementation/platform results below are separate from this historical checkpoint.

## Implemented M3.4-B area/orientation coverage

After owner approval of TD-024 and the independent contract, `slicing::area` implements immutable
SimpleCycle measurement with exact traversal sign and a single explicitly ranged nearest-even
area magnitude. Fourteen new `tests/section_area.rs` integration tests cover analytic triangle,
convex/concave/collinear-subdivided cycles; real tiny/huge coplanar and cancellation-sensitive
inputs; open/ambiguous/self-crossing/backtracking/collapsed skips; separate disjoint/touch/cross/
coincident/nested cycles; complete-only/isolated provenance; exact/one-below caps including skipped
metadata and rejection before reservation; every observed checkpoint/retry/source preservation;
inner long-cycle and global cross-cycle polling; empty/final/error precedence; lossless maps and
source-face reorder/winding; and all eleven small canonical fixtures.

Seven new private tests cover bit-convolution product oracles at every alignment residue/extremes,
carry/borrow/sign/zero/overflow, small integer rational determinant and lossless-map/reversal/cyclic
oracles, every normal binade's exact values and midpoint neighbors/ties/carry including MAX overflow,
subnormal/zero/min-normal boundaries, numerical cancellation, large translations, count-width
guards and structured inconsistent eligibility/coordinate/zero-area failures. No actual arbitrary
privately forged public certificate is accepted. One working API rustdoc example is added.

The arithmetic oracles do not call the production sign/rounder for expected values. Independent
integer determinants and expected neighboring IEEE bit patterns complement the earlier Fraction
model. Public range cases traverse actual upstream APIs without weakening their guards. The
existing exact orientation arithmetic/tests and upstream APIs/cadence remain unchanged. No
dependency, generator or canonical fixture changes; FX-11 remains optional and unexecuted.

Stable Linux/WSL Rust/Cargo 1.99.0 passes **481/0/2**, up from 459/0/2: 14 integration, seven private,
one example. Fmt, strict all-target Clippy, workspace test/build, warnings-denied private/public
rustdoc and git diff whitespace checks exit 0. Focused Release area integration 14, arithmetic 7,
existing validity integration 12 and unchanged exact-predicate 3 pass.

Native Windows 11 Pro build 26200, stable Rust/Cargo 1.99.0 `x86_64-pc-windows-msvc`, independently
passes **480/0/2** in the final full workspace replay; the existing Unix-only CLI test explains the
one-test difference. Fmt, strict all-target Clippy, workspace build, warnings-denied private/public
rustdoc and focused Release integration/arithmetic/validity/exact tests (14/7/12/3) exit 0. Real
native compilation/linking and execution confirm functional MSVC/SDK support, not WSL substitution.

The isolated native copy is a hashed **uncommitted working snapshot based on 2ca7e7e**, not a
committed implementation claim. All 108 exported files were verified using native Windows SHA-256;
final Rust/Cargo/generator/fixture bytes match the native-tested snapshot. Later validation-status
documentation updates are not silently represented as part of that earlier snapshot. Native source:
`%LOCALAPPDATA%\Temp\resin-slicer-m3-4b-validation\20261016-IwdWVH\source`.

**Retained failures/limitations:** The initial wrapper incorrectly shared output filenames because
of Bash dollar escaping; command exits were captured, but most per-command output was overwritten.
Its workspace test exit 101 is retained without inventing a cause or count. The corrected capture
also returned 101, recording 479 passed/1 failed/2 ignored: application-control OS error 4551 blocked
the unchanged `slicing::paths` doctest. A separate isolated replay passed 1/0/0, then the unchanged
full workspace passed 480/0/2, exit 0. No source, security policy, toolchain or dependency changed
between these runs. All surviving initial output, corrected failing output and successful replays
are archived separately. No failed command is relabelled as success; the host-policy trigger and
future recurrence remain unresolved. Final success establishes this scoped result, not immunity
from future Windows application-control restrictions.

External implementation logs:
`<evidence>/m3/m3-4b-implementation-20261016-IwdWVH/`.
No fuzz/performance campaign, actual allocator exhaustion or commercial execution occurs. Fixed
scratch and count/byte bounds are not measured runtime/RSS guarantees. Exactness is for represented
endpoints, not ideal pre-rounding geometry. Grid/construction/nesting/regions, EXP-03/04, M1 and
whole-M3 acceptance remain open.

## M3.4-C integer-grid research checks — no converter implemented

After the owner-requested M3.4-B commit `1c4f52e`, scoped static research and the
[integer-grid contract](M3-INTEGER-GRID-CONTRACT.md) initially left TD-025 proposed. The owner then
approved the binary/ExactOnly direction/domain and authorized design finalization only. Sections
8–11 fix metadata/API, a two-array 66-limb exact divisibility/range schedule, one outcome per graph
vertex, 3M vertex/6M agreed-axis/512 MiB requested-payload defaults and TD-014 checkpoint/error order.
The deterministic implementation matrix covers independent exact/boundary oracles, complete-only/
conflict provenance, resource/preflight limits, cancellation/retry and genuine native acceptance.
These are future tests, not current Rust coverage; no source/test/Cargo/generator/fixture changes.
Conversion implementation/native acceptance and construction/material gates remain open.

An independent standard-library Fraction/bit model, rerun by the main agent with exit 0, checks
134,004 small-box orientation triples, 185,500 product-by-product area partial states, 14,291 finite
binary64 decode/conversion-oracle cases, 33,475 rational rounding cases, 30,562 one-round inverse
cases and 51,000 segment-error samples. Explicit parity, range, double-rounding, rational-construction
and topology-change counterexamples complement symbolic proofs. A supplemental candidate model
passes 73,430 inverse shift/add cases across all proposed k and 24 exact signed boundary cases.
The proposed B=2^52 domain still gives a reduced 156-bit proper-intersection numerator; i128-safe
predicates/area do not certify a constructor. Finite checks do not prove every input, an unimplemented
limb helper, topology preservation, native behavior or measured performance/memory.

At that research checkpoint stable Rust/Cargo 1.99.0 Linux/WSL regression passed **481/0/2**, unchanged. Fmt, strict
all-target Clippy, workspace tests/build, warnings-denied private/public rustdoc and git diff
whitespace checks all exit 0; actual logs are external in the M3.4-C evidence directory. These
validate unchanged M3.4-B, not a future converter. Historical native
Windows 480/0/2 and host-policy limitations above remain separate; no new native Windows campaign,
fuzzing, benchmark, commercial execution or optional FX-11 execution occurs for documentation-only
research. Source and fixture SHA-256 checks preserve the committed production baseline.

Design-finalization logs are separate under
`<evidence>/m3/m3-4c-exactonly-design-20261016-oghNP5/`.
They do not extend the earlier commercial or native campaigns. An independent finite model passes
20,470 binary64 decodes and 90,353 forward schedule comparisons against a separate Fraction oracle
(15,541 exact, 37,515 off-grid, 37,297 out-of-range model classifications), including 18,572 aligned
boundaries and 8,388 exact half steps. Its 135 resource/cadence checks cover abstract formulas, not
Rust callbacks/allocators. Metadata/API/resource/checkpoint review and the model do not stand in
for a safe-Rust implementation, allocation failures or cross-platform tests. At design finalization,
stable regression again passed 481/0/2, with fmt/strict Clippy/tests/build/private+public rustdoc/diff
checks all exit 0. That checkpoint changed documentation only, not converter/native acceptance.

## Implemented M3.4-C ExactOnly coordinate coverage

After owner-requested contract commit `75acecd` and explicit implementation authorization,
`slicing::grid` implements Sections 8–11 of the independent contract. Sixteen new
`tests/exact_grid.rs` integration tests cover metadata validation/zero/exponents; analytic indices;
inclusive signed bounds and explicit rejection; ID/X/Y/residual precedence; exact origin subtraction;
complete-only/internal/duplicate/isolated and coincident identities; off-grid context not in a path;
world transforms and origin reindexing; open/ambiguous/crossed/collapsed/coincident/nested context;
exact/one-below independent caps before reservation; every checkpoint/retry/source preservation;
513-vertex global-axis cadence; empty/final/precancellation/error precedence; source permutations;
and all eleven small canonical fixtures with an independent bounded exact binary-scaling oracle.

Twelve private tests cover all finite exponent fields/selected mantissas/signs with a separate
individual-bit decode oracle, small signed integer divisibility, every supported k at exact domain
boundaries, midpoint neighbors/subnormals, extreme origin differences and residual precedence,
carry/borrow/capacity guards, wide differences against a separate signed binary-digit oracle,
structured malformed metadata slices, count/byte/isize guards and preserved conflicts. Two of
those tests deliberately perturb privately owned raw tangent records, then build normalization,
graph, paths and inspection normally: mixed and all-conflict views retain all observations; the
513-feature all-conflict stream exercises zero-axis budgets and cancellation at every observed
checkpoint. These are owned synthetic raw probes, not ordinary mesh-produced conflicts or public
forgeable inspection APIs. One working rustdoc example exercises the complete public pipeline.

Stable Linux/WSL Rust/Cargo 1.99.0 passes **510/0/2**, up from 481/0/2: 16 integration, 12 private,
one example. Fmt, strict all-target Clippy, workspace test/build, warnings-denied private/public
rustdoc and git diff whitespace checks exit 0. Focused Release grid integration 16, grid private 12,
unchanged validity integration 12, area integration 14 and exact-predicate tests 3 also pass.

Native Windows 11 Pro build 26200, stable Rust/Cargo 1.99.0 `x86_64-pc-windows-msvc`, passes
**509/0/2** in the final full workspace replay. The existing Unix-only CLI test explains the
one-test difference. Native fmt, strict Clippy, workspace build, warnings-denied private/public
rustdoc and focused Release grid/private/validity/area/exact suites (16/12/12/14/3) all exit 0.
Real native compilation/linking and execution establish functional MSVC/SDK support for this
campaign; no Linux Cargo substitutes for native validation.

The native copy is a hashed **uncommitted implementation snapshot based on `75acecd`**, not a
committed implementation claim. All 113 exported files were verified with native Windows SHA-256;
all 76 final Rust/Cargo/generator/fixture files match that tested snapshot. Later status/README
documentation updates are not represented as part of the earlier export. Isolated source directory:
`%LOCALAPPDATA%\Temp\resin-slicer-m3-4c-validation\20261016-gg4xO1\source`.

**Retained initial native failure:** the first workspace run exited 101 with 40 passed/1 failed/0
ignored before reaching core suites. The unchanged CLI test
`all_canonical_input_bytes_and_imported_geometry_are_unchanged` could not spawn its CLI process:
Windows application-control OS error 4551. An isolated unchanged replay passed 1/0/0, then the full
unchanged workspace replay passed 509/0/2, exit 0. No source, toolchain, installation or security
policy changed between runs. All initial/replay output and actual exits are archived separately.
The specific host-policy trigger and future recurrence are unresolved; final scoped acceptance
does not erase that failure or promise immunity from application-control restrictions.

No kernel, existing exact/area arithmetic, upstream numerical API/cadence, Cargo, generator or
canonical STL changes. No dependency, unsafe, fuzz/performance campaign, actual allocator exhaustion
or optional FX-11 execution. Finite oracles and fixed resource bounds are not universal topology,
RSS or throughput proofs. No constructor, inverse decoder, integer predicate/area, region or sampling
policy is added. Lossy/construction/material and M1/M3 gates remain open. External implementation
logs: `<evidence>/m3/m3-4c-implementation-20261016-gg4xO1/`.

## Implemented M3.5-A strict-region hierarchy coverage

TD-026 and [M3-REGION-HIERARCHY-CONTRACT.md](M3-REGION-HIERARCHY-CONTRACT.md) define the scoped,
nonconstructive exact hierarchy. `crates/resin-core/tests/region_hierarchy.rs` covers one strict
pipeline with disconnected roots, shell/hole/island nesting, orientation-independent reverse views,
path/region/hole ordering, exact pair/relationship accounting and immutable borrowed upstream input.
It also covers open chains, non-simple cycles, cross-path contact diagnostics, every independent
resource class, cancellation at every observed `Regions*` phase, atomic failure and retry.

Private `slicing::regions` tests independently exercise the half-open-Y positive-ray convention at
horizontal vertices and the inclusive TD-025 coordinate bound used by the `i128` side predicate.
The public-pipeline fixture retains collinear split vertices from the existing mesh traversal so it
also proves that no simplified/reconstructed ring is substituted for the canonical path. Crossings,
touches, overlaps and coincidences remain unresolved outcomes; no construction is tested or implied.

Stable Linux/WSL validation passes `cargo fmt --all -- --check`, strict workspace Clippy, workspace
build, warnings-denied public/private rustdoc, `git diff --check` and **518 passed, 0 failed,
2 ignored** workspace tests. The five focused hierarchy tests also pass in Release. Native Windows
validation remains required and is not inferred from these results. This is not whole-M3 fuzzing,
performance evidence, multi-layer validation or M3 closure.

## Implemented M3.5-B uniform schedule coverage

TD-027 and [M3-LAYER-SCHEDULE-CONTRACT.md](M3-LAYER-SCHEDULE-CONTRACT.md) define explicit exact
dyadic scheduling independently of mesh traversal and commercial defaults. The 26 integration tests
in `crates/resin-core/tests/layer_schedule.rs` use bounded i128/rational-unit oracles, native integer
rounding near 2^53, exact endpoint membership, signed ties, binade carries, subnormals, overflow,
duplicate-plane errors, u64 count boundaries, capacity precedence, every cancellation phase and
deterministic traces. A deterministic raw-binary64 exponent sweep checks a separate single-round
expression with exactly represented small indices. No test treats a rounded terminal comparison
as mathematical membership or a floating index as the full-u64 domain.

Nine private tests cover 20,470 decode/round trips, exact expression evaluation above 2^53 and at
u64::MAX, the 2176-bit capacity extremes, both-sign guard/sticky boundaries, nearest-even overflow
thresholds, exact recurrence consistency and defensive fixed-capacity overflow. One API doctest
demonstrates an explicitly configured schedule. No existing mesh/section/region behavior changes.

An additional independent Python `fractions.Fraction` oracle agrees on **8,000** deterministic
cases from seed 3487042: 5,728 successful schedules, 1,255 mathematical index-range failures,
790 resolution failures, 222 count-limit failures and five rounded-overflow failures. Arbitrary
finite binary64 input bits, positive heights, terminal rules and signed-cancellation cases are
checked against exact rational membership and Python's independent correctly rounded Fraction-to-f64
conversion. External runner/script/report and complete input/output remain in `/tmp/resin-m35b-oracle*`.
Input SHA-256 is `7dfa4fc38d9d4ea30cd1aed03d0432fd209ea3e2a5cefa4c1a0577613676fa3b`;
output SHA-256 is `1093a18bee9d565541f5602ea84aa308381381240042187d6135701f161cf72a`.
These temporary records are supplemental, not permanent native-platform acceptance.

The isolated `layer_schedule` fuzz target checks raw-f64 Count/Before/Through inputs, zero-first
single-round multiplication, bounded dyadic integer membership, independent native FMA rounding
for exactly represented small indices, deterministic reruns and bounded output. Offline nightly
ASan fuzzing with seed 3502 and a 20-second requested cap passes **1,256,859 executions** (21
reported seconds), exit 0, 419 MB peak RSS and no failure artifacts. Final retained-corpus replay
passes 712 files (572 accepted schedules, 140 expected rejections); initial replay passed 42 seeds.
No production assertion or numerical policy needed correction. Exact commands, logs, hashes and
corpus live outside Git under `/tmp/resin-m35b-fuzz/`.

The initial campaign completed 1,222,599 runs without an oracle/ASan finding but exited 1 during
LeakSanitizer teardown because the host disallows its ptrace operation. Its empty teardown artifact
replays normally. The successful second campaign explicitly sets `ASAN_OPTIONS=detect_leaks=0`:
address-safety instrumentation remains active, but no leak-freedom claim is made. Both original
and successful logs remain separate. This short campaign is bounded evidence, not sustained or
whole-M3 fuzz acceptance, performance measurement or proof of defect absence.

Stable Linux/WSL validation passes format checking, strict all-target workspace Clippy, workspace
build, warnings-denied public/private rustdoc and **554 passed, 0 failed, 2 ignored** workspace
tests. The 26 integration and nine private tests separately pass Release. Native Windows execution
is not performed in this session: the WSL interop registration is absent, although mounted Windows
application files are readable. Earlier native milestones are not evidence for this changed code.
Bit-level portable tests are ready for a separate native campaign. EXP-03 remains required for any
product-compatible default. This is not M3.6 progress/geometry orchestration or whole-M3 acceptance.

## Implemented M3.6 owned layer engine coverage

TD-029 and [M3-LAYER-ENGINE-CONTRACT.md](M3-LAYER-ENGINE-CONTRACT.md) define the independent
composition, ownership, lifecycle, progress and collection contract. Twelve integration tests in
`crates/resin-core/tests/layer_engine.rs` cover multiple analytic planes, a closed cube and empty
outside layer, disconnected roots and nested shell/hole/island geometry, transformed source
preservation and owned output outliving all borrowed inputs. Every owned flat reference, raw
observation/occurrence chain, summary and diagnostic is compared with the unchanged direct-stage
pipeline. A deterministic fixed-width little-endian/f64-bit test encoding additionally checks
repeatability; Debug formatting is not its correctness oracle.

Open paths, duplicate contributions, cross-path contacts, tangent points and coplanar boundaries
retain explicit outcomes and provenance. A second off-grid plane fails after a successful first
layer without invalidating that owned output. All eight stage-limit errors retain their typed
source and plane context. Cancellation tests inspect structured causes/checkpoints throughout
stages and all nine new orchestration/projection/collection checkpoints, including both sides of
reservation and empty final exhaustion. Progress counts are monotonic; the last yielded item is
not normal exhaustion, early drop has no completion, failure is fused, and neither aggregate cap
failure nor final collector cancellation emits Completed. Collector tests cover exact byte/count
boundaries, no successful partial collection and rejection of an already-started engine.

Three private output tests cover owned coplanar provenance/flat references, exact projection byte
cap acceptance with one-byte-less rejection before storage, and cancellation without publication.
The nested multilayer analytic fixture initially failed in both the engine and direct pipeline:
depths `[0,1,0,0]` instead of `[0,1,2,0]`. This exposed TD-026's ray eligibility missing its strict
upper-Y bound. Production now applies the specified half-open interval on both endpoints. A new
private regression checks 500 analytic non-boundary rectangle queries under collinear subdivision,
both traversal directions and every cyclic start; the existing hierarchy corpus remains passing.
The correction changes previously validated production geometry and requires new native validation.

Stable Linux/WSL gates pass format checking, strict workspace/all-target Clippy, workspace build,
warnings-denied public/private rustdoc and **571 passed, 0 failed, 2 ignored** tests. Focused Release
runs pass 12 engine integration, three output-private, three containment-private and five existing
region-hierarchy integration tests. Native Windows validation is pending: current WSL interop
registration is absent. Prior native results and Linux success are not substituted for it. No
commercial parity, physical printability, M1 completion or whole-M3 acceptance follows from M3.6.

The isolated `layer_engine` fuzz target exercises tiny full pipelines under fixed work/output caps,
including source preservation, owned reference conservation, explicit unresolved outcomes,
deterministic values/errors, streaming-versus-collection equivalence and cancellation fusion.
The final current-source ASan campaign passes **7,997 executions in 21 reported seconds**, seed
20261018, 431 MiB peak RSS and zero artifacts. An earlier passing campaign completed 8,730 runs;
the final campaign includes the strengthened reference/summary/unresolved oracle and corrected
containment source. Final replay passes 128 fixed seeds (86 accepted schedules/42 expected typed
failures) and 1,093 retained corpus inputs (384 accepted/709 expected typed failures). The isolated
fuzz workspace also passes offline strict all-target Clippy and formatting. Commands, environments,
logs, source/production hashes and seed digests remain in `/tmp/resin-m36-fuzz/`.

Both campaigns use `ASAN_OPTIONS=detect_leaks=0` because LeakSanitizer's required ptrace operation
is unavailable on this host; ASan remains enabled with debug assertions, without a leak-validation
claim. A short bounded campaign does not prove defect absence, cover arbitrary input complexity,
establish a performance budget, or close whole-M3 fuzz/native acceptance.

## Planned later M3 completion validation

The remaining-M3 audit subsequently implemented the constructive operation and dedicated strict
region campaign below. Large-input, native-platform and budget acceptance remain independent.

Structured fuzz targets cover schedule and full bounded layer generation. A dedicated region
assembly campaign and any future constructor's arrangement target remain independent work.

Whole-M3 closure additionally requires measured clean/deep/disconnected/pair-cap and multi-layer
time/requested-capacity/peak-RSS baselines, owner-approved preliminary budgets, deterministic test
encoding, warnings-denied rustdoc, focused Release checks and genuine native Windows/Linux validation.
No acceleration or parallel framework is accepted before those measurements.

## Remaining-M3 exact constructive composition coverage

TD-030, [M3-CONSTRUCTIVE-MATERIAL-CONTRACT.md](M3-CONSTRUCTIVE-MATERIAL-CONTRACT.md) and the
[audit](M3-COMPLETION-AUDIT.md) define the remaining implementation scope. Eight independent
`constructive_material` tests check exact source-group parity/union, rational crossings, coincident
and reversed rings, point/shared-edge/T-junction contacts, a hole touching its outer at a vertex,
five-deep nesting, source order permutations, every cap/cancellation phase and invalid input.
Independent rational occupancy and material-left probes over bounded fixtures, 36 three-rectangle
arrangements, closed-cycle conservation and complete original-edge atomic support partitions are
the oracles. No production point-in-ring implementation is its sole correctness oracle.

Eight private exact-kernel tests cover signed arithmetic, every single-bit multiplication capacity
pair, 6,561 independently checked small-grid segment pairs, the TD-025 >i128 intersection and
domain-extreme exact point ordering. Four `layer_composition` integration tests distinguish same-source
parity from independent-source union, preserve original strict diagnostics, test face-label count,
mixed path ownership and typed input-edge limits, cancellation/fusion, scratch caps and retained
per-layer payload. One new API doctest shows explicit material sources.

Stable Linux/WSL format, strict all-target Clippy, workspace tests/build and warnings-denied
public/private rustdoc pass: **592 passed, 0 failed, 2 ignored**. The eight constructive, four adapter
and eight arithmetic tests separately pass Release. No production dependency/unsafe kernel is added.

Two dedicated isolated ASan campaigns pass with `detect_leaks=0`, retaining address instrumentation
and debug assertions: `construct_material` has **6,713 executions / 21 reported seconds / 406 MiB
peak RSS**, seed 20261019; `strict_regions` has **1,796 / 21 seconds / 387 MiB**, seed 20261020.
Neither reports a geometry/oracle/sanitizer failure or artifact. Complete final corpora replay
425 and 153 inputs respectively, with unchanged geometry/oracle hashes. Strict-region cases use
nested/disconnected/contacting ribbons, exact dyadic planes, independent box containment depths,
role parity, direct-stage equivalence and raw provenance. Constructive cases additionally use
independent off-boundary occupancy, complete atomic support, deterministic errors and cancellation.
External commands/versions/source hashes/logs/corpus digests are in `/tmp/resin-m3-completion-fuzz/`.

A precursor replay encountered the already documented unavailable LeakSanitizer ptrace operation;
an initial corpus replay raced an active corpus replacement and was rerun after exhaustion. These
environment/harness failures are separate from successful final campaigns; no geometry assertion
was weakened. Leak-freedom is not claimed. Short small-input fuzzing does not establish worst-case
behavior, universal validity or whole-M3 acceptance.

The [M3 Release protocol](../tools/m3-profile/README.md) and `examples/m3_profile.rs` independently
validate six workloads before/after timing: nested/disconnected layers, 64 cycles, 150,024 source-face
evaluations, 24 overlapping material rectangles, rational triangle union and exact pair-cap rejection.
Three warmups/ten batches and three fresh RSS processes per workload retain timing/requested payload
and whole-process RSS separately. Initial medians are 2.074 ms, 15.725 ms and 6.435 ms for the three
strict three-layer collections; 13.395 ms rectangle union and 37.027 microseconds rational union.
The tiny cap-rejection timing is not meaningful throughput evidence. Raw samples/environment and
executable/source hashes are in `/tmp/resin-m3-profile/`. These are baseline observations, not adopted
large-input performance budgets. ExactOnly-rejected alternative plane settings remain a documented
domain limitation rather than being silently snapped.

Native Windows on this changed revision is pending because current WSL interop registration is
absent. Agreed-corpus/numerical-envelope review, large-input budgets and sustained fuzzing remain
separate acceptance work. EXP-03/04 and M1 remain open; no whole-M3 or physical printability claim.

## Implemented M2.1 mesh/import coverage

Tests are dependency-free and independent of commercial installations. Existing identity/CLI and
fixture-generator tests remain unchanged. New integration suites are:

- `crates/resin-core/tests/mesh_geometry.rs`: empty/point/negative/zero bounds, finite-range handling,
  invalid indices, signed zero, source winding, repeated/collinear and duplicate faces, thin triangles,
  and scale-normalized normals across very small/large magnitudes.
- `crates/resin-core/tests/stl_import.rs`: binary structure/endian/attributes/normals, every incomplete
  prefix of a valid binary facet, count mismatch/excess, `solid` binary headers, trailing-data policy,
  ASCII grammar/whitespace/names/scientific notation, syntax/numeric/underflow errors, all-axis/corner
  non-finite binary inputs, deterministic results, byte/triangle/vertex/line/token caps, reader
  interruption/I/O causes, and bounded deterministic single-byte mutations.
- `crates/resin-core/tests/reference_meshes.rs`: all eleven small manifest-listed fixtures, including
  FX-10 variants, counts/bounds, defect counts, every original corner and winding, exact promoted
  binary coordinates, and deterministic reruns. Test-local extraction reads only primitive fields
  from the versioned owned manifest; it is not a production general-purpose JSON parser. No guessed
  fixture filename or external JSON dependency is required.
- The STL public API has an executable documentation example with an explicit unit policy.

Manifest bounds are rounded to nine decimal places: endpoint comparisons allow 5.1e-10 mm for
that text-rounding boundary only. Actual serialized corner comparisons and the specified cube/
cylinder/prism extents use exact promoted values. No GUI hundredth-mm allowance is used as engine
precision. Tests separately distinguish exact repeated corners from computed-zero area; no robust
exact predicate, near-degeneracy threshold, watertightness, or self-intersection coverage is claimed.
Open/reversed/non-manifold fixtures retain every face; an empty basic diagnostic list is not validity.

FX-11 is not required or generated in ordinary tests. The explicit optional command is:

```console
cargo test --release -p resin-core --test reference_meshes optional_fx11_bounded_import_measurement -- --ignored --exact --nocapture
```

It requires the existing manifest-named local file, checks counts/diagnostics, and reports elapsed
file-read/import/analysis time. No timing threshold is an acceptance gate. On 2026-10-15 a separately
SHA-256-verified 16,384,084-byte FX-11 imported 327,680 triangles and 163,842 vertices in
156.994019 ms for one release-mode sample of the final implementation on AMD Ryzen 7 7700,
`x86_64-unknown-linux-gnu` under WSL,
Rust 1.99.0/Cargo 1.99.0. Hash verification preceded the read, and cache state/background load were
not controlled. Facet metadata occupies 32 bytes/face on this target. This is not a throughput
benchmark, repeated-run statistic, cold-cache result, total-memory measurement, or cross-product claim.

The final M2.1 gate includes format application/check, strict Clippy, workspace tests/build,
`git diff --check`, public documentation generation, and CLI help/version smoke checks. Routine
coverage is 49 new integration tests plus one new documentation test alongside the 17 existing
workspace tests; the optional FX-11 test is ignored by default and was run separately.
At M2.1, native Windows compilation/runtime, 32-bit execution, sustained fuzzing, peak-memory profiling,
cancellation, and full topology/transform tests remained outstanding. M2.2 transform coverage is
recorded below; the other limitations remain. Passing the bounded mutation
sweep does not substitute for a sustained fuzz campaign or hostile-input audit.

## Implemented M2.2 transform coverage

Starting baseline `0af4a13` contains 67 passing tests (including the STL documentation example) and
one default-ignored optional FX-11 test. No existing test or fixture is weakened or rewritten.
M2.2 adds 48 integration tests and one executable transform example:

- `tests/transformations.rs` covers point/direction identity and translation, uniform/non-uniform
  scale, each right-handed 90° axis rotation, signed/arbitrary angles, an analytic diagonal-axis
  120° rotation, explicit extrinsic XYZ order, and independent scalar trigonometric expectations.
  It covers quaternion normalization across maximum/subnormal magnitudes, q/-q equivalence versus
  representation equality, normalized rotation composition, exact/large pivots, direct/affine paths,
  affine shear/composition, noncommuting rotation/translation and rotation/non-uniform-scale orders,
  zero/reflection policies, source-independent direction behavior, deterministic repeats, all input
  categories' finite checks, and controlled arithmetic/cancellation overflow.
- `tests/world_bounds.rs` loads FX-01, FX-05, and defective FX-08 by manifest filename. It verifies
  analytic cube extrema, +5 mm lift without placement, below-plate pivoted scaling, source/metadata/
  winding/diagnostic preservation, empty meshes, unused vertices, large finite coordinates, singular/
  reflected bounds, point-error vertex/cause context, and finite-point/overflowing-extent failures.
  Independent scalar Rx→Ry→Rz equations—not production rotation/matrix/bounds code—produce mixed-axis,
  non-uniform, non-origin-pivot reference extrema from actual quantized STL vertices. A distinct
  eight-corner enclosure case proves that source-AABB rotation is not tight mesh bounding.

Simple transform comparisons use a scale-aware `64 * f64::EPSILON * max(1, abs(expected))` budget
for short small-angle operation chains. Fixture comparisons allow `256 * f64::EPSILON * 100` mm,
with tested point/pivot/offset magnitudes below 100 mm, to cover different trig/quaternion/scalar
evaluation paths. These are explicit test budgets, not universal numerical guarantees or engine
epsilons. Integer/identity and preserved buffers use exact equality where justified; quaternion norm
tests use eight epsilons for four-term normalization rounding. Very large angles are tested for
finite behavior only, not accurate phase recovery. General inverse/orthogonality proofs are not claimed.

FX-05 zero-pivot Rz(+30°) extents are independently recalculated as approximately
28.857155951470816/27.863533428766498/35.42976760864258 mm; the 1.25-scaled reference is
36.07144493933852/34.829416785958124/44.28720951080322 mm. A separate raw-STL standard-library scalar
calculation rechecked these numbers during implementation. The 0.005 mm allowance against EXP-02
CHITUBOX display text is confined to that secondary two-decimal GUI comparison. It is never used for
engine validation, and no GUI pivot/matrix convention is thereby proven. M1 reports remain unchanged.

Validation on 2026-10-15 with Rust/Cargo 1.99.0, `x86_64-unknown-linux-gnu` under WSL, passed 116
tests with zero failures and one existing optional test ignored: 114 unit/integration tests plus two
documentation examples. Formatting, strict Clippy, workspace tests/build,
`cargo doc --workspace --no-deps --document-private-items`, and `git diff --check` pass;
documentation is also checked with warnings denied. Native Windows validation
remains separate. M2.3 edge incidence, connectivity/winding diagnostics, normal conversion/inversion,
sustained fuzzing, cancellation, and workload timing/peak-memory profiling are not M2.2 claims.

## Implemented M2.3 topology coverage

Baseline `de566d6` on `main` was clean and contains 116 passing tests and one default-ignored
optional FX-11 import test. Existing M2.1/M2.2 suites, fixture generator, canonical STLs, manifest,
dependencies, and M1 research are unchanged. M2.3 adds 29 synthetic integration tests, nine routine
fixture integration tests, one capacity-overflow unit test, and one executable topology documentation
example: 40 new routine passing tests. A new optional FX-11 topology measurement is ignored by default.

`tests/topology.rs` independently specifies triangle, coherent/conflicting two-face edge,
three-face edge, closed tetrahedron, reversed/global-inward winding, interleaved disconnected
components, vertex-only touching tetrahedra, open bow-tie fans, same/opposite-winding duplicate
faces, repeated indices, fully collapsed faces, repeated coordinates with distinct indices,
collinearity, computed-zero-area underflow, excluded faces attached to otherwise closed surfaces,
coincident/nearby manual vertices without welding, empty/isolated meshes, invalid construction,
resource boundaries, structured error causes, and deterministic report/member/issue ordering.
High-degree cycle links and 4,096-face edge incidence exercise iterative algorithms without recursive
stack growth or dense pairwise adjacency. Resource caps include excluded faces and unused vertices.
An internal `usize::MAX` byte-capacity test exercises controlled overflow without allocating.
Synthetic transforms include mixed rotation, non-origin pivot, non-uniform scale, and reflection;
all are ordinary invertible scene operations, not geometry baking or singular-degeneracy claims.

`tests/topology_fixtures.rs` obtains every path from the authoritative manifest. Source vertices,
indices, winding, bounds, metadata/header/stored normals/attributes, and import diagnostics compare
equal before/after repeated topology and world-bound analysis across all 11 small fixture files.
FX-05 matches the cube's indexed topology despite baked oblique coordinates. FX-03/04/06/07 also
check closed component counts without establishing shell nesting or volume validity.

The following expectations were independently checked against construction specifications and a
separate raw binary-STL decoder/counter (not production topology code). All 12 available STL files,
including local FX-11, matched their manifest SHA-256 before validation. Exact imported identifiers
below follow first-occurrence coordinate deduplication, not original generator buffer indices.

| Fixture | Source V / F | Usable F | E | Boundary / two-face / non-manifold | Winding conflicts | Vertex links: interior / boundary / non-manifold / indeterminate |
| --- | --- | --- | --- | --- | --- | --- |
| FX-01 | 8 / 12 | 12 | 18 | 0 / 18 / 0 | 0 | 8 / 0 / 0 / 0 |
| FX-02 | 258 / 512 | 512 | 768 | 0 / 768 / 0 | 0 | 258 / 0 / 0 / 0 |
| FX-05 | 8 / 12 | 12 | 18 | 0 / 18 / 0 | 0 | 8 / 0 / 0 / 0 |
| FX-08 | 9 / 15 | 13 | 18 | 0 / 15 / 3 | 0 | 4 / 0 / 2 / 3 |
| FX-09 | 8 / 12 | 12 | 18 | 0 / 18 / 0 | 3 | 8 / 0 / 0 / 0 |
| FX-10A | 8 / 10 | 10 | 17 | 4 / 13 / 0 | 0 | 4 / 4 / 0 / 0 |
| FX-10B | 9 / 13 | 13 | 20 | 2 / 17 / 1 | 0 | 6 / 1 / 2 / 0 |

All seven primary fixtures have one usable edge component and one usable vertex component.
Cube/prism: 12 cube sides plus six quad diagonals. Cylinder: two 128-vertex rings plus two cap
centers; 256 ring edges + 128 vertical edges + 128 wall diagonals + 256 cap spokes. Open cube:
removing the top quad leaves four boundary sides, 10 triangles, and 17 edges. Added-face cube:
one third-use cube edge plus two new boundary edges yields 20 edges and 13 faces. Its imported
non-manifold edge is `(0,3)` with source faces `[1,4,12]`; endpoints have branched links.

FX-09 conflicts are `(0,1)` faces `[0,1]`, `(0,2)` faces `[0,10]`, and `(1,2)` faces `[0,8]`.
It remains closed combinatorially but fails supplied local winding. FX-08 retains all 15 faces:
12=`[0,0,3]` and 13=`[0,8,3]` are excluded, and 14 duplicates face 0. Duplicate edges `(0,1)`,
`(0,2)`, `(1,2)` each have three usable faces. Vertices 0/3/8 are indeterminate; 1/2 have duplicate
link edges; vertex 8 has excluded-only incidence and no component membership. Its usable counts
intentionally differ from the unchanged manifest's all-contribution counts (three boundary and
four non-manifold edges), which include degenerate edges. No raw evidence is reinterpreted or edited.

The optional topology command requires the existing manifest-named file and does not generate it:

```console
cargo test --release -p resin-core --test topology_fixtures optional_fx11_bounded_topology_measurement -- --ignored --exact --nocapture
```

On 2026-10-15, one release sample on AMD Ryzen 7 7700, Rust/Cargo 1.99.0,
`x86_64-unknown-linux-gnu` under WSL, passed V=163,842 / F=327,680 / E=491,520 with all interior
links, no defects, and one component under both definitions. These counts follow subdivision-7
icosphere construction: V=`10*4^7+2`, F=`20*4^7`, E=`30*4^7`. Read/import took 177.817004 ms;
topology alone (including basic geometric diagnostics) took 273.916543 ms. SHA verification was
separate. Cache/background load were uncontrolled; this is not a controlled benchmark, distribution,
memory measurement, or cross-platform/competitor performance claim. Report use/edge record payloads
are 24/32 bytes on this target; the architecture explains uncounted storage and temporary overhead.

The final M2.3 Linux/WSL gate passes formatting, strict workspace Clippy, workspace tests/build,
warnings-denied public/private documentation, `git diff --check`, and established CLI help/version
smoke checks. Routine results: 156 passed, zero failed, two ignored (153 unit/integration tests and
three documentation examples). The new optional FX-11 topology test additionally passed separately.
M2.3 scope is functionally complete under Linux/WSL; native Windows build/runtime remains unvalidated.
Sustained fuzzing, allocator-failure injection, 32-bit execution, cancellation, controlled throughput
and peak-memory profiling remain open. No self-intersection, global reorientation/orientability
solver, outwardness, nesting, positive-volume, wall-thickness, printability, or repair proof is claimed.

## M2.4-B coverage-guided fuzzing

The initial WSL preparation on 2026-10-15 starts from clean `main` at
`3c6ed2e84888b98194a564786f26228acd9783a7`. The separate `fuzz/` workspace defines binary STL,
ASCII STL, actual automatic-detection, and bounded indexed-topology libFuzzer targets. The shared
oracle checks finite geometry, indices/statistics/bounds/diagnostics, auto-selected parser agreement,
topology edge incidences, complete component membership, ordered repeatability, and source
preservation. Malformed imports may return structured errors; no manifoldness is presumed.
The production graph, APIs, implementation, root lockfile, canonical fixtures, and M1 research are
unchanged. The explicitly allowed `libfuzzer-sys` testing dependency is isolated to `fuzz/`.

Execution was initially blocked by missing nightly Rust and cargo-fuzz. The unchanged preparation
archive records zero coverage-guided executions at that stage. After the owner installed the tools,
the resumed checkout contained the 13 expected uncommitted preparation files at the same HEAD.
The existing infrastructure and seed corpus were reused; no production code or assertions changed.

All 11 committed small STL hashes were verified before preparing seven selected canonical/derived
fixture cases and synthetic seeds. There are 97 external seed files: binary 19, ASCII 18, auto 38,
topology 22. Stable compilation/lint and ordinary seed replay validate the shared oracle and expected
seed acceptance/rejection only. They do not validate nightly instrumentation, fuzz engine linkage,
mutation effectiveness, or coverage. Ordinary workspace validation still passes all 156 tests,
with zero failures and the same two optional tests ignored. FX-11 is not read or generated here.

See [the fuzzing protocol](FUZZ-CAMPAIGNS.md) for input encodings, dependency boundary, exact
preparation/resumption/reproduction commands, seed provenance, intended limits, failure handling,
and limitations. Evidence resides outside Git under
`<evidence>/m2/m2-4b/20261015-3c6ed2e-preparation-yZvuTT/`.
Its prerequisite/campaign JSON records the original blocker; seed manifests and preparation logs
support the narrower preparation findings.

### Resumed campaign results

On 2026-10-15, all four targets built and ran with nightly rustc 1.101.0-nightly (`7df81381d`,
2026-10-14), Cargo nightly 1.101.0-nightly (`3960fa69b`), cargo-fuzz 0.13.2, libfuzzer-sys 0.4.13,
GCC C++ 13.3.0, and LLVM 23.1.3 on WSL/Linux `x86_64-unknown-linux-gnu`. Stable Rust/Cargo 1.99.0
remained the default. The agent installed no tools; approved testing dependencies were resolved
only in the isolated package. The exact testing lockfile/tree/metadata are retained outside Git.
An initial offline all-platform metadata query lacked an unused platform package; Linux-filtered
metadata succeeded offline. This did not affect target builds or campaigns.

All four instrumented builds and runs exited 0. Installed Cargo/libFuzzer help verified option
meanings/support; ASan/libFuzzer runtime and coverage symbols were recorded from each binary.
Actual commands used ASan, debug assertions, 64 KiB maximum input, 3-second input timeout,
1,024 MB RSS cap, 256 MB single-allocation cap, and `-max_total_time=120`. Topology retained
512-vertex/1,024-face caps. Random seeds were 20261015/20261016/20261017/20261018 in table order:

| Target | Executions | Coverage INITED → DONE | Features INITED → DONE | Active corpus INITED → DONE | Reported seconds | Peak RSS MB |
| --- | ---: | --- | --- | --- | ---: | ---: |
| binary_stl | 453,022 | 414 → 456 | 3,190 → 5,886 | 18 → 800 | 121 | 432 |
| ascii_stl | 5,377,414 | 530 → 672 | 3,342 → 7,956 | 17 → 1,829 | 121 | 468 |
| auto_stl | 359,506 | 660 → 772 | 4,937 → 9,043 | 35 → 1,456 | 121 | 469 |
| topology | 36,177 | 2,583 → 2,812 | 13,525 → 29,386 | 22 → 1,200 | 121 | 497 |

Total: 6,226,119 coverage-guided executions. The reported 121 seconds reflects libFuzzer's
integer-second completion check for the configured 120-second budget. Active corpus counts are
libFuzzer's initialized/final in-memory selection, distinct from raw seed/discovery file counts;
coverage/features are per-instrumented-target counters, not whole-program percentages. No sanitizer
finding, crash, panic, invariant failure, timeout, resource-limit failure, watchdog, unsupported
option, or failure artifact was reported. No production fix or regression test was needed. Runtime/
RSS statistics are QA telemetry, not M2.4-C profiling or a controlled benchmark.

Infrastructure adjustments add requested runtime/corpus/failure/version reporting to the existing
runner and replace the preparation helper's obsolete missing-tool label with
`NOT_PERFORMED_BY_PREPARATION_CHECK`; no oracle was weakened. All 97 copied seeds retained their
hashes. The old preparation archive remains unchanged. New logs, discoveries, commands, build
results, tool/dependency records, source/seed provenance, and integrity manifests reside under:

`<evidence>/m2/m2-4b/20261015-3c6ed2e-resume-R4uSnn/`

Stable format, strict Clippy, workspace tests/build, warnings-denied rustdoc, and whitespace checks
pass; tests remain 156 passed, 0 failed, 2 ignored. Acceptance is **PASS for the initial bounded
M2.4-B scope**. Sustained campaigns, retained-corpus minimization/replay, expanded numerical/input
distributions, and regression monitoring remain outstanding. Short clean runs do not establish
absence of defects. Windows fuzzing, arbitrary-f64/transform fuzzing, and M2.4-C profiling were not
performed; canonical fixtures, their generator, production dependencies, and M1 research are unchanged.

## Implemented M2.4-C performance baseline

The dependency-free `resin-core` example and standard-library Python runner under
`tools/mesh-profile/` establish a stable Release baseline without production changes. See
[the measured baseline](PERFORMANCE-BASELINE.md) and [reproduction protocol](../tools/mesh-profile/README.md).
Four hash-verified fixtures (FX-01/02/05 and existing ignored FX-11) cover separate warm file reads,
buffered public import, resident-mesh topology, and five full-vertex world-bound transforms. Every
workload uses three warm-up batches and ten timed batches; small operations are batched. Results
validate outside timing against construction counts and independently decoded/scalar-transformed STL
vertices. Median/min/max/IQR and all raw samples are retained; no inconvenient sample is discarded.

All 32 timing workloads and 99 separate GNU Time RSS processes, including three runtime baselines,
pass. RSS is whole-process peak including setup, not exact mesh heap or a subtractable phase delta.
Five helper tests and three intentional nonzero rejection checks validate infrastructure. Ordinary
stable gates still pass 156 tests, zero failures, two ignored; optional old single-sample FX-11
tests stay ignored. Generated logs, executable, raw data, hashes, and provenance are external under
`<evidence>/m2/m2-4c/20261015-2119969-t3MuPY/`.
This is an initial WSL/Linux baseline, not a performance target, historical regression percentage,
native Windows benchmark, precise allocator profile, or whole-M2 closure.

## M2 Closure A: cooperative cancellation

Starting clean `main` at `d9fd30a3b5c89eecf01e9132ea8d768b0e92aa10`, this increment adds borrowed
synchronous cancellation predicates, additive entry points, and typed cancelled outcomes. See TD-014
and the architecture for precedence, no-partial-publication guarantees, 256-record polling, bounded
byte chunks, and explicit sorting/blocking-I/O/allocation/string/cleanup limitations.

`crates/resin-core/tests/cancellation.rs` adds 29 deterministic integration tests. They cover forced
binary/ASCII and automatic pre-cancellation; detection byte chunks; counter-driven binary and ASCII
parsing; deduplication; construction/index/source-bound validation; geometric and metadata diagnostics;
all topology phases; a generated 1,024-face fan with cancellation inside one high-degree neighborhood;
post-sort cancellation; direct/affine bounds; empty/small/final-checkpoint behavior; correct error
classification/causes; unchanged geometry/metadata; successful retry; read consumption and missing
paths; repeated Interrupted calls; resource/I/O/numeric-error distinction; deterministic traces; and
the final-check observation race. No sleep-based synchronization or real-time latency assertion is used.

Never-cancelled APIs compare equal to compatibility APIs across all 11 small canonical STL files,
including FX-10 A/B and defective fixtures, for import paths/readers/bytes, construction, diagnostics,
topology and mixed-axis/non-uniform/non-origin-pivot object/affine bounds. Additional ASCII and empty
cases are checked. All fixture paths come from the authoritative manifest; no fixture is modified.
Two control unit tests check the no-predicate path and exact 256-record polling helper. One executable
public cancellation documentation example exercises rejection at final import publication.

Existing tests remain intact: baseline 156 passed plus 32 new tests = **188 passed, 0 failed,
2 optional tests ignored** on stable Linux/WSL. Required format, strict Clippy, workspace test/build,
warnings-denied public/private rustdoc, whitespace and established CLI smoke checks pass. Native
Windows validation of the changed production code remains separate; the earlier M2.4-A result is
not reused as validation of cancellation. No CLI inspection, transform round-trip tests, benchmark,
fuzz campaign, canonical fixture change, dependency, unsafe code, or job scheduler is introduced.
Polling overhead is unmeasured; existing M2.4-C numbers remain tied to their historical commit.

## M2 Closure B: read-only CLI inspection

Starting clean `main` at `571c7772b326d68f6acbfcfe28efdaef27b71695` (committed Closure A), the CLI
adds `inspect --assume-mm <path>` through existing public import/statistics/source-bounds/diagnostic/
topology APIs. No production core, existing test, Cargo dependency, canonical fixture, generator,
fuzz assertion, cancellation infrastructure, or transform round-trip coverage changes.

`crates/resin-cli/tests/inspect.rs` adds 25 native-process integration tests on Linux/WSL, including
one Unix-only non-Unicode-path test (24 tests on Windows). Coverage includes FX-01 binary and ASCII
import, indexed counts, exact source bounds, full promoted-f32 FX-05 precision, empty binary/ASCII
semantics, degenerate/duplicate/reversed/open/non-manifold/disconnected fixtures, stored-normal and
attribute findings, and 201 generated defective faces proving both 100-detail limits/full totals/
explicit omissions. It checks usage errors, missing/malformed input, stdout/stderr and 0/1/2 status,
absolute/relative/space/leading-hyphen paths, deterministic reports/field order, all help/version
aliases, continued --slice rejection, and exact before/after bytes plus imported geometry across
all 11 small canonical files. Byte equality also proves unchanged file hashes. Temporary inputs
are isolated and cleaned without assertions tied to machine-specific directory names; no global
working directory is changed by tests. Test paths are resolved from the authoritative manifest.
One new report-writer unit test checks propagation of BrokenPipe without panic.

The unchanged 188-test baseline plus 26 new tests passes **214 tests, 0 failed, 2 optional tests
ignored** on stable Linux/WSL. Native Windows will omit the single Unix-specific test; no current
Windows pass is claimed. Required format, strict all-target Clippy, workspace test/build,
warnings-denied public/private rustdoc and whitespace gates pass. Actual native Linux/WSL CLI
inspection commands and external SHA-256 checks supplement integration tests. Import/analysis
failures precede report writes; output-device errors may leave partial bytes but return failure.
Topology allocation failures are not artificially injected by this suite.

See [CLI.md](CLI.md) for the exact output/units/precision/empty-mesh/error/exit contract and interpretation
limits. At Closure B, native Windows validation of Closure A/B, explicit Closure C transform
round-trip coverage, and final milestone closure remained outstanding. Closure C coverage is recorded
below; completed bounded M2.4 campaigns remain historical evidence and are not rerun here.

## M2 Closure C: explicit transform round-trip regressions

Starting clean `main` at `22f74d28266fed062a81b616af90685b98c15a25` (committed Closure B),
`crates/resin-core/tests/transformations.rs` adds ten deterministic tests in `round_trips`:

- Positive/negative translation of non-origin dyadic points, with exact explicit reversal.
- X/Y/Z rotations at +pi/2 and -0.731 radians, with off-axis points and scalar trigonometric oracles.
- Combined extrinsic XYZ rotation, with reversed-axis inverse and an incorrect-order counterexample.
- XYZW quaternion normalization and conjugate reversal, using an independently derived rational
  matrix for `[1,2,3,4]/sqrt(30)` and the asymmetric mapping `(3,-2,5) -> (27/5,14/5,1)`.
- Positive uniform and non-uniform scales, one-/two-axis reflections, reciprocal reversal, and
  exact equality for bounded dyadic values scaled by powers of two.
- Origin/non-origin pivots, the pivot itself, and asymmetric points under mixed-axis rotation and
  non-uniform signed scale.
- Chronological affine `then()` chains with translation, rotation, non-uniform scale, and a local
  pivot. The analytic example `(5,1,-2) -> (7,-2,13)` recovers the source in the correct order;
  undoing scale before rotation instead yields `(14,-2,-2)`.
- Forward/reverse affine direction vectors, including exact independence from translation/pivot.
- Every original vertex of manifest-resolved FX-01 and FX-05, with independent forward/reverse
  checks and exact equality of complete imported source values and before/after file bytes.
  This preserves coordinates, triangle indices/winding, bounds, diagnostics, headers, units,
  facet normals and attributes. Byte equality implies unchanged hashes; external manifest SHA-256
  verification also passes for all 12 available canonical files before and after validation.
- Partial zero-scale collapse of two distinct points to one result, proving no unique reverse
  exists. No zero reciprocal or public inverse API is introduced.

The test-local mathematical inverse is `c + S^-1 * R^-1 * (world - t - c)`. Affine construction
undoes translation, subtracts pivot, undoes rotation, applies reciprocal scale, then restores pivot.
For `R = Rz*Ry*Rx`, inverse rotations apply negative Z, then negative Y, then negative X; merely
negating an XYZ Euler tuple is not the inverse in general. Forward object, flattened affine and
explicit affine chains are checked against independent scalar Rx/Ry/Rz equations. Separate inverse
scalar equations and analytic rational/quarter-turn cases prevent a circular forward/reverse oracle.

Round-trip comparisons use absolute plus relative budget
`128 * f64::EPSILON * (1 + max(working_magnitude, abs(expected_component)))` per component.
The absolute term is approximately 2.84e-14 mm for positions; the relative term includes independently
calculated working magnitudes to account for cancellation near zero. These short chains use
coordinate/offset/intermediate magnitudes below 100 mm, `abs(scale)` in `[0.5,4]` (condition ratio
at most eight), and reciprocal magnitudes at most two. The allowance covers several quaternion,
affine-composition and scalar evaluations; it is a bounded regression budget, not a universal error
proof or an engine epsilon. Direction comparisons use the analogous component-unit budget.
Quaternion component normalization uses eight epsilons. Exact bounded dyadic cases and unchanged
source data use exact equality; arbitrary trigonometric results do not require bitwise recovery.
Fixture recovery targets the actual promoted binary32 coordinates, not pre-serialization ideals:
inverse transforms cannot restore STL quantization loss. Very small nonzero scales can amplify
error and are deliberately outside these round-trip claims, as are extreme-range inverses.
Existing non-finite-input and arithmetic-overflow typed-error tests remain unchanged and pass.

Stable Rust/Cargo 1.99.0 on `x86_64-unknown-linux-gnu` under WSL passes all 43 focused transformation
tests, including the ten new tests, and **224 workspace tests, 0 failed, 2 optional tests ignored**
(214 unchanged baseline tests plus ten additions). Format application/check, strict all-target
Clippy, workspace tests/build, warnings-denied public/private rustdoc, and `git diff --check` pass.
Only this strategy, the roadmap coverage status, and transformation tests change; production source,
CLI/cancellation behavior, dependencies, fixtures, and existing tests are unchanged. No production
defect was discovered. Raw validation logs and provenance are outside Git at
`<evidence>/m2/m2-closure-c/20261015-22f74d2-LQAhwD`.
At the Closure C checkpoint, validation covered Linux/WSL but not native Windows, so M2 remained open;
the final native Windows validation of the same production baseline is recorded below. The earlier
M2.4-A result was not reused for Closures A–C.

## Final M2 milestone acceptance

M2 is **COMPLETE** at accepted production baseline
`d8816303bbc4c2ba26213645ddf278db6574342b`. This decision applies the original roadmap scope,
deliverables, acceptance criteria, automated-test expectations, and Definition of Done without
marking M1 complete or expanding M2 into slicing, repair, printability analysis, or application job
orchestration.

| Original M2 requirement | Implementation evidence | Automated test evidence | Documentation | Linux/WSL | Native Windows | Final classification |
| --- | --- | --- | --- | --- | --- | --- |
| Native-Rust immutable indexed mesh, explicit units and source bounds | `mesh::TriangleMesh`, `Point3`, `Aabb`, explicit `StlUnitPolicy`; no foreign kernel or `unsafe` | `mesh_geometry`, `reference_meshes`, STL documentation examples | Architecture M2.1 contract; TD-009/010 | PASS | PASS | PASS |
| Binary and ASCII STL import with correct known-file bounds/orientation | `mesh::stl` byte/reader/path APIs, deterministic detection and exact-coordinate indexing | `stl_import`, all small manifest fixtures, binary/ASCII CLI cases | Architecture STL grammar/detection/unit policy; fixture provenance | PASS | PASS | PASS |
| Malformed/resource-exhausting input fails safely with context | Structured `StlError`, checked arithmetic, byte/face/vertex/text limits and I/O causes | Every binary truncation prefix, corrupt/malformed ASCII/binary, configured caps, deterministic mutation sweep | Architecture limits and failure modes | PASS | PASS | PASS |
| Stable diagnostics without silently changing original geometry | Separate retained metadata/diagnostics and immutable source buffers | Degenerate, duplicate, normal/attribute, source-byte/index/winding preservation regressions | TD-011; architecture diagnostic boundaries | PASS | PASS | PASS |
| Explicit source-to-world transforms, finite/range validation and reversible cases | `Rotation3`, `ObjectTransform`, `Affine3`, point/direction mapping and transformed-vertex bounds | 43 transformation plus 15 world-bound tests; ten explicit round trips with independent oracles | TD-012; architecture transform convention and tolerance limits | PASS | PASS | PASS |
| Adjacency/topology metadata and deterministic boundary/non-manifold/winding/component/vertex findings | Bounded source-index incidence, face components and complete vertex-link analysis | 29 topology integration tests plus canonical CLI defect reports | TD-013; architecture topology contract and bounded claims | PASS | PASS | PASS |
| Cooperative cancellation with atomic public outcomes | Additive borrowed `Cancellation` entry points and typed checkpoint errors across import, construction, diagnostics, topology and bounds | 29 focused cancellation integration tests plus documentation example | TD-014; architecture cadence, precedence and blocking limitations | PASS | PASS (29/29 focused) | PASS |
| Usable read-only CLI inspection path | `resin-cli inspect --assume-mm <path>` over public import/topology APIs; fixed 0/1/2 exit policy | 25 Linux integration tests; actual canonical, malformed, path and deterministic-output cases | README report/error contract; architecture adapter boundary | PASS | PASS (24/24; one test is Unix-only) | PASS |
| Parser/topology robustness and property-style invariants | Deterministic parameter sweeps, permutation/repeat invariants, bounded mutation coverage and four isolated libFuzzer targets | 97 seed replays and 6,226,119 bounded coverage-guided executions with no reported failure | Fuzz protocol and M2.4-B results above | PASS for bounded initial campaign | N/A; Windows fuzzing was not an M2 acceptance requirement | PASS |
| Representative timing and memory evidence | Dependency-free Release worker/runner for read, import, topology and five bounds mappings | 32 timing workloads/320 batches; 33 RSS workloads/99 processes; helper and rejection checks | `PERFORMANCE-BASELINE.md` and reproduction protocol | PASS for initial baseline | N/A; no native Windows performance claim | PASS |
| Public API documentation, fixture provenance and known limitations | Documented public items and executable examples; owned fixture manifest/hashes; headless boundaries | Warnings-denied public/private rustdoc and fixture-integrity checks | Architecture, technology decisions, fixture research, README | PASS | PASS | PASS |
| Stable cross-platform quality gates | Same production commit exported without `.git` or build artifacts and built with native MSVC | Linux 224/0/2; Windows 223/0/2, with only `#[cfg(unix)]` non-Unicode-path test absent | This section and archived Windows report | PASS | PASS | PASS |
| Repair, if introduced, must be conservative and explicitly requested | No repair API exists and no operation silently repairs/discards/reinterprets input | Source preservation and defect-report tests prove read-only behavior | AGENTS policy; TD-011/013; README | N/A | N/A | DEFERRED WITH JUSTIFICATION: the roadmap did not require a repair deliverable; any future repair needs separate approval/specification |

The three gaps from the earlier closure review are closed: cooperative cancellation is implemented
and tested, CLI mesh inspection is usable and tested, and deterministic transform round trips cover
translation, rotation/quaternion conventions, signed non-uniform scale, pivots, affine order,
directions, canonical sources, and singular zero scale.

Final native Windows validation used Rust/Cargo 1.99.0 and MSVC target
`x86_64-pc-windows-msvc` against the exact accepted source commit. Formatting, strict Clippy,
workspace build/tests, warnings-denied rustdoc, 29 cancellation tests, 24 CLI tests, ten Closure C
round trips, seven representative native CLI inspections, and fixture/source integrity passed with
no Windows-specific defect. Windows ran **223 passed, 0 failed, 2 ignored**; Linux/WSL ran
**224 passed, 0 failed, 2 ignored**. The difference is solely the intentionally Unix-only
non-Unicode-path regression. The 211-entry external evidence index verifies with SHA-256
`1b3adb055adcecd842a5e3cbfd8fa1e4b31d0a540d556c9b1aab7ec6f085956a` under
`<evidence>/m2/m2-final-windows/20261015-d881630-9e7cb78b/`.

The initial four-target fuzzing campaign remains bounded evidence: 6,226,119 executions found no
reported crash, panic, timeout, sanitizer finding or invariant failure, but do not prove defect
absence. The performance record remains a Release WSL/Linux baseline of 32 timing workloads, 320
timed batches and 99 separate whole-process RSS measurements, not a target, exact heap attribution,
or Windows benchmark. Both campaigns retain their original source provenance and limitations.

The final documentation-only closure gate on 2026-10-15 passes stable format checking, strict
all-target Clippy, **224 workspace tests with 0 failed and 2 ignored**, workspace build,
warnings-denied public/private rustdoc, and `git diff --check` under WSL/Linux. It does not substitute
for or rerun the archived native Windows validation; production source remains the validated commit.

Deferred work remains visible: sustained and expanded fuzzing; retained-corpus regression monitoring;
self-intersection, outwardness, nesting, thickness and printability analysis; any automatic or manual
repair; progress reporting, asynchronous scheduling and strict cancellation-latency guarantees;
precise allocator/phase memory attribution; optional FX-11 execution on Windows; cold-I/O, 32-bit and
native Windows performance measurements; and optimization driven by future budgets. None is an
original unmet M2 acceptance criterion. M3 — Geometry and Slicing Engine is the next implementation
milestone, subject to its own research, design and authorization gates; this closure does not start it.



## TD-031 represented-coordinate M3 coverage

The [contract](M3-REPRESENTED-COORDINATE-CONTRACT.md) separates the requirements and the
implementation. Eight public integration regressions cover decimal nested ribbons
at seven quarter-spaced planes, unchanged legacy off-grid failure, deterministic complete owned
layers, overlapping source union with physical scale and atom provenance, unchanged strict contact
diagnostics, zero conversion work, vertex/ray/payload/work/pair/arrangement limits, cancellation and
fused failure. Independent scaled integer arrangements agree with represented construction at
binary exponents -1000, -8, 0 and 900. A 60-bit exponent-zero crossing case exercises rational
vertices/topology; nonfinite and extreme dynamic-range inputs fail atomically. A private bit-encoding
test covers signs, signed zero, subnormals, decimal significands, shift truncation and range limits.
Existing exact predicates, integer construction, strict-region and owned projection suites remain
regression coverage. See the M3 measurement protocol for seven-plane timing/RSS observations and
the fuzz protocol for the 2,014-execution bounded ASan region campaign. Current native Windows,
agreed-corpus and adopted preliminary-budget acceptance remain outstanding.


Final TD-031 stable Linux gates pass `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`
(**601 passed, 0 failed, 2 ignored**), `cargo build --workspace`, warnings-denied public/private
rustdoc and `git diff --check`. All eight new public regressions pass in Release. The isolated
fuzz workspace passes format/check offline; its helper-only test command contains zero unit tests
and is not counted as fuzz execution. Raw gate logs are `/tmp/resin-m3-td031-*`; the ASan campaign's
source snapshot and SHA-256 index are retained under `/tmp/resin-m3-represented-fuzz/source/`.
Native Windows was not run because this session has no registered WSL interop bridge.


## M3 native Windows acceptance at 382fb62

On 2026-10-17 the [native validation](M3-NATIVE-WINDOWS-VALIDATION.md) passes against exact commit
`382fb62`: Windows 11 Pro build 26200, stable Rust/Cargo 1.99.0, native MSVC host/target. Format,
strict Clippy, workspace tests/build, warnings-denied public/private rustdoc and seven focused
Release suites all exit zero. Workspace result is **600 passed, 0 failed, 2 ignored**; Linux's extra
test is Unix-only. Native SHA-256 verifies all 150 exported source files before/after execution.
No production change is needed. The absent visible WSLInterop registration was insufficient to
diagnose reachability: sandbox launch fails with a vsock permission error, but authorized execution
outside the sandbox works. PowerShell script-file policy rejects `.ps1` execution; direct commands
complete without policy changes. Historical OS error 4551 does not recur. This supersedes the native
pending status above for this implementation revision; corpus/budget/reference gates remain open.


## M3 closure corpus, budgets and final cross-platform validation

The [closure record](M3-CLOSURE-ACCEPTANCE.md) fixes the corpus, area/tessellation envelopes,
preliminary 5-second/64-MiB-retained/512-MiB-RSS budgets, shared-edge proof, raw evidence identity,
failed attempts and final results. New acceptance tests cover the complete FX family, including
in-memory generation of the ignored 327,680-triangle FX-11 fixture, and an exact equatorial
octahedron. TD-032 closes the valid opposite-side on-plane edge classification gap while keeping
same-side, winding, duplicate/nonmanifold and coordinate conflicts unresolved. Path accounting
retains both supports only for the named two-occurrence candidate.

Final stable Linux gates pass fmt, strict all-target Clippy, build, warnings-denied rustdoc and
**611 passed, 0 failed, 2 ignored**. Native MSVC completes the full **Release** workspace with
**610/0/2**, including all CLI/process/cancellation tests and doctests, plus ten focused suites.
Native fmt/Clippy/build/rustdoc pass; a final example-only resource-cap delta is independently
hashed and checked natively. Initial missing-FX-11 setup failures and subsequent Debug OS 4551
Code Integrity failures remain explicit; Debug is not marked green. Native functional acceptance
uses the complete Release suite with no filtering or policy change. No new production dependency,
unsafe code or foreign geometry is introduced.

Extended ASan runs pass 16,069 constructor, 9,750 region, 115,606 engine and 2,806,053 schedule
executions. Exact-final-source follow-ups pass 5,129 / 1,732 / 11,238 / 908,755, respectively;
all artifacts directories remain empty. Final ordinary replay passes 2,401 retained inputs.
These are bounded campaigns with leak detection unavailable, not proof of defect absence.
Large-input Release samples and twelve independent process-memory runs all pass predeclared budgets.
M3 is accepted within this corpus/domain; broader future monitoring, native Debug host remediation
and M1 commercial-default research stay separate from implemented M3 behavior.


## TD-033 Karasu identity and package migration

After accepted M3 commit `8872287`, the owner requests `karasu-slicer` identity, `karasu-cli` and
`karasu-core` packages/directories, and `karasu-<module>` for future crates. All implementation/
doctest imports, manifests/lockfiles, isolated fuzz helper/package and live profiling/preparation
scripts are migrated. The former `resin-cli` executable calls the same CLI implementation and is
not a duplicate package. A process regression compares help/default/version, valid STL inspection,
usage errors and processing errors through both executable names: status/stdout/stderr agree.
Branding intentionally changes; argument semantics and geometry/diagnostic structures do not.

Stable Linux passes fmt, strict all-target Clippy, workspace build, warnings-denied private/public
rustdoc and **612 passed, 0 failed, 2 ignored** workspace tests. All five Python profiling-runner
tests pass. The isolated fuzz workspace passes offline all-target check/format; stable-helper replay
passes all 2,401 retained M3 inputs (687 constructor, 66 region, 1,021 engine, 627 schedule). These are
ordinary replays, not a new coverage-guided campaign. An independent consumer builds/runs with
`resin-core = { package = "karasu-core", path = ... }`, proving compatibility of the documented old
Rust import spelling through a Cargo alias. A comparison of all 61 core Rust files finds only
requested identifier/product replacements and rustfmt whitespace/trailing-comma changes. The
reference generator reproduces the exact version-1 manifest and all 12 fixture SHA-256 values,
including FX-11; historical headers/dataset identifiers are deliberately unchanged.

Native Windows uses a separate 156-file snapshot, source ZIP SHA-256
`909b7f011223d0feed247e82a103a1940b6d82573127835722421ee0d59912a3`.
Native before/after/retry hashes pass. Format, strict Clippy, build, warnings-denied public/private
rustdoc and eight focused Release suites pass, including the M3 acceptance corpus. Completed CLI
groups also pass the new alias process check. **Full native suites are not PASS:** Debug cannot launch
`stl_import-91011af142231cff.exe`; Release and its unchanged retry cannot launch
`section_normalization-b11a8b2ac4d1dfe5.exe`, all OS error 4551 from Windows application control.
Isolated Release STL import passes; isolated Release normalization is blocked before execution.
No assertion failure, signing-policy change, executable alteration or security bypass is involved.
These environment failures are retained separately from successful checks. Prior M3 native acceptance
is not silently applied to the renamed application; complete native acceptance of this naming
revision remains pending an environment that permits all generated test executables.

Raw source exports, integrity/consumer/corpus checks, command logs and native results remain outside
Git in `/tmp/karasu-rename/` and the corresponding native temporary directory. Status documentation
added after native export is not represented as part of that tested snapshot. No commit or remote
change is implied by this refactor.

## M4 LCD raster acceptance

M4 adds analytic integration tests in `tests/raster.rs`, constructed-rational/lifecycle regressions
in `tests/raster_adversarial.rs`, and full owned-fixture replay in `tests/raster_corpus.rs` (FX11 is
generated in memory on clean checkouts). Internal tests exercise quantizers exhaustively over their
supported integer domains, half-open spans, compensation against a naive grayscale oracle, and
PNG CRC/framing/block boundaries, writer errors and cancellation before/after partial output.

All eight orientation combinations and all supported sampling/selected grayscale modes are checked
against a separate per-sample physical point oracle. Hole/island masks have explicit golden bytes.
Compensation tests compare separable extrema with a direct zero-exterior neighborhood implementation;
they include oversized and anisotropic radii. A regression rejects precision loss hidden by large
grid-origin cancellation, and grid/represented constructed rational outputs must agree.

`tools/m4-profile/validate_png.py` separately validates CRCs, chunk order, full zlib consumption,
filter bytes, decoded length and every sample of the default analytic reference export. Golden
hashes, benchmark raw records, final required checks and native evidence are documented in
[M4-VALIDATION.md](M4-VALIDATION.md). Benchmarks separate geometry, raster and PNG time, and report
requested payload separately from process RSS. No end-to-end physical printer or commercial-parity
claim follows from image tests. Re-run quality gates and relevant independent decodes after any
raster/PNG semantic change; update goldens only with a documented reason.

M4 final Linux acceptance is 645/0/2; complete native Windows Debug and the final complete
Release retry both pass 644/0/2. The first Release run encountered host application-control
4551 launching one preexisting M3 doctest. Its unchanged isolated retry and subsequent complete
workspace retry pass; initial failure evidence remains retained. All 33 focused M4 Release tests
also pass, and native PNG bytes match Linux at reference and large benchmark dimensions.
This establishes acceptance for the tested source without claiming a permanent signing-policy fix.

## M5 profile, job and machine export coverage

`profile_storage`, `print_jobs` and `print_export` integration suites cover strict JSON/revisions,
platform path rules, history/no-clobber updates, immutable settings, transitions, schedule/travel
bounds, capability rejection, explicit grayscale loss, cancellation, source failures and deterministic
publication. Private codec tests cover runs, checksums, offsets and scene connected components.
`tools/m5-validate` contains independent Python container/reference readers and corruption tests.
Public third-party readers provide additional CTB/GOO/Photon interoperability evidence.

The profile JSON fuzz target exercises hostile bounded JSON under ASan; it is not a machine-format
fuzzer. Timing/RSS cases include three fresh processes each for small, 2,000-layer and P1-resolution
jobs. Native Windows validation uses a hash-verified source snapshot and byte-compares completed
artifacts with Linux. Evidence and remaining physical-device limits are in [M5 validation](M5-VALIDATION.md).

### Repeatable native Windows runs

Use the [native Windows procedure](../tools/windows-native/README.md) and checked-in snapshot/runner
tools. They build on local Windows storage, invoke native MSVC Rust, preserve individual command
exit codes and source hashes, and collect validation-scoped Code Integrity events. Application-control
launch rejection is an environment failure, not a test assertion or a passing suite. Reproducibility
requires the complete Debug/Release workspace suites; retain failed attempts before unchanged retries.

## Optional commercial comparisons and mandatory correctness coverage

TD-036 reclassifies M1 EXP-03/04 as optional compatibility research. They are not milestone gates and
must not be marked passed without execution. This changes no automated test requirement: independently
specified first/last-layer placement, sample versus printer Z, exact contact/coplanar cases, invalid
meshes, structured diagnostics, determinism and resource bounds remain required. Future claims of
commercial-equivalent behavior need scoped evidence rather than inferred parity. Protocols and
historical records remain available for targeted comparisons.

## M5.5-A preparation contract validation

`tests/preparation.rs` independently decodes canonical header/source/affine bytes and checks ordered
parameter admission, malformed/range inputs, source/placement/sampling/model-prefix stale anchors,
support-only edits, fixed-world cuts, signed-zero identity and raw-direction replay. History tests
cover full retained payload/count bounds, cancellation at admission/records/publication, undo/redo,
branching and owner-local generation ABA protection. These tests evaluate metadata/history only;
they do not substitute for PREP-S01–S09's future geometric/interactive acceptance.

See [specification](M55-IMPLEMENTATION-SPEC.md) for independent cube/sphere/cut/support fixtures,
separate length/volume/topology/connectivity obligations and B–E resource budgets. Native evidence
uses the checked-in Windows snapshot procedure; outcomes remain in [validation](M55-VALIDATION.md).

## M5.5-B hollowing validation

`tests/hollowing.rs` checks actual cube/polyhedral-sphere shells, world-space placement and nonuniform
scale, source/exterior preservation, exact component/region counts, invalid/unsupported recipe and
geometry failures, scalar/resource boundaries, progress and atomic cancellation. A subdivided-sphere
regression exercises lower inclusion certificates with combined source planes at unchanged accuracy.
The M3 integration test requires an outer material region and its cavity hole.

Private numeric/hull/admission tests include 4096 independent i128 orient3d comparisons, floating
extremes, exact square-root bound verification, integer point clouds and coplanar contact/horizon
cases. `tools/m55b-validate` independently reads f64 geometry with Python Fraction arithmetic for
represented volume, enclosure, supporting-halfspace and manifold/component checks; its checks do not
claim to replace the full Rust inner-erosion proof. Predeclared repeated timing/RSS cases include
cube and20/80/320-face sphere tessellations. The bounded hollowing fuzz target exercises malformed
small sources, valid affine fixtures and result/source invariants under explicit caps.

Actual commands, source hashes, sanitizer limits and native Windows outcomes are recorded in
[M5.5-B validation](M55B-VALIDATION.md); no Linux result is labeled native Windows evidence.

## M5.5-C drainage validation

`tests/material_difference.rs` compares exact body-minus-cutters arrangements with independent
analytic areas and rectangle-cell occupancy, including overlap, contacts, missing bodies, cavity
parity, represented scaling and cancellation. `tests/layer_difference.rs` covers both coordinate
modes, explicit labels, unchanged strict diagnostics and typed fused failure.

`tests/drainage.rs` evaluates real drained material with independent area/occupancy checks for all
six cardinal directions, exact terminal-cap planes, hollow wall retention, immutable plan/source
identity, zero enclosed/opened cavities, edits/removal, misses, shallow/oversized/non-exterior cuts,
overlap, limits and cancellation. Private cylinder tests independently certify emitted rings,
translated error bounds, malicious proposals, outward winding and precision/segment failure.
The bounded drainage fuzz harness exercises valid and invalid recipes and one actual difference
layer under independent limits. The owned example feeds drained layers into M4 rasterization;
`tools/m55c-validate` checks resulting pixels against independent analytic bounds. Declared fixture
budgets, exact source provenance and native Windows outcomes belong in the validation record.

## M5.5-D parametric support validation

`tests/support_plan.rs` checks complete parameter identity, historical KPREP1 embedding, stale
anchors, shape/placement/semantic tolerance edits, resource caps, horizontal/descending station
paths and transactional cancellation/history. `tests/parametric_supports.rs` checks real geometry
and constructed-material areas/occupancy for solid, hollow and drained models; bases/shaft/tip,
independent profiles/orientations/bends, sharp tips, genuine horizontal side contacts, collisions,
penetration/miss failures, explicit overlap union, source/plan immutability, edit/remove and limits.
Private profile tests reject custom convexity/origin failures and rounded world-ring rank collapse.

`tests/prepared_composition.rs` independently checks `(body minus cutters) union additives`, including
absent body, cutter/support coincidence, parity, overlaps, contacts, source ordering/winding, dyadic
coordinates, budget/cancellation and grid/represented adapter modes. Existing union/difference
regressions run unchanged. The owned support reference example exports geometry, five PGM masks,
five reference PNGs and CTB/GOO/Photon/PP1 jobs. `tools/m55d-validate` independently checks exact
represented component convexity/closure/volume and all 400,000 analytic PGM/reference samples,
then decodes machine masks against the validated reference. These are synthetic uncalibrated format
fixtures, not printer or mechanical acceptance. Full combined preparation remains M5.5-E.

A bounded ASan target covers small recipes, profiles, contacts, cancellation/resources and one actual
prepared-material layer. Declared profiling budgets, final source hashes and complete Linux/native
Windows outcomes belong in [validation](M55D-VALIDATION.md). GUI and hardware evidence remain M6/M7.

## M5.5-E integration and bounded closure validation

Full Linux formatting, strict Clippy, build, Debug/Release **852/0/2**, and warnings-denied
public/private rustdoc pass. Hash-verified native Windows MSVC runs pass the same gates with
Debug/Release **851/0/2** (one Unix-only CLI test accounts for the difference). New suites cover
13 session, five progress, four combined and two stress tests. The combined owned source traverses
M2 import, hollow/drain/support preparation, M3/M4 and all four M5 formats; independent readers
validate exact represented geometry and 1,600,000 analytic pixels. Stress adds eight supports,
24 components and 80,000 independently checked pixels. Python reader tests pass 12/12.

Four ASan campaigns produce zero artifacts; all 195 retained inputs replay successfully. Three
fresh Release processes per fixture pass predeclared time/RSS budgets. Cross-platform outputs
match except measured timers and the expected PP1 OS metadata field. Native source integrity is
verified before/after execution. Later documentation/standalone reader additions and a cosmetic
standalone replay-tool format boundary are explicitly recorded, without implying changed production
was validated by an older snapshot. See [closure](M55-CLOSURE-ACCEPTANCE.md) for exact identities,
scenario applicability, campaign counts, evidence paths, unsupported cases and remaining M6/M7 gates.

Panel orientation (TD-104): a GOO export declares its stored image's mirroring in the header's
two mirror bytes for each combination of the profile's mirrors (without mirroring: Y). The
Saturn 4 Ultra 16K preset's orientation was checked by hand against ChiTuBox's and Lychee's
exports of the same model (layers decoded and compared at several heights); that comparison
needs the files and is not part of the automated suite.
