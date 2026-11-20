//! Each document's subresources go through the loader the browser gave it
//! as it committed, whose origin their initiator is: a cross-origin child
//! frame's, a child frame's first `about:blank` (its parent's), a
//! dedicated worker's (its creator's). One the browser wouldn't take ends
//! the renderer, and the page tells nothing.

use browser::Profile;
use net::test_server::Route;

use crate::dom_storage::{partition, titles_until};
use crate::navigation::{server, tab_in};

/// A page whose cross-origin child frame loads an image, whose first
/// `about:blank` child frame `fetch()`es, and whose worker `fetch()`es:
/// each answers, and the page's title tells.
#[test]
fn each_documents_loads_go_through_its_loader() {
    let other = server(&[
        (
            "/child",
            Route::ok(
                "text/html",
                r#"<img src="/pixel.png" onload="parent.postMessage('image', '*')" onerror="parent.postMessage('image failed', '*')">"#,
            ),
        ),
        ("/pixel.png", Route::ok("image/png", PIXEL.to_vec())),
    ]);
    let page = format!(
        r#"<!DOCTYPE html><iframe id=blank></iframe><script>
const told = [];
const tell = (what) => {{
  told.push(what);
  if (told.length == 3) document.title = JSON.stringify(told.sort());
}};
onmessage = (e) => tell(e.data);
blank.contentWindow.fetch('/data').then((r) => r.text()).then(tell, () => tell('blank failed'));
new Worker('/worker.js').onmessage = (e) => tell(e.data);
const child = document.createElement('iframe');
child.src = '{}';
document.body.append(child);
</script>"#,
        other.url("/child")
    );
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        ("/data", Route::ok("text/plain", "blank")),
        (
            "/worker.js",
            Route::ok(
                "text/javascript",
                "fetch('/data').then((r) => r.text()).then((t) => postMessage('worker ' + t), () => postMessage('worker failed'))",
            ),
        ),
    ]);
    let (mut web_contents, host_rx) = tab_in(partition(&Profile::in_memory()));
    web_contents.navigate(server.url("/"));
    assert_eq!(
        titles_until(&mut web_contents, &host_rx, |title| title.starts_with('[')),
        r#"["blank","image","worker blank"]"#
    );
    web_contents.shutdown();
}

/// A 1x1 PNG.
const PIXEL: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0xf0,
    0x1f, 0x00, 0x05, 0x00, 0x01, 0xff, 0x89, 0x99, 0x3d, 0x1d, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];
