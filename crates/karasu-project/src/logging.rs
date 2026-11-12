//! Session logs (TD-096): what the application and the command line did, kept for the user to
//! send with a problem.
//!
//! **One logger** implements the [`log`] facade. Our code logs through it, under the areas in
//! [`AREAS`], and so do the libraries of the desktop stack (egui, wgpu, winit), under their own
//! module targets.
//!
//! **Levels.** Our records are kept from `info` by default. `KARASU_LOG` (`off`, `error`,
//! `warn`, `info`, `debug`, `trace`) changes that, for a debugging session. Other targets are
//! kept from `warn`, which keeps wgpu's validation errors.
//!
//! **Sinks.** Every kept record goes to:
//! - a **ring** of recent records, numbered in order, which a live log window reads without
//!   holding a lock while it draws;
//! - a **file**, when one is configured. Writes are buffered. A warning, an error or a panic is
//!   flushed at once, anything else within a second by a background thread. The file stops at
//!   its size cap with a last line saying so; later records still reach the ring.
//!
//! **Session files.** The application keeps one file per session in a folder:
//! - the name starts with the session's UTC start, so names sort by age;
//! - the oldest are removed beyond the number kept;
//! - the command line writes one named file only, when asked (`--log <file>`).
//!
//! **Format.** One English line per record, never localized:
//! `2026-10-23T09:41:07.123Z +12.345s INFO  slice: message`. A message's own line breaks are
//! indented on the following lines, so every record starts at the line's beginning.
//!
//! **Panics.** [`install`] adds a panic hook that records the message, the place and a captured
//! backtrace at `error`, flushes the file, then runs the previous hook.
//!
//! Logging never changes a result or an output, and nothing should be logged per frame.

use log::{Level, LevelFilter, Log, Metadata};
use std::{
    collections::VecDeque,
    fmt::{self, Write as _},
    fs::{self, File},
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, PoisonError,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// The areas our records are logged under (`log::info!(target: "slice", …)`).
pub const AREAS: &[&str] = &[
    "app", "session", "models", "repair", "prepare", "hollow", "holes", "slice", "verify",
    "project", "messages", "panic",
];

/// Sessions kept in the application's log folder (TD-096).
pub const SESSIONS_KEPT: usize = 10;

/// Largest log file, in bytes (TD-096).
pub const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;

/// Records kept in memory for a live log window (TD-096).
pub const RING_RECORDS: usize = 10_000;

/// The environment variable that sets our records' level.
pub const LEVEL_VARIABLE: &str = "KARASU_LOG";

/// How often buffered records are written out at the latest.
const FLUSH_PERIOD: Duration = Duration::from_secs(1);

/// One kept record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    /// Its number, from 0 in the order records were kept.
    pub sequence: u64,
    /// When it was made, in milliseconds since the Unix epoch (UTC).
    pub unix_ms: i64,
    /// When it was made, in milliseconds since the logger started.
    pub elapsed_ms: u64,
    /// Its level.
    pub level: Level,
    /// Its area, or another library's target.
    pub target: String,
    /// Its message.
    pub message: String,
}

impl fmt::Display for Record {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} +{}.{:03}s {:<5} {}: ",
            utc(self.unix_ms),
            self.elapsed_ms / 1000,
            self.elapsed_ms % 1000,
            self.level,
            self.target
        )?;
        let mut lines = self.message.lines();
        f.write_str(lines.next().unwrap_or(""))?;
        for line in lines {
            write!(f, "\n    {line}")?;
        }
        Ok(())
    }
}

/// `unix_ms` as an ISO 8601 UTC time with milliseconds.
#[must_use]
pub fn utc(unix_ms: i64) -> String {
    let (days, ms) = (
        unix_ms.div_euclid(86_400_000),
        unix_ms.rem_euclid(86_400_000),
    );
    // Days since 1970-01-01 to a civil date (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}

/// The recent records, numbered, for a live log window.
#[derive(Debug)]
pub struct Ring {
    capacity: usize,
    records: Mutex<VecDeque<Record>>,
}

impl Ring {
    fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            records: Mutex::new(VecDeque::new()),
        }
    }

    fn push(&self, record: Record) {
        let mut records = self.records.lock().unwrap_or_else(PoisonError::into_inner);
        if records.len() == self.capacity {
            records.pop_front();
        }
        records.push_back(record);
    }

    /// The records numbered from `sequence` on that are still kept, oldest first. A window
    /// passes one past the last record it showed.
    #[must_use]
    pub fn since(&self, sequence: u64) -> Vec<Record> {
        let records = self.records.lock().unwrap_or_else(PoisonError::into_inner);
        let skip = records
            .front()
            .map_or(0, |first| sequence.saturating_sub(first.sequence));
        records
            .iter()
            .skip(usize::try_from(skip).unwrap_or(usize::MAX))
            .cloned()
            .collect()
    }
}

/// Where records are written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Destination {
    /// Nowhere but the ring.
    Memory,
    /// A new session file in this folder, keeping the newest `keep` sessions (the application).
    Session {
        /// The log folder, created when missing.
        folder: PathBuf,
        /// Session files kept, this one included.
        keep: usize,
    },
    /// This file, replaced (the command line's `--log`).
    File(PathBuf),
}

/// How a logger is set up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogConfig {
    /// Where its records are written.
    pub destination: Destination,
    /// The level our areas are kept from.
    pub ours: LevelFilter,
    /// The level other targets are kept from.
    pub others: LevelFilter,
    /// Largest file, in bytes.
    pub max_file_bytes: u64,
    /// Records kept in the ring.
    pub ring_records: usize,
}

impl LogConfig {
    /// The defaults of TD-096 for `destination`, with our level from [`LEVEL_VARIABLE`] when it
    /// is set to a level name.
    #[must_use]
    pub fn new(destination: Destination) -> Self {
        Self {
            destination,
            ours: std::env::var(LEVEL_VARIABLE)
                .ok()
                .and_then(|value| value.trim().parse().ok())
                .unwrap_or(LevelFilter::Info),
            others: LevelFilter::Warn,
            max_file_bytes: MAX_FILE_BYTES,
            ring_records: RING_RECORDS,
        }
    }
}

/// Why logging could not start.
#[derive(Debug)]
pub enum LogError {
    /// The folder or the file could not be created.
    Io {
        /// The path.
        path: PathBuf,
        /// The cause.
        source: io::Error,
    },
    /// A logger was already installed in this process.
    AlreadyInstalled,
}

impl fmt::Display for LogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "cannot write the log {}: {source}", path.display())
            }
            Self::AlreadyInstalled => f.write_str("a logger is already installed"),
        }
    }
}

impl std::error::Error for LogError {}

/// The open log file.
struct Sink {
    writer: BufWriter<File>,
    written: u64,
    capped: bool,
    dirty: bool,
}

/// The logger: the ring and, when configured, the file.
pub struct Logger {
    config: LogConfig,
    started: Instant,
    started_unix_ms: i64,
    next: Mutex<u64>,
    ring: Arc<Ring>,
    sink: Option<Mutex<Sink>>,
    path: Option<PathBuf>,
}

impl fmt::Debug for Logger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Logger")
            .field("config", &self.config)
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// The session file name for a session started at `unix_ms` by process `pid`: it sorts by start.
#[must_use]
pub fn session_file_name(unix_ms: i64, pid: u32) -> String {
    let stamp: String = utc(unix_ms)
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect();
    format!("karasu-{stamp}-{pid}.log")
}

/// Remove the oldest session files of `folder` so that `keep - 1` remain, leaving room for a new
/// one. Other files are never touched.
fn prune(folder: &Path, keep: usize) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    let mut sessions: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|e| e == "log")
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("karasu-"))
        })
        .collect();
    sessions.sort();
    let excess = sessions.len().saturating_sub(keep.saturating_sub(1));
    for old in sessions.into_iter().take(excess) {
        let _ = fs::remove_file(old);
    }
}

impl Logger {
    /// A logger for `config`, opening its file.
    ///
    /// # Errors
    /// [`LogError::Io`] when the folder or the file cannot be created.
    pub fn new(config: LogConfig) -> Result<Self, LogError> {
        let started_unix_ms = now_unix_ms();
        let io = |path: &Path| {
            let path = path.to_owned();
            move |source| LogError::Io { path, source }
        };
        let path = match &config.destination {
            Destination::Memory => None,
            Destination::Session { folder, keep } => {
                fs::create_dir_all(folder).map_err(io(folder))?;
                prune(folder, *keep);
                Some(folder.join(session_file_name(started_unix_ms, std::process::id())))
            }
            Destination::File(path) => Some(path.clone()),
        };
        let sink = path
            .as_ref()
            .map(|path| {
                File::create(path).map_err(io(path)).map(|file| {
                    Mutex::new(Sink {
                        writer: BufWriter::new(file),
                        written: 0,
                        capped: false,
                        dirty: false,
                    })
                })
            })
            .transpose()?;
        Ok(Self {
            ring: Arc::new(Ring::new(config.ring_records)),
            config,
            started: Instant::now(),
            started_unix_ms,
            next: Mutex::new(0),
            sink,
            path,
        })
    }

    /// The file written, when there is one.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// The ring of recent records.
    #[must_use]
    pub fn ring(&self) -> Arc<Ring> {
        Arc::clone(&self.ring)
    }

    /// Whether a record of `level` under `target` is kept.
    #[must_use]
    pub fn keeps(&self, level: Level, target: &str) -> bool {
        let ours = AREAS.contains(&target) || target.starts_with("karasu");
        level
            <= if ours {
                self.config.ours
            } else {
                self.config.others
            }
    }

    /// The most detailed level any record is kept at.
    #[must_use]
    pub fn max_level(&self) -> LevelFilter {
        self.config.ours.max(self.config.others)
    }

    /// Keep a record, when its level is kept.
    pub fn keep(&self, level: Level, target: &str, message: String) {
        if !self.keeps(level, target) {
            return;
        }
        let elapsed_ms = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let record = {
            let mut next = self.next.lock().unwrap_or_else(PoisonError::into_inner);
            let record = Record {
                sequence: *next,
                unix_ms: self
                    .started_unix_ms
                    .saturating_add(i64::try_from(elapsed_ms).unwrap_or(i64::MAX)),
                elapsed_ms,
                level,
                target: target.to_owned(),
                message,
            };
            *next += 1;
            // The file follows the numbering's order.
            self.write(&record);
            record
        };
        self.ring.push(record);
    }

    fn write(&self, record: &Record) {
        let Some(sink) = &self.sink else {
            return;
        };
        let mut sink = sink.lock().unwrap_or_else(PoisonError::into_inner);
        if sink.capped {
            return;
        }
        let mut line = record.to_string();
        line.push('\n');
        let length = line.len() as u64;
        if sink.written + length > self.config.max_file_bytes {
            sink.capped = true;
            let _ = writeln!(
                sink.writer,
                "{} +{}s WARN  session: this log reached its {} byte limit; later records are \
                 kept in memory only",
                utc(record.unix_ms),
                record.elapsed_ms / 1000,
                self.config.max_file_bytes
            );
            let _ = sink.writer.flush();
            return;
        }
        if sink.writer.write_all(line.as_bytes()).is_ok() {
            sink.written += length;
            sink.dirty = true;
        }
        if record.level <= Level::Warn {
            let _ = sink.writer.flush();
            sink.dirty = false;
        }
    }

    /// Write out buffered records.
    pub fn flush_file(&self) {
        if let Some(sink) = &self.sink {
            let mut sink = sink.lock().unwrap_or_else(PoisonError::into_inner);
            if sink.dirty {
                let _ = sink.writer.flush();
                sink.dirty = false;
            }
        }
    }
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        self.keeps(metadata.level(), metadata.target())
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            let mut message = String::new();
            let _ = write!(message, "{}", record.args());
            self.keep(record.level(), record.target(), message);
        }
    }

    fn flush(&self) {
        self.flush_file();
    }
}

/// What [`install`] set up: the file and the ring.
#[derive(Clone, Debug)]
pub struct LogHandle {
    path: Option<PathBuf>,
    ring: Arc<Ring>,
}

impl LogHandle {
    /// The file written, when there is one.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// The ring of recent records, for a live log window.
    #[must_use]
    pub fn ring(&self) -> &Arc<Ring> {
        &self.ring
    }
}

/// Install the process's logger for `config`, with the panic hook and the background flush.
///
/// # Errors
/// [`LogError`] when the file cannot be created or a logger is already installed.
pub fn install(config: LogConfig) -> Result<LogHandle, LogError> {
    static INSTALLED: AtomicBool = AtomicBool::new(false);
    if INSTALLED.swap(true, Ordering::AcqRel) {
        return Err(LogError::AlreadyInstalled);
    }
    let logger: &'static Logger = Box::leak(Box::new(Logger::new(config)?));
    log::set_logger(logger).map_err(|_| LogError::AlreadyInstalled)?;
    log::set_max_level(logger.max_level());
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        logger.keep(
            Level::Error,
            "panic",
            format!("{info}\nbacktrace:\n{backtrace}"),
        );
        logger.flush_file();
        previous(info);
    }));
    if logger.sink.is_some() {
        let _ = std::thread::Builder::new()
            .name("karasu-log-flush".to_owned())
            .spawn(move || loop {
                std::thread::sleep(FLUSH_PERIOD);
                logger.flush_file();
            });
    }
    Ok(LogHandle {
        path: logger.path.clone(),
        ring: logger.ring(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("karasu-logging-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn config(destination: Destination) -> LogConfig {
        LogConfig {
            destination,
            ours: LevelFilter::Info,
            others: LevelFilter::Warn,
            max_file_bytes: MAX_FILE_BYTES,
            ring_records: 4,
        }
    }

    #[test]
    fn times_are_utc_with_milliseconds() {
        assert_eq!(utc(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(utc(951_782_400_000), "2000-02-29T00:00:00.000Z");
        assert_eq!(utc(1_792_748_467_123), "2026-10-23T09:41:07.123Z");
        assert_eq!(utc(-1), "1969-12-31T23:59:59.999Z");
        assert_eq!(
            session_file_name(1_792_748_467_123, 42),
            "karasu-20261023T094107123Z-42.log"
        );
    }

    #[test]
    fn records_are_one_line_each_with_continuations_indented() {
        let record = Record {
            sequence: 0,
            unix_ms: 1_792_748_467_123,
            elapsed_ms: 12_345,
            level: Level::Info,
            target: "slice".to_owned(),
            message: "sliced\nlayers: 3".to_owned(),
        };
        assert_eq!(
            record.to_string(),
            "2026-10-23T09:41:07.123Z +12.345s INFO  slice: sliced\n    layers: 3"
        );
    }

    #[test]
    fn levels_differ_for_our_areas_and_other_targets() {
        let logger = Logger::new(config(Destination::Memory)).unwrap();
        assert!(logger.keeps(Level::Info, "slice"));
        assert!(logger.keeps(Level::Info, "karasu_app::window"));
        assert!(!logger.keeps(Level::Debug, "slice"));
        assert!(!logger.keeps(Level::Info, "wgpu_core::device"));
        assert!(logger.keeps(Level::Warn, "wgpu_core::device"));
        assert_eq!(logger.max_level(), LevelFilter::Info);
    }

    #[test]
    fn the_ring_keeps_the_newest_records_in_order() {
        let logger = Logger::new(config(Destination::Memory)).unwrap();
        for k in 0..6 {
            logger.keep(Level::Info, "app", format!("record {k}"));
        }
        logger.keep(Level::Trace, "app", "not kept".to_owned());
        let ring = logger.ring();
        let all: Vec<u64> = ring.since(0).iter().map(|r| r.sequence).collect();
        assert_eq!(all, [2, 3, 4, 5]);
        let newer: Vec<String> = ring.since(4).into_iter().map(|r| r.message).collect();
        assert_eq!(newer, ["record 4", "record 5"]);
        assert_eq!(ring.since(6), Vec::new());
    }

    #[test]
    fn sessions_beyond_those_kept_are_removed_and_other_files_left() {
        let folder = temp("sessions");
        for k in 0..4 {
            fs::write(
                folder.join(format!("karasu-2026010{k}T000000000Z-1.log")),
                "old",
            )
            .unwrap();
        }
        fs::write(folder.join("notes.txt"), "mine").unwrap();
        let logger = Logger::new(config(Destination::Session {
            folder: folder.clone(),
            keep: 3,
        }))
        .unwrap();
        logger.keep(Level::Error, "app", "kept".to_owned());
        let mut names: Vec<String> = fs::read_dir(&folder)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names.len(), 4, "{names:?}");
        assert!(names.contains(&"notes.txt".to_owned()));
        assert!(names.contains(&"karasu-20260102T000000000Z-1.log".to_owned()));
        assert!(names.contains(&"karasu-20260103T000000000Z-1.log".to_owned()));
        let current = fs::read_to_string(logger.path().unwrap()).unwrap();
        assert!(current.ends_with("ERROR app: kept\n"), "{current}");
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_file_stops_at_its_cap_with_a_last_line_and_the_ring_goes_on() {
        let folder = temp("cap");
        let path = folder.join("cli.log");
        let logger = Logger::new(LogConfig {
            max_file_bytes: 200,
            ..config(Destination::File(path.clone()))
        })
        .unwrap();
        for k in 0..10 {
            logger.keep(Level::Warn, "slice", format!("record number {k}"));
        }
        let text = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines.len() < 10);
        assert!(
            lines.last().unwrap().contains("reached its 200 byte limit"),
            "{text}"
        );
        assert!(text.len() < 400);
        assert_eq!(
            logger.ring().since(0).last().unwrap().message,
            "record number 9"
        );
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn information_is_buffered_until_flushed() {
        let folder = temp("buffer");
        let path = folder.join("cli.log");
        let logger = Logger::new(config(Destination::File(path.clone()))).unwrap();
        logger.keep(Level::Info, "slice", "buffered".to_owned());
        assert_eq!(fs::read_to_string(&path).unwrap(), "");
        logger.flush_file();
        assert!(fs::read_to_string(&path).unwrap().contains("buffered"));
        let _ = fs::remove_dir_all(&folder);
    }
}
