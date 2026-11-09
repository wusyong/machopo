//! `innerHTML`, `outerHTML` and `insertAdjacentHTML` from script: what
//! they parse is the document's (wrappers, ids, selectors), `<script>`s
//! they make don't run, and `<noscript>` parses as raw text where
//! scripting is enabled (a window's document).

use crate::env::Env;

#[test]
fn markup_from_script() {
    let mut env = Env::window("about:blank");
    env.eval(
        "var html = document.createElement('html'); document.appendChild(html); \
         var body = document.createElement('body'); html.appendChild(body); \
         var div = document.createElement('div'); body.appendChild(div);",
    );
    let cases = [
        (
            "div.innerHTML = '<p id=p class=a>one <b>two</b></p>'; div.innerHTML",
            "<p id=\"p\" class=\"a\">one <b>two</b></p>",
        ),
        (
            "var p = document.getElementById('p'); p instanceof HTMLElement && p.parentNode === div",
            "true",
        ),
        ("div.querySelector('.a b').textContent", "two"),
        (
            "div.firstChild === p && div.innerHTML === div.innerHTML",
            "true",
        ),
        // Scripts made by markup never run.
        (
            "var ran = false; div.innerHTML = '<script>ran = true<\\/script>'; ran",
            "false",
        ),
        // A window's document has scripting enabled: `<noscript>` is raw text.
        (
            "div.innerHTML = '<noscript><p>x</p></noscript>'; div.firstChild.firstChild.nodeType",
            "3",
        ),
        ("div.outerHTML", "<div><noscript><p>x</p></noscript></div>"),
        (
            "div.innerHTML = '<i>i</i>'; div.firstChild.outerHTML = '<u>u</u>!'; div.innerHTML",
            "<u>u</u>!",
        ),
        (
            "div.insertAdjacentHTML('beforebegin', '<a>a</a>'); div.insertAdjacentHTML('afterend', 'z'); body.innerHTML",
            "<a>a</a><div><u>u</u>!</div>z",
        ),
        (
            "try { div.insertAdjacentHTML('nowhere', ''); } catch (e) { e.name }",
            "SyntaxError",
        ),
        (
            "try { html.outerHTML = ''; } catch (e) { e.name }",
            "NoModificationAllowedError",
        ),
        ("div.innerHTML = null; div.innerHTML", ""),
    ];
    for (source, expected) in cases {
        assert_eq!(env.eval(source), expected, "{source}");
    }
    // A document with no window has scripting disabled: `<noscript>`
    // holds markup.
    let mut env = Env::new();
    assert_eq!(
        env.eval(
            "var d = machopoDom.createDocument(); var e = d.createElement('div'); \
             e.innerHTML = '<noscript><p>x</p></noscript>'; e.firstChild.firstChild.nodeName"
        ),
        "P"
    );
}
