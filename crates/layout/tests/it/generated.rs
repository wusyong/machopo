//! `::before` and `::after` (css-pseudo 4, css-content 3): the boxes and
//! text their `content` generates in their element's, as their `display`
//! says; and the table `display` values on them.

use computed_style::PseudoId;
use gfx::{RectF, SizeF};

use crate::page::{Page, lay_out};

const AHEM: &str = "<style>body { margin: 0 } div { font: 20px/1 Ahem; width: 400px }</style>";

/// Inline by default: their text is on the element's lines, before and
/// after its own, as if it were its first and last child.
#[test]
fn before_and_after_are_inline_content() {
    let laid = lay_out(&format!(
        "{AHEM}<style>#d::before {{ content: 'A' }} #d::after {{ content: 'B' attr(data-x) }}</style>
         <div id=d data-x=CD>XX</div>"
    ));
    let lines = laid.lines("d");
    assert_eq!(lines.len(), 1);
    // A, XX, BCD.
    assert_eq!(lines[0].width, 120.0);
    let runs: Vec<_> = laid.fragments["d"].lines[0]
        .runs
        .iter()
        .map(|run| (run.pseudo, run.source.is_some()))
        .collect();
    assert_eq!(
        runs,
        [
            (Some(PseudoId::Before), false),
            (None, true),
            (Some(PseudoId::After), false)
        ]
    );
}

/// No `content` (or `normal`, or `none`): nothing generated.
#[test]
fn no_content_no_box() {
    let laid = lay_out(&format!(
        "{AHEM}<style>#a::before {{ color: red }} #b::before {{ content: none }}
           #c::after {{ content: normal }}</style>
         <div id=a>X</div><div id=b>X</div><div id=c>X</div>"
    ));
    for id in ["a", "b", "c"] {
        assert_eq!(laid.lines(id)[0].width, 20.0, "{id}");
    }
}

/// With a block-level `display`, a box of its own, the element's first or
/// last child: the element's node, marked as its pseudo-element's.
#[test]
fn a_block_pseudo_element_has_its_own_box() {
    let laid = lay_out(&format!(
        "{AHEM}<style>#d::before {{ content: 'A'; display: block; height: 30px }}
           #d::after {{ content: ''; display: inline-block; width: 10px; height: 10px }}</style>
         <div id=d>XX</div>"
    ));
    let d = &laid.fragments["d"];
    let first = &d.children[0].fragment;
    assert_eq!(first.pseudo, Some(PseudoId::Before));
    assert_eq!(first.node, d.node);
    assert_eq!(first.size.height, 30.0);
    assert_eq!(first.lines[0].runs[0].pseudo, Some(PseudoId::Before));
    // The rest is an anonymous block: the text and the inline-block.
    let rest = &d.children[1].fragment;
    assert_eq!(rest.node, None);
    let after = &rest.children[0].fragment;
    assert_eq!(after.pseudo, Some(PseudoId::After));
    assert_eq!(after.size.width, 10.0);
    // Not the element's own box.
    assert_eq!(laid.rect("d").height, 50.0);
}

/// The clearfix: an empty `::after` that clears holds the floats before it,
/// with `display: block`, and with `display: table` (an empty table).
#[test]
fn a_clearfix_holds_its_floats() {
    let laid = lay_out(&format!(
        "{AHEM}<style>.cf::after {{ content: ''; clear: both }} #block::after {{ display: block }}
           #table::after {{ display: table }} #inline::after {{ display: inline }}
           .f {{ float: left; width: 50px; height: 40px }}</style>
         <div id=block class=cf><div class=f></div></div>
         <div id=table class=cf><div class=f></div></div>
         <div id=inline class=cf><div class=f></div></div>"
    ));
    assert_eq!(laid.rect("block").height, 40.0);
    assert_eq!(laid.rect("table").height, 40.0);
    // `clear` doesn't apply to an inline box.
    assert_eq!(laid.rect("inline").height, 0.0);
}

/// The table column values generate no box, an element's or a
/// pseudo-element's (CSS 2 § 17.2), as `none`.
#[test]
fn table_columns_generate_no_box() {
    let laid = lay_out(&format!(
        "{AHEM}<style>#d::before {{ content: 'XX'; display: table-column }}</style>
         <div id=d>X<span id=s style='display: table-column-group'>YY</span></div>"
    ));
    assert_eq!(laid.lines("d")[0].width, 20.0);
    assert!(!laid.rects.contains_key("s"));
}

/// A text control or a `<select>` generates nothing (Blink's
/// `CanHaveGeneratedChildren`).
#[test]
fn form_controls_generate_nothing() {
    let laid = Page::without_shadow_trees(&format!(
        "{AHEM}<style>input::before, select::before {{ content: 'XX' }}</style>
         <input id=i><select id=s></select>"
    ))
    .lay_out();
    for id in ["i", "s"] {
        assert!(laid.fragments[id].lines.is_empty(), "{id}");
        assert!(laid.fragments[id].children.is_empty(), "{id}");
    }
}

/// An inline pseudo-element has an inline box of its own on the line, its
/// background and padding's, marked as it; `attr()` matches an HTML
/// attribute's name whatever case it is written in.
#[test]
fn an_inline_pseudo_element_has_an_inline_box() {
    let laid = lay_out(&format!(
        "{AHEM}<style>#d::before {{ content: attr(DATA-X); padding-left: 10px; background: green }}</style>
         <div id=d data-x=A>X</div>"
    ));
    let line = &laid.fragments["d"].lines[0];
    let boxes: Vec<_> = line
        .boxes
        .iter()
        .map(|b| (b.pseudo, b.rect.width))
        .collect();
    assert_eq!(boxes, [(Some(PseudoId::Before), 30.0)]);
    // `A` after the padding, then `X`.
    assert_eq!(line.rect.width, 50.0);
}

/// An element's geometry is its own boxes', not its pseudo-elements'
/// (which are in them, and have its node): its client rects, its box, and
/// whether it is fixed.
#[test]
fn geometry_leaves_pseudo_elements_out() {
    let laid = lay_out(&format!(
        "{AHEM}<style>#d::before {{ content: ''; display: block; height: 10px; position: fixed }}
           #s::after {{ content: 'XX' }}</style>
         <div id=d>X<span id=s>Y</span></div>"
    ));
    let view = laid.view.clone();
    let id = |name: &str| laid.node(name);
    let scroll = layout::ScrollOffsets::default();
    assert_eq!(
        layout::client_rects(&view, &scroll, &Default::default(), id("d")),
        [RectF::new(0.0, 0.0, 400.0, 20.0)]
    );
    // The span's box covers its `::after` too.
    assert_eq!(
        layout::client_rects(&view, &scroll, &Default::default(), id("s")),
        [RectF::new(20.0, 0.0, 60.0, 20.0)]
    );
    let element_box = layout::element_box(&view, &scroll, &Default::default(), id("d")).unwrap();
    assert_eq!(element_box.border_box, RectF::new(0.0, 0.0, 400.0, 20.0));
    assert!(!layout::is_fixed(&view, id("d")));
}

/// `inline-table` is laid out as `inline-block`: an atomic inline.
#[test]
fn an_inline_table_is_an_atomic_inline() {
    let laid = lay_out(&format!(
        "{AHEM}<div id=d>X<span id=t style='display: inline-table; width: 30px; height: 10px'></span>X</div>"
    ));
    assert_eq!(laid.lines("d")[0].width, 70.0);
    assert_eq!(laid.rect("t").size(), SizeF::new(30.0, 10.0));
}

/// A table part out of a table goes into an anonymous table, block-level:
/// a box between the lines (a row with no cells, as wide as no columns).
#[test]
fn table_parts_go_into_an_anonymous_table() {
    let laid = lay_out(&format!(
        "{AHEM}<div id=d>X<span id=r style='display: table-row; height: 10px'></span>X</div>"
    ));
    assert_eq!(laid.rect("r"), RectF::new(0.0, 20.0, 0.0, 10.0));
    assert_eq!(laid.rect("d").height, 50.0);
}

/// An inline element's box on the line takes the metrics of its own text,
/// not of a pseudo-element's before it.
#[test]
fn an_inline_elements_box_has_its_own_texts_metrics() {
    let laid = lay_out(&format!(
        "{AHEM}<style>#s::before {{ content: 'A'; font-size: 40px }}</style>
         <div id=d><span id=s style='font-size: 10px'>X</span></div>"
    ));
    let line = &laid.fragments["d"].lines[0];
    let span = line.boxes.iter().find(|b| b.pseudo.is_none()).unwrap();
    assert_eq!(span.rect.height, 10.0);
}

/// An inline element's box is its boxes on the lines, not an atomic inline
/// its pseudo-element generates in them.
#[test]
fn an_inline_elements_box_is_not_its_pseudo_elements() {
    let laid = lay_out(&format!(
        "{AHEM}<style>#s::before {{ content: ''; display: inline-block; width: 10px; height: 10px }}</style>
         <div id=d>X<span id=s>Y</span></div>"
    ));
    let view = laid.view.clone();
    let s = laid.node("s");
    let element_box = layout::element_box(
        &view,
        &layout::ScrollOffsets::default(),
        &Default::default(),
        s,
    )
    .unwrap();
    assert!(element_box.block.is_none());
    assert_eq!(element_box.border_box, RectF::new(20.0, 0.0, 30.0, 20.0));
}

/// A pseudo-element's box goes where its `float`, `position` and its
/// element's `display` put it: a float the lines go around, an absolutely
/// positioned box out of the flow, a flex item.
#[test]
fn pseudo_elements_float_are_positioned_and_are_flex_items() {
    let laid = lay_out(&format!(
        "{AHEM}<style>#f::before {{ content: ''; float: left; width: 30px; height: 10px }}
           #a {{ position: relative }}
           #a::before {{ content: 'XX'; position: absolute; top: 50px; left: 5px }}
           #x {{ display: flex }} #x::before {{ content: 'XX' }} #x::after {{ content: ''; width: 10px }}</style>
         <div id=f>X</div><div id=a>X</div><div id=x><span id=s>Y</span></div>"
    ));
    let f = &laid.fragments["f"];
    assert_eq!(f.children[0].fragment.pseudo, Some(PseudoId::Before));
    assert_eq!(laid.lines("f")[0].x, 30.0);
    // Out of the flow: `#a` is as tall as its own text.
    let a = &laid.fragments["a"];
    assert_eq!(laid.rect("a").height, 20.0);
    let positioned = a
        .children
        .iter()
        .find(|child| child.fragment.pseudo == Some(PseudoId::Before))
        .unwrap();
    assert_eq!((positioned.offset.x, positioned.offset.y), (5.0, 50.0));
    // Flex items: the `::before`'s text in an anonymous item, then the
    // span, then the `::after`.
    let x = &laid.fragments["x"];
    let items: Vec<_> = x
        .children
        .iter()
        .map(|child| {
            (
                child.fragment.pseudo,
                child.offset.x,
                child.fragment.size.width,
            )
        })
        .collect();
    assert_eq!(
        items,
        [
            (Some(PseudoId::Before), 0.0, 40.0),
            (None, 40.0, 20.0),
            (Some(PseudoId::After), 60.0, 10.0)
        ]
    );
}
