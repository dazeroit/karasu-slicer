//! The user's settings, kept across restarts (TD-097, M6.5 u3).
//!
//! **What.** Whatever the user sets: the colours, the repair on import, the main window's mode,
//! size and place, the layout (the sidebar, the tool panel, the sections, the dialogs' sizes and
//! the tool windows' places), the tools' own settings, and the printer, resin and print setting
//! last used. The keyboard shortcuts join when the
//! settings panel lets the user change them: stored now, they would only freeze today's
//! defaults.
//!
//! **Where.** One versioned JSON file, `settings.json`, in the application's preferences root
//! (`ProfilePaths::config`). Every field has a default, so a file from an earlier version reads
//! with the settings it lacks at their defaults.
//!
//! **Migrations.** A file of an earlier schema is brought up to date as it is read, and saved at
//! the current one: schema 2 (2026-10-30) turns the islands' join distance from the old default,
//! 1 mm, to the new one, none (owner: each island its own support where it starts); a distance
//! the user set otherwise stays. Schema 3 (2026-11-02) gives the hollowing blockers' colours
//! still at their first defaults the new, more visible ones; colours the user chose stay.
//! Schema 4 (2026-11-06, TD-136) turns the Automatic tool's Light preset and its braces on, its
//! first defaults, to Medium and off, and the new braces' diameter still at 0.8 mm to 1.0 mm: a
//! Light preset or braces chosen before cannot be told from the defaults and are turned too,
//! while another preset or diameter the user chose stays.
//!
//! **Never silently replaced.** A file that cannot be read, or that a newer version wrote, is
//! reported and left as it is: the session runs on the defaults (or on what it could read) and
//! saves nothing over it. A save writes a temporary file beside it and renames it over, so an
//! interrupted save leaves the previous file whole.
use crate::{actions::KeySetting, appearance::Appearance};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// The settings file's name in the preferences root.
pub const FILE_NAME: &str = "settings.json";
/// The settings file's schema.
pub const SCHEMA_VERSION: u32 = 4;
/// The islands' join distance's default before schema 2, in mm.
const OLD_JOIN_MM: f64 = 1.0;
/// The blockers' colours before schema 3.
const OLD_BLOCKER: [u8; 4] = [0xB8, 0xB4, 0xC8, 0x48];
const OLD_BLOCKER_NEW: [u8; 4] = [0xF2, 0x9B, 0x2E, 0x48];
const OLD_BLOCKER_SELECTED: [u8; 4] = [0xF5, 0xF4, 0xF8, 0x60];
/// The new braces' diameter before schema 4, in mm.
const OLD_BRACE_DIAMETER_MM: f64 = 0.8;
/// The critical angle's default, in degrees from the plate's plane (owner, TD-107).
pub const DEFAULT_OVERHANG_ANGLE_DEG: f64 = 45.0;
/// The critical angles the user may set, in degrees: a surface level with the plate always
/// needs support, and a vertical one never.
pub const OVERHANG_ANGLE_RANGE: std::ops::RangeInclusive<f64> = 1.0..=89.0;

/// Every setting the user keeps.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Must not exceed [`SCHEMA_VERSION`] for the file to be saved over.
    pub schema_version: u32,
    /// The scene's colours and their switches.
    pub appearance: Appearance,
    /// The repairs every opened model gets.
    pub repair_on_import: ImportRepair,
    /// The main window.
    pub window: WindowSettings,
    /// The layout's open parts and the windows' sizes and places.
    pub layout: LayoutSettings,
    /// The tools' own settings.
    pub tools: ToolSettings,
    /// The printer, resin and print setting the plate last used.
    pub profiles: ProfileChoice,
    /// The keyboard shortcuts that differ from the defaults, by action identity (an empty
    /// list: the user removed them).
    pub shortcuts: BTreeMap<String, Vec<KeySetting>>,
    /// The interface's language, or `None` for the reference language.
    pub language: Option<String>,
    /// The project files: the recent ones and the recovery copy (TD-130).
    pub projects: ProjectSettings,
    /// Arrange's and Export every plate's choices (TD-135, phases 4 and 5).
    pub plates: PlateSettings,
    /// Export models' choices (TD-139).
    pub models: ModelExportSettings,
}

/// The pattern of Export models' file names in a folder when the user sets none: the plate's
/// name and the model's (TD-139).
pub const DEFAULT_MODEL_PATTERN: &str = "{plate} {model}";

/// Export models' choices, kept from one use to the next (TD-139).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[allow(clippy::struct_excessive_bools)] // Independent switches the user keeps.
pub struct ModelExportSettings {
    /// The file format's extension: `stl`, `3mf` or `obj`.
    pub format: String,
    /// One file per model rather than one per plate.
    pub per_model: bool,
    /// With the supports.
    pub supports: bool,
    /// With the rafts.
    pub raft: bool,
    /// In 3MF and OBJ, the model, its supports and its raft as objects of their own, rather
    /// than one object per model.
    pub separate_objects: bool,
    /// The folder several files went to last.
    pub folder: Option<PathBuf>,
    /// The file names' pattern in a folder: `{project}`, `{number}`, `{plate}` and `{model}`.
    pub pattern: String,
}

impl Default for ModelExportSettings {
    fn default() -> Self {
        Self {
            format: "stl".to_owned(),
            per_model: false,
            supports: true,
            raft: true,
            separate_objects: false,
            folder: None,
            pattern: DEFAULT_MODEL_PATTERN.to_owned(),
        }
    }
}

/// The pattern of Export every plate's file names when the user sets none: the project's name,
/// the plate's number and its name (TD-135).
pub const DEFAULT_EXPORT_PATTERN: &str = "{project} {number} {plate}";

/// Arrange's and Export every plate's choices, kept from one use to the next (TD-135). The
/// spacing Arrange offers is the plate's own, not kept here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlateSettings {
    /// Where Arrange puts the models that do not fit.
    pub arrange_overflow: crate::workspace::Overflow,
    /// The folder Export every plate wrote to last.
    pub export_folder: Option<PathBuf>,
    /// The file names' pattern: `{project}`, `{number}` and `{plate}` are replaced.
    pub export_pattern: String,
    /// Export every plate slices first the plates not sliced, or out of date.
    pub export_slice_first: bool,
}

impl Default for PlateSettings {
    fn default() -> Self {
        Self {
            arrange_overflow: crate::workspace::Overflow::default(),
            export_folder: None,
            export_pattern: DEFAULT_EXPORT_PATTERN.to_owned(),
            export_slice_first: true,
        }
    }
}

/// How many recent project files File › Open recent lists.
pub const RECENT_PROJECTS: usize = 10;
/// The recovery copy's interval when the user sets none, in minutes.
pub const DEFAULT_RECOVERY_MINUTES: u32 = 3;
/// The intervals the user may set, in minutes; 0 writes no recovery copy.
pub const RECOVERY_MINUTES: std::ops::RangeInclusive<u32> = 0..=60;

/// The project files (TD-130).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectSettings {
    /// The project files opened or saved last, newest first, at most [`RECENT_PROJECTS`].
    pub recent: Vec<PathBuf>,
    /// How often a project with unsaved changes is copied for recovery, in minutes; 0 never.
    pub recovery_minutes: u32,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            recent: Vec::new(),
            recovery_minutes: DEFAULT_RECOVERY_MINUTES,
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            appearance: Appearance::default(),
            repair_on_import: ImportRepair::Exact,
            window: WindowSettings::default(),
            layout: LayoutSettings::default(),
            tools: ToolSettings::default(),
            profiles: ProfileChoice::default(),
            shortcuts: BTreeMap::new(),
            language: None,
            projects: ProjectSettings::default(),
            plates: PlateSettings::default(),
            models: ModelExportSettings::default(),
        }
    }
}

/// The printer, resin and print setting the plate last used, by their identities in the user's
/// library: a new session starts with them while they are still there and fit together.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProfileChoice {
    /// The printer.
    pub printer: Option<String>,
    /// The resin.
    pub resin: Option<String>,
    /// The print setting.
    pub print_setting: Option<String>,
}

/// The repair preset applied to every opened model (TD-092 r8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportRepair {
    /// Diagnose only.
    Off,
    /// The exact repairs.
    Exact,
    /// The standard repairs.
    Standard,
    /// The aggressive repairs.
    Aggressive,
}

/// How the main window opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowState {
    /// A window of its own size and place.
    Windowed,
    /// Maximized.
    Maximized,
    /// Covering the monitor.
    Fullscreen,
}

/// How the main window opens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenAs {
    /// As it was closed.
    #[default]
    AsClosed,
    /// Maximized.
    Maximized,
    /// Covering the monitor.
    Fullscreen,
    /// A window of its kept size, centred.
    Windowed,
}

/// The main window: how it opens, its state as it closed, and its size and place as a window
/// (kept while maximized, so it restores to them).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowSettings {
    /// How it opens (the user's choice).
    pub open_as: OpenAs,
    /// Its state as it closed.
    pub state: WindowState,
    /// The inner size as a window, in points.
    pub size: Option<[f32; 2]>,
    /// The outer top-left corner as a window, in points on the desktop.
    pub position: Option<[f32; 2]>,
}

impl Default for WindowSettings {
    fn default() -> Self {
        Self {
            open_as: OpenAs::AsClosed,
            state: WindowState::Maximized,
            size: None,
            position: None,
        }
    }
}

/// A tool window's place and size, in points.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowPlace {
    /// The outer top-left corner.
    pub position: [f32; 2],
    /// The content's size.
    pub size: [f32; 2],
}

/// The layout's open parts and the windows' sizes and places.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[allow(clippy::struct_excessive_bools)] // Each a part of the layout, open or not.
pub struct LayoutSettings {
    /// The sidebar is open.
    pub sidebar_open: bool,
    /// The tool panel is expanded.
    pub panel_open: bool,
    /// The workflow bar shows the step's tools.
    pub tools_shown: bool,
    /// The plates bar shows the plates' thumbnails (else a row of tabs, TD-135).
    pub plates_bar_open: bool,
    /// Collapsible sections, open or not, by name.
    pub sections: BTreeMap<String, bool>,
    /// Resizable dialogs' sizes, by dialog.
    pub dialogs: BTreeMap<String, [f32; 2]>,
    /// Tool windows' places, by window.
    pub windows: BTreeMap<String, WindowPlace>,
}

impl Default for LayoutSettings {
    fn default() -> Self {
        Self {
            sidebar_open: true,
            panel_open: true,
            tools_shown: true,
            plates_bar_open: true,
            sections: BTreeMap::new(),
            dialogs: BTreeMap::new(),
            windows: BTreeMap::new(),
        }
    }
}

/// The tools' own settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[allow(clippy::struct_excessive_bools)] // A record of the tools' switches, each its own setting.
pub struct ToolSettings {
    /// Transform: along the model's own axes, not the world's.
    pub transform_local: bool,
    /// Transform: moving, turning and scaling locked.
    pub transform_locks: [bool; 3],
    /// Transform: a typed scale scales the three axes alike.
    pub transform_uniform: bool,
    /// Repair: the stages show.
    pub repair_advanced: bool,
    /// Hollow: the grid cell and the passes show.
    pub hollow_advanced: bool,
    /// Hollow: the wall a new hollowing starts from, in mm (a default, see [`ToolDefaults`]).
    pub hollow_wall_mm: f64,
    /// Hollow: the grid cell it starts from, in mm, or `None` for the automatic one (a default).
    pub hollow_cell_mm: Option<f64>,
    /// Drain holes: the selected hole's tilt shows.
    pub holes_advanced: bool,
    /// Drain holes: a new hole's diameter, in mm (a default).
    pub hole_diameter_mm: f64,
    /// Drain holes: a new hole's depth, in mm, or `None` for the object's default (a default).
    pub hole_depth_mm: Option<f64>,
    /// Drain holes: new holes start from the cavity's surface, not the outer one (a default).
    pub holes_from_cavity: bool,
    /// Drain holes: the smallest opening that vents a suction cup, in mm² (a default, TD-114).
    pub cups_min_vent_mm2: f64,
    /// Drain holes: the suction cups' markers show.
    pub cups_shown: bool,
    /// Drain holes: the suction cups shown are at least this wide, in mm², and this tall, in mm.
    pub cups_show_from_mm2: f64,
    pub cups_show_taller_mm: f64,
    /// Slice: the slicing options show.
    pub slice_advanced: bool,
    /// Slice: every slice verifies the printed walls (TD-095).
    pub verify_walls: bool,
    /// Layers: the section follows the layer shown.
    pub layers_follow: bool,
    /// The print setting editor shows speeds in mm/min, not mm/s.
    pub speeds_per_minute: bool,
    /// The models' overhangs are shown (TD-107).
    pub show_overhangs: bool,
    /// The critical angle, in degrees from the plate's plane (TD-107).
    pub overhang_angle_deg: f64,
    /// Supports: the Advanced settings show (TD-108).
    pub supports_advanced: bool,
    /// Supports: a knee follows its support's foot when the foot is moved (TD-108).
    pub knee_follows_foot: bool,
    /// Supports: new supports go clear of the model below their tips (TD-108).
    pub avoid_model: bool,
    /// Supports: a click places a new support's tip and a second its foot (TD-108).
    pub tip_then_foot: bool,
    /// Supports: how the Grid mode picks an underside (TD-138).
    pub grid_pick: crate::grid_ui::plan::PickMode,
    /// Supports: a click places a branch's tip and a second, on a shaft, the branch.
    pub place_branch: bool,
    /// Supports: new supports are mini supports (owner, 2026-10-30).
    pub place_mini: bool,
    /// Supports, braces and drain holes: which the viewport shows (the strip's Show, owner
    /// 2026-10-31).
    pub supports_shown: crate::shown::Shown,
    pub braces_shown: crate::shown::Shown,
    pub holes_shown: crate::shown::Shown,
    /// Raft: the Advanced settings show (TD-108).
    pub raft_advanced: bool,
    /// Raft: a new raft's thickness, in mm (a default).
    pub raft_thickness_mm: f64,
    /// Raft: a new raft's margin beyond the pads, in mm (a default).
    pub raft_margin_mm: f64,
    /// Raft: a new raft's side slope from the plate, in degrees (a default).
    pub raft_slope_deg: f64,
    /// Raft: a new raft's fillets' diameter at the raft, in mm (a default).
    pub raft_fillet_diameter_mm: f64,
    /// Raft: a new raft's fillets' height above it, in mm (a default).
    pub raft_fillet_height_mm: f64,
    /// Raft: new rafts lie one per group of feet closer than this, in mm, or one under every
    /// foot when `None` (a default).
    pub raft_join_mm: Option<f64>,
    /// Raft: a new raft's shape (TD-144, a default).
    pub raft_shape: karasu_core::supports::RaftShape,
    /// Raft: a new web's bars' width, height and longest, in mm (TD-144, defaults).
    pub raft_bar_width_mm: f64,
    pub raft_bar_height_mm: f64,
    pub raft_bar_longest_mm: f64,
    /// Supports: the list window is open (owner, 2026-10-29).
    pub supports_list_open: bool,
    /// Supports: only their tips show while the tool is in use (owner, 2026-10-29).
    pub supports_tips_only: bool,
    /// Drain holes: the list window is open (owner, 2026-10-29).
    pub holes_list_open: bool,
    /// Hollow: what new blockers take (a default, owner 2026-11-02).
    pub new_blocker: crate::hollowing_ui::blockers::NewBlocker,
    /// Hollow: the blockers' list window is open.
    pub blockers_list_open: bool,
    /// Islands: the Advanced settings show (owner, 2026-10-29).
    pub islands_advanced: bool,
    /// Islands: the islands' markers show.
    pub islands_shown: bool,
    /// Islands: the local minima's markers show.
    pub minima_shown: bool,
    /// Islands: the list window is open.
    pub islands_list_open: bool,
    /// Automatic: the Advanced settings show (M6.4i).
    pub auto_advanced: bool,
    /// Orient: the Advanced settings show (TD-119).
    pub orient_advanced: bool,
    /// Orient: the constraints and priority a search starts from (a default).
    pub orient: crate::orient_ui::OrientValues,
    /// Automatic: the places the plan leaves unsupported show as markers.
    pub auto_left_shown: bool,
    /// Automatic: the list window of the places left is open.
    pub auto_list_open: bool,
    /// Automatic: how the plan is made (a default).
    pub auto_supports: crate::auto_ui::plan::AutoSupports,
    /// Grid (TD-138): how the grid is made (a default).
    pub grid: crate::grid_ui::plan::GridValues,
    /// Grid: the user's preset its supports start from, by name (a default); `None` starts
    /// from the grid's weight.
    pub grid_preset: Option<String>,
    /// Automatic: the user's preset its supports start from, by name (a default); `None` starts
    /// from the plan's weight.
    pub auto_preset: Option<String>,
    /// Automatic: the user's preset the cavity's supports start from, by name, when the cavity
    /// takes its own (a default, TD-137); `None` starts from the cavity's weight.
    pub auto_cavity_preset: Option<String>,
    /// Islands: the weight the islands' supports start from (a default).
    pub island_weight: crate::supports_ui::Weight,
    /// Islands: the user's preset the islands' supports start from, by name (a default); `None`
    /// starts from `island_weight`.
    pub island_preset: Option<String>,
    /// Islands: islands closer than this share a support, in mm (a default).
    pub island_join_mm: f64,
    /// Islands: Support the islands leaves out islands smaller than this, in mm² (a default).
    pub island_skip_under_mm2: f64,
    /// Islands: a support with no way down to the plate stands on the model (a default).
    pub island_foot_on_model: bool,
    /// Islands: where a support finds no room or no way down, a mini support stands on the
    /// model below its tip (a default, owner 2026-10-30).
    pub island_minis: bool,
    /// Islands: tips closer than this share a trunk, in mm; zero, no branches (a default).
    pub island_branch_mm: f64,
    /// Islands: the branches a trunk takes at most (a default).
    pub island_per_trunk: u32,
    /// Islands: trunks closer than this merge, in mm; zero, no merging (a default).
    pub island_merge_mm: f64,
    /// Islands: the trunks a group merges at most (a default).
    pub island_per_group: u32,
    /// Islands: how many times trunks merge (a default).
    pub island_levels: u32,
    /// Islands: the contacts fit the surface down to this diameter (TD-116); `None`, off.
    pub island_contact_fit_min_mm: Option<f64>,
    /// Islands: the contacts end in a ball, not a cone.
    pub island_sphere_contact: bool,
    /// Braces: each click adds a brace from the shaft clicked before (owner, 2026-10-29).
    pub braces_chain: bool,
    /// Braces: a brace's second end takes its first end's height.
    pub braces_level: bool,
    /// Braces: the list window is open.
    pub braces_list_open: bool,
    /// Braces: a new brace is as thick as the thinner of its shafts, up to `brace_diameter_mm`
    /// (a default).
    pub brace_auto_diameter: bool,
    /// Braces: a new brace's diameter, or an automatic one's largest, in mm (a default).
    pub brace_diameter_mm: f64,
    /// Braces: automatic braces join supports this close in plan, in mm (a default).
    pub brace_distance_mm: f64,
    /// Braces: the lowest automatic braces' height above the plate or raft, in mm (a default).
    pub brace_lowest_mm: f64,
    /// Braces: the height between automatic braces, in mm (a default).
    pub brace_step_mm: f64,
    /// Braces: how automatic braces join a pair of supports (a default).
    pub brace_pattern: crate::supports_ui::braces::Pattern,
    /// Supports: the user's own presets, for every project (owner, 2026-10-27).
    pub support_presets: Vec<crate::supports_ui::SupportPreset>,
    /// Supports: the user's preset new supports start from, by name (a default); `None` starts
    /// from `support_weight`.
    pub support_preset: Option<String>,
    /// Supports: the weight new supports start from (a default). Values changed in the panel
    /// are for the model at hand (owner, 2026-10-27); the user's presets keep values.
    pub support_weight: crate::supports_ui::Weight,
    /// Supports: new supports' contacts fit the surface down to this diameter (a default,
    /// TD-116); `None`, off.
    pub support_contact_fit_min_mm: Option<f64>,
}

/// The values a tool's new operation starts from, set in the Settings dialog only (owner,
/// 2026-10-27): a tool opens with them, and what the user changes in its panel is for the
/// model at hand, never kept.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToolDefaults {
    /// Hollow: the wall, in mm.
    pub wall_mm: f64,
    /// Hollow: the grid cell, in mm, or `None` for the automatic one.
    pub cell_mm: Option<f64>,
    /// Drain holes: the diameter, in mm.
    pub hole_diameter_mm: f64,
    /// Drain holes: the depth, in mm, or `None` for the model's default.
    pub hole_depth_mm: Option<f64>,
    /// Drain holes: they start from the cavity's surface, not the outer one.
    pub holes_from_cavity: bool,
    /// Drain holes: the smallest opening that vents a suction cup, in mm².
    pub min_vent_mm2: f64,
    /// Hollow: what new blockers take.
    pub blocker: crate::hollowing_ui::blockers::NewBlocker,
    /// Supports: the weight new supports start from.
    pub support_weight: crate::supports_ui::Weight,
    /// Supports: new supports' contacts fit the surface down to this diameter; `None`, off.
    pub support_contact_fit_min_mm: Option<f64>,
    /// Raft: what a new raft is made with.
    pub raft: karasu_core::supports::RaftSpec,
    /// Braces: what new braces are made with.
    pub braces: crate::supports_ui::braces::NewBraces,
    /// Islands: how their supports are made (the user's preset by place is the window's).
    pub island_supports: crate::islands_ui::plan::IslandSupports,
    /// Automatic: how the plan is made (the user's preset by place is the window's).
    pub auto_supports: crate::auto_ui::plan::AutoSupports,
    /// Grid: how the grid is made (the user's preset by place is the window's).
    pub grid: crate::grid_ui::plan::GridValues,
    /// Orient: the constraints and priority a search starts from.
    pub orient: crate::orient_ui::OrientValues,
}

impl Default for ToolDefaults {
    fn default() -> Self {
        ToolSettings::default().defaults()
    }
}

impl ToolSettings {
    /// The tools' defaults these settings hold; a value a tool would refuse is the built-in one.
    #[must_use]
    pub fn defaults(&self) -> ToolDefaults {
        let positive = |v: f64| v.is_finite() && v > 0.0;
        ToolDefaults {
            wall_mm: if positive(self.hollow_wall_mm) {
                self.hollow_wall_mm
            } else {
                crate::hollowing_ui::DEFAULT_WALL_MM
            },
            cell_mm: self.hollow_cell_mm.filter(|c| positive(*c)),
            hole_diameter_mm: if positive(self.hole_diameter_mm) {
                self.hole_diameter_mm
            } else {
                crate::hollowing_ui::DEFAULT_HOLE_MM
            },
            hole_depth_mm: self.hole_depth_mm.filter(|d| positive(*d)),
            holes_from_cavity: self.holes_from_cavity,
            min_vent_mm2: if crate::hollowing_ui::cups::MIN_VENT_MM2
                .contains(&self.cups_min_vent_mm2)
            {
                self.cups_min_vent_mm2
            } else {
                crate::hollowing_ui::cups::DEFAULT_MIN_VENT_MM2
            },
            blocker: self.new_blocker.valid(),
            support_weight: self.support_weight,
            support_contact_fit_min_mm: self
                .support_contact_fit_min_mm
                .filter(|m| crate::islands_ui::plan::CONTACT_MIN_MM.contains(m)),
            raft: crate::raft_ui::valid_raft(karasu_core::supports::RaftSpec {
                thickness_mm: self.raft_thickness_mm,
                margin_mm: self.raft_margin_mm,
                slope_deg: self.raft_slope_deg,
                fillet_diameter_mm: self.raft_fillet_diameter_mm,
                fillet_height_mm: self.raft_fillet_height_mm,
                layout: self
                    .raft_join_mm
                    .map_or(karasu_core::supports::RaftLayout::Single, |join_mm| {
                        karasu_core::supports::RaftLayout::Groups { join_mm }
                    }),
                shape: self.raft_shape,
                bars: karasu_core::supports::RaftBars {
                    width_mm: self.raft_bar_width_mm,
                    height_mm: self.raft_bar_height_mm,
                    longest_mm: self.raft_bar_longest_mm,
                },
            }),
            island_supports: crate::islands_ui::plan::IslandSupports {
                weight: self.island_weight,
                mine: None,
                join_mm: self.island_join_mm,
                skip_under_mm2: self.island_skip_under_mm2,
                foot_on_model: self.island_foot_on_model,
                minis: self.island_minis,
                branch_mm: self.island_branch_mm,
                per_trunk: self.island_per_trunk,
                merge_mm: self.island_merge_mm,
                per_group: self.island_per_group,
                levels: self.island_levels,
                contact_fit_min_mm: self.island_contact_fit_min_mm,
                sphere_contact: self.island_sphere_contact,
            }
            .valid(),
            auto_supports: crate::auto_ui::plan::AutoSupports {
                mine: None,
                cavity_mine: None,
                ..self.auto_supports
            }
            .valid(),
            grid: crate::grid_ui::plan::GridValues {
                mine: None,
                ..self.grid
            }
            .valid(),
            orient: self.orient.valid(),
            braces: crate::supports_ui::braces::NewBraces {
                auto_diameter: self.brace_auto_diameter,
                diameter_mm: self.brace_diameter_mm,
                max_distance_mm: self.brace_distance_mm,
                lowest_mm: self.brace_lowest_mm,
                step_mm: self.brace_step_mm,
                pattern: self.brace_pattern,
            }
            .valid(),
        }
    }

    /// Keep `defaults` in these settings.
    pub fn set_defaults(&mut self, defaults: &ToolDefaults) {
        self.hollow_wall_mm = defaults.wall_mm;
        self.hollow_cell_mm = defaults.cell_mm;
        self.hole_diameter_mm = defaults.hole_diameter_mm;
        self.hole_depth_mm = defaults.hole_depth_mm;
        self.holes_from_cavity = defaults.holes_from_cavity;
        self.cups_min_vent_mm2 = defaults.min_vent_mm2;
        self.new_blocker = defaults.blocker;
        self.support_weight = defaults.support_weight;
        self.support_contact_fit_min_mm = defaults.support_contact_fit_min_mm;
        self.raft_thickness_mm = defaults.raft.thickness_mm;
        self.raft_margin_mm = defaults.raft.margin_mm;
        self.raft_slope_deg = defaults.raft.slope_deg;
        self.raft_fillet_diameter_mm = defaults.raft.fillet_diameter_mm;
        self.raft_fillet_height_mm = defaults.raft.fillet_height_mm;
        self.raft_join_mm = match defaults.raft.layout {
            karasu_core::supports::RaftLayout::Single => None,
            karasu_core::supports::RaftLayout::Groups { join_mm } => Some(join_mm),
        };
        self.raft_shape = defaults.raft.shape;
        self.raft_bar_width_mm = defaults.raft.bars.width_mm;
        self.raft_bar_height_mm = defaults.raft.bars.height_mm;
        self.raft_bar_longest_mm = defaults.raft.bars.longest_mm;
        self.island_weight = defaults.island_supports.weight;
        self.island_join_mm = defaults.island_supports.join_mm;
        self.island_skip_under_mm2 = defaults.island_supports.skip_under_mm2;
        self.island_foot_on_model = defaults.island_supports.foot_on_model;
        self.island_minis = defaults.island_supports.minis;
        self.island_branch_mm = defaults.island_supports.branch_mm;
        self.island_per_trunk = defaults.island_supports.per_trunk;
        self.island_merge_mm = defaults.island_supports.merge_mm;
        self.island_per_group = defaults.island_supports.per_group;
        self.island_levels = defaults.island_supports.levels;
        self.island_contact_fit_min_mm = defaults.island_supports.contact_fit_min_mm;
        self.island_sphere_contact = defaults.island_supports.sphere_contact;
        self.auto_supports = crate::auto_ui::plan::AutoSupports {
            mine: None,
            cavity_mine: None,
            ..defaults.auto_supports
        };
        self.grid = crate::grid_ui::plan::GridValues {
            mine: None,
            ..defaults.grid
        };
        self.orient = defaults.orient;
        self.brace_auto_diameter = defaults.braces.auto_diameter;
        self.brace_diameter_mm = defaults.braces.diameter_mm;
        self.brace_distance_mm = defaults.braces.max_distance_mm;
        self.brace_lowest_mm = defaults.braces.lowest_mm;
        self.brace_step_mm = defaults.braces.step_mm;
        self.brace_pattern = defaults.braces.pattern;
    }
}

impl Default for ToolSettings {
    fn default() -> Self {
        Self {
            transform_local: false,
            transform_locks: [false; 3],
            transform_uniform: true,
            repair_advanced: false,
            hollow_advanced: false,
            hollow_wall_mm: crate::hollowing_ui::DEFAULT_WALL_MM,
            hollow_cell_mm: None,
            holes_advanced: false,
            hole_diameter_mm: crate::hollowing_ui::DEFAULT_HOLE_MM,
            hole_depth_mm: None,
            holes_from_cavity: false,
            cups_min_vent_mm2: crate::hollowing_ui::cups::DEFAULT_MIN_VENT_MM2,
            cups_shown: true,
            cups_show_from_mm2: crate::hollowing_ui::cups::DEFAULT_SHOW_FROM_MM2,
            cups_show_taller_mm: crate::hollowing_ui::cups::DEFAULT_SHOW_TALLER_MM,
            slice_advanced: false,
            verify_walls: false,
            layers_follow: true,
            speeds_per_minute: false,
            show_overhangs: true,
            overhang_angle_deg: DEFAULT_OVERHANG_ANGLE_DEG,
            supports_advanced: false,
            knee_follows_foot: true,
            avoid_model: true,
            tip_then_foot: false,
            grid_pick: crate::grid_ui::plan::PickMode::Face,
            place_branch: false,
            place_mini: false,
            supports_shown: crate::shown::Shown::All,
            braces_shown: crate::shown::Shown::All,
            holes_shown: crate::shown::Shown::All,
            raft_advanced: false,
            raft_thickness_mm: crate::raft_ui::DEFAULT_RAFT.thickness_mm,
            raft_margin_mm: crate::raft_ui::DEFAULT_RAFT.margin_mm,
            raft_slope_deg: crate::raft_ui::DEFAULT_RAFT.slope_deg,
            raft_fillet_diameter_mm: crate::raft_ui::DEFAULT_RAFT.fillet_diameter_mm,
            raft_fillet_height_mm: crate::raft_ui::DEFAULT_RAFT.fillet_height_mm,
            raft_join_mm: None,
            raft_shape: crate::raft_ui::DEFAULT_RAFT.shape,
            raft_bar_width_mm: crate::raft_ui::DEFAULT_RAFT.bars.width_mm,
            raft_bar_height_mm: crate::raft_ui::DEFAULT_RAFT.bars.height_mm,
            raft_bar_longest_mm: crate::raft_ui::DEFAULT_RAFT.bars.longest_mm,
            supports_list_open: true,
            supports_tips_only: false,
            holes_list_open: true,
            new_blocker: crate::hollowing_ui::blockers::NewBlocker::default(),
            blockers_list_open: true,
            islands_advanced: false,
            islands_shown: true,
            minima_shown: false,
            islands_list_open: true,
            auto_advanced: false,
            orient_advanced: false,
            orient: crate::orient_ui::DEFAULT_ORIENT,
            auto_left_shown: true,
            auto_list_open: true,
            auto_supports: crate::auto_ui::plan::DEFAULT_AUTO_SUPPORTS,
            auto_preset: None,
            auto_cavity_preset: None,
            grid: crate::grid_ui::plan::DEFAULT_GRID,
            grid_preset: None,
            island_weight: crate::islands_ui::plan::DEFAULT_ISLAND_SUPPORTS.weight,
            island_preset: None,
            island_join_mm: crate::islands_ui::plan::DEFAULT_ISLAND_SUPPORTS.join_mm,
            island_skip_under_mm2: crate::islands_ui::plan::DEFAULT_ISLAND_SUPPORTS.skip_under_mm2,
            island_foot_on_model: crate::islands_ui::plan::DEFAULT_ISLAND_SUPPORTS.foot_on_model,
            island_minis: crate::islands_ui::plan::DEFAULT_ISLAND_SUPPORTS.minis,
            island_branch_mm: crate::islands_ui::plan::DEFAULT_ISLAND_SUPPORTS.branch_mm,
            island_per_trunk: crate::islands_ui::plan::DEFAULT_ISLAND_SUPPORTS.per_trunk,
            island_merge_mm: crate::islands_ui::plan::DEFAULT_ISLAND_SUPPORTS.merge_mm,
            island_per_group: crate::islands_ui::plan::DEFAULT_ISLAND_SUPPORTS.per_group,
            island_levels: crate::islands_ui::plan::DEFAULT_ISLAND_SUPPORTS.levels,
            island_contact_fit_min_mm: None,
            island_sphere_contact: false,
            braces_chain: false,
            braces_level: false,
            braces_list_open: true,
            brace_auto_diameter: crate::supports_ui::braces::DEFAULT_BRACES.auto_diameter,
            brace_diameter_mm: crate::supports_ui::braces::DEFAULT_BRACES.diameter_mm,
            brace_distance_mm: crate::supports_ui::braces::DEFAULT_BRACES.max_distance_mm,
            brace_lowest_mm: crate::supports_ui::braces::DEFAULT_BRACES.lowest_mm,
            brace_step_mm: crate::supports_ui::braces::DEFAULT_BRACES.step_mm,
            brace_pattern: crate::supports_ui::braces::DEFAULT_BRACES.pattern,
            support_presets: Vec::new(),
            support_preset: None,
            support_weight: crate::supports_ui::Weight::Medium,
            support_contact_fit_min_mm: None,
        }
    }
}

/// What opening the settings file found.
#[derive(Clone, Debug, PartialEq)]
pub enum Opened {
    /// No file yet: the defaults, saved at the first change.
    Missing,
    /// The file, read.
    Read,
    /// The file cannot be read (the reason): the defaults, and nothing saved over it.
    Unreadable(String),
    /// A newer version wrote it (its schema): what could be read, and nothing saved over it.
    Newer(u32),
}

/// The settings file and what was last saved to it.
#[derive(Debug)]
pub struct Store {
    path: PathBuf,
    /// What opening it found.
    opened: Opened,
    /// Saving is allowed: the file was missing or read.
    writable: bool,
    /// The settings as last read or saved, to save only changes.
    saved: Settings,
}

impl Store {
    /// Open the settings file at `path`: the settings to start from, and what was found.
    #[must_use]
    pub fn open(path: PathBuf) -> (Self, Settings, Opened) {
        let (settings, opened) = match std::fs::read(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                (Settings::default(), Opened::Missing)
            }
            Err(error) => (Settings::default(), Opened::Unreadable(error.to_string())),
            Ok(bytes) => read(&bytes),
        };
        let writable = matches!(opened, Opened::Missing | Opened::Read);
        let store = Self {
            path,
            opened: opened.clone(),
            writable,
            saved: settings.clone(),
        };
        (store, settings, opened)
    }

    /// The file's path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// What opening the file found: whether this session saves to it.
    #[must_use]
    pub fn opened(&self) -> &Opened {
        &self.opened
    }

    /// Start over: a file this session would not save over (unreadable, or a newer version's)
    /// is kept beside, renamed with `.unreadable` or `.newer` (and a number if that is taken),
    /// and the defaults are saved from now on. Returns where the old file went, if anywhere.
    ///
    /// # Errors
    /// The old file cannot be renamed; nothing changes.
    pub fn start_over(&mut self) -> Result<Option<PathBuf>, String> {
        let suffix = match self.opened {
            Opened::Unreadable(_) => Some("unreadable"),
            Opened::Newer(_) => Some("newer"),
            Opened::Missing | Opened::Read => None,
        };
        let mut kept = None;
        if let Some(suffix) = suffix.filter(|_| self.path.exists()) {
            let free = (1..=999)
                .map(|n: u32| {
                    let name = if n == 1 {
                        format!("{FILE_NAME}.{suffix}")
                    } else {
                        format!("{FILE_NAME}.{suffix}-{n}")
                    };
                    self.path.with_file_name(name)
                })
                .find(|candidate| !candidate.exists())
                .unwrap_or_else(|| self.path.with_extension("json.old"));
            std::fs::rename(&self.path, &free)
                .map_err(|e| format!("cannot keep {} aside: {e}", self.path.display()))?;
            kept = Some(free);
        }
        self.opened = Opened::Missing;
        self.writable = true;
        // Saved again from now on, even if the defaults are what was last read.
        self.saved = Settings {
            schema_version: 0,
            ..Settings::default()
        };
        Ok(kept)
    }

    /// Whether `settings` differ from what was last saved and can be saved.
    #[must_use]
    pub fn changed(&self, settings: &Settings) -> bool {
        self.writable && *settings != self.saved
    }

    /// Save `settings` if they changed: written beside the file, then renamed over it.
    ///
    /// # Errors
    /// The file or its folder cannot be written; what was saved before stays.
    pub fn save(&mut self, settings: &Settings) -> Result<(), String> {
        if !self.changed(settings) {
            return Ok(());
        }
        let mut text = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
        text.push('\n');
        if let Some(folder) = self.path.parent() {
            std::fs::create_dir_all(folder)
                .map_err(|e| format!("cannot create {}: {e}", folder.display()))?;
        }
        let temporary = self.path.with_extension("json.partial");
        std::fs::write(&temporary, text)
            .map_err(|e| format!("cannot write {}: {e}", temporary.display()))?;
        std::fs::rename(&temporary, &self.path).map_err(|e| {
            let _ = std::fs::remove_file(&temporary);
            format!("cannot replace {}: {e}", self.path.display())
        })?;
        self.saved = settings.clone();
        Ok(())
    }
}

/// `settings`, read from a file of schema `schema`, brought up to the current one (see the
/// module).
fn migrated(mut settings: Settings, schema: u32) -> Settings {
    if schema < 2 {
        #[allow(clippy::float_cmp)] // The old default, exactly as it was saved.
        let old = |join: &mut f64| {
            if *join == OLD_JOIN_MM {
                *join = 0.0;
            }
        };
        old(&mut settings.tools.island_join_mm);
        old(&mut settings.tools.auto_supports.join_mm);
    }
    if schema < 3 {
        // The blockers' first colours, too faint (owner, 2026-11-02), take the new defaults
        // where the user kept them.
        let a = &mut settings.appearance;
        for (colour, old, new) in [
            (&mut a.blocker, OLD_BLOCKER, crate::appearance::BLOCKER),
            (
                &mut a.blocker_new,
                OLD_BLOCKER_NEW,
                crate::appearance::BLOCKER_NEW,
            ),
            (
                &mut a.blocker_selected,
                OLD_BLOCKER_SELECTED,
                crate::appearance::BLOCKER_SELECTED,
            ),
        ] {
            if colour.0 == old {
                colour.0 = new;
            }
        }
    }
    if schema < 4 {
        // The support test prints (TD-136): the Automatic tool starts from Medium without
        // braces, and an automatic brace is at most 1.0 mm thick, where the user kept the
        // first defaults.
        let t = &mut settings.tools;
        if t.auto_supports.weight == crate::supports_ui::Weight::Light {
            t.auto_supports.weight = crate::auto_ui::plan::DEFAULT_AUTO_SUPPORTS.weight;
        }
        t.auto_supports.braces = crate::auto_ui::plan::DEFAULT_AUTO_SUPPORTS.braces;
        #[allow(clippy::float_cmp)] // The old default, exactly as it was saved.
        if t.brace_diameter_mm == OLD_BRACE_DIAMETER_MM {
            t.brace_diameter_mm = crate::supports_ui::braces::DEFAULT_BRACES.diameter_mm;
        }
    }
    settings.schema_version = SCHEMA_VERSION;
    settings
}

/// The settings in `bytes`, and what they are.
fn read(bytes: &[u8]) -> (Settings, Opened) {
    // The schema first, so a newer file's unknown values are not mistaken for damage.
    #[derive(Deserialize)]
    struct Schema {
        schema_version: u32,
    }
    let schema = match serde_json::from_slice::<Schema>(bytes) {
        Ok(schema) => schema.schema_version,
        Err(error) => return (Settings::default(), Opened::Unreadable(error.to_string())),
    };
    match serde_json::from_slice::<Settings>(bytes) {
        Ok(settings) if schema > SCHEMA_VERSION => (settings, Opened::Newer(schema)),
        Ok(settings) => (migrated(settings, schema), Opened::Read),
        Err(_) if schema > SCHEMA_VERSION => (Settings::default(), Opened::Newer(schema)),
        Err(error) => (Settings::default(), Opened::Unreadable(error.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("karasu-settings-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        folder
    }

    #[test]
    fn a_schema_1_file_drops_the_old_join_distance_and_keeps_a_chosen_one() {
        let (settings, opened) = read(
            br#"{"schema_version": 1, "tools": {"island_join_mm": 1.0, "auto_supports": {"join_mm": 2.5}}}"#,
        );
        assert_eq!(opened, Opened::Read);
        assert_eq!(settings.schema_version, SCHEMA_VERSION);
        assert!(settings.tools.island_join_mm.abs() < f64::EPSILON);
        assert!((settings.tools.auto_supports.join_mm - 2.5).abs() < f64::EPSILON);
        // At schema 2, a 1 mm the user chose stays.
        let (settings, _) = read(br#"{"schema_version": 2, "tools": {"island_join_mm": 1.0}}"#);
        assert!((settings.tools.island_join_mm - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_schema_3_file_takes_the_tested_automatic_values_unless_chosen() {
        let (settings, _) = read(
            br#"{"schema_version": 3, "tools": {"auto_supports": {"weight": "light", "braces": true}, "brace_diameter_mm": 0.8}}"#,
        );
        let t = &settings.tools;
        assert_eq!(t.auto_supports.weight, crate::supports_ui::Weight::Medium);
        assert!(!t.auto_supports.braces);
        assert!((t.brace_diameter_mm - 1.0).abs() < f64::EPSILON);
        // Another preset and diameter stay; at schema 4, Light and braces stay too.
        let (settings, _) = read(
            br#"{"schema_version": 3, "tools": {"auto_supports": {"weight": "heavy"}, "brace_diameter_mm": 0.6}}"#,
        );
        assert_eq!(
            settings.tools.auto_supports.weight,
            crate::supports_ui::Weight::Heavy
        );
        assert!((settings.tools.brace_diameter_mm - 0.6).abs() < f64::EPSILON);
        let (settings, _) = read(
            br#"{"schema_version": 4, "tools": {"auto_supports": {"weight": "light", "braces": true}}}"#,
        );
        assert_eq!(
            settings.tools.auto_supports.weight,
            crate::supports_ui::Weight::Light
        );
        assert!(settings.tools.auto_supports.braces);
    }

    #[test]
    fn a_schema_2_file_takes_the_new_blocker_colours_unless_chosen() {
        let (settings, _) = read(
            br#"{"schema_version": 2, "appearance": {"blocker": [184, 180, 200, 72], "blocker_new": [1, 2, 3, 255]}}"#,
        );
        let a = settings.appearance;
        assert_eq!(a.blocker.0, crate::appearance::BLOCKER);
        assert_eq!(a.blocker_new.0, [1, 2, 3, 255]);
        assert_eq!(a.blocker_selected.0, crate::appearance::BLOCKER_SELECTED);
    }

    #[test]
    fn settings_are_saved_on_change_and_read_back() {
        let folder = folder("round");
        let path = folder.join("nested").join(FILE_NAME);
        let (mut store, settings, opened) = Store::open(path.clone());
        assert_eq!(opened, Opened::Missing);
        assert_eq!(settings, Settings::default());
        // Nothing changed: nothing written.
        store.save(&settings).unwrap();
        assert!(!path.exists());
        let mut changed = settings;
        changed.layout.sidebar_open = false;
        changed
            .layout
            .dialogs
            .insert("library".into(), [910.0, 600.0]);
        changed.tools.hollow_wall_mm = 1.5;
        changed.repair_on_import = ImportRepair::Off;
        changed.appearance.show_selection_outline = false;
        assert!(store.changed(&changed));
        store.save(&changed).unwrap();
        assert!(!store.changed(&changed));
        assert!(!path.with_extension("json.partial").exists());
        let (_, read, opened) = Store::open(path);
        assert_eq!(opened, Opened::Read);
        assert_eq!(read, changed);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn an_earlier_file_reads_with_defaults_for_what_it_lacks() {
        let (settings, opened) =
            read(br#"{"schema_version": 1, "layout": {"sidebar_open": false}}"#);
        assert_eq!(opened, Opened::Read);
        assert!(!settings.layout.sidebar_open);
        assert!(settings.layout.panel_open);
        assert_eq!(settings.tools, ToolSettings::default());
        assert_eq!(settings.appearance, Appearance::default());
    }

    #[test]
    fn a_damaged_or_newer_file_is_reported_and_never_saved_over() {
        let folder = folder("damaged");
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join(FILE_NAME);
        std::fs::write(&path, b"{ not json").unwrap();
        let (mut store, settings, opened) = Store::open(path.clone());
        assert!(matches!(opened, Opened::Unreadable(_)), "{opened:?}");
        assert_eq!(settings, Settings::default());
        let mut changed = settings;
        changed.layout.panel_open = false;
        assert!(!store.changed(&changed));
        store.save(&changed).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"{ not json");
        // A newer version's file: what is known is read, unknown fields are ignored, and the
        // file is left for that version.
        let newer = br#"{"schema_version": 7, "layout": {"panel_open": false}, "future": 1}"#;
        std::fs::write(&path, newer).unwrap();
        let (mut store, settings, opened) = Store::open(path.clone());
        assert_eq!(opened, Opened::Newer(7));
        assert!(!settings.layout.panel_open);
        store.save(&Settings::default()).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), newer);
        // A wrong value in a known field is damage.
        let (_, opened) = read(br#"{"schema_version": 1, "layout": {"panel_open": 3}}"#);
        assert!(matches!(opened, Opened::Unreadable(_)), "{opened:?}");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn starting_over_keeps_a_file_it_could_not_save_over_beside_the_new_one() {
        let folder = folder("over");
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join(FILE_NAME);
        std::fs::write(&path, b"{ damaged").unwrap();
        let (mut store, settings, _) = Store::open(path.clone());
        let kept = store.start_over().unwrap().unwrap();
        assert_eq!(kept, folder.join("settings.json.unreadable"));
        assert_eq!(std::fs::read(&kept).unwrap(), b"{ damaged");
        assert!(store.changed(&settings));
        store.save(&settings).unwrap();
        let (_, read, opened) = Store::open(path.clone());
        assert_eq!((read, opened), (Settings::default(), Opened::Read));
        // A second damaged file is kept under the next free name.
        std::fs::write(&path, b"{ again").unwrap();
        let (mut store, _, _) = Store::open(path.clone());
        let kept = store.start_over().unwrap().unwrap();
        assert_eq!(kept, folder.join("settings.json.unreadable-2"));
        // A readable file is not kept: starting over writes the defaults over it.
        let (mut store, _, _) = Store::open(folder.join("fresh.json"));
        assert_eq!(store.start_over().unwrap(), None);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
