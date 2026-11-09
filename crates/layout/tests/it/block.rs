//! Block layout on parsed, styled documents: where each box ends up.

use std::sync::Arc;

use computed_style::Overflow;
use dom::{Dom, NodeKind};
use gfx::SizeF;
use layout::{BoxFragment, BoxTree, TextContext};

use crate::page::{Laid, Page, VIEWPORT, each_node, lay_out, parse, r, style};

/// `html` and `body` without the margins the UA stylesheet gives them.
const RESET: &str = "<style>html, body { margin: 0 }</style>";

#[test]
fn auto_width_fills_and_auto_height_sums() {
    let l = lay_out(&format!(
        "{RESET}<div id=outer style='margin: 0 30px 0 20px'>
           <div id=a style='height: 30px'></div>
           <div id=b style='height: 70px; margin-left: 10px'></div>
         </div>"
    ));
    assert_eq!(l.rect("outer"), r(20.0, 0.0, 750.0, 100.0));
    assert_eq!(l.rect("a"), r(20.0, 0.0, 750.0, 30.0));
    assert_eq!(l.rect("b"), r(30.0, 30.0, 740.0, 70.0));
}

#[test]
fn padding_percentages_and_min_max() {
    let l = lay_out(&format!(
        "{RESET}<div id=outer style='width: 400px; padding: 10px 20px'>
           <div id=half style='width: 50%; height: 1px'></div>
           <div id=min style='width: 10%; min-width: 100px; height: 1px'></div>
           <div id=max style='width: 90%; max-width: 150px; height: 1px'></div>
           <div id=border-box style='width: 100px; padding: 5px; box-sizing: border-box'></div>
           <div id=content-box style='width: 100px; padding: 5px'></div>
         </div>"
    ));
    // Padding adds to the content box; percentages resolve against it.
    assert_eq!(l.rect("outer").width, 440.0);
    assert_eq!(l.rect("half"), r(20.0, 10.0, 200.0, 1.0));
    assert_eq!(l.rect("min").width, 100.0);
    assert_eq!(l.rect("max").width, 150.0);
    assert_eq!(l.rect("border-box").width, 100.0);
    assert_eq!(l.rect("content-box").width, 110.0);
}

/// Percentages of the top and bottom padding and border are of the
/// containing block's width too, not its height (CSS 2 § 8.4).
#[test]
fn vertical_padding_percentages_are_of_the_width() {
    let l = lay_out(&format!(
        "{RESET}<div style='width: 200px; height: 50px'>
           <div id=padded style='padding: 10% 0 5%'></div>
         </div>"
    ));
    assert_eq!(l.rect("padded"), r(0.0, 0.0, 200.0, 30.0));
}

#[test]
fn margins_collapse() {
    let l = lay_out(&format!(
        "{RESET}<div id=a style='height: 10px; margin-bottom: 20px'></div>
         <div id=b style='height: 10px; margin-top: 30px'></div>
         <div id=parent style='margin-top: 5px'>
           <div id=child style='height: 10px; margin-top: 40px'></div>
         </div>
         <div id=padded style='padding-top: 1px; margin-top: 5px'>
           <div id=inner style='height: 10px; margin-top: 40px'></div>
         </div>"
    ));
    assert_eq!(l.rect("a").y, 0.0);
    // Adjacent siblings: the larger margin wins.
    assert_eq!(l.rect("b").y, 40.0);
    // Parent and first child: 5px and 40px collapse to 40px, above the
    // parent.
    assert_eq!(l.rect("parent").y, 90.0);
    assert_eq!(l.rect("child").y, 90.0);
    // Padding separates them.
    assert_eq!(l.rect("padded").y, 105.0);
    assert_eq!(l.rect("inner").y, 146.0);
}

#[test]
fn root_margins_do_not_collapse_with_the_viewport() {
    // Nor with its children's (CSS 2.2 §8.3.1): `<body>`'s margin stays
    // inside the root's box, as in Chrome. The empty body's own top and
    // bottom margins collapse through it into one.
    let l = lay_out("<html id=html style='margin: 0'><body id=body style='margin: 25px'>");
    assert_eq!(l.rect("html"), r(0.0, 0.0, 800.0, 25.0));
    assert_eq!(l.rect("body"), r(25.0, 25.0, 750.0, 0.0));
}

#[test]
fn what_generates_no_box() {
    let l = lay_out(&format!(
        "{RESET}<div id=a style='height: 10px'></div>
         <div id=hidden style='display: none; height: 100px'><div id=in-hidden></div></div>
         <span id=inline style='font: 20px/1 Ahem'>X<div id=in-inline style='height: 50px'></div></span>
         <div id=b style='height: 10px'></div>"
    ));
    assert!(!l.rects.contains_key("hidden"));
    assert!(!l.rects.contains_key("in-hidden"));
    // An inline element has no box of its own; the block inside it splits
    // it, and its text goes into an anonymous block before the block.
    assert!(!l.rects.contains_key("inline"));
    assert_eq!(l.rect("in-inline"), r(0.0, 30.0, 800.0, 50.0));
    assert_eq!(l.rect("b").y, 80.0);
}

#[test]
fn document_size_covers_overflow() {
    let l = lay_out(&format!(
        "{RESET}<div style='height: 1000px'></div><div style='width: 1200px; height: 10px'></div>"
    ));
    assert_eq!(l.view.size, VIEWPORT);
    assert_eq!(layout::document_size(&l.view), SizeF::new(1200.0, 1010.0));
    let small = lay_out(&format!("{RESET}<div style='height: 10px'></div>"));
    assert_eq!(layout::document_size(&small.view), VIEWPORT);
}

#[test]
fn borders_take_space() {
    let l = lay_out(&format!(
        "{RESET}<div id=outer style='width: 100px; border: solid; border-width: 1px 2px 3px 4px'>
           <div id=inner style='height: 10px'></div>
         </div>
         <div id=border-box style='width: 100px; height: 50px; border: 10px solid; box-sizing: border-box'></div>
         <div id=no-style style='width: 100px; height: 5px; border-width: 30px'></div>"
    ));
    assert_eq!(l.rect("outer"), r(0.0, 0.0, 106.0, 14.0));
    assert_eq!(l.rect("inner"), r(4.0, 1.0, 100.0, 10.0));
    assert_eq!(l.rect("border-box"), r(0.0, 14.0, 100.0, 50.0));
    // No style, no width.
    assert_eq!(l.rect("no-style"), r(0.0, 64.0, 100.0, 5.0));
}

#[test]
fn overflow_hidden_is_a_block_formatting_context() {
    let l = lay_out(&format!(
        "{RESET}<div id=clip style='overflow: hidden; height: 50px'>
           <div id=child style='margin-top: 20px; height: 500px'></div>
         </div>"
    ));
    // The child's margin stays inside instead of collapsing through.
    assert_eq!(l.rect("clip").y, 0.0);
    assert_eq!(l.rect("child").y, 20.0);
    // And its overflow doesn't grow the document.
    assert_eq!(layout::document_size(&l.view), VIEWPORT);
}

/// `overflow: clip` clips without being a scroll container: no block
/// formatting context of its own, so its child's margin collapses through
/// it (css-overflow 3), but its overflow doesn't grow the document either.
#[test]
fn overflow_clip_is_not_a_block_formatting_context() {
    let l = lay_out(&format!(
        "{RESET}<div id=clip style='overflow: clip; height: 50px'>
           <div id=child style='margin-top: 20px; height: 500px'></div>
         </div>"
    ));
    assert_eq!(l.rect("clip").y, 20.0);
    assert_eq!(l.rect("child").y, 20.0);
    assert_eq!(layout::document_size(&l.view), VIEWPORT);
}

#[test]
fn scrollable_overflow_of_a_scroll_container() {
    // A 100x50 box with 10px padding and a 2px border holds a 300px-high
    // child: from the padding box's top, 10px of padding, the child to
    // 310px, then 10px of end padding.
    let laid = lay_out(
        "<style>html, body { margin: 0 }</style>
         <div style='width: 100px; height: 50px; padding: 10px; border: 2px solid; overflow: auto'>
           <div style='height: 300px'></div>
         </div>",
    );
    let scroller = &laid.view.children[0].fragment.children[0].fragment.children[0].fragment;
    assert_eq!(scroller.size, SizeF::new(124.0, 74.0));
    assert_eq!(
        layout::scrollable_overflow(scroller),
        SizeF::new(120.0, 320.0)
    );
    // Content that fits: just the padding box.
    let laid = lay_out(
        "<style>html, body { margin: 0 }</style>
         <div style='width: 100px; height: 50px; overflow: auto'><div style='height: 10px'></div></div>",
    );
    let scroller = &laid.view.children[0].fragment.children[0].fragment.children[0].fragment;
    assert_eq!(
        layout::scrollable_overflow(scroller),
        SizeF::new(100.0, 50.0)
    );
}

/// The end padding goes around the children's margin boxes (Blink's
/// in-flow bounds), not their border boxes: a negative margin takes it in,
/// a positive one adds to it; the border boxes count as they are. A last
/// child's margin that collapsed through its parent is outside it.
#[test]
fn scrollable_overflow_pads_the_margin_boxes() {
    let first = |laid: &Laid| -> Arc<BoxFragment> {
        laid.view.children[0].fragment.children[0].fragment.children[0]
            .fragment
            .clone()
    };
    // 10px of padding around four 20px flex items (whose margins don't
    // collapse), each pulled in by 5px at both ends (40px of content): the last box's border box ends at 55,
    // its margin box at 50, padded to 60; sideways the boxes stick out
    // 7px past the 20px content, padded inside the 60px padding box.
    let negative = "<style>html, body { margin: 0 } #t > div { margin: -5px -7px }
         #t > div > div { width: 20px; height: 20px }</style>
         <div id=t style='display: flex; flex-direction: column; width: 20px; padding: 10px 20px; OVERFLOW'>
           <div><div></div></div><div><div></div></div><div><div></div></div><div><div></div></div>
         </div>";
    for overflow in ["", "overflow: hidden"] {
        let box_ = first(&lay_out(&negative.replace("OVERFLOW", overflow)));
        assert_eq!(box_.size, SizeF::new(60.0, 60.0), "{overflow}");
        assert_eq!(
            layout::scrollable_overflow(&box_),
            SizeF::new(60.0, 60.0),
            "{overflow}"
        );
    }
    // A 30px bottom margin inside a scroll container with 10px of padding:
    // 10 + 100 + 30 + 10.
    let box_ = first(&lay_out(
        "<style>html, body { margin: 0 }</style>
         <div style='height: 50px; padding: 10px; overflow: auto'>
           <div style='height: 100px; margin-bottom: 30px'></div>
         </div>",
    ));
    assert_eq!(layout::scrollable_overflow(&box_).height, 150.0);
    // The child's 20px bottom margin collapses through a block of auto
    // height: the block ends with the child, and so does its overflow.
    // With a bottom padding it stays inside, padded. Past a fixed height,
    // it overflows.
    let collapsing = "<style>html, body { margin: 0 }</style>
         <div style='PADDING'><div style='height: 100px; margin-bottom: 20px'></div></div>";
    let box_ = first(&lay_out(&collapsing.replace("PADDING", "")));
    assert_eq!(box_.size.height, 100.0);
    assert_eq!(layout::scrollable_overflow(&box_).height, 100.0);
    let box_ = first(&lay_out(
        &collapsing.replace("PADDING", "padding-bottom: 5px"),
    ));
    assert_eq!(box_.size.height, 125.0);
    assert_eq!(layout::scrollable_overflow(&box_).height, 125.0);
    // Lines of text are in-flow content too: three 20px lines in 20px.
    let box_ = first(&lay_out(
        "<style>html, body { margin: 0 }</style>
         <div style='width: 40px; height: 20px; padding: 0 5px 5px 0; overflow: auto;
           font: 20px/1 Ahem'>XX XX XX</div>",
    ));
    assert_eq!(layout::scrollable_overflow(&box_), SizeF::new(45.0, 65.0));
    // A fixed height the child overflows: its margin is in the overflow.
    let box_ = first(&lay_out(&collapsing.replace("PADDING", "height: 50px")));
    assert_eq!(layout::scrollable_overflow(&box_).height, 120.0);
}

/// An absolutely positioned box reaches as far as its border box (and
/// its own margin and the end padding don't count): it isn't in-flow
/// content, which the end padding goes around. An in-flow box after it is
/// padded as before; so is a float, which is in-flow content.
#[test]
fn scrollable_overflow_doesnt_pad_an_absolute_box() {
    let first = |laid: &Laid| -> Arc<BoxFragment> {
        laid.view.children[0].fragment.children[0].fragment.children[0]
            .fragment
            .clone()
    };
    let page = "<style>html, body { margin: 0 }</style>
         <div style='position: relative; width: 100px; height: 50px; padding: 10px; overflow: auto'>
           <div style='height: 20px'></div>
           <div style='position: absolute; top: 300px; left: 150px; width: 10px; height: 10px; margin: 0 7px 7px 0'></div>
           CHILD
         </div>";
    // The absolute box's border box ends at 310 down and 160 across.
    let box_ = first(&lay_out(&page.replace("CHILD", "")));
    assert_eq!(layout::scrollable_overflow(&box_), SizeF::new(160.0, 310.0));
    // An in-flow box further down: 10 + 20 + 400, padded.
    let box_ = first(&lay_out(
        &page.replace("CHILD", "<div style='height: 400px'></div>"),
    ));
    assert_eq!(layout::scrollable_overflow(&box_).height, 440.0);
    // A float further down too.
    let box_ = first(&lay_out(&page.replace(
        "CHILD",
        "<div style='float: left; width: 10px; height: 400px'></div>",
    )));
    assert_eq!(layout::scrollable_overflow(&box_).height, 440.0);
}

/// A `flow-root` establishes a block formatting context: it holds its
/// floats, and its children's margins don't collapse through it.
#[test]
fn a_flow_root_establishes_a_block_formatting_context() {
    let laid = lay_out(&format!(
        "{RESET}<div id=root style='display: flow-root'><div style='float: left; width: 10px; height: 50px'></div></div>
         <div id=margins style='display: flow-root'><div style='margin-top: 20px; height: 10px'></div></div>"
    ));
    assert_eq!(laid.rect("root").height, 50.0);
    assert_eq!(laid.rect("margins"), r(0.0, 50.0, 800.0, 30.0));
}

#[test]
fn intrinsic_size_keywords_size_a_block_to_its_content() {
    // Ahem: each glyph a 20px square.
    let laid = lay_out(
        "<style>body { margin: 0; font: 20px/1 Ahem }</style>
         <div id=max style='width: max-content'>XX XXX</div>
         <div id=min style='width: min-content'>XX XXX</div>
         <div id=fit style='width: fit-content'>XX XXX</div>
         <div style='width: 100px'><div id=fit-narrow style='width: fit-content'>XX XXX</div></div>
         <div style='width: 100px'><div id=max-wide style='width: max-content'>XX XXX</div></div>
         <div style='width: 30px'><div id=fit-tight style='width: fit-content'>XX XXX</div></div>
         <div id=edges style='width: max-content; padding: 0 5px; border: 1px solid;
           box-sizing: border-box'>XX</div>
         <div id=height style='height: min-content'>XX</div>",
    );
    assert_eq!(laid.rect("max").width, 120.0);
    assert_eq!(laid.rect("min").width, 60.0);
    // As much as there is room for, up to its max-content width.
    assert_eq!(laid.rect("fit").width, 120.0);
    assert_eq!(laid.rect("fit-narrow").width, 100.0);
    assert_eq!(laid.rect("fit-narrow").height, 40.0);
    // No narrower than its min-content width, even past the room.
    assert_eq!(laid.rect("fit-tight").width, 60.0);
    // Its max-content width whatever the room: it overflows.
    assert_eq!(laid.rect("max-wide").width, 120.0);
    // The content's, plus the padding and border.
    assert_eq!(laid.rect("edges").width, 52.0);
    // In the block axis, the keywords are `auto`.
    assert_eq!(laid.rect("height").height, 20.0);
}

#[test]
fn building_clears_the_layout_tree_flags() {
    let (mut dom, document) = parse("<!doctype html><div><p>a<b>b</b></p></div>", false);
    let (token, _) = style(&mut dom, &document);
    let flagged = |dom: &Dom| {
        dom.read(|cx| {
            let mut flagged = 0;
            each_node(cx, document.get(cx.gc), &mut |node| {
                flagged += usize::from(
                    cx.kind(node) == NodeKind::Element && !cx.layout_tree_flags(node).is_clean(),
                );
            });
            flagged
        })
    };
    assert!(flagged(&dom) > 0, "the parser's insertions flag them");
    dom.read(|cx| BoxTree::build(cx, &token, document.get(cx.gc), &mut TextContext::new(true)));
    assert_eq!(flagged(&dom), 0);
}

#[test]
fn a_body_under_another_root_isnt_the_body() {
    let mut page = Page::build_with(
        "<!doctype html><body id=b style='overflow: hidden'>a",
        |cx, document| {
            let html = cx
                .first_child(document)
                .and_then(|doctype| cx.next_sibling(doctype));
            let html = html.expect("the root element");
            let body = cx.last_child(html).expect("the body");
            let root = cx.create_element(document, crate::page::html("div"));
            cx.remove(html);
            cx.append_child(document, root).unwrap();
            cx.append_child(root, body).unwrap();
        },
    );
    assert_eq!(
        page.tree.viewport_overflow(),
        (Overflow::Visible, Overflow::Visible)
    );
    let laid = page.lay_out();
    let view = &laid.view;
    let root = &view.children[0].fragment;
    let body = &root.children[0].fragment;
    assert_eq!(body.node, Some(laid.node("b")));
    assert_eq!(body.role, layout::BoxRole::Other, "{}", view.dump());
}
