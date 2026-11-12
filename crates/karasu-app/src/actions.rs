//! What the user can do, by name (M6.5): one list of actions that buttons, menus and keyboard
//! shortcuts all invoke, so each action is defined, enabled and logged in one place.
//!
//! **Actions** are performed by the window ([`crate::window`]); this module names them, gives
//! their labels and hints, and maps keys to them.
//!
//! **Shortcuts** are data, a [`Keymap`], so the settings panel can let the user change them and
//! keep them across restarts. The defaults are the shortcuts the window has always had. A
//! shortcut fires only while no widget holds the keyboard focus (a text field or a value being
//! typed), and the more specific of two
//! shortcuts on one key wins (Ctrl+Shift+Z before Ctrl+Z), because egui's matching ignores
//! extra Shift and Alt.
//!
//! The plate's undo history stays snapshot-based ([`karasu_project::project`]): actions are not
//! commands that undo themselves.
use crate::{
    i18n::tr,
    manipulate::Mode,
    section_ui::SectionStep,
    workflow::{Step, Tool},
};
use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A shortcut as the settings file keeps it: the key by egui's name, and the modifiers.
/// `command` is Ctrl, or ⌘ on a Mac.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeySetting {
    /// The key, as [`Key::name`] writes it.
    pub key: String,
    /// Ctrl (⌘ on a Mac).
    #[serde(default)]
    pub command: bool,
    /// Shift.
    #[serde(default)]
    pub shift: bool,
    /// Alt (Option on a Mac).
    #[serde(default)]
    pub alt: bool,
}

impl KeySetting {
    /// `shortcut`, to keep.
    #[must_use]
    pub fn of(shortcut: KeyboardShortcut) -> Self {
        Self {
            key: shortcut.logical_key.name().to_owned(),
            command: shortcut.modifiers.command,
            shift: shortcut.modifiers.shift,
            alt: shortcut.modifiers.alt,
        }
    }

    /// The shortcut kept, if its key is known.
    #[must_use]
    pub fn shortcut(&self) -> Option<KeyboardShortcut> {
        let key = Key::from_name(&self.key)?;
        Some(shortcut(key, self.command, self.shift, self.alt))
    }
}

/// A shortcut of `key` with Ctrl (⌘ on a Mac), Shift and Alt as held: how shortcuts are
/// written, whatever modifiers the platform's events add.
#[must_use]
pub fn shortcut(key: Key, command: bool, shift: bool, alt: bool) -> KeyboardShortcut {
    let mut modifiers = Modifiers::NONE;
    if command {
        modifiers |= Modifiers::COMMAND;
    }
    if shift {
        modifiers |= Modifiers::SHIFT;
    }
    if alt {
        modifiers |= Modifiers::ALT;
    }
    KeyboardShortcut::new(modifiers, key)
}

/// An action the user can take.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Action {
    /// Start a new project, once unsaved changes are saved or discarded (TD-130).
    NewProject,
    /// Open models, profiles or a project with the file dialog.
    Open,
    /// Save the project where it was saved or opened, or else choose where (TD-130).
    Save,
    /// Save the project to a file chosen now.
    SaveAs,
    /// Export the models as STL, 3MF or OBJ, with their supports and drain holes (TD-139).
    ExportModels,
    /// Add a plate after the shown one, with its profiles and settings, and show it (TD-135).
    NewPlate,
    /// Add a copy of the shown plate, its models too, after it, and show it.
    DuplicatePlate,
    /// Name the shown plate.
    RenamePlate,
    /// Delete the shown plate (undoable); the project keeps one plate at least.
    DeletePlate,
    /// Show the plate before the shown one.
    PreviousPlate,
    /// Show the plate after the shown one.
    NextPlate,
    /// Show the plates bar's thumbnails, or fold it to a row of tabs.
    TogglePlatesBar,
    /// Open or close the overview of every plate (TD-135).
    PlatesOverview,
    /// Arrange the plate shown's models, those that do not fit to new plates or left in place
    /// (TD-135, phase 5).
    ArrangeModels,
    /// Slice, one after another, every plate whose slice is missing or out of date (TD-135,
    /// phase 4).
    SliceEveryPlate,
    /// Export every plate's slice to a folder, slicing first those that need it (TD-135,
    /// phase 4).
    ExportEveryPlate,
    /// Undo the project's newest step: the plate's own, or a change of the plates (TD-135).
    Undo,
    /// Redo the project's most recently undone step.
    Redo,
    /// Frame the plate's objects.
    FitView,
    /// Turn the camera back to the starting view.
    HomeView,
    /// The handles move the selected object.
    MoveMode,
    /// The handles turn it.
    RotateMode,
    /// The handles scale it.
    ScaleMode,
    /// Lower the selected object onto the plate.
    DropToPlate,
    /// Delete the selected drain hole, or else the selected object once confirmed.
    Delete,
    /// End what the drain holes are doing, or else clear the selection.
    Cancel,
    /// Hide the selected model from the scene (it is still printed).
    Hide,
    /// Show only the selected model; again, show what was shown before.
    Isolate,
    /// Show every model.
    ShowAll,
    /// Show the models' overhangs, or not (TD-107).
    ToggleOverhangs,
    /// Start or stop placing with the tool in use: supports, or drain holes (the tool strip,
    /// owner 2026-10-29).
    Place,
    /// Place on the model's outside or in its cavity, with the tool in use.
    ToggleSurface,
    /// New supports avoid the model below their tips, or go straight down.
    ToggleAvoid,
    /// A support's knee follows its foot, or stays.
    ToggleKnee,
    /// Show the selected hollowed model's cavity from inside, its outside as a ghost (owner,
    /// 2026-10-29).
    ToggleCavityView,
    /// Show or hide the tool in use's list window: its supports, or its drain holes (owner,
    /// 2026-10-29).
    ToggleList,
    /// Show only the supports' tips, or the whole supports (owner, 2026-10-29).
    ToggleTips,
    /// Place mini supports, or standard ones (owner, 2026-10-30).
    ToggleMini,
    /// Show every item of the tool in use, or only those with errors (its options: only the
    /// selected one), the others hidden (owner, 2026-10-31).
    Show,
    /// Show the braces the Braces tool's values would add, to apply or discard (owner,
    /// 2026-10-29).
    AutoBraces,
    /// Show the islands found, or not (the Islands tool, owner 2026-10-29).
    ToggleIslands,
    /// Show the selected model's local minima, or not.
    ToggleMinima,
    /// Show the suction cups found, or not (the Drain holes tool, M6.4j).
    ShowCups,
    /// Every tool's values back to the defaults set in Settings (owner, 2026-11-02).
    RestoreDefaults,
    /// Show the supports planned under the islands without one, to apply or discard (the
    /// Islands tool, owner 2026-10-29).
    SupportIslands,
    /// The Automatic tool: show the places its plan leaves unsupported (M6.4i).
    ToggleLeft,
    /// Merge the selected model's trunks standing close together, to apply or discard (the
    /// Supports tool, owner 2026-10-30).
    MergeTrunks,
    /// Join the selected support to the shaft clicked next.
    JoinSupport,
    /// Supports on a grid on a flat underside clicked (TD-138): the Supports tool's Grid mode.
    GridSupports,
    /// Move the section's active handle (owner R10, 2026-11-10).
    Section(SectionStep),
    /// Choose a step of the workflow (TD-098).
    Step(Step),
    /// Choose a tool, which shows its panel.
    Tool(Tool),
    /// Show or hide the workflow bar's tools.
    ToggleTools,
    /// Move and turn along the world's axes, or the model's own.
    ToggleAxes,
    /// Lock moving, turning or scaling, or unlock it.
    ToggleLock(Mode),
    /// Whether a typed scale scales the three axes alike.
    ToggleUniform,
    /// Centre the selected model on the plate.
    CenterModel,
    /// Scale the selected model uniformly to the largest that fits the build volume.
    FitToPlate,
    /// Turn the selected model back to no rotation.
    ResetRotation,
    /// Scale the selected model back to its size.
    ResetScale,
    /// Open or close the sidebar.
    ToggleSidebar,
    /// Open or close the Messages window.
    ToggleMessages,
    /// Open or close the live log window (TD-096).
    ToggleLog,
    /// Open the settings (TD-097).
    OpenSettings,
    /// Open the profile editor on the plate's resin, or its printer.
    EditProfiles,
    /// Show who made the program, its licence and where its source is (TD-145).
    About,
    /// Close the application.
    Quit,
}

impl Action {
    /// The action's name, as buttons and the settings show it.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::NewProject => tr!("action-new-project"),
            Self::Open => tr!("action-open"),
            Self::Save => tr!("action-save"),
            Self::SaveAs => tr!("action-save-as"),
            Self::ExportModels => tr!("action-export-models"),
            Self::NewPlate => tr!("action-new-plate"),
            Self::DuplicatePlate => tr!("action-duplicate-plate"),
            Self::RenamePlate => tr!("action-rename-plate"),
            Self::DeletePlate => tr!("action-delete-plate"),
            Self::PreviousPlate => tr!("action-previous-plate"),
            Self::NextPlate => tr!("action-next-plate"),
            Self::TogglePlatesBar => tr!("action-plates-bar"),
            Self::PlatesOverview => tr!("action-plates-overview"),
            Self::ArrangeModels => tr!("action-arrange-models"),
            Self::SliceEveryPlate => tr!("action-slice-every-plate"),
            Self::ExportEveryPlate => tr!("action-export-every-plate"),
            Self::Undo => tr!("action-undo"),
            Self::Redo => tr!("action-redo"),
            Self::FitView => tr!("action-fit-view"),
            Self::HomeView => tr!("viewcube-home"),
            Self::MoveMode => tr!("mode-move"),
            Self::RotateMode => tr!("mode-rotate"),
            Self::ScaleMode => tr!("mode-scale"),
            Self::DropToPlate => tr!("action-drop-to-plate"),
            Self::Delete => tr!("action-delete"),
            Self::Cancel => tr!("action-cancel-or-deselect"),
            Self::Hide => tr!("action-hide"),
            Self::Isolate => tr!("action-isolate"),
            Self::ShowAll => tr!("action-show-all"),
            Self::ToggleOverhangs => tr!("action-overhangs"),
            Self::Place => tr!("action-place"),
            Self::ToggleSurface => tr!("action-surface"),
            Self::ToggleAvoid => tr!("supports-avoid"),
            Self::ToggleKnee => tr!("supports-knee-follows"),
            Self::ToggleCavityView => tr!("action-cavity-view"),
            Self::ToggleList => tr!("action-list"),
            Self::ToggleTips => tr!("action-tips"),
            Self::ToggleMini => tr!("action-mini"),
            Self::Show => tr!("action-show-items"),
            Self::AutoBraces => tr!("action-auto-braces"),
            Self::ToggleIslands => tr!("action-islands"),
            Self::ToggleMinima => tr!("action-minima"),
            Self::ShowCups => tr!("action-show-cups"),
            Self::RestoreDefaults => tr!("action-restore-defaults"),
            Self::SupportIslands => tr!("action-support-islands"),
            Self::ToggleLeft => tr!("action-left"),
            Self::MergeTrunks => tr!("action-merge-trunks"),
            Self::JoinSupport => tr!("action-join-support"),
            Self::GridSupports => tr!("action-grid-supports"),
            Self::Section(step) => step.label(),
            Self::Step(step) => step.label(),
            Self::Tool(tool) => tool.label(),
            Self::ToggleTools => tr!("action-toggle-tools"),
            Self::ToggleAxes => tr!("action-toggle-axes"),
            Self::ToggleLock(Mode::Move) => tr!("action-lock-move"),
            Self::ToggleLock(Mode::Rotate) => tr!("action-lock-rotate"),
            Self::ToggleLock(Mode::Scale) => tr!("action-lock-scale"),
            Self::ToggleUniform => tr!("transform-uniform-on"),
            Self::CenterModel => tr!("action-center-model"),
            Self::FitToPlate => tr!("action-fit-to-plate"),
            Self::ResetRotation => tr!("transform-reset-rotation"),
            Self::ResetScale => tr!("transform-reset-scale"),
            Self::ToggleSidebar => tr!("action-toggle-sidebar"),
            Self::ToggleMessages => tr!("messages-title"),
            Self::ToggleLog => tr!("log-title"),
            Self::OpenSettings => tr!("action-settings"),
            Self::EditProfiles => tr!("action-edit-profiles"),
            Self::About => tr!("action-about"),
            Self::Quit => tr!("action-quit"),
        }
    }

    /// What the action does, when its name alone does not say.
    #[must_use]
    pub fn hint(self) -> Option<String> {
        match self {
            Self::NewPlate => Some(tr!("action-new-plate-hint")),
            Self::DeletePlate => Some(tr!("action-delete-plate-hint")),
            Self::TogglePlatesBar => Some(tr!("action-plates-bar-hint")),
            Self::PlatesOverview => Some(tr!("action-plates-overview-hint")),
            Self::ArrangeModels => Some(tr!("action-arrange-models-hint")),
            Self::SliceEveryPlate => Some(tr!("action-slice-every-plate-hint")),
            Self::ExportEveryPlate => Some(tr!("action-export-every-plate-hint")),
            Self::ExportModels => Some(tr!("action-export-models-hint")),
            Self::Undo => Some(tr!("action-undo-hint")),
            Self::Redo => Some(tr!("action-redo-hint")),
            Self::Hide | Self::Isolate | Self::ShowAll => Some(tr!("action-visibility-hint")),
            Self::ToggleOverhangs => Some(tr!("action-overhangs-hint")),
            Self::Place => Some(tr!("action-place-hint")),
            Self::ToggleSurface => Some(tr!("action-surface-hint")),
            Self::ToggleAvoid => Some(tr!("supports-avoid-hint")),
            Self::ToggleKnee => Some(tr!("supports-knee-follows-hint")),
            Self::ToggleCavityView => Some(tr!("action-cavity-view-hint")),
            Self::ToggleList => Some(tr!("action-list-hint")),
            Self::ToggleTips => Some(tr!("action-tips-hint")),
            Self::ToggleMini => Some(tr!("action-mini-hint")),
            Self::Show => Some(tr!("action-show-items-hint")),
            Self::AutoBraces => Some(tr!("action-auto-braces-hint")),
            Self::ToggleIslands => Some(tr!("action-islands-hint")),
            Self::ToggleMinima => Some(tr!("action-minima-hint")),
            Self::ShowCups => Some(tr!("action-show-cups-hint")),
            Self::RestoreDefaults => Some(tr!("action-restore-defaults-hint")),
            Self::SupportIslands => Some(tr!("action-support-islands-hint")),
            Self::ToggleLeft => Some(tr!("action-left-hint")),
            Self::MergeTrunks => Some(tr!("action-merge-trunks-hint")),
            Self::JoinSupport => Some(tr!("action-join-support-hint")),
            Self::GridSupports => Some(tr!("action-grid-supports-hint")),
            Self::Section(_) => Some(tr!("section-step-hint")),
            _ => None,
        }
    }
}

impl Action {
    /// Every action, in the order the settings list them.
    #[must_use]
    pub fn all() -> Vec<Self> {
        let mut all = vec![
            Self::NewProject,
            Self::Open,
            Self::Save,
            Self::SaveAs,
            Self::ExportModels,
            Self::OpenSettings,
            Self::Quit,
            Self::NewPlate,
            Self::DuplicatePlate,
            Self::RenamePlate,
            Self::DeletePlate,
            Self::PreviousPlate,
            Self::NextPlate,
            Self::TogglePlatesBar,
            Self::PlatesOverview,
            Self::ArrangeModels,
            Self::SliceEveryPlate,
            Self::ExportEveryPlate,
            Self::Undo,
            Self::Redo,
            Self::Delete,
            Self::Cancel,
            Self::MoveMode,
            Self::RotateMode,
            Self::ScaleMode,
            Self::DropToPlate,
            Self::CenterModel,
            Self::FitToPlate,
            Self::ResetRotation,
            Self::ResetScale,
            Self::ToggleAxes,
            Self::ToggleLock(Mode::Move),
            Self::ToggleLock(Mode::Rotate),
            Self::ToggleLock(Mode::Scale),
            Self::ToggleUniform,
            Self::FitView,
            Self::HomeView,
            Self::Hide,
            Self::Isolate,
            Self::ShowAll,
            Self::ToggleOverhangs,
            Self::Place,
            Self::ToggleSurface,
            Self::ToggleAvoid,
            Self::ToggleKnee,
            Self::ToggleCavityView,
            Self::ToggleList,
            Self::ToggleTips,
            Self::ToggleMini,
            Self::Show,
            Self::AutoBraces,
            Self::ToggleIslands,
            Self::ToggleMinima,
            Self::ShowCups,
            Self::RestoreDefaults,
            Self::SupportIslands,
            Self::ToggleLeft,
            Self::MergeTrunks,
            Self::JoinSupport,
            Self::GridSupports,
            Self::ToggleTools,
            Self::ToggleSidebar,
        ];
        all.extend(SectionStep::ALL.map(Self::Section));
        all.extend(Step::ALL.map(Self::Step));
        all.extend(Tool::ALL.map(Self::Tool));
        all.extend([Self::ToggleMessages, Self::ToggleLog, Self::EditProfiles]);
        all.push(Self::About);
        all
    }

    /// The action's stable identity, as the settings file names it: never translated, never
    /// changed once released.
    #[must_use]
    pub fn id(self) -> String {
        let fixed = match self {
            Self::NewProject => "new-project",
            Self::Open => "open",
            Self::Save => "save",
            Self::SaveAs => "save-as",
            Self::ExportModels => "export-models",
            Self::NewPlate => "new-plate",
            Self::DuplicatePlate => "duplicate-plate",
            Self::RenamePlate => "rename-plate",
            Self::DeletePlate => "delete-plate",
            Self::PreviousPlate => "previous-plate",
            Self::NextPlate => "next-plate",
            Self::TogglePlatesBar => "plates-bar",
            Self::PlatesOverview => "plates-overview",
            Self::ArrangeModels => "arrange-models",
            Self::SliceEveryPlate => "slice-every-plate",
            Self::ExportEveryPlate => "export-every-plate",
            Self::Undo => "undo",
            Self::Redo => "redo",
            Self::FitView => "fit-view",
            Self::HomeView => "home-view",
            Self::MoveMode => "move-mode",
            Self::RotateMode => "rotate-mode",
            Self::ScaleMode => "scale-mode",
            Self::DropToPlate => "drop-to-plate",
            Self::Delete => "delete",
            Self::Cancel => "cancel",
            Self::Hide => "hide",
            Self::Isolate => "isolate",
            Self::ShowAll => "show-all",
            Self::ToggleOverhangs => "toggle-overhangs",
            Self::Place => "place",
            Self::ToggleSurface => "toggle-surface",
            Self::ToggleAvoid => "toggle-avoid",
            Self::ToggleKnee => "toggle-knee",
            Self::ToggleCavityView => "toggle-cavity-view",
            Self::ToggleList => "toggle-list",
            Self::ToggleTips => "toggle-tips",
            Self::ToggleMini => "toggle-mini",
            Self::Show => "show",
            Self::AutoBraces => "auto-braces",
            Self::ToggleIslands => "toggle-islands",
            Self::ToggleMinima => "toggle-minima",
            Self::ShowCups => "show-cups",
            Self::RestoreDefaults => "restore-defaults",
            Self::SupportIslands => "support-islands",
            Self::ToggleLeft => "toggle-left",
            Self::MergeTrunks => "merge-trunks",
            Self::JoinSupport => "join-support",
            Self::GridSupports => "grid-supports",
            Self::ToggleTools => "toggle-tools",
            Self::ToggleAxes => "toggle-axes",
            Self::ToggleUniform => "toggle-uniform",
            Self::CenterModel => "center-model",
            Self::FitToPlate => "fit-to-plate",
            Self::ResetRotation => "reset-rotation",
            Self::ResetScale => "reset-scale",
            Self::ToggleSidebar => "toggle-sidebar",
            Self::ToggleMessages => "toggle-messages",
            Self::ToggleLog => "toggle-log",
            Self::OpenSettings => "settings",
            Self::EditProfiles => "edit-profiles",
            Self::About => "about",
            Self::Quit => "quit",
            Self::Section(step) => {
                return format!("section-{}", format!("{step:?}").to_lowercase())
            }
            Self::Step(step) => return format!("step-{}", format!("{step:?}").to_lowercase()),
            Self::Tool(tool) => return format!("tool-{}", format!("{tool:?}").to_lowercase()),
            Self::ToggleLock(mode) => {
                return format!("lock-{}", format!("{mode:?}").to_lowercase())
            }
        };
        fixed.to_owned()
    }

    /// The action of identity `id`, if any.
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        Self::all().into_iter().find(|action| action.id() == id)
    }
}

/// The keyboard shortcuts of the actions. An action may have several, or none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keymap {
    /// Most specific first: more modifiers before fewer, so a shortcut is never taken by a
    /// shorter one on the same key.
    bindings: Vec<(KeyboardShortcut, Action)>,
}

impl Default for Keymap {
    fn default() -> Self {
        let none = Modifiers::NONE;
        let command = Modifiers::COMMAND;
        Self::new([
            (KeyboardShortcut::new(command, Key::N), Action::NewProject),
            (KeyboardShortcut::new(none, Key::F2), Action::RenamePlate),
            (
                KeyboardShortcut::new(command, Key::PageUp),
                Action::PreviousPlate,
            ),
            (
                KeyboardShortcut::new(command, Key::PageDown),
                Action::NextPlate,
            ),
            (KeyboardShortcut::new(command, Key::O), Action::Open),
            // The section's active handle (owner R10, 2026-11-10): the keys its slider had.
            (
                KeyboardShortcut::new(none, Key::ArrowUp),
                Action::Section(SectionStep::LayerUp),
            ),
            (
                KeyboardShortcut::new(none, Key::ArrowDown),
                Action::Section(SectionStep::LayerDown),
            ),
            (
                KeyboardShortcut::new(none, Key::PageUp),
                Action::Section(SectionStep::PageUp),
            ),
            (
                KeyboardShortcut::new(none, Key::PageDown),
                Action::Section(SectionStep::PageDown),
            ),
            (
                KeyboardShortcut::new(none, Key::Home),
                Action::Section(SectionStep::Bottom),
            ),
            (
                KeyboardShortcut::new(none, Key::End),
                Action::Section(SectionStep::Top),
            ),
            (KeyboardShortcut::new(command, Key::S), Action::Save),
            (
                KeyboardShortcut::new(command | Modifiers::SHIFT, Key::S),
                Action::SaveAs,
            ),
            (KeyboardShortcut::new(command, Key::Z), Action::Undo),
            (
                KeyboardShortcut::new(command | Modifiers::SHIFT, Key::Z),
                Action::Redo,
            ),
            (KeyboardShortcut::new(command, Key::Y), Action::Redo),
            // T as other slicers and CAD programs (owner, 2026-11-10; M before).
            (KeyboardShortcut::new(none, Key::T), Action::MoveMode),
            (KeyboardShortcut::new(none, Key::R), Action::RotateMode),
            (KeyboardShortcut::new(none, Key::S), Action::ScaleMode),
            (KeyboardShortcut::new(none, Key::Delete), Action::Delete),
            (KeyboardShortcut::new(none, Key::Escape), Action::Cancel),
            (KeyboardShortcut::new(none, Key::H), Action::Hide),
            (
                KeyboardShortcut::new(Modifiers::SHIFT, Key::H),
                Action::Isolate,
            ),
            (
                KeyboardShortcut::new(Modifiers::ALT, Key::H),
                Action::ShowAll,
            ),
            (
                KeyboardShortcut::new(command, Key::Comma),
                Action::OpenSettings,
            ),
            (KeyboardShortcut::new(none, Key::O), Action::ToggleOverhangs),
            (KeyboardShortcut::new(none, Key::P), Action::Place),
            (KeyboardShortcut::new(none, Key::C), Action::ToggleSurface),
            (KeyboardShortcut::new(none, Key::A), Action::ToggleAvoid),
            (KeyboardShortcut::new(none, Key::K), Action::ToggleKnee),
            (
                KeyboardShortcut::new(none, Key::V),
                Action::ToggleCavityView,
            ),
            (KeyboardShortcut::new(none, Key::L), Action::ToggleList),
            // Y since Move took T (owner, 2026-11-10).
            (KeyboardShortcut::new(none, Key::Y), Action::ToggleTips),
            (KeyboardShortcut::new(none, Key::G), Action::AutoBraces),
            (KeyboardShortcut::new(none, Key::I), Action::ToggleIslands),
            (KeyboardShortcut::new(none, Key::U), Action::SupportIslands),
            (KeyboardShortcut::new(none, Key::J), Action::JoinSupport),
            (KeyboardShortcut::new(none, Key::B), Action::GridSupports),
            (KeyboardShortcut::new(none, Key::X), Action::ShowCups),
        ])
    }
}

/// How many modifiers a shortcut holds: its specificity.
fn modifier_count(modifiers: Modifiers) -> usize {
    [
        modifiers.alt,
        modifiers.ctrl || modifiers.command,
        modifiers.shift,
        modifiers.mac_cmd,
    ]
    .into_iter()
    .filter(|&held| held)
    .count()
}

impl Keymap {
    /// A keymap of `bindings`, ordered most specific first (ties keep their order).
    pub fn new(bindings: impl IntoIterator<Item = (KeyboardShortcut, Action)>) -> Self {
        let mut bindings: Vec<_> = bindings.into_iter().collect();
        bindings.sort_by_key(|(shortcut, _)| std::cmp::Reverse(modifier_count(shortcut.modifiers)));
        Self { bindings }
    }

    /// The shortcuts of `action`.
    pub fn shortcuts(&self, action: Action) -> impl Iterator<Item = KeyboardShortcut> + '_ {
        self.bindings
            .iter()
            .filter(move |(_, a)| *a == action)
            .map(|(shortcut, _)| *shortcut)
    }

    /// The actions `keep` accepts whose shortcuts were pressed, their keys consumed; the other
    /// actions' keys are left for the widgets.
    pub fn pressed_where(
        &self,
        input: &mut egui::InputState,
        keep: impl Fn(Action) -> bool,
    ) -> Vec<Action> {
        self.bindings
            .iter()
            .filter(|(shortcut, action)| keep(*action) && input.consume_shortcut(shortcut))
            .map(|(_, action)| *action)
            .collect()
    }

    /// The action `shortcut` is bound to, if any.
    #[must_use]
    pub fn owner(&self, shortcut: KeyboardShortcut) -> Option<Action> {
        self.bindings
            .iter()
            .find(|(s, _)| *s == shortcut)
            .map(|(_, action)| *action)
    }

    /// Bind `shortcut` to `action` alone, in place of its shortcuts. Another action that had it
    /// loses it, and is returned.
    pub fn set(&mut self, action: Action, shortcut: KeyboardShortcut) -> Option<Action> {
        let displaced = self.owner(shortcut).filter(|&owner| owner != action);
        self.replace(action, &[shortcut]);
        displaced
    }

    /// Give `action` its default shortcuts again, taken from any action that had them since.
    pub fn reset(&mut self, action: Action) {
        let defaults: Vec<_> = Self::default().shortcuts(action).collect();
        self.replace(action, &defaults);
    }

    /// `action`'s shortcuts become `shortcuts`, each taken from any other action.
    fn replace(&mut self, action: Action, shortcuts: &[KeyboardShortcut]) {
        let mut bindings: Vec<_> = std::mem::take(&mut self.bindings)
            .into_iter()
            .filter(|(s, a)| *a != action && !shortcuts.contains(s))
            .collect();
        bindings.extend(shortcuts.iter().map(|&s| (s, action)));
        *self = Self::new(bindings);
    }

    /// The actions whose shortcuts differ from the defaults, with their shortcuts (none: the
    /// user removed them), as the settings file keeps them.
    #[must_use]
    pub fn overrides(&self) -> BTreeMap<String, Vec<KeySetting>> {
        let defaults = Self::default();
        Action::all()
            .into_iter()
            .filter_map(|action| {
                let mut now: Vec<_> = self.shortcuts(action).collect();
                let mut before: Vec<_> = defaults.shortcuts(action).collect();
                let key = |s: &KeyboardShortcut| {
                    (
                        s.logical_key.name(),
                        s.modifiers.command,
                        s.modifiers.shift,
                        s.modifiers.alt,
                    )
                };
                now.sort_by_key(key);
                before.sort_by_key(key);
                (now != before).then(|| {
                    (
                        action.id(),
                        now.iter().map(|s| KeySetting::of(*s)).collect(),
                    )
                })
            })
            .collect()
    }

    /// The default keymap with `overrides` applied, and what could not be: an unknown action
    /// or key is left out and named.
    #[must_use]
    pub fn with_overrides(overrides: &BTreeMap<String, Vec<KeySetting>>) -> (Self, Vec<String>) {
        let mut keymap = Self::default();
        let mut problems = Vec::new();
        for (id, keys) in overrides {
            let Some(action) = Action::from_id(id) else {
                problems.push(format!("unknown action {id:?}"));
                continue;
            };
            let mut shortcuts = Vec::new();
            for key in keys {
                match key.shortcut() {
                    Some(shortcut) => shortcuts.push(shortcut),
                    None => problems.push(format!("unknown key {:?} for {id}", key.key)),
                }
            }
            keymap.replace(action, &shortcuts);
        }
        (keymap, problems)
    }

    /// `action`'s label and hint, with its first shortcut as `ctx` writes it (for a tooltip).
    #[must_use]
    pub fn tooltip(&self, ctx: &egui::Context, action: Action) -> String {
        let text = action.hint().unwrap_or_else(|| action.label());
        match self.shortcuts(action).next() {
            Some(shortcut) => format!("{text} ({})", ctx.format_shortcut(&shortcut)),
            None => text,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pressed(keymap: &Keymap, modifiers: Modifiers, key: Key) -> Vec<Action> {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            events: vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            }],
            ..Default::default()
        };
        let mut actions = Vec::new();
        let mut output = ctx.run_ui(input, |ui| {
            actions = ui.input_mut(|i| keymap.pressed_where(i, |_| true));
        });
        output.textures_delta.clear();
        actions
    }

    #[test]
    fn the_default_shortcuts_are_the_windows_and_never_overlap() {
        let keymap = Keymap::default();
        let command = Modifiers::COMMAND;
        assert_eq!(pressed(&keymap, command, Key::Z), [Action::Undo]);
        // Ctrl+Shift+Z is redo, not undo, although egui's matching ignores extra Shift.
        assert_eq!(
            pressed(&keymap, command | Modifiers::SHIFT, Key::Z),
            [Action::Redo]
        );
        assert_eq!(pressed(&keymap, command, Key::Y), [Action::Redo]);
        assert_eq!(
            pressed(&keymap, Modifiers::NONE, Key::T),
            [Action::MoveMode]
        );
        // Y shows only the tips, Ctrl+Y still redoes; M is free.
        assert_eq!(
            pressed(&keymap, Modifiers::NONE, Key::Y),
            [Action::ToggleTips]
        );
        assert_eq!(
            pressed(&keymap, Modifiers::NONE, Key::M),
            Vec::<Action>::new()
        );
        assert_eq!(
            pressed(&keymap, Modifiers::NONE, Key::Escape),
            [Action::Cancel]
        );
        assert_eq!(
            pressed(&keymap, Modifiers::NONE, Key::Delete),
            [Action::Delete]
        );
        // H hides; with Shift it isolates and with Alt it shows all, never also hiding.
        assert_eq!(pressed(&keymap, Modifiers::NONE, Key::H), [Action::Hide]);
        assert_eq!(
            pressed(&keymap, Modifiers::SHIFT, Key::H),
            [Action::Isolate]
        );
        assert_eq!(pressed(&keymap, Modifiers::ALT, Key::H), [Action::ShowAll]);
        assert_eq!(
            pressed(&keymap, Modifiers::NONE, Key::O),
            [Action::ToggleOverhangs]
        );
        // A bare key's shortcut does not fire with Ctrl held, and an unbound key does nothing.
        assert_eq!(pressed(&keymap, command, Key::T), Vec::<Action>::new());
        assert_eq!(
            pressed(&keymap, Modifiers::NONE, Key::Q),
            Vec::<Action>::new()
        );
        // No two bindings share a shortcut.
        let shortcuts: Vec<_> = keymap.bindings.iter().map(|(s, _)| *s).collect();
        for (i, a) in shortcuts.iter().enumerate() {
            assert!(!shortcuts[i + 1..].contains(a), "{a:?}");
        }
    }

    #[test]
    fn more_specific_shortcuts_are_checked_first_whatever_their_order() {
        // Given in the "wrong" order, Ctrl+Shift+Z is still not taken by Ctrl+Z.
        let keymap = Keymap::new([
            (
                KeyboardShortcut::new(Modifiers::COMMAND, Key::Z),
                Action::Undo,
            ),
            (
                KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z),
                Action::Redo,
            ),
        ]);
        assert_eq!(
            pressed(&keymap, Modifiers::COMMAND | Modifiers::SHIFT, Key::Z),
            [Action::Redo]
        );
        assert_eq!(keymap.shortcuts(Action::Redo).count(), 1);
    }

    #[test]
    fn shortcuts_change_move_between_actions_and_keep_only_what_differs() {
        let mut keymap = Keymap::default();
        assert!(keymap.overrides().is_empty());
        let ctrl_d = shortcut(Key::D, true, false, false);
        // A free shortcut: the action has it alone.
        assert_eq!(keymap.set(Action::FitView, ctrl_d), None);
        assert_eq!(
            keymap.shortcuts(Action::FitView).collect::<Vec<_>>(),
            [ctrl_d]
        );
        // Another action's: it moves, and that action is named.
        let h = shortcut(Key::H, false, false, false);
        assert_eq!(keymap.set(Action::Isolate, h), Some(Action::Hide));
        assert_eq!(keymap.owner(h), Some(Action::Isolate));
        assert_eq!(keymap.shortcuts(Action::Hide).count(), 0);
        // Kept as overrides: the changed actions only, the emptied one included.
        let overrides = keymap.overrides();
        assert_eq!(
            overrides.keys().map(String::as_str).collect::<Vec<_>>(),
            ["fit-view", "hide", "isolate"]
        );
        assert_eq!(overrides["hide"], []);
        let (read, problems) = Keymap::with_overrides(&overrides);
        assert_eq!(problems, Vec::<String>::new());
        assert_eq!(read, keymap);
        // Reset takes the default back from whoever has it.
        keymap.reset(Action::Hide);
        assert_eq!(keymap.owner(h), Some(Action::Hide));
        assert_eq!(keymap.shortcuts(Action::Isolate).count(), 0);
        // What cannot be read is left out and named.
        let mut odd = BTreeMap::new();
        odd.insert("no-such-action".to_owned(), Vec::new());
        odd.insert(
            "undo".to_owned(),
            vec![KeySetting {
                key: "NoSuchKey".into(),
                command: true,
                shift: false,
                alt: false,
            }],
        );
        let (read, problems) = Keymap::with_overrides(&odd);
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert_eq!(read.shortcuts(Action::Undo).count(), 0);
    }

    #[test]
    fn every_action_has_a_label_and_tooltips_name_the_shortcut() {
        let all = Action::all();
        for action in &all {
            assert!(!action.label().is_empty(), "{action:?}");
            // Its identity names it alone, and reads back.
            assert_eq!(Action::from_id(&action.id()), Some(*action), "{action:?}");
        }
        let mut ids: Vec<String> = all.iter().map(|a| a.id()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), all.len());
        // Every default shortcut is for a listed action.
        let keymap = Keymap::default();
        assert!(keymap.bindings.iter().all(|(_, a)| all.contains(a)));
        let ctx = egui::Context::default();
        let keymap = Keymap::default();
        let undo = keymap.tooltip(&ctx, Action::Undo);
        assert!(
            undo.starts_with(&tr!("action-undo-hint")) && undo.ends_with(')'),
            "{undo}"
        );
        // An action without a shortcut is its name alone; the project's have theirs (TD-130).
        assert_eq!(
            keymap.tooltip(&ctx, Action::EditProfiles),
            tr!("action-edit-profiles")
        );
        for action in [
            Action::NewProject,
            Action::Open,
            Action::Save,
            Action::SaveAs,
        ] {
            let tooltip = keymap.tooltip(&ctx, action);
            assert!(tooltip.ends_with(')'), "{tooltip}");
        }
    }
}
