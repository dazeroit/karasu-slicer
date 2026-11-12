//! Independent analytic solids and atomic failure checks for bounded convex hollowing.
use karasu_core::{
    cancellation::{Cancellation, Checkpoint},
    geometry::{ObjectTransform, Point3, Rotation3, Vector3},
    mesh::TriangleMesh,
    preparation::{
        hollow::{hollow_mesh, HollowError, HollowLimits, HollowedModel, VolumeBounds},
        HoleSpec, HollowSpec, Operation, OperationKind, PlanLimits, PreparationPlan, SamplingSpec,
        SourceBinding, SupportSpec,
    },
};

fn cube(side: f64) -> TriangleMesh {
    let points = [
        [0., 0., 0.],
        [side, 0., 0.],
        [side, side, 0.],
        [0., side, 0.],
        [0., 0., side],
        [side, 0., side],
        [side, side, side],
        [0., side, side],
    ];
    TriangleMesh::try_new(
        points.map(|[x, y, z]| Point3::new(x, y, z)).to_vec(),
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
fn plan(thickness_mm: f64, placement: ObjectTransform) -> PreparationPlan {
    make_plan(
        placement,
        vec![Operation {
            id: 1,
            kind: OperationKind::Hollow(HollowSpec {
                blockers: Vec::new(),
                thickness_mm,
                guarantee: None,
            }),
        }],
    )
}
fn make_plan(placement: ObjectTransform, operations: Vec<Operation>) -> PreparationPlan {
    PreparationPlan::new(
        SourceBinding {
            original_sha256: [73; 32],
            placement,
        },
        SamplingSpec {
            cell_mm: 0.1,
            distance_error_mm: 1e-6,
            max_cells: 1_000_000,
        },
        operations,
        PlanLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap()
}
fn run(mesh: &TriangleMesh, plan: &PreparationPlan) -> Result<HollowedModel, HollowError> {
    hollow_mesh(
        mesh,
        plan,
        HollowLimits::default(),
        &mut Cancellation::never(),
    )
}
fn encloses(bounds: VolumeBounds, value: f64) {
    assert!(
        bounds.minimum_mm3 <= value && value <= bounds.maximum_mm3,
        "analytic {value} outside {bounds:?}"
    );
}
fn volume(mesh: &TriangleMesh) -> f64 {
    mesh.triangles()
        .iter()
        .map(|face| {
            let [a, b, c] = face.map(|v| mesh.vertices()[v as usize].coordinates());
            (a[0] * (b[1] * c[2] - b[2] * c[1])
                + a[1] * (b[2] * c[0] - b[0] * c[2])
                + a[2] * (b[0] * c[1] - b[1] * c[0]))
                / 6.
        })
        .sum()
}
#[test]
fn analytic_cube_preserves_exterior_and_binds_exact_plan() {
    let source = cube(20.);
    let vertices = source.vertices().to_vec();
    let faces = source.triangles().to_vec();
    let p = plan(2., ObjectTransform::identity());
    let output = run(&source, &p).unwrap();
    assert_eq!(source.vertices(), vertices);
    assert_eq!(source.triangles(), faces);
    assert_eq!(&output.mesh().vertices()[..vertices.len()], vertices);
    assert_eq!(&output.mesh().triangles()[..faces.len()], faces);
    let c = output.certificate();
    encloses(c.source_volume, 8000.);
    encloses(c.cavity_volume, 4096.);
    encloses(c.material_volume, 3904.);
    assert!(c.wall_offset_error_mm <= 1e-6);
    assert!((volume(output.cavity()) - 4096.).abs() < 1e-7);
    assert!((volume(output.mesh()) - 3904.).abs() < 1e-7);
    assert_eq!(output.plan_record(), p.canonical_record());
    assert_eq!(output.backend_id(), "convex-halfspaces-v1");
    let topology = output.mesh().analyze_topology().unwrap();
    assert!(topology.summary().closed_combinatorial_surface);
    assert_eq!(topology.summary().edge_components, 2);
    let cavity = output.cavity().analyze_topology().unwrap();
    assert!(cavity.summary().closed_combinatorial_surface);
    assert_eq!(cavity.summary().edge_components, 1);
}
#[test]
fn repeated_evaluation_is_bitwise_deterministic() {
    let source = cube(20.);
    let p = plan(2., ObjectTransform::identity());
    let a = run(&source, &p).unwrap();
    let b = run(&source, &p).unwrap();
    assert_eq!(a.mesh().vertices(), b.mesh().vertices());
    assert_eq!(a.mesh().triangles(), b.mesh().triangles());
    assert_eq!(a.certificate(), b.certificate());
    assert_eq!(a.work_units(), b.work_units());
    assert_eq!(a.requested_bytes(), b.requested_bytes());
    assert_ne!(
        a.plan_record(),
        run(&source, &plan(3., ObjectTransform::identity()))
            .unwrap()
            .plan_record()
    );
}
#[test]
fn nonuniform_placement_uses_world_millimeter_thickness() {
    let transform = ObjectTransform::new(
        Vector3::new(0., 0., 0.),
        Rotation3::identity(),
        Vector3::new(2., 3., 4.),
        Point3::new(0., 0., 0.),
    )
    .unwrap();
    let out = run(&cube(20.), &plan(2., transform)).unwrap();
    encloses(out.certificate().source_volume, 192_000.);
    encloses(out.certificate().cavity_volume, 36. * 56. * 76.);
    encloses(
        out.certificate().material_volume,
        192_000. - 36. * 56. * 76.,
    );
}
#[test]
fn translated_rotated_source_retains_analytic_material_volume() {
    let transform = ObjectTransform::new(
        Vector3::new(31., -47., 13.),
        Rotation3::from_axis_angle_radians(Vector3::new(0., 0., 1.), 0.37).unwrap(),
        Vector3::new(1., 1., 1.),
        Point3::new(0., 0., 0.),
    )
    .unwrap();
    let out = run(&cube(20.), &plan(2., transform)).unwrap();
    // Rotation evaluates represented binary64 points; permit its bounded input rounding.
    assert!((volume(out.mesh()) - 3904.).abs() < 1e-6);
    assert!(out.certificate().wall_offset_error_mm <= 1e-6);
}
#[test]
fn collapsed_and_excessive_cavities_fail_without_source_changes() {
    let source = cube(20.);
    let before = source.vertices().to_vec();
    for thickness in [10., 11., 100.] {
        assert!(run(&source, &plan(thickness, ObjectTransform::identity())).is_err());
    }
    assert_eq!(source.vertices(), before);
}
#[test]
fn invalid_open_inward_nonconvex_and_disconnected_inputs_are_rejected() {
    let mesh = cube(20.);
    let p = plan(2., ObjectTransform::identity());
    let open =
        TriangleMesh::try_new(mesh.vertices().to_vec(), mesh.triangles()[1..].to_vec()).unwrap();
    assert!(matches!(
        run(&open, &p),
        Err(HollowError::InvalidInput { .. })
    ));
    let inward = TriangleMesh::try_new(
        mesh.vertices().to_vec(),
        mesh.triangles()
            .iter()
            .map(|&[a, b, c]| [a, c, b])
            .collect(),
    )
    .unwrap();
    assert!(matches!(
        run(&inward, &p),
        Err(HollowError::Unsupported { .. })
    ));
    let mut dented = mesh.vertices().to_vec();
    dented[6] = Point3::new(8., 8., 8.);
    let nonconvex = TriangleMesh::try_new(dented, mesh.triangles().to_vec()).unwrap();
    assert!(matches!(
        run(&nonconvex, &p),
        Err(HollowError::Unsupported { .. })
    ));
    let mut points = mesh.vertices().to_vec();
    points.extend(mesh.vertices().iter().map(|v| {
        let [x, y, z] = v.coordinates();
        Point3::new(x + 30., y, z)
    }));
    let mut faces = mesh.triangles().to_vec();
    faces.extend(mesh.triangles().iter().map(|face| face.map(|v| v + 8)));
    let multiple = TriangleMesh::try_new(points, faces).unwrap();
    assert!(matches!(
        run(&multiple, &p),
        Err(HollowError::Unsupported { .. })
    ));
}
#[test]
fn unrelated_or_multioperation_recipes_do_not_imply_implemented_cuts() {
    let hole = Operation {
        id: 2,
        kind: OperationKind::Hole(HoleSpec {
            entry_mm: [0., 0., 20.],
            direction: [0., 0., -1.],
            length_mm: 4.,
            diameter_mm: 2.,
        }),
    };
    let both = make_plan(
        ObjectTransform::identity(),
        vec![
            Operation {
                id: 1,
                kind: OperationKind::Hollow(HollowSpec {
                    blockers: Vec::new(),
                    thickness_mm: 2.,
                    guarantee: None,
                }),
            },
            hole,
        ],
    );
    assert!(matches!(
        run(&cube(20.), &both),
        Err(HollowError::Unsupported { .. })
    ));
    let empty = make_plan(ObjectTransform::identity(), vec![]);
    let support = make_plan(
        ObjectTransform::identity(),
        vec![Operation {
            id: 3,
            kind: OperationKind::Support(SupportSpec {
                surface: empty.model_key(),
                contact_mm: [1., 1., 20.],
                base_xy_mm: [1., 1.],
                tip_radius_mm: 0.2,
                penetration_mm: 0.1,
                shaft_radius_mm: 0.5,
                base_radius_mm: 2.,
                base_height_mm: 1.,
            }),
        }],
    );
    assert!(matches!(
        run(&cube(20.), &support),
        Err(HollowError::Unsupported { .. })
    ));

    assert!(matches!(
        run(&cube(20.), &empty),
        Err(HollowError::Unsupported { .. })
    ));
}
#[test]
fn cancellation_at_every_hollow_phase_returns_no_result() {
    let source = cube(20.);
    let before = source.vertices().to_vec();
    let p = plan(2., ObjectTransform::identity());
    for stop in [
        Checkpoint::HollowStart,
        Checkpoint::HollowWork,
        Checkpoint::HollowCertificate,
        Checkpoint::HollowPublish,
    ] {
        let mut observed = false;
        let mut predicate = |at| {
            if at == stop {
                observed = true;
                true
            } else {
                false
            }
        };
        let result = hollow_mesh(
            &source,
            &p,
            HollowLimits::default(),
            &mut Cancellation::new(&mut predicate),
        );
        assert!(
            matches!(result, Err(HollowError::Cancelled(_))),
            "{stop:?}: {result:?}"
        );
        assert!(observed);
        assert_eq!(source.vertices(), before);
    }
}
#[test]
fn explicit_geometry_work_and_payload_caps_fail_atomically() {
    let source = cube(20.);
    let p = plan(2., ObjectTransform::identity());
    for limits in [
        HollowLimits {
            max_work: 1,
            ..HollowLimits::default()
        },
        HollowLimits {
            max_payload_bytes: 1,
            ..HollowLimits::default()
        },
        HollowLimits {
            max_output_vertices: 8,
            ..HollowLimits::default()
        },
        HollowLimits {
            max_output_triangles: 12,
            ..HollowLimits::default()
        },
        HollowLimits {
            max_input_vertices: 7,
            ..HollowLimits::default()
        },
        HollowLimits {
            max_input_triangles: 11,
            ..HollowLimits::default()
        },
        HollowLimits {
            max_planes: 5,
            ..HollowLimits::default()
        },
    ] {
        assert!(matches!(
            hollow_mesh(&source, &p, limits, &mut Cancellation::never()),
            Err(HollowError::Resource { .. })
        ));
    }
    assert_eq!(source.vertices(), cube(20.).vertices());
}

fn icosahedron(radius: f64) -> TriangleMesh {
    let phi = 1_f64.midpoint(5_f64.sqrt());
    let factor = radius / (1. + phi * phi).sqrt();
    let points = [
        [-1., phi, 0.],
        [1., phi, 0.],
        [-1., -phi, 0.],
        [1., -phi, 0.],
        [0., -1., phi],
        [0., 1., phi],
        [0., -1., -phi],
        [0., 1., -phi],
        [phi, 0., -1.],
        [phi, 0., 1.],
        [-phi, 0., -1.],
        [-phi, 0., 1.],
    ];
    let faces = vec![
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
    TriangleMesh::try_new(
        points
            .map(|[x, y, z]| Point3::new(x * factor, y * factor, z * factor))
            .to_vec(),
        faces,
    )
    .unwrap()
}

#[test]
fn regular_icosahedron_checks_non_axis_aligned_offset_against_analytic_polyhedron() {
    let radius = 3.;
    let source = icosahedron(radius);
    let thickness = 0.2;
    let out = run(&source, &plan(thickness, ObjectTransform::identity())).unwrap();
    let inradius = radius * ((5. + 2. * 5_f64.sqrt()) / 15.).sqrt();
    let edge = 4. * radius / (10. + 2. * 5_f64.sqrt()).sqrt();
    let source_volume = 5. / 12. * (3. + 5_f64.sqrt()) * edge.powi(3);
    let expected = source_volume * (1. - thickness / inradius).powi(3);
    // The source is the represented icosahedron, not the radius-3 smooth sphere.
    assert!((volume(out.cavity()) - expected).abs() < 1e-5);
    assert!(out.certificate().wall_offset_error_mm <= 1e-6);
}

#[test]
fn emitted_shell_reaches_m3_as_one_region_with_one_hole() {
    use karasu_core::slicing::{
        area::{measure_cycle_areas, AreaLimits},
        graph::{build_incidence_graph, GraphLimits},
        grid::{map_exact_grid_vertices, BinaryGridSpec, GridLimits},
        normalize::{normalize_section, NormalizationLimits},
        paths::{extract_paths, PathLimits},
        regions::{assemble_strict_regions, RegionLimits},
        section_mesh_z,
        validity::{inspect_planar_paths, ValidityLimits},
        SectionLimits, SectionTransform,
    };
    let shell = run(&cube(20.), &plan(2., ObjectTransform::identity())).unwrap();
    let section = section_mesh_z(
        shell.mesh(),
        10.,
        SectionTransform::Identity,
        SectionLimits::default(),
    )
    .unwrap();
    let normalized = normalize_section(&section, NormalizationLimits::default()).unwrap();
    let graph = build_incidence_graph(&normalized, GraphLimits::default()).unwrap();
    let paths = extract_paths(&graph, PathLimits::default()).unwrap();
    assert_eq!(paths.paths().len(), 2);
    assert!(paths.paths().iter().all(|p| p.closed));
    let inspection = inspect_planar_paths(&paths, ValidityLimits::default()).unwrap();
    let areas = measure_cycle_areas(&inspection, AreaLimits::default()).unwrap();
    let grid = map_exact_grid_vertices(
        &inspection,
        BinaryGridSpec::try_new([0., 0.], -40).unwrap(),
        GridLimits::default(),
    )
    .unwrap();
    let regions =
        assemble_strict_regions(&inspection, &areas, &grid, RegionLimits::default()).unwrap();
    assert_eq!(regions.regions().len(), 1);
    assert_eq!(regions.region_holes(0).unwrap().len(), 1);
}

#[test]
fn phase_progress_is_ordered_and_cancellation_never_reports_publication() {
    use karasu_core::preparation::hollow::{hollow_mesh_with_progress, HollowStage};
    use std::cell::Cell;
    let source = cube(20.);
    let p = plan(2., ObjectTransform::identity());
    let mut events = Vec::new();
    hollow_mesh_with_progress(
        &source,
        &p,
        HollowLimits::default(),
        &mut Cancellation::never(),
        &mut |progress| events.push(progress),
    )
    .unwrap();
    let expected = [
        HollowStage::Source,
        HollowStage::Construction,
        HollowStage::Reconstruction,
        HollowStage::Certification,
        HollowStage::Assembly,
        HollowStage::Published,
    ];
    assert_eq!(events.iter().map(|p| p.stage).collect::<Vec<_>>(), expected);
    assert_eq!(
        events
            .iter()
            .map(|p| p.completed_stages)
            .collect::<Vec<_>>(),
        [0, 1, 2, 3, 4, 5]
    );
    assert!(events.iter().all(|p| p.total_stages == 5));

    let cancelled = Cell::new(false);
    let mut predicate = |_| cancelled.get();
    events.clear();
    let result = hollow_mesh_with_progress(
        &source,
        &p,
        HollowLimits::default(),
        &mut Cancellation::new(&mut predicate),
        &mut |progress| {
            events.push(progress);
            if progress.stage == HollowStage::Certification {
                cancelled.set(true);
            }
        },
    );
    assert!(matches!(result, Err(HollowError::Cancelled(_))));
    assert_eq!(
        events.iter().map(|p| p.stage).collect::<Vec<_>>(),
        expected[..4]
    );
    assert_eq!(source.vertices(), cube(20.).vertices());
}

#[test]
fn refined_icosahedron_certifies_requested_error_with_independent_ball_bounds() {
    use std::collections::BTreeMap;
    let radius = 10.;
    let original = icosahedron(radius);
    let mut vertices = original.vertices().to_vec();
    let mut faces = Vec::new();
    let mut edge_midpoints = BTreeMap::new();
    for &[first, second, third] in original.triangles() {
        let mut midpoint = |left: u32, right: u32| {
            let edge = (left.min(right), left.max(right));
            *edge_midpoints.entry(edge).or_insert_with(|| {
                let left = vertices[left as usize].coordinates();
                let right = vertices[right as usize].coordinates();
                let middle =
                    std::array::from_fn::<_, 3, _>(|axis| left[axis].midpoint(right[axis]));
                let length = middle
                    .iter()
                    .map(|coordinate| coordinate * coordinate)
                    .sum::<f64>()
                    .sqrt();
                let index = u32::try_from(vertices.len()).unwrap();
                vertices.push(Point3::new(
                    middle[0] * radius / length,
                    middle[1] * radius / length,
                    middle[2] * radius / length,
                ));
                index
            })
        };
        let edge_first = midpoint(first, second);
        let edge_second = midpoint(second, third);
        let edge_third = midpoint(third, first);
        faces.extend([
            [first, edge_first, edge_third],
            [second, edge_second, edge_first],
            [third, edge_third, edge_second],
            [edge_first, edge_second, edge_third],
        ]);
    }
    let source = TriangleMesh::try_new(vertices, faces).unwrap();
    assert_eq!(source.triangles().len(), 80);
    let before = source.vertices().to_vec();
    let p = plan(2., ObjectTransform::identity());
    let out = run(&source, &p).unwrap();
    let certificate = out.certificate();
    assert!(certificate.wall_offset_error_mm <= p.sampling().distance_error_mm);
    assert_eq!(source.vertices(), before);
    assert_eq!(&out.mesh().vertices()[..before.len()], before);
    assert_eq!(
        &out.mesh().triangles()[..source.triangles().len()],
        source.triangles()
    );
    let inradius = source
        .triangles()
        .iter()
        .map(|face| {
            let [anchor, second, third] =
                face.map(|index| source.vertices()[index as usize].coordinates());
            let left = std::array::from_fn::<_, 3, _>(|axis| second[axis] - anchor[axis]);
            let right = std::array::from_fn::<_, 3, _>(|axis| third[axis] - anchor[axis]);
            let normal = [
                left[1] * right[2] - left[2] * right[1],
                left[2] * right[0] - left[0] * right[2],
                left[0] * right[1] - left[1] * right[0],
            ];
            let length = normal
                .iter()
                .map(|coordinate| coordinate * coordinate)
                .sum::<f64>()
                .sqrt();
            normal
                .iter()
                .zip(anchor)
                .map(|(coordinate, position)| coordinate * position)
                .sum::<f64>()
                / length
        })
        .fold(f64::INFINITY, f64::min);
    assert!(inradius > 9. && inradius < 10.);
    let ball_volume = |radius: f64| 4. * std::f64::consts::PI * radius.powi(3) / 3.;
    let lower = ball_volume(inradius - 2. - certificate.wall_offset_error_mm);
    let upper = ball_volume(10. - 2. + certificate.wall_offset_error_mm);
    // These approximate independent analytic comparisons allow only arithmetic
    // rounding, separately from the API's certified represented-geometry bounds.
    assert!(certificate.cavity_volume.minimum_mm3 >= lower - 1e-8);
    assert!(certificate.cavity_volume.maximum_mm3 <= upper + 1e-8);
}
