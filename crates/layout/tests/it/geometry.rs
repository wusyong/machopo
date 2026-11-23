//! Geometry found from the element (`BoxTree::geometry`): each check of a
//! `Page` holds every query of every node against the walk from the root,
//! so these are pages with what the way down to a box goes through, and
//! changes that move a box, give it another containing block or reuse its
//! room.

use dom::{Cx, NodeRef};
use gfx::PointF;
use layout::{Geometry, ScrollOffsets, StickyOffsets};

use crate::page::{Page, find, html};

/// `query` of the geometry of `page`'s last check, unscrolled.
fn with_geometry<R>(page: &Page, query: impl FnOnce(&Geometry<'_>) -> R) -> R {
    let view = page.view();
    let (scroll, sticky) = (ScrollOffsets::default(), StickyOffsets::default());
    query(
        &page
            .tree
            .geometry(&view, PointF::new(0.0, 0.0), &scroll, &sticky),
    )
}

/// The ids of `page`'s elements each have a box the geometry finds.
fn assert_found(page: &Page, ids: &[&str]) {
    with_geometry(page, |geometry| {
        for id in ids {
            assert!(
                !geometry.client_rects(page.id(id)).is_empty(),
                "no rects of #{id}"
            );
        }
    });
}

/// A `local` element of `style`.
fn element<'gc>(
    cx: &Cx<'_, 'gc>,
    document: NodeRef<'gc>,
    local: &str,
    style: &str,
) -> NodeRef<'gc> {
    let element = cx.create_element(document, html(local));
    cx.set_attribute(element, "style", style);
    element
}

/// Through what each step down applies (a transform, how far a sticky box
/// sticks, a scroll container's offset, nested), and to the boxes that
/// aren't their parents' children in the fragments: out-of-flow boxes,
/// held by their containing blocks, and outside markers, added last.
#[test]
fn boxes_are_found_through_what_moves_them() {
    let page = Page::new(
        "<style>.s { overflow: scroll; height: 60px } p { margin: 0 }</style>\
         <div id=t style='transform: translate(5px, 7px) rotate(10deg)'>\
           <div class=s id=s1><div class=s id=s2><p id=deep>deep</p></div></div>\
           <div style='position: sticky; top: 4px' id=sticky><b id=b>stuck</b></div>\
         </div>\
         <div style='position: relative' id=cb>\
           <div><div><i id=abs style='position: absolute; top: 3px'>abs</i></div></div>\
           <span><em id=fixed style='position: fixed; left: 2px'>fixed</em></span>\
         </div>\
         <ul><li id=li>item</li></ul>\
         <ol style='list-style-position: inside'><li id=inside>inside</li></ol>",
    );
    assert_found(
        &page,
        &[
            "t", "s1", "s2", "deep", "sticky", "b", "cb", "abs", "fixed", "li", "inside",
        ],
    );
}

/// Where the fragments' order isn't the tree's: a flex container's `order`,
/// floats after the atomic inlines of the lines beside them; an inline
/// element split by blocks and over several lines (its boxes in several
/// blocks' lines); a table's caption, and its columns, which have no
/// fragments.
#[test]
fn boxes_are_found_where_the_fragments_order_them() {
    let page = Page::new(
        "<div style='display: flex'><p id=second style='order: 2'>b</p>\
           <p id=first style='order: 1'>a</p></div>\
         <div style='width: 120px'>x <span id=atomic style='display: inline-block'>atomic</span>\
           <i id=float style='float: left'>float</i> y</div>\
         <div style='width: 60px'><span id=split>a long line <div id=block>block</div>\
           after the block, over lines</span></div>\
         <table><caption id=caption>caption</caption><col id=col>\
           <tr><td id=cell>cell</td></tr></table>",
    );
    assert_found(
        &page,
        &[
            "second", "first", "atomic", "float", "split", "block", "caption", "cell",
        ],
    );
    with_geometry(&page, |geometry| {
        let lines = geometry.client_rects(page.id("split")).len();
        assert!(lines > 2, "split, over lines");
        assert_eq!(geometry.element_box(page.id("col")), None, "no fragment");
    });
}

/// As the DOM changes: an out-of-flow box whose containing block becomes
/// another, an element moved, a block splitting an inline element, an
/// order changed, and boxes taken out and their room reused by new ones,
/// whose old links mustn't find them.
#[test]
fn boxes_are_found_after_changes() {
    let mut page = Page::new(
        "<div id=outer><div id=inner><b id=abs style='position: absolute'>abs</b></div></div>\
         <div id=flex style='display: flex'><p id=one>1</p><p id=two>2</p></div>\
         <p><span id=span>some text</span></p>\
         <div id=gone><p>a</p><p>b</p><p>c</p></div><div id=to></div>",
    );
    page.change(|cx, document| {
        cx.set_attribute(find(cx, document, "inner"), "style", "position: relative");
    });
    page.change(|cx, document| {
        cx.set_attribute(find(cx, document, "inner"), "style", "");
        cx.set_attribute(find(cx, document, "outer"), "style", "transform: scale(2)");
    });
    page.change(|cx, document| {
        cx.set_attribute(find(cx, document, "abs"), "style", "position: fixed");
    });
    page.change(|cx, document| {
        let span = find(cx, document, "span");
        let block = element(cx, document, "div", "");
        cx.append_child(span, block).unwrap();
        cx.set_attribute(find(cx, document, "two"), "style", "order: -1");
    });
    page.change(|cx, document| {
        let (abs, to) = (find(cx, document, "abs"), find(cx, document, "to"));
        cx.append_child(to, abs).unwrap();
    });
    let room = page.tree.capacity_for_testing();
    page.change(|cx, document| {
        cx.remove(find(cx, document, "gone"));
    });
    page.change(|cx, document| {
        let to = find(cx, document, "to");
        for _ in 0..3 {
            let new = element(cx, document, "p", "position: relative");
            cx.append_child(to, new).unwrap();
        }
    });
    assert_eq!(page.tree.capacity_for_testing(), room, "the room reused");
    assert_found(
        &page,
        &["outer", "inner", "abs", "one", "two", "span", "to"],
    );
}

/// A query goes into the fragments on the way to the box, not into its
/// siblings: ten times as many leaves it the same.
#[test]
fn a_query_goes_down_only_to_the_box() {
    let entered = |rows: usize| {
        let mut html = String::from("<div><section>");
        for i in 0..rows {
            if i == rows / 2 {
                html.push_str("<p id=middle>middle <span id=span>span</span></p>");
            } else {
                html.push_str("<p>row</p>");
            }
        }
        html.push_str("</section></div>");
        let page = Page::new(&html);
        with_geometry(&page, |geometry| {
            for id in ["middle", "span"] {
                let node = page.id(id);
                assert!(geometry.element_box(node).is_some());
                assert!(!geometry.client_rects(node).is_empty());
                assert!(!geometry.border_quads(node).is_empty());
                assert!(geometry.box_quads(node).is_some());
            }
            assert!(geometry.scroll_size(page.id("middle")).is_some());
            geometry.entered_for_testing()
        })
    };
    let few = entered(20);
    assert!(few > 0);
    assert_eq!(entered(200), few);
}
