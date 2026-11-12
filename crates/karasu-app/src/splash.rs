//! Startup splash screen: a borderless window showing the bundled artwork
//! (`assets/ui/images/splash.png`, the owner's artwork with the product name, square and opaque),
//! the subtitle and version, and a progress bar of the real startup steps. The artwork carries the
//! name, so the application does not draw it.
//!
//! **Progress:** the bar is the fraction of completed startup steps; it never animates on time
//! alone. The splash stays at least [`MINIMUM_DISPLAY`] so it does not flash; with nothing left
//! to do it simply shows a full bar until then.
//!
//! **Opaque (TD-141):** the window is not transparent. The rounded corners of TD-069 showed dark
//! on native Windows, where the surface does not compose alpha, so the artwork is square.
use crate::{i18n::tr, theme};
use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, Stroke, Vec2};
use std::time::{Duration, Instant};

/// The bundled artwork, at twice the splash size in points.
const ARTWORK: &[u8] = include_bytes!("../../../assets/ui/images/splash.png");
/// Splash size in points (the artwork is drawn at its pixel size divided by two).
pub const SIZE: Vec2 = Vec2::new(720.0, 405.0);
/// Shortest time the splash is shown.
pub const MINIMUM_DISPLAY: Duration = Duration::from_millis(1500);

/// Decoded straight-alpha RGBA artwork.
pub use crate::icons::Image as Artwork;

/// Decode the bundled artwork (8-bit RGBA PNG).
///
/// # Errors
/// A message when the PNG is malformed or not 8-bit RGBA.
pub fn decode_artwork() -> Result<Artwork, String> {
    crate::icons::decode_png(ARTWORK).map_err(|e| format!("splash artwork: {e}"))
}

/// One startup step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Decoding and uploading the interface artwork.
    Artwork,
    /// The graphics device has rendered a frame.
    Graphics,
    /// The main workspace state is ready.
    Workspace,
}

impl Step {
    /// Every step, in order.
    pub const ALL: [Self; 3] = [Self::Artwork, Self::Graphics, Self::Workspace];

    /// What the splash shows while the step runs.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::Artwork => tr!("splash-step-artwork"),
            Self::Graphics => tr!("splash-step-graphics"),
            Self::Workspace => tr!("splash-step-workspace"),
        }
    }
}

/// Startup progress.
#[derive(Debug)]
pub struct Startup {
    started: Instant,
    done: Vec<Step>,
}

impl Startup {
    /// Startup beginning now.
    #[must_use]
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
            done: Vec::new(),
        }
    }

    /// Record a completed step (idempotent).
    pub fn complete(&mut self, step: Step) {
        if !self.done.contains(&step) {
            self.done.push(step);
        }
    }

    /// Completed fraction, from 0 to 1.
    #[must_use]
    pub fn fraction(&self) -> f32 {
        #[allow(clippy::cast_precision_loss)] // A handful of steps.
        let fraction = self.done.len() as f32 / Step::ALL.len() as f32;
        fraction
    }

    /// The first step not yet completed.
    #[must_use]
    pub fn current(&self) -> Option<Step> {
        Step::ALL.into_iter().find(|s| !self.done.contains(s))
    }

    /// Whether every step is done and the splash has been shown long enough at `now`.
    #[must_use]
    pub fn finished(&self, now: Instant) -> bool {
        self.current().is_none() && now.duration_since(self.started) >= MINIMUM_DISPLAY
    }

    /// Time since startup began.
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }
}

/// Left edge of the artwork's title, in points: the text below lines up with it.
const TEXT_LEFT: f32 = 69.0;
/// Top of the subtitle, in points, just below the artwork's title.
const SUBTITLE_TOP: f32 = 222.0;
/// Top of the progress bar, in points.
const BAR_TOP: f32 = 280.0;

/// Draw the splash into `ui`: artwork (or a fallback panel with the name), subtitle, version,
/// step and progress bar.
pub fn show(ui: &mut egui::Ui, artwork: Option<&egui::TextureHandle>, startup: &Startup) {
    let rect = ui.max_rect();
    let painter = ui.painter();
    if let Some(texture) = artwork {
        painter.image(
            texture.id(),
            rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
    } else {
        painter.rect(
            rect,
            CornerRadius::ZERO,
            theme::SURFACE.color(),
            Stroke::new(1.0, theme::BORDER.color()),
            egui::StrokeKind::Inside,
        );
        // Without the artwork, the name it carries.
        painter.text(
            Pos2::new(rect.left() + TEXT_LEFT, rect.top() + SUBTITLE_TOP - 10.0),
            egui::Align2::LEFT_BOTTOM,
            karasu_core::APPLICATION_NAME,
            egui::FontId::proportional(40.0),
            theme::TEXT_PRIMARY.color(),
        );
    }
    let left = rect.left() + TEXT_LEFT;
    painter.text(
        Pos2::new(left, rect.top() + SUBTITLE_TOP),
        egui::Align2::LEFT_TOP,
        tr!("splash-tagline"),
        egui::FontId::proportional(18.0),
        theme::ACCENT.color(),
    );
    // Progress: the current step above a bar of the completed fraction, clear of the raven, and
    // the version below.
    let bar = Rect::from_min_size(Pos2::new(left, rect.top() + BAR_TOP), Vec2::new(220.0, 6.0));
    painter.text(
        bar.left_bottom() + Vec2::new(0.0, 12.0),
        egui::Align2::LEFT_TOP,
        tr!("splash-version", version = env!("CARGO_PKG_VERSION")),
        egui::FontId::proportional(14.0),
        theme::TEXT_PRIMARY.color(),
    );
    let status = startup
        .current()
        .map_or_else(|| tr!("splash-ready"), Step::label);
    painter.text(
        bar.left_top() - Vec2::new(0.0, 8.0),
        egui::Align2::LEFT_BOTTOM,
        status,
        egui::FontId::proportional(13.0),
        theme::ACCENT.color(),
    );
    painter.rect_filled(bar, CornerRadius::same(3), theme::BORDER.color());
    let filled = Rect::from_min_size(
        bar.min,
        Vec2::new(bar.width() * startup.fraction(), bar.height()),
    );
    painter.rect_filled(filled, CornerRadius::same(3), theme::PRIMARY.color());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artwork_is_opaque_rgba_at_twice_the_splash_size() {
        let art = decode_artwork().unwrap();
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let expected = [(SIZE.x * 2.0) as usize, (SIZE.y * 2.0) as usize];
        assert_eq!(art.size, expected);
        assert_eq!(art.rgba.len(), art.size[0] * art.size[1] * 4);
        // Opaque everywhere, corners included: the window is not transparent.
        let (pixels, rest) = art.rgba.as_chunks::<4>();
        assert!(rest.is_empty() && pixels.iter().all(|p| p[3] == 255));
    }

    #[test]
    fn progress_counts_real_steps_and_waits_for_the_minimum_display() {
        let mut startup = Startup::new();
        assert_eq!(startup.current(), Some(Step::Artwork));
        assert!((startup.fraction() - 0.0).abs() < f32::EPSILON);
        startup.complete(Step::Graphics);
        startup.complete(Step::Graphics);
        assert_eq!(startup.current(), Some(Step::Artwork));
        assert!((startup.fraction() - 1.0 / 3.0).abs() < 1e-6);
        startup.complete(Step::Artwork);
        startup.complete(Step::Workspace);
        assert_eq!(startup.current(), None);
        assert!((startup.fraction() - 1.0).abs() < f32::EPSILON);
        let begin = startup.started;
        assert!(!startup.finished(begin));
        assert!(startup.finished(begin + MINIMUM_DISPLAY));
    }
}
