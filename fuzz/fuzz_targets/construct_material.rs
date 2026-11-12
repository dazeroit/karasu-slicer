//! Exact constructive material composition fuzz entry point.
#![no_main]
libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    karasu_mesh_fuzz::construct_material::check_construct_material(data);
});
