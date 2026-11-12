//! The contact's height line and the held height (owner, 2026-11-06, TD-137).
//!
//! **The line.** While a support is placed or its tip dragged, the models' shader draws a line
//! around them at the height under the pointer ([`crate::viewport::Guide`]): it shows where a
//! row of supports stands, so that the next one can be put level with it.
//!
//! **Holding the height.** With Shift held, the height under the pointer when it was pressed
//! is kept: the tip goes where the surface at that height lies nearest the pointer's ray. The
//! surface's contour at that height is found once, when the height is held (one pass over the
//! surface's triangles, see [`contour`]), and kept while it is; each pointer move then looks
//! only at its edges ([`nearest`]).
//!
//! **Which point.** Where the pointer is on the model, the contour's point nearest the surface
//! under it, so that the tip stays on the side of the model the pointer is on; a point under the
//! pointer that lies on the held height already (a level face there, whose contour is only its
//! outline) is kept as it is. Off the model, the point the screen shows nearest the pointer:
//! by the angle between the ray and the line from the eye to it, and among those within
//! [`TIE_RAD`] of the nearest, the one nearest the eye. Only edges of triangles a support can
//! stand under (facing down, or vertical) are kept.
use super::{route::RouteSurface, SupportsUi, FACING_UP};
use crate::{
    camera::Ray,
    preparation_ui::PreparationUi,
    viewport::Section,
    workspace::{SurfaceHit, Workspace},
};
use karasu_core::mesh::TriangleMesh;
use std::sync::Arc;

/// Contour points this close in angle to the nearest are ties, in radians: the nearest to the
/// eye among them wins.
const TIE_RAD: f64 = 2.0e-3;

/// One edge of a surface's contour at a height: its ends, and the outward unit normal of the
/// triangle it crosses.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Edge {
    pub(crate) ends: [[f64; 3]; 2],
    pub(crate) normal: [f64; 3],
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// A height held with Shift, and the surface's contour there once found: the surface it was
/// found on and whether on its cavity.
#[derive(Default)]
pub(crate) struct Held {
    height_mm: f64,
    contour: Option<(Arc<RouteSurface>, bool, Vec<Edge>)>,
}

/// Where a moved tip goes: where the pointer meets the surface, or on the held height.
pub enum TipAt {
    /// Where the pointer's ray meets the surface.
    Free,
    /// On the held height (see the module documentation); nowhere where the section hides
    /// that height or no edge lies there.
    Held(Option<SurfaceHit>),
}

/// A point this close to the held height lies on it, in mm.
const LEVEL_MM: f64 = 1.0e-3;

impl SupportsUi {
    /// Whether a tip follows the pointer: a support being placed, or a tip dragged.
    fn tip_moving(&self) -> bool {
        self.placing || self.dragging_tip()
    }

    /// Hold the height under the pointer while `shift` is down; let it go when it is up.
    pub fn hold_height(&mut self, shift: bool) {
        if !shift || !self.tip_moving() {
            self.held = None;
        } else if self.held.is_none() {
            self.held = self.pointer_mm.map(|height_mm| Held {
                height_mm,
                contour: None,
            });
        }
    }

    /// The height of the contact's line, if a tip follows the pointer: the held height, or
    /// the one under the pointer.
    pub fn guide_height(&self) -> Option<f64> {
        if !self.tip_moving() {
            return None;
        }
        self.held.as_ref().map(|h| h.height_mm).or(self.pointer_mm)
    }

    /// Whether a height is held.
    pub fn holding(&self) -> bool {
        self.held.is_some()
    }

    /// Where a tip moved along `ray` on model `index` goes, `free` being where the ray meets
    /// the surface: on the held height, on its outside or its `cavity`, if a height is held
    /// and the surface for routes is ready; else free.
    pub fn tip_at(
        &mut self,
        workspace: &Workspace,
        preparation: &PreparationUi,
        (index, ray, free): (usize, &Ray, Option<SurfaceHit>),
        section: Section,
        cavity: bool,
    ) -> TipAt {
        let Some(height_mm) = self.held.as_ref().map(|h| h.height_mm) else {
            return TipAt::Free;
        };
        if section.below.is_some_and(|b| height_mm < b)
            || section.above.is_some_and(|a| height_mm > a)
        {
            return TipAt::Held(None);
        }
        if free.is_some_and(|f| (f.point[2] - height_mm).abs() <= LEVEL_MM) {
            return TipAt::Held(free);
        }
        let Some(surface) = self.route_surface(workspace, preparation, index) else {
            return TipAt::Free;
        };
        let Some(held) = self.held.as_mut() else {
            return TipAt::Free;
        };
        let current = held
            .contour
            .as_ref()
            .is_some_and(|(s, c, _)| Arc::ptr_eq(s, &surface) && *c == cavity);
        if !current {
            let Some(mesh) = surface.mesh() else {
                return TipAt::Free;
            };
            let edges = contour(
                mesh,
                height_mm,
                &|t| surface.is_cavity(t) == cavity,
                FACING_UP,
            );
            held.contour = Some((Arc::clone(&surface), cavity, edges));
        }
        let edges = held.contour.as_ref().map_or(&[][..], |c| &c.2[..]);
        let near = free.filter(|f| f.object == index).map(|f| f.point);
        TipAt::Held(nearest(edges, ray, near).map(|(point, normal)| SurfaceHit {
            object: index,
            point,
            normal,
        }))
    }
}

/// The contour of `mesh` (world mm) at `height_mm`: where each triangle `keep` accepts, facing
/// down at least as far as `facing_up` allows (the normal's height at most that), crosses the
/// height. A triangle lying in the height's plane has no edge there.
pub(crate) fn contour(
    mesh: &TriangleMesh,
    height_mm: f64,
    keep: &dyn Fn(u32) -> bool,
    facing_up: f64,
) -> Vec<Edge> {
    let vertices = mesh.vertices();
    let mut out = Vec::new();
    for (t, triangle) in mesh.triangles().iter().enumerate() {
        let p = triangle.map(|v| {
            let v = vertices[v as usize];
            [v.x, v.y, v.z]
        });
        let (low, high) = (
            p[0][2].min(p[1][2]).min(p[2][2]),
            p[0][2].max(p[1][2]).max(p[2][2]),
        );
        // A triangle lying in the height's plane, exactly level, has no edge there.
        #[allow(clippy::float_cmp)]
        if height_mm < low || height_mm > high || low == high {
            continue;
        }
        let n = cross(sub(p[1], p[0]), sub(p[2], p[0]));
        let length = dot(n, n).sqrt();
        if length == 0.0 {
            continue;
        }
        let normal = n.map(|c| c / length);
        #[allow(clippy::cast_possible_truncation)] // Triangle indices fit u32 (the mesh's own).
        if normal[2] > facing_up || !keep(t as u32) {
            continue;
        }
        // Where the triangle's sides cross the height: two points (a vertex on it counts once
        // per side meeting there, so the first two distinct ones are kept).
        let mut cut: Vec<[f64; 3]> = Vec::with_capacity(3);
        for (a, b) in [(p[0], p[1]), (p[1], p[2]), (p[2], p[0])] {
            let (za, zb) = (a[2] - height_mm, b[2] - height_mm);
            if (za <= 0.0 && zb >= 0.0) || (za >= 0.0 && zb <= 0.0) {
                // A side lying in the height's plane, exactly: its ends are taken by the others.
                #[allow(clippy::float_cmp)]
                if za == zb {
                    continue;
                }
                let s = za / (za - zb);
                let q = [
                    a[0] + (b[0] - a[0]) * s,
                    a[1] + (b[1] - a[1]) * s,
                    height_mm,
                ];
                if !cut.iter().any(|c| dot(sub(*c, q), sub(*c, q)) < 1e-18) {
                    cut.push(q);
                }
            }
        }
        if let [a, b, ..] = cut[..] {
            out.push(Edge {
                ends: [a, b],
                normal,
            });
        }
    }
    out
}

/// The point of `edges` nearest `near`, or without it nearest `ray` as the screen shows it,
/// with its edge's normal: see the module documentation. Behind the eye, none.
pub(crate) fn nearest(
    edges: &[Edge],
    ray: &Ray,
    near: Option<[f64; 3]>,
) -> Option<([f64; 3], [f64; 3])> {
    if let Some(near) = near {
        return edges
            .iter()
            .map(|edge| {
                let [a, b] = edge.ends;
                let along = sub(b, a);
                let length = dot(along, along);
                let s = if length > 1e-18 {
                    (dot(sub(near, a), along) / length).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let point = [
                    a[0] + along[0] * s,
                    a[1] + along[1] * s,
                    a[2] + along[2] * s,
                ];
                (dot(sub(point, near), sub(point, near)), point, edge.normal)
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, point, normal)| (point, normal));
    }
    // Each edge's point nearest the ray: its angle from the ray, its depth, and the point.
    let mut found: Vec<(f64, f64, [f64; 3], [f64; 3])> = Vec::new();
    for edge in edges {
        let [start, end] = edge.ends;
        let along = sub(end, start);
        let w = sub(start, ray.origin);
        let (aa, ab, bb) = (dot(along, along), dot(along, ray.direction), 1.0);
        let (aw, bw) = (dot(along, w), dot(ray.direction, w));
        // Closest points of the segment (a + s along) and the ray (origin + t direction).
        let denominator = aa * bb - ab * ab;
        let mut s = if denominator > 1e-18 {
            ((ab * bw - bb * aw) / denominator).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let point = [
            start[0] + along[0] * s,
            start[1] + along[1] * s,
            start[2] + along[2] * s,
        ];
        let to = sub(point, ray.origin);
        let t = dot(to, ray.direction);
        if t <= 0.0 {
            s = -1.0;
        }
        if s < 0.0 {
            continue;
        }
        let off = dot(to, to) - t * t;
        let angle = off.max(0.0).sqrt().atan2(t);
        found.push((angle, t, point, edge.normal));
    }
    let least = found.iter().map(|f| f.0).fold(f64::INFINITY, f64::min);
    found
        .into_iter()
        .filter(|f| f.0 <= least + TIE_RAD)
        .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.total_cmp(&b.0)))
        .map(|(_, _, point, normal)| (point, normal))
}

#[cfg(test)]
mod tests {
    use super::*;
    use karasu_core::geometry::Point3;

    /// A closed box from `min` to `max`, its faces wound outward.
    fn cube(min: [f64; 3], max: [f64; 3]) -> TriangleMesh {
        let corner = |i: usize| -> Point3 {
            let c: [f64; 3] =
                std::array::from_fn(|a| if i >> a & 1 == 1 { max[a] } else { min[a] });
            Point3::new(c[0], c[1], c[2])
        };
        let vertices = (0..8).map(corner).collect();
        let mut triangles = Vec::new();
        for q in [
            [0, 2, 3, 1],
            [4, 5, 7, 6],
            [0, 1, 5, 4],
            [2, 6, 7, 3],
            [0, 4, 6, 2],
            [1, 3, 7, 5],
        ] {
            triangles.push([q[0], q[1], q[2]]);
            triangles.push([q[0], q[2], q[3]]);
        }
        TriangleMesh::try_new(vertices, triangles).unwrap()
    }

    #[test]
    fn a_held_height_keeps_the_tip_on_it_until_shift_is_let_go() {
        use crate::supports_ui::tests::{lifted_cube, ray};
        use std::time::{Duration, Instant};
        let (mut workspace, preparation, directory) = lifted_cube();
        let mut ui = SupportsUi {
            placing: true,
            route_wanted: true,
            ..SupportsUi::default()
        };
        let started = Instant::now();
        while ui.route_surface(&workspace, &preparation, 0).is_none() {
            ui.poll(&workspace, &preparation, Some(0));
            assert!(started.elapsed() < Duration::from_secs(60), "no surface");
            std::thread::sleep(Duration::from_millis(10));
        }
        let placed = workspace.placement_of(0).unwrap();
        let (min, max) = (placed.min.coordinates(), placed.max.coordinates());
        let [cx, cy] = [0, 1].map(|k| f64::midpoint(min[k], max[k]));
        // Nothing held, no line, until a tip follows the pointer over the model.
        ui.hold_height(true);
        assert!(!ui.holding() && ui.guide_height().is_none());
        // Under the cube's bottom: the line at its height, then held there.
        let under = ray(cx, cy, true);
        let hit = workspace.hit(&under);
        ui.hover(&workspace, hit, 0);
        let bottom = min[2];
        assert!((ui.guide_height().unwrap() - bottom).abs() < 1e-9);
        ui.hold_height(true);
        assert!(ui.holding());
        // The pointer on the front wall, 3 mm higher: the tip goes down the wall to the held
        // height, under the pointer; the line stays there.
        let wall = Ray {
            origin: [cx + 1.0, min[1] - 50.0, bottom + 3.0],
            direction: [0.0, 1.0, 0.0],
        };
        let section = Section::default();
        let free = workspace.hit_in(&wall, section);
        assert!((free.unwrap().point[2] - bottom - 3.0).abs() < 1e-9);
        let TipAt::Held(Some(tip)) =
            ui.tip_at(&workspace, &preparation, (0, &wall, free), section, false)
        else {
            panic!("no tip on the held height");
        };
        assert!((tip.point[2] - bottom).abs() < 1e-9, "{tip:?}");
        assert!((tip.point[0] - cx - 1.0).abs() < 1e-6 && (tip.point[1] - min[1]).abs() < 1e-6);
        ui.hover(&workspace, Some(tip), 0);
        assert!((ui.guide_height().unwrap() - bottom).abs() < 1e-9);
        // Under the bottom elsewhere: a level face on the held height keeps its own point.
        let aside = ray(cx + 2.0, cy - 1.0, true);
        let free = workspace.hit_in(&aside, section);
        let TipAt::Held(Some(kept)) =
            ui.tip_at(&workspace, &preparation, (0, &aside, free), section, false)
        else {
            panic!("no tip on the bottom");
        };
        assert_eq!(Some(kept), free);
        // A section that hides the held height: nowhere.
        let above = Section {
            below: Some(bottom + 1.0),
            above: None,
        };
        assert!(matches!(
            ui.tip_at(&workspace, &preparation, (0, &wall, free), above, false),
            TipAt::Held(None)
        ));
        // Shift let go: free again; placing stopped: no line.
        ui.hold_height(false);
        assert!(matches!(
            ui.tip_at(&workspace, &preparation, (0, &wall, free), section, false),
            TipAt::Free
        ));
        ui.placing = false;
        assert!(ui.guide_height().is_none());
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn a_box_s_contour_at_a_height_is_its_four_walls() {
        let mesh = cube([0.0; 3], [10.0, 10.0, 10.0]);
        let edges = contour(&mesh, 4.0, &|_| true, 0.0);
        // Two triangles per wall, each crossing the height once.
        assert_eq!(edges.len(), 8);
        for edge in &edges {
            assert!(edge.ends.iter().all(|p| (p[2] - 4.0).abs() < 1e-12));
            assert!(edge.normal[2].abs() < 1e-12, "a wall is vertical");
        }
        // Outside the box, at its top's height (a level face), or kept out: nothing.
        assert_eq!(contour(&mesh, 12.0, &|_| true, 0.0).len(), 0);
        assert_eq!(contour(&mesh, 4.0, &|_| false, 0.0).len(), 0);
        // Walls facing a little up are left out when only faces turned down are wanted.
        assert_eq!(contour(&mesh, 4.0, &|_| true, -0.5).len(), 0);
    }

    #[test]
    fn the_point_nearest_the_pointer_is_on_the_front_of_the_model() {
        let mesh = cube([0.0; 3], [10.0, 10.0, 10.0]);
        let edges = contour(&mesh, 4.0, &|_| true, 0.0);
        // Seen from the front (-Y), level with the height, the pointer at X 3: both the front
        // wall and the back one lie under it; the front one wins.
        let ray = Ray {
            origin: [3.0, -50.0, 4.0],
            direction: [0.0, 1.0, 0.0],
        };
        let (point, normal) = nearest(&edges, &ray, None).unwrap();
        assert!(
            (point[0] - 3.0).abs() < 1e-9 && point[1].abs() < 1e-9,
            "{point:?}"
        );
        assert!((normal[1] + 1.0).abs() < 1e-12);
        // From above the height, aimed beside the box on the right: the nearest point is on
        // the right wall, at the height held.
        let ray = Ray {
            origin: [14.0, -50.0, 20.0],
            direction: {
                let d = [0.0, 50.0, -16.0];
                let l = dot(d, d).sqrt();
                d.map(|c| c / l)
            },
        };
        let (point, _) = nearest(&edges, &ray, None).unwrap();
        assert!(
            (point[0] - 10.0).abs() < 1e-9 && (point[2] - 4.0).abs() < 1e-12,
            "{point:?}"
        );
        // On the model, the point nearest the surface under the pointer: on the front wall
        // higher up, the front wall's point below it.
        let (point, _) = nearest(&edges, &ray, Some([6.0, 0.0, 7.0])).unwrap();
        assert!(
            (point[0] - 6.0).abs() < 1e-9 && point[1].abs() < 1e-9,
            "{point:?}"
        );
        // Behind the eye, nothing.
        let away = Ray {
            origin: [3.0, -50.0, 4.0],
            direction: [0.0, -1.0, 0.0],
        };
        assert!(nearest(&edges, &away, None).is_none());
    }
}
