//! The Braces tool (owner mockup "Braces tool", 2026-10-29): braces between the selected model's
//! support shafts, kept in its support document with its supports and built with them.
//!
//! **Placed** (the tool strip's Place): a click on a shaft sets a brace's first end at the
//! height clicked, and a click on another shaft adds the brace there at once, as one undoable
//! step. In a chain each click adds a brace from the shaft clicked before, until Escape; level,
//! the second end takes the first one's height. Brace heights are world values (the support
//! document keeps them so), so a brace stays where it was put when its model moves.
//!
//! **Automatic** (the tool strip's Automatic braces): the braces the panel's values would add
//! between neighbouring supports are planned on a thread of their own and shown, those kept and
//! those skipped, until Apply adds the kept ones as one undoable step, or Discard; see
//! [`karasu_core::supports::plan_braces`].
//!
//! **Changed** by the selected brace's end handles, which slide along their shafts, and by its
//! values in the panel. While an end is dragged the brace shows as a preview in place of its
//! built parts; released, it changes the document as one undoable step. The handles are marks
//! painted over the viewport, as the supports' are.
mod auto;
mod panel;

use super::{centre, support_draw, translation, SupportsUi};
use crate::{
    appearance::Appearance,
    camera::{Camera, Ray},
    gizmo::{along_axis, ray_triangle},
    i18n::tr,
    viewport::{Draw, Viewport},
    workspace::Workspace,
};
use eframe::{egui, egui_wgpu};
use karasu_core::{
    mesh::TriangleMesh,
    supports::{brace_mesh, Brace, BracePattern, BracePlan, SupportPart, SupportScene},
};
use serde::{Deserialize, Serialize};

pub use panel::brace_fields;

/// The viewport mesh key of the brace being placed or dragged.
const PREVIEW_KEY: u64 = 0x4252_4143_0000_0001; // "BRAC"
/// Undo merge keys of edits to one brace's values.
const MERGE_KEYS: u64 = 0x4252_4D00_0000_0000; // "BRM"
/// How far from an end handle a press still takes it, in points.
const HANDLE_REACH: f32 = 10.0;

/// A diameter the document accepts for a brace, in mm.
pub const DIAMETER_MM: std::ops::RangeInclusive<f64> = 0.1..=10.0;
/// The distance in plan within which automatic braces join supports, in mm.
pub const DISTANCE_MM: std::ops::RangeInclusive<f64> = 1.0..=100.0;
/// The lowest automatic braces' height above the plate or the raft, in mm.
pub const LOWEST_MM: std::ops::RangeInclusive<f64> = 0.0..=200.0;
/// The height between automatic braces, in mm.
pub const STEP_MM: std::ops::RangeInclusive<f64> = 0.5..=100.0;

/// How automatic braces join a pair of supports, as the settings keep it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Pattern {
    /// A level brace at each height.
    Horizontal,
    /// One diagonal per step, alternating sides.
    #[default]
    Zigzag,
    /// Both diagonals per step.
    Cross,
}

impl Pattern {
    /// The patterns, in the panel's order.
    pub const ALL: [Self; 3] = [Self::Horizontal, Self::Zigzag, Self::Cross];

    /// The core's pattern.
    #[must_use]
    pub fn core(self) -> BracePattern {
        match self {
            Self::Horizontal => BracePattern::Horizontal,
            Self::Zigzag => BracePattern::Zigzag,
            Self::Cross => BracePattern::Cross,
        }
    }

    /// The pattern's name.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::Horizontal => tr!("braces-pattern-horizontal"),
            Self::Zigzag => tr!("braces-pattern-zigzag"),
            Self::Cross => tr!("braces-pattern-cross"),
        }
    }
}

/// What new braces are made with: the diameter of every new brace, and where automatic ones go
/// (owner's mockup values). Tool values: they start from the settings each time the tool opens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NewBraces {
    /// Each new brace is as thick as the thinner of its two shafts where it meets them, up to
    /// `diameter_mm` (owner, 2026-11-06, TD-136), and follows them (owner, 2026-10-29);
    /// otherwise it takes `diameter_mm`.
    pub auto_diameter: bool,
    pub diameter_mm: f64,
    pub max_distance_mm: f64,
    pub lowest_mm: f64,
    pub step_mm: f64,
    pub pattern: Pattern,
}

/// The values new braces start from when the settings hold none (owner's mockup).
pub const DEFAULT_BRACES: NewBraces = NewBraces {
    auto_diameter: true,
    diameter_mm: 1.0,
    max_distance_mm: 25.0,
    lowest_mm: 6.0,
    step_mm: 6.0,
    pattern: Pattern::Zigzag,
};

impl Default for NewBraces {
    fn default() -> Self {
        DEFAULT_BRACES
    }
}

impl NewBraces {
    /// These values if every one is in its range, else the built-in ones.
    #[must_use]
    pub fn valid(self) -> Self {
        if DIAMETER_MM.contains(&self.diameter_mm)
            && DISTANCE_MM.contains(&self.max_distance_mm)
            && LOWEST_MM.contains(&self.lowest_mm)
            && STEP_MM.contains(&self.step_mm)
        {
            self
        } else {
            DEFAULT_BRACES
        }
    }

    /// The automatic braces these values plan.
    #[must_use]
    pub fn plan(&self) -> BracePlan {
        BracePlan {
            max_distance_mm: self.max_distance_mm,
            lowest_mm: self.lowest_mm,
            step_mm: self.step_mm,
            pattern: self.pattern.core(),
            diameter_mm: self.diameter(),
            max_diameter_mm: self.largest(),
        }
    }

    /// A new brace's diameter: `None` when automatic.
    #[must_use]
    pub fn diameter(&self) -> Option<f64> {
        (!self.auto_diameter).then_some(self.diameter_mm)
    }

    /// A new automatic brace's largest diameter: `None` when it has a diameter of its own.
    #[must_use]
    pub fn largest(&self) -> Option<f64> {
        self.auto_diameter.then_some(self.diameter_mm)
    }

    /// A new brace `id` between `from` and `to`, of these values' diameter.
    #[must_use]
    pub fn brace(&self, id: u64, from: End, to: End) -> Brace {
        Brace {
            id,
            from,
            to,
            diameter_mm: self.diameter(),
            max_diameter_mm: self.largest(),
        }
    }
}

/// A brace's end on a shaft: the support and the world height.
type End = (u64, f64);

/// A bar drawn as a preview: its mesh about `origin`, and which it is, so it is uploaded once.
struct Bar {
    stamp: u64,
    ends: [[f64; 3]; 2],
    mesh: TriangleMesh,
    origin: [f64; 3],
}

/// The selected brace's end being dragged along its shaft.
struct EndDrag {
    object: usize,
    /// The brace as it would be: the dragged end at its new height.
    brace: Brace,
    /// The end dragged: 0 for `from`, 1 for `to`.
    end: usize,
}

/// The Braces tool's state.
#[derive(Default)]
#[allow(clippy::struct_excessive_bools)] // The tool's switches and modes, each its own.
pub struct Bracing {
    /// Clicks place braces (the tool strip's Place).
    placing: bool,
    /// Each click adds a brace from the shaft clicked before (the Place options, kept with the
    /// settings).
    pub chain: bool,
    /// A brace's second end takes its first end's height (kept with the settings).
    pub level: bool,
    /// The list window is open (kept with the settings).
    pub list_open: bool,
    /// What new braces are made with.
    pub new: NewBraces,
    /// The first end clicked while placing, on model `.0`.
    first: Option<(usize, End)>,
    /// The shaft under the pointer while placing.
    hovered: Option<End>,
    /// The selected brace: its object and identity.
    selected: Option<(usize, u64)>,
    drag: Option<EndDrag>,
    preview: Option<Bar>,
    uploaded: Option<u64>,
    stamp: u64,
    /// The list window shows only the braces with errors; the others are then ghosts.
    pub(super) only_errors: bool,
    /// Which braces the viewport shows (the strip's Show, owner 2026-10-31), kept with the
    /// settings.
    pub shown: crate::shown::Shown,
    /// A brace was selected in the scene: the list scrolls to its row once.
    reveal: bool,
    /// The automatic braces being planned and shown, while that mode is on.
    auto: Option<auto::Auto>,
}

impl Bracing {
    /// The tool as it starts: its list window open, the rest by default.
    #[must_use]
    pub fn new() -> Self {
        Self {
            list_open: true,
            ..Self::default()
        }
    }
}

/// The nearest triangle `ray` meets among `scene`'s parts that `keep` accepts: the distance and
/// the part's entity.
pub(super) fn nearest(
    scene: &SupportScene,
    ray: &Ray,
    keep: impl Fn(SupportPart, u64) -> bool,
) -> Option<(f64, u64)> {
    let mesh = scene.mesh();
    let corner = |v: u32| mesh.vertices().get(v as usize).map(|p| [p.x, p.y, p.z]);
    scene
        .part_ranges()
        .iter()
        .filter(|r| keep(r.part, r.entity))
        .flat_map(|r| {
            mesh.triangles()[r.triangles.clone()]
                .iter()
                .filter_map(move |t| {
                    let corners = [corner(t[0])?, corner(t[1])?, corner(t[2])?];
                    ray_triangle(ray.origin, ray.direction, corners).map(|d| (d, r.entity))
                })
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
}

impl SupportsUi {
    /// Model `index`'s supports as last built.
    pub(crate) fn scene(&self, index: usize) -> Option<&SupportScene> {
        self.builds.get(&index)?.scene.as_ref().ok()
    }

    /// Whether clicks place braces.
    #[must_use]
    pub fn brace_placing(&self) -> bool {
        self.bracing.placing
    }

    /// Start or stop placing braces; it ends the automatic braces.
    pub fn toggle_brace_placing(&mut self) {
        self.bracing.placing = !self.bracing.placing;
        self.bracing.auto = None;
        self.forget_brace_placing();
    }

    /// Place braces in a chain or one by one, and level or as clicked; placing starts.
    pub fn set_brace_options(&mut self, chain: Option<bool>, level: Option<bool>) {
        let b = &mut self.bracing;
        b.chain = chain.unwrap_or(b.chain);
        b.level = level.unwrap_or(b.level);
        b.placing = true;
        b.auto = None;
        self.forget_brace_placing();
    }

    /// Whether the automatic braces show.
    #[must_use]
    pub fn auto_bracing(&self) -> bool {
        self.bracing.auto.is_some()
    }

    /// Show the automatic braces, or leave them without adding any; it ends placing.
    pub fn toggle_auto_braces(&mut self) {
        if self.bracing.auto.take().is_none() {
            self.bracing.auto = Some(auto::Auto::default());
            self.bracing.placing = false;
            self.bracing.drag = None;
            self.forget_brace_placing();
        }
    }

    /// Forget the first end clicked and the bar following the pointer.
    fn forget_brace_placing(&mut self) {
        self.bracing.first = None;
        self.bracing.hovered = None;
        self.bracing.preview = None;
    }

    /// Whether a brace of model `index` is selected, which Delete removes.
    #[must_use]
    pub fn brace_selected(&self, index: Option<usize>) -> bool {
        self.bracing
            .selected
            .is_some_and(|(object, _)| Some(object) == index)
    }

    /// Leave the Braces tool, or its model: nothing placed, dragged, selected or planned.
    pub fn close_braces(&mut self) {
        let b = &mut self.bracing;
        b.placing = false;
        b.drag = None;
        b.selected = None;
        b.auto = None;
        self.forget_brace_placing();
    }

    /// Escape: a drag put back, the first end taken back (a chain ended), placing or the
    /// automatic braces stopped, else the brace released. Returns whether it did any.
    pub(super) fn brace_escape(&mut self) -> bool {
        let b = &mut self.bracing;
        if b.drag.take().is_some() {
            b.preview = None;
            return true;
        }
        if b.first.take().is_some() {
            b.preview = None;
            return true;
        }
        if b.placing {
            b.placing = false;
            self.forget_brace_placing();
            return true;
        }
        if b.auto.take().is_some() {
            return true;
        }
        b.selected.take().is_some()
    }

    /// Delete: remove the selected brace as one undoable step. Returns whether there was one.
    pub(super) fn delete_brace(&mut self, workspace: &mut Workspace) -> bool {
        let Some((object, id)) = self.bracing.selected.take() else {
            return false;
        };
        Self::remove_brace(workspace, object, id);
        true
    }

    /// Remove brace `id` of model `index` as one undoable step.
    fn remove_brace(workspace: &mut Workspace, index: usize, id: u64) {
        if let Some(mut document) = workspace.supports(index).cloned() {
            document.braces.retain(|b| b.id != id);
            workspace.set_supports(index, Some(document), None);
        }
    }

    /// Replace brace `brace.id` of model `index`: as one undoable step, or merged with the next
    /// changes of the same brace (a value dragged in a field) when `merging`.
    fn replace_brace(workspace: &mut Workspace, index: usize, brace: Brace, merging: bool) {
        if let Some(mut document) = workspace.supports(index).cloned() {
            if let Some(b) = document.braces.iter_mut().find(|b| b.id == brace.id) {
                *b = brace;
                let key =
                    merging.then_some(MERGE_KEYS | (index as u64) << 24 | (brace.id & 0xFF_FFFF));
                workspace.set_supports(index, Some(document), key);
            }
        }
    }

    /// The support shaft of model `index`'s build that `ray` meets first, and the height there.
    pub(super) fn shaft_under(&self, ray: &Ray, index: usize) -> Option<End> {
        let scene = self.scene(index)?;
        let support = |id: u64| scene.supports().iter().any(|s| s.id == id);
        let (distance, id) = nearest(scene, ray, |part, entity| {
            part == SupportPart::Shaft && support(entity)
        })?;
        let z = ray.origin[2] + distance * ray.direction[2];
        scene.shaft_point(id, z).map(|_| (id, z))
    }

    /// The brace of model `index`'s build that `ray` meets first.
    fn brace_under(&self, ray: &Ray, index: usize) -> Option<u64> {
        let scene = self.scene(index)?;
        let wrong = super::errors(scene.findings());
        nearest(scene, ray, |part, id| {
            part == SupportPart::Brace
                && self
                    .seen(Some(super::Editing::Braces), index, part, id, &wrong)
                    .pickable()
        })
        .map(|(_, id)| id)
    }

    /// The point of end `end` on model `index`'s built shafts.
    fn end_point(&self, index: usize, (support, z): End) -> Option<[f64; 3]> {
        self.scene(index)?.shaft_point(support, z)
    }

    /// The diameter a brace of `diameter` (`None`: automatic) between `ends` on model `index`'s
    /// built shafts has: an automatic one, the thinner shaft's where it meets it.
    /// Brace `brace` of model `index`'s diameter, as built.
    pub(super) fn brace_diameter(&self, index: usize, brace: &Brace) -> Option<f64> {
        if brace.diameter_mm.is_some() {
            return brace.diameter_mm;
        }
        let scene = self.scene(index)?;
        let [a, b] = [brace.from, brace.to].map(|(support, z)| scene.shaft_diameter(support, z));
        Some(2.0 * brace.radius_mm(a? / 2.0, b? / 2.0))
    }

    /// Show a bar between `ends`, built again only when they change.
    fn show_bar(&mut self, ends: Option<[[f64; 3]; 2]>, diameter_mm: f64) {
        let b = &mut self.bracing;
        #[allow(clippy::float_cmp)] // The same ends exactly: nothing to build again.
        if b.preview.as_ref().map(|p| p.ends) == ends {
            return;
        }
        b.preview = ends.and_then(|ends| {
            let mesh = brace_mesh(&[(ends, diameter_mm)]).ok()?;
            b.stamp += 1;
            Some(Bar {
                stamp: b.stamp,
                ends,
                origin: centre(&mesh),
                mesh,
            })
        });
    }

    /// Follow the pointer's `ray` on model `index`: a dragged end slides along its shaft, and
    /// while placing, the shaft under it is found and the brace a click would add shown.
    pub fn brace_hover(&mut self, ray: Option<&Ray>, index: usize) {
        if self.bracing.drag.is_some() {
            if let Some(ray) = ray {
                self.drag_brace_end(ray);
            }
            return;
        }
        if !self.bracing.placing {
            return;
        }
        let hovered = ray.and_then(|r| self.shaft_under(r, index));
        self.bracing.hovered = hovered;
        let new = self.bracing.new;
        let bar = self
            .bracing
            .first
            .filter(|&(object, _)| object == index)
            .zip(hovered)
            .filter(|((_, (a, _)), (b, _))| a != b)
            .and_then(|((_, first), (support, z))| {
                let second = (support, if self.bracing.level { first.1 } else { z });
                let ends = [
                    self.end_point(index, first)?,
                    self.end_point(index, second)?,
                ];
                let diameter = self.brace_diameter(index, &new.brace(0, first, second))?;
                Some((ends, diameter))
            });
        let diameter = bar.map_or(new.diameter_mm, |(_, d)| d);
        self.show_bar(bar.map(|(ends, _)| ends), diameter);
    }

    /// A click along `ray` on model `index` with the Braces tool: while placing, a shaft clicked
    /// sets the first end, or adds the brace to it (and in a chain starts the next one there);
    /// otherwise the brace under the pointer is selected. Returns whether the click was the
    /// tool's (a click elsewhere selects models as usual).
    pub fn brace_click(&mut self, workspace: &mut Workspace, ray: &Ray, index: usize) -> bool {
        if self.bracing.auto.is_some() {
            return false;
        }
        if !self.bracing.placing {
            let picked = self.brace_under(ray, index);
            if picked.is_some() {
                self.bracing.selected = picked.map(|id| (index, id));
                self.bracing.reveal = true;
                return true;
            }
            return false;
        }
        let Some((support, z)) = self.shaft_under(ray, index) else {
            return true;
        };
        let first = self.bracing.first.filter(|&(object, _)| object == index);
        let Some((_, from)) = first else {
            self.bracing.first = Some((index, (support, z)));
            return true;
        };
        if from.0 == support {
            return true;
        }
        let to = (support, if self.bracing.level { from.1 } else { z });
        let Some(mut document) = workspace.supports(index).cloned() else {
            return true;
        };
        let id = Self::next_id(workspace, index);
        document.braces.push(self.bracing.new.brace(id, from, to));
        workspace.set_supports(index, Some(document), None);
        self.bracing.selected = Some((index, id));
        self.bracing.reveal = true;
        self.bracing.first = self.bracing.chain.then_some((index, to));
        self.bracing.preview = None;
        true
    }

    /// The selected brace of model `index`'s end handles, in world coordinates, as built.
    fn brace_handles(&self, workspace: &Workspace, index: usize) -> Vec<[f64; 3]> {
        let Some((object, id)) = self.bracing.selected.filter(|&(o, _)| o == index) else {
            return Vec::new();
        };
        let brace = match &self.bracing.drag {
            Some(drag) => drag.brace,
            None => match workspace
                .supports(object)
                .and_then(|d| d.braces.iter().find(|b| b.id == id))
            {
                Some(brace) => *brace,
                None => return Vec::new(),
            },
        };
        [brace.from, brace.to]
            .into_iter()
            .filter_map(|end| self.end_point(object, end))
            .collect()
    }

    /// A press at `pointer` on the viewport `rect`: an end handle of the selected brace of model
    /// `index` starts a drag along its shaft. Returns whether the press was taken.
    pub fn brace_press(
        &mut self,
        workspace: &Workspace,
        camera: &Camera,
        rect: egui::Rect,
        pointer: egui::Pos2,
        index: usize,
    ) -> bool {
        if self.bracing.placing || self.bracing.auto.is_some() {
            return false;
        }
        let Some((object, id)) = self.bracing.selected.filter(|&(o, _)| o == index) else {
            return false;
        };
        let Some(brace) = workspace
            .supports(object)
            .and_then(|d| d.braces.iter().find(|b| b.id == id))
            .copied()
        else {
            return false;
        };
        let size = [f64::from(rect.width()), f64::from(rect.height())];
        let near = |end: End| {
            let at = self.end_point(object, end)?;
            let [x, y] = camera.screen(at, size)?;
            #[allow(clippy::cast_possible_truncation)] // Screen positions.
            let at = rect.min + egui::vec2(x as f32, y as f32);
            Some(at.distance(pointer))
        };
        let reach = [brace.from, brace.to].map(|end| near(end).unwrap_or(f32::INFINITY));
        let end = usize::from(reach[1] < reach[0]);
        if reach[end] > HANDLE_REACH {
            return false;
        }
        self.bracing.drag = Some(EndDrag { object, brace, end });
        true
    }

    /// Slide the dragged end along its shaft to where `ray` passes closest, within the shaft.
    fn drag_brace_end(&mut self, ray: &Ray) {
        let Some(drag) = &self.bracing.drag else {
            return;
        };
        let (object, mut brace, which) = (drag.object, drag.brace, drag.end);
        let end = if which == 0 { brace.from } else { brace.to };
        let (Some(at), Some(scene)) = (self.end_point(object, end), self.scene(object)) else {
            return;
        };
        let Some([low, high]) = scene.shaft_span(end.0) else {
            return;
        };
        let Some(t) = along_axis(ray, at, [0.0, 0.0, 1.0]) else {
            return;
        };
        // Clear of the shaft's joints at its ends.
        let room = self.brace_diameter(object, &brace).unwrap_or(0.0);
        let z = (at[2] + t).clamp(low + room, (high - room).max(low + room));
        if which == 0 {
            brace.from.1 = z;
        } else {
            brace.to.1 = z;
        }
        let ends = self
            .end_point(object, brace.from)
            .zip(self.end_point(object, brace.to))
            .map(|(a, b)| [a, b]);
        if let Some(drag) = &mut self.bracing.drag {
            drag.brace = brace;
        }
        let diameter = self.brace_diameter(object, &brace).unwrap_or(room);
        self.show_bar(ends, diameter);
    }

    /// The end drag ended: the brace takes its new height as one undoable step.
    pub(super) fn release_brace(&mut self, workspace: &mut Workspace) {
        if let Some(drag) = self.bracing.drag.take() {
            self.bracing.preview = None;
            Self::replace_brace(workspace, drag.object, drag.brace, false);
        }
    }

    /// Whether a brace's end is being dragged.
    #[must_use]
    pub(super) fn brace_dragging(&self) -> bool {
        self.bracing.drag.is_some()
    }

    /// The brace being dragged, which shows as its preview instead.
    pub(super) fn dragged_brace(&self) -> Option<(usize, u64)> {
        self.bracing.drag.as_ref().map(|d| (d.object, d.brace.id))
    }

    /// The selected brace, which the scene draws lighter.
    pub(super) fn chosen_brace(&self) -> Option<(usize, u64)> {
        self.bracing.selected
    }

    /// Paint the selected brace's end handles over the viewport `rect` of model `index`, and the
    /// first end clicked while placing; the one under `hover` or dragged highlighted.
    #[allow(clippy::too_many_arguments)] // The overlay's whole context.
    pub fn paint_brace_handles(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        camera: &Camera,
        workspace: &Workspace,
        index: Option<usize>,
        hover: Option<egui::Pos2>,
        appearance: &Appearance,
    ) {
        let Some(index) = index else {
            return;
        };
        let size = [f64::from(rect.width()), f64::from(rect.height())];
        let screen = |p: [f64; 3]| {
            camera.screen(p, size).map(|[x, y]| {
                #[allow(clippy::cast_possible_truncation)] // Screen positions.
                let at = rect.min + egui::vec2(x as f32, y as f32);
                at
            })
        };
        if self.bracing.placing {
            let first = self
                .bracing
                .first
                .filter(|&(object, _)| object == index)
                .and_then(|(_, end)| self.end_point(index, end))
                .and_then(screen);
            if let Some(at) = first {
                painter.circle_filled(at, 6.0, appearance.hole_valid.color());
            }
            return;
        }
        let dragged = self.bracing.drag.as_ref().map(|d| d.end);
        for (k, at) in self.brace_handles(workspace, index).into_iter().enumerate() {
            let Some(at) = screen(at) else {
                continue;
            };
            let active = dragged == Some(k)
                || (dragged.is_none() && hover.is_some_and(|p| p.distance(at) <= HANDLE_REACH));
            let colour = if active {
                appearance.hovered.color()
            } else {
                appearance.uniform.color()
            };
            painter.circle_filled(at, 6.0, colour);
        }
    }

    /// The braces' previews: the one being placed or dragged, and the automatic ones (kept, and
    /// skipped in the error colour).
    pub(super) fn brace_draws(&self, appearance: &Appearance) -> Vec<Draw> {
        let mut draws = Vec::new();
        if let Some(bar) = &self.bracing.preview {
            draws.push(support_draw(
                PREVIEW_KEY,
                translation(bar.origin),
                appearance.hole_valid.linear(),
                None,
            ));
        }
        if let Some(auto) = &self.bracing.auto {
            draws.extend(auto.draws(appearance));
        }
        draws
    }

    /// Upload the previews once each, and free them when gone.
    pub(super) fn sync_braces(&mut self, viewport: &mut Viewport, state: &egui_wgpu::RenderState) {
        let b = &mut self.bracing;
        let at = b.preview.as_ref().map(|p| p.stamp);
        if at != b.uploaded {
            viewport.remove(PREVIEW_KEY);
            if let Some(bar) = &b.preview {
                viewport.upload(&state.device, PREVIEW_KEY, &bar.mesh, bar.origin);
            }
            b.uploaded = at;
        }
        match &mut b.auto {
            Some(auto) => auto.sync(viewport, state),
            None => auto::Auto::retire(viewport),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::supports_ui::tests::{built, lifted_cube, ray};
    use karasu_core::supports::{BaseSpec, Preset, SupportDocument};

    /// Two supports 8 mm apart under the cube lifted 15 mm, built: their shafts reach from about
    /// 2 mm to above 12 mm.
    pub(super) fn two_supports() -> (
        Workspace,
        crate::preparation_ui::PreparationUi,
        std::path::PathBuf,
        SupportsUi,
        [f64; 2],
    ) {
        let (mut workspace, mut preparation, directory) = lifted_cube();
        workspace.edit(|plate| plate.objects[0].pose.lift_mm = Some(15.0));
        crate::hollowing_ui::tests::prepared_within(
            &workspace,
            &mut preparation,
            std::time::Duration::from_secs(60),
        );
        let mut ui = SupportsUi::new();
        let placed = workspace.placement_of(0).unwrap();
        let [cx, cy] =
            [0, 1].map(|k| f64::midpoint(placed.min.coordinates()[k], placed.max.coordinates()[k]));
        ui.toggle_placing();
        for dx in [-4.0, 4.0] {
            let under = ray(cx + dx, cy, true);
            let hit = workspace.hit(&under);
            assert!(ui.click(&mut workspace, &under, hit, 0));
        }
        ui.toggle_placing();
        ui.close();
        built(&mut ui, &workspace, &preparation);
        (workspace, preparation, directory, ui, [cx, cy])
    }

    /// A ray across the shafts' plane at height `z`, from the side, through `x`.
    fn across(x: f64, y: f64, z: f64) -> Ray {
        Ray {
            origin: [x, y - 40.0, z],
            direction: [0.0, 1.0, 0.0],
        }
    }

    #[test]
    fn braces_are_placed_by_two_clicks_or_in_a_chain_and_level() {
        let (mut workspace, preparation, directory, mut ui, [cx, cy]) = two_supports();
        let ids: Vec<u64> = workspace
            .supports(0)
            .unwrap()
            .supports
            .iter()
            .map(|s| s.id)
            .collect();
        // Not placing, a click on a shaft is not the tool's.
        assert!(!ui.brace_click(&mut workspace, &across(cx - 4.0, cy, 5.0), 0));
        ui.toggle_brace_placing();
        // The first click sets an end at the height clicked; nothing is added yet.
        assert!(ui.brace_click(&mut workspace, &across(cx - 4.0, cy, 5.0), 0));
        assert_eq!(ui.bracing.first.map(|(_, e)| e.0), Some(ids[0]));
        assert_eq!(workspace.supports(0).unwrap().braces.len(), 0);
        // Following the pointer, the brace a click would add shows.
        ui.brace_hover(Some(&across(cx + 4.0, cy, 6.0)), 0);
        assert!(ui.bracing.preview.is_some());
        // The second click adds it, as one undoable step, and selects it.
        let revision = workspace.revision();
        assert!(ui.brace_click(&mut workspace, &across(cx + 4.0, cy, 6.0), 0));
        let braces = workspace.supports(0).unwrap().braces.clone();
        assert_eq!(braces.len(), 1);
        assert_eq!(braces[0].from.0, ids[0]);
        assert_eq!(braces[0].to.0, ids[1]);
        assert!((braces[0].from.1 - 5.0).abs() < 1e-6 && (braces[0].to.1 - 6.0).abs() < 1e-6);
        assert_ne!(workspace.revision(), revision);
        assert_eq!(ui.bracing.selected, Some((0, braces[0].id)));
        assert!(ui.bracing.first.is_none());
        // Level and in a chain: the next click goes on from the end just added.
        ui.set_brace_options(Some(true), Some(true));
        assert!(ui.brace_click(&mut workspace, &across(cx - 4.0, cy, 7.0), 0));
        assert!(ui.brace_click(&mut workspace, &across(cx + 4.0, cy, 8.0), 0));
        let braces = workspace.supports(0).unwrap().braces.clone();
        assert_eq!(braces.len(), 2);
        assert!((braces[1].to.1 - 7.0).abs() < 1e-6, "{:?}", braces[1]);
        assert_eq!(ui.bracing.first.map(|(_, e)| e.0), Some(ids[1]));
        // Escape ends the chain, then placing; built, the braces have no error.
        assert!(ui.escape());
        assert!(ui.bracing.first.is_none() && ui.brace_placing());
        assert!(ui.escape());
        assert!(!ui.brace_placing());
        built(&mut ui, &workspace, &preparation);
        let scene = ui.scene(0).unwrap();
        assert_eq!(scene.braces(), 2);
        assert!(!scene.has_errors(), "{:?}", scene.findings());
        // New braces' diameter is automatic: the shafts' where they meet them.
        assert_eq!(braces[0].diameter_mm, None);
        let shaft = scene.shaft_diameter(ids[0], 5.0).unwrap();
        let built = ui.brace_diameter(0, &braces[0]).unwrap();
        assert!(built > 0.1 && built <= shaft + 1e-12, "{built} {shaft}");
        // A click on a brace selects it; Delete removes it, a step of its own.
        ui.bracing.selected = None;
        assert!(ui.brace_click(&mut workspace, &across(cx, cy, 7.0), 0));
        assert_eq!(ui.bracing.selected, Some((0, braces[1].id)));
        assert!(ui.delete(&mut workspace));
        assert_eq!(workspace.supports(0).unwrap().braces.len(), 1);
        workspace.undo();
        assert_eq!(workspace.supports(0).unwrap().braces.len(), 2);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn a_dragged_end_slides_along_its_shaft_and_escape_puts_it_back() {
        let (mut workspace, preparation, directory, mut ui, [cx, cy]) = two_supports();
        ui.toggle_brace_placing();
        assert!(ui.brace_click(&mut workspace, &across(cx - 4.0, cy, 5.0), 0));
        assert!(ui.brace_click(&mut workspace, &across(cx + 4.0, cy, 5.0), 0));
        ui.toggle_brace_placing();
        built(&mut ui, &workspace, &preparation);
        let before = workspace.supports(0).unwrap().braces[0];
        let (object, _) = ui.bracing.selected.unwrap();
        ui.bracing.drag = Some(EndDrag {
            object,
            brace: before,
            end: 1,
        });
        // The pointer passes 1 mm higher beside the shaft: the end follows, within the shaft.
        ui.brace_hover(Some(&across(cx + 6.0, cy, 6.0)), 0);
        let moved = ui.bracing.drag.as_ref().unwrap().brace;
        assert!((moved.to.1 - 6.0).abs() < 1e-6, "{moved:?}");
        assert!(ui.dragged_brace().is_some() && ui.bracing.preview.is_some());
        // Far above the shaft's top it stops below it.
        ui.brace_hover(Some(&across(cx + 6.0, cy, 90.0)), 0);
        let top = ui.scene(0).unwrap().shaft_span(moved.to.0).unwrap()[1];
        assert!(ui.bracing.drag.as_ref().unwrap().brace.to.1 < top);
        // Escape puts it back; released, it takes the new height as one step.
        assert!(ui.escape());
        assert_eq!(workspace.supports(0).unwrap().braces[0], before);
        ui.bracing.drag = Some(EndDrag {
            object,
            brace: before,
            end: 1,
        });
        ui.brace_hover(Some(&across(cx + 6.0, cy, 6.0)), 0);
        ui.release(&mut workspace);
        assert!((workspace.supports(0).unwrap().braces[0].to.1 - 6.0).abs() < 1e-6);
        workspace.undo();
        assert_eq!(workspace.supports(0).unwrap().braces[0], before);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn new_brace_values_out_of_range_are_the_built_in_ones() {
        assert_eq!(DEFAULT_BRACES.valid(), DEFAULT_BRACES);
        let wrong = NewBraces {
            step_mm: 0.0,
            ..DEFAULT_BRACES
        };
        assert_eq!(wrong.valid(), DEFAULT_BRACES);
        // The built-in values make a brace the document accepts.
        let mut document = SupportDocument::new(Preset::Medium);
        for id in [1, 2] {
            document
                .supports
                .push(karasu_core::supports::ManualSupport {
                    id,
                    tip_mm: [f64::from(u8::try_from(id).unwrap()) * 5.0, 0.0, 10.0],
                    tip_axis: karasu_core::supports::AxisSpec::Normal,
                    bends: Vec::new(),
                    base: BaseSpec::Vertical,
                    parameters: Preset::Medium.parameters(),
                    accept_collision: false,
                    origin: karasu_core::supports::SupportOrigin::Placed,
                    kind: karasu_core::supports::SupportKind::Standard,
                });
        }
        document
            .braces
            .push(DEFAULT_BRACES.brace(4, (1, 6.0), (2, 6.0)));
        document.braces.push(Brace {
            id: 3,
            from: (1, 4.0),
            to: (2, 4.0),
            diameter_mm: Some(DEFAULT_BRACES.diameter_mm),
            max_diameter_mm: None,
        });
        assert!(document.validated().is_ok());
    }
}
