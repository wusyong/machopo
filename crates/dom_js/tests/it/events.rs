//! `EventTarget` and `Event` from JS: their constructors (only with `new`,
//! subclassable), `EventInit` read as WebIDL reads a dictionary, an
//! event's members, and where nodes stand among them.

use crate::env::Env;

#[test]
fn new_event_target_and_new_event() {
    let mut env = Env::new();
    let cases = [
        (
            "var t = new EventTarget(); [t instanceof EventTarget, String(t), EventTarget.length].join()",
            "true,[object EventTarget],0",
        ),
        (
            "var e = new Event('load'); [e instanceof Event, e.type, e.bubbles, e.cancelable, \
             e.composed, e.isTrusted, e.eventPhase, String(e), Event.length].join()",
            "true,load,false,false,false,false,0,[object Event],1",
        ),
        (
            "var e = new Event('x', {bubbles: true, composed: 1}); [e.bubbles, e.cancelable, e.composed].join()",
            "true,false,true",
        ),
        ("new Event('x', undefined).bubbles", "false"),
        ("new Event('x', null).cancelable", "false"),
        ("new Event(null).type", "null"),
        (
            "[Event.NONE, Event.CAPTURING_PHASE, Event.prototype.AT_TARGET, Event.BUBBLING_PHASE].join()",
            "0,1,2,3",
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(env.eval(source), expected, "{source}");
    }
}

/// A constructor needs `new` and its required arguments; a dictionary
/// that isn't an object is a `TypeError`.
#[test]
fn constructors_check_their_calls() {
    let mut env = Env::new();
    let cases = [
        (
            "Event('x')",
            "Exception: TypeError: Failed to construct 'Event': Please use the 'new' operator, \
             this DOM object constructor cannot be called as a function.",
        ),
        (
            "new Event()",
            "Exception: TypeError: Failed to construct 'Event': 1 argument required, but only 0 present.",
        ),
        (
            "new Event('x', 1)",
            "Exception: TypeError: The provided value is not of type 'EventInit'.",
        ),
        (
            "EventTarget()",
            "Exception: TypeError: Failed to construct 'EventTarget': Please use the 'new' operator, this DOM object constructor cannot be called as a function.",
        ),
        ("new Node()", "Exception: TypeError: Illegal constructor"),
    ];
    for (source, expected) in cases {
        assert_eq!(env.eval(source), expected, "{source}");
    }
}

/// `EventInit`'s members are read in their names' order, getters run; one
/// that throws stops the construction.
#[test]
fn event_init_is_read_in_order_and_may_throw() {
    let mut env = Env::new();
    assert_eq!(
        env.eval(
            "var read = []; var init = {}; \
             for (let name of ['composed', 'cancelable', 'bubbles', 'other']) \
               Object.defineProperty(init, name, {get() { read.push(name); return true; }}); \
             var e = new Event('x', init); [read.join(' '), e.bubbles, e.cancelable, e.composed].join()"
        ),
        "bubbles cancelable composed,true,true,true"
    );
    assert_eq!(
        env.eval(
            "var made = 0; try { new Event('x', {get cancelable() { throw new RangeError('no'); }}); made = 1; } \
             catch (x) { String(x) + ' ' + made }"
        ),
        "RangeError: no 0"
    );
}

/// A subclass's instance is an event of its own prototype.
#[test]
fn events_and_targets_subclass() {
    let mut env = Env::new();
    assert_eq!(
        env.eval(
            "class Mine extends Event { constructor() { super('mine', {cancelable: true}); this.own = 1; } } \
             var m = new Mine(); class Target extends EventTarget {} var t = new Target(); \
             [m instanceof Mine, m instanceof Event, m.type, m.cancelable, m.own, \
              t instanceof Target, t instanceof EventTarget].join()"
        ),
        "true,true,mine,true,1,true,true"
    );
}

/// Nodes are event targets: `Node`'s interface object and prototype
/// inherit `EventTarget`'s; an event's members don't take a node.
#[test]
fn nodes_are_event_targets() {
    let mut env = Env::new();
    assert_eq!(
        env.eval(
            "var d = machopoDom.createDocument(); \
             [d instanceof EventTarget, d.createElement('p') instanceof EventTarget, \
              Object.getPrototypeOf(Node) === EventTarget, \
              Object.getPrototypeOf(Node.prototype) === EventTarget.prototype, \
              d instanceof Event].join()"
        ),
        "true,true,true,true,false"
    );
    assert_eq!(
        env.eval(
            "try { Object.getOwnPropertyDescriptor(Event.prototype, 'type').get.call(d) } \
             catch (x) { x.constructor.name }"
        ),
        "TypeError"
    );
}

/// `timeStamp`: a number of milliseconds, coarsened to 100 µs (not to a
/// whole one: of events made apart, some have a fraction), a later event's
/// no earlier.
#[test]
fn time_stamps_are_coarsened_milliseconds() {
    let mut env = Env::new();
    assert_eq!(
        env.eval(
            "var a = new Event('x').timeStamp; var until = Date.now() + 3; while (Date.now() < until); \
             var b = new Event('x').timeStamp; \
             [typeof a, a >= 0, b - a >= 1.9, Math.abs(a * 10 - Math.round(a * 10)) < 1e-6, \
              Math.abs(b * 10 - Math.round(b * 10)) < 1e-6].join()"
        ),
        "number,true,true,true,true"
    );
    assert_eq!(
        env.eval(
            "var fractions = 0; for (let i = 0; i < 50; i++) { \
               let spin = 0; for (let j = 0; j < 20000 + i * 997; j++) spin += j; \
               if (!Number.isInteger(new Event('x').timeStamp)) fractions++; } fractions > 0"
        ),
        "true"
    );
}

/// Events and targets made by script live while script holds them, and
/// are collected after.
#[test]
fn events_and_targets_are_collected_with_their_wrappers() {
    let mut env = Env::new();
    env.eval(
        "var kept = new Event('kept', {bubbles: true}); var target = new EventTarget(); \
         for (let i = 0; i < 1000; i++) new Event('dropped');",
    );
    let made = env.used_bytes();
    env.full_gc();
    env.full_gc();
    assert!(env.used_bytes() < made, "{} then {made}", env.used_bytes());
    assert_eq!(
        env.eval("[kept.type, kept.bubbles, target instanceof EventTarget].join()"),
        "kept,true,true"
    );
}

/// A tree in a new document: `d`, its `html`, a `div` in it; `log`, what
/// listeners record.
const TREE: &str = "var d = machopoDom.createDocument(); var html = d.createElement('html'); \
     var div = d.createElement('div'); d.appendChild(html); html.appendChild(div); var log = [];";

#[test]
fn listeners_are_called_down_and_back_up() {
    let mut env = Env::new();
    env.eval(TREE);
    assert_eq!(
        env.eval(
            "for (let [name, node] of [['d', d], ['html', html], ['div', div]]) { \
               node.addEventListener('x', function (e) { log.push(name + ' bubble ' + e.eventPhase + ' ' + (this === e.currentTarget)); }); \
               node.addEventListener('x', e => log.push(name + ' capture ' + e.eventPhase), true); } \
             var e = new Event('x', {bubbles: true}); var ok = div.dispatchEvent(e); \
             [ok, log.join(';'), e.target === div, e.currentTarget, e.eventPhase, e.srcElement === div].join('|')"
        ),
        "true|d capture 1;html capture 1;div capture 2;div bubble 2 true;html bubble 3 true;d bubble 3 true|true||0|true"
    );
}

/// A listener's event is the one dispatched (its wrapper); `composedPath()`
/// is the path while it runs, empty after; `target` stays.
#[test]
fn a_listener_sees_the_dispatch() {
    let mut env = Env::new();
    env.eval(TREE);
    assert_eq!(
        env.eval(
            "var e = new Event('x'); var seen; \
             div.addEventListener('x', ev => { seen = [ev === e, ev.composedPath().length, \
               ev.composedPath()[0] === div, ev.composedPath()[2] === d].join(); }); \
             div.dispatchEvent(e); [seen, e.composedPath().length, e.target === div].join('|')"
        ),
        "true,3,true,true|0|true"
    );
}

/// Adding the same callback again (same type and capture) adds nothing;
/// removing takes the one of its capture; `once` runs once; an object's
/// `handleEvent` is called with it as `this`.
#[test]
fn adding_removing_once_and_handle_event() {
    let mut env = Env::new();
    env.eval(TREE);
    assert_eq!(
        env.eval(
            "var f = () => log.push('f'); div.addEventListener('x', f); div.addEventListener('x', f, {}); \
             div.addEventListener('x', f, true); div.dispatchEvent(new Event('x')); \
             div.removeEventListener('x', f, {capture: true}); div.dispatchEvent(new Event('x')); \
             div.removeEventListener('x', f); div.dispatchEvent(new Event('x')); \
             div.addEventListener('x', () => log.push('once'), {once: true}); \
             div.dispatchEvent(new Event('x')); div.dispatchEvent(new Event('x')); \
             var handler = {handleEvent(e) { log.push('handled ' + (this === handler) + ' ' + e.type); }}; \
             div.addEventListener('y', handler); div.dispatchEvent(new Event('y')); \
             div.addEventListener('x', null); log.join()"
        ),
        "f,f,f,once,handled true y"
    );
}

/// Stopping propagation lets the current target's other listeners run;
/// stopping it immediately doesn't; the flags are cleared after.
#[test]
fn stopping_propagation() {
    let mut env = Env::new();
    env.eval(TREE);
    assert_eq!(
        env.eval(
            "var stop = 'stopPropagation'; \
             div.addEventListener('x', e => { log.push(1); e[stop](); }); \
             div.addEventListener('x', () => log.push(2)); html.addEventListener('x', () => log.push('html')); \
             var e = new Event('x', {bubbles: true}); div.dispatchEvent(e); \
             var cancelBubble = e.cancelBubble; stop = 'stopImmediatePropagation'; div.dispatchEvent(e); \
             [log.join(), cancelBubble].join('|')"
        ),
        "1,2,1|false"
    );
    assert_eq!(
        env.eval(
            "log = []; var p = d.createElement('p'); html.appendChild(p); \
             p.addEventListener('b', e => { e.cancelBubble = true; e.cancelBubble = false; log.push(e.cancelBubble); }); \
             html.addEventListener('b', () => log.push('html')); \
             p.dispatchEvent(new Event('b', {bubbles: true})); log.join()"
        ),
        "true"
    );
}

/// Canceling: only a cancelable event, not in a passive listener;
/// `returnValue` the other side of `defaultPrevented`.
#[test]
fn canceling() {
    let mut env = Env::new();
    env.eval(TREE);
    assert_eq!(
        env.eval(
            "div.addEventListener('c', e => e.preventDefault()); \
             div.addEventListener('p', e => { e.preventDefault(); log.push(e.defaultPrevented); }, {passive: true}); \
             div.addEventListener('r', e => { e.returnValue = false; log.push(e.returnValue); }); \
             [div.dispatchEvent(new Event('c', {cancelable: true})), div.dispatchEvent(new Event('c')), \
              div.dispatchEvent(new Event('p', {cancelable: true})), \
              div.dispatchEvent(new Event('r', {cancelable: true})), log.join()].join('|')"
        ),
        "false|true|true|false|false,false"
    );
}

/// A listener removed by an earlier one isn't called, one added during the
/// dispatch isn't either.
#[test]
fn listeners_changed_during_a_dispatch() {
    let mut env = Env::new();
    env.eval(TREE);
    assert_eq!(
        env.eval(
            "var later = () => log.push('later'); var added = () => log.push('added'); \
             div.addEventListener('x', () => { log.push('first'); div.removeEventListener('x', later); \
               div.addEventListener('x', added); }); div.addEventListener('x', later); \
             div.dispatchEvent(new Event('x')); log.push('|'); div.dispatchEvent(new Event('x')); log.join()"
        ),
        "first,|,first,added"
    );
}

/// An event being dispatched can't be dispatched again (an
/// `InvalidStateError`); another event can, in a listener; `initEvent`
/// does nothing while it is dispatched, and initializes it after.
#[test]
fn nested_dispatches_and_init_event() {
    let mut env = Env::new();
    env.eval(TREE);
    assert_eq!(
        env.eval(
            "var e = new Event('x'); div.addEventListener('x', ev => { \
               try { html.dispatchEvent(ev); } catch (x) { log.push(x.name); } \
               ev.initEvent('changed', true, true); log.push(ev.type); \
               html.dispatchEvent(new Event('inner')); }); \
             html.addEventListener('inner', () => log.push('inner')); div.dispatchEvent(e); \
             e.initEvent('again', true); [log.join(), e.type, e.bubbles, e.cancelable, e.target].join('|')"
        ),
        "InvalidStateError,x,inner|again|true|false|"
    );
    assert_eq!(
        env.eval("try { div.dispatchEvent(div) } catch (x) { x.constructor.name }"),
        "TypeError"
    );
}

/// What a listener throws is reported, not rethrown: the dispatch goes on;
/// an object without a callable `handleEvent` is reported too.
#[test]
fn listener_exceptions_are_reported() {
    let mut env = Env::new();
    env.eval(TREE);
    assert_eq!(
        env.eval(
            "div.addEventListener('x', () => { throw new Error('boom'); }); \
             div.addEventListener('x', {handleEvent: 1}); div.addEventListener('x', () => log.push('after')); \
             [div.dispatchEvent(new Event('x')), log.join()].join()"
        ),
        "true,after"
    );
    assert_eq!(env.uncaught_exceptions(), 2);
}

/// A callback that is a wrapper (an element, with no `handleEvent`) only
/// its listener holds survives V8's scavenges, which drop an unmodified
/// wrapper nothing else holds strongly: called, it is reported as not
/// callable.
#[test]
fn a_wrapper_as_a_callback_survives_scavenges() {
    let mut env = Env::new();
    env.eval(TREE);
    env.eval("div.addEventListener('w', d.createElement('p'));");
    env.minor_gc();
    env.minor_gc();
    assert_eq!(env.eval("div.dispatchEvent(new Event('w'))"), "true");
    assert_eq!(env.uncaught_exceptions(), 1);
}

/// A plain target's listeners, and its event's target.
#[test]
fn a_plain_targets_listeners() {
    let mut env = Env::new();
    assert_eq!(
        env.eval(
            "var t = new EventTarget(); var got; t.addEventListener('x', e => { got = [e.target === t, \
               e.currentTarget === t, e.eventPhase, e.composedPath().length].join(); }); \
             t.dispatchEvent(new Event('x', {bubbles: true})); got"
        ),
        "true,true,2,1"
    );
}

/// A callback only its listener holds survives V8's scavenges and full
/// GCs while the target lives; a target, its listener's closure holding
/// the target back, is collected once script lets go (two heaps, one
/// cycle).
#[test]
fn listeners_keep_their_callbacks_and_cycles_go() {
    let mut env = Env::new();
    env.eval(TREE);
    env.eval("div.addEventListener('x', (() => { const box = {n: 0}; return () => log.push(++box.n); })());");
    env.minor_gc();
    env.minor_gc();
    env.full_gc();
    assert_eq!(
        env.eval("div.dispatchEvent(new Event('x')); log.join()"),
        "1"
    );
    env.eval(
        "for (let i = 0; i < 200; i++) { const node = d.createElement('p'); \
           node.addEventListener('x', () => node.textContent = String(i)); }",
    );
    let made = env.used_bytes();
    env.full_gc();
    env.full_gc();
    assert!(env.used_bytes() < made, "{} then {made}", env.used_bytes());
    assert_eq!(
        env.eval("div.dispatchEvent(new Event('x')); log.join()"),
        "1,2"
    );
}

/// `window.event`: `undefined` outside a listener, the event inside one
/// (an event handler, a listener object, a listener of an `EventTarget` of
/// its own too), back to the outer one after a nested dispatch, restored
/// after a listener that threw; not set for a listener in a shadow tree
/// (DOM's "invocation-target-in-shadow-tree"). `[Replaceable]`: assigning
/// it makes a data property in its place.
#[test]
fn window_event() {
    let mut env = Env::window("about:blank");
    assert_eq!(
        env.eval(
            "var log = [];
             var body = document.appendChild(document.createElement('html')).appendChild(document.createElement('body'));
             var div = body.appendChild(document.createElement('div'));
             var span = div.appendChild(document.createElement('span'));
             var outer = new Event('outer', { bubbles: true, composed: true });
             var inner = new Event('inner');
             div.addEventListener('outer', () => {
                 log.push(window.event === outer);
                 div.dispatchEvent(inner);
                 log.push(window.event === outer);
             });
             div.addEventListener('inner', () => log.push(window.event === inner));
             body.onclick = () => log.push(event === window.event && window.event.type);
             body.addEventListener('outer', { handleEvent() { log.push(window.event === outer); } });
             var own = new EventTarget();
             own.addEventListener('x', () => { log.push(window.event.type); throw new Error('boom'); });
             log.push(String(window.event), 'event' in window);
             span.dispatchEvent(outer);
             body.dispatchEvent(new Event('click'));
             own.dispatchEvent(new Event('x'));
             log.push(String(window.event));
             var shadow = span.attachShadow({ mode: 'open' });
             var p = shadow.appendChild(document.createElement('p'));
             var seen = [];
             p.addEventListener('s', () => seen.push(String(window.event)));
             shadow.addEventListener('s', () => seen.push(String(window.event)));
             span.addEventListener('s', (e) => seen.push(window.event === e));
             p.dispatchEvent(new Event('s', { bubbles: true, composed: true }));
             log.push(seen.join('|'));
             log.join(' ')"
        ),
        "undefined true true true true true click x undefined undefined|undefined|true"
    );
    assert_eq!(
        env.eval(
            "var d = Object.getOwnPropertyDescriptor(window, 'event');
             var before = [typeof d.get, typeof d.set, d.configurable].join();
             window.event = 5;
             var after = Object.getOwnPropertyDescriptor(window, 'event');
             self = 'mine';
             [before, after.value, after.writable, typeof after.get, self].join()"
        ),
        "function,function,true,5,true,undefined,mine"
    );
}

/// An ancestor's unforgeable members are each object's own, and only they:
/// a `MouseEvent`'s `isTrusted` (`Event`'s), a document's `location`; its
/// other members stay on its own prototype.
#[test]
fn unforgeable_members_of_ancestors() {
    let mut env = Env::window("about:blank");
    assert_eq!(
        env.eval(
            "[Object.getOwnPropertyNames(new MouseEvent('x')), Object.getOwnPropertyNames(new FocusEvent('x')), \
              Object.getOwnPropertyNames(document), \
              ['type', 'preventDefault', 'isTrusted'].filter((name) => MouseEvent.prototype.hasOwnProperty(name))].join(' / ')"
        ),
        "isTrusted / isTrusted / location / "
    );
}

/// The microtasks a listener of an event the UA dispatches queues run as
/// it returns, before the next listener, while `window.event` is still the
/// event (HTML's "call a user object's operation" cleans up inside "inner
/// invoke").
#[test]
fn microtasks_after_a_listener_see_its_event() {
    let mut page = crate::history::Page::new("https://example.test/");
    page.eval(
        "var seen = []; \
         addEventListener('load', e => { Promise.resolve().then(() => seen.push(window.event === e)); }); \
         addEventListener('load', () => seen.push('second', window.event.type)); 0",
    );
    page.load();
    assert_eq!(page.eval("seen.join()"), "true,second,load");
}
