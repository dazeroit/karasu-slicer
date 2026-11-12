//! Independent analytic area and sample occupancy checks for exact material subtraction.
use karasu_core::cancellation::{Cancellation, Checkpoint};
use karasu_core::slicing::construct::*;

fn rectangle(x: i64, y: i64, w: i64, h: i64) -> Vec<[i64; 2]> {
    vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]]
}
fn inputs<'a>(shapes: &'a [Vec<[i64; 2]>], ids: &[u64]) -> Vec<MaterialRing<'a>> {
    shapes
        .iter()
        .zip(ids)
        .enumerate()
        .map(|(source_path, (vertices, &material))| MaterialRing {
            vertices,
            material,
            source_path,
        })
        .collect()
}
fn point(p: ExactPoint) -> [i128; 2] {
    let denominator = p.denominator();
    assert!(denominator[1..].iter().all(|&word| word == 0));
    let denominator = i128::from(denominator[0]);
    [p.numerator_x(), p.numerator_y()].map(|(negative, words)| {
        assert!(words[1..].iter().all(|&word| word == 0));
        let numerator = i128::from(words[0]);
        assert_eq!(numerator % denominator, 0);
        let n = numerator / denominator;
        if negative {
            -n
        } else {
            n
        }
    })
}
fn area(out: &ConstructedMaterial) -> i128 {
    let doubled: i128 = out
        .edges()
        .iter()
        .map(|edge| {
            let a = point(out.vertices()[edge.endpoints[0]]);
            let b = point(out.vertices()[edge.endpoints[1]]);
            a[0] * b[1] - a[1] * b[0]
        })
        .sum();
    assert_eq!(doubled % 2, 0);
    doubled / 2
}
fn inside_polygon(points: impl Iterator<Item = [i128; 2]>, query: [i128; 2]) -> bool {
    let vertices: Vec<_> = points.collect();
    let mut inside = false;
    for (a, b) in vertices
        .iter()
        .zip(vertices.iter().cycle().skip(1))
        .take(vertices.len())
    {
        let a = a.map(|n| n * 2);
        let b = b.map(|n| n * 2);
        let side = (b[0] - a[0]) * (query[1] - a[1]) - (b[1] - a[1]) * (query[0] - a[0]);
        if (a[1] > query[1]) != (b[1] > query[1])
            && ((b[1] > a[1] && side > 0) || (b[1] < a[1] && side < 0))
        {
            inside = !inside;
        }
    }
    inside
}
fn verify(shapes: &[Vec<[i64; 2]>], ids: &[u64], expected_area: i128) -> ConstructedMaterial {
    let input = inputs(shapes, ids);
    let out = difference_material_rings(&input, ConstructLimits::default()).unwrap();
    assert_eq!(
        out,
        difference_material_rings(&input, ConstructLimits::default()).unwrap()
    );
    assert_eq!(area(&out), expected_area);
    for x in -3..27 {
        for y in -3..27 {
            let query = [x * 2 + 1, y * 2 + 1];
            let mut groups = std::collections::BTreeMap::<u64, bool>::new();
            for (shape, &id) in shapes.iter().zip(ids) {
                *groups.entry(id).or_default() ^=
                    inside_polygon(shape.iter().map(|p| p.map(i128::from)), query);
            }
            let expected = groups.get(&0).copied().unwrap_or(false)
                && !groups.iter().any(|(&id, &inside)| id != 0 && inside);
            let mut actual = false;
            for id in 0..out.rings().len() {
                let edges = out.ring_edges(id).unwrap();
                actual ^= inside_polygon(
                    edges
                        .iter()
                        .map(|&id| point(out.vertices()[out.edges()[id].endpoints[0]])),
                    query,
                );
            }
            assert_eq!(actual, expected, "sample {query:?}");
        }
    }
    out
}

#[test]
fn overlapping_cutters_are_unioned_instead_of_xored() {
    let out = verify(
        &[
            rectangle(0, 0, 10, 10),
            rectangle(2, 2, 4, 4),
            rectangle(4, 2, 4, 4),
        ],
        &[0, 1, 2],
        76,
    );
    assert_eq!((out.regions().len(), out.rings().len()), (1, 2));
}
#[test]
fn hollow_body_parity_and_cutters_crossing_outer_and_inner_boundaries() {
    let out = verify(
        &[
            rectangle(0, 0, 10, 10),
            rectangle(2, 2, 6, 6),
            rectangle(-1, 4, 5, 2),
        ],
        &[0, 0, 1],
        60,
    );
    assert_eq!((out.regions().len(), out.rings().len()), (1, 1));
}
#[test]
fn cutter_misses_edge_contacts_vertex_contacts_and_coincident_boundary() {
    for cutter in [
        rectangle(12, 2, 2, 2),
        rectangle(10, 2, 2, 2),
        rectangle(10, 10, 2, 2),
    ] {
        verify(&[rectangle(0, 0, 10, 10), cutter], &[0, 1], 100);
    }
    assert_eq!(
        verify(
            &[rectangle(0, 0, 10, 10), rectangle(0, 0, 10, 10)],
            &[0, 1],
            0
        )
        .regions(),
        []
    );
}
#[test]
fn empty_body_and_same_source_cutter_parity_are_explicit() {
    verify(&[rectangle(0, 0, 10, 10)], &[1], 0);
    verify(&[], &[], 0);
    verify(
        &[
            rectangle(0, 0, 10, 10),
            rectangle(2, 2, 4, 4),
            rectangle(4, 2, 4, 4),
        ],
        &[0, 1, 1],
        84,
    );
}
#[test]
fn represented_fractional_coordinates_keep_exact_scale() {
    let body = [[0.25, 0.25], [10.25, 0.25], [10.25, 10.25], [0.25, 10.25]];
    let cutter = [[2.25, 2.25], [4.25, 2.25], [4.25, 4.25], [2.25, 4.25]];
    let input = [
        RepresentedMaterialRing {
            vertices: &body,
            material: 0,
            source_path: 0,
        },
        RepresentedMaterialRing {
            vertices: &cutter,
            material: 1,
            source_path: 1,
        },
    ];
    let out = difference_represented_material_rings(&input, ConstructLimits::default()).unwrap();
    assert_eq!(out.dyadic_exponent_mm(), Some(-2));
    assert_eq!(area(&out), 96 * 16);
    assert_eq!(out.rings().len(), 2);
}
#[test]
fn cancellation_and_shared_budgets_reject_atomically() {
    let shape = [rectangle(0, 0, 10, 10)];
    let input = inputs(&shape, &[0]);
    for checkpoint in [
        Checkpoint::ConstructStart,
        Checkpoint::ConstructClassify,
        Checkpoint::ConstructComplete,
    ] {
        let mut predicate = |at| at == checkpoint;
        assert!(matches!(
            difference_material_rings_with_cancellation(
                &input,
                ConstructLimits::default(),
                &mut Cancellation::new(&mut predicate)
            ),
            Err(ConstructError::Cancelled(_))
        ));
    }
    let limits = ConstructLimits {
        max_work: 0,
        ..ConstructLimits::default()
    };
    assert!(difference_material_rings(&input, limits).is_err());
    let body = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let input = [RepresentedMaterialRing {
        vertices: &body,
        material: 0,
        source_path: 0,
    }];
    assert!(difference_represented_material_rings(&input, limits).is_err());
    let mut predicate = |at| at == Checkpoint::ConstructComplete;
    assert!(matches!(
        difference_represented_material_rings_with_cancellation(
            &input,
            ConstructLimits::default(),
            &mut Cancellation::new(&mut predicate)
        ),
        Err(ConstructError::Cancelled(_))
    ));
}

#[test]
fn crossing_cutters_split_body_and_source_order_or_winding_do_not_change_occupancy() {
    let mut shapes = vec![rectangle(0, 0, 10, 10), rectangle(4, -1, 2, 12)];
    let out = verify(&shapes, &[0, 1], 80);
    assert_eq!(out.regions().len(), 2);
    shapes[0].reverse();
    shapes[1].reverse();
    shapes.reverse();
    verify(&shapes, &[1, 0], 80);
}

#[test]
fn bounded_random_rectangles_match_independent_cell_area_and_occupancy() {
    let mut state = 17_u64;
    let mut next = || {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        i64::try_from((state >> 32) % 10).unwrap()
    };
    for _ in 0..24 {
        let shapes = [
            rectangle(0, 0, 10, 10),
            rectangle(next() - 2, next() - 2, next() + 1, next() + 1),
            rectangle(next() - 2, next() - 2, next() + 1, next() + 1),
        ];
        let expected = (0..10)
            .flat_map(|x| (0..10).map(move |y| [2 * x + 1, 2 * y + 1]))
            .filter(|&q| {
                !shapes[1..]
                    .iter()
                    .any(|ring| inside_polygon(ring.iter().map(|p| p.map(i128::from)), q))
            })
            .count();
        verify(&shapes, &[0, 1, 2], i128::try_from(expected).unwrap());
    }
}
