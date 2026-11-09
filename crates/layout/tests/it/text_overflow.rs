//! `text-overflow: ellipsis` with Ahem: which clusters a line keeps, where
//! the ellipsis goes, what is hidden, and when nothing is truncated; and
//! that it is all layout places clusters and glyphs for.

use std::sync::Arc;

use gfx::{NodeId, PointF};
use layout::{BoxFragment, LayoutUpdate};

fn lay_out(html: &str) -> Arc<BoxFragment> {
    lay_out_counted(html).0
}

/// `lay_out`, and what laying it out did.
fn lay_out_counted(html: &str) -> (Arc<BoxFragment>, LayoutUpdate) {
    let laid = crate::page::lay_out(html);
    (laid.view, laid.layout)
}

/// A run as painted: where it starts, its visible advance, how many of its
/// glyphs are painted, and its element.
#[derive(Debug, PartialEq)]
struct Run {
    x: f32,
    visible: f32,
    glyphs: usize,
    node: Option<NodeId>,
}

/// A line: its runs in paint order and whether each atomic inline is
/// hidden.
#[derive(Debug, PartialEq)]
struct Line {
    runs: Vec<Run>,
    atomics_hidden: Vec<bool>,
}

fn lines(view: &BoxFragment) -> Vec<Line> {
    fn walk(fragment: &BoxFragment, origin: PointF, out: &mut Vec<Line>) {
        for line in &fragment.lines {
            out.push(Line {
                runs: line
                    .runs
                    .iter()
                    .map(|run| Run {
                        x: origin.x + run.x,
                        visible: run.visible_advance,
                        glyphs: run.hidden_from.unwrap_or(run.glyphs().count()),
                        node: run.node,
                    })
                    .collect(),
                atomics_hidden: line.atomic_inlines.iter().map(|a| a.hidden).collect(),
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

const AHEM: &str = "<style>html, body { margin: 0 } div { font: 20px/1 Ahem; width: 100px }
    .e { overflow: hidden; text-overflow: ellipsis; white-space: nowrap }</style>";

/// Whether each run, in tree order, has its clusters placed.
fn clusters_made(view: &BoxFragment) -> Vec<bool> {
    fn walk(fragment: &BoxFragment, out: &mut Vec<bool>) {
        for line in &fragment.lines {
            out.extend(line.runs.iter().map(|run| run.clusters_made()));
        }
        for child in &fragment.children {
            walk(&child.fragment, out);
        }
    }
    let mut out = Vec::new();
    walk(view, &mut out);
    out
}

/// The runs without their elements.
fn shapes(line: &Line) -> Vec<(f32, f32, usize)> {
    line.runs
        .iter()
        .map(|r| (r.x, r.visible, r.glyphs))
        .collect()
}

/// Ahem has U+2026: one square, after 80px of text in a 100px block.
const ELLIPSIS: (f32, f32, usize) = (80.0, 20.0, 1);

#[test]
fn a_line_is_cut_where_the_ellipsis_fits() {
    let l = lines(&lay_out(&format!(
        "{AHEM}<div class=e>XXXXXXXX</div>
         <div class=e>XXXX XXXX</div>
         <div class=e style='white-space: normal'>XXXXXXXX XXXXXXXX</div>"
    )));
    // 160px of text in 100px: four squares, then the ellipsis.
    assert_eq!(shapes(&l[0]), [(0.0, 80.0, 4), ELLIPSIS]);
    assert_eq!(shapes(&l[1]), [(0.0, 80.0, 4), ELLIPSIS]);
    // Every overflowing line, not only the last.
    assert_eq!(shapes(&l[2]), [(0.0, 80.0, 4), ELLIPSIS]);
    assert_eq!(shapes(&l[3]), [(0.0, 80.0, 4), ELLIPSIS]);
}

#[test]
fn nothing_is_cut_without_overflow_or_clipping() {
    let l = lines(&lay_out(&format!(
        "{AHEM}<div class=e>XXXXX</div>
         <div class=e style='overflow: visible'>XXXXXXXX</div>
         <div class=e style='text-overflow: clip'>XXXXXXXX</div>"
    )));
    assert_eq!(shapes(&l[0]), [(0.0, 100.0, 5)]);
    assert_eq!(shapes(&l[1]), [(0.0, 160.0, 8)]);
    assert_eq!(shapes(&l[2]), [(0.0, 160.0, 8)]);
}

#[test]
fn runs_and_atomic_inlines_past_the_ellipsis_are_hidden() {
    let l = lines(&lay_out(&format!(
        "{AHEM}<div class=e>XX<span style='display: inline-block; width: 60px'></span>XXXX</div>
         <div class=e>X<span id=s>XXXXXXX</span>X</div>"
    )));
    // The inline-block (60px at 40) leaves no room for the ellipsis: it
    // and the text after it are hidden, the ellipsis follows "XX".
    assert_eq!(
        shapes(&l[0]),
        [(0.0, 40.0, 2), (100.0, 0.0, 0), (40.0, 20.0, 1)]
    );
    assert_eq!(l[0].atomics_hidden, [true]);
    // The span's run is cut after three squares; the ellipsis is the
    // span's (hit as its text is), the run after it is hidden. (The
    // inline-block has no line of its own.)
    assert_eq!(
        shapes(&l[1]),
        [(0.0, 20.0, 1), (20.0, 60.0, 3), (160.0, 0.0, 0), ELLIPSIS]
    );
    assert_eq!(l[1].runs[3].node, l[1].runs[1].node);
    assert_ne!(l[1].runs[3].node, l[1].runs[0].node);
}

#[test]
fn the_first_child_keeps_a_cluster_or_an_atomic_inline() {
    let l = lines(&lay_out(&format!(
        "{AHEM}<div class=e style='width: 30px'>XXXX</div>
         <div class=e style='width: 40px'><span style='display: inline-block; width: 60px'></span>XX</div>"
    )));
    // No room for a square and the ellipsis: the first square stays and
    // the ellipsis overflows (clipped by the block).
    assert_eq!(shapes(&l[0]), [(0.0, 20.0, 1), (20.0, 20.0, 1)]);
    // An atomic inline first stays whole, wider than the block; the text
    // after it is hidden, the ellipsis follows it. (One line: the
    // inline-block doesn't break a `nowrap` line.)
    assert_eq!(l.len(), 2);
    assert_eq!(shapes(&l[1]), [(60.0, 0.0, 0), (60.0, 20.0, 1)]);
    assert_eq!(l[1].atomics_hidden, [false]);
}

#[test]
fn anonymous_blocks_truncate_as_their_parent_does() {
    let l = lines(&lay_out(&format!(
        "{AHEM}<div class=e>XXXXXXXX<div style='height: 10px'></div>XXXXXXXX</div>"
    )));
    assert_eq!(shapes(&l[0]), [(0.0, 80.0, 4), ELLIPSIS]);
    assert_eq!(shapes(&l[1]), [(0.0, 80.0, 4), ELLIPSIS]);
}

#[test]
fn a_run_further_on_is_cut_where_its_clusters_are() {
    // The bold run starts at 30px: 50px of room before the ellipsis keeps
    // two of its squares (to 70px), not two and a half.
    let l = lines(&lay_out(&format!(
        "{AHEM}<div class=e>X<b style='padding-left: 10px'>XXXXXXXX</b></div>"
    )));
    assert_eq!(
        shapes(&l[0]),
        [(0.0, 20.0, 1), (30.0, 40.0, 2), (70.0, 20.0, 1)]
    );
}

#[test]
fn only_a_cut_run_has_its_clusters_and_glyphs_placed() {
    // Lines broken, a newline kept, a `<br>`, and text overflowing without
    // `text-overflow`: their clusters and glyphs are placed when asked.
    let (view, update) = lay_out_counted(&format!(
        "{AHEM}<div>XX XX XX XX XX XX</div>
         <div style='white-space: pre'>XX\nXX</div>
         <div>XX<br>XX</div>
         <div style='white-space: nowrap'>XXXXXXXXXX</div>"
    ));
    assert_eq!((update.clusters_placed, update.glyphs_placed), (0, 0));
    assert!(clusters_made(&view).iter().all(|&made| !made));
    // A cut run: its clusters, to find what fits, and its glyphs, to hide
    // those past the cut. Not the ellipsis's, nor the next block's.
    let (view, update) = lay_out_counted(&format!(
        "{AHEM}<div class=e>XXXXXXXXXX</div><div>XX XX</div>"
    ));
    assert_eq!((update.clusters_placed, update.glyphs_placed), (10, 10));
    assert_eq!(clusters_made(&view), [true, false, false]);
}
