//! The mandatory preparation scenarios (ROADMAP M7, run for M6.4h), driven through the
//! interface's own state: the tools' clicks, the panels' edits, the Slice panel's job and its
//! export.
//!
//! Each scenario starts from an STL file written to its own directory and checks that the file
//! and the imported mesh are never changed. Slices are exported in the printer's own format, so
//! the bytes compared are the file a user would print. Setting `KARASU_SCENARIOS_OUT` to a
//! directory keeps each scenario's export there with its SHA-256, for comparing platforms and
//! for the independent readers in `tools/m5-validate`.
//!
//! Project files do not exist yet (M6.6): reopening saved preparation is not applicable, and
//! replay is checked in memory, through undo and redo.
use crate::{
    camera::Ray,
    hollowing_ui::{tests::prepared_within, HollowingUi},
    messages::{Level, Message},
    preparation_ui::PreparationUi,
    slicing_ui::{
        tests::{finish, slicing_into, summary},
        SlicingUi,
    },
    supports_ui::{tests::built, SupportsUi},
    workspace::{
        tests::{box_stl, catalog_profiles, wait},
        Workspace,
    },
};
use karasu_core::{
    digest::{sha256, to_hex},
    mesh::TriangleMesh,
    supports::{BaseSpec, Preset},
};
use karasu_project::project::Hollowing;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

/// The printer every scenario slices for, in its own format.
const PRINTER: &str = "anycubic-photon-mono-x";

/// How long a preparation may take in a debug build.
const PREPARED: Duration = Duration::from_secs(120);

/// A 2 mm wall, its cell chosen by the application.
const WALL: Hollowing = Hollowing {
    wall_mm: 2.0,
    cell_mm: None,
    simplify: None,
};

/// One scenario's plate, as the main window holds it.
struct Session {
    workspace: Workspace,
    preparation: PreparationUi,
    slicing: SlicingUi,
    directory: PathBuf,
    source: PathBuf,
    source_bytes: Vec<u8>,
    imported: Arc<TriangleMesh>,
}

impl Session {
    /// `stl` opened on the scenario printer, in its format, lifted `lift_mm` (on the plate when
    /// 0), prepared. With `repair` off, nothing is repaired on import.
    fn open(name: &str, stl: &str, lift_mm: f64, repair: bool) -> Self {
        let directory =
            std::env::temp_dir().join(format!("karasu-scenario-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let source = directory.join(format!("{name}.stl"));
        std::fs::write(&source, stl).unwrap();
        let mut workspace = Workspace::new();
        if !repair {
            workspace.set_auto_repair(None);
        }
        workspace.open(source.clone());
        wait(&mut workspace);
        let (printer, resin, setting) = catalog_profiles(PRINTER);
        workspace.edit(|plate| {
            plate.printer = Some(printer);
            plate.resin = Some(resin);
            plate.print_setting = Some(setting);
            plate.objects[0].pose.drop = true;
            plate.objects[0].pose.lift_mm = (lift_mm > 0.0).then_some(lift_mm);
        });
        // The printer's own format, as the Slice panel chooses it.
        let format = crate::slicing_ui::plate_format(&workspace).expect("a format");
        workspace.edit(|plate| plate.settings.format = format);
        // Every edit so far set the plate up: the scenario's own steps are what undo takes back.
        workspace.forget_history();
        let imported = Arc::clone(&workspace.meshes().values().next().expect("imported").mesh);
        let mut preparation = PreparationUi::default();
        // As while drain holes are placed: the object is probed.
        preparation.set_probes(vec![0]);
        prepared_within(&workspace, &mut preparation, PREPARED);
        Self {
            workspace,
            preparation,
            slicing: slicing_into(&directory),
            source_bytes: std::fs::read(&source).unwrap(),
            directory,
            source,
            imported,
        }
    }

    /// Prepare the plate as it is now.
    fn prepare(&mut self) {
        prepared_within(&self.workspace, &mut self.preparation, PREPARED);
    }

    /// The selected model's bounds on the plate.
    fn bounds(&mut self) -> ([f64; 3], [f64; 3]) {
        let placed = self.workspace.placement_of(0).expect("placed");
        (placed.min.coordinates(), placed.max.coordinates())
    }

    /// Slice from the Slice panel and wait. Returns its messages.
    fn slice(&mut self) -> Vec<Message> {
        self.slicing.start(&self.workspace);
        finish(&mut self.slicing);
        self.slicing.take_messages()
    }

    /// Slice, check the slice is of the plate as it is, and export it as `name`. Returns the
    /// exported bytes and the slice's messages.
    fn slice_and_export(&mut self, name: &str) -> (Vec<u8>, Vec<Message>) {
        let messages = self.slice();
        assert!(
            self.slicing.current(&self.workspace),
            "no current slice: {messages:#?}"
        );
        let extension = crate::slicing_ui::extension(&self.workspace.plate().settings.format)
            .expect("a print file");
        let destination = self.directory.join(format!("{name}.{extension}"));
        assert!(self.slicing.export_to(&destination));
        let exported = self.slicing.take_messages();
        assert!(
            exported.iter().any(|m| m.level == Level::Success),
            "{exported:#?}"
        );
        let bytes = std::fs::read(&destination).unwrap();
        keep(name, &extension, &bytes);
        (bytes, messages)
    }

    /// The model file and its imported mesh are as they were opened.
    fn source_untouched(&self) {
        assert_eq!(std::fs::read(&self.source).unwrap(), self.source_bytes);
        let now = &self
            .workspace
            .meshes()
            .values()
            .next()
            .expect("imported")
            .mesh;
        assert_eq!(now.vertices(), self.imported.vertices());
        assert_eq!(now.triangles(), self.imported.triangles());
    }

    /// Hollow the model from the Hollow panel, as its Apply does.
    fn hollow(&mut self, hollowing: Hollowing) {
        self.workspace.set_hollowing(0, Some(hollowing));
        self.prepare();
    }

    /// Place a drain hole with the Drain holes tool on the -X face at (`y`, `z`) from the
    /// model's corner, `diameter_mm` wide or as the tool's default, and apply it. Returns
    /// whether a hole was cut.
    fn drain_hole(&mut self, y: f64, z: f64, diameter_mm: Option<f64>) -> bool {
        let (min, _) = self.bounds();
        let before = self.workspace.holes(0).len();
        let mut ui = HollowingUi::default();
        if let Some(diameter) = diameter_mm {
            *ui.next_hole().0 = diameter;
        }
        ui.toggle_placing();
        let ray = Ray {
            origin: [min[0] - 50.0, min[1] + y, min[2] + z],
            direction: [1.0, 0.0, 0.0],
        };
        let hit = self.workspace.hit(&ray);
        ui.hover(
            &self.workspace,
            &mut self.preparation,
            0,
            Some(&ray),
            hit,
            None,
        );
        ui.click(&mut self.workspace, &mut self.preparation, &ray, 0);
        ui.apply_pending(&mut self.workspace, 0);
        self.prepare();
        self.workspace.holes(0).len() > before
    }

    /// Place supports with the Supports tool under the model's bottom, at `xs` from its middle
    /// along X, "Avoid the model" off. Returns the tool, its builds current.
    fn place_supports(&mut self, xs: &[f64]) -> SupportsUi {
        let (min, max) = self.bounds();
        let [cx, cy] = [0, 1].map(|k| f64::midpoint(min[k], max[k]));
        let mut ui = SupportsUi::new();
        ui.straight_only = true;
        ui.toggle_placing();
        for &dx in xs {
            let up = Ray {
                origin: [cx + dx, cy, -50.0],
                direction: [0.0, 0.0, 1.0],
            };
            let hit = self.workspace.hit(&up);
            assert!(ui.click(&mut self.workspace, &up, hit, 0));
        }
        ui.toggle_placing();
        self.prepare();
        built(&mut ui, &self.workspace, &self.preparation);
        ui
    }

    /// Select the support standing at `dx` from the model's middle by clicking its shaft from
    /// the front, as in the viewport.
    fn select_support(&mut self, ui: &mut SupportsUi, dx: f64) -> u64 {
        let (min, max) = self.bounds();
        let cx = f64::midpoint(min[0], max[0]);
        let shaft = Ray {
            origin: [cx + dx, min[1] - 50.0, min[2] / 2.0],
            direction: [0.0, 1.0, 0.0],
        };
        assert!(ui.click(&mut self.workspace, &shaft, None, 0));
        assert!(ui.support_selected(Some(0)));
        self.support_ids()
            .into_iter()
            .find(|&id| (self.tip_x(id) - (cx + dx)).abs() < 1e-6)
            .expect("a support there")
    }

    /// The supports' identities, in order.
    fn support_ids(&self) -> Vec<u64> {
        self.workspace
            .supports(0)
            .map_or_default(|d| d.supports.iter().map(|s| s.id).collect())
    }

    /// Support `id`'s tip X on the plate.
    fn tip_x(&self, id: u64) -> f64 {
        let to_world = self.workspace.model_to_world(0).unwrap();
        let supports = &self.workspace.supports(0).unwrap().supports;
        let tip = supports.iter().find(|s| s.id == id).unwrap().tip_mm;
        to_world.point(tip)[0]
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

/// Keep `bytes` in `KARASU_SCENARIOS_OUT`, when set, as `name` with its SHA-256 beside it.
fn keep(name: &str, extension: &str, bytes: &[u8]) {
    let Some(out) = std::env::var_os("KARASU_SCENARIOS_OUT") else {
        return;
    };
    let out = Path::new(&out);
    std::fs::create_dir_all(out).unwrap();
    std::fs::write(out.join(format!("{name}.{extension}")), bytes).unwrap();
    std::fs::write(
        out.join(format!("{name}.sha256")),
        format!("{}  {name}.{extension}\n", to_hex(&sha256(bytes))),
    )
    .unwrap();
}

/// A 20 mm cube with its lower corner at the origin.
fn cube() -> String {
    box_stl([0.0; 3], [20.0; 3])
}

/// Whether `messages` hold one at `level` saying `text`.
fn says(messages: &[Message], level: Level, text: &str) -> bool {
    messages.iter().any(|m| m.level == level && m.text == text)
}

/// The guaranteed hollow wall of the last slice, in mm.
fn guaranteed_wall(slicing: &SlicingUi) -> f64 {
    line(slicing, "Hollow wall guaranteed (mm): ")
        .trim_start_matches("Hollow wall guaranteed (mm): ")
        .parse()
        .unwrap()
}

/// The summary line starting with `key`.
fn line<'a>(slicing: &'a SlicingUi, key: &str) -> &'a str {
    let summary = summary(slicing).expect("a slice");
    summary
        .lines()
        .find(|l| l.starts_with(key))
        .unwrap_or_else(|| panic!("no {key:?} in:\n{summary}"))
}

#[test]
fn a_solid_model_is_hollowed_drained_sliced_and_exported() {
    let mut s = Session::open("hollowed", &cube(), 0.0, true);
    s.hollow(WALL);
    assert!(s.drain_hole(10.0, 10.0, None));
    let (_, messages) = s.slice_and_export("hollowed");
    assert!(
        messages.iter().any(|m| m.level == Level::Success),
        "{messages:#?}"
    );
    assert!(guaranteed_wall(&s.slicing) >= 2.0);
    assert!(line(&s.slicing, "Drain hole 1: ").contains("opens to the outside"));
    assert!(line(&s.slicing, "Drainage: ").contains("every cavity region is opened"));
    assert!(
        !messages.iter().any(|m| m.level >= Level::Warning),
        "{messages:#?}"
    );
    s.source_untouched();
}

#[test]
fn supports_are_added_edited_and_removed_then_sliced_and_exported() {
    let mut s = Session::open("supported", &cube(), 5.0, true);
    let mut ui = s.place_supports(&[-6.0, 0.0, 6.0]);
    assert_eq!(s.support_ids().len(), 3);
    // Edited: the middle one made Heavy in the panel.
    let middle = s.select_support(&mut ui, 0.0);
    let mut heavy = s
        .workspace
        .supports(0)
        .unwrap()
        .supports
        .iter()
        .find(|x| x.id == middle)
        .unwrap()
        .clone();
    heavy.parameters = Preset::Heavy.parameters();
    SupportsUi::replace(&mut s.workspace, 0, heavy.clone());
    s.prepare();
    built(&mut ui, &s.workspace, &s.preparation);
    // Removed: the first one, selected and deleted.
    let first = s.select_support(&mut ui, -6.0);
    assert!(ui.delete(&mut s.workspace));
    let left = s.workspace.supports(0).unwrap().supports.clone();
    assert_eq!(left.len(), 2);
    assert!(!left.iter().any(|x| x.id == first));
    assert_eq!(left.iter().find(|x| x.id == middle), Some(&heavy));
    assert!(left.iter().all(|x| matches!(x.base, BaseSpec::Vertical)));
    let (_, messages) = s.slice_and_export("supported");
    assert!(
        line(&s.slicing, "Supports: ").starts_with("Supports: 2 (plate 2,"),
        "{}",
        summary(&s.slicing).unwrap()
    );
    assert!(
        !messages.iter().any(|m| m.level >= Level::Warning),
        "{messages:#?}"
    );
    s.source_untouched();
}

/// The cube lifted 5 mm, hollowed, drained on its side and held by three supports.
fn combined(name: &str) -> (Session, SupportsUi) {
    let mut s = Session::open(name, &cube(), 5.0, true);
    s.hollow(WALL);
    assert!(s.drain_hole(10.0, 10.0, None));
    let ui = s.place_supports(&[-6.0, 0.0, 6.0]);
    (s, ui)
}

#[test]
fn a_hollowed_drained_and_supported_model_is_sliced_and_exported() {
    let (mut s, _ui) = combined("combined");
    let (_, messages) = s.slice_and_export("combined");
    assert!(line(&s.slicing, "Drainage: ").contains("every cavity region is opened"));
    assert!(line(&s.slicing, "Supports: ").starts_with("Supports: 3 (plate 3,"));
    assert!(
        !messages.iter().any(|m| m.level >= Level::Warning),
        "{messages:#?}"
    );
    s.source_untouched();
}

#[test]
fn preparation_edited_is_recomputed_and_the_source_never_changes() {
    let (mut s, _ui) = combined("edited");
    let (first, _) = s.slice_and_export("edited-2mm");
    // A thinner wall: the slice shown is out of date until sliced again, and differs.
    s.hollow(Hollowing {
        wall_mm: 1.5,
        ..WALL
    });
    assert!(!s.slicing.current(&s.workspace));
    let (thinner, _) = s.slice_and_export("edited-1.5mm");
    assert_ne!(thinner, first);
    let thin = guaranteed_wall(&s.slicing);
    assert!((1.5..2.0).contains(&thin), "{thin}");
    s.source_untouched();
    // Back to 2 mm: the same file as first.
    s.hollow(WALL);
    let (again, _) = s.slice_and_export("edited-2mm-again");
    assert_eq!(again, first);
    s.source_untouched();
}

#[test]
fn undoing_and_redoing_the_preparation_replays_it_exactly() {
    let mut s = Session::open("replayed", &cube(), 0.0, true);
    let (plain, _) = s.slice_and_export("replayed-plain");
    // Lifted for supports, as the Raft tool's lift does.
    s.workspace
        .edit(|plate| plate.objects[0].pose.lift_mm = Some(5.0));
    s.prepare();
    s.hollow(WALL);
    assert!(s.drain_hole(10.0, 10.0, None));
    s.place_supports(&[-6.0, 0.0, 6.0]);
    let (prepared, _) = s.slice_and_export("replayed-prepared");
    assert_ne!(prepared, plain);
    let mut steps = 0;
    while s.workspace.undo() {
        steps += 1;
    }
    // The lift, hollowing, the hole, and each of the three supports.
    assert_eq!(steps, 6);
    s.prepare();
    let (undone, _) = s.slice_and_export("replayed-undone");
    assert_eq!(undone, plain);
    for _ in 0..steps {
        assert!(s.workspace.redo());
    }
    s.prepare();
    let (redone, _) = s.slice_and_export("replayed-redone");
    assert_eq!(redone, prepared);
    s.source_untouched();
}

#[test]
fn a_slice_finished_after_an_edit_is_never_taken_for_the_plate() {
    let (mut s, _ui) = combined("stale");
    s.slicing.start(&s.workspace);
    // Edited while it runs.
    s.workspace.set_hollowing(
        0,
        Some(Hollowing {
            wall_mm: 1.5,
            ..WALL
        }),
    );
    finish(&mut s.slicing);
    s.slicing.take_messages();
    assert!(!s.slicing.current(&s.workspace));
    s.prepare();
    s.slice_and_export("stale-after");
    let thin = guaranteed_wall(&s.slicing);
    assert!((1.5..2.0).contains(&thin), "{thin}");
}

#[test]
fn the_same_preparation_gives_the_same_file_every_time() {
    let (mut a, _ui) = combined("reproduced-a");
    let (first, _) = a.slice_and_export("reproduced");
    let (mut b, _ui) = combined("reproduced-b");
    let (second, _) = b.slice_and_export("reproduced-b");
    assert_eq!(first, second);
}

#[test]
fn an_open_model_is_refused_for_hollowing_with_its_fixes() {
    // The cube without its last facet, imported without repairs.
    let mut stl = cube();
    let last = stl.rfind("facet normal").unwrap();
    stl.replace_range(last..stl.rfind("endsolid").unwrap(), "");
    let mut s = Session::open("open", &stl, 0.0, false);
    s.workspace.set_hollowing(0, Some(WALL));
    let messages = s.slice();
    assert!(!s.slicing.current(&s.workspace));
    let refused = messages
        .iter()
        .find(|m| m.level == Level::Error)
        .unwrap_or_else(|| panic!("{messages:#?}"));
    assert!(!refused.fixes.is_empty(), "{refused:#?}");
    assert!(!s.slicing.export_to(&s.directory.join("open.pwmx")));
    s.source_untouched();
}

#[test]
fn an_enclosed_cavity_slices_with_a_warning_never_a_plain_success() {
    let mut s = Session::open("enclosed", &cube(), 0.0, true);
    s.hollow(WALL);
    let messages = s.slice();
    assert!(s.slicing.current(&s.workspace));
    assert!(
        says(
            &messages,
            Level::Warning,
            &crate::i18n::tr!("slice-cavity-enclosed")
        ),
        "{messages:#?}"
    );
    assert!(line(&s.slicing, "Drainage: ").contains("trap uncured resin"));
}

#[test]
fn a_drain_hole_that_misses_the_cavity_leaves_it_enclosed_and_says_so() {
    let mut s = Session::open("unreached", &cube(), 0.0, true);
    s.hollow(WALL);
    // 0.8 mm from the edge, a 1 mm hole stays in the 2 mm wall: it is placed, and told.
    assert!(s.drain_hole(0.8, 10.0, Some(1.0)));
    let messages = s.slice();
    assert!(line(&s.slicing, "Drain hole 1: ").contains("reaches no cavity"));
    assert!(line(&s.slicing, "Drainage: ").contains("may trap uncured resin"));
    assert!(
        says(
            &messages,
            Level::Warning,
            &crate::i18n::tr!("slice-cavity-enclosed")
        ),
        "{messages:#?}"
    );
}

#[test]
fn a_support_without_room_prints_with_a_warning_naming_it() {
    let mut s = Session::open("short", &cube(), 5.0, true);
    let mut ui = s.place_supports(&[-6.0, 6.0]);
    // One support's tip made longer than the lift leaves room for, in the panel.
    let id = s.select_support(&mut ui, 6.0);
    let mut long = s
        .workspace
        .supports(0)
        .unwrap()
        .supports
        .iter()
        .find(|x| x.id == id)
        .unwrap()
        .clone();
    long.parameters.tip_length_mm = 4.5;
    SupportsUi::replace(&mut s.workspace, 0, long);
    s.prepare();
    let messages = s.slice();
    assert!(s.slicing.current(&s.workspace));
    let warned = messages
        .iter()
        .find(|m| m.level == Level::Warning && m.text == crate::i18n::tr!("slice-support-errors"))
        .unwrap_or_else(|| panic!("{messages:#?}"));
    let detail = warned.detail.as_deref().unwrap();
    assert!(detail.contains(&format!("support {id},")), "{detail}");
}

#[test]
fn supports_that_no_longer_reach_the_plate_refuse_the_slice_and_say_why() {
    let mut s = Session::open("lowered", &cube(), 5.0, true);
    s.place_supports(&[-6.0, 6.0]);
    // The model lowered onto them: none fits under it any more.
    s.workspace
        .edit(|plate| plate.objects[0].pose.lift_mm = Some(2.0));
    s.prepare();
    let messages = s.slice();
    assert!(!s.slicing.current(&s.workspace));
    let refused = messages
        .iter()
        .find(|m| m.level == Level::Error)
        .unwrap_or_else(|| panic!("{messages:#?}"));
    let detail = refused.detail.as_deref().unwrap();
    assert!(detail.contains("no support"), "{detail}");
    assert!(
        detail.contains("support 1,") && detail.contains("support 2,"),
        "{detail}"
    );
}
