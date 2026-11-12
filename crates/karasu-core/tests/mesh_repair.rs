//! Explicit mesh repairs (TD-054, phase 2): duplicate collapse, consistent orientation and outward
//! orientation, checked against the meshes they must restore.
use karasu_core::{
    cancellation::Cancellation,
    geometry::Point3,
    mesh::{
        repair::{RepairPolicy, RepairReport},
        stl::{import_stl_path, StlOptions, StlUnitPolicy},
        topology::TopologyLimits,
        TriangleMesh,
    },
};
use std::path::Path;

const ALL: RepairPolicy = RepairPolicy {
    duplicates: true,
    orientation: true,
    outward: true,
};
const DEFAULT: RepairPolicy = RepairPolicy {
    duplicates: true,
    orientation: true,
    outward: false,
};

fn repair(mesh: &TriangleMesh, policy: RepairPolicy) -> (Option<TriangleMesh>, RepairReport) {
    mesh.repair_with_cancellation(
        policy,
        TopologyLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap()
}

/// Outward box faces (two per side), offset by `base` vertices.
fn box_faces(base: u32) -> Vec<[u32; 3]> {
    let quads = [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ];
    quads
        .iter()
        .flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]])
        .map(|f: [u32; 3]| f.map(|v| v + base))
        .collect()
}

fn box_points(min: [f64; 3], max: [f64; 3]) -> Vec<Point3> {
    (0..8)
        .map(|i| {
            Point3::new(
                if i & 1 == 0 { min[0] } else { max[0] },
                if i & 2 == 0 { min[1] } else { max[1] },
                if i & 4 == 0 { min[2] } else { max[2] },
            )
        })
        .collect()
}

fn flipped(face: [u32; 3]) -> [u32; 3] {
    [face[0], face[2], face[1]]
}

fn corpus(name: &str) -> TriangleMesh {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/reference/stl")
        .join(name);
    import_stl_path(path, StlOptions::new(StlUnitPolicy::AssumeMillimeters))
        .unwrap()
        .mesh
}

#[test]
fn clean_meshes_are_left_alone() {
    for name in [
        "fx-01-cube.stl",
        "fx-04-hollow-box.stl",
        "fx-06-multiple-volumes.stl",
    ] {
        let (derived, report) = repair(&corpus(name), DEFAULT);
        assert!(derived.is_none() && !report.changed(), "{name}");
    }
    let (derived, report) = repair(&corpus("fx-01-cube.stl"), RepairPolicy::default());
    assert!(derived.is_none() && report == RepairReport::default());
}

#[test]
fn corpus_defects_are_repaired_to_the_clean_cube() {
    let cube = corpus("fx-01-cube.stl");
    // FX-09: exactly its one reversed face is flipped back, leaving a clean closed cube.
    let source = corpus("fx-09-reversed-face.stl");
    let (fixed, report) = repair(&source, DEFAULT);
    assert_eq!((report.flipped_faces, report.reoriented_components), (1, 1));
    let fixed = fixed.unwrap();
    let changed: Vec<usize> = (0..12)
        .filter(|&f| fixed.triangles()[f] != source.triangles()[f])
        .collect();
    assert_eq!(changed.len(), 1);
    assert_eq!(
        fixed.triangles()[changed[0]],
        flipped(source.triangles()[changed[0]])
    );
    let defects = fixed
        .defect_report_with_cancellation(TopologyLimits::default(), &mut Cancellation::never())
        .unwrap();
    assert_eq!(
        [defects.winding_conflicts, defects.closed_components],
        [0, 1]
    );
    assert_eq!(cube.triangles().len(), fixed.triangles().len());
    // FX-08: the duplicate face is dropped; the degenerate faces stay.
    let (fixed, report) = repair(&corpus("fx-08-degenerate-faces.stl"), DEFAULT);
    assert_eq!(report.dropped_duplicate_faces, 1);
    let defects = fixed
        .unwrap()
        .defect_report_with_cancellation(TopologyLimits::default(), &mut Cancellation::never())
        .unwrap();
    assert_eq!(
        [
            defects.non_manifold_edges,
            defects.same_orientation_duplicates,
            defects.excluded_triangles
        ],
        [0, 0, 2]
    );
}

#[test]
fn duplicate_groups_collapse_to_their_net_orientation() {
    let points = box_points([0., 0., 0.], [2., 2., 2.]);
    let faces = box_faces(0);
    // Two extra copies of face 0 and one reversed copy: net +2 copies of the original; one face
    // (the original, earliest) survives.
    let mut doubled = faces.clone();
    doubled.extend([faces[0], flipped(faces[0]), faces[0]]);
    let (fixed, report) = repair(
        &TriangleMesh::try_new(points.clone(), doubled).unwrap(),
        DEFAULT,
    );
    assert_eq!(
        (
            report.dropped_duplicate_faces,
            report.cancelled_duplicate_groups
        ),
        (3, 0)
    );
    assert_eq!(fixed.unwrap().triangles(), faces.as_slice());
    // Two boxes glued along a shared face, each with its own copy (opposite orientations): the
    // internal double wall cancels and the union is one closed box.
    // Shared positions are welded, as STL import does.
    let mut points: Vec<Point3> = Vec::new();
    let mut glued = Vec::new();
    for corner in [[0., 0., 0.], [2., 0., 0.]] {
        let body = box_points(corner, [corner[0] + 2., 2., 2.]);
        let ids: Vec<u32> = body
            .iter()
            .map(|p| {
                let found = points.iter().position(|q| q == p);
                u32::try_from(found.unwrap_or_else(|| {
                    points.push(*p);
                    points.len() - 1
                }))
                .unwrap()
            })
            .collect();
        glued.extend(box_faces(0).into_iter().map(|f| f.map(|v| ids[v as usize])));
    }
    let (fixed, report) = repair(&TriangleMesh::try_new(points, glued).unwrap(), DEFAULT);
    assert_eq!(
        (
            report.dropped_duplicate_faces,
            report.cancelled_duplicate_groups
        ),
        (4, 2)
    );
    let defects = fixed
        .unwrap()
        .defect_report_with_cancellation(TopologyLimits::default(), &mut Cancellation::never())
        .unwrap();
    assert_eq!(
        [
            defects.boundary_edges,
            defects.winding_conflicts,
            defects.closed_components
        ],
        [0, 0, 1]
    );
}

#[test]
fn random_flips_are_undone_by_the_larger_area_class() {
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    let mut next = |m: u64| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state % m
    };
    // Two separate boxes of different sizes; each keeps its own majority orientation.
    let mut points = box_points([0., 0., 0.], [4., 3., 2.]);
    points.extend(box_points([10., 0., 0.], [11., 1., 1.]));
    let mut clean = box_faces(0);
    clean.extend(box_faces(8));
    for case in 0..200 {
        let mut faces = clean.clone();
        // Flip a random minority of each box's area.
        let flips: Vec<usize> = (0..24).filter(|_| next(4) == 0).collect();
        for &f in &flips {
            faces[f] = flipped(faces[f]);
        }
        let mesh = TriangleMesh::try_new(points.clone(), faces.clone()).unwrap();
        let (fixed, report) = repair(&mesh, ALL);
        let fixed = fixed.unwrap_or(mesh);
        let defects = fixed
            .defect_report_with_cancellation(TopologyLimits::default(), &mut Cancellation::never())
            .unwrap();
        assert_eq!(defects.winding_conflicts, 0, "case {case}");
        // With `outward`, both closed boxes end outward: exactly the clean faces.
        assert_eq!(
            fixed.triangles(),
            clean.as_slice(),
            "case {case} {report:?}"
        );
    }
}

#[test]
fn outward_turns_outermost_shells_and_keeps_nested_cavities() {
    // An inside-out box: consistent, so only `outward` changes it.
    let points = box_points([0., 0., 0.], [2., 3., 4.]);
    let inverted: Vec<_> = box_faces(0).into_iter().map(flipped).collect();
    let mesh = TriangleMesh::try_new(points.clone(), inverted).unwrap();
    let (unchanged, report) = repair(&mesh, DEFAULT);
    assert!(unchanged.is_none() && !report.changed());
    let (fixed, report) = repair(&mesh, ALL);
    assert_eq!(
        (report.outward_flipped_components, report.flipped_faces),
        (1, 12)
    );
    assert_eq!(fixed.unwrap().triangles(), box_faces(0).as_slice());
    // FX-04: the reversed cavity is nested inside the outer shell's bounds and stays reversed.
    let (fixed, report) = repair(&corpus("fx-04-hollow-box.stl"), ALL);
    assert!(fixed.is_none());
    assert_eq!(report.nested_reversed_components, 1);
}
