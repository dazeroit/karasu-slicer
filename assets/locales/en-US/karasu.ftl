# karasu-slicer interface texts: the reference catalog (TD-080).
#
# Every interface text is a message here. Translations live in assets/locales/<locale>/ with the
# same message identifiers and variables; a message they lack falls back to this catalog.
# Counts ($count and the like) select plural forms; other variables are text already formatted.

## Numbers

# The decimal separator of numbers shown in the interface.
number-decimal-separator = .

## Start-up

splash-tagline = MSLA/LCD resin slicer
splash-version = Version { $version }
splash-step-artwork = Loading interface artwork
splash-step-graphics = Starting the graphics device
splash-step-workspace = Preparing the workspace
splash-ready = Ready

## Workflow bar

action-new-project = New project
action-new-plate = New plate
action-new-plate-hint = A plate after this one, with its printer, resin, print setting and settings, and no model
action-duplicate-plate = Duplicate plate
action-rename-plate = Rename plate
action-delete-plate = Delete plate
action-delete-plate-hint = Delete the plate shown; Undo brings it back
action-previous-plate = Previous plate
action-next-plate = Next plate
action-plates-bar = Show plates bar
action-plates-bar-hint = The plates' thumbnails at the bottom of the view, or a row of tabs
menu-plate = Plate
plate-copy-name = { $name } (copy)
plate-rename-title = Rename plate
plate-rename-label = Name
plate-rename-action = Rename
plates-fold = Fold the plates bar
plates-unfold = Show the plates' thumbnails
plates-models = { $count ->
    [one] 1 model
   *[other] { $count } models
}
plates-card-hint = { $name } ({ $state }): click to show it, drag to move it among the plates, right-click for more
plates-state-none = Not sliced
plates-state-running = Slicing
plates-state-failed = Failed
plates-state-sliced = Sliced
plates-state-stale = Out of date
action-plates-overview = All plates
action-plates-overview-hint = Every plate side by side, with its profiles and its last slice
action-arrange-models = Arrange models
action-arrange-models-hint = Arrange the plate's models; those that do not fit go to a new plate or stay where they are
action-slice-every-plate = Slice every plate
action-slice-every-plate-hint = Slice, one after another, every plate not sliced as it is
action-export-every-plate = Export every plate
action-export-every-plate-hint = Export every plate's slice to a folder, slicing first those that need it
action-export-models = Export models
action-export-models-hint = Export the models as STL, 3MF or OBJ, as they stand on the plate, with their supports and drain holes
plates-overview-title = Plates ({ $count })
plates-overview-hint = Drag a card's image onto another to move the plate there; double-click it to show the plate
plates-overview-image-hint = { $name }: double-click to show it, drag onto another card to move it there
plates-overview-name = { $number } · { $name }
plates-overview-profiles = { $printer } · { $resin } · { $setting }
plates-overview-models = Models
plates-overview-layers = Layers · height
plates-overview-layers-value = { $layers } · { $height } mm
plates-overview-footprint = Models cover { $width } × { $depth } mm
plates-overview-open = Open
plates-overview-shown = This is the plate shown
plates-overview-export = Export the plate's last slice
plates-overview-last = The project keeps one plate at least
sidebar-clone-model = Clone
sidebar-check-object = Check { $name }
sidebar-check-all = Check every model
sidebar-check-all-hint = Check every model, or clear them all
sidebar-checked-none = None checked
sidebar-checked = { $count } checked
sidebar-hide-checked = Hide the { $count } checked (still printed)
sidebar-show-checked = Show the { $count } checked
sidebar-delete-checked = Delete the { $count } checked
delete-checked-title = Delete { $count } models?
delete-checked-text = The checked models leave the plate. Undo (Ctrl+Z) brings them all back.
sidebar-move-to-plate = Move to plate
clone-title = Clone { $model }
clone-copies = Copies
clone-plate = Plate
clone-plate-shown = { $plate } (shown)
clone-arrange = Arrange the copies
clone-arrange-hint = On: the copies go where there is room around the models already on the plate, which stay where they are, the plate's spacing apart; those that do not fit go to new plates after it. Off: every copy goes to the middle of the plate.
clone-action = Clone
clone-done = { $count ->
        [one] 1 copy of { $model } added to { $plate }
       *[other] { $count } copies of { $model } added to { $plate }
    }
clone-undo = Ctrl+Z removes every copy at once.
sidebar-copy-to-plate = Copy to plate
sidebar-export-model = Export model
plates-moved = { $model } moved to { $plate }
plates-moved-beside = { $model } moved to { $plate }, beside the models there
plates-copied = { $model } copied to { $plate }
plates-copied-beside = { $model } copied to { $plate }, beside the models there
plates-transfer-no-room = No room for { $model } on { $plate }, clear of the models there: it stays where it is
plates-transfer-loading = A model is still loading: try again once it has opened
arrange-title = Arrange models · { $plate }
arrange-spacing = Spacing between models
arrange-models = Models
arrange-models-all = All on the plate ({ $count })
arrange-overflow = Models that do not fit
arrange-overflow-new = Go to a new plate
arrange-overflow-stay = Stay where they are
arrange-hint-new = Footprints include supports and rafts. A new plate takes this plate's printer, resin, print setting and slicing options, and is arranged the same way; more plates follow while models are left.
arrange-hint-stay = Footprints include supports and rafts. What fits is arranged; the rest stays where it is, and the notification says so.
arrange-action = Arrange
arrange-done = Arranged: { $count ->
        [one] 1 model
       *[other] { $count } models
    } on { $plate }
arrange-moved = { $count ->
        [one] 1 did not fit and went to
       *[other] { $count } did not fit and went to
    } { $new ->
        [one] a new plate,
       *[other] new plates,
    } { $plates }.
arrange-stayed = { $count ->
        [one] 1 did not fit and stays where it was.
       *[other] { $count } did not fit and stay where they were.
    }
arrange-undo = Ctrl+Z undoes the whole arrangement.
arrange-no-printer = Choose a printer for the plate first: Arrange places the models on its plate
arrange-loading = A model is still loading: arrange once it has opened
export-all-title = Export every plate
export-all-folder = Folder
export-all-choose = Choose
export-all-folder-title = Folder for every plate's file
export-all-names = File names
export-all-names-hint = each plate's format
export-all-plate = Plate
export-all-file = File
export-all-format = Format
export-all-state = State
export-all-slice-first = Slice the plates not sliced, or out of date, first
export-all-slice-first-hint = Off: those plates are left out and listed in the notification
export-all-note = An existing file is never replaced without asking. Each file is written whole or not at all.
export-all-action = { $count ->
        [one] Export 1 plate
       *[other] Export { $count } plates
    }
export-all-waiting = Waiting
export-all-slicing = Slicing…
export-all-exported = Exported · { $size } MB
export-all-left-out = Left out
export-all-failed = Failed
export-all-unchecked = Not exported
export-all-no-folder = The folder cannot be created
export-all-replace-title = { $count ->
        [one] Replace a file?
       *[other] Replace { $count } files?
    }
export-all-replace-text = These files are in the folder already:
    { $files }
export-all-replace-action = Replace
batch-status = Slicing plate { $number } of { $count } · { $plate }{ $percent }
batch-stop = Stop
batch-plate-message = { $plate }: { $text }
batch-all-sliced = Every plate is sliced as it is
batch-plate-deleted = the plate was deleted
batch-not-started = the slice did not start
batch-changed = the plate changed while it was sliced
export-models-title = Export models
export-models-scope = Export
export-models-no-model = A model (select one)
export-models-plate = The plate · { $plate }
export-models-every = { $count ->
        [one] Every plate (1)
       *[other] Every plate ({ $count })
    }
export-models-format = Format
export-models-files = Files
export-models-per-plate = One file per plate
export-models-per-model = One file per model
export-models-supports = Supports
export-models-supports-hint = Each model's supports and braces, as they print
export-models-raft = Raft
export-models-raft-hint = Each model's raft, under its supports
export-models-content-joined = Each model as it stands on the plate, in millimetres: turned, scaled and hollowed, its drain holes cut, its supports and raft joined to it in one object.
export-models-content-objects = Each model as it stands on the plate, in millimetres: turned, scaled and hollowed, its drain holes cut. Each model, its supports and its raft are objects of their own: some programs rest each object on the plate apart.
export-models-objects = Objects
export-models-objects-joined = One per model
export-models-objects-separate = Model, supports and raft apart
export-models-names-hint = {"{"}project{"}"} {"{"}number{"}"} {"{"}plate{"}"} {"{"}model{"}"}
export-models-model = Model
export-models-working = Working…
export-models-action-one = Export
export-models-action = { $count ->
        [one] Export 1 file
       *[other] Export { $count } files
    }
export-models-folder-title = Choose the folder for the model files
export-models-phase = { $plate }: { $step }
export-models-failed = The models could not be exported
export-models-drilled = { $model }: drain holes cut; { $moved } vertices moved by at most { $distance } µm, nothing crosses
export-models-drilled-left = { $model }: drain holes cut; { $moved } vertices moved by at most { $distance } µm; { $crossings } crossing pairs and { $flat } flat triangles remain
export-models-specks = { $count ->
        [one] 1 speck under 15.6 µm left out
       *[other] { $count } specks under 15.6 µm left out
    }
export-models-separated = { $count ->
        [one] 1 vertex moved 0.06 µm apart where the model's bodies touched
       *[other] { $count } vertices moved 0.06 µm apart where the model's bodies touched
    }
export-models-cancelled = { $count ->
        [one] Model export cancelled after 1 file
       *[other] Model export cancelled after { $count } files
    }
export-models-done-one = Exported { $file }
export-models-done = { $count ->
        [one] Exported 1 model file
       *[other] Exported { $count } model files
    }
batch-exported = { $count ->
        [one] Exported 1 plate to { $folder }
       *[other] Exported { $count } plates to { $folder }
    }
batch-sliced = { $count ->
        [one] Sliced 1 plate
       *[other] Sliced { $count } plates
    }
batch-export-cancelled = Export every plate stopped: { $count } exported
batch-slice-cancelled = Slice every plate stopped: { $count } sliced
batch-left-out = Left out, not sliced as they are: { $plates }
batch-not-reached = { $count ->
        [one] 1 plate not reached
       *[other] { $count } plates not reached
    }
plates-drag-move = Drop on a plate to move { $model } there; hold Ctrl to copy it
plates-drag-copy = Drop on a plate to copy { $model } there
delete-plate-title = Delete { $name }?
delete-plate-text = The plate leaves the project with its { $count ->
        [one] model
       *[other] { $count } models
    }. Undo (Ctrl+Z) brings it back.
plates-overview-totals = All plates: { $models ->
        [one] 1 model
       *[other] { $models } models
    } · { $sliced } of { $plates } sliced{ $sliced ->
        [0] {""}
       *[other] {" "}· { $time } · { $volume } ml
    }
action-open = Open
action-save-as = Save as
action-undo = Undo
action-undo-hint = Undo the last change to the plate (Ctrl+Z)
action-redo = Redo
action-redo-hint = Redo the last undone change (Ctrl+Shift+Z or Ctrl+Y)
action-fit-view = Fit view
action-delete = Delete
action-hide = Hide
action-show = Show
action-isolate = Isolate
action-end-isolation = Show the others again
action-show-all = Show all
action-overhangs = Show overhangs
action-overhangs-hint = Mark the surfaces that need support, from the critical angle in the settings
strip-options = { $button }: options
action-place = Place
action-place-hint = Start or stop placing supports, or drain holes, with the tool in use
action-surface = Outside or cavity
action-surface-hint = Place on the model's outside, or in its cavity where the section shows it (a hollowed model)
action-visibility-hint = Hidden models are not drawn, but they are still printed
action-cancel-or-deselect = Cancel the drain-hole tool or clear the selection
action-toggle-tools = Show or hide the workflow tools
action-toggle-sidebar = Show or hide the sidebar
action-toggle-axes = Move and turn along the plate's axes (global) or the model's own (local)
transform-lock = Lock: no changes
transform-unlock = Unlock: allow changes
transform-reset-rotation = Reset the rotation
transform-reset-scale = Reset the scale
transform-uniform-on = Uniform: a typed value scales all three axes
transform-uniform-off = Not uniform: a typed value scales one axis
transform-show-handles = Show these handles in the viewport
action-center-model = Center
action-fit-to-plate = Fit to plate
transform-global = Global
transform-local = Local
action-quit = Quit

## Menu bar (M6.5, TD-098)

menu-file = File
menu-open-recent = Open recent
menu-edit = Edit
menu-view = View
menu-windows = Windows
menu-help = Help
action-about = About karasu-slicer
about-title = About karasu-slicer
about-licence = Licence: { $licence }.
about-licence-text = Read the licence
about-source = Source code:
about-trademark = The name karasu-slicer and its logo are trademarks of Ivan Maruca and are not licensed with the code.
about-notices = The licences of the third-party components are listed in THIRD-PARTY-NOTICES.txt, installed with the program.

## Workflow bar (M6.5, TD-098)

workflow-prepare = Prepare
workflow-hollowing = Hollowing
workflow-supports = Supports
workflow-export = Export
workflow-step-later = This step's tools come in a later milestone
workflow-show-tools = Show tools
workflow-hide-tools = Hide tools
tool-select = Select
tool-transform = Transform
tool-orient = Orient
tool-repair = Repair
tool-hollow = Hollow
tool-drain-holes = Drain holes
tool-raft = Raft
tool-automatic = Automatic
tool-islands = Islands
tool-braces = Braces
tool-supports = Manual
tool-slice = Slice and export
tool-layers = Layers
panel-collapse = Collapse the panel
panel-expand = Expand the panel
panel-no-model = No model selected

## Sidebar (M6.5, TD-098)

sidebar-plate = Plate
sidebar-close = Close the sidebar
sidebar-open = Open the sidebar
sidebar-objects = Models ({ $count })
sidebar-objects-shown = Models ({ $count }) · { $shown } shown
sidebar-hidden = { $name } is hidden from the view, but still printed. Show it with its eye.
sidebar-hide-object = Hide { $name } (still printed)
sidebar-show-object = Show { $name }
sidebar-no-objects = No models yet
sidebar-delete-object = Delete { $name }
delete-model-title = Delete { $name }?
delete-model-text = The model leaves the plate. Undo (Ctrl+Z) brings it back.
sidebar-bottom-layers = { $count } bottom layers
action-drop-to-plate = Drop to plate
mode-move = Move
mode-rotate = Rotate
mode-scale = Scale
dialog-open-title = Open models, profiles or a project
dialog-filter-all-openable = Projects, models and profiles
dialog-filter-projects = karasu-slicer projects
dialog-open-project-title = Open a project
dialog-save-title = Save the project
dialog-filter-models = Models
dialog-filter-profiles = Printer or resin profiles

## Viewport and status bar

viewport-empty-hint = Drop models and printer or resin profiles here
# Written on the tab outside the plate's front edge, lying on the plate.
plate-front = Front
status-objects = { $count ->
    [one] { $count } model
   *[other] { $count } models
}
status-triangles = { $count ->
    [one] { $count } triangle
   *[other] { $count } triangles
}
status-floating = { $count ->
    [one] { $count } model above the plate: slicing it needs supports
   *[other] { $count } models above the plate: slicing them needs supports
}
status-outside = { $count ->
    [one] { $count } model outside the build volume
   *[other] { $count } models outside the build volume
}
status-importing = Importing { $names }
status-checking = Checking { $names } for defects
status-gpu = GPU: { $adapter } · { $backend } · { $samples }x
status-gpu-none = GPU: not selected

## Preparation previews

preparation-starting = Starting
preparation-running = Preparing: { $phase }
preparation-refused = The plate cannot be prepared
preparation-objects-failed = { $count ->
    [one] { $count } model cannot be prepared
   *[other] { $count } models cannot be prepared
}

## View cube

viewcube-top = TOP
viewcube-bottom = BOTTOM
viewcube-front = FRONT
viewcube-back = BACK
viewcube-right = RIGHT
viewcube-left = LEFT
viewcube-hint = Click a face, an edge or a corner to look from there
viewcube-home = Back to the starting view

## Live log window (TD-096)

log-title = Log
log-shown-everything = Everything
log-shown-warnings = Warnings and errors
log-shown-errors = Errors
log-all-areas = All areas
log-search = Search the log
log-follow = Follow
log-follow-hint = Keep the newest records in view
log-copy = Copy
log-clear = Clear the view
log-clear-hint = Hides the records so far; the log file keeps them
log-not-kept = No log is kept in this session.
log-empty = Nothing matches.
log-open-folder = Open the logs folder
log-report = Save a diagnostic report
log-hide-paths = Hide my home folder
log-hide-paths-hint = The report writes ~ for your home folder, where your user name appears in paths
log-report-title = Save a diagnostic report
log-report-filter = Text
log-report-saved = Saved the diagnostic report to { $path }
log-report-failed = The diagnostic report could not be saved

## Settings dialog (TD-097, M6.5 u3)

action-settings = Settings
action-lock-move = Lock moving
action-lock-rotate = Lock turning
action-lock-scale = Lock scaling
settings-title = Settings
settings-applied = Changes apply at once and are kept across restarts.
settings-appearance = Appearance
settings-appearance-note = Colours apply at once, in the viewport behind this window. The alpha is the colour's opacity.
settings-lighting = Lighting
settings-brightness = Brightness
settings-brightness-hint = How bright the scene is: at 100 % a face turned to you shows its exact colour.
settings-ambient = Light on faces seen edge-on
settings-ambient-hint = The light a face seen edge-on or in shadow keeps: more lights the shadows, less shows the relief (35 % by default).
settings-colours-models = Models
settings-colours-scene = Plate and scene
settings-colours-handles = Handles
settings-colours-holes = Drain holes
settings-colours-blockers = Hollowing blockers
settings-colours-supports = Supports
settings-colours-markers = Repair markers
settings-colours-more = More colours ({ $count })
settings-colours-reset = Restore the default colours and lighting
settings-colour-object = Model
settings-colour-selected = Selected model
settings-colour-selection-outline = Selection outline
settings-colour-section-cap = Section caps
settings-colour-background = Background
settings-colour-plate = Plate surface
settings-colour-grid = Grid
settings-colour-grid-major = Grid, every 50 mm
settings-colour-plate-border = Plate border
settings-colour-front-tab = Front tab
settings-colour-axis-x = X axis
settings-colour-axis-y = Y axis
settings-colour-axis-z = Z axis
settings-colour-hovered = Handle under the pointer
settings-colour-drain-hole = Drain holes
settings-colour-hole-valid = Drain hole being placed
settings-colour-support = Supports
settings-colour-support-error = Supports with an error
settings-colour-support-selected = Selected support
settings-colour-support-error-selected = Selected support with an error
settings-colour-support-section-cap = Support section caps
settings-colour-contact-line = Contact height line
settings-colour-marker-fails = Makes slicing fail
settings-colour-marker-hollowing = Blocks hollowing
settings-colour-marker-harmless = Handled by slicing
settings-colour-marker-repaired = Repaired
settings-colour-outline = Plate and volume outline
settings-colour-front-label = Front tab word
settings-colour-uniform = Handles
settings-colour-free-move = Free move
settings-colour-free-move-hovered = Free move, under the pointer
settings-colour-cavity = Cavity
settings-colour-cavity-selected = Cavity of the selected model
settings-colour-model-ghost = Model in the cavity view
settings-colour-hole-selected = Selected drain hole
settings-colour-hole-unchecked = Drain hole being placed, not checked yet
settings-outline = Outline the selected model
settings-contact-line = Show the contact's height line
settings-contact-line-hint = While a support is placed or its tip dragged, a line around the model at the contact's height helps keep a row of supports level; Shift holds the height
settings-outside = Parts outside the build volume: colour, opacity and pattern
settings-pattern-solid = Solid
settings-pattern-stripes = Stripes
settings-shortcuts = Keyboard shortcuts
settings-hollowing = Hollowing
settings-defaults-note = The values new operations start from. What is changed in a tool's panel stays while you work and is never saved here; the button beside a changed value, or Edit › Restore all default values, brings the default back.
settings-wall = Wall thickness
settings-cell = Grid cell
settings-new-supports = New supports
settings-support-weight-hint = The preset the Manual tool starts from: a weight, or one of your own
settings-supports = Supports
settings-supports-note = The supports' settings. A surface facing the plate needs support when its slope above level is under the critical angle.
settings-overhang-angle = Critical angle
settings-overhang-angle-hint = Measured from the plate's plane: 45° by default. A smaller angle marks only flatter surfaces.
settings-overhang-show = Show the overhangs on the models
settings-overhang-show-note = Surfaces that need support turn yellow at the angle's limit, red as they become level. View › Show overhangs (O) switches them too.
settings-orient = Automatic orientation
settings-colours-overhangs = Overhangs
settings-colour-overhang-limit = At the critical angle
settings-colour-overhang-level = Level, facing the plate
settings-colour-protected = Protected points (Orient)
settings-shortcuts-note = Click a shortcut to change it, then press the new keys; Escape cancels. A shortcut fires only while no field takes the keyboard.
settings-shortcuts-search = Search actions or keys
settings-shortcuts-reset-all = Restore all
settings-shortcuts-steps = Workflow steps
settings-shortcuts-tools = Tools
settings-shortcuts-section = Section
settings-shortcuts-strip = Tool strip
section-step-layer-up = Active handle up a layer
section-step-layer-down = Active handle down a layer
section-step-page-up = Active handle up ten layers
section-step-page-down = Active handle down ten layers
section-step-bottom = Active handle to the plate
section-step-top = Active handle to the top
section-step-hint = Moves the section slider's outlined handle, the one last dragged, clicked or typed into, without clicking the slider first
settings-shortcuts-press = Press the new shortcut…
settings-shortcuts-none = None: click to add
settings-shortcuts-change = Click, then press the new shortcut
settings-shortcuts-reset = Restore the default
settings-shortcuts-conflict = { $keys } is the shortcut of “{ $owner }”. Use it for “{ $action }” instead? “{ $owner }” is left without it.
settings-shortcuts-use-here = Use it here
settings-shortcuts-skipped = Some saved shortcuts could not be read and were left out
settings-models = Opening models
settings-models-note = Every model opened gets these repairs as its own; a model's repairs can still be changed in the Repair tool.
settings-projects = Projects
settings-recovery-minutes = Recovery copy every (minutes)
settings-recovery-note = While a project has unsaved changes, a copy is kept this often, to restore after a crash or a power cut; 0 keeps none. It is removed once the project is saved.
settings-repair-none-name = None: diagnose only
settings-repair-none = Defects are reported; the model is sliced as it is.
settings-repair-exact = Repairs that change no geometry: orientation, duplicate and degenerate faces.
settings-repair-standard = The exact repairs, then small holes closed and nearby edges joined.
settings-repair-aggressive = The standard repairs, then larger holes and self-intersections; the shape may change slightly.
settings-window = Window
settings-window-opens = The window opens
settings-open-as-closed = As it was closed
settings-open-maximized = Maximized
settings-open-fullscreen = Full screen
settings-open-windowed = As a window, centred
settings-window-option = The --window option still chooses for one start.
settings-window-places = Dialogs and tool windows reopen where and as large as you left them.
settings-window-forget = Forget sizes and places
settings-window-forgotten = Forgotten: each opens at its default size, centred, from the next start.
settings-language = Language
settings-language-en-us = English (United States)
settings-language-pseudo = Pseudo-locale (developer mode)
settings-language-note = Translations appear here as they are added; the interface changes at once. The command line is not translated.
settings-language-fixed = This start uses { $locale }, chosen with --locale.
settings-language-missing = The language chosen has no catalog: the interface stays in English
settings-file = Settings file
settings-file-path = File
settings-file-state = State
settings-file-ok = Read, and saved after every change
settings-file-unreadable = Could not be read: { $reason }
settings-file-newer = Saved by a newer version (schema { $schema })
settings-file-none = Settings are not kept in this session.
settings-file-note-ok = Every setting here is kept across restarts. Starting over puts the defaults back for every setting.
settings-file-note-unreadable = The defaults apply this session and nothing is saved over the file, so it can still be repaired by hand. Starting over keeps it beside the new one.
settings-file-note-newer = What this version knows was read; changes this session are not saved over the newer file. Starting over keeps it beside the new one.
settings-file-open-folder = Open the folder
settings-file-open-failed = The folder could not be opened
settings-file-start-over = Start over with the defaults
settings-start-over-title = Start over with the default settings?
settings-start-over-text = Every setting goes back to its default: colours, shortcuts, the window, the tools. Your printers, resins and projects are not touched.
settings-start-over-unreadable = Every setting goes back to its default, and the unreadable file is kept beside the new one.
settings-start-over-newer = Every setting goes back to its default for this version, and the newer file is kept beside the new one.
settings-start-over-action = Start over
settings-start-over-failed = The settings could not start over
settings-started-over = The settings started over with their defaults
settings-kept-as = The previous file is kept as { $path }

## Preparation panel (hollowing and drain holes)

prepare-hollow = Hollowing
prepare-hollow-hint = Removes the interior deeper than the wall thickness, leaving a certified wall; the model in the project is never changed
prepare-wall = Wall thickness
prepare-cell = Grid cell
prepare-cell-hint = The sampling grid of the hollowing: smaller follows the model more closely and is slower. The wall is guaranteed either way. Auto chooses it from the wall and the model's surface, so the wall is at most about 0.05 mm thicker than set
prepare-auto = Auto
prepare-wall-below-minimum = Below about { $minimum } mm this model's cavity is predicted to need more memory than this computer allows: hollowing will likely be refused
prepare-cell-too-fine = This grid cell is predicted to give this model a cavity that needs more memory than this computer allows: hollowing will likely be refused
prepare-thin-wall = Walls under 1 mm are fragile: they may crack or leak while printing and cleaning
prepare-simplified = The cavity is simplified, as the project asks
prepare-hollow-applied = Applied
prepare-hollow-changed = Changed, not applied yet
prepare-hollow-none = Not hollowed
prepare-apply-hollow = Apply hollowing
prepare-remove-hollow = Remove hollowing
prepare-remove-hollow-hint = The model becomes solid again; its drain holes stay
prepare-scaled-wall = Scaled with the model, the wall is { $min }–{ $max } mm thick: set it again and apply
prepare-scaled-wall-hint = The cavity scales with the model. Set the wall thickness again and apply it to change it
prepare-new-holes = New holes
prepare-hole-refused = { $hole }: { $reason }
prepare-drainage = Drainage
prepare-no-cavity = No cavity
prepare-holes-solid = Solid model: new holes go { $depth } mm into it
prepare-drains = Every cavity drains
prepare-no-drain-hole = The cavity has no drain hole and will trap uncured resin
prepare-undrained = { $count ->
    [one] A cavity region has no drain hole and will trap uncured resin
   *[other] { $count } cavity regions have no drain hole and will trap uncured resin
}
prepare-hollowed = Hollowed: the wall is guaranteed { $wall } mm and every cavity drains
prepare-to-holes-hint = Open the Drain holes tool
prepare-hole = Hole { $number }
prepare-hole-remove = Remove this hole
prepare-add-holes = Add drain holes
prepare-holes-unavailable = Drain holes cannot be added to this model
prepare-placing = Click the model to add a hole; Escape stops
prepare-placing-hint = A green hole can be made where the pointer is, a red one cannot. Shape the selected hole with its handles or in its fields, then apply the new holes
prepare-new-hole = New hole { $number }
prepare-apply-holes = Apply holes ({ $count })
prepare-apply-holes-hint = Cut the new holes into the model
prepare-discard-holes = Discard
prepare-new-depth-hint = The depth of new holes, from the surface they start from. By default 1 mm past the wall of a hollowed model, or 3 mm into a solid one
prepare-start = Start from
prepare-start-outer = Outer surface
prepare-start-cavity = Cavity
hole-diameter = Diameter
hole-depth = Depth
hole-depth-hint = Auto reaches 1 mm past the cavity wall, wherever the hole is. A fixed depth is measured from the surface; the hole goes as deep as you set it, even through the model
hole-tilt = Tilt
hole-tilt-hint = The angle between the hole and the surface's perpendicular, up to 60°
hole-tilt-direction = Tilt direction
hole-finding-closed = This hole does not open to the outside, so it cannot drain
hole-finding-through = This hole passes through the model and opens on both sides
hole-finding-no-cavity = This hole reaches no cavity
hole-finding-in-material = This hole ends inside the material
prepare-pending = Preparing…
prepare-failed = This model cannot be prepared
prepare-solid = Solid model
prepare-needs-profiles = Choose a printer and a resin to prepare the model
prepare-guaranteed-wall = Guaranteed wall
prepare-guaranteed-wall-hint = The thinnest wall, proven over the whole cavity: at least the wall thickness set
prepare-thickest-wall = Thickest spots
prepare-thickest-wall-hint = The cavity follows the sampling grid, so the wall is thicker in places: this is the thickest wall measured at the corners of the cavity
prepare-passes = Passes
prepare-passes-hint = When the first cavity leaves the wall too thin, further passes move it inwards until the wall is guaranteed
prepare-length-mm = { $length } mm
prepare-cavity-volume = Cavity
prepare-material-volume = Material
prepare-volume-mm3 = { $volume } mm³
prepare-result = Result
prepare-resin-saved = Resin saved
prepare-resin-saved-hint = The cavity's share of the solid model's volume: resin the print does not take
prepare-percent = { $percent } %
prepare-done-hollowed = Hollowed: the wall is guaranteed { $wall } mm
prepare-done-removed = The model is solid again; its drain holes stay
prepare-done-failed = The model cannot be prepared
prepare-done-holes = { $count ->
    [one] One drain hole cut
   *[other] { $count } drain holes cut
}
prepare-done-undrained = { $count ->
    [one] Drain holes cut, but a cavity region has no drain hole and will trap uncured resin
   *[other] Drain holes cut, but { $count } cavity regions have no drain hole and will trap uncured resin
}
prepare-enclosed = The cavity is enclosed and will trap uncured resin: add drain holes
prepare-region-enclosed = Cavity region { $number } ({ $volume } mm³) has no drain hole and will trap uncured resin
prepare-region-drained = Cavity region { $number } ({ $volume } mm³) drains through hole { $holes }
prepare-tiny-regions = { $count ->
    [one] One enclosed pocket under 1 mm³ will trap a drop of uncured resin
   *[other] { $count } enclosed pockets under 1 mm³ will trap drops of uncured resin
}
prepare-region-show = Show
prepare-region-show-hint = Frame this region and section the model through it
prepare-pocket = Pocket { $number } ({ $volume } mm³)
prepare-pocket-tip = Pocket { $number } ({ $volume } mm³) will trap a drop of uncured resin
prepare-pockets-list = List the pockets
prepare-pockets-hide = Hide the pockets
prepare-pockets-more = { $count ->
    [one] and one more
   *[other] and { $count } more
}
prepare-cavity-markers = Cavity markers
prepare-marker-region = A region without a drain hole
prepare-marker-pocket = A pocket under 1 mm³
region-back = Back to the whole model

## Section panel

section-title = Section
section-hint = Drag the handles to cut the models below and above a layer; double-click the slider to show everything. Cut material is filled, so walls show their thickness
section-layer-label = Layer
section-z-label = Z
section-menu-from = From Z { $z } mm (layer { $layer })
section-add-adaptive = Add adaptive layers from here
section-add-fixed = Add fixed layers from here
section-add-needs-profiles = Choose a printer and a resin first: ranges start above their calibrated bottom layers
section-edit-range = Edit { $what }
section-remove-range = Remove { $what }
section-show-all = Show everything
layers-adaptive-name = adaptive layers
layers-fixed-name = fixed layers
layers-adaptive-from = Adaptive { $min }–{ $max } mm from { $from } mm
layers-fixed-from = Fixed { $height } mm from { $from } mm
layers-add-adaptive-title = Add adaptive layers
layers-add-fixed-title = Add fixed layers
layers-edit-title = Edit { $what }
layers-range = Range
layers-start-layer = Starts at layer
layers-start-height = Starts at height
layers-until = Until
layers-until-top = the top of the models
layers-until-next = the next range, at { $height } mm
layers-adaptive-heights = Adaptive heights
layers-thinnest = Thinnest layer
layers-thinnest-hint = Steep slopes and fine detail get layers this thin
layers-thickest = Thickest layer
layers-thickest-hint = Vertical walls get layers this thick
layers-step = Largest step
layers-step-hint = The largest stair step the layers may leave on a slope
layers-growth = Growth per layer
layers-growth-hint = How fast layers may thicken from one to the next; they thin at once
layers-fixed-height = Fixed height
layers-height = Layer height
layers-height-hint = Every layer of the range has this height
layers-own-exposure = Own exposure (else from the table below)
layers-exposures = Exposures
layers-exposures-note = Provisional: kept with the plate until the resin profiles hold them. A height between two rows takes an exposure in between; outside the table it has none, and the range cannot be applied
layers-add-row = Add a row
layers-remove-row = Remove this row
layers-apply = Apply
layers-applied = Layer heights set: { $what }
layers-removed = Layer range removed: { $what }. The plate slices with the heights below it; Undo brings it back
layers-check-base = The bottom and transition layers end at { $base } mm and keep the calibrated height: start at { $base } mm or above
layers-check-same-start = Another range starts at this height
layers-check-table = The exposure table needs increasing heights up to 1 mm and exposures above 0 and at most { $max } s
layers-check-limits = This printer takes layers from { $min } to { $max } mm thick
layers-check-min-max = The thinnest layer must not be thicker than the thickest
layers-check-cover = The exposure table must reach from { $min } to { $max } mm: add the rows around them
layers-check-sl1 = This file format stores one layer height: slicing will refuse varying layers
layers-check-above = The range starts above the models and changes nothing
slice-layers-about = about { $count }
slice-layers-about-hint = Adaptive layers are planned here from the models as opened; slicing plans them from the repaired models, so the count may differ slightly
slice-plan-error = The layer plan cannot be used: { $reason }

## Printer and resin profiles (TD-106)

printer-choose = Choose a printer
resin-choose = Choose a resin
action-edit-profiles = Printer and resin library
profiles-printer = Printer
profiles-resin = Resin
profiles-print-setting = Print setting
setting-choose = Choose a print setting
library-unavailable = The profile library cannot be opened
library-empty = Your library has no printer yet. Add the printer you own from the catalog, or describe a custom one.
action-add-your-printer = Add your printer
action-add-printer = Add printer
action-add-printer-short = Add printer
action-add-resin = Add resin
action-add-resin-short = Add resin
action-manage-library = Manage the library
action-new-setting = New print setting
action-edit-print-setting = Edit the print setting
action-new-print-setting = Enter the values
action-create = Create
action-save = Save
action-revert = Revert
action-apply = Apply
action-dismiss = Dismiss
action-review = Review
action-import = Import
action-delete-profile = Delete
resin-none-set-up = No resin of yours is set up for { $printer }
resin-not-set-up = Not set up for { $printer }
menu-search = Search
menu-no-match = No match
resin-set-up = { $name }: set up
setting-official = Official
setting-starting-point = Starting point
setting-user = Yours
field-name = Name
field-name-in-library = Name in your library
name-unique-hint = The name tells this printer apart from your others, so each name is unique. Add the same model as often as you have machines of it.
field-vendor = Vendor
vendor-none = None
field-kind = Kind
field-film = Release film
film-not-stated = Not stated
field-density = Density (g/ml)
field-cost = Cost per litre
field-colour = Colour
field-colour-hint = As the maker names it, such as Grey. Resins of one kind in different colours are different resins, each with its own print settings: colour changes the exposure.
field-swatch = Swatch
field-swatch-hint = Shown beside the resin's name, so resins are told apart at a glance
field-heating = Heating temperature
field-heating-hint = The vat's heating target, for printers that heat it; 25 °C unless set
field-build-volume = Build volume
field-panel = Panel (pixels)
field-pixel = Pixel
field-pixels-x = Pixels X
field-pixels-y = Pixels Y
field-width = Width
field-depth = Depth
field-height = Height
field-formats = Formats
field-release = Release
release-z-lift = Lift along Z
release-tilt = Tilting vat
field-sources = Sources
field-source = Source
field-orientation = Orientation
orientation-verified = Verified
orientation-not-verified = Not verified
orientation-hint = Verified: the panel's orientation was compared with the manufacturer's slicer output for this printer. Not verified: print the orientation test before relying on it.
orientation-title = How the panel shows the image
orientation-explain = A wrong choice prints the part mirrored. Pick how the panel must show a part that looks like this "F" on the plate, seen from above.
orientation-as-preview = As the preview
orientation-mirror-xy = Mirrored in X and Y (LCD_mirror)
orientation-mirror-x = Mirrored in X
orientation-mirror-y = Mirrored in Y
orientation-test-hint = Not sure? Print a small "F" and compare it with the preview.
field-layer-height = Layer height
field-exposure = Exposure
field-bottom-exposure = Bottom exposure
field-bottom-layers = Bottom layers
field-transition-layers = Transition layers
field-transition-layers-hint = Between the bottom and the normal layers, the exposure steps down linearly; motion and rests are the normal layers'.
field-light-power = Light power
field-bottom-light-power = Bottom light power
field-light-power-hint = The LED's power, 255 for full power; not every file format stores it.
field-antialias = Anti-aliasing
field-antialias-off = Off
field-antialias-hint = Off gives binary masks; 2, 4, 8 or 16 samples per pixel side give smoother edges with grey pixels.
field-min-gray = Lowest grey
field-min-gray-hint = Partly covered pixels never go below this grey, for resins that do not cure below it.
field-xy-compensation = XY compensation (px)
field-xy-compensation-hint = Positive grows the solid and shrinks holes; negative the reverse.
field-bottom-xy-compensation = Bottom layers instead (px), against elephant's foot
kind-not-stated = Not stated
kind-standard = Standard
kind-abs-like = ABS-like
kind-water-washable = Water-washable
kind-tough = Tough
kind-flexible = Flexible
kind-plant-based = Plant-based
kind-clear = Clear
kind-castable = Castable
kind-other = Other
add-printer-title = Add printer
catalog-tab = From the catalog
custom-printer-tab = Custom printer
custom-resin-tab = Custom resin
catalog-search = Search
catalog-vendor = Vendor
catalog-all = All
catalog-in-library = in your library
catalog-no-match = No printer matches. Describe it as a custom printer instead.
catalog-no-match-short = No format matches
formats-none = No format yet: add the ones the printer reads
formats-remove = Remove this format
formats-add = Add format
formats-all = Show every format
formats-all-hint = Off, the menu lists only the formats { $vendor } printers read
discard-title = Discard the changes?
discard-text = The changes you made in this window are not saved and will be lost.
catalog-no-settings = The catalog has no official print setting for this printer yet: add a resin and enter its values.
catalog-resins-to-add = Resins to add with their print settings for this printer:
catalog-resins-all = All
catalog-resins-all-hint = Tick every resin, or clear them all
catalog-resins-chosen = { $count } of { $total } chosen
custom-printer-note = A custom printer starts without print settings: add a resin, then start from a setting or enter its values.
printer-new-name = My printer
printer-identity = Identity
printer-panel = Panel
printer-timing = Timing
field-fixed-layer-time = Fixed time per layer
field-fixed-layer-time-hint = What the printer spends on every layer besides its exposure, rests and moves, such as preparing the next image; added to the print time estimate.
printer-edit-title = Printer
resin-edit-title = Resin
add-resin-title = Add resin
add-resin-for = For
add-resin-no-printer = No printer
add-resin-official = With official settings for { $printer }
add-resin-others = Other resins
add-resin-official-for = Official settings for
add-resin-settings-added = These print settings are added:
add-resin-material-only = The resin is added without print settings.
add-resin-already-set-up = This resin is already set up for this printer.
add-resin-start-from = No official setting exists for this resin on this printer. Start from one of the printer's settings: the copy is marked Starting point until you edit it.
add-resin-no-source = The printer has no print setting to start from yet: the resin is added without one, and you enter its values.
add-resin-without-setting = Add it without a print setting
add-resin-no-setting-added = Added { $name } without a print setting for { $printer }: enter its values to slice with it
setup-title = Set up { $resin } for { $printer }
setup-explain = The catalog has no print setting for this resin on this printer. Start from one of this printer's settings: the copy is marked Starting point until you edit it, so calibrate it before relying on it.
setup-none = Neither the catalog nor your library has a print setting for { $printer } to start from. Enter every value yourself, from the resin maker's table or a calibration print.
setup-from = Start from
setup-from-nothing = Nothing: enter every value
setting-new-title = New print setting
setting-new-explain = A print setting belongs to one printer and one resin. Start from one of the printer's settings, copied as a starting point to calibrate, or enter every value yourself.
setup-created = Created a starting point for { $resin }: calibrate it before relying on it
setting-title = Print setting
setting-for = For { $printer } and { $resin }
setting-enter-every-value = Enter every value: these are placeholders, not settings for this resin.
setting-tab-exposure = Exposure
setting-tab-motion = Rests and motion
setting-tab-image = Image
setting-unsaved = Unsaved changes: saving makes it yours
setting-no-changes = No changes
motion-rests = Rests
motion-normal = Normal layers
motion-bottom = Bottom layers
rest-after-retract = Rest after retract
rest-after-retract-aka = Before the exposure, once the plate is down. Photon Workshop calls it Off Time, CHITUBOX Rest Time After Retract, Lychee Wait Before Print.
rest-before-lift = Rest before lift
rest-before-lift-aka = After the exposure, before the plate rises. Photon Workshop: Wait Before Lift; CHITUBOX: Rest Time Before Lift; Lychee: Wait After Print.
rest-after-lift = Rest after lift
rest-after-lift-aka = At the top of the lift, before the plate comes down. Photon Workshop: Wait After Lift; CHITUBOX: Rest Time After Lift; Lychee: Wait After Lift.
motion-title = Platform motion
motion-two-stages = Two stages
motion-near-film = Near the film
motion-away-from-film = Away from the film
motion-distance = Lift distance
motion-lift-speed = Lift speed
motion-retract-distance = Retract distance
motion-retract-speed = Retract speed
motion-hint = The plate lifts through the stage near the film first and retracts through it last, so its retract speed is the last one.
update-title = Catalog update for { $name }
update-explain = The catalog's revision { $revision } changes these settings:
update-setting = Setting
update-yours = Yours
update-catalog = Catalog
update-projects = Apply takes the catalog's values for these settings. Projects keep their own copies.
update-applied = { $name } now has the catalog's values
update-dismissed = Update dismissed for { $name }
library-title = Library
library-tab-printers = Printers
library-tab-resins = Resins
library-tab-settings = Print settings
library-tab-empty = Nothing here yet.
library-column-name = Name
library-column-based-on = Based on
library-column-state = State
library-action-edit = Edit
library-action-duplicate = Duplicate, with its print settings
library-action-export = Export to a file
library-action-delete = Delete
library-custom = Custom
library-based-on = { $name }, catalog revision { $revision }
library-state-current = Up to date
library-state-update = Update available
library-update-available = Catalog update available
library-update-hint = A newer catalog revision changes this printer: review it
library-footer = Your profiles never change on their own: a newer catalog revision is offered for review. Deleting here never changes a project.
library-delete-title = Delete { $name }?
library-delete-with = { $count ->
    [0] Nothing else goes with it.
    [one] Its print setting goes with it.
   *[other] Its { $count } print settings go with it.
}
library-delete-setting = Only this print setting is deleted.
library-delete-projects = Projects keep their own copies.
library-deleted = Deleted { $name }
library-duplicated = Added { $name }, with the print settings
library-added = Added { $name } to your library
library-saved = Saved { $name }
library-failed = The library refused the change
library-name-taken = The name { $name } is already in use: choose another
library-export-title = Export a profile
library-exported = Exported { $path }
library-export-failed = The profile could not be exported
library-import-title = Import profiles
library-imported = Imported { $names }
library-import-failed = Cannot import { $name }
library-converted = Your profiles were converted to the new library: { $printers } printers, { $resins } resins and { $settings } print settings
library-converted-kept = The original files are kept in { $path }

## Repair menu

repair-before-slicing = Repairs before slicing
repair-duplicates = Merge duplicate faces
repair-orientation = Make face orientation consistent
repair-outward = Turn inside-out bodies outward
repair-outward-hint = Closed bodies whose faces all point inward are turned outward
repair-fill-holes = Fill holes in the model
repair-fill-holes-hint = { $edges ->
    [one] Caps holes whose rim has { $edges } edge; larger holes stay open
   *[other] Caps holes whose rim has up to { $edges } edges; larger holes stay open
}
repair-slicing = Slicing
repair-close-holes-up-to = Close holes in each layer, up to
repair-slivers-up-to = Leave reversed slivers empty, up to
repair-slivers-hint = Self-intersecting folds can leave tiny inside-out slivers in a layer. Up to this area per layer they stay empty and the slice summary reports them; a larger inside-out area still fails
repair-per-layer-mm2 = mm² per layer
repair-allow-defects = Slice despite mesh defects
repair-allow-defects-hint = Every layer must still close exactly, or slicing fails

## Slicing and export

format-none-supported = This printer lists no supported format
format-reference = Reference (karasu's own, for comparison and tests)
action-slice = Slice
slice-hidden-printed = { $count ->
    [one] 1 hidden model is printed too
   *[other] { $count } hidden models are printed too
}
action-slice-disabled = Needs models, a printer, a resin and a supported format
action-export = Export
action-export-disabled = Slice the plate first; a result older than the plate's last edit cannot be exported
action-cancel = Cancel
action-close = Close
action-discard = Discard
dialog-export-title = Export the sliced plate
progress-title = Slicing
progress-starting = Starting
progress-slicing-layers = Slicing layers
progress-seconds = { $seconds } s
progress-layers = { $done } / { $total } layers
slice-facts = { $layers ->
    [one] { $layers } layer · { $time } · { $volume } ml resin
   *[other] { $layers } layers · { $time } · { $volume } ml resin
}
slice-output = Output
slice-printer = Printer
slice-resin = Resin
slice-print-setting = Print setting
slice-exposure-value = { $normal } s · bottom { $bottom } s × { $count }
slice-pixels = { $value } px
slice-starting-point-hint = A starting point copied from another resin's setting: calibrate it before relying on it
slice-layer-height = Layer height
slice-layer-height-hint = From the print setting
slice-format = File format
slice-from-setting = The print setting's: { $value }
slice-antialias-hint = For this plate: Off gives binary masks; 2, 4, 8 or 16 samples per pixel side give smoother edges with grey pixels. The print setting's unless changed here
slice-antialias-binary-format = The plate's format stores binary masks only
slice-antialias-supports = Anti-alias the supports
slice-antialias-supports-hint = Off: the supports are binary, whole pixels at their centres, and only the model is anti-aliased
slice-antialias-supports-off = off, binary supports
slice-verify = Verify the printed walls
slice-verify-hint = Slower: checks the thinnest wall, breaches and pockets layer by layer
slice-layers = Layers
slice-height = Height
slice-time = Print time
slice-volume-ml = { $volume } ml
slice-state-running = Slicing the plate…
slice-state-profiles = Choose a printer, a resin and a print setting to slice
slice-state-no-models = Open a model to slice
slice-state-failed-hint = Details and fixes are in Messages
slice-state-none = Not sliced yet
slice-state-stale = Out of date: the plate changed after slicing. Slice again to export
slice-state-done = Sliced: { $facts }, ready to export
layers-no-slice = Slice the plate to see its layers
layers-follow = Follow in the viewport
layers-follow-hint = The section cuts the models at the layer shown
layers-this-layer = This layer
layers-z-fact = Height (Z)
layers-z-hint = The layer's top above the plate: where the build platform stops for it
layers-thickness = Thickness
layers-exposure = Exposure
layers-seconds = { $seconds } s
layers-kind = Kind
layers-kind-bottom = Bottom
layers-kind-transition = Transition
layers-kind-normal = Normal
layers-kind-plan = From the layer plan
layers-area = Exposed area
layers-area-hint = The cured area of the layer, from its mask; edge pixels count by their intensity
layers-mm2 = { $area } mm²
layers-islands = Islands
layers-islands-hint = Separate cured regions in this layer; a region with nothing cured under it needs a support
layers-zoom-out = Zoom out
layers-zoom-in = Zoom in
layers-open-window = Open in a larger window
layers-first = First layer
layers-previous = Previous layer
layers-next = Next layer
layers-last = Last layer
layers-of = of { $count }
layers-slider-hint = Drag or click to choose a layer
layers-stale = Out of date: these layers are of the plate before its last change
layers-shown = Layer { $layer } of { $count } · Z { $z } mm
walls-title = Printed walls
walls-passed = Walls verified
walls-failed = Wall verification failed
walls-go = Show layer { $layer }
walls-thinnest = Thinnest wall { $measured } mm (at least { $guaranteed } mm)
walls-no-cavity = No cavity
walls-thick = Thicker than { $wall } mm everywhere
walls-thin = { $count } cavity voxels under { $wall } mm
walls-breaches = Breached in { $count } voxels
walls-no-breach = No breach
walls-drained = Every cavity drains
walls-pockets = { $count ->
    [one] One pocket does not drain
   *[other] { $count } pockets do not drain
}
walls-pocket = Pocket { $number }: { $volume } mm³, layer { $layer }
walls-more-pockets = { $count } more
walls-legend-thin = thin wall
walls-legend-breach = breach
walls-legend-pocket = pocket
layers-note = The exported mask, reduced to fit and seen from above, with the plate's front at the bottom.

## Units of value fields (M6.5)

unit-mm = mm
unit-mm2 = mm²
unit-degree = °
unit-percent = %
unit-second = s
unit-ratio = ×
unit-mm-per-s = mm/s
unit-mm-per-min = mm/min
unit-celsius = °C

## Settings kept across restarts (TD-097)

settings-unreadable = Your settings could not be read: the defaults apply this session, and the file is left as it is
settings-newer = Your settings were saved by a newer version: changes this session are not saved over them
settings-not-saved = Your settings could not be saved: they are tried again after the next change

## Toast notifications (M6.5)

toast-info = Information
toast-success = Success
toast-warning = Warning
toast-error = Error
toast-close = Close the notification
toast-details = Details in Messages

## Messages

messages-title = Messages
messages-button = Messages ({ $count })
messages-button-new = Messages ({ $count } new)
messages-hover = Open Messages for the details
messages-none = No messages
messages-details = Details
action-clear = Clear
error-no-cache = There is no cache folder for sliced files
slice-plate-refused = The plate cannot be sliced
slice-cancelled = Slicing cancelled
slice-lost = Slicing stopped unexpectedly
slice-done = Sliced: { $facts }
slice-repairs-applied = Repairs were applied while slicing
slice-slivers-left = Small reversed slivers were left empty
slice-cavity-enclosed = A cavity has no drain hole opening it: it will trap uncured resin
slice-support-errors = Supports with errors were sliced as they are: check them before printing
slice-slivers-detail = Self-intersecting folds in the model left inside-out slivers: { $slivers }.
export-done = Exported { $path }
export-failed = The export failed
notice-cannot-open-profile = Cannot open a profile
notice-cannot-open-model = Cannot open a model
notice-cannot-check = { $name }: cannot check for mesh defects
notice-assumed-unit = { $name } declares no unit; it was read in { $unit }.
project-untitled = Untitled

## The project file (TD-130)

project-saved = Saved { $name }
project-not-saved = { $name } was not saved
project-opened = Opened { $name }
project-not-opened = { $name } could not be opened
project-recent-missing = That project file is no longer there
project-one-at-a-time = Only one project opens at a time: the first one chosen was opened.
loading-title = Opening { $name }
loading-reading = Reading the project file
loading-preparing = Preparing the models
loading-supports = Building the supports
loading-show-now = Show now
unsaved-title = Unsaved changes
unsaved-text = { $name } has changes that are not saved. Save them before going on?
unsaved-discard = Discard
recovery-title = Recover unsaved work
recovery-text = karasu-slicer closed without saving these projects. Restore one to go on with it, or discard them.
recovery-restore = Restore
recovery-discard = Discard all
recovery-later = Later
recovery-restored = Restored { $name }; save it to keep it
recovery-not-written = The recovery copy of the project could not be written
recovery-just-now = just now
recovery-minutes-ago = { $count ->
    [one] { $count } minute ago
   *[other] { $count } minutes ago
}
recovery-hours-ago = { $count ->
    [one] { $count } hour ago
   *[other] { $count } hours ago
}
recovery-days-ago = { $count ->
    [one] { $count } day ago
   *[other] { $count } days ago
}
plate-default-name = Plate { $number }

## Failed slices: the cause in a few words

slice-failed = Slicing failed; see Messages for the reason
slice-failed-inside-out = Slicing failed: part of the model is inside out
slice-failed-orientation = Slicing failed: some faces point the wrong way
slice-failed-holes = Slicing failed: the model has holes
slice-failed-defects = Slicing failed: the model has mesh defects
slice-failed-floating = Slicing failed: a model is above the plate without supports
slice-failed-outside = Slicing failed: a model lies outside the printable area

## Fixes offered by messages

fix-close-holes = Close holes in each layer
fix-tolerate-reversed = Leave small reversed slivers empty
fix-drop-to-plate = Drop lifted models to the plate
fix-and-slice = { $fix } and slice again
fix-applied = { $fix } (on)

## Opened models: what the diagnosis found

opened-clean = Opened { $name }: no mesh defects found
opened-findings = { $name }: { $findings }
finding-short-inside-out = inside-out bodies
finding-short-orientation = faces pointing the wrong way
finding-short-duplicates = duplicate faces
finding-short-holes = holes
finding-short-touching = touching bodies
finding-short-non-orientable = non-orientable bodies
finding-short-flat = flat bodies
finding-short-empty = no usable faces
finding-short-cracks = cracks
finding-short-degenerate = zero-area faces
finding-short-pinched = pinched vertices
finding-short-small = specks
finding-short-fins = fins
finding-short-dropped = facets without a position
finding-inside-out = { $count ->
    [one] { $count } closed body is inside out: its faces point inward
   *[other] { $count } closed bodies are inside out: their faces point inward
}
finding-orientation = { $count ->
    [one] Some faces point the wrong way in { $count } body
   *[other] Some faces point the wrong way in { $count } bodies
}
finding-duplicates = { $count ->
    [one] { $count } face is listed more than once
   *[other] { $count } faces are listed more than once
}
finding-holes = { $holes ->
    [one] The surface is open: { $holes } hole
   *[other] The surface is open: { $holes } holes
} ({ $edges ->
    [one] { $edges } open edge
   *[other] { $edges } open edges
})
finding-touching = { $count ->
    [one] { $count } edge is shared by more than two faces, as where bodies touch
   *[other] { $count } edges are shared by more than two faces, as where bodies touch
}
finding-non-orientable = { $count ->
    [one] { $count } body cannot be oriented consistently
   *[other] { $count } bodies cannot be oriented consistently
}
finding-flat = { $count ->
    [one] { $count } body encloses too little volume to tell inside from outside
   *[other] { $count } bodies enclose too little volume to tell inside from outside
}
finding-empty = The model has no usable faces
finding-cracks = { $count ->
    [one] { $count } hole is a crack of zero area along a line
   *[other] { $count } holes are cracks of zero area along a line
}
finding-degenerate = { $count ->
    [one] { $count } face has zero area
   *[other] { $count } faces have zero area
} ({ $needles } with two corners at one point, { $caps } with three corners on a line)
finding-pinched = { $count ->
    [one] { $count } vertex joins parts that touch only there
   *[other] { $count } vertices join parts that touch only there
}
finding-fins = { $count ->
    [one] { $count } edge has an extra open face, as a flap or a hanging sheet
   *[other] { $count } edges have an extra open face, as a flap or a hanging sheet
}
finding-small = { $count ->
    [one] { $count } body has less than 1 mm² of surface
   *[other] { $count } bodies have less than 1 mm² of surface
}
finding-dropped = { $count ->
    [one] 1 facet has no position (NaN or infinite coordinates) and was left out
   *[other] { $count } facets have no position (NaN or infinite coordinates) and were left out
}
finding-line = { $finding }. { $consequence }
consequence-harmless = Slicing fills them correctly
consequence-outward-handled = The plate turns them outward
consequence-orientation-handled = The plate makes orientation consistent
consequence-duplicates-handled = The plate merges them
consequence-holes-handled = Small gaps are closed in each layer; a layer with a wider gap fails
consequence-inside-out = Slicing will fail unless inside-out bodies are turned outward
consequence-holes = Slicing will fail unless holes are closed or filled
consequence-unfixable = Slicing may fail; fix the model in its modelling application
consequence-empty = There is nothing to slice
consequence-repairable = Slicing will fail unless the plate repairs it
consequence-cracks-handled = The plate closes them
consequence-fins-handled = The plate removes the extra faces
consequence-degenerate = Slicing and hollowing leave them out; they enclose no material
consequence-small = They print as specks
consequence-dropped-handled = What they formed is lost, as its place is unknown; the rest of the model is sliced
consequence-dropped = Slicing refuses the model until its repairs accept leaving them out (Exact does)

## Model repairs (TD-092): the Repair tool's panel

repair-checking = Checking the model…
repair-state-ready = Ready to print
repair-state-notes = Ready: slicing handles what remains
repair-state-hollowing = Slices, but hollowing needs a stronger repair
repair-state-needs = Needs repair: slicing would fail
repair-state-as-imported = As imported: { $state }
repair-show = Show
repair-show-hint = Turn the view towards this defect
repair-preset-plate = Plate repairs
repair-preset-off = No repairs
repair-preset-exact = Exact (moves no vertex)
repair-preset-standard = Standard (1 µm tolerances)
repair-preset-aggressive = Aggressive
repair-preset-rebuild = Rebuild as a solid
repair-preset-custom = Custom
repair-stages = Stages
repair-stage-non-finite = Leave out facets without a position
repair-stage-non-finite-hint = Facets with NaN or infinite coordinates have no place in the model; without this, slicing refuses a file that has them
repair-stage-weld = Weld vertices on open edges within
repair-stage-degenerate = Remove zero-area faces, collapse and flip within
repair-stage-fins = Remove fins (extra open faces)
repair-stage-cut = Also cut faces of edges left non-manifold
repair-stage-visibility = Turn open parts to face outside
repair-stage-split = Split holes at pinch vertices
repair-stage-refine = Refine and fair caps (adds vertices)
repair-stage-unbounded = Holes of any size
repair-stage-cracks = Close cracks within
repair-stage-shells = Remove small shells
repair-stage-shell-area = Smaller than
repair-stage-inner = Remove bodies inside other bodies
repair-stage-rebuild = Rebuild as a solid
repair-stage-rebuild-hint = The last resort: the model is rebuilt from its inside on a grid of this cell; it is always closed, and the report gives how far it moved
repair-rebuild-resolution = Cell
repair-report-none = The repairs changed nothing
repair-report-dropped = { $count ->
    [one] 1 facet without a position left out at import (facet { $first })
   *[other] { $count } facets without a position left out at import, in { $runs } runs from facet { $first }
}
repair-report-weld = Vertices welded: { $count } (largest move { $mm } mm)
repair-report-degenerate = Zero-area faces removed: { $needles }; short edges collapsed: { $edges }; flat faces flipped: { $flips }
repair-report-duplicates = Duplicate faces removed: { $count }
repair-report-flipped = Faces turned: { $count } (bodies turned outward: { $outward })
repair-report-fins = Fins removed: { $count }; faces cut: { $cut }
repair-report-visibility = Open parts turned to face outside: { $count }
repair-report-holes = Holes filled: { $filled } (largest { $area } mm²); cracks closed: { $cracks }; holes left open: { $left }
repair-report-shells = Small shells removed: { $small }; inner bodies removed: { $inner }
repair-report-rebuild = Rebuilt with { $mm } mm cells: within { $deviation } mm of the model; details lost up to { $lost } mm
slice-options = Slicing options
repair-try-hint = The next stronger repair; undo returns to this one
repair-remove = Remove repairs
repair-advanced = Advanced
repair-apply = Apply repairs
repair-applied = Applied: { $preset }
repair-changed = Changed, not applied yet
repair-defects = Defects ({ $count })
repair-markers = Markers
repair-use = Use { $preset }
repair-done-ready = Repaired: ready to print
repair-done-notes = Repaired: ready, slicing handles what remains
repair-done-hollowing = Repaired, but hollowing needs a stronger repair
repair-done-needs = Still not printable: slicing would fail
repair-done-failed = The repair failed
notice-auto-repair = Repairs on opening: { $preset }
fix-repair = Repair: { $preset }
slice-failed-open = A layer does not close: the model has holes or extra faces
slice-failed-non-finite = The model file has facets without a position (NaN or infinite coordinates)
repair-show-markers = Show markers on the model
repair-marker-tip = { $what }: { $effect }
repair-marker-fails = slicing fails here
repair-marker-hollowing = blocks hollowing
repair-marker-harmless = slicing handles it
repair-marker-repaired = repaired
repair-marker-rim = Hole edge
repair-marker-crack = Zero-area crack
repair-marker-non-manifold = Edge shared by more than two faces
repair-marker-winding = Faces pointing opposite ways
repair-marker-zero-area = Zero-area face
repair-marker-duplicate = Duplicate face
repair-marker-opposite = Face and its reverse
repair-marker-pinch = Bodies touching at one vertex
repair-marker-small = Tiny shell
repair-marker-non-orientable = Body that cannot be oriented
repair-marker-duplicates-collapsed = Duplicate faces merged
repair-marker-reoriented = Faces turned
repair-marker-turned-outward = Inside-out body turned outward
repair-marker-hole-filled = Hole filled
repair-marker-crack-closed = Crack closed
repair-marker-welded = Vertices welded
repair-marker-zero-area-removed = Zero-area face removed
repair-marker-edge-collapsed = Short edge collapsed
repair-marker-flat-flipped = Flat face flipped
repair-marker-fin-removed = Fin removed
repair-marker-faces-cut = Faces cut from a non-manifold edge
repair-marker-small-removed = Tiny shell removed
repair-marker-inner-removed = Body inside another removed
repair-marker-visibility = Open part turned outward
repair-marker-hole-refused = Hole left open
repair-marker-flat-body = Body too flat to orient
repair-marker-deviation = Largest change of the rebuilt solid

## The Supports tool (TD-108).
supports-new = New supports
supports-avoid = Avoid the model
supports-avoid-hint = Where a shaft straight down would go through the model below, a new support goes aside, its tip leaning to a knee, or stands on the model below; off, it goes straight down
supports-preset = Preset
supports-preset-hint = Light, medium or heavy supports, or a preset of your own: choosing one sets the values below
supports-presets-mine = Mine
supports-preset-custom = Custom
supports-preset-selected-hint = The preset whose values this support has, or Custom when they were changed: choosing one gives the support its values at once
supports-preset-save = Save these values as a new preset
supports-preset-new-name = My preset { $number }
supports-preset-update = Update this preset with these values
supports-preset-updated = Preset “{ $name }” updated
supports-preset-rename = Rename this preset
supports-preset-delete = Delete this preset
supports-preset-name = The preset's name
supports-preset-confirm = Save
supports-preset-cancel = Cancel
supports-preset-taken = Another preset has this name
supports-preset-empty = The preset needs a name
supports-preset-delete-title = Delete the preset
supports-preset-delete-text = Delete the preset “{ $name }”? Supports already placed keep their values.
supports-preset-delete-action = Delete
supports-light = Light
supports-medium = Medium
supports-heavy = Heavy
supports-contact = Contact diameter
supports-contact-hint = Where the tip touches the model; at most the shaft's diameter
supports-contact-fit = Fit contact to the surface
supports-contact-fit-hint = Where the contact would hang over an edge or pass through a thin wall, it narrows until it lies on the model, its penetration with it; it never widens
supports-contact-min = Smallest contact
supports-contact-min-hint = The contact narrows down to this diameter at most; where even this one does not fit, it is built and reported
supports-contact-built = Contact as built
supports-contact-built-hint = The contact's diameter after fitting it to the surface
supports-tip-length = Tip length
supports-tip-length-hint = From the contact to the shaft
supports-shaft = Shaft diameter
supports-shaft-hint = The support's body
supports-penetration = Penetration
supports-penetration-hint = How deep the tip goes into the model
supports-surface-width = At the surface
supports-surface-width-hint = How wide the contact is where it meets the model: the mark it leaves. The contact's diameter is its end's, inside the model; the tip widens from there towards its body
supports-contact-shape = Contact
supports-contact-shape-hint = How the tip ends on the model: a flat cone, or a ball
supports-cone = Cone
supports-sphere = Sphere
supports-sphere-diameter = Ball diameter
supports-sphere-diameter-hint = The ball at the contact; at least the contact's diameter
supports-tip-body = Tip body diameter
supports-tip-body-hint = Where the tip joins the shaft
supports-shaft-bottom = Shaft bottom diameter
supports-shaft-bottom-hint = A shaft wider at its foot; automatic keeps it straight
supports-pad-diameter = Pad diameter
supports-pad-diameter-hint = The foot on the plate
supports-pad-thickness = Pad thickness
supports-pad-thickness-hint = How thick the foot on the plate is
supports-cone-diameter = Cone diameter
supports-cone-diameter-hint = The cone from the pad to the shaft, where it meets the pad
supports-cone-height = Cone height
supports-cone-height-hint = From the pad's top to the shaft
supports-foot-contact = Foot contact diameter
supports-foot-contact-hint = A foot standing on the model, where it touches it
supports-foot-body = Foot body diameter
supports-foot-body-hint = A foot standing on the model, where it joins the shaft
supports-foot-length = Foot length
supports-foot-length-hint = A foot standing on the model, from its contact to the shaft
supports-foot-penetration = Foot penetration
supports-foot-penetration-hint = How deep a foot goes into the model
supports-sides = Sides
supports-sides-hint = Facets around each round part
supports-tip-direction = Tip direction
supports-tip-direction-hint = Along the surface's normal, or straight at the shaft; a handle turns it by hand
supports-tip-normal = From the surface
supports-tip-straight = Towards the shaft
supports-tip-turned = As turned
supports-checks = Checks
supports-clearance = Clearance
supports-clearance-hint = The least distance between a support and the model, proven for each support
supports-tip-limit = Tip angle limit
supports-tip-limit-hint = How far a tip may turn from the surface's normal
supports-dragging = Release where the support should be; Escape puts it back
action-cavity-view = Cavity view
action-cavity-view-hint = Shows the selected hollowed model's cavity from inside, with its outside as a ghost, and places new supports in the cavity
supports-knee-follows = Knee follows the foot
supports-knee-follows-hint = Moving a support's foot moves its knee too, so the shaft stays vertical; off, the knee stays and the shaft leans
supports-straighten = Remove the knee
supports-straighten-hint = The shaft goes straight down from the tip again
supports-support = Support { $number }
supports-support-inside = Support { $number } (inside)
supports-surface-outer = Outside
supports-surface-cavity = Cavity
supports-placing-cavity = Cut the model with the section to see its cavity, then click its ceiling
supports-placing-cavity-hint = The section's handles at the viewport's right show the inside of the model; a support goes from the cavity's ceiling down to its floor
supports-remove = Remove this support
supports-accept = Accept the collision
supports-accept-hint = The support prints through the model there; the finding stays reported
supports-placing = Click the model to add a support; Escape stops
supports-placing-foot = Click where the foot goes: on the plate, or on the model below; Escape takes the tip back
supports-placing-foot-hint = The shaft stands vertical under its knee, which leans the tip at most 45° from vertical
supports-place-one-click = One click: the whole support
supports-place-tip-then-foot = Tip, then foot
supports-place-branch = Branch onto a support
supports-placing-hint = A support goes where the pointer meets the selected model, with its tip along the surface and its shaft down to the plate
supports-facing-up = This surface faces up: a support's tip cannot reach it from below

## Show (owner, 2026-10-31): which of a tool's items the viewport shows.
action-show-items = Show
action-show-items-hint = Shows every item, only those with errors, or only the selected one; the others are hidden and cannot be picked. With errors in the list window turns the others into ghosts instead
show-all = All
show-errors = Only with errors
show-selected = Only the selected one
show-minis = Only the mini supports
settings-colour-filtered-ghost = Items left out by the list's filter

## Mini supports (owner, 2026-10-30): a thin rod with a ball at each end, standing on the model.
action-mini = Mini supports
action-mini-hint = New supports are mini supports: a thin rod with a ball at each end, no tip cone and no foot, standing on the model below or on another support's shaft. They hold spots with too little room below for a support, or no way down to the plate
supports-mini-diameter = Diameter
supports-mini-diameter-hint = The mini support's diameter, its rod and the balls at its ends
supports-mini-penetration = Penetration
supports-mini-penetration-hint = How deep each ball goes into the model; at zero it only touches the surface
supports-mini-support = Mini support { $number }
supports-mini-support-inside = Mini support { $number } (inside)
supports-mini-row = { $stands } · mini
supports-mini-no-foot = No surface below to stand on: a mini support needs the model below its tip
supports-mini-no-foot-hint = Straight below the tip, a mini support stands on the first surface facing up, its rod at most 45° from vertical; with Tip, then foot, click the foot where it goes
supports-placing-mini-foot = Click where the foot goes, on the model below; Escape takes the tip back
supports-placing-mini-foot-hint = The rod goes straight from the tip to the foot, which stands on a surface facing up below the tip
supports-minis-fallback = Mini supports where a support does not fit
supports-minis-fallback-hint = Where a support finds too little room below its tip, or no way down, a mini support stands on the model straight below the tip instead
supports-building = Building the supports…
supports-build-failed = The supports cannot be built
supports-with-errors = { $count ->
    [one] A support has an error: it prints as it is
   *[other] { $count } supports have errors: they print as they are
}
supports-errors-status = { $count ->
    [one] 1 support with an error
   *[other] { $count } supports with errors
}
supports-none = No supports
supports-count = { $count ->
    [one] 1 support, built without errors
   *[other] { $count } supports, built without errors
}
supports-unprepared = The model could not be prepared
supports-finding-soft = A value outside its recommended range
supports-finding-off-surface = Its tip is not on the model's surface
supports-finding-axis = Its tip turns too far from the surface
supports-finding-piercing = Its tip goes through a thin wall
supports-finding-contact-fit = Its contact does not lie on the model even at its smallest
supports-finding-collision = It crosses the model
supports-finding-accepted = It crosses the model (accepted)
supports-finding-drain-hole = It enters a drain hole through the model and may partly plug it
supports-finding-foot-in-hole = Its foot stands where a drain hole cuts the model away: nothing is under it
supports-finding-volume = It leaves the build volume
supports-finding-path = Its shaft cannot reach its foot
supports-finding-steep = Its shaft leans too far
supports-finding-parent = The support it stands on cannot hold it
supports-finding-raft = The raft has no support on the plate

## List windows (owner, 2026-10-29): a tool's items in a tool window of their own.
action-tips = Tips only
action-tips-hint = Shows only the supports' tips, so their shafts, bases and rafts do not hide the model's lower parts; a click picks a support by its tip
action-list = List
action-list-hint = Shows or hides the list of the selected model's supports, or drain holes, in a window of its own
list-all = All
list-state = State
list-ok = OK
list-number = #{ $number }
list-row-hint = Click to select it; double-click to bring the view to it
supports-list-title = Supports ({ $count })
supports-list-stands = Stands
supports-list-errors = With errors ({ $count })
supports-list-minis = Mini ({ $count })
supports-list-summary = { $count } supports, { $errors } with errors
supports-stands-plate = On the plate
supports-stands-model = On the model
supports-stands-cavity = In the cavity
supports-stands-support = On a support
supports-state-accepted = Collision accepted
supports-short-off-surface = Off the surface
supports-short-axis = Turned too far
supports-short-piercing = Through a wall
supports-short-collision = Crosses the model
supports-short-drain-hole = In a drain hole
supports-short-foot-in-hole = Foot in a drain hole
supports-short-volume = Outside the volume
supports-short-path = No way to its foot
supports-short-raft = Raft without supports
supports-selected = Selected support
holes-axes-surface = Surface
holes-list-title = Holes ({ $count })
holes-list-size = Size
holes-list-refused = Refused ({ $count })
holes-list-summary = { $count } holes, { $refused } refused
holes-list-new = New { $number }
holes-list-fact = Ø { $diameter } mm · { $depth }
holes-list-depth = { $depth } mm deep
holes-list-auto-depth = automatic ({ $depth } mm)
holes-state-refused = Refused
holes-state-checking = Checking
holes-state-new = Not applied
holes-selected = Selected hole

## The Raft tool (TD-108): the raft under the selected model's supports.
raft-on = Raft under the supports
raft-on-hint = The supports standing on the plate stand on a raft instead of their own pads; a new raft starts from the values in Settings › Supports
raft-thickness = Thickness
raft-thickness-hint = The raft's thickness; the shafts stand on its top through their fillets
raft-margin = Margin
raft-margin-hint = How far the raft reaches beyond the outermost fillets
raft-slope = Side slope
raft-slope-hint = The raft's sides, from the plate: 90° is vertical
raft-type = Type
raft-type-slab = Slab
raft-type-skate = Skate
raft-type-web = Web
raft-type-hint = Slab: the sides slope outwards down to the plate, so the raft is widest where it holds the plate. Skate: they slope inwards, so the raft is narrowest at the plate and a scraper slips under its edge. Web: a disc under each support and low bars joining neighbouring supports in triangles
raft-bar-longest = Longest bar
raft-bar-longest-hint = Supports farther apart than this are not joined by a bar
raft-bar-width = Bar width
raft-bar-width-hint = The bars' width at their top; they widen down to the plate by the side slope
raft-bar-height = Bar height
raft-bar-height-hint = The bars' height above the plate
raft-base = Base
raft-base-single = Single
raft-base-groups = Groups
raft-base-hint = One raft under every support on the plate, or one under each group of supports standing close together
raft-join = Join feet closer than
raft-join-hint = Supports whose feet are closer than this share a raft, directly or through others; farther ones get one of their own
raft-fillet-diameter = Fillet diameter
raft-fillet-diameter-hint = Where a shaft meets the raft: the width of the cone joining them, at the raft
raft-fillet-height = Fillet height
raft-fillet-height-hint = The joining cone's height above the raft; 0 joins the shaft straight
raft-count = Rafts
raft-under = Under
raft-under-count = { $count ->
    [one] 1 support on the plate
   *[other] { $count } supports on the plate
}
raft-footprint = Footprint
raft-footprint-size = { $width } × { $depth } mm
raft-state-waiting = The raft waits for a support standing on the plate
raft-state-no-base = The skate raft has nothing left at the plate: make it thinner, its sides steeper or its margin wider
raft-state-on = A raft under { $count } supports, the model { $lift } mm above the plate
raft-state-off = No raft: the model stands { $lift } mm above the plate
settings-new-raft = New rafts

## The Braces tool (owner, 2026-10-29).
action-auto-braces = Automatic braces
action-auto-braces-hint = Shows the braces the panel's values would add between neighbouring supports; Apply adds them, Discard leaves them
braces-new = New braces
braces-diameter = Diameter
braces-diameter-hint = The diameter of the next braces, placed or automatic; with the automatic diameter on, the largest they take. Its default is set in Settings › Supports
braces-auto-diameter = Automatic diameter
braces-auto-diameter-hint = Each brace is as thick as the thinner of the two shafts where it meets them, up to the largest diameter, and follows them when their values change
braces-largest-diameter = Largest diameter
braces-built-diameter = As built
braces-auto = Automatic braces
braces-distance = Maximum distance
braces-distance-hint = Supports farther apart than this, seen from above, are not joined
braces-lowest = Lowest brace
braces-lowest-hint = Height of the lowest braces above the plate, or above the raft when there is one
braces-step = Step
braces-step-hint = Height between a pair's braces; they stop below the lower tip
braces-pattern = Pattern
braces-pattern-hint = How each pair of neighbouring supports is joined between two heights
braces-pattern-horizontal = Horizontal
braces-pattern-zigzag = Zigzag
braces-pattern-cross = Cross
braces-selected = Selected brace
braces-selected-title = Brace #{ $number }
braces-selected-hint = This brace's diameter; it changes this brace alone. With the automatic diameter on, the largest it takes; the diameter worked out from its shafts is shown below
braces-joins-label = Joins
braces-joins = #{ $from } – #{ $to }
braces-height-on = Height on #{ $number }
braces-height-first = Height on the first support
braces-height-second = Height on the second support
braces-height-hint = Where the brace meets the support's shaft, in mm above the plate
braces-place-single = One by one: two clicks, one brace
braces-place-chain = Chain: each click joins the previous support
braces-place-as-clicked = As clicked: each end where its support is clicked
braces-place-level = Level: the second end at the first one's height
braces-placing = Click a support where a brace starts; Escape stops
braces-placing-hint = A brace's end goes where the pointer meets a support's shaft
braces-placing-second = Click another support where the brace ends; Escape takes the first end back
braces-placing-second-hint = The brace is added at once, as one step that Undo takes back
braces-placing-chain = Click the next support; Escape ends the chain
braces-placing-chain-hint = Each click adds a brace from the support clicked before
braces-need-supports = Braces join two supports: add supports first
braces-none = No braces yet
braces-state = { $count ->
    [one] One brace, built without errors
   *[other] { $count } braces, built without errors
}
braces-state-errors = { $count } braces, { $errors } with errors
braces-state-errors-hint = A brace with an error prints as it is; the list says why
braces-auto-planning = Planning the braces…
braces-auto-failed = The braces cannot be planned
braces-auto-state = { $count } braces to add, { $skipped } skipped
braces-skipped-model = { $count } would touch the model
braces-skipped-hole = { $count } would cross a drain hole
braces-skipped-joined = { $count } already joined there
braces-auto-apply = { $count ->
    [one] Apply one brace
   *[other] Apply { $count } braces
}
braces-auto-discard = Discard
braces-auto-added = { $count ->
    [one] One brace added (Undo takes it back)
   *[other] { $count } braces added (Undo takes them back)
}
braces-list-title = Braces ({ $count })
braces-list-summary = { $count } braces, { $errors } with errors
braces-remove = Delete this brace
settings-new-braces = New braces

## Deleting all of a tool's items (owner, 2026-10-29).
delete-all-supports = Delete all supports
delete-all-braces = Delete all braces
delete-all-holes = Delete all drain holes
delete-all-hint = Deletes them all from the selected model, once confirmed; Undo brings them back
delete-all-action = Delete all
delete-all-supports-title = Delete all supports?
delete-all-supports-text = { $braces ->
    [0] { $count ->
        [one] The selected model's support will be deleted.
       *[other] The selected model's { $count } supports will be deleted.
    }
   *[other] The selected model's { $count } supports will be deleted, and with them its { $braces } braces.
} Its raft stays. Undo brings them back.
delete-all-braces-title = Delete all braces?
delete-all-braces-text = { $count ->
    [one] The selected model's brace will be deleted.
   *[other] The selected model's { $count } braces will be deleted.
} Undo brings them back.
delete-all-holes-title = Delete all drain holes?
delete-all-holes-text = { $count ->
    [one] The selected model's drain hole will be deleted.
   *[other] The selected model's { $count } drain holes will be deleted.
} Undo brings back the applied ones; new holes not yet applied are dropped.

## The Islands tool (owner, 2026-10-29).
action-islands = Show islands
action-islands-hint = Shows the islands found: red without a support, green held, faint when the plate has changed since
action-minima = Show local minima
action-minima-hint = Shows the selected model's local minima: points lower than everything around them, where a layer starts with nothing below
islands-search = Search
islands-angle = Critical angle
islands-angle-hint = A surface steeper than this from the plate holds itself; its default is set in Settings › Supports
islands-hide-under = Hide islands under
islands-hide-under-hint = Smaller islands are left out of the markers, the list and the counts; Support the islands still supports them unless Skip islands under leaves them out
islands-hidden = { $count ->
    [one] One island under { $area } mm² hidden
   *[other] { $count } islands under { $area } mm² hidden
}
islands-island = Island
islands-of = Island { $number } of { $count }
islands-previous = Previous island
islands-next = Next island
islands-layers = Layers
islands-layer-span = { $first }–{ $last }
islands-height = Height
islands-height-mm = { $height } mm
islands-area = Area
islands-area-mm2 = { $area } mm²
islands-where = Where
islands-inside = In the cavity
islands-where-hint = Outside, or on the ceiling of a hollowed model's cavity (the cavity view, V, shows it). Inside the cavity too the resin cures in the vat with nothing holding it: such an island needs its support as much, from the cavity's floor
islands-outside = Outside
islands-support = Support
islands-held = Held by #{ $support }
islands-free = None
islands-support-here = Support it here
islands-support-here-hint = A support under its lowest point, with the Manual tool's values
islands-support-here-cavity-hint = A support from its lowest point down to the cavity's floor, with the Manual tool's values
islands-find = Find islands
islands-find-again = Find islands again
islands-find-hint = Slices the selected model without its supports and finds what starts in mid-air
islands-stop = Stop
islands-stop-hint = Stops searching; the islands found before stay
islands-finding = Finding islands… { $percent } %
islands-not-searched = Islands not searched yet
islands-out-of-date = The plate changed: find the islands again
islands-state-free = { $count ->
    [one] One island has no support
   *[other] { $count } islands have no support
}
islands-state-held = Every island is supported
islands-found-none = No island: nothing starts in mid-air
islands-found-held = { $count ->
    [one] One island, supported
   *[other] { $count } islands, every one supported
}
islands-found-free = { $count } islands; { $free } without a support
islands-took = Found in { $seconds } s
islands-failed = The islands could not be found
islands-refused = The model cannot be sliced to find its islands
islands-no-format = The printer writes no format the slicer knows: choose another printer
islands-stopped = The search stopped
islands-list-title = Islands ({ $count })
islands-list-fact = First layer and area
islands-list-everywhere = Everywhere
islands-list-outside = Outside ({ $count })
islands-list-cavity = In the cavity ({ $count })
islands-list-free = Without a support ({ $count })
islands-list-row = layer { $layer } · { $area } mm²{ $place }
islands-list-inside = { " · " }in the cavity
islands-list-not-searched = Not searched yet: Find islands, in the panel
islands-list-current = Found on the plate as it is
action-support-islands = Support the islands
action-support-islands-hint = Shows the supports planned under every island without one, with the panel's values; Apply adds them, Discard leaves them
islands-supports = Island supports
islands-preset = Preset
islands-preset-hint = What the islands' supports are made with: one of the Manual tool's presets. Its default is set in Settings › Supports
islands-join = Join islands closer than
islands-join-hint = Islands this close, across and in height, share one support
islands-skip-under = Skip islands under
islands-skip-under-hint = Smaller islands get no support from Support the islands; Support it here still supports them
islands-foot-on-model = Foot on the model when the plate is out of reach
islands-foot-on-model-hint = When no way down reaches the plate, the support's foot stands on the model below; off, such an island is skipped and said. In the cavity a support always stands on its floor
islands-planning = Planning the islands' supports…
islands-preparing-surface = Preparing the model's surface for routes…
islands-plan = { $branches ->
    [0] { $count } supports for { $islands } islands
   *[other] { $count } supports, { $branches } of them branches, for { $islands } islands
}
islands-plan-skipped = { $branches ->
    [0] { $count } supports for { $islands } islands; { $skipped } skipped
   *[other] { $count } supports, { $branches } of them branches, for { $islands } islands; { $skipped } skipped
}
islands-skip-out-of-reach = { $count } with no way down to the plate: turn on "Foot on the model", or support them in the Manual tool
islands-skip-no-way = { $count } with no way down clear of the model: support them in the Manual tool
islands-skip-no-surface = { $count } with no underside found under them
islands-skip-too-low = { $count } too close to the plate for the preset: raise the Automatic tool's Lift and generate again, or use a lighter preset
islands-skip-drain-hole = { $count } where a drain hole cuts the material away: nothing there to hold
islands-skip-foot-in-hole = { $count } whose support's foot would stand where a drain hole cuts the model away
islands-skip-collides = { $count } whose support would come closer to the model than the clearance
islands-skip-pierces = { $count } whose tip would reach through a thin part of the model
islands-skip-unbuildable = { $count } whose support cannot be built there with the preset: too close to the plate (raise the Automatic tool's Lift) or a lighter preset
islands-apply = { $count ->
    [one] Apply one support
   *[other] Apply { $count } supports
}
islands-apply-hint = Adds the green supports as one undoable step
islands-discard = Discard
islands-supports-added = { $count } supports for { $islands } islands added, as one step (Undo takes them back); { $skipped } skipped. Find the islands again to check the rest.
islands-here-added = { $count ->
    [one] One support holds island { $number }.
   *[other] { $count } supports hold island { $number }.
} Find the islands again to check the rest.
islands-here-none = Island { $number } got no support
islands-here-too-low = It starts { $room } mm above where supports stand, and a support of this preset needs { $needs } mm under its tip. Lift the model by { $more } mm more (Raft › Lift), or use a lighter preset.
settings-island-supports = Island supports
delete-island-supports = Delete the islands' supports
delete-island-supports-title = Delete the islands' supports?
delete-island-supports-text = { $count ->
    [one] The support the Islands tool placed on the selected model will be deleted, with the braces on it.
   *[other] The { $count } supports the Islands tool placed on the selected model will be deleted, with the braces on them.
} Supports placed otherwise stay. Undo brings them back.
supports-from-islands = { $stands } · island

## The Automatic tool (M6.4i, TD-112).
action-left = Show what is left unsupported
action-left-hint = Marks the places the plan leaves without support
auto-cover = Cover
auto-cover-all = Islands and overhangs
auto-cover-islands = Islands only
auto-cover-hint = Islands and overhangs: everything the analysis finds. Islands only: what starts in mid-air, as Support the islands does; overhangs are left to you
auto-place-everywhere = Everywhere
auto-place-outside = Outside
auto-place-cavity = Cavity only
auto-place-hint = Where on a hollowed model: everywhere, only outside, or only inside the cavity, from its ceiling to its floor. Generate replaces only the automatic supports of that place
auto-place-not-hollowed = Only a hollowed model has a cavity
auto-angle-hint = Surfaces facing the plate less steeply than this need support; its default is the overhangs' angle, set in Settings › Supports
auto-skip-under-hint = Smaller islands get no support
auto-supports = Supports
auto-spacing = Tip spacing
auto-spacing-hint = How far apart tips stand under a level surface; steeper surfaces take them further apart, up to the spacing at the critical angle
auto-steep-spacing = Spacing at the critical angle
auto-steep-spacing-hint = How far apart tips stand where the surface is as steep as the critical angle
auto-lift = Lift
auto-lift-hint = How high the model's lowest point stands above the plate: Generate lifts it there first, as one undoable step, and the supports standing on the plate grow with it
auto-density = Density
auto-density-hint = How many tips stand on the same area: 100% is what the spacings give; 200% twice as many tips, 50% half as many
auto-density-spacing = Tips every { $level } mm under a level surface, { $steep } mm at the critical angle
auto-cavity-own = Own values in the cavity
auto-cavity-own-hint = A hollowed model's cavity takes its own preset and density: its supports are not seen and hold a lighter part. Off, the cavity takes the outside's
auto-cavity-preset = Preset in the cavity
auto-cavity-preset-hint = What the cavity's supports are made with: one of the Manual tool's presets
auto-cavity-density = Density in the cavity
auto-cavity-density-hint = How many tips stand on the same area in the cavity: 100% is what the spacings give; 50% half as many
auto-wide = Closer under wide undersides
auto-wide-hint = An island or an overhang whose underside spans more than the area below takes its tips closer, in proportion to its area, down to the closest share: a wide surface pulls on the film and sags more
auto-wide-from = Wide from
auto-wide-from-hint = The underside area from which the tips stand closer
auto-wide-closest = As close as
auto-wide-closest-hint = The closest the tips stand under a wide underside, as a share of the spacing
auto-passes = Closing check: at most
auto-passes-hint = Where a support finds no way down, the next uncovered points are tried, pass after pass, up to this many times
auto-feet = Feet on the model
auto-feet-hint = When no way down reaches the plate, the support's foot stands on the model below. It leaves a mark there; off, such a place is left unsupported and said
auto-trees = Trees and braces
auto-branches = Branches
auto-branches-hint = Close tips branch from one trunk, and trunks close together merge, the main one thicker below
auto-braces = Braces
auto-braces-hint = Braces between neighbouring supports, as the Braces tool's automatic braces make them, with its values
auto-braces-summary = { $pattern }, from { $lowest } mm every { $step } mm, supports within { $distance } mm, { $diameter }
auto-braces-summary-hint = The Braces tool's values, shared with it: turn on Advanced to change them
auto-braces-diameter-auto = as thick as the thinner shaft, at most { $diameter } mm
auto-braces-diameter = { $diameter } mm thick
auto-planned = Planned
auto-minis-count = Mini supports
auto-applied = Applied
auto-supports-count = Supports
auto-branches-count = Branches and merged trunks
auto-braces-count = Braces
auto-islands-count = Islands held
auto-of = { $part } of { $whole }
auto-overhangs-count = Overhangs covered
auto-overhangs-covered = { $percent } %
auto-overhangs-not-covered = Not covered (islands only)
auto-check = Closing check
auto-check-passes = { $passes ->
    [one] One pass
   *[other] { $passes } passes
}, { $added } supports added
auto-took = Planned in
auto-seconds = { $seconds } s
auto-left-no-surface = No underside was found there
auto-left-out-of-reach = No way down reaches the plate, and feet on the model are off
auto-left-no-way = No way down clear of the model was found
auto-left-unbuildable = Its support cannot be built there with the preset
auto-left = Places left unsupported
auto-left-none = None
auto-left-count = { $count }: see the list (L)
auto-left-hint = The list window shows each place and why it was left; a double click brings the view to it
auto-list-title = Places left unsupported ({ $count })
auto-list-fact = Place
auto-list-state = Why
auto-list-islands = Islands ({ $count })
auto-list-island = Island · { $height } mm
auto-list-overhang = Overhang · { $height } mm
auto-list-current = Left by the plan shown
auto-list-none = Nothing planned yet: Generate, below the panel
auto-left-too-low = Too close to the plate for the preset: raise the Lift and generate again, or use a lighter preset
auto-left-drain-hole = A drain hole cuts the material away there: nothing to hold
auto-left-foot-in-hole = Its support's foot would stand where a drain hole cuts the model away
auto-left-collides = Its support would come closer to the model than the clearance
auto-left-pierces = Its tip would reach through a thin part of the model
auto-apply = { $count ->
    [one] Apply one support
   *[other] Apply { $count } supports
}
auto-apply-hint = Replaces the automatic supports with the planned ones, braces too, as one undoable step; supports placed otherwise stay
auto-discard = Discard
auto-generate = Generate
auto-generate-hint = Plans every support of the selected model with the panel's values, and previews them
auto-generate-again = Generate again
auto-generate-again-hint = Plans again with the panel's values; Apply replaces only the automatic supports
auto-stop = Stop
auto-stop-hint = Stops planning; nothing changes
auto-waiting = Preparing the model's surface…
auto-stage-finding = Finding islands and overhangs…
auto-stage-placing = Placing the tips…
auto-stage-growing = Routing the supports and growing trees…
auto-stage-checking = Checking the coverage…
auto-stage-verifying = Building the supports to check them…
auto-stage-bracing = Planning braces…
auto-out-of-date = The plate changed: generate again
auto-state-none = No automatic supports planned
auto-state-planned = { $count ->
    [one] One support planned; everything is covered
   *[other] { $count } supports planned; everything is covered
}
auto-state-planned-left = { $count } supports planned; { $left ->
    [one] one place left unsupported
   *[other] { $left } places left unsupported
}
auto-state-applied = { $count ->
    [one] One automatic support
   *[other] { $count } automatic supports
}
auto-state-applied-left = { $count } automatic supports; { $left ->
    [one] one place left unsupported
   *[other] { $left } places left unsupported
}
auto-added = { $count } supports and { $braces } braces added, as one step (Undo takes them back). { $left ->
    [0] Everything is covered.
    [one] One place is left unsupported: see the panel.
   *[other] { $left } places are left unsupported: see the panel.
}
auto-failed = The supports could not be planned
auto-refused = The model cannot be analysed
auto-stopped = Planning stopped
settings-auto-supports = Automatic supports
settings-grid-supports = Supports on a grid
delete-automatic-supports = Delete the automatic supports
delete-automatic-supports-title = Delete the automatic supports?
delete-automatic-supports-text = { $count ->
    [one] The support the Automatic tool planned on the selected model will be deleted, with the braces on it.
   *[other] The { $count } supports the Automatic tool planned on the selected model will be deleted, with the braces on them.
} Supports placed otherwise stay. Undo brings them back.

## Branches (owner, 2026-10-30).
supports-placing-branch-tip = Click the model where the branch's tip goes; Escape stops
supports-placing-branch-tip-hint = Then click a support's shaft: the branch joins it instead of reaching the plate
supports-placing-branch = Click a support's shaft to join the branch to it; Escape takes the tip back
supports-placing-branch-hint = It joins as high as it can, its slanted part at most 45° from vertical and clear of the model
supports-branch-refused = This shaft has no place within 45° of the tip clear of the model
supports-branch-refused-hint = Try a nearer support, or one whose shaft reaches lower
supports-branch-trunk-values = A branch is no thicker than its trunk: it takes its trunk's values
islands-branch = Branches: join tips within
islands-branch-hint = Tips this close, seen from above, share one trunk: the others join it as branches, no steeper than 45°. Zero makes no branches
islands-per-trunk = Branches per trunk
islands-per-trunk-hint = The most branches one trunk takes
islands-merge = Trunks: merge within
islands-merge-hint = Trunks on the plate this close, seen from above, merge: the one nearest their middle stays on the plate, thicker below for what it carries, and the others lean into it no steeper than 45°, below their own branches. Zero makes no merging
islands-per-group = Trunks per merge
islands-per-group-hint = The most trunks that merge into one, the main trunk included
islands-levels = Merge levels
islands-levels-hint = How many times trunks merge, each time the main trunks left on the plate

## Trees in the Supports tool (owner, 2026-10-30).
action-merge-trunks = Merge trunks
action-merge-trunks-hint = Shows how the selected model's supports standing close together would merge into trees, with the Islands tool's merge values; Apply merges them, Discard leaves them
action-join-support = Join a support
action-join-support-hint = The selected support joins the shaft clicked next: it goes down to a bend below what hangs from it, then leans into that shaft no steeper than 45°
action-grid-supports = Grid
action-grid-supports-hint = Supports in rows on a flat underside: click one to grid it, then Apply
supports-merge-apply = { $count ->
    [one] Merge one trunk
   *[other] Merge { $count } trunks
}
supports-merge-discard = Discard
supports-merge-working = Working out the merge…
supports-merge-none = No trunks close enough to merge
supports-merge-none-hint = Supports standing on their own merge when closer than the Islands tool's merge distance, with room below what hangs from them
supports-merge-state = { $count } trunks would merge into { $mains }
supports-merge-hint = The supports drawn as selected change; Apply merges them as one step that Undo takes back
supports-merged = { $count ->
    [one] One trunk merged (Undo takes it back)
   *[other] { $count } trunks merged (Undo takes them back)
}
supports-joining = Click the shaft the selected support joins; Escape stops
supports-joining-hint = It keeps its shaft down to a bend below what hangs from it, then leans into that shaft no steeper than 45°, clear of the model
supports-join-refused = It cannot join that shaft: none within 45° below what hangs from it, clear of the model
supports-join-refused-hint = Try a nearer support; a support cannot join itself or one standing on it
supports-join-already = The selected support already stands on another

## Suction cups in the Drain holes tool (M6.4j, TD-114)
action-show-cups = Show the suction cups
action-show-cups-hint = Shows the suction cups found: resin a layer encloses while it is peeled from the film, capped towards the plate
settings-colour-suction-cup = Suction cups without an opening
settings-colour-suction-cup-narrow = Suction cups with openings too narrow
cups-section = Suction cups
cups-min-vent = Minimum vent
cups-min-vent-hint = An opening vents a cup only where a circle of this area fits through it; a pixel or a slit does not. Its default is set in Settings › Hollowing
cups-min-vent-settings-hint = An opening vents a suction cup only where a circle of this area fits through it
cups-show-from = Show cups from
cups-show-taller = and taller than
cups-filter-hint = Smaller cups stay found, counted and in the list's All
cups-found-label = Found
cups-found-out-of-date = { $count } (out of date)
cups-shown-label = Shown
cups-shown-value = { $shown } · { $hidden } smaller hidden
cups-shown-hint = The cups at least as wide and as tall as the filter (Advanced); the list shows them all
cups-trapping-label = Trapping resin
cups-trapping-none = None
cups-trapping-value = { $count ->
    [one] 1 closes over its resin
   *[other] { $count } close over their resin
}
cups-cup = Suction cup
cups-of = Suction cup { $number } of { $count }
cups-previous = Previous suction cup
cups-next = Next suction cup
cups-height-span = { $bottom }–{ $top } mm
cups-widest = Widest
cups-widest-hint = Its widest layer's area: what the film pulls on
cups-volume = Volume
cups-volume-mm3 = { $volume } mm³
cups-opening = Opening
cups-opening-none = None
cups-opening-small = Narrower than the minimum vent
cups-then = Then
cups-then-closes = Closes over its resin: it stays trapped
cups-then-open = Opens to the outside
cups-vent = Add a vent here
cups-vent-hint = A new hole under the chosen lowest point's cap, out towards the plate (sideways when the plate is right under it), with New holes' diameter; Apply holes cuts it. A vent relieves only what lies above its height: moved up, it leaves the part under it a cup
cups-vent-out-of-date = Find the suction cups again first: the plate changed
cups-no-vent = { $count ->
    [one] No way out for a vent under one lowest point: place a hole there by hand
   *[other] No way out for a vent under { $count } lowest points: place holes there by hand
}
cups-find = Find suction cups
cups-find-again = Find suction cups again
cups-find-hint = Slices the selected model as it will be exported, supports and raft included, and finds the resin its layers enclose as they are peeled
cups-stop-hint = Stops searching; the cups found before stay
cups-finding = Finding suction cups… { $percent } %
cups-found = { $count } suction cups; { $shown } shown, the rest under the filter
cups-found-none = { $count ->
    [0] No suction cup
    [one] 1 suction cup, under the filter
   *[other] { $count } suction cups, all under the filter
}
cups-failed = The suction cups could not be found
cups-refused = The model cannot be sliced to find its suction cups
cups-stopped = The search stopped
cups-out-of-date = The plate changed: find the suction cups again
cups-tab-holes = Holes
cups-tab-cups = Suction cups
cups-list-title = Suction cups ({ $count })
cups-list-fact = Layers and widest area
cups-list-shown = Shown ({ $count })
cups-list-all = All ({ $count })
cups-list-not-searched = Not searched yet: Find suction cups, in the panel
cups-list-current = Found on the plate as it is, widest first
cups-list-row = layers { $first }–{ $last } · { $area } mm²
cups-state-traps = traps resin
cups-state-none = no opening
cups-state-small = opening too small
cups-slice-not-searched = Not searched
cups-slice-none = { $current ->
    [1] None shown
   *[other] None shown (out of date)
}
cups-slice-found = { $current ->
    [1] { $count } shown
   *[other] { $count } shown (out of date)
}
cups-slice-hint = What the last Find suction cups in the Drain holes tool found; a cup never stops a slice
cups-vents-needed = Vents needed
cups-vents-needed-hint = One under each lowest point of the cup, where a part of it starts on its own and pulls on its resin until it meets the rest (smaller parts are left out by the filter)
cups-part-of = Lowest point { $number } of { $count }
cups-part-previous = Previous lowest point
cups-part-next = Next lowest point
cups-part-cap = This point
cups-part-cap-value = z { $z } mm · { $height } mm alone · { $area } mm²
cups-vent-each = Add a vent at each lowest point ({ $count })
cups-vent-each-hint = A new hole under each lowest point the cup needs a vent at, as Add a vent here makes them

## Tools' values and their defaults (owner, 2026-11-02)
reset-to-default = Back to the default: { $value }
value-on = On
value-off = Off
action-restore-defaults = Restore all default values
action-restore-defaults-hint = Every tool's values go back to the defaults set in Settings; what is applied to the models stays
# Hollowing blockers (owner, 2026-11-02).
blockers-section = Blockers
blockers-next = New blockers
blockers-next-hint = A click on the model while placing (P) puts a blocker with these values there, centred on the point clicked. Blockers keep the model solid where they stand; their defaults are set in Settings › Hollowing
blockers-box = Box
blockers-sphere = Sphere
blockers-bottom-width = Lower base width
blockers-bottom-depth = Lower base depth
blockers-top-width = Upper base width
blockers-top-depth = Upper base depth
blockers-height = Height
blockers-diameter = Diameter
blockers-base = Solid base up to
blockers-add-base = Add a solid base
blockers-add-base-hint = A box over the whole footprint, from the model's lowest point up to this height: a base kept solid, a blocker like any other afterwards
blockers-selected = Blocker { $number }
blockers-selected-hint = In the model's frame: it follows the model when it moves, turns or scales. Move (T) and turn (R) it with the gizmo too
blockers-centre = Centre { $axis }
blockers-turn = Turn about { $axis }
blockers-changed = { $count ->
    [one] 1 blocker new or changed: Apply hollowing to cut the cavity again
   *[other] { $count } blockers new or changed: Apply hollowing to cut the cavity again
}
blockers-list-title = Blockers ({ $count })
blockers-list-shape = Shape
blockers-list-changed = New or changed ({ $count })
blockers-list-summary = { $count } blockers, { $changed } not applied yet
blockers-name = Blocker { $number }
blockers-applied = Applied
blockers-new = Not applied yet
blockers-row-hint = Click to select it; double-click to bring it into view
blockers-remove = Delete this blocker
blockers-sphere-fact = Sphere Ø { $diameter }
blockers-box-fact = { $bottom } to { $top } · { $height }
settings-colour-blocker = Applied
settings-colour-blocker-new = Not applied yet
settings-colour-blocker-selected = Selected

## The Orient tool (M6.4k, TD-119)
orient-constraints = Constraints
orient-axes = Tilt about
orient-axes-hint = Which plate axes the model may tilt about; its turn about the vertical axis is always kept
orient-axes-any = Any
orient-axes-x = X only
orient-axes-y = Y only
orient-limit = At most from now
orient-limit-hint = The search stays within this angle of the pose now: orient the model roughly by hand, and let it refine
orient-limit-angle = Most tilt
orient-keep-away = Keep supports away within
orient-keep-away-hint = How far round each protected point supports are avoided
orient-points = Protected points
orient-points-hint = Points picked on the model with Protect (P): their surface must not face the plate, and supports near them cost a lot
orient-points-none = None. Protect (P) on the tool strip, then click the face, or any surface to keep clean of supports.
orient-point = Point { $number }
orient-point-remove = Remove this point
orient-priority = Priority
orient-priority-hint = What the score favours: it sets the weights
orient-priority-fewer = Fewer supports
orient-priority-balanced = Balanced
orient-priority-faster = Faster
orient-weights-note = How much each measure counts in choosing: each is compared among the candidates (the largest counts as 1) and multiplied by its weight. 2× counts twice as much as 1×; 0× ignores it.
orient-weight-facing = Surface facing the plate
orient-weight-facing-hint = Area steeper than the critical angle, facing the plate: where supports go
orient-weight-islands = Islands
orient-weight-islands-hint = Places the print starts in mid-air
orient-weight-cups = Suction cups
orient-weight-cups-hint = Hollowed models: cavities closed towards the plate
orient-weight-height = Height
orient-weight-height-hint = More layers, a longer print
orient-weight-largest = Largest layer
orient-weight-largest-hint = The pull on the film at the widest layer
orient-weight-protected = Protected surfaces
orient-weight-protected-hint = Surface facing the plate and islands near the protected points
orient-orientations = Orientations
orient-none-yet = Find orientations proposes the best five the constraints allow, each with its facts.
orient-none-fits = No orientation fits the constraints: allow more tilt, another axis, or fewer protected points.
orient-out-of-date = The model, the constraints or the points changed: find the orientations again.
orient-tilt-none = As it is now
orient-tilt-x = Tilt { $x }° about X
orient-tilt-y = Tilt { $y }° about Y
orient-tilt-xy = Tilt { $x }° about X, { $y }° about Y
orient-fact-facing = Facing plate
orient-fact-islands = Islands
orient-fact-cups = Cups
orient-fact-height = Height
orient-fact-largest = Largest layer
orient-fact-protected = Protected
orient-protected-clear = clear of supports
orient-protected-near = supports near
orient-tag-fewest-supports = Fewest supports
orient-tag-fewest-islands = Fewest islands
orient-tag-fewest-cups = Fewest cups
orient-tag-lowest = Lowest
orient-warn-supports = Its supports are removed when it is turned (Apply asks first): they would stand in the air.
orient-warn-hollowed = The cavity turns with it; check the drain holes and suction cups afterwards.
orient-find = Find orientations
orient-find-hint = Score the tilts the constraints allow and propose the best five
orient-stop = Stop
orient-apply = Apply
orient-apply-hint = Turn the model to the chosen orientation, as one step
orient-discard = Discard
orient-discard-hint = Drop the orientations found and show the model as it is (Escape)
orient-stop-hint = Stop the search; the model stays as it is
orient-stage-rough = Scoring the tilts from the model's surface…
orient-stage-checking = Checking orientation { $number } of { $count } as a slice would…
orient-protecting = Click the model to protect a point; Escape stops
orient-state-idle = Set the constraints, then Find orientations
orient-state-found = { $count } orientations in { $seconds } s: hover to preview, Apply the chosen one
orient-found = Scored { $scored } tilts; { $count } orientations proposed in { $seconds } s.
orient-failed = The orientations could not be found
orient-stopped = The search was stopped
orient-refused = The model cannot be sliced for the search
orient-preparing = The hollowed model is still being prepared: find the orientations once it is ready.
orient-turned = Turned: { $tilt }. It rests on the plate again; Undo turns it back.
orient-turned-supports = Turned: { $tilt }, and its supports removed. Undo brings both back.
orient-confirm-title = Turn the model?
orient-confirm-text = The model has { $count } supports, placed for the pose it has now: turned, they would stand in the air, so they are removed with it. Undo brings the model and its supports back together.
orient-confirm-action = Turn and remove the supports

## Supports on a grid (the Supports tool's Grid mode, TD-138)
grid-face = Face
grid-face-area = Area
grid-face-tilt = Tilted from level
grid-supports = Supports
grid-preset-hint = What the grid's supports are made with: one of the Manual tool's presets. Its default is set in Settings › Supports
grid-spacing = Spacing along the rows
grid-spacing-hint = How far apart the tips stand along a row
grid-across = Spacing between rows
grid-across-hint = How far apart the rows stand
grid-along = Along the longest edge
grid-along-hint = The rows run along the face's longest edge; off, at the angle typed
grid-angle = Angle
grid-angle-hint = The rows' angle from the face's own X, in its plane
grid-edges = Tips along the edges
grid-edges-hint = Each edge and corner of the face takes its own tips the inset in, as the Automatic tool's do; a tilted face's lowest edge takes them close and on the face beside it
grid-inset = Edge inset
grid-inset-hint = How far in from the edges and the corners their tips stand
grid-flat = Flat within
grid-flat-hint = The triangles joined to the one clicked that turn from it by less than this make the face
grid-branches = Branches
grid-branches-hint = Tips close together share a trunk, the others joining it as branches; off, every support goes straight down to the plate
grid-planned = Planned
grid-fact-supports = Supports
grid-fact-corners = Corners
grid-fact-edges = Along the edges
grid-fact-sides = Beside the lowest edge
grid-fact-grid = On the grid
grid-fact-plate = Standing on their own
grid-fact-left = Left unsupported
grid-fact-time = Planned in
grid-apply = Apply { $count } supports
grid-discard = Discard
grid-planning = Planning the grid…
grid-out-of-date = The plate changed: the grid is planned again
grid-state-pick = Click a flat underside of the model to grid it
grid-state-pick-rectangle = Drag a rectangle over the model's underside to grid it
grid-state-pick-lasso = Draw around the model's underside to grid it
grid-pick-face = Click a flat face
grid-pick-rectangle = Rectangle
grid-pick-lasso = Lasso
grid-no-selection = No underside facing down lies in the shape drawn
grid-stale = The model's surface changed: select the underside again
grid-state-planned = { $count } supports on the grid
grid-state-planned-left = { $count } supports on the grid; { $left } places left unsupported
grid-facing-up = That face faces up: click an underside
grid-failed = The face could not be gridded
grid-none = No support could be made under the face
grid-too-low = The face is too close to the plate for the preset's supports: lift the model (the Raft tool's Lift) and click it again
grid-stopped = The grid's planning stopped
grid-no-surface = The model's surface is not ready
grid-no-face = No underside was found where you clicked
grid-too-small = The face is too small for a tip at these values
grid-added = { $count } supports added on the grid
grid-added-left = { $count } supports added on the grid; { $left } places left unsupported

# Names screen readers announce for controls whose visible label is drawn apart (TD-142).
a11y-automatic = { $setting }: automatic
a11y-section-top = { $what }, top of the section
a11y-section-bottom = { $what }, bottom of the section
a11y-viewport = 3D view
a11y-plate = { $name }, { $state }
a11y-axis = { $mode } { $axis }
a11y-log-shown = Messages shown
a11y-log-area = Area
a11y-pattern = { $name }: pattern
a11y-repair-preset = Repairs
a11y-colour = { $name }: colour
a11y-hex = { $name }: hexadecimal value
a11y-layer = Layer
a11y-resize = Resize
a11y-currency = Currency

# Status colours (TD-142).
settings-colours-status = Status colours
settings-colour-status-warning = Warnings
settings-colour-status-error = Errors
settings-colour-status-success = Successes
settings-colour-status-info-fill = Information notices
settings-colour-status-success-fill = Success notices
settings-colour-status-warning-fill = Warning notices
settings-colour-status-error-fill = Error notices
settings-status-contrast = Hard to read: { $colours }. Text needs 4.5:1 against its background.
settings-status-contrast-item = { $colour } ({ $ratio }:1)
messages-latest = { $level }: { $text }
messages-button-new-worst = Messages ({ $count } new, { $level })
supports-preview-error = A support here would have an error
supports-preview-error-hint = Outside the build volume, or its shaft cannot reach the plate. It can still be placed and changed.
holes-state-ok-notes = OK, with notes
settings-display = Display
