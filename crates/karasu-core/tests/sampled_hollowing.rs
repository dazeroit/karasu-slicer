//! Analytic acceptance tests for the sampled (non-convex) hollowing backend.
//!
//! Each fixture has a closed-form cavity under Euclidean inward-distance semantics. Tests check
//! volume against the analytic value, the certified wall enclosure against the requested
//! thickness, closed-manifold output, slicing of the assembled shell, determinism across thread
//! counts, and explicit rejection of unsupported inputs.

use karasu_core::{
    cancellation::{Cancellation, Checkpoint},
    geometry::{ObjectTransform, Point3, Rotation3, Vector3},
    mesh::TriangleMesh,
    preparation::{
        hollow::HollowError,
        sampled::{
            guaranteed_cell, hollow_mesh_sampled, hollow_mesh_sampled_with_progress, surface_area,
            SampledHollowLimits, SampledHollowStage, SampledHollowedModel,
            DEFAULT_CAVITY_TRIANGLE_BUDGET, DEFAULT_OVER_THICKNESS_MM,
            DEFAULT_SHORTFALL_COEFFICIENT,
        },
        HollowSpec, Operation, OperationKind, PlanLimits, PreparationPlan, SamplingSpec,
        SimplifySpec, SourceBinding, WallGuarantee,
    },
};
use std::f64::consts::PI;

/// Builds an indexed mesh from quads/triangles given as vertex index lists.
fn mesh(points: Vec<[f64; 3]>, triangles: Vec<[u32; 3]>) -> TriangleMesh {
    TriangleMesh::try_new(
        points
            .into_iter()
            .map(|[x, y, z]| Point3::new(x, y, z))
            .collect(),
        triangles,
    )
    .unwrap()
}

/// Axis-aligned box with outward counterclockwise faces.
fn cuboid(min: [f64; 3], max: [f64; 3], inward: bool) -> (Vec<[f64; 3]>, Vec<[u32; 3]>) {
    let points = (0..8)
        .map(|i| {
            [
                if i & 1 == 0 { min[0] } else { max[0] },
                if i & 2 == 0 { min[1] } else { max[1] },
                if i & 4 == 0 { min[2] } else { max[2] },
            ]
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
    let mut triangles = Vec::new();
    for q in quads {
        for mut t in [[q[0], q[1], q[2]], [q[0], q[2], q[3]]] {
            if inward {
                t.swap(1, 2);
            }
            triangles.push(t);
        }
    }
    (points, triangles)
}

fn cube(side: f64) -> TriangleMesh {
    let (p, t) = cuboid([0.0; 3], [side; 3], false);
    mesh(p, t)
}

#[allow(clippy::many_single_char_names)] // Parametric (u, v, a, b) and quad corner notation.
/// Torus around Z: major radius `big`, tube radius `small`, `u x v` quads, centred at height `z0`.
fn torus(big: f64, small: f64, u: u32, v: u32, z0: f64) -> TriangleMesh {
    let mut points = Vec::new();
    for i in 0..u {
        let a = 2.0 * PI * f64::from(i) / f64::from(u);
        for j in 0..v {
            let b = 2.0 * PI * f64::from(j) / f64::from(v);
            let r = big + small * b.cos();
            points.push([r * a.cos(), r * a.sin(), z0 + small * b.sin()]);
        }
    }
    let id = |i: u32, j: u32| (i % u) * v + (j % v);
    let mut triangles = Vec::new();
    for i in 0..u {
        for j in 0..v {
            let (a, b, c, d) = (id(i, j), id(i + 1, j), id(i + 1, j + 1), id(i, j + 1));
            triangles.push([a, b, c]);
            triangles.push([a, c, d]);
        }
    }
    mesh(points, triangles)
}

/// Latitude-longitude sphere with polar vertices.
fn sphere(radius: f64, rings: u32, segments: u32, center: [f64; 3]) -> TriangleMesh {
    let mut points = vec![[center[0], center[1], center[2] - radius]];
    for r in 1..rings {
        let phi = PI * f64::from(r) / f64::from(rings) - PI / 2.0;
        for s in 0..segments {
            let theta = 2.0 * PI * f64::from(s) / f64::from(segments);
            points.push([
                center[0] + radius * phi.cos() * theta.cos(),
                center[1] + radius * phi.cos() * theta.sin(),
                center[2] + radius * phi.sin(),
            ]);
        }
    }
    points.push([center[0], center[1], center[2] + radius]);
    let top = u32::try_from(points.len() - 1).unwrap();
    let ring = |r: u32, s: u32| 1 + (r - 1) * segments + (s % segments);
    let mut triangles = Vec::new();
    for s in 0..segments {
        triangles.push([0, ring(1, s + 1), ring(1, s)]);
        triangles.push([top, ring(rings - 1, s), ring(rings - 1, s + 1)]);
    }
    for r in 1..rings - 1 {
        for s in 0..segments {
            let (a, b, c, d) = (
                ring(r, s),
                ring(r, s + 1),
                ring(r + 1, s + 1),
                ring(r + 1, s),
            );
            triangles.push([a, b, c]);
            triangles.push([a, c, d]);
        }
    }
    mesh(points, triangles)
}

fn plan(thickness_mm: f64, cell_mm: f64) -> PreparationPlan {
    plan_rotated(thickness_mm, cell_mm, Rotation3::identity())
}

fn plan_rotated(thickness_mm: f64, cell_mm: f64, rotation: Rotation3) -> PreparationPlan {
    PreparationPlan::new(
        SourceBinding {
            original_sha256: [7; 32],
            placement: ObjectTransform::new(
                Vector3::new(0.0, 0.0, 0.0),
                rotation,
                Vector3::new(1.0, 1.0, 1.0),
                Point3::new(0.0, 0.0, 0.0),
            )
            .unwrap(),
        },
        SamplingSpec {
            cell_mm,
            distance_error_mm: 1e-6,
            max_cells: u64::MAX,
        },
        vec![Operation {
            id: 1,
            kind: OperationKind::Hollow(HollowSpec {
                blockers: Vec::new(),
                thickness_mm,
                guarantee: None,
            }),
        }],
        PlanLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap()
}

fn hollow(source: &TriangleMesh, thickness: f64, cell: f64) -> SampledHollowedModel {
    hollow_mesh_sampled(
        source,
        &plan(thickness, cell),
        SampledHollowLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap()
}

fn assert_wall(model: &SampledHollowedModel, thickness: f64) {
    let c = model.certificate();
    let [lo, hi] = c.surface_distance_mm;
    assert!(lo > 0.0 && lo >= thickness - c.band_mm, "{c:?}");
    assert!(hi <= thickness + c.band_mm, "{c:?}");
    let [vlo, vhi] = c.vertex_distance_mm;
    assert!(lo <= vlo && vhi <= hi, "{c:?}");
}

#[test]
fn cube_cavity_matches_the_eroded_cube() {
    let model = hollow(&cube(20.0), 2.0, 0.25);
    let c = model.certificate();
    assert_wall(&model, 2.0);
    assert_eq!(c.cavity_surface_components, 1);
    assert!((c.source_volume_mm3 - 8000.0).abs() < 1e-9, "{c:?}");
    // Analytic eroded cube: side 16, volume 4096; flat faces are reproduced almost exactly and
    // only the edges and corners are rounded by the sampling.
    let relative = (c.cavity_volume_mm3 - 4096.0).abs() / 4096.0;
    assert!(relative < 0.01, "{c:?}");
    // Iso-vertices on flat walls sit at the exact thickness up to the query budget.
    assert!((c.vertex_distance_mm[0] - 2.0).abs() < 0.05, "{c:?}");
    assert_eq!(model.backend_id(), "sampled-distance-tetra-v1");
    let shell = model.mesh();
    assert_eq!(
        shell.triangles().len(),
        12 + model.cavity().triangles().len()
    );
}

#[test]
fn the_certified_wall_is_proven_face_by_face() {
    // TD-093: the lower end is proven per cavity face, not widened by each face's size. On the
    // cube's flat walls and rounded edges it stays within a micrometre of the thinnest vertex,
    // at a fine and at a coarse cell, where widening lost most of a cell.
    for cell in [0.25, 1.0] {
        let model = hollow(&cube(20.0), 2.0, cell);
        let c = model.certificate();
        let [lo, _] = c.surface_distance_mm;
        assert!(lo <= c.vertex_distance_mm[0], "{c:?}");
        assert!(lo >= c.vertex_distance_mm[0] - 0.0011, "{c:?}");
        assert!(
            lo > 2.0 - 0.0011 - (2.0 - c.vertex_distance_mm[0]).abs(),
            "{c:?}"
        );
    }
}

#[test]
fn near_degenerate_faces_never_shorten_the_distance() {
    // A 10 mm cube whose top front edge is split at its midpoint M, closed by the zero-area face
    // (A, B, M). Rotation rounds that face into a near-collinear one that passes admission; its
    // computed plane is meaningless, and a distance below the true one would skip cavity cells
    // (an open or dented cavity). Each rotation below did so with a normal-projection face case.
    let source = mesh(
        vec![
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [10.0, 10.0, 0.0],
            [0.0, 10.0, 0.0],
            [0.0, 0.0, 10.0],
            [10.0, 0.0, 10.0],
            [10.0, 10.0, 10.0],
            [0.0, 10.0, 10.0],
            [5.0, 0.0, 10.0],
        ],
        vec![
            [0, 2, 1],
            [0, 3, 2],
            [4, 8, 7],
            [8, 6, 7],
            [8, 5, 6],
            [4, 5, 8],
            [0, 1, 5],
            [0, 5, 4],
            [1, 2, 6],
            [1, 6, 5],
            [2, 3, 7],
            [2, 7, 6],
            [3, 0, 4],
            [3, 4, 7],
        ],
    );
    let rotations = [
        (
            [
                -0.390_421_394_014_505_4,
                -0.234_614_704_082_262_15,
                0.385_623_992_668_479_8,
            ],
            5.014_424_458_078_681,
        ),
        (
            [
                -0.174_368_938_281_189_6,
                0.060_472_230_563_426_54,
                0.293_868_435_161_561,
            ],
            2.348_963_250_397_969,
        ),
        (
            [
                -0.338_357_162_809_188_37,
                0.326_110_132_418_312_77,
                -0.022_047_301_168_370_836,
            ],
            1.996_422_563_413_095_2,
        ),
        (
            [
                -0.470_909_240_616_031_8,
                -0.410_549_765_191_072_34,
                -0.278_249_016_315_133_1,
            ],
            3.254_749_852_244_380_3,
        ),
    ];
    for (axis, angle) in rotations {
        let rotation =
            Rotation3::from_axis_angle_radians(Vector3::new(axis[0], axis[1], axis[2]), angle)
                .unwrap();
        let model = hollow_mesh_sampled(
            &source,
            &plan_rotated(1.0, 0.25, rotation),
            SampledHollowLimits::default(),
            &mut Cancellation::never(),
        )
        .unwrap();
        // The cavity keeps the wall: its nearest vertex is exactly one thickness away (a dent
        // reached 0.55 mm), and the certified enclosure stays within the band below it.
        let c = model.certificate();
        assert!(
            (c.vertex_distance_mm[0] - 1.0).abs() < 1e-9,
            "{axis:?}: {c:?}"
        );
        assert!(
            c.surface_distance_mm[0] >= 1.0 - c.band_mm,
            "{axis:?}: {c:?}"
        );
    }
}

#[test]
fn torus_cavity_is_a_thinner_torus() {
    // Non-convex: the cavity is a torus with tube radius r - t around the same core circle.
    let (big, small, t) = (12.0, 5.0, 1.5);
    let source = torus(big, small, 96, 48, 10.0);
    let model = hollow(&source, t, 0.2);
    let c = model.certificate();
    assert_wall(&model, t);
    assert_eq!(c.cavity_surface_components, 1);
    let inner = small - t;
    let analytic = 2.0 * PI * PI * big * inner * inner;
    let relative = (c.cavity_volume_mm3 - analytic).abs() / analytic;
    // Polygonal source tessellation shrinks the tube slightly; 3% covers it with margin.
    assert!(relative < 0.03, "{c:?} analytic {analytic}");
}

#[test]
fn sphere_cavity_volume_converges_with_cell_size() {
    let source = sphere(10.0, 64, 128, [0.0, 0.0, 10.0]);
    let analytic = 4.0 / 3.0 * PI * 8.0f64.powi(3);
    let coarse = hollow(&source, 2.0, 0.4).certificate();
    let fine = hollow(&source, 2.0, 0.2).certificate();
    let error = |c: f64| (c - analytic).abs() / analytic;
    assert!(error(fine.cavity_volume_mm3) < 0.01, "{fine:?}");
    assert!(error(fine.cavity_volume_mm3) <= error(coarse.cavity_volume_mm3) + 1e-3);
    assert!(fine.band_mm < coarse.band_mm);
}

#[test]
fn existing_inner_void_is_preserved_and_walls_follow_it() {
    // Outer 30 mm box with an inward-facing 10 mm void: even-odd material is the shell between.
    let (mut points, mut triangles) = cuboid([0.0; 3], [30.0; 3], false);
    let (inner_points, inner_triangles) = cuboid([10.0; 3], [20.0; 3], true);
    triangles.extend(inner_triangles.into_iter().map(|t| t.map(|v| v + 8)));
    points.extend(inner_points);
    let source = mesh(points, triangles);
    let model = hollow(&source, 2.0, 0.25);
    let c = model.certificate();
    assert_wall(&model, 2.0);
    // The cavity lies 2 mm from both walls: the 26 mm box (sharp inward erosion) minus the inner
    // 10 mm box grown by 2 mm, whose edges and corners round to cylinder and sphere pieces.
    let grown = 1000.0 + 6.0 * 100.0 * 2.0 + 12.0 * 10.0 * PI * 4.0 / 4.0 + 4.0 / 3.0 * PI * 8.0;
    let analytic = 26.0f64.powi(3) - grown;
    let relative = (c.cavity_volume_mm3 - analytic).abs() / analytic;
    assert!(relative < 0.002, "{c:?} analytic {analytic}");
    // One cavity region bounded by two closed surfaces.
    assert_eq!(c.cavity_surface_components, 2);
    assert!((c.source_volume_mm3 - (27000.0 - 1000.0)).abs() < 1e-9);
}

#[test]
fn results_are_identical_for_any_thread_count() {
    let source = torus(8.0, 3.0, 48, 24, 5.0);
    let run = |threads| {
        hollow_mesh_sampled(
            &source,
            &plan(1.0, 0.2),
            SampledHollowLimits {
                threads,
                ..SampledHollowLimits::default()
            },
            &mut Cancellation::never(),
        )
        .unwrap()
    };
    let one = run(1);
    for threads in [2, 3, 8] {
        let other = run(threads);
        assert_eq!(one.cavity(), other.cavity(), "{threads}");
        assert_eq!(one.certificate(), other.certificate());
        assert_eq!(one.stats(), other.stats());
    }
}

#[test]
fn thin_models_report_that_there_is_nothing_to_hollow() {
    let error = hollow_mesh_sampled(
        &cube(3.0),
        &plan(2.0, 0.25),
        SampledHollowLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap_err();
    assert!(matches!(error, HollowError::Unsupported { .. }), "{error}");
    assert!(error.to_string().contains("nothing to hollow"));
}

#[test]
fn open_sources_and_coarse_cells_are_rejected_explicitly() {
    let (points, mut triangles) = cuboid([0.0; 3], [20.0; 3], false);
    triangles.truncate(10);
    let open = mesh(points, triangles);
    let error = hollow_mesh_sampled(
        &open,
        &plan(2.0, 0.25),
        SampledHollowLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("closed, consistently oriented"),
        "{error}"
    );
    let error = hollow_mesh_sampled(
        &cube(20.0),
        &plan(1.0, 0.6),
        SampledHollowLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("cell too coarse"), "{error}");
}

#[test]
fn resource_limits_and_cancellation_publish_nothing() {
    let source = cube(20.0);
    let limited = hollow_mesh_sampled(
        &source,
        &plan(2.0, 0.25),
        SampledHollowLimits {
            max_cavity_triangles: 100,
            ..SampledHollowLimits::default()
        },
        &mut Cancellation::never(),
    )
    .unwrap_err();
    assert!(matches!(limited, HollowError::Resource { .. }), "{limited}");
    for target in [
        Checkpoint::HollowStart,
        Checkpoint::HollowWork,
        Checkpoint::HollowCertificate,
        Checkpoint::HollowPublish,
    ] {
        let mut predicate = |c| c == target;
        let error = hollow_mesh_sampled(
            &source,
            &plan(2.0, 0.25),
            SampledHollowLimits::default(),
            &mut Cancellation::new(&mut predicate),
        )
        .unwrap_err();
        assert!(
            matches!(error, HollowError::Cancelled(_)),
            "{target:?}: {error}"
        );
    }
}

#[test]
fn progress_reports_every_stage_in_order() {
    let mut stages = Vec::new();
    hollow_mesh_sampled_with_progress(
        &cube(20.0),
        &plan(2.0, 0.5),
        SampledHollowLimits::default(),
        &mut Cancellation::never(),
        &mut |p| {
            if stages.last() != Some(&p.stage) {
                stages.push(p.stage);
            }
        },
    )
    .unwrap();
    assert_eq!(
        stages,
        [
            SampledHollowStage::Source,
            SampledHollowStage::Field,
            SampledHollowStage::Extraction,
            SampledHollowStage::Certification,
            SampledHollowStage::Assembly,
            SampledHollowStage::Published,
        ]
    );
}

/// Two bodies given as separate closed shells, welded by position.
fn two_bodies(a: ([f64; 3], [f64; 3]), b: ([f64; 3], [f64; 3]), inward: bool) -> TriangleMesh {
    let (mut points, mut triangles) = cuboid(a.0, a.1, false);
    let (more, faces) = cuboid(b.0, b.1, inward);
    let mut ids = Vec::new();
    for p in more {
        // Exact equality: shared corners are written from the same literals.
        let id = points
            .iter()
            .position(|q| q.map(f64::to_bits) == p.map(f64::to_bits))
            .unwrap_or_else(|| {
                points.push(p);
                points.len() - 1
            });
        ids.push(u32::try_from(id).unwrap());
    }
    triangles.extend(faces.iter().map(|f| f.map(|v| ids[v as usize])));
    mesh(points, triangles)
}

/// The generalized winding number of `mesh` at `p`: the faces' signed solid angles over 4π
/// (Van Oosterom and Strackee), an integer for closed surfaces away from them.
fn winding_at(mesh: &TriangleMesh, p: [f64; 3]) -> f64 {
    let total: f64 = mesh
        .triangles()
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|v| {
                let q = mesh.vertices()[v as usize];
                [q.x - p[0], q.y - p[1], q.z - p[2]]
            });
            let length = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            let dot = |u: [f64; 3], v: [f64; 3]| u[0] * v[0] + u[1] * v[1] + u[2] * v[2];
            let triple = a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                + a[2] * (b[0] * c[1] - b[1] * c[0]);
            let (la, lb, lc) = (length(a), length(b), length(c));
            let denominator = la * lb * lc + dot(a, b) * lc + dot(b, c) * la + dot(c, a) * lb;
            2.0 * triple.atan2(denominator)
        })
        .sum();
    total / (4.0 * PI)
}

#[test]
fn overlapping_bodies_are_hollowed_as_their_union() {
    // Boxes [0, 20] and [10, 30] along x overlap in [10, 20]. Each surface keeps its wall, so
    // three voids appear: in the first box only, in the overlap (inside both), in the second.
    let overlapping = two_bodies(([0.; 3], [20.; 3]), ([10., 0., 0.], [30., 20., 20.]), false);
    let model = hollow_mesh_sampled(
        &overlapping,
        &plan(2.0, 0.25),
        SampledHollowLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap();
    assert_eq!(model.certificate().cavity_surface_components, 3);
    assert_eq!((model.overlap().surfaces, model.overlap().deepest), (1, 2));
    // The overlap's void is subtracted twice: every void is empty, every wall material.
    let shell = model.mesh();
    assert!(shell.triangles().len() > model.exterior_faces() + model.cavity().triangles().len());
    for (x, expected) in [
        (5.0, 0.0),
        (15.0, 0.0),
        (25.0, 0.0),
        (1.0, 1.0),
        (9.0, 1.0),
        (11.0, 2.0),
        (21.0, 1.0),
    ] {
        let w = winding_at(shell, [x, 10.0, 10.0]);
        assert!((w - expected).abs() < 1e-6, "x = {x}: {w}");
    }
}

#[test]
fn zero_area_faces_and_cracks_are_left_out_of_the_source() {
    // A 20 mm box whose front side is split at the bottom edge's midpoint (vertex 8), the bottom
    // not, with the zero-area face an exporter leaves in the crack: hollowed without repair.
    let (mut points, triangles) = cuboid([0.; 3], [20.; 3], false);
    points.push([10.0, 0.0, 0.0]);
    let front = triangles
        .iter()
        .position(|t| {
            let z = |v: u32| points[v as usize];
            t.iter().all(|&v| z(v)[1] == 0.0) && t.iter().filter(|&&v| z(v)[2] == 0.0).count() == 2
        })
        .unwrap();
    let corners = triangles[front];
    // The front face's bottom edge (two corners at z = 0) gets the midpoint.
    let edge = (0..3)
        .find(|&e| {
            points[corners[e] as usize][2] == 0.0 && points[corners[(e + 1) % 3] as usize][2] == 0.0
        })
        .unwrap();
    let (start, end, apex) = (
        corners[edge],
        corners[(edge + 1) % 3],
        corners[(edge + 2) % 3],
    );
    let mut faces: Vec<[u32; 3]> = triangles
        .iter()
        .enumerate()
        .filter(|&(i, _)| i != front)
        .map(|(_, &t)| t)
        .collect();
    faces.extend([[start, 8, apex], [8, end, apex], [start, end, 8]]);
    let source = mesh(points, faces);
    let model = hollow_mesh_sampled(
        &source,
        &plan(2.0, 0.25),
        SampledHollowLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap();
    let left_out = model.measure_zero();
    assert_eq!((left_out.cracks_closed, left_out.zero_area_faces), (1, 1));
    assert_eq!(model.certificate().cavity_surface_components, 1);
    // The shell's exterior is the source with the crack closed: still 14 faces, the zero-area
    // face gone and the bottom face split in two.
    assert_eq!(model.exterior_faces(), 14);
}

#[test]
fn winding_admits_touching_bodies_and_refuses_inverted_cavities() {
    let hollow = |source: &TriangleMesh| {
        hollow_mesh_sampled(
            source,
            &plan(2.0, 0.25),
            SampledHollowLimits::default(),
            &mut Cancellation::never(),
        )
    };
    // Two 20 mm boxes sharing a face: a balanced surface with a double internal wall. Each box
    // keeps its own wall around the shared face, so two cavities appear.
    let touching = two_bodies(([0.; 3], [20.; 3]), ([20., 0., 0.], [40., 20., 20.]), false);
    let model = hollow(&touching).unwrap();
    assert_eq!(model.certificate().cavity_surface_components, 2);
    let c = model.certificate();
    assert!(c.surface_distance_mm[0] >= 2.0 - c.band_mm, "{c:?}");
    // A reversed shell outside any body: negative winding.
    let inverted = two_bodies(([0.; 3], [20.; 3]), ([30., 0., 0.], [50., 20., 20.]), true);
    let error = hollow(&inverted).unwrap_err();
    assert!(error.to_string().contains("negative winding"), "{error}");
}

fn simplified_plan(thickness_mm: f64, cell_mm: f64, tolerance_mm: f64) -> PreparationPlan {
    let base = plan(thickness_mm, cell_mm);
    PreparationPlan::new(
        base.source(),
        base.sampling(),
        vec![
            Operation {
                id: 1,
                kind: OperationKind::Hollow(HollowSpec {
                    blockers: Vec::new(),
                    thickness_mm,
                    guarantee: None,
                }),
            },
            Operation {
                id: 2,
                kind: OperationKind::Simplify(SimplifySpec { tolerance_mm }),
            },
        ],
        PlanLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap()
}

fn simplified(
    source: &TriangleMesh,
    plan: &PreparationPlan,
    threads: usize,
) -> SampledHollowedModel {
    hollow_mesh_sampled(
        source,
        plan,
        SampledHollowLimits {
            threads,
            ..SampledHollowLimits::default()
        },
        &mut Cancellation::never(),
    )
    .unwrap()
}

#[test]
fn simplification_collapses_flat_walls_and_keeps_the_certified_wall() {
    let source = cube(12.0);
    let original = hollow(&source, 2.0, 0.25);
    let model = simplified(&source, &simplified_plan(2.0, 0.25, 0.01), 0);
    let c = model.certificate();
    let s = c.simplification.unwrap();
    assert_eq!(s.triangles[0], original.cavity().triangles().len());
    assert_eq!(s.triangles[1], model.cavity().triangles().len());
    // Flat walls collapse to a small fraction; edges and corners keep their rounding.
    assert!(s.triangles[1] * 4 < s.triangles[0], "{s:?}");
    assert!(
        s.max_deviation_mm <= 0.01 && s.max_deviation_mm > 0.0,
        "{s:?}"
    );
    // The wall never gets thinner than the thinnest original cavity vertex.
    assert!(s.wall_floor_mm > 1.99 && s.wall_floor_mm < 2.0, "{s:?}");
    // Untouched edge faces keep their (looser) widened bounds; simplification never lowers it.
    assert!(
        c.surface_distance_mm[0] >= original.certificate().surface_distance_mm[0],
        "{c:?}"
    );
    // The cavity moves by at most the tolerance: its volume changes by at most area x 0.01.
    let area = 6.0 * 8.0 * 8.0;
    assert!(
        (c.cavity_volume_mm3 - s.cavity_volume_before_mm3).abs() <= area * 0.01,
        "{c:?}"
    );
    assert_eq!(c.cavity_surface_components, 1);
    assert_eq!(
        model.mesh().triangles().len(),
        12 + model.cavity().triangles().len()
    );
}

#[test]
fn simplification_respects_curvature_and_is_identical_for_any_thread_count() {
    let source = sphere(6.0, 16, 32, [0.0, 0.0, 6.0]);
    let tight = simplified(&source, &simplified_plan(1.5, 0.5, 0.002), 1);
    let loose = simplified(&source, &simplified_plan(1.5, 0.5, 0.05), 1);
    let [before, tight_after] = tight.certificate().simplification.unwrap().triangles;
    let loose_after = loose.certificate().simplification.unwrap().triangles[1];
    assert!(
        loose_after < tight_after && tight_after < before,
        "{before} {tight_after} {loose_after}"
    );
    for model in [&tight, &loose] {
        let s = model.certificate().simplification.unwrap();
        assert!(s.max_deviation_mm <= s.tolerance_mm, "{s:?}");
    }
    let other = simplified(&source, &simplified_plan(1.5, 0.5, 0.05), 3);
    assert_eq!(loose.cavity(), other.cavity());
    assert_eq!(loose.certificate(), other.certificate());
}

#[test]
fn simplification_must_follow_the_hollow_and_stay_below_the_wall() {
    let base = plan(2.0, 0.25);
    let misplaced = PreparationPlan::new(
        base.source(),
        base.sampling(),
        vec![Operation {
            id: 2,
            kind: OperationKind::Simplify(SimplifySpec { tolerance_mm: 0.01 }),
        }],
        PlanLimits::default(),
        &mut Cancellation::never(),
    );
    assert!(misplaced.is_err());
    let error = hollow_mesh_sampled(
        &cube(20.0),
        &simplified_plan(2.0, 0.25, 2.5),
        SampledHollowLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap_err();
    assert!(matches!(error, HollowError::InvalidInput { .. }), "{error}");
}

/// A hollow plan bound to `placement`.
fn plan_placed(thickness_mm: f64, cell_mm: f64, placement: ObjectTransform) -> PreparationPlan {
    PreparationPlan::new(
        SourceBinding {
            original_sha256: [7; 32],
            placement,
        },
        SamplingSpec {
            cell_mm,
            distance_error_mm: 1e-6,
            max_cells: u64::MAX,
        },
        vec![Operation {
            id: 1,
            kind: OperationKind::Hollow(HollowSpec {
                blockers: Vec::new(),
                thickness_mm,
                guarantee: None,
            }),
        }],
        PlanLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap()
}

fn transform(translation: [f64; 3], rotation: Rotation3, scale: [f64; 3]) -> ObjectTransform {
    ObjectTransform::new(
        Vector3::new(translation[0], translation[1], translation[2]),
        rotation,
        Vector3::new(scale[0], scale[1], scale[2]),
        Point3::new(10.0, 10.0, 10.0),
    )
    .unwrap()
}

fn relative(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs()
}

#[test]
fn a_model_frame_hollow_is_placed_without_hollowing_again() {
    // TD-089: hollowed once in the model frame, then mapped by any rigid placement.
    let model = hollow(&cube(20.0), 2.0, 0.5);
    let turn = Rotation3::from_axis_angle_radians(Vector3::new(1.0, 2.0, 3.0), 0.7).unwrap();
    let placement = transform([30.0, -12.5, 4.0], turn, [1.0; 3]);
    let placed = model.placed(placement, &mut Cancellation::never()).unwrap();
    let affine = placement.to_affine().unwrap();
    for (mesh, from) in [
        (placed.mesh(), model.mesh()),
        (placed.cavity(), model.cavity()),
    ] {
        assert_eq!(mesh.triangles(), from.triangles());
        for (&p, &q) in from.vertices().iter().zip(mesh.vertices()) {
            assert_eq!(affine.transform_point(p).unwrap(), q);
        }
    }
    // A rigid placement keeps every distance; the volumes are recomputed from the placed
    // surfaces and agree to rounding.
    let (a, b) = (model.certificate(), placed.certificate());
    assert_eq!(b.surface_distance_mm, a.surface_distance_mm);
    assert_eq!(b.vertex_distance_mm, a.vertex_distance_mm);
    assert_eq!(b.placement_scale, [1.0, 1.0]);
    assert!(relative(b.source_volume_mm3, a.source_volume_mm3) < 1e-9);
    assert!(relative(b.cavity_volume_mm3, a.cavity_volume_mm3) < 1e-9);
    assert_eq!(placed.plan_record(), model.plan_record());
    assert_eq!(placed.placed_by(), Some(placement));
    assert_eq!(model.placed_by(), None);
    assert!(placed
        .placed(placement, &mut Cancellation::never())
        .is_err());
    // The exterior is bit-identical to hollowing the placed source directly.
    let direct = hollow_mesh_sampled(
        &cube(20.0),
        &plan_placed(2.0, 0.5, placement),
        SampledHollowLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap();
    let exterior = direct.mesh().triangles().len() - direct.cavity().triangles().len();
    assert_eq!(
        &direct.mesh().triangles()[..exterior],
        &placed.mesh().triangles()[..exterior]
    );
    let used = cube(20.0).vertices().len();
    assert_eq!(
        &direct.mesh().vertices()[..used],
        &placed.mesh().vertices()[..used]
    );
}

#[test]
fn placements_mirror_and_scale_the_shell_and_its_wall() {
    let model = hollow(&cube(20.0), 2.0, 0.5);
    let model_certificate = model.certificate();
    // A mirror reverses every face, so both surfaces still face the right way: the volumes stay
    // positive.
    let mirrored = model
        .placed(
            transform([0.0; 3], Rotation3::identity(), [-1.0, 1.0, 1.0]),
            &mut Cancellation::never(),
        )
        .unwrap();
    for (mesh, from) in [
        (mirrored.mesh(), model.mesh()),
        (mirrored.cavity(), model.cavity()),
    ] {
        for (&[a, b, c], &placed) in from.triangles().iter().zip(mesh.triangles()) {
            assert_eq!(placed, [a, c, b]);
        }
    }
    let c = mirrored.certificate();
    assert!(relative(c.source_volume_mm3, model_certificate.source_volume_mm3) < 1e-9);
    assert!(relative(c.cavity_volume_mm3, model_certificate.cavity_volume_mm3) < 1e-9);
    // A scale scales the wall with the model, within its smallest and largest factor, and the
    // volumes by the determinant.
    let scaled = model
        .placed(
            transform([0.0; 3], Rotation3::identity(), [2.0, 2.0, 1.0]),
            &mut Cancellation::never(),
        )
        .unwrap()
        .certificate();
    let [lo, hi] = model_certificate.surface_distance_mm;
    assert_eq!(scaled.placement_scale, [1.0, 2.0]);
    assert_eq!(scaled.surface_distance_mm, [lo, hi * 2.0]);
    assert_eq!(scaled.thickness_mm, model_certificate.thickness_mm);
    assert!(
        relative(
            scaled.source_volume_mm3,
            4.0 * model_certificate.source_volume_mm3
        ) < 1e-9
    );
    // A zero factor would collapse the shell.
    assert!(model
        .placed(
            transform([0.0; 3], Rotation3::identity(), [1.0, 0.0, 1.0]),
            &mut Cancellation::never(),
        )
        .is_err());
}

/// A 30 mm box with an inward 10 mm void: the cavity rounds around the void's edges, where a
/// coarse cell falls short of the wall (TD-093).
fn box_with_void() -> TriangleMesh {
    let (mut points, mut triangles) = cuboid([0.0; 3], [30.0; 3], false);
    let (inner_points, inner_triangles) = cuboid([10.0; 3], [20.0; 3], true);
    triangles.extend(inner_triangles.into_iter().map(|t| t.map(|v| v + 8)));
    points.extend(inner_points);
    mesh(points, triangles)
}

fn guaranteed_plan(thickness_mm: f64, cell_mm: f64, coefficient: f64) -> PreparationPlan {
    PreparationPlan::new(
        SourceBinding {
            original_sha256: [7; 32],
            placement: ObjectTransform::new(
                Vector3::new(0.0, 0.0, 0.0),
                Rotation3::identity(),
                Vector3::new(1.0, 1.0, 1.0),
                Point3::new(0.0, 0.0, 0.0),
            )
            .unwrap(),
        },
        SamplingSpec {
            cell_mm,
            distance_error_mm: 1e-6,
            max_cells: u64::MAX,
        },
        vec![Operation {
            id: 1,
            kind: OperationKind::Hollow(HollowSpec {
                blockers: Vec::new(),
                thickness_mm,
                guarantee: Some(WallGuarantee {
                    shortfall_coefficient: coefficient,
                }),
            }),
        }],
        PlanLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap()
}

fn guaranteed(
    source: &TriangleMesh,
    plan: &PreparationPlan,
    threads: usize,
) -> SampledHollowedModel {
    hollow_mesh_sampled(
        source,
        plan,
        SampledHollowLimits {
            threads,
            ..SampledHollowLimits::default()
        },
        &mut Cancellation::never(),
    )
    .unwrap()
}

#[test]
fn the_guarantee_cell_follows_the_over_thickness_the_budget_and_the_grid() {
    // sqrt(0.05 * 2 / 0.39) = 0.506 mm from the over-thickness.
    let cell = guaranteed_cell(1000.0, 2.0, 1e-6, DEFAULT_OVER_THICKNESS_MM, 1e7);
    assert!(
        (cell - (0.05 * 2.0 / 0.39f64).sqrt()).abs() < 1e-12,
        "{cell}"
    );
    // A large surface: sqrt(7.9 * 5e6 / 1e7) = 1.99 mm from the budget, then the grid's bound
    // (2 - 2e-6) / sqrt(3) * 0.95 = 1.097 mm.
    let cell = guaranteed_cell(5.0e6, 2.0, 1e-6, DEFAULT_OVER_THICKNESS_MM, 1e7);
    assert!(
        (cell - (2.0 - 2e-6) / 3f64.sqrt() * 0.95).abs() < 1e-12,
        "{cell}"
    );
    // The cube's area.
    assert!((surface_area(&cube(20.0)) - 2400.0).abs() < 1e-9);
}

#[test]
fn the_guarantee_reaches_the_requested_wall_in_one_pass() {
    let source = box_with_void();
    let cell = guaranteed_cell(
        surface_area(&source),
        2.0,
        1e-6,
        DEFAULT_OVER_THICKNESS_MM,
        DEFAULT_CAVITY_TRIANGLE_BUDGET,
    );
    let model = guaranteed(
        &source,
        &guaranteed_plan(2.0, cell, DEFAULT_SHORTFALL_COEFFICIENT),
        0,
    );
    let c = model.certificate();
    assert_eq!(c.passes, 1, "{c:?}");
    assert!(
        (c.offset_mm - (2.0 + 0.40 * cell * cell / 2.0)).abs() < 1e-12,
        "{c:?}"
    );
    assert!(
        c.surface_distance_mm[0] + 2.0 * c.query_error_mm >= 2.0,
        "{c:?}"
    );
    assert_eq!(c.thickness_mm, 2.0);
    // Without the guarantee the same cell falls short of the wall around the void's edges.
    let plain = hollow(&source, 2.0, cell);
    assert!(
        plain.certificate().surface_distance_mm[0] < 1.99,
        "{:?}",
        plain.certificate()
    );
}

#[test]
fn a_short_first_pass_is_corrected_identically_for_any_thread_count() {
    // A coefficient far too low: the first pass falls short and the second raises the offset.
    let source = box_with_void();
    let plan = guaranteed_plan(2.0, 0.5, 0.001);
    let one = guaranteed(&source, &plan, 1);
    let c = one.certificate();
    assert_eq!(c.passes, 2, "{c:?}");
    assert!(
        c.surface_distance_mm[0] + 2.0 * c.query_error_mm >= 2.0,
        "{c:?}"
    );
    for threads in [3, 8] {
        let other = guaranteed(&source, &plan, threads);
        assert_eq!(other.cavity(), one.cavity(), "{threads}");
        assert_eq!(other.certificate(), one.certificate(), "{threads}");
    }
}

#[test]
fn the_guarantee_is_recorded_and_checked() {
    let without = plan(2.0, 0.5);
    let with = guaranteed_plan(2.0, 0.5, 0.4);
    assert_ne!(without.canonical_record(), with.canonical_record());
    assert!(with
        .canonical_record()
        .starts_with(&without.canonical_record()[..40]));
    for coefficient in [0.0, -0.1, 1.5, f64::NAN] {
        let refused = PreparationPlan::new(
            with.source(),
            with.sampling(),
            vec![Operation {
                id: 1,
                kind: OperationKind::Hollow(HollowSpec {
                    blockers: Vec::new(),
                    thickness_mm: 2.0,
                    guarantee: Some(WallGuarantee {
                        shortfall_coefficient: coefficient,
                    }),
                }),
            }],
            PlanLimits::default(),
            &mut Cancellation::never(),
        );
        assert!(refused.is_err(), "{coefficient}");
    }
    let message = HollowError::WallNotReached {
        requested_mm: 1.0,
        certified_mm: 0.97,
        passes: 6,
    }
    .to_string();
    assert!(message.contains("1 mm") && message.contains("0.97") && message.contains("6 passes"));
}
