//! Our changes to parley (the `machopo` branch of our fork, next to machopo at
//! `../third_party/parley`; see its README.machopo.md): inline boxes that are
//! kept with the content next to them (`InlineBoxLineBreak`), which is how
//! an inline element's start and end edges (margin, border, padding) are
//! laid out, and that add nothing to the line's height; an inline box
//! wraps as the span it is in does; and the line after a final forced
//! break is made only where asked (`Layout::set_line_after_final_break`).
//! They are tested here, next to their only user, in the cases machopo's
//! layout produces.
//!
//! Ahem at 20px: every glyph, space included, is 20px wide.

use std::sync::Arc;

use parley::fontique::{Blob, Collection, CollectionOptions, SourceCache};
use parley::{
    FontContext, FontFamily, FontFamilyName, InlineBox, InlineBoxKind, InlineBoxLineBreak, Layout,
    LayoutContext, LineHeight, PositionedLayoutItem, StyleProperty, TextWrapMode, VerticalAlign,
};

static AHEM: &[u8] = include_bytes!("../../../../resources/test_fonts/Ahem.ttf");

/// A box: `(text index, width, line_break)`, 0px high.
type Box = (usize, f32, InlineBoxLineBreak);

use InlineBoxLineBreak::{Auto, KeepWithNext, KeepWithPrevious};

fn layout(text: &str, boxes: &[Box]) -> Layout<u32> {
    layout_with(text, boxes, 1.0, TextWrapMode::Wrap)
}

/// With a `line-height` of `line_height` times the font size.
fn layout_with_line_height(text: &str, boxes: &[Box], line_height: f32) -> Layout<u32> {
    layout_with(text, boxes, line_height, TextWrapMode::Wrap)
}

/// With `line_height` times the font size, all of it in `wrap_mode`.
fn layout_with(
    text: &str,
    boxes: &[Box],
    line_height: f32,
    wrap_mode: TextWrapMode,
) -> Layout<u32> {
    let mut layout = Layout::new();
    build_into(&mut layout, text, boxes, line_height, wrap_mode);
    layout
}

/// `layout_with`'s layout, built into `layout` (a layout used again).
fn build_into(
    layout: &mut Layout<u32>,
    text: &str,
    boxes: &[Box],
    line_height: f32,
    wrap_mode: TextWrapMode,
) {
    let mut collection = Collection::new(CollectionOptions {
        shared: false,
        system_fonts: false,
    });
    collection.register_fonts(Blob::new(Arc::new(AHEM)), None);
    let mut fonts = FontContext {
        collection,
        source_cache: SourceCache::default(),
    };
    let mut cx: LayoutContext<u32> = LayoutContext::new();
    let mut builder = cx.ranged_builder(&mut fonts, text, 1.0, true);
    builder.push_default(StyleProperty::FontFamily(FontFamily::Single(
        FontFamilyName::named("Ahem"),
    )));
    builder.push_default(StyleProperty::FontSize(20.0));
    builder.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(
        line_height,
    )));
    builder.push_default(StyleProperty::TextWrapMode(wrap_mode));
    for (id, &(index, width, line_break)) in boxes.iter().enumerate() {
        builder.push_inline_box(InlineBox {
            id: id as u64,
            kind: InlineBoxKind::InFlow,
            index,
            width,
            height: 0.0,
            baseline: None,
            vertical_align: VerticalAlign::BASELINE,
            line_break,
        });
    }
    builder.build_into(layout, text);
}

/// Each line as its text with `[n]` for box `n`, e.g. `["aaa ", "[0]bbb"]`.
fn lines(text: &str, boxes: &[Box], width: f32) -> Vec<String> {
    let mut layout = layout(text, boxes);
    layout.break_all_lines(Some(width));
    layout
        .lines()
        .map(|line| {
            line.items()
                .map(|item| match item {
                    PositionedLayoutItem::GlyphRun(run) => text[run.run().text_range()].to_owned(),
                    PositionedLayoutItem::InlineBox(b) => format!("[{}]", b.id),
                })
                .collect()
        })
        .collect()
}

#[test]
fn a_start_edge_goes_to_the_next_line_with_its_content() {
    // Room for "aaa " (80) and the box (10), not for "bbb" too.
    assert_eq!(
        lines("aaa bbb", &[(4, 10.0, KeepWithNext)], 100.0),
        ["aaa ", "[0]bbb"]
    );
    // Upstream's default keeps the opportunity after the box.
    assert_eq!(
        lines("aaa bbb", &[(4, 10.0, Auto)], 100.0),
        ["aaa [0]", "bbb"]
    );
}

#[test]
fn an_end_edge_stays_with_the_content_before_it() {
    // "xx aaa" is 120 and fits 125, but not with the 10px box after "aaa".
    assert_eq!(
        lines("xx aaa bbb", &[(6, 10.0, KeepWithPrevious)], 125.0),
        ["xx ", "aaa[0] ", "bbb"]
    );
    // Upstream's default breaks right before the box.
    assert_eq!(
        lines("xx aaa bbb", &[(6, 10.0, Auto)], 125.0),
        ["xx aaa", "[0] bbb"]
    );
}

#[test]
fn edges_add_no_opportunities_where_everything_fits() {
    let boxes = [(4, 10.0, KeepWithNext), (7, 10.0, KeepWithPrevious)];
    assert_eq!(lines("aaa bbb ccc", &boxes, 1000.0), ["aaa [0]bbb[1] ccc"]);
    // Inside a word, an edge never lets it break.
    let boxes = [(2, 10.0, KeepWithNext), (4, 10.0, KeepWithPrevious)];
    assert_eq!(lines("aaaaaa", &boxes, 50.0), ["aa[0]aa[1]aa"]);
}

#[test]
fn nested_and_empty_spans_move_together() {
    // Two starts at the same place (`<a><b>bbb`).
    let nested = [(4, 10.0, KeepWithNext), (4, 10.0, KeepWithNext)];
    assert_eq!(lines("aaa bbb", &nested, 100.0), ["aaa ", "[0][1]bbb"]);
    // An empty span (`<a></a>bbb`).
    let empty = [(4, 10.0, KeepWithNext), (4, 10.0, KeepWithPrevious)];
    assert_eq!(lines("aaa bbb", &empty, 100.0), ["aaa ", "[0][1]bbb"]);
}

#[test]
fn a_start_edge_goes_with_an_atomic_inline() {
    // `<a><img>`: a start edge, then an image, at the same place.
    let boxes = [(4, 10.0, KeepWithNext), (4, 20.0, Auto)];
    assert_eq!(lines("aaa bbb", &boxes, 100.0), ["aaa ", "[0][1]bbb"]);
}

#[test]
fn min_content_counts_an_element_with_its_edges() {
    // "bbb" with 10px on each side is the widest thing that can't break.
    let boxes = [(4, 10.0, KeepWithNext), (7, 10.0, KeepWithPrevious)];
    let widths = layout("aaa bbb", &boxes).calculate_content_widths();
    assert_eq!((widths.min, widths.max), (80.0, 160.0));
    // Upstream's default can break on either side of each box.
    let boxes = [(4, 10.0, Auto), (7, 10.0, Auto)];
    assert_eq!(
        layout("aaa bbb", &boxes).calculate_content_widths().min,
        60.0
    );
}

#[test]
fn edges_add_nothing_to_the_line_height() {
    // A 10px line height for 20px Ahem (16px ascent, 4px descent): 5px of
    // half-leading off each side, so the text reaches 11px above the
    // baseline and stops 1px short of it below. A 0px box on the baseline
    // would reach down to it, and make the line 11px.
    let height = |line_break| {
        let mut layout = layout_with_line_height("aa", &[(1, 5.0, line_break)], 0.5);
        layout.break_all_lines(None);
        layout.lines().next().unwrap().metrics().line_height
    };
    assert_eq!(height(KeepWithNext), 10.0);
    assert_eq!(height(KeepWithPrevious), 10.0);
    // An atomic inline does count, as upstream has it.
    assert_eq!(height(Auto), 11.0);
}

#[test]
fn a_box_first_on_a_nowrap_line_doesnt_break_it() {
    // The wrap mode came only from text, so a box before any text took the
    // default and broke the line after it.
    let lines = |text: &str, boxes: &[Box]| {
        let mut layout = layout_with(text, boxes, 1.0, TextWrapMode::NoWrap);
        layout.break_all_lines(Some(40.0));
        let widths = layout.calculate_content_widths();
        (layout.lines().count(), widths.min)
    };
    assert_eq!(lines("aa", &[(0, 60.0, Auto)]), (1, 100.0));
    assert_eq!(
        lines("aa bb", &[(0, 60.0, Auto), (2, 60.0, Auto)]),
        (1, 220.0)
    );
}

#[test]
fn the_line_after_a_final_break_is_made_only_if_asked() {
    // Each line's items and the layout's height, broken all at once and
    // one line at a time (as beside floats), the line after a final break
    // made or not.
    let broken = |text: &str, boxes: &[Box], made: bool| {
        let mut layout = layout(text, boxes);
        layout.set_line_after_final_break(made);
        let mut yielded = 0;
        let mut breaker = layout.break_lines();
        breaker.state_mut().set_layout_max_advance(1000.0);
        breaker.state_mut().set_line_max_advance(1000.0);
        while breaker.break_next().is_some() {
            yielded += 1;
        }
        breaker.finish();
        let mut all = layout.clone();
        all.break_all_lines(Some(1000.0));
        assert_eq!(all.len(), layout.len(), "{text:?}");
        assert_eq!(yielded, layout.len(), "{text:?}");
        (layout.len(), layout.height())
    };
    assert_eq!(broken("a\n", &[], false), (1, 20.0));
    assert_eq!(broken("\n", &[], false), (1, 20.0));
    assert_eq!(broken("a\n\n", &[], false), (2, 40.0));
    assert_eq!(broken("a\nb", &[], false), (2, 40.0));
    // No text: the first line stays, though it has no items either.
    assert_eq!(broken("", &[], false).0, 1);
    // Upstream's default: an editor's caret goes on that line.
    assert_eq!(broken("a\n", &[], true), (2, 40.0));
    let mut unset = layout("a\n", &[]);
    unset.break_all_lines(Some(1000.0));
    assert_eq!(unset.len(), 2);
    // Built again, a layout is as new: the line is made again.
    unset.set_line_after_final_break(false);
    build_into(&mut unset, "a\n", &[], 1.0, TextWrapMode::Wrap);
    unset.break_all_lines(Some(1000.0));
    assert_eq!(unset.len(), 2);
    // An inline's end edge after the break is on that line, which isn't
    // empty: it stays (with no height, as an invisible line box).
    assert_eq!(broken("a\n", &[(2, 0.0, KeepWithPrevious)], false).0, 2);
}
