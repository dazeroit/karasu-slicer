//! Rebuilding a model as a solid (M6.4d r7, TD-092, stage R9): the last-resort repair.
//!
//! The solid is the set of points where the source's winding number is positive, the union of
//! its bodies, as slicing fills it. It is sampled on a uniform grid of the requested resolution:
//! the distance to the source, signed by the exact vertical-ray winding number (positive inside),
//! is evaluated only in blocks the surface may cross (a block whose ball the surface does not
//! touch has one sign), and its zero level is extracted by the marching tetrahedra of hollowing.
//! The result is closed, edge-manifold, consistently oriented outward and free of
//! self-intersections by construction, and is validated as closed and oriented anyway.
//!
//! **Deviation**, both ways, in mm:
//! - every point of the rebuilt surface lies within [`RebuildReport::max_deviation_mm`] of the
//!   source (a posteriori, from exact vertex distances widened per triangle, as hollowing
//!   certifies its walls);
//! - the source's vertices lie within [`RebuildReport::source_deviation_mm`] of the rebuilt
//!   surface (measured at the vertices only): a large value means a feature thinner than the
//!   resolution was lost.
//!
//! **Admission:** the winding number must be defined: zero-area faces and exact zero-area cracks
//! are left out (as hollowing does, H1), and the rest must be balanced. Open or unbalanced
//! surfaces are refused; hole filling and the non-manifold repair come first. Sharp edges and
//! corners are rounded to within the deviation; features thinner than about one cell vanish.
//! Results do not depend on the thread count.

use super::distance::{point_triangle_distance_sq, Triangle, QUERY_RELATIVE_ERROR};
use super::{
    distance_index, field, leave_out_measure_zero, resource, signed_volume, unsupported,
    validate_closed_oriented, vertex_distances, vertex_gap, weld, worker_threads, Grid,
    SampledHollowLimits, GRID_OFFSET_FRACTION,
};
use crate::{
    cancellation::{Cancellation, Checkpoint},
    mesh::{topology::TopologyLimits, TriangleMesh},
    preparation::hollow::HollowError,
};

/// The finest resolution accepted, in mm.
pub const MIN_REBUILD_MM: f64 = 0.001;
/// The coarsest resolution accepted, in mm.
pub const MAX_REBUILD_MM: f64 = 10.0;
/// The resolution of the `Rebuild` preset, in mm, about a resin printer's pixel and layer.
pub const DEFAULT_REBUILD_MM: f64 = 0.05;

/// Leaf blocks are this many cells on a side.
const LEAF_CELLS: u64 = 2;

/// What rebuilding did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RebuildReport {
    /// The grid's cell, in mm.
    pub resolution_mm: f64,
    /// Triangles of the source and of the rebuilt surface.
    pub triangles: [usize; 2],
    /// Upper bound of the distance from any point of the rebuilt surface to the source, in mm.
    pub max_deviation_mm: f64,
    /// Largest distance from a source vertex to the rebuilt surface, in mm.
    pub source_deviation_mm: f64,
    /// Where the source deviates most from the rebuilt surface (that source vertex), when it has
    /// vertices.
    pub largest_deviation_at: Option<[f64; 3]>,
    /// Signed volumes of the source and of the rebuilt solid, in mm³ (the source's counts
    /// overlapping bodies once per body).
    pub volume_mm3: [f64; 2],
    /// Edge-connected surfaces of the rebuilt solid.
    pub surfaces: usize,
}

/// Rebuild `source` as a solid at `resolution` (see the module documentation).
///
/// # Errors
/// A resolution out of range, a surface without a winding number, nothing inside at this
/// resolution, resource limits, cancellation, and a failed validation of the result.
#[allow(clippy::too_many_lines)] // Admission, grid, field, extraction, then the deviations.
pub fn rebuild_solid(
    source: &TriangleMesh,
    resolution: f64,
    limits: SampledHollowLimits,
    cancel: &mut Cancellation<'_>,
) -> Result<(TriangleMesh, RebuildReport), HollowError> {
    if !(MIN_REBUILD_MM..=MAX_REBUILD_MM).contains(&resolution) {
        return Err(unsupported(
            "rebuild resolution must be from 0.001 to 10 mm",
        ));
    }
    if source.triangles().is_empty() {
        return Err(unsupported("nothing to rebuild: the model has no faces"));
    }
    let topology = TopologyLimits {
        max_vertices: limits.max_source_vertices,
        max_triangles: limits.max_source_triangles,
    };
    let (world, _) = leave_out_measure_zero(clone_mesh(source)?, topology, cancel)?;
    let defects = world.defect_report_with_cancellation(topology, cancel)?;
    if defects.boundary_edges > 0
        || defects.winding_conflicts > 0
        || defects.unbalanced_non_manifold_edges > 0
    {
        return Err(unsupported(
            "rebuilding needs a winding number: the surface has holes, inconsistent orientation \
             or unbalanced edges; fill the holes and repair the non-manifold edges first",
        ));
    }
    // Grid: the source's box padded by two cells and offset by a fixed fraction, as hollowing's.
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    let mut triangles = Vec::new();
    triangles
        .try_reserve_exact(world.triangles().len())
        .map_err(|_| resource("triangle allocation"))?;
    for (index, face) in world.triangles().iter().enumerate() {
        cancel.poll(Checkpoint::HollowWork, index)?;
        let corners = face.map(|v| world.vertices()[v as usize].coordinates());
        for c in corners {
            for axis in 0..3 {
                min[axis] = min[axis].min(c[axis]);
                max[axis] = max[axis].max(c[axis]);
            }
        }
        triangles.push(Triangle::new(corners));
    }
    let magnitude = min
        .iter()
        .chain(max.iter())
        .map(|v| v.abs())
        .fold(0.0, f64::max)
        + 4.0 * resolution;
    let error = 2.0 * QUERY_RELATIVE_ERROR * (1.0 + magnitude);
    let origin: [f64; 3] =
        std::array::from_fn(|a| min[a] - (2.0 + GRID_OFFSET_FRACTION) * resolution);
    let mut vertices = [0u64; 3];
    for axis in 0..3 {
        let span = ((max[axis] - origin[axis]) / resolution).ceil() + 3.0;
        if !(span.is_finite() && span < 9.0e15) {
            return Err(resource("grid extent"));
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // In [3, 9e15).
        let span = span as u64;
        vertices[axis] = span;
    }
    let cells = (vertices[0] - 1)
        .checked_mul(vertices[1] - 1)
        .and_then(|v| v.checked_mul(vertices[2] - 1))
        .ok_or_else(|| resource("grid cells"))?;
    if cells > limits.max_grid_cells {
        return Err(resource("grid cells"));
    }
    let grid = Grid {
        origin,
        cell: resolution,
        vertices,
    };
    let bvh = distance_index(triangles, cancel)?;
    let bvh_bytes = bvh.retained_bytes() as u64;
    if bvh_bytes > limits.max_payload_bytes {
        return Err(resource("requested payload"));
    }
    let threads = worker_threads(limits.threads);
    let spec = field::FieldSpec {
        thickness: 0.0,
        margin: error,
        leaf: LEAF_CELLS,
        item: LEAF_CELLS * 8,
        max_queries: limits.max_distance_queries,
        max_triangles: limits.max_cavity_triangles,
        max_payload_bytes: limits.max_payload_bytes - bvh_bytes,
        rebuild: true,
    };
    let (outputs, _) = field::evaluate(
        &bvh,
        grid,
        spec,
        &field::Blockers::default(),
        threads,
        cancel,
        &mut |_, _| {},
    )
    .map_err(|outcome| match super::field_failure(&outcome) {
        HollowError::Resource {
            resource: "cavity triangles",
        } => resource("rebuilt triangles; choose a coarser resolution"),
        error => error,
    })?;
    if outputs.iter().all(|o| o.triangles.is_empty()) {
        return Err(unsupported(
            "nothing to rebuild: the model encloses no volume at this resolution",
        ));
    }
    let rebuilt = weld(outputs, cancel)?;
    let surfaces = validate_closed_oriented(&rebuilt, cancel)?;
    // Rebuilt surface to source: exact vertex distances widened per triangle.
    let distances = vertex_distances(&bvh, &rebuilt, threads);
    let mut max_deviation = 0.0f64;
    for (index, face) in rebuilt.triangles().iter().enumerate() {
        cancel.poll(Checkpoint::HollowWork, index)?;
        let p = face.map(|v| rebuilt.vertices()[v as usize].coordinates());
        let d = face.map(|v| distances[v as usize]);
        max_deviation = max_deviation.max(d[0].max(d[1]).max(d[2]) + vertex_gap(p));
    }
    max_deviation += error;
    // Source vertices to the rebuilt surface.
    let (source_deviation, at) = source_deviation(&world, &rebuilt, resolution, cancel)?;
    let report = RebuildReport {
        resolution_mm: resolution,
        triangles: [source.triangles().len(), rebuilt.triangles().len()],
        max_deviation_mm: max_deviation,
        source_deviation_mm: source_deviation,
        largest_deviation_at: at,
        volume_mm3: [signed_volume(&world), signed_volume(&rebuilt)],
        surfaces,
    };
    Ok((rebuilt, report))
}

/// Rings of cells searched around a source vertex at most; a vertex farther from every rebuilt
/// triangle is reported at this many cells, a lower bound.
const MAX_RINGS: i64 = 64;

/// The largest distance from a vertex used by `world` to the `rebuilt` surface, and that vertex.
///
/// The rebuilt triangles are indexed by the cells (of twice the resolution) their boxes cover.
/// Around each source vertex, rings of cells are searched outward with exact point-triangle
/// distances until the nearest distance found is no larger than the next ring's lower bound:
/// every cell not yet seen is then farther. The vertices are queried on every core.
fn source_deviation(
    world: &TriangleMesh,
    rebuilt: &TriangleMesh,
    resolution: f64,
    cancel: &mut Cancellation<'_>,
) -> Result<(f64, Option<[f64; 3]>), HollowError> {
    let size = 2.0 * resolution;
    let Some(bounds) = rebuilt.statistics().source_bounds else {
        return Ok((0.0, None));
    };
    let low = bounds.min().coordinates();
    #[allow(clippy::cast_possible_truncation)] // Clamped to the 21-bit key range below.
    let cell_of = |p: [f64; 3]| -> [i64; 3] {
        std::array::from_fn(|k| (((p[k] - low[k]) / size).floor() as i64).clamp(-1, (1 << 20) - 1))
    };
    #[allow(clippy::cast_sign_loss)] // Offset by one, so nonnegative.
    let key = |c: [i64; 3]| -> u64 {
        ((c[0] + 1) as u64) << 42 | ((c[1] + 1) as u64) << 21 | (c[2] + 1) as u64
    };
    let corners = |t: [u32; 3]| t.map(|v| rebuilt.vertices()[v as usize].coordinates());
    let mut entries: Vec<(u64, u32)> = Vec::new();
    for (index, &t) in rebuilt.triangles().iter().enumerate() {
        cancel.poll(Checkpoint::HollowWork, index)?;
        let p = corners(t);
        let lo = cell_of(std::array::from_fn(|k| p[0][k].min(p[1][k]).min(p[2][k])));
        let hi = cell_of(std::array::from_fn(|k| p[0][k].max(p[1][k]).max(p[2][k])));
        let face = u32::try_from(index).map_err(|_| resource("rebuilt face index"))?;
        for x in lo[0]..=hi[0] {
            for y in lo[1]..=hi[1] {
                for z in lo[2]..=hi[2] {
                    if entries.len() == entries.capacity() {
                        entries
                            .try_reserve(entries.len().max(1024))
                            .map_err(|_| resource("deviation index"))?;
                    }
                    entries.push((key([x, y, z]), face));
                }
            }
        }
    }
    crate::parallel::sort_by_key(&mut entries, |&e| e);
    cancel.check(Checkpoint::HollowWork)?;
    let mut used: Vec<u32> = Vec::new();
    let mut seen = vec![false; world.vertices().len()];
    for t in world.triangles() {
        for &v in t {
            if !std::mem::replace(&mut seen[v as usize], true) {
                used.push(v);
            }
        }
    }
    used.sort_unstable();
    let nearest = |v: u32| -> f64 {
        let p = world.vertices()[v as usize].coordinates();
        let centre = cell_of(p);
        let mut best = f64::INFINITY;
        for ring in 0..=MAX_RINGS {
            for x in -ring..=ring {
                for y in -ring..=ring {
                    for z in -ring..=ring {
                        if x.abs().max(y.abs()).max(z.abs()) != ring {
                            continue;
                        }
                        let k = key([centre[0] + x, centre[1] + y, centre[2] + z]);
                        let start = entries.partition_point(|e| e.0 < k);
                        for &(_, face) in entries[start..].iter().take_while(|e| e.0 == k) {
                            let t = Triangle::new(corners(rebuilt.triangles()[face as usize]));
                            best = best.min(point_triangle_distance_sq(p, &t));
                        }
                    }
                }
            }
            // Cells beyond this ring are at least `ring * size` away.
            #[allow(clippy::cast_precision_loss)] // Ring counts are tiny.
            let reach = ring as f64 * size;
            if best.sqrt() <= reach {
                return best.sqrt();
            }
        }
        // Nothing closer than the rings' reach: the nearest triangle found, or the reach itself.
        #[allow(clippy::cast_precision_loss)]
        let floor = MAX_RINGS as f64 * size;
        if best.is_finite() {
            best.sqrt()
        } else {
            floor
        }
    };
    let mut distances = vec![0.0f64; used.len()];
    crate::parallel::fill_with(&mut distances, |i| nearest(used[i]));
    cancel.check(Checkpoint::HollowWork)?;
    let mut largest = (0.0f64, None);
    for (&v, &d) in used.iter().zip(&distances) {
        if d > largest.0 {
            largest = (d, Some(world.vertices()[v as usize].coordinates()));
        }
    }
    Ok(largest)
}

fn clone_mesh(mesh: &TriangleMesh) -> Result<TriangleMesh, HollowError> {
    let mut vertices = Vec::new();
    vertices
        .try_reserve_exact(mesh.vertices().len())
        .map_err(|_| resource("source copy"))?;
    vertices.extend_from_slice(mesh.vertices());
    let mut faces = Vec::new();
    faces
        .try_reserve_exact(mesh.triangles().len())
        .map_err(|_| resource("source copy"))?;
    faces.extend_from_slice(mesh.triangles());
    Ok(TriangleMesh::try_new(vertices, faces)?)
}
