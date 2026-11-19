//! `direction: rtl` in layout: a paragraph's direction is its block's
//! (not its first strong character's), lines start on the right; flex
//! items and table columns go from the right; out-of-flow boxes' static
//! positions are on the right.

use crate::page::lay_out;

const AHEM: &str = "<style>body { margin: 0; font: 20px/1 Ahem } table { border-spacing: 0 }
    td { padding: 0 }</style>";

/// An `rtl` block's lines start on the right; an `ltr` one's on the
/// left, whatever its text's first strong character.
#[test]
fn a_paragraphs_direction_is_its_blocks() {
    let laid = lay_out(&format!(
        "{AHEM}<div id=rtl style='width: 200px; direction: rtl'>XX</div>
         <div id=ltr style='width: 200px'>&#x5d0;&#x5d0;</div>"
    ));
    assert_eq!(laid.lines("rtl")[0].x, 160.0);
    assert_eq!(laid.lines("ltr")[0].x, 0.0);
}

/// Flex items and table columns go from the right.
#[test]
fn items_and_columns_go_from_the_right() {
    let laid = lay_out(&format!(
        "{AHEM}<div style='display: flex; width: 200px; direction: rtl'>
           <div id=f1 style='width: 50px; height: 10px'></div>
           <div id=f2 style='width: 50px; height: 10px'></div></div>
         <table style='direction: rtl'><tr><td id=c1>X</td><td id=c2>XX</td></tr></table>"
    ));
    assert_eq!(laid.rect("f1").x, 150.0);
    assert_eq!(laid.rect("f2").x, 100.0);
    assert_eq!(laid.rect("c1").x, 40.0);
    assert_eq!(laid.rect("c2").x, 0.0);
}

/// An out-of-flow box's static position is at its container's
/// inline-start, the right: one block-level in inline content, below its
/// line; a table's own.
#[test]
fn static_positions_are_on_the_right() {
    let laid = lay_out(&format!(
        "{AHEM}<div style='position: relative; width: 200px; direction: rtl'>XX<div id=inline
           style='position: absolute; width: 30px; height: 10px'></div></div>
         <div style='position: relative'><div style='display: table; direction: rtl;
           width: 100px'><div id=table style='position: absolute; width: 10px;
           height: 10px'></div><div style='display: table-cell'>X</div></div></div>"
    ));
    assert_eq!(laid.rect("inline").x, 170.0);
    assert_eq!(laid.rect("table").x, 90.0);
}
