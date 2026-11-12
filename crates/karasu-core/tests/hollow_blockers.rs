//! Hollow blockers (owner, 2026-11-02): regions the sampled hollowing leaves solid.
//!
//! A cube of side 20 hollowed at 2 mm has the cavity `[2, 18]³` (4096 mm³). Each test blocks a
//! region of known volume and checks the cavity's volume and surfaces against the closed form,
//! and that the wall stays certified: blockers only take cavity away.

use karasu_core::{
    cancellation::Cancellation,
    geometry::{ObjectTransform, Point3, Rotation3, Vector3},
    mesh::TriangleMesh,
    preparation::{
        hollow::HollowError,
        sampled::{hollow_mesh_sampled, SampledHollowLimits, SampledHollowedModel},
        HollowBlocker, HollowSpec, Operation, OperationKind, PlanLimits, PreparationError,
        PreparationPlan, SamplingSpec, SourceBinding,
    },
};

const AXES: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// An axis-aligned cube `[0, side]³` with outward faces.
fn cube(side: f64) -> TriangleMesh {
    let points = (0..8)
        .map(|i| {
            let at = |bit: u32| if i & bit == 0 { 0.0 } else { side };
            Point3::new(at(1), at(2), at(4))
        })
        .collect();
    let quads = [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ];
    let triangles = quads
        .iter()
        .flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]])
        .collect();
    TriangleMesh::try_new(points, triangles).unwrap()
}

/// The plan hollowing at 2 mm on a 0.25 mm grid with `blockers`, the source scaled by `scale`.
fn plan(blockers: Vec<HollowBlocker>, scale: f64) -> Result<PreparationPlan, PreparationError> {
    PreparationPlan::new(
        SourceBinding {
            original_sha256: [7; 32],
            placement: ObjectTransform::new(
                Vector3::new(0.0, 0.0, 0.0),
                Rotation3::identity(),
                Vector3::new(scale, scale, scale),
                Point3::new(0.0, 0.0, 0.0),
            )
            .unwrap(),
        },
        SamplingSpec {
            cell_mm: 0.25,
            distance_error_mm: 1e-6,
            max_cells: u64::MAX,
        },
        vec![Operation {
            id: 1,
            kind: OperationKind::Hollow(HollowSpec {
                thickness_mm: 2.0,
                blockers,
                guarantee: None,
            }),
        }],
        PlanLimits::default(),
        &mut Cancellation::never(),
    )
}

fn hollow(source: &TriangleMesh, blockers: Vec<HollowBlocker>, scale: f64) -> SampledHollowedModel {
    hollow_mesh_sampled(
        source,
        &plan(blockers, scale).unwrap(),
        SampledHollowLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap()
}

/// A box blocker: `size` along the axes, centred at `centre`.
fn block(centre: [f64; 3], size: [f64; 3]) -> HollowBlocker {
    HollowBlocker::Frustum {
        centre,
        axes: AXES,
        bottom_mm: [size[0], size[1]],
        top_mm: [size[0], size[1]],
        height_mm: size[2],
    }
}

/// The cavity's volume is within `share` of `expected`, and its wall is still certified.
fn assert_cavity(model: &SampledHollowedModel, expected: f64, share: f64, surfaces: usize) {
    let c = model.certificate();
    let relative = (c.cavity_volume_mm3 - expected).abs() / expected;
    assert!(relative < share, "expected {expected}: {c:?}");
    assert_eq!(c.cavity_surface_components, surfaces, "{c:?}");
    let [lo, _] = c.surface_distance_mm;
    assert!(lo >= 2.0 - c.band_mm, "{c:?}");
}

#[test]
fn a_box_blocker_keeps_its_half_of_the_cavity_solid() {
    // Everything with x < 10 kept solid: the cavity is [10, 18] × [2, 18]².
    let model = hollow(
        &cube(20.0),
        vec![block([5.0, 10.0, 10.0], [10.0, 30.0, 30.0])],
        1.0,
    );
    assert_cavity(&model, 8.0 * 16.0 * 16.0, 0.01, 1);
}

#[test]
fn a_slab_splits_the_cavity_and_a_sphere_leaves_a_solid_ball_in_it() {
    // A slab 2 mm thick across the middle: two cavities of 7 × 16 × 16.
    let model = hollow(
        &cube(20.0),
        vec![block([10.0, 10.0, 10.0], [2.0, 30.0, 30.0])],
        1.0,
    );
    assert_cavity(&model, 2.0 * 7.0 * 16.0 * 16.0, 0.01, 2);
    // A ball of radius 4 in the middle stays solid: the cavity surrounds it.
    let ball = HollowBlocker::Sphere {
        centre: [10.0, 10.0, 10.0],
        radius_mm: 4.0,
    };
    let model = hollow(&cube(20.0), vec![ball], 1.0);
    let expected = 4096.0 - 4.0 / 3.0 * std::f64::consts::PI * 64.0;
    assert_cavity(&model, expected, 0.01, 2);
}

#[test]
fn a_pyramid_frustum_and_blockers_in_the_source_frame_of_a_scaled_model() {
    // A pyramid on the cavity's floor (apex at its ceiling): a third of the cavity solid.
    let pyramid = HollowBlocker::Frustum {
        centre: [10.0, 10.0, 10.0],
        axes: AXES,
        bottom_mm: [16.0, 16.0],
        top_mm: [0.0, 0.0],
        height_mm: 16.0,
    };
    let model = hollow(&cube(20.0), vec![pyramid], 1.0);
    assert_cavity(&model, 4096.0 - 256.0 * 16.0 / 3.0, 0.02, 1);
    // A cube of side 10 placed at twice its size: a blocker given in its own frame (x < 5)
    // keeps the same half solid as the first test.
    let model = hollow(
        &cube(10.0),
        vec![block([2.5, 5.0, 5.0], [5.0, 15.0, 15.0])],
        2.0,
    );
    assert_cavity(&model, 8.0 * 16.0 * 16.0, 0.01, 1);
}

#[test]
fn a_blocker_over_the_whole_cavity_leaves_none_and_bad_blockers_are_refused() {
    // Nothing left to hollow is said, as for a model too thin to hollow.
    let whole = hollow_mesh_sampled(
        &cube(20.0),
        &plan(vec![block([10.0, 10.0, 10.0], [30.0; 3])], 1.0).unwrap(),
        SampledHollowLimits::default(),
        &mut Cancellation::never(),
    );
    assert!(
        matches!(whole, Err(HollowError::Unsupported { .. })),
        "{whole:?}"
    );
    let skewed = HollowBlocker::Frustum {
        centre: [10.0; 3],
        axes: [[1.0, 0.0, 0.0], [0.5, 1.0, 0.0], [0.0, 0.0, 1.0]],
        bottom_mm: [4.0; 2],
        top_mm: [4.0; 2],
        height_mm: 4.0,
    };
    assert!(plan(vec![skewed], 1.0).is_err());
    let flat = HollowBlocker::Sphere {
        centre: [10.0; 3],
        radius_mm: 0.0,
    };
    assert!(plan(vec![flat], 1.0).is_err());
    // The plan's record tells hollowings with different blockers apart.
    let a = plan(vec![block([5.0, 10.0, 10.0], [10.0, 30.0, 30.0])], 1.0).unwrap();
    let b = plan(vec![block([6.0, 10.0, 10.0], [10.0, 30.0, 30.0])], 1.0).unwrap();
    assert_ne!(a.canonical_record(), b.canonical_record());
}
