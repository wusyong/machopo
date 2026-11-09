//! An `<iframe>`'s bookkeeping: what happened to the iframes, recorded in
//! order for the engine's frame steps (`take_element_changes`, asked
//! for by `Microtasks::elements`): connected and disconnected (in shadow
//! trees too), `src` and `srcdoc` changed while connected; nothing before
//! the first iframe is made, nor for other elements and attributes. Its
//! content document on both sides (`content_document`, `frame_owner`);
//! discarded with those below it as its browsing context goes.

use dom::ElementChange::Frame;
use dom::{
    Cx, Dom, FrameOwnerChange, NodeRef, QualName, ShadowRootInit, ShadowRootMode,
    SlotAssignmentMode,
};

/// A document with a window and a `<body>`: the `<body>`.
fn page<'gc>(cx: &Cx<'_, 'gc>) -> NodeRef<'gc> {
    let document = cx.create_document(true);
    cx.create_window(document);
    let html = cx.create_element(document, QualName::html("html"));
    cx.append_child(document, html).unwrap();
    let body = cx.create_element(document, QualName::html("body"));
    cx.append_child(html, body).unwrap();
    body
}

#[test]
fn what_happens_to_iframes_is_recorded_in_order() {
    let mut dom = Dom::new();
    let (body, div) = dom.mutate(|cx| {
        let body = page(cx);
        let div = cx.create_element(cx.document_of(body), QualName::html("div"));
        cx.append_child(body, div).unwrap();
        cx.remove(div);
        assert!(!cx.has_element_changes(), "no iframe made yet");
        (cx.root(body), cx.root(div))
    });
    assert!(!dom.take_microtasks().elements());
    let iframe = dom.mutate(|cx| {
        let (body, div) = (body.get(cx.gc), div.get(cx.gc));
        let iframe = cx.create_element(cx.document_of(body), QualName::html("iframe"));
        cx.set_attribute(iframe, "src", "a.html");
        assert!(!cx.has_element_changes(), "not connected");
        cx.append_child(div, iframe).unwrap();
        assert!(!cx.has_element_changes(), "in a disconnected tree");
        cx.append_child(body, div).unwrap();
        assert!(cx.has_element_changes());
        cx.root(iframe)
    });
    assert!(dom.take_microtasks().elements());
    dom.mutate(|cx| {
        let (body, div, iframe) = (body.get(cx.gc), div.get(cx.gc), iframe.get(cx.gc));
        let document = cx.document_of(body);
        cx.set_attribute(iframe, "src", "b.html");
        cx.set_attribute(iframe, "srcdoc", "<p>");
        cx.set_attribute(iframe, "name", "n");
        cx.set_attribute(div, "src", "c.html");
        cx.remove_attribute(iframe, "srcdoc");
        cx.remove(div);
        cx.set_attribute(iframe, "src", "d.html");
        let id = cx.node_id(iframe);
        assert_eq!(
            cx.take_element_changes(),
            [
                Frame(FrameOwnerChange::Inserted(id)),
                Frame(FrameOwnerChange::SrcChanged(id)),
                Frame(FrameOwnerChange::SrcdocChanged(id)),
                Frame(FrameOwnerChange::SrcdocChanged(id)),
                Frame(FrameOwnerChange::Removed(id)),
            ]
        );
        assert!(!cx.has_element_changes(), "taken");
        assert!(cx.take_element_changes().is_empty());

        // In a shadow tree, connected through its host.
        let host = cx.create_element(document, QualName::html("div"));
        let root = cx.attach_shadow(
            host,
            ShadowRootInit {
                mode: ShadowRootMode::Open,
                delegates_focus: false,
                slot_assignment: SlotAssignmentMode::Named,
                clonable: false,
                serializable: false,
            },
        );
        let shadowed = cx.create_element(document, QualName::html("iframe"));
        cx.append_child(root, shadowed).unwrap();
        cx.append_child(body, host).unwrap();
        cx.remove(host);
        let id = cx.node_id(shadowed);
        assert_eq!(
            cx.take_element_changes(),
            [
                Frame(FrameOwnerChange::Inserted(id)),
                Frame(FrameOwnerChange::Removed(id))
            ]
        );
    });
}

#[test]
fn a_discarded_content_document_takes_those_below_it() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let body = page(cx);
        let iframe = cx.create_element(cx.document_of(body), QualName::html("iframe"));
        cx.append_child(body, iframe).unwrap();
        let child = page(cx);
        let inner = cx.create_element(cx.document_of(child), QualName::html("iframe"));
        cx.append_child(child, inner).unwrap();
        let grandchild = page(cx);
        let (child, grandchild) = (cx.document_of(child), cx.document_of(grandchild));
        cx.set_content_document(iframe, Some(child));
        cx.set_content_document(inner, Some(grandchild));
        assert_eq!(id(cx, cx.content_document(iframe)), Some(cx.node_id(child)));
        assert_eq!(id(cx, cx.frame_owner(child)), Some(cx.node_id(iframe)));
        assert_eq!(id(cx, cx.frame_owner(grandchild)), Some(cx.node_id(inner)));

        assert!(!cx.is_discarded(child));
        assert_eq!(
            id(cx, cx.discard_content_document(iframe)),
            Some(cx.node_id(child))
        );
        assert!(cx.is_discarded(child));
        assert!(cx.is_discarded(grandchild));
        assert!(!cx.is_discarded(cx.document_of(body)));
        assert_eq!(id(cx, cx.content_document(iframe)), None);
        assert_eq!(id(cx, cx.content_document(inner)), None);
        assert_eq!(id(cx, cx.frame_owner(child)), None);
        assert_eq!(id(cx, cx.frame_owner(grandchild)), None);
        assert!(cx.discard_content_document(iframe).is_none(), "none left");

        // Another document for an iframe: the one before is no longer its,
        // and is discarded with those below it (unloaded); the new one
        // isn't.
        let first = page(cx);
        let nested = cx.create_element(cx.document_of(first), QualName::html("iframe"));
        cx.append_child(first, nested).unwrap();
        let (first, below) = (cx.document_of(first), cx.document_of(page(cx)));
        cx.set_content_document(iframe, Some(first));
        cx.set_content_document(nested, Some(below));
        let other = cx.document_of(page(cx));
        cx.set_content_document(iframe, Some(other));
        assert_eq!(id(cx, cx.frame_owner(first)), None);
        assert!(cx.is_discarded(first));
        assert!(cx.is_discarded(below));
        assert_eq!(id(cx, cx.content_document(nested)), None);
        assert!(!cx.is_discarded(other));
        assert_eq!(id(cx, cx.frame_owner(other)), Some(cx.node_id(iframe)));
        // The same one again: nothing discarded.
        cx.set_content_document(iframe, Some(other));
        assert!(!cx.is_discarded(other));
    });
}

/// `node`'s id, if there is one.
fn id<'gc>(cx: &Cx<'_, 'gc>, node: Option<NodeRef<'gc>>) -> Option<dom::NodeId> {
    node.map(|node| cx.node_id(node))
}

/// The focused frame of a tree: the one set while a frame of the tree shows
/// it, the top-level document once it is discarded.
#[test]
fn the_focused_frame_is_one_the_tree_shows() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let body = page(cx);
        let top = cx.document_of(body);
        let iframe = cx.create_element(top, QualName::html("iframe"));
        cx.append_child(body, iframe).unwrap();
        let child = cx.document_of(page(cx));
        cx.set_content_document(iframe, Some(child));
        assert_eq!(id(cx, Some(cx.focused_frame(top))), Some(cx.node_id(top)));
        assert_eq!(id(cx, Some(cx.top_document(child))), Some(cx.node_id(top)));
        cx.set_focused_frame(top, child);
        assert_eq!(id(cx, Some(cx.focused_frame(top))), Some(cx.node_id(child)));
        cx.discard_content_document(iframe);
        assert_eq!(id(cx, Some(cx.focused_frame(top))), Some(cx.node_id(top)));
    });
}

/// The base URL of `document`, as text.
fn base<'gc>(cx: &Cx<'_, 'gc>, document: NodeRef<'gc>) -> String {
    cx.document_base_url(document)
        .map(|url| url.to_string())
        .unwrap_or_default()
}

/// An iframe's `about:blank` takes its parent's base URL as it is made,
/// and keeps it (HTML's about base URL); a srcdoc document's follows its
/// parent's (also with a fragment: it is still the srcdoc document); either
/// with a `<base>` of its own goes by it.
#[test]
fn about_documents_base_urls() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let body = page(cx);
        let parent = cx.document_of(body);
        cx.set_document_url(parent, "https://example.test/a/page");
        let base_element = cx.create_element(parent, QualName::html("base"));
        cx.set_attribute(base_element, "href", "https://example.test/first/");
        cx.append_child(body, base_element).unwrap();
        let frame = |url: &str| {
            let iframe = cx.create_element(parent, QualName::html("iframe"));
            cx.append_child(body, iframe).unwrap();
            let child_body = page(cx);
            let child = cx.document_of(child_body);
            cx.set_document_url(child, url);
            cx.set_content_document(iframe, Some(child));
            (child, child_body)
        };
        let (blank, blank_body) = frame("about:blank");
        let (srcdoc, _) = frame("about:srcdoc#x");
        assert_eq!(base(cx, blank), "https://example.test/first/");
        assert_eq!(base(cx, srcdoc), "https://example.test/first/");

        cx.set_attribute(base_element, "href", "https://example.test/second/");
        assert_eq!(
            base(cx, blank),
            "https://example.test/first/",
            "kept as made"
        );
        assert_eq!(
            base(cx, srcdoc),
            "https://example.test/second/",
            "its parent's now"
        );

        let own = cx.create_element(blank, QualName::html("base"));
        cx.set_attribute(own, "href", "own/");
        cx.append_child(blank_body, own).unwrap();
        assert_eq!(base(cx, blank), "https://example.test/first/own/");
    });
}
