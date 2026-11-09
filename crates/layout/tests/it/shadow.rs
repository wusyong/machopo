//! Shadow trees, laid out as the flat tree: a host's box holds its shadow
//! tree's content, a slot is replaced by what is assigned to it (or its
//! fallback); and `display: contents`, whose element has no box and whose
//! children are its parent's content.

use std::collections::BTreeMap;
use std::sync::Arc;

use computed_style::PseudoId;
use dom::Cx;
use gfx::{NodeId, PointF, RectF};
use layout::{BoxFragment, BoxTree};

use crate::page::{Page, VIEWPORT, attach_shadow, find, has_fragment};

struct Laid {
    tree: BoxTree,
    /// Border boxes in LayoutView coordinates, by `id` attribute.
    rects: BTreeMap<String, RectF>,
    /// The texts laid out, in order: each run's text node's data.
    texts: Vec<String>,
    /// The ids of the elements with an inline box, in order (once each).
    inline_boxes: Vec<String>,
    /// How many lines each element with an id has.
    lines: BTreeMap<String, usize>,
    ids: BTreeMap<NodeId, String>,
}

/// `html`, with the element of each id in `shadows` given a shadow tree
/// of its markup, in order, styled and built.
fn page(html: &str, shadows: &[(&str, &str)]) -> Page {
    Page::build_with(html, |cx, document| {
        for (host, markup) in shadows {
            attach_shadow(cx, find(cx, document, host), markup);
        }
    })
}

/// [`page`], laid out.
fn lay_out(html: &str, shadows: &[(&str, &str)]) -> Laid {
    let mut page = page(html, shadows);
    let view = page.tree.layout(VIEWPORT);
    let ids = page.ids();
    let Page {
        document,
        dom,
        tree,
        ..
    } = page;
    let mut laid = Laid {
        tree,
        rects: BTreeMap::new(),
        texts: Vec::new(),
        inline_boxes: Vec::new(),
        lines: BTreeMap::new(),
        ids,
    };
    dom.read(|cx| laid.collect(cx, &view, PointF::new(0.0, 0.0)));
    // Before the DOM: a root mustn't outlive its heap.
    drop(document);
    laid
}

impl Laid {
    fn collect(&mut self, cx: &Cx<'_, '_>, fragment: &Arc<BoxFragment>, origin: PointF) {
        if let Some(id) = fragment
            .node
            .filter(|_| fragment.pseudo.is_none())
            .and_then(|n| self.ids.get(&n))
            .cloned()
        {
            let size = fragment.size;
            self.lines.insert(id.clone(), fragment.lines.len());
            self.rects
                .insert(id, RectF::new(origin.x, origin.y, size.width, size.height));
        }
        for line in &fragment.lines {
            for run in &line.runs {
                let text = match &run.source {
                    Some((node, _)) => cx.data(cx.node_by_id(*node).unwrap()).unwrap().to_string(),
                    None => format!("<{:?}>", run.pseudo),
                };
                if self.texts.last() != Some(&text) {
                    self.texts.push(text);
                }
            }
            for inline_box in line.boxes.iter().filter(|b| b.pseudo.is_none()) {
                if let Some(id) = self.ids.get(&inline_box.node)
                    && !self.inline_boxes.contains(id)
                {
                    self.inline_boxes.push(id.clone());
                }
            }
        }
        for child in &fragment.children {
            let at = PointF::new(origin.x + child.offset.x, origin.y + child.offset.y);
            self.collect(cx, &child.fragment, at);
        }
    }

    fn rect(&self, id: &str) -> RectF {
        *self
            .rects
            .get(id)
            .unwrap_or_else(|| panic!("no box for #{id}"))
    }

    fn id(&self, id: &str) -> NodeId {
        *self.ids.iter().find(|(_, i)| *i == id).unwrap().0
    }
}

const AHEM: &str = "<style>body { margin: 0; font: 10px/1 Ahem } div, p { margin: 0 }</style>";

/// A host's box holds its shadow tree; a slot is replaced by what is
/// assigned to it; a child assigned to no slot has no box.
#[test]
fn a_host_lays_out_its_shadow_tree() {
    let laid = lay_out(
        &format!(
            "{AHEM}<div id=host style='width: 100px'>\
             <p id=assigned style='height: 5px'></p><p id=none slot=none style='height: 7px'></p>light\
             </div>"
        ),
        &[(
            "host",
            "<div id=first style='height: 10px'></div><slot></slot><div id=last style='height: 20px'></div>",
        )],
    );
    assert_eq!(laid.rect("first").y, 0.0);
    assert_eq!(laid.rect("assigned"), RectF::new(0.0, 10.0, 100.0, 5.0));
    assert!(!laid.rects.contains_key("none"));
    // The text is assigned to the default slot, in an anonymous block.
    assert_eq!(laid.texts, ["light"]);
    assert_eq!(laid.rect("last").y, 25.0);
    assert_eq!(laid.rect("host").height, 45.0);
}

/// A slot with nothing assigned shows its children (its fallback); one
/// with something, not them. Slots in slots.
#[test]
fn fallback_and_slots_in_slots() {
    let laid = lay_out(
        &format!("{AHEM}<div id=host><b slot=x>assigned</b></div>"),
        &[
            (
                "host",
                "<slot name=y>fallback y</slot><span id=inner-host><slot name=x slot=z>fallback x</slot></span>",
            ),
            ("inner-host", "[<slot name=z></slot>]"),
        ],
    );
    assert_eq!(laid.texts, ["fallback y", "[", "assigned", "]"]);
}

/// `display: contents`: no box nor inline box of its own; its children,
/// block or inline, and its `::before` and `::after` are laid out as its
/// parent's; its text takes its style. Not for a replaced element or a
/// form control, which `contents` hides.
#[test]
fn display_contents_lays_out_its_children_as_its_parents() {
    let laid = lay_out(
        &format!(
            "{AHEM}<style>
               .c {{ display: contents; color: green }}
               #c-inline::before {{ content: '<' }}
               #c-inline::after {{ content: '>'; display: contents }}
             </style>
             <div id=a style='width: 200px'>\
               <span class=c id=c-inline>text <i id=i>inside</i></span>\
             </div>\
             <div id=b style='width: 200px'>\
               <div class=c id=c-block><p id=p style='height: 5px'></p></div>\
             </div>\
             <div id=d><img class=c id=img style='width: 10px; height: 10px'>\
               <input class=c id=input>\
               <canvas class=c><p id=canvas-fallback>fallback</p></canvas>\
               <select class=c><option>option</option></select></div>"
        ),
        &[],
    );
    assert!(!laid.rects.contains_key("c-inline"));
    assert!(!laid.rects.contains_key("c-block"));
    assert_eq!(laid.inline_boxes, ["i"], "no inline box for the contents");
    assert_eq!(laid.lines["a"], 1, "nothing breaks the line");
    assert_eq!(
        laid.texts,
        ["<Some(Before)>", "text ", "inside", "<Some(After)>"]
    );
    // A block in it is a child of the block it is in.
    assert_eq!(
        laid.rect("p"),
        RectF::new(0.0, laid.rect("b").y, 200.0, 5.0)
    );
    // Hidden, not laid out.
    assert!(!laid.rects.contains_key("img"));
    assert!(!laid.rects.contains_key("input"));
    assert!(!laid.rects.contains_key("canvas-fallback"));
    assert_eq!(laid.rect("d").height, 0.0);
}

/// A flex host's slotted children are flex items, laid out in a row.
#[test]
fn a_flex_hosts_slotted_children_are_flex_items() {
    let laid = lay_out(
        &format!(
            "{AHEM}<div id=host style='display: flex'>\
             <span id=a style='width: 10px; height: 5px'></span>\
             <span id=b style='width: 20px; height: 5px'></span></div>"
        ),
        &[("host", "<slot></slot>")],
    );
    assert_eq!(laid.rect("a"), RectF::new(0.0, 0.0, 10.0, 5.0));
    assert_eq!(laid.rect("b"), RectF::new(10.0, 0.0, 20.0, 5.0));
}

/// Counters go in the flat tree's order.
#[test]
fn counters_go_in_the_flat_trees_order() {
    let laid = lay_out(
        "<ol id=host><li id=b>b</li></ol>",
        &[("host", "<li id=a>a</li><slot></slot><li id=c>c</li>")],
    );
    let marker = |id: &str| {
        laid.tree
            .counter_text_for_testing(laid.id(id), PseudoId::Marker)
            .unwrap()
    };
    assert_eq!(
        [marker("a"), marker("b"), marker("c")],
        ["1. ", "2. ", "3. "]
    );
    // A `display: contents` element sets, resets and increments no counter,
    // and what is in it counts on from its siblings, as its parent's
    // children.
    let laid = lay_out(
        "<style>.c { display: contents; counter-reset: x 666; counter-increment: x }
           .i { counter-increment: x } .i::before { content: counter(x) }</style>\
         <div style='counter-reset: x 6'><span class=c></span><span class=i id=a></span>\
           <div class=c><span class=i id=b></span></div><span class=i id=c></span></div>",
        &[],
    );
    let before = |id: &str| {
        laid.tree
            .counter_text_for_testing(laid.id(id), PseudoId::Before)
            .unwrap()
    };
    assert_eq!([before("a"), before("b"), before("c")], ["7", "8", "9"]);
    // Its `::before` has a box, which counts.
    let laid = lay_out(
        "<style>.c { display: contents } .c::before { content: '-'; counter-increment: x 10 }
           .i::before { content: counter(x) }</style>\
         <div style='counter-reset: x'><span class=c></span><span class=i id=d></span></div>",
        &[],
    );
    assert_eq!(
        laid.tree
            .counter_text_for_testing(laid.id("d"), PseudoId::Before)
            .unwrap(),
        "10"
    );
    // Not an element that `contents` hides.
    let laid = lay_out(
        "<ol><li id=a>a</li>\
         <img style='display: contents; counter-increment: list-item'><li id=b>b</li></ol>",
        &[],
    );
    assert_eq!(
        laid.tree
            .counter_text_for_testing(laid.id("b"), PseudoId::Marker)
            .unwrap(),
        "2. "
    );
}

/// Text whose parent is `display: contents` (a slot, by default) has that
/// parent's font, as text in an inline element has its own: it has no
/// inline box whose font the text would take.
#[test]
fn text_in_a_contents_element_has_its_font() {
    let font_sizes = |html: &str, shadows: &[(&str, &str)]| {
        let view = page(html, shadows).tree.layout(VIEWPORT);
        fn sizes(fragment: &BoxFragment, out: &mut Vec<(f32, f32)>) {
            for line in &fragment.lines {
                for run in &line.runs {
                    out.push((run.font_size, line.rect.height));
                }
            }
            for child in &fragment.children {
                sizes(&child.fragment, out);
            }
        }
        let mut out = Vec::new();
        sizes(&view, &mut out);
        out
    };
    assert_eq!(
        font_sizes(
            "<style>span { display: contents; font: 40px/48px Ahem }</style>\
             <div>a<span>b</span></div>",
            &[]
        ),
        [(16.0, 48.0), (40.0, 48.0)]
    );
    assert_eq!(
        font_sizes(
            "<div id=host>slotted</div>",
            &[(
                "host",
                "<style>slot { font: 40px/48px Ahem }</style><slot></slot>"
            )]
        ),
        [(40.0, 48.0)]
    );
}

#[test]
fn display_contents_is_none_for_unusual_elements() {
    let laid = crate::page::lay_out(
        "<!doctype html><div id=d><video style='display: contents'>\
         <span id=in>x</span></video><b style='display: contents'>\
         <span id=kept>y</span></b></div>",
    );
    let view = &laid.view;
    assert!(!has_fragment(view, laid.node("in")), "{}", view.dump());
    assert!(has_fragment(view, laid.node("kept")), "{}", view.dump());
}
