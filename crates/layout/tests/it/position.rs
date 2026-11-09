//! `position: relative`: boxes, atomic inlines and inline elements moved
//! by `top` / `right` / `bottom` / `left` after layout, the rest laid out
//! as if they hadn't moved.

use gfx::{PointF, RectF};
use layout::BoxFragment;

use crate::page::{Laid, lay_out};

impl Laid {
    /// The first line of `id`'s content: its runs' x, baselines, and its
    /// inline boxes' rects, in LayoutView coordinates.
    fn first_line(&self, id: &str) -> (Vec<f32>, Vec<f32>, Vec<RectF>) {
        let origin = self.origin(id);
        let line = &self.fragment(id).lines[0];
        (
            line.runs.iter().map(|r| origin.x + r.x).collect(),
            line.runs.iter().map(|r| origin.y + r.baseline).collect(),
            line.boxes
                .iter()
                .map(|b| b.rect.offset(origin.x, origin.y))
                .collect(),
        )
    }
}

/// A rect's top left corner.
fn at(r: RectF) -> (f32, f32) {
    (r.x, r.y)
}

const AHEM: &str = "<style>body { margin: 0 } div, span, p { font: 20px/1 Ahem }
    p { margin: 0; height: 10px }</style>";

#[test]
fn blocks_move_and_what_follows_stays() {
    let laid = lay_out(&format!(
        "{AHEM}<div style='width: 200px; height: 100px'>
           <p id=moved style='position: relative; top: 5px; left: 7px'></p>
           <p id=after></p>
           <p id=right style='position: relative; right: 10px; bottom: 3px'></p>
           <p id=both style='position: relative; left: 1px; right: 50px; top: 2px; bottom: 50px'></p>
           <p id=percent style='position: relative; left: 50%; top: 50%'></p>
           <p id=static style='top: 5px; left: 7px'></p>
         </div>
         <div style='width: 200px'><p id=indefinite style='position: relative; top: 50%'></p></div>"
    ));
    assert_eq!(laid.rect("moved"), RectF::new(7.0, 5.0, 200.0, 10.0));
    // Laid out where `#moved` would be unmoved.
    assert_eq!(at(laid.rect("after")), (0.0, 10.0));
    // `right` and `bottom` the other way; `left` and `top` win.
    assert_eq!(at(laid.rect("right")), (-10.0, 17.0));
    assert_eq!(at(laid.rect("both")), (1.0, 32.0));
    // Percentages of the containing block: its height when definite.
    assert_eq!(at(laid.rect("percent")), (100.0, 90.0));
    assert_eq!(laid.rect("indefinite").y, 100.0);
    // Not positioned, not moved.
    assert_eq!(at(laid.rect("static")), (0.0, 50.0));
}

#[test]
fn items_and_atomic_inlines_move() {
    let laid = lay_out(&format!(
        "{AHEM}<div>X<span id=right style='display: inline-block; position: relative;
           right: 3px'>Y</span></div>
         <div style='display: flex'><p id=item style='width: 10px;
           position: relative; left: 3px'></p><p id=next style='width: 10px'></p></div>
         <div>X<span id=atomic style='display: inline-block; position: relative; top: -4px;
           left: 2px'>Y</span>Z</div>"
    ));
    // `right` moves it left.
    assert_eq!(laid.rect("right").x, 17.0);
    assert_eq!(laid.rect("item").x, 3.0);
    assert_eq!(laid.rect("next").x, 10.0);
    // Moved from where its line puts it.
    assert_eq!(at(laid.rect("atomic")), (22.0, 26.0));
}

#[test]
fn inline_elements_move_their_boxes_and_content() {
    let laid = lay_out(&format!(
        "{AHEM}<div id=line>A<span id=s style='position: relative; top: 3px; left: 5px;
           padding: 0 2px'>B<b style='position: relative; left: 1px'>C</b><i id=in
           style='display: inline-block'>E</i></span><u style='top: 9px; left: 9px'>F</u></div>"
    ));
    let (x, baselines, boxes) = laid.first_line("line");
    // `A`, then `B` and `C` moved with the span (and `C` with its own
    // element), then `F` where it would be: not positioned, its `top` and
    // `left` don't apply.
    assert_eq!(x, [0.0, 27.0, 48.0, 84.0]);
    assert_eq!(baselines, [16.0, 19.0, 19.0, 16.0]);
    // Its glyphs too.
    let line = &laid.fragments["line"].lines[0];
    assert_eq!(
        line.runs[1].glyphs().next().unwrap().1,
        PointF::new(27.0, 19.0)
    );
    // An atomic inline in it moves with it.
    assert_eq!(at(laid.rect("in")), (67.0, 3.0));
    // The span's box moves with it; `<b>`'s box with both.
    assert_eq!(at(boxes[0]), (25.0, 3.0));
    assert_eq!(at(boxes[1]), (48.0, 3.0));
}

#[test]
fn an_inline_element_moves_on_every_line_it_is_on() {
    let laid = lay_out(&format!(
        "{AHEM}<div id=lines style='width: 60px'><span style='position: relative; left: 4px;
           top: 50%; bottom: 2px'>AA BB</span></div>"
    ));
    let origin = laid.origins["lines"];
    let lines = &laid.fragments["lines"].lines;
    // `top: 50%` of an indefinite height is `auto`: moved up by `bottom`.
    for (i, line) in lines.iter().enumerate() {
        let y = 20.0 * i as f32;
        assert_eq!(origin.x + line.runs[0].x, 4.0, "line {i}");
        assert_eq!(origin.y + line.runs[0].baseline, y + 14.0, "line {i}");
        assert_eq!(line.boxes[0].rect.x, 4.0, "line {i}");
        assert_eq!(line.boxes[0].rect.y, y - 2.0, "line {i}");
    }
    assert_eq!(lines.len(), 2);
}

/// An absolutely positioned box is laid out against its containing block,
/// the nearest positioned ancestor's padding box, however deep it sits
/// below it; its own box is out of the flow, so what follows is laid out
/// as if it weren't there.
#[test]
fn absolute_boxes_are_laid_out_against_their_containing_block() {
    let laid = lay_out(
        "<style>body { margin: 0 } div { height: 20px }
           #cb { position: relative; margin: 10px; padding: 5px; border: 2px solid;
                 width: 300px; height: 200px }
         </style>
         <div id=cb>
           <div id=before></div>
           <div id=deep>
             <div id=abs style='position: absolute; top: 3px; left: 4px;
               width: 50px; height: 60px'></div>
             <div id=static-pos style='position: absolute; width: 10px; height: 10px'></div>
             <div id=after></div>
           </div>
         </div>",
    );
    // The containing block's padding box starts at (12, 12): its margin
    // and border. The box goes 4 and 3 into it, whatever is in between.
    assert_eq!(laid.rects["abs"], RectF::new(16.0, 15.0, 50.0, 60.0));
    // Out of the flow: `#after` is where it would be without it.
    assert_eq!(laid.rects["before"], RectF::new(17.0, 17.0, 300.0, 20.0));
    assert_eq!(laid.rects["after"], RectF::new(17.0, 37.0, 300.0, 20.0));
    // With both insets of an axis `auto`, where it would have been: in
    // `#deep`, after `#before`, measured from the containing block's
    // padding box, not its border box.
    assert_eq!(laid.rects["static-pos"], RectF::new(17.0, 37.0, 10.0, 10.0));
    // Laid out once, by the containing block: its fragment is the
    // containing block's child, and there is only one of it.
    let fragments = |id: &str| {
        fn count(fragment: &BoxFragment, node: gfx::NodeId) -> usize {
            usize::from(fragment.node == Some(node))
                + fragment
                    .children
                    .iter()
                    .map(|child| count(&child.fragment, node))
                    .sum::<usize>()
        }
        let node = *laid
            .ids
            .iter()
            .find(|(_, name)| *name == id)
            .expect("an element")
            .0;
        count(&laid.view, node)
    };
    assert_eq!(fragments("abs"), 1);
    let cb = &laid.fragments["cb"];
    assert!(
        cb.children
            .iter()
            .any(|child| child.fragment.node == laid.fragments["abs"].node),
        "the containing block holds it"
    );
}

/// Percentages and `auto` insets against the containing block: a size is
/// of its padding box, `auto` insets put the box where it would have been
/// (the static position), and both insets of an axis stretch it.
#[test]
fn absolute_sizes_and_static_positions() {
    let laid = lay_out(
        "<style>body { margin: 0 } div { height: 20px }
           #cb { position: relative; width: 200px; height: 100px; padding: 10px }
           #cb > div { height: 20px }
         </style>
         <div id=cb>
           <div id=first></div>
           <div id=percent style='position: absolute; top: 50%; left: 25%;
             width: 50%; height: 10%'></div>
           <div id=static-pos style='position: absolute; width: 30px; height: 30px'></div>
           <div id=stretched style='position: absolute; left: 0; right: 0; top: 0;
             bottom: 0; height: auto'></div>
           <div id=from-the-end style='position: absolute; right: 10px; bottom: 5px;
             width: 20px; height: 20px'></div>
         </div>",
    );
    // Of the containing block's padding box (220×120, from (0, 0)), as
    // CSS 2 § 10.1 says, not of its content box.
    assert_eq!(laid.rects["percent"], RectF::new(55.0, 60.0, 110.0, 12.0));
    // Where it would have been: after `#first`, at the content box's left.
    assert_eq!(laid.rects["static-pos"], RectF::new(10.0, 30.0, 30.0, 30.0));
    // Both insets of an axis: the padding box, less the insets.
    assert_eq!(laid.rects["stretched"], RectF::new(0.0, 0.0, 220.0, 120.0));
    // One inset `auto`: from the other edge, not the static position.
    assert_eq!(
        laid.rects["from-the-end"],
        RectF::new(220.0 - 10.0 - 20.0, 120.0 - 5.0 - 20.0, 20.0, 20.0)
    );
}

/// An absolutely positioned box is aligned in its inset-modified containing
/// block by `justify-self` and `align-self`; with `auto` insets, a flex
/// container puts it where its alignment would put a sole item, and a grid
/// container's child goes in its grid area (css-align 3, css-flexbox 1
/// § 4.1, css-grid 2 § 9).
#[test]
fn absolute_boxes_are_aligned_and_take_grid_areas() {
    let laid = lay_out(
        "<style>body { margin: 0 }
           .cb { position: relative; width: 200px; height: 100px }
         </style>
         <div class=cb>
           <div id=aligned style='position: absolute; inset: 0; width: 50px; height: 20px;
             justify-self: center; align-self: end'></div>
         </div>
         <div class=cb style='display: flex; justify-content: center; align-items: center'>
           <div id=in-flex style='position: absolute; width: 20px; height: 20px'></div>
         </div>
         <div class=cb style='display: grid; grid-template: 50px 50px / 100px 100px'>
           <div id=in-grid style='position: absolute; inset: 0; grid-area: 2 / 2'></div>
         </div>",
    );
    assert_eq!(laid.rects["aligned"], RectF::new(75.0, 80.0, 50.0, 20.0));
    assert_eq!(laid.rects["in-flex"], RectF::new(90.0, 140.0, 20.0, 20.0));
    assert_eq!(laid.rects["in-grid"], RectF::new(100.0, 250.0, 100.0, 50.0));
}

/// A fixed box is laid out against the viewport, however deep it sits and
/// whatever is positioned around it, and is no part of what can be
/// scrolled to.
#[test]
fn fixed_boxes_are_laid_out_against_the_viewport() {
    let laid = lay_out(
        "<style>body { margin: 0 }
           #cb { position: relative; margin: 10px; width: 300px; height: 200px }
         </style>
         <div id=cb>
           <div id=deep>
             <div id=bar style='position: fixed; top: 0; left: 0; right: 0; height: 50px'></div>
             <div id=corner style='position: fixed; bottom: 10px; right: 20px;
               width: 30px; height: 30px'></div>
             <div id=past style='position: fixed; top: 3000px; left: 900px;
               width: 40px; height: 40px'></div>
           </div>
         </div>
         <div id=tall style='height: 2000px'></div>",
    );
    // The viewport is 800×600: the bar spans it, the corner sits in it,
    // not in `#cb` (which `position: relative` would make the containing
    // block of an absolutely positioned box).
    assert_eq!(laid.rects["bar"], RectF::new(0.0, 0.0, 800.0, 50.0));
    assert_eq!(
        laid.rects["corner"],
        RectF::new(800.0 - 20.0 - 30.0, 600.0 - 10.0 - 30.0, 30.0, 30.0)
    );
    // The document is as big as the flow, not as the fixed boxes reach:
    // they don't scroll with it.
    assert_eq!(laid.rects["past"], RectF::new(900.0, 3000.0, 40.0, 40.0));
    let overflow = layout::scrollable_overflow(&laid.view);
    assert_eq!((overflow.width, overflow.height), (800.0, 2220.0));
}

/// An absolutely positioned box inside an atomic inline, whose containing
/// block is outside it, is laid out by that containing block: the line
/// passes it up, from where the atomic inline is.
#[test]
fn absolute_boxes_in_an_atomic_inline_go_up_through_the_line() {
    let laid = lay_out(
        "<style>body { margin: 0 } div { font: 20px/1 Ahem }</style>
         <div id=cb style='position: relative; width: 200px; height: 100px'>XX<span
           style='display: inline-block'>Y<div id=abs style='position: absolute;
             top: 5px; left: 7px; width: 10px; height: 10px'></div><div id=static-pos
             style='position: absolute; width: 10px; height: 10px'></div></span></div>",
    );
    assert_eq!(laid.rects["abs"], RectF::new(7.0, 5.0, 10.0, 10.0));
    // Where it would have been: below the inline-block's line, at its left.
    assert_eq!(laid.rects["static-pos"], RectF::new(40.0, 20.0, 10.0, 10.0));
}

/// An absolutely positioned box inside a float in a line, whose
/// containing block is outside it, goes up through the line too, from
/// where the float was placed.
#[test]
fn absolute_boxes_in_a_float_in_a_line_go_up_from_it() {
    let laid = lay_out(
        "<style>body { margin: 0 } div { font: 20px/1 Ahem }</style>
         <div id=cb style='position: relative; width: 200px; height: 100px'>XX<div
           style='float: right; width: 40px; height: 40px; margin-top: 3px'><div id=abs
             style='position: absolute; width: 10px; height: 10px'></div></div></div>",
    );
    assert_eq!(laid.rects["abs"], RectF::new(160.0, 3.0, 10.0, 10.0));
}

/// What an atomic inline passes up comes from where it is drawn: moved by
/// its own `position: relative` too.
#[test]
fn a_moved_atomic_inline_passes_up_from_where_it_is() {
    let laid = lay_out(
        "<style>body { margin: 0 } div { font: 20px/1 Ahem }</style>
         <div>XX<span style='display: inline-block; position: relative; left: 5px; top: 3px'>Y<div
           id=fixed style='position: fixed; width: 10px; height: 10px'></div></span></div>",
    );
    // Below the inline-block's line (it is 20px tall), moved with it.
    assert_eq!(laid.rects["fixed"], RectF::new(45.0, 23.0, 10.0, 10.0));
}

/// An out-of-flow box in inline content is part of it: the lines aren't
/// split around it, spaces collapse across it, and its static position is
/// where it is on its line, at the line's top if it was inline-level, or
/// at the start of the next line if it was block-level and something
/// comes before it (CSS 2 § 10.3.7).
#[test]
fn out_of_flow_boxes_in_inline_content() {
    let laid = lay_out(
        "<style>body { margin: 0 } div, span { font: 20px/1 Ahem }
           .abs { position: absolute; width: 10px; height: 10px }</style>
         <div id=para style='width: 300px'>XX <span id=inline class=abs></span> YY<div
           id=block-level class=abs></div>ZZ</div>
         <div id=first style='width: 300px'><div id=at-start class=abs></div>XX</div>
         <div id=ib style='width: 300px'>XX<span id=inline-block class=abs
           style='display: inline-block'></span>YY</div>
         <div id=after-atomic style='width: 300px'><span style='display: inline-block;
           width: 30px; height: 30px'></span><div id=below-atomic class=abs></div></div>
         <div id=alone style='margin-left: 5px'> <div id=lone class=abs></div> </div>",
    );
    // One line: `XX YY` (one space) and `ZZ` after it.
    assert_eq!(laid.rect("para").height, 20.0);
    assert_eq!(laid.first_line("para").0.last(), Some(&100.0));
    // After the space, at the line's top.
    assert_eq!(at(laid.rect("inline")), (60.0, 0.0));
    // Below the line it would have ended, at its start.
    assert_eq!(at(laid.rect("block-level")), (0.0, 20.0));
    // Block-level, but nothing before it: on the line.
    let top = laid.rect("first").y;
    assert_eq!(at(laid.rect("at-start")), (0.0, top));
    // An inline-block is inline-level: on its line, not below it.
    let top = laid.rect("ib").y;
    assert_eq!(at(laid.rect("inline-block")), (40.0, top));
    // An atomic inline before it is something too: below its line, which
    // is the inline-block on the baseline and the strut's 4px descent.
    let top = laid.rect("after-atomic").y;
    assert_eq!(at(laid.rect("below-atomic")), (0.0, top + 34.0));
    // Alone in its block, it still has a static position there.
    let top = laid.rect("alone").y;
    assert_eq!(at(laid.rect("lone")), (5.0, top));
}

/// Among blocks, an out-of-flow box goes with the inline content before
/// it; with none before it (or only spaces that collapse away), it is
/// among the blocks, and the content after it is an anonymous block of
/// its own (Blink's `LayoutBlockFlow::AddChild`).
#[test]
fn out_of_flow_boxes_go_with_the_inline_content_before_them() {
    let laid = lay_out(
        "<style>body { margin: 0 } div, span, p { font: 20px/1 Ahem }
           p { margin: 0; height: 10px }
           .abs { position: absolute; width: 10px; height: 10px }</style>
         <div><p></p>XX <span id=after-text class=abs></span>YY</div>
         <div id=second><p></p> <span id=after-block class=abs></span>YY</div>
         <div id=third style='width: 100px'><p></p> <span id=end-aligned class=abs
           style='justify-self: end'></span>YY</div>",
    );
    // In the anonymous block with `XX YY`, after `XX `.
    assert_eq!(at(laid.rect("after-text")), (60.0, 10.0));
    // Among the blocks, after the `<p>`: where the next block starts.
    let top = laid.rect("second").y;
    assert_eq!(at(laid.rect("after-block")), (0.0, top + 10.0));
    // Among the blocks, its static-position rectangle spans the content
    // box, which `justify-self` aligns it in.
    let top = laid.rect("third").y;
    assert_eq!(at(laid.rect("end-aligned")), (90.0, top + 10.0));
}

/// An inline formatting context's own box can be the containing block of
/// the out-of-flow boxes in it.
#[test]
fn inline_content_lays_out_its_own_out_of_flow_boxes() {
    let laid = lay_out(
        "<style>body { margin: 0 } div, span { font: 20px/1 Ahem }</style>
         <div id=cb style='position: relative; margin-left: 50px; border: 3px solid;
           width: 200px'>XX<span id=abs style='position: absolute; top: 5px; left: 7px;
           width: 10px; height: 10px'></span></div>",
    );
    assert_eq!(laid.rects["abs"], RectF::new(60.0, 8.0, 10.0, 10.0));
}

/// Its self-alignment aligns an out-of-flow box in inline content in its
/// static-position rectangle: the content box's width if it was
/// block-level, its line's height if it was inline-level.
#[test]
fn out_of_flow_boxes_in_inline_content_are_aligned() {
    let laid = lay_out(
        "<style>body { margin: 0 } div, span { font: 20px/1 Ahem }
           .abs { position: absolute; width: 10px; height: 10px }</style>
         <div style='width: 100px'><div id=block-level class=abs
           style='justify-self: end'></div></div>
         <div id=line style='width: 100px'>XX<span id=inline-level class=abs
           style='align-self: end'></span></div>
         <div id=items style='width: 100px; justify-items: center'><div id=by-items
           class=abs></div></div>
         <div id=lone-line style='width: 100px'> <span id=lone-end class=abs
           style='justify-self: end'></span> </div>",
    );
    assert_eq!(at(laid.rect("block-level")), (90.0, 0.0));
    // `auto` takes the container's `justify-items`.
    let top = laid.rect("items").y;
    assert_eq!(at(laid.rect("by-items")), (45.0, top));
    let top = laid.rect("line").y;
    assert_eq!(at(laid.rect("inline-level")), (40.0, top + 10.0));
    // Alone in inline content, an inline-level one's rectangle is the point
    // where it would have been: its end on it.
    let top = laid.rect("lone-line").y;
    assert_eq!(at(laid.rect("lone-end")), (-10.0, top));
}
