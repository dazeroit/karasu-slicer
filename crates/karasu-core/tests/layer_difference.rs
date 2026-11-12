//! Explicit material-source policy, unchanged strict reporting and atomic publication.
use karasu_core::{
    cancellation::{Cancellation, Checkpoint},
    geometry::Point3,
    mesh::TriangleMesh,
    slicing::{
        construct::ConstructLimits, engine::*, grid::BinaryGridSpec, regions::RegionFillRule,
        schedule::*, SectionTransform,
    },
};

fn ribbons() -> TriangleMesh {
    let points = [
        [0., 0.],
        [4., 0.],
        [4., 4.],
        [0., 4.],
        [2., 2.],
        [6., 2.],
        [6., 6.],
        [2., 6.],
    ];
    let vertices = points
        .iter()
        .flat_map(|&[x, y]| [Point3::new(x, y, -1.), Point3::new(x, y, 1.)])
        .collect();
    let faces = [
        [0, 1],
        [1, 2],
        [2, 3],
        [3, 0],
        [4, 5],
        [5, 6],
        [6, 7],
        [7, 4],
    ]
    .into_iter()
    .flat_map(|[a, b]| [[2 * a, 2 * b, 2 * b + 1], [2 * a, 2 * b + 1, 2 * a + 1]])
    .collect();
    TriangleMesh::try_new(vertices, faces).unwrap()
}
fn plan() -> LayerSchedule {
    build_layer_schedule(
        UniformScheduleSpec::try_new(0., 1., ScheduleEnd::Count(1)).unwrap(),
        ScheduleLimits {
            max_layers: 1,
            max_payload_bytes: 1024,
        },
    )
    .unwrap()
}
fn config() -> LayerConfig {
    LayerConfig {
        grid: BinaryGridSpec::try_new([0., 0.], 0).unwrap(),
        fill_rule: RegionFillRule::EvenOddStrict,
        stages: LayerStageLimits::default(),
        max_output_payload_bytes: 1 << 24,
    }
}
fn engine<'a>(
    mesh: &'a TriangleMesh,
    plan: &'a LayerSchedule,
) -> LayerEngine<'a, 'static, 'static> {
    LayerEngine::new(
        mesh,
        plan,
        SectionTransform::Identity,
        config(),
        Cancellation::never(),
        None,
    )
}

#[test]
fn difference_policy_changes_both_coordinate_modes_without_changing_diagnostics() {
    let mesh = ribbons();
    let plan = plan();
    let mut labels = [0; 16];
    labels[8..].fill(1);
    for represented in [false, true] {
        let make = || {
            let engine = engine(&mesh, &plan);
            if represented {
                engine.with_represented_coordinates()
            } else {
                engine
            }
        };
        let strict = make().next().unwrap().unwrap();
        let union = make()
            .with_material_union(&labels, ConstructLimits::default())
            .next()
            .unwrap()
            .unwrap();
        let difference = make()
            .with_material_difference(&labels, ConstructLimits::default())
            .next()
            .unwrap()
            .unwrap();
        let direct = compose_layer_difference(
            &strict,
            &labels,
            ConstructLimits::default(),
            &mut Cancellation::never(),
        )
        .unwrap();
        assert_eq!(difference.material_union().unwrap(), &direct);
        assert_ne!(union.material_union(), difference.material_union());
        assert_eq!(difference.completeness(), strict.completeness());
        assert_eq!(difference.region_diagnostics(), strict.region_diagnostics());
        assert_eq!(difference.raw(), strict.raw());
        assert_eq!(direct.regions().len(), 1);
        assert_eq!(direct.rings().len(), 1);
        // Independent analytic oracle: two side-four squares overlap in a
        // side-two square, so subtraction retains area 16 - 4 = 12.
        if !represented {
            let coordinate = |point: karasu_core::slicing::construct::ExactPoint| {
                let rational = |(negative, limbs): (bool, [u64; 8])| {
                    assert!(limbs[1..].iter().all(|&v| v == 0));
                    (if negative { -1.0 } else { 1.0 })
                        * f64::from(u32::try_from(limbs[0]).unwrap())
                };
                let denominator = point.denominator();
                assert!(denominator[1..].iter().all(|&v| v == 0));
                [
                    rational(point.numerator_x())
                        / f64::from(u32::try_from(denominator[0]).unwrap()),
                    rational(point.numerator_y())
                        / f64::from(u32::try_from(denominator[0]).unwrap()),
                ]
            };
            let twice_area: f64 = direct
                .edges()
                .iter()
                .map(|edge| {
                    let [a, b] = edge.endpoints.map(|id| coordinate(direct.vertices()[id]));
                    a[0] * b[1] - b[0] * a[1]
                })
                .sum();
            assert_eq!(twice_area, 24.0);
        }
    }
}

#[test]
fn absent_positive_material_yields_empty_difference() {
    let mesh = ribbons();
    let plan = plan();
    let labels = [4; 16];
    for represented in [false, true] {
        let mut source =
            engine(&mesh, &plan).with_material_difference(&labels, ConstructLimits::default());
        if represented {
            source = source.with_represented_coordinates();
        }
        let result = source.next().unwrap().unwrap();
        assert_eq!(result.material_union().unwrap().regions(), []);
        assert_eq!(result.material_union().unwrap().rings(), []);
    }
}

#[test]
fn difference_failure_and_cancellation_are_atomic_fused_and_typed() {
    let mesh = ribbons();
    let plan = plan();
    let mut source = engine(&mesh, &plan).with_material_difference(&[], ConstructLimits::default());
    assert!(matches!(
        source.next().unwrap().unwrap_err().cause,
        LayerFailure::Composition(LayerCompositionError::FaceMaterialCount {
            expected: 16,
            actual: 0
        })
    ));
    assert_eq!(source.completed_layers(), 0);
    assert!(source.next().is_none());
    let labels = [0; 16];
    let mut predicate = |point| point == Checkpoint::ConstructInput;
    let mut source = LayerEngine::new(
        &mesh,
        &plan,
        SectionTransform::Identity,
        config(),
        Cancellation::new(&mut predicate),
        None,
    )
    .with_material_difference(&labels, ConstructLimits::default());
    assert!(matches!(
        source.next().unwrap().unwrap_err().cause,
        LayerFailure::Composition(LayerCompositionError::Cancelled(_))
    ));
    assert_eq!(source.completed_layers(), 0);
    assert!(source.next().is_none());
}
