//! Scroll anchoring's geometry (`find_scroll_anchor`,
//! `scroll_anchor_offset`): which box or text in a scrolled viewport or
//! container is its anchor (Blink's `FindAnchorRecursive` and `Examine`),
//! and how far its top is below the top of what the scroller shows. With
//! Ahem, every glyph a 20px square.

use std::collections::BTreeMap;
use std::sync::Arc;

use dom::NodeKind;
use gfx::{NodeId, PointF, RectF};
use layout::{
    BoxFragment, ScrollAnchorNode, ScrollOffsets, find_scroll_anchor, scroll_anchor_offset,
};

use crate::page::{Page, VIEWPORT, each_node};

const STYLE: &str =
    "<style>html, body { margin: 0 } body { font: 20px/1 Ahem } div { height: 100px }</style>";

struct Laid {
    view: Arc<BoxFragment>,
    /// Each element with an `id` by it, and its first text node by
    /// `"<id> text"`.
    ids: BTreeMap<String, NodeId>,
}

fn lay_out(html: &str) -> Laid {
    let mut page = Page::build(&format!("{STYLE}{html}"));
    let ids = page.read(|cx, document| {
        let mut ids = BTreeMap::new();
        each_node(cx, document, &mut |node| {
            if cx.kind(node) == NodeKind::Element
                && let Some(id) = cx.get_attribute(node, "id")
            {
                let mut child = cx.first_child(node);
                while let Some(c) = child.filter(|c| cx.kind(*c) != NodeKind::Text) {
                    child = cx.next_sibling(c);
                }
                if let Some(text) = child {
                    ids.insert(format!("{id} text"), cx.node_id(text));
                }
                ids.insert(id.to_string(), cx.node_id(node));
            }
        });
        ids
    });
    Laid {
        view: page.tree.layout(VIEWPORT),
        ids,
    }
}

impl Laid {
    fn name(&self, anchor: ScrollAnchorNode) -> String {
        let name = self
            .ids
            .iter()
            .find(|(_, id)| **id == anchor.node())
            .map(|(name, _)| name.clone())
            .unwrap_or_else(|| "?".into());
        match anchor {
            ScrollAnchorNode::Box(_) => name,
            ScrollAnchorNode::Text(_) => format!("{name} (text)"),
        }
    }

    fn scroll(&self, containers: &[(&str, f32)]) -> ScrollOffsets {
        containers
            .iter()
            .map(|(name, y)| (self.ids[*name], PointF::new(0.0, *y)))
            .collect()
    }

    /// The viewport's anchor at `y`, and its offset.
    fn viewport_anchor(&self, y: f32) -> Option<(String, f32)> {
        find_scroll_anchor(
            &self.view,
            &ScrollOffsets::default(),
            &Default::default(),
            None,
            RectF::new(0.0, y, VIEWPORT.width, VIEWPORT.height),
            &|_| false,
            &[],
        )
        .map(|(anchor, offset)| (self.name(anchor), offset))
    }

    /// Container `name`'s anchor at `y` (the viewport at the top).
    fn container_anchor(&self, name: &str, y: f32) -> Option<(String, f32)> {
        find_scroll_anchor(
            &self.view,
            &self.scroll(&[(name, y)]),
            &Default::default(),
            Some(self.ids[name]),
            RectF::new(0.0, 0.0, VIEWPORT.width, VIEWPORT.height),
            &|_| false,
            &[],
        )
        .map(|(anchor, offset)| (self.name(anchor), offset))
    }
}

fn some(name: &str, offset: f32) -> Option<(String, f32)> {
    Some((name.into(), offset))
}

/// The first box all in view is it; one partly in view with nothing all
/// in view in it is it, above the top by as much as is hidden; one wholly
/// above isn't looked at, even touching the top.
#[test]
fn the_first_box_in_view() {
    let laid = lay_out(
        "<div id=a></div><div id=b></div><div id=c></div><div style='height: 2000px'></div>",
    );
    assert_eq!(laid.viewport_anchor(100.0), some("b", 0.0));
    assert_eq!(laid.viewport_anchor(150.0), some("b", -50.0));
    assert_eq!(laid.viewport_anchor(50.0), some("a", -50.0));
}

/// A box partly in view gives way to one in it all in view; what comes
/// after it isn't looked at.
#[test]
fn what_is_all_in_view_in_a_box_partly_in_view() {
    let laid = lay_out(
        "<div id=a style='height: 300px'><div id=a1 style='height: 120px'></div>\
         <div id=a2 style='height: 50px'></div></div>\
         <div id=b></div><div style='height: 2000px'></div>",
    );
    // `a1` partly in view: it, though `a2` is all in view.
    assert_eq!(laid.viewport_anchor(100.0), some("a1", -100.0));
    assert_eq!(laid.viewport_anchor(130.0), some("a2", -10.0));
    // A box all in view is it, not what is in it.
    let whole = lay_out(
        "<div style='height: 50px'></div><div id=a style='height: auto'>\
         <div id=a1 style='height: 50px'></div></div><div style='height: 2000px'></div>",
    );
    assert_eq!(whole.viewport_anchor(50.0), some("a", 0.0));
    // `a` partly in view, nothing in it in view: `a`.
    assert_eq!(laid.viewport_anchor(200.0), some("a", -200.0));
}

/// `overflow-anchor: none`, fixed and sticky boxes, and boxes with no
/// height aren't anchors, nor is what is in them.
#[test]
fn what_is_never_an_anchor() {
    for (skipped, b) in [
        ("overflow-anchor: none", 90.0),
        ("position: sticky; top: 0", 90.0),
        // Out of the flow: `b` is at the top.
        ("position: fixed; top: 0; width: 100px", -10.0),
    ] {
        let laid = lay_out(&format!(
            "<div id=a style='{skipped}'><div id=a1 style='height: 50px'></div></div>\
             <div id=b></div><div style='height: 2000px'></div>"
        ));
        assert_eq!(laid.viewport_anchor(10.0), some("b", b), "{skipped}");
    }
    // A fixed box isn't, though a transform contains it.
    let laid = lay_out(
        "<div id=t style='transform: translateX(0); height: 200px'>\
         <div id=f style='position: fixed; top: 50px; width: 10px; height: 10px'></div>\
         <div id=g></div></div><div style='height: 2000px'></div>",
    );
    assert_eq!(laid.viewport_anchor(10.0), some("g", -10.0));
    let laid = lay_out(
        "<div id=a style='height: 0'></div><div id=b></div><div style='height: 2000px'></div>",
    );
    assert_eq!(laid.viewport_anchor(10.0), some("b", -10.0));
    // One with no height in view, all of it (none).
    let laid = lay_out(
        "<div style='height: 50px'></div><div id=z style='height: 0'></div><div id=b></div>\
         <div style='height: 2000px'></div>",
    );
    assert_eq!(laid.viewport_anchor(50.0), some("b", 0.0));
    // `overflow-anchor` isn't inherited, but what is in a box with `none`
    // isn't looked at.
    let laid = lay_out(
        "<div id=a style='overflow-anchor: none; height: 200px'>\
         <div id=a1 style='overflow-anchor: auto'></div></div><div id=c style='height: 2000px'></div>",
    );
    assert_eq!(laid.viewport_anchor(10.0), some("c", 190.0));
}

/// An anonymous block is gone through to what is in it: text next to a
/// block.
#[test]
fn an_anonymous_block_is_gone_through() {
    let laid = lay_out(
        "<div id=a style='height: auto'>XX<div id=b style='height: 20px'></div></div>\
         <div style='height: 2000px'></div>",
    );
    assert_eq!(laid.viewport_anchor(5.0), some("a text (text)", -5.0));
}

/// A box is where its transform and those around it put it.
#[test]
fn a_transformed_box() {
    let laid = lay_out(
        "<div id=t style='transform: translateY(100px)'><div id=a></div></div>\
         <div id=f style='height: 2000px'></div>",
    );
    assert_eq!(laid.viewport_anchor(150.0), some("a", -50.0));
}

/// A positioned box is a candidate where it is; an absolute one is one
/// where its containing block places it.
#[test]
fn an_absolute_box_is_one() {
    let laid = lay_out(
        "<div id=a style='position: absolute; top: 300px; width: 10px; height: 10px'></div>\
         <div id=b style='height: 250px'></div><div style='height: 2000px'></div>",
    );
    assert_eq!(laid.viewport_anchor(260.0), some("a", 40.0));
    // One whose parent isn't looked at (no height), taken up by its
    // containing block after what is in the flow (Blink's
    // `FindAnchorInOOFs`).
    let laid = lay_out(
        "<div id=r style='position: relative; height: 1000px'><div id=p style='height: 0'>\
         <div id=x style='position: absolute; top: 300px; width: 10px; height: 10px'></div>\
         </div></div><div style='height: 2000px'></div>",
    );
    assert_eq!(laid.viewport_anchor(100.0), some("x", 200.0));
}

/// A box that doesn't clip what overflows it reaches down to where that
/// ends (Blink's `RelativeBounds`): above the top, it is still partly in
/// view, and what overflows into view is the anchor.
#[test]
fn a_box_reaches_as_far_as_what_overflows_it() {
    let laid = lay_out(
        "<div id=a style='height: 50px'><div id=a1 style='height: 200px'></div></div>\
         <div style='height: 2000px'></div>",
    );
    assert_eq!(laid.viewport_anchor(100.0), some("a1", -100.0));
    let laid = lay_out(
        "<div id=a style='height: 50px; overflow: hidden'><div style='height: 200px'></div></div>\
         <div id=b></div><div style='height: 2000px'></div>",
    );
    assert_eq!(laid.viewport_anchor(100.0), some("b", -50.0));
}

/// Text is an anchor as a box is, as one rect around its lines; an inline
/// element is gone through to its text, which `overflow-anchor: none` on
/// it skips.
#[test]
fn text_is_an_anchor() {
    let laid = lay_out(
        "<div id=a style='height: auto; width: 60px'>XX XX XX XX</div>\
         <div style='height: 2000px'></div>",
    );
    // Four lines, 0 to 80: partly in view.
    assert_eq!(laid.viewport_anchor(30.0), some("a text (text)", -30.0));
    let laid = lay_out(
        "<div id=a style='height: auto'><span id=s>XX</span><br>\
         <span id=t>XX</span></div><div style='height: 2000px'></div>",
    );
    assert_eq!(laid.viewport_anchor(10.0), some("s text (text)", -10.0));
    assert_eq!(laid.viewport_anchor(20.0), some("t text (text)", 0.0));
    let laid = lay_out(
        "<div id=a style='height: auto'><span id=s style='overflow-anchor: none'>XX</span><br>\
         <span id=t>XX</span></div><div style='height: 2000px'></div>",
    );
    assert_eq!(laid.viewport_anchor(10.0), some("t text (text)", 10.0));
}

/// What is on lines is taken line by line, then left to right, though an
/// inline-block on the baseline reaches above the line's text.
#[test]
fn a_line_top_to_bottom_then_left_to_right() {
    let laid = lay_out(
        "<div id=a style='height: auto'>XX<span id=i style='display: inline-block; \
         width: 20px; height: 20px'></span><br>YY</div><div style='height: 2000px'></div>",
    );
    // The text on the line under the inline-block's top: 4px down.
    assert_eq!(laid.viewport_anchor(5.0), some("a text (text)", -1.0));
    let laid = lay_out(
        "<div id=a style='height: auto'><span id=i style='display: inline-block; \
         width: 20px; height: 20px'></span>XX</div><div style='height: 2000px'></div>",
    );
    assert_eq!(laid.viewport_anchor(5.0), some("i", -5.0));
}

/// What is in another scroll container is looked at where it scrolled
/// to, though it clips it (Blink's search doesn't stop there); the
/// container is the anchor if nothing in it is in view. In a container,
/// what it shows of its content counts, wherever the container is.
#[test]
fn a_scroll_container() {
    let html = "<div id=pad style='height: 50px'></div>\
                <div id=s style='height: 200px; overflow: auto'>\
                <div id=s1></div><div id=s2></div><div id=s3></div></div>\
                <div style='height: 2000px'></div>";
    let laid = lay_out(html);
    // `s1` from 50 to 150, partly in view (and partly hidden by `s`).
    assert_eq!(laid.viewport_anchor(100.0), some("s1", -50.0));
    let empty = lay_out(&html.replace("<div id=s1></div><div id=s2></div><div id=s3></div>", ""));
    assert_eq!(empty.viewport_anchor(100.0), some("s", -50.0));
    // At 150 in it, `s2` shows from 100 - 150 = -50 below its top.
    assert_eq!(laid.container_anchor("s", 150.0), some("s2", -50.0));
    assert_eq!(laid.container_anchor("s", 100.0), some("s2", 0.0));
    // What it shows is inside its border.
    let laid = lay_out(&html.replace("overflow: auto", "overflow: auto; border-top: 10px solid"));
    assert_eq!(laid.container_anchor("s", 150.0), some("s2", -50.0));
}

/// The offset now: where the anchor's top is below the top of what the
/// scroller shows; text from its first line; `None` for a node without a
/// box.
#[test]
fn the_offset_of_an_anchor() {
    let laid = lay_out(
        "<div id=a></div><div id=b style='height: auto'>XX</div>\
         <div id=n style='display: none'></div><div style='height: 2000px'></div>",
    );
    let offset = |anchor, y: f32| {
        scroll_anchor_offset(
            &laid.view,
            &ScrollOffsets::default(),
            &Default::default(),
            None,
            RectF::new(0.0, y, VIEWPORT.width, VIEWPORT.height),
            anchor,
        )
    };
    assert_eq!(
        offset(ScrollAnchorNode::Box(laid.ids["b"]), 30.0),
        Some(70.0)
    );
    assert_eq!(
        offset(ScrollAnchorNode::Text(laid.ids["b text"]), 130.0),
        Some(-30.0)
    );
    assert_eq!(offset(ScrollAnchorNode::Box(laid.ids["n"]), 30.0), None);
}
