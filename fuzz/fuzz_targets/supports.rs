//! Bounded parametric support generation and prepared-material section checks.
#![no_main]
libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    karasu_mesh_fuzz::supports::check_supports(data);
});
