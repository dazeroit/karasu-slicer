//! New supports placed clear of the model (owner, 2026-10-29): where a shaft straight down
//! would go through more of the model below its tip, the support a click adds goes aside (its
//! shaft in the nearest clear column, the tip leaning to a knee) or, failing that, stands on
//! the model below (see `karasu_core::supports::find_route`). The panel's switch "Avoid the
//! model" (on by default) turns it off: the shaft then goes straight down, as before.
//!
//! The route is found against the selected model's surface (its hollowed shell, or the placed
//! model) and a grid of its triangles, built on a thread of its own once per preparation and
//! placement while supports are placed; the preview and the click then find routes at once.
use super::SupportsUi;
use crate::{preparation_ui::PreparationUi, workspace::Workspace};
use karasu_core::{
    cancellation::Cancellation,
    geometry::ObjectTransform,
    mesh::TriangleMesh,
    supports::{
        find_mini_foot, find_route, AxisSpec, BaseSpec, ManualSupport, MiniQuery, Route,
        RouteQuery, SupportKind, SupportParameters, SurfaceGrid,
    },
};
use karasu_project::pipeline::{placed_mesh, Preparation};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::Instant,
};

/// The surface routes are found against.
enum SurfaceMesh {
    /// The placed model, solid.
    Placed(TriangleMesh),
    /// The hollowed shell of object `index` in a preparation.
    Hollow(Arc<Preparation>, usize),
}

impl SurfaceMesh {
    /// Where the hollowed shell's cavity triangles start, for a hollowed model.
    fn interior_from(&self) -> Option<usize> {
        match self {
            Self::Placed(_) => None,
            Self::Hollow(preparation, index) => Some(
                preparation
                    .objects
                    .get(*index)?
                    .as_ref()
                    .ok()?
                    .hollowed()?
                    .exterior_faces(),
            ),
        }
    }

    fn mesh(&self) -> Option<&TriangleMesh> {
        match self {
            Self::Placed(mesh) => Some(mesh),
            Self::Hollow(preparation, index) => Some(
                preparation
                    .objects
                    .get(*index)?
                    .as_ref()
                    .ok()?
                    .hollowed()?
                    .mesh(),
            ),
        }
    }
}

/// What a surface is of: the object, its placement and the preparation's generation.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Key {
    object: usize,
    placement: ObjectTransform,
    generation: u64,
}

/// The selected model's surface and its grid, shared with the threads that find routes on it.
pub(crate) struct RouteSurface {
    key: Key,
    mesh: SurfaceMesh,
    grid: SurfaceGrid,
}

impl RouteSurface {
    /// The surface: the hollowed shell, or the placed model, in world millimetres.
    pub(crate) fn mesh(&self) -> Option<&TriangleMesh> {
        self.mesh.mesh().filter(|m| self.grid.fits(m))
    }

    /// Its grid.
    pub(crate) fn grid(&self) -> &SurfaceGrid {
        &self.grid
    }

    /// The surface's outward normal at `point` as a support's build measures it (TD-124): the
    /// mean within `radius` of the triangles facing the `facet` normal's way; `facet` itself
    /// without the surface. Planned supports are built alone against a triangle facing this
    /// way, so they take the shape the whole surface gives them.
    pub(crate) fn normal_at(&self, point: [f64; 3], facet: [f64; 3], radius: f64) -> [f64; 3] {
        self.mesh()
            .map_or(facet, |m| self.grid.surface_normal(m, point, facet, radius))
    }

    /// Whether triangle `t` of the surface is one of a hollowed shell's cavity triangles.
    pub(crate) fn is_cavity(&self, t: u32) -> bool {
        self.mesh
            .interior_from()
            .is_some_and(|first| t as usize >= first)
    }
}

/// A surface being gridded on its own thread.
pub(super) struct RouteJob {
    key: Key,
    stop: Arc<AtomicBool>,
    receiver: mpsc::Receiver<Option<RouteSurface>>,
}

impl Drop for RouteJob {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// A support in world coordinates, with the surfaces it stands on: its tip's, and its foot's
/// when it stands on the model (point and outward normal), for its preview.
pub(crate) struct Routed {
    pub(crate) support: ManualSupport,
    pub(crate) anchors: Vec<([f64; 3], [f64; 3])>,
}

/// The support of `parameters` with its tip at `tip` (outward `normal`) on `surface`, in world
/// coordinates, routed clear of the model (see [`find_route`]): straight down, aside with a
/// knee, or standing on the model below; with the way found. `document` gives the clearance,
/// the floor and the axis limit, its defaults without one.
pub(crate) fn route_support(
    surface: &RouteSurface,
    parameters: &SupportParameters,
    axis: AxisSpec,
    document: Option<&karasu_core::supports::SupportDocument>,
    tip: [f64; 3],
    normal: [f64; 3],
) -> (Routed, Route) {
    let mut routed = Routed {
        support: ManualSupport {
            id: 1,
            tip_mm: tip,
            tip_axis: axis,
            bends: Vec::new(),
            base: BaseSpec::Vertical,
            parameters: parameters.clone(),
            accept_collision: false,
            origin: karasu_core::supports::SupportOrigin::Placed,
            kind: karasu_core::supports::SupportKind::Standard,
        },
        anchors: vec![(
            tip,
            surface.normal_at(tip, normal, parameters.footprint_radius_mm()),
        )],
    };
    let Some(mesh) = surface.mesh() else {
        return (routed, Route::None);
    };
    let query = RouteQuery {
        tip_mm: tip,
        normal,
        tip: karasu_core::supports::TipCone::of(
            parameters,
            document.map_or(karasu_core::supports::DEFAULT_ANCHOR_TOLERANCE_MM, |d| {
                d.anchor_tolerance_mm
            }),
        ),
        radius_mm: widest(parameters) + document.map_or(0.3, |d| d.clearance_mm),
        floor_mm: document
            .and_then(|d| d.raft)
            .map_or(0.0, |r| r.thickness_mm),
        foot_length_mm: parameters.foot_length_mm,
        foot_radius_mm: parameters.foot_contact_diameter_mm / 2.0,
        max_axis_deviation_deg: document.map_or(60.0, |d| d.max_axis_deviation_deg),
        towards_shaft: axis == AxisSpec::Straight,
    };
    let route = find_route(mesh, &surface.grid, &query);
    match route {
        Route::Straight | Route::None => {}
        Route::Tilted(axis) => routed.support.tip_axis = AxisSpec::Direction(axis),
        Route::Aside(knee) => {
            routed.support.bends = vec![knee];
            routed.support.tip_axis = AxisSpec::Straight;
        }
        Route::OnModel {
            point_mm,
            normal,
            axis,
        } => {
            // Towards its shaft, as the route checked it (TD-132).
            routed.support.base = BaseSpec::Model {
                point_mm,
                axis: AxisSpec::Direction(axis),
            };
            let radius = parameters.foot_contact_diameter_mm / 2.0;
            routed
                .anchors
                .push((point_mm, surface.normal_at(point_mm, normal, radius)));
        }
    }
    (routed, route)
}

/// The mini support of `parameters` with its tip at `tip` (outward `normal`) on `surface`, in
/// world coordinates, standing on the model (see [`find_mini_foot`]): straight below its tip,
/// or, when `clear` (the Islands and Automatic tools, TD-126), where its rod keeps off the model.
/// `None` when it finds nowhere to stand. `document` gives the floor, the plate without one.
pub(crate) fn mini_routed(
    surface: &RouteSurface,
    parameters: &SupportParameters,
    document: Option<&karasu_core::supports::SupportDocument>,
    tip: [f64; 3],
    normal: [f64; 3],
    clear: bool,
) -> Option<Routed> {
    let mesh = surface.mesh()?;
    let query = MiniQuery {
        tip_mm: tip,
        normal,
        diameter_mm: parameters.mini_diameter_mm,
        penetration_mm: parameters.mini_penetration_mm,
        floor_mm: document
            .and_then(|d| d.raft)
            .map_or(0.0, |r| r.thickness_mm),
        clear,
    };
    let (point_mm, foot_normal) = find_mini_foot(mesh, &surface.grid, &query)?;
    Some(Routed {
        support: ManualSupport {
            id: 1,
            tip_mm: tip,
            tip_axis: AxisSpec::Normal,
            bends: Vec::new(),
            base: BaseSpec::Model {
                point_mm,
                axis: AxisSpec::Normal,
            },
            parameters: parameters.clone(),
            accept_collision: false,
            origin: karasu_core::supports::SupportOrigin::Placed,
            kind: SupportKind::Mini,
        },
        anchors: {
            let radius = parameters.mini_diameter_mm / 2.0;
            vec![
                (tip, surface.normal_at(tip, normal, radius)),
                (point_mm, surface.normal_at(point_mm, foot_normal, radius)),
            ]
        },
    })
}

/// `axis`, a tip's or a foot's in world coordinates, in the model frame `to_model` maps to: a
/// direction turns with the model (a tilted tip, TD-124); the others are the model's own.
pub(crate) fn axis_to_model(axis: AxisSpec, to_model: &crate::gizmo::Affine) -> AxisSpec {
    match axis {
        AxisSpec::Direction(d) => AxisSpec::Direction(to_model.vector(d)),
        other => other,
    }
}

/// The widest radius of a support's parts along its path, in mm.
pub(super) fn widest(p: &SupportParameters) -> f64 {
    p.shaft_diameter_mm
        .max(p.tip_body_diameter_mm)
        .max(p.shaft_bottom_diameter_mm.unwrap_or(0.0))
        / 2.0
}

impl SupportsUi {
    /// What object `index`'s surface for routes is of now, if it is prepared.
    fn route_key(
        workspace: &Workspace,
        preparation: &PreparationUi,
        index: usize,
    ) -> Option<(Key, Arc<Preparation>)> {
        let placement = workspace.placement_of(index)?.transform;
        let (shared, generation) = preparation.shared(workspace, index)?;
        Some((
            Key {
                object: index,
                placement,
                generation,
            },
            shared,
        ))
    }

    /// Keep the selected model's surface for routes while supports are placed on it: take a
    /// finished grid, and start one when the model or its preparation changed. Returns whether
    /// a repaint is needed soon.
    pub(super) fn poll_route(
        &mut self,
        workspace: &Workspace,
        preparation: &PreparationUi,
        selected: Option<usize>,
    ) -> bool {
        if let Some(job) = &self.route_job {
            match job.receiver.try_recv() {
                Ok(surface) => {
                    self.route = surface.map(Arc::new);
                    self.route_job = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => self.route_job = None,
            }
        }
        let wanted = selected.filter(|_| {
            (self.placing && (!self.straight_only || self.on_cavity))
                || self.route_wanted
                || self.merging.is_some()
                || self.joining.is_some()
        });
        let Some((key, shared)) =
            wanted.and_then(|index| Self::route_key(workspace, preparation, index))
        else {
            return self.route_job.is_some();
        };
        if self.route.as_ref().is_some_and(|r| r.key == key)
            || self.route_job.as_ref().is_some_and(|j| j.key == key)
        {
            return self.route_job.is_some();
        }
        let hollow = shared
            .objects
            .get(key.object)
            .and_then(|o| o.as_ref().ok())
            .is_some_and(|o| o.hollowed().is_some());
        let source = workspace
            .plate()
            .objects
            .get(key.object)
            .and_then(|o| workspace.meshes().get(&o.model))
            .map(|m| Arc::clone(&m.mesh));
        let Some(source) = source else {
            return self.route_job.is_some();
        };
        let stop = Arc::new(AtomicBool::new(false));
        let cancelled = Arc::clone(&stop);
        let (sender, receiver) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("support-routes".to_owned())
            .spawn(move || {
                let started = Instant::now();
                let mesh = if hollow {
                    SurfaceMesh::Hollow(shared, key.object)
                } else {
                    match placed_mesh(&source, &key.placement) {
                        Ok(mesh) => SurfaceMesh::Placed(mesh),
                        Err(error) => {
                            log::warn!(target: "supports", "no surface for routes: {error}");
                            let _ = sender.send(None);
                            return;
                        }
                    }
                };
                let mut stop = |_| cancelled.load(Ordering::Relaxed);
                let grid = mesh
                    .mesh()
                    .and_then(|m| SurfaceGrid::build(m, &mut Cancellation::new(&mut stop)).ok());
                if let Some(m) = mesh.mesh() {
                    log::debug!(
                        target: "supports",
                        "gridded {} triangles for routes in {:.0} ms",
                        m.triangles().len(),
                        started.elapsed().as_secs_f64() * 1000.0
                    );
                }
                let _ = sender.send(grid.map(|grid| RouteSurface { key, mesh, grid }));
            });
        self.route_job = spawned.ok().map(|_| RouteJob {
            key,
            stop,
            receiver,
        });
        true
    }

    /// The support the next values make with its tip at `tip` (outward `normal`) on object
    /// `index`, in world coordinates: routed clear of the model when it can be (the switch on,
    /// the surface gridded), otherwise straight down. A mini support stands on the model
    /// straight below its tip (see [`find_mini_foot`]); `None` when it finds nowhere to stand,
    /// or before the surface is gridded.
    pub(super) fn routed(
        &self,
        workspace: &Workspace,
        index: usize,
        tip: [f64; 3],
        normal: [f64; 3],
    ) -> Option<Routed> {
        let parameters = self.next.parameters();
        if self.mini {
            let surface = self.current_route(workspace, index)?;
            // By hand: the user places it, touching allowed (owner, 2026-11-04).
            return mini_routed(
                surface,
                &parameters,
                workspace.supports(index),
                tip,
                normal,
                false,
            );
        }
        // In the cavity a support always finds its way: straight down it would go through the
        // cavity's floor and the shell below, so its foot stands on the floor.
        let current = workspace.placement_of(index).map(|p| p.transform);
        let surface = self
            .route
            .as_ref()
            .filter(|r| r.key.object == index && Some(r.key.placement) == current)
            .filter(|_| !self.straight_only || self.on_cavity);
        Some(match surface {
            Some(surface) => {
                route_support(
                    surface,
                    &parameters,
                    self.next.axis(),
                    workspace.supports(index),
                    tip,
                    normal,
                )
                .0
            }
            None => Routed {
                support: ManualSupport {
                    id: 1,
                    tip_mm: tip,
                    tip_axis: self.next.axis(),
                    bends: Vec::new(),
                    base: BaseSpec::Vertical,
                    parameters,
                    accept_collision: false,
                    origin: karasu_core::supports::SupportOrigin::Placed,
                    kind: karasu_core::supports::SupportKind::Standard,
                },
                anchors: vec![(tip, normal)],
            },
        })
    }

    /// Model `index`'s surface for routes, when it was gridded for the model where it stands.
    pub(super) fn current_route(
        &self,
        workspace: &Workspace,
        index: usize,
    ) -> Option<&RouteSurface> {
        let current = workspace.placement_of(index).map(|p| p.transform);
        self.route
            .as_deref()
            .filter(|r| r.key.object == index && Some(r.key.placement) == current)
    }

    /// The selected model `index`'s surface for routes, when it is ready for the model as it
    /// stands: the islands' supports are planned on it on a thread of their own.
    pub(crate) fn route_surface(
        &self,
        workspace: &Workspace,
        preparation: &PreparationUi,
        index: usize,
    ) -> Option<Arc<RouteSurface>> {
        let (key, _) = Self::route_key(workspace, preparation, index)?;
        self.route.as_ref().filter(|r| r.key == key).cloned()
    }
}

#[cfg(test)]
pub(in crate::supports_ui) mod tests {
    use super::*;
    use crate::supports_ui::height::TipAt;
    use crate::{
        hollowing_ui::tests::placed,
        workspace::{tests::box_stl, SurfaceHit},
    };
    use std::time::Duration;

    #[test]
    fn a_tilted_tip_turns_with_the_model_and_the_others_are_its_own() {
        // A model turned a quarter about Z: world +X is its -Y.
        let to_model = crate::gizmo::Affine {
            linear: [[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            translation: [3.0, 4.0, 0.0],
        };
        assert_eq!(
            axis_to_model(AxisSpec::Direction([1.0, 0.0, -1.0]), &to_model),
            AxisSpec::Direction([0.0, -1.0, -1.0])
        );
        for own in [AxisSpec::Normal, AxisSpec::Straight] {
            assert_eq!(axis_to_model(own, &to_model), own);
        }
    }

    /// A slab over a block under its left part, resting on the block: an overhang with more of
    /// the model below it.
    pub(in crate::supports_ui) fn ledge() -> (Workspace, PreparationUi, std::path::PathBuf) {
        // Room for a knee beside the block a little under 45° from the slab's bottom.
        ledge_at(8.5)
    }

    /// [`ledge`] with the slab's bottom at `slab_mm` (the block's top is at 4 mm).
    pub(in crate::supports_ui) fn ledge_at(
        slab_mm: f64,
    ) -> (Workspace, PreparationUi, std::path::PathBuf) {
        // A directory of its own per call: the tests using the ledge run in parallel and each
        // removes its directory when it ends (native Windows validation of 838563d: one test's
        // removal took the file another was opening).
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let directory = std::env::temp_dir().join(format!(
            "karasu-support-routes-{}-{slab_mm}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("ledge.stl");
        let block = box_stl([0.0, 0.0, 3.0], [6.0, 10.0, 4.0]);
        let inner = block
            .trim_start_matches("solid box\n")
            .trim_end_matches("endsolid box\n");
        let slab = box_stl([0.0, 0.0, slab_mm], [20.0, 10.0, slab_mm + 2.0]);
        let stl = slab.replace("endsolid box\n", &format!("{inner}endsolid box\n"));
        std::fs::write(&path, stl).unwrap();
        let (workspace, preparation) = placed(path, None, Duration::from_secs(60));
        (workspace, preparation, directory)
    }

    #[test]
    fn a_support_in_the_cavity_goes_from_its_ceiling_to_its_floor() {
        // A 20 mm cube hollowed with a 2 mm wall: its cavity from 2 to 18 mm.
        let (mut workspace, preparation, directory) = crate::hollowing_ui::tests::cube(true);
        let mut ui = SupportsUi {
            placing: true,
            on_cavity: true,
            straight_only: true,
            ..SupportsUi::default()
        };
        let started = Instant::now();
        while ui.poll(&workspace, &preparation, Some(0)) || ui.route.is_none() {
            assert!(started.elapsed() < Duration::from_secs(60), "no grid");
            std::thread::sleep(Duration::from_millis(10));
        }
        let [x0, y0, _] = workspace.placement_of(0).unwrap().min.coordinates();
        // From below, the cavity's ceiling is the face turned away: it faces down, into the void.
        let up = crate::camera::Ray {
            origin: [x0 + 10.0, y0 + 10.0, -50.0],
            direction: [0.0, 0.0, 1.0],
        };
        let hit = preparation
            .cavity_hit(&workspace, 0, &up, crate::viewport::Section::default())
            .expect("the cavity's ceiling");
        assert!(hit.normal[2] < -0.99 && hit.point[2] > 15.0, "{hit:?}");
        // Even with "Avoid the model" off, its foot stands on the cavity's floor.
        assert!(ui.click(&mut workspace, &up, Some(hit), 0));
        let added = workspace.supports(0).unwrap().supports[0].clone();
        let BaseSpec::Model { point_mm, .. } = added.base else {
            panic!("{:?}", added.base);
        };
        let floor = workspace.model_to_world(0).unwrap().point(point_mm);
        assert!(floor[2] > 1.0 && floor[2] < 3.0, "{floor:?}");
        // Built against the hollowed shell: an interior support, without an error.
        ui.placing = false;
        super::super::tests::built(&mut ui, &workspace, &preparation);
        let scene = ui.builds[&0].scene.as_ref().unwrap();
        assert!(!scene.has_errors(), "{:?}", scene.findings());
        assert!(scene.supports()[0].interior_tip);
        // Its tip handle moves on the cavity's ceiling, not on the model's outside.
        ui.selected = Some((0, added.id));
        assert!(ui.press_tip_for_test(&mut workspace));
        assert!(ui.dragging_inside());
        let aside = crate::camera::Ray {
            origin: [x0 + 7.0, y0 + 10.0, -50.0],
            direction: [0.0, 0.0, 1.0],
        };
        let ceiling =
            preparation.cavity_hit(&workspace, 0, &aside, crate::viewport::Section::default());
        ui.drag_to(
            &mut workspace,
            &aside,
            (crate::viewport::Section::default(), TipAt::Free),
            ceiling,
        );
        ui.release(&mut workspace);
        let moved = workspace.supports(0).unwrap().supports[0].clone();
        let tip = workspace.model_to_world(0).unwrap().point(moved.tip_mm);
        assert!((tip[0] - x0 - 7.0).abs() < 1e-9 && tip[2] > 15.0, "{tip:?}");
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn a_support_over_more_of_the_model_goes_aside_unless_told_not_to() {
        let (mut workspace, preparation, directory) = ledge();
        let mut ui = SupportsUi {
            placing: true,
            ..SupportsUi::default()
        };
        // The surface is gridded off the interface thread while supports are placed.
        let started = Instant::now();
        while ui.poll(&workspace, &preparation, Some(0)) || ui.route.is_none() {
            assert!(started.elapsed() < Duration::from_secs(60), "no grid");
            std::thread::sleep(Duration::from_millis(10));
        }
        let placement = workspace.placement_of(0).unwrap();
        let [x0, y0, z0] = placement.min.coordinates();
        // Under the slab, 4 mm from its left edge: the block is below.
        let tip = [x0 + 4.0, y0 + 5.0, z0 + 5.5];
        let hit = SurfaceHit {
            object: 0,
            point: tip,
            normal: [0.0, 0.0, -1.0],
        };
        ui.hover(&workspace, Some(hit), 0);
        let preview = ui.preview.as_ref().expect("a preview");
        assert!(!preview.error);
        let routed = ui.routed(&workspace, 0, tip, [0.0, 0.0, -1.0]).unwrap();
        let [knee] = routed.support.bends[..] else {
            panic!("not aside: {:?}", routed.support.bends);
        };
        // Beside the block, the shaft clear of it.
        assert!(knee[0] >= x0 + 6.0, "{knee:?}");
        assert_eq!(routed.support.tip_axis, AxisSpec::Straight);
        // A click adds it so, in the model's frame.
        let ray = crate::camera::Ray {
            origin: [tip[0], tip[1], -50.0],
            direction: [0.0, 0.0, 1.0],
        };
        assert!(ui.click(&mut workspace, &ray, Some(hit), 0));
        let added = workspace.supports(0).unwrap().supports[0].clone();
        assert_eq!(added.bends.len(), 1);
        let back = workspace.model_to_world(0).unwrap().point(added.bends[0]);
        assert!((0..3).all(|k| (back[k] - knee[k]).abs() < 1e-9));
        // With "Avoid the model" off, straight down.
        ui.straight_only = true;
        let straight = ui.routed(&workspace, 0, tip, [0.0, 0.0, -1.0]).unwrap();
        assert_eq!(straight.support.bends.len(), 0);
        assert_eq!(straight.support.base, BaseSpec::Vertical);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn a_routed_support_builds_as_its_route_was_checked_whichever_way_its_tip_points() {
        // TD-129: a tip pointing towards its shaft is built straight down below it, so the route
        // must check that tip, not one along the surface's normal.
        for towards_shaft in [false, true] {
            let (mut workspace, preparation, directory) = ledge();
            let mut ui = SupportsUi {
                placing: true,
                ..SupportsUi::default()
            };
            ui.next.values.tip_towards_shaft = towards_shaft;
            let started = Instant::now();
            while ui.poll(&workspace, &preparation, Some(0)) || ui.route.is_none() {
                assert!(started.elapsed() < Duration::from_secs(60), "no grid");
                std::thread::sleep(Duration::from_millis(10));
            }
            let [x0, y0, z0] = workspace.placement_of(0).unwrap().min.coordinates();
            // Under the slab clear of the block, and on its right side, which faces sideways:
            // straight down is 90° from its normal.
            let tips = [
                ([x0 + 15.0, y0 + 5.0, z0 + 5.5], [0.0, 0.0, -1.0]),
                ([x0 + 20.0, y0 + 5.0, z0 + 6.5], [1.0, 0.0, 0.0]),
            ];
            for (tip, normal) in tips {
                let hit = SurfaceHit {
                    object: 0,
                    point: tip,
                    normal,
                };
                let ray = crate::camera::Ray {
                    origin: std::array::from_fn(|k| tip[k] + 50.0 * normal[k]),
                    direction: normal.map(|c| -c),
                };
                assert!(ui.click(&mut workspace, &ray, Some(hit), 0));
            }
            let added = workspace.supports(0).unwrap().supports.clone();
            assert_eq!(added.len(), 2);
            // The side's support does not keep a tip straight down: it is turned within the
            // limit, or keeps the normal.
            if towards_shaft {
                assert_ne!(added[1].tip_axis, AxisSpec::Straight, "{:?}", added[1]);
            }
            ui.placing = false;
            super::super::tests::built(&mut ui, &workspace, &preparation);
            let scene = ui.builds[&0].scene.as_ref().unwrap();
            assert!(
                !scene.has_errors(),
                "towards the shaft {towards_shaft}: {:?}",
                scene.findings()
            );
            let _ = std::fs::remove_dir_all(directory);
        }
    }

    #[test]
    fn a_mini_support_stands_on_the_model_below_its_tip_or_nowhere() {
        use super::super::{handles::Grip, Candidate};
        use karasu_core::supports::SupportKind;
        let (mut workspace, preparation, directory) = ledge();
        let mut ui = SupportsUi {
            placing: true,
            mini: true,
            ..SupportsUi::default()
        };
        let started = Instant::now();
        while ui.poll(&workspace, &preparation, Some(0)) || ui.route.is_none() {
            assert!(started.elapsed() < Duration::from_secs(60), "no grid");
            std::thread::sleep(Duration::from_millis(10));
        }
        let [x0, y0, z0] = workspace.placement_of(0).unwrap().min.coordinates();
        let under = |x: f64| {
            let tip = [x0 + x, y0 + 5.0, z0 + 5.5];
            let hit = SurfaceHit {
                object: 0,
                point: tip,
                normal: [0.0, 0.0, -1.0],
            };
            let ray = crate::camera::Ray {
                origin: [tip[0], tip[1], -50.0],
                direction: [0.0, 0.0, 1.0],
            };
            (hit, ray)
        };
        // Over the block: one click stands it on the block's top, a rod and two balls.
        let (hit, ray) = under(3.0);
        ui.hover(&workspace, Some(hit), 0);
        assert!(!ui.preview.as_ref().expect("a preview").error);
        assert!(ui.click(&mut workspace, &ray, Some(hit), 0));
        let added = workspace.supports(0).unwrap().supports[0].clone();
        assert_eq!(added.kind, SupportKind::Mini);
        let BaseSpec::Model { point_mm, .. } = added.base else {
            panic!("{:?}", added.base);
        };
        let foot = workspace.model_to_world(0).unwrap().point(point_mm);
        assert!((foot[2] - (z0 + 1.0)).abs() < 1e-9, "{foot:?}");
        ui.placing = false;
        super::super::tests::built(&mut ui, &workspace, &preparation);
        let scene = ui.builds[&0].scene.as_ref().unwrap();
        assert!(!scene.has_errors(), "{:?}", scene.findings());
        // Selected, it has its tip and its foot to move, nothing to turn.
        assert_eq!(ui.grips_for_test(&workspace, 0), [Grip::Tip, Grip::Foot]);
        // Beyond the block only the plate is below: nowhere to stand, nothing added.
        ui.placing = true;
        let (hit, ray) = under(15.0);
        ui.hover(&workspace, Some(hit), 0);
        assert_eq!(ui.candidate, Some(Candidate::NoFoot));
        assert!(ui.preview.is_none());
        assert!(ui.click(&mut workspace, &ray, Some(hit), 0));
        assert_eq!(workspace.supports(0).unwrap().supports.len(), 1);
        // Tip, then foot: the foot waits for the model, then stands where the pointer meets it.
        ui.set_tip_then_foot(true);
        let (hit, ray) = under(2.0);
        assert!(ui.click(&mut workspace, &ray, Some(hit), 0));
        assert!(ui.click(&mut workspace, &ray, Some(hit), 0));
        assert_eq!(workspace.supports(0).unwrap().supports.len(), 1);
        // From the left, under the slab, down onto the block's top at 1 mm from its edge.
        let slanted = crate::camera::Ray {
            origin: [x0 - 9.0, y0 + 5.0, z0 + 11.0],
            direction: [
                std::f64::consts::FRAC_1_SQRT_2,
                0.0,
                -std::f64::consts::FRAC_1_SQRT_2,
            ],
        };
        ui.drag_to(
            &mut workspace,
            &slanted,
            (crate::viewport::Section::default(), TipAt::Free),
            None,
        );
        assert!(ui.click(&mut workspace, &slanted, None, 0));
        let footed = workspace.supports(0).unwrap().supports[1].clone();
        assert_eq!(footed.kind, SupportKind::Mini);
        let BaseSpec::Model { point_mm, .. } = footed.base else {
            panic!("{:?}", footed.base);
        };
        let foot = workspace.model_to_world(0).unwrap().point(point_mm);
        assert!(
            (foot[0] - (x0 + 1.0)).abs() < 1e-6 && (foot[2] - (z0 + 1.0)).abs() < 1e-6,
            "{foot:?}"
        );
        let _ = std::fs::remove_dir_all(directory);
    }
}
