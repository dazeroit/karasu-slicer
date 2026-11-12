//! The move, rotate and scale gizmos of the selected object (M6.2, TD-073 and TD-074).
//!
//! **Handles:** drawn over the viewport at the object's anchor (its source bounding-box centre,
//! placed), a fixed size on screen, following the object while it is dragged.
//! - Move: arrows along X, Y and Z, ending in arrowheads that point along their axis, and a
//!   filled square at the origin for free moves on the plate's plane. Only the handles move an
//!   object, so dragging across it never moves it by accident.
//! - Rotate: rings about X, Y and Z.
//! - **Axes (M6.5):** moving and turning use the world's axes (global), or the object's own axes,
//!   which turn with it (local). The free-move square stays on the plate's plane either way.
//! - Scale: lines along the object's own X, Y and Z axes (which turn with it), and a uniform
//!   handle.
//!
//! The axes use the conventional red, green and blue ([`crate::appearance`]) and are labelled X,
//! Y and Z, so they are told apart without relying on colour alone.
//!
//! **Dragging** edits the plate's object from the values it had when the drag began, so the
//! result depends only on the pointer, never on how many frames the drag took:
//! - moving sets an explicit translation; moving in Z also ends resting on the plate (the
//!   workspace keeps the object from going below it);
//! - turning composes a world-axis turn after the object's rotation, or a local one before it;
//!   Shift snaps to 15 degrees;
//! - scaling multiplies the scale of one axis, or of all three, by the ratio of the pointer's
//!   distances from the anchor.
use crate::{
    appearance::Appearance,
    camera::{Camera, Ray},
    gizmo::{
        along_axis, angle_about, euler_matrix, on_plane, segment_distance, turned, turned_local,
    },
};
use eframe::egui;
use karasu_project::project::PlateObject;

/// The axes moving and turning use (M6.5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Space {
    /// The world's: the plate's edges and its vertical.
    #[default]
    Global,
    /// The object's own, turning with it.
    Local,
}

impl Space {
    /// The axes of this space for an object rotated by `rotate_deg`.
    #[must_use]
    pub fn axes(self, rotate_deg: [f64; 3]) -> [[f64; 3]; 3] {
        match self {
            Self::Global => [unit(0), unit(1), unit(2)],
            Self::Local => object_axes(rotate_deg),
        }
    }
}

/// What dragging a handle does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Mode {
    /// Move.
    Move,
    /// Turn about a world axis.
    Rotate,
    /// Scale.
    Scale,
}

/// A handle's action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Move along axis 0 (X), 1 (Y) or 2 (Z) of the drag's space.
    Axis(usize),
    /// Move freely on the horizontal plane.
    Plane,
    /// Turn about axis 0, 1 or 2 of the drag's space.
    Ring(usize),
    /// Scale along the object's axis 0, 1 or 2.
    Scale(usize),
    /// Scale all axes alike.
    Uniform,
    /// A size handle on a hollowing blocker's side (TD-115): which one is the blocker's to say.
    /// Its polyline is a single point, drawn as a dot; it moves no object.
    Size(u8),
}

/// A handle: its action and its shape, a world polyline.
#[derive(Clone, Debug, PartialEq)]
pub struct Handle {
    /// The action.
    pub kind: Kind,
    /// Points of the polyline, in mm.
    pub points: Vec<[f64; 3]>,
}

/// Arrowhead length and base width, in points.
const ARROW_LENGTH: f32 = 16.0;
const ARROW_WIDTH: f32 = 12.0;

/// The unit vector of world axis `axis`.
fn unit(axis: usize) -> [f64; 3] {
    let mut v = [0.0; 3];
    v[axis] = 1.0;
    v
}

/// The object's own axes: the columns of its rotation.
#[must_use]
pub fn object_axes(rotate_deg: [f64; 3]) -> [[f64; 3]; 3] {
    let m = euler_matrix(rotate_deg);
    std::array::from_fn(|axis| std::array::from_fn(|row| m[row][axis]))
}

/// The handles of `mode` at `anchor`, `size` mm long, for an object rotated by `rotate_deg`, its
/// arrows and rings along the axes of `space`.
#[must_use]
pub fn handles(
    mode: Mode,
    anchor: [f64; 3],
    size: f64,
    rotate_deg: [f64; 3],
    space: Space,
) -> Vec<Handle> {
    let axes = space.axes(rotate_deg);
    let at = |offset: [f64; 3]| std::array::from_fn(|k| anchor[k] + offset[k]);
    let line = |kind, direction: [f64; 3]| Handle {
        kind,
        points: vec![anchor, at(direction.map(|c| c * size))],
    };
    match mode {
        Mode::Move => {
            let mut list: Vec<Handle> = (0..3)
                .map(|axis| line(Kind::Axis(axis), axes[axis]))
                .collect();
            let s = size * 0.18;
            list.push(Handle {
                kind: Kind::Plane,
                points: vec![
                    at([s, s, 0.0]),
                    at([-s, s, 0.0]),
                    at([-s, -s, 0.0]),
                    at([s, -s, 0.0]),
                    at([s, s, 0.0]),
                ],
            });
            list
        }
        Mode::Rotate => rings(anchor, size, axes),
        Mode::Scale => {
            let axes = object_axes(rotate_deg);
            let mut list: Vec<Handle> = (0..3)
                .map(|axis| line(Kind::Scale(axis), axes[axis]))
                .collect();
            let d = size * 0.6 / 3f64.sqrt();
            list.push(Handle {
                kind: Kind::Uniform,
                points: vec![anchor, at([d, d, d])],
            });
            list
        }
    }
}

/// Rings about the unit `axes` at `anchor`, `size` mm in radius: [`Kind::Ring`] `k` turns about
/// `axes[k]`.
#[must_use]
pub fn rings(anchor: [f64; 3], size: f64, axes: [[f64; 3]; 3]) -> Vec<Handle> {
    (0..3)
        .map(|axis| {
            let (u, v) = (axes[(axis + 1) % 3], axes[(axis + 2) % 3]);
            let points = (0..=48)
                .map(|i| {
                    let (s, c) = (f64::from(i) / 48.0 * std::f64::consts::TAU).sin_cos();
                    std::array::from_fn(|k| anchor[k] + size * (c * u[k] + s * v[k]))
                })
                .collect();
            Handle {
                kind: Kind::Ring(axis),
                points,
            }
        })
        .collect()
}

/// Whether `point` lies inside the closed polygon `polygon` (even-odd rule).
fn inside(point: [f64; 2], polygon: &[[f64; 2]]) -> bool {
    let mut odd = false;
    for (i, a) in polygon.iter().enumerate() {
        let b = polygon[(i + 1) % polygon.len()];
        if (a[1] > point[1]) != (b[1] > point[1])
            && point[0] < a[0] + (b[0] - a[0]) * (point[1] - a[1]) / (b[1] - a[1])
        {
            odd = !odd;
        }
    }
    odd
}

/// The handle within `reach` pixels of `pointer` (closest first), in a viewport `size` pixels
/// large. The free-move square is hit anywhere inside it.
#[must_use]
pub fn hit(
    handles: &[Handle],
    camera: &Camera,
    size: [f64; 2],
    pointer: [f64; 2],
    reach: f64,
) -> Option<usize> {
    handles
        .iter()
        .enumerate()
        .filter_map(|(index, handle)| {
            let screen: Vec<[f64; 2]> = handle
                .points
                .iter()
                .filter_map(|&p| camera.screen(p, size))
                .collect();
            let distance = if handle.kind == Kind::Plane && inside(pointer, &screen) {
                0.0
            } else {
                screen
                    .windows(2)
                    .map(|s| segment_distance(pointer, s[0], s[1]))
                    .fold(f64::INFINITY, f64::min)
            };
            (distance <= reach).then_some((distance, index))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, index)| index)
}

fn color(kind: Kind, appearance: &Appearance) -> egui::Color32 {
    match kind {
        Kind::Axis(axis) | Kind::Ring(axis) | Kind::Scale(axis) => {
            appearance.axes[axis.min(2)].color()
        }
        Kind::Plane => appearance.free_move.color(),
        Kind::Uniform | Kind::Size(_) => appearance.uniform.color(),
    }
}

fn label(kind: Kind) -> &'static str {
    match kind {
        Kind::Axis(0) | Kind::Ring(0) | Kind::Scale(0) => "X",
        Kind::Axis(1) | Kind::Ring(1) | Kind::Scale(1) => "Y",
        Kind::Axis(_) | Kind::Ring(_) | Kind::Scale(_) => "Z",
        Kind::Plane | Kind::Size(_) => "",
        Kind::Uniform => "XYZ",
    }
}

/// Draw the handles over the viewport `rect`; `hovered` is highlighted.
pub fn draw(
    painter: &egui::Painter,
    rect: egui::Rect,
    camera: &Camera,
    handles: &[Handle],
    hovered: Option<usize>,
    appearance: &Appearance,
    dim: bool,
) {
    let size = [f64::from(rect.width()), f64::from(rect.height())];
    #[allow(clippy::cast_possible_truncation)] // Screen points.
    let to_pos = |[x, y]: [f64; 2]| rect.min + egui::vec2(x as f32, y as f32);
    for (index, handle) in handles.iter().enumerate() {
        let hover = hovered == Some(index);
        let color = match (handle.kind, hover) {
            (Kind::Plane, true) => appearance.free_move_hovered.color(),
            (_, true) => appearance.hovered.color(),
            (kind, false) => color(kind, appearance),
        };
        // A locked transform's handles are faded: they show the axes, and do nothing.
        let color = if dim {
            color.gamma_multiply(0.35)
        } else {
            color
        };
        let points: Vec<egui::Pos2> = handle
            .points
            .iter()
            .filter_map(|&p| camera.screen(p, size))
            .map(to_pos)
            .collect();
        if let (Kind::Size(_), Some(&at)) = (handle.kind, points.first()) {
            // A size handle: a dot ringed in the plate's dark, seen on any surface.
            painter.circle(at, 6.0, color, egui::Stroke::new(1.5, egui::Color32::BLACK));
            continue;
        }
        if handle.kind == Kind::Plane {
            // The free-move square, filled.
            let corners = points[..points.len().saturating_sub(1)].to_vec();
            painter.add(egui::Shape::convex_polygon(
                corners,
                color,
                egui::Stroke::NONE,
            ));
            continue;
        }
        let tip = match handle.kind {
            Kind::Ring(_) => points.get(points.len() / 8).copied(),
            Kind::Plane | Kind::Size(_) => None,
            Kind::Axis(_) | Kind::Scale(_) | Kind::Uniform => points.last().copied(),
        };
        match (handle.kind, points.first().copied(), tip) {
            (Kind::Axis(_), Some(start), Some(tip)) => {
                // An arrowhead pointing along the axis as seen on screen.
                let along = (tip - start).normalized();
                let back = tip - along * ARROW_LENGTH;
                let wing = egui::vec2(-along.y, along.x) * (ARROW_WIDTH / 2.0);
                painter.line_segment([start, back], egui::Stroke::new(3.0, color));
                painter.add(egui::Shape::convex_polygon(
                    vec![tip, back + wing, back - wing],
                    color,
                    egui::Stroke::NONE,
                ));
            }
            _ => {
                painter.add(egui::Shape::line(
                    points.clone(),
                    egui::Stroke::new(3.0, color),
                ));
            }
        }
        let Some(tip) = tip else {
            continue;
        };
        if matches!(handle.kind, Kind::Scale(_) | Kind::Uniform) {
            // Scale handles end in squares; rings carry a dot by their label.
            painter.rect_filled(
                egui::Rect::from_center_size(tip, egui::vec2(11.0, 11.0)),
                0.0,
                color,
            );
        } else if matches!(handle.kind, Kind::Ring(_)) {
            painter.circle_filled(tip, 6.0, color);
        }
        painter.text(
            tip + egui::vec2(9.0, -9.0),
            egui::Align2::LEFT_BOTTOM,
            label(handle.kind),
            egui::FontId::proportional(14.0),
            color,
        );
    }
}

/// A drag in progress: the handle, where it began and the object's values then.
#[derive(Clone, Debug, PartialEq)]
pub struct Drag {
    /// The handle's action.
    pub kind: Kind,
    /// The object dragged.
    pub object: usize,
    /// The point the drag works about: the anchor, or where the object was grabbed.
    pub anchor: [f64; 3],
    /// The pointer ray when the drag began.
    pub start_ray: Ray,
    /// The pointer when the drag began, in viewport pixels.
    pub start_pointer: [f64; 2],
    /// The object when the drag began.
    pub start: PlateObject,
    /// Its translation then, as placed (centred and dropped values included).
    pub start_translation: [f64; 3],
    /// The axes the drag moves and turns along.
    pub space: Space,
}

/// The object after dragging to `ray` and `pointer` (viewport pixels), from the drag's start
/// values; `None` when the pointer gives no position (a ray parallel to the handle).
#[must_use]
pub fn apply(
    drag: &Drag,
    camera: &Camera,
    size: [f64; 2],
    ray: &Ray,
    pointer: [f64; 2],
    snap: bool,
) -> Option<PlateObject> {
    let mut object = drag.start.clone();
    match drag.kind {
        Kind::Axis(axis) => {
            let direction = drag.space.axes(drag.start.pose.rotate_deg)[axis];
            let from = along_axis(&drag.start_ray, drag.anchor, direction)?;
            let to = along_axis(ray, drag.anchor, direction)?;
            object.translate_mm =
                std::array::from_fn(|k| drag.start_translation[k] + direction[k] * (to - from));
            object.pose.center = false;
            // Any move with a height in it lifts the object off the plate.
            if direction[2].abs() > 1e-9 {
                object.pose.drop = false;
                object.pose.lift_mm = None;
            }
        }
        Kind::Plane => {
            let up = unit(2);
            let from = on_plane(&drag.start_ray, drag.anchor, up)?;
            let to = on_plane(ray, drag.anchor, up)?;
            object.translate_mm = drag.start_translation;
            object.translate_mm[0] += to[0] - from[0];
            object.translate_mm[1] += to[1] - from[1];
            object.pose.center = false;
        }
        Kind::Ring(axis) => {
            let start = drag.start.pose.rotate_deg;
            let normal = drag.space.axes(start)[axis];
            let from = on_plane(&drag.start_ray, drag.anchor, normal)?;
            let to = on_plane(ray, drag.anchor, normal)?;
            let mut degrees = angle_about(drag.anchor, normal, from, to);
            if snap {
                degrees = (degrees / 15.0).round() * 15.0;
            }
            object.pose.rotate_deg = match drag.space {
                Space::Global => turned(start, axis, degrees),
                Space::Local => turned_local(start, axis, degrees),
            };
        }
        Kind::Scale(axis) => {
            let direction = object_axes(drag.start.pose.rotate_deg)[axis];
            let from = along_axis(&drag.start_ray, drag.anchor, direction)?;
            let to = along_axis(ray, drag.anchor, direction)?;
            if from.abs() < 1e-9 {
                return None;
            }
            object.pose.scale[axis] = (drag.start.pose.scale[axis] * to / from).clamp(0.01, 1000.0);
        }
        // Blockers' handles: never an object's.
        Kind::Size(_) => return None,
        Kind::Uniform => {
            let centre = camera.screen(drag.anchor, size)?;
            let reach = |p: [f64; 2]| (p[0] - centre[0]).hypot(p[1] - centre[1]);
            let factor = reach(pointer) / reach(drag.start_pointer).max(1.0);
            object.pose.scale = drag
                .start
                .pose
                .scale
                .map(|s| (s * factor).clamp(0.01, 1000.0));
        }
    }
    Some(object)
}

#[cfg(test)]
mod tests {
    use super::*;
    use karasu_project::project::ModelId;

    fn object() -> PlateObject {
        PlateObject::new("box", ModelId([1; 32]))
    }

    #[test]
    fn handles_are_hit_near_their_lines() {
        let camera = Camera::new([0.0; 3], 200.0);
        let size = [800.0, 600.0];
        let moves = handles(Mode::Move, [0.0; 3], 20.0, [0.0; 3], Space::Global);
        assert_eq!(
            moves.iter().map(|h| h.kind).collect::<Vec<_>>(),
            [Kind::Axis(0), Kind::Axis(1), Kind::Axis(2), Kind::Plane]
        );
        let tip = camera.screen([20.0, 0.0, 0.0], size).unwrap();
        assert_eq!(hit(&moves, &camera, size, tip, 8.0), Some(0));
        // The free-move square is hit anywhere inside, even far from its edges.
        let centre = camera.screen([0.0, 0.0, 0.0], size).unwrap();
        let big = handles(Mode::Move, [0.0; 3], 200.0, [0.0; 3], Space::Global);
        assert_eq!(hit(&big[3..], &camera, size, centre, 2.0), Some(0));
        assert_eq!(hit(&moves, &camera, size, [0.0, 0.0], 8.0), None);
        assert_eq!(
            handles(Mode::Rotate, [0.0; 3], 20.0, [0.0; 3], Space::Global).len(),
            3
        );
        // Scale handles follow the object's own axes.
        let scales = handles(Mode::Scale, [0.0; 3], 20.0, [0.0, 0.0, 90.0], Space::Global);
        assert_eq!(scales.len(), 4);
        let x_tip = scales[0].points[1];
        assert!(
            x_tip[0].abs() < 1e-9 && (x_tip[1] - 20.0).abs() < 1e-9,
            "{x_tip:?}"
        );
    }

    #[test]
    fn local_axes_move_and_turn_with_the_object() {
        // An object turned 90 degrees about Z: its own X is the world's Y.
        let turned_z = [0.0, 0.0, 90.0];
        let arrows = handles(Mode::Move, [0.0; 3], 20.0, turned_z, Space::Local);
        let x_tip = arrows[0].points[1];
        assert!(
            x_tip[0].abs() < 1e-9 && (x_tip[1] - 20.0).abs() < 1e-9,
            "{x_tip:?}"
        );
        // The free-move square stays on the plate's plane.
        assert!(arrows[3].points.iter().all(|p| p[2].abs() < 1e-12));
        // Dragging along the local X arrow moves the object along the world's Y.
        let camera = Camera::new([0.0; 3], 200.0);
        let size = [800.0, 600.0];
        let pointer = |p: [f64; 3]| camera.screen(p, size).unwrap();
        let mut start = object();
        start.pose.rotate_deg = turned_z;
        let drag = Drag {
            kind: Kind::Axis(0),
            object: 0,
            anchor: [0.0; 3],
            start_ray: camera.ray(pointer([0.0, 5.0, 0.0]), size),
            start_pointer: pointer([0.0, 5.0, 0.0]),
            start,
            start_translation: [0.0; 3],
            space: Space::Local,
        };
        let ray = camera.ray(pointer([0.0, 15.0, 0.0]), size);
        let moved = apply(&drag, &camera, size, &ray, pointer([0.0, 15.0, 0.0]), false).unwrap();
        let t = moved.translate_mm;
        assert!(
            t[0].abs() < 1e-6 && (t[1] - 10.0).abs() < 1e-6 && t[2].abs() < 1e-9,
            "{t:?}"
        );
        // Turning about the local X equals turning about the world's Y here.
        let local = crate::gizmo::turned_local(turned_z, 0, 30.0);
        let world = crate::gizmo::turned(turned_z, 1, 30.0);
        let (a, b) = (euler_matrix(local), euler_matrix(world));
        for r in 0..3 {
            for c in 0..3 {
                assert!((a[r][c] - b[r][c]).abs() < 1e-9, "{local:?} {world:?}");
            }
        }
    }

    #[test]
    fn drags_edit_the_object_from_its_start_values() {
        let camera = Camera::new([0.0; 3], 200.0);
        let size = [800.0, 600.0];
        let pointer = |p: [f64; 3]| camera.screen(p, size).unwrap();
        let start = |kind, from: [f64; 3]| Drag {
            kind,
            object: 0,
            anchor: [0.0; 3],
            start_ray: camera.ray(pointer(from), size),
            start_pointer: pointer(from),
            start: object(),
            start_translation: [1.0, 2.0, 3.0],
            space: Space::Global,
        };
        let drag = |kind, from: [f64; 3], to: [f64; 3], snap| {
            let ray = camera.ray(pointer(to), size);
            apply(&start(kind, from), &camera, size, &ray, pointer(to), snap).unwrap()
        };
        let x = [10.0, 0.0, 0.0];
        // Along X by 15 mm.
        let moved = drag(Kind::Axis(0), x, [25.0, 0.0, 0.0], false);
        assert!(
            (moved.translate_mm[0] - 16.0).abs() < 1e-6,
            "{:?}",
            moved.translate_mm
        );
        assert_eq!(moved.translate_mm[1..], [2.0, 3.0]);
        assert!(!moved.pose.center && moved.pose.drop);
        // Freely on the plane: 15 mm along X and 5 along Y.
        let free = drag(Kind::Plane, x, [25.0, 5.0, 0.0], false);
        assert!((free.translate_mm[0] - 16.0).abs() < 1e-6);
        assert!((free.translate_mm[1] - 7.0).abs() < 1e-6);
        // Up along Z by 4 mm: the object no longer rests on the plate.
        let up = drag(Kind::Axis(2), [0.0, 0.0, 10.0], [0.0, 0.0, 14.0], false);
        assert!(
            (up.translate_mm[2] - 7.0).abs() < 1e-6,
            "{:?}",
            up.translate_mm
        );
        assert!(!up.pose.drop);
        // A quarter turn about Z, and snapping to 15 degrees.
        let turn = drag(Kind::Ring(2), x, [0.0, 10.0, 0.0], false);
        assert!((turn.pose.rotate_deg[2] - 90.0).abs() < 1e-6);
        let near = [10.0 * 0.3f64.cos(), 10.0 * 0.3f64.sin(), 0.0];
        let snapped = drag(Kind::Ring(2), x, near, true);
        assert!((snapped.pose.rotate_deg[2] - 15.0).abs() < 1e-9);
        // Doubling along the X axis scales X only.
        let stretched = drag(Kind::Scale(0), x, [20.0, 0.0, 0.0], false);
        assert!(
            (stretched.pose.scale[0] - 2.0).abs() < 1e-6,
            "{:?}",
            stretched.pose.scale
        );
        assert_eq!(stretched.pose.scale[1..], [1.0, 1.0]);
        // Uniform scaling scales all three.
        let scaled = drag(Kind::Uniform, x, [20.0, 0.0, 0.0], false);
        for s in scaled.pose.scale {
            assert!((s - 2.0).abs() < 0.05, "{s}");
        }
    }
}
