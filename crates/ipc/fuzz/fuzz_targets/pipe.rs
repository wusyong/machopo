//! A data pipe across a channel, its reader keeping to the protocol or
//! not: the checks are the integration tests' (`tests/it/pipe_fuzz.rs`),
//! which also replay `regressions/pipe/`.

#![no_main]

use libfuzzer_sys::fuzz_target;

#[path = "../../tests/it/pipe_fuzz.rs"]
#[allow(dead_code)]
mod pipe_fuzz;

fuzz_target!(|data: &[u8]| pipe_fuzz::run(data));
