//! A node's subtree's bloom filter (`Cx::subtree_bloom`): the bits of an
//! attribute's name and of the classes set on an element are its own and
//! its ancestors'; an inserted subtree's are its new ancestors'; a clone
//! has its original's.

use dom::{Cx, Dom, NodeRef, QualName, attribute_bloom, class_bloom, names};

fn has<'gc>(cx: &Cx<'_, 'gc>, node: NodeRef<'gc>, bits: u32) -> bool {
    cx.subtree_bloom(node) & bits == bits
}

#[test]
fn an_attribute_and_classes_set_are_the_ancestors_bits() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let [root, a, b, c] = ["div", "p", "span", "b"]
            .map(|local| cx.create_element(document, QualName::html(local)));
        cx.append_child(root, a).unwrap();
        cx.append_child(a, b).unwrap();
        cx.append_child(b, c).unwrap();
        let data = attribute_bloom(names::intern("data-k"));
        cx.set_attribute(c, "data-k", "1");
        for node in [root, a, b, c] {
            assert!(has(cx, node, data));
        }
        let classes = class_bloom("x") | class_bloom("why");
        cx.set_attribute(b, "class", " x\twhy ");
        for node in [root, a, b] {
            assert!(has(cx, node, classes | attribute_bloom(names::CLASS)));
        }
        // A bit an ancestor has already stops nothing below it: another
        // attribute's still goes up.
        let other = attribute_bloom(names::intern("data-other"));
        cx.set_attribute(c, "data-other", "");
        assert!(has(cx, root, other | data));
    });
}

#[test]
fn an_inserted_subtree_brings_its_bits() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let [one, two, item, leaf] = ["div", "section", "li", "b"]
            .map(|local| cx.create_element(document, QualName::html(local)));
        cx.append_child(item, leaf).unwrap();
        cx.set_attribute(leaf, "data-id", "7");
        cx.set_attribute(item, "class", "item");
        let bits = attribute_bloom(names::intern("data-id")) | class_bloom("item");
        assert!(!has(cx, one, bits));
        cx.append_child(one, item).unwrap();
        assert!(has(cx, one, bits));
        // Moved: the new parent's too.
        cx.append_child(two, item).unwrap();
        assert!(has(cx, two, bits));
    });
}

#[test]
fn a_clone_has_its_originals_bits() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let [root, child] =
            ["div", "a"].map(|local| cx.create_element(document, QualName::html(local)));
        cx.append_child(root, child).unwrap();
        cx.set_attribute(child, "href", "#");
        cx.set_attribute(root, "class", "card");
        let copy = cx.clone_node(root, None, true);
        assert!(has(
            cx,
            copy,
            attribute_bloom(names::intern("href")) | class_bloom("card")
        ));
    });
}
