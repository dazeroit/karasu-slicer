//! Independent exact occupancy and topology acceptance for explicit material construction.
use karasu_core::cancellation::{Cancellation, Cancelled, Checkpoint};
use karasu_core::slicing::construct::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug)]
struct Rat(i128, i128);
impl Rat {
    fn sub(self, rhs: Self) -> Self {
        Self(self.0 * rhs.1 - rhs.0 * self.1, self.1 * rhs.1)
    }
    fn mul(self, rhs: Self) -> Self {
        Self(self.0 * rhs.0, self.1 * rhs.1)
    }
    fn cmp(self, rhs: Self) -> std::cmp::Ordering {
        (self.0 * rhs.1).cmp(&(rhs.0 * self.1))
    }
}
type Point = [Rat; 2];
fn magnitude(limbs: [u64; 8]) -> i128 {
    assert!(limbs[2..].iter().all(|&v| v == 0));
    i128::try_from(u128::from(limbs[0]) | (u128::from(limbs[1]) << 64)).unwrap()
}
fn exact(p: ExactPoint) -> Point {
    let d = magnitude(p.denominator());
    [p.numerator_x(), p.numerator_y()].map(|(negative, n)| {
        Rat(
            if negative {
                -magnitude(n)
            } else {
                magnitude(n)
            },
            d,
        )
    })
}
fn integer(p: [i64; 2]) -> Point {
    p.map(|v| Rat(i128::from(v), 1))
}
fn side(a: Point, b: Point, q: Point) -> i128 {
    b[0].sub(a[0])
        .mul(q[1].sub(a[1]))
        .sub(b[1].sub(a[1]).mul(q[0].sub(a[0])))
        .0
}
// Independent half-open ray crossing; None explicitly excludes boundary samples.
fn occupancy(edges: impl IntoIterator<Item = (Point, Point)>, q: Point) -> Option<bool> {
    let mut inside = false;
    for (a, b) in edges {
        let s = side(a, b, q);
        let between = |axis: usize| {
            let low = if a[axis].cmp(b[axis]).is_le() {
                a[axis]
            } else {
                b[axis]
            };
            let high = if a[axis].cmp(b[axis]).is_ge() {
                a[axis]
            } else {
                b[axis]
            };
            low.cmp(q[axis]).is_le() && high.cmp(q[axis]).is_ge()
        };
        if s == 0 && between(0) && between(1) {
            return None;
        }
        if a[1].cmp(q[1]).is_gt() != b[1].cmp(q[1]).is_gt()
            && ((b[1].cmp(a[1]).is_gt() && s > 0) || (b[1].cmp(a[1]).is_lt() && s < 0))
        {
            inside = !inside;
        }
    }
    Some(inside)
}
fn input_occupancy(rings: &[MaterialRing<'_>], q: Point) -> Option<bool> {
    let mut groups = BTreeMap::new();
    for ring in rings {
        let inside = occupancy(
            (0..ring.vertices.len()).map(|i| {
                (
                    integer(ring.vertices[i]),
                    integer(ring.vertices[(i + 1) % ring.vertices.len()]),
                )
            }),
            q,
        )?;
        *groups.entry(ring.material).or_insert(false) ^= inside;
    }
    Some(groups.values().any(|v| *v))
}
fn rectangle(x: i64, y: i64, w: i64, h: i64) -> Vec<[i64; 2]> {
    vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]]
}
fn rings<'a>(shapes: &'a [Vec<[i64; 2]>], groups: &[u64]) -> Vec<MaterialRing<'a>> {
    shapes
        .iter()
        .zip(groups)
        .enumerate()
        .map(|(source_path, (vertices, &material))| MaterialRing {
            vertices,
            material,
            source_path,
        })
        .collect()
}
fn verify(shapes: &[Vec<[i64; 2]>], groups: &[u64]) -> ConstructedMaterial {
    let input = rings(shapes, groups);
    let out = union_material_rings(&input, ConstructLimits::default()).unwrap();
    assert_eq!(
        out,
        union_material_rings(&input, ConstructLimits::default()).unwrap()
    );
    for x in -7..43 {
        for y in -7..43 {
            let q = [Rat(x, 3), Rat(y, 3)];
            if let Some(expected) = input_occupancy(&input, q) {
                assert_eq!(
                    occupancy(
                        out.edges().iter().map(|e| (
                            exact(out.vertices()[e.endpoints[0]]),
                            exact(out.vertices()[e.endpoints[1]])
                        )),
                        q
                    ),
                    Some(expected),
                    "sample {q:?}, shapes {shapes:?}"
                );
            }
        }
    }
    let mut used = BTreeSet::new();
    for id in 0..out.rings().len() {
        let cycle = out.ring_edges(id).unwrap();
        assert!(cycle.len() >= 3);
        let mut vertices = BTreeSet::new();
        for (i, &edge) in cycle.iter().enumerate() {
            assert!(used.insert(edge));
            let e = out.edges()[edge];
            assert!(vertices.insert(e.endpoints[0]));
            assert_eq!(
                e.endpoints[1],
                out.edges()[cycle[(i + 1) % cycle.len()]].endpoints[0]
            );
        }
        assert_eq!(
            out.edges()[cycle[0]].endpoints[0],
            *vertices.first().unwrap()
        );
    }
    assert_eq!(used.len(), out.edges().len());
    for (id, atom) in out.atoms().iter().enumerate() {
        assert!(out.vertices()[atom.endpoints[0]] < out.vertices()[atom.endpoints[1]]);
        let supports = out.atom_supports(id).unwrap();
        assert_ne!(supports, []);
        for support in supports {
            let r = &shapes[support.path];
            for &vertex in &atom.endpoints {
                assert_eq!(
                    side(
                        integer(r[support.edge]),
                        integer(r[(support.edge + 1) % r.len()]),
                        exact(out.vertices()[vertex])
                    ),
                    0
                );
            }
        }
        if let Some(edge) = atom.boundary {
            assert_ne!(atom.left_material, atom.right_material);
            assert_eq!(
                out.edges()[edge].endpoints,
                if atom.left_material {
                    atom.endpoints
                } else {
                    [atom.endpoints[1], atom.endpoints[0]]
                }
            );
        } else {
            assert_eq!(atom.left_material, atom.right_material);
        }
    }
    verify_orientation(&input, shapes, &out);
    verify_provenance(shapes, &out);
    out
}
fn verify_orientation(
    input: &[MaterialRing<'_>],
    shapes: &[Vec<[i64; 2]>],
    out: &ConstructedMaterial,
) {
    // Fixture coordinates are bounded by 12, and all proper intersection denominators
    // by 288. These finite samples stay inside their incident arrangement cells.
    for (id, edge) in out.edges().iter().enumerate() {
        let a = exact(out.vertices()[edge.endpoints[0]]);
        let b = exact(out.vertices()[edge.endpoints[1]]);
        let support = out.edge_supports(id).unwrap()[0];
        let shape = &shapes[support.path];
        let original_a = shape[support.edge];
        let original_b = shape[(support.edge + 1) % shape.len()];
        let canonical_forward = original_a < original_b;
        let atom_forward = out.atoms()[edge.atom].endpoints == edge.endpoints;
        let sign = if canonical_forward == atom_forward {
            1
        } else {
            -1
        };
        let dx = i128::from(original_b[0] - original_a[0]) * sign;
        let dy = i128::from(original_b[1] - original_a[1]) * sign;
        let midpoint = [0, 1].map(|axis| {
            Rat(
                a[axis].0 * b[axis].1 + b[axis].0 * a[axis].1,
                2 * a[axis].1 * b[axis].1,
            )
        });
        let probe = |normal: [i128; 2]| {
            [0, 1].map(|axis| midpoint[axis].sub(Rat(-normal[axis], 1_000_000)))
        };
        assert_eq!(input_occupancy(input, probe([-dy, dx])), Some(true));
        assert_eq!(input_occupancy(input, probe([dy, -dx])), Some(false));
    }
    let mut holes = 0;
    for (id, region) in out.regions().iter().enumerate() {
        let outer = region.outer_boundary();
        assert!(out.rings()[outer].depth.is_multiple_of(2));
        for &hole in out.region_holes(id).unwrap() {
            assert_eq!(out.rings()[hole].parent, Some(outer));
            assert_eq!(out.rings()[hole].depth, out.rings()[outer].depth + 1);
            holes += 1;
        }
    }
    assert_eq!(holes, out.summary().holes);
}
fn verify_provenance(shapes: &[Vec<[i64; 2]>], out: &ConstructedMaterial) {
    // Every original directed edge is covered exactly once by its supported atoms.
    for (path, shape) in shapes.iter().enumerate() {
        for source_edge in 0..shape.len() {
            let mut pieces: Vec<_> = out
                .atoms()
                .iter()
                .enumerate()
                .filter_map(|(id, atom)| {
                    out.atom_supports(id)
                        .unwrap()
                        .iter()
                        .any(|support| support.path == path && support.edge == source_edge)
                        .then_some([
                            out.vertices()[atom.endpoints[0]],
                            out.vertices()[atom.endpoints[1]],
                        ])
                })
                .collect();
            pieces.sort_unstable();
            assert_ne!(pieces.len(), 0);
            for pair in pieces.windows(2) {
                assert_eq!(pair[0][1], pair[1][0]);
            }
            let a = integer(shape[source_edge]);
            let b = integer(shape[(source_edge + 1) % shape.len()]);
            let ends = [exact(pieces[0][0]), exact(pieces.last().unwrap()[1])];
            for axis in 0..2 {
                let (low, high) = if a[axis].cmp(b[axis]).is_le() {
                    (a[axis], b[axis])
                } else {
                    (b[axis], a[axis])
                };
                if a[0].cmp(b[0]).is_eq() || axis == 0 {
                    assert!(ends[0][axis].cmp(low).is_eq());
                    assert!(ends[1][axis].cmp(high).is_eq());
                } else {
                    assert!(
                        ends[0][axis].cmp(a[axis]).is_eq() || ends[0][axis].cmp(b[axis]).is_eq()
                    );
                    assert!(
                        ends[1][axis].cmp(a[axis]).is_eq() || ends[1][axis].cmp(b[axis]).is_eq()
                    );
                }
            }
        }
    }
}

#[test]
fn overlap_parity_union_and_coincidence() {
    let shapes = vec![rectangle(0, 0, 6, 6), rectangle(3, 2, 6, 6)];
    assert_eq!(verify(&shapes, &[0, 1]).regions().len(), 1);
    verify(&shapes, &[0, 0]);
    let mut reversed = shapes[0].clone();
    reversed.reverse();
    let duplicate = vec![shapes[0].clone(), reversed];
    assert_eq!(verify(&duplicate, &[0, 0]).edges(), []);
    assert_eq!(verify(&duplicate, &[0, 1]).rings().len(), 1);
}
#[test]
fn contacts_shared_edges_and_nested_islands() {
    for shapes in [
        vec![rectangle(0, 0, 4, 4), rectangle(4, 4, 4, 4)],
        vec![rectangle(0, 0, 4, 4), rectangle(4, 0, 4, 4)],
        vec![rectangle(0, 0, 4, 4), rectangle(4, 1, 4, 2)],
        vec![rectangle(0, 0, 8, 8), vec![[0, 4], [2, 2], [4, 4], [2, 6]]],
    ] {
        verify(&shapes, &[0, 1]);
        verify(&shapes, &[0, 0]);
    }
    let shapes = vec![
        rectangle(0, 0, 12, 12),
        rectangle(1, 1, 10, 10),
        rectangle(2, 2, 8, 8),
        rectangle(3, 3, 6, 6),
        rectangle(4, 4, 4, 4),
    ];
    let out = verify(&shapes, &[0, 0, 0, 0, 0]);
    assert_eq!(out.regions().len(), 3);
    assert_eq!(out.summary().holes, 2);
}
#[test]
fn rational_intersections_and_source_permutations() {
    let shapes = vec![vec![[0, 0], [9, 3], [1, 9]], vec![[0, 7], [8, 0], [9, 9]]];
    verify(&shapes, &[0, 1]);
    verify(&shapes, &[0, 0]);
    let transformed: Vec<_> = shapes
        .iter()
        .rev()
        .map(|r| {
            let mut r = r.clone();
            r.reverse();
            r.rotate_left(1);
            r
        })
        .collect();
    verify(&transformed, &[1, 0]);
}
#[test]
fn invalid_inputs_are_structured() {
    for (vertices, reason) in [
        (vec![[0, 0], [1, 1]], ConstructInputIssue::TooFewVertices),
        (
            vec![[0, 0], [1, 0], [0, 0]],
            ConstructInputIssue::CollapsedEdge,
        ),
        (
            vec![[0, 0], [4, 4], [0, 4], [4, 0]],
            ConstructInputIssue::SelfContact,
        ),
        (
            vec![[0, 0], [1_i64 << 53, 0], [0, 1]],
            ConstructInputIssue::CoordinateRange,
        ),
    ] {
        let input = [MaterialRing {
            vertices: &vertices,
            material: 0,
            source_path: 42,
        }];
        assert_eq!(
            union_material_rings(&input, ConstructLimits::default()).unwrap_err(),
            ConstructError::InvalidRing { path: 42, reason }
        );
    }
    let shape = rectangle(0, 0, 2, 2);
    let r = MaterialRing {
        vertices: &shape,
        material: 0,
        source_path: 7,
    };
    assert_eq!(
        union_material_rings(&[r, r], ConstructLimits::default()).unwrap_err(),
        ConstructError::InvalidRing {
            path: 7,
            reason: ConstructInputIssue::DuplicatePath
        }
    );
}
#[test]
fn resource_caps_and_all_phase_cancellations_are_atomic() {
    let shapes = vec![rectangle(0, 0, 4, 4), rectangle(2, 2, 4, 4)];
    let input = rings(&shapes, &[0, 1]);
    for resource in [
        ConstructResource::InputEdges,
        ConstructResource::Pairs,
        ConstructResource::Vertices,
        ConstructResource::Atoms,
        ConstructResource::Work,
        ConstructResource::PayloadBytes,
    ] {
        let mut limits = ConstructLimits::default();
        match resource {
            ConstructResource::InputEdges => limits.max_input_edges = 0,
            ConstructResource::Pairs => limits.max_pairs = 0,
            ConstructResource::Vertices => limits.max_vertices = 0,
            ConstructResource::Atoms => limits.max_atoms = 0,
            ConstructResource::Work => limits.max_work = 0,
            ConstructResource::PayloadBytes => limits.max_payload_bytes = 0,
        }
        assert_eq!(
            union_material_rings(&input, limits).unwrap_err(),
            ConstructError::LimitExceeded { resource, limit: 0 }
        );
    }
    for checkpoint in [
        Checkpoint::ConstructStart,
        Checkpoint::ConstructInput,
        Checkpoint::ConstructPairs,
        Checkpoint::ConstructSplit,
        Checkpoint::ConstructClassify,
        Checkpoint::ConstructTopology,
        Checkpoint::ConstructStorage,
        Checkpoint::ConstructComplete,
    ] {
        let mut predicate = |phase| phase == checkpoint;
        assert_eq!(
            union_material_rings_with_cancellation(
                &input,
                ConstructLimits::default(),
                &mut Cancellation::new(&mut predicate)
            )
            .unwrap_err(),
            ConstructError::Cancelled(Cancelled { checkpoint })
        );
    }
}
#[test]
fn full_domain_and_greater_than_i128_intersection() {
    let b = 1_i64 << 52;
    let shapes = vec![
        vec![[0, 0], [b, b - 1], [0, b]],
        vec![[b, 0], [0, b], [b, b]],
    ];
    let out = union_material_rings(&rings(&shapes, &[0, 1]), ConstructLimits::default()).unwrap();
    assert!(out.vertices().iter().any(|p| p.numerator_x().1[2] != 0));
    let full = vec![rectangle(-b, -b, 2 * b, 2 * b)];
    let out = union_material_rings(&rings(&full, &[0]), ConstructLimits::default()).unwrap();
    assert_eq!(out.edges().len(), 4);
    assert_eq!(out.regions().len(), 1);
}

#[test]
fn bounded_rectangle_arrangements_match_independent_occupancy() {
    for x in -1..=4 {
        for y in -1..=4 {
            let shapes = vec![
                rectangle(0, 0, 4, 4),
                rectangle(x, y, 3, 2),
                rectangle(1, 1, 2, 2),
            ];
            verify(&shapes, &[0, 1, 0]);
        }
    }
}

#[test]
fn exact_resource_boundaries_and_empty_material() {
    let empty = union_material_rings(&[], ConstructLimits::default()).unwrap();
    assert_eq!(empty.vertices(), []);
    assert_eq!(empty.edges(), []);
    assert_eq!(empty.regions(), []);
    let shapes = vec![rectangle(0, 0, 4, 4), rectangle(2, 2, 4, 4)];
    let input = rings(&shapes, &[0, 1]);
    let baseline = union_material_rings(&input, ConstructLimits::default()).unwrap();
    let summary = baseline.summary();
    let limits = ConstructLimits {
        max_input_edges: summary.input_edges,
        max_pairs: summary.pairs,
        max_vertices: summary.vertices,
        max_atoms: summary.atoms,
        max_work: summary.work,
        max_payload_bytes: summary.requested_payload_bytes,
    };
    assert_eq!(union_material_rings(&input, limits).unwrap(), baseline);
    for resource in [
        ConstructResource::InputEdges,
        ConstructResource::Pairs,
        ConstructResource::Vertices,
        ConstructResource::Atoms,
        ConstructResource::Work,
        ConstructResource::PayloadBytes,
    ] {
        let mut smaller = limits;
        let cap = match resource {
            ConstructResource::InputEdges => &mut smaller.max_input_edges,
            ConstructResource::Pairs => &mut smaller.max_pairs,
            ConstructResource::Vertices => &mut smaller.max_vertices,
            ConstructResource::Atoms => &mut smaller.max_atoms,
            ConstructResource::Work => &mut smaller.max_work,
            ConstructResource::PayloadBytes => &mut smaller.max_payload_bytes,
        };
        *cap -= 1;
        let limit = *cap;
        assert_eq!(
            union_material_rings(&input, smaller).unwrap_err(),
            ConstructError::LimitExceeded { resource, limit }
        );
    }
}
