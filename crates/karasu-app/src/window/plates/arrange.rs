//! Arrange models… (TD-135, phase 5, from the owner-approved mockup "Arrange with the overflow
//! to a new plate"): a modal dialog that arranges the plate shown's models with the packer
//! (from the plate's middle outwards), their footprints with supports and rafts, the spacing
//! between them the plate's own to start with. The models that do not fit go to new plates after it, with its printer,
//! resin, print setting and slicing options, arranged the same way, as many as needed; or, by
//! choice, stay where they are. With the overflow to new plates, "Group by height" (TD-162, on
//! by default) puts models of similar height on one plate when that shortens the plates'
//! estimated time. "Turn about Z" (TD-162, off by default) lets models turn about the plate's
//! vertical, by any angle, to take less room. The choices are kept across restarts; the spacing
//! is not (it is the plate's). One undoable step; a notification says what happened.
//!
//! **On a thread of its own** (owner, 2026-11-19): Arrange measures every model, which takes
//! long on large ones, so it works beside the window. The dialog stays, its controls disabled,
//! with a progress bar after a moment and only Cancel; done, it closes and the notification
//! comes. Cancelled, or with the plate changed meanwhile, nothing changes.
//!
//! **Every plate** (TD-162, phase 3; owner, 2026-11-19): the dialog's first choice, "This plate"
//! or "Every plate", which the Plate menu's "Arrange every plate" opens on. With it, "Same
//! printer" or "All plates" says which plates it takes, "Remove plates left empty" whether the
//! plates it empties go, and a line says which plates and models it takes and which plates it
//! leaves out and why. The choices of which plates and of removing are kept across restarts.
use crate::{
    dialog::{self, Part, Size},
    i18n::tr,
    messages::{Level, Message},
    widgets::{choice, value_row, Unit, UnitField},
    window::MainWindow,
    workspace::{
        ArrangeJob, ArrangeOptions, ArrangeScope, ArrangeStage, Arranged, Footprint, LeftOut,
        Overflow, Planned, PlateChoice, Unarranged,
    },
};
use eframe::egui;
use std::time::{Duration, Instant};

/// Arrange working on a thread of its own, its dialog showing how far it has gone.
pub(in crate::window) struct Running {
    job: ArrangeJob,
    started: Instant,
    /// The plate's name, for the notification.
    plate: String,
    /// How many models the plate shown held: "Every plate" forgets them all.
    shown_models: usize,
}

/// The spacing Arrange accepts, in mm.
const SPACING_MM: std::ops::RangeInclusive<f64> = 0.0..=50.0;

impl MainWindow {
    /// Open Arrange models… on `scope`, its spacing the plate shown's.
    pub(super) fn open_arrange(&mut self, scope: ArrangeScope) {
        self.plates.arranging = Some((self.workspace.plate().settings.spacing_mm, scope));
    }

    /// Arrange models…, while it is open: modal, Enter arranges, Escape cancels; while it works,
    /// its progress and Cancel, until it is done.
    pub(in crate::window) fn arrange_dialog(&mut self, ctx: &egui::Context) {
        let Some((mut spacing, mut scope)) = self.plates.arranging.take() else {
            return;
        };
        // Working: done once its result arrives, the dialog closed.
        if let Some(result) = self.plates.arrange_job.as_ref().and_then(|r| r.job.poll()) {
            if let Some(running) = self.plates.arrange_job.take() {
                self.finish_arrange(&running, result);
            }
            return;
        }
        let working = self
            .plates
            .arrange_job
            .as_ref()
            .map(|r| (r.job.progress(), r.started.elapsed()));
        if working.is_some() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        let mut overflow = self.plates.choices.arrange_overflow;
        let mut by_height = self.plates.choices.arrange_by_height;
        let mut turn = self.plates.choices.arrange_turn;
        let mut which = self.plates.choices.arrange_plates;
        let mut remove_empty = self.plates.choices.arrange_remove_empty;
        let mut footprint = self.plates.choices.arrange_footprint;
        let (models, summary) = self.arrange_summary(scope, which);
        let title = tr!("arrange-title", plate = &self.workspace.plate().name);
        let mut done = None;
        let response = dialog::show(
            ctx,
            "arrange-models",
            &title,
            Size::Fixed(400.0),
            |ui, part| match part {
                Part::Body => {
                    let mut choices = Choices {
                        scope: &mut scope,
                        which: &mut which,
                        remove_empty: &mut remove_empty,
                        spacing: &mut spacing,
                        overflow: &mut overflow,
                        by_height: &mut by_height,
                        turn: &mut turn,
                        footprint: &mut footprint,
                    };
                    arrange_body(ui, &mut choices, (models, summary.as_deref()), working);
                }
                Part::Footer { enter } => {
                    // Working, only Cancel.
                    if working.is_none()
                        && dialog::main_action(ui, &tr!("arrange-action"), models > 0, enter)
                    {
                        done = Some(true);
                    }
                    if dialog::cancel_action(ui) {
                        done = Some(false);
                    }
                }
            },
        );
        if working.is_some() {
            if done == Some(false) || response.cancel {
                self.cancel_arrange();
            } else {
                self.plates.arranging = Some((spacing, scope));
            }
            return;
        }
        let choices = &mut self.plates.choices;
        choices.arrange_overflow = overflow;
        choices.arrange_by_height = by_height;
        choices.arrange_turn = turn;
        choices.arrange_plates = which;
        choices.arrange_remove_empty = remove_empty;
        choices.arrange_footprint = footprint;
        match done {
            Some(true) => {
                let options = ArrangeOptions {
                    spacing_mm: spacing,
                    overflow,
                    by_height,
                    turn,
                    scope,
                    plates: which,
                    remove_empty,
                    footprint,
                };
                if self.start_arrange(options) {
                    self.plates.arranging = Some((spacing, scope));
                }
            }
            Some(false) => {}
            None if response.cancel => {}
            None => self.plates.arranging = Some((spacing, scope)),
        }
    }

    /// The models Arrange takes with `scope` and `which` plates as they stand, and with every
    /// plate the line saying which plates and models it takes and which plates it leaves out.
    fn arrange_summary(&self, scope: ArrangeScope, which: PlateChoice) -> (usize, Option<String>) {
        let (taken, left) = self.workspace.arranged_plates(scope, which);
        let project = self.workspace.project();
        let models: usize = taken
            .iter()
            .filter_map(|&id| project.plate(id))
            .map(|p| p.content().objects.len())
            .sum();
        if scope == ArrangeScope::ThisPlate {
            return (models, None);
        }
        let mut line = tr!("arrange-every-takes", plates = taken.len(), models = models);
        if !left.is_empty() {
            let out: Vec<String> = left
                .iter()
                .map(|&(id, why)| {
                    let name = project
                        .plate(id)
                        .map_or_else(String::new, |p| p.content().name.clone());
                    let why = match why {
                        LeftOut::NoPrinter => tr!("arrange-left-no-printer"),
                        LeftOut::AnotherPrinter => tr!("arrange-left-another-printer"),
                    };
                    format!("{name}: {why}")
                })
                .collect();
            line = format!(
                "{line} · {}",
                tr!("arrange-left-out", plates = out.join("; "))
            );
        }
        (models, Some(line))
    }

    /// Start arranging the plate shown with `options` on a thread of its own. Returns whether
    /// it started; otherwise a notification says why.
    pub(in crate::window) fn start_arrange(&mut self, options: ArrangeOptions) -> bool {
        match self.workspace.arrange_input(options) {
            Ok(input) => {
                self.plates.arrange_job = Some(Running {
                    job: ArrangeJob::start(input),
                    started: Instant::now(),
                    plate: self.workspace.plate().name.clone(),
                    shown_models: self.workspace.plate().objects.len(),
                });
                true
            }
            Err(why) => {
                self.messages.push(unarranged_message(why));
                false
            }
        }
    }

    /// Stop the arrangement working: nothing changes, and a notification says so.
    pub(in crate::window) fn cancel_arrange(&mut self) {
        if let Some(running) = self.plates.arrange_job.take() {
            running.job.cancel();
            self.messages
                .push(unarranged_message(Unarranged::Cancelled));
        }
    }

    /// Make the arrangement `result` that `running` planned, and say what happened.
    fn finish_arrange(&mut self, running: &Running, result: Result<Planned, Unarranged>) {
        let message = match result.and_then(|planned| self.workspace.finish_arrange(planned)) {
            Ok(arranged) => {
                // From the last, so that each index is still the model's; with every plate,
                // the plate shown's models were all arranged again, and are all forgotten.
                let gone: Vec<usize> = if arranged.every.is_some() {
                    (0..running.shown_models).collect()
                } else {
                    arranged.removed.clone()
                };
                for &index in gone.iter().rev() {
                    self.forget_object(index);
                }
                arranged_message(&running.plate, &arranged)
            }
            Err(why) => unarranged_message(why),
        };
        self.messages.push(message);
    }
}

/// The dialog's choices, edited in its body.
struct Choices<'a> {
    scope: &'a mut ArrangeScope,
    which: &'a mut PlateChoice,
    remove_empty: &'a mut bool,
    spacing: &'a mut f64,
    overflow: &'a mut Overflow,
    by_height: &'a mut bool,
    turn: &'a mut bool,
    footprint: &'a mut Footprint,
}

/// The dialog's body: its choices for the plate's `models`, disabled while it works, with its
/// progress (`working`: how far, and for how long) once it takes a moment.
fn arrange_body(
    ui: &mut egui::Ui,
    choices: &mut Choices<'_>,
    (models, summary): (usize, Option<&str>),
    working: Option<((ArrangeStage, usize, usize), Duration)>,
) {
    let Choices {
        scope,
        which,
        remove_empty,
        spacing,
        overflow,
        by_height,
        turn,
        footprint,
    } = choices;
    ui.add_enabled_ui(working.is_none(), |ui| {
        // This plate, or every plate (TD-162, phase 3).
        ui.label(tr!("arrange-scope"));
        let mut chosen = usize::from(**scope == ArrangeScope::EveryPlate);
        let labels = [tr!("arrange-scope-this"), tr!("arrange-scope-every")];
        if choice(ui, &mut chosen, &labels).changed() {
            **scope = if chosen == 0 {
                ArrangeScope::ThisPlate
            } else {
                ArrangeScope::EveryPlate
            };
        }
        if **scope == ArrangeScope::EveryPlate {
            every_rows(ui, which, remove_empty, summary);
        }
        ui.add_space(4.0);
        // The room each model takes (TD-165): its rectangle, or its outline.
        ui.label(tr!("arrange-footprint"));
        let mut chosen = usize::from(**footprint == Footprint::Outline);
        let labels = [
            tr!("arrange-footprint-box"),
            tr!("arrange-footprint-outline"),
        ];
        if choice(ui, &mut chosen, &labels)
            .on_hover_text(tr!("arrange-footprint-hint"))
            .changed()
        {
            **footprint = if chosen == 0 {
                Footprint::Box
            } else {
                Footprint::Outline
            };
        }
        ui.add_space(4.0);
        value_row(ui, tr!("arrange-spacing"), |ui| {
            ui.add(UnitField::new(spacing, Unit::Millimetre).range(SPACING_MM))
        });
        if **scope == ArrangeScope::ThisPlate {
            value_row(ui, tr!("arrange-models"), |ui| {
                ui.label(tr!("arrange-models-all", count = models))
            });
        }
        ui.add_space(4.0);
        ui.label(tr!("arrange-overflow"));
        let mut chosen = usize::from(**overflow == Overflow::Stay);
        let labels = [tr!("arrange-overflow-new"), tr!("arrange-overflow-stay")];
        if choice(ui, &mut chosen, &labels).changed() {
            **overflow = if chosen == 0 {
                Overflow::NewPlates
            } else {
                Overflow::Stay
            };
        }
        // Only plates that are added can be grouped (TD-162).
        if **overflow == Overflow::NewPlates {
            ui.add_space(4.0);
            crate::widgets::default_switch(
                ui,
                by_height,
                (tr!("arrange-by-height"), tr!("arrange-by-height-hint")),
                None,
            );
        }
        // Turning changes the orientation the user chose: off to start with.
        ui.add_space(4.0);
        crate::widgets::default_switch(
            ui,
            turn,
            (tr!("arrange-turn"), tr!("arrange-turn-hint")),
            None,
        );
        ui.add_space(4.0);
        let hint = match **overflow {
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
        // Its progress, once it takes a moment, as a slice's.
        if let Some(((stage, measured, count), elapsed)) = working {
            if elapsed >= crate::slicing_ui::BUSY_DELAY {
                progress_line(ui, stage, measured, count);
            }
        }
    });
}

/// The rows "Every plate" adds: which plates, removing those left empty, and what it takes.
fn every_rows(
    ui: &mut egui::Ui,
    which: &mut PlateChoice,
    remove_empty: &mut bool,
    summary: Option<&str>,
) {
    ui.add_space(4.0);
    ui.label(tr!("arrange-plates"));
    let mut chosen = usize::from(*which == PlateChoice::All);
    let labels = [tr!("arrange-plates-same"), tr!("arrange-plates-all")];
    if choice(ui, &mut chosen, &labels).changed() {
        *which = if chosen == 0 {
            PlateChoice::SamePrinter
        } else {
            PlateChoice::All
        };
    }
    ui.add_space(4.0);
    crate::widgets::default_switch(
        ui,
        remove_empty,
        (
            tr!("arrange-remove-empty"),
            tr!("arrange-remove-empty-hint"),
        ),
        None,
    );
    if let Some(summary) = summary {
        ui.add(
            egui::Label::new(
                egui::RichText::new(summary)
                    .small()
                    .color(ui.visuals().weak_text_color()),
            )
            .wrap(),
        );
    }
}

/// The progress of an arrangement working: a bar, and what it does.
fn progress_line(ui: &mut egui::Ui, stage: ArrangeStage, measured: usize, count: usize) {
    ui.add_space(6.0);
    let (text, share) = match stage {
        ArrangeStage::Measuring => {
            #[allow(clippy::cast_precision_loss)] // Model counts.
            let share = measured as f32 / count.max(1) as f32;
            (
                tr!(
                    "arrange-measuring",
                    number = (measured + 1).min(count),
                    count = count
                ),
                share,
            )
        }
        ArrangeStage::Packing => (tr!("arrange-packing"), 1.0),
    };
    ui.label(text);
    ui.add(
        egui::ProgressBar::new(share)
            .desired_height(6.0)
            .animate(stage == ArrangeStage::Packing),
    );
}

/// The notification of an arrangement that was not made.
fn unarranged_message(why: Unarranged) -> Message {
    match why {
        Unarranged::NoPrinter => Message::new(Level::Warning, tr!("arrange-no-printer")),
        Unarranged::Loading => Message::new(Level::Info, tr!("arrange-loading")),
        Unarranged::Changed => Message::new(Level::Warning, tr!("arrange-changed")),
        Unarranged::Cancelled => Message::new(Level::Info, tr!("arrange-cancelled")),
    }
}

/// The notification of an arrangement of plate `plate`: how many models it holds now, and
/// where the others went or that they stayed.
fn arranged_message(plate: &str, arranged: &Arranged) -> Message {
    let text = match &arranged.every {
        Some((models, plates, _)) => tr!(
            "arrange-every-done",
            models = *models - arranged.stayed,
            plates = *plates + arranged.new_plates.len()
        ),
        None => tr!("arrange-done", count = arranged.on_plate, plate = plate),
    };
    // What else it did, in turn, then how to undo it.
    let mut detail = Vec::new();
    if arranged.moved > 0 {
        detail.push(tr!(
            "arrange-moved",
            count = arranged.moved,
            plates = arranged.new_plates.join(", "),
            new = arranged.new_plates.len()
        ));
    }
    if let Some((_, _, emptied)) = arranged.every.as_ref().filter(|e| !e.2.is_empty()) {
        detail.push(tr!(
            "arrange-every-removed",
            count = emptied.len(),
            plates = emptied.join(", ")
        ));
    }
    if arranged.stayed > 0 {
        detail.push(tr!("arrange-stayed", count = arranged.stayed));
    }
    // Grouped by height: what it saved (TD-162).
    match (arranged.grouped, arranged.saved_s) {
        (false, _) => {}
        (true, 0) => detail.push(tr!("arrange-grouped-fewer")),
        #[allow(clippy::cast_precision_loss)] // Seconds of a print.
        (true, saved) => detail.push(tr!(
            "arrange-grouped-saved",
            saved = crate::slicing_ui::duration(saved as f64)
        )),
    }
    if arranged.turned > 0 {
        detail.push(tr!("arrange-turned", count = arranged.turned));
    }
    detail.push(tr!("arrange-undo"));
    let level = if arranged.stayed > 0 {
        Level::Warning
    } else {
        Level::Success
    };
    Message::new(level, text).detail(detail.join(" "))
}

#[cfg(test)]
mod tests {
    use super::{arranged_message, unarranged_message};
    use crate::workspace::ArrangeOptions;

    impl MainWindow {
        /// Arrange the plate shown with `options` on this thread, and say what happened: the
        /// tests' way, where the window starts [`Self::start_arrange`].
        pub(in crate::window) fn arrange(&mut self, options: ArrangeOptions) {
            let plate = self.workspace.plate().name.clone();
            let message = match self.workspace.arrange(options) {
                Ok(arranged) => {
                    // From the last, so that each index is still the model's.
                    for &index in arranged.removed.iter().rev() {
                        self.forget_object(index);
                    }
                    arranged_message(&plate, &arranged)
                }
                Err(why) => unarranged_message(why),
            };
            self.messages.push(message);
        }
    }

    use crate::{actions::Action, i18n::tr, window::MainWindow, workspace::Overflow};
    use eframe::egui;

    #[test]
    fn arrange_works_beside_the_window_and_cancels() {
        // Owner, 2026-11-19: the dialog stays while Arrange works on its own thread, its
        // controls disabled with Cancel; done, it closes and the notification comes.
        let ctx = egui::Context::default();
        let folder =
            std::env::temp_dir().join(format!("karasu-window-arrange-job-{}", std::process::id()));
        let mut window = MainWindow::new(Vec::new());
        window.workspace = crate::slicing_ui::tests::hollowed_cube(&folder);
        window.follow_plate(&ctx);
        let plate = window.workspace.plate().name.clone();
        let options = crate::workspace::ArrangeOptions {
            spacing_mm: 2.0,
            overflow: Overflow::NewPlates,
            by_height: true,
            turn: true,
            scope: crate::workspace::ArrangeScope::ThisPlate,
            plates: crate::workspace::PlateChoice::SamePrinter,
            remove_empty: false,
            footprint: crate::workspace::Footprint::Box,
        };
        let frame = |window: &mut MainWindow| {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                window.arrange_dialog(&ui.ctx().clone());
            });
            output.textures_delta.clear();
        };
        // Started from the dialog: it stays open until the result arrives, then closes.
        window.perform(Action::ArrangeModels);
        assert!(window.start_arrange(options));
        let started = std::time::Instant::now();
        while window.plates.arranging.is_some() {
            frame(&mut window);
            assert!(started.elapsed().as_secs() < 60, "still arranging");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(window.plates.arrange_job.is_none());
        let latest = window.messages.latest().unwrap();
        assert_eq!(
            latest.text,
            tr!("arrange-done", count = 1usize, plate = plate.as_str())
        );
        // Cancelled: nothing changes, and it is said.
        let revision = window.workspace.revision();
        window.perform(Action::ArrangeModels);
        assert!(window.start_arrange(options));
        window.cancel_arrange();
        assert!(window.plates.arrange_job.is_none());
        assert_eq!(window.workspace.revision(), revision);
        assert_eq!(
            window.messages.latest().unwrap().text,
            tr!("arrange-cancelled")
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

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
        assert_eq!(
            window.plates.arranging,
            Some((spacing, crate::workspace::ArrangeScope::ThisPlate))
        );
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
            tr!("arrange-by-height"),
            tr!("arrange-turn"),
            tr!("arrange-action"),
        ] {
            assert!(texts.contains(&expected), "{expected} in {texts:?}");
        }
        // Grouping by height shows only with the overflow to new plates (TD-162).
        window.plates.choices.arrange_overflow = Overflow::Stay;
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
        assert!(!texts.contains(&tr!("arrange-by-height")), "{texts:?}");
        assert!(texts.contains(&tr!("arrange-turn")), "{texts:?}");
        window.plates.choices.arrange_overflow = Overflow::NewPlates;
        assert_eq!(
            window.plates.arranging,
            Some((spacing, crate::workspace::ArrangeScope::ThisPlate))
        );
        window.plates.arranging = None;
        // Arranged: the one model fits, nothing goes elsewhere.
        window.selected = Some(0);
        window.arrange(crate::workspace::ArrangeOptions {
            spacing_mm: spacing,
            overflow: Overflow::NewPlates,
            by_height: true,
            turn: false,
            scope: crate::workspace::ArrangeScope::ThisPlate,
            plates: crate::workspace::PlateChoice::SamePrinter,
            remove_empty: false,
            footprint: crate::workspace::Footprint::Box,
        });
        let latest = window.messages.latest().unwrap();
        assert_eq!(
            latest.text,
            tr!("arrange-done", count = 1usize, plate = plate.as_str())
        );
        assert_eq!(latest.detail, Some(tr!("arrange-undo")));
        assert_eq!(window.selected, Some(0));
        assert_eq!(window.workspace.plates().len(), 1);
        // The choices are kept across restarts; grouping by height is on to start with, turning
        // off (owner, 2026-11-19).
        assert!(window.plates.choices.arrange_by_height);
        assert!(!window.plates.choices.arrange_turn);
        window.plates.choices.arrange_overflow = Overflow::Stay;
        window.plates.choices.arrange_by_height = false;
        window.plates.choices.arrange_turn = true;
        let settings = window.collect_settings(&ctx);
        assert_eq!(settings.plates.arrange_overflow, Overflow::Stay);
        assert!(!settings.plates.arrange_by_height);
        assert!(settings.plates.arrange_turn);
        // An empty plate has nothing to arrange.
        window.perform(Action::NewPlate);
        window.follow_plate(&ctx);
        assert!(!window.enabled(Action::ArrangeModels));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn arrange_every_plate_opens_the_same_dialog_with_its_own_rows() {
        // TD-162, phase 3: which plates, removing those left empty, and what it takes; the
        // choices kept across restarts.
        let ctx = egui::Context::default();
        let folder = std::env::temp_dir().join(format!(
            "karasu-window-arrange-every-{}",
            std::process::id()
        ));
        let mut window = MainWindow::new(Vec::new());
        window.workspace = crate::slicing_ui::tests::hollowed_cube(&folder);
        window.follow_plate(&ctx);
        // Arrange every plate opens the same dialog on every plate, with its own rows: which
        // plates, removing those left empty, and what it takes (TD-162, phase 3).
        assert!(window.enabled(Action::ArrangeEveryPlate));
        window.perform(Action::ArrangeEveryPlate);
        assert_eq!(
            window.plates.arranging.map(|(_, scope)| scope),
            Some(crate::workspace::ArrangeScope::EveryPlate)
        );
        let texts = shown_texts(&mut window, &ctx);
        for expected in [
            tr!("arrange-scope-every"),
            tr!("arrange-plates-same"),
            tr!("arrange-plates-all"),
            tr!("arrange-remove-empty"),
            tr!("arrange-every-takes", plates = 1usize, models = 1usize),
        ] {
            assert!(texts.contains(&expected), "{expected} in {texts:?}");
        }
        window.plates.arranging = None;
        window.perform(Action::ArrangeModels);
        let texts = shown_texts(&mut window, &ctx);
        assert!(!texts.contains(&tr!("arrange-remove-empty")), "{texts:?}");
        window.plates.arranging = None;
        window.plates.choices.arrange_plates = crate::workspace::PlateChoice::All;
        window.plates.choices.arrange_remove_empty = true;
        let settings = window.collect_settings(&ctx);
        assert_eq!(
            settings.plates.arrange_plates,
            crate::workspace::PlateChoice::All
        );
        assert!(settings.plates.arrange_remove_empty);
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// The texts the Arrange dialog draws, after two frames.
    fn shown_texts(window: &mut MainWindow, ctx: &egui::Context) -> Vec<String> {
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
        texts
    }
}
