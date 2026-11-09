//! `navigator` in a whole browser: the one user agent, which
//! the page's request sent as `User-Agent` and its script reads, Chrome's;
//! a child frame has its own `navigator`, of its own realm even when this
//! window's getter reads it, with the same user agent, its initial
//! `about:blank` too.

use std::collections::HashMap;
use std::time::Duration;

use net::test_server::{Route, TestServer};
use shell::headless;

#[test]
fn the_user_agent_is_the_one_requests_send() {
    let page = r#"<!DOCTYPE html>
<iframe id="f" srcdoc="<p>child"></iframe>
<script>
onload = () => {
  const childWindow = document.getElementById('f').contentWindow;
  // This realm's getter (`[Global]`: the window's own property): the
  // wrapper is still made in the child's realm.
  const get = Object.getOwnPropertyDescriptor(window, 'navigator').get;
  const child = get.call(childWindow);
  const blank = document.body.appendChild(document.createElement('iframe')).contentWindow;
  testRunner.reportHarnessResults([
    navigator.userAgent, child !== navigator, child.userAgent === navigator.userAgent,
    child instanceof childWindow.Navigator, child instanceof Navigator,
    blank.navigator.userAgent === navigator.userAgent,
  ].join(' / '));
};
</script>"#;
    let server = TestServer::start(HashMap::from([(
        "/".to_string(),
        Route::ok("text/html", page),
    )]))
    .expect("test server starts");
    let report = headless::run_testharness(
        &server.url("/"),
        Duration::from_secs(30),
        Default::default(),
    )
    .expect("the page reports");
    let agent = browser::user_agent();
    assert_eq!(
        report,
        format!("{agent} / true / true / true / false / true")
    );
    assert!(agent.contains(" Chrome/"), "{agent}");
    let requests = server.requests();
    assert_eq!(requests[0].header("user-agent"), Some(agent.as_str()));
}

/// The DOM's `navigator` has the same user agent, and no automation;
/// its `userActivation` says the user's click activated the window.
#[test]
fn the_navigator() {
    use protocol::input::{InputEvent, Modifiers, MouseButton};
    let page = r#"<!DOCTYPE html>
<script>
const a = navigator.userActivation;
const log = [navigator.userAgent, navigator.appVersion, navigator.webdriver, a.hasBeenActive, a.isActive];
onmousedown = () => {
  log.push(a.hasBeenActive, a.isActive);
  testRunner.reportHarnessResults(log.join(' / '));
};
</script>"#;
    let server = TestServer::start(HashMap::from([(
        "/".to_string(),
        Route::ok("text/html", page),
    )]))
    .expect("test server starts");
    let down = InputEvent::MouseDown {
        x: 10.0,
        y: 10.0,
        button: MouseButton::Left,
        buttons: MouseButton::Left.buttons_bit(),
        modifiers: Modifiers::NONE,
        click_count: 1,
    };
    let report = headless::run_testharness_after_input(
        &server.url("/"),
        gfx::Size::new(100, 100),
        &[down],
        Duration::from_secs(30),
    )
    .expect("the page reports");
    let agent = browser::user_agent();
    let version = agent.strip_prefix("Mozilla/").expect("Chrome's");
    assert_eq!(
        report,
        format!("{agent} / {version} / false / false / false / true / true")
    );
}

/// The DOM's user activation goes through the frame tree (HTML's
/// activation notification steps): a click in the top-level document
/// activates its same-origin child, not one of another origin;
/// `showPicker()` then consumes the page's transient activation, every
/// window's, keeping the sticky one. A click in the child of another
/// origin activates the frames above it, not its sibling.
#[test]
fn the_user_activation_goes_through_the_frame_tree() {
    use protocol::input::{InputEvent, Modifiers, MouseButton};
    let page = r#"<!DOCTYPE html>
<body style="margin: 0">
<iframe id=same srcdoc="<p>same" style="position: absolute; left: 50px; top: 0; width: 40px; height: 40px; border: 0"></iframe>
<iframe id=other style="position: absolute; left: 50px; top: 50px; width: 40px; height: 40px; border: 0"></iframe>
<select id=s style="position: absolute; left: 0; top: 60px"><option>a</select>
<script>
other.src = location.href.replace('127.0.0.1', 'localhost') + 'child.html';
const state = w => [w.navigator.userActivation.hasBeenActive, w.navigator.userActivation.isActive].join(' ');
const states = () => 'top ' + state(window) + ', same ' + state(same.contentWindow);
const log = [];
const ask = () => new Promise(resolve => {
  onmessage = e => resolve(e.data);
  other.contentWindow.postMessage('ask', '*');
});
onmousedown = async () => {
  log.push('clicked: ' + states(), 'other ' + await ask());
  s.showPicker();
  log.push('consumed: ' + states(), 'other ' + await ask());
  testRunner.reportHarnessResults(log.join(' / '));
};
// A click in the child of another origin: it says so.
onmessage = e => testRunner.reportHarnessResults('the child clicked: ' + states());
</script>"#;
    let child = r#"<!DOCTYPE html>
<script>
const state = () => [navigator.userActivation.hasBeenActive, navigator.userActivation.isActive].join(' ');
onmessage = () => parent.postMessage(state(), '*');
onmousedown = () => parent.postMessage('down', '*');
</script>"#;
    let server = TestServer::start(HashMap::from([
        ("/".to_string(), Route::ok("text/html", page)),
        ("/child.html".to_string(), Route::ok("text/html", child)),
    ]))
    .expect("test server starts");
    let run = |x, y| {
        let down = InputEvent::MouseDown {
            x,
            y,
            button: MouseButton::Left,
            buttons: MouseButton::Left.buttons_bit(),
            modifiers: Modifiers::NONE,
            click_count: 1,
        };
        headless::run_testharness_after_input(
            &server.url("/"),
            gfx::Size::new(100, 100),
            &[down],
            Duration::from_secs(30),
        )
        .expect("the page reports")
    };
    assert_eq!(
        run(10.0, 10.0),
        "clicked: top true true, same true true / other false false / \
         consumed: top true false, same true false / other false false"
    );
    assert_eq!(
        run(60.0, 60.0),
        "the child clicked: top true true, same false false"
    );
}
