//! Hit testing a parsed, styled, laid-out document: which element is at a
//! point, through transforms, clips and scroll offsets, and in paint order.

use std::collections::HashMap;
use std::sync::Arc;

use dom::{ShadowRootInit, ShadowRootMode, SlotAssignmentMode};
use gfx::{NodeId, PointF, RectF, SizeF};
use layout::BoxFragment;
use paint::{HitTestResult, PrePaint, ScrollHitTestRegion, ScrollNodeId, ScrollOffsets};

use crate::page;

const VIEWPORT: SizeF = SizeF::new(800.0, 600.0);

struct Page {
    view: Arc<BoxFragment>,
    ids: HashMap<String, NodeId>,
}

fn lay_out(html: &str) -> Page {
    lay_out_with_shadows(html, &[])
}

/// [`lay_out`], with the element of each id in `shadows` given an open
/// shadow tree of its markup. Each element with a `data-wheel` attribute
/// gets a blocking `wheel` listener.
fn lay_out_with_shadows(html: &str, shadows: &[(&str, &str)]) -> Page {
    let mut page = page::Page::with(html, |cx, document| {
        for (host, markup) in shadows {
            let host = page::find(cx, document, host);
            let root = cx.attach_shadow(
                host,
                ShadowRootInit {
                    mode: ShadowRootMode::Open,
                    delegates_focus: false,
                    slot_assignment: SlotAssignmentMode::Named,
                    clonable: false,
                    serializable: false,
                },
            );
            let fragment = html::parser::parse_fragment_in(cx, host, markup, false);
            cx.append_child(root, fragment).unwrap();
        }
        page::listen_for_wheels(cx, document);
    });
    Page {
        view: page.lay_out(VIEWPORT),
        ids: page.ids(),
    }
}

impl Page {
    fn pre(&self, scroll: &ScrollOffsets) -> PrePaint {
        paint::pre_paint(&self.view, scroll, 1.0)
    }

    fn hit_with(&self, scroll: &ScrollOffsets, x: f32, y: f32) -> HitTestResult {
        paint::hit_test(&self.view, &self.pre(scroll), PointF::new(x, y))
    }

    /// The `id` of the element at `(x, y)`, unscrolled.
    fn at(&self, x: f32, y: f32) -> Option<&str> {
        self.name(self.hit_with(&Default::default(), x, y).node)
    }

    fn name(&self, node: Option<NodeId>) -> Option<&str> {
        let node = node?;
        Some(
            self.ids
                .iter()
                .find(|(_, id)| **id == node)
                .map_or("(no id)", |(name, _)| name.as_str()),
        )
    }
}

const RESET: &str = "<style>html, body { margin: 0 }</style>";

#[test]
fn text_inline_boxes_and_blocks() {
    let page = lay_out(&format!(
        "{RESET}<body id=body>
         <div id=block style='height: 100px; font: 20px/1 Ahem'>XX<span id=span style='padding-left: 30px'>XX<span id=inner>X</span></span></div>
         <div id=parent><p id=p style='height: 10px; margin: 0'></p><span id=after>X</span> text</div>"
    ));
    // Text right in the block is the block's.
    assert_eq!(page.at(5.0, 5.0), Some("block"));
    // The span's text, and its padding, which only its box covers.
    assert_eq!(page.at(95.0, 5.0), Some("span"));
    assert_eq!(page.at(45.0, 5.0), Some("span"));
    // Text in an inline element in another: the inner one, painted last.
    assert_eq!(page.at(115.0, 5.0), Some("inner"));
    // Below the line: the block's own box.
    assert_eq!(page.at(5.0, 50.0), Some("block"));
    // Past the text in the anonymous block after `<p>`: the anonymous
    // block's background, which belongs to the element around it.
    assert_eq!(page.at(5.0, 105.0), Some("p"));
    assert_eq!(page.at(700.0, 115.0), Some("parent"));
    // Below everything: no element, not even `<html>` (it ends there).
    assert_eq!(page.at(5.0, 500.0), None);
}

#[test]
fn what_text_overflow_hides_is_not_hit() {
    let page = lay_out(&format!(
        "{RESET}<body id=body>
         <div id=block style='width: 110px; height: 100px; font: 20px/1 Ahem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap'>X<span id=span>XXXXXXX</span>X</div>
         <div id=block2 style='width: 100px; height: 20px; font: 20px/1 Ahem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap'>XX<span id=ib style='display: inline-block; width: 60px; height: 20px'></span>XX</div>"
    ));
    // The visible squares: the block's, then three of the span's.
    assert_eq!(page.at(5.0, 5.0), Some("block"));
    assert_eq!(page.at(25.0, 5.0), Some("span"));
    assert_eq!(page.at(75.0, 5.0), Some("span"));
    // The ellipsis (one square in Ahem) follows the span's text and is
    // its. Past it, the span's box still covers its hidden text (its
    // geometry is as if nothing were hidden).
    assert_eq!(page.at(95.0, 5.0), Some("span"));
    assert_eq!(page.at(105.0, 5.0), Some("span"));
    // Below the line, where the hidden text would be: the block.
    assert_eq!(page.at(95.0, 50.0), Some("block"));
    // A hidden atomic inline: the ellipsis (the block's: it follows the
    // block's own text) covers its start, the block the rest.
    assert_eq!(page.at(45.0, 105.0), Some("block2"));
    assert_eq!(page.at(75.0, 105.0), Some("block2"));
}

/// A float is hit before the blocks around it, whose backgrounds it is
/// painted over, and before them as a whole: what is in it before the
/// block after it (CSS 2 Appendix E, backwards).
#[test]
fn floats_are_hit_before_the_blocks_around_them() {
    let page = lay_out(&format!(
        "{RESET}<body id=body>
         <div id=float style='float: left; width: 100px; height: 100px'>
           <div id=inner style='margin: 25px; width: 50px; height: 50px'></div>
         </div>
         <div id=after style='width: 200px; height: 150px'></div>"
    ));
    assert_eq!(page.at(10.0, 10.0), Some("float"));
    assert_eq!(page.at(50.0, 50.0), Some("inner"));
    assert_eq!(page.at(150.0, 10.0), Some("after"));
}

/// A float in a line is hit as one beside the block is.
#[test]
fn a_float_in_a_line_is_hit() {
    let page = lay_out(&format!(
        "{RESET}<body id=body><div id=para style='font: 20px/1 Ahem; width: 200px'>XX<span
           id=float style='float: left; width: 40px; height: 40px'></span></div>"
    ));
    assert_eq!(page.at(10.0, 10.0), Some("float"));
    assert_eq!(page.at(50.0, 10.0), Some("para"));
}

/// A pseudo-element's text and box are hit as its element (Blink sends
/// the events to the element).
#[test]
fn a_pseudo_element_is_hit_as_its_element() {
    let page = lay_out(&format!(
        "{RESET}<style>#text::before {{ content: 'XX' }}
           #box::before {{ content: ''; display: block; width: 50px; height: 50px }}</style>
         <body id=body><div id=text style='font: 20px/1 Ahem; width: 200px'></div><div id=box></div>"
    ));
    assert_eq!(page.at(10.0, 10.0), Some("text"));
    assert_eq!(page.at(10.0, 40.0), Some("box"));
}

/// An outside marker, out in its list's padding, is hit as its list item.
#[test]
fn a_marker_is_hit_as_its_list_item() {
    let page = lay_out(&format!(
        "{RESET}<body id=body><ol id=ol style='margin: 0; padding-left: 100px; font: 20px/1 Ahem'>
         <li id=li>X</ol>"
    ));
    // `1. ` from 40 to 100.
    assert_eq!(page.at(50.0, 10.0), Some("li"));
    assert_eq!(page.at(10.0, 10.0), Some("ol"));
}

/// An element's bounds and paint offset are its own boxes', not those of
/// the pseudo-element boxes in it, which have its node too: a block's, an
/// inline box's, and an atomic inline's in an inline element.
#[test]
fn an_elements_bounds_are_its_own_boxs() {
    let page = lay_out(&format!(
        "{RESET}<style>#d::before {{ content: ''; display: block; position: relative;
           left: 200px; width: 10px; height: 10px }}
           #i::before {{ content: 'X'; padding-left: 300px }}
           #s::before {{ content: ''; display: inline-block; width: 10px; height: 10px }}</style>
         <div id=d style='width: 100px; height: 50px; margin-left: 3px'></div>
         <div id=i style='width: 100px; height: 50px'></div>
         <div style='font: 20px/1 Ahem'>X<span id=s>Y</span></div>"
    ));
    let pre = page.pre(&Default::default());
    let (d, i, s) = (page.ids["d"], page.ids["i"], page.ids["s"]);
    assert_eq!(
        paint::element_bounds(&page.view, &pre, d),
        Some(RectF::new(3.0, 0.0, 100.0, 50.0))
    );
    assert_eq!(
        paint::paint_offset(&page.view, &pre, d),
        Some(PointF::new(3.0, 0.0))
    );
    assert_eq!(
        paint::element_bounds(&page.view, &pre, i),
        Some(RectF::new(0.0, 50.0, 100.0, 50.0))
    );
    // An inline element's boxes on the lines have no paint offset.
    assert_eq!(paint::paint_offset(&page.view, &pre, s), None);
}

#[test]
fn transforms_move_what_is_hit() {
    let page = lay_out(&format!(
        "{RESET}<body id=body>
         <div id=moved style='width: 50px; height: 50px; transform: translate(100px, 0)'></div>
         <div id=scaled style='width: 50px; height: 50px; transform: scale(2)'></div>"
    ));
    assert_eq!(page.at(120.0, 10.0), Some("moved"));
    // Where it would be untransformed is just `<body>`.
    assert_eq!(page.at(10.0, 10.0), Some("body"));
    // Scaled about its center (25, 75): from -25 to 75 across, and from
    // 25 to 125 down, over the first box's space.
    assert_eq!(page.at(70.0, 60.0), Some("scaled"));
    assert_eq!(page.at(80.0, 60.0), Some("body"));
}

/// What `clip-path` cuts away isn't hit, even in its bounds (Blink's
/// `ClipPathClipper::HitTest`), nor what it holds there; `clip` cuts an
/// absolutely positioned box to its rect.
#[test]
fn clip_paths_and_clips_cut_what_is_hit() {
    let page = lay_out(&format!(
        "{RESET}<body id=body>
         <div id=circle style='width: 100px; height: 100px; clip-path: circle(50%)'><div id=in style='height: 100px'></div></div>
         <div id=even style='width: 100px; height: 100px; clip-path: path(evenodd, \"M0 0 h100 v100 h-100 Z M25 25 h50 v50 h-50 Z\")'></div>
         <div id=clipped style='position: absolute; top: 300px; width: 100px; height: 100px; clip: rect(0, 50px, auto, auto)'></div>
         <svg style='position: absolute; top: 450px' width=100 height=100>
           <clipPath id=half><rect width=50 height=100 /></clipPath>
           <rect id=series width=100 height=100 clip-path='url(#half)' />
         </svg>"
    ));
    assert_eq!(page.at(50.0, 50.0), Some("in"));
    // In its bounds, out of the circle.
    assert_eq!(page.at(5.0, 5.0), Some("body"));
    // The hole an even-odd path leaves.
    assert_eq!(page.at(10.0, 110.0), Some("even"));
    assert_eq!(page.at(50.0, 150.0), Some("body"));
    assert_eq!(page.at(25.0, 350.0), Some("clipped"));
    // Below `<body>`: nothing.
    assert_eq!(page.at(75.0, 350.0), None);
    // An SVG element's `<clipPath>` cuts what it is hit by.
    assert_eq!(page.at(25.0, 500.0), Some("series"));
    assert_ne!(page.at(75.0, 500.0), Some("series"));
}

/// An out-of-flow box is hit where its parent places it in tree order, as
/// it is painted: under what comes after it, though its containing block
/// holds it after its in-flow boxes; in a stacking context its containing
/// block is outside, under what comes after that.
#[test]
fn out_of_flow_boxes_are_hit_where_the_tree_places_them() {
    let page = lay_out(&format!(
        "{RESET}<body id=body style='position: relative'>
         <div><div id=abs style='position: absolute; width: 100px; height: 100px'></div></div>
         <div id=translucent style='opacity: 0.5; width: 50px; height: 50px'></div>
         <div id=faded style='opacity: 0.5; height: 100px; margin-top: 50px'>
           <div><div id=escaped style='position: absolute; top: 200px; width: 100px; height: 100px'></div></div>
         </div>
         <div id=after style='position: relative; width: 50px; height: 50px'></div>"
    ));
    assert_eq!(page.at(25.0, 25.0), Some("translucent"));
    assert_eq!(page.at(75.0, 75.0), Some("abs"));
    // `#after`, at 200px, over the box escaping `#faded`.
    assert_eq!(page.at(25.0, 225.0), Some("after"));
    assert_eq!(page.at(75.0, 225.0), Some("escaped"));
}

/// A turned box is hit where it is turned to, about its origin: a 100×20
/// bar turned a quarter about its center stands from 40 to 60 across and
/// -40 to 60 down; one turned about its top left corner lies to the left
/// of it.
#[test]
fn turned_boxes_are_hit_where_they_are_turned_to() {
    let page = lay_out(&format!(
        "{RESET}<body id=body style='padding-top: 100px'>
         <div id=center style='width: 100px; height: 20px; transform: rotate(90deg)'></div>
         <div id=corner style='margin-left: 200px; width: 100px; height: 20px;
                               transform-origin: 0 0; transform: rotate(90deg)'></div>"
    ));
    // The center bar: within the turned shape, not where it was.
    assert_eq!(page.at(50.0, 70.0), Some("center"));
    assert_eq!(page.at(50.0, 155.0), Some("center"));
    assert_eq!(page.at(10.0, 110.0), Some("body"));
    // The corner one, from (200, 120): it now covers 180 to 200 across
    // and 120 to 220 down.
    assert_eq!(page.at(190.0, 200.0), Some("corner"));
    assert_eq!(page.at(250.0, 125.0), Some("body"));
}

#[test]
fn clipped_content_is_not_hit() {
    let page = lay_out(&format!(
        "{RESET}<body id=body>
         <div id=clip style='width: 50px; height: 50px; overflow: hidden'>
           <div id=big style='width: 200px; height: 200px'></div>
         </div>
         <div id=next style='height: 300px'></div>"
    ));
    assert_eq!(page.at(10.0, 10.0), Some("big"));
    // Outside the clip `big` isn't painted, so isn't hit: what is under
    // it is.
    assert_eq!(page.at(100.0, 10.0), Some("body"));
    assert_eq!(page.at(10.0, 100.0), Some("next"));
}

#[test]
fn scroll_offsets_and_scroll_containers() {
    let page = lay_out(&format!(
        "{RESET}<body id=body>
         <div id=scroller style='width: 100px; height: 50px; padding: 10px; overflow: auto'>
           <div id=a style='height: 50px'></div><div id=b style='height: 50px'></div>
         </div>"
    ));
    let pre = page.pre(&Default::default());
    let scroller = pre
        .trees
        .scroll
        .iter()
        .position(|node| {
            node.as_ref()
                .is_some_and(|node| node.element == page.ids["scroller"])
        })
        .map(paint::ScrollNodeId);
    assert!(scroller.is_some());
    let hit = page.hit_with(&Default::default(), 20.0, 20.0);
    assert_eq!((page.name(hit.node), hit.scroll), (Some("a"), scroller));
    // Scrolled by 50, `b` is there.
    let scrolled: ScrollOffsets = [(page.ids["scroller"], PointF::new(0.0, 50.0))]
        .into_iter()
        .collect();
    assert_eq!(
        page.name(page.hit_with(&scrolled, 20.0, 20.0).node),
        Some("b")
    );
    // The container's own padding: still in it, so a wheel there scrolls
    // it.
    let hit = page.hit_with(&Default::default(), 5.0, 5.0);
    assert_eq!(
        (page.name(hit.node), hit.scroll),
        (Some("scroller"), scroller)
    );
    // Outside it, nothing but the viewport scrolls.
    let hit = page.hit_with(&Default::default(), 200.0, 5.0);
    assert_eq!((page.name(hit.node), hit.scroll), (Some("body"), None));
}

impl Page {
    /// Where the element `id` is, scrolled by `scroll`.
    fn bounds_with(&self, scroll: &ScrollOffsets, id: &str) -> Option<RectF> {
        paint::element_bounds(&self.view, &self.pre(scroll), self.ids[id])
    }

    fn bounds(&self, id: &str) -> Option<RectF> {
        self.bounds_with(&Default::default(), id)
    }
}

#[test]
fn an_elements_bounds_are_its_border_boxes() {
    let page = lay_out(&format!(
        "{RESET}<body id=body style='font: 20px/1 Ahem'>
         <div id=block style='margin: 10px; padding: 5px; border: 2px solid; width: 100px; height: 30px'>XXXXXXXXXXXX</div>
         <div style='width: 100px'>XX<span id=span style='padding-left: 10px'>XXX XXX</span></div>
         <div id=hidden style='display: none'>X</div>"
    ));
    // The border box, without the text overflowing it.
    assert_eq!(
        page.bounds("block"),
        Some(RectF::new(10.0, 10.0, 114.0, 44.0))
    );
    // An inline element on two lines: its box on the first (padding and
    // three glyphs after two others), and on the second.
    assert_eq!(
        page.bounds("span"),
        Some(RectF::new(0.0, 64.0, 110.0, 40.0))
    );
    assert_eq!(page.bounds("hidden"), None);
}

#[test]
fn bounds_follow_transforms_and_scroll_offsets() {
    let page = lay_out(&format!(
        "{RESET}<body id=body>
         <div id=moved style='width: 50px; height: 50px; transform: translate(100px, 20px)'></div>
         <div id=scroller style='width: 100px; height: 50px; overflow: auto'>
           <div style='height: 50px'></div><div id=b style='height: 50px'></div>
         </div>"
    ));
    assert_eq!(
        page.bounds("moved"),
        Some(RectF::new(100.0, 20.0, 50.0, 50.0))
    );
    // Below the scroller's port, where it is laid out; not cut by its clip.
    assert_eq!(page.bounds("b"), Some(RectF::new(0.0, 100.0, 100.0, 50.0)));
    let scrolled: ScrollOffsets = [(page.ids["scroller"], PointF::new(0.0, 50.0))]
        .into_iter()
        .collect();
    assert_eq!(
        page.bounds_with(&scrolled, "b"),
        Some(RectF::new(0.0, 50.0, 100.0, 50.0))
    );
}

/// Text and inline boxes right in a scroll container move with its offset,
/// as its own border box doesn't.
#[test]
fn inline_bounds_in_a_scroll_container_scroll() {
    let page = lay_out(&format!(
        "{RESET}<body id=body>
         <div id=lines style='width: 20px; height: 20px; overflow: auto; font: 20px/1 Ahem'>X X <span id=s>X</span></div>"
    ));
    assert_eq!(page.bounds("s"), Some(RectF::new(0.0, 40.0, 20.0, 20.0)));
    let scrolled: ScrollOffsets = [(page.ids["lines"], PointF::new(0.0, 20.0))]
        .into_iter()
        .collect();
    assert_eq!(
        page.bounds_with(&scrolled, "s"),
        Some(RectF::new(0.0, 20.0, 20.0, 20.0))
    );
    assert_eq!(
        page.bounds_with(&scrolled, "lines"),
        Some(RectF::new(0.0, 0.0, 20.0, 20.0))
    );
}

#[test]
fn what_paints_on_top_is_hit() {
    let page = lay_out(&format!(
        "{RESET}<body id=body>
         <div id=translucent style='height: 50px; opacity: 0.5'></div>
         <div id=under style='height: 50px; margin-top: -50px'></div>
         <div id=first style='height: 20px; opacity: 0.5'></div>
         <div id=second style='height: 20px; margin-top: -20px; opacity: 0.5'></div>
         <div style='height: 20px; font: 20px/1 Ahem'><span id=text>X</span></div>
         <div id=later style='height: 20px; margin-top: -20px'></div>"
    ));
    // A stacking context paints over the normal flow, even a later
    // sibling.
    assert_eq!(page.at(10.0, 10.0), Some("translucent"));
    // Of two stacking contexts, the later one paints on top.
    assert_eq!(page.at(10.0, 60.0), Some("second"));
    // Text paints after every block background, even a later one's.
    assert_eq!(page.at(10.0, 80.0), Some("text"));
    // Next to the text, the later block's background is on top.
    assert_eq!(page.at(50.0, 80.0), Some("later"));
}

impl Page {
    /// The region as painted with the containers scrolled by `scroll`.
    fn region(&self, scroll: &ScrollOffsets) -> ScrollHitTestRegion {
        let pre = self.pre(scroll);
        let composited = paint::composited_scrollers(&pre.trees);
        let size = layout::document_size(&self.view);
        let artifact = paint::paint(&self.view, &pre, size, &());
        paint::main_thread_scroll_hit_test_region(&artifact, &pre, &composited)
    }

    /// At every point, every `step` px over `area`, with the containers
    /// scrolled by `scroll`: the compositor, given the region painted at
    /// `painted` (which differs only in what containers that are layers
    /// are scrolled by: the compositor moves those without painting), either
    /// sends a wheel to the main thread, or scrolls what the main thread
    /// would. That is, the scrollers a wheel can move from the container the
    /// compositor picks (the innermost that is a layer, by its border box
    /// cut by its clips) and those around it that are layers are those from
    /// the one hit testing picks and around it.
    fn assert_compositor_agrees(
        &self,
        painted: &ScrollOffsets,
        scroll: &ScrollOffsets,
        area: RectF,
        step: f32,
    ) {
        let region = self.region(painted);
        let pre = self.pre(scroll);
        let trees = &pre.trees;
        let composited = paint::composited_scrollers(trees);
        let to_root = trees.to_root();
        let picked_at: Vec<Option<RectF>> = trees
            .scroll
            .iter()
            .zip(&composited)
            .map(|(node, &composited)| {
                composited.then(|| {
                    let node = node.as_ref().expect("a composited container's node");
                    let mut rect = to_root[node.transform.0].map_rect(node.border_box);
                    for clip in trees.clip_chain(node.clip) {
                        let clip = &trees.clip[clip.0];
                        if let Some(clip_rect) = clip.rect {
                            rect = rect.intersect(&to_root[clip.transform.0].map_rect(clip_rect));
                        }
                    }
                    rect
                })
            })
            .collect();
        let movable = |node: &paint::ScrollNode| {
            (node.user_scrollable_x && node.contents.width > node.container.width)
                || (node.user_scrollable_y && node.contents.height > node.container.height)
        };
        let chain = |mut from: Option<ScrollNodeId>, only_composited: bool| {
            let mut out = Vec::new();
            while let Some(node) = from {
                let n = &trees.scroll(node);
                if movable(n) && (!only_composited || composited[node.0]) {
                    out.push(node);
                }
                from = n.parent;
            }
            out
        };
        let mut y = area.y;
        while y < area.bottom() {
            let mut x = area.x;
            while x < area.right() {
                let p = PointF::new(x, y);
                let under: Vec<usize> = (0..trees.scroll.len())
                    .filter(|&i| picked_at[i].is_some_and(|r| r.contains(p)))
                    .collect();
                let forwarded = region.root.iter().any(|r| r.contains(p))
                    || under.iter().any(|&i| {
                        let space = trees.scroll(paint::ScrollNodeId(i)).scroll_translation;
                        let origin = to_root[space.0].as_translation().unwrap();
                        let inside = PointF::new(p.x - origin.x, p.y - origin.y);
                        region.scrollers[i].iter().any(|r| r.contains(inside))
                    });
                if !forwarded {
                    let main = paint::hit_test(&self.view, &pre, p).scroll;
                    let compositor = under.last().map(|&i| ScrollNodeId(i));
                    assert_eq!(
                        chain(compositor, true),
                        chain(main, false),
                        "({x}, {y}): the compositor would scroll other scrollers, region {region:?}"
                    );
                }
                x += step;
            }
            y += step;
        }
    }

    fn assert_compositor_agrees_unscrolled(&self, area: RectF, step: f32) {
        self.assert_compositor_agrees(&Default::default(), &Default::default(), area, step);
    }
}

fn area(x: f32, y: f32, width: f32, height: f32) -> RectF {
    RectF::new(x, y, width, height)
}

/// A container that isn't a layer (its opacity), which the user can
/// scroll: its border box, grown by 1px.
#[test]
fn the_region_covers_a_containers_border() {
    let page = lay_out(&format!(
        "{RESET}<div style='width: 100px; height: 50px; border: 10px solid; overflow: auto; opacity: 0.9'>
           <div style='height: 300px'></div>
         </div>"
    ));
    assert_eq!(
        page.region(&Default::default()).root,
        [area(-1.0, -1.0, 122.0, 72.0)]
    );
    page.assert_compositor_agrees_unscrolled(area(0.0, 0.0, 200.0, 120.0), 0.5);
}

/// Content is clipped to the padding box with each edge rounded, which can
/// reach past the border box by half a px; scaled up, by several. The
/// region rounds in the container's own space first.
#[test]
fn the_region_covers_a_scaled_containers_rounded_clip() {
    let page = lay_out(&format!(
        "{RESET}<div style='height: 300px'></div>
         <div style='width: 20px; height: 50.6px; transform: scale(10); overflow: auto'>
           <div style='height: 300px'></div>
         </div>"
    ));
    page.assert_compositor_agrees_unscrolled(area(-120.0, -10.0, 260.0, 620.0), 0.5);
}

/// The turned container isn't a layer; the one around it is, and the
/// compositor scrolls it without painting again: the turned one's rect
/// moves with it.
#[test]
fn the_region_in_a_container_that_is_a_layer_moves_with_it() {
    let page = lay_out(&format!(
        "{RESET}<div id=outer style='width: 200px; height: 150px; overflow: auto'>
           <div style='height: 40px'></div>
           <div id=inner style='width: 100px; height: 60px; overflow: auto; transform: matrix(0.866, 0.5, -0.5, 0.866, 0, 0)'>
             <div style='height: 300px'></div>
           </div>
           <div style='height: 400px'></div>
         </div>"
    ));
    let outer = page.ids["outer"];
    let region = page.region(&Default::default());
    assert_eq!(region.root, []);
    assert_eq!(region.scrollers[0].len(), 1);
    for scrolled in [0.0, 30.0, 300.0] {
        let scroll: ScrollOffsets = [(outer, PointF::new(0.0, scrolled))].into_iter().collect();
        page.assert_compositor_agrees(
            &Default::default(),
            &scroll,
            area(-20.0, -20.0, 260.0, 220.0),
            0.5,
        );
    }
}

#[test]
fn the_region_is_cut_by_the_clips_outside_not_by_the_viewport() {
    // The inner container sticks out of the outer one, which clips it.
    // Neither is a layer (the opacity); the outer one, `hidden`, can't be
    // scrolled by the user, and is left out.
    let page = lay_out(&format!(
        "{RESET}<div style='width: 100px; height: 50px; margin-bottom: 100px; overflow: hidden; opacity: 0.9'>
           <div style='height: 100px; overflow: auto'><div style='height: 300px'></div></div>
         </div>
         <div style='height: 700px'></div>
         <div style='width: 100px; height: 50px; overflow: auto; opacity: 0.9'><div style='height: 300px'></div></div>"
    ));
    let region = page.region(&Default::default()).root;
    assert_eq!(region.len(), 2);
    // Below the outer one, where the inner one is clipped away, a wheel
    // scrolls the viewport, as hit testing finds no container there.
    assert!(!region.iter().any(|r| r.contains(PointF::new(10.0, 70.0))));
    // The container far below the viewport (600 high) is there all the
    // same: the compositor may scroll it into view.
    assert!(region.iter().any(|r| r.contains(PointF::new(10.0, 860.0))));
    page.assert_compositor_agrees_unscrolled(area(0.0, 0.0, 150.0, 950.0), 1.0);
}

#[test]
fn the_viewports_overflow_has_no_container_and_bodys_own_is_one() {
    // `<body>`'s `overflow` goes to the viewport: no container.
    let page = lay_out(
        "<style>html, body { margin: 0 } body { overflow: hidden }</style><div style='height: 1000px'></div>",
    );
    assert!(page.pre(&Default::default()).trees.scroll.is_empty());
    // The root's wins; `<body>` keeps its own and is a container.
    let page = lay_out(
        "<style>html { overflow: auto } body { overflow: auto; margin: 0; height: 100px }</style><div style='height: 1000px'></div>",
    );
    assert_eq!(page.pre(&Default::default()).trees.scroll.len(), 1);
    page.assert_compositor_agrees_unscrolled(area(0.0, 0.0, 820.0, 200.0), 2.0);
}

/// The common case: what comes after the container is below it, so the
/// compositor picks it, and the region is empty.
#[test]
fn a_container_that_is_a_layer_and_not_covered_needs_no_region() {
    let page = lay_out(&format!(
        "{RESET}<p style='font: 20px/1 Ahem'>Before</p>
         <div style='width: 100px; height: 50px; border: 5px solid; overflow: auto'>
           <div style='height: 300px; background-color: blue'>text</div>
         </div>
         <p style='font: 20px/1 Ahem'>After</p>"
    ));
    let region = page.region(&Default::default());
    assert_eq!(region.root, []);
    assert!(region.scrollers.iter().all(Vec::is_empty));
    page.assert_compositor_agrees_unscrolled(area(0.0, 0.0, 200.0, 150.0), 0.5);
}

/// What is painted after a container, outside it, and over it, is what the
/// main thread picks there: it goes to the main thread, whether it paints
/// anything or not.
#[test]
fn a_container_covered_by_what_is_painted_after_it_goes_to_the_main_thread() {
    for after in ["background-color: green", ""] {
        let page = lay_out(&format!(
            "{RESET}<div style='width: 100px; height: 50px; overflow: auto'>
               <div style='height: 300px'></div>
             </div>
             <div style='width: 40px; height: 20px; margin: -10px 0 0 80px; {after}'></div>"
        ));
        assert_eq!(
            page.region(&Default::default()).root,
            [area(-1.0, -1.0, 102.0, 52.0)],
            "{after:?}"
        );
        page.assert_compositor_agrees_unscrolled(area(0.0, 0.0, 150.0, 100.0), 0.5);
    }
}

/// Its own content painted after the rest of the page (a stacking context
/// in it) is in it: what the main thread picks there scrolls it.
#[test]
fn a_containers_own_content_doesnt_cover_it() {
    let page = lay_out(&format!(
        "{RESET}<div style='width: 100px; height: 50px; overflow: auto'>
           <div style='height: 30px; opacity: 0.5; background-color: blue'></div>
           <div style='height: 300px'></div>
         </div>
         <div style='height: 20px'></div>"
    ));
    let region = page.region(&Default::default());
    assert_eq!(region.root, []);
    page.assert_compositor_agrees_unscrolled(area(0.0, 0.0, 150.0, 100.0), 0.5);
}

/// A container with nothing to scroll isn't a layer, and is passed by: the
/// compositor scrolls the one in it, which is a layer.
#[test]
fn a_container_with_nothing_to_scroll_is_left_out() {
    let page = lay_out(&format!(
        "{RESET}<div style='overflow: auto'>
           <div id=inner style='width: 100px; height: 50px; overflow: auto'>
             <div style='height: 300px'></div>
           </div>
         </div>"
    ));
    let region = page.region(&Default::default());
    assert_eq!(region.root, []);
    page.assert_compositor_agrees_unscrolled(area(0.0, 0.0, 150.0, 100.0), 0.5);
}

/// The compositor may scroll the outer container, so what is painted after
/// it may come over the inner one: both go to the main thread, the inner
/// one's rect moving with the outer one.
#[test]
fn what_may_scroll_under_something_painted_after_it_is_covered() {
    let page = lay_out(&format!(
        "{RESET}<div id=outer style='width: 200px; height: 100px; overflow: auto'>
           <div style='width: 100px; height: 40px; overflow: auto'><div style='height: 300px'></div></div>
           <div style='height: 300px'></div>
         </div>
         <div style='width: 50px; height: 30px; margin-top: -40px'></div>"
    ));
    let outer = page.ids["outer"];
    let region = page.region(&Default::default());
    assert_eq!(region.root.len(), 1, "the outer one: {region:?}");
    assert_eq!(region.scrollers[0].len(), 1, "the inner one: {region:?}");
    for scrolled in [0.0, 20.0, 50.0] {
        let scroll: ScrollOffsets = [(outer, PointF::new(0.0, scrolled))].into_iter().collect();
        page.assert_compositor_agrees(
            &Default::default(),
            &scroll,
            area(0.0, 0.0, 250.0, 150.0),
            0.5,
        );
    }
}

/// What is in another container that is a layer, painted after one, may be
/// anywhere in that container once the compositor scrolls it: over the
/// first one, though painted beside it. (The other container's own box
/// isn't found: `pointer-events: none`, its content's `auto`.)
#[test]
fn what_another_layer_may_scroll_over_a_container_covers_it() {
    let page = lay_out(&format!(
        "{RESET}<div style='width: 100px; height: 50px; overflow: auto'>
           <div style='height: 300px'></div>
         </div>
         <div id=other style='width: 100px; height: 100px; margin-top: -40px; overflow: auto; pointer-events: none'>
           <div style='height: 60px'></div>
           <div style='height: 30px; pointer-events: auto'></div>
           <div style='height: 300px'></div>
         </div>"
    ));
    let other = page.ids["other"];
    let region = page.region(&Default::default());
    // The other one too: nothing picks it.
    assert_eq!(
        region.root,
        [area(-1.0, -1.0, 102.0, 52.0), area(-1.0, 9.0, 102.0, 102.0)]
    );
    for scrolled in [0.0, 30.0, 60.0] {
        let scroll: ScrollOffsets = [(other, PointF::new(0.0, scrolled))].into_iter().collect();
        page.assert_compositor_agrees(
            &Default::default(),
            &scroll,
            area(0.0, 0.0, 150.0, 150.0),
            0.5,
        );
    }
}

/// What covers a container may be further in a run of targets that starts
/// beside it: in a stacking context, a box sticking out of it after one
/// that doesn't.
#[test]
fn a_box_further_in_a_run_of_targets_covers_a_container() {
    let page = lay_out(&format!(
        "{RESET}<div style='width: 100px; height: 50px; overflow: auto'>
           <div style='height: 300px'></div>
         </div>
         <div style='display: flow-root; width: 100px; height: 20px; margin-top: 30px; opacity: 0.5'>
           <div style='height: 5px'></div>
           <div style='width: 20px; height: 10px; margin-top: -45px'></div>
         </div>"
    ));
    let region = page.region(&Default::default());
    assert_eq!(region.root, [area(-1.0, -1.0, 102.0, 52.0)]);
    page.assert_compositor_agrees_unscrolled(area(0.0, 0.0, 150.0, 120.0), 0.5);
}

/// A container that isn't a layer is the main thread's only where a wheel
/// can move it: `hidden` axes don't count.
#[test]
fn a_container_the_user_cant_scroll_is_left_out() {
    let page = |overflow: &str, content: &str| {
        lay_out(&format!(
            "{RESET}<div style='width: 100px; height: 50px; opacity: 0.9; {overflow}'>
               <div style='{content}'></div>
             </div>"
        ))
    };
    for (overflow, content, in_region) in [
        ("overflow: hidden", "width: 300px; height: 300px", false),
        (
            "overflow-x: auto; overflow-y: hidden",
            "height: 300px",
            false,
        ),
        (
            "overflow-x: auto; overflow-y: hidden",
            "width: 300px; height: 10px",
            true,
        ),
        (
            "overflow-x: hidden; overflow-y: auto",
            "width: 300px; height: 10px",
            false,
        ),
        (
            "overflow-x: hidden; overflow-y: auto",
            "height: 300px",
            true,
        ),
    ] {
        let page = page(overflow, content);
        assert_eq!(
            page.region(&Default::default()).root.len(),
            usize::from(in_region),
            "{overflow}; {content}"
        );
        page.assert_compositor_agrees_unscrolled(area(0.0, 0.0, 150.0, 100.0), 1.0);
    }
}

/// A container that isn't a layer, below the port of one that is: the
/// compositor scrolls it into view without painting again. Its rect is in
/// the outer one's content space (which its offset moves: painted while
/// scrolled, the rect is where it is unscrolled), not cut by the outer
/// one's clip, which it moves within.
#[test]
fn the_region_below_a_container_layers_port_comes_into_view_with_it() {
    let page = lay_out(&format!(
        "{RESET}<div style='height: 30px'></div>
         <div id=outer style='margin-left: 20px; width: 200px; height: 100px; overflow: auto'>
           <div style='height: 150px'></div>
           <div style='width: 100px; height: 50px; overflow: auto; opacity: 0.9'><div style='height: 300px'></div></div>
           <div style='height: 300px'></div>
         </div>"
    ));
    let outer = page.ids["outer"];
    let at = |y: f32| -> ScrollOffsets { [(outer, PointF::new(0.0, y))].into_iter().collect() };
    for painted in [0.0, 20.0] {
        let region = page.region(&at(painted));
        assert_eq!(region.root, []);
        assert_eq!(region.scrollers[0], [area(19.0, 179.0, 102.0, 52.0)]);
        for scrolled in [0.0, 100.0, 150.0] {
            page.assert_compositor_agrees(
                &at(painted),
                &at(scrolled),
                area(0.0, 0.0, 250.0, 150.0),
                0.5,
            );
        }
    }
}

#[test]
fn flex_items_and_z_index_are_hit_as_painted() {
    let page = lay_out(&format!(
        "{RESET}<div id=c style='display: flex; font: 20px/1 Ahem'>
           <div id=a style='width: 40px; height: 40px; margin-right: -20px'><span id=text>X</span></div>
           <div id=b style='width: 40px; height: 40px'></div>
           <div id=raised style='width: 40px; height: 40px; z-index: 1'></div>
           <div id=over style='width: 40px; height: 40px; margin-left: -20px'></div>
           <div id=sunk style='width: 40px; height: 40px; z-index: -1; margin-left: -10px'></div>
         </div>"
    ));
    // `#b` paints whole after `#a`, its background over `#a`'s text.
    assert_eq!(page.at(25.0, 10.0), Some("b"));
    assert_eq!(page.at(5.0, 10.0), Some("text"));
    // `z-index: 1` over the later item.
    assert_eq!(page.at(70.0, 10.0), Some("raised"));
    assert_eq!(page.at(105.0, 10.0), Some("over"));
    // A negative `z-index` under the earlier item, and under its flex
    // container's border box where nothing else is.
    assert_eq!(page.at(115.0, 10.0), Some("over"));
    assert_eq!(page.at(130.0, 10.0), Some("c"));
}

#[test]
fn a_negative_z_index_is_hit_over_its_stacking_contexts_own_box() {
    let page = lay_out(&format!(
        "{RESET}<div id=root style='display: flex; opacity: 0.5; height: 40px'>
           <div id=negative style='width: 40px; z-index: -1'></div>
         </div>"
    ));
    assert_eq!(page.at(20.0, 20.0), Some("negative"));
    assert_eq!(page.at(60.0, 20.0), Some("root"));
}

/// A flex item is painted whole after the items before it: one that is a
/// container is over those it overlaps, not covered by them.
#[test]
fn a_flex_item_container_is_over_the_items_before_it() {
    let page = lay_out(&format!(
        "{RESET}<div style='display: flex'>
           <div style='width: 60px; height: 50px; margin-right: -20px; background-color: green'></div>
           <div style='width: 100px; height: 50px; overflow: auto'>
             <div style='height: 300px'></div>
           </div>
         </div>"
    ));
    let region = page.region(&Default::default());
    assert_eq!(region.root, []);
    assert!(region.scrollers.iter().all(Vec::is_empty));
    page.assert_compositor_agrees_unscrolled(area(0.0, 0.0, 200.0, 100.0), 0.5);
}

#[test]
fn inline_blocks_are_hit_as_painted() {
    let page = lay_out(&format!(
        "{RESET}<div id=d style='font: 20px/1 Ahem'><span id=ib style='display: inline-block;
           width: 40px; height: 40px; margin-right: -20px'><b id=in>X</b></span><b id=over>YY</b></div>"
    ));
    // Its text, and its own box below it.
    assert_eq!(page.at(5.0, 5.0), Some("in"));
    assert_eq!(page.at(30.0, 30.0), Some("ib"));
    // The text after it on the line is painted over it.
    assert_eq!(page.at(25.0, 5.0), Some("over"));
    // Past it, the block's own box.
    assert_eq!(page.at(50.0, 30.0), Some("d"));
}

#[test]
fn text_after_an_inline_block_is_hit_over_its_text() {
    let page = lay_out(&format!(
        "{RESET}<div style='font: 20px/1 Ahem'><span style='display: inline-block;
           margin-right: -20px'><b id=in>XX</b></span><b id=over>Y</b></div>"
    ));
    // `Y` over the second `X`.
    assert_eq!(page.at(25.0, 5.0), Some("over"));
    assert_eq!(page.at(5.0, 5.0), Some("in"));
}

/// An inline-block is painted whole after the inline element around it: a
/// scroll container that is one is over that element's box, not covered
/// by it.
#[test]
fn an_inline_block_container_is_over_the_inline_box_around_it() {
    let page = lay_out(&format!(
        "{RESET}<div style='font: 20px/1 Ahem'><b style='padding: 20px 0; background-color: blue'>X<span
           style='display: inline-block; width: 100px; height: 50px; overflow: auto'>
           <div style='height: 300px'></div></span></b></div>"
    ));
    let region = page.region(&Default::default());
    assert_eq!(region.root, []);
    assert!(region.scrollers.iter().all(Vec::is_empty));
}

impl Page {
    /// The wheel event handler region painted with the containers scrolled
    /// by `scroll`.
    fn wheel_region(&self, scroll: &ScrollOffsets, whole_document: bool) -> ScrollHitTestRegion {
        let pre = self.pre(scroll);
        let composited = paint::composited_scrollers(&pre.trees);
        let artifact = paint::paint(&self.view, &pre, layout::document_size(&self.view), &());
        paint::wheel_event_handler_region(&artifact, &pre, &composited, whole_document)
    }
}

/// The wheel event handler region: the boxes of the elements in a blocking
/// listener's subtree, made whole px and grown 1px; one in a container
/// that is a layer in that container's content space (where it is
/// unscrolled, whatever it was painted at), not cut by the container's
/// clip, which it moves within; one in a container that isn't a layer cut
/// by its clip; the whole document if the window's or the document's
/// listener is blocking.
#[test]
fn the_wheel_event_handler_region() {
    let page = |listener: &str| {
        let wheel = |id: &str| if id == listener { " data-wheel" } else { "" };
        lay_out(&format!(
            "{RESET}<div id=listener style='height: 50px'{}><div id=child style='width: 100px; height: 20px'></div></div>
             <div id=other style='height: 50px'></div>
             <div style='height: 30px'></div>
             <div id=outer style='margin-left: 20px; width: 200px; height: 100px; overflow: auto'>
               <div style='height: 150px'></div>
               <div id=deep style='width: 100px; height: 50px'{}></div>
               <div style='height: 300px'></div>
             </div>
             <div style='height: 30px; overflow: hidden; opacity: 0.9'><div id=tall style='height: 100px'{}></div></div>",
            wheel("listener"),
            wheel("deep"),
            wheel("tall"),
        ))
    };
    let region = page("listener").wheel_region(&Default::default(), false);
    assert_eq!(
        region.root,
        [area(-1.0, -1.0, 802.0, 52.0), area(-1.0, -1.0, 102.0, 22.0)]
    );
    assert_eq!(region.scrollers, [Vec::<RectF>::new(), Vec::new()]);

    let deep = page("deep");
    let outer = deep.ids["outer"];
    for painted in [0.0, 20.0] {
        let scroll: ScrollOffsets = [(outer, PointF::new(0.0, painted))].into_iter().collect();
        let region = deep.wheel_region(&scroll, false);
        assert_eq!(region.root, []);
        assert_eq!(
            region.scrollers,
            [vec![area(19.0, 279.0, 102.0, 52.0)], Vec::new()]
        );
    }

    // Cut by the clip of a container that isn't a layer.
    let region = page("tall").wheel_region(&Default::default(), false);
    assert_eq!(region.root, [area(-1.0, 229.0, 802.0, 32.0)]);

    let region = page("").wheel_region(&Default::default(), true);
    assert_eq!(region.root.len(), 1);
    assert!(region.root[0].contains(PointF::new(0.0, 100_000.0)));
}

/// An SVG element's targets are in the region as its style says: inside the
/// listener the `<svg>` is in.
#[test]
fn an_svgs_shapes_are_in_the_wheel_event_handler_region() {
    let page = lay_out(&format!(
        "{RESET}<div data-wheel style='width: 20px; height: 20px'>
           <svg width=10 height=10 style='display: block; overflow: visible'><rect x=50 width=10 height=10 /></svg>
         </div>"
    ));
    let region = page.wheel_region(&Default::default(), false);
    assert!(
        region
            .root
            .iter()
            .any(|r| r.contains(PointF::new(55.0, 5.0))),
        "{region:?}"
    );
}

#[test]
fn positioned_boxes_are_hit_as_painted() {
    let page = lay_out(&format!(
        "{RESET}<div style='font: 20px/1 Ahem'>
           <div id=moved style='position: relative; top: 10px; height: 20px'></div>
           <div id=next style='height: 20px'>X</div>
           <div id=low style='position: relative; z-index: 1; height: 20px'></div>
           <div id=high style='position: relative; z-index: 2; height: 20px; margin-top: -10px'></div>
         </div>"
    ));
    // Painted after the text of the box after it, which it moved over.
    assert_eq!(page.at(5.0, 25.0), Some("moved"));
    assert_eq!(page.at(5.0, 35.0), Some("next"));
    // The higher `z-index` over the lower one.
    assert_eq!(page.at(5.0, 55.0), Some("high"));
    assert_eq!(page.at(5.0, 45.0), Some("low"));
}

/// A positioned box is painted after the flow that follows it: a scroll
/// container that is one is over the block after it, not covered by it.
#[test]
fn a_positioned_container_is_over_the_flow_after_it() {
    let page = lay_out(&format!(
        "{RESET}<div style='position: relative; width: 100px; height: 50px; overflow: auto'>
           <div style='height: 300px'></div>
         </div>
         <div style='height: 50px; margin-top: -20px; background-color: green'></div>"
    ));
    let region = page.region(&Default::default());
    assert_eq!(region.root, []);
    assert!(region.scrollers.iter().all(Vec::is_empty), "{region:?}");
}

/// An absolutely positioned box is hit where its containing block placed
/// it, over the flow it was taken out of.
#[test]
fn absolute_boxes_are_hit_where_their_containing_block_puts_them() {
    let page = lay_out(&format!(
        "{RESET}<div id=cb style='position: relative; width: 200px; height: 100px'>
           <div id=flow style='height: 100px'></div>
           <div id=deep>
             <div id=abs style='position: absolute; top: 20px; left: 30px;
               width: 50px; height: 40px'></div>
           </div>
         </div>"
    ));
    // Over the in-flow block that fills the containing block.
    assert_eq!(page.at(40.0, 30.0), Some("abs"));
    assert_eq!(page.at(40.0, 70.0), Some("flow"));
    // Outside it: the box it was taken out of has no say.
    assert_eq!(page.at(20.0, 30.0), Some("flow"));
}

/// A box fixed to the viewport is hit where it shows, however far the
/// document is scrolled: the point comes in the document's coordinates,
/// and the box is painted at its layout position.
#[test]
fn fixed_boxes_are_hit_where_they_show() {
    let page = lay_out(&format!(
        "{RESET}<div id=bar style='position: fixed; top: 0; left: 0; width: 200px;
           height: 30px'></div>
         <div id=flow style='height: 2000px'></div>"
    ));
    let hit = |scroll: PointF, x: f32, y: f32| {
        let pre = paint::pre_paint_frame_tree(
            &page.view,
            &Default::default(),
            &Default::default(),
            &paint::ChildFrames::default(),
            1.0,
            scroll,
            None,
        );
        page.name(paint::hit_test(&page.view, &pre, PointF::new(x, y)).node)
    };
    // Unscrolled: the bar is at the top, the flow under it.
    assert_eq!(hit(PointF::new(0.0, 0.0), 10.0, 10.0), Some("bar"));
    assert_eq!(hit(PointF::new(0.0, 0.0), 10.0, 50.0), Some("flow"));
    // Scrolled by 500: at the top of the viewport (500 in the document)
    // the bar is still what shows.
    assert_eq!(hit(PointF::new(0.0, 500.0), 10.0, 510.0), Some("bar"));
    assert_eq!(hit(PointF::new(0.0, 500.0), 10.0, 10.0), Some("flow"));
}

#[test]
fn rounded_corners_are_not_hit_outside_their_curves() {
    let page = lay_out(&format!(
        "{RESET}<body id=body>
         <div id=card style='width: 100px; height: 100px; border-radius: 50px; overflow: hidden'>
           <div id=big style='width: 100px; height: 100px'></div>
         </div>
         <div id=round style='width: 100px; height: 100px; border-radius: 20px'></div>"
    ));
    // The middle of the circle: the content, which its clip leaves.
    assert_eq!(page.at(50.0, 50.0), Some("big"));
    // Inside the box but past the curve: neither the content, cut by the
    // rounded clip, nor the box itself.
    assert_eq!(page.at(5.0, 5.0), Some("body"));
    assert_eq!(page.at(95.0, 95.0), Some("body"));
    // A box with rounded corners and no clip: its own corners miss too.
    assert_eq!(page.at(2.0, 102.0), Some("body"));
    assert_eq!(page.at(10.0, 110.0), Some("round"));
    assert_eq!(page.at(50.0, 101.0), Some("round"));
}

/// What a shadow tree lays out is hit as its elements; text is hit as its
/// parent in the flat tree: a slot for a host's text child assigned to it,
/// a `display: contents` element for its own.
#[test]
fn shadow_trees_are_hit_along_the_flat_tree() {
    let page = lay_out_with_shadows(
        &format!(
            "{RESET}<style>body {{ font: 20px/1 Ahem }}</style>\
             <div id=host style='width: 100px'>text<b id=light>light</b></div>\
             <div id=outer><span id=contents style='display: contents'>abc</span></div>"
        ),
        &[(
            "host",
            "<div id=inner style='height: 20px'></div><slot id=slot></slot>",
        )],
    );
    assert_eq!(page.at(5.0, 5.0), Some("inner"));
    assert_eq!(page.at(5.0, 25.0), Some("slot"));
    assert_eq!(page.at(85.0, 25.0), Some("light"));
    assert_eq!(page.at(5.0, 45.0), Some("contents"));
}

/// What is in a cell that layout hides (in a collapsed row, or with all its
/// columns collapsed) isn't hit, even made visible again.
#[test]
fn hidden_table_parts_are_not_hit() {
    let page = lay_out(&format!(
        "{RESET}<body id=body><table style='border-spacing: 0'>
           <col style='visibility: collapse'><col>
           <tr><td style='padding: 0'><div id=column style='width: 100px; height: 100px'></div></td>
             <td id=shown style='padding: 0; width: 10px'></td></tr>
           <tr style='visibility: collapse'><td style='padding: 0'><div id=row
             style='visibility: visible; width: 100px; height: 100px'></div></td><td></td></tr>
           </table>
         <div id=next style='height: 300px'></div>"
    ));
    assert_eq!(page.at(5.0, 50.0), Some("shown"));
    assert_eq!(page.at(50.0, 50.0), Some("body"));
    // Below the table, where the collapsed row's cell's block overflows.
    assert_eq!(page.at(50.0, 150.0), Some("next"));
}

/// A sticky box is hit where it stuck: its translation node moves it and
/// all in it, set in place as its scrollport scrolls, the boxes not walked
/// again.
#[test]
fn a_sticky_box_is_hit_where_it_stuck() {
    let page = lay_out(&format!(
        "{RESET}<body id=body>
         <div id=c style='overflow: auto; height: 100px'>
           <div id=before style='height: 200px'></div>
           <div id=s style='position: sticky; top: 0; height: 20px'><div id=in style='width: 10px; height: 10px'></div></div>
           <div id=after style='height: 1000px'></div>
         </div>
         <div style='height: 2000px'></div>"
    ));
    let positions = layout::StickyPositions::new(&page.view, VIEWPORT);
    let pre_paint = |y: f32, previous: Option<PrePaint>| {
        let mut scroll = ScrollOffsets::default();
        scroll.insert(page.ids["c"], PointF::new(0.0, y));
        let sticky = layout::StickyState {
            offsets: positions.offsets(&scroll, PointF::new(0.0, 0.0)),
            reaches: positions.reaches(),
        };
        paint::pre_paint_frame_tree(
            &page.view,
            &scroll,
            &sticky,
            &Default::default(),
            1.0,
            PointF::new(0.0, 0.0),
            previous,
        )
    };
    let pre = pre_paint(300.0, None);
    let at = |pre: &PrePaint, x: f32, y: f32| {
        page.name(paint::hit_test(&page.view, pre, PointF::new(x, y)).node)
    };
    // Scrolled 300 in its container: stuck at the top, over what
    // scrolled under it, with what is in it.
    assert_eq!(at(&pre, 5.0, 5.0), Some("in"));
    assert_eq!(at(&pre, 50.0, 15.0), Some("s"));
    assert_eq!(at(&pre, 50.0, 25.0), Some("after"));
    // Scrolled back: where it was laid out, after `before`, which is
    // all there is at the top; then, short of the top, where the flow put
    // it.
    let pre = pre_paint(0.0, Some(pre));
    assert_eq!(pre.walked_for_testing(), 0);
    assert_eq!(at(&pre, 50.0, 15.0), Some("before"));
    let pre = pre_paint(190.0, Some(pre));
    assert_eq!(at(&pre, 5.0, 15.0), Some("in"));
    assert_eq!(at(&pre, 50.0, 5.0), Some("before"));
    let pre = pre_paint(300.0, Some(pre));
    assert_eq!(at(&pre, 5.0, 5.0), Some("in"));
}

impl Page {
    /// The two regions as painted with the viewport scrolled `y` down and
    /// the sticky boxes where they stick then.
    fn stuck_regions(&self, y: f32) -> (ScrollHitTestRegion, ScrollHitTestRegion) {
        let positions = layout::StickyPositions::new(&self.view, VIEWPORT);
        let stuck = layout::StickyState {
            offsets: positions.offsets(&Default::default(), PointF::new(0.0, y)),
            reaches: positions.reaches(),
        };
        let pre = paint::pre_paint_frame_tree(
            &self.view,
            &Default::default(),
            &stuck,
            &Default::default(),
            1.0,
            PointF::new(0.0, y),
            None,
        );
        let composited = paint::composited_scrollers(&pre.trees);
        let artifact = paint::paint(&self.view, &pre, layout::document_size(&self.view), &());
        (
            paint::main_thread_scroll_hit_test_region(&artifact, &pre, &composited),
            paint::wheel_event_handler_region(&artifact, &pre, &composited, false),
        )
    }
}

/// What a sticky box the compositor moves has in it is in the regions in
/// its translation's space, where it was laid out, however far it stuck
/// when painted: a container in it that isn't a layer, and what is in a
/// blocking `wheel` listener.
#[test]
fn a_sticky_boxs_regions_are_in_its_space() {
    let page = lay_out(&format!(
        "{RESET}<div style='height: 100px'></div>
         <div style='position: sticky; top: 0; height: 60px'>
           <div style='margin-left: 10px; width: 50px; height: 40px; overflow: auto; opacity: 0.5'>
             <div style='height: 100px'></div>
           </div>
           <div data-wheel style='margin-left: 100px; width: 20px; height: 20px'></div>
         </div>
         <div style='height: 2000px'></div>"
    ));
    for y in [0.0, 500.0] {
        let (region, wheel) = page.stuck_regions(y);
        assert_eq!(region.root, [], "scrolled {y}");
        assert_eq!(region.sticky.len(), 1, "scrolled {y}");
        // At 10, 100 (the box laid out at 100, its 1px grown around).
        assert_eq!(
            region.sticky[0].1,
            [RectF::new(9.0, 99.0, 52.0, 42.0)],
            "scrolled {y}"
        );
        assert_eq!(wheel.root, [], "scrolled {y}");
        assert_eq!(
            wheel.sticky,
            [(
                region.sticky[0].0,
                vec![RectF::new(99.0, 139.0, 22.0, 22.0)]
            )]
        );
    }
}

/// A sticky box painted after a container that is a layer, which it may
/// stick over as the viewport scrolls: what it has for hit testing is the
/// main thread's, where it is (in its space), rather than all of the
/// container. One that can't reach the container leaves it alone.
#[test]
fn a_sticky_box_over_a_container_is_the_main_threads() {
    let page = |wrapped: bool| {
        let sticky = "<div style='position: sticky; top: 0; height: 20px; width: 200px'></div>";
        let sticky = if wrapped {
            format!("<div style='height: 50px'>{sticky}</div>")
        } else {
            sticky.to_string()
        };
        lay_out(&format!(
            "{RESET}{sticky}
             <div style='height: 100px; width: 100px; overflow: auto'><div style='height: 300px'></div></div>
             <div style='height: 2000px'></div>"
        ))
    };
    // In the body, it sticks over the container as the page scrolls past.
    let (region, _) = page(false).stuck_regions(0.0);
    assert_eq!(region.scrollers, [Vec::<RectF>::new()], "not covered");
    assert_eq!(region.sticky.len(), 1);
    assert_eq!(region.sticky[0].1, [RectF::new(-1.0, -1.0, 202.0, 22.0)]);
    // In a 50px block above the container, it stops there.
    let (region, _) = page(true).stuck_regions(0.0);
    assert_eq!(region.scrollers, [Vec::<RectF>::new()]);
    assert_eq!(region.sticky, []);
}
