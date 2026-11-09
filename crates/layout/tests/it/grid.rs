//! Grid layout on parsed, styled documents: the tracks, where items are
//! placed, and in which order.

use gfx::RectF;

use crate::page::lay_out;

const BODY: &str = "<style>body { margin: 0 } .g > div { min-height: 10px }</style>";

#[test]
fn fixed_and_flexible_tracks() {
    let laid = lay_out(&format!(
        "{BODY}<div style='display: grid; width: 400px;
           grid-template-columns: 100px 1fr 2fr; grid-template-rows: 20px 30px' class=g>
           <div id=a></div><div id=b></div><div id=c></div><div id=d></div>
         </div>"
    ));
    // 300px left for 3fr.
    assert_eq!(laid.rect("a"), RectF::new(0.0, 0.0, 100.0, 20.0));
    assert_eq!(laid.rect("b"), RectF::new(100.0, 0.0, 100.0, 20.0));
    assert_eq!(laid.rect("c"), RectF::new(200.0, 0.0, 200.0, 20.0));
    // Auto-placed on the next row.
    assert_eq!(laid.rect("d"), RectF::new(0.0, 20.0, 100.0, 30.0));
}

/// A template of more columns than 16 bits count goes to taffy saturated,
/// and taffy lays out no more than its 10000 tracks: Chrome lays out all
/// 65536 (up to Blink's million).
#[test]
fn a_template_past_taffys_tracks() {
    let row = vec!["a"; 65_536].join(" ");
    let laid = lay_out(&format!(
        "{BODY}<div style='display: grid; grid-auto-columns: 1px; width: max-content;
           grid-template-areas: \"{row}\"' class=g><div id=a style='grid-area: a'></div></div>"
    ));
    assert_eq!(laid.rect("a").width, 10_000.0);
}

#[test]
fn repeat_minmax_and_gaps() {
    let laid = lay_out(&format!(
        "{BODY}<div style='display: grid; width: 330px; gap: 5px 10px;
           grid-template-columns: repeat(auto-fill, minmax(100px, 1fr))' class=g>
           <div id=a></div><div id=b></div><div id=c></div><div id=d></div>
         </div>
         <div style='display: grid; width: 300px; grid-template-columns: repeat(2, 50px 1fr)' class=g>
           <div id=e></div><div id=f></div><div id=h></div><div id=i></div>
         </div>"
    ));
    // Three 100px tracks fit with their gaps; they share the 10px left.
    let close = |a: f32, b: f32| (a - b).abs() < 0.01;
    assert!(close(laid.rect("a").width, 103.333), "{:?}", laid.rect("a"));
    assert!(close(laid.rect("c").x, 226.667), "{:?}", laid.rect("c"));
    assert_eq!(laid.rect("d").y, 15.0);
    // 50px 1fr 50px 1fr: 100px each for the flexible ones.
    assert_eq!(laid.rect("f").x, 50.0);
    assert_eq!(laid.rect("h").x, 150.0);
    assert_eq!(laid.rect("i").width, 100.0);
}

#[test]
fn intrinsic_tracks_size_to_their_items() {
    let laid = lay_out(
        "<style>body { margin: 0; font: 20px/1 Ahem }</style>
         <div style='display: grid; grid-template-columns: min-content max-content auto fit-content(50px)'>
           <div id=min>XX XXX</div><div id=max>XX XXX</div><div id=auto>X</div>
           <div id=fit>XX XXX</div>
         </div>",
    );
    assert_eq!(laid.rect("min").width, 60.0);
    assert_eq!(laid.rect("max").width, 120.0);
    // Up to 50px, then the min-content width: the longest word.
    assert_eq!(laid.rect("fit").width, 60.0);
    // `auto` takes what is left.
    assert_eq!(laid.rect("auto").width, 800.0 - 60.0 - 120.0 - 60.0);
}

/// A grid with flexible tracks that shrinks to fit in a definite room is
/// as wide as the room, between its min-content and max-content widths,
/// as in Chrome, and its `fr` tracks share the room evenly, whatever
/// their items' widths: in a float or an
/// absolutely positioned box, or one itself, and as a column flexbox's
/// item.
#[test]
fn flexible_tracks_shrink_to_fit_the_room() {
    let grid = |id: &str| {
        format!(
            "<div id={id} style='display: grid; grid-template-columns: 1fr 1fr; column-gap: 10px'>\
             <div id={id}-a>XX XX XX</div><div>XX</div></div>"
        )
    };
    let room =
        |inner: String| format!("<div style='width: 150px; display: flow-root'>{inner}</div>");
    let laid = lay_out(&format!(
        "<style>body {{ margin: 0; font: 20px/1 Ahem }} #floated {{ float: left }} #positioned {{ position: absolute }}</style>{}{}{}{}{}{}",
        room(format!("<div style='float: left'>{}</div>", grid("float"))),
        room(grid("floated")),
        room(format!("<div style='position: relative'>{}</div>", grid("positioned"))),
        room(format!(
            "<div style='position: relative'><div style='position: absolute'>{}</div></div>",
            grid("abs")
        )),
        room(format!(
            "<div style='float: left'><div style='display: flex; flex-direction: column'>{}</div></div>",
            grid("column")
        )),
        room(
            "<div style='float: left'><div id=narrow style='display: grid; grid-template-columns: 1fr 1fr; column-gap: 10px'>\
             <div>XXXX XX</div><div>XXXX XX</div></div></div>"
                .to_owned()
        ),
    ));
    for id in ["float", "floated", "positioned", "abs", "column"] {
        assert_eq!(laid.rect(id).width, 150.0, "#{id}");
        assert_eq!(laid.rect(&format!("{id}-a")).width, 70.0, "#{id}");
    }
    // No narrower than its min-content width.
    assert_eq!(laid.rect("narrow").width, 170.0);
}

/// `fit-content()` of a `calc()` with a percentage: of the grid's width.
#[test]
fn fit_content_of_a_calc() {
    let laid = lay_out(
        "<style>body { margin: 0; font: 20px/1 Ahem }</style>
         <div style='display: grid; width: 400px;
           grid-template-columns: fit-content(calc(20% + 20px)) 1fr'>
           <div id=fit>XX XXX</div><div></div>
         </div>",
    );
    // Up to 100px: between the item's min-content and max-content widths.
    assert_eq!(laid.rect("fit").width, 100.0);
}

#[test]
fn items_placed_by_line_number_and_span() {
    let laid = lay_out(&format!(
        "{BODY}<div style='display: grid; width: 300px;
           grid-template-columns: repeat(3, 100px); grid-auto-rows: 10px' class=g>
           <div id=a style='grid-column: 2 / span 2'></div>
           <div id=b style='grid-row: 2; grid-column: -2'></div>
           <div id=c style='grid-area: 3 / 1 / 5 / 2'></div>
           <div id=d></div>
         </div>"
    ));
    assert_eq!(laid.rect("a"), RectF::new(100.0, 0.0, 200.0, 10.0));
    // Line -2 is the third column's start.
    assert_eq!(laid.rect("b"), RectF::new(200.0, 10.0, 100.0, 10.0));
    assert_eq!(laid.rect("c"), RectF::new(0.0, 20.0, 100.0, 20.0));
    // Sparse: the cursor went past the first cell, placing `#a`, and
    // doesn't go back (css-grid 2, "grid item placement algorithm").
    assert_eq!(laid.rect("d"), RectF::new(0.0, 10.0, 100.0, 10.0));
}

#[test]
fn auto_flow_column_and_dense() {
    let laid = lay_out(&format!(
        "{BODY}<div style='display: grid; grid-template-rows: 10px 10px;
           grid-auto-flow: column; grid-auto-columns: 50px' class=g>
           <div id=a></div><div id=b></div><div id=c></div>
         </div>
         <div style='display: grid; width: 300px; grid-template-columns: repeat(3, 100px);
           grid-auto-flow: dense; grid-auto-rows: 10px' class=g>
           <div id=wide style='grid-column: span 2'></div><div id=wider style='grid-column: span 3'></div>
           <div id=filler></div>
         </div>"
    ));
    assert_eq!(laid.rect("b"), RectF::new(0.0, 10.0, 50.0, 10.0));
    assert_eq!(laid.rect("c"), RectF::new(50.0, 0.0, 50.0, 10.0));
    // `dense` fills the hole after `#wide`.
    assert_eq!(laid.rect("filler").y, 20.0);
    assert_eq!(laid.rect("filler").x, 200.0);
}

#[test]
fn items_align_in_their_areas() {
    let laid = lay_out(&format!(
        "{BODY}<div style='display: grid; width: 200px; grid-template-columns: 100px 100px;
           grid-template-rows: 50px; justify-items: center; align-items: end' class=g>
           <div id=a style='width: 20px; height: 10px'></div>
           <div id=b style='width: 20px; height: 10px; justify-self: start; align-self: start'></div>
         </div>
         <div style='display: grid; width: 200px; grid-template-columns: 100px; grid-template-rows: 50px;
           place-content: center' class=g>
           <div id=c></div>
         </div>"
    ));
    assert_eq!(laid.rect("a"), RectF::new(40.0, 40.0, 20.0, 10.0));
    assert_eq!(laid.rect("b"), RectF::new(100.0, 0.0, 20.0, 10.0));
    // The grid centered in its container.
    assert_eq!(laid.rect("c").x, 50.0);
}

/// `normal` on the item is its own value, not `auto`: it doesn't take the
/// container's `justify-items` and `align-items`, and stretches.
#[test]
fn normal_self_alignment_is_not_auto() {
    let laid = lay_out(&format!(
        "{BODY}<div style='display: grid; grid-template: 50px / 100px 100px;
           justify-items: center; align-items: center' class=g>
           <div id=normal style='justify-self: normal; align-self: normal'></div>
           <div id=auto></div>
         </div>"
    ));
    assert_eq!(laid.rect("normal"), RectF::new(0.0, 0.0, 100.0, 50.0));
    assert_eq!(laid.rect("auto"), RectF::new(150.0, 20.0, 0.0, 10.0));
}

#[test]
fn grid_items_are_in_order_and_text_is_an_anonymous_item() {
    let laid = lay_out(&format!(
        "{BODY}<div id=g style='display: grid; grid-template-columns: repeat(3, 50px)' class=g>
           text<div id=a style='order: 1'></div><div id=b></div>
         </div>"
    ));
    assert_eq!(laid.children("g"), ["-", "b", "a"]);
    assert_eq!(laid.rect("b").x, 50.0);
    assert_eq!(laid.rect("a").x, 100.0);
}

#[test]
fn auto_fit_collapses_empty_tracks_and_auto_fill_keeps_them() {
    let laid = lay_out(&format!(
        "{BODY}<div style='display: grid; width: 300px;
           grid-template-columns: repeat(auto-fill, minmax(100px, 1fr))' class=g>
           <div id=fill></div>
         </div>
         <div style='display: grid; width: 300px;
           grid-template-columns: repeat(auto-fit, minmax(100px, 1fr))' class=g>
           <div id=fit></div>
         </div>"
    ));
    // Three tracks either way; `auto-fit` collapses the two empty ones, and
    // the one left takes their room.
    assert_eq!(laid.rect("fill").width, 100.0);
    assert_eq!(laid.rect("fit").width, 300.0);
}

#[test]
fn a_flexible_tracks_minimum_is_its_content() {
    let laid = lay_out(
        "<style>body { margin: 0; font: 20px/1 Ahem }</style>
         <div style='display: grid; width: 100px; grid-template-columns: 1fr 1fr'>
           <div id=word>XXXXX</div><div id=other></div>
         </div>",
    );
    // `1fr` is `minmax(auto, 1fr)`: no narrower than the word.
    assert_eq!(laid.rect("word").width, 100.0);
    assert_eq!(laid.rect("other").x, 100.0);
}

#[test]
fn items_placed_by_line_names() {
    let laid = lay_out(&format!(
        "{BODY}<div style='display: grid; width: 350px; grid-auto-rows: 10px;
           grid-template-columns: [start] 50px [mid] repeat(3, [col] 100px) [end]' class=g>
           <div id=mid style='grid-column: mid / end'></div>
           <div id=second-col style='grid-column: col 2'></div>
           <div id=span style='grid-column: start / span col 2'></div>
           <div id=span-from-mid style='grid-column: mid / span col 2'></div>
         </div>"
    ));
    assert_eq!(laid.rect("mid"), RectF::new(50.0, 0.0, 300.0, 10.0));
    // The second line named `col`: the third column's start.
    assert_eq!(laid.rect("second-col").x, 150.0);
    // Up to the second `col` line after the start: counted from there,
    // not from the grid's first line.
    assert_eq!(laid.rect("span").width, 150.0);
    assert_eq!(laid.rect("span-from-mid").width, 200.0);
}

#[test]
fn items_placed_in_areas() {
    let laid = lay_out(&format!(
        "{BODY}<div style='display: grid; width: 300px;
           grid-template: \"head head\" 20px \"nav main\" 40px \"foot foot\" 10px / 100px 1fr' class=g>
           <div id=main style='grid-area: main'></div>
           <div id=head style='grid-area: head'></div>
           <div id=nav style='grid-row: nav-start / nav-end; grid-column: nav'></div>
           <div id=foot style='grid-area: foot'></div>
         </div>"
    ));
    assert_eq!(laid.rect("head"), RectF::new(0.0, 0.0, 300.0, 20.0));
    assert_eq!(laid.rect("nav"), RectF::new(0.0, 20.0, 100.0, 40.0));
    assert_eq!(laid.rect("main"), RectF::new(100.0, 20.0, 200.0, 40.0));
    assert_eq!(laid.rect("foot"), RectF::new(0.0, 60.0, 300.0, 10.0));
}

#[test]
fn names_only_in_a_repeat() {
    let laid = lay_out(&format!(
        "{BODY}<div style='display: grid; width: 300px; height: 300px;
           grid-template-columns: 50px repeat(auto-fill, [foo] 100px);
           grid-template-rows: repeat(2, 100px [bar])'>
           <div id=a style='grid-area: bar / foo 2'></div>
         </div>"
    ));
    // Columns: 50px, then two of 100px, `foo` before each; rows: `bar`
    // after each.
    assert_eq!(laid.rect("a"), RectF::new(150.0, 100.0, 100.0, 100.0));
}
