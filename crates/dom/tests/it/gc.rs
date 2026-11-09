//! The DOM on its GC: a rooted tree survives scavenges and full GCs with
//! its links, attributes and text; dropping its root frees it.

use dom::{Dom, QualName};
use elise::HeapOptions;

use crate::children;

#[test]
fn a_rooted_tree_survives_gcs_and_goes_with_its_root() {
    let mut dom = Dom::with_options(HeapOptions {
        verify: true,
        ..HeapOptions::default()
    });
    let root = dom.mutate(|cx| {
        let doc = cx.create_document(true);
        let html = cx.create_element(doc, QualName::html("html"));
        cx.append_child(doc, html).unwrap();
        for i in 0..100 {
            let div = cx.create_element(doc, QualName::html("div"));
            cx.set_attribute(div, "id", &format!("d{i}"));
            cx.append_child(div, cx.create_text(doc, &format!("t{i}")))
                .unwrap();
            cx.append_child(html, div).unwrap();
        }
        // Garbage: never in the tree.
        for _ in 0..100 {
            let lost = cx.create_element(doc, QualName::html("p"));
            cx.set_attribute(lost, "class", "lost");
        }
        cx.root(doc)
    });
    let check = |dom: &mut Dom| {
        dom.mutate(|cx| {
            let doc = root.get(cx.gc);
            let html = cx.first_child(doc).unwrap();
            assert_eq!(children(cx, doc), ["HTML"]);
            let mut child = cx.first_child(html);
            let mut i = 0;
            while let Some(div) = child {
                assert!(i < 100, "the siblings' links loop");
                assert_eq!(cx.get_attribute(div, "id").unwrap(), &*format!("d{i}"));
                assert_eq!(cx.text_content(div).unwrap(), &*format!("t{i}"));
                assert!(elise::Gc::ptr_eq(cx.document_of(div), doc));
                child = cx.next_sibling(div);
                i += 1;
            }
            assert_eq!(i, 100);
        });
    };
    // Garbage until the young tree has been through scavenges.
    while dom.stats().scavenges < 2 {
        dom.mutate(|cx| {
            let doc = root.get(cx.gc);
            for _ in 0..2000 {
                cx.create_element(doc, QualName::html("span"));
            }
        });
    }
    check(&mut dom);
    dom.collect();
    check(&mut dom);
    let used = dom.stats().used_bytes;
    drop(root);
    dom.collect();
    assert!(
        dom.stats().used_bytes < used / 4,
        "the tree went with its root"
    );
}

/// Where an engine drives the heap a node is old from its allocation (its
/// wrapper's edge never moves), and so is the data hung on it: its text,
/// attributes and their list, element and rare data, classes, its
/// children's list, an input's type, a form association, an `Attr`'s and
/// a document's data. None of it is copied by scavenges. (A `classList` is
/// old for its wrapper.)
#[test]
fn a_nodes_data_is_allocated_beside_it() {
    let mut dom = Dom::with_options(HeapOptions {
        driven: true,
        verify: true,
        ..HeapOptions::default()
    });
    let young = |dom: &Dom| dom.stats().young_bytes;
    let before = young(&dom);
    dom.mutate(|cx| {
        let doc = cx.create_document(true);
        let text = cx.create_text(doc, "text");
        let div = cx.create_element(doc, QualName::html("div"));
        cx.append_child(div, text).unwrap();
        for i in 0..8 {
            // Past the list's first capacity: it grows.
            cx.set_attribute(div, &format!("data-{i}"), "value");
        }
        cx.set_attribute(div, "class", "a b");
        assert_eq!(cx.class_names(div).len(), 2);
        cx.class_list(div);
        cx.child_nodes(div);
        let form = cx.create_element(doc, QualName::html("form"));
        let input = cx.create_element(doc, QualName::html("input"));
        cx.append_child(form, input).unwrap();
        assert!(cx.form_owner(input).is_some());
        // The parser's fast path: the list made its size, an input its type.
        let local = |name| dom::names::intern(name);
        let parsed = cx.create_html_element_with_attributes(
            doc,
            local("input"),
            &[(local("type"), "checkbox"), (local("name"), "n")],
        );
        assert!(cx.attribute_at(parsed, 1).is_some());
        assert!(cx.attr_node(div, "data-0").is_some());
        cx.create_doctype(doc, "html", "", "");
        cx.create_element(doc, QualName::html("template"));
    });
    assert_eq!(young(&dom), before, "nothing went to the nursery");
}

/// The heap a DOM lends (`Dom::heap_mut`) is its own: inside the frames an
/// engine enters in it, its operations' safepoints run no GC.
#[test]
fn the_heap_a_dom_lends_is_its_own() {
    let mut dom = Dom::with_options(HeapOptions {
        nursery_bytes: 256 << 10,
        verify: true,
        ..HeapOptions::default()
    });
    let doc = dom.mutate(|cx| cx.root(cx.create_document(true)));
    dom.heap_mut().enter_frames();
    for _ in 0..20 {
        dom.mutate(|cx| {
            let doc = doc.get(cx.gc);
            for _ in 0..2000 {
                cx.create_element(doc, QualName::html("span"));
            }
        });
    }
    assert_eq!(dom.stats().scavenges, 0);
    assert!(dom.heap_mut().should_collect());
    // Dropped inside them, the heap leaks.
    assert!(dom.heap_mut().in_frames());
}
