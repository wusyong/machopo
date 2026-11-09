//! A live collection of elements by qualified name
//! (`getElementsByTagName`): what it matches in HTML and XML documents, in
//! tree order, live, and `namedItem`.

use dom::{Dom, QualName};
use elise::HeapOptions;

fn same<'gc>(a: &[dom::NodeRef<'gc>], b: &[dom::NodeRef<'gc>]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| elise::Gc::ptr_eq(*a, *b))
}

/// `<root><a/><b><a/></b><svg:a/><A/></root>`: the names an HTML
/// document's collection lowercases, and the elements in tree order.
#[test]
fn a_collection_matches_qualified_names_in_tree_order() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let root = cx.create_element(document, QualName::html("div"));
        cx.append_child(document, root).unwrap();
        let first = cx.create_element(document, QualName::html("a"));
        let b = cx.create_element(document, QualName::html("b"));
        let nested = cx.create_element(document, QualName::html("a"));
        let svg = cx.create_element(
            document,
            QualName {
                namespace: Some("http://www.w3.org/2000/svg"),
                prefix: None,
                local: "a",
            },
        );
        let svg_upper = cx.create_element(
            document,
            QualName {
                namespace: Some("http://www.w3.org/2000/svg"),
                prefix: None,
                local: "A",
            },
        );
        let prefixed = cx.create_element(
            document,
            QualName {
                namespace: Some("urn:x"),
                prefix: Some("p"),
                local: "a",
            },
        );
        for child in [first, b, svg, svg_upper, prefixed] {
            cx.append_child(root, child).unwrap();
        }
        cx.append_child(b, nested).unwrap();
        let ids = |collection| {
            let length = cx.collection_length(collection);
            (0..length)
                .map(|index| cx.collection_item(collection, index).unwrap())
                .collect::<Vec<_>>()
        };
        // HTML elements by the lowercased name, others as given; in tree
        // order, the root itself not.
        let a = cx.elements_by_tag_name(root, "A");
        assert!(same(&ids(a), &[first, nested, svg_upper]));
        let lower = cx.elements_by_tag_name(root, "a");
        assert!(same(&ids(lower), &[first, nested, svg]));
        assert!(same(
            &ids(cx.elements_by_tag_name(root, "p:a")),
            &[prefixed]
        ));
        assert!(same(&ids(cx.elements_by_tag_name(b, "a")), &[nested]));
        assert_eq!(cx.collection_length(cx.elements_by_tag_name(root, "*")), 6);
        assert_eq!(
            cx.collection_length(cx.elements_by_tag_name(document, "*")),
            7
        );
        assert_eq!(
            cx.collection_length(cx.elements_by_tag_name(root, "none")),
            0
        );
        assert!(cx.collection_item(lower, 3).is_none());
        // Live: an element added, one taken out, a name made after.
        let later = cx.create_element(document, QualName::html("a"));
        cx.append_child(root, later).unwrap();
        cx.pre_remove(root, first).unwrap();
        assert!(same(&ids(lower), &[nested, svg, later]));
        let new_name = cx.elements_by_tag_name(root, "fresh");
        let fresh = cx.create_element(document, QualName::html("fresh"));
        cx.append_child(b, fresh).unwrap();
        assert!(same(&ids(new_name), &[fresh]));
    });
}

/// In an XML document no name is lowercased.
#[test]
fn an_xml_documents_collection_compares_as_given() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(false);
        let root = cx.create_element(document, QualName::html("root"));
        cx.append_child(document, root).unwrap();
        let upper = cx.create_element(document, QualName::html("A"));
        cx.append_child(root, upper).unwrap();
        assert_eq!(cx.collection_length(cx.elements_by_tag_name(root, "a")), 0);
        assert_eq!(cx.collection_length(cx.elements_by_tag_name(root, "A")), 1);
    });
}

/// `namedItem`: the first element whose `id` is the key, or an HTML
/// element whose `name` is; none for the empty key.
#[test]
fn a_collections_named_item() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let root = cx.create_element(document, QualName::html("div"));
        let by_name = cx.create_element(document, QualName::html("meta"));
        cx.set_attribute(by_name, "name", "k");
        let svg_name = cx.create_element(
            document,
            QualName {
                namespace: Some("http://www.w3.org/2000/svg"),
                prefix: None,
                local: "g",
            },
        );
        cx.set_attribute(svg_name, "name", "s");
        let by_id = cx.create_element(document, QualName::html("p"));
        cx.set_attribute(by_id, "id", "k");
        // An empty `id` (and `name`) isn't the empty key's.
        let empty = cx.create_element(document, QualName::html("p"));
        cx.set_attribute(empty, "id", "");
        cx.set_attribute(empty, "name", "");
        for child in [empty, by_name, svg_name, by_id] {
            cx.append_child(root, child).unwrap();
        }
        let all = cx.elements_by_tag_name(root, "*");
        assert!(elise::Gc::ptr_eq(
            cx.collection_named_item(all, "k").unwrap(),
            by_name
        ));
        assert!(
            cx.collection_named_item(all, "s").is_none(),
            "only an HTML element by name"
        );
        assert!(cx.collection_named_item(all, "").is_none());
        cx.remove_attribute(by_name, "name");
        assert!(elise::Gc::ptr_eq(
            cx.collection_named_item(all, "k").unwrap(),
            by_id
        ));
    });
}

/// A collection keeps its root alive through GCs.
#[test]
fn a_collection_keeps_its_root() {
    let mut dom = Dom::with_options(HeapOptions {
        verify: true,
        ..HeapOptions::default()
    });
    let collection = dom.mutate(|cx| {
        let document = cx.create_document(true);
        let root = cx.create_element(document, QualName::html("div"));
        let child = cx.create_element(document, QualName::html("span"));
        cx.append_child(root, child).unwrap();
        elise::Persistent::new(cx.gc, cx.elements_by_tag_name(root, "span"))
    });
    dom.collect();
    dom.mutate(|cx| assert_eq!(cx.collection_length(collection.get(cx.gc)), 1));
}

/// Going through a collection in order (jQuery's `cleanData` reads
/// `getElementsByTagName("*")[i]` until it runs out) looks at each node
/// once, not at those before it again; its length is known then, counted
/// with no walk, and an index past it found with none.
#[test]
fn a_collection_walked_in_order_takes_a_step_an_element() {
    let steps = || dom::collection_steps_for_testing();
    if steps().is_none() {
        // Counted in debug builds only.
        return;
    }
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let root = cx.create_element(document, QualName::html("div"));
        // 100 `li`s, each with a `b`, then 50 `span`s none matches.
        for _ in 0..100 {
            let item = cx.create_element(document, QualName::html("li"));
            cx.append_child(item, cx.create_element(document, QualName::html("b")))
                .unwrap();
            cx.append_child(root, item).unwrap();
        }
        for _ in 0..50 {
            cx.append_child(root, cx.create_element(document, QualName::html("span")))
                .unwrap();
        }
        let items = cx.elements_by_tag_name(root, "li");
        let before = steps().unwrap();
        let mut index = 0;
        while cx.collection_item(items, index).is_some() {
            index += 1;
        }
        assert_eq!(index, 100);
        assert_eq!(steps().unwrap() - before, 250, "each node once");
        let before = steps().unwrap();
        assert_eq!(cx.collection_length(items), 100);
        assert!(cx.collection_item(items, 120).is_none());
        assert_eq!(steps().unwrap() - before, 0);
        // The length counted from the last one found, after a change.
        cx.append_child(root, cx.create_element(document, QualName::html("li")))
            .unwrap();
        assert!(cx.collection_item(items, 99).is_some());
        let before = steps().unwrap();
        assert_eq!(cx.collection_length(items), 101);
        assert_eq!(
            steps().unwrap() - before,
            52,
            "the last `b`, the spans, the new one"
        );
    });
}

/// Changing the attributes of a collection's elements while going
/// through it (`items[i].className = …`) keeps its place when its kind
/// doesn't match by them: a step an element still, its length still
/// known. Going back through one is a step an element too: from its last
/// once its length is counted, then from the one before; a children's
/// collection back by its children alone.
#[test]
fn a_collection_keeps_its_place_through_other_changes_and_back() {
    let steps = || dom::collection_steps_for_testing();
    if steps().is_none() {
        // Counted in debug builds only.
        return;
    }
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let root = cx.create_element(document, QualName::html("div"));
        // 100 `li`s, each with a `b`, then 50 `span`s none matches.
        for _ in 0..100 {
            let item = cx.create_element(document, QualName::html("li"));
            cx.append_child(item, cx.create_element(document, QualName::html("b")))
                .unwrap();
            cx.append_child(root, item).unwrap();
        }
        for _ in 0..50 {
            cx.append_child(root, cx.create_element(document, QualName::html("span")))
                .unwrap();
        }
        let items = cx.elements_by_tag_name(root, "li");
        let before = steps().unwrap();
        let mut index = 0;
        while let Some(item) = cx.collection_item(items, index) {
            cx.set_attribute(item, "class", "done");
            cx.set_attribute(item, "id", &format!("i{index}"));
            cx.set_attribute(item, "name", "n");
            cx.set_attribute(item, "href", "#");
            cx.set_attribute(item, "title", "t");
            index += 1;
        }
        assert_eq!(index, 100);
        assert_eq!(steps().unwrap() - before, 250, "each node once");
        let before = steps().unwrap();
        assert_eq!(cx.collection_length(items), 100);
        assert_eq!(steps().unwrap() - before, 0, "the length known");
        // A collection by class keeps its length through another
        // attribute's change.
        let done = cx.elements_by_class_name(root, "done");
        assert_eq!(cx.collection_length(done), 100);
        let first = cx.collection_item(items, 0).unwrap();
        cx.set_attribute(first, "title", "u");
        let before = steps().unwrap();
        assert_eq!(cx.collection_length(done), 100);
        assert_eq!(steps().unwrap() - before, 0, "a title isn't a class");

        // Back: another collection of the same elements, its length
        // counted, then from its last.
        let back = cx.elements_by_tag_name(root, "LI");
        assert_eq!(cx.collection_length(back), 100);
        let forward: Vec<_> = (0..100)
            .map(|index| cx.collection_item(items, index).unwrap())
            .collect();
        let before = steps().unwrap();
        for index in (0..100).rev() {
            let item = cx.collection_item(back, index).unwrap();
            assert!(elise::Gc::ptr_eq(item, forward[index as usize]), "{index}");
        }
        // The spans and the last `b` to the last, then a `b` and an `li`
        // each; the first two from the root.
        assert_eq!(steps().unwrap() - before, 250, "each node once");

        // A children's collection goes back by the root's children.
        let children = cx.children_collection(root);
        assert_eq!(cx.collection_length(children), 150);
        let before = steps().unwrap();
        for index in (0..150).rev() {
            let child = cx.collection_item(children, index).unwrap();
            let expected = if index < 100 {
                forward[index as usize]
            } else {
                cx.collection_item(cx.elements_by_tag_name(root, "span"), index - 100)
                    .unwrap()
            };
            assert!(elise::Gc::ptr_eq(child, expected), "{index}");
        }
        assert!(steps().unwrap() - before >= 150);
    });
}

/// The collections matching by an attribute follow its changes, each
/// kind its own: by class a `class`, a document's `links` an `href`, its `anchors` and
/// `getElementsByName`'s a `name`, the window's named objects and
/// `document.all`'s an `id` or (an `img`'s) a `name`; another attribute's
/// change keeps their caches.
#[test]
fn a_collection_by_an_attribute_follows_its_changes() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let html = cx.create_element(document, QualName::html("html"));
        cx.append_child(document, html).unwrap();
        let a = cx.create_element(document, QualName::html("a"));
        let div = cx.create_element(document, QualName::html("div"));
        let img = cx.create_element(document, QualName::html("img"));
        for child in [a, div, img] {
            cx.append_child(html, child).unwrap();
        }
        let links = cx.document_links(document, false);
        let anchors = cx.document_links(document, true);
        let by_name = cx.elements_by_name_list(document, "n");
        let window_named = cx.window_named_collection(document, "m");
        let all_named = cx.all_named_collection(document, "m");
        let by_class = cx.elements_by_class_name(document, "c");
        let lengths = || {
            [
                cx.collection_length(links),
                cx.collection_length(anchors),
                cx.node_list_length(by_name),
                cx.collection_length(window_named),
                cx.collection_length(all_named),
                cx.collection_length(by_class),
            ]
        };
        assert_eq!(lengths(), [0, 0, 0, 0, 0, 0]);
        cx.set_attribute(a, "title", "t");
        assert_eq!(lengths(), [0, 0, 0, 0, 0, 0]);
        cx.set_attribute(a, "class", "c");
        assert_eq!(lengths(), [0, 0, 0, 0, 0, 1]);
        cx.set_attribute(a, "href", "#");
        assert_eq!(lengths(), [1, 0, 0, 0, 0, 1]);
        cx.set_attribute(a, "name", "n");
        assert_eq!(lengths(), [1, 1, 1, 0, 0, 1]);
        cx.set_attribute(div, "id", "m");
        assert_eq!(lengths(), [1, 1, 1, 1, 1, 1]);
        cx.set_attribute(img, "name", "m");
        assert_eq!(lengths(), [1, 1, 1, 2, 2, 1]);
        cx.remove_attribute(a, "href");
        cx.remove_attribute(div, "id");
        cx.remove_attribute(a, "class");
        assert_eq!(lengths(), [0, 1, 1, 1, 1, 0]);
    });
}

/// A collection's cache follows every change: an element inserted before
/// the one last found, one removed, a class changed, the document's mode
/// (a class matched without case in quirks mode); and an earlier index
/// read after a later one.
#[test]
fn a_collections_cache_follows_every_change() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let root = cx.create_element(document, QualName::html("div"));
        cx.append_child(document, root).unwrap();
        let p: Vec<_> = (0..3)
            .map(|_| {
                let p = cx.create_element(document, QualName::html("p"));
                cx.set_attribute(p, "class", "x");
                cx.append_child(root, p).unwrap();
                p
            })
            .collect();
        let ps = cx.elements_by_tag_name(root, "p");
        let xs = cx.elements_by_class_name(root, "x");
        let children = cx.children_collection(root);
        let item = |collection, index| cx.collection_item(collection, index);
        assert!(elise::Gc::ptr_eq(item(ps, 2).unwrap(), p[2]));
        assert!(elise::Gc::ptr_eq(item(ps, 0).unwrap(), p[0]), "back");
        assert!(elise::Gc::ptr_eq(item(ps, 1).unwrap(), p[1]));
        assert_eq!(cx.collection_length(ps), 3);
        assert!(item(ps, 3).is_none());

        let inserted = cx.create_element(document, QualName::html("p"));
        cx.insert(root, inserted, Some(p[1]));
        assert!(elise::Gc::ptr_eq(item(ps, 1).unwrap(), inserted));
        assert!(elise::Gc::ptr_eq(item(ps, 3).unwrap(), p[2]));
        assert_eq!(cx.collection_length(ps), 4);

        cx.remove(p[0]);
        assert!(elise::Gc::ptr_eq(item(ps, 0).unwrap(), inserted));
        assert!(elise::Gc::ptr_eq(item(children, 0).unwrap(), inserted));
        assert_eq!(cx.collection_length(ps), 3);
        assert_eq!(cx.collection_length(children), 3);

        assert!(elise::Gc::ptr_eq(item(xs, 0).unwrap(), p[1]));
        assert!(elise::Gc::ptr_eq(item(xs, 1).unwrap(), p[2]));
        cx.set_attribute(p[1], "class", "y");
        assert!(elise::Gc::ptr_eq(item(xs, 0).unwrap(), p[2]));
        assert_eq!(cx.collection_length(xs), 1);

        cx.set_attribute(inserted, "class", "X");
        assert_eq!(cx.collection_length(xs), 1);
        cx.set_quirks_mode(document, dom::QuirksMode::Quirks);
        assert_eq!(cx.collection_length(xs), 2);
        assert!(elise::Gc::ptr_eq(item(xs, 0).unwrap(), inserted));
    });
}

/// A root's collection by name is the same object while something holds
/// it (Blink's `NodeListsNodeData`), for the same kind and name only; the
/// root keeps none of them alive: one no one holds is collected, and the
/// next ask makes another.
#[test]
fn a_collection_by_name_is_the_same_while_held() {
    let mut dom = Dom::with_options(HeapOptions {
        verify: true,
        ..HeapOptions::default()
    });
    let (root, held, weak) = dom.mutate(|cx| {
        let document = cx.create_document(true);
        let root = cx.create_element(document, QualName::html("div"));
        cx.append_child(document, root).unwrap();
        let spans = cx.elements_by_tag_name(root, "span");
        assert!(elise::Gc::ptr_eq(
            spans,
            cx.elements_by_tag_name(root, "span")
        ));
        // Another name, kind or root: another collection.
        assert!(!elise::Gc::ptr_eq(
            spans,
            cx.elements_by_tag_name(root, "b")
        ));
        assert!(!elise::Gc::ptr_eq(
            spans,
            cx.elements_by_tag_name(root, "SPAN")
        ));
        let ns = cx.elements_by_tag_name_ns(root, Some("http://www.w3.org/1999/xhtml"), "span");
        assert!(!elise::Gc::ptr_eq(spans, ns));
        assert!(elise::Gc::ptr_eq(
            ns,
            cx.elements_by_tag_name_ns(root, Some("http://www.w3.org/1999/xhtml"), "span")
        ));
        assert!(!elise::Gc::ptr_eq(
            spans,
            cx.elements_by_tag_name(document, "span")
        ));
        // The same numbers, another kind (no prefix, no namespace).
        assert!(!elise::Gc::ptr_eq(
            spans,
            cx.elements_by_tag_name_ns(root, None, "span")
        ));
        // Live as ever.
        cx.append_child(root, cx.create_element(document, QualName::html("span")))
            .unwrap();
        assert_eq!(cx.collection_length(spans), 1);
        // A held one stays found as more names come than the slots.
        for name in ["i", "u", "s", "q", "p", "a", "em", "dd", "dt", "li"] {
            cx.elements_by_tag_name(root, name);
        }
        let held = elise::Persistent::new(cx.gc, cx.elements_by_tag_name(root, "ul"));
        let weak = elise::WeakPersistent::new(cx.gc, cx.elements_by_tag_name(root, "ol"));
        (cx.root(root), held, weak)
    });
    dom.collect();
    dom.mutate(|cx| {
        let root = root.get(cx.gc);
        assert!(weak.get(cx.gc).is_none(), "no one held it");
        assert!(elise::Gc::ptr_eq(
            held.get(cx.gc),
            cx.elements_by_tag_name(root, "ul")
        ));
        assert_eq!(
            cx.collection_length(cx.elements_by_tag_name(root, "span")),
            1
        );
    });
}
