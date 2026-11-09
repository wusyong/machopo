//! Idle callbacks: `requestIdleCallback`'s handles and `cancelIdleCallback`,
//! an idle period running only those requested before it started, one at
//! a time, the `IdleDeadline` they get, and a timeout that runs one as a
//! timer does, before a timer due later.

use std::time::{Duration, Instant};

use dom_api::ScriptEngine;

use crate::history::Page;

#[test]
fn an_idle_period_runs_the_callbacks_requested_before_it() {
    let mut page = Page::new("https://example.test/");
    assert_eq!(
        page.eval(
            "var log = []; var a = requestIdleCallback(d => log.push(['a', d instanceof IdleDeadline, d.didTimeout, d.timeRemaining() > 40, d.timeRemaining() <= 50].join())); \
             var b = requestIdleCallback(() => log.push('b')); cancelIdleCallback(b); cancelIdleCallback(99); \
             requestIdleCallback(() => { log.push('c'); requestIdleCallback(() => log.push('e')); throw new Error('thrown'); }); \
             addEventListener('error', e => log.push(e.message)); \
             var r = [a, b]; try { requestIdleCallback(5); } catch (e) { r.push(e.name); } r.join()"
        ),
        "1,2,TypeError"
    );
    assert!(page.engine.has_idle_callbacks());
    assert!(
        !page.engine.has_runnable_idle_callback(),
        "none before a period starts"
    );
    assert!(!page.engine.run_idle_callback(Instant::now()));
    page.engine.start_idle_period();
    assert!(page.engine.has_runnable_idle_callback());
    let deadline = Instant::now() + Duration::from_millis(50);
    assert!(page.engine.run_idle_callback(deadline));
    assert_eq!(
        page.eval("log.join(' ')"),
        "a,true,false,true,true",
        "one at a time"
    );
    assert!(page.engine.run_idle_callback(deadline));
    assert!(
        !page.engine.run_idle_callback(deadline),
        "`e` waits for the next period"
    );
    assert_eq!(
        page.eval("log.join(' ')"),
        "a,true,false,true,true c Uncaught Error: thrown"
    );
    assert!(page.engine.has_idle_callbacks());
    page.engine.start_idle_period();
    // A period that has ended leaves no time.
    assert!(page.engine.run_idle_callback(Instant::now()));
    assert_eq!(page.eval("log.slice(3).join()"), "e");
    assert!(!page.engine.has_idle_callbacks());
    assert_eq!(
        page.eval("var d; requestIdleCallback(x => { d = x; }); 0"),
        "0"
    );
    page.engine.start_idle_period();
    page.engine.run_idle_callback(Instant::now());
    assert_eq!(page.eval("d.timeRemaining()"), "0");
}

/// A deadline off the 100 µs grid (as the next frame's is, while frames
/// are drawn): the time left on the grid, never past it (WPT's
/// `deadline-max-rAF`: not 16.7 of 16.67). 50.067 ms ahead, not a
/// frame's 16.67: the check of how much is left allows 10 ms between the
/// deadline taken and the script reading it, which a loaded machine
/// (the crate's tests on every core) took past 1.67 ms.
#[test]
fn the_time_remaining_is_never_past_the_deadline() {
    let mut page = Page::new("https://example.test/");
    page.eval("var r; requestIdleCallback(d => { r = d.timeRemaining(); }); 0");
    page.engine.start_idle_period();
    assert!(
        page.engine
            .run_idle_callback(Instant::now() + Duration::from_micros(50_067))
    );
    assert_eq!(
        page.eval("[r <= 50.067, r > 40, Math.abs(r * 10 - Math.round(r * 10)) < 1e-9].join()"),
        "true,true,true"
    );
}

#[test]
fn a_timeout_runs_one_as_a_timer() {
    let mut page = Page::new("https://example.test/");
    page.eval(
        "var log = []; setTimeout(() => log.push('early timer'), 0); \
         requestIdleCallback(d => log.push(['idle', d.didTimeout, d.timeRemaining()].join()), { timeout: 1 }); \
         requestIdleCallback(() => log.push('no timeout'), { timeout: 0 }); \
         setTimeout(() => log.push('late timer'), 3); 0",
    );
    std::thread::sleep(Duration::from_millis(10));
    while page.engine.run_timers() {}
    assert_eq!(
        page.eval("log.join(' ; ')"),
        "early timer ; idle,true,0 ; late timer",
        "a timeout due before a timer first; none for a timeout of 0"
    );
    assert!(page.engine.has_idle_callbacks());
}

/// A timeout alone makes the window's timers due: the host wakes for it
/// and runs it.
#[test]
fn a_timeout_alone_is_due() {
    let mut page = Page::new("https://example.test/");
    page.eval("var log = []; requestIdleCallback(d => log.push(d.didTimeout), { timeout: 5 }); 0");
    let wait = page.engine.until_next_timer().expect("a timer due");
    assert!(wait <= Duration::from_millis(5), "{wait:?}");
    std::thread::sleep(Duration::from_millis(10));
    assert!(page.engine.run_timers());
    assert_eq!(page.eval("log.join()"), "true");
}
