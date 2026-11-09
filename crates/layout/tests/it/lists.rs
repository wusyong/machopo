//! List items and counters (css-lists 3): the marker each list item
//! shows, outside its first line or as its first inline box, and the
//! values of the counters, in list items' markers and in `content`.

use std::collections::BTreeMap;
use std::sync::Arc;

use computed_style::PseudoId;
use gfx::{PointF, SizeF};
use layout::{BoxFragment, BoxTree, FragmentLink};

use crate::page::Page;

/// A page laid out, and its box tree, which has what the counters gave.
struct Laid {
    tree: BoxTree,
    laid: crate::page::Laid,
    fragments: BTreeMap<String, Arc<BoxFragment>>,
}

fn lay_out(html: &str) -> Laid {
    let mut page = Page::build(html);
    let laid = page.lay_out();
    Laid {
        fragments: laid.fragments.clone(),
        tree: page.tree,
        laid,
    }
}

impl Laid {
    /// `id`'s outside marker, where its list item's fragment has it.
    fn marker(&self, id: &str) -> Option<&FragmentLink> {
        self.fragments[id]
            .children
            .iter()
            .find(|child| child.fragment.pseudo == Some(PseudoId::Marker))
    }

    /// What the counters gave `id`'s `pseudo`.
    fn text(&self, id: &str, pseudo: PseudoId) -> Option<String> {
        self.tree
            .counter_text_for_testing(self.laid.node(id), pseudo)
    }

    fn marker_text(&self, id: &str) -> Option<String> {
        self.text(id, PseudoId::Marker)
    }
}

const AHEM: &str = "<style>body { margin: 0 } ol, ul { margin: 0; padding-left: 100px;
    font: 20px/1 Ahem }</style>";

/// An outside marker ends where the list item's content box starts, on
/// its first line's baseline, and takes no room on the line: the text
/// starts at the content box.
#[test]
fn an_outside_marker_is_before_the_first_line() {
    let laid = lay_out(&format!(
        "{AHEM}<ol><li id=a>XX<li id=b><span style='line-height: 40px'>X</span></ol>"
    ));
    assert_eq!(laid.marker_text("a").as_deref(), Some("1. "));
    let marker = laid.marker("a").expect("a marker");
    // "1. ": 60px wide.
    assert_eq!(marker.fragment.size, SizeF::new(60.0, 20.0));
    assert_eq!(marker.offset, PointF::new(-60.0, 0.0));
    assert_eq!(marker.fragment.node, laid.fragments["a"].node);
    assert_eq!(laid.fragments["a"].lines[0].rect.x, 0.0);
    // On the baseline of a taller line: 10px down.
    assert_eq!(laid.marker("b").unwrap().offset.y, 10.0);
}

/// A marker doesn't take its list item's `text-indent`, which indents the
/// item's own first line.
#[test]
fn an_outside_marker_isnt_indented() {
    let laid = lay_out(&format!(
        "{AHEM}<ol style='text-indent: 40px'><li id=a>X</ol>"
    ));
    let marker = laid.marker("a").expect("a marker");
    assert_eq!(marker.fragment.size, SizeF::new(60.0, 20.0));
    assert_eq!(marker.offset.x, -60.0);
    assert_eq!(laid.fragments["a"].lines[0].rect.x, 40.0);
}

/// The marker goes at the start of the first line: past a float beside
/// it, not where `text-align` puts the line's content.
#[test]
fn an_outside_marker_starts_where_the_line_does() {
    let laid = lay_out(&format!(
        "{AHEM}<div style='float: left; width: 200px; height: 40px'></div>
         <ol><li id=a>X<li id=b style='text-align: right'>X</ol>"
    ));
    // The float takes the first 200px; the list item's content box starts
    // at 100.
    assert_eq!(laid.marker("a").unwrap().offset.x, 100.0 - 60.0);
    assert_eq!(laid.marker("b").unwrap().offset.x, 100.0 - 60.0);
}

/// The marker goes on the first line in the list item, in a block in it
/// too; one without lines has it at the top, and is as tall as it.
#[test]
fn an_outside_marker_finds_the_first_line() {
    let laid = lay_out(&format!(
        "{AHEM}<ul style='list-style-type: decimal'>
           <li id=nested><div style='padding-top: 15px'><div style='height: 5px'></div>X</div>
           <li id=empty>
           <li id=block><div style='height: 50px'></div>
         </ul>"
    ));
    assert_eq!(laid.marker("nested").unwrap().offset.y, 20.0);
    assert_eq!(laid.marker("empty").unwrap().offset.y, 0.0);
    assert_eq!(laid.fragments["empty"].size.height, 20.0);
    assert_eq!(laid.marker("block").unwrap().offset.y, 0.0);
    assert_eq!(laid.fragments["block"].size.height, 50.0);
}

/// An inside marker is the list item's first inline box, on its first
/// line; `list-style-type: none`, or `::marker { content: none }`, shows
/// none; `::marker`'s `content` takes the place of the type's.
#[test]
fn inside_markers_and_none() {
    let laid = lay_out(&format!(
        "{AHEM}<style>#c::marker {{ content: '>' counter(list-item) }}
           #d::marker {{ content: none }}</style>
         <ol style='list-style-position: inside'><li id=a>X</ol>
         <ol><li id=b style='list-style-type: none'>X<li id=c>X<li id=d>X<li id=e style='list-style-type: \"-\"'>X</ol>"
    ));
    let line = &laid.fragments["a"].lines[0];
    assert_eq!(line.boxes[0].pseudo, Some(PseudoId::Marker));
    // "1. X".
    assert_eq!(line.rect.width, 80.0);
    assert!(laid.marker("a").is_none());
    for id in ["b", "d"] {
        assert!(laid.marker(id).is_none(), "{id}");
    }
    assert_eq!(laid.marker_text("c").as_deref(), Some(">2"));
    assert_eq!(laid.marker_text("e").as_deref(), Some("-"));
}

/// The list items' numbers: `<ol start>`, `reversed` (counting down to
/// the first `<li value>`, or to 1), `<li value>`, the counter styles; an
/// item with `display: none` isn't counted, and a list in an item starts
/// its own.
#[test]
fn list_items_are_numbered() {
    let laid = lay_out(&format!(
        "{AHEM}<ol start=4><li id=a1><li id=a2 value=10><li id=a3></ol>
         <ol reversed><li id=b1><li id=b2 style='display: none'><li id=b3><li id=b4></ol>
         <ol reversed start=5><li id=c1><li id=c2></ol>
         <ol reversed><li id=f1><li id=f2 value=30><li id=f3 value=35><li id=f4></ol>
         <ol type=I><li id=d1><li id=d2 type=a><li id=d3><ol><li id=d4></ol></ol>
         <ul><li id=e1><ul><li id=e2></ul></ul>"
    ));
    let markers: Vec<_> = [
        "a1", "a2", "a3", "b1", "b3", "b4", "c1", "c2", "f1", "f2", "f3", "f4", "d1", "d2", "d3",
        "d4", "e1", "e2",
    ]
    .iter()
    .map(|id| laid.marker_text(id).unwrap())
    .collect();
    assert_eq!(
        markers,
        [
            "4. ", "10. ", "11. ", "3. ", "2. ", "1. ", "5. ", "4. ", "31. ", "30. ", "35. ",
            "34. ", "I. ", "b. ", "III. ", "1. ", "• ", "◦ "
        ]
    );
    assert_eq!(laid.marker_text("b2"), None);
}

/// `counter()` and `counters()` in a `::before`: reset, incremented and
/// set by elements and by the pseudo-elements themselves; an element
/// inherits its parent's counters, and those of its preceding sibling's
/// whose names its parent's hasn't (css-lists 3, "Inheriting Counters");
/// a nested reset makes another counter of the same name, which
/// `counters()` shows with the outer ones.
#[test]
fn counters_in_content() {
    let laid = lay_out(
        "<style>body { counter-reset: h } h2::before { counter-increment: h;
           content: counter(h, upper-roman) '.' } .s { counter-reset: s 4 }
           p { counter-increment: s } p::before { content: counters(s, '-') }
           #set { counter-set: s 9 } #undef::before { content: counter(x) }
           #big { counter-reset: b 2100000000 } #big p { counter-increment: b 50000000 }
           #big p::before { content: counter(b) }
           ol { counter-reset: list-item } .g::before { content: counters(list-item, '.') }
           #after::after { content: counter(z) }</style>
         <h2 id=h1></h2><h2 id=h2></h2>
         <div class=s><p id=p1></p><div class=s><p id=p2></p></div><p id=p3></p></div>
         <p id=p4></p><p id=set></p><div id=undef></div>
         <div id=big><p id=b1></p><p id=b2></p></div>
         <ol><li id=g1 class=g></ol><ol><li id=g2 class=g></ol>
         <div id=after><i style='counter-reset: z 5'></i></div>",
    );
    let text = |id: &str| laid.text(id, PseudoId::Before).unwrap();
    assert_eq!(text("h1"), "I.");
    assert_eq!(text("h2"), "II.");
    assert_eq!(text("p1"), "5");
    assert_eq!(text("p2"), "5-5");
    // After the inner `.s`: its counter isn't inherited from it, as its
    // parent has one of that name.
    assert_eq!(text("p3"), "6");
    // After the outer `.s`: its counter is, as `<body>` has none.
    assert_eq!(text("p4"), "7");
    assert_eq!(text("set"), "9");
    assert_eq!(text("undef"), "0");
    // An increment that would overflow leaves the value.
    assert_eq!(text("b1"), "2100000000");
    assert_eq!(text("b2"), "2100000000");
    // The second list's reset takes the place of its preceding sibling's.
    assert_eq!(text("g2"), "1");
    // `::after` comes after the children: their counters are its.
    assert_eq!(laid.text("after", PseudoId::After).as_deref(), Some("5"));
    // Shown as generated text.
    let h1 = &laid.fragments["h1"];
    assert_eq!(h1.lines[0].runs[0].pseudo, Some(PseudoId::Before));
}

/// Style containment scopes the counters in the element's subtree
/// (css-contain 2): what is in it increments or sets of a counter from
/// outside is a new counter there, which the following siblings inside
/// have too (WPT's `contain-style-counters-005.html`), and nothing outside;
/// outside, the counter goes on as it was. The element's own increment is
/// outside, and a counter from outside reads as it is.
#[test]
fn style_containment_scopes_counters() {
    let laid = lay_out(
        "<style>body { counter-reset: n 1 } p { counter-increment: n } p::before { content: counter(n) }
           .c { contain: style } i::before { content: counter(n) }</style>
         <p id=a></p>
         <div class=c><i id=r></i><p id=b></p><p id=c></p></div>
         <p id=d></p>
         <div class=c style='counter-increment: n'><p id=e></p></div>
         <p id=f></p>",
    );
    let text = |id: &str| laid.text(id, PseudoId::Before).unwrap();
    assert_eq!(text("a"), "2");
    assert_eq!(text("r"), "2");
    assert_eq!(text("b"), "1");
    assert_eq!(text("c"), "2");
    assert_eq!(text("d"), "3");
    // The element's own increment is the outer counter's (to 4).
    assert_eq!(text("e"), "1");
    assert_eq!(text("f"), "5");
}
