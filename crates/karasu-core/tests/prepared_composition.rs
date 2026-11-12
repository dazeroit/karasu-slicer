//! Independent analytic area and sample occupancy checks for prepared positive and negative material composition.
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
    let out = prepared_material_rings(&input, ConstructLimits::default()).unwrap();
    assert_eq!(
        out,
        prepared_material_rings(&input, ConstructLimits::default()).unwrap()
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
            let expected = (groups.get(&0).copied().unwrap_or(false)
                && !groups
                    .iter()
                    .any(|(&id, &inside)| id != 0 && id < PREPARED_ADDITIVE_ID_START && inside))
                || groups
                    .iter()
                    .any(|(&id, &inside)| id >= PREPARED_ADDITIVE_ID_START && inside);
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
fn cutters_do_not_erase_additives_and_additive_overlap_is_union() {
    let shapes = [
        rectangle(0, 0, 10, 10),
        rectangle(2, 0, 6, 10),
        rectangle(3, 2, 4, 4),
        rectangle(5, 2, 4, 4),
    ];
    // Body minus cutter: 40. Two overlapping positive supports add 24, four already in body.
    verify(&shapes, &[0, 1, PREPARED_ADDITIVE_ID_START, u64::MAX], 60);
}

#[test]
fn supports_survive_absent_body_and_identical_cutter() {
    let shapes = [rectangle(2, 2, 8, 8), rectangle(2, 2, 8, 8)];
    verify(&shapes, &[1, PREPARED_ADDITIVE_ID_START], 64);
    verify(&shapes[..1], &[PREPARED_ADDITIVE_ID_START], 64);
}

#[test]
fn body_holes_and_per_component_holes_retain_parity() {
    let shapes = [
        rectangle(0, 0, 20, 20),
        rectangle(4, 4, 12, 12),
        rectangle(6, 6, 8, 8),
        rectangle(8, 8, 4, 4),
    ];
    verify(
        &shapes,
        &[0, 0, PREPARED_ADDITIVE_ID_START, PREPARED_ADDITIVE_ID_START],
        304,
    );
}

#[test]
fn edge_and_point_contacts_are_invariant_under_winding_and_input_order() {
    let shapes = [
        rectangle(0, 0, 4, 4),
        rectangle(4, 0, 4, 4),
        rectangle(8, 4, 4, 4),
        rectangle(0, 4, 4, 4),
    ];
    let ids = [0, PREPARED_ADDITIVE_ID_START, u64::MAX, 1];
    let expected = verify(&shapes, &ids, 48);
    let mut reversed = shapes.clone();
    for shape in &mut reversed {
        shape.reverse();
    }
    reversed.reverse();
    let mut reversed_ids = ids;
    reversed_ids.reverse();
    let actual = verify(&reversed, &reversed_ids, 48);
    // Vertex indices and provenance follow caller order; the represented vertex set is invariant.
    let mut expected_points: Vec<_> = expected.vertices().iter().copied().map(point).collect();
    let mut actual_points: Vec<_> = actual.vertices().iter().copied().map(point).collect();
    expected_points.sort_unstable();
    actual_points.sort_unstable();
    assert_eq!(expected_points, actual_points);
    assert_eq!(expected.regions().len(), actual.regions().len());
}

#[test]
fn represented_coordinates_retain_exact_dyadic_scale_and_positive_semantics() {
    let body = [[0., 0.], [5., 0.], [5., 5.], [0., 5.]];
    let cutter = [[1.25, 0.], [3.75, 0.], [3.75, 5.], [1.25, 5.]];
    let support = [[2.5, 1.25], [5., 1.25], [5., 3.75], [2.5, 3.75]];
    let rings = [
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
        RepresentedMaterialRing {
            vertices: &support,
            material: PREPARED_ADDITIVE_ID_START,
            source_path: 2,
        },
    ];
    let out = prepared_represented_material_rings(&rings, ConstructLimits::default()).unwrap();
    assert_eq!(out.dyadic_exponent_mm(), Some(-2));
    // Integer-coordinate constructor on the corresponding exact dyadic encoding is identical.
    let shapes = [
        rectangle(0, 0, 20, 20),
        rectangle(5, 0, 10, 20),
        rectangle(10, 5, 10, 10),
    ];
    let integer = verify(&shapes, &[0, 1, PREPARED_ADDITIVE_ID_START], 250);
    assert_eq!(out.vertices(), integer.vertices());
    assert_eq!(out.edges(), integer.edges());
}

#[test]
fn cancellation_and_independent_limits_publish_no_partial_material() {
    let shapes = [rectangle(0, 0, 4, 4)];
    let rings = inputs(&shapes, &[PREPARED_ADDITIVE_ID_START]);
    let mut stop = |p| p == Checkpoint::ConstructClassify;
    let error = prepared_material_rings_with_cancellation(
        &rings,
        ConstructLimits::default(),
        &mut Cancellation::new(&mut stop),
    )
    .unwrap_err();
    assert!(matches!(error, ConstructError::Cancelled(_)));
    let limits = ConstructLimits {
        max_input_edges: 0,
        ..ConstructLimits::default()
    };
    assert!(prepared_material_rings(&rings, limits).is_err());
    assert_eq!(
        prepared_material_rings(&[], ConstructLimits::default())
            .unwrap()
            .regions(),
        []
    );
}

fn adapter_mesh() -> karasu_core::mesh::TriangleMesh {
    use karasu_core::{geometry::Point3, mesh::TriangleMesh};
    let shapes = [
        rectangle(0, 0, 10, 10),
        rectangle(2, 0, 6, 10),
        rectangle(3, 2, 4, 4),
    ];
    let mut vertices = Vec::new();
    let mut faces = Vec::new();
    for shape in shapes {
        let start = u32::try_from(vertices.len()).unwrap();
        for [x, y] in shape {
            vertices.extend([
                Point3::new(
                    f64::from(i32::try_from(x).unwrap()),
                    f64::from(i32::try_from(y).unwrap()),
                    -1.,
                ),
                Point3::new(
                    f64::from(i32::try_from(x).unwrap()),
                    f64::from(i32::try_from(y).unwrap()),
                    1.,
                ),
            ]);
        }
        for i in 0..4 {
            let a = start + 2 * i;
            let b = start + 2 * ((i + 1) % 4);
            faces.extend([[a, b, b + 1], [a, b + 1, a + 1]]);
        }
    }
    TriangleMesh::try_new(vertices, faces).unwrap()
}

#[test]
fn layer_adapter_both_coordinate_modes_preserve_diagnostics_and_support_material() {
    use karasu_core::slicing::{
        engine::*, grid::BinaryGridSpec, regions::RegionFillRule, schedule::*, SectionTransform,
    };
    let mesh = adapter_mesh();
    let schedule = build_layer_schedule(
        UniformScheduleSpec::try_new(0., 1., ScheduleEnd::Count(1)).unwrap(),
        ScheduleLimits {
            max_layers: 1,
            max_payload_bytes: 1024,
        },
    )
    .unwrap();
    let config = LayerConfig {
        grid: BinaryGridSpec::try_new([0., 0.], 0).unwrap(),
        fill_rule: RegionFillRule::EvenOddStrict,
        stages: LayerStageLimits::default(),
        max_output_payload_bytes: 1 << 24,
    };
    let mut ids = [0; 24];
    ids[8..16].fill(1);
    ids[16..].fill(PREPARED_ADDITIVE_ID_START);
    for represented in [false, true] {
        let make = || {
            let engine = LayerEngine::new(
                &mesh,
                &schedule,
                SectionTransform::Identity,
                config,
                Cancellation::never(),
                None,
            );
            if represented {
                engine.with_represented_coordinates()
            } else {
                engine
            }
        };
        let strict = make().next().unwrap().unwrap();
        let prepared = make()
            .with_prepared_materials(&ids, ConstructLimits::default())
            .next()
            .unwrap()
            .unwrap();
        assert_eq!(strict.raw(), prepared.raw());
        assert_eq!(strict.completeness(), prepared.completeness());
        assert_eq!(strict.region_diagnostics(), prepared.region_diagnostics());
        let direct = compose_layer_prepared(
            &strict,
            &ids,
            ConstructLimits::default(),
            &mut Cancellation::never(),
        )
        .unwrap();
        assert_eq!(prepared.material_union().unwrap(), &direct);
        let exponent = direct.dyadic_exponent_mm().unwrap_or(0);
        assert_eq!(
            f64::from(i32::try_from(area(&direct)).unwrap()) * 2_f64.powi(2 * i32::from(exponent)),
            56.
        );
    }
}
