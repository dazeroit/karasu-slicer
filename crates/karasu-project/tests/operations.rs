//! Object operations (TD-089, TD-090, M6.4c c3): the timeline of hollowing and drain holes, holes
//! in the model frame, and cavities reused by previews and slicing.
mod common;

use common::{box_stl, material, print_setting, printer, TempDirectory};
use karasu_core::preparation::{sampled::HoleSurface, HoleTilt};
use karasu_project::{
    pipeline::{prepare_with, slice, slice_with, Observer, PipelineCache, Silent},
    project::{
        Hollowing, ObjectOperation, OperationKind, PlateContent, PlateId, PlateObject,
        PlateSettings, Project, ProjectError,
    },
    request::{Blocker, DrainHole},
};
use std::fs;

const HOLLOW: Hollowing = Hollowing {
    wall_mm: 2.,
    cell_mm: Some(0.5),
    simplify: None,
};

/// A hole on the block's +Y face, in its model frame.
fn hole() -> DrainHole {
    DrainHole::Surface {
        surface_mm: [10., 20., 5.],
        diameter_mm: 3.,
        outside_mm: 1.,
        inside_mm: 1.,
        tilt: None,
        depth_mm: Some(3.),
        surface: HoleSurface::Outer,
    }
}

/// A 20 x 20 x 10 mm block at the model origin, placed at `translate_mm` on one plate.
fn plate(object: PlateObject) -> (Project, PlateId) {
    let mut project = Project::new("Operations");
    let model = project.add_model(
        "block.stl",
        box_stl([0., 0., 0.], [20., 20., 10.]).into_bytes(),
    );
    let mut object = PlateObject { model, ..object };
    object.arrange = false;
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

fn block() -> PlateObject {
    PlateObject::new("block", karasu_project::project::ModelId([0; 32]))
}

#[test]
fn operations_keep_their_identities_and_order() {
    let mut object = block();
    let first = object.push_operation(OperationKind::DrainHole(hole()));
    object.set_hollowing(Some(HOLLOW));
    let second = object.push_operation(OperationKind::DrainHole(hole()));
    // Hollowing goes first, with a new identity; holes keep theirs.
    assert!(matches!(
        object.operations[0].kind,
        OperationKind::Hollow(_)
    ));
    let hollow_id = object.operations[0].id;
    assert_eq!(
        [first, hollow_id, second],
        [1, 2, 3],
        "{:?}",
        object.operations
    );
    // Applying other settings replaces the hollowing in place.
    object.set_hollowing(Some(Hollowing {
        wall_mm: 3.,
        ..HOLLOW
    }));
    assert_eq!(object.operations[0].id, hollow_id);
    assert_eq!(object.hollowing().unwrap().wall_mm, 3.);
    // A suspended operation stays in the timeline but does not apply.
    object.operations[1].enabled = false;
    assert_eq!(object.drain_holes().count(), 1);
    // Removed operations' identities are never reused while later ones exist.
    object.remove_operation(first);
    assert_eq!(object.push_operation(OperationKind::DrainHole(hole())), 4);
    object.set_hollowing(None);
    assert!(object.hollowing().is_none());
    assert_eq!(object.drain_holes().count(), 2);
}

#[test]
fn an_object_is_hollowed_once() {
    let mut object = block();
    object.set_hollowing(Some(HOLLOW));
    object.operations.push(ObjectOperation {
        id: 9,
        enabled: true,
        kind: OperationKind::Hollow(HOLLOW),
    });
    let (project, plate) = plate(object);
    let error = project
        .request(plate, std::env::temp_dir().join("unused.goo"))
        .unwrap_err();
    assert!(matches!(error, ProjectError::Invalid { .. }), "{error}");
    assert!(error.to_string().contains("hollowed 2 times"), "{error}");
}

#[test]
fn holes_in_the_model_frame_follow_the_object() {
    let prepared_hole = |translate_mm: [f64; 3], mirror: bool| {
        let mut object = block();
        object.translate_mm = translate_mm;
        object.pose.mirror = [mirror, false, false];
        object.set_hollowing(Some(HOLLOW));
        object.push_operation(OperationKind::DrainHole(hole()));
        let (project, plate) = plate(object);
        let (request, _) = project
            .request(plate, std::env::temp_dir().join("unused.goo"))
            .unwrap();
        let preparation =
            karasu_project::pipeline::prepare(&request, &mut Silent, &|_| false).unwrap();
        let object = preparation.objects[0].as_ref().unwrap();
        let holes = object.holes().unwrap();
        assert!(holes.findings()[0].opens_outside && holes.findings()[0].reaches_cavity);
        holes.specs()[0]
    };
    let here = prepared_hole([0., 0., 0.], false);
    let moved = prepared_hole([30., 15., 0.], false);
    // The same hole, 30 mm along X and 15 along Y, in the same direction.
    for (k, shift) in [30., 15., 0.].into_iter().enumerate() {
        assert!(
            (moved.entry_mm[k] - here.entry_mm[k] - shift).abs() < 1e-9,
            "{here:?} {moved:?}"
        );
    }
    assert_eq!(moved.direction, here.direction);
    // Mirrored about X, the block keeps its +Y face, and the hole stays on it.
    let mirrored = prepared_hole([0., 0., 0.], true);
    assert_eq!(mirrored.direction, here.direction);
}

#[test]
fn a_tilted_hole_leans_the_same_way_on_the_turned_object() {
    let direction = |turn_deg: f64| {
        let mut object = block();
        object.translate_mm = [20., 20., 0.];
        object.pose.rotate_deg[2] = turn_deg;
        object.set_hollowing(Some(HOLLOW));
        // On the +Y face, leaning 20 degrees towards the model's +Z.
        let DrainHole::Surface {
            surface_mm,
            diameter_mm,
            outside_mm,
            inside_mm,
            depth_mm,
            surface,
            ..
        } = hole()
        else {
            unreachable!()
        };
        object.push_operation(OperationKind::DrainHole(DrainHole::Surface {
            surface_mm,
            diameter_mm,
            outside_mm,
            inside_mm,
            tilt: Some(HoleTilt {
                angle_deg: 20.,
                azimuth_deg: 0.,
            }),
            depth_mm,
            surface,
        }));
        let (project, plate) = plate(object);
        let (request, _) = project
            .request(plate, std::env::temp_dir().join("unused.goo"))
            .unwrap();
        let preparation =
            karasu_project::pipeline::prepare(&request, &mut Silent, &|_| false).unwrap();
        let object = preparation.objects[0].as_ref().unwrap();
        let spec = object.holes().unwrap().specs()[0];
        let length = spec.direction.iter().map(|c| c * c).sum::<f64>().sqrt();
        spec.direction.map(|c| c / length)
    };
    let upright = direction(0.);
    // Inward from +Y, rising towards +Z.
    assert!(upright[1] < -0.9 && upright[2] > 0.3, "{upright:?}");
    // Turned a quarter about Z, the axis turns with the block: (x, y, z) becomes (-y, x, z).
    let turned = direction(90.);
    let expected = [-upright[1], upright[0], upright[2]];
    for k in 0..3 {
        assert!(
            (turned[k] - expected[k]).abs() < 1e-6,
            "{turned:?} {expected:?}"
        );
    }
}

#[test]
fn previews_probe_only_objects_with_holes_or_holes_being_placed() {
    // The probe costs as much as the rest of a preview after a move: solid and hollowed objects
    // alike go without, unless they have holes or holes are being placed on them.
    for hollowing in [None, Some(HOLLOW)] {
        let probe = |holes: bool, probes: &[usize]| {
            let mut object = block();
            object.set_hollowing(hollowing);
            if holes {
                object.push_operation(OperationKind::DrainHole(DrainHole::Axis {
                    entry_mm: [10., 10., -1.],
                    direction: [0., 0., 1.],
                    length_mm: 4.,
                    diameter_mm: 3.,
                }));
            }
            let (project, plate) = plate(object);
            let (request, _) = project
                .request(plate, std::env::temp_dir().join("unused.goo"))
                .unwrap();
            let preparation =
                prepare_with(&request, None, probes, &mut Silent, &|_| false).unwrap();
            preparation.objects[0].as_ref().unwrap().probe().is_some()
        };
        assert!(!probe(false, &[]), "{hollowing:?}");
        assert!(probe(false, &[0]), "{hollowing:?}");
        assert!(probe(true, &[]), "{hollowing:?}");
    }
}

#[test]
fn previews_say_at_once_when_no_hole_can_be_indexed_and_keep_the_object() {
    // TD-094: a budget too small for the hole index. The preview prepares the object all the
    // same and says why holes cannot be added, before any is placed; slicing it with a hole
    // is refused.
    let request = |holes: bool| {
        let mut object = block();
        if holes {
            object.push_operation(OperationKind::DrainHole(hole()));
        }
        let (project, plate) = plate(object);
        let (mut request, _) = project
            .request(plate, std::env::temp_dir().join("unused.goo"))
            .unwrap();
        request.memory_budget_bytes = Some(64);
        request
    };
    for (holes, probes) in [(false, &[][..]), (false, &[0][..]), (true, &[][..])] {
        let preparation =
            prepare_with(&request(holes), None, probes, &mut Silent, &|_| false).unwrap();
        let object = preparation.objects[0].as_ref().unwrap();
        let reason = object.drainage_unavailable().unwrap().reason();
        assert!(reason.contains("cannot index"), "{reason}");
        assert!(object.probe().is_none() && object.holes().is_none());
    }
    let error = slice(&request(true), &mut Silent, &|_| false).unwrap_err();
    assert!(error.to_string().contains("cannot index"), "{error}");
    // Within the default budget, nothing is said.
    let mut fits = request(false);
    fits.memory_budget_bytes = None;
    let preparation = prepare_with(&fits, None, &[], &mut Silent, &|_| false).unwrap();
    assert!(preparation.objects[0]
        .as_ref()
        .unwrap()
        .drainage_unavailable()
        .is_none());
}

#[test]
fn the_predicted_minimum_wall_fits_the_cavity_cap() {
    use karasu_core::preparation::sampled::{
        guaranteed_cell, predicted_cavity_triangles, ProbeLimits, DEFAULT_CAVITY_TRIANGLE_BUDGET,
        DEFAULT_OVER_THICKNESS_MM,
    };
    use karasu_project::pipeline::{cavity_triangle_cap, predicted_minimum_wall_mm};
    // TD-094: the default cap is the hollowing's own limit, which the default hole index takes;
    // a budget caps the cavity at what its index takes.
    assert_eq!(cavity_triangle_cap(None), 20_000_000);
    assert!(ProbeLimits::default().max_faces() >= 20_000_000);
    assert_eq!(
        cavity_triangle_cap(Some(ProbeLimits::BYTES_PER_FACE * 1000)),
        1000
    );
    #[allow(clippy::cast_precision_loss)]
    let predicted = |area: f64, wall: f64, budget: Option<u64>| {
        let cell = guaranteed_cell(
            area,
            wall,
            1e-6,
            DEFAULT_OVER_THICKNESS_MM,
            DEFAULT_CAVITY_TRIANGLE_BUDGET,
        );
        1.1 * predicted_cavity_triangles(area, cell) / cavity_triangle_cap(budget) as f64
    };
    // With the 10 % margin, a source the size of `Base_01.stl`: about 0.28 mm by default,
    // thinner with more memory.
    let area = 52_600.0;
    let minimum = predicted_minimum_wall_mm(area, None);
    assert!((0.26..0.29).contains(&minimum), "{minimum}");
    assert!(predicted(area, minimum, None) <= 1.0);
    assert!(predicted(area, minimum * 0.99, None) > 1.0);
    let more = Some(ProbeLimits::BYTES_PER_FACE * 40_000_000);
    assert!(predicted_minimum_wall_mm(area, more) < minimum);
    // A small source reaches the thinnest wall considered.
    assert!(predicted_minimum_wall_mm(10.0, None) < 0.01);
}

#[test]
fn a_cavity_over_the_memory_budget_is_refused_with_a_remedy() {
    let mut object = block();
    object.set_hollowing(Some(HOLLOW));
    let (project, plate) = plate(object);
    let (mut request, _) = project
        .request(plate, std::env::temp_dir().join("unused.goo"))
        .unwrap();
    request.memory_budget_bytes = Some(64 << 10);
    let preparation = prepare_with(&request, None, &[], &mut Silent, &|_| false).unwrap();
    let error = preparation.objects[0].as_ref().unwrap_err().to_string();
    assert!(
        error.contains("cannot hollow")
            && error.contains("a thicker wall makes the cavity smaller"),
        "{error}"
    );
}

#[test]
fn previews_hollow_objects_out_of_the_build_volume_and_slicing_refuses_them() {
    // Beyond the 100 x 60 mm LCD area: the preview still hollows it, so the cavity stays
    // visible while the object is moved back.
    let mut object = block();
    object.translate_mm = [95., 50., 0.];
    object.set_hollowing(Some(HOLLOW));
    let (project, plate) = plate(object);
    let (request, _) = project
        .request(plate, std::env::temp_dir().join("unused.goo"))
        .unwrap();
    let preparation = karasu_project::pipeline::prepare(&request, &mut Silent, &|_| false).unwrap();
    let object = preparation.objects[0].as_ref().unwrap();
    assert!(object.hollowed().is_some());
    let error = slice(&request, &mut Silent, &|_| false).unwrap_err();
    assert!(
        error.to_string().contains("outside the LCD area"),
        "{error}"
    );
}

#[test]
fn previews_lifted_objects_with_their_holes_and_slicing_refuses_them() {
    // Lifted 20 mm without supports: the preview hollows it and cuts its hole where it is, as
    // the interface shows it while it is moved; slicing asks to rest it on the plate.
    let mut object = block();
    object.translate_mm[2] = 20.0;
    object.pose.drop = false;
    object.set_hollowing(Some(HOLLOW));
    object.push_operation(OperationKind::DrainHole(hole()));
    let (project, plate) = plate(object);
    let (request, _) = project
        .request(plate, std::env::temp_dir().join("unused.goo"))
        .unwrap();
    let preparation = karasu_project::pipeline::prepare(&request, &mut Silent, &|_| false).unwrap();
    let object = preparation.objects[0].as_ref().unwrap();
    assert!(object.hollowed().is_some() && object.holes().is_some());
    let error = slice(&request, &mut Silent, &|_| false).unwrap_err();
    assert!(
        error.to_string().contains("must rest on the build plate"),
        "{error}"
    );
    // Lifted as the Supports tool lifts it, before any support is placed (TD-108): prepared
    // where it is, and slicing asks for it to rest on the plate as well.
    let mut object = block();
    object.pose.drop = true;
    object.pose.lift_mm = Some(5.0);
    let (project, lifted) = self::plate(object);
    let (request, _) = project
        .request(lifted, std::env::temp_dir().join("unused.goo"))
        .unwrap();
    let preparation = karasu_project::pipeline::prepare(&request, &mut Silent, &|_| false).unwrap();
    assert!(preparation.objects[0].is_ok());
    let error = slice(&request, &mut Silent, &|_| false).unwrap_err();
    assert!(
        error.to_string().contains("must rest on the build plate"),
        "{error}"
    );
    // For an analysis only (the islands search), it is sliced where it stands: its bottom
    // starts in mid-air.
    let mut floating = request;
    floating.allow_floating = true;
    floating.find_unsupported = Some(45.0);
    floating.format = karasu_project::request::REFERENCE_FORMAT.to_owned();
    let output = std::env::temp_dir().join(format!("karasu-floating-{}", std::process::id()));
    floating.output = output.clone();
    let outcome = slice(&floating, &mut Silent, &|_| false).unwrap();
    let found = outcome.unsupported.expect("unsupported regions");
    assert_eq!(found.report.islands(), 1);
    let island = &found.report.regions[0];
    assert!((found.world(island)[2] - 5.0).abs() < 0.2);
    let _ = std::fs::remove_file(&output);
}

/// The phases a job reports.
#[derive(Default)]
struct Phases(Vec<String>);

impl Observer for Phases {
    fn phase(&mut self, name: &str) {
        self.0.push(name.to_owned());
    }
    fn begin_layers(&mut self, _: usize) {}
    fn layers(&mut self, _: usize) {}
}

#[test]
fn hole_edits_reuse_the_probe_and_a_move_indexes_again() {
    // TD-094: the probe is kept under the cavity and the exact placement.
    let temp = TempDirectory::new();
    let mut object = block();
    object.set_hollowing(Some(HOLLOW));
    object.push_operation(OperationKind::DrainHole(hole()));
    let (mut project, plate) = plate(object);
    let cache = PipelineCache::default();
    let request =
        |project: &Project, name: &str| project.request(plate, temp.path(name)).unwrap().0;
    let phases = |project: &Project| {
        let mut phases = Phases::default();
        prepare_with(
            &request(project, "unused"),
            Some(&cache),
            &[],
            &mut phases,
            &|_| false,
        )
        .unwrap();
        phases.0
    };
    assert!(phases(&project)
        .iter()
        .any(|p| p == "Drain holes: indexing"));
    assert_eq!(cache.probes(), 1);
    // Another hole: the same probe.
    project
        .edit_plate(plate, |content| {
            content.objects[0].push_operation(OperationKind::DrainHole(hole()));
        })
        .unwrap();
    let edited = phases(&project);
    assert!(edited.iter().any(|p| p == "Drain holes: index reused"));
    assert!(!edited.iter().any(|p| p == "Drain holes: indexing"));
    // Moved: indexed again where it now is, and slicing from the cache writes what slicing
    // without one writes.
    project
        .edit_plate(plate, |content| {
            content.objects[0].translate_mm = [25., 10., 0.];
        })
        .unwrap();
    assert!(phases(&project)
        .iter()
        .any(|p| p == "Drain holes: indexing"));
    slice_with(
        &request(&project, "cached.goo"),
        Some(&cache),
        &mut Silent,
        &|_| false,
    )
    .unwrap();
    slice(&request(&project, "fresh.goo"), &mut Silent, &|_| false).unwrap();
    assert_eq!(
        fs::read(temp.path("cached.goo")).unwrap(),
        fs::read(temp.path("fresh.goo")).unwrap()
    );
}

#[test]
fn previews_and_slices_share_cavities_and_outputs_do_not_change() {
    let temp = TempDirectory::new();
    let mut object = block();
    object.set_hollowing(Some(HOLLOW));
    object.push_operation(OperationKind::DrainHole(hole()));
    let (mut project, plate) = plate(object);
    let cache = PipelineCache::default();
    let request =
        |project: &Project, name: &str| project.request(plate, temp.path(name)).unwrap().0;
    // A preview hollows once and keeps the cavity.
    let mut phases = Phases::default();
    prepare_with(
        &request(&project, "unused"),
        Some(&cache),
        &[],
        &mut phases,
        &|_| false,
    )
    .unwrap();
    assert!(phases.0.iter().any(|p| p == "Hollowing"));
    assert_eq!((cache.cavities(), cache.models()), (1, 1));
    // Slicing takes it, and writes exactly what slicing without a cache writes.
    let mut phases = Phases::default();
    slice_with(
        &request(&project, "cached.goo"),
        Some(&cache),
        &mut phases,
        &|_| false,
    )
    .unwrap();
    assert!(phases.0.iter().any(|p| p == "Hollowing: cavity reused"));
    assert!(!phases.0.iter().any(|p| p == "Hollowing"));
    // The model, imported, checked and repaired by the preview, is not read again.
    assert!(phases.0.iter().any(|p| p == "Importing model: reused"));
    assert!(!phases.0.iter().any(|p| p == "Checking topology"));
    slice(&request(&project, "fresh.goo"), &mut Silent, &|_| false).unwrap();
    assert_eq!(
        fs::read(temp.path("cached.goo")).unwrap(),
        fs::read(temp.path("fresh.goo")).unwrap()
    );
    // Moving the object or editing its holes keeps the cavity.
    project
        .edit_plate(plate, |content| {
            content.objects[0].translate_mm = [25., 10., 0.];
            content.objects[0].push_operation(OperationKind::DrainHole(hole()));
        })
        .unwrap();
    let mut phases = Phases::default();
    prepare_with(
        &request(&project, "unused"),
        Some(&cache),
        &[],
        &mut phases,
        &|_| false,
    )
    .unwrap();
    assert!(phases.0.iter().any(|p| p == "Hollowing: cavity reused"));
    // Another wall is another cavity.
    project
        .edit_plate(plate, |content| {
            content.objects[0].set_hollowing(Some(Hollowing {
                wall_mm: 2.5,
                ..HOLLOW
            }));
        })
        .unwrap();
    let mut phases = Phases::default();
    prepare_with(
        &request(&project, "unused"),
        Some(&cache),
        &[],
        &mut phases,
        &|_| false,
    )
    .unwrap();
    assert!(phases.0.iter().any(|p| p == "Hollowing"));
    assert_eq!((cache.cavities(), cache.models()), (2, 1));
}

#[test]
fn imported_models_are_kept_and_repaired_again_only_for_new_settings() {
    let temp = TempDirectory::new();
    let (mut project, plate) = plate(block());
    let cache = PipelineCache::default();
    let slice_into = |project: &Project, name: &str| {
        let mut phases = Phases::default();
        let (request, _) = project.request(plate, temp.path(name)).unwrap();
        slice_with(&request, Some(&cache), &mut phases, &|_| false).unwrap();
        slice(
            &project
                .request(plate, temp.path(&format!("fresh-{name}")))
                .unwrap()
                .0,
            &mut Silent,
            &|_| false,
        )
        .unwrap();
        // Cached or not, the same bytes.
        assert_eq!(
            fs::read(temp.path(name)).unwrap(),
            fs::read(temp.path(&format!("fresh-{name}"))).unwrap()
        );
        phases.0
    };
    let first = slice_into(&project, "a.goo");
    assert!(first.iter().any(|p| p == "Checking topology"));
    assert!(!first.iter().any(|p| p == "Importing model: reused"));
    // Moved: nothing is imported, checked or repaired again.
    project
        .edit_plate(plate, |content| {
            content.objects[0].translate_mm = [30., 20., 0.];
        })
        .unwrap();
    let moved = slice_into(&project, "b.goo");
    assert!(moved.iter().any(|p| p == "Importing model: reused"));
    assert!(!moved.iter().any(|p| p == "Checking topology"));
    // Other repairs: the import is kept, the repairs are made for the new settings.
    project
        .edit_plate(plate, |content| {
            content.settings.repair.duplicates = !content.settings.repair.duplicates;
        })
        .unwrap();
    let repaired = slice_into(&project, "c.goo");
    assert!(repaired.iter().any(|p| p == "Importing model: reused"));
    assert!(repaired.iter().any(|p| p == "Checking topology"));
    assert_eq!(cache.models(), 1);
}

#[test]
fn removing_the_hollowing_returns_exactly_to_the_solid() {
    let temp = TempDirectory::new();
    let (mut project, plate) = plate(block());
    project
        .request(plate, temp.path("never.goo"))
        .map(|(request, _)| slice(&request, &mut Silent, &|_| false).unwrap())
        .unwrap();
    project
        .edit_plate(plate, |content| {
            content.objects[0].set_hollowing(Some(HOLLOW));
        })
        .unwrap();
    project
        .edit_plate(plate, |content| content.objects[0].set_hollowing(None))
        .unwrap();
    project
        .request(plate, temp.path("removed.goo"))
        .map(|(request, _)| slice(&request, &mut Silent, &|_| false).unwrap())
        .unwrap();
    assert_eq!(
        fs::read(temp.path("never.goo")).unwrap(),
        fs::read(temp.path("removed.goo")).unwrap()
    );
}

#[test]
fn a_blocker_keeps_its_region_of_the_cavity_solid() {
    // Owner, 2026-11-02: a box over the block's half x < 10, in its model frame, leaves that half
    // of the cavity solid; previews do not reuse the cavity hollowed without it.
    let cavity_volume = |blocker: Option<Blocker>| {
        let mut object = block();
        object.set_hollowing(Some(HOLLOW));
        if let Some(blocker) = blocker {
            object.push_operation(OperationKind::Blocker(blocker));
        }
        let (project, plate) = plate(object);
        let request = project
            .request(plate, std::env::temp_dir().join("unused.goo"))
            .unwrap()
            .0;
        let preparation = prepare_with(&request, None, &[], &mut Silent, &|_| false).unwrap();
        let object = preparation.objects[0].as_ref().unwrap();
        object.hollowed().unwrap().certificate().cavity_volume_mm3
    };
    let whole = cavity_volume(None);
    let half = cavity_volume(Some(Blocker::Frustum {
        centre_mm: [5.0, 10.0, 5.0],
        rotation_deg: [0.0; 3],
        bottom_mm: [10.0, 30.0],
        top_mm: [10.0, 30.0],
        height_mm: 30.0,
    }));
    // The cavity [2, 18] × [2, 18] × [2, 8] (its edges rounded and its wall raised for the
    // guarantee) loses x < 10: half its length.
    assert!((whole - 16.0 * 16.0 * 6.0).abs() / whole < 0.06, "{whole}");
    assert!((half / whole - 0.5).abs() < 0.03, "{half} of {whole}");
    // A blocker turned a quarter about Z with its width along Y keeps the same half.
    let turned = cavity_volume(Some(Blocker::Frustum {
        centre_mm: [5.0, 10.0, 5.0],
        rotation_deg: [0.0, 0.0, 90.0],
        bottom_mm: [30.0, 10.0],
        top_mm: [30.0, 10.0],
        height_mm: 30.0,
    }));
    assert!((turned - half).abs() < 1e-6, "{turned} {half}");
}
