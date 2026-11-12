//! What the application asks of the desktop: showing a folder in the system's file manager.

/// Show `folder` in the system's file manager (Explorer, the macOS Finder, or what
/// `xdg-open` chooses on Linux).
///
/// # Errors
/// The file manager cannot be started.
pub fn open_folder(folder: &std::path::Path) -> std::io::Result<()> {
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program)
        .arg(folder)
        .spawn()
        .map(|_| ())
}
