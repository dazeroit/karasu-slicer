//! The preparation panel of the selected object: hollowing, drain holes and the section view
//! (M6.4b, TD-083; holes reworked in TD-084, TD-085 and TD-089).
//!
//! **Hollowing** is an operation on the untouched model (TD-089): its wall thickness and grid
//! cell are edited as a draft, and "Apply hollowing" stores them; "Remove hollowing" takes the
//! operation away and keeps the holes. A wall under 1 mm is warned about, not refused (TD-093).
//! Cavity simplification is not offered (TD-093): a project that asks for it keeps it, and the
//! panel says so. The panel says whether the draft is applied, and the wall a scaled object
//! really has, since the cavity scales with it. It shows what the background preparation
//! ([`crate::preparation_ui`]) certified: the guaranteed wall, the thickest spot, the passes the
//! guarantee took, the volumes and the resin saved; the Drain holes panel shows every cavity
//! region with the holes that drain it. A region no hole reaches is reported as trapping uncured
//! resin; nothing is claimed about resin flow. The two tool panels are drawn by [`panel`].
//!
//! **Drain holes** are independent of hollowing: a solid object takes them too. They are kept
//! in the object's model frame, so they follow it; where they go and how they lean is resolved
//! by the probe of the object as it is placed. They are solids, drawn solid where in view and
//! as a faint ghost where the model hides them (TD-085, TD-086):
//! - With "Place holes" on, the hole a click would place follows the pointer on the
//!   selected object's outer surface, or on its cavity where the section shows it, green where
//!   it can be cut and red where it cannot (the reason is in the panel's footer). A click adds a *new*
//!   hole, green, with what it would do, and selects it; "Apply holes" cuts the new holes,
//!   "Discard" drops them.
//! - New holes are shaped exactly like applied ones before they are applied (owner, 2026-10-21):
//!   selected, dragged, resized, deepened and tilted with the same handles and fields, and
//!   checked as they change. Applied holes are blue, the selected one lighter, a refused one
//!   red. A hole is selected and dragged along the surface by its solid; it is checked all the
//!   way and returns where it was when released where it is refused.
//! - The selected hole has three handles: at its rim to change the diameter, at its end to set a
//!   fixed depth, and on its axis outside the model to tilt it freely. Its turn gizmo's rings
//!   lean it about one axis at a time, the world's or the surface's (TD-120). The panel edits
//!   the same values.
//! - Delete removes the selected hole; Escape ends a drag or placing, then releases the hole.
//!
//! The section, which shows the cavity and the wall inside the model, is the viewport's own
//! panel ([`crate::section_ui`]).
//!
//! Every applied change is a plate edit: undoable, and followed by a new preparation. The model
//! in the project is never changed.
use crate::shown::Seen;
use crate::{
    appearance::Appearance,
    camera::{Camera, Mat4, Ray},
    gizmo::{along_axis, angle_about, on_plane, ray_mesh, Affine},
    manipulate::{self, Handle, Kind, Space},
    preparation_ui::{centre, translation, PreparationUi, Verdict},
    viewport::{Draw, HoleCylinder, Viewport},
    workspace::{SurfaceHit, Workspace},
};
use eframe::{egui, egui_wgpu};
use karasu_core::{
    geometry::Point3,
    mesh::TriangleMesh,
    preparation::{
        sampled::{axis_tilt, hole_tangents, map_tilt, HoleFrame, HoleSurface},
        HoleSpec, HoleTilt, MAX_HOLE_TILT_DEG,
    },
};
use karasu_project::{
    project::{Hollowing, PlateObject},
    request::DrainHole,
};
use std::collections::HashMap;

pub mod blockers;
pub mod cups;
mod list;
mod panel;

/// A part of the preparation panel, one per tool of the workflow bar (TD-098).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// Hollowing, and the preparation's report.
    Hollow,
    /// Drain holes, and the report.
    Holes,
}

/// Wall thickness of a newly hollowed object, in mm.
pub const DEFAULT_WALL_MM: f64 = 2.0;
/// The thinnest wall the field takes, in mm: a bound of the field, not of hollowing (TD-093).
const MIN_WALL_MM: f64 = 0.1;
/// Walls under this thickness, in mm, are warned about (TD-093).
const THIN_WALL_MM: f64 = 1.0;
/// Diameter of a newly placed drain hole, in mm.
pub const DEFAULT_HOLE_MM: f64 = 3.0;
/// Depth of a new hole in a solid object, in mm.
const SOLID_HOLE_DEPTH_MM: f64 = 3.0;
/// How far a new hole in a hollowed object reaches past its wall, in mm.
const PAST_WALL_MM: f64 = 1.0;
/// Margins of a surface hole outside the model and inside the cavity, in mm (as the command
/// line's `--surface-drain-hole`).
const HOLE_MARGIN_MM: f64 = karasu_core::preparation::SURFACE_HOLE_MARGIN_MM;
/// Cavity regions below this volume, in mm³, are reported together.
const TINY_REGION_MM3: f64 = 1.0;
/// Undo merge keys of the panel's values, apart from the other keys.
const MERGE_KEYS: u64 = 0x484F_4C4C_0000_0000;
/// Viewport mesh keys of drain-hole solids, apart from the other keys.
const SOLID_KEYS: u64 = 0xD4A1_0000_0000_0000;
/// Viewport mesh keys of new holes' solids, apart from the applied ones.
const PENDING_KEYS: u64 = SOLID_KEYS | 1 << 52;
/// How far from a handle a press still takes it, in points.
const HANDLE_REACH: f32 = 10.0;
/// How near a ring of the turn gizmo a press takes it, in points: as the viewport's gizmos.
const RING_REACH: f64 = 8.0;
/// How far outside the surface the tilt handle stands, in mm.
const TILT_HANDLE_MM: f64 = 4.0;
/// Opacity of a hole's ghost, where the model hides it.
const GHOST_ALPHA: f32 = 0.3;

/// The hollowing of an object that was never hollowed.
pub(crate) const DEFAULT_HOLLOWING: Hollowing = Hollowing {
    wall_mm: DEFAULT_WALL_MM,
    cell_mm: None,
    simplify: None,
};

/// One undo step per value of one object (or hole): a dragged value is one change.
fn merge_key(object: usize, field: u64) -> u64 {
    MERGE_KEYS | (object as u64) << 16 | field
}

/// The surface a hole starts from: explicit cylinders start outside.
fn start_of(hole: &DrainHole) -> HoleSurface {
    match hole {
        DrainHole::Surface { surface, .. } => *surface,
        DrainHole::Axis { .. } => HoleSurface::Outer,
    }
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|k| a[k] + b[k])
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|k| a[k] - b[k])
}

fn scale(a: [f64; 3], s: f64) -> [f64; 3] {
    a.map(|c| c * s)
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|k| a[k] * b[k]).sum()
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn unit(a: [f64; 3]) -> Option<[f64; 3]> {
    let length = dot(a, a).sqrt();
    (length > 1e-12).then(|| scale(a, 1.0 / length))
}

/// The matrix of `map`.
fn matrix(map: &Affine) -> Mat4 {
    Mat4(std::array::from_fn(|row| {
        if row == 3 {
            [0.0, 0.0, 0.0, 1.0]
        } else {
            let l = map.linear[row];
            [l[0], l[1], l[2], map.translation[row]]
        }
    }))
}

/// `mesh` with its vertices mapped by `map`, its faces as they are.
fn map_mesh(mesh: &TriangleMesh, map: &Affine) -> Option<TriangleMesh> {
    let vertices = mesh
        .vertices()
        .iter()
        .map(|p| {
            let [x, y, z] = map.point([p.x, p.y, p.z]);
            Point3::new(x, y, z)
        })
        .collect();
    TriangleMesh::try_new(vertices, mesh.triangles().to_vec()).ok()
}

/// A tilt measured about the world normal `outward` of an object placed by `map`, in the
/// object's model frame, where holes are kept (TD-089).
fn model_tilt(tilt: HoleTilt, outward: [f64; 3], map: &Affine) -> Option<HoleTilt> {
    let inverse = map.inverse()?;
    // Normals go back to the model frame by the transposed linear part.
    let back = std::array::from_fn(|c| (0..3).map(|r| map.linear[r][c] * outward[r]).sum());
    map_tilt(tilt, outward, inverse.linear, unit(back)?)
}

/// The axes a hole's rings turn about, for its unit `outward` normal: the world's, or the
/// surface's own (its tangents, then the normal), which turn the axis across the surface and
/// about the normal.
fn ring_axes(space: Space, outward: [f64; 3]) -> [[f64; 3]; 3] {
    match space {
        Space::Global => Space::Global.axes([0.0; 3]),
        Space::Local => {
            let (u, v) = hole_tangents(outward);
            [u, v, outward]
        }
    }
}

/// The tilt, in the model frame placed by `map`, of a hole whose inward world `axis` leans from
/// the world `outward` normal: an angle of exactly 0 when it does not lean (see [`recorded`]),
/// and the most allowed when it leans past it, still facing the same way. `None` when the
/// placement cannot carry it.
fn lean(axis: [f64; 3], outward: [f64; 3], map: &Affine) -> Option<HoleTilt> {
    let world = axis_tilt(outward, axis)?;
    if world.angle_deg < 1e-9 {
        return Some(HoleTilt {
            angle_deg: 0.0,
            azimuth_deg: 0.0,
        });
    }
    let world = HoleTilt {
        angle_deg: world.angle_deg.min(MAX_HOLE_TILT_DEG),
        ..world
    };
    model_tilt(world, outward, map)
}

/// `tilt` as a hole records it: none for a perpendicular hole.
fn recorded(tilt: HoleTilt) -> Option<HoleTilt> {
    (tilt.angle_deg > 0.0).then_some(tilt)
}

/// Surface hole `hole` of object `object`, its diameter just changed, leaning so that its
/// inward world `axis` stays as it was (TD-121): its normal, the mean over its radius, moves
/// with the diameter, and only its size should. As it is where the normal cannot be found.
fn keeping_axis(
    workspace: &Workspace,
    preparation: &PreparationUi,
    object: usize,
    mut hole: DrainHole,
    axis: [f64; 3],
) -> DrainHole {
    let kept = preparation
        .hole_outward(workspace, object, &hole)
        .zip(workspace.model_to_world(object))
        .and_then(|(outward, map)| lean(axis, outward, &map));
    if let (DrainHole::Surface { tilt, .. }, Some(kept)) = (&mut hole, kept) {
        *tilt = recorded(kept);
    }
    hole
}

/// `v` turned by `degrees` about the unit `axis`, counterclockwise seen from its tip.
fn rotated(v: [f64; 3], axis: [f64; 3], degrees: f64) -> [f64; 3] {
    let (sin, cos) = degrees.to_radians().sin_cos();
    let along = scale(axis, dot(axis, v) * (1.0 - cos));
    add(add(scale(v, cos), scale(cross(axis, v), sin)), along)
}

/// The depth of a new hole in `object`: past the wall of a hollowed object, as the wall is at
/// its scale, or a few millimetres into a solid one.
fn default_depth(object: Option<&PlateObject>) -> f64 {
    object
        .and_then(|o| {
            let largest = o.pose.scale.iter().copied().fold(0.0, f64::max);
            Some(o.hollowing()?.wall_mm * largest)
        })
        .map_or(SOLID_HOLE_DEPTH_MM, |wall| wall + PAST_WALL_MM)
}

/// A cylinder for display only (a hole whose cutter the core could not give): `sides` faces
/// around the segment from `entry` along the unit `axis`.
fn display_cylinder(
    entry: [f64; 3],
    axis: [f64; 3],
    length: f64,
    radius: f64,
) -> Option<TriangleMesh> {
    const SIDES: u32 = 24;
    let helper = if axis[2].abs() < 0.9 {
        [0.0, 0.0, 1.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let u = unit(cross(axis, helper))?;
    let v = cross(axis, u);
    let end = add(entry, scale(axis, length));
    let mut vertices = Vec::new();
    for base in [entry, end] {
        for k in 0..SIDES {
            let angle = f64::from(k) * std::f64::consts::TAU / f64::from(SIDES);
            let p = add(
                base,
                add(
                    scale(u, radius * angle.cos()),
                    scale(v, radius * angle.sin()),
                ),
            );
            vertices.push(Point3::new(p[0], p[1], p[2]));
        }
    }
    vertices.push(Point3::new(entry[0], entry[1], entry[2]));
    vertices.push(Point3::new(end[0], end[1], end[2]));
    let (start_centre, end_centre) = (2 * SIDES, 2 * SIDES + 1);
    let mut triangles = Vec::new();
    for k in 0..SIDES {
        let next = (k + 1) % SIDES;
        triangles.push([k, next, SIDES + next]);
        triangles.push([k, SIDES + next, SIDES + k]);
        triangles.push([start_centre, next, k]);
        triangles.push([end_centre, SIDES + k, SIDES + next]);
    }
    TriangleMesh::try_new(vertices, triangles).ok()
}

/// What a drag of the selected hole changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Grip {
    /// The solid: the hole slides along the surface.
    Body,
    /// The rim handle: the diameter.
    Rim,
    /// The end handle: a fixed depth.
    End,
    /// The axis handle outside the model: the tilt.
    Tilt,
    /// A ring of the turn gizmo: the axis turns about axis 0, 1 or 2 of the drag's space.
    Ring(usize),
}

/// A hole of an object: applied (in its timeline) or new (placed, not applied yet).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HoleRef {
    /// The applied hole at this position among the object's enabled holes.
    Applied(usize),
    /// The new hole at this position in the panel's list of new holes.
    New(usize),
}

/// A hole being dragged: which grip, and the hole's geometry in world coordinates when the drag
/// began, so the handles' arithmetic stays stable while the hole changes.
#[derive(Clone, Copy, Debug)]
struct HoleDrag {
    object: usize,
    which: HoleRef,
    grip: Grip,
    frame: Option<HoleFrame>,
    axis: [f64; 3],
    /// The pointer's ray when the drag began, and the axes the rings turn about (world).
    start_ray: Ray,
    axes: [[f64; 3]; 3],
    /// Whether a ring turns in steps of 15 degrees (Shift).
    snap: bool,
}

/// A hole the pointer would place, or a dragged hole's new shape, in its object's model frame,
/// with the probe's verdict.
pub struct Candidate {
    object: usize,
    hole: DrainHole,
    /// `None` while the object has no probe yet (it is being prepared).
    verdict: Option<Verdict>,
    /// Where the pointer is, to draw the hole before it is probed.
    at: Option<SurfaceHit>,
    generation: u64,
}

/// A hole solid uploaded to the viewport.
struct Solid {
    key: u64,
    origin: [f64; 3],
}

/// Hollowing settings edited before they are applied to `object`.
#[derive(Clone, Copy, Debug)]
struct Draft {
    object: usize,
    /// The object's hollowing when the draft began: the draft starts again when it changes
    /// (applied, removed or undone).
    base: Option<Hollowing>,
    edit: Hollowing,
}

/// A hole placed but not applied yet (TD-089), in its object's model frame. It is edited like an
/// applied hole, with the same live checks, before it is applied.
struct Pending {
    object: usize,
    hole: DrainHole,
    /// Its verdict and the plate revision it was checked at. An outdated one is kept until the
    /// object has a probe again: its solid, in the model frame, still follows the object.
    verdict: Option<(u64, Verdict)>,
    /// Its solid in the model frame, until it is uploaded.
    mesh: Option<TriangleMesh>,
    solid: Option<Solid>,
}

/// An applied change of an object, reported once it is prepared (TD-099).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Awaiting {
    /// Hollowing applied to `object`, at the plate's `revision`.
    Hollowed { object: usize, revision: u64 },
    /// `count` new holes applied to `object`, at the plate's `revision`.
    Holes {
        object: usize,
        revision: u64,
        count: usize,
    },
}

/// The location of the cavity regions that trap resin (TD-101): the markers and the region shown.
struct CavityView {
    /// The markers show (a view preference of the Drain holes tool).
    markers: bool,
    /// The markers drawn last, for the pointer: the object, the region's position, bounds and
    /// marked point, and what the marker says.
    #[allow(clippy::type_complexity)] // One record per marker, read in one place.
    marks: Vec<(usize, (usize, [[f64; 3]; 2], [f64; 3]), String)>,
    /// Whether the marker's mesh is uploaded.
    uploaded: bool,
    /// The bounds of a region asked to be shown, for the window.
    request: Option<[[f64; 3]; 2]>,
    /// The region shown, as (object, region position), highlighted in the panel.
    shown: Option<(usize, usize)>,
    /// The Drainage section lists its pockets one by one.
    pockets_open: bool,
}

impl Default for CavityView {
    fn default() -> Self {
        Self {
            markers: true,
            marks: Vec::new(),
            uploaded: false,
            request: None,
            shown: None,
            pockets_open: false,
        }
    }
}

/// The panel's state.
#[allow(clippy::struct_excessive_bools)] // The tool's switches and modes, each its own.
pub struct HollowingUi {
    /// The object's Repair section (TD-092 r8).
    repair: crate::repair_panel::RepairPanel,
    /// The Hollow panel's Advanced switch: the grid cell and the passes show.
    hollow_advanced: bool,
    /// The Drain holes panel's Advanced switch: the selected hole's tilt shows.
    holes_advanced: bool,
    /// The tool's hollowing: what a model never hollowed starts from (design rule 6, TD-098).
    new_hollowing: Hollowing,
    /// An applied change waiting for its preparation, to report it.
    awaiting: Option<Awaiting>,
    /// Messages for the window: what applied changes did.
    outbox: Vec<crate::messages::Message>,
    /// How the cavity regions that trap resin are located (TD-101).
    cavity: CavityView,
    /// The pointer places drain holes on the selected object.
    placing: bool,
    /// Diameter of the next hole placed.
    hole_mm: f64,
    /// Depth of the next hole placed, or `None` for the object's default ([`default_depth`]).
    depth_mm: Option<f64>,
    /// The surface new holes start from.
    start: HoleSurface,
    /// The selected hole, as (object, hole).
    selected: Option<(usize, HoleRef)>,
    /// The list window is open (owner, 2026-10-29), kept with the settings.
    pub holes_list_open: bool,
    /// The list window shows only the refused holes; the others are then ghosts.
    only_refused: bool,
    /// Which holes the viewport shows while the tool is in use (the strip's Show, owner
    /// 2026-10-31), kept with the settings.
    pub holes_shown: crate::shown::Shown,
    /// The suction cups (M6.4j, TD-114).
    pub cups: cups::CupsUi,
    /// The hollowing blockers (owner, 2026-11-02).
    pub blockers: blockers::BlockersUi,
    /// The tools' defaults, for their fields' reset buttons (owner, 2026-11-02).
    pub tool_defaults: crate::settings::ToolDefaults,
    /// The list window lists the suction cups, not the holes.
    cups_listed: bool,
    /// A hole was selected in the scene: the list scrolls to its row once.
    reveal_hole: bool,
    dragging: Option<HoleDrag>,
    candidate: Option<Candidate>,
    generations: u64,
    /// Hole solids uploaded for the plate revision they show, by (object, index).
    solids: (u64, HashMap<(usize, usize), Solid>),
    /// Solids of holes as last probed, shown while their moved object is prepared again, with
    /// the revision they were probed at.
    held_solids: (u64, HashMap<(usize, usize), Solid>),
    /// The candidate's solid and the generation it shows.
    candidate_solid: Option<(u64, Solid)>,
    draft: Option<Draft>,
    pending: Vec<Pending>,
    /// How many objects the plate had when holes were last placed: objects may have changed
    /// places since it changed, so the new holes are dropped.
    pending_among: usize,
    /// The last viewport key given to a new hole's solid.
    pending_keys: u64,
    /// Viewport meshes to free at the next draw.
    retired: Vec<u64>,
    /// The applied holes' cylinders, which open the objects in the viewport, and the range of
    /// them that cuts each object.
    openings: (Vec<HoleCylinder>, HashMap<usize, [u32; 2]>),
}

impl Default for HollowingUi {
    fn default() -> Self {
        Self {
            repair: crate::repair_panel::RepairPanel::default(),
            hollow_advanced: false,
            holes_advanced: false,
            new_hollowing: DEFAULT_HOLLOWING,
            awaiting: None,
            outbox: Vec::new(),
            cavity: CavityView::default(),
            placing: false,
            hole_mm: DEFAULT_HOLE_MM,
            depth_mm: None,
            start: HoleSurface::Outer,
            selected: None,
            holes_list_open: true,
            only_refused: false,
            holes_shown: crate::shown::Shown::All,
            cups: cups::CupsUi::default(),
            blockers: blockers::BlockersUi::default(),
            tool_defaults: crate::settings::ToolDefaults::default(),
            cups_listed: false,
            reveal_hole: false,
            dragging: None,
            candidate: None,
            generations: 0,
            solids: (0, HashMap::new()),
            held_solids: (0, HashMap::new()),
            candidate_solid: None,
            draft: None,
            pending: Vec::new(),
            pending_among: 0,
            pending_keys: 0,
            retired: Vec::new(),
            openings: (Vec::new(), HashMap::new()),
        }
    }
}

/// The colour of an applied hole: red when refused (not `valid`); blue while its object is
/// `focused`, lighter when the hole is `hole_selected`; else its model's colour, the selected
/// one when `object_selected`, as it is only an opening there.
#[allow(clippy::fn_params_excessive_bools)] // Four independent facts of one hole.
fn applied_colour(
    appearance: &Appearance,
    valid: bool,
    focused: bool,
    object_selected: bool,
    hole_selected: bool,
) -> crate::appearance::Rgba {
    if !valid {
        appearance.support_error
    } else if !focused {
        if object_selected {
            appearance.selected
        } else {
            appearance.object
        }
    } else if hole_selected {
        appearance.hole_selected
    } else {
        appearance.drain_hole
    }
}

/// A hole the strip's Show or the list's filter leaves out as a ghost (owner, 2026-10-31): a
/// faint translucent solid, in the filtered colour, drawn through the model.
fn ghost_draw(solid: &Solid, appearance: &Appearance, place: Option<&Mat4>) -> Draw {
    let [_, ghost] = solid_draws(solid, appearance.filtered_ghost, place);
    Draw {
        color: appearance.filtered_ghost.linear(),
        ..ghost
    }
}

/// A hole solid drawn twice (TD-086): solid where it is in view, with depth like any mesh, and
/// as a faint ghost over everything where the model hides it, so it reads as in front of,
/// inside or behind the model as the view turns. `place` maps the solid's coordinates to world
/// ones, when they are not.
fn solid_draws(solid: &Solid, rgba: crate::appearance::Rgba, place: Option<&Mat4>) -> [Draw; 2] {
    let color = rgba.linear();
    let mut ghost = color;
    ghost[3] *= GHOST_ALPHA;
    let model = place.map_or_else(
        || translation(solid.origin),
        |place| place.mul(&translation(solid.origin)),
    );
    let draw = |color, overlay| Draw {
        mesh: solid.key,
        model,
        color,
        overlay,
        cap: crate::viewport::Cap::Off,
        holes: [0, 0],
        bore: None,
        outside: None,
        triangles: None,
        look: crate::viewport::Look::Solid,
    };
    [draw(color, false), draw(ghost, true)]
}

/// The cylinder a hole that can be cut opens, in world coordinates; `follow` carries it to
/// where its moved object is now.
fn opening(verdict: &Verdict, follow: Option<&Mat4>) -> Option<HoleCylinder> {
    let spec = verdict.spec.filter(|_| verdict.is_valid())?;
    let (mut entry, mut axis) = (spec.entry_mm, spec.direction);
    if let Some(Mat4(m)) = follow {
        entry = std::array::from_fn(|r| (0..3).map(|c| m[r][c] * entry[c]).sum::<f64>() + m[r][3]);
        axis = std::array::from_fn(|r| (0..3).map(|c| m[r][c] * axis[c]).sum());
    }
    let length = dot(axis, axis).sqrt();
    let scale = length / dot(spec.direction, spec.direction).sqrt();
    Some(HoleCylinder {
        entry_mm: entry,
        axis: unit(axis)?,
        length_mm: spec.length_mm * scale,
        radius_mm: spec.diameter_mm / 2.0,
    })
}

/// The identity placement: hole solids are in world coordinates.
const IDENTITY: Affine = Affine {
    linear: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
    translation: [0.0; 3],
};

/// A hole solid's mesh: the core's cutter, or a cylinder built for display.
enum SolidMesh<'a> {
    Cutter(&'a TriangleMesh),
    Display(TriangleMesh),
}

impl std::ops::Deref for SolidMesh<'_> {
    type Target = TriangleMesh;

    fn deref(&self) -> &TriangleMesh {
        match self {
            Self::Cutter(mesh) => mesh,
            Self::Display(mesh) => mesh,
        }
    }
}

/// The solid to show for a verdict: the core's cutter, or a display cylinder from its spec.
fn verdict_solid(verdict: &Verdict) -> Option<SolidMesh<'_>> {
    if let Some(cutter) = &verdict.cutter {
        return Some(SolidMesh::Cutter(cutter));
    }
    let spec = verdict.spec?;
    let axis = unit(spec.direction)?;
    display_cylinder(spec.entry_mm, axis, spec.length_mm, spec.diameter_mm / 2.0)
        .map(SolidMesh::Display)
}

/// Whether `a` is no farther than `b` along `ray`.
fn nearer(ray: Option<&Ray>, a: &SurfaceHit, b: &SurfaceHit) -> bool {
    ray.is_none_or(|ray| {
        let away = |hit: &SurfaceHit| {
            let d = sub(hit.point, ray.origin);
            dot(d, d)
        };
        away(a) <= away(b)
    })
}

impl HollowingUi {
    /// Whether the pointer places holes or drags one: the viewport then follows it.
    #[must_use]
    pub fn tracking(&self) -> bool {
        self.placing || self.dragging.is_some()
    }

    /// Whether a hole is being dragged.
    #[must_use]
    pub fn dragging(&self) -> bool {
        self.dragging.is_some()
    }

    /// Whether the pointer follows the cavity: new holes start from it, or a hole that does is
    /// dragged along it.
    #[must_use]
    pub fn wants_cavity(&self, workspace: &Workspace) -> bool {
        match self.dragging {
            Some(drag) => {
                drag.grip == Grip::Body
                    && self
                        .hole_of(workspace, drag.object, drag.which)
                        .is_some_and(|hole| start_of(&hole) == HoleSurface::Cavity)
            }
            None => self.placing && self.start == HoleSurface::Cavity,
        }
    }

    /// The objects that need a hole probe (TD-089): the one holes are placed on, `placing_on`,
    /// and those with new holes, which are checked again as they change or their object moves.
    /// Objects with applied holes have one anyway; others go without, as it is costly.
    #[must_use]
    pub fn probes(&self, placing_on: Option<usize>) -> Vec<usize> {
        let mut objects: Vec<usize> = placing_on
            .into_iter()
            .chain(self.pending.iter().map(|p| p.object))
            .collect();
        objects.sort_unstable();
        objects.dedup();
        objects
    }

    /// The panel closed (nothing selected): stop placing and dragging holes and release the
    /// selected hole. New holes stay with their object.
    pub fn close(&mut self) {
        self.placing = false;
        self.selected = None;
        self.dragging = None;
        self.candidate = None;
    }

    /// Escape: cancel a drag, else stop placing, else release the selected hole. Returns
    /// whether it did any of these.
    pub fn escape(&mut self) -> bool {
        if self.dragging.take().is_some() || self.placing {
            self.placing = false;
            self.candidate = None;
            return true;
        }
        self.selected.take().is_some()
    }

    /// Delete: remove the selected hole. Returns whether there was one.
    pub fn delete(&mut self, workspace: &mut Workspace) -> bool {
        let Some((object, which)) = self.selected else {
            return false;
        };
        self.dragging = None;
        match which {
            HoleRef::Applied(index) => {
                self.selected = None;
                workspace.remove_hole(object, index);
            }
            HoleRef::New(position) => self.drop_new(position),
        }
        true
    }

    /// Hole `which` of `object`, in its model frame.
    fn hole_of(&self, workspace: &Workspace, object: usize, which: HoleRef) -> Option<DrainHole> {
        match which {
            HoleRef::Applied(index) => workspace.holes(object).get(index).copied(),
            HoleRef::New(position) => self
                .pending
                .get(position)
                .filter(|p| p.object == object)
                .map(|p| p.hole),
        }
    }

    /// The current verdict of new hole `position`, while it describes the plate as it is.
    fn new_verdict(&self, workspace: &Workspace, position: usize) -> Option<&Verdict> {
        let (checked, verdict) = self.pending.get(position)?.verdict.as_ref()?;
        (*checked == workspace.revision()).then_some(verdict)
    }

    /// The resolved cut and frame of hole `which` of `object`, in world coordinates.
    fn shape(
        &self,
        workspace: &Workspace,
        preparation: &mut PreparationUi,
        object: usize,
        which: HoleRef,
    ) -> Option<(HoleSpec, HoleFrame)> {
        let verdict = match which {
            HoleRef::Applied(index) => preparation.verdicts(workspace, object)?.get(index)?,
            HoleRef::New(position) => self.new_verdict(workspace, position)?,
        };
        verdict.spec.zip(verdict.frame)
    }

    /// Drop new hole `position`, keeping the selection on the same holes.
    fn drop_new(&mut self, position: usize) {
        if position >= self.pending.len() {
            return;
        }
        let pending = self.pending.remove(position);
        self.retired.extend(pending.solid.map(|s| s.key));
        self.selected = match self.selected {
            Some((_, HoleRef::New(p))) if p == position => None,
            Some((o, HoleRef::New(p))) if p > position => Some((o, HoleRef::New(p - 1))),
            other => other,
        };
    }

    /// Give new hole `position` the `verdict` just made for it: its solid is rebuilt in the
    /// model frame and uploaded at the next draw.
    fn set_new_verdict(&mut self, workspace: &Workspace, position: usize, verdict: Verdict) {
        let Some(pending) = self.pending.get_mut(position) else {
            return;
        };
        pending.mesh = workspace
            .model_to_world(pending.object)
            .and_then(|map| map.inverse())
            .and_then(|back| map_mesh(&*verdict_solid(&verdict)?, &back));
        if let Some(old) = pending.solid.take() {
            self.retired.push(old.key);
        }
        pending.verdict = Some((workspace.revision(), verdict));
    }

    /// Check new hole `position` again, when its object has a probe.
    fn recheck(&mut self, workspace: &Workspace, preparation: &PreparationUi, position: usize) {
        let Some(pending) = self.pending.get(position) else {
            return;
        };
        if let Some(verdict) = preparation.verdict(workspace, pending.object, &pending.hole) {
            self.set_new_verdict(workspace, position, verdict);
        } else if let Some((checked, _)) = &mut self.pending[position].verdict {
            // No probe yet: the last verdict stays on show, outdated, until there is one.
            *checked = u64::MAX;
        }
    }

    /// The depth of the next hole placed on `object`.
    fn new_depth(&self, workspace: &Workspace, object: usize) -> f64 {
        self.depth_mm
            .unwrap_or_else(|| default_depth(workspace.plate().objects.get(object)))
    }

    /// Follow the pointer while placing or dragging a hole: `ray` is the pointer's, `hit` where
    /// it meets a model and `cavity` where it meets the selected object's visible cavity.
    pub fn hover(
        &mut self,
        workspace: &Workspace,
        preparation: &mut PreparationUi,
        selected: usize,
        ray: Option<&Ray>,
        hit: Option<SurfaceHit>,
        cavity: Option<SurfaceHit>,
    ) {
        let hit = hit.filter(|h| h.object == selected);
        // The cavity is seen only where nothing of the model is in front of it.
        let cavity = cavity
            .filter(|c| c.object == selected && hit.as_ref().is_none_or(|h| nearer(ray, c, h)));
        let on = |surface| match surface {
            HoleSurface::Outer => hit,
            HoleSurface::Cavity => cavity,
        };
        let shaped =
            workspace
                .model_to_world(selected)
                .and_then(|map| match (self.dragging, ray) {
                    (Some(drag), Some(ray)) => {
                        let hole = self.hole_of(workspace, drag.object, drag.which)?;
                        let at = on(start_of(&hole));
                        let hole = Self::dragged(hole, &drag, ray, at, &map)?;
                        // A new diameter keeps the axis where it was (TD-121).
                        let hole = if drag.grip == Grip::Rim {
                            keeping_axis(workspace, preparation, drag.object, hole, drag.axis)
                        } else {
                            hole
                        };
                        Some((hole, at))
                    }
                    (None, _) if self.placing => {
                        let at = on(self.start)?;
                        let hole = DrainHole::Surface {
                            surface_mm: map.inverse()?.point(at.point),
                            diameter_mm: self.hole_mm,
                            outside_mm: HOLE_MARGIN_MM,
                            inside_mm: HOLE_MARGIN_MM,
                            tilt: None,
                            depth_mm: Some(self.new_depth(workspace, selected)),
                            surface: self.start,
                        };
                        Some((hole, Some(at)))
                    }
                    _ => None,
                });
        let Some((hole, at)) = shaped else {
            // A dragged hole keeps its last shape when the pointer leaves the model.
            if self.dragging.is_none() {
                self.candidate = None;
            }
            return;
        };
        if self
            .candidate
            .as_ref()
            .is_some_and(|c| c.object == selected && c.hole == hole && c.verdict.is_some())
        {
            return;
        }
        self.generations += 1;
        let verdict = preparation.verdict(workspace, selected, &hole);
        self.candidate = Some(Candidate {
            object: selected,
            hole,
            verdict,
            at,
            generation: self.generations,
        });
    }

    /// The dragged hole's new shape, in the model frame of its object placed by `map`, for the
    /// pointer `ray` (and its `hit` on the surface the hole starts from).
    fn dragged(
        hole: DrainHole,
        drag: &HoleDrag,
        ray: &Ray,
        hit: Option<SurfaceHit>,
        map: &Affine,
    ) -> Option<DrainHole> {
        let DrainHole::Surface {
            surface_mm,
            diameter_mm,
            outside_mm,
            inside_mm,
            tilt,
            depth_mm,
            surface,
        } = hole
        else {
            return None;
        };
        let frame = drag.frame?;
        let shaped = |point, diameter_mm, tilt, depth_mm| DrainHole::Surface {
            surface_mm: point,
            diameter_mm,
            outside_mm,
            inside_mm,
            tilt,
            depth_mm,
            surface,
        };
        // Diameters and depths are millimetres in both frames; points and tilts are mapped.
        match drag.grip {
            Grip::Body => {
                let point = map.inverse()?.point(hit?.point);
                Some(shaped(point, diameter_mm, tilt, depth_mm))
            }
            Grip::End => {
                let t = along_axis(ray, frame.foot_mm, drag.axis)?;
                Some(shaped(
                    surface_mm,
                    diameter_mm,
                    tilt,
                    Some(t.clamp(0.1, 100.0)),
                ))
            }
            Grip::Rim => {
                let t = along_axis(ray, frame.foot_mm, drag.axis)?;
                let on_axis = add(frame.foot_mm, scale(drag.axis, t));
                let away = sub(on_axis, ray.origin);
                let direction = unit(ray.direction)?;
                let distance = dot(cross(away, direction), cross(away, direction)).sqrt();
                Some(shaped(
                    surface_mm,
                    (2.0 * distance).clamp(0.5, 20.0),
                    tilt,
                    depth_mm,
                ))
            }
            Grip::Tilt => {
                // The handle moves on the plane `height` outside the surface; the axis runs
                // from it through the foot.
                let outward = frame.outward;
                let height = outside_mm + TILT_HANDLE_MM;
                let centre = add(frame.foot_mm, scale(outward, height));
                let facing = dot(ray.direction, outward);
                if facing.abs() < 1e-9 {
                    return None;
                }
                let along = dot(sub(centre, ray.origin), outward) / facing;
                let on_plane = add(ray.origin, scale(ray.direction, along));
                let offset = sub(on_plane, centre);
                let off = dot(offset, offset).sqrt();
                let (first, second) = hole_tangents(outward);
                let world = (off > 1e-6).then(|| HoleTilt {
                    angle_deg: off.atan2(height).to_degrees().min(MAX_HOLE_TILT_DEG),
                    azimuth_deg: (-dot(offset, second))
                        .atan2(-dot(offset, first))
                        .to_degrees()
                        .rem_euclid(360.0),
                });
                let tilt = match world {
                    Some(world) => Some(model_tilt(world, outward, map)?),
                    None => None,
                };
                Some(shaped(surface_mm, diameter_mm, tilt, depth_mm))
            }
            Grip::Ring(k) => {
                // The axis as it was turned about the ring's axis by the pointer's angle about
                // the foot; a lean past the limit stops at it, still facing the way it turned.
                let normal = drag.axes[k];
                let from = on_plane(&drag.start_ray, frame.foot_mm, normal)?;
                let to = on_plane(ray, frame.foot_mm, normal)?;
                let mut degrees = angle_about(frame.foot_mm, normal, from, to);
                if drag.snap {
                    degrees = (degrees / 15.0).round() * 15.0;
                }
                let axis = rotated(drag.axis, normal, degrees);
                let tilt = recorded(lean(axis, frame.outward, map)?);
                Some(shaped(surface_mm, diameter_mm, tilt, depth_mm))
            }
        }
    }

    /// The selected hole's shape and frame in world coordinates, from its current verdict.
    fn selected_shape(
        &self,
        workspace: &Workspace,
        preparation: &mut PreparationUi,
    ) -> Option<(HoleSpec, HoleFrame)> {
        let (object, which) = self.selected?;
        // While dragging, the handles follow the candidate.
        let candidate = self
            .candidate
            .as_ref()
            .filter(|_| self.dragging.is_some())
            .and_then(|c| c.verdict.as_ref());
        match candidate {
            Some(verdict) => verdict.spec.zip(verdict.frame),
            None => self.shape(workspace, preparation, object, which),
        }
    }

    /// Where the selected hole's turn gizmo is centred: the foot of its axis on the surface, in
    /// world coordinates.
    pub fn ring_anchor(
        &self,
        workspace: &Workspace,
        preparation: &mut PreparationUi,
    ) -> Option<[f64; 3]> {
        Some(self.selected_shape(workspace, preparation)?.1.foot_mm)
    }

    /// The selected hole's turn gizmo, `size` mm in radius, about the axes of `space`.
    pub fn rings(
        &self,
        workspace: &Workspace,
        preparation: &mut PreparationUi,
        size: f64,
        space: Space,
    ) -> Vec<Handle> {
        self.selected_shape(workspace, preparation)
            .map_or_else(Vec::new, |(_, frame)| {
                manipulate::rings(frame.foot_mm, size, ring_axes(space, frame.outward))
            })
    }

    /// The ring of the turn gizmo being dragged, if one is.
    #[must_use]
    pub fn turning(&self) -> Option<usize> {
        match self.dragging?.grip {
            Grip::Ring(k) => Some(k),
            _ => None,
        }
    }

    /// Whether a ring dragged now turns in steps of 15 degrees.
    pub fn set_snap(&mut self, snap: bool) {
        if let Some(drag) = &mut self.dragging {
            drag.snap = snap;
        }
    }

    /// The handles of the selected hole, in world coordinates, from its current verdict.
    fn handles(
        &self,
        workspace: &Workspace,
        preparation: &mut PreparationUi,
    ) -> Vec<(Grip, [f64; 3])> {
        let Some((spec, frame)) = self.selected_shape(workspace, preparation) else {
            return Vec::new();
        };
        let Some(axis) = unit(spec.direction) else {
            return Vec::new();
        };
        let radius = spec.diameter_mm / 2.0;
        let (u, _) = hole_tangents(frame.outward);
        let across = unit(sub(u, scale(axis, dot(u, axis)))).unwrap_or(u);
        let outside = dot(sub(frame.foot_mm, spec.entry_mm), axis);
        vec![
            (Grip::Rim, add(spec.entry_mm, scale(across, radius))),
            (Grip::End, add(spec.entry_mm, scale(axis, spec.length_mm))),
            (
                Grip::Tilt,
                sub(frame.foot_mm, scale(axis, outside + TILT_HANDLE_MM)),
            ),
        ]
    }

    /// Paint the selected hole's handles over the viewport `rect`; the one under `pointer`
    /// highlighted.
    #[allow(clippy::too_many_arguments)] // The overlay's whole context.
    pub fn paint_handles(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        camera: &Camera,
        workspace: &Workspace,
        preparation: &mut PreparationUi,
        hover: Option<egui::Pos2>,
        appearance: &Appearance,
    ) {
        let size = [f64::from(rect.width()), f64::from(rect.height())];
        let handles = self.handles(workspace, preparation);
        let screen = |p: [f64; 3]| {
            camera.screen(p, size).map(|[x, y]| {
                #[allow(clippy::cast_possible_truncation)] // Screen positions.
                let at = rect.min + egui::vec2(x as f32, y as f32);
                at
            })
        };
        // The tilt handle hangs on a line from the hole's entry.
        if let (Some(&(_, tilt)), Some(&(_, end))) = (
            handles.iter().find(|h| h.0 == Grip::Tilt),
            handles.iter().find(|h| h.0 == Grip::End),
        ) {
            if let (Some(a), Some(b)) = (screen(tilt), screen(end)) {
                painter.line_segment([a, b], egui::Stroke::new(1.5, appearance.uniform.color()));
            }
        }
        for (grip, at) in handles {
            let Some(at) = screen(at) else {
                continue;
            };
            let active = self.dragging.is_some_and(|d| d.grip == grip)
                || hover.is_some_and(|p| p.distance(at) <= HANDLE_REACH);
            let color = if active {
                appearance.hovered.color()
            } else {
                appearance.uniform.color()
            };
            match grip {
                Grip::Rim => {
                    painter.circle_filled(at, 6.0, color);
                }
                Grip::End => {
                    painter.rect_filled(
                        egui::Rect::from_center_size(at, egui::vec2(11.0, 11.0)),
                        2.0,
                        color,
                    );
                }
                Grip::Tilt => {
                    painter.circle_stroke(at, 7.0, egui::Stroke::new(2.5, color));
                }
                Grip::Body | Grip::Ring(_) => {}
            }
        }
    }

    /// Drop the new holes when the plate's objects may have changed places.
    fn check_pending(&mut self, workspace: &Workspace) {
        if !self.pending.is_empty() && self.pending_among != workspace.plate().objects.len() {
            self.discard(None);
        }
    }

    /// How many drain holes model `index` has, applied and new.
    #[must_use]
    pub fn hole_count(&self, workspace: &Workspace, index: usize) -> usize {
        workspace.holes(index).len() + self.pending.iter().filter(|p| p.object == index).count()
    }

    /// Delete every drain hole of model `index`: the applied ones as one undoable step, the new
    /// ones dropped.
    pub fn delete_all_holes(&mut self, workspace: &mut Workspace, index: usize) {
        self.discard(Some(index));
        if !workspace.holes(index).is_empty() {
            workspace.remove_holes(index);
        }
        if self.selected.is_some_and(|(object, _)| object == index) {
            self.selected = None;
            self.dragging = None;
        }
    }

    /// Drop the new holes of `object`, or of every object.
    fn discard(&mut self, object: Option<usize>) {
        let (dropped, kept) = std::mem::take(&mut self.pending)
            .into_iter()
            .partition(|p| object.is_none_or(|o| p.object == o));
        self.pending = kept;
        let dropped: Vec<Pending> = dropped;
        self.retired
            .extend(dropped.iter().filter_map(|p| Some(p.solid.as_ref()?.key)));
        if matches!(self.selected, Some((_, HoleRef::New(_)))) {
            self.selected = None;
            self.dragging = None;
        }
    }

    /// Apply the new holes of `object`: they join its timeline as one plate edit.
    pub(crate) fn apply_pending(&mut self, workspace: &mut Workspace, object: usize) {
        let holes = self
            .pending
            .iter()
            .filter(|p| p.object == object)
            .map(|p| p.hole)
            .collect();
        workspace.add_holes(object, holes);
        self.discard(Some(object));
    }

    /// The hole solids to draw over the model, uploading what changed. `focus` is the object
    /// whose holes are edited (the Drain holes tool on the selected object): its applied holes
    /// show blue (the selected one lighter), with their ghosts where the model hides them, its
    /// new holes green, and the candidate green or red; the dragged hole shows as the
    /// candidate. Elsewhere a cut hole shows only as the opening it makes, its bore in the
    /// model's colour (`selected` is the selected object), so the plate reads as it will print;
    /// a refused hole stays red, and new holes are not drawn. Only the draws and their colours
    /// change with the focus: nothing is uploaded again.
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)] // Every hole of the plate.
    pub fn draws(
        &mut self,
        viewport: &mut Viewport,
        state: &egui_wgpu::RenderState,
        workspace: &Workspace,
        preparation: &mut PreparationUi,
        appearance: &Appearance,
        focus: Option<usize>,
        selected: Option<usize>,
    ) -> Vec<Draw> {
        self.check_pending(workspace);
        for key in self.retired.drain(..) {
            viewport.remove(key);
        }
        let revision = workspace.revision();
        if self.solids.0 != revision {
            for (_, solid) in self.solids.1.drain() {
                viewport.remove(solid.key);
            }
            self.solids.0 = revision;
        }
        let mut draws = Vec::new();
        let (shown, ghosts, chosen) = (
            self.holes_shown,
            self.holes_list_open && self.only_refused,
            self.selected,
        );
        self.openings.0.clear();
        self.openings.1.clear();
        for object in 0..workspace.plate().objects.len() {
            let count = workspace.holes(object).len();
            // A hidden object's holes hide with it, and open nothing.
            if count == 0 || workspace.is_hidden(object) {
                continue;
            }
            let first = self.openings.0.len();
            let bores = draws.len();
            // While a moved object is prepared again, its holes as last probed follow it.
            let (verdicts, follow, solids) = if let Some(verdicts) =
                preparation.verdicts(workspace, object)
            {
                (verdicts, None, &mut self.solids.1)
            } else {
                let Some((probed, verdicts, follow)) = preparation.held_verdicts(workspace, object)
                else {
                    continue;
                };
                if self.held_solids.0 != probed {
                    for (_, solid) in self.held_solids.1.drain() {
                        viewport.remove(solid.key);
                    }
                    self.held_solids.0 = probed;
                }
                (verdicts, Some(follow), &mut self.held_solids.1)
            };
            for (index, verdict) in verdicts.iter().enumerate().take(count) {
                if self
                    .dragging
                    .is_some_and(|d| d.object == object && d.which == HoleRef::Applied(index))
                {
                    continue;
                }
                if let std::collections::hash_map::Entry::Vacant(slot) =
                    solids.entry((object, index))
                {
                    let Some(mesh) = verdict_solid(verdict) else {
                        continue;
                    };
                    // Held solids take the keys' top bit, apart from the current ones.
                    let held = u64::from(follow.is_some()) << 51;
                    let key = SOLID_KEYS
                        | held
                        | (revision & 0xFFFF_FFFF) << 20
                        | (object as u64 & 0xFF) << 12
                        | index as u64 & 0xFFF;
                    let origin = centre(&mesh);
                    viewport.upload(&state.device, key, &mesh, origin);
                    slot.insert(Solid { key, origin });
                }
                let solid = &solids[&(object, index)];
                // The strip's Show and the list's filter, on the focused object (owner,
                // 2026-10-31): a hole left out still opens its object, as elsewhere.
                let seen = if focus == Some(object) {
                    crate::shown::seen(
                        shown,
                        ghosts.then_some(!verdict.is_valid()),
                        !verdict.is_valid(),
                        chosen == Some((object, HoleRef::Applied(index))),
                        false,
                    )
                } else {
                    Seen::Solid
                };
                if seen != Seen::Solid && !verdict.is_valid() {
                    if seen == Seen::Ghost {
                        draws.push(ghost_draw(solid, appearance, follow.as_ref()));
                    }
                    continue;
                }
                let focused = focus == Some(object) && seen == Seen::Solid;
                let color = applied_colour(
                    appearance,
                    verdict.is_valid(),
                    focused,
                    selected == Some(object),
                    chosen == Some((object, HoleRef::Applied(index))),
                );
                let [mut solid_draw, ghost] = solid_draws(solid, color, follow.as_ref());
                // A hole that is cut opens the object: its solid shows as its bore.
                if let Some(cylinder) = opening(verdict, follow.as_ref()) {
                    solid_draw.bore = u32::try_from(self.openings.0.len()).ok();
                    self.openings.0.push(cylinder);
                }
                draws.push(solid_draw);
                if focused {
                    draws.push(ghost);
                } else if seen == Seen::Ghost {
                    draws.push(ghost_draw(solid, appearance, follow.as_ref()));
                }
            }
            let range =
                [first, self.openings.0.len() - first].map(|n| u32::try_from(n).unwrap_or(0));
            for draw in &mut draws[bores..] {
                if draw.bore.is_some() {
                    draw.holes = range;
                }
            }
            self.openings.1.insert(object, range);
        }
        draws.extend(self.pending_draws(
            viewport,
            state,
            workspace,
            preparation,
            appearance,
            focus,
        ));
        if focus.is_some() {
            draws.extend(
                self.candidate_draw(viewport, state, workspace, appearance)
                    .into_iter()
                    .flatten(),
            );
        }
        draws
    }

    /// The cylinders of the applied holes that open the objects, as of the last
    /// [`Self::draws`].
    #[must_use]
    pub fn openings(&self) -> &[HoleCylinder] {
        &self.openings.0
    }

    /// The range of [`Self::openings`] that cuts `object`.
    #[must_use]
    pub fn cuts(&self, object: usize) -> [u32; 2] {
        self.openings.1.get(&object).copied().unwrap_or([0, 0])
    }

    /// The new holes' solids, kept in their object's model frame and drawn where it is now.
    fn pending_draws(
        &mut self,
        viewport: &mut Viewport,
        state: &egui_wgpu::RenderState,
        workspace: &Workspace,
        preparation: &PreparationUi,
        appearance: &Appearance,
        focus: Option<usize>,
    ) -> Vec<Draw> {
        // Checked again once the plate changed, when the object has a probe.
        let revision = workspace.revision();
        for position in 0..self.pending.len() {
            if self.pending[position]
                .verdict
                .as_ref()
                .is_none_or(|(checked, _)| *checked != revision)
            {
                self.recheck(workspace, preparation, position);
            }
        }
        let dragged = self.dragging.map(|d| d.which);
        let mut draws = Vec::new();
        for (position, pending) in self.pending.iter_mut().enumerate() {
            if let Some(mesh) = pending.mesh.take() {
                self.pending_keys += 1;
                let key = PENDING_KEYS | self.pending_keys & 0xF_FFFF;
                let origin = centre(&mesh);
                viewport.upload(&state.device, key, &mesh, origin);
                pending.solid = Some(Solid { key, origin });
            }
            let (Some(solid), Some(map)) =
                (&pending.solid, workspace.model_to_world(pending.object))
            else {
                continue;
            };
            // Only the focused object's new holes show.
            if dragged == Some(HoleRef::New(position)) || focus != Some(pending.object) {
                continue;
            }
            let wrong = pending.verdict.as_ref().is_some_and(|(_, v)| !v.is_valid());
            let seen = crate::shown::seen(
                self.holes_shown,
                (self.holes_list_open && self.only_refused).then_some(wrong),
                wrong,
                self.selected == Some((pending.object, HoleRef::New(position))),
                false,
            );
            let color = if wrong {
                appearance.support_error
            } else {
                appearance.hole_valid
            };
            match seen {
                Seen::Hidden => {}
                Seen::Ghost => draws.push(ghost_draw(solid, appearance, Some(&matrix(&map)))),
                Seen::Solid => draws.extend(solid_draws(solid, color, Some(&matrix(&map)))),
            }
        }
        draws
    }

    /// The candidate's solid: green where it can be cut, red where it cannot, grey until
    /// probed.
    fn candidate_draw(
        &mut self,
        viewport: &mut Viewport,
        state: &egui_wgpu::RenderState,
        workspace: &Workspace,
        appearance: &Appearance,
    ) -> Option<[Draw; 2]> {
        let Some(candidate) = &self.candidate else {
            if let Some((_, old)) = self.candidate_solid.take() {
                viewport.remove(old.key);
            }
            return None;
        };
        {
            let outdated = self
                .candidate_solid
                .as_ref()
                .is_none_or(|(generation, _)| *generation != candidate.generation);
            if outdated {
                if let Some((_, old)) = self.candidate_solid.take() {
                    viewport.remove(old.key);
                }
                let mesh = match &candidate.verdict {
                    Some(verdict) => verdict_solid(verdict),
                    // Not probed yet: a cylinder into the surface where the pointer is, grey.
                    None => candidate.at.and_then(|at| {
                        let depth = match candidate.hole {
                            DrainHole::Surface { depth_mm, .. } => depth_mm,
                            DrainHole::Axis { .. } => None,
                        };
                        display_cylinder(
                            add(at.point, scale(at.normal, HOLE_MARGIN_MM)),
                            scale(at.normal, -1.0),
                            HOLE_MARGIN_MM
                                + depth.unwrap_or_else(|| {
                                    default_depth(workspace.plate().objects.get(candidate.object))
                                }),
                            candidate.hole.diameter_mm() / 2.0,
                        )
                        .map(SolidMesh::Display)
                    }),
                };
                if let Some(mesh) = mesh {
                    let key = SOLID_KEYS | 0xF_FFFF;
                    let origin = centre(&mesh);
                    viewport.upload(&state.device, key, &mesh, origin);
                    self.candidate_solid = Some((candidate.generation, Solid { key, origin }));
                }
            }
        }
        let (_, solid) = self.candidate_solid.as_ref()?;
        let color = match &candidate.verdict {
            Some(v) if v.is_valid() => appearance.hole_valid,
            Some(_) => appearance.support_error,
            None => appearance.hole_unchecked,
        };
        Some(solid_draws(solid, color, None))
    }

    /// How hole `which` of object `object` shows while the tool is in use (see
    /// [`crate::shown`]); `refused` when it cannot be cut.
    fn hole_seen(&self, object: usize, which: HoleRef, refused: bool) -> Seen {
        crate::shown::seen(
            self.holes_shown,
            (self.holes_list_open && self.only_refused).then_some(refused),
            refused,
            self.selected == Some((object, which)),
            false,
        )
    }

    /// The hole of object `selected`, applied or new, whose solid `ray` meets first.
    fn hole_under(
        &self,
        workspace: &Workspace,
        preparation: &mut PreparationUi,
        ray: &Ray,
        selected: usize,
    ) -> Option<HoleRef> {
        let count = workspace.holes(selected).len();
        // Hidden and ghost holes are not picked: what shows is.
        let applied = preparation
            .verdicts(workspace, selected)
            .into_iter()
            .flat_map(|verdicts| verdicts.iter().take(count).enumerate())
            .filter(|(index, verdict)| {
                self.hole_seen(selected, HoleRef::Applied(*index), !verdict.is_valid())
                    .pickable()
            })
            .filter_map(|(index, verdict)| {
                let mesh = verdict_solid(verdict)?;
                ray_mesh(ray, &IDENTITY, &mesh).map(|(t, _)| (t, HoleRef::Applied(index)))
            });
        let new = (0..self.pending.len())
            .filter(|&p| self.pending[p].object == selected)
            .filter_map(|position| {
                let verdict = self.new_verdict(workspace, position)?;
                if !self
                    .hole_seen(selected, HoleRef::New(position), !verdict.is_valid())
                    .pickable()
                {
                    return None;
                }
                let mesh = verdict_solid(verdict)?;
                ray_mesh(ray, &IDENTITY, &mesh).map(|(t, _)| (t, HoleRef::New(position)))
            });
        applied
            .collect::<Vec<_>>()
            .into_iter()
            .chain(new)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, which)| which)
    }

    /// A click in the viewport: while placing, it adds the candidate as a new hole when it can
    /// be cut, and selects it; otherwise a click on a hole's solid selects it. Returns whether
    /// the click was taken.
    pub fn click(
        &mut self,
        workspace: &mut Workspace,
        preparation: &mut PreparationUi,
        ray: &Ray,
        selected: usize,
    ) -> bool {
        if self.placing {
            if let Some(candidate) = self.candidate.take() {
                self.place(workspace, candidate);
            }
            // While placing, clicks never select or clear: a refused place is shown, not taken.
            return true;
        }
        match self.hole_under(workspace, preparation, ray, selected) {
            Some(which) => {
                self.selected = Some((selected, which));
                self.reveal_hole = true;
                true
            }
            None => false,
        }
    }

    /// Keep `candidate` as a new hole, when it can be cut, with its solid in the model frame,
    /// and select it so it can be shaped at once.
    fn place(&mut self, workspace: &Workspace, candidate: Candidate) {
        let Some(verdict) = candidate.verdict.filter(Verdict::is_valid) else {
            return;
        };
        self.check_pending(workspace);
        self.pending.push(Pending {
            object: candidate.object,
            hole: candidate.hole,
            verdict: None,
            mesh: None,
            solid: None,
        });
        let position = self.pending.len() - 1;
        self.set_new_verdict(workspace, position, verdict);
        self.pending_among = workspace.plate().objects.len();
        self.selected = Some((candidate.object, HoleRef::New(position)));
        self.reveal_hole = true;
    }

    /// A press that starts a drag at `pointer` (screen) along `ray`: on a handle of the
    /// selected hole, on a ring of its turn gizmo `rings` (about the axes of `space`, as
    /// [`Self::rings`] gave them), or on a hole's solid. While placing, only the handles and
    /// rings are taken: a click places. Returns whether the drag was taken.
    #[allow(clippy::too_many_arguments)] // The press's whole context.
    pub fn press(
        &mut self,
        workspace: &Workspace,
        preparation: &mut PreparationUi,
        camera: &Camera,
        rect: egui::Rect,
        pointer: egui::Pos2,
        ray: &Ray,
        selected: usize,
        rings: &[Handle],
        space: Space,
    ) -> bool {
        let size = [f64::from(rect.width()), f64::from(rect.height())];
        let on_handle = self
            .handles(workspace, preparation)
            .into_iter()
            .filter_map(|(grip, at)| {
                let [x, y] = camera.screen(at, size)?;
                #[allow(clippy::cast_possible_truncation)] // Screen positions.
                let at = rect.min + egui::vec2(x as f32, y as f32);
                let distance = at.distance(pointer);
                (distance <= HANDLE_REACH).then_some((distance, grip))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, grip)| grip)
            .or_else(|| {
                let local = pointer - rect.min;
                let at = [f64::from(local.x), f64::from(local.y)];
                let ring = manipulate::hit(rings, camera, size, at, RING_REACH)?;
                match rings[ring].kind {
                    Kind::Ring(k) => Some(Grip::Ring(k)),
                    _ => None,
                }
            });
        let (which, grip) = match (on_handle, self.selected) {
            (Some(grip), Some((object, which))) if object == selected => (which, grip),
            _ if self.placing => return false,
            _ => match self.hole_under(workspace, preparation, ray, selected) {
                Some(which) => (which, Grip::Body),
                None => return false,
            },
        };
        let (axis, frame) = match self.shape(workspace, preparation, selected, which) {
            Some((spec, frame)) => (unit(spec.direction), Some(frame)),
            None => (None, None),
        };
        self.selected = Some((selected, which));
        self.reveal_hole = true;
        self.candidate = None;
        let outward = frame.map_or([0.0, 0.0, 1.0], |f| f.outward);
        self.dragging = Some(HoleDrag {
            object: selected,
            which,
            grip,
            frame,
            axis: axis.unwrap_or([0.0, 0.0, -1.0]),
            start_ray: *ray,
            axes: ring_axes(space, outward),
            snap: false,
        });
        true
    }

    /// The drag ended: the dragged hole takes its new shape where it can be cut, or stays as it
    /// was. An applied hole changes by a plate edit, a new one in the panel.
    pub fn release(&mut self, workspace: &mut Workspace) {
        let Some(drag) = self.dragging.take() else {
            return;
        };
        let Some(candidate) = self.candidate.take() else {
            return;
        };
        let Some(verdict) = candidate.verdict.filter(Verdict::is_valid) else {
            return;
        };
        match drag.which {
            HoleRef::Applied(index) => {
                workspace.set_hole(drag.object, index, candidate.hole, None);
            }
            HoleRef::New(position) => {
                if let Some(pending) = self.pending.get_mut(position) {
                    pending.hole = candidate.hole;
                    self.set_new_verdict(workspace, position, verdict);
                }
            }
        }
    }

    /// Why the candidate would be refused, for the panel.
    fn candidate_refusal(&self) -> Option<&str> {
        self.candidate
            .as_ref()?
            .verdict
            .as_ref()?
            .refusal
            .as_deref()
    }

    /// The draft of object `index`, whose applied hollowing is `applied`: kept while it is
    /// the same object and its hollowing has not changed since, started again otherwise, from
    /// the applied hollowing or the tool's.
    fn draft(&mut self, index: usize, applied: Option<Hollowing>) -> &mut Draft {
        let draft = self
            .draft
            .filter(|d| d.object == index && d.base == applied)
            .unwrap_or(Draft {
                object: index,
                base: applied,
                edit: applied.unwrap_or(self.new_hollowing),
            });
        self.draft.insert(draft)
    }

    /// The Repair section's markers for object `index` (see
    /// [`crate::repair_panel::RepairPanel::draws`]).
    #[allow(clippy::too_many_arguments)] // The viewport, the plate and the eye.
    pub fn repair_draws(
        &mut self,
        viewport: &mut Viewport,
        state: &egui_wgpu::RenderState,
        workspace: &mut Workspace,
        preparation: &PreparationUi,
        index: Option<usize>,
        eye: [f64; 3],
        appearance: &Appearance,
    ) -> Vec<Draw> {
        self.repair.draws(
            viewport,
            state,
            workspace,
            preparation,
            index,
            eye,
            appearance,
        )
    }

    /// What the repair marker under `pointer` marks, if any.
    #[must_use]
    pub fn repair_marker_at(
        &self,
        camera: &Camera,
        rect: egui::Rect,
        pointer: egui::Pos2,
    ) -> Option<String> {
        self.repair
            .marker_at(camera, rect, pointer)
            .map(str::to_owned)
    }

    /// A world point the Repair section asked to frame, once.
    pub fn take_focus(&mut self) -> Option<[f64; 3]> {
        self.repair.take_focus()
    }

    /// The Repair panel's Advanced switch.
    pub fn repair_advanced(&mut self) -> &mut bool {
        &mut self.repair.advanced
    }

    /// The tool's hollowing: what a model never hollowed starts from (a user setting).
    pub fn new_hollowing(&mut self) -> &mut Hollowing {
        &mut self.new_hollowing
    }

    /// The next drain hole's diameter, depth (`None`: the object's default) and starting
    /// surface: the tool's values (user settings).
    pub fn next_hole(&mut self) -> (&mut f64, &mut Option<f64>, &mut HoleSurface) {
        (&mut self.hole_mm, &mut self.depth_mm, &mut self.start)
    }

    /// Whether drain holes are being placed (the tool strip's Place, owner 2026-10-29).
    #[must_use]
    pub fn is_placing(&self) -> bool {
        self.placing
    }

    /// Start or stop placing drain holes.
    pub fn toggle_placing(&mut self) {
        self.placing = !self.placing;
        self.candidate = None;
    }

    /// Whether a drain hole is selected, which Delete removes.
    #[must_use]
    pub fn hole_selected(&self) -> bool {
        self.selected.is_some()
    }

    /// The Repair panel's footer: the selected object's state.
    pub fn repair_footer(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &mut Workspace,
        preparation: &PreparationUi,
        index: Option<usize>,
        icons: Option<&crate::icons::Icons>,
    ) {
        self.repair.footer(ui, workspace, preparation, index, icons);
    }

    /// What applied repairs, hollowing and holes did, once prepared.
    pub fn take_messages(
        &mut self,
        workspace: &Workspace,
        preparation: &PreparationUi,
    ) -> Vec<crate::messages::Message> {
        let mut messages = self.repair.take_messages(workspace, preparation);
        self.report_applied(workspace, preparation);
        messages.append(&mut self.outbox);
        messages.append(&mut self.cups.outbox);
        messages
    }

    /// The Repair tool's panel, for the selected object `index` if any (TD-098).
    pub fn repair_panel(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &mut Workspace,
        preparation: &PreparationUi,
        index: Option<usize>,
        appearance: &Appearance,
    ) {
        self.repair
            .section(ui, workspace, preparation, index, appearance);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::panel::{notes, predicted_limit};
    use super::*;
    use crate::i18n::tr;
    use crate::viewport::Section;
    use karasu_core::preparation::sampled::SampledHollowCertificate;
    use std::time::{Duration, Instant};

    /// Wait for the preparation of the plate as it is now.
    pub(crate) fn prepared_within(
        workspace: &Workspace,
        preparation: &mut PreparationUi,
        limit: Duration,
    ) {
        let started = Instant::now();
        loop {
            preparation.poll(workspace);
            if preparation.current(workspace).is_some() {
                return;
            }
            assert!(started.elapsed() < limit, "no preparation");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The model at `path` on a preset printer, hollowed as given, prepared within `limit`.
    pub(crate) fn placed(
        path: std::path::PathBuf,
        hollowing: Option<Hollowing>,
        limit: Duration,
    ) -> (Workspace, PreparationUi) {
        let mut workspace = Workspace::new();
        workspace.open(path);
        crate::workspace::tests::wait(&mut workspace);
        let (printer, resin, setting) =
            crate::workspace::tests::catalog_profiles("anycubic-photon-mono-x");
        workspace.edit(|plate| {
            plate.printer = Some(printer);
            plate.resin = Some(resin);
            plate.print_setting = Some(setting);
            plate.objects[0].set_hollowing(hollowing);
        });
        // As while holes are placed on it: an object without holes has no probe otherwise.
        let mut preparation = PreparationUi::default();
        preparation.set_probes(vec![0]);
        prepared_within(&workspace, &mut preparation, limit);
        (workspace, preparation)
    }

    /// A 20 mm cube written to a new directory, and that directory.
    pub(crate) fn cube_file(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        // Tests run in parallel and remove their directory: each call gets its own.
        static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let call = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "karasu-holes-ui-{}-{name}-{call}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("cube.stl");
        let stl = crate::workspace::tests::box_stl([0.0; 3], [20.0; 3]);
        std::fs::write(&path, stl).unwrap();
        (path, directory)
    }

    /// A 20 mm cube on a preset printer, hollowed at 2 mm when `hollow`, prepared.
    pub(crate) fn cube(hollow: bool) -> (Workspace, PreparationUi, std::path::PathBuf) {
        let (path, directory) = cube_file(&hollow.to_string());
        let hollowing = hollow.then_some(Hollowing {
            wall_mm: 2.0,
            cell_mm: Some(0.25),
            simplify: None,
        });
        let (workspace, preparation) = placed(path, hollowing, Duration::from_secs(60));
        (workspace, preparation, directory)
    }

    /// The certificate of object 0's hollowing in the current preparation.
    fn certificate(workspace: &Workspace, preparation: &PreparationUi) -> SampledHollowCertificate {
        let current = preparation.current(workspace).unwrap();
        let object = current.objects[0].as_ref().unwrap();
        object.hollowed().unwrap().certificate()
    }

    /// A surface hole's point.
    fn surface_point(hole: &DrainHole) -> Option<[f64; 3]> {
        match hole {
            DrainHole::Surface { surface_mm, .. } => Some(*surface_mm),
            DrainHole::Axis { .. } => None,
        }
    }

    fn hit(point: [f64; 3]) -> SurfaceHit {
        SurfaceHit {
            object: 0,
            point,
            normal: [-1.0, 0.0, 0.0],
        }
    }

    fn verdict_is_valid(ui: &HollowingUi) -> Option<bool> {
        let candidate = ui.candidate.as_ref()?;
        candidate.verdict.as_ref().map(Verdict::is_valid)
    }

    /// A ray straight at the -X face, from outside.
    fn ray_at(y: f64, z: f64) -> Ray {
        Ray {
            origin: [-50.0, y, z],
            direction: [1.0, 0.0, 0.0],
        }
    }

    fn hole_of(w: &Workspace) -> DrainHole {
        w.holes(0)[0]
    }

    fn hover_at(
        ui: &mut HollowingUi,
        workspace: &Workspace,
        preparation: &mut PreparationUi,
        y: f64,
        z: f64,
    ) -> Ray {
        let ray = ray_at(y, z);
        ui.hover(
            workspace,
            preparation,
            0,
            Some(&ray),
            Some(hit([0.0, y, z])),
            None,
        );
        ray
    }

    #[test]
    fn new_holes_wait_to_be_applied_and_applied_ones_are_dragged_back_when_refused() {
        let (mut workspace, mut preparation, directory) = cube(true);
        let mut ui = HollowingUi {
            placing: true,
            ..HollowingUi::default()
        };
        let holes = |w: &Workspace| w.plate().objects[0].drain_holes().count();
        // A new hole goes past the 2 mm wall by default: at the -X face's centre it reaches
        // the cavity.
        let centre = hover_at(&mut ui, &workspace, &mut preparation, 10.0, 10.0);
        assert_eq!(verdict_is_valid(&ui), Some(true));
        assert!(matches!(
            ui.candidate.as_ref().unwrap().hole,
            DrainHole::Surface {
                depth_mm: Some(3.0),
                ..
            }
        ));
        // 1.6 mm from the edge the axis passes beside the cavity and ends in the wall: it can
        // be cut, and says so.
        let edge = hover_at(&mut ui, &workspace, &mut preparation, 1.6, 10.0);
        assert_eq!(verdict_is_valid(&ui), Some(true));
        let note = ui
            .candidate
            .as_ref()
            .and_then(|c| c.verdict.as_ref()?.findings.as_ref())
            .map(notes)
            .unwrap();
        assert!(note.contains(&tr!("hole-finding-in-material")), "{note:?}");
        // A click keeps it as a new hole, not cut yet; discarding drops it.
        assert!(ui.click(&mut workspace, &mut preparation, &edge, 0));
        assert_eq!((holes(&workspace), ui.pending.len()), (0, 1));
        ui.discard(Some(0));
        assert!(ui.pending.is_empty());
        // At the centre, then applied: one plate edit, and the hole is cut.
        hover_at(&mut ui, &workspace, &mut preparation, 10.0, 10.0);
        assert!(ui.click(&mut workspace, &mut preparation, &centre, 0));
        ui.apply_pending(&mut workspace, 0);
        assert_eq!((holes(&workspace), ui.pending.len()), (1, 0));
        // Adding a hole changed only holes: the probe still answers at once, and a second hole
        // overlapping the first is accepted (TD-089: holes may overlap).
        hover_at(&mut ui, &workspace, &mut preparation, 11.0, 10.0);
        assert_eq!(verdict_is_valid(&ui), Some(true));
        assert!(preparation.verdicts(&workspace, 0).unwrap()[0].is_valid());
        // Not placing, a click on the hole's solid selects it, and a click beside it does not.
        ui.placing = false;
        ui.candidate = None;
        assert!(ui.click(&mut workspace, &mut preparation, &centre, 0));
        assert_eq!(ui.selected, Some((0, HoleRef::Applied(0))));
        assert!(!ui.click(&mut workspace, &mut preparation, &ray_at(16.0, 16.0), 0));
        // With an automatic depth, a hole dragged to the edge finds no cavity: released there,
        // it stays where it was.
        let DrainHole::Surface {
            surface_mm,
            diameter_mm,
            outside_mm,
            inside_mm,
            tilt,
            surface,
            ..
        } = hole_of(&workspace)
        else {
            unreachable!()
        };
        let automatic = DrainHole::Surface {
            surface_mm,
            diameter_mm,
            outside_mm,
            inside_mm,
            tilt,
            depth_mm: None,
            surface,
        };
        workspace.set_hole(0, 0, automatic, None);
        let camera = Camera::new([0.0; 3], 100.0);
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let far = egui::pos2(-1000.0, -1000.0);
        assert!(press(
            &mut ui,
            &workspace,
            &mut preparation,
            &camera,
            rect,
            far,
            &centre
        ));
        hover_at(&mut ui, &workspace, &mut preparation, 1.6, 10.0);
        assert_eq!(verdict_is_valid(&ui), Some(false));
        assert!(ui.candidate_refusal().is_some());
        ui.release(&mut workspace);
        assert_eq!(surface_point(&hole_of(&workspace)), Some([0.0, 10.0, 10.0]));
        // Dragged to a place it can be cut, it moves there, in one undo step.
        assert!(press(
            &mut ui,
            &workspace,
            &mut preparation,
            &camera,
            rect,
            far,
            &centre
        ));
        hover_at(&mut ui, &workspace, &mut preparation, 8.0, 12.0);
        ui.release(&mut workspace);
        assert_eq!(surface_point(&hole_of(&workspace)), Some([0.0, 8.0, 12.0]));
        workspace.undo();
        assert_eq!(surface_point(&hole_of(&workspace)), Some([0.0, 10.0, 10.0]));
        // Delete removes the selected hole; Escape then releases nothing more.
        assert!(ui.delete(&mut workspace));
        assert_eq!(holes(&workspace), 0);
        assert!(!ui.escape());
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn applied_holes_follow_the_first_move_while_it_is_prepared() {
        // Owner, 2026-10-22: holes applied, then the object moved for the first time. Their
        // verdicts were taken with the previous probe before the holes were prepared; the
        // finished preparation must not leave the moved object without holes to show.
        let (mut workspace, mut preparation, directory) = cube(true);
        let mut ui = HollowingUi {
            placing: true,
            ..HollowingUi::default()
        };
        let centre = hover_at(&mut ui, &workspace, &mut preparation, 10.0, 10.0);
        assert!(ui.click(&mut workspace, &mut preparation, &centre, 0));
        ui.apply_pending(&mut workspace, 0);
        // Drawn at once, with the probe of the preparation before the holes.
        assert!(preparation.verdicts(&workspace, 0).is_some());
        prepared_within(&workspace, &mut preparation, Duration::from_secs(60));
        assert!(preparation.verdicts(&workspace, 0).is_some());
        workspace.edit(|plate| plate.objects[0].translate_mm[0] += 5.0);
        assert!(preparation.verdicts(&workspace, 0).is_none());
        let (_, held, _) = preparation.held_verdicts(&workspace, 0).unwrap();
        assert_eq!(held.len(), 1);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn new_holes_are_shaped_and_moved_before_they_are_applied() {
        let (mut workspace, mut preparation, directory) = cube(true);
        let mut ui = HollowingUi {
            placing: true,
            ..HollowingUi::default()
        };
        // A placed hole is new and selected at once, with its handles.
        let centre = hover_at(&mut ui, &workspace, &mut preparation, 10.0, 10.0);
        assert!(ui.click(&mut workspace, &mut preparation, &centre, 0));
        assert_eq!(ui.selected, Some((0, HoleRef::New(0))));
        assert_eq!(ui.handles(&workspace, &mut preparation).len(), 3);
        // Widened in the panel, it is checked again at once.
        if let DrainHole::Surface { diameter_mm, .. } = &mut ui.pending[0].hole {
            *diameter_mm = 5.0;
        }
        ui.recheck(&workspace, &preparation, 0);
        let spec = ui.new_verdict(&workspace, 0).unwrap().spec.unwrap();
        assert!((spec.diameter_mm - 5.0).abs() < 1e-9);
        // Dragged by its solid, it moves and stays new: nothing is cut yet.
        ui.placing = false;
        ui.candidate = None;
        let camera = Camera::new([0.0; 3], 100.0);
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let far = egui::pos2(-1000.0, -1000.0);
        assert!(press(
            &mut ui,
            &workspace,
            &mut preparation,
            &camera,
            rect,
            far,
            &centre
        ));
        hover_at(&mut ui, &workspace, &mut preparation, 8.0, 12.0);
        ui.release(&mut workspace);
        assert_eq!(surface_point(&ui.pending[0].hole), Some([0.0, 8.0, 12.0]));
        assert!(ui.new_verdict(&workspace, 0).unwrap().is_valid());
        assert_eq!(workspace.holes(0).len(), 0);
        // Delete drops the selected new hole.
        assert!(ui.delete(&mut workspace));
        assert!(ui.pending.is_empty());
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn holes_start_from_the_cavity_where_the_section_shows_it() {
        let (workspace, mut preparation, directory) = cube(true);
        // Looking down into the cube cut at 12 mm: the cavity floor, 2 mm up, is seen.
        let down = Ray {
            origin: [10.0, 10.0, 50.0],
            direction: [0.0, 0.0, -1.0],
        };
        let cut = Section {
            below: None,
            above: Some(12.0),
        };
        let floor = preparation.cavity_hit(&workspace, 0, &down, cut).unwrap();
        assert!((floor.point[2] - 2.0).abs() < 0.3, "{floor:?}");
        // Uncut, the ray meets the cavity's ceiling from the material side: not seen.
        let ceiling = preparation.cavity_hit(&workspace, 0, &down, Section::default());
        assert!(ceiling.is_none_or(|h| (h.point[2] - 2.0).abs() < 0.3));
        // A hole from the floor goes out through the bottom wall.
        let mut ui = HollowingUi {
            placing: true,
            start: HoleSurface::Cavity,
            ..HollowingUi::default()
        };
        assert!(ui.wants_cavity(&workspace));
        ui.hover(
            &workspace,
            &mut preparation,
            0,
            Some(&down),
            None,
            Some(floor),
        );
        let verdict = ui.candidate.as_ref().unwrap().verdict.as_ref().unwrap();
        assert!(verdict.is_valid(), "{verdict:?}");
        let findings = verdict.findings.as_ref().unwrap();
        assert!(
            findings.opens_outside && findings.reaches_cavity,
            "{findings:?}"
        );
        assert!(verdict.spec.unwrap().direction[2] < -0.99);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn solid_objects_take_holes_once_probed() {
        let (mut workspace, mut preparation, directory) = cube(false);
        let mut ui = HollowingUi {
            placing: true,
            ..HollowingUi::default()
        };
        let centre = hover_at(&mut ui, &workspace, &mut preparation, 10.0, 10.0);
        let candidate = ui.candidate.as_ref().unwrap();
        // 3 mm into the solid: it opens, reaches no cavity and ends in the material.
        assert!(matches!(
            candidate.hole,
            DrainHole::Surface {
                depth_mm: Some(SOLID_HOLE_DEPTH_MM),
                ..
            }
        ));
        let findings = candidate
            .verdict
            .as_ref()
            .unwrap()
            .findings
            .clone()
            .unwrap();
        assert!(findings.opens_outside && !findings.reaches_cavity && findings.ends_in_material);
        assert!(ui.click(&mut workspace, &mut preparation, &centre, 0));
        ui.apply_pending(&mut workspace, 0);
        assert_eq!(workspace.holes(0).len(), 1);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn the_handles_set_the_diameter_the_depth_and_the_tilt() {
        let hole = DrainHole::Surface {
            surface_mm: [0.0, 10.0, 10.0],
            diameter_mm: 3.0,
            outside_mm: 1.0,
            inside_mm: 1.0,
            tilt: None,
            depth_mm: None,
            surface: HoleSurface::Outer,
        };
        let down = |x: f64, y: f64| Ray {
            origin: [x, y, 50.0],
            direction: [0.0, 0.0, -1.0],
        };
        // A hole on the -X face: foot at (0, 10, 10), outward -X, axis +X.
        let drag = |grip| HoleDrag {
            object: 0,
            which: HoleRef::Applied(0),
            grip,
            frame: Some(HoleFrame {
                foot_mm: [0.0, 10.0, 10.0],
                outward: [-1.0, 0.0, 0.0],
            }),
            axis: [1.0, 0.0, 0.0],
            start_ray: down(0.0, 0.0),
            axes: ring_axes(Space::Global, [-1.0, 0.0, 0.0]),
            snap: false,
        };
        let fields = |hole: DrainHole| match hole {
            DrainHole::Surface {
                diameter_mm,
                tilt,
                depth_mm,
                ..
            } => (diameter_mm, tilt, depth_mm),
            DrainHole::Axis { .. } => unreachable!(),
        };
        let dragged = |grip, ray: &Ray, hit| {
            HollowingUi::dragged(hole, &drag(grip), ray, hit, &IDENTITY).unwrap()
        };
        // The end handle, dragged to X = 3.5 above the axis: a fixed depth of 3.5 mm.
        assert_eq!(
            fields(dragged(Grip::End, &down(3.5, 10.0), None)).2,
            Some(3.5)
        );
        // The rim handle, 2.5 mm beside the axis: a 5 mm diameter.
        let rim = dragged(Grip::Rim, &down(2.0, 12.5), None);
        assert!((fields(rim).0 - 5.0).abs() < 1e-9);
        // The tilt handle, moved sideways on its plane 5 mm outside the surface: the axis leans
        // away from the pointer by atan(offset / height).
        let height = 1.0 + TILT_HANDLE_MM;
        let tilted = dragged(
            Grip::Tilt,
            &Ray {
                origin: [-height - 10.0, 10.0 + height, 10.0],
                direction: [1.0, 0.0, 0.0],
            },
            None,
        );
        let tilt = fields(tilted).1.unwrap();
        assert!((tilt.angle_deg - 45.0).abs() < 1e-9, "{tilt:?}");
        // The axis it describes points from the handle through the foot.
        let axis = karasu_core::preparation::sampled::tilted_axis([-1.0, 0.0, 0.0], tilt);
        let expected = unit([height, -height, 0.0]).unwrap();
        for k in 0..3 {
            assert!((axis[k] - expected[k]).abs() < 1e-9, "{axis:?}");
        }
        // The body follows the surface.
        let moved = dragged(Grip::Body, &down(0.0, 8.0), Some(hit([0.0, 8.0, 12.0])));
        assert_eq!(surface_point(&moved), Some([0.0, 8.0, 12.0]));
    }

    /// A press on hole handles or solids of object 0, without the turn gizmo.
    fn press(
        ui: &mut HollowingUi,
        workspace: &Workspace,
        preparation: &mut PreparationUi,
        camera: &Camera,
        rect: egui::Rect,
        at: egui::Pos2,
        ray: &Ray,
    ) -> bool {
        ui.press(
            workspace,
            preparation,
            camera,
            rect,
            at,
            ray,
            0,
            &[],
            Space::Global,
        )
    }

    #[test]
    fn a_ring_turns_the_axis_about_one_axis_alone() {
        let hole = DrainHole::Surface {
            surface_mm: [0.0, 10.0, 10.0],
            diameter_mm: 3.0,
            outside_mm: 1.0,
            inside_mm: 1.0,
            tilt: None,
            depth_mm: None,
            surface: HoleSurface::Outer,
        };
        let outward = [-1.0, 0.0, 0.0];
        let down = |x: f64, y: f64| Ray {
            origin: [x, y, 50.0],
            direction: [0.0, 0.0, -1.0],
        };
        // A hole on the -X face, axis +X; the pointer starts 5 mm along +X of its foot, above.
        let drag = |space, k, snap| HoleDrag {
            object: 0,
            which: HoleRef::Applied(0),
            grip: Grip::Ring(k),
            frame: Some(HoleFrame {
                foot_mm: [0.0, 10.0, 10.0],
                outward,
            }),
            axis: [1.0, 0.0, 0.0],
            start_ray: down(5.0, 10.0),
            axes: ring_axes(space, outward),
            snap,
        };
        let axis_after = |drag: &HoleDrag, ray: &Ray| {
            let turned = HollowingUi::dragged(hole, drag, ray, None, &IDENTITY).unwrap();
            let DrainHole::Surface { tilt, .. } = turned else {
                unreachable!()
            };
            tilt.map_or([1.0, 0.0, 0.0], |t| {
                karasu_core::preparation::sampled::tilted_axis(outward, t)
            })
        };
        let close = |a: [f64; 3], b: [f64; 3]| (0..3).all(|k| (a[k] - b[k]).abs() < 1e-9);
        let at = |degrees: f64| {
            let (sin, cos) = degrees.to_radians().sin_cos();
            down(5.0 * cos, 10.0 + 5.0 * sin)
        };
        let leaning = |degrees: f64| {
            let (sin, cos) = degrees.to_radians().sin_cos();
            [cos, sin, 0.0]
        };
        // About world Z by the pointer's 30 degrees about the foot: the axis turns in XY only.
        let about_z = drag(Space::Global, 2, false);
        assert!(close(axis_after(&about_z, &at(30.0)), leaning(30.0)));
        // Shift: 50 degrees snaps to 45.
        let snapped = drag(Space::Global, 2, true);
        assert!(close(axis_after(&snapped, &at(50.0)), leaning(45.0)));
        // Past the limit the lean stops at it, facing the way it turned.
        let far = axis_after(&about_z, &at(80.0));
        assert!(close(far, leaning(MAX_HOLE_TILT_DEG)), "{far:?}");
        // About the surface's normal, an upright hole stays upright.
        let spun = HoleDrag {
            start_ray: Ray {
                origin: [-50.0, 15.0, 10.0],
                direction: [1.0, 0.0, 0.0],
            },
            ..drag(Space::Local, 2, false)
        };
        let ray = Ray {
            origin: [-50.0, 10.0, 15.0],
            direction: [1.0, 0.0, 0.0],
        };
        let turned = HollowingUi::dragged(hole, &spun, &ray, None, &IDENTITY).unwrap();
        assert!(matches!(turned, DrainHole::Surface { tilt: None, .. }));
    }

    #[test]
    fn a_new_diameter_keeps_the_axis_where_it_was() {
        // TD-121: on the -X face, 2 mm from the -Y edge, a 3 mm hole's normal is the face's; a
        // 6 mm one reaches the -Y face and its normal leans towards it.
        let (mut workspace, preparation, directory) = cube(false);
        let placement = workspace.placements()[0].clone().unwrap();
        let (min, max) = (placement.min, placement.max);
        let foot = [min.x, min.y + 2.0, f64::midpoint(min.z, max.z)];
        let map = workspace.model_to_world(0).unwrap();
        let sized = |diameter_mm| DrainHole::Surface {
            surface_mm: map.inverse().unwrap().point(foot),
            diameter_mm,
            outside_mm: 1.0,
            inside_mm: 1.0,
            tilt: None,
            depth_mm: Some(3.0),
            surface: HoleSurface::Outer,
        };
        let axis = |hole: &DrainHole| {
            let verdict = preparation.verdict(&workspace, 0, hole).unwrap();
            unit(verdict.spec.unwrap().direction).unwrap()
        };
        let before = axis(&sized(3.0));
        assert!(
            (dot(before, [1.0, 0.0, 0.0]) - 1.0).abs() < 1e-9,
            "{before:?}"
        );
        // Widened as it is, the hole turns with the normal.
        let turned = axis(&sized(6.0));
        assert!(dot(turned, before) < 0.99, "{turned:?}");
        // Widened keeping its axis, it leans from the new normal back onto the old axis.
        let kept = keeping_axis(&workspace, &preparation, 0, sized(6.0), before);
        assert!(matches!(kept, DrainHole::Surface { tilt: Some(_), .. }));
        let after = axis(&kept);
        assert!((dot(after, before) - 1.0).abs() < 1e-9, "{after:?}");
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_tilt_set_on_a_turned_object_is_kept_in_its_model_frame() {
        // An object turned a quarter about Z: its +X face is the world's +Y face.
        let map = Affine {
            linear: [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            translation: [5.0, 0.0, 0.0],
        };
        let world = HoleTilt {
            angle_deg: 30.0,
            azimuth_deg: 0.0,
        };
        let model = model_tilt(world, [0.0, 1.0, 0.0], &map).unwrap();
        // The same lean, measured about the model's +X normal.
        let axis = karasu_core::preparation::sampled::tilted_axis([1.0, 0.0, 0.0], model);
        let turned = map.vector(axis);
        let expected = karasu_core::preparation::sampled::tilted_axis([0.0, 1.0, 0.0], world);
        for k in 0..3 {
            assert!((turned[k] - expected[k]).abs() < 1e-9, "{turned:?}");
        }
        assert!((model.angle_deg - 30.0).abs() < 1e-9);
    }

    #[test]
    fn probes_are_asked_for_where_holes_are_placed_or_new() {
        let mut ui = HollowingUi::default();
        assert_eq!(ui.probes(None), Vec::<usize>::new());
        let hole = DrainHole::Axis {
            entry_mm: [0.0; 3],
            direction: [0.0, 0.0, 1.0],
            length_mm: 1.0,
            diameter_mm: 1.0,
        };
        for object in [2, 0, 2] {
            ui.pending.push(Pending {
                object,
                hole,
                verdict: None,
                mesh: None,
                solid: None,
            });
        }
        assert_eq!(ui.probes(None), [0, 2]);
        assert_eq!(ui.probes(Some(1)), [0, 1, 2]);
    }

    #[test]
    fn an_automatic_cell_guarantees_the_wall_set() {
        // TD-093: with the cell left to the rule, the wall shown is proven at least the one set,
        // on a coarser grid than the explicit default.
        let (path, directory) = cube_file("auto");
        let (workspace, preparation) =
            placed(path, Some(DEFAULT_HOLLOWING), Duration::from_secs(60));
        let c = certificate(&workspace, &preparation);
        assert!(c.cell_mm > 0.25, "{c:?}");
        assert!(
            c.surface_distance_mm[0] + 2.0 * c.query_error_mm >= DEFAULT_WALL_MM,
            "{c:?}"
        );
        assert!(c.vertex_distance_mm[1] >= c.surface_distance_mm[0]);
        assert!(c.passes >= 1);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn walls_and_cells_over_the_predicted_memory_are_warned_about() {
        // TD-094: predicted from the model's area, before anything is applied.
        let (mut workspace, _preparation, directory) = cube(false);
        assert!(workspace
            .meshes()
            .values()
            .all(|m| (m.area_mm2 - 2400.0).abs() < 1e-6));
        workspace.set_memory_budget(None);
        assert!(predicted_limit(&workspace, 0, &DEFAULT_HOLLOWING).is_none());
        let fine = Hollowing {
            cell_mm: Some(0.25),
            ..DEFAULT_HOLLOWING
        };
        assert!(predicted_limit(&workspace, 0, &fine).is_none());
        // A budget of a thousand cavity triangles.
        let bytes = karasu_core::preparation::sampled::ProbeLimits::BYTES_PER_FACE * 1000;
        workspace.set_memory_budget(Some(bytes));
        let warning = predicted_limit(&workspace, 0, &DEFAULT_HOLLOWING).unwrap();
        assert!(warning.contains("predicted"), "{warning}");
        assert_eq!(
            predicted_limit(&workspace, 0, &fine),
            Some(tr!("prepare-cell-too-fine"))
        );
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn holes_are_known_to_be_unavailable_before_any_is_placed() {
        // TD-094: with a memory budget no hole index fits in, the prepared object says so at
        // once, and stays prepared.
        let (mut workspace, mut preparation, directory) = cube(false);
        assert!(preparation.drainage_unavailable(&workspace, 0).is_none());
        workspace.set_memory_budget(Some(64));
        // A budget is not a plate edit: the next edit prepares the plate again under it.
        workspace.edit(|plate| plate.objects[0].translate_mm[0] += 1.0);
        prepared_within(&workspace, &mut preparation, Duration::from_secs(60));
        let reason = preparation.drainage_unavailable(&workspace, 0).unwrap();
        assert!(reason.contains("cannot index"), "{reason}");
        assert!(preparation.current(&workspace).unwrap().objects[0].is_ok());
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// Hollows a local model from the panel's defaults and drains it with a hole placed from
    /// above (TD-093, M6.4e h4): `KARASU_INTERFACE_MODEL` names the STL file,
    /// `KARASU_INTERFACE_WALL` the wall (default 1 mm). Run it in release:
    /// `cargo test --release -p karasu-app a_local_model -- --ignored --nocapture`.
    #[test]
    #[ignore = "hollows a local large model; see the documentation"]
    fn a_local_model_is_hollowed_and_drained_from_the_panel() {
        let Ok(path) = std::env::var("KARASU_INTERFACE_MODEL") else {
            eprintln!("KARASU_INTERFACE_MODEL is not set: nothing to hollow");
            return;
        };
        let wall = std::env::var("KARASU_INTERFACE_WALL").map_or(1.0, |w| w.parse().unwrap());
        let limit = Duration::from_secs(600);
        let started = Instant::now();
        let hollowing = Hollowing {
            wall_mm: wall,
            ..DEFAULT_HOLLOWING
        };
        let (mut workspace, mut preparation) = placed(path.into(), Some(hollowing), limit);
        let c = certificate(&workspace, &preparation);
        println!(
            "hollowed in {:.1} s: cell {:.3} mm, {} passes, guaranteed {:.4} mm, thickest {:.4} mm",
            started.elapsed().as_secs_f64(),
            c.cell_mm,
            c.passes,
            c.surface_distance_mm[0],
            c.vertex_distance_mm[1]
        );
        assert!(
            c.surface_distance_mm[0] + 2.0 * c.query_error_mm >= wall,
            "{c:?}"
        );
        // The first place on a grid over the top where a new hole drains the cavity.
        let placement = workspace.placements()[0].clone().unwrap();
        let (min, max) = (placement.min, placement.max);
        let mut ui = HollowingUi {
            placing: true,
            ..HollowingUi::default()
        };
        let ray = (1..10)
            .flat_map(|i| (1..10).map(move |j| (i, j)))
            .map(|(i, j)| Ray {
                origin: [
                    min.x + (max.x - min.x) * f64::from(i) / 10.0,
                    min.y + (max.y - min.y) * f64::from(j) / 10.0,
                    max.z + 10.0,
                ],
                direction: [0.0, 0.0, -1.0],
            })
            .find(|ray| {
                let hit = workspace.hit(ray);
                ui.hover(&workspace, &mut preparation, 0, Some(ray), hit, None);
                let verdict = ui.candidate.as_ref().and_then(|c| c.verdict.as_ref());
                verdict
                    .and_then(|v| v.findings.as_ref())
                    .is_some_and(|f| f.opens_outside && f.reaches_cavity)
            })
            .expect("no place on the top takes a draining hole");
        assert!(ui.click(&mut workspace, &mut preparation, &ray, 0));
        // What keeping the axis adds to each step of a rim drag (TD-121), beside a verdict.
        let placed = ui.pending[0].hole;
        for diameter_mm in [2.0, 5.0, 10.0] {
            let mut hole = placed;
            if let DrainHole::Surface { diameter_mm: d, .. } = &mut hole {
                *d = diameter_mm;
            }
            let started = Instant::now();
            for _ in 0..100 {
                assert!(preparation.hole_outward(&workspace, 0, &hole).is_some());
            }
            let outward = started.elapsed().as_secs_f64() * 10.0;
            let started = Instant::now();
            assert!(preparation.verdict(&workspace, 0, &hole).is_some());
            println!(
                "{diameter_mm} mm hole: normal {outward:.3} ms, verdict {:.1} ms",
                started.elapsed().as_secs_f64() * 1000.0
            );
        }
        let started = Instant::now();
        ui.apply_pending(&mut workspace, 0);
        prepared_within(&workspace, &mut preparation, limit);
        println!("drilled in {:.1} s", started.elapsed().as_secs_f64());
        let current = preparation.current(&workspace).unwrap();
        let holes = current.objects[0].as_ref().unwrap().holes().unwrap();
        assert!(holes.regions().iter().any(|r| !r.drained_by.is_empty()));
        // Moved: the cavity is reused, the holes are found and cut again.
        let started = Instant::now();
        workspace.edit(|plate| plate.objects[0].translate_mm[0] += 5.0);
        prepared_within(&workspace, &mut preparation, limit);
        println!("moved in {:.1} s", started.elapsed().as_secs_f64());
    }

    #[test]
    fn holes_are_blue_only_while_edited_and_otherwise_take_their_models_colour() {
        let a = Appearance::default();
        // Edited (the Drain holes tool on their model): blue, the selected one lighter.
        assert_eq!(applied_colour(&a, true, true, true, false), a.drain_hole);
        assert_eq!(applied_colour(&a, true, true, true, true), a.hole_selected);
        // Elsewhere only openings, in their model's colour.
        assert_eq!(applied_colour(&a, true, false, false, false), a.object);
        assert_eq!(applied_colour(&a, true, false, true, false), a.selected);
        // A refused hole stays red everywhere.
        assert_eq!(
            applied_colour(&a, false, false, false, false),
            a.support_error
        );
        assert_eq!(applied_colour(&a, false, true, true, true), a.support_error);
    }

    #[test]
    fn holes_follow_the_strip_s_show_and_the_list_s_filter() {
        use crate::shown::Shown;
        let mut ui = HollowingUi::default();
        let [one, two] = [HoleRef::Applied(0), HoleRef::New(0)];
        assert_eq!(ui.hole_seen(0, one, false), Seen::Solid);
        // Only the refused ones: the others hidden, the selected one kept.
        ui.holes_shown = Shown::Errors;
        assert_eq!(ui.hole_seen(0, one, false), Seen::Hidden);
        assert_eq!(ui.hole_seen(0, two, true), Seen::Solid);
        ui.selected = Some((0, one));
        assert_eq!(ui.hole_seen(0, one, false), Seen::Solid);
        // The list's Refused, every hole shown: the others as ghosts.
        ui.holes_shown = Shown::All;
        ui.selected = None;
        ui.holes_list_open = true;
        ui.only_refused = true;
        assert_eq!(ui.hole_seen(0, one, false), Seen::Ghost);
        assert_eq!(ui.hole_seen(0, two, true), Seen::Solid);
        ui.holes_list_open = false;
        assert_eq!(ui.hole_seen(0, one, false), Seen::Solid);
    }

    #[test]
    fn the_hollowing_draft_starts_again_when_the_object_or_its_hollowing_changes() {
        let mut ui = HollowingUi::default();
        ui.draft(0, None).edit.wall_mm = 3.0;
        assert_eq!(ui.draft(0, None).edit.wall_mm, 3.0);
        // Applied: the draft is the applied hollowing.
        let applied = Hollowing {
            wall_mm: 3.0,
            ..DEFAULT_HOLLOWING
        };
        assert_eq!(ui.draft(0, Some(applied)).edit, applied);
        // Another object starts from its own.
        assert_eq!(ui.draft(1, None).edit, DEFAULT_HOLLOWING);
        // New holes go past the wall at the object's scale, or 3 mm into a solid one.
        let mut object = PlateObject::new("o", karasu_project::project::ModelId([0; 32]));
        assert_eq!(default_depth(Some(&object)), SOLID_HOLE_DEPTH_MM);
        object.set_hollowing(Some(applied));
        object.pose.scale = [1.0, 2.0, 1.0];
        assert_eq!(default_depth(Some(&object)), 7.0);
        assert_ne!(merge_key(0, 16), merge_key(1, 16));
    }
}
