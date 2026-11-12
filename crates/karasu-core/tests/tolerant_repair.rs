//! Welding, degenerate faces and tolerant cracks (M6.4d r2, TD-092, stages R1, R2 and R4).
use karasu_core::{
    cancellation::Cancellation,
    geometry::{ObjectTransform, Point3},
    mesh::{
        repair::RepairChange,
        topology::{DefectReport, TopologyLimits},
        TriangleMesh,
    },
    preparation::{
        repair::{
            repair_source, FillHoles, RepairError, RepairPreset, RepairSpec, SourceRepairReport,
            MAX_TOLERANCE_MM,
        },
        Operation, OperationKind, PlanLimits, PreparationPlan, SamplingSpec, SourceBinding,
    },
};

fn repair(mesh: &TriangleMesh, spec: RepairSpec) -> (Option<TriangleMesh>, SourceRepairReport) {
    repair_source(
        mesh,
        spec,
        TopologyLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap()
}

fn defects(mesh: &TriangleMesh) -> DefectReport {
    mesh.defect_report_with_cancellation(TopologyLimits::default(), &mut Cancellation::never())
        .unwrap()
}

/// Only the given stage.
fn only(stage: impl FnOnce(&mut RepairSpec)) -> RepairSpec {
    let mut spec = RepairPreset::Off.spec();
    stage(&mut spec);
    spec
}

/// The corners of a 10 mm cube and its outward faces as indices.
fn cube() -> (Vec<Point3>, Vec<[u32; 3]>) {
    let points = (0..8)
        .map(|i| {
            let at = |bit| if i & bit == 0 { 0.0 } else { 10.0 };
            Point3::new(at(1), at(2), at(4))
        })
        .collect();
    let faces = [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ]
    .iter()
    .flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]])
    .collect();
    (points, faces)
}

fn closed(mesh: &TriangleMesh) -> bool {
    let d = defects(mesh);
    d.boundary_edges == 0
        && d.winding_conflicts == 0
        && d.unbalanced_non_manifold_edges == 0
        && d.excluded_triangles == 0
}

/// Deterministic pseudo-random numbers in [0, 1).
struct Random(u64);

impl Random {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        #[allow(clippy::cast_precision_loss)]
        let unit = (self.0 >> 11) as f64 / (1u64 << 53) as f64;
        unit
    }
}

fn distance(p: Point3, q: Point3) -> f64 {
    ((p.x - q.x).powi(2) + (p.y - q.y).powi(2) + (p.z - q.z).powi(2)).sqrt()
}

#[test]
fn welding_never_merges_points_farther_apart_than_the_tolerance() {
    // A soup of separate triangles whose corners scatter around shared centres, closer and farther
    // than the tolerance: every corner moves by at most the tolerance, and corners that end up
    // as one vertex were within the tolerance of each other.
    let tolerance = 0.01;
    for seed in [1u64, 7, 42, 1234] {
        let mut random = Random(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
        let centres: Vec<[f64; 3]> = (0..40)
            .map(|i| {
                let k = f64::from(i);
                [k * 0.5, (k * 0.37) % 3.0, (k * 0.71) % 2.0]
            })
            .collect();
        let mut points = Vec::new();
        let mut faces = Vec::new();
        for f in 0..200u32 {
            let mut corner = |c: usize| {
                let base = centres[c];
                let jitter = |r: &mut Random| (r.next() - 0.5) * 2.0 * tolerance;
                points.push(Point3::new(
                    base[0] + jitter(&mut random),
                    base[1] + jitter(&mut random),
                    base[2] + jitter(&mut random),
                ));
                u32::try_from(points.len() - 1).unwrap()
            };
            let pick = (f as usize * 7) % 38;
            faces.push([corner(pick), corner(pick + 1), corner(pick + 2)]);
        }
        let mesh = TriangleMesh::try_new(points, faces).unwrap();
        let (welded, report) = repair(&mesh, only(|s| s.weld_mm = Some(tolerance)));
        let welded = welded.unwrap();
        let weld = report.weld.unwrap();
        assert!(weld.clusters > 0 && weld.max_displacement_mm <= tolerance);
        // Corners 0.5 mm apart never merge, so every face survives, in order.
        assert_eq!(weld.faces_collapsed, 0);
        assert_eq!(welded.triangles().len(), mesh.triangles().len());
        let mut merged = std::collections::HashMap::<u32, Vec<Point3>>::default();
        for (before, after) in mesh.triangles().iter().zip(welded.triangles()) {
            for (&b, &a) in before.iter().zip(after) {
                let (original, moved) =
                    (mesh.vertices()[b as usize], welded.vertices()[a as usize]);
                assert!(distance(original, moved) <= tolerance);
                merged.entry(a).or_default().push(original);
            }
        }
        for group in merged.values() {
            for p in group {
                for q in group {
                    assert!(distance(*p, *q) <= tolerance, "{p:?} {q:?}");
                }
            }
        }
        assert_eq!(
            welded.vertices().len(),
            mesh.vertices().len() - weld.vertices_merged
        );
        // Deterministic: the same result again.
        let (again, _) = repair(&mesh, only(|s| s.weld_mm = Some(tolerance)));
        assert_eq!(again.unwrap(), welded);
    }
}

#[test]
fn a_corner_split_below_the_tolerance_closes_and_one_above_stays_open() {
    // Corner 7 duplicated 0.5 µm away (vertex 8); the top faces use the copy.
    let (mut points, mut faces) = cube();
    points.push(Point3::new(10.0, 10.0, 10.0 + 0.0005));
    for face in faces
        .iter_mut()
        .filter(|f| f.contains(&7) && f.contains(&4))
    {
        for v in face.iter_mut().filter(|v| **v == 7) {
            *v = 8;
        }
    }
    let mesh = TriangleMesh::try_new(points, faces).unwrap();
    assert!(!closed(&mesh));
    let (still_open, report) = repair(&mesh, only(|s| s.weld_mm = Some(0.0001)));
    assert!(still_open.is_none());
    assert_eq!(report.weld.unwrap().clusters, 0);
    let (welded, report) = repair(&mesh, only(|s| s.weld_mm = Some(0.001)));
    let welded = welded.unwrap();
    assert!(closed(&welded));
    assert_eq!(welded.vertices().len(), 8);
    let weld = report.weld.as_ref().unwrap();
    assert_eq!((weld.clusters, weld.vertices_merged), (1, 1));
    assert!((weld.max_displacement_mm - 0.0005).abs() < 1e-12);
    // The kept vertex is the smaller index: the original corner.
    assert_eq!(
        report
            .all_samples()
            .filter(|s| s.change == RepairChange::Welded)
            .map(|s| s.point)
            .collect::<Vec<_>>(),
        [[10.0; 3]]
    );
}

#[test]
fn exact_welding_merges_equal_positions_and_collapsed_faces_are_reported() {
    // Corner 7 duplicated at the same position: exact welding closes the cube.
    let (mut points, mut faces) = cube();
    points.push(points[7]);
    for face in faces
        .iter_mut()
        .filter(|f| f.contains(&7) && f.contains(&4))
    {
        for v in face.iter_mut().filter(|v| **v == 7) {
            *v = 8;
        }
    }
    // A sliver whose two corners are 0.2 µm apart collapses when welded at 1 µm.
    points.extend([
        Point3::new(20.0, 0.0, 0.0),
        Point3::new(20.0, 0.0, 0.0002),
        Point3::new(25.0, 0.0, 0.0),
    ]);
    faces.push([9, 10, 11]);
    let mesh = TriangleMesh::try_new(points, faces).unwrap();
    let (exact, report) = repair(&mesh, only(|s| s.weld_mm = Some(0.0)));
    let weld = report.weld.unwrap();
    assert_eq!((weld.clusters, weld.faces_collapsed), (1, 0));
    assert_eq!(weld.max_displacement_mm, 0.0);
    assert_eq!(exact.unwrap().triangles().len(), 13);
    let (tolerant, report) = repair(&mesh, only(|s| s.weld_mm = Some(0.001)));
    let weld = report.weld.unwrap();
    assert_eq!((weld.clusters, weld.faces_collapsed), (2, 1));
    assert!((weld.collapsed_area_mm2 - 0.5 * 5.0 * 0.0002).abs() < 1e-12);
    let tolerant = tolerant.unwrap();
    assert_eq!(tolerant.triangles().len(), 12);
    assert!(closed(&tolerant));
}

/// A cube whose front and bottom faces share a vertex 8 inserted on their common edge 0-1 at `x`
/// (and `z` above it): both faces split there.
fn split_bottom_edge(x: f64, z: f64) -> TriangleMesh {
    let (mut points, faces) = cube();
    points.push(Point3::new(x, 0.0, z));
    let mut faces: Vec<[u32; 3]> = faces
        .into_iter()
        .filter(|f| *f != [0, 1, 5] && *f != [0, 3, 1])
        .collect();
    faces.extend([[0, 8, 5], [8, 1, 5], [0, 3, 8], [8, 3, 1]]);
    TriangleMesh::try_new(points, faces).unwrap()
}

#[test]
fn needles_are_removed_and_short_edges_collapse_within_the_tolerance() {
    // A face with a repeated vertex is removed even without a tolerance.
    let (points, mut faces) = cube();
    faces.push([0, 0, 7]);
    let mesh = TriangleMesh::try_new(points, faces).unwrap();
    let (cleaned, report) = repair(&mesh, only(|s| s.degenerate_mm = Some(0.0)));
    assert_eq!(report.degenerate.unwrap().needles_removed, 1);
    assert_eq!(cleaned.unwrap().triangles().len(), 12);
    // An edge of 0.1 µm at corner 0 collapses at 1 µm, giving back the cube.
    let mesh = split_bottom_edge(0.0001, 0.0);
    let (unchanged, report) = repair(&mesh, only(|s| s.degenerate_mm = Some(0.00001)));
    assert!(unchanged.is_none());
    assert_eq!(report.degenerate.unwrap().left, 0);
    let (collapsed, report) = repair(&mesh, only(|s| s.degenerate_mm = Some(0.001)));
    let collapsed = collapsed.unwrap();
    let d = report.degenerate.unwrap();
    assert_eq!((d.edges_collapsed, d.left), (1, 0));
    assert!((d.max_displacement_mm - 0.0001).abs() < 1e-12);
    assert_eq!(
        (collapsed.vertices().len(), collapsed.triangles().len()),
        (8, 12)
    );
    assert!(closed(&collapsed));
    let (cube_points, _) = cube();
    assert_eq!(collapsed.vertices(), &cube_points[..]);
}

#[test]
fn flat_faces_flip_with_their_neighbour_without_moving_a_vertex() {
    // The front side split at a vertex 0.1 µm above the bottom edge's midpoint, the bottom not:
    // the gap is closed by a flat face (apex 0.1 µm from its longest edge).
    let (mut points, faces) = cube();
    points.push(Point3::new(5.0, 0.0, 0.0001));
    let mut faces: Vec<[u32; 3]> = faces.into_iter().filter(|t| *t != [0, 1, 5]).collect();
    faces.extend([[0, 8, 5], [8, 1, 5], [0, 1, 8]]);
    let mesh = TriangleMesh::try_new(points, faces).unwrap();
    assert!(closed(&mesh));
    let (flipped, report) = repair(&mesh, only(|s| s.degenerate_mm = Some(0.001)));
    let flipped = flipped.unwrap();
    let d = report.degenerate.as_ref().unwrap();
    assert_eq!((d.caps_flipped, d.edges_collapsed, d.left), (1, 0, 0));
    assert!((d.max_cap_height_mm - 0.0001).abs() < 1e-9);
    assert_eq!(flipped.vertices(), mesh.vertices());
    assert!(closed(&flipped));
    assert_eq!(defects(&flipped).non_manifold_edges, 0);
    let samples: Vec<_> = report
        .all_samples()
        .filter(|s| s.change == RepairChange::CapFlipped)
        .map(|s| s.point)
        .collect();
    assert_eq!(samples, [[5.0, 0.0, 0.0001]]);
}

#[test]
fn cracks_within_the_tolerance_zip_instead_of_being_capped() {
    // As above without the flat face: a sliver hole whose middle vertex is 0.1 µm off the line.
    let (mut points, faces) = cube();
    points.push(Point3::new(5.0, 0.0, 0.0001));
    let mut faces: Vec<[u32; 3]> = faces.into_iter().filter(|t| *t != [0, 1, 5]).collect();
    faces.extend([[0, 8, 5], [8, 1, 5]]);
    let mesh = TriangleMesh::try_new(points, faces).unwrap();
    let fill = |crack_mm| {
        only(|s| {
            s.fill_holes = Some(FillHoles::default());
            s.crack_mm = crack_mm;
        })
    };
    // Exactly: a sliver cap of 0.0005 mm².
    let (capped, report) = repair(&mesh, fill(None));
    let f = report.fill.unwrap();
    assert_eq!((f.holes_filled, f.cracks_closed), (1, 0));
    assert!(closed(&capped.unwrap()));
    // Within 1 µm: the bottom face is split at the off-line vertex instead.
    let (zipped, report) = repair(&mesh, fill(Some(0.001)));
    let f = report.fill.unwrap();
    assert_eq!((f.holes_filled, f.cracks_closed), (0, 1));
    assert!((f.largest_crack_offset_mm - 0.0001).abs() < 1e-9);
    let zipped = zipped.unwrap();
    assert!(closed(&zipped));
    assert_eq!(zipped.vertices(), mesh.vertices());
    // Below the offset, the crack is capped as before.
    let (_, report) = repair(&mesh, fill(Some(0.00001)));
    assert_eq!(report.fill.unwrap().holes_filled, 1);
}

#[test]
fn a_crack_in_a_knot_of_touching_faces_closes_into_a_balanced_edge() {
    // The crack of a zero-area cap at a T-junction (vertex 8 on the bottom edge), and a
    // tetrahedron touching the cube from below along the segment from 8 to corner 3, the edge
    // that splitting the bottom face creates. That edge then has four faces, two each way.
    let (mut points, faces) = cube();
    points.push(Point3::new(5.0, 0.0, 0.0));
    let mut faces: Vec<[u32; 3]> = faces.into_iter().filter(|t| *t != [0, 1, 5]).collect();
    faces.extend([[0, 8, 5], [8, 1, 5], [0, 1, 8]]);
    points.extend([Point3::new(6.0, 7.0, -4.0), Point3::new(9.0, 3.0, -4.0)]);
    let tetra = [[8u32, 3, 9], [3, 10, 9], [8, 10, 3], [8, 9, 10]];
    let volume = |t: [u32; 3]| {
        let [a, b, c] = t.map(|v| points[v as usize]);
        a.x * (b.y * c.z - b.z * c.y)
            + a.y * (b.z * c.x - b.x * c.z)
            + a.z * (b.x * c.y - b.y * c.x)
    };
    let outward = tetra.iter().map(|&t| volume(t)).sum::<f64>() > 0.0;
    faces.extend(
        tetra
            .iter()
            .map(|&[a, b, c]| if outward { [a, b, c] } else { [a, c, b] }),
    );
    let mesh = TriangleMesh::try_new(points, faces).unwrap();
    let (closed_mesh, report) = repair(&mesh, RepairPreset::Exact.spec());
    let f = report.fill.unwrap();
    assert_eq!((f.cracks_closed, f.holes_refused), (1, 0));
    let d = defects(&closed_mesh.unwrap());
    assert_eq!(
        (
            d.boundary_edges,
            d.unbalanced_non_manifold_edges,
            d.excluded_triangles,
            d.non_manifold_edges
        ),
        (0, 0, 0, 1)
    );
}

#[test]
fn tolerances_are_checked_and_recorded_only_when_set() {
    for spec in [
        only(|s| s.weld_mm = Some(-0.001)),
        only(|s| s.weld_mm = Some(MAX_TOLERANCE_MM * 2.0)),
        only(|s| s.degenerate_mm = Some(f64::NAN)),
        only(|s| s.crack_mm = Some(0.001)),
    ] {
        let (points, faces) = cube();
        let mesh = TriangleMesh::try_new(points, faces).unwrap();
        assert!(matches!(
            repair_source(
                &mesh,
                spec,
                TopologyLimits::default(),
                &mut Cancellation::never()
            ),
            Err(RepairError::Invalid(_))
        ));
    }
    let record = |spec: RepairSpec| {
        PreparationPlan::new(
            SourceBinding {
                original_sha256: [1; 32],
                placement: ObjectTransform::identity(),
            },
            SamplingSpec {
                cell_mm: 0.1,
                distance_error_mm: 0.001,
                max_cells: 100_000,
            },
            vec![Operation {
                id: 1,
                kind: OperationKind::Repair(spec),
            }],
            PlanLimits::default(),
            &mut Cancellation::never(),
        )
        .map(|plan| plan.canonical_record().to_vec())
    };
    let exact = record(RepairPreset::Exact.spec()).unwrap();
    let standard = record(RepairPreset::Standard.spec()).unwrap();
    let plain = record(RepairSpec {
        weld_mm: None,
        degenerate_mm: None,
        ..RepairPreset::Exact.spec()
    })
    .unwrap();
    // Each tolerance adds a marker and a value, the non-manifold mode a marker and a byte; none
    // leaves the earlier record unchanged.
    assert_eq!(exact.len(), plain.len() + 2 * 9);
    assert_eq!(standard.len(), plain.len() + 3 * 9 + 2);
    let aggressive = record(RepairPreset::Aggressive.spec()).unwrap();
    // Shell bounds: a marker, two values and a flag; visibility: a marker.
    assert_eq!(aggressive.len(), standard.len() + 18 + 1);
    assert_ne!(aggressive, standard);
    assert_ne!(exact, standard);
    assert!(record(only(|s| s.weld_mm = Some(2.0))).is_err());
    assert_eq!(
        RepairPreset::of(RepairPreset::Standard.spec()),
        Some(RepairPreset::Standard)
    );
}

#[test]
fn welding_leaves_closed_surfaces_alone_however_close() {
    // Two closed cubes 0.5 µm apart: welding at 1 µm must not pinch them together.
    let (points, faces) = cube();
    let mut all_points = points.clone();
    all_points.extend(points.iter().map(|p| Point3::new(p.x + 10.0005, p.y, p.z)));
    let mut all_faces = faces.clone();
    all_faces.extend(faces.iter().map(|f| f.map(|v| v + 8)));
    let mesh = TriangleMesh::try_new(all_points, all_faces).unwrap();
    let (unchanged, report) = repair(&mesh, only(|s| s.weld_mm = Some(0.001)));
    assert!(unchanged.is_none());
    assert_eq!(report.weld.unwrap().clusters, 0);
}
