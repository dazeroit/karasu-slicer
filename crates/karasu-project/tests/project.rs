//! The project model and background jobs: library identity, plate revisions, plate requests over
//! embedded data, and jobs with progress, cancellation and freshness.
mod common;

use common::{
    box_stl, dense_box_stl, field, material, print_setting, printer, resin, TempDirectory,
};
use karasu_core::{
    preparation::repair::RepairPreset, profiles::store::ProfileDocument, supports::SupportDocument,
};
use karasu_project::{
    jobs::{Event, Job, JobError, Progress},
    pipeline::{slice, Silent},
    project::{
        Change, Freshness, Hollowing, ModelId, OperationKind, Plate, PlateContent, PlateId,
        PlateObject, PlateSettings, Project, ProjectError,
    },
    request::{
        format_ids, format_name, format_names, Input, Pose, SliceRequest, Subject, DEFAULT_REPAIR,
        REFERENCE_FORMAT,
    },
};
use std::fs;

/// A project with a 20 x 10 x 2 mm box in its library and one plate of two arranged copies.
fn project() -> (Project, ModelId, PlateId) {
    let mut project = Project::new("Test project");
    let model = project.add_model(
        "box.stl",
        box_stl([10., 20., 0.], [30., 30., 2.]).into_bytes(),
    );
    let mut object = PlateObject::new("box", model);
    object.copies = 2;
    let plate = project.add_plate(PlateContent {
        name: "Plate 1".into(),
        printer: Some(printer()),
        resin: Some(material()),
        print_setting: Some(print_setting()),
        settings: PlateSettings::new("goo-v3"),
        objects: vec![object],
    });
    (project, model, plate)
}

#[test]
fn the_library_stores_each_model_once_and_keeps_models_in_use() {
    let (mut project, model, plate) = project();
    let bytes = box_stl([10., 20., 0.], [30., 30., 2.]).into_bytes();
    // The same bytes under another name are the same model.
    assert_eq!(project.add_model("copy.stl", bytes), model);
    assert_eq!(project.models().len(), 1);
    assert_eq!(project.model(model).unwrap().name, "box.stl");
    assert_eq!(model.to_string().len(), 64);
    let other = project.add_model("other.stl", box_stl([0.; 3], [5., 5., 5.]).into_bytes());
    assert_ne!(other, model);
    // A model placed on a plate cannot be removed; an unused one can.
    assert_eq!(
        project.remove_model(model),
        Err(ProjectError::ModelInUse {
            model,
            plates: vec![plate]
        })
    );
    assert_eq!(project.remove_model(other).unwrap().unwrap().id, other);
    assert_eq!(project.remove_model(other), Ok(None));
}

#[test]
fn every_plate_edit_advances_its_revision_and_outdates_older_results() {
    let (mut project, _, plate) = project();
    assert_eq!(project.plate(plate).unwrap().revision(), 1);
    assert_eq!(project.freshness(plate, 1), Freshness::Current);
    assert_eq!(
        project.edit_plate(plate, |p| p.name = "Renamed".into()),
        Ok(2)
    );
    // Even an edit that changes nothing outdates results: they are recomputed, not trusted.
    assert_eq!(project.edit_plate(plate, |_| {}), Ok(3));
    assert_eq!(project.freshness(plate, 1), Freshness::Stale { current: 3 });
    assert_eq!(project.freshness(plate, 3), Freshness::Current);
    let other = project.add_plate(project.plate(plate).unwrap().content().clone());
    assert_ne!(other, plate);
    // TD-135: no revision of one plate is ever another's, so a result computed for one plate
    // never reads as current for another.
    assert_eq!(project.plate(other).unwrap().revision(), 4);
    assert_eq!(project.freshness(other, 3), Freshness::Stale { current: 4 });
    assert!(project.remove_plate(plate).is_some());
    assert_eq!(project.freshness(plate, 3), Freshness::Removed);
    assert_eq!(
        project.edit_plate(plate, |_| {}),
        Err(ProjectError::UnknownPlate(plate))
    );
    // Identities are never reused.
    assert!(
        project
            .add_plate(project.plate(other).unwrap().content().clone())
            .0
            > other.0
    );
}

#[test]
fn plate_requests_embed_their_data_and_refuse_invalid_plates() {
    let (mut project, model, plate) = project();
    let temp = TempDirectory::new();
    let (request, revision) = project.request(plate, temp.path("plate.goo")).unwrap();
    assert_eq!(revision, 1);
    assert!(matches!(&request.subject, Subject::Plate(spec) if spec.objects.len() == 1));
    assert!(matches!(&request.printer, Input::Loaded { name, .. } if name == "Test printer"));
    // An empty plate, a missing model, refused settings and a refused object.
    project.edit_plate(plate, |p| p.objects.clear()).unwrap();
    assert_eq!(
        project.request(plate, temp.path("x.goo")).unwrap_err(),
        ProjectError::EmptyPlate(plate)
    );
    let missing = ModelId([7; 32]);
    project
        .edit_plate(plate, |p| {
            p.objects = vec![PlateObject::new("ghost", missing)];
        })
        .unwrap();
    assert_eq!(
        project.request(plate, temp.path("x.goo")).unwrap_err(),
        ProjectError::UnknownModel {
            plate,
            object: 1,
            model: missing
        }
    );
    project
        .edit_plate(plate, |p| {
            p.objects = vec![PlateObject::new("box", model)];
            p.print_setting.as_mut().unwrap().image.min_gray = 40;
        })
        .unwrap();
    let error = project.request(plate, temp.path("x.goo")).unwrap_err();
    assert!(error.to_string().contains("minimum gray"), "{error}");
    project
        .edit_plate(plate, |p| {
            p.print_setting.as_mut().unwrap().image.min_gray = 0;
            p.objects[0].accept_support_collisions = true;
        })
        .unwrap();
    let error = project.request(plate, temp.path("x.goo")).unwrap_err();
    assert!(
        error.to_string().contains("object 1 (\"box\")")
            && error.to_string().contains("--accept-support-collisions"),
        "{error}"
    );
    // A lifted object without supports is a request (TD-108: lifted before its supports are
    // placed); slicing it asks for supports or a resting model.
    project
        .edit_plate(plate, |p| {
            p.objects[0].accept_support_collisions = false;
            p.objects[0].pose.lift_mm = Some(3.);
        })
        .unwrap();
    project.request(plate, temp.path("x.goo")).unwrap();
    assert_eq!(
        project
            .request(PlateId(99), temp.path("x.goo"))
            .unwrap_err(),
        ProjectError::UnknownPlate(PlateId(99))
    );
    project.edit_plate(plate, |p| p.resin = None).unwrap();
    assert_eq!(
        project.request(plate, temp.path("x.goo")).unwrap_err(),
        ProjectError::NoResin(plate)
    );
    project.edit_plate(plate, |p| p.printer = None).unwrap();
    assert_eq!(
        project.request(plate, temp.path("x.goo")).unwrap_err(),
        ProjectError::NoPrinter(plate)
    );
}

#[test]
fn an_embedded_plate_slices_exactly_like_the_same_plate_document() {
    let (mut project, _, plate) = project();
    project
        .edit_plate(plate, |p| {
            p.objects[0].set_hollowing(Some(Hollowing {
                wall_mm: 0.5,
                cell_mm: Some(0.125),
                simplify: None,
            }));
            p.objects[0].pose = Pose {
                drop: true,
                ..Pose::default()
            };
        })
        .unwrap();
    let temp = TempDirectory::new();
    let (embedded, _) = project.request(plate, temp.path("embedded.goo")).unwrap();
    let outcome = slice(&embedded, &mut Silent, &|_| false).unwrap();
    // The same plate as files: a plate document beside the model and profiles.
    temp.write("box.stl", box_stl([10., 20., 0.], [30., 30., 2.]));
    let document = temp.write(
        "plate.json",
        r#"{"format":"karasu-plate","version":1,"objects":[{"name":"box","model":"box.stl",
            "units":"mm","drop":true,"copies":2,"hollow":{"wall_mm":0.5,"cell_mm":0.125}}]}"#,
    );
    let files = SliceRequest {
        subject: Subject::PlateFile(document),
        printer: Input::File(temp.write(
            "printer.json",
            ProfileDocument::Printer(printer()).to_json().unwrap(),
        )),
        resin: Input::File(temp.write(
            "resin.json",
            ProfileDocument::Resin(resin()).to_json().unwrap(),
        )),
        output: temp.path("files.goo"),
        ..embedded.clone()
    };
    let from_files = slice(&files, &mut Silent, &|_| false).unwrap();
    assert_eq!(
        fs::read(temp.path("embedded.goo")).unwrap(),
        fs::read(temp.path("files.goo")).unwrap()
    );
    // The summaries differ only where they name the inputs and the output.
    let differing = |a: &str, b: &str| -> Vec<(String, String)> {
        a.lines()
            .zip(b.lines())
            .filter(|(x, y)| x != y)
            .map(|(x, y)| (x.to_owned(), y.to_owned()))
            .collect()
    };
    let differences = differing(&outcome.summary, &from_files.summary);
    let keys: Vec<&str> = differences
        .iter()
        .map(|(line, _)| line.split(": ").next().unwrap())
        .collect();
    assert_eq!(
        keys,
        ["Plate", "Output path", "Model name"],
        "{differences:?}"
    );
    assert_eq!(field(&outcome.summary, "Plate"), "\"Plate 1\"");
    assert_eq!(field(&outcome.summary, "Model name"), "\"box.stl\"");
    assert_eq!(
        field(&outcome.summary, "Objects"),
        "1, instances 2, spacing 2 mm"
    );
}

#[test]
fn a_background_job_reports_progress_and_its_revision() {
    let (mut project, _, plate) = project();
    let temp = TempDirectory::new();
    let (request, revision) = project.request(plate, temp.path("job.goo")).unwrap();
    let mut job = Job::start(plate, revision, request, true);
    assert_eq!((job.plate(), job.revision()), (plate, 1));
    // The plate is edited while the job runs: its result is then stale.
    project
        .edit_plate(plate, |p| p.name = "Edited".into())
        .unwrap();
    let mut progress = Vec::new();
    let finished = loop {
        let mut done = None;
        for event in job.poll() {
            match event {
                Event::Progress(p) => progress.push(p),
                Event::Finished(f) => done = Some(f),
            }
        }
        if let Some(finished) = done {
            break finished;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    assert!(job.poll().is_empty());
    // The kept masks are the exported layers, in order: the 20 x 10 mm box covers 200 pixels.
    assert_eq!(finished.masks.len(), 8);
    assert_eq!(
        finished.masks.iter().map(|m| m.index).collect::<Vec<_>>(),
        (0..8).collect::<Vec<_>>()
    );
    let lit = |m: &karasu_project::jobs::LayerMask| -> u32 {
        m.mask
            .rows()
            .flatten()
            .map(|span| span.end() - span.start())
            .sum()
    };
    assert!(finished.masks.iter().all(|m| lit(m) == 2 * 200));
    let outcome = finished.result.unwrap();
    assert!(temp.path("job.goo").is_file());
    assert_eq!(field(&outcome.summary, "Layers"), "8");
    assert_eq!(
        progress.first(),
        Some(&Progress::Phase("Reading profiles".into()))
    );
    assert_eq!(
        progress.last(),
        Some(&Progress::Layers {
            completed: 8,
            total: 8
        })
    );
    assert_eq!(
        project.freshness(finished.plate, finished.revision),
        Freshness::Stale { current: 2 }
    );
}

#[test]
fn a_cancelled_background_job_publishes_nothing() {
    // A dense model (about 200,000 triangles), so the job is still importing when it is
    // cancelled.
    let mut project = Project::new("Cancel");
    let model = project.add_model(
        "dense.stl",
        dense_box_stl([10., 10., 0.], [40., 40., 10.], 128),
    );
    let plate = project.add_plate(PlateContent {
        name: "Plate".into(),
        printer: Some(printer()),
        resin: Some(material()),
        print_setting: Some(print_setting()),
        settings: PlateSettings::new("goo-v3"),
        objects: vec![PlateObject::new("dense", model)],
    });
    let temp = TempDirectory::new();
    let (request, revision) = project.request(plate, temp.path("cancelled.goo")).unwrap();
    let job = Job::start(plate, revision, request, false);
    job.cancel();
    let finished = job.wait();
    assert_eq!(finished.result.unwrap_err(), JobError::Cancelled);
    assert!(fs::read_dir(&temp.0).unwrap().next().is_none());
}

/// A plate with a 20 x 10 x 2 mm slab floating at Z 5 on two supports, bound to the pose they
/// were placed for (TD-072).
fn supported_plate() -> (Project, PlateId) {
    let mut project = Project::new("Supports");
    let model = project.add_model(
        "slab.stl",
        box_stl([10., 20., 5.], [30., 30., 7.]).into_bytes(),
    );
    let supports = SupportDocument::from_json(
        br#"{"format":"karasu-supports","version":1,
            "supports":[{"id":1,"tip":{"point":[15,25,5]},"base":{"vertical":true}},
                        {"id":2,"tip":{"point":[25,25,5]},"base":{"vertical":true}}]}"#,
    )
    .unwrap();
    let mut object = PlateObject::new("slab", model);
    object.pose = Pose::default();
    object.arrange = false;
    object.supports = Some(supports);
    let plate = project.add_plate(PlateContent {
        name: "Plate".into(),
        printer: Some(printer()),
        resin: Some(material()),
        print_setting: Some(print_setting()),
        settings: PlateSettings::new("goo-v3"),
        objects: vec![object],
    });
    (project, plate)
}

#[test]
fn supports_follow_their_object_through_every_placement() {
    let (mut project, plate) = supported_plate();
    let temp = TempDirectory::new();
    let run = |project: &Project, name: &str| {
        let (request, _) = project.request(plate, temp.path(name)).unwrap();
        slice(&request, &mut Silent, &|_| false)
    };
    let volume = |summary: &str| -> f64 {
        field(summary, "Sampled mask volume (mm^3)")
            .parse()
            .unwrap()
    };
    let placed = run(&project, "placed.goo").unwrap().summary;
    // Moved on the plate and turned a quarter about the vertical: the supports come along, and
    // the print is the same.
    project
        .edit_plate(plate, |p| {
            p.objects[0].translate_mm = [20., 10., 0.];
            p.objects[0].pose.rotate_deg = [0., 0., 90.];
        })
        .unwrap();
    let turned = run(&project, "turned.goo").unwrap().summary;
    assert_eq!(field(&turned, "Supports"), field(&placed, "Supports"));
    assert_eq!(field(&turned, "Layers"), field(&placed, "Layers"));
    let (a, b) = (volume(&placed), volume(&turned));
    assert!((a - b).abs() < 0.01 * a, "{a} {b}");
    // The supports are in the model's frame (TD-108): a tilt, a new scale or a new height
    // carries their tips with the model, and the shafts reach the plate below them.
    for (edit, name) in [
        Box::new(|p: &mut PlateContent| p.objects[0].pose.rotate_deg = [10., 0., 0.])
            as Box<dyn Fn(&mut PlateContent)>,
        Box::new(|p: &mut PlateContent| {
            p.objects[0].pose.rotate_deg = [0.; 3];
            p.objects[0].pose.scale = [1.1, 1., 1.];
        }),
        Box::new(|p: &mut PlateContent| {
            p.objects[0].pose.scale = [1.; 3];
            p.objects[0].translate_mm = [0., 0., 1.];
        }),
    ]
    .into_iter()
    .zip(["tilted.goo", "scaled.goo", "raised.goo"])
    {
        project.edit_plate(plate, |p| edit(p)).unwrap();
        let summary = run(&project, name).unwrap().summary;
        assert_eq!(field(&summary, "Supports"), field(&placed, "Supports"));
    }
    // A model-frame document records no placement: a binding is refused, never ignored.
    project
        .edit_plate(plate, |p| {
            let doc = p.objects[0].supports.as_mut().unwrap();
            doc.binding = Some(karasu_core::supports::Binding {
                model_sha256: None,
                translation_mm: Some([0.; 3]),
                pose: None,
            });
        })
        .unwrap();
    let error = run(&project, "bound.goo").unwrap_err();
    assert!(error.message().contains("binding"), "{error}");
}

#[test]
fn a_projects_supports_with_errors_are_sliced_with_a_warning() {
    // Owner, 2026-10-27 (TD-108): a support with an error does not stop the print.
    let (mut project, plate) = supported_plate();
    project
        .edit_plate(plate, |p| {
            let doc = p.objects[0].supports.as_mut().unwrap();
            let mut floating = doc.supports[0].clone();
            floating.id = 3;
            floating.tip_mm = [20., 25., 9.];
            doc.supports.push(floating);
        })
        .unwrap();
    let temp = TempDirectory::new();
    let (request, _) = project.request(plate, temp.path("errors.goo")).unwrap();
    assert!(request.slice_support_errors);
    let summary = slice(&request, &mut Silent, &|_| false).unwrap().summary;
    assert!(
        field(&summary, "Supports sliced with errors").starts_with("1 errors"),
        "{summary}"
    );
    // The command line's policy refuses them.
    let refused = karasu_project::request::SliceRequest {
        slice_support_errors: false,
        allow_floating: false,
        output: temp.path("refused.goo"),
        ..request
    };
    let error = slice(&refused, &mut Silent, &|_| false).unwrap_err();
    assert!(error.message().contains("cannot be printed"), "{error}");
}

#[test]
fn undo_and_redo_restore_contents_as_new_revisions() {
    let (mut project, model, plate) = project();
    let content = |p: &Project| p.plate(plate).unwrap().content().clone();
    let original = content(&project);
    assert!(!project.plate(plate).unwrap().can_undo());
    assert_eq!(project.undo_plate(plate), Ok(None));
    // An edit that changes nothing is a revision but no undo step.
    project.edit_plate(plate, |_| {}).unwrap();
    assert!(!project.plate(plate).unwrap().can_undo());
    // Two separate edits are two steps.
    project
        .edit_plate(plate, |c| c.name = "Renamed".into())
        .unwrap();
    project
        .edit_plate(plate, |c| c.objects.push(PlateObject::new("second", model)))
        .unwrap();
    let edited = content(&project);
    let before_undo = project.plate(plate).unwrap().revision();
    let undone = project.undo_plate(plate).unwrap().unwrap();
    assert!(undone > before_undo);
    assert_eq!(content(&project).objects.len(), 1);
    assert_eq!(content(&project).name, "Renamed");
    // A result from before the undo stays stale, even if the content matched it.
    assert!(matches!(
        project.freshness(plate, before_undo),
        Freshness::Stale { .. }
    ));
    project.undo_plate(plate).unwrap().unwrap();
    assert_eq!(content(&project), original);
    assert!(project.plate(plate).unwrap().can_redo());
    project.redo_plate(plate).unwrap().unwrap();
    project.redo_plate(plate).unwrap().unwrap();
    assert_eq!(content(&project), edited);
    assert_eq!(project.redo_plate(plate), Ok(None));
    // A new edit clears the redo steps.
    project.undo_plate(plate).unwrap();
    project
        .edit_plate(plate, |c| c.name = "Other".into())
        .unwrap();
    assert!(!project.plate(plate).unwrap().can_redo());
    assert_eq!(
        project.undo_plate(PlateId(99)),
        Err(ProjectError::UnknownPlate(PlateId(99)))
    );
    // Clearing the history keeps the content and leaves nothing to undo or redo.
    project.undo_plate(plate).unwrap();
    let kept = content(&project);
    project.clear_plate_history(plate).unwrap();
    assert_eq!(content(&project), kept);
    assert!(!project.plate(plate).unwrap().can_undo());
    assert!(!project.plate(plate).unwrap().can_redo());
    assert_eq!(
        project.clear_plate_history(PlateId(99)),
        Err(ProjectError::UnknownPlate(PlateId(99)))
    );
}

#[test]
fn merged_edits_are_one_undo_step_until_another_edit() {
    let (mut project, _, plate) = project();
    let x = |p: &Project| p.plate(plate).unwrap().content().objects[0].translate_mm[0];
    // A drag: many edits with one key.
    for step in 1..=10 {
        project
            .edit_plate_merging(plate, 7, |c| c.objects[0].translate_mm[0] = f64::from(step))
            .unwrap();
    }
    // A second drag with the same key after another edit is a new step.
    project
        .edit_plate(plate, |c| c.name = "Moved".into())
        .unwrap();
    for step in 11..=12 {
        project
            .edit_plate_merging(plate, 7, |c| c.objects[0].translate_mm[0] = f64::from(step))
            .unwrap();
    }
    // A merged step that returns to its start is no step.
    project
        .edit_plate(plate, |c| c.name = "Moved again".into())
        .unwrap();
    project
        .edit_plate_merging(plate, 8, |c| c.name = "Moving".into())
        .unwrap();
    project
        .edit_plate_merging(plate, 8, |c| c.name = "Moved again".into())
        .unwrap();
    project.undo_plate(plate).unwrap();
    assert_eq!(project.plate(plate).unwrap().content().name, "Moved");
    assert_eq!(x(&project), 12.);
    project.undo_plate(plate).unwrap();
    assert_eq!(x(&project), 10.);
    project.undo_plate(plate).unwrap();
    assert_eq!(project.plate(plate).unwrap().content().name, "Plate 1");
    project.undo_plate(plate).unwrap();
    assert_eq!(x(&project), 0.);
    assert!(!project.plate(plate).unwrap().can_undo());
    // The history keeps the newest steps only.
    for step in 0..(karasu_project::project::MAX_UNDO_STEPS + 5) {
        project
            .edit_plate(plate, |c| c.name = format!("step {step}"))
            .unwrap();
    }
    let mut undone = 0;
    while project.undo_plate(plate).unwrap().is_some() {
        undone += 1;
    }
    assert_eq!(undone, karasu_project::project::MAX_UNDO_STEPS);
}

#[test]
fn an_object_repaired_on_its_own_is_hollowed_where_the_plate_repair_refuses() {
    let (mut project, _, plate) = project();
    // The same box without its first facet: open, so hollowing needs its hole filled.
    let closed = box_stl([10., 20., 0.], [30., 30., 2.]);
    let start = closed.find("facet").unwrap();
    let end = closed.find("endfacet").unwrap() + "endfacet\n".len();
    let open = format!("{}{}", &closed[..start], &closed[end..]);
    let model = project.add_model("open.stl", open.into_bytes());
    let hollow = Hollowing {
        wall_mm: 0.5,
        cell_mm: Some(0.125),
        simplify: None,
    };
    project
        .edit_plate(plate, |p| {
            let mut object = PlateObject::new("open", model);
            object.set_hollowing(Some(hollow));
            p.objects = vec![object];
        })
        .unwrap();
    let temp = TempDirectory::new();
    let (request, _) = project.request(plate, temp.path("plate.goo")).unwrap();
    let Subject::Plate(spec) = &request.subject else {
        panic!("not a plate request");
    };
    assert_eq!(spec.objects[0].repair, None);
    assert_eq!(request.for_object(&spec.objects[0]).repair, DEFAULT_REPAIR);
    let refused = slice(&request, &mut Silent, &|_| false).unwrap_err();
    assert!(refused.to_string().contains("closed surface"), "{refused}");
    // Its own exact repair comes first in its timeline, before the hollowing.
    project
        .edit_plate(plate, |p| {
            p.objects[0].set_repair(Some(RepairPreset::Exact.spec()));
        })
        .unwrap();
    let content = project.plate(plate).unwrap().content();
    assert!(matches!(
        content.objects[0].operations[..],
        [
            ref repair @ karasu_project::project::ObjectOperation {
                kind: OperationKind::Repair(_),
                ..
            },
            karasu_project::project::ObjectOperation {
                kind: OperationKind::Hollow(_),
                ..
            }
        ] if repair.enabled
    ));
    let (request, _) = project.request(plate, temp.path("plate.goo")).unwrap();
    let Subject::Plate(spec) = &request.subject else {
        panic!("not a plate request");
    };
    assert_eq!(
        request.for_object(&spec.objects[0]).repair,
        RepairPreset::Exact.spec()
    );
    let outcome = slice(&request, &mut Silent, &|_| false).unwrap();
    assert!(
        field(&outcome.summary, "Repairs").contains("1 hole"),
        "{}",
        outcome.summary
    );
    // Back to the plate's repairs, and a second repair or one after another operation refused.
    project
        .edit_plate(plate, |p| p.objects[0].set_repair(None))
        .unwrap();
    assert_eq!(
        project.plate(plate).unwrap().content().objects[0].repair(),
        None
    );
    for at_end in [false, true] {
        project
            .edit_plate(plate, |p| {
                let object = &mut p.objects[0];
                object.set_repair(Some(RepairPreset::Exact.spec()));
                if at_end {
                    object.operations.rotate_left(1);
                } else {
                    object.push_operation(OperationKind::Repair(RepairPreset::Off.spec()));
                }
            })
            .unwrap();
        let error = project.request(plate, temp.path("x.goo")).unwrap_err();
        assert!(error.to_string().contains("repaired once"), "{error}");
        project
            .edit_plate(plate, |p| {
                p.objects[0]
                    .operations
                    .retain(|o| !matches!(o.kind, OperationKind::Repair(_)));
            })
            .unwrap();
    }
}

/// Every machine format has a readable name, once; the reference format is not a printer's and is
/// not named (TD-106).
#[test]
fn every_machine_format_is_named_and_the_reference_is_not() {
    let names = format_names();
    let all = format_ids();
    let ids: Vec<&str> = all
        .split(", ")
        .filter(|id| *id != REFERENCE_FORMAT)
        .collect();
    assert_eq!(names.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(), ids);
    for name in &names {
        assert!(!name.family.is_empty() && !name.name.is_empty(), "{name:?}");
        assert!(!name.extension.is_empty() && !name.extension.starts_with('.'));
        assert_eq!(format_name(&name.id).as_ref(), Some(name));
    }
    let mut readable: Vec<(&str, &str)> =
        names.iter().map(|f| (f.family, f.name.as_str())).collect();
    readable.sort_unstable();
    readable.dedup();
    assert_eq!(readable.len(), names.len(), "two formats share a name");
    assert_eq!(format_name(REFERENCE_FORMAT), None);
    assert_eq!(format_name("no-such-format"), None);
    // Every CTB says it is unencrypted (owner, 2026-11-11): karasu writes no encrypted CTB.
    for name in names.iter().filter(|f| f.extension == "ctb") {
        assert!(name.name.ends_with(", unencrypted"), "{name:?}");
    }
    assert_eq!(
        format_name("ctb-v4-plain").map(|f| f.name),
        Some("CTB version 4, unencrypted".to_owned())
    );
}

#[test]
fn the_structure_of_plates_undoes_and_redoes_with_the_plates_own_steps_in_order() {
    // TD-135: inserting, deleting and moving plates are steps of the project, undone and redone
    // with each plate's own edits, newest first.
    let (mut project, _, first) = project();
    let content = project.plate(first).unwrap().content().clone();
    let named = |name: &str| PlateContent {
        name: name.into(),
        ..content.clone()
    };
    let order = |project: &Project| -> Vec<String> {
        project
            .plates()
            .iter()
            .map(|p| p.content().name.clone())
            .collect()
    };
    let start = order(&project);
    project
        .edit_plate(first, |p| p.name = "First".into())
        .unwrap();
    let second = project.insert_plate(1, named("Second"));
    let third = project.insert_plate(1, named("Third"));
    assert_eq!(order(&project), ["First", "Third", "Second"]);
    project.move_plate(second, 0).unwrap();
    assert_eq!(order(&project), ["Second", "First", "Third"]);
    project
        .edit_plate(third, |p| p.name = "Third, edited".into())
        .unwrap();
    project.delete_plate(first).unwrap();
    assert_eq!(order(&project), ["Second", "Third, edited"]);
    assert_eq!(project.history(third), (true, false));
    // Undo from the third plate: its rename was before the deletion, so the deletion first.
    assert_eq!(
        project.undo(third).unwrap(),
        Some(Change::Structure(Some(first)))
    );
    assert_eq!(order(&project), ["Second", "First", "Third, edited"]);
    // Then its own rename, then the move, then the two inserts, then the first plate's rename.
    assert!(matches!(project.undo(third).unwrap(), Some(Change::Plate(id, _)) if id == third));
    assert_eq!(order(&project), ["Second", "First", "Third"]);
    assert!(matches!(
        project.undo(third).unwrap(),
        Some(Change::Structure(_))
    ));
    assert_eq!(order(&project), ["First", "Third", "Second"]);
    assert_eq!(project.undo(third).unwrap(), Some(Change::Structure(None)));
    assert_eq!(order(&project), ["First", "Second"]);
    assert_eq!(project.undo(first).unwrap(), Some(Change::Structure(None)));
    assert!(matches!(project.undo(first).unwrap(), Some(Change::Plate(id, _)) if id == first));
    assert_eq!(order(&project), start);
    assert_eq!(project.undo(first).unwrap(), None);
    // Redo replays them in the order they were made, from the plate shown, which becomes the
    // plate a structure step shows, as the application follows it. The third plate's own
    // rename waits until that plate is shown.
    let mut shown = first;
    let redo = |project: &mut Project, shown: &mut PlateId| {
        let change = project.redo(*shown).unwrap();
        if let Some(Change::Structure(Some(id))) = change {
            *shown = id;
        }
        change.is_some()
    };
    for _ in 0..5 {
        assert!(redo(&mut project, &mut shown));
    }
    assert_eq!(order(&project), ["Second", "Third"]);
    assert!(!redo(&mut project, &mut shown));
    shown = third;
    assert!(redo(&mut project, &mut shown));
    assert_eq!(order(&project), ["Second", "Third, edited"]);
    // A new step clears what was left to redo.
    project.undo(second).unwrap();
    project.insert_plate(9, named("Fourth"));
    assert_eq!(project.history(second), (true, false));
    // A plate's revision after any of this is still unique.
    let mut revisions: Vec<u64> = project.plates().iter().map(Plate::revision).collect();
    revisions.sort_unstable();
    revisions.dedup();
    assert_eq!(revisions.len(), project.plates().len());
}

#[test]
fn objects_moved_between_plates_are_one_step_undone_after_later_edits_of_those_plates() {
    // TD-135, phase 3: a move between two plates, or to a new one, is one step of the project;
    // undoing it first undoes what was done since to the plates it changed, and an edit after
    // undoing it leaves nothing to redo.
    let (mut project, _, first) = project();
    let content = project.plate(first).unwrap().content().clone();
    let second = project.insert_plate(
        1,
        PlateContent {
            name: "Second".into(),
            objects: Vec::new(),
            ..content.clone()
        },
    );
    let objects =
        |project: &Project, id: PlateId| project.plate(id).unwrap().content().objects.len();
    assert_eq!(
        (objects(&project, first), objects(&project, second)),
        (1, 0)
    );
    // Move the object from the first plate to the second.
    let mut from = content.clone();
    let object = from.objects.remove(0);
    let mut to = project.plate(second).unwrap().content().clone();
    to.objects.push(object.clone());
    let revisions = (
        project.plate(first).unwrap().revision(),
        project.plate(second).unwrap().revision(),
    );
    assert_eq!(
        project
            .edit_plates(vec![(first, from), (second, to)], Vec::new())
            .unwrap(),
        Vec::<PlateId>::new()
    );
    assert_eq!(
        (objects(&project, first), objects(&project, second)),
        (0, 1)
    );
    assert!(project.plate(first).unwrap().revision() > revisions.0);
    assert!(project.plate(second).unwrap().revision() > revisions.1);
    // An unknown plate changes nothing.
    assert!(project
        .edit_plates(
            vec![(first, content.clone()), (PlateId(999), content.clone())],
            Vec::new()
        )
        .is_err());
    assert_eq!(objects(&project, first), 0);
    // The second plate is edited after the move: undone from the first plate, that edit goes
    // first, then the move, which shows the first plate.
    project
        .edit_plate(second, |p| p.objects[0].translate_mm[0] = 5.0)
        .unwrap();
    assert!(matches!(project.undo(first).unwrap(), Some(Change::Plate(id, _)) if id == second));
    assert_eq!(
        project.plate(second).unwrap().content().objects[0].translate_mm[0],
        object.translate_mm[0]
    );
    assert_eq!(
        project.undo(first).unwrap(),
        Some(Change::Structure(Some(first)))
    );
    assert_eq!(
        (objects(&project, first), objects(&project, second)),
        (1, 0)
    );
    // Redo replays the move; the second plate's own edit waits until that plate is shown, as
    // every plate's own steps do.
    assert_eq!(
        project.redo(first).unwrap(),
        Some(Change::Structure(Some(first)))
    );
    assert_eq!(project.redo(first).unwrap(), None);
    assert!(matches!(project.redo(second).unwrap(), Some(Change::Plate(id, _)) if id == second));
    assert_eq!(
        project.plate(second).unwrap().content().objects[0].translate_mm[0],
        5.0
    );
    // A copy to a new plate after the second: one step, undone whole.
    let mut copy = content.clone();
    copy.name = "Third".into();
    let third = project
        .edit_plates(Vec::new(), vec![(2, copy)])
        .unwrap()
        .first()
        .copied()
        .expect("a new plate");
    assert_eq!(project.plates()[2].id(), third);
    assert_eq!(project.undo(third).unwrap(), Some(Change::Structure(None)));
    assert!(project.plate(third).is_none());
    assert_eq!(
        project.redo(first).unwrap(),
        Some(Change::Structure(Some(third)))
    );
    // Undone, then the first plate edited: the move back is no longer redone.
    project.undo(third).unwrap();
    project.undo(first).unwrap();
    project.undo(first).unwrap();
    assert_eq!(
        (objects(&project, first), objects(&project, second)),
        (1, 0)
    );
    project
        .edit_plate(first, |p| p.name = "Edited".into())
        .unwrap();
    while project.redo(first).unwrap().is_some() {}
    assert_eq!(
        (objects(&project, first), objects(&project, second)),
        (1, 0)
    );
}

#[test]
fn several_plates_added_with_an_edit_are_one_step() {
    // TD-135, phase 5: Arrange's overflow edits the plate and adds new plates after it, one
    // step undone and redone whole.
    let (mut project, _, first) = project();
    let content = project.plate(first).unwrap().content().clone();
    let mut left = content.clone();
    let object = left.objects.remove(0);
    let extra = |name: &str| PlateContent {
        name: name.into(),
        objects: vec![object.clone()],
        ..content.clone()
    };
    let added = project
        .edit_plates(
            vec![(first, left)],
            vec![(1, extra("First (2)")), (2, extra("First (3)"))],
        )
        .unwrap();
    assert_eq!(added.len(), 2);
    let names = |project: &Project| -> Vec<String> {
        project
            .plates()
            .iter()
            .map(|p| p.content().name.clone())
            .collect()
    };
    let after = names(&project);
    assert_eq!(after[1..], ["First (2)", "First (3)"]);
    assert_eq!(project.plate(first).unwrap().content().objects.len(), 0);
    // Undone whole: one plate with its object again; redone whole.
    project.undo(first).unwrap();
    assert_eq!(project.plates().len(), 1);
    assert_eq!(project.plate(first).unwrap().content().objects.len(), 1);
    project.redo(first).unwrap();
    assert_eq!(names(&project), after);
    assert_eq!(project.plate(added[1]).unwrap().content().objects.len(), 1);
}
