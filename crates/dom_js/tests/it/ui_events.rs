//! `UIEvent`, `MouseEvent` and `KeyboardEvent` from script: their constructors (the init
//! dictionaries' numbers converted as WebIDL's types), members, interfaces
//! and prototypes.

use crate::env::Env;

#[test]
fn mouse_events_are_made_and_read() {
    let mut env = Env::window("about:blank");
    let cases = [
        (
            "var e = new MouseEvent('click', { bubbles: true, clientX: 10.5, clientY: 20, screenX: 1, screenY: 2, \
               movementX: -3, movementY: 4, button: 2, buttons: 5, ctrlKey: true, metaKey: true, detail: 3, modifierCapsLock: true }); \
             [e.type, e.bubbles, e.clientX, e.x, e.clientY, e.y, e.screenX, e.screenY, e.movementX, e.movementY, e.button, e.buttons, \
              e.ctrlKey, e.shiftKey, e.altKey, e.metaKey, e.detail, e.which, e.view, e.relatedTarget, e.isTrusted].join()",
            "click,true,10.5,10.5,20,20,1,2,-3,4,2,5,true,false,false,true,3,3,,,false",
        ),
        (
            "[e.getModifierState('Control'), e.getModifierState('CapsLock'), e.getModifierState('Shift'), e.getModifierState('control')].join()",
            "true,true,false,false",
        ),
        (
            "[e instanceof MouseEvent, e instanceof UIEvent, e instanceof Event, Object.getPrototypeOf(e) === MouseEvent.prototype, \
              Object.getPrototypeOf(MouseEvent.prototype) === UIEvent.prototype].join()",
            "true,true,true,true,true",
        ),
        // No layout: page and offset coordinates are the client's.
        (
            "[e.pageX, e.pageY, e.offsetX, e.offsetY].join()",
            "10.5,20,10.5,20",
        ),
        // WebIDL's integers: modulo their sizes.
        (
            "var m = new MouseEvent('x', { button: 65537, buttons: -1, detail: 4294967297.7 }); [m.button, m.buttons, m.detail, m.which].join()",
            "1,65535,1,2",
        ),
        ("new MouseEvent('x', { button: -1 }).which", "0"),
        (
            "var s = new MouseEvent('x', { shiftKey: true, altKey: true }); [s.shiftKey, s.ctrlKey, s.altKey, s.metaKey].join()",
            "true,false,true,false",
        ),
        (
            "try { new MouseEvent('x', { clientX: NaN }); 'no' } catch (e) { e instanceof TypeError }",
            "true",
        ),
        (
            "var d = new MouseEvent('x'); [d.button, d.buttons, d.clientX, d.detail, d.bubbles].join()",
            "0,0,0,0,false",
        ),
        (
            "var u = new UIEvent('u', { detail: -2, cancelable: true }); [u.detail, u.cancelable, u.view, u.which, u instanceof UIEvent, u instanceof MouseEvent].join()",
            "-2,true,,0,true,false",
        ),
        (
            "try { UIEvent('u') } catch (e) { e instanceof TypeError }",
            "true",
        ),
        // A brand check: `UIEvent`'s getters take a mouse event, not a plain event.
        (
            "var get = Object.getOwnPropertyDescriptor(UIEvent.prototype, 'detail').get; \
             [get.call(e), (() => { try { get.call(new Event('x')); return 'no' } catch (err) { return err instanceof TypeError } })()].join()",
            "3,true",
        ),
    ];
    for (script, expected) in cases {
        assert_eq!(env.eval(script), expected, "{script}");
    }
}

/// `fromElement` and `toElement`, as Chrome's: a `mouseout`'s (or
/// `mouseleave`'s) target and `relatedTarget`, any other event's the other
/// way round; `null` for what isn't a node.
#[test]
fn from_element_and_to_element() {
    let mut env = Env::window("about:blank");
    assert_eq!(
        env.eval(
            "document.appendChild(document.createElement('html')).appendChild(document.createElement('body'));
             const a = document.body.appendChild(document.createElement('p'));
             const b = document.body.appendChild(document.createElement('p'));
             a.id = 'a';
             b.id = 'b';
             const name = n => n ? n.id || n.constructor.name : String(n);
             const pair = e => name(e.fromElement) + '/' + name(e.toElement);
             const at = (type, related, target) => {
               const e = new MouseEvent(type, { relatedTarget: related });
               let seen;
               target.addEventListener(type, () => seen = pair(e), { once: true });
               target.dispatchEvent(e);
               return seen;
             };
             [pair(new MouseEvent('mouseover', { relatedTarget: a })),
              at('mouseout', a, b), at('mouseleave', a, b), at('mouseover', a, b),
              at('click', a, b), at('mouseout', window, b), at('mouseover', null, window),
              'fromElement' in MouseEvent.prototype, 'fromElement' in UIEvent.prototype].join()"
        ),
        "a/null,b/a,b/a,a/b,a/b,b/null,null/null,true,false"
    );
}

/// The init dictionaries' `view` (a window) and `relatedTarget` (any event
/// target), null when null or missing; another value a `TypeError`.
#[test]
fn init_dictionaries_give_view_and_related_target() {
    let mut env = Env::window("about:blank");
    let cases = [
        (
            "var p = document.createElement('p'); var t = new EventTarget(); \
             [new UIEvent('x', { view: window }).view === window, new KeyboardEvent('x', { view: window }).view === window, \
              new InputEvent('x', { view: window }).view === window, new FocusEvent('x', { view: window }).view === window, \
              new MouseEvent('x', { view: window, relatedTarget: p }).relatedTarget === p, \
              new WheelEvent('x', { relatedTarget: window }).relatedTarget === window, \
              new FocusEvent('x', { relatedTarget: t }).relatedTarget === t, \
              new MouseEvent('x', { view: null, relatedTarget: null }).view, new MouseEvent('x').relatedTarget].join()",
            "true,true,true,true,true,true,true,,",
        ),
        (
            "var error = f => { try { f(); return 'none'; } catch (e) { return e.name; } }; \
             [error(() => new UIEvent('x', { view: document })), error(() => new MouseEvent('x', { relatedTarget: {} })), \
              error(() => new FocusEvent('x', { relatedTarget: 1 }))].join()",
            "TypeError,TypeError,TypeError",
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(env.eval(source), expected, "{source}");
    }
}

#[test]
fn keyboard_events_are_made_and_read() {
    let mut env = Env::window("about:blank");
    let cases = [
        (
            "var k = new KeyboardEvent('keydown', { key: 'a', code: 'KeyA', location: 2, repeat: true, isComposing: true, \
               ctrlKey: true, shiftKey: true, keyCode: 65, charCode: 97, detail: 1, modifierNumLock: true }); \
             [k.type, k.key, k.code, k.location, k.repeat, k.isComposing, k.ctrlKey, k.shiftKey, k.altKey, k.metaKey, \
              k.keyCode, k.charCode, k.which, k.detail, k.view].join()",
            "keydown,a,KeyA,2,true,true,true,true,false,false,65,97,65,1,",
        ),
        (
            "[k.getModifierState('NumLock'), k.getModifierState('Shift'), k.getModifierState('Alt')].join()",
            "true,true,false",
        ),
        (
            "[k instanceof KeyboardEvent, k instanceof UIEvent, k instanceof MouseEvent, KeyboardEvent.DOM_KEY_LOCATION_NUMPAD, \
              Object.getPrototypeOf(KeyboardEvent.prototype) === UIEvent.prototype].join()",
            "true,true,false,3,true",
        ),
        // `DOMString` members converted (`ToString`); the defaults.
        (
            "var d = new KeyboardEvent('k', { key: 5, code: null }); [d.key, d.code].join()",
            "5,null",
        ),
        (
            "var e = new KeyboardEvent('k'); [JSON.stringify(e.key), e.location, e.keyCode, e.repeat, e.bubbles].join()",
            "\"\",0,0,false,false",
        ),
        // A brand check: a mouse event isn't a keyboard event.
        (
            "var get = Object.getOwnPropertyDescriptor(KeyboardEvent.prototype, 'key').get; \
             (() => { try { get.call(new MouseEvent('m')); return 'no' } catch (err) { return err instanceof TypeError } })()",
            "true",
        ),
    ];
    for (script, expected) in cases {
        assert_eq!(env.eval(script), expected, "{script}");
    }
}

#[test]
fn focus_events_are_made_and_read() {
    let mut env = Env::window("about:blank");
    let cases = [
        (
            "var f = new FocusEvent('focus', { detail: 2, bubbles: true }); \
             [f.type, f.detail, f.bubbles, f.relatedTarget, f.view, f instanceof FocusEvent, f instanceof UIEvent, \
              Object.getPrototypeOf(FocusEvent.prototype) === UIEvent.prototype].join()",
            "focus,2,true,,,true,true,true",
        ),
        // No frame: nothing takes the focus.
        (
            "var d = document.createElement('div'); d.tabIndex = 3; d.focus(); [d.tabIndex, document.hasFocus()].join()",
            "3,false",
        ),
    ];
    for (script, expected) in cases {
        assert_eq!(env.eval(script), expected, "{script}");
    }
}
