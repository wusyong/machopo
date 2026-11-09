//! Script's geometry (`layout::client_rects`, from the fragment tree)
//! against pre-paint's (`element_bounds`, from the property trees): the two
//! mappings must place every box the same, through transforms, scroll
//! containers' offsets and lines. Script's skips pre-paint, as Chrome's
//! forced layout does.

use std::collections::HashMap;

use gfx::{NodeId, PointF, RectF, SizeF};
use paint::ScrollOffsets;

use crate::page;

const VIEWPORT: SizeF = SizeF::new(400.0, 300.0);

/// Every element with an `id` in `html`, laid out and scrolled by `scroll`
/// (by id): the union of its client rects, and its pre-paint bounds.
fn both(
    html: &str,
    scroll: &[(&str, f32, f32)],
) -> Vec<(String, usize, Option<RectF>, Option<RectF>)> {
    let (view, ids) = page::lay_out(html, VIEWPORT);
    let offsets: ScrollOffsets = scroll
        .iter()
        .map(|(id, x, y)| (ids[*id], PointF::new(*x, *y)))
        .collect();
    let pre = paint::pre_paint(&view, &offsets, 1.0);
    let mut out: Vec<_> = ids
        .iter()
        .map(|(name, &id)| {
            let rects = layout::client_rects(&view, &offsets, &Default::default(), id);
            let union = rects.iter().copied().reduce(|a, b| a.union(&b));
            (
                name.clone(),
                rects.len(),
                union,
                paint::element_bounds(&view, &pre, id),
            )
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// The two agree on every element; how many client rects each has.
fn assert_same(html: &str, scroll: &[(&str, f32, f32)]) -> HashMap<String, usize> {
    both(html, scroll)
        .into_iter()
        .map(|(name, count, rects, bounds)| {
            assert_eq!(rects, bounds, "#{name}");
            (name, count)
        })
        .collect()
}

#[test]
fn transforms_place_boxes_as_pre_paint_does() {
    assert_same(
        "<style>body { margin: 0 } div { width: 100px; height: 40px }</style>\
         <div id=plain></div>\
         <div id=moved style='transform: translate(10px, 5px)'>\
           <div id=inside style='margin-left: 7px; width: 20px; height: 10px'></div></div>\
         <div id=scaled style='transform: scale(2, 0.5)'>\
           <div id=nested style='transform: translate(50%, 0); width: 30px'></div></div>\
         <div id=matrix style='transform: matrix(1, 0.2, 0.3, 1, 4, 8)'></div>\
         <div id=turned style='transform: rotate(30deg) skew(10deg, 5deg)'>\
           <div id=turned_inside style='margin-left: 7px; width: 20px; height: 10px'></div></div>\
         <div id=cornered style='transform-origin: 0 100%; transform: rotate(-45deg)'></div>\
         <div id=content style='padding: 5px 10px; border: 3px solid; transform-box: content-box;\
                                transform-origin: 0 0; transform: scale(2) rotate(10deg)'></div>",
        &[],
    );
}

#[test]
fn scroll_containers_move_their_content_as_pre_paint_does() {
    assert_same(
        "<style>body { margin: 0 }</style>\
         <div id=outer style='height: 100px; overflow: auto'>\
           <div style='height: 50px'></div>\
           <div id=inner style='height: 60px; overflow: auto; transform: translate(3px, 0)'>\
             <div style='height: 40px'></div><div id=deep style='height: 10px'></div>\
             <p style='margin: 0; font: 20px/1 Ahem'>a <span id=word>b</span></p>\
             <div style='height: 200px'></div></div>\
           <div style='height: 300px'></div></div>\
         <div id=lined style='width: 60px; height: 30px; overflow: auto; font: 20px/1 Ahem'>\
           aa <span id=on_line>bb</span> cc dd ee</div>",
        &[
            ("outer", 0.0, 30.0),
            ("inner", 0.0, 25.0),
            ("lined", 0.0, 20.0),
        ],
    );
}

#[test]
fn inline_boxes_on_lines_as_pre_paint_does() {
    let counts = assert_same(
        "<style>body { margin: 0; width: 120px; font: 20px/1 Ahem }</style>\
         <p id=p style='margin: 0'>aa <span id=span>bb cc dd ee</span> ff\
           <b id=empty></b></p>\
         <div id=flex style='display: flex; transform: translate(0, 4px)'>\
           <span id=item style='width: 30px'>x</span>\
           <span id=other>y <i id=italic>z</i></span></div>",
        &[],
    );
    // A box on each line the span is on; one for a block.
    assert!(counts["span"] > 1, "{counts:?}");
    assert_eq!(counts["p"], 1);
}

/// `bounding_rect` as CSSOM View says: an empty list is an empty rect at the
/// origin; rects without width or height don't count unless all are like
/// that, when the first is it.
#[test]
fn the_bounding_rect_of_client_rects() {
    let r = RectF::new;
    assert_eq!(layout::bounding_rect(&[]), r(0.0, 0.0, 0.0, 0.0));
    assert_eq!(
        layout::bounding_rect(&[r(5.0, 5.0, 0.0, 10.0), r(1.0, 1.0, 2.0, 0.0)]),
        r(5.0, 5.0, 0.0, 10.0)
    );
    assert_eq!(
        layout::bounding_rect(&[
            r(50.0, 50.0, 0.0, 0.0),
            r(0.0, 0.0, 10.0, 10.0),
            r(20.0, 5.0, 10.0, 10.0)
        ]),
        r(0.0, 0.0, 30.0, 15.0)
    );
}

/// The shared transform (`box_transform`) against numbers worked out by
/// hand, which comparing with pre-paint can't check: a 100×40 box scaled
/// twice around its center; one 30×40 moved by half its width; and one
/// with two functions, which don't commute (the first is the outermost).
#[test]
fn transforms_are_about_the_center_and_percentages_of_the_box() {
    let (view, ids) = page::lay_out(
        "<style>body { margin: 0 } div { width: 100px; height: 40px }</style>\
         <div id=scaled style='transform: scale(2)'></div>\
         <div id=moved style='width: 30px; transform: translate(50%, 25%)'></div>\
         <div id=both style='transform: translate(10px) scale(2)'></div>",
        VIEWPORT,
    );
    let rects =
        |id: &str| layout::client_rects(&view, &Default::default(), &Default::default(), ids[id]);
    assert_eq!(rects("scaled"), [RectF::new(-50.0, -20.0, 200.0, 80.0)]);
    assert_eq!(rects("moved"), [RectF::new(15.0, 50.0, 30.0, 40.0)]);
    // Scaled about its center, then moved by 10: the other order would
    // move it by 20.
    assert_eq!(rects("both"), [RectF::new(-40.0, 60.0, 200.0, 80.0)]);
}

/// `transform-origin` in the reference box (`transform-box`), against
/// numbers worked out by hand: a 100×40 box turned a quarter about its
/// center, one about its top left corner, one skewed along x by 45°
/// about its center (each row 20 across), and one scaled twice about its
/// content box's top left (its 3px border and 10px padding put that at
/// 13 across and 13 down in its 126×66 border box).
#[test]
fn transforms_turn_about_their_origin_in_their_reference_box() {
    let (view, ids) = page::lay_out(
        "<style>body { margin: 0 } div { width: 100px; height: 40px }</style>\
         <div id=turned style='transform: rotate(90deg)'></div>\
         <div id=cornered style='transform-origin: left top; transform: rotate(90deg)'></div>\
         <div id=skewed style='transform: skewX(45deg)'></div>\
         <div id=content style='padding: 10px; border: 3px solid; transform-box: content-box;\
                                transform-origin: 0 0; transform: scale(2)'></div>",
        VIEWPORT,
    );
    let rect = |id: &str| {
        let [r] =
            layout::client_rects(&view, &Default::default(), &Default::default(), ids[id])[..]
        else {
            panic!("#{id} has one box");
        };
        // To a thousandth: the tangent of 45° isn't exactly 1.
        [r.x, r.y, r.width, r.height].map(|v| (v * 1000.0).round() / 1000.0)
    };
    assert_eq!(rect("turned"), [30.0, -30.0, 40.0, 100.0]);
    assert_eq!(rect("cornered"), [-40.0, 40.0, 40.0, 100.0]);
    assert_eq!(rect("skewed"), [-20.0, 80.0, 140.0, 40.0]);
    // From (0, 120): the content box's corner (13, 133) stays.
    assert_eq!(rect("content"), [-13.0, 107.0, 252.0, 132.0]);
}

/// Each element with an `id` in `html`, laid out and scrolled by `scroll`:
/// its `layout::element_box`, and its client rects.
fn element_boxes(
    html: &str,
    scroll: &[(&str, f32, f32)],
) -> HashMap<String, (Option<gfx::ElementBox>, Vec<RectF>)> {
    let (view, ids) = page::lay_out(html, VIEWPORT);
    let offsets: ScrollOffsets = scroll
        .iter()
        .map(|(id, x, y)| (ids[*id], PointF::new(*x, *y)))
        .collect();
    ids.iter()
        .map(|(name, &id)| {
            (
                name.clone(),
                (
                    layout::element_box(&view, &offsets, &Default::default(), id),
                    layout::client_rects(&view, &offsets, &Default::default(), id),
                ),
            )
        })
        .collect()
}

/// `element_box` places boxes before transforms and scrolling (where
/// `offset*` reads them), unlike the client rects; a scroll container has
/// its offset, borders and scrollable overflow; an inline element's box is
/// around its lines, starting at its first one, and has no block geometry.
#[test]
fn element_boxes_are_before_transforms_and_scrolling() {
    let boxes = element_boxes(
        r#"<style>
            body { margin: 0 }
            #scroller { overflow: scroll; width: 100px; height: 50px; border: 2px solid;
                        margin-left: 10px; transform: translate(5px, 5px) }
            #content { width: 300px; height: 200px }
            #lines { width: 60px }
           </style>
           <div id="scroller"><div id="content"></div></div>
           <div id="lines">xx <span id="span">aaaa bbbb cccc</span></div>
           <div id="none" style="display: none"></div>"#,
        &[("scroller", 0.0, 30.0)],
    );
    let (scroller, scroller_rects) = &boxes["scroller"];
    let scroller = scroller.expect("a box");
    assert_eq!(scroller.border_box, RectF::new(10.0, 0.0, 104.0, 54.0));
    assert_eq!(scroller.start, PointF::new(10.0, 0.0));
    let block = scroller.block.expect("a block");
    assert_eq!(block.border, [2.0; 4]);
    assert_eq!(block.margin, [0.0, 0.0, 0.0, 10.0], "the used margins");
    assert_eq!(block.padding, [0.0; 4]);
    assert_eq!(block.scroll, Some(PointF::new(0.0, 30.0)));
    assert_eq!(block.scroll_size, SizeF::new(300.0, 200.0));
    // The client rect is moved by the transform.
    assert_eq!(scroller_rects[0], RectF::new(15.0, 5.0, 104.0, 54.0));

    // Inside the scroller: neither its transform nor its offset.
    let (content, rects) = &boxes["content"];
    let content = content.expect("a box");
    assert_eq!(content.border_box, RectF::new(12.0, 2.0, 300.0, 200.0));
    assert_eq!(content.block.expect("a block").scroll, None);
    assert_eq!(rects[0], RectF::new(17.0, -23.0, 300.0, 200.0));

    // Not transformed or scrolled: around the client rects.
    let (span, rects) = &boxes["span"];
    let span = span.expect("a box");
    assert!(rects.len() > 1, "on several lines");
    let union = rects[1..].iter().fold(rects[0], |a, b| a.union(b));
    assert_eq!(span.border_box, union);
    assert_eq!(span.start, PointF::new(rects[0].x, rects[0].y));
    // It starts after "xx " on its first line, right of where the others
    // start.
    assert!(span.start.x > span.border_box.x);
    assert_eq!(span.block, None);

    assert_eq!(boxes["none"].0, None);
}

/// The root element's margins don't collapse with its children's (CSS 2.2
/// §8.3.1): the root establishes a block formatting context, so `<body>`'s
/// margin and its first child's stay inside the root's box, which starts
/// at the top, as in Chrome.
#[test]
fn the_roots_margins_dont_collapse_with_its_childrens() {
    let boxes = element_boxes(
        r#"<html id="root"><body id="body" style="margin: 8px">
           <div id="first" style="margin-top: 5px; height: 10px"></div></body></html>"#,
        &[],
    );
    let y = |name: &str| boxes[name].0.expect("a box").border_box.y;
    assert_eq!(y("root"), 0.0);
    assert_eq!(y("body"), 8.0);
    assert_eq!(y("first"), 8.0);
}

/// `hit_test_all` lists every element at a point, the topmost first, each
/// once (an element hit by its box and its text too): what is painted over
/// another comes before it, its ancestors after; nothing where only the
/// canvas is. `hit_test` is its first.
#[test]
fn hit_test_all_lists_what_is_under_the_point() {
    let (view, ids) = page::lay_out(
        r#"<html id="html"><body id="body" style="margin: 0">
           <div id="under" style="height: 50px">text under</div>
           <div id="over" style="height: 50px; transform: translate(0, -40px)">over</div>
           </body></html>"#,
        VIEWPORT,
    );
    let pre = paint::pre_paint(&view, &ScrollOffsets::default(), 1.0);
    let names = |nodes: Vec<NodeId>| -> Vec<String> {
        nodes
            .iter()
            .map(|n| {
                ids.iter()
                    .find(|(_, id)| *id == n)
                    .map(|(k, _)| k.clone())
                    .unwrap()
            })
            .collect()
    };
    // Where `over` (moved up 40px) covers `under`.
    let at = PointF::new(5.0, 15.0);
    assert_eq!(
        names(paint::hit_test_all(&view, &pre, at)),
        ["over", "under", "body", "html"]
    );
    assert_eq!(paint::hit_test(&view, &pre, at).node, Some(ids["over"]));
    // Below everything: the canvas only.
    assert_eq!(
        paint::hit_test_all(&view, &pre, PointF::new(5.0, 290.0)),
        []
    );
}

/// A `visibility: hidden` box isn't hit, its text and inline boxes
/// neither, as they aren't painted; its `visible` descendants are, and
/// what is under it.
#[test]
fn hidden_boxes_arent_hit() {
    let (view, ids) = page::lay_out(
        r#"<html id="html"><body id="body" style="margin: 0; font: 10px/1 Ahem">
           <div id="under" style="height: 50px"></div>
           <div id="hidden" style="height: 50px; margin-top: -50px; visibility: hidden">
             <div id="shown" style="height: 20px; visibility: visible"></div>
             <span id="span">hidden text</span></div>
           </body></html>"#,
        VIEWPORT,
    );
    let pre = paint::pre_paint(&view, &ScrollOffsets::default(), 1.0);
    let hit = |x: f32, y: f32| paint::hit_test(&view, &pre, PointF::new(x, y)).node;
    assert_eq!(hit(5.0, 10.0), Some(ids["shown"]));
    // The hidden text (on the line after `shown`) and its box: `under`.
    assert_eq!(hit(5.0, 25.0), Some(ids["under"]));
    assert_eq!(hit(150.0, 45.0), Some(ids["under"]));
}

/// `pointer-events: none` boxes, text and inline boxes aren't hit, an
/// iframe's child frame neither; what is under them is, and so are their
/// descendants that say otherwise (`auto`).
#[test]
fn pointer_events_none_isnt_hit() {
    let (view, ids) = page::lay_out(
        r#"<html id="html"><body id="body" style="margin: 0; font: 10px/1 Ahem">
           <div id="under" style="height: 50px"></div>
           <div id="over" style="height: 50px; margin-top: -50px; pointer-events: none">
             <div id="auto" style="height: 20px; pointer-events: auto"></div>
             <span id="span">text</span></div>
           </body></html>"#,
        VIEWPORT,
    );
    let pre = paint::pre_paint(&view, &ScrollOffsets::default(), 1.0);
    let hit = |x: f32, y: f32| paint::hit_test(&view, &pre, PointF::new(x, y)).node;
    assert_eq!(hit(5.0, 10.0), Some(ids["auto"]));
    assert_eq!(hit(5.0, 25.0), Some(ids["under"]));
    assert_eq!(hit(150.0, 45.0), Some(ids["under"]));
}

/// `hit_test_all` lists an element for its own box, not its text (Blink's
/// `ElementsFromHitTestResult` drops text nodes) nor an anonymous box in
/// it. Where `outer`'s text is over `inner` (pulled up by a negative
/// margin), `inner` comes first, then `outer` for its own box, not the
/// anonymous block around the line; `hit_test` is `outer`, for its text.
/// An inline `span` around the text is listed first, for its inline box.
#[test]
fn hit_test_all_lists_no_element_for_its_text() {
    for (text, first) in [
        ("Hello", None),
        (r#"<span id="span">Hello</span>"#, Some("span")),
    ] {
        lists_no_element_for_its_text(text, first);
    }
}

fn lists_no_element_for_its_text(text: &str, first: Option<&str>) {
    let (view, ids) = page::lay_out(
        &format!(
            r#"<html id="html"><body id="body" style="margin: 0">
           <div id="outer"><div id="inner" style="height: 100px; margin-bottom: -100px"></div>{text}</div>
           </body></html>"#
        ),
        VIEWPORT,
    );
    let pre = paint::pre_paint(&view, &ScrollOffsets::default(), 1.0);
    let at = PointF::new(2.0, 2.0);
    let all: Vec<NodeId> = first
        .into_iter()
        .chain(["inner", "outer", "body", "html"])
        .map(|k| ids[k])
        .collect();
    assert_eq!(paint::hit_test_all(&view, &pre, at), all, "{text}");
    let topmost = first.unwrap_or("outer");
    assert_eq!(paint::hit_test(&view, &pre, at).node, Some(ids[topmost]));
}

/// The used padding and margins a resolved `padding-*` / `margin-*`
/// reads: percentages and `auto` resolved by layout.
#[test]
fn boxes_carry_their_used_padding_and_margins() {
    let boxes = element_boxes(
        r#"<style>
            body { margin: 0; width: 400px }
            #a { width: 100px; padding: 1px 10% 3px 4px; margin: 0 auto }
           </style>
           <div id="a"></div>"#,
        &[],
    );
    let block = boxes["a"].0.expect("a box").block.expect("a block");
    assert_eq!(
        block.padding,
        [1.0, 40.0, 3.0, 4.0],
        "a percentage against the containing block's width"
    );
    assert_eq!(
        block.margin,
        [0.0, 128.0, 0.0, 128.0],
        "`auto` is what layout gave it: (400 - 100 - 44) / 2 each side"
    );
}
