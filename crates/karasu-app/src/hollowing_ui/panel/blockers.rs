//! The Blockers section of the Hollow panel (owner mockup "Hollowing blockers", 2026-11-02):
//! what new blockers take, Solid base up to, and the selected blocker's values.
use super::HollowingUi;
use crate::{
    hollowing_ui::blockers::{NewBlocker, BASE_MM, SIZE_MM, TOP_MM},
    i18n::tr,
    widgets::{
        default_choice, default_row, length_default, value_row, DefaultValue, Unit, UnitField,
    },
    workspace::Workspace,
};
use eframe::egui;
use karasu_project::request::Blocker;

/// A row for a length in `range`: with a reset button to `default` when given.
fn length_row(
    ui: &mut egui::Ui,
    label: String,
    value: &mut f64,
    default: Option<f64>,
    range: std::ops::RangeInclusive<f64>,
) -> bool {
    let default = default.map(|d| length_default(d, 2, Unit::Millimetre));
    default_row(ui, label, value, default.as_ref(), |ui, value| {
        ui.add(
            UnitField::new(value, Unit::Millimetre)
                .range(range)
                .step(0.1),
        )
    })
}

/// The rows of a box's sizes: its bases and height, each reset to `defaults` when given.
fn box_rows(
    ui: &mut egui::Ui,
    bottom: &mut [f64; 2],
    top: &mut [f64; 2],
    height: &mut f64,
    defaults: Option<&NewBlocker>,
) -> bool {
    let mut changed = false;
    let rows = [
        (tr!("blockers-bottom-width"), 0, false),
        (tr!("blockers-bottom-depth"), 1, false),
        (tr!("blockers-top-width"), 0, true),
        (tr!("blockers-top-depth"), 1, true),
    ];
    for (label, k, upper) in rows {
        let (value, default, range) = if upper {
            (&mut top[k], defaults.map(|d| d.top_mm[k]), TOP_MM)
        } else {
            (&mut bottom[k], defaults.map(|d| d.bottom_mm[k]), SIZE_MM)
        };
        changed |= length_row(ui, label, value, default, range);
    }
    changed |= length_row(
        ui,
        tr!("blockers-height"),
        height,
        defaults.map(|d| d.height_mm),
        SIZE_MM,
    );
    changed
}

impl HollowingUi {
    /// The Blockers section of model `index`.
    pub(super) fn blockers_section(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &mut Workspace,
        index: Option<usize>,
    ) {
        let defaults = self.tool_defaults.blocker;
        ui.add_enabled_ui(index.is_some(), |ui| {
            crate::widgets::group_with_hint(
                ui,
                tr!("blockers-next"),
                tr!("blockers-next-hint"),
                |ui| {
                    let next = &mut self.blockers.next;
                    let mut chosen = usize::from(next.sphere);
                    let shown = DefaultValue::new(
                        usize::from(defaults.sphere),
                        if defaults.sphere {
                            tr!("blockers-sphere")
                        } else {
                            tr!("blockers-box")
                        },
                    );
                    let labels = [tr!("blockers-box"), tr!("blockers-sphere")];
                    if default_choice(ui, &mut chosen, &labels, Some(&shown)).changed() {
                        next.sphere = chosen == 1;
                    }
                    if next.sphere {
                        length_row(
                            ui,
                            tr!("blockers-diameter"),
                            &mut next.diameter_mm,
                            Some(defaults.diameter_mm),
                            SIZE_MM,
                        );
                    } else {
                        box_rows(
                            ui,
                            &mut next.bottom_mm,
                            &mut next.top_mm,
                            &mut next.height_mm,
                            Some(&defaults),
                        );
                    }
                    ui.add_space(4.0);
                    length_row(
                        ui,
                        tr!("blockers-base"),
                        &mut next.base_mm,
                        Some(defaults.base_mm),
                        BASE_MM,
                    );
                    let add = ui
                        .add_sized(
                            [ui.available_width(), ui.spacing().interact_size.y],
                            egui::Button::new(tr!("blockers-add-base")).truncate(),
                        )
                        .on_hover_text(tr!("blockers-add-base-hint"));
                    if let (true, Some(index)) = (add.clicked(), index) {
                        self.blockers.add_solid_base(workspace, index);
                    }
                },
            );
        });
        if let Some(index) = index {
            self.selected_blocker(ui, workspace, index);
        }
    }

    /// The selected blocker's values, in its model's frame: its centre, a box's turn and sizes,
    /// a sphere's diameter.
    fn selected_blocker(&mut self, ui: &mut egui::Ui, workspace: &Workspace, index: usize) {
        let Some(selected) = self.blockers.selected else {
            return;
        };
        let Some(mut blocker) = self
            .blockers
            .draft(workspace, index)
            .edit
            .get(selected)
            .copied()
        else {
            return;
        };
        let changed = crate::widgets::group_with_hint(
            ui,
            tr!("blockers-selected", number = selected + 1),
            tr!("blockers-selected-hint"),
            |ui| {
                let mut changed = false;
                let axes = ["X", "Y", "Z"];
                match &mut blocker {
                    Blocker::Sphere {
                        centre_mm,
                        diameter_mm,
                    } => {
                        for (k, axis) in axes.iter().enumerate() {
                            changed |= centre_row(ui, axis, &mut centre_mm[k]);
                        }
                        changed |=
                            length_row(ui, tr!("blockers-diameter"), diameter_mm, None, SIZE_MM);
                    }
                    Blocker::Frustum {
                        centre_mm,
                        rotation_deg,
                        bottom_mm,
                        top_mm,
                        height_mm,
                    } => {
                        for (k, axis) in axes.iter().enumerate() {
                            changed |= centre_row(ui, axis, &mut centre_mm[k]);
                        }
                        for (k, axis) in axes.iter().enumerate() {
                            changed |= value_row(ui, tr!("blockers-turn", axis = *axis), |ui| {
                                ui.add(
                                    UnitField::new(&mut rotation_deg[k], Unit::Degree)
                                        .range(-360.0..=360.0)
                                        .step(1.0),
                                )
                            })
                            .changed();
                        }
                        changed |= box_rows(ui, bottom_mm, top_mm, height_mm, None);
                    }
                }
                changed
            },
        );
        if changed {
            self.blockers.set_selected(workspace, index, blocker);
        }
    }
}

/// A row for the centre's coordinate along `axis`, in the model frame.
fn centre_row(ui: &mut egui::Ui, axis: &str, value: &mut f64) -> bool {
    value_row(ui, tr!("blockers-centre", axis = axis), |ui| {
        ui.add(
            UnitField::new(value, Unit::Millimetre)
                .range(-10_000.0..=10_000.0)
                .step(0.1),
        )
    })
    .changed()
}
