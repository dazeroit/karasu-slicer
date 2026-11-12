//! Preparation for display (TD-082): every object prepared as slicing prepares it, with support
//! findings kept instead of refused, one result per object, in the background.
mod common;

use common::{box_stl, material, print_setting, printer};
use karasu_core::{
    preparation::sampled::HoleSurface,
    supports::{FindingKind, Severity, SupportDocument},
};
use karasu_project::{
    jobs::{JobError, PreparationJob},
    pipeline::{prepare, slice, Silent},
    project::{
        Hollowing, OperationKind, PlateContent, PlateId, PlateObject, PlateSettings, Project,
    },
    request::{DrainHole, Pose},
};

/// A slab 5 mm above the plate on two vertical supports, plus `extra` supports JSON.
fn supports(extra: &str) -> SupportDocument {
    SupportDocument::from_json(
        format!(
            r#"{{"format":"karasu-supports","version":1,
                "supports":[{{"id":1,"tip":{{"point":[15,25,5]}},"base":{{"vertical":true}}}},
                            {{"id":2,"tip":{{"point":[25,25,5]}},"base":{{"vertical":true}}}}{extra}]}}"#
        )
        .as_bytes(),
    )
    .unwrap()
}

/// Three objects: the supported slab, a hollowed and drained box, and a box lifted off the plate
/// without supports.
fn plate(extra: &str) -> (Project, PlateId) {
    let mut project = Project::new("Preparation");
    let slab = project.add_model(
        "slab.stl",
        box_stl([10., 20., 5.], [30., 30., 7.]).into_bytes(),
    );
    let block = project.add_model(
        "block.stl",
        box_stl([50., 15., 0.], [70., 35., 10.]).into_bytes(),
    );
    let mut supported = PlateObject::new("slab", slab);
    supported.pose = Pose::default();
    supported.arrange = false;
    supported.supports = Some(supports(extra));
    let mut hollowed = PlateObject::new("block", block);
    hollowed.arrange = false;
    hollowed.set_hollowing(Some(Hollowing {
        wall_mm: 2.,
        cell_mm: Some(0.5),
        simplify: None,
    }));
    // The block's model frame is its world frame: it rests on the plate where it was drawn.
    hollowed.push_operation(OperationKind::DrainHole(DrainHole::Surface {
        surface_mm: [60., 35., 5.],
        diameter_mm: 3.,
        outside_mm: 1.,
        inside_mm: 1.,
        tilt: None,
        depth_mm: None,
        surface: HoleSurface::Outer,
    }));
    let mut lifted = PlateObject::new("lifted", block);
    lifted.arrange = false;
    lifted.pose = Pose::default();
    lifted.translate_mm = [0., 0., 3.];
    lifted.copies = 1;
    let plate = project.add_plate(PlateContent {
        name: "Plate".into(),
        printer: Some(printer()),
        resin: Some(material()),
        print_setting: Some(print_setting()),
        settings: PlateSettings::new("goo-v3"),
        objects: vec![supported, hollowed, lifted],
    });
    (project, plate)
}

#[test]
fn each_object_is_prepared_on_its_own_with_its_findings() {
    // A third support whose tip is in the air, above the slab.
    let (project, plate) = plate(r#",{"id":3,"tip":{"point":[20,25,9]},"base":{"vertical":true}}"#);
    let (request, _) = project
        .request(plate, std::env::temp_dir().join("unused.goo"))
        .unwrap();
    let preparation = prepare(&request, &mut Silent, &|_| false).unwrap();
    assert_eq!(preparation.objects.len(), 3);
    // The slab: its supports are built, and the bad one is a finding, not a failure.
    let slab = preparation.objects[0].as_ref().unwrap();
    assert!(slab.hollowed().is_none());
    let scene = slab.supports().unwrap();
    assert_eq!(scene.supports().len(), 3);
    assert!(scene.has_errors());
    assert!(scene.findings().iter().any(|f| f.entity == 3
        && f.severity == Severity::Error
        && f.kind == FindingKind::AnchorOffSurface));
    assert!(!scene
        .findings()
        .iter()
        .any(|f| f.entity != 3 && f.severity == Severity::Error));
    // The block: hollowed with a certified wall, drained by its one hole.
    let block = preparation.objects[1].as_ref().unwrap();
    let hollow = block.hollowed().unwrap();
    assert!(
        hollow.certificate().surface_distance_mm[0] >= 1.5,
        "{:?}",
        hollow.certificate()
    );
    assert_ne!(hollow.cavity().triangles().len(), 0);
    let holes = block.holes().unwrap();
    assert_eq!(holes.specs().len(), 1);
    assert_eq!(holes.enclosed(), 0);
    assert!(holes.findings()[0].opens_outside && holes.findings()[0].reaches_cavity);
    assert!(block.supports().is_none());
    // The lifted box is prepared where it is (an interface shows it while it is moved).
    assert!(preparation.objects[2].is_ok());
    // Slicing the same request refuses the plate (the lifted object alone: see `operations`).
    assert!(slice(&request, &mut Silent, &|_| false).is_err());
}

#[test]
fn preparation_runs_in_the_background_and_can_be_cancelled() {
    let (project, plate) = plate("");
    let (request, revision) = project
        .request(plate, std::env::temp_dir().join("unused.goo"))
        .unwrap();
    let finished = PreparationJob::start(plate, revision, request.clone()).wait();
    assert_eq!((finished.plate, finished.revision), (plate, revision));
    let preparation = finished.result.unwrap();
    let slab = preparation.objects[0].as_ref().unwrap();
    assert!(!slab.supports().unwrap().has_errors());
    let job = PreparationJob::start(plate, revision, request);
    job.cancel();
    assert!(matches!(job.wait().result, Err(JobError::Cancelled)));
}

#[test]
fn a_refused_hole_keeps_the_cavity_and_the_probe() {
    let (mut project, plate) = plate("");
    // The block's hole moved near its edge, beyond the hole's radius from it: its axis passes
    // beside the cavity, which starts 2 mm in, so its automatic depth finds none.
    project
        .edit_plate(plate, |content| {
            let (_, hole) = content.objects[1].drain_holes_mut().next().unwrap();
            *hole = DrainHole::Surface {
                surface_mm: [51.6, 35., 5.],
                diameter_mm: 3.,
                outside_mm: 1.,
                inside_mm: 1.,
                tilt: None,
                depth_mm: None,
                surface: HoleSurface::Outer,
            };
        })
        .unwrap();
    let (request, _) = project
        .request(plate, std::env::temp_dir().join("unused.goo"))
        .unwrap();
    let preparation = prepare(&request, &mut Silent, &|_| false).unwrap();
    let block = preparation.objects[1].as_ref().unwrap();
    assert!(block.holes().is_none());
    assert!(block.hollowed().is_some());
    let refusal = block.drainage_error().unwrap();
    assert!(refusal.reason().contains("drain hole 1"), "{refusal}");
    // The probe still resolves a hole at the face's centre, and not at the edge.
    let probe = block.probe().unwrap();
    let centre = DrainHole::Surface {
        surface_mm: [60., 35., 5.],
        diameter_mm: 3.,
        outside_mm: 1.,
        inside_mm: 1.,
        tilt: None,
        depth_mm: None,
        surface: HoleSurface::Outer,
    };
    let edge = request_hole_at([51.6, 35., 5.]);
    let mut never = karasu_core::cancellation::Cancellation::never();
    assert!(centre.resolve(probe, &mut never).is_ok());
    assert!(edge.resolve(probe, &mut never).is_err());
    // Slicing the same plate is refused.
    assert!(slice(&request, &mut Silent, &|_| false).is_err());
}

fn request_hole_at(surface_mm: [f64; 3]) -> DrainHole {
    DrainHole::Surface {
        surface_mm,
        diameter_mm: 3.,
        outside_mm: 1.,
        inside_mm: 1.,
        tilt: None,
        depth_mm: None,
        surface: HoleSurface::Outer,
    }
}

#[test]
fn hollowing_is_computed_in_the_model_frame_and_follows_the_object() {
    // TD-089: the same model moved or mirrored keeps the same cavity, mapped by its placement;
    // mirroring a hollowed object is accepted.
    let mut project = Project::new("Model frame");
    let block = project.add_model(
        "block.stl",
        box_stl([0., 0., 0.], [20., 20., 10.]).into_bytes(),
    );
    let object = |translate_mm: [f64; 3], mirror: [bool; 3]| {
        let mut object = PlateObject::new("block", block);
        object.arrange = false;
        object.translate_mm = translate_mm;
        object.pose.mirror = mirror;
        object.set_hollowing(Some(Hollowing {
            wall_mm: 2.,
            cell_mm: Some(0.5),
            simplify: None,
        }));
        object
    };
    let plate = project.add_plate(PlateContent {
        name: "Plate".into(),
        printer: Some(printer()),
        resin: Some(material()),
        print_setting: Some(print_setting()),
        settings: PlateSettings::new("goo-v3"),
        objects: vec![
            object([10., 10., 0.], [false; 3]),
            object([45., 30., 0.], [false; 3]),
            object([80., 10., 0.], [true, false, false]),
        ],
    });
    let output =
        std::env::temp_dir().join(format!("karasu-model-frame-{}.goo", std::process::id()));
    let _ = std::fs::remove_file(&output);
    let (request, _) = project.request(plate, output.clone()).unwrap();
    let preparation = prepare(&request, &mut Silent, &|_| false).unwrap();
    let hollows: Vec<_> = preparation
        .objects
        .iter()
        .map(|o| o.as_ref().unwrap().hollowed().unwrap())
        .collect();
    let first = hollows[0].certificate();
    for hollow in &hollows[1..] {
        let c = hollow.certificate();
        assert_eq!(hollow.plan_record(), hollows[0].plan_record());
        assert_eq!(c.surface_distance_mm, first.surface_distance_mm);
        assert!(
            (c.cavity_volume_mm3 - first.cavity_volume_mm3).abs() < 1e-6,
            "{c:?}"
        );
        assert!(c.cavity_volume_mm3 > 0.0);
    }
    // The moved copy has the very same cavity faces; the mirrored one has them reversed.
    assert_eq!(
        hollows[1].cavity().triangles(),
        hollows[0].cavity().triangles()
    );
    for (&[a, b, c], &mirrored) in hollows[0]
        .cavity()
        .triangles()
        .iter()
        .zip(hollows[2].cavity().triangles())
    {
        assert_eq!(mirrored, [a, c, b]);
    }
    // And the plate slices.
    let sliced = slice(&request, &mut Silent, &|_| false);
    let _ = std::fs::remove_file(&output);
    assert!(sliced.is_ok(), "{:?}", sliced.err());
}

/// The slab of `plate()` as a mesh, its model frame being its world frame.
fn slab_mesh() -> karasu_core::mesh::TriangleMesh {
    use karasu_core::geometry::Point3;
    let (min, max) = ([10., 20., 5.], [30., 30., 7.]);
    let points = (0..8)
        .map(|i| {
            Point3::new(
                if i & 1 == 0 { min[0] } else { max[0] },
                if i & 2 == 0 { min[1] } else { max[1] },
                if i & 4 == 0 { min[2] } else { max[2] },
            )
        })
        .collect();
    let mut triangles = Vec::new();
    for q in [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ] {
        triangles.push([q[0], q[1], q[2]]);
        triangles.push([q[0], q[2], q[3]]);
    }
    karasu_core::mesh::TriangleMesh::try_new(points, triangles).unwrap()
}

#[test]
fn an_objects_supports_are_built_where_they_are_edited() {
    use karasu_core::{cancellation::Cancellation, geometry::ObjectTransform};
    use karasu_project::pipeline::build_model_supports;
    let (project, plate) = plate("");
    let (request, _) = project
        .request(plate, std::env::temp_dir().join("unused.goo"))
        .unwrap();
    let preparation = prepare(&request, &mut Silent, &|_| false).unwrap();
    let slab = preparation.objects[0].as_ref().unwrap();
    let mesh = slab_mesh();
    let placement = karasu_project::placement::place(
        &mesh,
        [0.; 3],
        &Pose::default(),
        [0., 0.],
        &mut Cancellation::never(),
    )
    .unwrap()
    .transform;
    assert_eq!(placement, ObjectTransform::identity());
    let build = |extra: &str| {
        build_model_supports(
            &supports(extra),
            &mesh,
            &placement,
            slab,
            &printer(),
            &|_| false,
        )
    };
    // Two applied supports and a draft between them: built with every part's triangles.
    let scene = build(r#",{"id":3,"tip":{"point":[20,25,5]},"base":{"vertical":true}}"#).unwrap();
    assert!(!scene.has_errors(), "{:?}", scene.findings());
    assert_eq!(scene.supports().len(), 3);
    assert_eq!(
        scene.part_ranges().last().unwrap().triangles.end,
        scene.mesh().triangles().len()
    );
    // A draft in the air is a finding to show, not a failure.
    let scene = build(r#",{"id":3,"tip":{"point":[20,25,9]},"base":{"vertical":true}}"#).unwrap();
    assert!(scene
        .findings()
        .iter()
        .any(|f| f.entity == 3 && f.kind == FindingKind::AnchorOffSurface));
    // A document bound to a placement is refused: the model's frame needs none.
    let mut bound = supports("");
    bound.binding = Some(karasu_core::supports::Binding {
        model_sha256: None,
        translation_mm: Some([0.; 3]),
        pose: None,
    });
    let error =
        build_model_supports(&bound, &mesh, &placement, slab, &printer(), &|_| false).unwrap_err();
    assert!(error.contains("binding"), "{error}");
}
