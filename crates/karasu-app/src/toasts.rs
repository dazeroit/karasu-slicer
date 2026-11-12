//! Toast notifications (M6.5, owner's design): every message also shows briefly at the bottom of
//! the viewport, centred, and stays in the Messages window.
//!
//! **A toast** is a small fixed-width card filled in its severity's colour: a title bar with the
//! severity's icon, its name and a close button, then the message. A toast may be timed: a thin
//! bar at its bottom runs from full to empty, right to left, and the toast closes when it ends.
//! The pointer resting on a toast pauses its time. Untimed toasts stay until closed.
//!
//! **Stacking:** the newest toast is lowest; at most [`MAX_SHOWN`] show at once, the older ones
//! keeping their time behind them. Toasts never take the keyboard and are drawn over the scene.
//!
//! **Cost:** nothing is drawn or repainted without toasts; while a timed one shows, the frame is
//! repainted about 30 times a second for its bar.
use crate::{
    i18n::tr,
    icons::{Icon, Icons},
    theme,
};
use eframe::egui;
use std::time::{Duration, Instant};

/// How serious a notification is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Success,
    Warning,
    Error,
}

impl Severity {
    /// The severity's name, the toast's title.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::Info => tr!("toast-info"),
            Self::Success => tr!("toast-success"),
            Self::Warning => tr!("toast-warning"),
            Self::Error => tr!("toast-error"),
        }
    }

    /// How long a toast of this severity shows unless its message says otherwise: errors stay
    /// until closed.
    #[must_use]
    pub fn default_duration(self) -> Option<Duration> {
        match self {
            Self::Info => Some(Duration::from_secs(5)),
            Self::Success => Some(Duration::from_secs(4)),
            Self::Warning => Some(Duration::from_secs(8)),
            Self::Error => None,
        }
    }

    fn fill(self) -> egui::Color32 {
        match self {
            Self::Info => theme::INFO_FILL.color(),
            Self::Success => theme::SUCCESS_FILL.color(),
            Self::Warning => theme::WARNING_FILL.color(),
            Self::Error => theme::ERROR_FILL.color(),
        }
    }

    /// The severity's icon, beside its colour wherever it shows (TD-142).
    pub(crate) fn icon(self) -> Icon {
        match self {
            Self::Info => Icon::Info,
            Self::Success => Icon::Success,
            Self::Warning => Icon::Warning,
            Self::Error => Icon::Error,
        }
    }
}

/// One toast.
#[derive(Clone, Debug, PartialEq)]
struct Toast {
    id: u64,
    severity: Severity,
    text: String,
    /// The message has more in the Messages window (a detail or fixes).
    more: bool,
    /// How long it shows, or none until closed.
    duration: Option<Duration>,
    /// When its time started, shifted by the pauses so far.
    started: Instant,
    /// When the pointer came to rest on it, while it does.
    paused: Option<Instant>,
}

impl Toast {
    /// The fraction of its time left, from 1 to 0; 1 for an untimed toast.
    fn left(&self, now: Instant) -> f32 {
        let Some(duration) = self.duration else {
            return 1.0;
        };
        let elapsed = self
            .paused
            .unwrap_or(now)
            .saturating_duration_since(self.started);
        (1.0 - elapsed.as_secs_f32() / duration.as_secs_f32().max(1e-3)).clamp(0.0, 1.0)
    }

    fn pause(&mut self, now: Instant) {
        self.paused.get_or_insert(now);
    }

    fn resume(&mut self, now: Instant) {
        if let Some(paused) = self.paused.take() {
            self.started += now.saturating_duration_since(paused);
        }
    }
}

/// At most this many toasts show at once.
pub const MAX_SHOWN: usize = 3;
/// A toast's width, its gap above the viewport's bottom and between toasts, in points.
const WIDTH: f32 = 440.0;
const BOTTOM_MARGIN: f32 = 20.0;
const GAP: f32 = 10.0;

/// The toasts showing.
#[derive(Debug, Default)]
pub struct Toasts {
    list: Vec<Toast>,
    next: u64,
}

impl Toasts {
    /// Show `text` as a toast of `severity` for `duration` (none: until closed); `more` says the
    /// Messages window has more about it.
    pub fn push(
        &mut self,
        severity: Severity,
        text: String,
        more: bool,
        duration: Option<Duration>,
    ) {
        self.next += 1;
        self.list.push(Toast {
            id: self.next,
            severity,
            text,
            more,
            duration,
            started: Instant::now(),
            paused: None,
        });
    }

    /// Drop the toasts whose time ran out by `now`.
    fn expire(&mut self, now: Instant) {
        self.list
            .retain(|t| t.duration.is_none() || t.left(now) > 0.0);
    }

    /// Draw the toasts at the bottom centre of `viewport`; returns whether a toast's link to the
    /// Messages window was clicked.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        viewport: egui::Rect,
        icons: Option<&Icons>,
    ) -> bool {
        let now = Instant::now();
        self.expire(now);
        if self.list.is_empty() {
            return false;
        }
        let pointer = ctx.pointer_hover_pos();
        let mut closed = Vec::new();
        let mut open_messages = false;
        let first = self.list.len().saturating_sub(MAX_SHOWN);
        // Under a modal dialog the notifications go under its backdrop, dimmed with the rest of
        // the window, and take no click and no focus: the dialog keeps the keyboard (TD-142).
        let modal = ctx.memory(|m| m.top_modal_layer().is_some());
        egui::Area::new(egui::Id::new("toasts"))
            .order(if modal {
                egui::Order::Middle
            } else {
                egui::Order::Foreground
            })
            .pivot(egui::Align2::CENTER_BOTTOM)
            .fixed_pos(viewport.center_bottom() - egui::vec2(0.0, BOTTOM_MARGIN))
            .show(ctx, |ui| {
                ui.set_width(WIDTH);
                ui.spacing_mut().item_spacing.y = GAP;
                for toast in &mut self.list[first..] {
                    let (rect, close, link) = card(ui, toast, now, icons);
                    if close {
                        closed.push(toast.id);
                    }
                    open_messages |= link;
                    if pointer.is_some_and(|p| rect.contains(p)) {
                        toast.pause(now);
                    } else {
                        toast.resume(now);
                    }
                }
            });
        self.list.retain(|t| !closed.contains(&t.id));
        if self.list.iter().any(|t| t.duration.is_some()) {
            ctx.request_repaint_after(Duration::from_millis(33));
        }
        open_messages
    }
}

/// A panel's notice (TD-098): what the tool needs and lacks, at the top of its panel, as long as
/// it lacks it ("No model selected"). It looks like a toast of `severity`, without a close
/// button or a time: it is the panel's state, not an event, and goes when the need is met.
pub fn notice(ui: &mut egui::Ui, severity: Severity, text: &str, icons: Option<&Icons>) {
    let colour = theme::TEXT_PRIMARY.color();
    egui::Frame::new()
        .fill(severity.fill())
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (icon, _) =
                    ui.allocate_exact_size(egui::Vec2::splat(18.0), egui::Sense::hover());
                if let Some(icons) = icons {
                    icons.paint(ui.painter(), severity.icon(), icon, colour);
                }
                ui.add(egui::Label::new(egui::RichText::new(text).color(colour)).wrap());
            });
        });
}

/// One toast's card; returns its rectangle, whether it was closed, and whether its link to the
/// Messages window was clicked.
fn card(
    ui: &mut egui::Ui,
    toast: &Toast,
    now: Instant,
    icons: Option<&Icons>,
) -> (egui::Rect, bool, bool) {
    let text = theme::TEXT_PRIMARY.color();
    let mut close = false;
    let mut link = false;
    let radius = 8u8;
    let shown = egui::Frame::new()
        .fill(toast.severity.fill())
        .corner_radius(radius)
        .stroke(egui::Stroke::new(1.0, text.gamma_multiply(0.18)))
        .show(ui, |ui| {
            ui.set_width(WIDTH);
            ui.spacing_mut().item_spacing.y = 0.0;
            egui::Frame::new()
                .inner_margin(egui::Margin {
                    left: 12,
                    right: 8,
                    top: 8,
                    bottom: 12,
                })
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    ui.horizontal(|ui| {
                        let (icon, _) =
                            ui.allocate_exact_size(egui::Vec2::splat(20.0), egui::Sense::hover());
                        if let Some(icons) = icons {
                            icons.paint(ui.painter(), toast.severity.icon(), icon, text);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            close = crate::widgets::icon_button_on(
                                ui,
                                icons,
                                Icon::Close,
                                &tr!("toast-close"),
                                toast.severity.fill(),
                            )
                            .clicked();
                            ui.with_layout(
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    let title = egui::RichText::new(toast.severity.label())
                                        .strong()
                                        .color(text);
                                    ui.add(egui::Label::new(title).truncate());
                                },
                            );
                        });
                    });
                    ui.add(egui::Label::new(egui::RichText::new(&toast.text).color(text)).wrap());
                    if toast.more {
                        link = ui
                            .link(
                                egui::RichText::new(tr!("toast-details"))
                                    .color(text)
                                    .underline(),
                            )
                            .clicked();
                    }
                });
            if toast.duration.is_some() {
                let (bar, _) = ui.allocate_exact_size(egui::vec2(WIDTH, 3.0), egui::Sense::hover());
                let bottom = egui::CornerRadius {
                    nw: 0,
                    ne: 0,
                    sw: radius,
                    se: radius,
                };
                ui.painter()
                    .rect_filled(bar, bottom, text.gamma_multiply(0.18));
                let left = egui::Rect::from_min_size(
                    bar.min,
                    egui::vec2(bar.width() * toast.left(now), bar.height()),
                );
                ui.painter()
                    .rect_filled(left, bottom, text.gamma_multiply(0.7));
            }
        });
    (shown.response.rect, close, link)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    impl Toasts {
        /// The severities and durations of the toasts, oldest first.
        pub(crate) fn shown(&self) -> Vec<(Severity, Option<Duration>)> {
            self.list.iter().map(|t| (t.severity, t.duration)).collect()
        }
    }

    #[test]
    fn timed_toasts_run_out_and_pause_under_the_pointer() {
        let mut toasts = Toasts::default();
        toasts.push(
            Severity::Info,
            "Opened".into(),
            false,
            Some(Duration::from_secs(5)),
        );
        toasts.push(Severity::Error, "Failed".into(), true, None);
        let start = toasts.list[0].started;
        assert!((toasts.list[0].left(start) - 1.0).abs() < 1e-6);
        let half = start + Duration::from_millis(2500);
        assert!((toasts.list[0].left(half) - 0.5).abs() < 1e-3);
        // Paused at half time for 10 s, it still has half its time when the pointer leaves.
        toasts.list[0].pause(half);
        let later = half + Duration::from_secs(10);
        assert!((toasts.list[0].left(later) - 0.5).abs() < 1e-3);
        toasts.list[0].resume(later);
        assert!((toasts.list[0].left(later) - 0.5).abs() < 1e-3);
        // Its time over, it goes; the error stays until closed.
        toasts.expire(later + Duration::from_secs(3));
        assert_eq!(toasts.list.len(), 1);
        assert_eq!(toasts.list[0].severity, Severity::Error);
        toasts.expire(later + Duration::from_secs(3600));
        assert_eq!(toasts.list.len(), 1);
    }

    #[test]
    fn a_notice_shows_its_text_without_a_close_button() {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            notice(ui, Severity::Info, "No model selected", None);
        });
        output.textures_delta.clear();
        let texts: Vec<String> = output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::epaint::Shape::Text(t) => Some(t.galley.text().to_owned()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, ["No model selected"]);
    }

    #[test]
    fn severities_have_their_defaults_and_errors_stay() {
        assert_eq!(
            Severity::Info.default_duration(),
            Some(Duration::from_secs(5))
        );
        assert_eq!(
            Severity::Success.default_duration(),
            Some(Duration::from_secs(4))
        );
        assert_eq!(
            Severity::Warning.default_duration(),
            Some(Duration::from_secs(8))
        );
        assert_eq!(Severity::Error.default_duration(), None);
        for severity in [
            Severity::Info,
            Severity::Success,
            Severity::Warning,
            Severity::Error,
        ] {
            assert_ne!(severity.label(), "");
        }
    }

    #[test]
    fn the_close_button_lights_in_its_toasts_colour_and_keeps_its_icon_readable() {
        let text = theme::TEXT_PRIMARY.rgba;
        for severity in [
            Severity::Info,
            Severity::Success,
            Severity::Warning,
            Severity::Error,
        ] {
            let fill = severity.fill();
            let hovered = crate::widgets::hovered_on(fill);
            // Lighter than the toast, of its hue: not the interface's violet.
            assert!(hovered.r() >= fill.r() && hovered.g() >= fill.g() && hovered.b() >= fill.b());
            assert_ne!(hovered, fill);
            assert_ne!(hovered, theme::INTERACTION.color());
            assert_ne!(hovered, theme::PRIMARY.color());
            // The icon on it keeps the non-text contrast of WCAG 1.4.11.
            let ratio = theme::contrast(hovered.to_srgba_unmultiplied(), text);
            assert!(ratio >= 3.0, "{severity:?}: {ratio:.2}");
        }
    }

    #[test]
    fn at_most_three_show_the_newest_lowest() {
        let ctx = egui::Context::default();
        let mut toasts = Toasts::default();
        for k in 0..5 {
            toasts.push(Severity::Warning, format!("Warning {k}"), false, None);
        }
        let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0));
        // egui lays a new area out unseen in its first frame and shows it in the next.
        let mut output = egui::FullOutput::default();
        for _ in 0..2 {
            output = ctx.run_ui(egui::RawInput::default(), |ui| {
                toasts.show(ui.ctx(), viewport, None);
            });
            output.textures_delta.clear();
        }
        // Every toast is kept (none timed out), three drawn: their texts are in the shapes.
        assert_eq!(toasts.list.len(), 5);
        let texts: Vec<String> = output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::epaint::Shape::Text(t) => Some(t.galley.text().to_owned()),
                _ => None,
            })
            .collect();
        for k in 0..5 {
            let drawn = texts.iter().any(|t| t == &format!("Warning {k}"));
            assert_eq!(drawn, k >= 2, "Warning {k}");
        }
    }
}
