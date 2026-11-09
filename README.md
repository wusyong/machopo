# machopo

A WebAssembly browser from the future: wasmtime runs WebAssembly scripts,
V8 runs JavaScript, and the DOM lives on its own garbage collector
([elise](https://github.com/wusyong/elise)), which wasm's GC objects share.

The documentation is in Traditional Chinese. Start with
`docs/architecture.md` (how it is built), `docs/limitations.md` (what it
doesn't do yet), `docs/testing.md` (the kinds of tests and how to run them)
and `rfcs/README.md` (the features since the MVP, each an RFC);
`docs/README.md` says what each document holds.

## Getting the dependencies

machopo builds against projects checked out next to it, in
`../third_party/`, each at the commit `third_party.toml` pins:

```text
<some directory>/
  machopo/          this repository
  third_party/
    elise/          the DOM's GC (machopo's own project)
    parley/         our fork of parley (README.machopo.md)
    rusty_v8/       our fork of rusty_v8, its v8 submodule our fork of V8 (README.machopo.md)
    wasmtime/       our fork of wasmtime, with elise as a collector (README.machopo.md)
    wpt/            web-platform-tests, upstream, never changed
    speedometer/    Speedometer 3.1, upstream
    blink_perf/     Chromium's third_party/blink/perf_tests, upstream (a download)
```

```sh
python3 scripts/ci.py third-party   # clone what's missing, move the rest to the pinned commits
```

It leaves a checkout with work of its own alone, and says so. Every
`test.py` command runs it first. The forks are developed on their
`machopo` branches; a change to one pins its new commit in
`third_party.toml` in the same change, the fork pushed first.

V8 isn't built locally: the `v8` crate's build script downloads the V8
that our rusty_v8 fork's CI built, from the release that
`.cargo/config.toml`'s `RUSTY_V8_MIRROR_TAG` names (cached in
`~/.cargo/.rusty_v8`). Prebuilt for Linux x86-64, macOS arm64 and
Windows x86-64 (release only).

## Building and running

The toolchain is pinned (`rust-toolchain.toml`); rustup installs it. The
first build also downloads Skia's prebuilt library.

```sh
cargo run                                  # a window with the built-in demo page
cargo run -- https://example.com/          # a URL (http, https, file, about) or a local file
cargo run -- --headless out.png --size 800x600 --scale 2
                                           # no window: one frame saved as a PNG
cargo run -- --remote-debugging-port=9222 https://example.com
                                           # its tabs show up in Chrome's chrome://inspect (CDP)
cargo build -p shell -p driver && target/debug/machopodriver --port 4444
                                           # a WebDriver server
cargo run -- --help                        # every option and environment variable
```

The keyboard and mouse in the window: `crates/shell/README.md`.

## Testing

```sh
cargo test --workspace                     # the unit and integration tests
python3 scripts/test.py reftest            # machopo's own reftests (tests/reftests/)
python3 scripts/test.py wpt [dom/nodes/]   # web-platform-tests, through upstream's wptrunner
python3 scripts/test.py wdspec             # WPT's WebDriver tests
python3 scripts/test.py scenarios          # the scenarios (tests/scenarios/), with no platform window
python3 scripts/test.py automation         # the same in a real window
python3 scripts/sanity.py -h               # checks of the repository's rules
```

Each script's `-h` describes its commands (`ci.py`, `test.py`, `bench.py`,
`sanity.py`). The full suites run in CI: WPT and the rest every day, the GC's
`matrix` and `fuzz` every week.

## Working on the V8 fork

To change rusty_v8's C++ or V8 itself, fetch the fork's submodules
(`git -C ../third_party/rusty_v8 submodule update --init --recursive --depth 1`)
and build V8 from it:

```sh
python3 scripts/ci.py v8                   # debug and release V8, into ../third_party/rusty_v8_artifacts
export RUSTY_V8_ARCHIVE=$PWD/../third_party/rusty_v8_artifacts   # link it instead of the prebuilt
```

On macOS, bindgen needs libclang 21.1+ (`brew install llvm`); sccache, if
it's on the `PATH`, caches V8's objects. A change the prebuilt V8 holds
(the C++, `Cargo.toml`, the `v8` submodule) needs a new release of the
fork, then `RUSTY_V8_MIRROR_TAG` moved to it: see the fork's
`README.machopo.md`, "Releases". A change to its Rust alone is pinned by
`rev` in `third_party.toml`, past the release tag.

## License

machopo is licensed under either of MIT or Apache 2.0, at your option
(`LICENSE-MIT`, `LICENSE-APACHE`), as is the DOM's interface for wasm
scripts (`crates/dom_wasm/wit/machopo-dom.wit`). Files copied from other
projects keep their licenses: `crates/dom/fixed_names.txt` is from
web_atoms (`MIT OR Apache-2.0`); the fonts and pages used in tests are
listed in `resources/test_fonts/README.md` and `tests/sites/README.md`.
