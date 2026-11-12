//! The plate's masks in the Slice panel (TD-134): its anti-aliasing and smallest gray, each the
//! print setting's unless the plate overrides it, and whether its supports are anti-aliased with
//! the model or binary.
//!
//! A value equal to the print setting's is no override: the plate then follows its print
//! setting, and the reset button beside a value brings it back. Choosing a value is one undoable
//! edit of the plate, so a slice of the plate as it was becomes out of date. Levels the plate's
//! format cannot store are disabled, with why.
use crate::{
    i18n::tr,
    profiles_ui::{antialias_label, ANTIALIAS_LEVELS},
    widgets::{default_row, default_switch, DefaultValue, Unit, UnitField, FIELD_WIDTH},
    workspace::Workspace,
};
use eframe::egui;
use karasu_project::{project::ImageOverride, request::machine_exporter_with};

/// The edit key of a smallest gray typed digit by digit: one undo step.
const GRAY_EDIT: u64 = 0x4D41_534B_4752_4159; // "MASKGRAY"

/// The masks' rows for the plate of `workspace`: nothing without a print setting.
pub(super) fn rows(ui: &mut egui::Ui, workspace: &mut Workspace) {
    let plate = workspace.plate();
    let Some(setting) = plate.setting().map(|s| s.image.clone()) else {
        return;
    };
    let format = plate.settings.format.as_str();
    let image = plate.settings.image;
    let (antialias, min_gray) = image.resolve(&setting);
    let mut chosen = image;

    // Anti-aliasing, the print setting's unless overridden.
    let mut level = antialias;
    let default = DefaultValue::new(
        setting.antialias,
        tr!(
            "slice-from-setting",
            value = antialias_label(setting.antialias)
        ),
    );
    let changed = default_row(
        ui,
        tr!("field-antialias"),
        &mut level,
        Some(&default),
        |ui, level| {
            let before = *level;
            let mut response = egui::ComboBox::from_id_salt("plate-antialias")
                .selected_text(antialias_label(*level))
                .width(FIELD_WIDTH)
                .show_ui(ui, |ui| {
                    for samples in ANTIALIAS_LEVELS {
                        let stores = machine_exporter_with(format, samples).is_ok();
                        let item = ui
                            .add_enabled(
                                stores,
                                egui::Button::selectable(
                                    *level == samples,
                                    antialias_label(samples),
                                ),
                            )
                            .on_disabled_hover_text(tr!("slice-antialias-binary-format"));
                        if item.clicked() {
                            *level = samples;
                        }
                    }
                })
                .response
                .on_hover_text(tr!("slice-antialias-hint"));
            if *level != before {
                response.mark_changed();
            }
            response
        },
    );
    if changed {
        chosen.antialias = (level != setting.antialias).then_some(level);
    }

    // The smallest gray, only for anti-aliased masks.
    ui.add_enabled_ui(antialias > 1, |ui| {
        let mut gray = f64::from(min_gray);
        let default = DefaultValue::new(
            f64::from(setting.min_gray),
            tr!("slice-from-setting", value = setting.min_gray.to_string()),
        );
        let changed = default_row(
            ui,
            tr!("field-min-gray"),
            &mut gray,
            Some(&default),
            |ui, gray| {
                ui.add(
                    UnitField::new(gray, Unit::Count)
                        .range(0.0..=255.0)
                        .step(1.0)
                        .decimals(0),
                )
                .on_hover_text(tr!("field-min-gray-hint"))
            },
        );
        if changed {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Within 0..=255.
            let gray = gray.round().clamp(0.0, 255.0) as u8;
            chosen.min_gray = (gray != setting.min_gray).then_some(gray);
        }
    });

    // The supports, binary unless anti-aliased with the model.
    ui.add_enabled_ui(antialias > 1, |ui| {
        let mut on = image.antialias_supports;
        let default = DefaultValue::new(false, tr!("slice-antialias-supports-off"));
        if default_switch(
            ui,
            &mut on,
            (
                tr!("slice-antialias-supports"),
                tr!("slice-antialias-supports-hint"),
            ),
            Some(&default),
        ) {
            chosen.antialias_supports = on;
        }
    });

    if chosen != image {
        apply(workspace, chosen, chosen.min_gray != image.min_gray);
    }
}

/// Make `chosen` the plate's override: one undoable edit, merged while a gray is typed.
fn apply(workspace: &mut Workspace, chosen: ImageOverride, typing: bool) {
    let edit = |plate: &mut karasu_project::project::PlateContent| plate.settings.image = chosen;
    if typing {
        workspace.edit_merging(GRAY_EDIT, edit);
    } else {
        workspace.edit(edit);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use karasu_core::profiles::ImageSettings;

    /// The texts the rows draw for `workspace`'s plate.
    fn drawn(workspace: &mut Workspace) -> Vec<String> {
        let ctx = egui::Context::default();
        let mut texts = Vec::new();
        for _ in 0..2 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| rows(ui, workspace));
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
        texts
    }

    #[test]
    fn the_plate_overrides_its_print_setting_s_masks_as_one_undoable_edit() {
        let folder =
            std::env::temp_dir().join(format!("karasu-slice-masks-{}", std::process::id()));
        let mut workspace = crate::slicing_ui::tests::hollowed_cube(&folder);
        let before = workspace.plate().clone();
        let texts = drawn(&mut workspace);
        for expected in [
            tr!("field-antialias"),
            tr!("field-min-gray"),
            tr!("slice-antialias-supports"),
        ] {
            assert!(texts.contains(&expected), "{expected} in {texts:?}");
        }
        // Drawing changes nothing.
        assert_eq!(*workspace.plate(), before);
        let revision = workspace.revision();
        let off = ImageOverride {
            antialias: Some(1),
            ..ImageOverride::default()
        };
        apply(&mut workspace, off, false);
        assert_eq!(workspace.plate().settings.image, off);
        assert!(workspace.revision() > revision);
        assert!(drawn(&mut workspace).contains(&tr!("field-antialias-off")));
        // A gray typed digit by digit is one step.
        for gray in [Some(4), Some(40)] {
            apply(
                &mut workspace,
                ImageOverride {
                    antialias: Some(4),
                    min_gray: gray,
                    antialias_supports: false,
                },
                true,
            );
        }
        assert_eq!(workspace.plate().settings.image.min_gray, Some(40));
        assert!(workspace.undo());
        assert_eq!(workspace.plate().settings.image, off);
        assert!(workspace.undo());
        assert_eq!(*workspace.plate(), before);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_value_equal_to_the_print_setting_s_is_no_override() {
        let setting = ImageSettings {
            antialias: 16,
            min_gray: 30,
            ..ImageSettings::default()
        };
        let mut image = ImageOverride::default();
        assert_eq!(image.resolve(&setting), (16, 30));
        image.antialias = Some(1);
        assert_eq!(image.resolve(&setting), (1, 0));
        image.antialias = Some(4);
        image.min_gray = Some(0);
        assert_eq!(image.resolve(&setting), (4, 0));
        assert_eq!(antialias_label(1), tr!("field-antialias-off"));
        assert_eq!(antialias_label(8), "8");
    }
}
