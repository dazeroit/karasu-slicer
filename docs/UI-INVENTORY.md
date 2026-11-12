# Interface inventory (M6.5 input)

**Status:** taken on 2026-10-23, at the start of the interface review (M6.5), from the code at
`d109049` plus the actions layer (TD-097). It records what `karasu-app` shows and does today, so
the new layout places everything that exists and loses nothing. It is a snapshot, not a design:
the layout decision and the migration steps are in [M6-PLAN](M6-PLAN.md#m65-plan-layout-and-interface-review).

Texts are the `en-US` catalog's. Files are in `crates/karasu-app/src/`.

## 1. Screen structure

egui places panels in the order they are added (`window.rs`, `MainWindow::show`):

| Area | Size | Shown | Module |
| --- | --- | --- | --- |
| Workflow bar (top) | full width, one row | always | `window.rs` |
| Status bar (bottom) | full width, one row | always | `window.rs`, `messages.rs`, `slicing_ui.rs`, `preparation_ui.rs` |
| Layers panel (far right) | 300 pt, fixed | once a slice exists, current or not | `slicing_ui.rs` |
| Section panel (right of the viewport) | 78 pt, fixed | always | `section_ui.rs` |
| Viewport (centre) | the rest | always | `window.rs`, `viewport.rs` |
| Floating | | | slicing progress, Profiles, Preparation, Colours (test), Messages |

The main window opens maximized (1280 × 800 windowed, at least 640 × 400), after a splash.

## 2. Splash

Borderless and transparent, 720 × 420 pt, at least 1.5 s and until three real start-up steps
finish: artwork, "karasu-slicer", "MSLA/LCD resin slicer", the current step over a progress bar,
and the version. No controls. Models named on the command line import behind it.

## 3. Workflow bar

Left to right, in one non-wrapping row of up to about 20 items:

1. "karasu-slicer" and the version.
2. **Open…** (action): native dialog for models (STL, OBJ, 3MF) and profiles (JSON).
3. **Undo**, **Redo** (actions; disabled when nothing to undo or redo).
4. The plate's name.
5. **Printer** combo (presets, and "{name} (mine)"); "No profile library" when the library
   cannot open.
6. **Resin** combo, with a note that presets are starting points (only with a printer).
7. **Edit profiles** (only with a printer): opens the Profiles window.
8. **Repair** menu (section 3.1).
9. **Format** combo (only with a printer); it picks the first supported format by itself when
   the current one is not offered, which is a plate edit made while drawing.
10. **Slice** (needs objects, a resin and a format; hidden without a printer).
11. **Export…** (only when the last slice matches the plate).
12. **Fit view** (action).
13. **Colours (test)** toggle (temporary).
14. **Move (M)**, **Rotate (R)**, **Scale (S)** toggles (actions), shown even without a
    selection.
15. The selected object's name, and **Drop to plate** (action) when it is lifted.

### 3.1 Repair menu

Plate-wide settings, each a plate edit (repeated changes to one setting merge into one undo
step), except the first:
- "Repair opened models": Diagnose only, Exact (default), Standard, Aggressive. A workspace
  setting, not saved.
- "Repairs before slicing": merge duplicate faces; make face orientation consistent; turn
  inside-out bodies outward (forces orientation); fill holes in the model.
- "Slicing": close holes in each layer up to {mm}; leave reversed slivers empty up to
  {mm² per layer} (on by default); slice despite mesh defects.

## 4. Status bar

Left: "{n} objects · {n} triangles"; the slice's layers, time and resin (grey and "out of date"
after an edit); the preparation's state or failures; floating objects; objects outside the build
volume; imports and defect checks in progress. Right: the latest message (truncated), the
**Messages** toggle ("Messages ({n} new)", coloured by the worst unseen level), and the GPU
adapter, backend and multisampling.

## 5. Section panel

"Section", the upper cut's layer and height, a two-handle vertical slider (the nearer handle
follows the pointer, snapped to the resin's layers, at least one layer apart; double-click shows
everything), and the lower cut's layer and height. View only: it changes no plate data, but holes
are placed and the zoom aims only where it shows the surface.

## 6. Layers panel

"Layers"; **Verify walls** (slices again with verification; only when the slice is current);
after a verification, the printed walls' findings with links that show their layer ("Thinnest
wall", "Breached in {n} voxels", up to 8 "Pocket {k}" and "{n} more"); a layer slider with
**‹** and **›**; "Z {z} mm"; the layer's mask, seen from above with the front at the bottom; the
findings' legend; a note on what the image is.

## 7. Viewport

| Overlay | Shown |
| --- | --- |
| Models, plate, grid, build volume, front tab and word, section caps, previews (cavity, supports), hole solids and ghosts | always; the plate fades out from below |
| Selection outline | an object is selected and the outline is on |
| Outside-volume filter | parts of models outside the build volume |
| "Drop models and printer or resin profiles here" | the plate is empty |
| Gizmo (move arrows and free-move square, rotate rings, scale handles) | an object is selected |
| Selected hole's handles (rim, end, tilt) | a hole is selected |
| Repair markers and their tooltip | an object is selected, markers on, a preparation exists |
| View cube and home button | always, top right |
| Slicing progress card ("Slicing", phase, progress, **Cancel**) | a slice runs over 150 ms, centred near the bottom |
| Preparation window | an object is selected, pinned under the view cube |

## 8. Floating windows

**Preparation: {name}** (fixed position, no close button, no scroll area), in four sections:
1. *Repair:* the state ("Ready to print", "Needs repair…", and so on); **Show markers on the
   model**; the marker legend; one row per finding with **Show** (turns the view to it); the
   repair preset combo (plate, none, Exact, Standard, Aggressive, Rebuild, Custom); a collapsed
   **Stages** section with about 15 repair stages and tolerances; the repairs' report; **Try
   {preset}**; **Remove repairs**.
2. *Hollowing:* wall thickness; grid cell (Auto or a value); the predicted-memory and fragile-wall
   warnings; the state (applied, changed, not hollowed); **Apply hollowing**; **Remove
   hollowing**; the wall range when scaled.
3. *Drain holes:* applied holes ("Hole {n}" and **×**, notes, and when selected diameter, depth
   with Auto, tilt and tilt direction); new holes ("New hole {n}", **×**, fields, **Apply holes
   ({n})**, **Discard**); **Add drain holes** with the next hole's diameter and depth, and "Start
   from" outer surface or cavity; a hint; the candidate's refusal and notes.
4. *Report:* solid, waiting for a printer and resin, preparing, refused, or the certificate
   (guaranteed wall, thickest spots, passes, cavity and material volumes) and drainage per cavity
   region.

**Profiles** (movable, closable): **Printer** / **Resin** tabs; the printer's name, identity,
build volume, panel pixels, plate origin, output swaps and mirrors, formats; the resin's name,
identity and calibration for the plate's printer (layer height, exposures, bottom and
transition layers); a status line; **Save to my profiles**. Edits reach the plate only when
saved.

**Messages** (movable, resizable, closable): **Clear**; newest first, each with its text,
**Details**, and fix buttons ("{fix}", "{fix} and slice again", or "{fix} (on)" disabled).

**Viewport colours (test)** (temporary): every colour the viewport reads each frame, the outside
filters' colours and patterns, the outline switch, **Restore the defaults**.

Native dialogs: open, and export.

## 9. Keyboard (TD-097 keymap)

| Shortcut | Action |
| --- | --- |
| Ctrl+Z | Undo |
| Ctrl+Shift+Z, Ctrl+Y | Redo |
| M, R, S | Move, Rotate, Scale handles |
| Delete | Delete the selected drain hole, or else the selected object |
| Escape | Stop placing or dragging holes, release the selected hole, or else clear the selection |
| Shift, while turning | Snap the turn to 15° (a drag modifier, not an action) |

Shortcuts work while no widget holds the keyboard focus. Not bound: open, save, fit, home,
slice, export.

## 10. Mouse in the viewport

| Gesture | Effect |
| --- | --- |
| Primary drag | a hole handle or a hole's body (selected object), else a gizmo handle, else orbit |
| Secondary or middle drag | pan |
| Wheel | zoom at the surface under the pointer (M6.4f) |
| Click | while placing holes: add one; on a hole: select it; else select the object under the pointer, or clear the selection |
| View cube and home | turn to a face, edge or corner, or to the starting view |

Views fit by themselves after an import and when a printer first appears.

## 11. Files and command line

Files are opened by dropping them anywhere, from **Open…**, or on the command line. A JSON
profile applies to the plate only (not saved to the library); other files import as models.
Options: `--gpu-backend`, `--smoke-test`, `--window`, `--locale`, `--locale-dir`, `--help`.

## 12. What persists

**Kept:** user profiles (saved from the Profiles window, with revision history), exported slices,
session logs.

**Lost at every restart** (TD-097 requires them kept, through the settings panel's store):
- the window's size and place, and every panel's and window's state;
- "Repair opened models" and the plate's repair settings;
- appearance colours, patterns and the outline switch;
- the gizmo mode, the camera, the section, the layer slider;
- "Show markers on the model", the next hole's diameter, depth and start surface;
- the language, the window mode and the GPU backend (command line only).

The project itself (objects, poses, preparation, profiles chosen, undo history) has no file yet:
that is M6.6.

## 13. Actions found in several places

Undo and redo (bar, keys); handle modes (bar, keys); opening files (dialog, drop, command line);
choosing printer and resin (combos, opening a JSON, saving a profile); dropping to the plate (bar,
a message's fix); plate repair settings (Repair menu, message fixes); per-object repair (preset
combo, **Try**, a message's fix, "Repair opened models"); slicing (bar, fixes "and slice again",
**Verify walls**); framing (fit, home, automatic fit, **Show**); selection (click, Escape);
removing a hole (**×**, Delete); hole size and tilt (fields, handles); stopping placement (toggle,
Escape); choosing a layer (slider, arrows, finding links).

## 14. Findings for the review

- **Preparation window:** fixed in place, no scroll area: with Stages open it can run past the
  bottom of the viewport.
- **Workflow bar:** one row of about 20 items; it will not fit longer translations (M6.3b).
- **Repair stages:** a value dragged in the Stages section makes one undo step per frame (no
  merge key), unlike the Repair menu.
- **Format combo:** picks a format by itself while drawn, as a plate edit (an undo step nobody
  asked for).
- **Marker legend:** drawn with the default colours, not the configured ones.
- **No delete button** for an object: only the Delete key.
- **Hole depth while placing:** once edited, there is no way back to automatic.
- **Owner's backlog** (memory of earlier reviews): the auto-repair combo closes the Repair menu
  when clicked; models needing repair are not tinted in the viewport; the layer preview is too
  small and the pocket list stops at 8; the verification map's material should stay white and
  its cyan be in the legend; the live log window and diagnostics; the CPU shared by diagnosis and
  preparation.
