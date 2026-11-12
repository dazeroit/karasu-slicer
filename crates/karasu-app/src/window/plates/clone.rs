//! Clone (owner, 2026-11-10): a model's menu, in the sidebar or the viewport, adds copies of
//! the model. A modal dialog asks how many, onto which plate (the one shown to start with,
//! another, or a new one), and whether to arrange them: on by default, the copies packed around
//! the models already on the plate, which stay where they are, those that do not fit going to
//! new plates after it; off, every copy goes to the middle of the plate. One undoable step; a
//! notification says where the copies went.
use crate::{
    dialog::{self, Part, Size},
    i18n::tr,
    messages::{Level, Message},
    widgets::{default_switch, value_row, Unit, UnitField},
    window::MainWindow,
    workspace::{Cloned, Refused},
};
use eframe::egui;
use karasu_project::project::PlateId;

/// How many copies one Clone adds at most.
const COPIES: std::ops::RangeInclusive<f64> = 1.0..=100.0;

/// Clone's dialog, while it is open.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::window) struct CloneForm {
    /// The model cloned, by its place on the plate shown.
    index: usize,
    /// How many copies, a whole number in [`COPIES`].
    copies: f64,
    /// The plate the copies go to; `None`: a new plate after the plate shown.
    to: Option<PlateId>,
    /// The copies are arranged around the models there, or put in the middle of the plate.
    arranged: bool,
}

impl MainWindow {
    /// Open Clone for model `index` of the plate shown: one copy, onto the plate shown,
    /// arranged.
    pub(in crate::window) fn open_clone(&mut self, index: usize) {
        if index < self.workspace.plate().objects.len() {
            self.plates.cloning = Some(CloneForm {
                index,
                copies: 1.0,
                to: Some(self.workspace.plate_id()),
                arranged: true,
            });
        }
    }

    /// The plates' model dialogs, while open: Arrange models… and Clone.
    pub(in crate::window) fn plate_dialogs(&mut self, ctx: &egui::Context) {
        self.arrange_dialog(ctx);
        self.clone_dialog(ctx);
    }

    /// Clone, while it is open: modal, Enter clones, Escape cancels.
    pub(in crate::window) fn clone_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.plates.cloning.take() else {
            return;
        };
        let Some(model) = self
            .workspace
            .plate()
            .objects
            .get(form.index)
            .map(|o| o.name.clone())
        else {
            return;
        };
        let title = tr!("clone-title", model = &model);
        let mut done = None;
        let response = dialog::show(
            ctx,
            "clone-model",
            &title,
            Size::Fixed(400.0),
            |ui, part| match part {
                Part::Body => self.clone_body(ui, &mut form),
                Part::Footer { enter } => {
                    if dialog::main_action(ui, &tr!("clone-action"), true, enter) {
                        done = Some(true);
                    }
                    if dialog::cancel_action(ui) {
                        done = Some(false);
                    }
                }
            },
        );
        match done {
            Some(true) => self.clone_model(&form),
            Some(false) => {}
            None if response.cancel => {}
            None => self.plates.cloning = Some(form),
        }
    }

    /// The dialog's fields: the copies, the plate and the arrangement.
    fn clone_body(&self, ui: &mut egui::Ui, form: &mut CloneForm) {
        value_row(ui, tr!("clone-copies"), |ui| {
            ui.add(
                UnitField::new(&mut form.copies, Unit::Count)
                    .range(COPIES)
                    .step(1.0)
                    .decimals(0),
            )
        });
        let shown = self.workspace.plate_id();
        let label = |to: Option<PlateId>| match to {
            None => tr!("action-new-plate"),
            Some(id) => {
                let plates = self.workspace.plates();
                let number = plates
                    .iter()
                    .position(|p| p.id() == id)
                    .map_or(0, |k| k + 1);
                let name = plates
                    .iter()
                    .find(|p| p.id() == id)
                    .map_or_default(|p| p.content().name.clone());
                let name = tr!("plates-overview-name", number = number, name = name);
                if id == shown {
                    tr!("clone-plate-shown", plate = name)
                } else {
                    name
                }
            }
        };
        value_row(ui, tr!("clone-plate"), |ui| {
            egui::ComboBox::from_id_salt("clone-plate")
                .width(ui.available_width())
                .selected_text(label(form.to))
                .show_ui(ui, |ui| {
                    let plates: Vec<PlateId> = self
                        .workspace
                        .plates()
                        .iter()
                        .map(karasu_project::project::Plate::id)
                        .collect();
                    for id in plates {
                        ui.selectable_value(&mut form.to, Some(id), label(Some(id)));
                    }
                    ui.separator();
                    ui.selectable_value(&mut form.to, None, label(None));
                })
                .response
        });
        ui.add_space(4.0);
        default_switch(
            ui,
            &mut form.arranged,
            (tr!("clone-arrange"), tr!("clone-arrange-hint")),
            None,
        );
    }

    /// Add `form`'s copies and say where they went.
    fn clone_model(&mut self, form: &CloneForm) {
        let Some(model) = self
            .workspace
            .plate()
            .objects
            .get(form.index)
            .map(|o| o.name.clone())
        else {
            return;
        };
        let name = self.new_plate_name();
        let plate = form
            .to
            .and_then(|id| self.workspace.project().plate(id))
            .map_or_else(|| name.clone(), |p| p.content().name.clone());
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // 1 to 100.
        let copies = form.copies.round().clamp(*COPIES.start(), *COPIES.end()) as usize;
        let message =
            match self
                .workspace
                .clone_object(form.index, copies, form.to, form.arranged, name)
            {
                Ok(cloned) => cloned_message(&model, &plate, &cloned),
                Err(Refused::NoRoom) => Message::new(
                    Level::Warning,
                    tr!("plates-transfer-no-room", model = &model, plate = &plate),
                ),
                Err(Refused::Loading) => Message::new(Level::Info, tr!("plates-transfer-loading")),
                Err(Refused::Unknown) => return,
            };
        self.messages.push(message);
    }
}

/// The notification of copies of `model` added to `plate`: how many, and those on new plates.
fn cloned_message(model: &str, plate: &str, cloned: &Cloned) -> Message {
    let text = tr!(
        "clone-done",
        count = cloned.on_plate,
        model = model,
        plate = plate
    );
    let undo = tr!("clone-undo");
    if cloned.moved > 0 {
        Message::new(Level::Success, text).detail(format!(
            "{} {undo}",
            tr!(
                "arrange-moved",
                count = cloned.moved,
                plates = cloned.new_plates.join(", "),
                new = cloned.new_plates.len()
            )
        ))
    } else {
        Message::new(Level::Success, text).detail(undo)
    }
}

#[cfg(test)]
mod tests {
    use super::CloneForm;
    use crate::{i18n::tr, window::MainWindow};
    use eframe::egui;

    #[test]
    fn clone_asks_how_many_and_where_then_says_where_they_went() {
        // Owner, 2026-11-10: one copy, onto the plate shown, arranged, to start with.
        let ctx = egui::Context::default();
        let folder =
            std::env::temp_dir().join(format!("karasu-window-clone-{}", std::process::id()));
        let mut window = MainWindow::new(Vec::new());
        window.workspace = crate::slicing_ui::tests::hollowed_cube(&folder);
        window.follow_plate(&ctx);
        window.sidebar_request(crate::sidebar::Request::Clone(0));
        let shown = window.workspace.plate_id();
        assert_eq!(
            window.plates.cloning,
            Some(CloneForm {
                index: 0,
                copies: 1.0,
                to: Some(shown),
                arranged: true,
            })
        );
        let model = window.workspace.plate().objects[0].name.clone();
        let plate = window.workspace.plate().name.clone();
        let mut texts = Vec::new();
        for _ in 0..2 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                window.clone_dialog(&ui.ctx().clone());
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
            tr!("clone-title", model = model.as_str()),
            tr!("clone-copies"),
            tr!("clone-arrange"),
            tr!("clone-action"),
        ] {
            assert!(texts.contains(&expected), "{expected} in {texts:?}");
        }
        assert!(window.plates.cloning.is_some());
        // Three copies: on the plate shown, said in a notification.
        let form = CloneForm {
            copies: 3.0,
            ..window.plates.cloning.take().unwrap()
        };
        window.clone_model(&form);
        assert_eq!(window.workspace.plate().objects.len(), 4);
        let latest = window.messages.latest().unwrap();
        assert_eq!(
            latest.text,
            tr!(
                "clone-done",
                count = 3usize,
                model = model.as_str(),
                plate = plate.as_str()
            )
        );
        assert_eq!(latest.detail, Some(tr!("clone-undo")));
        // One step undoes them all.
        assert!(window.workspace.undo());
        assert_eq!(window.workspace.plate().objects.len(), 1);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
