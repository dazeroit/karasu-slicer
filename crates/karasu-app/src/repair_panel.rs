//! The Repair section of an object's panel and its viewport markers (M6.4d r8, TD-092).
//!
//! - **State:** the printability of the object as it will be sliced, from the background
//!   preparation (the repaired mesh's diagnosis), or of the model as imported until it is known.
//! - **Defects:** each finding with what slicing will do about it, and a button that frames
//!   where it is.
//! - **Repairs:** the object's own repairs, a timeline operation (TD-090): the plate's repairs,
//!   a preset, or custom stages with their tolerances. Every change is one plate edit, so it is
//!   undone like any other, and **Remove repairs** returns to the plate's.
//! - **Report:** what the repairs changed, and **the next step** when the state is not ready:
//!   the next stronger preset.
//! - **Markers:** the selected object's defects and repairs, as small solids drawn on the GPU,
//!   through the model like a drain hole's ghost; the solid is uploaded once and placed per
//!   marker. Their colour is their severity (see [`Severity`]): red where slicing fails, orange
//!   where only hollowing is blocked, grey where slicing copes, green where a repair changed the
//!   model. The pointer over one names it; the section has a legend and hides them on request.
use crate::{
    appearance::Appearance,
    camera::Mat4,
    i18n::{decimal, tr},
    icons::{Icon, Icons},
    messages::{self, Level, Message},
    preparation_ui::PreparationUi,
    theme,
    viewport::{Draw, Viewport},
    widgets::{self, check_value_row, value_row, Unit, UnitField},
    workspace::Workspace,
};
use eframe::{egui, egui_wgpu};
use karasu_core::{
    geometry::Point3,
    mesh::{
        repair::RepairChange,
        topology::{DefectClass, DefectReport},
        TriangleMesh,
    },
    preparation::repair::{
        FillHoles, NonManifold, RepairPreset, RepairSpec, Shells, SourceRepairReport,
        DEFAULT_REBUILD_MM, MAX_REBUILD_MM, MIN_REBUILD_MM,
    },
};
use karasu_project::{
    diagnosis::{Diagnosis, Finding, Printability, Treatment},
    pipeline::RepairOutcome,
};

/// The viewport key of the marker solid.
const MARKER_KEY: u64 = 0x5245_5041_4952_4D4B; // "REPAIRMK"
/// Marker radius as a fraction of its distance from the eye, so every marker keeps its size on
/// screen whatever the zoom and wherever the camera looks.
pub const MARKER_SIZE: f64 = 0.006;

/// The presets offered, weakest first.
const LADDER: [RepairPreset; 4] = [
    RepairPreset::Exact,
    RepairPreset::Standard,
    RepairPreset::Aggressive,
    RepairPreset::Rebuild,
];

/// The Repair section's state.
#[derive(Default)]
pub struct RepairPanel {
    /// A world point to frame, taken by the window.
    focus: Option<[f64; 3]>,
    /// Whether the marker solid is uploaded.
    uploaded: bool,
    /// Whether the markers are hidden (shown by default).
    hidden: bool,
    /// The markers drawn last: world position and what they mark, for the pointer.
    markers: Vec<([f64; 3], String)>,
    /// Whether the stages show (the panel's Advanced switch).
    pub advanced: bool,
    /// A repair chosen for an object and not applied yet: the object, and the repair (none:
    /// the plate's).
    draft: Option<(usize, Option<RepairSpec>)>,
    /// A repair applied and not prepared yet, for the toast that reports it: the object, and
    /// the plate's revision the repair made.
    awaiting: Option<(usize, u64)>,
    /// Messages for the window: what applied repairs did.
    outbox: Vec<Message>,
}

/// What a marker marks, and so its colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// A defect that makes slicing fail.
    Fails,
    /// A defect that only blocks hollowing.
    Hollowing,
    /// A defect slicing handles.
    Harmless,
    /// A place a repair changed.
    Repaired,
}

impl Severity {
    fn color(self, appearance: &Appearance) -> crate::appearance::Rgba {
        match self {
            Self::Fails => appearance.marker_fails,
            Self::Hollowing => appearance.marker_hollowing,
            Self::Harmless => appearance.marker_harmless,
            Self::Repaired => appearance.marker_repaired,
        }
    }

    fn effect(self) -> String {
        match self {
            Self::Fails => tr!("repair-marker-fails"),
            Self::Hollowing => tr!("repair-marker-hollowing"),
            Self::Harmless => tr!("repair-marker-harmless"),
            Self::Repaired => tr!("repair-marker-repaired"),
        }
    }
}

/// The severity of a defect of `class` in a mesh with `defects`, sliced with or without layer
/// closure. Positions do not tell balanced non-manifold edges from unbalanced ones: all count as
/// failing while any is unbalanced.
#[must_use]
pub fn severity(class: DefectClass, defects: &DefectReport, close_holes: bool) -> Severity {
    match class {
        DefectClass::WindingConflict | DefectClass::NonOrientable => Severity::Fails,
        DefectClass::NonManifoldEdge if defects.unbalanced_non_manifold_edges > 0 => {
            Severity::Fails
        }
        DefectClass::Rim | DefectClass::Crack if !close_holes => Severity::Fails,
        DefectClass::Rim => Severity::Hollowing,
        _ => Severity::Harmless,
    }
}

fn defect_label(class: DefectClass) -> String {
    match class {
        DefectClass::Rim => tr!("repair-marker-rim"),
        DefectClass::Crack => tr!("repair-marker-crack"),
        DefectClass::NonManifoldEdge => tr!("repair-marker-non-manifold"),
        DefectClass::WindingConflict => tr!("repair-marker-winding"),
        DefectClass::Needle | DefectClass::Cap => tr!("repair-marker-zero-area"),
        DefectClass::Duplicate => tr!("repair-marker-duplicate"),
        DefectClass::OppositePair => tr!("repair-marker-opposite"),
        DefectClass::NonManifoldVertex => tr!("repair-marker-pinch"),
        DefectClass::SmallComponent => tr!("repair-marker-small"),
        DefectClass::NonOrientable => tr!("repair-marker-non-orientable"),
    }
}

/// What a repair sample marks, and its severity: changes are repairs; a hole left open, a body
/// whose volume could not be told and the rebuild's largest deviation are not.
fn change_label(change: RepairChange) -> (String, Severity) {
    let repaired = |text: String| (text, Severity::Repaired);
    match change {
        RepairChange::Duplicates => repaired(tr!("repair-marker-duplicates-collapsed")),
        RepairChange::Reoriented => repaired(tr!("repair-marker-reoriented")),
        RepairChange::TurnedOutward => repaired(tr!("repair-marker-turned-outward")),
        RepairChange::HoleFilled => repaired(tr!("repair-marker-hole-filled")),
        RepairChange::CrackClosed => repaired(tr!("repair-marker-crack-closed")),
        RepairChange::Welded => repaired(tr!("repair-marker-welded")),
        RepairChange::NeedleRemoved => repaired(tr!("repair-marker-zero-area-removed")),
        RepairChange::EdgeCollapsed => repaired(tr!("repair-marker-edge-collapsed")),
        RepairChange::CapFlipped => repaired(tr!("repair-marker-flat-flipped")),
        RepairChange::FinRemoved => repaired(tr!("repair-marker-fin-removed")),
        RepairChange::FacesCut => repaired(tr!("repair-marker-faces-cut")),
        RepairChange::SmallShellRemoved => repaired(tr!("repair-marker-small-removed")),
        RepairChange::InnerShellRemoved => repaired(tr!("repair-marker-inner-removed")),
        RepairChange::VisibilityFlipped => repaired(tr!("repair-marker-visibility")),
        RepairChange::HoleRefused => (tr!("repair-marker-hole-refused"), Severity::Hollowing),
        RepairChange::AmbiguousVolume => (tr!("repair-marker-flat-body"), Severity::Fails),
        RepairChange::RebuildDeviation => (tr!("repair-marker-deviation"), Severity::Harmless),
    }
}

/// The markers' legend: a swatch per severity.
fn legend(ui: &mut egui::Ui, appearance: &Appearance) {
    ui.horizontal_wrapped(|ui| {
        for severity in [
            Severity::Fails,
            Severity::Hollowing,
            Severity::Harmless,
            Severity::Repaired,
        ] {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
            ui.painter()
                .circle_filled(rect.center(), 4.5, severity.color(appearance).color());
            ui.label(egui::RichText::new(severity.effect()).small());
        }
    });
}

/// The name of a preset, or of the plate's repairs for `None`.
#[must_use]
pub fn preset_name(preset: Option<RepairPreset>) -> String {
    match preset {
        None => tr!("repair-preset-plate"),
        Some(RepairPreset::Off) => tr!("repair-preset-off"),
        Some(RepairPreset::Exact) => tr!("repair-preset-exact"),
        Some(RepairPreset::Standard) => tr!("repair-preset-standard"),
        Some(RepairPreset::Aggressive) => tr!("repair-preset-aggressive"),
        Some(RepairPreset::Rebuild) => tr!("repair-preset-rebuild"),
    }
}

/// The preset `spec` is, a rebuild at any resolution included.
#[must_use]
pub fn preset_of(spec: RepairSpec) -> Option<RepairPreset> {
    RepairPreset::of(RepairSpec {
        rebuild_mm: spec.rebuild_mm.map(|_| DEFAULT_REBUILD_MM),
        ..spec
    })
}

/// How strong a repair is: its place on the ladder (custom repairs count as exact ones).
#[must_use]
pub fn rank(spec: Option<RepairSpec>) -> usize {
    match spec.map(preset_of) {
        None | Some(Some(RepairPreset::Off)) => 0,
        Some(Some(preset)) => LADDER
            .iter()
            .position(|&p| p == preset)
            .map_or(1, |p| p + 1),
        Some(None) => 1,
    }
}

/// The next preset stronger than `spec`, if any.
#[must_use]
pub fn next_preset(spec: Option<RepairSpec>) -> Option<RepairPreset> {
    LADDER.get(rank(spec)).copied()
}

/// The defect classes whose positions show a finding.
fn classes(finding: Finding) -> &'static [DefectClass] {
    match finding {
        Finding::Holes { .. } => &[DefectClass::Rim],
        Finding::Cracks { .. } => &[DefectClass::Crack],
        Finding::NonManifold { .. } | Finding::Fins { .. } => &[DefectClass::NonManifoldEdge],
        Finding::InconsistentOrientation { .. } => &[DefectClass::WindingConflict],
        Finding::DuplicateFaces { .. } => &[DefectClass::Duplicate, DefectClass::OppositePair],
        Finding::DegenerateFaces { .. } => &[DefectClass::Needle, DefectClass::Cap],
        Finding::PinchedVertices { .. } => &[DefectClass::NonManifoldVertex],
        Finding::SmallBodies { .. } => &[DefectClass::SmallComponent],
        Finding::NonOrientable { .. } => &[DefectClass::NonOrientable],
        _ => &[],
    }
}

fn state_text(state: Printability) -> (String, egui::Color32) {
    match state {
        Printability::Ready => (tr!("repair-state-ready"), theme::SUCCESS.color()),
        Printability::ReadyWithNotes => (tr!("repair-state-notes"), theme::SUCCESS.color()),
        Printability::NeedsRepairForHollowing => {
            (tr!("repair-state-hollowing"), theme::WARNING.color())
        }
        Printability::NeedsRepair => (tr!("repair-state-needs"), theme::ERROR.color()),
    }
}

/// The icon of a state, as its colour's severity.
fn state_icon(state: Printability) -> Icon {
    match state {
        Printability::Ready | Printability::ReadyWithNotes => Icon::Success,
        Printability::NeedsRepairForHollowing => Icon::Warning,
        Printability::NeedsRepair => Icon::Error,
    }
}

/// The message of a prepared repair: the model's state as a success, warning or error, and what
/// the repairs changed as its detail.
fn repaired_message(outcome: &RepairOutcome, close_holes: bool) -> Message {
    let (level, text) = match outcome.state.printability(close_holes) {
        Printability::Ready => (Level::Success, tr!("repair-done-ready")),
        Printability::ReadyWithNotes => (Level::Success, tr!("repair-done-notes")),
        Printability::NeedsRepairForHollowing => (Level::Warning, tr!("repair-done-hollowing")),
        Printability::NeedsRepair => (Level::Error, tr!("repair-done-needs")),
    };
    let lines = report_lines(&outcome.report);
    let detail = if lines.is_empty() {
        tr!("repair-report-none")
    } else {
        lines.join("\n")
    };
    Message::new(level, text).detail(detail)
}

/// What the repairs changed, one line per stage that changed something.
fn report_lines(report: &SourceRepairReport) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(dropped) = &report.non_finite {
        lines.push(tr!(
            "repair-report-dropped",
            count = dropped.facets,
            runs = dropped.runs,
            first = dropped.ranges.first().map_or(0, |r| r.0)
        ));
    }
    if let Some(w) = report.weld.as_ref().filter(|w| w.clusters > 0) {
        lines.push(tr!(
            "repair-report-weld",
            count = w.vertices_merged,
            mm = decimal(w.max_displacement_mm, 4)
        ));
    }
    if let Some(d) = &report.degenerate {
        if d.needles_removed + d.edges_collapsed + d.caps_flipped > 0 {
            lines.push(tr!(
                "repair-report-degenerate",
                needles = d.needles_removed,
                edges = d.edges_collapsed,
                flips = d.caps_flipped
            ));
        }
    }
    if report.mesh.dropped_duplicate_faces > 0 {
        lines.push(tr!(
            "repair-report-duplicates",
            count = report.mesh.dropped_duplicate_faces
        ));
    }
    if report.mesh.flipped_faces > 0 {
        lines.push(tr!(
            "repair-report-flipped",
            count = report.mesh.flipped_faces,
            outward = report.mesh.outward_flipped_components
        ));
    }
    if let Some(n) = report
        .non_manifold
        .as_ref()
        .filter(|n| n.fins_removed + n.faces_cut > 0)
    {
        lines.push(tr!(
            "repair-report-fins",
            count = n.fins_removed,
            cut = n.faces_cut
        ));
    }
    if let Some(v) = report.visibility.as_ref().filter(|v| v.parts_flipped > 0) {
        lines.push(tr!("repair-report-visibility", count = v.parts_flipped));
    }
    if let Some(f) = &report.fill {
        if f.holes_filled + f.cracks_closed + f.holes_refused > 0 {
            lines.push(tr!(
                "repair-report-holes",
                filled = f.holes_filled,
                area = decimal(f.largest_cap_mm2, 2),
                cracks = f.cracks_closed,
                left = f.holes_refused
            ));
        }
    }
    if let Some(s) = report
        .shells
        .as_ref()
        .filter(|s| s.small_removed + s.inner_removed > 0)
    {
        lines.push(tr!(
            "repair-report-shells",
            small = s.small_removed,
            inner = s.inner_removed
        ));
    }
    if let Some(r) = &report.rebuild {
        lines.push(tr!(
            "repair-report-rebuild",
            mm = decimal(r.resolution_mm, 3),
            deviation = decimal(r.max_deviation_mm, 3),
            lost = decimal(r.source_deviation_mm, 3)
        ));
    }
    lines
}

/// The custom stages: every stage with its tolerance; returns whether `spec` changed.
#[allow(clippy::too_many_lines)] // One control per stage.
fn stages(ui: &mut egui::Ui, spec: &mut RepairSpec) -> bool {
    let before = *spec;
    // A tolerance's field shows its default while the stage is off (the panel keeps its
    // structure), and the value it will take when turned on.
    let tolerance = |ui: &mut egui::Ui, value: &mut Option<f64>, label: String, default: f64| {
        let mut on = value.is_some();
        let mut mm = value.unwrap_or(default);
        check_value_row(ui, &mut on, label, |ui| {
            ui.add(
                UnitField::new(&mut mm, Unit::Millimetre)
                    .range(0.0..=1.0)
                    .step(0.0001)
                    .decimals(4),
            )
        });
        *value = on.then_some(mm);
    };
    ui.checkbox(&mut spec.non_finite, tr!("repair-stage-non-finite"))
        .on_hover_text(tr!("repair-stage-non-finite-hint"));
    tolerance(ui, &mut spec.weld_mm, tr!("repair-stage-weld"), 0.001);
    tolerance(
        ui,
        &mut spec.degenerate_mm,
        tr!("repair-stage-degenerate"),
        0.001,
    );
    ui.checkbox(&mut spec.duplicates, tr!("repair-duplicates"));
    ui.checkbox(&mut spec.orientation, tr!("repair-orientation"));
    ui.checkbox(&mut spec.outward, tr!("repair-outward"));
    if spec.outward {
        spec.orientation = true;
    }
    let mut fins = spec.non_manifold.is_some();
    let mut cut = spec.non_manifold == Some(NonManifold::Aggressive);
    ui.checkbox(&mut fins, tr!("repair-stage-fins"));
    ui.add_enabled(fins, egui::Checkbox::new(&mut cut, tr!("repair-stage-cut")));
    spec.non_manifold = fins.then_some(if cut {
        NonManifold::Aggressive
    } else {
        NonManifold::Fins
    });
    ui.checkbox(&mut spec.visibility, tr!("repair-stage-visibility"));
    let mut fill = spec.fill_holes.is_some();
    if ui.checkbox(&mut fill, tr!("repair-fill-holes")).changed() {
        spec.fill_holes = fill.then(FillHoles::default);
        if !fill {
            spec.crack_mm = None;
        }
    }
    if let Some(holes) = &mut spec.fill_holes {
        ui.indent("repair-holes", |ui| {
            ui.checkbox(&mut holes.split_rims, tr!("repair-stage-split"));
            ui.checkbox(&mut holes.refine, tr!("repair-stage-refine"));
            let mut unbounded = holes.max_hole_edges == usize::MAX;
            if ui
                .checkbox(&mut unbounded, tr!("repair-stage-unbounded"))
                .changed()
            {
                holes.max_hole_edges = if unbounded {
                    usize::MAX
                } else {
                    FillHoles::default().max_hole_edges
                };
            }
        });
        tolerance(ui, &mut spec.crack_mm, tr!("repair-stage-cracks"), 0.001);
    }
    let mut shells = spec.shells.is_some();
    if ui
        .checkbox(&mut shells, tr!("repair-stage-shells"))
        .changed()
    {
        spec.shells = shells.then_some(Shells {
            min_area_mm2: 1.0,
            min_volume_mm3: 0.0,
            inner: false,
        });
    }
    if let Some(bounds) = &mut spec.shells {
        ui.indent("repair-shells", |ui| {
            value_row(ui, tr!("repair-stage-shell-area"), |ui| {
                ui.add(
                    UnitField::new(&mut bounds.min_area_mm2, Unit::SquareMillimetre)
                        .range(0.0..=1000.0)
                        .step(0.1)
                        .decimals(1),
                )
            });
            ui.checkbox(&mut bounds.inner, tr!("repair-stage-inner"));
        });
    }
    let mut rebuild = spec.rebuild_mm.is_some();
    if ui
        .checkbox(&mut rebuild, tr!("repair-stage-rebuild"))
        .on_hover_text(tr!("repair-stage-rebuild-hint"))
        .changed()
    {
        spec.rebuild_mm = rebuild.then_some(DEFAULT_REBUILD_MM);
    }
    if let Some(mm) = &mut spec.rebuild_mm {
        ui.indent("repair-rebuild", |ui| resolution(ui, mm));
    }
    *spec != before
}

fn resolution(ui: &mut egui::Ui, mm: &mut f64) {
    value_row(ui, tr!("repair-rebuild-resolution"), |ui| {
        ui.add(
            UnitField::new(mm, Unit::Millimetre)
                .range(MIN_REBUILD_MM..=MAX_REBUILD_MM)
                .step(0.005)
                .decimals(3),
        )
    });
}

impl RepairPanel {
    /// What the marker under `pointer` (in `rect`, the viewport) marks, if one is within reach.
    #[must_use]
    pub fn marker_at(
        &self,
        camera: &crate::camera::Camera,
        rect: egui::Rect,
        pointer: egui::Pos2,
    ) -> Option<&str> {
        const REACH: f32 = 10.0;
        let size = [f64::from(rect.width()), f64::from(rect.height())];
        self.markers
            .iter()
            .filter_map(|(p, label)| {
                let [x, y] = camera.screen(*p, size)?;
                #[allow(clippy::cast_possible_truncation)] // Screen positions.
                let at = rect.min + egui::vec2(x as f32, y as f32);
                let distance = at.distance(pointer);
                (distance <= REACH).then_some((distance, label.as_str()))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, label)| label)
    }

    /// A world point the user asked to see, once.
    pub fn take_focus(&mut self) -> Option<[f64; 3]> {
        self.focus.take()
    }

    /// The Repair tool's panel (TD-098), for the selected object `index` if any: the repair to
    /// apply (a preset, and its stages when Advanced is on), then the defects and the markers.
    /// A repair is chosen as a draft and applied with Apply repairs; when its preparation ends,
    /// a message (a toast) says what it did. Without a selection the structure is the same, its
    /// controls disabled, the preset showing the one an opened model gets. The draft's buttons
    /// and the model's state are the panel's footer ([`Self::footer`]).
    #[allow(clippy::too_many_lines)] // One panel, top to bottom.
    pub fn section(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &mut Workspace,
        preparation: &PreparationUi,
        index: Option<usize>,
        appearance: &Appearance,
    ) {
        let object = index.and_then(|i| {
            workspace
                .plate()
                .objects
                .get(i)
                .map(|o| (i, o.repair(), o.model))
        });
        // A draft belongs to the object it was made for.
        if self.draft.is_some_and(|(o, _)| Some(o) != index) {
            self.draft = None;
        }
        let own = object.and_then(|(_, own, _)| own);
        let shown = match object {
            Some((i, _, _)) => self
                .draft
                .filter(|(o, _)| *o == i)
                .map_or(own, |(_, draft)| draft),
            None => workspace.auto_repair().map(RepairPreset::spec),
        };
        let mut chosen = shown;
        ui.add_enabled_ui(object.is_some(), |ui| {
            let current = shown.map(preset_of);
            let label = match current {
                Some(None) => tr!("repair-preset-custom"),
                Some(Some(preset)) => preset_name(Some(preset)),
                None => preset_name(None),
            };
            let combo = egui::ComboBox::from_id_salt("repair-preset")
                .selected_text(label)
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(shown.is_none(), preset_name(None))
                        .clicked()
                    {
                        chosen = None;
                    }
                    for preset in std::iter::once(RepairPreset::Off).chain(LADDER) {
                        if ui
                            .selectable_label(
                                current == Some(Some(preset)),
                                preset_name(Some(preset)),
                            )
                            .clicked()
                        {
                            chosen = Some(preset.spec());
                        }
                    }
                });
            crate::widgets::name(&combo.response, &tr!("a11y-repair-preset"));
            // The stages, for experienced users: with the plate's repairs they show its stages,
            // disabled (they are the Slice panel's).
            if self.advanced {
                let own_repair = chosen.is_some();
                let mut custom = chosen.unwrap_or(workspace.plate().settings.repair);
                ui.add_enabled_ui(own_repair, |ui| {
                    let before = custom;
                    if let Some(mm) = &mut custom.rebuild_mm {
                        resolution(ui, mm);
                    }
                    egui::CollapsingHeader::new(tr!("repair-stages"))
                        .id_salt("repair-stages")
                        .default_open(true)
                        .show(ui, |ui| {
                            stages(ui, &mut custom);
                        });
                    if own_repair && custom != before {
                        chosen = Some(custom);
                    }
                });
            }
            // The draft's state and its buttons are the footer's (design rule, TD-098).
            if let Some((i, _, _)) = object {
                self.draft = (chosen != own).then_some((i, chosen));
            }
        });
        ui.add_space(4.0);
        // The defects: of the mesh as it will be sliced, or of the model as imported meanwhile.
        let (diagnosis, as_imported, close_holes) = Self::diagnosis(workspace, preparation, index);
        let findings: Vec<Finding> = diagnosis.as_ref().map_or_default(Diagnosis::findings);
        egui::CollapsingHeader::new(tr!("repair-defects", count = findings.len()))
            .id_salt("repair-defects")
            .default_open(true)
            .show(ui, |ui| {
                let (Some(diagnosis), Some((index, _, _))) = (&diagnosis, object) else {
                    return;
                };
                let treatment = Treatment {
                    repair: if as_imported {
                        own.unwrap_or(workspace.plate().settings.repair)
                    } else {
                        RepairPreset::Off.spec()
                    },
                    close_holes,
                };
                let placement = workspace
                    .placements()
                    .get(index)
                    .and_then(|p| p.as_ref().ok())
                    .map(|p| p.transform);
                for &finding in &findings {
                    ui.horizontal_wrapped(|ui| {
                        let point = diagnosis
                            .defects
                            .samples
                            .iter()
                            .find(|s| classes(finding).contains(&s.class))
                            .map(|s| s.point);
                        if let (Some([x, y, z]), Some(transform)) = (point, placement) {
                            if ui
                                .small_button(tr!("repair-show"))
                                .on_hover_text(tr!("repair-show-hint"))
                                .clicked()
                            {
                                if let Ok(p) = transform.transform_point(Point3::new(x, y, z)) {
                                    self.focus = Some([p.x, p.y, p.z]);
                                }
                            }
                        }
                        ui.add(
                            egui::Label::new(tr!(
                                "finding-line",
                                finding = messages::describe(finding),
                                consequence =
                                    messages::consequence(finding, finding.handling(treatment)),
                            ))
                            .wrap(),
                        );
                    });
                }
            });
        // A view preference, not the object's: always available, its legend always shown.
        egui::CollapsingHeader::new(tr!("repair-markers"))
            .id_salt("repair-markers")
            .show(ui, |ui| {
                let mut shown = !self.hidden;
                if ui
                    .checkbox(&mut shown, tr!("repair-show-markers"))
                    .changed()
                {
                    self.hidden = !shown;
                }
                legend(ui, appearance);
            });
    }

    /// The panel's footer (design rule, TD-098): the draft's state with Apply repairs and
    /// Remove repairs, then the selected object's state and, when its repair is not enough, a
    /// button applying the next stronger preset at once.
    pub fn footer(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &mut Workspace,
        preparation: &PreparationUi,
        index: Option<usize>,
        icons: Option<&Icons>,
    ) {
        let own = index
            .and_then(|i| workspace.plate().objects.get(i))
            .and_then(karasu_project::project::PlateObject::repair);
        self.draft_actions(ui, workspace, index, own);
        ui.add_space(4.0);
        let (diagnosis, as_imported, close_holes) = Self::diagnosis(workspace, preparation, index);
        let state = diagnosis.as_ref().map(|d| d.printability(close_holes));
        let (text, colour, icon) = match (state, index) {
            (Some(state), _) => {
                let (text, colour) = state_text(state);
                let text = if as_imported {
                    tr!("repair-state-as-imported", state = text)
                } else {
                    text
                };
                (text, colour, Some(state_icon(state)))
            }
            (None, Some(_)) => (tr!("repair-checking"), theme::BORDER.color(), None),
            (None, None) => ("–".to_owned(), theme::BORDER.color(), None),
        };
        widgets::state_line(ui, icons, icon, &text, colour);
        let next = next_preset(own)
            .filter(|_| state.is_some_and(|s| s >= Printability::NeedsRepairForHollowing));
        if let (Some(next), Some(i)) = (next, index) {
            if ui
                .add_sized(
                    [ui.available_width(), 0.0],
                    egui::Button::new(tr!("repair-use", preset = preset_name(Some(next)))),
                )
                .on_hover_text(tr!("repair-try-hint"))
                .clicked()
            {
                self.apply(workspace, i, Some(next.spec()));
            }
        }
    }

    /// The draft's state for object `index`, whose own repair is `own`, and the buttons that
    /// apply or remove it: disabled without a selection.
    fn draft_actions(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &mut Workspace,
        index: Option<usize>,
        own: Option<RepairSpec>,
    ) {
        let draft = self
            .draft
            .filter(|(o, _)| Some(*o) == index)
            .map(|(_, draft)| draft);
        let (state, colour) = match (index, draft) {
            (None, _) => ("–".to_owned(), theme::BORDER.color()),
            (Some(_), Some(_)) => (tr!("repair-changed"), theme::WARNING.color()),
            (Some(_), None) => {
                let name = own.map_or_else(
                    || preset_name(None),
                    |spec| {
                        preset_of(spec)
                            .map_or_else(|| tr!("repair-preset-custom"), |p| preset_name(Some(p)))
                    },
                );
                (tr!("repair-applied", preset = name), theme::ACCENT.color())
            }
        };
        ui.add_enabled_ui(index.is_some(), |ui| {
            ui.add(egui::Label::new(egui::RichText::new(state).color(colour).small()).wrap());
            let (apply, remove) = widgets::action_pair(
                ui,
                [tr!("repair-apply"), tr!("repair-remove")],
                (draft.is_some(), own.is_some()),
            );
            if let Some(i) = index {
                if apply.clicked() {
                    self.apply(workspace, i, draft.unwrap_or(own));
                } else if remove.clicked() {
                    self.apply(workspace, i, None);
                }
            }
        });
    }

    /// Apply `repair` to object `index`, and wait for its preparation to report it.
    fn apply(&mut self, workspace: &mut Workspace, index: usize, repair: Option<RepairSpec>) {
        workspace.set_repair(index, repair);
        self.draft = None;
        self.awaiting = Some((index, workspace.revision()));
    }

    /// The diagnosis shown for object `index`: of the mesh as it will be sliced once prepared,
    /// else of the model as imported (and so `true`); with the plate's layer closure.
    fn diagnosis(
        workspace: &Workspace,
        preparation: &PreparationUi,
        index: Option<usize>,
    ) -> (Option<Diagnosis>, bool, bool) {
        let close_holes = workspace.plate().settings.close_holes_mm.is_some();
        let Some((index, model)) =
            index.and_then(|i| workspace.plate().objects.get(i).map(|o| (i, o.model)))
        else {
            return (None, false, close_holes);
        };
        let outcome = preparation
            .current(workspace)
            .and_then(|p| p.objects.get(index))
            .and_then(|r| r.as_ref().ok())
            .and_then(|o| o.repair());
        match outcome {
            Some(outcome) => (Some(outcome.state.clone()), false, close_holes),
            None => (workspace.diagnosis(model).cloned(), true, close_holes),
        }
    }

    /// The messages of applied repairs whose preparation ended: what each did. A repair
    /// overtaken by another edit before it was prepared reports nothing.
    pub fn take_messages(
        &mut self,
        workspace: &Workspace,
        preparation: &PreparationUi,
    ) -> Vec<Message> {
        if let Some((index, revision)) = self.awaiting {
            if workspace.revision() != revision {
                self.awaiting = None;
            } else if let Some(prepared) = preparation.current(workspace) {
                self.awaiting = None;
                let close_holes = workspace.plate().settings.close_holes_mm.is_some();
                match prepared.objects.get(index) {
                    Some(Ok(object)) => {
                        if let Some(outcome) = object.repair() {
                            self.outbox.push(repaired_message(outcome, close_holes));
                        }
                    }
                    Some(Err(error)) => self.outbox.push(
                        Message::new(Level::Error, tr!("repair-done-failed"))
                            .detail(error.reason().to_owned()),
                    ),
                    None => {}
                }
            }
        }
        std::mem::take(&mut self.outbox)
    }

    /// The markers of object `index`'s defects and repairs, seen from `eye` (world mm).
    #[allow(clippy::too_many_arguments)] // The viewport, the plate and the eye.
    pub fn draws(
        &mut self,
        viewport: &mut Viewport,
        state: &egui_wgpu::RenderState,
        workspace: &mut Workspace,
        preparation: &PreparationUi,
        index: Option<usize>,
        eye: [f64; 3],
        appearance: &Appearance,
    ) -> Vec<Draw> {
        self.markers.clear();
        let Some(index) = index.filter(|_| !self.hidden) else {
            return Vec::new();
        };
        let Some(outcome) = preparation
            .current(workspace)
            .and_then(|p| p.objects.get(index))
            .and_then(|r| r.as_ref().ok())
            .and_then(|o| o.repair())
            .cloned()
        else {
            return Vec::new();
        };
        let Some(Ok(placement)) = workspace.placements().get(index).cloned() else {
            return Vec::new();
        };
        if !self.uploaded {
            viewport.upload(&state.device, MARKER_KEY, &marker(), [0.0; 3]);
            self.uploaded = true;
        }
        let close_holes = workspace.plate().settings.close_holes_mm.is_some();
        let defects = &outcome.state.defects;
        // A crack's rim is marked as the crack.
        let cracks: Vec<[f64; 3]> = defects
            .samples
            .iter()
            .filter(|s| s.class == DefectClass::Crack)
            .map(|s| s.point)
            .collect();
        let marked = defects
            .samples
            .iter()
            .filter(|s| !(s.class == DefectClass::Rim && cracks.contains(&s.point)))
            .map(|s| {
                (
                    s.point,
                    defect_label(s.class),
                    severity(s.class, defects, close_holes),
                )
            })
            .chain(outcome.report.all_samples().map(|s| {
                let (label, severity) = change_label(s.change);
                (s.point, label, severity)
            }));
        let mut draws = Vec::new();
        for ([x, y, z], label, severity) in marked {
            let Ok(p) = placement.transform.transform_point(Point3::new(x, y, z)) else {
                continue;
            };
            self.markers.push((
                [p.x, p.y, p.z],
                tr!(
                    "repair-marker-tip",
                    what = label,
                    effect = severity.effect()
                ),
            ));
            let size = MARKER_SIZE
                * ((p.x - eye[0]).powi(2) + (p.y - eye[1]).powi(2) + (p.z - eye[2]).powi(2)).sqrt();
            let color = severity.color(appearance).linear();
            let mut ghost = color;
            ghost[3] *= 0.5;
            let model = Mat4([
                [size, 0.0, 0.0, p.x],
                [0.0, size, 0.0, p.y],
                [0.0, 0.0, size, p.z],
                [0.0, 0.0, 0.0, 1.0],
            ]);
            for (color, overlay) in [(color, false), (ghost, true)] {
                draws.push(Draw {
                    mesh: MARKER_KEY,
                    model,
                    color,
                    overlay,
                    cap: crate::viewport::Cap::Off,
                    holes: [0, 0],
                    bore: None,
                    outside: None,
                    triangles: None,
                    look: crate::viewport::Look::Solid,
                });
            }
        }
        draws
    }
}

/// The marker solid: an octahedron of unit radius.
pub(crate) fn marker() -> TriangleMesh {
    let points = vec![
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(-1.0, 0.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
        Point3::new(0.0, -1.0, 0.0),
        Point3::new(0.0, 0.0, 1.0),
        Point3::new(0.0, 0.0, -1.0),
    ];
    let faces = vec![
        [0, 2, 4],
        [2, 1, 4],
        [1, 3, 4],
        [3, 0, 4],
        [2, 0, 5],
        [1, 2, 5],
        [3, 1, 5],
        [0, 3, 5],
    ];
    // Six distinct finite points and valid indices: construction cannot fail.
    TriangleMesh::try_new(points, faces).expect("the marker octahedron is a valid mesh")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_stage_fits_the_panel_in_english_and_in_a_longer_language() {
        // Every stage on, Advanced on: the panel's rows wrap within its width instead of
        // widening it, also in the pseudo-locale, whose texts are about a third longer.
        let fixtures =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/reference/stl");
        let mut workspace = Workspace::new();
        workspace.open(fixtures.join("fx-01-cube.stl"));
        crate::workspace::tests::wait(&mut workspace);
        let mut all = RepairPreset::Aggressive.spec();
        all.shells = Some(Shells {
            min_area_mm2: 1.0,
            min_volume_mm3: 0.0,
            inner: true,
        });
        all.rebuild_mm = Some(DEFAULT_REBUILD_MM);
        all.crack_mm = Some(0.001);
        workspace.set_repair(0, Some(all));
        let preparation = PreparationUi::default();
        // The width a panel's body has: the panel, less its frame.
        let width = 316.0;
        for locale in ["en-US", "pseudo"] {
            crate::i18n::install(crate::i18n::Catalog::open(locale, None).unwrap());
            let mut panel = RepairPanel {
                advanced: true,
                ..RepairPanel::default()
            };
            let ctx = egui::Context::default();
            let mut used = 0.0;
            for _ in 0..2 {
                let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                    ui.allocate_ui(egui::vec2(width, 4000.0), |ui| {
                        ui.set_width(width);
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                        panel.section(
                            ui,
                            &mut workspace,
                            &preparation,
                            Some(0),
                            &Appearance::default(),
                        );
                        used = ui.min_rect().width();
                    });
                });
                output.textures_delta.clear();
            }
            assert!(used <= width + 0.5, "{locale}: {used} wide in {width}");
        }
        crate::i18n::install(crate::i18n::Catalog::reference());
    }

    #[test]
    fn a_repair_is_a_draft_until_applied_and_its_report_waits_for_its_preparation() {
        let fixtures =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/reference/stl");
        let mut workspace = Workspace::new();
        workspace.open(fixtures.join("fx-01-cube.stl"));
        crate::workspace::tests::wait(&mut workspace);
        let imported = workspace.plate().objects[0].repair();
        let standard = Some(RepairPreset::Standard.spec());
        assert_ne!(imported, standard);
        // A draft shown in the panel changes nothing on the plate.
        let mut panel = RepairPanel {
            draft: Some((0, standard)),
            ..RepairPanel::default()
        };
        let preparation = PreparationUi::default();
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            panel.section(
                ui,
                &mut workspace,
                &preparation,
                Some(0),
                &Appearance::default(),
            );
        });
        output.textures_delta.clear();
        assert_eq!(workspace.plate().objects[0].repair(), imported);
        assert_eq!(panel.draft, Some((0, standard)));
        // Applied, it is the model's, and its report waits for the preparation.
        panel.apply(&mut workspace, 0, standard);
        assert_eq!(workspace.plate().objects[0].repair(), standard);
        assert_eq!(panel.draft, None);
        assert!(panel.awaiting.is_some());
        assert_eq!(panel.take_messages(&workspace, &preparation), Vec::new());
        // Another edit before it was prepared: the repair is overtaken and reports nothing.
        workspace.edit(|plate| plate.objects[0].translate_mm[0] += 1.0);
        assert_eq!(panel.take_messages(&workspace, &preparation), Vec::new());
        assert_eq!(panel.awaiting, None);
        // A draft for another model is dropped when the selection changes.
        panel.draft = Some((1, standard));
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            panel.section(
                ui,
                &mut workspace,
                &preparation,
                Some(0),
                &Appearance::default(),
            );
        });
        output.textures_delta.clear();
        assert_eq!(panel.draft, None);
    }

    #[test]
    fn without_a_selection_the_panel_keeps_its_structure_and_changes_nothing() {
        // The same controls, disabled, the preset showing what an opened model gets (Exact).
        let ctx = egui::Context::default();
        let mut panel = RepairPanel::default();
        let mut workspace = Workspace::new();
        let preparation = PreparationUi::default();
        let before = workspace.plate().clone();
        let mut texts = Vec::new();
        for _ in 0..2 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                panel.section(
                    ui,
                    &mut workspace,
                    &preparation,
                    None,
                    &Appearance::default(),
                );
                panel.footer(ui, &mut workspace, &preparation, None, None);
            });
            output.textures_delta.clear();
            texts = output
                .shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::epaint::Shape::Text(t) => Some(t.galley.text().to_owned()),
                    _ => None,
                })
                .collect();
        }
        for expected in [
            preset_name(Some(RepairPreset::Exact)),
            tr!("repair-apply"),
            tr!("repair-remove"),
            tr!("repair-defects", count = 0_usize),
            tr!("repair-markers"),
        ] {
            assert!(texts.contains(&expected), "{expected} in {texts:?}");
        }
        assert_eq!(*workspace.plate(), before);
    }
    use crate::messages::Message;
    use karasu_project::pipeline::{slice, Silent};

    #[test]
    fn markers_take_the_colour_of_what_they_mean() {
        let clean = karasu_core::mesh::TriangleMesh::try_new(Vec::new(), Vec::new())
            .unwrap()
            .defect_report_with_cancellation(
                karasu_core::mesh::topology::TopologyLimits::default(),
                &mut karasu_core::cancellation::Cancellation::never(),
            )
            .unwrap();
        // Holes: closed in layers they only block hollowing; without closure slicing fails.
        assert_eq!(
            severity(DefectClass::Rim, &clean, true),
            Severity::Hollowing
        );
        assert_eq!(severity(DefectClass::Rim, &clean, false), Severity::Fails);
        // Zero-area cracks need closure only.
        assert_eq!(
            severity(DefectClass::Crack, &clean, true),
            Severity::Harmless
        );
        // Touching bodies: harmless while every non-manifold edge balances.
        assert_eq!(
            severity(DefectClass::NonManifoldEdge, &clean, true),
            Severity::Harmless
        );
        let fins = DefectReport {
            unbalanced_non_manifold_edges: 1,
            ..clean.clone()
        };
        assert_eq!(
            severity(DefectClass::NonManifoldEdge, &fins, true),
            Severity::Fails
        );
        assert_eq!(
            severity(DefectClass::NonManifoldVertex, &clean, true),
            Severity::Harmless
        );
        assert_eq!(
            severity(DefectClass::WindingConflict, &clean, true),
            Severity::Fails
        );
        // Repairs are green, apart from what they could not do.
        assert_eq!(
            change_label(RepairChange::CrackClosed).1,
            Severity::Repaired
        );
        assert_eq!(
            change_label(RepairChange::HoleRefused).1,
            Severity::Hollowing
        );
    }

    #[test]
    fn presets_climb_a_ladder() {
        assert_eq!(rank(None), 0);
        assert_eq!(next_preset(None), Some(RepairPreset::Exact));
        assert_eq!(
            next_preset(Some(RepairPreset::Exact.spec())),
            Some(RepairPreset::Standard)
        );
        let rebuild = RepairSpec {
            rebuild_mm: Some(0.2),
            ..RepairPreset::Rebuild.spec()
        };
        assert_eq!(preset_of(rebuild), Some(RepairPreset::Rebuild));
        assert_eq!(next_preset(Some(rebuild)), None);
        // Custom repairs count as exact ones.
        let custom = RepairSpec {
            weld_mm: Some(0.01),
            ..RepairPreset::Exact.spec()
        };
        assert_eq!(preset_of(custom), None);
        assert_eq!(next_preset(Some(custom)), Some(RepairPreset::Standard));
    }

    /// TD-092 r8's acceptance: every tracked fixture, opened as a user opens it (repaired on
    /// import), reaches slicing; when a slice fails, the first fix its message offers is the
    /// guided step, and the slice after it succeeds.
    #[test]
    fn every_corpus_model_reaches_slicing_from_the_interface() {
        let (printer, resin, setting) =
            crate::workspace::tests::catalog_profiles("anycubic-photon-mono-x");
        let fixtures =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/reference/stl");
        let output = std::env::temp_dir().join(format!("karasu-corpus-{}", std::process::id()));
        std::fs::create_dir_all(&output).unwrap();
        for name in [
            "fx-01-cube.stl",
            "fx-02-cylinder.stl",
            "fx-03-sphere.stl",
            "fx-04-hollow-box.stl",
            "fx-05-rotated-prism.stl",
            "fx-06-multiple-volumes.stl",
            "fx-07-internal-cavity.stl",
            "fx-08-degenerate-faces.stl",
            "fx-09-reversed-face.stl",
            "fx-10a-open.stl",
            "fx-10b-non-manifold.stl",
        ] {
            // The profiles first, as a user has them: an opened model is then centred.
            let mut workspace = Workspace::new();
            // The format the printer takes, as the toolbar selects it.
            let format = printer
                .format_ids
                .iter()
                .find(|f| karasu_project::request::format_ids().contains(f.as_str()))
                .cloned()
                .unwrap();
            workspace.edit(|plate| {
                plate.printer = Some(printer.clone());
                plate.resin = Some(resin.clone());
                plate.print_setting = Some(setting.clone());
                plate.settings.format = format;
            });
            workspace.open(fixtures.join(name));
            crate::workspace::tests::wait(&mut workspace);
            assert!(workspace.notices().iter().any(|n| n
                .detail
                .as_deref()
                .is_some_and(|d| d.contains(&preset_name(Some(RepairPreset::Exact))))));
            let mut fixed = Vec::new();
            for attempt in 0..3 {
                let extension =
                    crate::slicing_ui::extension(&workspace.plate().settings.format).unwrap();
                let path = output.join(format!("{name}-{attempt}.{extension}"));
                let (request, _) = workspace.request(path).unwrap();
                match slice(&request, &mut Silent, &|_| false) {
                    Ok(_) => break,
                    Err(error) => {
                        let message = Message::failed_slice(error.reason(), error.remedies());
                        let fix = *message
                            .fixes
                            .first()
                            .unwrap_or_else(|| panic!("{name}: no fix for {}", error.reason()));
                        assert!(attempt < 2, "{name}: still failing after {fixed:?}");
                        fix.apply(&mut workspace);
                        fixed.push(fix);
                    }
                }
            }
            // Only the fin needs more than the repairs on import.
            if name == "fx-10b-non-manifold.stl" {
                assert_eq!(
                    fixed,
                    [crate::messages::Fix::Repair(RepairPreset::Standard)],
                    "{name}"
                );
            } else {
                assert!(fixed.is_empty(), "{name}: {fixed:?}");
            }
        }
        std::fs::remove_dir_all(&output).ok();
    }
}
