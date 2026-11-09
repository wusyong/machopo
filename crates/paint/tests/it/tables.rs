//! Tables painted as Blink paints separated ones: the table's background
//! and border around its grid, not its captions; a row group's and a row's
//! background in each of its cells, not in the border spacing; an empty
//! cell with `empty-cells: hide` painting nothing.

use compositor_paint::{PaintFlags, PaintOp};
use gfx::{Color, RectF, SizeF};
use paint::PaintArtifact;

fn paint_html(html: &str) -> PaintArtifact {
    let (view, _) = crate::page::lay_out(html, SizeF::new(800.0, 600.0));
    let pre = paint::pre_paint(&view, &Default::default(), 1.0);
    paint::paint(&view, &pre, layout::document_size(&view), &())
}

/// The rectangles filled with `color`, in paint order.
fn fills(artifact: &PaintArtifact, color: Color) -> Vec<RectF> {
    artifact
        .items
        .iter()
        .filter_map(|item| match item.op {
            PaintOp::DrawRect {
                rect,
                flags: PaintFlags { color: c, .. },
            } if c == color => Some(rect),
            _ => None,
        })
        .collect()
}

const STYLE: &str = "<style>body { margin: 0; font: 20px/1 Ahem } td { padding: 0 }</style>";

const RED: Color = Color::rgb(255, 0, 0);
const LIME: Color = Color::rgb(0, 255, 0);
const BLUE: Color = Color::rgb(0, 0, 255);

#[test]
fn a_tables_background_and_border_go_around_its_grid() {
    let artifact = paint_html(&format!(
        "{STYLE}<table style='background: red; border: 2px solid blue; border-spacing: 0'>
           <caption>XX</caption><tr><td>XX</td></tr>
           <caption style='caption-side: bottom'>X</caption></table>"
    ));
    // Its border box: 44px wide, below the 20px caption, 24px tall.
    assert_eq!(fills(&artifact, RED), [RectF::new(0.0, 20.0, 44.0, 24.0)]);
    // The border, whatever draws it.
    let border: Vec<RectF> = artifact
        .items
        .iter()
        .filter(|item| match &item.op {
            PaintOp::DrawRect { flags, .. }
            | PaintOp::DrawRRect { flags, .. }
            | PaintOp::DrawDRRect { flags, .. }
            | PaintOp::DrawPath { flags, .. } => flags.color == BLUE,
            _ => false,
        })
        .map(|item| item.visual_rect)
        .collect();
    assert!(!border.is_empty());
    for side in border {
        assert!(side.y >= 20.0 && side.y + side.height <= 44.0, "{side:?}");
    }
}

#[test]
fn row_and_row_group_backgrounds_go_in_their_cells() {
    let artifact = paint_html(&format!(
        "{STYLE}<table style='border-spacing: 10px'><tbody style='background: blue'>
           <tr style='background: lime'><td>X</td><td style='background: red'>XX</td></tr>
           </tbody></table>"
    ));
    let cells = [
        RectF::new(10.0, 10.0, 20.0, 20.0),
        RectF::new(40.0, 10.0, 40.0, 20.0),
    ];
    assert_eq!(fills(&artifact, BLUE), cells);
    assert_eq!(fills(&artifact, LIME), cells);
    assert_eq!(fills(&artifact, RED), [cells[1]]);
    // The row group's, then the row's, then the cell's.
    let kinds: Vec<Color> = artifact
        .items
        .iter()
        .filter_map(|item| match item.op {
            PaintOp::DrawRect { rect, ref flags } if rect == cells[1] => Some(flags.color),
            _ => None,
        })
        .collect();
    assert_eq!(kinds, [BLUE, LIME, RED]);
}

#[test]
fn an_empty_cell_can_hide() {
    let artifact = paint_html(&format!(
        "{STYLE}<table style='border-spacing: 0; empty-cells: hide'><tr style='background: lime'>
           <td style='background: red; border: 1px solid blue; width: 10px'></td>
           <td style='background: red'>X</td>
           <td style='background: red; empty-cells: show; width: 10px'></td></tr></table>"
    ));
    // The cell with text, and the empty one that shows.
    let red = fills(&artifact, RED);
    assert_eq!(red.len(), 2, "{red:?}");
    assert!(fills(&artifact, BLUE).is_empty());
    assert_eq!(fills(&artifact, LIME).len(), 2);
}

/// A column group's and a column's backgrounds go in the cells that start
/// in their columns (a spanning cell too), not in the border spacing nor in
/// a hidden cell; a group's under its column's.
#[test]
fn column_backgrounds_go_in_the_cells_that_start_in_them() {
    let artifact = paint_html(&format!(
        "{STYLE}<table style='border-spacing: 10px'><colgroup style='background: blue'>
           <col style='background: lime'><col></colgroup>
           <tr><td>X</td><td>XX</td></tr><tr><td colspan=2>X</td></tr>
           <tr><td style='visibility: hidden'>X</td><td>X</td></tr></table>"
    ));
    let (c1, c2) = (
        RectF::new(10.0, 10.0, 20.0, 20.0),
        RectF::new(40.0, 10.0, 40.0, 20.0),
    );
    let span = RectF::new(10.0, 40.0, 70.0, 20.0);
    let last = RectF::new(40.0, 70.0, 40.0, 20.0);
    assert_eq!(fills(&artifact, BLUE), [c1, c2, span, last]);
    assert_eq!(fills(&artifact, LIME), [c1, span]);
    let first: Vec<Color> = artifact
        .items
        .iter()
        .filter_map(|item| match item.op {
            PaintOp::DrawRect { rect, ref flags } if rect == c1 => Some(flags.color),
            _ => None,
        })
        .collect();
    assert_eq!(first, [BLUE, LIME]);
}

/// The rectangles drawn (filled) in `color`, of any paint op that fills
/// one.
fn rects(artifact: &PaintArtifact, color: Color) -> Vec<RectF> {
    fills(artifact, color)
}

/// Collapsed borders are the table's, painted after its cells'
/// backgrounds: each edge on its grid line, as wide as its border; at a
/// joint of equal edges, both take it. A cell paints no border of its own,
/// and `empty-cells` does nothing.
#[test]
fn a_table_paints_its_collapsed_borders() {
    let artifact = paint_html(&format!(
        "{STYLE}<table style='border-collapse: collapse; border: 2px solid blue'>
           <tr><td style='background: lime; border: 2px solid red; empty-cells: hide; width: 20px;
             height: 20px'></td><td style='width: 20px; height: 20px'></td></tr></table>"
    ));
    // Equal to the table's, the first cell's edges are its own (it is
    // merged first); the second cell's are the table's, stopping short of
    // the first cell's joint, over the last two.
    assert_eq!(
        rects(&artifact, BLUE),
        [
            RectF::new(24.0, 0.0, 22.0, 2.0),
            RectF::new(44.0, 0.0, 2.0, 24.0),
            RectF::new(24.0, 22.0, 22.0, 2.0),
        ]
    );
    assert_eq!(rects(&artifact, RED).len(), 4);
    // Nothing else in red: the cell draws no border of its own.
    let red = artifact.items.iter().filter(|item| match &item.op {
        PaintOp::DrawRect { flags, .. }
        | PaintOp::DrawRRect { flags, .. }
        | PaintOp::DrawDRRect { flags, .. }
        | PaintOp::DrawPath { flags, .. } => flags.color == RED,
        _ => false,
    });
    assert_eq!(red.count(), 4);
    let lime = artifact
        .items
        .iter()
        .position(|i| matches!(&i.op, PaintOp::DrawRect { flags, .. } if flags.color == LIME));
    let blue = artifact
        .items
        .iter()
        .position(|i| matches!(&i.op, PaintOp::DrawRect { flags, .. } if flags.color == BLUE));
    assert!(lime.unwrap() < blue.unwrap());
}

/// Of two collapsed borders as wide, the higher style wins the edge
/// (`double` over `solid`), and `inset` is drawn as `ridge` (two bands); a
/// cell spanning rows leaves the edge inside it to no one (not its next
/// row's border); a `hidden` border wins and draws nothing.
#[test]
fn collapsed_border_styles_and_spans() {
    let double = paint_html(&format!(
        "{STYLE}<table style='border-collapse: collapse'><tr>
           <td style='border: 6px solid red; width: 20px; height: 20px'></td>
           <td style='border: 6px double blue; width: 20px; height: 20px'></td></tr></table>"
    ));
    // The edge between them, at x 26..32 (after the table's 3px and the
    // first cell's 26px): blue's two lines.
    let between: Vec<RectF> = rects(&double, BLUE)
        .into_iter()
        .filter(|r| r.x >= 26.0 && r.right() <= 32.0 && r.height > 6.0)
        .collect();
    assert_eq!(between.len(), 2, "{between:?}");
    assert!(
        rects(&double, RED)
            .iter()
            .all(|r| r.right() <= 26.0 || r.x >= 32.0)
    );
    let inset = paint_html(&format!(
        "{STYLE}<table style='border-collapse: collapse'><tr>
           <td style='border: 4px inset lime; width: 20px; height: 20px'></td></tr></table>"
    ));
    let top: Vec<RectF> = inset
        .items
        .iter()
        .filter_map(|item| match item.op {
            PaintOp::DrawRect { rect, .. } if rect.y < 4.0 && rect.width > 20.0 => Some(rect),
            _ => None,
        })
        .collect();
    assert_eq!(top.len(), 2, "{top:?}");
    let span = paint_html(&format!(
        "{STYLE}<table style='border-collapse: collapse'>
           <tr><td rowspan=2 style='width: 20px'></td><td style='width: 20px; height: 20px'></td></tr>
           <tr style='border-top: 4px solid blue'><td style='height: 20px'></td></tr></table>"
    ));
    let blue = rects(&span, BLUE);
    assert!(
        !blue.is_empty() && blue.iter().all(|r| r.x >= 18.0),
        "{blue:?}"
    );
    let hidden = paint_html(&format!(
        "{STYLE}<table style='border-collapse: collapse; border: 9px hidden'><tr>
           <td style='border: 4px solid blue; width: 20px; height: 20px'></td></tr></table>"
    ));
    assert!(rects(&hidden, BLUE).is_empty());
    // At a joint, the wider edge draws: the red top over the blue left.
    let joint = paint_html(&format!(
        "{STYLE}<table style='border-collapse: collapse'><tr><td style='border-top: 8px solid red;
           border-left: 2px solid blue; width: 20px; height: 20px'></td></tr></table>"
    ));
    assert_eq!(rects(&joint, RED)[0].x, 0.0);
    assert_eq!(rects(&joint, BLUE)[0].y, 8.0);
    // An invisible table draws none, whatever its cells.
    let invisible = paint_html(&format!(
        "{STYLE}<table style='border-collapse: collapse; border: 2px solid blue; visibility: hidden'>
           <tr><td style='visibility: visible; width: 20px; height: 20px'></td></tr></table>"
    ));
    assert!(rects(&invisible, BLUE).is_empty());
}

/// A cell painted apart from its table (positioned) has the backgrounds
/// in it inside its collapsed border, not over the borders its table
/// painted before.
#[test]
fn a_positioned_cells_background_stays_inside_its_collapsed_border() {
    let artifact = paint_html(&format!(
        "{STYLE}<table style='border-collapse: collapse'><tr style='background: red'>
           <td style='position: relative; background: lime; border: 4px solid blue; width: 20px;
             height: 20px'></td></tr></table>"
    ));
    assert_eq!(rects(&artifact, LIME), [RectF::new(4.0, 4.0, 20.0, 20.0)]);
    assert_eq!(rects(&artifact, RED), [RectF::new(4.0, 4.0, 20.0, 20.0)]);
}
