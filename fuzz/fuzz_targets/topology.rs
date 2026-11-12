//! Bounded indexed source mesh topology fuzz entry point.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    karasu_mesh_fuzz::check_topology(data);
});
