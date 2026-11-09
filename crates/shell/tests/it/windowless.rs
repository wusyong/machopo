//! Scenarios in a browser window without the platform's window
//! (`shell::windowless`): the scenario runner's commands, against pages
//! written for each test, with Viz's thread, its `BeginFrame`s and real
//! time.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crossbeam_channel::{Receiver, unbounded};
use gfx::{PointF, Size};
use protocol::TabId;
use protocol::automation::{
    AttachToTarget, Bounds, CaptureWindow, Command, DispatchMouseEvent, GetTargetInfo, Message,
    Method, Navigate, QueryElement, SetWindowBounds,
};
use protocol::renderer::RendererHostMsg;
use shell::automation::{Context, Failure};
use shell::automation_session::{AutomationSession, ClientEvent, ClientId, Input, Outbox};
use shell::browser_view;
use shell::browser_window::{self, BrowserWindow};
use shell::omnibox::Clipboard;
use shell::windowless::{self, Options};

/// A directory of its own for `test`, with `pages` (name, markup) in it.
fn pages(test: &str, pages: &[(&str, &str)]) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("machopo-windowless-{}-{test}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (name, markup) in pages {
        std::fs::write(dir.join(name), markup).unwrap();
    }
    dir
}

fn run_in(dir: &Path, scenario: &str) -> Result<(), Failure> {
    run_with(dir, scenario, false)
}

/// [`run_in`], with macOS's keyboard shortcuts if `mac`.
fn run_with(dir: &Path, scenario: &str, mac: bool) -> Result<(), Failure> {
    run_with_options(dir, scenario, mac, &Options::default())
}

fn run_with_options(
    dir: &Path,
    scenario: &str,
    mac: bool,
    options: &Options,
) -> Result<(), Failure> {
    let scenario = shell::scenario::parse(scenario).expect("the scenario parses");
    let context = Context {
        base: dir.to_owned(),
        out: Some(dir.join("out")),
        mac,
    };
    windowless::run(&scenario, &context, options)
}

const BUTTON: &str = "<!DOCTYPE html><style>html, body { margin: 0 } \
    #b { display: block; width: 100px; height: 40px; background: blue }</style>\
    <title>one</title><span id=b onclick='clicked()'></span><p id=out>not yet</p>\
    <script>let clicks = 0; \
    function clicked() { document.getElementById('out').textContent = `clicked ${++clicks}`; }\
    </script>";

#[test]
fn clicks_go_to_the_element_and_waits_see_the_change() {
    let dir = pages("click", &[("one.html", BUTTON)]);
    run_in(
        &dir,
        "navigate one.html\nwait-loaded\nwait-text #out \"not yet\"\n\
         click #b\nwait-text #out \"clicked 1\"\nclick #b\nwait-text #out \"clicked 2\"",
    )
    .unwrap();
}

/// Keys go where the window sends them: F6 to the address field, typing
/// into it, Enter navigating.
#[test]
fn keys_type_in_the_address_field() {
    let dir = pages(
        "keys",
        &[
            ("one.html", BUTTON),
            ("two.html", "<!DOCTYPE html><p id=where>page two</p>"),
        ],
    );
    let two = shell::to_url(&dir.join("two.html").to_string_lossy());
    run_in(
        &dir,
        &format!(
            "navigate one.html\nwait-loaded\nkey F6\ntype \"{two}\"\nkey Enter\n\
             wait-loaded\nwait-text #where \"page two\""
        ),
    )
    .unwrap();
}

/// The page has the system's focus while its web view has the keys: the
/// address field taking them takes it (the window gets `blur`), a click in
/// the page gives it back (`focus`).
#[test]
fn the_address_field_takes_the_pages_focus() {
    let page = "<!DOCTYPE html><p id=out>start</p>\
        <span id=b style='display: block; width: 100px; height: 40px'>b</span>\
        <script>const log = []; for (const type of ['focus', 'blur']) \
        addEventListener(type, e => { if (e.target === window) { log.push(type); \
        document.getElementById('out').textContent = log.join(' ') + ' ' + document.hasFocus(); } });\
        </script>";
    let dir = pages("focus", &[("one.html", page)]);
    run_in(
        &dir,
        "navigate one.html\nwait-loaded\nwait-text #out \"start\"\nkey F6\n\
         wait-text #out \"blur false\"\nclick #b\nwait-text #out \"blur focus true\"",
    )
    .unwrap();
}

/// Tab from the address field goes into the page, to its first focusable
/// element; Tab past its last gives the address field the keys back (the
/// page's window gets `blur`); Shift+Tab from it goes to the page's last.
const TABS: &str = "<!DOCTYPE html><p id=out>start</p><p id=keys></p>\
    <div id=a tabindex=0>a</div><div id=b tabindex=0>b</div>\
    <script>const log = []; const note = name => () => { log.push(name); \
    document.getElementById('out').textContent = log.join(' '); }; \
    document.getElementById('a').addEventListener('focus', note('a')); \
    document.getElementById('b').addEventListener('focus', note('b')); \
    addEventListener('blur', e => { if (e.target === window) note('blur')(); }); \
    const keys = document.getElementById('keys'); \
    addEventListener('keyup', e => keys.textContent += e.key);</script>";

const TAB_SCENARIO: &str = "navigate one.html\nwait-loaded\nkey F6\nwait-text #out \"blur\"\n\
    key Tab\nwait-text #out \"blur a\"\nkey Tab\nwait-text #out \"blur a b\"\n\
    wait-text #keys \"Tab\"\n\
    key Tab\nwait-text #out \"blur a b blur\"\n\
    key shift+Tab\nwait-text #out \"blur a b blur b\"";

/// A key's key-up goes where its key-down went: the address field's keys
/// (F6, the Tab out of it) don't reach the page. Tab past the last element
/// goes to the browser (`TakeFocus`), and Shift+Tab from the address field
/// into the page's last (`SetInitialFocus`).
#[test]
fn tab_goes_between_the_address_field_and_the_page() {
    let dir = pages("tab", &[("one.html", TABS)]);
    run_in(&dir, TAB_SCENARIO).unwrap();
}

/// `click` is a click of its own however soon it comes after the last;
/// `dblclick` clicks twice, the second a double click.
#[test]
fn clicks_and_double_clicks() {
    let page = "<!DOCTYPE html><p id=out></p><div id=b style='height: 50px'>b</div>\
        <script>const out = document.getElementById('out'); \
        const note = e => out.textContent += (out.textContent ? ' ' : '') + e.type + ':' + e.detail; \
        addEventListener('click', note); addEventListener('dblclick', note);</script>";
    let dir = pages("dblclick", &[("one.html", page)]);
    run_in(
        &dir,
        "navigate one.html\nwait-loaded\nclick #b\nclick #b\ndblclick #b\n\
         wait-text #out \"click:1 click:1 click:1 click:2 dblclick:2\"",
    )
    .unwrap();
}

/// A wheel scrolls what is under the pointer; the frames show it.
#[test]
fn a_wheel_scrolls_and_the_frames_differ() {
    let tall = "<!DOCTYPE html><style>html, body { margin: 0 }</style>\
        <div id=top style='height: 300px; background: red'></div>\
        <div style='height: 2000px; background: green'></div>";
    let dir = pages("wheel", &[("tall.html", tall)]);
    run_in(
        &dir,
        "navigate tall.html\nwait-loaded\ncapture top\n\
         wheel #top 0 200\nexpect-differs top",
    )
    .unwrap();
}

/// Within an element's box, only what changes there counts.
#[test]
fn frames_differ_where_an_element_changed() {
    let boxes = "<!DOCTYPE html><style>div { width: 50px; height: 50px; background: red }</style>\
        <div id=a></div><div id=b onclick='this.setAttribute(`style`, `background: green`)'></div>";
    let dir = pages("region", &[("boxes.html", boxes)]);
    let start = "navigate boxes.html\nwait-loaded\ncapture before\nclick #b\n";
    run_in(&dir, &format!("{start}expect-differs before #b")).unwrap();
    // At 2x the box is in device pixels.
    run_in(
        &dir,
        &format!("window scale=2\n{start}expect-differs before #b"),
    )
    .unwrap();
    let failure = run_in(
        &dir,
        &format!("{start}timeout 0.5\nexpect-differs before #a"),
    )
    .unwrap_err();
    assert_eq!(failure.message, "#a still looks as it did at \"before\"");
}

#[test]
fn screenshots_are_saved_at_the_windows_size() {
    let dir = pages("screenshot", &[("one.html", BUTTON)]);
    run_in(
        &dir,
        "navigate one.html\nwait-loaded\nscreenshot first\nresize 300 200\nscreenshot small.png",
    )
    .unwrap();
    let size = |name: &str| {
        let decoder = png::Decoder::new(std::io::BufReader::new(
            std::fs::File::open(dir.join("out").join(name)).unwrap(),
        ));
        let info = decoder.read_info().unwrap();
        (info.info().width, info.info().height)
    };
    assert_eq!(size("first.png"), (800, 600));
    assert_eq!(size("small.png"), (300, 200));
}

/// A `wait-change` right after input compares with the element before the
/// input, which has changed it by the time the page is looked at (`platform`
/// lines between them don't count); any other compares with when it began.
#[test]
fn a_change_the_input_made_is_waited_for() {
    let dir = pages("change", &[("one.html", BUTTON)]);
    let start = "navigate one.html\nwait-loaded\n";
    run_in(&dir, &format!("{start}click #b\nwait-change #out")).unwrap();
    run_in(
        &dir,
        &format!("{start}click #b\nplatform all\nwait-change #out"),
    )
    .unwrap();
    // The short timeout only for the wait meant to fail: with the other
    // tests' pages loading meanwhile, the click's box can take longer.
    for after in ["wait-text #out \"clicked 1\"", "wait-change #out"] {
        let failure = run_in(
            &dir,
            &format!("{start}click #b\n{after}\ntimeout 0.3\nwait-change #out"),
        )
        .unwrap_err();
        assert_eq!(failure.line, 6, "after {after}");
        assert!(
            failure.message.starts_with("#out didn't change"),
            "{failure}"
        );
    }
}

/// A failed check names its line and what it saw instead.
#[test]
fn failures_name_the_line_and_what_was_seen() {
    let dir = pages(
        "failures",
        &[
            ("one.html", BUTTON),
            (
                "far.html",
                "<!DOCTYPE html><style>body { margin: 0 }</style>\
                 <div style='height: 2000px'></div><p id=far style='height: 3000px'>far</p>",
            ),
        ],
    );
    let start = "navigate one.html\nwait-loaded\ntimeout 0.3\n";
    let failure = run_in(&dir, &format!("{start}wait-text #out \"clicked 1\"")).unwrap_err();
    assert_eq!(failure.line, 4);
    assert_eq!(
        failure.message,
        "#out never had the text \"clicked 1\"; it had \"not yet\""
    );
    let failure = run_in(&dir, &format!("{start}wait-text #none \"x\"")).unwrap_err();
    assert_eq!(failure.message, "nothing matched #none");
    let failure = run_in(&dir, &format!("{start}click #none")).unwrap_err();
    assert_eq!(failure.message, "nothing matching #none has a box");
    // Taller than the window: scrolled to, its center is still below it,
    // and no click goes off the page, to nothing.
    let failure = run_in(&dir, "navigate far.html\nwait-loaded\nclick #far").unwrap_err();
    assert_eq!(failure.line, 3);
    assert!(
        failure.message.starts_with("the center of #far, ("),
        "{failure}"
    );
    let failure = run_in(&dir, &format!("{start}wait-change #out")).unwrap_err();
    assert!(
        failure.message.starts_with("#out didn't change"),
        "{failure}"
    );
    // The capture with the default timeout: with the other tests' pages
    // loading meanwhile, the first frame can take longer than the short one.
    let failure = run_in(
        &dir,
        "navigate one.html\nwait-loaded\ncapture a\ntimeout 0.3\nexpect-differs a",
    )
    .unwrap_err();
    assert_eq!(
        (failure.line, failure.message.as_str()),
        (5, "the window still looks as it did at \"a\"")
    );
    let failure = run_in(&dir, &format!("{start}expect-differs b")).unwrap_err();
    assert_eq!(failure.message, "nothing was captured as \"b\"");
}

/// A click at an element below the window scrolls it into view first, as
/// WebDriver's does; in view, it doesn't scroll again.
#[test]
fn a_click_scrolls_its_element_into_view() {
    let far = BUTTON.replace("<span id=b", "<div style='height: 2000px'></div><span id=b");
    let dir = pages("click-scrolls", &[("far.html", &far)]);
    run_in(
        &dir,
        "navigate far.html\nwait-loaded\nclick #b\nwait-text #out \"clicked 1\"\n\
         click #b\nwait-text #out \"clicked 2\"",
    )
    .unwrap();
}

/// The pixels of a screenshot, RGBA, and its width.
fn screenshot(dir: &Path, name: &str) -> (Vec<u8>, u32) {
    let decoder = png::Decoder::new(std::io::BufReader::new(
        std::fs::File::open(dir.join("out").join(format!("{name}.png"))).unwrap(),
    ));
    let mut reader = decoder.read_info().unwrap();
    let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut pixels).unwrap();
    pixels.truncate(info.buffer_size());
    (pixels, info.width)
}

/// `scroll-to` scrolls an element below the window into view, and what is
/// captured after it shows it; one in view already stays where it is.
#[test]
fn scroll_to_brings_an_element_into_view() {
    let page = "<!DOCTYPE html><style>html, body { margin: 0 } div { height: 100px }</style>\
        <div style='background: red'></div><div id=near style='background: lime'></div>\
        <div style='height: 2000px'></div>\
        <div id=far style='height: 2000px; background: blue'></div>";
    let dir = pages("scroll-to", &[("page.html", page)]);
    run_in(
        &dir,
        "navigate page.html\nwait-loaded\nscreenshot top\nscroll-to #near\nscreenshot near\n\
         scroll-to #far\nscreenshot far",
    )
    .unwrap();
    let (top, width) = screenshot(&dir, "top");
    assert_eq!(screenshot(&dir, "near").0, top, "#near was in view");
    // The bottom row of the window: the spacer, then #far.
    let bottom = |pixels: &[u8]| {
        let i = (pixels.len() / 4 - width as usize / 2) * 4;
        pixels[i..i + 3].to_vec()
    };
    assert_eq!(bottom(&top), [255, 255, 255]);
    assert_eq!(bottom(&screenshot(&dir, "far").0), [0, 0, 255]);
}

/// `scroll-to` waits for its element to have a box, as `click` does.
#[test]
fn scroll_to_waits_for_its_element() {
    let page = "<!DOCTYPE html><style>html, body { margin: 0 }</style>\
        <div style='height: 2000px'></div><script>setTimeout(() => {\
        const late = document.createElement('div'); late.id = 'late';\
        late.setAttribute('style', 'height: 2000px; background: blue');\
        document.body.appendChild(late); }, 300);</script>";
    let dir = pages("scroll-to-late", &[("page.html", page)]);
    run_in(
        &dir,
        "navigate page.html\nwait-loaded\nscroll-to #late\nscreenshot late",
    )
    .unwrap();
    let (pixels, width) = screenshot(&dir, "late");
    let i = (pixels.len() / 4 - width as usize / 2) * 4;
    assert_eq!(pixels[i..i + 3], [0, 0, 255]);
}

/// Run from a file, relative paths are the file's; parse errors name their
/// line.
#[test]
fn scenario_files() {
    let dir = pages(
        "files",
        &[
            ("one.html", BUTTON),
            (
                "good.txt",
                "navigate one.html\nwait-loaded\nwait-text #out \"not yet\"",
            ),
            ("bad.txt", "navigate one.html\nfly"),
        ],
    );
    windowless::run_file(&dir.join("good.txt"), None, false, &Options::default()).unwrap();
    let failure =
        windowless::run_file(&dir.join("bad.txt"), None, false, &Options::default()).unwrap_err();
    assert_eq!(failure.to_string(), "line 2: unknown command `fly`");
}

/// An element that appears later is clicked once it has a box.
#[test]
fn a_click_waits_for_its_element() {
    let late = "<!DOCTYPE html><p id=out>waiting</p><script>setTimeout(() => {\
        const b = document.createElement('p');\
        b.setAttribute('id', 'late');\
        b.textContent = 'late';\
        b.addEventListener('click', () => document.getElementById('out').textContent = 'late clicked');\
        document.body.appendChild(b);\
        }, 300);</script>";
    let dir = pages("late", &[("late.html", late)]);
    run_in(
        &dir,
        "navigate late.html\nwait-loaded\nclick #late\nwait-text #out \"late clicked\"",
    )
    .unwrap();
}

/// Modifiers are held for their key only. A key with Ctrl is a shortcut and
/// doesn't reach the page, so Ctrl+End doesn't scroll; the End after it
/// does.
#[test]
fn modifiers_are_held_for_their_key() {
    // Stripes of different heights, so every scroll position looks different.
    let stripes: String = (1..=60)
        .map(|i| {
            format!(
                "<div style='height: {}px; background: {}'></div>",
                20 + i,
                ["red", "blue", "white"][i % 3]
            )
        })
        .collect();
    let tall = format!("<!DOCTYPE html><style>html, body {{ margin: 0 }}</style>{stripes}");
    let dir = pages("modifiers", &[("tall.html", &tall)]);
    let start = "navigate tall.html\nwait-loaded\ncapture top\nkey ctrl+End\n";
    let failure = run_in(&dir, &format!("{start}timeout 0.5\nexpect-differs top")).unwrap_err();
    assert_eq!(
        failure.message,
        "the window still looks as it did at \"top\""
    );
    run_in(&dir, &format!("{start}key End\nexpect-differs top")).unwrap();
}

/// No clipboard: these tests don't copy.
struct NoClipboard;

impl Clipboard for NoClipboard {
    fn read(&mut self) -> Option<String> {
        None
    }
    fn write(&mut self, _: &str) {}
}

/// A browser window without the platform's, for the tests that drive its
/// session themselves.
fn browser() -> (
    BrowserWindow,
    Receiver<(TabId, browser::ContentsEvent)>,
    windowless::ContextLost,
) {
    browser_at(1.0)
}

/// [`browser`], 400×300 physical px at `scale`.
fn browser_at(
    scale: f32,
) -> (
    BrowserWindow,
    Receiver<(TabId, browser::ContentsEvent)>,
    windowless::ContextLost,
) {
    let size = Size::new(400, 300);
    let (host_tx, host_rx) = unbounded();
    let (browser, context_lost) = windowless::start(
        browser_window::Options {
            test_fonts: true,
            automation: true,
            ..Default::default()
        },
        size,
        scale,
        Box::new(NoClipboard),
        Box::new(move |tab, msg| {
            let _ = host_tx.send((tab, msg));
        }),
    )
    .unwrap();
    (browser, host_rx, context_lost)
}

fn command(id: i32, method: Method) -> Command {
    Command {
        id,
        session_id: None,
        method,
    }
}

/// Command `id`'s answer, if the outbox has had it; what is before it is
/// passed over.
fn answer(messages: &Receiver<Message>, id: i32) -> Option<Result<serde_json::Value, String>> {
    messages.try_iter().find_map(|message| match message {
        Message::Response(response) if response.id == id => {
            Some(response.result.map_err(|error| error.message))
        }
        _ => None,
    })
}

/// Whether the outbox has had command `id`'s answer, which succeeded.
fn answered(messages: &Receiver<Message>, id: i32) -> bool {
    messages.try_iter().any(|message| match message {
        Message::Response(response) if response.id == id => {
            assert!(response.result.is_ok(), "{response:?}");
            true
        }
        _ => false,
    })
}

/// `Machopo.waitForLoad` is answered once the navigation's `DidFinishLoad` has
/// come, and at once for one that has.
#[test]
fn waiting_for_a_load_waits_for_did_finish_load() {
    let dir = pages("load", &[("one.html", BUTTON)]);
    let (mut browser, host_rx, _context_lost) = browser();
    let (outbox, messages) = unbounded();
    let mut session = AutomationSession::new(&browser, outbox);
    let url = shell::to_url(&dir.join("one.html").to_string_lossy());
    let navigate = Method::MachopoNavigateFromAddressBar(Navigate { url });
    assert!(
        session
            .command(command(1, navigate), &mut browser)
            .is_none()
    );
    assert!(answered(&messages, 1));
    assert!(
        session
            .command(command(2, Method::MachopoWaitForLoad), &mut browser)
            .is_none()
    );
    loop {
        // Nothing answers it but `DidFinishLoad`, which the session hasn't had.
        assert!(!answered(&messages, 2));
        let (tab, event) = host_rx.recv_timeout(Duration::from_secs(30)).unwrap();
        let Some(msg) = browser.view_mut().on_contents_event(tab, event) else {
            continue;
        };
        session.on_renderer_message(tab, &msg, &browser);
        if let RendererHostMsg::DidFinishLoad { .. } = msg {
            break;
        }
    }
    assert!(answered(&messages, 2));
    session.command(command(3, Method::MachopoWaitForLoad), &mut browser);
    assert!(answered(&messages, 3));
    browser.shutdown();
}

/// `primary` is Cmd with macOS's shortcuts and Ctrl with the others':
/// either way it takes the address field.
#[test]
fn primary_is_the_platforms_shortcut_key() {
    let dir = pages(
        "primary",
        &[
            ("one.html", BUTTON),
            ("two.html", "<!DOCTYPE html><p id=where>page two</p>"),
        ],
    );
    let two = shell::to_url(&dir.join("two.html").to_string_lossy());
    let scenario = format!(
        "navigate one.html\nwait-loaded\nkey primary+l\ntype \"{two}\"\nkey Enter\n\
         wait-text #where \"page two\""
    );
    run_with(&dir, &scenario, true).unwrap();
    run_with(&dir, &scenario, false).unwrap();
}

/// A `platform` block's steps run only with that platform's shortcuts.
#[test]
fn platform_blocks() {
    let dir = pages("platform", &[("one.html", BUTTON)]);
    let scenario = "navigate one.html\nwait-loaded\ntimeout 0.3\n\
        platform mac\nwait-text #out \"mac\"\n\
        platform other\nwait-text #out \"other\"\n\
        platform all\nwait-text #out \"not yet\"";
    assert_eq!(run_with(&dir, scenario, true).unwrap_err().line, 5);
    assert_eq!(run_with(&dir, scenario, false).unwrap_err().line, 7);
    let without_all = scenario.replace("wait-text #out \"not yet\"", "wait-text #out \"all\"");
    let blocks_only = "navigate one.html\nwait-loaded\nplatform mac\nplatform other\nplatform all\nwait-text #out \"not yet\"";
    run_with(&dir, blocks_only, true).unwrap();
    assert_eq!(run_with(&dir, &without_all, true).unwrap_err().line, 5);
}

/// `wait-busy` waits until script keeps the main thread busy; a page that
/// never is fails it.
#[test]
fn waiting_until_the_page_is_busy() {
    let busy = "<!DOCTYPE html><style>#b { display: block; height: 40px; background: blue }</style>\
        <span id=b onclick='busy()'></span><p id=out>idle</p><script>function busy() {\
        const end = Date.now() + 1500; while (Date.now() < end) {}\
        document.getElementById('out').textContent = 'done'; }</script>";
    let dir = pages("busy", &[("busy.html", busy)]);
    run_in(
        &dir,
        "navigate busy.html\nwait-loaded\nclick #b\nwait-busy\nwait-text #out \"done\"",
    )
    .unwrap();
    let failure = run_in(
        &dir,
        "navigate busy.html\nwait-loaded\ntimeout 0.5\nwait-busy",
    )
    .unwrap_err();
    assert_eq!(failure.message, "the page never got busy");
    // While it is busy, a frame showing the page waits for the main thread;
    // `now` takes the next drawn.
    let busy = "navigate busy.html
wait-loaded
click #b
wait-busy
timeout 0.5
";
    run_in(
        &dir,
        &format!(
            "{busy}capture first now
screenshot shot now"
        ),
    )
    .unwrap();
    // Without `now`, the frame that shows the click's change: the click
    // isn't handled yet.
    let failure = run_in(&dir, &format!("{busy}capture first")).unwrap_err();
    assert_eq!(
        failure.message,
        "the page hasn't handled the input given it, so no frame shows it: is it busy (then add `now`)?"
    );
}

#[test]
fn the_scenario_sets_the_windows_size() {
    let dir = pages("size", &[("one.html", BUTTON)]);
    run_in(
        &dir,
        "window size=320x240 scale=1.5\nnavigate one.html\nwait-loaded\nscreenshot sized",
    )
    .unwrap();
    let decoder = png::Decoder::new(std::io::BufReader::new(
        std::fs::File::open(dir.join("out").join("sized.png")).unwrap(),
    ));
    let info = decoder.read_info().unwrap();
    assert_eq!((info.info().width, info.info().height), (480, 360));
}

/// A click right after a change that moves its element goes where the
/// element is now: the next element is looked for once the renderer has
/// handled the click that changed the page, with the frame that shows the
/// change. Before that wait, a frame drawn between looking for the target
/// and clicking it sent some clicks to the line added above it (most runs
/// of `windowless`, though seldom this test alone).
#[test]
fn a_click_follows_an_element_a_change_just_moved() {
    let page = "<!DOCTYPE html><style>html, body { margin: 0 } p { margin: 0; height: 30px }</style>\
        <p id=grow onclick='grow()'>grow</p><div id=lines></div>\
        <p id=target onclick='hit()'>target</p><p id=out>none</p><script>\
        function grow() { const p = document.createElement('p'); p.textContent = 'line';\
        document.getElementById('lines').appendChild(p); }\
        let hits = 0; function hit() { document.getElementById('out').textContent = String(++hits); }\
        requestAnimationFrame(function frame() { requestAnimationFrame(frame); });</script>";
    let dir = pages("moved", &[("moved.html", page)]);
    // Tall enough for the target after the last line added.
    let mut scenario = String::from("window size=800x1400\nnavigate moved.html\nwait-loaded\n");
    // Each click moves the target down by its own height: a click at its
    // old place lands on the line added above it. The animation keeps main
    // frames coming, so one may be drawn between the target's box being
    // asked for and the click.
    for _ in 0..40 {
        scenario.push_str("click #grow\nclick #target\n");
    }
    scenario.push_str("timeout 1\nwait-text #out \"40\"");
    run_in(&dir, &scenario).unwrap();
}

/// What is asked about the page waits for the input given it to be
/// handled: while a click's listener keeps the page busy, looking for an
/// element times out on that, not on an unanswered query.
#[test]
fn a_query_waits_for_the_input_given() {
    let busy = "<!DOCTYPE html><style>#b { display: block; height: 40px; background: blue }</style>\
        <span id=b onclick='busy()'></span><p id=out>idle</p><script>function busy() {\
        const end = Date.now() + 1500; while (Date.now() < end) {}\
        document.getElementById('out').textContent = 'done'; }</script>";
    let dir = pages("input-handled", &[("busy.html", busy)]);
    let failure = run_in(
        &dir,
        "navigate busy.html\nwait-loaded\nclick #b\ntimeout 0.3\nwait-text #out \"done\"",
    )
    .unwrap_err();
    assert_eq!(
        failure.message,
        "the page hasn't handled the input given it: is it busy?"
    );
    // Once it has, the query sees what the listener did.
    run_in(
        &dir,
        "navigate busy.html\nwait-loaded\nclick #b\nwait-text #out \"done\"",
    )
    .unwrap();
}

/// Each tab's loads are its own: waiting for the shown tab's isn't
/// answered by another tab's, and another tab's renderer going leaves the
/// wait alone.
#[test]
fn waiting_for_a_load_waits_for_the_shown_tabs() {
    let dir = pages("tab-loads", &[("one.html", BUTTON)]);
    let (mut browser, host_rx, _context_lost) = browser();
    let (outbox, messages) = unbounded();
    let mut session = AutomationSession::new(&browser, outbox);
    let until_loaded = |browser: &mut BrowserWindow, session: &mut AutomationSession, of: TabId| loop {
        let (tab, event) = host_rx.recv_timeout(Duration::from_secs(30)).unwrap();
        let Some(msg) = browser.view_mut().on_contents_event(tab, event) else {
            continue;
        };
        session.on_renderer_message(tab, &msg, browser);
        if tab == of && matches!(msg, RendererHostMsg::DidFinishLoad { .. }) {
            return;
        }
    };
    // The first tab's second navigation has loaded.
    let url = shell::to_url(&dir.join("one.html").to_string_lossy());
    for id in 1..=2 {
        let navigate = Method::MachopoNavigateFromAddressBar(Navigate { url: url.clone() });
        session.command(command(id, navigate), &mut browser);
        until_loaded(&mut browser, &mut session, TabId(0));
    }
    // A new tab's first hasn't.
    browser.view_mut().run(browser_view::Command::NewTab);
    session.command(command(3, Method::MachopoWaitForLoad), &mut browser);
    assert!(!answered(&messages, 3));
    // Another tab's load, however late a navigation, is another tab's.
    let other = RendererHostMsg::DidFinishLoad {
        navigation_id: 99,
        frame_token: None,
        failed_subresources: Vec::new(),
        reftest_wait: false,
    };
    session.on_renderer_message(TabId(0), &other, &browser);
    assert!(!answered(&messages, 3));
    session.on_renderer_message(TabId(0), &RendererHostMsg::RendererGone, &browser);
    assert!(!answered(&messages, 3));
    until_loaded(&mut browser, &mut session, TabId(1));
    assert!(answered(&messages, 3));
    browser.shutdown();
}

/// The window's bounds are CSS px, as `Browser.Bounds` are: at 2x, the
/// 400×300 physical px window is 200×150; a size not given stays.
#[test]
fn the_windows_bounds_are_css_px() {
    let (mut browser, _host_rx, _context_lost) = browser_at(2.0);
    let (outbox, messages) = unbounded();
    let mut session = AutomationSession::new(&browser, outbox);
    let window = Method::BrowserGetWindowForTarget(GetTargetInfo::default());
    session.command(command(1, window), &mut browser);
    let bounds = answer(&messages, 1).unwrap().unwrap()["bounds"].clone();
    assert_eq!(
        (bounds["width"].as_u64(), bounds["height"].as_u64()),
        (Some(200), Some(150))
    );
    let set = Method::BrowserSetWindowBounds(SetWindowBounds {
        window_id: 1,
        bounds: Bounds {
            width: Some(100),
            ..Bounds::default()
        },
    });
    let (input, _) = session.command(command(2, set), &mut browser).unwrap();
    assert_eq!(
        input,
        Input::SetBounds {
            origin: None,
            width: 100.0,
            height: 150.0
        }
    );
    // Only one coordinate: the other stays where the window is.
    browser.view_mut().move_to(PointF::new(10.0, 20.0));
    let set = Method::BrowserSetWindowBounds(SetWindowBounds {
        window_id: 1,
        bounds: Bounds {
            left: Some(-30),
            ..Bounds::default()
        },
    });
    let (input, _) = session.command(command(3, set), &mut browser).unwrap();
    assert_eq!(
        input,
        Input::SetBounds {
            origin: Some(PointF::new(-30.0, 20.0)),
            width: 200.0,
            height: 150.0
        }
    );
    browser.shutdown();
}

/// What waits for the shown tab's renderer fails once it has gone: a load,
/// a query, a capture, a scroll waiting for its frame and input waiting to
/// be handled.
#[test]
fn a_renderer_gone_fails_what_waits_for_it() {
    let dir = pages("gone", &[("one.html", BUTTON)]);
    let (mut browser, _host_rx, _context_lost) = browser();
    let (outbox, messages) = unbounded();
    let mut session = AutomationSession::new(&browser, outbox);
    let url = shell::to_url(&dir.join("one.html").to_string_lossy());
    session.command(
        command(1, Method::MachopoNavigateFromAddressBar(Navigate { url })),
        &mut browser,
    );
    let query = || QueryElement {
        selector: "#b".into(),
        frames: Vec::new(),
    };
    // None of them answered: the renderer's messages aren't given to the
    // session.
    let waits = [
        Method::MachopoWaitForLoad,
        Method::MachopoQueryElement(query()),
        Method::MachopoCaptureWindow(CaptureWindow { now: false }),
        Method::MachopoScrollIntoViewIfNeeded(query()),
    ];
    for (id, method) in (2..).zip(waits) {
        assert!(session.command(command(id, method), &mut browser).is_none());
    }
    // And a press waiting for the renderer to handle it, as a click from
    // the driver does.
    let attach = Method::TargetAttachToTarget(AttachToTarget {
        target_id: session.target_id(TabId(0)).expect("the tab is seen"),
        flatten: true,
    });
    session.command(command(6, attach), &mut browser);
    let attached = answer(&messages, 6).expect("attached").unwrap();
    let press = Command {
        id: 7,
        session_id: attached["sessionId"].as_str().map(str::to_owned),
        method: Method::InputDispatchMouseEvent(DispatchMouseEvent {
            kind: "mousePressed".into(),
            x: 10.0,
            y: 10.0,
            button: Some("left".into()),
            buttons: 1,
            click_count: 1,
            ..DispatchMouseEvent::default()
        }),
    };
    assert!(session.command(press, &mut browser).is_none());
    let gone = RendererHostMsg::RendererGone;
    session.on_renderer_message(TabId(0), &gone, &browser);
    let answers: Vec<_> = messages.try_iter().collect();
    for id in [2, 3, 4, 5, 7] {
        let answered = answers.iter().find_map(|message| match message {
            Message::Response(response) if response.id == id => Some(&response.result),
            _ => None,
        });
        let error = answered.unwrap_or_else(|| panic!("{id} wasn't answered: {answers:?}"));
        assert_eq!(
            error.as_ref().unwrap_err().message,
            "the tab's renderer has gone"
        );
    }
    browser.shutdown();
}

/// A runner sends what it came to, then its commands' channel closes:
/// both may be there when `drive` looks, and what it came to wins.
#[test]
fn what_the_runner_came_to_wins_over_its_channel_closing() {
    let (mut browser, host_rx, context_lost) = browser();
    let (outbox, _messages) = unbounded();
    let mut session = AutomationSession::new(&browser, outbox);
    // `select!` picks among what is ready at random.
    for _ in 0..20 {
        let (_, commands) = unbounded();
        let (done_tx, done) = unbounded();
        done_tx.send("came to").unwrap();
        let ended = windowless::drive(
            &mut browser,
            &mut session,
            &host_rx,
            &context_lost,
            &commands,
            &crossbeam_channel::never(),
            &done,
            1.0,
        );
        assert!(matches!(ended, windowless::Ended::Done(Some("came to"))));
    }
    browser.shutdown();
}

/// A remote debugging client of a window a person uses (no driver): the
/// browser's methods are there, the `Machopo` domain isn't; one that has gone
/// takes its sessions with it.
#[test]
fn without_a_driver_there_is_no_machopo_domain() {
    let (mut browser, _host_rx, _context_lost) = browser();
    let mut session = AutomationSession::for_remote_debugging(&browser);
    let connect = |session: &mut AutomationSession, browser: &mut BrowserWindow| {
        let (outbox, messages) = unbounded::<Message>();
        let (accepted, client) = unbounded::<Option<ClientId>>();
        session.client_event(
            ClientEvent::Connect {
                outbox: Outbox::from(outbox),
                page: None,
                accepted: Box::new(move |client| accepted.send(client).unwrap()),
            },
            browser,
        );
        (client.recv().unwrap().expect("connected"), messages)
    };
    let (client, messages) = connect(&mut session, &mut browser);
    let send =
        |session: &mut AutomationSession, browser: &mut BrowserWindow, client, id, method| {
            session.client_event(
                ClientEvent::Command(client, Ok(command(id, method))),
                browser,
            );
        };
    send(
        &mut session,
        &mut browser,
        client,
        1,
        Method::MachopoGetTitle,
    );
    let refused = messages.try_iter().find_map(|message| match message {
        Message::Response(response) if response.id == 1 => Some(response.result),
        _ => None,
    });
    assert_eq!(
        refused.expect("answered").unwrap_err().code,
        protocol::automation::METHOD_NOT_FOUND
    );
    send(
        &mut session,
        &mut browser,
        client,
        2,
        Method::BrowserGetVersion,
    );
    let version = answer(&messages, 2).expect("answered").unwrap();
    assert!(
        version["product"]
            .as_str()
            .unwrap()
            .starts_with("Headlessmachopo/")
    );

    let attach = Method::TargetAttachToTarget(AttachToTarget {
        target_id: session.target_id(TabId(0)).expect("the tab is seen"),
        flatten: true,
    });
    send(&mut session, &mut browser, client, 3, attach);
    let attached = answer(&messages, 3).expect("answered").unwrap();
    let id = attached["sessionId"].as_str().unwrap().to_owned();
    session.client_event(ClientEvent::Disconnect(client), &mut browser);
    // The same session id, from another client, is no session.
    let (other, others) = connect(&mut session, &mut browser);
    let enable = Command {
        id: 4,
        session_id: Some(id),
        method: Method::PageEnable,
    };
    session.client_event(ClientEvent::Command(other, Ok(enable)), &mut browser);
    let refused = others.try_iter().find_map(|message| match message {
        Message::Response(response) if response.id == 4 => Some(response.result),
        _ => None,
    });
    assert_eq!(
        refused.expect("answered").unwrap_err().code,
        protocol::automation::SESSION_NOT_FOUND
    );
    // Nor is the tab attached any more.
    session.client_event(
        ClientEvent::Command(other, Ok(command(5, Method::TargetGetTargets))),
        &mut browser,
    );
    let targets = answer(&others, 5).expect("answered").unwrap();
    assert_eq!(targets["targetInfos"][0]["attached"], false);
    browser.shutdown();
}

/// A page's DevTools message from a renderer reaches the session's client
/// as it is, with the session's id; one for a session of another tab, one
/// that isn't a JSON object, and one over the limit are dropped (a
/// renderer is not to be trusted).
#[test]
fn a_renderers_devtools_messages_are_checked() {
    let (mut browser, _host_rx, _context_lost) = browser();
    let (outbox, messages) = unbounded();
    let mut session = AutomationSession::new(&browser, outbox);
    let attach = Method::TargetAttachToTarget(AttachToTarget {
        target_id: session.target_id(TabId(0)).expect("the tab is seen"),
        flatten: true,
    });
    session.command(command(1, attach), &mut browser);
    let id = answer(&messages, 1).expect("answered").unwrap()["sessionId"]
        .as_str()
        .unwrap()
        .to_owned();
    let from = |session: &mut AutomationSession, tab, message: String| {
        session.on_renderer_message(
            tab,
            &RendererHostMsg::DevToolsMessage {
                session: id.clone(),
                message,
            },
            &browser,
        );
    };
    from(
        &mut session,
        TabId(0),
        r#"{"id": 2, "result": {"x": 1}}"#.to_owned(),
    );
    let passed = messages.try_iter().collect::<Vec<_>>();
    assert_eq!(passed.len(), 1, "{passed:?}");
    let Message::Raw { session_id, object } = &passed[0] else {
        panic!("{passed:?}");
    };
    assert_eq!(session_id.as_deref(), Some(id.as_str()));
    assert_eq!(object["result"]["x"], 1);
    // Another tab's renderer, not JSON, too long.
    from(
        &mut session,
        TabId(1),
        r#"{"id": 3, "result": {}}"#.to_owned(),
    );
    from(&mut session, TabId(0), "[3]".to_owned());
    let mut huge = String::from(r#"{"id": 4, "result": ""#);
    huge.push_str(&"x".repeat(256 << 20));
    huge.push_str(r#""}"#);
    from(&mut session, TabId(0), huge);
    assert!(messages.try_iter().next().is_none());
    browser.shutdown();
}

/// A shadow tree's `<link>` is loaded and applies to its tree.
#[test]
fn a_shadow_trees_link_loads() {
    let page = "<!DOCTYPE html><div id=host></div><p id=out>loading</p><script>\
        const root = document.getElementById('host').attachShadow({ mode: 'open' });\
        const link = document.createElement('link');\
        link.setAttribute('rel', 'stylesheet');\
        link.setAttribute('href', 'shadow.css');\
        root.appendChild(link);\
        const box = document.createElement('div');\
        root.appendChild(box);\
        window.addEventListener('load', () => {\
          document.getElementById('out').textContent = `width ${box.offsetWidth}`;\
        });</script>";
    let dir = pages(
        "next-dom-shadow-link",
        &[
            ("page.html", page),
            ("shadow.css", "div { width: 120px; height: 10px }"),
        ],
    );
    run_in(
        &dir,
        "navigate page.html\nwait-loaded\nwait-text #out \"width 120\"",
    )
    .unwrap();
}

/// On the DOM: a control's UA shadow tree is built before the frame's
/// style, so its content is laid out: an input button as wide as its
/// label, the parser's and one script inserts (a script's geometry asks
/// for the layout, which builds it too).
#[test]
fn a_controls_tree_is_built() {
    let page = "<!DOCTYPE html><input type=submit id=a value='a fairly long label'><p id=out>loading</p><script>\
        const b = document.body.appendChild(document.createElement('input'));\
        b.type = 'submit';\
        b.value = 'another fairly long label';\
        const now = b.offsetWidth;\
        window.addEventListener('load', () => {\
          const a = document.getElementById('a').offsetWidth;\
          document.getElementById('out').textContent = a > 80 && now > 80 ? 'wide' : `narrow ${a} ${now}`;\
        });</script>";
    let dir = pages("next-dom-control-tree", &[("page.html", page)]);
    run_in(
        &dir,
        "navigate page.html\nwait-loaded\nwait-text #out \"wide\"",
    )
    .unwrap();
}

/// On the DOM (`Options::next_dom`): the title the browser shows,
/// and a script's change to it; `scroll-to` an element in view (nothing
/// moves), one in a scroller, one below the window (the screenshots show
/// them) and a click that scrolls its element into view first; the text a
/// wait sees; nothing in a child frame.
#[test]
fn a_scenario() {
    let page = "<!DOCTYPE html><title> one\n page </title>\
        <style>html, body { margin: 0 } div { height: 100px }</style>\
        <div style='opacity: 0.99'><div id=box style='overflow: auto'><div style='height: 1000px'></div>\
        <div id=inner style='background: red'></div></div></div>\
        <div style='height: 200px'></div><div id=near style='background: lime'></div>\
        <div style='height: 1600px'></div>\
        <div id=far style='height: 2000px; background: blue'></div>\
        <div style='height: 2000px'></div>\
        <span id=b style='display: block; width: 100px; height: 40px; background: lime'></span>\
        <p id=out>not yet</p>\
        <script>let clicks = 0; document.getElementById('b').addEventListener('click', () => {\
          document.getElementById('out').textContent = `clicked ${++clicks}`;\
          document.querySelector('title').textContent = 'two';\
        });</script>";
    let dir = pages("next-dom", &[("page.html", page)]);
    run_in(
        &dir,
        "navigate page.html\nwait-loaded\nwait-title \"one page\"\nscreenshot top\n\
         scroll-to #near\nscreenshot near\nscroll-to #inner\nscreenshot inner\n\
         scroll-to #far\nscreenshot far\nclick #b\nwait-text #out \"clicked 1\"\nwait-title \"two\"",
    )
    .unwrap();
    // The bottom row of the window: the spacer, then #far.
    let bottom = |(pixels, width): (Vec<u8>, u32)| {
        let i = (pixels.len() / 4 - width as usize / 2) * 4;
        pixels[i..i + 3].to_vec()
    };
    let top = screenshot(&dir, "top");
    assert_eq!(screenshot(&dir, "near").0, top.0, "#near was in view");
    // #inner in its scroller's view, at the top of the window: a scroller
    // in an effect is no layer of its own, and is painted scrolled.
    let red = |(pixels, width): (Vec<u8>, u32)| {
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .skip(width as usize / 2)
            .step_by(width as usize)
            .any(|pixel| pixel[0] > 250 && pixel[1] < 10 && pixel[2] < 10)
    };
    assert!(!red(top.clone()));
    assert!(
        red(screenshot(&dir, "inner")),
        "#inner scrolled into its scroller's view"
    );
    assert_eq!(bottom(top), [255, 255, 255]);
    assert_eq!(bottom(screenshot(&dir, "far")), [0, 0, 255]);
    // What isn't an iframe has no child frame: nothing is in one.
    let failure = run_in(
        &dir,
        "navigate page.html\nwait-loaded\ntimeout 1\nframe #b\nwait-text #out \"not yet\"",
    )
    .unwrap_err();
    assert!(failure.message.contains("#out"), "{failure:?}");
}
