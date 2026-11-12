//! The plate's layer heights in the interface (M6.5, TD-102): ranges of adaptive or fixed layers
//! set from the section bar, their window, and the planned layers the section bar snaps to.
//!
//! **The plan** is the plate's `karasu-layers` document (TD-064), a plate edit like any other:
//! undoable, and a slice made before it is out of date. Its first range starts at the plate with
//! the resin's calibrated height; the ranges the user adds start above the bottom and transition
//! layers, each until the next one or the top. The exposure table is kept with the plate for now
//! (provisional, until the resin profiles hold it). Without any range the plate has no plan and
//! slices with the calibrated height.
//!
//! **The planned layers** come from the core's planner, as slicing plans them: exactly for the
//! calibrated height and fixed ranges; for adaptive ranges from the models' slopes, read from
//! the imported models placed on the plate, while slicing reads the repaired ones, so the count
//! is then said to be approximate. Planning runs on a thread of its own, again only when the
//! plate changes; the last result stays in use meanwhile.
//!
//! **The window** checks a range before applying it and says why it cannot be applied; nothing
//! is corrected behind the user's back.
use crate::{
    i18n::{decimal, tr},
    messages::{Level, Message},
    theme,
    widgets::{fact_row, value_row, Unit, UnitField},
    workspace::Workspace,
};
use eframe::egui;
use karasu_core::{
    cancellation::Cancellation,
    layer_plan::{plan_layers, AdaptiveRule, HeightRange, LayerPlanDocument, RangeHeight},
    mesh::TriangleMesh,
    profiles::{PrintSettings, PrinterLimits},
};
use std::sync::{mpsc, Arc};

/// The plan's Z step, in nanometres: heights are typed in thousandths of a millimetre.
const STEP_NM: u64 = 1000;
/// Layers planned at most, as slicing allows.
const MAX_LAYERS: usize = 1_000_000;
/// Default adaptive rule of a new range: 0.02 to 0.1 mm, stair steps of 0.02 mm, growth 1.5.
const DEFAULT_ADAPTIVE: Kind = Kind::Adaptive {
    min_mm: 0.02,
    max_mm: 0.1,
    deviation_mm: 0.02,
    growth: 1.5,
};

/// Millimetres to the plan's nanometres, on the step.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Heights within a metre.
fn to_nm(mm: f64) -> u64 {
    ((mm.max(0.0) * 1e6 / 1e3).round() as u64) * STEP_NM
}

/// Nanometres to millimetres.
#[allow(clippy::cast_precision_loss)] // Within a metre.
fn to_mm(nm: u64) -> f64 {
    nm as f64 / 1e6
}

/// How a range chooses its layers' heights.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// From the models' slopes, between `min_mm` and `max_mm`.
    Adaptive {
        min_mm: f64,
        max_mm: f64,
        /// The largest stair step left on a slope.
        deviation_mm: f64,
        /// The largest ratio between a layer and the one below it.
        growth: f64,
    },
    /// One height, with its own exposure or the table's.
    Fixed {
        height_mm: f64,
        exposure_s: Option<f64>,
    },
}

/// A range of the plan the user set: from `from_mm` to the next range or the top.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Range {
    pub from_mm: f64,
    pub kind: Kind,
}

impl Range {
    /// Its colour on the section bar.
    #[must_use]
    pub fn colour(&self) -> egui::Color32 {
        match self.kind {
            Kind::Adaptive { .. } => theme::LAYERS_ADAPTIVE.color(),
            Kind::Fixed { .. } => theme::LAYERS_FIXED.color(),
        }
    }

    /// Its name in a menu: "adaptive layers" or "fixed layers".
    #[must_use]
    pub fn name(&self) -> String {
        match self.kind {
            Kind::Adaptive { .. } => tr!("layers-adaptive-name"),
            Kind::Fixed { .. } => tr!("layers-fixed-name"),
        }
    }

    /// What it does, in a line.
    #[must_use]
    pub fn describe(&self) -> String {
        let from = decimal(self.from_mm, 2);
        match self.kind {
            Kind::Adaptive { min_mm, max_mm, .. } => tr!(
                "layers-adaptive-from",
                min = decimal(min_mm, 3),
                max = decimal(max_mm, 3),
                from = from
            ),
            Kind::Fixed { height_mm, .. } => tr!(
                "layers-fixed-from",
                height = decimal(height_mm, 3),
                from = from
            ),
        }
    }
}

/// The calibration the plate slices with: its print setting's (TD-106).
#[must_use]
pub fn calibration(workspace: &Workspace) -> Option<PrintSettings> {
    workspace.plate().calibration().cloned()
}

/// The height the bottom and transition layers reach, which keep the calibrated height.
#[must_use]
pub fn base_mm(calibration: &PrintSettings) -> f64 {
    f64::from(calibration.bottom_layer_count + calibration.transition_layer_count)
        * calibration.layer_height_mm
}

/// The ranges of the plate's plan above its first, in order.
#[must_use]
pub fn ranges(workspace: &Workspace) -> Vec<Range> {
    let Some(document) = &workspace.plate().settings.layer_plan else {
        return Vec::new();
    };
    document
        .ranges
        .iter()
        .skip(1)
        .map(|r| Range {
            from_mm: to_mm(r.from_nm),
            kind: match r.height {
                RangeHeight::Adaptive(rule) => Kind::Adaptive {
                    min_mm: to_mm(rule.min_nm),
                    max_mm: to_mm(rule.max_nm),
                    deviation_mm: rule.max_deviation_mm,
                    growth: rule.max_growth_ratio,
                },
                RangeHeight::Fixed(nm) => Kind::Fixed {
                    height_mm: to_mm(nm),
                    exposure_s: r.exposure_s,
                },
            },
        })
        .collect()
}

/// The plan's exposure table, as (layer height mm, exposure s); without a plan, the calibrated
/// height's exposure, to start from.
#[must_use]
pub fn exposures(workspace: &Workspace) -> Vec<(f64, f64)> {
    match &workspace.plate().settings.layer_plan {
        Some(document) => document
            .exposures
            .iter()
            .map(|&(nm, s)| (to_mm(nm), s))
            .collect(),
        None => calibration(workspace)
            .map(|c| vec![(c.layer_height_mm, c.normal.exposure_s)])
            .unwrap_or_default(),
    }
}

/// The plan of `ranges` and `exposures` over a first range from the plate: the plan's own when
/// it has one, else the calibrated height `calibrated_mm`. `None` without ranges: the plate then
/// slices with the calibrated height.
#[must_use]
pub fn document(
    first: Option<HeightRange>,
    calibrated_mm: f64,
    ranges: &[Range],
    exposures: &[(f64, f64)],
) -> Option<LayerPlanDocument> {
    if ranges.is_empty() {
        return None;
    }
    let mut sorted = ranges.to_vec();
    sorted.sort_by(|a, b| a.from_mm.total_cmp(&b.from_mm));
    let first = first.unwrap_or(HeightRange {
        from_nm: 0,
        height: RangeHeight::Fixed(to_nm(calibrated_mm)),
        exposure_s: None,
    });
    let rest = sorted.iter().map(|r| HeightRange {
        from_nm: to_nm(r.from_mm),
        height: match r.kind {
            Kind::Adaptive {
                min_mm,
                max_mm,
                deviation_mm,
                growth,
            } => RangeHeight::Adaptive(AdaptiveRule {
                min_nm: to_nm(min_mm),
                max_nm: to_nm(max_mm),
                max_deviation_mm: deviation_mm,
                max_growth_ratio: growth,
            }),
            Kind::Fixed { height_mm, .. } => RangeHeight::Fixed(to_nm(height_mm)),
        },
        exposure_s: match r.kind {
            Kind::Fixed { exposure_s, .. } => exposure_s,
            Kind::Adaptive { .. } => None,
        },
    });
    Some(LayerPlanDocument {
        z_step_nm: STEP_NM,
        ranges: std::iter::once(first).chain(rest).collect(),
        exposures: exposures.iter().map(|&(h, s)| (to_nm(h), s)).collect(),
    })
}

/// Give the plate `ranges` and `exposures`, as one plate edit.
pub fn set(workspace: &mut Workspace, ranges: &[Range], exposures: &[(f64, f64)]) {
    let Some(calibration) = calibration(workspace) else {
        return;
    };
    let first = workspace
        .plate()
        .settings
        .layer_plan
        .as_ref()
        .and_then(|d| d.ranges.first().copied());
    let plan = document(first, calibration.layer_height_mm, ranges, exposures);
    workspace.edit(|plate| plate.settings.layer_plan = plan);
}

/// The plate's layer heights in a line: the calibrated height, then each range.
#[must_use]
pub fn summary(workspace: &Workspace) -> String {
    let Some(calibration) = calibration(workspace) else {
        return "–".to_owned();
    };
    let base = tr!(
        "prepare-length-mm",
        length = decimal(calibration.layer_height_mm, 3)
    );
    std::iter::once(base)
        .chain(ranges(workspace).iter().map(Range::describe))
        .collect::<Vec<_>>()
        .join("; ")
}

/// What is wrong with a range, which forbids applying it, and what deserves a warning.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Check {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// What the plate's printer and calibration ask of a range.
pub struct Bounds<'a> {
    pub calibration: &'a PrintSettings,
    pub limits: &'a PrinterLimits,
    /// The plate's file format.
    pub format: &'a str,
    /// The models' top, if any.
    pub top_mm: Option<f64>,
}

/// Whether `table` gives an exposure to every height from `low` to `high` (mm): two points
/// around them, as the table is never extrapolated.
fn covers(table: &[(f64, f64)], low: f64, high: f64) -> bool {
    match (table.first(), table.last()) {
        (Some(first), Some(last)) => to_nm(first.0) <= to_nm(low) && to_nm(last.0) >= to_nm(high),
        _ => false,
    }
}

/// Check `range` among the plate's `others` with the exposure `table`.
#[must_use]
pub fn check(range: &Range, others: &[Range], table: &[(f64, f64)], bounds: &Bounds<'_>) -> Check {
    let mut check = Check::default();
    let calibrated = bounds.calibration.layer_height_mm;
    let base = base_mm(bounds.calibration);
    if to_nm(range.from_mm) < to_nm(base).max(STEP_NM) {
        check
            .errors
            .push(tr!("layers-check-base", base = decimal(base, 2)));
    }
    if others
        .iter()
        .any(|o| to_nm(o.from_mm) == to_nm(range.from_mm))
    {
        check.errors.push(tr!("layers-check-same-start"));
    }
    let increasing = table.windows(2).all(|w| to_nm(w[0].0) < to_nm(w[1].0));
    let valid = table.iter().all(|&(h, s)| {
        to_nm(h) > 0 && to_nm(h) <= 1_000_000 && s > 0.0 && s <= bounds.limits.max_exposure_s
    });
    if !(increasing && valid) {
        check.errors.push(tr!(
            "layers-check-table",
            max = decimal(bounds.limits.max_exposure_s, 1)
        ));
    }
    let within = |h: f64| {
        to_nm(h) >= to_nm(bounds.limits.min_layer_height_mm)
            && to_nm(h) <= to_nm(bounds.limits.max_layer_height_mm)
    };
    let limits = || {
        tr!(
            "layers-check-limits",
            min = decimal(bounds.limits.min_layer_height_mm, 3),
            max = decimal(bounds.limits.max_layer_height_mm, 3)
        )
    };
    match range.kind {
        Kind::Adaptive { min_mm, max_mm, .. } => {
            if to_nm(min_mm) > to_nm(max_mm) {
                check.errors.push(tr!("layers-check-min-max"));
            } else if !(within(min_mm) && within(max_mm)) {
                check.errors.push(limits());
            } else if !covers(table, min_mm, max_mm) {
                check.errors.push(tr!(
                    "layers-check-cover",
                    min = decimal(min_mm, 3),
                    max = decimal(max_mm, 3)
                ));
            }
        }
        Kind::Fixed {
            height_mm,
            exposure_s,
        } => {
            if !within(height_mm) {
                check.errors.push(limits());
            } else if exposure_s.is_none()
                && to_nm(height_mm) != to_nm(calibrated)
                && !covers(table, height_mm, height_mm)
            {
                check.errors.push(tr!(
                    "layers-check-cover",
                    min = decimal(height_mm, 3),
                    max = decimal(height_mm, 3)
                ));
            }
        }
    }
    if bounds.format.starts_with("prusa-sl1") {
        check.warnings.push(tr!("layers-check-sl1"));
    }
    if bounds.top_mm.is_some_and(|top| range.from_mm >= top) {
        check.warnings.push(tr!("layers-check-above"));
    }
    check
}

/// The planned layers of the plate as they were last computed.
#[derive(Clone, Debug, Default)]
struct Planned {
    /// Layer boundaries from the plate up, in mm: 0, then each layer's top.
    boundaries: Arc<Vec<f64>>,
    /// Whether adaptive heights came from the imported models, not the repaired ones.
    approximate: bool,
    /// Why the plan cannot be used, if it cannot.
    error: Option<String>,
}

/// What a planning thread sends back: its revision and layer heights, or why it failed.
type Outcome = (u64, Result<Vec<u64>, String>);

/// The plate's planned layers, kept for the plate's revision and computed off the interface
/// thread when they read the models' slopes.
#[derive(Default)]
pub struct Planner {
    revision: Option<u64>,
    planned: Planned,
    pending: Option<mpsc::Receiver<Outcome>>,
}

/// The models placed on the plate, as one mesh in world millimetres.
fn placed_models(workspace: &mut Workspace) -> Result<TriangleMesh, String> {
    let placements: Vec<_> = workspace.placements().to_vec();
    let plate = workspace.plate();
    let (mut vertices, mut triangles) = (Vec::new(), Vec::new());
    for (object, placement) in plate.objects.iter().zip(&placements) {
        let (Some(loaded), Ok(placement)) = (workspace.meshes().get(&object.model), placement)
        else {
            continue;
        };
        let mesh = karasu_project::pipeline::placed_mesh(&loaded.mesh, &placement.transform)?;
        let base = u32::try_from(vertices.len()).map_err(|_| "too many vertices".to_owned())?;
        vertices.extend_from_slice(mesh.vertices());
        triangles.extend(mesh.triangles().iter().map(|t| t.map(|v| v + base)));
    }
    TriangleMesh::try_new(vertices, triangles).map_err(|e| e.to_string())
}

/// Heights in nanometres to boundaries in millimetres, from the plate up.
fn boundaries(heights_nm: &[u64]) -> Vec<f64> {
    let mut at = 0u64;
    std::iter::once(0.0)
        .chain(heights_nm.iter().map(|h| {
            at += h;
            to_mm(at)
        }))
        .collect()
}

impl Planner {
    /// Bring the planned layers up to the plate's revision. Returns whether a plan is still
    /// being computed, so the window repaints until it ends.
    pub fn update(&mut self, workspace: &mut Workspace) -> bool {
        if let Some(receiver) = &self.pending {
            match receiver.try_recv() {
                Ok((revision, outcome)) => {
                    self.pending = None;
                    if Some(revision) == self.revision {
                        self.planned = match outcome {
                            Ok(heights) => Planned {
                                boundaries: Arc::new(boundaries(&heights)),
                                approximate: true,
                                error: None,
                            },
                            Err(error) => Planned {
                                error: Some(error),
                                ..Planned::default()
                            },
                        };
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => self.pending = None,
            }
        }
        let revision = workspace.revision();
        if self.revision == Some(revision) {
            return self.pending.is_some();
        }
        self.revision = Some(revision);
        let top = workspace
            .placements()
            .iter()
            .flatten()
            .map(|p| p.max.z)
            .reduce(f64::max);
        let (Some(calibration), Some(top)) = (calibration(workspace), top) else {
            self.planned = Planned::default();
            return false;
        };
        let document = workspace
            .plate()
            .settings
            .layer_plan
            .clone()
            .unwrap_or_else(|| LayerPlanDocument {
                z_step_nm: STEP_NM,
                ranges: vec![HeightRange {
                    from_nm: 0,
                    height: RangeHeight::Fixed(to_nm(calibration.layer_height_mm)),
                    exposure_s: None,
                }],
                exposures: Vec::new(),
            });
        let plan = move |surface: Option<&TriangleMesh>| {
            plan_layers(
                &document,
                &calibration,
                surface,
                top,
                MAX_LAYERS,
                &mut Cancellation::never(),
            )
            .map(|p| p.heights_nm)
            .map_err(|e| e.to_string())
        };
        if !workspace
            .plate()
            .settings
            .layer_plan
            .as_ref()
            .is_some_and(LayerPlanDocument::is_adaptive)
        {
            // Without slopes to read, planning is a quick stack of heights.
            self.planned = match plan(None) {
                Ok(heights) => Planned {
                    boundaries: Arc::new(boundaries(&heights)),
                    approximate: false,
                    error: None,
                },
                Err(error) => Planned {
                    error: Some(error),
                    ..Planned::default()
                },
            };
            return false;
        }
        let surface = placed_models(workspace);
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let outcome = surface.and_then(|surface| plan(Some(&surface)));
            let _ = sender.send((revision, outcome));
        });
        self.pending = Some(receiver);
        true
    }

    /// The planned layer boundaries, 0 then each layer's top in mm; empty without a plan.
    #[must_use]
    pub fn boundaries(&self) -> Arc<Vec<f64>> {
        Arc::clone(&self.planned.boundaries)
    }

    /// The planned layers' count, and whether it is approximate; `None` without a plan.
    #[must_use]
    pub fn count(&self) -> Option<(usize, bool)> {
        let layers = self.planned.boundaries.len().checked_sub(1)?;
        (layers > 0).then_some((layers, self.planned.approximate))
    }

    /// Why the plate's plan cannot be used, if it cannot.
    #[must_use]
    pub fn error(&self) -> Option<&str> {
        self.planned.error.as_deref()
    }
}

/// The window that adds or edits a range.
pub struct RangeWindow {
    /// The range edited, as it was, or `None` for a new one.
    editing: Option<Range>,
    range: Range,
    table: Vec<(f64, f64)>,
}

impl RangeWindow {
    /// A window adding a range of `kind` from `from_mm`, adaptive when `adaptive`.
    #[must_use]
    pub fn add(workspace: &Workspace, from_mm: f64, adaptive: bool) -> Self {
        let calibrated = calibration(workspace).map_or(0.05, |c| c.layer_height_mm);
        let kind = if adaptive {
            DEFAULT_ADAPTIVE
        } else {
            Kind::Fixed {
                height_mm: calibrated,
                exposure_s: None,
            }
        };
        Self {
            editing: None,
            range: Range { from_mm, kind },
            table: exposures(workspace),
        }
    }

    /// A window editing `range`.
    #[must_use]
    pub fn edit(workspace: &Workspace, range: Range) -> Self {
        Self {
            editing: Some(range),
            range,
            table: exposures(workspace),
        }
    }

    /// Show the dialog (modal, fixed width), `boundaries` giving the layers its start snaps to.
    /// Returns whether it stays open, and a message when it applied the range.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        workspace: &mut Workspace,
        boundaries: &[f64],
    ) -> (bool, Option<Message>) {
        let mut close = false;
        let mut message = None;
        let title = match (self.editing.is_some(), self.range.kind) {
            (false, Kind::Adaptive { .. }) => tr!("layers-add-adaptive-title"),
            (false, Kind::Fixed { .. }) => tr!("layers-add-fixed-title"),
            (true, _) => tr!("layers-edit-title", what = self.range.name()),
        };
        let others: Vec<Range> = ranges(workspace)
            .into_iter()
            .filter(|r| Some(*r) != self.editing)
            .collect();
        // The checks of the range as it was last drawn: the footer needs them before the body.
        let bounds = calibration(workspace).zip(workspace.printer().cloned());
        let format = workspace.plate().settings.format.clone();
        let top = boundaries.last().copied();
        let check = bounds.as_ref().map(|(calibration, printer)| {
            check(
                &self.range,
                &others,
                &self.table,
                &Bounds {
                    calibration,
                    limits: &printer.limits,
                    format: &format,
                    top_mm: top,
                },
            )
        });
        let (mut apply, mut cancel) = (false, false);
        let response = crate::dialog::show(
            ctx,
            "layer-range",
            &title,
            crate::dialog::Size::Fixed(400.0),
            |ui, part| match part {
                crate::dialog::Part::Body => {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                    self.fields(ui, boundaries, &others);
                    let Some(check) = &check else {
                        return;
                    };
                    // Errors and warnings each with their icon, not their colour alone
                    // (TD-142).
                    let icons = crate::icons::Icons::shared(ui.ctx());
                    for (texts, colour, icon) in [
                        (
                            &check.errors,
                            theme::ERROR.color(),
                            crate::icons::Icon::Error,
                        ),
                        (
                            &check.warnings,
                            theme::WARNING.color(),
                            crate::icons::Icon::Warning,
                        ),
                    ] {
                        for text in texts {
                            crate::widgets::state_line(
                                ui,
                                icons.as_deref(),
                                Some(icon),
                                text,
                                colour,
                            );
                        }
                    }
                }
                crate::dialog::Part::Footer { enter } => {
                    let can = check.as_ref().is_some_and(|c| c.errors.is_empty());
                    apply = crate::dialog::main_action(ui, &tr!("layers-apply"), can, enter);
                    cancel = crate::dialog::cancel_action(ui);
                }
            },
        );
        if apply {
            let mut all = others.clone();
            all.push(self.range);
            set(workspace, &all, &self.table);
            message = Some(Message::new(
                Level::Success,
                tr!("layers-applied", what = self.range.describe()),
            ));
            close = true;
        }
        if cancel || response.cancel {
            close = true;
        }
        (!close, message)
    }

    /// The window's fields: the start, where the range ends, its heights and the exposures.
    fn fields(&mut self, ui: &mut egui::Ui, boundaries: &[f64], others: &[Range]) {
        let snap = |z: f64| nearest(boundaries, z).map_or(z, |k| boundaries[k]);
        crate::widgets::group(ui, tr!("layers-range"), |ui| {
            #[allow(clippy::cast_precision_loss)] // Layer numbers.
            let mut layer = nearest(boundaries, self.range.from_mm).unwrap_or(0) as f64;
            #[allow(clippy::cast_precision_loss)] // Layer numbers.
            let last = boundaries.len().saturating_sub(1) as f64;
            if value_row(ui, tr!("layers-start-layer"), |ui| {
                ui.add(
                    UnitField::new(&mut layer, Unit::Count)
                        .range(0.0..=last)
                        .step(1.0)
                        .decimals(0),
                )
            })
            .changed()
            {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Within range.
                if let Some(&z) = boundaries.get(layer.round() as usize) {
                    self.range.from_mm = z;
                }
            }
            let mut from = self.range.from_mm;
            if value_row(ui, tr!("layers-start-height"), |ui| {
                ui.add(
                    UnitField::new(&mut from, Unit::Millimetre)
                        .range(0.0..=1000.0)
                        .step(0.05),
                )
            })
            .changed()
            {
                self.range.from_mm = snap(from);
            }
            let until = others
                .iter()
                .map(|r| r.from_mm)
                .filter(|&z| z > self.range.from_mm)
                .reduce(f64::min)
                .map_or_else(
                    || tr!("layers-until-top"),
                    |z| tr!("layers-until-next", height = decimal(z, 2)),
                );
            fact_row(ui, &tr!("layers-until"), &until);
        });
        ui.add_space(4.0);
        self.heights(ui);
        ui.add_space(4.0);
        self.table(ui);
    }

    /// The range's heights: the adaptive rule, or the fixed height and its own exposure.
    fn heights(&mut self, ui: &mut egui::Ui) {
        let height = |ui: &mut egui::Ui, label: String, value: &mut f64, hint: String| {
            value_row(ui, label, |ui| {
                ui.add(
                    UnitField::new(value, Unit::Millimetre)
                        .range(0.001..=1.0)
                        .step(0.005)
                        .decimals(3),
                )
                .on_hover_text(hint)
            });
        };
        match &mut self.range.kind {
            Kind::Adaptive {
                min_mm,
                max_mm,
                deviation_mm,
                growth,
            } => {
                crate::widgets::group(ui, tr!("layers-adaptive-heights"), |ui| {
                    height(
                        ui,
                        tr!("layers-thinnest"),
                        min_mm,
                        tr!("layers-thinnest-hint"),
                    );
                    height(
                        ui,
                        tr!("layers-thickest"),
                        max_mm,
                        tr!("layers-thickest-hint"),
                    );
                    value_row(ui, tr!("layers-step"), |ui| {
                        ui.add(
                            UnitField::new(deviation_mm, Unit::Millimetre)
                                .range(0.001..=1.0)
                                .step(0.005)
                                .decimals(3),
                        )
                        .on_hover_text(tr!("layers-step-hint"))
                    });
                    value_row(ui, tr!("layers-growth"), |ui| {
                        ui.add(
                            UnitField::new(growth, Unit::Ratio)
                                .range(1.0..=4.0)
                                .step(0.1)
                                .decimals(2),
                        )
                        .on_hover_text(tr!("layers-growth-hint"))
                    });
                });
            }
            Kind::Fixed {
                height_mm,
                exposure_s,
            } => {
                crate::widgets::group(ui, tr!("layers-fixed-height"), |ui| {
                    height(
                        ui,
                        tr!("layers-height"),
                        height_mm,
                        tr!("layers-height-hint"),
                    );
                    let mut own = exposure_s.is_some();
                    let mut seconds = exposure_s.unwrap_or(2.0);
                    if crate::widgets::check_value_row(
                        ui,
                        &mut own,
                        tr!("layers-own-exposure"),
                        |ui| {
                            ui.add(
                                UnitField::new(&mut seconds, Unit::Second)
                                    .range(0.1..=300.0)
                                    .step(0.1)
                                    .decimals(1),
                            )
                        },
                    ) {
                        *exposure_s = own.then_some(seconds);
                    }
                });
            }
        }
    }

    /// The provisional exposure table: a row per height, a row added or removed.
    fn table(&mut self, ui: &mut egui::Ui) {
        crate::widgets::group(ui, tr!("layers-exposures"), |ui| {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(tr!("layers-exposures-note"))
                        .small()
                        .color(ui.visuals().weak_text_color()),
                )
                .wrap(),
            );
            let mut removed = None;
            for (k, (height, seconds)) in self.table.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.add(
                        UnitField::new(height, Unit::Millimetre)
                            .range(0.001..=1.0)
                            .step(0.005)
                            .decimals(3),
                    );
                    ui.add(
                        UnitField::new(seconds, Unit::Second)
                            .range(0.1..=300.0)
                            .step(0.1)
                            .decimals(1),
                    );
                    let icons = crate::icons::Icons::shared(ui.ctx());
                    if crate::widgets::icon_button(
                        ui,
                        icons.as_deref(),
                        crate::icons::Icon::Remove,
                        false,
                        &tr!("layers-remove-row"),
                    )
                    .clicked()
                    {
                        removed = Some(k);
                    }
                });
            }
            if let Some(k) = removed {
                self.table.remove(k);
            }
            if ui.button(tr!("layers-add-row")).clicked() {
                let next = self
                    .table
                    .last()
                    .map_or((0.05, 2.0), |&(h, s)| (h + 0.025, s));
                self.table.push(next);
            }
        });
    }
}

/// The position in `boundaries` of the one nearest `z`, if any.
#[must_use]
pub fn nearest(boundaries: &[f64], z: f64) -> Option<usize> {
    if boundaries.is_empty() {
        return None;
    }
    let after = boundaries.partition_point(|&b| b < z);
    Some(match after {
        0 => 0,
        k if k >= boundaries.len() => boundaries.len() - 1,
        k if (boundaries[k] - z) < (z - boundaries[k - 1]) => k,
        k => k - 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calibration() -> PrintSettings {
        crate::workspace::tests::catalog_profiles("anycubic-photon-mono-x")
            .2
            .settings
    }

    fn limits() -> PrinterLimits {
        PrinterLimits {
            min_layer_height_mm: 0.01,
            max_layer_height_mm: 0.2,
            max_exposure_s: 120.0,
            max_speed_mm_s: 20.0,
            ..crate::workspace::tests::catalog_profiles("anycubic-photon-mono-x")
                .0
                .limits
        }
    }

    #[test]
    fn ranges_make_a_document_the_core_accepts_and_read_back_the_same() {
        let c = calibration();
        let ranges = [
            Range {
                from_mm: 20.0,
                kind: DEFAULT_ADAPTIVE,
            },
            Range {
                from_mm: 5.0,
                kind: Kind::Fixed {
                    height_mm: 0.025,
                    exposure_s: Some(1.6),
                },
            },
        ];
        let table = [(0.02, 1.2), (0.05, 2.0), (0.1, 3.5)];
        assert_eq!(document(None, c.layer_height_mm, &[], &table), None);
        let doc = document(None, c.layer_height_mm, &ranges, &table).unwrap();
        // The first range is the calibration's, from the plate; the others in order.
        assert_eq!(doc.ranges[0].from_nm, 0);
        assert_eq!(
            doc.ranges[0].height,
            RangeHeight::Fixed(to_nm(c.layer_height_mm))
        );
        assert_eq!(doc.ranges[1].from_nm, 5_000_000);
        assert_eq!(doc.ranges[2].from_nm, 20_000_000);
        // The core plans it, for a 30 mm tall block without slopes.
        let flat = TriangleMesh::try_new(Vec::new(), Vec::new()).unwrap();
        let planned = plan_layers(
            &doc,
            &c,
            Some(&flat),
            30.0,
            MAX_LAYERS,
            &mut Cancellation::never(),
        )
        .unwrap();
        let tops = boundaries(&planned.heights_nm);
        assert!(tops.windows(2).all(|w| w[1] > w[0]));
        assert!(
            (tops.last().unwrap() - 30.0).abs() < 0.1,
            "{:?}",
            tops.last()
        );
        // Between 5 and 20 mm the layers are 0.025 mm thick.
        let k = nearest(&tops, 10.0).unwrap();
        assert!((tops[k + 1] - tops[k] - 0.025).abs() < 1e-9);
    }

    #[test]
    fn a_range_is_checked_before_it_is_applied() {
        let c = calibration();
        let l = limits();
        let bounds = Bounds {
            calibration: &c,
            limits: &l,
            format: "goo",
            top_mm: Some(30.0),
        };
        let table = [(0.02, 1.2), (0.05, 2.0), (0.1, 3.5)];
        let good = Range {
            from_mm: 20.0,
            kind: DEFAULT_ADAPTIVE,
        };
        assert_eq!(check(&good, &[], &table, &bounds), Check::default());
        // Within the bottom and transition layers.
        let low = Range {
            from_mm: base_mm(&c) / 2.0,
            ..good
        };
        assert_eq!(check(&low, &[], &table, &bounds).errors.len(), 1);
        // Another range starts there.
        assert_eq!(check(&good, &[good], &table, &bounds).errors.len(), 1);
        // The table does not reach 0.1 mm.
        let short = [(0.02, 1.2), (0.05, 2.0)];
        assert_eq!(
            check(&good, &[], &short, &bounds).errors,
            [tr!(
                "layers-check-cover",
                min = decimal(0.02, 3),
                max = decimal(0.1, 3)
            )]
        );
        // A fixed height with its own exposure needs no table; outside the printer it is refused.
        let fixed = Range {
            from_mm: 5.0,
            kind: Kind::Fixed {
                height_mm: 0.025,
                exposure_s: Some(1.6),
            },
        };
        assert_eq!(
            check(&fixed, &[], &[], &bounds).errors,
            Vec::<String>::new()
        );
        let thick = Range {
            kind: Kind::Fixed {
                height_mm: 0.5,
                exposure_s: Some(1.6),
            },
            ..fixed
        };
        assert_eq!(check(&thick, &[], &[], &bounds).errors.len(), 1);
        // SL1 stores one height: a warning, and a range above the models another.
        let sl1 = Bounds {
            format: "prusa-sl1",
            top_mm: Some(10.0),
            ..bounds
        };
        assert_eq!(check(&good, &[], &table, &sl1).warnings.len(), 2);
    }

    #[test]
    fn a_plan_set_on_the_plate_is_planned_and_summarised() {
        let folder = std::env::temp_dir().join(format!("karasu-plan-{}", std::process::id()));
        let mut workspace = crate::slicing_ui::tests::hollowed_cube(&folder);
        let c = super::calibration(&workspace).unwrap();
        let mut planner = Planner::default();
        // Without a plan: the calibrated height, exactly.
        assert!(!planner.update(&mut workspace));
        let (uniform, about) = planner.count().unwrap();
        assert!(!about);
        assert_eq!(
            summary(&workspace),
            tr!("prepare-length-mm", length = decimal(c.layer_height_mm, 3))
        );
        // A fixed range of thinner layers with its own exposure: more layers, still exact.
        let fixed = Range {
            from_mm: (base_mm(&c) + 1.0).max(2.0),
            kind: Kind::Fixed {
                height_mm: 0.025,
                exposure_s: Some(1.6),
            },
        };
        set(&mut workspace, &[fixed], &[]);
        assert_eq!(ranges(&workspace), [fixed]);
        assert!(!planner.update(&mut workspace));
        assert_eq!(planner.error(), None);
        let (thinner, about) = planner.count().unwrap();
        assert!(thinner > uniform && !about, "{thinner} {uniform}");
        assert!(summary(&workspace).ends_with(&fixed.describe()));
        // An adaptive range is planned on a thread of its own, and said to be approximate.
        let adaptive = Range {
            from_mm: fixed.from_mm,
            kind: DEFAULT_ADAPTIVE,
        };
        set(&mut workspace, &[adaptive], &[(0.02, 1.2), (0.1, 3.5)]);
        let started = std::time::Instant::now();
        while planner.update(&mut workspace) {
            assert!(started.elapsed().as_secs() < 60, "no plan");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(planner.error(), None);
        assert!(planner.count().unwrap().1);
        let boundaries = planner.boundaries();
        assert!(boundaries.windows(2).all(|w| w[1] > w[0]));
        // Without ranges the plate has no plan again.
        set(&mut workspace, &[], &[]);
        assert_eq!(workspace.plate().settings.layer_plan, None);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_nearest_boundary_is_found() {
        let b = [0.0, 0.05, 0.1, 0.2];
        assert_eq!(nearest(&b, -1.0), Some(0));
        assert_eq!(nearest(&b, 0.06), Some(1));
        assert_eq!(nearest(&b, 0.16), Some(3));
        assert_eq!(nearest(&b, 9.0), Some(3));
        assert_eq!(nearest(&[], 1.0), None);
    }
}
