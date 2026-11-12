# Workspace, crates and naming

Moved from the repository README on 2026-11-10 (owner): the README keeps what a reader needs to
build and use the software, and points here for the rest.

## Naming and compatibility

The product is **karasu-slicer**, formerly Resin Slicer. Application packages and source directories
are `karasu-cli`, `karasu-core`, `karasu-project` and `karasu-app`; Rust imports use `karasu_core`. New crates must use the
`karasu-<module>` prefix. `cargo run -p karasu-cli -- --help` selects the primary executable;
the former `resin-cli` binary remains a compatibility entry point with identical parsing, outputs
and exit statuses. Both display the new product identity. Package selectors use the new package
names; an existing Rust consumer can keep `use resin_core::...` through an explicit dependency alias:

```toml
[dependencies]
resin-core = { package = "karasu-core", path = "../karasu-slicer/crates/karasu-core" }
```

Only naming changes: geometry APIs, units, algorithms, formats, version, diagnostics semantics and
resource policies remain intact. The checkout directory is `/develop/dazero/karasu-slicer-analysis`; external
historical research paths retain their original names.
The existing `reference-fixtures` tool and version-1 fixture dataset/STL headers preserve their
historical identifiers and exact bytes. Archived research/validation records retain original names;
current API documentation and commands use Karasu names. See TD-033 for the migration boundary.

After relocating the checkout, rebuild workspace packages if Cargo reuses binaries with the old
`CARGO_MANIFEST_DIR`: `cargo clean -p karasu-core -p karasu-cli -p reference-fixtures`, then
`cargo test --workspace` and `cargo build --workspace`. This removes package artifacts, not sources,
fixtures or Git history.

## Workspace

The owner-approved expanded MVP includes **parametric hollowing, manual drainage holes and editable
manual 3D supports**. These are mandatory capabilities of **M5.5 — Model Preparation Engine**,
with interactive editing in M6 and end-to-end acceptance in M7. Plan/history contracts and bounded
convex hollowing, cardinal finite drainage cuts and parametric manual supports are implemented.
Combined headless preparation is accepted in [M5.5 closure](M55-CLOSURE-ACCEPTANCE.md).
Automatic support generation, advanced island/overhang optimization and auto-orientation remain
Post-MVP. M5.5 retains immutable sources and reversible preparation settings/history, generates
validated derived geometry for the slicing pipeline, and reports unsupported/ambiguous cases rather
than promising universal hollowing, physical drainage or support strength. Necessary internal geometry
for hollowing/cuts is part of MVP even if generalized Boolean workflows remain Post-MVP.

The roadmap's M5.5 placement is engineering order: preparation precedes M3 slicing, M4 rasterization
and M5 export at runtime. The bounded preparation representation is documented below; no project-file
format is introduced. M1 remains open; M2 and M3 are accepted within their documented contracts and
validation scope.
See [ROADMAP.md](../ROADMAP.md), TD-020 and PREP-001–009 for requirements, stages and acceptance.

- `karasu-core` is the reusable, headless library; it provides immutable indexed source meshes,
  explicit-policy STL import, non-mutating object/affine transforms, and deterministic topology
  reports, plus the local section kernel and raw single-plane mesh traversal, without repair or
  placement.
- `karasu-project` is the headless orchestration shared by the command line and the desktop
  application: the slicing request and the complete pipeline, with caller-supplied progress and
  cancellation (TD-070); the in-memory project model (library of embedded models, plates with
  their own printer, resin and settings, revisions) and background jobs (TD-071).
- `karasu-cli` is the headless command-line adapter (`inspect` and `slice`); `slice` parses its
  options into a `karasu-project` request and runs the shared pipeline.
- `karasu-app` is the desktop application (M6, egui, eframe and wgpu), its executable `karasu`:
  `cargo run --release -p karasu-app -- [FILE...]`.
  - **Viewport (M6.2, TD-073):**
    - Drop models and `karasu` printer or resin profiles (JSON) on the window, or name them on
      the command line. Models import in the background, centred and dropped onto the plate;
      STL is read in millimetres, with a notice.
    - Drag with the primary button to orbit, the secondary or middle button to pan; scroll to
      zoom; "Fit view" frames the build volume.
    - Click an object to select it. Only its handles move it, so dragging across an object
      orbits the view.
    - M, R and S show the handles (TD-074):
      - move: the arrows along X, Y or Z, or the yellow square at the origin for free moves on
        the plate;
      - rotate: about X, Y or Z, with Shift snapping turns to 15 degrees;
      - scale: along the object's own X, Y or Z, or uniformly.
    - Delete removes the object, and Escape clears the selection.
    - An object moved up stays above the plate until "Drop to plate", and never goes below the
      plate. The status bar warns that slicing a floating object needs supports.
    - Objects outside the build volume are shown in grey, and the status bar counts them.
  - **Core workflow (M6.3, TD-077):**
    - "Open…" opens models and profiles in the native dialog.
    - Add your printers from the catalog (`assets/catalog`, grouped by vendor, with their
      sources) or as custom printers; then choose among your printers, your resins set up for
      the printer, and their print settings (TD-106). Catalog settings are manufacturers'
      starting points, not verified calibrations.
    - The library window edits, duplicates, exports, imports and deletes your profiles, and
      offers a newer catalog revision without applying it on its own.
    - Choose the output format, then Slice. A progress card with Cancel follows the job, and the
      status bar shows layers, time and resin.
    - The layer panel shows each exported layer's mask exactly as written.
    - "Export…" saves the file where you choose.
  - **Undo and preparation previews (M6.4a, TD-081, TD-082):**
    - Undo and Redo (Ctrl+Z, Ctrl+Shift+Z or Ctrl+Y) step through the plate's edits; a drag is
      one step.
    - A plate with hollowing or supports is prepared in the background shortly after each edit;
      supports are drawn in the viewport, in red when they have an error, and the status bar
      shows the progress and what cannot be prepared.
  - **Hollowing and drain holes (M6.4b, TD-083):**
    - Select an object: its preparation panel hollows it (wall thickness, grid, simplification)
      and shows the certified wall, the volumes and which cavity regions drain.
    - "Add drain holes", then move over the model: the hole's solid follows the pointer, green
      where it can be made and red where it cannot (the panel says why). A click places a green
      hole; placed holes are blue.
    - Click a hole's solid to select it and drag it along the surface. Its handles set the
      diameter (rim), the depth (end) and the tilt (the ring on its axis); the panel has the
      same fields. Delete removes it. Holes move with the object.
    - Holes behind the model show as a faded ghost.
  - **Section panel (M6.4b3, TD-086):** the narrow panel at the viewport's right cuts the models
    between two heights. Drag the lower handle up or the upper handle down; heights snap to
    layers. Cut solids are filled, so a hollowed model shows its wall around the cavity.
    Double-click the slider to show everything.
  - **Plate front (M6.4b4, TD-087):** a tab marked "Front" shows the plate's front edge; the
    layer panel shows each layer from above with the front at the bottom, whatever the printer's
    panel orientation.
  - **Plate from below (M6.4b4, TD-088):** turning the view below the plate fades its surface
    and grid out, so the models' undersides stay clear; a border keeps the plate's limits.
  - **Object repairs (M6.4d r8, TD-092):**
    - Every opened model gets the repairs chosen in the Repair menu's "Repair opened models"
      (default Exact, which moves no vertex; "Diagnose only" leaves them to the plate), as its
      own; the message says so.
    - The selected object's panel starts with its Repair section: the state (ready, ready with
      notes, slices but hollowing needs a stronger repair, needs repair), each defect with what
      slicing will do about it and "Show", which turns the view towards it; the repairs (the
      plate's, Off, Exact, Standard, Aggressive, Rebuild with its cell, or custom stages with
      their tolerances); what they changed; "Try" the next stronger preset when the state is not
      ready; and "Remove repairs". Every change is one undo step.
    - The selected object's defects and repairs show as markers through the model, coloured by
      what they mean: red where slicing fails, orange where only hollowing is blocked, grey where
      slicing copes, green where a repair changed the model. The pointer over a marker names
      it; the section has a legend and "Show markers on the model" hides them.
    - A slice that fails because a layer does not close, or for the defects slicing refuses,
      offers "Repair: Standard" first; reversed slivers also offer "Repair: Rebuild as a solid",
      last.
    - Every tracked fixture reaches slicing this way (test).
  - **Repairs and messages (M6.3, TD-078):**
    - Every opened model is checked for defects (inside-out bodies, holes, faces pointing the
      wrong way, duplicate faces) without being changed; a message says what the plate will do
      about each.
    - The Repair menu sets the plate's repairs: merge duplicate faces, make orientation
      consistent, turn inside-out bodies outward, fill holes, close holes in each layer, or
      slice despite defects.
    - The status bar shows the latest message in one line; "Messages" lists them all with their
      details. A failed slice explains its cause and offers a fix that slices again.
    - Self-intersecting folds can leave tiny inside-out slivers in a layer. New plates leave up
      to 0.1 mm² of them empty per layer (Repair menu) and report them; a larger inside-out
      area still fails (TD-079). On the command line, `--reversed-tolerance <mm2>` does the same.
    - The interface texts live in Fluent catalogs (`assets/locales`, TD-080). The language is a
      setting (only `en-US` exists yet); `--locale <tag>` chooses it for one start,
      `--locale pseudo` shows a pseudo-locale for testing, and `--locale-dir <folder>` loads
      catalogs from `<folder>/<tag>/*.ftl` without rebuilding.
    - File › Settings… (Ctrl+,) sets the viewport's colours, the keyboard shortcuts, the repair
      of opened models, how the window opens and the language. Every setting, and the layout
      as the user leaves it, is kept across restarts in `settings.json` in the preferences
      folder (TD-097); a file that cannot be read is reported and never written over.
    - `--dev` is the developer mode: the settings list the pseudo-locale among the languages.
  - `--gpu-backend auto|vulkan|dx12|metal|gl` selects the backends; `auto` uses OpenGL only when
    no modern backend can present.
  - `--window windowed|maximized|fullscreen` chooses how the main window opens this time; the
    setting decides otherwise (by default as it was closed, TD-075, TD-097).
  - At startup the window shows a borderless splash. It has the owner's
    artwork `assets/ui/images/splash.png` with the name, the version and the progress of the real
    startup steps, and stays at least 1.5 s. Then it becomes the main window (TD-069, TD-141).
  - `--smoke-test` goes through the splash, renders a few frames of the main window, prints the
    adapter, the splash time and whether the surface supports transparency as JSON, and exits.
    With files, it waits for their import and adds a measured 90-frame camera orbit (frame times
    and triangles); a failed import exits with status 4.
  - On Windows, Release builds are GUI applications, so no console window opens. Their help,
    errors and smoke-test output are visible only when standard output and error are
    redirected, for example
    `Start-Process karasu.exe -ArgumentList '--smoke-test' -Wait -NoNewWindow -RedirectStandardOutput out.txt`.
  - Linux builds need the usual X11/Wayland, Vulkan or OpenGL system libraries at run time.
- `docs/` records architecture, testing, technology, and reference-research policies.
- `ROADMAP.md` defines milestone scope and acceptance criteria.

The core remains independent of presentation frameworks. The desktop GUI will use **Rust, egui,
eframe and wgpu** (TD-042); implementation and native GPU validation belong to M6. A focused custom
wgpu 3D viewport integrated with eframe is the recommended prototype, not an implemented feature.
The approved desktop color palette is recorded in [UI design](UI-DESIGN.md).

The mesh/import API and its unit, detection, resource, normal, and numerical policies are documented
in [docs/ARCHITECTURE.md](ARCHITECTURE.md), alongside M2.2's right-handed, column-vector,
local-pivot transformation convention and M2.3 topology contracts. `TriangleMesh::analyze_topology()`
returns separate incidence, vertex-manifold, local-winding, and closed-combinatorial findings.
Degenerate faces remain preserved and make affected neighborhoods indeterminate; duplicate faces
retain their incidences. Even a closed combinatorial surface is not a verified printable solid:
self-intersections, outward orientation, volume, nesting, and thickness are not established.
Core transforms never infer printer placement or proprietary GUI rules.

The core-only `slicing::section_mesh_z(&mesh, plane_z, transform, limits)` and additive
`section_mesh_z_with_cancellation(..., &mut cancellation)` return an immutable `MeshSection`.
`SectionTransform::{Identity, Object(&ObjectTransform), Affine(&Affine3)}` explicitly selects the
source-to-world mapping; no implicit flattening, grounding or sampling is performed. `SectionLimits`
bounds source faces, retained events and requested event-buffer payload bytes. All non-None raw
events remain in source-face order, including tangencies, coplanar triangles, duplicates and collapsed
segments. Face records retain source indices and resolve corner/edge provenance without welding.
This is not a contour, filled region or printable-solid result, and no slicing CLI is introduced.
See the architecture for numerical errors, post-transform context and cancellation limitations.

`slicing::normalize::normalize_section(&raw, limits)` and its additive cancellation-aware counterpart
borrow that raw section unchanged. Indexed vertices and unordered source-edge pairs define identity;
coordinate coincidence never welds unrelated features. All point observations and segment occurrences
remain accessible, including internal coplanar triangulation edges, duplicates, collapse and tangency.
Only a verified two-triangle local coplanar patch receives internal-edge classification; all unresolved
ownership/conflicts carry structured diagnostics. This is not a filled-region or global validity test.
Combined native Windows validation of M3.2-A/B subsequently passed in the
[consolidated M3.2/M3.3 campaign](TEST-STRATEGY.md#subsequent-consolidated-native-windows-m32m33-validation).

M3.3-A adds `slicing::graph::{build_incidence_graph, build_incidence_graph_with_cancellation}` over
borrowed normalization with `GraphLimits`. It retains every source-feature/group/occurrence reference
and exposes separate complete and locally eligible adjacency, degree/multiplicity counts and ordered
conflict/unresolved/endpoint/branch diagnostics. Internal coplanar and unresolved groups never enter
candidate adjacency. Equal coordinates do not connect different features; crossing interiors are not
split. This is not contour extraction, closure, orientation, a filled region or a printability claim.
Native Windows validation of the graph subsequently passed in the consolidated campaign linked
below; path extraction is separate below.

M3.3-B now provides `slicing::paths::{extract_paths, extract_paths_with_cancellation}` over the
immutable graph with `PathLimits`. It produces canonical open chains, provisional graph-simple cycle
candidates and ambiguous boundary-stopped trails, plus candidate components and an outcome for every
complete group. Branches and unresolved neighborhoods stop continuity; complete-only groups and raw
multiplicity stay inspectable. No paths are artificially closed, welded or routed through branches.
Canonical direction uses graph IDs, not clockwise/counterclockwise orientation. Even a clean graph
cycle can self-intersect or have zero area in XY; polygon validity and regions remain M3.4 work.
Consolidated native Windows validation of M3.2-A/B and M3.3-A/B passed at committed HEAD
`c4865117e18d998fe80d48d8fac1022b5731ebc8`: 440 passed, zero failed and two optional ignored.
The [validation record](TEST-STRATEGY.md#subsequent-consolidated-native-windows-m32m33-validation)
distinguishes the owner-supplied `mesh_section` Release run from machine-captured checks and retains
earlier host-policy failures. This scoped validation does not close M3.3 or M3.

M3.4 entry research/design is documented in the
comparative research and
[independent design](M3-PLANAR-VALIDITY-DESIGN.md). The owner approved TD-023's narrow staged
exception for detection-only M3.4-A, now implemented in `slicing::validity`. Ordinary and
cancellation-aware `inspect_planar_paths` borrow immutable paths and report exact represented-XY
contacts, collapsed edges and local cycle embedding, with bounded work/storage and atomic errors.
Open/ambiguous paths stay ineligible; cross-path contacts and unassessed complete-only context stay
explicit. A simple cycle is not valid material. Integer conversion, nesting,
regions, repair and layer scheduling remain unimplemented and independently gated.
Scoped M3.4-A validation passes Linux/WSL 459/0/2 and native Windows 458/0/2; the
[test record](TEST-STRATEGY.md#implemented-m34-a-planar-embedding-coverage) retains earlier
host-policy/capture failures separately. This does not close M3 or M1.

M3.4-B adds `slicing::area::{measure_cycle_areas, measure_cycle_areas_with_cancellation}` under
owner-approved TD-024. It borrows that inspection, measures only existing SimpleCycle paths and
retains skipped paths and all diagnostics/provenance. Exact current traversal orientation is separate
from the positive nearest-even area magnitude in mm^2: finite, RoundedToZero or Overflow. No ring
is reversed or assigned an outer/hole role; no summed material area or physical grid is introduced.
The [contract](M3-AREA-ORIENTATION-CONTRACT.md) defines bounds, errors, rounding and cancellation;
[tests/platform evidence](TEST-STRATEGY.md#implemented-m34-b-areaorientation-coverage) distinguish
current scoped acceptance from still-open construction, regions, scheduling and M1/M3 gates.

M3.4-C adds `slicing::grid::{map_exact_grid_vertices, map_exact_grid_vertices_with_cancellation}`
under TD-025. Supply an explicit validated `BinaryGridSpec` (world-XY origin and binary step exponent)
and `GridLimits`. The borrowed view returns one outcome per original graph vertex: exact integer XY
or a retained unresolved coordinate conflict. Every agreed point, including isolated/complete-only
context, must be exactly on-grid and within the fixed integer domain, otherwise the whole operation
fails with vertex/axis context. There is no default origin/scale, rounding, welding, inverse decoder,
integer area, repair or material shortcut. Count/work/requested-byte caps and atomic TD-014
cancellation preserve the original inspection and source. See the
[contract](M3-INTEGER-GRID-CONTRACT.md) and [test strategy](TEST-STRATEGY.md).

M3.5-A adds `slicing::regions::{assemble_strict_regions,
assemble_strict_regions_with_cancellation}` under TD-026. It borrows matching immutable planar,
area and ExactOnly-grid reports; accepts only non-contacting simple cycles with exact grid vertices;
and derives a deterministic `EvenOddStrict` containment forest. Boundaries retain path identity,
depth, parent, current orientation, role and a nonmutating reverse-view flag. Open, ambiguous,
non-simple, coordinate-conflicted and contacting paths remain explicit non-boundary outcomes, while
unresolved complete-only context remains diagnostic data. The
[contract](M3-REGION-HIERARCHY-CONTRACT.md) defines the exact half-open integer ray rule,
limits and atomic cancellation. No layer schedule, construction/union, repair, raster or printability
claim is added; the conditional overlay decision and whole-M3 validation remain open.

M3.5-B adds `slicing::schedule::{UniformScheduleSpec, ScheduleEnd, ScheduleLimits,
build_layer_schedule, build_layer_schedule_with_cancellation}` under TD-027. An explicit finite
first plane, positive height and `Count`/`Before`/`Through` rule define an immutable uniform plane
plan. Exact dyadic terminal comparisons precede one nearest-even binary64 rounding; u64 layer
indices are contiguous and published Z values strictly increase. Range/resolution, count/payload
limits, host storage failures and cancellation produce atomic typed errors. No printer/default
plane, variable height, exposure, geometry orchestration or new CLI command is selected. See the
[schedule contract](M3-LAYER-SCHEDULE-CONTRACT.md). Native Windows validation and whole-M3
acceptance remain separate; EXP-03 and M1 are open.

M3.6 adds `slicing::engine::{LayerEngine, LayerConfig, collect_layers}` under TD-029. It composes
the reviewed stages over an immutable source, transform and schedule, yielding one fully owned
`RegionLayer` at a time with flat geometry, raw provenance, ambiguity outcomes and diagnostics.
Operational errors retain their stage/plane context and fuse the stream. Synchronous progress counts
completed layers; normal exhaustion is distinct from yielding the last layer. An optional fresh-engine
collector applies separate layer/aggregate payload caps and publishes only a complete collection.
Open/contacting/ambiguous geometry remains explicitly unresolved; exact-grid errors are never repaired.
See the [layer engine contract](M3-LAYER-ENGINE-CONTRACT.md). A nested-layer regression also
corrects the existing strict containment ray's upper-Y bound. No construction, raster, export or new
CLI command is introduced. Native Windows and whole-M3 acceptance remain separate work.

M3.5-C adds `slicing::construct::union_material_rings` and the engine's explicit
`with_material_union(face_materials, limits)` option under TD-030. Material is even-odd within a
caller-named source and union across sources; source identity is never inferred. Exact rational
intersections, contact/coincidence provenance, material-left cycles and outer/hole hierarchy remain
in integer-grid units without snapping or floating reconstruction. The owned strict layer and its
diagnostics are preserved; `material_union()` exposes the separate constructed result only after
successful composition. Self-contacting input rings, open/ambiguous source paths, mixed ownership
and off-grid input remain typed failures. See the [contract](M3-CONSTRUCTIVE-MATERIAL-CONTRACT.md)
and [M3 audit](M3-COMPLETION-AUDIT.md). Native Windows/corpus/budget acceptance remains separate.
