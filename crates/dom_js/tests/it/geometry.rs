//! `DOMRectReadOnly` and `DOMRect` from script: the constructors,
//! `fromRect`, WebIDL's `unrestricted double` conversions, which of the two
//! a rect is, and `toJSON()`.

use crate::env::Env;

/// The constructors and `fromRect`, `DOMRect` a `DOMRectReadOnly` whose
/// own `x`, `y`, `width` and `height` are settable; the values are
/// `unrestricted double`s (NaN and the infinities kept, anything else
/// converted as by `Number()`).
#[test]
fn rects_from_script() {
    let mut env = Env::window("about:blank");
    assert_eq!(
        env.eval(
            "var log = [];
             const r = new DOMRect(1, '2', -3);
             log.push(r instanceof DOMRectReadOnly, r.x, r.y, r.width, r.height, r.left, r.right);
             r.x = '10'; r.height = Infinity; r.y = NaN;
             log.push(r.x, r.height, r.bottom, r.top);
             const ro = new DOMRectReadOnly(5);
             ro.x = 7;
             log.push(ro.x, ro instanceof DOMRect, typeof Object.getOwnPropertyDescriptor(DOMRectReadOnly.prototype, 'x').set,
                      typeof Object.getOwnPropertyDescriptor(DOMRect.prototype, 'x').set);
             const f = DOMRect.fromRect({ y: 3, width: 4 });
             const g = DOMRectReadOnly.fromRect();
             log.push(f.constructor.name, f.x, f.y, f.width, g.constructor.name, g.height,
                      DOMRect.fromRect({ x: NaN }).x);
             try { DOMRect(); } catch (e) { log.push(e.constructor.name); }
             try { Object.getOwnPropertyDescriptor(DOMRect.prototype, 'x').set.call(ro, 1); }
             catch (e) { log.push(e.constructor.name); }
             log.join(' / ')"
        ),
        "true / 1 / 2 / -3 / 0 / -2 / 1 / 10 / Infinity / NaN / NaN / 5 / false / undefined / function / \
         DOMRect / 0 / 3 / 4 / DOMRectReadOnly / 0 / NaN / TypeError / TypeError"
    );
}

/// `toJSON()`: a plain object with each attribute's value in the
/// interface's order, as Chrome 152 gives (a negative width makes `left`
/// and `right` differ from `x` and `x + width`); `DOMRect` has it from
/// `DOMRectReadOnly`.
#[test]
fn rects_to_json() {
    let mut env = Env::window("about:blank");
    assert_eq!(
        env.eval(
            "var log = [];
             const j = new DOMRect(1, 2, -3, 4).toJSON();
             log.push(JSON.stringify(j), Object.getPrototypeOf(j) === Object.prototype,
                      JSON.stringify(new DOMRectReadOnly(1, 2, 3, 4)),
                      DOMRect.prototype.hasOwnProperty('toJSON'));
             try { DOMRectReadOnly.prototype.toJSON.call({}); } catch (e) { log.push(e.constructor.name); }
             log.join(' / ')"
        ),
        r#"{"x":1,"y":2,"width":-3,"height":4,"top":2,"right":1,"bottom":6,"left":-2} / true / "#
            .to_owned()
            + r#"{"x":1,"y":2,"width":3,"height":4,"top":2,"right":4,"bottom":6,"left":1} / false / "#
            + "TypeError"
    );
}
