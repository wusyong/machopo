//! The browser's side of a tab against a renderer that sends whatever it
//! likes: the checks are the integration tests'
//! (`tests/it/renderer_message_fuzz.rs`), which also replay
//! `regressions/`.

#![no_main]

use libfuzzer_sys::fuzz_target;

#[path = "../../tests/it/renderer_message_fuzz.rs"]
#[allow(dead_code)]
mod renderer_message_fuzz;

fuzz_target!(|data: &[u8]| {
    renderer_message_fuzz::run(data);
});
