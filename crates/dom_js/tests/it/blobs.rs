//! File API's `Blob` from script: its constructor's parts and options,
//! `size`, `type`, `slice()`, and reading its bytes.

use std::time::Instant;

use dom_api::{ClassicScript, EngineSettings, ScriptEngine};
use dom_js::V8Engine;

/// What `source` pushed to `log`, joined with ` / `, its microtasks and
/// then its tasks run (a read settles in a task).
fn run(source: &str) -> String {
    let mut engine = V8Engine::new(EngineSettings { web_tests: true });
    engine
        .create_window(engine.top_frame(), "https://example.test/", Instant::now())
        .expect("a window");
    let run = |engine: &mut V8Engine, source: &str| {
        engine.run_classic_script(
            engine.top_frame(),
            &ClassicScript {
                source,
                url: "https://example.test/",
                base_url: None,
                line: 1,
                muted: false,
                use_credentials: false,
            },
        );
    };
    run(
        &mut engine,
        "var log = [];
         var error = f => {
             try { f(); return 'no error'; } catch (e) { return e.constructor.name; }
         };",
    );
    run(&mut engine, source);
    engine.perform_microtask_checkpoint();
    engine.run_due_tasks();
    run(
        &mut engine,
        "testRunner.reportHarnessResults(log.join(' / '))",
    );
    engine.take_harness_results().expect("reported")
}

/// Its parts' bytes in order: a string's UTF-8 (lone surrogates U+FFFD),
/// a buffer's or a view's as they were, a blob's, each converted as the
/// parts are iterated (what converting one runs changes what comes
/// after); a string's line endings
/// native with `endings: 'native'`. Its type lowercased, or empty with a
/// character outside U+0020 to U+007E. Parts that aren't an iterable
/// object (a string isn't one), or an `endings` that isn't one, a
/// `TypeError`.
#[test]
fn constructor() {
    assert_eq!(
        run("var empty = new Blob();
             log.push(typeof Blob, String(empty), empty.size, JSON.stringify(empty.type));
             var buffer = new Uint8Array([1, 2, 3, 4]).buffer;
             var b = new Blob(['ab', new Uint8Array(buffer, 1, 2), new Blob(['\\u00e9']), buffer, '\\ud800'],
                              {type: 'Text/Plain;Charset=UTF-8'});
             log.push(b.size, b.type);
             b.arrayBuffer().then(bytes => log.push(Array.from(new Uint8Array(bytes)).join()));
             log.push(new Blob(['a\\r\\nb\\rc'], {endings: 'native'}).size,
                      new Blob(['a\\r\\nb\\rc']).size, error(() => new Blob('abc')),
                      new Blob([], {type: 'caf\\u00e9'}).type === '');
             log.push(error(() => new Blob(1)), error(() => new Blob(null)),
                      error(() => new Blob([], {endings: 'Native'})), error(() => Blob([])),
                      error(() => new Blob(window)), error(() => new Blob(document.createElement('div'))));
             var parts = ['a', {toString() { parts.pop(); return 'b'; }}, 'c'];
             new Blob(parts).text().then(text => log.push(text));"),
        "function / [object Blob] / 0 / \"\" / 13 / text/plain;charset=utf-8 / 5 / 6 / TypeError / true / \
         TypeError / TypeError / TypeError / TypeError / TypeError / TypeError / \
         97,98,2,3,195,169,1,2,3,4,239,191,189 / ab"
    );
}

/// `slice(start, end, contentType)`: from the end if negative, clamped to
/// the blob, rounded and clamped as `[Clamp] long long`; its type none
/// (empty) unless given, normalized.
#[test]
fn slice() {
    assert_eq!(
        run("var b = new Blob(['abcdef'], {type: 'text/plain'});
             var read = blob => blob.text().then(text => log.push(`${text}:${blob.type}`));
             read(b.slice());
             read(b.slice(1, -1, 'A/B'));
             read(b.slice(-2));
             read(b.slice(4, 2));
             read(b.slice(1.5, 3.5));
             read(b.slice(-Infinity, NaN));
             read(b.slice(2).slice(1, 2));
             log.push(b.slice.length);"),
        "0 / abcdef: / bcde:a/b / ef: / : / cd: / : / d:"
    );
}

/// `text()` decodes UTF-8 (a BOM dropped), `arrayBuffer()` and `bytes()`
/// give new buffers each time, of the bytes.
#[test]
fn reading() {
    assert_eq!(
        run(
            "var b = new Blob([new Uint8Array([0xef, 0xbb, 0xbf, 0x68, 0xff])]);
             b.text().then(text => log.push(text, text.length));
             Promise.all([b.arrayBuffer(), b.arrayBuffer()]).then(([one, two]) =>
                 log.push(one instanceof ArrayBuffer, one !== two, one.byteLength));
             b.bytes().then(bytes => log.push(bytes instanceof Uint8Array, bytes.join()));"
        ),
        // Each read in a task of its own, in order: both buffers' before
        // the bytes'.
        "h\u{fffd} / 2 / true / true / 5 / true / 239,187,191,104,255"
    );
}

/// A member returning a promise rejects it where another throws (WebIDL's
/// operations and attributes of promise types): a `this` that isn't one
/// of its interface's, an argument that doesn't convert; its name and
/// `length` as any member's, and a right `this` reads as ever.
#[test]
fn promise_members_reject_what_they_throw() {
    assert_eq!(
        run(
            "var reasons = (...promises) => Promise.allSettled(promises).then(results =>
                 log.push(results.map(r => r.status + ':' + (r.reason && r.reason.name)).join()));
             reasons(Blob.prototype.text.call({}), Blob.prototype.bytes.call(document),
                     Element.prototype.scrollTo.call(window, 0, 0),
                     Object.getOwnPropertyDescriptor(Animation.prototype, 'ready').get.call({}),
                     new Blob(['a']).text());
             log.push(Blob.prototype.text.name, Blob.prototype.arrayBuffer.length, window.scrollBy.length,
                      Object.getOwnPropertyDescriptor(Animation.prototype, 'finished').get.name);
             reasons(document.createElement('div').scrollTo({ get left() { throw new RangeError('no'); } }));"
        ),
        "text / 0 / 0 / get finished / rejected:RangeError / \
         rejected:TypeError,rejected:TypeError,rejected:TypeError,rejected:TypeError,fulfilled:undefined"
    );
}

/// A read settles in a task of the window's (File API's "read the
/// blob"): after the microtasks queued before it and after the script that
/// asked, not as a microtask of its own.
#[test]
fn a_read_settles_in_a_task() {
    let mut engine = V8Engine::new(EngineSettings { web_tests: true });
    engine
        .create_window(engine.top_frame(), "https://example.test/", Instant::now())
        .expect("a window");
    let run = |engine: &mut V8Engine, source: &str| {
        engine.run_classic_script(
            engine.top_frame(),
            &ClassicScript {
                source,
                url: "https://example.test/",
                base_url: None,
                line: 1,
                muted: false,
                use_credentials: false,
            },
        );
    };
    run(
        &mut engine,
        "var log = [];
         new Blob(['a']).text().then(text => log.push('text ' + text));
         Promise.resolve().then(() => log.push('micro'));",
    );
    engine.perform_microtask_checkpoint();
    run(&mut engine, "log.push('before the task')");
    assert!(engine.dom_mut().has_queued_tasks(), "the read is a task");
    assert!(engine.run_due_tasks());
    run(
        &mut engine,
        "testRunner.reportHarnessResults(log.join(' / '))",
    );
    assert_eq!(
        engine.take_harness_results().as_deref(),
        Some("micro / before the task / text a")
    );
}
