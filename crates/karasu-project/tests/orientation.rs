//! The Orient tool's search without the application (TD-158): tilts of one placed model proposed
//! under the constraints, each checked by slicing the model alone in that pose.
mod common;

use common::{box_stl, material, print_setting, printer, TempDirectory};
use karasu_core::{
    analysis::orientation::{Protected, Tilt},
    cancellation::Cancellation,
    mesh::formats::LengthUnit,
};
use karasu_project::{
    orient::{
        search, turn_rows, turned, Axes, Found, Inputs, OrientValues, Report, SearchError, Stage,
        CHECKED, DEFAULT_ORIENT, SHOWN,
    },
    pipeline::import_mesh,
    placement::{centre, place, plate_centre},
    project::{PlateContent, PlateObject, PlateSettings, Project},
    request::ModelSource,
    support_plan::generate::alone,
};
use std::sync::{atomic::AtomicBool, Arc};

/// The critical angle, in degrees from the plate.
const ANGLE_DEG: f64 = 45.0;

/// What a search told.
#[derive(Default)]
struct Told {
    stages: Vec<Stage>,
    shares: Vec<f32>,
}

impl Report for Told {
    fn stage(&mut self, stage: Stage) {
        self.stages.push(stage);
    }

    fn progress(&mut self, share: f32) {
        self.shares.push(share);
    }
}

/// The search's inputs for a 6 × 10 × 3 mm box lying flat on the plate, centred and dropped,
/// as the Orient tool makes them; `protected` in the world, in the pose now.
fn flat_box(temp: &TempDirectory, values: OrientValues, protected: Vec<Protected>) -> Inputs {
    let bytes = box_stl([0.0; 3], [6.0, 10.0, 3.0]).into_bytes();
    let mut project = Project::new("Orientation");
    let model = project.add_model("box.stl", bytes.clone());
    let mut object = PlateObject::new("box", model);
    object.pose.center = true;
    object.pose.drop = true;
    let plate = project.add_plate(PlateContent {
        name: "Plate 1".into(),
        printer: Some(printer()),
        resin: Some(material()),
        print_setting: Some(print_setting()),
        settings: PlateSettings::new("goo-v3"),
        objects: vec![object.clone()],
    });
    let output = temp.path("orientation.goo");
    let (request, _) = project.request(plate, output.clone()).unwrap();
    let request = alone(request, 0, output.clone(), "goo-v3".into(), ANGLE_DEG).unwrap();
    let source = ModelSource::Embedded {
        name: "box.stl".into(),
        bytes: Arc::from(bytes),
    };
    let mesh = import_mesh(&source, Some(LengthUnit::Millimeter), &|_| false)
        .unwrap()
        .mesh;
    let mut never = |_| false;
    let placement = place(
        &mesh,
        object.translate_mm,
        &object.pose,
        plate_centre(&printer()),
        &mut Cancellation::new(&mut never),
    )
    .unwrap();
    let anchor = placement
        .transform
        .transform_point(centre(&mesh).unwrap())
        .unwrap();
    Inputs {
        index: 0,
        source: Arc::new(mesh),
        placement: placement.transform,
        shell: None,
        protected,
        request,
        output,
        values,
        angle_deg: ANGLE_DEG,
        anchor: [anchor.x, anchor.y, anchor.z],
        min_z: placement.min.z,
        rotate_deg: object.pose.rotate_deg,
        drop: object.pose.drop,
    }
}

fn searched(inputs: &Inputs) -> (Found, Told) {
    let mut told = Told::default();
    let found = search(inputs, &mut told, &AtomicBool::new(false)).unwrap();
    (found, told)
}

#[test]
fn a_flat_box_is_proposed_as_it_lies_and_every_stage_is_told() {
    let temp = TempDirectory::new();
    // Within 30 degrees of lying flat, about X only: lying flat needs no support at all.
    let values = OrientValues {
        axes: Axes::X,
        ..DEFAULT_ORIENT
    };
    let inputs = flat_box(&temp, values, Vec::new());
    let (found, told) = searched(&inputs);
    assert!(found.scored > 0);
    assert!(!found.candidates.is_empty() && found.candidates.len() <= SHOWN);
    assert_eq!(found.anchor, inputs.anchor);
    let best = found.candidates[0];
    assert!(best.tilt.angle_deg() < 0.5, "{best:?}");
    assert_eq!((best.facing_mm2, best.islands, best.cups), (0.0, 0, None));
    for c in &found.candidates {
        assert!(c.tilt.angle_deg() <= 30.0 + 1e-9, "{c:?}");
        assert!(c.tilt.down[0].abs() < 1e-12, "about X only: {c:?}");
    }
    // The rough pass first, then each check in turn, its slice's progress between 0 and 1.
    let checked = told.stages.len() - 1;
    assert!((1..=CHECKED).contains(&checked), "{:?}", told.stages);
    assert_eq!(told.stages[0], Stage::Rough);
    for (k, stage) in told.stages[1..].iter().enumerate() {
        assert_eq!(*stage, Stage::Checking(k + 1, checked));
    }
    assert_ne!(told.shares, [] as [f32; 0]);
    assert!(told.shares.iter().all(|s| (0.0..=1.0).contains(s)));
    // Each check's slice is removed.
    assert!(!inputs.output.exists());
    // Equal inputs give equal candidates.
    assert_eq!(searched(&inputs).0, found);
}

#[test]
fn a_protected_point_refuses_turning_its_surface_to_the_plate() {
    let temp = TempDirectory::new();
    let values = OrientValues {
        axes: Axes::Y,
        limited: false,
        ..DEFAULT_ORIENT
    };
    let unprotected = flat_box(&temp, values, Vec::new());
    let (free, _) = searched(&unprotected);
    assert!(
        free.candidates.iter().any(|c| c.tilt.angle_deg() > 135.0),
        "{:?}",
        free.candidates
    );
    // The top's middle, protected: no tilt beyond 90 degrees plus the critical angle may turn it
    // down.
    let top = Protected {
        point: [unprotected.anchor[0], unprotected.anchor[1], 3.0],
        normal: [0.0, 0.0, 1.0],
    };
    let (found, _) = searched(&flat_box(&temp, values, vec![top]));
    assert_ne!(found.candidates, []);
    assert!(found
        .candidates
        .iter()
        .all(|c| c.tilt.angle_deg() <= 135.0 + 1e-9));
}

#[test]
fn a_stopped_search_proposes_nothing() {
    let temp = TempDirectory::new();
    let inputs = flat_box(&temp, DEFAULT_ORIENT, Vec::new());
    let mut told = Told::default();
    assert_eq!(
        search(&inputs, &mut told, &AtomicBool::new(true)),
        Err(SearchError::Stopped)
    );
    assert_eq!(told.stages, [Stage::Rough]);
    assert!(!inputs.output.exists());
}

#[test]
fn a_turn_keeps_its_pivot_and_moves_up_by_its_lift() {
    let tilt = Tilt::towards([0.0, 0.5, -0.5f64.sqrt() * 1.2], [1.0, 0.0, 0.0]).unwrap();
    let anchor = [10.0, 20.0, 5.0];
    let m = turn_rows(&tilt, anchor, 2.0);
    let at = |p: [f64; 3]| -> [f64; 3] {
        std::array::from_fn(|r| m[r][0] * p[0] + m[r][1] * p[1] + m[r][2] * p[2] + m[r][3])
    };
    let pivot = at(anchor);
    assert!((pivot[0] - 10.0).abs() < 1e-12 && (pivot[1] - 20.0).abs() < 1e-12);
    assert!((pivot[2] - 7.0).abs() < 1e-12);
    assert_eq!(m[3], [0.0, 0.0, 0.0, 1.0]);
    // The same map as a point turned on its own.
    let p = [13.0, 18.0, 9.0];
    let (a, b) = (at(p), turned(&tilt, anchor, 2.0, p));
    assert!((0..3).all(|k| (a[k] - b[k]).abs() < 1e-12));
}
