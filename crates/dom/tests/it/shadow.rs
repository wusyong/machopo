//! Shadow trees: a shadow root and its host, the tree scope of the nodes
//! in it (their shadow root as their owner: moved as a subtree goes in or
//! out, their node document the shadow root's), connected through hosts,
//! ids out of the document's table, adoption into another document
//! through the shadow trees, and a clonable shadow root cloned with its
//! host.

use dom::{Dom, QualName, ShadowRootInit, ShadowRootMode, SlotAssignmentMode};

fn open() -> ShadowRootInit {
    ShadowRootInit {
        mode: ShadowRootMode::Open,
        delegates_focus: false,
        slot_assignment: SlotAssignmentMode::Named,
        clonable: false,
        serializable: false,
    }
}

#[test]
fn a_shadow_root_is_its_hosts() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let host = cx.create_element(document, QualName::html("div"));
        assert!(cx.shadow_root(host).is_none());
        let init = ShadowRootInit {
            mode: ShadowRootMode::Closed,
            delegates_focus: true,
            slot_assignment: SlotAssignmentMode::Manual,
            clonable: true,
            serializable: true,
        };
        let root = cx.attach_shadow(host, init);
        assert!(
            cx.shadow_root(host)
                .is_some_and(|of| elise::Gc::ptr_eq(of, root))
        );
        assert!(
            cx.shadow_host(root)
                .is_some_and(|of| elise::Gc::ptr_eq(of, host))
        );
        assert_eq!(cx.shadow_root_init(root), Some(init));
        assert!(cx.is_shadow_root(root) && !cx.is_shadow_root(host));
        assert!(cx.shadow_root_init(host).is_none());
        assert!(elise::Gc::ptr_eq(cx.document_of(root), document));
        assert!(cx.parent(root).is_none());
        assert!(
            cx.parent_or_shadow_host(root)
                .is_some_and(|of| elise::Gc::ptr_eq(of, host))
        );
        // A shadow root's listeners are its own.
        assert!(cx.listeners(dom::TargetRef::Node(root)).is_none());
    });
}

#[test]
fn a_shadow_tree_is_its_nodes_tree_scope() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let host = cx.create_element(document, QualName::html("div"));
        cx.append_child(document, host).unwrap();
        let root = cx.attach_shadow(host, open());
        assert!(cx.is_connected(root));
        let a = cx.create_element(document, QualName::html("p"));
        let b = cx.create_text(document, "b");
        cx.append_child(a, b).unwrap();
        cx.append_child(root, a).unwrap();
        for node in [a, b] {
            assert!(
                cx.containing_shadow_root(node)
                    .is_some_and(|of| elise::Gc::ptr_eq(of, root))
            );
            assert!(elise::Gc::ptr_eq(cx.document_of(node), document));
            assert!(
                cx.owner_document(node)
                    .is_some_and(|of| elise::Gc::ptr_eq(of, document))
            );
            assert!(cx.is_connected(node));
            assert!(elise::Gc::ptr_eq(cx.root_node(node, false), root));
            assert!(elise::Gc::ptr_eq(cx.root_node(node, true), document));
        }
        // A shadow root is in no shadow tree itself.
        assert!(cx.containing_shadow_root(root).is_none());
        assert!(cx.containing_shadow_root(host).is_none());
        // Out of the shadow tree: its document's, its own root.
        cx.remove(a);
        for node in [a, b] {
            assert!(cx.containing_shadow_root(node).is_none());
            assert!(elise::Gc::ptr_eq(cx.document_of(node), document));
            assert!(!cx.is_connected(node));
        }
        assert!(elise::Gc::ptr_eq(cx.root_node(b, false), a));
        // Through a fragment into it, and its host out of the document.
        let fragment = cx.create_document_fragment(document);
        cx.append_child(fragment, a).unwrap();
        cx.append_child(root, fragment).unwrap();
        assert!(
            cx.containing_shadow_root(b)
                .is_some_and(|of| elise::Gc::ptr_eq(of, root))
        );
        cx.remove(host);
        assert!(!cx.is_connected(root) && !cx.is_connected(b));
        assert!(elise::Gc::ptr_eq(cx.root_node(b, true), host));
        cx.append_child(document, host).unwrap();
        assert!(cx.is_connected(root) && cx.is_connected(b));
    });
}

/// A node in a shadow tree is retargeted out of the trees another isn't
/// in; closed trees hide their nodes from the others.
#[test]
fn retargeting_goes_out_to_the_hosts() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let host = cx.create_element(document, QualName::html("div"));
        cx.append_child(document, host).unwrap();
        let root = cx.attach_shadow(
            host,
            ShadowRootInit {
                mode: ShadowRootMode::Closed,
                ..open()
            },
        );
        let inner = cx.create_element(document, QualName::html("span"));
        cx.append_child(root, inner).unwrap();
        let nested_root = cx.attach_shadow(inner, open());
        let deep = cx.create_element(document, QualName::html("b"));
        cx.append_child(nested_root, deep).unwrap();
        // In the host's own tree, outside its shadow tree.
        let outside = cx.create_element(document, QualName::html("i"));
        cx.append_child(host, outside).unwrap();
        let same = |a, b| elise::Gc::ptr_eq(a, b);
        assert!(same(cx.retarget(deep, Some(outside)), host));
        assert!(same(cx.retarget(deep, Some(inner)), inner));
        assert!(same(cx.retarget(deep, Some(deep)), deep));
        assert!(same(cx.retarget(deep, None), host));
        assert!(same(cx.retarget(outside, Some(deep)), outside));
        assert!(
            cx.is_in_shadow_tree_of(root, deep) && !cx.is_in_shadow_tree_of(nested_root, inner)
        );
        assert!(cx.is_closed_shadow_hidden(deep, outside));
        assert!(cx.is_closed_shadow_hidden(inner, host));
        assert!(!cx.is_closed_shadow_hidden(deep, inner));
        assert!(!cx.is_closed_shadow_hidden(outside, deep));
    });
}

#[test]
fn ids_in_a_shadow_tree_are_not_the_documents() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let host = cx.create_element(document, QualName::html("div"));
        cx.append_child(document, host).unwrap();
        let root = cx.attach_shadow(host, open());
        let before = cx.create_element(document, QualName::html("p"));
        cx.set_attribute(before, "id", "x");
        cx.append_child(root, before).unwrap();
        let after = cx.create_element(document, QualName::html("p"));
        cx.append_child(root, after).unwrap();
        cx.set_attribute(after, "id", "y");
        assert!(cx.element_by_id(document, "x").is_none());
        assert!(cx.element_by_id(document, "y").is_none());
        assert!(
            cx.element_by_id(root, "x")
                .is_some_and(|of| elise::Gc::ptr_eq(of, before))
        );
        assert!(
            cx.element_by_id(root, "y")
                .is_some_and(|of| elise::Gc::ptr_eq(of, after))
        );
        // Out of the shadow tree into the document: the document's.
        cx.remove(after);
        cx.append_child(host, after).unwrap();
        assert!(
            cx.element_by_id(document, "y")
                .is_some_and(|of| elise::Gc::ptr_eq(of, after))
        );
    });
}

/// Adopting a host takes its shadow tree to the document too; its nodes
/// keep their shadow root as their owner.
#[test]
fn a_host_adopted_takes_its_shadow_tree() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let first = cx.create_document(true);
        let second = cx.create_document(true);
        let host = cx.create_element(first, QualName::html("div"));
        let root = cx.attach_shadow(host, open());
        let inner = cx.create_element(first, QualName::html("span"));
        cx.append_child(root, inner).unwrap();
        let nested = cx.attach_shadow(inner, open());
        let deep = cx.create_text(first, "deep");
        cx.append_child(nested, deep).unwrap();
        cx.append_child(second, host).unwrap();
        for node in [host, root, inner, nested, deep] {
            assert!(elise::Gc::ptr_eq(cx.document_of(node), second));
        }
        assert!(
            cx.containing_shadow_root(inner)
                .is_some_and(|of| elise::Gc::ptr_eq(of, root))
        );
        assert!(
            cx.containing_shadow_root(deep)
                .is_some_and(|of| elise::Gc::ptr_eq(of, nested))
        );
        assert!(cx.is_connected(deep));
    });
}

/// DOM's clone: a clonable shadow root comes with its host, deep or not;
/// another doesn't.
#[test]
fn a_clonable_shadow_root_is_cloned_with_its_host() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let host = cx.create_element(document, QualName::html("div"));
        let init = ShadowRootInit {
            clonable: true,
            delegates_focus: true,
            ..open()
        };
        let root = cx.attach_shadow(host, init);
        let inner = cx.create_element(document, QualName::html("span"));
        cx.append_child(root, inner).unwrap();
        for deep in [false, true] {
            let copy = cx.clone_node(host, None, deep);
            let copied = cx
                .shadow_root(copy)
                .expect("a clonable shadow root, cloned");
            assert!(!elise::Gc::ptr_eq(copied, root));
            assert_eq!(cx.shadow_root_init(copied), Some(init));
            let child = cx.first_child(copied).expect("its children cloned");
            assert_eq!(cx.node_name(child).to_str(), "SPAN");
            assert!(
                cx.containing_shadow_root(child)
                    .is_some_and(|of| elise::Gc::ptr_eq(of, copied))
            );
        }
        let plain = cx.create_element(document, QualName::html("div"));
        cx.attach_shadow(plain, open());
        assert!(cx.shadow_root(cx.clone_node(plain, None, true)).is_none());
    });
}

fn same<'gc>(list: Vec<dom::NodeRef<'gc>>, want: &[dom::NodeRef<'gc>]) -> bool {
    list.len() == want.len()
        && list
            .iter()
            .zip(want)
            .all(|(x, y)| elise::Gc::ptr_eq(*x, *y))
}

/// The flat tree: a host's children are its shadow root's, a slot's its
/// assigned nodes (its fallback if none, found by name while the
/// assignment is stale); its parents, for the dirty bits, the other way.
#[test]
fn the_flat_tree_goes_through_hosts_and_slots() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let host = cx.create_element(document, QualName::html("div"));
        cx.append_child(document, host).unwrap();
        let a = cx.create_element(document, QualName::html("a"));
        let b = cx.create_element(document, QualName::html("b"));
        cx.set_attribute(a, "slot", "s");
        cx.append_child(host, a).unwrap();
        cx.append_child(host, b).unwrap();
        let root = cx.attach_shadow(host, open());
        let slot = cx.create_element(document, QualName::html("slot"));
        cx.set_attribute(slot, "name", "s");
        let fallback = cx.create_element(document, QualName::html("i"));
        cx.append_child(slot, fallback).unwrap();
        let empty = cx.create_element(document, QualName::html("slot"));
        cx.set_attribute(empty, "name", "z");
        let unused = cx.create_element(document, QualName::html("u"));
        cx.append_child(empty, unused).unwrap();
        cx.append_child(root, slot).unwrap();
        cx.append_child(root, empty).unwrap();

        // Stale, then worked out: the same.
        for _ in 0..2 {
            assert!(same(cx.flat_tree_children(host).collect(), &[slot, empty]));
            assert!(same(cx.flat_tree_children(slot).collect(), &[a]));
            assert!(same(cx.flat_tree_children(empty).collect(), &[unused]));
            cx.recalc_slot_assignments();
        }
        assert!(
            cx.flat_tree_parent(a)
                .is_some_and(|of| elise::Gc::ptr_eq(of, slot))
        );
        assert!(
            cx.flat_tree_parent(b).is_none(),
            "no slot: not in the flat tree"
        );
        assert!(
            cx.flat_tree_parent(slot)
                .is_some_and(|of| elise::Gc::ptr_eq(of, host))
        );
        assert!(
            cx.flat_tree_parent(root)
                .is_some_and(|of| elise::Gc::ptr_eq(of, host))
        );
        assert!(
            cx.flat_tree_parent(fallback)
                .is_some_and(|of| elise::Gc::ptr_eq(of, slot))
        );
        assert!(
            cx.flat_tree_parent(host)
                .is_some_and(|of| elise::Gc::ptr_eq(of, document))
        );
        // Stale (a slottable renamed): found by name, the fallback if none.
        cx.set_attribute(b, "slot", "s");
        assert!(same(cx.flat_tree_children(slot).collect(), &[a, b]));
        assert!(same(cx.flat_tree_children(empty).collect(), &[unused]));
        cx.recalc_slot_assignments();
        // A slot's assigned nodes gone: its fallback.
        cx.remove(b);
        cx.remove(a);
        cx.recalc_slot_assignments();
        assert!(same(cx.flat_tree_children(slot).collect(), &[fallback]));
        assert!(same(cx.flat_tree_children(b).collect(), &[]));
    });
}

/// The `:hover` chain is the flat tree's: a slotted node's goes through its
/// slot and the shadow tree to the host; a shadow tree's element's to the
/// host.
#[test]
fn hover_goes_up_the_flat_tree() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let html = cx.create_element(document, QualName::html("html"));
        cx.append_child(document, html).unwrap();
        let host = cx.create_element(document, QualName::html("div"));
        cx.append_child(html, host).unwrap();
        let slotted = cx.create_element(document, QualName::html("span"));
        cx.append_child(host, slotted).unwrap();
        let root = cx.attach_shadow(host, open());
        let wrap = cx.create_element(document, QualName::html("p"));
        let slot = cx.create_element(document, QualName::html("slot"));
        cx.append_child(wrap, slot).unwrap();
        cx.append_child(root, wrap).unwrap();
        cx.recalc_slot_assignments();
        cx.set_hover_element(document, Some(slotted));
        for node in [slotted, slot, wrap, host, html] {
            assert!(cx.is_hovered(node));
        }
        let inside = cx.create_element(document, QualName::html("i"));
        cx.append_child(root, inside).unwrap();
        cx.set_hover_element(document, Some(inside));
        assert!(cx.is_hovered(inside) && cx.is_hovered(host));
        assert!(!cx.is_hovered(slotted) && !cx.is_hovered(wrap));
    });
}

/// A shadow tree's sheets that may have changed (a `<style>` or `<link>`
/// coming or going with a subtree, a nested shadow tree's too, its text
/// or attributes changing) put its root in the list style takes, once,
/// and mark its host, not the document; the document's own don't.
#[test]
fn a_shadow_trees_sheets_changing_list_its_root() {
    use dom::StyleFlags as S;
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let html = cx.create_element(document, QualName::html("html"));
        cx.append_child(document, html).unwrap();
        let host = cx.create_element(document, QualName::html("div"));
        cx.append_child(html, host).unwrap();
        let root = cx.attach_shadow(host, open());
        let roots = || cx.take_sheet_changed_roots(document).len();
        assert_eq!(roots(), 0);
        cx.clear_style_flags(document);
        cx.clear_style_flags(host);
        let style = cx.create_element(document, QualName::html("style"));
        cx.append_child(root, style).unwrap();
        assert!(cx.style_flags(host).contains(S::STYLE_SHEETS_CHANGED));
        assert!(
            !cx.style_flags(document).contains(S::STYLE_SHEETS_CHANGED),
            "the shadow tree's"
        );
        // Twice: listed once; left for the renderer until style takes it.
        let text = cx.create_text(document, "p {}");
        cx.append_child(style, text).unwrap();
        assert_eq!(cx.sheet_changed_roots(document).len(), 1);
        assert_eq!(roots(), 1);
        assert_eq!(roots(), 0, "taken");
        cx.set_data(text, "q {}");
        assert_eq!(roots(), 1, "its text");
        cx.set_attribute(style, "media", "print");
        assert_eq!(roots(), 1, "its attributes");
        // Going, in a subtree; and a nested shadow tree's that comes.
        cx.remove(style);
        assert_eq!(roots(), 1);
        let inner_host = cx.create_element(document, QualName::html("span"));
        let inner = cx.attach_shadow(inner_host, open());
        cx.append_child(inner, cx.create_element(document, QualName::html("style")))
            .unwrap();
        assert_eq!(roots(), 0, "not connected");
        cx.append_child(root, inner_host).unwrap();
        let listed = cx.take_sheet_changed_roots(document);
        assert!(
            listed.len() == 1 && elise::Gc::ptr_eq(listed[0], inner),
            "the nested tree's"
        );
        // The document's: its own flag.
        cx.append_child(html, cx.create_element(document, QualName::html("style")))
            .unwrap();
        assert_eq!(roots(), 0);
        assert!(cx.style_flags(document).contains(S::STYLE_SHEETS_CHANGED));
    });
}

/// Each document lists its own shadow roots and constructed sheets (each
/// frame's style takes its own document's); a listed shadow root adopted
/// into another document is in that one's list.
#[test]
fn each_document_lists_its_own_changed_sheets() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let documents = [cx.create_document(true), cx.create_document(true)];
        let [a, b] = documents.map(|document| {
            let html = cx.create_element(document, QualName::html("html"));
            cx.append_child(document, html).unwrap();
            html
        });
        let host = cx.create_element(documents[1], QualName::html("div"));
        cx.append_child(b, host).unwrap();
        let root = cx.attach_shadow(host, open());
        cx.append_child(
            root,
            cx.create_element(documents[1], QualName::html("style")),
        )
        .unwrap();
        assert!(cx.take_sheet_changed_roots(documents[0]).is_empty());
        assert_eq!(cx.sheet_changed_roots(documents[1]).len(), 1);
        // Adopted while listed: the other's.
        cx.remove(host);
        cx.adopt_node(host, documents[0]);
        assert!(cx.take_sheet_changed_roots(documents[1]).is_empty());
        cx.append_child(a, host).unwrap();
        let listed = cx.take_sheet_changed_roots(documents[0]);
        assert_eq!(listed.len(), 1);
        assert!(elise::Gc::ptr_eq(listed[0], root));
        // A constructed sheet: its constructor document's.
        let contents = || std::sync::Arc::new(css::StyleSheetContents::parse("p {}", None));
        let sheet = cx.create_style_sheet(documents[1], contents(), "", false);
        cx.replace_style_sheet_contents(sheet, contents());
        assert!(cx.take_changed_style_sheets(documents[0]).is_empty());
        assert_eq!(cx.take_changed_style_sheets(documents[1]).len(), 1);
    });
}
