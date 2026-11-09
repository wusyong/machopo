//! Resize observers and their observations: on the window's list, on the
//! target's, which keeps them (an observer no script holds lives while it
//! observes something); observing again with the same box keeps what was
//! reported, with another starts anew; what changes the version the
//! renderer watches; an entry's values and sizes.

use dom::{BoxSize, Dom, NodeRoot, QualName, ResizeObserverBox, ResizeObserverEntryValues};
use elise::Persistent;

/// A document with a window and `<html>`, and two `<div>`s not in it.
fn page(dom: &mut Dom) -> (Persistent<dom::Window>, NodeRoot, NodeRoot) {
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let window = cx.create_window(document);
        let html = cx.create_element(document, QualName::html("html"));
        cx.append_child(document, html).unwrap();
        let a = cx.create_element(document, QualName::html("div"));
        let b = cx.create_element(document, QualName::html("div"));
        (Persistent::new(cx.gc, window), cx.root(a), cx.root(b))
    })
}

const SIZE: BoxSize = BoxSize {
    inline: 10.0,
    block: 20.0,
};

/// `observe` an element (not a document); again with the same box: nothing,
/// what it reported stands; with another: anew, last; `unobserve` and
/// `disconnect` take them away. Observing changes the version.
#[test]
fn observations_and_the_version() {
    let mut dom = Dom::new();
    let (window, a, b) = page(&mut dom);
    dom.mutate(|cx| {
        let window = window.get(cx.gc);
        let (a, b) = (a.get(cx.gc), b.get(cx.gc));
        let observer = cx.create_resize_observer(window);
        let before = cx.resize_version();
        cx.observe_resize(observer, a, ResizeObserverBox::ContentBox);
        assert_ne!(cx.resize_version(), before);
        cx.observe_resize(observer, b, ResizeObserverBox::BorderBox);
        cx.observe_resize(observer, cx.document_of(a), ResizeObserverBox::ContentBox);
        let state = || {
            cx.resize_observations(observer)
                .into_iter()
                .map(|o| (cx.node_id(o.target), o.observed_box, o.last_reported))
                .collect::<Vec<_>>()
        };
        let (ia, ib) = (cx.node_id(a), cx.node_id(b));
        assert_eq!(
            state(),
            [
                (ia, ResizeObserverBox::ContentBox, None),
                (ib, ResizeObserverBox::BorderBox, None)
            ]
        );
        cx.set_resize_observation_reported(observer, a, SIZE);
        let version = cx.resize_version();
        cx.observe_resize(observer, a, ResizeObserverBox::ContentBox);
        assert_eq!(cx.resize_version(), version, "the same box: nothing");
        assert_eq!(state()[0], (ia, ResizeObserverBox::ContentBox, Some(SIZE)));
        cx.observe_resize(observer, a, ResizeObserverBox::DevicePixelContentBox);
        assert_eq!(
            state(),
            [
                (ib, ResizeObserverBox::BorderBox, None),
                (ia, ResizeObserverBox::DevicePixelContentBox, None),
            ]
        );
        cx.unobserve_resize(observer, b);
        assert_eq!(state().len(), 1);
        cx.disconnect_resize_observer(observer);
        assert!(state().is_empty());
        // Observed again after: anew.
        cx.observe_resize(observer, a, ResizeObserverBox::DevicePixelContentBox);
        assert_eq!(
            state(),
            [(ia, ResizeObserverBox::DevicePixelContentBox, None)]
        );
        cx.disconnect_resize_observer(observer);
        assert_eq!(cx.window_resize_observers(window).len(), 1);
    });
}

/// An observer lives while it observes a target that lives, though no
/// script holds it; with its targets gone, or none, it goes, and off its
/// window's list.
#[test]
fn a_target_keeps_its_observer() {
    let mut dom = Dom::new();
    let (window, a, _) = page(&mut dom);
    let (observing, idle) = dom.mutate(|cx| {
        let window = window.get(cx.gc);
        let observing = cx.create_resize_observer(window);
        cx.observe_resize(observing, a.get(cx.gc), ResizeObserverBox::ContentBox);
        let idle = cx.create_resize_observer(window);
        (
            elise::WeakPersistent::new(cx.gc, observing),
            elise::WeakPersistent::new(cx.gc, idle),
        )
    });
    dom.collect();
    dom.read(|cx| {
        assert!(observing.get(cx.gc).is_some());
        assert!(idle.get(cx.gc).is_none());
        assert_eq!(cx.window_resize_observers(window.get(cx.gc)).len(), 1);
    });
    drop(a);
    dom.collect();
    dom.read(|cx| {
        assert!(observing.get(cx.gc).is_none());
        assert!(cx.window_resize_observers(window.get(cx.gc)).is_empty());
    });
}

/// An entry keeps its target, rect and three sizes, each the same object
/// every time.
#[test]
fn an_entry_keeps_what_it_was_made_with() {
    let mut dom = Dom::new();
    let (_, a, _) = page(&mut dom);
    let entry = dom.mutate(|cx| {
        let entry = cx.create_resize_entry(
            a.get(cx.gc),
            ResizeObserverEntryValues {
                content_rect: [1.0, 2.0, 10.5, 20.0],
                border_box: BoxSize {
                    inline: 12.0,
                    block: 22.0,
                },
                content_box: BoxSize {
                    inline: 10.5,
                    block: 20.0,
                },
                device_pixel_content_box: BoxSize {
                    inline: 21.0,
                    block: 40.0,
                },
            },
        );
        Persistent::new(cx.gc, entry)
    });
    drop(a);
    dom.collect();
    dom.read(|cx| {
        let entry = entry.get(cx.gc);
        assert_eq!(
            cx.kind(cx.resize_entry_target(entry)),
            dom::NodeKind::Element
        );
        assert_eq!(
            cx.rect(cx.resize_entry_content_rect(entry)),
            [1.0, 2.0, 10.5, 20.0]
        );
        let size = |b| cx.resize_size(cx.resize_entry_size(entry, b));
        assert_eq!(
            size(ResizeObserverBox::BorderBox),
            BoxSize {
                inline: 12.0,
                block: 22.0
            }
        );
        assert_eq!(
            size(ResizeObserverBox::ContentBox),
            BoxSize {
                inline: 10.5,
                block: 20.0
            }
        );
        assert_eq!(
            size(ResizeObserverBox::DevicePixelContentBox),
            BoxSize {
                inline: 21.0,
                block: 40.0
            }
        );
        assert!(elise::Gc::ptr_eq(
            cx.resize_entry_size(entry, ResizeObserverBox::BorderBox),
            cx.resize_entry_size(entry, ResizeObserverBox::BorderBox)
        ));
    });
}

/// `unobserve` takes the observation off its target's list: observing and
/// unobserving again and again leaves the heap as it was.
#[test]
fn unobserving_leaves_nothing_on_the_target() {
    let mut dom = Dom::new();
    let (window, a, _) = page(&mut dom);
    let observer =
        dom.mutate(|cx| Persistent::new(cx.gc, cx.create_resize_observer(window.get(cx.gc))));
    let cycle = |dom: &mut Dom, times: usize| {
        for _ in 0..times {
            dom.mutate(|cx| {
                cx.observe_resize(
                    observer.get(cx.gc),
                    a.get(cx.gc),
                    ResizeObserverBox::ContentBox,
                );
                cx.unobserve_resize(observer.get(cx.gc), a.get(cx.gc));
            });
        }
        dom.collect();
        dom.used_bytes()
    };
    let before = cycle(&mut dom, 10);
    let after = cycle(&mut dom, 1000);
    assert!(after < before + 4096, "{before} then {after}");
}
