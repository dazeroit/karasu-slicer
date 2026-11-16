//! Supports routed clear of the model (owner, 2026-10-29): where a shaft straight down would go
//! through more of the model below its tip, a support goes aside (its shaft in the nearest clear
//! column, the tip leaning to a knee) or, failing that, stands on the model below (see
//! `karasu_core::supports::find_route`). Supports placed by hand and the planners route their
//! supports alike.
//!
//! The route is found against a model's surface (its hollowed shell, or the placed model) and a
//! grid of its triangles, [`RouteSurface`]: built once per preparation and placement, then
//! routes are found at once.
use crate::pipeline::{placed_mesh, Preparation};
use karasu_core::{
    cancellation::Cancellation,
    geometry::ObjectTransform,
    mesh::TriangleMesh,
    supports::{
        find_mini_foot, find_route, AxisSpec, BaseSpec, ManualSupport, MiniQuery, Route,
        RouteQuery, SupportKind, SupportParameters, SurfaceGrid,
    },
};
use std::sync::Arc;

/// The surface routes are found against.
pub enum SurfaceMesh {
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

    /// The surface: the placed model, or the hollowed shell when the preparation holds one.
    #[must_use]
    pub fn mesh(&self) -> Option<&TriangleMesh> {
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

/// A model's surface and its grid, shared with the threads that find routes on it.
pub struct RouteSurface {
    mesh: SurfaceMesh,
    grid: SurfaceGrid,
}

impl RouteSurface {
    /// The surface `mesh` with `grid`, the grid built from that surface's mesh.
    #[must_use]
    pub fn new(mesh: SurfaceMesh, grid: SurfaceGrid) -> Self {
        Self { mesh, grid }
    }

    /// Object `index`'s surface for routes, gridded: the hollowed shell `preparation` holds for
    /// it, or else `source` (the model as imported) placed by `placement`. The Supports tool
    /// grids the selected model so, and the command line's automatic supports (TD-157) too.
    /// `Ok(None)` when the grid is not built: cancelled, or a surface it refuses.
    ///
    /// # Errors
    /// A placement that cannot carry the source's vertices (see [`placed_mesh`]).
    pub fn build(
        source: &TriangleMesh,
        placement: &ObjectTransform,
        preparation: Arc<Preparation>,
        index: usize,
        cancel: &mut Cancellation<'_>,
    ) -> Result<Option<Self>, String> {
        let hollow = preparation
            .objects
            .get(index)
            .and_then(|o| o.as_ref().ok())
            .is_some_and(|o| o.hollowed().is_some());
        let mesh = if hollow {
            SurfaceMesh::Hollow(preparation, index)
        } else {
            SurfaceMesh::Placed(placed_mesh(source, placement)?)
        };
        let grid = mesh.mesh().and_then(|m| SurfaceGrid::build(m, cancel).ok());
        Ok(grid.map(|grid| Self::new(mesh, grid)))
    }

    /// The surface: the hollowed shell, or the placed model, in world millimetres.
    #[must_use]
    pub fn mesh(&self) -> Option<&TriangleMesh> {
        self.mesh.mesh().filter(|m| self.grid.fits(m))
    }

    /// Its grid.
    #[must_use]
    pub fn grid(&self) -> &SurfaceGrid {
        &self.grid
    }

    /// The surface's outward normal at `point` as a support's build measures it (TD-124): the
    /// mean within `radius` of the triangles facing the `facet` normal's way; `facet` itself
    /// without the surface. Planned supports are built alone against a triangle facing this
    /// way, so they take the shape the whole surface gives them.
    #[must_use]
    pub fn normal_at(&self, point: [f64; 3], facet: [f64; 3], radius: f64) -> [f64; 3] {
        self.mesh()
            .map_or(facet, |m| self.grid.surface_normal(m, point, facet, radius))
    }

    /// Whether triangle `t` of the surface is one of a hollowed shell's cavity triangles.
    #[must_use]
    pub fn is_cavity(&self, t: u32) -> bool {
        self.mesh
            .interior_from()
            .is_some_and(|first| t as usize >= first)
    }
}

/// A support in world coordinates, with the surfaces it stands on: its tip's, and its foot's
/// when it stands on the model (point and outward normal), for its preview.
pub struct Routed {
    /// The support, in world coordinates.
    pub support: ManualSupport,
    /// The surfaces it stands on: its tip's, then its foot's on the model, each a point and
    /// the surface's outward normal there.
    pub anchors: Vec<([f64; 3], [f64; 3])>,
}

/// The support of `parameters` with its tip at `tip` (outward `normal`) on `surface`, in world
/// coordinates, routed clear of the model (see [`find_route`]): straight down, aside with a
/// knee, or standing on the model below; with the way found. `document` gives the clearance,
/// the floor and the axis limit, its defaults without one.
#[must_use]
pub fn route_support(
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
#[must_use]
pub fn mini_routed(
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
#[must_use]
pub fn axis_to_model(axis: AxisSpec, to_model: &crate::placement::Affine) -> AxisSpec {
    match axis {
        AxisSpec::Direction(d) => AxisSpec::Direction(to_model.vector(d)),
        other => other,
    }
}

/// The widest radius of a support's parts along its path, in mm.
#[must_use]
pub fn widest(p: &SupportParameters) -> f64 {
    p.shaft_diameter_mm
        .max(p.tip_body_diameter_mm)
        .max(p.shaft_bottom_diameter_mm.unwrap_or(0.0))
        / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tilted_tip_turns_with_the_model_and_the_others_are_its_own() {
        // A model turned a quarter about Z: world +X is its -Y.
        let to_model = crate::placement::Affine {
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
}
