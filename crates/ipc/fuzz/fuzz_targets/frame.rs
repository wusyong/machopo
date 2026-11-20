//! The framing against whatever a peer sends: the checks are the
//! integration tests' (`tests/it/frame_fuzz.rs`), which also replay
//! `regressions/`.

#![no_main]

use libfuzzer_sys::fuzz_target;

#[path = "../../tests/it/frame_fuzz.rs"]
#[allow(dead_code)]
mod frame_fuzz;

fuzz_target!(|data: &[u8]| frame_fuzz::run(data));
