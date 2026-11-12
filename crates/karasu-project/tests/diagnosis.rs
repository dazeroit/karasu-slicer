//! Diagnosis of imported models.
mod common;

use common::box_stl;
use karasu_core::{
    cancellation::Cancellation,
    geometry::Point3,
    mesh::{formats::LengthUnit, topology::TopologyLimits, TriangleMesh},
    preparation::repair::{repair_source, RepairPreset},
};
use karasu_project::{
    diagnosis::{diagnose, Diagnosis, Finding, Handling, Printability, Treatment},
    pipeline::import_mesh,
    request::{ModelSource, DEFAULT_REPAIR},
};
use std::sync::Arc;

fn mesh(stl: String) -> TriangleMesh {
    let source = ModelSource::Embedded {
        name: "model.stl".into(),
        bytes: Arc::from(stl.into_bytes()),
    };
    import_mesh(&source, Some(LengthUnit::Millimeter), &|_| false)
        .unwrap()
        .mesh
}

/// The box's facets, each as its three vertex lines.
fn facets(stl: &str) -> Vec<[String; 3]> {
    let vertices: Vec<String> = stl
        .lines()
        .filter(|l| l.starts_with("vertex"))
        .map(str::to_owned)
        .collect();
    vertices
        .chunks(3)
        .map(|c| [c[0].clone(), c[1].clone(), c[2].clone()])
        .collect()
}

fn stl(facets: &[[String; 3]]) -> String {
    let mut text = String::from("solid model\n");
    for facet in facets {
        text.push_str("facet normal 0 0 0\nouter loop\n");
        for vertex in facet {
            text.push_str(vertex);
            text.push('\n');
        }
        text.push_str("endloop\nendfacet\n");
    }
    text + "endsolid model\n"
}

fn findings(stl: String) -> Vec<Finding> {
    diagnose(&mesh(stl), &mut Cancellation::new(&mut |_| false))
        .unwrap()
        .findings()
}

#[test]
fn a_closed_outward_box_has_no_findings() {
    assert_eq!(findings(box_stl([0.; 3], [10., 10., 10.])), []);
}

#[test]
fn inside_out_bodies_holes_and_flipped_faces_are_found() {
    let cube = facets(&box_stl([0.; 3], [10., 10., 10.]));
    // Every facet reversed: one inside-out body.
    let reversed: Vec<[String; 3]> = cube
        .iter()
        .map(|[a, b, c]| [c.clone(), b.clone(), a.clone()])
        .collect();
    let inside_out = findings(stl(&reversed));
    assert_eq!(inside_out, [Finding::InsideOut { bodies: 1 }]);
    // One facet missing: a hole with three boundary edges.
    let open = findings(stl(&cube[1..]));
    assert_eq!(open, [Finding::Holes { holes: 1, edges: 3 }]);
    // One facet reversed: inconsistent orientation in one body.
    let mut flipped = cube.clone();
    let [a, b, c] = flipped[0].clone();
    flipped[0] = [c, b, a];
    let flipped = findings(stl(&flipped));
    assert_eq!(flipped, [Finding::InconsistentOrientation { bodies: 1 }]);
    // A facet listed twice: its edges are shared by three faces until the repair removes it.
    let mut doubled = cube.clone();
    doubled.push(cube[0].clone());
    assert_eq!(
        findings(stl(&doubled)),
        [
            Finding::DuplicateFaces { faces: 1 },
            Finding::NonManifold { edges: 3 }
        ]
    );
    // A cavity (a reversed box inside another) is not a finding.
    let mut cavity = cube;
    cavity.extend(
        facets(&box_stl([3.; 3], [7., 7., 7.]))
            .into_iter()
            .map(|[a, b, c]| [c, b, a]),
    );
    assert_eq!(findings(stl(&cavity)), []);
}

#[test]
fn handling_follows_the_plate_settings() {
    let defaults = Treatment {
        repair: DEFAULT_REPAIR,
        close_holes: true,
    };
    let inside_out = Finding::InsideOut { bodies: 1 };
    assert_eq!(inside_out.handling(defaults), Handling::Unhandled);
    let mut outward = defaults;
    outward.repair.outward = true;
    assert_eq!(inside_out.handling(outward), Handling::Handled);
    let holes = Finding::Holes { holes: 1, edges: 3 };
    assert_eq!(holes.handling(defaults), Handling::Handled);
    let open = Treatment {
        close_holes: false,
        ..defaults
    };
    assert_eq!(holes.handling(open), Handling::Unhandled);
    assert_eq!(
        Finding::InconsistentOrientation { bodies: 1 }.handling(defaults),
        Handling::Handled
    );
    assert_eq!(
        Finding::NonManifold { edges: 4 }.handling(defaults),
        Handling::Harmless
    );
}

fn diagnosis(mesh: &TriangleMesh) -> Diagnosis {
    diagnose(mesh, &mut Cancellation::new(&mut |_| false)).unwrap()
}

/// A box's corners and outward faces, the corner indices offset by `base`.
fn cube(min: f64, max: f64, base: u32) -> (Vec<Point3>, Vec<[u32; 3]>) {
    let points = (0..8)
        .map(|i| {
            let at = |bit| if i & bit == 0 { min } else { max };
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
    .map(|t: [u32; 3]| t.map(|v| v + base))
    .collect();
    (points, faces)
}

#[test]
fn printability_follows_what_slicing_and_hollowing_refuse() {
    let state = |stl: String, close_holes| diagnosis(&mesh(stl)).printability(close_holes);
    let cube_facets = facets(&box_stl([0.; 3], [10., 10., 10.]));
    assert_eq!(state(stl(&cube_facets), false), Printability::Ready);
    // A hole: layer closure slices it, hollowing refuses it; without closure slicing fails.
    let open = stl(&cube_facets[1..]);
    assert_eq!(
        state(open.clone(), true),
        Printability::NeedsRepairForHollowing
    );
    assert_eq!(state(open, false), Printability::NeedsRepair);
    // Inside out: rasterizing refuses negative winding.
    let reversed: Vec<[String; 3]> = cube_facets
        .iter()
        .map(|[a, b, c]| [c.clone(), b.clone(), a.clone()])
        .collect();
    assert_eq!(state(stl(&reversed), true), Printability::NeedsRepair);
    // Bodies touching at one corner: slicing fills them by union.
    let (mut points, mut faces) = cube(0., 10., 0);
    let (more, other) = cube(10., 20., 8);
    points.extend(more);
    faces.extend(
        other
            .into_iter()
            .map(|t| t.map(|v| if v == 8 { 7 } else { v })),
    );
    let pinched = diagnosis(&TriangleMesh::try_new(points, faces).unwrap());
    assert_eq!(
        pinched.findings(),
        [Finding::PinchedVertices { vertices: 1 }]
    );
    assert_eq!(pinched.printability(false), Printability::ReadyWithNotes);
}

#[test]
fn a_crack_with_its_cap_is_ready_with_notes_and_exact_repair_makes_it_ready() {
    // The front side split at the bottom edge's midpoint (vertex 8), the bottom not, with the
    // zero-area cap an exporter leaves in the crack.
    let (mut points, faces) = cube(0., 10., 0);
    points.push(Point3::new(5., 0., 0.));
    let mut faces: Vec<[u32; 3]> = faces.into_iter().filter(|t| *t != [0, 1, 5]).collect();
    faces.extend([[0, 8, 5], [8, 1, 5], [0, 1, 8]]);
    let mesh = TriangleMesh::try_new(points, faces).unwrap();
    let found = diagnosis(&mesh);
    assert_eq!(
        found.findings(),
        [
            Finding::Holes { holes: 1, edges: 3 },
            Finding::Cracks { cracks: 1 },
            Finding::DegenerateFaces {
                needles: 0,
                caps: 1
            },
        ]
    );
    // Layer closure slices it and hollowing leaves the crack out; without closure slicing fails.
    assert_eq!(found.printability(true), Printability::ReadyWithNotes);
    assert_eq!(found.printability(false), Printability::NeedsRepair);
    let (repaired, _) = repair_source(
        &mesh,
        RepairPreset::Exact.spec(),
        TopologyLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap();
    let repaired = diagnosis(&repaired.unwrap());
    assert_eq!(repaired.findings(), []);
    assert_eq!(repaired.printability(false), Printability::Ready);
}

#[test]
fn fins_need_repair_and_duplicate_faces_are_not_taken_for_fins() {
    // FX-10B: a cube with a flap folded onto its front face along the bottom edge.
    let source = ModelSource::File(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/reference/stl/fx-10b-non-manifold.stl"),
    );
    let mesh = import_mesh(&source, Some(LengthUnit::Millimeter), &|_| false)
        .unwrap()
        .mesh;
    let found = diagnosis(&mesh);
    assert!(found.findings().contains(&Finding::Fins { edges: 1 }));
    assert_eq!(found.printability(true), Printability::NeedsRepair);
    let defaults = Treatment {
        repair: DEFAULT_REPAIR,
        close_holes: true,
    };
    assert_eq!(
        Finding::Fins { edges: 1 }.handling(defaults),
        Handling::Unhandled
    );
    let standard = Treatment {
        repair: RepairPreset::Standard.spec(),
        close_holes: true,
    };
    assert_eq!(
        Finding::Fins { edges: 1 }.handling(standard),
        Handling::Handled
    );
    // A face listed twice makes three-face edges, which duplicate collapse resolves: no fins.
    let cube = facets(&box_stl([0.; 3], [10., 10., 10.]));
    let mut doubled = cube.clone();
    doubled.push(cube[0].clone());
    let found = diagnose(&mesh_of(&doubled), &mut Cancellation::new(&mut |_| false)).unwrap();
    assert_eq!(found.fins, 0);
}

fn mesh_of(facets: &[[String; 3]]) -> TriangleMesh {
    mesh(stl(facets))
}

#[test]
fn facets_dropped_at_import_are_a_note_that_exact_repair_accepts() {
    let text = box_stl([0., 0., 0.], [10., 10., 10.]).replace(
        "endsolid box",
        "facet normal 0 0 0\nouter loop\nvertex 0 0 inf\nvertex 1 0 0\nvertex 0 1 0\nendloop\n\
         endfacet\nendsolid box",
    );
    let source = ModelSource::Embedded {
        name: "model.stl".into(),
        bytes: Arc::from(text.into_bytes()),
    };
    let imported = import_mesh(&source, Some(LengthUnit::Millimeter), &|_| false).unwrap();
    let dropped = imported.dropped.unwrap();
    assert_eq!(
        (dropped.facets, dropped.ranges.as_slice()),
        (1, &[(12, 1)][..])
    );
    // The box alone is ready; with the dropped facet it is ready with a note.
    let clean = diagnosis(&imported.mesh);
    assert_eq!(clean.printability(false), Printability::Ready);
    let noted = Diagnosis {
        dropped_facets: dropped.facets,
        ..clean
    };
    assert_eq!(noted.printability(false), Printability::ReadyWithNotes);
    let finding = Finding::DroppedFacets { facets: 1 };
    assert_eq!(noted.findings(), [finding]);
    // Jobs refuse it unless the repairs accept it, as Exact does.
    let treatment = |repair| Treatment {
        repair,
        close_holes: false,
    };
    assert_eq!(
        finding.handling(treatment(DEFAULT_REPAIR)),
        Handling::Unhandled
    );
    assert_eq!(
        finding.handling(treatment(RepairPreset::Exact.spec())),
        Handling::Handled
    );
}
