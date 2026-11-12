//! Rebuilding a model as a solid (M6.4d r7, TD-092, stage R9).
use karasu_core::{
    cancellation::Cancellation,
    geometry::Point3,
    mesh::{
        stl::{import_stl_path, StlOptions, StlUnitPolicy},
        topology::{DefectReport, TopologyLimits},
        TriangleMesh,
    },
    preparation::{
        repair::{repair_source, self_intersections, RepairPreset, RepairSpec},
        sampled::{rebuild_solid, RebuildReport, SampledHollowLimits},
    },
};
use std::path::Path;

fn boxes(specs: &[([f64; 3], [f64; 3])]) -> TriangleMesh {
    let mut points = Vec::new();
    let mut faces = Vec::new();
    for &(min, max) in specs {
        let base = u32::try_from(points.len()).unwrap();
        for i in 0..8 {
            let at = |bit: usize, axis: usize| if i & bit == 0 { min[axis] } else { max[axis] };
            points.push(Point3::new(at(1, 0), at(2, 1), at(4, 2)));
        }
        for q in [
            [0, 2, 3, 1],
            [4, 5, 7, 6],
            [0, 1, 5, 4],
            [2, 6, 7, 3],
            [0, 4, 6, 2],
            [1, 3, 7, 5],
        ] {
            faces.push([q[0] + base, q[1] + base, q[2] + base]);
            faces.push([q[0] + base, q[2] + base, q[3] + base]);
        }
    }
    TriangleMesh::try_new(points, faces).unwrap()
}

fn rebuild(mesh: &TriangleMesh, resolution: f64) -> (TriangleMesh, RebuildReport) {
    rebuild_solid(
        mesh,
        resolution,
        SampledHollowLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap()
}

fn defects(mesh: &TriangleMesh) -> DefectReport {
    mesh.defect_report_with_cancellation(TopologyLimits::default(), &mut Cancellation::never())
        .unwrap()
}

fn closed_manifold(mesh: &TriangleMesh) -> bool {
    let d = defects(mesh);
    d.boundary_edges == 0
        && d.non_manifold_edges == 0
        && d.winding_conflicts == 0
        && d.excluded_triangles == 0
        && d.non_manifold_vertices == 0
}

#[test]
fn a_box_is_rebuilt_closed_outward_and_within_its_deviation() {
    let source = boxes(&[([0.0; 3], [10.0; 3])]);
    let (solid, report) = rebuild(&source, 0.5);
    assert!(closed_manifold(&solid));
    assert_eq!(report.surfaces, 1);
    // Outward, about the box's volume (edges and corners are rounded within the deviation).
    assert!((report.volume_mm3[0] - 1000.0).abs() < 1e-9);
    assert!(
        report.volume_mm3[1] > 950.0 && report.volume_mm3[1] < 1000.0,
        "{report:?}"
    );
    // Both deviations within about two cells.
    assert!(report.max_deviation_mm < 1.0, "{report:?}");
    assert!(report.source_deviation_mm < 1.0, "{report:?}");
    // Every rebuilt vertex lies within the reported deviation of the box's surface.
    for p in solid.vertices() {
        let inside = [p.x, p.y, p.z].map(|c| c.min(10.0 - c));
        let outside = [p.x, p.y, p.z].map(|c| (-c).max(c - 10.0).max(0.0));
        let distance = if outside.iter().all(|&o| o == 0.0) {
            inside.iter().copied().fold(f64::INFINITY, f64::min)
        } else {
            outside.iter().map(|o| o * o).sum::<f64>().sqrt()
        };
        assert!(distance <= report.max_deviation_mm, "{p:?} {distance}");
    }
}

#[test]
fn overlapping_bodies_become_one_shell_without_self_intersections() {
    let source = boxes(&[
        ([0.0; 3], [20.0; 3]),
        ([10.0, 5.0, 5.0], [30.0, 15.0, 15.0]),
    ]);
    let (solid, report) = rebuild(&source, 0.5);
    assert!(closed_manifold(&solid));
    assert_eq!(report.surfaces, 1);
    let pairs = self_intersections(&solid, 10, &mut Cancellation::never())
        .unwrap()
        .pairs;
    assert_eq!(pairs, 0);
    // The union: 8000 + 2000 - 1000 mm³, less the rounding of its edges.
    assert!(
        report.volume_mm3[1] > 8800.0 && report.volume_mm3[1] < 9000.0,
        "{report:?}"
    );
    // The source's volume counts the overlap twice.
    assert!((report.volume_mm3[0] - 10000.0).abs() < 1e-9);
}

#[test]
fn a_feature_thinner_than_the_resolution_is_lost_and_reported() {
    // A box and, apart from it, a 0.1 mm thin plate: at 0.5 mm the plate vanishes.
    let source = boxes(&[([0.0; 3], [10.0; 3]), ([20.0, 0.0, 0.0], [25.0, 5.0, 0.1])]);
    let (solid, report) = rebuild(&source, 0.5);
    assert!(closed_manifold(&solid));
    assert_eq!(report.surfaces, 1);
    assert!(report.source_deviation_mm > 2.0, "{report:?}");
    let at = report.largest_deviation_at.unwrap();
    assert!(at[0] >= 20.0, "{at:?}");
}

#[test]
fn surfaces_without_a_winding_number_and_bad_resolutions_are_refused() {
    let (points, mut faces) = {
        let open = boxes(&[([0.0; 3], [10.0; 3])]);
        (open.vertices().to_vec(), open.triangles().to_vec())
    };
    faces.truncate(10);
    let open = TriangleMesh::try_new(points, faces).unwrap();
    let error = rebuild_solid(
        &open,
        0.5,
        SampledHollowLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("winding number"), "{error}");
    let cube = boxes(&[([0.0; 3], [10.0; 3])]);
    for resolution in [0.0, 0.0005, 11.0, f64::NAN] {
        assert!(rebuild_solid(
            &cube,
            resolution,
            SampledHollowLimits::default(),
            &mut Cancellation::never()
        )
        .is_err());
    }
}

#[test]
fn every_tracked_fixture_becomes_closed_and_manifold_with_the_rebuild_preset() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/reference/stl");
    let spec = RepairSpec {
        rebuild_mm: Some(0.5),
        ..RepairPreset::Rebuild.spec()
    };
    for name in [
        "fx-01-cube.stl",
        "fx-02-cylinder.stl",
        "fx-03-sphere.stl",
        "fx-04-hollow-box.stl",
        "fx-05-rotated-prism.stl",
        "fx-06-multiple-volumes.stl",
        "fx-07-internal-cavity.stl",
        "fx-08-degenerate-faces.stl",
        "fx-09-reversed-face.stl",
        "fx-10a-open.stl",
        "fx-10b-non-manifold.stl",
    ] {
        let mesh = import_stl_path(
            directory.join(name),
            StlOptions::new(StlUnitPolicy::AssumeMillimeters),
        )
        .unwrap()
        .mesh;
        let (solid, report) = repair_source(
            &mesh,
            spec,
            TopologyLimits::default(),
            &mut Cancellation::never(),
        )
        .unwrap_or_else(|e| panic!("{name}: {e}"));
        let solid = solid.unwrap();
        assert!(closed_manifold(&solid), "{name}: {:?}", defects(&solid));
        // The certified bound widens exact vertex distances by each triangle's size: about two
        // cells at sharp oblique edges.
        let rebuilt = report.rebuild.unwrap();
        assert!(rebuilt.max_deviation_mm < 1.25, "{name}: {rebuilt:?}");
    }
}

#[test]
fn results_are_identical_for_any_thread_count() {
    let source = boxes(&[([0.0; 3], [10.0; 3]), ([5.0, 2.0, 2.0], [15.0, 8.0, 8.0])]);
    let run = |threads| {
        rebuild_solid(
            &source,
            0.4,
            SampledHollowLimits {
                threads,
                ..SampledHollowLimits::default()
            },
            &mut Cancellation::never(),
        )
        .unwrap()
    };
    let (one, report) = run(1);
    let (many, again) = run(8);
    assert_eq!(one, many);
    assert_eq!(report, again);
}
