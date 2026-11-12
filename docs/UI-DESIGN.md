# Desktop visual design

## Approved color palette

The project owner selected this palette for karasu-slicer on 2026-10-18.
These are the authoritative base colors for the M6 egui/eframe desktop interface.
Hex values describe opaque sRGB colors; rendering code must handle color-space conversion explicitly.

| Name | Semantic token | Hex | Intended role |
| --- | --- | --- | --- |
| Raven Black | `background` | `#0B0B12` | General application background |
| Charcoal Ink | `surface` | `#1A1C24` | Panels and surfaces |
| Beak Slate | `border` | `#4A465C` | Borders and separators |
| Resin Purple | `primary` | `#6F35F5` | Primary actions |
| Electric Violet | `interaction` | `#8E52FF` | Hover and selection states |
| Soft Lilac | `accent` | `#C9B2FF` | Accents and indicators |
| Mist White | `text_primary` | `#F5F4F8` | Primary text |

Use semantic tokens in the future theme implementation instead of scattering literal color values
through widgets. Keep the approved values unchanged; any additional disabled, focus, diagnostic or
viewport-material colors must be specified explicitly during M6 rather than inferred as approved
palette entries. Validate text contrast, focus visibility and selected states in their actual
combinations. Status and geometry diagnostics must also use labels, symbols or patterns so color
is not their sole carrier of meaning. The palette selection alone is not accessibility validation.

The palette defines application styling, not exported layer-mask values, resin exposure, source
material identity or the final appearance of 3D models. GPU viewport lighting/material choices remain
part of the M6 rendering prototype. No theme implementation or additional dependency is introduced
by this design record. See [TD-042](TECHNOLOGY-DECISIONS.md#td-042--rust-desktop-interface-with-egui-eframe-and-wgpu)
for the selected presentation stack.

## Brand palette and interface colours (owner decision, 2026-10-20; TD-074)

- **The approved palette (above) is the brand identity**, used for the interface chrome (panels,
  bars, buttons, text).
- **The interface also uses other colours** where convention or legibility matters. Every
  interface colour, the palette's uses included, must be configurable in the settings.
- **The graphics as a whole will be revisited** in the layout and style phase (M6.5).

**Viewport defaults** (`karasu-app::appearance`, applied until the user changes them; kept in the
settings file, TD-097):

| Use | Default |
| --- | --- |
| Models | `#b8b0a8`, a warm grey (owner choice, 2026-10-23; was `#fcbf87`) |
| Selected model | `#d9a088`, a flesh tone, as sculpting applications show the model worked on (owner choice) |
| Selection outline (M6.4f) | `#c8c8c8`, on by default; both configurable |
| Model outside the build volume (M6.4f) | its colour kept; the parts outside take a filter: `#d41e1e` at 88 % solid, or dark `#15161c` stripes at 75 % on the selected model; the status bar names how many |
| Background | Raven Black `#0b0b12` |
| Plate surface | `#1c1c22` (dark grey), 80 % transparent (alpha `0x33`, owner choice) |
| Plate grid, every 10 mm | `#4a465c` (grey) |
| Plate grid, every 50 mm | `#6b6876` (a lighter grey, to stand out) |
| Plate outline and build volume edges | `#3a3a42` (a darker grey, owner choice) |
| Gizmo X, Y, Z | `#e5484d`, `#46c25a`, `#3e7bfa` (conventional red, green, blue) |
| Free-move square (filled) | `#ffff00` at alpha 50 of 255, about 20 % (owner choice), `#ffe066` under the pointer |
| Uniform scale handle | `#f5f4f8` |
| Other handles under the pointer | `#ffd14a` |
| Section caps | `#f5f5f5` (owner choice) |
| Support section caps | `#2e5bd9` (blue, owner choice 2026-10-30) |
| Cavity in the section view | `#8d8a7e` (owner choice); `#9c6b58` for the selected model's |
| Drain hole being placed, not checked yet | `#6e6a7a` |

Move handles end in arrowheads pointing along their axis; scale handles end in squares.

Every colour, the palette tokens included, carries alpha (TD-076); all defaults are opaque.
Translucent models will render correctly once the sorted transparent pass arrives with the
hollowing view (M6.4).

Handles are always labelled X, Y or Z, so they do not rely on colour alone. Colours are
lit in linear light and encoded back to sRGB by the shaders, so a face turned towards the viewer
shows exactly the configured colour; the light follows the view.

## Tool panels (design rule, owner 2026-10-23)

1. **All the content can be reached.** A panel grows downwards at most to the viewport's bottom,
   less a margin; content taller than that scrolls inside the panel.
2. **Fixed size.** A panel keeps its width whatever its content: a long title is cut with an
   ellipsis, and long labels, texts and translations wrap or are cut. Nothing widens or
   deforms a panel.
3. **The title is the tool's name only.** No file or object name: a name can be very long and
   would break the design.
4. **No messages in tool panels.** Information, warnings, errors and confirmations do not
   appear inside a tool panel; they go to the notification system (toasts, TD-099). Panels
   hold the tool's settings, its actions and the facts the settings need. One exception: a
   *notice* at the top of a panel says what the tool needs and lacks ("No model selected"),
   as long as it lacks it. It looks like an information toast without a close button or a
   time, and the panel's structure below it stays the same.
4b. **Rows keep the width.** Text of free length is never on one line with a control (egui
    never wraps a row): a setting's checkbox takes the whole width and wraps, its value field on
    the line below, indented; a labelled value has its label wrap left of a fixed-width field.
    The shared rows (`widgets::check_value_row`, `widgets::value_row`) do this. As a safety net
    a panel's body has a fixed rectangle, text wraps by default, and anything wider is cut. A
    test lays the Repair panel out with every stage on, in English and in the pseudo-locale.
5. **The same structure always.** A panel shows the same sections and settings whether or not
   an object is selected. Without a selection, the object's controls are disabled, never
   hidden or replaced.
6. **Tool values and object values.** A tool's values (what its next operation will use: the
   next drain hole's diameter, the wall for a new hollowing, the next support's weight and
   values) start from the tool's defaults, which the user sets in the Settings dialog only.
   What is changed in a tool's panel stays while the user works, when the tool closes and
   opens again, and is never saved as a default (owner, 2026-11-02; it revises "every time the
   tool opens it shows them", 2026-10-27). A value that differs from its default shows a reset
   button (the reset icon, its tooltip naming the default) before its field; Edit › Restore all
   default values brings every one back, as does start-up (and, later, a new project). A
   default changed in Settings carries to the values still at the old default; those the user
   changed stay. They are kept while the selection changes within the open tool. An object's values (its applied wall, its
   holes, its supports) belong to the object and show only while it is selected; without a
   selection the disabled controls show the tool's values, never those of an object that is not
   selected. The repair a new model gets is a preference, in the Settings dialog too.
7. **Plate settings live where they act.** Settings of the plate, not of an object (the plate's
   slicing repairs and tolerances), are not in an object's panel: they are with the tool that
   uses them (Slice). Preferences of the user (the repair a model gets when opened) belong to
   the settings.
8. **Actions stay in view.** A panel's main actions (Apply, Remove, Discard and the like), the
   draft's state above them and the model's state sit in a fixed footer under a rule, below
   the scrolling body: they are always visible and never scroll with the content. The body
   holds the settings and the facts they need. (Owner, 2026-10-25.)

## Repair panel (M6.5)

From the top: the repair preset, its stages when the panel's **Advanced** switch is on, then
**Defects (n)** and **Markers** (the switch and its legend, always shown), both collapsible. A
fixed footer under a rule holds the draft's state ("Applied: …" or "Changed, not applied yet"),
**Apply repairs** and **Remove repairs**, then the model's state with its icon and colour (green
ready, orange hollowing blocked, red slicing would fail) and, when the repair is not enough,
**Use {preset}**, which applies the next stronger preset at once. Choosing a preset or a
stage only edits a draft; applying it is an undoable plate edit, and its outcome comes as a
toast once the model is prepared, with what the repairs changed as its detail.

## Transform panel (M6.5)

From the top: **Global | Local**, a rounded bar whose highlight slides to the choice, steering
the handles only: arrows and rings follow the plate's axes or the model's own, the free-move
square stays on the plate's plane, and in the Scale mode the bar is disabled showing Local, as
scaling always follows the model's axes. Then **Move**, **Rotate** and **Scale**, each with a
header that shows its handles when clicked (lit when shown) and icon buttons at its right:
a lock on each (open, or closed and lit; it stops the section's handles, values and buttons,
which fade), Reset on Rotate and Scale, and Uniform on Scale (a typed percentage scales the
three axes alike; the handles have their own uniform handle). Below each header, X, Y and Z in
value fields that follow the handles: the centre's X and Y and the lowest point's height on the
plate in millimetres, the angles about the plate's axes in degrees, the scale along the model's
axes in percent. Move has **Drop to plate** and **Center**; Scale has **Fit to plate** (the
largest uniform scale that fits the build volume, then centred and rested). Locks and Uniform are
settings of the tool, applying to any model selected.

## Hollow panel (M6.5)

The header has the tool's name and an **Advanced** switch. The body: **Hollowing**, with the wall
thickness and, with Advanced, the grid cell (Auto, or a value); then **Result**, collapsible:
the guaranteed wall, the thickest spots, the passes (with Advanced), the cavity and material
volumes, and the **resin saved** (the cavity's share of the solid model's volume). The fixed
footer: the draft's state ("Applied", "Changed, not applied yet", "Not hollowed"), **Apply
hollowing** and **Remove hollowing**, then the model's state, the most important first: a wall
predicted to exceed the memory (red), a wall under 1 mm (orange), a solid model, a wall changed
by scaling (orange), the preparation's state, a cavity that does not drain (orange, with **Add
drain holes**, which opens the Drain holes tool), else green "Hollowed: the wall is guaranteed
… and every cavity drains". The wall a model never hollowed starts from is the tool's value:
editing it, or applying a hollowing, sets it. Applying reports the guaranteed wall as a toast
once prepared; removing says the model is solid again.

**Blockers** (TD-115; owner mockup "Hollowing blockers", 2026-11-02): a collapsible section
between Hollowing and Result.
- **New blockers:** Box or Sphere; a box's lower and upper base (width and depth) and height, or
  a sphere's diameter; each with its reset button to the default set in Settings › Hollowing.
- **Solid base up to** (mm) and **Add a solid base**: a box over the whole footprint, from the
  model's lowest point up to that height.
- **Blocker n**, the selected one: its centre (X, Y, Z in the model's frame), a box's turn about
  X, Y and Z and its sizes, or a sphere's diameter.
- The tool strip: Place blockers (P: a click on the model puts one centred there), Move (T), Turn
  (R) and Scale (S) for the selected one's gizmo, the list window (L) and Delete. A box scales
  along its width, depth and height (both bases alike) or uniformly; a sphere only uniformly,
  its diameter; a sphere does not turn.
- **Size handles** (owner, 2026-11-03): dots on the selected blocker, whatever the gizmo. A
  box's dot at the middle of a base's side widens or narrows that base alone, evenly about the
  axis (a box becomes a truncated pyramid); a dot at the middle of a base moves that base alone,
  the other staying. A sphere's dots, at the ends of its axes, set its diameter. The fields
  follow while dragging.
- Blockers are drafts until **Apply hollowing**: the footer says how many are new or changed,
  and Remove hollowing removes them with the hollowing. In the viewport they show through the
  model: teal once applied, violet while a draft, magenta when selected, each colour set in
  Settings › Colours › Hollowing blockers.
- The list window "Blockers (n)": All or New or changed, each row its shape and sizes, Applied
  or Not applied yet, and a bin; a double click brings the view to it.

## Drain holes panel (M6.5)

The header has the tool's name and an **Advanced** switch. The body: **New holes**, with the
**Place holes** button (lit while placing; its label says a click stops it), the diameter and
depth (Auto, or a value) of the next hole, and **Start from**, a sliding bar Outer surface |
Cavity whose Cavity is unavailable on a model not hollowed; these are the tool's values. Then
the selected hole ("Hole n", "New hole n", or "Selected hole" disabled without one): its state
and its diameter, depth and, with Advanced, its tilt and tilt direction. The holes are listed
in a tool window, "Holes (n)" (owner, 2026-10-29, as the supports' list): opened by the tool
strip's List (L), All or only the Refused, applied holes then new ones, each a row with a dot
in the colour the viewport draws it (blue applied, green new, grey unchecked, red refused), its
size (diameter and depth), its state (OK, Refused, Checking, Not applied) and a bin; why a hole
is refused and what it does (it does not open outside, passes through, reaches no cavity, ends
in the material) are its row's tooltip. A click selects the hole, a double click also turns the
view to it. Then **Drainage**, collapsible: each cavity region with the holes that drain it (a
green mark) or none (an orange mark), and the pockets under 1 mm³ counted together. The fixed
footer: **Delete all drain holes** (owner, 2026-10-29: after a confirmation, the applied ones as
one undoable step and the new ones dropped), **Apply holes (n)** and **Discard**, enabled while
there are new holes, then the
model's state: why no hole can be made, while placing what the hole under the pointer would do
(red when refused) or how to place one, a refused hole, a solid model, the preparation's state,
a cavity that does not drain, else green "Every cavity drains". Applying reports the holes cut
as a toast once prepared, as a warning when a cavity region still does not drain.

The selected hole (TD-083, TD-120) has a handle at its rim (the diameter), at its end (a fixed
depth) and on its axis outside the model (a free tilt), and a turn gizmo: rings X, Y and Z at
the foot of its axis, about the world's axes or, with Surface, about the surface's tangents and
its normal. A ring leans the hole about that axis alone (Shift snaps to 15°), up to the 60°
limit. A new diameter, by the rim handle or the panel, keeps the hole's axis where it was
(TD-121): its tilt fields change instead.

Holes in the viewport (owner, 2026-10-25): only with the Drain holes tool and on the selected
model do holes show as edited: applied ones blue with their ghosts where the model hides them,
new ones green, the hole under the pointer green or red. Elsewhere, and on other models, a cut
hole is only the opening it makes, its inner wall in the model's colour, as it will print; a
refused hole stays red, and new holes are not drawn until the tool is back on their model.

Cavity regions that trap resin (TD-101): in **Drainage**, each region no hole drains has
**Show**; the pockets under 1 mm³ are counted, with "List the pockets" listing them (at most 20)
each with Show. With the tool on the selected model, an orange marker locates every undrained
region through the model (smaller for pockets); the pointer on one says what it marks, and a
click shows it. Show frames the region and cuts the models through it, so its cavity shows from
above; **Back to the whole model**, at the viewport's top left, restores the view and the
section. The **Cavity markers** switch, with its legend, ends the section.

**Suction cups** (M6.4j, TD-114; owner mockup "Suction cups in the Drain holes tool",
2026-10-31): a collapsible section under Drainage.
- **Values:**
  - **Minimum vent** (mm², its default from Settings › Hollowing): an opening vents
    a cup only where a circle of this area fits through it.
  - With Advanced, the display filter, kept with the settings: **Show cups from** (mm², the
    widest layer) and **and taller than** (mm), 1 mm² and 0.5 mm by default.
- **Facts**, once searched: how many were found (out of date once the plate changes), how many
  are shown with the smaller ones hidden, and how many close over their resin.
- **Navigator**, "Suction cup n of m", through the shown ones, widest first. The previous and
  next buttons frame the cup and cut the section at the top of the layer where its vent goes (that layer shows; owner, 2026-11-04), with Back to
  the whole model as for a cavity region. The chosen cup's facts follow:
  - its layers;
  - its height, from its cap to its last layer's top;
  - its widest area;
  - its volume;
  - its opening: none, or narrower than the minimum vent;
  - what then happens: it closes over its resin, or opens to the outside.
- **Parts** (owner, 2026-10-31): a cavity with several lowest points is one cup, but each part
  needs its own vent.
  - **Vents needed** counts them. A stepper, "Lowest point n of m", goes through them, framing
    each.
  - Markers stand at the chosen cup's vent points.
- **Add a vent here** adds a new hole, selected, with New holes' diameter, for the chosen part;
  **Add a vent at each lowest point (n)** adds one for every part. A vent starts where a circle
  of the minimum vent first fits in the part, above a narrow tip. It takes the shortest way out
  among straight down (preferred), down at 45° and level: through a cavity's wall, or through
  the wall of a crevice of the surface. The hole is turned along the way (at most 60°). A way
  that ends on the plate under a resting model is not taken. Parts without a way out are said
  in a toast. Choosing a cup or a part cuts the section in the layer where its vent goes, where
  it is plainly closed.
- **Footer:** **Find suction cups** (Find again, or Stop while running, with a progress bar)
  sits under Apply holes. It slices the model as it will be exported, with its supports, raft
  and holes, in the background; a toast says what was found and how long it took.
- **Viewport:** markers at each shown cup's cap, through the model, red without an opening and
  amber with openings too narrow (both colours in Settings › Colours). They are faint when out of
  date. The chosen cup is larger, in the selection's colour; a click on a marker chooses and
  frames its cup.
- **List window:** a second tab, **Suction cups**, with Shown (n) and All (n). The rows show the
  layers and widest area, and the state (traps resin, no opening, opening too small).
- **Slice panel:** a **Suction cups** line says what the last search found, or Not searched.
  A cup never stops a slice.

## Section panel and layer heights (M6.5, TD-102)

A column at the viewport's right, 104 points wide, always shown. Over and under the two-handle
slider, each handle's **Layer** and **Z** are value fields, typed or stepped; the active handle
(last dragged, clicked near or typed into) is outlined. Keyboard shortcuts move it, the slider
focused or not (owner R10, 2026-11-10: the cut is stepped while another tool is used, without
clicking the slider and losing the layer): by default the Up and Down arrows a layer, Page Up
and Page Down ten, Home and End to the plate and the top, set in Settings › Keyboard shortcuts
› Section. They wait while a field is typed in, a dialog is open, or another control holds the
keyboard (reached with Tab, or the layer image, which takes the same keys).
Handles snap to the plate's planned layers, numbered from the plate. The plate's layer ranges
are bands on the slider, violet for adaptive and teal for fixed. A right click opens a menu at
the layer clicked: "Add adaptive layers from here", "Add fixed layers from here", on a band
"Edit" and "Remove", and "Show everything". The range's window has the start (layer or height,
filled from the click), where it ends, the adaptive rule or the fixed height and its own
exposure, and the exposure table (provisional, kept with the plate until the resin profiles hold
it). What forbids applying is said in red, what deserves a warning in orange; Apply and Cancel
end the window. Applying and removing come as toasts; both are undoable plate edits.

## Slice and export panel (M6.5)

The plate's panel, not a model's: it needs no selection. The header has the tool's name and an
**Advanced** switch. The body: **Output**, with the printer, the resin, the print setting with its
exposure and XY compensation (a starting point says so) and a link to its editor,
and the layer height as facts, the **File format** among those the printer lists
and this build writes, by readable name (the family on hover; karasu's reference format, which is
no printer's, is offered only with Advanced), the plate's masks (TD-134): **Anti-aliasing** (Off,
2, 4, 8 or 16; levels the format cannot store disabled, with why), **Lowest grey** (only with
anti-aliasing) and **Anti-alias the supports** (off by default: binary supports), each the print
setting's unless changed here, a reset button beside a changed value naming the print setting's,
one undoable plate edit; and the **Verify the printed walls** switch (a setting of the tool: every
slice then verifies the walls, TD-095); with Advanced, **Slicing options**: the plate's repairs
before slicing and its tolerances while slicing (design rule 7); then **Result**: layers,
height, print time, resin and format of the last slice, faded once the plate changed, and, for a
verified slice, the printed walls' findings, each opening the Layers tool on its layer. The
fixed footer: **Slice** and **Export…** (only for a slice of the plate as it is), or while a
slice runs its phase, a progress bar and **Cancel**; then the slice's state, most important
first: running, a printer and resin to choose, no model, no supported format, the last failure
(its details in Messages), not sliced yet, out of date, else green "Sliced: … ready to export".
Output's **Layer height** gives the plate's heights in a line and **Layers** the planned count,
"about" when adaptive ranges were planned from the models as opened (TD-102).
The status bar says a slice runs; the progress card over the viewport is gone. The owner expects
to review the panel again with the printer and resin profiles.

## Layers panel (M6.5, TD-103)

A tool of the Export step, beside Slice and export. The body: the layer image (the exported mask
seen from above, the plate's front at the bottom, material white with the verification's marks
over it), zoomed with the wheel at the pointer or the − and + buttons, moved with a drag, and
opened in a larger window with the enlarge button or a double click; with the image focused, the
arrows, Page Up and Page Down, Home and End browse the layers. Then **Follow in the viewport**
(on by default: the section cuts the models at the layer shown), **This layer** (height,
thickness, exposure and kind as printed, exposed area, islands), and, when the slice verified
the walls, **Printed walls**: the findings (the thinnest wall, breaches, each pocket), each
showing its layer with the finding ringed, and the marks' legend. The fixed footer: first, previous, the typed layer "of n",
next and last, a slider with a visible track (the part up to the layer in the accent colour and
a handle like the section bar's), then the layer shown with its height, or that the slice is out
of date. The larger window has the same slider, between previous and next, under its image.
Without a slice the panel keeps its structure under a notice.

## Windows (design rule, owner 2026-10-26)

Every window is one of two kinds, and its properties are decided before it is built.

- **Dialogs** carry a task to complete or cancel:
  - modal: the rest of the interface is dimmed and cannot be used while they are open;
  - they open in the middle of the screen;
  - a title bar with the dialog's name and a close button, which cancels;
  - a fixed footer with Cancel and the main action at the right, always in view; the body
    scrolls or fills the space;
  - Escape cancels; Enter performs the main action when nothing has the keyboard;
  - never larger than the screen less a margin;
  - closing one with unsaved changes asks whether to discard them;
  - a dialog opened from another opens over it, and closing it returns to it.
- **Tool windows** are views kept open while working: not modal, movable and resizable, they
  remember their position and size, and the first time they open in the middle.

Positions and sizes are remembered across restarts (the settings store, TD-097): a resizable
dialog reopens at the size it was left at, a tool window where and as large as it was left. The app's `dialog` module draws both kinds: `dialog::show` the dialog's
frame (title bar, body, footer, Escape and Enter) around content that draws the body and the
footer's actions, `dialog::confirm` the small confirmations, and `dialog::tool_window` a tool
window's defaults. A dialog opened from another is stacked over it; only the topmost one acts.

| Window | Kind | Opens | Resizable | Size (default, minimum) |
| --- | --- | --- | --- | --- |
| Add printer, Add resin | dialog | middle | yes | 900 × 600, 760 × 480 |
| Library | dialog | middle | yes | 900 × 560, 760 × 400 |
| Settings | dialog | middle | yes | 920 × 640, 760 × 480 |
| Print setting editor | dialog | middle | yes | 720 × 560, 640 × 440 |
| Edit printer | dialog | middle | yes | 800 × 560, 760 × 480 |
| Edit resin, Set up, Catalog update | dialog | middle | no | fixed width, height of the content |
| Delete and discard confirmations | dialog over a dialog | middle | no | small |
| Layer range (TD-102) | dialog | middle | no | fixed width, height of the content |
| Messages, Log, the Layers tool's larger view | tool window | last place (first: middle) | yes | remembered |
| Supports list, Holes list (owner, 2026-10-29) | tool window, opened from the tool strip (L) | last place (first: middle) | yes | 340 × 440, 280 × 200; remembered |
| Plates overview (TD-135) | tool window, from All plates… | middle, every time (owner, 2026-11-05) | yes | 940 × 640, 308 × 420; size remembered |
| Delete plate (TD-135) | confirmation, only for a plate with models | middle | no | small |
| Rename plate (TD-135) | dialog | middle | no | 360 wide, height of the content |

## Sidebar (M6.5, TD-098; owner, 2026-10-26)

Two collapsible sections, open by default (remembered for the session, and across restarts once
the settings store holds them):
- **Plate**, with the button closing the sidebar in its header: the printer, resin and print
  setting menus below. It stays in view; it scrolls on its own when the window leaves the
  models' list less than five rows.
- **Models (n)**: one row per model, the file name from the left, shortened in the middle when it
  does not fit and keeping its extension (`VillageHouse(3x3)_32….stl`, the full name on hover).
  The list scrolls on its own. The selected row is lit, a row under the pointer lightly; the
  delete button shows on those two rows only and deletes at once (undo brings the model back).
  The Delete key deletes the selected drain hole, or else asks before deleting the selected
  model: the key may have been meant for a hole that was not selected.
- **Move or copy to another plate** (TD-135): the row's menu, or the row dragged onto a plate;
  see Plates.
- **Checked models** (owner, 2026-11-10): each row has a check box at its left, apart from the
  selection the tools work on (a click on the box checks, one on the name selects). A bar over
  the list has a check box for every model (mixed while only some are checked; a click checks
  them all, or clears them when all are), "n checked" ("None checked"), and the checked models'
  eye (hides them while any shows, shows them when all are hidden) and bin. The bin asks first,
  "Delete 3 models?", and deletes them in one undoable step. Checks follow their models when
  one before them is deleted and are cleared when another plate is shown.
- **Hide and isolate** (owner, 2026-10-26): each row has an eye, beside the delete button and
  shown with it; a hidden model's row is faint and keeps its struck eye in view, and cannot be
  selected until shown. A right click on a row offers Hide or Show, Isolate (only that model
  shown; again, "Show the others again" restores exactly what was shown before), Show all and
  Delete. A right click on a model in the viewport selects it and opens the same menu at the
  pointer (owner, 2026-11-10); one beside every model opens nothing, and a right drag still
  pans. The header reads "Models (10) · 3 shown" with a "Show all" link while any is hidden.
  Shortcuts: H hides the selected model, Shift+H isolates it, Alt+H shows all (View menu too).
  Hiding is a state of the view, not of the plate: not saved, not undone, and a hidden model is
  still sliced and exported; the Slice panel's footer says how many hidden models print. A
  hidden model is not drawn (its previews and holes neither) and cannot be picked; its geometry
  stays uploaded, so showing it again costs nothing.

While a dialog is open, the window's keyboard shortcuts wait: Escape and Enter are the dialog's.

## Plates (TD-135, owner 2026-11-05)

From the owner-approved mockup "karasu multiple plates mockup". Phase 1:
- **The plates bar**, at the bottom of the viewport between the sidebar and the section bar:
  one card per plate in project order, its thumbnail from above, its name, its number of models
  and where its slice stands (Not sliced, Slicing, Failed, Sliced, Out of date), the plate shown
  in a dark violet (`#2C2547`) outlined in the accent (owner, 2026-11-05: quieter than the
  selection's cyan, its white text 13:1); then New and All plates…. A click shows a plate, a card dragged onto another moves it there, a right
  click renames, duplicates or deletes it. The bar folds (its chevron, or Plate › Show plates
  bar) to a row of tabs 40 points tall with Previous and Next; folded or not is kept across
  restarts.
- **The Plate menu:** New plate (named "Plate n" with the first number from the plates' count
  up that no plate has, owner 2026-11-05), Duplicate plate, Rename plate… (F2, a modal dialog),
  Delete plate (confirmed when the plate has models, at once when it is empty, owner
  2026-11-05; undoable either way; the last plate stays), Previous plate (Ctrl+Page Up), Next
  plate (Ctrl+Page Down), Show plates bar, All plates….
- **Showing another plate** starts the plate's interfaces afresh (the selection, the
  preparation, the supports, the tools' plans, the section), keeps the tools' values for the
  next operation, and keeps each plate's slicing (its result, its layers and a slice running),
  so a plate shown again can be exported without slicing it again.
- **Undo** takes the newest step of the plate shown or of the plates themselves (a plate added,
  duplicated, deleted or moved); undoing a step of the plates shows the plate it brings back.
- **Thumbnails** show the plate's models from above on its printer's plate, drawn by the
  viewport's renderer with the meshes it holds; again only when the plate changes (the plate
  shown at most twice a second while a model is dragged), none while the bar is folded.

Phase 2 (owner mockup, 2026-11-05):
- **The overview** (All plates…, in the plates bar and the Plate menu): a tool window of cards,
  one per plate in project order, as many side by side as fit, then a dashed New plate card.
  A card shows the plate's thumbnail from above (256 × 144) with its slice's state at the top
  right and the box its models cover at the bottom left; "2 · Torso"; the printer, resin and
  print setting; its models, layers and height, print time and resin (faded when the plate has
  changed since its slice); then Open (primary; disabled for the plate shown), Duplicate,
  Export (only with a slice of the plate as it is) and Delete (disabled on the last plate),
  icons with tooltips. A card's image dragged onto another's moves the plate there; a double
  click shows it. The footer totals the plates: models, plates sliced as they are, their print
  time and resin, then Slice every plate and Export every plate… (phase 4); while a batch
  runs, its progress and Stop.

Phase 3 (2026-11-06):
- **Moving and copying models:** a model row's right-click menu has Move to plate and Copy to
  plate, each listing the other plates and New plate. A row dragged onto a plate's card or tab
  in the bar, or its image in the overview, moves the model there; with Ctrl it copies it. The
  plate under the pointer is outlined in Electric Violet while it can take the model, and a line
  beside the pointer says "Drop on a plate to move … there; hold Ctrl to copy it". A
  notification says where the model went ("…, beside the models there" when it could not keep
  its place) or that there is no room for it.
- **Clone** (owner, 2026-11-10): the same menu, in the sidebar or the viewport, has Clone. A
  modal dialog "Clone <model>" asks for the copies (1 to 100, one to start with), the plate
  (the one shown, marked "(shown)", another plate, or New plate) and "Arrange the copies" (on).
  On, the copies are arranged from the plate's middle outwards around the models already on
  the plate, which stay where they are, the plate's spacing apart; on the model's own plate the
  model is arranged with its copies (owner, 2026-11-10, after trying it), the first in the
  middle. Those that do not fit go to new plates after it, named "Arms (2)" as Arrange names
  them, the model staying on its plate. Off, every copy goes to the middle of the plate,
  its footprint (supports and raft included) centred there. One undoable step; a notification
  says how many copies the plate got and where the others went. A model too large for an empty
  plate is refused, nothing added.

Phases 4 and 5 (owner mockups "Export every plate" and "Arrange with the overflow to a new
plate", 2026-11-07):
- **Export every plate…** (Plate menu, overview footer): a modal, resizable dialog. Folder with
  Choose…; File names with the pattern `{project} {number} {plate}` and "each plate's format";
  a striped table of the plates (check box, "1 · Head", file, format, state); the switch "Slice
  the plates not sliced, or out of date, first" with its hint "Off: those plates are left out
  and listed in the notification"; the note "An existing file is never replaced without asking.
  Each file is written whole or not at all." Footer: "Export 3 plates" (primary), Cancel, and
  while it works "Slicing plate 2 of 3 · Torso · 42 %" at the left, the fields disabled. Files
  already there are named in a destructive confirmation (Replace). One notification at the end.
- **Slice every plate** (Plate menu, overview footer): no dialog; the status bar and the
  overview's footer show the progress, the footer's Stop ends it; one notification at the end.
- **Arrange models…** (Plate menu): a modal dialog "Arrange models · Arms": Spacing between
  models (mm, the plate's spacing to start with), Models "All on the plate (n)", Models that do
  not fit as a two-way choice "Go to a new plate" / "Stay where they are" with a weak help line
  for the choice; footer Arrange (primary) and Cancel. The notification "Arranged: 4 models on
  Arms" says in its detail where the others went and that Ctrl+Z undoes the whole arrangement.

## The project file (TD-130, owner 2026-11-04)

The File menu holds New project (Ctrl+N), Open… (Ctrl+O: projects, models and profiles), Open
recent (the last ten project files; one no longer found is removed with a notification), Save
(Ctrl+S) and Save as… (Ctrl+Shift+S), then Settings and Quit. A `.karasu` file dropped on the
window or named on the command line opens as the project; a model file is added to the plate.
- **Unsaved changes:** the window's title is the project's file name (or its name, untitled),
  marked with a dot while it has changes not saved. An untitled project with no model has
  nothing to lose. Closing the window, New and Open then ask in a modal dialog: Save (the main
  action, Enter), Discard, or Cancel (Escape); Save with no file yet asks where first. Closing
  while a save runs waits for it.
- **What a new or opened project resets:** the tools' values return to their defaults and the
  view starts over; the user's settings stay. A new project takes the printer, resin and print
  setting last used; an opened one keeps its own.
- **Saving and opening** run off the interface thread; a notification says when either is done
  or why it failed, the file left as it was.
- **Opening shows all at once** (owner, 2026-11-04): a modal dialog, "Opening <file>", says what
  is being done (reading the file, importing the models, preparing them, building the supports)
  while the scene draws no model; everything is computed and uploaded meanwhile, so the models
  appear with their hollowing, supports and raft together, and "Opened" is said then. Its one
  button, Show now (or Escape), shows it as it comes instead; there is no Cancel, since the
  project it replaced is already closed.
- **The recovery copy:** while the project has unsaved changes it is copied every few minutes
  (Settings › Models › Projects, 3 by default, 0 for none). After a crash or a power cut, the next
  start offers the copies left, newest first: Restore one (it opens unsaved, to save again
  where it was saved), Discard all, or Later.

## Settings (TD-097, owner mockup 2026-10-26)

File › Settings… (Ctrl+,) opens a resizable dialog: the sections at the left, the chosen one at
the right, and a footer with Close alone, since every change applies at once and is kept.
- **Appearance:** the lighting, each a slider with its field (the scene's brightness, 50–150 %, and the light a face seen
  edge-on keeps, 0–80 %, 35 % by default: more lights the shadows, less shows the relief; two
  shader parameters, nothing uploaded again); the colours under headings (models, plate and scene, handles, holes and
  supports, with the selected support's own colours, valid and with an error: owner
  2026-10-29; repair markers), each with its picker and its hexadecimal value (`#RRGGBB`, or
  `#RRGGBBAA` with an opacity); "More colours" for the rest; the selection outline's switch;
  the filter over parts outside the build volume (colour and pattern, for models and for the
  selected one); "Restore the default colours". The plate, grid and front tab are built again
  when their colours change.
- **Hollowing** (owner, 2026-10-27): the defaults new hollowings and drain holes start from:
  the wall and the grid cell (automatic or fixed), the hole's diameter, its depth (automatic or
  fixed) and the surface it starts from. **Supports** adds the preset new supports start from: a weight, or one of the user's own.
  The tools open with these defaults; changing one sets the open tool's values to it.
- **Keyboard shortcuts:** every action, each once (a test checks it), in groups as the other
  sections' (owner, 2026-11-10): the menus' (File, Edit, View, Plate, Windows), then Transform,
  Section, Tool strip, Steps and Tools; the action's name at the left, its hint under the
  pointer; with a search. Clicking a shortcut
  waits for the new keys (Escape cancels); a shortcut another action has asks first ("Use it
  here", the other action left without it); each row restores its default, and "Restore all"
  every one. Only shortcuts that differ from the defaults are saved.
- **Opening models:** the repairs every opened model gets, each with what it does.
- **Window:** how the window opens (as it was closed, maximized, full screen, or as a window,
  centred); "Forget sizes and places" of the dialogs and tool windows.
- **Language:** English (United States); the pseudo-locale only in developer mode (`--dev`).
- **Settings file:** where it is, its state (saved, unreadable, or a newer version's), "Open
  the folder" and "Start over with the defaults…" (confirmed), which keeps an unreadable or
  newer file beside the new one. A strip over the sections says when the file is not saved to.

## Overhangs on the models (TD-107)

View › Show overhangs (O) tints the surfaces that need support, live as the model turns: those
facing the plate whose slope above level is under the critical angle, yellow at the angle's
limit to red when level; a base resting on the plate is not tinted. The angle (45° by default)
is in the Settings dialog's Supports section, the two colours in Appearance. Islands, overhang
regions and minima come later in the Supports panel (TD-108), switched on there only when
needed.

## Tool strip (owner, 2026-10-29; TD-109)

A narrow column of icon buttons anchored at the viewport's left, a quarter of the way down, for
the tool in use's modes and switches, one click away; the tool's panel keeps its values. It is
opaque but light: the panels' colour, a little translucent, with no blur. A button performs its
action (every one is an action with a configurable shortcut, its name and key in its tooltip),
lit while on; a button with options shows a small triangle in its corner, which opens them, as a
right click does. Each tool says whether it shows a strip and what is in it:
- **Supports:** Place supports (P, with its options: one click, tip then foot, or branch onto a
  support; choosing one starts placing that way), Mini supports (TD-113: new supports are mini
  supports; no default key), Outside or cavity (C, with its options; a hollowed model
  only), Cavity view (V; a hollowed model only), Tips only (Y), the list window (L), Avoid the model (A), Knee follows the foot (K), then Merge trunks and Join a
  support (J), then Show overhangs (O), Show and Delete the selected support.
- **Hollow:** Place blockers (P), then Move (T), Turn (R, a box only) and Scale (S) the selected
  blocker, then the list window (L) and Delete the selected blocker (TD-115).
- **Drain holes:** Place holes (P), Start from the outer surface or the cavity (C, with its
  options), the axes the selected hole's rings turn about (Global or Surface, with its options;
  no default key; TD-120), then the list window (L), Show and Delete the selected hole, then
  Show the suction cups (X; TD-114).
- **Braces:** Place braces (P, with its options in two groups: one by one or in a chain, as
  clicked or level; choosing one starts placing that way), Automatic braces (G), then the
  Cavity view (V; a hollowed model only), the list window (L), Show and Delete the selected
  brace.

**Show and the list's filter** (owner, 2026-10-31): with many supports, braces or drain holes, a
wrong one is hard to see and to reach among the others. Both apply while the tool is in use, to
its items on the selected model; the model itself is never changed, since it must be seen well
while the items are edited. Only the draws change, so nothing is uploaded again.
- **Show** (the strip's button, no default key; lit while it leaves items out, its icon an eye
  struck through) hides the others: its options are All, Only with errors and Only the
  selected one, and in the Manual tool also Only the mini supports (owner, 2026-11-04); a click
  goes from All to Only with errors and back. With Only with errors, or the mini supports, the
  selected item stays too, so one fixed while selected stays in view until another is chosen.
  It is kept with the settings, per tool.
- **The list window's "With errors"** (for holes, "Refused"; in the Manual tool also "Mini",
  owner R8 2026-11-10), while the list is open, turns the others into ghosts: faint, drawn
  through the model in the colour "Items left out by the list's filter" (Settings, colours), so
  where the listed ones stand still reads.
- A hidden item or a ghost cannot be picked in the viewport. In Manual every support, brace and
  raft follows the filter; in Braces only the braces do, the supports they join staying whole;
  a drain hole left out still opens its model, as it does outside the tool.

## Supports panel (TD-108, owner mockup 2026-10-26, revised 2026-10-27)

The Supports step has its four tools: Supports, Islands, Braces and Raft (owner, 2026-10-27). Built in stages; what exists now:
- **New supports:** "Place supports" (as "Place holes"). While placing, the support a click
  would add follows the pointer under the selected model, green, or red where it shows an
  error alone (outside the build volume); it is built against a small triangle of the surface
  under the pointer (about 0.05 ms, whatever the model's size). A click adds it to the model at
  once, as one undoable step, its tip along the surface and its shaft straight down to the
  plate, and selects it (owner, 2026-10-27: no Apply or Discard). A surface facing up takes
  none, and the footer says why.
- **The contact's height line** (owner, 2026-11-06, TD-137): while a support is placed or its
  tip dragged, a thin line runs around the model at the height under the pointer, so that a
  row of supports can be kept level, as other slicers show it. Holding Shift holds that height:
  the tip goes where the surface at that height lies nearest the surface under the pointer (on
  a level face at that height, under the pointer itself), and the line stays there until Shift
  is let go. Its colour (dark by default, to stand out on both model colours) and whether it
  shows are set in Settings › Colours. The preset (Light, Medium, Heavy, then the user's own under
  "Mine") sets every value: the contact diameter, tip length, shaft diameter and penetration in
  view, with the contact's width at the surface beside them ("At the surface", the mark it
  leaves, TD-136), the rest under Advanced; they can be changed for the model at hand and are not kept
  (owner, 2026-10-27: each model needs its own supports). Beside the presets, icon buttons save
  the values as a new preset (its name typed below, Save or Cancel; a name another preset has
  is refused, said in the button's tooltip), update the user's preset chosen with the values
  as they are (enabled once they differ; a notification says so, owner 2026-10-29), rename it,
  and delete it after a confirmation. The user's presets are kept with the settings, for every project.
- **Fit contact to the surface** (owner, 2026-11-02; TD-116): a switch under the contact
  diameter, off by default, and while on, "Smallest contact" (0.2 mm when switched on, at most
  the contact). The core narrows the contact, with its penetration, down to that diameter where
  it would hang over an edge or go through a thin wall; it never widens. It is the tool's choice,
  kept whatever preset is chosen, and a support fitting its contact still shows its preset. The
  selected support shows "Contact as built" with the diameter its last build used. The Islands
  and Automatic panels have the same two fields under their preset; each tool's default is in
  Settings › Supports. Under their preset, those two panels also choose the contact's shape,
  Cone or Sphere (owner, 2026-11-02), over what the preset holds; Cone by default.
- **Cavity view** (owner, 2026-10-29): a toggle on the tool strip, for a hollowed model. The
  selected model's outside becomes a faint ghost (its colour and opacity in Settings, "Model in
  the cavity view") and its cavity is seen from inside: the near walls are left out, so the
  ceiling, the floor and the far walls show with the supports standing in it, without a
  section. Turning it on places new supports in the cavity; turning it off gives back the
  surface chosen before; choosing Outside, leaving the tool or opening it again ends it.
- **Tips only** (owner, 2026-10-29): a tool strip switch (Y; T until 2026-11-10, when Move took it), kept with the settings. While the
  Supports tool is in use, the placed supports show only their tips: their shafts, joints,
  bases and rafts, which hide the model's lower parts, are left out of the draws (the same
  uploaded meshes, fewer triangle ranges), and a click picks a support only by its tip. The
  support a click would add shows only its tip as well; a foot being placed (tip then foot)
  and a dragged handle's support show whole. Elsewhere the supports show whole.
- **Tip, then foot** (owner, 2026-10-29): the Place button's other option, kept like the
  switches. The first click places the tip where one click would; nothing is added yet, and the
  new support's foot follows the pointer as a dragged foot does: on the plate, or on the model
  below where the pointer meets a surface facing up nearer than the plate (inside the cavity, on
  its floor), the shaft vertical under its knee and the tip leaning to it at most 45° from
  vertical, shown as it would be built. The second click adds it, as one undoable step, even
  where it shows an error, and selects it; placing goes on. Escape takes the tip back; the
  viewport still turns between the clicks. The footer says where the foot goes.
- **Surface** (owner, 2026-10-29): Outside or Cavity, the latter for a hollowed model only and
  back to Outside with Restore all default values. In the cavity a support goes from the cavity's
  ceiling, picked where the section shows the inside (the cavity seen from within, as drain
  holes are), down to its floor: its foot always stands on the floor, whatever "Avoid the
  model" says. The footer asks for the section until the pointer meets the cavity. Such a
  support's row reads "(inside)", and its tip and foot handles move on the cavity's surface.
- **Avoid the model** (a switch under "Place supports", on by default, owner 2026-10-29):
  where a shaft straight down would go through more of the model below the tip, the support
  goes aside, its shaft in the nearest clear column and its tip leaning to a knee, at most 45°
  from vertical; failing that, its foot stands on the model below, pointing up towards its
  shaft as far as the axis limit allows (TD-132); failing that, it goes
  straight down and its build shows the collision. The preview shows the route, and the click
  adds it so. Off, supports go straight down.
- **The list window** (owner, 2026-10-29: a model may have hundreds of supports): a tool window
  "Supports (n)" of the selected model's supports, opened and closed by the tool strip's List
  (L), open or closed as it was left (kept with the settings). All, only those With errors, or
  only the Mini supports (owner R8, 2026-11-10), each filter with its count;
  each row has the support's number and dot (its colour in the scene), where it stands (on the
  plate, on the model, in the cavity), its state (OK, its first error in a few words, or a
  collision accepted) with its findings as the tooltip, and a bin. A click selects the support,
  a double click also turns the view to it, and a support selected in the scene scrolls the
  list to its row. Only the rows in view are laid out: measured at 0.03 ms a frame for 10, 1,000
  or 10,000 rows alike (Linux release).
- **The selected support** (panel section "Support n", or "Selected support" disabled without
  one): its state, its preset (owner, 2026-10-29: the preset whose values it has, a weight
  first, or "Custom" once a value changed; choosing another gives it that preset's sizes at
  once, as one undoable step, where it stands, its knee, its tip's direction and an accepted
  collision kept), its four values (the rest under Advanced) and the tip's direction, which
  change it alone, "Remove the knee", and, for a support that crosses the model, "Accept the
  collision" (the finding stays reported). A click on a support in the scene selects it, drawn
  lighter; Delete removes it and Escape stops placing, then releases it.
- **Mini supports** (owner, 2026-10-30; TD-113): a tool strip switch beside Place, kept with
  the settings. On, new supports are mini supports: a thin rod with a ball at each end, no tip
  cone and no foot, standing on the model or on another support's shaft. The three ways of
  placing still apply:
  - one click stands it on the first surface straight below its tip, facing up, within 45° of
    vertical; without one the footer says "No surface below to stand on" and a click adds
    nothing;
  - tip, then foot: the foot follows the pointer on the model only, the rod straight from the
    tip;
  - branch onto a support.

  The panel's values are then the mini support's two: "Diameter" and "Penetration", which starts
  at zero (the balls touch the surface). A selected mini support reads "Mini support n", shows
  those two values and only its tip and foot (or junction) handles. Its list row reads
  "· mini". A mini support may touch the model anywhere (owner, 2026-10-30): it is never red
  for it; where its rod goes through the model, its row's tooltip says so.
- **Errors print, with a warning** (owner, 2026-10-27): a support with an error is kept, drawn
  red and listed; the footer says how many supports have errors and that they print as they
  are, the status bar counts them, and a slice with them ends with a warning. A support that
  cannot be built at all (its tip off the surface) is listed but not drawn.
- **Advanced** (owner, 2026-10-27): every other value of the next support and of the selected
  one (the contact's shape and ball, the tip's body, a tapered shaft, the pad and its cone, a
  foot on the model, the sides) and the tip's direction (from the surface, towards the shaft,
  or as a handle turned it; a route keeps off the model with the tip as it is built, TD-129);
  the fields keep the couplings the core requires (a contact at most as wide as the tip's body,
  a ball at least as wide as the contact, a cone at most as wide as the pad). Checks: the
  clearance proven from the model and the tip angle limit, kept in the model's support document.
- **Branches** (owner, 2026-10-30): with the Place option "Branch onto a support", a click on
  the model sets a branch's tip (a dot, its tip alone shown before) and a click on a support's
  shaft adds the branch, with the next support's values, as one undoable step: it joins the
  shaft as high as it can with its slanted part at most 45° from vertical, its tip pointing at
  the junction within the tip angle limit and, where the model's surface for routes is ready,
  clear of the model; the branch it would add follows the pointer over the shafts, and a shaft
  with no such place is said in the footer (red) and adds nothing. Escape takes the tip back.
  A branch is no thicker than its trunk where it joins it (owner, 2026-10-30): placed, or given
  a preset, thicker than that (its shaft or its tip's body), it takes its trunk's values, so its
  preset is its trunk's, and a notification says so; a value changed in the panel past that is
  brought back to the trunk's thickness.
  The junction is a handle (a square) that slides along the trunk; released, the branch is
  brought back to no thicker than the trunk where it now joins it.
- **Trees** (owner, 2026-10-30): **Merge trunks** (a mode) works out on a thread of its own how
  the selected model's supports standing on their own (on the plate, the raft or the model,
  placed by hand or by the Islands tool) would merge, as the Islands tool merges its own, with
  its merge values; the supports it would change are drawn in the selection's colour and the
  footer says how many trunks would merge into how many, with Apply (one undoable step) and
  Discard. **Join a support** (J) takes the selected support standing on its own: a click on a
  shaft bends it below what hangs from it (its branches' junctions and braces' ends) and leans
  it at most 45° into that shaft, clear of the model, as one undoable step; a shaft it cannot
  reach that way, itself or one standing on it is said in the footer (red). Escape stops either.
  The junction is a handle (a square) that slides along the trunk. Deleting a support deletes
  the branches on it, and theirs, with the braces on any of them (Undo brings them back).
- **Grid** (owner, 2026-11-06, TD-138; the mockup "Supports on a grid"; the strip's Grid, B):
  a mode, which ends placing and is ended by it or Escape. Its button's options (the triangle)
  choose how an underside is picked, kept with the settings: **Click a flat face** (a click on
  the selected model's underside picks the triangles joined to the one clicked that turn from
  it by less than "Flat within", 5°, under Advanced; a face facing up is said and nothing is
  picked), **Rectangle** or **Lasso** (a drag draws the shape over the viewport instead of
  turning the view, Escape drops it; released, it picks the triangles facing down it touches,
  seen through the model, and the tips stay inside it); its grid is planned on a thread of its own and shown in the colour of a valid hole,
  with Apply (one undoable step; the supports are placed ones, which the Automatic tool keeps)
  and Discard in the footer. The panel shows the face (area, tilt), the grid's values (preset;
  "Spacing along the rows" and "Spacing between rows", 3 mm; "Along the longest edge", on, or
  the angle typed; "Tips along the edges", on, with "Edge inset", 0.4 mm, under Advanced;
  "Branches", off, with the branch distance and branches per trunk), and what is planned
  (supports, corners, along the edges, beside the lowest edge, on the grid, standing on their
  own, left unsupported, time). A value changed, or the model moved, plans the same face
  again. The values start from Settings › Supports › Supports on a grid.
- **Handles** (owner, 2026-10-27): the selected support shows its tip (a dot: it slides on the
  model's surface facing down), its foot (a square: it moves on the plate, or the raft, or on
  the model where the pointer meets a surface facing up below the tip, nearer than the plate:
  dragged off the model it stands on the plate, and back; owner 2026-10-29; on the model it is
  a dot, a second tip, with a direction ring of its own on a line from it, which turns the foot
  at most the tip angle limit from the surface, owner 2026-10-29) and its
  direction (a ring on a line from the tip: it turns the tip, at most the tip angle limit from
  the surface), and its knee (a diamond, owner 2026-10-29: the top of the vertical shaft; it
  moves up and down the shaft, which stays where it stands so the support never moves towards
  its neighbours, and the tip leans from its contact to the knee; it stays 2 mm above the plate
  at least and at most where the slanted part is 45° from vertical; "Remove the knee" in the
  support's row puts the shaft straight down from the tip's join again). A knee goes along
  when the tip slides, and follows the foot when the foot moves, keeping the shaft vertical,
  unless the tool strip's switch "Knee follows the foot" (on by default, kept like the other
  switches) is off: then the knee stays and the shaft leans (owner, 2026-10-29). A support
  without a knee gets one when its foot moves, where its shaft began (the tip's join), and its
  tip then points at it; the knee goes down as the foot goes away, the slanted part at most
  45° from vertical. Where a dragged support cannot be built at all, its handle waits at the
  last place it could. While dragged, the support shows as it would be, built alone as the placing
  preview, in place of its built parts; released, it changes as one undoable step and is built
  against the whole surface; Escape puts it back. The handles are painted over the viewport,
  a constant size on screen, as the drain holes' are.
- **Handle gizmo** (owner, 2026-10-29): once clicked or pressed, the knee and a foot on the
  plate show a small gizmo, as the Move tool's, about 34 points long whatever the zoom: the
  knee X, Y and Z arrows and a square that moves it freely on its level; a foot on the plate X
  and Y arrows.
  Dragging an arrow holds the handle to that world axis, the square to the horizontal plane;
  dragging the handle itself moves it as before (the knee up and down its shaft, the foot where
  the pointer meets the plate or the model). Held by the gizmo the foot stays on the plate, and
  the knee moved aside takes a foot on the plate along, keeping the shaft vertical, unless
  "Knee follows the foot" is off. The arrows take the axes' colours, the square the free
  move's, and the one under the pointer or held the hovered handle's. The tip, its direction
  and a foot on the model have no gizmo: they move freely on the surface. The gizmo stays until
  another support is picked, the selection is released or the tool closes.
- **Footer:** **Delete all supports** (owner, 2026-10-29: after a confirmation, every support
  and so every brace, as one undoable step; the raft stays), then the state: placing, a surface
  facing up, a handle dragged, supports with errors (their reasons in the tooltip), the build
  running, or the count of supports built without errors.
- Every model's supports are built against its latest preparation off the interface thread,
  the selected model's first; the preparation leaves them out, so adding or changing a support
  never prepares the model again. The scene draws them on the GPU, uploaded once per build;
  while a moved model is built again, its supports are carried with it. Supports are kept in
  the model's frame: they follow every move, turn and scale. A model lifted off the plate is
  prepared where it is before it has supports; slicing still asks for supports or for it to
  rest on the plate.

The four tools' mockup is on the canvas (row "Supports step: four tools", 2026-10-27). Coming
next: Islands, Raft and Braces.

## Orient panel (M6.4k, TD-119, owner mockup 2026-11-03)

The Orient tool, in the Prepare step after Transform, proposes tilts of the selected model: the
model only turns, about its centre, resting where it rested; its turn about the vertical axis is
kept.
- **Constraints:** Tilt about (Any, X only, Y only); At most from now (a switch, on at 30° by
  default, and the most tilt); Protected points, picked on the model with the strip's Protect
  (P), each with its bin; with Advanced, how far round a point supports are kept away (5 mm).
  A tilt that would turn a protected point's surface to face the plate is never proposed.
- **Priority:** Fewer supports, Balanced or Faster; with Advanced, the weights it sets (surface
  facing the plate, islands, suction cups, height, largest layer, protected surfaces), shown as
  multipliers (×) under a note: each measure is compared among the candidates, the largest
  counting as 1, then multiplied by its weight (owner, 2026-11-03: the values had no unit).
- **Orientations:** up to five candidates, each a card with its tilt, the surface facing the
  plate, islands, cups (hollowed models), height, largest layer and whether protected points
  are clear of supports; the best at a measure is tagged (Fewest supports, Fewest islands,
  Fewest cups, Lowest). Hovering a card previews the model turned to it, overhangs tinted; a
  click chooses it. Candidates for an earlier plate, values or points are out of date.
- **Footer:** what turning means for this model (supports removed, cavity to check); once
  candidates are found, Apply and Discard (owner, 2026-11-03: back to the model as it is; Escape
  too); Find orientations (Stop while searching), the progress, and the state. While Protect is
  on, the preview is off, so a click lands on the model as it is. Apply turns the
  model as one undoable step; a model with supports asks first, in a modal dialog ("Turn and
  remove the supports" or Cancel).
- **Strip:** Protect (P), Delete (the last protected point), the overhangs (O). Protected
  points show as green markers (Settings › Colours › Overhangs), turning with the preview.
- The tool's values start from Settings › Supports › Automatic orientation and stay while the
  user works, each field with its reset button.

## Automatic panel (M6.4i, TD-112, owner mockup 2026-10-30)

On the Supports step's strip, the Automatic tool shows the plain support icon and the tool for
placing supports by hand is named **Manual**, with the support-and-plus icon (owner,
2026-10-30: the two icons swapped, "Supports" renamed).

The Automatic tool, first in the Supports step, plans every support of the selected model at
once, previews them and applies them as one step. The Islands tool keeps Support the islands
for now (owner, 2026-10-30).
- **Generate** (the footer's button; Stop while it runs, nothing changing) slices the selected
  model alone, without its supports, as Find islands does, and plans with the panel's values on
  the Manual tool's surface for routes. Its progress shows under the button, and the state
  line names the stage: finding, placing the tips, routing and growing trees, checking the
  coverage, building the supports to check them, planning braces. It waits first for the surface for routes, while the tool keeps
  it ready.
- **What is planned.** Islands are supported as Support the islands supports them; an island a
  kept support (placed by hand or by the Islands tool) already holds is left to it. With
  "Islands and overhangs", every region's underside is probed on a grid half the tip spacing
  apart. A point facing the plate less steeply than the critical angle wants a tip within its
  reach: the tip spacing under a level surface, growing with the slope up to the spacing at the
  critical angle. Where the underside ends (nothing beyond it, a drain hole, a step or a sharp
  turn), its outline takes points of its own 0.4 mm in from the edge and from both edges at a
  corner (owner, 2026-11-06, TD-136); a leading edge, which the underside rises away from and
  so prints first, takes its tips the contact's radius from it, and the face beside it side
  tips as close, alternating with them. Corners are taken first, then edges, then the inside,
  each lowest first, and one becomes a tip when no tip is within its reach yet; kept supports'
  tips count. A side tip that finds no support is a place left unsupported. Tips close together share trunks and trunks merge, as
  the Islands tool grows them; braces are the Braces tool's automatic ones, with its values.
- **The closing check:** where a support finds no way down, the next uncovered points are
  tried, pass after pass (3 by default). What is still uncovered is joined into places left
  unsupported, each with why.
- **The final check:** the supports are built as a slice builds them, against the model. One
  built with an error (a tip piercing a thin part, a part too close to the model or outside the
  build volume) goes, and what hung from it is grown again on supports of its own. Its place
  is left, with why: the plan never offers a support that would print with an error.
- **Panel:**
  - **Lift** (owner, 2026-11-04, TD-128): how high the model's lowest point stands above the
    plate, 5 mm by default, with the tool's other values in Settings › Supports. Generate lifts
    the model there first, as one undoable step, when it stands elsewhere, and plans on the
    model where it then stands; supports standing on the plate grow with it.
  - **Cover:** "Islands and overhangs" or "Islands only"; where, for a hollowed model
    (owner, 2026-10-30): "Everywhere" (the default), "Outside" or "Cavity only". Generate then
    replaces only the automatic supports of that place, those whose tip touches the cavity's
    surface or the outside's, so the cavity can be planned again without touching the outside; the critical angle (from Settings › Supports' overhang angle each time the tool
    opens); with Advanced, "Skip islands under".
  - **Supports:** the preset (a weight or one of the user's; Medium by default, TD-136);
    "Tip spacing" (2.5 mm); with
    Advanced, "Spacing at the critical angle" (5 mm), "Join islands closer than" (0 mm, owner 2026-10-30) and
    "Closing check: at most" (3 passes); "Density" (100%, 50 to 200%: the tips per area, a
    value field with the app's slider under it and the spacings it gives under them; TD-127);
    "Closer under wide undersides" (on), with "Wide from" (20 mm²) and "As close as" (60%);
    for a hollowed model, "Own values in the cavity" (off; TD-137), which gives the cavity its
    own preset and density ("Preset in the cavity", "Density in the cavity"), starting from the
    outside's when switched on;
    "Feet on the model" (on, owner 2026-10-30); "Mini
    supports where a support does not fit" (on, TD-113). The plan's facts count the mini
    supports when there are any.
  - **Trees and braces:** "Branches" (on), with the branch distance and branches per trunk, and
    with Advanced the merging values; "Braces" (off, TD-136). The braces' values are the Braces tool's
    own, shared with it (owner, 2026-10-30): with Advanced, every one as the Braces tool and the
    settings show them (diameter, distance, lowest height, step, pattern); otherwise a line
    saying them.

  Every value's default is set in Settings › Supports › Automatic supports.
- **Planned / Applied:** the supports, branches and merged trunks, braces, islands held, the
  share of the overhangs' points covered, the closing check's passes and supports added, the
  time taken, and how many places are left unsupported.
- **List window** (owner, 2026-10-30, as the other tools' lists; the strip's List, L): the
  places left, each an island or an overhang at its height, with why; All or Islands; a click
  chooses one, drawn larger in the selection's colour, and a double click brings the view to
  it. Places cannot be deleted, so the rows have no bin.
- **Scene:** the planned supports in the colour of a valid hole, uploaded once. The places left
  unsupported are orange markers drawn through the model. The strip shows the overhangs, the
  places left unsupported (a switch kept with the settings), the cavity view and the list window.
- **Footer:** **Delete the automatic supports** (after a confirmation, as one undoable step; the
  others stay). While a plan shows: **Apply N supports**, which replaces the automatic supports
  with the planned ones, braces too, as one undoable step; and **Discard**. Then **Generate**
  (**Generate again** once the model has automatic supports) or **Stop**, and the state:
  planning and at what, how many supports are planned and how many places are left, out of
  date once the plate changes (a plan made for another plate cannot be applied), or what Apply
  added. Apply's notification is a warning when places are left.

## Islands panel (TD-108, TD-107, owner mockup 2026-10-29)

The Islands tool, in the Supports step after Supports, finds what the selected model's print
starts in mid-air and which of those places a support holds.
- **Find islands** (the footer's button; Stop while it runs, the islands found before staying):
  the selected model alone, without its supports, is sliced on a thread of its own in the
  plate's format as Slice slices it, a lifted model where it stands (an analysis only, never an
  export), and its
  layers' masks are read for the unsupported regions at the critical angle (TD-107). An island
  is where one piece starts in mid-air (owner, 2026-10-30): several may grow into one region,
  each its own island. A support holds an island when its tip stands where it starts: on its
  first layer's footprint, widened by 0.6 mm and by the join distance (islands that close share
  a support), within 1 mm above it. A support beside it does not. The tool
  never searches by itself: once the plate changes, the islands found show as out of date until
  found again. A notification says what was found and how long it took.
- **Scene:** markers a constant size on screen, drawn on the GPU through the model as the
  cavity markers are: red without a support, green held, faint when out of date; the chosen one
  in the selection's colour (owner, 2026-10-29: it stands out among many), twice as large, drawn
  over the others and whole through the model, its row's dot in the list too; a click on one chooses it. The local minima (TD-107 level 2) of the prepared surface
  (the hollowed shell's, its cavity's ceiling included) show as small yellow markers when asked,
  worked out on a thread of their own.
- **Panel:** Search (the critical angle, its default Settings › Supports' overhang angle; with Advanced, "Hide islands under": smaller islands are left out of the markers, the list, the stepping and the counts, and the list's summary and the search's notification say how many are hidden (owner, 2026-11-05); Support the islands still supports them unless "Skip islands under" leaves them out); **Island supports** (owner
  mockup revised 2026-10-29): the preset (a weight or one of the user's), "Branches: join tips within" (5 mm; zero
  makes no branches), "Branches per trunk" (4), "Trunks: merge within" (10 mm; zero makes no
  merging), "Trunks per merge" (3) and "Merge levels" (2), with Advanced "Join islands closer than"
  (0 mm: every island its own support, owner 2026-10-30) and "Skip islands under" (mm², for
  Support the islands only), "Foot on the model when the plate is out of reach" (on) and "Mini
  supports where a support does not fit" (on, TD-113), all
  their defaults set in Settings › Supports › Island supports; then the chosen
  island: previous and next (the view turns to it; one in the cavity turns the cavity view on,
  and one outside turns it off again unless the user turned it on), its layers, height, area, place (outside or in the cavity) and support, and **Support it
  here**, which plans its supports by the rules below and adds them once planned, as one
  undoable step.
- **Support the islands (U):** a mode, as the automatic braces: the supports for every island
  without one are planned on a thread of their own and shown in green, again when a value or
  the plate changes. One support an island (owner, 2026-10-30), where it starts: under its
  first layer's centroid. If that is refused (no way down, or the final check), the next point
  of its first layer at least 2 mm from every refused one is tried, three at most. Islands
  closer than the join distance (in plan and in height) share one support under the lowest; an
  island of the group that support does not reach gets its own. An island too close to the
  plate for the preset (its tip, base cone and base need 3.2 mm with Light) is left, and Support
  it here says how much more to lift the model. What grows from an island
  higher up is an overhang, the Automatic tool's.
  Tips closer than the branch distance in plan (in the cavity, or outside, never mixed), up to
  the branches a trunk takes, share one trunk (owner, 2026-10-30: branching, phase 2): the tip
  nearest their middle that finds its way down; the others join its shaft as branches, as a
  branch placed by hand does, or get a support of their own when they cannot. Then trunks
  standing on their own (on the plate or the model) closer than the merge distance merge, level
  after level (phase 3): the one nearest
  their middle stays where it stands, its foot widened by the square root of the tips it carries
  over five, at most twice; the others keep their shafts down to a bend below their own lowest
  branch and lean into it at most 45° from vertical and at most the branch distance aside
  (owner, 2026-10-30), clear of the model, or stay on the plate when they cannot. A
  tip goes where a line straight up from under the island meets the surface; each support is
  routed as placing routes it (straight, aside with a knee, or on the model below, the last only
  with the switch on or in the cavity, where a support always goes from the ceiling to the
  floor), and kept only when it builds with the preset (not too close to the plate). Then the
  final check, as the Automatic tool's: supports a slice would build with an error (a tip
  piercing a thin part, a collision, outside the build volume) go; one entering a drain hole
  stays, only warned about, but one whose tip or foot a hole cuts away goes (TD-131), and an
  island left without one is skipped with why, in the footer's tooltip and the notification. The footer
  says how many supports for how many islands and how many islands were skipped (why, in its
  tooltip), with **Apply** (one undoable step, a notification, the mode ends) and **Discard**.
  The plan waits for the model's surface for routes, kept ready while the tool is in use.
- **List window:** the islands, their first layer, area and place, and their support; All or
  Without a support, and Everywhere, Outside or In the cavity (owner, 2026-10-30; an island on
  the cavity's ceiling cures in the vat as one outside does, the Where fact's tooltip says); a row chooses one, a double click brings the view to it. Islands cannot be
  deleted, so the rows have no bin.
- **Going to an island** (owner, 2026-11-04): stepping to one in the panel, or a double click in
  the list, frames it from above with the section cutting the models through the middle of its
  first layer, so the layer where it starts in mid-air shows; **Back to the whole model**, at the
  viewport's top left, restores the view and the section, as for a cavity region.
- **Footer:** **Delete the islands' supports** (owner, 2026-10-29: the supports the Islands tool
  placed, marked so in the support document, and the braces on them, after a confirmation, as
  one undoable step; supports placed otherwise stay), Apply and Discard while Support the
  islands shows its plan, Find islands (again) or Stop, then the search's progress, not searched
  yet, out of date, how many islands have no support, or every island supported. In the
  Supports tool's list a support the Islands tool placed says "· island".

## Braces panel (TD-108, owner mockup 2026-10-29)

The Braces tool, in the Supports step between Supports and Raft, joins the selected model's
support shafts with braces, kept in its support document and built with its supports (a brace's
heights are world values, so it stays where it was put when the model moves).
- **Placing** (the strip's Place): a click on a shaft sets a brace's first end at the height
  clicked; a click on another shaft adds the brace at once, as one undoable step, and selects
  it. In a chain each click adds a brace from the shaft clicked before; level, the second end
  takes the first one's height. While the second end is sought, the brace a click would add
  follows the pointer in green. Escape takes the first end back (ends a chain), then stops.
- **Automatic braces** (the strip's G): the braces the panel's values would add between
  neighbouring supports are planned on a thread of their own, again whenever a value, the
  supports or the model change, and shown in green; those that would touch the model or cross
  a drain hole show red. The footer says how many would be added and how many were skipped
  (why, in its tooltip), with Apply (one undoable step, then a notification, and the mode ends)
  and Discard. The two modes exclude each other.
- **Panel:** New braces (the switch "Automatic diameter", on by default: each brace as thick
  as the thinner of its two shafts where it meets them, up to the "Largest diameter" typed
  (1.0 mm by default, owner 2026-11-06, TD-136), following them when they change; off, the
  "Diameter" typed), Automatic braces (maximum distance, lowest brace, step, pattern:
  Horizontal, Zigzag or Cross), then the selected brace (the supports it joins, its automatic
  diameter or its own, its largest typed and the one worked out ("As built") shown while automatic, and its height on each, which
  change it alone, and its findings). The new values' defaults are set in Settings › Supports ›
  New braces (automatic, else 0.8 mm; 25 mm, 6 mm, 6 mm, Zigzag, until changed there).
- **Handles:** the selected brace's ends, dots that slide along their shafts; the brace shows as
  its preview while dragged, and changes as one undoable step when released.
- **List window** (L): the selected model's braces, what each joins and its state; a row
  selects it, a double click brings the view to it, the bin deletes it.
- **Footer:** **Delete all braces** (after a confirmation, as one undoable step), Apply and
  Discard with the automatic braces shown, then placing and what a click does, the automatic
  braces' plan, or how many braces there are and how many have errors (a brace with an error
  prints as it is).

## Raft panel (TD-108, owner mockup 2026-10-29)

The Raft tool, in the Supports step after Supports, holds the selected model's raft: values of
the model, kept with it, shown while it is selected, each change one undoable step (a value
dragged in a field merges into one). It has no modes, so no tool strip. The model's lift moved
to the Automatic tool (owner, 2026-11-04, TD-128); the Transform panel's Z still places the
model by hand.
- **Raft** (revised by the owner, 2026-10-29: the raft is the supports' foot): with the switch
  "Raft under the supports", the supports standing on the plate lose their own pads and stand on
  the raft through a fillet, a low wide cone. Type (TD-144): Slab, its sides sloping out down
  to the plate, Skate, sloping in so a scraper slips under its edge (choosing it sets the
  slope to 45°, still editable), or Web, a disc under each support and low bars joining
  neighbouring supports in triangles, with "Longest bar" (20 mm) and, with Advanced, the bars'
  width and height (1 mm and 0.5 mm); Web hides Base, since the longest bar groups the feet. Base: Single, one raft under every foot, or Groups, one under each group of feet
  closer than "Join feet closer than" (10 mm when first chosen). Its thickness, and with
  Advanced its margin beyond the outermost fillets, its sides' slope, and the fillet's diameter
  and height (0 joins the shaft straight), are in the ranges the support document accepts. A
  new raft starts from Settings › Supports › New rafts (the same fields; 0.5 mm, 1 mm, 60°, a
  fillet 2 mm wide and 0.8 mm high, Single, Slab, until changed there). Under it: how many
  rafts were built (a web's separate pieces), how many supports stand on the plate, and the
  footprint they cover.
- **Footer:** the model's base in a few words: a raft under n supports and the model's height,
  a raft waiting for a support on the plate (orange), a skate with nothing left at the plate
  under its narrowest foot (red: its build fails), or no raft and the model's height.

## Printer, resin and print setting (TD-106)

The app reopens with the printer, resin and print setting last used, while they are still in
the library and fit together (TD-097).

The sidebar's plate card has three menus. **Printer** lists only the user's printers (name and
model), then "Add printer…" and "Manage the library…"; the card shows the panel, the build
volume and whether the orientation is verified, and "Catalog update available" when a newer
catalog revision changes the printer. **Resin** lists the user's resins set up for the printer;
the others follow under "Not set up for …" with "Set up…" (a starting point copied from one of
the printer's settings), then "Add resin…". **Print setting**, once a resin is chosen, is a menu
of that resin's settings on the printer, each with its status (official, starting point, yours),
then "New print setting…" and "Manage the library…"; the chosen setting's facts and source
follow with "Edit the print setting…". An empty library shows only "Add your printer…".

A library can hold many profiles (owner, 2026-11-11), so the three menus share one form
(`profiles_ui::menu`): the list scrolls once it is taller than 320 points; the menu's actions sit
below it, outside the scrolling part, and stay in view; with more than ten entries a search field
heads the menu and takes the keyboard focus as it opens, listing the entries whose name contains
the text, ignoring case ("No match" when none does). Clicking the field keeps the menu open;
choosing an entry or an action closes it.

The windows: **Add printer** (catalog by vendor with search, details and what is verified, a
unique name, and the resins with print settings for it as a checklist, only the vendor's
standard resin ticked at first, under an "All" box that ticks every resin or clears them, mixed
while only some are (owner, 2026-11-11, TD-146); or a custom printer with its panel, its formats chosen by
readable name from a menu with a search, grouped by family, listing the formats of the chosen
vendor's printers unless "Show every format" is on (CTB named with its version and
"unencrypted", such as "CTB version 4, unencrypted", owner 2026-11-11: karasu writes no
encrypted CTB), and the orientation shown as an "F"),
**Add resin** (catalog for a printer, official settings first, or start from a setting; or a
custom resin), **Set up** and **New print setting** (one dialog: the printer and the resin,
chosen in it when opened from the library or the print setting menu, and what to start from, one
of the printer's settings copied as a starting point or nothing, every value then entered in the
editor; the library's Print settings tab opens it from its footer), the **print setting editor** (Exposure; Rests and
motion, with other slicers' names in the tooltips, two stages near and away from the film, speeds
in mm/s or mm/min; Image: anti-aliasing, named as in the Slice panel (Off for binary masks, then
2, 4, 8 or 16 samples per pixel side; TD-134), lowest grey, XY compensation), **printer and resin
forms**, the **library** (printers, resins, print settings in a table with
column headers, striped rows lit under the pointer, each row's state against the catalog and its
actions as outlined icon buttons with tooltips: edit, duplicate (printers, with their print
settings), export and delete with confirmation, on every row; import and add in the footer) and a **catalog update** (each change,
yours and the catalog's, applied or dismissed). Outcomes are notifications.

A resin has a colour and a swatch (TD-118, owner 2026-11-03): the resin form's "Colour" (the
maker's name) and "Swatch" (a colour picker, optional). The swatch shows as a small square beside
the resin's name in the sidebar's resin menu and in the library, where the colour joins the row's
facts. The print setting editor's Exposure section ends with "Heating temperature" (°C, 25 by
default). The model is not drawn in the resin's colour.

## Value fields (M6.5)

A value with a unit is one field of fixed width (100 points) split in two: the value at the
left, typed after a click (Enter or leaving keeps it, Escape restores it, out-of-range values are
brought into range, a point or a comma as the decimal separator), stepped by the Up and Down
keys; the unit at the right in a shaded segment, always in the same place, where a horizontal
drag changes the value (it takes no keyboard focus: one Tab stop per field, TD-142). Values are kept in the unit's base (millimetres); a field only shows and
reads them. A later variant lets the unit segment choose among units (mm or µm for a layer
height), the choice being a user setting.

## Sliders (design rule, owner 2026-10-26)

Every slider is the application's own, `widgets::range_slider` (or `track_slider`, as wide as its
space, for the layers), never egui's: a visible 4-point track in the border colour, the part up
to the value in the accent colour, and a filled 10 × 18 point handle like the section bar's,
primary at rest, the interaction colour under the pointer or dragged, the border colour when
disabled. A click or a drag sets the value; focused, the arrows, Page Up and Page Down, Home and
End step it (TD-142). A slider for a setting has a fixed width (200
points in the Settings dialog) and its value field beside it (see Value fields), both setting
the value, so it is read exactly and typed as well as slid; the layers' sliders show the layer
in the panel's own fields. A test scans the application's sources for egui's slider.

## Groups and dependent settings (design rule, owner 2026-11-10)

- **A section of a panel or a window is a group** (`widgets::group`): a rounded frame in the
  border colour, its title in capitals and the accent colour set into the top edge, its rows
  inside, and a gap before the next group. It replaces bold section titles.
- **Settings that a switch turns on** (`widgets::depends`) stand under that switch, indented and
  joined to it by an accent rule on their left; with the switch off they are hidden.
- **Switches** are as tall as the other controls; a label beside one is centred on it, its first
  line when it wraps (`widgets::switch_label`).
- **Handles** that are not an axis (supports', drain holes', the uniform scale) are amber,
  `#E6A817`, apart from the hover yellow `#FFD14A`; the colour is set in Settings, Handles.
- The closed sidebar's button keeps an 8-point margin on every side; the plates bar's fold
  button stays at the bar's right end, folded or open.

## Action labels (design rule, owner 2026-11-10)

Buttons, actions and menu items carry no ellipsis ("Open", "Settings", "Export", not "Open…"),
even when they open a dialog. An ellipsis stays only where text is cut to fit, and in messages
of work in progress ("Slicing…").

## Tooltips (design rule, owner 2026-11-10)

A tooltip explains what a parameter or an action of the slicer does, and nothing more: no
printer models, no values a particular printer uses, no notes on a printer's settings. The only
exception is a short clarification against another slicer where it helps (the name the same
parameter has in Photon Workshop, CHITUBOX or Lychee). Printer-specific facts belong in the
catalog and its sources.

## Accessibility (design rule, TD-142; owner 2026-11-10)

- **Every control has a name** a screen reader can announce: its visible label, given to it with
  `widgets::name` when the label is drawn apart (a row's label names its field). A control
  without visible text (an icon button, a plate's card) is named by its tooltip or its content.
- **The keyboard reaches everything** with Tab and Shift+Tab, in a modal dialog only the dialog.
  A control that only a drag uses takes no focus. The application's sliders step with the arrows,
  Page Up and Page Down, Home and End.
- **Focus ring:** the control with the keyboard's focus has a 2-point accent outline
  (`#C9B2FF`) 1 point outside it, shown while the keyboard is in use and gone after a click.
  Areas that scroll are shown with `focus::scrolled`, so a focused control is brought into view.
- **A window that opens takes the focus** on its first control; the 3D view takes none (the
  camera has no keys, owner 2026-11-10).
- **Colour is never the only carrier of meaning:** a state, a level or a refusal also has an
  icon or words where it shows (a message's level, a plate's state, a support that would have
  an error). The viewport's tints are explained by their lists and findings.
- **Status colours are the user's** (Settings › Appearance › Status colours): warnings, errors,
  successes and the notifications' fills; the settings name any that reads under 4.5:1.
- **Checked by tests:** the accessibility check draws every tool and dialog with AccessKit on and
  fails on a control without a name, a hidden or nameless Tab stop, or a stop outside an open
  modal. A new window or tool is added to it.

## Glossary (owner, 2026-10-23)

The interface uses one word for each thing, in every text and translation:
- **model**: what the user opens and places on the plate, every copy included. Never "object"
  in the interface (the code's plate *objects* are instances of the library's *models*; the
  user sees each one as a model).

## Toast notifications (M6.5, TD-099)

At the bottom centre of the viewport, 20 points above its bottom, 440 points wide, at most three,
the newest lowest. Fills: information `#155e8a`, success `#17663a`, warning `#9a4a06`, error
`#9b1c1c`, each passing 4.5:1 with the primary text. Title bar: the severity's icon, its name,
a close button. Under the pointer the close button takes the toast's own fill a fifth of the way
to white, not the interface's violet (owner R7, 2026-11-10), so it belongs to its toast; the
icon on it keeps 3:1. Timed toasts (information 5 s, success 4 s, warning 8 s by default; errors
until closed) show a 3-point bar emptying right to left; the pointer pauses it. Every toast is
in the Messages window too.

## Icons (M6.5, TD-098)

Original line icons, generated by `tools/assets/make_icons.py` (deterministic, no dependency):
each icon is a few strokes, 1.6 units wide with round caps and joins, on a 24-unit grid. The
script writes `assets/ui/icons/<name>.svg` (the editable source) and `<name>.png` (96 × 96,
white, the strokes' coverage as alpha). The application uploads each PNG once, with mipmaps,
and tints it when drawing, so an icon follows its button's state and the theme. Tool icons are
drawn 32 points wide in 76-point square buttons. The first set: select (a pointer), transform (four
arrows), repair (a wrench), hollow (a cube), drain holes (a drop), slice (stacked layers),
layers (a frame with a section), enlarge (four corners), and the severities: information (a
circled i), success (a circled check), warning (a triangle with !), error (a circled cross). The
library's rows add edit (a pencil), duplicate (two sheets), export (an arrow out of a tray) and
delete (a bin), and save (an arrow into a tray: a support preset updated); then chevrons left
and right, first and last (a chevron to a bar), plus, minus,
close (a cross) and remove (a circled minus, its bar two strokes thick so it reads small: it
takes a row out of a form). Deleting an item from a list window is the bin, at the row's whole
height (owner, 2026-10-29).

**Rule (owner, 2026-10-26):** a button shows an icon with a tooltip, or words; never a symbol
character such as ×, ‹ or an emoji, which depends on the font and says nothing to a screen
reader. A test scans the application's sources for buttons labelled with symbols alone. Icon
buttons are plain over images and in title bars, outlined among text and fields (the library's
rows, the layer steps). The icons are shared through the egui context as well, for code that is
not handed them, such as the dialogs' frame.

## Startup splash (TD-069, implemented)

- **Window:** borderless and opaque, 720 × 405 points (16:9).
- **Artwork:** `assets/ui/images/splash.png`, the owner's artwork (2026-11-09; the raven with the
  product name; the owner keeps the 1672 × 941 master), prepared by
  `tools/assets/make_splash.ps1 -Master <file>` (Windows, System.Drawing): resampled with high-quality bicubic
  filtering to cover 1440 × 810 pixels, twice the splash size for high-DPI displays, square and
  opaque. It replaces the generated artwork of TD-069 (a sphere of layer slices) and its rounded
  transparent corners, which showed dark on native Windows (owner, 2026-11-09).
- **Text:** the artwork carries the name, so the application draws only the rest, lined up with
  the artwork's title and clear of the raven: a subtitle in Soft Lilac, the current startup step
  above a Beak Slate bar filled with Resin Purple, and the version in Mist White. Without the
  artwork the fallback panel shows the name as well.
- **Replacing the artwork:** replace the master and run the script, or replace the PNG with any
  opaque 1440 × 810 8-bit RGBA image. A test checks the size, the format and the opacity. The text positions follow the artwork's title (`splash.rs`).

## UI/UX concept (owner ideas, 2026-10-19)

**Status:** ideas recorded at the owner's request for M6. They are not yet an approved
specification: each one will be detailed, prototyped and confirmed during M6.

### Owner ideas

*The layout decided in M6.5 is [TD-098](TECHNOLOGY-DECISIONS.md): a menu bar, a workflow bar
with steps and their tools, fixed panels at the viewport's right, and a closable left sidebar
with the printer, the resin and the objects. Panels are not user-arranged (point 3 below is
superseded) and stay opaque (point 5 is dropped).*

1. **Project → plates.** A project holds several plates, each with its own printer and resin
   profile (and so its own layer plan, exposures and export format). One project can target
   different printers with different resins.
   - **Navigation:** plates are presented like an operating system's virtual desktops, not side by
     side in the viewport.
   - **Overview:** a pop-up shows the plates as live thumbnails; selecting one loads it into the
     UI.
2. **Extensible, modular UI:** panels are integrated incrementally as features need them.
3. **User-editable layout:** the user can rearrange, dock and resize panels.
4. **Large central viewport:**
   - it shows the current plate;
   - it has transform gizmos;
   - a lateral slider scrolls through the layers.
5. **Side panels with a light frosted-glass effect:** slight transparency and blur, but not so
   much that readability suffers. *Dropped (owner, 2026-10-23): panels and windows stay opaque,
   in the panel colour, with no blur.*
6. **Contextual panels** for the current operation: hollowing, drain holes, supports and so on.
7. **Top bar for the workflow:** prepare, hollow, supports, export, and so on.
8. **Bottom bar for information.**

### Notes for M6 (design input, not decisions)

**Plates:**
- **Data model:** build the application state as "project → plates (printer, resin, objects,
  settings)" from the start, even before the plate overview exists. Adding plates later to a
  single-plate application would be costly. A plate maps directly to one headless core job,
  extending the TD-065 plate document with printer and resin references.
- **Shared models:** a project-level model library (each source loaded once, identified by its
  hash), with per-plate instances. Preparation (hollowing, holes, supports) is per plate by
  default, with explicit "copy preparation from…" actions.
- **Changing printer:** moving or copying an object to a plate with another printer or resin
  marks its preparation and slicing as stale: build volume, minimum drain diameter, `auto`
  simplification, exposures and adaptive layers all depend on them. The core already refuses
  invalid results; the UI must show the stale state, never hide it.
- **Support coordinates:** supports should be stored in the object's frame, not in absolute plate
  coordinates, so they follow the object between plates of different sizes. Today's CLI support
  documents are bound to a world placement; the GUI needs this generalization.
- **Overview pop-up:** each plate shows its name, printer, resin and state (ready, stale, errors).
  Ideas: drag and drop objects between plates; export all plates. Keep the current plate always
  visible, for example "Plate 2/5 · printer · resin", with keyboard shortcuts to switch, so users
  do not forget other plates exist.
- **Background jobs:** per-plate background preparation and slicing, cancellable, with progress in
  the bottom bar. Thumbnails reuse the headless preview renderer.

**Panels and layout:**
- **Docking:** egui does not provide docking by itself. A docking crate (for example `egui_dock`)
  would need the dependency review required by `AGENTS.md`; otherwise a small in-house layout
  model. The user's layout should persist per user, separate from projects.
- **Frosted glass (dropped, owner, 2026-10-23):** panels and windows stay opaque, so no blur
  pass is built. The note stands for the record: egui draws translucent fills natively, but blur
  would need a custom wgpu pass sampling the viewport behind the panels.
- **Contextual panels:** they follow the selected workflow step from the top bar. Each panel binds
  to a headless core operation and shows its diagnostics and certificates: certified wall range,
  hole openings, support findings, simplification deviation.

**Viewport and layer slider:**
- **Viewport:** the custom wgpu viewport recommended by TD-042.
- **Layer slider:** it shows the sliced mask of the current prepared revision, never a stale one,
  and can overlay layer height, exposure and, later, analysis results.
