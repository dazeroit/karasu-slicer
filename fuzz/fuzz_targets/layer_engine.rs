//! Bounded owned layer engine fuzz entry point.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    karasu_mesh_fuzz::layer_engine::check_layer_engine(data);
});
