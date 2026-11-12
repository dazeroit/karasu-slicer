//! karasu-slicer desktop application (M6).
//!
//! **M6.0 foundations:** a native window themed with the approved palette, on the wgpu renderer
//! selected in TD-042, with a deterministic adapter choice and an OpenGL fallback (TD-067).
//!
//! **M6.2 viewport (TD-073):** the main window shows the plate of a `karasu-project` project in a
//! 3D viewport ([`window`], [`viewport`], [`camera`]). Models and profiles are opened by dropping
//! them on the window or naming them on the command line ([`workspace`]). All slicing and
//! preparation stay in the headless crates.
//!
//! **Startup:** the window opens as a borderless splash (artwork, version and the progress of the
//! real startup steps, see [`splash`]), then becomes the decorated main window.
//!
//! **Options:**
//! - `--gpu-backend auto|vulkan|dx12|metal|gl` selects the graphics backends (default `auto`).
//! - `--smoke-test` renders a few frames, prints the chosen adapter as one JSON line on standard
//!   output and exits; it fails after a watchdog timeout when no frame is rendered. With files,
//!   it waits for their import, turns the camera for a measured run and reports the frame times
//!   and the triangles drawn; a failed import fails it. Native validation uses it.
//! - `--locale <tag>` chooses the interface language for this start, instead of the settings'
//!   (`pseudo` for the pseudo-locale), and `--locale-dir <folder>` adds catalogs from
//!   `<folder>/<tag>/*.ftl` ([`i18n`], TD-080).
//! - `--window windowed|maximized|fullscreen` chooses how the main window opens this time,
//!   instead of the settings' (TD-075, TD-097).
//! - `--dev` is the developer mode: the settings list the pseudo-locale among the languages.
//! - The user's settings are read from and saved to the preferences root (TD-097), except in a
//!   smoke test.
//! - Any other argument is a file to open: a model, or a printer or resin profile.
//!
//! **Windows console:** Release builds use the Windows GUI subsystem, so no console window opens
//! with the interface. Standard output and error then reach only a caller that redirects them
//! (native validation does); Debug builds keep the console for development diagnostics.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(test)]
mod accessibility_check;
mod actions;
mod appearance;
mod auto_ui;
mod camera;
mod desktop;
mod dialog;
mod focus;
mod gizmo;
mod gpu;
mod grid_ui;
mod hollowing_ui;
mod i18n;
// The icon resource the build script links into `karasu.exe`, compiled here for its tests.
#[cfg(test)]
mod icon_resource;
mod icons;
mod islands_ui;
mod layer_plan_ui;
mod list_window;
mod log_window;
mod manipulate;
mod messages;
mod orient_ui;
mod preparation_ui;
mod profiles_ui;
mod raft_ui;
mod repair_panel;
mod repair_ui;
#[cfg(test)]
mod scenarios;
mod screen;
mod section_ui;
mod settings;
mod shown;
mod sidebar;
mod slicing_ui;
mod splash;
mod supports_ui;
mod theme;
mod toasts;
mod tool_strip;
mod transform_ui;
mod viewcube;
mod viewport;
mod visibility;
mod widgets;
mod window;
mod workflow;
mod workspace;

use eframe::egui;
use std::{
    path::PathBuf,
    process::ExitCode,
    sync::{Arc, Mutex},
    time::Duration,
};

/// Frames rendered before a smoke test without files succeeds.
const SMOKE_FRAMES: u32 = 10;
/// Frames of the measured run of a smoke test with files.
const SMOKE_MEASURED_FRAMES: u32 = 90;
/// Time after which a smoke test that has not finished fails.
const SMOKE_TIMEOUT: Duration = Duration::from_secs(120);

/// Parsed command-line options.
#[derive(Debug, PartialEq, Eq)]
struct Options {
    backend: gpu::BackendChoice,
    smoke_test: bool,
    /// `--window`, which overrides how the window last closed.
    window: Option<screen::WindowMode>,
    /// `--locale`, which overrides the settings' language for this start.
    locale: Option<String>,
    /// `--dev`: developer mode.
    dev: bool,
    locale_dir: Option<PathBuf>,
    files: Vec<PathBuf>,
}

fn parse(arguments: impl Iterator<Item = String>) -> Result<Options, String> {
    let mut options = Options {
        backend: gpu::BackendChoice::Auto,
        smoke_test: false,
        window: None,
        locale: None,
        dev: false,
        locale_dir: None,
        files: Vec::new(),
    };
    let mut arguments = arguments;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--gpu-backend" => {
                let value = arguments.next().unwrap_or_default();
                options.backend = gpu::BackendChoice::parse(&value).ok_or_else(|| {
                    format!("--gpu-backend must be auto, vulkan, dx12, metal or gl, not {value:?}")
                })?;
            }
            "--smoke-test" => options.smoke_test = true,
            "--window" => {
                let value = arguments.next().unwrap_or_default();
                options.window = Some(screen::WindowMode::parse(&value).ok_or_else(|| {
                    format!("--window must be windowed, maximized or fullscreen, not {value:?}")
                })?);
            }
            "--locale" => {
                options.locale = Some(
                    arguments
                        .next()
                        .ok_or("--locale requires a language tag such as en-US")?,
                );
            }
            "--dev" => options.dev = true,
            "--locale-dir" => {
                options.locale_dir = Some(PathBuf::from(
                    arguments.next().ok_or("--locale-dir requires a folder")?,
                ));
            }
            "-h" | "--help" => return Err(help()),
            other if other.starts_with('-') => {
                return Err(format!("unknown option {other:?}\n\n{}", help()))
            }
            file => options.files.push(PathBuf::from(file)),
        }
    }
    Ok(options)
}

fn help() -> String {
    format!(
        "{} {} desktop application\n\nUsage: karasu [--gpu-backend auto|vulkan|dx12|metal|gl] \
         [--smoke-test] [--window windowed|maximized|fullscreen]\n       [--locale <tag>] [--locale-dir <folder>] [--dev] [FILE...]\n\n\
         --window and --locale override the settings for this start; --locale pseudo shows the\n\
         pseudo-locale. --locale-dir adds catalogs from <folder>/<tag>/*.ftl, overriding bundled\n\
         messages. --dev is the developer mode (the pseudo-locale among the languages).\n\
         FILE: a project (.karasu) or a model to open, or a printer or resin profile (JSON).",
        karasu_core::APPLICATION_NAME,
        env!("CARGO_PKG_VERSION")
    )
}

/// Main window size in points, after the splash.
const MAIN_SIZE: egui::Vec2 = egui::Vec2::new(1280.0, 800.0);
/// Smallest main window size in points.
const MAIN_MIN_SIZE: egui::Vec2 = egui::Vec2::new(640.0, 400.0);

/// Startup splash, then the main window.
enum Phase {
    Splash {
        startup: splash::Startup,
        artwork: Option<egui::TextureHandle>,
        frames: u32,
    },
    Main {
        /// Time the splash was shown.
        splash: Duration,
        /// Main-window frames rendered.
        frames: u32,
        /// Frames until a windowed main window is centred again with its actual outer size
        /// (title bar and borders included), or `None` once done.
        recentre_in: Option<u32>,
    },
}

/// The application state of M6.0: the chosen adapter, the phase and the smoke-test switch.
struct App {
    chosen: Arc<Mutex<Option<gpu::Selected>>>,
    smoke_test: bool,
    phase: Phase,
    /// `--window`: how the main window opens, instead of how it last closed.
    mode: Option<screen::WindowMode>,
    window: window::MainWindow,
    /// Frame times of a smoke test's measured run.
    timing: window::Timing,
}

impl App {
    fn selected(&self) -> Option<gpu::Selected> {
        self.chosen.lock().ok().and_then(|slot| slot.clone())
    }

    /// One splash frame: run the startup steps, draw, and switch to the main window when done.
    fn splash_frame(&mut self, ui: &mut egui::Ui) {
        let Phase::Splash {
            startup,
            artwork,
            frames,
        } = &mut self.phase
        else {
            return;
        };
        if *frames == 0 {
            // Centre the splash on its monitor, whatever the windowing system chose.
            let ctx = ui.ctx();
            if let Some(monitor) = ctx.input(|i| i.viewport().monitor_size) {
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(screen::centred(
                    monitor,
                    splash::SIZE,
                )));
            }
        }
        if startup.current() == Some(splash::Step::Artwork) {
            match splash::decode_artwork() {
                Ok(art) => {
                    let image = egui::ColorImage::from_rgba_unmultiplied(art.size, &art.rgba);
                    *artwork = Some(ui.ctx().load_texture(
                        "splash-artwork",
                        image,
                        egui::TextureOptions::LINEAR,
                    ));
                }
                // The splash falls back to a plain rounded panel; startup continues.
                Err(error) => eprintln!("warning: cannot load the splash artwork: {error}"),
            }
            startup.complete(splash::Step::Artwork);
        }
        // A previous splash frame was presented: the graphics device works.
        if *frames > 0 {
            startup.complete(splash::Step::Graphics);
        }
        if startup.current() == Some(splash::Step::Workspace) {
            startup.complete(splash::Step::Workspace);
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new())
            .show(ui, |ui| splash::show(ui, artwork.as_ref(), startup));
        *frames += 1;
        if startup.finished(std::time::Instant::now()) {
            let ctx = ui.ctx();
            ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Resizable(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(MAIN_MIN_SIZE));
            // As the window last closed (TD-097): its size and place as a window, kept on the
            // monitor, so a maximized window also restores to them; else the default size,
            // centred.
            let kept = self.window.window_settings();
            let monitor = ctx.input(|i| i.viewport().monitor_size);
            let size = kept
                .size
                .map_or(MAIN_SIZE, |[w, h]| egui::vec2(w, h))
                .max(MAIN_MIN_SIZE)
                .min(monitor.unwrap_or(egui::Vec2::INFINITY));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
            // "As a window, centred" leaves the kept place aside.
            let position = kept
                .position
                .filter(|_| kept.open_as != settings::OpenAs::Windowed)
                .map(|[x, y]| egui::pos2(x, y));
            if let Some(monitor) = monitor {
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(
                    screen::kept_or_centred(position, size, monitor),
                ));
            }
            let mode = self.mode.unwrap_or(match (kept.open_as, kept.state) {
                (settings::OpenAs::Maximized, _)
                | (settings::OpenAs::AsClosed, settings::WindowState::Maximized) => {
                    screen::WindowMode::Maximized
                }
                (settings::OpenAs::Fullscreen, _)
                | (settings::OpenAs::AsClosed, settings::WindowState::Fullscreen) => {
                    screen::WindowMode::Fullscreen
                }
                (settings::OpenAs::Windowed, _)
                | (settings::OpenAs::AsClosed, settings::WindowState::Windowed) => {
                    screen::WindowMode::Windowed
                }
            });
            match mode {
                screen::WindowMode::Windowed => {}
                screen::WindowMode::Maximized => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
                }
                screen::WindowMode::Fullscreen => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
                }
            }
            self.phase = Phase::Main {
                splash: startup.elapsed(),
                frames: 0,
                // A window with no place kept is centred again once its frame is known.
                recentre_in: (mode == screen::WindowMode::Windowed && position.is_none())
                    .then_some(2),
            };
        }
        ui.ctx().request_repaint();
    }

    /// One main-window frame.
    fn main_frame(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        let adapter = self.selected();
        let measuring = self.smoke_test && !self.window.busy();
        if measuring {
            // The measured run turns the camera every frame, as an orbiting user does.
            self.window.spin();
        }
        self.window.show(ui, frame, adapter.as_ref());
        let Phase::Main {
            splash,
            frames,
            recentre_in,
        } = &mut self.phase
        else {
            return;
        };
        if let Some(wait) = recentre_in {
            // Once the decorations exist, centre the window by its actual outer size.
            if *wait > 0 {
                *wait -= 1;
                ui.ctx().request_repaint();
            } else {
                *recentre_in = None;
                let (outer, monitor) =
                    ui.input(|i| (i.viewport().outer_rect, i.viewport().monitor_size));
                if let (Some(outer), Some(monitor)) = (outer, monitor) {
                    let target = screen::centred(monitor, outer.size());
                    if (outer.min - target).length() > 1.0 {
                        ui.ctx()
                            .send_viewport_cmd(egui::ViewportCommand::OuterPosition(target));
                    }
                }
            }
        }
        if !measuring {
            return;
        }
        let failed = |m: &&messages::Message| m.level == messages::Level::Error;
        if self.window.workspace().notices().iter().any(|m| failed(&m)) {
            for notice in self.window.workspace().notices().iter().filter(failed) {
                eprintln!(
                    "error: {}: {}",
                    notice.text,
                    notice.detail.as_deref().unwrap_or_default()
                );
            }
            std::process::exit(4);
        }
        let workspace = self.window.workspace();
        let objects = workspace.plate().objects.len();
        let target = if objects == 0 {
            SMOKE_FRAMES
        } else {
            SMOKE_MEASURED_FRAMES
        };
        *frames += 1;
        self.timing.frame();
        // Report once; closing may take another frame.
        if *frames == target {
            let a = adapter.unwrap_or_default();
            let quote = |s: &str| format!("{s:?}");
            let (mean, max) = self.timing.summary();
            println!(
                "{{\"frames\":{},\"splash_ms\":{},\"backend\":{},\"adapter\":{},\
                 \"device_type\":{},\"driver\":{},\"transparent\":{},\"samples\":{},\
                 \"objects\":{},\"triangles\":{},\"frame_ms_mean\":{:.2},\"frame_ms_max\":{:.2}}}",
                frames,
                splash.as_millis(),
                quote(&a.backend),
                quote(&a.adapter),
                quote(&a.device_type),
                quote(&a.driver),
                a.transparent,
                self.window.samples().unwrap_or(0),
                objects,
                workspace.triangles(),
                mean,
                max
            );
        }
        if *frames >= target {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        } else {
            ui.ctx().request_repaint();
        }
    }
}

impl eframe::App for App {
    /// The settings are saved as the application closes, and the session's recovery copy is
    /// removed (TD-130).
    fn on_exit(&mut self) {
        self.window.persist(true);
        self.window.end_session();
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        match self.phase {
            Phase::Splash { .. } => self.splash_frame(ui),
            Phase::Main { .. } => self.main_frame(ui, frame),
        }
    }

    /// The background colour: the window is opaque, the splash's artwork too (TD-141).
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Rgba::from(theme::BACKGROUND.color()).to_array()
    }
}

/// Start the session log (TD-096): a file in the cache root's `logs` folder, or memory only when
/// the folder cannot be written (said on standard error, where a console has one). The session's
/// facts come first.
fn start_log() -> Option<karasu_project::logging::LogHandle> {
    use karasu_project::logging::{install, Destination, LogConfig, SESSIONS_KEPT};
    let destination = karasu_core::profiles::store::ProfilePaths::discover().map_or(
        Destination::Memory,
        |paths| Destination::Session {
            folder: paths.cache.join("logs"),
            keep: SESSIONS_KEPT,
        },
    );
    let handle = install(LogConfig::new(destination)).or_else(|error| {
        eprintln!("warning: the session log is kept in memory only: {error}");
        install(LogConfig::new(Destination::Memory))
    });
    log::info!(
        target: "session",
        "{} {} ({}) on {} {}, {} threads, log {}",
        karasu_core::APPLICATION_NAME,
        env!("CARGO_PKG_VERSION"),
        if cfg!(debug_assertions) { "debug" } else { "release" },
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::thread::available_parallelism().map_or(1, usize::from),
        handle
            .as_ref()
            .ok()
            .and_then(|h| h.path())
            .map_or_else(|| "in memory".to_owned(), |p| p.display().to_string())
    );
    handle.ok()
}

fn main() -> ExitCode {
    let options = match parse(std::env::args().skip(1)) {
        Ok(options) => options,
        Err(message) => {
            let help = message.starts_with(karasu_core::APPLICATION_NAME);
            if help {
                println!("{message}");
                return ExitCode::SUCCESS;
            }
            eprintln!("error: {message}");
            return ExitCode::from(2);
        }
    };
    let log = start_log();
    let locale = options
        .locale
        .clone()
        .unwrap_or_else(|| i18n::REFERENCE.to_owned());
    match i18n::Catalog::open(&locale, options.locale_dir.as_deref()) {
        // The interface runs on this thread.
        Ok(catalog) => i18n::install(catalog),
        Err(message) => {
            eprintln!("error: {message}");
            return ExitCode::from(2);
        }
    }
    if options.smoke_test {
        // A window that never renders must not hang native validation.
        std::thread::spawn(|| {
            std::thread::sleep(SMOKE_TIMEOUT);
            eprintln!("error: smoke test timed out without rendering {SMOKE_FRAMES} frames");
            std::process::exit(3);
        });
    }
    let chosen = Arc::new(Mutex::new(None));
    let native = eframe::NativeOptions {
        // The window starts as the splash: borderless, opaque, fixed size, centred.
        viewport: with_icon(egui::ViewportBuilder::default())
            .with_title(window_title())
            .with_decorations(false)
            .with_transparent(false)
            .with_resizable(false)
            .with_inner_size(splash::SIZE),
        centered: true,
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: gpu::configuration(options.backend, Arc::clone(&chosen)),
        ..eframe::NativeOptions::default()
    };
    let smoke_test = options.smoke_test;
    let mode = options.window;
    let files = options.files;
    let result = eframe::run_native(
        karasu_core::APPLICATION_NAME,
        native,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx);
            // The user's settings, but never a smoke test's: validation must not read or change
            // them.
            let mut window = window::MainWindow::new(files);
            window.set_launch(window::Launch {
                dev: options.dev,
                locale_fixed: options.locale.is_some(),
                locale,
                locale_dir: options.locale_dir,
                log,
            });
            let paths = (!smoke_test)
                .then(|| karasu_core::profiles::store::ProfilePaths::discover().ok())
                .flatten();
            // A smoke test closes when it is done, whatever it opened, and keeps no copies.
            match &paths {
                Some(paths) => window.keep_recovery_in(paths.data.join("recovery")),
                None => window.close_without_asking(),
            }
            let settings = paths.map(|paths| paths.config.join(settings::FILE_NAME));
            window.load_settings(&cc.egui_ctx, settings);
            Ok(Box::new(App {
                chosen,
                smoke_test,
                mode,
                phase: Phase::Splash {
                    startup: splash::Startup::new(),
                    artwork: None,
                    frames: 0,
                },
                // Imports start now and continue behind the splash.
                window,
                timing: window::Timing::default(),
            }))
        }),
    );
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: cannot start the desktop application: {error}");
            ExitCode::from(1)
        }
    }
}

/// The main window's title: the application's name and version (owner request, M6.5).
fn window_title() -> String {
    format!(
        "{} {}",
        karasu_core::APPLICATION_NAME,
        env!("CARGO_PKG_VERSION")
    )
}

/// The application's icon (the owner's artwork, `assets/ui/icon/`), at 256 pixels, for the
/// window's title bar and the taskbar. A malformed image leaves the system's default icon.
fn with_icon(builder: egui::ViewportBuilder) -> egui::ViewportBuilder {
    match icons::decode_png(include_bytes!("../../../assets/ui/icon/karasu-256.png")) {
        Ok(image) => builder.with_icon(egui::IconData {
            rgba: image.rgba,
            width: u32::try_from(image.size[0]).unwrap_or(0),
            height: u32::try_from(image.size[1]).unwrap_or(0),
        }),
        Err(error) => {
            log::warn!(target: "app", "the application icon cannot be read: {error}");
            builder
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_title_names_the_application_and_its_version() {
        assert_eq!(
            window_title(),
            format!("karasu-slicer {}", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn the_application_icon_is_a_square_rgba_image() {
        let image =
            crate::icons::decode_png(include_bytes!("../../../assets/ui/icon/karasu-256.png"))
                .unwrap();
        assert_eq!(image.size, [256, 256]);
    }

    fn parse_args(values: &[&str]) -> Result<Options, String> {
        parse(values.iter().map(|v| (*v).to_owned()))
    }

    #[test]
    fn options_parse_with_explicit_defaults_and_refusals() {
        assert_eq!(
            parse_args(&[]),
            Ok(Options {
                backend: gpu::BackendChoice::Auto,
                smoke_test: false,
                window: None,
                locale: None,
                dev: false,
                locale_dir: None,
                files: Vec::new(),
            })
        );
        assert_eq!(
            parse_args(&["--smoke-test", "--gpu-backend", "gl"]),
            Ok(Options {
                backend: gpu::BackendChoice::Gl,
                smoke_test: true,
                window: None,
                locale: None,
                dev: false,
                locale_dir: None,
                files: Vec::new(),
            })
        );
        assert!(parse_args(&["--gpu-backend", "webgpu"])
            .unwrap_err()
            .contains("--gpu-backend must be"));
        assert!(parse_args(&["--gpu-backend"]).is_err());
        assert_eq!(
            parse_args(&["model.stl", "--smoke-test", "printer.json"])
                .unwrap()
                .files,
            [PathBuf::from("model.stl"), PathBuf::from("printer.json")]
        );
        assert_eq!(
            parse_args(&["--window", "windowed"]).unwrap().window,
            Some(screen::WindowMode::Windowed)
        );
        let localised = parse_args(&["--locale", "pseudo", "--locale-dir", "catalogs"]).unwrap();
        assert_eq!(localised.locale.as_deref(), Some(i18n::PSEUDO));
        assert!(parse_args(&["--dev"]).unwrap().dev);
        assert_eq!(localised.locale_dir, Some(PathBuf::from("catalogs")));
        assert!(parse_args(&["--locale"]).is_err());
        assert!(parse_args(&["--window", "huge"])
            .unwrap_err()
            .contains("--window must be"));
        assert!(parse_args(&["--colour"])
            .unwrap_err()
            .starts_with("unknown option"));
        assert!(parse_args(&["--help"])
            .unwrap_err()
            .starts_with(karasu_core::APPLICATION_NAME));
    }
}
