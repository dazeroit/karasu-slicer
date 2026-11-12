//! Immutable indexed source geometry, bounded model import (STL, OBJ, 3MF through
//! [`formats`]), and non-mutating transformed bounds.

mod bounds;
mod diagnostics;
mod error;
pub mod formats;
pub mod repair;
pub mod stl;
pub mod topology;
pub(crate) mod weld;

use crate::cancellation::{Cancellation, Checkpoint};
use crate::geometry::{Affine3, ObjectTransform, Point3, TransformError};
pub use bounds::{Aabb, TransformedBoundsError};
pub use diagnostics::{Degeneracy, FaceNormal, MeshDiagnostic};
pub use error::{ImportResource, MeshError, StlError};

/// Three vertex-buffer indices in preserved source winding order.
pub type Triangle = [u32; 3];

/// Basic structural statistics, not a watertightness/manifold/printability classification.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshStatistics {
    /// Number of entries in the vertex buffer (not necessarily distinct for manual construction).
    pub vertex_count: usize,
    /// Number of faces, including duplicate and degenerate faces.
    pub triangle_count: usize,
    /// Source-space bounds over the entire vertex buffer, including unused manual vertices.
    pub source_bounds: Option<Aabb>,
}

/// Owned source coordinates in millimeters and indexed topology. Geometry is immutable after
/// validation; STL metadata/normals and scene transforms are separate objects.
#[derive(Debug, PartialEq)]
pub struct TriangleMesh {
    vertices: Vec<Point3>,
    triangles: Vec<Triangle>,
    bounds: Option<Aabb>,
}

impl TriangleMesh {
    /// Analyzes source connectivity without mutation, using default import-sized resource caps.
    /// See [`topology`] for exclusion rules and the limited meaning of manifold findings.
    ///
    /// # Errors
    /// Returns a structured resource, arithmetic, or allocation failure; no partial report escapes.
    pub fn analyze_topology(&self) -> Result<topology::TopologyReport, topology::TopologyError> {
        self.analyze_topology_with_limits(topology::TopologyLimits::default())
    }

    /// Analyzes source connectivity with explicit vertex/triangle caps, independent of transforms.
    ///
    /// # Errors
    /// Returns a structured resource, arithmetic, or allocation failure.
    pub fn analyze_topology_with_limits(
        &self,
        limits: topology::TopologyLimits,
    ) -> Result<topology::TopologyReport, topology::TopologyError> {
        self.analyze_topology_with_cancellation(limits, &mut Cancellation::never())
    }

    /// Analyzes topology with explicit limits and synchronous cooperative cancellation.
    /// See [`crate::cancellation`] for polling, atomic results, and uninterruptible phase limits.
    ///
    /// # Errors
    /// Returns typed cancellation or the same resource/arithmetic/allocation errors as ordinary analysis.
    pub fn analyze_topology_with_cancellation(
        &self,
        limits: topology::TopologyLimits,
        cancellation: &mut Cancellation<'_>,
    ) -> Result<topology::TopologyReport, topology::TopologyError> {
        topology::analyze(self, limits, cancellation)
    }

    /// Edge closure and orientation counts only (see [`topology::EdgeSummary`]), with one word of
    /// scratch per directed edge use instead of the full report's incidence structures.
    ///
    /// # Errors
    /// Returns typed cancellation and the same resource, arithmetic and allocation errors as
    /// [`Self::analyze_topology_with_cancellation`].
    pub fn edge_summary_with_cancellation(
        &self,
        limits: topology::TopologyLimits,
        cancellation: &mut Cancellation<'_>,
    ) -> Result<topology::EdgeSummary, topology::TopologyError> {
        topology::edge_summary(self, limits, cancellation)
    }

    /// Exact defect counts and boundary rims (see [`topology::DefectReport`]) from one sorted word
    /// pair per directed edge use, without the full report's incidence structures.
    ///
    /// # Errors
    /// Returns typed cancellation and the same resource, arithmetic and allocation errors as
    /// [`Self::analyze_topology_with_cancellation`].
    pub fn defect_report_with_cancellation(
        &self,
        limits: topology::TopologyLimits,
        cancellation: &mut Cancellation<'_>,
    ) -> Result<topology::DefectReport, topology::TopologyError> {
        topology::defects(self, limits, cancellation).map(|(report, _)| report)
    }

    /// The defect report and the boundary rim table used by section closure.
    pub(crate) fn rims_with_cancellation(
        &self,
        limits: topology::TopologyLimits,
        cancellation: &mut Cancellation<'_>,
    ) -> Result<(topology::DefectReport, topology::RimTable), topology::TopologyError> {
        topology::defects(self, limits, cancellation)
    }

    /// Apply explicit repairs (TD-054, see [`repair`]) to a derived mesh. Returns `None` when
    /// nothing changed, and the report of every change.
    ///
    /// # Errors
    /// Cancellation, the topology limits, and allocation or size failures.
    pub fn repair_with_cancellation(
        &self,
        policy: repair::RepairPolicy,
        limits: topology::TopologyLimits,
        cancellation: &mut Cancellation<'_>,
    ) -> Result<(Option<Self>, repair::RepairReport), topology::TopologyError> {
        repair::repair(self, policy, limits, &mut Vec::new(), cancellation)
    }

    /// [`Self::repair_with_cancellation`], with where each kind of change was made: at most
    /// [`repair::MAX_REPAIR_SAMPLES`] positions per kind, in source coordinates.
    ///
    /// # Errors
    /// Cancellation, the topology limits, and allocation or size failures.
    pub fn repair_with_samples(
        &self,
        policy: repair::RepairPolicy,
        limits: topology::TopologyLimits,
        cancellation: &mut Cancellation<'_>,
    ) -> Result<
        (
            Option<Self>,
            repair::RepairReport,
            Vec<repair::RepairSample>,
        ),
        topology::TopologyError,
    > {
        let mut samples = Vec::new();
        let (mesh, report) = repair::repair(self, policy, limits, &mut samples, cancellation)?;
        Ok((mesh, report, samples))
    }

    /// Takes ownership without welding/reordering vertices or faces; normalizes signed zeros.
    /// Unused vertices are retained and included in bounds. Empty buffers are valid.
    ///
    /// # Errors
    /// Rejects non-finite coordinates, invalid indices, and unrepresentable finite extents.
    pub fn try_new(vertices: Vec<Point3>, triangles: Vec<Triangle>) -> Result<Self, MeshError> {
        Self::try_new_with_cancellation(vertices, triangles, &mut Cancellation::never())
    }

    /// Constructs an owned mesh with cancellation during validation and source-bound accumulation.
    /// Input buffers are consumed; cancellation drops them without publishing a partial mesh.
    ///
    /// # Errors
    /// Returns typed cancellation or the same construction failures as [`Self::try_new`].
    pub fn try_new_with_cancellation(
        mut vertices: Vec<Point3>,
        triangles: Vec<Triangle>,
        cancellation: &mut Cancellation<'_>,
    ) -> Result<Self, MeshError> {
        cancellation.check(Checkpoint::MeshStart)?;
        for (vertex, point) in vertices.iter_mut().enumerate() {
            cancellation.poll(Checkpoint::MeshVertices, vertex)?;
            for (axis, value) in [&mut point.x, &mut point.y, &mut point.z]
                .into_iter()
                .enumerate()
            {
                if !value.is_finite() {
                    return Err(MeshError::NonFiniteVertex { vertex, axis });
                }
                if *value == 0.0 {
                    *value = 0.0;
                }
            }
        }
        for (triangle, face) in triangles.iter().enumerate() {
            cancellation.poll(Checkpoint::MeshIndices, triangle)?;
            for (corner, &index) in face.iter().enumerate() {
                if index as usize >= vertices.len() {
                    return Err(MeshError::InvalidIndex {
                        triangle,
                        corner,
                        index,
                        vertex_count: vertices.len(),
                    });
                }
            }
        }
        let bounds = Aabb::from_vertices(&vertices, cancellation)?;
        cancellation.check(Checkpoint::MeshComplete)?;
        Ok(Self {
            vertices,
            triangles,
            bounds,
        })
    }

    /// Immutable original vertex buffer in millimeters.
    #[must_use]
    pub fn vertices(&self) -> &[Point3] {
        &self.vertices
    }

    /// Immutable face buffer in source facet/corner order.
    #[must_use]
    pub fn triangles(&self) -> &[Triangle] {
        &self.triangles
    }

    /// Source bounds, without scene placement or rotation. `None` means no vertices.
    #[must_use]
    pub const fn source_bounds(&self) -> Option<Aabb> {
        self.bounds
    }

    /// Tight extrema of every transformed source vertex under the direct local-pivot object rule.
    /// Includes unused manual vertices, like source bounds. No placement, copies, or mutation occurs.
    /// `None` means no vertices. Precision is that of the computed points, not exact real arithmetic.
    ///
    /// # Errors
    /// Returns the first non-finite transform failure with source vertex context, or extent overflow.
    pub fn world_bounds(
        &self,
        transform: &ObjectTransform,
    ) -> Result<Option<Aabb>, TransformedBoundsError> {
        self.world_bounds_with_cancellation(transform, &mut Cancellation::never())
    }

    /// Computes tight world bounds with cooperative cancellation over every source vertex.
    /// Empty results also pass the entry and final checkpoints. Source geometry is never changed.
    ///
    /// # Errors
    /// Returns typed cancellation or the ordinary transformed-bound failures.
    pub fn world_bounds_with_cancellation(
        &self,
        transform: &ObjectTransform,
        cancellation: &mut Cancellation<'_>,
    ) -> Result<Option<Aabb>, TransformedBoundsError> {
        self.bound_transformed(|point| transform.transform_point(point), cancellation)
    }

    /// Tight transformed-vertex bounds for an explicitly composed affine mapping. The output space
    /// is the affine's target space (world when used as source-to-world). Does not transform the
    /// source AABB corners or decompose the affine into object parameters.
    ///
    /// # Errors
    /// Returns a contextual transform failure or an unrepresentable output extent.
    pub fn transformed_bounds(
        &self,
        transform: &Affine3,
    ) -> Result<Option<Aabb>, TransformedBoundsError> {
        self.transformed_bounds_with_cancellation(transform, &mut Cancellation::never())
    }

    /// Computes tight affine bounds with cooperative cancellation, without source mutation.
    ///
    /// # Errors
    /// Returns typed cancellation or the ordinary transformed-bound failures.
    pub fn transformed_bounds_with_cancellation(
        &self,
        transform: &Affine3,
        cancellation: &mut Cancellation<'_>,
    ) -> Result<Option<Aabb>, TransformedBoundsError> {
        self.bound_transformed(|point| transform.transform_point(point), cancellation)
    }

    fn bound_transformed(
        &self,
        transform: impl Fn(Point3) -> Result<Point3, TransformError>,
        cancellation: &mut Cancellation<'_>,
    ) -> Result<Option<Aabb>, TransformedBoundsError> {
        cancellation.check(Checkpoint::WorldBounds)?;
        let mut points = self.vertices.iter().copied().enumerate();
        let Some((vertex, first)) = points.next() else {
            cancellation.check(Checkpoint::BoundsComplete)?;
            return Ok(None);
        };
        let first =
            transform(first).map_err(|source| TransformedBoundsError::Vertex { vertex, source })?;
        let mut min = first.coordinates();
        let mut max = min;
        for (vertex, point) in points {
            cancellation.poll(Checkpoint::WorldBounds, vertex)?;
            let point = transform(point)
                .map_err(|source| TransformedBoundsError::Vertex { vertex, source })?;
            for (axis, value) in point.coordinates().into_iter().enumerate() {
                min[axis] = min[axis].min(value);
                max[axis] = max[axis].max(value);
            }
        }
        let bounds = Aabb::from_extrema(
            Point3::new(min[0], min[1], min[2]),
            Point3::new(max[0], max[1], max[2]),
        )
        .map(Some)
        .map_err(|_| TransformedBoundsError::UnrepresentableExtent)?;
        cancellation.check(Checkpoint::BoundsComplete)?;
        Ok(bounds)
    }

    /// Basic counts and source bounds, deterministically derived at construction.
    #[must_use]
    pub fn statistics(&self) -> MeshStatistics {
        MeshStatistics {
            vertex_count: self.vertices.len(),
            triangle_count: self.triangles.len(),
            source_bounds: self.bounds,
        }
    }

    /// Returns a geometry-derived normal, or `None` if the triangle index is absent.
    #[must_use]
    pub fn triangle_normal(&self, triangle: usize) -> Option<FaceNormal> {
        self.triangles
            .get(triangle)
            .map(|indices| diagnostics::normal(indices.map(|i| self.vertices[i as usize])))
    }

    /// Detects repeated corners, zero computed area, and exact duplicate face geometry.
    /// No geometry is removed or changed. No topology or self-intersection claim is made.
    ///
    /// # Errors
    /// Returns an allocation error if diagnostic/key buffers cannot be reserved.
    pub fn diagnostics(&self) -> Result<Vec<MeshDiagnostic>, MeshError> {
        self.diagnostics_with_cancellation(&mut Cancellation::never())
    }

    /// Returns complete geometric diagnostics or typed cancellation; source geometry stays intact.
    ///
    /// # Errors
    /// Returns cancellation or the same allocation failures as [`Self::diagnostics`].
    pub fn diagnostics_with_cancellation(
        &self,
        cancellation: &mut Cancellation<'_>,
    ) -> Result<Vec<MeshDiagnostic>, MeshError> {
        diagnostics::analyze(self, cancellation)
    }
}
