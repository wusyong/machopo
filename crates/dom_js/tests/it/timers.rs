//! A window's timers in a realm whose global is it, run by the host
//! (`run_timers`); what a listener or a timer throws reported at the
//! window as an `ErrorEvent`.

use crate::env::Env;

/// `setTimeout` and `setInterval`: ids from 1, their callbacks called with
/// the window as `this` and their rest arguments, in order, once due; a
/// microtask checkpoint after each; `clearTimeout` and `clearInterval`.
#[test]
fn timers_run_when_due() {
    let mut env = Env::window("about:blank");
    assert_eq!(env.next_timer(), None);
    assert_eq!(
        env.eval(
            "var log = []; \
             var a = setTimeout(function (x, y) { log.push('a ' + (this === window) + ' ' + x + y); \
                 Promise.resolve().then(() => log.push('micro')); }, 0, 'p', 1); \
             var b = setTimeout(() => log.push('b'), 0); \
             var c = setInterval(() => log.push('c'), 100); \
             var d = setTimeout(() => log.push('d'), 1000); \
             [a, b, c, d, setTimeout.length, setInterval.length].join()"
        ),
        "1,2,3,4,1,1"
    );
    // Made a moment apart (well under 50 ms, even on a busy machine), so
    // due a moment apart: `first` is `a`'s.
    let first = env.next_timer().unwrap();
    assert_eq!(env.run_timers(Some(first + 50.0)), 2);
    assert_eq!(env.eval("log.join()"), "a true p1,micro,b");
    // The interval, due 100 ms after it was made, then 100 ms after it
    // ran: nothing between.
    let c = env.next_timer().unwrap();
    assert!(c >= first + 100.0, "{c} {first}");
    assert_eq!(env.run_timers(Some(c - 1.0)), 0);
    assert_eq!(env.run_timers(Some(c)), 1);
    assert_eq!(env.next_timer(), Some(c + 100.0));
    assert_eq!(env.run_timers(Some(c + 100.0)), 1);
    assert_eq!(
        env.eval("clearInterval(c); clearTimeout(d); clearTimeout(); log.join()"),
        "a true p1,micro,b,c,c"
    );
    assert_eq!(env.run_timers(Some(first + 2000.0)), 0);
    assert_eq!(env.next_timer(), None);
    // By the window's clock; a strict function's `this` is the window too,
    // not undefined.
    assert_eq!(
        env.eval("setTimeout(function () { 'use strict'; log.push('now ' + (this === window)); })"),
        "5"
    );
    assert_eq!(env.run_timers(None), 1);
    assert_eq!(env.eval("log[log.length - 1]"), "now true");
}

/// A handler that isn't a function is a string, run as a classic script
/// in the window's realm when the timer is due (its rest arguments
/// ignored; what it throws reported at the window, a syntax error too); a
/// timeout converts as a `long`; the handler is required.
#[test]
fn a_timers_arguments() {
    let mut env = Env::window("https://example.test/page.html");
    let cases = [
        ("try { setTimeout(); } catch (e) { e.name }", "TypeError"),
        ("typeof setTimeout(() => {}, '10')", "number"),
        (
            "try { setTimeout(() => {}, {valueOf() { throw new Error('v'); }}); } catch (e) { e.message }",
            "v",
        ),
        (
            "try { setTimeout({toString() { throw new Error('s'); }}); } catch (e) { e.message }",
            "s",
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(env.eval(source), expected, "{source}");
    }
    env.eval(
        "var log = []; window.addEventListener('error', e => { log.push(e.message + ' ' + e.filename); e.preventDefault(); }); \
         setTimeout('log.push(\\'string \\' + (this === window) + \\' \\' + typeof arguments)', 0, 'ignored'); \
         setTimeout({toString() { return 'log.push(\\'object\\')'; }}); \
         setTimeout({});",
    );
    let at = env.next_timer().unwrap() + 50.0;
    // And the cases' own timer.
    assert_eq!(env.run_timers(Some(at)), 4);
    assert_eq!(
        env.eval("log.join('|')"),
        "string true undefined|object|Uncaught SyntaxError: Unexpected identifier 'Object' https://example.test/page.html"
    );
}

/// `queueMicrotask`: the callback runs at the next checkpoint, in order
/// with promise jobs, `this` undefined; what it throws is reported at the
/// window and the queue goes on; a callback that isn't a function is a
/// `TypeError`; the property is the window's own, as an operation of a
/// `[Global]` interface.
#[test]
fn queue_microtask_queues_a_callback() {
    let mut env = Env::window("about:blank");
    env.eval(
        "var log = []; window.addEventListener('error', e => { log.push('error ' + e.message); e.preventDefault(); }); \
         queueMicrotask(function () { 'use strict'; log.push('first ' + this); }); \
         Promise.resolve().then(() => log.push('promise')); \
         queueMicrotask(() => { throw new Error('boom'); }); \
         queueMicrotask(() => log.push('last')); log.push('sync');",
    );
    assert_eq!(env.eval("log.join()"), "sync");
    env.checkpoint();
    assert_eq!(
        env.eval("log.join()"),
        "sync,first undefined,promise,error Uncaught Error: boom,last"
    );
    let cases = [
        (
            "try { queueMicrotask('x'); } catch (e) { e.name }",
            "TypeError",
        ),
        (
            "try { queueMicrotask(); } catch (e) { e.name }",
            "TypeError",
        ),
        ("queueMicrotask.length", "1"),
        (
            "Object.getOwnPropertyDescriptor(window, 'queueMicrotask').enumerable",
            "true",
        ),
        (
            "try { new queueMicrotask(() => {}); } catch (e) { e.name }",
            "TypeError",
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(env.eval(source), expected, "{source}");
    }
}

/// Timers keep their callbacks and arguments through V8's GCs, which only
/// the timers hold.
#[test]
fn timers_keep_their_callbacks_through_gcs() {
    let mut env = Env::window("about:blank");
    env.eval(
        "var log = []; (function () { var o = {v: 'kept'}; \
         setTimeout(function (x) { log.push(x.v); }, 0, o); \
         setInterval(() => log.push('i'), 1); })();",
    );
    env.minor_gc();
    env.minor_gc();
    env.full_gc();
    let at = env.next_timer().unwrap() + 50.0;
    assert_eq!(env.run_timers(Some(at)), 2);
    assert_eq!(env.eval("log.join()"), "kept,i");
}

/// What a timer or a listener throws fires an `ErrorEvent` at the window:
/// its message, filename, line, column and error; not logged if a listener
/// cancels it; one thrown by an `error` listener isn't reported again.
#[test]
fn exceptions_are_reported_at_the_window() {
    let mut env = Env::window("about:blank");
    env.eval_at(
        "var log = []; var cancel = false; \
         window.addEventListener('error', e => { \
           log.push([e instanceof ErrorEvent, e.message, e.filename, e.lineno, e.colno, \
                     e.error && e.error.tag, e.isTrusted, e.cancelable, e.bubbles].join()); \
           if (cancel) e.preventDefault(); });\n\
         setTimeout(() => { var e = new Error('boom'); e.tag = 't'; throw e; });",
        "https://example.test/a.js",
    );
    assert_eq!(env.run_timers(Some(1e6)), 1);
    // Where it was thrown: `throw`'s line and column.
    assert_eq!(
        env.eval("log.join('|')"),
        "true,Uncaught Error: boom,https://example.test/a.js,2,60,t,true,true,false"
    );
    assert_eq!(env.uncaught_exceptions(), 1, "not canceled: logged");
    // A listener's, canceled: not logged; a primitive thrown is the error.
    assert_eq!(
        env.eval(
            "log = []; cancel = true; var t = new EventTarget(); \
             t.addEventListener('x', () => { throw 42; }); t.dispatchEvent(new Event('x')); \
             log.length"
        ),
        "1"
    );
    assert_eq!(env.uncaught_exceptions(), 1);
    assert_eq!(
        env.eval("log[0].split(',').slice(0, 2).join()"),
        "true,Uncaught 42"
    );
    // An `error` listener that throws: logged, not fired again.
    env.eval(
        "log = []; cancel = false; window.addEventListener('error', () => { throw new Error('again'); }); \
         setTimeout(() => { throw new Error('first'); });",
    );
    assert_eq!(env.run_timers(Some(1e6)), 1);
    assert_eq!(env.eval("log.length"), "1");
    assert_eq!(
        env.uncaught_exceptions(),
        3,
        "the first and the listener's own"
    );
    // Reporting again afterwards.
    env.eval("setTimeout(() => { throw new Error('later'); });");
    env.run_timers(Some(2e6));
    assert_eq!(env.eval("log.length"), "2");
}

/// An `ErrorEvent`'s error stays while the event lives, through V8's GCs
/// (only the event holds it): a reported one's, and one script made.
#[test]
fn an_error_events_error_survives_gcs() {
    let mut env = Env::window("about:blank");
    env.eval(
        "var kept; window.addEventListener('error', e => { kept = e; e.preventDefault(); }); \
         setTimeout(() => { throw {tag: 'held'}; });",
    );
    env.run_timers(Some(1e6));
    env.minor_gc();
    env.minor_gc();
    env.full_gc();
    assert_eq!(
        env.eval("[kept.error.tag, kept.error === kept.error].join()"),
        "held,true"
    );
    // A primitive thrown (a Smi, a heap number, a string) as well.
    env.eval(
        "var errors = []; window.addEventListener('error', e => { errors.push(e); e.preventDefault(); }); \
         setTimeout(() => { throw 7; }); setTimeout(() => { throw 0.5; }); \
         setTimeout(() => { throw 'text' + errors.length; });",
    );
    env.run_timers(Some(2e6));
    env.minor_gc();
    env.full_gc();
    assert_eq!(env.eval("errors.map(e => e.error).join()"), "7,0.5,text2");
    // Script's, its constructor's error.
    env.eval("var made = new ErrorEvent('x', { error: { tag: 'made' } }); 0");
    env.minor_gc();
    env.full_gc();
    assert_eq!(
        env.eval("[made.error.tag, new ErrorEvent('x').error].join()"),
        "made,"
    );
}
