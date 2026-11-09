//! Pages loaded over HTTP from a local test server, rendered headless:
//! the stylesheet rules that only HTTP can exercise (status, MIME type),
//! and that stylesheets block rendering until they load, and the parser's
//! scripts.

use std::collections::HashMap;
use std::time::Duration;

use gfx::Size;
use net::test_server::{Route, TestServer};
use shell::headless::{self, Frame};

fn server(routes: &[(&str, Route)]) -> TestServer {
    TestServer::start(
        routes
            .iter()
            .map(|(path, route)| (path.to_string(), route.clone()))
            .collect::<HashMap<_, _>>(),
    )
    .expect("test server starts")
}

fn render(url: &str) -> Frame {
    headless::render(url, Size::new(200, 200), 1.0, true).expect("the page renders")
}

/// Premultiplied RGBA at `(x, y)`.
fn pixel(frame: &Frame, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * frame.size.width + x) * 4) as usize;
    frame.pixels[i..i + 4].try_into().unwrap()
}

const GREEN: [u8; 4] = [0, 128, 0, 255];

#[test]
fn stylesheets_need_an_ok_status_and_text_css() {
    let page = r#"<!DOCTYPE html>
        <style>html, body { margin: 0 } div { width: 20px; height: 20px; background-color: green }</style>
        <link rel="stylesheet" href="/ok.css">
        <link rel="stylesheet" href="/not-found.css">
        <link rel="stylesheet" href="/plain.css">
        <div id="a"></div><div id="b"></div><div id="c"></div>"#;
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        (
            "/ok.css",
            Route::ok("text/css; charset=utf-8", "#a { background-color: green }"),
        ),
        // Neither of these applies, or the squares would be red.
        (
            "/not-found.css",
            Route::new(404, "#b { background-color: red }").with_header("Content-Type", "text/css"),
        ),
        (
            "/plain.css",
            Route::ok("text/plain", "#c { background-color: red }"),
        ),
    ]);
    let frame = render(&server.url("/"));
    for y in [10, 30, 50] {
        assert_eq!(pixel(&frame, 10, y), GREEN, "square at y = {y}");
    }
}

/// The subresources that failed, or whose content was of no use, are told
/// to the browser with the document's load (`DidFinishLoad`): a sheet of
/// a 404 or of another type, a `<link>` whose `href` doesn't parse, an
/// image that doesn't decode, a script and a module of a 404 (one script
/// inserts too). Those that loaded aren't.
#[test]
fn the_subresources_that_failed_are_told_with_the_load() {
    let png = include_bytes!("../../../../tests/reftests/images/resources/green-40x30.png");
    let page = r#"<!DOCTYPE html>
        <link rel="stylesheet" href="/ok.css">
        <link rel="stylesheet" href="/missing.css">
        <link rel="stylesheet" href="/plain.css">
        <link rel="stylesheet" href="http://[">
        <img src="/ok.png"><img src="/broken.png">
        <script src="/missing.js"></script>
        <script type="module" src="/missing.mjs"></script>
        <script>
        const inserted = document.createElement('script');
        inserted.src = '/inserted.js';
        document.head.append(inserted);
        </script>"#;
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        ("/ok.css", Route::ok("text/css", "")),
        (
            "/missing.css",
            Route::new(404, "").with_header("Content-Type", "text/css"),
        ),
        ("/plain.css", Route::ok("text/plain", "")),
        ("/ok.png", Route::ok("image/png", png.to_vec())),
        ("/broken.png", Route::ok("image/png", "not a png")),
    ]);
    let base = server.url("/");
    let frame = render(&base);
    let mut failed: Vec<String> = frame
        .failed_subresources
        .iter()
        .map(|url| url.trim_start_matches(base.as_str()).to_owned())
        .collect();
    failed.sort();
    assert_eq!(
        failed,
        [
            "broken.png",
            "http://[",
            "inserted.js",
            "missing.css",
            "missing.js",
            "missing.mjs",
            "plain.css",
        ]
    );
}

/// An iframe's initial `about:blank` is a document as any other: a
/// stylesheet `<link>` script inserts into it loads and applies.
#[test]
fn a_link_in_an_iframes_initial_about_blank_loads() {
    let page = r#"<!DOCTYPE html>
<iframe></iframe>
<script>
onload = () => {
  const child = document.querySelector('iframe').contentDocument;
  const p = child.createElement('p');
  child.body.append(p);
  const link = child.createElement('link');
  link.rel = 'stylesheet';
  link.href = new URL('/green.css', location.href).href;
  link.onload = () => testRunner.reportHarnessResults(
      `${child.URL} ${child.defaultView.getComputedStyle(p).color}`);
  link.onerror = () => testRunner.reportHarnessResults('error');
  child.head.append(link);
};
</script>"#;
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        (
            "/green.css",
            Route::ok("text/css", "p { color: rgb(0, 128, 0) }"),
        ),
    ]);
    let result = headless::run_testharness(
        &server.url("/"),
        Duration::from_secs(30),
        Default::default(),
    )
    .expect("the page reports");
    assert_eq!(result, "about:blank rgb(0, 128, 0)");
}

/// A `srcdoc` document commits in a task of its own, as a fetched one
/// would once its response came: the parser goes on first, so an
/// `onload` set after the iframe is parsed hears its `load`, which comes
/// before the window's.
#[test]
fn a_srcdoc_document_commits_after_the_parser_goes_on() {
    let page = r#"<!DOCTYPE html>
<iframe srcdoc="<p id=inside>inside</p>"></iframe>
<script>
const log = [];
const iframe = document.querySelector('iframe');
log.push(iframe.contentDocument.URL);
iframe.onload = () => log.push(
    `load ${iframe.contentDocument.URL} ${iframe.contentDocument.getElementById('inside') !== null}`);
onload = () => testRunner.reportHarnessResults(log.join(' / '));
</script>"#;
    let server = server(&[("/", Route::ok("text/html", page))]);
    let result = headless::run_testharness(
        &server.url("/"),
        Duration::from_secs(30),
        Default::default(),
    )
    .expect("the page reports");
    assert_eq!(result, "about:blank / load about:srcdoc true");
}

/// HTML runs a script only from an ok response, and never one served as
/// an image, audio, video or CSV; any other type runs (`text/plain` too).
#[test]
fn scripts_run_only_from_ok_responses_of_script_types() {
    let page = r#"<!DOCTYPE html>
        <style>html, body { margin: 0 } div { width: 20px; height: 20px; background-color: green }</style>
        <div id="a"></div><div id="b"></div><div id="c" style="background-color: red"></div>
        <script src="/not-found.js"></script>
        <script src="/image.js"></script>
        <script src="/plain.js"></script>"#;
    let red = |id: &str| {
        format!("document.getElementById('{id}').setAttribute('style', 'background-color: red')")
    };
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        (
            "/not-found.js",
            Route::new(404, red("a")).with_header("Content-Type", "text/javascript"),
        ),
        ("/image.js", Route::ok("image/png", red("b"))),
        (
            "/plain.js",
            Route::ok(
                "text/plain",
                "document.getElementById('c').setAttribute('style', 'background-color: green')",
            ),
        ),
    ]);
    let frame = render(&server.url("/"));
    for y in [10, 30, 50] {
        assert_eq!(pixel(&frame, 10, y), GREEN, "square at y = {y}");
    }
}

/// Loads start whenever the parser stops, not once the document is done: a
/// stylesheet the parser has seen is asked for before the script after it
/// runs, even if that script removes it.
#[test]
fn subresources_start_loading_when_the_parser_stops() {
    let page = r#"<!DOCTYPE html>
        <link rel="stylesheet" href="/seen.css">
        <script>document.querySelector("head").removeChild(document.querySelector("link"));</script>"#;
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        ("/seen.css", Route::ok("text/css", "")),
    ]);
    render(&server.url("/"));
    let asked: Vec<String> = server.requests().into_iter().map(|r| r.target).collect();
    assert!(asked.iter().any(|t| t == "/seen.css"), "{asked:?}");
}

#[test]
fn quirks_mode_takes_any_mime_type() {
    // No doctype: quirks mode.
    let page = r#"<style>html, body { margin: 0 } div { width: 20px; height: 20px; background-color: red }</style>
        <link rel="stylesheet" href="/plain.css"><div></div>"#;
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        (
            "/plain.css",
            Route::ok("text/plain", "div { background-color: green }"),
        ),
    ]);
    assert_eq!(pixel(&render(&server.url("/")), 10, 10), GREEN);
}

#[test]
fn a_stylesheet_blocks_rendering_until_it_loads() {
    let page = r#"<!DOCTYPE html>
        <link rel="stylesheet" href="/slow.css">
        <style>html, body { margin: 0 }</style>
        <div style="width: 20px; height: 20px"></div>"#;
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        (
            "/slow.css",
            Route::ok("text/css", "div { background-color: green }")
                .with_delay(Duration::from_millis(300)),
        ),
    ]);
    let frame = render(&server.url("/"));
    assert_eq!(pixel(&frame, 10, 10), GREEN);
    // Nothing was drawn before the sheet arrived.
    assert_eq!(frame.frames_drawn, 1);
}

/// The parser's scripts wait for the stylesheets the parser's `<link>`s
/// load, of media that match (HTML's "a style sheet that is blocking
/// scripts"): an inline one (the current script as it runs) sees `a.css`,
/// an external one that loaded first `c.css`; a deferred one and a module
/// one, `b.css`, after the last of them. An `async` one doesn't wait; nor
/// does any for a stylesheet script asked for (or `innerHTML` made), one
/// of other media, one removed or no longer a stylesheet while it loads,
/// or one that failed.
#[test]
fn stylesheets_block_the_parsers_scripts() {
    let page = r#"<!DOCTYPE html>
<style>#t { width: 20px; height: 20px }</style>
<div id="t"></div>
<script>
window.log = [];
window.w = () => getComputedStyle(document.getElementById('t')).width;
window.h = () => getComputedStyle(document.getElementById('t')).height;
const late = document.createElement('link');
late.rel = 'stylesheet';
late.href = '/late.css';
document.head.append(late);
document.body.insertAdjacentHTML('beforeend', '<link rel="stylesheet" href="/fragment.css">');
</script>
<script async src="/async.js"></script>
<link rel="stylesheet" href="/a.css">
<link rel="stylesheet" href="/narrow.css" media="(max-width: 1px)">
<link rel="stylesheet" href="/gone.css" id="gone">
<link rel="stylesheet" href="/unrel.css" id="unrel">
<script async src="/remove.js"></script>
<script id="inline">
log.push(`inline ${w()} ${h()} ${document.currentScript.id} ${
    performance.now() < 2500 ? 'early' : 'late'}`);
</script>
<link rel="stylesheet" href="/c.css">
<script src="/external.js"></script>
<script defer src="/defer.js"></script>
<script type="module">log.push(`module ${w()} ${h()}`);</script>
<link rel="stylesheet" href="/missing.css">
<script>log.push('after a failed one');</script>
<script>onload = () => testRunner.reportHarnessResults(log.join(' / '));</script>
<link rel="stylesheet" href="/b.css">"#;
    let slow = |css: &str| Route::ok("text/css", css).with_delay(Duration::from_secs(4));
    let script = |js: &str| Route::ok("text/javascript", js);
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        (
            "/a.css",
            Route::ok("text/css", "#t { width: 40px }").with_delay(Duration::from_secs(1)),
        ),
        ("/late.css", slow("#t { height: 99px }")),
        ("/fragment.css", slow("#t { width: 3px }")),
        ("/unrel.css", slow("#t { width: 4px }")),
        (
            "/b.css",
            Route::ok("text/css", "#t { height: 30px }").with_delay(Duration::from_secs(1)),
        ),
        (
            "/c.css",
            Route::ok("text/css", "#t { width: 50px }").with_delay(Duration::from_secs(1)),
        ),
        ("/narrow.css", slow("#t { width: 1px }")),
        ("/gone.css", slow("#t { width: 2px }")),
        ("/async.js", script("log.push(`async ${w()}`);")),
        (
            "/remove.js",
            script(
                "document.getElementById('gone').remove(); document.getElementById('unrel').rel = 'icon';",
            ),
        ),
        ("/external.js", script("log.push(`external ${w()}`);")),
        ("/defer.js", script("log.push(`defer ${w()} ${h()}`);")),
    ]);
    let result = headless::run_testharness(
        &server.url("/"),
        Duration::from_secs(30),
        Default::default(),
    )
    .expect("the page reports");
    assert_eq!(
        result,
        "async 20px / inline 40px 20px inline early / external 50px / after a failed one / \
         defer 50px 30px / module 50px 30px"
    );
}

/// A stylesheet that blocks the parser's scripts stops blocking them once
/// script removes its `<link>`, though its response never comes: the
/// parser's next script runs then, not when the sheet would have loaded.
#[test]
fn a_removed_links_sheet_stops_blocking_the_parsers_scripts() {
    let page = r#"<!DOCTYPE html>
<link rel="stylesheet" href="/held.css" id="held">
<script async src="/remove.js"></script>
<script>testRunner.reportHarnessResults('ran without the sheet');</script>"#;
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        (
            "/held.css",
            Route::ok("text/css", "").with_delay(Duration::from_secs(600)),
        ),
        (
            "/remove.js",
            Route::ok(
                "text/javascript",
                "document.getElementById('held').remove();",
            ),
        ),
    ]);
    let result = headless::run_testharness(
        &server.url("/"),
        Duration::from_secs(10),
        Default::default(),
    )
    .expect("the page reports");
    assert_eq!(result, "ran without the sheet");
}

/// A frame's media queries asked while its stylesheet loads see its
/// iframe's size, from the parent's layout, without styling the frame's
/// document: once the stylesheet is in, its first style already has it,
/// so nothing transitions from the style it would have had without it.
#[test]
fn a_loading_frames_media_queries_leave_it_unstyled() {
    let page = r#"<!DOCTYPE html>
<iframe id="f" src="/child.html"></iframe>
<script>
const f = document.getElementById('f');
let matched;
const ask = () => {
  const w = f.contentWindow;
  const t = w.document.getElementById('t');
  if (!t) {
    setTimeout(ask, 5);
    return;
  }
  matched = w.matchMedia('(max-width: 400px)').matches;
};
ask();
onload = () => {
  const w = f.contentWindow;
  testRunner.reportHarnessResults(
      `${matched} ${w.getComputedStyle(w.document.getElementById('t')).color}`);
};
</script>"#;
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        (
            "/child.html",
            Route::ok(
                "text/html",
                r#"<!DOCTYPE html><link rel="stylesheet" href="/slow.css"><div id="t">x</div>"#,
            ),
        ),
        (
            "/slow.css",
            Route::ok(
                "text/css",
                "#t { color: rgb(0, 128, 0); transition: color 100s linear }",
            )
            .with_delay(Duration::from_secs(1)),
        ),
    ]);
    let result = headless::run_testharness(
        &server.url("/"),
        Duration::from_secs(30),
        Default::default(),
    )
    .expect("the page reports");
    assert_eq!(result, "true rgb(0, 128, 0)");
}

/// The same for a frame whose `<link>` script just inserted, before the
/// renderer started loading it: the frame waits for it all the same, as
/// Blink's `<link>` starts loading as it is inserted.
#[test]
fn a_frames_media_queries_before_its_link_loads_leave_it_unstyled() {
    let child = r#"<!DOCTYPE html>
<script>
const link = document.createElement('link');
link.rel = 'stylesheet';
link.href = '/slow.css';
document.head.append(link);
const t = document.createElement('div');
t.id = 't';
document.documentElement.append(t);
parent.matched = matchMedia('(max-width: 400px)').matches;
</script>"#;
    let page = r#"<!DOCTYPE html>
<iframe id="f" src="/child.html"></iframe>
<script>
onload = () => {
  const w = document.getElementById('f').contentWindow;
  testRunner.reportHarnessResults(
      `${matched} ${w.getComputedStyle(w.document.getElementById('t')).color}`);
};
</script>"#;
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        ("/child.html", Route::ok("text/html", child)),
        (
            "/slow.css",
            Route::ok(
                "text/css",
                "#t { color: rgb(0, 128, 0); transition: color 100s linear }",
            )
            .with_delay(Duration::from_secs(1)),
        ),
    ]);
    let result = headless::run_testharness(
        &server.url("/"),
        Duration::from_secs(30),
        Default::default(),
    )
    .expect("the page reports");
    assert_eq!(result, "true rgb(0, 128, 0)");
}

#[test]
fn a_web_page_may_not_load_local_files() {
    let dir = std::env::temp_dir().join(format!("machopo-file-access-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let css = dir.join("red.css");
    std::fs::write(&css, "div { background-color: red }").unwrap();
    let css_url = net::file_url(&css).unwrap();
    let page = format!(
        r#"<!DOCTYPE html>
        <style>html, body {{ margin: 0 }} div {{ width: 20px; height: 20px; background-color: green }}</style>
        <link rel="stylesheet" href="{css_url}"><div></div>"#
    );
    let server = server(&[("/", Route::ok("text/html", page))]);
    assert_eq!(pixel(&render(&server.url("/")), 10, 10), GREEN);
    let _ = std::fs::remove_dir_all(&dir);
}

static GREEN_PNG: &[u8] =
    include_bytes!("../../../../tests/reftests/images/resources/green-40x30.png");

#[test]
fn the_load_waits_for_images() {
    let page = r#"<!DOCTYPE html>
        <style>html, body { margin: 0 } img { display: block }</style>
        <img src="/slow.png">"#;
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        (
            "/slow.png",
            Route::ok("image/png", GREEN_PNG).with_delay(Duration::from_millis(300)),
        ),
    ]);
    let frame = render(&server.url("/"));
    assert_eq!(pixel(&frame, 10, 10), GREEN);
}

#[test]
fn an_image_with_an_error_status_shows() {
    // A PNG body, but a 404: drawn all the same, as in Chrome (Blink's
    // `ImageResource` decodes what comes; WPT's
    // `404-response-with-actual-image-data.html`).
    let page = r#"<!DOCTYPE html>
        <style>html, body { margin: 0 } img { display: block }</style>
        <img src="/missing.png" width="10" height="10">
        <div style="width: 10px; height: 10px; background-color: green"></div>"#;
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        (
            "/missing.png",
            Route::new(404, GREEN_PNG).with_header("Content-Type", "image/png"),
        ),
    ]);
    let frame = render(&server.url("/"));
    assert_eq!(pixel(&frame, 5, 5), GREEN);
    assert_eq!(pixel(&frame, 5, 15), GREEN);
}

mod scrolling {
    //! Input end to end: the browser forwards it, the renderer's compositor
    //! (or main thread) scrolls, and the compositor draws the new offsets.

    use protocol::input::{InputEvent, KeyboardEvent, Modifiers, ScrollDelta};

    use super::*;

    const PAGE: &str = r#"<!DOCTYPE html>
        <style>html, body { margin: 0 } div { width: 20px }</style>
        <div id=box style="height: 50px; overflow: auto">
          <div style="height: 50px; background-color: red"></div>
          <div style="height: 50px; background-color: green"></div>
        </div>
        <div style="height: 950px; background-color: red"></div>
        <div style="height: 200px; background-color: green"></div>"#;

    fn scrolled(input: &[InputEvent]) -> Frame {
        let server = server(&[("/", Route::ok("text/html", PAGE))]);
        headless::render_after_input(&server.url("/"), Size::new(200, 200), 1.0, true, input)
            .expect("the page renders")
    }

    #[test]
    fn a_wheel_over_a_scroll_container_scrolls_it() {
        let frame = scrolled(&[InputEvent::wheel(
            10.0,
            10.0,
            ScrollDelta::Pixels { x: 0.0, y: 50.0 },
        )]);
        // The container's green half has come up; the viewport stayed.
        assert_eq!(pixel(&frame, 10, 10), GREEN);
        assert_eq!(pixel(&frame, 10, 60), [255, 0, 0, 255]);
    }

    #[test]
    fn a_wheel_elsewhere_scrolls_the_viewport_in_lines() {
        // 25 lines of 40px: 1000px, past the red to the green at the end.
        let frame = scrolled(&[InputEvent::wheel(
            100.0,
            100.0,
            ScrollDelta::Lines { x: 0.0, y: 25.0 },
        )]);
        assert_eq!(pixel(&frame, 10, 10), GREEN);
    }

    #[test]
    fn end_scrolls_the_viewport_to_the_bottom() {
        let end = |down| {
            InputEvent::Key(KeyboardEvent {
                down,
                key: "End".to_owned(),
                code: "End".to_owned(),
                key_code: 35,
                text: String::new(),
                location: 0,
                repeat: false,
                modifiers: Modifiers::NONE,
            })
        };
        let frame = scrolled(&[end(true), end(false)]);
        // The document is 1200px; the last 200 are green.
        assert_eq!(pixel(&frame, 10, 10), GREEN);
        assert_eq!(pixel(&frame, 10, 190), GREEN);
    }
}

mod links {
    //! Clicks end to end: the renderer hit tests, follows the link, and the
    //! frame shows where it went.

    use protocol::input::{InputEvent, Modifiers, MouseButton};

    use super::*;

    fn click(x: f32, y: f32) -> [InputEvent; 2] {
        let button = MouseButton::Left;
        [
            InputEvent::mouse_down(x, y, button, Modifiers::NONE),
            InputEvent::mouse_up(x, y, button, Modifiers::NONE),
        ]
    }

    fn clicked(url: &str, x: f32, y: f32) -> Frame {
        headless::render_after_input(url, Size::new(200, 200), 1.0, true, &click(x, y))
            .expect("the page renders")
    }

    #[test]
    fn a_click_on_a_links_text_loads_the_page() {
        let server = server(&[
            (
                "/",
                Route::ok(
                    "text/html",
                    r#"<!DOCTYPE html><style>html, body { margin: 0 }</style>
                    <p style="font: 20px/1 Ahem; margin: 0">XX <a href="next"><span>XX</span></a></p>"#,
                ),
            ),
            (
                "/next",
                Route::ok(
                    "text/html",
                    r#"<!DOCTYPE html><title>Next</title>
                    <style>html, body { margin: 0 } img { display: block }</style>
                    <img src="/slow.png">"#,
                ),
            ),
            (
                "/slow.png",
                Route::ok("image/png", GREEN_PNG).with_delay(Duration::from_millis(300)),
            ),
        ]);
        // On the link's text, in a `<span>` in it.
        let frame = clicked(&server.url("/"), 70.0, 10.0);
        assert_eq!(frame.url, Some(server.url("/next")));
        assert_eq!(frame.title.as_deref(), Some("Next"));
        // The new page once loaded, its slow image too.
        assert_eq!(pixel(&frame, 10, 10), GREEN);
    }

    #[test]
    fn a_link_to_a_fragment_scrolls_without_loading() {
        let server = server(&[(
            "/",
            Route::ok(
                "text/html",
                r##"<!DOCTYPE html><style>html, body { margin: 0 } div { height: 1000px }</style>
                <a href="#target" style="display: block; height: 50px"></a>
                <div style="background-color: red"></div>
                <div id="target" style="background-color: green"></div>"##,
            ),
        )]);
        let frame = clicked(&server.url("/"), 10.0, 10.0);
        assert_eq!(frame.url, Some(format!("{}#target", server.url("/"))));
        assert_eq!(pixel(&frame, 10, 10), GREEN);
        assert_eq!(pixel(&frame, 10, 190), GREEN);
        assert_eq!(server.requests().len(), 1);
    }

    /// A link to `#top`, or to the empty fragment (`#`), with no element of
    /// that name: the top of the document.
    #[test]
    fn a_link_to_top_scrolls_to_the_top() {
        for fragment in ["top", ""] {
            let server = server(&[(
                "/",
                Route::ok(
                    "text/html",
                    format!(
                        r##"<!DOCTYPE html><style>html, body {{ margin: 0 }} div {{ height: 1000px }}</style>
                <div style="background-color: green"></div>
                <div id="target" style="background-color: red"><a href="#{fragment}" style="display: block; height: 50px"></a></div>"##
                    ),
                ),
            )]);
            // Loaded at the red target, whose link goes back to the top.
            let frame = clicked(&format!("{}#target", server.url("/")), 10.0, 10.0);
            assert_eq!(frame.url, Some(format!("{}#{fragment}", server.url("/"))));
            assert_eq!(pixel(&frame, 10, 10), GREEN, "#{fragment}");
        }
    }
}

/// Background images load with the page, and the frame waits for them as
/// it does for `<img>`s: a stylesheet's `url()` resolves against the
/// stylesheet, each URL loads once however many elements show it, and an
/// element that isn't rendered loads none.
#[test]
fn background_images_load_with_the_page() {
    let png = include_bytes!("../../../../tests/reftests/images/resources/green-40x30.png");
    let page = r#"<!DOCTYPE html>
        <style>html, body { margin: 0 } div { width: 20px; height: 20px }
               #b, #c { background: url(/green.png) }
               #hidden { display: none; background: url(/never.png) }</style>
        <link rel="stylesheet" href="/css/sheet.css">
        <div id="a"></div><div id="b"></div><div id="c"></div><div id="hidden"></div>"#;
    // The stylesheet comes last, when nothing else is loading: the
    // document isn't loaded until the image it names is.
    let delay = Duration::from_millis(300);
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        (
            "/css/sheet.css",
            Route::ok("text/css", "#a { background-image: url(img/green.png) }").with_delay(delay),
        ),
        (
            "/css/img/green.png",
            Route::ok("image/png", png.to_vec()).with_delay(delay),
        ),
        ("/green.png", Route::ok("image/png", png.to_vec())),
        ("/never.png", Route::ok("image/png", png.to_vec())),
    ]);
    let frame = render(&server.url("/"));
    for y in [10, 30, 50] {
        assert_eq!(pixel(&frame, 10, y), GREEN, "square at y = {y}");
    }
    assert!(frame.failed_subresources.is_empty());
    let requested = |path: &str| {
        server
            .requests()
            .iter()
            .filter(|request| request.target == path)
            .count()
    };
    assert_eq!(requested("/css/img/green.png"), 1);
    assert_eq!(requested("/green.png"), 1);
    assert_eq!(requested("/never.png"), 0);
}

/// A background image script shows later loads: an element given one by a
/// class, and one that stops being in a `display: none` subtree (its own
/// style unchanged); not one still in such a subtree.
#[test]
fn background_images_script_shows_load() {
    let png = include_bytes!("../../../../tests/reftests/images/resources/green-40x30.png");
    let page = r#"<!DOCTYPE html><html class="reftest-wait">
        <style>div { width: 20px; height: 20px } .on { background: url(/later.png) }
               #inner { background: url(/shown.png) } #still { background: url(/never.png) }
               .hidden { display: none }</style>
        <div id="a"></div><div class="hidden" id="outer"><div id="inner"></div></div>
        <div class="hidden"><div id="still"></div></div>
        <script>
        onload = () => {
          document.getElementById('a').className = 'on';
          document.getElementById('outer').className = '';
          setTimeout(() => document.documentElement.className = '', 100);
        };
        </script>"#;
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        ("/later.png", Route::ok("image/png", png.to_vec())),
        ("/shown.png", Route::ok("image/png", png.to_vec())),
        ("/never.png", Route::ok("image/png", png.to_vec())),
    ]);
    render(&server.url("/"));
    let requested = |path: &str| {
        server
            .requests()
            .iter()
            .filter(|request| request.target == path)
            .count()
    };
    assert_eq!(requested("/later.png"), 1);
    assert_eq!(requested("/shown.png"), 1);
    assert_eq!(requested("/never.png"), 0);
}

/// A document's content language is its response's `Content-Language`,
/// up to the first comma (as Blink takes it), a child frame's its own;
/// `:lang()` matches an element without a language of its own by it, and
/// a `<meta http-equiv="content-language">` inserted later replaces it.
#[test]
fn content_language_comes_from_the_response() {
    let page = r#"<!DOCTYPE html>
        <div id="a"></div><div id="b" lang="ja"></div>
        <iframe id="f" src="/child"></iframe>
        <script>
        onload = () => {
          const matches = (e, s) => e.matches(s) ? s : 'not ' + s;
          const child = document.getElementById('f').contentDocument.body;
          const a = document.getElementById('a');
          const log = [matches(a, ':lang(de)'), matches(document.getElementById('b'), ':lang(de)'),
                       matches(child, ':lang(fr)')];
          const meta = document.createElement('meta');
          meta.setAttribute('http-equiv', 'content-language');
          meta.setAttribute('content', 'es');
          document.head.append(meta);
          log.push(matches(a, ':lang(es)'));
          document.title = log.join(' / ');
        };
        </script>"#;
    let server = server(&[
        (
            "/",
            Route::ok("text/html", page).with_header("Content-Language", " de , en"),
        ),
        (
            "/child",
            Route::ok("text/html", "<!DOCTYPE html><body>").with_header("Content-Language", "fr"),
        ),
    ]);
    let frame = render(&server.url("/"));
    assert_eq!(
        frame.title.as_deref(),
        Some(":lang(de) / not :lang(de) / :lang(fr) / :lang(es)")
    );
}

/// An `image-set()` shows the image for the device scale factor, its
/// natural size divided by its resolution, and only that one loads, as in
/// Chrome 152: `a.png` (40×30) at 1x, `b.png` (20×40, at 2x 10×20 CSS px)
/// at 2x.
#[test]
fn an_image_set_shows_and_loads_the_image_for_the_scale() {
    let page = r#"<!DOCTYPE html>
        <style>html, body { margin: 0 }
        div { width: 100px; height: 50px; background-repeat: no-repeat;
              background-image: image-set(url(/b.png) 2x, url(/a.png) 1x) }</style>
        <div></div>"#;
    for (scale, shown, (width, height)) in [(1.0, "/a.png", (40, 30)), (2.0, "/b.png", (20, 40))] {
        let server = server(&[
            ("/", Route::ok("text/html", page)),
            ("/a.png", Route::ok("image/png", GREEN_PNG)),
            ("/b.png", Route::ok("image/png", TALL_GREEN_PNG)),
        ]);
        let frame = headless::render(&server.url("/"), Size::new(200, 200), scale, true)
            .expect("the page renders");
        // Device pixels.
        assert_eq!(pixel(&frame, width - 1, height - 1), GREEN, "at {scale}x");
        assert_ne!(pixel(&frame, width, height - 1), GREEN, "at {scale}x");
        assert_ne!(pixel(&frame, width - 1, height), GREEN, "at {scale}x");
        let images: Vec<String> = server
            .requests()
            .into_iter()
            .map(|request| request.target)
            .filter(|target| target.ends_with(".png"))
            .collect();
        assert_eq!(images, [shown], "at {scale}x");
    }
}

static TALL_GREEN_PNG: &[u8] =
    include_bytes!("../../../../tests/reftests/images/resources/green-20x40.png");

/// A page reached through a redirect: headless hands the tab the
/// navigation's redirect, and it goes on to the page.
#[test]
fn a_page_reached_through_a_redirect_renders() {
    let page = "<!DOCTYPE html><style>html, body { margin: 0; background: green }</style>";
    let server = server(&[
        ("/start", Route::redirect(302, "/page")),
        ("/page", Route::ok("text/html", page)),
    ]);
    let frame = render(&server.url("/start"));
    assert_eq!(pixel(&frame, 100, 100), GREEN);
}
