# Architecture

## Current naming

TD-033 names the product `karasu-slicer`, the application packages/directories `karasu-cli` and
`karasu-core`, and the core Rust import `karasu_core`. New crates use `karasu-<module>`. The former
CLI executable remains a compatibility entry point to the same implementation; no geometry API or
format changes accompany the rename. Historical paths and version-1 corpus identities stay stable.

## Architectural goals

karasu-slicer is a headless processing core with thin application adapters. The architecture favors
explicit data flow, testable transformations, deterministic output, bounded resource use, and clear
failure reporting. M0 creates only `karasu-core` and `karasu-cli`; the modules described below are
planned boundaries unless identified as implemented below. M2.1 implements source meshes and STL
import; M2.2 adds immutable scene transforms and tight world-space bounds; M2.3 adds derived source
topology and structural findings. M3.1 adds the local triangle–horizontal-plane kernel. M3.2-A adds
raw single-plane mesh traversal; M3.2-B adds provenance-aware incidence normalization and local
coplanar ownership. M3.3-A adds a conservative incidence graph over that borrowed normalization.
M3.3-B adds conservative ordered graph paths; M3.4-A adds detection-only represented-XY path embedding
inspection. M3.4-B measures locally simple cycles' exact traversal orientation and explicitly ranged
area without mutation. M3.5-A derives strict exact containment and `EvenOddStrict` material roles
only for non-contacting eligible cycles. M3.5-B adds explicit exact-dyadic uniform layer plans.
M3.6 composes the strict stages into owned atomic layer items, synchronous progress and a bounded
atomic collector. TD-030 adds exact source-material composition with explicit source ownership.
M4 adds the independent `raster` module for LCD projection, bounded coverage masks, compensation,
streaming, preview metadata and reference PNGs. Printer placement, GUI and machine-format print
pipelines remain later work.
M5.5 preparation is planned below, not implemented.
M2 is accepted at production baseline `d8816303bbc4c2ba26213645ddf278db6574342b` after stable
Linux/WSL and native Windows validation; M3 is accepted for its documented corpus and contracts; historical stage-specific pending
statements below are superseded by the M3 closure record.

## Initial workspace

### `karasu-core`

The reusable library will own domain data and processing logic. It must not depend on a CLI, GUI,
windowing system, or platform-specific presentation API. During M0 it exports only stable product
identity constants. M2.1 adds `geometry` and `mesh`, and M2.2 extends them with tested transforms and
transformed bounds. M2.3 adds `mesh::topology`, described below. Later modules require their first
working behavior.

### `karasu-cli`

The command-line executable is a headless application adapter. In M0 it provides help and version
information. Later it may orchestrate imports, slicing jobs, diagnostics, reference exports,
progress, and cancellation through public `karasu-core` interfaces.

M2 Closure B adds `karasu-cli inspect --assume-mm <path>` as a read-only adapter over public STL
import, mesh statistics/source bounds, import diagnostics and topology analysis. It completes both
processing stages before writing a report; the adapter owns only argument/error/presentation policy,
not parser or geometry algorithms. Bounded detail output retains full counts and source identifiers.
The syntax, numeric precision, empty-mesh behavior, deterministic text/ordering, diagnostic meanings,
and 0/1/2 exit contract are documented in [CLI.md](CLI.md). No CLI cancellation, repair, placement,
slicing, framework or dependency is introduced; core and presentation remain separate.

TD-043 adds `karasu-cli slice`, a synchronous adapter that composes public contracts only:
`import_stl_path`, `TriangleMesh::analyze_topology` (closed-surface gate unless explicitly waived),
an `ObjectTransform` translation with world bounds, `build_layer_schedule` at slab midpoints,
`LayerEngine` with represented coordinates and strict even-odd regions, `RasterStream` binary masks
with `OutsidePolicy::Reject`, `PrintJob` and `export_machine`/`export_reference`. The adapter owns
argument, placement-acceptance, error and summary policy; it adds no geometry algorithm, snapping,
repair or clipping. TD-044 adds the CLI's only external dependency, `ctrlc`, whose handler thread
sets an atomic flag read by every core `Cancellation` predicate; cancellation stays cooperative and
synchronous on the processing thread. Advisory progress (phases, completed masks) is written to
stderr by a CLI adapter and never alters stdout. See [CLI.md](CLI.md) for the user contract.

Since M6.1 (TD-070) that orchestration lives in `karasu-project`. The CLI parses the options into a
`SliceRequest`, renders progress, handles Ctrl-C and prints the summary the pipeline returns.

### `karasu-project` (M6.1, TD-070)

The headless orchestration shared by `karasu-cli` and `karasu-app`. It depends only on
`karasu-core` and owns no algorithm.
- **`request`:** `SliceRequest`, the validated description of one job: a model or a plate
  document, the profiles, the format and output, and every processing option.
- **`pipeline`:** `slice(request, observer, stop)`:
  - runs the job to a published, create-only output and returns its summary text, export
    summary and resin usage;
  - reports phases and layers to an `Observer`;
  - polls the caller's cancellation predicate at every core checkpoint and worker;
  - publishes nothing on failure.
- **`project` (TD-071):** the in-memory project model:
  - a library of embedded models identified by SHA-256;
  - plates, each with printer, resin, settings and objects, and a revision that every edit
    increments;
  - `request(plate)` builds the pipeline request over embedded data, and `freshness` says whether
    a result still describes its plate.
- **`jobs` (TD-071):** plate requests sliced on their own threads, with progress events,
  cancellation and the revision they were built from.
- **Supports follow their object (TD-072):** a support document bound to a pose moves rigidly
  with its object across moves on the plate and turns about the vertical axis; other pose
  changes are refused.
- **`library` (TD-077, TD-106):** the built-in catalog of printers, resins and print settings
  by vendor (`assets/catalog`, compiled in, read-only) and the user's library (`profiles/v2`):
  copies of catalog entries with the revision they came from, custom profiles, starting points,
  catalog updates offered, and the one-time conversion of a version-one library.
- **Plates and print settings (TD-106):** a plate holds a printer, a resin and a print setting;
  the setting composes the calibrated resin a job takes and supplies its anti-aliasing and
  compensation.
- **History (TD-081):** each plate keeps up to 200 undo steps; undo and redo restore earlier
  contents as new revisions, and merged edits (a drag) are one step.
- **Preparation previews (TD-082):** `pipeline::prepare` prepares every object as slicing does,
  without slicing, with one result per object and support findings kept for display;
  `jobs::PreparationJob` runs it in the background.
- **`diagnosis` (TD-078):** what is wrong with an imported model, read without changing it, and
  whether a plate's settings deal with each finding.
- **Reversed slivers (TD-079):** `reversed_tolerance_mm2` leaves a bounded negative-winding area
  per layer empty (`WindingRule::PositiveBounded`) and reports it; beyond the bound the layer
  fails with its position.
- **Errors (TD-078):** a `PipelineError` carries the command line's message, the reason without
  options, and the remedies, so an interface can word failures itself and offer fixes.
- **Still to come:** the project file (M6.6).

### `karasu-app` (M6.0, TD-068)

The desktop application crate runs eframe and egui on wgpu (TD-042).
- **What it has:**
  - the palette as semantic tokens (`theme`), with WCAG contrast checks in its tests;
  - a deterministic adapter choice (`gpu`): among adapters that can present, modern backends
    before OpenGL, then discrete, integrated, virtual, other and software devices, ties in
    enumeration order;
  - an explicit `--gpu-backend` choice;
  - a startup splash (`splash`, TD-069): the window opens borderless and transparent with the
    bundled artwork, the version and the progress of the real startup steps, then reconfigures
    itself into the decorated main window;
  - a `--smoke-test` mode for native validation;
  - **the 3D viewport (M6.2, TD-073):**
    - `workspace`: the open project and its plate, files opened by drop or command line,
      background imports, cached placements;
    - `viewport`: the wgpu renderer, offscreen and multisampled, with meshes uploaded once and
      placed by matrix;
    - `camera`: the orbit camera;
    - `gizmo` and `manipulate`: picking and the move, rotate and scale handles, which edit the
      plate through the project's revisions;
    - `window`: the workflow bar, the viewport and the status bar;
  - **the core workflow (M6.3, TD-077):** `profiles_ui` (printer and resin selection, basic
    editor) and `slicing_ui` (background slicing with progress and Cancel, export; since
    M6.5 its `panel` is the Slice and export tool and its `layers` the Layers tool, TD-103,
    showing each exported mask with the layer as the print job resolved it, which
    `SliceOutcome::layers` carries);
  - **repairs and messages (M6.3, TD-078):** `repair_ui` (the plate's repairs) and `messages`
    (the status line and the Messages window, with details and fixes);
  - **preparation (M6.4a, TD-081, TD-082):** `preparation_ui` prepares the plate in the
    background after a quiet pause and draws the current preparation; Undo and Redo;
  - **hollowing and drain holes (M6.4b, TD-083, TD-084; reworked in M6.4c, TD-089):**
    `hollowing_ui`, the selected object's preparation panel: the hollowing draft applied and
    removed as an operation, new holes placed as drafts and applied, from the outer surface or
    the cavity, and applied holes dragged and edited, each checked by the object's `HoleProbe`
    while the pointer moves. Holes are kept in the object's model frame, so they follow it;
    `preparation_ui` places them, tilts included, with the probe. Since M6.4e h4 (TD-093) the
    panel shows the guaranteed wall, the thickest spot and the passes, warns under 1 mm, and no
    longer offers cavity simplification. TD-094: it warns below the wall whose cavity is
    predicted to exceed the machine's memory budget (`Workspace::memory_budget`, half of the
    installed memory), and disables new holes when the prepared object cannot be indexed. The
    viewport draws a mesh too large for one GPU buffer in parts. Since M6.5 its submodule
    `hollowing_ui::panel` draws the Hollow and Drain holes tool panels, each a scrolling body and
    a fixed footer with the actions and the model's state, and reports applied changes as
    messages once prepared;
  - **viewport review (M6.4f):** `viewcube` (the view cube and home button), zoom at the
    pointer, the selection outline (a mask and a full-screen pass in `viewport`), surface
    filters for the parts of models outside the build volume; the viewport's colours are
    edited live in the settings dialog (`window/settings_dialog.rs`, TD-097), which replaced
    the temporary `colours_test` window;
  - **actions (M6.5, TD-097):** `actions` names what the user can do and maps keyboard
    shortcuts to it (`Keymap`); the window enables and performs every action in one place, for
    buttons, toggles, shortcuts and the view cube alike. User settings, shortcuts included,
    are to persist in the preferences root with the settings panel;
  - **layout (M6.5, TD-098):** `window` lays out a menu bar, the workflow bar (`workflow`:
    steps and their tools), a closable sidebar (the plate's printer and resin from
    `profiles_ui`, and `sidebar`'s list of objects), the viewport with the chosen tool's panel
    docked at its right (an egui area, not a window: `repair_panel` for Repair, the
    preparation parts of `hollowing_ui`, slicing from `slicing_ui` with the plate's slicing
    repairs from `repair_ui`), and the status bar. The repair of opened models is a File
    submenu until the settings panel.
    Gizmo handles belong to the Transform tool, drain holes to the Drain holes tool and repair
    markers to the Repair tool;
  - **notifications (M6.5, TD-099):** `toasts` shows every message pushed to `messages` as a
    toast at the viewport's bottom, timed by the message's `Lasting`, and the Messages window
    keeps them all;
  - **session logs (TD-096):** `karasu_project::logging` implements the `log` facade for the
    application and the command line: session files (the 10 newest, up to 20 MB each), a ring of
    recent records for a live window, a panic hook; the geometry core does not log;
  - **wall verification (TD-095):** `raster::verify` in the core measures printed walls,
    breaches and undrained pockets on layer masks alone; `pipeline::verification` feeds it the
    exported masks with the drain holes and cavities rasterized alone, behind
    `slice --verify-walls`;
  - **section (M6.4b3, TD-086):** `section_ui`, the panel that cuts the models between two
    layer heights; `viewport` clips with two planes and caps cut solids with a stencil parity
    count, and draws hole solids with depth plus a ghost where the model hides them;
  - **layer heights (M6.5, TD-102):** `layer_plan_ui` keeps the plate's `karasu-layers` plan
    (ranges set from the section bar, a provisional exposure table), checks a range before it
    is applied, and plans the plate's layers with the core's planner, on a thread of its own
    for adaptive ranges; `section_ui` snaps its handles to those layers, with typed fields,
    keys and the ranges' menu;
  - **hole openings (M6.4c, TD-089):** `viewport` cuts applied holes out of models, cavities and
    section caps by discarding fragments inside their cylinders (a data texture written when
    the holes change) and draws each bore only in material, by a stencil parity count;
  - **plate front (M6.4b4, TD-087):** `workspace::FrontTab` places the front tab and its word;
    `viewport` draws the word's glyphs on the plate from a copy of egui's font atlas;
    `slicing_ui::plate_view` turns layer masks to the plate seen from above;
  - **plate from below (M6.4b4, TD-088):** `viewport` fades the plate's surface and grid by a
    frame opacity, which `window` animates by the camera's side of the plate; the border, tab and
    volume edges stay. The supports' plate bases (pads and cones) and the rafts fade with it
    (owner R9, 2026-11-10): `supports_ui` draws them as ghosts of falling opacity, then leaves
    them out and stops picking them, so only the shafts and tips stand under the model; feet
    standing on the model stay (owner, 2026-11-10);
  - **localisation (M6.3b, TD-080):** `i18n` formats every interface text from the Fluent
    catalogs in `assets/locales` (`en-US` is the reference and the fallback), with a
    pseudo-locale; the headless crates and the command line stay in English.
- **What it does not have yet:** preparation tools (M6.4), layout and settings (M6.5), the
  project file and plate overview (M6.6). All algorithms stay in `karasu-core`, orchestrated by
  `karasu-project`.

## Planned responsibility boundaries

These are logical modules inside `karasu-core` unless scale or compile-time evidence later justifies
additional crates.

- **Mesh:** vertices, indexed faces, transformations, file import/export interfaces, topology
  metadata, and conservative diagnostics. Parsing reports malformed data; it does not silently
  change geometry.
- **Geometry:** vectors and transforms not adequately covered by reviewed dependencies, robust
  predicates, spatial indexes, intersections, polygon operations, Boolean operations, distance
  fields, and volumetric operations. APIs make tolerance and degeneracy policies visible.
- **Slicing:** layer-plane generation, mesh-wide use of the local geometry intersection kernel,
  segment normalization, contour reconstruction, region construction, and layer generation.
- **Raster:** conversion between physical and LCD coordinates, polygon coverage, grayscale masks,
  antialiasing, pixel compensation, and image production.
- **Printing:** printer/resin profiles, exposure and motion settings, layer metadata, complete print
  job models, and validation.
- **Export:** deterministic reference PNG/JSON output and isolated machine-format exporters behind
  explicit interfaces.
- **Model preparation (mandatory MVP, M5.5):** ordered non-destructive hollowing, manual drainage
  cuts and editable manual 3D supports; derived geometry/provenance, cavity/contact diagnostics,
  reproducible history and bounded execution. Headless core operations, not GUI-owned algorithms;
  this is a planned boundary, not an implemented or placeholder module.
- **Print analysis (TD-107, `analysis`):** where a placed model's material starts in mid-air:
  local minima (`analysis::minima`) and the print's unsupported regions on its layer masks
  (`analysis::unsupported`, islands and overhangs); the view tints overhangs by slope in its
  shader. The print's suction cups (TD-114, `analysis::suction`): resin a layer encloses while
  it is peeled, capped towards the plate and not vented by an opening of the minimum vent,
  found on the exported masks by sweeps of their empty pixels, whole and eroded. Orientation
  (TD-119, `analysis::orientation`): tilts sampled under the user's constraints, each scored
  from the mesh on every core (area facing the plate, lowest points, height, protected
  surfaces); the largest layer from oriented section segments; candidates ranked by normalized
  weighted measures. The app's Orient tool checks the best by slicing the model alone in each
  pose (islands, suction cups).
- **Advanced geometry (Post-MVP):** automatic supports, advanced island/overhang optimization,
  auto-orientation, advanced repair and generalized 3D Boolean workflows beyond required M5.5
  operations. Necessary MVP shell/intersection/subtraction or equivalent geometry is not deferred
  merely because the broader user-facing operation is advanced.
- **Application services:** job orchestration, progress events, cooperative cancellation, and error
  presentation. They coordinate domain modules without embedding UI policy in the core.

Data should flow in one direction:

```text
input files -> immutable source/scene -> preparation -> slice regions -> raster -> print job/export
                         |                    |              |                     |
                    diagnostics          diagnostics    diagnostics            validation
```

Expensive stages should eventually support streaming or bounded batches. Intermediate values must
have explicit ownership; caches require measured benefit, bounded lifetime, and deterministic
invalidation.

## Planned M5.5 preparation architecture — mandatory expanded MVP

TD-020 and PREP-001–009 establish the owner-approved scope. **Not implemented:** no preparation
algorithm, representation, dependency, project-file format or GUI framework is selected here.
M5.5 follows M5 in engineering order, but preparation feeds M3 at runtime. Existing M2 acceptance,
M3.1/3.2 contracts, raw source buffers and unresolved M1/M3 gates are unchanged.

Conceptual enabled-operation order, not a frozen Rust API or compulsory operation on every object:

```text
Immutable Source Mesh
  -> Explicit Transform/Placement
  -> Parametric Hollowing
  -> Manual Drainage Operations
  -> Manual Support Geometry
  -> Prepared Scene/Geometry
  -> M3 Slicing -> M4 Rasterization -> M5 Export
```

An object may deliberately omit a preparation stage (for example a solid model with manual supports).
The operation record must preserve enabled stages and their order; hollow/cut/support operations are
not generally commutative. M5.5-A defines supported ordering and upstream-edit invalidation rather
than claiming arbitrary reorderings are equivalent. M6 consumes headless operations; engine
validation in M5.5-E cannot depend on the future GUI being implemented.

### Sources, revisions and coordinate frames

Retain source mesh/STL identity and hashes, named object/preparation/world/platform frames and
explicit placement transforms. Derivation creates new geometry/components with source-asset,
operation and revision provenance; new faces/vertices need their own identity mapping, not reuse of
M2 source IDs as though topology were unchanged. Non-destructive reversibility means disabling,
removing or editing history and recomputing from source/earlier revisions, not mathematical inversion
of a shell cut or singular transform. No in-place automatic repair is implied.

M5.5-A must define the evaluation space for wall thickness and hole/support dimensions in mm,
orientation/pivot conventions and contact/hole anchor semantics. The conceptual transform-first
order does not decide an internal baking API. In particular, non-uniform scaling after shell
construction changes thickness; upstream transform changes must trigger the specified recomputation
or an explicit validated change in geometry meaning, never a silently stale thickness claim.
Prepared and source bounds remain separate. Moving a model must follow documented support-to-model
and support-to-platform relationships; stale contacts/cuts require explicit diagnostics or an
explicitly specified rebind operation, not proximity guessing.

A reproducible history records source identity, operation IDs/order/parameters, frames, algorithm/
schema versions, approximation settings and deterministic seeds if used. Derived caches are keyed by
the full relevant revision and invalidated on edits; cache use must not alter results. In-memory
history/replay is mandatory. Saved-project integration is conditional on a persistence mechanism:
if available, it must retain these settings and source references with version/missing-asset errors;
this revision does not select a project format or mandate an otherwise absent persistence subsystem.

### Geometry, validity and material combination

M5.5-A compares offset/shell, voxel, signed-distance and Boolean/equivalent strategies against
supported geometry, approximation error, topology, resource use and native-Rust dependency policy.
Define prerequisites for open, non-manifold, self-intersecting or indeterminate input and reject or
diagnose unsupported cases. Existing M2 closedness/local winding is not a global embedding/solid
proof. Required validity and cut/connectivity checks belong to MVP even when advanced repair is
deferred; unsupported input must not be silently repaired to make an algorithm appear successful.

Hollowing must retain thickness/boundary and cavity diagnostics. Drainage must distinguish a geometric
opening that reaches the intended cavity from a miss, shallow/failed cut, insufficient clearance or
enclosed/possibly undrained region; unknown connectivity is not affirmative drainage. Support geometry
needs explicit tip/shaft/base and model/contact/platform relationships. Intended contact/penetration
is different from unintended collision or a disconnected tip/base. None certifies mechanical strength,
fluid drainage/venting or universal printability.

The prepared-scene contract must define material/void/component roles and how model/support geometry
is combined for M3. A verified 3D union or separate components with independently validated material
union at the slicing boundary are alternatives to evaluate, not choices made here. Mere overlapping
meshes are insufficient: joint slicing must avoid duplicated exposure, parity-created voids or lost
supports while preserving intended cavities and drainage. Restricting generalized user-facing 3D
Booleans to Post-MVP cannot defer geometry operations essential to MVP hollowing/cuts/combination.
No new interpretation is imposed on M3.1 or the raw/normalized M3.2 incidence contracts.

### Error budgets, execution and application handoff

Define per-operation numeric/approximation/measurement tolerances and composed preparation-to-slice/
raster error budgets in named units before implementation. Resolution-dependent construction must
report its error/resolution settings; do not replace the existing no-global-epsilon policy or use a
display/physical pixel allowance as an unexplained geometry tolerance. Canonical geometry, history
and diagnostics follow existing determinism requirements; tolerances do not excuse unexplained
serialized differences. Native Windows and Linux evidence remains mandatory.

Budget source/intermediate/output geometry, field/grid storage if selected, diagnostics and concurrent
live revisions; use checked sizes and fallible allocation. Resource caps are not a whole-process RSS
promise. Plan named-stage progress separately from cooperative work checkpoints, cancellation at
entry/long phases/final publication, and documented indivisible phases. A cancelled/failed revision
never becomes the successful current prepared result or final export. Preserve source and prior
completed snapshots distinctly and allow retry; these contracts extend rather than replace TD-014.

M6 owns interaction/background scheduling, selection and presentation, not geometric truth. It must
support cavity/hole/support previews, diagnostic overlays, add/edit/remove, revision-aware recompute,
responsive progress/cancellation and slices of the current prepared revision. A background completion
must not overwrite newer edits; an old preview must be labeled stale, not current. M5.5-E supplies
headless integration through M3/M4/M5, and M6/M7 test the full interactive workflow on both platforms.
Physical-print claims require separately authorized trials with recorded printer/resin/process
conditions; geometry alone does not validate drainage, support strength or exposure success.

These additions increase shell/offset, connectivity, intersection/Boolean, support-contact, memory/
performance, editing/replay and cross-platform risk. No completion estimate, automatic support/island
feature, broad repair workflow, algorithm library or implementation is authorized by this document.

## Geometric conventions

### Units and coordinate spaces

- Millimeters are the canonical physical unit for model, scene, printer-volume, and layer geometry.
- A right-handed model/world coordinate system is used: positive X points right, positive Y points
  away from the front of the build plate, and positive Z points upward from the plate.
- The build plate lies on `Z = 0`. Its origin is initially the front-left corner. Printer profiles
  may describe a different machine origin, but conversion must be explicit at the raster/export
  boundary rather than changing world geometry implicitly.
- Mesh importers preserve source coordinates and units when known. A source-to-world transform
  records unit conversion, axis conversion, rotation, scale, and translation. Unknown units are an
  error or an explicit user choice, never an invisible guess.
- Layer planes are world-space planes at explicit Z heights in millimeters.

The build-plate origin and Y direction are initial conventions subject to validation against real
printer formats in M1 and M5. Any revision requires a recorded migration decision and regression
fixtures.

### Numeric representations

- Principal 3D calculations use `f64`.
- Mathematical geometry is the ideal continuous model; computed geometry carries finite-precision
  error and a stated tolerance policy.
- Polygon algorithms may use scaled integer coordinates when their range and quantization error are
  derived from job dimensions. Conversion metadata must retain the scale and rounding rule.
- The mesh Boolean kernel (`boolean`, TD-140) snaps its inputs once to a fixed global grid of
  2⁻¹⁵ mm (ties to even, at most 0.015 µm per axis, coordinates within ±2,000 mm) and decides
  everything in fixed-width integers: planes with integer coefficients, vertices as exact
  homogeneous meeting points of three planes, with proved bit bounds and no floating point in
  any decision. Its result lies on the grid, exact in `f64` and, within ±512 mm, in `f32`: the
  points it made are rounded to the grid, and the crossings rounding makes are resolved in
  rounds that move the vertices near them to the centres of 15.6 µm cells (at most 7.8 µm per
  axis); what remains is counted, and a result that is not closed is refused (phase 5).
- LCD coordinates are discrete pixel indices plus, when needed, continuous subpixel coordinates.
  Pixel pitch and physical origin come from the printer profile.
- Raster masks use explicitly declared dimensions, row order, bit depth, and transfer/coverage
  meaning. Exporters own machine-specific flips or rotations.

### Tolerances and robustness

There is no universal epsilon. Exact comparisons use zero tolerance where that is the specified
contract. Each approximate comparison must use a tolerance with a physical or numerical
meaning, such as an explicitly selected import welding distance, plane-classification band,
contour endpoint snapping distance, or integer quantization step. Tolerances must be:

1. named by purpose;
2. expressed in the relevant unit;
3. derived from input scale, machine resolution, or error analysis;
4. passed through context/configuration where behavior may vary; and
5. included in regression-test descriptions.

Robust predicates should use exact/adaptive methods where classification correctness demands them.
An approximate construction may follow a robust classification, but the distinction must be
documented.

### Orientation, winding, and contours

- Indexed triangle vertex order defines the face normal by the right-hand rule.
- Local winding consistency does not establish outward-facing normals: a closed surface may be
  globally inward, and geometric embedding/nesting requires separate validation. Import preserves
  winding; topology separately reports local inconsistency without reversing faces.
- Slice regions use counterclockwise outer rings and clockwise hole rings when viewed from positive
  Z toward the build plate.
- Closed contours are topological rings without a repeated terminal vertex in the canonical in-memory
  representation.
- Open contours remain explicit diagnostic results. They are not automatically closed unless a
  caller selects a documented repair operation with bounded criteria.
- Duplicate, zero-length, collinear, coplanar, tangent, and vertex-on-plane cases require explicit
  policies in M3. Output must not depend on hash-map randomization or thread scheduling.

### Invalid and ambiguous geometry

Import and analysis distinguish malformed files, non-finite coordinates, out-of-range values,
degenerate faces, duplicate faces, boundary edges, non-manifold edges/vertices, inconsistent winding,
self-intersection, and disconnected components. The system should return useful partial diagnostics
when safe, but must not label a mesh printable without completing required validation.

Repair is separate from detection. Each repair operation will report what changed and allow the
original mesh to remain available. Ambiguity that can change material topology requires user or
policy input.

## Determinism and concurrency

Given identical normalized input, profile, configuration, and software version, serialized outputs
must be byte-identical unless a format contains an explicitly controlled timestamp or identifier.
Canonical ordering and stable tie-breakers are required. Parallel algorithms must merge results in a
defined order and may not expose nondeterministic floating-point reduction order.

Cancellation is cooperative at bounded intervals. A cancelled stage must not publish a complete
artifact. Temporary output should be finalized atomically where platform APIs permit.

## Errors and API design

Libraries return typed errors with stage, entity, and underlying cause. Recoverable input failures
are not panics. Internal invariant failures may panic only when the invariant is local, documented,
and tested. Batch diagnostics should retain all safely discoverable issues rather than fail after the
first independent defect.

Public processing APIs should accept explicit immutable inputs and configuration and return owned or
clearly borrowed results. Interfaces should be introduced alongside implementations and executable
tests, not predeclared for speculative features.

## Implemented M3.1 local section kernel

TD-015–017 freeze the first local primitive in
[M3-TRIANGLE-PLANE-CONTRACT.md](M3-TRIANGLE-PLANE-CONTRACT.md), which owns the full truth table,
numerical derivation, event/provenance definitions, API and deterministic test matrix. Its design-time
status text is retained unchanged; this section and the roadmap record implementation status.
`geometry::section::intersect_triangle_z([Point3; 3], plane_z)` accepts three immutable
finite points and an explicit finite horizontal plane in the same coordinate space. It requires
constant work/stack storage and no mesh, allocation, dependency, cancellation/progress callback or
resource configuration. The public module is introduced alongside its working implementation/tests.

Exact represented-f64 comparisons decide Below/On/Above without a subtraction or classification
epsilon. Checked small-weight convex construction supplies approximate XY with an explicit error
bound and typed range/parameter rejections. Exact on-plane source coordinates are reused, Z is
assigned from the plane, and returned numerical zeros are normalized. Source corner/edge identities
remain recoverable after canonical endpoint ordering.

The raw event distinguishes empty intersection, tangent point, segment, coplanar edge and coplanar
triangle. Coincident represented endpoints remain a separate explicit condition, with both origins
retained. Repeated-position flags and qualified computed-cross observations preserve source
degeneracy context. No event certifies nonzero area or exact affine rank. In particular, a raw
coplanar triangle may have point or line support and is not a filled face or region.

Callers may obtain the three points through existing M2 checked transforms; the kernel neither
applies transforms nor changes original mesh buffers or metadata. Earlier transform error and
rounding are distinct from section construction error. Mesh traversal will attach global source IDs
and own cooperative cancellation. M3.2 must independently specify shared-edge ownership,
suppression, deduplication and normalization; contours, polygon containment/scale and rasterization
remain later boundaries. EXP-03/04 are still pending for their layer-placement and compatibility
questions. M3.1 implementation does not imply whole-M3 readiness or completion. Native Windows
validation of this kernel subsequently passed at `5f12ebcdf6b75366479ddf974555d6afa1feda24`:
256 passed, zero failed, two ignored, plus 28 focused tests in Debug and Release and four private
kernel tests. M2 Windows results were not reused; the Unix-only CLI test explains WSL's extra pass.

The implementation uses fixed arrays and in-place bounded sorting. Coordinates are normalized only
after corner/axis/plane validation. Active crossings are ordered by their source geometric edge keys
before construction; returned endpoints use coordinate/provenance ordering. Source-area observation
uses coordinate-ordered independently scaled edges; its range failure remains context even when an
exact event succeeds. Error variants distinguish invalid vertex/plane inputs, required-distance
overflow, unsupported normal-positive parameters, and guarded coordinate overflow. Coordinate
overflow is unreachable under the specified sign branches/arithmetic assumptions; the private guard
is tested directly rather than inventing an invalid public reproducer.

Required arithmetic remains separate `f64` operations without `mul_add`, fast-math or reassociation.
The coordinate-wise bound is `64 * 2^-53 * M + 16 * 2^-1074` mm, where M is the maximum absolute
source endpoint coordinate on that axis. It covers successful constructions only, not every finite
input, exact collinearity, relative accuracy near zero, or correctness under a caller-modified
floating-point environment. In particular, two exact-real coincident intersections can round to
distinct positions, and distinct exact intersections can round together; both source origins survive.

## Implemented M3.2-A raw single-plane mesh sections

### API, coordinate space and immutable ownership

`slicing::section_mesh_z(&TriangleMesh, plane_z, SectionTransform, SectionLimits)` returns a complete
owned `MeshSection`. The additive `section_mesh_z_with_cancellation` takes the same arguments followed
by `&mut Cancellation`. There is no implicit unit/plane/transform selection. Input geometry is an
immutable borrowed mesh in mm; the plane names world-space Z in mm, finite even for empty input.
Returned plane and point zeros are normalized to +0; no other coordinate normalization is applied.

`SectionTransform::Identity` interprets original coordinates as world coordinates without arithmetic.
`Object(&ObjectTransform)` calls the M2 direct pivot API; `Affine(&Affine3)` calls the checked M2
affine API. Neither path converts to the other or retries errors. Object order remains
`t + c + R*(S*(p-c))`; affine composition/shear remains explicit. Arbitrary finite negative/singular
maps are permitted by those APIs; no determinant computation, inverse, normal conversion or winding
repair is performed. Numerical collapse remains visible. Kernel context is observed on the actual
computed world points, not source-space rank or an exact-real transformed triangle.

### Reference scan and result provenance

Every source face is visited in ascending buffer order. Resolve its three validated `u32` vertex
indices in original corner order, compute the three world points, and call the unchanged M3.1 kernel.
There is no source-Z, normal, topology or approximate candidate filter, even for off-plane faces;
unused vertex-buffer entries are not processed. A future accelerated implementation must prove
equal raw output/completeness and specify error/cancellation ordering against this reference scan.
No transformed-vertex cache, copied source mesh, spatial index or diagnostic-detail scratch exists.

The result exposes only immutable `events()` and value `summary()`/`plane_z()` accessors. Each
`FaceSection` retains a source face ID, its three original source indices and the full unchanged
`TrianglePlaneIntersection`. This compact mapping avoids duplicating an independently mapped event
tree: `source_vertex(CornerId)`, `source_edge(LocalEdge)` and `endpoint_origin(EndpointOrigin)` resolve
every local identity into indexed mesh incidence. `MeshEndpointOrigin` retains both local identity
and the mesh index/pair. Keep the owning face ID to distinguish separate uses of a shared feature.
IDs name the input mesh, not a persistent asset namespace. The result does not retain source ownership;
callers needing to dereference IDs must retain the corresponding source asset.

Source edges use directed pairs 01/12/20, including `[v,v]`; they are not ordinary topology `EdgeKey`s
or future connectivity/ownership keys. Distinct corners sharing one index still have distinct local
identities. Distinct indices at identical coordinates remain distinct. All non-None events survive,
including duplicates, reversed faces, tangent points, coincident segments and rank-lost coplanar
triangles. No positive-length/area, closed-contour, solid or printable classification follows.

Ordering is source-face order followed by the original kernel payload order. Promoted vertex IDs do
not re-sort tied endpoints or corners. None events alone are omitted; `non_intersecting_faces` counts
them, and totals include all faces' four qualified area observations, even for omitted events.
`source_faces = non_intersecting_faces + retained_events` on success, and exactly one area observation
is counted per scanned face. No face is silently excluded as degenerate or non-manifold.

### Work/storage limits, errors and cancellation

`SectionLimits` is independent of topology caps: default `max_faces = 1_000_000`,
`max_events = 1_000_000`, `max_event_bytes = 256 MiB`. These are configurable safety defaults,
not a measured peak-memory target or a geometric tolerance. Face cap applies before traversal,
including duplicate/degenerate/no-hit faces. Event/byte caps apply on emission; zero event storage
permits empty/no-hit results. Count increments and all event byte/capacity calculations are checked,
including `isize` allocation range. Geometric fallible `try_reserve_exact` growth is requested only
when an emitted event needs storage, bounded by the smaller of faces, event cap and byte cap divided
by record size. No worst-case upfront allocation is performed. Summary uses fixed-size counters only.

Work is O(F) checked point/kernel evaluations plus amortized O(S) buffer growth for F faces and S
non-None events. Requested output capacity is O(S), scratch is O(1). Byte limits bound requested
payload capacity using the current `size_of::<FaceSection>()`, not a stable layout/serialization.
Allocator over-allocation, metadata, transient old/new buffers during reallocation, retained source,
and process memory are excluded. No hard RSS bound or allocation-failure injection is claimed.

`SectionError` distinguishes cancellation, non-finite plane, resource kind/limit, size overflow,
allocation failure, transform failure and local kernel failure. Transform errors carry first source
face/corner/vertex and preserve `TransformError`; intersection errors carry face/index triple and
unchanged `TrianglePlaneError` so local failing edges resolve to source indices. `Error::source`
preserves transform, kernel and cancellation causes. No private coordinates are logged, no numerical
fallback occurs, and no successful partial section escapes any failure.

Checkpoints extend the existing non-exhaustive enum: `SectionStart` before validation/limits,
`SectionFaces` before faces 0,256,512,..., `SectionStorage` immediately before/after each event-vector
growth and `SectionComplete` before publication, including empty/no-hit results. Observed cancellation
wins there. Otherwise precedence is finite plane, face cap, source-face order, original corner-order
transform, M3.1 error precedence, totals and output storage limits/reservation. Ordinary errors before
the next poll remain ordinary errors; no error-path or post-publication poll is added.
Allocations/reallocation copies, bounded individual transforms/kernel calls and cleanup cannot be
interrupted internally. No mesh-wide sorting phase exists. These are deterministic synchronous work
checkpoints, not progress events, stable release-to-release poll counts or real-time latency bounds.
No thread, async runtime, dependency or unrelated cancellation controller is added.

### Deliberately deferred interpretation

TD-018 authorizes raw traversal only. M3.2-B must decide shared-edge/coplanar ownership, multiplicity
normalization, degeneracy/ambiguity handling and downstream identity before contours (M3.3). No
coordinate binning, proximity repair or vendor connectivity routine is reproduced. The previously
recorded commercial comparisons motivate retaining provenance/multiplicity and a full-scan reference;
they do not choose an ownership or quantization policy. EXP-03/04 remain open, respectively for final
layer placement and tangent/coplanar compatibility. No layer sequence, polygon fill, integer scale,
rasterization, progress reporting or CLI slicing command belongs to M3.2-A. Native Windows validation
of the new traversal was pending at this implementation checkpoint; it subsequently passed in the
[consolidated M3.2/M3.3 campaign](TEST-STRATEGY.md#subsequent-consolidated-native-windows-m32m33-validation),
not by reuse of the earlier M3.1 Windows record.

## Implemented M3.2-B normalized incidence view

### Input contract, identities and observations

`slicing::normalize::{normalize_section, normalize_section_with_cancellation}` borrows a complete
M3.2-A `MeshSection`, explicit `NormalizationLimits` and optionally the existing `Cancellation`.
`NormalizedSection<'a>::raw()` returns that same immutable input by reference; no raw occurrence is
changed, erased or copied into a replacement mesh. Source identities are scoped to that raw input's
mesh namespace; combining independent assets requires caller-owned asset identity, not coordinate keys.
Preflight validates retained count/summary arithmetic, ascending in-range face IDs, event/side/local
origin consistency and finite on-plane point payloads. This is not an external raw-data deserializer;
the complete source scan and source-index validation remain the M3.2-A/M2 guarantees.

`SourceFeature::Vertex(v)` identifies an exact on-plane indexed corner; `Edge([min(a,b),max(a,b)])`
identifies the strict source-edge crossing even when its represented position rounds onto a vertex
or another feature. Reflections, singularity and geometric coincidence do not change source identity.
Every `EndpointObservation` retains normalized world coordinates, raw event index and local origin.
Repeated local corners sharing one mesh index remain separate observations. Feature groups retain all
observation IDs and either `CoordinateAgreement::Agreed(point)` or `Conflict`; conflicting features
have no chosen coordinate representative. Exact-coordinate groups are independent and expose the
number of distinct source features at one point. Close coordinates never share a coordinate group.
Neither identity grouping nor exact coincidence is proximity repair or a global overlap test.

### Segment occurrences, ordering and multiplicity

Raw segments and coplanar edges contribute one segment each; coplanar triangles contribute all three
original directed edges with their three corner observations reused between edges. Tangent points
remain observations with zero segment uses, not fabricated segments. `ContributionKind` distinguishes
these classes; the original raw record supplies face/corner/edge, sides and coincidence/area context.
`SegmentOccurrence` endpoint IDs are ordered by source feature (Vertex before Edge, then index/pair).
The `reversed` field describes reversal of the incoming payload, not a surface winding inferred from
M3.1's coordinate-canonical crossing output. Coplanar source winding comes from raw local edge IDs.

Flat index arrays give feature, coordinate and segment groups. Feature and segment groups are
lexicographic source keys with original occurrence order as tie-breaker; coordinate groups use
normalized finite X/Y/Z, feature, then original observation ID. Group ranges reference explicit index
arrays, whose entries reference flat observations/segments. Every range length/occurrence is retained.
Opposite uses, duplicate faces, zero length and disconnected coincident geometry never undergo parity
cancellation. No hash iteration or per-occurrence heap allocation is used. Summary includes tangent/
coplanar input, features/observations/segments/groups, conflicts, exact coincidence, internal edges and
unresolved groups. Raw segment-end counts on features expose branch incidence but do not certify a
manifold neighborhood or contour degree after future ownership interpretation.

### Conservative local coplanar ownership

`Ownership::CrossingCandidate` means one ordinary noncollapsed crossing without conflicting or
degenerate/unassessed context; it is not a closed-contour assertion. A coplanar edge becomes
`CoplanarPatchInternal` only when all these invariants hold:

1. Exactly two edge occurrences from two distinct nonduplicate indexed coplanar triangles; no extra
   coplanar/noncoplanar incidence on that source edge in the complete raw scan.
2. Shared source vertex coordinates agree, and each triangle's vertex features have no coordinate
   conflicts. Source edge and third indices are distinct; repeated-index/coordinate collapse fails.
3. Source-directed edge uses oppose (local winding consistency).
4. Exact XY determinants are nonzero and the two third vertices lie on strictly opposite sides of
   the shared represented edge. Their closed triangles then overlap only on that edge.

Both occurrences remain in all buffers; this class alone is not a local patch-boundary candidate.
One exactly verified nondegenerate coplanar contribution yields `CoplanarPatchBoundary`, meaning
exterior to that local raw patch, not the boundary of filled material. Two same-side triangles,
same/opposite-winding duplicates, winding conflict, more than two incident faces, repeated/collapsed
geometry or finite coordinate conflict remain `Unresolved`. Subsequent TD-032 admits verified
noncoplanar on-plane crossings; TD-039 admits the narrowly verified coplanar-cap/attached-side
perimeter case described below. Other mixed incidences remain unresolved; no filled-solid
certificate is inferred. `IncidenceIssues::contains(IncidenceIssue)` exposes independent structured reasons.
For two-contribution groups duplicate source triples are explicitly flagged; high-degree groups expose
all provenance and non-manifold/multiplicity findings without claiming an exhaustive pairwise defect list.

The private exact sign uses finite binary64 values as integers times `2^-1074`, not a geometry grid.
Coordinates use at most 2098 magnitude bits, differences 2099, products 4198 and determinant 4199;
132 base-2^32 limbs (4224 bits) suffice. Signed add/subtract and schoolbook products use bounded `u64`
carry arithmetic and fixed stack arrays. No floating subtraction/product, adaptive dependency,
quantization, physical scale or M3.1 predicate replacement is introduced. Exact coplanar rank can thus
be independently certified despite a zero/unavailable raw computed-cross observation, which remains
unchanged. This is correctness-first, not a performance or general exact-geometry library claim.

### Limits, errors and cancellation

Defaults are one million raw events, three million observations and independently three million
segment occurrences, and 512 MiB aggregate requested payload capacity. Preflight checks counts,
`usize` multiplication/addition and `isize` allocation sizes before eight fallible flat-buffer
reservations (observations, feature order/groups, coordinate order/groups, segments and segment
order/groups). Group buffer capacities are conservative P/S upper bounds. Limits leave existing
M3.2-A struct literals unchanged and do not imply RSS caps: borrowed raw geometry, allocator
metadata/over-allocation, exact-predicate stack and runtime memory are excluded.

For E events, P<=3E observations and S<=3E segment occurrences, time is
O(E+(P+S)log(P+S)), including binary feature lookups, with O(P+S) storage and bounded exact-sign
scratch. There is no all-pairs overlap scan or high-degree pair expansion. Sorting is in-place
unstable with complete deterministic ties; global segment overlap/intersection remains unassessed.

`NormalizationError` distinguishes cancellation, resource kind/limit, size overflow, allocation,
inconsistent provenance and invalid numerical observations (nonfinite/off-plane). Finite coordinate
inconsistency is instead `CoordinateAgreement::Conflict` and a coordinate-conflict issue on affected
segment groups; ambiguous representable geometry is `Ownership::Unresolved`. Both retain complete
observations rather than returning a false successful boundary or applying repair.

Additive checkpoints are `NormalizationStart`, `NormalizationEvents` (both preflight and extraction,
every 256 events), `NormalizationStorage` (before/after eight nonempty reservations),
`NormalizationSorting` (before/after three index sorts), `NormalizationGroups` (including long group
inner scans/classification) and `NormalizationComplete`. Empty input also checks entry/final.
Observed cancellation wins at the checkpoint; ordinary errors before the next poll remain ordinary.
No successful partial result escapes. Allocations, individual sorts, bounded exact signs and drop
cannot be interrupted inside. Counts are build-specific work checkpoints, not progress percentages,
cross-release contracts or strict wall-clock bounds. No new concurrency or dependency is introduced.

TD-019 completes the scoped normalization/ownership implementation, not commercial compatibility,
M3.2 milestone closure or M3.3 contour policy. At this normalization checkpoint, branch/routing and
open/ambiguous-path handling were deferred to M3.3 below; geometric orientation, filling, repair and
layer sampling remain absent, and EXP-04 remains open. Combined native Windows M3.2-A/B validation
was pending at that checkpoint and subsequently passed in the consolidated campaign linked above;
it is not inferred from WSL.

## Implemented M3.3-A deterministic contour incidence graph

### API, identity and non-destructive ownership

`slicing::graph::build_incidence_graph(&NormalizedSection, GraphLimits)` and its additive
`build_incidence_graph_with_cancellation(..., &mut Cancellation)` return an immutable
`IncidenceGraph<'normalized, 'raw>`. `normalized()` returns the same input, which retains the raw
section unchanged. No source mesh is rescanned or copied. Public slice/value accessors expose the
result; invalid adjacency/occurrence query IDs return None. TD-021 records this graph-only decision.

One vertex per normalized feature includes tangent-only isolated observations. Vertex ID equals
feature-group index, ordered Vertex(index) before Edge(sorted index pair). An actual On source
corner is Vertex provenance, not a distinct edge-interior endpoint: TD-019 validation permits Edge
only for strict opposite-side crossings. A strict crossing rounded to the coordinate of a source
vertex is still Edge provenance, not equivalent. Reversed local source-edge directions share the
unordered identity. Coincident disconnected features and singular-transform collapse remain distinct.
No coordinate bins, epsilon, tolerance band, welding or automatic XY intersection splitting exists.

Conflicting feature observations retain `CoordinateAgreement::Conflict` with no chosen point. Their
groups remain unresolved and excluded from candidate adjacency; all source positions/origins remain
inspectable. Source IDs name this input asset; arbitrary independent meshes cannot be concatenated
without an explicit caller-owned asset namespace. IDs are not allocation addresses or serialized IDs.

### Complete versus candidate incidence and degree semantics

`edges()` contains one canonical record per normalized feature-pair group, with endpoint vertex IDs,
group reference, multiplicity and eligibility. `occurrences(edge)` borrows every normalized segment
ID in original occurrence order. Segment observation IDs recover raw event, source face, original
local origin/contribution and winding. Parallel/duplicate/reversed/collapsed occurrences are grouped
for indexing only, never deleted or parity-cancelled; source index triples and all issues survive.

`complete_adjacency(vertex)` contains both endpoint slots for every incident group, including
self-loops, verified internal coplanar edges and unresolved groups. `candidate_adjacency(vertex)`
contains only CrossingCandidate and CoplanarPatchBoundary groups under unchanged TD-019. Neither is
a filled-boundary guarantee. The sole local internal exclusion is the committed verified patch rule;
no unresolved ownership is guessed or promoted. Candidate degree two with unresolved incidences is
not certified connectivity for an accepted contour. Degree data and unresolved-edge diagnostics must
be considered together by M3.3-B; no current `is_valid_contour` shortcut exists.

`VertexDegrees` distinguishes unique complete group IDs (a self-loop once), unique eligible group
IDs, eligible incidence ends (a loop twice), raw occurrence ends (each loop occurrence twice), and
unique unresolved group IDs. Raw multiplicity is not degree or occurrence parity. A repeated source
index may therefore give one self-loop and a parallel multi-occurrence group without a candidate.
Potential endpoint/branch diagnostics use candidate end degree one/>two, not raw multiplicity.
Tangent-only vertices have zero complete incidence and do not fabricate edges.

Five flat buffers store vertex/edge records, complete/candidate endpoint entries and diagnostics.
Adjacency is sorted by `(vertex ID, edge ID, endpoint slot)`; private per-vertex contiguous ranges
provide CSR-like access without per-incidence allocation. Edge order equals normalized group order.
No hash iteration or encounter-selected coordinate representative is involved. Diagnostics have
three canonical phases: coordinate-conflict vertices, unresolved edges with unchanged issue bits,
then degree-one/branch vertices. Summaries count groups, occurrences, entries, uncertainty, isolation,
potential endpoints/branches and requested bytes, not connected components, paths or filled regions.

### Limits, complexity and cancellation

GraphLimits defaults: 3,000,000 vertices, groups and independently borrowed occurrence references;
12,000,000 aggregate complete-plus-candidate adjacency entries; 9,000,000 actual diagnostics;
512 MiB aggregate requested buffer payload. Preflight counts groups/eligibility without expanding
raw occurrences. Reserve exact requested V vertices, G edges, 2G complete ends, 2C candidate ends,
and min(2V+U, diagnostic cap) diagnostic slots, where U is unresolved groups. Actual diagnostic caps
are checked before insertion, so a low cap does not reject a graph merely because its upper bound
is larger. Aggregate requested bytes include spare diagnostic capacity and no other heap scratch.
All count/byte/capacity arithmetic is checked, including isize range; reservations are fallible.

For V features, G groups, C candidate groups, A=2G+2C entries and D diagnostics, time is
O(V + G log(V+1) + A log(A+1) + D), storage O(V+G+A+D) with diagnostic capacity O(V+G).
Sorted normalized keys are reused; endpoint lookup is binary search, not sorting/copying raw S
occurrences. S occurrence references remain borrowed and separately capped. Retained source/raw/
normalization, allocator metadata/over-allocation, stack and destruction costs are excluded from the
byte budget. No benchmark, performance advantage, worst-case RSS or stable struct layout is claimed.

GraphError distinguishes Cancelled (underlying cause preserved), LimitExceeded(resource, limit),
SizeOverflow and Allocation. Finite numerical coordinate conflict and geometric ambiguity instead
survive as different structured diagnostic variants. Privately constructed immutable normalized input
already supplies validated provenance and finite on-plane coordinates; graph construction does not
invent an alternate normalization/deserialization error policy. The endpoint feature lookup uses a
tested private-constructor invariant; it is not a recoverable malformed-input API.

GraphStart checks before limits. GraphRecords polls input preflight and vertex/edge assembly;
GraphAdjacency polls emission, per-vertex indexing and high-degree inner loops; GraphDiagnostics
polls all diagnostic passes, every 256 records from zero. GraphStorage checks immediately before/after
each nonempty reservation, GraphSorting before/after each of two in-place sorts, and GraphComplete
before publication even for empty input. Cancellation wins when observed; earlier ordinary failures
remain ordinary failures. No partial successful graph escapes; retry preserves source and normalized
input. Allocations, individual sorts and drop are indivisible, without threads/async/progress or a
strict latency guarantee. At this graph-only checkpoint, path interpretation was deferred to M3.3-B
below; geometric validity/orientation remains separate. Then-pending M3.2-A/B and M3.3-A native
validation subsequently passed in the consolidated campaign. M1/EXP-03/04 and remaining M3 gates
stay open, and neither M3.3 nor M3 is declared complete.

## Implemented M3.3-B conservative deterministic contour paths

This section supersedes earlier stage-scoped deferrals of path extraction, not their geometry
contracts or recorded implementation-stage results. The subsequent consolidated native validation
is recorded separately below.

### API, taxonomy and provenance

`slicing::paths::{extract_paths, extract_paths_with_cancellation}` accepts the immutable graph,
`PathLimits` and optionally the existing cancellation controller. `SectionPaths<'graph,'normalized,
'raw>` borrows the same graph transitively through normalization/raw events. No mesh rescan, graph
reconstruction, new ownership policy, source points or per-path raw occurrence copies are introduced.
`paths()` returns canonical metadata; `path_vertices(id)`/`path_edges(id)` give flat index slices;
`components()`, `edge_outcome(group)`, `diagnostics()` and `summary()` expose accounting and uncertainty.
Invalid query IDs return None. Result-local graph IDs remain tied to the original source asset.

`OpenChain` means maximal clean graph continuity between degree-one endpoints. `ClosedCycleCandidate`
means a clean degree-two cycle with at least three distinct graph vertices, not a validated simple
XY polygon. `AmbiguousTrail` means continuity was stopped at a branch or unresolved/conflicting
neighborhood; its `closed` flag can still record an actual graph return to its start. Closed walks
omit the repeated terminal vertex (vertex count = edge count); open trails retain both endpoints
(vertex count = edge count + 1) and explicit canonical endpoint IDs. Graph closure, graph-simple cycle
candidacy and geometric filled-boundary validity are deliberately distinct.

Each complete graph group has one EdgeOutcome: Path {path, offset}, InternalCoplanar, or Unresolved.
Every candidate group occurs in exactly one path; none is duplicated to close a trail. Complete-only
groups remain inspectable through outcomes and the borrowed graph, preserving all raw multiplicity,
original source faces, local origins, directed contributions and coordinate observations. A grouped
parallel/duplicate incidence is not multiple ordinary path edges. Self-loops/collapse/non-manifold
groups remain unresolved under unchanged TD-019. Current candidate groups are unique nonloop
feature pairs with one occurrence; separately parallel/self-loop candidates would contradict that
contract and are rejected as inconsistent rather than silently promoted into contour routing.

### Conservative decomposition and canonical identity direction

Candidate components are discovered by bounded DFS over existing candidate adjacency only; IDs are
ordered by minimum graph vertex. Isolated tangent and complete-only vertices do not fabricate path
components. Component records carry member/edge/path counts and graph issue flags, not region data.
Complete-only unresolved groups never connect separate candidate components.

Continuation requires degree exactly two and no unresolved neighbor or coordinate conflict. At any
other vertex, seed a trail for each unconsumed incident candidate and stop at the next boundary.
This preserves T/crossroad arms and branch-connected cycles without selecting a continuation through
a junction. Scan remaining unconsumed groups only after all boundaries; those lie on wholly clean
degree-two components. Each traversed group is marked once. Per-path vertex stamps detect an invalid
revisit other than actual return to the start; malformed adjacency is a typed error, not a loop.
An unresolved neighbor stops even degree-two continuity; candidate paths are not called verified
contours just because the narrower view looks clean. Conflicts never acquire a representative point.

Open paths start at the smaller endpoint vertex ID and reverse both index sequences if necessary.
Closed walks rotate both sequences to their unique minimum vertex and choose the direction with the
smaller first edge ID; when reversing, keep the first vertex and reverse remaining vertices/all edges.
This minimizes the cyclic sequence of (vertex, outgoing edge) keys for these graph-simple trails.
Path records sort by (first vertex, first edge), a unique total key because edges are consumed once;
outcomes are then remapped to final path IDs/offsets. No growing front insertion or area calculation
exists. A closed ambiguous trail's canonical start can differ from its routing boundary; its issue
flags and vertex diagnostics still expose that boundary and do not imply resolved continuation.

Canonical direction is not clockwise/counterclockwise, outward normal, interior/exterior or a region
winding policy. Bow-tie/overlapping/zero-area XY embeddings can still be provisional graph candidates.
M3.4 must specify planar validity, intersections/overlaps, geometric orientation, integer conversion,
nesting/holes and material interpretation. No polygon repair, splitting, closing, Boolean, layer,
raster or model-preparation operation is supplied by this stage. EXP-03/04 remain open.

The subsequent owner-approved [M3.4-A design](M3-PLANAR-VALIDITY-DESIGN.md), supported by
scoped static research, separates graph continuity,
per-path embedding, cross-path compatibility and filled material. TD-023 explicitly permits the
detection-only represented-XY stage before integer conversion. It does not modify earlier mathematical
contracts or satisfy the polygon-grid gate. No grid, area accumulator, material/repair rule, region
or layer policy is selected.

### Limits, errors, complexity and cancellation

PathLimits defaults: 3,000,000 input vertices/groups, paths, edge references and components;
6,000,000 vertex references and actual diagnostics; 512 MiB aggregate requested capacity. Input caps
bound inspection, including complete-only data. Output caps apply to actual counts, not conservative
worst-case reservations. With V vertices, G complete groups and C candidates, reserve up to C path
records, 2C vertex references, C edge references, min(V,C) components and V+G diagnostics, each output
capacity intersected with its count cap; also G outcome records, V vertex states and V stack entries.
The requested-byte sum covers all eight buffers, including unused capacity and scratch later released.
Checked usize/isize arithmetic and fallible reservation precede construction. Empty input permits all
zero caps. Resource limits exclude borrowed graph/raw/normalization, allocator overhead/over-allocation,
stack locals and process memory; they are not whole-process peak-RSS or latency bounds.

For V vertices, G complete groups, A adjacency entries, C candidates, P paths and D new diagnostics,
time is O(V+G+A+C+P log(P+1)+D), requested storage O(V+G+C+P+D). Consistency inspection and DFS/trails
are linear; canonical scans/rotations/reversals sum to O(C+P), and unique-key record sorting costs
O(P log(P+1)). No raw-occurrence expansion, all-pairs geometry, per-path heap object or benchmarked
speedup is claimed. Path/component/outcome/diagnostic records and index sequences are immutable after
publication; DFS/visit state is scratch. Native results must be validated, not inferred from Linux.

PathError distinguishes Cancelled (with source), LimitExceeded(resource,limit), SizeOverflow,
Allocation and InvalidGraph(vertex,edge). Defensive input inspection checks IDs, local eligibility/
group correspondence, multiplicity, adjacency order/endpoints/degree totals and aggregate counts.
It consumes privately built graphs, not an external malformed-graph deserializer; internal range
construction remains the graph's invariant. Missing/duplicate candidate consumption or invalid revisits
cannot become successful partial output. Finite coordinate conflicts and representable topology
uncertainty instead remain PathIssue flags and ordered diagnostics; original graph issues survive.

PathsStart precedes caps. PathsInput covers inspection/initialization; PathsComponents covers DFS and
high-degree neighbors; PathsTraversal covers boundaries, inner walks and final accounting;
PathsCanonicalization covers min-key scans and both sides of bulk rotation/reversal phases;
PathsStorage brackets each nonempty reservation; PathsSorting brackets the path-record sort;
PathsDiagnostics covers vertex/group/component passes; PathsComplete precedes final publication even
when empty. Record loops poll every 256 iterations from zero. Cancellation wins where observed,
earlier ordinary errors remain ordinary, and no partial success escapes. Allocations, individual bulk
rotation/reversal phases, sort and drop are indivisible; no runtime, thread, progress or preemption
contract is added. M3.3-A/B and M3.2-A/B consolidated native Windows validation subsequently passed
at committed HEAD `c4865117e18d998fe80d48d8fac1022b5731ebc8`; the
[validation record](TEST-STRATEGY.md#subsequent-consolidated-native-windows-m32m33-validation)
distinguishes the owner-supplied Release transcript and reported exit from machine-captured checks.
This does not close M3.3 or M3. M1 and expanded M5.5/M6/M7 commitments remain unchanged.

## Implemented M3.4-A represented-coordinate path embedding inspection

`slicing::validity::{inspect_planar_paths, inspect_planar_paths_with_cancellation}` borrow existing
immutable `SectionPaths`; their `PlanarInspection` retains upstream graph/normalization/raw context.
`PathEdgeRef` is a canonical path ID and edge offset, not a constructed intersection point.
`PathEmbedding` describes local SimpleCycle/NonSimpleCycle/Ineligible status plus collapse and
forbidden-self-pair counts. All open/ambiguous paths remain ineligible; their contacts are still
examined. Cross-path findings do not change individual simplicity or assign containment/material.

Finite agreed world-space endpoint XY values are compared exactly as represented. The existing
fixed-limb orientation sign moves unchanged to shared private `slicing::exact`. Closed coordinate
interval tests and conservative AABB rejection require no epsilon, subtraction, grid or new point.
Diagnostics distinguish proper crossing, endpoint/endpoint, endpoint/interior, collinear singleton,
positive overlap, coincidence and collapsed contacts, retaining directed endpoint roles. Adjacent
edges are exempt only for their prescribed singleton graph endpoint, including graph-closed wrap;
backtracking/overlap is never exempt. Different feature IDs at equal XY are never welded.

Collapse findings precede pair findings; both use path/offset lexicographic order. All E*(E-1)/2
unordered pairs count, even disjoint/AABB-rejected/allowed-adjacent pairs. Input vertices/groups,
paths, vertex/edge references, pair work, actual diagnostics and requested bytes have independent
`ValidityLimits`. Defaults and detailed accounting are in Section 10 of the design. Checked pair
work precedes allocation; buffers use checked usize/isize byte arithmetic and fallible reservations.
Diagnostic growth is clipped to actual-count and byte caps, never silently truncated. Requested
capacity includes scratch and excludes input, stack, allocator over-allocation/overhead and RSS.
For V graph vertices, G groups, P paths, E edges and D findings: O(V+G+P+E^2) work and O(P+E+D)
requested storage. This is a bounded reference, not a measured large-scene performance claim.

The result explicitly counts unassessed complete-only internal/unresolved groups and isolated
vertices. Original diagnostics, multiplicity and source feature/face provenance remain borrowed.
No report certifies all section geometry or exposes `valid_material`/`printable`. Exact predicates
on rounded endpoints cannot restore ideal pre-rounding source geometry.

ValidityError distinguishes cancellation (with source), count/work/byte limits, overflow, allocation
and inconsistent path/group references with available context. Entry, bounded input/inner-pair loops,
every finding, reservation boundaries and final publication poll TD-014; observed cancellation or
failure publishes no successful partial report. Fixed-size predicates, allocations and destruction
remain indivisible. No thread, dependency, progress API or strict latency guarantee is introduced.
Area/orientation, integer construction, nesting, regions, EXP-03 scheduling and remaining M3/M1
acceptance remain outstanding. Platform evidence is separate in the test strategy.

## Implemented M3.4-B cycle area and traversal orientation

The owner approved the [area/orientation contract](M3-AREA-ORIENTATION-CONTRACT.md) and TD-024 after
the scoped research. `slicing::area` exposes ordinary and
cancellation-aware `measure_cycle_areas` functions. `SectionAreas` borrows `PlanarInspection` and
returns one outcome per original path ID. Only locally SimpleCycle paths are measured; open,
ambiguous and non-simple paths are explicitly skipped. All diagnostics/source links, cross-path
contacts and complete-only uncertainty remain borrowed, not interpreted as material.

A separate private 134-u32-limb magnitude plus sign exactly sums represented-dyadic shoelace
products for checked per-cycle N<=u64::MAX. Existing orientation arithmetic is unchanged. Sign
defines current canonical traversal, not source winding or outer/hole role. Magnitude is rounded
once from integer bits, nearest-even, to positive finite mm^2, RoundedToZero or Overflow. The last
two retain exact orientation; they are complete representation outcomes, not zero/infinity scalars,
warning-only loss or operational errors. Exact-zero SimpleCycle area is a structured consistency
failure. Exact encoding is not physical quantization or ideal geometry before upstream rounding.

AreaLimits cap all input paths/vertex references/edge references (including skipped), assessed
closing terms and requested output bytes. Defaults are 3M/6M/3M/3M and 512 MiB. Checked usize/isize
and u64-domain preflight precedes one fallible flat outcome reservation. No coordinate copy or
contact rescan occurs. Cost is O(P+134*T), O(P) requested output and fixed arithmetic stack, not
measured throughput/RSS. Borrowed input, fixed stack and allocator overhead/over-allocation are
excluded from requested bytes.

TD-014 AreaStart/Input/Storage/Terms/Rounding/Records/Complete checkpoints poll bounded loops
every 256 from zero; term indices continue across cycles. Reservation and per-cycle conversion have
checks on both sides; final publication checks empty/all-skipped input. Fixed arithmetic, allocation
and cleanup are indivisible, without latency/progress guarantees. Typed errors distinguish sourced
cancellation, limits, overflow, allocation and path/vertex-contextual consistency. No successful
partial report, mutation, dependency or thread is introduced. Physical grid, constructions,
nesting/regions, final layer scheduling and M1/M3 gates remain open.

## Implemented M3.4-C ExactOnly coordinate view

The [integer-grid contract](M3-INTEGER-GRID-CONTRACT.md), supported by
scoped comparative research, was committed at `75acecd`
before the owner-authorized implementation. `slicing::grid` now provides that limited API.
TD-025's owner-approved direction uses explicit per-job binary step/origin/domain metadata and an
ExactOnly coordinate view over the borrowed planar inspection. Exact equality preserves converted
represented positions without choosing a positive loss allowance; off-grid/range input fails
atomically. Retain original feature identity, unresolved conflicts and all borrowed provenance;
coordinate equality must not weld features or confer polygon/material validity.

The implementation provides a validated private-field BinaryGridSpec, a borrowed
ExactGridView, one exact-or-unresolved outcome per graph vertex and ordinary/cancellable entry
points. Exact division uses a private two-array 66-limb sign/magnitude helper; nonzero residual
precedes range rejection. All agreed vertices, including complete-only/isolated points, are checked.
Default caps are 3M vertices/6M agreed-axis conversions/512 MiB requested outcome payload. One flat
fallible reservation, deterministic preflight/error order and GridStart/Input/Storage/Records/
Coordinates/Complete checkpoints prevent successful partial publication. Fixed per-axis arithmetic
and allocation are indivisible; requested bytes exclude borrowed storage, fixed stack and allocator
overhead. No pair or path-walk scan and no new integer predicate/area/inverse API is included.

This fixes a zero-displacement coordinate contract, not a chosen mm step or rational construction
proof. Actual Linux/native acceptance is recorded separately in the test strategy. No inverse
decoder, pair audit, repair, region, layer policy, dependency or printer limit is added. Lossy
conversion and construction/material gates, EXP-03/04, M1 and remaining M3 stay open. Existing
M2–M3.4-B ownership/numerical contracts are intact.

## M3 completion boundary after M3.4-C

The completion research and
[independent specification](M3-COMPLETION-SPECIFICATION.md) are a non-authorizing design checkpoint.
They preserve the existing borrowed provenance pipeline but identify an ownership boundary before a
public layer iterator: temporary raw/normalized/graph/path/inspection/area/grid values cannot form a
self-referential owned layer result. A future layer engine should therefore convert reviewed stage
outputs into one owned final `RegionLayer` per plane while retaining source IDs and exact-grid identity.

The first implemented region stage is deliberately nonconstructive. The
[M3.5-A contract](M3-REGION-HIERARCHY-CONTRACT.md) and TD-026 accept only matching exact-grid
`SimpleCycle` inputs with measured nonzero orientation and no cross-path contact. The implementation
builds an exact integer containment forest and applies `EvenOddStrict`: even depths are material
outers and direct odd-depth children are holes. Orientation controls only a nonmutating
forward/reverse boundary view; it does not infer material. Tangents, internal patch edges,
unresolved groups, open paths and contacts retain separate outcomes. No source mesh or upstream
result is modified.

The direct reference classifier uses inclusive integer AABBs, a positive-X half-open-Y ray and exact
signed `i128` side products within TD-025's `|q| <= 2^52` domain. It has separate caps for path,
candidate vertex, pair, ray-edge, relation, boundary, region, diagnostic and requested-capacity
work. Its synchronous TD-014 checkpoints are `RegionsStart`, `RegionsInput`, `RegionsStorage`,
`RegionsBounds`, `RegionsPairs`, `RegionsForest`, `RegionsOutput` and `RegionsComplete`.
Operational failure/cancellation yields no hierarchy; representable ambiguity yields a complete
`GeometricallyUnresolved` result instead of a repair or a printable-layer claim.

General overlay remains a distinct architecture gate. M2 does not establish an embedded outward
solid and M3.3 traversal is identity-canonical, so overlapping source components cannot be assigned
union/parity semantics implicitly. Any constructor must specify source-material ownership, fill rule,
exact rational growth, coincident-edge policy, constructed provenance and output conversion. TD-025's
156-bit reduced-numerator counterexample excludes an unreviewed i128-only implementation.

Layer scheduling is independent of geometry. Implemented TD-027 `slicing::schedule` requires a
validated explicit first plane, positive finite height, `Count`/`Before`/`Through` and caller-selected
layer/payload caps. Exact dyadic terminal membership precedes a single nearest-even binary64
publication rounding. All published planes are finite and strictly increasing with contiguous u64
indices. Overflow or duplicate rounded planes reject the entire plan, with index context.

The [schedule contract](M3-LAYER-SCHEDULE-CONTRACT.md) proves 34-u64-limb capacity for the complete
finite-input/u64 domain. At most 65 exact probes derive a terminal count before any allocation;
an exact integer recurrence then emits an owned immutable plan in O(34*(64+N)) time and O(N)
requested output. This avoids floating accumulation and repeated exact multiplication per output,
without a measured speed claim. Host storage limits do not redefine the mathematical index domain.
`ScheduleStart`, `ScheduleCount`, `ScheduleStorage`, `SchedulePlanes` and `ScheduleComplete` implement
atomic TD-014 cancellation. The result is a plane plan, not layer geometry or a job controller.

EXP-03 remains necessary for compatibility defaults. Construction remains independently gated.

### Implemented M3.6 owned layer composition

TD-029 `slicing::engine::LayerEngine` borrows immutable mesh/transform/schedule and composes section,
normalization, graph, paths, validity, area, exact-grid and strict-regions functions unchanged except
for the corrected TD-026 half-open ray predicate described below. Explicit `LayerConfig` includes
grid/fill interpretation, all stage limits and an independent projection payload cap. Each successful
item owns its raw events and flat provenance/geometry/diagnostic data; no self reference survives.
Derived adjacency and coordinate-sort lookups are omitted without dropping their underlying data.

`StreamState` distinguishes unstarted, running, normally exhausted and failed streams. A typed error
retains plane context and the original stage cause, then fuses iteration. Earlier items stay valid,
but only advancing past the last item and passing `LayersComplete` establishes complete generation.
`LayerProgress` reports exact monotonic completed-layer counts for `M3LayerGeneration`, plus current
index and lifecycle kind; it is independent of the cancellation predicate. Observer panic is outside
the contract. No proprietary percentage weighting or stop-latency guarantee is assumed.

`collect_layers` accepts a fresh engine and independent count/aggregate payload caps. Header storage
is checked/reserved once; generated payload is charged before retention. Completion is deferred
until final exhaustion and `CollectionComplete`; failure returns no successful partial collection.
Per-layer unresolved geometry remains explicit even in a complete collection. Output payload counts
retained raw vector capacity and requested flat capacities; collection additionally counts headers.
Allocator overhead, source/schedule memory and temporary overlapping upstream reports are excluded.
This bounds requested retained data, not process RSS or elapsed time.

The [contract](M3-LAYER-ENGINE-CONTRACT.md) documents nine additional cancellation checkpoints,
ownership, ordering, diagnostics and failure precedence. Work remains serial and demand-driven,
with one pipeline's working data per iteration; caller-retained layers or collections add memory.
No spatial accelerator, cache or parallel framework is selected without measured budgets.

The M3.6 analytic nested-layer corpus found that TD-026's implementation omitted the strict upper-Y
bound when selecting ray-crossing edges. It now enforces the original half-open straddle on both
endpoints, preserving nesting under collinear subdivision and either traversal direction. This is a
correction to the accepted mathematical contract, not a new fill/repair policy.

### Implemented M3.5-C exact constructive material

TD-030 `slicing::construct` accepts individually simple closed integer rings within TD-025's domain,
with explicit source-group IDs. Even-odd within groups and union across groups distinguish cavities
from nested independently filled objects. An original-segment arrangement constructs homogeneous
rational intersections, splits all contacts/overlaps, retains every atomic support and classifies
left/right material with exact symbolic infinitesimals. Eight-u64-limb checked arithmetic covers
all intermediates without a new dependency. No f64 reconstruction or physical offset is introduced.

Material-left boundary walks are decomposed into edge-disjoint cycles with shared contact vertices;
exact local turns and symbolic geometric-interior containment determine roles/parents. All atoms
retain side outcomes and source path/edge supports, including internal/cancelled context. This is a
distinct constructed representation, not a relabeling of strict non-contacting boundaries. The
[contract](M3-CONSTRUCTIVE-MATERIAL-CONTRACT.md) proves the bounds and documents work/capacity limits,
ordering, contact topology and atomic cancellation. Defensive topology failures are invariant errors.

The engine's explicit face-material map requires one ID per original source face and consistent
ownership across each eligible path's raw occurrences. It rejects unresolved source groups,
open/ambiguous/non-simple paths and mixed ownership, preserving upstream diagnostics. Strict callers
remain unchanged. Successful construction is attached separately to the owned layer before
`LayerComplete`, with checked extra retained payload charged to per-layer and collection limits.
Temporary adapter scratch is independently charged against constructor capacity. Rational grid
coordinates retain their explicit grid metadata for future raster interpretation.

The [audit](M3-COMPLETION-AUDIT.md) records the remaining native-platform, corpus and budget gates.

### Implemented sampled-distance hollowing (TD-045)

`preparation::sampled::hollow_mesh_sampled` consumes the same immutable `PreparationPlan`
(one hollow operation; sampling cell, query error and cell cap). It returns a
`SampledHollowedModel`: placed exterior plus reversed cavity, the outward cavity surface,
certificate, statistics and plan record.

Since M6.4e h1 (TD-093) the certificate's lower end is proven per cavity face:
- a face whose widened vertex bound already passes is accepted at once;
- otherwise `Bvh::clearance` proves it beyond the smallest vertex distance less 1 µm, on the
  source index hollowing built (with its plane index);
- otherwise the face gets its own `Bvh::lower_bound`.

The faces are shared by worker threads through an atomic counter, in batches. The minimum is
exact and order-free, so the result is the same for any thread count. A face that meets the source
refuses the cavity; the old refusal came from widening vertex distances by each face's size.

Since h2 a `HollowSpec` may carry a `WallGuarantee` (TD-093). `hollow_mesh_sampled_with_progress`
then runs passes, each one extraction (`hollow_pass`: grid, field, welding, validation, the
optional simplification and the certificate) sharing the source index and its plane index:
- the first pass at `thickness + coefficient · cell² / thickness`;
- a short pass raises the offset by 1.25 times its shortfall;
- after a pass the proof cannot separate, or three short ones at one cell, the cell is halved;
- at most six passes, then `HollowError::WallNotReached`.

`guaranteed_cell` gives the rule's first cell from the source's `surface_area`. The certificate
keeps the requested thickness and adds the offset and the passes. Plans without a guarantee are
hollowed and recorded as before.

Private submodules:
- `bvh`: deterministic tree; nearest distance; exact symbolically perturbed vertical-ray parity.
- `distance`: binary64 point-triangle distance with an analytic error budget.
- `field`: adaptive Lipschitz blocks; `std` scoped threads merged in item order.
- `tetra`: Kuhn marching tetrahedra.

The backend is independent of the convex modules except for shared error and numeric types.
Source meshes are never mutated. `karasu_core::digest::sha256` lets callers bind plans to source
bytes.

**Model frame (TD-089, M6.4c c1):** the pipeline binds the hollow plan to the identity placement,
so the source is hollowed in its model frame and the cavity does not depend on where the object
lies. `SampledHollowedModel::placed` then maps the shell and cavity by the object's placement:
- every vertex through the placement's flattened affine map, as a placed plan maps its source;
- faces reversed under a reflection, so mirrored objects hollow too;
- certificate distances scaled by the smallest and largest absolute scale factor
  (`placement_scale`), volumes recomputed.

`drain_hollowed_sampled` certifies drain holes on such a placed shell without hollowing again;
`drain_model_sampled` remains the hollow-then-drain composition.

**Blockers (TD-115):** a `HollowSpec` may carry `HollowBlocker`s, frustums and spheres in the
plan's source frame (the model frame in the pipeline). The field is `min(d - t, s_B)`, `s_B` the
least 1-Lipschitz signed bound of the blockers taken back by the placement's inverse and scaled
by its least singular value; the grid's pruning and the wall proof stay valid, and blockers only
take cavity away. The plan's record encodes them. The project keeps them as
`OperationKind::Blocker` in the object's timeline (`request::Blocker`, model frame).

**Free drain holes (TD-089, M6.4c c2):** the pipeline cuts holes with `sampled::holes`, not with
certified drainage.
- `HoleProbe` resolves holes on the outer or the cavity surface and reports `HoleFindings`: whether
  a hole opens outside, passes through, reaches a cavity or ends in material, from its axis and
  eight side lines.
- `HoleCuts` serves the cylinders' sections per layer, indexed by height.
- Cut rings are removed as their union: each ring is walked counter-clockwise and a sample is cut
  where the winding is not zero.

Holes are cut into solid objects as well as hollowed ones; only a diameter below the printable
minimum is refused.

**Object operations and the cavity cache (TD-089, TD-090, M6.4c c3):**
- `project::PlateObject::operations` is each object's timeline: identities, enabled flags,
  hollowing and drain holes, the holes in the object's model frame.
- Requests carry `Coordinates` for holes and for supports: the pipeline places model-frame
  holes before cutting them, and a model-frame support document (`SupportDocument::placed`,
  TD-108) before building it. The interface keeps both in the object's model frame;
  `pipeline::build_model_supports` builds an object's supports, drafts included, against its
  preparation while they are edited.
- `pipeline::PipelineCache` keeps model-frame cavities, keyed by the hollow plan's record,
  format and unit, and models held in memory as imported, checked and repaired, keyed by the
  identity of their bytes. `prepare_with`, `slice_with` and the jobs share one, so a moved object
  or edited holes never import, repair or hollow again, and slicing reuses the preview's work.
  Models read from files (the command line) are not kept.
- A tilt is measured about the surface normal, so a model-frame tilt is carried to the world by
  its axis (`sampled::map_tilt`, `DrainHole::with_world_tilt`, M6.4c c4): a tilted hole turns
  with its object.
- A preview builds an object's hole probe only when the object has drain holes or
  `prepare_with` is asked for it (holes being placed, or new holes not yet applied): it costs as
  much as the rest of a preview after a move.

### Drainage of sampled cavities and raster difference (TD-047)

`preparation::sampled::drain_model_sampled` returns a `SampledDrainedModel`. It holds the hollow
model, the union of cutter prisms, the certified openings and the cavity regions. Each region
keeps a vertex of its outer surface and that surface's exact coordinate extrema, in the cavity
mesh's coordinates, so an interface can locate it (TD-101); they cost one point and one box per
cavity surface component, gathered in the pass that already measures the components' volumes.

Private module roles:
- `drain`: certification.
- `contact`: exact segment/triangle intersection and improper-mesh-contact rules.
- BVH extensions: labeled parity, exact segment queries with a conservative slab filter, triangle
  clearance, a bounded best-first lower bound for whole caps through their enclosing disks, and a
  cancellable self-pair traversal. Construction reserves its exact node count and polls
  cancellation.

The drained solid is never built as a mesh:
- `body_layers` slices the shell as one strict stream.
- The model implements `raster::LayerCut`, returning analytic cutter sections per plane.
- `raster::stream::DifferenceRasterStream` subtracts those rings per sample.

The render core keeps one scanline state per edge set; a plain render is the special case with no
cut set. Each row is written from the sorted sample-space endpoints of its sub-row spans as runs of
constant coverage, so its work follows the span endpoints and covered runs rather than the panel
width (TD-050). A crossing's first sample column comes from an interpolated estimate corrected by
the exact orientation sign of the sample centre against the edge, so every sample is classified
exactly against the projected edges, whatever their direction.

### Winding-union layers (TD-050)

`slicing::winding` builds an alternative to the strict layer from the same section kernel:
- planes are always vertex-avoiding, so every crossed face yields one segment between two edge
  crossings, and faces sharing an edge get bit-identical endpoints;
- each segment is oriented from the face's cyclic order and its lone corner's side (material on
  the left for outward faces; reversed under a reflecting transform);
- the section must balance exactly at every crossing position; open boundaries and inconsistent
  orientation fail with the offending edges, and nothing is repaired;
- the raster sweeps signed crossings under `WindingRule::{Positive, NonZero, EvenOdd}` and
  reports the sampled winding range. Positive refuses negative winding.
Overlapping, touching and self-intersecting bodies are thus sliced as their union, and reversed
shells (hollowing cavities) subtract.

Open meshes (TD-054): `mesh::topology` chains boundary half-edges into rims, and builds the lean
`DefectReport` from one sorted array of directed uses. With `SectionClosure`, a layer's unbalanced
crossings on boundary edges are grouped by rim and ordered along it. Ends and starts alternate, so
each end is joined to a start of the same rim by a straight chord, as long as the chord stays within
the gap bound. For a hole crossed twice this is the slice's winding number thresholded at 0.5.
Imbalance off the rims and over-long chords fail the layer.

`mesh::repair` (TD-054 phase 2) works on a derived face list:
- duplicate groups collapse to their net orientation (a stacked group keeps its net count where
  one face would unbalance a balanced edge, TD-092 r9);
- orientation is made consistent per component through union-find with parity, keeping the
  larger-area class;
- opt-in `outward` flips outermost closed shells enclosing negative volume, and the shells nested
  in them when every outermost shell around those turns (TD-092 r9).
The CLI slices the repaired mesh and reports every change.

M6.4d r1 (TD-092) locates every defect and every change:
- `DefectReport` adds needles and caps, cracks (rim loops on a line), pinch vertices, components
  under `SMALL_COMPONENT_MM2`, and up to `MAX_SAMPLES` positions per `DefectClass`;
- `preparation::repair::self_intersections` (on request, not at import) counts face pairs that
  meet improperly, by the exact contact test of hole filling over a BVH, up to a bound, with
  positions;
- `RepairReport` changes are located through `TriangleMesh::repair_with_samples` and the fill
  report (`RepairSample`, up to `MAX_REPAIR_SAMPLES` per `RepairChange`), gathered by
  `SourceRepairReport::all_samples`; the fill report adds the largest cap's area;
- `RepairPreset::{Off, Exact}` name fixed `RepairSpec`s; tolerant stages join presets as they are
  built, so no preset promises an unimplemented stage;
- `karasu_project::diagnosis` adds findings for cracks, zero-area faces, pinch vertices and small
  bodies, and `Diagnosis::printability` sums a diagnosis up as `Ready`, `ReadyWithNotes`,
  `NeedsRepairForHollowing` or `NeedsRepair`, following the checks union slicing, rasterizing and
  hollowing admission apply to the mesh as diagnosed; diagnosing the repaired mesh gives the
  state after repair;
- an object's own repair is `OperationKind::Repair` in its timeline, first and at most once; its
  request carries `ObjectSpec::repair`, which replaces the job's repairs for that object (plate
  documents keep the job's).

M6.4d r2 (TD-092) adds the tolerant stages to `repair_source`, before duplicate collapse:
- `preparation::repair::weld` (R1, `RepairSpec::weld_mm`): vertices on open edges merge into
  clusters of diameter at most the tolerance, grown from the smallest index over a sorted grid of
  cell size the tolerance; the neighbour test runs on every core, clustering sequentially;
- `preparation::repair::degenerate` (R2, `degenerate_mm`): faces with a repeated vertex are
  removed; with a tolerance, short edges collapse under the link condition and a no-flip check,
  and flat faces flip with their neighbour when both new faces are well shaped, in deterministic
  passes over the faces;
- crack closing (R4, `crack_mm`) zips rims within the tolerance of a line, and accepts any
  balanced result: splitting keeps every face's region, so the winding number cannot change;
- tolerances are recorded in the plan under markers 0xF3 to 0xF5, only when set, so earlier
  records are unchanged; `RepairPreset::Standard` sets all three to 0.001 mm.

M6.4d r3 (TD-092) makes sampled hollowing tolerant:
- **H1:** admission closes exact zero-area cracks (hole filling in crack-only mode) and leaves out
  faces of zero computed area, then requires a balanced surface; `MeasureZeroDefects` reports
  both. The shell's exterior is that surface, which changes no inside/outside state.
- **H2:** the field's interior is a positive winding number. Distances are to every source face,
  so no void contains one and the winding number is constant in each void; the assembly emits
  each reversed cavity surface once per unit of that winding number (`CavityOverlap`), after one
  copy of all of them, so the shell's winding number is zero in every void.
  `SampledHollowedModel::exterior_faces` gives the exterior prefix, which callers no longer
  derive from the cavity's face count.
- **S1:** drain-hole certification works unchanged on such shells (the overlap's void is drained
  like any other); supports never depended on the model's topology.

M6.4d r4 (TD-092) adds `preparation::repair::fins` (R5, `RepairSpec::non_manifold`), after
duplicate collapse and orientation: parts joined through two-face edges, each edge's balance and
each part's unbalanced uses, then fins (open parts surplus at an unbalanced non-manifold edge,
removed when no edge gets worse and they are not the largest part there, smallest first) and, in
the aggressive mode, every face on an edge left unbalanced. `karasu_project::diagnosis` reports
fins left after the default repairs (`Diagnosis::fins`, `Finding::Fins`), and an unbalanced
non-manifold edge makes the state `NeedsRepair`, since layers crossing it do not close.

M6.4d r5 (TD-092) extends hole filling (R7, `FillHoles::{split_rims, refine}` and an unbounded
`max_hole_edges`): a `Capper` caps simple loops, splitting rims through a vertex twice into them;
loops up to `MAX_EXACT_CAP_EDGES` start from the minimum-weight triangulation, larger ones (with
refinement) from a centroid fan; `fill::refine::Patch` refines by centroid splits and Delaunay
edge swaps, then fairs by a membrane and a thin plate whose rim umbrella includes the mesh
neighbours outside the cap. New vertices get indices after the mesh's and are compacted to the
surviving caps; a refined cap that meets the mesh falls back to its unrefined triangulation
before the hole is refused. Since r9, a planar rim at a crease keeps the flat membrane, with the
thin plate's cap as its first alternative (`Cap::alternatives`, tried in order; of two caps that
meet, one with an alternative is replaced first), and contact validation indexes only the caps
and the mesh faces whose box meets a cap's.

M6.4d r6 (TD-092) adds `preparation::sampled::shells`: `orient_by_visibility` (R6, before hole
filling) turns open parts by exact segment tests against a BVH of the model, and
`remove_shells` (R8, after hole filling) removes small shells and, when asked, inner shells:
closed outward shells that no other shell crosses (exact contact pairs) and whose vertex the
other closed shells' per-label ray parity, weighted by their orientation, puts at a winding
number of one or more.

M6.4d r7 (TD-092) adds `preparation::sampled::rebuild` (R9, `RepairSpec::rebuild_mm`, last in
`repair_source`): the hollowing field in a rebuild mode (`FieldSpec::rebuild`) samples the
distance signed by the exact vertical-ray winding number only in blocks the surface may cross;
a sample takes its sign from an already signed grid neighbour when their distances sum to more
than their spacing (the distance is 1-Lipschitz), so rays are cast only near the surface. The
zero level is extracted, welded and validated as hollowing's cavity is; the deviation is
certified from exact vertex distances, and the source's vertices are measured against the
rebuilt triangles through a cell index searched in rings.

M6.4d r8 (TD-092) brings repair to the interface:
- `pipeline::ObjectPreparation::repair` gives a preview's `RepairOutcome`: the repairs applied,
  their report, and the full diagnosis of the mesh as it will be sliced, computed once per
  repairs (`RepairedSource` keeps it in the pipeline cache). A preview no longer stops at the
  topology check (slicing still does), so the defects can be shown.
- `karasu-app`'s `repair_panel` is the object panel's Repair section, editing the object's own
  repairs through `Workspace::set_repair`, and draws the markers: one octahedron uploaded once
  and placed per marker, sized from the camera distance, drawn solid and as a ghost overlay.
  The app prepares every object once a printer and resin are chosen.
- `Workspace` applies the auto-repair preset on import and keeps each model's import diagnosis.
- `pipeline::Remedy::RepairDefects` names the repair for a layer that does not close; the
  interface turns it into `Fix::Repair(Standard)`, and reversed slivers into
  `Fix::Repair(Rebuild)` after the plate's fixes.

Phase 3 (TD-054) moves source repair into the preparation plan: `OperationKind::Repair`, first
in the plan, is applied by `preparation::repair::repair_source` in the order duplicates and
orientation, hole filling (`preparation::sampled::fill`: minimum-weight caps of simple rims, no new
vertices, exact contact validation over a BVH; zero-area cracks closed by splitting the faces along
them, TD-091), then outward. Hollowing and drainage apply it to
the bound source before admission, which requires balanced edges and no degenerate faces. The
field classifies points by the exact vertical-ray winding number: 0 is outside, 1 inside, and
any other value fails, because a cavity in an overlap would keep internal walls. On any input the strict pipeline accepts, every rule gives
the strict mask. `raster::parallel::LayerFill` selects the fill; the CLI defaults to union. An entry is exterior when an exact straight segment, in one of 26 directions, joins it to
the outside of the source's bounding box. Only when every such segment is blocked does drainage verify that the
source is embedded with disjoint bodies (exact BVH self-pair check, at most once per call), which
the per-component parity fallback requires. Drainage reports the `SampledHollowStage::Drainage`
progress stage after hollowing.

### Slicing throughput (TD-046)

The layer engine can opt into a per-face world Z index (`with_face_z_index`), so each plane
evaluates only the faces whose closed Z range contains it, still in ascending source order.

Planar validity:
- tests only segment pairs whose closed XY boxes overlap, found by an X sweep and processed in
  lexicographic order;
- its exact 2D orientation uses an outward-rounded interval filter before the integer fallback.

All three keep results identical to the exhaustive forms; summaries record prefiltered faces and
exact pair tests.

### Cavity simplification and oriented drain holes (TD-066)

**`preparation::sampled::simplify`:**
- reduces the extracted cavity by deterministic half-edge collapses between extraction and
  certification;
- per accepted collapse it proves:
  - the link condition;
  - a two-sided deviation bound from an injective projection of star and fan;
  - an exact clearance from the source above the wall floor (`Bvh::clearance`, with indexed
    planes);
  - embedding (`improper_contact` over a face grid);
- is driven by the plan operation `Simplify`.

**Drain cuts:**
- any direction: `oblique_cylinder`, sections as convex hulls of edge crossings;
- `SurfaceHole` operations are resolved inside `drain_model_sampled` from the source normal and
  the first cavity crossing, then certified as before.

### Multiple models per plate (TD-065)

`karasu_core::plate` holds the `karasu-plate` document and two pure functions: the deterministic
packer `arrange` (from the plate's middle outwards, shelves as a fallback) and
`overlapping_pairs`. Model files, preparation and slicing stay in the
caller.
- **Preparation:** the CLI prepares every object once at its design placement (`prepare_object`,
  the single-model pipeline).
- **Placement:** the CLI computes footprints with supports and drain cutters, then arranges the
  instances.
- **Slicing:** all instances are concatenated into one world mesh, sliced with the existing
  positive-winding union. Drained instances contribute translated cut rings through a `LayerCut`
  wrapper; their footprints must not overlap, so the rings stay disjoint.
- **Single model:** it keeps its placement transform path unchanged.

### Variable and adaptive layer heights (TD-064)

`layer_plan` turns a `karasu-layers` document into per-layer heights in integer nanometres and
complete `LayerOverride`s.
- **Ranges:** the carry boundary policy applies.
- **Adaptive:** a slope bound from a Z-bucket index of the placed model.
- **Exposure:** never inferred.

`slicing::schedule::build_slab_schedule` turns the heights into a `LayerSchedule` with `Slab`s:
- it samples at slab centres, and its `spec()` is `None`;
- positions are exact divisions of integers, so they never accumulate floating-point error.

`PrintJob` reads layer positions and thicknesses from the slabs. It enforces calibrated bottom and
transition layers, printer height limits and explicit settings for every other thickness. Writers
derive thickness from consecutive positions; SL1 refuses non-uniform jobs.

### Output formats (TD-063)

`BuiltinFormat` gains:
- `CtbV3Plain`, `CtbV4Plain` and `CtbV5Plain` (`print_job::ctb`, inside the machine encoder);
- `AnycubicWorkshop(target)` (`print_job::photon_workshop`; its layer images reuse PP1's PW0
  encoder);
- `PrusaSl1FirmwareMotion` and `PrusaSl1sFirmwareMotion` (`print_job::sl1`, a ZIP encoder beside
  PP1).

All of them keep the streaming contract:
- metadata is written or reserved first;
- layers are encoded on any thread and written in order;
- records and totals are patched at the end;
- outputs are independent of thread count and platform.

### Manual supports (TD-062)

`supports` is independent of the convex KSUP1 plans of M5.5-D.
- **Document:** `SupportDocument` reads a versioned JSON document. Each support gets a complete
  parameter set from a preset, the document defaults and its own overrides.
- **Build:** `build_supports` builds deterministic closed convex parts and validates them against
  the sliced surface:
  - anchors and interior classification;
  - contacts fitted to the surface (TD-116, `supports::fit`): a support with
    `contact_fit_min_mm` narrows its contact, ball and penetration together until the
    footprint has room along the surface's mean normal, tested with exact segment crossings on
    the same grid;
  - axis deviation and piercing;
  - grid-indexed, conservative separating-axis clearance proofs;
  - drain holes and the build volume.
  It returns a `SupportScene` with the concatenated mesh, structured findings and each support's
  contact diameter as built.
- **Slicing:** the CLI concatenates the scene with the surface (the placed model, or the hollowed
  shell) and slices it under the positive winding rule, so supports and the model form one
  material.

### CLI placement, compensation and resin usage (TD-061)

- **Placement:** the CLI composes one `ObjectTransform`: rotation from
  `Rotation3::from_euler_xyz_degrees` (platform-independent trigonometry), positive scale, and
  mirroring as negative scale, all about the source bounding-box centre. The translation is then
  explicit, or computed by `--center` and `--drop` from the bounds placed without one. Dropping
  rests the model exactly on Z = 0.
- **Mirroring:** a mirrored placement is sliced correctly because winding layers reverse section
  directions under a negative determinant. Hollowing plans still require positive scales.
- **Bottom raster:** `ParallelRaster::bottom` gives the first N layers their own `RasterConfig`;
  the CLI uses it for bottom-layer XY compensation.
- **Resin usage:** `ResinProfile::usage` converts the sampled volume into resin volume, mass and
  cost.

### Minimum edge gray (TD-060)

`RasterConfig::minimum_gray` sets the smallest intensity of a partially covered pixel.
- Coverage `c` strictly between 0 and 1 maps to `minimum + (255 − minimum)·c` in exact integer
  arithmetic, then rounds to the nearest of `grayscale_levels` (ties up).
- Empty pixels stay 0 and full pixels 255.
- With 0 the quantizer is unchanged.

The remap lives in the per-count level table, so span rasterization, statistics and compensation
need no other change. Reference records carry the value when nonzero.

### Print-file previews (TD-059)

`preview::render_preview` draws a mesh under a placement into an RGB image.
- **Camera and fit:** an orthographic camera from the front right and above, fitted to the
  projected outline.
- **Rasterization:** 3 × 3 supersampling, `f64` depth and two-sided Lambert shading.
- **Determinism:** same pixels on every platform; position on the plate does not matter.

The CLI renders the sliced surface (the repaired model or the hollowed shell) at
`BuiltinExporter::preview_sizes` and passes it with `with_previews`. The encoders then store it:
- GOO as RGB565 thumbnails;
- CTB and Photon as RLE15 previews between the header and the layer table;
- PP1 as RGB PNGs.

Sizes are checked before any output. Without previews, files are unchanged from before TD-059.

### Parallel import (TD-058)

`mesh::weld` welds corner positions exactly on all cores. It shards corners by key hash, finds
each corner's first equal corner per shard in increasing order, and numbers vertices in order of
first occurrence, so the result does not depend on the thread count.
- STL binary decodes records in parallel and welds straight from the input bytes.
- OBJ parses line chunks in parallel and triangulates face groups in parallel.
- 3MF inflates each model part on a second thread while the XML parser consumes it.
- Geometry diagnostics compute degeneracy and duplicate keys in parallel.

### Model formats (TD-057)

`mesh::formats` is the import boundary.
- **Registry:** a `FormatRegistry` of `ModelFormat` implementations (STL, OBJ, 3MF, and any
  registered later), selected by extension.
- **Pipeline:** each format parses bytes into a file-unit `SourceModel`. The registry then
  resolves the unit (declared, assumed, or a conflict) and scales non-millimetre meshes after
  every transform.
- **Construction:** OBJ and 3MF build through the shared exact-bit welding builder used by STL.
- **3MF stack:** a bounded in-memory ZIP reader (`formats::zip`, with `miniz_oxide` inflate and
  CRC-32 checks) under a streaming `quick-xml` model parser.

### Parallel topology analysis (TD-056)

The defect report, repair, edge summary, full topology report and import duplicate check sort
with `parallel::sort_by_key`. It sorts chunks on scoped threads and then merges them, splitting
each merge by co-ranking. Its result equals the sequential unstable sort because every key is
unique or covers its element.

Face usability is computed once per report by `parallel::fill_with`. The CLI runs the import-time
defect report and source repair concurrently.

### Span masks (TD-055)

A `RasterImage` holds its mask as a `raster::mask::SpanMask`: per row, the sorted, disjoint,
maximal runs of one nonzero grayscale8 value, in canonical form.
- **Producer:** the rasterizer's row writer pushes its runs directly, so no dense image is
  allocated. Compensation alone works on a dense copy.
- **Consumers:**
  - the GOO, CTB, Photon V1 and PW0 codecs encode from the row-major runs, coalesced across rows;
  - PP1 scene metrics label components from the row spans;
  - the reference PNG and CRC stream one row at a time.
- **Budgets:** retained spans are charged against the raster payload budget. The parallel
  windows account for each finished layer's retained bytes.
- **Dense access:** `to_pixels()` materializes the image when a caller needs it.

### Parallel layer stream (TD-050)

`raster::parallel::with_parallel_raster` runs the engine's per-layer kernel (`LayerKernel`, shared
with `LayerEngine`) and rasterization on scoped worker threads:
- Workers share an immutable `FaceZIndex` and keep their own `FaceZCursor`.
- A bounded reorder window delivers images in layer order.
- The first error in layer order ends the stream, the caller's cancellation predicate is polled by
  all workers, and an early consumer stop joins them.
- The CLI uses it with all cores, together with `export::export_machine_parallel`. Codecs split into
  precheck, pure encode and emit; `ExportSession::layer_encoder` and `write_encoded` (with
  defaults) let workers encode while layers are still written strictly in order.

### Vertex-avoiding layer planes (TD-048)

`LayerEngine::with_vertex_avoiding_planes` (used by the CLI) samples every layer on a plane that
contains no mesh vertex. A scheduled plane equal to the world Z of a face corner moves to the middle
of the vertex-free interval just below it, bounded by the next lower vertex height and by the bottom
of its slab. With no vertex on the plane, horizontal faces, on-plane edges and vertices cannot occur,
so the section is generic and equals the section just below the scheduled plane. The layer keeps its
scheduled `plane()` identity (print jobs check it); `sampled_z_mm()` reports the sectioned Z, which
analytic cutter sections also use. The heights are computed once (8 bytes per face corner).

### Grid-independent represented-coordinate layers (TD-031)

The [contract](M3-REPRESENTED-COORDINATE-CONTRACT.md) adds
`LayerEngine::with_represented_coordinates()` and a corresponding borrowed coordinate view. Agreed
world-XY millimeters remain unchanged; explicit outcomes, counts and a mode flag distinguish them
from integer indices. Grid metadata is inactive in this mode, including empty outputs. The same
strict forest uses exact represented-f64 orientation without division or epsilon. Existing exact
grid behavior remains available and unchanged by default.

Optional source-labelled material union decodes represented values into a lossless common dyadic
encoding bounded by 60 magnitude bits, retaining the exponent on the exact rational output. Its
private bound is proved against the existing 512-bit arithmetic, and encoding scratch/work consume
explicit budgets. Unsupported dynamic range is a structured failure. No rounded index is labelled
exact, no raw provenance is lost and no output escapes before final cancellation checks. The
audit's former mandatory-grid usability issue is resolved
within these explicit domains; platform/corpus/budget acceptance remains separate.

ExactOnly's narrow numerical domain remains explicit; construction does not cure upstream off-grid
rounding. Lossy conversion and physical error budgets still require their own contract.

## Implemented M2.1 mesh foundation

### Representation and ownership

`karasu-core::geometry` provides distinct `Point3` and `Vector3` values with `f64` components. A plain
value can contain non-finite components; geometry containers validate at entry. `mesh::TriangleMesh`
owns one `Vec<Point3>` and one `Vec<[u32; 3]>`, exposes immutable slices, and caches an optional
source-space `Aabb`. Bounds include the full vertex buffer, including unused manually supplied
vertices. `try_new` takes ownership, validates every coordinate/index, normalizes signed zero, and
rejects extents whose subtraction would overflow. It does not weld or reorder manually supplied data.
Empty vertices/faces are valid; empty vertices have `None` bounds, not infinities. Dimensions and
center are finite for accepted bounds; center uses minimum plus half extent to avoid endpoint-sum
overflow. Statistics are counts/bounds, not a printability classification.

STL import reconstructs shared vertex identity using exact finite `f64` coordinate bits after
normalizing `-0` to `+0`. First occurrence determines vertex index; facet and corner order remain
unchanged. No distance tolerance or proximity welding is applied. Binary coordinates are promoted
exactly from file `f32` to `f64`, without recovering precision lost during serialization. ASCII
decimal coordinates round to `f64`; nonzero decimals that underflow to zero are rejected. NaN and
infinite vertices are errors before spatial keys are created. Determinism does not depend on hash
table iteration: tables are lookup-only, and output follows source order. Exact coincident coordinates
reconstruct adjacency candidates; STL itself has no shared-index topology or connectivity intent.

`mesh::stl::ImportedStl` separates the mesh, original header/normal/attribute metadata, and diagnostic
list. Each original corner is recoverable by its facet/corner index through the face buffer. Raw STL
corner records and encoded input are not duplicated in the result. The reader path temporarily
buffers encoded input to support structural format detection, then releases it after import. Vertex
identity tables are dropped before duplicate-triangle analysis to reduce overlapping scratch storage.

### API and explicit units

`import_stl_path`, `import_stl_reader`, and `import_stl_bytes` all require `StlOptions`. Callers must
explicitly choose `StlUnitPolicy::AssumeMillimeters`; STL does not declare standard units. This policy
is retained in metadata and maps one source unit to one mm without modifying coordinates. Other unit
conversions are not implemented or silently inferred. Format and resource limits can be overridden.
The public module documentation contains an executable usage example.

`StlOptions::non_finite` (TD-092 r9) refuses a facet with a NaN or infinite corner coordinate
(`NonFinitePolicy::Refuse`, the default, `StlError::NonFiniteCoordinate`) or leaves it out
(`Drop`): `ImportedStl::dropped` then counts the facets, lists their first runs and keeps the
refusal the default would give. Mesh triangles, facet metadata and diagnostics follow the kept
facets. `ModelImportOptions::non_finite` passes the policy to STL (OBJ and 3MF refuse such
coordinates whatever it is) and records a drop as `ImportNote::NonFiniteFacetsDropped`. The
pipeline always imports with `Drop`, so its cached import does not depend on the repairs, and
refuses the job with the import's own message unless `RepairSpec::non_finite` accepts it (part of
`Exact`); the plan record leaves the switch out, as the mesh a successful job uses is the same
either way.

No coordinate is centered, lifted, rotated, scaled, repaired, or otherwise adjusted to resemble a
reference GUI. FX-05 retains its baked obliqueness at original source coordinates. M2.2 transforms
and world-space bounds below are separate non-mutating operations. Additional import unit policies
and automatic placement are not implemented. M2.3 source topology is described separately below.

### Binary parsing and format detection

Binary import validates the 80-byte opaque header, four-byte little-endian count, and exactly
`84 + 50 * count` bytes with checked arithmetic. Each record has a stored normal, three vertices,
and an opaque little-endian `u16` attribute field. Zero facets are allowed. Truncation, incomplete
records, and any trailing bytes (including whitespace or purported attribute payloads) are errors.
Nonzero attribute bits are retained and diagnosed, never interpreted as color or vendor extensions.

Auto detection gives exact count/length agreement binary precedence, even for a header beginning
with `solid`. Otherwise, ASCII data without NULs and with a whitespace-delimited `solid` opener
selects ASCII; remaining data at least 84 bytes long or containing non-text/NUL bytes selects binary
validation. Short unrecognized text is rejected. Failed candidate-size arithmetic does not prevent
ASCII detection on smaller pointer-width platforms. Detection is a documented selection rule, not
an infallible recovery of malformed input intent. A selected parser never silently falls back to
another; callers can select Binary or Ascii explicitly. A binary-looking `solid` header alone is
never sufficient evidence for ASCII.

### Supported ASCII grammar

Keywords are case-sensitive. One solid is supported, with one logical record per nonblank line:

```text
solid [optional name]
facet normal nx ny nz
outer loop
vertex x y z
vertex x y z
vertex x y z
endloop
endfacet
... further facets ...
endsolid [optional name]
```

Leading/trailing whitespace, blank lines, spaces/tabs between tokens, LF/CRLF, signed decimals, and
scientific notation are accepted. Optional opening/closing labels are retained (trimmed), may differ,
and do not define units. Exactly three numeric components/corners are required. After `endsolid`,
only whitespace is accepted. Extra solids, comments, unrecognized records, binary data/NULs, Unicode
names, uppercase keywords, and multiple logical records on one line are deliberately unsupported.
Syntax failures report one-based line context. Empty solids are valid; empty encoded input is not.

### Normal and diagnostic policy

Original normals are retained separately, at 32 bytes per `FacetMetadata` on the tested 64-bit target
(including optional attribute and padding; layout is not a serialization contract). Zero/non-finite
normal values do not invalidate finite vertex geometry and are diagnosed without repair or replacement.
Binary normal components are promoted to `f64`; NaN payload/signaling-bit preservation is not promised.
Geometry-derived `FaceNormal` values are computed on demand from preserved winding. Finite nonzero
stored normals opposing the computed direction are diagnosed by negative dot product; this does not
constitute exhaustive normal validation. No face is reversed to match a file normal.

Non-destructive geometry diagnostics distinguish exact repeated corners from `ZeroComputedArea`:
the latter means a scale-normalized `f64` cross product vanished, not an exact collinearity proof.
Edge/direction scaling avoids ordinary cross-product overflow/underflow for very small or large
triangles. Extreme aspect ratios and cancellation can still make the computed result undefined or
unreliable; an undefined normal remains explicit, not silently repaired. No near-degeneracy threshold,
global epsilon, exact/adaptive predicate, area accuracy guarantee, or watertightness claim is made.
Duplicate geometry uses the same three exact point keys independent of winding or cyclic corner
order, including when manually constructed faces use distinct indices for coincident points. Every
duplicate references the first face, and every face remains present.

Diagnostic order is geometry findings in triangle order (degeneracy then duplicate), followed by
normal/attribute observations in facet order. Invalid coordinates/indices instead fail construction
with contextual errors. Error types preserve I/O/mesh causes and identify facets, corners, axes,
required/actual byte lengths, line numbers, or exceeded limits. No partial mesh is published on
structural or resource failure. No foreign code/dependency or commercial implementation is involved.

### Resource and execution limits

Default caps (TD-053) are 1,280 MiB encoded input, 25,000,000 facets, 75,000,000 distinct vertices,
4,096 bytes per
ASCII line (excluding LF, including a CR), and 128 bytes per numeric token. These bound work/input
sizes, not total resident memory. All growable parser/diagnostic buffers use fallible reservations;
no capacity is allocated solely from an unchecked file count. Binary expected size is validated
before records are parsed. Reader buffering consumes at most the byte cap plus one probe byte,
handles interrupted reads, and preserves I/O failures. Worst-case mesh, metadata, and identity/key
tables can consume substantially more memory than the encoded file; callers must select smaller
caps for stricter environments. Allocation errors are reported where the allocator permits recovery.

Parsing is sequential. Vertex/duplicate key lookup is expected linear-time overall, with hash-table
scratch space proportional to distinct vertices/faces; diagnostic storage is linear in findings.
No peak-memory budget or worst-case hash-table timing guarantee has been measured. Blocking readers
can block indefinitely; cooperative cancellation, streaming detection, complete topology/repair,
self-intersection analysis, sustained fuzzing, and native Windows validation remain later work.

## Implemented M2.2 scene transformations

### Mathematical convention and API

All transforms use `f64`, millimeters for positions/displacements, dimensionless scale, right-handed
active rotations, and column vectors. `geometry::ObjectTransform` stores validated immutable
translation `t`, unit-quaternion rotation `R`, local-axis scale `S`, and source/local pivot `c`:

```text
p_world = t + c + R * (S * (p_local - c))
M = T(t) * T(c) * R * S * T(-c)
L = R * S; b = (c - L*c) + t; p_world = L*p_local + b
```

Direct point evaluation subtracts the pivot, scales, rotates, adds the pivot, then adds world-space
translation. A point equal to the pivot therefore maps to `c+t`. Exact identity linear parameters
bypass unnecessary pivot subtraction/addition, including the equivalent negative-identity quaternion;
no approximate-angle/scale snapping is used. The object origin and pivot are not inferred from
centroids, source bounds, printer profiles, or commercial numeric position fields.

`geometry::Affine3` stores a finite row-major 3×3 coefficient array plus translation, equivalent to a
4×4 matrix with fixed bottom row `[0,0,0,1]`. Storage order is not vector orientation. `from_parts`
accepts general finite affine coefficients, including shear/reflection/singularity; identity,
translation, rotation, independent and uniform scale constructors are available. `a.then(b)` means
apply `a` first and `b` second: matrix `B*A`. Multiplication evaluates rows, columns, and dot-product
components in a fixed order. Composed non-uniform scales and rotations are not commuted or silently
decomposed into one object transform; shear is retained. Alternative orders use explicit affine
composition, not a scene graph or hidden UI order.

Both types expose checked `transform_point` and `transform_vector`. A direction/displacement uses
only the linear part (object local scale then rotation), never pivot or translation. This is not a
normal-vector transform: non-uniform-scale normal conversion would require a separately specified
inverse-transpose/singularity policy and is deferred. Existing file normals remain source metadata.
`ObjectTransform::to_affine` explicitly flattens parameters for composition; no inverse, general
affine decomposition, or baking API is implemented.

### Rotation and validation policy

`geometry::Rotation3` stores private X/Y/Z/W quaternion components. Constructors reject non-finite
values and exactly zero quaternions/axes. Nonzero quaternion inputs and rotation compositions are
explicitly normalized using maximum-component scaling before the norm, without a near-zero cutoff.
Axis-angle input normalizes its finite nonzero axis, then constructs the half-angle quaternion.
Angles are exclusively radians in the API; names include `_radians`. Euler convenience is fixed-axis
extrinsic X then Y then Z: `Rz*Ry*Rx` for column vectors. It does not promise uniquely recoverable
Euler angles. Quaternion `then` uses `q_next*q_self`, followed by normalization. No sign canonicalization
is imposed; quaternion representation equality differs from equivalence of rotations q and -q.

Every finite scale factor is supported: zero is singular and can collapse triangles; negative
factors reflect the corresponding local axes. With all axes nonzero, an odd number of negatives
reverses orientation. No source triangle order, stored normal, or diagnostic is repaired/reversed.
No invertibility claim follows from valid finite parameters. Users can change/remove a transform
because the source mesh remains intact, not because every mapping has an inverse.

Non-finite points, directions, matrix entries, translations, pivots, scales, axes, quaternions, and
angles produce `TransformError` with parameter/component context. Each point/composition product
and sum is checked; non-finite intermediate arithmetic is an error even if a differently associated
or exact calculation could cancel to a finite result. Parameters being finite does not guarantee
that every source point, composed matrix, or derived extent is representable. No epsilon/clamp,
repair, axis reorder, or retry under another formula hides failure.

Flattened affine evaluation and direct pivot evaluation need not be bit-identical: floating-point
association and cancellation differ. A large-pivot object can map its pivot correctly but fail
flattening because `L*c` overflows; direct evaluation remains usable. Conversely, direct subtraction
can overflow for an extreme point/pivot even if a different exact formulation could succeed. These
paths remain explicit APIs, not automatic fallbacks. Subnormal rounding/underflow and cancellation
are ordinary `f64` effects and can collapse distinctions; tiny angles/factors are not redefined by
an arbitrary tolerance. Standard-library trig argument reduction limits very-large-angle accuracy.
Repeated results use fixed evaluation order on the same build/target; cross-target trig results and
long composition chains are not promised bit-identical or exact. Quaternion normalization limits norm
drift but is not a proof of error-free matrix orthogonality or geometric predicates.

### Tight world bounds and preserved source data

`TriangleMesh::world_bounds(&ObjectTransform)` evaluates the direct pivot rule for every original
vertex. `TriangleMesh::transformed_bounds(&Affine3)` evaluates every vertex under a caller-composed
affine, whose target is world space when used as source-to-world. Both return `Option<Aabb>` with
the same finite dimensions/center contract as source bounds. Empty vertex buffers return `None`;
unused manually supplied vertices remain included, consistently with M2.1. `Aabb`'s producing API
names its space; it does not silently convert between source and world.

These O(vertex-count), O(1)-scratch, non-mutating operations accumulate extrema directly from computed
vertices, not a transformed eight-corner source AABB or a copied/transformed mesh. Bounds are tight
for those computed point values, not a guarantee of exact real arithmetic or outward rounding.
`TransformedBoundsError::Vertex` retains the first failing source vertex and underlying transform
cause; `UnrepresentableExtent` rejects finite extrema whose difference overflows. No partial bounds
are returned. Source vertices, indices, winding, bounds, STL header/facet metadata, and original
diagnostics remain unchanged, including after failures.

Independently recalculated FX-05 Rz(+30°), zero-pivot, unit-scale vertex extents are approximately
28.857155951470816 / 27.863533428766498 / 35.42976760864258 mm. With uniform 1.25 and translation
(15,10,5), extents are 36.07144493933852 / 34.829416785958124 / 44.28720951080322 mm and bounds are
[-3.035722469669260,-7.414708392979062,5] to [33.035722469669260,27.414708392979062,49.28720951080322]
mm. Tests derive references independently from actual serialized vertices, also using scalar Rx/Ry/Rz
equations for mixed-axis, non-uniform, non-origin-pivot cases. The rotated source AABB gives larger
enclosure extents ≈31.780873450250112 / 36.673286697723630 / 35.42976760864258 mm and is not substituted
for tight mesh bounds. CHITUBOX's rounded EXP-02 fields are secondary consistency evidence only;
Lychee's unchanged rotated size fields are not world-bound requirements. Neither GUI establishes
proprietary pivot/composition semantics. No M1 conclusion is rewritten by this implementation.

Translation Z=+5 stays requested; pivoted scaling may legitimately place vertices below Z=0.
Printer bounds, centering, grounding, placement automation, normal transformation, cancellation,
topology repair, slicing/rasterization, and GUI controls are outside M2.2. Native Windows validation,
large-workload timing/memory measurement, and sustained fuzzing remain separate quality work.

## Implemented M2.3 source topology

### Identity, ownership, and public API

`TriangleMesh::analyze_topology()` returns a separately owned `mesh::topology::TopologyReport`.
`analyze_topology_with_limits(TopologyLimits)` accepts tighter or larger explicit input caps.
The source mesh remains immutable; no coordinates, indices, winding, bounds, STL metadata,
diagnostics, or scene transforms are changed. No repair or derived mesh is produced. Public report
buffers are exposed only as borrowed slices, and component/vertex membership is resolved by checked
index accessors returning `Option`. Source triangle and vertex identifiers are zero-based.

Connectivity is exclusively index identity. M2.1 STL import deduplicates exactly equal coordinates
(signed zero normalized), so coincident serialized corners become one vertex, potentially joining
touching surfaces. It does not use a welding tolerance. `TriangleMesh::try_new` retains manually
supplied distinct indices even at identical or arbitrarily close positions; topology does not merge
them. Geometric duplicate diagnostics can therefore exist without shared topological identity.
Scene transforms are not inputs to source topology. Ordinary invertible transforms leave its report
unchanged; this says nothing about degeneracy after baking a singular or numerically collapsing map.

### Edges, excluded faces, and winding

Each usable triangle contributes its original `a->b`, `b->c`, `c->a`, including source triangle and
local edge index. `EdgeKey` is `(min(a,b),max(a,b))`; collapsed pairs cannot construct an ordinary key.
Flat directed uses are sorted by key, source triangle, and local edge; edge records index contiguous
ranges. One face means boundary, two means two-face, more than two means non-manifold. Duplicate
faces are never collapsed. Same direction on a two-face edge is a local winding conflict referencing
the edge and both faces. More-than-two-face edges do not receive a fabricated pairwise winding result.

All existing mesh geometric diagnostics are returned unchanged in `geometry_diagnostics()`, including
duplicates. Stored-normal/attribute observations remain on the separate imported STL result.
Repeated-position and zero-computed-area faces are preserved but excluded from ordinary edges,
links, and triangle components. Exclusions identify source face/reason, and every vertex touching an
excluded face is indeterminate, even when the usable sub-link looks healthy or already defective.
The full vertex-to-source-face index includes exclusions and counts repeated corners once per face.
The M2.1 scaled floating-point normal policy is reused without a new epsilon or exact-predicate claim;
underflow/cancellation may exclude a mathematically noncollinear face. No incomplete neighborhood
is labeled manifold. Vertices touched only by excluded faces are indeterminate, not isolated.

Local consistency describes the supplied winding only. No solver determines whether inconsistent
faces could be reoriented globally. No outwardness, volume, or embedding test is implemented, and
neither normals nor indices are reversed automatically.

### Components and vertex links

`edge_components()` partitions usable triangles connected by shared edges, including non-manifold
edges. `vertex_components()` partitions usable triangles sharing any source vertex. Two closed
shells touching only at one index yield two edge components but one vertex component. Each partition
stores ascending source triangles and distinct vertices in flat buffers, ordered by minimum source
triangle; interleaved source members need not form a contiguous source range. Component summaries
include triangle/vertex/edge counts and boundary/two-face/non-manifold/winding counts. A vertex can
belong to multiple edge components; counts must not be summed as globally distinct vertices.
Excluded faces and isolated vertices are not members of these triangle partitions. Isolated source
vertices receive explicit records/issues and prevent a whole-mesh surface-manifold claim.

At each vertex `v`, every usable incident triangle contributes the opposite neighbor pair as one
undirected link edge, retaining multiplicity. One connected simple cycle of at least three nodes
means interior-manifold; one simple path (two degree-one endpoints, all other degrees two) means
boundary-manifold. Multiple link components mean disconnected fans. Repeated link edges invalidate
the simple-link condition, including oppositely wound duplicate faces. Branches and other invalid
degree patterns are non-manifold. Defect priority is disconnected fans, duplicate link edges, then
branched link; the primary diagnostic does not enumerate every simultaneous defect. Non-manifold
edge endpoints acquire branched/invalid links rather than being mislabeled ordinary boundary fans.
Excluded incident faces override the usable-link classification with indeterminate; usable face,
excluded face, and usable-link component counts remain available.

### Separate classification semantics

`TopologySummary` deliberately exposes no `is_valid` or `is_watertight` shortcut:

| Finding | Whole-mesh definition |
| --- | --- |
| `no_boundary_edges` | No observed usable one-face edges; vacuously true for empty/excluded-only input |
| `edge_manifold` | Every usable edge has at most two incidences; exclusions prevent affirmative completion |
| `vertex_manifold` | Every source vertex has one complete simple path/cycle link; isolated vertices fail |
| `local_winding` | All usable two-face traversals oppose; exclusions/non-manifold edges prevent affirmative completion |
| `closed_combinatorial_surface` | Nonempty usable surface, no exclusions/isolated vertices, edge- and vertex-manifold, no boundary |

Checks return `Satisfied`, `Failed`, `Indeterminate`, or `NotApplicable`. A known counterexample
takes precedence over incompleteness; otherwise exclusions produce indeterminate, and no usable
surface produces not-applicable. Isolated vertices are counterexamples only to whole-mesh
vertex-manifoldness. Closed combinatorial classification deliberately does not require the supplied
winding to be consistent: FX-09 is closed combinatorially but fails local winding. This is a simple
triangulated-surface condition, not a general cell-complex acceptance policy for duplicate faces.

Even affirmative checks do not establish absence of self-intersections, positive enclosed volume,
outward orientation, valid nested shells, wall thickness, or resin printability. Global orientability
when supplied winding conflicts remains unassessed. A union of disconnected closed surfaces may
satisfy the combinatorial condition; connectivity alone never proves closedness or a solid.

### Ordering, resource failures, and complexity

Edges are lexicographic; their uses are source-triangle/local-edge ordered. Vertex records and full
incident-face lists follow source order. Components follow minimum face, with ascending members.
Issues are grouped as source-ordered exclusions, key-ordered edge findings, then source-ordered
vertex findings. Geometric diagnostics retain M2.1 order. No hash iteration order is exposed.
Repeated analysis of the same immutable mesh produces equal reports on the same computed-degeneracy
classification. Cross-target floating-point degeneracy behavior is not an exact-predicate guarantee.

Sorting edges, component membership, and links takes O(F log F + V) time, with expected O(F) existing
geometric-diagnostic hashing; link lookup is logarithmic in local degree. Disjoint sets use iterative
path compression and union by size. A high-incidence edge/vertex joins all faces to a representative,
never all face pairs. No recursion is used. Retained report and scratch storage are O(V+F+E), not a
dense face-adjacency matrix. Buffers are reserved fallibly; derived lengths/byte capacities are checked.
Counters are bounded by checked 3F incidences or preallocated node sets. Partial reports never escape
an allocation/count/resource failure. Invalid indices are rejected by immutable mesh construction.

Default caps match `StlOptions` import defaults: 75,000,000 original vertices and 25,000,000
original triangles, including excluded/unused data. Caps are not a peak-memory budget: the full
report needs roughly 300 bytes per triangle. `TriangleMesh::edge_summary_with_cancellation` computes
only the edge closure and orientation counts (identical to the report's) from one sorted word per
directed edge use, which is all winding-union admission needs (TD-053). `TopologyError` distinguishes cap resource,
size overflow, and underlying `MeshError` allocation/diagnostic causes through `Error::source`.
Three directed uses per usable face and one edge record per unique edge dominate adjacency payload.
On the validated 64-bit target these are 24 bytes/use and 32 bytes/edge: FX-11 has 22.5 MiB of use
payload and 15 MiB of edge payload alone, excluding spare capacity, source/metadata, vertex incidence,
component buffers, diagnostics, and temporary sets/sorts. No peak-RSS claim follows. Cancellation,
sustained fuzzing, controlled performance/memory profiling, and native Windows validation remain open.

## Memory and performance

Large jobs must not require all raster layers to be resident simultaneously. Future stages should
support bounded queues, per-layer processing, compact mesh indexes, and caller-controlled caching.
Memory budgets and cancellation points become acceptance criteria when the corresponding stages are
implemented. Optimization requires benchmark evidence and must preserve correctness baselines.

M2.4-C provides the first controlled repeated WSL/Linux Release baseline in
[PERFORMANCE-BASELINE.md](PERFORMANCE-BASELINE.md). Its separate byte-read, public import, resident
topology, and full-vertex world-bound timings do not change algorithms or representation. The
dependency-free example retains results until timing stops, excluding teardown/validation but not
worker overhead. Separate GNU Time runs measure whole-process peak RSS including setup and allocator
retention; they do not measure exact phase heap consumption or establish a hard memory budget.
Earlier phase-specific statements above describe their validation status at implementation time;
the new baseline supersedes the absence of controlled timing/RSS evidence, not numerical limitations.

## M2 Closure A: synchronous cooperative cancellation

`cancellation::{Cancellation, Checkpoint, Cancelled}` supplies a dependency-free, caller-controlled
contract. `Cancellation::new(&mut predicate)` exclusively borrows a `FnMut(Checkpoint) -> bool`.
The predicate executes synchronously on the invoking thread; `true` requests cancellation. It should
return promptly and must not panic. No internal thread, signal handler, async runtime, progress
event, percentage, job scheduler, or global flag exists. A caller may capture its own atomic flag;
the caller owns any cross-thread synchronization. The controller does not latch requests or reset
predicate state; cancellation observed during one invocation immediately unwinds that invocation
through typed errors. A later call may use the same predicate if its caller-owned state permits it.

### Public entry points and covered phases

Existing signatures/options and successful behavior are retained as no-predicate compatibility
wrappers. Additive entry points accept `&mut Cancellation<'_>`:

- `import_stl_path_with_cancellation`, `import_stl_reader_with_cancellation`, and
  `import_stl_bytes_with_cancellation`: path opening, reader buffering, automatic detection, binary
  decoding, ASCII classification/line validation/record consumption, exact vertex deduplication,
  owned mesh validation/source bounds, geometry diagnostics, and stored-normal/attribute diagnostics.
- `TriangleMesh::try_new_with_cancellation`: finite coordinates, signed-zero normalization, valid
  indices, and source-space bounds. Buffers are consumed, not borrowed; failure drops their ownership.
- `TriangleMesh::diagnostics_with_cancellation`: degeneracy and duplicate-face diagnostics.
- `TriangleMesh::analyze_topology_with_cancellation(limits, cancellation)`: explicit source caps,
  preparation/diagnostics/exclusions, edge uses/grouping/sorting, vertex indexing and complete links,
  both connectivity partitions and their ordering, summary counts, and final report publication.
- `world_bounds_with_cancellation` and `transformed_bounds_with_cancellation`: all actual source
  vertices under the existing object/affine rules. Constant-time point transforms are unchanged.

### Observation, precedence, and publication

Every public cancellable operation checks before work and before successful publication, including
empty results. Path pre-cancellation is checked before opening; reader pre-cancellation precedes
consumption. The public `Checkpoint` enum identifies phases, not source entity IDs or progress.
Nested operations may have their own entry/final checkpoints before the outer final checkpoint.
Exact sequences are deterministic for a fixed input/options/build but poll counts are not an API
compatibility promise across releases. No caller should use counts as a portable work-progress API.

`MeshError`, `StlError`, `TopologyError`, and `TransformedBoundsError` expose distinct `Cancelled`
variants containing the observed checkpoint and implementing `Error::source`. Subordinate mesh
cancellation is flattened to the outer STL/topology cancellation variant, not disguised as ordinary
mesh validation failure. Cancellation remains distinct from I/O, numeric, malformed-input, resource,
and allocation errors. Existing non-exhaustive error enums permit the additive variants.

A predicate returning true wins at that checkpoint, before the following action. Ordinary errors
encountered before another checkpoint remain ordinary errors; there is no extra poll on an error
path. For readers, the post-read poll occurs before inspecting the read result, so an observed
request there wins even if the read also returned an I/O error. `Interrupted` is retried when no
request is observed; it is never itself a request. At final publication, true drops the completed
temporary value and returns cancellation; a request not observed by that final check need not stop
success. This is cooperative observation, not atomic synchronization with an external request flag.

Cancellation publishes no successful partial mesh, report, diagnostic list, or bounds. Derived work
borrows immutable geometry; coordinates, triangle indices/winding, source bounds, metadata and
original diagnostics remain unchanged. Temporary allocations are released by normal ownership/drop.
Consumed external reader bytes cannot be rolled back. Successful retry with unchanged source input
and a non-cancelling controller is supported; a consumed reader must be repositioned/recreated.

### Cadence and explicit phase-level limitations

Record loops poll at indices 0,256,512,... and phase boundaries. High-incidence vertex/component
inner loops and edge grouping also poll; cancellation is not limited to an outer per-vertex loop.
Byte classification scans use at-most-8-KiB chunks; reader calls are at most 8 KiB under the
cap/probe rule, with checks before/after each call. These are work
intervals, not millisecond or strict CPU-instruction bounds; a record may include hash-table or
logarithmic disjoint-set/search work.

Individual edge, link/node, and component sorts and adjacent deduplication calls are uninterruptible
library phases, checked immediately before/after. Buffer reservation/initialization, individual hash
operations, UTF-8/string/line scanning and trimming, label copies, bulk copies, and cleanup can also
delay the next checkpoint. In particular, an overlong ASCII line is scanned to find its boundary
before its length is rejected; caller-increased limits enlarge this phase. Source caps do not turn
these limitations into a strict small cancellation-latency or resident-memory guarantee. No sorting
algorithm or geometry representation is replaced to promise stronger responsiveness.

An external blocking `File::open`/`Read` cannot be forcibly interrupted. Repeated `Interrupted`
responses have checks between attempts, but a call that never returns cannot reach another check.
Cancellation during allocation failure/cleanup is likewise not preemptive.

This section and TD-014 supersede earlier phase-time statements that cancellation was unimplemented;
those statements remain historical, not a deferral of this contract. CLI inspection, transform
round-trip coverage, native Windows validation of this increment, and whole-M2 closure subsequently
passed at the accepted M2 baseline. M3–M5 scheduling/progress and later GUI responsiveness are
separate work. Polling adds branches,
and enabled predicates add callback work; overhead has not been measured. The M2.4-C baseline is
historical for its recorded commit and is not rerun or claimed representative of this change.

## Cross-platform boundary

Paths, line endings, file replacement, memory mapping, threading, and graphics facilities can differ
between Windows and Linux. Platform-specific code belongs behind narrow adapters and must not leak
into geometric rules. WSL/Linux results never substitute for native Windows compilation and runtime
tests.



## M3 closure: shared on-plane edge crossings

TD-032 resolves a missing local normalization case discovered by the complete reference corpus.
Two distinct nonduplicate faces with coherent opposite edge uses, agreed noncollapsed coordinates,
valid ordinary rank context and opposite strict third-vertex Z signs produce one named on-plane
crossing candidate. Both source occurrences remain available. Candidate graph degree uses one edge;
raw multiplicity is two and path validation requires that exact count for this ownership. Same-side,
incoherent, nonmanifold, uncertified mixed-coplanar and conflicting cases retain unresolved outcomes.
TD-039 subsequently admits a coherent cap perimeter as described below. No source
mutation or inferred outward/embedded-solid certificate is involved. See the
[closure contract and evidence](M3-CLOSURE-ACCEPTANCE.md) for proof, corpus, budgets and validation.

## Implemented M4 LCD layer engine (TD-034)

The authoritative [LCD implementation contract](M4-LCD-IMPLEMENTATION.md) specifies profile validation,
coordinate permutation/reflection, pixel boundaries versus centers, arithmetic domain, M3 eligibility,
rational-to-raster conversion, half-open filling, midpoint AA and quantization. It also defines
rectangular grayscale morphology, exterior-zero semantics, clipping, resource accounting, cancellation,
preview statistics and no-cache streaming. The [PNG design](M4-PNG-DESIGN.md) defines bounded output
framing and failure behavior. No dependencies, new crates, GUI, exposure calibration, printer-specific
format or proprietary algorithm are introduced. Commercial unknowns remain research unknowns.

`raster::{rasterize_layer,rasterize_layer_with_cancellation}` accepts immutable owned M3 layers.
Complete strict layers use reviewed even-odd boundaries; attached constructed material uses its exact
material boundary set and explicit world/grid scale. An attached union resolves supported contacts
without pretending the original strict completeness changed. Projection is the explicit lossy
boundary; M3 vertices and topology are never rounded in place. Unsupported ranges/collapsed edges
return errors. No open/ambiguous contour can silently produce a plausible successful mask.

Requested-payload preflight includes edge conversion, ordered/active IDs, crossings, row counts,
output and compensation scratch together. The active-edge sweep avoids scanning irrelevant edges
per row; separable monotone extrema make compensation linear in pixels regardless of radius.
Sorting/allocation/bulk initialization cannot be interrupted internally and have boundary checks.
A work limit is an operation-charge limit, not a wall-clock deadline. PNG adds at most 65,535 bytes
of reserved sample scratch and fixed stack state, independently of raster working storage.

`raster::stream::RasterStream` is lazy and fused, stops on the first source/raster failure and records
completed image count. It owns no cache; output retention is explicitly caller-controlled. It accepts
any fallible owned layer iterator without adding GUI/framework coupling. Profile, settings, layer Z,
index, nonzero bounds/count and intensity sum travel with each image for preview. Intensities are
sampled geometric coverage, not emitted light, printable volume or a physical success guarantee.

## M5 jobs, persistence and exporters

`profiles` defines bounded schema-one printer/resin records and explicit printer-bound calibrations.
`profiles::store` separates portable data, configuration and disposable cache through native user
locations; profile updates lock the library, check revisions, preserve immutable history and publish
complete files. No commercial presets are bundled.

`print_job::PrintJob` owns immutable snapshots and a uniform sample schedule with complete overrides.
It resolves bottom/transition/normal settings lazily; sample Z and printer position Z remain distinct.
`export` consumes one owned raster at a time through a public registry/session interface. Exact
versioned exporter IDs and capability checks prevent extension-based format guessing. High-level
machine publication is create-only and transactional. Reference export streams PNG/JSONL and writes
a versioned manifest last. Bounded metadata is separate from mask and encoded-layer budgets.

Source coverage estimates and post-quantization encoded volumes have distinct meanings. Nominal
motion/exposure time is not firmware timing. See [M5 specification](M5-IMPLEMENTATION-SPEC.md),
[PP1 protocol](M5-PP1-SPEC.md) and [validation](M5-VALIDATION.md).

## M5.5-A preparation plans and history

`preparation` admits immutable source/placement/sampling and ordered operation records. Source
SHA-256 is asserted by the caller, not verified by this metadata module. Physical settings use world
millimeters after the canonical flattened placement. Full upstream content bytes bind support anchors;
upstream edits require explicit rebinding. Canonical records and shared immutable plans avoid mutable
source/settings aliases. No evaluated geometry or solid-validity claim is returned.

Bounded history counts current/undo/redo snapshots, publishes edits only after resource/cancellation
checks and increments owner-local generations through undo/redo to prevent stale background reuse.
Future evaluated geometry must pin the same canonical plan and recorded affine. The
[M5.5-A specification](M55-IMPLEMENTATION-SPEC.md) defines operation order, error/trust boundaries,
algorithm alternatives, subsequent geometry obligations and independent fixture budgets.

## M5.5-B convex shell geometry

`preparation::hollow` consumes one immutable source mesh and one-hollow plan, applies the recorded
affine placement, and constructs an inward cavity plus reversed cavity boundary in a shell mesh.
This first backend requires one embedded closed outward convex polyhedral solid. Private source
admission and candidate-hull topology use interval-filtered exact orient3d; no source repair/welding
or unproved nearest-normal inside classifier is used. The original placed exterior remains intact.

Floating clipping and explicitly bounded dyadic proposal snapping are separated from geometric
truth: an independently checked final convex boundary and interval supporting-halfspace inclusion
certificate must establish the requested erosion error before publication. Nonnegative source-plane
combinations tighten the lower bracket without assuming exact floating solves. Volumes are bounded
for the represented triangle geometry; erosion-parameter error is not a Hausdorff or physical-wall
strength claim. Headless cavity previews, named progress, explicit limits and cancellable atomic
results are available; cuts, supports and full prepared-scene integration remain later steps.
See [B contract](M55B-HOLLOWING-SPEC.md) and [validation](M55B-VALIDATION.md).

### Finite drainage CSG scenes

`preparation::drainage` evaluates immutable hollow-first plans followed by cardinal finite cuts.
Its `DrainedModel` owns a private boundary scene plus explicit material-source labels; it intentionally
provides no final Boolean triangle-mesh accessor. Only `layer_engine()` evaluates drained material.
The original hollowed model remains available as an explicitly uncut preview/comparison result.

Certified polygonal cylinders have independently bounded radial/axial approximation. Entire-cap
supporting-halfspace tests prove exterior-to-cavity connection and terminal clearance; uncertain,
missed, shallow, potentially overlapping or unsupported inputs return structured failures with IDs.
One accepted opening connects the single convex cavity geometrically, without asserting physical
resin removal. Exact planar difference preserves body parity and subtracts the union of cutter
parities; existing union APIs retain their behavior. Rasterization consumes the selected constructed
material through the compatibility `material_union()` layer accessor. See
[M5.5-C specification](M55C-DRAINAGE-SPEC.md) for admission, independent budgets and error meanings.

### Coherent coplanar cap perimeter (TD-039)

A complete raw source-edge group containing exactly one nondegenerate coplanar triangle edge and
one attached noncoplanar on-edge occurrence may retain `CoplanarPatchBoundary` ownership. Require
two distinct nonduplicate source faces, agreed noncollapsed source coordinates, opposite directed
edge uses, no independent incidence issue, and a strict third-vertex Z side on the attached face.
The latter plus a noncollapsed in-plane edge proves its ordinary nondegenerate rank. Extra cap/side
incidence, conflicts, winding errors, duplicates and degeneracy remain unresolved. Both occurrences
are retained. Path admission accepts the verified boundary's multiplicity two; graph degree still
counts one candidate edge. This resolves exact drainage terminal-cap planes without deleting
cutter context or bypassing general unresolved-source checks. It establishes a local patch
perimeter candidate, not global solid validity or outwardness.

### Parametric manual support scenes (TD-040)

`preparation::supports` preserves historical plan contracts and adds immutable `SupportPlan`/bounded
`SupportHistory` with full `KSUP1` recipe identity. Each station owns an explicit convex polygonal
section, world center and full right-handed rotation. Adjacent section hulls generate closed outward
components; varying stations controls base, shaft, neck, tip, inclination, bends, twist and taper.
Horizontal/returning paths above the platform are allowed. Exact post-placement rank checks prevent
shared sections collapsing to points/edges. Profiles are native polygonal shapes, not implicit smooth
surface promises. Components are previews/explicit parts, not a final Boolean mesh export.

The source is either one certified convex solid or the existing hollow/drain scene. Whole-component
halfspace certificates establish positive intended tip penetration through one source facet, platform
contact and conservative separation from unintended model/cavity/cutter contacts. Different supports
require separation unless explicit union is selected. A failure to prove separation is reported as
uncertainty rather than a proved collision. Identity includes full model revision and semantic
contact/clearance policy; stale contacts never automatically reproject or move with the model.

Prepared exact planar material is `(body minus union cutters) union additive component sources`.
Explicit high-bit labels separate additives from cutters, preserving all existing union/difference
policies and strict diagnostics. This material feeds M4 rasterization/M5 formats through current
interfaces. Source digests remain loader assertions; physical strength, arbitrary concave meshes,
GUI/runtime job coordination and broad end-to-end acceptance remain separately scoped. See the
[M5.5-D contract](M55D-SUPPORTS-SPEC.md) and [validation](M55D-VALIDATION.md).

### Preparation revision ownership and handoff (TD-041)

`PreparationSession` adds a headless editing boundary around immutable `SupportPlan` and bounded
`SupportHistory`. Owned tasks capture an immutable source, exact canonical record, generation,
request and opaque owner identity. Evaluation returns an opaque whole result/error; acceptance
checks all identities before publication. Current-only geometry is borrowed through Ready state;
retained preview exposes its original plan and components without a layer engine. Failure/cancel
retains completed preview while rejecting current use; stale workers cannot restore readiness.
The caller establishes source-byte/decoded-mesh correspondence and limits concurrent tasks.

Deterministic support stage/count observations are advisory; computed Complete precedes session
acceptance. Session start/publish checkpoints complement nested geometry cancellation. No threads,
GUI/persistence framework, new dependency or numerical kernel change is introduced. The existing
material expression feeds M3/M4/M5 unchanged. Composed geometric errors remain separate component
contracts, never a universal summed wall-thickness/Hausdorff bound. See
[E specification](M55E-INTEGRATION-SPEC.md) and [closure](M55-CLOSURE-ACCEPTANCE.md).

### Selected desktop presentation stack (TD-042)

The owner selected Rust with egui widgets, eframe application/window integration and the wgpu GPU
backend for M6. Implementation, dependency/version audit and native interactive validation are
pending. The recommended prototype is a focused custom wgpu viewport sharing eframe's device/queue
and using egui-wgpu callbacks, with render-to-texture where the chosen passes require it. A general
3D engine is not selected. See [TD-042](TECHNOLOGY-DECISIONS.md#td-042--rust-desktop-interface-with-egui-eframe-and-wgpu)
for rationale, alternatives and adoption gates.

The core retains authoritative f64 geometry and CPU slicing/export semantics. Display buffers,
cameras, clipping, picking feedback and GPU previews live in the presentation adapter; GPU f32
conversion requires explicit origin/range/error handling. Selection maps to authoritative entity
identity and revision. Background work uses the session ownership/current-publication contract;
preview and export cannot silently mix revisions. No GUI dependency is introduced into the core.
