//! The tree: nodes' kinds and names, inserting, removing and moving them
//! (the links both ways, and the node document), and pre-insert's and
//! pre-remove's errors.

use dom::{Dom, DomError, NodeKind, QualName, names};
use elise::Gc;

use crate::{children, children_backwards};

const SVG: &str = "http://www.w3.org/2000/svg";

#[test]
fn nodes_have_their_kinds_types_and_names() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let div = cx.create_element(document, QualName::html("div"));
        let text = cx.create_text(document, "hello");
        assert_eq!(cx.kind(document), NodeKind::Document);
        assert_eq!(cx.kind(div), NodeKind::Element);
        assert_eq!(cx.kind(text), NodeKind::Text);
        assert_eq!(
            [document, div, text].map(|node| cx.node_type(node)),
            [9, 1, 3]
        );
        assert_eq!(cx.node_name(document), "#document");
        assert_eq!(cx.node_name(text), "#text");
        let name = cx.element_name(div).unwrap();
        assert_eq!(name.namespace, names::HTML_NAMESPACE);
        assert_eq!(name.prefix, names::NONE);
        assert_eq!(names::get(name.local), "div");
        assert_eq!(cx.element_name(text), None);
        assert_eq!(cx.element_name(document), None);
    });
}

/// `nodeName`: an HTML element's qualified name uppercased in an HTML
/// document only; others' as they are, a prefix and a colon first.
#[test]
fn node_names_are_uppercased_for_html_elements_in_html_documents() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let html = cx.create_document(true);
        let xml = cx.create_document(false);
        let in_html = cx.create_element(html, QualName::html("dIv"));
        let in_xml = cx.create_element(xml, QualName::html("dIv"));
        let svg = cx.create_element(
            html,
            QualName {
                namespace: Some(SVG),
                prefix: None,
                local: "foreignObject",
            },
        );
        let prefixed = cx.create_element(
            html,
            QualName {
                namespace: Some("http://www.w3.org/1999/xhtml"),
                prefix: Some("h"),
                local: "p",
            },
        );
        let prefixed_svg = cx.create_element(
            html,
            QualName {
                namespace: Some(SVG),
                prefix: Some("s"),
                local: "g",
            },
        );
        let no_namespace = cx.create_element(html, QualName::local("x"));
        assert_eq!(cx.node_name(in_html), "DIV");
        assert_eq!(cx.node_name(in_xml), "dIv");
        assert_eq!(cx.node_name(svg), "foreignObject");
        assert_eq!(cx.node_name(prefixed), "H:P");
        assert_eq!(cx.node_name(prefixed_svg), "s:g");
        assert_eq!(cx.node_name(no_namespace), "x");
        assert_eq!(
            cx.element_name(no_namespace).unwrap().namespace,
            names::NONE
        );
    });
}

/// The same name is the same number; an empty namespace or prefix is none.
/// An element's `nodeName` that isn't its local name (uppercased, or with
/// a prefix) is one string of the DOM's names, made the first time: the
/// same string for every element of that name, each time.
#[test]
fn node_names_are_made_once() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let html = cx.create_document(true);
        let [a, b] = [0, 1].map(|_| cx.create_element(html, QualName::html("div")));
        let first = cx.node_name(a);
        assert_eq!(first.to_str(), "DIV");
        assert!(dom::DomString::ptr_eq(&first, &cx.node_name(a)));
        assert!(dom::DomString::ptr_eq(&first, &cx.node_name(b)));
        assert_eq!(cx.node_name_id(a), cx.node_name_id(b));
        let svg = QualName {
            namespace: Some("http://www.w3.org/2000/svg"),
            prefix: Some("s"),
            local: "rect",
        };
        let [c, d] = [0, 1].map(|_| cx.create_element(html, svg));
        assert_eq!(cx.node_name(c).to_str(), "s:rect");
        assert!(dom::DomString::ptr_eq(&cx.node_name(c), &cx.node_name(d)));
        // The same name otherwise cased is another.
        let xml = cx.create_document(false);
        let e = cx.create_element(xml, QualName::html("div"));
        assert_eq!(cx.node_name(e).to_str(), "div");
        assert_ne!(cx.node_name_id(e), cx.node_name_id(a));
    });
}

#[test]
fn names_are_interned() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let doc = cx.create_document(true);
        let a = cx.create_element(doc, QualName::html("p"));
        let b = cx.create_element(doc, QualName::html("p"));
        let empty = cx.create_element(
            doc,
            QualName {
                namespace: Some(""),
                prefix: Some(""),
                local: "p",
            },
        );
        let svg = cx.create_element(
            doc,
            QualName {
                namespace: Some(SVG),
                prefix: None,
                local: "p",
            },
        );
        let (a, b, empty, svg) = (
            cx.element_name(a).unwrap(),
            cx.element_name(b).unwrap(),
            cx.element_name(empty).unwrap(),
            cx.element_name(svg).unwrap(),
        );
        assert_eq!(a, b);
        assert_eq!(a.local, empty.local);
        assert_eq!((empty.namespace, empty.prefix), (names::NONE, names::NONE));
        assert_eq!(svg.namespace, names::SVG_NAMESPACE);
        assert_eq!(names::get(names::NONE), "");
    });
}

#[test]
fn children_are_inserted_where_asked_and_linked_both_ways() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let doc = cx.create_document(true);
        let parent = cx.create_element(doc, QualName::html("div"));
        let [a, b, c] = ["a", "b", "c"].map(|name| cx.create_element(doc, QualName::local(name)));
        cx.append_child(parent, c).unwrap();
        cx.pre_insert(parent, a, Some(c)).unwrap();
        cx.pre_insert(parent, b, Some(c)).unwrap();
        assert_eq!(children(cx, parent), ["a", "b", "c"]);
        assert_eq!(children_backwards(cx, parent), ["a", "b", "c"]);
        for child in [a, b, c] {
            assert!(Gc::ptr_eq(cx.parent(child).unwrap(), parent));
        }
        assert!(cx.previous_sibling(a).is_none());
        assert!(cx.next_sibling(c).is_none());
        // Before itself: where it is.
        assert!(Gc::ptr_eq(cx.pre_insert(parent, b, Some(b)).unwrap(), b));
        assert_eq!(children(cx, parent), ["a", "b", "c"]);
        // Moving a child: out of its place, into the new one.
        cx.pre_insert(parent, c, Some(a)).unwrap();
        assert_eq!(children(cx, parent), ["c", "a", "b"]);
        assert_eq!(children_backwards(cx, parent), ["c", "a", "b"]);
        cx.append_child(parent, c).unwrap();
        assert_eq!(children(cx, parent), ["a", "b", "c"]);
        assert_eq!(children_backwards(cx, parent), ["a", "b", "c"]);
    });
}

#[test]
fn a_node_moved_to_another_parent_leaves_the_first() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let doc = cx.create_document(true);
        let [one, two] = ["one", "two"].map(|name| cx.create_element(doc, QualName::local(name)));
        let [a, b, c] = ["a", "b", "c"].map(|name| cx.create_element(doc, QualName::local(name)));
        for child in [a, b, c] {
            cx.append_child(one, child).unwrap();
        }
        cx.append_child(two, b).unwrap();
        assert_eq!(children(cx, one), ["a", "c"]);
        assert_eq!(children_backwards(cx, one), ["a", "c"]);
        assert_eq!(children(cx, two), ["b"]);
        assert!(Gc::ptr_eq(cx.parent(b).unwrap(), two));
        // The first and last move too.
        cx.append_child(two, a).unwrap();
        cx.append_child(two, c).unwrap();
        assert!(cx.first_child(one).is_none() && cx.last_child(one).is_none());
        assert_eq!(children(cx, two), ["b", "a", "c"]);
        assert_eq!(children_backwards(cx, two), ["b", "a", "c"]);
    });
}

#[test]
fn removed_children_are_unlinked() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let doc = cx.create_document(true);
        let parent = cx.create_element(doc, QualName::local("p"));
        let [a, b, c] = ["a", "b", "c"].map(|name| cx.create_element(doc, QualName::local(name)));
        for child in [a, b, c] {
            cx.append_child(parent, child).unwrap();
        }
        assert!(Gc::ptr_eq(cx.pre_remove(parent, b).unwrap(), b));
        assert_eq!(children(cx, parent), ["a", "c"]);
        assert_eq!(children_backwards(cx, parent), ["a", "c"]);
        assert!(cx.parent(b).is_none());
        assert!(cx.previous_sibling(b).is_none() && cx.next_sibling(b).is_none());
        cx.pre_remove(parent, a).unwrap();
        cx.pre_remove(parent, c).unwrap();
        assert!(cx.first_child(parent).is_none() && cx.last_child(parent).is_none());
        // Removing a node with no parent does nothing.
        cx.remove(a);
        assert!(cx.parent(a).is_none());
    });
}

#[test]
fn pre_insert_and_pre_remove_refuse_what_the_dom_does() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let doc = cx.create_document(true);
        let other = cx.create_document(true);
        let parent = cx.create_element(doc, QualName::local("p"));
        let child = cx.create_element(doc, QualName::local("c"));
        let stranger = cx.create_element(doc, QualName::local("s"));
        let text = cx.create_text(doc, "t");
        cx.append_child(parent, child).unwrap();
        // A text can't have children.
        assert_eq!(
            cx.append_child(text, stranger).err(),
            Some(DomError::HierarchyRequest)
        );
        // A node into itself or its descendant.
        assert_eq!(
            cx.append_child(parent, parent).err(),
            Some(DomError::HierarchyRequest)
        );
        assert_eq!(
            cx.append_child(child, parent).err(),
            Some(DomError::HierarchyRequest)
        );
        // Before a node that isn't a child.
        assert_eq!(
            cx.pre_insert(parent, stranger, Some(stranger)).err(),
            Some(DomError::NotFound)
        );
        // A document anywhere; a text in a document; a second element in a
        // document.
        assert_eq!(
            cx.append_child(parent, other).err(),
            Some(DomError::HierarchyRequest)
        );
        assert_eq!(
            cx.append_child(doc, text).err(),
            Some(DomError::HierarchyRequest)
        );
        let root = cx.create_element(doc, QualName::html("html"));
        cx.append_child(doc, root).unwrap();
        assert_eq!(
            cx.append_child(doc, stranger).err(),
            Some(DomError::HierarchyRequest)
        );
        // Removing a node that isn't a child.
        assert_eq!(
            cx.pre_remove(parent, stranger).err(),
            Some(DomError::NotFound)
        );
        // Nothing changed.
        assert_eq!(children(cx, parent), ["c"]);
        assert_eq!(children(cx, doc), ["HTML"]);
        assert!(cx.parent(stranger).is_none());
    });
}

/// A node moved into another document's tree is adopted: it and its
/// descendants take that node document.
#[test]
fn a_node_inserted_into_another_document_is_adopted_with_its_descendants() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let first = cx.create_document(true);
        let second = cx.create_document(false);
        let target = cx.create_element(second, QualName::local("t"));
        let moved = cx.create_element(first, QualName::html("div"));
        let inner = cx.create_element(first, QualName::html("span"));
        let text = cx.create_text(first, "x");
        cx.append_child(moved, inner).unwrap();
        cx.append_child(inner, text).unwrap();
        assert_eq!(cx.node_name(moved), "DIV");
        cx.append_child(target, moved).unwrap();
        for node in [moved, inner, text] {
            assert!(Gc::ptr_eq(cx.owner_document(node).unwrap(), second));
            assert!(Gc::ptr_eq(cx.document_of(node), second));
        }
        // An XML document's HTML element keeps its name's case.
        assert_eq!(cx.node_name(moved), "div");
        assert!(cx.owner_document(second).is_none());
        assert!(Gc::ptr_eq(cx.document_of(second), second));
    });
}

/// `insert` doesn't check (the parser's): it moves the node as pre-insert
/// would.
#[test]
fn insert_moves_a_node_without_checks() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let doc = cx.create_document(true);
        let one = cx.create_element(doc, QualName::local("one"));
        let two = cx.create_element(doc, QualName::local("two"));
        let [a, b] = ["a", "b"].map(|name| cx.create_element(doc, QualName::local(name)));
        cx.insert(one, a, None);
        cx.insert(one, b, Some(a));
        assert_eq!(children(cx, one), ["b", "a"]);
        cx.insert(two, b, None);
        assert_eq!(children(cx, one), ["a"]);
        assert_eq!(children_backwards(cx, one), ["a"]);
        assert_eq!(children(cx, two), ["b"]);
    });
}

/// In a DOM no JS engine drives, a node's wrapper's bytes are its engine's
/// word: 0 at first, each node's own; in one a JS engine drives, they
/// aren't.
#[test]
fn a_nodes_engine_word_is_its_own_where_no_js_engine_drives() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let div = cx.create_element(document, QualName::html("div"));
        assert_eq!((cx.engine_word(document), cx.engine_word(div)), (0, 0));
        cx.set_engine_word(div, u64::MAX);
        assert_eq!(
            (cx.engine_word(document), cx.engine_word(div)),
            (0, u64::MAX)
        );
    });
    let mut driven = Dom::with_options(elise::HeapOptions {
        driven: true,
        ..elise::HeapOptions::default()
    });
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        driven.mutate(|cx| cx.engine_word(cx.create_document(true)))
    }));
    assert!(result.is_err());
}

/// `childNodes`' cache is cleared by every change of the children: an
/// insertion, a removal, `textContent`'s and a clone's links.
#[test]
fn a_child_list_follows_every_change_of_the_children() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let parent = cx.create_element(document, QualName::html("div"));
        let list = cx.child_nodes(parent);
        assert!(Gc::ptr_eq(list, cx.child_nodes(parent)));
        let a = cx.create_element(document, QualName::html("a"));
        let b = cx.create_element(document, QualName::html("b"));
        cx.append_child(parent, a).unwrap();
        cx.append_child(parent, b).unwrap();
        assert_eq!(cx.node_list_length(list), 2);
        assert!(
            cx.node_list_item(list, 1)
                .is_some_and(|at| Gc::ptr_eq(at, b))
        );
        cx.remove(a);
        assert_eq!(cx.node_list_length(list), 1);
        assert!(
            cx.node_list_item(list, 0)
                .is_some_and(|at| Gc::ptr_eq(at, b))
        );
        assert!(cx.node_list_item(list, 1).is_none());
        cx.set_text_content(parent, "x");
        assert_eq!(cx.node_list_length(list), 1);
        assert_eq!(cx.kind(cx.node_list_item(list, 0).unwrap()), NodeKind::Text);
        cx.set_text_content(parent, "");
        assert_eq!(cx.node_list_length(list), 0);
        // A child more past the cached one, the length known.
        for name in ["i", "u", "s"] {
            cx.append_child(parent, cx.create_element(document, QualName::html(name)))
                .unwrap();
            assert_eq!(
                cx.node_list_length(list),
                u32::try_from(children(cx, parent).len()).unwrap()
            );
        }
        assert_eq!(
            cx.node_name(cx.node_list_item(list, 2).unwrap()).to_str(),
            "S"
        );
        assert_eq!(
            cx.node_name(cx.node_list_item(list, 0).unwrap()).to_str(),
            "I"
        );
        assert_eq!(
            cx.node_name(cx.node_list_item(list, 1).unwrap()).to_str(),
            "U"
        );
        // Backwards from the last, the length known.
        assert_eq!(cx.node_list_length(list), 3);
        assert_eq!(
            cx.node_name(cx.node_list_item(list, 2).unwrap()).to_str(),
            "S"
        );
        // The content of a template cloned deep: linked into the copy.
        let copy = cx.clone_node(parent, None, false);
        let copy_list = cx.child_nodes(copy);
        assert_eq!(cx.node_list_length(copy_list), 0);
        cx.insert(copy, cx.create_text(document, "t"), None);
        assert_eq!(cx.node_list_length(copy_list), 1);
    });
}

/// A host's `textContent` set (its children linked, not inserted): the
/// new text gets its slot.
#[test]
fn text_content_on_a_host_is_slotted() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let host = cx.create_element(document, QualName::html("div"));
        let root = cx.attach_shadow(
            host,
            dom::ShadowRootInit {
                mode: dom::ShadowRootMode::Open,
                delegates_focus: false,
                slot_assignment: dom::SlotAssignmentMode::Named,
                clonable: false,
                serializable: false,
            },
        );
        let slot = cx.create_element(document, QualName::html("slot"));
        cx.append_child(root, slot).unwrap();
        assert!(cx.assigned_nodes(slot).is_empty());
        cx.take_signal_slots();
        cx.set_text_content(host, "x");
        let text = cx.first_child(host).unwrap();
        assert!(
            cx.assigned_slot(text)
                .is_some_and(|of| Gc::ptr_eq(of, slot))
        );
        assert_eq!(cx.assigned_nodes(slot).len(), 1);
        assert_eq!(cx.take_signal_slots().len(), 1);
    });
}

#[test]
fn replace_child_checks_and_moves() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let parent = cx.create_element(document, QualName::html("div"));
        let [a, b, c] =
            ["a", "b", "c"].map(|name| cx.create_element(document, QualName::html(name)));
        for child in [a, b, c] {
            cx.append_child(parent, child).unwrap();
        }
        let new = cx.create_element(document, QualName::html("u"));
        assert!(
            cx.replace_child(parent, new, b)
                .is_ok_and(|old| Gc::ptr_eq(old, b))
        );
        assert!(cx.parent(b).is_none());
        assert_eq!(children(cx, parent), ["A", "U", "C"]);
        // Its own sibling moved into another's place.
        cx.replace_child(parent, c, a).unwrap();
        assert_eq!(children(cx, parent), ["C", "U"]);
        assert_eq!(children_backwards(cx, parent).len(), 2);
        assert_eq!(
            cx.replace_child(parent, a, b).err(),
            Some(DomError::NotFound)
        );
        assert_eq!(cx.replace_child(c, new, c).err(), Some(DomError::NotFound));
        assert_eq!(
            cx.replace_child(c, parent, c).err(),
            Some(DomError::HierarchyRequest)
        );
        cx.append_child(c, a).unwrap();
        assert_eq!(
            cx.replace_child(c, parent, a).err(),
            Some(DomError::HierarchyRequest)
        );
        // A document's doctype before its element, one of each.
        let doctype = cx.create_doctype(document, "html", "", "");
        let html = cx.create_element(document, QualName::html("html"));
        cx.append_child(document, doctype).unwrap();
        cx.append_child(document, html).unwrap();
        let other = cx.create_element(document, QualName::html("html"));
        assert!(cx.replace_child(document, other, html).is_ok());
        assert_eq!(
            cx.replace_child(document, html, doctype).err(),
            Some(DomError::HierarchyRequest)
        );
        let second = cx.create_doctype(document, "html", "", "");
        assert!(cx.replace_child(document, second, doctype).is_ok());
        assert_eq!(
            cx.replace_child(document, cx.create_text(document, "t"), other)
                .err(),
            Some(DomError::HierarchyRequest)
        );
    });
}

/// DOM's "replace"'s checks of a document's children: one element after
/// the doctype, one doctype before it, no text; and of the parent.
#[test]
fn replace_child_keeps_a_document_well_formed() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let text = cx.create_text(document, "t");
        let lone = cx.create_element(document, QualName::html("a"));
        assert_eq!(
            cx.replace_child(text, lone, text).err(),
            Some(DomError::HierarchyRequest)
        );
        // An element before the doctype.
        let comment = cx.create_comment(document, "c");
        let doctype = cx.create_doctype(document, "html", "", "");
        cx.append_child(document, comment).unwrap();
        cx.append_child(document, doctype).unwrap();
        let element = cx.create_element(document, QualName::html("html"));
        assert_eq!(
            cx.replace_child(document, element, comment).err(),
            Some(DomError::HierarchyRequest)
        );
        // A fragment: no text, at most one element.
        let fragment = cx.create_document_fragment(document);
        cx.append_child(fragment, cx.create_text(document, "x"))
            .unwrap();
        assert_eq!(
            cx.replace_child(document, fragment, doctype).err(),
            Some(DomError::HierarchyRequest)
        );
        let two = cx.create_document_fragment(document);
        for name in ["a", "b"] {
            cx.append_child(two, cx.create_element(document, QualName::html(name)))
                .unwrap();
        }
        let after = cx.create_comment(document, "after");
        cx.append_child(document, after).unwrap();
        assert_eq!(
            cx.replace_child(document, two, after).err(),
            Some(DomError::HierarchyRequest)
        );
        // A doctype after the element.
        cx.append_child(document, element).unwrap();
        let tail = cx.create_comment(document, "tail");
        cx.append_child(document, tail).unwrap();
        let second = cx.create_doctype(document, "html", "", "");
        cx.remove(doctype);
        assert_eq!(
            cx.replace_child(document, second, tail).err(),
            Some(DomError::HierarchyRequest)
        );
        assert!(cx.replace_child(document, second, comment).is_ok());
    });
}

#[test]
fn equal_nodes_are_of_one_kind_and_have_the_same() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        assert!(!cx.is_equal_node(
            cx.create_text(document, "x"),
            cx.create_comment(document, "x")
        ));
        assert!(cx.is_equal_node(
            cx.create_comment(document, "x"),
            cx.create_comment(document, "x")
        ));
        assert!(!cx.is_equal_node(
            cx.create_comment(document, "x"),
            cx.create_comment(document, "y")
        ));
        let doctype = |name, public| cx.create_doctype(document, name, public, "");
        assert!(cx.is_equal_node(doctype("html", ""), doctype("html", "")));
        assert!(!cx.is_equal_node(doctype("html", ""), doctype("svg", "")));
        assert!(!cx.is_equal_node(doctype("html", ""), doctype("html", "p")));
    });
}

/// A quirks mode document's classes match ASCII case-insensitively.
#[test]
fn classes_match_in_any_case_in_quirks_mode() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        for (quirks, found) in [(dom::QuirksMode::NoQuirks, 1), (dom::QuirksMode::Quirks, 2)] {
            let document = cx.create_document(true);
            cx.set_quirks_mode(document, quirks);
            let root = cx.create_element(document, QualName::html("div"));
            cx.append_child(document, root).unwrap();
            for class in ["Big", "big"] {
                let child = cx.create_element(document, QualName::html("p"));
                cx.set_attribute(child, "class", class);
                cx.append_child(root, child).unwrap();
            }
            let collection = cx.elements_by_class_name(root, "big");
            assert_eq!(cx.collection_length(collection), found);
        }
    });
}

/// The children's links after each removal (the first, the last, one
/// between, the only one) and insertion: forward and back from the last
/// child the same children; the first has no previous sibling (its link,
/// round the ring, is the last child), the last no next, a removed node
/// neither.
#[test]
fn children_link_both_ways_after_every_removal() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let doc = cx.create_document(true);
        let parent = cx.create_element(doc, QualName::html("div"));
        let [a, b, c, d] =
            ["a", "b", "c", "d"].map(|name| cx.create_element(doc, QualName::local(name)));
        let check = |want: &[&str]| {
            assert_eq!(children(cx, parent), want);
            assert_eq!(
                children_backwards(cx, parent),
                want,
                "back from the last child"
            );
            if let Some(first) = cx.first_child(parent) {
                assert!(cx.previous_sibling(first).is_none());
            }
            if let Some(last) = cx.last_child(parent) {
                assert!(cx.next_sibling(last).is_none());
            }
        };
        check(&[]);
        cx.append_child(parent, b).unwrap();
        check(&["b"]);
        cx.pre_insert(parent, a, Some(b)).unwrap();
        cx.append_child(parent, d).unwrap();
        cx.pre_insert(parent, c, Some(d)).unwrap();
        check(&["a", "b", "c", "d"]);
        cx.remove(a);
        check(&["b", "c", "d"]);
        cx.remove(d);
        check(&["b", "c"]);
        cx.append_child(parent, d).unwrap();
        cx.remove(c);
        check(&["b", "d"]);
        cx.remove(b);
        check(&["d"]);
        cx.remove(d);
        check(&[]);
        for node in [a, b, c, d] {
            assert!(cx.previous_sibling(node).is_none() && cx.next_sibling(node).is_none());
        }
    });
}
