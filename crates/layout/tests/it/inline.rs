//! Inline layout with Ahem, where every glyph is a 1em square: line boxes,
//! wrapping, white space, line height, alignment, forced breaks and
//! anonymous blocks.

use std::sync::Arc;

use dom::{QualifiedName, names};
use gfx::{NodeId, PointF, RectF};
use layout::{BoxFragment, BoxRole, LineEndKind};

use crate::page::{Page, VIEWPORT};

fn lay_out(html: &str) -> Arc<BoxFragment> {
    lay_out_counting(html).0
}

/// Also how many times it broke lines.
fn lay_out_counting(html: &str) -> (Arc<BoxFragment>, usize) {
    let laid = crate::page::lay_out(html);
    (laid.view, laid.layout.lines_broken)
}

/// A line in LayoutView coordinates, with its runs' glyph x positions.
#[derive(Debug, PartialEq)]
struct Line {
    rect: RectF,
    baseline: Vec<f32>,
    glyph_x: Vec<f32>,
}

fn lines(view: &BoxFragment) -> Vec<Line> {
    fn walk(fragment: &BoxFragment, origin: PointF, out: &mut Vec<Line>) {
        for line in &fragment.lines {
            let r = line.rect;
            out.push(Line {
                rect: RectF::new(origin.x + r.x, origin.y + r.y, r.width, r.height),
                baseline: line
                    .runs
                    .iter()
                    .map(|run| origin.y + run.baseline)
                    .collect(),
                glyph_x: line
                    .runs
                    .iter()
                    .flat_map(|run| run.glyphs().map(move |(_, p)| origin.x + p.x))
                    .collect(),
            });
        }
        for child in &fragment.children {
            let at = PointF::new(origin.x + child.offset.x, origin.y + child.offset.y);
            walk(&child.fragment, at, out);
        }
    }
    let mut out = Vec::new();
    walk(view, PointF::new(0.0, 0.0), &mut out);
    out
}

/// The widths of the boxes with lines, in tree order.
fn line_box_widths(view: &BoxFragment) -> Vec<f32> {
    fn walk(fragment: &BoxFragment, out: &mut Vec<f32>) {
        if !fragment.lines.is_empty() {
            out.push(fragment.size.width);
        }
        for child in &fragment.children {
            walk(&child.fragment, out);
        }
    }
    let mut out = Vec::new();
    walk(view, &mut out);
    out
}

const AHEM: &str = "<style>html, body { margin: 0 } div { font: 20px/1 Ahem }</style>";

#[test]
fn one_glyph_is_a_square() {
    let l = lines(&lay_out(&format!("{AHEM}<div>X</div>")));
    assert_eq!(l.len(), 1);
    assert_eq!(l[0].rect, RectF::new(0.0, 0.0, 20.0, 20.0));
    // Ahem's ascent is 0.8em.
    assert_eq!(l[0].baseline, [16.0]);
    assert_eq!(l[0].glyph_x, [0.0]);
}

#[test]
fn wraps_at_spaces() {
    let l = lines(&lay_out(&format!(
        "{AHEM}<div style='width: 100px'>XX XX XX</div>"
    )));
    let rects: Vec<RectF> = l.iter().map(|l| l.rect).collect();
    assert_eq!(
        rects,
        [
            RectF::new(0.0, 0.0, 100.0, 20.0),
            RectF::new(0.0, 20.0, 40.0, 20.0)
        ]
    );
    // The space between the lines hangs off the end of the first: it is
    // still a (blank) glyph, at 100, but not part of the line's width.
    assert_eq!(l[0].glyph_x, [0.0, 20.0, 40.0, 60.0, 80.0, 100.0]);
    assert_eq!(l[1].glyph_x, [0.0, 20.0]);
}

#[test]
fn white_space() {
    let normal = lines(&lay_out(&format!("{AHEM}<div>  X  \n\t X  </div>")));
    assert_eq!(normal.len(), 1);
    assert_eq!(normal[0].glyph_x, [0.0, 20.0, 40.0]);
    let pre = lines(&lay_out(&format!(
        "{AHEM}<div style='white-space: pre'> X  X\nX</div>"
    )));
    assert_eq!(pre.len(), 2);
    assert_eq!(pre[0].glyph_x, [0.0, 20.0, 40.0, 60.0, 80.0]);
    assert_eq!(pre[1].rect.y, 20.0);
    let nowrap = lines(&lay_out(&format!(
        "{AHEM}<div style='width: 50px; white-space: nowrap'>XX XX XX</div>"
    )));
    assert_eq!(nowrap.len(), 1);
    assert_eq!(nowrap[0].rect.width, 160.0);
}

/// `letter-spacing` goes after every character (the last too, as in
/// Chrome), `word-spacing` after every space, `text-indent` before the
/// element's first line only (a percentage of the containing block's
/// width); the spacing counts in the intrinsic widths.
#[test]
fn spacing_and_indent() {
    let l = lines(&lay_out(&format!(
        "{AHEM}<div style='letter-spacing: 5px'>XX<span style='letter-spacing: normal'>XX</span></div>
         <div style='word-spacing: 10px'>X X<span style='word-spacing: normal'>X X</span></div>
         <div style='text-indent: 30px'>X<br>X</div>
         <div style='width: 100px; text-indent: 10%'>X</div>
         <div><div style='display: inline-block; letter-spacing: 5px'>XX</div></div>
         <div style='text-indent: 30px'>X<div style='height: 10px'></div>X</div>"
    )));
    // `normal` in a spaced element is 0, not the parent's.
    assert_eq!(l[0].glyph_x, [0.0, 25.0, 50.0, 70.0]);
    assert_eq!(l[1].glyph_x, [0.0, 20.0, 50.0, 70.0, 90.0, 110.0]);
    assert_eq!(l[2].glyph_x, [30.0]);
    assert_eq!(l[3].glyph_x, [0.0]);
    assert_eq!(l[4].glyph_x, [10.0]);
    let inline_block = l
        .iter()
        .find(|l| l.glyph_x == [0.0, 25.0] && l.rect.y > 0.0);
    assert_eq!(inline_block.map(|l| l.rect.width), Some(50.0));
    // The first anonymous block's line is the element's first formatted
    // line, the one after the block isn't.
    let anonymous: Vec<&[f32]> = l[l.len() - 2..].iter().map(|l| &l.glyph_x[..]).collect();
    assert_eq!(anonymous, [&[30.0][..], &[0.0]]);
}

/// An anonymous block's first line is its element's first formatted line,
/// and indented, unless a block box comes before it in the flow: a float
/// or an out-of-flow box doesn't count, and a block in an inline element
/// is as any. An anonymous flex or grid item's is always indented
/// (Chrome).
#[test]
fn the_first_anonymous_block_is_indented() {
    let l = lines(&lay_out(&format!(
        "{AHEM}<div style='text-indent: 30px'><div style='float: left; width: 10px; height: 10px'></div>X<div></div></div>
         <div style='text-indent: 30px'><div style='position: absolute'></div>X<div></div></div>
         <div style='display: flex; text-indent: 30px'>X<div></div></div>
         <div style='display: flex; text-indent: 30px'><div></div>X</div>
         <div style='display: grid; text-indent: 30px'><div></div>X</div>
         <div style='text-indent: 30px'><span>X<div></div>X</span></div>
         <div style='text-indent: 30px'><div style='height: 10px'></div><div style='float: left; width: 10px; height: 10px'></div>X</div>"
    )));
    let x: Vec<f32> = l.iter().map(|l| l.glyph_x[0]).collect();
    // Beside the 10px floats, the first indented and the last not.
    assert_eq!(x, [40.0, 30.0, 30.0, 30.0, 30.0, 30.0, 0.0, 10.0]);
}

/// A preserved tab reaches the next tab stop from the line's start edge
/// (`text-indent` included): every 8 spaces by default, `tab-size` spaces
/// (with letter spacing) or px; one nearer than half a space goes on to
/// the stop after it. The tabs count in a line that wraps, and in the
/// intrinsic widths, from where they are on each of their lines.
#[test]
fn tabs_reach_the_next_tab_stop() {
    let laid = lay_out(&format!(
        "{AHEM}<div style='white-space: pre'>X\tX</div>
         <div style='white-space: pre; tab-size: 4'>X\tX\tX</div>
         <div style='white-space: pre; tab-size: 2; letter-spacing: 5px'>X\tX</div>
         <div style='white-space: pre; tab-size: 50px'>X\tXX\tX</div>
         <div style='white-space: pre; tab-size: 25px'>X\tX</div>
         <div style='white-space: pre; tab-size: 4; text-indent: 10px'>X\tX</div>
         <div style='white-space: pre-wrap; width: 100px; tab-size: 4'>XXXX XX\tX</div>
         <div style='width: max-content; white-space: pre; tab-size: 4'>X\tX</div>
         <div style='width: min-content; white-space: break-spaces; tab-size: 4'>XX X\tX</div>"
    ));
    let l = lines(&laid);
    // A tab stop every 160 (8 spaces of 20).
    assert_eq!(l[0].glyph_x, [0.0, 20.0, 160.0]);
    // Every 80.
    assert_eq!(l[1].glyph_x, [0.0, 20.0, 80.0, 100.0, 160.0]);
    // Every 2 × 25: from 25 to 50, the gaps in the tab's advance.
    assert_eq!(l[2].glyph_x, [0.0, 25.0, 50.0]);
    // Every 50px.
    assert_eq!(l[3].glyph_x, [0.0, 20.0, 50.0, 70.0, 90.0, 100.0]);
    // At 20 the stop at 25 is nearer than half a space (10): the next one.
    assert_eq!(l[4].glyph_x, [0.0, 20.0, 50.0]);
    // From the start edge: the indent is 10, the tab reaches 80.
    assert_eq!(l[5].glyph_x, [10.0, 30.0, 80.0]);
    // Wrapped after `XXXX `, the tab on the second line reaches its stop
    // from that line's start: `XX` at 0, the tab to 80.
    assert_eq!(l[7].glyph_x, [0.0, 20.0, 40.0, 80.0]);
    let widths = line_box_widths(&laid);
    let [max, min] = widths[widths.len() - 2..] else {
        unreachable!()
    };
    // The max-content width has the tab to 80 and an `X`.
    assert_eq!(max, 100.0);
    // The min-content width is its widest piece between opportunities
    // (preserved white space doesn't hang in `break-spaces`): `X` and the
    // tab from 20 to 80, where on one line the tab would be from 80 to 160.
    assert_eq!(min, 80.0);
    // Shrink-to-fit takes the measured widths, not a line broken: an
    // inline-block and a float as wide as the tab to 80 and an `X`.
    for html in [
        "<div style='width: 300px'><div style='display: inline-block; white-space: pre; tab-size: 4'>X\tX</div></div>",
        "<div style='width: 300px'><div style='float: left; white-space: pre; tab-size: 4'>X\tX</div></div>",
    ] {
        let widths = line_box_widths(&lay_out(&format!("{AHEM}{html}")));
        assert_eq!(widths.last(), Some(&100.0), "{html}");
    }
}

/// A line may break after a soft hyphen, with a hyphen at its end
/// (css-text 3 § 5.4), where the hyphen fits, or with no other
/// opportunity on the line; not with `hyphens: none`. The intrinsic widths
/// count the hyphen too.
#[test]
fn a_line_breaks_after_a_soft_hyphen_with_a_hyphen() {
    let laid = lay_out(&format!(
        "{AHEM}<div style='width: 100px'>XXX&shy;XXX</div>
         <div style='width: 80px'>XX&shy;XX&shy;XX</div>
         <div style='width: 60px'>XXX&shy;X</div>
         <div style='width: 100px; hyphens: none'>XXX&shy;XXX</div>
         <div style='width: 100px; text-align: right'>XXX&shy;XXX</div>
         <div style='width: 100px'>&#x200F;XXX&shy;XXX</div>
         <div style='width: 100px'><span style='font-size: 10px'>XXXXXXXXX&shy;XX</span></div>
         <div style='width: 100px'>X<span style='font-size: 10px'>XXXXXXX&shy;XXX</span></div>
         <div style='width: 100px'><span style='position: relative; left: 5px; top: 2px'>XXX&shy;XXX</span></div>
         <div style='width: 60px; hyphens: none; overflow-wrap: anywhere'>XXX&shy;XXX</div>
         <div style='width: min-content'>XX&shy;XXX</div>
         <div style='width: min-content'>XXX&shy;X</div>
         <div style='width: min-content; hyphens: none'>XX&shy;XXX</div>"
    ));
    let l = lines(&laid);
    // The soft hyphen is a glyph with no advance; the hyphen, a 20px
    // square, comes last on its line.
    assert_eq!(l[0].glyph_x, [0.0, 20.0, 40.0, 60.0, 60.0]);
    assert_eq!(l[0].rect.width, 80.0);
    assert_eq!(l[1].glyph_x, [0.0, 20.0, 40.0]);
    // After `XXXX` the hyphen wouldn't fit: the line breaks at the first
    // soft hyphen, and the second one breaks nothing.
    assert_eq!(l[2].glyph_x, [0.0, 20.0, 40.0, 40.0]);
    assert_eq!(l[3].glyph_x, [0.0, 20.0, 40.0, 40.0, 60.0]);
    // Nowhere else to break: the hyphen overflows.
    assert_eq!(l[4].rect.width, 80.0);
    assert_eq!(l[5].glyph_x, [0.0]);
    // `hyphens: none`: one line, no hyphen.
    assert_eq!(l[6].rect.width, 120.0);
    assert_eq!(l[6].glyph_x.len(), 7);
    // Aligned with the hyphen.
    assert_eq!(l[7].rect, RectF::new(20.0, 140.0, 80.0, 20.0));
    assert_eq!(l[7].glyph_x, [20.0, 40.0, 60.0, 80.0, 80.0]);
    // In a right-to-left paragraph (a right-to-left mark first) the hyphen
    // goes on the left of the line's text.
    assert_eq!(l[9].rect, RectF::new(20.0, 180.0, 80.0, 20.0));
    assert_eq!(l[9].glyph_x.last(), Some(&20.0));
    assert_eq!(l[9].glyph_x[..3], [40.0, 60.0, 80.0]);
    // The hyphen is in the soft hyphen's style: 10px, so it fits.
    assert_eq!(l[11].rect.width, 100.0);
    assert_eq!(l[11].glyph_x.last(), Some(&90.0));
    // After a run of the block's: `X`, seven 10px squares, the soft hyphen
    // and the hyphen.
    assert_eq!(l[13].rect.width, 100.0);
    assert_eq!(l[13].glyph_x.len(), 10);
    assert_eq!(l[13].glyph_x.last(), Some(&90.0));
    // Moved with the element it is in.
    assert_eq!(l[15].glyph_x.last(), Some(&65.0));
    assert_eq!(l[15].baseline.last(), l[15].baseline.first());
    // With none, `overflow-wrap` may break after the soft hyphen, as
    // anywhere else.
    assert_eq!(l[17].glyph_x, [0.0, 20.0, 40.0, 60.0]);
    assert_eq!(l[18].glyph_x, [0.0, 20.0, 40.0]);
    // The min-content widths: `XX-` and `XXX`; `XXX-`; without breaking.
    let widths = line_box_widths(&laid);
    assert_eq!(widths[widths.len() - 3..], [60.0, 80.0, 100.0]);
}

/// The opportunity after a soft hyphen that an inline element's start edge
/// takes is taken as the soft hyphen has it, in the line breaking and the
/// intrinsic widths; an atomic inline after a soft hyphen has its own
/// opportunities, with no hyphen.
#[test]
fn a_soft_hyphen_before_an_inline_element() {
    const BOX: &str = "<span style='display: inline-block; width: 20px; height: 20px'></span>";
    let laid = lay_out(&format!(
        "{AHEM}<div style='width: 100px'>XXX&shy;<span>XXX</span></div>
         <div style='width: 100px; hyphens: none'>XXX&shy;<span>XXX</span></div>
         <div style='width: 100px'>XX&shy;{BOX}XXX</div>
         <div style='width: min-content'>XX&shy;<span>XXX</span></div>
         <div style='width: min-content; hyphens: none'>XX&shy;<span>XXX</span></div>"
    ));
    let l = lines(&laid);
    assert_eq!(l[0].rect.width, 80.0);
    assert_eq!(l[0].glyph_x, [0.0, 20.0, 40.0, 60.0, 60.0]);
    assert_eq!(l[1].glyph_x, [0.0, 20.0, 40.0]);
    assert_eq!(l[2].rect.width, 120.0);
    // Broken after the box, where `X` would have fitted a hyphen.
    assert_eq!(l[3].rect.width, 60.0);
    assert_eq!(l[3].glyph_x, [0.0, 20.0, 40.0]);
    let widths = line_box_widths(&laid);
    assert_eq!(widths[widths.len() - 2..], [60.0, 100.0]);
}

/// The min-content width shrink-to-fit takes (measured, not a line broken)
/// counts the hyphen at the end of each piece a soft hyphen ends, after an
/// inline element's edge too; with `hyphens: none` the soft hyphen ends
/// none; past an atomic inline it is forgotten.
#[test]
fn the_measured_min_content_width_counts_the_hyphen() {
    const BOX: &str = "<span style='display: inline-block; width: 20px; height: 20px'></span>";
    let laid = lay_out(&format!(
        "{AHEM}<div style='width: 0'><div style='float: left'>XXX&shy;X</div></div>
         <div style='width: 0'><div style='float: left; hyphens: none'>XX&shy;XXX</div></div>
         <div style='width: 0'><div style='float: left'>XXX&shy;<span>X</span></div></div>
         <div style='width: 0'><div style='float: left; hyphens: none'>XX&shy;<span>XXX</span></div></div>
         <div style='width: 0'><div style='float: left'>X<span style='font-size: 60px'>&shy;</span>{BOX}X</div></div>"
    ));
    let widths = line_box_widths(&laid);
    // `XXX-`; `XXXXX`; `XXX-` before the span; `XXXXX`; `X`, the box, `X`.
    assert_eq!(widths, [80.0, 100.0, 80.0, 100.0, 20.0]);
}

/// Tab stops are counted in the block container's space, Chrome's: its
/// font's and its spacing, whatever the tab's own; the tab's own
/// `tab-size` says how many, a `display: contents` element's too. They are
/// from the container's start edge, where a float beside the line takes
/// room too, and go on before it (a negative indent); without any (a size
/// of 0), a tab has only its letter spacing.
#[test]
fn tab_stops_are_the_block_containers() {
    let l = lines(&lay_out(&format!(
        "{AHEM}<div style='white-space: pre; tab-size: 2'><span style='font-size: 40px'>\tX</span></div>
         <div style='white-space: pre; tab-size: 2'>X<span style='tab-size: 4'>\tX</span></div>
         <div style='white-space: pre; tab-size: 2; word-spacing: 10px'><span style='word-spacing: 0'>\tX</span></div>
         <div style='white-space: pre; tab-size: 25px'>X<span style='font-size: 4px'>\tX</span></div>
         <div style='white-space: pre; tab-size: 4'><span style='float: left; width: 30px; height: 20px'></span>X\tX</div>
         <div style='white-space: pre; tab-size: 2'><span style='display: contents; tab-size: 4'>X\tX</span></div>
         <div style='white-space: pre; tab-size: 0; letter-spacing: 5px'>X\tX</div>
         <div style='white-space: pre; tab-size: 4; text-indent: -30px'>X\tX</div>"
    )));
    // Two of the block's 20px spaces, not of the span's 40px ones.
    assert_eq!(l[0].glyph_x, [0.0, 40.0]);
    // The span's 4 spaces.
    assert_eq!(l[1].glyph_x, [0.0, 20.0, 80.0]);
    // With the block's word spacing: every 2 × 30.
    assert_eq!(l[2].glyph_x, [0.0, 60.0]);
    // At 20 the stop at 25 is nearer than half the block's space (10).
    assert_eq!(l[3].glyph_x, [0.0, 20.0, 50.0]);
    // Beside the float, `X` at 30 and the tab to 80, not to 30 + 80.
    assert_eq!(l[4].glyph_x, [30.0, 50.0, 80.0]);
    // Text whose element has no span of its own has its `tab-size` too.
    assert_eq!(l[5].glyph_x, [0.0, 20.0, 80.0]);
    // No tab stops: the tab has only its letter spacing.
    assert_eq!(l[6].glyph_x, [0.0, 25.0, 30.0]);
    // Outdented, the tab at -10 reaches the stop at 0.
    assert_eq!(l[7].glyph_x, [-30.0, -10.0, 0.0]);
}

/// `word-break: break-all` breaks between any two characters,
/// `overflow-wrap: anywhere` (and Chrome's `word-break: break-word`)
/// where a word would overflow; `break-spaces` wraps preserved spaces
/// instead of hanging them; the longhands of `white-space` work alone.
#[test]
fn breaking_words_and_spaces() {
    let l = lines(&lay_out(&format!(
        "{AHEM}<div style='width: 40px; word-break: break-all'>XXX</div>
         <div style='width: 40px; overflow-wrap: anywhere'>XXX</div>
         <div style='width: 40px; word-break: break-word'>XXX</div>
         <div style='width: 40px; white-space: break-spaces'>X     X</div>
         <div style='width: 40px; text-wrap-mode: nowrap'>XX XX</div>
         <div style='white-space-collapse: preserve'>X  X</div>"
    )));
    let glyphs: Vec<&[f32]> = l.iter().map(|l| &l.glyph_x[..]).collect();
    assert_eq!(
        glyphs,
        [
            &[0.0, 20.0][..],
            &[0.0],
            &[0.0, 20.0],
            &[0.0],
            &[0.0, 20.0],
            &[0.0],
            // `X ` then two spaces, two spaces, `X`.
            &[0.0, 20.0],
            &[0.0, 20.0],
            &[0.0, 20.0],
            &[0.0],
            &[0.0, 20.0, 40.0, 60.0, 80.0],
            &[0.0, 20.0, 40.0, 60.0],
        ]
    );
}

/// `text-transform` changes the text that is shaped: `ab` in uppercase
/// is as wide as `AB`.
#[test]
fn text_transform() {
    let l = lines(&lay_out(
        "<div style='text-transform: uppercase'>ab</div><div>AB</div><div>ab</div>",
    ));
    assert_eq!(l[0].rect.width, l[1].rect.width);
    assert_ne!(l[0].rect.width, l[2].rect.width);
}

/// `vertical-align`: a length raises the box's baseline (a percentage of
/// its own `line-height`, `normal` being the font's), `sub` lowers it by
/// a fifth of the font size, `top` and `bottom` put the box against the
/// line box; the line grows around them. The block's own text isn't
/// moved by the block's.
#[test]
fn vertical_align() {
    let l = lines(&lay_out(&format!(
        "{AHEM}<div>X<span style='vertical-align: 5px'>X</span></div>
         <div>X<span style='vertical-align: 50%'>X</span></div>
         <div>X<span style='vertical-align: 50%; line-height: normal'>X</span></div>
         <div>X<span style='vertical-align: sub'>X</span></div>
         <div style='line-height: 40px'><span style='vertical-align: top; line-height: 20px'>X</span>X<span style='vertical-align: bottom; line-height: 20px'>X</span></div>
         <div style='vertical-align: sub'>X<span style='vertical-align: 5px'>X</span></div>
         <div style='font: 20px serif'>x</div>
         <div style='font: 20px serif'>x<span style='vertical-align: 100%'>x</span></div>
         <div>X<span style='display: inline-block; vertical-align: 5px'>X</span></div>"
    )));
    let line = height_and_baselines;
    assert_eq!(line(&l[0]), (25.0, vec![21.0, 16.0]));
    assert_eq!(line(&l[1]), (30.0, vec![26.0, 16.0]));
    assert_eq!(
        line(&l[2]),
        (30.0, vec![26.0, 16.0]),
        "Ahem's normal line height is 1em"
    );
    assert_eq!(line(&l[3]), (24.0, vec![16.0, 20.0]));
    assert_eq!(line(&l[4]), (40.0, vec![16.0, 26.0, 36.0]));
    // Not the block's own text: its `vertical-align` is for where it is
    // inline-level, if anywhere.
    assert_eq!(line(&l[5]), (25.0, vec![21.0, 16.0]));
    // A percentage of a `normal` line height is of the font's, which
    // isn't the font size for a font with a line gap (Lato here).
    let normal = l[6].rect.height;
    assert_ne!(normal, 20.0);
    assert_eq!(
        (l[7].rect.height, l[7].baseline[0] - l[7].baseline[1]),
        (2.0 * normal, normal)
    );
    // An atomic inline's own: the inline-block's line (the last) is 5px
    // above the block's baseline.
    assert_eq!(line(&l[8]), (25.0, vec![21.0]));
    assert_eq!(line(&l[9]), (20.0, vec![16.0]));
    assert_eq!(l[9].rect.y, l[8].rect.y);
}

/// A line's height and its runs' baselines from its top.
fn height_and_baselines(l: &Line) -> (f32, Vec<f32>) {
    let baselines: Vec<f32> = l.baseline.iter().map(|b| b - l.rect.y).collect();
    (l.rect.height, baselines)
}

#[test]
fn vertical_align_nests() {
    let l = lines(&lay_out(&format!(
        "{AHEM}<div>X<span style='vertical-align: 5px'>X<span style='vertical-align: 5px'>X</span>X</span>X</div>
         <div style='font-size: 30px'>X<span style='vertical-align: super'>X<span style='vertical-align: super'>X</span></span></div>
         <div style='font-size: 40px'><span style='font-size: 20px'>X<span style='vertical-align: middle'>X</span></span></div>
         <div>X<span style='vertical-align: 5px'><span style='display: inline-block'>X</span></span></div>"
    )));
    let line = height_and_baselines;
    // Each shift is from the parent's baseline, so they add up, and ends
    // with its element.
    assert_eq!(line(&l[0]), (30.0, vec![26.0, 21.0, 16.0, 21.0, 26.0]));
    // `super` is a third of the parent's font size.
    assert_eq!(line(&l[1]), (50.0, vec![44.0, 34.0, 24.0]));
    // `middle` is half the parent's x-height (Ahem's is 0.8em) above its
    // baseline, less half the box's own height above its baseline: the
    // parent is the 20px span, not the 40px block.
    assert_eq!(line(&l[2]), (40.0, vec![32.0, 30.0]));
    // An atomic inline moves with the span it is in: the inline-block's
    // line (the second here) sits 5px above the block's baseline.
    assert_eq!(line(&l[3]), (25.0, vec![21.0]));
    assert_eq!(line(&l[4]), (20.0, vec![16.0]));
    assert_eq!(l[4].rect.y, l[3].rect.y);
}

#[test]
fn vertical_align_goes_on_after_a_block() {
    // A block inside inline elements splits them: the anonymous block after
    // it is inside the same elements, nested the same way (the shift is
    // `super` of the 30px block, not of the 60px span).
    let l = lines(&lay_out(&format!(
        "{AHEM}<div style='font-size: 30px'><span style='vertical-align: super'><span style='font-size: 60px'>X<div>X</div>X</span></span></div>"
    )));
    let line = height_and_baselines;
    assert_eq!(line(&l[0]), (64.0, vec![48.0]));
    assert_eq!(line(&l[1]), (20.0, vec![16.0]), "the div's own 20px font");
    assert_eq!(line(&l[2]), (64.0, vec![48.0]));
}

#[test]
fn line_height_and_alignment() {
    let l = lines(&lay_out(&format!(
        "{AHEM}<div style='line-height: 50px'>X</div>
         <div style='width: 100px; text-align: center'>XX</div>
         <div style='width: 100px; text-align: right'>XX</div>"
    )));
    // Half-leading: (50 - 20) / 2 above the glyph.
    assert_eq!(l[0].rect, RectF::new(0.0, 0.0, 20.0, 50.0));
    assert_eq!(l[0].baseline, [15.0 + 16.0]);
    assert_eq!(l[1].glyph_x, [30.0, 50.0]);
    assert_eq!(l[2].glyph_x, [60.0, 80.0]);
    assert_eq!(l[2].rect.y, 70.0);
}

#[test]
fn forced_breaks_and_anonymous_blocks() {
    let view = lay_out(&format!(
        "{AHEM}<div id=d>X<br>XX<div style='height: 10px'></div>  <span>X</span></div>"
    ));
    let l = lines(&view);
    let rects: Vec<RectF> = l.iter().map(|l| l.rect).collect();
    assert_eq!(
        rects,
        [
            RectF::new(0.0, 0.0, 20.0, 20.0),
            RectF::new(0.0, 20.0, 40.0, 20.0),
            // After the 10px block, in the second anonymous block.
            RectF::new(0.0, 50.0, 20.0, 20.0),
        ]
    );
    // The div holds two anonymous blocks around its block child.
    // `head` has no box, so `body` is the root's only child.
    let body = &view.children[0].fragment.children[0].fragment;
    let div = &body.children[0].fragment;
    let roles: Vec<BoxRole> = div.children.iter().map(|c| c.fragment.role).collect();
    assert_eq!(
        roles,
        [
            BoxRole::AnonymousBlock,
            BoxRole::Other,
            BoxRole::AnonymousBlock
        ]
    );
    assert!(div.lines.is_empty());
}

/// A forced break that ends the content (`<br>`, a preserved newline) is
/// followed by no line (Chrome 152: `<div>X<br></div>` is one line tall),
/// beside floats too; one before it is.
#[test]
fn a_final_forced_break_makes_no_line() {
    let view = lay_out(&format!(
        "{AHEM}<div>X<br></div><div><br></div><div>X<br><br></div>
         <div style='white-space: pre'>X\n</div><div style='white-space: pre'>X\n\n</div>
         <div style='display: flow-root'><div style='float: left; width: 10px; height: 5px'></div>X<br></div>"
    ));
    let body = &view.children[0].fragment.children[0].fragment;
    let heights: Vec<f32> = body
        .children
        .iter()
        .map(|child| child.fragment.size.height)
        .collect();
    assert_eq!(heights, [20.0, 20.0, 40.0, 20.0, 40.0, 20.0]);
}

#[test]
fn white_space_between_blocks_makes_no_anonymous_block() {
    let view = lay_out(&format!(
        "{AHEM}<div>\n  <div style='height: 10px'></div>\n  <div style='height: 10px'></div>\n</div>"
    ));
    // `head` has no box, so `body` is the root's only child.
    let body = &view.children[0].fragment.children[0].fragment;
    assert_eq!(body.children[0].fragment.children.len(), 2);
    assert!(lines(&view).is_empty());
}

#[test]
fn real_text_is_shaped_with_lato() {
    // Every generic family is Lato in test-fonts mode; it has proportional
    // widths, so "iii" is narrower than "WWW".
    let l = lines(&lay_out(
        "<div style='font-family: sans-serif; font-size: 20px'>iii</div>
         <div style='font-family: serif; font-size: 20px'>WWW</div>",
    ));
    assert_eq!(l.len(), 2);
    assert!(l[0].rect.width < l[1].rect.width / 2.0, "{l:?}");
    assert!(
        l[0].rect.height > 20.0,
        "normal line height uses the font's metrics"
    );
}

#[test]
fn bold_is_synthesized_on_a_lighter_face() {
    // Lato is only there in Medium (500). fontique alone asks for emboldening
    // only above 700, so `bold` came out as thin as the rest of the text.
    fn embolden(fragment: &BoxFragment) -> Vec<bool> {
        fragment
            .lines
            .iter()
            .flat_map(|l| l.runs.iter().map(|r| r.embolden))
            .chain(fragment.children.iter().flat_map(|c| embolden(&c.fragment)))
            .collect()
    }
    let view = lay_out(
        "<style>div { font: 20px sans-serif }</style>
         <div>normal</div>
         <div style='font-weight: 500'>medium</div>
         <div style='font-weight: 599'>just below</div>
         <div style='font-weight: 600'>semibold</div>
         <div style='font-weight: bold'>bold</div>
         <div><b>b</b></div>
         <div style='font-weight: 900'>black</div>",
    );
    assert_eq!(
        embolden(&view),
        [false, false, false, true, true, true, true]
    );
}

/// Each line's inline boxes in LayoutView coordinates: the rect, and
/// whether the start and end borders are drawn there.
fn inline_boxes(view: &BoxFragment) -> Vec<Vec<(RectF, bool, bool)>> {
    fn walk(fragment: &BoxFragment, origin: PointF, out: &mut Vec<Vec<(RectF, bool, bool)>>) {
        for line in &fragment.lines {
            out.push(
                line.boxes
                    .iter()
                    .map(|b| {
                        let r = b.rect;
                        (
                            RectF::new(origin.x + r.x, origin.y + r.y, r.width, r.height),
                            b.border.left > 0.0,
                            b.border.right > 0.0,
                        )
                    })
                    .collect(),
            );
        }
        for child in &fragment.children {
            let at = PointF::new(origin.x + child.offset.x, origin.y + child.offset.y);
            walk(&child.fragment, at, out);
        }
    }
    let mut out = Vec::new();
    walk(view, PointF::new(0.0, 0.0), &mut out);
    out
}

#[test]
fn inline_box_split_by_a_block() {
    // The span is split around the block: its first part (in the first
    // anonymous block) has only the start border, the last part only the
    // end one, as when it wraps.
    let view = lay_out(&format!(
        "{AHEM}<div><span style='border: 5px solid'>aa<div>b</div>cc</span></div>"
    ));
    let boxes = inline_boxes(&view);
    assert_eq!(
        boxes,
        [
            vec![(RectF::new(0.0, -5.0, 45.0, 30.0), true, false)],
            vec![],
            vec![(RectF::new(0.0, 35.0, 45.0, 30.0), false, true)],
        ]
    );
    // The text after the block starts at the line's start: no start edge.
    assert_eq!(lines(&view)[2].glyph_x, [0.0, 20.0]);
}

#[test]
fn inline_box_edges_resolve_percentages_against_the_block() {
    // 10% of the 200px block, on each side.
    let view = lay_out(&format!(
        "{AHEM}<div style='width: 200px'>a<span style='padding: 0 10%'>b</span>c</div>"
    ));
    assert_eq!(lines(&view)[0].glyph_x, [0.0, 40.0, 80.0]);
    assert_eq!(
        inline_boxes(&view)[0][0].0,
        RectF::new(20.0, 0.0, 60.0, 20.0)
    );
}

#[test]
fn edges_keep_to_their_content() {
    // The start edge (10px) fits on the first line, its word doesn't: it
    // goes down with the word. "XX XXX" fits the 125px line, not with the
    // end edge after it: the word goes down with its edge.
    let l = lines(&lay_out(&format!(
        "{AHEM}<div style='width: 100px'>XXX <span style='padding-left: 10px'>XXX</span></div>
         <div style='width: 125px'>XX <span style='padding-right: 10px'>XXX</span> XXX</div>"
    )));
    // The spaces have glyphs too, after the edge on the fourth line.
    let glyphs: Vec<&[f32]> = l.iter().map(|l| &l.glyph_x[..]).collect();
    assert_eq!(
        glyphs,
        [
            &[0.0, 20.0, 40.0, 60.0][..],
            &[10.0, 30.0, 50.0],
            &[0.0, 20.0, 40.0],
            &[0.0, 20.0, 40.0, 70.0],
            &[0.0, 20.0, 40.0],
        ]
    );
}

#[test]
fn text_knows_its_element() {
    let mut page = Page::build(&format!("{AHEM}<div>a<a href=x>b</a></div>"));
    let find = |local| {
        page.read(|cx, document| {
            let mut found = None;
            crate::page::each_node(cx, document, &mut |node| {
                if found.is_none() && cx.qualified_name(node) == Some(QualifiedName::html(local)) {
                    found = Some(cx.node_id(node));
                }
            });
            found.unwrap()
        })
    };
    let (div, link) = (find(names::local::DIV), find(names::local::A));
    let view = page.tree.layout(VIEWPORT);
    fn runs(fragment: &BoxFragment) -> Vec<Option<NodeId>> {
        fragment
            .lines
            .iter()
            .flat_map(|l| l.runs.iter().map(|r| r.node))
            .chain(fragment.children.iter().flat_map(|c| runs(&c.fragment)))
            .collect()
    }
    assert_eq!(runs(&view), [Some(div), Some(link)]);
}

/// How each line ends, with the box a selection over its end draws
/// (Chrome 152 paints a space's box after the line's text), in tree
/// order: the kind, where the box is and how wide.
fn ends(html: &str) -> Vec<Option<(String, f32, f32)>> {
    fn walk(fragment: &BoxFragment, out: &mut Vec<Option<(String, f32, f32)>>) {
        for line in &fragment.lines {
            out.push(line.end.as_ref().map(|end| {
                let kind = match end.kind() {
                    LineEndKind::Wrap { offset, .. } => format!("wrap {offset}"),
                    LineEndKind::End { offset, .. } => format!("end {offset}"),
                    LineEndKind::Newline { offset, .. } => format!("newline {offset}"),
                    LineEndKind::Break(_) => "break".to_string(),
                };
                (kind, end.x(), end.width)
            }));
        }
        for child in &fragment.children {
            walk(&child.fragment, out);
        }
    }
    let mut out = Vec::new();
    walk(&lay_out(&format!("{AHEM}{html}")), &mut out);
    out
}

#[test]
fn how_lines_end() {
    let end = |kind: &str, x: f32, width: f32| Some((kind.to_string(), x, width));
    // A `<br>` in its own font, on an empty line too.
    assert_eq!(
        ends("<div>XX<br style='font-size: 10px'><br style='font-size: 40px'>X</div>"),
        [
            end("break", 40.0, 10.0),
            end("break", 0.0, 40.0),
            end("end 1", 20.0, 20.0)
        ]
    );
    // A space hanging past a wrapped line isn't before it.
    assert_eq!(
        ends("<div style='width: 50px'>XX XX</div>"),
        [end("wrap 2", 40.0, 20.0), end("end 5", 40.0, 20.0)]
    );
    assert_eq!(
        ends("<div style='white-space: pre'>XX\nX</div>"),
        [end("newline 2", 40.0, 20.0), end("end 4", 20.0, 20.0)]
    );
    // Likewise in text that starts further on the line (its clusters are
    // placed from its start).
    assert_eq!(
        ends("<div style='width: 50px'>X<b>X X</b></div>"),
        [end("wrap 1", 40.0, 20.0), end("end 3", 20.0, 20.0)]
    );
    assert_eq!(
        ends("<div style='white-space: pre'>X<b>X\nX</b></div>"),
        [end("newline 1", 40.0, 20.0), end("end 3", 20.0, 20.0)]
    );
    // After the text, not the edges of the element it ends in.
    assert_eq!(
        ends("<div>X<span style='padding-right: 10px'>X</span></div>"),
        [end("end 1", 40.0, 20.0)]
    );
    assert_eq!(
        ends("<div><img style='width: 10px; height: 10px'></div>"),
        [None]
    );
}

/// Measured for its width alone, inline content breaks no lines: its
/// content widths give it. A shrink-to-fit box's text is broken once, at
/// the width the box ends up; a float in it is placed beside its lines,
/// and an indent moves where they end, which the width needs.
#[test]
fn a_width_alone_breaks_no_lines() {
    let ahem = "<style>body { margin: 0; font: 20px/1 Ahem }</style>";
    for (html, width, broken) in [
        ("<div style='width: max-content'>XX XX</div>", 100.0, 1),
        ("<div style='width: min-content'>XX XX</div>", 40.0, 1),
        (
            "<div style='width: 70px'><div style='width: fit-content'>XX XX</div></div>",
            70.0,
            1,
        ),
        // A percentage width is `auto` in the min-content contribution
        // (css-sizing 3, "cyclic percentage"): its content counts, not the
        // percentage of the width measured. (Its own text is broken too.)
        (
            "<div style='width: min-content'><span style='display: inline-block; width: 50%'>XX</span> X</div>",
            40.0,
            2,
        ),
        // Its content widths leave an indent out: its lines give them.
        (
            "<div style='width: max-content; text-indent: 10px'>XX XX</div>",
            110.0,
            2,
        ),
        (
            "<div style='width: min-content; text-indent: 10px'>XX XX</div>",
            50.0,
            2,
        ),
        // A percentage is 0 there, as in Blink.
        (
            "<div style='width: max-content; text-indent: 10%'>XX XX</div>",
            100.0,
            1,
        ),
        (
            "<div style='width: fit-content'>XX <span style='display: inline-block; width: 50px'></span></div>",
            110.0,
            1,
        ),
        // A float is placed beside the lines broken to measure it.
        (
            "<div style='width: fit-content'><span style='float: left'>X</span>XX XX</div>",
            120.0,
            3,
        ),
    ] {
        let (view, lines_broken) = lay_out_counting(&format!("{ahem}{html}"));
        let body = &view.children[0].fragment.children[0].fragment;
        let mut a = &body.children[0].fragment;
        while a.lines.is_empty() {
            a = &a.children[0].fragment;
        }
        assert_eq!(
            a.size.width,
            width,
            "{html}
{}",
            view.dump()
        );
        assert_eq!(lines_broken, broken, "{html}");
    }
}

/// The lines are broken again as the fragments are made where the last
/// break was at another width (parley keeps only the last): a float
/// shrunk to fit no room has its lines broken in the room and is laid out
/// as wide as its content.
#[test]
fn lines_are_broken_again_for_their_fragments() {
    let ahem = "<style>body { margin: 0; font: 20px/1 Ahem }</style>";
    for (html, broken, again) in [
        ("<div style='width: 100px'>XX XX</div>", 1, 0),
        (
            "<div style='width: 100px'><div style='float: left'>XX XX</div></div>",
            1,
            0,
        ),
        (
            "<div style='width: 0'><div style='float: left'>XX XX</div></div>",
            2,
            1,
        ),
    ] {
        let layout = crate::page::lay_out(&format!("{ahem}{html}")).layout;
        assert_eq!(
            (layout.lines_broken, layout.lines_broken_for_fragments),
            (broken, again),
            "{html}"
        );
    }
}
