//! The Supports tool's panel (TD-108): the next support's values, the selected model's
//! supports with each one's values, the document's checks, and the footer's state.
//!
//! The panel follows the tool panel rules (`docs/UI-DESIGN.md`, "Tool panels"): the body
//! scrolls; the footer holds the model's state only, supports being applied at once (owner,
//! 2026-10-27). Four values are always in view (contact, tip length, shaft, penetration); the
//! rest, and the checks, are under Advanced. A mini support (TD-113) has two: its diameter and
//! its penetration, from zero.
use super::{
    describe, errors, Candidate, NextSupport, SupportPreset, SupportValues, SupportsUi, Weight,
    MERGE_KEYS,
};
use crate::{
    i18n::tr,
    icons::{Icon, Icons},
    preparation_ui::PreparationUi,
    theme,
    widgets::{
        action_pair, auto_default, choice_among, default_auto_row, default_choice, default_row,
        default_switch, fact_row, icon_button, length_default, reset_button, state_line, switch,
        switch_default, text_field, value_row, with_unit, DefaultValue, Unit, UnitField,
    },
    workspace::Workspace,
};
use eframe::egui;
use karasu_core::supports::{
    AxisSpec, ContactShape, FindingKind, ManualSupport, Severity, SupportKind, SupportParameters,
};
use std::ops::RangeInclusive;

/// Undo merge keys of the document's checks, apart from the supports' own.
const CHECK_KEYS: u64 = MERGE_KEYS | 0xFF_0000_0000;

/// A value in millimetres in `range`, with its hint, and a reset to `default` when given.
/// Returns whether it changed.
fn millimetres(
    ui: &mut egui::Ui,
    label: String,
    hint: String,
    value: &mut f64,
    range: RangeInclusive<f64>,
    default: Option<f64>,
) -> bool {
    let default = default.map(|d| length_default(d, 2, Unit::Millimetre));
    default_row(ui, label, value, default.as_ref(), |ui, value| {
        ui.add(
            UnitField::new(value, Unit::Millimetre)
                .range(range)
                .step(0.05),
        )
        .on_hover_text(hint)
    })
}

/// The smallest diameter a contact fitting the surface narrows to when it is switched on, in mm:
/// the least contact the core recommends.
pub(crate) const FIT_MIN_MM: f64 = 0.2;

/// Whether the contact fits the surface (TD-116; owner, 2026-11-02), and the smallest diameter
/// it narrows to, at most `contact`: a switch, then the diameter while it is on. With `default`,
/// each one's reset button. Shared by the Supports, Islands and Automatic panels. Returns
/// whether either changed.
#[allow(clippy::option_option)] // No default (no reset buttons), or a default that may be off.
pub(crate) fn fit_fields(
    ui: &mut egui::Ui,
    least: &mut Option<f64>,
    contact: f64,
    default: Option<Option<f64>>,
) -> bool {
    let mut on = least.is_some();
    let mut changed = default_switch(
        ui,
        &mut on,
        (
            tr!("supports-contact-fit"),
            tr!("supports-contact-fit-hint"),
        ),
        default.map(|d| switch_default(d.is_some())).as_ref(),
    );
    if changed {
        *least = on.then(|| default.flatten().unwrap_or(FIT_MIN_MM).min(contact));
    }
    let on = least.is_some();
    crate::widgets::depends(ui, on, |ui| {
        if let Some(value) = least.as_mut() {
            changed |= millimetres(
                ui,
                tr!("supports-contact-min"),
                tr!("supports-contact-min-hint"),
                value,
                0.05..=contact.max(0.05),
                default.flatten(),
            );
        }
    });
    changed
}

/// How the contact ends on the model, a cone or a ball (`sphere`), with its label above; with
/// `default`, its reset button. Shared by the Supports, Islands and Automatic panels. Returns
/// whether it changed.
pub(crate) fn shape_field(ui: &mut egui::Ui, sphere: &mut bool, default: Option<bool>) -> bool {
    ui.label(
        egui::RichText::new(tr!("supports-contact-shape")).color(ui.visuals().weak_text_color()),
    );
    let mut shape = usize::from(*sphere);
    let labels = [tr!("supports-cone"), tr!("supports-sphere")];
    let default =
        default.map(|d| DefaultValue::new(usize::from(d), labels[usize::from(d)].clone()));
    let changed = default_choice(ui, &mut shape, &labels, default.as_ref())
        .on_hover_text(tr!("supports-contact-shape-hint"))
        .changed();
    if changed {
        *sphere = shape == 1;
    }
    changed
}

/// Keep the couplings the core requires (TD-062) after a value changed: the contact is at most
/// as wide as the tip where it joins the shaft, the smallest a fitted contact narrows to at most
/// the contact, a contact ball at least as wide as the contact,
/// the cone at most as wide as the pad, and a foot's contact at most as wide as its body. The
/// fields' ranges follow the same couplings, so this only narrows a value another just
/// narrowed.
pub(super) fn couple(p: &mut SupportParameters) {
    p.contact_diameter_mm = p.contact_diameter_mm.min(p.tip_body_diameter_mm);
    p.contact_fit_min_mm = p.contact_fit_min_mm.map(|m| m.min(p.contact_diameter_mm));
    p.contact_sphere_diameter_mm = p.contact_sphere_diameter_mm.max(p.contact_diameter_mm);
    p.cone_diameter_mm = p.cone_diameter_mm.min(p.pad_diameter_mm);
    p.foot_contact_diameter_mm = p.foot_contact_diameter_mm.min(p.foot_body_diameter_mm);
    p.mini_penetration_mm = p.mini_penetration_mm.min(deepest_mini(p.mini_diameter_mm));
}

/// The deepest a mini support of `diameter` reaches below the surface: less than its radius
/// (the core's coupling), by a hundredth of a millimetre.
fn deepest_mini(diameter: f64) -> f64 {
    (diameter / 2.0 - 0.01).max(0.0)
}

/// A mini support's values (owner, 2026-10-30): its diameter and how deep its ends reach, from
/// zero, where they touch the surface. Returns whether either changed.
fn mini_fields(
    ui: &mut egui::Ui,
    p: &mut SupportParameters,
    defaults: Option<&SupportParameters>,
) -> bool {
    let mut changed = millimetres(
        ui,
        tr!("supports-mini-diameter"),
        tr!("supports-mini-diameter-hint"),
        &mut p.mini_diameter_mm,
        0.1..=2.0,
        defaults.map(|d| d.mini_diameter_mm),
    );
    changed |= millimetres(
        ui,
        tr!("supports-mini-penetration"),
        tr!("supports-mini-penetration-hint"),
        &mut p.mini_penetration_mm,
        0.0..=deepest_mini(p.mini_diameter_mm),
        defaults.map(|d| d.mini_penetration_mm),
    );
    if changed {
        couple(p);
    }
    changed
}

/// A support's values: the four in view and the contact's width at the surface, then, with
/// `advanced`, every other one. A shaft changed in the basic view takes the tip's body with it
/// where they were alike (Light's and Medium's are; Heavy's tip is the slimmer, TD-136). With
/// `defaults` (the next support's, its preset's values), each field's reset button. Returns
/// whether any changed.
fn parameter_fields(
    ui: &mut egui::Ui,
    p: &mut SupportParameters,
    advanced: bool,
    defaults: Option<&SupportParameters>,
) -> bool {
    let shaft_before = p.shaft_diameter_mm;
    let mut changed = millimetres(
        ui,
        tr!("supports-contact"),
        tr!("supports-contact-hint"),
        &mut p.contact_diameter_mm,
        0.05..=p.tip_body_diameter_mm.clamp(0.05, 5.0),
        defaults.map(|d| d.contact_diameter_mm),
    );
    p.contact_fit_min_mm = p.contact_fit_min_mm.map(|m| m.min(p.contact_diameter_mm));
    changed |= fit_fields(
        ui,
        &mut p.contact_fit_min_mm,
        p.contact_diameter_mm,
        defaults.map(|d| d.contact_fit_min_mm),
    );
    changed |= millimetres(
        ui,
        tr!("supports-tip-length"),
        tr!("supports-tip-length-hint"),
        &mut p.tip_length_mm,
        0.2..=30.0,
        defaults.map(|d| d.tip_length_mm),
    );
    changed |= millimetres(
        ui,
        tr!("supports-shaft"),
        tr!("supports-shaft-hint"),
        &mut p.shaft_diameter_mm,
        0.1..=20.0,
        defaults.map(|d| d.shaft_diameter_mm),
    );
    changed |= millimetres(
        ui,
        tr!("supports-penetration"),
        tr!("supports-penetration-hint"),
        &mut p.penetration_mm,
        0.0..=5.0,
        defaults.map(|d| d.penetration_mm),
    );
    fact_row(
        ui,
        &tr!("supports-surface-width"),
        &with_unit(p.surface_width_mm(), 2, Unit::Millimetre),
    )
    .on_hover_text(tr!("supports-surface-width-hint"));
    #[allow(clippy::float_cmp)] // The values the fields held before: changed or not, alike or not.
    if !advanced && p.shaft_diameter_mm != shaft_before && p.tip_body_diameter_mm == shaft_before {
        p.tip_body_diameter_mm = p.shaft_diameter_mm.clamp(0.1, 10.0);
    }
    if advanced {
        changed |= advanced_fields(ui, p, defaults);
    }
    if changed {
        couple(p);
    }
    changed
}

/// The values under Advanced: the contact's shape and ball, the tip's body, a tapered shaft, the
/// pad and its cone, a foot on the model, and the sides. Returns whether any changed.
#[allow(clippy::too_many_lines)] // One row per value, in the panel's order.
fn advanced_fields(
    ui: &mut egui::Ui,
    p: &mut SupportParameters,
    defaults: Option<&SupportParameters>,
) -> bool {
    let mut changed = false;
    let mut sphere = p.contact_shape == ContactShape::Sphere;
    if shape_field(
        ui,
        &mut sphere,
        defaults.map(|d| d.contact_shape == ContactShape::Sphere),
    ) {
        p.contact_shape = if sphere {
            ContactShape::Sphere
        } else {
            ContactShape::Cone
        };
        changed = true;
    }
    if p.contact_shape == ContactShape::Sphere {
        changed |= millimetres(
            ui,
            tr!("supports-sphere-diameter"),
            tr!("supports-sphere-diameter-hint"),
            &mut p.contact_sphere_diameter_mm,
            p.contact_diameter_mm.max(0.05)..=5.0,
            defaults.map(|d| d.contact_sphere_diameter_mm),
        );
    }
    changed |= millimetres(
        ui,
        tr!("supports-tip-body"),
        tr!("supports-tip-body-hint"),
        &mut p.tip_body_diameter_mm,
        0.1..=10.0,
        defaults.map(|d| d.tip_body_diameter_mm),
    );
    let default = defaults.map(|d| auto_default(d.shaft_bottom_diameter_mm, Unit::Millimetre));
    let automatic = p.shaft_diameter_mm;
    changed |= default_auto_row(
        ui,
        tr!("supports-shaft-bottom"),
        &mut p.shaft_bottom_diameter_mm,
        automatic,
        default.as_ref(),
        |ui, bottom| {
            ui.add(
                UnitField::new(bottom, Unit::Millimetre)
                    .range(0.1..=20.0)
                    .step(0.05),
            )
            .on_hover_text(tr!("supports-shaft-bottom-hint"))
        },
    );
    changed |= millimetres(
        ui,
        tr!("supports-pad-diameter"),
        tr!("supports-pad-diameter-hint"),
        &mut p.pad_diameter_mm,
        0.5..=50.0,
        defaults.map(|d| d.pad_diameter_mm),
    );
    changed |= millimetres(
        ui,
        tr!("supports-pad-thickness"),
        tr!("supports-pad-thickness-hint"),
        &mut p.pad_thickness_mm,
        0.1..=5.0,
        defaults.map(|d| d.pad_thickness_mm),
    );
    changed |= millimetres(
        ui,
        tr!("supports-cone-diameter"),
        tr!("supports-cone-diameter-hint"),
        &mut p.cone_diameter_mm,
        0.1..=p.pad_diameter_mm.clamp(0.1, 50.0),
        defaults.map(|d| d.cone_diameter_mm),
    );
    changed |= millimetres(
        ui,
        tr!("supports-cone-height"),
        tr!("supports-cone-height-hint"),
        &mut p.cone_height_mm,
        0.0..=20.0,
        defaults.map(|d| d.cone_height_mm),
    );
    changed |= millimetres(
        ui,
        tr!("supports-foot-contact"),
        tr!("supports-foot-contact-hint"),
        &mut p.foot_contact_diameter_mm,
        0.05..=p.foot_body_diameter_mm.clamp(0.05, 5.0),
        defaults.map(|d| d.foot_contact_diameter_mm),
    );
    changed |= millimetres(
        ui,
        tr!("supports-foot-body"),
        tr!("supports-foot-body-hint"),
        &mut p.foot_body_diameter_mm,
        0.1..=10.0,
        defaults.map(|d| d.foot_body_diameter_mm),
    );
    changed |= millimetres(
        ui,
        tr!("supports-foot-length"),
        tr!("supports-foot-length-hint"),
        &mut p.foot_length_mm,
        0.2..=30.0,
        defaults.map(|d| d.foot_length_mm),
    );
    changed |= millimetres(
        ui,
        tr!("supports-foot-penetration"),
        tr!("supports-foot-penetration-hint"),
        &mut p.foot_penetration_mm,
        0.0..=5.0,
        defaults.map(|d| d.foot_penetration_mm),
    );
    let mut sides = f64::from(p.sides);
    let default = defaults.map(|d| DefaultValue::new(f64::from(d.sides), d.sides.to_string()));
    if default_row(
        ui,
        tr!("supports-sides"),
        &mut sides,
        default.as_ref(),
        |ui, sides| {
            ui.add(
                UnitField::new(sides, Unit::Count)
                    .range(3.0..=64.0)
                    .step(1.0)
                    .decimals(0),
            )
            .on_hover_text(tr!("supports-sides-hint"))
        },
    ) {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // 3 to 64.
        {
            p.sides = sides.round() as u16;
        }
        changed = true;
    }

    changed
}

/// The tip's direction: along the surface's normal, at the shaft, or as a handle turned it
/// (offered only then). Returns the new axis when another was chosen.
fn tip_direction(ui: &mut egui::Ui, axis: AxisSpec, turned: bool) -> Option<AxisSpec> {
    ui.label(
        egui::RichText::new(tr!("supports-tip-direction")).color(ui.visuals().weak_text_color()),
    );
    let mut chosen = match axis {
        AxisSpec::Normal => 0,
        AxisSpec::Straight => 1,
        AxisSpec::Direction(_) => 2,
    };
    let mut labels = vec![tr!("supports-tip-normal"), tr!("supports-tip-straight")];
    let unavailable: &[usize] = if turned {
        labels.push(tr!("supports-tip-turned"));
        &[2]
    } else {
        &[]
    };
    if choice_among(ui, &mut chosen, &labels, unavailable)
        .on_hover_text(tr!("supports-tip-direction-hint"))
        .changed()
    {
        return Some(if chosen == 1 {
            AxisSpec::Straight
        } else {
            AxisSpec::Normal
        });
    }
    None
}

/// A preset: a weight, or the user's own at this place in their list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PresetRef {
    Weight(Weight),
    Mine(usize),
}

impl PresetRef {
    /// Its name.
    fn label(self, presets: &[SupportPreset]) -> String {
        match self {
            Self::Weight(weight) => weight.label(),
            Self::Mine(i) => presets.get(i).map_or_else(String::new, |p| p.name.clone()),
        }
    }

    /// Its values, if it still exists and they are valid.
    fn values(self, presets: &[SupportPreset]) -> Option<SupportValues> {
        match self {
            Self::Weight(weight) => Some(SupportValues::of(weight)),
            Self::Mine(i) => presets
                .get(i)
                .map(|p| p.values)
                .filter(SupportValues::is_valid),
        }
    }
}

/// The preset whose values are `p` exactly (a weight first, then the user's own), if any: a
/// support's values changed since it was placed match none. Whether its contact fits the
/// surface is not a preset's.
fn matching_preset(p: &SupportParameters, presets: &[SupportPreset]) -> Option<PresetRef> {
    let p = SupportParameters {
        contact_fit_min_mm: None,
        ..p.clone()
    };
    Weight::ALL
        .into_iter()
        .map(PresetRef::Weight)
        .chain((0..presets.len()).map(PresetRef::Mine))
        .find(|r| r.values(presets).is_some_and(|v| v.parameters() == p))
}

/// `support` with the values of a preset, `values`: its sizes only. Where it stands, its knee,
/// its tip's direction, its contact's fitting and an accepted collision stay as they are.
fn with_preset(support: &ManualSupport, values: &SupportValues) -> ManualSupport {
    ManualSupport {
        parameters: super::fitted(values.parameters(), support.parameters.contact_fit_min_mm),
        ..support.clone()
    }
}

/// The presets' menu, `width` wide: the weights, then the user's own under "Mine", `chosen`
/// marked and named (or "Custom" when none is). Returns the one picked.
fn preset_menu(
    ui: &mut egui::Ui,
    id_salt: &str,
    width: f32,
    presets: &[SupportPreset],
    chosen: Option<PresetRef>,
) -> (Option<PresetRef>, egui::Response) {
    let mut picked = None;
    let shown = chosen.map_or_else(|| tr!("supports-preset-custom"), |c| c.label(presets));
    let response = egui::ComboBox::from_id_salt(id_salt)
        .width(width)
        .truncate()
        .selected_text(shown)
        .show_ui(ui, |ui| {
            for weight in Weight::ALL {
                let this = PresetRef::Weight(weight);
                if ui
                    .selectable_label(chosen == Some(this), weight.label())
                    .clicked()
                {
                    picked = Some(this);
                }
            }
            if !presets.is_empty() {
                ui.separator();
                ui.label(
                    egui::RichText::new(tr!("supports-presets-mine"))
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
            }
            for (i, preset) in presets.iter().enumerate() {
                let this = PresetRef::Mine(i);
                if ui
                    .selectable_label(chosen == Some(this), preset.name.as_str())
                    .clicked()
                {
                    picked = Some(this);
                }
            }
        })
        .response;
    crate::widgets::name(&response, &tr!("supports-preset"));
    (picked, response)
}

impl SupportsUi {
    /// The preset row (owner's mockup, 2026-10-27): the presets, built in then the user's own,
    /// and the buttons that save the values as a new preset, rename the user's preset chosen and
    /// delete it; then the name being typed, when one is.
    fn preset_row(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new(tr!("supports-preset")).color(ui.visuals().weak_text_color()));
        let icons = Icons::shared(ui.ctx());
        let mine = self.next.mine.filter(|&i| i < self.presets.len());
        ui.horizontal(|ui| {
            let buttons = 5.0 * (ui.spacing().interact_size.y + ui.spacing().item_spacing.x);
            let width = (ui.available_width() - buttons).max(60.0);
            let chosen = Some(mine.map_or(PresetRef::Weight(self.next.weight), PresetRef::Mine));
            let (picked, response) =
                preset_menu(ui, "support-preset", width, &self.presets, chosen);
            response.on_hover_text(tr!("supports-preset-hint"));
            // A preset chosen keeps the contact's fitting: the tool's, not the preset's.
            match picked {
                Some(PresetRef::Weight(weight)) => {
                    self.next = self.next.chosen(NextSupport::of(weight));
                }
                Some(PresetRef::Mine(i)) => {
                    self.next = self.next.chosen(NextSupport::of_preset(&self.presets, i));
                }
                None => {}
            }
            if icon_button(
                ui,
                icons.as_deref(),
                Icon::Plus,
                false,
                &tr!("supports-preset-save"),
            )
            .clicked()
            {
                let name = tr!("supports-preset-new-name", number = self.presets.len() + 1);
                self.naming = Some((None, name));
            }
            // The user's preset chosen takes the values as they are now (owner, 2026-10-29).
            let changed = mine.is_some_and(|i| self.presets[i].values != self.next.values);
            ui.add_enabled_ui(changed, |ui| {
                if icon_button(
                    ui,
                    icons.as_deref(),
                    Icon::Save,
                    false,
                    &tr!("supports-preset-update"),
                )
                .clicked()
                {
                    if let Some(i) = mine {
                        self.update_preset(i);
                    }
                }
            });
            ui.add_enabled_ui(mine.is_some(), |ui| {
                if icon_button(
                    ui,
                    icons.as_deref(),
                    Icon::Edit,
                    false,
                    &tr!("supports-preset-rename"),
                )
                .clicked()
                {
                    if let Some(i) = mine {
                        self.naming = Some((Some(i), self.presets[i].name.clone()));
                    }
                }
                if icon_button(
                    ui,
                    icons.as_deref(),
                    Icon::Delete,
                    false,
                    &tr!("supports-preset-delete"),
                )
                .clicked()
                {
                    self.deleting = mine;
                }
            });
            // Another preset than the default one: its reset (owner, 2026-11-02).
            let default = self.next_default;
            let other = (self.next.weight, self.next.mine) != (default.weight, default.mine);
            let name = default
                .mine
                .map_or(PresetRef::Weight(default.weight), PresetRef::Mine)
                .label(&self.presets);
            if reset_button(ui, other, &name) {
                self.next = self.next.chosen(default);
            }
        });
        self.naming_row(ui);
        self.confirm_deletion(ui.ctx());
    }

    /// The name of a preset being typed, with Save and Cancel: a new preset takes the next
    /// support's values, a renamed one keeps its own. A name another preset has is not taken.
    fn naming_row(&mut self, ui: &mut egui::Ui) {
        let Some((which, mut name)) = self.naming.take() else {
            return;
        };
        let trimmed = name.trim().to_owned();
        let taken = self
            .presets
            .iter()
            .enumerate()
            .any(|(i, p)| Some(i) != which && p.name == trimmed);
        let mut done = None;
        ui.horizontal(|ui| {
            let field = ui.add(
                text_field(&mut name, (ui.available_width() - 150.0).max(60.0))
                    .hint_text(tr!("supports-preset-name")),
            );
            if which.is_none() && !field.has_focus() && !field.lost_focus() {
                field.request_focus();
            }
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let can = !trimmed.is_empty() && !taken;
            // Why a name is not taken is the button's tooltip, not panel text (TD-098).
            let why = if taken {
                tr!("supports-preset-taken")
            } else {
                tr!("supports-preset-empty")
            };
            if ui
                .add_enabled(can, egui::Button::new(tr!("supports-preset-confirm")))
                .on_disabled_hover_text(why)
                .clicked()
                || (enter && can)
            {
                done = Some(true);
            }
            if ui.button(tr!("supports-preset-cancel")).clicked() {
                done = Some(false);
            }
        });
        match (done, which) {
            (Some(true), Some(i)) => self.presets[i].name = trimmed,
            (Some(true), None) => {
                self.presets.push(SupportPreset {
                    name: trimmed,
                    values: self.next.values,
                });
                self.next.mine = Some(self.presets.len() - 1);
            }
            (Some(false), _) => {}
            (None, _) => self.naming = Some((which, name)),
        }
    }

    /// The user's yes before a preset is deleted: it cannot be undone.
    fn confirm_deletion(&mut self, ctx: &egui::Context) {
        let Some(i) = self.deleting.filter(|&i| i < self.presets.len()) else {
            self.deleting = None;
            return;
        };
        let text = tr!(
            "supports-preset-delete-text",
            name = self.presets[i].name.as_str()
        );
        match crate::dialog::confirm(
            ctx,
            "support-preset-delete",
            &tr!("supports-preset-delete-title"),
            &text,
            &tr!("supports-preset-delete-action"),
            true,
        ) {
            Some(true) => {
                self.delete_preset(i);
                self.deleting = None;
            }
            Some(false) => self.deleting = None,
            None => {}
        }
    }

    /// The user's preset `i` takes the next support's values as they are now, said in a
    /// notification.
    pub(super) fn update_preset(&mut self, i: usize) {
        let Some(preset) = self.presets.get_mut(i) else {
            return;
        };
        preset.values = self.next.values;
        self.outbox.push(crate::messages::Message::new(
            crate::messages::Level::Success,
            tr!("supports-preset-updated", name = preset.name.as_str()),
        ));
    }

    /// Delete the user's preset `i`: what referred to it by place follows, and what referred to
    /// it falls back to the weights.
    pub(super) fn delete_preset(&mut self, i: usize) {
        if i >= self.presets.len() {
            return;
        }
        self.presets.remove(i);
        let shift = |place: Option<usize>| match place {
            Some(p) if p == i => None,
            Some(p) if p > i => Some(p - 1),
            other => other,
        };
        self.next.mine = shift(self.next.mine);
        self.default_preset = shift(self.default_preset);
        self.islands.mine = shift(self.islands.mine);
        self.islands_default_mine = shift(self.islands_default_mine);
        self.automatic.mine = shift(self.automatic.mine);
        self.automatic_default_mine = shift(self.automatic_default_mine);
        self.automatic.cavity_mine = shift(self.automatic.cavity_mine);
        self.automatic_cavity_default_mine = shift(self.automatic_cavity_default_mine);
        self.grid.mine = shift(self.grid.mine);
        self.grid_default_mine = shift(self.grid_default_mine);
    }

    /// The panel's body: the next supports' settings, the list, then the checks.
    pub fn panel(&mut self, ui: &mut egui::Ui, workspace: &mut Workspace, index: Option<usize>) {
        let index = index.filter(|&i| i < workspace.plate().objects.len());
        if index.is_none() {
            self.close();
        }
        ui.add_enabled_ui(index.is_some(), |ui| {
            crate::widgets::group(ui, tr!("supports-new"), |ui| {
                // Placing, the surface and the switches are on the tool strip (owner,
                // 2026-10-29); a model without a cavity has no cavity to place in.
                let hollowed = index
                    .and_then(|i| workspace.plate().objects.get(i))
                    .is_some_and(|o| o.hollowing().is_some());
                if !hollowed {
                    self.on_cavity = false;
                }
                self.preset_row(ui);
                let mut p = self.next.parameters();
                // A value's default is its preset's (owner, 2026-11-02); the preset's own reset
                // returns to the default preset.
                let preset = self
                    .next
                    .mine
                    .filter(|&i| i < self.presets.len())
                    .map_or(PresetRef::Weight(self.next.weight), PresetRef::Mine)
                    .values(&self.presets)
                    .map(|v| super::fitted(v.parameters(), self.next_default.contact_fit_min_mm));
                let changed = if self.mini {
                    mini_fields(ui, &mut p, preset.as_ref())
                } else {
                    parameter_fields(ui, &mut p, self.advanced, preset.as_ref())
                };
                if changed {
                    self.next.values = SupportValues::with(&p, self.next.values.tip_towards_shaft);
                    self.next.contact_fit_min_mm = p.contact_fit_min_mm;
                }
                if self.advanced && !self.mini {
                    if let Some(axis) = tip_direction(ui, self.next.axis(), false) {
                        self.next.values.tip_towards_shaft = axis == AxisSpec::Straight;
                    }
                }
            });
        });
        self.selected_support(ui, workspace, index);
        if self.advanced {
            checks(ui, workspace, index);
        }
    }

    /// The selected support of model `index`: its state and its values, which change it alone
    /// (the list of supports is a window of its own, owner 2026-10-29). Without one, the same
    /// fields, disabled, with the next support's values.
    fn selected_support(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &mut Workspace,
        index: Option<usize>,
    ) {
        let chosen = index.and_then(|object| {
            let (o, id) = self.selected?;
            (o == object).then_some(id)
        });
        let support: Option<ManualSupport> = chosen.and_then(|id| {
            workspace
                .supports(index?)?
                .supports
                .iter()
                .find(|s| s.id == id)
                .cloned()
        });
        let findings = match (&support, index) {
            (Some(s), Some(object)) => self.findings(object, s.id),
            _ => Vec::new(),
        };
        let title = self.selected_title(support.as_ref(), index);
        crate::widgets::group(ui, title, |ui| {
            let advanced = self.advanced;
            let shown = support.clone().unwrap_or_else(|| ManualSupport {
                id: 0,
                tip_mm: [0.0; 3],
                tip_axis: self.next.axis(),
                bends: Vec::new(),
                base: karasu_core::supports::BaseSpec::Vertical,
                parameters: self.next.parameters(),
                accept_collision: false,
                origin: karasu_core::supports::SupportOrigin::Placed,
                kind: self.next_kind(),
            });
            // Its values, or the next support's without one: a mini support's are its own two.
            let mini = shown.kind == SupportKind::Mini;
            let mut changed = shown.clone();
            let mut represet = None;
            ui.add_enabled_ui(support.is_some(), |ui| {
                let error = findings.iter().find(|f| f.severity == Severity::Error);
                let state = match error {
                    Some(f) => super::list::short(f.kind),
                    None if support.is_some() => tr!("list-ok"),
                    None => "–".to_owned(),
                };
                let hint: Vec<String> = findings.iter().map(describe).collect();
                let line = fact_row(ui, &tr!("list-state"), &state);
                if !hint.is_empty() {
                    line.on_hover_text(hint.join("\n"));
                }
                self.built_contact_row(ui, support.as_ref(), index);
                // Its preset, if its values are one's; another chosen applies it at once (owner,
                // 2026-10-29).
                ui.label(
                    egui::RichText::new(tr!("supports-preset"))
                        .color(ui.visuals().weak_text_color()),
                );
                let matched = matching_preset(&shown.parameters, &self.presets);
                let width = ui.available_width();
                let (picked, response) =
                    preset_menu(ui, "selected-support-preset", width, &self.presets, matched);
                response.on_hover_text(tr!("supports-preset-selected-hint"));
                if let Some(values) = picked.and_then(|p| p.values(&self.presets)) {
                    represet = Some(with_preset(&shown, &values));
                }
                if mini {
                    mini_fields(ui, &mut changed.parameters, None);
                } else {
                    parameter_fields(ui, &mut changed.parameters, advanced, None);
                }
                if advanced && !mini {
                    let turned = matches!(shown.tip_axis, AxisSpec::Direction(_));
                    if let Some(axis) = tip_direction(ui, shown.tip_axis, turned) {
                        changed.tip_axis = axis;
                    }
                }
                // A knee set with its handle is taken away here: the shaft goes straight down from
                // the tip's join again.
                if !changed.bends.is_empty()
                    && ui
                        .button(tr!("supports-straighten"))
                        .on_hover_text(tr!("supports-straighten-hint"))
                        .clicked()
                {
                    changed.bends.clear();
                }
                let collides = findings.iter().any(|f| {
                    f.kind == FindingKind::Collision
                        && matches!(f.severity, Severity::Error | Severity::Accepted)
                });
                if collides || changed.accept_collision {
                    ui.horizontal(|ui| {
                        let accept = switch(ui, &mut changed.accept_collision)
                            .on_hover_text(tr!("supports-accept-hint"));
                        crate::widgets::name(&accept, &tr!("supports-accept"));
                        ui.add(egui::Label::new(tr!("supports-accept")).wrap());
                    });
                }
            });
            if let (Some(support), Some(object)) = (support, index) {
                // A branch is no thicker than its trunk where it joins it (owner, 2026-10-30).
                let represet = represet.map(|r| self.as_branch(workspace, object, r, true));
                let changed = self.no_thicker(object, changed);
                // A preset chosen is one step of its own; a field changed merges with the next.
                if let Some(represet) = represet.filter(|r| *r != support) {
                    Self::replace_once(workspace, object, represet);
                } else if changed != support {
                    Self::replace(workspace, object, changed);
                }
            }
        });
    }

    /// A support whose contact fits the surface: the diameter it was built with.
    fn built_contact_row(
        &self,
        ui: &mut egui::Ui,
        support: Option<&ManualSupport>,
        index: Option<usize>,
    ) {
        let built = support
            .filter(|s| s.parameters.contact_fit_min_mm.is_some())
            .zip(index)
            .and_then(|(s, object)| self.built_contact(object, s.id));
        if let Some(diameter) = built {
            fact_row(
                ui,
                &tr!("supports-contact-built"),
                &with_unit(diameter, 2, Unit::Millimetre),
            )
            .on_hover_text(tr!("supports-contact-built-hint"));
        }
    }

    /// The selected support's heading: its number, a mini support's said, and whether it is
    /// inside the cavity; without one, "Selected support".
    fn selected_title(&self, support: Option<&ManualSupport>, index: Option<usize>) -> String {
        let Some(s) = support else {
            return tr!("supports-selected");
        };
        let mini = s.kind == SupportKind::Mini;
        match index {
            Some(object) if self.interior(object, s.id) => {
                if mini {
                    tr!("supports-mini-support-inside", number = s.id)
                } else {
                    tr!("supports-support-inside", number = s.id)
                }
            }
            _ if mini => tr!("supports-mini-support", number = s.id),
            _ => tr!("supports-support", number = s.id),
        }
    }

    /// The panel's footer: the selected model's supports' state.
    pub fn footer(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &mut Workspace,
        preparation: &PreparationUi,
        index: Option<usize>,
        icons: Option<&Icons>,
    ) {
        let index = index.filter(|&i| i < workspace.plate().objects.len());
        if self.merging() {
            let count = self.merge_counts(index).map_or(0, |(n, _)| n);
            let (apply, discard) = action_pair(
                ui,
                [
                    tr!("supports-merge-apply", count = count),
                    tr!("supports-merge-discard"),
                ],
                (count > 0, true),
            );
            if let (true, Some(i)) = (apply.clicked(), index) {
                self.apply_merging(workspace, i);
            } else if discard.clicked() {
                self.toggle_merging(index);
            }
            ui.add_space(4.0);
        }
        let (text, colour, icon, detail) = self.state(workspace, preparation, index);
        let line = state_line(ui, icons, icon, &text, colour);
        if let Some(detail) = detail {
            line.on_hover_text(detail);
        }
    }

    /// What the footer says while trunks merge or a support waits to join a shaft, if either.
    fn tree_state(
        &self,
        index: usize,
    ) -> Option<(String, egui::Color32, Option<Icon>, Option<String>)> {
        if self.merging() {
            return Some(match self.merge_counts(Some(index)) {
                None => (
                    tr!("supports-merge-working"),
                    theme::ACCENT.color(),
                    Some(Icon::Info),
                    None,
                ),
                Some((0, _)) => (
                    tr!("supports-merge-none"),
                    theme::BORDER.color(),
                    Some(Icon::Info),
                    Some(tr!("supports-merge-none-hint")),
                ),
                Some((count, mains)) => (
                    tr!("supports-merge-state", count = count, mains = mains),
                    theme::ACCENT.color(),
                    Some(Icon::Info),
                    Some(tr!("supports-merge-hint")),
                ),
            });
        }
        if self.joining() {
            return Some(if self.join_refused {
                (
                    tr!("supports-join-refused"),
                    theme::ERROR.color(),
                    Some(Icon::Error),
                    Some(tr!("supports-join-refused-hint")),
                )
            } else {
                (
                    tr!("supports-joining"),
                    theme::ACCENT.color(),
                    Some(Icon::Info),
                    Some(tr!("supports-joining-hint")),
                )
            });
        }
        None
    }

    /// What the footer says while branches are placed: the tip to click, the shaft to click,
    /// or a shaft refused.
    fn branch_state(&self) -> (String, egui::Color32, Option<Icon>, Option<String>) {
        let (text, hint, colour, icon) = match (self.branch_tip.is_some(), self.branch_refused) {
            (false, _) => (
                tr!("supports-placing-branch-tip"),
                tr!("supports-placing-branch-tip-hint"),
                theme::ACCENT.color(),
                Icon::Info,
            ),
            (true, false) => (
                tr!("supports-placing-branch"),
                tr!("supports-placing-branch-hint"),
                theme::ACCENT.color(),
                Icon::Info,
            ),
            (true, true) => (
                tr!("supports-branch-refused"),
                tr!("supports-branch-refused-hint"),
                theme::ERROR.color(),
                Icon::Error,
            ),
        };
        (text, colour, Some(icon), Some(hint))
    }

    /// What the footer says while supports are placed: the foot to click, a refused spot (a
    /// surface facing up, or nowhere for a mini support to stand), or where to click.
    fn placing_state(&self) -> (String, egui::Color32, Option<Icon>, Option<String>) {
        if self.following() {
            let (text, hint) = if self.mini {
                (
                    tr!("supports-placing-mini-foot"),
                    tr!("supports-placing-mini-foot-hint"),
                )
            } else {
                (
                    tr!("supports-placing-foot"),
                    tr!("supports-placing-foot-hint"),
                )
            };
            return (text, theme::ACCENT.color(), Some(Icon::Info), Some(hint));
        }
        match self.candidate {
            Some(Candidate::FacingUp) => {
                return (
                    tr!("supports-facing-up"),
                    theme::ERROR.color(),
                    Some(Icon::Error),
                    None,
                );
            }
            Some(Candidate::NoFoot) => {
                return (
                    tr!("supports-mini-no-foot"),
                    theme::ERROR.color(),
                    Some(Icon::Error),
                    Some(tr!("supports-mini-no-foot-hint")),
                );
            }
            Some(Candidate::Here) | None => {}
        }
        // The support a click would add shows an error: said here, not by its red alone
        // (TD-142).
        if self.preview.as_ref().is_some_and(|preview| preview.error) {
            return (
                tr!("supports-preview-error"),
                theme::ERROR.color(),
                Some(Icon::Error),
                Some(tr!("supports-preview-error-hint")),
            );
        }
        // In the cavity, the section has to show it first.
        let (text, hint) = if self.on_cavity && !self.cavity_view && self.candidate.is_none() {
            (
                tr!("supports-placing-cavity"),
                tr!("supports-placing-cavity-hint"),
            )
        } else {
            (tr!("supports-placing"), tr!("supports-placing-hint"))
        };
        (text, theme::ACCENT.color(), Some(Icon::Info), Some(hint))
    }

    /// What the footer says: placing, a refused spot, a handle dragged, a build that failed,
    /// supports with errors (which print as they are), a build running, or how many supports
    /// there are.
    fn state(
        &self,
        workspace: &Workspace,
        preparation: &PreparationUi,
        index: Option<usize>,
    ) -> (String, egui::Color32, Option<Icon>, Option<String>) {
        let Some(index) = index else {
            return ("–".to_owned(), theme::BORDER.color(), None, None);
        };
        if workspace.plate().setting().is_none() {
            return (
                tr!("prepare-needs-profiles"),
                theme::WARNING.color(),
                Some(Icon::Warning),
                None,
            );
        }
        if self.placing && self.branching {
            return self.branch_state();
        }
        if let Some(state) = self.tree_state(index) {
            return state;
        }
        if self.placing {
            return self.placing_state();
        }
        if self.dragging() {
            return (
                tr!("supports-dragging"),
                theme::ACCENT.color(),
                Some(Icon::Info),
                None,
            );
        }
        let count = workspace.supports(index).map_or(0, |d| d.supports.len());
        if count == 0 {
            return (tr!("supports-none"), theme::BORDER.color(), None, None);
        }
        if !self.current(workspace, preparation, index) {
            return (tr!("supports-building"), theme::BORDER.color(), None, None);
        }
        match self.builds.get(&index).map(|b| &b.scene) {
            Some(Err(reason)) => (
                tr!("supports-build-failed"),
                theme::ERROR.color(),
                Some(Icon::Error),
                Some(reason.clone()),
            ),
            Some(Ok(scene)) if scene.has_errors() => {
                let wrong = errors(scene.findings()).len();
                let detail: Vec<String> = scene
                    .findings()
                    .iter()
                    .filter(|f| f.severity == Severity::Error)
                    .map(|f| tr!("supports-support", number = f.entity) + ": " + &describe(f))
                    .collect();
                (
                    tr!("supports-with-errors", count = wrong),
                    theme::WARNING.color(),
                    Some(Icon::Warning),
                    Some(detail.join("\n")),
                )
            }
            _ => (
                tr!("supports-count", count = count),
                theme::SUCCESS.color(),
                Some(Icon::Success),
                None,
            ),
        }
    }
}

/// The checks of object `index`'s supports (Advanced): the clearance proven from the model and
/// how far a tip may turn from the surface, kept in its support document. Without supports
/// they show the defaults, disabled.
fn checks(ui: &mut egui::Ui, workspace: &mut Workspace, index: Option<usize>) {
    let document = index.and_then(|i| workspace.supports(i)).cloned();
    egui::CollapsingHeader::new(tr!("supports-checks"))
        .id_salt("supports-checks")
        .default_open(true)
        .show(ui, |ui| {
            let defaults =
                karasu_core::supports::SupportDocument::new(karasu_core::supports::Preset::Medium);
            let mut edited = document.clone().unwrap_or(defaults);
            ui.add_enabled_ui(document.is_some(), |ui| {
                value_row(ui, tr!("supports-clearance"), |ui| {
                    ui.add(
                        UnitField::new(&mut edited.clearance_mm, Unit::Millimetre)
                            .range(0.0..=5.0)
                            .step(0.05),
                    )
                    .on_hover_text(tr!("supports-clearance-hint"))
                });
                value_row(ui, tr!("supports-tip-limit"), |ui| {
                    ui.add(
                        UnitField::new(&mut edited.max_axis_deviation_deg, Unit::Degree)
                            .range(0.0..=89.0)
                            .step(1.0)
                            .decimals(0),
                    )
                    .on_hover_text(tr!("supports-tip-limit-hint"))
                });
            });
            if let (Some(i), Some(before)) = (index, document) {
                if edited != before {
                    workspace.set_supports(i, Some(edited), Some(CHECK_KEYS | i as u64));
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_supports_preset_is_found_by_its_values_and_choosing_one_changes_only_its_sizes() {
        let fine = SupportValues {
            tip_length_mm: 1.75,
            ..SupportValues::of(Weight::Light)
        };
        let presets = vec![SupportPreset {
            name: "Fine".to_owned(),
            values: fine,
        }];
        let medium = SupportValues::of(Weight::Medium).parameters();
        assert_eq!(
            matching_preset(&medium, &presets),
            Some(PresetRef::Weight(Weight::Medium))
        );
        assert_eq!(
            matching_preset(&fine.parameters(), &presets),
            Some(PresetRef::Mine(0))
        );
        // A value changed since: no preset, "Custom".
        let changed = SupportParameters {
            shaft_diameter_mm: medium.shaft_diameter_mm + 0.1,
            ..medium.clone()
        };
        assert_eq!(matching_preset(&changed, &presets), None);
        // Choosing one: its sizes, the rest as it was.
        let placed = ManualSupport {
            id: 7,
            tip_mm: [1.0, 2.0, 30.0],
            tip_axis: AxisSpec::Direction([0.0, 0.6, -0.8]),
            bends: vec![[3.0, 2.0, 20.0]],
            base: karasu_core::supports::BaseSpec::Platform([3.0, 2.0]),
            parameters: medium,
            accept_collision: true,
            origin: karasu_core::supports::SupportOrigin::Placed,
            kind: karasu_core::supports::SupportKind::Standard,
        };
        let heavy = with_preset(&placed, &SupportValues::of(Weight::Heavy));
        assert_eq!(
            heavy.parameters,
            SupportValues::of(Weight::Heavy).parameters()
        );
        assert_eq!(
            ManualSupport {
                parameters: placed.parameters.clone(),
                ..heavy
            },
            placed
        );
        assert_eq!(PresetRef::Mine(0).label(&presets), "Fine".to_owned());
        assert_eq!(PresetRef::Mine(3).values(&presets), None);
    }

    #[test]
    fn presets_of_the_users_own_are_chosen_and_deleted_by_place() {
        let preset = |name: &str, weight| SupportPreset {
            name: name.to_owned(),
            values: SupportValues {
                tip_length_mm: 4.0,
                ..SupportValues::of(weight)
            },
        };
        let mut ui = SupportsUi {
            presets: vec![
                preset("Fine", Weight::Light),
                preset("Bases", Weight::Heavy),
                preset("Wide", Weight::Heavy),
            ],
            ..SupportsUi::default()
        };
        // Choosing one takes its values, every one of them.
        ui.next = NextSupport::of_preset(&ui.presets, 2);
        assert_eq!(ui.next.mine, Some(2));
        assert_eq!(ui.next.values, ui.presets[2].values);
        assert_eq!(ui.next.parameters().tip_length_mm, 4.0);
        ui.default_preset = Some(1);
        // Deleting one before them keeps them chosen; deleting the default falls back.
        ui.delete_preset(0);
        assert_eq!((ui.next.mine, ui.default_preset), (Some(1), Some(0)));
        ui.delete_preset(0);
        assert_eq!((ui.next.mine, ui.default_preset), (Some(0), None));
        ui.delete_preset(0);
        assert_eq!(ui.next.mine, None);
        // Updated with values changed in the panel, said in a notification.
        ui.presets = vec![preset("Fine", Weight::Light)];
        ui.next = NextSupport::of_preset(&ui.presets, 0);
        ui.next.values.shaft_diameter_mm = 0.9;
        ui.update_preset(0);
        assert_eq!(ui.presets[0].values.shaft_diameter_mm, 0.9);
        assert_eq!(ui.take_messages().len(), 1);
        ui.presets.clear();
        // A place beyond the list gives Medium.
        assert_eq!(
            NextSupport::of_preset(&ui.presets, 5),
            NextSupport::of(Weight::Medium)
        );
    }

    #[test]
    fn the_fields_keep_the_couplings_the_core_requires() {
        // A tip body narrowed below the contact narrows the contact, and a ball follows it.
        let mut p = Weight::Heavy.preset().parameters();
        p.tip_body_diameter_mm = 0.3;
        couple(&mut p);
        assert!(p.contact_diameter_mm <= p.tip_body_diameter_mm);
        assert!(p.contact_sphere_diameter_mm >= p.contact_diameter_mm);
        // A pad narrowed below its cone, and a foot's body below its contact.
        p.pad_diameter_mm = 1.0;
        p.foot_body_diameter_mm = 0.2;
        couple(&mut p);
        assert!(p.cone_diameter_mm <= p.pad_diameter_mm);
        assert!(p.foot_contact_diameter_mm <= p.foot_body_diameter_mm);
        p.validate(1, &mut Vec::new()).unwrap();
        // A contact narrowed below the smallest a fitted contact narrows to takes it along.
        p.contact_fit_min_mm = Some(0.5);
        p.contact_diameter_mm = 0.25;
        couple(&mut p);
        assert_eq!(p.contact_fit_min_mm, Some(0.25));
        p.validate(1, &mut Vec::new()).unwrap();
    }

    #[test]
    fn fitting_the_contact_is_the_tools_choice_kept_across_presets() {
        // Fitting on, down to 0.35 mm: Light's 0.3 mm contact caps it.
        let next = NextSupport::of(Weight::Heavy).fitting(Some(0.35));
        assert_eq!(next.parameters().contact_fit_min_mm, Some(0.35));
        let light = next.chosen(NextSupport::of(Weight::Light));
        assert_eq!(light.weight, Weight::Light);
        assert_eq!(light.contact_fit_min_mm, Some(0.35));
        assert_eq!(light.parameters().contact_fit_min_mm, Some(0.3));
        light.parameters().validate(1, &mut Vec::new()).unwrap();
        // A support fitting its contact still matches its preset, and keeps fitting when
        // another is chosen for it.
        let support = ManualSupport {
            id: 1,
            tip_mm: [0.0; 3],
            tip_axis: AxisSpec::Normal,
            bends: Vec::new(),
            base: karasu_core::supports::BaseSpec::Vertical,
            parameters: next.parameters(),
            accept_collision: false,
            origin: karasu_core::supports::SupportOrigin::Placed,
            kind: SupportKind::Standard,
        };
        assert_eq!(
            matching_preset(&support.parameters, &[]),
            Some(PresetRef::Weight(Weight::Heavy))
        );
        let changed = with_preset(&support, &SupportValues::of(Weight::Light));
        assert_eq!(changed.parameters.contact_diameter_mm, 0.3);
        assert_eq!(changed.parameters.contact_fit_min_mm, Some(0.3));
        // The Islands and Automatic tools fit theirs when asked, and refuse a smallest out of
        // range from the settings.
        let islands = crate::islands_ui::plan::IslandSupports {
            contact_fit_min_mm: Some(0.2),
            ..Default::default()
        };
        assert_eq!(islands.next(&[]).parameters().contact_fit_min_mm, Some(0.2));
        let automatic = crate::auto_ui::plan::AutoSupports {
            contact_fit_min_mm: Some(0.25),
            ..Default::default()
        };
        assert_eq!(
            automatic.next(&[]).parameters().contact_fit_min_mm,
            Some(0.25)
        );
        // ... and end their contacts in a ball when asked, whatever the preset.
        let balls = crate::islands_ui::plan::IslandSupports {
            sphere_contact: true,
            ..islands
        };
        let p = balls.next(&[]).parameters();
        assert_eq!(p.contact_shape, ContactShape::Sphere);
        p.validate(1, &mut Vec::new()).unwrap();
        let automatic = crate::auto_ui::plan::AutoSupports {
            sphere_contact: true,
            ..automatic
        };
        assert_eq!(
            automatic.next(&[]).parameters().contact_shape,
            ContactShape::Sphere
        );
        let wrong = crate::islands_ui::plan::IslandSupports {
            contact_fit_min_mm: Some(0.0),
            ..islands
        };
        assert_eq!(wrong.valid().contact_fit_min_mm, None);
    }
}
