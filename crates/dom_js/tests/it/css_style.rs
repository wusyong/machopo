//! `element.style` from script: CSSOM's members, the camel-cased and
//! dashed names, the `style` attribute serialized as it is read.

use crate::env::Env;

#[test]
fn style_from_script() {
    let mut env = Env::window("about:blank");
    env.eval("var e = document.createElement('div');");
    let cases = [
        (
            "var s = e.style; s instanceof CSSStyleDeclaration && e.style === s",
            "true",
        ),
        // `CSSStyleProperties`' (CSSOM's): `element.style`, `getComputedStyle()`
        // and a style rule's `style`, `cssFloat` its own.
        (
            "var sheet = new CSSStyleSheet(); sheet.insertRule('a {}'); \
             [s instanceof CSSStyleProperties, getComputedStyle(e) instanceof CSSStyleProperties, \
              sheet.cssRules[0].style instanceof CSSStyleProperties, \
              Object.getPrototypeOf(CSSStyleProperties.prototype) === CSSStyleDeclaration.prototype, \
              CSSStyleProperties.prototype.hasOwnProperty('cssFloat'), \
              CSSStyleDeclaration.prototype.hasOwnProperty('cssFloat')].join()",
            "true,true,true,true,true,false",
        ),
        (
            "[s.length, s.cssText, e.hasAttribute('style')].join()",
            "0,,false",
        ),
        (
            "s.color = 'red'; [s.color, s.length, s[0], s[1]].join()",
            "red,1,color,",
        ),
        ("[0 in s, 1 in s].join()", "true,false"),
        ("e.getAttribute('style')", "color: red;"),
        ("s.backgroundColor = 'blue'; s['background-color']", "blue"),
        ("'color' in s && !('nothing' in s)", "true"),
        // A name that is no property is an ordinary property.
        (
            "s.nothing = 1; [s.nothing, Object.keys(s).includes('nothing')].join()",
            "1,true",
        ),
        (
            "s.setProperty('margin-top', '2px', 'important'); s.getPropertyPriority('margin-top')",
            "important",
        ),
        (
            "s.removeProperty('color') + ',' + e.getAttribute('style')",
            "red,background-color: blue; margin-top: 2px !important;",
        ),
        (
            "e.setAttribute('style', 'width: 5px'); [s.width, s.backgroundColor, s.length].join()",
            "5px,,1",
        ),
        (
            "e.style = 'height: 1px'; [e.getAttribute('style'), s.height].join()",
            "height: 1px;,1px",
        ),
        ("s.cssFloat = 'left'; s.float", "left"),
        ("s.cssText = 'color: green'; s.cssText", "color: green;"),
        ("s.color = null; e.getAttribute('style')", ""),
        (
            "Object.getOwnPropertyDescriptor(CSSStyleDeclaration.prototype, 'getter')",
            "undefined",
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(env.eval(source), expected, "{source}");
    }
}

/// The supported property names are the declaration's own keys (camel-
/// cased); `getComputedStyle` is the window's,
/// a new `CSSStyleDeclaration` each call, read only, its errors
/// `DOMException`s (no layout here: no style, empty values).
#[test]
fn names_and_get_computed_style() {
    let mut env = Env::window("about:blank");
    let cases = [
        (
            "var e = document.createElement('div'); var keys = Reflect.ownKeys(e.style); \
             [keys.includes('backgroundColor'), keys.includes('borderBlockEndColor'), 'color' in e.style].join()",
            "true,true,true",
        ),
        (
            "var c = getComputedStyle(e); [c instanceof CSSStyleDeclaration, c === getComputedStyle(e), \
             c.color, c.length, c.parentRule].join()",
            "true,false,,0,",
        ),
        (
            "try { c.color = 'red'; 'no' } catch (x) { x.name }",
            "NoModificationAllowedError",
        ),
        (
            "try { c.setProperty('color', 'red'); 'no' } catch (x) { x.name }",
            "NoModificationAllowedError",
        ),
        (
            "try { getComputedStyle(e, '::nope'); 'no' } catch (x) { x.name }",
            "SyntaxError",
        ),
        (
            "try { getComputedStyle(document); 'no' } catch (x) { x.name }",
            "TypeError",
        ),
    ];
    for (script, expected) in cases {
        assert_eq!(env.eval(script), expected, "{script}");
    }
}
