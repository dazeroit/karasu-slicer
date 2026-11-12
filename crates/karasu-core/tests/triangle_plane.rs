//! Independent incidence, exact dyadic/rational, and constrained metamorphic section oracles.
#![allow(clippy::float_cmp)] // Exact represented-value comparisons are the contract under test.

use karasu_core::geometry::{
    section::{
        intersect_triangle_z, CornerId, EndpointCoincidence, EndpointOrigin, LocalEdge,
        SectionEndpoint, SourceCorner, TriangleAreaObservation, TrianglePlaneError,
        TrianglePlaneEvent, VertexPlaneSide,
    },
    Affine3, Point3, Vector3,
};
use karasu_core::mesh::TriangleMesh;

const PERMUTATIONS: [[usize; 3]; 6] = [
    [0, 1, 2],
    [0, 2, 1],
    [1, 0, 2],
    [1, 2, 0],
    [2, 0, 1],
    [2, 1, 0],
];
const CORNERS: [CornerId; 3] = [CornerId::Corner0, CornerId::Corner1, CornerId::Corner2];
const EDGES: [LocalEdge; 3] = [LocalEdge::Edge01, LocalEdge::Edge12, LocalEdge::Edge20];
const PAIRS: [[usize; 2]; 3] = [[0, 1], [1, 2], [2, 0]];
const ETA: f64 = f64::from_bits(1);

fn bits(point: Point3) -> [u64; 3] {
    point.coordinates().map(f64::to_bits)
}

fn endpoints(event: TrianglePlaneEvent) -> Vec<SectionEndpoint> {
    match event {
        TrianglePlaneEvent::None => vec![],
        TrianglePlaneEvent::TangentPoint { endpoint } => vec![endpoint],
        TrianglePlaneEvent::Segment { endpoints, .. }
        | TrianglePlaneEvent::CoplanarEdge { endpoints, .. } => endpoints.to_vec(),
        TrianglePlaneEvent::CoplanarTriangle { corners } => corners
            .map(|corner| SectionEndpoint {
                position: corner.position,
                origin: EndpointOrigin::Vertex(corner.corner),
            })
            .to_vec(),
    }
}

fn event_kind(event: TrianglePlaneEvent) -> usize {
    match event {
        TrianglePlaneEvent::None => 0,
        TrianglePlaneEvent::TangentPoint { .. } => 1,
        TrianglePlaneEvent::Segment { .. } => 2,
        TrianglePlaneEvent::CoplanarEdge { .. } => 3,
        TrianglePlaneEvent::CoplanarTriangle { .. } => 4,
    }
}

fn origin_key(origin: EndpointOrigin) -> (u8, usize) {
    match origin {
        EndpointOrigin::Vertex(corner) => (0, corner as usize),
        EndpointOrigin::Edge(edge) => (1, edge as usize),
    }
}

fn mapped_edge(edge: LocalEdge, permutation: [usize; 3]) -> LocalEdge {
    let [a, b] = PAIRS[edge as usize].map(|i| permutation[i]);
    let pair = [a.min(b), a.max(b)];
    match pair {
        [0, 1] => LocalEdge::Edge01,
        [1, 2] => LocalEdge::Edge12,
        [0, 2] => LocalEdge::Edge20,
        _ => panic!("invalid permutation edge"),
    }
}

fn mapped_origin(origin: EndpointOrigin, permutation: [usize; 3]) -> EndpointOrigin {
    match origin {
        EndpointOrigin::Vertex(corner) => {
            EndpointOrigin::Vertex(CORNERS[permutation[corner as usize]])
        }
        EndpointOrigin::Edge(edge) => EndpointOrigin::Edge(mapped_edge(edge, permutation)),
    }
}

fn assert_permutations(vertices: [Point3; 3], plane: f64) {
    let baseline = intersect_triangle_z(vertices, plane).unwrap();
    for permutation in PERMUTATIONS {
        let result = intersect_triangle_z(permutation.map(|i| vertices[i]), plane).unwrap();
        assert_eq!(result.sides, permutation.map(|i| baseline.sides[i]));
        assert_eq!(
            result.context.area_observation,
            baseline.context.area_observation
        );
        for (edge, [a, b]) in PAIRS.into_iter().enumerate() {
            assert_eq!(
                result.context.equal_corner_pairs[edge],
                vertices[permutation[a]] == vertices[permutation[b]]
            );
        }
        assert_eq!(event_kind(result.event), event_kind(baseline.event));
        match (baseline.event, result.event) {
            (
                TrianglePlaneEvent::CoplanarEdge {
                    edge: old,
                    coincidence: x,
                    ..
                },
                TrianglePlaneEvent::CoplanarEdge {
                    edge: new,
                    coincidence: y,
                    ..
                },
            ) => {
                assert_eq!(old, mapped_edge(new, permutation));
                assert_eq!(x, y);
            }
            (
                TrianglePlaneEvent::Segment { coincidence: x, .. },
                TrianglePlaneEvent::Segment { coincidence: y, .. },
            ) => assert_eq!(x, y),
            _ => {}
        }
        // Canonical numeric geometry is bit-identical, including ties. Compare remapped source
        // identity as a multiset because local-ID tie ordering can change under relabeling.
        let old = endpoints(baseline.event);
        let new = endpoints(result.event);
        assert_eq!(
            old.iter().map(|p| bits(p.position)).collect::<Vec<_>>(),
            new.iter().map(|p| bits(p.position)).collect::<Vec<_>>()
        );
        let mut old_records = old
            .iter()
            .map(|p| (bits(p.position), origin_key(p.origin)))
            .collect::<Vec<_>>();
        let mut new_records = new
            .iter()
            .map(|p| {
                (
                    bits(p.position),
                    origin_key(mapped_origin(p.origin, permutation)),
                )
            })
            .collect::<Vec<_>>();
        old_records.sort_unstable();
        new_records.sort_unstable();
        assert_eq!(old_records, new_records);
    }
}

#[test]
fn exhaustive_27_words_and_six_permutations_match_incidence_table() {
    use VertexPlaneSide::{Above as A, Below as B, On as O};
    // Independent literal Section 4 oracle: (word, strict crossings, singleton incidences,
    // coplanar edges, event kind). Singleton incidences include duplicate On-corner hits.
    let cases = [
        ([B, B, B], 0, 0, 0, 0),
        ([B, B, O], 0, 2, 0, 1),
        ([B, B, A], 2, 2, 0, 2),
        ([B, O, B], 0, 2, 0, 1),
        ([B, O, O], 0, 2, 1, 3),
        ([B, O, A], 1, 3, 0, 2),
        ([B, A, B], 2, 2, 0, 2),
        ([B, A, O], 1, 3, 0, 2),
        ([B, A, A], 2, 2, 0, 2),
        ([O, B, B], 0, 2, 0, 1),
        ([O, B, O], 0, 2, 1, 3),
        ([O, B, A], 1, 3, 0, 2),
        ([O, O, B], 0, 2, 1, 3),
        ([O, O, O], 0, 0, 3, 4),
        ([O, O, A], 0, 2, 1, 3),
        ([O, A, B], 1, 3, 0, 2),
        ([O, A, O], 0, 2, 1, 3),
        ([O, A, A], 0, 2, 0, 1),
        ([A, B, B], 2, 2, 0, 2),
        ([A, B, O], 1, 3, 0, 2),
        ([A, B, A], 2, 2, 0, 2),
        ([A, O, B], 1, 3, 0, 2),
        ([A, O, O], 0, 2, 1, 3),
        ([A, O, A], 0, 2, 0, 1),
        ([A, A, B], 2, 2, 0, 2),
        ([A, A, O], 0, 2, 0, 1),
        ([A, A, A], 0, 0, 0, 0),
    ];
    let mut totals = [0; 5];
    for (sides, c, p, e, kind) in cases {
        let mut vertices = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(4.0, 0.0, 0.0),
            Point3::new(0.0, 4.0, 0.0),
        ];
        for (vertex, side) in vertices.iter_mut().zip(sides) {
            vertex.z = match side {
                B => -1.0,
                O => 0.0,
                A => 1.0,
            };
        }
        let result = intersect_triangle_z(vertices, 0.0).unwrap();
        assert_eq!(result.sides, sides);
        assert_eq!(event_kind(result.event), kind);
        totals[kind] += 1;
        let mut counts = [0; 3];
        let mut origins = Vec::new();
        for (edge, [a, b]) in EDGES.into_iter().zip(PAIRS) {
            match (sides[a], sides[b]) {
                (B, A) | (A, B) => {
                    counts[0] += 1;
                    counts[1] += 1;
                    origins.push(EndpointOrigin::Edge(edge));
                }
                (O, O) => {
                    counts[2] += 1;
                    if kind == 3 {
                        assert!(
                            matches!(result.event, TrianglePlaneEvent::CoplanarEdge { edge: actual, .. } if actual == edge)
                        );
                    }
                }
                (O, _) | (_, O) => counts[1] += 1,
                _ => {}
            }
        }
        assert_eq!(counts, [c, p, e]);
        origins.extend(
            sides
                .iter()
                .enumerate()
                .filter(|(_, s)| **s == O)
                .map(|(i, _)| EndpointOrigin::Vertex(CORNERS[i])),
        );
        let mut actual = endpoints(result.event)
            .iter()
            .map(|p| origin_key(p.origin))
            .collect::<Vec<_>>();
        let mut expected = origins.into_iter().map(origin_key).collect::<Vec<_>>();
        actual.sort_unstable();
        expected.sort_unstable();
        assert_eq!(actual, expected);
        assert_eq!(result.context.equal_corner_pairs, [false; 3]);
        assert_eq!(
            result.context.area_observation,
            TriangleAreaObservation::NonzeroComputedCross
        );
        assert_permutations(vertices, 0.0);
    }
    assert_eq!(totals, [2, 6, 12, 6, 1]);
}

#[test]
fn proper_crossing_has_exact_analytic_coordinates_and_edge_origins() {
    let result = intersect_triangle_z(
        [
            Point3::new(0.0, 0.0, -1.0),
            Point3::new(4.0, 0.0, 1.0),
            Point3::new(0.0, 4.0, 1.0),
        ],
        0.0,
    )
    .unwrap();
    assert_eq!(
        result.event,
        TrianglePlaneEvent::Segment {
            endpoints: [
                SectionEndpoint {
                    position: Point3::new(0.0, 2.0, 0.0),
                    origin: EndpointOrigin::Edge(LocalEdge::Edge20)
                },
                SectionEndpoint {
                    position: Point3::new(2.0, 0.0, 0.0),
                    origin: EndpointOrigin::Edge(LocalEdge::Edge01)
                },
            ],
            coincidence: EndpointCoincidence::Distinct
        }
    );
}

#[test]
fn exact_on_corner_and_opposite_crossing_preserve_distinct_origins() {
    let vertices = [
        Point3::new(-0.0, 0.0, 0.0),
        Point3::new(4.0, 0.0, -1.0),
        Point3::new(0.0, 4.0, 1.0),
    ];
    let points = endpoints(intersect_triangle_z(vertices, 0.0).unwrap().event);
    assert_eq!(bits(points[0].position), [0; 3]);
    assert_eq!(points[0].origin, EndpointOrigin::Vertex(CornerId::Corner0));
    assert_eq!(points[1].position, Point3::new(2.0, 2.0, 0.0));
    assert_eq!(points[1].origin, EndpointOrigin::Edge(LocalEdge::Edge12));
    assert_permutations(vertices, 0.0);
}

#[test]
fn tangent_and_coplanar_events_reuse_extreme_source_coordinates_without_construction() {
    let source = Point3::new(f64::MAX, -f64::MAX, f64::MAX);
    let tangent = [
        source,
        Point3::new(1.0, 0.0, -f64::MAX),
        Point3::new(0.0, 1.0, -f64::MAX),
    ];
    assert_eq!(
        intersect_triangle_z(tangent, f64::MAX).unwrap().event,
        TrianglePlaneEvent::TangentPoint {
            endpoint: SectionEndpoint {
                position: source,
                origin: EndpointOrigin::Vertex(CornerId::Corner0)
            }
        }
    );
    for z in [-1.0, 1.0] {
        let vertices = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(4.0, 0.0, 0.0),
            Point3::new(0.0, 4.0, z),
        ];
        let result = intersect_triangle_z(vertices, 0.0).unwrap();
        assert!(matches!(
            result.event,
            TrianglePlaneEvent::CoplanarEdge {
                edge: LocalEdge::Edge01,
                coincidence: EndpointCoincidence::Distinct,
                ..
            }
        ));
        assert_permutations(vertices, 0.0);
    }
}

#[test]
fn coplanar_corner_records_sort_coordinates_then_source_id() {
    let vertices = [
        Point3::new(4.0, 0.0, 0.0),
        Point3::new(0.0, 4.0, 0.0),
        Point3::new(0.0, 0.0, -0.0),
    ];
    assert_eq!(
        intersect_triangle_z(vertices, 0.0).unwrap().event,
        TrianglePlaneEvent::CoplanarTriangle {
            corners: [
                SourceCorner {
                    corner: CornerId::Corner2,
                    position: Point3::new(0.0, 0.0, 0.0)
                },
                SourceCorner {
                    corner: CornerId::Corner1,
                    position: vertices[1]
                },
                SourceCorner {
                    corner: CornerId::Corner0,
                    position: vertices[0]
                },
            ]
        }
    );
    assert_permutations(vertices, 0.0);
}

#[test]
fn repeated_positions_preserve_all_local_identities_and_flags() {
    let a = Point3::new(0.0, 0.0, 0.0);
    for (vertices, plane, flags) in [
        ([a; 3], 0.0, [true; 3]),
        ([a; 3], 1.0, [true; 3]),
        (
            [a, a, Point3::new(1.0, 0.0, 0.0)],
            0.0,
            [true, false, false],
        ),
        (
            [a, a, Point3::new(0.0, 0.0, 1.0)],
            0.0,
            [true, false, false],
        ),
        (
            [a, a, Point3::new(0.0, 0.0, 1.0)],
            0.5,
            [true, false, false],
        ),
    ] {
        let result = intersect_triangle_z(vertices, plane).unwrap();
        assert_eq!(result.context.equal_corner_pairs, flags);
        assert_eq!(
            result.context.area_observation,
            TriangleAreaObservation::RepeatedPosition
        );
        assert_permutations(vertices, plane);
    }
    let result = intersect_triangle_z([a, a, Point3::new(0.0, 0.0, 1.0)], 0.0).unwrap();
    assert!(matches!(
        result.event,
        TrianglePlaneEvent::CoplanarEdge {
            coincidence: EndpointCoincidence::CoincidentRepresented,
            ..
        }
    ));
    assert_ne!(
        endpoints(result.event)[0].origin,
        endpoints(result.event)[1].origin
    );
}

#[test]
fn collinear_crossings_keep_segment_incidence_and_both_origins() {
    for vertices in [
        [
            Point3::new(0.0, 0.0, -1.0),
            Point3::new(0.0, 0.0, 1.0),
            Point3::new(0.0, 0.0, 2.0),
        ],
        [
            Point3::new(-2.0, -4.0, -1.0),
            Point3::new(2.0, 4.0, 1.0),
            Point3::new(6.0, 12.0, 3.0),
        ],
        [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 0.0, -1.0),
            Point3::new(0.0, 0.0, 1.0),
        ],
    ] {
        let result = intersect_triangle_z(vertices, 0.0).unwrap();
        assert_eq!(
            result.context.area_observation,
            TriangleAreaObservation::ZeroComputedCross
        );
        assert!(matches!(
            result.event,
            TrianglePlaneEvent::Segment {
                coincidence: EndpointCoincidence::CoincidentRepresented,
                ..
            }
        ));
        let points = endpoints(result.event);
        assert_eq!(bits(points[0].position), bits(points[1].position));
        assert_ne!(points[0].origin, points[1].origin);
        assert_permutations(vertices, 0.0);
    }
}

#[test]
fn coplanar_rank_loss_is_not_an_area_face() {
    let vertices = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
    ];
    let result = intersect_triangle_z(vertices, 0.0).unwrap();
    assert_eq!(
        result.context.area_observation,
        TriangleAreaObservation::ZeroComputedCross
    );
    assert_eq!(endpoints(result.event).len(), 3);
    assert_eq!(event_kind(result.event), 4);
    assert_permutations(vertices, 0.0);
}

#[test]
fn source_area_range_failure_is_context_even_for_empty_or_exact_events() {
    let vertices = [
        Point3::new(-f64::MAX, 0.0, f64::MAX),
        Point3::new(f64::MAX, 0.0, f64::MAX),
        Point3::new(0.0, 1.0, f64::MAX),
    ];
    for (plane, kind) in [(f64::MAX, 4), (-f64::MAX, 0)] {
        let result = intersect_triangle_z(vertices, plane).unwrap();
        assert_eq!(
            result.context.area_observation,
            TriangleAreaObservation::UnavailableRange
        );
        assert_eq!(event_kind(result.event), kind);
        assert_permutations(vertices, plane);
    }
}

#[test]
fn signed_zero_combinations_are_on_normalized_and_never_mutate_inputs() {
    // Every sign assignment to all nine source coordinates and the plane: 1,024 cases.
    for signs in 0..1024 {
        let zero = |bit| if signs & (1 << bit) == 0 { 0.0 } else { -0.0 };
        let vertices =
            std::array::from_fn(|i| Point3::new(zero(i * 3), zero(i * 3 + 1), zero(i * 3 + 2)));
        let before = vertices.map(bits);
        let result = intersect_triangle_z(vertices, zero(9)).unwrap();
        assert_eq!(result.sides, [VertexPlaneSide::On; 3]);
        assert_eq!(result.context.equal_corner_pairs, [true; 3]);
        for point in endpoints(result.event) {
            assert_eq!(bits(point.position), [0; 3]);
        }
        assert_eq!(vertices.map(bits), before);
    }
}

#[test]
fn adjacent_representable_values_classify_without_a_band() {
    let lower = f64::from_bits(1.0_f64.to_bits() - 1);
    let upper = f64::from_bits(1.0_f64.to_bits() + 1);
    let result = intersect_triangle_z(
        [
            Point3::new(0.0, 0.0, lower),
            Point3::new(4.0, 0.0, 1.0),
            Point3::new(0.0, 4.0, upper),
        ],
        1.0,
    )
    .unwrap();
    assert_eq!(
        result.sides,
        [
            VertexPlaneSide::Below,
            VertexPlaneSide::On,
            VertexPlaneSide::Above
        ]
    );
    assert_eq!(event_kind(result.event), 2);
}

#[test]
fn subnormal_plane_distances_and_small_normal_values_succeed() {
    for (lo, plane, hi) in [
        (ETA, 2.0 * ETA, 3.0 * ETA),
        (-ETA, 0.0, ETA),
        (-f64::MIN_POSITIVE, 0.0, f64::MIN_POSITIVE),
    ] {
        let vertices = [
            Point3::new(7.0, -2.0, lo),
            Point3::new(7.0, 2.0, hi),
            Point3::new(9.0, 2.0, hi),
        ];
        let points = endpoints(intersect_triangle_z(vertices, plane).unwrap().event);
        assert_eq!(points[0].position, Point3::new(7.0, 0.0, plane));
        assert_eq!(points[1].position, Point3::new(8.0, 0.0, plane));
        assert_permutations(vertices, plane);
    }
}

#[test]
fn gradual_xy_underflow_obeys_exact_eta_scaled_bound() {
    let vertices = [
        Point3::new(0.0, ETA, -1.0),
        Point3::new(ETA, 0.0, 1.0),
        Point3::new(ETA, 0.0, 2.0),
    ];
    let points = endpoints(intersect_triangle_z(vertices, 0.0).unwrap().event);
    // Edge01 ideal XY=(eta/2,eta/2), not representable. In units eta, the exact error is 1/2;
    // it is <=16 before even adding 64*u*M. No f64 computation drops the subnormal allowance.
    let midpoint = points
        .iter()
        .find(|p| p.origin == EndpointOrigin::Edge(LocalEdge::Edge01))
        .unwrap();
    assert_eq!(bits(midpoint.position), [0, 1, 0]);
    for actual in [midpoint.position.x, midpoint.position.y] {
        let twice_error = (2 * i128::from(actual.to_bits()) - 1).abs();
        assert!(twice_error <= 32);
    }
    assert_permutations(vertices, 0.0);
}

#[test]
fn balanced_opposite_extremes_avoid_denominator_and_coordinate_overflow() {
    let vertices = [
        Point3::new(-f64::MAX, f64::MAX, -f64::MAX),
        Point3::new(f64::MAX, -f64::MAX, f64::MAX),
        Point3::new(f64::MAX, f64::MAX, f64::MAX),
    ];
    let points = endpoints(intersect_triangle_z(vertices, 0.0).unwrap().event);
    assert_eq!(points[0].position, Point3::new(0.0, 0.0, 0.0));
    assert_eq!(points[1].position, Point3::new(0.0, f64::MAX, 0.0));
    assert_permutations(vertices, 0.0);
}

#[test]
fn required_distance_overflow_has_geometric_not_source_edge_precedence() {
    let vertices = [
        Point3::new(1.0, 0.0, -f64::MAX),
        Point3::new(2.0, 0.0, f64::MAX),
        Point3::new(0.0, 0.0, -f64::MAX),
    ];
    for permutation in PERMUTATIONS {
        let error =
            intersect_triangle_z(permutation.map(|i| vertices[i]), f64::MAX / 2.0).unwrap_err();
        let TrianglePlaneError::DistanceOverflow { edge } = error else {
            panic!("{error:?}")
        };
        // Edge12 has the smaller coordinate key even though Edge01 has the smaller local ID.
        assert_eq!(mapped_edge(edge, permutation), LocalEdge::Edge12);
    }
}

#[test]
fn unbalanced_extreme_crossing_rejects_parameter_not_classification() {
    let vertices = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, f64::MAX),
        Point3::new(0.0, 1.0, 0.0),
    ];
    assert_eq!(
        intersect_triangle_z(vertices, ETA),
        Err(TrianglePlaneError::UnrepresentableParameter {
            edge: LocalEdge::Edge01
        })
    );
}

#[test]
fn normal_positive_ratio_and_weight_boundaries_follow_exact_checks() {
    // d_near=1 and d_far=2^1022: ratio and weight round to MIN_POSITIVE, so both are normal.
    // Moving far just above 2^1022 makes the rounded ratio subnormal and must reject.
    let boundary = 2.0_f64.powi(1022);
    for (far, accepted) in [
        (f64::from_bits(boundary.to_bits() - 1), true),
        (boundary, true),
        (f64::from_bits(boundary.to_bits() + 1), false),
    ] {
        let result = intersect_triangle_z(
            [
                Point3::new(0.0, 0.0, -1.0),
                Point3::new(1.0, 0.0, far),
                Point3::new(0.0, 1.0, far),
            ],
            0.0,
        );
        if accepted {
            assert!(result.is_ok());
        } else {
            assert!(matches!(
                result,
                Err(TrianglePlaneError::UnrepresentableParameter { .. })
            ));
        }
    }
    // A positive subnormal r (or zero) is rejected; it is not silently converted to a tangent.
    for near in [ETA, f64::from_bits(f64::MIN_POSITIVE.to_bits() - 1)] {
        assert!(matches!(
            intersect_triangle_z(
                [
                    Point3::new(0.0, 0.0, -near),
                    Point3::new(1.0, 0.0, 1.0),
                    Point3::new(0.0, 1.0, 1.0)
                ],
                0.0
            ),
            Err(TrianglePlaneError::UnrepresentableParameter { .. })
        ));
    }
}

#[test]
fn rounded_endpoint_coincidence_does_not_replace_edge_provenance() {
    let offset = 2.0_f64.powi(52);
    let vertices = [
        Point3::new(offset, offset, -1.0),
        Point3::new(offset + 1.0, offset, 3.0),
        Point3::new(offset, offset + 1.0, 3.0),
    ];
    let result = intersect_triangle_z(vertices, 0.0).unwrap();
    assert!(matches!(
        result.event,
        TrianglePlaneEvent::Segment {
            coincidence: EndpointCoincidence::CoincidentRepresented,
            ..
        }
    ));
    for point in endpoints(result.event) {
        assert_eq!(point.position, Point3::new(offset, offset, 0.0));
        assert!(matches!(point.origin, EndpointOrigin::Edge(_)));
    }
    assert_permutations(vertices, 0.0);
}

#[test]
fn every_nonfinite_input_reports_exact_location_and_validation_precedence() {
    let base = [
        Point3::new(0.0, 0.0, -1.0),
        Point3::new(1.0, 0.0, 1.0),
        Point3::new(0.0, 1.0, 1.0),
    ];
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for corner in 0..3 {
            for axis in 0..3 {
                let mut vertices = base;
                let mut coordinates = vertices[corner].coordinates();
                coordinates[axis] = value;
                vertices[corner] = Point3::new(coordinates[0], coordinates[1], coordinates[2]);
                let before = vertices.map(bits);
                assert_eq!(
                    intersect_triangle_z(vertices, value),
                    Err(TrianglePlaneError::NonFiniteVertex {
                        corner: CORNERS[corner],
                        axis
                    })
                );
                assert_eq!(vertices.map(bits), before);
            }
        }
        assert_eq!(
            intersect_triangle_z(base, value),
            Err(TrianglePlaneError::NonFinitePlane)
        );
    }
    let vertices = [
        Point3::new(0.0, f64::NAN, f64::INFINITY),
        Point3::new(f64::NAN, 0.0, 0.0),
        base[2],
    ];
    assert_eq!(
        intersect_triangle_z(vertices, f64::NAN),
        Err(TrianglePlaneError::NonFiniteVertex {
            corner: CornerId::Corner0,
            axis: 1
        })
    );
}

// An independent exact dyadic decoder for the bounded normal-number rational sweep below.
// All products/shifts are deliberately small enough for i128 (asserted by checked arithmetic).
fn exact_dyadic(value: f64) -> (i128, i128) {
    if value == 0.0 {
        return (0, 1);
    }
    let bits = value.to_bits();
    let exponent = i32::try_from((bits >> 52) & 0x7ff).unwrap() - 1023 - 52;
    let significand = i128::from((bits & ((1_u64 << 52) - 1)) | (1_u64 << 52));
    let sign = if value < 0.0 { -1 } else { 1 };
    if exponent >= 0 {
        (
            sign * significand.checked_shl(exponent.unsigned_abs()).unwrap(),
            1,
        )
    } else {
        (
            sign * significand,
            1_i128.checked_shl(exponent.unsigned_abs()).unwrap(),
        )
    }
}

fn assert_rational_bound(actual: f64, numerator: i128, denominator: i128, magnitude: i32) {
    let (actual_n, actual_d) = exact_dyadic(actual);
    let error_n = actual_n
        .checked_mul(denominator)
        .unwrap()
        .checked_sub(numerator.checked_mul(actual_d).unwrap())
        .unwrap()
        .abs();
    // 64*u*M = M/2^47. Proving this stricter normal-term bound also proves the full bound;
    // the separate eta-scaled test above retains the underflow allowance where it matters.
    let left = error_n.checked_mul(1_i128 << 47).unwrap();
    let right = i128::from(magnitude)
        .checked_mul(actual_d)
        .unwrap()
        .checked_mul(denominator)
        .unwrap();
    assert!(
        left <= right,
        "{actual} vs {numerator}/{denominator}, M={magnitude}"
    );
    // For these bounded nonzero ideals, constrain ULP error using local binary64 spacing.
    if numerator != 0 {
        #[allow(clippy::cast_precision_loss)] // Numerators/denominators are bounded small integers.
        let expected = numerator as f64 / denominator as f64;
        let spacing = (expected.abs().next_up() - expected.abs()).abs();
        let bound = 64.0 * (f64::EPSILON / 2.0) * f64::from(magnitude);
        assert!((actual - expected).abs() / spacing <= bound / spacing + 1.0);
    }
}

#[test]
fn bounded_integer_rational_oracle_checks_inexact_crossings_and_containment() {
    // Direct integer line-plane equations are an oracle, not the production small-weight formula.
    // 16*16*9 = 2,304 triangles, two independent rational endpoints each.
    for below in 1..=16 {
        for above in 1..=16 {
            for shift in -4..=4 {
                let vertices = [
                    Point3::new(
                        f64::from(shift - 7),
                        f64::from(shift + 3),
                        -f64::from(below),
                    ),
                    Point3::new(
                        f64::from(shift + 11),
                        f64::from(shift - 9),
                        f64::from(above),
                    ),
                    Point3::new(
                        f64::from(shift - 5),
                        f64::from(shift + 13),
                        f64::from(above),
                    ),
                ];
                let result = intersect_triangle_z(vertices, 0.0).unwrap();
                for point in endpoints(result.event) {
                    let EndpointOrigin::Edge(edge) = point.origin else {
                        panic!("expected strict source edge")
                    };
                    let other = if edge == LocalEdge::Edge01 {
                        1
                    } else {
                        assert_eq!(edge, LocalEdge::Edge20);
                        2
                    };
                    for axis in 0..2 {
                        let a = vertices[0].coordinates()[axis];
                        let b = vertices[other].coordinates()[axis];
                        #[allow(clippy::cast_possible_truncation)]
                        // Inputs are exactly represented integers in [-13,17].
                        let numerator =
                            i128::from(above) * (a as i128) + i128::from(below) * (b as i128);
                        let denominator = i128::from(above + below);
                        let magnitude = 32; // Conservative endpoint-axis maximum for this bounded grid.
                                            // Use the actual M required by the contract, not the conservative grid cap.
                        #[allow(clippy::cast_possible_truncation)]
                        let actual_m = a.abs().max(b.abs()) as i32;
                        assert!(actual_m <= magnitude);
                        let actual = point.position.coordinates()[axis];
                        assert_rational_bound(actual, numerator, denominator, actual_m);
                        assert!(actual >= a.min(b) && actual <= a.max(b));
                    }
                    assert_eq!(point.position.z.to_bits(), 0);
                }
            }
        }
    }
}

#[test]
fn lossless_dyadic_scaling_and_translation_have_exact_metamorphic_oracles() {
    let vertices = [
        Point3::new(0.0, 0.0, -1.0),
        Point3::new(4.0, 0.0, 1.0),
        Point3::new(0.0, 4.0, 1.0),
    ];
    let baseline = endpoints(intersect_triangle_z(vertices, 0.0).unwrap().event);
    for exponent in -20..=20 {
        let scale = 2.0_f64.powi(exponent);
        let translation = Point3::new(8.0 * scale, -4.0 * scale, 2.0 * scale);
        let map = |p: Point3| {
            Point3::new(
                p.x * scale + translation.x,
                p.y * scale + translation.y,
                p.z * scale + translation.z,
            )
        };
        let result = endpoints(
            intersect_triangle_z(vertices.map(map), translation.z)
                .unwrap()
                .event,
        );
        for (actual, old) in result.into_iter().zip(&baseline) {
            assert_eq!(bits(actual.position), bits(map(old.position)));
            assert_eq!(actual.origin, old.origin);
        }
    }
}

#[test]
fn repeated_success_and_failure_leave_source_mesh_and_transforms_unchanged() {
    let vertices = [
        Point3::new(-0.0, 0.0, -1.0),
        Point3::new(4.0, 0.0, 1.0),
        Point3::new(0.0, 4.0, 1.0),
    ];
    let before = vertices.map(bits);
    let mesh = TriangleMesh::try_new(vertices.to_vec(), vec![[0, 1, 2]]).unwrap();
    let source =
        TriangleMesh::try_new(mesh.vertices().to_vec(), mesh.triangles().to_vec()).unwrap();
    let transform = Affine3::from_translation(Vector3::new(2.0, 3.0, 4.0)).unwrap();
    let transformed = vertices.map(|p| transform.transform_point(p).unwrap());
    let expected = intersect_triangle_z(transformed, 4.0).unwrap();
    for _ in 0..100 {
        let actual = intersect_triangle_z(transformed, 4.0).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(
            endpoints(actual.event)
                .iter()
                .map(|p| bits(p.position))
                .collect::<Vec<_>>(),
            endpoints(expected.event)
                .iter()
                .map(|p| bits(p.position))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            intersect_triangle_z(vertices, f64::NAN),
            Err(TrianglePlaneError::NonFinitePlane)
        );
    }
    assert_eq!(vertices.map(bits), before);
    assert_eq!(mesh, source);
    assert_eq!(transform.translation(), Vector3::new(2.0, 3.0, 4.0));
}

#[test]
fn errors_are_structured_documented_and_do_not_leak_coordinates() {
    let errors = [
        TrianglePlaneError::NonFiniteVertex {
            corner: CornerId::Corner2,
            axis: 1,
        },
        TrianglePlaneError::NonFinitePlane,
        TrianglePlaneError::DistanceOverflow {
            edge: LocalEdge::Edge20,
        },
        TrianglePlaneError::UnrepresentableParameter {
            edge: LocalEdge::Edge12,
        },
        TrianglePlaneError::CoordinateOverflow {
            edge: LocalEdge::Edge01,
            axis: 0,
        },
    ];
    for error in errors {
        let text = error.to_string();
        assert_ne!(text, "");
        assert!(text.contains("section"));
        assert!(std::error::Error::source(&error).is_none());
    }
}

#[test]
fn inexact_collinear_constructions_are_not_required_to_coincide() {
    let vertices = [
        Point3::new(-2.0, -4.0, -1.0),
        Point3::new(2.0, 4.0, 1.0),
        Point3::new(4.0, 8.0, 2.0),
    ];
    let result = intersect_triangle_z(vertices, 0.0).unwrap();
    assert_eq!(
        result.context.area_observation,
        TriangleAreaObservation::ZeroComputedCross
    );
    // Both ideal endpoints are exactly (0,0,0); one edge uses an inexact third weight.
    // The contract observes represented coincidence, not exact collinearity of constructions.
    assert!(matches!(
        result.event,
        TrianglePlaneEvent::Segment {
            coincidence: EndpointCoincidence::Distinct,
            ..
        }
    ));
    for point in endpoints(result.event) {
        assert_rational_bound(point.position.x, 0, 1, 4);
        assert_rational_bound(point.position.y, 0, 1, 8);
    }
    assert_permutations(vertices, 0.0);
}

#[test]
fn independent_rational_bounds_hold_across_normal_binades() {
    // Exact power-of-two scaling changes the magnitude, not the integer line-plane oracle.
    // Here every nonzero output and rescaling stays normal; division by scale is lossless.
    for exponent in [-1000, -500, -20, 0, 200, 900, 1000] {
        let scale = 2.0_f64.powi(exponent);
        for below in 1..=11 {
            for above in 1..=11 {
                let vertices = [
                    Point3::new(-7.0 * scale, 3.0 * scale, -f64::from(below)),
                    Point3::new(11.0 * scale, -9.0 * scale, f64::from(above)),
                    Point3::new(-5.0 * scale, 13.0 * scale, f64::from(above)),
                ];
                for point in endpoints(intersect_triangle_z(vertices, 0.0).unwrap().event) {
                    let EndpointOrigin::Edge(edge) = point.origin else {
                        panic!("strict crossing expected")
                    };
                    let (x, y, mx, my) = match edge {
                        LocalEdge::Edge01 => (11, -9, 11, 9),
                        LocalEdge::Edge20 => (-5, 13, 7, 13),
                        LocalEdge::Edge12 => panic!("unexpected edge"),
                    };
                    assert_rational_bound(
                        point.position.x / scale,
                        i128::from(-7 * above + x * below),
                        i128::from(above + below),
                        mx,
                    );
                    assert_rational_bound(
                        point.position.y / scale,
                        i128::from(3 * above + y * below),
                        i128::from(above + below),
                        my,
                    );
                }
            }
        }
    }
}

#[test]
fn equal_coordinate_edge_origins_sort_by_source_geometry_before_local_id() {
    let vertices = [
        Point3::new(0.0, 0.0, -1.0),
        Point3::new(0.0, 0.0, 2.0),
        Point3::new(0.0, 0.0, 1.0),
    ];
    let points = endpoints(intersect_triangle_z(vertices, 0.0).unwrap().event);
    assert_eq!(points[0].origin, EndpointOrigin::Edge(LocalEdge::Edge20));
    assert_eq!(points[1].origin, EndpointOrigin::Edge(LocalEdge::Edge01));
    assert_permutations(vertices, 0.0);
}

#[test]
fn strict_crossings_at_both_zero_planes_normalize_all_output_zeros() {
    for plane in [-0.0, 0.0] {
        for zero in [-0.0, 0.0] {
            let vertices = [
                Point3::new(zero, zero, -1.0),
                Point3::new(zero, zero, 1.0),
                Point3::new(2.0, zero, 1.0),
            ];
            let result = intersect_triangle_z(vertices, plane).unwrap();
            for point in endpoints(result.event) {
                assert_eq!(point.position.y.to_bits(), 0);
                assert_eq!(point.position.z.to_bits(), 0);
                if point.position.x == 0.0 {
                    assert_eq!(point.position.x.to_bits(), 0);
                }
            }
            assert_permutations(vertices, plane);
        }
    }
}

#[test]
fn inexact_ordinary_crossings_are_bit_identical_under_relabeling() {
    for below in 1..=7 {
        for above in 1..=7 {
            let vertices = [
                Point3::new(-7.0, 3.0, -f64::from(below)),
                Point3::new(11.0, -9.0, f64::from(above)),
                Point3::new(-5.0, 13.0, f64::from(above)),
            ];
            assert_permutations(vertices, 0.0);
        }
    }
}
