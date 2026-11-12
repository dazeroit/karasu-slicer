//! Where the windows appear (TD-075): the splash and the main window are centred on the monitor,
//! and the main window opens in the chosen mode.
//!
//! The window reopens as it closed (its mode, and as a window its size and place, TD-097);
//! `--window` chooses the mode instead. The monitor is the one the window opens on, as the
//! windowing system reports its size; positions are in points.
use eframe::egui::{self, Pos2, Vec2};

/// How the main window opens after the splash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowMode {
    /// A centred window of the default size.
    Windowed,
    /// Maximized, with its title bar and the taskbar visible (the default).
    Maximized,
    /// Fullscreen, without a title bar, covering the monitor.
    Fullscreen,
}

impl WindowMode {
    /// Parse a `--window` value.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "windowed" => Self::Windowed,
            "maximized" | "maximised" => Self::Maximized,
            "fullscreen" => Self::Fullscreen,
            _ => return None,
        })
    }
}

/// The top-left corner that centres a window of `size` on a monitor of `monitor` size, never
/// left of or above the monitor's own corner.
#[must_use]
pub fn centred(monitor: Vec2, size: Vec2) -> Pos2 {
    ((monitor - size) / 2.0).max(Vec2::ZERO).to_pos2()
}

/// Where a window of `size` kept at `position` reopens on a monitor of `monitor_size`: there,
/// when enough of it shows on the monitor to be grabbed (a title bar's worth), or else centred.
/// A window left on another monitor, or on one since removed, comes back centred.
#[must_use]
pub fn kept_or_centred(position: Option<Pos2>, size: Vec2, monitor_size: Vec2) -> Pos2 {
    const GRAB: Vec2 = Vec2::new(120.0, 40.0);
    let monitor = egui::Rect::from_min_size(Pos2::ZERO, monitor_size);
    position
        .filter(|p| {
            let title = egui::Rect::from_min_size(*p, egui::vec2(size.x, GRAB.y));
            let shown = title.intersect(monitor);
            shown.width() >= GRAB.x && shown.height() >= GRAB.y
        })
        .unwrap_or_else(|| centred(monitor_size, size))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kept_window_reopens_where_it_was_unless_off_the_monitor() {
        let monitor = Vec2::new(1920.0, 1080.0);
        let size = Vec2::new(1280.0, 800.0);
        let kept = Pos2::new(100.0, 50.0);
        assert_eq!(kept_or_centred(Some(kept), size, monitor), kept);
        // Mostly off the right edge, or above the top: centred.
        let centre = centred(monitor, size);
        assert_eq!(
            kept_or_centred(Some(Pos2::new(1850.0, 50.0)), size, monitor),
            centre
        );
        assert_eq!(
            kept_or_centred(Some(Pos2::new(100.0, -60.0)), size, monitor),
            centre
        );
        assert_eq!(kept_or_centred(None, size, monitor), centre);
        // Partly off the left edge, its title still to grab: kept.
        let left = Pos2::new(-600.0, 50.0);
        assert_eq!(kept_or_centred(Some(left), size, monitor), left);
    }

    #[test]
    fn modes_parse_and_windows_centre() {
        assert_eq!(WindowMode::parse("maximized"), Some(WindowMode::Maximized));
        assert_eq!(WindowMode::parse("windowed"), Some(WindowMode::Windowed));
        assert_eq!(
            WindowMode::parse("fullscreen"),
            Some(WindowMode::Fullscreen)
        );
        assert_eq!(WindowMode::parse("tiny"), None);
        assert_eq!(
            centred(Vec2::new(1920.0, 1080.0), Vec2::new(720.0, 420.0)),
            Pos2::new(600.0, 330.0)
        );
        // A window larger than the monitor starts at its corner.
        assert_eq!(
            centred(Vec2::new(1280.0, 720.0), Vec2::new(1400.0, 800.0)),
            Pos2::ZERO
        );
    }
}
