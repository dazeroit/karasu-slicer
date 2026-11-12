//! Arrange models… (TD-135, phase 5, from the owner-approved mockup "Arrange with the overflow
//! to a new plate"): a modal dialog that arranges the plate shown's models with the packer
//! (from the plate's middle outwards), their footprints with supports and rafts, the spacing
//! between them the plate's own to start with. The models that do not fit go to new plates after it, with its printer,
//! resin, print setting and slicing options, arranged the same way, as many as needed; or, by
//! choice, stay where they are. The choice is kept across restarts; the spacing is not (it is
//! the plate's). One undoable step; a notification says what happened.
use crate::{
    dialog::{self, Part, Size},
    i18n::tr,
    messages::{Level, Message},
    widgets::{choice, value_row, Unit, UnitField},
    window::MainWindow,
    workspace::{Arranged, Overflow, Unarranged},
};
use eframe::egui;

/// The spacing Arrange accepts, in mm.
const SPACING_MM: std::ops::RangeInclusive<f64> = 0.0..=50.0;

impl MainWindow {
    /// Open Arrange models… for the plate shown, its spacing the plate's.
    pub(super) fn open_arrange(&mut self) {
        self.plates.arranging = Some(self.workspace.plate().settings.spacing_mm);
    }

    /// Arrange models…, while it is open: modal, Enter arranges, Escape cancels.
    pub(in crate::window) fn arrange_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut spacing) = self.plates.arranging.take() else {
            return;
        };
        let mut overflow = self.plates.choices.arrange_overflow;
        let models = self.workspace.plate().objects.len();
        let title = tr!("arrange-title", plate = &self.workspace.plate().name);
        let mut done = None;
        let response = dialog::show(
            ctx,
            "arrange-models",
            &title,
            Size::Fixed(400.0),
            |ui, part| match part {
                Part::Body => {
                    value_row(ui, tr!("arrange-spacing"), |ui| {
                        ui.add(UnitField::new(&mut spacing, Unit::Millimetre).range(SPACING_MM))
                    });
                    value_row(ui, tr!("arrange-models"), |ui| {
                        ui.label(tr!("arrange-models-all", count = models))
                    });
                    ui.add_space(4.0);
                    ui.label(tr!("arrange-overflow"));
                    let mut chosen = usize::from(overflow == Overflow::Stay);
                    let labels = [tr!("arrange-overflow-new"), tr!("arrange-overflow-stay")];
                    if choice(ui, &mut chosen, &labels).changed() {
                        overflow = if chosen == 0 {
                            Overflow::NewPlates
                        } else {
                            Overflow::Stay
                        };
                    }
                    ui.add_space(4.0);
                    let hint = match overflow {
                        Overflow::NewPlates => tr!("arrange-hint-new"),
                        Overflow::Stay => tr!("arrange-hint-stay"),
                    };
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(hint)
                                .small()
                                .color(ui.visuals().weak_text_color()),
                        )
                        .wrap(),
                    );
                }
                Part::Footer { enter } => {
                    if dialog::main_action(ui, &tr!("arrange-action"), models > 0, enter) {
                        done = Some(true);
                    }
                    if dialog::cancel_action(ui) {
                        done = Some(false);
                    }
                }
            },
        );
        self.plates.choices.arrange_overflow = overflow;
        match done {
            Some(true) => self.arrange(spacing, overflow),
            Some(false) => {}
            None if response.cancel => {}
            None => self.plates.arranging = Some(spacing),
        }
    }

    /// Arrange the plate shown, `spacing_mm` apart, and say what happened.
    pub(in crate::window) fn arrange(&mut self, spacing_mm: f64, overflow: Overflow) {
        let plate = self.workspace.plate().name.clone();
        let message = match self.workspace.arrange(spacing_mm, overflow) {
            Ok(arranged) => {
                // From the last, so that each index is still the model's.
                for &index in arranged.removed.iter().rev() {
                    self.forget_object(index);
                }
                arranged_message(&plate, &arranged)
            }
            Err(Unarranged::NoPrinter) => Message::new(Level::Warning, tr!("arrange-no-printer")),
            Err(Unarranged::Loading) => Message::new(Level::Info, tr!("arrange-loading")),
        };
        self.messages.push(message);
    }
}

/// The notification of an arrangement of plate `plate`: how many models it holds now, and
/// where the others went or that they stayed.
fn arranged_message(plate: &str, arranged: &Arranged) -> Message {
    let text = tr!("arrange-done", count = arranged.on_plate, plate = plate);
    let undo = tr!("arrange-undo");
    if arranged.moved > 0 {
        let plates = arranged.new_plates.join(", ");
        Message::new(Level::Success, text).detail(format!(
            "{} {undo}",
            tr!(
                "arrange-moved",
                count = arranged.moved,
                plates = plates,
                new = arranged.new_plates.len()
            )
        ))
    } else if arranged.stayed > 0 {
        Message::new(Level::Warning, text).detail(format!(
            "{} {undo}",
            tr!("arrange-stayed", count = arranged.stayed)
        ))
    } else {
        Message::new(Level::Success, text).detail(undo)
    }
}

#[cfg(test)]
mod tests {
    use crate::{actions::Action, i18n::tr, window::MainWindow, workspace::Overflow};
    use eframe::egui;

    #[test]
    fn arrange_opens_with_the_plate_s_spacing_and_says_what_it_did() {
        // TD-135, phase 5: the dialog starts from the plate's spacing and the kept choice;
        // the notification says how many models the plate holds and that Ctrl+Z undoes it.
        let ctx = egui::Context::default();
        let folder =
            std::env::temp_dir().join(format!("karasu-window-arrange-{}", std::process::id()));
        let mut window = MainWindow::new(Vec::new());
        window.workspace = crate::slicing_ui::tests::hollowed_cube(&folder);
        window.follow_plate(&ctx);
        assert!(window.enabled(Action::ArrangeModels));
        window.perform(Action::ArrangeModels);
        let spacing = window.workspace.plate().settings.spacing_mm;
        assert_eq!(window.plates.arranging, Some(spacing));
        let plate = window.workspace.plate().name.clone();
        let mut texts = Vec::new();
        for _ in 0..2 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                window.arrange_dialog(&ui.ctx().clone());
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
            tr!("arrange-title", plate = plate.as_str()),
            tr!("arrange-overflow-new"),
            tr!("arrange-overflow-stay"),
            tr!("arrange-action"),
        ] {
            assert!(texts.contains(&expected), "{expected} in {texts:?}");
        }
        assert_eq!(window.plates.arranging, Some(spacing));
        window.plates.arranging = None;
        // Arranged: the one model fits, nothing goes elsewhere.
        window.selected = Some(0);
        window.arrange(spacing, Overflow::NewPlates);
        let latest = window.messages.latest().unwrap();
        assert_eq!(
            latest.text,
            tr!("arrange-done", count = 1usize, plate = plate.as_str())
        );
        assert_eq!(latest.detail, Some(tr!("arrange-undo")));
        assert_eq!(window.selected, Some(0));
        assert_eq!(window.workspace.plates().len(), 1);
        // The choice is kept across restarts.
        window.plates.choices.arrange_overflow = Overflow::Stay;
        let settings = window.collect_settings(&ctx);
        assert_eq!(settings.plates.arrange_overflow, Overflow::Stay);
        // An empty plate has nothing to arrange.
        window.perform(Action::NewPlate);
        window.follow_plate(&ctx);
        assert!(!window.enabled(Action::ArrangeModels));
        let _ = std::fs::remove_dir_all(&folder);
    }
}
