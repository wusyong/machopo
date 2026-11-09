//! The objects besides nodes: an event's type, flags and time, a plain
//! event target, and either as an `ObjectRef` (its wrapper's edge, a root
//! that keeps it through GCs).

use dom::{Dom, EventFlags, ObjectRef};
use elise::HeapOptions;

#[test]
fn an_event_has_its_type_flags_and_time() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let flags = EventFlags {
            bubbles: true,
            cancelable: false,
            composed: true,
            trusted: true,
        };
        let event = cx.create_event(dom::names::intern("click"), flags, None);
        assert_eq!(dom::names::get(cx.event_type(event)), "click");
        assert_eq!(cx.event_flags(event), flags);
        for flags in [
            EventFlags::default(),
            EventFlags {
                cancelable: true,
                ..EventFlags::default()
            },
        ] {
            assert_eq!(
                cx.event_flags(cx.create_event(dom::names::intern("x"), flags, None)),
                flags
            );
        }
        // A type is a name's number: Blink's event types have fixed ones,
        // others are given one the first time.
        assert_eq!(cx.event_type(event), dom::names::event::CLICK);
        let made = cx.create_event(dom::names::intern("made-up"), EventFlags::default(), None);
        let again = cx.create_event(dom::names::intern("made-up"), EventFlags::default(), None);
        assert_eq!(cx.event_type(made), cx.event_type(again));
        assert_ne!(cx.event_type(made), cx.event_type(event));
    });
    // Times count from the DOM's making, in milliseconds.
    std::thread::sleep(std::time::Duration::from_millis(2));
    let first = dom.mutate(|cx| {
        cx.event_time_stamp(cx.create_event(dom::names::intern("a"), EventFlags::default(), None))
    });
    std::thread::sleep(std::time::Duration::from_millis(2));
    let later = dom.mutate(|cx| {
        cx.event_time_stamp(cx.create_event(dom::names::intern("b"), EventFlags::default(), None))
    });
    assert!(first >= 2.0, "{first}");
    assert!(later >= first + 2.0, "{first} then {later}");
}

/// Each object has its own wrapper edge, and a root keeps it, of its
/// type, through scavenges and full GCs.
#[test]
fn objects_have_their_own_wrapper_edges_and_roots() {
    let mut dom = Dom::with_options(HeapOptions {
        verify: true,
        ..HeapOptions::default()
    });
    let roots = dom.mutate(|cx| {
        let objects = [
            ObjectRef::Node(cx.create_document(true)),
            ObjectRef::Event(cx.create_event(dom::names::intern("e"), EventFlags::default(), None)),
            ObjectRef::EventTarget(cx.create_event_target()),
        ];
        let edges = objects.map(|object| std::ptr::from_ref(cx.object_wrapper_edge(object)));
        assert!(edges[0] != edges[1] && edges[1] != edges[2] && edges[0] != edges[2]);
        // Each object's own: the same each time it is asked.
        for (object, edge) in objects.iter().zip(edges) {
            assert!(std::ptr::eq(cx.object_wrapper_edge(*object), edge));
        }
        if let ObjectRef::Node(node) = objects[0] {
            assert!(std::ptr::eq(edges[0], cx.wrapper_edge(node)));
        }
        for _ in 0..100 {
            cx.create_event(dom::names::intern("garbage"), EventFlags::default(), None);
            cx.create_event_target();
        }
        objects.map(|object| cx.root_object(object))
    });
    for _ in 0..2 {
        dom.collect();
        dom.mutate(|cx| {
            let objects = roots.each_ref().map(|root| root.get(cx.gc));
            assert!(matches!(objects[0], ObjectRef::Node(node) if cx.node_type(node) == 9));
            assert!(
                matches!(objects[1], ObjectRef::Event(event) if dom::names::get(cx.event_type(event)) == "e")
            );
            assert!(matches!(objects[2], ObjectRef::EventTarget(_)));
        });
    }
    let live = dom.stats().used_bytes;
    drop(roots);
    dom.collect();
    assert!(dom.stats().used_bytes < live);
}

/// Every kind of node holds listeners (in its kind's data, a template's
/// with its contents), and a plain target; a node with none reads as none.
#[test]
fn every_target_holds_listeners() {
    use dom::{ListenerFlags, QualName, TargetRef};
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let template = cx.create_element(document, QualName::html("template"));
        let targets = [
            TargetRef::Node(document),
            TargetRef::Node(cx.create_doctype(document, "html", "", "")),
            TargetRef::Node(cx.create_document_fragment(document)),
            TargetRef::Node(cx.create_element(document, QualName::html("div"))),
            TargetRef::Node(template),
            TargetRef::Node(cx.create_text(document, "t")),
            TargetRef::Node(cx.create_comment(document, "c")),
            TargetRef::Node(cx.create_processing_instruction(document, "t", "d")),
            TargetRef::EventTarget(cx.create_event_target()),
        ];
        for target in targets {
            assert!(cx.listeners(target).is_none());
            let first = cx.add_listener(target, dom::names::intern("x"), ListenerFlags::default());
            let second = cx.add_listener(
                target,
                dom::names::intern("y"),
                ListenerFlags {
                    capture: true,
                    ..ListenerFlags::default()
                },
            );
            let listeners = cx.listeners(target).expect("its listeners");
            assert_eq!(listeners.len(), 2);
            assert!(elise::Gc::ptr_eq(listeners.get(cx.gc, 0).unwrap(), first));
            assert!(elise::Gc::ptr_eq(listeners.get(cx.gc, 1).unwrap(), second));
        }
        // A template keeps its contents with its listeners.
        assert!(cx.template_content(template).is_some());
        // Others of the same kind have none.
        assert!(
            cx.listeners(TargetRef::Node(cx.create_text(document, "u")))
                .is_none()
        );
        assert!(
            cx.listeners(TargetRef::EventTarget(cx.create_event_target()))
                .is_none()
        );
        // Text data is still a text's.
        let TargetRef::Node(text) = targets[5] else {
            unreachable!()
        };
        assert_eq!(cx.data(text).unwrap(), "t");
    });
}

/// A listener found by type, capture and the engine's comparison; removed
/// and marked so; flags read back.
#[test]
fn listeners_are_found_and_removed() {
    use dom::{ListenerFlags, TargetRef};
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let target = TargetRef::EventTarget(cx.create_event_target());
        let flags = ListenerFlags {
            capture: false,
            once: true,
            passive: true,
        };
        let listener = cx.add_listener(target, dom::names::intern("x"), flags);
        cx.callback_edge(listener).set_embedder_word(cx.gc, 7);
        assert_eq!(cx.listener_flags(listener), flags);
        let found = |event_type: &str, capture: bool, word: u64| {
            cx.find_listener(target, dom::names::intern(event_type), capture, |edge| {
                edge.embedder_word(cx.gc) == word
            })
            .is_some()
        };
        assert!(found("x", false, 7));
        assert!(!found("y", false, 7), "another type");
        assert!(!found("x", true, 7), "another capture");
        assert!(!found("x", false, 8), "another callback");
        assert!(!cx.listener_removed(listener));
        cx.remove_listener(target, listener);
        assert!(cx.listener_removed(listener));
        assert_eq!(cx.listeners(target).unwrap().len(), 0);
        assert!(!found("x", false, 7));
    });
}

/// An event's dispatch state: its flags and phase each their own bits; its
/// targets; its path and snapshot, refilled in place; `initEvent`.
#[test]
fn an_events_dispatch_state() {
    use dom::{EventState, ListenerFlags, QualName, TargetRef};
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let flags = EventFlags {
            bubbles: true,
            cancelable: true,
            composed: true,
            trusted: true,
        };
        let event = cx.create_event(dom::names::intern("x"), flags, None);
        for state in [
            EventState {
                stop_propagation: true,
                ..EventState::default()
            },
            EventState {
                stop_immediate_propagation: true,
                canceled: true,
                ..EventState::default()
            },
            EventState {
                dispatching: true,
                in_passive_listener: true,
                ..EventState::default()
            },
        ] {
            cx.set_event_state(event, state);
            assert_eq!(cx.event_state(event), state);
            for phase in 0..4 {
                cx.set_event_phase(event, phase);
                assert_eq!(cx.event_phase(event), phase);
                assert_eq!(cx.event_state(event), state);
                assert_eq!(cx.event_flags(event), flags);
            }
        }
        let document = cx.create_document(true);
        let div = cx.create_element(document, QualName::html("div"));
        let plain = cx.create_event_target();
        cx.set_event_target(event, Some(TargetRef::Node(div)));
        cx.set_event_current_target(event, Some(TargetRef::EventTarget(plain)));
        assert!(cx.event_target(event).unwrap().ptr_eq(TargetRef::Node(div)));
        assert!(
            cx.event_current_target(event)
                .unwrap()
                .ptr_eq(TargetRef::EventTarget(plain))
        );
        let path = [TargetRef::Node(div), TargetRef::Node(document)];
        cx.set_event_path(event, &path);
        assert_eq!(cx.event_path_len(event), 2);
        assert!(
            cx.event_path_at(event, 1)
                .unwrap()
                .ptr_eq(TargetRef::Node(document))
        );
        // Set again: the old path replaced.
        cx.set_event_path(event, &path[1..]);
        assert_eq!(cx.event_path_len(event), 1);
        assert!(
            cx.event_path_at(event, 0)
                .unwrap()
                .ptr_eq(TargetRef::Node(document))
        );
        cx.clear_event_path(event);
        assert!(cx.event_path(event).is_empty());
        cx.set_event_path(event, &path[..1]);
        assert_eq!(cx.event_path(event).len(), 1);

        // The snapshot: the target's listeners of the event's type, in order.
        let target = TargetRef::Node(div);
        let a = cx.add_listener(target, dom::names::intern("x"), ListenerFlags::default());
        cx.add_listener(target, dom::names::intern("y"), ListenerFlags::default());
        let b = cx.add_listener(target, dom::names::intern("x"), ListenerFlags::default());
        assert_eq!(cx.snapshot_listeners(event, target), 2);
        assert!(elise::Gc::ptr_eq(cx.snapshot_at(event, 0).unwrap(), a));
        assert!(elise::Gc::ptr_eq(cx.snapshot_at(event, 1).unwrap(), b));
        assert_eq!(
            cx.snapshot_listeners(event, TargetRef::EventTarget(plain)),
            0
        );
        assert!(cx.snapshot_at(event, 0).is_none());

        // `initEvent`: a new type and flags, composed kept, not trusted,
        // its stop and cancel flags and its target cleared.
        cx.init_event(event, dom::names::intern("y"), false, true);
        assert_eq!(dom::names::get(cx.event_type(event)), "y");
        assert_eq!(
            cx.event_flags(event),
            EventFlags {
                bubbles: false,
                cancelable: true,
                composed: true,
                trusted: false
            }
        );
        let state = cx.event_state(event);
        assert!(!state.stop_propagation && !state.stop_immediate_propagation && !state.canceled);
        assert!(cx.event_target(event).is_none());
        assert_eq!(cx.snapshot_listeners(event, target), 1, "its new type's");
        cx.set_event_trusted(event, true);
        assert!(cx.event_flags(event).trusted);
    });
}

/// An event is 48 bytes, a 56-byte slot with its header (72 and 80 while
/// its type was a string out of the heap): its subclass's data (a 4-byte
/// edge) took padding. A listener is 16, with no finalizer either.
#[test]
fn an_event_fits_its_slot() {
    assert_eq!(std::mem::size_of::<dom::Event>(), 48);
    assert_eq!(std::mem::size_of::<dom::Listener>(), 16);
}

/// Events and listeners have no finalizer (their types are names'
/// numbers): dead ones in the nursery cost a scavenge nothing to finalize.
#[test]
fn events_and_listeners_have_no_finalizer() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let target = dom::TargetRef::EventTarget(cx.create_event_target());
        for _ in 0..100 {
            cx.create_event(dom::names::intern("made-up"), EventFlags::default(), None);
            cx.add_listener(
                target,
                dom::names::event::CLICK,
                dom::ListenerFlags::default(),
            );
        }
    });
    dom.collect();
    assert_eq!(dom.stats().last_scavenge.finalized, 0);
    // What it counts: an `ErrorEvent`'s data holds strings, finalized.
    dom.mutate(|cx| {
        let info = dom::ErrorInfo {
            message: dom::DomString::new("m"),
            filename: dom::DomString::new("f"),
            lineno: 1,
            colno: 1,
        };
        for _ in 0..10 {
            cx.create_error_event(&info, None);
        }
    });
    dom.collect();
    assert_eq!(
        dom.stats().last_scavenge.finalized,
        10,
        "the error events' data"
    );
}

/// An event handler's content attribute: set, its element's (an HTML
/// one's, a plain attribute) handler is an uncompiled one of its value,
/// at the parser's line (1 for script's); set again, the same listener;
/// removed, none. A `<body>`'s window ones are its window's, its others
/// its own; a window-only one on another element is nothing's.
#[test]
fn a_handlers_content_attribute_sets_it() {
    use dom::names::event;
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let window = cx.create_window(document);
        let div = cx.create_element(document, dom::QualName::html("div"));
        let target = dom::TargetRef::Node(div);
        cx.set_attribute(div, "onclick", "a()");
        let first = cx.event_handler(target, event::CLICK).expect("a handler");
        let (source, line, element) = cx.uncompiled_handler(first).expect("uncompiled");
        assert_eq!((&*source.to_str(), line), ("a()", 1));
        assert!(elise::Gc::ptr_eq(element, div));
        cx.set_parser_line(7);
        cx.set_attribute(div, "onclick", "b()");
        cx.set_parser_line(0);
        let again = cx.event_handler(target, event::CLICK).expect("a handler");
        assert!(elise::Gc::ptr_eq(first, again), "the same listener");
        let (source, line, _) = cx.uncompiled_handler(again).unwrap();
        assert_eq!((&*source.to_str(), line), ("b()", 7));
        // An attribute of the same local name in a namespace touches it not
        // (a compiled one stays compiled).
        cx.set_compiled_handler(target, event::CLICK);
        cx.set_attribute_ns(
            div,
            dom::QualName {
                namespace: Some("urn:y"),
                prefix: None,
                local: "onclick",
            },
            "y",
        );
        assert!(
            cx.event_handler(target, event::CLICK)
                .is_some_and(|handler| elise::Gc::ptr_eq(handler, first))
        );
        assert!(!cx.handler_uncompiled(first));
        cx.remove_attribute(div, "onclick");
        assert!(cx.event_handler(target, event::CLICK).is_none());
        // Not a handler's: another namespace's attribute, an element of a
        // namespace but HTML's and SVG's, a window-only handler on a
        // `<div>`. An SVG element's is its own.
        cx.set_attribute_ns(
            div,
            dom::QualName {
                namespace: Some("urn:x"),
                prefix: None,
                local: "onclick",
            },
            "x",
        );
        let svg = cx.create_element(
            document,
            dom::QualName {
                namespace: Some("http://www.w3.org/2000/svg"),
                prefix: None,
                local: "g",
            },
        );
        let math = cx.create_element(
            document,
            dom::QualName {
                namespace: Some("http://www.w3.org/1998/Math/MathML"),
                prefix: None,
                local: "math",
            },
        );
        cx.set_attribute(svg, "onclick", "x");
        cx.set_attribute(math, "onclick", "x");
        cx.set_attribute(div, "onafterprint", "x");
        assert!(cx.event_handler(target, event::CLICK).is_none());
        assert!(
            cx.event_handler(dom::TargetRef::Node(math), event::CLICK)
                .is_none()
        );
        assert!(
            cx.event_handler(dom::TargetRef::Node(svg), event::CLICK)
                .is_some()
        );
        assert!(cx.event_handler(target, event::AFTERPRINT).is_none());
        // A `<body>`'s.
        let body = cx.create_element(document, dom::QualName::html("body"));
        for (name, value) in [("onload", "l"), ("onafterprint", "p"), ("onclick", "c")] {
            cx.set_attribute(body, name, value);
        }
        let window = dom::TargetRef::Window(window);
        let source = |target, event_type| {
            cx.uncompiled_handler(cx.event_handler(target, event_type).unwrap())
                .unwrap()
                .0
                .to_str()
                .to_string()
        };
        assert_eq!(source(window, event::LOAD), "l");
        assert_eq!(source(window, event::AFTERPRINT), "p");
        assert_eq!(source(dom::TargetRef::Node(body), event::CLICK), "c");
        assert!(
            cx.event_handler(dom::TargetRef::Node(body), event::LOAD)
                .is_none()
        );
    });
}

/// A handler is a listener in its place: set again, it stays; taken out
/// and set again, it is last. `find_listener` never finds it; set
/// compiled, its source goes.
#[test]
fn a_handler_keeps_its_place_among_the_listeners() {
    use dom::names::event;
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let target = dom::TargetRef::EventTarget(cx.create_event_target());
        let flags = dom::ListenerFlags::default();
        let first = cx.add_listener(target, event::CLICK, flags);
        let element = cx.create_element(cx.create_document(true), dom::QualName::html("div"));
        cx.set_uncompiled_handler(target, event::CLICK, dom::DomString::new("x"), 1, element);
        let handler = cx.event_handler(target, event::CLICK).unwrap();
        let last = cx.add_listener(target, event::CLICK, flags);
        let event_ = cx.create_event(event::CLICK, EventFlags::default(), None);
        let order = || {
            let count = cx.snapshot_listeners(event_, target);
            (0..count)
                .map(|at| cx.snapshot_at(event_, at).unwrap())
                .collect::<Vec<_>>()
        };
        assert!(same(&order(), &[first, handler, last]));
        assert!(cx.listener_is_handler(handler) && cx.handler_uncompiled(handler));
        assert_eq!(cx.listener_type(handler), event::CLICK);
        let compiled = cx.set_compiled_handler(target, event::CLICK);
        assert!(elise::Gc::ptr_eq(compiled, handler) && !cx.handler_uncompiled(handler));
        assert!(cx.uncompiled_handler(handler).is_none());
        assert!(
            same(&order(), &[first, handler, last]),
            "set again, in its place"
        );
        assert!(
            cx.find_listener(target, event::CLICK, false, |_| true)
                .is_some_and(|found| elise::Gc::ptr_eq(found, first))
        );
        cx.remove_event_handler(target, event::CLICK);
        assert!(cx.listener_removed(handler));
        let again = cx.set_compiled_handler(target, event::CLICK);
        assert!(
            same(&order(), &[first, last, again]),
            "set after it went: last"
        );
    });
}

fn same<'gc>(a: &[elise::Gc<'gc, dom::Listener>], b: &[elise::Gc<'gc, dom::Listener>]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| elise::Gc::ptr_eq(*a, *b))
}

/// DOM's default passive value: a wheel or touch listener on a window, a
/// document, its root element or its body; an event handler's there too
/// (HTML adds it with no `passive`); not another target's, nor another
/// type's.
#[test]
fn wheel_listeners_at_the_top_are_passive_by_default() {
    use dom::names::event;
    use dom::{QualName, TargetRef};
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let html = cx.create_element(document, QualName::html("html"));
        cx.append_child(document, html).unwrap();
        let frameset = cx.create_element(document, QualName::html("frameset"));
        cx.append_child(html, frameset).unwrap();
        let div = cx.create_element(document, QualName::html("div"));
        cx.append_child(frameset, div).unwrap();
        let window = cx.create_window(document);
        let plain = cx.create_event_target();
        let top = [
            TargetRef::Window(window),
            TargetRef::Node(document),
            TargetRef::Node(html),
            TargetRef::Node(frameset),
        ];
        for target in top {
            for event_type in [
                event::WHEEL,
                event::MOUSEWHEEL,
                event::TOUCHSTART,
                event::TOUCHMOVE,
            ] {
                assert!(cx.default_passive(target, event_type));
            }
            assert!(!cx.default_passive(target, event::CLICK));
            assert!(!cx.default_passive(target, event::TOUCHEND));
        }
        assert!(!cx.default_passive(TargetRef::Node(div), event::WHEEL));
        assert!(!cx.default_passive(TargetRef::EventTarget(plain), event::WHEEL));
        // A `<body>` that isn't the root's child isn't the body.
        let inner = cx.create_element(document, QualName::html("body"));
        cx.append_child(div, inner).unwrap();
        assert!(!cx.default_passive(TargetRef::Node(inner), event::WHEEL));
        // Event handlers.
        cx.set_compiled_handler(TargetRef::Node(frameset), event::WHEEL);
        cx.set_compiled_handler(TargetRef::Node(div), event::WHEEL);
        let passive = |node| {
            let listener = cx
                .event_handler(TargetRef::Node(node), event::WHEEL)
                .unwrap();
            cx.listener_flags(listener).passive
        };
        assert!(passive(frameset));
        assert!(!passive(div));
    });
}

/// The DOM counts its wheel listeners and their changes (what the
/// compositor's region is made from), and whether an element has a
/// blocking one: its own, or the root's document's or window's; a passive
/// one, or one of another type, doesn't block.
#[test]
fn wheel_listeners_are_counted_and_block() {
    use dom::names::event;
    use dom::{ListenerFlags, QualName, TargetRef};
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let html = cx.create_element(document, QualName::html("html"));
        cx.append_child(document, html).unwrap();
        let div = cx.create_element(document, QualName::html("div"));
        cx.append_child(html, div).unwrap();
        let window = cx.create_window(document);
        let blocking = ListenerFlags::default();
        let passive = ListenerFlags {
            passive: true,
            ..ListenerFlags::default()
        };
        assert!(!cx.may_have_wheel_listeners());
        let start = cx.wheel_listeners_version();
        cx.add_listener(TargetRef::Node(div), event::CLICK, blocking);
        assert!(!cx.may_have_wheel_listeners());
        assert_eq!(cx.wheel_listeners_version(), start);
        let on_div = cx.add_listener(TargetRef::Node(div), event::MOUSEWHEEL, passive);
        assert!(cx.may_have_wheel_listeners());
        assert!(!cx.has_own_blocking_wheel_listener(div));
        let blocking_on_div = cx.add_listener(TargetRef::Node(div), event::WHEEL, blocking);
        assert!(cx.has_own_blocking_wheel_listener(div));
        assert!(!cx.has_own_blocking_wheel_listener(html));
        assert!(!cx.document_blocks_wheel(document));
        assert_eq!(cx.wheel_listeners_version(), start + 2);
        cx.remove_listener(TargetRef::Node(div), blocking_on_div);
        assert!(!cx.has_own_blocking_wheel_listener(div));
        cx.remove_listener(TargetRef::Node(div), on_div);
        assert!(!cx.may_have_wheel_listeners());
        assert_eq!(cx.wheel_listeners_version(), start + 4);
        // The window's blocks the root element and the whole document,
        // not the elements below (they inherit it in style).
        let on_window = cx.add_listener(TargetRef::Window(window), event::WHEEL, blocking);
        assert!(cx.has_own_blocking_wheel_listener(html));
        assert!(cx.document_blocks_wheel(document));
        assert!(!cx.has_own_blocking_wheel_listener(div));
        cx.remove_listener(TargetRef::Window(window), on_window);
        cx.add_listener(TargetRef::Node(document), event::MOUSEWHEEL, blocking);
        assert!(cx.document_blocks_wheel(document));
    });
}
