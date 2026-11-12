//! Hollowing blockers (owner, 2026-11-02): regions of a model the hollowing keeps solid.
//!
//! A blocker is a box whose two bases may differ (a truncated pyramid) or a sphere, kept in its
//! model's frame ([`Blocker`]): it follows every move, turn and scale of the model, as drain
//! holes do. The Hollow tool places one centred on the point clicked on the model, with the New
//! blockers' values; the move and turn gizmos then place it freely, and the scale gizmo and its
//! fields size it.
//!
//! **Size handles** (owner mockup, 2026-11-02; added 2026-11-03): dots on the selected blocker,
//! whatever the gizmo. A box has one at the middle of each side of its two bases, which widens
//! or narrows that base alone, evenly about the axis, and one at the middle of each base, which
//! moves that base alone, the other staying where it is. A sphere has one at each end of its
//! three axes, which sets its diameter. A drag follows the pointer along the handle's outward
//! direction.
//!
//! **Draft:** blockers change the cavity, which takes seconds on a large model, so they are
//! edited as a draft of the selected model's list and applied with Apply hollowing, together
//! with the wall. The draft starts again from the applied blockers when another model is shown
//! or the applied ones change (applied, removed or undone).
//!
//! **Display:** each blocker is uploaded once per shape, in its own frame, and placed with its
//! model's placement and its own turn and centre, so moving or turning it never uploads again.
//! It is drawn translucent where in view, and fainter where the model hides it: applied
//! blockers teal, new or changed ones violet, the selected one magenta (colours the user sets).
use super::matrix;
use crate::{
    appearance::Appearance,
    camera::{Mat4, Ray},
    gizmo::{
        along_axis, angle_about, euler_matrix, matrix_euler, on_plane, ray_triangle, turned,
        turned_local, Affine,
    },
    i18n::{decimal, tr},
    list_window::{self, Event, Row, Spec},
    manipulate::{self, Handle, Kind, Mode, Space},
    viewport::{Cap, Draw, Look, Viewport},
    widgets::{with_unit, Unit},
    workspace::{SurfaceHit, Workspace},
};
use eframe::{egui, egui_wgpu};
use karasu_core::{geometry::Point3, mesh::TriangleMesh};
use karasu_project::request::Blocker;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A box's base sides, its height and a sphere's diameter, in mm.
pub const SIZE_MM: std::ops::RangeInclusive<f64> = 0.1..=500.0;
/// A box's upper base sides, in mm: zero makes a pyramid's apex.
pub const TOP_MM: std::ops::RangeInclusive<f64> = 0.0..=500.0;
/// The Solid base's height, in mm.
pub const BASE_MM: std::ops::RangeInclusive<f64> = 0.1..=100.0;
/// How far the Solid base's box reaches past the model's footprint and below its lowest point,
/// in mm: the base is kept solid to its very edge.
const BASE_MARGIN_MM: f64 = 1.0;
/// The upper base drawn for a pyramid's apex, in mm: a mesh has no zero-sized faces.
const APEX_MM: f64 = 0.01;
/// The viewport keys of the blockers' meshes.
const MESH_KEYS: u64 = 0xB10C_0000_0000_0000;
/// How much of its opacity a blocker keeps where the model hides it.
const HIDDEN_SHARE: f32 = 0.75;
/// The sphere's mesh: slices about its axis and stacks from pole to pole.
const SLICES: u32 = 32;
const STACKS: u32 = 16;
/// How far around a blocker the view frames it, in mm.
const AROUND_MM: f64 = 4.0;

/// The values a new blocker takes (owner mockup, 2026-11-02): a default set in Settings, kept
/// while the user works (design rule 6).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NewBlocker {
    /// A sphere, not a box.
    pub sphere: bool,
    /// The box's lower base: width and depth, in mm.
    pub bottom_mm: [f64; 2],
    /// Its upper base: width and depth, in mm.
    pub top_mm: [f64; 2],
    /// Its height, in mm.
    pub height_mm: f64,
    /// The sphere's diameter, in mm.
    pub diameter_mm: f64,
    /// The Solid base's height above the model's lowest point, in mm.
    pub base_mm: f64,
}

impl Default for NewBlocker {
    fn default() -> Self {
        Self {
            sphere: false,
            bottom_mm: [10.0, 10.0],
            top_mm: [6.0, 6.0],
            height_mm: 8.0,
            diameter_mm: 8.0,
            base_mm: 3.0,
        }
    }
}

impl NewBlocker {
    /// These values with each one out of its range replaced by the built-in default.
    #[must_use]
    pub fn valid(self) -> Self {
        let d = Self::default();
        let size = |v: f64, d: f64| if SIZE_MM.contains(&v) { v } else { d };
        let top = |v: f64, d: f64| if TOP_MM.contains(&v) { v } else { d };
        Self {
            sphere: self.sphere,
            bottom_mm: [
                size(self.bottom_mm[0], d.bottom_mm[0]),
                size(self.bottom_mm[1], d.bottom_mm[1]),
            ],
            top_mm: [
                top(self.top_mm[0], d.top_mm[0]),
                top(self.top_mm[1], d.top_mm[1]),
            ],
            height_mm: size(self.height_mm, d.height_mm),
            diameter_mm: size(self.diameter_mm, d.diameter_mm),
            base_mm: if BASE_MM.contains(&self.base_mm) {
                self.base_mm
            } else {
                d.base_mm
            },
        }
    }

    /// A blocker of these values centred at `centre_mm`, in the model frame, unturned.
    #[must_use]
    pub fn at(&self, centre_mm: [f64; 3]) -> Blocker {
        if self.sphere {
            Blocker::Sphere {
                centre_mm,
                diameter_mm: self.diameter_mm,
            }
        } else {
            Blocker::Frustum {
                centre_mm,
                rotation_deg: [0.0; 3],
                bottom_mm: self.bottom_mm,
                top_mm: self.top_mm,
                height_mm: self.height_mm,
            }
        }
    }
}

/// A model's blockers as edited, and as applied when the draft began.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Draft {
    pub(super) object: usize,
    pub(super) base: Vec<Blocker>,
    pub(super) edit: Vec<Blocker>,
}

/// A gizmo drag in progress, from the blocker's values when it began.
#[derive(Clone, Debug)]
struct Drag {
    kind: Kind,
    /// The blocker dragged, in the draft.
    index: usize,
    /// Its centre in the world when the drag began.
    anchor: [f64; 3],
    start_ray: Ray,
    start: Blocker,
    /// The handles' world axes.
    axes: [[f64; 3]; 3],
    /// The model's turn: its model frame's axes in the world, without its scale.
    pose: [[f64; 3]; 3],
    /// World to model frame.
    back: Affine,
    space: Space,
    /// For a size handle: world millimetres per model millimetre along its direction (`axes[0]`).
    stretch: f64,
}

/// The selected blocker: its place in the draft, and its centre and turn in the world.
#[derive(Clone, Copy, Debug)]
struct Selected {
    position: usize,
    blocker: Blocker,
    anchor: [f64; 3],
    world: [[f64; 3]; 3],
}

/// The Hollow tool's blockers.
pub struct BlockersUi {
    /// The pointer places blockers on the selected model.
    pub placing: bool,
    /// What new blockers take.
    pub next: NewBlocker,
    /// The list window is open, kept with the settings.
    pub list_open: bool,
    pub(super) draft: Option<Draft>,
    /// The selected blocker, in the draft.
    pub(super) selected: Option<usize>,
    /// A blocker was selected in the scene: the list scrolls to its row once.
    reveal: bool,
    drag: Option<Drag>,
    /// Viewport meshes uploaded, by shape.
    meshes: HashMap<[u64; 6], u64>,
    keys: u64,
}

impl Default for BlockersUi {
    fn default() -> Self {
        Self {
            placing: false,
            next: NewBlocker::default(),
            list_open: true,
            draft: None,
            selected: None,
            reveal: false,
            drag: None,
            meshes: HashMap::new(),
            keys: 0,
        }
    }
}

/// The corners of a box with bases `bottom` and `top` (width, depth) `height` apart, centred
/// at the origin: the lower base first, each base counterclockwise seen from above.
fn frustum_points(bottom: [f64; 2], top: [f64; 2], height: f64) -> [[f64; 3]; 8] {
    let corner = |[w, d]: [f64; 2], z: f64, k: usize| {
        let (u, v) = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)][k];
        [u * w / 2.0, v * d / 2.0, z]
    };
    std::array::from_fn(|i| {
        if i < 4 {
            corner(bottom, -height / 2.0, i)
        } else {
            corner(top.map(|s| s.max(APEX_MM)), height / 2.0, i - 4)
        }
    })
}

/// The box's faces, turned outwards.
const FRUSTUM_FACES: [[u32; 3]; 12] = [
    [0, 2, 1],
    [0, 3, 2],
    [4, 5, 6],
    [4, 6, 7],
    [0, 1, 5],
    [0, 5, 4],
    [1, 2, 6],
    [1, 6, 5],
    [2, 3, 7],
    [2, 7, 6],
    [3, 0, 4],
    [3, 4, 7],
];

/// A sphere of radius 1 about the origin, its faces turned outwards.
fn unit_sphere() -> Option<TriangleMesh> {
    let mut points = vec![Point3::new(0.0, 0.0, -1.0)];
    for stack in 1..STACKS {
        let polar = std::f64::consts::PI * f64::from(stack) / f64::from(STACKS);
        let (ring, z) = (polar.sin(), -polar.cos());
        for slice in 0..SLICES {
            let angle = std::f64::consts::TAU * f64::from(slice) / f64::from(SLICES);
            points.push(Point3::new(ring * angle.cos(), ring * angle.sin(), z));
        }
    }
    points.push(Point3::new(0.0, 0.0, 1.0));
    let at = |stack: u32, slice: u32| 1 + (stack - 1) * SLICES + slice % SLICES;
    let top = 1 + (STACKS - 1) * SLICES;
    let mut triangles = Vec::new();
    for slice in 0..SLICES {
        triangles.push([0, at(1, slice + 1), at(1, slice)]);
        triangles.push([top, at(STACKS - 1, slice), at(STACKS - 1, slice + 1)]);
        for stack in 1..STACKS - 1 {
            let (a, b) = (at(stack, slice), at(stack, slice + 1));
            let (c, d) = (at(stack + 1, slice + 1), at(stack + 1, slice));
            triangles.push([a, b, c]);
            triangles.push([a, c, d]);
        }
    }
    TriangleMesh::try_new(points, triangles).ok()
}

/// The key of `blocker`'s shape: a box as it is sized; any sphere, as its placement scales one
/// of radius 1.
fn shape(blocker: &Blocker) -> [u64; 6] {
    match *blocker {
        Blocker::Sphere { .. } => [1, 0, 0, 0, 0, 0],
        Blocker::Frustum {
            bottom_mm,
            top_mm,
            height_mm,
            ..
        } => [
            0,
            bottom_mm[0].to_bits(),
            bottom_mm[1].to_bits(),
            top_mm[0].to_bits(),
            top_mm[1].to_bits(),
            height_mm.to_bits(),
        ],
    }
}

/// `blocker`'s mesh in its own frame.
fn mesh_of(blocker: &Blocker) -> Option<TriangleMesh> {
    match *blocker {
        Blocker::Sphere { .. } => unit_sphere(),
        Blocker::Frustum {
            bottom_mm,
            top_mm,
            height_mm,
            ..
        } => {
            let points = frustum_points(bottom_mm, top_mm, height_mm)
                .iter()
                .map(|p| Point3::new(p[0], p[1], p[2]))
                .collect();
            TriangleMesh::try_new(points, FRUSTUM_FACES.to_vec()).ok()
        }
    }
}

/// `blocker`'s turn (rows), the identity for a sphere.
fn turn_of(blocker: &Blocker) -> [[f64; 3]; 3] {
    match *blocker {
        Blocker::Frustum { rotation_deg, .. } => euler_matrix(rotation_deg),
        Blocker::Sphere { .. } => euler_matrix([0.0; 3]),
    }
}

/// The map from `blocker`'s own frame to its model frame: its turn and centre, and a sphere's
/// radius.
fn own_frame(blocker: &Blocker) -> Mat4 {
    let turn = turn_of(blocker);
    let size = match *blocker {
        Blocker::Sphere { diameter_mm, .. } => diameter_mm / 2.0,
        Blocker::Frustum { .. } => 1.0,
    };
    let centre = blocker.centre_mm();
    Mat4(std::array::from_fn(|row| {
        if row == 3 {
            [0.0, 0.0, 0.0, 1.0]
        } else {
            let t = turn[row];
            [t[0] * size, t[1] * size, t[2] * size, centre[row]]
        }
    }))
}

/// The size handles of `blocker` (see the module's documentation), in its model frame: each
/// one's code, point, and outward unit direction. A box's codes: 0 to 3 the lower base's sides
/// (+X, -X, +Y, -Y), 4 to 7 the upper base's, 8 the upper base, 9 the lower base. A sphere's:
/// 0 to 5, +X, -X, +Y, -Y, +Z, -Z.
fn sides(blocker: &Blocker) -> Vec<(u8, [f64; 3], [f64; 3])> {
    let centre = blocker.centre_mm();
    let turn = turn_of(blocker);
    // Own frame to model frame: the turn (rows) applied, then the centre.
    let to_model = |v: [f64; 3]| -> [f64; 3] {
        std::array::from_fn(|r| (0..3).map(|c| turn[r][c] * v[c]).sum())
    };
    let ends = [
        [1.0, 0.0, 0.0],
        [-1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, -1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, -1.0],
    ];
    let mut list = Vec::new();
    let mut push = |code: u8, local: [f64; 3], out: [f64; 3]| {
        let at = to_model(local);
        list.push((
            code,
            std::array::from_fn(|k| centre[k] + at[k]),
            to_model(out),
        ));
    };
    match *blocker {
        Blocker::Sphere { diameter_mm, .. } => {
            for (code, end) in (0u8..).zip(ends) {
                push(code, end.map(|c| c * diameter_mm / 2.0), end);
            }
        }
        Blocker::Frustum {
            bottom_mm,
            top_mm,
            height_mm,
            ..
        } => {
            for (base, sizes, z) in [
                (0u8, bottom_mm, -height_mm / 2.0),
                (4, top_mm, height_mm / 2.0),
            ] {
                for (code, end) in (0u8..).zip(&ends[..4]) {
                    let half = sizes[usize::from(code / 2)] / 2.0;
                    push(base + code, [end[0] * half, end[1] * half, z], *end);
                }
            }
            push(8, [0.0, 0.0, height_mm / 2.0], ends[4]);
            push(9, [0.0, 0.0, -height_mm / 2.0], ends[5]);
        }
    }
    list
}

/// `blocker` with the size handle `code` (see [`sides`]) dragged `delta` mm outwards, in its
/// model frame. Each size stays within its range.
fn resized(blocker: Blocker, code: u8, delta: f64) -> Blocker {
    let size = |v: f64| v.clamp(*SIZE_MM.start(), *SIZE_MM.end());
    match blocker {
        Blocker::Sphere {
            centre_mm,
            diameter_mm,
        } => Blocker::Sphere {
            centre_mm,
            diameter_mm: size(diameter_mm + 2.0 * delta),
        },
        Blocker::Frustum {
            mut centre_mm,
            rotation_deg,
            mut bottom_mm,
            mut top_mm,
            mut height_mm,
        } => {
            let k = usize::from(code % 4 / 2);
            match code {
                0..=3 => bottom_mm[k] = size(bottom_mm[k] + 2.0 * delta),
                4..=7 => {
                    top_mm[k] = (top_mm[k] + 2.0 * delta).clamp(*TOP_MM.start(), *TOP_MM.end());
                }
                _ => {
                    // One base moves, the other stays: the centre goes half the way.
                    let height = size(height_mm + delta);
                    let shift = (height - height_mm) / 2.0 * if code == 8 { 1.0 } else { -1.0 };
                    let turn = euler_matrix(rotation_deg);
                    for (r, row) in turn.iter().enumerate() {
                        centre_mm[r] += row[2] * shift;
                    }
                    height_mm = height;
                }
            }
            Blocker::Frustum {
                centre_mm,
                rotation_deg,
                bottom_mm,
                top_mm,
                height_mm,
            }
        }
    }
}

/// `blocker` with its centre at `centre_mm`.
fn moved(blocker: Blocker, centre_mm: [f64; 3]) -> Blocker {
    match blocker {
        Blocker::Sphere { diameter_mm, .. } => Blocker::Sphere {
            centre_mm,
            diameter_mm,
        },
        Blocker::Frustum {
            rotation_deg,
            bottom_mm,
            top_mm,
            height_mm,
            ..
        } => Blocker::Frustum {
            centre_mm,
            rotation_deg,
            bottom_mm,
            top_mm,
            height_mm,
        },
    }
}

/// The ray parameter of `ray`'s first hit with `blocker` of a model placed by `map`.
fn ray_blocker(ray: &Ray, map: &Affine, blocker: &Blocker) -> Option<f64> {
    let back = map.inverse()?;
    // The ray in the model frame: an affine map keeps the ray's parameter.
    let origin = back.point(ray.origin);
    let direction = back.vector(ray.direction);
    let centre = blocker.centre_mm();
    let from: [f64; 3] = std::array::from_fn(|k| origin[k] - centre[k]);
    match *blocker {
        Blocker::Sphere { diameter_mm, .. } => {
            let r = diameter_mm / 2.0;
            let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
            let (a, b, c) = (
                dot(direction, direction),
                dot(from, direction),
                dot(from, from) - r * r,
            );
            let disc = b * b - a * c;
            if a <= 0.0 || disc < 0.0 {
                return None;
            }
            let root = disc.sqrt();
            [(-b - root) / a, (-b + root) / a]
                .into_iter()
                .find(|t| *t >= 0.0)
        }
        Blocker::Frustum {
            bottom_mm,
            top_mm,
            height_mm,
            ..
        } => {
            // Into the box's own frame: its turn's transpose.
            let turn = turn_of(blocker);
            let own =
                |v: [f64; 3]| std::array::from_fn(|c| (0..3).map(|r| turn[r][c] * v[r]).sum());
            let (origin, direction) = (own(from), own(direction));
            let corners = frustum_points(bottom_mm, top_mm, height_mm);
            FRUSTUM_FACES
                .iter()
                .filter_map(|f| ray_triangle(origin, direction, f.map(|i| corners[i as usize])))
                .min_by(f64::total_cmp)
        }
    }
}

/// The turn of object `index` of `workspace`: its rotation, without its scale.
fn pose_of(workspace: &Workspace, index: usize) -> [[f64; 3]; 3] {
    euler_matrix(
        workspace
            .plate()
            .objects
            .get(index)
            .map_or([0.0; 3], |o| o.pose.rotate_deg),
    )
}

/// `a · b` for rotations (rows).
fn product(a: [[f64; 3]; 3], b: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    std::array::from_fn(|r| std::array::from_fn(|c| (0..3).map(|k| a[r][k] * b[k][c]).sum()))
}

/// The transpose of `m`: a rotation's inverse.
fn transposed(m: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    std::array::from_fn(|r| std::array::from_fn(|c| m[c][r]))
}

impl BlockersUi {
    /// The draft of object `index`: kept while it is the same object and its applied blockers
    /// have not changed since, started again from them otherwise.
    pub(super) fn draft(&mut self, workspace: &Workspace, index: usize) -> &mut Draft {
        let Some(object) = workspace.plate().objects.get(index) else {
            return self.draft.insert(Draft {
                object: index,
                base: Vec::new(),
                edit: Vec::new(),
            });
        };
        let current = self
            .draft
            .as_ref()
            .is_some_and(|d| d.object == index && object.blockers().eq(d.base.iter()));
        if !current {
            let base: Vec<Blocker> = object.blockers().copied().collect();
            self.selected = None;
            self.drag = None;
            self.draft = Some(Draft {
                object: index,
                edit: base.clone(),
                base,
            });
        }
        self.draft.get_or_insert_with(|| Draft {
            object: index,
            base: Vec::new(),
            edit: Vec::new(),
        })
    }

    /// Object `index`'s blockers as edited.
    pub fn edited(&mut self, workspace: &Workspace, index: usize) -> Vec<Blocker> {
        self.draft(workspace, index).edit.clone()
    }

    /// How many of object `index`'s blockers are new or changed, and whether any applied one
    /// was removed.
    pub fn changes(&mut self, workspace: &Workspace, index: usize) -> (usize, bool) {
        let draft = self.draft(workspace, index);
        let changed = draft
            .edit
            .iter()
            .filter(|b| !draft.base.contains(b))
            .count();
        let removed = draft.base.iter().any(|b| !draft.edit.contains(b));
        (changed, removed)
    }

    /// The draft applied: it is now what the model has.
    pub fn applied(&mut self) {
        self.draft = None;
        self.selected = None;
        self.drag = None;
    }

    /// Whether a gizmo drag is in progress.
    #[must_use]
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Whether a blocker is selected.
    #[must_use]
    pub fn has_selection(&self) -> bool {
        self.selected.is_some()
    }

    /// Stop placing and dragging: the tool closed.
    pub fn close(&mut self) {
        self.placing = false;
        self.drag = None;
    }

    /// Escape: stop dragging or placing, else drop the selection. Returns whether there was
    /// something to stop.
    pub fn escape(&mut self) -> bool {
        if self.drag.take().is_some() || self.placing {
            self.placing = false;
            return true;
        }
        self.selected.take().is_some()
    }

    /// Delete the selected blocker from the draft. Returns whether there was one.
    pub fn delete(&mut self) -> bool {
        let (Some(selected), Some(draft)) = (self.selected.take(), self.draft.as_mut()) else {
            return false;
        };
        self.drag = None;
        if selected < draft.edit.len() {
            draft.edit.remove(selected);
        }
        true
    }

    /// A click along `ray` on object `index`, which `hit` met (where the section shows it): a
    /// blocker under the pointer is selected; while placing, a new one is put centred on the
    /// model's surface. Returns whether the click was taken.
    pub fn click(
        &mut self,
        workspace: &Workspace,
        ray: &Ray,
        hit: Option<SurfaceHit>,
        index: usize,
    ) -> bool {
        let Some(map) = workspace.model_to_world(index) else {
            return false;
        };
        let draft = self.draft(workspace, index);
        let under = draft
            .edit
            .iter()
            .enumerate()
            .filter_map(|(i, b)| Some((ray_blocker(ray, &map, b)?, i)))
            .min_by(|a, b| a.0.total_cmp(&b.0));
        let surface = hit.filter(|h| h.object == index);
        // While placing, the model's surface in front of a blocker takes the new one.
        let nearer_surface = surface.is_some_and(|h| {
            let t: f64 = (0..3)
                .map(|k| (h.point[k] - ray.origin[k]) * ray.direction[k])
                .sum();
            under.is_none_or(|(u, _)| t < u)
        });
        if self.placing && nearer_surface {
            if let (Some(hit), Some(back)) = (surface, map.inverse()) {
                let blocker = self.next.at(back.point(hit.point));
                let draft = self.draft(workspace, index);
                draft.edit.push(blocker);
                self.selected = Some(draft.edit.len() - 1);
                self.reveal = true;
                return true;
            }
        }
        if let Some((_, i)) = under {
            self.selected = Some(i);
            self.reveal = true;
            return true;
        }
        // A click on the model away from the blockers drops the selection, and keeps the
        // model's.
        self.selected.take().is_some() && surface.is_some()
    }

    /// The selected blocker of object `index`, its centre in the world, and its world turn.
    fn selected_in_world(&mut self, workspace: &Workspace, index: usize) -> Option<Selected> {
        let map = workspace.model_to_world(index)?;
        let pose = pose_of(workspace, index);
        let position = self.selected?;
        let blocker = *self.draft(workspace, index).edit.get(position)?;
        Some(Selected {
            position,
            blocker,
            anchor: map.point(blocker.centre_mm()),
            world: product(pose, turn_of(&blocker)),
        })
    }

    /// The selected blocker's centre in the world, where its gizmo stands.
    pub fn anchor(&mut self, workspace: &Workspace, index: usize) -> Option<[f64; 3]> {
        self.selected_in_world(workspace, index).map(|s| s.anchor)
    }

    /// The selected blocker's gizmo of `mode`, `size` mm long, along the axes of `space`: the
    /// world's, or the blocker's own (a sphere does not turn, and scales alike in every
    /// direction); then its size handles.
    pub fn handles(
        &mut self,
        workspace: &Workspace,
        index: usize,
        mode: Mode,
        space: Space,
        size: f64,
    ) -> Vec<Handle> {
        let Some(Selected {
            blocker,
            anchor,
            world,
            ..
        }) = self.selected_in_world(workspace, index)
        else {
            return Vec::new();
        };
        let sphere = matches!(blocker, Blocker::Sphere { .. });
        let mode = match mode {
            Mode::Rotate if sphere => Mode::Move,
            mode => mode,
        };
        let mut handles = manipulate::handles(mode, anchor, size, matrix_euler(world), space);
        // A sphere scales alike in every direction: its diameter.
        if sphere {
            handles.retain(|h| !matches!(h.kind, Kind::Scale(_)));
        }
        if let Some(map) = workspace.model_to_world(index) {
            handles.extend(sides(&blocker).into_iter().map(|(code, point, _)| {
                let at = map.point(point);
                Handle {
                    kind: Kind::Size(code),
                    points: vec![at, at],
                }
            }));
        }
        handles
    }

    /// Whether the selected blocker is a sphere; `None` without a selection.
    #[must_use]
    pub fn selected_sphere(&self) -> Option<bool> {
        let blocker = self.draft.as_ref()?.edit.get(self.selected?)?;
        Some(matches!(blocker, Blocker::Sphere { .. }))
    }

    /// A press on `handles` (the gizmo's, as [`Self::handles`] gave them) at handle `hit`,
    /// along `ray`: starts dragging the selected blocker of object `index`.
    pub fn press(
        &mut self,
        workspace: &Workspace,
        index: usize,
        handles: &[Handle],
        hit: Option<usize>,
        ray: &Ray,
        space: Space,
    ) -> bool {
        let Some(kind) = hit.and_then(|h| handles.get(h)).map(|h| h.kind) else {
            return false;
        };
        let Some(Selected {
            position,
            blocker,
            anchor,
            world,
        }) = self.selected_in_world(workspace, index)
        else {
            return false;
        };
        let Some(map) = workspace.model_to_world(index) else {
            return false;
        };
        let Some(back) = map.inverse() else {
            return false;
        };
        // Scaling is along the blocker's own sides, whatever the space.
        let mut axes = if matches!(kind, Kind::Scale(_) | Kind::Uniform) {
            Space::Local.axes(matrix_euler(world))
        } else {
            space.axes(matrix_euler(world))
        };
        let (mut anchor, mut stretch) = (anchor, 1.0);
        // A size handle works from where it is, along its outward direction in the world.
        if let Kind::Size(code) = kind {
            let Some((_, point, out)) = sides(&blocker).into_iter().find(|s| s.0 == code) else {
                return false;
            };
            let along = map.vector(out);
            stretch = (0..3).map(|k| along[k] * along[k]).sum::<f64>().sqrt();
            if stretch < 1e-12 {
                return false;
            }
            anchor = map.point(point);
            axes[0] = along.map(|c| c / stretch);
        }
        self.drag = Some(Drag {
            kind,
            index: position,
            anchor,
            start_ray: *ray,
            start: blocker,
            axes,
            pose: pose_of(workspace, index),
            back,
            space,
            stretch,
        });
        true
    }

    /// Follow the drag to the pointer's `ray`; `snap` turns in steps of 15 degrees.
    pub fn drag_to(&mut self, ray: &Ray, snap: bool) {
        let Some(drag) = &self.drag else {
            return;
        };
        let Some(blocker) = dragged(drag, ray, snap) else {
            return;
        };
        if let Some(edit) = self.draft.as_mut().and_then(|d| d.edit.get_mut(drag.index)) {
            *edit = blocker;
        }
    }

    /// The drag ended.
    pub fn release(&mut self) {
        self.drag = None;
    }

    /// Set the selected blocker of object `index` to `blocker`, from its panel's fields.
    pub(super) fn set_selected(&mut self, workspace: &Workspace, index: usize, blocker: Blocker) {
        let Some(selected) = self.selected else {
            return;
        };
        if let Some(edit) = self.draft(workspace, index).edit.get_mut(selected) {
            *edit = blocker;
        }
    }

    /// Add a box over object `index`'s whole footprint, from below its lowest point up to the
    /// Solid base's height above it: a base kept solid, a blocker like any other afterwards.
    pub fn add_solid_base(&mut self, workspace: &mut Workspace, index: usize) -> bool {
        let Some((_, placement)) = workspace.anchor(index) else {
            return false;
        };
        let (Some(map), height) = (workspace.model_to_world(index), self.next.base_mm) else {
            return false;
        };
        let Some(back) = map.inverse() else {
            return false;
        };
        let (min, max) = (placement.min, placement.max);
        let low = [
            min.x - BASE_MARGIN_MM,
            min.y - BASE_MARGIN_MM,
            min.z - BASE_MARGIN_MM,
        ];
        let high = [
            max.x + BASE_MARGIN_MM,
            max.y + BASE_MARGIN_MM,
            min.z + height,
        ];
        let centre: [f64; 3] = std::array::from_fn(|k| f64::midpoint(low[k], high[k]));
        // Square with the world: turned back by the model's turn, its sides measured along
        // the model frame's axes as they are scaled.
        let turn = transposed(pose_of(workspace, index));
        let size: [f64; 3] = std::array::from_fn(|k| {
            let axis = std::array::from_fn(|r| turn[r][k]);
            let along = map.vector(axis);
            let stretch = (0..3).map(|c| along[c] * along[c]).sum::<f64>().sqrt();
            (high[k] - low[k]) / stretch.max(1e-12)
        });
        let blocker = Blocker::Frustum {
            centre_mm: back.point(centre),
            rotation_deg: matrix_euler(turn),
            bottom_mm: [size[0], size[1]],
            top_mm: [size[0], size[1]],
            height_mm: size[2],
        };
        let draft = self.draft(workspace, index);
        draft.edit.push(blocker);
        self.selected = Some(draft.edit.len() - 1);
        self.reveal = true;
        true
    }

    /// The draws of object `focus`'s blockers, while the Hollow tool shows it.
    pub fn draws(
        &mut self,
        viewport: &mut Viewport,
        state: &egui_wgpu::RenderState,
        workspace: &Workspace,
        focus: Option<usize>,
        appearance: &Appearance,
    ) -> Vec<Draw> {
        let Some((index, map)) = focus.and_then(|i| Some((i, workspace.model_to_world(i)?))) else {
            return Vec::new();
        };
        let place = matrix(&map);
        let selected = self.selected;
        let draft = self.draft(workspace, index).clone();
        let mut draws = Vec::new();
        let mut used = Vec::new();
        for (i, blocker) in draft.edit.iter().enumerate() {
            let shape = shape(blocker);
            let key = if let Some(&key) = self.meshes.get(&shape) {
                key
            } else {
                let Some(mesh) = mesh_of(blocker) else {
                    continue;
                };
                self.keys += 1;
                let key = MESH_KEYS | self.keys & 0xFFFF_FFFF;
                viewport.upload(&state.device, key, &mesh, [0.0; 3]);
                self.meshes.insert(shape, key);
                key
            };
            used.push(shape);
            let rgba = if selected == Some(i) {
                appearance.blocker_selected
            } else if draft.base.contains(blocker) {
                appearance.blocker
            } else {
                appearance.blocker_new
            };
            let color = rgba.linear();
            let mut hidden = color;
            hidden[3] *= HIDDEN_SHARE;
            let model = place.mul(&own_frame(blocker));
            let draw = |color, overlay, look| Draw {
                mesh: key,
                model,
                color,
                overlay,
                cap: Cap::Off,
                holes: [0, 0],
                bore: None,
                outside: None,
                triangles: None,
                look,
            };
            draws.push(draw(color, false, Look::Ghost));
            draws.push(draw(hidden, true, Look::Solid));
        }
        // Shapes no blocker has any more are freed: sizes typed one digit at a time leave many.
        self.meshes.retain(|shape, key| {
            let keep = used.contains(shape);
            if !keep {
                viewport.remove(*key);
            }
            keep
        });
        draws
    }

    /// The list window of object `index`'s blockers, while open. Returns where the view should
    /// go, after a double click on a row.
    #[allow(clippy::too_many_lines)] // The window's spec, rows and events.
    pub fn list_window(
        &mut self,
        ctx: &egui::Context,
        workspace: &Workspace,
        index: Option<usize>,
        appearance: &Appearance,
    ) -> Option<[[f64; 3]; 2]> {
        if !self.list_open {
            return None;
        }
        let index = index.filter(|&i| i < workspace.plate().objects.len());
        let draft = match index {
            Some(i) => self.draft(workspace, i).clone(),
            None => Draft {
                object: 0,
                base: Vec::new(),
                edit: Vec::new(),
            },
        };
        let changed = draft
            .edit
            .iter()
            .filter(|b| !draft.base.contains(b))
            .count();
        let items: Vec<usize> = (0..draft.edit.len()).collect();
        let selected = self.selected;
        let reveal = std::mem::take(&mut self.reveal)
            .then_some(selected)
            .flatten();
        let spec = Spec {
            id: "blockers-list",
            title: tr!("blockers-list-title", count = draft.edit.len()),
            fact: tr!("blockers-list-shape"),
            state: tr!("list-state"),
            tabs: Vec::new(),
            filters: vec![
                tr!("list-all"),
                tr!("blockers-list-changed", count = changed),
            ],
            places: Vec::new(),
            summary: if index.is_some() {
                tr!(
                    "blockers-list-summary",
                    count = draft.edit.len(),
                    changed = changed
                )
            } else {
                tr!("panel-no-model")
            },
            remove: Some(tr!("blockers-remove")),
        };
        let mut only = 0;
        let row = |i: usize| {
            let blocker = draft.edit.get(i);
            let applied = blocker.is_some_and(|b| draft.base.contains(b));
            let colour = if applied {
                appearance.blocker
            } else {
                appearance.blocker_new
            };
            Row {
                dot: egui::Color32::from_rgb(colour.0[0], colour.0[1], colour.0[2]),
                name: tr!("blockers-name", number = i + 1),
                fact: blocker.map_or_else(String::new, describe),
                state: if applied {
                    tr!("blockers-applied")
                } else {
                    tr!("blockers-new")
                },
                state_colour: if applied {
                    crate::theme::TEXT_PRIMARY.color()
                } else {
                    crate::theme::WARNING.color()
                },
                hint: tr!("blockers-row-hint"),
                selected: selected == Some(i),
            }
        };
        let event = list_window::show(
            ctx,
            &spec,
            &mut self.list_open,
            &mut 0,
            &mut only,
            &mut 0,
            &items,
            reveal,
            row,
        );
        let index = index?;
        match event? {
            Event::Picked(i) => {
                self.selected = Some(i);
                None
            }
            Event::Focused(i) => {
                self.selected = Some(i);
                let blocker = draft.edit.get(i)?;
                let at = workspace.model_to_world(index)?.point(blocker.centre_mm());
                let half = reach_mm(blocker) + AROUND_MM;
                Some([at.map(|c| c - half), at.map(|c| c + half)])
            }
            Event::Removed(i) => {
                self.selected = Some(i);
                self.delete();
                None
            }
        }
    }
}

/// How far `blocker` reaches from its centre, in its model frame, in mm.
fn reach_mm(blocker: &Blocker) -> f64 {
    match *blocker {
        Blocker::Sphere { diameter_mm, .. } => diameter_mm / 2.0,
        Blocker::Frustum {
            bottom_mm,
            top_mm,
            height_mm,
            ..
        } => {
            let w = bottom_mm[0].max(top_mm[0]);
            let d = bottom_mm[1].max(top_mm[1]);
            (w * w + d * d + height_mm * height_mm).sqrt() / 2.0
        }
    }
}

/// What `blocker` is, for its list row: a sphere's diameter, a box's sides.
fn describe(blocker: &Blocker) -> String {
    match *blocker {
        Blocker::Sphere { diameter_mm, .. } => {
            tr!(
                "blockers-sphere-fact",
                diameter = with_unit(diameter_mm, 1, Unit::Millimetre)
            )
        }
        Blocker::Frustum {
            bottom_mm,
            top_mm,
            height_mm,
            ..
        } => {
            let sides = |[w, d]: [f64; 2]| format!("{}×{}", decimal(w, 1), decimal(d, 1));
            tr!(
                "blockers-box-fact",
                bottom = sides(bottom_mm),
                top = sides(top_mm),
                height = with_unit(height_mm, 1, Unit::Millimetre)
            )
        }
    }
}

/// `blocker` scaled by `factor` along its own `axis` (0 its width, 1 its depth, 2 its height),
/// or along all three with `None`, about its centre; a sphere's diameter scales either way.
/// Each size stays within its range.
fn scaled(blocker: Blocker, axis: Option<usize>, factor: f64) -> Blocker {
    let size = |v: f64| (v * factor).clamp(*SIZE_MM.start(), *SIZE_MM.end());
    let top = |v: f64| (v * factor).clamp(*TOP_MM.start(), *TOP_MM.end());
    match blocker {
        Blocker::Sphere {
            centre_mm,
            diameter_mm,
        } => Blocker::Sphere {
            centre_mm,
            diameter_mm: size(diameter_mm),
        },
        Blocker::Frustum {
            centre_mm,
            rotation_deg,
            mut bottom_mm,
            mut top_mm,
            mut height_mm,
        } => {
            for k in 0..2 {
                if axis.is_none_or(|a| a == k) {
                    bottom_mm[k] = size(bottom_mm[k]);
                    top_mm[k] = top(top_mm[k]);
                }
            }
            if axis.is_none_or(|a| a == 2) {
                height_mm = size(height_mm);
            }
            Blocker::Frustum {
                centre_mm,
                rotation_deg,
                bottom_mm,
                top_mm,
                height_mm,
            }
        }
    }
}

/// The blocker `drag` gives with the pointer along `ray`; `None` where the pointer gives no
/// position (a ray along the handle).
fn dragged(drag: &Drag, ray: &Ray, snap: bool) -> Option<Blocker> {
    let start = drag.start;
    let centre = start.centre_mm();
    let shift = |world: [f64; 3]| {
        let model = drag.back.vector(world);
        moved(start, std::array::from_fn(|k| centre[k] + model[k]))
    };
    match drag.kind {
        Kind::Axis(axis) => {
            let direction = drag.axes[axis];
            let from = along_axis(&drag.start_ray, drag.anchor, direction)?;
            let to = along_axis(ray, drag.anchor, direction)?;
            Some(shift(direction.map(|c| c * (to - from))))
        }
        Kind::Plane => {
            let up = [0.0, 0.0, 1.0];
            let from = on_plane(&drag.start_ray, drag.anchor, up)?;
            let to = on_plane(ray, drag.anchor, up)?;
            Some(shift([to[0] - from[0], to[1] - from[1], 0.0]))
        }
        Kind::Ring(axis) => {
            let Blocker::Frustum {
                centre_mm,
                rotation_deg,
                bottom_mm,
                top_mm,
                height_mm,
            } = start
            else {
                return None;
            };
            let normal = drag.axes[axis];
            let from = on_plane(&drag.start_ray, drag.anchor, normal)?;
            let to = on_plane(ray, drag.anchor, normal)?;
            let mut degrees = angle_about(drag.anchor, normal, from, to);
            if snap {
                degrees = (degrees / 15.0).round() * 15.0;
            }
            let rotation_deg = match drag.space {
                // About its own axis: before its turn, in the model frame alike.
                Space::Local => turned_local(rotation_deg, axis, degrees),
                // About a world axis: turned in the world, then brought back by the model's.
                Space::Global => {
                    let world = matrix_euler(product(drag.pose, euler_matrix(rotation_deg)));
                    let world = euler_matrix(turned(world, axis, degrees));
                    matrix_euler(product(transposed(drag.pose), world))
                }
            };
            Some(Blocker::Frustum {
                centre_mm,
                rotation_deg,
                bottom_mm,
                top_mm,
                height_mm,
            })
        }
        Kind::Scale(axis) => {
            let direction = drag.axes[axis];
            let from = along_axis(&drag.start_ray, drag.anchor, direction)?;
            let to = along_axis(ray, drag.anchor, direction)?;
            if from.abs() < 1e-9 {
                return None;
            }
            Some(scaled(start, Some(axis), to / from))
        }
        Kind::Size(code) => {
            let direction = drag.axes[0];
            let from = along_axis(&drag.start_ray, drag.anchor, direction)?;
            let to = along_axis(ray, drag.anchor, direction)?;
            Some(resized(start, code, (to - from) / drag.stretch))
        }
        Kind::Uniform => {
            // On the plane through the centre facing the view, the ratio of the pointer's
            // distances from it.
            let normal = drag.start_ray.direction;
            let from = on_plane(&drag.start_ray, drag.anchor, normal)?;
            let to = on_plane(ray, drag.anchor, normal)?;
            let reach = |p: [f64; 3]| {
                (0..3)
                    .map(|k| (p[k] - drag.anchor[k]).powi(2))
                    .sum::<f64>()
                    .sqrt()
            };
            let start = reach(from);
            if start < 1e-9 {
                return None;
            }
            Some(scaled(drag.start, None, reach(to) / start))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The signed volume of `mesh`: positive when its faces turn outwards.
    fn volume(mesh: &TriangleMesh) -> f64 {
        let v = mesh.vertices();
        mesh.triangles()
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| v[i as usize]);
                (a.x * (b.y * c.z - b.z * c.y) - a.y * (b.x * c.z - b.z * c.x)
                    + a.z * (b.x * c.y - b.y * c.x))
                    / 6.0
            })
            .sum()
    }

    fn frustum(centre_mm: [f64; 3], rotation_deg: [f64; 3]) -> Blocker {
        Blocker::Frustum {
            centre_mm,
            rotation_deg,
            bottom_mm: [10.0, 6.0],
            top_mm: [4.0, 2.0],
            height_mm: 8.0,
        }
    }

    #[test]
    fn the_meshes_are_closed_outwards_and_as_large_as_their_blockers() {
        let mesh = mesh_of(&frustum([0.0; 3], [0.0; 3])).unwrap();
        // A prismatoid's volume: h/6 (A + a + 4 M), M the mid-height section, 7 × 4.
        let expected = 8.0 / 6.0 * (60.0 + 8.0 + 4.0 * 28.0);
        assert!((volume(&mesh) - expected).abs() < 1e-9, "{}", volume(&mesh));
        let ball = unit_sphere().unwrap();
        let v = volume(&ball);
        assert!(
            v > 0.95 * 4.0 / 3.0 * std::f64::consts::PI && v < 4.2,
            "{v}"
        );
    }

    #[test]
    fn rays_hit_blockers_where_they_stand_in_the_model_frame() {
        // A model scaled twice and moved: a blocker at its model point (5, 0, 0) stands at
        // world (20, 0, 0).
        let map = Affine {
            linear: [[2.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 2.0]],
            translation: [10.0, 0.0, 0.0],
        };
        let ray = Ray {
            origin: [20.0, 0.0, 100.0],
            direction: [0.0, 0.0, -1.0],
        };
        let ball = Blocker::Sphere {
            centre_mm: [5.0, 0.0, 0.0],
            diameter_mm: 4.0,
        };
        // Its top is at world height 4 (radius 2, scaled twice).
        let t = ray_blocker(&ray, &map, &ball).unwrap();
        assert!((t - 96.0).abs() < 1e-9, "{t}");
        // A box turned on its side about X: its depth upright, 4 at mid-height where the ray
        // passes, so its top there is at 2 · 2. Unturned, its top is at 4 · 2.
        let block = frustum([5.0, 0.0, 0.0], [90.0, 0.0, 0.0]);
        let t = ray_blocker(&ray, &map, &block).unwrap();
        assert!((t - 96.0).abs() < 1e-6, "{t}");
        let upright = frustum([5.0, 0.0, 0.0], [0.0; 3]);
        let t = ray_blocker(&ray, &map, &upright).unwrap();
        assert!((t - 92.0).abs() < 1e-6, "{t}");
        let away = Ray {
            origin: [40.0, 0.0, 100.0],
            direction: [0.0, 0.0, -1.0],
        };
        assert!(ray_blocker(&away, &map, &block).is_none());
    }

    #[test]
    fn a_drag_moves_and_turns_in_the_model_frame() {
        // A model turned 90 degrees about Z and scaled twice: world X is its model -Y.
        let pose = euler_matrix([0.0, 0.0, 90.0]);
        let linear = std::array::from_fn(|r| std::array::from_fn(|c| pose[r][c] * 2.0));
        let map = Affine {
            linear,
            translation: [0.0; 3],
        };
        let start = frustum([1.0, 2.0, 3.0], [0.0; 3]);
        let anchor = map.point(start.centre_mm());
        let ray_at = |x: f64| Ray {
            origin: [x, anchor[1], 100.0],
            direction: [0.0, 0.0, -1.0],
        };
        let drag = Drag {
            kind: Kind::Axis(0),
            index: 0,
            anchor,
            start_ray: ray_at(anchor[0]),
            start,
            axes: Space::Global.axes([0.0; 3]),
            pose,
            back: map.inverse().unwrap(),
            space: Space::Global,
            stretch: 1.0,
        };
        // 4 mm along world X: 2 mm along model -Y.
        let moved = dragged(&drag, &ray_at(anchor[0] + 4.0), false).unwrap();
        let c = moved.centre_mm();
        assert!(
            (c[0] - 1.0).abs() < 1e-9 && (c[1] - 0.0).abs() < 1e-9,
            "{c:?}"
        );
        // A turn about world Z turns it about the model's Z alike.
        let ring = Drag {
            kind: Kind::Ring(2),
            start_ray: Ray {
                origin: [anchor[0] + 5.0, anchor[1], 100.0],
                direction: [0.0, 0.0, -1.0],
            },
            ..drag
        };
        let turned = dragged(
            &ring,
            &Ray {
                origin: [anchor[0], anchor[1] + 5.0, 100.0],
                direction: [0.0, 0.0, -1.0],
            },
            false,
        )
        .unwrap();
        let Blocker::Frustum { rotation_deg, .. } = turned else {
            panic!("{turned:?}");
        };
        assert!((rotation_deg[2] - 90.0).abs() < 1e-6, "{rotation_deg:?}");
    }

    #[test]
    fn scale_handles_size_a_box_along_its_sides_and_a_sphere_alike() {
        let map = Affine {
            linear: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            translation: [0.0; 3],
        };
        // A box turned a quarter about Z: its width runs along world Y.
        let start = frustum([0.0; 3], [0.0, 0.0, 90.0]);
        let world = euler_matrix([0.0, 0.0, 90.0]);
        let down = |x: f64, y: f64| Ray {
            origin: [x, y, 100.0],
            direction: [0.0, 0.0, -1.0],
        };
        let drag = Drag {
            kind: Kind::Scale(0),
            index: 0,
            anchor: [0.0; 3],
            start_ray: down(0.0, 5.0),
            start,
            axes: Space::Local.axes(matrix_euler(world)),
            pose: euler_matrix([0.0; 3]),
            back: map.inverse().unwrap(),
            space: Space::Global,
            stretch: 1.0,
        };
        // Pulled from 5 to 10 mm along world Y: twice as wide, top and bottom alike.
        let Some(Blocker::Frustum {
            bottom_mm,
            top_mm,
            height_mm,
            ..
        }) = dragged(&drag, &down(0.0, 10.0), false)
        else {
            panic!("no box");
        };
        assert!((bottom_mm[0] - 20.0).abs() < 1e-9 && (top_mm[0] - 8.0).abs() < 1e-9);
        assert!((bottom_mm[1] - 6.0).abs() < 1e-9 && (height_mm - 8.0).abs() < 1e-9);
        // The uniform handle: a sphere's diameter grows with the pointer's distance.
        let ball = Blocker::Sphere {
            centre_mm: [0.0; 3],
            diameter_mm: 4.0,
        };
        let side = |x: f64| Ray {
            origin: [x, 0.0, 100.0],
            direction: [0.0, 0.0, -1.0],
        };
        let uniform = Drag {
            kind: Kind::Uniform,
            start: ball,
            start_ray: side(2.0),
            ..drag
        };
        let grown = dragged(&uniform, &side(3.0), false);
        assert!(matches!(
            grown,
            Some(Blocker::Sphere { diameter_mm, .. }) if (diameter_mm - 6.0).abs() < 1e-9
        ));
    }

    #[test]
    #[allow(clippy::float_cmp)] // Sizes untouched, or clamped to a range's end, exactly.
    fn size_handles_size_one_base_or_move_one_face_and_set_a_sphere_s_diameter() {
        // The test box (10 × 6 below, 4 × 2 above, 8 high) turned a quarter about Z at
        // (1, 2, 3): its width runs along the model's Y.
        let start = frustum([1.0, 2.0, 3.0], [0.0, 0.0, 90.0]);
        let handles = sides(&start);
        assert_eq!(handles.len(), 10);
        let near = |a: [f64; 3], b: [f64; 3]| (0..3).all(|k| (a[k] - b[k]).abs() < 1e-9);
        // The lower base's +X side: half its width along model +Y, at its height.
        let (_, point, out) = handles[0];
        assert!(near(point, [1.0, 7.0, -1.0]) && near(out, [0.0, 1.0, 0.0]));
        // The upper base's -Y side and the upper base itself.
        assert!(near(handles[7].1, [2.0, 2.0, 7.0]));
        assert!(near(handles[8].1, [1.0, 2.0, 7.0]) && near(handles[8].2, [0.0, 0.0, 1.0]));
        // Widening the lower base by 1 mm a side: 2 mm wider, the upper base as it was.
        let Blocker::Frustum {
            centre_mm,
            bottom_mm,
            top_mm,
            height_mm,
            ..
        } = resized(start, 0, 1.0)
        else {
            panic!("no box");
        };
        assert!((bottom_mm[0] - 12.0).abs() < 1e-9 && (bottom_mm[1] - 6.0).abs() < 1e-9);
        assert!(top_mm == [4.0, 2.0] && (height_mm - 8.0).abs() < 1e-9);
        assert!(near(centre_mm, [1.0, 2.0, 3.0]));
        // The upper base narrowed past nothing is an apex; the lower one keeps its least size.
        assert!(
            matches!(resized(start, 6, -5.0), Blocker::Frustum { top_mm, .. } if top_mm[1] == 0.0)
        );
        assert!(
            matches!(resized(start, 1, -50.0), Blocker::Frustum { bottom_mm, .. } if bottom_mm[0] == *SIZE_MM.start())
        );
        // The lower base moved down 2 mm: 10 high, the upper base where it was.
        let lowered = resized(start, 9, 2.0);
        let top = sides(&lowered)[8].1;
        assert!(near(top, [1.0, 2.0, 7.0]) && near(sides(&lowered)[9].1, [1.0, 2.0, -3.0]));
        // A sphere's handles sit at its surface and set its diameter.
        let ball = Blocker::Sphere {
            centre_mm: [0.0; 3],
            diameter_mm: 4.0,
        };
        assert!(near(sides(&ball)[5].1, [0.0, 0.0, -2.0]));
        // Dragged on a model twice as large: 3 world mm out are 1.5 model mm, 3 mm wider.
        let drag = Drag {
            kind: Kind::Size(1),
            index: 0,
            anchor: [-4.0, 0.0, 0.0],
            start_ray: Ray {
                origin: [-4.0, 0.0, 100.0],
                direction: [0.0, 0.0, -1.0],
            },
            start: ball,
            axes: [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            pose: euler_matrix([0.0; 3]),
            back: Affine {
                linear: [[0.5, 0.0, 0.0], [0.0, 0.5, 0.0], [0.0, 0.0, 0.5]],
                translation: [0.0; 3],
            },
            space: Space::Global,
            stretch: 2.0,
        };
        let pulled = Ray {
            origin: [-7.0, 0.0, 100.0],
            direction: [0.0, 0.0, -1.0],
        };
        assert!(matches!(
            dragged(&drag, &pulled, false),
            Some(Blocker::Sphere { diameter_mm, .. }) if (diameter_mm - 7.0).abs() < 1e-9
        ));
    }

    #[test]
    #[allow(clippy::float_cmp)] // Defaults kept exactly.
    fn new_blockers_take_valid_values_and_are_centred_where_asked() {
        let next = NewBlocker {
            height_mm: -1.0,
            top_mm: [0.0, 900.0],
            ..NewBlocker::default()
        }
        .valid();
        assert_eq!(next.height_mm, NewBlocker::default().height_mm);
        assert_eq!(next.top_mm, [0.0, NewBlocker::default().top_mm[1]]);
        assert_eq!(next.at([1.0, 2.0, 3.0]).centre_mm(), [1.0, 2.0, 3.0]);
        let ball = NewBlocker {
            sphere: true,
            ..next
        };
        assert!(matches!(
            ball.at([0.0; 3]),
            Blocker::Sphere { diameter_mm, .. } if diameter_mm == 8.0
        ));
    }

    #[test]
    fn blockers_are_placed_on_the_model_dragged_and_applied_with_the_hollowing() {
        let (path, directory) = crate::hollowing_ui::tests::cube_file("blockers");
        let mut workspace = Workspace::new();
        workspace.open(path);
        crate::workspace::tests::wait(&mut workspace);
        let map = workspace.model_to_world(0).unwrap();
        let top = map.point([10.0, 10.0, 20.0]);
        let down = Ray {
            origin: [top[0], top[1], top[2] + 50.0],
            direction: [0.0, 0.0, -1.0],
        };
        let near = |a: [f64; 3], b: [f64; 3]| (0..3).all(|k| (a[k] - b[k]).abs() < 1e-6);
        // A click while placing puts one centred on the point clicked, in the model frame.
        let mut ui = BlockersUi {
            placing: true,
            ..BlockersUi::default()
        };
        let hit = workspace.hit(&down);
        assert!(ui.click(&workspace, &down, hit, 0));
        let edited = ui.edited(&workspace, 0);
        assert_eq!(edited.len(), 1);
        assert!(
            near(edited[0].centre_mm(), [10.0, 10.0, 20.0]),
            "{edited:?}"
        );
        assert_eq!(ui.changes(&workspace, 0), (1, false));
        // Not placing, a click on it selects it.
        ui.placing = false;
        ui.selected = None;
        assert!(ui.click(&workspace, &down, hit, 0));
        assert_eq!(ui.selected, Some(0));
        // Its Z arrow drags it 6 mm down.
        let handles = ui.handles(&workspace, 0, Mode::Move, Space::Global, 10.0);
        let arrow = handles.iter().position(|h| h.kind == Kind::Axis(2));
        let side = |z: f64| Ray {
            origin: [top[0] + 50.0, top[1], z],
            direction: [-1.0, 0.0, 0.0],
        };
        assert!(ui.press(&workspace, 0, &handles, arrow, &side(top[2]), Space::Global));
        ui.drag_to(&side(top[2] - 6.0), false);
        ui.release();
        let edited = ui.edited(&workspace, 0);
        assert!(
            near(edited[0].centre_mm(), [10.0, 10.0, 14.0]),
            "{edited:?}"
        );
        // Applied with the hollowing: the model has it, and the draft has no changes.
        let hollowing = crate::hollowing_ui::DEFAULT_HOLLOWING;
        workspace.hollow_with(0, Some(hollowing), edited.clone());
        ui.applied();
        let object = &workspace.plate().objects[0];
        assert_eq!(object.blockers().copied().collect::<Vec<_>>(), edited);
        assert_eq!(ui.changes(&workspace, 0), (0, false));
        // Deleted from the draft, it waits for Apply hollowing.
        ui.selected = Some(0);
        assert!(ui.delete());
        assert_eq!(ui.changes(&workspace, 0), (0, true));
        // The solid base: over the footprint and a margin, up to 3 mm above the lowest point.
        assert!(ui.add_solid_base(&mut workspace, 0));
        let edited = ui.edited(&workspace, 0);
        let Some(&Blocker::Frustum {
            centre_mm,
            bottom_mm,
            height_mm,
            ..
        }) = edited.last()
        else {
            panic!("{edited:?}");
        };
        let (_, placement) = workspace.anchor(0).unwrap();
        let low = placement.min.z;
        let centre = map.point(centre_mm);
        assert!((centre[2] - (low + 1.0)).abs() < 1e-6, "{centre:?}");
        assert!((height_mm - 4.0).abs() < 1e-6 && (bottom_mm[0] - 22.0).abs() < 1e-6);
        // Remove hollowing takes the blockers with it.
        workspace.hollow_with(0, None, Vec::new());
        assert_eq!(workspace.plate().objects[0].blockers().count(), 0);
        std::fs::remove_dir_all(directory).ok();
    }
}
