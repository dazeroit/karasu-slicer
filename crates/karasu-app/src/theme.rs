//! The approved colour palette (`docs/UI-DESIGN.md`) as semantic tokens, and the egui visuals
//! built from them. Widgets use the tokens, never literal colours.
//!
//! **Status colours are the user's** (TD-142): the warning, error and success colours and the
//! notifications' fills are set in Settings › Appearance. The window hands them to
//! [`set_status`] every frame, and their tokens answer with them on the interface's thread;
//! elsewhere, and until set, with the defaults here.
use eframe::egui::{self, Color32, Stroke, Visuals};
use std::cell::Cell;

/// The status colours a user can set, in order: [`WARNING`], [`ERROR`], [`SUCCESS`],
/// [`INFO_FILL`], [`SUCCESS_FILL`], [`WARNING_FILL`], [`ERROR_FILL`].
pub const STATUS_TOKENS: [Token; STATUS_COUNT] = [
    WARNING,
    ERROR,
    SUCCESS,
    INFO_FILL,
    SUCCESS_FILL,
    WARNING_FILL,
    ERROR_FILL,
];
/// How many status colours there are.
pub const STATUS_COUNT: usize = 7;

thread_local! {
    /// The status colours set on this thread, the interface's; none until set.
    static STATUS: Cell<Option<[[u8; 4]; STATUS_COUNT]>> = const { Cell::new(None) };
}

/// Use `colours` (sRGB with alpha, in the order of [`STATUS_TOKENS`]) for the status tokens on
/// this thread from now on.
pub fn set_status(colours: [[u8; 4]; STATUS_COUNT]) {
    STATUS.with(|status| status.set(Some(colours)));
}

/// A colour of the approved palette: sRGB red, green and blue, and alpha (TD-076). The palette
/// is opaque; alpha lets a configured interface colour be translucent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    /// Palette name.
    pub name: &'static str,
    /// sRGB red, green and blue, then alpha (255 is opaque), not premultiplied: the default.
    pub rgba: [u8; 4],
    /// For a status colour, its place in [`STATUS_TOKENS`].
    status: Option<usize>,
}

impl Token {
    /// The colour in use: a status colour as the user set it, otherwise the token's own; sRGB
    /// with alpha, not premultiplied.
    #[must_use]
    pub fn current(self) -> [u8; 4] {
        self.status
            .and_then(|k| STATUS.with(Cell::get).map(|colours| colours[k]))
            .unwrap_or(self.rgba)
    }

    /// The colour in use, for egui.
    #[must_use]
    pub fn color(self) -> Color32 {
        let [r, g, b, a] = self.current();
        Color32::from_rgba_unmultiplied(r, g, b, a)
    }
}

/// General application background (Raven Black).
pub const BACKGROUND: Token = Token {
    name: "Raven Black",
    rgba: [0x0B, 0x0B, 0x12, 0xFF],
    status: None,
};
/// Panels and surfaces (Charcoal Ink).
pub const SURFACE: Token = Token {
    name: "Charcoal Ink",
    rgba: [0x1A, 0x1C, 0x24, 0xFF],
    status: None,
};
/// Borders and separators (Beak Slate).
pub const BORDER: Token = Token {
    name: "Beak Slate",
    rgba: [0x4A, 0x46, 0x5C, 0xFF],
    status: None,
};
/// Primary actions (Resin Purple).
pub const PRIMARY: Token = Token {
    name: "Resin Purple",
    rgba: [0x6F, 0x35, 0xF5, 0xFF],
    status: None,
};
/// Hover and selection states (Electric Violet).
pub const INTERACTION: Token = Token {
    name: "Electric Violet",
    rgba: [0x8E, 0x52, 0xFF, 0xFF],
    status: None,
};
/// Accents and indicators (Soft Lilac).
pub const ACCENT: Token = Token {
    name: "Soft Lilac",
    rgba: [0xC9, 0xB2, 0xFF, 0xFF],
    status: None,
};
/// Primary text (Mist White).
pub const TEXT_PRIMARY: Token = Token {
    name: "Mist White",
    rgba: [0xF5, 0xF4, 0xF8, 0xFF],
    status: None,
};

/// Warnings (amber). Status colours are not brand colours: the owner chose to use other colours
/// in the interface too, every one configurable (M6.5); the status colours are the user's
/// (TD-142, see the module).
pub const WARNING: Token = Token {
    name: "Status Amber",
    rgba: [0xF2, 0xB3, 0x3D, 0xFF],
    status: Some(0),
};
/// Errors (red); see [`WARNING`].
pub const ERROR: Token = Token {
    name: "Status Red",
    rgba: [0xFF, 0x6B, 0x6B, 0xFF],
    status: Some(1),
};
/// Success (green); see [`WARNING`].
pub const SUCCESS: Token = Token {
    name: "Status Green",
    rgba: [0x5B, 0xD1, 0x7A, 0xFF],
    status: Some(2),
};
/// The fills of notifications (M6.5), one per severity: dark enough for primary text on them;
/// see [`WARNING`].
pub const INFO_FILL: Token = Token {
    name: "Notice Azure",
    rgba: [0x15, 0x5E, 0x8A, 0xFF],
    status: Some(3),
};
pub const SUCCESS_FILL: Token = Token {
    name: "Notice Green",
    rgba: [0x17, 0x66, 0x3A, 0xFF],
    status: Some(4),
};
pub const WARNING_FILL: Token = Token {
    name: "Notice Orange",
    rgba: [0x9A, 0x4A, 0x06, 0xFF],
    status: Some(5),
};
pub const ERROR_FILL: Token = Token {
    name: "Notice Red",
    rgba: [0x9B, 0x1C, 0x1C, 0xFF],
    status: Some(6),
};
/// A finding chosen on a map (cyan), unlike material, the brand's violets and the other status
/// colours; see [`WARNING`].
pub const SELECTION: Token = Token {
    name: "Status Cyan",
    rgba: [0x3D, 0xD6, 0xF2, 0xFF],
    status: None,
};

/// Adaptive layer heights on the section bar (TD-102): a blue violet apart from the brand's.
pub const LAYERS_ADAPTIVE: Token = Token {
    name: "Layers Violet",
    rgba: [0x5B, 0x4B, 0xD6, 0xFF],
    status: None,
};
/// Fixed layer heights of their own on the section bar (TD-102).
pub const LAYERS_FIXED: Token = Token {
    name: "Layers Teal",
    rgba: [0x2E, 0x9C, 0x8F, 0xFF],
    status: None,
};

/// Dark visuals built only from the palette tokens.
#[must_use]
pub fn visuals() -> Visuals {
    let mut visuals = Visuals::dark();
    visuals.override_text_color = Some(TEXT_PRIMARY.color());
    visuals.panel_fill = SURFACE.color();
    visuals.window_fill = SURFACE.color();
    visuals.extreme_bg_color = BACKGROUND.color();
    visuals.faint_bg_color = SURFACE.color();
    visuals.window_stroke = Stroke::new(1.0, BORDER.color());
    visuals.hyperlink_color = ACCENT.color();
    visuals.selection.bg_fill = PRIMARY.color();
    visuals.selection.stroke = Stroke::new(1.0, TEXT_PRIMARY.color());
    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_fill = SURFACE.color();
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER.color());
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY.color());
    widgets.inactive.bg_fill = SURFACE.color();
    widgets.inactive.weak_bg_fill = SURFACE.color();
    widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER.color());
    widgets.hovered.bg_fill = INTERACTION.color();
    widgets.hovered.weak_bg_fill = INTERACTION.color();
    widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT.color());
    widgets.active.bg_fill = PRIMARY.color();
    widgets.active.weak_bg_fill = PRIMARY.color();
    widgets.active.bg_stroke = Stroke::new(1.0, ACCENT.color());
    visuals
}

/// Apply the palette visuals to every theme of `ctx` (the application is dark only).
pub fn apply(ctx: &egui::Context) {
    ctx.set_visuals_of(egui::Theme::Dark, visuals());
    ctx.set_visuals_of(egui::Theme::Light, visuals());
    ctx.all_styles_mut(|style| style.spacing = spacing(&style.spacing));
}

/// Roomier controls than egui's defaults (owner, M6.5): buttons padded 10 by 5 points (egui: 4
/// by 1), controls at least 26 points tall (egui: 18), so buttons, combo boxes and fields line
/// up, and 6 points between rows (egui: 3). The workflow bar sizes its own buttons.
#[must_use]
pub fn spacing(base: &egui::style::Spacing) -> egui::style::Spacing {
    egui::style::Spacing {
        button_padding: egui::vec2(10.0, 5.0),
        interact_size: egui::vec2(base.interact_size.x, 26.0),
        item_spacing: egui::vec2(base.item_spacing.x, 6.0),
        ..base.clone()
    }
}

/// WCAG 2.x contrast ratio of two tokens' default colours, from 1 to 21.
#[cfg(test)]
fn contrast_ratio(a: Token, b: Token) -> f64 {
    contrast(a.rgba, b.rgba)
}

/// WCAG 2.x contrast ratio of colour `a` seen over the opaque colour `b` (sRGB with alpha), from 1
/// to 21: 4.5 is the least for text, 3 for large text and the parts of controls.
#[must_use]
pub fn contrast(a: [u8; 4], b: [u8; 4]) -> f64 {
    let channel = |c: f64| {
        if c <= 0.040_45 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let luminance =
        |[r, g, b]: [f64; 3]| 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
    let unit = |rgba: [u8; 4]| rgba.map(|c| f64::from(c) / 255.0);
    let (fg, bg) = (unit(a), unit(b));
    // The foreground is seen over the (opaque) background.
    let seen: [f64; 3] = std::array::from_fn(|k| fg[k] * fg[3] + bg[k] * (1.0 - fg[3]));
    let (la, lb) = (luminance(seen), luminance([bg[0], bg[1], bg[2]]));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_keep_the_approved_values() {
        let hex = |t: Token| format!("#{:02X}{:02X}{:02X}", t.rgba[0], t.rgba[1], t.rgba[2]);
        let palette = [
            (BACKGROUND, "#0B0B12"),
            (SURFACE, "#1A1C24"),
            (BORDER, "#4A465C"),
            (PRIMARY, "#6F35F5"),
            (INTERACTION, "#8E52FF"),
            (ACCENT, "#C9B2FF"),
            (TEXT_PRIMARY, "#F5F4F8"),
        ];
        for (token, value) in palette {
            assert_eq!(hex(token), value, "{}", token.name);
            assert_eq!(token.rgba[3], 0xFF, "{} is opaque", token.name);
        }
    }

    #[test]
    fn text_combinations_meet_wcag_aa_contrast() {
        // Body text (4.5:1) on the background, the panels and the primary action fill.
        for fill in [BACKGROUND, SURFACE, PRIMARY] {
            let ratio = contrast_ratio(TEXT_PRIMARY, fill);
            assert!(
                ratio >= 4.5,
                "{} on {}: {ratio:.2}",
                TEXT_PRIMARY.name,
                fill.name
            );
        }
        // Text on the notifications' fills.
        for fill in [INFO_FILL, SUCCESS_FILL, WARNING_FILL, ERROR_FILL] {
            let ratio = contrast_ratio(TEXT_PRIMARY, fill);
            assert!(
                ratio >= 4.5,
                "{} on {}: {ratio:.2}",
                TEXT_PRIMARY.name,
                fill.name
            );
        }
        // Accent indicators and status text on panels.
        for token in [ACCENT, WARNING, ERROR, SUCCESS] {
            let ratio = contrast_ratio(token, SURFACE);
            assert!(
                ratio >= 4.5,
                "{} on {}: {ratio:.2}",
                token.name,
                SURFACE.name
            );
        }
        // Borders are non-text separators: 3:1 is not required of them, only recorded.
        assert!(contrast_ratio(BORDER, SURFACE) > 1.5);
        assert!((contrast_ratio(TEXT_PRIMARY, TEXT_PRIMARY) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn status_colours_are_the_users_on_the_interface_thread_only() {
        let red = [0xD0, 0x10, 0x10, 0xFF];
        let mut colours = STATUS_TOKENS.map(|token| token.rgba);
        colours[1] = red;
        set_status(colours);
        assert_eq!(ERROR.current(), red);
        assert_eq!(WARNING.current(), WARNING.rgba);
        // Palette tokens are never the user's.
        assert_eq!(ACCENT.current(), ACCENT.rgba);
        // Another thread draws nothing and keeps the defaults.
        assert_eq!(
            std::thread::spawn(|| ERROR.current()).join().unwrap(),
            ERROR.rgba
        );
        assert!(contrast(red, SURFACE.rgba) < 4.5);
        assert!(
            (contrast(ERROR.rgba, SURFACE.rgba) - contrast_ratio(ERROR, SURFACE)).abs() < 1e-12
        );
    }

    #[test]
    fn controls_are_roomier_than_egui_defaults() {
        let base = egui::style::Spacing::default();
        let ours = spacing(&base);
        assert_eq!(ours.button_padding, egui::vec2(10.0, 5.0));
        assert!((ours.interact_size.y - 26.0).abs() < f32::EPSILON);
        assert!((ours.item_spacing.y - 6.0).abs() < f32::EPSILON);
        // Everything else is egui's.
        assert_eq!(ours.indent, base.indent);
    }

    #[test]
    fn visuals_use_only_palette_colours() {
        let visuals = visuals();
        assert_eq!(visuals.panel_fill, SURFACE.color());
        assert_eq!(visuals.extreme_bg_color, BACKGROUND.color());
        assert_eq!(visuals.override_text_color, Some(TEXT_PRIMARY.color()));
        assert_eq!(visuals.selection.bg_fill, PRIMARY.color());
        assert_eq!(visuals.widgets.hovered.bg_fill, INTERACTION.color());
    }
}
