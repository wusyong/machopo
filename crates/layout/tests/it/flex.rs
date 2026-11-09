//! Flex layout on parsed, styled documents: which boxes are flex items, in
//! which order, and where taffy's flex algorithm puts them.

use gfx::{PointF, RectF, SizeF};
use layout::{BoxFragment, BoxRole};

use crate::page::{Laid, lay_out};

/// The baselines of the text runs below `id`, in LayoutView coordinates.
fn baselines(laid: &Laid, id: &str) -> Vec<f32> {
    fn collect(fragment: &BoxFragment, origin: PointF, out: &mut Vec<f32>) {
        for line in &fragment.lines {
            out.extend(line.runs.iter().map(|run| origin.y + run.baseline));
        }
        for child in &fragment.children {
            let at = PointF::new(origin.x + child.offset.x, origin.y + child.offset.y);
            collect(&child.fragment, at, out);
        }
    }
    let rect = laid.rect(id);
    let mut out = Vec::new();
    collect(laid.fragment(id), PointF::new(rect.x, rect.y), &mut out);
    out
}

const BODY: &str = "<style>body { margin: 0 } div div { height: 10px }</style>";

#[test]
fn a_row_puts_items_side_by_side() {
    let laid = lay_out(&format!(
        "{BODY}<div id=c style='display: flex; width: 300px'>
           <div id=a style='width: 50px'></div><div id=b style='width: 70px'></div>
         </div>"
    ));
    assert_eq!(laid.rect("a"), RectF::new(0.0, 0.0, 50.0, 10.0));
    assert_eq!(laid.rect("b"), RectF::new(50.0, 0.0, 70.0, 10.0));
    // A flex container is block-level: it fills the width it is given.
    assert_eq!(laid.rect("c"), RectF::new(0.0, 0.0, 300.0, 10.0));
}

#[test]
fn justify_content_shares_the_free_space() {
    let laid = lay_out(&format!(
        "{BODY}<div style='display: flex; width: 300px; justify-content: space-between'>
           <div id=a style='width: 50px'></div><div id=b style='width: 50px'></div>
           <div id=c style='width: 50px'></div>
         </div>
         <div style='display: flex; width: 300px; justify-content: center'>
           <div id=d style='width: 100px'></div>
         </div>"
    ));
    assert_eq!(laid.rect("a").x, 0.0);
    assert_eq!(laid.rect("b").x, 125.0);
    assert_eq!(laid.rect("c").x, 250.0);
    assert_eq!(laid.rect("d").x, 100.0);
}

#[test]
fn flex_grows_and_shrinks_items() {
    let laid = lay_out(&format!(
        "{BODY}<div style='display: flex; width: 300px'>
           <div id=a style='flex: 1'></div><div id=b style='flex: 2'></div>
         </div>
         <div style='display: flex; width: 100px'>
           <div id=c style='width: 100px'></div>
           <div id=d style='width: 100px; flex-shrink: 0'></div>
         </div>
         <div style='display: flex; width: 300px'>
           <div id=e style='flex: 1; width: 100px'></div><div id=f style='flex: 1 1 50px'></div>
         </div>"
    ));
    assert_eq!(laid.rect("a").width, 100.0);
    assert_eq!(laid.rect("b").width, 200.0);
    // `c` shrinks to nothing; `d` doesn't, and overflows.
    assert_eq!(laid.rect("c").width, 0.0);
    assert_eq!(laid.rect("d"), RectF::new(0.0, 10.0, 100.0, 10.0));
    // From their bases, 0 (`flex: 1` is `1 1 0%`, not the width) and
    // 50px, each takes half of the rest.
    assert_eq!(laid.rect("e").width, 125.0);
    assert_eq!(laid.rect("f").width, 175.0);
}

#[test]
fn a_column_stacks_items() {
    let laid = lay_out(&format!(
        "{BODY}<div style='display: flex; flex-direction: column; width: 300px; align-items: flex-end'>
           <div id=a style='width: 50px'></div><div id=b style='width: 70px'></div>
         </div>"
    ));
    assert_eq!(laid.rect("a"), RectF::new(250.0, 0.0, 50.0, 10.0));
    assert_eq!(laid.rect("b"), RectF::new(230.0, 10.0, 70.0, 10.0));
}

#[test]
fn items_wrap_with_gaps() {
    let laid = lay_out(&format!(
        "{BODY}<div style='display: flex; flex-wrap: wrap; width: 100px; gap: 5px 10px'>
           <div id=a style='width: 40px'></div><div id=b style='width: 40px'></div>
           <div id=c style='width: 40px'></div>
         </div>"
    ));
    assert_eq!(laid.rect("a"), RectF::new(0.0, 0.0, 40.0, 10.0));
    assert_eq!(laid.rect("b"), RectF::new(50.0, 0.0, 40.0, 10.0));
    assert_eq!(laid.rect("c"), RectF::new(0.0, 15.0, 40.0, 10.0));
}

#[test]
fn items_stretch_in_the_cross_axis_unless_aligned() {
    let laid = lay_out(&format!(
        "{BODY}<div style='display: flex; height: 50px'>
           <p id=a style='margin: 0; width: 10px'></p>
           <p id=b style='margin: 0; width: 10px; align-self: center; height: 10px'></p>
           <p id=c style='margin: 0; width: 10px; height: 10px'></p>
         </div>"
    ));
    assert_eq!(laid.rect("a").height, 50.0);
    assert_eq!(laid.rect("b").y, 20.0);
    // A definite height doesn't stretch.
    assert_eq!(laid.rect("c").height, 10.0);
}

#[test]
fn self_start_and_self_end_are_the_items_own_start_and_end() {
    let laid = lay_out(&format!(
        "{BODY}<div style='display: flex; height: 50px; align-items: self-end'>
           <p id=end style='margin: 0; width: 10px; height: 10px'></p>
           <p id=start style='margin: 0; width: 10px; height: 10px; align-self: self-start'></p>
         </div>"
    ));
    assert_eq!(laid.rect("end").y, 40.0);
    assert_eq!(laid.rect("start").y, 0.0);
}

#[test]
fn inline_children_are_items_and_text_runs_anonymous_items() {
    let laid = lay_out(
        "<div id=c style='display: flex'>
           Some text <span id=span>a span</span> more <b>text</b>
           <div id=block></div>
         </div>",
    );
    // `<span>` and `<b>` are blockified items; each run of text between
    // them an anonymous one; white space alone none.
    let c = laid.fragment("c");
    let roles: Vec<BoxRole> = c.children.iter().map(|c| c.fragment.role).collect();
    assert_eq!(
        roles,
        [
            BoxRole::AnonymousBlock,
            BoxRole::Other,
            BoxRole::AnonymousBlock,
            BoxRole::Other,
            BoxRole::Other,
        ]
    );
    assert_eq!(laid.children("c")[1], "span");
    assert_eq!(laid.children("c")[4], "block");
    // Side by side, the text fitting on one line each.
    let xs: Vec<f32> = c.children.iter().map(|c| c.offset.x).collect();
    assert!(xs.is_sorted(), "{xs:?}");
    assert!(c.children.iter().all(|c| c.offset.y == 0.0));
}

#[test]
fn a_flex_container_with_only_text_has_one_anonymous_item() {
    let laid = lay_out(
        "<div id=c style='display: flex; justify-content: flex-end; width: 400px'>text</div>
         <div id=empty style='display: flex'>  </div>
         <div id=pre style='display: flex; white-space: pre'> <b>a</b>&#9;\n<b>b</b> </div>",
    );
    let c = laid.fragment("c");
    assert!(c.lines.is_empty(), "its text is in an item");
    assert_eq!(c.children.len(), 1);
    let item = &c.children[0];
    assert_eq!(item.fragment.role, BoxRole::AnonymousBlock);
    assert_eq!(item.fragment.lines.len(), 1);
    // Sized to its text, at the end.
    assert_eq!(item.offset.x + item.fragment.size.width, 400.0);
    assert!(item.fragment.size.width < 100.0);
    assert!(laid.fragment("empty").children.is_empty());
    // Only white space makes no item even where it doesn't collapse.
    assert_eq!(laid.fragment("pre").children.len(), 2);
}

#[test]
fn order_lays_items_out_and_makes_their_fragments_in_order() {
    let laid = lay_out(&format!(
        "{BODY}<div id=c style='display: flex'>
           <div id=a style='width: 10px; order: 2'></div>
           <div id=b style='width: 20px; order: 1'></div>
           <div id=d style='width: 30px'></div>
           <div id=e style='width: 40px; order: 1'></div>
           <div id=f style='width: 50px; order: -1'></div>
         </div>"
    ));
    // Equal ones stay in document order.
    assert_eq!(laid.children("c"), ["f", "d", "b", "e", "a"]);
    assert_eq!(laid.rect("f").x, 0.0);
    assert_eq!(laid.rect("d").x, 50.0);
    assert_eq!(laid.rect("b").x, 80.0);
    assert_eq!(laid.rect("e").x, 100.0);
    assert_eq!(laid.rect("a").x, 140.0);
}

#[test]
fn order_only_applies_to_flex_items() {
    let laid = lay_out(&format!(
        "{BODY}<div id=c>
           <div id=a style='order: 2'></div><div id=b style='order: 1'></div>
         </div>"
    ));
    assert_eq!(laid.children("c"), ["a", "b"]);
    assert_eq!(laid.rect("b").y, 10.0);
}

#[test]
fn baseline_alignment_lines_up_the_first_lines() {
    let laid = lay_out(
        "<style>body { margin: 0; font: 16px/1 test }</style>
         <div style='display: flex; align-items: baseline'>
           <div id=small>small</div>
           <div id=big style='font-size: 40px; padding-top: 3px'>big</div>
           <div id=nested><p style='margin: 5px 0 0'>nested</p><p>second</p></div>
           <div id=bottom style='align-self: flex-start'>top</div>
         </div>",
    );
    let first = |id: &str| baselines(&laid, id)[0];
    let big = first("big");
    assert_eq!(first("small"), big);
    // A block's first baseline is its first child's.
    assert_eq!(first("nested"), big);
    assert_ne!(first("bottom"), big);
    assert_eq!(laid.rect("big").y, 0.0);
    assert!(laid.rect("small").y > 0.0);
}

/// An item whose only line holds out-of-flow boxes has no first baseline
/// (the line is empty, as in Blink): it is aligned as one with no line at
/// all, a float's, by its synthesized one.
#[test]
fn an_empty_line_gives_no_baseline() {
    let laid = lay_out(
        "<style>body { margin: 0; font: 20px/1 Ahem } .f { display: flex; align-items: baseline }</style>
         <div class=f><div id=a>X</div><div style='height: 50px; position: relative'><b style='position: absolute'>B</b></div></div>
         <div id=floats class=f><div id=b>X</div><div style='height: 50px'><div style='float: left; width: 5px; height: 5px'></div></div></div>",
    );
    let below_floats = laid.rect("b").y - laid.rect("floats").y;
    assert!(below_floats > 0.0);
    assert_eq!(laid.rect("a").y, below_floats);
}

#[test]
fn baselines_come_from_the_lines_at_the_final_width() {
    // Ahem: each glyph a 20px square (40px for the big one), its baseline
    // 0.8em down. A definite size isn't measured again when laid out: the
    // lines it was last broken into are its min-content ones.
    let laid = lay_out(
        "<style>body { margin: 0 } b { font-size: 40px }</style>
         <div style='display: flex; align-items: baseline; font: 20px/1 Ahem'>
           <div id=one-line style='height: 60px'>X <b>X</b></div>
           <div id=padded style='width: 100px; height: 60px; padding-right: 100px'>X X X X <b>X</b></div>
           <div id=small>X</div>
         </div>",
    );
    // One line, 80px wide, with the big glyph's baseline.
    assert_eq!(laid.rect("one-line").y, 0.0);
    // Its first line is "X X" in 100px, not all of it in 200px.
    assert_eq!(laid.rect("padded").y, 16.0);
    let first = |id: &str| baselines(&laid, id)[0];
    assert_eq!(first("one-line"), 32.0);
    assert_eq!(first("padded"), 32.0);
    assert_eq!(first("small"), 32.0);
}

#[test]
fn items_dont_shrink_below_their_min_content() {
    let laid = lay_out(
        "<style>body { margin: 0; font: 16px/1 test }</style>
         <div style='display: flex; width: 10px'>
           <div id=word>unbreakable</div>
           <div id=zero style='min-width: 0'>unbreakable</div>
         </div>
         <div style='display: flex'><div id=room>unbreakable</div></div>",
    );
    // `min-width: auto` is the word's width, as with room for it.
    let word = laid.rect("room").width;
    assert!(word > 10.0);
    assert_eq!(laid.rect("word").width, word);
    assert_eq!(laid.rect("zero").width, 0.0);
}

#[test]
fn a_flex_root_and_body() {
    let laid = lay_out(
        "<html id=html style='display: flex'><body id=body style='margin: 0; width: 20px'>
           <div id=a style='height: 10px'></div>",
    );
    // Sized to its one item, `<body>`.
    assert_eq!(laid.rect("html"), RectF::new(0.0, 0.0, 800.0, 10.0));
    assert_eq!(laid.rect("body"), RectF::new(0.0, 0.0, 20.0, 10.0));
    assert_eq!(laid.rect("a").width, 20.0);
}

#[test]
fn a_padded_items_text_wraps_in_its_content_box() {
    // 100px for "XX XX X" (140px): two lines, whatever the padding.
    let laid = lay_out(
        "<style>body { margin: 0 }</style>
         <div id=c style='display: flex; font: 20px/1 Ahem'>
           <div id=item style='width: 100px; padding-left: 60px'>XX XX X</div>
         </div>
         <div style='display: flex; align-items: baseline; font: 20px/1 Ahem'>
           <div id=bordered style='width: 100px; border: solid; border-width: 5px 0 0 60px'>XX XX X</div>
           <div id=plain>X</div>
         </div>",
    );
    assert_eq!(laid.rect("item"), RectF::new(0.0, 0.0, 160.0, 40.0));
    assert_eq!(laid.rect("c").height, 40.0);
    // Borders too; and the first baseline is below the top one.
    assert_eq!(laid.rect("bordered"), RectF::new(0.0, 40.0, 160.0, 45.0));
    assert_eq!(
        baselines(&laid, "bordered")[0],
        baselines(&laid, "plain")[0]
    );
    assert_eq!(laid.rect("plain").y, 45.0);
}

#[test]
fn intrinsic_sizes_of_flex_items() {
    let laid = lay_out(
        "<style>body { margin: 0; font: 20px/1 Ahem }</style>
         <div style='display: flex'>
           <div id=content style='flex-basis: content; width: 200px'>XX</div>
           <div id=sized style='width: 200px'>XX</div>
           <div id=min style='width: min-content'>XX XXX</div>
         </div>
         <div style='display: flex; flex-direction: column; align-items: flex-start'>
           <div id=max style='width: max-content'>XX XXX</div>
         </div>",
    );
    // `content` is the item's content, not its `width`.
    assert_eq!(laid.rect("content").width, 40.0);
    assert_eq!(laid.rect("sized").width, 200.0);
    assert_eq!(laid.rect("min").width, 60.0);
    assert_eq!(laid.rect("max").width, 120.0);
}

/// A flex item's automatic minimum size is its content's, but 0 for a
/// scroll container: `overflow: clip`, which isn't one, keeps it, where
/// `hidden` lets the item shrink to nothing.
#[test]
fn a_clipping_item_keeps_its_minimum_size() {
    let laid = lay_out(
        "<style>html, body { margin: 0 }</style>
         <div style='display: flex; width: 50px'>
           <div id=clip style='overflow: clip'><div style='width: 100px; height: 10px'></div></div>
         </div>
         <div style='display: flex; width: 50px'>
           <div id=hidden style='overflow: hidden'><div style='width: 100px; height: 10px'></div></div>
         </div>",
    );
    assert_eq!(laid.rect("clip").width, 100.0);
    assert_eq!(laid.rect("hidden").width, 50.0);
}

/// A `-webkit-box` lays its items out as a flex container by its own
/// longhands (Blink's `FlexLayoutAlgorithm` for `-webkit-box`):
/// `-webkit-box-flex` grows and shrinks an item (its `flex-grow` doesn't),
/// `-webkit-box-ordinal-group` orders them, `-webkit-box-orient` and
/// `-webkit-box-direction` say which way, `-webkit-box-pack` packs them and
/// `-webkit-box-align` aligns all of them, whatever their `align-self`.
#[test]
fn webkit_box() {
    let laid = lay_out(
        "<style>body { margin: 0; font: 20px/1 Ahem } .box { display: -webkit-box; width: 300px; height: 100px }</style>
         <div class=box><div id=a style='-webkit-box-flex: 1'>X</div><div id=b style='-webkit-box-flex: 2'>X</div><div id=c style='flex-grow: 5'>X</div></div>
         <div class=box style='-webkit-box-orient: vertical'><div id=d style='-webkit-box-ordinal-group: 2'>X</div><div id=e>X</div></div>
         <div class=box style='-webkit-box-direction: reverse; -webkit-box-align: center'><div id=f style='align-self: end'>X</div><div id=g>X</div></div>
         <div class=box style='-webkit-box-pack: justify; -webkit-box-align: start'><div id=h>X</div><div id=i>X</div></div>
         <div class=box style='-webkit-box-pack: center'><div id=j>X</div></div>
         <div class=box style='-webkit-box-pack: end'><div id=k>X</div></div>
         <div class=box style='flex-wrap: wrap'><div id=l style='width: 200px'>X</div><div id=m style='width: 200px'>X</div></div>",
    );
    let at = |id: &str| {
        let r = laid.rect(id);
        (r.x, r.y % 100.0, r.width, r.height)
    };
    // 240px left: a third and two to `a` and `b`; `c` doesn't grow, and
    // all are stretched.
    assert_eq!(at("a"), (0.0, 0.0, 100.0, 100.0));
    assert_eq!(at("b"), (100.0, 0.0, 180.0, 100.0));
    assert_eq!(at("c"), (280.0, 0.0, 20.0, 100.0));
    // A column, `d` after `e`.
    assert_eq!(at("e"), (0.0, 0.0, 300.0, 20.0));
    assert_eq!(at("d"), (0.0, 20.0, 300.0, 20.0));
    // From the right, centered whatever `align-self` says.
    assert_eq!(at("f"), (280.0, 40.0, 20.0, 20.0));
    assert_eq!(at("g"), (260.0, 40.0, 20.0, 20.0));
    assert_eq!(at("h"), (0.0, 0.0, 20.0, 20.0));
    assert_eq!(at("i"), (280.0, 0.0, 20.0, 20.0));
    assert_eq!(at("j").0, 140.0);
    assert_eq!(at("k").0, 280.0);
    // One line, whatever `flex-wrap` says: side by side, past the end
    // (with no `-webkit-box-flex` they don't shrink either).
    assert_eq!(at("l"), (0.0, 0.0, 200.0, 100.0));
    assert_eq!(at("m"), (200.0, 0.0, 200.0, 100.0));
}

/// A `-webkit-box`'s items have no automatic minimum size: they shrink
/// past their content. Its inline children aren't blockified (Blink's
/// `BlockifiesChildren` leaves `-webkit-box` out): with its text they go
/// into one anonymous item.
#[test]
fn webkit_box_items() {
    let laid = lay_out(
        "<style>body { margin: 0; font: 20px/1 Ahem }</style>
         <div style='display: -webkit-box; width: 50px'><div id=a style='-webkit-box-flex: 1'>XXX</div><div id=b style='-webkit-box-flex: 1'>XXX</div></div>
         <div id=box style='display: -webkit-box; width: 300px'><span id=s>X</span> X</div>",
    );
    assert_eq!(laid.rect("a").width, 25.0);
    assert_eq!(laid.rect("b").width, 25.0);
    // One item, as wide as `X X`, its lines its own.
    let items = &laid.fragment("box").children;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].fragment.size.width, 60.0);
}

/// An item of a horizontal `-webkit-box` with an `auto` width is
/// `fit-content` (Blink's flex base size for `-webkit-box`): its text
/// wraps in the box. A vertical box's isn't narrowed, nor a width of its
/// own, nor an item of a `flex` container.
#[test]
fn webkit_box_items_fit_their_content_in() {
    let laid = lay_out(
        "<style>body { margin: 0; font: 20px/1 Ahem }</style>
         <div style='display: -webkit-box; width: 100px'><div id=a>XX XX XX XX</div></div>
         <div style='display: -webkit-box; width: 100px'><div id=b style='width: 200px'>XX XX XX XX</div></div>
         <div style='display: flex; width: 100px'><div id=c style='flex-shrink: 0'>XX XX XX XX</div></div>",
    );
    assert_eq!(laid.rect("a").size(), SizeF::new(100.0, 40.0));
    assert_eq!(laid.rect("b").width, 200.0);
    assert_eq!(laid.rect("c").width, 220.0);
}

/// Each column below is in a float, which asks it for its width before it
/// lays it out (a float laid out on its own sizes itself as it lays out).
const AHEM: &str =
    "<style>body { margin: 0; font: 20px/1 Ahem } .f { float: left; clear: left }</style>";
const COLUMN: &str = "display: flex; flex-direction: column";

/// A single-line column flexbox that shrinks to fit is as wide as its
/// widest in-flow item with its margins, plus its own padding and border,
/// within its `min-width` and `max-width`.
#[test]
fn a_column_is_as_wide_as_its_widest_item() {
    let laid = lay_out(&format!(
        "{AHEM}<div class=f><div id=a style='{COLUMN}; padding: 0 3px; border: solid; border-width: 0 2px'>\
         <div>XX XX</div><div style='margin: 0 15px'>XXXX</div>\
         <div style='position: absolute'>XXXXXXXXXX</div>\
         <div style='display: none'>XXXXXXXXXX</div></div></div>\
         <div class=f><div id=b style='display: flex; flex-direction: column-reverse'><div>XX</div><div>XXX</div></div></div>\
         <div class=f><div id=c style='{COLUMN}; min-width: 200px; padding: 0 5px'>XX</div></div>\
         <div class=f><div id=d style='{COLUMN}; max-width: 50px; padding: 0 5px'>XX XX</div></div>\
         <div class=f><div id=e style='{COLUMN}; box-sizing: border-box; max-width: 50px; padding: 0 5px'>XX XX</div></div>"
    ));
    let width = |id: &str| laid.rect(id).width;
    assert_eq!(width("a"), 110.0 + 10.0);
    assert_eq!(width("b"), 60.0);
    assert_eq!(width("c"), 210.0);
    assert_eq!(width("d"), 60.0);
    assert_eq!(width("e"), 50.0);
}

/// Its min-content and max-content widths are its items' widest, and
/// shrink-to-fit lies between them.
#[test]
fn a_columns_content_widths() {
    let items = "<div>XX XX</div><div>XXX</div>";
    let laid = lay_out(&format!(
        "{AHEM}<div class=f style='width: min-content'><div id=min style='{COLUMN}'>{items}</div></div>\
         <div class=f style='width: max-content'><div id=max style='{COLUMN}'>{items}</div></div>\
         <div style='width: 70px'><div class=f><div id=fit style='{COLUMN}'>{items}</div></div></div>\
         <div style='width: 30px'><div class=f><div id=narrow style='{COLUMN}'>{items}</div></div></div>\
         <div style='width: 300px'><div class=f><div id=wide style='{COLUMN}'>{items}</div></div></div>"
    ));
    let width = |id: &str| laid.rect(id).width;
    assert_eq!(width("min"), 60.0);
    assert_eq!(width("max"), 100.0);
    assert_eq!(width("fit"), 70.0);
    assert_eq!(width("narrow"), 60.0);
    assert_eq!(width("wide"), 100.0);
}

/// An item's own width, sizing keyword, `min-width` and `max-width` give
/// its contribution.
#[test]
fn a_column_items_own_width() {
    let column = |id: &str, item: &str| {
        format!(
            "<div class=f><div id={id} style='{COLUMN}'><div style='{item}'>XX XX XX</div></div></div>"
        )
    };
    let laid = lay_out(&format!(
        "{AHEM}{}{}{}{}{}{}{}",
        column("a", "width: min-content"),
        column("b", "max-width: 30px"),
        column("c", "min-width: 230px"),
        column("d", "width: 50px; padding: 0 5px"),
        column("e", "box-sizing: border-box; width: 50px; padding: 0 5px"),
        column("f", "padding: 0 5px; border: solid; border-width: 0 1px"),
        column("g", "margin: 0 15px"),
    ));
    let width = |id: &str| laid.rect(id).width;
    assert_eq!(width("a"), 40.0);
    assert_eq!(width("b"), 30.0);
    assert_eq!(width("c"), 230.0);
    assert_eq!(width("d"), 60.0);
    assert_eq!(width("e"), 50.0);
    assert_eq!(width("f"), 172.0);
    assert_eq!(width("g"), 190.0);
    // In a min-content width, an item that is `max-content` wide.
    let laid = lay_out(&format!(
        "{AHEM}<div class=f style='width: min-content'><div id=a style='{COLUMN}'>\
         <div style='width: max-content'>XX XX XX</div></div></div>"
    ));
    assert_eq!(laid.rect("a").width, 160.0);
}

/// A percentage-width image in a column in a definite width: a min-content
/// width compresses it, so it narrows the column to the room, not widens it
/// to its own width. (The image's own column, with an item with a ratio,
/// goes to taffy; the one around it doesn't.)
#[test]
fn a_column_fits_a_compressible_item_in() {
    let laid = lay_out(&format!(
        "{AHEM}<div style='width: 195px'><div class=f><div id=a style='{COLUMN}'><div style='{COLUMN}'>\
         <svg id=s width=1280 height=720 viewBox='0 0 1280 720' style='width: 100%; height: auto'></svg>\
         </div></div></div></div>"
    ));
    assert_eq!(laid.rect("a").width, 195.0);
    assert_eq!(
        laid.rect("s").size(),
        SizeF::new(195.0, 195.0 * 720.0 / 1280.0)
    );
}

/// What sizes a column's width through heights: lines that wrap; and what
/// gives it without its items: a width of its own. A row isn't a column.
/// An absolutely positioned one is measured for its height too.
#[test]
fn what_a_columns_width_needs_heights_for() {
    let laid = lay_out(&format!(
        "{AHEM}<div class=f><div id=wrap style='{COLUMN}; flex-wrap: wrap; height: 40px'><div>XXXX</div><div>XX</div><div>XXX</div></div></div>\
         <div class=f><div id=sized style='{COLUMN}; width: 30px'>XXXX</div></div>\
         <div class=f><div id=row style='display: flex'><div>XX</div><div>XX</div></div></div>\
         <div style='display: grid; grid-template-columns: min-content'><div id=grid style='{COLUMN}; width: 30px'>XXXX</div></div>         <div style='position: relative'><div id=abs style='position: absolute; {COLUMN}'><div>XX XX</div><div>XXX</div></div></div>"
    ));
    let width = |id: &str| laid.rect(id).width;
    assert_eq!(width("wrap"), 140.0);
    assert_eq!(width("sized"), 30.0);
    assert_eq!(width("row"), 80.0);
    assert_eq!(width("grid"), 30.0);
    assert_eq!(laid.rect("abs").size(), SizeF::new(100.0, 40.0));
}

/// A row that shrinks to fit in a definite room is as wide as the room,
/// between its min-content and max-content widths, as in Chrome: in a
/// float or an absolutely positioned box, or one itself, in an
/// inline-block, at `width: fit-content` and as a column's item. Items
/// without flex base sizes of their own (`flex-basis: 0`) still keep it
/// to their min-content widths; its margins take room, and its padding is
/// in its width.
#[test]
fn a_row_shrinks_to_fit_its_room() {
    let row = "<div>XX XX XX XX</div>";
    let room =
        |inner: String| format!("<div style='width: 100px; display: flow-root'>{inner}</div>");
    let laid = lay_out(&format!(
        "{AHEM}{}{}{}{}{}{}{}{}{}{}{}",
        room(format!("<div class=f><div id=float style='display: flex'>{row}</div></div>")),
        room(format!("<div id=floated class=f style='display: flex'>{row}</div>")),
        room(format!(
            "<div style='position: relative'><div id=positioned style='position: absolute; display: flex'>{row}</div></div>"
        )),
        room(format!(
            "<div style='position: relative'><div style='position: absolute'>\
             <div id=abs style='display: flex'>{row}</div></div></div>"
        )),
        room(format!("<div style='display: inline-block'><div id=inline style='display: flex'>{row}</div></div>")),
        room(format!("<div id=fit style='display: flex; width: fit-content'>{row}</div>")),
        room(format!("<div class=f><div style='{COLUMN}'><div id=column style='display: flex'>{row}</div></div></div>")),
        room(
            "<div class=f><div id=basis style='display: flex'>\
             <div style='flex: 1 1 0px'>XX XX</div><div style='flex: 1 1 0px'>XX XX</div></div></div>"
                .to_owned()
        ),
        room(format!("<div class=f><div id=edges style='display: flex; padding: 0 5px; margin: 0 10px'>{row}</div></div>")),
        room("<div class=f><div id=wide style='display: flex'><div>XXXXXXXX</div></div></div>".to_owned()),
        room(format!("<div class=f><div id=reverse style='display: flex; flex-direction: row-reverse'>{row}</div></div>")),
    ));
    let width = |id: &str| laid.rect(id).width;
    for id in [
        "float",
        "floated",
        "positioned",
        "abs",
        "inline",
        "fit",
        "column",
        "basis",
        "reverse",
    ] {
        assert_eq!(width(id), 100.0, "#{id}");
    }
    assert_eq!(width("edges"), 80.0);
    assert_eq!(width("wide"), 160.0);
}

/// A column whose lines wrap, which taffy sizes, shrinks to fit a
/// definite room too, in a float or as one.
#[test]
fn a_column_that_wraps_shrinks_to_fit_its_room() {
    let items = "<div>XX XX XX XX</div><div>XX</div>";
    let column = format!("{COLUMN}; flex-wrap: wrap; height: 40px");
    let laid = lay_out(&format!(
        "{AHEM}<div style='width: 100px; display: flow-root'>\
         <div class=f><div id=float style='{column}'>{items}</div></div>\
         <div id=floated class=f style='{column}'>{items}</div></div>"
    ));
    assert_eq!(laid.rect("float").width, 100.0);
    assert_eq!(laid.rect("floated").width, 100.0);
}

/// A row or a grid that shrinks to fit is measured for its width
/// without its items' lines: each is broken into lines only at the width
/// it gets, twice (for its height there, and laid out).
#[test]
fn what_shrinks_to_fit_breaks_its_lines_at_its_width() {
    let page = |container: &str| {
        lay_out(&format!(
            "{AHEM}<div style='width: 100px'><div class=f style='{container}'>\
             <div>XX XX XX</div><div>XX XX</div></div></div>"
        ))
        .layout
        .lines_broken
    };
    assert_eq!(page("display: flex"), 4);
    assert_eq!(page("display: grid; grid-template-columns: 1fr 1fr"), 4);
}

/// An editor's page: a row whose item is a column of paragraphs. The row
/// asks the column for its min-content width, which needs no line of its
/// paragraphs: each is broken into lines once, at the width it gets.
#[test]
fn a_column_in_a_row_breaks_its_paragraphs_once() {
    let page = |app: &str| {
        lay_out(&format!(
            "<style>body {{ margin: 0; font: 20px/1 Ahem; display: flex }}</style>\
             <div style='{COLUMN}; flex: 1{app}'><div style='flex: 1; overflow: auto'>\
             <p>XX XX XX</p><p>XX XX XX</p><p>XX XX XX</p></div></div>"
        ))
        .layout
        .lines_broken
    };
    assert_eq!(page(""), 3);
    assert_eq!(page("; flex-direction: column-reverse"), 3);
    // Lines that wrap go through taffy's flex algorithm, which lays each
    // paragraph out at its min-content width too.
    assert_eq!(page("; flex-wrap: wrap"), 6);
}
