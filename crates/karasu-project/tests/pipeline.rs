//! The shared pipeline as a library: outcome, progress, cancellation and request validation.
mod common;

use common::{box_stl, field, printer, resin, TempDirectory};
use karasu_core::{
    cancellation::Checkpoint, preparation::sampled::HoleSurface, profiles::store::ProfileDocument,
};
use karasu_project::{
    pipeline::{slice, Observer, Silent},
    request::{
        format_ids, machine_exporter, Coordinates, DrainHole, Fill, Input, ModelSource, Pose,
        SliceRequest, Subject, DEFAULT_JOB_ID, DEFAULT_REPAIR, REFERENCE_FORMAT,
    },
};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// A 100 x 60 mm, 1 mm pixel printer, a 0.25 mm resin, and a 20 x 10 x 2 mm box on the plate.
struct Setup {
    temp: TempDirectory,
    printer: PathBuf,
    resin: PathBuf,
    model: PathBuf,
}

impl Setup {
    fn new() -> Self {
        let temp = TempDirectory::new();
        Self {
            printer: temp.write(
                "printer.json",
                ProfileDocument::Printer(printer()).to_json().unwrap(),
            ),
            resin: temp.write(
                "resin.json",
                ProfileDocument::Resin(resin()).to_json().unwrap(),
            ),
            model: temp.write("box.stl", box_stl([10., 20., 0.], [30., 30., 2.])),
            temp,
        }
    }

    fn request(&self, format: &str, output: &Path) -> SliceRequest {
        SliceRequest {
            subject: Subject::Model(ModelSource::File(self.model.clone())),
            units: Some(karasu_core::mesh::formats::LengthUnit::Millimeter),
            legacy_stl: false,
            printer: Input::File(self.printer.clone()),
            resin: Input::File(self.resin.clone()),
            format: format.into(),
            output: output.to_path_buf(),
            translate_mm: [0.; 3],
            pose: Pose::default(),
            job_id: DEFAULT_JOB_ID.into(),
            allow_topology_defects: false,
            fill: Fill::Union,
            antialias: 1,
            min_gray: 0,
            antialias_supports: false,
            xy_compensation_px: 0,
            bottom_xy_compensation_px: None,
            close_holes_mm: Some(2.),
            reversed_tolerance_mm2: None,
            repair: DEFAULT_REPAIR,
            hollow_mm: None,
            hollow_cell_mm: None,
            drain_holes: Vec::new(),
            hole_coordinates: Coordinates::World,
            support_coordinates: Coordinates::World,
            hollow_simplify: None,
            hollow_blockers: Vec::new(),
            supports: None,
            accept_support_collisions: false,
            layer_plan: None,
            copies: 1,
            arrange: false,
            spacing_mm: 2.,
            memory_budget_bytes: None,
            verify_walls: false,
            find_unsupported: None,
            find_suction_cups: None,
            slice_support_errors: false,
            allow_floating: false,
        }
    }
}

/// Records what the pipeline reports.
#[derive(Default)]
struct Recorder {
    phases: Vec<String>,
    total: Option<usize>,
    completed: Vec<usize>,
}

impl Observer for Recorder {
    fn phase(&mut self, name: &str) {
        self.phases.push(name.to_owned());
    }

    fn begin_layers(&mut self, total: usize) {
        self.total = Some(total);
    }

    fn layers(&mut self, completed: usize) {
        self.completed.push(completed);
    }
}

#[test]
fn a_library_job_publishes_its_output_and_returns_the_summary() {
    let setup = Setup::new();
    let output = setup.temp.path("box.goo");
    let mut recorder = Recorder::default();
    let outcome = slice(&setup.request("goo-v3", &output), &mut recorder, &|_| false).unwrap();
    assert!(output.is_file());
    assert_eq!(
        fs::metadata(&output).unwrap().len(),
        outcome.export.output_bytes
    );
    assert!(outcome.summary.ends_with('\n'));
    assert_eq!(field(&outcome.summary, "Layers"), "8");
    assert_eq!(field(&outcome.summary, "Format"), "goo-v3");
    // 20 x 10 mm over 2 mm of 1 mm pixels: exactly 400 mm^3.
    assert_eq!(field(&outcome.summary, "Sampled mask volume (mm^3)"), "400");
    assert!(
        (outcome.resin.volume_ml - 0.4).abs() < 1e-12,
        "{:?}",
        outcome.resin
    );
    // Phases in pipeline order; every layer reported once, in order.
    assert_eq!(
        recorder.phases,
        [
            "Reading profiles",
            "Importing model",
            "Checking topology",
            "Rendering previews",
            "Planning layers"
        ]
    );
    assert_eq!(recorder.total, Some(8));
    assert_eq!(recorder.completed, (1..=8).collect::<Vec<_>>());
    // The same request is refused once its output exists: outputs are create-only.
    let error = slice(&setup.request("goo-v3", &output), &mut Silent, &|_| false).unwrap_err();
    assert!(error.message().starts_with("cannot export"), "{error}");
}

#[test]
fn a_cancelled_job_publishes_nothing() {
    let setup = Setup::new();
    for checkpoint in [None, Some(Checkpoint::RasterStart)] {
        let output = setup.temp.path("cancelled.goo");
        // Cancel at the first checkpoint, or once layer generation starts.
        let stop = |at: Checkpoint| checkpoint.is_none_or(|c| c == at);
        let result = slice(&setup.request("goo-v3", &output), &mut Silent, &stop);
        assert!(result.is_err(), "{checkpoint:?}");
        assert!(!output.exists(), "{checkpoint:?}");
        let staging: Vec<_> = fs::read_dir(&setup.temp.0)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .filter(|name| name.to_string_lossy().contains("cancelled"))
            .collect();
        assert!(staging.is_empty(), "{staging:?}");
    }
}

#[test]
fn request_validation_refuses_conflicting_options() {
    let setup = Setup::new();
    let base = setup.request("goo-v3", &setup.temp.path("unused.goo"));
    assert_eq!(base.clone().checked(), Ok(base.clone()));
    let refused = |request: SliceRequest| request.checked().unwrap_err();
    assert!(refused(SliceRequest {
        drain_holes: vec![DrainHole::Surface {
            surface_mm: [20., 25., 1.],
            diameter_mm: 3.,
            outside_mm: 1.,
            inside_mm: 1.,
            tilt: None,
            depth_mm: None,
            surface: HoleSurface::Outer,
        }],
        ..base.clone()
    })
    .contains("requires --hollow"));
    assert!(refused(SliceRequest {
        min_gray: 10,
        antialias_supports: false,
        ..base.clone()
    })
    .contains("--antialias"));
    assert!(refused(SliceRequest {
        format: "unknown".into(),
        ..base.clone()
    })
    .contains("supported:"));
    assert!(refused(SliceRequest {
        format: "photon-v1-firmware-motion".into(),
        antialias: 4,
        ..base
    })
    .contains("binary masks"));
    // Every listed identifier selects an exporter; the reference format writes images instead.
    for id in format_ids().split(", ") {
        let exporter = machine_exporter(id).unwrap();
        assert_eq!(exporter.is_none(), id == REFERENCE_FORMAT, "{id}");
    }
}

#[test]
fn refusals_name_their_remedies_apart_from_the_command_line_hints() {
    use karasu_project::pipeline::Remedy;
    let setup = Setup::new();
    // An inside-out box: every facet listed clockwise seen from outside.
    let mut inside_out = String::new();
    let mut vertices = Vec::new();
    for line in box_stl([10., 20., 0.], [30., 30., 2.]).lines() {
        if line.starts_with("vertex") {
            vertices.push(line.to_owned());
            if vertices.len() == 3 {
                for vertex in vertices.drain(..).rev() {
                    inside_out.push_str(&vertex);
                    inside_out.push('\n');
                }
            }
        } else {
            inside_out.push_str(line);
            inside_out.push('\n');
        }
    }
    let model = setup.temp.write("inside-out.stl", inside_out);
    let request = |output: &str| SliceRequest {
        subject: Subject::Model(ModelSource::File(model.clone())),
        ..setup.request("goo-v3", &setup.temp.path(output))
    };
    let error = slice(&request("a.goo"), &mut Silent, &|_| false).unwrap_err();
    assert_eq!(
        error.remedies(),
        [Remedy::RepairOutward, Remedy::TolerateReversed]
    );
    assert!(error.message().contains("--repair outward"), "{error}");
    assert!(!error.reason().contains("--"), "{}", error.reason());
    assert!(error.message().starts_with(error.reason()));
    // The remedy works.
    let mut repaired = request("b.goo");
    repaired.repair.outward = true;
    slice(&repaired, &mut Silent, &|_| false).unwrap();
    // A model above the plate, and one beside it.
    let lifted = SliceRequest {
        translate_mm: [0., 0., 1.],
        ..setup.request("goo-v3", &setup.temp.path("c.goo"))
    };
    let error = slice(&lifted, &mut Silent, &|_| false).unwrap_err();
    assert_eq!(error.remedies(), [Remedy::RestOnPlate]);
    assert!(error.message().contains("--drop"), "{error}");
    assert!(!error.reason().contains("--"), "{}", error.reason());
    let beside = SliceRequest {
        translate_mm: [80., 0., 0.],
        ..setup.request("goo-v3", &setup.temp.path("d.goo"))
    };
    let error = slice(&beside, &mut Silent, &|_| false).unwrap_err();
    assert_eq!(error.remedies(), [Remedy::MoveOntoPlate]);
    assert!(!error.reason().contains("--"), "{}", error.reason());
    // Without a hint the reason is the whole message.
    let error = slice(&lifted, &mut Silent, &|_| true).unwrap_err();
    assert_eq!(error.remedies(), []);
    assert_eq!(error.reason(), error.message());
}

#[test]
fn facets_without_a_position_are_refused_unless_the_repairs_accept_them() {
    use karasu_project::pipeline::Remedy;
    let setup = Setup::new();
    // The box and one facet with a NaN coordinate after its 12 facets.
    let text = box_stl([10., 20., 0.], [30., 30., 2.]).replace(
        "endsolid box",
        "facet normal 0 0 0\nouter loop\nvertex nan 0 0\nvertex 1 0 0\nvertex 0 1 0\n\
         endloop\nendfacet\nendsolid box",
    );
    let model = setup.temp.write("nan.stl", text);
    let request = |output: &str| SliceRequest {
        subject: Subject::Model(ModelSource::File(model.clone())),
        ..setup.request("goo-v3", &setup.temp.path(output))
    };
    // Refused with the import's own words, then the remedy.
    let error = slice(&request("a.goo"), &mut Silent, &|_| false).unwrap_err();
    assert_eq!(
        error.reason(),
        format!(
            "cannot import STL {}: STL facet 12, corner 0, axis 0: coordinate must be finite",
            ModelSource::File(model.clone()).label()
        )
    );
    assert_eq!(error.remedies(), [Remedy::DropNonFinite]);
    assert!(error.message().contains("--repair non-finite"), "{error}");
    assert!(!setup.temp.path("a.goo").exists());
    // Accepted: the box slices, and the summary names what was left out.
    let mut accepted = request("b.goo");
    accepted.repair.non_finite = true;
    let summary = slice(&accepted, &mut Silent, &|_| false).unwrap();
    assert_eq!(
        field(&summary.summary, "Import notes"),
        "1 facet(s) with non-finite coordinates dropped, in 1 run(s) (facet 12); their \
         positions are unknown"
    );
    let plain = slice(&request("c.goo"), &mut Silent, &|_| false);
    assert!(plain.is_err());
    // The same output as the box alone.
    let reference = slice(
        &setup.request("goo-v3", &setup.temp.path("d.goo")),
        &mut Silent,
        &|_| false,
    )
    .unwrap();
    assert_eq!(
        field(&summary.summary, "Resin volume (ml)"),
        field(&reference.summary, "Resin volume (ml)")
    );
    assert_eq!(
        fs::read(setup.temp.path("b.goo")).unwrap(),
        fs::read(setup.temp.path("d.goo")).unwrap()
    );
}

#[test]
fn a_reversed_sliver_within_the_tolerance_is_left_empty_and_reported() {
    use karasu_project::pipeline::Remedy;
    let setup = Setup::new();
    // The outward box and, beside it, a 2 x 2 mm inside-out box: on the 1 mm pixels of the test
    // printer its section holds 4 negative samples in each layer.
    let mut model = box_stl([10., 20., 0.], [30., 30., 2.]).replace("endsolid box\n", "");
    let mut vertices = Vec::new();
    for line in box_stl([40., 20., 0.], [42., 22., 2.]).lines() {
        if line.starts_with("vertex") {
            vertices.push(line.to_owned());
            if vertices.len() == 3 {
                model.push_str("facet normal 0 0 0\nouter loop\n");
                for vertex in vertices.drain(..).rev() {
                    model.push_str(&vertex);
                    model.push('\n');
                }
                model.push_str("endloop\nendfacet\n");
            }
        }
    }
    model.push_str("endsolid box\n");
    let path = setup.temp.write("sliver.stl", model);
    let request = |output: &str, tolerance: Option<f64>| SliceRequest {
        subject: Subject::Model(ModelSource::File(path.clone())),
        reversed_tolerance_mm2: tolerance,
        ..setup.request("goo-v3", &setup.temp.path(output))
    };
    // Refused without a tolerance, offering both remedies.
    let error = slice(&request("a.goo", None), &mut Silent, &|_| false).unwrap_err();
    assert_eq!(
        error.remedies(),
        [Remedy::RepairOutward, Remedy::TolerateReversed]
    );
    assert!(error.reason().contains("at layer 0, pixel ("), "{error}");
    assert!(
        error.message().contains("--reversed-tolerance <mm2>"),
        "{error}"
    );
    // A tolerance below the sliver still fails, now naming the count and the bound.
    let error = slice(&request("b.goo", Some(3.5)), &mut Silent, &|_| false).unwrap_err();
    assert!(
        error
            .reason()
            .contains("over 4 samples, more than the 3 tolerated"),
        "{error}"
    );
    assert_eq!(error.remedies(), [Remedy::RepairOutward]);
    // Within the tolerance it slices like the outward box alone, and the summary says so.
    let tolerated = slice(&request("c.goo", Some(4.)), &mut Silent, &|_| false).unwrap();
    assert_eq!(
        field(&tolerated.summary, "Reversed slivers left empty"),
        "8 layers, 32 mm^2 in all, largest 4 mm^2 at layer 0 (bound 4 mm^2 per layer)"
    );
    let plain = slice(
        &setup.request("goo-v3", &setup.temp.path("d.goo")),
        &mut Silent,
        &|_| false,
    )
    .unwrap();
    assert_eq!(tolerated.export.layers, plain.export.layers);
    assert_eq!(
        tolerated.export.coverage_volume_mm3,
        plain.export.coverage_volume_mm3
    );
    // Without negative winding the line reports none; without a tolerance there is no line.
    let clean = slice(
        &SliceRequest {
            reversed_tolerance_mm2: Some(1.),
            ..setup.request("goo-v3", &setup.temp.path("e.goo"))
        },
        &mut Silent,
        &|_| false,
    )
    .unwrap();
    assert_eq!(
        field(&clean.summary, "Reversed slivers left empty"),
        "none (bound 1 mm^2 per layer)"
    );
    assert!(!plain.summary.contains("Reversed slivers"));
}
