//! Hole filling and source repairs as a preparation operation (TD-054, phase 3).
use karasu_core::{
    cancellation::Cancellation,
    geometry::{ObjectTransform, Point3, Rotation3, Vector3},
    mesh::{
        repair::RepairChange,
        stl::{import_stl_path, StlOptions, StlUnitPolicy},
        topology::{DefectReport, TopologyLimits},
        TriangleMesh,
    },
    preparation::{
        repair::{repair_source, FillHoles, HoleRefusal, RepairError, RepairPreset, RepairSpec},
        sampled::{hollow_mesh_sampled, SampledHollowLimits},
        HollowSpec, Operation, OperationKind, PlanLimits, PreparationPlan, SamplingSpec,
        SourceBinding,
    },
};
use std::path::Path;

const FILL: RepairSpec = RepairSpec {
    duplicates: true,
    orientation: true,
    outward: false,
    fill_holes: Some(FillHoles {
        max_hole_edges: 256,
        split_rims: false,
        refine: false,
    }),
    allow_unfilled: true,
    weld_mm: None,
    degenerate_mm: None,
    crack_mm: None,
    non_manifold: None,
    visibility: false,
    shells: None,
    rebuild_mm: None,
    non_finite: false,
};

fn defects(mesh: &TriangleMesh) -> DefectReport {
    mesh.defect_report_with_cancellation(TopologyLimits::default(), &mut Cancellation::never())
        .unwrap()
}

fn repair(mesh: &TriangleMesh, spec: RepairSpec) -> Result<Option<TriangleMesh>, RepairError> {
    repair_source(
        mesh,
        spec,
        TopologyLimits::default(),
        &mut Cancellation::never(),
    )
    .map(|(mesh, _)| mesh)
}

/// Six times the signed volume.
fn volume6(mesh: &TriangleMesh) -> f64 {
    mesh.triangles()
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|v| mesh.vertices()[v as usize]);
            a.x * (b.y * c.z - b.z * c.y)
                + a.y * (b.z * c.x - b.x * c.z)
                + a.z * (b.x * c.y - b.y * c.x)
        })
        .sum()
}

fn box_mesh(min: [f64; 3], max: [f64; 3], skip: &[usize]) -> TriangleMesh {
    let points = (0..8)
        .map(|i| {
            Point3::new(
                if i & 1 == 0 { min[0] } else { max[0] },
                if i & 2 == 0 { min[1] } else { max[1] },
                if i & 4 == 0 { min[2] } else { max[2] },
            )
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
        .enumerate()
        .filter(|(i, _)| !skip.contains(i))
        .map(|(_, t)| t)
        .collect();
    TriangleMesh::try_new(points, triangles).unwrap()
}

/// A vertical prism over a regular `n`-gon without its top cap: one `n`-edge hole.
fn open_prism(n: u32, radius: f64, height: f64) -> TriangleMesh {
    let mut points = Vec::new();
    for z in [0., height] {
        for k in 0..n {
            let a = std::f64::consts::TAU * f64::from(k) / f64::from(n);
            points.push(Point3::new(radius * a.cos(), radius * a.sin(), z));
        }
    }
    points.push(Point3::new(0., 0., 0.));
    let mut triangles = Vec::new();
    for i in 0..n {
        let j = (i + 1) % n;
        triangles.push([i, j, j + n]);
        triangles.push([i, j + n, i + n]);
        triangles.push([2 * n, j, i]);
    }
    TriangleMesh::try_new(points, triangles).unwrap()
}

#[test]
fn a_missing_box_face_is_capped_into_the_closed_box() {
    for skip in [vec![2, 3], vec![0], vec![10, 11]] {
        let open = box_mesh([0.; 3], [2., 3., 4.], &skip);
        let filled = repair(&open, FILL).unwrap().unwrap();
        let d = defects(&filled);
        assert_eq!(
            [
                d.boundary_edges,
                d.winding_conflicts,
                d.non_manifold_edges,
                d.closed_components
            ],
            [0, 0, 0, 1],
            "{skip:?}"
        );
        assert!((volume6(&filled) - 6. * 24.).abs() < 1e-9, "{skip:?}");
        // Idempotent: nothing left to repair.
        assert!(repair(&filled, FILL).unwrap().is_none());
    }
}

#[test]
fn a_polygonal_top_hole_is_capped_without_new_vertices() {
    let open = open_prism(48, 10., 5.);
    let (filled, report) = repair_source(
        &open,
        FILL,
        TopologyLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap();
    let filled = filled.unwrap();
    let fill = report.fill.unwrap();
    assert_eq!(
        (
            fill.holes_filled,
            fill.cap_triangles,
            fill.largest_filled_edges
        ),
        (1, 46, 48)
    );
    assert_eq!(filled.vertices().len(), open.vertices().len());
    let d = defects(&filled);
    assert_eq!([d.boundary_edges, d.closed_components], [0, 1]);
    // The flat cap encloses the prism: 48-gon area times height.
    let area = 0.5 * 48. * 100. * (std::f64::consts::TAU / 48.).sin();
    assert!((volume6(&filled) / 6. - area * 5.).abs() < 1e-9);
    assert!((fill.cap_area_mm2 - area).abs() < 1e-9);
}

#[test]
fn large_chained_and_intersecting_holes_are_refused() {
    // Too large for the bound.
    let open = open_prism(48, 10., 5.);
    let small = RepairSpec {
        fill_holes: Some(FillHoles {
            max_hole_edges: 16,
            ..FillHoles::default()
        }),
        allow_unfilled: false,
        ..FILL
    };
    let Err(RepairError::UnfilledHoles { holes, first }) = repair(&open, small) else {
        panic!("a 48-edge hole filled under a 16-edge bound");
    };
    assert_eq!(
        (holes, first.edges, first.reason),
        (1, 48, HoleRefusal::TooLarge)
    );
    // FX-10B: the fin's rim is a chain ending at its non-manifold edge.
    let fin = import_stl_path(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/reference/stl/fx-10b-non-manifold.stl"),
        StlOptions::new(StlUnitPolicy::AssumeMillimeters),
    )
    .unwrap()
    .mesh;
    let (unchanged, report) = repair_source(
        &fin,
        FILL,
        TopologyLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap();
    assert!(unchanged.is_none());
    assert_eq!(
        report.fill.unwrap().refused[0].reason,
        HoleRefusal::OpenChain
    );
    // A box without its top, with a small closed body poking through the top plane: the flat cap
    // would cut through that body, so the hole is refused.
    let mut points = box_mesh([0.; 3], [10., 10., 4.], &[]).vertices().to_vec();
    let mut triangles = box_mesh([0.; 3], [10., 10., 4.], &[2, 3])
        .triangles()
        .to_vec();
    let inner = box_mesh([4., 4., 2.], [6., 6., 6.], &[]);
    points.extend_from_slice(inner.vertices());
    triangles.extend(inner.triangles().iter().map(|t| t.map(|v| v + 8)));
    let pierced = TriangleMesh::try_new(points, triangles).unwrap();
    let (unchanged, report) = repair_source(
        &pierced,
        FILL,
        TopologyLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap();
    assert!(unchanged.is_none());
    let fill = report.fill.unwrap();
    assert_eq!(
        (
            fill.holes_filled,
            fill.holes_refused,
            fill.refused[0].reason
        ),
        (0, 1, HoleRefusal::Intersects)
    );
}

#[test]
fn hollowing_applies_the_plans_repair_to_an_open_source() {
    // FX-10A: the cube without its top. Without the repair hollowing refuses it.
    let open = import_stl_path(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/reference/stl/fx-10a-open.stl"),
        StlOptions::new(StlUnitPolicy::AssumeMillimeters),
    )
    .unwrap()
    .mesh;
    let plan = |repair: Option<RepairSpec>| {
        let mut operations = Vec::new();
        if let Some(spec) = repair {
            operations.push(Operation {
                id: 1,
                kind: OperationKind::Repair(spec),
            });
        }
        operations.push(Operation {
            id: 2,
            kind: OperationKind::Hollow(HollowSpec {
                blockers: Vec::new(),
                thickness_mm: 2.,
                guarantee: None,
            }),
        });
        PreparationPlan::new(
            SourceBinding {
                original_sha256: [0; 32],
                placement: ObjectTransform::new(
                    Vector3::new(0., 0., 0.),
                    Rotation3::identity(),
                    Vector3::new(1., 1., 1.),
                    Point3::new(0., 0., 0.),
                )
                .unwrap(),
            },
            SamplingSpec {
                cell_mm: 0.25,
                distance_error_mm: 1e-6,
                max_cells: u64::MAX,
            },
            operations,
            PlanLimits::default(),
            &mut Cancellation::never(),
        )
        .unwrap()
    };
    let hollow = |plan: &PreparationPlan| {
        hollow_mesh_sampled(
            &open,
            plan,
            SampledHollowLimits::default(),
            &mut Cancellation::never(),
        )
    };
    assert!(hollow(&plan(None)).is_err());
    let spec = RepairSpec {
        allow_unfilled: false,
        ..FILL
    };
    let model = hollow(&plan(Some(spec))).unwrap();
    // The 20 mm cube with a 2 mm wall: a 16 mm cavity (within the sampling band).
    let c = model.certificate();
    assert!((c.cavity_volume_mm3 - 4096.).abs() < 0.05 * 4096., "{c:?}");
    // A repair must come first.
    let misplaced = PreparationPlan::new(
        plan(None).source(),
        plan(None).sampling(),
        vec![
            Operation {
                id: 2,
                kind: OperationKind::Hollow(HollowSpec {
                    blockers: Vec::new(),
                    thickness_mm: 2.,
                    guarantee: None,
                }),
            },
            Operation {
                id: 1,
                kind: OperationKind::Repair(spec),
            },
        ],
        PlanLimits::default(),
        &mut Cancellation::never(),
    );
    assert!(misplaced.is_err());
}

/// A box whose bottom-front edge has a T-junction: the front side is split at the edge's midpoint
/// (vertex 8) and the bottom is not, so a zero-length crack runs along the edge, with or without
/// the zero-area face an exporter leaves in it.
fn cracked_box(sliver: bool) -> TriangleMesh {
    let closed = box_mesh([0.; 3], [10., 10., 10.], &[]);
    let mut points = closed.vertices().to_vec();
    points.push(Point3::new(5., 0., 0.));
    let mut triangles: Vec<[u32; 3]> = closed
        .triangles()
        .iter()
        .copied()
        .filter(|t| *t != [0, 1, 5])
        .collect();
    triangles.extend([[0, 8, 5], [8, 1, 5]]);
    if sliver {
        triangles.push([0, 1, 8]);
    }
    TriangleMesh::try_new(points, triangles).unwrap()
}

#[test]
fn cracks_at_t_junctions_are_closed_by_splitting_without_new_vertices() {
    for sliver in [false, true] {
        let mesh = cracked_box(sliver);
        let found = defects(&mesh);
        assert_eq!(found.boundary_edges, 3, "{found:?}");
        let (repaired, report) = repair_source(
            &mesh,
            RepairSpec {
                allow_unfilled: false,
                ..FILL
            },
            TopologyLimits::default(),
            &mut Cancellation::never(),
        )
        .unwrap();
        let repaired = repaired.unwrap();
        assert!(report.changed());
        let fill = report.fill.unwrap();
        assert_eq!(
            (fill.holes_filled, fill.cracks_closed, fill.faces_split),
            (0, 1, 1),
            "{fill:?}"
        );
        assert_eq!(fill.slivers_removed, usize::from(sliver));
        assert_eq!(fill.holes_refused, 0);
        // Closed, with the same vertices and the same volume.
        let after = defects(&repaired);
        assert_eq!(after.boundary_edges, 0, "{after:?}");
        assert_eq!(repaired.vertices(), mesh.vertices());
        assert!((volume6(&repaired) - 6000.).abs() < 1e-9);
    }
}

#[test]
fn a_crack_that_does_not_close_by_splitting_stays_refused() {
    // The crack's middle vertex is off the edge: the rim is not on a line, so it is capped like
    // any other hole, with a small triangle of area.
    let mut mesh = cracked_box(false);
    let mut points = mesh.vertices().to_vec();
    points[8] = Point3::new(5., 0., 0.001);
    mesh = TriangleMesh::try_new(points, mesh.triangles().to_vec()).unwrap();
    let (_, report) = repair_source(
        &mesh,
        FILL,
        TopologyLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap();
    let fill = report.fill.unwrap();
    assert_eq!((fill.holes_filled, fill.cracks_closed), (1, 0), "{fill:?}");
}

#[test]
fn presets_are_fixed_specs() {
    assert!(RepairPreset::Off.spec().is_empty());
    let exact = RepairPreset::Exact.spec();
    assert!(exact.duplicates && exact.orientation && exact.outward && exact.allow_unfilled);
    assert_eq!(
        exact.fill_holes,
        Some(FillHoles {
            split_rims: true,
            ..FillHoles::default()
        })
    );
    for preset in [
        RepairPreset::Off,
        RepairPreset::Exact,
        RepairPreset::Standard,
        RepairPreset::Aggressive,
    ] {
        assert_eq!(RepairPreset::of(preset.spec()), Some(preset));
    }
    assert_eq!(RepairPreset::of(FILL), None);
}

#[test]
fn every_change_of_the_exact_preset_is_located() {
    // An open box (its top missing), an inside-out box with one face listed twice, and an open
    // prism whose 48-edge hole a 16-edge bound refuses.
    let open = box_mesh([0.; 3], [2., 3., 4.], &[2, 3]);
    let inside_out = box_mesh([10.; 3], [12.; 3], &[]);
    let prism = open_prism(48, 1., 1.);
    let mut points = open.vertices().to_vec();
    let mut triangles = open.triangles().to_vec();
    let base = 8;
    points.extend_from_slice(inside_out.vertices());
    triangles.extend(
        inside_out
            .triangles()
            .iter()
            .map(|t| [t[0] + base, t[2] + base, t[1] + base]),
    );
    triangles.push(*triangles.last().unwrap());
    let base = u32::try_from(points.len()).unwrap();
    points.extend(
        prism
            .vertices()
            .iter()
            .map(|p| Point3::new(p.x - 20., p.y, p.z)),
    );
    triangles.extend(prism.triangles().iter().map(|t| t.map(|v| v + base)));
    let mesh = TriangleMesh::try_new(points, triangles).unwrap();
    let spec = RepairSpec {
        fill_holes: Some(FillHoles {
            max_hole_edges: 16,
            ..FillHoles::default()
        }),
        ..RepairPreset::Exact.spec()
    };
    let (repaired, report) = repair_source(
        &mesh,
        spec,
        TopologyLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap();
    assert!(repaired.is_some());
    let at = |change| -> Vec<[f64; 3]> {
        report
            .all_samples()
            .filter(|s| s.change == change)
            .map(|s| s.point)
            .collect()
    };
    // The duplicate group's face lies on the inside-out box; the box turns at its lowest corner.
    let duplicates = at(RepairChange::Duplicates);
    assert_eq!(duplicates.len(), 1);
    assert!(duplicates[0].iter().all(|c| (10.0..=12.0).contains(c)));
    assert_eq!(at(RepairChange::TurnedOutward), [[10.0; 3]]);
    // The top's rim starts at one of its corners, at z = 4; the prism's at its rim, z = 1.
    let filled = at(RepairChange::HoleFilled);
    assert_eq!(filled.len(), 1);
    assert!(
        (filled[0][2] - 4.0).abs() < 1e-12 && filled[0][0] <= 2.0,
        "{filled:?}"
    );
    let refused = at(RepairChange::HoleRefused);
    assert_eq!(refused.len(), 1);
    assert!(
        (refused[0][2] - 1.0).abs() < 1e-12 && refused[0][0] < -18.0,
        "{refused:?}"
    );
    let fill = report.fill.as_ref().unwrap();
    assert!((fill.largest_cap_mm2 - 6.0).abs() < 1e-9);
    assert_eq!(fill.holes_refused, 1);
}
