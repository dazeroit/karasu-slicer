//! Puts the application icon into `karasu.exe` when building for Windows with MSVC (TD-141):
//! `assets/ui/icon/karasu.ico` becomes a resource file in `OUT_DIR`, passed to the linker of the
//! `karasu` executable. Other targets build nothing here. See `src/icon_resource.rs`.
#[path = "src/icon_resource.rs"]
#[allow(dead_code)] // `IconImage`'s fields are read by the tests in the application only.
mod icon_resource;

use std::{env, fs, path::PathBuf};

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("set by Cargo"));
    let icon = manifest.join("../../assets/ui/icon/karasu.ico");
    println!("cargo::rerun-if-changed={}", icon.display());
    println!("cargo::rerun-if-changed=src/icon_resource.rs");
    let windows = env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows");
    let msvc = env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|env| env == "msvc");
    if !(windows && msvc) {
        return;
    }
    let ico = fs::read(&icon).unwrap_or_else(|e| panic!("cannot read {}: {e}", icon.display()));
    let res = icon_resource::read_ico(&ico)
        .and_then(|images| icon_resource::resource_file(&images))
        .unwrap_or_else(|e| panic!("{}: {e}", icon.display()));
    let out = PathBuf::from(env::var("OUT_DIR").expect("set by Cargo")).join("karasu-icon.res");
    fs::write(&out, res).unwrap_or_else(|e| panic!("cannot write {}: {e}", out.display()));
    // The MSVC linker takes a resource file as an input.
    println!("cargo::rustc-link-arg-bin=karasu={}", out.display());
}
