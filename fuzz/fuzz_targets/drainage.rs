//! Bounded drainage scene and exact material-difference checks.
#![no_main]
libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    karasu_mesh_fuzz::drainage::check_drainage(data);
});
