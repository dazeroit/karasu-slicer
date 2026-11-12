//! The suction cups in the Drain holes panel and its list window (M6.4j, TD-114; owner mockup
//! "Suction cups in the Drain holes tool", 2026-10-31): the search's minimum vent and display
//! filter, what it found, and a navigator through the cups shown with Add a vent here; the
//! footer's Find suction cups; the list window's second tab.
use super::HollowingUi;
use crate::{
    appearance::Appearance,
    hollowing_ui::cups::{
        Cup, DEFAULT_SHOW_FROM_MM2, DEFAULT_SHOW_TALLER_MM, MIN_VENT_MM2, SHOW_FROM_MM2,
        SHOW_TALLER_MM,
    },
    i18n::{decimal, tr},
    icons::{Icon, Icons},
    list_window::{self, Event, Row, Spec},
    preparation_ui::PreparationUi,
    theme,
    widgets::{default_row, fact_row, icon_button, length_default, Unit, UnitField},
    workspace::Workspace,
};
use eframe::egui;
use karasu_core::analysis::suction::Opening;

/// The colour a cup's marker takes, by its openings.
fn marker_colour(cup: &Cup, appearance: &Appearance) -> egui::Color32 {
    match cup.opening {
        Opening::None => appearance.suction_cup.color(),
        Opening::TooSmall => appearance.suction_cup_narrow.color(),
    }
}

/// A cup's state in the list: it traps its resin, or how it is open.
fn state(cup: &Cup) -> String {
    match (cup.closes, cup.opening) {
        (true, _) => tr!("cups-state-traps"),
        (false, Opening::None) => tr!("cups-state-none"),
        (false, Opening::TooSmall) => tr!("cups-state-small"),
    }
}

impl HollowingUi {
    /// The Suction cups section of model `index`: the search's values, what it found and the
    /// chosen cup.
    pub(super) fn cups_section(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &mut Workspace,
        preparation: &PreparationUi,
        index: Option<usize>,
        icons: Option<&Icons>,
    ) {
        let advanced = self.holes_advanced;
        let vent = length_default(self.tool_defaults.min_vent_mm2, 2, Unit::SquareMillimetre);
        ui.add_enabled_ui(index.is_some(), |ui| {
            default_row(
                ui,
                tr!("cups-min-vent"),
                &mut self.cups.min_vent_mm2,
                Some(&vent),
                |ui, area| {
                    ui.add(
                        UnitField::new(area, Unit::SquareMillimetre)
                            .range(MIN_VENT_MM2)
                            .step(0.05),
                    )
                    .on_hover_text(tr!("cups-min-vent-hint"))
                },
            );
            if advanced {
                let from = length_default(DEFAULT_SHOW_FROM_MM2, 2, Unit::SquareMillimetre);
                default_row(
                    ui,
                    tr!("cups-show-from"),
                    &mut self.cups.show_from_mm2,
                    Some(&from),
                    |ui, area| {
                        ui.add(
                            UnitField::new(area, Unit::SquareMillimetre)
                                .range(SHOW_FROM_MM2)
                                .step(0.05),
                        )
                        .on_hover_text(tr!("cups-filter-hint"))
                    },
                );
                let taller = length_default(DEFAULT_SHOW_TALLER_MM, 2, Unit::Millimetre);
                default_row(
                    ui,
                    tr!("cups-show-taller"),
                    &mut self.cups.show_taller_mm,
                    Some(&taller),
                    |ui, height| {
                        ui.add(
                            UnitField::new(height, Unit::Millimetre)
                                .range(SHOW_TALLER_MM)
                                .step(0.05),
                        )
                        .on_hover_text(tr!("cups-filter-hint"))
                    },
                );
            }
        });
        let Some((cups, current)) = self
            .cups
            .found(workspace, index)
            .map(|(c, current)| (c.to_vec(), current))
        else {
            return;
        };
        let shown = self.cups.shown(&cups);
        let found = if current {
            cups.len().to_string()
        } else {
            tr!("cups-found-out-of-date", count = cups.len())
        };
        fact_row(ui, &tr!("cups-found-label"), &found);
        fact_row(
            ui,
            &tr!("cups-shown-label"),
            &tr!(
                "cups-shown-value",
                shown = shown.len(),
                hidden = cups.len() - shown.len()
            ),
        )
        .on_hover_text(tr!("cups-shown-hint"));
        let trapping = shown.iter().filter(|&&k| cups[k].closes).count();
        fact_row(
            ui,
            &tr!("cups-trapping-label"),
            &if trapping == 0 {
                tr!("cups-trapping-none")
            } else {
                tr!("cups-trapping-value", count = trapping)
            },
        );
        if let Some(i) = index {
            self.cups_navigator(ui, workspace, preparation, i, (&cups, current), icons);
        }
    }

    /// The chosen cup of model `index`: steps through the cups shown, its facts and Add a vent
    /// here.
    fn cups_navigator(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &mut Workspace,
        preparation: &PreparationUi,
        index: usize,
        (cups, current): (&[Cup], bool),
        icons: Option<&Icons>,
    ) {
        let shown = self.cups.shown(cups);
        self.cups.at = self.cups.at.min(cups.len().saturating_sub(1));
        let place = shown.iter().position(|&k| k == self.cups.at);
        let cup = cups.get(self.cups.at).cloned();
        ui.add_space(4.0);
        let mut go = None;
        ui.horizontal(|ui| {
            let count = shown.len();
            ui.add_enabled_ui(count > 1, |ui| {
                if icon_button(ui, icons, Icon::ChevronLeft, false, &tr!("cups-previous")).clicked()
                {
                    go = Some(match place {
                        Some(p) => shown[(p + count - 1) % count],
                        None => shown[count - 1],
                    });
                }
            });
            let title = match place {
                Some(p) => tr!("cups-of", number = p + 1, count = count),
                None => tr!("cups-cup"),
            };
            crate::widgets::group(ui, title, |ui| {
                ui.add_enabled_ui(count > 1, |ui| {
                    if icon_button(ui, icons, Icon::ChevronRight, false, &tr!("cups-next"))
                        .clicked()
                    {
                        go = Some(match place {
                            Some(p) => shown[(p + 1) % count],
                            None => shown[0],
                        });
                    }
                });
            });
        });
        if let Some(k) = go {
            self.cups.go_to(workspace, index, k);
        }
        // A cup under the filter has its facts while it is chosen from the list of all.
        let Some(cup) = cup.filter(|_| place.is_some() || self.cups.all) else {
            return;
        };
        let (open, colour) = match cup.opening {
            Opening::None => (tr!("cups-opening-none"), theme::ERROR.color()),
            Opening::TooSmall => (tr!("cups-opening-small"), theme::WARNING.color()),
        };
        fact_row(
            ui,
            &tr!("islands-layers"),
            &tr!(
                "islands-layer-span",
                first = cup.layers[0] + 1,
                last = cup.layers[1] + 1
            ),
        );
        fact_row(
            ui,
            &tr!("islands-height"),
            &tr!(
                "cups-height-span",
                bottom = decimal(cup.heights[0], 2),
                top = decimal(cup.heights[1], 2)
            ),
        );
        fact_row(
            ui,
            &tr!("cups-widest"),
            &tr!("islands-area-mm2", area = decimal(cup.area_mm2, 2)),
        )
        .on_hover_text(tr!("cups-widest-hint"));
        fact_row(
            ui,
            &tr!("cups-volume"),
            &tr!("cups-volume-mm3", volume = decimal(cup.volume_mm3, 2)),
        );
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(tr!("cups-opening")).color(ui.visuals().weak_text_color()),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                ui.label(egui::RichText::new(open).color(colour));
            });
        });
        fact_row(
            ui,
            &tr!("cups-then"),
            &if cup.closes {
                tr!("cups-then-closes")
            } else {
                tr!("cups-then-open")
            },
        );
        self.cups_parts(ui, workspace, preparation, index, (&cup, current), icons);
    }

    /// The chosen cup's separately capped parts (owner, 2026-10-31): how many vents it needs,
    /// a stepper through their caps when more than one, the chosen part's facts, and its vent
    /// or one under each.
    fn cups_parts(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &mut Workspace,
        preparation: &PreparationUi,
        index: usize,
        (cup, current): (&Cup, bool),
        icons: Option<&Icons>,
    ) {
        let parts = self.cups.shown_parts(cup);
        let count = parts.len();
        fact_row(ui, &tr!("cups-vents-needed"), &count.to_string())
            .on_hover_text(tr!("cups-vents-needed-hint"));
        let place = parts.iter().position(|&p| p == self.cups.part).unwrap_or(0);
        self.cups.part = parts.get(place).copied().unwrap_or(0);
        if count > 1 {
            let mut go = None;
            ui.horizontal(|ui| {
                if icon_button(
                    ui,
                    icons,
                    Icon::ChevronLeft,
                    false,
                    &tr!("cups-part-previous"),
                )
                .clicked()
                {
                    go = Some(parts[(place + count - 1) % count]);
                }
                ui.label(tr!("cups-part-of", number = place + 1, count = count));
                if icon_button(ui, icons, Icon::ChevronRight, false, &tr!("cups-part-next"))
                    .clicked()
                {
                    go = Some(parts[(place + 1) % count]);
                }
            });
            if let Some(p) = go {
                self.cups.go_to_part(workspace, index, self.cups.at, p);
            }
            if let Some(part) = cup.parts.get(self.cups.part) {
                fact_row(
                    ui,
                    &tr!("cups-part-cap"),
                    &tr!(
                        "cups-part-cap-value",
                        z = decimal(part.vent[2], 2),
                        height = decimal(part.height_mm(), 2),
                        area = decimal(part.area_mm2, 2)
                    ),
                );
            }
        }
        let size = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
        let can = current && self.cups.progress(Some(index)).is_none();
        let vent = ui
            .add_enabled(can, egui::Button::new(tr!("cups-vent")).min_size(size))
            .on_hover_text(tr!("cups-vent-hint"))
            .on_disabled_hover_text(tr!("cups-vent-out-of-date"))
            .clicked();
        let every = count > 1
            && ui
                .add_enabled(
                    can,
                    egui::Button::new(tr!("cups-vent-each", count = count)).min_size(size),
                )
                .on_hover_text(tr!("cups-vent-each-hint"))
                .on_disabled_hover_text(tr!("cups-vent-out-of-date"))
                .clicked();
        if vent || every {
            self.add_vents(workspace, preparation, index, every);
        }
    }

    /// The Slice panel's line on the suction cups: what the last search found, or that none
    /// ran. It never stops a slice.
    pub fn cups_slice_fact(&self, ui: &mut egui::Ui, workspace: &Workspace) {
        let (text, colour) = match self.cups.last(workspace) {
            None => (tr!("cups-slice-not-searched"), ui.visuals().text_color()),
            Some((0, current)) => (
                tr!("cups-slice-none", current = usize::from(current)),
                ui.visuals().text_color(),
            ),
            Some((count, current)) => (
                tr!(
                    "cups-slice-found",
                    count = count,
                    current = usize::from(current)
                ),
                theme::WARNING.color(),
            ),
        };
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(tr!("cups-section")).color(ui.visuals().weak_text_color()),
            )
            .on_hover_text(tr!("cups-slice-hint"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                ui.add(egui::Label::new(egui::RichText::new(text).color(colour)).wrap());
            });
        });
    }

    /// The footer's Find suction cups (or Stop), and the search's progress while it runs.
    pub(super) fn cups_footer(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &Workspace,
        index: Option<usize>,
    ) {
        let running = self.cups.searching();
        let (label, hint) = if running {
            (tr!("islands-stop"), tr!("cups-stop-hint"))
        } else if self.cups.found(workspace, index).is_some() {
            (tr!("cups-find-again"), tr!("cups-find-hint"))
        } else {
            (tr!("cups-find"), tr!("cups-find-hint"))
        };
        let size = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
        let clicked = ui
            .add_enabled(
                index.is_some() && workspace.plate().setting().is_some(),
                egui::Button::new(label).min_size(size),
            )
            .on_hover_text(hint)
            .clicked();
        if clicked {
            match (running, index) {
                (true, _) => self.cups.stop(),
                (false, Some(i)) => self.cups.find(workspace, i),
                (false, None) => {}
            }
        }
        if let Some(share) = self.cups.progress(index) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // A percentage.
            let percent = (share * 100.0).round().clamp(0.0, 100.0) as u32;
            ui.add(egui::ProgressBar::new(share).desired_height(6.0))
                .on_hover_text(tr!("cups-finding", percent = percent));
        }
    }

    /// The list window's second tab: model `index`'s cups, the shown ones or all; a double
    /// click on a row frames its cup.
    pub(in crate::hollowing_ui) fn cups_window(
        &mut self,
        ctx: &egui::Context,
        workspace: &Workspace,
        index: Option<usize>,
        appearance: &Appearance,
    ) {
        let (cups, current) = self
            .cups
            .found(workspace, index)
            .map_or((Vec::new(), true), |(c, current)| (c.to_vec(), current));
        let shown = self.cups.shown(&cups);
        let items: Vec<usize> = if self.cups.all {
            (0..cups.len()).collect()
        } else {
            shown.clone()
        };
        let reveal = std::mem::take(&mut self.cups.reveal)
            .then_some(self.cups.at)
            .filter(|&k| k < cups.len());
        let searched = index.is_some() && self.cups.found(workspace, index).is_some();
        let spec = Spec {
            id: "holes-list",
            title: tr!("cups-list-title", count = cups.len()),
            fact: tr!("cups-list-fact"),
            state: tr!("list-state"),
            tabs: vec![tr!("cups-tab-holes"), tr!("cups-tab-cups")],
            filters: vec![
                tr!("cups-list-shown", count = shown.len()),
                tr!("cups-list-all", count = cups.len()),
            ],
            places: Vec::new(),
            summary: if index.is_none() {
                tr!("panel-no-model")
            } else if !searched {
                tr!("cups-list-not-searched")
            } else if current {
                tr!("cups-list-current")
            } else {
                tr!("cups-out-of-date")
            },
            remove: None,
        };
        let at = self.cups.at;
        let row = |k: usize| {
            let cup = &cups[k];
            let colour = marker_colour(cup, appearance);
            Row {
                dot: if k == at {
                    appearance.support_selected.color()
                } else {
                    colour
                },
                name: tr!("list-number", number = k + 1),
                fact: tr!(
                    "cups-list-row",
                    first = cup.layers[0] + 1,
                    last = cup.layers[1] + 1,
                    area = decimal(cup.area_mm2, 2)
                ),
                state: state(cup),
                state_colour: if cup.closes {
                    theme::ERROR.color()
                } else {
                    theme::WARNING.color()
                },
                hint: tr!("list-row-hint"),
                selected: k == at,
            }
        };
        let mut tab = 1;
        let mut filter = usize::from(self.cups.all);
        let event = list_window::show(
            ctx,
            &spec,
            &mut self.holes_list_open,
            &mut tab,
            &mut filter,
            &mut 0,
            &items,
            reveal,
            row,
        );
        self.cups.all = filter == 1;
        self.cups_listed = tab == 1;
        let Some(object) = index else {
            return;
        };
        match event {
            Some(Event::Picked(k)) => self.cups.at = k,
            Some(Event::Focused(k)) => self.cups.go_to(workspace, object, k),
            Some(Event::Removed(_)) | None => {}
        }
    }
}
