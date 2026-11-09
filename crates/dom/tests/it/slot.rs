//! Slots: a shadow tree's slots get its host's children by name (the
//! first slot of a name in tree order) or as `assign` gives them, kept as
//! the tree and the names change; DOM's signal slots, the slots whose
//! assigned nodes changed, each once until taken.

use dom::{Cx, Dom, NodeRef, QualName, ShadowRootInit, ShadowRootMode, SlotAssignmentMode};

fn init(slot_assignment: SlotAssignmentMode) -> ShadowRootInit {
    ShadowRootInit {
        mode: ShadowRootMode::Open,
        delegates_focus: false,
        slot_assignment,
        clonable: false,
        serializable: false,
    }
}

fn same<'gc>(a: &[NodeRef<'gc>], b: &[NodeRef<'gc>]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| elise::Gc::ptr_eq(*a, *b))
}

fn slot<'gc>(cx: &Cx<'_, 'gc>, document: NodeRef<'gc>, name: Option<&str>) -> NodeRef<'gc> {
    let slot = cx.create_element(document, QualName::html("slot"));
    if let Some(name) = name {
        cx.set_attribute(slot, "name", name);
    }
    slot
}

#[test]
fn slots_get_their_hosts_children_by_name() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let host = cx.create_element(document, QualName::html("div"));
        let a = cx.create_element(document, QualName::html("span"));
        cx.set_attribute(a, "slot", "a");
        let b = cx.create_element(document, QualName::html("b"));
        let text = cx.create_text(document, "t");
        for child in [a, b, text] {
            cx.append_child(host, child).unwrap();
        }
        let root = cx.attach_shadow(host, init(SlotAssignmentMode::Named));
        let named = slot(cx, document, Some("a"));
        let default = slot(cx, document, None);
        cx.append_child(root, named).unwrap();
        cx.append_child(root, default).unwrap();
        assert!(cx.is_slot(named) && !cx.is_slot(a));
        assert!(same(&cx.assigned_nodes(named), &[a]));
        assert!(same(&cx.assigned_nodes(default), &[b, text]));
        assert!(
            cx.assigned_slot(text)
                .is_some_and(|of| elise::Gc::ptr_eq(of, default))
        );
        // Signaled once each, taken once.
        let signaled = cx.take_signal_slots();
        assert!(same(&signaled, &[named, default]));
        assert!(cx.take_signal_slots().is_empty());
        // Renamed: the old slot's and the new's.
        cx.set_attribute(b, "slot", "a");
        assert!(same(&cx.take_signal_slots(), &[default, named]));
        assert!(same(&cx.assigned_nodes(named), &[a, b]));
        // The same name again: no change.
        cx.set_attribute(b, "slot", "a");
        assert!(cx.take_signal_slots().is_empty());
        // A slot of the name before the first: it gets them.
        let first = slot(cx, document, Some("a"));
        cx.insert(root, first, Some(named));
        assert!(same(&cx.take_signal_slots(), &[first, named]));
        assert!(same(&cx.assigned_nodes(first), &[a, b]) && cx.assigned_nodes(named).is_empty());
        // A slot renamed away: the next of the name gets them.
        cx.set_attribute(first, "name", "z");
        assert!(same(&cx.take_signal_slots(), &[first, named]));
        assert!(same(&cx.assigned_nodes(named), &[a, b]));
        // Out of the shadow tree: no slot.
        cx.remove(named);
        assert!(!cx.is_slot(named) && cx.assigned_nodes(named).is_empty());
        assert!(same(&cx.take_signal_slots(), &[named]));
        assert!(cx.assigned_slot(a).is_none());
        // A host's child going: its slot.
        cx.remove(text);
        assert!(same(&cx.take_signal_slots(), &[default]));
        assert!(cx.assigned_slot(text).is_none() && cx.assigned_nodes(default).is_empty());
    });
}

#[test]
fn a_slots_fallback_content_signals_it_while_it_has_no_nodes() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let host = cx.create_element(document, QualName::html("div"));
        let root = cx.attach_shadow(host, init(SlotAssignmentMode::Named));
        let empty = slot(cx, document, None);
        cx.append_child(root, empty).unwrap();
        assert!(cx.take_signal_slots().is_empty());
        let fallback = cx.create_element(document, QualName::html("i"));
        cx.append_child(empty, fallback).unwrap();
        assert!(same(&cx.take_signal_slots(), &[empty]));
        assert!(same(&cx.flattened_assigned_nodes(empty), &[fallback]));
        let child = cx.create_text(document, "x");
        cx.append_child(host, child).unwrap();
        assert!(same(&cx.take_signal_slots(), &[empty]));
        cx.append_child(empty, cx.create_element(document, QualName::html("b")))
            .unwrap();
        assert!(cx.take_signal_slots().is_empty());
        assert!(same(&cx.flattened_assigned_nodes(empty), &[child]));
    });
}

#[test]
fn manual_slots_get_the_nodes_assigned() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let host = cx.create_element(document, QualName::html("div"));
        let a = cx.create_element(document, QualName::html("span"));
        let b = cx.create_text(document, "b");
        let outside = cx.create_element(document, QualName::html("u"));
        cx.append_child(host, a).unwrap();
        cx.append_child(host, b).unwrap();
        let root = cx.attach_shadow(host, init(SlotAssignmentMode::Manual));
        let first = slot(cx, document, None);
        let second = slot(cx, document, None);
        cx.append_child(root, first).unwrap();
        cx.append_child(root, second).unwrap();
        assert!(cx.assigned_nodes(first).is_empty() && cx.take_signal_slots().is_empty());
        cx.assign(first, &[b, outside, a, b]);
        assert!(same(&cx.manually_assigned(first), &[b, outside, a]));
        assert!(same(&cx.assigned_nodes(first), &[b, a]));
        assert!(same(&cx.take_signal_slots(), &[first]));
        // The same again: no change.
        cx.assign(first, &[b, outside, a]);
        assert!(cx.take_signal_slots().is_empty());
        cx.append_child(host, outside).unwrap();
        assert!(same(&cx.assigned_nodes(first), &[b, outside, a]));
        // To another: no longer the first's.
        cx.assign(second, &[a]);
        assert!(same(&cx.take_signal_slots(), &[first, second]));
        assert!(same(&cx.assigned_nodes(first), &[b, outside]));
        assert!(
            cx.assigned_slot(a)
                .is_some_and(|of| elise::Gc::ptr_eq(of, second))
        );
        // Out of the host: assigned again when it comes back.
        cx.remove(a);
        assert!(same(&cx.take_signal_slots(), &[second]));
        assert!(cx.assigned_nodes(second).is_empty());
        cx.append_child(host, a).unwrap();
        assert!(same(&cx.assigned_nodes(second), &[a]));
        // A slot that goes: its nodes no one's.
        cx.remove(first);
        assert!(same(&cx.take_signal_slots(), &[first]));
        assert!(cx.assigned_slot(b).is_none());
    });
}

/// Of the slots of a name, the first in tree order is signaled and found,
/// before the assignment is worked out again too; a comment is no
/// slottable; a stale assignment is read as it will be.
#[test]
fn the_first_slot_of_a_name_is_found_before_the_assignment_is() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let host = cx.create_element(document, QualName::html("div"));
        let root = cx.attach_shadow(host, init(SlotAssignmentMode::Named));
        let first = slot(cx, document, Some("a"));
        let second = slot(cx, document, Some("a"));
        let default = slot(cx, document, None);
        for at in [first, second, default] {
            cx.append_child(root, at).unwrap();
        }
        cx.take_signal_slots();
        let a = cx.create_element(document, QualName::html("span"));
        cx.set_attribute(a, "slot", "a");
        cx.append_child(host, a).unwrap();
        assert!(same(&cx.take_signal_slots(), &[first]));
        // Stale: found without working it out.
        assert!(
            cx.assigned_slot(a)
                .is_some_and(|of| elise::Gc::ptr_eq(of, first))
        );
        let comment = cx.create_comment(document, "c");
        cx.append_child(host, comment).unwrap();
        assert!(cx.take_signal_slots().is_empty());
        assert!(cx.assigned_slot(comment).is_none());
        // Renamed, stale again: its new slot.
        cx.set_attribute(a, "slot", "");
        assert!(same(&cx.take_signal_slots(), &[first, default]));
        assert!(
            cx.assigned_slot(a)
                .is_some_and(|of| elise::Gc::ptr_eq(of, default))
        );
        assert!(same(&cx.assigned_nodes(default), &[a]) && cx.assigned_nodes(first).is_empty());
        // Into the host through a fragment: each child its slot.
        let fragment = cx.create_document_fragment(document);
        let b = cx.create_text(document, "b");
        let c = cx.create_element(document, QualName::html("i"));
        cx.set_attribute(c, "slot", "a");
        cx.append_child(fragment, b).unwrap();
        cx.append_child(fragment, c).unwrap();
        cx.append_child(host, fragment).unwrap();
        assert!(same(&cx.take_signal_slots(), &[default, first]));
        assert!(
            same(&cx.assigned_nodes(default), &[a, b]) && same(&cx.assigned_nodes(first), &[c])
        );
        // A fragment with a slot into the shadow tree: the slots worked out.
        let fragment = cx.create_document_fragment(document);
        let before = slot(cx, document, Some("a"));
        cx.append_child(fragment, before).unwrap();
        cx.insert(root, fragment, Some(first));
        assert!(same(&cx.take_signal_slots(), &[before, first]));
        assert!(same(&cx.assigned_nodes(before), &[c]));
    });
}

#[test]
fn a_manual_slot_out_of_the_tree_or_given_others_isnt_a_nodes() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let host = cx.create_element(document, QualName::html("div"));
        let a = cx.create_element(document, QualName::html("span"));
        let b = cx.create_element(document, QualName::html("b"));
        cx.append_child(host, a).unwrap();
        cx.append_child(host, b).unwrap();
        let root = cx.attach_shadow(host, init(SlotAssignmentMode::Manual));
        let first = slot(cx, document, None);
        cx.append_child(root, first).unwrap();
        cx.assign(first, &[a, b]);
        // Given others: the old nodes are no longer its.
        cx.assign(first, &[b]);
        cx.take_signal_slots();
        cx.append_child(host, cx.create_text(document, "stale"))
            .unwrap();
        assert!(cx.assigned_slot(a).is_none());
        cx.remove(a);
        assert!(cx.take_signal_slots().is_empty());
        // `slot` names nothing in the manual mode.
        cx.set_attribute(b, "slot", "x");
        assert!(cx.take_signal_slots().is_empty());
        // Out of the tree: its nodes no one's, stale or not.
        cx.remove(first);
        cx.take_signal_slots();
        cx.append_child(host, cx.create_text(document, "stale"))
            .unwrap();
        assert!(cx.assigned_slot(b).is_none());
        cx.remove(b);
        assert!(cx.take_signal_slots().is_empty());
    });
}

/// A node assigned to a slot of another shadow tree: the slot it leaves
/// is signaled, its tree's assignment worked out again.
#[test]
fn a_node_assigned_across_shadow_trees_leaves_its_slot() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let host = cx.create_element(document, QualName::html("div"));
        let a = cx.create_element(document, QualName::html("span"));
        cx.append_child(host, a).unwrap();
        let root = cx.attach_shadow(host, init(SlotAssignmentMode::Manual));
        let mine = slot(cx, document, None);
        cx.append_child(root, mine).unwrap();
        let other_host = cx.create_element(document, QualName::html("div"));
        let other_root = cx.attach_shadow(other_host, init(SlotAssignmentMode::Manual));
        let theirs = slot(cx, document, None);
        cx.append_child(other_root, theirs).unwrap();
        cx.assign(mine, &[a]);
        assert!(same(&cx.assigned_nodes(mine), &[a]));
        cx.take_signal_slots();
        cx.assign(theirs, &[a]);
        // It isn't the other host's child: in neither.
        assert!(same(&cx.take_signal_slots(), &[mine]));
        assert!(cx.assigned_nodes(mine).is_empty() && cx.assigned_nodes(theirs).is_empty());
        assert!(cx.assigned_slot(a).is_none());
        // Stale: still not the other tree's slot.
        cx.append_child(host, cx.create_text(document, "stale"))
            .unwrap();
        assert!(cx.assigned_slot(a).is_none());
    });
}
