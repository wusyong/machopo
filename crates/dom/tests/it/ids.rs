//! A document's elements by id (`Cx::element_by_id`, `getElementById`):
//! the first in tree order, kept as elements with ids come, go and
//! change them in the document's tree; another root's found by a walk.

use dom::{Cx, Dom, NodeRef, QualName};

fn element<'gc>(cx: &Cx<'_, 'gc>, document: NodeRef<'gc>, id: Option<&str>) -> NodeRef<'gc> {
    let element = cx.create_element(document, QualName::html("div"));
    if let Some(id) = id {
        cx.set_attribute(element, "id", id);
    }
    element
}

fn same<'gc>(a: Option<NodeRef<'gc>>, b: NodeRef<'gc>) -> bool {
    a.is_some_and(|a| elise::Gc::ptr_eq(a, b))
}

#[test]
fn the_first_in_tree_order_is_found() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let root = element(cx, document, None);
        cx.append_child(document, root).unwrap();
        let a = element(cx, document, Some("x"));
        cx.append_child(root, a).unwrap();
        assert!(same(cx.element_by_id(document, "x"), a));
        assert!(cx.element_by_id(document, "y").is_none());
        assert!(cx.element_by_id(document, "").is_none());
        // Another before it: the first is found again by a walk.
        let b = element(cx, document, Some("x"));
        cx.insert(root, b, Some(a));
        assert!(same(cx.element_by_id(document, "x"), b));
        cx.remove(b);
        assert!(same(cx.element_by_id(document, "x"), a));
        // Another after it: still the first.
        let after = element(cx, document, Some("x"));
        cx.append_child(root, after).unwrap();
        assert!(same(cx.element_by_id(document, "x"), a));
        cx.remove(after);
        // Its id changed, it isn't found by the old one.
        cx.set_attribute(a, "id", "z");
        assert!(cx.element_by_id(document, "x").is_none());
        assert!(same(cx.element_by_id(document, "z"), a));
        cx.remove_attribute(a, "id");
        assert!(cx.element_by_id(document, "z").is_none());
        // A subtree with ids comes and goes.
        let c = element(cx, document, None);
        let d = element(cx, document, Some("d"));
        cx.append_child(c, d).unwrap();
        assert!(cx.element_by_id(document, "d").is_none(), "not in the tree");
        cx.append_child(root, c).unwrap();
        assert!(same(cx.element_by_id(document, "d"), d));
        cx.remove(c);
        assert!(cx.element_by_id(document, "d").is_none());
        assert_eq!(cx.id_table_len(document), 0, "nothing kept once gone");
    });
}

/// An id set again to its value leaves the table as it is (Blink compares
/// the old and new ids before `UpdateId`): the first of two with it is
/// still the one found, with no walk; a new value moves it.
#[test]
fn an_id_set_again_to_its_value_changes_no_table() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let root = element(cx, document, None);
        cx.append_child(document, root).unwrap();
        let a = element(cx, document, Some("x"));
        let b = element(cx, document, Some("x"));
        cx.append_child(root, a).unwrap();
        cx.append_child(root, b).unwrap();
        assert!(same(cx.element_by_id(document, "x"), a));
        let changes = dom::id_table_changes_for_testing().expect("a debug build");
        let walks = dom::id_walks();
        cx.set_attribute(a, "id", "x");
        cx.set_attribute(b, "id", "x");
        assert_eq!(dom::id_table_changes_for_testing().unwrap(), changes);
        assert!(same(cx.element_by_id(document, "x"), a));
        assert_eq!(dom::id_walks(), walks, "the first still known");
        // Another value: out under the old one, in under the new.
        cx.set_attribute(a, "id", "y");
        assert_eq!(dom::id_table_changes_for_testing().unwrap() - changes, 2);
        assert!(same(cx.element_by_id(document, "x"), b));
        assert!(same(cx.element_by_id(document, "y"), a));
    });
}

#[test]
fn another_root_is_walked() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let fragment = cx.create_document_fragment(document);
        let a = element(cx, document, Some("a"));
        let b = element(cx, document, Some("a"));
        cx.append_child(fragment, a).unwrap();
        cx.append_child(fragment, b).unwrap();
        assert!(same(cx.element_by_id(fragment, "a"), a));
        assert_eq!(
            cx.id_table_len(document),
            0,
            "a fragment isn't the document's tree"
        );
    });
}

/// The table finds an id's element with no walk; with several, one walk
/// finds the first, which it keeps.
#[test]
fn the_table_saves_walks() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let root = element(cx, document, None);
        cx.append_child(document, root).unwrap();
        let a = element(cx, document, Some("x"));
        cx.append_child(root, a).unwrap();
        let walks = dom::id_walks();
        assert!(same(cx.element_by_id(document, "x"), a));
        assert!(cx.element_by_id(document, "nothing").is_none());
        assert_eq!(dom::id_walks(), walks, "one with the id: no walk");
        let b = element(cx, document, Some("x"));
        cx.append_child(root, b).unwrap();
        assert!(same(cx.element_by_id(document, "x"), a));
        assert!(same(cx.element_by_id(document, "x"), a));
        assert_eq!(dom::id_walks(), walks + 1, "two: one walk, the first kept");
    });
}

fn named<'gc>(cx: &Cx<'_, 'gc>, document: NodeRef<'gc>, tag: &str, name: &str) -> NodeRef<'gc> {
    let element = cx.create_element(document, QualName::html(tag));
    cx.set_attribute(element, "name", name);
    element
}

fn one<'gc>(found: dom::WindowNamed<'gc>, element: NodeRef<'gc>) -> bool {
    matches!(found, dom::WindowNamed::One(one) if elise::Gc::ptr_eq(one, element))
}

/// The window's named objects: by id, by a named element's `name`, an
/// iframe's by its `name` once it has a document; several or none; kept
/// as they come, go and change, from the tables with no walk while one
/// has the name.
#[test]
fn the_windows_names_are_kept() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let root = element(cx, document, None);
        cx.append_child(document, root).unwrap();
        // A `div`'s `name` isn't one: no table.
        let div = named(cx, document, "div", "x");
        cx.append_child(root, div).unwrap();
        assert!(matches!(
            cx.window_named(document, "x"),
            dom::WindowNamed::None
        ));
        assert_eq!(cx.name_table_len(document), 0);
        let img = named(cx, document, "img", "x");
        cx.append_child(root, img).unwrap();
        let walks = dom::id_walks();
        assert!(one(cx.window_named(document, "x"), img));
        assert_eq!(dom::id_walks(), walks, "one with the name: no walk");
        // Its id too: still one.
        cx.set_attribute(img, "id", "x");
        assert!(one(cx.window_named(document, "x"), img));
        // Another's id: several, down to one as it goes.
        let other = element(cx, document, Some("x"));
        cx.append_child(root, other).unwrap();
        assert!(matches!(
            cx.window_named(document, "x"),
            dom::WindowNamed::Several
        ));
        cx.remove(other);
        assert!(one(cx.window_named(document, "x"), img));
        // Its name changes, then it goes.
        cx.remove_attribute(img, "id");
        cx.set_attribute(img, "name", "y");
        assert!(matches!(
            cx.window_named(document, "x"),
            dom::WindowNamed::None
        ));
        assert!(one(cx.window_named(document, "y"), img));
        cx.remove(img);
        assert!(matches!(
            cx.window_named(document, "y"),
            dom::WindowNamed::None
        ));
        assert_eq!(cx.name_table_len(document), 0, "nothing kept once gone");
        // An iframe: once it has a document, before an element.
        let form = named(cx, document, "form", "f");
        cx.append_child(root, form).unwrap();
        let iframe = named(cx, document, "iframe", "f");
        cx.append_child(root, iframe).unwrap();
        assert!(one(cx.window_named(document, "f"), form));
        let child = cx.create_document(true);
        cx.set_content_document(iframe, Some(child));
        assert!(matches!(
            cx.window_named(document, "f"),
            dom::WindowNamed::Frame(frame) if elise::Gc::ptr_eq(frame, iframe)
        ));
        // In a shadow tree: none.
        let host = element(cx, document, None);
        cx.append_child(root, host).unwrap();
        let shadow = cx.attach_shadow(
            host,
            dom::ShadowRootInit {
                mode: dom::ShadowRootMode::Open,
                delegates_focus: false,
                slot_assignment: dom::SlotAssignmentMode::Named,
                clonable: false,
                serializable: false,
            },
        );
        let inside = named(cx, document, "embed", "s");
        cx.append_child(shadow, inside).unwrap();
        assert!(matches!(
            cx.window_named(document, "s"),
            dom::WindowNamed::None
        ));
        cx.set_attribute(inside, "name", "t");
        assert!(matches!(
            cx.window_named(document, "t"),
            dom::WindowNamed::None
        ));
    });
}
