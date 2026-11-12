//! Bounded untrusted JSON profile and exact round-trip fuzz entry point.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    karasu_mesh_fuzz::profile_json::check_profile_json(data);
});
