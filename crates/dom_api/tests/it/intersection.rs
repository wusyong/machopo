//! What the rendering update makes of the intersections the host computed
//! (`update_intersections`): an entry the first time, then only when the
//! threshold index, whether it intersects or whether it is visible
//! changed; one task for the window, which calls each observer with
//! entries back (`notify_intersection_observers`). The renderer computes
//! the intersections (`shell`'s tests drive it).

use std::rc::Rc;

use dom::{IntersectionObserver, IntersectionObserverOptions, NodeRoot, QualName, Window};
use dom_api::{ApiCx, ComputedIntersection};
use elise::Persistent;
use gfx::MarginLength;
use gfx::RectF;

use crate::geometry::page;
use crate::{TestDom, with_host};

/// Fields drop in order: the roots before the heap.
struct Page {
    window: Persistent<Window>,
    body: Rc<NodeRoot>,
    dom: TestDom,
}

impl Page {
    fn new() -> Self {
        let (mut dom, nodes, _) = page(false, &[]);
        let window = dom.mutate(|cx| Persistent::new(cx.gc, cx.create_window(nodes[0].get(cx.gc))));
        Self {
            dom,
            window,
            body: nodes[2].clone(),
        }
    }

    /// A `<div id=id>` in the body.
    fn target(&mut self, id: &str) -> Rc<NodeRoot> {
        let body = self.body.clone();
        self.dom.mutate(|cx| {
            let body = body.get(cx.gc);
            let div = cx.create_element(cx.document_of(body), QualName::html("div"));
            cx.set_attribute(div, "id", id);
            cx.append_child(body, div).unwrap();
            Rc::new(cx.root(div))
        })
    }

    /// An observer of the window's, of callback number `callback`, with
    /// `thresholds` (and `track_visibility`), observing `targets`.
    fn observer(
        &mut self,
        callback: u64,
        thresholds: &[f64],
        track_visibility: bool,
        targets: &[&Rc<NodeRoot>],
    ) -> Persistent<IntersectionObserver> {
        let window = &self.window;
        self.dom.mutate(|cx| {
            let observer = cx.create_intersection_observer(
                window.get(cx.gc),
                None,
                IntersectionObserverOptions {
                    root_margin: [MarginLength::Px(0.0); 4],
                    scroll_margin: [MarginLength::Px(0.0); 4],
                    thresholds: thresholds.to_vec(),
                    delay_micros: if track_visibility { 100_000 } else { 0 },
                    track_visibility,
                },
            );
            cx.intersection_callback_edge(observer)
                .set_embedder_word(cx.gc, callback);
            for target in targets {
                cx.observe_intersection(observer, target.get(cx.gc));
            }
            Persistent::new(cx.gc, observer)
        })
    }

    /// `update_intersections` at `time` with what each of `computed` says:
    /// whether the task is to be queued.
    fn update(&mut self, time: f64, computed: &[Computed<'_>]) -> bool {
        let window = &self.window;
        self.dom.mutate(|cx| {
            let computed: Vec<ComputedIntersection<'_>> = computed
                .iter()
                .map(|c| ComputedIntersection {
                    observer: c.observer.get(cx.gc),
                    target: c.target.get(cx.gc),
                    target_rect: c.target_rect,
                    intersection_rect: c.intersection_rect,
                    root_bounds: c.root_bounds,
                    is_visible: c.is_visible,
                })
                .collect();
            dom_api::update_intersections(&ApiCx::new(cx), window.get(cx.gc), &computed, time)
        })
    }

    /// The task: what each observer called back was given.
    fn notify(&mut self) -> Vec<(u64, Vec<String>)> {
        let window = self.window.clone();
        with_host(&self.dom, |host| {
            dom_api::notify_intersection_observers(host, &window)
        });
        self.dom.1.intersection_log.take()
    }

    /// `takeRecords()`: how many.
    fn take_records(&mut self, observer: &Persistent<IntersectionObserver>) -> usize {
        self.dom
            .mutate(|cx| cx.take_intersection_entries(observer.get(cx.gc)).len())
    }
}

struct Computed<'a> {
    observer: &'a Persistent<IntersectionObserver>,
    target: &'a Rc<NodeRoot>,
    target_rect: Option<RectF>,
    intersection_rect: Option<RectF>,
    root_bounds: Option<RectF>,
    is_visible: bool,
}

const ROOT: Option<RectF> = Some(RectF::new(0.0, 0.0, 800.0, 600.0));

/// A 100 by 100 target at `y`, the part of it above 600 in the root.
fn at<'a>(
    observer: &'a Persistent<IntersectionObserver>,
    target: &'a Rc<NodeRoot>,
    y: f32,
) -> Computed<'a> {
    let visible = (600.0 - y).clamp(0.0, 100.0);
    Computed {
        observer,
        target,
        target_rect: Some(RectF::new(0.0, y, 100.0, 100.0)),
        intersection_rect: (visible > 0.0 || y == 600.0)
            .then(|| RectF::new(0.0, y, 100.0, visible)),
        root_bounds: ROOT,
        is_visible: false,
    }
}

/// The first computation always gives an entry; the same again nothing;
/// then a change of the threshold index one. A ratio below the first
/// threshold doesn't intersect (as Chrome), though its rect and ratio are
/// given; one at the root's edge (an empty intersection, the ratio 0)
/// does; a target
/// with no area that meets the root has the ratio 1, one with no rect
/// zeros. One task for the window, whatever the number of entries; each
/// observer called with its entries, in the order it got its first (not
/// one whose entries `takeRecords()` took).
#[test]
fn what_the_rendering_update_queues() {
    let mut page = Page::new();
    let (a, z, edge) = (page.target("a"), page.target("z"), page.target("edge"));
    let first = page.observer(1, &[0.5, 1.0], false, &[&a, &z]);
    let second = page.observer(2, &[0.0], false, &[&a, &edge]);
    let zero = Some(RectF::new(5.0, 5.0, 0.0, 0.0));
    let none = Computed {
        target_rect: None,
        intersection_rect: None,
        root_bounds: Some(RectF::default()),
        ..at(&second, &a, 0.0)
    };
    // The second's first: it waits for the task before the first's.
    assert!(page.update(1.0, &[none, at(&second, &edge, 600.0)]));
    assert!(!page.update(
        1.0,
        &[
            at(&first, &a, 575.0),
            Computed {
                target_rect: zero,
                intersection_rect: zero,
                ..at(&first, &z, 0.0)
            },
        ]
    ));
    assert_eq!(
        page.notify(),
        [
            (
                2,
                vec![
                    "a 1 false 0 false 0,0,0,0 0,0,0,0 0,0,0,0".to_owned(),
                    "edge 1 true 0 false 0,0,800,600 0,600,100,100 0,600,100,0".to_owned(),
                ]
            ),
            (
                1,
                vec![
                    "a 1 false 0.25 false 0,0,800,600 0,575,100,100 0,575,100,25".to_owned(),
                    "z 1 true 1 false 0,0,800,600 5,5,0,0 5,5,0,0".to_owned(),
                ]
            ),
        ]
    );
    // The same again: nothing, no task.
    assert!(!page.update(2.0, &[at(&first, &a, 575.0), at(&second, &edge, 600.0)]));
    assert!(page.notify().is_empty());
    // At 0.5 the first's `a` crosses a threshold; at 0.75 it doesn't.
    assert!(page.update(3.0, &[at(&first, &a, 550.0)]));
    assert!(!page.update(4.0, &[at(&first, &a, 525.0)]));
    assert_eq!(
        page.notify(),
        [(
            1,
            vec!["a 3 true 0.5 false 0,0,800,600 0,550,100,100 0,550,100,50".to_owned(),]
        )]
    );
    // Taken before the task: the observer isn't called back.
    assert!(page.update(5.0, &[at(&first, &a, 500.0), at(&second, &edge, 700.0)]));
    assert_eq!(page.take_records(&first), 1);
    assert_eq!(
        page.notify(),
        [(
            2,
            vec!["edge 5 false 0 false 0,0,800,600 0,700,100,100 0,0,0,0".to_owned(),]
        )]
    );
}

/// One unobserved (or disconnected) since it was computed gets nothing;
/// what was queued before stays (the spec's `disconnect()` doesn't empty
/// the queue).
#[test]
fn an_unobserved_target_gets_nothing() {
    let mut page = Page::new();
    let (a, b) = (page.target("a"), page.target("b"));
    let observer = page.observer(1, &[0.0], false, &[&a, &b]);
    assert!(page.update(1.0, &[at(&observer, &a, 0.0)]));
    page.dom
        .mutate(|cx| cx.unobserve_intersection(observer.get(cx.gc), a.get(cx.gc)));
    assert!(!page.update(1.0, &[at(&observer, &a, 700.0)]));
    page.dom
        .mutate(|cx| cx.disconnect_intersection_observer(observer.get(cx.gc)));
    assert!(!page.update(1.0, &[at(&observer, &b, 0.0)]));
    let calls = page.notify();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1.len(), 1);
    assert!(calls[0].1[0].starts_with("a 1 true"), "{calls:?}");
}

/// Whether the target is visible is a change of its own: the same
/// intersection, visible now, gets an entry that says so; the same again
/// nothing, and hidden again another. One that doesn't intersect isn't
/// visible, whatever the host found (Blink computes it only for one that
/// does).
#[test]
fn a_change_of_visibility_brings_an_entry() {
    let mut page = Page::new();
    let a = page.target("a");
    let observer = page.observer(1, &[0.0], true, &[&a]);
    let mut seen = Vec::new();
    for (y, is_visible) in [
        (0.0, false),
        (0.0, true),
        (0.0, true),
        (0.0, false),
        (700.0, true),
    ] {
        page.update(
            1.0,
            &[Computed {
                is_visible,
                ..at(&observer, &a, y)
            }],
        );
        for (_, entries) in page.notify() {
            seen.extend(entries.into_iter().map(|e| {
                let fields: Vec<&str> = e.split(' ').collect();
                format!("{} {}", fields[2], fields[4])
            }));
        }
    }
    assert_eq!(
        seen,
        ["true false", "true true", "true false", "false false"]
    );
}

/// An entry's time is when its intersection was computed, coarsened as
/// `performance.now()` is.
#[test]
fn an_entrys_time_is_its_computations() {
    let mut page = Page::new();
    let a = page.target("a");
    let observer = page.observer(1, &[0.0], false, &[&a]);
    page.update(1234.56789, &[at(&observer, &a, 0.0)]);
    let calls = page.notify();
    assert!(calls[0].1[0].starts_with("a 1234.5 "), "{calls:?}");
}
