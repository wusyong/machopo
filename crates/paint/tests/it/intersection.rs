//! Intersection Observer's geometry (`paint::intersections`): a target's
//! rect, its part in the root and the root's rect, from pre-paint's
//! property trees, through scroll offsets, transforms, scroll containers'
//! clips, iframes' content boxes, and with the root's and scroll margins.

use std::sync::Arc;

use gfx::{MarginLength, Margins, NodeId, PointF, RectF, SizeF};
use layout::BoxFragment;
use paint::{
    ChildFrames, IntersectionGeometry, IntersectionRequest, IntersectionRoot, PrePaint,
    ScrollOffsets,
};

use crate::page::Frames;

const VIEWPORT: SizeF = SizeF::new(400.0, 300.0);

const NONE: Margins = [MarginLength::Px(0.0); 4];

/// The scroll offsets, by id.
fn offsets(frames: &Frames, scroll: &[(&str, f32, f32)]) -> ScrollOffsets {
    scroll
        .iter()
        .map(|(id, x, y)| (frames.id(id), PointF::new(*x, *y)))
        .collect()
}

impl Frames {
    /// Whether `node` is `target` or in it.
    fn contains(&self, target: NodeId, node: NodeId) -> bool {
        std::iter::successors(Some(node), |n| self.parents.get(n).copied()).any(|n| n == target)
    }

    /// The geometry of `requests`, with what `view` painted as `pre`
    /// placed it.
    fn intersections(
        &self,
        view: &Arc<BoxFragment>,
        pre: &PrePaint,
        requests: &[IntersectionRequest],
    ) -> Vec<IntersectionGeometry> {
        let artifact = paint::paint(view, pre, layout::document_size(view), &());
        let contains = |target, node| self.contains(target, node);
        paint::intersections(
            view,
            pre,
            self.document(view),
            VIEWPORT,
            requests,
            Some(&paint::Occlusion {
                artifact: &artifact,
                contains: &contains,
            }),
        )
    }
}

fn request(target: NodeId, root: IntersectionRoot) -> IntersectionRequest {
    IntersectionRequest {
        target,
        root,
        root_margin: Some(NONE),
        scroll_margin: NONE,
        visibility: false,
    }
}

/// The geometry of each request, in a page `html` scrolled by
/// `viewport_scroll` and `scroll` (by id).
struct Page {
    frames: Frames,
    view: Arc<BoxFragment>,
    pre: PrePaint,
}

impl Page {
    fn new(html: &str, viewport_scroll: PointF, scroll: &[(&str, f32, f32)]) -> Self {
        let mut frames = Frames::new();
        let view = frames.lay_out(html, VIEWPORT);
        let pre = paint::pre_paint_frame_tree(
            &view,
            &offsets(&frames, scroll),
            &Default::default(),
            &ChildFrames::default(),
            1.0,
            viewport_scroll,
            None,
        );
        Self { frames, view, pre }
    }

    fn id(&self, id: &str) -> NodeId {
        self.frames.id(id)
    }

    fn geometry(&self, requests: &[IntersectionRequest]) -> Vec<IntersectionGeometry> {
        self.frames.intersections(&self.view, &self.pre, requests)
    }

    /// `target`'s with the implicit root and no margins.
    fn implicit(&self, target: &str) -> IntersectionGeometry {
        self.geometry(&[request(self.id(target), IntersectionRoot::Implicit)])[0]
    }
}

fn rect(x: f32, y: f32, width: f32, height: f32) -> Option<RectF> {
    Some(RectF::new(x, y, width, height))
}

/// The implicit root is the viewport, scrolled or not; a target is cut to
/// it, meets it with an edge (an empty rect), or misses it.
#[test]
fn the_viewport_as_the_root() {
    let html = "<style>body { margin: 0 } div { width: 100px; height: 100px }</style>\
        <div style='height: 250px'></div><div id=a></div>\
        <div id=b style='margin-top: 0'></div><div style='height: 50px'></div>\
        <div id=c></div>";
    // a: 250..350, b: 350..450, c: 500..600.
    let page = Page::new(html, PointF::new(0.0, 0.0), &[]);
    let viewport = rect(0.0, 0.0, 400.0, 300.0);
    assert_eq!(
        page.implicit("a"),
        IntersectionGeometry {
            target: rect(0.0, 250.0, 100.0, 100.0),
            intersection: rect(0.0, 250.0, 100.0, 50.0),
            root: viewport,
            visible: false,
        }
    );
    let b = page.implicit("b");
    assert_eq!(b.intersection, None);
    // The document as the root: its viewport too.
    let document = page.frames.document(&page.view);
    let a = page.geometry(&[request(page.id("a"), IntersectionRoot::Document(document))])[0];
    assert_eq!(a.root, viewport);
    assert_eq!(a.intersection, rect(0.0, 250.0, 100.0, 50.0));
    // Scrolled by 50: `b`'s top edge is on the viewport's bottom one.
    let page = Page::new(html, PointF::new(0.0, 50.0), &[]);
    let b = page.implicit("b");
    assert_eq!(b.target, rect(0.0, 300.0, 100.0, 100.0));
    assert_eq!(b.intersection, rect(0.0, 300.0, 100.0, 0.0));
    assert_eq!(b.root, viewport);
    assert_eq!(page.implicit("c").intersection, None);
    // An element with no box has no rect, and meets nothing.
    let page = Page::new(
        "<div id=none style='display: none'></div>",
        PointF::new(0.0, 0.0),
        &[],
    );
    assert_eq!(
        page.implicit("none"),
        IntersectionGeometry {
            target: None,
            intersection: None,
            root: viewport,
            visible: false,
        }
    );
}

/// The root's margin grows (or shrinks) its rect, percentages of its
/// height and width; a target in a document it doesn't apply to gets the
/// rect as it is.
#[test]
fn the_roots_margin() {
    let page = Page::new(
        "<style>body { margin: 0 }</style><div style='height: 320px'></div>\
         <div id=t style='width: 10px; height: 10px'></div>",
        PointF::new(0.0, 0.0),
        &[],
    );
    let t = page.id("t");
    let with = |margin: Margins| {
        let mut request = request(t, IntersectionRoot::Implicit);
        request.root_margin = Some(margin);
        page.geometry(&[request])[0]
    };
    let grown = with([
        MarginLength::Percent(5.0),
        MarginLength::Percent(10.0),
        MarginLength::Percent(10.0),
        MarginLength::Px(-5.0),
    ]);
    // 10% of 300 at the bottom: down to 330, over the target at 320..330;
    // 5% of it at the top, 10% of 400 at the right.
    assert_eq!(grown.root, rect(5.0, -15.0, 435.0, 345.0));
    assert_eq!(grown.intersection, rect(5.0, 320.0, 5.0, 10.0));
    let shrunk = with([MarginLength::Px(-10.0); 4]);
    assert_eq!(shrunk.root, rect(10.0, 10.0, 380.0, 280.0));
    let mut no_margin = request(t, IntersectionRoot::Implicit);
    no_margin.root_margin = None;
    assert_eq!(
        page.geometry(&[no_margin])[0].root,
        rect(0.0, 0.0, 400.0, 300.0)
    );
}

/// A scroll container between the target and the root cuts it to its
/// padding box, scrolled; the scroll margin grows that clip, not the
/// root's.
#[test]
fn a_scroll_container_and_the_scroll_margin() {
    let html = "<style>body { margin: 0 }</style>\
        <div id=s style='overflow: hidden; width: 100px; height: 100px; border: 10px solid; \
          margin: 20px'>\
          <div style='height: 150px'></div>\
          <div id=t style='width: 50px; height: 50px'></div>\
          <div style='height: 500px'></div>\
        </div>";
    // The padding box: (30, 30) 100×100; `t` at 30 + 150 less the scroll.
    let page = Page::new(html, PointF::new(0.0, 0.0), &[("s", 0.0, 120.0)]);
    let t = page.implicit("t");
    assert_eq!(t.target, rect(30.0, 60.0, 50.0, 50.0));
    assert_eq!(t.intersection, rect(30.0, 60.0, 50.0, 50.0));
    let page = Page::new(html, PointF::new(0.0, 0.0), &[("s", 0.0, 60.0)]);
    let t = page.implicit("t");
    assert_eq!(t.target, rect(30.0, 120.0, 50.0, 50.0));
    assert_eq!(t.intersection, rect(30.0, 120.0, 50.0, 10.0));
    let page = Page::new(html, PointF::new(0.0, 0.0), &[]);
    assert_eq!(page.implicit("t").intersection, None);
    // 60px more below the clip (at 130): 10px of the target, whose top is
    // at 180.
    let mut with_margin = request(page.id("t"), IntersectionRoot::Implicit);
    with_margin.scroll_margin = [MarginLength::Px(60.0); 4];
    let t = page.geometry(&[with_margin])[0];
    assert_eq!(t.intersection, rect(30.0, 180.0, 50.0, 10.0));
    assert_eq!(t.root, rect(0.0, 0.0, 400.0, 300.0));
    // A percentage is of the clip's own size.
    with_margin.scroll_margin = [MarginLength::Percent(60.0); 4];
    assert_eq!(
        page.geometry(&[with_margin])[0].intersection,
        rect(30.0, 180.0, 50.0, 10.0)
    );
}

/// Transforms move and scale the target's rect, as pre-paint places it
/// (the same as `element_bounds`, less the viewport's scroll offset).
#[test]
fn transforms() {
    let html = "<style>body { margin: 0 }</style>\
        <div style='transform: translate(30px, 40px) scale(2); width: 100px; height: 100px'>\
          <div id=t style='width: 10px; height: 10px; margin-left: 50px'></div>\
        </div>\
        <div id=u style='width: 20px; height: 20px; transform: translateY(400px)'></div>";
    let page = Page::new(html, PointF::new(0.0, 10.0), &[]);
    for id in ["t", "u"] {
        let bounds =
            paint::element_bounds(&page.view, &page.pre, page.id(id)).map(|r| r.offset(0.0, -10.0));
        assert_eq!(page.implicit(id).target, bounds, "#{id}");
    }
    // Scaled about the box's center (50, 50) and moved: `t` (at (50, 0))
    // at (80, -10), 20×20, less the viewport's 10px: its bottom edge on
    // the viewport's top one.
    assert_eq!(page.implicit("t").target, rect(80.0, -20.0, 20.0, 20.0));
    assert_eq!(page.implicit("t").intersection, rect(80.0, 0.0, 20.0, 0.0));
    assert_eq!(page.implicit("u").intersection, None);
}

/// An element root: its padding box if it clips, else its border box;
/// only its descendants meet it, cut by the clips between them, not by
/// those around the root.
#[test]
fn an_element_as_the_root() {
    let html = "<style>body { margin: 0 } div { box-sizing: border-box }</style>\
        <div id=outer style='overflow: hidden; width: 100px; height: 100px'>\
          <div id=root style='overflow: auto; width: 300px; height: 200px; border: 5px solid; \
            padding: 3px'>\
            <div id=t style='width: 50px; height: 50px; margin-top: 160px'></div>\
          </div>\
        </div>\
        <div id=plain style='width: 40px; height: 40px; border: 2px solid'>\
          <div id=in style='width: 10px; height: 10px'></div>\
        </div>\
        <div id=sibling style='width: 10px; height: 10px'></div>\
        <div id=contained style='contain: paint; width: 40px; height: 40px; border: 2px solid'>\
          <div id=in-contained style='width: 10px; height: 10px'></div>\
        </div>";
    let page = Page::new(html, PointF::new(0.0, 0.0), &[]);
    let root = page.id("root");
    let [t, sibling] = [page.id("t"), page.id("sibling")];
    let found = page.geometry(&[
        request(t, IntersectionRoot::Element(root)),
        request(sibling, IntersectionRoot::Element(root)),
        request(page.id("in"), IntersectionRoot::Element(page.id("plain"))),
        request(
            page.id("in-contained"),
            IntersectionRoot::Element(page.id("contained")),
        ),
    ]);
    // The padding box; `#outer`'s clip (100×100) cuts neither.
    assert_eq!(found[0].root, rect(5.0, 5.0, 290.0, 190.0));
    assert_eq!(found[0].target, rect(8.0, 168.0, 50.0, 50.0));
    assert_eq!(found[0].intersection, rect(8.0, 168.0, 50.0, 27.0));
    // Not a descendant: the root's rect only.
    assert_eq!(found[1].target, None);
    assert_eq!(found[1].intersection, None);
    assert_eq!(found[1].root, rect(5.0, 5.0, 290.0, 190.0));
    // A root that doesn't clip: its border box.
    assert_eq!(found[2].root, rect(0.0, 100.0, 40.0, 40.0));
    assert_eq!(found[2].intersection, rect(2.0, 102.0, 10.0, 10.0));
    // Paint containment clips: its padding box.
    assert_eq!(found[3].root, rect(2.0, 152.0, 36.0, 36.0));
    // A root with no box: nothing.
    let page = Page::new(
        "<div id=r style='display: none'><div id=t></div></div><div id=u></div>",
        PointF::new(0.0, 0.0),
        &[],
    );
    let found = page.geometry(&[request(
        page.id("u"),
        IntersectionRoot::Element(page.id("r")),
    )]);
    assert_eq!(found[0].root, None);
    assert_eq!(found[0].intersection, None);
    // An element isn't its own descendant.
    let page = Page::new(
        "<div id=r style='height: 10px'></div>",
        PointF::new(0.0, 0.0),
        &[],
    );
    let r = page.id("r");
    assert_eq!(
        page.geometry(&[request(r, IntersectionRoot::Element(r))])[0].intersection,
        None
    );
}

/// What is fixed to the viewport stays there as it scrolls.
#[test]
fn fixed_to_the_viewport() {
    let page = Page::new(
        "<style>body { margin: 0; height: 2000px }</style>\
         <div id=f style='position: fixed; top: 10px; left: 20px; width: 30px; height: 40px'>\
         </div>",
        PointF::new(0.0, 500.0),
        &[],
    );
    let f = page.implicit("f");
    assert_eq!(f.target, rect(20.0, 10.0, 30.0, 40.0));
    assert_eq!(f.intersection, rect(20.0, 10.0, 30.0, 40.0));
}

/// An inline element's rect is its boxes' on each line together, as
/// `element_bounds` has them.
#[test]
fn an_inline_element() {
    let page = Page::new(
        "<style>body { margin: 0; width: 60px }</style>\
         <p style='margin: 0'>xx <span id=s>xxx xxx xxx xxx</span></p>",
        PointF::new(0.0, 0.0),
        &[],
    );
    let bounds = paint::element_bounds(&page.view, &page.pre, page.id("s"));
    assert!(bounds.is_some());
    assert_eq!(page.implicit("s").target, bounds);
    assert_eq!(page.implicit("s").intersection, bounds);
}

const PAGE: &str = "<style>body { margin: 0 }</style>\
    <div style='height: 10px'></div>\
    <iframe id=f style='display: block; width: 100px; height: 60px; margin-left: 20px; \
      border: 5px solid blue; padding: 3px'></iframe>";

const CHILD: &str = "<style>body { margin: 0 }</style><body id=b>\
    <div style='height: 50px'></div>\
    <div id=c style='width: 50px; height: 20px; margin-left: 7px'></div>\
    <div style='height: 200px'></div>";

/// A target in a child frame: cut to the iframe's content box, then the
/// viewport; its rects in its own viewport, the implicit root's in the
/// main frame's; its own document as the root is its viewport.
#[test]
fn a_child_frame() {
    let mut frames = Frames::new();
    let page = frames.lay_out(PAGE, VIEWPORT);
    let child = frames.lay_out(CHILD, SizeF::new(100.0, 60.0));
    let pre_for = |scroll: f32| {
        let child_frames = ChildFrames::from_iter([(
            frames.id("f"),
            frames.child_frame(&child, PointF::new(0.0, scroll)),
        )]);
        paint::pre_paint_frame_tree(
            &page,
            &Default::default(),
            &Default::default(),
            &child_frames,
            1.0,
            PointF::new(0.0, 0.0),
            None,
        )
    };
    let c = frames.id("c");
    let child_document = frames.document(&child);
    let geometry = |pre: &PrePaint, root: IntersectionRoot| {
        frames.intersections(&page, pre, &[request(c, root)])[0]
    };
    // The content box is at (28, 18) in the page; `c` at 50 in the child,
    // 10px of it showing.
    let pre = pre_for(0.0);
    let implicit = geometry(&pre, IntersectionRoot::Implicit);
    assert_eq!(implicit.target, rect(7.0, 50.0, 50.0, 20.0));
    assert_eq!(implicit.intersection, rect(7.0, 50.0, 50.0, 10.0));
    assert_eq!(implicit.root, rect(0.0, 0.0, 400.0, 300.0));
    let own = geometry(&pre, IntersectionRoot::Document(child_document));
    assert_eq!(own.root, rect(0.0, 0.0, 100.0, 60.0));
    assert_eq!(own.intersection, rect(7.0, 50.0, 50.0, 10.0));
    // Scrolled by 40: all of it shows, at 10 in the child's viewport.
    let pre = pre_for(40.0);
    let implicit = geometry(&pre, IntersectionRoot::Implicit);
    assert_eq!(implicit.target, rect(7.0, 10.0, 50.0, 20.0));
    assert_eq!(implicit.intersection, rect(7.0, 10.0, 50.0, 20.0));
    // The page's document, or the iframe, as the root: not in it.
    let main = geometry(&pre, IntersectionRoot::Document(frames.document(&page)));
    assert_eq!(main.root, rect(0.0, 0.0, 400.0, 300.0));
    assert_eq!((main.target, main.intersection), (None, None));
    let iframe = geometry(&pre, IntersectionRoot::Element(frames.id("f")));
    assert_eq!((iframe.target, iframe.intersection), (None, None));
    // An element in the child frame as the root: its rect in the child's
    // viewport, the iframe's clip (around it) not cutting the target.
    let body = geometry(&pre, IntersectionRoot::Element(frames.id("b")));
    assert_eq!(body.root, rect(0.0, -40.0, 100.0, 270.0));
    assert_eq!(body.intersection, rect(7.0, 10.0, 50.0, 20.0));
    let body = geometry(&pre_for(0.0), IntersectionRoot::Element(frames.id("b")));
    assert_eq!(body.intersection, rect(7.0, 50.0, 50.0, 20.0));
    // The scroll margin grows the child's viewport: scrolled back to 0, 10
    // more px show.
    let pre = pre_for(0.0);
    let mut with_margin = request(c, IntersectionRoot::Implicit);
    with_margin.scroll_margin = [MarginLength::Px(10.0); 4];
    let grown = frames.intersections(&page, &pre, &[with_margin])[0];
    assert_eq!(grown.intersection, rect(7.0, 50.0, 50.0, 20.0));
}

/// An SVG element: a shape by its bounds with its stroke, a group by what
/// is in it; cut to the `<svg>`'s viewport.
#[test]
fn svg_elements() {
    let page = Page::new(
        "<style>body { margin: 0 }</style>\
         <svg width=100 height=100 style='display: block'>\
           <rect id=r x=10 y=20 width=30 height=40 stroke=black stroke-width=4 />\
           <g id=g><rect x=50 y=50 width=10 height=10 /><rect x=70 y=60 width=10 height=10 /></g>\
           <rect id=out x=90 y=90 width=20 height=20 />\
         </svg>",
        PointF::new(0.0, 0.0),
        &[],
    );
    assert_eq!(page.implicit("r").target, rect(8.0, 18.0, 34.0, 44.0));
    assert_eq!(page.implicit("g").target, rect(50.0, 50.0, 30.0, 20.0));
    let out = page.implicit("out");
    assert_eq!(out.target, rect(90.0, 90.0, 20.0, 20.0));
    assert_eq!(out.intersection, rect(90.0, 90.0, 10.0, 10.0));
}

/// A clip in a space that can't be undone (`scale(0)`): nothing meets it.
#[test]
fn a_clip_that_cant_be_mapped() {
    let page = Page::new(
        "<div style='transform: scale(0); overflow: hidden; width: 100px; height: 100px'>\
           <div id=t style='width: 10px; height: 10px'></div>\
         </div>",
        PointF::new(0.0, 0.0),
        &[],
    );
    let t = page.implicit("t");
    assert!(t.target.is_some());
    assert_eq!(t.intersection, None);
}

/// A scrolled root stays where it is; what is in it moves.
#[test]
fn a_scrolled_element_root() {
    let page = Page::new(
        "<style>body { margin: 0 }</style>\
         <div id=root style='overflow: auto; width: 100px; height: 100px'>\
           <div style='height: 150px'></div>\
           <div id=t style='width: 50px; height: 50px'></div>\
           <div style='height: 500px'></div>\
         </div>",
        PointF::new(0.0, 0.0),
        &[("root", 0.0, 100.0)],
    );
    let found = page.geometry(&[request(
        page.id("t"),
        IntersectionRoot::Element(page.id("root")),
    )]);
    assert_eq!(found[0].root, rect(0.0, 0.0, 100.0, 100.0));
    assert_eq!(found[0].target, rect(0.0, 50.0, 50.0, 50.0));
    assert_eq!(found[0].intersection, rect(0.0, 50.0, 50.0, 50.0));
}

/// Whether a target is visible, when asked (Blink's
/// `HasDistortingVisualEffects`): not under an opacity below 1 or a
/// filter, its own or an ancestor's; nor transformed but by a translation
/// and the same scale up on both axes (a rotation or a skew distorts, also
/// with such a scale). Not asked, or not meeting the root, it isn't.
#[test]
fn what_distorts_a_target() {
    // Four to a row, each 20px with 40px around it: all in the viewport,
    // and nothing covers another's target.
    let html = "<style>body { margin: 0; display: flex; flex-wrap: wrap }\
        body > div { transform-origin: 0 0; width: 20px; height: 20px; margin: 40px }\
        div div { width: 20px; height: 20px }</style>\
        <div><div id=plain></div></div>\
        <div style='opacity: 0.5'><div id=faded></div></div>\
        <div style='opacity: 1'><div id=opaque></div></div>\
        <div><div id=blurred style='filter: blur(1px)'></div></div>\
        <div style='transform: translate(5px, 5px)'><div id=moved></div></div>\
        <div style='transform: scale(2)'><div id=bigger></div></div>\
        <div style='transform: scale(0.5)'><div id=smaller></div></div>\
        <div style='transform: scale(2, 3)'><div id=stretched></div></div>\
        <div style='transform: rotate(10deg)'><div id=turned></div></div>\
        <div style='transform: scale(2) rotate(10deg)'><div id=turned-bigger></div></div>\
        <div style='transform: skew(10deg)'><div id=skewed></div></div>\
        <div style='position: absolute; top: 5000px'><div id=below></div></div>";
    let page = Page::new(html, PointF::new(0.0, 0.0), &[]);
    let visible = |id: &str, visibility: bool| {
        page.geometry(&[IntersectionRequest {
            visibility,
            ..request(page.id(id), IntersectionRoot::Implicit)
        }])[0]
            .visible
    };
    let all: Vec<_> = [
        "plain",
        "faded",
        "opaque",
        "blurred",
        "moved",
        "bigger",
        "smaller",
        "stretched",
        "turned",
        "turned-bigger",
        "skewed",
        "below",
    ]
    .into_iter()
    .filter(|id| visible(id, true))
    .collect();
    assert_eq!(all, ["plain", "opaque", "moved", "bigger"]);
    assert!(!visible("plain", false), "not asked");
}

/// A target in a child frame is distorted by what is around the iframe:
/// an opacity on the iframe's ancestor.
#[test]
fn what_distorts_a_target_in_a_child_frame() {
    let mut frames = Frames::new();
    let page = frames.lay_out(
        "<style>body { margin: 0 } iframe { border: 0; width: 100px; height: 60px }</style>\
         <iframe id=f></iframe><div style='opacity: 0.5'><iframe id=g></iframe></div>",
        VIEWPORT,
    );
    let child = |frames: &mut Frames, id: &str| {
        frames.lay_out(
            &format!("<div id={id} style='width: 10px; height: 10px'></div>"),
            SizeF::new(100.0, 60.0),
        )
    };
    let (in_f, in_g) = (child(&mut frames, "in-f"), child(&mut frames, "in-g"));
    let child_frames = ChildFrames::from_iter([
        (
            frames.id("f"),
            frames.child_frame(&in_f, PointF::new(0.0, 0.0)),
        ),
        (
            frames.id("g"),
            frames.child_frame(&in_g, PointF::new(0.0, 0.0)),
        ),
    ]);
    let pre = paint::pre_paint_frame_tree(
        &page,
        &Default::default(),
        &Default::default(),
        &child_frames,
        1.0,
        PointF::new(0.0, 0.0),
        None,
    );
    let geometry = frames.intersections(
        &page,
        &pre,
        &["in-f", "in-g"].map(|id| IntersectionRequest {
            visibility: true,
            ..request(frames.id(id), IntersectionRoot::Implicit)
        }),
    );
    let visible: Vec<_> = geometry.iter().map(|g| g.visible).collect();
    assert_eq!(visible, [true, false]);
}

/// What covers a target, so it isn't visible (Blink's
/// `HitTestForOcclusion`), as Chrome 152 has it: what is painted after it
/// over its rect, by its box and by how far its paint reaches (shadows, an
/// outline, its text's shadows, a filter), `pointer-events: none` and an
/// empty box too, an SVG shape by what it paints; not a turned box only
/// around the rect, what a clip cuts away (`overflow`, `clip-path`, a
/// scroll container), `visibility: hidden`, an opacity of 0, a run's
/// glyphs past its text box. Nothing over it, it is visible. Each case is
/// a page of its own: the target at (10, 10), 40px wide and high.
#[test]
fn what_covers_a_target() {
    let cases = [
        ("nothing", "", true),
        (
            "a box shadow",
            "<div class=o style='left: 100px; top: 10px; box-shadow: -50px 0 0 10px red'></div>",
            false,
        ),
        (
            "an inset shadow",
            "<div class=o style='left: 70px; top: 10px; box-shadow: inset 0 0 5px red'></div>",
            true,
        ),
        (
            "a shadow's spread",
            "<div class=o style='left: 60px; top: 10px; box-shadow: 0 0 0 15px red'></div>",
            false,
        ),
        (
            "an outline",
            "<div class=o style='left: 60px; top: 30px; outline: 15px solid red'></div>",
            false,
        ),
        (
            "an outline's offset",
            "<div class=o style='left: 75px; top: 10px; outline: 5px solid red; \
             outline-offset: 25px'></div>",
            false,
        ),
        (
            "a text shadow",
            "<div style='position: absolute; left: 100px; top: 15px; font: 20px monospace; \
             text-shadow: -60px 0 red'>X</div>",
            false,
        ),
        (
            "a blur",
            "<div class=o style='left: 70px; top: 10px; background: red; filter: blur(8px)'></div>",
            false,
        ),
        (
            "a drop shadow",
            "<div class=o style='left: 100px; top: 10px; background: red; \
             filter: drop-shadow(-70px 0 0 red)'></div>",
            false,
        ),
        (
            "a turned box around it",
            "<div class=o style='left: 62px; top: 52px; width: 30px; height: 30px; \
             background: red; transform: rotate(45deg)'></div>",
            true,
        ),
        (
            "a turned box over it",
            "<div class=o style='left: 45px; top: 20px; background: red; \
             transform: rotate(45deg)'></div>",
            false,
        ),
        (
            "what overflow cuts away",
            "<div style='position: absolute; left: 100px; top: 0; width: 10px; height: 60px; \
             overflow: hidden'><div class=o style='left: -80px; top: 10px; background: red'>\
             </div></div>",
            true,
        ),
        (
            "what a clip path cuts away",
            "<div class=o style='left: 20px; top: 10px; background: red; \
             clip-path: inset(0 0 0 100%)'></div>",
            true,
        ),
        (
            "what a scroll container cuts away",
            "<div style='position: absolute; left: 0; top: 52px; width: 300px; height: 8px; \
             overflow: scroll'><div class=o style='left: 20px; top: -40px; background: red'>\
             </div></div>",
            true,
        ),
        (
            "visibility: hidden",
            "<div class=o style='left: 20px; top: 10px; background: red; visibility: hidden'>\
             </div>",
            true,
        ),
        (
            "pointer-events: none",
            "<div class=o style='left: 20px; top: 10px; background: red; pointer-events: none'>\
             </div>",
            false,
        ),
        (
            "an opacity of 0",
            "<div class=o style='left: 20px; top: 10px; background: red; opacity: 0'></div>",
            true,
        ),
        (
            "an empty box",
            "<div class=o style='left: 20px; top: 10px'></div>",
            false,
        ),
        (
            "text's glyphs, only around its text box",
            // Its text box ends 4px above the target (the tests' font's
            // ascent and descent are 72px); what the item leaves room for
            // reaches 30px further.
            "<div style='position: absolute; left: 30px; top: -35px; font: 60px serif; \
             line-height: 10px; height: 10px'>gj</div>",
            true,
        ),
        (
            "an SVG shape at its corner",
            "<svg style='position: absolute; left: 50px; top: 50px; width: 100px; height: 100px; \
             overflow: visible'><rect width=100 height=100 fill=blue /></svg>",
            true,
        ),
        (
            "an SVG shape out of its <svg> over it",
            "<svg style='position: absolute; left: 100px; top: 10px; width: 10px; height: 10px; \
             overflow: visible'><rect x=-70 width=20 height=20 fill=blue /></svg>",
            false,
        ),
        (
            "a box beside it",
            "<div class=o style='left: 50px; top: 10px; background: red'></div>",
            true,
        ),
    ];
    for (name, over, expected) in cases {
        let page = Page::new(
            &format!(
                "<style>body {{ margin: 0 }} .o {{ position: absolute; width: 20px; \
                 height: 20px }}</style>\
                 <div id=t style='position: absolute; left: 10px; top: 10px; width: 40px; \
                 height: 40px; background: green'><span>in it</span></div>{over}"
            ),
            PointF::new(0.0, 0.0),
            &[],
        );
        let geometry = page.geometry(&[IntersectionRequest {
            visibility: true,
            ..request(page.id("t"), IntersectionRoot::Implicit)
        }])[0];
        assert_eq!(geometry.visible, expected, "{name}");
    }
}

/// What covers a target in a child frame: the page's box over it, not one
/// over another part of the iframe (Chrome 152 for a frame in its process;
/// machopo's frames all are). A target under the page's iframe: covered.
#[test]
fn what_covers_a_target_in_a_child_frame() {
    let mut frames = Frames::new();
    let page = frames.lay_out(
        "<style>body { margin: 0 } iframe { position: absolute; border: 0; width: 200px; \
         height: 100px } .o { position: absolute; width: 20px; height: 20px }</style>\
         <iframe id=f style='top: 0'></iframe><div class=o style='left: 20px; top: 20px'></div>\
         <iframe id=g style='top: 110px'></iframe>\
         <div class=o style='left: 150px; top: 170px'></div>\
         <div id=under style='position: absolute; top: 230px; width: 10px; height: 10px'></div>\
         <iframe id=h style='top: 220px'></iframe>",
        VIEWPORT,
    );
    let child = |frames: &mut Frames, id: &str| {
        frames.lay_out(
            &format!(
                "<div id={id} style='position: absolute; left: 10px; top: 10px; width: 50px; \
                 height: 50px'></div>"
            ),
            SizeF::new(200.0, 100.0),
        )
    };
    let (in_f, in_g, in_h) = (
        child(&mut frames, "in-f"),
        child(&mut frames, "in-g"),
        child(&mut frames, "in-h"),
    );
    let child_frames = ChildFrames::from_iter([("f", &in_f), ("g", &in_g), ("h", &in_h)].map(
        |(iframe, view)| {
            (
                frames.id(iframe),
                frames.child_frame(view, PointF::new(0.0, 0.0)),
            )
        },
    ));
    let pre = paint::pre_paint_frame_tree(
        &page,
        &Default::default(),
        &Default::default(),
        &child_frames,
        1.0,
        PointF::new(0.0, 0.0),
        None,
    );
    let geometry = frames.intersections(
        &page,
        &pre,
        &["in-f", "in-g", "under"].map(|id| IntersectionRequest {
            visibility: true,
            ..request(frames.id(id), IntersectionRoot::Implicit)
        }),
    );
    let visible: Vec<_> = geometry.iter().map(|g| g.visible).collect();
    assert_eq!(visible, [false, true, false]);
}

/// An iframe as the target: what its child frame paints over its box is
/// the target too (Blink's hit test stops at the iframe); the page's box
/// over it covers it.
#[test]
fn an_iframe_as_the_target() {
    let mut frames = Frames::new();
    let page = frames.lay_out(
        "<style>body { margin: 0 } iframe { position: absolute; border: 0; width: 100px; \
         height: 60px } .o { position: absolute; width: 20px; height: 20px }</style>\
         <iframe id=f style='top: 0'></iframe>\
         <iframe id=g style='top: 100px'></iframe><div class=o style='left: 10px; top: 110px'></div>",
        VIEWPORT,
    );
    let child = |frames: &mut Frames| {
        frames.lay_out(
            "<body style='margin: 0; background: blue'><div style='height: 60px'>text</div>",
            SizeF::new(100.0, 60.0),
        )
    };
    let (in_f, in_g) = (child(&mut frames), child(&mut frames));
    for (iframe, view) in [("f", &in_f), ("g", &in_g)] {
        let (document, iframe) = (frames.document(view), frames.id(iframe));
        frames.parents.insert(document, iframe);
    }
    let child_frames =
        ChildFrames::from_iter([("f", &in_f), ("g", &in_g)].map(|(iframe, view)| {
            (
                frames.id(iframe),
                frames.child_frame(view, PointF::new(0.0, 0.0)),
            )
        }));
    let pre = paint::pre_paint_frame_tree(
        &page,
        &Default::default(),
        &Default::default(),
        &child_frames,
        1.0,
        PointF::new(0.0, 0.0),
        None,
    );
    let geometry = frames.intersections(
        &page,
        &pre,
        &["f", "g"].map(|id| IntersectionRequest {
            visibility: true,
            ..request(frames.id(id), IntersectionRoot::Implicit)
        }),
    );
    let visible: Vec<_> = geometry.iter().map(|g| g.visible).collect();
    assert_eq!(visible, [true, false]);
}

/// The occlusion test where the page is scrolled: the target's rect and
/// what covers it are both in the document, a box fixed to the viewport
/// where the viewport is now. And SVG text by its cells: a `<text>` over
/// the target covers it, one only near it doesn't.
#[test]
fn what_covers_a_target_scrolled() {
    let html = "<style>body { margin: 0; height: 2000px } div { position: absolute; \
                width: 20px; height: 20px }</style>\
        <div id=a style='top: 150px'></div><div style='left: 10px; top: 160px'></div>\
        <div id=b style='left: 100px; top: 150px'></div>\
        <div style='position: fixed; left: 110px; top: 60px'></div>\
        <div id=c style='left: 200px; top: 300px'></div>\
        <div style='position: fixed; left: 210px; top: 300px'></div>\
        <div id=d style='left: 300px; top: 150px'></div>\
        <svg style='position: absolute; left: 300px; top: 150px; width: 20px; height: 20px; \
         overflow: visible'><text x=5 y=15 font-size=10>t</text></svg>\
        <div id=e style='left: 300px; top: 250px'></div>\
        <svg style='position: absolute; left: 300px; top: 275px; width: 20px; height: 20px; \
         overflow: visible'><text x=0 y=10 font-size=10>t</text></svg>";
    let page = Page::new(html, PointF::new(0.0, 100.0), &[]);
    let visible: Vec<_> = ["a", "b", "c", "d", "e"]
        .map(|id| {
            page.geometry(&[IntersectionRequest {
                visibility: true,
                ..request(page.id(id), IntersectionRoot::Implicit)
            }])[0]
                .visible
        })
        .into();
    // `a` is under the box after it; `b` under the fixed box, at the
    // document's 160 with the viewport scrolled 100; `c`, at the
    // document's 300, not under the fixed one, at its 400. `e`'s text's
    // cells start 5px below it; what its item leaves room for, above.
    assert_eq!(visible, [false, false, true, false, true]);
}
