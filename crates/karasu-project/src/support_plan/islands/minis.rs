//! The mini pass (TD-167): after the standard supports, mini supports where none holds, and
//! mini supports standing on one another.
//!
//! **Standard supports first.** The Islands and Automatic tools grow every support they can as a
//! standard support first, with mini supports off; only then does this pass place mini supports,
//! at the islands still not held and the overhang candidates still uncovered (the callers
//! choose them, the Automatic tool with its mini density).
//!
//! **A mini support at a tip** ([`Minis::place`]) stands on the model below it, its foot found
//! in a fan of directions with its rod clear of the model (TD-126), no longer than the maximum
//! length and rising at least the minimum height; with none, it joins the rod of a mini support
//! placed before it, as a branch ([`junction_on`] with the ball along the surface's mean normal,
//! TD-164), within the same limits. With neither, the tip is refused: as too short when a mini
//! support would rise less than the minimum height, as too long when it would stand beyond the
//! maximum length.
//!
//! **Branches** ([`Minis::branch`]): then, longest rod first (ties in the plan's order), a mini
//! support of the pass that carries no branch becomes a branch of another one's rod when its
//! own rod gets shorter. A trunk carries at most the values' branches per mini support, and a
//! branch none: one level.
//!
//! Everything follows the plan's order: the pass is deterministic.
use super::{
    check, mini_built, start_tips, Growing, Island, Outcome, Planned, Skip, Tip, ATTEMPTS,
};
use crate::support_plan::{
    branches::{junction_on, Junction},
    route::{MiniLimits, Routed},
};
use karasu_core::{
    geometry::plan_length,
    mesh::TriangleMesh,
    supports::{AxisSpec, BaseSpec, ManualSupport, SupportKind, SupportParameters, SupportScene},
};
use std::sync::atomic::{AtomicBool, Ordering};

/// A mini support the pass placed on the model: its place among the plan's supports, its build
/// alone (support 1), its rod's length and the branches it carries; `None` once it is a branch.
struct Standing {
    place: usize,
    scene: SupportScene,
    rod_mm: f64,
    carries: u32,
}

/// The mini pass over a plan (see the module).
pub(crate) struct Minis<'a> {
    growing: &'a Growing<'a>,
    parameters: SupportParameters,
    /// The mini supports the pass placed on the model, in order; `None` once a branch.
    standing: Vec<Option<Standing>>,
}

impl<'a> Minis<'a> {
    /// A pass growing with `growing`'s values: its preset, its mini supports' branches and
    /// maximum length.
    pub(crate) fn new(growing: &'a Growing<'a>) -> Self {
        Self {
            growing,
            parameters: growing.next.parameters(),
            standing: Vec::new(),
        }
    }

    /// Each of `islands` (their places in the search with them) that no support of `outcome`
    /// holds gets a mini support at the first of its places ([`start_tips`]) where one fits,
    /// [`ATTEMPTS`] at most, in their order. A place refused as too long leaves why in `why_at`.
    pub(crate) fn at_islands(
        &mut self,
        islands: &[(usize, Island)],
        mesh: &TriangleMesh,
        outcome: &mut Outcome,
        why_at: &mut Vec<([f64; 3], Skip)>,
        stop: &AtomicBool,
    ) {
        for (_, island) in islands {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            let tips: Vec<[f64; 3]> = outcome
                .supports
                .iter()
                .map(|p| p.routed.support.tip_mm)
                .collect();
            if check::held(island, &tips) {
                continue;
            }
            for place in start_tips(self.growing.surface, mesh, island)
                .into_iter()
                .take(ATTEMPTS)
            {
                match self.place(outcome, place) {
                    Ok(()) => break,
                    Err(Some(why)) => refused(why_at, place.0, why),
                    Err(None) => {}
                }
            }
        }
    }

    /// How long and how high a mini support may be.
    fn limits(&self) -> MiniLimits {
        self.growing.values.mini_limits()
    }

    /// Whether a mini support at `point` (outward `normal`) would stand on the model within
    /// `limits` (none: unlimited).
    fn stands(&self, (point, normal): ([f64; 3], [f64; 3]), limits: Option<MiniLimits>) -> bool {
        let g = self.growing;
        mini_built(
            g.surface,
            &self.parameters,
            g.document,
            g.volume,
            (point, normal),
            limits,
        )
        .is_some()
    }

    /// A mini support at `tip`, added to `outcome`: its own on the model, else a branch onto a
    /// mini support of the pass. `Err` with why when neither: [`Skip::MiniTooShort`] when one
    /// would stand on the model with no minimum height, [`Skip::MiniTooLong`] when one would with
    /// no maximum length, [`Skip::MiniFootHangs`] when one would with its foot's ball hanging off
    /// the model (TD-169), [`Skip::MiniTouches`] when one would touching the model beside its
    /// contacts (TD-170), else `None` (the standard support's refusal stands).
    pub(crate) fn place(&mut self, outcome: &mut Outcome, tip: Tip) -> Result<(), Option<Skip>> {
        let (point, normal, _) = tip;
        let g = self.growing;
        if let Some((mut routed, scene)) = mini_built(
            g.surface,
            &self.parameters,
            g.document,
            g.volume,
            (point, normal),
            Some(self.limits()),
        ) {
            routed.support.origin = g.origin;
            let rod_mm = rod_length(&scene);
            self.standing.push(Some(Standing {
                place: outcome.supports.len(),
                scene,
                rod_mm,
                carries: 0,
            }));
            outcome.supports.push(Planned {
                routed,
                joins: None,
            });
            return Ok(());
        }
        if let Some((k, z, _)) = self.junction(point, normal, None, f64::INFINITY) {
            self.join(outcome, None, k, (point, normal), z);
            return Ok(());
        }
        // Which limit kept it from standing, if one did.
        let limits = self.limits();
        let lower = MiniLimits {
            min_height_mm: 0.0,
            ..limits
        };
        let longer = MiniLimits {
            max_length_mm: f64::INFINITY,
            ..limits
        };
        let hanging = MiniLimits {
            foot_rests: false,
            ..limits
        };
        let touching = MiniLimits {
            keep_off: false,
            ..limits
        };
        if self.stands((point, normal), Some(lower)) {
            Err(Some(Skip::MiniTooShort))
        } else if self.stands((point, normal), Some(longer)) {
            Err(Some(Skip::MiniTooLong))
        } else if self.stands((point, normal), Some(hanging)) {
            Err(Some(Skip::MiniFootHangs))
        } else if self.stands((point, normal), Some(touching)) {
            Err(Some(Skip::MiniTouches))
        } else {
            Err(None)
        }
    }

    /// The mini supports of the pass that carry no branch become branches where their rods get
    /// shorter: longest rod first, ties in the plan's order (see the module).
    pub(crate) fn branch(&mut self, outcome: &mut Outcome) {
        if self.growing.values.mini_branches > 0 {
            self.branch_standing(outcome);
        }
        outcome.branches = outcome
            .supports
            .iter()
            .filter(|p| p.joins.is_some())
            .count();
    }

    fn branch_standing(&mut self, outcome: &mut Outcome) {
        let mut order: Vec<usize> = (0..self.standing.len()).collect();
        let rod = |k: usize| self.standing[k].as_ref().map_or(0.0, |s| s.rod_mm);
        order.sort_by(|&a, &b| rod(b).total_cmp(&rod(a)).then(a.cmp(&b)));
        for k in order {
            let Some(own) = self.standing[k].as_ref() else {
                continue;
            };
            if own.carries > 0 {
                continue;
            }
            let (place, rod_mm) = (own.place, own.rod_mm);
            let Some(&(tip, normal)) = outcome.supports[place].routed.anchors.first() else {
                continue;
            };
            if let Some((trunk, z, _)) = self.junction(tip, normal, Some(k), rod_mm) {
                self.join(outcome, Some(k), trunk, (tip, normal), z);
            }
        }
    }

    /// The mini support of the pass whose rod the branch from `tip` (outward `normal`) joins
    /// with the shortest rod, shorter than `shorter_than`: its place in `standing`, the
    /// junction's height and the rod's length; never `except`, nor one that is a branch or
    /// carries as many branches as the values allow.
    fn junction(
        &self,
        tip: [f64; 3],
        normal: [f64; 3],
        except: Option<usize>,
        shorter_than: f64,
    ) -> Option<(usize, f64, f64)> {
        let g = self.growing;
        let most = g.values.mini_branches;
        let limits = MiniLimits {
            max_length_mm: self.limits().max_length_mm.min(shorter_than),
            ..self.limits()
        };
        let longest = limits.max_length_mm;
        let r = self.parameters.mini_diameter_mm / 2.0;
        let axis = g.surface.normal_at(tip, normal, r);
        let length = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        let along = length(axis);
        if along.is_nan() || along <= 0.0 {
            return None;
        }
        let out = (r - self.parameters.mini_penetration_mm) / along;
        let top = [0, 1, 2].map(|c| tip[c] + axis[c] * out);
        let mut best: Option<(usize, f64, f64)> = None;
        for (k, s) in self.standing.iter().enumerate() {
            let Some(s) = s.as_ref() else {
                continue;
            };
            if Some(k) == except || s.carries >= most {
                continue;
            }
            // Within reach in plan: its rod's nearest end no farther than the longest rod.
            let Some(path) = s.scene.shaft_path(1) else {
                continue;
            };
            let near = path
                .iter()
                .map(|p| plan_length(p[0] - top[0], p[1] - top[1]))
                .fold(f64::INFINITY, f64::min);
            let span = path
                .windows(2)
                .map(|w| plan_length(w[1][0] - w[0][0], w[1][1] - w[0][1]))
                .sum::<f64>();
            if near - span > longest {
                continue;
            }
            let Some(at) = junction_on(&Junction {
                scene: &s.scene,
                parent: 1,
                trunk_mm: self.parameters.mini_diameter_mm,
                tip,
                normal,
                parameters: &self.parameters,
                kind: SupportKind::Mini,
                document: g.document,
                surface: Some(g.surface),
                keep_normal: true,
                limits: Some(limits),
            }) else {
                continue;
            };
            let rod = length([top[0] - at[0], top[1] - at[1], top[2] - at[2]]);
            if rod < shorter_than && best.is_none_or(|b| rod < b.2) {
                best = Some((k, at[2], rod));
            }
        }
        best
    }

    /// `tip` (with its outward `normal`) as a branch of `standing[trunk]` at height `z`: the
    /// support at `branch`'s place when one of the pass becomes a branch, else a new one.
    fn join(
        &mut self,
        outcome: &mut Outcome,
        branch: Option<usize>,
        trunk: usize,
        (tip, normal): ([f64; 3], [f64; 3]),
        z: f64,
    ) {
        let g = self.growing;
        let r = self.parameters.mini_diameter_mm / 2.0;
        let Some(parent) = self.standing[trunk].as_mut() else {
            return;
        };
        parent.carries += 1;
        let trunk_place = parent.place;
        let planned = Planned {
            routed: Routed {
                support: ManualSupport {
                    id: 1,
                    tip_mm: tip,
                    tip_axis: AxisSpec::Normal,
                    bends: Vec::new(),
                    // The branch's base is its trunk's ([`super::add_planned`]).
                    base: BaseSpec::Vertical,
                    parameters: self.parameters.clone(),
                    accept_collision: false,
                    origin: g.origin,
                    kind: SupportKind::Mini,
                },
                anchors: vec![(tip, g.surface.normal_at(tip, normal, r))],
            },
            joins: Some((trunk_place, z)),
        };
        match branch {
            Some(k) => {
                if let Some(own) = self.standing[k].take() {
                    outcome.supports[own.place] = planned;
                }
            }
            None => outcome.supports.push(planned),
        }
    }
}

/// `why` the mini pass refused a mini support at `point`, in `why_at`: it replaces what was said
/// there before (the standard support's refusal), as the last thing tried.
pub(crate) fn refused(why_at: &mut Vec<([f64; 3], Skip)>, point: [f64; 3], why: Skip) {
    // The very point the standard support was refused at, given again: compared exactly.
    #[allow(clippy::float_cmp)]
    why_at.retain(|w| w.0 != point);
    why_at.push((point, why));
}

/// The length of support 1's rod in `scene`, from its tip's ball to its foot's, in mm.
fn rod_length(scene: &SupportScene) -> f64 {
    scene.shaft_path(1).map_or(0.0, |path| {
        path.windows(2)
            .map(|w| {
                let d = [w[1][0] - w[0][0], w[1][1] - w[0][1], w[1][2] - w[0][2]];
                (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
            })
            .sum()
    })
}

#[cfg(test)]
mod tests {
    use super::super::{add_planned, plan, Island, IslandSupports, Outcome, Skip};
    use crate::{
        placement::Affine,
        support_plan::{
            route::{RouteSurface, SurfaceMesh},
            values::{NextSupport, Weight},
        },
    };
    use karasu_core::{
        cancellation::Cancellation,
        geometry::Point3,
        mesh::TriangleMesh,
        supports::{
            build_supports, BaseSpec, BuildVolume, Preset, SupportDocument, SupportKind,
            SupportOptions, SupportSurface, SurfaceGrid,
        },
    };
    use std::sync::atomic::AtomicBool;

    /// Closed boxes, each from its lowest corner to its highest, facing out.
    fn boxes(list: &[([f64; 3], [f64; 3])]) -> TriangleMesh {
        let (mut points, mut triangles) = (Vec::new(), Vec::new());
        for &(min, max) in list {
            let base = u32::try_from(points.len()).unwrap();
            for i in 0..8 {
                points.push(Point3::new(
                    if i & 1 == 0 { min[0] } else { max[0] },
                    if i & 2 == 0 { min[1] } else { max[1] },
                    if i & 4 == 0 { min[2] } else { max[2] },
                ));
            }
            for q in [
                [0, 2, 3, 1],
                [4, 5, 7, 6],
                [0, 1, 5, 4],
                [2, 6, 7, 3],
                [0, 4, 6, 2],
                [1, 3, 7, 5],
            ] {
                for t in [[q[0], q[1], q[2]], [q[0], q[2], q[3]]] {
                    triangles.push(t.map(|v| v + base));
                }
            }
        }
        TriangleMesh::try_new(points, triangles).unwrap()
    }

    /// The surface for routes of the boxes `list`.
    fn surface(list: &[([f64; 3], [f64; 3])]) -> RouteSurface {
        surface_of(boxes(list))
    }

    /// The surface for routes of `mesh`.
    fn surface_of(mesh: TriangleMesh) -> RouteSurface {
        let grid = SurfaceGrid::build(&mesh, &mut Cancellation::never()).unwrap();
        RouteSurface::new(SurfaceMesh::Placed(mesh), grid)
    }

    /// A slab from Z 6 to 8 over X and Y 0 to 12, over a row of ridges along Y from the plate to
    /// Z 3.2, side by side from X -2 to 14: each rises at `steep`° from level facing -X and up,
    /// and drops straight down on its +X side. Every surface under the slab facing up is that
    /// steep.
    fn slab_over_ridges(steep: f64) -> TriangleMesh {
        let (top, low) = (3.2, 0.0);
        let width = (top - low) / steep.to_radians().tan();
        let mut points = Vec::new();
        let mut triangles: Vec<[u32; 3]> = Vec::new();
        let mut prism = |section: &[[f64; 2]]| {
            let first = u32::try_from(points.len()).unwrap();
            let sides = u32::try_from(section.len()).unwrap();
            for y in [-5.0, 17.0] {
                for &[x, z] in section {
                    points.push(Point3::new(x, y, z));
                }
            }
            let mut faces = Vec::new();
            for k in 1..sides - 1 {
                faces.push([first, first + k, first + k + 1]);
                faces.push([first + sides, first + sides + k + 1, first + sides + k]);
            }
            for k in 0..sides {
                let next = (k + 1) % sides;
                faces.push([first + k, first + sides + k, first + sides + next]);
                faces.push([first + k, first + sides + next, first + next]);
            }
            // Each face out of the prism: away from its middle.
            let corners = f64::from(2 * sides);
            let middle: Vec<f64> = (0..3)
                .map(|axis| {
                    points[first as usize..]
                        .iter()
                        .map(|p| [p.x, p.y, p.z][axis])
                        .sum::<f64>()
                        / corners
                })
                .collect();
            let at = |v: u32| {
                let p = points[v as usize];
                [p.x, p.y, p.z]
            };
            for mut face in faces {
                let [one, two, three] = face.map(at);
                let along = [0, 1, 2].map(|axis| two[axis] - one[axis]);
                let across = [0, 1, 2].map(|axis| three[axis] - one[axis]);
                let outward = [
                    along[1] * across[2] - along[2] * across[1],
                    along[2] * across[0] - along[0] * across[2],
                    along[0] * across[1] - along[1] * across[0],
                ];
                let away: f64 = (0..3)
                    .map(|axis| outward[axis] * (one[axis] - middle[axis]))
                    .sum();
                if away < 0.0 {
                    face.swap(1, 2);
                }
                triangles.push(face);
            }
        };
        prism(&[[0.0, 6.0], [12.0, 6.0], [12.0, 8.0], [0.0, 8.0]]);
        let mut x = -2.0;
        while x < 14.0 {
            prism(&[[x, low], [x + width, low], [x + width, top]]);
            x += width;
        }
        TriangleMesh::try_new(points, triangles).unwrap()
    }

    fn volume() -> BuildVolume {
        BuildVolume {
            origin_xy_mm: [-50.0, -50.0],
            size_mm: [150.0, 150.0, 100.0],
        }
    }

    /// An island starting at the underside `z` of a slab spanning `min` to `max` in plan, up to
    /// `top`.
    fn island(min: [f64; 2], max: [f64; 2], z: f64, top: f64) -> Island {
        Island {
            point: [
                f64::midpoint(min[0], max[0]),
                f64::midpoint(min[1], max[1]),
                z,
            ],
            layers: [100, 120],
            heights: [z - 0.05, top],
            area_mm2: (max[0] - min[0]) * (max[1] - min[1]),
            footprint: [min, max],
            inside: false,
        }
    }

    /// The Islands tool's plan of `islands` on the boxes `list`, Light, with `values`, no final
    /// check.
    fn planned(
        list: &[([f64; 3], [f64; 3])],
        islands: &[Island],
        values: &IslandSupports,
    ) -> Outcome {
        planned_on(&surface(list), islands, values)
    }

    /// The Islands tool's plan of `islands` on `surface`, Light, with `values`, no final check.
    fn planned_on(surface: &RouteSurface, islands: &[Island], values: &IslandSupports) -> Outcome {
        let numbered: Vec<(usize, Island)> = islands.iter().copied().enumerate().collect();
        plan(
            &numbered,
            surface,
            &NextSupport::of(Weight::Light),
            None,
            values,
            volume(),
            None,
            &AtomicBool::new(false),
        )
    }

    fn values() -> IslandSupports {
        IslandSupports {
            weight: Weight::Light,
            ..IslandSupports::default()
        }
    }

    fn kinds(outcome: &Outcome) -> Vec<SupportKind> {
        outcome
            .supports
            .iter()
            .map(|p| p.routed.support.kind)
            .collect()
    }

    #[test]
    fn an_island_takes_a_standard_support_at_another_place_before_a_mini_one() {
        // A slab whose underside starts at Z 6, over a block from Z 2 to 3.2 under its first
        // 6.5 mm along X. Where the island starts (X 6), a Light support has no room above the
        // block and no way past it; 2 mm along, beyond the block, it goes straight down.
        let mesh = [
            ([0.0, 0.0, 6.0], [12.0, 12.0, 8.0]),
            ([0.0, 0.0, 2.0], [6.5, 12.0, 3.2]),
        ];
        let start = island([0.0, 0.0], [12.0, 12.0], 6.0, 8.0);
        // Mini supports on: still the standard support, as with them off (TD-167).
        for minis in [true, false] {
            let made = planned(&mesh, &[start], &IslandSupports { minis, ..values() });
            assert_eq!(kinds(&made), [SupportKind::Standard], "minis {minis}");
            assert_eq!(made.held, 1);
            // Planned without the model, its final check did not run, and it says so (TD-171).
            assert_eq!(made.unchecked.as_deref(), Some(super::super::NO_MODEL));
            let tip = made.supports[0].routed.support.tip_mm;
            assert!(tip[0] > 7.9, "{tip:?}");
        }
    }

    #[test]
    fn a_mini_support_out_of_its_limits_is_not_placed_and_says_why() {
        // A slab over a block reaching Z 3.2 everywhere below it: no standard support fits, and
        // a mini support's rod runs 2.6 mm from its ball (Z 5.9) to its foot's (Z 3.3).
        let mesh = [
            ([0.0, 0.0, 6.0], [12.0, 12.0, 8.0]),
            ([-5.0, -5.0, 0.0], [17.0, 17.0, 3.2]),
        ];
        let start = island([0.0, 0.0], [12.0, 12.0], 6.0, 8.0);
        let made = planned(&mesh, &[start], &values());
        assert_eq!(kinds(&made), [SupportKind::Mini]);
        let BaseSpec::Model { point_mm, .. } = made.supports[0].routed.support.base else {
            panic!("{:?}", made.supports[0].routed.support.base);
        };
        assert!((point_mm[2] - 3.2).abs() < 1e-9, "{point_mm:?}");
        let short = IslandSupports {
            mini_max_length_mm: 2.0,
            ..values()
        };
        let made = planned(&mesh, &[start], &short);
        assert!(made.supports.is_empty());
        assert_eq!(made.skipped, [(0, Skip::MiniTooLong)]);
        // Rising less than a minimum height (owner, 2026-11-20): 2.6 mm against 3 mm.
        let high = IslandSupports {
            mini_min_height_mm: 3.0,
            ..values()
        };
        let made = planned(&mesh, &[start], &high);
        assert!(made.supports.is_empty());
        assert_eq!(made.skipped, [(0, Skip::MiniTooShort)]);
    }

    #[test]
    fn a_mini_support_whose_foot_would_hang_is_not_placed_and_says_why() {
        // TD-169: a slab 2.8 mm over ridges whose faces rise at 80°: no standard support fits,
        // and a mini support could stand only on a ridge's face, where its foot's ball would
        // hang off it and start in mid-air. Its feet rest on the model, so none is placed; the
        // reason says a mini support would stand there with its foot hanging.
        let start = island([0.0, 0.0], [12.0, 12.0], 6.0, 8.0);
        let made = planned_on(&surface_of(slab_over_ridges(80.0)), &[start], &values());
        assert!(made.supports.is_empty(), "{:?}", kinds(&made));
        assert_eq!(made.skipped, [(0, Skip::MiniFootHangs)]);
    }

    #[test]
    fn a_mini_support_that_would_touch_the_model_is_not_placed_and_says_why() {
        // TD-170: a slab 0.05 mm over a block, but for a slot 0.36 mm wide under its middle,
        // down to Z 3.2. No standard support fits; a Light mini support (0.3 mm) fits down the
        // slot only 0.03 mm from its walls, closer than the gap. It is not placed, and the
        // reason says a mini support would touch the model there.
        let mesh = [
            ([0.0, 0.0, 6.0], [12.0, 12.0, 8.0]),
            ([-5.0, -5.0, 0.0], [5.82, 17.0, 5.95]),
            ([6.18, -5.0, 0.0], [17.0, 17.0, 5.95]),
            ([5.82, -5.0, 0.0], [6.18, 5.82, 5.95]),
            ([5.82, 6.18, 0.0], [6.18, 17.0, 5.95]),
            ([5.82, 5.82, 0.0], [6.18, 6.18, 3.2]),
        ];
        let start = island([5.82, 5.82], [6.18, 6.18], 6.0, 8.0);
        let made = planned(&mesh, &[start], &values());
        assert!(made.supports.is_empty(), "{:?}", kinds(&made));
        assert_eq!(made.skipped, [(0, Skip::MiniTouches)]);
    }

    #[test]
    fn a_mini_support_stands_on_another_where_its_rod_gets_shorter() {
        // Two slabs over a floor at Z 2 that leaves no way to the plate, feet on the model off:
        // only mini supports. The higher slab's, 5 mm beside the lower one's, reaches the lower
        // one's rod within 45° with a rod shorter than its own to the floor.
        let mesh = [
            ([-10.0, -10.0, 0.0], [40.0, 30.0, 2.0]),
            ([4.0, 4.0, 8.0], [8.0, 8.0, 9.0]),
            ([9.0, 4.0, 10.0], [13.0, 8.0, 11.0]),
        ];
        let islands = [
            island([4.0, 4.0], [8.0, 8.0], 8.0, 9.0),
            island([9.0, 4.0], [13.0, 8.0], 10.0, 11.0),
        ];
        let branching = IslandSupports {
            foot_on_model: false,
            ..values()
        };
        let made = planned(&mesh, &islands, &branching);
        assert_eq!(kinds(&made), [SupportKind::Mini, SupportKind::Mini]);
        assert_eq!(made.held, 2);
        assert_eq!(made.branches, 1);
        assert!(made.supports[0].joins.is_none());
        let (trunk, z) = made.supports[1].joins.expect("a branch");
        assert_eq!(trunk, 0);
        // Within 45° from the branch's ball, 5 mm aside, and above the trunk's foot.
        assert!(z < 9.9 - 5.0 && z > 2.0, "{z}");
        // Built against the model, as a slice builds them, neither has an error.
        let mut document = SupportDocument::new(Preset::Light);
        let identity = Affine {
            linear: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            translation: [0.0; 3],
        };
        let ids = add_planned(&mut document, &identity, &made);
        assert_eq!(
            document.supports[1].base,
            BaseSpec::Support {
                parent: ids[0],
                z_mm: z
            }
        );
        let scene = build_supports(
            &document,
            SupportSurface {
                mesh: &boxes(&mesh),
                interior_from: None,
            },
            &[],
            volume(),
            SupportOptions::default(),
            &mut Cancellation::never(),
        )
        .unwrap();
        assert!(!scene.has_errors(), "{:?}", scene.findings());
        // No branches asked: each stands on the floor.
        let alone = IslandSupports {
            mini_branches: 0,
            ..branching
        };
        let made = planned(&mesh, &islands, &alone);
        assert_eq!(made.branches, 0);
        assert!(made.supports.iter().all(|p| p.joins.is_none()));
    }
}
