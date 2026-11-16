//! The sidebar's profile menus: printer, resin and print setting (owner, 2026-11-11).
//!
//! A library can hold many profiles, so each menu is a combo box in three parts:
//! - a search field, shown only once the list is longer than [`SEARCH_FROM`], which takes the
//!   keyboard focus as the menu opens;
//! - the list, which scrolls once it is taller than [`LIST_HEIGHT`];
//! - the menu's actions (add, manage), below the list and always in view.
//!
//! The menu stays open while the search field is clicked; choosing an entry or an action closes
//! it (the caller calls `ui.close()`). The query lives in egui's temporary memory while the menu
//! is open and is cleared when it closes.
//!
//! **Long names** (issue #18, design rule "Fixed size"): the button keeps the width it is given
//! and the menu that width, so a long name never widens the sidebar. A name that does not fit is
//! cut with an ellipsis, shows in full in its tooltip, and is given in full to screen readers
//! ([`crate::widgets::menu_entry`] for the menu's entries).
use crate::{
    i18n::tr,
    widgets::{text_field, wider},
};

pub(super) use crate::widgets::menu_entry as entry;
use eframe::egui;

/// A list longer than this gets a search field.
pub(super) const SEARCH_FROM: usize = 10;

/// The tallest the list grows before it scrolls, in points.
pub(super) const LIST_HEIGHT: f32 = 320.0;

/// What the search field holds: entries whose text contains it, ignoring case, are listed.
pub(super) struct Query(String);

impl Query {
    fn new(text: &str) -> Self {
        Self(text.trim().to_lowercase())
    }

    /// Whether an entry whose text is `text` is listed.
    pub(super) fn matches(&self, text: &str) -> bool {
        self.0.is_empty() || text.to_lowercase().contains(&self.0)
    }

    /// Whether the search field is empty.
    pub(super) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// A profile menu named `id_salt`, showing `selected` on its button, `full` being its whole text,
/// with `count` entries. `list` adds the entries the query matches and returns how many it added;
/// `actions` adds the menu's actions. The button and the menu keep the width available.
pub(super) fn show(
    ui: &mut egui::Ui,
    id_salt: &str,
    selected: impl Into<egui::WidgetText>,
    full: &str,
    count: usize,
    list: impl FnOnce(&mut egui::Ui, &Query) -> usize,
    actions: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    let query_id = egui::Id::new(("profile-menu-query", id_salt));
    let width = ui.available_width();
    let selected = selected.into();
    // The button's text has its width less the padding and the arrow.
    let spacing = ui.spacing();
    let room = width - 2.0 * spacing.button_padding.x - spacing.icon_width - spacing.icon_spacing;
    let cut = wider(ui, &selected, room);
    let combo = egui::ComboBox::from_id_salt(id_salt)
        .selected_text(selected)
        .width(width)
        .wrap_mode(egui::TextWrapMode::Truncate)
        // The list scrolls on its own; the whole menu scrolls only when the window is too short
        // to hold it.
        .height(ui.ctx().content_rect().height())
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show_ui(ui, |ui| {
            // Entries are cut to the button's width rather than widening the menu.
            ui.set_max_width(width);
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
            let opened = ui.data(|d| d.get_temp::<String>(query_id)).is_none();
            let mut text = ui
                .data(|d| d.get_temp::<String>(query_id))
                .unwrap_or_default();
            if count > SEARCH_FROM {
                let search = ui
                    .add(text_field(&mut text, ui.available_width()).hint_text(tr!("menu-search")));
                crate::widgets::name(&search, &tr!("menu-search"));
                if opened {
                    search.request_focus();
                }
                ui.add_space(4.0);
            }
            ui.data_mut(|d| d.insert_temp(query_id, text.clone()));
            let query = Query::new(&text);
            egui::ScrollArea::vertical()
                .id_salt((id_salt, "list"))
                .max_height(LIST_HEIGHT)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    if list(ui, &query) == 0 && !query.is_empty() {
                        ui.label(
                            egui::RichText::new(tr!("menu-no-match"))
                                .color(ui.visuals().weak_text_color()),
                        );
                    }
                });
            ui.separator();
            actions(ui);
        });
    if combo.inner.is_none() {
        ui.data_mut(|d| d.remove::<String>(query_id));
    }
    // Screen readers get the whole name; the widget's own name (Printer, Resin) is the caller's.
    let full_value = full.to_owned();
    ui.ctx()
        .accesskit_node_builder(combo.response.id, |node| node.set_value(full_value));
    if cut {
        combo.response.on_hover_text(full)
    } else {
        combo.response
    }
}

#[cfg(test)]
mod tests {
    use super::{entry, show, Query, SEARCH_FROM};
    use eframe::egui;

    #[test]
    fn the_query_matches_part_of_a_name_ignoring_case_and_spaces() {
        let query = Query::new("  stand ");
        assert!(query.matches("Anycubic Standard Resin"));
        assert!(!query.matches("ELEGOO ABS-like Resin"));
        assert!(Query::new("").matches("anything"));
        assert!(Query::new("   ").is_empty());
    }

    /// A menu of `count` entries and two actions, open, its search field holding `query`;
    /// returns the texts painted.
    fn painted(query: Option<&str>, count: usize) -> String {
        let ctx = egui::Context::default();
        let names: Vec<String> = (0..count).map(|i| format!("Resin {i:02}")).collect();
        let mut texts = String::new();
        let mut button: Option<egui::Id> = None;
        for _ in 0..4 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                // The first frame draws the button; the next ones open its popup, which egui's
                // combo box keeps under the button's id with "popup".
                if let Some(id) = button {
                    egui::Popup::open_id(ui.ctx(), id.with("popup"));
                }
                if let Some(query) = query {
                    ui.data_mut(|d| {
                        d.insert_temp(
                            egui::Id::new(("profile-menu-query", "resins")),
                            query.to_owned(),
                        );
                    });
                }
                let response = show(
                    ui,
                    "resins",
                    "Resin 00",
                    "Resin 00",
                    names.len(),
                    |ui, q| {
                        let mut shown = 0;
                        for name in names.iter().filter(|n| q.matches(n)) {
                            ui.label(name);
                            shown += 1;
                        }
                        shown
                    },
                    |ui| {
                        ui.label("Add resin");
                        ui.label("Manage the library");
                    },
                );
                button = Some(response.id);
            });
            output.textures_delta.clear();
            texts = output
                .shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::epaint::Shape::Text(t) => Some(t.galley.text().to_owned()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
        }
        texts
    }

    #[test]
    fn a_long_list_scrolls_and_keeps_its_actions_in_view() {
        let texts = painted(None, 30);
        assert!(texts.contains("Resin 01"), "{texts}");
        assert!(!texts.contains("Resin 29"), "the list scrolls: {texts}");
        assert!(texts.contains(&crate::i18n::tr!("menu-search")), "{texts}");
        assert!(texts.contains("Add resin"), "{texts}");
        assert!(texts.contains("Manage the library"), "{texts}");
    }

    #[test]
    fn the_search_field_narrows_a_long_list_and_says_when_nothing_matches() {
        const { assert!(30 > SEARCH_FROM) };
        let texts = painted(Some("29"), 30);
        assert!(texts.contains("Resin 29"), "{texts}");
        assert!(!texts.contains("Resin 01"), "{texts}");
        assert!(texts.contains("Add resin"), "{texts}");
        let none = painted(Some("zzz"), 30);
        assert!(none.contains(&crate::i18n::tr!("menu-no-match")), "{none}");
    }

    #[test]
    fn a_long_entry_is_cut_to_the_menu_and_named_in_full() {
        // Issue #18: the open menu keeps the button's width; a long entry is cut, with its whole
        // name for screen readers.
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let long = format!("Resin {}", "with a very long name ".repeat(12).trim_end());
        let (mut button, mut entry_width, mut nodes) = (None::<egui::Id>, 0.0f32, Vec::new());
        for _ in 0..4 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                if let Some(id) = button {
                    egui::Popup::open_id(ui.ctx(), id.with("popup"));
                }
                ui.allocate_ui_with_layout(
                    egui::vec2(200.0, 400.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(200.0);
                        let response = show(
                            ui,
                            "long",
                            long.clone(),
                            &long,
                            1,
                            |ui, _| {
                                entry_width = entry(ui, true, long.clone(), &long).rect.width();
                                1
                            },
                            |_| {},
                        );
                        button = Some(response.id);
                    },
                );
            });
            output.textures_delta.clear();
            nodes = output
                .platform_output
                .accesskit_update
                .map_or_default(|update| update.nodes);
        }
        assert!(entry_width > 0.0 && entry_width <= 200.0, "{entry_width}");
        let named = |name: &str| {
            nodes
                .iter()
                .filter(|(_, n)| n.label() == Some(name) || n.value() == Some(name))
                .count()
        };
        // The button and the entry.
        assert!(named(&long) >= 2, "{}", named(&long));
    }

    #[test]
    fn a_short_list_has_no_search_field() {
        let texts = painted(None, SEARCH_FROM);
        assert!(!texts.contains(&crate::i18n::tr!("menu-search")), "{texts}");
        assert!(
            texts.contains(&format!("Resin {:02}", SEARCH_FROM - 1)),
            "{texts}"
        );
    }
}
