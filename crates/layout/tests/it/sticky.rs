//! `position: sticky`: laid out where the flow puts it, then moved into
//! its scrollport by its insets as that scrolls, as far as its containing
//! block lets it (Blink's `StickyPositionScrollingConstraints`), and
//! script's geometry with it.

use std::sync::Arc;

use gfx::{NodeId, PointF, RectF};
use layout::{BoxFragment, ScrollOffsets, StickyOffsets, StickyPositions};

use crate::page::VIEWPORT;

/// A page laid out, and where its sticky boxes are.
struct Laid {
    view: Arc<BoxFragment>,
    laid: crate::page::Laid,
    sticky: StickyPositions,
}

fn lay_out(html: &str) -> Laid {
    let laid = crate::page::lay_out(html);
    let sticky = StickyPositions::new(&laid.view, VIEWPORT);
    Laid {
        view: laid.view.clone(),
        laid,
        sticky,
    }
}

impl Laid {
    fn id(&self, name: &str) -> NodeId {
        self.laid.node(name)
    }

    /// How far `name` moves with the viewport at `viewport` and the
    /// containers at `containers` (by `id`).
    fn offset(&self, name: &str, viewport: f32, containers: &[(&str, f32)]) -> PointF {
        let offsets = self.offsets(viewport, containers);
        offsets
            .get(&(self.id(name), None))
            .copied()
            .unwrap_or_else(|| panic!("#{name} sticks"))
    }

    fn offsets(&self, viewport: f32, containers: &[(&str, f32)]) -> StickyOffsets {
        self.sticky
            .offsets(&self.scroll(containers), PointF::new(0.0, viewport))
    }

    fn scroll(&self, containers: &[(&str, f32)]) -> ScrollOffsets {
        containers
            .iter()
            .map(|(name, y)| (self.id(name), PointF::new(0.0, *y)))
            .collect()
    }
}

/// A box 50px tall at y = 200 in a 1000px containing block, `top: 10px`.
const TOP: &str = r#"<body style="margin: 0">
    <div id=block style="height: 1000px">
        <div style="height: 200px"></div>
        <div id=s style="position: sticky; top: 10px; height: 50px"></div>
    </div>
    <div style="height: 2000px"></div>"#;

#[test]
fn a_box_sticks_to_the_top_of_the_viewport_until_its_containing_block_ends() {
    let laid = lay_out(TOP);
    // In view below the top: where the flow put it.
    assert_eq!(laid.offset("s", 0.0, &[]), PointF::new(0.0, 0.0));
    assert_eq!(laid.offset("s", 190.0, &[]), PointF::new(0.0, 0.0));
    // Scrolled past: 10px below the viewport's top.
    assert_eq!(laid.offset("s", 250.0, &[]), PointF::new(0.0, 60.0));
    // No further than its containing block's end, 1000px.
    assert_eq!(laid.offset("s", 980.0, &[]), PointF::new(0.0, 750.0));
    assert_eq!(laid.offset("s", 1500.0, &[]), PointF::new(0.0, 750.0));
}

#[test]
fn a_box_sticks_to_the_bottom_until_its_containing_block_starts() {
    let laid = lay_out(
        r#"<body style="margin: 0">
        <div style="height: 300px"></div>
        <div id=block style="height: 600px">
            <div style="height: 500px"></div>
            <div id=s style="position: sticky; bottom: 10px; height: 50px"></div>
        </div>
        <div style="height: 2000px"></div>"#,
    );
    // At 800 to 850, below a viewport ending at 600: up to 590.
    assert_eq!(laid.offset("s", 0.0, &[]), PointF::new(0.0, -260.0));
    // No higher than its containing block's top, 300.
    assert_eq!(laid.offset("s", -400.0, &[]), PointF::new(0.0, -500.0));
    // In view: where it is.
    assert_eq!(laid.offset("s", 300.0, &[]), PointF::new(0.0, 0.0));
}

#[test]
fn a_box_sticks_sideways_by_left_and_right() {
    let laid = lay_out(
        r#"<body style="margin: 0">
        <div id=c style="overflow: auto; width: 200px; height: 100px">
            <div style="width: 1000px; display: flex">
                <div style="flex: none; width: 300px"></div>
                <div id=l style="position: sticky; left: 5px; flex: none; width: 50px"></div>
                <div style="flex: none; width: 400px"></div>
                <div id=r style="position: sticky; right: 5px; flex: none; width: 50px"></div>
            </div>
        </div>"#,
    );
    let at = |name: &str, x: f32| {
        let mut scroll = ScrollOffsets::default();
        scroll.insert(laid.id("c"), PointF::new(x, 0.0));
        laid.sticky.offsets(&scroll, PointF::new(0.0, 0.0))[&(laid.id(name), None)]
    };
    // `l` at 300: scrolled 400 across, it stays 5px from the left.
    assert_eq!(at("l", 0.0), PointF::new(0.0, 0.0));
    assert_eq!(at("l", 400.0), PointF::new(105.0, 0.0));
    // `r` at 750 to 800, past the scrollport's right edge at 200.
    assert_eq!(at("r", 0.0), PointF::new(-605.0, 0.0));
    assert_eq!(at("r", 700.0), PointF::new(0.0, 0.0));
}

#[test]
fn a_scroll_containers_scrollport_less_its_padding_is_what_a_box_sticks_in() {
    let laid = lay_out(
        r#"<body style="margin: 0">
        <div style="height: 50px"></div>
        <div id=c style="overflow: auto; height: 100px; border: 5px solid; padding: 10px">
            <div style="height: 100px"></div>
            <div id=s style="position: sticky; top: 3px; height: 20px"></div>
            <div style="height: 1000px"></div>
        </div>
        <div style="height: 2000px"></div>"#,
    );
    let c = &laid.sticky.boxes[0];
    assert_eq!(c.scroller, Some(laid.id("c")));
    // In the container's content space: from its padding box's origin.
    assert_eq!(c.constraining_rect, RectF::new(10.0, 10.0, 770.0, 100.0));
    assert_eq!(c.sticky_box, RectF::new(10.0, 110.0, 770.0, 20.0));
    // The container is its containing block: what scrolls in it, less its
    // padding.
    assert_eq!(c.containing_block, RectF::new(10.0, 10.0, 770.0, 1120.0));
    // The viewport's scrolling doesn't move it in its container.
    assert_eq!(laid.offset("s", 300.0, &[]), PointF::new(0.0, 0.0));
    // Its container's does: 10 + 3 below the scrollport's top.
    assert_eq!(
        laid.offset("s", 0.0, &[("c", 200.0)]),
        PointF::new(0.0, 103.0)
    );
}

#[test]
fn its_margins_keep_it_off_its_containing_blocks_edges() {
    let laid = lay_out(
        r#"<body style="margin: 0">
        <div id=block style="height: 500px; padding-bottom: 20px">
            <div id=s style="position: sticky; top: 0; height: 50px; margin: 0 auto 30px; width: 100px"></div>
        </div>
        <div style="height: 2000px"></div>"#,
    );
    // Its containing block's content box (500px), less its bottom margin;
    // `auto` margins are 0.
    assert_eq!(
        laid.sticky.boxes[0].containing_block,
        RectF::new(0.0, 0.0, 800.0, 470.0)
    );
    assert_eq!(laid.offset("s", 1000.0, &[]), PointF::new(0.0, 420.0));
}

#[test]
fn percentages_are_of_the_scrollport_and_the_end_inset_gives_way() {
    let laid = lay_out(
        r#"<body style="margin: 0">
        <div id=c style="overflow: auto; height: 200px; width: 400px">
            <div id=s style="position: sticky; top: 10%; bottom: 80%; left: 50%; height: 50px"></div>
            <div style="height: 1000px"></div>
        </div>"#,
    );
    let c = &laid.sticky.boxes[0];
    // 10% and 80% of 200px leave 20px for a 50px box: the bottom gives
    // up 30px of its 160.
    assert_eq!(
        c.resolved_inset,
        [Some(20.0), None, Some(160.0), Some(200.0)]
    );
    assert_eq!(c.inset, [Some(20.0), None, Some(130.0), Some(200.0)]);
}

#[test]
fn a_box_in_a_sticky_box_moves_with_its_containing_block() {
    let laid = lay_out(
        r#"<body style="margin: 0">
        <div style="height: 100px"></div>
        <div id=outer style="position: sticky; top: 0; height: 300px">
            <div style="height: 100px"></div>
            <div id=inner style="position: sticky; top: 20px; height: 50px"></div>
        </div>
        <div style="height: 2000px"></div>"#,
    );
    let [outer, inner] = &laid.sticky.boxes[..] else {
        panic!("two sticky boxes");
    };
    assert_eq!(outer.shifting_containing_block, None);
    assert_eq!(inner.shifting_containing_block, Some(0));
    // Scrolled 250: the outer box sticks 150 down, at 250 to 550; the
    // inner one, at 350 in it, is already 20 below the top.
    assert_eq!(laid.offset("outer", 250.0, &[]), PointF::new(0.0, 150.0));
    assert_eq!(laid.offset("inner", 250.0, &[]), PointF::new(0.0, 0.0));
    // Scrolled 2400: the outer box stops at its containing block's (the
    // body's) end, at 2100 to 2400; the inner one, at 2200 in it, would
    // go to 2420 but stops at the outer box's end.
    let offsets = laid.offsets(2400.0, &[]);
    assert_eq!(offsets[&(laid.id("outer"), None)], PointF::new(0.0, 2000.0));
    assert_eq!(offsets[&(laid.id("inner"), None)], PointF::new(0.0, 150.0));
}

#[test]
fn a_box_in_a_fixed_box_doesnt_move_with_the_viewport() {
    let laid = lay_out(
        r#"<body style="margin: 0">
        <div style="position: fixed; top: 0; height: 300px; width: 100px">
            <div style="height: 700px"></div>
            <div id=s style="position: sticky; bottom: 0; height: 50px"></div>
        </div>
        <div style="height: 2000px"></div>"#,
    );
    assert!(laid.sticky.boxes[0].fixed_to_view);
    // Under the viewport's bottom however far the document is scrolled:
    // up to it, but no higher than its containing block lets it.
    assert_eq!(laid.offset("s", 0.0, &[]), PointF::new(0.0, -150.0));
    assert_eq!(laid.offset("s", 500.0, &[]), PointF::new(0.0, -150.0));
}

#[test]
fn only_boxes_with_an_inset_and_outside_tables_stick() {
    let laid = lay_out(
        r#"<body style="margin: 0">
        <div id=auto style="position: sticky; height: 10px"></div>
        <table><tr id=row style="position: sticky; top: 0"><td id=cell style="position: sticky; top: 0">x</td></tr></table>
        <div id=wrap><span id=inline style="position: sticky; top: 0">x</span>
        <span id=ib style="position: sticky; top: 0; display: inline-block">x</span></div>"#,
    );
    let stuck: Vec<NodeId> = laid.sticky.boxes.iter().map(|c| c.key.0).collect();
    assert_eq!(stuck, [laid.id("ib")]);
    // Found under the boxes that say so.
    assert!(laid.view.sticky_below);
    let empty = lay_out(r#"<div style="position: sticky; height: 10px"></div>"#);
    assert!(!empty.view.sticky_below);
    assert!(empty.sticky.is_empty());
}

#[test]
fn an_atomic_inlines_containing_block_is_the_block_around_its_line() {
    let laid = lay_out(
        r#"<body style="margin: 0">
        <div id=block style="height: 400px">text
            <span id=s style="position: sticky; top: 0; display: inline-block; height: 20px">x</span>
            <div></div>
        </div>
        <div style="height: 2000px"></div>"#,
    );
    // Not the anonymous block its line is in, but the block around it.
    let c = &laid.sticky.boxes[0];
    assert_eq!(c.containing_block, RectF::new(0.0, 0.0, 800.0, 400.0));
}

#[test]
fn script_sees_a_sticky_box_where_it_stuck() {
    let laid = lay_out(TOP);
    let scroll = ScrollOffsets::default();
    let sticky = laid.offsets(250.0, &[]);
    // The document's coordinates: the viewport's offset isn't taken off.
    assert_eq!(
        layout::client_rects(&laid.view, &scroll, &sticky, laid.id("s")),
        [RectF::new(0.0, 260.0, 800.0, 50.0)]
    );
    let element_box = layout::element_box(&laid.view, &scroll, &sticky, laid.id("s")).unwrap();
    assert_eq!(element_box.border_box, RectF::new(0.0, 260.0, 800.0, 50.0));
    // Unstuck: where the flow put it.
    assert_eq!(
        layout::client_rects(&laid.view, &scroll, &Default::default(), laid.id("s")),
        [RectF::new(0.0, 200.0, 800.0, 50.0)]
    );
}

#[test]
fn a_box_in_a_scroll_container_sticks_in_it_alone() {
    let laid = lay_out(
        r#"<body style="margin: 0">
        <div style="height: 100px"></div>
        <div id=outer style="position: sticky; top: 0; height: 300px">
            <div id=c style="overflow: auto; height: 100px">
                <div id=inner style="position: sticky; top: 0; height: 20px"></div>
                <div style="height: 500px"></div>
            </div>
        </div>
        <div style="height: 2000px"></div>"#,
    );
    let [outer, inner] = &laid.sticky.boxes[..] else {
        panic!("two sticky boxes");
    };
    assert_eq!(outer.scroller, None);
    assert_eq!(inner.scroller, Some(laid.id("c")));
    // The outer box moves the container, and the inner box with it, but
    // not in it.
    assert_eq!(inner.shifting_containing_block, None);
    let offsets = laid.offsets(150.0, &[]);
    assert_eq!(offsets[&(laid.id("outer"), None)], PointF::new(0.0, 50.0));
    assert_eq!(offsets[&(laid.id("inner"), None)], PointF::new(0.0, 0.0));
    assert_eq!(
        laid.offset("inner", 150.0, &[("c", 30.0)]),
        PointF::new(0.0, 30.0)
    );
}

#[test]
fn three_sticky_boxes_deep_each_moves_with_those_around_it() {
    let laid = lay_out(
        r#"<body style="margin: 0">
        <div style="height: 100px"></div>
        <div id=a style="position: sticky; top: 0; height: 600px">
            <div style="height: 100px"></div>
            <div id=b style="position: sticky; top: 0; height: 300px">
                <div style="height: 100px"></div>
                <div id=c style="position: sticky; top: 0; height: 50px"></div>
            </div>
        </div>
        <div style="height: 2000px"></div>"#,
    );
    let at = |scroll: f32| {
        let offsets = laid.offsets(scroll, &[]);
        ["a", "b", "c"].map(|name| offsets[&(laid.id(name), None)].y)
    };
    // Scrolled 400: `a` sticks, moving `b` and `c` with it: they are at
    // or below the top already.
    assert_eq!(at(400.0), [300.0, 0.0, 0.0]);
    // Scrolled 2500: `a` stops at the body's end; `b` sticks in it to its
    // end, and `c` stays at the top with both.
    assert_eq!(at(2500.0), [2000.0, 200.0, 0.0]);
}
