//! The installed logger (TD-096), in a process of its own: records through the `log` facade, the
//! ring, and a panic recorded with its backtrace before the program goes on.
use karasu_project::logging::{install, Destination, LogConfig, LogError};
use log::LevelFilter;
use std::fs;

#[test]
fn the_installed_logger_records_the_facade_and_panics() {
    let folder =
        std::env::temp_dir().join(format!("karasu-logging-install-{}", std::process::id()));
    let _ = fs::remove_dir_all(&folder);
    let config = LogConfig {
        ours: LevelFilter::Info,
        ..LogConfig::new(Destination::Session {
            folder: folder.clone(),
            keep: 10,
        })
    };
    let handle = install(config.clone()).unwrap();
    assert!(matches!(install(config), Err(LogError::AlreadyInstalled)));
    log::info!(target: "slice", "sliced 3 layers");
    log::debug!(target: "slice", "not kept");
    log::info!(target: "wgpu_core::device", "not kept either");
    log::warn!(target: "wgpu_core::device", "a validation warning");
    let panicked = std::panic::catch_unwind(|| panic!("a test panic"));
    assert!(panicked.is_err());
    let messages: Vec<String> = handle
        .ring()
        .since(0)
        .into_iter()
        .map(|r| format!("{} {}", r.target, r.message))
        .collect();
    assert_eq!(messages[0], "slice sliced 3 layers");
    assert_eq!(messages[1], "wgpu_core::device a validation warning");
    assert!(messages[2].starts_with("panic "), "{messages:?}");
    assert!(messages[2].contains("a test panic"));
    assert!(messages[2].contains("backtrace:"));
    // The panic flushed the file, with everything before it.
    let text = fs::read_to_string(handle.path().unwrap()).unwrap();
    assert!(text.contains("INFO  slice: sliced 3 layers"), "{text}");
    assert!(text.contains("WARN  wgpu_core::device: a validation warning"));
    assert!(text.contains("ERROR panic: "));
    assert!(!text.contains("not kept"));
    let _ = fs::remove_dir_all(&folder);
}
