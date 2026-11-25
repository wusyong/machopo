//! The `<link>`s' sheets as their host fetches them: each asked for once
//! in its document; the requests sent kept by number till their responses
//! come, each blocking its document's rendering, its parser's scripts and
//! its `load` as it was sent; a `<link>` taken away blocks scripts no
//! more, and is kept till its response comes.

use dom::{Cx, Dom, NodeRef, QualName, ScriptFlags, SentSheet, ShadowRootInit, ShadowRootMode};
use dom::{NodeRoot, SlotAssignmentMode};

/// A document at `https://example.test/` with a window and a `<body>`:
/// the `<body>`.
fn page<'gc>(cx: &Cx<'_, 'gc>) -> NodeRef<'gc> {
    let document = cx.create_document(true);
    cx.create_window(document);
    cx.set_document_url(document, "https://example.test/");
    let html = cx.create_element(document, QualName::html("html"));
    cx.append_child(document, html).unwrap();
    let body = cx.create_element(document, QualName::html("body"));
    cx.append_child(html, body).unwrap();
    body
}

/// A `<link rel=stylesheet href=a.css>` in `parent`, the parser's if
/// `parser_inserted`.
fn link<'gc>(cx: &Cx<'_, 'gc>, parent: NodeRef<'gc>, parser_inserted: bool) -> NodeRef<'gc> {
    let link = cx.create_element(cx.document_of(parent), QualName::html("link"));
    cx.set_attribute(link, "rel", "stylesheet");
    cx.set_attribute(link, "href", "a.css");
    cx.set_script_flags(
        link,
        ScriptFlags {
            parser_inserted,
            ..ScriptFlags::default()
        },
    );
    cx.append_child(parent, link).unwrap();
    link
}

/// `link`'s sheet of `document` sent as `request_id`, blocking what
/// `blocks_render` and `blocks_scripts` say.
fn send<'gc>(
    cx: &Cx<'_, 'gc>,
    link: NodeRef<'gc>,
    request_id: u64,
    blocks_render: bool,
    blocks_scripts: bool,
) {
    cx.sheet_sent(
        link,
        request_id,
        SentSheet {
            link: cx.node_id(link),
            document: cx.node_id(cx.document_of(link)),
            url: "https://example.test/a.css".to_owned(),
            blocks_render,
            blocks_scripts,
        },
    );
}

#[test]
fn a_sent_sheet_blocks_till_its_response_comes() {
    let mut dom = Dom::new();
    let (document, id) = dom.mutate(|cx| {
        let body = page(cx);
        let document = cx.document_of(body);
        let blocking = link(cx, body, true);
        let alternate = link(cx, body, false);
        send(cx, blocking, 7, true, true);
        send(cx, alternate, 8, false, false);
        (cx.root(document), cx.node_id(document))
    });
    assert!(dom.sheets_block_render(id));
    assert!(dom.sheets_pending_for_scripts(id));
    assert_eq!(dom.sent_sheets_of(id), 2);
    assert_eq!(
        dom.sent_sheet(7).map(|sent| sent.url).as_deref(),
        Some("https://example.test/a.css")
    );
    dom.mutate(|cx| {
        let document = document.get(cx.gc);
        assert!(cx.sheets_block_scripts(document));
        assert!(cx.has_load_requests(document));
        let (sent, link) = cx.take_sent_sheet(7).expect("sent");
        assert!(sent.blocks_scripts && sent.blocks_render);
        assert!(link.is_some_and(|link| cx.node_id(link) == sent.link));
        assert!(cx.take_sent_sheet(7).is_none(), "taken once");
        assert!(!cx.sheets_block_scripts(document));
        // The alternate one still delays `load`, blocking nothing.
        assert!(cx.has_load_requests(document));
        cx.take_sent_sheet(8).expect("sent");
        assert!(!cx.has_load_requests(document));
    });
    assert!(!dom.sheets_block_render(id));
    assert!(!dom.sheets_pending_for_scripts(id));
    assert_eq!(dom.sent_sheets_of(id), 0);
}

#[test]
fn a_link_taken_away_blocks_scripts_no_more() {
    let mut dom = Dom::new();
    let (id, kept) = dom.mutate(|cx| {
        let body = page(cx);
        let document = cx.document_of(body);
        let link = link(cx, body, true);
        send(cx, link, 1, true, true);
        assert!(cx.sheets_block_scripts(document));
        cx.remove(link);
        assert!(!cx.sheets_block_scripts(document), "taken out");
        (cx.node_id(document), (cx.root(body), cx.root(link)))
    });
    // Still loading, which the parser's scripts look again at.
    assert!(dom.sheets_pending_for_scripts(id));
    dom.mutate(|cx| {
        let (body, link) = (kept.0.get(cx.gc), kept.1.get(cx.gc));
        let document = cx.document_of(body);
        cx.append_child(body, link).unwrap();
        assert!(cx.sheets_block_scripts(document), "back");

        cx.set_attribute(link, "rel", "icon");
        assert!(!cx.sheets_block_scripts(document), "no stylesheet");
        cx.set_attribute(link, "rel", "stylesheet");

        let host = cx.create_element(document, QualName::html("div"));
        cx.append_child(body, host).unwrap();
        let shadow = cx.attach_shadow(
            host,
            ShadowRootInit {
                mode: ShadowRootMode::Open,
                delegates_focus: false,
                slot_assignment: SlotAssignmentMode::Named,
                clonable: false,
                serializable: false,
            },
        );
        cx.append_child(shadow, link).unwrap();
        assert!(!cx.sheets_block_scripts(document), "in a shadow tree");

        let other = cx.create_document(true);
        cx.adopt_node(link, other);
        cx.append_child(other, link).unwrap();
        assert!(!cx.sheets_block_scripts(document), "another document's");
    });
}

#[test]
fn a_link_is_asked_for_once_in_its_document() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let body = page(cx);
        let link = link(cx, body, false);
        assert!(!cx.sheet_requested(link));
        cx.set_sheet_requested(link);
        assert!(cx.sheet_requested(link));
        cx.remove(link);
        cx.append_child(body, link).unwrap();
        assert!(cx.sheet_requested(link), "the same document");
        let other = cx.create_document(true);
        cx.adopt_node(link, other);
        assert!(!cx.sheet_requested(link), "asked for anew in another");

        // A script's bit is its own: not a `<link>`'s.
        let script = cx.create_element(cx.document_of(body), QualName::html("script"));
        cx.set_sheet_requested(script);
        assert!(!cx.sheet_requested(script));
        assert!(!cx.script_flags(script).already_started);
    });
}

#[test]
fn a_sent_sheets_link_is_kept_till_its_response_comes() {
    let mut dom = Dom::new();
    let document: NodeRoot = dom.mutate(|cx| {
        let body = page(cx);
        let link = link(cx, body, true);
        send(cx, link, 3, true, true);
        cx.remove(link);
        cx.root(cx.document_of(body))
    });
    dom.collect();
    dom.mutate(|cx| {
        let (sent, link) = cx.take_sent_sheet(3).expect("sent");
        let link = link.expect("kept");
        assert_eq!(cx.node_id(link), sent.link);
        assert!(!cx.has_load_requests(document.get(cx.gc)));
    });
}

#[test]
fn the_hrefs_that_asked_for_nothing_are_taken_once() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.document_of(page(cx));
        assert!(cx.take_failed_sheets(document).is_empty());
        cx.note_sheet_failed(document, "http://[".to_owned());
        cx.note_sheet_failed(document, "b.css".to_owned());
        assert_eq!(cx.take_failed_sheets(document), ["http://[", "b.css"]);
        assert!(cx.take_failed_sheets(document).is_empty());
    });
}
