//! Uninstrumented preparation check. This program never reports fuzzing coverage or executions.

use karasu_mesh_fuzz::{check_ascii, check_auto, check_binary, check_topology};
use std::{env, fs, path::PathBuf};

fn main() {
    let args: Vec<_> = env::args().collect();
    assert_eq!(args.len(), 3, "usage: replay TARGET SEED_DIRECTORY_OR_FILE");
    let input = PathBuf::from(&args[2]);
    let mut paths: Vec<PathBuf> = if input.is_file() {
        vec![input]
    } else {
        fs::read_dir(input)
            .expect("seed directory")
            .map(|entry| entry.expect("seed entry").path())
            .filter(|p| p.is_file())
            .collect()
    };
    paths.sort();
    assert!(!paths.is_empty(), "empty seed directory");
    let (mut accepted, mut rejected) = (0, 0);
    for path in &paths {
        let bytes = fs::read(path).expect("seed bytes");
        let name = path
            .file_name()
            .expect("seed name")
            .to_str()
            .expect("ASCII seed name");
        let result = match args[1].as_str() {
            "binary_stl" => check_binary(&bytes),
            "ascii_stl" => check_ascii(&bytes),
            "auto_stl" => check_auto(&bytes),
            "topology" => {
                check_topology(&bytes);
                true
            }
            "layer_schedule" => karasu_mesh_fuzz::schedule::check_layer_schedule(&bytes),
            "construct_material" => {
                karasu_mesh_fuzz::construct_material::check_construct_material(&bytes)
            }
            "strict_regions" => karasu_mesh_fuzz::strict_regions::check_strict_regions(&bytes),
            "layer_engine" => karasu_mesh_fuzz::layer_engine::check_layer_engine(&bytes),
            "profile_json" => karasu_mesh_fuzz::profile_json::check_profile_json(&bytes),
            "hollow_mesh" => karasu_mesh_fuzz::hollow_mesh::check_hollow_mesh(&bytes),
            "preparation_session" => {
                karasu_mesh_fuzz::preparation_session::check_preparation_session(&bytes)
            }
            "supports" => karasu_mesh_fuzz::supports::check_supports(&bytes),
            "drainage" => karasu_mesh_fuzz::drainage::check_drainage(&bytes),
            _ => panic!("unknown target"),
        };
        if name.starts_with("accept-") {
            assert!(result, "expected seed acceptance: {name}");
        }
        if name.starts_with("reject-") {
            assert!(!result, "expected seed rejection: {name}");
        }
        if result {
            accepted += 1;
        } else {
            rejected += 1;
        }
    }
    println!(
        "PREPARATION ONLY: {} seed files; accepted={accepted}; rejected={rejected}; target={}",
        paths.len(),
        args[1]
    );
}
