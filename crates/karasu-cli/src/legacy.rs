//! Compatibility executable for scripts invoking the former CLI name.

#[path = "main.rs"]
mod application;

fn main() -> std::process::ExitCode {
    application::main()
}
