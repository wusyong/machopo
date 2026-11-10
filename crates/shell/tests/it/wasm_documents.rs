//! Wasm documents in a whole browser: a document that declares its scripts
//! wasm (`<meta name="script" content="wasm">`) runs the components its
//! `<script type="application/wasm" src>`s name, served as
//! `application/wasm`, and what they add to the page is drawn; one that
//! isn't served so, or isn't there, is a failed subresource and doesn't
//! run; a document that doesn't declare it fetches and runs none. (What
//! the page does between such documents is the scenario `wasm-document`.)

use std::path::PathBuf;

use gfx::Size;
use shell::headless::{self, Frame};

/// A page with `html` in this test's scratch directory, the page guest
/// beside it as `page.wasm` and, the same bytes, as `page.bin` (not
/// `application/wasm`); its `file:` URL.
fn page(name: &str, html: &str) -> String {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shell-wasm-documents");
    std::fs::create_dir_all(&dir).unwrap();
    let guest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../dom_wasm/tests/guests/page.wasm");
    for copy in ["page.wasm", "page.bin"] {
        std::fs::copy(&guest, dir.join(copy)).unwrap();
    }
    let path = dir.join(name);
    std::fs::write(&path, html).unwrap();
    net::file_url(&path).expect("an absolute path")
}

fn render(url: &str) -> Frame {
    headless::render(url, Size::new(200, 100), 1.0, true).expect("the page renders")
}

/// The page with `head` in its head and one script, `script`: the guest's
/// `hello` case appends a paragraph to the root element.
fn document(head: &str, script: &str) -> String {
    format!(r#"<!DOCTYPE html><html data-case="hello"><head>{head}</head><body>{script}"#)
}

const META: &str = r#"<meta name="script" content="wasm">"#;

#[test]
fn a_wasm_documents_script_runs_and_what_it_adds_is_drawn() {
    let ran = render(&page(
        "runs.html",
        &document(META, r#"<script type="application/wasm" src="page.wasm"></script>"#),
    ));
    assert_eq!(ran.failed_subresources, [""; 0]);
    // The same page with no script draws nothing.
    let empty = render(&page("empty.html", &document(META, "")));
    assert!(ran.pixels != empty.pixels, "the guest's paragraph is drawn");
}

#[test]
fn a_wasm_script_not_served_as_wasm_fails_to_load() {
    let failed = render(&page(
        "not-wasm.html",
        &document(META, r#"<script type="application/wasm" src="page.bin"></script>"#),
    ));
    assert_eq!(failed.failed_subresources.len(), 1, "{:?}", failed.failed_subresources);
    assert!(failed.failed_subresources[0].contains("page.bin"));
    let empty = render(&page("empty.html", &document(META, "")));
    assert!(failed.pixels == empty.pixels, "it didn't run");
}

#[test]
fn a_wasm_script_that_isnt_there_fails_to_load() {
    let failed = render(&page(
        "missing.html",
        &document(META, r#"<script type="application/wasm" src="none.wasm"></script>"#),
    ));
    assert_eq!(failed.failed_subresources.len(), 1, "{:?}", failed.failed_subresources);
    assert!(failed.failed_subresources[0].contains("none.wasm"));
}

/// Without the declaration the document is a JS one: its wasm script is
/// of a type it doesn't run, not fetched.
#[test]
fn a_document_that_doesnt_declare_it_runs_no_wasm() {
    let script = r#"<script type="application/wasm" src="none.wasm"></script>"#;
    let js = render(&page("js.html", &document("", script)));
    assert_eq!(js.failed_subresources, [""; 0], "not fetched");
    // Nor with a `<meta>` of another name, or another content.
    for head in [
        r#"<meta name="scripts" content="wasm">"#,
        r#"<meta name="script" content="js">"#,
    ] {
        let other = render(&page("other.html", &document(head, script)));
        assert_eq!(other.failed_subresources, [""; 0], "{head}");
    }
    // Nor a document that isn't `text/html`, whatever it declares.
    let xhtml = render(&page("declares.xhtml", &document(META, script)));
    assert_eq!(xhtml.failed_subresources, [""; 0], "XHTML");
}

/// JS in a wasm document isn't run, nor fetched: only wasm scripts are.
#[test]
fn a_wasm_document_runs_no_js() {
    let page = render(&page(
        "no-js.html",
        &document(
            META,
            r#"<script src="none.js"></script><script>document.body.append("JS")</script>"#,
        ),
    ));
    assert_eq!(page.failed_subresources, [""; 0], "not fetched");
    let empty = render(&self::page("empty.html", &document(META, "")));
    assert!(page.pixels == empty.pixels, "it didn't run");
}
