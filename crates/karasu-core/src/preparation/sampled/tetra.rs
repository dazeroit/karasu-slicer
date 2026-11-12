//! Marching tetrahedra over the Kuhn (Freudenthal) subdivision of each grid cell.
//!
//! Every cube is split into six tetrahedra sharing its main diagonal. The split is identical in
//! every cell, so neighbouring cells triangulate shared faces identically and the tetrahedra form
//! one conforming triangulation of the grid. On each tetrahedron the sampled field is extended
//! linearly; its zero set is a planar triangle or quadrilateral strictly inside the tetrahedron
//! whenever no vertex value is zero. The union is therefore a closed, edge-manifold, embedded
//! surface: polygons of different tetrahedra meet only along the shared iso-edges on common faces.
//! Iso-vertices are identified by their grid edge, never by position, so welding is exact.
//!
//! Corner bits: x = 1, y = 2, z = 4. Within a Kuhn tetrahedron every edge joins a corner to a
//! corner whose bit set contains it, so an edge is named by its lower corner and a nonzero
//! direction in `{0,1}^3`.

/// The six positively oriented tetrahedra: `(b-a) x (c-a) . (d-a) > 0` in grid coordinates.
pub(super) const TETRAHEDRA: [[usize; 4]; 6] = [
    [0, 1, 3, 7],
    [0, 2, 6, 7],
    [0, 4, 5, 7],
    [0, 5, 1, 7],
    [0, 6, 4, 7],
    [0, 3, 2, 7],
];

/// An iso-vertex location: the cube corners at the ends of the crossed grid edge, lower first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CornerEdge {
    pub(super) lower: usize,
    pub(super) upper: usize,
}

fn edge(a: usize, b: usize) -> CornerEdge {
    // Kuhn edges are comparable under bit inclusion; the subset corner is the lower end.
    if a & b == a {
        CornerEdge { lower: a, upper: b }
    } else {
        CornerEdge { lower: b, upper: a }
    }
}

/// Reorder tetrahedron vertex positions so `front` comes first, preserving orientation parity.
fn even_order(tet: [usize; 4], front: &[usize]) -> [usize; 4] {
    let mut positions = [0usize; 4];
    let mut n = 0;
    for &p in front {
        positions[n] = p;
        n += 1;
    }
    for p in 0..4 {
        if !front.contains(&p) {
            positions[n] = p;
            n += 1;
        }
    }
    let mut inversions = 0;
    for i in 0..4 {
        for j in i + 1..4 {
            if positions[i] > positions[j] {
                inversions += 1;
            }
        }
    }
    if inversions % 2 == 1 {
        positions.swap(2, 3);
    }
    positions.map(|p| tet[p])
}

/// Emit the iso-triangles of one cell, oriented with normals pointing from positive values
/// (cavity) towards negative values (material). `values` must contain no zeros.
pub(super) fn polygonize_cell(values: &[f64; 8], mut emit: impl FnMut([CornerEdge; 3])) {
    for tet in TETRAHEDRA {
        let positive: Vec<usize> = (0..4).filter(|&p| values[tet[p]] > 0.0).collect();
        match positive.len() {
            1 => {
                let o = even_order(tet, &positive);
                emit([edge(o[0], o[1]), edge(o[0], o[2]), edge(o[0], o[3])]);
            }
            3 => {
                let negative = (0..4).find(|p| !positive.contains(p)).unwrap_or(0);
                let o = even_order(tet, &[negative]);
                emit([edge(o[0], o[1]), edge(o[0], o[3]), edge(o[0], o[2])]);
            }
            2 => {
                let o = even_order(tet, &positive);
                let (e02, e03, e13, e12) = (
                    edge(o[0], o[2]),
                    edge(o[0], o[3]),
                    edge(o[1], o[3]),
                    edge(o[1], o[2]),
                );
                emit([e02, e03, e13]);
                emit([e02, e13, e12]);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{polygonize_cell, CornerEdge, TETRAHEDRA};
    use std::collections::HashMap;

    fn corner(c: usize) -> [f64; 3] {
        let bit = |shift: usize| if c >> shift & 1 == 1 { 1.0 } else { 0.0 };
        [bit(0), bit(1), bit(2)]
    }

    #[allow(clippy::many_single_char_names)]
    fn orient(a: [f64; 3], b: [f64; 3], c: [f64; 3], d: [f64; 3]) -> f64 {
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let w = [d[0] - a[0], d[1] - a[1], d[2] - a[2]];
        (u[1] * v[2] - u[2] * v[1]) * w[0]
            + (u[2] * v[0] - u[0] * v[2]) * w[1]
            + (u[0] * v[1] - u[1] * v[0]) * w[2]
    }

    fn point(values: &[f64; 8], e: CornerEdge) -> [f64; 3] {
        let (a, b) = (corner(e.lower), corner(e.upper));
        let s = values[e.lower] / (values[e.lower] - values[e.upper]);
        [0, 1, 2].map(|i| a[i] + s * (b[i] - a[i]))
    }

    #[test]
    fn tetrahedra_are_positive_and_tile_the_unit_cube() {
        let mut volume = 0.0;
        for t in TETRAHEDRA {
            let o = orient(corner(t[0]), corner(t[1]), corner(t[2]), corner(t[3]));
            assert!(o > 0.0, "{t:?}");
            volume += o;
        }
        // Six unit determinants: total volume 6 * (1/6) = 1, the whole cube.
        assert_eq!(volume, 6.0);
    }

    #[test]
    fn every_sign_pattern_gives_a_closed_outward_surface_patch() {
        // Within one cube each iso-edge on a cube face is used once and each interior one twice.
        for pattern in 1u32..255 {
            let values: [f64; 8] =
                std::array::from_fn(|c| if pattern >> c & 1 == 1 { 1.0 } else { -1.5 });
            let mut directed: HashMap<(usize, usize, usize, usize), i32> = HashMap::new();
            let mut triangles = Vec::new();
            polygonize_cell(&values, |t| triangles.push(t));
            for t in &triangles {
                let p = t.map(|e| point(&values, e));
                let n = {
                    let u = [0, 1, 2].map(|i| p[1][i] - p[0][i]);
                    let v = [0, 1, 2].map(|i| p[2][i] - p[0][i]);
                    [
                        u[1] * v[2] - u[2] * v[1],
                        u[2] * v[0] - u[0] * v[2],
                        u[0] * v[1] - u[1] * v[0],
                    ]
                };
                assert!(n.iter().map(|x| x * x).sum::<f64>() > 0.0, "{pattern}");
                for k in 0..3 {
                    let (a, b) = (t[k], t[(k + 1) % 3]);
                    let key = (a.lower, a.upper, b.lower, b.upper);
                    let reverse = (b.lower, b.upper, a.lower, a.upper);
                    *directed.entry(key).or_default() += 1;
                    *directed.entry(reverse).or_default() -= 1;
                }
            }
            // Interior iso-edges are traversed once in each direction, so they cancel.
            for ((al, au, bl, bu), count) in directed {
                assert!(count.abs() <= 1, "{pattern}: {al}{au}-{bl}{bu} {count}");
            }
        }
    }

    #[test]
    fn single_positive_corner_normal_points_away_from_it() {
        let mut values = [-1.0; 8];
        values[0] = 1.0;
        let mut triangles = Vec::new();
        polygonize_cell(&values, |t| triangles.push(t));
        // Corner 0 belongs to all six tetrahedra, each cut by one triangle.
        assert_eq!(triangles.len(), 6);
        for t in triangles {
            let p = t.map(|e| point(&values, e));
            let u = [0, 1, 2].map(|i| p[1][i] - p[0][i]);
            let v = [0, 1, 2].map(|i| p[2][i] - p[0][i]);
            let n = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            // Corner 0 is the origin; the outward (away from cavity) normal has positive components.
            assert!(n[0] + n[1] + n[2] > 0.0, "{n:?}");
        }
    }
}
