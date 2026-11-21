//! A whole endpoint against a peer that sends whatever it likes: the
//! checks are the integration tests' (`tests/it/endpoint_fuzz.rs`), which
//! also replay `regressions/endpoint/`.

#![no_main]

use libfuzzer_sys::fuzz_target;

#[path = "../../tests/it/endpoint_fuzz.rs"]
#[allow(dead_code)]
mod endpoint_fuzz;

fuzz_target!(|data: &[u8]| endpoint_fuzz::run(data));
