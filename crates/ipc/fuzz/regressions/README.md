# The fuzzers' finds

Each `.bin` is an input a fuzzer failed on, kept with the fix: `cargo test -p ipc` replays them. Name one for what it found.

- Here: the framing's (`tests/it/frame_fuzz.rs`).
- `region/`: the regions' (`tests/it/region_fuzz.rs`).
- `pipe/`: the pipes' (`tests/it/pipe_fuzz.rs`).
