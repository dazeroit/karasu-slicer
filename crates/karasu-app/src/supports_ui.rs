//! The Supports tool (M6.4g, TD-108), from the owner's mockup "Supports workflow (M6.4g)".
//!
//! **Supports follow the model.** An object's supports are kept in its model frame (TD-108), so
//! they go along with every move, turn and scale; the pipeline places them before building them.
//!
//! **Applied at once** (owner, 2026-10-27): with "Place supports" on, the support a click would
//! add follows the pointer under the selected model; a click adds it there, standing on the
//! plate below its tip, with the next support's values (the weight and its four values, kept as
//! the user's settings), as one undoable step. A support with an error is added all the same,
//! red, to be changed; it prints as it is, with a warning (TD-108). A surface facing up takes
//! no support: a tip there would point into the air.
//!
//! **Built where they are edited.** Every object's supports are built against its latest
//! preparation (the hollowed shell, or the placed model) on a thread of their own, the selected
//! object's first, with every finding: the anchors, the axes, a tip piercing a thin wall, the
//! clearance proven from the model, drain holes and the build volume. The preparation leaves
//! the supports out, so changing them never prepares a model again. The scene draws the built
//! parts on the GPU, uploaded once per build, each support in its colour (applied, selected or
//! wrong); a build whose model moves is carried along until it is built again. A collision can
//! be accepted per support: the finding stays.
//!
//! **Mini supports** (owner, 2026-10-30; TD-113): with the tool strip's Mini on, new supports are
//! thin rods with a ball at each end, standing on the model below their tips or on a support's
//! shaft, placed in any of the three ways.
//!
//! The list shows every support; a row opens the support's values, which change it alone.
//! Escape stops placing, then releases the support; Delete removes the selected one.
use crate::{
    appearance::Appearance,
    camera::{Mat4, Ray},
    gizmo::{ray_mesh, ray_triangle, Affine},
    i18n::tr,
    messages::{Message, Status},
    preparation_ui::{carried, centre, translation, PreparationUi},
    theme,
    viewport::{Draw, Viewport},
    workspace::{SurfaceHit, Workspace},
};
use eframe::egui_wgpu;
use karasu_core::{
    cancellation::Cancellation,
    geometry::{ObjectTransform, Point3},
    mesh::TriangleMesh,
    supports::{
        build_supports, AxisSpec, BaseSpec, BuildVolume, ContactShape, FindingKind, ManualSupport,
        Preset, Severity, SupportDocument, SupportFinding, SupportKind, SupportOptions,
        SupportOrigin, SupportParameters, SupportPart, SupportScene, SupportSurface,
    },
};
use karasu_project::pipeline::{build_model_supports, Preparation};
use serde::{Deserialize, Serialize};
pub mod braces;
pub(crate) mod branches;
mod handles;
pub(crate) mod height;
mod list;
pub(crate) mod merging;
pub(crate) mod panel;
pub(crate) mod route;
pub(crate) mod trees;

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::Instant,
};

/// The viewport mesh keys of each object's built supports.
const SCENE_KEYS: u64 = 0x5355_5050_0000_0000; // "SUPP"
/// The identity of the stand-in a branch's preview joins (see [`alone`]); the branch is 1.
const STAND_IN: u64 = 2;
/// The viewport mesh key of the support a click would add.
const PREVIEW_KEY: u64 = 0x5355_5050_4F52_5453; // "SUPPORTS"
/// Undo merge keys of edits to one support's values.
const MERGE_KEYS: u64 = 0x5355_504D_0000_0000; // "SUPM"
/// A surface whose outward normal rises above this (the cosine of its angle from straight down,
/// negated) faces up: a tip there would point into the air.
const FACING_UP: f64 = 0.0;

/// The weight of new supports: the core's presets (TD-062).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Weight {
    Light,
    #[default]
    Medium,
    Heavy,
}

impl Weight {
    /// The weights, lightest first.
    pub const ALL: [Self; 3] = [Self::Light, Self::Medium, Self::Heavy];

    fn preset(self) -> Preset {
        match self {
            Self::Light => Preset::Light,
            Self::Medium => Preset::Medium,
            Self::Heavy => Preset::Heavy,
        }
    }

    /// The weight's name.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::Light => tr!("supports-light"),
            Self::Medium => tr!("supports-medium"),
            Self::Heavy => tr!("supports-heavy"),
        }
    }
}

/// Every value of a support and how its tip leans: what a preset keeps and what the next
/// support is made with.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SupportValues {
    pub contact_diameter_mm: f64,
    pub tip_length_mm: f64,
    pub shaft_diameter_mm: f64,
    pub penetration_mm: f64,
    /// A ball at the contact, not a flat cone.
    pub sphere_contact: bool,
    pub contact_sphere_diameter_mm: f64,
    pub tip_body_diameter_mm: f64,
    /// A shaft wider at its foot; `None` keeps it straight.
    pub shaft_bottom_diameter_mm: Option<f64>,
    pub sides: u16,
    pub pad_diameter_mm: f64,
    pub pad_thickness_mm: f64,
    pub cone_diameter_mm: f64,
    pub cone_height_mm: f64,
    pub foot_contact_diameter_mm: f64,
    pub foot_body_diameter_mm: f64,
    pub foot_length_mm: f64,
    pub foot_penetration_mm: f64,
    /// A mini support's diameter and the depth of its ends (owner, 2026-10-30).
    pub mini_diameter_mm: f64,
    pub mini_penetration_mm: f64,
    /// The tip points at the shaft, not along the surface's normal.
    pub tip_towards_shaft: bool,
}

impl Default for SupportValues {
    fn default() -> Self {
        Self::of(Weight::Medium)
    }
}

impl SupportValues {
    /// The weight's own values, the tip along the surface's normal.
    #[must_use]
    pub fn of(weight: Weight) -> Self {
        Self::with(&weight.preset().parameters(), false)
    }

    /// The values `p`, the tip pointing at the shaft when `tip_towards_shaft`.
    fn with(p: &SupportParameters, tip_towards_shaft: bool) -> Self {
        Self {
            contact_diameter_mm: p.contact_diameter_mm,
            tip_length_mm: p.tip_length_mm,
            shaft_diameter_mm: p.shaft_diameter_mm,
            penetration_mm: p.penetration_mm,
            sphere_contact: p.contact_shape == ContactShape::Sphere,
            contact_sphere_diameter_mm: p.contact_sphere_diameter_mm,
            tip_body_diameter_mm: p.tip_body_diameter_mm,
            shaft_bottom_diameter_mm: p.shaft_bottom_diameter_mm,
            sides: p.sides,
            pad_diameter_mm: p.pad_diameter_mm,
            pad_thickness_mm: p.pad_thickness_mm,
            cone_diameter_mm: p.cone_diameter_mm,
            cone_height_mm: p.cone_height_mm,
            foot_contact_diameter_mm: p.foot_contact_diameter_mm,
            foot_body_diameter_mm: p.foot_body_diameter_mm,
            foot_length_mm: p.foot_length_mm,
            foot_penetration_mm: p.foot_penetration_mm,
            mini_diameter_mm: p.mini_diameter_mm,
            mini_penetration_mm: p.mini_penetration_mm,
            tip_towards_shaft,
        }
    }

    /// The complete parameters of a support made with these values.
    #[must_use]
    pub fn parameters(&self) -> SupportParameters {
        SupportParameters {
            contact_shape: if self.sphere_contact {
                ContactShape::Sphere
            } else {
                ContactShape::Cone
            },
            contact_diameter_mm: self.contact_diameter_mm,
            contact_fit_min_mm: None,
            contact_sphere_diameter_mm: self.contact_sphere_diameter_mm,
            tip_body_diameter_mm: self.tip_body_diameter_mm,
            tip_length_mm: self.tip_length_mm,
            penetration_mm: self.penetration_mm,
            shaft_diameter_mm: self.shaft_diameter_mm,
            shaft_bottom_diameter_mm: self.shaft_bottom_diameter_mm,
            sides: self.sides,
            pad_diameter_mm: self.pad_diameter_mm,
            pad_thickness_mm: self.pad_thickness_mm,
            cone_diameter_mm: self.cone_diameter_mm,
            cone_height_mm: self.cone_height_mm,
            foot_contact_diameter_mm: self.foot_contact_diameter_mm,
            foot_body_diameter_mm: self.foot_body_diameter_mm,
            foot_length_mm: self.foot_length_mm,
            foot_penetration_mm: self.foot_penetration_mm,
            mini_diameter_mm: self.mini_diameter_mm,
            mini_penetration_mm: self.mini_penetration_mm,
        }
    }

    /// Whether a support can be made with these values (a settings file may keep any): their
    /// ranges and couplings, as the core checks them.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.parameters().validate(0, &mut Vec::new()).is_ok()
    }
}

/// A support preset of the user's own (owner, 2026-10-27): a name and every value, kept with the
/// settings for every project, beside Light, Medium and Heavy.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SupportPreset {
    pub name: String,
    pub values: SupportValues,
}

/// What the next support is made with: the preset chosen (a weight, or one of the user's own)
/// and its values as changed in the panel (the four in view, the rest under Advanced). Choosing
/// a preset sets every value to its own. Changed values are for the model at hand (owner,
/// 2026-10-27): the presets keep values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NextSupport {
    /// The weight chosen, or the one the user's preset was made from.
    pub weight: Weight,
    /// The user's preset chosen, by its place in the list.
    pub mine: Option<usize>,
    pub values: SupportValues,
    /// The contact fits the surface, narrowing down to this diameter at least (TD-116; owner,
    /// 2026-11-02): the tool's choice, kept whatever preset is chosen, not a preset's value.
    pub contact_fit_min_mm: Option<f64>,
}

impl Default for NextSupport {
    fn default() -> Self {
        Self::of(Weight::Medium)
    }
}

impl NextSupport {
    /// The weight's own values, the tip along the surface's normal.
    #[must_use]
    pub fn of(weight: Weight) -> Self {
        Self {
            weight,
            mine: None,
            values: SupportValues::of(weight),
            contact_fit_min_mm: None,
        }
    }

    /// The user's preset `index` of `presets`, or Medium when there is none.
    #[must_use]
    pub fn of_preset(presets: &[SupportPreset], index: usize) -> Self {
        presets
            .get(index)
            .filter(|p| p.values.is_valid())
            .map_or_else(Self::default, |preset| Self {
                weight: Weight::Medium,
                mine: Some(index),
                values: preset.values,
                contact_fit_min_mm: None,
            })
    }

    /// `preset`'s values, with this one's contact fitting (a preset chosen keeps it).
    #[must_use]
    pub fn chosen(&self, preset: Self) -> Self {
        Self {
            contact_fit_min_mm: self.contact_fit_min_mm,
            ..preset
        }
    }

    /// These values, the contact fitting the surface down to `least` when given.
    #[must_use]
    pub fn fitting(self, least: Option<f64>) -> Self {
        Self {
            contact_fit_min_mm: least,
            ..self
        }
    }

    /// The complete parameters of a support made now: a smallest fitted contact wider than the
    /// contact is the contact.
    #[must_use]
    pub fn parameters(&self) -> SupportParameters {
        fitted(self.values.parameters(), self.contact_fit_min_mm)
    }

    /// The next support's tip axis.
    #[must_use]
    pub fn axis(&self) -> AxisSpec {
        if self.values.tip_towards_shaft {
            AxisSpec::Straight
        } else {
            AxisSpec::Normal
        }
    }
}

/// `parameters` whose contact fits the surface down to `least` when given, at most their
/// contact (the core's coupling).
#[must_use]
pub fn fitted(mut parameters: SupportParameters, least: Option<f64>) -> SupportParameters {
    parameters.contact_fit_min_mm = least.map(|m| m.min(parameters.contact_diameter_mm));
    parameters
}

/// What a build is of: an object's supports in its model frame, its placement and the
/// preparation's generation. A build is outdated when any changes.
#[derive(Clone, Debug, PartialEq)]
struct Input {
    document: SupportDocument,
    placement: ObjectTransform,
    generation: u64,
}

/// A build running on its own thread.
struct Job {
    object: usize,
    input: Input,
    stop: Arc<AtomicBool>,
    receiver: mpsc::Receiver<Result<SupportScene, String>>,
}

impl Drop for Job {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// An object's latest build, and whether its mesh is in the viewport.
struct Built {
    input: Input,
    scene: Result<SupportScene, String>,
    origin: [f64; 3],
    /// The object's placement when built, inverted: carries the build to where the object is
    /// while it is built again.
    back: Option<Affine>,
    uploaded: bool,
}

/// What the pointer is over while placing: where a click puts a support, or why it cannot.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Candidate {
    /// A support's tip can go here, in world coordinates.
    Here,
    /// The surface faces up.
    FacingUp,
    /// A mini support finds no surface below its tip to stand on.
    NoFoot,
}

/// The support a click would add, where the pointer is, as built: drawn while placing.
struct Preview {
    /// Where its tip is and what it is made with: built again only when either changes.
    at: [f64; 3],
    next: NextSupport,
    scene: SupportScene,
    origin: [f64; 3],
    /// It shows an error alone (outside the build volume, a shaft that cannot reach the plate).
    error: bool,
    /// Which build it is, so it is uploaded once.
    stamp: u64,
}

impl Preview {
    /// A preview of the support numbered 1 in `scene`, at `at`, made with `next`: its parts
    /// alone when the scene holds a stand-in for the support it joins (a branch).
    fn of(scene: SupportScene, at: [f64; 3], next: NextSupport) -> Self {
        let error = scene
            .findings()
            .iter()
            .any(|f| f.entity == 1 && f.severity == Severity::Error);
        Self {
            at,
            next,
            origin: centre(scene.mesh()),
            error,
            scene,
            stamp: 0,
        }
    }

    /// The triangles of the support numbered 1 (its parts follow each other), or `None` for
    /// the whole scene when it holds nothing else.
    fn range(&self) -> Option<[u32; 2]> {
        let mine = || self.scene.part_ranges().iter().filter(|r| r.entity == 1);
        if self.scene.part_ranges().iter().all(|r| r.entity == 1) {
            return None;
        }
        let start = mine().map(|r| r.triangles.start).min()?;
        let end = mine().map(|r| r.triangles.end).max()?;
        Some([start, end - start].map(|n| u32::try_from(n).unwrap_or(u32::MAX)))
    }
}

/// The Supports tool's state, and every object's built supports.
#[derive(Default)]
#[allow(clippy::struct_excessive_bools)] // The tool's switches and modes, each its own.
pub struct SupportsUi {
    /// The panel's Advanced switch.
    pub advanced: bool,
    /// The tools' defaults, for their fields' reset buttons (owner, 2026-11-02), and the next
    /// support's.
    pub tool_defaults: crate::settings::ToolDefaults,
    pub next_default: NextSupport,
    /// A knee stays where it is when its support's foot is moved: the panel's switch "Knee
    /// follows the foot" turned off. By default the knee follows, keeping the shaft vertical
    /// (owner, 2026-10-29).
    pub knee_stays: bool,
    /// What the next support is made with.
    pub next: NextSupport,
    placing: bool,
    candidate: Option<Candidate>,
    /// The height under the pointer while a support is placed or its tip dragged (TD-137).
    pointer_mm: Option<f64>,
    /// The height Shift holds, if it does (TD-137).
    held: Option<height::Held>,
    /// The selected support: its object and identity.
    selected: Option<(usize, u64)>,
    job: Option<Job>,
    /// Each object's latest build, by its index on the plate.
    builds: HashMap<usize, Built>,
    /// The plate's object count the builds were made for: indices name other objects after a
    /// model is added or removed.
    objects: usize,
    /// Viewport meshes to free.
    retired: Vec<u64>,
    preview: Option<Preview>,
    /// The preview in the viewport, by its stamp, so it is uploaded once.
    uploaded_preview: Option<u64>,
    /// The previews built so far: each one's stamp.
    stamp: u64,
    /// The selected support's handle being dragged.
    drag: Option<handles::Drag>,
    /// The selected support's handle last pressed, which shows its gizmo (the knee, or a foot
    /// on the plate; owner, 2026-10-29).
    active: Option<handles::Grip>,
    /// A click places a new support's tip, and a second its foot (the tool strip's Place
    /// option "Tip, then foot", owner 2026-10-29); otherwise one click places a whole support.
    pub tip_then_foot: bool,
    /// A click places a branch's tip, and a second, on a support's shaft, the branch joining it
    /// (the Place option "Branch onto a support", owner 2026-10-30). Kept with the settings.
    pub branching: bool,
    /// New supports are mini supports (the tool strip's Mini, owner 2026-10-30), placed in any
    /// of the three ways: one click stands one on the model straight below its tip. Kept with
    /// the settings.
    pub mini: bool,
    /// The branch's tip clicked, waiting for the shaft it joins.
    branch_tip: Option<branches::BranchTip>,
    /// The shaft under the pointer offers no junction within 45° clear of the model.
    branch_refused: bool,
    /// Merge trunks: the merge shown, to apply or discard (owner, 2026-10-30).
    merging: Option<merging::Merging>,
    /// Join a support: the selected support, waiting for the shaft it joins.
    joining: Option<(usize, u64)>,
    /// The shaft clicked cannot take the support waiting to join it.
    join_refused: bool,
    /// New supports go straight down, not routed clear of the model: the panel's switch
    /// "Avoid the model" turned off (owner, 2026-10-29).
    pub straight_only: bool,
    /// New supports go inside the hollowed model's cavity, where the section shows it: their
    /// tips on its ceiling, their feet on its floor (the panel's Surface choice, TD-108).
    pub on_cavity: bool,
    /// The selected hollowed model's cavity shows from inside, its outside as a ghost (the
    /// tool strip's cavity view, owner 2026-10-29); new supports then go in the cavity.
    pub cavity_view: bool,
    /// Where new supports went before the cavity view, so they go there again after it.
    surface_before: Option<bool>,
    /// The list window is open (owner, 2026-10-29), kept with the settings.
    pub list_open: bool,
    /// The selected model's surface for routes is kept ready although nothing is placed: the
    /// Islands tool adds supports under its islands.
    pub route_wanted: bool,
    /// Only the supports' tips show while the tool is in use (owner, 2026-10-29): their shafts,
    /// bases and rafts hide the model's lower parts otherwise. Kept with the settings.
    pub tips_only: bool,
    /// Which supports the list window lists: every one, those with errors or the mini ones
    /// (owner R8, 2026-11-10); the others are then ghosts.
    list_filter: list::ListFilter,
    /// How far the supports' plate bases (pads and cones) and the rafts have faded out, from 0
    /// (shown) to 1 (gone): they go with the plate's surface while the view is below the plate,
    /// so only the shafts and tips stand under the model (owner R9, 2026-11-10). Set each frame
    /// from the view.
    pub base_hidden: f32,
    /// Which supports the viewport shows while the tool is in use (the strip's Show, owner
    /// 2026-10-31), kept with the settings.
    pub shown: crate::shown::Shown,
    /// A support was selected in the scene: the list scrolls to its row once.
    reveal: bool,
    /// The user's own presets (owner, 2026-10-27), kept with the settings.
    pub presets: Vec<SupportPreset>,
    /// The user's preset new supports start from, set in the settings; `None` starts from the
    /// default weight.
    pub default_preset: Option<usize>,
    /// A preset's name being typed: for a new one with the next support's values, or for
    /// renaming the one at this place.
    naming: Option<(Option<usize>, String)>,
    /// The user's preset whose deletion waits for the user's yes.
    deleting: Option<usize>,
    /// The selected model's surface for routes, and its grid being built.
    route: Option<Arc<route::RouteSurface>>,
    route_job: Option<route::RouteJob>,
    outbox: Vec<Message>,
    /// The Braces tool's state (owner, 2026-10-29): its braces are built with the supports.
    pub bracing: braces::Bracing,
    /// How the Islands tool's supports are made (tool values), kept here with the presets they
    /// name by place.
    pub islands: crate::islands_ui::plan::IslandSupports,
    /// The user's preset island supports start from, set in the settings; `None` starts from
    /// the default weight.
    pub islands_default_mine: Option<usize>,
    /// How the Automatic tool's plans are made (tool values), kept here with the presets they
    /// name by place.
    pub automatic: crate::auto_ui::plan::AutoSupports,
    /// The user's preset the cavity's automatic supports start from, set in the settings
    /// (TD-137); `None` starts from the cavity's default weight.
    pub automatic_cavity_default_mine: Option<usize>,
    /// How the Grid mode's supports are made (tool values, TD-138), kept here with the presets
    /// they name by place.
    pub grid: crate::grid_ui::plan::GridValues,
    /// The user's preset the grid's supports start from, set in the settings; `None` starts
    /// from the default weight.
    pub grid_default_mine: Option<usize>,
    /// The user's preset the Automatic tool's supports start from, set in the settings.
    pub automatic_default_mine: Option<usize>,
}

/// Which tool edits the selected model's supports: its supports, or its braces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Editing {
    Supports,
    Braces,
}

/// The identity map: built supports are in world coordinates.
const IDENTITY: Affine = Affine {
    linear: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
    translation: [0.0; 3],
};

/// The viewport mesh key of object `index`'s built supports.
fn scene_key(index: usize) -> u64 {
    SCENE_KEYS | (index as u64 & 0xFFFF_FFFF)
}

impl SupportsUi {
    /// The tool as it starts: its list window open (owner, 2026-10-29), the rest by default.
    #[must_use]
    pub fn new() -> Self {
        Self {
            list_open: true,
            bracing: braces::Bracing::new(),
            ..Self::default()
        }
    }

    /// Whether the pointer places supports, so a click is the tool's.
    #[must_use]
    pub fn placing(&self) -> bool {
        self.placing
    }

    /// Start or stop placing supports (the tool strip's Place, owner 2026-10-29).
    pub fn toggle_placing(&mut self) {
        self.placing = !self.placing;
        self.forget_placing();
    }

    /// Place new supports in the cavity, or on the outside, which ends the cavity view.
    pub fn set_cavity(&mut self, on: bool) {
        self.on_cavity = on;
        if !on {
            self.cavity_view = false;
            self.surface_before = None;
        }
        self.forget_placing();
    }

    /// Show the selected model's cavity from inside, or not: on, new supports go in the cavity;
    /// off, where they went before (owner, 2026-10-29).
    pub fn toggle_cavity_view(&mut self) {
        if self.cavity_view {
            self.cavity_view = false;
            let before = self.surface_before.take().unwrap_or(self.on_cavity);
            self.set_cavity(before);
        } else {
            self.surface_before = Some(self.on_cavity);
            self.set_cavity(true);
            self.cavity_view = true;
        }
    }

    /// Place new supports tip then foot, in two clicks, or whole in one.
    pub fn set_tip_then_foot(&mut self, on: bool) {
        self.tip_then_foot = on;
        if on {
            self.branching = false;
        }
        self.forget_placing();
    }

    /// Place mini supports, or standard ones.
    pub fn toggle_mini(&mut self) {
        self.mini = !self.mini;
        self.forget_placing();
    }

    /// What new supports are made of.
    fn next_kind(&self) -> SupportKind {
        if self.mini {
            SupportKind::Mini
        } else {
            SupportKind::Standard
        }
    }

    /// The next support's tip axis: a mini support's ball sits along the surface's normal.
    fn next_axis(&self) -> AxisSpec {
        if self.mini {
            AxisSpec::Normal
        } else {
            self.next.axis()
        }
    }

    /// Forget where the pointer would place a support, and a new support's foot being placed.
    fn forget_placing(&mut self) {
        self.candidate = None;
        self.preview = None;
        self.branch_tip = None;
        self.branch_refused = false;
        self.drag.take_if(|d| d.adding);
    }

    /// Whether a support of object `index` is selected, which Delete removes.
    #[must_use]
    pub fn support_selected(&self, index: Option<usize>) -> bool {
        self.selected
            .is_some_and(|(object, _)| Some(object) == index)
    }

    /// The messages to show as notifications, taken.
    pub fn take_messages(&mut self) -> Vec<Message> {
        std::mem::take(&mut self.outbox)
    }

    /// Stop placing and release the selected support when the tool or the model is left; the
    /// cavity view ends too.
    pub fn close(&mut self) {
        self.cavity_view = false;
        self.surface_before = None;
        self.stop_editing();
    }

    /// Stop placing and release the selected support, the cavity view kept: the Braces tool,
    /// which shares it, is in use.
    pub fn stop_editing(&mut self) {
        self.joining = None;
        self.merging = None;
        self.branch_tip = None;
        self.branch_refused = false;
        self.drag = None;
        self.active = None;
        self.placing = false;
        self.candidate = None;
        self.preview = None;
        self.selected = None;
    }

    /// Escape: stop placing, else release the selected support. Returns whether it did either.
    pub fn escape(&mut self) -> bool {
        if self.brace_escape() {
            return true;
        }
        if self.cancel_drag() {
            return true;
        }
        if self.joining.take().is_some() || self.merging.take().is_some() {
            return true;
        }
        // A branch's tip clicked is taken back first.
        if self.branch_tip.take().is_some() {
            self.preview = None;
            self.branch_refused = false;
            return true;
        }
        if self.placing {
            self.placing = false;
            self.candidate = None;
            self.preview = None;
            return true;
        }
        self.active = None;
        self.selected.take().is_some()
    }

    /// Delete: remove the selected support. Returns whether there was one.
    pub fn delete(&mut self, workspace: &mut Workspace) -> bool {
        if self.delete_brace(workspace) {
            return true;
        }
        let Some((object, id)) = self.selected.take() else {
            return false;
        };
        self.remove(workspace, object, id);
        true
    }

    /// Delete every support of model `index`, and so every brace, as one undoable step; its
    /// raft stays, waiting for supports.
    pub fn delete_all_supports(&mut self, workspace: &mut Workspace, index: usize) {
        if let Some(mut document) = workspace.supports(index).cloned() {
            document.supports.clear();
            document.braces.clear();
            workspace.set_supports(index, Some(document), None);
        }
        self.drag = None;
        self.active = None;
        self.selected = self.selected.filter(|&(object, _)| object != index);
        self.close_braces();
    }

    /// Delete model `index`'s supports the Islands tool placed, and the braces on them, as one
    /// undoable step; the others stay.
    pub fn delete_island_supports(&mut self, workspace: &mut Workspace, index: usize) {
        self.delete_of_origin(workspace, index, SupportOrigin::Islands);
    }

    /// Delete model `index`'s supports the Automatic tool planned, and the braces on them, as
    /// one undoable step; the others stay.
    pub fn delete_automatic_supports(&mut self, workspace: &mut Workspace, index: usize) {
        self.delete_of_origin(workspace, index, SupportOrigin::Automatic);
    }

    /// Delete model `index`'s supports of `origin`, the branches and braces on them, as one
    /// undoable step.
    fn delete_of_origin(&mut self, workspace: &mut Workspace, index: usize, origin: SupportOrigin) {
        if let Some(mut document) = workspace.supports(index).cloned() {
            let gone: Vec<u64> = document
                .supports
                .iter()
                .filter(|s| s.origin == origin)
                .map(|s| s.id)
                .collect();
            // The branches on them go too.
            let gone = branches::with_branches(&document, &gone);
            branches::without(&mut document, &gone);
            workspace.set_supports(index, Some(document), None);
            if self
                .selected
                .is_some_and(|(object, id)| object == index && gone.contains(&id))
            {
                self.selected = None;
            }
        }
        self.drag = None;
        self.active = None;
        self.close_braces();
    }

    /// Delete every brace of model `index` as one undoable step.
    pub fn delete_all_braces(&mut self, workspace: &mut Workspace, index: usize) {
        if let Some(mut document) = workspace.supports(index).cloned() {
            document.braces.clear();
            workspace.set_supports(index, Some(document), None);
        }
        self.close_braces();
    }

    /// An identity no support or brace of object `index` has.
    fn next_id(workspace: &Workspace, index: usize) -> u64 {
        workspace.supports(index).map_or(1, |document| {
            document
                .supports
                .iter()
                .map(|s| s.id)
                .chain(document.braces.iter().map(|b| b.id))
                .max()
                .unwrap_or(0)
                + 1
        })
    }

    /// Remove support `id` of object `index`, the branches on it (owner, 2026-10-30) and the
    /// braces on any of them, as one undoable step.
    fn remove(&mut self, workspace: &mut Workspace, index: usize, id: u64) {
        if let Some(mut document) = workspace.supports(index).cloned() {
            branches::without(&mut document, &[id]);
            workspace.set_supports(index, Some(document), None);
        }
        if self.selected == Some((index, id)) {
            self.selected = None;
        }
    }

    /// Replace support `support.id` of object `index` with `support`, as an undoable step
    /// merged with the next changes of the same support (a value dragged in a field).
    pub(crate) fn replace(workspace: &mut Workspace, index: usize, support: ManualSupport) {
        if let Some(mut document) = workspace.supports(index).cloned() {
            if let Some(s) = document.supports.iter_mut().find(|s| s.id == support.id) {
                let key = MERGE_KEYS | (index as u64) << 24 | (support.id & 0xFF_FFFF);
                *s = support;
                workspace.set_supports(index, Some(document), Some(key));
            }
        }
    }

    /// Replace support `support.id` of object `index` with `support`, as one undoable step (a
    /// handle's drag, released).
    fn replace_once(workspace: &mut Workspace, index: usize, support: ManualSupport) {
        if let Some(mut document) = workspace.supports(index).cloned() {
            if let Some(s) = document.supports.iter_mut().find(|s| s.id == support.id) {
                *s = support;
                workspace.set_supports(index, Some(document), None);
            }
        }
    }

    /// Whether object `index`'s latest build describes its supports as they are now.
    fn current(&self, workspace: &Workspace, preparation: &PreparationUi, index: usize) -> bool {
        Self::input(workspace, preparation, index)
            .zip(self.builds.get(&index))
            .is_some_and(|((input, _), built)| built.input == input)
    }

    /// Follow the pointer's `hit` on the selected model while placing: the support a click
    /// would add is shown there, as holes are (owner, 2026-10-27).
    pub fn hover(&mut self, workspace: &Workspace, hit: Option<SurfaceHit>, selected: usize) {
        if !self.placing {
            self.candidate = None;
            self.preview = None;
            self.pointer_mm = None;
            return;
        }
        let hit = hit.filter(|h| h.object == selected);
        self.pointer_mm = hit.map(|h| h.point[2]);
        self.candidate = hit.map(|h| {
            if h.normal[2] > FACING_UP {
                Candidate::FacingUp
            } else {
                Candidate::Here
            }
        });
        let Some(hit) = hit.filter(|_| self.candidate == Some(Candidate::Here)) else {
            self.preview = None;
            return;
        };
        // Exactly the same point: the pointer has not moved, so nothing is built again.
        #[allow(clippy::float_cmp)]
        let same = self
            .preview
            .as_ref()
            .is_some_and(|p| p.at == hit.point && p.next == self.next);
        if !same {
            self.stamp += 1;
            let stamp = self.stamp;
            let Some(routed) = self.routed(workspace, selected, hit.point, hit.normal) else {
                self.candidate = Some(Candidate::NoFoot);
                self.preview = None;
                return;
            };
            self.preview =
                preview(workspace, &hit, self.next, routed).map(|p| Preview { stamp, ..p });
        }
    }

    /// A click in the viewport on the selected model `index`: while placing, a support is added
    /// where the ray meets its underside, at once and as one undoable step (owner,
    /// 2026-10-27), even where it shows an error: it is drawn red and can be changed; otherwise
    /// the support under the pointer is selected. Returns whether the click was the tool's (a
    /// click elsewhere selects models as usual).
    pub fn click(
        &mut self,
        workspace: &mut Workspace,
        ray: &Ray,
        hit: Option<SurfaceHit>,
        index: usize,
    ) -> bool {
        if self.joining.is_some() {
            return self.join_click(workspace, ray, index);
        }
        if self.placing {
            if self.branching {
                return self.branch_click(workspace, ray, hit, index);
            }
            if self.add_footed(workspace) {
                return true;
            }
            let Some(hit) = hit.filter(|h| h.object == index) else {
                return true;
            };
            if hit.normal[2] > FACING_UP {
                self.candidate = Some(Candidate::FacingUp);
                return true;
            }
            if self.tip_then_foot {
                self.start_foot(workspace, index, &hit);
                return true;
            }
            if let Some(id) = self.add_at(workspace, index, &hit) {
                self.selected = Some((index, id));
                self.reveal = true;
            }
            return true;
        }
        let picked = self.pick(ray, index);
        if picked.is_some() {
            let chosen = picked.map(|id| (index, id));
            if chosen != self.selected {
                self.active = None;
            }
            self.selected = chosen;
            self.reveal = true;
            return true;
        }
        false
    }

    /// Add the support placing would add with its tip at `hit` on model `index`, as one undoable
    /// step. Returns its identity.
    fn add_at(&mut self, workspace: &mut Workspace, index: usize, hit: &SurfaceHit) -> Option<u64> {
        let to_model = workspace.model_to_world(index)?.inverse()?;
        let id = Self::next_id(workspace, index);
        let mut document = workspace
            .supports(index)
            .cloned()
            .unwrap_or_else(|| SupportDocument::new(Preset::Medium));
        // Routed clear of the model as the preview showed it, then in the model's frame.
        let Some(routed) = self.routed(workspace, index, hit.point, hit.normal) else {
            self.candidate = Some(Candidate::NoFoot);
            return None;
        };
        let world = routed.support;
        document.supports.push(ManualSupport {
            id,
            tip_mm: to_model.point(world.tip_mm),
            tip_axis: crate::supports_ui::route::axis_to_model(world.tip_axis, &to_model),
            bends: world.bends.iter().map(|&b| to_model.point(b)).collect(),
            base: match world.base {
                BaseSpec::Model { point_mm, axis } => BaseSpec::Model {
                    point_mm: to_model.point(point_mm),
                    axis: crate::supports_ui::route::axis_to_model(axis, &to_model),
                },
                other => other,
            },
            parameters: world.parameters,
            accept_collision: false,
            origin: karasu_core::supports::SupportOrigin::Placed,
            kind: world.kind,
        });
        workspace.set_supports(index, Some(document), None);
        Some(id)
    }

    /// How part `part` of entity `entity` of object `index`'s build shows (see [`crate::shown`]),
    /// `wrong` its build's entities with errors: in the Manual tool every support, brace and
    /// raft by the tool's Show and list filter; in the Braces tool its braces by its own; the
    /// rest solid.
    fn seen(
        &self,
        editing: Option<Editing>,
        index: usize,
        part: SupportPart,
        entity: u64,
        wrong: &[u64],
    ) -> crate::shown::Seen {
        let wrong = wrong.binary_search(&entity).is_ok();
        match editing {
            Some(Editing::Supports) => {
                // Looked up only when a filter asks: it searches the build's supports.
                let minis = self.shown == crate::shown::Shown::Minis
                    || (self.list_open && self.list_filter == list::ListFilter::Minis);
                let mini = minis && self.is_mini(index, entity);
                crate::shown::seen(
                    self.shown,
                    self.list_filter.listed(self.list_open, wrong, mini),
                    wrong,
                    self.selected == Some((index, entity)),
                    mini,
                )
            }
            Some(Editing::Braces) if part == SupportPart::Brace => crate::shown::seen(
                self.bracing.shown,
                (self.bracing.list_open && self.bracing.only_errors).then_some(wrong),
                wrong,
                self.chosen_brace() == Some((index, entity)),
                false,
            ),
            _ => crate::shown::Seen::Solid,
        }
    }

    /// Whether entity `entity` of object `index`'s build is a mini support.
    fn is_mini(&self, index: usize, entity: u64) -> bool {
        self.builds
            .get(&index)
            .and_then(|b| b.scene.as_ref().ok())
            .is_some_and(|scene| {
                scene
                    .supports()
                    .iter()
                    .any(|s| s.id == entity && s.kind == SupportKind::Mini)
            })
    }

    /// The support of object `index`'s build that `ray` meets first.
    fn pick(&self, ray: &Ray, index: usize) -> Option<u64> {
        let built = self.builds.get(&index)?;
        let scene = built.scene.as_ref().ok()?;
        let support = |entity: u64| scene.supports().iter().any(|s| s.id == entity);
        // Hidden and ghost supports are not picked: what shows is.
        let wrong = errors(scene.findings());
        // Below the plate, the plate bases and rafts fade out: what is left is picked.
        let bases_out = self.base_hidden > 0.0;
        let filtered = self.shown != crate::shown::Shown::All
            || (self.list_open && self.list_filter != list::ListFilter::All)
            || bases_out;
        if filtered {
            return braces::nearest(scene, ray, |part, entity| {
                support(entity)
                    && (!self.tips_only || part == SupportPart::Contact)
                    && !(bases_out && on_plate(part))
                    && self
                        .seen(Some(Editing::Supports), index, part, entity, &wrong)
                        .pickable()
            })
            .map(|(_, entity)| entity);
        }
        if self.tips_only {
            // Only what shows: the tips.
            let mesh = scene.mesh();
            let corner = |v: u32| mesh.vertices().get(v as usize).map(|p| [p.x, p.y, p.z]);
            return scene
                .part_ranges()
                .iter()
                .filter(|r| r.part == SupportPart::Contact && support(r.entity))
                .flat_map(|r| {
                    mesh.triangles()[r.triangles.clone()]
                        .iter()
                        .filter_map(move |t| {
                            let corners = [corner(t[0])?, corner(t[1])?, corner(t[2])?];
                            ray_triangle(ray.origin, ray.direction, corners).map(|d| (d, r.entity))
                        })
                })
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, entity)| entity);
        }
        let (_, triangle) = ray_mesh(ray, &IDENTITY, scene.mesh())?;
        scene
            .part_ranges()
            .iter()
            .find(|r| r.triangles.contains(&triangle))
            .map(|r| r.entity)
            .filter(|&entity| support(entity))
    }

    /// What object `index`'s supports are built from now, if it has supports and a preparation
    /// that still describes its surface.
    fn input(
        workspace: &Workspace,
        preparation: &PreparationUi,
        index: usize,
    ) -> Option<(Input, Arc<Preparation>)> {
        let document = workspace
            .supports(index)
            .filter(|d| !d.supports.is_empty() || d.raft.is_some())?;
        let placement = workspace.placement_of(index)?.transform;
        let (shared, generation) = preparation.shared(workspace, index)?;
        Some((
            Input {
                document: document.clone(),
                placement,
                generation,
            },
            shared,
        ))
    }

    /// Keep every object's built supports current, the `selected` object's first: take a
    /// finished build, drop an outdated one, and start the next. Returns whether a repaint is
    /// needed soon.
    pub fn poll(
        &mut self,
        workspace: &Workspace,
        preparation: &PreparationUi,
        selected: Option<usize>,
    ) -> bool {
        let routing = self.poll_route(workspace, preparation, selected);
        let planning = self.poll_auto(workspace, preparation, selected);
        let merging = self.poll_merging(workspace, preparation);
        self.poll_builds(workspace, preparation, selected) || routing || planning || merging
    }

    /// Keep every object's builds current: see [`Self::poll`].
    fn poll_builds(
        &mut self,
        workspace: &Workspace,
        preparation: &PreparationUi,
        selected: Option<usize>,
    ) -> bool {
        let count = workspace.plate().objects.len();
        if count != self.objects {
            self.objects = count;
            self.job = None;
            self.retired
                .extend(self.builds.keys().map(|&i| scene_key(i)));
            self.builds.clear();
            // A support of a model no longer on the plate is no longer selected.
            self.selected = self.selected.filter(|&(object, _)| object < count);
        }
        if let Some(job) = &self.job {
            match job.receiver.try_recv() {
                Ok(scene) => {
                    let origin = scene.as_ref().map_or([0.0; 3], |s| centre(s.mesh()));
                    let back = affine_of(&job.input.placement).and_then(|a| a.inverse());
                    let object = job.object;
                    let input = job.input.clone();
                    self.job = None;
                    self.builds.insert(
                        object,
                        Built {
                            input,
                            scene,
                            origin,
                            back,
                            uploaded: false,
                        },
                    );
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => self.job = None,
            }
        }
        // Objects without supports keep no build.
        let gone: Vec<usize> = self
            .builds
            .keys()
            .copied()
            .filter(|&i| {
                workspace
                    .supports(i)
                    .is_none_or(|d| d.supports.is_empty() && d.raft.is_none())
            })
            .collect();
        for i in gone {
            self.builds.remove(&i);
            self.retired.push(scene_key(i));
        }
        if let Some(job) = &self.job {
            let outdated = workspace
                .supports(job.object)
                .is_none_or(|d| *d != job.input.document);
            if !outdated {
                return true;
            }
            self.job = None;
        }
        let order = selected
            .into_iter()
            .chain((0..count).filter(|&i| Some(i) != selected));
        for index in order {
            let Some((input, shared)) = Self::input(workspace, preparation, index) else {
                continue;
            };
            if self.builds.get(&index).is_some_and(|b| b.input == input) {
                continue;
            }
            self.job = start(workspace, index, input, shared);
            return self.job.is_some();
        }
        false
    }

    /// Upload each new build's mesh once, the preview each time it moves, and free the meshes
    /// of builds that are gone.
    pub fn sync(&mut self, viewport: &mut Viewport, state: &egui_wgpu::RenderState) {
        for key in self.retired.drain(..) {
            viewport.remove(key);
        }
        self.sync_braces(viewport, state);
        let at = self.preview.as_ref().map(|p| p.stamp);
        if at != self.uploaded_preview {
            viewport.remove(PREVIEW_KEY);
            if let Some(preview) = &self.preview {
                viewport.upload(
                    &state.device,
                    PREVIEW_KEY,
                    preview.scene.mesh(),
                    preview.origin,
                );
            }
            self.uploaded_preview = at;
        }
        for (&index, built) in self.builds.iter_mut().filter(|(_, b)| !b.uploaded) {
            built.uploaded = true;
            viewport.remove(scene_key(index));
            if let Ok(scene) = &built.scene {
                if !scene.mesh().triangles().is_empty() {
                    viewport.upload(&state.device, scene_key(index), scene.mesh(), built.origin);
                }
            }
        }
    }

    /// The support a click would add, or the one being dragged, while the tool `edits`: whole,
    /// or its tip alone where only tips show.
    fn preview_draws(&self, edits: bool, appearance: &Appearance) -> Vec<Draw> {
        let mut draws = Vec::new();
        let previewing = (self.placing || self.drag.is_some()) && edits;
        if let Some(preview) = self.preview.as_ref().filter(|_| previewing) {
            let colour = if preview.error {
                appearance.support_error
            } else {
                appearance.hole_valid
            };
            // With only the tips shown, the support a click would add shows only its tip too
            // (owner, 2026-10-29); a foot being placed or a handle dragged shows whole.
            // So does a branch's, before its tip is clicked: the shaft it joins comes next.
            let tip_alone = self.tips_only || (self.branching && self.branch_tip.is_none());
            if tip_alone && self.placing && self.drag.is_none() {
                for part in preview
                    .scene
                    .part_ranges()
                    .iter()
                    .filter(|r| r.part == SupportPart::Contact)
                {
                    let range = [part.triangles.start, part.triangles.len()]
                        .map(|n| u32::try_from(n).unwrap_or(u32::MAX));
                    draws.push(support_draw(
                        PREVIEW_KEY,
                        translation(preview.origin),
                        colour.linear(),
                        Some(range),
                    ));
                }
            } else {
                draws.push(support_draw(
                    PREVIEW_KEY,
                    translation(preview.origin),
                    colour.linear(),
                    preview.range(),
                ));
            }
        }
        draws
    }

    /// Every object's supports as built, each support and brace in its colour: red with an
    /// error, the selected one lighter while a tool is `editing` the `selected` model, and the
    /// previews while placing or dragging. A build whose object moved is carried to where the
    /// object is until it is built again.
    #[must_use]
    pub fn draws(
        &self,
        workspace: &Workspace,
        selected: Option<usize>,
        editing: Option<Editing>,
        appearance: &Appearance,
    ) -> Vec<Draw> {
        let mut draws: Vec<Draw> = Vec::new();
        let edits = editing == Some(Editing::Supports);
        if editing == Some(Editing::Braces) {
            draws.extend(self.brace_draws(appearance));
        }
        draws.extend(self.preview_draws(edits, appearance));
        let mut indices: Vec<usize> = self.builds.keys().copied().collect();
        indices.sort_unstable();
        for index in indices {
            let built = &self.builds[&index];
            let Ok(scene) = &built.scene else {
                continue;
            };
            if workspace.is_hidden(index) {
                continue;
            }
            let carried = match (built.back, workspace.model_to_world(index)) {
                (Some(back), Some(forward)) => carried(&back, &forward),
                _ => continue,
            };
            let model = carried.mul(&translation(built.origin));
            let wrong = errors(scene.findings());
            let chosen = match editing {
                Some(Editing::Supports) => self.selected,
                Some(Editing::Braces) => self.chosen_brace(),
                None => None,
            }
            .filter(|&(object, _)| object == index && Some(object) == selected);
            let paint = |entity: u64| -> [f32; 4] {
                // What Merge trunks would change shows as selected.
                let marked =
                    chosen == Some((index, entity)) || (edits && self.merge_marks(index, entity));
                let rgba = match (wrong.contains(&entity), marked) {
                    (true, true) => appearance.support_error_selected,
                    (true, false) => appearance.support_error,
                    (false, true) => appearance.support_selected,
                    (false, false) => appearance.support,
                };
                rgba.linear()
            };
            let first_draw = draws.len();
            let hidden = self
                .dragged_support()
                .or_else(|| self.dragged_brace())
                .filter(|&(object, _)| object == index);
            let tips = self.tips_only && edits;
            let editing = editing.filter(|_| Some(index) == selected);
            for part in scene.part_ranges() {
                // A support being dragged shows as its preview instead; with only the tips
                // shown, the rest of each support and the rafts are left out.
                if hidden.is_some_and(|(_, id)| id == part.entity)
                    || (tips && part.part != SupportPart::Contact)
                {
                    continue;
                }
                // The strip's Show hides; the list's With errors leaves ghosts (owner,
                // 2026-10-31).
                let seen = self.seen(editing, index, part.part, part.entity, &wrong);
                let (mut color, mut look, mut cap) = match seen {
                    crate::shown::Seen::Hidden => continue,
                    crate::shown::Seen::Ghost => (
                        appearance.filtered_ghost.linear(),
                        crate::viewport::Look::Ghost,
                        crate::viewport::Cap::Off,
                    ),
                    crate::shown::Seen::Solid => (
                        paint(part.entity),
                        crate::viewport::Look::Solid,
                        crate::viewport::Cap::Support,
                    ),
                };
                // Seen from below the plate, the plate bases and rafts fade out with it: as
                // ghosts on the way, then left out (owner R9, 2026-11-10).
                if self.base_hidden > 0.0 && on_plate(part.part) {
                    if self.base_hidden >= 1.0 {
                        continue;
                    }
                    color[3] *= 1.0 - self.base_hidden;
                    look = crate::viewport::Look::Ghost;
                    cap = crate::viewport::Cap::Off;
                }
                let [first, count] = [part.triangles.start, part.triangles.len()]
                    .map(|n| u32::try_from(n).unwrap_or(u32::MAX));
                // Consecutive parts of one colour and look are one draw (colours compare
                // exactly: they come from the same few appearance colours).
                #[allow(clippy::float_cmp)]
                if let Some(last) = draws[first_draw..]
                    .last_mut()
                    .filter(|d| d.color == color && d.look == look)
                {
                    if let Some([start, n]) = last.triangles.filter(|[s, n]| s + n == first) {
                        last.triangles = Some([start, n + count]);
                        continue;
                    }
                }
                draws.push(Draw {
                    cap,
                    look,
                    ..support_draw(scene_key(index), model, color, Some([first, count]))
                });
            }
        }
        draws
    }

    /// The status bar's word on the supports: how many have errors, which print as they are.
    #[must_use]
    pub fn statuses(&self) -> Vec<Status> {
        let errors: usize = self
            .builds
            .values()
            .filter_map(|b| b.scene.as_ref().ok())
            .map(|s| errors(s.findings()).len())
            .sum();
        if errors == 0 {
            return Vec::new();
        }
        vec![Status::new(
            tr!("supports-errors-status", count = errors),
            theme::WARNING.color(),
        )]
    }

    /// Whether no build is running and none is left to start: every object whose supports can
    /// be built has its current build. A project opened waits for it (TD-130). Asked only while
    /// one opens: it compares each object's input.
    #[must_use]
    pub fn built(&self, workspace: &Workspace, preparation: &PreparationUi) -> bool {
        self.job.is_none()
            && (0..workspace.plate().objects.len()).all(|i| {
                Self::input(workspace, preparation, i)
                    .is_none_or(|(input, _)| self.builds.get(&i).is_some_and(|b| b.input == input))
            })
    }

    /// Object `index`'s rafts as last built: how many, and the width and depth in mm of what
    /// they cover together, if any was built.
    #[must_use]
    pub fn raft_built(&self, index: usize) -> Option<(usize, [f64; 2])> {
        let scene = self.builds.get(&index)?.scene.as_ref().ok()?;
        let mesh = scene.mesh();
        let (mut low, mut high) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        let rafts: Vec<_> = scene
            .part_ranges()
            .iter()
            .filter(|r| r.part == karasu_core::supports::SupportPart::Raft)
            .collect();
        for range in &rafts {
            for triangle in &mesh.triangles()[range.triangles.clone()] {
                for &v in triangle {
                    let p = mesh.vertices()[v as usize];
                    for (k, c) in [p.x, p.y].into_iter().enumerate() {
                        low[k] = low[k].min(c);
                        high[k] = high[k].max(c);
                    }
                }
            }
        }
        // A web's discs and bars count as the pieces they make (TD-144).
        (low[0] <= high[0]).then(|| (scene.rafts(), [high[0] - low[0], high[1] - low[1]]))
    }

    /// The findings of support `id` in the latest build of object `index`.
    /// The contact's diameter support `id` of model `index` was last built with, if built.
    fn built_contact(&self, index: usize, id: u64) -> Option<f64> {
        let scene = self.builds.get(&index)?.scene.as_ref().ok()?;
        scene
            .supports()
            .iter()
            .find(|s| s.id == id)?
            .contact_diameter_mm
    }

    fn findings(&self, index: usize, id: u64) -> Vec<SupportFinding> {
        self.builds
            .get(&index)
            .and_then(|b| b.scene.as_ref().ok())
            .map_or_else(Vec::new, |scene| {
                scene
                    .findings()
                    .iter()
                    .filter(|f| f.entity == id)
                    .cloned()
                    .collect()
            })
    }
}

/// Whether `part` lies on the plate: a support's pad and the cone above it, and a raft. They
/// fade with the plate while the view is below it, and cannot be picked then.
const fn on_plate(part: SupportPart) -> bool {
    matches!(
        part,
        SupportPart::Pad | SupportPart::Cone | SupportPart::Raft
    )
}

/// One draw of supports: capped by the section in their own colour, never cut by holes.
fn support_draw(mesh: u64, model: Mat4, color: [f32; 4], triangles: Option<[u32; 2]>) -> Draw {
    Draw {
        mesh,
        model,
        color,
        overlay: false,
        cap: crate::viewport::Cap::Support,
        holes: [0, 0],
        bore: None,
        outside: None,
        triangles,
        look: crate::viewport::Look::Solid,
    }
}

/// The map of `placement`.
fn affine_of(placement: &ObjectTransform) -> Option<Affine> {
    let affine = placement.to_affine().ok()?;
    Some(Affine {
        linear: affine.linear(),
        translation: affine.translation().components(),
    })
}

/// Start building `input`, object `index`'s supports in its model frame, against its
/// preparation in `shared`, on a thread of its own.
fn start(
    workspace: &Workspace,
    index: usize,
    input: Input,
    shared: Arc<Preparation>,
) -> Option<Job> {
    let object = workspace.plate().objects.get(index)?;
    let mesh = Arc::clone(&workspace.meshes().get(&object.model)?.mesh);
    let printer = workspace.printer()?.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let (sender, receiver) = mpsc::channel();
    let cancelled = Arc::clone(&stop);
    let document = input.document.clone();
    let placement = input.placement;
    let spawned = std::thread::Builder::new()
        .name("supports".to_owned())
        .spawn(move || {
            let Some(Ok(prepared)) = shared.objects.get(index) else {
                let _ = sender.send(Err(tr!("supports-unprepared")));
                return;
            };
            let started = Instant::now();
            let result =
                build_model_supports(&document, &mesh, &placement, prepared, &printer, &|_| {
                    cancelled.load(Ordering::Relaxed)
                });
            if let Ok(scene) = &result {
                log::debug!(
                    target: "supports",
                    "built {} supports ({} parts, {} findings) in {:.0} ms",
                    scene.supports().len(),
                    scene.parts(),
                    scene.findings().len(),
                    started.elapsed().as_secs_f64() * 1000.0
                );
            }
            let _ = sender.send(result);
        });
    match spawned {
        Ok(_) => Some(Job {
            object: index,
            input,
            stop,
            receiver,
        }),
        Err(error) => {
            log::warn!(target: "supports", "cannot start building the supports: {error}");
            None
        }
    }
}

/// The support `next` makes with its tip at `hit`, `routed` clear of the model: see [`alone`].
fn preview(
    workspace: &Workspace,
    hit: &SurfaceHit,
    next: NextSupport,
    routed: route::Routed,
) -> Option<Preview> {
    let scene = alone(workspace, routed.support, &routed.anchors, None)?;
    Some(Preview::of(scene, hit.point, next))
}

/// `support`, in world coordinates, built against a small triangle of the surface at each of
/// its `anchors` (its tip's, and its foot's on the model: point and outward normal): its shape,
/// and what it shows alone (the build volume, a path to its foot). The whole surface is checked
/// once it is added or moved; this one is only shown, so it stays cheap enough to follow the
/// pointer (about 0.05 ms).
fn alone(
    workspace: &Workspace,
    mut support: ManualSupport,
    anchors: &[([f64; 3], [f64; 3])],
    junction: Option<[f64; 3]>,
) -> Option<SupportScene> {
    let printer = workspace.printer()?;
    let p = &support.parameters;
    let radius = p
        .contact_sphere_diameter_mm
        .max(p.tip_body_diameter_mm)
        .max(p.foot_body_diameter_mm)
        + 1.0;
    let mut points = Vec::new();
    let mut triangles = Vec::new();
    for &(point, normal) in anchors {
        let one = tangent_triangle(point, normal, radius)?;
        let base = u32::try_from(points.len()).ok()?;
        points.extend_from_slice(one.vertices());
        triangles.extend(one.triangles().iter().map(|t| t.map(|v| v + base)));
    }
    let mut document = SupportDocument::new(Preset::Medium);
    // A branch joins a stand-in for its support: a vertical one through the junction.
    if let Some(at) = junction {
        let p = &support.parameters;
        let tip = [at[0], at[1], at[2] + p.tip_length_mm + 1.0];
        let one = tangent_triangle(tip, [0.0, 0.0, -1.0], radius)?;
        let base = u32::try_from(points.len()).ok()?;
        points.extend_from_slice(one.vertices());
        triangles.extend(one.triangles().iter().map(|t| t.map(|v| v + base)));
        document.supports.push(ManualSupport {
            id: STAND_IN,
            tip_mm: tip,
            tip_axis: AxisSpec::Normal,
            bends: Vec::new(),
            base: BaseSpec::Vertical,
            parameters: p.clone(),
            accept_collision: true,
            origin: SupportOrigin::Placed,
            kind: karasu_core::supports::SupportKind::Standard,
        });
        support.base = BaseSpec::Support {
            parent: STAND_IN,
            z_mm: at[2],
        };
    }
    let surface = TriangleMesh::try_new(points, triangles).ok()?;
    document.supports.push(support);
    let scene = build_supports(
        &document,
        SupportSurface {
            mesh: &surface,
            interior_from: None,
        },
        &[],
        BuildVolume {
            origin_xy_mm: printer.origin_xy_mm,
            size_mm: printer.build_size_mm,
        },
        SupportOptions::default(),
        &mut Cancellation::never(),
    )
    .ok()?;
    (!scene.mesh().triangles().is_empty()).then_some(scene)
}

/// An equilateral triangle of circumradius `radius` centred on `point`, in the plane normal to
/// `normal` and facing along it: the surface where a support's tip touches, for its preview.
pub(crate) fn tangent_triangle(
    point: [f64; 3],
    normal: [f64; 3],
    radius: f64,
) -> Option<TriangleMesh> {
    let length = normal.iter().map(|v| v * v).sum::<f64>().sqrt();
    if !(length.is_finite() && length > 0.0) {
        return None;
    }
    let n = normal.map(|v| v / length);
    // A direction across the normal: the axis it is least along.
    let other = if n[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let u = cross(n, other);
    let u_length = u.iter().map(|v| v * v).sum::<f64>().sqrt();
    let u = u.map(|v| v / u_length);
    let v = cross(n, u);
    let corner = |angle_deg: f64| {
        let (sin, cos) = angle_deg.to_radians().sin_cos();
        Point3::new(
            point[0] + radius * (cos * u[0] + sin * v[0]),
            point[1] + radius * (cos * u[1] + sin * v[1]),
            point[2] + radius * (cos * u[2] + sin * v[2]),
        )
    };
    // Counter-clockwise about the normal (u, then v = n × u): the face looks along `normal`.
    let corners = vec![corner(0.0), corner(120.0), corner(240.0)];
    TriangleMesh::try_new(corners, vec![[0, 1, 2]]).ok()
}

/// The entities with an error finding.
fn errors(findings: &[SupportFinding]) -> Vec<u64> {
    let mut entities: Vec<u64> = findings
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .map(|f| f.entity)
        .collect();
    entities.sort_unstable();
    entities.dedup();
    entities
}

/// A finding in the interface's words, with the core's numbers.
fn describe(finding: &SupportFinding) -> String {
    let what = match finding.kind {
        FindingKind::SoftRange => tr!("supports-finding-soft"),
        FindingKind::AnchorOffSurface => tr!("supports-finding-off-surface"),
        FindingKind::AxisDeviation => tr!("supports-finding-axis"),
        FindingKind::Piercing => tr!("supports-finding-piercing"),
        FindingKind::ContactFit => tr!("supports-finding-contact-fit"),
        FindingKind::Collision if finding.severity == Severity::Accepted => {
            tr!("supports-finding-accepted")
        }
        FindingKind::Collision => tr!("supports-finding-collision"),
        FindingKind::DrainHole => tr!("supports-finding-drain-hole"),
        FindingKind::FootInHole => tr!("supports-finding-foot-in-hole"),
        FindingKind::BuildVolume => tr!("supports-finding-volume"),
        FindingKind::InvalidPath => tr!("supports-finding-path"),
        FindingKind::SteepShaft => tr!("supports-finding-steep"),
        FindingKind::InvalidParent => tr!("supports-finding-parent"),
        FindingKind::EmptyRaft => tr!("supports-finding-raft"),
    };
    format!("{what} ({})", finding.detail)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::hollowing_ui::tests::{cube_file, placed, prepared_within};
    use std::time::Duration;

    /// A 20 mm cube lifted 5 mm above the plate, as before supports are placed, prepared.
    pub(crate) fn lifted_cube() -> (Workspace, PreparationUi, std::path::PathBuf) {
        let (path, directory) = cube_file("supports");
        let (mut workspace, mut preparation) = placed(path, None, Duration::from_secs(60));
        workspace.edit(|plate| {
            plate.objects[0].pose.drop = true;
            plate.objects[0].pose.lift_mm = Some(5.0);
        });
        prepared_within(&workspace, &mut preparation, Duration::from_secs(60));
        (workspace, preparation, directory)
    }

    #[test]
    fn with_only_the_tips_shown_the_rest_is_neither_drawn_nor_picked() {
        let (mut workspace, preparation, directory) = lifted_cube();
        let mut ui = SupportsUi {
            placing: true,
            ..SupportsUi::default()
        };
        let placed = workspace.placement_of(0).unwrap();
        let [cx, cy] =
            [0, 1].map(|k| f64::midpoint(placed.min.coordinates()[k], placed.max.coordinates()[k]));
        let under = ray(cx, cy, true);
        let hit = workspace.hit(&under);
        assert!(ui.click(&mut workspace, &under, hit, 0));
        ui.placing = false;
        ui.selected = None;
        built(&mut ui, &workspace, &preparation);
        let appearance = Appearance::default();
        let drawn = |ui: &SupportsUi| -> u32 {
            ui.draws(&workspace, Some(0), Some(Editing::Supports), &appearance)
                .iter()
                .filter_map(|d| d.triangles.map(|[_, n]| n))
                .sum()
        };
        let scene = ui.builds[&0].scene.as_ref().unwrap();
        let tips: usize = scene
            .part_ranges()
            .iter()
            .filter(|r| r.part == SupportPart::Contact)
            .map(|r| r.triangles.len())
            .sum();
        // Through the shaft's middle, from the side.
        let zs: Vec<f64> = scene
            .part_ranges()
            .iter()
            .filter(|r| r.part == SupportPart::Shaft)
            .flat_map(|r| {
                scene.mesh().triangles()[r.triangles.clone()]
                    .iter()
                    .flatten()
            })
            .map(|&v| scene.mesh().vertices()[v as usize].z)
            .collect();
        let middle = f64::midpoint(
            zs.iter().copied().fold(f64::INFINITY, f64::min),
            zs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        );
        let all = drawn(&ui);
        let shaft = Ray {
            origin: [cx - 40.0, cy, middle],
            direction: [1.0, 0.0, 0.0],
        };
        assert!(ui.pick(&shaft, 0).is_some());
        ui.tips_only = true;
        assert_eq!(drawn(&ui) as usize, tips);
        assert!((drawn(&ui)) < all);
        assert!(ui.pick(&shaft, 0).is_none());
        // Its tip is still picked, from below.
        let tip = Ray {
            origin: [cx, cy, 4.9],
            direction: [0.0, 0.0, 1.0],
        };
        assert!(ui.pick(&tip, 0).is_some());
        // The support a click would add shows only its tip as well.
        ui.placing = true;
        let below = ray(cx + 3.0, cy + 3.0, true);
        let hit = workspace.hit(&below);
        ui.hover(&workspace, hit, 0);
        let preview = ui.preview.as_ref().expect("a preview");
        let preview_tip: usize = preview
            .scene
            .part_ranges()
            .iter()
            .filter(|r| r.part == SupportPart::Contact)
            .map(|r| r.triangles.len())
            .sum();
        assert!(preview_tip < preview.scene.mesh().triangles().len());
        let shown: u32 = ui
            .draws(&workspace, Some(0), Some(Editing::Supports), &appearance)
            .iter()
            .filter(|d| d.mesh == PREVIEW_KEY)
            .map(|d| d.triangles.map_or(u32::MAX, |[_, n]| n))
            .sum();
        assert_eq!(shown as usize, preview_tip);
        ui.placing = false;
        // Only while the tool is in use: elsewhere the supports show whole.
        let elsewhere: u32 = ui
            .draws(&workspace, Some(0), None, &appearance)
            .iter()
            .filter_map(|d| d.triangles.map(|[_, n]| n))
            .sum();
        assert_eq!(elsewhere, all);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn seen_from_below_the_plate_the_bases_fade_out_and_are_not_picked() {
        use crate::viewport::Look;
        let (mut workspace, preparation, directory) = lifted_cube();
        let mut ui = SupportsUi {
            placing: true,
            ..SupportsUi::default()
        };
        let placed = workspace.placement_of(0).unwrap();
        let [cx, cy] =
            [0, 1].map(|k| f64::midpoint(placed.min.coordinates()[k], placed.max.coordinates()[k]));
        let under = ray(cx, cy, true);
        let hit = workspace.hit(&under);
        assert!(ui.click(&mut workspace, &under, hit, 0));
        ui.placing = false;
        ui.selected = None;
        built(&mut ui, &workspace, &preparation);
        let appearance = Appearance::default();
        let scene = ui.builds[&0].scene.as_ref().unwrap();
        let count = |keep: fn(SupportPart) -> bool| -> usize {
            scene
                .part_ranges()
                .iter()
                .filter(|r| keep(r.part))
                .map(|r| r.triangles.len())
                .sum()
        };
        let bases = count(on_plate);
        assert!(bases > 0);
        let all = count(|_| true);
        let drawn = |ui: &SupportsUi, look: Look| -> usize {
            ui.draws(&workspace, Some(0), None, &appearance)
                .iter()
                .filter(|d| d.look == look)
                .filter_map(|d| d.triangles.map(|[_, n]| n as usize))
                .sum()
        };
        // From above, whole and solid.
        assert_eq!((drawn(&ui, Look::Solid), drawn(&ui, Look::Ghost)), (all, 0));
        // On the way below, the bases are fading ghosts.
        ui.base_hidden = 0.5;
        assert_eq!(
            (drawn(&ui, Look::Solid), drawn(&ui, Look::Ghost)),
            (all - bases, bases)
        );
        let ghost = ui
            .draws(&workspace, Some(0), None, &appearance)
            .into_iter()
            .find(|d| d.look == Look::Ghost)
            .unwrap();
        assert!((ghost.color[3] - 0.5).abs() < 1e-6);
        assert_eq!(ghost.cap, crate::viewport::Cap::Off);
        // Below, gone: the shafts and tips are left.
        ui.base_hidden = 1.0;
        assert_eq!(
            (drawn(&ui, Look::Solid), drawn(&ui, Look::Ghost)),
            (all - bases, 0)
        );
        // A pad is no longer picked from below; up through it, the tip still is.
        let pad = Ray {
            origin: [cx, cy, -10.0],
            direction: [0.0, 0.0, 1.0],
        };
        ui.base_hidden = 0.0;
        let through = ui.pick(&pad, 0);
        assert!(through.is_some());
        let side = Ray {
            origin: [cx - 40.0, cy, 0.2],
            direction: [1.0, 0.0, 0.0],
        };
        assert!(ui.pick(&side, 0).is_some());
        ui.base_hidden = 1.0;
        assert!(ui.pick(&side, 0).is_none());
        assert_eq!(ui.pick(&pad, 0), through);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn show_hides_the_others_and_the_list_s_filter_leaves_them_as_ghosts() {
        use crate::shown::Shown;
        use crate::viewport::Look;
        let (mut workspace, preparation, directory) = lifted_cube();
        let mut ui = SupportsUi {
            placing: true,
            ..SupportsUi::default()
        };
        let placed = workspace.placement_of(0).unwrap();
        let [cx, cy] =
            [0, 1].map(|k| f64::midpoint(placed.min.coordinates()[k], placed.max.coordinates()[k]));
        let [good, bad] = [ray(cx, cy, true), ray(cx + 3.0, cy + 3.0, true)];
        for under in [good, bad] {
            let hit = workspace.hit(&under);
            assert!(ui.click(&mut workspace, &under, hit, 0));
        }
        // The second support's tip turned past the axis limit: an error, still built.
        let mut document = workspace.supports(0).unwrap().clone();
        document.supports[1].tip_axis = AxisSpec::Direction([1.0, 0.0, -0.1]);
        let [one, two] = [document.supports[0].id, document.supports[1].id];
        workspace.set_supports(0, Some(document), None);
        ui.placing = false;
        ui.selected = None;
        built(&mut ui, &workspace, &preparation);
        let scene = ui.builds[&0].scene.as_ref().unwrap();
        assert_eq!(errors(scene.findings()), [two]);
        let size = |id: u64| -> u32 {
            scene
                .part_ranges()
                .iter()
                .filter(|r| r.entity == id)
                .map(|r| u32::try_from(r.triangles.len()).unwrap())
                .sum()
        };
        let [first, second] = [size(one), size(two)];
        let appearance = Appearance::default();
        // Triangles drawn solid, and as ghosts.
        let drawn = |ui: &SupportsUi, editing: Option<Editing>| -> [u32; 2] {
            let draws = ui.draws(&workspace, Some(0), editing, &appearance);
            [Look::Solid, Look::Ghost].map(|look| {
                draws
                    .iter()
                    .filter(|d| d.look == look)
                    .filter_map(|d| d.triangles.map(|[_, n]| n))
                    .sum()
            })
        };
        let editing = Some(Editing::Supports);
        assert_eq!(drawn(&ui, editing), [first + second, 0]);
        assert_eq!(ui.pick(&good, 0), Some(one));
        // Only with errors: the other is hidden and cannot be picked.
        ui.shown = Shown::Errors;
        assert_eq!(drawn(&ui, editing), [second, 0]);
        assert_eq!(ui.pick(&good, 0), None);
        assert_eq!(ui.pick(&bad, 0), Some(two));
        // ... but a selected one stays, fixed or not.
        ui.selected = Some((0, one));
        assert_eq!(drawn(&ui, editing), [first + second, 0]);
        // Only the selected one.
        ui.shown = Shown::Selected;
        assert_eq!(drawn(&ui, editing), [first, 0]);
        assert_eq!(ui.pick(&bad, 0), None);
        // Only the mini supports (owner, 2026-11-04): neither is one, so only the selected one
        // stays, and the other cannot be picked.
        assert!(!ui.is_mini(0, one) && !ui.is_mini(0, two));
        ui.shown = Shown::Minis;
        assert_eq!(drawn(&ui, editing), [first, 0]);
        assert_eq!(ui.pick(&bad, 0), None);
        // The list's With errors, with every support shown: the other is a ghost, not picked.
        ui.shown = Shown::All;
        ui.selected = None;
        ui.list_open = true;
        ui.list_filter = list::ListFilter::Errors;
        assert_eq!(drawn(&ui, editing), [second, first]);
        assert_eq!(ui.pick(&good, 0), None);
        // The list's Mini (owner R8, 2026-11-10): neither is one, so both are ghosts, the
        // wrong one too, and neither is picked.
        ui.list_filter = list::ListFilter::Minis;
        assert_eq!(drawn(&ui, editing), [0, first + second]);
        assert_eq!(ui.pick(&bad, 0), None);
        ui.list_filter = list::ListFilter::Errors;
        // Its list closed, no ghosts.
        ui.list_open = false;
        assert_eq!(drawn(&ui, editing), [first + second, 0]);
        // Only while the tool is in use.
        ui.shown = Shown::Errors;
        assert_eq!(drawn(&ui, None), [first + second, 0]);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn the_cavity_view_places_in_the_cavity_and_gives_back_the_surface_after() {
        let mut ui = SupportsUi::default();
        // From the outside: on, in the cavity; off, outside again.
        ui.toggle_cavity_view();
        assert!(ui.cavity_view && ui.on_cavity);
        ui.toggle_cavity_view();
        assert!(!ui.cavity_view && !ui.on_cavity);
        // From the cavity: it stays there after.
        ui.set_cavity(true);
        ui.toggle_cavity_view();
        ui.toggle_cavity_view();
        assert!(!ui.cavity_view && ui.on_cavity);
        // Choosing the outside ends the view; so does leaving the tool.
        ui.toggle_cavity_view();
        ui.set_cavity(false);
        assert!(!ui.cavity_view && !ui.on_cavity);
        ui.toggle_cavity_view();
        ui.close();
        assert!(!ui.cavity_view);
    }

    /// A ray up from below the plate at (`x`, `y`), or down from above it.
    pub(crate) fn ray(x: f64, y: f64, up: bool) -> Ray {
        Ray {
            origin: [x, y, if up { -50.0 } else { 80.0 }],
            direction: [0.0, 0.0, if up { 1.0 } else { -1.0 }],
        }
    }

    /// Poll until the builds are current, within a minute.
    pub(crate) fn built(ui: &mut SupportsUi, workspace: &Workspace, preparation: &PreparationUi) {
        let started = Instant::now();
        while ui.poll(workspace, preparation, Some(0)) {
            assert!(started.elapsed() < Duration::from_secs(60), "no build");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn supports_are_added_at_once_built_and_drawn_in_their_colours() {
        let (mut workspace, mut preparation, directory) = lifted_cube();
        let mut ui = SupportsUi {
            placing: true,
            ..SupportsUi::default()
        };
        let placed = workspace.placement_of(0).unwrap();
        let [cx, cy] =
            [0, 1].map(|k| f64::midpoint(placed.min.coordinates()[k], placed.max.coordinates()[k]));
        // Under the cube, the support a click would add is shown, built alone in a moment.
        let under = ray(cx, cy, true);
        let hit = workspace.hit(&under);
        let started = Instant::now();
        ui.hover(&workspace, hit, 0);
        let took = started.elapsed();
        let preview = ui.preview.as_ref().expect("a preview under the model");
        assert!(!preview.error);
        assert_eq!(preview.scene.supports().len(), 1);
        eprintln!("support preview built in {took:?}");
        // The cube's top faces up: no support there, and nothing shown.
        let top = ray(cx, cy, false);
        let hit = workspace.hit(&top);
        ui.hover(&workspace, hit, 0);
        assert!(ui.preview.is_none());
        assert!(ui.click(&mut workspace, &top, hit, 0));
        assert_eq!(ui.candidate, Some(Candidate::FacingUp));
        assert!(workspace.supports(0).is_none());
        // Two under its bottom, 5 mm above the plate: each added at once, a step of its own.
        let revision = workspace.revision();
        for dx in [-4.0, 4.0] {
            let under = ray(cx + dx, cy, true);
            let hit = workspace.hit(&under);
            assert!(ui.click(&mut workspace, &under, hit, 0));
        }
        assert_eq!(workspace.supports(0).unwrap().supports.len(), 2);
        assert_eq!(ui.selected, Some((0, 2)));
        // Changing only supports keeps the preparation: nothing is prepared again.
        assert_ne!(workspace.revision(), revision);
        assert!(preparation.current(&workspace).is_some());
        assert!(!preparation.poll(&workspace));
        built(&mut ui, &workspace, &preparation);
        let scene = ui.builds[&0].scene.as_ref().unwrap();
        assert_eq!(scene.supports().len(), 2);
        assert!(!scene.has_errors(), "{:?}", scene.findings());
        assert_eq!(ui.statuses().len(), 0);
        // Drawn in the supports' colour, one draw for both (their parts follow each other)
        // when neither is selected.
        let appearance = Appearance::default();
        ui.selected = None;
        let draws = ui.draws(&workspace, Some(0), Some(Editing::Supports), &appearance);
        assert_eq!(draws.len(), 1);
        assert_eq!(draws[0].color, appearance.support.linear());
        // Not placing, a click on a support selects it, and it is drawn lighter.
        ui.placing = false;
        let under = ray(cx - 4.0, cy, true);
        let hit = workspace.hit(&under);
        assert!(ui.click(&mut workspace, &under, hit, 0));
        assert_eq!(ui.selected, Some((0, 1)));
        let draws = ui.draws(&workspace, Some(0), Some(Editing::Supports), &appearance);
        let selected = appearance.support_selected.linear();
        assert!(draws.iter().any(|d| d
            .color
            .iter()
            .zip(selected)
            .all(|(a, b)| (a - b).abs() < 1e-6)));
        // A support whose tip is in the air is kept and counted, with its finding in the list;
        // it cannot be built, so it is not drawn, and the other one still is.
        workspace.edit(|plate| {
            let document = plate.objects[0].supports.as_mut().unwrap();
            document.supports[1].tip_mm[2] -= 3.0;
        });
        built(&mut ui, &workspace, &preparation);
        assert_eq!(ui.statuses().len(), 1);
        assert!(ui
            .findings(0, 2)
            .iter()
            .any(|f| f.kind == FindingKind::AnchorOffSurface));
        assert_ne!(
            ui.draws(&workspace, Some(0), Some(Editing::Supports), &appearance)
                .len(),
            0
        );
        // Undo takes the edit back; Delete removes the selected support as a step of its own.
        workspace.undo();
        assert!(ui.delete(&mut workspace));
        assert_eq!(workspace.supports(0).unwrap().supports.len(), 1);
        workspace.undo();
        assert_eq!(workspace.supports(0).unwrap().supports.len(), 2);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn the_next_support_keeps_every_value_of_its_weight() {
        for weight in Weight::ALL {
            let next = NextSupport::of(weight);
            assert_eq!(next.parameters(), weight.preset().parameters());
            assert_eq!(next.axis(), AxisSpec::Normal);
            assert!(next.values.is_valid());
        }
    }
}
