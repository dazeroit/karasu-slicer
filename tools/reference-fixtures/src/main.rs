//! Deterministic binary STL generator for the independently specified M1 fixture dataset.

mod dataset;
mod geometry;
mod sha256;
mod stl;

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

fn usage() -> &'static str {
    "Usage: reference-fixtures --output-dir <PATH>\n\
     \n\
     Generate the deterministic M1 Stage C.0 binary STL dataset and manifest.\n\
     The output directory is explicit and only known dataset files are written."
}

fn parse_output_dir() -> Result<PathBuf, String> {
    let mut args = env::args_os().skip(1);
    let Some(first) = args.next() else {
        return Err(format!("missing --output-dir\n\n{}", usage()));
    };

    if first == "--help" || first == "-h" {
        println!("{}", usage());
        return Ok(PathBuf::new());
    }

    if first != "--output-dir" {
        return Err(format!(
            "unsupported argument: {}\n\n{}",
            first.to_string_lossy(),
            usage()
        ));
    }

    let output_dir = args
        .next()
        .ok_or_else(|| format!("missing path after --output-dir\n\n{}", usage()))?;
    if let Some(extra) = args.next() {
        return Err(format!(
            "unsupported argument: {}\n\n{}",
            extra.to_string_lossy(),
            usage()
        ));
    }

    Ok(PathBuf::from(output_dir))
}

fn main() -> ExitCode {
    let output_dir = match parse_output_dir() {
        Ok(path) if path.as_os_str().is_empty() => return ExitCode::SUCCESS,
        Ok(path) => path,
        Err(message) => {
            eprintln!("error: {message}");
            return ExitCode::from(2);
        }
    };

    match dataset::write_dataset(&output_dir) {
        Ok(summary) => {
            println!(
                "generated {} STL files and {} in {}",
                summary.stl_count,
                summary.manifest_name,
                output_dir.display()
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!(
                "error: failed to generate reference fixtures in {}: {error}",
                output_dir.display()
            );
            ExitCode::FAILURE
        }
    }
}
