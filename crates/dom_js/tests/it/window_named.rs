//! HTML's named access on the `Window` object: the realm's named
//! properties object under `Window.prototype`, finding child windows by
//! their iframes' names, elements by id and the named elements by `name`
//! (one, or a live collection of several), not shadowing what the window
//! or its prototypes have.

use crate::history::Page;

#[test]
fn elements_and_child_windows_by_name() {
    let mut page = Page::new("https://example.test/");
    page.load();
    // Where it is, and what it is.
    assert_eq!(
        page.eval(
            "var p = Object.getPrototypeOf(Window.prototype); \
             [Object.prototype.toString.call(p), Object.getPrototypeOf(p) === EventTarget.prototype, typeof nothing].join()"
        ),
        "[object WindowProperties],true,undefined"
    );
    // By id (any element), by name (only a named element's).
    assert_eq!(
        page.eval(
            "var d = document.body.appendChild(document.createElement('div')); d.id = 'a'; \
             var n = document.body.appendChild(document.createElement('div')); n.setAttribute('name', 'b'); \
             var img = document.body.appendChild(document.createElement('img')); img.name = 'c'; \
             [window.a === d, typeof window.b, window.c === img, 'a' in window].join()"
        ),
        "true,undefined,true,true"
    );
    // Several: a live collection, the same one; down to one: the element.
    assert_eq!(
        page.eval(
            "var form = document.body.appendChild(document.createElement('form')); form.name = 'a'; \
             document.body.appendChild(document.createElement('p')).setAttribute('name', 'a'); \
             var all = window.a; var r = [Object.prototype.toString.call(all), all.length, all === window.a]; \
             var e = document.body.appendChild(document.createElement('embed')); e.id = 'a'; r.push(all.length); \
             e.remove(); form.remove(); r.push(window.a === d, all.length); r.join()"
        ),
        "[object HTMLCollection],2,true,3,true,1"
    );
    // The same element by both its id and its name: one.
    assert_eq!(
        page.eval("img.id = 'c'; [window.c === img, img.name = 'z', window.z === img, window.c === img].join()"),
        "true,z,true,true"
    );
    // The window's own and its prototypes' come first; a global defined
    // shadows it; it isn't deleted or defined on.
    assert_eq!(
        page.eval(
            "var s = document.body.appendChild(document.createElement('span')); s.id = 'addEventListener'; \
             var t = document.body.appendChild(document.createElement('span')); t.id = 'document'; \
             var u = document.body.appendChild(document.createElement('span')); u.id = 'later'; \
             var r = [typeof window.addEventListener, window.document === document, window.later === u]; \
             var p = Object.getPrototypeOf(Window.prototype); \
             r.push(delete p.later, p.later === u, Reflect.defineProperty(p, 'x', { value: 1 }), p.x); \
             window.later = 5; r.push(window.later); r.join()"
        ),
        "function,true,true,false,true,false,,5"
    );
    // A child window by its iframe's name, before an element of the name.
    assert_eq!(
        page.eval(
            "var x = document.body.appendChild(document.createElement('img')); x.name = 'kid'; \
             var f = document.body.appendChild(document.createElement('iframe')); f.name = 'kid'; \
             var r = [window.kid === f.contentWindow]; f.name = 'other'; \
             r.push(window.kid === x, window.other === f.contentWindow); f.remove(); r.push(typeof window.other); r.join()"
        ),
        "true,true,true,undefined"
    );
}

/// Not in shadow trees; nor of a document not the window's.
#[test]
fn only_the_documents_tree() {
    let mut page = Page::new("https://example.test/");
    page.load();
    assert_eq!(
        page.eval(
            "var host = document.body.appendChild(document.createElement('div')); var root = host.attachShadow({ mode: 'open' }); \
             var i = root.appendChild(document.createElement('img')); i.id = 'inside'; i.name = 'inside2'; \
             var other = document.implementation.createHTMLDocument(''); \
             var o = other.body.appendChild(other.createElement('img')); o.id = 'elsewhere'; \
             var r = [typeof window.inside, typeof window.inside2, typeof window.elsewhere]; \
             document.body.appendChild(i); r.push(window.inside === i, window.inside2 === i); r.join()"
        ),
        "undefined,undefined,undefined,true,true"
    );
}
