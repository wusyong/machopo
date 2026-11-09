//! HTML's messages from script: `window.postMessage` (a task of its own,
//! its data a structured clone, `origin`, `source`, `targetOrigin`), to a
//! child frame's window; `ArrayBuffer`s transferred, platform objects a
//! `DataCloneError`; `MessageChannel`'s ports (`start()` or `onmessage`,
//! `close()`, transferred in a message); `new MessageEvent()`.

use std::time::Instant;

use dom_api::{ClassicScript, EngineSettings, FrameRequest, ScriptEngine};
use dom_js::V8Engine;

/// A window of a document at `https://example.test/a/` with
/// `<html><body>`; `log`, `error(f)`.
fn engine() -> V8Engine {
    let mut engine = V8Engine::new(EngineSettings { web_tests: true });
    engine
        .create_window(
            engine.top_frame(),
            "https://example.test/a/",
            Instant::now(),
        )
        .expect("a window");
    run(
        &mut engine,
        "var log = [];
         var error = f => {
             try { f(); return 'no error'; } catch (e) { return e.name || e.constructor.name; }
         };
         document.appendChild(document.createElement('html')).appendChild(document.createElement('body'));",
    );
    engine
}

fn run(engine: &mut V8Engine, source: &str) {
    engine.run_classic_script(
        engine.top_frame(),
        &ClassicScript {
            source,
            url: "https://example.test/a/",
            base_url: None,
            line: 1,
            muted: false,
            use_credentials: false,
        },
    );
}

fn log(engine: &mut V8Engine) -> String {
    run(
        engine,
        "testRunner.reportHarnessResults(log.join(' / ')); log = [];",
    );
    engine.take_harness_results().expect("reported")
}

/// A message to the window itself: not at once, a task of its own; its
/// data a clone (a new object of the same values, `Map`s too), the
/// caller's origin and window; `targetOrigin` `*`, `/`, a URL of another
/// origin (lost), not a URL (`SyntaxError`), or the options.
/// HTML's "clean up after running a callback": an event the UA dispatches
/// (a posted message) runs the microtasks after each listener, as the stack
/// is empty then; one script dispatches runs them only after the script.
#[test]
fn microtasks_run_between_the_listeners_of_a_ua_event() {
    let mut engine = engine();
    run(
        &mut engine,
        "addEventListener('message', () => { log.push('first'); Promise.resolve().then(() => log.push('micro')); });
         addEventListener('message', () => log.push('second'));
         postMessage(1, '*');
         dispatchEvent(new MessageEvent('message'));
         log.push('dispatched');",
    );
    assert_eq!(log(&mut engine), "first / second / dispatched");
    engine.perform_microtask_checkpoint();
    assert_eq!(log(&mut engine), "micro");
    assert!(engine.run_posted_messages());
    assert_eq!(log(&mut engine), "first / micro / second");
}

#[test]
fn a_message_to_a_window() {
    let mut engine = engine();
    run(
        &mut engine,
        "var sent = {a: 1, b: [1, 2], m: new Map([[1, 'x']])};
         addEventListener('message', e => log.push(
             `${e.data.a} ${e.data.b.join()} ${e.data.m.get(1)} ${e.data !== sent} ${e.origin} ` +
             `${e.source === window} ${e instanceof MessageEvent} ${e.ports.length} ${e.isTrusted} ${e.lastEventId === ''}`));
         postMessage(sent, '*');
         log.push('posted');",
    );
    assert_eq!(log(&mut engine), "posted");
    assert_eq!(
        engine.until_next_timer(),
        Some(std::time::Duration::ZERO),
        "waiting"
    );
    assert!(engine.run_posted_messages());
    assert_eq!(
        log(&mut engine),
        "1 1,2 x true https://example.test true true 0 true true"
    );
    assert!(!engine.run_posted_messages());
    run(
        &mut engine,
        "postMessage({a: 2, b: [], m: new Map()}, '/');
         postMessage({a: 3, b: [], m: new Map()}, 'https://other.test');
         postMessage({a: 4, b: [], m: new Map()}, {targetOrigin: 'https://example.test/x'});
         log.push(error(() => postMessage(1, 'nonsense')), postMessage.length);",
    );
    assert_eq!(log(&mut engine), "SyntaxError / 1");
    engine.run_posted_messages();
    let delivered = log(&mut engine);
    assert!(
        delivered.starts_with("2 ") && delivered.contains(" / 4 ") && !delivered.contains("3 "),
        "{delivered}"
    );
}

/// What can't be cloned: a `DataCloneError`, nothing posted; an
/// `ArrayBuffer` transferred: detached here, its bytes there.
#[test]
fn clones_and_transfers() {
    let mut engine = engine();
    run(
        &mut engine,
        "addEventListener('message', e => log.push(new Uint8Array(e.data.buffer).join()));
         log.push(error(() => postMessage(() => {}, '*')), error(() => postMessage(document, '*')),
                  error(() => postMessage(1, '*', [1])));
         var buffer = new Uint8Array([1, 2, 3]).buffer;
         log.push(error(() => postMessage(1, '*', [buffer, buffer])));
         // A transfer list that isn't a sequence.
         log.push(error(() => postMessage(1, '*', null)), error(() => postMessage(1, '*', {length: 1})));
         postMessage({buffer}, {targetOrigin: '*', transfer: [buffer]});
         log.push(buffer.byteLength);",
    );
    assert_eq!(
        log(&mut engine),
        "DataCloneError / DataCloneError / DataCloneError / DataCloneError / TypeError / TypeError / 0"
    );
    engine.run_posted_messages();
    assert_eq!(log(&mut engine), "1,2,3");
}

/// To a child frame's window: in its realm, from the parent (`source`).
#[test]
fn a_message_to_a_child_frame() {
    let mut engine = engine();
    run(
        &mut engine,
        "var f = document.body.appendChild(document.createElement('iframe'));
         var w = f.contentWindow;
         w.addEventListener('message', e => log.push(e.data, e.source === window,
                                                    Object.getPrototypeOf(e.data) === w.Array.prototype));
         w.postMessage([1], '*');",
    );
    engine.take_frame_requests().iter().for_each(|request| {
        assert!(
            matches!(request, FrameRequest::Created { .. }),
            "{request:?}"
        );
    });
    engine.run_posted_messages();
    assert_eq!(log(&mut engine), "1 / true / true");
}

/// A channel's ports: a message waits until its port is started
/// (`start()`, or `onmessage` set); none after `close()`; a port
/// transferred in a message works where it went, the one sent detached (a
/// `DataCloneError` sent again).
#[test]
fn a_channels_ports() {
    let mut engine = engine();
    run(
        &mut engine,
        "var c = new MessageChannel();
         log.push(String(c.port1), c.port1 instanceof EventTarget, c.port1 !== c.port2);
         c.port1.addEventListener('message', e => log.push('1 got ' + e.data + ' ' + (e.target === c.port1)));
         c.port2.postMessage('hi');",
    );
    assert_eq!(log(&mut engine), "[object MessagePort] / true / true");
    assert!(!engine.run_posted_messages(), "not started");
    run(&mut engine, "c.port1.start();");
    assert!(engine.run_posted_messages());
    assert_eq!(log(&mut engine), "1 got hi true");
    run(
        &mut engine,
        "c.port2.onmessage = e => log.push('2 got ' + e.data);
         c.port1.postMessage('back');",
    );
    engine.run_posted_messages();
    assert_eq!(log(&mut engine), "2 got back");
    // Transferred.
    run(
        &mut engine,
        "var d = new MessageChannel();
         onmessage = e => {
             log.push('ports ' + e.ports.length + ' ' + (e.data.port === e.ports[0]));
             e.ports[0].onmessage = m => log.push('moved got ' + m.data);
         };
         d.port1.postMessage('early');
         postMessage({port: d.port2}, '*', [d.port2]);
         log.push(error(() => postMessage('again', '*', [d.port2])));
         d.port1.postMessage('through');",
    );
    assert_eq!(log(&mut engine), "DataCloneError");
    engine.run_posted_messages();
    engine.run_posted_messages();
    assert_eq!(
        log(&mut engine),
        "ports 1 true / moved got early / moved got through"
    );
    // Sent, then the sender closed: delivered all the same; then no more.
    run(
        &mut engine,
        "c.port1.postMessage('before'); c.port1.close(); c.port2.postMessage('lost');",
    );
    assert!(engine.run_posted_messages());
    assert_eq!(log(&mut engine), "2 got before");
    run(&mut engine, "c.port1.postMessage('after');");
    assert!(!engine.run_posted_messages());
    // A port's options: its transfer list.
    run(
        &mut engine,
        "var e = new MessageChannel(), f = new MessageChannel();
         e.port2.onmessage = m => log.push('options ' + m.ports.length);
         e.port1.postMessage(1, {transfer: [f.port1]});
         log.push(error(() => e.port1.postMessage(1, [e.port1])));",
    );
    assert_eq!(log(&mut engine), "DataCloneError");
    engine.run_posted_messages();
    assert_eq!(log(&mut engine), "options 1");
}

/// `new MessageEvent(type, init)`: its `data` (null without one),
/// `origin`, `lastEventId`; no `source`, no ports.
#[test]
fn the_constructor() {
    let mut engine = engine();
    run(
        &mut engine,
        "var e = new MessageEvent('message', {data: {x: 1}, origin: 'o', lastEventId: 'l'});
         log.push(e.data.x, e.origin, e.lastEventId, String(e.source), e.ports.length, e.isTrusted,
                  String(new MessageEvent('m').data));",
    );
    assert_eq!(log(&mut engine), "1 / o / l / null / 0 / false / null");
}

/// `initMessageEvent` on `createEvent('MessageEvent')`'s and on a posted
/// message's event (nothing while it is dispatched; after, its data the one
/// given); its ports are message ports, its source a window.
#[test]
fn init_message_event() {
    let mut engine = engine();
    run(
        &mut engine,
        "var made = document.createEvent('MessageEvent');
         made.initMessageEvent('t', true, false, 5, 'o2', 'l2', null);
         log.push([made instanceof MessageEvent, made.type, made.bubbles, made.cancelable,
                   made.data, made.origin, made.lastEventId, made.source, made.ports.length].join());
         var channel = new MessageChannel();
         made.initMessageEvent('u', false, false, undefined, undefined, undefined, window, [channel.port1]);
         log.push([made.type, made.data, made.origin === '', made.source === window,
                   made.ports[0] === channel.port1].join());
         var none = document.createEvent('MessageEvent');
         none.initMessageEvent('v');
         log.push(String(none.data), none.origin === '', String(none.source));
         var name = f => { try { f(); return 'none'; } catch (e) { return e.name; } };
         log.push(name(() => made.initMessageEvent('w', false, false, 1, '', '', null, [window])),
                  name(() => made.initMessageEvent('w', false, false, 1, '', '', null, [new EventTarget()])),
                  name(() => made.initMessageEvent('w', false, false, 1, '', '', document)));
         onmessage = ev => {
           ev.initMessageEvent('during', false, false, 7);
           log.push([ev.type, ev.data].join());
           globalThis.posted = ev;
         };
         postMessage('posted', '*');",
    );
    engine.run_posted_messages();
    run(
        &mut engine,
        "posted.initMessageEvent('after', false, false, 9, 'o3');
         log.push([posted.type, posted.data, posted.origin, posted.source].join());",
    );
    assert_eq!(
        log(&mut engine),
        "true,t,true,false,5,o2,l2,,0 / u,,true,true,true / null / true / null / TypeError / TypeError / TypeError / \
         message,posted / after,9,o3,"
    );
}

/// A started port no script holds stays while it is entangled (its window
/// keeps it, Blink's `HasPendingActivity`): its messages come after a full
/// GC.
#[test]
fn a_started_port_stays() {
    let mut env = crate::env::Env::window("https://example.test/");
    env.eval(
        "var got = [];
         (function () {
             var c = new MessageChannel();
             c.port1.onmessage = e => got.push(e.data);
             c.port2.postMessage('queued');
         })(); 0",
    );
    env.full_gc();
    env.minor_gc();
    env.full_gc();
    assert_eq!(env.run_posted_messages(), 1);
    assert_eq!(env.eval("got.join()"), "queued");
}
