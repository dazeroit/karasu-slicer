//! Non-manifold edges (M6.4d r4, TD-092, stage R5): fins removed, bodies kept.
use karasu_core::{
    cancellation::Cancellation,
    geometry::Point3,
    mesh::{
        repair::RepairChange,
        stl::{import_stl_path, StlOptions, StlUnitPolicy},
        topology::{DefectReport, TopologyLimits},
        TriangleMesh,
    },
    preparation::repair::{
        repair_source, NonManifold, NonManifoldReport, RepairPreset, RepairSpec,
    },
};
use std::path::Path;

fn defects(mesh: &TriangleMesh) -> DefectReport {
    mesh.defect_report_with_cancellation(TopologyLimits::default(), &mut Cancellation::never())
        .unwrap()
}

fn repair(mesh: &TriangleMesh, mode: NonManifold) -> (Option<TriangleMesh>, NonManifoldReport) {
    let spec = RepairSpec {
        non_manifold: Some(mode),
        ..RepairPreset::Off.spec()
    };
    let (repaired, report) = repair_source(
        mesh,
        spec,
        TopologyLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap();
    (repaired, report.non_manifold.unwrap())
}

/// A 10 mm cube's corners and outward faces, without the faces listed in `skip`.
fn cube(skip: &[usize]) -> (Vec<Point3>, Vec<[u32; 3]>) {
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
    .enumerate()
    .filter(|(i, _)| !skip.contains(i))
    .map(|(_, t)| t)
    .collect();
    (points, faces)
}

/// A square sheet of two faces hanging from the cube's bottom front edge (vertices 0 and 1),
/// out to `y = -reach`, below the bottom.
fn with_sheet(skip: &[usize], reach: f64) -> TriangleMesh {
    let (mut points, mut faces) = cube(skip);
    points.extend([
        Point3::new(0.0, -reach, -reach),
        Point3::new(10.0, -reach, -reach),
    ]);
    faces.extend([[0, 1, 9], [0, 9, 8]]);
    TriangleMesh::try_new(points, faces).unwrap()
}

#[test]
fn the_flap_of_fx_10b_is_removed_and_the_cube_closes() {
    let mesh = import_stl_path(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/reference/stl/fx-10b-non-manifold.stl"),
        StlOptions::new(StlUnitPolicy::AssumeMillimeters),
    )
    .unwrap()
    .mesh;
    let (repaired, report) = repair(&mesh, NonManifold::Fins);
    assert_eq!(
        (report.fins_removed, report.fin_faces, report.edges_left),
        (1, 1, 0)
    );
    assert!((report.fin_area_mm2 - 100.0).abs() < 1e-9);
    assert_eq!(report.samples[0].change, RepairChange::FinRemoved);
    let d = defects(&repaired.unwrap());
    assert_eq!(
        (d.boundary_edges, d.non_manifold_edges, d.closed_components),
        (0, 0, 1)
    );
}

#[test]
fn a_hanging_sheet_is_removed_whole_and_the_body_it_hangs_from_never() {
    let (repaired, report) = repair(&with_sheet(&[], 3.0), NonManifold::Fins);
    assert_eq!((report.fins_removed, report.fin_faces), (1, 2));
    let repaired = repaired.unwrap();
    assert_eq!(repaired.triangles().len(), 12);
    assert_eq!(defects(&repaired).boundary_edges, 0);
    // An open box (no top) with the sheet: the sheet goes, the box and its hole stay.
    let (repaired, report) = repair(&with_sheet(&[2, 3], 3.0), NonManifold::Fins);
    assert_eq!((report.fins_removed, report.fin_faces), (1, 2));
    let d = defects(&repaired.unwrap());
    assert_eq!((d.rim_loops, d.non_manifold_edges), (1, 0));
    // A sheet larger than the open box it hangs from is not taken for a fin: the edge is left.
    let (unchanged, report) = repair(&with_sheet(&[2, 3], 40.0), NonManifold::Fins);
    assert!(unchanged.is_none());
    assert_eq!((report.fins_removed, report.edges_left), (0, 1));
}

#[test]
fn balanced_non_manifold_edges_are_left_alone() {
    // Two cubes sharing an edge: four faces on it, two each way.
    let (mut points, mut faces) = cube(&[]);
    let base = u32::try_from(points.len()).unwrap();
    points.extend(
        cube(&[])
            .0
            .into_iter()
            .map(|p| Point3::new(p.x + 10.0, p.y + 10.0, p.z)),
    );
    // Corners 0 and 4 of the second cube are corners 3 and 7 of the first.
    let map = |v: u32| match v {
        0 => 3,
        4 => 7,
        v => v + base,
    };
    faces.extend(cube(&[]).1.into_iter().map(|t| t.map(map)));
    let mesh = TriangleMesh::try_new(points, faces).unwrap();
    assert_eq!(defects(&mesh).non_manifold_edges, 1);
    let (unchanged, report) = repair(&mesh, NonManifold::Aggressive);
    assert!(unchanged.is_none());
    assert_eq!(report, NonManifoldReport::default());
}

#[test]
fn the_aggressive_mode_cuts_the_faces_of_edges_left_unbalanced() {
    // The large sheet on the open box: fins leave the edge, the aggressive mode cuts its three
    // faces (two of the box, one of the sheet) and no unbalanced edge is left.
    let mesh = with_sheet(&[2, 3], 40.0);
    let (cut, report) = repair(&mesh, NonManifold::Aggressive);
    assert_eq!((report.fins_removed, report.faces_cut), (0, 3));
    assert_eq!(report.edges_left, 0);
    assert_eq!(defects(&cut.unwrap()).unbalanced_non_manifold_edges, 0);
    assert!(report
        .samples
        .iter()
        .all(|s| s.change == RepairChange::FacesCut));
}
