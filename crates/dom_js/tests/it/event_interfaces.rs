//! `CustomEvent`, `ErrorEvent`, `WheelEvent`, `InputEvent`, `TextEvent`, `document.createEvent` and
//! the legacy `init*Event`s from script: their constructors and members, a
//! `detail` kept through GCs, the interfaces `createEvent` makes and
//! refuses, an uninitialized event's dispatch, and what each `init*Event`
//! sets and keeps.

use crate::env::Env;

fn check(env: &mut Env, cases: &[(&str, &str)]) {
    for (script, expected) in cases {
        assert_eq!(env.eval(script), *expected, "{script}");
    }
}

#[test]
fn custom_events_carry_a_detail() {
    let mut env = Env::window("about:blank");
    check(
        &mut env,
        &[
            (
                "var o = { a: 1 }; var c = new CustomEvent('c', { detail: o, bubbles: true }); \
                 [c.type, c.bubbles, c.detail === o, c instanceof CustomEvent, c instanceof Event].join()",
                "c,true,true,true,true",
            ),
            // `any detail = null`: missing and undefined are null.
            (
                "[new CustomEvent('c').detail === null, new CustomEvent('c', {}).detail === null, \
                  new CustomEvent('c', { detail: undefined }).detail === null, new CustomEvent('c', { detail: 0 }).detail].join()",
                "true,true,true,0",
            ),
            (
                "var log = []; var t = new EventTarget(); t.addEventListener('c', (e) => log.push(e.detail)); \
                 t.dispatchEvent(new CustomEvent('c', { detail: 'x' })); log.join()",
                "x",
            ),
            (
                "c.initCustomEvent('d', false, true, 2); [c.type, c.bubbles, c.cancelable, c.detail].join()",
                "d,false,true,2",
            ),
            ("c.initCustomEvent('e'); c.detail === null", "true"),
        ],
    );
    // The detail is the event's: GCs keep it.
    env.eval("var kept = {}; var k = new CustomEvent('k', { detail: kept });");
    env.minor_gc();
    env.full_gc();
    assert_eq!(env.eval("k.detail === kept"), "true");
}

/// `new ErrorEvent()`: its members as given, in their names' order, their
/// defaults empty, 0 and undefined; untrusted, not cancelable unless asked.
#[test]
fn error_events_are_made_from_their_init() {
    let mut env = Env::window("about:blank");
    check(
        &mut env,
        &[
            (
                "var x = {}; var e = new ErrorEvent('oops', { message: 'm', filename: 'f.js', lineno: 3, colno: 4, error: x, bubbles: true }); \
                 [e.type, e.message, e.filename, e.lineno, e.colno, e.error === x, e.bubbles, e.cancelable, e.isTrusted, \
                  e instanceof ErrorEvent].join()",
                "oops,m,f.js,3,4,true,true,false,false,true",
            ),
            (
                "var d = new ErrorEvent('error'); [JSON.stringify(d.message), JSON.stringify(d.filename), d.lineno, d.colno, \
                  d.error === undefined].join()",
                "\"\",\"\",0,0,true",
            ),
            (
                "var order = []; var init = {}; for (const k of ['message', 'lineno', 'filename', 'error', 'colno']) \
                   Object.defineProperty(init, k, { get() { order.push(k); return undefined; } }); \
                 new ErrorEvent('e', init); order.join()",
                "colno,error,filename,lineno,message",
            ),
        ],
    );
}

#[test]
fn create_event_makes_uninitialized_events() {
    let mut env = Env::window("about:blank");
    check(
        &mut env,
        &[
            (
                "var e = document.createEvent('Event'); [JSON.stringify(e.type), e.bubbles, e.isTrusted, e instanceof Event].join()",
                "\"\",false,false,true",
            ),
            // DOM's table, ASCII case-insensitively.
            (
                "['event', 'Events', 'HTMLEvents', 'SVGEvents', 'UIEvent', 'uievents', 'MouseEvent', 'MOUSEEVENTS', \
                  'KeyboardEvent', 'FocusEvent', 'CustomEvent'].map((n) => document.createEvent(n).constructor.name).join()",
                "Event,Event,Event,Event,UIEvent,UIEvent,MouseEvent,MouseEvent,KeyboardEvent,FocusEvent,CustomEvent",
            ),
            // Not in the table, or of an interface machopo hasn't; `TextEvent`
            // and `MessageEvent` are in it.
            (
                "['WheelEvent', 'InputEvent', 'TextEvent', 'MessageEvent', 'Nope'].map((n) => { \
                   try { document.createEvent(n); return 'made' } catch (err) { return err.name } }).join()",
                "NotSupportedError,NotSupportedError,made,made,NotSupportedError",
            ),
            (
                "try { document.dispatchEvent(e); 'dispatched' } catch (err) { err.name }",
                "InvalidStateError",
            ),
            (
                "var heard = 0; document.addEventListener('x', () => heard++); e.initEvent('x', true, false); \
                 [document.dispatchEvent(e), heard, e.bubbles].join()",
                "true,1,true",
            ),
            (
                "var c = document.createEvent('CustomEvent'); [c.detail === null, c instanceof CustomEvent].join()",
                "true,true",
            ),
            (
                "var m = document.createEvent('MouseEvents'); [m.clientX, m.button, m.relatedTarget, m.view].join()",
                "0,0,,",
            ),
        ],
    );
}

#[test]
fn init_events_set_what_they_take() {
    let mut env = Env::window("about:blank");
    check(
        &mut env,
        &[
            (
                "var u = document.createEvent('UIEvent'); u.initUIEvent('u', true, true, window, 7); \
                 [u.type, u.bubbles, u.cancelable, u.view === window, u.detail].join()",
                "u,true,true,true,7",
            ),
            (
                "try { u.initUIEvent('u', false, false, {}, 0); 'set' } catch (err) { err instanceof TypeError }",
                "true",
            ),
            // Of any UI event, its own data kept.
            (
                "var uw = new WheelEvent('w', { deltaY: 2 }); uw.initUIEvent('v', false, false, window, 4); \
                 var uk = new KeyboardEvent('k', { key: 'q' }); uk.initUIEvent('k2', false, false, window, 6); \
                 var ui = new InputEvent('i', { data: 'z' }); ui.initUIEvent('i2', false, false, window, 8); \
                 var uf = new FocusEvent('f'); uf.initUIEvent('f2', false, false, window, 9); \
                 [uw.detail, uw.view === window, uw.deltaY, uk.detail, uk.view === window, uk.key, ui.detail, ui.view === window, \
                  ui.data, uf.detail, uf.view === window].join()",
                "4,true,2,6,true,q,8,true,z,9,true",
            ),
            // WebIDL `long`s; a `short` button, modulo its size.
            (
                "var m = document.createEvent('MouseEvent'); \
                 m.initMouseEvent('click', true, true, window, 2, 1, 2, 3.9, 4, true, false, true, false, 65538, document); \
                 [m.type, m.detail, m.screenX, m.screenY, m.clientX, m.clientY, m.ctrlKey, m.altKey, m.shiftKey, m.metaKey, \
                  m.button, m.relatedTarget === document, m.view === window].join()",
                "click,2,1,2,3,4,true,false,true,false,2,true,true",
            ),
            // Any event target is a related target.
            (
                "var t = new EventTarget(); m.initMouseEvent('x', false, false, null, 0, 0, 0, 0, 0, false, false, false, false, 0, t); \
                 var r1 = m.relatedTarget === t; \
                 m.initMouseEvent('x', false, false, null, 0, 0, 0, 0, 0, false, false, false, false, 0, window); \
                 [r1, m.relatedTarget === window, m.view].join()",
                "true,true,",
            ),
            (
                "try { m.initMouseEvent('x', false, false, null, 0, 0, 0, 0, 0, false, false, false, false, 0, {}); 'set' } \
                 catch (err) { err instanceof TypeError }",
                "true",
            ),
            // Only the four keys it takes are down after it; `buttons` and
            // the movement are kept.
            (
                "var b = new MouseEvent('b', { buttons: 4, movementX: 3, modifierCapsLock: true, altKey: true }); \
                 b.initMouseEvent('b', false, false, null, 0, 0, 0, 0, 0, true, false, false, false, 0, null); \
                 [b.buttons, b.movementX, b.getModifierState('CapsLock'), b.altKey, b.ctrlKey].join()",
                "4,3,false,false,true",
            ),
            // A wheel event stays one, its deltas kept.
            (
                "var w = new WheelEvent('wheel', { deltaY: 3 }); \
                 w.initMouseEvent('w2', false, false, null, 0, 0, 0, 5, 0, false, false, false, false, 1, null); \
                 [w instanceof WheelEvent, w.deltaY, w.clientX, w.button, w.type].join()",
                "true,3,5,1,w2",
            ),
            (
                "var k = document.createEvent('KeyboardEvent'); \
                 k.initKeyboardEvent('keydown', true, true, window, 'a', 1, true, true, false, false); \
                 [k.type, k.key, k.location, k.ctrlKey, k.altKey, k.shiftKey, k.metaKey, k.detail, k.view === window].join()",
                "keydown,a,1,true,true,false,false,0,true",
            ),
            // `optional DOMString keyArg = ""`: undefined is the default,
            // null a string.
            (
                "k.initKeyboardEvent('k'); var k1 = JSON.stringify(k.key); k.initKeyboardEvent('k', false, false, null, null); [k1, k.key].join()",
                "\"\",null",
            ),
            (
                "var kc = new KeyboardEvent('k', { code: 'KeyA', keyCode: 65, modifierCapsLock: true, detail: 5 }); \
                 kc.initKeyboardEvent('k', false, false, null, 'b'); \
                 [kc.code, kc.keyCode, kc.getModifierState('CapsLock'), kc.key, kc.detail].join()",
                "KeyA,65,false,b,0",
            ),
        ],
    );
}

/// An `init*Event` while the event is dispatched changes nothing.
#[test]
fn init_events_do_nothing_while_dispatched() {
    let mut env = Env::window("about:blank");
    let cases = [
        "e.initEvent('other', false, false)",
        "e.initUIEvent('other', false, false, null, 9)",
        "e.initMouseEvent('other', false, false, null, 9, 0, 0, 9, 0, false, false, false, false, 0, null)",
    ];
    for init in cases {
        let script = format!(
            "var t = new EventTarget(); var e = new MouseEvent('m', {{ clientX: 1, detail: 2 }}); \
             t.addEventListener('m', () => {{ {init} }}); t.dispatchEvent(e); [e.type, e.clientX, e.detail].join()"
        );
        assert_eq!(env.eval(&script), "m,1,2", "{init}");
    }
    assert_eq!(
        env.eval(
            "var t = new EventTarget(); var c = new CustomEvent('c', { detail: 1 }); \
             t.addEventListener('c', () => c.initCustomEvent('other', false, false, 2)); t.dispatchEvent(c); [c.type, c.detail].join()"
        ),
        "c,1"
    );
}

#[test]
fn wheel_events_are_made_and_read() {
    let mut env = Env::window("about:blank");
    check(
        &mut env,
        &[
            (
                "var w = new WheelEvent('wheel', { deltaX: 1.5, deltaY: -40, deltaZ: 2, deltaMode: 1, clientX: 3, ctrlKey: true, \
                   button: 1, bubbles: true }); \
                 [w.type, w.deltaX, w.deltaY, w.deltaZ, w.deltaMode, w.clientX, w.ctrlKey, w.button, w.which, w.bubbles].join()",
                "wheel,1.5,-40,2,1,3,true,1,2,true",
            ),
            // Chrome's legacy deltas: the other way, whole.
            (
                "[w.wheelDeltaX, w.wheelDeltaY, w.wheelDelta].join()",
                "-1,40,40",
            ),
            ("new WheelEvent('w', { deltaX: 120 }).wheelDelta", "-120"),
            (
                "var z = new WheelEvent('w'); [z.deltaX, z.deltaY, z.deltaZ, z.deltaMode, z.wheelDelta].join()",
                "0,0,0,0,0",
            ),
            (
                "[WheelEvent.DOM_DELTA_PIXEL, WheelEvent.DOM_DELTA_LINE, WheelEvent.DOM_DELTA_PAGE, w.DOM_DELTA_PAGE].join()",
                "0,1,2,2",
            ),
            (
                "[w instanceof WheelEvent, w instanceof MouseEvent, w instanceof UIEvent, \
                  Object.getPrototypeOf(WheelEvent.prototype) === MouseEvent.prototype, \
                  Object.getOwnPropertyDescriptor(MouseEvent.prototype, 'clientX').get.call(w), \
                  Object.getOwnPropertyDescriptor(UIEvent.prototype, 'detail').get.call(w)].join()",
                "true,true,true,true,3,0",
            ),
            (
                "try { new WheelEvent('w', { deltaY: NaN }); 'made' } catch (err) { err instanceof TypeError }",
                "true",
            ),
            // A brand check: a mouse event isn't a wheel event.
            (
                "try { Object.getOwnPropertyDescriptor(WheelEvent.prototype, 'deltaY').get.call(new MouseEvent('m')); 'read' } \
                 catch (err) { err instanceof TypeError }",
                "true",
            ),
        ],
    );
}

#[test]
fn input_events_are_made_and_read() {
    let mut env = Env::window("about:blank");
    check(
        &mut env,
        &[
            (
                "var i = new InputEvent('beforeinput', { data: 'a', inputType: 'insertText', isComposing: true, cancelable: true, detail: 2 }); \
                 [i.type, i.data, i.inputType, i.isComposing, i.cancelable, i.detail, i instanceof InputEvent, i instanceof UIEvent].join()",
                "beforeinput,a,insertText,true,true,2,true,true",
            ),
            // `DOMString? data = null`.
            (
                "var d = new InputEvent('input'); [d.data === null, JSON.stringify(d.inputType), d.isComposing].join()",
                "true,\"\",false",
            ),
            (
                "[new InputEvent('i', { data: null }).data === null, new InputEvent('i', { data: undefined }).data === null, \
                  new InputEvent('i', { data: 5 }).data].join()",
                "true,true,5",
            ),
            (
                "try { Object.getOwnPropertyDescriptor(InputEvent.prototype, 'data').get.call(new UIEvent('u')); 'read' } \
                 catch (err) { err instanceof TypeError }",
                "true",
            ),
        ],
    );
}

/// The legacy `TextEvent` (`textInput`'s): `createEvent`'s only, its
/// `data` and `initTextEvent` (`"undefined"` without data; nothing while
/// it is dispatched).
#[test]
fn text_events() {
    let mut env = Env::window("about:blank");
    check(
        &mut env,
        &[
            (
                "var t = document.createEvent('TextEvent'); [t instanceof TextEvent, t instanceof UIEvent, JSON.stringify(t.data), typeof TextEvent].join()",
                "true,true,\"\",function",
            ),
            (
                "var c; try { new TextEvent('t'); c = 'made'; } catch (e) { c = e.name; } c",
                "TypeError",
            ),
            (
                "t.initTextEvent('textInput', true, true, window, 'abc'); [t.type, t.bubbles, t.cancelable, t.view === window, t.data].join()",
                "textInput,true,true,true,abc",
            ),
            (
                "t.initTextEvent('x'); [t.type, t.data, t.bubbles, t.view].join()",
                "x,undefined,false,",
            ),
            (
                "var during; document.addEventListener('x', e => { e.initTextEvent('y', false, false, null, 'z'); during = e.type + e.data; }); \
                 document.dispatchEvent(t); during",
                "xundefined",
            ),
        ],
    );
}

/// `TransitionEvent` and `AnimationEvent`: their constructors (the init's
/// members, and their defaults), members, prototypes; `elapsedTime` a
/// restricted `double`.
#[test]
fn transition_and_animation_events() {
    let mut env = Env::window("about:blank");
    check(
        &mut env,
        &[
            (
                "var t = new TransitionEvent('transitionend', { propertyName: 'width', elapsedTime: 1.5, \
                 pseudoElement: '::before', bubbles: true }); \
                 [t.type, t.propertyName, t.elapsedTime, t.pseudoElement, t.bubbles, t.isTrusted, \
                  t instanceof TransitionEvent, t instanceof Event, t instanceof AnimationEvent].join()",
                "transitionend,width,1.5,::before,true,false,true,true,false",
            ),
            (
                "var a = new AnimationEvent('animationstart', { animationName: 'spin', elapsedTime: 2 }); \
                 [a.animationName, a.elapsedTime, a.pseudoElement, a instanceof AnimationEvent, \
                  a.propertyName].join()",
                "spin,2,,true,",
            ),
            (
                "var d = new TransitionEvent('x'); [d.propertyName, d.elapsedTime, d.pseudoElement, \
                 new AnimationEvent('y').animationName].join('|')",
                "|0||",
            ),
            (
                "try { new TransitionEvent('x', { elapsedTime: NaN }); 'none' } catch (e) { e.name }",
                "TypeError",
            ),
        ],
    );
}
