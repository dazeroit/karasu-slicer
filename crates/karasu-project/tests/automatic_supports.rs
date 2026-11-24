//! The Automatic tool's Generate and Apply without the application (TD-157): automatic supports
//! planned for the model of a slicing request, as the command line plans them.
mod common;

use common::{box_stl, printer, resin, TempDirectory};
use karasu_core::{
    cancellation::Checkpoint,
    profiles::store::ProfileDocument,
    supports::{
        AxisSpec, BaseSpec, ManualSupport, Preset, SupportDocument, SupportKind, SupportOrigin,
    },
};
use karasu_project::{
    pipeline::{slice, Observer, Silent},
    request::{
        Coordinates, Fill, Input, ModelSource, Pose, SliceRequest, Subject, DEFAULT_JOB_ID,
        DEFAULT_REPAIR,
    },
    support_plan::{
        auto::{Cover, NotCounted, Place},
        generate::{generate, given, Generated, Run},
        settings::AutoSettings,
    },
};
use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

/// Where the source model lands: moved 30 mm in X and 20 mm in Y, its lowest point lifted.
const AT: [f64; 2] = [30.0, 20.0];

/// An ASCII STL facet list for `triangles`, each counter-clockwise seen from outside.
fn facets(text: &mut String, triangles: &[[[f64; 3]; 3]]) {
    for triangle in triangles {
        text.push_str("facet normal 0 0 0\nouter loop\n");
        for [x, y, z] in triangle {
            writeln!(text, "vertex {x} {y} {z}").unwrap();
        }
        text.push_str("endloop\nendfacet\n");
    }
}

/// A pillar 6 mm square and 10 mm tall with a ledge 8 mm long at its top (its underside 7 mm
/// up: an overhang), as one closed L extruded 6 mm along Y; and apart from it, a box floating
/// from 4 to 6 mm (an island). Lifted, the pillar's bottom is an island too.
fn ledge_and_island_stl() -> String {
    // The L's profile in X and Z, counter-clockwise, and its two caps' triangles.
    let profile = [
        [0.0, 0.0],
        [6.0, 0.0],
        [6.0, 7.0],
        [14.0, 7.0],
        [14.0, 10.0],
        [6.0, 10.0],
        [0.0, 10.0],
    ];
    let cap = [[0, 1, 2], [0, 2, 5], [0, 5, 6], [2, 3, 4], [2, 4, 5]];
    let at = |k: usize, y: f64| [profile[k][0], y, profile[k][1]];
    let mut triangles = Vec::new();
    for [a, b, c] in cap {
        // Seen from -Y the profile turns counter-clockwise.
        triangles.push([at(a, 0.0), at(b, 0.0), at(c, 0.0)]);
        triangles.push([at(a, 6.0), at(c, 6.0), at(b, 6.0)]);
    }
    for k in 0..profile.len() {
        let next = (k + 1) % profile.len();
        let (p0, q0, p1, q1) = (at(k, 0.0), at(next, 0.0), at(k, 6.0), at(next, 6.0));
        triangles.push([p0, q1, q0]);
        triangles.push([p0, p1, q1]);
    }
    let mut text = String::from("solid model\n");
    facets(&mut text, &triangles);
    let island = box_stl([18.0, 0.0, 4.0], [22.0, 6.0, 6.0]);
    for line in island
        .lines()
        .filter(|l| !l.starts_with("solid") && !l.starts_with("endsolid"))
    {
        text.push_str(line);
        text.push('\n');
    }
    text.push_str("endsolid model\n");
    text
}

/// The printer, the resin and the model in a folder of their own, with a scratch folder.
struct Setup {
    temp: TempDirectory,
    printer: PathBuf,
    resin: PathBuf,
    model: PathBuf,
    scratch: PathBuf,
}

impl Setup {
    fn new() -> Self {
        let temp = TempDirectory::new();
        let scratch = temp.path("scratch");
        fs::create_dir(&scratch).unwrap();
        Self {
            printer: temp.write(
                "printer.json",
                ProfileDocument::Printer(printer()).to_json().unwrap(),
            ),
            resin: temp.write(
                "resin.json",
                ProfileDocument::Resin(resin()).to_json().unwrap(),
            ),
            model: temp.write("ledge.stl", ledge_and_island_stl()),
            scratch,
            temp,
        }
    }

    /// The model moved by [`AT`] and dropped, as the command line places it for its automatic
    /// supports.
    fn request(&self, output: &Path) -> SliceRequest {
        SliceRequest {
            subject: Subject::Model(ModelSource::File(self.model.clone())),
            units: Some(karasu_core::mesh::formats::LengthUnit::Millimeter),
            legacy_stl: false,
            printer: Input::File(self.printer.clone()),
            resin: Input::File(self.resin.clone()),
            format: "goo-v3".into(),
            output: output.to_path_buf(),
            translate_mm: [AT[0], AT[1], 0.0],
            pose: Pose {
                drop: true,
                ..Pose::default()
            },
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
            arrange_turn: false,
            arrange_footprint: karasu_core::plate::Footprint::Box,
            memory_budget_bytes: None,
            verify_walls: false,
            find_unsupported: None,
            find_suction_cups: None,
            slice_support_errors: false,
            allow_floating: false,
        }
    }

    /// Generate for `request` with `settings`, reporting to `observer`, stopped by `stop`.
    fn generate(
        &self,
        request: &SliceRequest,
        settings: &AutoSettings,
        observer: &mut dyn Observer,
        stop: &AtomicBool,
    ) -> Result<Generated, String> {
        generate(
            request,
            settings,
            Run {
                cache: None,
                scratch: &self.scratch,
                observer,
                stop,
            },
        )
    }

    /// Whether the scratch folder holds nothing: the slice of the model alone was removed.
    fn scratch_is_empty(&self) -> bool {
        fs::read_dir(&self.scratch).unwrap().next().is_none()
    }
}

/// Generate with `settings` and no stop, which must succeed.
fn planned(setup: &Setup, request: &SliceRequest, settings: &AutoSettings) -> Generated {
    let stop = AtomicBool::new(false);
    setup
        .generate(request, settings, &mut Silent, &stop)
        .unwrap()
}

#[test]
fn automatic_supports_hold_the_islands_and_the_overhang_and_slice() {
    let setup = Setup::new();
    let output = setup.temp.path("model.goo");
    let request = setup.request(&output);
    let settings = AutoSettings::default();
    let generated = planned(&setup, &request, &settings);
    // Lifted to the plan's lift, 5 mm, as Generate lifts it.
    assert!((generated.lift_mm - 5.0).abs() < 1e-12);
    assert_eq!(generated.request.pose.lift_mm, Some(5.0));
    let outcome = &generated.outcome;
    // Both islands (the pillar's lifted bottom and the floating box) are held, and the ledge's
    // underside is covered.
    assert_eq!(outcome.islands, [2, 2], "{:?}", outcome.left);
    assert!(outcome.overhangs[1] > 0);
    assert_eq!(
        outcome.overhangs[0], outcome.overhangs[1],
        "{:?}",
        outcome.left
    );
    assert!(outcome.left.is_empty(), "{:?}", outcome.left);
    // No island is left out of the counts, and the final check ran (TD-171).
    assert!(outcome.not_counted.is_empty(), "{:?}", outcome.not_counted);
    assert_eq!(outcome.plan.unchecked, None);
    // Every support is automatic, in both frames, and the model frame's is the source's.
    let document = generated.document.as_ref().expect("a document");
    let model = generated.model_document.as_ref().expect("a model document");
    assert_eq!(document.supports.len(), outcome.plan.supports.len());
    assert_eq!(model.supports.len(), document.supports.len());
    assert!(document
        .supports
        .iter()
        .all(|s| s.origin == SupportOrigin::Automatic));
    let under_ledge = document.supports.iter().any(|s| {
        let [x, _, z] = s.tip_mm;
        (AT[0] + 6.0..=AT[0] + 14.0).contains(&x) && (z - 12.0).abs() < 0.5
    });
    assert!(under_ledge, "{:?}", document.supports);
    for world in &document.supports {
        let source = model.supports.iter().find(|s| s.id == world.id).unwrap();
        let back = [
            source.tip_mm[0] + AT[0],
            source.tip_mm[1] + AT[1],
            source.tip_mm[2] + 5.0,
        ];
        assert!((0..3).all(|k| (world.tip_mm[k] - back[k]).abs() < 1e-9));
    }
    // The request slices: the supports hold the lifted model.
    assert!(matches!(
        &generated.request.supports,
        Some(Input::Loaded { value, .. }) if value == document
    ));
    let sliced = slice(&generated.request, &mut Silent, &|_: Checkpoint| false).unwrap();
    assert!(output.is_file());
    assert!(
        sliced
            .summary
            .contains(&format!("Supports: {} (plate", document.supports.len())),
        "{}",
        sliced.summary
    );
    // The slice of the model alone is gone.
    assert!(setup.scratch_is_empty());
}

#[test]
fn automatic_supports_are_the_same_every_time() {
    let setup = Setup::new();
    let request = setup.request(&setup.temp.path("model.goo"));
    let settings = AutoSettings::default();
    let first = planned(&setup, &request, &settings);
    let second = planned(&setup, &request, &settings);
    assert!(first.document.is_some());
    assert_eq!(first.document, second.document);
    assert_eq!(first.model_document, second.model_document);
    assert_eq!(first.request, second.request);
}

/// A support placed by hand under the pillar's lifted bottom, in world coordinates.
fn by_hand() -> SupportDocument {
    let mut document = SupportDocument::new(Preset::Medium);
    document.supports.push(ManualSupport {
        id: 7,
        tip_mm: [AT[0] + 3.0, AT[1] + 3.0, 5.0],
        tip_axis: AxisSpec::Normal,
        bends: Vec::new(),
        base: BaseSpec::Vertical,
        parameters: Preset::Medium.parameters(),
        accept_collision: false,
        origin: SupportOrigin::Placed,
        kind: SupportKind::Standard,
    });
    document
}

#[test]
fn given_supports_are_kept_as_given_and_the_automatic_ones_replaced() {
    let setup = Setup::new();
    let mut request = setup.request(&setup.temp.path("model.goo"));
    request.supports = Some(Input::Loaded {
        name: "by hand".into(),
        value: by_hand(),
    });
    let settings = AutoSettings::default();
    let generated = planned(&setup, &request, &settings);
    let document = generated.document.as_ref().unwrap();
    // The support by hand is there exactly as given, and holds the pillar's bottom: the plan
    // holds only the floating box.
    assert_eq!(generated.kept, 1);
    assert_eq!(
        document.supports.iter().find(|s| s.id == 7),
        Some(&by_hand().supports[0])
    );
    assert_eq!(generated.outcome.islands, [1, 1]);
    // The island the support by hand holds is counted apart, never silently left out (TD-171).
    assert_eq!(
        generated.outcome.not_counted,
        NotCounted {
            kept: 1,
            ..NotCounted::default()
        }
    );
    assert_eq!(
        document.supports.len(),
        1 + generated.outcome.plan.supports.len()
    );
    // Given again with the automatic ones, they are replaced, not added to: the same document.
    let mut again = request.clone();
    again.supports = Some(Input::Loaded {
        name: "written".into(),
        value: document.clone(),
    });
    let second = planned(&setup, &again, &settings);
    assert_eq!(second.replaced, generated.outcome.plan.supports.len());
    assert_eq!(second.kept, 1);
    assert_eq!(second.document.as_ref(), Some(document));
}

#[test]
fn the_values_choose_what_is_covered_and_how() {
    let setup = Setup::new();
    let request = setup.request(&setup.temp.path("model.goo"));
    // Islands only: the ledge's underside is left to the user.
    let mut settings = AutoSettings::default();
    settings.values.cover = Cover::IslandsOnly;
    let islands = planned(&setup, &request, &settings);
    assert_eq!(islands.outcome.overhangs, [0, 0]);
    assert_eq!(islands.outcome.islands, [2, 2]);
    let everything = planned(&setup, &request, &AutoSettings::default());
    assert!(
        islands.outcome.plan.supports.len() < everything.outcome.plan.supports.len(),
        "{} against {}",
        islands.outcome.plan.supports.len(),
        everything.outcome.plan.supports.len()
    );
    // Without minis no support is a mini one; without branches none joins another.
    let mut settings = AutoSettings::default();
    settings.values.minis = false;
    settings.values.branches = false;
    let plain = planned(&setup, &request, &settings);
    let document = plain.document.unwrap();
    assert!(document
        .supports
        .iter()
        .all(|s| s.kind == SupportKind::Standard && !matches!(s.base, BaseSpec::Support { .. })));
    // A model that is not hollowed has no cavity: planned everywhere whatever is asked.
    let mut settings = AutoSettings::default();
    settings.values.place = Place::Cavity;
    let cavity = planned(&setup, &request, &settings);
    assert_eq!(cavity.values.place, Place::Everywhere);
    assert_eq!(cavity.document, everything.document);
    // A denser plan takes more tips.
    let mut settings = AutoSettings::default();
    settings.values.density_pct = 200.0;
    let dense = planned(&setup, &request, &settings);
    assert!(dense.outcome.plan.supports.len() > everything.outcome.plan.supports.len());
}

/// Stops the run as soon as the model is sliced alone.
struct StopWhenSlicing<'a>(&'a AtomicBool);

impl Observer for StopWhenSlicing<'_> {
    fn phase(&mut self, _: &str) {}

    fn begin_layers(&mut self, _: usize) {
        self.0.store(true, Ordering::Relaxed);
    }

    fn layers(&mut self, _: usize) {}
}

#[test]
fn a_stopped_run_fails_and_leaves_no_slice_behind() {
    let setup = Setup::new();
    let request = setup.request(&setup.temp.path("model.goo"));
    let stop = AtomicBool::new(false);
    let mut observer = StopWhenSlicing(&stop);
    let error = setup
        .generate(&request, &AutoSettings::default(), &mut observer, &stop)
        .err()
        .expect("stopped");
    assert!(error.contains("cancelled"), "{error}");
    assert!(setup.scratch_is_empty());
    // A plate is refused before any work.
    let mut plate = request.clone();
    plate.subject = Subject::PlateFile(setup.temp.path("plate.json"));
    let stop = AtomicBool::new(false);
    let error = setup
        .generate(&plate, &AutoSettings::default(), &mut Silent, &stop)
        .err()
        .expect("refused");
    assert!(error.contains("one model"), "{error}");
}

#[test]
fn braces_are_planned_between_the_given_supports() {
    let setup = Setup::new();
    let mut request = setup.request(&setup.temp.path("model.goo"));
    // Two supports by hand under the ledge, 12 mm up and 4 mm apart.
    let mut document = by_hand();
    document.supports[0].tip_mm = [AT[0] + 8.0, AT[1] + 3.0, 12.0];
    let mut second = document.supports[0].clone();
    second.id = 8;
    second.tip_mm = [AT[0] + 12.0, AT[1] + 3.0, 12.0];
    document.supports.push(second);
    request.pose.lift_mm = Some(5.0);
    request.supports = Some(Input::Loaded {
        name: "by hand".into(),
        value: document.clone(),
    });
    let stop = AtomicBool::new(false);
    let run = |braces| {
        given(
            &request,
            braces,
            Run {
                cache: None,
                scratch: &setup.scratch,
                observer: &mut Silent,
                stop: &stop,
            },
        )
        .unwrap()
    };
    // Without braces: the document as the job builds it, the request as it was.
    let plain = run(None);
    assert_eq!(plain.document, document);
    assert_eq!(plain.request, request);
    let values = karasu_project::support_plan::braces::NewBraces {
        lowest_mm: 2.0,
        step_mm: 2.0,
        ..karasu_project::support_plan::braces::DEFAULT_BRACES
    };
    let braced = run(Some(&values));
    assert!(braced.braces > 0);
    assert_eq!(braced.document.braces.len(), braced.braces);
    assert_eq!(braced.document.supports, document.supports);
    assert!(matches!(
        &braced.request.supports,
        Some(Input::Loaded { value, .. }) if *value == braced.document
    ));
}
