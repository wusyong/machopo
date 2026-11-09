//! Tables: the fixup's anonymous boxes, the columns' widths from the
//! cells, the rows' heights, border spacing, captions, the cells'
//! `vertical-align` and the table's baseline.

use std::collections::BTreeMap;

use gfx::{NodeId, RectF, SizeF};
use layout::BoxFragment;

use crate::page::{Laid, lay_out};

const AHEM: &str = "<style>body { margin: 0; font: 20px/1 Ahem } table { border-spacing: 0 }
    td, th { padding: 0 }</style>";

/// The dump of the table boxes under the element with id `id`, without
/// lines: what the fixup made.
fn structure(laid: &Laid, id: &str) -> String {
    fn walk(
        fragment: &BoxFragment,
        ids: &BTreeMap<NodeId, String>,
        depth: usize,
        out: &mut String,
    ) {
        let name = match fragment.node {
            Some(node) => ids
                .get(&node)
                .map_or("element", |id| id.as_str())
                .to_owned(),
            None => "anonymous".to_owned(),
        };
        out.push_str(&format!(
            "{:indent$}{name} {:?}\n",
            "",
            fragment.style.display,
            indent = depth * 2
        ));
        for child in &fragment.children {
            walk(&child.fragment, ids, depth + 1, out);
        }
    }
    let mut out = String::new();
    walk(&laid.fragments[id], &laid.ids, 0, &mut out);
    out
}

/// Cells of `display: table-cell` next to each other go into one
/// anonymous table, row group and row, white space between them dropped;
/// content in a table that isn't in a cell goes into anonymous ones.
#[test]
fn the_fixup_makes_the_missing_boxes() {
    let laid = lay_out(&format!(
        "{AHEM}<div id=cells><div id=a style='display: table-cell'>A</div>
           <div id=b style='display: table-cell'>BB</div></div>
         <div id=bare style='display: table'>X<span>Y</span></div>
         <div id=rows style='display: table'>
           <div id=row style='display: table-row'><span id=s>S</span></div>
           <div id=c1 style='display: table-cell'>C</div></div>
         <div id=group style='display: table'><div id=g style='display: table-row-group'>
           <div id=c2 style='display: table-cell'>D</div></div></div>"
    ));
    assert_eq!(
        structure(&laid, "cells"),
        "cells Block\n  anonymous Table(Table)\n    anonymous Table(RowGroup)\n      \
         anonymous Table(Row)\n        a Table(Cell)\n        b Table(Cell)\n"
    );
    // Side by side, each as wide as its content.
    assert_eq!(laid.rect("a"), RectF::new(0.0, 0.0, 20.0, 20.0));
    assert_eq!(laid.rect("b"), RectF::new(20.0, 0.0, 40.0, 20.0));
    assert_eq!(
        structure(&laid, "bare"),
        "bare Table(Table)\n  anonymous Table(RowGroup)\n    anonymous Table(Row)\n      \
         anonymous Table(Cell)\n"
    );
    assert_eq!(laid.rect("bare").size(), SizeF::new(40.0, 20.0));
    // A row's content that isn't a cell goes into an anonymous cell; a
    // cell in a table, into an anonymous row group and row.
    assert_eq!(
        structure(&laid, "rows"),
        "rows Table(Table)\n  anonymous Table(RowGroup)\n    row Table(Row)\n      \
         anonymous Table(Cell)\n    anonymous Table(Row)\n      c1 Table(Cell)\n"
    );
    assert_eq!(
        structure(&laid, "group"),
        "group Table(Table)\n  g Table(RowGroup)\n    anonymous Table(Row)\n      c2 Table(Cell)\n"
    );
}

/// White space directly in a table, row group or row makes nothing, even
/// preserved (Chrome), unless other content is with it (between two
/// inline-blocks, it is a space on their line); preserved white space
/// between cells in a block splits their anonymous tables, as it is
/// content there.
#[test]
fn white_space_between_table_parts() {
    let laid = lay_out(&format!(
        "{AHEM}<div id=pre style='display: table; white-space: pre'> <div id=a
           style='display: table-cell'>A</div> <div id=b style='display: table-cell'>B</div> </div>
         <div id=block style='white-space: pre'><div style='display: table-cell'>A</div> <div
           style='display: table-cell'>B</div></div>
         <div id=inline style='display: table; width: 100px'><span
           style='display: inline-block; width: 50%'>A</span> <span id=second
           style='display: inline-block; width: 50%'>B</span></div>"
    ));
    assert_eq!(
        structure(&laid, "pre"),
        "pre Table(Table)\n  anonymous Table(RowGroup)\n    anonymous Table(Row)\n      \
         a Table(Cell)\n      b Table(Cell)\n"
    );
    assert_eq!(laid.rect("pre").size(), SizeF::new(40.0, 20.0));
    assert_eq!(
        structure(&laid, "block"),
        "block Block\n  anonymous Table(Table)\n    anonymous Table(RowGroup)\n      \
         anonymous Table(Row)\n        element Table(Cell)\n  anonymous Block\n  \
         anonymous Table(Table)\n    anonymous Table(RowGroup)\n      anonymous Table(Row)\n        \
         element Table(Cell)\n"
    ); // The space and the second inline-block don't fit after the first.
    let inline = laid.rect("inline");
    assert_eq!(laid.rect("second").y, inline.y + 20.0);
}

/// A cell in an inline element goes into an anonymous inline-table, an
/// atomic inline on the line, on the baseline of its first row.
#[test]
fn a_cell_in_an_inline_element_is_an_inline_table() {
    let laid = lay_out(&format!(
        "{AHEM}<div id=d>A<span>B<span id=c style='display: table-cell'>C</span> <b>D</b></span>E</div>"
    ));
    assert_eq!(laid.rect("c"), RectF::new(40.0, 0.0, 20.0, 20.0));
    assert_eq!(laid.rect("d").height, 20.0);
    // The space after it stays on the line.
    assert_eq!(laid.fragments["d"].lines[0].rect.width, 120.0);
    assert_eq!(
        structure(&laid, "d"),
        "d Block\n  anonymous InlineTable\n    anonymous Table(RowGroup)\n      \
         anonymous Table(Row)\n        c Table(Cell)\n"
    );
}

/// The columns' widths from the cells (Blink's column sizing): an `auto`
/// table as wide as its content's maximum, in the room it has; a wider
/// one gives the room to the `auto` columns as their maxima go; fixed and
/// percentage widths; a cell spanning columns (a header cell too) spreads
/// its width over them.
#[test]
fn columns_are_sized_from_their_cells() {
    let laid = lay_out(&format!(
        "{AHEM}<table id=auto><tr><td id=a1>XXXX XXXX</td><td id=a2>X</td></tr></table>
         <div style='width: 100px'><table id=narrow><tr><td id=n1>XXXX XXXX XXXX XXXX</td>
           <td id=n2>X</td></tr></table></div>
         <table id=wide style='width: 200px'><tr><td id=w1>XXXX XXXX XXXX XXXX</td>
           <td id=w2>X</td></tr></table>
         <table><tr><td id=f1 style='width: 50px'>X</td><td id=f2>XX</td></tr></table>
         <table style='width: 400px'><tr><td id=p1 style='width: 25%'>X</td><td id=p2>X</td></tr>
           </table>
         <table><tr><td colspan=2 id=span>XXXXXX</td></tr><tr><td id=s1>X</td><td id=s2>XX</td>
           </tr></table>
         <table><tr><th colspan=2 id=head>X</th></tr><tr><td>XXXX</td><td>XXXX</td></tr></table>
         <div style='width: 10px'><table id=tight><tr><td colspan=2>XXXXXX</td></tr><tr>
           <td id=t1>X</td><td id=t2>XX</td></tr></table></div>
         <div style='width: 50px'><table id=min><tr><td>XXXX</td></tr></table></div>
         <table id=less style='width: 10px'><tr><td>XXXX</td></tr></table>"
    ));
    assert_eq!(laid.rect("auto").width, 200.0);
    assert_eq!(laid.rect("a1").width, 180.0);
    // The room, each column at its minimum.
    assert_eq!(laid.rect("narrow").width, 100.0);
    assert_eq!(laid.rect("n1").width, 80.0);
    assert_eq!(laid.rect("n2").width, 20.0);
    // 100px over the minima, all to the column whose maximum is wider.
    assert_eq!(laid.rect("w1").width, 180.0);
    assert_eq!(laid.rect("w2").width, 20.0);
    // A fixed width is the column's maximum.
    assert_eq!(laid.rect("f1").width, 50.0);
    assert_eq!(laid.rect("f2").width, 40.0);
    // A percentage of the table; the `auto` column takes the rest.
    assert_eq!(laid.rect("p1").width, 100.0);
    assert_eq!(laid.rect("p2").width, 300.0);
    // 60px beyond the columns' 60px, in proportion to their maxima.
    assert_eq!(laid.rect("span").width, 120.0);
    assert_eq!(laid.rect("s1").width, 40.0);
    assert_eq!(laid.rect("s2").width, 80.0);
    assert_eq!(
        laid.rect("head").width,
        160.0,
        "a header cell spans as a cell"
    );
    // Its minimum is spread the same way: at its minimum, the table is.
    assert_eq!(laid.rect("tight").width, 120.0);
    assert_eq!(laid.rect("t1").width, 40.0);
    assert_eq!(laid.rect("t2").width, 80.0);
    // Never narrower than its content's minimum.
    assert_eq!(laid.rect("min").width, 80.0);
    assert_eq!(laid.rect("less").width, 80.0);
}

/// Border spacing around and between the cells; the row groups and rows
/// span the columns, not the spacing outside them; a column no cell starts
/// in is merged away, with no spacing of its own.
#[test]
fn border_spacing_and_the_parts_boxes() {
    let laid = lay_out(&format!(
        "{AHEM}<table id=t style='border-spacing: 5px 7px; border: 2px solid'><tbody id=g>
           <tr id=r1><td id=c1>X</td><td id=c2>XX</td></tr><tr id=r2><td id=c3>X</td><td>X</td>
           </tr></tbody></table>
         <table id=m style='border-spacing: 10px'><tr><td id=mc colspan=3>X</td></tr></table>"
    ));
    assert_eq!(laid.rect("t").size(), SizeF::new(79.0, 65.0));
    assert_eq!(laid.rect("g"), RectF::new(7.0, 9.0, 65.0, 47.0));
    assert_eq!(laid.rect("r1"), RectF::new(7.0, 9.0, 65.0, 20.0));
    assert_eq!(laid.rect("r2"), RectF::new(7.0, 36.0, 65.0, 20.0));
    assert_eq!(laid.rect("c1"), RectF::new(7.0, 9.0, 20.0, 20.0));
    assert_eq!(laid.rect("c2"), RectF::new(32.0, 9.0, 40.0, 20.0));
    assert_eq!(laid.rect("c3"), RectF::new(7.0, 36.0, 20.0, 20.0));
    let m = laid.rect("m");
    assert_eq!(m.size(), SizeF::new(40.0, 40.0));
    assert_eq!(
        laid.rect("mc"),
        RectF::new(m.x + 10.0, m.y + 10.0, 20.0, 20.0)
    );
}

/// A row is as tall as its tallest cell and its own `height`; a cell's
/// `height` is a minimum, its `max-height` nothing; every cell is as tall as its row; a cell spanning
/// rows makes room for itself in them.
#[test]
fn rows_are_as_tall_as_their_cells() {
    let laid = lay_out(&format!(
        "{AHEM}<table><tr><td id=h1 style='height: 50px'>X</td><td id=h2>X</td></tr>
           <tr style='height: 30px'><td id=h3>X</td><td>X</td></tr></table>
         <table id=rows><tr><td id=rs rowspan=2 style='height: 100px'>X</td><td id=r1>X</td></tr>
           <tr><td id=r2>X</td></tr></table>
         <table><tr><td id=max style='height: 10px; max-height: 10px; overflow: hidden'>
           <div style='height: 40px'></div></td></tr></table>"
    ));
    assert_eq!(laid.rect("h1").height, 50.0);
    assert_eq!(laid.rect("h2").height, 50.0);
    let h3 = laid.rect("h3");
    assert_eq!((h3.y - laid.rect("h1").y, h3.height), (50.0, 30.0));
    let top = laid.rect("rows").y;
    assert_eq!(laid.rect("rs"), RectF::new(0.0, top, 20.0, 100.0));
    assert_eq!(laid.rect("r1"), RectF::new(20.0, top, 20.0, 50.0));
    assert_eq!(laid.rect("r2"), RectF::new(20.0, top + 50.0, 20.0, 50.0));
    assert_eq!(laid.rect("max").height, 40.0);
}

/// The first line's place in a cell, from its top.
fn first_line_y(laid: &Laid, id: &str) -> f32 {
    laid.fragments[id].lines[0].rect.y
}

/// A cell's content goes to its top, middle or bottom, or onto its row's
/// baseline; the cells of an HTML table are `middle`, as their row group.
#[test]
fn vertical_align_places_a_cells_content() {
    let laid = lay_out(&format!(
        "{AHEM}<table><tr><td style='height: 60px'>X</td><td id=top valign=top>X</td>
           <td id=middle>X</td><td id=bottom style='vertical-align: bottom'>X</td></tr></table>
         <table><tr><td id=small style='vertical-align: baseline'>X</td>
           <td id=big style='vertical-align: baseline; font-size: 40px'>X</td>
           <td id=sub style='vertical-align: sub'>X</td></tr></table>"
    ));
    assert_eq!(first_line_y(&laid, "top"), 0.0);
    assert_eq!(first_line_y(&laid, "middle"), 20.0);
    assert_eq!(first_line_y(&laid, "bottom"), 40.0);
    // On the baseline of the bigger text, 32px down: the row as tall as
    // that and the bigger text's 8px below it.
    assert_eq!(laid.rect("small").height, 40.0);
    assert_eq!(first_line_y(&laid, "small"), 16.0);
    assert_eq!(first_line_y(&laid, "big"), 0.0);
    // Any other value is `baseline` in a cell.
    assert_eq!(first_line_y(&laid, "sub"), 16.0);
}

/// The block children of a cell move with its content.
#[test]
fn a_cells_block_content_is_aligned() {
    let laid = lay_out(&format!(
        "{AHEM}<table><tr><td style='height: 60px'>X</td><td id=cell><div id=block>X</div></td>
           </tr></table>"
    ));
    let cell = laid.rect("cell");
    assert_eq!(laid.rect("block").y - cell.y, 20.0);
}

/// Captions above and below the grid, as wide as the table, which is no
/// narrower than they are; a relatively positioned one moved.
#[test]
fn captions_go_above_and_below() {
    let laid = lay_out(&format!(
        "{AHEM}<table id=t><caption id=top>XXXXXX</caption>
           <caption id=bottom style='caption-side: bottom; position: relative; left: -5px;
             top: 10%'>X</caption>
           <tr><td id=cell>X</td></tr></table>"
    ));
    assert_eq!(laid.rect("t"), RectF::new(0.0, 0.0, 120.0, 60.0));
    assert_eq!(laid.rect("top"), RectF::new(0.0, 0.0, 120.0, 20.0));
    assert_eq!(laid.rect("cell"), RectF::new(0.0, 20.0, 120.0, 20.0));
    // Moved by its `position: relative` (`top` a percentage of nothing).
    assert_eq!(laid.rect("bottom"), RectF::new(-5.0, 40.0, 120.0, 20.0));
}

/// A table's baseline is its first row's: on a line as an inline-table,
/// and as the last thing in an inline-block.
#[test]
fn a_tables_baseline_is_its_first_rows() {
    let row = "<tr><td style='vertical-align: baseline; font-size: 40px'>X</td></tr>";
    let laid = lay_out(&format!(
        "{AHEM}<div id=line>A<table id=inline style='display: inline-table'>{row}</table>B</div>
         <div id=line2>A<span id=block style='display: inline-block'><table>{row}</table></span></div>"
    ));
    let line = laid.rect("line");
    assert_eq!(laid.rect("inline"), RectF::new(20.0, line.y, 40.0, 40.0));
    assert_eq!(line.height, 40.0);
    assert_eq!(laid.rect("block").y, laid.rect("line2").y);
    assert_eq!(laid.rect("line2").height, 40.0);
}

/// An empty table is its border and its own size; a table isn't stretched
/// by its containing block, and keeps clear of the floats beside it, as an
/// element that makes a block formatting context does.
#[test]
fn a_table_in_a_block() {
    let laid = lay_out(&format!(
        "{AHEM}<table id=empty style='width: 50px; height: 30px; border: 1px solid'></table>
         <table id=bare style='border: 2px solid'></table>
         <div style='width: 200px'><table id=centered style='margin: 0 auto'><tr><td>XX</td></tr>
           </table></div>
         <div style='width: 200px'><div style='float: left; width: 50px; height: 50px'></div>
           <table id=beside><tr><td>X</td></tr></table></div>
         <div style='display: inline-block'><table id=fit><tr><td>XX XX</td></tr></table></div>"
    ));
    assert_eq!(laid.rect("empty").size(), SizeF::new(50.0, 30.0));
    assert_eq!(laid.rect("bare").size(), SizeF::new(4.0, 4.0));
    assert_eq!(laid.rect("centered").x, 80.0);
    assert_eq!(laid.rect("beside").x, 50.0);
    assert_eq!(laid.rect("fit").width, 100.0);
}

/// An out-of-flow box in a cell: laid out by a positioned cell, or passed
/// up through the table from where the cell's aligned content has it.
#[test]
fn out_of_flow_boxes_in_cells() {
    let laid = lay_out(&format!(
        "{AHEM}<div style='height: 10px'></div><table><tr><td style='position: relative'>X<div
           id=abs style='position: absolute; left: 5px; top: 6px; width: 1px; height: 1px'></div>
           </td></tr></table>
         <div id=container style='position: relative'><table><tr><td style='height: 40px'>X<span id=static
           style='position: absolute'>Y</span></td><td style='height: 40px'><div id=alone
           style='position: absolute; width: 1px; height: 1px'></div></td></tr></table></div>"
    ));
    assert_eq!(laid.rect("abs"), RectF::new(5.0, 16.0, 1.0, 1.0));
    // After `X` on the middle-aligned line, 10px down the 40px cell.
    let container = laid.rect("container");
    assert_eq!(
        laid.rect("static"),
        RectF::new(20.0, container.y + 10.0, 20.0, 20.0)
    );
    // Nothing in flow to follow: at the top of its cell.
    assert_eq!(laid.rect("alone").y, container.y);
}

/// An out-of-flow box among a row's cells is the row's, not in a cell of
/// its own; a part in an inline element in a row is in an anonymous cell
/// with it, in an anonymous inline-table.
#[test]
fn what_isnt_a_rows_cell() {
    let laid = lay_out(&format!(
        "{AHEM}<div style='display: table'><div id=row style='display: table-row'>
           <div id=c style='display: table-cell'>X</div><div id=abs
           style='position: absolute; left: 5px; top: 6px'>Y</div></div></div>
         <div style='display: table'><div id=row2 style='display: table-row'><span>
           <div id=inner style='display: table-cell'>Z</div></span></div></div>"
    ));
    assert_eq!(structure(&laid, "row"), "row Table(Row)\n  c Table(Cell)\n");
    assert_eq!(laid.rect("abs"), RectF::new(5.0, 6.0, 20.0, 20.0));
    assert_eq!(
        structure(&laid, "row2"),
        "row2 Table(Row)\n  anonymous Table(Cell)\n    anonymous InlineTable\n      \
         anonymous Table(RowGroup)\n        anonymous Table(Row)\n          inner Table(Cell)\n"
    );
}

/// A positioned row is the containing block of an out-of-flow box in its
/// cells.
#[test]
fn a_positioned_row_is_a_containing_block() {
    let laid = lay_out(&format!(
        "{AHEM}<div style='height: 10px'></div><table><tr><td>X</td></tr>
           <tr style='position: relative'><td>X<div id=abs
           style='position: absolute; left: 5px; top: 6px; width: 1px; height: 1px'></div></td>
           </tr></table>"
    ));
    assert_eq!(laid.rect("abs"), RectF::new(5.0, 36.0, 1.0, 1.0));
}

/// The header group goes first and the footer group last, wherever they
/// are; a rowspan of 0 spans the rest of its row group.
#[test]
fn row_groups_and_rowspans() {
    let laid = lay_out(&format!(
        "{AHEM}<table><tfoot><tr><td id=f>F</td></tr></tfoot><tbody><tr><td id=b>B</td></tr>
           </tbody><thead><tr><td id=h>H</td></tr></thead></table>
         <table id=t><tr><td id=zero rowspan=0>X</td><td>X</td></tr><tr><td>X</td></tr>
           <tr><td id=last>X</td></tr></table>"
    ));
    assert_eq!(laid.rect("h").y, 0.0);
    assert_eq!(laid.rect("b").y, 20.0);
    assert_eq!(laid.rect("f").y, 40.0);
    let top = laid.rect("t").y;
    assert_eq!(laid.rect("zero"), RectF::new(0.0, top, 20.0, 60.0));
    assert_eq!(laid.rect("last"), RectF::new(20.0, top + 40.0, 20.0, 20.0));
}

/// A cell goes in the first column from where the row's last ended that
/// no cell from a row above spans down into, past several such cells, in
/// whatever order those started. One spanning columns may lie over one
/// from above (the third row's `g` over `c`); the next cell goes after it.
/// Each table's cells are where Chrome 152 puts them.
#[test]
fn cells_skip_the_columns_spanned_from_above() {
    let laid = lay_out(&format!(
        "{AHEM}<table style='table-layout: fixed; width: 60px; border-spacing: 0'>
           <col style='width: 10px' span=6>
           <tr><td id=a rowspan=4>X</td><td id=b>X</td><td id=c rowspan=3 colspan=2>X</td>
             <td id=d>X</td></tr>
           <tr><td id=e>X</td><td id=f>X</td></tr>
           <tr><td id=g colspan=3>X</td></tr>
           <tr><td id=h>X</td><td id=i rowspan=0>X</td></tr>
           <tr><td id=j>X</td><td id=k>X</td></tr></table>"
    ));
    let x = |id: &str| laid.rect(id).x;
    assert_eq!(
        ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k"].map(x),
        [
            0.0, 10.0, 20.0, 40.0, 10.0, 40.0, 10.0, 10.0, 20.0, 0.0, 10.0
        ]
    );
    // A cell spanning rows from a later row, left of one from an earlier:
    // the third row's `z` is past both.
    let laid = lay_out(&format!(
        "{AHEM}<table style='table-layout: fixed; width: 60px; border-spacing: 0'>
           <col style='width: 10px' span=6>
           <tr><td id=w>X</td><td id=x rowspan=3>X</td></tr>
           <tr><td id=y rowspan=2>X</td></tr>
           <tr><td id=z>X</td></tr></table>"
    ));
    let x = |id: &str| laid.rect(id).x;
    assert_eq!(["w", "x", "y", "z"].map(x), [0.0, 10.0, 0.0, 20.0]);
    // A cell spanning columns over one from above: the next goes after it,
    // not back to where the one from above ends.
    let laid = lay_out(&format!(
        "{AHEM}<table style='table-layout: fixed; width: 60px; border-spacing: 0'>
           <col style='width: 10px' span=6>
           <tr><td id=p>X</td><td id=q rowspan=2>X</td></tr>
           <tr><td id=r colspan=3>X</td><td id=s>X</td></tr></table>"
    ));
    let x = |id: &str| laid.rect(id).x;
    assert_eq!(["p", "q", "r", "s"].map(x), [0.0, 10.0, 0.0, 30.0]);
}

/// A table of many columns (a row of a thousand cells, each of a thousand
/// columns) and many rows is laid out in time as cells are many, not
/// columns times rows: only the cells spanning rows are looked at to find
/// a row's free columns, as Blink does.
#[test]
fn many_columns_and_many_rows() {
    let html = format!(
        "{AHEM}<table style='border-spacing: 0'><tr>{}</tr>{}<tr><td id=last>X</td></tr></table>",
        "<td colspan=1000></td>".repeat(1000),
        "<tr></tr>".repeat(10_000)
    );
    let laid = lay_out(&html);
    assert_eq!(laid.rect("last").x, 0.0);
}

/// The rest of the column sizing: a column's fixed width holds against a
/// cell's content; `min-width` and `max-width` on cells; growing fixed or
/// percentage columns past their widths; percentage columns between their
/// minimum and percentage; an `auto` table wide enough for its percentage
/// columns to be their percentages; percentages held to 100; a cell's
/// padding percentage of nothing for the columns, then of the table but
/// its outer border spacing.
#[test]
fn more_column_sizing() {
    let laid = lay_out(&format!(
        "{AHEM}<table id=fixed><tr><td style='width: 50px'>X</td></tr><tr><td>XX XX</td></tr>
           </table>
         <table><tr><td id=min style='min-width: 60px'>X</td><td id=max style='max-width: 30px'>XX</td>
           </tr></table>
         <table><tr><td id=both style='min-width: 50px; max-width: 30px'>X</td></tr></table>
         <table style='width: 200px'><tr><td id=g1 style='width: 50px'>X</td>
           <td id=g2 style='width: 30px'>X</td></tr></table>
         <table style='width: 400px'><tr><td id=gp1 style='width: 25%'>X</td>
           <td id=gp2 style='width: 25%'>X</td></tr></table>
         <table style='width: 100px'><tr><td id=pp1 style='width: 50%'>X</td><td id=pp2>XXX</td>
           </tr></table>
         <table id=estimate><tr><td id=e1 style='width: 50%'>X</td><td id=e2>XXXX</td></tr></table>
         <table style='width: 400px'><tr><td id=c1 style='width: 80%'>X</td>
           <td id=c2 style='width: 80%'>X</td></tr></table>
         <table style='border-spacing: 10px 0'><tr><td id=pad style='padding-left: 50%'>
           <div id=padded style='width: 100px; height: 1px'></div></td><td>XXXXX</td></tr></table>
         <table style='border-spacing: 0'><tr><td id=calc_pad style='padding-left: calc(10% + 10px)'>
           <div style='width: 50px; height: 1px'></div></td></tr></table>"
    ));
    assert_eq!(laid.rect("fixed").width, 50.0);
    assert_eq!(laid.rect("min").width, 60.0);
    assert_eq!(laid.rect("max").width, 30.0);
    assert_eq!(laid.rect("both").width, 50.0);
    // 120px over the fixed widths, in proportion to them.
    assert_eq!(laid.rect("g1").width, 125.0);
    assert_eq!(laid.rect("g2").width, 75.0);
    // 200px over the percentages, in proportion to them.
    assert_eq!(laid.rect("gp1").width, 200.0);
    assert_eq!(laid.rect("gp2").width, 200.0);
    // 20px over the minima, to the percentage column.
    assert_eq!(laid.rect("pp1").width, 40.0);
    assert_eq!(laid.rect("pp2").width, 60.0);
    // 80px is 50%: as wide as the other column's 80px.
    assert_eq!(laid.rect("estimate").width, 160.0);
    assert_eq!(laid.rect("e1").width, 80.0);
    assert_eq!(laid.rect("e2").width, 80.0);
    // The second is 20%.
    assert_eq!(laid.rect("c1").width, 320.0);
    assert_eq!(laid.rect("c2").width, 80.0);
    // Columns of 100px, 10px apart: 105px of padding.
    let pad = laid.rect("pad");
    assert_eq!(pad.width, 100.0);
    assert_eq!(laid.rect("padded").x, pad.x + 105.0);
    // The 10px of the `calc()` count for the column.
    assert_eq!(laid.rect("calc_pad").width, 60.0);
}

/// A table's own sizes: `min-width` and `max-width` (never below its
/// content's minimum), and `height` (to its rows).
#[test]
fn a_tables_own_sizes() {
    let laid = lay_out(&format!(
        "{AHEM}<table id=max style='max-width: 50px'><tr><td>XXXX</td></tr></table>
         <table id=min style='min-width: 300px'><tr><td>X</td></tr></table>
         <table id=tall style='height: 100px'><tr><td id=cell>X</td></tr></table>"
    ));
    assert_eq!(laid.rect("max").width, 80.0);
    assert_eq!(laid.rect("min").width, 300.0);
    assert_eq!(laid.rect("tall").height, 100.0);
    assert_eq!(laid.rect("cell").height, 100.0);
}

/// A table's margins don't collapse through it, empty as it is.
#[test]
fn an_empty_tables_margins_dont_collapse_through_it() {
    let laid = lay_out(&format!(
        "{AHEM}<div id=d style='border: 1px solid'><table style='margin: 10px 0'></table>
           <div id=next style='height: 1px'></div></div>"
    ));
    assert_eq!(laid.rect("next").y - laid.rect("d").y, 21.0);
}

/// A row without baseline-aligned cells has its first cell's baseline; an
/// empty cell's is its content box's bottom, not at its `height`, and the
/// table's at its final height; a cell's margins do nothing;
/// an inline-table that clips is aligned by its first row too.
#[test]
fn more_baselines() {
    let laid = lay_out(&format!(
        "{AHEM}<div id=line>A<table id=t style='display: inline-table'><tr><td
           style='vertical-align: top; font-size: 40px'>X</td></tr></table></div>
         <table><tr><td id=text style='vertical-align: baseline'>X</td><td
           style='vertical-align: baseline; padding-bottom: 10px'></td></tr></table>
         <table><tr><td id=margin style='margin: 10px'>X</td></tr></table>
         <table><tr><td id=beside style='vertical-align: baseline'>X</td>
           <td style='vertical-align: baseline; height: 50px'></td></tr></table>
         <div id=line2>A<table id=clips style='display: inline-table; overflow: hidden'><tr><td
           style='vertical-align: baseline; font-size: 40px'>X</td></tr></table></div>
         <div id=line3>A<table id=hollow style='display: inline-table'><tr><td
           style='height: 50px'></td></tr></table></div>
         <div id=line4>A<table id=tall style='display: inline-table; height: 100px'><tr><td
           style='vertical-align: baseline'></td></tr></table></div>"
    ));
    // Its baseline 32px down, A's 16px down: the table at the top.
    assert_eq!(laid.rect("t").y, laid.rect("line").y);
    assert_eq!(laid.rect("line").height, 40.0);
    // 16px above the shared baseline, the empty cell's 10px of padding
    // below it.
    assert_eq!(laid.rect("text").height, 26.0);
    let margin = laid.rect("margin");
    assert_eq!(margin.size(), SizeF::new(20.0, 20.0));
    // The empty cell's baseline at its content's 0px: the text stays up.
    let beside = laid.rect("beside");
    assert_eq!(beside.height, 50.0);
    assert_eq!(first_line_y(&laid, "beside"), 0.0);
    // An inline-table's baseline is its first row's, whatever its
    // `overflow` (Blink's table layout sets it as it is).
    assert_eq!(laid.rect("clips").y, laid.rect("line2").y);
    assert_eq!(laid.rect("line2").height, 40.0);
    // On the bottom of the cell's 50px (its final height), A's baseline
    // 16px down.
    assert_eq!(laid.rect("hollow").y, laid.rect("line3").y);
    assert_eq!(laid.rect("line3").height, 54.0);
    // Laid out empty, then stretched to the table's 100px.
    assert_eq!(laid.rect("tall").y, laid.rect("line4").y);
    assert_eq!(laid.rect("line4").height, 104.0);
}

/// `table-layout: fixed` with a `width`: the columns from the first row's
/// cells' widths alone, the rest shared out evenly to the `auto` columns;
/// without `auto` ones, to the fixed ones in proportion; no narrower than
/// the fixed columns; without a `width`, an `auto` layout.
#[test]
fn fixed_table_layout() {
    let laid = lay_out(&format!(
        "{AHEM}<style>.fixed {{ table-layout: fixed }}</style>
         <table class=fixed style='width: 300px'><tr><td id=f1 style='width: 100px'>X</td>
           <td id=f2>X</td><td id=f3>X</td></tr><tr><td>XXXXXXXXXX</td><td>X</td><td>X</td></tr>
           </table>
         <table class=fixed style='width: 400px'><tr><td id=p1 style='width: 25%'>XXXXXXXX</td>
           <td id=p2>X</td></tr></table>
         <table class=fixed style='width: 300px'><tr><td colspan=2 style='width: 100px'>X</td>
           <td>X</td></tr><tr><td id=s1>X</td><td id=s2>X</td><td id=s3>X</td></tr></table>
         <table class=fixed style='width: 300px'><tr><td id=g1 style='width: 50px'>X</td>
           <td id=g2 style='width: 100px'>X</td></tr></table>
         <table id=narrow class=fixed style='width: 50px'><tr><td style='width: 100px'>X</td>
           </tr></table>
         <table id=auto class=fixed><tr><td>XXXX</td></tr></table>
         <table class=fixed style='width: 100px'><tr><td id=t1 style='width: 50px'>X</td>
           <td id=t2 style='width: 80%'>X</td></tr></table>
         <table class=fixed style='width: 400px'><tr><td colspan=2 style='width: 50%'>X</td>
           <td>X</td></tr><tr><td id=sp1>X</td><td id=sp2>X</td><td id=sp3>X</td></tr></table>
         <table class=fixed style='width: 200px'><tr><td id=pc1 style='width: 25%'>X</td>
           <td id=pc2 style='width: 25%'>X</td></tr></table>"
    ));
    assert_eq!(laid.rect("f1").width, 100.0);
    assert_eq!(laid.rect("f2").width, 100.0);
    assert_eq!(laid.rect("f3").width, 100.0);
    // Narrower than its content.
    assert_eq!(laid.rect("p1").width, 100.0);
    assert_eq!(laid.rect("p2").width, 300.0);
    assert_eq!(laid.rect("s1").width, 50.0);
    assert_eq!(laid.rect("s2").width, 50.0);
    assert_eq!(laid.rect("s3").width, 200.0);
    assert_eq!(laid.rect("g1").width, 100.0);
    assert_eq!(laid.rect("g2").width, 200.0);
    assert_eq!(laid.rect("narrow").width, 100.0);
    assert_eq!(laid.rect("auto").width, 80.0);
    // Too narrow for both: the percentage column gets what is left.
    assert_eq!(laid.rect("t1").width, 50.0);
    assert_eq!(laid.rect("t2").width, 50.0);
    // A spanning cell's percentage, shared out.
    assert_eq!(laid.rect("sp1").width, 100.0);
    assert_eq!(laid.rect("sp2").width, 100.0);
    assert_eq!(laid.rect("sp3").width, 200.0);
    // Only percentages: they grow in proportion.
    assert_eq!(laid.rect("pc1").width, 100.0);
    assert_eq!(laid.rect("pc2").width, 100.0);
}

/// What a cell spanning rows needs beyond them goes to the rows without a
/// fixed height (a cell's counts), in proportion to their heights; a
/// table's `height` beyond its rows to its row groups without a fixed
/// height first (a body first among empty ones), then to those with one,
/// bodies first; a row group's to its rows.
#[test]
fn heights_are_spread_over_rows() {
    let laid = lay_out(&format!(
        "{AHEM}<table><tr><td rowspan=2 style='height: 120px'>X</td><td id=q1>X</td></tr>
           <tr><td id=q2 style='height: 40px'>X</td></tr></table>
         <table style='height: 110px'><thead><tr><td id=h>X</td></tr></thead><tbody>
           <tr><td id=b1>X</td></tr><tr><td id=b2 style='height: 40px'>X</td></tr></tbody></table>
         <table style='height: 60px'><thead><tr><td id=only>X</td></tr></thead></table>
         <table><tbody style='height: 60px'><tr><td id=g1>X</td></tr><tr><td id=g2>X</td></tr>
           </tbody></table>
         <table style='height: 200px'><thead style='height: 20px'><tr><td id=fh>X</td></tr></thead>
           <tbody style='height: 20px'><tr><td id=fb>X</td></tr></tbody></table>
         <table style='height: 160px'><tr><td id=p1><div style='height: 20px'></div></td></tr>
           <tr><td id=p2><div style='height: 60px'></div></td></tr></table>"
    ));
    assert_eq!(laid.rect("q1").height, 80.0);
    assert_eq!(laid.rect("q2").height, 40.0);
    // The header has no fixed height, the body a cell with one.
    assert_eq!(laid.rect("h").height, 50.0);
    assert_eq!(laid.rect("b1").height, 20.0);
    assert_eq!(laid.rect("b2").height, 40.0);
    assert_eq!(laid.rect("only").height, 60.0);
    assert_eq!(laid.rect("g1").height, 30.0);
    assert_eq!(laid.rect("g2").height, 30.0);
    // Both fixed: the body takes it all.
    assert_eq!(laid.rect("fh").height, 20.0);
    assert_eq!(laid.rect("fb").height, 180.0);
    // 80px in proportion to 20px and 60px.
    assert_eq!(laid.rect("p1").height, 40.0);
    assert_eq!(laid.rect("p2").height, 120.0);
}

/// A cell is laid out again at its final height for a child with a
/// percentage height, if its height is set (its own or its table's; not
/// its row's, as in Chrome), and when it is positioned, for the
/// out-of-flow boxes in it.
#[test]
fn a_cell_laid_out_at_its_height() {
    let laid = lay_out(&format!(
        "{AHEM}<table><tr><td style='height: 60px'><div id=own style='height: 100%'></div></td>
           </tr></table>
         <table style='height: 80px'><tr><td><div id=table style='height: 50%'></div></td></tr>
           </table>
         <table><tr style='height: 60px'><td><div id=row style='height: 100%'></div></td></tr>
           </table>
         <table><tr><td><div id=none style='height: 100%'>X</div></td>
           <td style='height: 60px'>Y</td></tr></table>
         <table><tr><td id=cell style='position: relative'>X<div id=bottom
           style='position: absolute; bottom: 0; width: 1px; height: 10px'></div></td>
           <td style='height: 60px'>Y</td></tr></table>
         <div id=static-container style='position: relative'><table><tr><td style='height: 60px;
           vertical-align: top'><div style='height: 100%'></div><div id=static
           style='position: absolute; width: 1px; height: 1px'></div></td></tr></table></div>"
    ));
    assert_eq!(laid.rect("own").height, 60.0);
    // Its static position below the child with the cell's height.
    assert_eq!(
        laid.rect("static").y,
        laid.rect("static-container").y + 60.0
    );
    assert_eq!(laid.rect("table").height, 40.0);
    assert_eq!(laid.rect("row").height, 0.0);
    assert_eq!(laid.rect("none").height, 20.0);
    let cell = laid.rect("cell");
    assert_eq!(laid.rect("bottom").y, cell.y + 50.0);
}

/// A table as a flex item and as an out-of-flow box is as wide as its
/// content, between its minimum and maximum; a flex item's percentage
/// `width` is its basis; a block sized to its content is as wide as the
/// table's content makes it, over its `width`, which says whether the
/// table fits beside a float.
#[test]
fn a_table_sized_to_fit() {
    let laid = lay_out(&format!(
        "{AHEM}<div style='display: flex'><table id=item><tr><td>XX XX</td></tr></table></div>
         <div style='display: flex; width: 30px'><table id=shrunk><tr><td>XX XX</td></tr></table>
           </div>
         <div style='position: relative; width: 400px'><table id=abs style='position: absolute'>
           <tr><td>XX XX</td></tr></table></div>
         <div style='display: flex; width: 200px'><div style='width: 200px'></div>
           <table id=basis style='width: 100%'><tr><td>XX XX</td></tr></table></div>
         <div id=fit style='width: min-content'><table style='width: 10px; margin: 0 5px'>
           <tr><td>XX</td></tr></table></div>
         <div id=floats style='width: 300px'><div style='float: left; width: 200px; height: 20px'></div>
           <table id=below style='width: 50%'><tr><td>X</td></tr></table></div>"
    ));
    assert_eq!(laid.rect("item").width, 100.0);
    assert_eq!(laid.rect("shrunk").width, 40.0);
    assert_eq!(laid.rect("abs").width, 100.0);
    // A percentage `width` is its flex basis: shrunk as much as the
    // other item, to 100px.
    assert_eq!(laid.rect("basis").width, 100.0);
    assert_eq!(laid.rect("fit").width, 50.0);
    let floats = laid.rect("floats");
    let below = laid.rect("below");
    assert_eq!((below.x, below.y), (floats.x, floats.y + 20.0));
}

/// What Chrome's column sizing does that the tests above don't show: a
/// table sized by its container (a flex item, a cell) or `width:
/// max-content` is as wide as its columns' maxima, its percentages not
/// widening it, as they do a table laid out in its own room; past every
/// maximum, a fixed column grows before a percentage one; without columns,
/// rows are as wide as the table.
#[test]
fn intrinsic_widths_and_growth_like_chrome() {
    let row = "<tr><td style='width: 50%'><div style='width: 30px; height: 1px'></div></td>
        <td><div style='width: 100px; height: 1px'></div></td></tr>";
    let laid = lay_out(&format!(
        "{AHEM}<div style='width: 600px'><table id=room>{row}</table></div>
         <div style='display: flex'><table id=flex>{row}</table></div>
         <table id=max style='width: max-content'>{row}</table>
         <table style='width: 300px'><tr><td id=pc style='width: 10%'>X</td>
           <td id=fx style='width: 20px'>X</td></tr></table>
         <table style='width: 300px'><tr><td id=calc style='width: calc(10% + 0px)'>X</td>
           <td style='width: 20px'>X</td></tr></table>
         <table id=empty style='height: 60px; width: 60px; border-spacing: 10px'><tr id=er></tr>
           </table>"
    ));
    // In its own room: 100px is the half that isn't 50%.
    assert_eq!(laid.rect("room").width, 200.0);
    assert_eq!(laid.rect("flex").width, 130.0);
    assert_eq!(laid.rect("max").width, 130.0);
    // The percentage column at its percentage, the fixed one the rest.
    assert_eq!(laid.rect("pc").width, 30.0);
    assert_eq!(laid.rect("fx").width, 270.0);
    // A percentage and 0px is the percentage.
    assert_eq!(laid.rect("calc").width, 30.0);
    let empty = laid.rect("empty");
    assert_eq!(
        laid.rect("er"),
        RectF::new(empty.x, empty.y + 10.0, 60.0, 40.0)
    );
}

/// Fixed layout like Chrome's: any `width` but `auto` makes it fixed
/// (`min-content` is its fixed columns); every spanned column has its
/// share, merged or not; a percentage cell's padding adds to its width
/// (`content-box`), and the percentage columns are scaled to fit, a 0
/// column not growing.
#[test]
fn fixed_layout_like_chrome() {
    let laid = lay_out(&format!(
        "{AHEM}<style>table {{ table-layout: fixed; border-spacing: 0 }}</style>
         <table id=min style='width: min-content'><tr><td style='width: 200px'>X</td>
           <td><div style='width: 100px'>X</div></td></tr></table>
         <table style='width: 200px'><tr><td id=two colspan=2></td><td id=eight colspan=8></td>
           </tr></table>
         <table style='width: 136px'><tr><td id=p1 style='width: 50%; padding: 0 6px'>X</td>
           <td id=p2 style='width: 30%; padding: 0 6px'>X</td>
           <td id=p3 style='width: 20%; padding: 0 6px'>X</td></tr></table>
         <table style='width: 100px'><tr><td id=zero style='width: 0'></td>
           <td id=half style='width: 50%'></td></tr></table>
         <table style='width: 100px'><tr><td id=calc style='width: calc(20% + 0px)'></td>
           <td></td></tr></table>
         <table style='width: 200px'><tr><td id=span2 colspan=2 style='width: 50%; padding: 0 10px'>
           </td><td id=after></td></tr></table>"
    ));
    assert_eq!(laid.rect("min").width, 200.0);
    assert_eq!(laid.rect("two").width, 40.0);
    assert_eq!(laid.rect("eight").width, 160.0);
    // 80, 52.8 and 39.2 scaled to the 136px they fit in.
    let p1 = laid.rect("p1").width;
    assert!((p1 - 63.256).abs() < 0.01, "{p1}");
    let total = p1 + laid.rect("p2").width + laid.rect("p3").width;
    assert!((total - 136.0).abs() < 0.01, "{total}");
    assert_eq!(laid.rect("zero").width, 0.0);
    assert_eq!(laid.rect("half").width, 100.0);
    assert_eq!(laid.rect("calc").width, 20.0);
    // A spanning cell's percentage shared out, not its padding.
    assert_eq!(laid.rect("span2").width, 100.0);
    assert_eq!(laid.rect("after").width, 100.0);
}

/// Heights like Chrome's: a table's `height` beyond its rows goes to a
/// row group with a percentage `height` first, up to it, then to those
/// with a fixed one before those with a percentage; rows that only a cell
/// spans give it all in the last; a baseline-aligned cell's `height`
/// doesn't push the row below the baseline; a percentage height in a cell
/// with a `height` is of nothing while the row is measured; an
/// out-of-flow table's `width`, or the size its insets give, doesn't make
/// it smaller than its content, nor does a flex container.
#[test]
fn heights_and_sizes_like_chrome() {
    let laid = lay_out(&format!(
        "{AHEM}<table style='height: 100px'><tbody id=pct style='height: 40%'><tr><td>X</td></tr>
           </tbody><tbody id=rest><tr><td>X</td></tr></tbody></table>
         <table style='height: 100px'><tbody id=over style='height: 200%'><tr><td>X</td></tr>
           </tbody></table>
         <table><tr id=row><td style='vertical-align: baseline; height: 100px'>X</td>
           <td style='vertical-align: baseline'><div style='height: 50px'></div></td></tr></table>
         <div style='position: relative'><table id=abs style='position: absolute; width: 50px'>
           <tr><td><div style='width: 200px'></div></td></tr></table></div>
         <table style='height: 200px'><tbody id=fixed style='height: 50px'><tr><td>X</td></tr>
           </tbody><tbody id=pc style='height: 10%'><tr><td>X</td></tr></tbody></table>
         <table><tr id=r1><td rowspan=2 style='height: 100px'></td></tr><tr id=r2></tr></table>
         <table><tr><td id=d style='height: 50px'><div id=dd style='height: 200%'>X</div></td></tr>
           </table>
         <table><tr><td id=d2 style='height: 50px'><span id=ib style='display: inline-block;
           width: 10px; height: 200%'></span></td></tr></table>
         <div style='display: flex; width: 30px'><table id=shrunk style='min-width: 0'>
           <tr><td>XX XX</td></tr></table></div>
         <div style='display: flex; flex-direction: column; height: 10px'><table id=squashed
           style='min-height: 0'><tr><td>XX</td></tr></table></div>
         <div style='position: relative; width: 30px; height: 10px'><table id=insets
           style='position: absolute; left: 0; right: 0; top: 0; bottom: 0'><tr><td>XX XX</td></tr>
           </table></div>"
    ));
    assert_eq!(laid.rect("pct").height, 40.0);
    assert_eq!(laid.rect("rest").height, 60.0);
    assert_eq!(laid.rect("over").height, 100.0);
    // The first cell's 100px, not the block's 50px above the baseline and
    // the first cell's 84px below it.
    assert_eq!(laid.rect("row").height, 100.0);
    assert_eq!(laid.rect("abs").width, 200.0);
    // 130px over the rows, to the one with a fixed height.
    assert_eq!(laid.rect("fixed").height, 180.0);
    assert_eq!(laid.rect("pc").height, 20.0);
    assert_eq!(laid.rect("r1").height, 0.0);
    assert_eq!(laid.rect("r2").height, 100.0);
    // The row as tall as the cell's 50px, the block 200% of it; an
    // inline-block too.
    assert_eq!(laid.rect("d").height, 50.0);
    assert_eq!(laid.rect("dd").height, 100.0);
    assert_eq!(laid.rect("d2").height, 50.0);
    assert_eq!(laid.rect("ib").height, 100.0);
    // A flex item no smaller than its content, whatever its minimum.
    assert_eq!(laid.rect("shrunk").width, 40.0);
    assert_eq!(laid.rect("squashed").height, 20.0);
    // "XX XX" on two lines in its 40px.
    assert_eq!(laid.rect("insets").size(), SizeF::new(40.0, 40.0));
}

/// Columns and column groups: a column's fixed `width` is its maximum,
/// which its cells' content can widen; `span` makes as many columns; a
/// group's fixed `width` is its columns' without one, and its own `span`
/// columns' without columns; a percentage and a `min-width` count too; a
/// column with a width takes room with no
/// cell in it, one without (or 0 wide) is merged away. They have no
/// fragments, and no boxes out of a table.
#[test]
fn columns_give_widths() {
    let laid = lay_out(&format!(
        "{AHEM}<table id=t1><col id=c50 style='width: 50px'><col span=2 style='width: 30px'>
           <tr><td id=a1></td><td id=a2>XX</td><td id=a3></td></tr></table>
         <table id=t2><colgroup style='width: 40px'><col><col style='width: 10px'></colgroup>
           <colgroup span=2 style='width: 25px'></colgroup>
           <tr><td id=b1></td><td id=b2></td><td id=b3></td><td id=b4></td></tr></table>
         <table id=t3><col span=3 style='width: 20px'><tr><td id=c1></td></tr></table>
         <table id=t4><col span=3><col span=2 style='width: 0'><tr><td id=d1>X</td></tr></table>
         <table id=t5><col style='width: 50%'><tr><td>X</td><td>XXX</td></tr></table>
         <table><col style='min-width: 100px'><tr><td id=e1>X</td></tr></table>
         <div id=out><col id=lone style='width: 100px'></div>"
    ));
    let widths = |ids: &[&str]| -> Vec<f32> { ids.iter().map(|id| laid.rect(id).width).collect() };
    assert_eq!(widths(&["a1", "a2", "a3", "t1"]), [50.0, 40.0, 30.0, 120.0]);
    assert_eq!(
        widths(&["b1", "b2", "b3", "b4", "t2"]),
        [40.0, 10.0, 25.0, 25.0, 100.0]
    );
    assert_eq!(widths(&["c1", "t3"]), [20.0, 60.0]);
    assert_eq!(widths(&["d1", "t4"]), [20.0, 20.0]);
    // As wide as 60px are the other 50%.
    assert_eq!(laid.rect("t5").width, 120.0);
    assert_eq!(laid.rect("e1").width, 100.0);
    assert!(!laid.rects.contains_key("c50") && !laid.rects.contains_key("lone"));
    assert_eq!(laid.rect("out").height, 0.0);
}

/// A fixed table takes its columns' widths before its first row's cells'
/// (which fill the columns without one); a spanning cell's percentage goes
/// to the `auto` columns it spans. In an auto table, a spanning cell
/// without a fixed width doesn't widen fixed columns.
#[test]
fn columns_in_fixed_tables_and_spans() {
    let laid = lay_out(&format!(
        "{AHEM}<table style='table-layout: fixed; width: 200px'><col style='width: 50px'>
           <col style='width: 25%'><col><tr><td id=f1 style='width: 100px'></td><td id=f2></td>
           <td id=f3 style='width: 30px'></td><td id=f4></td></tr></table>
         <table style='table-layout: fixed; width: 400px'><col style='width: 25%'>
           <col style='width: 25px'><col style='width: 25px'><col>
           <tr><td colspan=4 style='width: 50%'></td><td></td></tr>
           <tr><td></td><td></td><td></td><td id=g4></td><td></td></tr></table>
         <table id=u><col style='width: 50px'><col style='width: 50px'>
           <tr><td id=u1></td><td></td></tr><tr><td colspan=2>XXXXX XXXXX</td></tr></table>"
    ));
    let widths = |ids: &[&str]| -> Vec<f32> { ids.iter().map(|id| laid.rect(id).width).collect() };
    assert_eq!(widths(&["f1", "f2", "f3", "f4"]), [50.0, 50.0, 30.0, 70.0]);
    // 50% over the four columns: 12.5% of 400px for the `auto` one.
    assert_eq!(laid.rect("g4").width, 50.0);
    assert_eq!(widths(&["u1", "u"]), [50.0, 100.0]);
}

/// `visibility: collapse`: a column is sized, the rows with it, then it
/// takes no room (the table narrower by it, a cell spanning it narrower,
/// one only in it hidden); a row, or a row group's rows, 0 tall with no
/// border spacing, and hidden, its cells 0 tall too, a cell spanning into
/// it shorter by it, a table with a `height` shorter by it.
#[test]
fn collapsed_columns_and_rows() {
    let laid = lay_out(&format!(
        "{AHEM}<table id=vc><col><col style='visibility: collapse'><col>
           <tr><td id=v1>X</td><td id=v2>XX</td><td id=v3>X</td></tr>
           <tr><td colspan=3 id=v4>X</td></tr></table>
         <table id=vr style='border-spacing: 10px'><tr id=r1><td>X</td><td id=rs rowspan=2>X</td></tr>
           <tr id=r2 style='visibility: collapse'><td id=rc>X</td><td id=rc2 rowspan=2>X</td></tr>
           <tr id=r3><td>X</td></tr></table>
         <table id=vg style='border-spacing: 10px'><tbody style='visibility: collapse'>
           <tr style='visibility: visible'><td>X</td></tr></tbody><tbody><tr id=g1><td>X</td></tr></tbody></table>
         <table><col style='visibility: collapse'><col><tr><td>X X X</td><td id=beside>X</td></tr>
           </table>
         <table><col><col style='visibility: collapse'><tr><td>XXXX</td><td>XXXX</td></tr>
           <tr><td id=wrapped colspan=2>XX XX<div id=half style='width: 50%'></div></td></tr></table>
         <table><col><col><col style='visibility: collapse'>
           <tr><td style='height: 60px'>X</td><td id=middle colspan=2>XX XX</td></tr>
           <tr><td>X</td><td>XX</td><td>XXX</td></tr></table>
         <table id=vh style='height: 150px'><tr><td>X</td></tr>
           <tr style='visibility: collapse'><td>X</td></tr><tr><td>X</td></tr></table>"
    ));
    let vc = laid.rect("vc");
    assert_eq!(vc.width, 40.0);
    assert_eq!(laid.rect("v2").width, 0.0);
    assert_eq!(laid.rect("v3").x, vc.x + 20.0);
    assert_eq!(laid.rect("v4").width, 40.0);
    // The rows sized with the collapsed column at its width: its text on
    // one line.
    assert_eq!(laid.rect("beside").height, 20.0);
    // Laid out again at its 80px: half of it is 40px.
    assert_eq!(laid.rect("half").width, 40.0);
    // On two lines at its 40px, in the middle of its 60px row by them.
    assert_eq!(first_line_y(&laid, "middle"), 10.0);
    assert!(laid.fragments["v2"].hidden);
    assert!(!laid.fragments["v1"].hidden && !laid.fragments["v4"].hidden);
    let (vr, r1, r3) = (laid.rect("vr"), laid.rect("r1"), laid.rect("r3"));
    assert_eq!(vr.height, 70.0);
    assert_eq!(laid.rect("r2").height, 0.0);
    assert!(laid.fragments["r2"].hidden && !laid.fragments["r1"].hidden);
    // Its cell 0 tall; one spanning into it as tall as the rest.
    assert_eq!(laid.rect("rc").height, 0.0);
    assert_eq!(laid.rect("rc2").height, 0.0);
    assert_eq!(laid.rect("rs").height, 20.0);
    assert_eq!(r3.y, r1.y + 30.0);
    let vg = laid.rect("vg");
    assert_eq!(vg.height, 40.0);
    assert_eq!(laid.rect("g1").y, vg.y + 10.0);
    // Its `height` spread over the three rows, then the collapsed one's
    // third taken off.
    assert_eq!(laid.rect("vh").height, 100.0);
}

/// Collapsed borders: each edge takes the widest border of the boxes it is
/// on (cell, row, row group, column, table), `hidden` over all; a cell's
/// border is the half of its edges, the table's the half of its outermost;
/// the table has no padding nor border spacing then, and none of its own
/// without rows.
#[test]
fn collapsed_borders() {
    let laid = lay_out(&format!(
        "{AHEM}<style>.c {{ border-collapse: collapse }}</style>
         <table id=t1 class=c style='border: 6px solid; padding: 10px; border-spacing: 10px'>
           <tr><td id=a style='border: 2px solid'>X</td><td id=b style='border: 10px solid'>X</td>
           </tr></table>
         <table id=t2 class=c style='border: 10px hidden'><tr><td id=h style='border: 4px solid'>X</td>
           </tr></table>
         <table class=c><col style='border-left: 12px solid'>
           <tr style='border-top: 8px solid'><td id=r style='border: 2px solid'>X</td></tr></table>
         <table id=e class=c style='border: 4px solid; width: 50px; height: 20px'></table>
         <div id=float style='float: left'><table class=c style='width: 30px; padding: 40px'>
           </table></div>
         <table class=c style='border: 10px solid'><tr><td id=hc style='border: 2px hidden'>X</td>
           <td id=next>X</td></tr></table>"
    ));
    let sides = |id: &str| {
        let b = laid.fragments[id].border;
        [b.top, b.right, b.bottom, b.left]
    };
    // Its left edge the table's 6px, its right one the other cell's 10px.
    assert_eq!(sides("a"), [3.0, 5.0, 3.0, 3.0]);
    assert_eq!(sides("b"), [5.0; 4]);
    assert_eq!(sides("t1"), [5.0, 5.0, 5.0, 3.0]);
    assert_eq!(laid.fragments["t1"].padding.left, 0.0);
    let (t1, a, b) = (laid.rect("t1"), laid.rect("a"), laid.rect("b"));
    assert_eq!(t1.size(), SizeF::new(66.0, 40.0));
    assert_eq!(
        (a.x, a.width, b.x, b.width),
        (t1.x + 3.0, 28.0, t1.x + 31.0, 30.0)
    );
    assert_eq!(sides("h"), [0.0; 4]);
    assert_eq!(sides("t2"), [0.0; 4]);
    // The row's 8px on top, the column's 12px on the left.
    assert_eq!(sides("r"), [4.0, 1.0, 1.0, 6.0]);
    assert_eq!(sides("e"), [0.0; 4]);
    assert_eq!(laid.rect("e").size(), SizeF::new(50.0, 20.0));
    // Its padding none to its container either.
    assert_eq!(laid.rect("float").width, 30.0);
    // A cell's `hidden` keeps the table's 10px off its edges.
    assert_eq!(sides("hc"), [0.0; 4]);
    assert_eq!(sides("next"), [5.0, 5.0, 5.0, 0.0]);
}
