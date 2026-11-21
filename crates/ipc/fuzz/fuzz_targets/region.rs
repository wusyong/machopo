//! The regions against whatever a peer names and gives back: the checks
//! are the integration tests' (`tests/it/region_fuzz.rs`), which also
//! replay `regressions/region/`.

#![no_main]

use libfuzzer_sys::fuzz_target;

#[path = "../../tests/it/region_fuzz.rs"]
#[allow(dead_code)]
mod region_fuzz;

fuzz_target!(|data: &[u8]| region_fuzz::run(data));
