//! `AbortController` and `AbortSignal`: aborting (the reason given, or a
//! new `AbortError`; once), the abort steps' order (algorithms, then
//! `abort` at the signal, then at its dependents), `AbortSignal.abort()`,
//! `timeout()` (a `TimeoutError` as its timer fires), `any()` (its
//! sources flattened, aborted already, aborted by any), `throwIfAborted()`,
//! `addEventListener`'s `signal`; and what a GC leaves of `any()`'s
//! signals: one with an `abort` listener stays while its source does, one
//! with none goes.

use crate::history::Page;

#[test]
fn a_controller_aborts_its_signal() {
    let mut page = Page::new("https://example.test/");
    assert_eq!(
        page.eval(
            "var log = []; var c = new AbortController(); var s = c.signal;
             s.onabort = e => log.push(['onabort', e.type, e.isTrusted, e.bubbles, e.cancelable, s.aborted].join(' '));
             s.addEventListener('abort', () => log.push('listener'));
             var r = [s === c.signal, s instanceof AbortSignal, s instanceof EventTarget, s.aborted, String(s.reason)];
             c.abort(); c.abort('again');
             r.push(s.aborted, s.reason instanceof DOMException, s.reason.name, s.reason.message, s.reason === s.reason, log.join(' | '));
             var c2 = new AbortController(); c2.abort(5); r.push(c2.signal.reason);
             var c3 = new AbortController(); c3.abort(undefined); r.push(c3.signal.reason.name);
             try { s.throwIfAborted(); } catch (e) { r.push(e === s.reason); }
             new AbortController().signal.throwIfAborted(); r.join()"
        ),
        "true,true,true,false,undefined,true,true,AbortError,signal is aborted without reason,true,onabort abort true false false true | listener,5,AbortError,true"
    );
    assert_eq!(
        page.eval(
            "var a = AbortSignal.abort(), b = AbortSignal.abort('why'); var r = [a.aborted, a.reason.name, b.reason];
             try { AbortSignal.abort.call(null); r.push('static'); } catch (e) { r.push(e.name); }
             for (var t of [() => AbortSignal.timeout(-1), () => AbortSignal.timeout(NaN), () => AbortSignal.any([1]), () => AbortSignal.any([new EventTarget()]), () => AbortSignal.any(5)]) {
                 try { t(); r.push('ok'); } catch (e) { r.push(e.name); }
             } r.join()"
        ),
        "true,AbortError,why,static,TypeError,TypeError,TypeError,TypeError,TypeError"
    );
}

#[test]
fn a_timeout_aborts_as_its_timer_fires() {
    let mut env = crate::env::Env::window("https://example.test/");
    env.eval(
        "var log = []; var s = AbortSignal.timeout(10);
         s.onabort = () => log.push([s.reason.name, s.reason.message, s.reason instanceof DOMException].join());
         var gone = AbortSignal.timeout(10); gone.onabort = () => log.push('second'); 0",
    );
    assert_eq!(env.eval("log.join() + s.aborted"), "false");
    let due = env.next_timer().expect("the timeouts' timer");
    assert_eq!(
        env.run_timers(Some(due - 1.0)),
        0,
        "not before its milliseconds"
    );
    // Past both: the second was made after the first, a millisecond or
    // more later where the tests load the machine.
    env.run_timers(Some(due + 1000.0));
    assert_eq!(
        env.eval("log.join(' | ')"),
        "TimeoutError,signal timed out,true | second"
    );
}

#[test]
fn any_follows_its_sources() {
    let mut page = Page::new("https://example.test/");
    assert_eq!(
        page.eval(
            "var log = []; var a = new AbortController(), b = new AbortController();
             var any = AbortSignal.any([a.signal, b.signal]); var nested = AbortSignal.any([any]);
             a.signal.onabort = () => log.push('a ' + any.aborted + ' ' + nested.aborted);
             any.onabort = () => log.push('any ' + (any.reason === a.signal.reason));
             nested.onabort = () => log.push('nested ' + (nested.reason === 'first'));
             var r = [any.aborted, String(any.reason), AbortSignal.any([]).aborted];
             a.abort('first'); b.abort('second');
             r.push(log.join(' | '), any.reason, nested.reason);
             var early = AbortSignal.any([AbortSignal.abort('done'), new AbortController().signal]);
             r.push(early.aborted, early.reason); r.join()"
        ),
        "false,undefined,false,a true true | any true | nested true,first,first,true,done"
    );
}

#[test]
fn a_signal_removes_its_listeners() {
    let mut page = Page::new("https://example.test/");
    assert_eq!(
        page.eval(
            "var log = []; var c = new AbortController(); var t = new EventTarget();
             t.addEventListener('x', () => log.push('x'), { signal: c.signal });
             document.addEventListener('y', () => log.push('y'), { signal: c.signal, once: true });
             t.dispatchEvent(new Event('x')); c.abort(); t.dispatchEvent(new Event('x'));
             document.dispatchEvent(new Event('y'));
             t.addEventListener('x', () => log.push('late'), { signal: c.signal }); t.dispatchEvent(new Event('x'));
             var r = [log.join()];
             try { t.addEventListener('x', () => {}, { signal: t }); } catch (e) { r.push(e.name); }
             try { t.addEventListener('x', () => {}, { signal: 1 }); } catch (e) { r.push(e.name); }
             try { t.addEventListener('x', () => {}, { signal: null }); } catch (e) { r.push(e.name); }
             r.join()"
        ),
        "x,TypeError,TypeError,TypeError"
    );
}

#[test]
fn a_dependent_with_a_listener_stays_and_one_without_goes() {
    let mut env = crate::env::Env::window("https://example.test/");
    env.eval(
        "var log = []; var c = new AbortController();
         (function () {
             AbortSignal.any([c.signal]).addEventListener('abort', () => log.push('kept'));
             var other = AbortSignal.any([c.signal]); other.onabort = () => log.push('handler');
         })();
         var weak = (function () { return new WeakRef(AbortSignal.any([c.signal])); })();
         var unlistened = (function () { var s = AbortSignal.any([c.signal]); s.onabort = () => {}; s.onabort = null; return new WeakRef(s); })(); 0",
    );
    env.checkpoint();
    env.full_gc();
    env.minor_gc();
    env.full_gc();
    assert_eq!(
        env.eval("String(weak.deref()) + ' ' + String(unlistened.deref())"),
        "undefined undefined",
        "none listening: gone while their source stays"
    );
    env.eval("c.abort(); 0");
    assert_eq!(env.eval("log.join()"), "kept,handler");
}
