use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Vec3 {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) z: f64,
}

impl Vec3 {
    pub(crate) const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub(crate) fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }

    pub(crate) fn sub(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }

    pub(crate) fn scale(self, factor: f64) -> Self {
        Self::new(self.x * factor, self.y * factor, self.z * factor)
    }

    pub(crate) fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    pub(crate) fn cross(self, other: Self) -> Self {
        Self::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    pub(crate) fn length(self) -> f64 {
        self.dot(self).sqrt()
    }

    pub(crate) fn normalized(self) -> Self {
        let length = self.length();
        assert!(length > 0.0, "cannot normalize a zero-length vector");
        self.scale(1.0 / length)
    }

    pub(crate) fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Mesh {
    pub(crate) vertices: Vec<Vec3>,
    pub(crate) triangles: Vec<[usize; 3]>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Bounds {
    pub(crate) min: Vec3,
    pub(crate) max: Vec3,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Topology {
    pub(crate) connected_components: usize,
    pub(crate) boundary_edges: usize,
    pub(crate) non_manifold_edges: usize,
    pub(crate) inconsistent_oriented_edges: usize,
    pub(crate) degenerate_triangles: usize,
    pub(crate) duplicate_triangles: usize,
}

impl Mesh {
    pub(crate) fn bounds(&self) -> Bounds {
        let first = self.vertices[0];
        let mut min = first;
        let mut max = first;
        for vertex in &self.vertices[1..] {
            min.x = min.x.min(vertex.x);
            min.y = min.y.min(vertex.y);
            min.z = min.z.min(vertex.z);
            max.x = max.x.max(vertex.x);
            max.y = max.y.max(vertex.y);
            max.z = max.z.max(vertex.z);
        }
        Bounds { min, max }
    }

    pub(crate) fn all_coordinates_finite(&self) -> bool {
        self.vertices.iter().all(|vertex| vertex.is_finite())
    }

    pub(crate) fn signed_volume(&self) -> f64 {
        self.triangles
            .iter()
            .map(|triangle| {
                let a = self.vertices[triangle[0]];
                let b = self.vertices[triangle[1]];
                let c = self.vertices[triangle[2]];
                a.dot(b.cross(c)) / 6.0
            })
            .sum()
    }

    pub(crate) fn topology(&self) -> Topology {
        let mut edge_uses: BTreeMap<(usize, usize), Vec<(usize, usize)>> = BTreeMap::new();
        let mut vertex_to_triangles = vec![Vec::new(); self.vertices.len()];
        let mut degenerate_triangles = 0;
        let mut canonical_triangles = BTreeSet::new();
        let mut duplicate_triangles = 0;

        for (triangle_index, triangle) in self.triangles.iter().enumerate() {
            for vertex in triangle {
                vertex_to_triangles[*vertex].push(triangle_index);
            }
            for (from, to) in [
                (triangle[0], triangle[1]),
                (triangle[1], triangle[2]),
                (triangle[2], triangle[0]),
            ] {
                let key = if from < to { (from, to) } else { (to, from) };
                edge_uses.entry(key).or_default().push((from, to));
            }

            let a = self.vertices[triangle[0]];
            let b = self.vertices[triangle[1]];
            let c = self.vertices[triangle[2]];
            if b.sub(a).cross(c.sub(a)).length() == 0.0 {
                degenerate_triangles += 1;
            }

            let mut canonical = *triangle;
            canonical.sort_unstable();
            if !canonical_triangles.insert(canonical) {
                duplicate_triangles += 1;
            }
        }

        let boundary_edges = edge_uses.values().filter(|uses| uses.len() == 1).count();
        let non_manifold_edges = edge_uses.values().filter(|uses| uses.len() > 2).count();
        let inconsistent_oriented_edges = edge_uses
            .values()
            .filter(|uses| uses.len() == 2 && uses[0] == uses[1])
            .count();

        let mut visited = vec![false; self.triangles.len()];
        let mut connected_components = 0;
        for start in 0..self.triangles.len() {
            if visited[start] {
                continue;
            }
            connected_components += 1;
            visited[start] = true;
            let mut queue = VecDeque::from([start]);
            while let Some(current) = queue.pop_front() {
                for vertex in self.triangles[current] {
                    for adjacent in &vertex_to_triangles[vertex] {
                        if !visited[*adjacent] {
                            visited[*adjacent] = true;
                            queue.push_back(*adjacent);
                        }
                    }
                }
            }
        }

        Topology {
            connected_components,
            boundary_edges,
            non_manifold_edges,
            inconsistent_oriented_edges,
            degenerate_triangles,
            duplicate_triangles,
        }
    }
}

pub(crate) fn box_mesh(min: Vec3, max: Vec3, inward: bool) -> Mesh {
    let vertices = vec![
        Vec3::new(min.x, min.y, min.z),
        Vec3::new(max.x, min.y, min.z),
        Vec3::new(max.x, max.y, min.z),
        Vec3::new(min.x, max.y, min.z),
        Vec3::new(min.x, min.y, max.z),
        Vec3::new(max.x, min.y, max.z),
        Vec3::new(max.x, max.y, max.z),
        Vec3::new(min.x, max.y, max.z),
    ];
    let quads = [
        [0, 3, 2, 1],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [1, 2, 6, 5],
        [2, 3, 7, 6],
        [3, 0, 4, 7],
    ];
    let mut triangles = Vec::with_capacity(12);
    for [a, b, c, d] in quads {
        triangles.push([a, b, c]);
        triangles.push([a, c, d]);
    }
    if inward {
        for triangle in &mut triangles {
            triangle.swap(1, 2);
        }
    }
    Mesh {
        vertices,
        triangles,
    }
}

pub(crate) fn merge_meshes(meshes: &[Mesh]) -> Mesh {
    let vertex_count = meshes.iter().map(|mesh| mesh.vertices.len()).sum();
    let triangle_count = meshes.iter().map(|mesh| mesh.triangles.len()).sum();
    let mut vertices = Vec::with_capacity(vertex_count);
    let mut triangles = Vec::with_capacity(triangle_count);
    for mesh in meshes {
        let offset = vertices.len();
        vertices.extend_from_slice(&mesh.vertices);
        triangles.extend(
            mesh.triangles
                .iter()
                .map(|triangle| triangle.map(|index| index + offset)),
        );
    }
    Mesh {
        vertices,
        triangles,
    }
}

pub(crate) fn cylinder(radius: f64, height: f64, segments: usize) -> Mesh {
    let segment_count = u32::try_from(segments).expect("fixture segment count must fit in u32");
    let mut vertices = Vec::with_capacity(2 * segments + 2);
    vertices.push(Vec3::new(0.0, 0.0, 0.0));
    vertices.push(Vec3::new(0.0, 0.0, height));
    for z in [0.0, height] {
        for index in 0..segments {
            let index = u32::try_from(index).expect("fixture segment index must fit in u32");
            let angle = std::f64::consts::TAU * f64::from(index) / f64::from(segment_count);
            vertices.push(Vec3::new(radius * angle.cos(), radius * angle.sin(), z));
        }
    }

    let mut triangles = Vec::with_capacity(4 * segments);
    for index in 0..segments {
        let next = (index + 1) % segments;
        let bottom = 2 + index;
        let bottom_next = 2 + next;
        let top = 2 + segments + index;
        let top_next = 2 + segments + next;
        triangles.push([0, bottom_next, bottom]);
        triangles.push([1, top, top_next]);
        triangles.push([bottom, bottom_next, top_next]);
        triangles.push([bottom, top_next, top]);
    }
    Mesh {
        vertices,
        triangles,
    }
}

pub(crate) fn icosphere(radius: f64, center: Vec3, subdivisions: u32, inward: bool) -> Mesh {
    let golden = 1.0_f64.midpoint(5.0_f64.sqrt());
    let mut vertices = vec![
        Vec3::new(-1.0, golden, 0.0),
        Vec3::new(1.0, golden, 0.0),
        Vec3::new(-1.0, -golden, 0.0),
        Vec3::new(1.0, -golden, 0.0),
        Vec3::new(0.0, -1.0, golden),
        Vec3::new(0.0, 1.0, golden),
        Vec3::new(0.0, -1.0, -golden),
        Vec3::new(0.0, 1.0, -golden),
        Vec3::new(golden, 0.0, -1.0),
        Vec3::new(golden, 0.0, 1.0),
        Vec3::new(-golden, 0.0, -1.0),
        Vec3::new(-golden, 0.0, 1.0),
    ];
    for vertex in &mut vertices {
        *vertex = vertex.normalized();
    }

    let mut triangles = vec![
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];

    for _ in 0..subdivisions {
        let mut midpoints = BTreeMap::new();
        let mut next_triangles = Vec::with_capacity(triangles.len() * 4);
        for [a, b, c] in triangles {
            let ab = midpoint_index(a, b, &mut vertices, &mut midpoints);
            let bc = midpoint_index(b, c, &mut vertices, &mut midpoints);
            let ca = midpoint_index(c, a, &mut vertices, &mut midpoints);
            next_triangles.extend([[a, ab, ca], [b, bc, ab], [c, ca, bc], [ab, bc, ca]]);
        }
        triangles = next_triangles;
    }

    for vertex in &mut vertices {
        *vertex = vertex.scale(radius).add(center);
    }
    if inward {
        for triangle in &mut triangles {
            triangle.swap(1, 2);
        }
    }
    Mesh {
        vertices,
        triangles,
    }
}

fn midpoint_index(
    a: usize,
    b: usize,
    vertices: &mut Vec<Vec3>,
    midpoints: &mut BTreeMap<(usize, usize), usize>,
) -> usize {
    let key = if a < b { (a, b) } else { (b, a) };
    if let Some(index) = midpoints.get(&key) {
        return *index;
    }
    let midpoint = vertices[a].add(vertices[b]).scale(0.5).normalized();
    let index = vertices.len();
    vertices.push(midpoint);
    midpoints.insert(key, index);
    index
}

pub(crate) fn rotated_prism() -> Mesh {
    let mut mesh = box_mesh(
        Vec3::new(-5.0, -10.0, -15.0),
        Vec3::new(5.0, 10.0, 15.0),
        false,
    );
    let x_angle = 23.0_f64.to_radians();
    let z_angle = 17.0_f64.to_radians();
    for vertex in &mut mesh.vertices {
        let after_x = Vec3::new(
            vertex.x,
            vertex.y * x_angle.cos() - vertex.z * x_angle.sin(),
            vertex.y * x_angle.sin() + vertex.z * x_angle.cos(),
        );
        *vertex = Vec3::new(
            after_x.x * z_angle.cos() - after_x.y * z_angle.sin(),
            after_x.x * z_angle.sin() + after_x.y * z_angle.cos(),
            after_x.z,
        );
    }
    let min_z = mesh.bounds().min.z;
    for vertex in &mut mesh.vertices {
        vertex.z -= min_z;
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::{box_mesh, cylinder, icosphere, Vec3};

    #[test]
    fn box_is_closed_and_consistently_oriented() {
        let mesh = box_mesh(Vec3::new(-1.0, -1.0, 0.0), Vec3::new(1.0, 1.0, 2.0), false);
        let topology = mesh.topology();
        assert_eq!(topology.connected_components, 1);
        assert_eq!(topology.boundary_edges, 0);
        assert_eq!(topology.non_manifold_edges, 0);
        assert_eq!(topology.inconsistent_oriented_edges, 0);
        assert_eq!(topology.degenerate_triangles, 0);
        assert!((mesh.signed_volume() - 8.0).abs() < 1.0e-12);

        let inward = box_mesh(Vec3::new(-1.0, -1.0, 0.0), Vec3::new(1.0, 1.0, 2.0), true);
        assert!((inward.signed_volume() + 8.0).abs() < 1.0e-12);
    }

    #[test]
    fn cylinder_counts_match_the_specification() {
        let mesh = cylinder(10.0, 20.0, 128);
        assert_eq!(mesh.vertices.len(), 258);
        assert_eq!(mesh.triangles.len(), 512);
        assert_eq!(mesh.topology().boundary_edges, 0);
    }

    #[test]
    fn icosphere_counts_are_deterministic() {
        let mesh = icosphere(10.0, Vec3::new(0.0, 0.0, 10.0), 3, false);
        assert_eq!(mesh.vertices.len(), 642);
        assert_eq!(mesh.triangles.len(), 1_280);
        assert_eq!(mesh.topology().boundary_edges, 0);
    }
}
