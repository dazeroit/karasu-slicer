//! Bounded preparation session ownership and publication invariants.
#![no_main]
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    karasu_mesh_fuzz::preparation_session::check_preparation_session(data);
});
