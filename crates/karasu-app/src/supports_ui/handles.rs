//! The selected support's handles (TD-108, owner 2026-10-27): its tip slides on the model's
//! surface, its foot moves on the plate or the model, its direction turns the tip (and a foot
//! on the model has a direction of its own), and its knee (owner,
//! 2026-10-29) raises or lowers the top of the vertical shaft: the shaft stays where it stands
//! and the tip leans from its contact to the knee, so a support never moves towards its
//! neighbours.
//!
//! While a handle is dragged, the support is shown as it would be, built alone against a small
//! triangle of the surface at its tip (as the placing preview, about 0.05 ms), in place of its
//! built parts; released, it changes the model's support document as one undoable step and is
//! built against the whole surface. Escape puts it back. The handles are painted over the
//! viewport as the drain holes' are: small marks a constant size on screen.
use super::{alone, height::TipAt, Preview, SupportsUi, FACING_UP};
use crate::{
    appearance::Appearance,
    camera::{Camera, Ray},
    gizmo::{along_axis, Affine},
    viewport::Section,
    workspace::SurfaceHit,
    workspace::Workspace,
};
use eframe::egui;
use karasu_core::supports::{
    AxisSpec, BaseSpec, ManualSupport, Preset, SupportDocument, SupportKind, SupportPart,
    SupportScene,
};

/// How far from a handle a press still takes it, in points.
const HANDLE_REACH: f32 = 10.0;
/// How far beyond the tip's join the direction handle stands, in mm.
const DIRECTION_REACH_MM: f64 = 2.0;
/// How far above a foot's join on the model a knee stays at least, in mm.
const KNEE_ABOVE_FOOT_MM: f64 = 0.5;
/// How far above a foot on the model a ray starts to find the surface's normal there, in mm.
const FOOT_PROBE_MM: f64 = 0.2;
/// How high above the plate (or the raft) a knee stays at least, in mm: room for the pad and
/// its cone.
const KNEE_FLOOR_MM: f64 = 2.0;
/// How far beyond the tip's join a knee stays at least, in mm: the slanted part keeps a shaft.
const KNEE_PAST_JOIN_MM: f64 = 0.5;

/// The height of a knee, from `z` the pointer asked for: at least `floor` (room for the pad and
/// its cone above the plate) and at most where the slanted part from the `tip` to the knee,
/// standing at `xy`, is 45° from vertical (the steepest a shaft prints, TD-062) and the knee
/// is past the tip's `length_mm`; the floor wins where they cross.
fn knee_height(z: f64, floor: f64, tip: [f64; 3], xy: [f64; 2], length_mm: f64) -> f64 {
    let across = (xy[0] - tip[0]).hypot(xy[1] - tip[1]);
    let reach = length_mm + KNEE_PAST_JOIN_MM;
    let drop = (reach * reach - across * across).max(0.0).sqrt();
    z.min(tip[2] - drop.max(across)).max(floor)
}

/// A handle of the selected support.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Grip {
    /// The tip, on the model's surface.
    Tip,
    /// The foot, on the plate.
    Foot,
    /// The tip's direction.
    Direction,
    /// The knee, where the shaft turns straight down.
    Knee,
    /// The direction of a foot on the model, which turns it as the tip's direction turns the
    /// tip (owner, 2026-10-29).
    FootDirection,
    /// Where a branch joins its trunk, which slides along the trunk's shaft (owner, 2026-10-30).
    Junction,
}

/// How a dragged handle is held to a line or a plane (owner, 2026-10-29): along one of the
/// world's axes, whatever the view, or free on the horizontal plane through it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Lock {
    /// Along `direction` through `origin`, where the pointer's ray was `start` along it when
    /// pressed.
    Axis {
        direction: [f64; 3],
        origin: [f64; 3],
        start: f64,
    },
    /// On the horizontal plane through `origin`.
    Level { origin: [f64; 3] },
}

impl Lock {
    /// Where `ray` asks the handle to be.
    fn point(self, ray: &Ray) -> Option<[f64; 3]> {
        match self {
            Self::Axis {
                direction,
                origin,
                start,
            } => {
                let t = along_axis(ray, origin, direction)? - start;
                Some(add(origin, scale(direction, t)))
            }
            Self::Level { origin } => on_plane(ray, origin, [0.0, 0.0, 1.0]),
        }
    }
}

/// An arrow of the active handle's gizmo, or its free-move square (`direction` `None`), from
/// the handle to its end, in world coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Arrow {
    grip: Grip,
    direction: Option<[f64; 3]>,
    /// The axis's index (X, Y, Z), for its colour.
    axis: usize,
    from: [f64; 3],
    to: [f64; 3],
}

/// Distance on screen from `p` to the segment `[a, b]`.
fn segment_distance(p: egui::Pos2, a: egui::Pos2, b: egui::Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    (a + ab * t).distance(p)
}

/// How long the gizmo's arrows are on screen, in points.
const ARROW_POINTS: f64 = 34.0;

/// A handle being dragged.
pub(super) struct Drag {
    pub(super) object: usize,
    grip: Grip,
    /// The support as dragged, in its model's frame.
    pub(super) support: ManualSupport,
    /// The surface's outward normal at the tip, in world coordinates.
    normal: [f64; 3],
    /// The tip's axis as built, in world coordinates.
    axis: [f64; 3],
    /// The knee when pressed, in world coordinates: the vertical shaft it tops stays there.
    knee: [f64; 3],
    /// The surface's outward normal where the foot stands on the model, in world coordinates.
    foot_normal: [f64; 3],
    /// The last shape the drag could build, which a release keeps: where the pointer asks for
    /// a support that cannot be built at all, the handle waits there.
    good: Option<ManualSupport>,
    /// The support stands inside the cavity: its tip and foot move on the cavity's surface.
    inside: bool,
    /// Held to an axis or the level plane by the gizmo; `None` for the handle's own move.
    lock: Option<Lock>,
    /// A new support, its tip clicked, whose foot follows the pointer until a click adds it
    /// (placing "Tip, then foot", owner 2026-10-29); not in the model's document yet.
    pub(super) adding: bool,
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: [f64; 3], k: f64) -> [f64; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn unit(a: [f64; 3]) -> Option<[f64; 3]> {
    let length = dot(a, a).sqrt();
    (length.is_finite() && length > 1e-12).then(|| scale(a, 1.0 / length))
}

/// The world point where `ray` meets the plane through `point` normal to `normal`, ahead of it.
fn on_plane(ray: &Ray, point: [f64; 3], normal: [f64; 3]) -> Option<[f64; 3]> {
    let facing = dot(ray.direction, normal);
    if facing.abs() < 1e-9 {
        return None;
    }
    let t = dot(sub(point, ray.origin), normal) / facing;
    (t > 0.0).then(|| add(ray.origin, scale(ray.direction, t)))
}

/// The centre of the bounding box of `entity`'s parts of kind `part` in `scene`, if it has
/// any: a round part's axis, whatever its triangles.
fn part_centre(scene: &SupportScene, entity: u64, part: SupportPart) -> Option<[f64; 3]> {
    let mesh = scene.mesh();
    let (mut low, mut high) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for range in scene
        .part_ranges()
        .iter()
        .filter(|r| r.entity == entity && r.part == part)
    {
        for triangle in &mesh.triangles()[range.triangles.clone()] {
            for &v in triangle {
                let p = mesh.vertices()[v as usize];
                for (k, c) in [p.x, p.y, p.z].into_iter().enumerate() {
                    low[k] = low[k].min(c);
                    high[k] = high[k].max(c);
                }
            }
        }
    }
    (low[0] <= high[0]).then(|| std::array::from_fn(|k| f64::midpoint(low[k], high[k])))
}

/// Where the handles of support `entity` of `scene` are, in world coordinates, its tip at `tip`,
/// its tip `length_mm` long, its first bend at `knee` and its foot on the model at `foot`, if it
/// has them, its foot `foot_length_mm` long: the tip, the direction (beyond the tip's join along
/// its axis), the knee (the bend, or the tip's join), the foot (on the model, or the pad's
/// centre on the plate) and a foot on the model's direction (beyond its join along its axis).
fn handle_points(
    scene: &SupportScene,
    entity: u64,
    tip: [f64; 3],
    length_mm: f64,
    knee: Option<[f64; 3]>,
    foot: Option<[f64; 3]>,
    foot_length_mm: f64,
) -> Vec<(Grip, [f64; 3])> {
    let mut handles = vec![(Grip::Tip, tip)];
    if let Some(contact) = part_centre(scene, entity, SupportPart::Contact) {
        if let Some(axis) = unit(sub(contact, tip)) {
            handles.push((
                Grip::Direction,
                add(tip, scale(axis, length_mm + DIRECTION_REACH_MM)),
            ));
            handles.push((Grip::Knee, knee.unwrap_or(add(tip, scale(axis, length_mm)))));
        }
    }
    if let Some(foot) = foot {
        handles.push((Grip::Foot, foot));
        let body = part_centre(scene, entity, SupportPart::Foot);
        if let Some(axis) = body.and_then(|body| unit(sub(body, foot))) {
            handles.push((
                Grip::FootDirection,
                add(foot, scale(axis, foot_length_mm + DIRECTION_REACH_MM)),
            ));
        }
    } else if let Some(pad) = part_centre(scene, entity, SupportPart::Pad) {
        handles.push((Grip::Foot, [pad[0], pad[1], 0.0]));
    } else if let Some(fillet) = part_centre(scene, entity, SupportPart::Cone) {
        // On a raft the support has no pad: its foot is its fillet, on the raft.
        handles.push((Grip::Foot, fillet));
    } else if let Some(junction) = part_centre(scene, entity, SupportPart::Junction) {
        // A branch's foot is where it joins its trunk.
        handles.push((Grip::Junction, junction));
    }
    handles
}

/// Where `base` stands on the model, in world coordinates, if it does.
fn model_foot(base: &BaseSpec, to_world: &Affine) -> Option<[f64; 3]> {
    match base {
        BaseSpec::Model { point_mm, .. } => Some(to_world.point(*point_mm)),
        _ => None,
    }
}

/// The model frame's `[x, y]` that `to_world` carries, at Z 0, to the world's `[wx, wy]` (a foot
/// on the plate, see `SupportDocument::placed`), or `None` for a placement standing the frame's
/// plane on edge.
fn plate_point(to_world: &Affine, [wx, wy]: [f64; 2]) -> Option<[f64; 2]> {
    let l = to_world.linear;
    let det = l[0][0] * l[1][1] - l[0][1] * l[1][0];
    if det.abs() < 1e-12 || !det.is_finite() {
        return None;
    }
    let (rx, ry) = (wx - to_world.translation[0], wy - to_world.translation[1]);
    Some([
        (l[1][1] * rx - l[0][1] * ry) / det,
        (l[0][0] * ry - l[1][0] * rx) / det,
    ])
}

/// The direction a direction handle asks for, of a tip or a foot on the model at `end` on a
/// surface with outward `normal`: where `ray` meets the plane across the normal `reach` mm
/// above `end`, at most `limit_deg` from the normal; in world coordinates and, by `to_model`,
/// in the model's frame.
fn turned(
    ray: &Ray,
    end: [f64; 3],
    normal: [f64; 3],
    reach: f64,
    limit_deg: f64,
    to_model: &Affine,
) -> Option<([f64; 3], [f64; 3])> {
    let at = on_plane(ray, add(end, scale(normal, reach)), normal)?;
    let direction = within(unit(sub(at, end))?, normal, limit_deg);
    Some((direction, unit(to_model.vector(direction))?))
}

/// `direction` turned towards `normal` until it is at most `limit_deg` from it.
fn within(direction: [f64; 3], normal: [f64; 3], limit_deg: f64) -> [f64; 3] {
    let cos = dot(direction, normal).clamp(-1.0, 1.0);
    let limit = limit_deg.to_radians();
    if cos.acos() <= limit {
        return direction;
    }
    // The part of `direction` across the normal, at the limit's angle from it.
    let Some(across) = unit(sub(direction, scale(normal, cos))) else {
        return normal;
    };
    add(scale(normal, limit.cos()), scale(across, limit.sin()))
}

/// Move a dragged knee to where `ray` asks (by the handle, up and down the vertical shaft,
/// which stays where it stands; by the gizmo, along an axis or on the level plane, the foot
/// following unless `knee_stays`), at least the knee floor above `floor` (the plate or the raft's
/// top). Returns whether it moved.
fn move_knee(
    drag: &mut Drag,
    ray: &Ray,
    to_world: &Affine,
    to_model: &Affine,
    floor: f64,
    knee_stays: bool,
) -> bool {
    // The owner's rule (2026-10-29); the tip leans from its contact to the knee.
    let tip = to_world.point(drag.support.tip_mm);
    let [x, y, z] = if let Some(lock) = drag.lock {
        let Some(point) = lock.point(ray) else {
            return false;
        };
        point
    } else {
        let [x, y, _] = drag.knee;
        let Some(z) = along_axis(ray, [x, y, 0.0], [0.0, 0.0, 1.0]) else {
            return false;
        };
        [x, y, z]
    };
    let z = knee_height(
        z,
        floor + KNEE_FLOOR_MM,
        tip,
        [x, y],
        drag.support.parameters.tip_length_mm,
    );
    drag.support.bends = vec![to_model.point([x, y, z])];
    drag.support.tip_axis = AxisSpec::Straight;
    // Moved aside, a foot on the plate goes along, keeping the shaft vertical.
    if drag.lock.is_some() && !knee_stays {
        if let BaseSpec::Platform(_) = drag.support.base {
            if let Some(xy) = plate_point(to_world, [x, y]) {
                drag.support.base = BaseSpec::Platform(xy);
            }
        }
    }
    true
}

/// Slide a dragged branch's junction up and down its trunk's shaft in `scene`, within it, to
/// where `ray` passes closest. Returns whether it moved.
fn slide_junction(drag: &mut Drag, scene: &SupportScene, ray: &Ray) -> bool {
    let BaseSpec::Support { parent, z_mm } = drag.support.base else {
        return false;
    };
    let (Some(at), Some([low, high])) = (scene.shaft_point(parent, z_mm), scene.shaft_span(parent))
    else {
        return false;
    };
    let Some(t) = along_axis(ray, at, [0.0, 0.0, 1.0]) else {
        return false;
    };
    let room = drag.support.parameters.shaft_diameter_mm;
    let z = (at[2] + t).clamp(low + room, (high - room).max(low + room));
    drag.support.base = BaseSpec::Support { parent, z_mm: z };
    true
}

/// Follow the pointer's `ray` with a dragged foot: on the model where the ray meets a surface
/// facing up below the tip's join, nearer than the plate, otherwise on the plate (`section`
/// keeps only what it shows); its knee follows unless `knee_stays`. Returns whether the foot
/// moved.
fn drag_foot(
    drag: &mut Drag,
    workspace: &mut Workspace,
    ray: &Ray,
    section: Section,
    cavity: Option<SurfaceHit>,
    knee_stays: bool,
) -> bool {
    if drag.support.kind == SupportKind::Mini {
        return drag_mini_foot(drag, workspace, ray, section, cavity);
    }
    // Held to an axis, the foot stays on the plate, where the axis takes it.
    let locked = match drag.lock {
        Some(lock) if !drag.inside => lock.point(ray),
        _ => None,
    };
    let Some(to_world) = workspace.model_to_world(drag.object) else {
        return false;
    };
    let Some(to_model) = to_world.inverse() else {
        return false;
    };
    let height = workspace
        .supports(drag.object)
        .and_then(|d| d.raft)
        .map_or(0.0, |r| r.thickness_mm);
    let tip = to_world.point(drag.support.tip_mm);
    let parameters = &drag.support.parameters;
    // On the model where the ray meets a surface facing up below the tip's join,
    // nearer than the plate; otherwise on the plate (owner, 2026-10-29).
    // Inside the cavity, only its floor: a foot there never reaches the plate.
    let met = if drag.inside {
        cavity
    } else if drag.lock.is_some() {
        None
    } else {
        workspace.hit_in(ray, section)
    };
    let on_model = met.filter(|h| {
        h.object == drag.object
            && h.normal[2] > 0.0
            && h.point[2] < tip[2] - parameters.tip_length_mm
    });
    let plate = match locked {
        Some([x, y, _]) => Some([x, y, height]),
        None => on_plane(ray, [0.0, 0.0, height], [0.0, 0.0, 1.0]).filter(|_| !drag.inside),
    };
    let nearer = |p: [f64; 3]| dot(sub(p, ray.origin), ray.direction);
    let on_model = on_model.filter(|h| plate.is_none_or(|plate| nearer(h.point) <= nearer(plate)));
    let (at, floor) = if let Some(hit) = on_model {
        let join = hit.point[2] + parameters.foot_length_mm * hit.normal[2].max(0.0);
        drag.support.base = BaseSpec::Model {
            point_mm: to_model.point(hit.point),
            axis: AxisSpec::Normal,
        };
        drag.foot_normal = hit.normal;
        // The knee stays above the foot's join.
        (hit.point, join + KNEE_ABOVE_FOOT_MM)
    } else {
        let Some(at) = plate else {
            return false;
        };
        let Some(xy) = plate_point(&to_world, [at[0], at[1]]) else {
            return false;
        };
        drag.support.base = BaseSpec::Platform(xy);
        (at, height + KNEE_FLOOR_MM)
    };
    // The knee follows the foot, so the shaft stays vertical (owner, 2026-10-29),
    // unless the panel's switch says otherwise. A support without a knee gets one
    // where its shaft began, the tip's join; the tip then points at it. The knee
    // goes down as the foot goes away, the slanted part at most 45° from vertical.
    if !knee_stays {
        let z = drag
            .support
            .bends
            .first()
            .map_or(drag.knee[2], |&b| to_world.point(b)[2]);
        let z = knee_height(
            z,
            floor,
            tip,
            [at[0], at[1]],
            drag.support.parameters.tip_length_mm,
        );
        drag.support.bends = vec![to_model.point([at[0], at[1], z])];
        drag.support.tip_axis = AxisSpec::Straight;
    }

    true
}

/// Follow the pointer's `ray` with a dragged mini support's foot: on the model only, where the
/// ray meets a surface facing up below the tip (`section` keeps only what it shows; inside the
/// cavity, its floor); the rod goes straight from the tip to it. Returns whether the foot
/// moved.
fn drag_mini_foot(
    drag: &mut Drag,
    workspace: &mut Workspace,
    ray: &Ray,
    section: Section,
    cavity: Option<SurfaceHit>,
) -> bool {
    let Some(to_world) = workspace.model_to_world(drag.object) else {
        return false;
    };
    let Some(to_model) = to_world.inverse() else {
        return false;
    };
    let tip = to_world.point(drag.support.tip_mm);
    let met = if drag.inside {
        cavity
    } else {
        workspace.hit_in(ray, section)
    };
    let Some(hit) =
        met.filter(|h| h.object == drag.object && h.normal[2] > 0.0 && h.point[2] < tip[2])
    else {
        return false;
    };
    drag.support.base = BaseSpec::Model {
        point_mm: to_model.point(hit.point),
        axis: AxisSpec::Normal,
    };
    drag.support.bends.clear();
    drag.foot_normal = hit.normal;
    true
}

/// The handle among `handles` nearest `pointer` on the viewport `rect`, within reach.
fn grip_under(
    handles: &[(Grip, [f64; 3])],
    camera: &Camera,
    rect: egui::Rect,
    pointer: egui::Pos2,
) -> Option<Grip> {
    let size = [f64::from(rect.width()), f64::from(rect.height())];
    handles
        .iter()
        .filter_map(|&(grip, at)| {
            let [x, y] = camera.screen(at, size)?;
            #[allow(clippy::cast_possible_truncation)] // Screen positions.
            let distance = (rect.min + egui::vec2(x as f32, y as f32)).distance(pointer);
            (distance <= HANDLE_REACH).then_some((distance, grip))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, grip)| grip)
}

/// The surface's outward normal at a support's tip on object `object`, its `handles` given:
/// where a ray from its direction handle along its axis meets the model, or straight up from
/// below it; straight down when neither meets it.
fn tip_normal(workspace: &mut Workspace, object: usize, handles: &[(Grip, [f64; 3])]) -> [f64; 3] {
    let Some(&(_, tip)) = handles.first() else {
        return [0.0, 0.0, -1.0];
    };
    let from = handles
        .iter()
        .find(|h| h.0 == Grip::Direction)
        .map_or(sub(tip, [0.0, 0.0, 1.0]), |h| h.1);
    unit(sub(tip, from))
        .and_then(|direction| {
            workspace
                .hit_in(
                    &Ray {
                        origin: from,
                        direction,
                    },
                    Section::default(),
                )
                .filter(|h| h.object == object)
                .map(|h| h.normal)
        })
        .unwrap_or([0.0, 0.0, -1.0])
}

/// The surface's outward normal where `base` stands on object `object`'s model, from just above
/// it straight down; straight up when it stands elsewhere or nothing is met.
fn foot_normal(workspace: &mut Workspace, object: usize, base: &BaseSpec) -> [f64; 3] {
    workspace
        .model_to_world(object)
        .and_then(|m| model_foot(base, &m))
        .and_then(|foot| {
            workspace
                .hit_in(
                    &Ray {
                        origin: add(foot, [0.0, 0.0, FOOT_PROBE_MM]),
                        direction: [0.0, 0.0, -1.0],
                    },
                    Section::default(),
                )
                .filter(|h| h.object == object)
                .map(|h| h.normal)
        })
        .unwrap_or([0.0, 0.0, 1.0])
}

impl SupportsUi {
    /// Whether a handle is being dragged, the pointer's button held: a support's, or a brace's
    /// end.
    #[must_use]
    pub fn dragging(&self) -> bool {
        self.drag.as_ref().is_some_and(|d| !d.adding) || self.brace_dragging()
    }

    /// Whether a handle follows the pointer: dragged, or a new support's foot being placed.
    #[must_use]
    pub fn following(&self) -> bool {
        self.drag.is_some()
    }

    /// Whether support `id` of object `index` has its tip on the cavity, by its latest build.
    pub(super) fn interior(&self, index: usize, id: u64) -> bool {
        self.builds
            .get(&index)
            .and_then(|b| b.scene.as_ref().ok())
            .and_then(|s| s.supports().iter().find(|b| b.id == id))
            .is_some_and(|b| b.interior_tip)
    }

    /// Whether the support being dragged stands inside the cavity, so the pointer is followed
    /// on the cavity's surface.
    #[must_use]
    pub fn dragging_inside(&self) -> bool {
        self.drag.as_ref().is_some_and(|d| d.inside)
    }

    /// Whether a support's tip is being dragged.
    pub(super) fn dragging_tip(&self) -> bool {
        self.drag.as_ref().is_some_and(|d| d.grip == Grip::Tip)
    }

    /// The support being dragged, by its object and identity, so its built parts are hidden.
    pub(super) fn dragged_support(&self) -> Option<(usize, u64)> {
        self.drag.as_ref().map(|d| (d.object, d.support.id))
    }

    /// The selected support's handles on object `index`, in world coordinates: from its drag
    /// while dragged, otherwise from its latest build. A mini support has its tip and its foot
    /// (or junction) only: no direction, knee or foot direction to turn.
    fn handles(&self, workspace: &Workspace, index: usize) -> Vec<(Grip, [f64; 3])> {
        let mut handles = self.every_handle(workspace, index);
        let kind = match &self.drag {
            Some(drag) => Some(drag.support.kind),
            None => self
                .selected
                .filter(|&(o, _)| o == index)
                .and_then(|(o, id)| {
                    let document = workspace.supports(o)?;
                    document
                        .supports
                        .iter()
                        .find(|s| s.id == id)
                        .map(|s| s.kind)
                }),
        };
        if kind == Some(SupportKind::Mini) {
            handles.retain(|h| matches!(h.0, Grip::Tip | Grip::Foot | Grip::Junction));
        }
        handles
    }

    /// Every handle [`Self::handles`] may show.
    fn every_handle(&self, workspace: &Workspace, index: usize) -> Vec<(Grip, [f64; 3])> {
        if let (Some(drag), Some(preview)) = (&self.drag, &self.preview) {
            let tip = workspace
                .model_to_world(drag.object)
                .map(|m| m.point(drag.support.tip_mm));
            let to_world = workspace.model_to_world(drag.object);
            let knee = drag
                .support
                .bends
                .first()
                .zip(to_world)
                .map(|(&b, m)| m.point(b));
            let foot = to_world.and_then(|m| model_foot(&drag.support.base, &m));
            return tip.map_or_else(Vec::new, |tip| {
                handle_points(
                    &preview.scene,
                    1,
                    tip,
                    drag.support.parameters.tip_length_mm,
                    knee,
                    foot,
                    drag.support.parameters.foot_length_mm,
                )
            });
        }
        let Some((object, id)) = self.selected.filter(|&(o, _)| o == index) else {
            return Vec::new();
        };
        let Some(support) = workspace
            .supports(object)
            .and_then(|d| d.supports.iter().find(|s| s.id == id))
        else {
            return Vec::new();
        };
        let Some(to_world) = workspace.model_to_world(object) else {
            return Vec::new();
        };
        let tip = to_world.point(support.tip_mm);
        let knee = support.bends.first().map(|&b| to_world.point(b));
        let foot = model_foot(&support.base, &to_world);
        match self.builds.get(&object).and_then(|b| b.scene.as_ref().ok()) {
            Some(scene) => {
                let p = &support.parameters;
                handle_points(
                    scene,
                    id,
                    tip,
                    p.tip_length_mm,
                    knee,
                    foot,
                    p.foot_length_mm,
                )
            }
            None => vec![(Grip::Tip, tip)],
        }
    }

    /// The arrow, or the level square, of the active handle's gizmo under `pointer` on the
    /// viewport `rect`, if any.
    fn arrow_under(
        &self,
        workspace: &Workspace,
        index: usize,
        camera: &Camera,
        rect: egui::Rect,
        pointer: egui::Pos2,
    ) -> Option<Arrow> {
        let size = [f64::from(rect.width()), f64::from(rect.height())];
        let screen = |p: [f64; 3]| {
            camera.screen(p, size).map(|[x, y]| {
                #[allow(clippy::cast_possible_truncation)] // Screen positions.
                let at = rect.min + egui::vec2(x as f32, y as f32);
                at
            })
        };
        self.arrows(workspace, index, camera, size)
            .into_iter()
            .filter_map(|arrow| {
                let (from, to) = (screen(arrow.from)?, screen(arrow.to)?);
                let distance = match arrow.direction {
                    Some(_) => segment_distance(pointer, from, to),
                    None => to.distance(pointer),
                };
                (distance <= HANDLE_REACH).then_some((distance, arrow))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, arrow)| arrow)
    }

    /// The active handle's gizmo on object `index`, `size` the viewport's in points: X, Y and Z
    /// arrows and the level square for the knee, X and Y arrows for a foot on the plate; about
    /// [`ARROW_POINTS`] long on screen.
    fn arrows(
        &self,
        workspace: &Workspace,
        index: usize,
        camera: &Camera,
        size: [f64; 2],
    ) -> Vec<Arrow> {
        let Some(grip) = self
            .active
            .filter(|_| self.selected.is_some_and(|(o, _)| o == index))
        else {
            return Vec::new();
        };
        let on_plate = self
            .drag
            .as_ref()
            .map(|d| &d.support.base)
            .or_else(|| {
                let (object, id) = self.selected?;
                workspace
                    .supports(object)?
                    .supports
                    .iter()
                    .find(|s| s.id == id)
                    .map(|s| &s.base)
            })
            .is_some_and(|b| !matches!(b, BaseSpec::Model { .. }));
        let axes: &[usize] = match grip {
            Grip::Knee => &[0, 1, 2],
            Grip::Foot if on_plate => &[0, 1],
            _ => return Vec::new(),
        };
        let Some(&(_, at)) = self.handles(workspace, index).iter().find(|h| h.0 == grip) else {
            return Vec::new();
        };
        // The world length that looks `ARROW_POINTS` long here.
        let Some(here) = camera.screen(at, size) else {
            return Vec::new();
        };
        let per_mm = (0..3)
            .filter_map(|k| {
                let mut unit_step = [0.0; 3];
                unit_step[k] = 1.0;
                let [x, y] = camera.screen(add(at, unit_step), size)?;
                Some((x - here[0]).hypot(y - here[1]))
            })
            .fold(0.0, f64::max);
        if per_mm <= 0.0 {
            return Vec::new();
        }
        let length = ARROW_POINTS / per_mm;
        let mut arrows: Vec<Arrow> = axes
            .iter()
            .map(|&k| {
                let mut direction = [0.0; 3];
                direction[k] = 1.0;
                Arrow {
                    grip,
                    direction: Some(direction),
                    axis: k,
                    from: at,
                    to: add(at, scale(direction, length)),
                }
            })
            .collect();
        if grip == Grip::Knee {
            arrows.push(Arrow {
                grip,
                direction: None,
                axis: 0,
                from: at,
                to: add(at, [length * 0.4, length * 0.4, 0.0]),
            });
        }
        arrows
    }

    /// A press at `pointer` on the viewport `rect`, along `ray`: a handle of the selected
    /// support of model `index` starts a drag. Returns whether the press was taken.
    #[allow(clippy::too_many_arguments)] // The press's whole context.
    pub fn press(
        &mut self,
        workspace: &mut Workspace,
        camera: &Camera,
        rect: egui::Rect,
        pointer: egui::Pos2,
        index: usize,
    ) -> bool {
        if self.placing {
            return false;
        }
        let size = [f64::from(rect.width()), f64::from(rect.height())];
        let handles = self.handles(workspace, index);
        // The active handle's gizmo first: an arrow holds the drag to its axis, the square to
        // the level plane (owner, 2026-10-29).
        let ray = camera.ray(
            [
                f64::from(pointer.x - rect.min.x),
                f64::from(pointer.y - rect.min.y),
            ],
            size,
        );
        let arrow = self.arrow_under(workspace, index, camera, rect, pointer);
        let grip = arrow
            .map(|a| a.grip)
            .or_else(|| grip_under(&handles, camera, rect, pointer));
        let (Some(grip), Some((object, id))) = (grip, self.selected) else {
            return false;
        };
        // The knee and a foot on the plate show their gizmo once pressed.
        self.active = matches!(grip, Grip::Knee | Grip::Foot).then_some(grip);
        let lock = arrow.and_then(|a| match a.direction {
            Some(direction) => Some(Lock::Axis {
                direction,
                origin: a.from,
                start: along_axis(&ray, a.from, direction)?,
            }),
            None => Some(Lock::Level { origin: a.from }),
        });
        let Some(support) = workspace
            .supports(object)
            .and_then(|d| d.supports.iter().find(|s| s.id == id))
            .cloned()
        else {
            return false;
        };
        let Some(&(_, tip)) = handles.first() else {
            return false;
        };
        let normal = tip_normal(workspace, object, &handles);
        let axis = handles
            .iter()
            .find(|h| h.0 == Grip::Direction)
            .and_then(|h| unit(sub(h.1, tip)))
            .unwrap_or(scale(normal, 1.0));
        let knee = handles
            .iter()
            .find(|h| h.0 == Grip::Knee)
            .map_or(tip, |h| h.1);
        // A support whose tip is on the cavity stays inside it.
        let inside = self.interior(object, id);
        let foot_normal = foot_normal(workspace, object, &support.base);
        self.drag = Some(Drag {
            object,
            grip,
            support,
            normal,
            axis,
            knee,
            foot_normal,
            good: None,
            inside,
            lock,
            adding: false,
        });
        self.rebuild_drag(workspace);
        true
    }

    /// Placing "Tip, then foot" (owner, 2026-10-29): the tip clicked at `hit` on model `index`,
    /// the new support's foot follows the pointer, as a dragged foot does (on the plate, or on
    /// the model below, the shaft vertical under its knee), until a click adds it.
    pub(super) fn start_foot(&mut self, workspace: &Workspace, index: usize, hit: &SurfaceHit) {
        let Some(to_model) = workspace.model_to_world(index).and_then(|m| m.inverse()) else {
            return;
        };
        let support = ManualSupport {
            id: Self::next_id(workspace, index),
            tip_mm: to_model.point(hit.point),
            tip_axis: self.next_axis(),
            bends: Vec::new(),
            base: BaseSpec::Vertical,
            parameters: self.next.parameters(),
            accept_collision: false,
            origin: karasu_core::supports::SupportOrigin::Placed,
            kind: self.next_kind(),
        };
        self.drag = Some(Drag {
            object: index,
            grip: Grip::Foot,
            support,
            normal: hit.normal,
            axis: hit.normal,
            knee: hit.point,
            foot_normal: [0.0, 0.0, 1.0],
            good: None,
            inside: self.on_cavity,
            lock: None,
            adding: true,
        });
        self.preview = None;
        self.rebuild_drag(workspace);
    }

    /// The click that places a new support's foot: the support is added as it shows, even
    /// where it shows an error (as a one-click support is), as one undoable step, and selected.
    /// Returns whether a foot was being placed.
    pub(super) fn add_footed(&mut self, workspace: &mut Workspace) -> bool {
        // A mini support waits for its foot on the model: on the plate it has none.
        if self.drag.as_ref().is_some_and(|d| {
            d.adding
                && d.support.kind == SupportKind::Mini
                && !matches!(d.support.base, BaseSpec::Model { .. })
        }) {
            return true;
        }
        let Some(drag) = self.drag.take_if(|d| d.adding) else {
            return false;
        };
        self.preview = None;
        let id = Self::next_id(workspace, drag.object);
        let mut document = workspace
            .supports(drag.object)
            .cloned()
            .unwrap_or_else(|| SupportDocument::new(Preset::Medium));
        document.supports.push(ManualSupport { id, ..drag.support });
        workspace.set_supports(drag.object, Some(document), None);
        self.selected = Some((drag.object, id));
        self.reveal = true;
        true
    }

    /// A click at `pointer` on the viewport `rect` on a handle of the selected support of model
    /// `index`, or on its gizmo: the knee and a foot show their gizmo, the other handles hide
    /// it. Returns whether the click was taken.
    pub fn touch(
        &mut self,
        workspace: &Workspace,
        camera: &Camera,
        rect: egui::Rect,
        pointer: egui::Pos2,
        index: usize,
    ) -> bool {
        if self.placing || self.selected.is_none_or(|(o, _)| o != index) {
            return false;
        }
        let grip = self
            .arrow_under(workspace, index, camera, rect, pointer)
            .map(|a| a.grip)
            .or_else(|| grip_under(&self.handles(workspace, index), camera, rect, pointer));
        let Some(grip) = grip else {
            return false;
        };
        self.active = matches!(grip, Grip::Knee | Grip::Foot).then_some(grip);
        true
    }

    /// Follow the pointer's `ray` with the dragged handle: the tip where the ray meets the
    /// model's surface facing down (`section` keeps only what it shows), the foot where it
    /// meets the plate, the direction where it meets the plane above the tip across its normal,
    /// at most the document's limit from it. A support inside the cavity moves on the cavity's
    /// surface, where the ray meets it from inside (`cavity`). A tip on a height Shift holds
    /// goes where `held` says (TD-137).
    pub fn drag_to(
        &mut self,
        workspace: &mut Workspace,
        ray: &Ray,
        (section, held): (Section, TipAt),
        cavity: Option<SurfaceHit>,
    ) {
        let Some(drag) = &mut self.drag else {
            return;
        };
        let Some(to_world) = workspace.model_to_world(drag.object) else {
            return;
        };
        let Some(to_model) = to_world.inverse() else {
            return;
        };
        let limit = workspace
            .supports(drag.object)
            .map_or(60.0, |d| d.max_axis_deviation_deg);
        match drag.grip {
            Grip::Tip => {
                let met = match held {
                    TipAt::Held(hit) => hit,
                    TipAt::Free if drag.inside => cavity,
                    TipAt::Free => workspace.hit_in(ray, section),
                };
                let hit = met.filter(|h| h.object == drag.object && h.normal[2] <= FACING_UP);
                let Some(hit) = hit else {
                    return;
                };
                self.pointer_mm = Some(hit.point[2]);
                // A knee goes along with the tip.
                let moved = sub(hit.point, to_world.point(drag.support.tip_mm));
                for bend in &mut drag.support.bends {
                    *bend = to_model.point(add(to_world.point(*bend), moved));
                }
                drag.support.tip_mm = to_model.point(hit.point);
                drag.normal = hit.normal;
            }
            Grip::Foot => {
                // A new support's shaft stands vertical under its knee.
                let knee_stays = self.knee_stays && !drag.adding;
                if !drag_foot(drag, workspace, ray, section, cavity, knee_stays) {
                    return;
                }
            }
            Grip::Direction => {
                let tip = to_world.point(drag.support.tip_mm);
                let reach = drag.support.parameters.tip_length_mm + DIRECTION_REACH_MM;
                let Some((direction, model)) =
                    turned(ray, tip, drag.normal, reach, limit, &to_model)
                else {
                    return;
                };
                drag.support.tip_axis = AxisSpec::Direction(model);
                drag.axis = direction;
            }
            Grip::FootDirection => {
                // As the tip's direction, across the surface's normal where the foot stands.
                let Some(foot) = model_foot(&drag.support.base, &to_world) else {
                    return;
                };
                let reach = drag.support.parameters.foot_length_mm + DIRECTION_REACH_MM;
                let Some((_, model)) = turned(ray, foot, drag.foot_normal, reach, limit, &to_model)
                else {
                    return;
                };
                if let BaseSpec::Model { axis, .. } = &mut drag.support.base {
                    *axis = AxisSpec::Direction(model);
                }
            }
            Grip::Junction => {
                let scene = self
                    .builds
                    .get(&drag.object)
                    .and_then(|b| b.scene.as_ref().ok());
                if !scene.is_some_and(|scene| slide_junction(drag, scene, ray)) {
                    return;
                }
            }
            Grip::Knee => {
                let floor = workspace
                    .supports(drag.object)
                    .and_then(|d| d.raft)
                    .map_or(0.0, |r| r.thickness_mm);
                if !move_knee(drag, ray, &to_world, &to_model, floor, self.knee_stays) {
                    return;
                }
            }
        }
        self.rebuild_drag(workspace);
    }

    /// Build the dragged support alone, as it is now, for the preview.
    fn rebuild_drag(&mut self, workspace: &Workspace) {
        let Some(drag) = &self.drag else {
            return;
        };
        let placed = workspace
            .placement_of(drag.object)
            .and_then(|p| p.transform.to_affine().ok())
            .and_then(|affine| {
                let mut document = SupportDocument::new(Preset::Medium);
                document.supports.push(ManualSupport {
                    id: 1,
                    ..drag.support.clone()
                });
                document.placed(&affine).ok()
            });
        // A branch is shown joining its support where it does, as built.
        let junction = match drag.support.base {
            BaseSpec::Support { parent, z_mm } => self
                .builds
                .get(&drag.object)
                .and_then(|b| b.scene.as_ref().ok())
                .and_then(|scene| scene.shaft_point(parent, z_mm)),
            _ => None,
        };
        let scene = placed
            .and_then(|mut d| d.supports.pop())
            .and_then(|support| {
                // As the build measures them on the whole surface (TD-124).
                let surface = self.current_route(workspace, drag.object);
                let p = &support.parameters;
                let at = |point, facet, radius| {
                    surface.map_or(facet, |s| s.normal_at(point, facet, radius))
                };
                let mut anchors = vec![(
                    support.tip_mm,
                    at(support.tip_mm, drag.normal, p.footprint_radius_mm()),
                )];
                if let BaseSpec::Model { point_mm, .. } = support.base {
                    let radius = p.foot_contact_diameter_mm / 2.0;
                    anchors.push((point_mm, at(point_mm, drag.foot_normal, radius)));
                }
                alone(workspace, support, &anchors, junction)
            });
        let Some(scene) = scene else {
            // Not buildable there: back to the last shape that was.
            if let Some(drag) = &mut self.drag {
                if let Some(good) = &drag.good {
                    drag.support = good.clone();
                }
            }
            return;
        };
        if let Some(drag) = &mut self.drag {
            drag.good = Some(drag.support.clone());
        }
        self.stamp += 1;
        self.preview = Some(Preview {
            stamp: self.stamp,
            ..Preview::of(scene, [0.0; 3], self.next)
        });
    }

    /// The drag ended: the support takes its new shape as one undoable step.
    pub fn release(&mut self, workspace: &mut Workspace) {
        self.release_brace(workspace);
        // A new support's foot waits for its click, whatever the pointer's buttons did.
        let Some(drag) = self.drag.take_if(|d| !d.adding) else {
            return;
        };
        self.preview = None;
        // A branch moved along its trunk stays no thicker than the trunk where it now joins it.
        let support = self.no_thicker(drag.object, drag.support);
        Self::replace_once(workspace, drag.object, support);
    }

    /// Escape during a drag: the support stays as it was. Returns whether there was a drag.
    pub(super) fn cancel_drag(&mut self) -> bool {
        self.preview = None;
        self.drag.take().is_some()
    }

    /// Paint the selected support's handles over the viewport `rect` of model `index`; the one
    /// under `pointer` or dragged highlighted.
    #[allow(clippy::too_many_arguments)] // The overlay's whole context.
    pub fn paint_handles(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        camera: &Camera,
        workspace: &Workspace,
        index: Option<usize>,
        hover: Option<egui::Pos2>,
        appearance: &Appearance,
    ) {
        let size = [f64::from(rect.width()), f64::from(rect.height())];
        // A branch's tip clicked shows where it is while its shaft is looked for.
        if let Some(tip) = self.branch_tip.filter(|t| Some(t.object) == index) {
            if let Some([x, y]) = camera.screen(tip.point, size) {
                #[allow(clippy::cast_possible_truncation)] // Screen positions.
                let at = rect.min + egui::vec2(x as f32, y as f32);
                painter.circle_filled(at, 6.0, appearance.hole_valid.color());
            }
        }
        let Some(index) = index.filter(|_| !self.placing) else {
            return;
        };
        let screen = |p: [f64; 3]| {
            camera.screen(p, size).map(|[x, y]| {
                #[allow(clippy::cast_possible_truncation)] // Screen positions.
                let at = rect.min + egui::vec2(x as f32, y as f32);
                at
            })
        };
        self.paint_gizmo(
            painter, &screen, camera, workspace, index, size, hover, appearance,
        );
        let handles = self.handles(workspace, index);
        let at = |grip: Grip| handles.iter().find(|h| h.0 == grip).map(|h| h.1);
        for (end, direction) in [
            (Grip::Tip, Grip::Direction),
            (Grip::Foot, Grip::FootDirection),
        ] {
            if let Some((a, b)) = at(end).and_then(screen).zip(at(direction).and_then(screen)) {
                painter.line_segment([a, b], egui::Stroke::new(1.5, appearance.uniform.color()));
            }
        }
        // A foot on the model is a second tip: a dot, as the tip's.
        let foot_on_model = at(Grip::FootDirection).is_some();
        for (grip, at) in handles {
            let Some(at) = screen(at) else {
                continue;
            };
            let active = self.drag.as_ref().is_some_and(|d| d.grip == grip)
                || hover.is_some_and(|p| p.distance(at) <= HANDLE_REACH);
            let color = if active {
                appearance.hovered.color()
            } else {
                appearance.uniform.color()
            };
            match grip {
                Grip::Tip => {
                    painter.circle_filled(at, 6.0, color);
                }
                Grip::Foot if foot_on_model => {
                    painter.circle_filled(at, 6.0, color);
                }
                Grip::Foot | Grip::Junction => {
                    painter.rect_filled(
                        egui::Rect::from_center_size(at, egui::vec2(11.0, 11.0)),
                        2.0,
                        color,
                    );
                }
                Grip::Direction | Grip::FootDirection => {
                    painter.circle_stroke(at, 7.0, egui::Stroke::new(2.5, color));
                }
                Grip::Knee => {
                    let r = 7.0;
                    painter.add(egui::Shape::convex_polygon(
                        vec![
                            at + egui::vec2(0.0, -r),
                            at + egui::vec2(r, 0.0),
                            at + egui::vec2(0.0, r),
                            at + egui::vec2(-r, 0.0),
                        ],
                        color,
                        egui::Stroke::NONE,
                    ));
                }
            }
        }
    }

    /// Paint the active handle's gizmo, its points placed on screen by `screen`: arrows in the
    /// axes' colours, the level square; the one under `hover` or held highlighted.
    #[allow(clippy::too_many_arguments)] // The overlay's whole context.
    fn paint_gizmo(
        &self,
        painter: &egui::Painter,
        screen: &dyn Fn([f64; 3]) -> Option<egui::Pos2>,
        camera: &Camera,
        workspace: &Workspace,
        index: usize,
        size: [f64; 2],
        hover: Option<egui::Pos2>,
        appearance: &Appearance,
    ) {
        for arrow in self.arrows(workspace, index, camera, size) {
            let (Some(from), Some(to)) = (screen(arrow.from), screen(arrow.to)) else {
                continue;
            };
            let dragged = self
                .drag
                .as_ref()
                .is_some_and(|d| match (d.lock, arrow.direction) {
                    (Some(Lock::Axis { direction, .. }), Some(mine)) => {
                        dot(direction, mine) > 0.999
                    }
                    (Some(Lock::Level { .. }), None) => true,
                    _ => false,
                });
            let near = hover.is_some_and(|p| match arrow.direction {
                Some(_) => segment_distance(p, from, to) <= HANDLE_REACH,
                None => to.distance(p) <= HANDLE_REACH,
            });
            let colour = if dragged || near {
                appearance.hovered.color()
            } else if arrow.direction.is_some() {
                appearance.axes[arrow.axis].color()
            } else {
                appearance.free_move.color()
            };
            if arrow.direction.is_some() {
                painter.line_segment([from, to], egui::Stroke::new(2.0, colour));
                let along = (to - from).normalized();
                let across = egui::vec2(-along.y, along.x);
                painter.add(egui::Shape::convex_polygon(
                    vec![to + along * 7.0, to + across * 4.5, to - across * 4.5],
                    colour,
                    egui::Stroke::NONE,
                ));
            } else {
                painter.rect_filled(
                    egui::Rect::from_center_size(to, egui::vec2(10.0, 10.0)),
                    2.0,
                    colour,
                );
            }
        }
    }
}

#[cfg(test)]
impl SupportsUi {
    /// Start dragging the selected support's tip, as a press on its handle would, without a
    /// screen. Returns whether a support is selected.
    pub(super) fn press_tip_for_test(&mut self, workspace: &mut Workspace) -> bool {
        self.press_for_test(workspace, Grip::Tip)
    }

    /// The selected support's handles on object `index`, by what they are.
    pub(super) fn grips_for_test(&self, workspace: &Workspace, index: usize) -> Vec<Grip> {
        self.handles(workspace, index).iter().map(|h| h.0).collect()
    }

    /// Press the selected support's junction with its trunk, as a press on its handle does.
    pub(super) fn press_junction_for_test(&mut self, workspace: &mut Workspace) -> bool {
        self.press_for_test(workspace, Grip::Junction)
    }

    fn press_for_test(&mut self, workspace: &mut Workspace, grip: Grip) -> bool {
        let Some((object, id)) = self.selected else {
            return false;
        };
        let Some(support) = workspace
            .supports(object)
            .and_then(|d| d.supports.iter().find(|s| s.id == id))
            .cloned()
        else {
            return false;
        };
        let inside = self.interior(object, id);
        self.drag = Some(Drag {
            object,
            grip,
            support,
            normal: [0.0, 0.0, -1.0],
            axis: [0.0, 0.0, -1.0],
            knee: [0.0; 3],
            foot_normal: [0.0, 0.0, 1.0],
            good: None,
            inside,
            lock: None,
            adding: false,
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::supports_ui::tests::{built, lifted_cube, ray};

    #[test]
    #[allow(clippy::too_many_lines)] // One support through every handle, step by step.
    fn handles_move_a_support_as_one_step_and_escape_puts_it_back() {
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
        built(&mut ui, &workspace, &preparation);
        // The selected support shows its tip, its direction and its foot.
        let handles = ui.handles(&workspace, 0);
        let grips: Vec<Grip> = handles.iter().map(|h| h.0).collect();
        assert_eq!(grips, [Grip::Tip, Grip::Direction, Grip::Knee, Grip::Foot]);
        let foot = handles[3].1;
        assert!(
            (foot[0] - cx).abs() < 1e-6 && (foot[1] - cy).abs() < 1e-6 && foot[2] == 0.0,
            "{foot:?} {cx} {cy}"
        );
        let document = |w: &Workspace| w.supports(0).unwrap().clone();
        let before = document(&workspace);
        let support = before.supports[0].clone();
        // The foot dragged 3 mm along X on the plate: shown as it would be, then one step.
        ui.drag = Some(Drag {
            object: 0,
            grip: Grip::Foot,
            support: support.clone(),
            normal: [0.0, 0.0, -1.0],
            axis: [0.0, 0.0, -1.0],
            knee: [0.0; 3],
            foot_normal: [0.0, 0.0, 1.0],
            good: None,
            inside: false,
            lock: None,
            adding: false,
        });
        let down = Ray {
            origin: [cx + 3.0, cy, 50.0],
            direction: [0.0, 0.0, -1.0],
        };
        ui.drag_to(
            &mut workspace,
            &down,
            (Section::default(), TipAt::Free),
            None,
        );
        assert!(ui.preview.is_some());
        let revision = workspace.revision();
        ui.release(&mut workspace);
        assert_ne!(workspace.revision(), revision);
        let moved = document(&workspace).supports[0].clone();
        let BaseSpec::Platform(xy) = moved.base else {
            panic!("{:?}", moved.base);
        };
        let world = workspace
            .model_to_world(0)
            .unwrap()
            .point([xy[0], xy[1], 0.0]);
        assert!((world[0] - cx - 3.0).abs() < 1e-9 && (world[1] - cy).abs() < 1e-9);
        // It had no knee: it got one above the foot, so its shaft stays vertical, and the tip
        // points at it.
        let knee = workspace.model_to_world(0).unwrap().point(moved.bends[0]);
        assert!((knee[0] - world[0]).abs() < 1e-9 && (knee[1] - world[1]).abs() < 1e-9);
        assert_eq!(moved.tip_axis, AxisSpec::Straight);
        // Built again: the foot's handle is where it was dropped.
        built(&mut ui, &workspace, &preparation);
        let foot = ui.handles(&workspace, 0)[3].1;
        assert!((foot[0] - cx - 3.0).abs() < 1e-6, "{foot:?}");
        // The tip slides on the bottom face; the direction turns, at most the limit.
        ui.drag = Some(Drag {
            object: 0,
            grip: Grip::Tip,
            support: moved.clone(),
            normal: [0.0, 0.0, -1.0],
            axis: [0.0, 0.0, -1.0],
            knee: [0.0; 3],
            foot_normal: [0.0, 0.0, 1.0],
            good: None,
            inside: false,
            lock: None,
            adding: false,
        });
        ui.drag_to(
            &mut workspace,
            &ray(cx - 2.0, cy, true),
            (Section::default(), TipAt::Free),
            None,
        );
        let tip = workspace
            .model_to_world(0)
            .unwrap()
            .point(ui.drag.as_ref().unwrap().support.tip_mm);
        assert!((tip[0] - cx + 2.0).abs() < 1e-9, "{tip:?}");
        ui.drag.as_mut().unwrap().grip = Grip::Direction;
        // Up from beside the cube to the plane 4.5 mm under the tip, 12 mm from it: 69° from
        // the surface's normal, beyond the 60° limit.
        let sideways = Ray {
            origin: [cx + 40.0, cy, -10.0],
            direction: unit([-30.0, 0.0, 10.5]).unwrap(),
        };
        ui.drag_to(
            &mut workspace,
            &sideways,
            (Section::default(), TipAt::Free),
            None,
        );
        let AxisSpec::Direction(axis) = ui.drag.as_ref().unwrap().support.tip_axis else {
            panic!("the tip was not turned");
        };
        let from_normal = dot(unit(axis).unwrap(), [0.0, 0.0, -1.0])
            .acos()
            .to_degrees();
        assert!(from_normal <= 60.0 + 1e-9, "{from_normal}");
        // Escape during a drag puts it back: the document is as the foot's drag left it.
        ui.drag = Some(Drag {
            object: 0,
            grip: Grip::Tip,
            support: moved.clone(),
            normal: [0.0, 0.0, -1.0],
            axis: [0.0, 0.0, -1.0],
            knee: [0.0; 3],
            foot_normal: [0.0, 0.0, 1.0],
            good: None,
            inside: false,
            lock: None,
            adding: false,
        });
        ui.drag_to(
            &mut workspace,
            &ray(cx + 2.0, cy, true),
            (Section::default(), TipAt::Free),
            None,
        );
        assert!(ui.escape());
        assert!(ui.drag.is_none());
        assert_eq!(document(&workspace).supports[0], moved);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    #[allow(clippy::too_many_lines)] // One support's knee, moved every way in turn.
    fn the_knee_moves_up_and_down_the_shaft_which_stays_where_it_stands() {
        let (mut workspace, mut preparation, directory) = lifted_cube();
        // 10 mm above the plate: room for a knee between the tip and the pad.
        workspace.edit(|plate| plate.objects[0].pose.lift_mm = Some(10.0));
        crate::hollowing_ui::tests::prepared_within(
            &workspace,
            &mut preparation,
            std::time::Duration::from_secs(60),
        );
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
        built(&mut ui, &workspace, &preparation);
        let document = |w: &Workspace| w.supports(0).unwrap().clone();
        let support = document(&workspace).supports[0].clone();
        // The shaft stands 3 mm beside the tip; the knee tops it.
        let shaft = [cx + 3.0, cy, 6.0];
        let knee_drag = |support: &ManualSupport| Drag {
            object: 0,
            grip: Grip::Knee,
            support: support.clone(),
            normal: [0.0, 0.0, -1.0],
            axis: [0.0, 0.0, -1.0],
            knee: shaft,
            foot_normal: [0.0, 0.0, 1.0],
            good: None,
            inside: false,
            lock: None,
            adding: false,
        };
        ui.drag = Some(knee_drag(&support));
        let level = |z: f64| Ray {
            origin: [cx + 40.0, cy, z],
            direction: [-1.0, 0.0, 0.0],
        };
        let knee = |ui: &SupportsUi, w: &Workspace| {
            w.model_to_world(0)
                .unwrap()
                .point(ui.drag.as_ref().unwrap().support.bends[0])
        };
        // Down to the floor, 2 mm above the plate, the shaft where it stands; the tip leans to
        // the knee.
        ui.drag_to(
            &mut workspace,
            &level(-20.0),
            (Section::default(), TipAt::Free),
            None,
        );
        let at = knee(&ui, &workspace);
        assert!((at[0] - shaft[0]).abs() < 1e-9 && (at[1] - shaft[1]).abs() < 1e-9);
        assert!((at[2] - 2.0).abs() < 1e-9, "the floor: {at:?}");
        assert_eq!(
            ui.drag.as_ref().unwrap().support.tip_axis,
            AxisSpec::Straight
        );
        // Up to 45° from vertical from the tip, 3 mm across: 3 mm under it.
        ui.drag_to(
            &mut workspace,
            &level(30.0),
            (Section::default(), TipAt::Free),
            None,
        );
        let at = knee(&ui, &workspace);
        let tip = workspace.model_to_world(0).unwrap().point(support.tip_mm);
        assert!((at[2] - (tip[2] - 3.0)).abs() < 1e-9, "45 degrees: {at:?}");
        // Right under the tip, the knee stays past the tip's join.
        assert!((knee_height(30.0, 2.0, tip, [tip[0], tip[1]], 2.5) - (tip[2] - 3.0)).abs() < 1e-9);
        // Released: the support's bend, one step.
        ui.drag_to(
            &mut workspace,
            &level(5.0),
            (Section::default(), TipAt::Free),
            None,
        );
        ui.release(&mut workspace);
        let moved = document(&workspace).supports[0].clone();
        assert_eq!(moved.bends.len(), 1);
        // A click on the knee, without a drag, shows its gizmo; one on the tip hides it.
        built(&mut ui, &workspace, &preparation);
        let to_world = workspace.model_to_world(0).unwrap();
        let camera = Camera::new(to_world.point(moved.bends[0]), 150.0);
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let on_screen = |grip: Grip, ui: &SupportsUi| {
            let (_, at) = ui
                .handles(&workspace, 0)
                .into_iter()
                .find(|h| h.0 == grip)
                .unwrap();
            let [x, y] = camera.screen(at, [800.0, 600.0]).unwrap();
            #[allow(clippy::cast_possible_truncation)] // Screen positions.
            let at = egui::pos2(x as f32, y as f32);
            at
        };
        assert!(ui.touch(&workspace, &camera, rect, on_screen(Grip::Knee, &ui), 0));
        assert_eq!(ui.active, Some(Grip::Knee));
        assert_eq!(ui.arrows(&workspace, 0, &camera, [800.0, 600.0]).len(), 4);
        assert!(ui.touch(&workspace, &camera, rect, on_screen(Grip::Tip, &ui), 0));
        assert_eq!(ui.active, None);
        assert!(!ui.touch(&workspace, &camera, rect, egui::pos2(5.0, 5.0), 0));
        // The foot moved: by default the knee follows it, so the shaft stays vertical; with
        // the switch off, the knee stays and the shaft leans.
        let to_world = workspace.model_to_world(0).unwrap();
        let foot_drag = |support: &ManualSupport| Drag {
            grip: Grip::Foot,
            ..knee_drag(support)
        };
        let down = Ray {
            origin: [cx + 3.5, cy + 0.5, 50.0],
            direction: [0.0, 0.0, -1.0],
        };
        for stays in [false, true] {
            ui.knee_stays = stays;
            ui.drag = Some(foot_drag(&moved));
            ui.drag_to(
                &mut workspace,
                &down,
                (Section::default(), TipAt::Free),
                None,
            );
            let bend = to_world.point(ui.drag.as_ref().unwrap().support.bends[0]);
            let before = to_world.point(moved.bends[0]);
            if stays {
                assert!(sub(bend, before).iter().all(|v| v.abs() < 1e-9), "{bend:?}");
            } else {
                assert!((bend[0] - cx - 3.5).abs() < 1e-9 && (bend[1] - cy - 0.5).abs() < 1e-9);
                // 3.5 mm from the tip, 45 degrees allow the knee where it was.
                assert!(
                    (bend[2] - before[2]).abs() < 1e-9,
                    "the knee keeps its height"
                );
            }
            assert!(ui.cancel_drag());
        }
        ui.knee_stays = false;
        // Held by the gizmo's X arrow, the knee moves along X only and its foot follows, the
        // shaft vertical; held by a foot's Y arrow, the foot moves along Y only.
        let lock = |origin: [f64; 3], direction: [f64; 3], ray: &Ray| Lock::Axis {
            direction,
            origin,
            start: along_axis(ray, origin, direction).unwrap(),
        };
        let down_at = |x: f64, y: f64| Ray {
            origin: [x, y, 50.0],
            direction: [0.0, 0.0, -1.0],
        };
        // A foot on the plate, where it stands; one below the knee, `knee`.
        let on_plate = |base: &BaseSpec, knee: [f64; 3]| match base {
            BaseSpec::Platform([x, y]) => to_world.point([*x, *y, 0.0]),
            BaseSpec::Vertical => [knee[0], knee[1], 0.0],
            _ => panic!("a foot on the plate"),
        };
        let before = to_world.point(moved.bends[0]);
        ui.drag = Some(Drag {
            lock: Some(lock(
                before,
                [1.0, 0.0, 0.0],
                &down_at(before[0], before[1]),
            )),
            ..knee_drag(&moved)
        });
        ui.drag_to(
            &mut workspace,
            &down_at(before[0] + 1.0, before[1] + 5.0),
            (Section::default(), TipAt::Free),
            None,
        );
        let support = &ui.drag.as_ref().unwrap().support;
        let bend = to_world.point(support.bends[0]);
        let wanted = add(before, [1.0, 0.0, 0.0]);
        assert!(sub(bend, wanted).iter().all(|v| v.abs() < 1e-9), "{bend:?}");
        let foot = on_plate(&support.base, bend);
        assert!((foot[0] - bend[0]).abs() < 1e-9 && (foot[1] - bend[1]).abs() < 1e-9);
        assert!(ui.cancel_drag());
        let foot = on_plate(&moved.base, before);
        ui.drag = Some(Drag {
            lock: Some(lock(foot, [0.0, 1.0, 0.0], &down_at(foot[0], foot[1]))),
            ..foot_drag(&moved)
        });
        ui.drag_to(
            &mut workspace,
            &down_at(foot[0] + 4.0, foot[1] + 1.0),
            (Section::default(), TipAt::Free),
            None,
        );
        let support = &ui.drag.as_ref().unwrap().support;
        let at = on_plate(&support.base, to_world.point(support.bends[0]));
        assert!(
            (at[0] - foot[0]).abs() < 1e-9 && (at[1] - foot[1] - 1.0).abs() < 1e-9,
            "{at:?}"
        );
        assert!(ui.cancel_drag());
        // Escape during a drag puts it back.
        ui.drag = Some(knee_drag(&moved));
        ui.drag_to(
            &mut workspace,
            &level(-20.0),
            (Section::default(), TipAt::Free),
            None,
        );
        assert!(ui.escape());
        assert!(ui.drag.is_none());
        assert_eq!(document(&workspace).supports[0], moved);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn tip_then_foot_adds_a_support_on_the_second_click_with_its_foot_there() {
        let (mut workspace, mut preparation, directory) = lifted_cube();
        workspace.edit(|plate| plate.objects[0].pose.lift_mm = Some(10.0));
        crate::hollowing_ui::tests::prepared_within(
            &workspace,
            &mut preparation,
            std::time::Duration::from_secs(60),
        );
        let mut ui = SupportsUi {
            placing: true,
            tip_then_foot: true,
            ..SupportsUi::default()
        };
        let placed = workspace.placement_of(0).unwrap();
        let [cx, cy] =
            [0, 1].map(|k| f64::midpoint(placed.min.coordinates()[k], placed.max.coordinates()[k]));
        let count = |w: &Workspace| w.supports(0).map_or(0, |d| d.supports.len());
        // The first click places the tip: nothing is added yet, the foot follows the pointer.
        let under = ray(cx, cy, true);
        let hit = workspace.hit(&under);
        assert!(ui.click(&mut workspace, &under, hit, 0));
        assert_eq!(count(&workspace), 0);
        assert!(ui.following() && !ui.dragging());
        let down = Ray {
            origin: [cx + 4.0, cy + 1.0, 50.0],
            direction: [0.0, 0.0, -1.0],
        };
        ui.drag_to(
            &mut workspace,
            &down,
            (Section::default(), TipAt::Free),
            None,
        );
        // The end of an orbit's drag does not add it.
        ui.release(&mut workspace);
        assert!(ui.following());
        assert_eq!(count(&workspace), 0);
        // The second click adds it, its foot there and its shaft vertical under its knee, and
        // selects it; placing goes on.
        let hit = workspace.hit(&down);
        assert!(ui.click(&mut workspace, &down, hit, 0));
        assert!(!ui.following() && ui.placing);
        let support = workspace.supports(0).unwrap().supports[0].clone();
        assert_eq!(ui.selected, Some((0, support.id)));
        let to_world = workspace.model_to_world(0).unwrap();
        let BaseSpec::Platform([x, y]) = support.base else {
            panic!("a foot on the plate: {:?}", support.base);
        };
        let foot = to_world.point([x, y, 0.0]);
        let knee = to_world.point(support.bends[0]);
        for at in [foot, knee] {
            assert!((at[0] - cx - 4.0).abs() < 1e-9 && (at[1] - cy - 1.0).abs() < 1e-9);
        }
        assert_eq!(support.tip_axis, AxisSpec::Straight);
        // Escape after a tip takes the tip back, still placing; nothing more is added.
        let hit = workspace.hit(&under);
        assert!(ui.click(&mut workspace, &under, hit, 0));
        assert!(ui.following());
        assert!(ui.escape());
        assert!(!ui.following() && ui.placing);
        assert_eq!(count(&workspace), 1);
        // Placing whole supports again drops a foot being placed.
        let hit = workspace.hit(&under);
        assert!(ui.click(&mut workspace, &under, hit, 0));
        ui.set_tip_then_foot(false);
        assert!(!ui.following());
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    #[allow(clippy::too_many_lines)] // One foot, moved and turned in turn.
    fn a_foot_moves_onto_the_model_below_and_back_onto_the_plate() {
        // The slab 6 mm above the block: room for the tip, a knee and the foot between.
        let (mut workspace, _preparation, directory) =
            crate::supports_ui::route::tests::ledge_at(10.0);
        let [x0, y0, z0] = workspace.placement_of(0).unwrap().min.coordinates();
        let mut ui = SupportsUi {
            placing: true,
            straight_only: true,
            ..SupportsUi::default()
        };
        // A support under the slab, over the block: straight down, through the block.
        let tip = [x0 + 4.0, y0 + 5.0, z0 + 7.0];
        let hit = crate::workspace::SurfaceHit {
            object: 0,
            point: tip,
            normal: [0.0, 0.0, -1.0],
        };
        let up = Ray {
            origin: [tip[0], tip[1], -50.0],
            direction: [0.0, 0.0, 1.0],
        };
        assert!(ui.click(&mut workspace, &up, Some(hit), 0));
        let support = workspace.supports(0).unwrap().supports[0].clone();
        ui.drag = Some(Drag {
            object: 0,
            grip: Grip::Foot,
            support,
            normal: [0.0, 0.0, -1.0],
            axis: [0.0, 0.0, -1.0],
            knee: add(tip, [0.0, 0.0, -2.5]),
            foot_normal: [0.0, 0.0, 1.0],
            good: None,
            inside: false,
            lock: None,
            adding: false,
        });
        // Onto the block's top, from in front, under the slab: the foot stands on the model,
        // and the knee follows it.
        let target = [x0 + 3.0, y0 + 5.0, z0 + 1.0];
        let origin = [x0 + 3.0, y0 - 25.0, z0 + 2.0];
        let onto = Ray {
            origin,
            direction: unit(sub(target, origin)).unwrap(),
        };
        ui.drag_to(
            &mut workspace,
            &onto,
            (Section::default(), TipAt::Free),
            None,
        );
        let to_world = workspace.model_to_world(0).unwrap();
        let dragged = ui.drag.as_ref().unwrap().support.clone();
        let BaseSpec::Model { point_mm, .. } = dragged.base else {
            panic!("{:?}", dragged.base);
        };
        let foot = to_world.point(point_mm);
        assert!(sub(foot, target).iter().all(|v| v.abs() < 1e-9), "{foot:?}");
        let knee = to_world.point(dragged.bends[0]);
        assert!((knee[0] - foot[0]).abs() < 1e-9 && (knee[1] - foot[1]).abs() < 1e-9);
        assert!(
            knee[2] >= foot[2] + dragged.parameters.foot_length_mm,
            "{knee:?}"
        );
        // Its handle is on the model, where the foot stands.
        assert!(ui
            .handles(&workspace, 0)
            .iter()
            .any(|&(grip, at)| grip == Grip::Foot && sub(at, foot).iter().all(|v| v.abs() < 1e-9)));
        // Its direction handle stands above it along its axis, the surface's normal, beyond
        // its join; dragged, it turns the foot, at most the axis limit from the normal.
        let reach = dragged.parameters.foot_length_mm + DIRECTION_REACH_MM;
        assert!(ui.handles(&workspace, 0).iter().any(|&(grip, at)| {
            grip == Grip::FootDirection
                && sub(at, add(foot, [0.0, 0.0, reach]))
                    .iter()
                    .all(|v| v.abs() < 1e-6)
        }));
        ui.drag.as_mut().unwrap().grip = Grip::FootDirection;
        let toward = |x: f64| {
            let target = add(foot, [x, 0.0, reach]);
            let origin = add(target, [0.0, -20.0, 20.0]);
            Ray {
                origin,
                direction: unit(sub(target, origin)).unwrap(),
            }
        };
        let foot_axis = |ui: &SupportsUi| match ui.drag.as_ref().unwrap().support.base {
            BaseSpec::Model {
                axis: AxisSpec::Direction(d),
                ..
            } => unit(to_world.vector(d)).unwrap(),
            ref other => panic!("{other:?}"),
        };
        ui.drag_to(
            &mut workspace,
            &toward(1.0),
            (Section::default(), TipAt::Free),
            None,
        );
        let axis = foot_axis(&ui);
        let wanted = unit([1.0, 0.0, reach]).unwrap();
        assert!(sub(axis, wanted).iter().all(|v| v.abs() < 1e-9), "{axis:?}");
        ui.drag_to(
            &mut workspace,
            &toward(100.0),
            (Section::default(), TipAt::Free),
            None,
        );
        let limit = workspace.supports(0).unwrap().max_axis_deviation_deg;
        let degrees = foot_axis(&ui)[2].acos().to_degrees();
        assert!((degrees - limit).abs() < 1e-6, "{degrees}");
        ui.drag.as_mut().unwrap().grip = Grip::Foot;
        ui.drag_to(
            &mut workspace,
            &onto,
            (Section::default(), TipAt::Free),
            None,
        );
        // Released, then back onto the plate beyond the block.
        ui.release(&mut workspace);
        let on_model = workspace.supports(0).unwrap().supports[0].clone();
        assert!(matches!(on_model.base, BaseSpec::Model { .. }));
        ui.drag = Some(Drag {
            object: 0,
            grip: Grip::Foot,
            support: on_model,
            normal: [0.0, 0.0, -1.0],
            axis: [0.0, 0.0, -1.0],
            knee,
            foot_normal: [0.0, 0.0, 1.0],
            good: None,
            inside: false,
            lock: None,
            adding: false,
        });
        let down = Ray {
            origin: [x0 + 9.0, y0 + 5.0, z0 + 4.0],
            direction: [0.0, 0.0, -1.0],
        };
        ui.drag_to(
            &mut workspace,
            &down,
            (Section::default(), TipAt::Free),
            None,
        );
        assert!(matches!(
            ui.drag.as_ref().unwrap().support.base,
            BaseSpec::Platform(_)
        ));
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn a_foot_on_the_plate_comes_back_to_the_model_frame() {
        // Turned a quarter about Z, scaled twice in X and moved: the world point maps back to
        // the model frame's point the placement carries there.
        let to_world = Affine {
            linear: [[0.0, -1.0, 0.0], [2.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            translation: [10.0, 20.0, 5.0],
        };
        let model = plate_point(&to_world, [7.0, 26.0]).unwrap();
        let back = to_world.point([model[0], model[1], 0.0]);
        assert!((back[0] - 7.0).abs() < 1e-12 && (back[1] - 26.0).abs() < 1e-12);
        // A frame stood on edge has no point on the plate's plane to give.
        let edge = Affine {
            linear: [[1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]],
            translation: [0.0; 3],
        };
        assert!(plate_point(&edge, [1.0, 1.0]).is_none());
    }

    #[test]
    fn a_turned_tip_stays_within_the_limit_from_the_surface() {
        let normal = [0.0, 0.0, -1.0];
        // Within the limit: as it is.
        let near = unit([0.3, 0.0, -1.0]).unwrap();
        assert_eq!(within(near, normal, 60.0), near);
        // Beyond it: turned back to the limit, on the same side.
        let far = unit([1.0, 0.0, -0.1]).unwrap();
        let kept = within(far, normal, 60.0);
        let angle = dot(kept, normal).acos().to_degrees();
        assert!((angle - 60.0).abs() < 1e-9, "{angle}");
        assert!(kept[0] > 0.0 && kept[1].abs() < 1e-12);
    }
}
