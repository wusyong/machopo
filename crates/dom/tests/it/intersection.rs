//! Intersection observers and their observations: on the window's list,
//! on the target's, which keeps them (an observer no script holds lives
//! while it observes something); the root held weakly; what changes the
//! version the renderer watches; the entry queue and the window's list of
//! observers waiting for the task.

use dom::{
    Dom, IntersectionEntryValues, IntersectionObserverOptions, IntersectionObserverRoot, NodeRoot,
    QualName,
};
use elise::Persistent;
use gfx::{MarginLength, RectF};

fn options() -> IntersectionObserverOptions {
    IntersectionObserverOptions {
        root_margin: [MarginLength::Px(0.0); 4],
        scroll_margin: [MarginLength::Px(0.0); 4],
        thresholds: vec![0.0],
        delay_micros: 0,
        track_visibility: false,
    }
}

/// A document with a window and `<html>`, and two `<div>`s not in it.
fn page(dom: &mut Dom) -> (Persistent<dom::Window>, NodeRoot, NodeRoot, NodeRoot) {
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let window = cx.create_window(document);
        let html = cx.create_element(document, QualName::html("html"));
        cx.append_child(document, html).unwrap();
        let a = cx.create_element(document, QualName::html("div"));
        let b = cx.create_element(document, QualName::html("div"));
        (
            Persistent::new(cx.gc, window),
            cx.root(html),
            cx.root(a),
            cx.root(b),
        )
    })
}

/// `observe` once per target, a node that isn't an element not at all;
/// `unobserve` and `disconnect` take them away; each change (an observer
/// made too) a new version.
#[test]
fn observations_and_the_version() {
    let mut dom = Dom::new();
    let (window, html, a, b) = page(&mut dom);
    dom.mutate(|cx| {
        let window = window.get(cx.gc);
        let (a, b) = (a.get(cx.gc), b.get(cx.gc));
        let before = cx.intersection_version();
        let observer = cx.create_intersection_observer(window, None, options());
        let made = cx.intersection_version();
        assert_ne!(made, before);
        cx.observe_intersection(observer, a);
        cx.observe_intersection(observer, b);
        let observed = cx.intersection_version();
        assert_ne!(observed, made);
        // Again, or a document: nothing.
        cx.observe_intersection(observer, a);
        cx.observe_intersection(observer, cx.document_of(a));
        assert_eq!(cx.intersection_version(), observed);
        let targets = || {
            cx.intersection_observations(observer)
                .into_iter()
                .map(|o| cx.node_id(o.target))
                .collect::<Vec<_>>()
        };
        assert_eq!(targets(), [cx.node_id(a), cx.node_id(b)]);
        let state = cx.intersection_observations(observer)[0];
        assert_eq!(state.previous_threshold_index, None);
        assert_eq!(state.last_run, None);
        // An observer that isn't observing it: nothing.
        let other = cx.create_intersection_observer(window, Some(html.get(cx.gc)), options());
        let version = cx.intersection_version();
        cx.unobserve_intersection(other, a);
        assert_eq!(cx.intersection_version(), version);
        cx.unobserve_intersection(observer, a);
        assert_ne!(cx.intersection_version(), version);
        assert_eq!(targets(), [cx.node_id(b)]);
        let version = cx.intersection_version();
        cx.disconnect_intersection_observer(observer);
        assert_ne!(cx.intersection_version(), version);
        assert!(targets().is_empty());
        // Observing again after.
        cx.observe_intersection(observer, a);
        assert_eq!(targets(), [cx.node_id(a)]);
        assert_eq!(cx.window_intersection_observers(window).len(), 2);
    });
}

/// The state the rendering update leaves on an observation.
#[test]
fn an_observations_state() {
    let mut dom = Dom::new();
    let (window, _, a, _) = page(&mut dom);
    dom.mutate(|cx| {
        let a = a.get(cx.gc);
        let observer = cx.create_intersection_observer(window.get(cx.gc), None, options());
        cx.observe_intersection(observer, a);
        cx.set_intersection_observation_run(observer, a, 12.5);
        cx.set_intersection_observation_state(observer, a, 2, true, true);
        let state = cx.intersection_observations(observer)[0];
        assert_eq!(state.last_run, Some(12.5));
        assert_eq!(state.previous_threshold_index, Some(2));
        assert!(state.previous_is_intersecting && state.previous_is_visible);
    });
}

/// An observer lives while it observes a target that lives, though no
/// script holds it (WPT's `observer-without-js-reference.html`); with its
/// targets gone, or none, it goes, and off its window's list.
#[test]
fn a_target_keeps_its_observer() {
    let mut dom = Dom::new();
    let (window, _, a, b) = page(&mut dom);
    let (observing, idle) = dom.mutate(|cx| {
        let window = window.get(cx.gc);
        let observing = cx.create_intersection_observer(window, None, options());
        cx.observe_intersection(observing, a.get(cx.gc));
        cx.observe_intersection(observing, b.get(cx.gc));
        let idle = cx.create_intersection_observer(window, None, options());
        (
            elise::WeakPersistent::new(cx.gc, observing),
            elise::WeakPersistent::new(cx.gc, idle),
        )
    });
    dom.collect();
    dom.read(|cx| {
        assert!(observing.get(cx.gc).is_some());
        assert!(idle.get(cx.gc).is_none());
        assert_eq!(cx.window_intersection_observers(window.get(cx.gc)).len(), 1);
    });
    drop(a);
    dom.collect();
    assert!(
        dom.read(|cx| observing.get(cx.gc).is_some()),
        "`b` still keeps it"
    );
    drop(b);
    dom.collect();
    dom.read(|cx| {
        assert!(observing.get(cx.gc).is_none());
        assert!(
            cx.window_intersection_observers(window.get(cx.gc))
                .is_empty()
        );
    });
}

/// An explicit root is held weakly: once it is gone the observer has
/// nothing to compute against (not the implicit root).
#[test]
fn the_root_is_held_weakly() {
    let mut dom = Dom::new();
    let (window, _, a, b) = page(&mut dom);
    let observer = dom.mutate(|cx| {
        let observer =
            cx.create_intersection_observer(window.get(cx.gc), Some(a.get(cx.gc)), options());
        cx.observe_intersection(observer, b.get(cx.gc));
        assert!(matches!(
            cx.intersection_observer_root(observer),
            IntersectionObserverRoot::Node(root) if elise::Gc::ptr_eq(root, a.get(cx.gc))
        ));
        Persistent::new(cx.gc, observer)
    });
    drop(a);
    dom.collect();
    dom.read(|cx| {
        assert!(matches!(
            cx.intersection_observer_root(observer.get(cx.gc)),
            IntersectionObserverRoot::Gone
        ));
    });
}

/// Entries queue on their observer; the first observer with one puts the
/// window's task to be queued, the rest wait with it, in the order they
/// got their first; taking them empties both. An entry keeps its target.
#[test]
fn the_entry_queue() {
    let mut dom = Dom::new();
    let (window, _, a, _) = page(&mut dom);
    let entry = dom.mutate(|cx| {
        let window = window.get(cx.gc);
        let a = a.get(cx.gc);
        let first = cx.create_intersection_observer(window, None, options());
        let second = cx.create_intersection_observer(window, None, options());
        let values = IntersectionEntryValues {
            time: 1.5,
            root_bounds: None,
            bounding_client_rect: RectF::new(1.0, 2.0, 3.0, 4.0),
            intersection_rect: RectF::default(),
            is_intersecting: false,
            is_visible: false,
            intersection_ratio: 0.0,
        };
        let entry = || cx.create_intersection_entry(a, values);
        assert!(cx.queue_intersection_entry(window, second, entry()));
        assert!(!cx.queue_intersection_entry(window, first, entry()));
        assert!(!cx.queue_intersection_entry(window, second, entry()));
        let pending = cx.take_pending_intersection_observers(window);
        assert_eq!(pending.len(), 2);
        assert!(elise::Gc::ptr_eq(pending[0], second) && elise::Gc::ptr_eq(pending[1], first));
        assert!(cx.take_pending_intersection_observers(window).is_empty());
        assert_eq!(cx.take_intersection_entries(second).len(), 2);
        assert!(cx.take_intersection_entries(second).is_empty());
        // Waiting again once taken.
        assert!(cx.queue_intersection_entry(window, second, entry()));
        let entry = cx.take_intersection_entries(first)[0];
        assert!(cx.intersection_entry_root_bounds(entry).is_none());
        assert_eq!(
            cx.rect(cx.intersection_entry_bounding_client_rect(entry)),
            [1.0, 2.0, 3.0, 4.0]
        );
        assert_eq!(
            cx.intersection_entry_values(entry),
            (1.5, false, 0.0, false)
        );
        Persistent::new(cx.gc, entry)
    });
    drop(a);
    dom.collect();
    dom.read(|cx| {
        let target = cx.intersection_entry_target(entry.get(cx.gc));
        assert_eq!(cx.kind(target), dom::NodeKind::Element);
    });
}

/// `unobserve` takes the observation off its target's list: observing and
/// unobserving again and again leaves the heap as it was.
#[test]
fn unobserving_leaves_nothing_on_the_target() {
    let mut dom = Dom::new();
    let (window, _, a, _) = page(&mut dom);
    let observer = dom.mutate(|cx| {
        Persistent::new(
            cx.gc,
            cx.create_intersection_observer(window.get(cx.gc), None, options()),
        )
    });
    let cycle = |dom: &mut Dom, times: usize| {
        for _ in 0..times {
            dom.mutate(|cx| {
                cx.observe_intersection(observer.get(cx.gc), a.get(cx.gc));
                cx.unobserve_intersection(observer.get(cx.gc), a.get(cx.gc));
            });
        }
        dom.collect();
        dom.used_bytes()
    };
    let before = cycle(&mut dom, 10);
    let after = cycle(&mut dom, 1000);
    assert!(after < before + 4096, "{before} then {after}");
}
