//! Bounded parametric support admission, contact generation and prepared-layer invariants.
use karasu_core::{
    cancellation::{Cancellation, Checkpoint},
    geometry::{ObjectTransform, Point3, Rotation3, Vector3},
    mesh::TriangleMesh,
    preparation::{
        drainage::DrainageLimits, hollow::HollowLimits, supports::*, HollowSpec, Operation,
        OperationKind, PlanLimits, PreparationPlan, SamplingSpec, SourceBinding,
    },
    slicing::{
        construct::ConstructLimits,
        engine::{LayerConfig, LayerStageLimits},
        grid::BinaryGridSpec,
        regions::RegionFillRule,
        schedule::*,
    },
};
fn byte(data: &[u8], index: usize) -> u8 {
    data.get(index).copied().unwrap_or(0)
}
fn cube() -> TriangleMesh {
    TriangleMesh::try_new(
        [
            [0., 0., 0.],
            [8., 0., 0.],
            [8., 8., 0.],
            [0., 8., 0.],
            [0., 0., 8.],
            [8., 0., 8.],
            [8., 8., 8.],
            [0., 8., 8.],
        ]
        .map(|[x, y, z]| Point3::new(x, y, z))
        .to_vec(),
        vec![
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
        ],
    )
    .unwrap()
}

/// Exercise bounded independent public support-plan and geometry contracts.
/// Invalid parameters, stale anchors, conservative geometric rejection and budgets are expected.
pub fn check_supports(data: &[u8]) -> bool {
    if data.len() > 256 {
        return false;
    }
    let source = cube();
    let vertices = source.vertices().to_vec();
    let faces = source.triangles().to_vec();
    let mode = byte(data, 0) % 16;
    let make_model = |thickness: f64| {
        PreparationPlan::new(
            SourceBinding {
                original_sha256: [93; 32],
                placement: ObjectTransform::new(
                    Vector3::new(0., 0., 4.),
                    Rotation3::identity(),
                    Vector3::new(1., 1., 1.),
                    Point3::new(0., 0., 0.),
                )
                .unwrap(),
            },
            SamplingSpec {
                cell_mm: 0.1,
                distance_error_mm: 0.001,
                max_cells: 1,
            },
            if byte(data, 1) & 1 == 0 {
                vec![]
            } else {
                vec![Operation {
                    id: 1,
                    kind: OperationKind::Hollow(HollowSpec {
                        thickness_mm: thickness,
                    }),
                }]
            },
            PlanLimits::default(),
            &mut Cancellation::never(),
        )
        .unwrap()
    };
    let model = make_model(1.);
    let upstream = model.canonical_record().to_vec();
    let component = HollowLimits {
        max_input_vertices: 64,
        max_input_triangles: 128,
        max_planes: 64,
        max_output_vertices: 128,
        max_output_triangles: 256,
        max_work: 200_000,
        max_payload_bytes: 2 * 1024 * 1024,
    };
    let limits = SupportLimits {
        drainage: DrainageLimits {
            hollow: component,
            max_holes: 2,
            max_segments: 32,
            max_work: 100_000,
            max_payload_bytes: 4 * 1024 * 1024,
            minimum_clearance_mm: 1e-6,
        },
        max_supports: 2,
        max_stations: 5,
        max_profile_vertices: 16,
        component,
        max_work: if mode == 14 { 2_000_000 } else { 1_200_000 },
        max_payload_bytes: if mode == 14 { 24 } else { 16 } * 1024 * 1024,
        clearance_mm: 1e-6,
        contact_tolerance_mm: 1e-5,
    };
    let mut recipe = SupportRecipe {
        id: 1,
        surface: model.model_key(),
        contact: SupportContact {
            source_triangle: 0,
            position_mm: [5., 3., 4.],
            maximum_penetration_mm: 0.5,
        },
        stations: [(0., 1.), (0.5, 1.), (3.5, 0.4), (4.25, 0.2)]
            .into_iter()
            .map(|(z, size)| SupportStation {
                center_mm: [5., 3., z],
                rotation: Rotation3::identity(),
                profile: SupportProfile::Rectangle {
                    size_mm: [size, size],
                },
            })
            .collect(),
    };
    match mode {
        1 => recipe.stations[3].profile = SupportProfile::Point,
        2 => recipe.stations[1].center_mm[0] += f64::from(byte(data, 2) % 5) / 8.,
        3 => {
            let angle = f64::from(byte(data, 2)) / 64.;
            recipe.stations[2].rotation = match byte(data, 4) % 3 {
                0 => Rotation3::around_x_radians(angle),
                1 => Rotation3::around_y_radians(angle),
                _ => Rotation3::around_z_radians(angle),
            }
            .unwrap();
        }
        4 => {
            recipe.stations[1].profile = SupportProfile::EllipsePolygon {
                radii_mm: [0.5, 0.3],
                sides: 8,
            }
        }
        5 => {
            recipe.stations[1].profile = SupportProfile::ConvexPolygon {
                vertices_mm: vec![[-0.5, -0.5], [0.5, -0.5], [0., 0.5]],
            }
        }
        6 => recipe.stations[0].center_mm[0] = f64::NAN,
        7 => recipe.stations[1].center_mm[2] = 0.,
        8 => recipe.stations[0].profile = SupportProfile::Point,
        9 => {
            recipe.stations[3].center_mm[2] = 5.5;
        }
        10 => recipe.contact.source_triangle = 99,
        11 => {
            let other = make_model(2.);
            if byte(data, 1) & 1 != 0 {
                recipe.surface = other.model_key();
            } else {
                recipe.id = 0;
            }
        }
        12 => {
            recipe.stations[1].profile = SupportProfile::RegularPolygon {
                radius_mm: 0.5,
                sides: 32,
            }
        }
        13 => recipe.contact.position_mm[2] = 4.5,
        _ => {}
    }
    let mut recipes = vec![recipe];
    if mode == 14 {
        let mut second = recipes[0].clone();
        second.id = 2;
        recipes.push(second);
    }
    let policy = if byte(data, 3) & 1 == 0 {
        SupportIntersectionPolicy::RejectUncertified
    } else {
        SupportIntersectionPolicy::Union
    };
    let mut stop = |p| mode == 15 && p == Checkpoint::SupportPublish;
    let plan = SupportPlan::new(
        model.clone(),
        recipes,
        policy,
        limits,
        &mut Cancellation::never(),
    );
    assert_eq!(model.canonical_record(), upstream);
    let Ok(plan) = plan else {
        return false;
    };
    let record = plan.canonical_record().to_vec();
    let result = generate_supports(&source, &plan, limits, &mut Cancellation::new(&mut stop));
    assert_eq!(source.vertices(), vertices);
    assert_eq!(source.triangles(), faces);
    assert_eq!(plan.canonical_record(), record);
    let Ok(scene) = result else {
        return false;
    };
    assert_eq!(scene.plan_record(), record);
    assert_eq!(scene.certificates().len(), plan.recipes().len());
    for c in scene.certificates() {
        assert!(c.minimum_penetration_mm.is_finite() && c.minimum_penetration_mm > 0.);
        assert!(c.maximum_penetration_mm.is_finite() && c.maximum_penetration_mm <= 0.5);
        assert!(c.clearance_mm.is_finite() && c.clearance_mm > 0.);
    }
    let schedule = build_layer_schedule(
        UniformScheduleSpec::try_new(0.25, 1., ScheduleEnd::Count(1)).unwrap(),
        ScheduleLimits {
            max_layers: 1,
            max_payload_bytes: 1024,
        },
    )
    .unwrap();
    let config = LayerConfig {
        grid: BinaryGridSpec::try_new([0.; 2], 0).unwrap(),
        fill_rule: RegionFillRule::EvenOddStrict,
        stages: LayerStageLimits::default(),
        max_output_payload_bytes: 4 * 1024 * 1024,
    };
    let construct = ConstructLimits {
        max_input_edges: 256,
        max_pairs: 40_000,
        max_vertices: 1024,
        max_atoms: 2048,
        max_work: 200_000,
        max_payload_bytes: 4 * 1024 * 1024,
    };
    let mut layers = scene.layer_engine(&schedule, config, construct, Cancellation::never(), None);
    if let Some(Ok(layer)) = layers.next() {
        let material = layer.material_union().unwrap();
        assert!(!material.regions().is_empty());
    }
    assert!(layers.next().is_none());
    true
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_acceptance_and_failure_families() {
        for hollow in [0, 1] {
            for mode in 0..=5 {
                assert!(
                    check_supports(&[mode, hollow]),
                    "mode={mode} hollow={hollow}"
                );
            }
            for mode in 6..=15 {
                assert!(
                    !check_supports(&[mode, hollow]),
                    "mode={mode} hollow={hollow}"
                );
            }
        }
        assert!(check_supports(&[14, 0, 0, 1]));
    }
}
