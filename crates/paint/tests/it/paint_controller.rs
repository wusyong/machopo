//! Painting again after a change with a `PaintController`: which boxes'
//! items are taken from the last paint, and that the artifact, and what is
//! rastered again, are what painting everything gives.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use dom::{Cx, NodeRef, TargetRef, names};
use gfx::{ImageData, RectF, Size, SizeF};
use gfx::{NodeId, svg::SvgNaturalSize};
use paint::{PaintArtifact, PaintController, PaintedLayer};

use crate::page::{self, find};

const VIEWPORT: SizeF = SizeF::new(800.0, 600.0);

struct Page {
    page: page::Page,
    controller: PaintController,
    /// The last pre-paint, which the next keeps the node ids of.
    pre: Option<paint::PrePaint>,
    /// The scroll containers' offsets the page is painted with.
    scroll: paint::ScrollOffsets,
    /// Whether each pre-paint is from scratch, not from the last.
    fresh_pre_paints: bool,
    /// The child frames, laid out once.
    child_frames: paint::ChildFrames,
    device_scale_factor: f32,
    /// The document's layer as last painted, with and without the
    /// controller.
    layer: Option<(PaintedLayer, PaintedLayer)>,
    /// Every paint's, in order.
    history: Vec<(PaintedLayer, PaintedLayer)>,
    /// What is drawn as selected.
    selected: Selected,
}

/// What is drawn as selected, by text node (and `<br>`), and the caret
/// while it is shown, as the renderer gives paint.
#[derive(Default)]
struct Selected(
    RefCell<HashMap<NodeId, paint::TextSelection>>,
    std::cell::Cell<Option<paint::Caret>>,
);

impl paint::BackgroundImages for Selected {
    fn image(&self, _: &str) -> Option<(ImageData, Size)> {
        None
    }

    fn selection(&self, text: NodeId) -> Option<paint::TextSelection> {
        self.0.borrow().get(&text).cloned()
    }

    fn caret(&self) -> Option<paint::Caret> {
        self.1.get()
    }
}

/// One paint: what it painted, and what it rasters again.
struct Painted {
    artifact: Arc<PaintArtifact>,
    invalidation: Vec<RectF>,
}

impl Page {
    fn new(html: &str) -> Self {
        Self::of(page::Page::new(html))
    }

    /// [`Page::new`], the document with a window: its images load.
    fn with_window(html: &str) -> Self {
        Self::of(page::Page::with(html, page::give_window))
    }

    fn of(page: page::Page) -> Self {
        let mut page = Self {
            page,
            controller: PaintController::default(),
            pre: None,
            scroll: Default::default(),
            fresh_pre_paints: false,
            child_frames: Default::default(),
            device_scale_factor: 1.0,
            layer: None,
            history: Vec::new(),
            selected: Selected::default(),
        };
        page.paint();
        page
    }

    fn id(&self, id: &str) -> NodeId {
        self.page.id(id)
    }

    /// The element `id`'s first child's id, and its own.
    fn text_of(&self, id: &str) -> (NodeId, NodeId) {
        let document = &self.page.document;
        self.page.dom.read(|cx| {
            let element = find(cx, document.get(cx.gc), id);
            let text = cx.first_child(element).expect("a child");
            (cx.node_id(text), cx.node_id(element))
        })
    }

    /// Gives iframe `id` a child frame of `html`, laid out once.
    fn set_child_frame(&mut self, id: &str, html: &str) -> Painted {
        let iframe = self.id(id);
        let (mut tree, document) = self.page.frame(html, |_, _| {});
        let layout_view = tree.layout(SizeF::new(300.0, 150.0));
        let frame = paint::ChildFrame {
            document,
            viewport_scroll: gfx::PointF::new(0.0, 0.0),
            user_scrollable_x: true,
            user_scrollable_y: true,
            scroll: Default::default(),
            document_size: layout::document_size(&layout_view),
            layout_view,
        };
        self.child_frames.insert(iframe, frame);
        self.paint()
    }

    fn change(&mut self, change: impl for<'gc> FnOnce(&Cx<'_, 'gc>, NodeRef<'gc>)) -> Painted {
        let document = &self.page.document;
        self.page.dom.mutate(|cx| change(cx, document.get(cx.gc)));
        self.paint()
    }

    fn set_style(&mut self, id: &str, style: &str) -> Painted {
        self.change(|cx, document| cx.set_attribute(find(cx, document, id), "style", style))
    }

    /// The images the page's `<img>`s fetch come, `image` each: it is
    /// painted after they are fetched, and again as they come.
    fn image_arrives(&mut self, image: dom::Image) -> Painted {
        let fetched = self.page.fetch_images();
        assert!(!fetched.is_empty(), "an image fetched");
        self.paint();
        self.change(|cx, _| {
            for url in &fetched {
                cx.image_fetched(url, Some(image.clone()), true);
            }
        })
    }

    /// Styles, lays out and paints the page again, checking the paint is
    /// the one painting everything gives, and what it rasters again too.
    fn paint(&mut self) -> Painted {
        self.page.update();
        let view = self.page.lay_out(VIEWPORT);
        let pre = paint::pre_paint_frame_tree(
            &view,
            &self.scroll,
            &Default::default(),
            &self.child_frames,
            self.device_scale_factor,
            gfx::PointF::new(0.0, 0.0),
            self.pre.take().filter(|_| !self.fresh_pre_paints),
        );
        let document = layout::document_size(&view);
        let artifact = self.controller.paint(&view, &pre, document, &self.selected);
        let fresh = Arc::new(paint::paint(&view, &pre, document, &self.selected));
        assert_eq!(
            artifact.items, fresh.items,
            "the items painting everything gives"
        );
        assert_eq!(artifact.chunks, fresh.chunks);
        assert_eq!(artifact.hit_test, fresh.hit_test, "the targets too");
        // The controller's, after the last as the compositor paints it,
        // taking whole the chunks painted as they were; and painting
        // everything's, compared item by item.
        let layers = paint::layerize(&artifact, &pre.trees, document, crate::MAX_TEXTURE_SIZE);
        let fresh_layers = paint::layerize(&fresh, &pre.trees, document, crate::MAX_TEXTURE_SIZE);
        let uncached = PaintedLayer::new(&fresh, &pre.trees, &fresh_layers[0]);
        let (cached, invalidation) = match &self.layer {
            Some((old_cached, old_uncached)) => {
                let (cached, invalidation) =
                    PaintedLayer::after(&artifact, &pre.trees, &layers[0], old_cached);
                assert_eq!(
                    invalidation,
                    uncached.invalidation(old_uncached),
                    "what comparing every item rasters again"
                );
                (cached, invalidation)
            }
            None => (
                PaintedLayer::new(&artifact, &pre.trees, &layers[0]),
                Vec::new(),
            ),
        };
        self.history.push((cached.clone(), uncached.clone()));
        self.layer = Some((cached, uncached));
        self.pre = Some(pre);
        Painted {
            artifact,
            invalidation,
        }
    }
}

impl Painted {
    /// The elements with items painted anew, not taken from the last
    /// paint.
    fn repainted(&self) -> BTreeSet<Option<NodeId>> {
        let cached = self.artifact.cached.as_ref().expect("painted before");
        self.artifact
            .items
            .iter()
            .zip(&cached.indices)
            .filter(|(_, index)| index.is_none())
            .map(|(item, _)| item.client)
            .collect()
    }
}

const PAGE: &str = "<style>html, body { margin: 0 } \
    div { width: 100px; height: 50px; background-color: green }</style>\
    <body style='background-color: white'>\
    <section id=section style='height: 300px; background-color: yellow'>\
    <div id=a></div><div id=b></div><div id=c></div></section>\
    <p id=p style='font: 20px/1 Ahem'>Some <span id=s style='background-color: blue'>text</span></p>\
    </body>";

#[test]
fn nothing_changed_takes_every_item() {
    let mut page = Page::new(PAGE);
    let painted = page.paint();
    // Only the canvas background, which is painted each time.
    assert_eq!(painted.repainted().len(), 1, "{:?}", painted.repainted());
    assert!(painted.invalidation.is_empty());
}

#[test]
fn a_color_paints_that_box_again_and_nothing_else() {
    let mut page = Page::new(PAGE);
    let b = page.id("b");
    let painted = page.set_style("b", "background-color: red");
    let body = page.id("section");
    // The canvas background, and `#b`; `<section>`'s fragment is new, but
    // it paints the same.
    assert!(painted.repainted().contains(&Some(b)));
    assert!(!painted.repainted().contains(&Some(body)));
    assert_eq!(painted.repainted().len(), 2);
}

#[test]
fn a_box_that_grows_moves_those_after_it() {
    let mut page = Page::new(PAGE);
    let (a, b, c) = (page.id("a"), page.id("b"), page.id("c"));
    let painted = page.set_style("b", "height: 80px");
    let repainted = painted.repainted();
    // Moved, so painted again; `#a` and the fixed-height section not.
    assert!(repainted.contains(&Some(b)) && repainted.contains(&Some(c)));
    assert!(!repainted.contains(&Some(a)));
    assert!(!repainted.contains(&Some(page.id("section"))));
}

#[test]
fn changed_text_paints_its_block_again() {
    let mut page = Page::new(PAGE);
    let painted = page.change(|cx, document| {
        let text = cx.first_child(find(cx, document, "s")).unwrap();
        cx.set_data(text, "words");
    });
    let (p, s) = (page.id("p"), page.id("s"));
    let repainted = painted.repainted();
    // The paragraph's lines: its own text, and the span's box and text.
    assert!(repainted.contains(&Some(p)) || repainted.contains(&Some(s)));
    for id in ["a", "b", "c", "section"] {
        assert!(!repainted.contains(&Some(page.id(id))), "#{id}");
    }
}

#[test]
fn another_device_scale_factor_paints_everything_again() {
    let mut page = Page::new(PAGE);
    page.device_scale_factor = 2.0;
    let painted = page.paint();
    // Nothing is taken from a paint snapped to other pixels.
    assert!(painted.artifact.cached.is_none());
}

#[test]
fn the_canvas_taking_another_background_paints_the_box_it_left() {
    let mut page = Page::new(
        "<html id=html><body id=body style='background-color: green'><p>Text</p></body></html>",
    );
    // The root's background goes to the canvas now, and `<body>` paints
    // its own, with the same fragment as before.
    page.set_style("html", "background-color: blue");
    page.set_style("html", "");
}

/// A box of a fixed size that gets lines, or loses them, paints the same
/// itself but not its lines: its items are not taken from the last paint.
#[test]
fn text_coming_or_going_paints_its_box_again() {
    let mut page = Page::new(
        "<div id=d style='width: 100px; height: 50px; font: 20px/1 Ahem; background-color: green'></div>",
    );
    page.change(|cx, document| {
        let text = cx.create_text(document, "Text");
        cx.append_child(find(cx, document, "d"), text).unwrap();
    });
    page.change(|cx, document| cx.remove(cx.first_child(find(cx, document, "d")).unwrap()));
}

/// A container that grows with what is in it paints its background again,
/// though only what is in it changed.
#[test]
fn a_container_that_grows_paints_its_background_again() {
    let mut page = Page::new(
        "<style>html, body { margin: 0 }</style>\
         <div id=outer style='background-color: yellow; padding: 5px'>\
         <div id=inner style='height: 50px; background-color: green'></div></div>",
    );
    let outer = page.id("outer");
    let painted = page.set_style("inner", "height: 80px; background-color: green");
    assert!(painted.repainted().contains(&Some(outer)));
}

/// An image arriving gives its box a new fragment of the same size and
/// style: the image is painted.
#[test]
fn an_image_that_arrives_is_painted() {
    let mut page =
        Page::with_window("<img id=i src=a.png style='display: block; width: 20px; height: 20px'>");
    let id = page.id("i");
    let image = dom::Image::Raster(ImageData::new(&b""[..]), Size::new(20, 20));
    let painted = page.image_arrives(image);
    assert!(painted.repainted().contains(&Some(id)));
}

/// So does an SVG image's, which the fragment has apart from a bitmap
/// (`svg_image`): the box, the size it had while loading (its `width` and
/// `height` attributes), is painted (its background says so: these tests
/// paint no SVG image).
#[test]
fn an_svg_image_that_arrives_is_painted() {
    let mut page = Page::with_window(
        "<img id=i src=a.svg width=20 height=20 style='background-color: green'>",
    );
    let id = page.id("i");
    let image = dom::Image::Svg(
        1,
        SvgNaturalSize {
            width: Some(20.0),
            height: Some(20.0),
            ratio: Some(1.0),
        },
    );
    let painted = page.image_arrives(image);
    assert!(painted.repainted().contains(&Some(id)));
}

/// A scroll container's own background and border are under its box's
/// state, its lines under its content's (clipped and scrolled): taken from
/// the last paint, they go back under the same.
#[test]
fn a_scroll_container_takes_its_items_under_their_states() {
    let mut page = Page::new(
        "<style>html, body { margin: 0 }</style>\
         <div style='width: 100px; height: 50px; overflow: auto; background-color: yellow; \
         border: 2px solid blue; font: 20px/1 Ahem'>Text that overflows the box</div>\
         <div id=other style='height: 20px; background-color: green'></div>",
    );
    page.set_style("other", "height: 20px; background-color: red");
}

/// Items taken from the last paint say so only next to that paint: next to
/// an older one (a layer that wasn't in the last), they are compared.
#[test]
fn copied_items_compare_with_an_older_paint_by_what_they_paint() {
    let mut page = Page::new(PAGE);
    page.set_style("b", "background-color: red");
    page.paint();
    let (first, latest) = (&page.history[0], &page.history[2]);
    assert_eq!(
        latest.0.invalidation(&first.0),
        latest.1.invalidation(&first.1)
    );
    assert!(!latest.0.invalidation(&first.0).is_empty(), "#b changed");
}

/// An inline-block that clips is painted under its own clip, also when the
/// line it is on is painted again.
#[test]
fn an_inline_block_paints_under_its_own_properties_again() {
    let mut page = Page::new(
        "<style>body { margin: 0; font: 20px/1 Ahem }</style>
         <div>A<span style='display: inline-block; width: 30px; overflow: hidden'>BBB</span>C</div>
         <div id=other style='height: 10px'></div>",
    );
    // The paint checks it gives what painting everything does.
    page.set_style("other", "height: 10px; background-color: red");
}

/// Outlines are painted anew each time, around boxes taken from the last
/// paint too: a focus ring that comes and goes rasters again only around
/// its box, not the other box's outline and shadow, the same as before.
#[test]
fn outlines_are_painted_around_boxes_taken_from_the_last_paint() {
    let mut page = Page::new(
        "<style>html, body { margin: 0 }</style>\
         <div id=a style='height: 20px; outline: 2px solid red; box-shadow: 3px 3px blue'></div>\
         <div id=b style='height: 20px'>text</div>",
    );
    // The ring around `b`'s box (y 20 to 40), 3px wide, and its line
    // painted again with the box's new style.
    let around_b = RectF::new(-3.0, 17.0, 806.0, 26.0);
    for style in [
        "height: 20px; outline: auto 1px -webkit-focus-ring-color",
        "height: 20px",
    ] {
        let painted = page.set_style("b", style);
        assert!(!painted.invalidation.is_empty());
        assert!(
            painted
                .invalidation
                .iter()
                .all(|r| around_b.intersect(r) == *r),
            "{:?}",
            painted.invalidation
        );
    }
}

/// A table part paints from where its children are: a table whose caption
/// goes below its grid, or a row that loses a cell, paints again though
/// its style and size are the same.
#[test]
fn table_parts_paint_again_when_their_children_move() {
    let mut page = Page::new(
        "<style>td { padding: 0 }</style>\
         <table id=t style='border: 1px solid; width: 100px'>\
         <caption id=c>Caption</caption>\
         <tr style='background-color: red'><td>A</td><td id=b>B</td></tr></table>",
    );
    page.set_style("c", "caption-side: bottom");
    page.change(|cx, document| cx.remove(find(cx, document, "b")));
}

/// A collapsed table's borders paint again when a cell's border changes,
/// though only its color does.
#[test]
fn collapsed_borders_paint_again() {
    let mut page = Page::new(
        "<table style='border-collapse: collapse'><tr><td id=a style='border: 2px solid red'>A</td>\
           <td>B</td></tr></table>",
    );
    page.set_style("a", "border: 2px solid blue");
    page.set_style("a", "border: 4px dashed blue");
}

/// A control whose state changes paints its native look again, though its
/// style and size are the same.
#[test]
fn a_controls_state_paints_it_again() {
    let mut page = Page::new("<input type=checkbox id=box>");
    let id = page.id("box");
    let painted = page.change(|cx, document| cx.set_checked(find(cx, document, "box"), true));
    assert!(
        painted.repainted().contains(&Some(id)),
        "{:?}",
        painted.repainted()
    );
}

/// A box given a transform (a node of its own) doesn't make another box's
/// nodes other ones: the later box keeps its node's id, and its items.
#[test]
fn a_new_node_keeps_the_other_boxes_ids_and_items() {
    let mut page = Page::new(
        "<style>html, body { margin: 0 } div { width: 100px; height: 50px; background-color: green }</style>\
         <div id=a></div><div id=c style='transform: translateX(2px)'></div>",
    );
    let c = page.id("c");
    let before = page.pre.as_ref().unwrap().trees.transform.len();
    let painted = page.set_style("a", "transform: translateX(1px)");
    assert!(
        !painted.repainted().contains(&Some(c)),
        "{:?}",
        painted.repainted()
    );
    assert_eq!(page.pre.as_ref().unwrap().trees.transform.len(), before + 1);
}

/// A node whose owner is gone leaves its place empty, for a later one to
/// take: a scroll node's is none, which the scroll containers leave out.
#[test]
fn a_gone_nodes_place_is_taken_again() {
    let mut page = Page::new(
        "<style>html, body { margin: 0 } #s { width: 100px; height: 50px; overflow: scroll }</style>\
         <div id=s><div style='height: 200px'></div></div><div id=t style='width: 10px; height: 10px'></div>",
    );
    let trees = |page: &Page| page.pre.as_ref().unwrap().trees.clone();
    assert_eq!(trees(&page).scroll_nodes().count(), 1);
    page.set_style("s", "overflow: visible");
    let gone = trees(&page);
    assert_eq!(gone.scroll.len(), 1, "the place stays");
    assert!(gone.scroll[0].is_none());
    assert_eq!(gone.scroll_nodes().count(), 0);
    let t = page.id("t");
    page.set_style("t", "width: 10px; height: 10px; overflow: scroll");
    let taken = trees(&page);
    assert_eq!(taken.scroll.len(), 1, "the place is taken again");
    assert_eq!(taken.scroll_nodes().next().unwrap().1.element, t);
}

/// Each node comes after its parent.
fn parents_first(trees: &paint::PropertyTrees) {
    for (index, node) in trees.transform.iter().enumerate() {
        assert!(
            node.parent.is_none_or(|parent| parent.0 < index),
            "transform {index}"
        );
    }
    for (index, node) in trees.clip.iter().enumerate() {
        assert!(
            node.parent.is_none_or(|parent| parent.0 < index),
            "clip {index}"
        );
    }
    for (index, node) in trees.effect.iter().enumerate() {
        assert!(
            node.parent.is_none_or(|parent| parent.0 < index),
            "effect {index}"
        );
    }
    for (id, node) in trees.scroll_nodes() {
        assert!(
            node.parent.is_none_or(|parent| parent.0 < id.0),
            "scroll {}",
            id.0
        );
    }
}

/// A free place before a new node's parent's isn't its: it takes one after.
#[test]
fn a_free_place_before_the_parent_isnt_taken() {
    let mut page = Page::new(
        "<style>html, body { margin: 0 } div { width: 10px; height: 10px }</style>\
         <div id=a style='transform: translateX(5px); opacity: 0.5'></div>\
         <div id=p style='transform: translateX(1px); opacity: 0.5'><div id=c></div></div>",
    );
    page.set_style("a", "");
    let freed = page.pre.as_ref().unwrap().trees.transform.len();
    page.set_style("c", "transform: translateX(3px); opacity: 0.5");
    let pre = page.pre.as_ref().unwrap();
    parents_first(&pre.trees);
    assert_eq!(pre.trees.transform.len(), freed + 1, "a new place");
    let c = pre.trees.transform.len() - 1;
    assert_eq!(
        pre.trees.to_root()[c]
            .map_point(gfx::PointF::new(0.0, 0.0))
            .x,
        4.0
    );
}

/// A box given a node after a box in it had one: the child's node takes a
/// place after the new one, is moved by it, and keeps the new place.
/// (Debug builds check every pre-paint against one from scratch.)
#[test]
fn a_node_new_around_another_moves_it_after_it() {
    let mut page = Page::new(
        "<style>html, body { margin: 0 }</style>\
         <div id=p><div id=c style='transform: translateX(3px); width: 10px; height: 10px; background-color: green'></div></div>",
    );
    let before = page.pre.as_ref().unwrap().trees.transform.len();
    page.set_style("p", "transform: translateX(1px)");
    let pre = page.pre.as_ref().unwrap();
    parents_first(&pre.trees);
    assert_eq!(
        pre.trees.transform.len(),
        before + 2,
        "the parent's, then the child's"
    );
    let child = pre.trees.transform.len() - 1;
    assert_eq!(
        pre.trees.to_root()[child]
            .map_point(gfx::PointF::new(0.0, 0.0))
            .x,
        4.0
    );
    let c = page.id("c");
    let painted = page.set_style(
        "c",
        "transform: translateX(3px); width: 10px; height: 10px; background-color: blue",
    );
    assert_eq!(painted.repainted(), BTreeSet::from([Some(c)]));
    let pre = page.pre.as_ref().unwrap();
    assert_eq!(pre.trees.transform.len(), child + 1, "no other place");
    assert_eq!(
        pre.trees.to_root()[child]
            .map_point(gfx::PointF::new(0.0, 0.0))
            .x,
        4.0
    );
}

/// A scroll container made around one that was already: the inner one's
/// nodes take places after its, and the layers find which container moves
/// what.
#[test]
fn an_outer_scroll_container_moves_an_inner_one_after_it() {
    let mut page = Page::new(
        "<style>html, body { margin: 0 } #inner { width: 100px; height: 50px; overflow: scroll }</style>\
         <div id=outer style='width: 200px; height: 100px'>\
         <div id=inner><div style='height: 200px; background-color: green'></div></div>\
         <div style='height: 300px'></div></div>",
    );
    page.set_style("outer", "width: 200px; height: 100px; overflow: scroll");
    let (outer, inner) = (page.id("outer"), page.id("inner"));
    let pre = page.pre.as_ref().unwrap();
    parents_first(&pre.trees);
    let nodes: Vec<_> = pre.trees.scroll_nodes().collect();
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].1.element, outer, "the outer one first");
    assert_eq!(nodes[1].1.element, inner);
    assert_eq!(nodes[1].1.parent, Some(nodes[0].0));
}

/// A tree with more empty places than nodes (at least 32) starts again
/// the next pre-paint: nodes new under one at its end would only make it
/// longer. A few empty places stay.
#[test]
fn a_mostly_empty_tree_starts_again() {
    let list = |count: usize| "<div style='transform: translateX(1px)'></div>".repeat(count);
    for (count, starts_again) in [(10, false), (40, true)] {
        let mut page = Page::new(&format!(
            "<style>html, body {{ margin: 0 }} div {{ width: 10px; height: 10px }}</style>\
             <section id=list>{}</section><div id=other></div>",
            list(count)
        ));
        let transforms = |page: &Page| page.pre.as_ref().unwrap().trees.transform.len();
        let full = transforms(&page);
        page.set_style("list", "display: none");
        assert_eq!(transforms(&page), full, "the places stay");
        page.set_style("other", "background-color: green");
        let after = transforms(&page);
        if starts_again {
            assert_eq!(after, 1, "only the root's");
        } else {
            assert_eq!(after, full);
        }
    }
}

/// A fragment layout left as it was, in the same place under the same
/// nodes, isn't walked again, nor is what is in it: another box's change
/// walks it and its ancestors.
#[test]
fn an_unchanged_subtree_isnt_walked_again() {
    let mut page = Page::new(&format!(
        "<style>html, body {{ margin: 0 }} p {{ margin: 0; opacity: 0.5 }}</style>\
         <div id=list>{}</div><div id=other style='width: 10px; height: 10px'></div>",
        "<p>item</p>".repeat(20)
    ));
    let all = page.pre.as_ref().unwrap().walked_for_testing();
    assert!(all > 20, "{all}");
    page.set_style("other", "width: 20px; height: 10px");
    let walked = page.pre.as_ref().unwrap().walked_for_testing();
    assert!(walked < 8, "{walked} of {all}");
}

/// Scrolling a container sets its scroll translation in place: nothing is
/// walked again. (Debug builds check it against a pre-paint from scratch.)
#[test]
fn scrolling_walks_nothing_again() {
    let mut page = Page::new(
        "<style>html, body { margin: 0 } #s { width: 100px; height: 50px; overflow: scroll }</style>\
         <div id=s><div style='height: 200px; background-color: green'></div></div>",
    );
    let s = page.id("s");
    page.scroll.insert(s, gfx::PointF::new(0.0, 30.0));
    page.paint();
    let pre = page.pre.as_ref().unwrap();
    assert_eq!(pre.walked_for_testing(), 0);
    let (_, node) = pre.trees.scroll_nodes().next().unwrap();
    assert_eq!(
        pre.trees.transform[node.scroll_translation.0].matrix,
        gfx::Transform::translate(0.0, -30.0)
    );
}

/// A box whose fragment moved walks again, and all in it, at its new
/// place; one whose ancestor's node changed doesn't (its nodes are the
/// same).
#[test]
fn a_moved_box_walks_again_but_not_under_a_changed_node() {
    let mut page = Page::new(
        "<style>html, body { margin: 0 } div { width: 10px; height: 10px }</style>\
         <div id=first></div><div id=t style='transform: translateX(1px)'><div id=in></div></div>",
    );
    page.set_style("first", "width: 10px; height: 30px");
    let moved = page.pre.as_ref().unwrap().walked_for_testing();
    page.set_style("t", "transform: translateX(5px)");
    let under = page.pre.as_ref().unwrap().walked_for_testing();
    assert!(moved > under, "{moved} {under}");
}

/// A box whose own fragment changed walks again, but its children that
/// didn't aren't: they are found by their box's last fragment's.
#[test]
fn a_changed_box_keeps_its_unchanged_children() {
    let mut page = Page::new(&format!(
        "<style>html, body {{ margin: 0 }} p {{ margin: 0; opacity: 0.5 }}</style>\
         <div id=list>{}</div>",
        "<p>item</p>".repeat(20)
    ));
    page.set_style("list", "background-color: green");
    let walked = page.pre.as_ref().unwrap().walked_for_testing();
    assert!(walked < 8, "{walked}");
}

/// What a box gone from the tree had is let go of: its nodes, and its SVG
/// content's states. (Debug builds check the nodes and states left are
/// those of a pre-paint from scratch.)
#[test]
fn a_gone_boxs_nodes_and_svg_states_are_let_go_of() {
    let mut page = Page::new(
        "<style>html, body { margin: 0 }</style>\
         <div id=keep style='opacity: 0.5; width: 10px; height: 10px'></div>\
         <div id=gone style='transform: translateX(1px); overflow: scroll; width: 10px; height: 10px'>\
         <svg width=10 height=10><g opacity=0.5 transform='translate(1 1)'><rect width=5 height=5 fill=green /></g></svg></div>",
    );
    let full = page.pre.as_ref().unwrap().trees.scroll_nodes().count();
    assert_eq!(full, 1);
    page.change(|cx, document| cx.remove(find(cx, document, "gone")));
    assert_eq!(page.pre.as_ref().unwrap().trees.scroll_nodes().count(), 0);
}

/// A stacking context nothing in which changed takes all its items from
/// the last paint, without its boxes being walked; one with a change in it
/// is painted, its boxes taking what they can.
#[test]
fn an_unchanged_stacking_context_is_taken_whole() {
    let mut page = Page::new(&format!(
        "<style>html, body {{ margin: 0 }} p {{ margin: 0 }}</style>\
         <div id=context style='opacity: 0.5'>{}<p id=last>last</p></div>\
         <div id=other style='width: 10px; height: 10px; background-color: green'></div>",
        "<p>item</p>".repeat(20)
    ));
    page.set_style("other", "width: 10px; height: 10px; background-color: blue");
    let (walked, taken) = page.controller.walked_for_testing();
    assert!(taken >= 1 && walked < 10, "{walked} {taken}");
    let last = page.id("last");
    let painted = page.set_style("last", "color: blue");
    assert!(painted.repainted().contains(&Some(last)));
    let (walked, _) = page.controller.walked_for_testing();
    assert!(walked > 20, "{walked}");
}

/// An out-of-flow box is painted in the stacking context its parent is in,
/// under its effect, though its containing block, outside, holds it. Moved
/// by its containing block (taller for what comes after), the stacking
/// contexts it is in aren't taken whole. Walked again alone (another clip
/// around its containing block, those around its parent kept by an
/// `overflow` clip), twice, it keeps the effect its parent last gave it, and
/// the stacking contexts it is in, not walked, aren't taken whole either;
/// its parent's effect
/// gone, it is walked again under the next one. (The harness checks the
/// paint against painting everything; debug builds, pre-paint against
/// walking everything, opacity and all.)
#[test]
fn an_out_of_flow_box_paints_under_its_parents_effect_again() {
    let mut page = Page::new(&format!(
        "<style>html, body {{ margin: 0 }} #escaped {{ position: absolute; bottom: 0; width: 10px;
           height: 10px; background-color: green }}</style>
         {}
         <div id=around><div style='position: relative'>
           <div style='overflow: hidden'><div id=faded style='opacity: 0.5'>
             <div id=inner style='opacity: 0.5'><div><div id=escaped></div></div></div>
           </div></div>
           <div id=spacer style='height: 100px'></div>
         </div></div>",
        "<p>item</p>".repeat(20)
    ));
    let escaped = page.id("escaped");
    for height in [200, 300] {
        let painted = page.set_style("spacer", &format!("height: {height}px"));
        assert!(painted.repainted().contains(&Some(escaped)));
        assert_eq!(
            page.controller.walked_for_testing().1,
            0,
            "none taken whole"
        );
    }
    for clip in ["clip-path: inset(1px)", ""] {
        page.set_style("around", clip);
    }
    // Another fragment for it: the last one's effect let go of.
    let painted = page.set_style("escaped", "background-color: lime");
    assert!(painted.repainted().contains(&Some(escaped)));
    page.set_style("inner", "");
    page.set_style("faded", "");
}

/// What hit testing finds in a child frame goes into the next paint with
/// what its document paints only while hit testing goes into it: none of it
/// while its iframe is `pointer-events: none`, all of it again after. (The
/// harness checks the targets against painting everything; debug builds,
/// against what hit testing finds.)
#[test]
fn a_child_frames_targets_follow_its_iframes_pointer_events() {
    let mut page = Page::new(
        "<style>html, body { margin: 0 }</style>\
         <iframe id=f style='display: block; border: 0'></iframe>",
    );
    let shown = page.set_child_frame(
        "f",
        "<style>body { margin: 0 }</style><div style='height: 20px'>text</div>",
    );
    let all = shown.artifact.hit_test.rects.len();
    page.paint();
    let taken = |page: &Page| page.controller.walked_for_testing().1;
    assert_eq!(taken(&page), 1, "the child frame's document whole");
    let none = page.set_style("f", "display: block; border: 0; pointer-events: none");
    // The iframe, and in it the canvas, `<html>`, `<body>`, the `<div>`
    // and its text.
    assert_eq!(none.artifact.hit_test.rects.len(), all - 6);
    assert_eq!(taken(&page), 0);
    let again = page.set_style("f", "display: block; border: 0");
    assert_eq!(again.artifact.hit_test.rects.len(), all);
    assert_eq!(taken(&page), 0);
}

/// A blocking `wheel` listener coming paints the boxes it is over again,
/// their targets now inside it; the stacking context beside them is taken
/// whole. Going, the same back.
#[test]
fn a_wheel_listener_paints_again_the_boxes_it_is_over() {
    let mut page = Page::new(
        "<style>html, body { margin: 0 } p { margin: 0 }</style>\
         <div id=context style='opacity: 0.5'><p>a</p><p>b</p></div>\
         <div id=listened style='position: relative; z-index: 0'><p>x</p></div>",
    );
    let inside = |painted: &Painted| -> usize {
        let data = &painted.artifact.hit_test;
        data.chunks
            .iter()
            .filter(|chunk| chunk.wheel)
            .map(|chunk| chunk.rects.len())
            .sum()
    };
    let added = page.change(|cx, document| {
        let listened = TargetRef::Node(find(cx, document, "listened"));
        cx.add_listener(listened, names::event::WHEEL, Default::default());
    });
    // The `<div>`, the `<p>` and its text.
    assert_eq!(inside(&added), 3);
    assert_eq!(page.controller.walked_for_testing().1, 1, "the other one");
    let removed = page.change(|cx, document| {
        let listened = TargetRef::Node(find(cx, document, "listened"));
        let listener = cx.listeners(listened).unwrap().get(cx.gc, 0).unwrap();
        cx.remove_listener(listened, listener);
    });
    assert_eq!(inside(&removed), 0);
}

/// Painted again after a change, the layer takes whole each chunk painted
/// as it was (its items copied, in order, from one chunk, under the same
/// state), comparing none of its items; the chunk with the change is
/// compared item by item. (The harness checks what it rasters again is
/// what comparing every item gives.)
#[test]
fn the_chunks_painted_as_they_were_are_taken_whole() {
    let mut page = Page::new(
        "<style>html, body { margin: 0 } p { margin: 0 }</style>\
         <div style='opacity: 0.5'><p>a</p><p>b</p></div>\
         <div style='transform: translateX(1px)'><p>c</p></div>\
         <div id=other style='width: 10px; height: 10px; background-color: green'></div>",
    );
    let painted = page.set_style("other", "width: 10px; height: 10px; background-color: blue");
    assert_eq!(
        painted
            .invalidation
            .iter()
            .map(|r| r.size())
            .collect::<Vec<_>>(),
        [SizeF::new(10.0, 10.0)],
        "only the box"
    );
    let (taken, chunks) = page.layer.as_ref().unwrap().0.chunks_taken_for_testing();
    assert_eq!((taken, chunks), (2, 3), "the two stacking contexts'");
}

/// A chunk is taken whole only if all its items are copied, in order, from
/// one chunk under the same state: not if one of them is painted anew, one
/// went from its end, or its opacity changed. (The harness checks each
/// against comparing every item.)
#[test]
fn a_chunk_changed_anyhow_isnt_taken_whole() {
    let mut page = Page::new(
        "<style>html, body { margin: 0 } p { margin: 0; width: 50px; height: 10px }</style>\
         <div id=context style='opacity: 0.5'><p style='background-color: green'></p>\
         <p id=second style='background-color: green'></p><p id=last style='background-color: green'></p></div>",
    );
    let repainted = page.set_style("second", "background-color: blue");
    assert_eq!(repainted.invalidation.len(), 1);
    let removed = page.change(|cx, document| cx.remove(find(cx, document, "last")));
    assert_eq!(removed.invalidation.len(), 1);
    let fainter = page.set_style("context", "opacity: 0.4");
    // Both boxes, where they were and where they are.
    assert_eq!(fainter.invalidation.len(), 4);
}

/// Scrolling (its matrices only) and an SVG's content take a stacking
/// context whole too.
#[test]
fn a_scrolled_stacking_context_with_svg_is_taken_whole() {
    let mut page = Page::new(
        "<style>html, body { margin: 0 } #s { width: 100px; height: 50px; overflow: scroll; position: relative }</style>\
         <div id=s><svg width=10 height=10><rect width=5 height=5 fill=green /></svg><div style='height: 200px'></div></div>",
    );
    let s = page.id("s");
    page.scroll.insert(s, gfx::PointF::new(0.0, 30.0));
    page.paint();
    let (walked, taken) = page.controller.walked_for_testing();
    assert_eq!((walked, taken), (0, 1));
}

/// A stacking context whose box gave the canvas its background, or now
/// does, paints again: whether its background is painted changed.
#[test]
fn the_canvas_backgrounds_source_changing_paints_a_stacking_context_again() {
    let mut page = Page::new(
        "<html id=root><body id=body style='opacity: 0.5; background-color: green; margin: 0'>\
         <div style='width: 10px; height: 10px'></div></body></html>",
    );
    let body = page.id("body");
    let painted = page.set_style("root", "background-color: blue");
    assert!(
        painted.repainted().contains(&Some(body)),
        "{:?}",
        painted.repainted()
    );
}

/// An element to paint anew paints the stacking contexts it is in again;
/// another is taken whole.
#[test]
fn an_invalidated_element_paints_only_its_stacking_contexts_again() {
    let mut page = Page::new(
        "<style>html, body { margin: 0 } div { opacity: 0.5 }</style>\
         <div><p id=a>a</p></div><div><p>b</p></div>",
    );
    let a = page.id("a");
    page.controller.invalidate([a]);
    let painted = page.paint();
    assert!(painted.repainted().contains(&Some(a)));
    assert_eq!(
        page.controller.walked_for_testing().1,
        1,
        "the other taken whole"
    );
}

/// A stacking context in one taken whole is taken whole again when the
/// outer one paints anew.
#[test]
fn a_stacking_context_in_one_taken_whole_is_taken_whole_again() {
    let mut page = Page::new(
        "<style>html, body { margin: 0 } p { margin: 0 }</style>\
         <div style='position: relative; z-index: 0'><div style='position: relative; z-index: 0'>\
         <p>in</p></div><p id=out>out</p></div>\
         <div id=other style='width: 10px; height: 10px; background-color: green'></div>",
    );
    page.set_style("other", "width: 10px; height: 10px; background-color: blue");
    page.set_style("out", "color: blue");
    assert_eq!(page.controller.walked_for_testing().1, 1, "the inner one");
}

/// A stacking context taken whole after a target came before it (an
/// element no longer hidden, which paints nothing) has its targets where
/// they now are, and is taken whole from there again.
#[test]
fn a_stacking_context_taken_whole_after_a_new_target_is_taken_whole_again() {
    let mut page = Page::new(
        "<style>html, body { margin: 0 } p { margin: 0 }</style>\
         <div id=before style='height: 10px; visibility: hidden'></div>\
         <div style='position: relative; z-index: 0'><p>in</p></div>\
         <div id=other style='width: 10px; height: 10px; background-color: green'></div>",
    );
    let shown = page.set_style("before", "height: 10px");
    assert_eq!(page.controller.walked_for_testing().1, 1);
    let blue = page.set_style("other", "width: 10px; height: 10px; background-color: blue");
    assert_eq!(page.controller.walked_for_testing().1, 1);
    assert_eq!(blue.artifact.hit_test, shown.artifact.hit_test);
}

/// A stacking context taken whole into a new one around it is in that one:
/// an element in it to paint anew paints the new one again.
#[test]
fn an_element_to_paint_anew_paints_the_new_stacking_context_around_again() {
    let mut page = Page::new(
        "<style>html, body { margin: 0 }</style>\
         <div id=outer style='position: relative; z-index: 0'>\
         <div style='position: relative; z-index: 0'><p id=p>p</p></div></div>",
    );
    page.set_style(
        "outer",
        "position: relative; z-index: 0; background-color: green",
    );
    let p = page.id("p");
    page.controller.invalidate([p]);
    let painted = page.paint();
    assert!(
        painted.repainted().contains(&Some(p)),
        "{:?}",
        painted.repainted()
    );
}

/// Pre-paints from scratch give other ids: a stacking context isn't taken
/// whole from a paint under another one's nodes.
#[test]
fn a_pre_paint_from_scratch_takes_no_stacking_context_whole() {
    let mut page = Page::new(
        "<style>html, body { margin: 0 }</style>\
         <div id=a style='width: 10px; height: 10px'></div>\
         <div style='opacity: 0.5'><p style='opacity: 0.5'>p</p></div>",
    );
    page.fresh_pre_paints = true;
    page.set_style("a", "width: 10px; height: 10px; opacity: 0.5");
    assert_eq!(page.controller.walked_for_testing().1, 0);
}

/// A box that stops being the containing block of a fixed box in it (its
/// `transform` gone) has it where it was in tree order, though the viewport
/// now holds it: with the fragment the viewport's layout made last, after
/// the box's own, which it had made again for it.
#[test]
fn a_fixed_box_its_containing_block_lets_go_of_paints_with_it() {
    let mut page = Page::new(
        "<style>html, body { margin: 0 } #t { position: absolute; top: 100px; left: 100px }
           #f { position: fixed; top: 150px; left: 150px; width: 10px; height: 10px;
             background-color: green }</style>
         <body><div id=t style='transform: translateX(4px)'><div id=f></div></div>",
    );
    let f = page.id("f");
    let painted = page.set_style("t", "");
    assert!(painted.repainted().contains(&Some(f)));
}

/// A selection that changes paints again the boxes with the text it left
/// or took, as the renderer invalidates them (a text node's parent, a
/// `<br>` itself), and nothing else: text right in a block, in an inline
/// element, in an anonymous block, a `<br>` ending a line. (The harness
/// checks the paint against painting everything.)
#[test]
fn a_selection_paints_again_what_it_leaves_and_takes() {
    let mut page = Page::new(
        "<style>html, body { margin: 0 } p { margin: 0; font: 20px/1 Ahem }</style>\
         <p id=a>aa</p><p id=b>b<span id=s>s</span><br id=br>b</p>\
         <div id=d style='font: 20px/1 Ahem'>d<p>block</p>d</div><p id=c>c</p>",
    );
    let (a, s, d) = (page.text_of("a"), page.text_of("s"), page.text_of("d"));
    let (b, br) = (page.id("b"), page.id("br"));
    // Each step: what is selected (a node, the element the renderer
    // invalidates for it, the part), and the elements painted again.
    type Selected<'a> = &'a [(NodeId, NodeId, std::ops::Range<u32>)];
    let steps: [(Selected<'_>, &[NodeId]); 5] = [
        (&[(a.0, a.1, 0..1)], &[a.1]),
        // The block with the `<br>`, and the text in it.
        (&[(br, br, 0..1)], &[a.1, b, s.1]),
        (&[(s.0, s.1, 0..u32::MAX), (br, br, 0..1)], &[b, s.1]),
        (&[(d.0, d.1, 0..1)], &[b, s.1, d.1]),
        (&[], &[d.1]),
    ];
    let mut last: Vec<NodeId> = Vec::new();
    for (selected, repainted) in steps {
        *page.selected.0.borrow_mut() = selected
            .iter()
            .map(|(node, _, range)| {
                let range = range.clone();
                (
                    *node,
                    paint::TextSelection {
                        range,
                        active: true,
                    },
                )
            })
            .collect();
        let now: Vec<NodeId> = selected.iter().map(|&(_, element, _)| element).collect();
        page.controller.invalidate(last.iter().chain(&now).copied());
        let painted = page.paint();
        let want: BTreeSet<Option<NodeId>> = repainted.iter().map(|&e| Some(e)).collect();
        assert_eq!(painted.repainted(), want, "{selected:?}");
        last = now;
    }
}

/// Painting places glyphs, not clusters: only selected text has its
/// clusters worked out (where the selection is, and where its lines end:
/// all of a selected text node's), none of the rest.
#[test]
fn only_selected_text_has_its_clusters_worked_out() {
    let mut page = Page::new(
        "<style>html, body { margin: 0 } p { margin: 0; font: 20px/1 Ahem; width: 100px }</style>\
         <p id=a>aa aa aa</p><p id=b>bb bb bb</p>",
    );
    let (text, _) = page.text_of("a");
    let (a, b) = (page.id("a"), page.id("b"));
    page.paint();
    let selection = paint::TextSelection {
        range: 0..4,
        active: true,
    };
    *page.selected.0.borrow_mut() = [(text, selection)].into();
    page.controller.invalidate([a]);
    page.paint();
    fn made(fragment: &layout::BoxFragment, out: &mut Vec<(Option<NodeId>, bool)>) {
        for line in &fragment.lines {
            out.extend(line.runs.iter().map(|run| (run.node, run.clusters_made())));
        }
        for child in &fragment.children {
            made(&child.fragment, out);
        }
    }
    let mut runs = Vec::new();
    made(&page.page.lay_out(VIEWPORT), &mut runs);
    // "aa aa" and "aa" on two lines, the selection in the first (whether
    // it reaches each line's end takes the line's); "bb" alike, of a text
    // node unselected.
    let of = |node| -> Vec<bool> {
        runs.iter()
            .filter(|&&(n, _)| n == Some(node))
            .map(|&(_, made)| made)
            .collect()
    };
    assert_eq!(of(a), [true, true], "{runs:?}");
    assert_eq!(of(b), [false, false], "{runs:?}");
}

/// The caret blinking paints only the caret again (its own display item
/// client): the boxes of its block take all they painted, though its
/// stacking context is walked to it, not taken whole; one that moves to
/// another block paints there. (The harness checks the paint against
/// painting everything.)
#[test]
fn a_blinking_caret_paints_only_itself_again() {
    let mut page = Page::new(
        "<style>html, body { margin: 0 } p { margin: 0; font: 20px/1 Ahem }</style>\
         <p id=a>aaaa</p><p id=b>bb</p><div style='opacity: 0.5'><p>far</p></div>",
    );
    let (a_text, a) = page.text_of("a");
    let (b_text, b) = page.text_of("b");
    let caret = |text, block| paint::Caret {
        at: paint::CaretPosition::Text { text, offset: 2 },
        block,
    };
    let caret_items = |painted: &Painted| {
        painted
            .artifact
            .items
            .iter()
            .filter(|item| item.kind == paint::DisplayItemKind::Caret)
            .count()
    };
    let blink = |page: &mut Page, shown: Option<paint::Caret>, blocks: &[NodeId]| {
        page.selected.1.set(shown);
        page.controller.invalidate_caret(blocks.iter().copied());
        page.paint()
    };
    let shown = blink(&mut page, Some(caret(a_text, a)), &[a]);
    assert_eq!(caret_items(&shown), 1);
    assert_eq!(shown.repainted(), [Some(a)].into(), "the caret alone");
    let hidden = blink(&mut page, None, &[a]);
    assert_eq!(caret_items(&hidden), 0);
    assert!(hidden.repainted().is_empty(), "nothing painted anew");
    let (_, whole) = page.controller.walked_for_testing();
    assert_eq!(whole, 1, "the stacking context away from it taken whole");
    let shown = blink(&mut page, Some(caret(a_text, a)), &[a]);
    assert_eq!(shown.repainted(), [Some(a)].into());
    let moved = blink(&mut page, Some(caret(b_text, b)), &[a, b]);
    assert_eq!(caret_items(&moved), 1);
    assert_eq!(moved.repainted(), [Some(b)].into());
    // Not invalidated: the last paint's caret, the stacking context whole.
    page.paint();
    assert_eq!(page.controller.walked_for_testing().0, 0, "nothing walked");
}

/// What gone fragments painted is let go of as a whole: a relayout of
/// everything leaves no more than twice the boxes in the tree kept, a
/// pruning only those in it, and what is kept still paints the next time.
#[test]
fn what_gone_fragments_painted_is_let_go_of() {
    // Over a thousand boxes, nested three deep: a gone tree's boxes are
    // held by their gone parents.
    let boxes = "<div><div><div class=leaf></div></div></div>".repeat(400);
    let mut page = Page::new(&format!(
        "<style>html, body {{ margin: 0 }} div {{ padding: 1px }}          .leaf {{ height: 1px; background-color: green }}</style>         <div id=w style='width: 500px'>{boxes}</div>         <div style='opacity: 0.5'><p>far</p></div>         <div id=t style='height: 10px; background-color: green'></div>"
    ));
    let live = page.controller.kept_for_testing();
    assert!(live.0 > 1200, "{live:?} kept");
    let mut last = live.0;
    let mut pruned = false;
    for width in [400, 500].repeat(10) {
        // Every box in `#w` gets a new fragment, and the root's.
        page.set_style("w", &format!("width: {width}px"));
        let kept = page.controller.kept_for_testing();
        assert!(kept.0 <= 2 * live.0, "{kept:?} kept of {live:?}");
        if kept.0 < last {
            assert_eq!(kept, live, "those in the tree, after a pruning");
            pruned = true;
            break;
        }
        last = kept.0;
    }
    assert!(pruned, "pruned");
    // Painted just after a pruning: the boxes it kept paint the same.
    let t = page.id("t");
    page.controller.invalidate([t]);
    let painted = page.paint();
    assert!(painted.repainted().contains(&Some(t)));
    assert!(painted.repainted().len() <= 2, "{:?}", painted.repainted());
    let (_, whole) = page.controller.walked_for_testing();
    assert_eq!(whole, 1, "the stacking context away from it taken whole");
}
