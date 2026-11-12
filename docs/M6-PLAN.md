# M6 plan: desktop application

**Status:** proposed 2026-10-19 and accepted by the owner as the M6 plan (TD-067). Phases may be
refined as M6 progresses; open decisions are listed at the end.

**Progress:**
- **M6.0 is done** (TD-068): dependency review, themed window, adapter selection with the OpenGL
  fallback, smoke tests on Linux and native Windows; startup splash (TD-069).
- **M6.1 is split into three steps:**
  - **M6.1a, done (TD-070):** the shared pipeline in `karasu-project`. CLI outputs are
    byte-identical on a 20-case suite.
  - **M6.1b, done (TD-071):** the project model (library of embedded models, plates with their
    own printer, resin, settings and objects, revisions) and background jobs with progress and
    cancellation.
  - **M6.1c, done (TD-072):** supports follow their object through moves on the plate and turns
    about the vertical axis; tilts, scale and mirroring changes are refused.
- **M6.1 is complete.**
- **Owner review of M6.2 (TD-074):** RGB gizmo axes and a `#fcbf87` model colour as defaults,
  all interface colours configurable in the settings (to be built with the layout and style
  work, M6.5), free and Z moves, per-axis scaling, handles that follow the object. The graphics
  as a whole will be revisited later.
- **Window placement (TD-075):** the splash and the main window open centred, and the main
  window opens maximized by default (windowed and fullscreen are the alternatives, a future
  setting).
- **Settings to build** (with the layout and style work, M6.5): the interface colours
  (TD-074) and the window's start mode (TD-075).
- **M6.2, done (TD-073):** the wgpu viewport, opening files by drop or command line, picking and
  the move, rotate and scale gizmos. FX-12 orbits at the display refresh rate on native Windows
  (Vulkan and OpenGL).
- **M6.3, done (TD-077):** the first complete use from the interface: import, printer and resin
  presets and the user library with a basic editor, slicing with progress and Cancel, the layer
  panel of exported masks, and export through native dialogs. The refinement (TD-078) adds the
  Repair menu, the diagnosis of opened models and the Messages window.
- **M6.3b, done (TD-080):** localisation foundations: Fluent catalogs (`en-US` complete), a
  pseudo-locale, and tests. M6.5 adds the language setting, locale number formatting in fields
  and a workflow bar that fits longer translations.
- **M6.4 is split into steps:**
  - **M6.4a, done (TD-081, TD-082):** foundations. Undo and redo of plate edits; headless
    preparation previews in the background, with support findings as data; picking with the
    surface normal; supports drawn in the viewport.
  - **M6.4b, done (TD-083):** the preparation panel (wall, grid, simplification, what was
    certified), the section view of the cavity, and drain holes placed by clicking the surface,
    edited, removed and carried along when the object moves.
  - **M6.4b2, done (TD-084, TD-085):** after the owner's reviews, holes are solids checked
    while they are placed (green or red, following the pointer), selected and dragged by their
    solid, and widened, deepened and tilted with handles or in the panel.
  - **M6.4b3, done (TD-086):** the section panel at the viewport's right (two handles, snapped
    to layers), cut solids capped so a hollowed wall shows its thickness, and hole solids hidden
    behind the model with a faded ghost.
  - **M6.4b4, in progress (TD-087, TD-088):** the owner's review of the viewport and its
    rendering, point by point. Done so far:
    - the plate's front tab, and layers shown as the plate;
    - the plate's surface and grid fading out from below, with a border that stays.
  - **M6.4c, done 2026-10-21 (TD-089, TD-090):** hollowing and drain holes rebuilt as operations
    on an untouched model, in its model frame; the object's operation list for the timeline. See
    [the M6.4c plan](#m64c-plan-hollowing-and-drain-holes-as-operations).
  - **M6.4d, done 2026-10-21 (TD-092, r1 to r9):** mesh repair: every repair, exact first, then with tolerances the
    user sets, up to rebuilding the surface; never silent; tolerant hollowing; a guided flow from
    import to a printable model. Phases r1 to r9 in the [repair plan](REPAIR-PLAN.md).
  - **M6.4e, done 2026-10-22 (TD-093, h1 to h5):** certified fast hollowing: a predicted cell
    and offset with an exact per-triangle wall certificate, so any wall is guaranteed in seconds.
    Phases and results in the [plan](HOLLOW-CERTIFIED-PLAN.md).
  - **Order changed (owner, 2026-10-23):** the interface has grown with M6.4's features and is
    getting cluttered, so it is reviewed before manual supports. The order is now:
    1. **M6.4f:** a viewport review, on the owner's list of changes from trying the
       application. The first list (2026-10-23):
       - *Zoom towards the pointer:* the wheel zooms at the surface under the pointer, or at
         the target's depth where there is none. The point is picked once per wheel gesture,
         and the target moves towards it, so panning keeps its pace near the model.
       - *Deeper zoom:* the closest distance drops from 1 mm to 0.05 mm, enough to fill the view
         with a miniature's detail.
       - *View cube:* in the viewport's top right. Its faces, edges and corners give 26 views,
         reached with a short eased turn. Hovering lights every cell of a view together (both
         sides of an edge, all three around a corner), and the face names are printed on the
         faces, turning with them. A home button beside it returns to the starting view
         fitted to the plate. The preparation panel now opens below it.
       - *Selection outline:* the selected object's whole silhouette is outlined on the GPU,
         over everything and uncut by the section, so a section view still shows the object's
         shape. Its colour and whether it is shown are appearance settings, for the settings
         panel to come.
       - *Colours (test):* a temporary window edited the viewport's colours live; the settings
         dialog (M6.5 u3, TD-097) took it in and it was removed. The plate, grid, border,
         volume edges and front tab are built again when their colours change.
       - *Colours chosen with it (owner, 2026-10-23):* models `#b8b0a8`, the selected model
         `#d9a088`, section caps `#f5f5f5`, selection outline `#c8c8c8`, free move `#ffff00`
         at alpha 50 of 255, cavity `#8d8a7e`. The selected model's cavity has its own colour
         (`#9c6b58` by default), so a hollowed model reads as selected in the section view
         too.
       - *Outside the build volume:* a model keeps its colour, and the parts of its surface
         outside the volume take a surface filter (colour, opacity, and a solid or striped
         pattern), red on models and dark stripes on the selected one, as in Lychee Slicer.
         The test is per fragment on the GPU, so the filter's edge follows the volume's faces
         and nothing is uploaded when a model moves. Overhangs are meant to use the same
         filters. Both styles are in the settings dialog's Appearance.
    2. **M6.5:** the layout, style and interface review, brought forward
       ([plan](#m65-plan-layout-and-interface-review)). It takes in the interface backlog: the
       layer viewer and the verification map, the live log window, the repair menu, model
       tinting, and the CPU contention between diagnosis and preparation.
    3. **M6.4g:** manual supports added by clicking, edited and removed (base, preset, raft,
       braces), with each support's findings shown on it, as timeline operations. Adaptive
       cavity extraction is deferred after the MVP (owner, 2026-10-26: the slicer copes, and
       its gain is the machine's performance, not the print). Before the supports (owner,
       2026-10-26): how to compute the models' overhangs, to show them, to guide manual
       supports, and later for automatic orientation and supports; the critical angle is 45°
       by default and set by the user. Whether automatic orientation and supports come into
       M6 is decided after the manual supports. Done on 2026-10-30, with native Windows
       validation passed for bcbb38d (TD-108).
       The Supports step follows the owner's mockup (TD-108, revised 2026-10-27): the core and
       pipeline, the Supports panel, its four tools (Supports, Islands, Braces, Raft with the
       lift), the handles, the user's presets, the automatic foot, and branches and trees.
    4. **M6.4h:** the ROADMAP's mandatory preparation scenarios from the interface, on Linux and
       native Windows (TD-111, [record](M64H-SCENARIOS.md)): automated in `karasu-app`, and
       an interactive checklist for the owner.
    5. **M6.4i, M6.4j, M6.4k** (owner, 2026-10-30, TD-110): automatic supports (overhangs,
       density, a closing check), suction-cup detection, then automatic orientation.
       M6.4j's analysis and command line were done on 2026-10-31 (TD-114,
       [specification](SUCTION-CUP-SPEC.md)). The Drain holes tool's part followed from a
       mockup; native Windows validation passed on 2026-11-02 (c528394), with the hollowing
       blockers (TD-115); a physical print remains. Contacts fitted to the surface (TD-116)
       followed on 2026-11-02, for the three support tools. M6.4k, the Orient tool (TD-119),
       was implemented on 2026-11-03 and awaits the owner's trial.
    6. **M6.6** plates and project file, then **M6.7** hardening and distribution, then
       **M6.8** a further visual review of the interface before the release milestone (owner,
       2026-11-05).

**Inputs:**
- [ROADMAP](../ROADMAP.md): M6 scope, acceptance criteria and Definition of Done.
- [TD-042](TECHNOLOGY-DECISIONS.md#td-042--rust-desktop-interface-with-egui-eframe-and-wgpu): egui,
  eframe and wgpu, with a custom wgpu viewport.
- [UI design](UI-DESIGN.md): the approved palette and the owner's UI/UX concept.
- The engine capabilities through TD-066.

## Goal

A native Windows and Linux desktop application in which a user can:
- import, position, prepare (hollowing, drain holes, manual supports), slice, inspect the layers and
  export;
- do so on projects made of several plates, each with its own printer and resin.

All algorithms stay in the headless core; the GUI owns none.

## Architecture

**Two new crates, and no more** (`AGENTS.md` forbids speculative crates):

| Crate | Content | GUI dependencies |
| --- | --- | --- |
| `karasu-project` | Project model (project → plates → instances; model library), the per-plate preparation and slicing pipeline, background jobs with cancellation and revision numbers, project persistence | none |
| `karasu-app` | eframe/egui application: wgpu viewport, panels, docking layout, theme | egui, eframe, wgpu, egui_dock |

**Shared pipeline:**
- The CLI's orchestration (import → placement → hollowing → supports → plate → slicing → export)
  moves into `karasu-project`, so the CLI and the application run the same code.
- Acceptance: CLI outputs stay byte-identical on the 16 cross-platform cases and the
  TD-064/TD-065/TD-066 cases.

**Revisions:**
- Every background result carries the revision of the plate it was computed from.
- An older result never replaces a newer edit; the UI shows the plate as stale instead (TD-041,
  ROADMAP).

**Data model from the start (owner decision):**
- The application state is "project → plates (printer, resin, objects, settings)" from M6.1, even
  before the plate overview exists.
- A plate maps to one headless job; it extends the TD-065 plate document with printer and resin
  references.
- Models live in a project library (each source loaded once, identified by its SHA-256), with
  per-plate instances.
- Preparation is per plate by default, with explicit "copy preparation from…" actions.

**Supports in the object frame:**
- Support documents are bound to a world placement today. They are generalized to the object's
  frame, so supports follow the object through moves, rotations and copies to other plates.
- The CLI keeps accepting today's documents.

## M6.4c plan: hollowing and drain holes as operations

**Decisions:** [TD-089](TECHNOLOGY-DECISIONS.md#td-089--hollowing-and-drain-holes-as-operations-on-an-untouched-model)
and [TD-090](TECHNOLOGY-DECISIONS.md#td-090--the-object-timeline), owner decisions of
2026-10-20.

**Goal:** moving, turning or scaling a hollowed object, and adding or editing a drain hole,
never hollows it again. Slicing reuses the cavity. Holes are checked along their whole length.
Every operation can be removed exactly.

**Phases** (each with its tests and gates; core phases first):

| Phase | Content | Verifiable result |
| --- | --- | --- |
| **c1 Model-frame hollowing** (core), **done 2026-10-21** | Hollow the source in its model frame; the cavity as a kept artifact with an input key; the placement applied afterwards; scale and mirror accepted; the scaled wall range reported | A rigid transform reuses the cavity bit for bit; same key, same bytes; the scaled wall range is exact for uniform and per-axis scales |
| **c2 Independent drain holes** (core), **done 2026-10-21** | Holes in the model frame with a depth set by the user, from the outer surface or from the cavity's surface; no cavity search; overlaps allowed and subtracted as a union per layer; holes indexed by height, with a large explicit bound set by measurement (1024 or more); whole-length findings (through both sides, no cavity reached, ending in material, not opening outside, enclosed regions); only a diameter below the printable minimum is refused | A through hole slices as one opening on both sides; overlapping holes slice as their union; a hole without hollowing slices; a hole from inside opens the wall; the findings are reported as data; 1024 holes slice within a measured time |
| **c3 Operations and cache** (project), **done 2026-10-21** | Each object's ordered operations with identities and enabled flags (TD-090); apply, remove and replace; the cavity kept by key and shared by previews and slicing; the plate document's new version; undo integration | Slicing a prepared object runs no hollowing (measured); removing hollowing gives the same layers as an object never hollowed, byte for byte |
| **c4 Interface** (app), **done 2026-10-21** | Apply and remove hollowing for the selected object; the pending state of edited parameters; the scale message; the apply-holes workflow with today's live preview; holes as real openings by GPU subtraction; previous cavity and holes shown dimmed while computing; the preparation panel no longer covering the layer panel | Moving or turning a hollowed 700,000-triangle model computes nothing (measured); a hole is checked within 100 ms on it |
| **c5 CLI, formats and records**, **done 2026-10-21** | The CLI keeps its options, with world holes converted to the model frame; the comparison baseline regenerated for hollowed cases and recorded, and every other case byte-identical; benchmarks before and after | The comparison suite and the benchmark record |
| **c6 Validation**, **done 2026-10-21** | Linux gates and native Windows: the owner's scenarios on the Yorick miniature (hollow, holes, move, rotate, scale, slice) | Recorded native run |

**Out of scope:** the timeline panel (M6.5), hollowing blockers (with or after the timeline), 3D
mesh Booleans and exporting hollowed meshes (a later decision), and manual supports (M6.4g, after
the repair milestone M6.4d of TD-092, the hollowing milestone M6.4e of TD-093, the viewport review
M6.4f and the interface review M6.5).

**Risks:**
- **Bit-identity under rigid transforms:** this is why the cavity is computed in the model
  frame. The tests compare the transformed cavity, not a recomputation.
- **Checks along the whole length:** the cylinder's side must be tested against the source and
  the cavity surfaces with the existing surface indexes, within the 100 ms interactive budget.
- **GPU subtraction of many overlapping holes:** the cost is bounded by drawing the holes'
  convex cylinders; the timing is measured on many holes.

## M6.5 plan: layout and interface review

**Approach (owner, 2026-10-23):** the owner first decides a general layout; the interface then
moves into it step by step, each step tried by the owner on Windows before the next. M6.5 sets
the layout, the style and the settings, and clears today's backlog; features the core gains
later (manual supports first) get their interface in the places the layout keeps for them, so
the interface work continues after M6.5.

| Step | Content | State |
| --- | --- | --- |
| **u1 Inventory and actions** | [Interface inventory](UI-INVENTORY.md) of everything shown today, persisted or not, with the findings for the review. `actions`: one list of actions, enabled and performed in one place, and a `Keymap` of configurable shortcuts (TD-097) | done |
| **u2 Layout** | The owner described the layout (2026-10-23), recorded as TD-098: a menu bar; a workflow bar whose steps show their tools, which open fixed panels at the viewport's right; a left sidebar, closable sideways, with the printer, the resin and the objects; the section bar with a typed layer; opaque panels with larger buttons; no docking crate. Open points are settled as the steps reach them | in progress |
| **u3 Settings** | The settings store: every user setting kept across restarts in a versioned file in the preferences root (TD-097), a file it cannot read reported, never silently replaced. The settings panel: colours (replacing the colours test window, plate and grid live too), shortcuts, auto-repair on import, window mode, language | Done 2026-10-26: the store, then the settings dialog (TD-097) |
| **u4 Skeleton** | The menu bar (File, Edit, View, Windows, every entry an action); the workflow bar (steps Prepare, Hollowing, Supports, Export, centred, a fading rule, the step's tools centred, collapsible to the steps and a crumb of where the user is); the closable sidebar with the printer and resin and their facts, and the objects (select, delete); the tool panel docked under the view cube, collapsible and scrolling, holding today's Preparation window's parts, the plate's repairs (no longer a menu that closes under the pointer) and slicing. Only existing tools are offered: Supports is disabled until M6.4g | done, to be tried by the owner |
| **u5 onwards: migration** | One area per step (tools to add to the steps, section bar with a typed layer, the slicing and layers panel after the owner's review, messages and the live log window (done 2026-10-26), status bar), each with the findings and backlog items it touches. Owner's notes so far: the menu bar holds menus only; the version moves to the window's title ("karasu-slicer 0.1.0"); the steps' buttons are pills, the tools' buttons equal squares with an icon and a small capital label; original icons generated into `assets/ui/icons/`, artwork in `assets/ui/images/`; a neutral Select tool first in Prepare, with no handles and no panel; repair markers show with the Repair tool only; the status bar shows the objects and triangles, one line of short messages (work in progress, what is wrong, the latest message) cut with an ellipsis, then the Messages button and the adapter at the right; the slice's facts move to the Slice panel; tool panels follow the owner's design rule (TD-098); every message is also a toast (TD-099); the Repair panel keeps one structure with or without a selection, the plate's slicing repairs move to the Slice panel's "Slicing options" and the repair of opened models to the File menu until the settings panel; "model" is the interface's one word (glossary), and a panel needing a model says "No model selected" in a notice at its top; roomier controls (buttons padded 10 by 5, controls 26 points tall); a switch widget; the Repair panel rebuilt from its mockup: the repair chosen as a draft and applied with Apply repairs (its outcome a toast), stages behind an Advanced switch, defects and markers in collapsible sections, the model's state in a fixed footer with "Use {preset}" applying the next stronger repair at once; panel rows that wrap instead of widening the panel, value fields with their unit in a segment of their own, and a body that cuts anything wider, tested in the pseudo-locale; global or local axes for moving and turning; the Transform panel rebuilt from its mockup (sliding Global/Local choice, Move, Rotate and Scale sections with clickable headers, locks, resets, uniform scaling, typed values following the handles, Center and Fit to plate); a panel's main actions in its fixed footer with the model's state, always in view; the Hollow and Drain holes panels rebuilt from their mockups (Advanced switches, the certified facts with the resin saved, the hole list with each hole's reasons as its tooltip, the drainage by region, the footer's state leading from an undrained cavity to the Drain holes tool, applied changes reported as toasts); the Slice and export panel rebuilt from its mockup (output facts and format, wall verification as a setting, slicing options behind Advanced, the result with the printed walls, Slice and Export or the progress with Cancel in the footer, the progress card removed), to be reviewed again with the printer and resin profiles; the section bar with typed layers, keys and an active handle, and adaptive or fixed layer ranges added from its menu in a window with a provisional exposure table (TD-102); the Layers tool in the Export step replacing the layer column: the white layer image with zoom, a larger window and keys, Follow in the viewport, each layer's printed facts with its area and islands, browsing in the footer (TD-103) | in progress |

Each step keeps the quality gates and native Windows validation, and is committed after the
owner's approval.

**Closed with reservations (owner, 2026-10-26).** M6.5 stops here, to be taken up again later:
the sidebar's review for multiple plates and the models' state icons; the Slice and Layers
panels if the owner reviews them again; the backlog (a model with defects tinted in the
viewport, the CPU shared by diagnosis and preparation, the profile data still to decide, the
orientation of printers the owner does not have).

## Phases

| Phase | Content | Verifiable result |
| --- | --- | --- |
| **M6.0 Foundations** | Dependency review (egui, eframe, wgpu, winit, egui_dock, native file dialogs) under `AGENTS.md`; minimal window with the palette theme; native start on Windows and Linux; first GPU smoke tests, including the OpenGL fallback | A themed empty window on both platforms; the dependency review record |
| **M6.1 Project and pipeline** | `karasu-project`: project/plate/instance model, per-plate profiles, jobs and revisions; the CLI moves to the shared pipeline; supports in the object frame | Headless model and job tests; comparison cases byte-identical |
| **M6.2 Viewport** | wgpu renderer: plate and build volume, large meshes (FX-12, 2.7 M triangles), camera, picking, translate/rotate/scale gizmos | FX-12 navigable smoothly; picking and gizmos tested |
| **M6.3 Core workflow** | Import, placement, printer and resin selection (basic profile editor), slicing with progress and cancellation, the layer slider showing the real masks, export | **First complete use:** import → slice → export from the UI |
| **M6.4 Preparation** | Hollowing panel (parameters, sectioned cavity view, certificates); drain holes by clicking the surface, editable; manual supports (tips, branches, braces, raft) with live findings; non-destructive edits | The ROADMAP's mandatory preparation scenarios work from the UI |
| **M6.5 Layout and style** | The TD-098 layout (it replaces the user-arranged egui_dock layout): menu bar, workflow bar with steps and tools, fixed contextual panels, a closable left sidebar, an information bottom bar; opaque panels | Persistent custom layout; contrast checked |
| **M6.6 Plates and project** | Plate overview pop-up with thumbnails and state; moving and copying objects between plates; export all plates; the project file | Save, reopen and recompute give identical results |
| **M6.7 Hardening** | Accessibility (keyboard, focus, non-colour cues); packaging prototype; signing feasibility; full native validation | M6 Definition of Done |
| **M6.8 Visual review** (owner, 2026-11-05) | A further visual review of the whole interface before the release milestone (M7): layout, spacing, hierarchy, colours and icons, and the deferred UI/UX points; no mockup (owner, 2026-11-10): the points the owner names, one by one | The owner approves the reviewed interface on both platforms |

**Parallel track:** a kit for physical print tests on the owner's Anycubic Photon. It contains
test models, files, a protocol and a recording sheet, and is prepared once the exact model is
known.

## Assets and default presets (owner request, 2026-10-20)

**Folder:** a repository `assets/` folder holds:
- `assets/ui/`: the interface's images, by kind (owner, 2026-10-23): `assets/ui/images/` for
  artwork (the splash) and `assets/ui/icons/` for icons;
- `assets/catalog/`: the built-in catalog of printers, resins and print settings by vendor
  (TD-106; `assets/presets/` until then).

The folders are created with their first real content: the first presets with the profile library
(M6.3), and the first images with the layout (M6.5). No empty placeholder folders.
`assets/ui/splash.png` (TD-069) was the first content, moved to `assets/ui/images/` in M6.5.
The first presets arrived with M6.3 (TD-077): eight printers and two manufacturers' standard
resins. TD-106 turned them into the catalog, generated by `tools/assets/make_catalog.py` with
its sources in [CATALOG-SOURCES](CATALOG-SOURCES.md). It is compiled into the executable for now; packaging
(M6.7) may install it beside it instead.

**Rules:**
- **Presets use the existing profile document format** (`ProfileDocument` JSON, as in the CLI's
  `--printer` and `--resin`). Every preset is validated by the core in automated tests, so a
  broken preset fails the build, not the user.
- **Provenance:**
  - printer presets come from public manufacturer specifications (build volume, resolution,
    pixel pitch, supported formats), with the source recorded per preset;
  - resin presets are starting points from public manufacturer recommendations, never presented
    as verified calibrations. The UI marks them as such.
- **Packaging:** whether presets and images are embedded in the executable or installed next to
  it is decided with packaging (M6.7). Either way, user-created profiles stay in the user's
  configuration folder, separate from the defaults, which are read-only.
- **Images:** only original or properly licensed assets, with their licence recorded (the
  reference applications' assets are never reused, per `AGENTS.md`).

## Owner decisions (2026-10-19)

1. **Data model:** project → plates from M6.1. The plate overview UI follows in M6.6.
2. **Docking:** use `egui_dock`, subject to the dependency review of M6.0.
3. **File dialogs:** native operating-system dialogs for now (for example `rfd`, subject to
   review). The final choice stays open.
4. **Project file:** models are embedded in the project file. Its format is open (below).
5. **GPU baseline:** Vulkan, DX12 and Metal where available, plus the **OpenGL fallback** for older
   machines, loaded only when the machine has no adapter for a modern backend (TD-100). Features the fallback cannot provide, such as the panel blur, degrade to the opaque
   style.
6. **Code signing:** desirable; feasibility and costs are studied in M6.7.

## Open decisions

### Project file format (decided on 2026-11-04, TD-130)

**Decided (owner, 2026-11-04):** a ZIP package of karasu's own, not 3MF, with the `.karasu`
extension: the project document as JSON and each model file byte for byte; cavities recomputed
on reopening; a recovery copy saved automatically. Exporting models as STL or 3MF follows later.
The models are embedded, by owner decision. The options as they were weighed:

| Option | For | Against |
| --- | --- | --- |
| **3MF container** (ZIP + XML; core 3MF model parts plus karasu data) | Open, documented format; other tools can open at least the geometry; karasu already reads 3MF and writes ZIP archives (SL1) | 3MF rules (relationships, content types, required-extension handling) must be followed exactly; plates and preparation need a karasu part that other tools ignore |
| **karasu format** (for example a ZIP with JSON plus mesh files) | Full freedom; the simplest to write and version | Readable only by karasu; another file format to document and keep compatible |
| **JSON with referenced files** | Trivial | Contradicts the "embedded models" decision |

**Recommendation at the time (not taken: the owner chose a karasu package):**
- A 3MF-conformant package: the source meshes as core 3MF objects, and the karasu project data
  (plates, profiles, preparation plans, supports, layer plans) as versioned JSON parts under a
  karasu path, with their own relationships.
- Whether it uses the `.3mf` extension or a karasu extension is to be decided. `.3mf` maximizes
  interoperability; a karasu extension avoids confusion with plain model files.
- Before fixing it, check under the clean-room rules how the reference slicers store their
  projects.

### Code signing

The study, in M6.7, covers:
- **Windows:** Authenticode signing with a code-signing certificate or a signing service, and
  SmartScreen reputation.
- **Linux:** signatures for packages or AppImages.
- **Costs and keys:** recurring costs, and custody of the keys.

Signing may also reduce the heuristic antivirus false positives seen with fresh, unsigned
executables (TD-063 record). Until it is decided, builds are unsigned, and antivirus blocks are
handled as environment blocks, as today.

**Studied on 2026-11-10:** [CODE-SIGNING-RESEARCH](research/CODE-SIGNING-RESEARCH.md). Owner,
the same day: no signing for now, and an individual as the publisher when it comes. The licence,
AGPL-3.0-or-later with a CLA (TD-145, 2026-11-11), leaves Certum Open Source as the open-source
route.

## Main risks

- **Dependencies:** wgpu brings a large dependency tree, and the M6.0 review is a real gate.
- **Performance:** multi-million-triangle meshes must stay smooth, including on the OpenGL
  fallback.
- **Consistency:** background revisions must never present stale results as current.
- **Antivirus:** false positives on Windows may also affect the application executable.
