use super::{MeshError, TriangleMesh};
use crate::cancellation::{Cancellation, Checkpoint};
use crate::geometry::{Point3, Vector3};

/// Why a geometric normal cannot be supplied. No near-degeneracy tolerance is applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Degeneracy {
    /// At least two corners have exactly equal coordinates (signed zero is normalized).
    RepeatedVertex,
    /// The scale-normalized `f64` cross product is zero. This is not an exact predicate:
    /// collinearity, underflow, or cancellation can produce this result.
    ZeroComputedArea,
}

/// A geometry-derived normal, never a file-supplied normal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FaceNormal {
    /// Right-hand-rule unit normal from preserved triangle order.
    Unit(Vector3),
    /// No reliable unit normal exists under the documented arithmetic policy.
    Undefined(Degeneracy),
}

/// Non-destructive, deterministically ordered observations. None implies printability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshDiagnostic {
    /// Exactly repeated corners or zero computed area; triangle is retained.
    DegenerateTriangle {
        /// Triangle index in source facet order.
        triangle: usize,
        /// Exact-repeat versus computed-area classification.
        kind: Degeneracy,
    },
    /// Same three exact corner coordinates as an earlier face, regardless of winding.
    DuplicateTriangle {
        /// Later triangle index.
        triangle: usize,
        /// First triangle with this geometry.
        first: usize,
    },
    /// A retained file normal has at least one non-finite component.
    NonFiniteStoredNormal {
        /// Source facet index.
        triangle: usize,
    },
    /// A retained file normal is exactly zero; finite vertices still import.
    ZeroStoredNormal {
        /// Source facet index.
        triangle: usize,
    },
    /// A finite stored normal has negative dot product with the computed unit normal.
    OpposingStoredNormal {
        /// Source facet index. Winding is not changed.
        triangle: usize,
    },
    /// Opaque binary attribute bits are nonzero; no vendor/color interpretation is made.
    NonzeroAttribute {
        /// Source facet index.
        triangle: usize,
        /// Original unsigned field.
        value: u16,
    },
}

pub(super) fn normal([origin, second, third]: [Point3; 3]) -> FaceNormal {
    if origin == second || second == third || third == origin {
        return FaceNormal::Undefined(Degeneracy::RepeatedVertex);
    }
    let edge = |point: Point3| {
        Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z).scaled_direction()
    };
    // Mesh bounds guarantee finite differences. Distinct corners give nonzero edges.
    let (Some(first_edge), Some(second_edge)) = (edge(second), edge(third)) else {
        return FaceNormal::Undefined(Degeneracy::ZeroComputedArea);
    };
    let cross = Vector3::new(
        first_edge.y * second_edge.z - first_edge.z * second_edge.y,
        first_edge.z * second_edge.x - first_edge.x * second_edge.z,
        first_edge.x * second_edge.y - first_edge.y * second_edge.x,
    );
    match cross.unit() {
        Some(n) => FaceNormal::Unit(n),
        None => FaceNormal::Undefined(Degeneracy::ZeroComputedArea),
    }
}

pub(super) fn analyze(
    mesh: &TriangleMesh,
    cancellation: &mut Cancellation<'_>,
) -> Result<Vec<MeshDiagnostic>, MeshError> {
    cancellation.check(Checkpoint::DiagnosticsStart)?;
    let duplicates = duplicate_triangles(mesh, cancellation)?;
    // Degeneracy of every triangle, computed on all cores.
    let mut degeneracies = Vec::new();
    degeneracies
        .try_reserve_exact(mesh.triangles().len())
        .map_err(|_| allocation("degeneracy flags"))?;
    degeneracies.resize(mesh.triangles().len(), None);
    crate::parallel::fill_with(&mut degeneracies, |triangle| {
        let points = mesh.triangles()[triangle].map(|index| mesh.vertices()[index as usize]);
        match normal(points) {
            FaceNormal::Undefined(kind) => Some(kind),
            FaceNormal::Unit(_) => None,
        }
    });
    let mut diagnostics = Vec::new();
    let mut next = duplicates.iter().peekable();
    for (triangle, degeneracy) in degeneracies.iter().enumerate() {
        cancellation.poll(Checkpoint::GeometryDiagnostics, triangle)?;
        if let Some(kind) = *degeneracy {
            push(
                &mut diagnostics,
                MeshDiagnostic::DegenerateTriangle { triangle, kind },
            )?;
        }
        if let Some(&&(_, first)) = next.peek().filter(|(t, _)| *t == triangle) {
            next.next();
            push(
                &mut diagnostics,
                MeshDiagnostic::DuplicateTriangle { triangle, first },
            )?;
        }
    }
    cancellation.check(Checkpoint::DiagnosticsComplete)?;
    Ok(diagnostics)
}

/// Whether each triangle has a defined normal, i.e. is not excluded as degenerate; evaluated on
/// every available core.
pub(super) fn usable_faces(mesh: &TriangleMesh) -> Result<Vec<bool>, MeshError> {
    let count = mesh.triangles().len();
    let mut usable = Vec::new();
    usable
        .try_reserve_exact(count)
        .map_err(|_| allocation("usable triangles"))?;
    usable.resize(count, false);
    crate::parallel::fill_with(&mut usable, |face| {
        let corners = mesh.triangles()[face].map(|index| mesh.vertices()[index as usize]);
        !matches!(normal(corners), FaceNormal::Undefined(_))
    });
    Ok(usable)
}

fn allocation(context: &'static str) -> MeshError {
    MeshError::Allocation { context }
}

/// For every vertex, the smallest vertex index with exactly the same coordinates (signed zero
/// normalized), so equal positions share one identity.
pub(super) fn canonical_vertices(
    mesh: &TriangleMesh,
    cancellation: &mut Cancellation<'_>,
) -> Result<Vec<u32>, MeshError> {
    let vertices = mesh.vertices();
    let count = u32::try_from(vertices.len()).map_err(|_| allocation("vertex identities"))?;
    let mut order = Vec::new();
    order
        .try_reserve_exact(vertices.len())
        .map_err(|_| allocation("vertex identities"))?;
    order.extend(0..count);
    cancellation.check(Checkpoint::GeometryDiagnostics)?;
    // Keys include the index, so the parallel sort's order is unique.
    crate::parallel::sort_by_key(&mut order, |&v| (vertices[v as usize].key(), v));
    cancellation.check(Checkpoint::GeometryDiagnostics)?;
    let mut canonical = Vec::new();
    canonical
        .try_reserve_exact(vertices.len())
        .map_err(|_| allocation("vertex identities"))?;
    canonical.resize(vertices.len(), 0);
    let mut first = 0;
    for (position, &v) in order.iter().enumerate() {
        cancellation.poll(Checkpoint::GeometryDiagnostics, position)?;
        if position == 0 || vertices[v as usize].key() != vertices[first as usize].key() {
            first = v;
        }
        canonical[v as usize] = first;
    }
    Ok(canonical)
}

/// `(triangle, first)` for every triangle whose three corner positions, regardless of winding,
/// equal those of an earlier triangle `first` (the earliest one), in ascending triangle order.
///
/// Corners are compared through canonical vertex identities and grouped by sorting, so scratch
/// is a few words per vertex and triangle instead of a coordinate hash table.
fn duplicate_triangles(
    mesh: &TriangleMesh,
    cancellation: &mut Cancellation<'_>,
) -> Result<Vec<(usize, usize)>, MeshError> {
    let canonical = canonical_vertices(mesh, cancellation)?;
    let mut keys = Vec::new();
    keys.try_reserve_exact(mesh.triangles().len())
        .map_err(|_| allocation("duplicate-triangle keys"))?;
    keys.resize(mesh.triangles().len(), ([0u32; 3], 0usize));
    cancellation.check(Checkpoint::GeometryDiagnostics)?;
    crate::parallel::fill_with(&mut keys, |triangle| {
        let mut key = mesh.triangles()[triangle].map(|index| canonical[index as usize]);
        key.sort_unstable();
        (key, triangle)
    });
    drop(canonical);
    cancellation.check(Checkpoint::GeometryDiagnostics)?;
    crate::parallel::sort_by_key(&mut keys, |&key| key);
    cancellation.check(Checkpoint::GeometryDiagnostics)?;
    let mut duplicates = Vec::new();
    let mut group = 0;
    for position in 0..keys.len() {
        cancellation.poll(Checkpoint::GeometryDiagnostics, position)?;
        if keys[position].0 != keys[group].0 {
            group = position;
        } else if position != group {
            duplicates
                .try_reserve(1)
                .map_err(|_| allocation("duplicate-triangle keys"))?;
            duplicates.push((keys[position].1, keys[group].1));
        }
    }
    duplicates.sort_unstable();
    Ok(duplicates)
}

pub(super) fn push(items: &mut Vec<MeshDiagnostic>, item: MeshDiagnostic) -> Result<(), MeshError> {
    items.try_reserve(1).map_err(|_| MeshError::Allocation {
        context: "mesh diagnostics",
    })?;
    items.push(item);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// The coordinate-keyed hash detection the sorted grouping replaced.
    fn hashed(mesh: &TriangleMesh) -> Vec<MeshDiagnostic> {
        let mut diagnostics = Vec::new();
        let mut seen = HashMap::new();
        for (triangle, indices) in mesh.triangles().iter().enumerate() {
            let points = indices.map(|index| mesh.vertices()[index as usize]);
            if let FaceNormal::Undefined(kind) = normal(points) {
                diagnostics.push(MeshDiagnostic::DegenerateTriangle { triangle, kind });
            }
            let mut key = points.map(Point3::key);
            key.sort_unstable();
            if let Some(&first) = seen.get(&key) {
                diagnostics.push(MeshDiagnostic::DuplicateTriangle { triangle, first });
            } else {
                seen.insert(key, triangle);
            }
        }
        diagnostics
    }

    #[test]
    fn sorted_duplicate_grouping_matches_coordinate_hashing() {
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = |m: u32| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            u32::try_from(state % u64::from(m)).unwrap()
        };
        for case in 0..200 {
            // Few distinct positions, repeated at several indices (including -0.0 and +0.0), so
            // duplicates, repeated corners and permuted windings are frequent.
            let positions = 2 + next(6);
            let vertices: Vec<Point3> = (0..=2 + next(20))
                .map(|_| {
                    let p = next(positions);
                    let zero = if next(2) == 0 { 0.0 } else { -0.0 };
                    Point3::new(f64::from(p % 3), f64::from(p / 3), zero)
                })
                .collect();
            let count = u32::try_from(vertices.len()).unwrap();
            let triangles: Vec<[u32; 3]> = (0..=next(40))
                .map(|_| [next(count), next(count), next(count)])
                .collect();
            let mesh = TriangleMesh::try_new(vertices, triangles).unwrap();
            let actual = analyze(&mesh, &mut Cancellation::never()).unwrap();
            assert_eq!(actual, hashed(&mesh), "case {case}");
        }
    }
}
