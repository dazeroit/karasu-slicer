//! Model export (TD-139): a plate's models written as STL, 3MF and OBJ, as the print holds them,
//! with their supports and raft by choice and their drain holes cut.
mod common;

use common::{box_stl, material, print_setting, printer, TempDirectory};
use karasu_core::{
    cancellation::Checkpoint,
    export::models::ModelFileFormat,
    mesh::formats::{FormatRegistry, LengthUnit, ModelImportOptions},
    preparation::sampled::HoleSurface,
    supports::SupportDocument,
};
use karasu_project::{
    pipeline::{
        export_models, model_file_name, ModelExport, ModelFiles, ModelObjects, NameFields, Silent,
    },
    project::{
        Hollowing, OperationKind, PlateContent, PlateId, PlateObject, PlateSettings, Project,
    },
    request::{DrainHole, Pose, SliceRequest},
};
use std::path::{Path, PathBuf};

/// A slab 5 mm above the plate on two supports standing on a raft, and a hollowed box drained by
/// one hole.
fn plate() -> (Project, PlateId) {
    let mut project = Project::new("Export");
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
    supported.supports = Some(
        SupportDocument::from_json(
            br#"{"format":"karasu-supports","version":1,
                "raft":{"thickness_mm":1,"margin_mm":1,"slope_deg":45},
                "supports":[{"id":1,"tip":{"point":[15,25,5]},"base":{"vertical":true}},
                            {"id":2,"tip":{"point":[25,25,5]},"base":{"vertical":true}}]}"#,
        )
        .unwrap(),
    );
    let mut hollowed = PlateObject::new("block", block);
    hollowed.arrange = false;
    hollowed.set_hollowing(Some(Hollowing {
        wall_mm: 2.,
        cell_mm: Some(0.5),
        simplify: None,
    }));
    hollowed.push_operation(OperationKind::DrainHole(DrainHole::Surface {
        surface_mm: [60., 35., 5.],
        diameter_mm: 3.,
        outside_mm: 1.,
        inside_mm: 1.,
        tilt: None,
        depth_mm: None,
        surface: HoleSurface::Outer,
    }));
    let plate = project.add_plate(PlateContent {
        name: "Plate".into(),
        printer: Some(printer()),
        resin: Some(material()),
        print_setting: Some(print_setting()),
        settings: PlateSettings::new("goo-v3"),
        objects: vec![supported, hollowed],
    });
    (project, plate)
}

fn request() -> SliceRequest {
    let (project, plate) = plate();
    project
        .request(plate, std::env::temp_dir().join("unused.goo"))
        .unwrap()
        .0
}

fn options(format: ModelFileFormat, files: ModelFiles) -> ModelExport {
    ModelExport {
        format,
        files,
        supports: true,
        raft: true,
        object: None,
        replace: false,
        objects: ModelObjects::Separate,
    }
}

/// The file read back by karasu's importer: its triangles and the objects it places.
fn read(path: &Path) -> (usize, usize) {
    let imported = FormatRegistry::builtin()
        .import_path(
            path,
            &ModelImportOptions::new(Some(LengthUnit::Millimeter)),
            &mut karasu_core::cancellation::Cancellation::never(),
        )
        .unwrap();
    (imported.mesh.triangles().len(), imported.objects)
}

/// A destination for each file: `name` for the plate's, `<n> <model>` for a model's.
fn named(
    dir: &Path,
    name: &str,
    format: ModelFileFormat,
) -> impl Fn(Option<(usize, &str)>) -> PathBuf {
    let (dir, name) = (dir.to_path_buf(), name.to_owned());
    move |model| {
        let stem = model.map_or_else(|| name.clone(), |(n, m)| format!("{n} {m}"));
        dir.join(format!("{stem}.{}", format.extension()))
    }
}

#[test]
fn a_plate_is_one_file_with_its_supports_raft_and_holes_cut() {
    let temp = TempDirectory::new();
    let format = ModelFileFormat::Stl;
    let outcome = export_models(
        &request(),
        &options(format, ModelFiles::PerPlate),
        &named(&temp.0, "plate", format),
        None,
        &mut Silent,
        &|_| false,
    )
    .unwrap();
    assert_eq!(outcome.files.len(), 1);
    let file = &outcome.files[0];
    assert_eq!(file.models, ["slab", "block"]);
    let summary = file.result.as_ref().unwrap();
    // The slab, its supports and its raft, and the block: four bodies.
    assert_eq!(summary.bodies, 4);
    assert_eq!(read(&file.path).0, summary.triangles);
    // The block's hole is cut, and nothing crosses.
    assert_eq!(outcome.drilled.len(), 1);
    let drilled = &outcome.drilled[0];
    assert_eq!(
        (drilled.model.as_str(), drilled.flat, drilled.crossings),
        ("block", 0, 0)
    );
    assert!(drilled.farthest_mm <= 1.0 / 128.0);
}

#[test]
fn a_file_per_model_takes_its_own_supports_and_raft() {
    let temp = TempDirectory::new();
    let format = ModelFileFormat::ThreeMf;
    let outcome = export_models(
        &request(),
        &options(format, ModelFiles::PerModel),
        &named(&temp.0, "plate", format),
        None,
        &mut Silent,
        &|_| false,
    )
    .unwrap();
    let paths: Vec<_> = outcome.files.iter().map(|f| f.path.clone()).collect();
    assert_eq!(paths, [temp.path("1 slab.3mf"), temp.path("2 block.3mf")]);
    let bodies: Vec<usize> = outcome
        .files
        .iter()
        .map(|f| f.result.as_ref().unwrap().bodies)
        .collect();
    // The slab with its supports and raft; the block alone. 3MF places each as an object.
    assert_eq!(bodies, [3, 1]);
    assert_eq!(read(&paths[0]).1, 3);
    assert_eq!(read(&paths[1]).1, 1);
}

#[test]
fn supports_and_raft_are_left_out_when_asked_and_one_model_is_exported_alone() {
    let temp = TempDirectory::new();
    let format = ModelFileFormat::Obj;
    let mut export = options(format, ModelFiles::PerModel);
    export.supports = false;
    export.raft = false;
    export.object = Some(0);
    let outcome = export_models(
        &request(),
        &export,
        &named(&temp.0, "plate", format),
        None,
        &mut Silent,
        &|_| false,
    )
    .unwrap();
    assert_eq!(outcome.files.len(), 1);
    assert_eq!(outcome.drilled.len(), 0);
    let summary = outcome.files[0].result.as_ref().unwrap();
    // The slab's box alone.
    assert_eq!((summary.bodies, summary.triangles), (1, 12));
    export.object = Some(5);
    let error = export_models(
        &request(),
        &export,
        &named(&temp.0, "plate", format),
        None,
        &mut Silent,
        &|_| false,
    )
    .unwrap_err();
    assert!(error.to_string().contains("no object 6"), "{error}");
}

#[test]
fn an_existing_file_is_kept_unless_replacing_it_was_confirmed() {
    let temp = TempDirectory::new();
    let format = ModelFileFormat::Stl;
    let destination = named(&temp.0, "plate", format);
    let existing = temp.write("1 slab.stl", "keep");
    let mut export = options(format, ModelFiles::PerModel);
    let outcome = export_models(
        &request(),
        &export,
        &destination,
        None,
        &mut Silent,
        &|_| false,
    )
    .unwrap();
    // The slab's file is refused and left as it was; the block's is written.
    let error = outcome.files[0].result.as_ref().unwrap_err();
    assert!(error.to_string().contains("never replaces"), "{error}");
    assert_eq!(std::fs::read_to_string(&existing).unwrap(), "keep");
    assert!(outcome.files[1].result.is_ok());
    export.replace = true;
    let outcome = export_models(
        &request(),
        &export,
        &destination,
        None,
        &mut Silent,
        &|_| false,
    )
    .unwrap();
    assert!(outcome.files.iter().all(|f| f.result.is_ok()));
    assert_ne!(std::fs::read(&existing).unwrap(), b"keep");
}

#[test]
fn a_cancelled_export_writes_nothing() {
    let temp = TempDirectory::new();
    let format = ModelFileFormat::Stl;
    let stop = |c: Checkpoint| c == Checkpoint::BooleanCells;
    let result = export_models(
        &request(),
        &options(format, ModelFiles::PerPlate),
        &named(&temp.0, "plate", format),
        None,
        &mut Silent,
        &stop,
    );
    assert!(result.is_err());
    assert_eq!(std::fs::read_dir(&temp.0).unwrap().count(), 0);
}

#[test]
fn file_names_come_from_the_pattern() {
    let fields = NameFields {
        project: "Venom",
        plate: "Arms",
        number: Some(2),
        model: Some("Left arm"),
    };
    assert_eq!(model_file_name("{plate} {model}", &fields), "Arms Left arm");
    assert_eq!(model_file_name("{project}_{number}", &fields), "Venom_2");
}

#[test]
fn a_job_exports_plates_one_after_another_and_stops_when_cancelled() {
    use karasu_project::jobs::{ModelExportEvent, ModelExportJob, PlateExport, PlateFiles};
    let temp = TempDirectory::new();
    let export = options(ModelFileFormat::Stl, ModelFiles::PerPlate);
    let plates = vec![
        PlateExport {
            request: request(),
            name: "Plate".into(),
            files: PlateFiles::One(temp.path("plate.stl")),
        },
        PlateExport {
            request: request(),
            name: "Plate".into(),
            files: PlateFiles::PerModel(vec![(1, temp.path("block.stl"))]),
        },
    ];
    let events = ModelExportJob::start(plates.clone(), export, None).wait();
    let mut done = Vec::new();
    for event in &events {
        if let ModelExportEvent::Plate { plate, result } = event {
            let outcome = result.as_ref().unwrap();
            assert!(outcome.files.iter().all(|f| f.result.is_ok()));
            done.push((*plate, outcome.files.len()));
        }
    }
    // The second plate's one model alone: the block.
    assert_eq!(done, [(0, 1), (1, 1)]);
    assert!(matches!(
        events.last(),
        Some(ModelExportEvent::Finished { cancelled: false })
    ));
    assert!(temp.path("plate.stl").is_file() && temp.path("block.stl").is_file());
    // Cancelled at once: nothing more is written.
    let other = TempDirectory::new();
    let plates: Vec<PlateExport> = plates
        .into_iter()
        .map(|mut p| {
            p.files = PlateFiles::One(other.path(&format!("{}.stl", p.name)));
            p
        })
        .collect();
    let job = ModelExportJob::start(plates, export, None);
    job.cancel();
    let events = job.wait();
    assert!(matches!(
        events.last(),
        Some(ModelExportEvent::Finished { cancelled: true })
    ));
    assert_eq!(std::fs::read_dir(&other.0).unwrap().count(), 0);
}

#[test]
fn each_model_with_its_supports_and_raft_is_one_object_unless_kept_apart() {
    let temp = TempDirectory::new();
    let format = ModelFileFormat::ThreeMf;
    let mut export = options(format, ModelFiles::PerPlate);
    export.objects = ModelObjects::OnePerModel;
    let outcome = export_models(
        &request(),
        &export,
        &named(&temp.0, "plate", format),
        None,
        &mut Silent,
        &|_| false,
    )
    .unwrap();
    let summary = outcome.files[0].result.as_ref().unwrap();
    // The slab with its supports and raft, and the block: two objects.
    assert_eq!(summary.bodies, 2);
    assert_eq!(read(&outcome.files[0].path).1, 2);
}
