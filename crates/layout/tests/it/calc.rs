//! `calc()` of a length and a percentage in layout: each resolved against
//! the size its percentage is of, in block, flex and grid layout, and
//! through taffy's handles.

use std::collections::BTreeMap;

use gfx::RectF;

use crate::page::r;

/// Border boxes in LayoutView coordinates, by `id` attribute.
fn lay_out(html: &str) -> BTreeMap<String, RectF> {
    crate::page::lay_out(&format!("<style>html, body {{ margin: 0 }}</style>{html}")).rects
}

#[test]
fn in_block_layout() {
    let l = lay_out(
        "<div id=outer style='width: 400px; height: 200px'>
           <div id=a style='width: calc(50% - 20px); height: calc(10% + 5px);
                             margin-left: calc(10% + 2px); padding-right: calc(5% + 1px)'></div>
           <div id=b style='width: calc(100% + 10px); height: 1px'></div>
           <div id=c style='width: min(100px, 50px); height: clamp(1px, 3px, 2px)'></div>
         </div>",
    );
    // 50% of 400 less 20, and the padding, 5% of 400 and 1; 10% of 200
    // and 5; 10% of 400 and 2.
    assert_eq!(l["a"], r(42.0, 0.0, 201.0, 25.0));
    assert_eq!(l["b"].width, 410.0);
    assert_eq!(l["c"].width, 50.0);
    assert_eq!(l["c"].height, 2.0);
}

#[test]
fn a_negative_result_is_clamped_where_none_is_allowed() {
    let l = lay_out(
        "<div style='width: 100px'>
           <div id=a style='width: calc(10% - 50px); height: 1px'></div>
           <div id=b style='width: 50px; padding-left: calc(10% - 50px); height: 1px'></div>
         </div>",
    );
    assert_eq!(l["a"].width, 0.0);
    assert_eq!(l["b"].width, 50.0);
}

#[test]
fn in_flex_and_grid_layout() {
    let l = lay_out(
        "<div style='display: flex; width: 300px'>
           <div id=item style='flex-basis: calc(50% + 10px); height: 1px'></div>
         </div>
         <div style='display: grid; width: 300px;
                     grid-template-columns: calc(25% + 5px) minmax(calc(10% + 1px), 1fr)'>
           <div id=first style='height: 1px'></div>
           <div id=second style='height: 1px'></div>
         </div>",
    );
    assert_eq!(l["item"].width, 160.0);
    assert_eq!(l["first"].width, 80.0);
    assert_eq!(l["second"].width, 220.0);
}
