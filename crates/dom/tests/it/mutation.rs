//! Mutation observers' records (`Cx::observe` and the changes that queue
//! them): what each change records and for whom, the transient
//! registrations, disconnecting, taking the records, the microtask's flag.

use dom::{Cx, Dom, MutationObserver, MutationRecord, NodeRef, ObserveOptions, QualName};
use elise::Gc;

fn options(f: impl FnOnce(&mut ObserveOptions)) -> ObserveOptions {
    let mut options = ObserveOptions::default();
    f(&mut options);
    options
}

/// A record as text: its type, target's and siblings' names, the added
/// and removed nodes' names, the attribute and the old value.
fn describe<'gc>(cx: &Cx<'_, 'gc>, record: Gc<'gc, MutationRecord>) -> String {
    let name = |node: Option<NodeRef<'gc>>| {
        node.map_or_else(|| "-".to_owned(), |node| cx.node_name(node).to_string())
    };
    let list = |list| {
        (0..cx.node_list_length(list))
            .map(|index| name(cx.node_list_item(list, index)))
            .collect::<Vec<_>>()
            .join(",")
    };
    let (attribute, _) = cx.record_attribute(record);
    format!(
        "{} {} +[{}] -[{}] {} {} {} {}",
        cx.record_kind(record).name(),
        name(Some(cx.record_target(record))),
        list(cx.record_added(record)),
        list(cx.record_removed(record)),
        name(cx.record_previous_sibling(record)),
        name(cx.record_next_sibling(record)),
        attribute.map_or_else(|| "-".to_owned(), |id| dom::names::get(id).to_string()),
        cx.record_old_value(record)
            .map_or_else(|| "-".to_owned(), |old| old.to_string()),
    )
}

fn records<'gc>(cx: &Cx<'_, 'gc>, observer: Gc<'gc, MutationObserver>) -> Vec<String> {
    cx.take_records(observer)
        .into_iter()
        .map(|record| describe(cx, record))
        .collect()
}

fn element<'gc>(cx: &Cx<'_, 'gc>, document: NodeRef<'gc>, name: &str) -> NodeRef<'gc> {
    cx.create_element(document, QualName::html(name))
}

#[test]
fn tree_changes_record_what_was_added_and_removed() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let doc = cx.create_document(true);
        let div = element(cx, doc, "div");
        let observer = cx.create_mutation_observer();
        cx.observe(observer, div, options(|o| o.child_list = true), None);
        let (a, b) = (element(cx, doc, "a"), element(cx, doc, "b"));
        cx.append_child(div, a).unwrap();
        cx.insert(div, b, Some(a));
        cx.remove(a);
        assert_eq!(
            records(cx, observer),
            [
                "childList DIV +[A] -[] - - - -",
                "childList DIV +[B] -[] - A - -",
                "childList DIV +[] -[A] B - - -",
            ]
        );
        // A fragment's children: one record for them all; the fragment
        // isn't observed.
        let fragment = cx.create_document_fragment(doc);
        let (i, u) = (element(cx, doc, "i"), element(cx, doc, "u"));
        cx.append_child(fragment, i).unwrap();
        cx.append_child(fragment, u).unwrap();
        cx.take_records(observer);
        cx.append_child(div, fragment).unwrap();
        assert_eq!(records(cx, observer), ["childList DIV +[I,U] -[] B - - -"]);
        // Replacing every child: one record.
        cx.set_text_content(div, "t");
        assert_eq!(
            records(cx, observer),
            ["childList DIV +[#text] -[B,I,U] - - - -"]
        );
        // A child's own children aren't the observer's without `subtree`.
        let span = element(cx, doc, "span");
        cx.replace_all(div, Some(span));
        cx.take_records(observer);
        cx.append_child(span, element(cx, doc, "em")).unwrap();
        assert!(records(cx, observer).is_empty());
        // Moving a node records its removal from its old parent.
        let other = element(cx, doc, "section");
        cx.observe(observer, other, options(|o| o.child_list = true), None);
        cx.append_child(other, span).unwrap();
        assert_eq!(
            records(cx, observer),
            [
                "childList DIV +[] -[SPAN] - - - -",
                "childList SECTION +[SPAN] -[] - - - -"
            ]
        );
        // `outerHTML`'s replacement: one record, where it was.
        let p = element(cx, doc, "p");
        cx.append_child(other, p).unwrap();
        cx.take_records(observer);
        let fragment = cx.create_document_fragment(doc);
        cx.append_child(fragment, element(cx, doc, "q")).unwrap();
        cx.replace_with_new(span, fragment);
        assert_eq!(
            records(cx, observer),
            ["childList SECTION +[Q] -[SPAN] - P - -"]
        );
    });
}

#[test]
fn attributes_and_data_record_their_old_values_when_asked() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let doc = cx.create_document(true);
        let div = element(cx, doc, "div");
        let text = cx.create_text(doc, "one");
        cx.append_child(div, text).unwrap();
        let plain = cx.create_mutation_observer();
        let old = cx.create_mutation_observer();
        cx.observe(
            plain,
            div,
            options(|o| {
                o.attributes = true;
                o.character_data = true;
                o.subtree = true
            }),
            None,
        );
        cx.observe(
            old,
            div,
            options(|o| {
                o.attributes = true;
                o.attribute_old_value = true;
                o.character_data = true;
                o.character_data_old_value = true;
                o.subtree = true;
            }),
            Some(&["title"]),
        );
        cx.set_attribute(div, "title", "a");
        cx.set_attribute(div, "title", "b");
        cx.set_attribute(div, "lang", "en");
        cx.set_data(text, "two");
        assert_eq!(
            records(cx, plain),
            [
                "attributes DIV +[] -[] - - title -",
                "attributes DIV +[] -[] - - title -",
                "attributes DIV +[] -[] - - lang -",
                "characterData #text +[] -[] - - - -",
            ]
        );
        assert_eq!(
            records(cx, old),
            [
                "attributes DIV +[] -[] - - title -",
                "attributes DIV +[] -[] - - title a",
                "characterData #text +[] -[] - - - one",
            ],
            "the filter's attributes alone, with their old values"
        );
        // A namespaced attribute isn't the filter's.
        cx.set_attribute_ns(
            div,
            QualName {
                namespace: Some("urn:x"),
                prefix: None,
                local: "title",
            },
            "x",
        );
        assert!(records(cx, old).is_empty());
        assert_eq!(records(cx, plain).len(), 1);
        // Observing again replaces the options.
        cx.observe(plain, div, options(|o| o.child_list = true), None);
        cx.set_attribute(div, "title", "c");
        assert!(records(cx, plain).is_empty());
    });
}

#[test]
fn a_removed_node_is_still_observed_until_the_delivery() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let doc = cx.create_document(true);
        let div = element(cx, doc, "div");
        let child = element(cx, doc, "p");
        cx.append_child(div, child).unwrap();
        let observer = cx.create_mutation_observer();
        cx.observe(
            observer,
            div,
            options(|o| {
                o.attributes = true;
                o.subtree = true
            }),
            None,
        );
        cx.remove(child);
        cx.set_attribute(child, "id", "x");
        assert_eq!(
            records(cx, observer),
            ["attributes P +[] -[] - - id -"],
            "a transient registration"
        );
        assert_eq!(cx.take_pending_observers().len(), 1);
        cx.remove_transient_registrations(observer);
        cx.set_attribute(child, "id", "y");
        assert!(records(cx, observer).is_empty(), "gone with the delivery");
    });
}

#[test]
fn disconnect_takes_the_registrations_and_the_records() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let doc = cx.create_document(true);
        let div = element(cx, doc, "div");
        let observer = cx.create_mutation_observer();
        cx.observe(observer, div, options(|o| o.attributes = true), None);
        cx.set_attribute(div, "a", "1");
        cx.disconnect(observer);
        assert!(cx.take_records(observer).is_empty());
        cx.set_attribute(div, "a", "2");
        assert!(cx.take_records(observer).is_empty());
    });
}

/// The engine is told to queue the microtask once until the delivery
/// takes the pending observers.
#[test]
fn the_microtask_is_queued_once_a_delivery() {
    let mut dom = Dom::new();
    let div = dom.mutate(|cx| {
        let doc = cx.create_document(true);
        let div = element(cx, doc, "div");
        let observer = cx.create_mutation_observer();
        cx.observe(observer, div, options(|o| o.attributes = true), None);
        cx.root(div)
    });
    assert!(!dom.take_mutation_microtask());
    dom.mutate(|cx| {
        cx.set_attribute(div.get(cx.gc), "a", "1");
        cx.set_attribute(div.get(cx.gc), "a", "2");
    });
    assert!(dom.take_mutation_microtask());
    assert!(!dom.take_mutation_microtask(), "queued already");
    dom.mutate(|cx| cx.set_attribute(div.get(cx.gc), "a", "3"));
    assert!(!dom.take_mutation_microtask(), "not delivered yet");
    let delivered = dom.mutate(|cx| {
        let observers = cx.take_pending_observers();
        observers
            .iter()
            .map(|observer| cx.take_records(*observer).len())
            .sum::<usize>()
    });
    assert_eq!(delivered, 3);
    dom.mutate(|cx| cx.set_attribute(div.get(cx.gc), "a", "4"));
    assert!(
        dom.take_mutation_microtask(),
        "queued again after the delivery"
    );
}
