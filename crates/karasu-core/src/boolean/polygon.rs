//! Convex polygons as planes (TD-140): a supporting plane, one bounding plane per edge, and the
//! vertices as the exact meeting points of the support with consecutive edge planes.
//!
//! A polygon's interior lies on the positive side of every edge plane. Its vertex `i` is where
//! edge planes `i − 1` and `i` meet the support: edge `i` runs from vertex `i` to vertex `i + 1`.
//! Splitting by a plane never makes a new kind of plane: every edge plane is an input triangle's
//! edge plane, a cell wall or another polygon's support, so every vertex stays within the bounds
//! of [`super::plane`] however often a polygon is cut. The vertices are kept with the polygon so
//! no meeting point is computed twice.
use super::{
    grid::GridPoint,
    plane::{Plane, Point},
};
use std::cmp::Ordering;

/// A convex polygon of the arrangement.
#[derive(Clone, Debug)]
pub(crate) struct Polygon {
    /// The supporting plane; its positive side is the polygon's front.
    pub(crate) support: Plane,
    /// The bounding planes, interior on their positive side; edge `i` from vertex `i` to `i + 1`.
    pub(crate) edges: Vec<Plane>,
    /// Vertex `i`: the support with edge planes `i − 1` and `i`.
    pub(crate) vertices: Vec<Point>,
    /// The input it comes from.
    pub(crate) input: u32,
    /// The input triangle it is a part of, numbered across all inputs by the arrangement.
    pub(crate) source: u32,
    /// A box around the polygon in grid steps, widened past the approximation's error: a path
    /// outside it cannot cross the polygon. For culling only, never a decision.
    pub(crate) reach: [[f64; 3]; 2],
}

/// The box of `vertices`, widened by a millionth of a step and a hundred-billionth of the
/// coordinates: far beyond the error of [`Point::approximate`] (a few units in the last place).
fn reach(vertices: &[Point]) -> [[f64; 3]; 2] {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for v in vertices {
        let c = v
            .approximate()
            .coordinates()
            .map(|mm| mm * super::grid::STEPS_PER_MM);
        for k in 0..3 {
            lo[k] = lo[k].min(c[k]);
            hi[k] = hi[k].max(c[k]);
        }
    }
    for k in 0..3 {
        let margin = 1e-6 + 1e-11 * lo[k].abs().max(hi[k].abs());
        lo[k] -= margin;
        hi[k] += margin;
    }
    [lo, hi]
}

/// The two parts of a polygon cut by a plane: on its positive and on its negative side.
pub(crate) struct Split {
    pub(crate) positive: Option<Polygon>,
    pub(crate) negative: Option<Polygon>,
}

/// What a walk needs of a polygon ([`super::trace::walk`]): its planes, its box and its input.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Sheet<'a> {
    pub(crate) support: &'a Plane,
    pub(crate) edges: &'a [Plane],
    pub(crate) reach: &'a [[f64; 3]; 2],
    pub(crate) input: u32,
}

/// An input triangle as the arrangement keeps every one of them: its planes and its grid
/// corners, with no exact vertices (they are its corners, made when needed) and no allocation.
/// It takes about 330 bytes, where a [`Polygon`] of three vertices takes about 800: the inputs
/// of a Boolean are millions of triangles, most of which no other surface meets.
#[derive(Clone, Debug)]
pub(crate) struct InputTriangle {
    /// The supporting plane; its positive side is the triangle's front.
    pub(crate) support: Plane,
    /// The bounding planes, interior on their positive side; edge `i` from corner `i` to `i + 1`.
    pub(crate) edges: [Plane; 3],
    pub(crate) corners: [GridPoint; 3],
    /// The input it comes from.
    pub(crate) input: u32,
    /// As [`Polygon::reach`].
    pub(crate) reach: [[f64; 3]; 2],
}

impl InputTriangle {
    /// Triangle `(a, b, c)` of input `input`, its front where `(a, b, c)` turns counterclockwise;
    /// `None` when the three are collinear (a triangle of no area, which adds nothing to the
    /// winding numbers). Each edge plane is the edge extruded along the axis the support's normal
    /// leans on most, which it is never parallel to.
    pub(crate) fn new(a: GridPoint, b: GridPoint, c: GridPoint, input: u32) -> Option<Self> {
        let support = Plane::through(a, b, c)?;
        let axis = (0..3)
            .max_by_key(|&k| (support.n[k].unsigned_abs(), std::cmp::Reverse(k)))
            .unwrap_or(0);
        let edge = |p: GridPoint, q: GridPoint, opposite: GridPoint| {
            let plane = Plane::extrusion(p, q, axis)
                .expect("an edge is never parallel to the normal's main axis");
            if plane.side_of_grid(opposite) == Ordering::Less {
                plane.flipped()
            } else {
                plane
            }
        };
        let corners = [a, b, c];
        Some(Self {
            support,
            edges: [edge(a, b, c), edge(b, c, a), edge(c, a, b)],
            corners,
            input,
            reach: reach(&corners.map(Point::grid)),
        })
    }

    /// The corners as exact points.
    pub(crate) fn vertices(&self) -> [Point; 3] {
        self.corners.map(Point::grid)
    }

    /// The triangle as a polygon to cut, a part of input triangle `source`.
    pub(crate) fn polygon(&self, source: u32) -> Polygon {
        Polygon {
            support: self.support,
            edges: self.edges.to_vec(),
            vertices: self.vertices().to_vec(),
            input: self.input,
            source,
            reach: self.reach,
        }
    }

    /// What a walk needs of it.
    pub(crate) fn sheet(&self) -> Sheet<'_> {
        Sheet {
            support: &self.support,
            edges: &self.edges,
            reach: &self.reach,
            input: self.input,
        }
    }
}

impl Polygon {
    /// [`InputTriangle::new`] as a polygon, a part of input triangle 0.
    #[cfg(test)]
    pub(crate) fn triangle(a: GridPoint, b: GridPoint, c: GridPoint, input: u32) -> Option<Self> {
        InputTriangle::new(a, b, c, input).map(|t| t.polygon(0))
    }

    /// What a walk needs of it.
    pub(crate) fn sheet(&self) -> Sheet<'_> {
        Sheet {
            support: &self.support,
            edges: &self.edges,
            reach: &self.reach,
            input: self.input,
        }
    }

    /// The sides of the vertices to `plane`.
    pub(crate) fn sides(&self, plane: &Plane) -> Vec<Ordering> {
        self.vertices.iter().map(|v| plane.side(v)).collect()
    }

    /// Whether some vertex lies strictly on each side of `plane`.
    pub(crate) fn straddles(&self, plane: &Plane) -> bool {
        let sides = self.sides(plane);
        sides.contains(&Ordering::Greater) && sides.contains(&Ordering::Less)
    }

    /// The polygon cut by `plane`. A part exists only when some vertex lies strictly on its side;
    /// a polygon on the plane itself has neither. `plane` must not be the polygon's support.
    pub(crate) fn split(&self, plane: &Plane) -> Split {
        let sides = self.sides(plane);
        let positive = sides
            .contains(&Ordering::Greater)
            .then(|| self.part(plane, &sides));
        let flipped = plane.flipped();
        let reversed: Vec<Ordering> = sides.iter().map(|s| s.reverse()).collect();
        let negative = sides
            .contains(&Ordering::Less)
            .then(|| self.part(&flipped, &reversed));
        Split { positive, negative }
    }

    /// The part on the positive side of `plane`, whose vertices' sides are `sides`.
    fn part(&self, plane: &Plane, sides: &[Ordering]) -> Self {
        let count = self.vertices.len();
        let mut edges = Vec::with_capacity(count + 1);
        let mut vertices = Vec::with_capacity(count + 1);
        let crossing = |i: usize| {
            Point::meet(&self.support, &self.edges[i], plane)
                .expect("an edge crossing a plane meets it in one point")
        };
        for i in 0..count {
            let (a, b) = (sides[i], sides[(i + 1) % count]);
            match (a, b) {
                // Vertex i kept, and edge i stays (on the plane or entering the positive side).
                (Ordering::Greater | Ordering::Equal, Ordering::Greater | Ordering::Equal) => {
                    vertices.push(self.vertices[i]);
                    edges.push(self.edges[i]);
                }
                // Leaving through the edge's interior: vertex i, then the crossing along `plane`.
                (Ordering::Greater, Ordering::Less) => {
                    vertices.push(self.vertices[i]);
                    edges.push(self.edges[i]);
                    vertices.push(crossing(i));
                    edges.push(*plane);
                }
                // Leaving from vertex i, which lies on the plane: on along `plane`.
                (Ordering::Equal, Ordering::Less) => {
                    vertices.push(self.vertices[i]);
                    edges.push(*plane);
                }
                // Entering through the edge's interior: the crossing, then on along edge i.
                (Ordering::Less, Ordering::Greater) => {
                    vertices.push(crossing(i));
                    edges.push(self.edges[i]);
                }
                // Outside, or reaching the plane at vertex i + 1 (kept next).
                (Ordering::Less, Ordering::Less | Ordering::Equal) => {}
            }
        }
        Self {
            support: self.support,
            edges,
            reach: reach(&vertices),
            vertices,
            input: self.input,
            source: self.source,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Polygon;
    use crate::boolean::{grid::GridPoint, int::tests::Rng, plane::Plane};
    use std::cmp::Ordering;

    fn g(x: i64, y: i64, z: i64) -> GridPoint {
        GridPoint { x, y, z }
    }

    /// Every vertex lies on the support and on the two edge planes it is made of, strictly inside
    /// every other edge plane.
    fn consistent(polygon: &Polygon) {
        let count = polygon.vertices.len();
        assert!(count >= 3, "{count} vertices");
        for (i, v) in polygon.vertices.iter().enumerate() {
            assert_eq!(polygon.support.side(v), Ordering::Equal);
            let previous = (i + count - 1) % count;
            for (j, edge) in polygon.edges.iter().enumerate() {
                let side = edge.side(v);
                if j == i || j == previous {
                    assert_eq!(side, Ordering::Equal, "vertex {i} on edge {j}");
                } else {
                    assert_ne!(side, Ordering::Less, "vertex {i} outside edge {j}");
                }
            }
        }
    }

    #[test]
    fn a_triangle_s_planes_bound_it() {
        let t = Polygon::triangle(g(0, 0, 0), g(10, 0, 0), g(0, 10, 3), 0).unwrap();
        consistent(&t);
        // Its front is counterclockwise.
        assert_eq!(t.support.side_of_grid(g(0, 0, 100)), Ordering::Greater);
        assert!(Polygon::triangle(g(0, 0, 0), g(1, 1, 1), g(2, 2, 2), 0).is_none());
    }

    #[test]
    fn cuts_keep_every_vertex_exact_and_the_parts_cover_the_whole() {
        let mut rng = Rng::new(21);
        let mut cut = 0;
        for _ in 0..3_000 {
            fn p(rng: &mut Rng) -> GridPoint {
                let mut c = || i64::try_from(rng.signed(20)).unwrap();
                g(c(), c(), c())
            }
            let Some(mut polygon) = Polygon::triangle(p(&mut rng), p(&mut rng), p(&mut rng), 1)
            else {
                continue;
            };
            // Up to five random cuts, each part checked, and a random part kept.
            for _ in 0..5 {
                let Some(plane) = Plane::through(p(&mut rng), p(&mut rng), p(&mut rng)) else {
                    continue;
                };
                if plane.parallel(&polygon.support) {
                    continue;
                }
                let split = polygon.split(&plane);
                let straddled = polygon.straddles(&plane);
                assert_eq!(
                    split.positive.is_some() && split.negative.is_some(),
                    straddled
                );
                for (part, side) in [
                    (&split.positive, Ordering::Greater),
                    (&split.negative, Ordering::Less),
                ] {
                    if let Some(part) = part {
                        consistent(part);
                        for v in &part.vertices {
                            assert_ne!(plane.side(v), side.reverse());
                        }
                    }
                }
                if straddled {
                    cut += 1;
                    polygon = if rng.next() & 1 == 0 {
                        split.positive.unwrap()
                    } else {
                        split.negative.unwrap()
                    };
                }
            }
        }
        assert!(cut > 3_000, "{cut}");
    }

    #[test]
    fn a_cut_through_a_vertex_or_along_an_edge_makes_no_sliver() {
        let t = Polygon::triangle(g(0, 0, 0), g(10, 0, 0), g(0, 10, 0), 0).unwrap();
        // x = 0 runs along the edge from (0, 10) to (0, 0): the whole triangle is on one side.
        let wall = Plane::through(g(0, 0, 0), g(0, 0, 5), g(0, 7, 0)).unwrap();
        let split = t.split(&wall);
        assert_ne!(split.positive.is_some(), split.negative.is_some());
        let kept = split.positive.or(split.negative).unwrap();
        assert_eq!(kept.vertices.len(), 3);
        consistent(&kept);
        // x = y passes through vertex (0, 0) and the middle of the opposite edge.
        let diagonal = Plane::through(g(0, 0, 0), g(0, 0, 5), g(5, 5, 0)).unwrap();
        let split = t.split(&diagonal);
        for part in [split.positive.unwrap(), split.negative.unwrap()] {
            assert_eq!(part.vertices.len(), 3);
            consistent(&part);
        }
    }
}
