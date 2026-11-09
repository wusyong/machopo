//! The trees' changes noted for DevTools' `DOM` agent: while the host
//! wants them, each insertion (with the sibling before), removal,
//! attribute set or removed, text changed, inline style set and shadow
//! root attached, in order, until taken; nothing while it doesn't, and
//! those noted dropped once it stops.

use std::sync::Arc;

use dom::{
    Cx, Dom, DomChange, NodeRef, QualName, ShadowRootInit, ShadowRootMode, SlotAssignmentMode,
};

/// A document with a `<body>`: the `<body>`.
fn page<'gc>(cx: &Cx<'_, 'gc>) -> NodeRef<'gc> {
    let document = cx.create_document(true);
    let html = cx.create_element(document, QualName::html("html"));
    cx.append_child(document, html).unwrap();
    let body = cx.create_element(document, QualName::html("body"));
    cx.append_child(html, body).unwrap();
    body
}

#[test]
fn changes_are_noted_in_order_while_wanted() {
    let mut dom = Dom::new();
    let body = dom.mutate(|cx| cx.root(page(cx)));
    // Not wanted: nothing.
    dom.mutate(|cx| {
        let body = body.get(cx.gc);
        let p = cx.create_element(cx.document_of(body), QualName::html("p"));
        cx.append_child(body, p).unwrap();
    });
    assert!(dom.take_dom_changes().is_empty());

    dom.set_dom_changes_wanted(true);
    let (expected, root) = dom.mutate(|cx| {
        let body = body.get(cx.gc);
        let document = cx.document_of(body);
        let first = cx.first_child(body).unwrap();
        let div = cx.create_element(document, QualName::html("div"));
        cx.append_child(body, div).unwrap();
        cx.set_attribute(div, "title", "hi");
        let title = cx.attribute_name_at(div, 0).unwrap();
        cx.remove_attribute(div, "title");
        let text = cx.create_text(document, "a");
        cx.append_child(div, text).unwrap();
        cx.set_data(text, "b");
        cx.set_inline_style(div, Arc::new(css::CssPropertyValueSet::parse("color: red")));
        let root = cx.attach_shadow(
            div,
            ShadowRootInit {
                mode: ShadowRootMode::Open,
                delegates_focus: false,
                slot_assignment: SlotAssignmentMode::Named,
                clonable: false,
                serializable: false,
            },
        );
        cx.remove(first);
        let id = |node| cx.node_id(node);
        (
            vec![
                DomChange::Inserted {
                    parent: id(body),
                    node: id(div),
                    previous: Some(id(first)),
                },
                DomChange::Attribute {
                    element: id(div),
                    name: title,
                },
                DomChange::Attribute {
                    element: id(div),
                    name: title,
                },
                DomChange::Inserted {
                    parent: id(div),
                    node: id(text),
                    previous: None,
                },
                DomChange::CharacterData { node: id(text) },
                DomChange::InlineStyle { element: id(div) },
                DomChange::ShadowAttached {
                    host: id(div),
                    root: id(root),
                },
                DomChange::Removed {
                    parent: id(body),
                    node: id(first),
                },
            ],
            cx.root(root),
        )
    });
    assert_eq!(dom.take_dom_changes(), expected);
    // Taken: none left.
    assert!(dom.take_dom_changes().is_empty());
    drop(root);

    // Noted, then not wanted: dropped.
    dom.mutate(|cx| {
        let body = body.get(cx.gc);
        cx.set_attribute(body, "class", "x");
    });
    dom.set_dom_changes_wanted(false);
    assert!(dom.take_dom_changes().is_empty());
}
