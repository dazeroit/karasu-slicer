//! Help › About karasu-slicer (owner, 2026-11-11, TD-145): a modal dialog with the program's
//! name and version, the copyright, the licence's notice (the AGPL's "Appropriate Legal
//! Notices" an interactive interface shows: the licence, no warranty, where the source is) and
//! the trademark note. Enter or Escape closes it.
use crate::{
    dialog::{self, Part, Size},
    i18n::tr,
    window::MainWindow,
};
use eframe::egui;
use karasu_core::{
    APPLICATION_DESCRIPTION, APPLICATION_NAME, COPYRIGHT, LEGAL_NOTICE, LICENCE, LICENCE_URL,
    SOURCE_URL,
};

impl MainWindow {
    /// Help › About karasu-slicer, while it is open.
    pub(super) fn about_dialog(&mut self, ctx: &egui::Context) {
        if !self.about_open {
            return;
        }
        let mut close = false;
        let response = dialog::show(
            ctx,
            "about",
            &tr!("about-title"),
            Size::Fixed(460.0),
            |ui, part| match part {
                Part::Body => about_body(ui),
                Part::Footer { enter } => {
                    if dialog::main_action(ui, &tr!("action-close"), true, enter) {
                        close = true;
                    }
                }
            },
        );
        if close || response.cancel {
            self.about_open = false;
        }
    }
}

/// The dialog's text: who, what, the licence and its links, the trademark.
fn about_body(ui: &mut egui::Ui) {
    let weak = ui.visuals().weak_text_color();
    ui.label(
        egui::RichText::new(format!("{APPLICATION_NAME} {}", env!("CARGO_PKG_VERSION"))).heading(),
    );
    ui.label(APPLICATION_DESCRIPTION);
    ui.add_space(8.0);
    ui.label(COPYRIGHT);
    ui.add_space(4.0);
    ui.add(egui::Label::new(egui::RichText::new(LEGAL_NOTICE).small()).wrap());
    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        ui.label(tr!("about-licence", licence = LICENCE));
        ui.hyperlink_to(tr!("about-licence-text"), LICENCE_URL);
    });
    ui.horizontal_wrapped(|ui| {
        ui.label(tr!("about-source"));
        ui.hyperlink_to(SOURCE_URL, SOURCE_URL);
    });
    ui.add_space(8.0);
    ui.add(egui::Label::new(egui::RichText::new(tr!("about-trademark")).color(weak)).wrap());
    ui.add(egui::Label::new(egui::RichText::new(tr!("about-notices")).color(weak)).wrap());
}

#[cfg(test)]
mod tests {
    use crate::{actions::Action, i18n::tr, window::MainWindow};
    use eframe::egui;

    #[test]
    fn about_shows_the_copyright_the_licence_and_no_warranty_and_closes() {
        let ctx = egui::Context::default();
        let mut window = MainWindow::new(Vec::new());
        assert!(window.enabled(Action::About));
        window.perform(Action::About);
        let mut texts = Vec::new();
        for _ in 0..2 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                window.about_dialog(&ui.ctx().clone());
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
        let all = texts.join("\n");
        for expected in [
            tr!("about-title"),
            karasu_core::COPYRIGHT.to_owned(),
            "WITHOUT ANY WARRANTY".to_owned(),
            karasu_core::SOURCE_URL.to_owned(),
        ] {
            assert!(all.contains(&expected), "{expected} in {all}");
        }
        // Escape closes it.
        let escape = egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        let mut output = ctx.run_ui(
            egui::RawInput {
                events: vec![escape],
                ..Default::default()
            },
            |ui| window.about_dialog(&ui.ctx().clone()),
        );
        output.textures_delta.clear();
        assert!(!window.about_open);
    }
}
