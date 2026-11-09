//! A window's custom element registry: made once and kept by the window;
//! its definitions, found by name, in order; "element definition is
//! running"; the pending `whenDefined` promises' numbers, one per name,
//! taken when it is defined; and the valid custom element names. An
//! element's state (undefined as made with a valid name, in the tree scope
//! byte beside the shadow bits), the upgrade candidates (connected while
//! undefined, held weakly, taken in shadow-including tree order), the
//! construction stack, and definitions found by the engine's number for
//! their constructors.

use dom::{
    ConstructionTop, CustomElementState as State, DefinitionInit, Dom, QualName, ShadowRootInit,
    ShadowRootMode, SlotAssignmentMode, names,
};

fn init(name: &str) -> DefinitionInit {
    DefinitionInit {
        name: names::intern(name),
        local_name: names::intern(name),
        observed: Box::new([names::intern("x")]),
        disable_internals: true,
        disable_shadow: false,
    }
}

#[test]
fn a_registry_keeps_its_definitions() {
    let mut dom = Dom::new();
    let window = dom.mutate(|cx| {
        let document = cx.create_document(true);
        let window = cx.create_window(document);
        elise::Persistent::new(cx.gc, window)
    });
    dom.mutate(|cx| {
        let window = window.get(cx.gc);
        assert!(cx.custom_elements_if_made(window).is_none());
        let registry = cx.custom_elements(window);
        assert!(elise::Gc::ptr_eq(cx.custom_elements(window), registry));
        assert!(elise::Gc::ptr_eq(
            cx.custom_elements_window(registry),
            window
        ));
        assert!(cx.custom_element_definitions(registry).is_empty());
        let a = cx.add_custom_element_definition(registry, init("a-a"));
        let b = cx.add_custom_element_definition(registry, init("b-b"));
        assert_eq!(cx.definition_name(b), names::intern("b-b"));
        assert_eq!(cx.definition_local_name(b), names::intern("b-b"));
        assert_eq!(cx.definition_observed_attributes(a), [names::intern("x")]);
        assert_eq!(cx.definition_disabled(a), (true, false));
        assert!(!cx.element_definition_is_running(registry));
        cx.set_element_definition_is_running(registry, true);
        assert!(cx.element_definition_is_running(registry));
    });
    dom.collect();
    dom.mutate(|cx| {
        let registry = cx
            .custom_elements_if_made(window.get(cx.gc))
            .expect("kept by its window");
        let found: Vec<_> = cx
            .custom_element_definitions(registry)
            .into_iter()
            .map(|definition| cx.definition_name(definition))
            .collect();
        assert_eq!(found, [names::intern("a-a"), names::intern("b-b")]);
        let b = cx
            .custom_element_definition(registry, names::intern("b-b"))
            .unwrap();
        assert_eq!(cx.definition_name(b), names::intern("b-b"));
        assert!(
            cx.custom_element_definition(registry, names::intern("c-c"))
                .is_none()
        );
        assert!(cx.element_definition_is_running(registry));
    });
}

#[test]
fn when_defined_numbers_one_promise_per_name() {
    let mut dom = Dom::new();
    let registry = dom.mutate(|cx| {
        let document = cx.create_document(true);
        let window = cx.create_window(document);
        elise::Persistent::new(cx.gc, cx.custom_elements(window))
    });
    let (a, b) = dom.mutate(|cx| {
        let registry = registry.get(cx.gc);
        let (a, b) = (names::intern("w-a"), names::intern("w-b"));
        assert_eq!(cx.when_defined(registry, a), None);
        let first = cx.add_when_defined(registry, a);
        let second = cx.add_when_defined(registry, b);
        assert_ne!(first, second);
        assert_eq!(cx.when_defined(registry, a), Some(first));
        assert_eq!(cx.when_defined(registry, b), Some(second));
        (first, second)
    });
    dom.collect();
    dom.mutate(|cx| {
        let registry = registry.get(cx.gc);
        let name = names::intern("w-a");
        assert_eq!(cx.when_defined(registry, name), Some(a));
        assert_eq!(cx.take_when_defined(registry, name), [a]);
        assert_eq!(cx.when_defined(registry, name), None);
        assert!(cx.take_when_defined(registry, name).is_empty());
        assert_eq!(cx.when_defined(registry, names::intern("w-b")), Some(b));
        // Another of the same name later gets another number.
        let again = cx.add_when_defined(registry, name);
        assert!(again != a && again != b);
    });
}

#[test]
fn valid_custom_element_names() {
    let valid = [
        "a-b",
        "a-",
        "x-\u{b7}",
        "a-b.c_d",
        "a-\u{1f171}",
        "annotation-xml-custom",
        "a-\u{b}",
    ];
    let invalid = [
        "",
        "ab",
        "-a",
        "1-a",
        "A-b",
        "a-B",
        "\u{e9}-a",
        "a-b c",
        "a-b\t",
        "a-b/",
        "a-b>",
        "a-\0",
        "a-\u{c}",
        "a-\r",
        "a-\n",
        "annotation-xml",
        "color-profile",
        "font-face",
        "font-face-src",
        "font-face-uri",
        "font-face-format",
        "font-face-name",
        "missing-glyph",
    ];
    for name in valid {
        assert!(dom::is_valid_custom_element_name(name), "{name:?}");
    }
    for name in invalid {
        assert!(!dom::is_valid_custom_element_name(name), "{name:?}");
    }
}

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
fn a_valid_names_element_is_undefined() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let custom = cx.create_element(document, QualName::html("x-a"));
        assert_eq!(cx.custom_element_state(custom), State::Undefined);
        for plain in [
            cx.create_element(document, QualName::html("div")),
            cx.create_element(document, QualName::html("font-face")),
            cx.create_element(
                document,
                QualName {
                    namespace: Some("http://www.w3.org/2000/svg"),
                    prefix: None,
                    local: "x-a",
                },
            ),
        ] {
            assert_eq!(cx.custom_element_state(plain), State::Uncustomized);
            assert_eq!(
                cx.custom_element_state(cx.clone_node(plain, None, false)),
                State::Uncustomized
            );
        }
        // A copy is undefined, whatever its original's state.
        cx.set_custom_element_state(custom, State::Custom);
        assert_eq!(
            cx.custom_element_state(cx.clone_node(custom, None, false)),
            State::Undefined
        );
        // Each state, beside the shadow bits.
        cx.attach_shadow(custom, open());
        for state in [
            State::Failed,
            State::Precustomized,
            State::Custom,
            State::Uncustomized,
            State::Undefined,
        ] {
            cx.set_custom_element_state(custom, state);
            assert_eq!(cx.custom_element_state(custom), state);
            assert!(cx.is_shadow_host(custom));
        }
    });
}

#[test]
fn connected_undefined_elements_are_candidates_in_tree_order() {
    let mut dom = Dom::new();
    let (registry, gone) = dom.mutate(|cx| {
        let document = cx.create_document(true);
        let window = cx.create_window(document);
        let html = cx.create_element(document, QualName::html("html"));
        cx.append_child(document, html).unwrap();
        let named = |id: &str| {
            let element = cx.create_element(document, QualName::html("c-a"));
            cx.set_attribute(element, "id", id);
            element
        };
        // Out of the document first, connected with their parent.
        let outer = named("outer");
        cx.append_child(outer, named("inner")).unwrap();
        let host = cx.create_element(document, QualName::html("div"));
        let root = cx.attach_shadow(host, open());
        cx.append_child(root, named("shadow")).unwrap();
        cx.append_child(host, named("light")).unwrap();
        cx.append_child(html, host).unwrap();
        cx.insert(html, outer, Some(host));
        cx.append_child(html, named("last")).unwrap();
        // Not connected; connected, then taken out; of another name, and
        // of a name defined.
        named("loose");
        let gone = named("gone");
        cx.append_child(html, gone).unwrap();
        cx.pre_remove(html, gone).unwrap();
        // Taken out and kept; connected twice.
        let out = named("out");
        cx.append_child(html, out).unwrap();
        cx.pre_remove(html, out).unwrap();
        cx.append_child(html, cx.create_element(document, QualName::html("div")))
            .unwrap();
        let twice = named("twice");
        cx.append_child(html, twice).unwrap();
        cx.pre_remove(html, twice).unwrap();
        cx.append_child(html, twice).unwrap();
        let other = cx.create_element(document, QualName::html("c-b"));
        cx.append_child(html, other).unwrap();
        let registry = cx.custom_elements(window);
        cx.add_custom_element_definition(registry, super_init("c-d"));
        let defined = cx.create_element(document, QualName::html("c-d"));
        cx.append_child(html, defined).unwrap();
        (
            elise::Persistent::new(cx.gc, registry),
            (
                elise::WeakPersistent::new(cx.gc, gone),
                elise::Persistent::new(cx.gc, out),
            ),
        )
    });
    let (gone, _out) = gone;
    dom.collect();
    dom.mutate(|cx| {
        // Held weakly: the one taken out and dropped is gone.
        assert!(gone.get(cx.gc).is_none());
        let registry = registry.get(cx.gc);
        let ids: Vec<String> = cx
            .take_upgrade_candidates(registry, names::intern("c-a"))
            .into_iter()
            .map(|element| {
                cx.get_attribute(element, "id")
                    .unwrap()
                    .to_str()
                    .into_owned()
            })
            .collect();
        // The host's shadow tree before its children.
        assert_eq!(ids, ["outer", "inner", "shadow", "light", "last", "twice"]);
        assert!(
            cx.take_upgrade_candidates(registry, names::intern("c-a"))
                .is_empty()
        );
        assert_eq!(
            cx.take_upgrade_candidates(registry, names::intern("c-b"))
                .len(),
            1
        );
        assert!(
            cx.take_upgrade_candidates(registry, names::intern("c-d"))
                .is_empty()
        );
    });
}

#[test]
fn many_dropped_candidates_are_let_go_of() {
    let mut dom = Dom::new();
    let (registry, kept) = dom.mutate(|cx| {
        let document = cx.create_document(true);
        let window = cx.create_window(document);
        let html = cx.create_element(document, QualName::html("html"));
        cx.append_child(document, html).unwrap();
        let kept = cx.create_element(document, QualName::html("p-a"));
        cx.append_child(html, kept).unwrap();
        (
            elise::Persistent::new(cx.gc, cx.custom_elements(window)),
            elise::Persistent::new(cx.gc, kept),
        )
    });
    // Each round connects and drops a hundred: what the list holds stays
    // near what lives (without letting go, 5000 more candidates).
    let mut first = 0;
    for round in 0..50 {
        dom.mutate(|cx| {
            let html = cx.parent(kept.get(cx.gc)).unwrap();
            let document = cx.document_of(html);
            for _ in 0..100 {
                let element = cx.create_element(document, QualName::html("p-a"));
                cx.append_child(html, element).unwrap();
                cx.pre_remove(html, element).unwrap();
            }
        });
        dom.collect();
        if round == 0 {
            first = dom.stats().used_bytes;
        }
    }
    let used = dom.stats().used_bytes;
    dom.mutate(|cx| {
        let found = cx.take_upgrade_candidates(registry.get(cx.gc), names::intern("p-a"));
        assert_eq!(found.len(), 1);
        assert!(elise::Gc::ptr_eq(found[0], kept.get(cx.gc)));
    });
    assert!(
        used < first + 8 * 1024,
        "{first} bytes after a round, {used} after them"
    );
}

fn super_init(name: &str) -> DefinitionInit {
    init(name)
}

#[test]
fn a_construction_stack_and_constructor_numbers() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let window = cx.create_window(document);
        let registry = cx.custom_elements(window);
        let a = cx.add_custom_element_definition(registry, init("s-a"));
        let b = cx.add_custom_element_definition(registry, init("s-b"));
        cx.set_definition_constructor_number(registry, a, 7);
        cx.set_definition_constructor_number(registry, b, 7);
        let numbered = cx.custom_element_definitions_numbered(registry, 7);
        assert_eq!(numbered.len(), 2);
        assert!(elise::Gc::ptr_eq(numbered[0], a) && elise::Gc::ptr_eq(numbered[1], b));
        assert!(cx.custom_element_definitions_numbered(registry, 8).is_empty());
        assert!(cx.construction_top(a).is_none());
        let (one, two) = (
            cx.create_element(document, QualName::html("s-a")),
            cx.create_element(document, QualName::html("s-a")),
        );
        cx.push_construction(a, one);
        cx.push_construction(a, two);
        assert!(matches!(cx.construction_top(a), Some(ConstructionTop::Element(top)) if elise::Gc::ptr_eq(top, two)));
        cx.mark_construction_top(a);
        assert!(matches!(cx.construction_top(a), Some(ConstructionTop::AlreadyConstructed)));
        cx.pop_construction(a);
        assert!(matches!(cx.construction_top(a), Some(ConstructionTop::Element(top)) if elise::Gc::ptr_eq(top, one)));
        assert!(cx.construction_top(b).is_none());
        cx.pop_construction(a);
        assert!(cx.construction_top(a).is_none());
        // A definition on an element; found for it by its name.
        assert!(cx.element_custom_definition(one).is_none());
        cx.set_element_custom_definition(one, a);
        assert!(elise::Gc::ptr_eq(cx.element_custom_definition(one).unwrap(), a));
        cx.clear_element_custom_definition(one);
        assert!(cx.element_custom_definition(one).is_none());
        assert!(elise::Gc::ptr_eq(cx.custom_element_definition_for(two).unwrap(), a));
        let other = cx.create_element(document, QualName::html("s-c"));
        assert!(cx.custom_element_definition_for(other).is_none());
        let found = cx.upgradable_under(document);
        assert!(found.is_empty(), "none in the tree");
        let html = cx.create_element(document, QualName::html("html"));
        cx.append_child(document, html).unwrap();
        cx.append_child(html, two).unwrap();
        cx.append_child(html, other).unwrap();
        let found = cx.upgradable_under(document);
        assert_eq!(found.len(), 1);
        assert!(elise::Gc::ptr_eq(found[0].0, two) && elise::Gc::ptr_eq(found[0].1, a));
    });
}

/// A document with `<html>` connected, its window's registry defining
/// `name`.
fn defined_page<'gc>(
    cx: &dom::Cx<'_, 'gc>,
    name: &str,
) -> (
    dom::NodeRef<'gc>,
    elise::Gc<'gc, dom::CustomElementDefinition>,
) {
    let document = cx.create_document(true);
    let window = cx.create_window(document);
    let html = cx.create_element(document, QualName::html("html"));
    cx.append_child(document, html).unwrap();
    let registry = cx.custom_elements(window);
    (html, cx.add_custom_element_definition(registry, init(name)))
}

#[test]
fn reactions_go_on_the_innermost_scopes_queue_pushed_lazily() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let (html, definition) = defined_page(cx, "q-a");
        let document = cx.document_of(html);
        let element = |id: &str| {
            let element = cx.create_element(document, QualName::html("q-a"));
            cx.set_attribute(element, "id", id);
            element
        };
        let id = |element| {
            cx.get_attribute(element, "id")
                .unwrap()
                .to_str()
                .into_owned()
        };
        // A scope with none: no queue.
        cx.enter_ce_reactions();
        assert!(!cx.leave_ce_reactions());
        // Connected in a scope: an upgrade reaction there; a nested scope
        // gets its own queue, the outer keeps its.
        cx.enter_ce_reactions();
        let (a, b, c) = (element("a"), element("b"), element("c"));
        cx.append_child(html, a).unwrap();
        cx.enter_ce_reactions();
        cx.append_child(html, b).unwrap();
        assert!(cx.leave_ce_reactions());
        let inner = cx
            .take_left_element_queue()
            .expect("the inner scope's queue");
        assert_eq!(
            cx.take_from_element_queue(&inner).map(id).as_deref(),
            Some("b")
        );
        assert!(cx.take_from_element_queue(&inner).is_none());
        // A scope with none between: the outer's queue stays.
        cx.enter_ce_reactions();
        assert!(!cx.leave_ce_reactions());
        cx.append_child(html, c).unwrap();
        assert!(cx.leave_ce_reactions());
        let outer = cx
            .take_left_element_queue()
            .expect("the outer scope's queue");
        let ids: Vec<String> = std::iter::from_fn(|| cx.take_from_element_queue(&outer))
            .map(id)
            .collect();
        assert_eq!(ids, ["a", "c"]);
        // Each element's reaction is its upgrade, once.
        for element in [a, b, c] {
            assert!(matches!(
                cx.take_element_reaction(element),
                Some(dom::ReactionRef::Upgrade(found)) if elise::Gc::ptr_eq(found, definition)
            ));
            assert!(cx.take_element_reaction(element).is_none());
        }
        // A copy gets one as it is made, connected or not.
        cx.enter_ce_reactions();
        let copy = cx.clone_node(a, None, false);
        assert!(cx.leave_ce_reactions());
        let queue = cx.take_left_element_queue().expect("the copy's");
        assert!(elise::Gc::ptr_eq(
            cx.take_from_element_queue(&queue).unwrap(),
            copy
        ));
        assert!(cx.take_element_reaction(copy).is_some());
        // Not a defined name's: none.
        cx.enter_ce_reactions();
        let other = cx.create_element(document, QualName::html("q-b"));
        cx.append_child(html, other).unwrap();
        cx.clone_node(other, None, false);
        assert!(!cx.leave_ce_reactions());
    });
}

#[test]
fn reactions_with_no_scope_go_in_the_backup_queue() {
    let mut dom = Dom::new();
    let elements = dom.mutate(|cx| {
        let (html, _) = defined_page(cx, "q-a");
        let document = cx.document_of(html);
        let (a, b) = (
            cx.create_element(document, QualName::html("q-a")),
            cx.create_element(document, QualName::html("q-a")),
        );
        cx.append_child(html, a).unwrap();
        cx.append_child(html, b).unwrap();
        (
            elise::Persistent::new(cx.gc, a),
            elise::Persistent::new(cx.gc, b),
        )
    });
    // The microtask once, while its processing is queued.
    assert!(dom.take_reactions_microtask());
    assert!(!dom.take_reactions_microtask());
    dom.mutate(|cx| {
        let first = cx.take_from_backup_element_queue().unwrap();
        assert!(elise::Gc::ptr_eq(first, elements.0.get(cx.gc)));
        // One enqueued as it is processed: in the same run, no other
        // microtask.
        let html = cx.parent(first).unwrap();
        let late = cx.create_element(cx.document_of(html), QualName::html("q-a"));
        cx.append_child(html, late).unwrap();
        assert!(elise::Gc::ptr_eq(
            cx.take_from_backup_element_queue().unwrap(),
            elements.1.get(cx.gc)
        ));
        assert!(elise::Gc::ptr_eq(
            cx.take_from_backup_element_queue().unwrap(),
            late
        ));
        assert!(cx.take_from_backup_element_queue().is_none());
    });
    assert!(!dom.take_reactions_microtask());
    // Done: the next one queues it again.
    dom.mutate(|cx| {
        let html = cx.parent(elements.0.get(cx.gc)).unwrap();
        let again = cx.create_element(cx.document_of(html), QualName::html("q-a"));
        cx.append_child(html, again).unwrap();
    });
    assert!(dom.take_reactions_microtask());
}
