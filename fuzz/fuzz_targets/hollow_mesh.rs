//! Bounded convex preparation admission and successful-shell invariant checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    karasu_mesh_fuzz::hollow_mesh::check_hollow_mesh(data);
});
