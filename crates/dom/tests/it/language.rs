//! A document's content language: its response's `Content-Language` up to
//! the first comma, or its `<meta http-equiv="content-language">`'s as one
//! is connected or changed (none with a comma, nor a disconnected one's);
//! the document styled again as it changes.

use dom::{Cx, Dom, NodeRef, QualName, StyleFlags};

/// A document with a window and a `<head>`: the `<head>`.
fn head<'gc>(cx: &Cx<'_, 'gc>) -> NodeRef<'gc> {
    let document = cx.create_document(true);
    cx.create_window(document);
    let html = cx.create_element(document, QualName::html("html"));
    cx.append_child(document, html).unwrap();
    let head = cx.create_element(document, QualName::html("head"));
    cx.append_child(html, head).unwrap();
    head
}

fn meta<'gc>(cx: &Cx<'_, 'gc>, document: NodeRef<'gc>, content: &str) -> NodeRef<'gc> {
    let meta = cx.create_element(document, QualName::html("meta"));
    cx.set_attribute(meta, "http-equiv", "Content-Language");
    cx.set_attribute(meta, "content", content);
    meta
}

#[test]
fn a_responses_content_language_is_its_first() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.document_of(head(cx));
        assert_eq!(cx.content_language(document), None);
        cx.set_content_language(document, Some(" de , en"));
        assert_eq!(cx.content_language(document).as_deref(), Some("de"));
        cx.set_content_language(document, Some(" "));
        assert_eq!(cx.content_language(document), None, "an empty one is none");
    });
}

#[test]
fn a_meta_sets_it_as_it_is_connected_or_changed() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let head = head(cx);
        let document = cx.document_of(head);
        cx.set_content_language(document, Some("de"));
        let away = meta(cx, document, "fr");
        assert_eq!(
            cx.content_language(document).as_deref(),
            Some("de"),
            "not connected"
        );
        cx.append_child(head, meta(cx, document, "es, en")).unwrap();
        assert_eq!(
            cx.content_language(document).as_deref(),
            Some("de"),
            "a comma"
        );
        cx.append_child(head, away).unwrap();
        assert_eq!(cx.content_language(document).as_deref(), Some("fr"));
        cx.set_attribute(away, "content", "  pt  br");
        assert_eq!(cx.content_language(document).as_deref(), Some("pt"));
    });
}

#[test]
fn a_change_styles_the_document_again() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let head = head(cx);
        let document = cx.document_of(head);
        let root = cx.parent(head).unwrap();
        cx.set_content_language(document, Some("de"));
        cx.clear_style_flags(root);
        cx.set_content_language(document, Some("de"));
        assert_eq!(cx.style_flags(root), StyleFlags::CLEAN, "the same");
        cx.set_content_language(document, Some("fr"));
        assert!(
            cx.style_flags(root)
                .contains(StyleFlags::NEEDS_STYLE_RECALC)
        );
    });
}
