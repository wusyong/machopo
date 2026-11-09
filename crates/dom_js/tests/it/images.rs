//! `HTMLImageElement` from script: `Image()`, the reflected attributes,
//! `naturalWidth`, `complete`, `currentSrc`, `width` and `height` of an
//! image not rendered, `load` and `error` once the host hands the fetch
//! back, a new `src`'s pending request, and `decode()`'s promises.

use crate::env::Env;

/// A window at `https://example.test/dir/`, and `log`, what the events and
/// promises wrote.
fn window() -> Env {
    let mut env = Env::window("https://example.test/dir/page.html");
    env.eval(
        "document.appendChild(document.createElement('html')).appendChild(document.createElement('body')); \
         var log = []; \
         function watch(img) { img.onload = () => log.push('load ' + img.naturalWidth); \
           img.onerror = () => log.push('error'); return img; } 0",
    );
    env
}

fn check(env: &mut Env, cases: &[(&str, &str)]) {
    for (script, expected) in cases {
        assert_eq!(env.eval(script), *expected, "{script}");
    }
}

/// An image of `width` by 2, as the host hands it to the DOM.
fn image(width: u32) -> dom::Image {
    dom::Image::Raster(
        gfx::ImageData::new(vec![0u8; 4]),
        gfx::Size { width, height: 2 },
    )
}

/// The host's turn: the microtasks run, the URLs fetched taken.
fn fetches(env: &mut Env) -> Vec<String> {
    env.checkpoint();
    env.dom()
        .take_image_fetches()
        .into_iter()
        .map(|fetch| fetch.url)
        .collect()
}

/// The host hands `url` back (`None`: it failed), and runs the tasks.
fn fetched(env: &mut Env, url: &str, image: Option<dom::Image>) {
    env.dom().mutate(|cx| cx.image_fetched(url, image, true));
    env.run_element_tasks();
    env.checkpoint();
}

#[test]
fn the_image_constructor_and_the_reflected_attributes() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var i = new Image(3, 4); [i.localName, i instanceof HTMLImageElement, i.getAttribute('width'), i.getAttribute('height')].join()",
                "img,true,3,4",
            ),
            (
                "[new Image().hasAttribute('width'), new Image(1).hasAttribute('height')].join()",
                "false,false",
            ),
            ("Image.prototype === HTMLImageElement.prototype", "true"),
            ("i.ownerDocument === document", "true"),
            // `src` and `lowsrc` resolve against the document's URL.
            ("i.src = 'a.png'; i.src", "https://example.test/dir/a.png"),
            ("i.getAttribute('src')", "a.png"),
            ("i.src = ''; i.src", "https://example.test/dir/page.html"),
            ("new Image().src", ""),
            // `crossOrigin`: null without it, a CORS settings keyword.
            ("String(i.crossOrigin)", "null"),
            (
                "i.setAttribute('crossorigin', ''); i.crossOrigin",
                "anonymous",
            ),
            (
                "i.setAttribute('crossorigin', 'USE-credentials'); i.crossOrigin",
                "use-credentials",
            ),
            (
                "i.crossOrigin = null; i.hasAttribute('crossorigin')",
                "false",
            ),
            // The enumerated ones: their default, invalid and missing.
            (
                "[i.loading, i.decoding, i.fetchPriority, JSON.stringify(i.referrerPolicy)].join()",
                "eager,auto,auto,\"\"",
            ),
            (
                "i.loading = 'LAZY'; i.decoding = 'x'; i.fetchPriority = 'high'; i.referrerPolicy = 'origin'; \
                 [i.loading, i.decoding, i.fetchPriority, i.referrerPolicy].join()",
                "lazy,auto,high,origin",
            ),
            (
                "i.alt = 'a'; i.isMap = true; i.useMap = '#m'; [i.alt, i.isMap, i.useMap].join()",
                "a,true,#m",
            ),
            ("i.hspace = 5; i.getAttribute('hspace')", "5"),
        ],
    );
}

#[test]
fn an_image_loads_once_the_host_hands_it_back() {
    let mut env = window();
    check(
        &mut env,
        &[
            ("var i = watch(new Image()); i.complete", "true"),
            (
                "i.src = 'a.png'; [i.complete, i.naturalWidth, JSON.stringify(i.currentSrc)].join()",
                "false,0,\"\"",
            ),
        ],
    );
    assert_eq!(fetches(&mut env), ["https://example.test/dir/a.png"]);
    // Loading: no `currentSrc` yet, as in Chrome.
    check(
        &mut env,
        &[(
            "[i.complete, log.length, JSON.stringify(i.currentSrc)].join()",
            "false,0,\"\"",
        )],
    );
    fetched(&mut env, "https://example.test/dir/a.png", Some(image(7)));
    check(
        &mut env,
        &[
            ("log.join()", "load 7"),
            (
                "[i.complete, i.naturalWidth, i.naturalHeight, i.currentSrc].join()",
                "true,7,2,https://example.test/dir/a.png",
            ),
            // Not rendered: the attribute, else the natural size.
            ("[i.width, i.height].join()", "7,2"),
            ("i.setAttribute('width', '12'); i.width", "12"),
            // Another, of the same URL: available at once, `load` in a task.
            (
                "var j = watch(new Image()); j.src = 'a.png'; [j.complete, j.naturalWidth].join()",
                "true,7",
            ),
        ],
    );
    assert!(fetches(&mut env).is_empty());
    env.run_element_tasks();
    check(&mut env, &[("log.join()", "load 7,load 7")]);
}

#[test]
fn a_new_src_keeps_the_image_till_its_own_comes() {
    let mut env = window();
    env.eval("var i = watch(new Image()); i.src = 'a.png'");
    fetches(&mut env);
    fetched(&mut env, "https://example.test/dir/a.png", Some(image(7)));
    check(
        &mut env,
        &[(
            "i.src = 'b.png'; [i.complete, i.naturalWidth].join()",
            "false,7",
        )],
    );
    assert_eq!(fetches(&mut env), ["https://example.test/dir/b.png"]);
    check(
        &mut env,
        &[(
            "[i.complete, i.naturalWidth, i.currentSrc].join()",
            "false,7,https://example.test/dir/a.png",
        )],
    );
    fetched(&mut env, "https://example.test/dir/b.png", Some(image(9)));
    check(
        &mut env,
        &[
            ("log.join()", "load 7,load 9"),
            (
                "[i.complete, i.currentSrc].join()",
                "true,https://example.test/dir/b.png",
            ),
            // Taken away: gone at once, no event.
            (
                "i.removeAttribute('src'); [i.naturalWidth, i.complete, i.width].join()",
                "0,true,0",
            ),
        ],
    );
    env.checkpoint();
    env.run_element_tasks();
    check(&mut env, &[("log.join()", "load 7,load 9")]);
}

#[test]
fn a_failed_image_fires_error() {
    let mut env = window();
    env.eval("var i = watch(new Image()); i.src = 'gone.png'");
    fetches(&mut env);
    fetched(&mut env, "https://example.test/dir/gone.png", None);
    check(
        &mut env,
        &[
            ("log.join()", "error"),
            (
                "[i.complete, i.naturalWidth, i.currentSrc].join()",
                "true,0,https://example.test/dir/gone.png",
            ),
        ],
    );
    // An empty `src`: an error too, with no fetch.
    env.eval("log = []; var e = watch(new Image()); e.src = ''");
    assert!(fetches(&mut env).is_empty());
    env.run_element_tasks();
    check(&mut env, &[("log.join()", "error")]);
}

#[test]
fn decode_settles_with_the_request() {
    let mut env = window();
    env.eval(
        "var results = []; function settle(name, p) { p.then(() => results.push(name + ' ok'), \
           (e) => results.push(name + ' ' + e.name)); } \
         var i = new Image(); i.src = 'a.png'; settle('first', i.decode())",
    );
    fetches(&mut env);
    fetched(&mut env, "https://example.test/dir/a.png", Some(image(1)));
    check(&mut env, &[("results.join()", "first ok")]);
    // Of an available image: in a task.
    env.eval("settle('available', i.decode())");
    env.run_element_tasks();
    env.checkpoint();
    check(&mut env, &[("results.join()", "first ok,available ok")]);
    // The `src` changed before it loaded: rejected.
    env.eval("i.src = 'b.png'; settle('changed', i.decode()); i.src = 'c.png'");
    env.run_element_tasks();
    env.checkpoint();
    check(
        &mut env,
        &[(
            "results.join()",
            "first ok,available ok,changed EncodingError",
        )],
    );
    // A broken image's: rejected.
    env.eval("var g = new Image(); g.src = 'gone.png'; settle('broken', g.decode())");
    fetches(&mut env);
    fetched(&mut env, "https://example.test/dir/gone.png", None);
    check(
        &mut env,
        &[(
            "results.join()",
            "first ok,available ok,changed EncodingError,broken EncodingError",
        )],
    );
}

/// `<picture>` and `<source>` have their interfaces; `<source>`'s
/// attributes reflect (`src` resolved against the document's URL).
#[test]
fn picture_and_source() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var p = document.createElement('picture'); [p instanceof HTMLPictureElement, String(p)].join()",
                "true,[object HTMLPictureElement]",
            ),
            (
                "var s = document.createElement('source'); s instanceof HTMLSourceElement",
                "true",
            ),
            (
                "s.srcset = 'a.png 1x'; s.sizes = '10px'; s.media = '(min-width: 1px)'; s.type = 'image/png'; \
                 [s.getAttribute('srcset'), s.getAttribute('sizes'), s.getAttribute('media'), s.getAttribute('type')].join('|')",
                "a.png 1x|10px|(min-width: 1px)|image/png",
            ),
            ("s.src = 'b.png'; s.src", "https://example.test/dir/b.png"),
            (
                "s.width = 3; s.height = 4; [s.getAttribute('width'), s.height].join()",
                "3,4",
            ),
        ],
    );
}
