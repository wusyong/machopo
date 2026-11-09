//! Images in a whole browser: SVG images (`image/svg+xml`), their natural
//! sizes and the boxes they make, as Chrome 152 gives them. Each page
//! reports what it read through `testRunner`
//! (`headless::run_testharness`).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use net::test_server::Route;
use shell::headless;

use crate::automation_pipe::{Browser, server};

/// A page with `html` in this test's scratch directory; its `file:` URL.
fn page(name: &str, html: &str) -> String {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shell-images");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, html).unwrap();
    net::file_url(&path).expect("an absolute path")
}

/// What the page at `url` reports.
fn report(url: &str) -> String {
    headless::run_testharness(url, Duration::from_secs(30), Default::default())
        .expect("the page reports")
}

/// An SVG image's natural size is its root's `width` and `height`
/// attributes, where they are lengths (not percentages, not its CSS),
/// one missing through the other and the `viewBox`'s ratio, else 300×150
/// (with only a ratio, 300 wide and the height through it); with only a
/// ratio it fills its container's width; its ratio is its
/// `width` and `height`'s before its `viewBox`'s. A root that isn't an
/// SVG `<svg>` is a broken image. `<picture>` takes an `image/svg+xml`
/// source.
#[test]
fn svg_images_natural_sizes() {
    let url = page(
        "svg-sizes.html",
        r##"<!DOCTYPE html><body style="margin:0">
<div id=box style="width:400px"></div>
<picture id=pic><source type="image/svg+xml" srcset="data:image/svg+xml,%3Csvg%20xmlns='http://www.w3.org/2000/svg'%20width='7'%20height='5'/%3E"><img id=pimg src="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=="></picture>
<script>
const NS = 'xmlns="http://www.w3.org/2000/svg"';
const R = '<rect width="100%" height="100%" fill="lime"/>';
const cases = {
  wh: `<svg ${NS} width="20" height="10">${R}</svg>`,
  vb: `<svg ${NS} viewBox="0 0 100 25">${R}</svg>`,
  w_vb: `<svg ${NS} width="40" viewBox="0 0 100 50">${R}</svg>`,
  h_vb: `<svg ${NS} height="40" viewBox="0 0 100 50">${R}</svg>`,
  w: `<svg ${NS} width="40">${R}</svg>`,
  pct: `<svg ${NS} width="50%" height="10">${R}</svg>`,
  none: `<svg ${NS}>${R}</svg>`,
  em: `<svg ${NS} width="2em" height="1em">${R}</svg>`,
  cssw: `<svg ${NS} style="width:30px;height:15px">${R}</svg>`,
  notsvg: `<html xmlns="http://www.w3.org/1999/xhtml"><rect/></html>`,
  nons: `<svg width="20" height="10"/>`,
  // Its ratio from width and height, not from the viewBox: as wide as 40px
  // makes it 20 high.
  whvb_40: `<svg ${NS} width="20" height="10" viewBox="0 0 1 1">${R}</svg>`,
};
const out = [];
const box = document.getElementById("box");
let pending = Object.keys(cases).length + 1;
const pimg = document.getElementById("pimg");
const report = () => { out.push(`picture: ${pimg.currentSrc.startsWith("data:image/svg+xml")} ${pimg.naturalWidth}x${pimg.naturalHeight}`); done(); };
if (pimg.complete) report(); else pimg.onload = report;
for (const [name, svg] of Object.entries(cases)) {
  const img = new Image();
  img.onload = () => {
    if (name.endsWith("_40")) img.style.width = "40px";
    box.append(img);
    const r = img.getBoundingClientRect();
    out.push(`${name}: natural ${img.naturalWidth}x${img.naturalHeight} box ${r.width}x${r.height}`); done();
  };
  img.onerror = () => { out.push(`${name}: error`); done(); };
  img.src = "data:image/svg+xml," + encodeURIComponent(svg);
}
function done() {
  if (--pending) return;
  testRunner.reportHarnessResults(out.sort().join("\n"));
}
</script>"##,
    );
    let expected = [
        "cssw: natural 300x150 box 300x150",
        "em: natural 32x16 box 32x16",
        "h_vb: natural 80x40 box 80x40",
        "none: natural 300x150 box 300x150",
        "nons: error",
        "notsvg: error",
        "pct: natural 300x10 box 300x10",
        "picture: true 7x5",
        "vb: natural 300x75 box 400x100",
        "w: natural 40x150 box 40x150",
        "w_vb: natural 40x20 box 40x20",
        "wh: natural 20x10 box 20x10",
        "whvb_40: natural 20x10 box 40x20",
    ];
    assert_eq!(report(&url), expected.join("\n"));
}

/// A frame's `<picture>` selects again as the frame is resized (HTML's
/// "react to environment changes"): a source that loads fires `load`; one
/// that fails fires no `error`, as in Chrome (the first selection's
/// failure does).
#[test]
fn an_image_selected_again_that_fails_fires_no_error() {
    let url = page(
        "reselect.html",
        r##"<!DOCTYPE html><body style="margin:0">
<iframe id=f style="width:50px;height:50px;border:0"></iframe>
<script>
const green = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
const narrow = src => `<picture><source media="(max-width:100px)" srcset="${src}">`;
f.srcdoc = `${narrow("data:image/png;base64,AAAA")}<img id=heals src="${green}"></picture>
${narrow(green)}<img id=breaks src="data:image/png;base64,AAAB"></picture>`;
const out = [];
f.onload = () => {
  const doc = f.contentDocument;
  const images = ["heals", "breaks"].map(id => doc.getElementById(id));
  const settled = img => new Promise(resolve => {
    if (img.complete) return resolve();
    img.addEventListener("load", resolve, { once: true });
    img.addEventListener("error", resolve, { once: true });
  });
  Promise.all(images.map(settled)).then(() => {
    for (const img of images) {
      out.push(`${img.id}: ${img.naturalWidth}`);
      img.onload = () => out.push(`${img.id}: load`);
      img.onerror = () => out.push(`${img.id}: error`);
    }
    f.style.width = "200px";
    setTimeout(() => testRunner.reportHarnessResults(out.join(" / ")), 300);
  });
};
</script>"##,
    );
    assert_eq!(
        report(&url),
        "heals: 0 / breaks: 1 / heals: load",
        "no error for `breaks`"
    );
}

/// Malformed XML is drawn as xml5ever recovers it, where Chrome breaks the
/// image at the first error; only a root that isn't an SVG `<svg>` breaks
/// it. A file of another MIME type isn't taken for SVG, whatever is in it.
#[test]
fn malformed_svg_images_still_load() {
    let url = page(
        "svg-malformed.html",
        r##"<!DOCTYPE html><script>
const NS = 'xmlns="http://www.w3.org/2000/svg"';
const cases = {
  mismatch: ["image/svg+xml", `<svg ${NS} width="20" height="10"><g></svg>`],
  unclosed: ["image/svg+xml", `<svg ${NS} width="20" height="10"><g>`],
  plain: ["text/plain", `<svg ${NS} width="20" height="10"/>`],
};
const out = [];
let pending = Object.keys(cases).length;
for (const [name, [type, svg]] of Object.entries(cases)) {
  const img = new Image();
  const done = what => {
    out.push(`${name}: ${what}`);
    if (!--pending) testRunner.reportHarnessResults(out.sort().join(" "));
  };
  img.onload = () => done(`${img.naturalWidth}x${img.naturalHeight}`);
  img.onerror = () => done("error");
  img.src = `data:${type},` + encodeURIComponent(svg);
}
</script>"##,
    );
    assert_eq!(report(&url), "mismatch: 20x10 plain: error unclosed: 20x10");
}

/// An `<img>` nothing holds (a `new Image()` in a function's variable)
/// isn't collected while its image loads: its `load` fires when it comes,
/// after a full GC (`gc()`, V8's `--expose-gc`) in between, and so does
/// its document's, which it delays (collected, it would never come).
/// Loaded, it is let go: the next GC collects it.
#[test]
fn an_image_nothing_holds_still_loads() {
    let server = server(&[
        (
            "/",
            Route::ok(
                "text/html",
                r#"<!DOCTYPE html><body><script>
const report = id => {
  const p = document.createElement("p");
  p.id = id;
  document.body.append(p);
};
let held;
// Out here, so that the timers' closures don't hold `load`'s `img`.
const afterLoad = () => {
  gc();
  setTimeout(() => report(held.deref() ? "kept" : "collected"));
};
function load() {
  const img = new Image();
  held = new WeakRef(img);
  img.onload = () => {
    report(`loaded-${img.naturalWidth}x${img.naturalHeight}`);
    // A task later, its `load` done, nothing holds it.
    setTimeout(afterLoad);
  };
  img.onerror = () => report("failed");
  img.src = "/image.svg";
}
load();
setTimeout(() => gc(), 50);
</script>"#,
            ),
        ),
        (
            "/image.svg",
            Route::ok(
                "image/svg+xml",
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="3"/>"#,
            )
            .with_delay(Duration::from_millis(500)),
        ),
    ]);
    let flags = std::ffi::OsStr::new("--expose-gc");
    let mut browser = Browser::start_with_env(&[("MACHOPO_V8_FLAGS", flags)]);
    let session = browser.attach();
    browser.navigate(&session, &server.url("/"));
    let deadline = Instant::now() + Duration::from_secs(10);
    let found = loop {
        let found = ["#collected", "#kept", "#failed"]
            .into_iter()
            .find(|selector| !browser.query(selector)["element"].is_null());
        if found.is_some() || Instant::now() > deadline {
            break found;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(!browser.query("#loaded-2x3")["element"].is_null());
    assert_eq!(found, Some("#collected"));
    assert!(browser.end().success());
}

/// A document that goes while one of its images loads lets its `<img>`
/// go: machopo ends cleanly after a navigation away from it (its realm, which
/// outlives the frame's view, would otherwise hold the root past the DOM).
#[test]
fn a_document_gone_lets_its_loading_images_go() {
    let server = server(&[
        (
            "/",
            Route::ok(
                "text/html",
                r#"<!DOCTYPE html><body><script>
new Image().src = "/slow.svg";
const p = document.createElement("p");
p.id = "asked";
document.body.append(p);
</script>"#,
            ),
        ),
        (
            "/slow.svg",
            Route::ok(
                "image/svg+xml",
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="3"/>"#,
            )
            .with_delay(Duration::from_secs(5)),
        ),
        ("/next", Route::ok("text/html", "<!DOCTYPE html>next")),
    ]);
    let mut browser = Browser::start_with_env(&[]);
    let session = browser.attach();
    // Its `load` waits for the image: on once it has asked for it.
    browser.start_navigation(&session, &server.url("/"));
    let deadline = Instant::now() + Duration::from_secs(10);
    while browser.query("#asked")["element"].is_null() {
        assert!(Instant::now() < deadline, "the page asks for its image");
        std::thread::sleep(Duration::from_millis(50));
    }
    browser.navigate(&session, &server.url("/next"));
    assert!(browser.end().success());
}
