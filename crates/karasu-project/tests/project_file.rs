//! The project file (TD-130): a project saved as a `.karasu` package reads back equal, saves to
//! the same bytes, slices to the same files after reopening, and a damaged or foreign package is
//! refused with where and why.
mod common;

use common::{box_stl, material, print_setting, printer, TempDirectory};
use karasu_core::{
    archive::{ArchiveLimits, ArchiveReader, ArchiveWriter},
    layer_plan::LayerPlanDocument,
    mesh::formats::LengthUnit,
    preparation::{repair::RepairPreset, sampled::HoleSurface, HoleTilt},
    supports::SupportDocument,
};
use karasu_project::{
    pipeline::{slice, Silent},
    project::{
        file::{self, ProjectFileError, DOCUMENT_ENTRY},
        Hollowing, ImageOverride, OperationKind, PlateContent, PlateId, PlateObject, PlateSettings,
        Project, ProtectedPoint,
    },
    request::{Blocker, DrainHole, Simplify},
};
use std::fs;

const HOLLOW: Hollowing = Hollowing {
    wall_mm: 2.,
    cell_mm: Some(0.5),
    simplify: None,
};

/// A 20 x 20 x 10 mm block, hollowed and drilled, lifted 6 mm on two supports.
fn prepared(project: &mut Project) -> PlateObject {
    let model = project.add_model(
        "Block.STL",
        box_stl([0., 0., 0.], [20., 20., 10.]).into_bytes(),
    );
    let mut object = PlateObject::new("block", model);
    object.arrange = false;
    object.pose.lift_mm = Some(6.);
    object.set_hollowing(Some(HOLLOW));
    object.push_operation(OperationKind::DrainHole(DrainHole::Surface {
        surface_mm: [10., 20., 5.],
        diameter_mm: 3.,
        outside_mm: 1.,
        inside_mm: 1.,
        tilt: None,
        depth_mm: Some(3.),
        surface: HoleSurface::Outer,
    }));
    object.supports = Some(
        SupportDocument::from_json(
            br#"{"format":"karasu-supports","version":1,"preset":"light",
                "supports":[{"id":1,"tip":{"point":[5,5,0]},"base":{"vertical":true}},
                            {"id":2,"tip":{"point":[15,15,0]},"base":{"vertical":true},
                             "parameters":{"contact_diameter_mm":0.35}}]}"#,
        )
        .unwrap(),
    );
    object
}

/// Every kind of value a project holds: two plates, one empty without profiles; an object with
/// every operation, units, mirroring, copies and protected points; a layer plan.
fn rich() -> (Project, PlateId) {
    let mut project = Project::new("Rich «project»");
    let mut object = prepared(&mut project);
    object.set_repair(Some(RepairPreset::Standard.spec()));
    object.set_hollowing(Some(Hollowing {
        simplify: Some(Simplify::Auto),
        ..HOLLOW
    }));
    object.push_operation(OperationKind::DrainHole(DrainHole::Axis {
        entry_mm: [10., 10., -1.],
        direction: [0., 0.1, 1.],
        length_mm: 4.5,
        diameter_mm: 2.5,
    }));
    let tilted = object.push_operation(OperationKind::DrainHole(DrainHole::Surface {
        surface_mm: [0., 10., 5.],
        diameter_mm: 2.,
        outside_mm: 0.5,
        inside_mm: 1.5,
        tilt: Some(HoleTilt {
            angle_deg: 22.5,
            azimuth_deg: -90.,
        }),
        depth_mm: None,
        surface: HoleSurface::Cavity,
    }));
    object
        .operations
        .iter_mut()
        .find(|o| o.id == tilted)
        .unwrap()
        .enabled = false;
    object.push_operation(OperationKind::Blocker(Blocker::Frustum {
        centre_mm: [10., 10., 2.],
        rotation_deg: [0., 15., 30.],
        bottom_mm: [4., 3.],
        top_mm: [2., 0.],
        height_mm: 3.,
    }));
    object.push_operation(OperationKind::Blocker(Blocker::Sphere {
        centre_mm: [5., 5., 5.],
        diameter_mm: 1.25,
    }));
    object.protected = vec![ProtectedPoint {
        point_mm: [10., 0., 5.],
        normal: [0., -1., 0.],
    }];
    let mut copy = object.clone();
    copy.name = "copies".into();
    copy.units = Some(LengthUnit::Inch);
    copy.pose.mirror = [true, false, true];
    copy.pose.rotate_deg = [0., 0., 33.3];
    copy.pose.scale = [0.05, 0.05, 0.05];
    copy.copies = 3;
    copy.arrange = true;
    let mut settings = PlateSettings::new("goo-v3");
    settings.layer_plan = Some(
        LayerPlanDocument::from_json(
            br#"{"format":"karasu-layers","version":1,"ranges":[
                {"from_mm":0,"height_mm":0.05},{"from_mm":2,"height_mm":0.03}]}"#,
        )
        .unwrap(),
    );
    settings.reversed_tolerance_mm2 = Some(0.5);
    let plate = project.add_plate(PlateContent {
        name: "Plate".into(),
        printer: Some(printer()),
        resin: Some(material()),
        print_setting: Some(print_setting()),
        settings,
        objects: vec![object, copy],
    });
    let empty = project.add_plate(PlateContent {
        name: "Empty".into(),
        printer: None,
        resin: None,
        print_setting: None,
        settings: PlateSettings::new("ctb-v4-plain"),
        objects: Vec::new(),
    });
    // A removed plate's identity is not reused when the project is reopened.
    project.remove_plate(empty);
    project.add_plate(PlateContent {
        name: "Second".into(),
        printer: None,
        resin: None,
        print_setting: None,
        settings: PlateSettings::new("ctb-v4-plain"),
        objects: Vec::new(),
    });
    // A model no plate uses is not saved.
    project.add_model("unused.stl", b"solid unused\nendsolid unused\n".to_vec());
    (project, plate)
}

#[test]
fn a_project_reads_back_equal_and_saves_to_the_same_bytes() {
    let (mut project, plate) = rich();
    // An edit leaves history and a revision past 1: neither is saved.
    project
        .edit_plate(plate, |p| p.objects[1].translate_mm = [1.5, -2.25, 0.])
        .unwrap();
    let bytes = file::to_bytes(&project);
    let opened = file::from_bytes(&bytes).unwrap();
    assert_eq!(opened.name, project.name);
    let used: Vec<_> = project
        .models()
        .iter()
        .filter(|m| m.name != "unused.stl")
        .cloned()
        .collect();
    assert_eq!(opened.models(), used.as_slice());
    let ids = |p: &Project| {
        p.plates()
            .iter()
            .map(karasu_project::project::Plate::id)
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&opened), ids(&project));
    // Each plate read starts at a new revision of its own (TD-135), with no history.
    for ((read, saved), revision) in opened.plates().iter().zip(project.plates()).zip(1..) {
        assert_eq!(read.content(), saved.content());
        assert_eq!(read.revision(), revision);
        assert!(!read.can_undo() && !read.can_redo());
    }
    assert_eq!(opened.history(opened.plates()[0].id()), (false, false));
    // A new plate takes an identity never used.
    let mut reopened = opened.clone();
    let next = reopened.add_plate(project.plates()[0].content().clone());
    assert!(ids(&project).iter().all(|&id| id < next), "{next:?}");
    // Deterministic: the same project saves to the same bytes, and so does the one read.
    assert_eq!(file::to_bytes(&project), bytes);
    assert_eq!(file::to_bytes(&opened), bytes);
    // The document comes first, then the models in the project's order.
    let reader = ArchiveReader::open(&bytes, limits()).unwrap();
    let names: Vec<&str> = reader.names().collect();
    assert_eq!(names.len(), 2);
    assert_eq!(names[0], DOCUMENT_ENTRY);
    assert!(
        names[1].starts_with("models/")
            && std::path::Path::new(names[1])
                .extension()
                .is_some_and(|e| e == "stl"),
        "{names:?}"
    );
}

#[test]
fn a_project_saved_and_reopened_slices_to_the_same_files() {
    let temp = TempDirectory::new();
    let mut project = Project::new("Recompute");
    let object = prepared(&mut project);
    let plate = project.add_plate(PlateContent {
        name: "Plate".into(),
        printer: Some(printer()),
        resin: Some(material()),
        print_setting: Some(print_setting()),
        settings: PlateSettings::new("goo-v3"),
        objects: vec![object],
    });
    let path = temp.path("recompute.karasu");
    file::save(&project, &path).unwrap();
    let reopened = file::open(&path).unwrap();
    for (project, output) in [(&project, "before.goo"), (&reopened, "after.goo")] {
        let (request, _) = project.request(plate, temp.path(output)).unwrap();
        slice(&request, &mut Silent, &|_| false).unwrap();
    }
    let before = fs::read(temp.path("before.goo")).unwrap();
    assert_ne!(before.len(), 0);
    assert_eq!(fs::read(temp.path("after.goo")).unwrap(), before);
}

#[test]
fn a_save_replaces_the_file_whole_or_leaves_it() {
    let temp = TempDirectory::new();
    let (project, _) = rich();
    let path = temp.path("project.karasu");
    file::save(&project, &path).unwrap();
    let mut renamed = project.clone();
    renamed.name = "Renamed".into();
    file::save(&renamed, &path).unwrap();
    assert_eq!(file::open(&path).unwrap().name, "Renamed");
    // A target that cannot be replaced (a folder) is reported, and no temporary file is left.
    let folder = temp.path("folder.karasu");
    fs::create_dir(&folder).unwrap();
    fs::write(folder.join("inside"), b"kept").unwrap();
    let error = file::save(&project, &folder).unwrap_err();
    assert!(matches!(error, ProjectFileError::Io { .. }), "{error}");
    assert_eq!(fs::read(folder.join("inside")).unwrap(), b"kept");
    let left: Vec<_> = fs::read_dir(&temp.0)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| {
            std::path::Path::new(name)
                .extension()
                .is_some_and(|e| e == "tmp")
        })
        .collect();
    assert!(left.is_empty(), "{left:?}");
    // A folder that does not exist.
    let error = file::save(&project, &temp.path("missing/project.karasu")).unwrap_err();
    assert!(matches!(error, ProjectFileError::Io { .. }), "{error}");
}

fn limits() -> ArchiveLimits {
    ArchiveLimits {
        max_entries: 64,
        max_entry_bytes: 1 << 30,
    }
}

/// The package `bytes` with each entry passed through `change` (dropped when it returns
/// `None`), then `extra` entries added.
fn repacked(
    bytes: &[u8],
    change: impl Fn(&str, Vec<u8>) -> Option<Vec<u8>>,
    extra: &[(&str, &[u8])],
) -> Vec<u8> {
    let reader = ArchiveReader::open(bytes, limits()).unwrap();
    let mut writer = ArchiveWriter::new(Vec::new());
    for name in reader.names() {
        let data = reader.read(name).unwrap().unwrap();
        if let Some(data) = change(name, data) {
            writer.add(name, &data).unwrap();
        }
    }
    for (name, data) in extra {
        writer.add(name, data).unwrap();
    }
    writer.finish().unwrap()
}

/// The package with its document edited as JSON text.
fn with_document(bytes: &[u8], edit: impl Fn(String) -> String) -> Vec<u8> {
    repacked(
        bytes,
        |name, data| {
            Some(if name == DOCUMENT_ENTRY {
                edit(String::from_utf8(data).unwrap()).into_bytes()
            } else {
                data
            })
        },
        &[],
    )
}

#[test]
fn a_damaged_or_foreign_package_is_refused_with_where_and_why() {
    let (project, _) = rich();
    let bytes = file::to_bytes(&project);
    let refused = |bytes: &[u8]| file::from_bytes(bytes).unwrap_err();
    // Not a package at all, or a package without a project.
    assert!(matches!(
        refused(b"solid block\nendsolid block\n"),
        ProjectFileError::NotAProject(_)
    ));
    let mut empty = ArchiveWriter::new(Vec::new());
    empty.add("model.stl", b"solid").unwrap();
    assert!(matches!(
        refused(&empty.finish().unwrap()),
        ProjectFileError::NotAProject(_)
    ));
    // Truncated.
    assert!(file::from_bytes(&bytes[..bytes.len() / 2]).is_err());
    // Another format, a newer version, an unknown field.
    let foreign = with_document(&bytes, |d| d.replacen("\"karasu-project\"", "\"other\"", 1));
    assert!(matches!(
        refused(&foreign),
        ProjectFileError::NotAProject(_)
    ));
    let newer = with_document(&bytes, |d| d.replacen("\"version\":1", "\"version\":2", 1));
    let error = refused(&newer);
    assert!(
        matches!(error, ProjectFileError::Newer { version: 2 }),
        "{error}"
    );
    assert!(error.to_string().contains("newer karasu-slicer"), "{error}");
    let unknown = with_document(&bytes, |d| d.replacen('{', "{\"colour\":\"red\",", 1));
    assert!(matches!(refused(&unknown), ProjectFileError::Json(_)));
    // A model altered, missing, or an entry the document does not list.
    let model = ArchiveReader::open(&bytes, limits())
        .unwrap()
        .names()
        .nth(1)
        .unwrap()
        .to_owned();
    let altered = repacked(
        &bytes,
        |name, mut data| {
            if name == model {
                data[0] ^= 1;
            }
            Some(data)
        },
        &[],
    );
    let error = refused(&altered);
    assert!(error.to_string().contains("altered"), "{error}");
    let missing = repacked(&bytes, |name, data| (name != model).then_some(data), &[]);
    let error = refused(&missing);
    assert!(error.to_string().contains("does not hold"), "{error}");
    let extra = repacked(&bytes, |_, data| Some(data), &[("notes.txt", b"hello")]);
    let error = refused(&extra);
    assert!(error.to_string().contains("notes.txt"), "{error}");
    // A value out of range is reported with its plate and object.
    let flat = with_document(&bytes, |d| {
        d.replacen("\"scale\":[1.0,1.0,1.0]", "\"scale\":[1.0,0.0,1.0]", 1)
    });
    let error = refused(&flat);
    assert!(matches!(error, ProjectFileError::Invalid { .. }), "{error}");
    let message = error.to_string();
    assert!(
        message.contains("plate 1") && message.contains("object 1") && message.contains("scale"),
        "{message}"
    );
    // Two plates with one identity.
    let twins = with_document(&bytes, |d| d.replace("\"id\":3,", "\"id\":1,"));
    let error = refused(&twins);
    assert!(error.to_string().contains("unique"), "{error}");
    // Supports the core refuses.
    let wrong = with_document(&bytes, |d| {
        d.replacen(
            "\"contact_diameter_mm\":0.35",
            "\"contact_diameter_mm\":-1",
            1,
        )
    });
    let error = refused(&wrong);
    assert!(error.to_string().contains("supports"), "{error}");
}

#[test]
fn a_package_beyond_the_classic_zip_limits_reads_back() {
    // ZIP64 records, as a package beyond 4 GiB has them, read back the same project.
    let (project, _) = rich();
    let reader_bytes = file::to_bytes(&project);
    let reader = ArchiveReader::open(&reader_bytes, limits()).unwrap();
    let mut wide = ArchiveWriter::new(Vec::new()).with_zip64_always();
    for name in reader.names() {
        wide.add(name, &reader.read(name).unwrap().unwrap())
            .unwrap();
    }
    let opened = file::from_bytes(&wide.finish().unwrap()).unwrap();
    assert_eq!(file::to_bytes(&opened), reader_bytes);
}

#[test]
#[ignore = "saves and opens a local large model; set KARASU_PROJECT_MODEL to its path"]
fn a_large_model_saves_and_opens_in_measured_time() {
    // TD-130 measurements: KARASU_PROJECT_MODEL=<model.stl> cargo test --release -p
    // karasu-project --test project_file -- --ignored --nocapture
    let Ok(model) = std::env::var("KARASU_PROJECT_MODEL") else {
        return;
    };
    let bytes = fs::read(&model).unwrap();
    let mut project = Project::new("Large");
    let id = project.add_model(
        std::path::Path::new(&model)
            .file_name()
            .unwrap()
            .to_string_lossy(),
        bytes.clone(),
    );
    project.add_plate(PlateContent {
        name: "Plate".into(),
        printer: Some(printer()),
        resin: Some(material()),
        print_setting: Some(print_setting()),
        settings: PlateSettings::new("goo-v3"),
        objects: vec![PlateObject::new("model", id)],
    });
    let temp = TempDirectory::new();
    let path = temp.path("large.karasu");
    let started = std::time::Instant::now();
    file::save(&project, &path).unwrap();
    let saved = started.elapsed().as_secs_f64();
    let size = fs::metadata(&path).unwrap().len();
    let started = std::time::Instant::now();
    let opened = file::open(&path).unwrap();
    let read = started.elapsed().as_secs_f64();
    assert_eq!(opened.models()[0].bytes.as_ref(), bytes.as_slice());
    #[allow(clippy::cast_precision_loss)] // Sizes in MB, for the report.
    let mb = |n: u64| n as f64 / 1e6;
    println!(
        "model {:.1} MB, project {:.1} MB ({:.0} %); saved in {saved:.2} s, opened in {read:.2} s",
        mb(bytes.len() as u64),
        mb(size),
        100.0 * mb(size) / mb(bytes.len() as u64)
    );
}

#[test]
fn a_recovery_folder_keeps_each_model_once_and_reads_back_the_project() {
    let temp = TempDirectory::new();
    let dir = temp.path("recovery");
    let (mut project, plate) = rich();
    file::save_folder(&project, &dir).unwrap();
    let model = fs::read_dir(dir.join("models"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let written = fs::metadata(&model).unwrap().modified().unwrap();
    // An edit rewrites the document, not the model already there.
    project
        .edit_plate(plate, |p| p.objects[0].translate_mm = [4., 5., 0.])
        .unwrap();
    file::save_folder(&project, &dir).unwrap();
    assert_eq!(fs::metadata(&model).unwrap().modified().unwrap(), written);
    let opened = file::open_folder(&dir).unwrap();
    assert_eq!(file::to_bytes(&opened), file::to_bytes(&project));
    // A model no longer used leaves the folder with the object.
    project.edit_plate(plate, |p| p.objects.clear()).unwrap();
    project
        .edit_plate(plate, |p| p.name = "Empty now".into())
        .unwrap();
    file::save_folder(&project, &dir).unwrap();
    assert_eq!(fs::read_dir(dir.join("models")).unwrap().count(), 0);
    assert_eq!(
        file::open_folder(&dir).unwrap().plates()[0].content().name,
        "Empty now"
    );
    // A model altered in the folder is refused as in a package.
    let (project, _) = rich();
    file::save_folder(&project, &dir).unwrap();
    fs::write(&model, b"solid altered\nendsolid altered\n").unwrap();
    let error = file::open_folder(&dir).unwrap_err();
    assert!(error.to_string().contains("altered"), "{error}");
}

#[test]
fn a_plates_own_anti_aliasing_reads_back_and_slices_as_asked() {
    // TD-134: a plate's override of its print setting's anti-aliasing and smallest gray, and
    // its supports anti-aliased with the model or not.
    let (mut project, plate) = rich();
    let document = |project: &Project| {
        let bytes = file::to_bytes(project);
        let archive = ArchiveReader::open(&bytes, limits()).unwrap();
        String::from_utf8(archive.read(DOCUMENT_ENTRY).unwrap().unwrap()).unwrap()
    };
    // The plates' settings as written.
    let written = |project: &Project| -> Vec<serde_json::Value> {
        let value: serde_json::Value = serde_json::from_str(&document(project)).unwrap();
        value["plates"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["settings"].clone())
            .collect()
    };
    // Without one nothing is written, and the print setting's values apply.
    assert!(written(&project).iter().all(|s| s.get("image").is_none()));
    let request = |project: &Project| project.request(plate, "out.goo".into()).unwrap().0;
    let setting = print_setting().image;
    let plain = request(&project);
    assert_eq!(
        (plain.antialias, plain.min_gray, plain.antialias_supports),
        (setting.antialias, setting.min_gray, false)
    );
    // Anti-aliasing off clears the gray whatever is given; on, the gray given applies.
    let set = |project: &mut Project, image: ImageOverride| {
        project
            .edit_plate(plate, |content| content.settings.image = image)
            .unwrap();
    };
    set(
        &mut project,
        ImageOverride {
            antialias: Some(1),
            min_gray: Some(40),
            antialias_supports: true,
        },
    );
    let off = request(&project);
    assert_eq!(
        (off.antialias, off.min_gray, off.antialias_supports),
        (1, 0, true)
    );
    let image = ImageOverride {
        antialias: Some(4),
        min_gray: Some(40),
        antialias_supports: false,
    };
    set(&mut project, image);
    let on = request(&project);
    assert_eq!(
        (on.antialias, on.min_gray, on.antialias_supports),
        (4, 40, false)
    );
    // It reads back equal, written in the plate's settings.
    assert_eq!(
        written(&project)[0]["image"],
        serde_json::json!({"antialias": 4, "min_gray": 40})
    );
    let reopened = file::from_bytes(&file::to_bytes(&project)).unwrap();
    assert_eq!(
        reopened.plate(plate).unwrap().content().settings.image,
        image
    );
    // A level the masks cannot take is refused, with where.
    let bytes = file::to_bytes(&project);
    let bad = with_document(&bytes, |d| {
        d.replacen("\"antialias\":4", "\"antialias\":3", 1)
    });
    let error = file::from_bytes(&bad).unwrap_err().to_string();
    assert!(error.contains("antialias is 3"), "{error}");
}
