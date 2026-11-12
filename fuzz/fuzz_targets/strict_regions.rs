//! Dedicated nested/contact region pipeline fuzz entry point.
#![no_main]
libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    karasu_mesh_fuzz::strict_regions::check_strict_regions(data);
});
