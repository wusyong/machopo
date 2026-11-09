//! A document committed in the renderer, its parser pausing at each script; inline and external
//! classic scripts run in order, each seeing only what was parsed before
//! it and itself as `currentScript`, a microtask checkpoint after each
//! (when `currentScript` is null again, on the document it was set on if
//! the script moved); module, other-typed and
//! `nomodule` scripts don't run, nor one whose `src` is empty (its
//! `error` fired) or doesn't load; one script inserts runs as it is
//! inserted; what a script throws is reported at the window; the page's
//! `testRunner.reportHarnessResults` ends the run. What the parser inserts
//! reaches the mutation observers before the next script. Drawn: its style,
//! layout and paint, and what a timer's script changed.

use std::path::PathBuf;
use std::time::Duration;

use shell::headless;

/// `files` (name, contents) in this test's scratch directory; the `file:`
/// URL of the first.
fn site(test: &str, files: &[(&str, &str)]) -> String {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("shell-documents")
        .join(test);
    std::fs::create_dir_all(&dir).unwrap();
    for (name, contents) in files {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().expect("in the site")).unwrap();
        std::fs::write(path, contents).unwrap();
    }
    net::file_url(&dir.join(files[0].0)).expect("an absolute path")
}

fn run(url: &str) -> String {
    headless::run_testharness(url, Duration::from_secs(30), Default::default())
        .expect("the page reports")
}

const PAGE: &str = r#"<!DOCTYPE html>
<title>scripts</title>
<script>var log = ['inline ' + document.getElementsByTagName('p').length];</script>
<p>one</p>
<script src="ext.js"></script>
<p>two</p>
<script type="module">log.push('module');</script>
<script type="text/plain">log.push('plain');</script>
<script nomodule>log.push('nomodule');</script>
<script type=" TEXT/JavaScript ">log.push('typed');</script>
<script>
window.addEventListener('error', e => log.push('error ' + e.lineno + ' ' + e.filename.split('/').pop()));
log.push(document instanceof Document && window.document === document);
</script>
<script>

notDefined();
</script>
<script src="missing.js"></script>
<script src=""></script>
<script>Promise.resolve().then(() => log.push('micro ' + document.currentScript));</script>
<script>var other = document.implementation.createHTMLDocument('');
other.body.appendChild(document.currentScript);
Promise.resolve().then(() => log.push('moved ' + document.currentScript + ' ' + other.currentScript));</script>
<script>log.push('after');</script>
<script>testRunner.reportHarnessResults(log.join())</script>
"#;

const EXTERNAL: &str = "log.push('external ' + document.getElementsByTagName('p').length);\nlog.push(document.currentScript.getAttribute('src'));\n";

#[test]
fn scripts_run_in_order_as_the_parser_pauses() {
    let url = site("scripts", &[("index.html", PAGE), ("ext.js", EXTERNAL)]);
    assert_eq!(
        run(&url),
        "inline 0,external 1,ext.js,typed,true,error 17 index.html,micro null,moved null null,after"
    );
}

const INSERTED: &str = r#"<!DOCTYPE html>
<script id=empty></script>
<script>
var log = [];
var s = document.createElement('script');
s.text = "log.push('inserted ' + (document.currentScript === s))";
document.head.appendChild(s);
log.push('after insert ' + document.currentScript.id);
log.push('async ' + document.getElementById('empty').async + ' ' + document.currentScript.async);
document.getElementById('empty').text = "log.push('filled')";
</script>
<script id=last src=" " onerror="log.push('error'); testRunner.reportHarnessResults(log.join())"></script>
"#;

/// A script script inserts runs as it is inserted (`currentScript` it,
/// then the inserting one's again); the parser's empty one runs once
/// script fills it, as soon as it can (`async`, as the parser's that ran
/// isn't); an empty `src` fires `error` at its element.
#[test]
fn scripts_script_inserts_run_as_inserted() {
    let url = site("inserted", &[("index.html", INSERTED)]);
    assert_eq!(
        run(&url),
        "inserted true,after insert ,async true false,filled,error"
    );
}

/// The document's encoding and content type, as its load gave them.
#[test]
fn the_documents_encoding_and_content_type() {
    let url = site(
        "metadata",
        &[(
            "index.html",
            "<meta charset=windows-1252><script>\
             testRunner.reportHarnessResults(document.characterSet + ' ' + document.contentType)</script>",
        )],
    );
    assert_eq!(run(&url), "windows-1252 text/html");
}

/// Premultiplied RGBA at `(x, y)`.
fn pixel(frame: &headless::Frame, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * frame.size.width + x) * 4) as usize;
    frame.pixels[i..i + 4].try_into().unwrap()
}

const GREEN: [u8; 4] = [0, 128, 0, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];

/// The DOM's document drawn: its `<style>` and `style` attributes,
/// its boxes, text, and the frame written as headless writes one.
#[test]
fn a_document_is_drawn() {
    let url = site(
        "drawn",
        &[(
            "index.html",
            "<!doctype html><style>body { margin: 0 } .g { background: green }</style>\
             <div class=g style='width: 50px; height: 20px'></div>\
             <div style='margin-left: 60px; width: 30px; height: 30px; background: rgb(0, 128, 0)'></div>",
        )],
    );
    let frame = headless::render_after_input(&url, gfx::Size::new(100, 100), 1.0, true, &[])
        .expect("drawn");
    assert_eq!(pixel(&frame, 10, 10), GREEN);
    assert_eq!(pixel(&frame, 55, 10), WHITE);
    assert_eq!(pixel(&frame, 70, 30), GREEN);
    assert_eq!(pixel(&frame, 70, 60), WHITE);
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shell-documents/drawn.png");
    headless::run(
        &url,
        headless::Options {
            out: out.clone(),
            viewport_size: gfx::Size::new(100, 100),
            device_scale_factor: 1.0,
            test_fonts: true,
            num_raster_threads: None,
        },
    )
    .expect("written");
    assert!(out.exists());
}

/// What script changes is drawn: before the load, and from a timer while
/// the root element keeps `reftest-wait` (the frame that shows it taking
/// it off is the one written).
#[test]
fn what_script_changes_is_drawn() {
    let url = site(
        "changed",
        &[(
            "index.html",
            "<!doctype html><html class=reftest-wait><style>body { margin: 0 } div { height: 20px }</style>\
             <div id=a></div><div id=b></div>\
             <script>const [a, b] = document.getElementsByTagName('div');\
             a.setAttribute('style', 'background: green');\
             setTimeout(() => { b.setAttribute('style', 'background: green');\
             document.documentElement.removeAttribute('class') }, 50)</script>",
        )],
    );
    let frame = headless::render_after_input(&url, gfx::Size::new(100, 100), 1.0, true, &[])
        .expect("drawn");
    assert_eq!(pixel(&frame, 10, 10), GREEN, "before the load");
    assert_eq!(pixel(&frame, 10, 30), GREEN, "from the timer");
    assert_eq!(pixel(&frame, 10, 50), WHITE);
}

/// The compositor scrolls the DOM's viewport and its scroll
/// containers (the main thread takes the offsets back: the next frame
/// draws them where they scrolled to).
#[test]
fn the_compositor_scrolls_it() {
    use protocol::input::{InputEvent, Modifiers, ScrollDelta};
    let wheel = |x, y, dy| InputEvent::MouseWheel {
        x,
        y,
        delta: ScrollDelta::Pixels { x: 0.0, y: dy },
        buttons: 0,
        modifiers: Modifiers::default(),
    };
    let url = site(
        "scrolled",
        &[(
            "index.html",
            "<!doctype html><style>body { margin: 0 } .g { background: green }</style>\
             <div class=g style='height: 100px'></div><div style='height: 2000px'></div>",
        )],
    );
    let size = gfx::Size::new(100, 100);
    let still = headless::render_after_input(&url, size, 1.0, true, &[]).expect("drawn");
    assert_eq!(pixel(&still, 10, 10), GREEN);
    let scrolled = headless::render_after_input(&url, size, 1.0, true, &[wheel(10.0, 10.0, 150.0)])
        .expect("drawn");
    assert_eq!(pixel(&scrolled, 10, 10), WHITE, "the viewport scrolled");
    let url = site(
        "container",
        &[(
            "index.html",
            "<!doctype html><style>body { margin: 0 } .g { background: green }</style>\
             <div style='height: 50px; overflow: auto'><div class=g style='height: 50px'></div>\
             <div style='height: 500px'></div></div><div class=g style='height: 20px'></div>\
             <div style='height: 2000px'></div>",
        )],
    );
    let scrolled = headless::render_after_input(&url, size, 1.0, true, &[wheel(10.0, 10.0, 60.0)])
        .expect("drawn");
    assert_eq!(pixel(&scrolled, 10, 10), WHITE, "the container scrolled");
    assert_eq!(pixel(&scrolled, 10, 60), GREEN, "the viewport didn't");
}

/// The load asks for the frame `DidFinishLoad` goes with, though it
/// changed nothing (a slow external script, the last thing parsed, that
/// does nothing): the page's first frame was before it.
#[test]
fn the_load_is_drawn() {
    use std::collections::HashMap;

    use net::test_server::{Route, TestServer};
    let mut slow = Route::ok("text/javascript", "");
    slow.delay = Duration::from_millis(300);
    let server = TestServer::start(HashMap::from([
        (
            "/index.html".to_owned(),
            Route::ok(
                "text/html",
                "<!doctype html><style>body { margin: 0 }</style>\
                 <div style='height: 20px; background: green'></div><script src=\"/slow.js\"></script>",
            ),
        ),
        ("/slow.js".to_owned(), slow),
    ]))
    .expect("the test server starts");
    let frame = headless::render_after_input(
        &server.url("/index.html"),
        gfx::Size::new(100, 100),
        1.0,
        true,
        &[],
    )
    .expect("drawn once loaded");
    assert_eq!(pixel(&frame, 10, 10), GREEN);
}

/// A route serving `css` as CSS after `delay` ms.
fn sheet(css: &str, delay: u64) -> net::test_server::Route {
    let mut route = net::test_server::Route::ok("text/css", css);
    route.delay = Duration::from_millis(delay);
    route
}

/// A `<link>`'s sheet is drawn, and blocks rendering till it is in: the
/// first frame drawn has it.
#[test]
fn a_linked_sheet_blocks_rendering() {
    use std::collections::HashMap;

    use net::test_server::{Route, TestServer};
    let server = TestServer::start(HashMap::from([
        (
            "/index.html".to_owned(),
            Route::ok(
                "text/html",
                "<!doctype html><link rel=stylesheet href=/a.css>\
                 <style>body { margin: 0 }</style><div style='height: 20px'></div>",
            ),
        ),
        ("/a.css".to_owned(), sheet("div { background: green }", 300)),
    ]))
    .expect("the test server starts");
    let frame = headless::render_after_input(
        &server.url("/index.html"),
        gfx::Size::new(100, 100),
        1.0,
        true,
        &[],
    )
    .expect("drawn");
    assert_eq!(pixel(&frame, 10, 10), GREEN);
    assert_eq!(frame.frames_drawn, 1, "nothing drawn before the sheet");
}

/// An alternate `<link>`'s sheet loads (the window's `load` waits for it),
/// but blocks nothing and doesn't apply; a sheet that isn't `text/css`
/// applies only in quirks mode.
#[test]
fn an_alternate_sheet_doesnt_block_or_apply() {
    use std::collections::HashMap;

    use net::test_server::{Route, TestServer};
    let mut plain = Route::ok("text/plain", "div { margin-left: 50px }");
    plain.delay = Duration::from_millis(100);
    let server = TestServer::start(HashMap::from([
        (
            "/index.html".to_owned(),
            Route::ok(
                "text/html",
                "<!doctype html><link rel='alternate stylesheet' href=/alt.css>\
                 <link rel=stylesheet href=/plain.css>\
                 <style>body { margin: 0 }</style><div style='height: 20px; background: green'></div>\
                 <script>window.addEventListener('load', () => document.getElementsByTagName('div')[0]\
                 .setAttribute('style', 'height: 20px; background: blue'))</script>",
            ),
        ),
        (
            "/quirks.html".to_owned(),
            Route::ok(
                "text/html",
                "<link rel=stylesheet href=/plain.css>\
                 <style>body { margin: 0 }</style><div style='height: 20px; background: green'></div>",
            ),
        ),
        ("/alt.css".to_owned(), sheet("div { visibility: hidden }", 500)),
        ("/plain.css".to_owned(), plain),
    ]))
    .expect("the test server starts");
    let size = gfx::Size::new(100, 100);
    // Green before the alternate one came (the other one, `text/plain`,
    // in), blue from the window's `load` after it: two frames.
    let frame = headless::render_after_input(&server.url("/index.html"), size, 1.0, true, &[])
        .expect("drawn");
    assert_eq!(pixel(&frame, 10, 10), [0, 0, 255, 255], "neither applies");
    assert_eq!(frame.frames_drawn, 2, "drawn before the alternate one came");
    let frame = headless::render_after_input(&server.url("/quirks.html"), size, 1.0, true, &[])
        .expect("drawn");
    assert_eq!(pixel(&frame, 10, 10), WHITE, "quirks mode takes it");
    assert_eq!(pixel(&frame, 60, 10), GREEN);
}

/// A `<link>` script adds later, from a timer, loads too.
#[test]
fn a_link_script_adds_loads() {
    use std::collections::HashMap;

    use net::test_server::{Route, TestServer};
    let server = TestServer::start(HashMap::from([
        (
            "/index.html".to_owned(),
            Route::ok(
                "text/html",
                "<!doctype html><html class=reftest-wait><style>body { margin: 0 }</style>\
                 <div style='height: 20px'></div><script>setTimeout(() => {\
                 const link = document.createElement('link'); link.setAttribute('rel', 'stylesheet');\
                 link.setAttribute('href', '/a.css');\
                 link.addEventListener('load', () => document.documentElement.removeAttribute('class'));\
                 document.documentElement.appendChild(link) }, 50)</script>",
            ),
        ),
        ("/a.css".to_owned(), sheet("div { background: green }", 0)),
    ]))
    .expect("the test server starts");
    let frame = headless::render_after_input(
        &server.url("/index.html"),
        gfx::Size::new(100, 100),
        1.0,
        true,
        &[],
    )
    .expect("drawn");
    assert_eq!(pixel(&frame, 10, 10), GREEN);
}

/// `matchMedia` through the renderer: a document's environment is its
/// frame's viewport; an iframe resized by script reports `change` to its
/// document's list at the next rendering update.
#[test]
fn media_queries_follow_the_frames_viewport() {
    use std::collections::HashMap;

    use net::test_server::{Route, TestServer};
    let server = TestServer::start(HashMap::from([
        (
            "/index.html".to_owned(),
            Route::ok(
                "text/html",
                "<!DOCTYPE html><style>body { margin: 0 } iframe { border: 0; width: 100px; height: 50px }</style>\
                 <script>const log = [matchMedia('(min-width: 400px)').matches, matchMedia('(max-width: 900px)').matches];\
                 function heard(matches) { log.push('changed ' + matches); testRunner.reportHarnessResults(log.join()); }\
                 function ready(narrow) { log.push('frame ' + narrow);\
                   requestAnimationFrame(() => { document.querySelector('iframe').style.width = '300px'; }); }</script>\
                 <iframe src=/frame.html></iframe>",
            ),
        ),
        (
            "/frame.html".to_owned(),
            Route::ok(
                "text/html",
                "<!DOCTYPE html><script>const wide = matchMedia('(min-width: 200px)');\
                 wide.onchange = (e) => parent.heard(e.matches);\
                 onload = () => parent.ready(wide.matches);</script>",
            ),
        ),
    ]))
    .expect("the test server starts");
    assert_eq!(
        headless::run_testharness(
            &server.url("/index.html"),
            Duration::from_secs(30),
            Default::default()
        )
        .expect("the page reports"),
        "true,true,frame false,changed true"
    );
}

/// `getComputedStyle` through the renderer: a value style decides, one
/// layout does (the used width), a pseudo-element's, `::selection`'s;
/// and a `::selection` rule drawn on the selected text (its children
/// inherit it).
#[test]
fn computed_styles_and_selection() {
    let url = site(
        "computed-styles",
        &[(
            "index.html",
            "<!DOCTYPE html><style>body { margin: 0 } #d { width: 50%; color: rgb(1, 2, 3) }\
             #d::before { content: 'x'; color: blue } p::selection { background-color: green; color: green }\
             p { font: 20px/1 Ahem, monospace; margin: 0 }</style>\
             <p id=p><span>XXXXX</span></p><div style='width: 100px'><div id=d></div></div><script>\
             const d = document.getElementById('d'), s = getComputedStyle(d);\
             const p = document.getElementById('p');\
             getSelection().selectAllChildren(p);\
             testRunner.reportHarnessResults([s.color, s.width, getComputedStyle(d, '::before').color,\
               getComputedStyle(p.firstChild, '::selection').backgroundColor,\
               getComputedStyle(d, '::selection').backgroundColor].join('|'));</script>",
        )],
    );
    assert_eq!(
        run(&url),
        "rgb(1, 2, 3)|50px|rgb(0, 0, 255)|rgb(0, 128, 0)|rgba(0, 0, 0, 0)"
    );
    let frame = headless::render_after_input(&url, gfx::Size::new(100, 100), 1.0, true, &[])
        .expect("drawn");
    // The span's text, selected, in its parent's `::selection` colors.
    assert_eq!(pixel(&frame, 5, 5), GREEN);
}

/// Script's scrolling through the renderer: the viewport and a scroll
/// container scrolled at once (clamped to what can be scrolled), their
/// `scroll` events in the next frame's scroll steps, before its animation
/// frame callbacks, once each, in the order they first moved, a microtask
/// checkpoint after each: the document's bubbling to the window, the
/// element's not; a frame asked for by a scroll alone; a page scrolled
/// after it was drawn drawn again.
#[test]
fn script_scrolls_and_its_scroll_events() {
    let url = site(
        "scrolling",
        &[
            (
                "index.html",
                "<!DOCTYPE html><style>html, body { margin: 0 } #s { width: 50px; height: 50px; overflow: auto }\
                 #s div { height: 200px }</style><div style='height: 100px'></div>\
                 <div id=s><div></div></div><div style='height: 2000px'></div><script>\
                 const log = [], s = document.getElementById('s');\
                 addEventListener('scroll', (e) => { log.push('window ' + (e.target === document) + ' ' + e.bubbles);\
                   Promise.resolve().then(() => log.push('micro')); });\
                 document.addEventListener('scroll', () => log.push('document ' + scrollY));\
                 s.addEventListener('scroll', (e) => { log.push('s ' + e.bubbles + ' ' + s.scrollTop);\
                   if (s.scrollTop === 10) testRunner.reportHarnessResults(log.join()); });\
                 scrollTo(0, 100); s.scrollTop = 30; s.scrollTo({ top: 1000 }); scrollBy(0, 50);\
                 log.push('now ' + scrollY + ' ' + s.scrollTop);\
                 requestAnimationFrame(() => { log.push('frame'); setTimeout(() => { s.scrollTop = 10; }); });\
                 </script>",
            ),
            (
                "drawn.html",
                "<!DOCTYPE html><html class=reftest-wait><style>html, body { margin: 0 }\
                 #s { width: 50px; height: 50px; overflow: hidden } #s div { height: 50px }</style>\
                 <div style='height: 100px; background: red'></div><div id=s><div style='background: red'></div>\
                 <div style='background: green'></div></div><div style='height: 2000px'></div><script>\
                 const frame = (f) => requestAnimationFrame(() => requestAnimationFrame(f));\
                 frame(() => { scrollTo(0, 100); document.getElementById('s').scrollTop = 50;\
                   frame(() => document.documentElement.removeAttribute('class')); });</script>",
            ),
        ],
    );
    assert_eq!(
        run(&url),
        "now 150 150,document 150,window true true,micro,s false 150,frame,s false 10"
    );
    let drawn = url.replace("index.html", "drawn.html");
    let frame = headless::render_after_input(&drawn, gfx::Size::new(100, 100), 1.0, true, &[])
        .expect("drawn");
    assert_eq!(
        pixel(&frame, 10, 10),
        GREEN,
        "the viewport and the container scrolled"
    );
    assert_eq!(pixel(&frame, 80, 10), WHITE);
}

/// CSS transitions and animations through the renderer: a transition
/// started by a style change once drawn, its `transitionrun`,
/// `transitionstart` and `transitionend` (trusted, bubbling, its
/// property and elapsed time), its value partway as `getComputedStyle`
/// reads it; a CSS animation's `animationstart` and `animationend`.
#[test]
fn transitions_and_animations() {
    let url = site(
        "animations",
        &[(
            "index.html",
            "<!DOCTYPE html><style>#t { width: 10px; transition: width 200ms linear }\
             @keyframes grow { from { height: 0 } to { height: 10px } }\
             #a { animation: grow 50ms }</style><div id=t></div><div id=a></div><script>\
             const log = [], t = document.getElementById('t'), a = document.getElementById('a');\
             let partway;\
             for (const type of ['transitionrun', 'transitionstart', 'transitionend', 'animationstart', 'animationend'])\
               document.addEventListener(type, (e) => { log.push(e.type + ' ' + e.isTrusted + ' ' + e.bubbles + ' '\
                 + (e.propertyName || e.animationName) + ' ' + e.elapsedTime + ' ' + (e.target.id));\
                 if (e.type === 'transitionstart') requestAnimationFrame(() => requestAnimationFrame(() => {\
                   const w = parseFloat(getComputedStyle(t).width); partway = w > 10 && w < 110; }));\
                 if (e.type === 'transitionend') testRunner.reportHarnessResults(log.join() + '|' + partway); });\
             requestAnimationFrame(() => requestAnimationFrame(() => { t.style.width = '110px'; }));\
             </script>",
        )],
    );
    assert_eq!(
        run(&url),
        "animationstart true true grow 0 a,transitionrun true true width 0 t,transitionstart true true width 0 t,\
         animationend true true grow 0.05 a,transitionend true true width 0.2 t|true"
    );
}

/// A frame whose only change is an animation's value runs the rendering
/// update: an element an animation moves into the viewport, with no other
/// change asking for a frame, reaches an intersection observer (which
/// the rendering update computes, after its paint).
#[test]
fn an_animations_value_is_rendered() {
    let url = site(
        "animation-rendering",
        &[(
            "index.html",
            "<!DOCTYPE html><style>@keyframes in { from { margin-left: -100px } to { margin-left: 0 } }\
             #r { width: 10px; height: 10px; animation: in 100ms steps(1, end) forwards }</style>\
             <div id=r></div><script>const seen = [];\
             new IntersectionObserver((entries) => { seen.push(entries[0].isIntersecting);\
               if (entries[0].isIntersecting) testRunner.reportHarnessResults(seen.join()); })\
               .observe(document.getElementById('r'));</script>",
        )],
    );
    assert_eq!(run(&url), "false,true");
}

/// The events one update of animations sends, in css-animations 2's
/// order: tree order, an element's before its `::marker`'s before its
/// `::before`'s (style starts them in another order); a microtask
/// checkpoint after each.
#[test]
fn animation_events_in_order_each_checkpointed() {
    let url = site(
        "animation-events",
        &[(
            "index.html",
            "<!DOCTYPE html><style>@keyframes g { to { opacity: 0.5 } }\
             li, li::marker, li::before { animation: g 10s } li::before { content: 'x' }</style>\
             <ul><li id=l></li><li id=m></li></ul><script>const log = [];\
             document.addEventListener('animationstart', (e) => { log.push(e.target.id + e.pseudoElement);\
               Promise.resolve().then(() => log.push('micro'));\
               if (log.length === 11) setTimeout(() => testRunner.reportHarnessResults(log.join())); });</script>",
        )],
    );
    assert_eq!(
        run(&url),
        "l,micro,l::marker,micro,l::before,micro,m,micro,m::marker,micro,m::before,micro"
    );
}

/// Web Animations through the renderer: `animate()` plays an animation
/// whose `ready` resolves once a frame shows it and `finished` once it
/// ends, its callbacks (and their microtasks) before the `finish` event;
/// `getAnimations()` lists a CSS animation as a `CSSAnimation`, the same
/// object each time; `cancel()` rejects `finished` and fires `cancel`.
#[test]
fn web_animations() {
    let url = site(
        "web-animations",
        &[(
            "index.html",
            "<!DOCTYPE html><style>@keyframes spin { to { opacity: 0.5 } } #c { animation: spin 10s }</style>\
             <div id=a></div><div id=c></div><script>\
             const log = [], a = document.getElementById('a'), c = document.getElementById('c');\
             const anim = a.animate({ opacity: [1, 0] }, { duration: 50, id: 'fade' });\
             log.push(anim instanceof Animation, anim.id, anim.playState, anim.pending, anim.timeline === document.timeline);\
             anim.onfinish = (e) => log.push('finish ' + (e instanceof AnimationPlaybackEvent) + ' ' + e.currentTime);\
             anim.ready.then((x) => log.push('ready ' + (x === anim) + ' ' + anim.pending));\
             anim.finished.then((x) => {\
               log.push('finished ' + (x === anim) + ' ' + anim.playState);\
               const css = c.getAnimations()[0];\
               log.push(css instanceof CSSAnimation, css.animationName, css === c.getAnimations()[0]);\
               log.push(document.getAnimations().length);\
               css.oncancel = () => { log.push('cancel'); testRunner.reportHarnessResults(log.join()); };\
               css.finished.catch((e) => log.push(e.name));\
               css.cancel();\
             });</script>",
        )],
    );
    assert_eq!(
        run(&url),
        "true,fade,running,true,true,ready true false,finished true finished,true,spin,true,1,\
         AbortError,finish true 50,cancel"
    );
}

/// Script's changes to animations ask for a frame of their own: the
/// `cancel` event of a paused animation (nothing runs) whose element left
/// the document, canceled by a timer, fires.
#[test]
fn script_changed_animations_get_frames() {
    let url = site(
        "script-animation-frames",
        &[(
            "index.html",
            "<!DOCTYPE html><body><script>\
             const d = document.body.appendChild(document.createElement('div'));\
             const a = d.animate({}, 100000); a.pause(); d.remove();\
             a.oncancel = () => testRunner.reportHarnessResults('cancel ' + a.playState);\
             a.ready.then(() => setTimeout(() => a.cancel(), 30));</script>",
        )],
    );
    assert_eq!(run(&url), "cancel idle");
}

/// `getAnimations()` lists a document's in composite order: its CSS
/// transitions before its CSS animations.
#[test]
fn get_animations_in_composite_order() {
    let url = site(
        "animation-order",
        &[(
            "index.html",
            "<!DOCTYPE html><style>@keyframes g { to { opacity: 0.5 } }\
             #t { opacity: 1; transition: opacity 10s } #t.out { opacity: 0; animation: g 10s }</style>\
             <div id=t></div><script>const t = document.getElementById('t');\
             requestAnimationFrame(() => requestAnimationFrame(() => { t.className = 'out';\
               testRunner.reportHarnessResults(document.getAnimations().map((x) => x.constructor.name).join(' '));\
             }));</script>",
        )],
    );
    assert_eq!(run(&url), "CSSTransition CSSAnimation");
}

/// Web Animations' effects through the renderer, with their keyframes
/// computed: replaced animations are removed (`remove`, `replaceState`)
/// unless persisted; a target changed moves the value from one element
/// to the other; `commitStyles()` writes its value to the inline style
/// of its target (one not rendered: an error).
#[test]
fn effects_replaced_committed_and_retargeted() {
    let url = site(
        "web-animation-effects",
        &[(
            "index.html",
            "<!DOCTYPE html><div id=a></div><div id=b></div><div id=c></div><script>\
             const log = [], a = document.getElementById('a'), b = document.getElementById('b'),\
               c = document.getElementById('c'), fill = { duration: 10, fill: 'forwards' };\
             const x = a.animate({ opacity: 0.1 }, fill), y = a.animate({ opacity: 0.2, width: '1px' }, fill);\
             const z = a.animate({ width: '2px' }, fill), w = a.animate({ opacity: 0.3 }, fill); y.persist();\
             y.onremove = () => log.push('y removed');\
             x.onremove = (e) => { log.push(e.type, [x, y, z, w].map((n) => n.replaceState).join(' '),\
                 a.getAnimations().length, getComputedStyle(a).opacity);\
               const m = b.animate({ opacity: [0.5, 0.5] }, 100000); m.pause();\
               m.ready.then(() => { m.effect.target = c; requestAnimationFrame(() => {\
                   log.push(getComputedStyle(b).opacity, getComputedStyle(c).opacity);\
                   m.commitStyles(); log.push(c.style.opacity, b.style.opacity);\
                   b.style.display = 'none';\
                   try { m.effect.target = b; m.commitStyles(); } catch (e) { log.push(e.name); }\
                   testRunner.reportHarnessResults(log.join()); }); }); };</script>",
        )],
    );
    assert_eq!(
        run(&url),
        "remove,removed persisted active active,3,0.3,1,0.5,0.5,,InvalidStateError"
    );
}

/// Script's animations that don't run, paused or filling once done, keep
/// their values when their element is styled again (its class changed).
#[test]
fn script_animations_not_running_kept_when_restyled() {
    let url = site(
        "script-animations-restyled",
        &[(
            "index.html",
            "<!DOCTYPE html><style>.x { color: green }</style><div id=p></div><div id=f></div><script>\
             const p = document.getElementById('p'), f = document.getElementById('f');\
             const paused = p.animate({ opacity: [0.5, 0.5] }, 100000); paused.pause();\
             const filling = f.animate({ opacity: 0.25 }, { duration: 1, fill: 'forwards' });\
             filling.finished.then(() => { p.className = f.className = 'x';\
               requestAnimationFrame(() => testRunner.reportHarnessResults(\
                 getComputedStyle(p).opacity + ' ' + getComputedStyle(f).opacity)); });</script>",
        )],
    );
    assert_eq!(run(&url), "0.5 0.25");
}

/// The animations of a frame script made as a rendering's animations
/// started (their promises' callbacks ran then), and of one in it, start
/// with the next rendering, which styles them: replaced once their
/// keyframes are computed, the last of them stays.
#[test]
fn animations_of_a_frame_made_while_starting_others() {
    let url = site(
        "frame-made-while-starting",
        &[(
            "index.html",
            "<!DOCTYPE html><body><script>\
             const play = async (doc) => { const d = doc.body.appendChild(doc.createElement('div'));\
               const a = d.animate({ opacity: 1 }, { duration: 1, fill: 'forwards' });\
               const b = d.animate({ opacity: 1 }, { duration: 1, fill: 'forwards' });\
               await a.finished; return a.replaceState + ' ' + b.replaceState; };\
             (async () => { const log = [await play(document)];\
               const frame = document.createElement('iframe');\
               await new Promise((loaded) => { frame.onload = loaded; document.body.appendChild(frame); });\
               log.push(await play(frame.contentDocument));\
               const inner = frame.contentDocument.createElement('iframe');\
               await new Promise((loaded) => { inner.onload = loaded; frame.contentDocument.body.appendChild(inner); });\
               log.push(await play(inner.contentDocument));\
               testRunner.reportHarnessResults(log.join());\
             })();</script>",
        )],
    );
    assert_eq!(run(&url), "removed active,removed active,removed active");
}

/// The classic history API through the browser: `pushState` to a
/// fragment changes the URL and the state and doesn't scroll, a
/// navigation to a fragment scrolls with the next frames and fires
/// `popstate` as the setter returns, then `hashchange` in a task; `history.back()` is the browser's
/// traversal, whose `popstate` has the entry's state (`history.state`'s
/// object) and which scrolls to the fragment unless the mode is manual.
#[test]
fn history_through_the_browser() {
    for (mode, scrolled) in [("auto", "true"), ("manual", "false")] {
        let url = site(
            &format!("history-{mode}"),
            &[(
                "index.html",
                &format!(
                    "<!DOCTYPE html><style>html, body {{ margin: 0 }}</style>\
                     <div style='height: 3000px'></div><div id=far style='height: 3000px'></div>\
                     <div id=far2 style='height: 3000px'></div><script>\
                     const frames = (n, then) => n ? requestAnimationFrame(() => frames(n - 1, then)) : then();\
                     const log = [];\
                     onload = () => {{\
                       history.scrollRestoration = '{mode}';\
                       history.pushState({{ n: 1 }}, '', '#far');\
                       log.push(location.hash, history.state.n, history.state === history.state, history.length);\
                       frames(2, () => {{\
                         log.push(scrollY);\
                         addEventListener('hashchange', (e) => log.push('hashchange ' + e.newURL.split('#')[1]), {{ once: true }});\
                         addEventListener('popstate', (e) => log.push('popstate ' + e.state), {{ once: true }});\
                         location.hash = '#far2';\
                         log.push(location.hash);\
                         frames(2, () => {{\
                           log.push(scrollY > 3000);\
                           scrollTo(0, 0);\
                           addEventListener('popstate', (e) => frames(2, () => {{\
                             log.push('back ' + location.hash, e.state.n, e.state === history.state, scrollY > 0);\
                             testRunner.reportHarnessResults(log.join(' / '));\
                           }}));\
                           history.back();\
                         }});\
                       }});\
                     }};</script>"
                ),
            )],
        );
        assert_eq!(
            run(&url),
            format!(
                "#far / 1 / true / 2 / 0 / popstate null / #far2 / hashchange far2 / true / back #far / 1 / true / {scrolled}"
            ),
            "{mode}"
        );
    }
}

/// The document's target element (`:target`) is the element its URL's
/// fragment names once it is parsed, and moves with a navigation to a
/// fragment (at once) and a traversal; not with `pushState`.
#[test]
fn the_target_element_follows_the_fragment() {
    let url = site(
        "target",
        &[(
            "index.html",
            "<!DOCTYPE html><style>:target { color: rgb(0, 128, 0) }</style>\
             <div id=a></div><div id=b></div><div id=c></div><script>\
             const log = [];\
             const target = () => (document.querySelector(':target') || { id: 'none' }).id;\
             addEventListener('DOMContentLoaded', () => log.push(target()));\
             onload = () => {\
               log.push(target(), getComputedStyle(document.getElementById('a')).color);\
               history.pushState(null, '', '#c');\
               log.push(target());\
               location.hash = '#b';\
               log.push(target(), getComputedStyle(document.getElementById('a')).color);\
               addEventListener('popstate', () => {\
                 log.push(target());\
                 testRunner.reportHarnessResults(log.join(' / '));\
               });\
               history.back();\
             };</script>",
        )],
    );
    assert_eq!(
        run(&format!("{url}#a")),
        "none / a / rgb(0, 128, 0) / a / b / rgb(0, 0, 0) / a"
    );
}

/// The top-level frame's `location` navigates it through the browser, a
/// new entry once the document is completely loaded (after its `load`):
/// the next page has it.
#[test]
fn the_top_level_location_navigates() {
    let url = site(
        "top-location",
        &[
            (
                "index.html",
                "<!DOCTYPE html><script>onload = () => setTimeout(() => { location.href = 'next.html#x'; });</script>",
            ),
            (
                "next.html",
                "<!DOCTYPE html><script>testRunner.reportHarnessResults(\
                 location.pathname.split('/').pop() + location.hash + ' ' + history.length);</script>",
            ),
        ],
    );
    assert_eq!(run(&url), "next.html#x 2");
}

/// The user's click on a link follows it through the browser, its URL
/// against the document's base URL (a `<base>`'s, which the sheet that
/// lays the links out is loaded against too); one to a new tab
/// (`target=_blank`) leaves the page where it is.
#[test]
fn links_the_user_follows() {
    use protocol::input::{InputEvent, Modifiers, MouseButton};
    let click = |x, y| {
        [
            InputEvent::MouseDown {
                x,
                y,
                button: MouseButton::Left,
                buttons: MouseButton::Left.buttons_bit(),
                modifiers: Modifiers::NONE,
                click_count: 1,
            },
            InputEvent::MouseUp {
                x,
                y,
                button: MouseButton::Left,
                buttons: 0,
                modifiers: Modifiers::NONE,
                click_count: 1,
            },
        ]
    };
    let url = site(
        "links",
        &[
            (
                "index.html",
                "<!DOCTYPE html><base href='sub/'><link rel=stylesheet href='s.css'>\
                 <a href='next.html'>next</a><a href='next.html' target=_blank>new tab</a>\
                 <script>onclick = (e) => { if (e.target.target) setTimeout(() => testRunner.reportHarnessResults('stayed'), 300); };</script>",
            ),
            (
                "sub/s.css",
                "body { margin: 0 } a { display: block; height: 50px }",
            ),
            (
                "sub/next.html",
                "<!DOCTYPE html><script>testRunner.reportHarnessResults(location.pathname.split('/').slice(-2).join('/'));</script>",
            ),
        ],
    );
    let size = gfx::Size::new(100, 100);
    let followed = headless::run_testharness_after_input(
        &url,
        size,
        &click(10.0, 10.0),
        Duration::from_secs(30),
    );
    assert_eq!(followed.as_deref(), Ok("sub/next.html"));
    let new_tab = headless::run_testharness_after_input(
        &url,
        size,
        &click(10.0, 60.0),
        Duration::from_secs(30),
    );
    assert_eq!(new_tab.as_deref(), Ok("stayed"));
}

/// A `javascript:` URL navigated to runs its script in the frame.
#[test]
fn a_javascript_url_runs_in_the_frame() {
    let url = site(
        "javascript-url",
        &[(
            "index.html",
            "<!DOCTYPE html><script>var ran = 'in the page';\
             onload = () => { location.href = 'javascript:testRunner.reportHarnessResults(ran + \\'%20\\' + location.pathname.split(\\'/\\').pop())'; };</script>",
        )],
    );
    assert_eq!(run(&url), "in the page index.html");
}

/// `scrollIntoView` through the renderer: an element in an iframe below
/// the fold brought into view in its frame's viewport (as far as it
/// scrolls) and then in the parent's, where the element is now; each
/// frame's `scroll` event, the parent's first.
#[test]
fn scroll_into_view_through_frames() {
    use std::collections::HashMap;

    use net::test_server::{Route, TestServer};
    let server = TestServer::start(HashMap::from([
        (
            "/index.html".to_owned(),
            Route::ok(
                "text/html",
                "<!DOCTYPE html><style>body { margin: 0 } iframe { display: block; border: 0; width: 100px; height: 100px }\
                 </style><div style='height: 1000px'></div><iframe src=/frame.html></iframe>\
                 <div style='height: 1000px'></div><script>const log = [];\
                 addEventListener('scroll', () => log.push('parent'));\
                 function scrolled(own) { log.push(own, scrollY); testRunner.reportHarnessResults(log.join()); }</script>",
            ),
        ),
        (
            "/frame.html".to_owned(),
            Route::ok(
                "text/html",
                "<!DOCTYPE html><style>body { margin: 0 }</style><div style='height: 500px'></div>\
                 <div id=t style='height: 10px'></div><div style='height: 50px'></div>\
                 <script>onscroll = () => parent.scrolled(scrollY);\
                 onload = () => document.getElementById('t').scrollIntoView();</script>",
            ),
        ),
    ]))
    .expect("the test server starts");
    // The frame scrolled as far as it goes (the target 40 from its top),
    // the target at the parent's top.
    assert_eq!(run(&server.url("/index.html")), "parent,460,1040");
}

/// A `<link>`'s sheet as script sees it: its `href`, its rules; another
/// origin's, fetched `no-cors`, not origin-clean (its rules a
/// `SecurityError`), with `crossorigin` by CORS, which allows it (its
/// rules readable) or doesn't (a network error from another origin: not
/// origin-clean either); an edit of a `<style>`'s sheet drawn.
#[test]
fn link_sheets_from_script() {
    use std::collections::HashMap;

    use net::test_server::{Route, TestServer};
    let server = TestServer::start(HashMap::from([
        (
            "/index.html".to_owned(),
            Route::ok(
                "text/html",
                "<!doctype html><html class=reftest-wait><style id=s>body { margin: 0 }</style>\
                 <link rel=stylesheet href=/a.css><div style='height: 20px'></div>\
                 <script>const log = []; const s = document.styleSheets[0];\
                 const own = document.styleSheets[1];\
                 log.push(own.href.split('/').pop(), own.cssRules[0].cssText, own.ownerNode.localName);\
                 s.insertRule('div { background: green }', 1);\
                 const other = own.href.replace('127.0.0.1', 'localhost');\
                 const read = []; let pending = 3;\
                 [[other, false], [other, true], [other.replace('a.css', 'b.css'), true]].forEach(([href, cors], at) => {\
                   const link = document.createElement('link'); link.rel = 'stylesheet'; link.href = href;\
                   if (cors) link.crossOrigin = '';\
                   link.onload = link.onerror = (event) => {\
                     try { read[at] = event.type + ' ' + link.sheet.cssRules.length; }\
                     catch (e) { read[at] = event.type + ' ' + e.name; }\
                     if (--pending === 0) { testRunner.reportHarnessResults(log.concat(read).join());\
                       document.documentElement.removeAttribute('class'); }\
                   };\
                   document.head.appendChild(link);\
                 });</script>",
            ),
        ),
        (
            "/a.css".to_owned(),
            sheet("p { color: blue }", 0).with_header("Access-Control-Allow-Origin", "*"),
        ),
        // Not for CORS: a network error by CORS.
        ("/b.css".to_owned(), sheet("p {}", 0)),
    ]))
    .expect("the test server starts");
    assert_eq!(
        run(&server.url("/index.html")),
        "a.css,p { color: blue; },link,load SecurityError,load 1,error SecurityError"
    );
    let frame = headless::render_after_input(
        &server.url("/index.html"),
        gfx::Size::new(100, 100),
        1.0,
        true,
        &[],
    )
    .expect("drawn");
    assert_eq!(pixel(&frame, 10, 10), GREEN, "the rule script put in");
}

/// A `<link>`'s `load`, or `error` for one that failed, as its sheet
/// comes; the window's `load` after them. (Its handlers in attributes: a
/// script after the links waits for their sheets.)
#[test]
fn a_links_events_come_before_the_windows_load() {
    use std::collections::HashMap;

    use net::test_server::{Route, TestServer};
    // CSS, but an error status: no sheet.
    let mut gone = Route::ok("text/css", "p {}");
    gone.status = 404;
    gone.delay = Duration::from_millis(200);
    let server = TestServer::start(HashMap::from([
        (
            "/index.html".to_owned(),
            Route::ok(
                "text/html",
                "<!doctype html><script>const log = []; window.addEventListener('load', () => {\
                 log.push('window'); testRunner.reportHarnessResults(log.join()) });</script>\
                 <link rel=stylesheet href=/a.css onload=\"log.push('load a')\" onerror=\"log.push('error a')\">\
                 <link rel=stylesheet href=/gone.css onload=\"log.push('load b')\" onerror=\"log.push('error b')\">",
            ),
        ),
        ("/a.css".to_owned(), sheet("p {}", 100)),
        ("/gone.css".to_owned(), gone),
    ]))
    .expect("the test server starts");
    assert_eq!(run(&server.url("/index.html")), "load a,error b,window");
}

/// A script whose response is an HTTP error (404 with a body) doesn't run;
/// the parser goes on after it.
#[test]
fn a_script_that_failed_doesnt_run() {
    use std::collections::HashMap;

    use net::test_server::{Route, TestServer};
    let server = TestServer::start(HashMap::from([
        (
            "/index.html".to_owned(),
            Route::ok(
                "text/html",
                "<script>var log = [];</script><script src=\"/gone.js\"></script>\
                 <script>testRunner.reportHarnessResults(log.join() || 'none')</script>",
            ),
        ),
        ("/gone.js".to_owned(), Route::new(404, "log.push('ran');")),
    ]))
    .expect("the test server starts");
    assert_eq!(run(&server.url("/index.html")), "none");
}

/// WPT's real `testharness.js` on a page of the DOM, end to end: the
/// document's readiness while it parses and at `load`, an async test that
/// waits for the window's `load`, one for a timer, a promise test resolved
/// from `queueMicrotask`, and a failing one; the harness completes once
/// the window has loaded, and a report script hands its results to
/// `testRunner` (as content_shell's `testharnessreport.js`).
#[test]
fn testharness_runs() {
    let harness = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../third_party/wpt/resources/testharness.js"),
    )
    .expect("WPT's testharness.js");
    let report = "setup({output: false}); \
                  add_completion_callback((tests, status) => testRunner.reportHarnessResults( \
                    tests.map(t => t.name + ':' + t.status).join() + '|' + status.status));";
    let page = r#"<!DOCTYPE html>
<title>smoke</title>
<script src="testharness.js"></script>
<script src="report.js"></script>
<script>
test(() => assert_equals(document.readyState, 'loading'), 'loading');
async_test(t => {
  window.addEventListener('load', t.step_func_done(() => assert_equals(document.readyState, 'complete')));
}, 'load');
async_test(t => { setTimeout(t.step_func_done(), 10); }, 'timeout');
promise_test(() => new Promise(resolve => queueMicrotask(resolve)), 'microtask');
test(() => assert_true(false), 'fails');
</script>
"#;
    let url = site(
        "testharness",
        &[
            ("index.html", page),
            ("testharness.js", &harness),
            ("report.js", report),
        ],
    );
    // testharness's statuses: PASS 0, FAIL 1; the harness's OK 0.
    assert_eq!(
        run(&url),
        "loading:0,load:0,timeout:0,microtask:0,fails:1|0"
    );
}

/// What the parser inserts reaches the mutation observers before the next
/// script runs: the tree builder's microtask checkpoint at a script's end
/// tag.
#[test]
fn the_parsers_insertions_are_delivered_before_the_next_script() {
    let page = r#"<!DOCTYPE html>
<script>
var log = [];
new MutationObserver(records => log.push(records.map(r => r.addedNodes[0].nodeName).join(' ')))
  .observe(document, { childList: true, subtree: true });
</script><p></p><script>log.push('script'); testRunner.reportHarnessResults(log.join())</script>
"#;
    let url = site("observers", &[("index.html", page)]);
    assert_eq!(
        run(&url),
        "BODY P SCRIPT #text,script",
        "the body the `<p>` makes first"
    );
}

/// Script reads the layout, brought up to date as it asks (a style it just
/// changed is laid out first): elements' rects, boxes and offsets, what is
/// at a point, the viewport (headless: 800×600 at 1x), its size before
/// anything is laid out too.
#[test]
fn script_reads_the_layout() {
    let page = r#"<!DOCTYPE html>
<script>const early = [innerWidth, innerHeight, devicePixelRatio];</script>
<style>html, body { margin: 0 }</style>
<div id="box" style="margin-left: 10px; width: 100px; height: 50px; border: 2px solid; padding: 3px"></div>
<div id="outer" style="position: relative; left: 4px; width: 60px; height: 30px; overflow: hidden">
  <div id="inner" style="margin-top: 5px; width: 200px; height: 80px"></div>
</div>
<div id="fixed" style="position: fixed; top: 0; left: 0; width: 1px; height: 1px"></div>
<div id="none" style="display: none"></div>
<script>
const log = [];
addEventListener('error', e => testRunner.reportHarnessResults(e.message + ' after ' + log.join()));
const rect = r => [r.x, r.y, r.width, r.height, r.top, r.right, r.bottom, r.left].join(' ');
const box = document.getElementById('box');
const b = box.getBoundingClientRect();
log.push(rect(b), b instanceof DOMRect && b instanceof DOMRectReadOnly, b !== box.getBoundingClientRect());
b.x = NaN;
log.push(b.x, b.left);
const rects = box.getClientRects();
log.push(rects.length, rects[0].width, rects.item(1), rects instanceof DOMRectList);
log.push(document.getElementById('none').getClientRects().length,
         rect(document.createElement('div').getBoundingClientRect()));
log.push(box.offsetLeft, box.offsetTop, box.offsetWidth, box.offsetHeight, box.offsetParent.nodeName);
log.push(box.clientTop, box.clientLeft, box.clientWidth, box.clientHeight);
const outer = document.getElementById('outer'), inner = document.getElementById('inner');
log.push(inner.offsetParent.id, inner.offsetLeft, inner.offsetTop, outer.offsetLeft);
log.push(outer.scrollWidth, outer.scrollHeight, outer.scrollTop, outer.scrollLeft);
log.push(document.getElementById('fixed').offsetParent, document.querySelector('body').offsetParent,
         document.getElementById('none').offsetParent, document.getElementById('none').offsetWidth);
box.style.width = '20px';
log.push(box.offsetWidth, box.getBoundingClientRect().width);
log.push(document.documentElement.clientWidth, document.documentElement.clientHeight,
         document.scrollingElement === document.documentElement);
log.push(document.elementFromPoint(15, 5).id, document.elementFromPoint(700, 500).nodeName,
         document.elementFromPoint(-1, 5), document.elementFromPoint(800, 5));
try { document.elementFromPoint(NaN, 0); } catch (e) { log.push(e.name); }
log.push(innerWidth, innerHeight, devicePixelRatio, scrollX, scrollY, pageXOffset, pageYOffset, early);
testRunner.reportHarnessResults(log.join());
</script>
"#;
    let url = site("geometry", &[("index.html", page)]);
    assert_eq!(
        run(&url),
        [
            "10 0 110 60 0 120 60 10,true,true",
            "NaN,NaN",
            "1,110,,true",
            "0,0 0 0 0 0 0 0 0",
            "10,0,110,60,BODY",
            "2,2,106,56",
            "outer,0,5,4",
            "200,85,0,0",
            ",,,0",
            "30,30",
            "800,600,true",
            "box,HTML,,",
            "TypeError",
            "800,600,1,0,0,0,0,800,600,1",
        ]
        .join(",")
    );
}

/// The mouse on the DOM's document (headless input once it is drawn;
/// a move waits for the next frame, so each goes just before a press,
/// which dispatches it first): the boundary events and their related
/// targets as the pointer goes from one element to another, `mousemove`,
/// a click (and the second's `dblclick`), the secondary button's
/// `contextmenu` and `auxclick`; `:hover` and `:active` matched, and a
/// `:hover` rule laid out; the target's offsets; a microtask run before
/// the next event.
#[test]
fn the_mouse() {
    use protocol::input::{InputEvent, Modifiers, MouseButton};
    let page = r#"<!DOCTYPE html>
<style>
html, body { margin: 0 }
#a { width: 100px; height: 100px }
#b { width: 50px; height: 50px }
#b:hover { width: 60px }
#c { width: 100px; height: 100px; border-left: 5px solid; border-top: 3px solid }
</style>
<div id="a"><div id="b"></div></div><div id="c"></div>
<script>
const log = [];
addEventListener('error', e => testRunner.reportHarnessResults(e.message + ' after ' + log.join('\n')));
const [a, b, c] = ['a', 'b', 'c'].map(id => document.getElementById(id));
const body = document.querySelector('body');
const name = node => node ? (node.id || node.nodeName) : '-';
// The secondary button's `auxclick` and `contextmenu`: the last event, in
// either order (Windows sends `contextmenu` as the button goes up).
let secondary = 0;
for (const type of ['mouseover', 'mouseout', 'mouseenter', 'mouseleave', 'mousemove', 'mousedown', 'mouseup', 'click',
                    'dblclick', 'auxclick', 'contextmenu']) {
  document.addEventListener(type, e => {
    let entry = type + ' ' + name(e.target);
    if (type.startsWith('mouseo') || type.endsWith('enter') || type.endsWith('leave')) entry += '<' + name(e.relatedTarget);
    if (type == 'mousemove') entry += ' ' + [e.clientX, e.clientY, e.movementX, e.offsetX, e.offsetY, e.pageX, b.offsetWidth,
                                             b.matches(':hover'), a.matches(':hover')].join(' ');
    // Run before the next event: a checkpoint after each.
    if (type == 'mousedown') Promise.resolve().then(() => log.push('micro'));
    if (type == 'mousedown') entry += ' ' + [e.button, e.buttons, e.detail, e.target.matches(':active'), body.matches(':active'),
                                             e.target.matches(':hover'), a.matches(':active')].join(' ');
    if (type == 'click' || type == 'dblclick') entry += ' ' + e.detail;
    if (type == 'mouseup') entry += ' ' + e.target.matches(':active');
    log.push(entry);
    if ((type == 'auxclick' || type == 'contextmenu') && ++secondary == 2) testRunner.reportHarnessResults(log.join('\n') + '\n' + (e instanceof MouseEvent && e.isTrusted && e.view === window));
  }, true);
}
</script>
"#;
    let url = site("mouse", &[("index.html", page)]);
    let move_to = |x, y| InputEvent::MouseMove {
        x,
        y,
        buttons: 0,
        modifiers: Modifiers::NONE,
    };
    let click = |x, y, button: MouseButton, count| {
        [
            InputEvent::MouseDown {
                x,
                y,
                button,
                buttons: button.buttons_bit(),
                modifiers: Modifiers::NONE,
                click_count: count,
            },
            InputEvent::MouseUp {
                x,
                y,
                button,
                buttons: 0,
                modifiers: Modifiers::NONE,
                click_count: count,
            },
        ]
    };
    let mut input = vec![move_to(10.0, 10.0)];
    input.extend(click(10.0, 10.0, MouseButton::Left, 1));
    input.push(move_to(75.0, 75.0));
    input.extend(click(75.0, 75.0, MouseButton::Left, 2));
    input.push(move_to(10.0, 150.0));
    input.extend(click(10.0, 150.0, MouseButton::Right, 1));
    let report = headless::run_testharness_after_input(
        &url,
        gfx::Size::new(200, 300),
        &input,
        Duration::from_secs(30),
    )
    .expect("the page reports");
    // Blink's `WebFrameWidgetImpl`: the context menu as the secondary
    // button goes down, or (Windows) as it goes up.
    let secondary: &[&str] = if cfg!(windows) {
        &["mouseup c false", "auxclick c", "contextmenu c"]
    } else {
        &["contextmenu c", "mouseup c false", "auxclick c"]
    };
    let expected = [
        "mouseover b<-",
        "mouseenter #document<-",
        "mouseenter HTML<-",
        "mouseenter BODY<-",
        "mouseenter a<-",
        "mouseenter b<-",
        "mousemove b 10 10 0 10 10 10 60 true true",
        "mousedown b 0 1 1 true true true true",
        "micro",
        "mouseup b false",
        "click b 1",
        "mouseout b<a",
        "mouseleave b<a",
        "mouseover a<b",
        "mousemove a 75 75 65 75 75 75 50 false true",
        "mousedown a 0 1 2 true true true true",
        "micro",
        "mouseup a false",
        "click a 2",
        "dblclick a 2",
        "mouseout a<c",
        "mouseleave a<c",
        "mouseover c<a",
        "mouseenter c<a",
        "mousemove c 10 150 -65 5 47 10 50 false false",
        "mousedown c 2 2 1 true true true false",
        "micro",
    ]
    .iter()
    .chain(secondary)
    .chain(&["true"])
    .copied()
    .collect::<Vec<_>>();
    assert_eq!(report, expected.join("\n"));
}

/// What the mouse changed is drawn: a `:hover` rule, no script run. After
/// a press and release: a move's ack names no frame (it goes as the move is
/// dispatched), a press's the frame its handling asked for.
#[test]
fn hover_is_drawn() {
    let page = r#"<!DOCTYPE html>
<style>html, body { margin: 0 } div { width: 100px; height: 100px; background: white } div:hover { background: green }</style>
<div></div>
"#;
    let url = site("hover-drawn", &[("index.html", page)]);
    let before = headless::render_after_input(&url, gfx::Size::new(200, 200), 1.0, true, &[])
        .expect("the page renders");
    assert_eq!(pixel(&before, 50, 50), WHITE);
    use protocol::input::{InputEvent, Modifiers, MouseButton};
    let press = |down| {
        let (button, modifiers, click_count) = (MouseButton::Left, Modifiers::NONE, 1);
        if down {
            InputEvent::MouseDown {
                x: 50.0,
                y: 50.0,
                button,
                buttons: 1,
                modifiers,
                click_count,
            }
        } else {
            InputEvent::MouseUp {
                x: 50.0,
                y: 50.0,
                button,
                buttons: 0,
                modifiers,
                click_count,
            }
        }
    };
    let input = [press(true), press(false)];
    let after = headless::render_after_input(&url, gfx::Size::new(200, 200), 1.0, true, &input)
        .expect("the page renders");
    assert_eq!(pixel(&after, 50, 50), GREEN);
}

/// Keys on the DOM's document (headless input): `keydown`, `keypress`
/// for one that types, `keyup`, at the body (no focus yet), with the
/// platform's key, code and codes; a `keydown` canceled types nothing.
#[test]
fn keys() {
    use protocol::input::{InputEvent, KeyboardEvent, Modifiers};
    let page = r#"<!DOCTYPE html>
<script>
const log = [];
addEventListener('error', e => testRunner.reportHarnessResults(e.message + ' after ' + log.join('\n')));
for (const type of ['keydown', 'keypress', 'keyup']) {
  document.addEventListener(type, e => {
    log.push([type, e.target.nodeName, e.key, e.code, e.keyCode, e.charCode, e.shiftKey, e.isTrusted, e instanceof KeyboardEvent, e.view === window].join(' '));
    if (type == 'keydown' && e.key == 'b') e.preventDefault();
    if (type == 'keyup' && e.key == 'b') testRunner.reportHarnessResults(log.join('\n'));
  });
}
</script>
<body></body>
"#;
    let url = site("keys", &[("index.html", page)]);
    let key = |down, key: &str, code: &str, key_code, text: &str, shift| {
        InputEvent::Key(KeyboardEvent {
            down,
            key: key.to_owned(),
            code: code.to_owned(),
            key_code,
            text: text.to_owned(),
            location: 0,
            repeat: false,
            modifiers: Modifiers {
                shift,
                ..Modifiers::NONE
            },
        })
    };
    let input = [
        key(true, "A", "KeyA", 65, "A", true),
        key(false, "A", "KeyA", 65, "", true),
        key(true, "b", "KeyB", 66, "b", false),
        key(false, "b", "KeyB", 66, "", false),
    ];
    let report = headless::run_testharness_after_input(
        &url,
        gfx::Size::new(100, 100),
        &input,
        Duration::from_secs(30),
    )
    .expect("the page reports");
    assert_eq!(
        report,
        [
            "keydown BODY A KeyA 65 0 true true true true",
            "keypress BODY A KeyA 65 65 true true true true",
            "keyup BODY A KeyA 65 0 true true true true",
            "keydown BODY b KeyB 66 0 false true true true",
            "keyup BODY b KeyB 66 0 false true true true",
        ]
        .join("\n")
    );
}

/// Animation frame callbacks on the DOM: each frame's before its
/// rendering, the time their argument, `this` undefined, one cancelled
/// not run, one that throws reported without stopping the others, one
/// requested in a callback the next frame's, later.
#[test]
fn animation_frames() {
    let page = r#"<!DOCTYPE html>
<div style="width: 10px; height: 10px"></div>
<script>
const log = [];
addEventListener('error', e => log.push('error ' + e.message.split(': ').pop()));
const div = document.querySelector('div');
let first;
requestAnimationFrame(function (time) {
  'use strict';
  log.push(typeof time, this === undefined, time > 0);
  first = time;
  div.style.width = '50px';
  requestAnimationFrame(next => {
    log.push(next > first, div.offsetWidth);
    testRunner.reportHarnessResults(log.join());
  });
});
cancelAnimationFrame(requestAnimationFrame(() => log.push('cancelled')));
requestAnimationFrame(() => { throw new Error('thrown'); });
requestAnimationFrame(() => log.push('after'));
</script>
"#;
    let url = site("animation-frames", &[("index.html", page)]);
    assert_eq!(run(&url), "number,true,true,error thrown,after,true,50");
}

/// What an animation frame callback changes is drawn: `reftest-wait` taken
/// off in the callback, the frame after shows its color.
#[test]
fn an_animation_frame_is_drawn() {
    let page = r#"<!DOCTYPE html>
<html class="reftest-wait">
<style>html, body { margin: 0 } div { width: 100px; height: 100px; background: white }</style>
<div></div>
<script>
requestAnimationFrame(() => {
  document.querySelector('div').style.background = 'green';
  document.documentElement.className = '';
});
</script>
"#;
    let url = site("animation-frame-drawn", &[("index.html", page)]);
    let frame = headless::render_after_input(&url, gfx::Size::new(200, 200), 1.0, true, &[])
        .expect("the page renders");
    assert_eq!(pixel(&frame, 50, 50), GREEN);
}

/// Focus on the DOM (the page has the system's focus, as a headless
/// tab does): `focus()` and its events, `activeElement`, a `:focus` rule
/// laid out, `hasFocus()`; the fixup after the focused element's box
/// went (the next frame's), with `blur`; one taken out loses it with none.
#[test]
fn focus() {
    let page = r#"<!DOCTYPE html>
<style>div { width: 10px; height: 10px } div:focus { width: 30px } p:focus-within { color: green }</style>
<p><div id="a" tabindex="0"></div></p><div id="b" tabindex="-1"></div>
<script>
const log = [];
addEventListener('error', e => testRunner.reportHarnessResults(e.message + ' after ' + log.join()));
for (const type of ['focus', 'blur', 'focusin', 'focusout'])
  document.addEventListener(type, e => log.push(type + ' ' + e.target.id + '<' + (e.relatedTarget ? e.relatedTarget.id : '')
                                                + ' ' + (e instanceof FocusEvent)), true);
const [a, b] = [document.getElementById('a'), document.getElementById('b')];
addEventListener('load', () => {
  log.push(document.hasFocus(), document.activeElement.nodeName);
  a.focus();
  log.push(document.activeElement.id, a.offsetWidth, a.matches(':focus'), b.matches(':focus'));
  b.focus();
  b.style.display = 'none';
  requestAnimationFrame(() => requestAnimationFrame(() => {
    log.push(document.activeElement.nodeName);
    a.focus();
    a.parentNode.removeChild(a);
    log.push(document.activeElement.nodeName);
    testRunner.reportHarnessResults(log.join());
  }));
});
</script>
"#;
    let url = site("focus", &[("index.html", page)]);
    assert_eq!(
        run(&url),
        [
            "true",
            "BODY",
            "focus a< true",
            "focusin a< true",
            "a",
            "30",
            "true",
            "false",
            "blur a<b true",
            "focusout a<b true",
            "focus b<a true",
            "focusin b<a true",
            "blur b< true",
            "focusout b< true",
            "BODY",
            "focus a< true",
            "focusin a< true",
            "BODY",
        ]
        .join(",")
    );
}

/// Tab, keys and the mouse move the focus on the DOM (headless input):
/// Tab through the page's order, the keys going to the focused element, a
/// press focusing what it went down on (without a ring), and away from
/// anything that can take it to the viewport.
#[test]
fn tab_and_the_mouse_move_the_focus() {
    use protocol::input::{InputEvent, KeyboardEvent, Modifiers, MouseButton};
    let page = r##"<!DOCTYPE html>
<style>html, body { margin: 0 } div { width: 100px; height: 50px }</style>
<div id="a" tabindex="0"></div><a id="link" href="#">link</a><div id="plain"></div><div id="b" tabindex="1"></div>
<script>
const log = [];
addEventListener('error', e => testRunner.reportHarnessResults(e.message + ' after ' + log.join()));
document.addEventListener('keydown', e => log.push(e.key + '@' + e.target.id));
document.addEventListener('focusin', e => log.push('focus ' + e.target.id + ' ' + e.target.matches(':focus-visible')));
document.addEventListener('mouseup', e => {
  if (e.target.id == 'plain') {
    log.push('active ' + document.activeElement.nodeName);
    testRunner.reportHarnessResults(log.join());
  }
});
</script>
"##;
    let url = site("tab", &[("index.html", page)]);
    let key = |name: &str, down| {
        InputEvent::Key(KeyboardEvent {
            down,
            key: name.to_owned(),
            code: name.to_owned(),
            key_code: 9,
            text: String::new(),
            location: 0,
            repeat: false,
            modifiers: Modifiers::NONE,
        })
    };
    let press = |y: f32, down| {
        if down {
            InputEvent::MouseDown {
                x: 10.0,
                y,
                button: MouseButton::Left,
                buttons: 1,
                modifiers: Modifiers::NONE,
                click_count: 1,
            }
        } else {
            InputEvent::MouseUp {
                x: 10.0,
                y,
                button: MouseButton::Left,
                buttons: 0,
                modifiers: Modifiers::NONE,
                click_count: 1,
            }
        }
    };
    let input = [
        key("Tab", true),
        key("Tab", false),
        key("Tab", true),
        key("Tab", false),
        // The link, below `a`; then `plain`, below it.
        press(60.0, true),
        press(60.0, false),
        press(90.0, true),
        press(90.0, false),
    ];
    let report = headless::run_testharness_after_input(
        &url,
        gfx::Size::new(200, 300),
        &input,
        Duration::from_secs(30),
    )
    .expect("the page reports");
    assert_eq!(
        report,
        [
            "Tab@",
            "focus b true",
            "Tab@b",
            "focus a true",
            "focus link false",
            "active BODY"
        ]
        .join(",")
    );
}

/// A script `body` the test server sends after `delay` milliseconds.
fn script(body: &str, delay: u64) -> net::test_server::Route {
    let mut route = net::test_server::Route::ok("text/javascript", body);
    route.delay = Duration::from_millis(delay);
    route
}

/// The parser's scripts in their turns: a parser-blocking one before what
/// follows it, the deferred ones in order after parsing (one fetched while
/// the parser waited, a slow one, one that came before it) and before
/// `DOMContentLoaded`, an `async` one as soon as it
/// came (after them, being slower); the window's `load` after all.
#[test]
fn the_parsers_external_scripts_run_in_their_turns() {
    use std::collections::HashMap;

    use net::test_server::TestServer;
    let server = TestServer::start(HashMap::from([
        (
            "/index.html".to_owned(),
            net::test_server::Route::ok(
                "text/html",
                "<!doctype html><script>var log = [];\
                 document.addEventListener('DOMContentLoaded', () => log.push('dcl ' + document.readyState));\
                 window.addEventListener('load', () => { log.push('load'); testRunner.reportHarnessResults(log.join()) });\
                 </script>\
                 <script src=/first-defer.js defer></script>\
                 <script src=/slow-defer.js defer></script><script src=/fast-defer.js defer></script>\
                 <script src=/async.js async></script><script src=/blocking.js></script>\
                 <script>log.push('inline ' + document.readyState)</script>",
            ),
        ),
        ("/first-defer.js".to_owned(), script("log.push('first defer')", 0)),
        ("/slow-defer.js".to_owned(), script("log.push('slow defer')", 300)),
        ("/fast-defer.js".to_owned(), script("log.push('fast defer')", 0)),
        ("/async.js".to_owned(), script("log.push('async')", 900)),
        ("/blocking.js".to_owned(), script("log.push('blocking')", 100)),
    ]))
    .expect("the test server starts");
    assert_eq!(
        run(&server.url("/index.html")),
        "blocking,inline loading,first defer,slow defer,fast defer,dcl interactive,async,load"
    );
}

/// The scripts script inserts: as soon as each came (an async one before
/// a slower in-order one), or in order (`async = false`: the slow first);
/// `load` at each, `error` at one that didn't load (404) or isn't a
/// script's MIME type; the window's `load` waits for them.
#[test]
fn inserted_external_scripts_run_in_their_turns() {
    use std::collections::HashMap;

    use net::test_server::{Route, TestServer};
    let mut gone = Route::ok("text/javascript", "log.push('never')");
    gone.status = 404;
    gone.delay = Duration::from_millis(600);
    let mut image = Route::ok("image/png", "log.push('never')");
    image.delay = Duration::from_millis(700);
    let server = TestServer::start(HashMap::from([
        (
            "/index.html".to_owned(),
            Route::ok(
                "text/html",
                "<!doctype html><script>var log = [];\
                 window.addEventListener('load', () => { log.push('load'); testRunner.reportHarnessResults(log.join()) });\
                 for (const [name, async] of [['slow', false], ['fast', false], ['async', true], ['gone', true], ['image', true]]) {\
                   const s = document.createElement('script'); s.src = '/' + name + '.js'; s.async = async;\
                   s.onload = () => log.push('load ' + name); s.onerror = () => log.push('error ' + name);\
                   document.head.appendChild(s);\
                 }</script>",
            ),
        ),
        ("/slow.js".to_owned(), script("log.push('slow')", 400)),
        ("/fast.js".to_owned(), script("log.push('fast')", 0)),
        ("/async.js".to_owned(), script("log.push('async')", 100)),
        ("/gone.js".to_owned(), gone),
        ("/image.js".to_owned(), image),
    ]))
    .expect("the test server starts");
    assert_eq!(
        run(&server.url("/index.html")),
        "async,load async,slow,load slow,fast,load fast,error gone,error image,load"
    );
}

/// A stylesheet the parser inserted blocks its scripts, inline ones too,
/// till it is in: the script after it runs after its `load`.
#[test]
fn a_parsers_stylesheet_blocks_its_scripts() {
    use std::collections::HashMap;

    use net::test_server::{Route, TestServer};
    let server = TestServer::start(HashMap::from([
        (
            "/index.html".to_owned(),
            Route::ok(
                "text/html",
                "<!doctype html><script>var log = [];</script>\
                 <link rel=stylesheet href=/a.css onload=\"log.push('sheet')\">\
                 <script>log.push('inline'); testRunner.reportHarnessResults(log.join())</script>",
            ),
        ),
        ("/a.css".to_owned(), sheet("p {}", 300)),
    ]))
    .expect("the test server starts");
    assert_eq!(run(&server.url("/index.html")), "sheet,inline");
}

/// A response that says no charset (and has no BOM) is decoded by the
/// `charset` attribute's encoding, else the document's; another origin's
/// errors, fetched `no-cors`, are muted (`Script error.`); with
/// `crossorigin` it goes by CORS, which this server doesn't allow: an
/// `error` at its element.
#[test]
fn an_external_scripts_charset_and_muted_errors() {
    use std::collections::HashMap;

    use net::test_server::{Route, TestServer};
    let server = TestServer::start(HashMap::from([
        (
            "/index.html".to_owned(),
            Route::ok(
                "text/html; charset=utf-8",
                "<!doctype html><script>var log = [];\
                 window.onerror = (message, filename, line) => log.push(message + ' ' + filename.split('/').pop() + ' ' + line);\
                 window.addEventListener('load', () => testRunner.reportHarnessResults(log.join()));</script>\
                 <script src=/latin1.js charset=windows-1252></script>\
                 <script src=/latin1.js></script>\
                 <script>const other = location.href.replace('127.0.0.1', 'localhost').replace('index.html', 'latin1.js');\
                 for (const cors of [false, true]) {\
                   const s = document.createElement('script'); s.src = other; s.async = false;\
                   if (cors) { s.crossOrigin = ''; s.onerror = () => log.push('cors error'); }\
                   document.head.appendChild(s);\
                 }</script>",
            ),
        ),
        ("/latin1.js".to_owned(), Route::ok("text/javascript", b"throw new Error('\xe9');".to_vec())),
    ]))
    .expect("the test server starts");
    assert_eq!(
        run(&server.url("/index.html")),
        "Uncaught Error: \u{e9} latin1.js 1,Uncaught Error: \u{fffd} latin1.js 1,Script error.  0,cors error"
    );
}

/// At a script's end tag the parser performs a microtask checkpoint before
/// it prepares the script: a mutation observer that heard of its insertion
/// changes it first (here, to a type that doesn't run).
#[test]
fn the_parsers_checkpoint_comes_before_it_prepares_a_script() {
    let url = site(
        "checkpoint",
        &[(
            "index.html",
            "<!DOCTYPE html><script>var log = [];\
             new MutationObserver(records => { for (const record of records) for (const node of record.addedNodes)\
               if (node.id === 'changed') node.type = 'text/plain'; }).observe(document, {childList: true, subtree: true});\
             </script><script id=changed>log.push('ran')</script>\
             <script>testRunner.reportHarnessResults(log.join() || 'none')</script>",
        )],
    );
    assert_eq!(run(&url), "none");
}

/// The parser's module scripts: deferred, in document order with the
/// classic deferred ones (an external one after its imports, an inline
/// one, one served as another MIME type than JavaScript's: `error`),
/// before `DOMContentLoaded`; an `async` one as soon as its graph came;
/// one script inserts with an import, as soon as it can; another origin's
/// by CORS: `error` where the server doesn't allow it, run where it does.
/// The window's `load` after all.
#[test]
fn module_scripts_run_in_their_turns() {
    use std::collections::HashMap;

    use net::test_server::{Route, TestServer};
    // Another origin on the same server, mapped to its address rather than
    // `localhost`: where that resolves to `::1` first, which the server
    // doesn't listen on, each request waits out the failed attempt (on
    // Windows a refused connection takes seconds) and comes out of turn.
    let network = net::NetworkContextParams {
        host_resolver_rules: net::HostMappingRules::parse("MAP other.test 127.0.0.1").unwrap(),
        ..Default::default()
    };
    let server = TestServer::start(HashMap::from([
        (
            "/index.html".to_owned(),
            Route::ok(
                "text/html",
                "<!doctype html><script>var log = [];\
                 document.addEventListener('DOMContentLoaded', () => log.push('dcl'));\
                 window.addEventListener('load', () => { log.push('load'); testRunner.reportHarnessResults(log.join()) });\
                 </script>\
                 <script type=module src=/main.mjs></script>\
                 <script src=/classic.js defer></script>\
                 <script type=module>log.push('inline ' + import.meta.url.split('/').pop())</script>\
                 <script type=module async src=/async.mjs></script>\
                 <script type=module src=/plain.mjs onerror=\"log.push('error plain')\"></script>\
                 <script>log.push('parsed ' + document.readyState);\
                 const s = document.createElement('script'); s.type = 'module'; s.src = '/inserted.mjs';\
                 document.head.appendChild(s);\
                 const other = document.createElement('script'); other.type = 'module';\
                 other.src = location.href.replace('127.0.0.1', 'other.test').replace('index.html', 'dep.mjs');\
                 other.onerror = () => log.push('error other');\
                 document.head.appendChild(other);\
                 const allowed = document.createElement('script'); allowed.type = 'module';\
                 allowed.src = location.href.replace('127.0.0.1', 'other.test').replace('index.html', 'cors.mjs');\
                 document.head.appendChild(allowed);</script>",
            ),
        ),
        ("/main.mjs".to_owned(), script("import { name } from './dep.mjs'; log.push('main ' + name);", 200)),
        ("/dep.mjs".to_owned(), script("export const name = 'dep'; log.push('dep');", 100)),
        ("/classic.js".to_owned(), script("log.push('classic defer')", 0)),
        ("/async.mjs".to_owned(), script("log.push('async')", 900)),
        ("/plain.mjs".to_owned(), Route::ok("text/plain", "log.push('never')")),
        ("/inserted.mjs".to_owned(), script("import './dep.mjs'; log.push('inserted')", 600)),
        (
            "/cors.mjs".to_owned(),
            script("log.push('cors')", 750).with_header("Access-Control-Allow-Origin", "*"),
        ),
    ]))
    .expect("the test server starts");
    assert_eq!(
        headless::run_testharness(&server.url("/index.html"), Duration::from_secs(30), network)
            .expect("the page reports"),
        "parsed loading,error other,dep,main dep,classic defer,inline index.html,error plain,dcl,inserted,cors,async,load"
    );
}

/// `import()` through the network: a module's graph fetched, its namespace
/// the promise's; a 404 rejects. A `<link rel=modulepreload>`'s module is
/// fetched once (`load`), and an `import()` of it after takes it from the
/// map. The window's `load` doesn't wait for an `import()`.
#[test]
fn dynamic_imports_and_module_preloads() {
    use std::collections::HashMap;

    use net::test_server::{Route, TestServer};
    let mut gone = Route::ok("text/javascript", "");
    gone.status = 404;
    gone.delay = Duration::from_millis(150);
    let server = TestServer::start(HashMap::from([
        (
            "/index.html".to_owned(),
            Route::ok(
                "text/html",
                "<!doctype html><link rel=modulepreload href=/pre.mjs onload=\"log.push('preloaded')\">\
                 <script>var log = []; window.addEventListener('load', () => log.push('load'));\
                 import('/m.mjs').then(m => log.push('m ' + m.value), e => log.push('m failed ' + e));\
                 import('/gone.mjs').catch(e => log.push(e.name));\
                 setTimeout(() => import('/pre.mjs').then(p => { log.push('pre ' + p.value);\
                   testRunner.reportHarnessResults(log.join()); }), 800);</script>",
            ),
        ),
        ("/m.mjs".to_owned(), script("import { dep } from './dep.mjs'; export const value = dep;", 300)),
        ("/dep.mjs".to_owned(), script("export const dep = 'dep';", 0)),
        ("/pre.mjs".to_owned(), script("export const value = 'pre';", 50)),
        ("/gone.mjs".to_owned(), gone),
    ]))
    .expect("the test server starts");
    let result = run(&server.url("/index.html"));
    assert_eq!(result, "load,preloaded,TypeError,m dep,pre pre");
    let pre = server
        .requests()
        .iter()
        .filter(|request| request.target == "/pre.mjs")
        .count();
    assert_eq!(pre, 1, "the preload's module comes from the map");
}

/// A blob URL `URL.createObjectURL` made loads (an `<img>` of it), one a
/// task made too (its load starts after the browser heard of it); one
/// revoked before the element resolved it doesn't, though the browser hears
/// of the revoke only after the task.
#[test]
fn blob_urls_load_until_revoked() {
    use std::collections::HashMap;

    use net::test_server::{Route, TestServer};
    // Of an origin of its own (a `file:` page's is opaque: no blob URL of
    // it is revoked).
    let page = r#"<!DOCTYPE html><body><script>
const svg = () => new Blob(['<svg xmlns="http://www.w3.org/2000/svg" width="7" height="3"/>'], { type: 'image/svg+xml' });
const first = URL.createObjectURL(svg());
const a = document.body.appendChild(document.createElement('img'));
const log = [];
const done = what => { log.push(what); if (log.length == 2) testRunner.reportHarnessResults(a.naturalWidth + ' ' + log.sort().join(' ')); };
a.onload = () => {
  const second = URL.createObjectURL(svg());
  URL.revokeObjectURL(second);
  const b = document.body.appendChild(document.createElement('img'));
  b.onload = () => done('revoked:load');
  b.onerror = () => done('revoked:error');
  b.src = second;
  const c = document.body.appendChild(document.createElement('img'));
  c.onload = () => done('in-a-task:load');
  c.onerror = () => done('in-a-task:error');
  c.src = URL.createObjectURL(svg());
};
a.onerror = () => testRunner.reportHarnessResults('first error');
a.src = first;
</script>"#;
    let server = TestServer::start(HashMap::from([(
        "/".to_string(),
        Route::ok("text/html", page),
    )]))
    .expect("test server starts");
    assert_eq!(run(&server.url("/")), "7 in-a-task:load revoked:error");
}

/// The DOM's navigation timing: the network's times the browser told
/// at the commit, the document's marked as its events fired, in order; a
/// child frame's of its own load.
#[test]
fn navigation_timing() {
    use std::collections::HashMap;

    use net::test_server::{Route, TestServer};
    let page = r#"<!DOCTYPE html><body><iframe src="/child"></iframe><script>
onload = () => setTimeout(() => {
  const n = performance.getEntriesByType('navigation')[0];
  const order = [n.fetchStart, n.requestStart, n.responseStart, n.responseEnd, n.domInteractive, n.domContentLoadedEventStart,
                 n.domContentLoadedEventEnd, n.domComplete, n.loadEventStart, n.loadEventEnd];
  const child = document.querySelector('iframe').contentWindow.performance.getEntriesByType('navigation')[0];
  testRunner.reportHarnessResults([
    n.name.endsWith('/'), n.type, n.responseStatus, n.contentType, n.decodedBodySize > 0, order.every((t, i) => t > 0 && (i == 0 || t >= order[i - 1])),
    performance.timing.loadEventEnd >= performance.timing.navigationStart, child.name.endsWith('/child'), child.responseEnd > 0,
  ].join(' '));
}, 0);
</script>"#;
    let server = TestServer::start(HashMap::from([
        ("/".to_string(), Route::ok("text/html", page)),
        ("/child".to_string(), Route::ok("text/html", "<p>child")),
    ]))
    .expect("test server starts");
    assert_eq!(
        run(&server.url("/")),
        "true navigate 200 text/html true true true true true"
    );
}

/// The DOM's idle callbacks in the renderer's idle periods: one with
/// time left before the next frame or a long period's 50 ms, those a
/// callback requests in the next period; a timeout's while frames keep
/// the loop from idling.
#[test]
fn idle_callbacks() {
    let page = r#"<!DOCTYPE html><script>
const log = [];
requestIdleCallback(d => {
  const left = d.timeRemaining();
  log.push(`first ${d.didTimeout} ${left > 0 && left <= 50}`);
  requestIdleCallback(d => {
    log.push(`second ${d.didTimeout}`);
    // Frames each 16 ms, and a callback that keeps it busy: its timeout.
    const frame = () => { const start = performance.now(); while (performance.now() - start < 20) {} requestAnimationFrame(frame); };
    requestAnimationFrame(frame);
    requestIdleCallback(d => {
      log.push(`busy ${d.didTimeout ? 'timed out' : d.timeRemaining() <= 16}`);
      testRunner.reportHarnessResults(log.join(', '));
    }, { timeout: 100 });
  });
});
</script>"#;
    let url = site("idle-callbacks", &[("index.html", page)]);
    assert_eq!(run(&url), "first false true, second false, busy timed out");
}

/// No idle period starts while the DOM's timers are due: a chain of
/// busy `setTimeout(0)`s keeps an idle callback waiting until it ends.
#[test]
fn idle_periods_wait_for_the_timers() {
    let page = r#"<!DOCTYPE html><script>
let left = 5;
requestIdleCallback(d => testRunner.reportHarnessResults(`left ${left}, ${d.didTimeout}`));
const busy = () => {
  const start = performance.now();
  while (performance.now() - start < 20) {}
  if (left > 0) { left--; setTimeout(busy, 0); }
};
setTimeout(busy, 0);
</script>"#;
    let url = site("idle-timers", &[("index.html", page)]);
    assert_eq!(run(&url), "left 0, false");
}

/// The DOM's `fetch()` in a whole browser: a response of the server's
/// (its body read), a slow one aborted (its request cancelled, the promise
/// rejected at once), and a blob URL's, before and after it is revoked.
#[test]
fn fetch() {
    use std::collections::HashMap;

    use net::test_server::{Route, TestServer};
    let page = r#"<!DOCTYPE html><script>
(async () => {
  const log = [];
  const r = await fetch('/data', { headers: { 'X-A': '1' } });
  log.push(`${r.status} ${r.type} ${r.headers.get('content-type')} ${JSON.stringify(await r.json())}`);
  const c = new AbortController();
  const slow = fetch('/slow', { signal: c.signal });
  setTimeout(() => c.abort(), 20);
  const started = performance.now();
  await slow.catch(e => log.push(`${e.name} ${performance.now() - started < 1500}`));
  const url = URL.createObjectURL(new Blob(['blob'], { type: 'text/x' }));
  const b = await fetch(url);
  log.push(`${b.headers.get('content-type')} ${await b.text()}`);
  URL.revokeObjectURL(url);
  await fetch(url).catch(e => log.push(`revoked ${e.name}`));
  testRunner.reportHarnessResults(log.join(', '));
})();
</script>"#;
    let server = TestServer::start(HashMap::from([
        ("/".to_string(), Route::ok("text/html", page)),
        (
            "/data".to_string(),
            Route::ok("application/json", r#"{"a": [1]}"#),
        ),
        (
            "/slow".to_string(),
            Route::ok("text/plain", "late").with_delay(Duration::from_secs(3)),
        ),
    ]))
    .expect("test server starts");
    assert_eq!(
        run(&server.url("/")),
        "200 basic application/json {\"a\":[1]}, AbortError true, text/x blob, revoked TypeError"
    );
}
