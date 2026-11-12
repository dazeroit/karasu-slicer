//! Self-intersections of a mesh (M6.4d r1, TD-092): pairs of faces that meet anywhere other than
//! the vertices and edges they share, by the exact contact test of hole filling over a BVH.
//!
//! This is a diagnosis on request, not part of the defect report computed at every import: on a
//! million faces it costs about as much as the import itself. Overlapping shells (two closed
//! bodies crossing) count like a body crossing itself; the slicer unions both.

use super::{
    bvh::{BuildFailure, Bvh},
    contact::improper_contact,
    distance::Triangle,
};
use crate::{
    cancellation::{Cancellation, Cancelled, Checkpoint},
    mesh::{FaceNormal, TriangleMesh},
};
use std::{error::Error, fmt};

/// What the self-intersection diagnosis found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IntersectionReport {
    /// Pairs of faces found meeting improperly, up to the bound asked for.
    pub pairs: usize,
    /// Whether every pair was examined: `false` when the bound stopped the search.
    pub complete: bool,
    /// Where: the centroid of the first face of the first pairs found, at most
    /// [`MAX_INTERSECTION_SAMPLES`], in source coordinates.
    pub samples: Vec<[f64; 3]>,
}

/// At most this many intersection positions are kept.
pub const MAX_INTERSECTION_SAMPLES: usize = 16;

/// Why the diagnosis could not run.
#[derive(Debug, PartialEq)]
pub enum IntersectionError {
    /// Cooperative cancellation.
    Cancelled(Cancelled),
    /// The search structure could not be allocated.
    Resource,
}

impl fmt::Display for IntersectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled(e) => write!(f, "{e}"),
            Self::Resource => f.write_str("cannot allocate the self-intersection search"),
        }
    }
}

impl Error for IntersectionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Cancelled(e) => Some(e),
            Self::Resource => None,
        }
    }
}

impl From<Cancelled> for IntersectionError {
    fn from(e: Cancelled) -> Self {
        Self::Cancelled(e)
    }
}

/// Find the faces of `mesh` that meet improperly, stopping after `max_pairs`. Faces without a
/// reliable normal are left out, as topology leaves them out: the defect report counts them.
///
/// # Errors
/// Cancellation and allocation failures.
pub fn self_intersections(
    mesh: &TriangleMesh,
    max_pairs: usize,
    cancel: &mut Cancellation<'_>,
) -> Result<IntersectionReport, IntersectionError> {
    let vertex = |v: u32| {
        let p = mesh.vertices()[v as usize];
        [p.x, p.y, p.z]
    };
    let corners = |face: u32| mesh.triangles()[face as usize].map(vertex);
    let mut ids = Vec::new();
    let mut triangles = Vec::new();
    for (face, t) in mesh.triangles().iter().enumerate() {
        cancel.poll(Checkpoint::PreparationRecords, face)?;
        if matches!(mesh.triangle_normal(face), Some(FaceNormal::Unit(_))) {
            ids.push(u32::try_from(face).map_err(|_| IntersectionError::Resource)?);
            triangles.push(Triangle::new(t.map(vertex)));
        }
    }
    let bvh =
        Bvh::build(triangles, cancel, Checkpoint::PreparationRecords).map_err(|e| match e {
            BuildFailure::Cancelled(c) => IntersectionError::Cancelled(c),
            BuildFailure::Allocation => IntersectionError::Resource,
        })?;
    let mut report = IntersectionReport {
        complete: true,
        ..IntersectionReport::default()
    };
    // The search visits each overlapping pair once; returning true stops it.
    bvh.find_pair(cancel, Checkpoint::PreparationRecords, |a, b| {
        let (first, second) = (ids[a as usize], ids[b as usize]);
        let faces = (
            mesh.triangles()[first as usize],
            mesh.triangles()[second as usize],
        );
        if !improper_contact(faces.0, corners(first), faces.1, corners(second)) {
            return false;
        }
        report.pairs += 1;
        if report.samples.len() < MAX_INTERSECTION_SAMPLES {
            let triangle = corners(first);
            report.samples.push(std::array::from_fn(|k| {
                (triangle[0][k] + triangle[1][k] + triangle[2][k]) / 3.0
            }));
        }
        if report.pairs >= max_pairs {
            report.complete = false;
            return true;
        }
        false
    })?;
    Ok(report)
}
