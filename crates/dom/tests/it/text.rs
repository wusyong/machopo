//! Text: a text's data, and `textContent` of each kind.

use dom::{DomString, QualName};

use crate::children;

#[test]
fn a_texts_data_is_read_and_replaced() {
    let mut dom = dom::Dom::new();
    dom.mutate(|cx| {
        let doc = cx.create_document(true);
        let text = cx.create_text(doc, "héllo");
        let div = cx.create_element(doc, QualName::html("div"));
        let data = cx.data(text).unwrap();
        assert_eq!(data, "héllo");
        assert!(!data.is_ascii());
        cx.set_data(text, "bye");
        assert_eq!(cx.data(text).unwrap(), "bye");
        // What was read before keeps its value: strings don't change.
        assert_eq!(data, "héllo");
        assert_eq!(cx.data(div), None);
        assert_eq!(cx.data(doc), None);
    });
}

/// Text data isn't interned: equal data, two strings.
#[test]
fn text_data_is_not_interned() {
    let mut dom = dom::Dom::new();
    dom.mutate(|cx| {
        let doc = cx.create_document(true);
        let a = cx.data(cx.create_text(doc, "same")).unwrap();
        let b = cx.data(cx.create_text(doc, "same")).unwrap();
        assert_eq!(a, b);
        assert!(!DomString::ptr_eq(&a, &b));
    });
    assert_eq!(dom.interned_values(), 0);
}

#[test]
fn text_content_joins_the_descendant_texts_in_tree_order() {
    let mut dom = dom::Dom::new();
    dom.mutate(|cx| {
        let doc = cx.create_document(true);
        let div = cx.create_element(doc, QualName::html("div"));
        let span = cx.create_element(doc, QualName::html("span"));
        let em = cx.create_element(doc, QualName::html("em"));
        let sibling = cx.create_element(doc, QualName::html("b"));
        let outside = cx.create_element(doc, QualName::html("i"));
        cx.append_child(div, cx.create_text(doc, "a")).unwrap();
        cx.append_child(div, span).unwrap();
        cx.append_child(span, cx.create_text(doc, "b")).unwrap();
        cx.append_child(span, em).unwrap();
        cx.append_child(em, cx.create_text(doc, "c")).unwrap();
        cx.append_child(span, cx.create_text(doc, "d")).unwrap();
        cx.append_child(div, sibling).unwrap();
        cx.append_child(div, cx.create_text(doc, "e")).unwrap();
        cx.append_child(outside, cx.create_text(doc, "not"))
            .unwrap();
        cx.append_child(sibling, outside).unwrap();
        assert_eq!(cx.text_content(div).unwrap(), "abcdnote");
        // Only the descendants: not what follows the node itself.
        assert_eq!(cx.text_content(span).unwrap(), "bcd");
        assert_eq!(cx.text_content(em).unwrap(), "c");
        let empty = cx.create_element(doc, QualName::html("p"));
        assert_eq!(cx.text_content(empty).unwrap(), "");
        let text = cx.create_text(doc, "t");
        assert_eq!(cx.text_content(text).unwrap(), "t");
        assert_eq!(cx.text_content(doc), None);
    });
}

#[test]
fn setting_text_content_replaces_an_elements_children() {
    let mut dom = dom::Dom::new();
    dom.mutate(|cx| {
        let doc = cx.create_document(true);
        let div = cx.create_element(doc, QualName::html("div"));
        let old = cx.create_element(doc, QualName::html("span"));
        cx.append_child(div, old).unwrap();
        cx.append_child(div, cx.create_text(doc, "x")).unwrap();
        cx.set_text_content(div, "new");
        assert_eq!(children(cx, div), ["#text"]);
        assert_eq!(cx.text_content(div).unwrap(), "new");
        assert!(cx.parent(old).is_none());
        let text = cx.first_child(div).unwrap();
        assert!(elise::Gc::ptr_eq(cx.document_of(text), doc));
        // Empty: no text at all.
        cx.set_text_content(div, "");
        assert!(cx.first_child(div).is_none() && cx.last_child(div).is_none());
        // A text's data; a document: nothing.
        let text = cx.create_text(doc, "a");
        cx.set_text_content(text, "b");
        assert_eq!(cx.data(text).unwrap(), "b");
        let root = cx.create_element(doc, QualName::html("html"));
        cx.append_child(doc, root).unwrap();
        cx.set_text_content(doc, "ignored");
        assert_eq!(children(cx, doc), ["HTML"]);
    });
}

#[test]
#[should_panic(expected = "a character data node's data")]
fn only_character_data_has_data_to_set() {
    let mut dom = dom::Dom::new();
    dom.mutate(|cx| {
        let doc = cx.create_document(true);
        let div = cx.create_element(doc, QualName::html("div"));
        cx.set_data(div, "x");
    });
}
