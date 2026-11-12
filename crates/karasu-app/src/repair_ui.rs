//! The plate's repairs (M6.3): the plate's source repairs and layer closure. Since M6.5
//! (TD-098) they are the Slice panel's slicing options, shown with its Advanced switch, rather
//! than a menu, so changing them no longer closes a menu under the pointer.
//!
//! Every switch maps to one plate setting the pipeline already has ([`PlateSettings::repair`],
//! `close_holes_mm`, `reversed_tolerance_mm2`, `allow_topology_defects`), so the menu and the
//! command line repair models the same way. Changing a setting advances the plate's revision: a
//! slice made before it is out of date. Repairs apply to a derived mesh while slicing; the model
//! in the project is never changed. Each setting changed repeatedly is one undo step (TD-081).
use crate::{
    i18n::tr,
    widgets::{check_value_row, Unit, UnitField},
    workspace::Workspace,
};
use eframe::egui;
use karasu_core::preparation::repair::FillHoles;
use karasu_project::{
    project::PlateSettings,
    request::{DEFAULT_CLOSE_HOLES_MM, DEFAULT_REVERSED_TOLERANCE_MM2, MAX_REVERSED_TOLERANCE_MM2},
};

/// Smallest and largest gap closed in each layer, in mm.
const CLOSE_HOLES_RANGE: std::ops::RangeInclusive<f64> = 0.05..=20.0;
/// Smallest reversed-sliver tolerance offered, in mm² per layer.
const MIN_REVERSED_TOLERANCE_MM2: f64 = 0.001;

/// The plate's slicing repairs and tolerances, for the Slice panel.
pub fn plate_settings(ui: &mut egui::Ui, workspace: &mut Workspace) {
    let mut settings = workspace.plate().settings.clone();
    edit(ui, &mut settings);
    let before = &workspace.plate().settings;
    if settings != *before {
        let key = merge_key(before, &settings);
        workspace.edit_merging(key, |plate| plate.settings = settings);
    }
}

/// Undo merge keys of the menu's settings, apart from the drag keys (small counters).
const MERGE_KEYS: u64 = 0x5245_5041_4952_0000;

/// One undo step per setting: consecutive changes of the same setting (a dragged value)
/// merge, and a change of another setting starts a new step.
fn merge_key(before: &PlateSettings, after: &PlateSettings) -> u64 {
    let (a, b) = (&before.repair, &after.repair);
    let changed = [
        a.duplicates != b.duplicates,
        a.orientation != b.orientation || a.outward != b.outward,
        a.fill_holes != b.fill_holes,
        before.close_holes_mm != after.close_holes_mm,
        before.reversed_tolerance_mm2 != after.reversed_tolerance_mm2,
        before.allow_topology_defects != after.allow_topology_defects,
    ];
    let field = changed.iter().position(|&c| c).unwrap_or(changed.len());
    MERGE_KEYS + field as u64
}

fn edit(ui: &mut egui::Ui, settings: &mut PlateSettings) {
    let repair = &mut settings.repair;
    crate::widgets::group(ui, tr!("repair-before-slicing"), |ui| {
        ui.checkbox(&mut repair.duplicates, tr!("repair-duplicates"));
        if ui
            .checkbox(&mut repair.orientation, tr!("repair-orientation"))
            .changed()
            && !repair.orientation
        {
            // Outward orientation needs consistent orientation.
            repair.outward = false;
        }
        if ui
            .checkbox(&mut repair.outward, tr!("repair-outward"))
            .on_hover_text(tr!("repair-outward-hint"))
            .changed()
            && repair.outward
        {
            repair.orientation = true;
        }
        let mut fill = repair.fill_holes.is_some();
        if ui
            .checkbox(&mut fill, tr!("repair-fill-holes"))
            .on_hover_text(tr!(
                "repair-fill-holes-hint",
                edges = FillHoles::default().max_hole_edges
            ))
            .changed()
        {
            repair.fill_holes = fill.then(FillHoles::default);
        }
    });
    crate::widgets::group(ui, tr!("repair-slicing"), |ui| {
        // Rows that wrap within the panel (design rule, TD-098): the value under its setting.
        let mut close = settings.close_holes_mm.is_some();
        let mut gap = settings.close_holes_mm.unwrap_or(DEFAULT_CLOSE_HOLES_MM);
        if check_value_row(ui, &mut close, tr!("repair-close-holes-up-to"), |ui| {
            ui.add(
                UnitField::new(&mut gap, Unit::Millimetre)
                    .range(CLOSE_HOLES_RANGE)
                    .step(0.05),
            )
        }) {
            settings.close_holes_mm = close.then_some(gap);
        }
        let mut tolerate = settings.reversed_tolerance_mm2.is_some();
        let mut area = settings
            .reversed_tolerance_mm2
            .unwrap_or(DEFAULT_REVERSED_TOLERANCE_MM2);
        let label = format!(
            "{} ({})",
            tr!("repair-slivers-up-to"),
            tr!("repair-per-layer-mm2")
        );
        let changed = check_value_row(ui, &mut tolerate, label, |ui| {
            ui.add(
                UnitField::new(&mut area, Unit::SquareMillimetre)
                    .range(MIN_REVERSED_TOLERANCE_MM2..=MAX_REVERSED_TOLERANCE_MM2)
                    .step(0.01)
                    .decimals(3),
            )
            .on_hover_text(tr!("repair-slivers-hint"))
        });
        if changed {
            settings.reversed_tolerance_mm2 = tolerate.then_some(area);
        }
        ui.checkbox(
            &mut settings.allow_topology_defects,
            tr!("repair-allow-defects"),
        )
        .on_hover_text(tr!("repair-allow-defects-hint"));
    });
}
