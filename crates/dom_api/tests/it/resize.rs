//! The rendering update's resize observer steps (`gather_and_broadcast`)
//! against a fake layout: an observation is called back the first time
//! and when its observed box's size changes; the sizes, the content rect
//! at the padding, the device-pixel content box snapped from where the
//! box is painted; the depth broadcast and what a round skips, the depth
//! the flat tree's; and an entry read as its observer is called.

use std::rc::Rc;

use dom::{NodeRoot, QualName, ResizeObserver, ResizeObserverBox, Window};
use elise::Persistent;
use gfx::PointF;

use crate::geometry::{block, fake, page};
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
            window,
            body: nodes[2].clone(),
            dom,
        }
    }

    /// A `<div id=id>` in `parent`, laid out `width` by 10 at the origin.
    fn div(&mut self, parent: &NodeRoot, id: &str, width: f32) -> Rc<NodeRoot> {
        let div = self.dom.mutate(|cx| {
            let parent = parent.get(cx.gc);
            let div = cx.create_element(cx.document_of(parent), QualName::html("div"));
            cx.set_attribute(div, "id", id);
            cx.append_child(parent, div).unwrap();
            Rc::new(cx.root(div))
        });
        self.resize(&div, width);
        div
    }

    /// `element` laid out `width` by 10 now.
    fn resize(&self, element: &NodeRoot, width: f32) {
        let id = self.dom.read(|cx| cx.node_id(element.get(cx.gc)));
        fake(&self.dom, |layout| {
            layout
                .boxes
                .insert(id, block(0.0, 0.0, width, 10.0, [0.0; 4]));
        });
    }

    /// An observer of callback number `callback` observing `targets`'
    /// content boxes.
    fn observer(&mut self, callback: u64, targets: &[&Rc<NodeRoot>]) -> Persistent<ResizeObserver> {
        let window = &self.window;
        self.dom.mutate(|cx| {
            let observer = cx.create_resize_observer(window.get(cx.gc));
            cx.resize_callback_edge(observer)
                .set_embedder_word(cx.gc, callback);
            for target in targets {
                cx.observe_resize(observer, target.get(cx.gc), ResizeObserverBox::ContentBox);
            }
            Persistent::new(cx.gc, observer)
        })
    }

    /// A round at `depth`: what it did, and each observer called with
    /// its entries.
    fn round(&mut self, depth: usize) -> (dom_api::ResizeObserverRound, Vec<(u64, Vec<String>)>) {
        let window = self.window.clone();
        let round = with_host(&self.dom, |host| {
            dom_api::gather_and_broadcast(host, &window, depth)
        });
        (round, self.dom.1.resize_log.take())
    }
}

/// The first round calls every observation back, whatever its size (no
/// box: zeros), in the order observed, the observers in the order made;
/// the content box inside the borders and padding, the content rect at
/// the padding's offset; the device-pixel content box snapped from where
/// the border box is painted (10.4px from 0.3px at 2x: 20 pixels). Then
/// nothing until a size changes.
#[test]
fn what_a_round_calls_back() {
    let mut page = Page::new();
    let body = page.body.clone();
    let (a, none) = (page.div(&body, "a", 10.0), page.div(&body, "none", 0.0));
    let ids = page
        .dom
        .read(|cx| (cx.node_id(a.get(cx.gc)), cx.node_id(none.get(cx.gc))));
    fake(&page.dom, |layout| {
        let mut b = block(0.0, 0.0, 30.4, 22.0, [1.0, 2.0, 3.0, 4.0]);
        b.block.as_mut().unwrap().padding = [5.0, 6.0, 7.0, 8.0];
        layout.boxes.insert(ids.0, b);
        layout.boxes.remove(&ids.1);
        layout.paint_offsets.insert(ids.0, PointF::new(0.3, 0.0));
        layout.viewport.device_scale_factor = 2.0;
    });
    let first = page.observer(1, &[&none, &a]);
    let second = page.observer(2, &[&a]);
    let (round, calls) = page.round(0);
    assert_eq!(round.broadcast, Some(3));
    assert!(!round.skipped);
    // The content box: 30.4 less the borders (2, 4) and the padding (6, 8)
    // is 10.4 wide; in device pixels from the border box's paint offset
    // (Blink's), (0.6 + 20.8) rounded less 0.6 rounded: 21 - 1 = 20.
    assert_eq!(
        calls,
        [
            (
                1,
                vec![
                    "none 0,0,0,0 0x0 0x0 0x0".to_owned(),
                    "a 8,5,10.399999618530273,6 30.399999618530273x22 10.399999618530273x6 20x12"
                        .to_owned()
                ]
            ),
            (
                2,
                vec![
                    "a 8,5,10.399999618530273,6 30.399999618530273x22 10.399999618530273x6 20x12"
                        .to_owned()
                ]
            ),
        ]
    );
    // The same again: nothing; a change of another box's size (the
    // border's) neither.
    assert_eq!(page.round(0).1, []);
    fake(&page.dom, |layout| {
        layout
            .boxes
            .get_mut(&ids.0)
            .unwrap()
            .block
            .as_mut()
            .unwrap()
            .border = [1.0, 2.0, 3.0, 2.0];
        layout.boxes.get_mut(&ids.0).unwrap().border_box.width -= 2.0;
    });
    assert_eq!(page.round(0).1, []);
    page.resize(&a, 50.0);
    let calls = page.round(0).1;
    assert_eq!(calls.len(), 2);
    assert!(calls[0].1[0].starts_with("a 0,0,50,10"), "{calls:?}");
    let _ = (first, second);
}

/// A round calls back only what is deeper than its depth, broadcasting
/// the shallowest it called; what changed no deeper waits (skipped).
#[test]
fn the_depth_and_what_is_skipped() {
    let mut page = Page::new();
    let body = page.body.clone();
    let outer = page.div(&body, "outer", 10.0);
    let inner = page.div(&outer, "inner", 10.0);
    let observer = page.observer(1, &[&outer, &inner]);
    let (round, _) = page.round(0);
    assert_eq!(round.broadcast, Some(3), "html, body, outer");
    page.resize(&outer, 20.0);
    page.resize(&inner, 20.0);
    let (round, calls) = page.round(3);
    assert_eq!(round.broadcast, Some(4));
    assert!(round.skipped, "outer, as deep as 3");
    assert_eq!(calls.len(), 1);
    assert!(calls[0].1[0].starts_with("inner "), "{calls:?}");
    // At no depth it is called back.
    let (round, calls) = page.round(0);
    assert!(!round.skipped);
    assert!(calls[0].1[0].starts_with("outer "), "{calls:?}");
    let _ = observer;
}

/// The depth is the flat tree's: an element assigned to a slot in its
/// host's shadow tree is as deep as the slot's ancestors make it (body,
/// host, the shadow tree's `<div>`, the slot), not just under its host.
#[test]
fn the_depth_is_the_flat_trees() {
    let mut page = Page::new();
    let body = page.body.clone();
    let host = page.div(&body, "host", 10.0);
    page.dom.mutate(|cx| {
        let host = host.get(cx.gc);
        let document = cx.document_of(host);
        let root = cx.attach_shadow(
            host,
            dom::ShadowRootInit {
                mode: dom::ShadowRootMode::Open,
                delegates_focus: false,
                slot_assignment: dom::SlotAssignmentMode::Named,
                clonable: false,
                serializable: false,
            },
        );
        let div = cx.create_element(document, QualName::html("div"));
        cx.append_child(root, div).unwrap();
        let slot = cx.create_element(document, QualName::html("slot"));
        cx.append_child(div, slot).unwrap();
    });
    let slotted = page.div(&host, "slotted", 10.0);
    let observer = page.observer(1, &[&slotted]);
    let (round, _) = page.round(0);
    assert_eq!(
        round.broadcast,
        Some(6),
        "html, body, host, div, slot, slotted"
    );
    let _ = observer;
}

/// An observer's entries are read as it is called: what the layout says
/// then (a callback before it laid the document out anew), and that is
/// what it last reported.
#[test]
fn entries_are_read_as_their_observer_is_called() {
    let mut page = Page::new();
    let body = page.body.clone();
    let a = page.div(&body, "a", 10.0);
    // The first observer's callback (100 or more) lays `a` out at 77px.
    let first = page.observer(100, &[&a]);
    let second = page.observer(2, &[&a]);
    let calls = page.round(0).1;
    assert!(calls[0].1[0].starts_with("a 0,0,10,"), "{calls:?}");
    assert!(calls[1].1[0].starts_with("a 0,0,77,"), "{calls:?}");
    // The second reported 77: only the first is called back again.
    let calls = page.round(0).1;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, 100);
    let _ = (first, second);
}

/// What an observation reports is its own box's size: a border-box one
/// whose border box is unchanged isn't called back again, though its
/// content box differs from it.
#[test]
fn a_border_box_observation_reports_its_box() {
    let mut page = Page::new();
    let body = page.body.clone();
    let a = page.div(&body, "a", 10.0);
    let id = page.dom.read(|cx| cx.node_id(a.get(cx.gc)));
    fake(&page.dom, |layout| {
        layout
            .boxes
            .insert(id, block(0.0, 0.0, 30.0, 10.0, [1.0, 2.0, 1.0, 2.0]));
    });
    let observer = page.dom.mutate(|cx| {
        let observer = cx.create_resize_observer(page.window.get(cx.gc));
        cx.resize_callback_edge(observer)
            .set_embedder_word(cx.gc, 1);
        cx.observe_resize(observer, a.get(cx.gc), ResizeObserverBox::BorderBox);
        Persistent::new(cx.gc, observer)
    });
    assert_eq!(page.round(0).1.len(), 1);
    assert_eq!(page.round(0).1, []);
    let _ = observer;
}
