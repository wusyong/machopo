//! Links and the base URL: a document's base URL (its first `<base
//! href>`'s, kept until a `<base>` or the URL changes) and what resolves
//! against it, `<base>`'s members, `HTMLHyperlinkElementUtils` of `<a>`
//! and `<area>`, following a hyperlink (the frame its `target` chooses;
//! script's clicks open no tab), and `document.title`.

use dom::FrameNavigation;
use dom_api::{FrameRequest, ScriptEngine};

use crate::history::{Page, to};

#[test]
fn the_base_url_and_what_resolves_against_it() {
    let mut page = Page::new("https://example.test/a/b.html");
    assert_eq!(
        page.eval(
            "var img = document.body.appendChild(document.createElement('img')); img.setAttribute('src', 'x.png'); \
             [document.baseURI, img.src].join()"
        ),
        "https://example.test/a/b.html,https://example.test/a/x.png"
    );
    assert_eq!(
        page.eval(
            "var base = document.createElement('base'); base.href = 'https://other.test/dir/'; \
             document.body.prepend(base); [document.baseURI, img.src, document.body.baseURI].join()"
        ),
        "https://other.test/dir/,https://other.test/dir/x.png,https://other.test/dir/"
    );
    // The first `<base>` with an `href` in tree order; its `href` changed;
    // `data:` and `javascript:` ignored.
    assert_eq!(
        page.eval(
            "var other = document.createElement('base'); other.href = '/later/'; document.body.append(other); \
             base.removeAttribute('href'); var first = document.baseURI; base.href = 'data:text/plain,x'; \
             [first, document.baseURI].join()"
        ),
        "https://example.test/later/,https://example.test/a/b.html"
    );
    // `<base>`'s `href` against the fallback base URL, `target` as it is;
    // removed, the URL again.
    assert_eq!(
        page.eval(
            "base.href = 'c/'; base.target = 'kid'; var href = base.href; other.remove(); base.remove(); \
             [href, base.target, document.baseURI, img.src].join()"
        ),
        "https://example.test/a/c/,kid,https://example.test/a/b.html,https://example.test/a/x.png"
    );
    // A script's `src` and an iframe's resolve against it too.
    assert_eq!(
        page.eval("document.body.prepend(base); var s = document.createElement('script'); s.setAttribute('src', 'y.js'); s.src"),
        "https://example.test/a/c/y.js"
    );
}

#[test]
fn hyperlink_element_utils() {
    let mut page = Page::new("https://example.test/a/b.html");
    assert_eq!(
        page.eval(
            "var a = document.createElement('a'); a.href = 'https://u:p@host.test:81/p/q?s#h'; \
             [a.href, String(a), a.origin, a.protocol, a.username, a.password, a.host, a.hostname, a.port, \
              a.pathname, a.search, a.hash].join(' ')"
        ),
        "https://u:p@host.test:81/p/q?s#h https://u:p@host.test:81/p/q?s#h https://host.test:81 https: u p \
         host.test:81 host.test 81 /p/q ?s #h"
    );
    assert_eq!(
        page.eval(
            "a.protocol = 'http'; a.username = 'v'; a.port = ''; a.pathname = 'r'; a.search = 't'; a.hash = 'i'; \
             a.getAttribute('href')"
        ),
        "http://v:p@host.test/r?t#i"
    );
    // Without an `href`, and one that doesn't parse; an opaque path.
    assert_eq!(
        page.eval(
            "var b = document.createElement('a'); var none = [b.href, b.protocol, b.host].join('|'); \
             b.setAttribute('href', 'http://[x'); b.host = 'y'; \
             var m = document.createElement('a'); m.href = 'MAILTO:z@example.test'; m.host = 'h'; \
             [none, b.href, b.getAttribute('href'), m.href, m.getAttribute('href')].join(' ')"
        ),
        "|:| http://[x http://[x mailto:z@example.test MAILTO:z@example.test",
        "an opaque path's host: nothing written back"
    );
    assert_eq!(
        page.eval(
            "a.text = 'go'; [a.text, a.textContent, a.relList.supports('noopener'), a.relList.supports('stylesheet'), \
             a instanceof HTMLAnchorElement, document.createElement('area') instanceof HTMLAreaElement, \
             document.createElement('base') instanceof HTMLBaseElement].join()"
        ),
        "go,go,true,false,true,true,true"
    );
    assert_eq!(
        page.eval("var area = document.createElement('area'); area.href = '/z'; area.pathname"),
        "/z"
    );
}

#[test]
fn following_hyperlinks() {
    let mut page = Page::new("https://example.test/a/b.html");
    page.load();
    page.eval(
        "var f = document.body.appendChild(document.createElement('iframe')); f.name = 'kid'; \
         var a = document.body.appendChild(document.createElement('a')); a.href = 'c.html'; 0",
    );
    let child = page
        .engine
        .take_frame_requests()
        .into_iter()
        .find_map(|request| match request {
            FrameRequest::Created { frame, .. } => Some(frame),
            _ => None,
        })
        .expect("the iframe's frame");
    let top = page.engine.top_frame();
    let followed = |page: &mut Page, source: &str| {
        page.eval(source);
        page.engine
            .take_frame_requests()
            .into_iter()
            .filter_map(|request| match request {
                FrameRequest::Navigation { frame, navigation } => Some((frame, navigation)),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    // Script's click: the document's frame, without the user replacing
    // the entry until it is completely loaded (it is).
    assert_eq!(
        followed(&mut page, "a.click()"),
        [(top, to("https://example.test/a/c.html", false))]
    );
    assert_eq!(
        followed(&mut page, "a.target = '_self'; a.click()"),
        [(top, to("https://example.test/a/c.html", false))]
    );
    assert_eq!(
        followed(&mut page, "a.target = '_top'; a.click()"),
        [(top, to("https://example.test/a/c.html", false))]
    );
    // A name: the frame of that name (its initial `about:blank`, not
    // completely loaded: replaced).
    assert_eq!(
        followed(&mut page, "a.target = 'kid'; a.click()"),
        [(child, to("https://example.test/a/c.html", true))]
    );
    // From the child frame's document: `_top` and `_parent` are this one.
    assert_eq!(
        followed(
            &mut page,
            "var c = f.contentDocument.body.appendChild(f.contentDocument.createElement('a')); c.href = 'https://example.test/g'; \
             c.target = '_top'; c.click(); c.target = '_parent'; c.click(); c.target = ''; c.click()"
        ),
        [
            (top, to("https://example.test/g", false)),
            (top, to("https://example.test/g", false)),
            (child, to("https://example.test/g", true)),
        ]
    );
    // A new tab, by script: none.
    assert_eq!(followed(&mut page, "a.target = '_blank'; a.click()"), []);
    assert_eq!(followed(&mut page, "a.target = 'nobody'; a.click()"), []);
    // The `<base>`'s target and URL.
    assert_eq!(
        followed(
            &mut page,
            "a.removeAttribute('target'); var base = document.createElement('base'); base.target = 'kid'; \
             base.href = '/d/'; document.body.prepend(base); a.click()"
        ),
        [(child, to("https://example.test/d/c.html", true))]
    );
    // A fragment of the document: within it.
    page.eval("base.remove(); a.href = '#f'; a.click(); 0");
    assert!(matches!(
        &page.navigations()[..],
        [FrameNavigation::SameDocument { url, .. }] if url == "https://example.test/a/b.html#f"
    ));
    // Disconnected, or a click canceled: nothing.
    assert_eq!(
        followed(
            &mut page,
            "a.remove(); a.click(); document.body.append(a); a.onclick = (e) => e.preventDefault(); a.click()"
        ),
        []
    );
    // A click on an element in it follows it.
    assert_eq!(
        followed(
            &mut page,
            "a.onclick = null; a.href = 'e.html'; a.appendChild(document.createElement('span')).click()"
        ),
        [(top, to("https://example.test/a/e.html", false))]
    );
}

#[test]
fn document_title() {
    let mut page = Page::new("https://example.test/");
    assert_eq!(
        page.eval("document.documentElement.prepend(document.createElement('head')); var t0 = document.title; document.title = '  a   b '; [t0, document.title, document.querySelector('head > title').textContent].join('|')"),
        "|a b|  a   b "
    );
    assert_eq!(page.eval("document.querySelector('title').append(document.createElement('b')); document.title = 'c'; document.querySelector('title').childNodes.length"), "1");
    // An SVG document element: its first SVG `title` child, made first.
    assert_eq!(
        page.eval(
            "var d = document.implementation.createDocument('http://www.w3.org/2000/svg', 'svg', null); \
             d.documentElement.appendChild(d.createElementNS('http://www.w3.org/2000/svg', 'g')); \
             d.title = 'x'; [d.title, d.documentElement.firstChild.localName].join()"
        ),
        "x,title"
    );
    // No head: nothing.
    assert_eq!(page.eval("var h = document.implementation.createHTMLDocument(''); h.head.remove(); h.title = 'y'; h.title"), "");
}
