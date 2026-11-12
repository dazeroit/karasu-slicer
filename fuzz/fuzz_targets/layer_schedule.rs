//! Bounded exact uniform schedule fuzz entry point.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    karasu_mesh_fuzz::schedule::check_layer_schedule(data);
});
