//! `contain`: size containment's sizes, layout containment's baseline and
//! formatting context, the containing blocks `transform` and containment
//! make, and which fixed boxes are fixed to the viewport.

use gfx::SizeF;

use crate::page::{Page, lay_out};

const AHEM: &str = "<style>body { margin: 0; font: 20px/1 Ahem }</style>";

/// Size containment: what is in the box counts for nothing in its size. A
/// shrink-to-fit box is as wide as its padding and border, any box with an
/// `auto` height as tall; what it holds is laid out, overflowing. A size
/// or `min-*` of its own holds, and a block in flow keeps its width.
/// `inline-size` contains the width only; tables don't take it.
#[test]
fn size_containment() {
    let laid = lay_out(&format!(
        "{AHEM}<div><span id=a style='display: inline-block; contain: size; padding: 5px'>XX XX</span></div>
         <div id=b style='contain: size'>XX XX</div>
         <div id=c style='contain: size; height: 30px; width: 50px'>XX</div>
         <div id=d style='contain: size; min-height: 15px; box-sizing: border-box; padding: 2px'>XX</div>
         <div><span id=e style='display: inline-block; contain: inline-size'>XX XX</span></div>
         <div style='display: flex; flex-direction: column; align-items: start'><div id=f style='contain: size'>XX</div></div>
         <div><span id=g style='display: inline-table; contain: size'><span style='display: table-cell'>XX</span></span></div>
         <canvas id=h style='contain: size'></canvas><canvas id=i style='contain: size; width: 30px'></canvas>
         <canvas id=j style='contain: inline-size'></canvas><canvas id=k style='contain: inline-size; height: 50px'></canvas>
         <div><span id=l style='display: inline-block; contain: size; width: 30px; height: 20px'>XX XX</span></div>"
    ));
    assert_eq!(laid.rect("a").size(), SizeF::new(10.0, 10.0));
    assert_eq!(laid.rect("b").size(), SizeF::new(800.0, 0.0));
    assert_eq!(laid.rect("c").size(), SizeF::new(50.0, 30.0));
    assert_eq!(laid.rect("d").height, 15.0);
    // As tall as its text broken at width 0: a line a word.
    assert_eq!(laid.rect("e").size(), SizeF::new(0.0, 40.0));
    assert_eq!(laid.rect("f").size(), SizeF::new(0.0, 0.0));
    assert_eq!(laid.rect("g").size(), SizeF::new(40.0, 20.0));
    // A replaced element has no natural size or ratio.
    assert_eq!(laid.rect("h").size(), SizeF::new(0.0, 0.0));
    assert_eq!(laid.rect("i").size(), SizeF::new(30.0, 0.0));
    // With inline-size containment, no natural width, nor a ratio to
    // take one from its height.
    assert_eq!(laid.rect("j").size(), SizeF::new(0.0, 150.0));
    assert_eq!(laid.rect("k").size(), SizeF::new(0.0, 50.0));
    // Its own size, given as an atomic inline's isn't.
    assert_eq!(laid.rect("l").size(), SizeF::new(30.0, 20.0));
}

/// Layout containment: the box has no baseline, so an inline-block's is
/// its bottom margin edge, and one around it takes its baseline from the
/// line before; it holds its floats (an independent formatting context).
#[test]
fn layout_containment() {
    let laid = lay_out(&format!(
        "{AHEM}<div id=line>X<span id=a style='display: inline-block; contain: layout; height: 40px'>X</span></div>
         <div id=b style='contain: layout'><div style='float: left; width: 10px; height: 50px'></div></div>
         <div id=line2><span style='display: inline-block; height: 100px'></span><span id=c style='display: inline-block'>X<div style='contain: layout'>X</div></span></div>"
    ));
    // `c`'s baseline is its first line's (16 down), on the line's, which
    // the empty inline-block before it puts 100 down.
    assert_eq!(laid.rect("c").y, laid.rect("line2").y + 84.0);
    // The inline-block's bottom on the line's baseline: the line is 40 +
    // the descent tall, the `X` before it 40 - 16 down.
    assert_eq!(laid.rect("a").y, 0.0);
    assert_eq!(laid.rect("line").height, 44.0);
    assert_eq!(laid.rect("b").height, 50.0);
}

/// A box with a `transform`, or layout or paint containment, is the
/// containing block of the absolute and fixed boxes in it, static or not;
/// a fixed box no box around contains is fixed to the viewport.
#[test]
fn containing_blocks_for_out_of_flow_boxes() {
    let laid = lay_out(&format!(
        "{AHEM}<div style='margin-top: 50px; height: 10px; transform: translateX(0)'><div id=a style='position: absolute; top: 0'>X</div><div id=b style='position: fixed; top: 0'>X</div></div>
         <div style='margin-top: 50px; height: 10px; contain: paint'><div id=c style='position: fixed; top: 0'>X</div></div>
         <div style='margin-top: 50px; height: 10px; contain: layout'><div id=d style='position: absolute; top: 0'>X</div></div>
         <div style='margin-top: 50px; height: 10px; contain: style size'><div id=e style='position: fixed; top: 0'>X</div></div>
         <table style='margin-top: 50px'><tr style='contain: paint'><td><div id=f style='position: fixed; top: 0'>X</div></td></tr></table>
         <div style='margin-top: 50px; height: 10px; filter: opacity(1)'><div id=g style='position: fixed; top: 0'>X</div></div>"
    ));
    assert_eq!(laid.rect("a").y, 50.0);
    assert_eq!(laid.rect("b").y, 50.0);
    assert_eq!(laid.rect("c").y, 110.0);
    assert_eq!(laid.rect("d").y, 170.0);
    // Style and size containment contain neither.
    assert_eq!(laid.rect("e").y, 0.0);
    let fixed = |id: &str| laid.fragment(id).fixed_to_viewport;
    assert!(!fixed("b"));
    assert!(!fixed("c"));
    assert!(fixed("e"));
    // Nor does a row's, which doesn't apply.
    assert_eq!(laid.rect("f").y, 0.0);
    assert!(fixed("f"));
    // A filter contains them (filter-effects 1), a no-op one too.
    assert!(!fixed("g"));
}

/// The root element's filter contains no out-of-flow box (Blink's
/// `ComputeIsFixedContainer` leaves out the document element).
#[test]
fn a_filtered_root_contains_nothing() {
    let laid = lay_out(
        "<html style='filter: blur(1px)'><body style='margin: 0'>
         <div style='height: 50px'></div><div id=f style='position: fixed; top: 10px'>X</div>",
    );
    assert!(laid.fragment("f").fixed_to_viewport);
    assert_eq!(laid.rect("f").y, 10.0);
}

/// What paint containment clips, and a fixed box fixed to the viewport,
/// reach nowhere in the document's size; a fixed box a box around
/// contains does, there and in a scroll container's overflow. `is_fixed` is the fixed boxes fixed to the viewport.
#[test]
fn contained_overflow_and_fixed_boxes() {
    let laid = lay_out(
        "<style>body { margin: 0 }</style>
         <div style='contain: paint; height: 10px'><div style='height: 3000px'></div></div>
         <div style='transform: translate(0); height: 10px'><div id=c style='position: fixed; top: 2000px; height: 10px'></div></div>
         <div id=v style='position: fixed; top: 5000px; height: 10px'></div>
         <div id=s style='overflow: auto; height: 100px; transform: translate(0)'><div style='position: fixed; top: 500px; height: 10px'></div></div>",
    );
    assert_eq!(layout::document_size(&laid.view).height, 2020.0);
    // A scroll container that contains a fixed box scrolls to it.
    assert_eq!(
        layout::scrollable_overflow(laid.fragment("s")).height,
        510.0
    );
    let node = |id: &str| laid.fragment(id).node.unwrap();
    assert!(!layout::is_fixed(&laid.view, node("c")));
    assert!(layout::is_fixed(&laid.view, node("v")));
}

/// Containment on the root element or `<body>` keeps `<body>`'s
/// `overflow` from the viewport: style containment on any box, the others
/// on one they apply to (not a non-atomic inline box).
#[test]
fn containment_keeps_the_bodys_overflow_to_itself() {
    use computed_style::Overflow::{Hidden, Visible};
    let viewport = |html: &str| Page::build(html).tree.viewport_overflow();
    assert_eq!(
        viewport("<body style='overflow: hidden'>"),
        (Hidden, Hidden)
    );
    assert_eq!(
        viewport("<body style='overflow: hidden; contain: paint'>"),
        (Visible, Visible)
    );
    assert_eq!(
        viewport("<html style='contain: size'><body style='overflow: hidden'>"),
        (Visible, Visible)
    );
    let inline = "<body style='overflow: hidden; display: inline; contain:";
    assert_eq!(viewport(&format!("{inline} style'>")), (Visible, Visible));
    assert_eq!(viewport(&format!("{inline} paint'>")), (Hidden, Hidden));
}

/// A table cell that contains the out-of-flow boxes in it lays them out
/// against its height in the row, not its content's.
#[test]
fn a_containing_cell_lays_out_against_the_rows_height() {
    let laid = lay_out(
        "<table style='border-spacing: 0'><tr><td style='height: 100px; padding: 0'></td>
           <td id=cell style='contain: layout; padding: 0'><div id=x style='position: absolute; bottom: 0; width: 10px; height: 10px'></div></td></tr></table>",
    );
    let (cell, x) = (laid.rect("cell"), laid.rect("x"));
    assert_eq!(cell.height, 100.0);
    assert_eq!(x.y + x.height, cell.y + cell.height);
}
