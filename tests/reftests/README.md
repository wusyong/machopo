# Reftests

Our own reftests, in WPT's format, run by upstream's wptrunner as WPT's are: at `/_machopo/` beside WPT (as Servo's and Firefox's own tests are at `/_mozilla/`), through machopodriver and machopo. Run them with:

```sh
python3 scripts/test.py reftest           # all of them
python3 scripts/test.py reftest scroll/   # those under scroll/
```

WPT's reftests run with the rest of WPT (`python3 scripts/test.py wpt`, see `tests/wpt/README.md`); the two can't run at the same time (wptserve's ports are fixed). Those in `wasm/` run in `cargo test` instead (below).

- **A test** is any file with a `<link rel=match href=…>` or `<link rel=mismatch href=…>`, as wptrunner's manifest has them. It passes if it renders the same as its `match` references and differently from its `mismatch` ones; a reference with references of its own holds only if it, in turn, passes against them (chains). `href` is a path relative to the test. Name references `*-ref.html`. A test lives in a directory (WPT's manifest takes the root's own files, and those under a `resources/` or `support/` directory, for support files).
- **Fuzziness**: `<meta name=fuzzy content="maxDifference=0-2;totalPixels=0-50">`, as in WPT.
- **Rendering**: each page from wptserve (`http://web-platform.test:8000/_machopo/<path>`), at 800×600 CSS px, with the test fonts only (`machopo --test-fonts`): Ahem, and Lato for every generic family. Text layout tests use `font: 20px/1 Ahem`, so each glyph is a 20×20 square. The screenshot is taken once the page has loaded, two animation frames have been drawn and, for a test whose root element has the `reftest-wait` class, script has removed it (wptrunner's `test-wait.js`). Animation frames and timers run in real time.
- **Another scale**: `subsuites.json` lists wptrunner's subsuites: the tests of `hidpi/` run with `machopo --scale 2` or `--scale 1.5` (each with its references), to check that edges snap to device pixels, and only so (`test.py reftest` leaves `hidpi/` out of the default subsuite).
- **A fragment**: a test that scrolls to a `#target` sets `location.hash` from its `load` event and takes off `reftest-wait` two frames later (`scroll/`), since wptrunner loads a test at its own URL.
- **Files**: `.html`, `.htm`, `.xht` and `.xhtml` (XHTML goes through the HTML parser, with CDATA section markers dropped first).
- **Known failures** go in `metadata/`, wptrunner's `.ini` at the test's path (`flex/f1-row.html` → `metadata/flex/f1-row.html.ini`; none for a test that passes), with the reason as `#` comments; `disabled:` with a reason skips a test. `MACHOPO_UPDATE_EXPECTATIONS=1 python3 scripts/test.py reftest` writes the results as the expectations.
- **Results**: each failing test's screenshots, `-actual.png`, `-expected.png` and `-diff.png` (the differing pixels in red), go under `target/reftest-results/screenshots/`, with wptrunner's logs.
