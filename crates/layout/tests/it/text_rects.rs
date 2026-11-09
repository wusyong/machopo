//! The boxes of a part of a text node (`text_rects`, for `Range`'s
//! `getClientRects`) with Ahem, where every glyph is a 1em square (20px
//! here, its ascent 16px): one per run on a line, from the range's start
//! to its end in UTF-16 code units, as tall as the font; collapsed white
//! space taking no room; a collapsed range a box of no width.

use std::sync::Arc;

use gfx::{NodeId, RectF};
use layout::{BoxFragment, ScrollOffsets, text_rects};

use crate::page::{Page, VIEWPORT, find};

const AHEM: &str = "<style>html, body { margin: 0 } div { font: 20px/1 Ahem }</style>";

/// The page laid out, the element with id `id`, and its `index`th child
/// (a text node).
fn lay_out(html: &str, id: &str, index: usize) -> (Arc<BoxFragment>, NodeId, NodeId) {
    let mut page = Page::build(&format!("{AHEM}{html}"));
    let (element, text) = page.read(|cx, document| {
        let element = find(cx, document, id);
        let mut text = cx.first_child(element);
        for _ in 0..index {
            text = text.and_then(|child| cx.next_sibling(child));
        }
        (cx.node_id(element), cx.node_id(text.expect("the text")))
    });
    (page.tree.layout(VIEWPORT), element, text)
}

fn rects(html: &str, id: &str, index: usize, start: u32, end: u32) -> Vec<RectF> {
    let (view, _, text) = lay_out(html, id, index);
    text_rects(
        &view,
        &ScrollOffsets::default(),
        &Default::default(),
        text,
        start,
        end,
    )
}

/// From the start of the range to its end, as tall as the font; a
/// collapsed range has no width.
#[test]
fn a_part_of_a_text() {
    let html = "<div id=a>XX XX</div>";
    assert_eq!(
        rects(html, "a", 0, 1, 4),
        [RectF::new(20.0, 0.0, 60.0, 20.0)]
    );
    assert_eq!(
        rects(html, "a", 0, 0, 5),
        [RectF::new(0.0, 0.0, 100.0, 20.0)]
    );
    assert_eq!(
        rects(html, "a", 0, 2, 2),
        [RectF::new(40.0, 0.0, 0.0, 20.0)]
    );
    assert_eq!(
        rects(html, "a", 0, 5, 5),
        [RectF::new(100.0, 0.0, 0.0, 20.0)]
    );
}

/// White space that collapsed away takes no room: a range only in it is a
/// box of no width where the next kept character is.
#[test]
fn collapsed_white_space() {
    let html = "<div id=a>  XX   XX  </div>";
    // Kept: "XX XX", from offsets 2, 3, 4 (the space), 7, 8.
    assert_eq!(rects(html, "a", 0, 0, 2), [RectF::new(0.0, 0.0, 0.0, 20.0)]);
    assert_eq!(
        rects(html, "a", 0, 5, 7),
        [RectF::new(60.0, 0.0, 0.0, 20.0)]
    );
    assert_eq!(
        rects(html, "a", 0, 4, 8),
        [RectF::new(40.0, 0.0, 40.0, 20.0)]
    );
    assert_eq!(
        rects(html, "a", 0, 2, 11),
        [RectF::new(0.0, 0.0, 100.0, 20.0)]
    );
}

/// A character `text-transform` expands (`ß` to `SS`) is one character of
/// the text node: the range over it covers both glyphs.
#[test]
fn an_expanded_character() {
    let html = "<div id=a style='text-transform: uppercase'>xß</div>";
    assert_eq!(
        rects(html, "a", 0, 1, 2),
        [RectF::new(20.0, 0.0, 40.0, 20.0)]
    );
    assert_eq!(
        rects(html, "a", 0, 0, 1),
        [RectF::new(0.0, 0.0, 20.0, 20.0)]
    );
}

/// A range over a wrapped line has a box on each line; the space that
/// hangs at the end of the first takes no room.
#[test]
fn across_lines() {
    let html = "<div id=a style='width: 100px'>XX XX XX</div>";
    assert_eq!(
        rects(html, "a", 0, 1, 8),
        [
            RectF::new(20.0, 0.0, 80.0, 20.0),
            RectF::new(0.0, 20.0, 40.0, 20.0),
        ]
    );
    // A collapsed range at the break is at the start of the second line
    // (the run it is in); a range in the second line only is in its run
    // only.
    assert_eq!(
        rects(html, "a", 0, 6, 6),
        [RectF::new(0.0, 20.0, 0.0, 20.0)]
    );
    assert_eq!(
        rects(html, "a", 0, 7, 8),
        [RectF::new(20.0, 20.0, 20.0, 20.0)]
    );
    // Preserved spaces hanging past the line take no room either.
    let pre = "<div id=a style='width: 100px; white-space: pre-wrap'>XXXXX   XX</div>";
    assert_eq!(
        rects(pre, "a", 0, 6, 8),
        [RectF::new(100.0, 0.0, 0.0, 20.0)]
    );
    assert_eq!(
        rects(pre, "a", 0, 5, 7),
        [RectF::new(100.0, 0.0, 0.0, 20.0)]
    );
}

/// A long text node over many lines: each line's run finds its part of
/// the text, far into the node's characters.
#[test]
fn a_long_text_over_lines() {
    // Nine characters a line ("XXXX XXXX"), the space after them hanging.
    let html = format!(
        "<div id=a style='width: 200px'>{}</div>",
        "XXXX ".repeat(16).trim_end()
    );
    assert_eq!(
        rects(&html, "a", 0, 20, 24),
        [RectF::new(0.0, 40.0, 80.0, 20.0)]
    );
    assert_eq!(
        rects(&html, "a", 0, 75, 79),
        [RectF::new(100.0, 140.0, 80.0, 20.0)]
    );
    assert_eq!(
        rects(&html, "a", 0, 33, 47),
        [
            RectF::new(60.0, 60.0, 120.0, 20.0),
            RectF::new(0.0, 80.0, 140.0, 20.0),
        ]
    );
}

/// Right-to-left text (by its characters: machopo has no `direction`): its
/// runs' clusters come in visual order, the text's last first, and each is
/// where its character is in the data.
#[test]
fn right_to_left_text() {
    let html = "<div id=a>\u{5D0}\u{5D1}\u{5D2} \u{5D3}\u{5D4}\u{5D5} \u{5D6}\u{5D7}\u{5D8}</div>";
    let (view, _, _) = lay_out(html, "a", 0);
    fn clusters(fragment: &BoxFragment, out: &mut Vec<Vec<(u32, u32)>>) {
        for line in &fragment.lines {
            for run in &line.runs {
                out.push(run.clusters().iter().map(|c| (c.start, c.end)).collect());
            }
        }
        for child in &fragment.children {
            clusters(&child.fragment, out);
        }
    }
    let mut runs = Vec::new();
    clusters(&view, &mut runs);
    let mut all: Vec<(u32, u32)> = Vec::new();
    for run in &runs {
        assert!(
            run.windows(2).all(|w| w[0].0 > w[1].0),
            "last first: {run:?}"
        );
        all.extend(run);
    }
    all.sort();
    assert_eq!(all, (0..11).map(|i| (i, i + 1)).collect::<Vec<_>>());
}

/// A collapsed range after a preserved newline is at the next line's
/// start; before it, at the first line's end.
#[test]
fn around_a_newline() {
    let html = "<div id=a style='white-space: pre'>XX\nXX</div>";
    assert_eq!(
        rects(html, "a", 0, 2, 2),
        [RectF::new(40.0, 0.0, 0.0, 20.0)]
    );
    assert_eq!(
        rects(html, "a", 0, 3, 3),
        [RectF::new(0.0, 20.0, 0.0, 20.0)]
    );
}

/// Each text node is its own: the second of an element's, or one in an
/// inline element moved by `position: relative`.
#[test]
fn text_nodes_of_their_own() {
    assert_eq!(
        rects("<div id=a>XX<!---->YY</div>", "a", 2, 0, 1),
        [RectF::new(40.0, 0.0, 20.0, 20.0)]
    );
    assert_eq!(
        rects(
            "<div>XX<span id=s style='position: relative; left: 10px'>YY</span></div>",
            "s",
            0,
            1,
            2
        ),
        [RectF::new(70.0, 0.0, 20.0, 20.0)]
    );
}

/// Offsets are UTF-16 code units: past a character outside the BMP (two
/// of them), the next glyph's box starts where that character's ends. A
/// point inside it takes the whole character, as Chrome's.
#[test]
fn utf16_offsets() {
    let html = "<div id=a>X\u{1F600}X</div>";
    let before = rects(html, "a", 0, 0, 3);
    let last = rects(html, "a", 0, 3, 4);
    assert_eq!(before.len(), 1);
    assert_eq!(last.len(), 1);
    assert_eq!(last[0].x, before[0].x + before[0].width);
    assert_eq!(last[0].width, 20.0);
    assert_eq!(rects(html, "a", 0, 0, 2), before);
    assert_eq!(rects(html, "a", 0, 2, 3), rects(html, "a", 0, 1, 3));
}

/// Text that isn't rendered has no box.
#[test]
fn no_box() {
    assert_eq!(
        rects(
            "<div style='display: none'><p id=a>XX</p></div>",
            "a",
            0,
            0,
            1
        ),
        []
    );
}

/// The boxes go with the element's `transform`, and with the scroll
/// container's offset.
#[test]
fn transformed_and_scrolled() {
    assert_eq!(
        rects(
            "<div id=a style='transform: translateX(10px)'>XX</div>",
            "a",
            0,
            0,
            1
        ),
        [RectF::new(10.0, 0.0, 20.0, 20.0)]
    );
    let (view, element, text) = lay_out(
        "<div id=a style='overflow: hidden; height: 20px; width: 40px'>XX XX</div>",
        "a",
        0,
    );
    let scroll = ScrollOffsets::from_iter([(element, gfx::PointF::new(0.0, 20.0))]);
    assert_eq!(
        text_rects(&view, &scroll, &Default::default(), text, 3, 4),
        [RectF::new(0.0, 0.0, 20.0, 20.0)]
    );
}
