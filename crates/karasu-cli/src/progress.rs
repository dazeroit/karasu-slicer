//! Advisory stderr progress for long-running commands; never part of the stdout report contract.
//!
//! Live mode rewrites one terminal line; line mode prints one record per completed percentage for
//! logs and wrappers. Progress write failures are ignored because they cannot affect the artifact.

use std::{
    fmt::Write as _,
    io::{self, IsTerminal, Write},
    time::{Duration, Instant},
};

/// Caller-selected reporting policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ProgressMode {
    /// Live progress only when stderr is a terminal.
    Auto,
    /// Always report: live on a terminal, otherwise one line per completed percentage.
    Always,
    /// Never report.
    Never,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Style {
    Off,
    Live,
    Lines,
}

/// Phase and layer-count reporter writing to stderr.
pub(super) struct Progress {
    style: Style,
    /// Characters of the current live line, so a shorter update can erase it without ANSI codes.
    width: usize,
    total_layers: usize,
    last_percent: Option<usize>,
    started: Option<Instant>,
}

impl Progress {
    pub(super) fn new(mode: ProgressMode) -> Self {
        let terminal = io::stderr().is_terminal();
        let style = match mode {
            ProgressMode::Never => Style::Off,
            ProgressMode::Auto if !terminal => Style::Off,
            _ if terminal => Style::Live,
            _ => Style::Lines,
        };
        Self {
            style,
            width: 0,
            total_layers: 0,
            last_percent: None,
            started: None,
        }
    }

    /// Announce a named pipeline phase.
    pub(super) fn phase(&mut self, name: &str) {
        self.emit(&format!("{name}..."));
    }

    /// Begin per-layer reporting for a known nonzero total.
    pub(super) fn begin_layers(&mut self, total: usize) {
        self.total_layers = total;
        self.last_percent = None;
        self.started = Some(Instant::now());
        self.layers(0);
    }

    /// Report completed layers; output is throttled to percentage changes.
    pub(super) fn layers(&mut self, completed: usize) {
        if self.style == Style::Off || self.total_layers == 0 {
            return;
        }
        let percent = completed.saturating_mul(100) / self.total_layers;
        if self.last_percent == Some(percent) {
            return;
        }
        self.last_percent = Some(percent);
        let total = self.total_layers;
        let mut text = format!("Slicing layer {completed}/{total} ({percent}%)");
        if let (Some(started), true) = (self.started, completed > 0 && completed < total) {
            // Linear extrapolation from the observed average layer time.
            let remaining = remaining(started.elapsed(), completed, total);
            let _ = write!(text, ", about {} remaining", duration(remaining));
        }
        self.emit(&text);
    }

    /// Erase any live line so later stderr output starts on a clean line.
    pub(super) fn finish(&mut self) {
        if self.style == Style::Live && self.width > 0 {
            let mut stderr = io::stderr().lock();
            let _ = write!(stderr, "\r{:width$}\r", "", width = self.width);
            let _ = stderr.flush();
            self.width = 0;
        }
    }

    /// Print an out-of-band notice (for example an interrupt request) on its own line.
    pub(super) fn notice(&mut self, text: &str) {
        self.finish();
        let _ = writeln!(io::stderr().lock(), "{text}");
    }

    fn emit(&mut self, text: &str) {
        let mut stderr = io::stderr().lock();
        match self.style {
            Style::Off => {}
            Style::Lines => {
                let _ = writeln!(stderr, "progress: {text}");
            }
            Style::Live => {
                let width = text.chars().count();
                let pad = self.width.saturating_sub(width);
                let _ = write!(stderr, "\r{text}{:pad$}", "");
                let _ = stderr.flush();
                self.width = width;
            }
        }
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        self.finish();
    }
}

fn remaining(elapsed: Duration, completed: usize, total: usize) -> Duration {
    let wide = |n: usize| u128::try_from(n).unwrap_or(u128::MAX);
    let nanos = elapsed
        .as_nanos()
        .saturating_mul(wide(total.saturating_sub(completed)))
        / wide(completed.max(1));
    Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
}

fn duration(value: Duration) -> String {
    let seconds = value.as_secs();
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// The pipeline reports its phases and completed layers here.
impl karasu_project::pipeline::Observer for Progress {
    fn phase(&mut self, name: &str) {
        Progress::phase(self, name);
    }

    fn begin_layers(&mut self, total: usize) {
        Progress::begin_layers(self, total);
    }

    fn layers(&mut self, completed: usize) {
        Progress::layers(self, completed);
    }
}

#[cfg(test)]
mod tests {
    use super::{duration, remaining};
    use std::time::Duration;

    #[test]
    fn durations_use_minutes_and_hours_without_locale() {
        assert_eq!(duration(Duration::from_secs(0)), "0:00");
        assert_eq!(duration(Duration::from_secs(75)), "1:15");
        assert_eq!(duration(Duration::from_secs(3_725)), "1:02:05");
    }

    #[test]
    fn remaining_time_extrapolates_the_average_layer_time() {
        let elapsed = Duration::from_secs(10);
        assert_eq!(remaining(elapsed, 25, 100), Duration::from_secs(30));
        assert_eq!(remaining(elapsed, 100, 100), Duration::ZERO);
        assert_eq!(remaining(elapsed, 0, 100), Duration::from_secs(1_000));
    }
}
