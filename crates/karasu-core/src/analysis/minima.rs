//! Local minima of a placed model's surface (TD-107, level 2).
//!
//! **What.** A surface point whose neighbours along the mesh's edges are all higher, with the
//! model's material above it, starts a layer of material that has nothing below it: an island
//! begins there, and it needs a support. The slope alone (level 1) misses these points: the
//! faces around a downward cone's tip are steep, but the tip itself starts in mid-air.
//!
//! **Level groups.** Vertices joined by edges whose ends lie at the same height (within
//! [`LEVEL_TOLERANCE_MM`]) form one group: a level edge, a level loop (a torus lying flat) or a
//! level face facing down. A group is a minimum when every edge leaving it climbs. Its point is
//! its vertices' centroid at its lowest height.
//!
//! **Material above.** A minimum of a closed surface's outer side always has material above.
//! Inside a hollowed model the cavity's floor has its lowest point too, but material lies below
//! it: a group counts only when the faces around it face down, by their area-weighted normals in
//! the placement (the mesh is oriented outward, as everywhere in karasu-slicer; a mirroring
//! placement turns the triangles' winding, which is accounted for).
//!
//! **On the plate.** A minimum at or under [`PLATE_CONTACT_MM`] rests on the plate: it is
//! reported, marked, since the plate holds it.
//!
//! **Determinism.** Groups are found by a union of level edges in vertex order and reported
//! sorted by height, then x, then y; equal inputs give equal reports on every platform.
//!
//! **Cost.** Linear in the mesh: one pass to place the vertices, one over the triangles' edges,
//! and adjacency arrays of six indices per triangle.
use crate::{
    geometry::{Affine3, Point3, TransformError},
    mesh::TriangleMesh,
};

/// Heights within this of each other are level, in mm: a placement's rotation leaves the
/// vertices of a level face differing by rounding (about 1e-12 of their size), far below it,
/// while any real slope of a model differs by far more over an edge.
pub const LEVEL_TOLERANCE_MM: f64 = 1.0e-6;

/// At or under this height a minimum rests on the plate, in mm: the plate holds it (the same
/// height the view uses not to mark a resting base as an overhang).
pub const PLATE_CONTACT_MM: f64 = 1.0e-2;

/// A local minimum: where a layer of material starts with nothing below it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Minimum {
    /// Its point in the placement: the group's centroid in x and y, at its lowest height.
    pub point: Point3,
    /// How many vertices it groups: one for a point, more for a level edge, loop or face.
    pub vertices: usize,
    /// It rests on the plate, which holds it.
    pub on_plate: bool,
}

/// Why the minima could not be found.
#[derive(Clone, Debug, PartialEq)]
pub enum MinimaError {
    /// Placing vertex `vertex` failed.
    Transform {
        /// The vertex's index in the mesh.
        vertex: usize,
        /// The failure.
        source: TransformError,
    },
    /// The mesh has more vertices or edge ends than this analysis indexes (`u32`).
    TooLarge,
}

impl std::fmt::Display for MinimaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transform { vertex, source } => {
                write!(f, "cannot place vertex {vertex}: {source}")
            }
            Self::TooLarge => write!(f, "the mesh is too large to analyse its minima"),
        }
    }
}

impl std::error::Error for MinimaError {}

/// The local minima of `mesh` placed by `placement` (source to world, z up from the plate),
/// sorted by height, then x, then y.
///
/// # Errors
/// [`MinimaError`] when a vertex cannot be placed or the mesh is too large to index.
pub fn local_minima(mesh: &TriangleMesh, placement: &Affine3) -> Result<Vec<Minimum>, MinimaError> {
    let placed: Vec<Point3> = mesh
        .vertices()
        .iter()
        .enumerate()
        .map(|(vertex, point)| {
            placement
                .transform_point(*point)
                .map_err(|source| MinimaError::Transform { vertex, source })
        })
        .collect::<Result<_, _>>()?;
    let count = placed.len();
    if u32::try_from(count).is_err() || mesh.triangles().len().checked_mul(6).is_none() {
        return Err(MinimaError::TooLarge);
    }
    // A mirroring placement reverses the triangles' winding: their normals turn with it.
    let flip = if determinant(placement.linear()) < 0.0 {
        -1.0
    } else {
        1.0
    };
    let (start, neighbours, facing) = adjacency(mesh, &placed, flip);
    let around = |vertex: usize| &neighbours[start[vertex]..start[vertex + 1]];
    // Level groups: a union of level edges, rooted at the lowest index for determinism.
    let mut parent: Vec<usize> = (0..count).collect();
    for vertex in 0..count {
        for &other in around(vertex) {
            let other = other as usize;
            if other > vertex && (placed[vertex].z - placed[other].z).abs() <= LEVEL_TOLERANCE_MM {
                union(&mut parent, vertex, other);
            }
        }
    }
    let mut groups: Vec<Option<Group>> = (0..count).map(|_| None).collect();
    for vertex in 0..count {
        if around(vertex).is_empty() {
            continue;
        }
        let root = find(&mut parent, vertex);
        let p = placed[vertex];
        let leaves_down = around(vertex).iter().any(|&other| {
            let other = other as usize;
            find_const(&parent, other) != root && placed[other].z < p.z
        });
        let group = groups[root].get_or_insert(Group {
            climbs: true,
            vertices: 0,
            x: 0.0,
            y: 0.0,
            z: f64::INFINITY,
            facing: 0.0,
        });
        group.climbs &= !leaves_down;
        group.vertices += 1;
        group.x += p.x;
        group.y += p.y;
        group.z = group.z.min(p.z);
        group.facing += facing[vertex];
    }
    let mut minima: Vec<Minimum> = groups
        .into_iter()
        .flatten()
        .filter(|g| g.climbs && g.facing < 0.0)
        .map(|g| {
            #[allow(clippy::cast_precision_loss)] // Vertex counts far below 2^52.
            let n = g.vertices as f64;
            Minimum {
                point: Point3::new(g.x / n, g.y / n, g.z),
                vertices: g.vertices,
                on_plate: g.z <= PLATE_CONTACT_MM,
            }
        })
        .collect();
    minima.sort_by(|a, b| {
        a.point
            .z
            .total_cmp(&b.point.z)
            .then(a.point.x.total_cmp(&b.point.x))
            .then(a.point.y.total_cmp(&b.point.y))
    });
    Ok(minima)
}

/// Per level group: whether every edge leaving it climbs, its vertices, the sums of their x and
/// y, its lowest z and the z of its faces' summed normals (below zero, they face down).
struct Group {
    climbs: bool,
    vertices: usize,
    x: f64,
    y: f64,
    z: f64,
    facing: f64,
}

/// Each vertex's neighbours along edges, as compressed rows (`start`, then `neighbours`), and the
/// z of its faces' summed area-weighted normals in the placement (`flip` is -1 for a mirroring
/// placement): below zero, they face down.
fn adjacency(
    mesh: &TriangleMesh,
    placed: &[Point3],
    flip: f64,
) -> (Vec<usize>, Vec<u32>, Vec<f64>) {
    let count = placed.len();
    let mut degree = vec![0_u32; count + 1];
    let mut facing = vec![0.0_f64; count];
    for &[a, b, c] in mesh.triangles() {
        let [a, b, c] = [a as usize, b as usize, c as usize];
        for vertex in [a, b, c] {
            degree[vertex] += 2;
        }
        let (pa, pb, pc) = (placed[a], placed[b], placed[c]);
        let normal_z = (pb.x - pa.x) * (pc.y - pa.y) - (pb.y - pa.y) * (pc.x - pa.x);
        for vertex in [a, b, c] {
            facing[vertex] += flip * normal_z;
        }
    }
    let mut start = vec![0_usize; count + 1];
    for vertex in 0..count {
        start[vertex + 1] = start[vertex] + degree[vertex] as usize;
    }
    let mut next = start.clone();
    let mut neighbours = vec![0_u32; start[count]];
    for &[a, b, c] in mesh.triangles() {
        for (from, to) in [(a, b), (b, c), (c, a)] {
            neighbours[next[from as usize]] = to;
            next[from as usize] += 1;
            neighbours[next[to as usize]] = from;
            next[to as usize] += 1;
        }
    }
    (start, neighbours, facing)
}

fn determinant(m: [[f64; 3]; 3]) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

fn find(parent: &mut [usize], vertex: usize) -> usize {
    let mut root = vertex;
    while parent[root] != root {
        root = parent[root];
    }
    // Path compression keeps later finds short.
    let mut at = vertex;
    while parent[at] != root {
        let up = parent[at];
        parent[at] = root;
        at = up;
    }
    root
}

fn find_const(parent: &[usize], vertex: usize) -> usize {
    let mut root = vertex;
    while parent[root] != root {
        root = parent[root];
    }
    root
}

fn union(parent: &mut [usize], a: usize, b: usize) {
    let (ra, rb) = (find(parent, a), find(parent, b));
    // The lower index is the root: groups do not depend on the order edges are met.
    if ra < rb {
        parent[rb] = ra;
    } else if rb < ra {
        parent[ra] = rb;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{Rotation3, Vector3};

    fn mesh(vertices: &[[f64; 3]], triangles: &[[u32; 3]]) -> TriangleMesh {
        TriangleMesh::try_new(
            vertices
                .iter()
                .map(|&[x, y, z]| Point3::new(x, y, z))
                .collect(),
            triangles.to_vec(),
        )
        .unwrap()
    }

    /// A closed, outward pyramid over a square base at `base_z`, its apex at `apex_z`
    /// (under the base: pointing down).
    fn pyramid(base_z: f64, apex_z: f64) -> TriangleMesh {
        let v = [
            [-1.0, -1.0, base_z],
            [1.0, -1.0, base_z],
            [1.0, 1.0, base_z],
            [-1.0, 1.0, base_z],
            [0.0, 0.0, apex_z],
        ];
        // Outward winding for an apex below the base: the base faces up, the sides down.
        let t = [
            [0, 1, 2],
            [0, 2, 3],
            [0, 4, 1],
            [1, 4, 2],
            [2, 4, 3],
            [3, 4, 0],
        ];
        mesh(&v, &t)
    }

    fn cube(low: [f64; 3], high: [f64; 3]) -> TriangleMesh {
        let [x0, y0, z0] = low;
        let [x1, y1, z1] = high;
        let v = [
            [x0, y0, z0],
            [x1, y0, z0],
            [x1, y1, z0],
            [x0, y1, z0],
            [x0, y0, z1],
            [x1, y0, z1],
            [x1, y1, z1],
            [x0, y1, z1],
        ];
        let t = [
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
        mesh(&v, &t)
    }

    #[test]
    fn a_downward_tip_is_a_minimum_and_a_level_bottom_a_group() {
        let tip = local_minima(&pyramid(5.0, 2.0), &Affine3::identity()).unwrap();
        assert_eq!(tip.len(), 1);
        assert_eq!(tip[0].point, Point3::new(0.0, 0.0, 2.0));
        assert_eq!(tip[0].vertices, 1);
        assert!(!tip[0].on_plate);
        // A cube floating 3 mm up: its bottom face is one level group of four vertices.
        let bottom = local_minima(
            &cube([0.0, 0.0, 3.0], [2.0, 4.0, 5.0]),
            &Affine3::identity(),
        )
        .unwrap();
        assert_eq!(bottom.len(), 1);
        assert_eq!(bottom[0].vertices, 4);
        assert_eq!(bottom[0].point, Point3::new(1.0, 2.0, 3.0));
        // Resting on the plate, it is marked so.
        let resting = local_minima(
            &cube([0.0, 0.0, 0.0], [2.0, 2.0, 2.0]),
            &Affine3::identity(),
        )
        .unwrap();
        assert!(resting[0].on_plate);
    }

    #[test]
    fn a_turned_cube_finds_its_lowest_corner_and_level_rounding_stays_one_group() {
        let cube = cube([-1.0, -1.0, -1.0], [1.0, 1.0, 1.0]);
        // Turned so the diagonal (1, 1, 1) points up: the corner (-1, -1, -1) points down.
        let axis = Vector3::new(
            std::f64::consts::FRAC_1_SQRT_2,
            -std::f64::consts::FRAC_1_SQRT_2,
            0.0,
        );
        let angle = (1.0 / 3.0_f64.sqrt()).acos();
        let turned =
            Affine3::from_rotation(Rotation3::from_axis_angle_radians(axis, angle).unwrap());
        let corner = local_minima(&cube, &turned).unwrap();
        assert_eq!(corner.len(), 1, "{corner:?}");
        assert_eq!(corner[0].vertices, 1);
        // Turned 90 degrees about x: the bottom face is level again up to rounding, still one
        // group of four.
        let quarter = Affine3::from_rotation(
            Rotation3::from_euler_xyz_degrees(Vector3::new(90.0, 0.0, 0.0)).unwrap(),
        );
        let face = local_minima(&cube, &quarter).unwrap();
        assert_eq!(face.len(), 1, "{face:?}");
        assert_eq!(face[0].vertices, 4);
    }

    #[test]
    fn a_flat_torus_starts_on_its_lowest_loop() {
        // Radius 5, tube 1, centred 3 mm up; the tube's sections include its lowest point.
        let (major, minor) = (16_u32, 8_u32);
        let mut vertices = Vec::new();
        for i in 0..major {
            let u = f64::from(i) * std::f64::consts::TAU / f64::from(major);
            for j in 0..minor {
                let v = f64::from(j) * std::f64::consts::TAU / f64::from(minor);
                let r = 5.0 + v.cos();
                vertices.push([r * u.cos(), r * u.sin(), 3.0 + v.sin()]);
            }
        }
        let index = |i: u32, j: u32| (i % major) * minor + j % minor;
        let mut triangles = Vec::new();
        for i in 0..major {
            for j in 0..minor {
                let (a, b, c, d) = (
                    index(i, j),
                    index(i + 1, j),
                    index(i + 1, j + 1),
                    index(i, j + 1),
                );
                triangles.push([a, b, c]);
                triangles.push([a, c, d]);
            }
        }
        let torus = mesh(&vertices, &triangles);
        let minima = local_minima(&torus, &Affine3::identity()).unwrap();
        assert_eq!(minima.len(), 1, "{minima:?}");
        assert_eq!(minima[0].vertices, 16);
        assert!((minima[0].point.z - 2.0).abs() < 1e-12);
        assert!(minima[0].point.x.abs() < 1e-9 && minima[0].point.y.abs() < 1e-9);
    }

    #[test]
    fn a_cavity_floor_has_material_below_and_is_no_minimum() {
        // A hollow cube: outer shell outward, inner shell inward (the cavity's faces point
        // into it). The cavity's floor is lowest around it, with material under it.
        let outer = cube([0.0, 0.0, 2.0], [10.0, 10.0, 12.0]);
        let inner = cube([2.0, 2.0, 4.0], [8.0, 8.0, 10.0]);
        let mut vertices = outer.vertices().to_vec();
        let mut triangles = outer.triangles().to_vec();
        let offset = u32::try_from(vertices.len()).unwrap();
        vertices.extend_from_slice(inner.vertices());
        triangles.extend(
            inner
                .triangles()
                .iter()
                .map(|&[a, b, c]| [a + offset, c + offset, b + offset]),
        );
        let hollow = TriangleMesh::try_new(vertices, triangles).unwrap();
        let minima = local_minima(&hollow, &Affine3::identity()).unwrap();
        assert_eq!(minima.len(), 1, "{minima:?}");
        assert!((minima[0].point.z - 2.0).abs() < 1e-12);
    }

    #[test]
    fn a_mirrored_placement_keeps_the_material_side() {
        let mirror = Affine3::from_scale(Vector3::new(-1.0, 1.0, 1.0)).unwrap();
        let tip = local_minima(&pyramid(5.0, 2.0), &mirror).unwrap();
        assert_eq!(tip.len(), 1);
        assert_eq!(tip[0].point.z, 2.0);
    }
}
