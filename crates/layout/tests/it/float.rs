//! `float` and `clear`: block-level floats placed by taffy's block layout
//! (CSS 2 § 9.5), what clears them, and the boxes and lines that avoid
//! them.

use gfx::{RectF, SizeF};

use crate::page::{Page, lay_out};

const RESET: &str = "<style>body { margin: 0 } div { box-sizing: border-box }</style>";

/// Left floats go left, each to the right of the one before, right floats
/// right; one that doesn't fit beside them goes lower, as high as it fits
/// (CSS 2 § 9.5.1).
#[test]
fn floats_go_left_and_right() {
    let laid = lay_out(&format!(
        "{RESET}<div style='width: 300px'>
           <div id=a style='float: left; width: 100px; height: 50px'></div>
           <div id=b style='float: right; width: 80px; height: 40px'></div>
           <div id=c style='float: left; width: 50px; height: 30px'></div>
           <div id=d style='float: left; width: 100px; height: 20px'></div>
         </div>"
    ));
    assert_eq!(laid.rect("a"), RectF::new(0.0, 0.0, 100.0, 50.0));
    assert_eq!(laid.rect("b"), RectF::new(220.0, 0.0, 80.0, 40.0));
    assert_eq!(laid.rect("c"), RectF::new(100.0, 0.0, 50.0, 30.0));
    // 100px wide: no room between `c` and `b`, but below `c`, beside `a`,
    // there is: as high as it can go (rule 8).
    assert_eq!(laid.rect("d"), RectF::new(100.0, 30.0, 100.0, 20.0));
    for id in ["a", "b", "c", "d"] {
        assert!(laid.fragments[id].floating, "{id}");
    }
}

/// A float takes no room in the flow; `clear` puts a block below the
/// floats on its side, and a box that establishes a block formatting
/// context is beside them, not over them (CSS 2 § 9.5).
#[test]
fn clearing_and_avoiding_floats() {
    let laid = lay_out(&format!(
        "{RESET}<div style='width: 300px'>
           <div id=left style='float: left; width: 100px; height: 50px'></div>
           <div id=right style='float: right; width: 50px; height: 80px'></div>
           <div id=flow style='height: 10px'></div>
           <div id=bfc style='overflow: hidden; height: 10px'></div>
           <div id=clear-left style='clear: left; height: 10px'></div>
           <div id=clear-both style='clear: both; height: 10px'></div>
         </div>"
    ));
    // Under the floats, from the left edge: they took no room.
    assert_eq!(laid.rect("flow"), RectF::new(0.0, 0.0, 300.0, 10.0));
    // Between them.
    assert_eq!(laid.rect("bfc"), RectF::new(100.0, 10.0, 150.0, 10.0));
    assert_eq!(laid.rect("clear-left").y, 50.0);
    assert_eq!(laid.rect("clear-both").y, 80.0);
}

/// A float in a flex container is a flex item: its `float` does nothing.
#[test]
fn a_flex_item_does_not_float() {
    let laid = lay_out(&format!(
        "{RESET}<div style='display: flex; width: 300px'>
           <div id=first style='width: 50px; height: 10px'></div>
           <div id=item style='float: right; width: 50px; height: 10px'></div>
         </div>"
    ));
    assert_eq!(laid.rect("item"), RectF::new(50.0, 0.0, 50.0, 10.0));
    assert!(!laid.fragments["item"].floating);
}

/// A block that holds only floats is as tall as its flow, which they take
/// no room in; the block formatting context's root grows to hold them.
#[test]
fn floats_take_no_room_but_their_root_holds_them() {
    let laid = lay_out(&format!(
        "{RESET}<div id=root style='overflow: hidden; width: 300px'>
           <div id=holder><div style='float: left; width: 50px; height: 40px'></div></div>
         </div>"
    ));
    assert_eq!(laid.rect("holder").height, 0.0);
    assert_eq!(laid.rect("root").height, 40.0);
}

const AHEM: &str = "<style>body { margin: 0 } div, p { font: 20px/1 Ahem; margin: 0 }</style>";

/// Lines beside a float are as wide as the room it leaves, and start past
/// it; below it they are the whole width again (CSS 2 § 9.5).
#[test]
fn lines_go_around_floats() {
    let laid = lay_out(&format!(
        "{AHEM}<div style='width: 200px'>
           <div style='float: left; width: 100px; height: 40px'></div>
           <p id=left>XX XX XX XX XX XX XX</p>
         </div>
         <div style='width: 200px'>
           <div style='float: right; width: 60px; height: 20px'></div>
           <p id=right>XX XX XX XX XX XX</p>
         </div>"
    ));
    let lines = laid.lines("left");
    // Two words beside the float (100px of room), four below it.
    assert_eq!(lines[0], RectF::new(100.0, 0.0, 100.0, 20.0));
    assert_eq!(lines[1], RectF::new(100.0, 20.0, 100.0, 20.0));
    assert_eq!(lines[2], RectF::new(0.0, 40.0, 160.0, 20.0));
    // The paragraph is as tall as its lines: in the flow, under the float.
    assert_eq!(laid.rect("left"), RectF::new(0.0, 0.0, 200.0, 60.0));
    let top = laid.rect("right").y;
    let lines = laid.lines("right");
    assert_eq!(lines[0], RectF::new(0.0, top, 100.0, 20.0));
    assert_eq!(lines[1], RectF::new(0.0, top + 20.0, 160.0, 20.0));
}

/// A line whose first word doesn't fit beside the floats goes down to
/// where it does (Blink's next layout opportunity), not over them.
#[test]
fn a_line_that_doesnt_fit_goes_below() {
    let laid = lay_out(&format!(
        "{AHEM}<div style='width: 200px'>
           <div style='float: left; width: 100px; height: 40px'></div>
           <p id=para>XXXXXX XX</p>
         </div>"
    ));
    let lines = laid.lines("para");
    assert_eq!(lines[0], RectF::new(0.0, 40.0, 180.0, 20.0));
    assert_eq!(laid.rect("para").height, 60.0);
}

/// A line reaching down into a float lower than where it starts is in the
/// room that float leaves too: broken again, narrower.
#[test]
fn a_line_reaching_a_lower_float_is_narrower() {
    let laid = lay_out(&format!(
        "{AHEM}<div style='width: 200px'>
           <div style='height: 0'><div style='height: 15px'></div><div style='float: right;
             width: 100px; height: 50px'></div></div>
           <p id=para>XX XX XX XX XX XX</p>
         </div>"
    ));
    let lines = laid.lines("para");
    // From 0 to 20 the line reaches the float, from 15: two words.
    assert_eq!(lines[0], RectF::new(0.0, 0.0, 100.0, 20.0));
}

/// The room beside a float is in the content box: padding wider than the
/// float keeps the lines where they were.
#[test]
fn lines_beside_floats_keep_to_their_content_box() {
    let laid = lay_out(&format!(
        "{AHEM}<div style='width: 300px'>
           <div style='float: left; width: 100px; height: 20px'></div>
           <p id=padded style='padding-left: 150px'>XX</p>
         </div>"
    ));
    assert_eq!(laid.lines("padded")[0].x, 150.0);
}

/// A line that doesn't wrap and is too wide for the room beside a float
/// goes below it, where it has the whole width (Blink tries the next
/// layout opportunity for a line that overflows one).
#[test]
fn a_line_that_doesnt_wrap_goes_below_a_float() {
    let laid = lay_out(&format!(
        "{AHEM}<div style='width: 200px'>
           <div style='float: right; width: 100px; height: 20px'></div>
           <p id=nowrap style='white-space: nowrap'>XXXXXXXX</p>
         </div>"
    ));
    assert_eq!(laid.lines("nowrap")[0], RectF::new(0.0, 20.0, 160.0, 20.0));
}

/// Beside floats, the content ends at its last line's bottom, but for a
/// last line with nothing on it: the white space after an atomic inline
/// too wide for any room, which parley's own height leaves out too.
#[test]
fn a_last_empty_line_beside_floats_takes_no_room() {
    let laid = lay_out(&format!(
        "{AHEM}<div style='width: 200px'>
           <div style='float: left; width: 100px; height: 40px'></div>
           <p id=para><span style='display: inline-block; width: 250px; height: 20px'></span> </p>
         </div>"
    ));
    assert_eq!(laid.lines("para")[0].y, 40.0);
    // The inline-block on the baseline and the strut's 4px descent.
    assert_eq!(laid.rect("para").height, 64.0);
}

/// A float after inline content goes beside the line it comes in, at its
/// top, and that line and those beside it are as wide as the room it
/// leaves, the content before it on the line too (CSS 2 § 9.5.1). Its
/// containing block is the content box.
#[test]
fn a_float_in_a_line_goes_beside_it() {
    let laid = lay_out(&format!(
        "{AHEM}<div id=para style='width: 200px; padding: 5px 0 0 7px'>XX <span id=float
           style='float: left; width: 40px; height: 40px'></span>XX XX XX</div>"
    ));
    assert_eq!(laid.rect("float"), RectF::new(7.0, 5.0, 40.0, 40.0));
    assert!(laid.fragments["float"].floating);
    let lines = laid.lines("para");
    assert_eq!(lines[0], RectF::new(47.0, 5.0, 160.0, 20.0));
    assert_eq!(lines[1], RectF::new(47.0, 25.0, 40.0, 20.0));
    // Its lines, not its float: the block formatting context's root holds
    // that.
    assert_eq!(laid.rect("para").height, 45.0);
}

/// A float too wide for what is left of its line goes below it (Blink's
/// `ShouldPushFloatAfterLine`), and so does one after it on the line,
/// even one that would fit; the next line goes beside them.
#[test]
fn a_float_that_doesnt_fit_goes_below_its_line() {
    let laid = lay_out(&format!(
        "{AHEM}<div id=para style='width: 200px'>XXXXXXXX<span id=float
           style='float: left; width: 100px; height: 20px'></span><span id=next
           style='float: left; width: 20px; height: 20px'></span> XX</div>"
    ));
    assert_eq!(laid.rect("float"), RectF::new(0.0, 20.0, 100.0, 20.0));
    assert_eq!(laid.rect("next"), RectF::new(100.0, 20.0, 20.0, 20.0));
    let lines = laid.lines("para");
    assert_eq!(lines[0], RectF::new(0.0, 0.0, 160.0, 20.0));
    assert_eq!(lines[1], RectF::new(120.0, 20.0, 40.0, 20.0));
}

/// A float too wide for its line, where the line then breaks before it,
/// goes below the next line, the one it is on.
#[test]
fn a_float_below_a_line_that_breaks_before_it_goes_below_the_next() {
    let laid = lay_out(&format!(
        "{AHEM}<div id=para style='width: 200px'>XX XXXXXX<span id=float
           style='float: left; width: 100px; height: 20px'></span>XX</div>"
    ));
    assert_eq!(laid.rect("float"), RectF::new(0.0, 40.0, 100.0, 20.0));
}

/// A float that fit in its line, where the line then breaks before it,
/// is on the next line: it takes no room from this one, and goes beside
/// the next (Blink's `RewindFloats`).
#[test]
fn a_float_the_line_breaks_before_goes_beside_the_next() {
    let laid = lay_out(&format!(
        "{AHEM}<div id=para style='width: 200px'>XX XXXX<span id=float
           style='float: left; width: 40px; height: 40px'></span>XXXX</div>"
    ));
    let lines = laid.lines("para");
    assert_eq!(lines[0], RectF::new(0.0, 0.0, 40.0, 20.0));
    assert_eq!(laid.rect("float"), RectF::new(0.0, 20.0, 40.0, 40.0));
    assert_eq!(lines[1], RectF::new(40.0, 20.0, 160.0, 20.0));
}

/// A float that fits beside what comes before it on the line, but leaves
/// too little room for what comes after it that the line can't break
/// before, stays at the line's top: the line goes down past it, to where
/// it fits (CSS 2 § 9.5: a line box too short for any content is shifted
/// down).
#[test]
fn a_line_too_long_beside_its_float_goes_below_it() {
    let laid = lay_out(&format!(
        "{AHEM}<div id=para style='width: 200px'>XX<span id=float
           style='float: left; width: 100px; height: 20px'></span>XXXXXX</div>"
    ));
    assert_eq!(laid.rect("float"), RectF::new(0.0, 0.0, 100.0, 20.0));
    let lines = laid.lines("para");
    assert_eq!(lines[0], RectF::new(0.0, 20.0, 160.0, 20.0));
}

/// Beside a float placed before, a line too long for the room its own
/// float leaves goes down past both, its float staying at its top, beside
/// the first (Blink places a float that fits where it comes, then tries
/// the line in the next layout opportunity).
#[test]
fn a_line_too_long_beside_its_float_goes_past_the_floats_before_too() {
    let laid = lay_out(&format!(
        "{AHEM}<div style='width: 200px'>
           <div style='float: left; width: 100px; height: 20px'></div>
           <div id=para><span id=float style='float: left; width: 40px; height: 40px'></span>XXXXXX</div>
         </div>"
    ));
    assert_eq!(laid.rect("float"), RectF::new(100.0, 0.0, 40.0, 40.0));
    assert_eq!(laid.lines("para")[0], RectF::new(0.0, 40.0, 120.0, 20.0));
}

/// The floats in a line are in the block formatting context the lines are
/// in: a box after them that establishes one is beside them, and one that
/// establishes its own holds them (CSS 2 § 10.6.7).
#[test]
fn floats_in_lines_are_in_their_block_formatting_context() {
    let laid = lay_out(&format!(
        "{AHEM}<div style='width: 200px'>
           <div>XX<span style='float: right; width: 40px; height: 60px'></span></div>
           <div id=beside style='overflow: hidden; height: 10px'></div>
         </div>
         <div id=root style='overflow: hidden; width: 200px'>XX<span id=held
           style='float: left; width: 40px; height: 60px'></span></div>"
    ));
    assert_eq!(laid.rect("beside"), RectF::new(0.0, 20.0, 160.0, 10.0));
    let root = laid.rect("root");
    assert_eq!(root.height, 60.0);
    assert_eq!(laid.rect("held").y, root.y);
    assert_eq!(laid.lines("root")[0].x, 40.0);
}

/// A shrink-to-fit box is as wide as its lines with the floats in them
/// beside them (Blink's `InlineNode::ComputeMinMaxSizes`): a float and an
/// inline-block side by side, in the box or in an inline-block in it. At
/// its narrowest, as wide as its widest float.
#[test]
fn floats_in_lines_widen_a_shrink_to_fit_box() {
    let laid = lay_out(&format!(
        "{AHEM}<div id=outer style='float: left'><div id=float
           style='float: right; width: 100px; height: 20px'></div><span id=atomic
           style='display: inline-block; width: 100px; height: 20px'></span></div>
         <div style='clear: both'></div>
         <div id=nested style='float: left'><span style='display: inline-block'>XX<span
           style='float: left; width: 60px; height: 20px'></span></span></div>
         <div style='clear: both; width: 10px'><div id=narrow style='float: left'>XX XX<span
           style='float: left; width: 60px; height: 10px'></span></div></div>"
    ));
    assert_eq!(laid.rect("outer").width, 200.0);
    assert_eq!(laid.rect("float").x, 100.0);
    assert_eq!(laid.rect("atomic").x, 0.0);
    assert_eq!(laid.rect("nested").width, 100.0);
    assert_eq!(laid.rect("narrow").width, 60.0);
}

/// Each layout puts the floats in the lines into the block formatting
/// context again, which is a new one each time: the lines aren't taken
/// from the last layout as they are.
#[test]
fn floats_in_lines_are_placed_at_every_layout() {
    let mut page = Page::build(&format!(
        "{AHEM}<div style='width: 200px'>
           <div>XX<span style='float: right; width: 40px; height: 60px'></span></div>
           <div id=beside style='overflow: hidden; height: 10px'></div>
         </div>"
    ));
    let tree = &mut page.tree;
    tree.layout(SizeF::new(800.0, 600.0));
    // Another height only: the paragraph is laid out at the same width.
    let view = tree.layout(SizeF::new(800.0, 500.0));
    let html = &view.children[0].fragment;
    let body = &html.children[0].fragment;
    let outer = &body.children[0].fragment;
    let beside = &outer.children[1];
    assert_eq!(beside.fragment.size.width, 160.0);
}

/// Negative margins can make a float's margin box narrower than nothing:
/// it takes no room from its line, which is no wider than its room.
#[test]
fn a_float_narrower_than_nothing_takes_no_room() {
    let laid = lay_out(&format!(
        "{AHEM}<div id=para style='width: 200px'>XX <span
           style='float: left; width: 40px; height: 20px; margin-left: -60px'></span>XX</div>"
    ));
    assert_eq!(laid.lines("para")[0], RectF::new(0.0, 0.0, 100.0, 20.0));
}
