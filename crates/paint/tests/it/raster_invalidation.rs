//! Raster invalidation: what a layer has to raster again when a page is
//! painted again after a change.

use std::sync::Arc;

use gfx::{RectF, SizeF};
use paint::PaintedLayer;

use crate::page::Page;

const VIEWPORT: SizeF = SizeF::new(800.0, 600.0);

thread_local! {
    /// One for every paint, as a renderer keeps one: the same fonts each
    /// time.
    static TEXT: std::cell::Cell<Option<layout::TextContext>> =
        const { std::cell::Cell::new(None) };
}

/// Each layer of `html`, painted.
fn painted(html: &str) -> Vec<PaintedLayer> {
    let text = TEXT
        .take()
        .unwrap_or_else(|| layout::TextContext::new(true));
    let mut page = Page::with_text(html, text, |_, _| {});
    let view = page.lay_out(VIEWPORT);
    TEXT.set(Some(page.text));
    let pre = paint::pre_paint(&view, &Default::default(), 1.0);
    let document = layout::document_size(&view);
    let artifact = Arc::new(paint::paint(&view, &pre, document, &()));
    paint::layerize(&artifact, &pre.trees, document, crate::MAX_TEXTURE_SIZE)
        .iter()
        .map(|layer| PaintedLayer::new(&artifact, &pre.trees, layer))
        .collect()
}

/// What the document's layer rasters again going from `old` to `new`,
/// merged into one rect (or none).
fn invalidated(old: &str, new: &str) -> Option<RectF> {
    let (old, new) = (painted(old), painted(new));
    new[0].invalidation(&old[0]).into_iter().reduce(union)
}

fn union(a: RectF, b: RectF) -> RectF {
    let (x0, y0) = (a.x.min(b.x), a.y.min(b.y));
    let (x1, y1) = (a.right().max(b.right()), a.bottom().max(b.bottom()));
    RectF::new(x0, y0, x1 - x0, y1 - y0)
}

fn covers(outer: RectF, inner: RectF) -> bool {
    outer.x <= inner.x
        && outer.y <= inner.y
        && outer.right() >= inner.right()
        && outer.bottom() >= inner.bottom()
}

const RESET: &str = "<style>html, body { margin: 0 } div { width: 100px; height: 50px }</style>";

fn page(boxes: &str) -> String {
    format!("{RESET}<body style='background-color: white'>{boxes}</body>")
}

#[test]
fn nothing_changed_rasters_nothing() {
    let html =
        page("<div style='background-color: green'></div><p style='font: 20px/1 Ahem'>Text</p>");
    assert_eq!(invalidated(&html, &html), None);
}

#[test]
fn a_box_that_changed_color_is_rastered_again_and_nothing_else() {
    let old = page(
        "<div style='background-color: green'></div><div style='background-color: blue'></div>",
    );
    let new = page(
        "<div style='background-color: green'></div><div style='background-color: red'></div>",
    );
    assert_eq!(
        invalidated(&old, &new),
        Some(RectF::new(0.0, 50.0, 100.0, 50.0))
    );
}

#[test]
fn a_box_that_moved_is_rastered_where_it_was_and_where_it_is() {
    let old = page("<div style='background-color: green'></div>");
    let new = page("<div style='margin-left: 200px; background-color: green'></div>");
    let (old, new) = (painted(&old), painted(&new));
    let mut rects = new[0].invalidation(&old[0]);
    rects.sort_by(|a, b| a.x.total_cmp(&b.x));
    assert_eq!(
        rects,
        [
            RectF::new(0.0, 0.0, 100.0, 50.0),
            RectF::new(200.0, 0.0, 100.0, 50.0)
        ]
    );
}

#[test]
fn a_box_that_came_or_went_is_rastered_where_it_is_or_was() {
    let one = page("<div style='background-color: green'></div>");
    let two = page(
        "<div style='background-color: green'></div><div style='background-color: blue'></div>",
    );
    let added = Some(RectF::new(0.0, 50.0, 100.0, 50.0));
    assert_eq!(invalidated(&one, &two), added);
    assert_eq!(invalidated(&two, &one), added);
}

/// The same items, but now under another opacity or transform: drawn
/// differently all the same.
#[test]
fn a_box_under_another_effect_or_transform_is_rastered_again() {
    let plain = page("<div style='background-color: green'></div>");
    let faded = page("<div style='background-color: green; opacity: 0.5'></div>");
    assert_eq!(
        invalidated(&plain, &faded),
        Some(RectF::new(0.0, 0.0, 100.0, 50.0))
    );
    // Its chunk the same but for the opacity.
    let fainter = page("<div style='background-color: green; opacity: 0.4'></div>");
    assert_eq!(
        invalidated(&faded, &fainter),
        Some(RectF::new(0.0, 0.0, 100.0, 50.0))
    );
    let turned = page(
        "<div style='background-color: green; transform: matrix(0.985, 0.174, -0.174, 0.985, 0, 0)'></div>",
    );
    let rect = invalidated(&plain, &turned).expect("rastered again");
    assert!(
        covers(rect, RectF::new(-1.0, -1.0, 102.0, 52.0)),
        "{rect:?}: where it was, and anti-aliased edges where it is"
    );
}

/// Two overlapping boxes that swap places in tree order, where they are
/// staying the same: the same items, but the other one is on top.
#[test]
fn boxes_painted_in_another_order_are_rastered_again() {
    let old = page(
        "<div id=a style='background-color: green'></div>
         <div id=b style='background-color: blue; margin-top: -25px'></div>",
    );
    let new = page(
        "<div id=b style='background-color: blue; margin-top: 25px'></div>
         <div id=a style='background-color: green; margin-top: -75px'></div>",
    );
    let rect = invalidated(&old, &new).expect("rastered again");
    assert!(covers(rect, RectF::new(0.0, 0.0, 100.0, 75.0)), "{rect:?}");
}

#[test]
fn text_that_changed_is_rastered_again_where_it_is() {
    let old =
        page("<div style='font: 20px/1 Ahem'>Ab</div><div style='font: 20px/1 Ahem'>Cd</div>");
    let new =
        page("<div style='font: 20px/1 Ahem'>Ab</div><div style='font: 20px/1 Ahem'>Ce</div>");
    // The second box's text, at 50 (each box is 50 high), whose bounds
    // reach half a font size past its line: 40 to 80.
    let rect = invalidated(&old, &new).expect("rastered again");
    assert_eq!(
        (rect.y, rect.bottom()),
        (40.0, 80.0),
        "only the second box's"
    );
}

/// The same two boxes, where they were and as they were, but the first now
/// paints after the second (a transform that moves nothing makes it a
/// stacking context): it is on top now, where they overlap. (It is in a
/// chunk of its own now, and the second in another than they shared: the
/// second went from one and came to the other.)
#[test]
fn the_same_items_painted_in_another_order_are_rastered_again() {
    let old = page(
        "<div style='background-color: green'></div>
         <div style='background-color: blue; margin-top: -25px'></div>",
    );
    let new = page(
        "<div style='background-color: green; transform: translate(0px, 0px)'></div>
         <div style='background-color: blue; margin-top: -25px'></div>",
    );
    let rect = invalidated(&old, &new).expect("rastered again");
    assert!(covers(rect, RectF::new(0.0, 25.0, 100.0, 25.0)), "{rect:?}");
}

/// Each line of an element's text is an item of its own: a change in the
/// second line rasters only it, not the first, which is the same.
#[test]
fn only_the_line_that_changed_is_rastered_again() {
    let text = |words: &str| {
        page(&format!(
            "<div style='width: 40px; font: 20px/1 Ahem'>{words}</div>"
        ))
    };
    // Two lines: 0 to 20 and 20 to 40; the second's bounds reach half a
    // font size past it, 10 to 50.
    let rect = invalidated(&text("XX XX"), &text("XX XY")).expect("rastered again");
    assert_eq!((rect.y, rect.bottom()), (10.0, 50.0), "{rect:?}");
}

/// Under a transform that isn't a translation, anti-aliasing reaches a
/// pixel past the edges.
#[test]
fn a_scaled_box_is_rastered_again_with_a_pixel_around_it() {
    let box_ = |color: &str| {
        page(&format!(
            "<div style='background-color: {color}; transform: scale(2)'></div>"
        ))
    };
    // 100x50, scaled about its center (50, 25): -50, -25 to 150, 75.
    assert_eq!(
        invalidated(&box_("green"), &box_("red")),
        Some(RectF::new(-51.0, -26.0, 202.0, 102.0))
    );
}

/// What a clip cuts away isn't drawn, and isn't rastered again.
#[test]
fn what_is_clipped_away_is_not_rastered_again() {
    // The opacity keeps the container in the document's layer.
    let clipped = |color: &str| {
        page(&format!(
            "<div style='overflow: hidden; opacity: 0.9'>
               <div style='width: 200px; height: 20px; background-color: {color}'></div>
             </div>"
        ))
    };
    assert_eq!(
        invalidated(&clipped("green"), &clipped("red")),
        Some(RectF::new(0.0, 0.0, 100.0, 20.0))
    );
}

/// A clip that changed shape in the same bounds (a `clip-path`'s path, a
/// rounded corner) rasters what it cuts again.
#[test]
fn a_clip_of_another_shape_in_the_same_bounds_is_rastered_again() {
    let clipped = |clip: &str| {
        page(&format!(
            "<div style='background-color: green; clip-path: {clip}'></div>"
        ))
    };
    let (box_, triangle) = (
        "polygon(0 0, 100% 0, 100% 100%, 0 100%)",
        "polygon(0 0, 100% 0, 0 100%)",
    );
    assert_eq!(
        invalidated(&clipped(box_), &clipped(triangle)),
        Some(RectF::new(0.0, 0.0, 100.0, 50.0))
    );
    assert_eq!(
        invalidated(&clipped("inset(0)"), &clipped("inset(0 round 10px)")),
        Some(RectF::new(0.0, 0.0, 100.0, 50.0))
    );
}

/// A filter that changed rasters what it made again, as far as it reaches
/// past what it filtered (three times a blur's sigma) and past the clips
/// inside it; another opacity, where it filters.
#[test]
fn a_changed_filter_is_rastered_again_as_far_as_it_reaches() {
    let filtered = |filter: &str| {
        page(&format!(
            "<div style='margin: 40px; overflow: hidden; height: 20px; filter: {filter}'>
               <div style='height: 40px; background-color: green'></div></div>"
        ))
    };
    assert_eq!(
        invalidated(&filtered("blur(1px)"), &filtered("blur(2px)")),
        Some(RectF::new(34.0, 34.0, 112.0, 32.0))
    );
    assert_eq!(
        invalidated(&filtered("opacity(1)"), &filtered("opacity(0.5)")),
        Some(RectF::new(40.0, 40.0, 100.0, 20.0))
    );
    // A shadow's offset, and its blur's sigma (half its radius), the
    // larger of the two filters'.
    assert_eq!(
        invalidated(
            &filtered("drop-shadow(red 4px 0 2px)"),
            &filtered("drop-shadow(blue 4px 0 2px)")
        ),
        Some(RectF::new(33.0, 33.0, 114.0, 34.0))
    );
    // In the filter's space, scaled into the layer's (past the pixel of
    // anti-aliasing a scaled item gets): 6px twice.
    let scaled = |filter: &str| {
        page(&format!(
            "<div style='margin: 40px; height: 20px; transform: scale(2); filter: {filter}'>
               <div style='height: 20px; background-color: green'></div></div>"
        ))
    };
    assert_eq!(
        invalidated(&scaled("blur(1px)"), &scaled("blur(2px)")).map(|r| r.y),
        Some(30.0 - 1.0 - 12.0)
    );
}

/// A stacking context's chunk that came or went is rastered where it is or
/// was; two that swap places in paint order, where they overlap.
#[test]
fn chunks_that_came_went_or_swapped_are_rastered_again() {
    let one = page("<div style='background-color: green'></div>");
    let two = page(
        "<div style='background-color: green'></div>\
         <div style='background-color: blue; opacity: 0.5'></div>",
    );
    let added = Some(RectF::new(0.0, 50.0, 100.0, 50.0));
    assert_eq!(invalidated(&one, &two), added);
    assert_eq!(invalidated(&two, &one), added);
    let order = |first: i32, second: i32| {
        page(&format!(
            "<div style='background-color: green; opacity: 0.5; position: relative; z-index: {first}'></div>\
             <div style='background-color: blue; opacity: 0.5; position: relative; z-index: {second}; margin-top: -25px'></div>"
        ))
    };
    let rect = invalidated(&order(1, 2), &order(2, 1)).expect("rastered again");
    assert!(covers(rect, RectF::new(0.0, 25.0, 100.0, 25.0)), "{rect:?}");
}

/// An element's text in two chunks starting with it (in its own stacking
/// context, its lines go on after an inline block whose content is
/// clipped): each is told from the other, so nothing is rastered again when
/// nothing changed, and only the inline block's text when it changed.
#[test]
fn an_elements_items_in_two_chunks_are_told_apart() {
    let text = |inner: &str| {
        page(&format!(
            "<p style='font: 20px/1 Ahem; opacity: 0.9'>aaa <span style='display: inline-block; overflow: hidden; width: 60px; height: 20px'>{inner}</span> ccc</p>"
        ))
    };
    assert_eq!(invalidated(&text("bbb"), &text("bbb")), None);
    let rect = invalidated(&text("bbb"), &text("bbx")).expect("rastered again");
    assert!(rect.x >= 80.0 && rect.right() <= 140.0, "{rect:?}");
}
