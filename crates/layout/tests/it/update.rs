//! Updating the box tree after a change (`BoxTree::update`), and
//! laying it out again: what is built, shaped, restyled and laid out
//! again, which fragments are kept, the counters' passes and the lists
//! numbered, and that it lays out as building everything again does
//! (`Page::check`).

use std::collections::HashMap;
use std::sync::Arc;

use computed_style::PseudoId;
use dom::{Cx, NodeRef};
use gfx::SizeF;
use layout::{BoxFragment, BoxTreeUpdate};

use crate::page::{BASE_URL, Page, VIEWPORT, ensure_shadow_trees, find, html, open, raster, svg};

/// The fragments of the elements with an id, by id.
fn fragments(view: &BoxFragment, page: &Page) -> HashMap<String, Arc<BoxFragment>> {
    fn collect(
        fragment: &Arc<BoxFragment>,
        ids: &std::collections::BTreeMap<gfx::NodeId, String>,
        out: &mut HashMap<String, Arc<BoxFragment>>,
    ) {
        // An element's own box, not its pseudo-elements', which have its
        // node too.
        if let Some(id) = fragment
            .node
            .filter(|_| fragment.pseudo.is_none())
            .and_then(|node| ids.get(&node))
        {
            out.insert(id.clone(), fragment.clone());
        }
        for child in &fragment.children {
            collect(&child.fragment, ids, out);
        }
    }
    let ids = page.ids();
    let mut out = HashMap::new();
    for child in &view.children {
        collect(&child.fragment, &ids, &mut out);
    }
    out
}

/// The ids whose fragments are the same as before.
fn kept(page: &Page, before: &BoxFragment) -> Vec<String> {
    let after = page.view();
    let (old, new) = (fragments(before, page), fragments(&after, page));
    let mut kept: Vec<String> = new
        .iter()
        .filter(|(id, fragment)| old.get(*id).is_some_and(|old| Arc::ptr_eq(old, fragment)))
        .map(|(id, _)| id.clone())
        .collect();
    kept.sort();
    kept
}

/// Boxes laid out and fragments made, as `Page::laid_out` gives them.
fn laid_out(laid_out: usize, fragments: usize) -> (usize, usize) {
    (laid_out, fragments)
}

fn set_style(page: &mut Page, id: &str, style: &str) -> BoxTreeUpdate {
    set(page, id, "style", style)
}

/// Sets the attribute `name` of the element whose `id` is `id`.
fn set(page: &mut Page, id: &str, name: &str, value: &str) -> BoxTreeUpdate {
    page.change(|cx, document| cx.set_attribute(find(cx, document, id), name, value))
}

fn update(built: usize, shaped: usize, restyled: usize) -> BoxTreeUpdate {
    BoxTreeUpdate {
        full: false,
        built,
        shaped,
        restyled,
        svg_captured: 0,
    }
}

/// An element named `name` with `attributes`, of `document`.
fn element<'gc>(
    cx: &Cx<'_, 'gc>,
    document: NodeRef<'gc>,
    name: &str,
    attributes: &[(&str, &str)],
) -> NodeRef<'gc> {
    let element = cx.create_element(document, html(name));
    for (name, value) in attributes {
        cx.set_attribute(element, name, value);
    }
    element
}

/// The first child of the element whose `id` is `id`.
fn first_child<'gc>(cx: &Cx<'_, 'gc>, document: NodeRef<'gc>, id: &str) -> NodeRef<'gc> {
    cx.first_child(find(cx, document, id)).expect("a child")
}

/// The `<img>`s whose `id`s are `ids` given images of `width` × `height`
/// (each its own, `<id>.png`): asked for, then come. What the update as
/// they come did.
fn give_images(page: &mut Page, ids: &[&str], width: u32, height: u32) -> BoxTreeUpdate {
    // A window to fetch them in, first.
    page.request_images();
    page.change(|cx, document| {
        for id in ids {
            cx.set_attribute(find(cx, document, id), "src", &format!("{id}.png"));
        }
    });
    assert_eq!(page.request_images().len(), ids.len());
    page.update();
    page.change(|cx, _| {
        for id in ids {
            cx.image_fetched(
                &format!("{BASE_URL}{id}.png"),
                Some(raster(width, height)),
                true,
            );
        }
    })
}

#[test]
fn nothing_changed_nothing_done() {
    let mut page = Page::new("<p>Some text <b>here</b></p>");
    assert_eq!(page.update(), update(0, 0, 0));
}

#[test]
fn a_color_changes_the_styles_in_place() {
    let mut page = Page::new("<p id=p style='color: red'>Some text <span>here</span></p>");
    // The paragraph and its span, which inherits the color: no text is
    // shaped again.
    assert_eq!(set_style(&mut page, "p", "color: blue"), update(0, 0, 2));
}

/// A `<br>`, which inherits the color too, has its style in its newline's
/// run: changed in place there, nothing built or laid out again. What
/// changes its line's height, a font, still lays the block out again, and
/// one gone shapes the content again.
#[test]
fn a_color_changes_a_brs_style_in_place() {
    let mut page = Page::new(
        "<body><p id=p style='color: red'>One<br id=br>two <b>three<br></b>four</p></body>",
    );
    // The paragraph, its `<b>` and both `<br>`s.
    assert_eq!(set_style(&mut page, "p", "color: blue"), update(0, 0, 4));
    // The paragraph, `<body>`, `<html>` and the `LayoutView`.
    assert_eq!(page.laid_out(), laid_out(0, 4));
    page.check();
    assert_eq!(set_style(&mut page, "br", "color: green"), update(0, 0, 1));
    assert_eq!(page.laid_out(), laid_out(0, 4));
    page.check();
    assert_eq!(
        set_style(&mut page, "br", "font-size: 40px"),
        update(0, 1, 0)
    );
    assert_ne!(page.laid_out(), laid_out(0, 4));
    page.check();
    assert_eq!(set_style(&mut page, "br", "display: none"), update(0, 1, 0));
    page.check();
}

/// A style changed in place reaches the hyphen a soft hyphen ends a line
/// with: it takes its run's style and element as the lines are made into
/// fragments, and no text node's part.
#[test]
fn a_new_color_reaches_the_hyphen() {
    const STYLE: &str = "width: 80px; font: 20px/1 Ahem";
    let mut page = Page::new(&format!(
        "<div id=d style='{STYLE}; color: red'>XXX&shy;XXX</div>"
    ));
    assert_eq!(
        set_style(&mut page, "d", &format!("{STYLE}; color: blue")),
        update(0, 0, 1)
    );
    let view = page.view();
    let runs = &fragments(&view, &page)["d"].lines[0].runs;
    let [text, hyphen] = &runs[..] else {
        panic!("{runs:?}")
    };
    assert_eq!(hyphen.style.color, text.style.color);
    assert_eq!(hyphen.node, text.node);
    assert_eq!(hyphen.source, None);
}

#[test]
fn an_anonymous_block_takes_the_color_of_its_parent() {
    let mut page = Page::new("<div id=d style='color: red'>Text<p>Paragraph</p></div>");
    // `#d` and its paragraph; the anonymous block around "Text" inherits.
    assert_eq!(set_style(&mut page, "d", "color: blue"), update(0, 0, 2));
}

#[test]
fn what_only_paints_changes_in_place() {
    let mut page = Page::new(
        "<div id=d>A block</div>\
         <p>Some <span id=s>inline</span> text <img id=i style='width: 10px; height: 10px'></p>",
    );
    const BASE: &str = "border: 2px solid blue";
    // Each from `BASE` and the first, to `BASE` and the second.
    for (from, change) in [
        ("", "color: red"),
        ("", "background-color: green"),
        ("", "border-color: red"),
        // `none` makes the width 0, which is laid out otherwise; at 0 it
        // changes nothing laid out.
        ("border-width: 0px", "border-width: 0px; border-style: none"),
        ("", "opacity: 0.5"),
        ("", "transform: translate(10px, 0)"),
        ("", "text-decoration: underline"),
        ("", "border-radius: 4px 50% / 10%"),
        ("", "box-shadow: 1px 2px 3px red, inset 0 0 4px"),
        ("", "outline: 2px solid blue; outline-offset: 3px"),
        (
            "",
            "background: url(a.png) right 10px top / 20px no-repeat fixed content-box, \
             linear-gradient(red, blue)",
        ),
    ] {
        for (id, size) in [("d", ""), ("s", ""), ("i", "width: 10px; height: 10px; ")] {
            set_style(&mut page, id, &format!("{size}{BASE}; {from}"));
            assert_eq!(
                set_style(&mut page, id, &format!("{size}{BASE}; {change}")),
                update(0, 0, 1),
                "#{id} {change}"
            );
        }
    }
}

#[test]
fn a_size_lays_the_box_out_again_and_shapes_its_own_text() {
    let mut page = Page::new("<div id=d><p>One</p>Two<p>Three</p></div>");
    // `#d` takes the width, and makes its anonymous block again around
    // "Two": its paragraphs keep their boxes.
    assert_eq!(set_style(&mut page, "d", "width: 100px"), update(1, 1, 1));
}

#[test]
fn a_font_size_shapes_inline_content_again() {
    let mut page = Page::new("<p id=p>Some <span id=s>text</span></p><p>Other</p>");
    assert_eq!(
        set_style(&mut page, "s", "font-size: 30px"),
        update(0, 1, 0)
    );
}

#[test]
fn white_space_can_keep_text_that_collapsed_away() {
    let mut page = Page::new("<div id=d><p>a</p>   <p>b</p></div>");
    // The spaces between the paragraphs now make an anonymous block; the
    // paragraphs inherit `white-space`, and shape their text again.
    assert_eq!(
        set_style(&mut page, "d", "white-space: pre"),
        update(1, 3, 3)
    );
}

#[test]
fn changed_text_shapes_only_its_block() {
    let mut page = Page::new("<p>One</p><p id=p>Two <b>bold</b></p><p>Three</p>");
    assert_eq!(
        page.change(|cx, document| cx.set_data(first_child(cx, document, "p"), "Changed ")),
        update(0, 1, 0)
    );
}

/// Breaking the lines of text with a tab copies its shaped text (a tab's
/// advance changes, and the kept shaping holds the text too): the tree
/// built from scratch has another copy, whose runs are the same.
#[test]
fn text_with_a_tab_lays_out_as_built_from_scratch() {
    let mut page = Page::new("<pre>a\tb</pre><p id=p>Two</p>");
    assert_eq!(set_style(&mut page, "p", "width: 10px"), update(0, 1, 1));
}

#[test]
fn an_inserted_block_is_the_only_one_built() {
    let mut page = Page::new("<div id=d><p>One</p><p id=two>Two</p></div>");
    assert_eq!(
        page.change(|cx, document| {
            let p = cx.create_element(document, html("p"));
            let text = cx.create_text(document, "New");
            cx.append_child(p, text).unwrap();
            let (d, two) = (find(cx, document, "d"), find(cx, document, "two"));
            cx.pre_insert(d, p, Some(two)).unwrap();
        }),
        update(1, 1, 0)
    );
}

#[test]
fn a_removed_block_leaves_room_for_new_ones() {
    let mut page = Page::new("<div id=d><p>One</p><p>Two</p></div>");
    // A block in a block, so the inner one goes with it; both empty,
    // nothing to shape.
    let added = page.change(|cx, document| {
        let p = cx.create_element(document, html("div"));
        let inner = cx.create_element(document, html("p"));
        cx.append_child(p, inner).unwrap();
        cx.append_child(find(cx, document, "d"), p).unwrap();
    });
    assert_eq!(added, update(2, 0, 0));
    let p = page.read(|cx, document| cx.root(cx.last_child(find(cx, document, "d")).unwrap()));
    let capacity = page.tree.capacity_for_testing();
    for _ in 0..3 {
        assert_eq!(
            page.change(|cx, _| cx.remove(p.get(cx.gc))),
            update(0, 0, 0)
        );
        assert_eq!(
            page.change(|cx, document| {
                cx.append_child(find(cx, document, "d"), p.get(cx.gc))
                    .unwrap();
            }),
            update(2, 0, 0)
        );
    }
    assert_eq!(page.tree.capacity_for_testing(), capacity);
}

#[test]
fn a_removed_inline_element_shapes_its_block_again() {
    let mut page = Page::new("<p>Some <b id=b>bold <i id=i>text</i></b></p>");
    assert_eq!(
        page.change(|cx, document| cx.remove(find(cx, document, "i"))),
        update(0, 1, 0)
    );
    assert_eq!(
        page.change(|cx, document| cx.remove(find(cx, document, "b"))),
        update(0, 1, 0)
    );
}

#[test]
fn display_builds_the_boxes_again() {
    let mut page =
        Page::new("<div id=d>Text<div><p>One</p><p>Two</p></div></div><p id=after>After</p>");
    // Gone, and back: `#d`, its anonymous block, its inner block and
    // paragraphs; the anonymous block and paragraphs shape their text.
    assert_eq!(set_style(&mut page, "d", "display: none"), update(0, 0, 0));
    assert_eq!(set_style(&mut page, "d", "display: block"), update(5, 3, 0));
    // Made inline, its text goes into an anonymous block of its parent's,
    // and the blocks in it are built again.
    assert_eq!(
        set_style(&mut page, "d", "display: inline"),
        update(4, 3, 0)
    );
}

#[test]
fn a_block_in_an_inline_element_follows_it() {
    // (A `<p>` would end at the `<div>`.)
    let mut page = Page::new("<div>a <span id=s>b <div id=d>c</div> d</span> e</div>");
    // The span's text is in two anonymous blocks, which both take the
    // color, and so does the block in it.
    assert_eq!(set_style(&mut page, "s", "color: green"), update(0, 0, 2));
    // Gone, its parent is one inline formatting context; back, the block
    // in it is built again, and the two anonymous blocks around it.
    assert_eq!(set_style(&mut page, "s", "display: none"), update(0, 1, 0));
    assert_eq!(
        set_style(&mut page, "s", "display: inline"),
        update(3, 3, 0)
    );
}

#[test]
fn a_moved_subtree_is_built_again_with_its_new_styles() {
    // Moved up: where they were is collected again after where they go,
    // and their old boxes are still there when the new ones are built.
    // (A `<p>` would end at the `<div>` in the span.)
    let mut page = Page::new(
        "<div id=to>To</div><div><div id=a><div id=b>Moved</div></div></div>\
         <div><span id=s>In <div id=c>a span</div></span></div>",
    );
    // `#b`'s and `#c`'s new colors are not looked at: their boxes are
    // built again, as their moved ancestors' are.
    page.change(|cx, document| {
        let to = find(cx, document, "to");
        cx.set_attribute(find(cx, document, "b"), "style", "color: red");
        cx.append_child(to, find(cx, document, "a")).unwrap();
        cx.set_attribute(find(cx, document, "c"), "style", "color: red");
        cx.append_child(to, find(cx, document, "s")).unwrap();
    });
}

#[test]
fn a_comment_changes_no_boxes() {
    let mut page = Page::new("<p id=p>Text</p>");
    assert_eq!(
        page.change(|cx, document| {
            let comment = cx.create_comment(document, "note");
            cx.append_child(find(cx, document, "p"), comment).unwrap();
        }),
        update(0, 0, 0)
    );
}

#[test]
fn changes_without_boxes_do_nothing() {
    let mut page = Page::new(
        // Text next to the blocks: collecting the body's content again
        // would make and shape an anonymous block.
        "<div style='display: none'><p id=p>Hidden <span id=s>text</span></p></div>Shown<p>Shown</p>",
    );
    assert_eq!(
        page.change(|cx, document| {
            let (p, s) = (find(cx, document, "p"), find(cx, document, "s"));
            cx.set_data(cx.first_child(s).unwrap(), "changed");
            cx.set_attribute(p, "style", "color: red");
            let div = cx.create_element(document, html("div"));
            cx.append_child(p, div).unwrap();
            cx.remove(s);
        }),
        update(0, 0, 0)
    );
}

#[test]
fn an_image_takes_its_size_when_it_arrives() {
    let mut page = Page::new(
        "<p>Text <img id=inline> more</p><img id=block style='display: block'><p>After</p>",
    );
    for id in ["inline", "block"] {
        // Its box is built again, an atomic inline as a block-level box;
        // the inline content it is in is collected, and shaped, again.
        let shaped = usize::from(id == "inline");
        assert_eq!(
            give_images(&mut page, &[id], 20, 30),
            update(1, shaped, 0),
            "{id}"
        );
    }
}

/// A new bitmap size (its `width` or `height` changing) builds the
/// canvas's box again, as big as the new size.
#[test]
fn a_canvas_takes_its_new_size() {
    let mut page = Page::new("<p>Text <canvas id=c width=20 height=10></canvas> more</p>");
    let size = |page: &Page| fragments(&page.view(), page)["c"].size;
    assert_eq!(size(&page), SizeF::new(20.0, 10.0));
    let did = set(&mut page, "c", "width", "40");
    // Built again, and the inline content it is in shaped again.
    assert_eq!(did, update(1, 1, 0));
    assert_eq!(size(&page), SizeF::new(40.0, 10.0));
}

#[test]
fn a_replaced_box_has_no_content() {
    let mut page = Page::new("<img id=i style='display: block; width: 10px; height: 10px'>");
    page.change(|cx, document| {
        let text = cx.create_text(document, "not rendered");
        cx.append_child(find(cx, document, "i"), text).unwrap();
    });
}

#[test]
fn the_element_whose_overflow_went_to_the_viewport_keeps_its_own_visible() {
    let mut page = Page::new("<body id=b style='overflow: hidden'><p>Text</p></body>");
    assert_eq!(
        set_style(&mut page, "b", "overflow: hidden; background-color: green"),
        update(0, 0, 1)
    );
    assert_eq!(
        set_style(&mut page, "b", "overflow: hidden; margin: 20px"),
        update(0, 0, 1)
    );
}

#[test]
fn what_the_whole_tree_depends_on_builds_everything() {
    let mut page = Page::new("<html id=html><body id=b><p>Text</p></body></html>");
    let full = |update: BoxTreeUpdate| update.full;
    // What goes to the viewport.
    assert!(full(set_style(&mut page, "b", "overflow: hidden")));
    assert!(full(set_style(&mut page, "html", "overflow: hidden")));
    // The root's box.
    assert!(full(set_style(&mut page, "html", "display: none")));
    assert!(full(set_style(&mut page, "html", "display: block")));
    // `<body>`.
    assert!(full(page.change(|cx, document| {
        let body = cx.create_element(document, html("body"));
        let (html, b) = (find(cx, document, "html"), find(cx, document, "b"));
        cx.pre_insert(html, body, Some(b)).unwrap();
    })));
}

#[test]
fn two_changes_in_one_block_shape_it_once() {
    let mut page = Page::new("<p><span id=a>One</span> <span id=b>two</span></p>");
    assert_eq!(
        page.change(|cx, document| {
            for id in ["a", "b"] {
                cx.set_attribute(find(cx, document, id), "style", "font-size: 30px");
            }
        }),
        update(0, 1, 0)
    );
}

#[test]
fn a_block_image_takes_its_new_size() {
    let mut page =
        Page::new("<img id=i style='display: block; width: 10px; height: 10px'><p>After</p>");
    assert_eq!(
        set_style(
            &mut page,
            "i",
            // An `auto` width is its natural one (none, without an image),
            // not the containing block's as for other blocks.
            "display: block; height: 15px; padding: 3px"
        ),
        update(0, 0, 1)
    );
}

/// Moved within its parent, a block gets its new box before the old one
/// goes: the next change finds the new one.
#[test]
fn a_box_built_again_is_found_again() {
    let mut page = Page::new("<div id=d><p id=a>A</p><p id=b>B</p></div>");
    assert_eq!(
        page.change(|cx, document| {
            let (d, a, b) = (
                find(cx, document, "d"),
                find(cx, document, "a"),
                find(cx, document, "b"),
            );
            cx.pre_insert(d, b, Some(a)).unwrap();
        }),
        update(1, 1, 0)
    );
    assert_eq!(set_style(&mut page, "b", "color: red"), update(0, 0, 1));
}

#[test]
fn a_pseudo_elements_new_content_collects_its_box_again() {
    let mut page = Page::new(
        "<style>.x::before { content: 'b' } .y::after { content: 'a'; display: block }</style>\
         <p id=p>t</p><div id=d>u</div>",
    );
    set(&mut page, "p", "class", "x");
    set(&mut page, "d", "class", "y");
    set(&mut page, "p", "class", "");
}

const THREE: &str = "<body id=body><p id=one>One</p>\
    <p id=two>Two, long enough to wrap: the quick brown fox jumps over the lazy dog, \
    the quick brown fox jumps over the lazy dog, the quick brown fox jumps over the lazy dog, \
    the quick brown fox jumps over the lazy dog.</p><p id=three>Three</p></body>";

#[test]
fn nothing_changed_nothing_laid_out() {
    let mut page = Page::new(THREE);
    let before = page.view();
    page.update();
    assert_eq!(page.laid_out(), laid_out(0, 0));
    assert!(Arc::ptr_eq(&before, &page.view()));
}

#[test]
fn a_color_makes_fragments_only_on_the_way_down() {
    let mut page = Page::new(THREE);
    let before = page.view();
    set_style(&mut page, "two", "color: red");
    // `#two`, `<body>`, `<html>` and the `LayoutView`: nothing laid out,
    // and the lines as they were broken.
    assert_eq!(page.laid_out(), laid_out(0, 4));
    assert_eq!(kept(&page, &before), ["one", "three"]);
}

/// What only moves the box where it is drawn, or cuts it: a transform,
/// its origin and its reference box, `clip-path` and `clip`, make
/// fragments only, as a color does.
#[test]
fn a_transform_origin_makes_fragments_only() {
    for style in [
        "transform: rotate(10deg)",
        "transform-origin: 0 0",
        "transform-box: content-box",
        "clip-path: circle(10px)",
        "clip: rect(0, 1px, 2px, 3px)",
        "text-decoration: underline wavy red 3px",
        "text-underline-offset: 2px; text-underline-position: under",
        "text-shadow: 1px 1px red",
        "-webkit-text-fill-color: red",
        "filter: blur(1px)",
        "isolation: isolate",
    ] {
        let mut page = Page::new(THREE);
        set_style(&mut page, "two", style);
        assert_eq!(page.laid_out(), laid_out(0, 4), "{style}");
    }
}

/// A property machopo parses but doesn't act on yet, or that only the pointer
/// shows (`cursor`), lays nothing out again.
#[test]
fn a_property_parsed_only_lays_nothing_out() {
    for style in [
        "user-select: none",
        "-webkit-user-modify: read-write",
        "forced-color-adjust: none",
        "-webkit-tap-highlight-color: red",
        "text-size-adjust: none",
        "-webkit-font-smoothing: antialiased",
        "resize: both",
        "scroll-behavior: smooth",
        "border-image: linear-gradient(red, blue) 10 / 2px / 1px round",
        "cursor: url(a.png) 1 2, move",
    ] {
        let mut page = Page::new(THREE);
        set_style(&mut page, "two", style);
        assert_eq!(page.laid_out(), laid_out(0, 4), "{style}");
    }
}

/// What the caret and a selection are drawn in (`caret-color`, a
/// `::selection` rule) only paint: nothing is laid out again.
#[test]
fn the_caret_and_selection_colors_lay_nothing_out() {
    let mut page = Page::new(THREE);
    set_style(&mut page, "two", "caret-color: red");
    assert_eq!(page.laid_out(), laid_out(0, 4));
    let three = THREE.replace(
        "<body id=body>",
        "<style>.on::selection { background-color: red; text-shadow: 1px 1px red }</style><body id=body>",
    );
    // A page at a time: one token a thread.
    drop(page);
    let mut page = Page::new(&three);
    set(&mut page, "two", "class", "on");
    assert_eq!(page.laid_out(), laid_out(0, 4));
}

/// The inherited values that place the text, among those that only paint
/// (the text's shadow, its fill): laid out again, whether or not one that
/// only paints changed with them.
#[test]
fn a_text_indent_lays_out_again() {
    for style in [
        "text-indent: 20px",
        "letter-spacing: 2px",
        "text-rendering: optimizeSpeed",
        "hyphens: none",
        "text-indent: 20px; text-shadow: 1px 1px red",
        "hyphens: none; text-shadow: 1px 1px red",
    ] {
        let mut page = Page::new(THREE);
        set_style(&mut page, "two", style);
        assert_eq!(page.laid_out(), laid_out(4, 4), "{style}");
    }
}

/// A sticky box is laid out where the flow puts it: its insets move it
/// only after layout, as its scrollport scrolls.
#[test]
fn a_sticky_boxs_inset_makes_fragments_only() {
    let mut page = Page::new(THREE);
    set_style(&mut page, "two", "position: sticky; top: 10px");
    set_style(&mut page, "two", "position: sticky; top: 20%; bottom: 3px");
    assert_eq!(page.laid_out(), laid_out(0, 4));
    // Relatively positioned, its insets move it in layout: laid out again.
    set_style(
        &mut page,
        "two",
        "position: relative; top: 20%; bottom: 3px",
    );
    assert_ne!(page.laid_out(), laid_out(0, 4));
}

#[test]
fn a_margin_lays_out_the_box_and_its_ancestors() {
    let mut page = Page::new(THREE);
    let before = page.view();
    set_style(&mut page, "two", "margin-top: 50px");
    // The paragraphs around it are where they were, or only moved: their
    // fragments are kept, and `FragmentLink` puts them elsewhere.
    assert_eq!(page.laid_out(), laid_out(4, 4));
    assert_eq!(kept(&page, &before), ["one", "three"]);
}

#[test]
fn changed_text_lays_out_its_block() {
    let mut page = Page::new(THREE);
    let before = page.view();
    page.change(|cx, document| cx.set_data(first_child(cx, document, "two"), "Short now"));
    assert_eq!(page.laid_out(), laid_out(4, 4));
    assert_eq!(kept(&page, &before), ["one", "three"]);
}

#[test]
fn a_removed_box_lays_out_its_parent() {
    let mut page = Page::new(THREE);
    let before = page.view();
    page.change(|cx, document| cx.remove(find(cx, document, "two")));
    assert_eq!(page.laid_out(), laid_out(3, 3));
    assert_eq!(kept(&page, &before), ["one", "three"]);
}

#[test]
fn another_viewport_lays_out_what_it_changes() {
    let mut page = Page::new(THREE);
    for viewport in [SizeF::new(300.0, 600.0), SizeF::new(300.0, 200.0), VIEWPORT] {
        page.viewport = viewport;
        page.check();
        assert_eq!(page.view().size, viewport);
    }
    // Only the height: what fills the width is laid out as before, even
    // `<body>`, whose height is its content's.
    let before = page.view();
    page.viewport = SizeF::new(800.0, 300.0);
    page.check();
    assert_eq!(kept(&page, &before), ["body", "one", "three", "two"]);
}

/// The ids of the elements whose fragments are `id`'s children, in order;
/// `-` for anonymous boxes.
fn child_ids(page: &Page, id: &str) -> Vec<String> {
    let view = page.view();
    let fragment = fragments(&view, page)[id].clone();
    let ids = page.ids();
    fragment
        .children
        .iter()
        .map(|child| match child.fragment.node {
            Some(node) => ids.get(&node).cloned().unwrap_or_default(),
            None => "-".into(),
        })
        .collect()
}

const FLEX: &str = "<div id=c style='display: flex'>\
    <p id=a>A</p><p id=b>B</p><p id=d>D</p></div><p id=after>After</p>";

#[test]
fn an_items_order_only_lays_its_container_out_again() {
    let mut page = Page::new(FLEX);
    let before = page.view();
    // Restyled in place, shaping its own text again as a size would: no
    // box is built, and the box tree keeps document order.
    assert_eq!(set_style(&mut page, "b", "order: -1"), update(0, 1, 1));
    assert_eq!(child_ids(&page, "c"), ["b", "a", "d"]);
    // `#b` and the boxes it is in; the other items are where they were,
    // or only moved.
    assert_eq!(page.laid_out(), laid_out(5, 5));
    assert_eq!(kept(&page, &before), ["a", "after", "d"]);
    // Back in document order: no sorted list left behind.
    set_style(&mut page, "b", "order: 0");
    assert_eq!(child_ids(&page, "c"), ["a", "b", "d"]);
}

#[test]
fn an_item_inserted_or_removed_goes_where_its_order_says() {
    let mut page = Page::new(FLEX);
    set_style(&mut page, "d", "order: -1");
    page.change(|cx, document| {
        let p = element(cx, document, "p", &[("id", "new")]);
        cx.append_child(find(cx, document, "c"), p).unwrap();
    });
    assert_eq!(child_ids(&page, "c"), ["d", "a", "b", "new"]);
    page.change(|cx, document| cx.remove(find(cx, document, "a")));
    assert_eq!(child_ids(&page, "c"), ["d", "b", "new"]);
}

#[test]
fn a_containers_display_builds_its_items() {
    let mut page = Page::new("<div id=c>Text <span id=s>span</span><p id=p>p</p></div>");
    // Its children, blockified, are built again with it: `#c`, an
    // anonymous item around "Text ", `#s` and `#p`.
    assert_eq!(set_style(&mut page, "c", "display: flex"), update(4, 3, 0));
    assert_eq!(child_ids(&page, "c"), ["-", "s", "p"]);
    // And back: one anonymous block around "Text " and `#s`.
    assert_eq!(set_style(&mut page, "c", "display: block"), update(3, 2, 0));
    assert_eq!(child_ids(&page, "c"), ["-", "p"]);
}

#[test]
fn text_in_a_flex_container_is_an_anonymous_item() {
    let mut page = Page::new(FLEX);
    // Collected again: an anonymous item around the new text, shaped.
    assert_eq!(
        page.change(|cx, document| {
            let text = cx.create_text(document, "Text");
            let (c, b) = (find(cx, document, "c"), find(cx, document, "b"));
            cx.pre_insert(c, text, Some(b)).unwrap();
        }),
        update(1, 1, 0)
    );
    assert_eq!(child_ids(&page, "c"), ["a", "-", "b", "d"]);
    // A flex container's text: only white space makes no item.
    page.change(|cx, document| {
        let a = find(cx, document, "a");
        cx.set_data(cx.next_sibling(a).unwrap(), "  ");
    });
    assert_eq!(child_ids(&page, "c"), ["a", "b", "d"]);
}

#[test]
fn an_image_flex_item_is_sized_by_taffy() {
    let mut page = Page::new(
        "<div id=c style='display: flex; height: 60px'><img id=item><img id=padded style='padding: 5px'></div>
         <div style='display: flex; width: 0'><img id=shrunk style='height: 30px'></div>
         <div style='height: 200px'><img id=block style='display: block; height: 50%'></div>
         <img id=padded-block style='display: block; width: 40px; padding: 5px'>
         <div style='display: grid; grid-template-rows: 60px'><img id=grid-item></div>
         <div style='display: grid; grid-template-rows: 60px; grid-template-columns: 50px;
           place-items: stretch'><img id=grid-stretched></div>
         <div style='display: grid; grid-template: 60px / 50px'>
           <img id=grid-wide style='justify-self: stretch'><img id=grid-tall style='align-self: stretch; grid-row: 1'></div>
         <span style='display: inline-grid; grid-template-rows: 60px'><img id=inline-grid-item></span>",
    );
    give_images(
        &mut page,
        &[
            "item",
            "padded",
            "shrunk",
            "block",
            "padded-block",
            "grid-item",
            "grid-stretched",
            "grid-wide",
            "grid-tall",
            "inline-grid-item",
        ],
        20,
        40,
    );
    let view = page.view();
    let laid = fragments(&view, &page);
    let size = |id: &str| laid[id].size;
    // Stretched to 60px high, as wide as the ratio says.
    assert_eq!(size("item"), SizeF::new(30.0, 60.0));
    // The ratio is the content box's: 50px high, 25px wide.
    assert_eq!(size("padded"), SizeF::new(35.0, 60.0));
    // No narrower than its natural width at that height.
    assert_eq!(size("shrunk"), SizeF::new(15.0, 30.0));
    // Outside a flex container, a percentage height is still `auto`.
    assert_eq!(size("block"), SizeF::new(20.0, 40.0));
    // The ratio is the content box's there too.
    assert_eq!(size("padded-block"), SizeF::new(50.0, 90.0));
    // A grid item with a natural size isn't stretched: it keeps it.
    assert_eq!(size("grid-item"), SizeF::new(20.0, 40.0));
    // Unless its alignment says otherwise.
    assert_eq!(size("grid-stretched"), SizeF::new(50.0, 60.0));
    // Stretched on one axis, the other follows through the ratio.
    assert_eq!(size("grid-wide"), SizeF::new(50.0, 100.0));
    assert_eq!(size("grid-tall"), SizeF::new(30.0, 60.0));
    // In an inline-level grid too.
    assert_eq!(size("inline-grid-item"), SizeF::new(20.0, 40.0));
    // Restyled in place, each stays what it was.
    set_style(&mut page, "item", "margin-left: 1px");
    set_style(
        &mut page,
        "block",
        "display: block; height: 50%; margin-left: 1px",
    );
    let view = page.view();
    assert_eq!(
        fragments(&view, &page)["block"].size,
        SizeF::new(20.0, 40.0)
    );
    assert_eq!(fragments(&view, &page)["item"].size, SizeF::new(30.0, 60.0));
}

#[test]
fn a_z_index_only_paints_again() {
    let mut page = Page::new(FLEX);
    let before = page.view();
    assert_eq!(set_style(&mut page, "b", "z-index: 1"), update(0, 0, 1));
    // Nothing laid out: `#b`'s fragment and those it is in are made again,
    // for its new style.
    assert_eq!(page.laid_out(), laid_out(0, 5));
    assert_eq!(kept(&page, &before), ["a", "after", "d"]);
}

/// A box measured anew for an intrinsic size but laid out as it was keeps
/// its fragment: only being laid out makes a new one.
#[test]
fn a_box_only_measured_keeps_its_fragment() {
    let mut page = Page::new(
        "<div id=c style='width: 100px'>\
         <div id=b style='width: 100px; height: 10px'>Text</div></div>",
    );
    let before = page.view();
    // `#c` asks `#b` its min-content width, which it wasn't asked before:
    // 100px, so `#c` is as wide as it was, and `#b` is laid out from the
    // cache.
    set_style(&mut page, "c", "width: min-content");
    assert_eq!(page.view().children[0].fragment.size.width, 800.0);
    assert_eq!(kept(&page, &before), ["b"]);
    // `#c`, `<body>`, `<html>` and the `LayoutView`.
    assert_eq!(page.laid_out(), laid_out(4, 4));
}

/// A percentage width is cyclic in an intrinsic size: the image counts at
/// its natural width for the max-content width, at 0 for the min-content
/// one; then it takes its percentage of the width that gives.
#[test]
fn an_images_percentage_width_in_an_intrinsic_size() {
    let mut page = Page::new(
        "<style>body { margin: 0 }</style>
         <div id=fit style='width: fit-content'><img id=a width=50%></div>
         <div id=max style='width: max-content'><img id=b width=50%></div>
         <div id=min style='width: min-content'><img id=c width=50%></div>",
    );
    give_images(&mut page, &["a", "b", "c"], 20, 40);
    let view = page.view();
    let laid = fragments(&view, &page);
    assert_eq!(laid["fit"].size.width, 20.0);
    assert_eq!(laid["max"].size.width, 20.0);
    assert_eq!(laid["min"].size.width, 0.0);
    // Half of 20px, at the natural ratio.
    assert_eq!(laid["a"].size, SizeF::new(10.0, 20.0));
    assert_eq!(laid["b"].size, SizeF::new(10.0, 20.0));
}

/// A grid item placed elsewhere is laid out again with its container; no
/// box is built.
#[test]
fn a_grid_items_placement_only_lays_its_container_out_again() {
    let mut page = Page::new(
        "<div id=c style='display: grid; grid-template-columns: 50px 50px'>\
         <p id=a>A</p><p id=b>B</p></div><p id=after>After</p>",
    );
    assert_eq!(set_style(&mut page, "a", "grid-column: 2"), update(0, 1, 1));
    let view = page.view();
    let laid = fragments(&view, &page);
    let offset = |id: &str| {
        let c = &laid["c"];
        c.children
            .iter()
            .find(|child| Arc::ptr_eq(&child.fragment, &laid[id]))
            .unwrap()
            .offset
    };
    assert_eq!(offset("a").x, 50.0);
    // `#b` goes on to the next row, the first one being full from column 2.
    assert_eq!(offset("b").x, 0.0);
    assert!(offset("b").y > 0.0);
}

/// What changes in an inline-block lays the line it is on out again,
/// without collecting the inline content around it again; a new color
/// only paints.
#[test]
fn an_inline_block_changes_in_place() {
    let mut page = Page::new(
        "<div id=d style='font: 20px/1 Ahem'>A<span id=ib style='display: inline-block'>B</span>\
         <span id=s>C</span></div><p id=after>After</p>",
    );
    // Its own content is collected and shaped again, not the line's.
    let grown = page.change(|cx, document| {
        let text = cx.create_text(document, "B");
        cx.append_child(find(cx, document, "ib"), text).unwrap();
    });
    assert_eq!(grown, update(0, 1, 0));
    assert_eq!(
        set_style(&mut page, "ib", "display: inline-block; width: 100px"),
        update(0, 1, 1)
    );
    // Taller, the line is too.
    assert_eq!(
        set_style(
            &mut page,
            "ib",
            "display: inline-block; width: 100px; height: 50px"
        ),
        update(0, 1, 1)
    );
    let before = page.view();
    assert_eq!(
        set_style(
            &mut page,
            "ib",
            "display: inline-block; width: 100px; height: 50px; background-color: red"
        ),
        update(0, 0, 1)
    );
    // Nothing laid out: its fragment and those it is in are made again.
    assert_eq!(page.laid_out(), laid_out(0, 5));
    assert_eq!(kept(&page, &before), ["after"]);
    // Block-level, it splits the line into two anonymous blocks.
    assert_eq!(
        set_style(&mut page, "ib", "display: block"),
        update(3, 3, 0)
    );
}

/// An inline-block kept while what is around it is collected again goes
/// to the anonymous block made for it, and is laid out there.
#[test]
fn a_kept_inline_block_moves_to_the_new_anonymous_block() {
    let mut page = Page::new(
        "<div id=d style='font: 20px/1 Ahem'><b id=b>A</b><span id=ib style='display: inline-block'>B</span>         <div>block</div></div>",
    );
    let before = page.view();
    // `<b>` takes another font size: the line is collected again, the
    // inline-block's box kept.
    assert_eq!(
        set_style(&mut page, "b", "font-size: 40px"),
        update(1, 1, 0)
    );
    assert!(kept(&page, &before).contains(&"ib".to_owned()));
    // And again: its box is still in use.
    assert_eq!(
        set_style(&mut page, "b", "font-size: 30px"),
        update(1, 1, 0)
    );
}

/// An inline image with a percentage width takes the new width of its
/// line when the viewport changes, with nothing changed in the DOM.
#[test]
fn an_inline_images_percentage_follows_the_viewport() {
    let mut page = Page::new("<style>body { margin: 0 }</style><p>A <img id=i width=50%></p>");
    give_images(&mut page, &["i"], 20, 40);
    for (viewport, width) in [(400.0, 200.0), (800.0, 400.0)] {
        page.viewport = SizeF::new(viewport, 600.0);
        page.check();
        let view = page.view();
        assert_eq!(fragments(&view, &page)["i"].size.width, width);
    }
}

/// An inline-block's percentage height follows its containing block's
/// height when the viewport changes it, with nothing changed in the DOM.
#[test]
fn an_inline_blocks_percentage_height_follows_the_viewport() {
    let mut page = Page::new(
        "<style>html, body { margin: 0; height: 100% }</style>
         <div style='height: 50%'><span id=ib style='display: inline-block; height: 50%'></span></div>",
    );
    for (viewport, height) in [(400.0, 100.0), (600.0, 150.0)] {
        page.viewport = SizeF::new(800.0, viewport);
        page.check();
        let view = page.view();
        assert_eq!(fragments(&view, &page)["ib"].size.height, height);
    }
}

/// Moving a relatively positioned box lays it out again, the boxes around
/// it as they were; its `z-index` only paints.
#[test]
fn a_relative_offset_moves_only_that_box() {
    let mut page = Page::new(
        "<div id=d style='font: 20px/1 Ahem'><p id=a style='position: relative'>A</p><p id=b>B</p></div>",
    );
    let before = page.view();
    assert_eq!(
        set_style(&mut page, "a", "position: relative; top: 5px"),
        update(0, 1, 1)
    );
    assert_eq!(kept(&page, &before), ["b"]);
    let before = page.view();
    assert_eq!(
        set_style(&mut page, "a", "position: relative; top: 5px; z-index: 3"),
        update(0, 0, 1)
    );
    // Nothing laid out: `#a`'s fragment and those it is in are made again.
    assert_eq!(page.laid_out(), laid_out(0, 5));
    assert_eq!(kept(&page, &before), ["b"]);
}

/// An absolutely positioned box's new inset lays its containing block out
/// again, not only the box: the containing block
/// is what places it. What is in flow next to it keeps its fragment.
#[test]
fn an_absolute_box_lays_its_containing_block_out_again() {
    let mut page = Page::new(
        "<div id=cb style='position: relative; height: 100px'>
           <p id=flow style='font: 20px/1 Ahem'>A</p>
           <div id=deep><div id=abs style='position: absolute; top: 5px'></div></div>
         </div>
         <p id=after style='font: 20px/1 Ahem'>B</p>",
    );
    let before = page.view();
    assert_eq!(
        set_style(&mut page, "abs", "position: absolute; top: 25px"),
        update(0, 0, 1)
    );
    // The containing block and the box were laid out again; what is in
    // flow around them kept its fragment.
    assert_eq!(kept(&page, &before), ["after", "flow"]);
    // Taking a box out of the flow is a change of the box tree: it is
    // built again.
    let height = |page: &Page| {
        let view = page.view();
        fragments(&view, page)["deep"].size.height
    };
    let before_height = height(&page);
    assert_eq!(
        set_style(&mut page, "deep", "position: absolute; height: 10px"),
        // Built again: the box and the one in it, whose static position
        // is in the inline content it holds alone.
        update(2, 1, 0)
    );
    assert_ne!(height(&page), before_height, "out of the flow");
}

/// The content of the `<svg>` box in `view`.
fn svg_content(view: &BoxFragment) -> Option<Arc<layout::SvgFragment>> {
    view.svg.clone().or_else(|| {
        view.children
            .iter()
            .find_map(|child| svg_content(&child.fragment))
    })
}

/// The content of each `<svg>` box in `view`, in order.
fn all_svg_content(view: &BoxFragment) -> Vec<Arc<layout::SvgFragment>> {
    fn all(f: &BoxFragment, out: &mut Vec<Arc<layout::SvgFragment>>) {
        out.extend(f.svg.clone());
        for child in &f.children {
            all(&child.fragment, out);
        }
    }
    let mut svgs = Vec::new();
    all(view, &mut svgs);
    svgs
}

/// What a `<use>` copies changed, in another `<svg>`: the `<svg>` the
/// `<use>` is in takes its content again (the copy's styles are new).
#[test]
fn an_svg_takes_its_content_again_when_what_its_use_copies_changes() {
    let mut page = Page::new(
        "<svg id=defs><rect id=r width=10 height=10 fill=red /></svg>\
         <svg id=s width=100 height=50><use id=u href=#r /></svg>",
    );
    let fill = |page: &Page| {
        let svg = all_svg_content(&page.view())[1].clone();
        let layout::SvgNodeKind::Container(copy) = &svg.children[0].kind else {
            panic!("the <use>");
        };
        copy[0].style.svg.fill.clone()
    };
    let before = fill(&page);
    set(&mut page, "r", "fill", "green");
    assert_ne!(fill(&page), before);
}

/// A `linearGradient` with the id `later` and a stop of `color`, at the
/// end of `#defs`.
fn later_gradient<'gc>(cx: &Cx<'_, 'gc>, document: NodeRef<'gc>, color: &str) -> NodeRef<'gc> {
    let gradient = cx.create_element(document, svg("linearGradient"));
    cx.set_attribute(gradient, "id", "later");
    let stop = cx.create_element(document, svg("stop"));
    cx.set_attribute(stop, "stop-color", color);
    cx.append_child(gradient, stop).unwrap();
    cx.append_child(find(cx, document, "defs"), gradient)
        .unwrap();
    gradient
}

/// What a shape's `url()` reaches changed, in another `<svg>`: a stop, the
/// gradient's attributes or `href`, an element given the id later; the
/// shape's `<svg>` takes its content again (and lays out as a new tree
/// does, `check`).
#[test]
fn an_svg_takes_its_content_again_when_its_gradients_change() {
    let mut page = Page::new(
        "<svg id=defs><linearGradient id=g><stop id=s stop-color=red /><stop offset=1 stop-color=red /></linearGradient>\
         <linearGradient id=h href=#other /><linearGradient id=other><stop stop-color=red /></linearGradient></svg>\
         <svg id=s width=100 height=50><rect width=10 height=10 fill='url(#g)' />\
         <rect width=10 height=10 fill='url(#h)' /><rect width=10 height=10 fill='url(#later) red' /></svg>",
    );
    let servers = |page: &Page| {
        all_svg_content(&page.view())[1]
            .children
            .iter()
            .map(|node| match &node.kind {
                layout::SvgNodeKind::Shape(shape) => shape.fill_server.clone(),
                _ => panic!("a shape"),
            })
            .collect::<Vec<_>>()
    };
    let before = servers(&page);
    set(&mut page, "s", "stop-color", "blue");
    let after = servers(&page);
    assert_ne!(after[0], before[0], "the stop's color");
    set(&mut page, "g", "x2", "0.5");
    assert_ne!(servers(&page)[0], after[0], "an attribute");
    set(&mut page, "h", "href", "#g");
    assert!(
        matches!(servers(&page)[1], Some(layout::SvgPaintServer::Gradient(_))),
        "another href"
    );
    assert_eq!(servers(&page)[2], None, "no #later yet");
    set(&mut page, "other", "id", "later");
    assert!(
        matches!(servers(&page)[2], Some(layout::SvgPaintServer::Color(_))),
        "an id given later"
    );
    // The element goes: the fallback, with nothing on the chain touched.
    page.change(|cx, document| cx.remove(find(cx, document, "later")));
    assert_eq!(servers(&page)[2], None, "gone");
    // Of two with the id, the first goes: the second, untouched, is found.
    let first = page
        .dom
        .mutate(|cx| cx.root(later_gradient(cx, page.document.get(cx.gc), "lime")));
    page.update();
    page.change(|cx, document| {
        later_gradient(cx, document, "blue");
    });
    let lime = servers(&page)[2].clone();
    assert_eq!(
        lime,
        Some(layout::SvgPaintServer::Color(gfx::Color::rgb(0, 255, 0)))
    );
    page.change(|cx, _| cx.remove(first.get(cx.gc)));
    assert_eq!(
        servers(&page)[2],
        Some(layout::SvgPaintServer::Color(gfx::Color::rgb(0, 0, 255)))
    );
}

/// A `<use>`'s copy paints with a gradient that changes: the `<svg>` the
/// `<use>` is in takes its content again.
#[test]
fn an_svg_takes_its_content_again_when_a_copys_gradient_changes() {
    let mut page = Page::new(
        "<svg id=defs><linearGradient id=g><stop id=s stop-color=red /><stop offset=1 stop-color=red /></linearGradient>\
         <rect id=r width=10 height=10 fill=red /></svg>\
         <svg width=100 height=50><use href=#r /></svg>",
    );
    // The copy paints with the gradient only once copied again.
    set(&mut page, "r", "fill", "url(#g)");
    let copy_server = |page: &Page| {
        let svgs = all_svg_content(&page.view());
        let layout::SvgNodeKind::Container(copy) = &svgs[1].children[0].kind else {
            panic!("the <use>");
        };
        match &copy[0].kind {
            layout::SvgNodeKind::Shape(shape) => shape.fill_server.clone(),
            _ => panic!("a shape"),
        }
    };
    let before = copy_server(&page);
    set(&mut page, "s", "stop-color", "blue");
    assert_ne!(copy_server(&page), before);
}

/// A `<text>`'s characters change, or a `<tspan>`'s position: the `<svg>`
/// takes its content again.
#[test]
fn an_svg_takes_its_content_again_when_its_text_changes() {
    let mut page =
        Page::new("<svg width=100 height=50><text id=t y=10>ab<tspan id=s>c</tspan></text></svg>");
    let cells = |page: &Page| match &svg_content(&page.view()).unwrap().children[0].kind {
        layout::SvgNodeKind::Text(text) => text.cells.iter().map(|c| c.origin).collect::<Vec<_>>(),
        _ => panic!("the text"),
    };
    assert_eq!(cells(&page).len(), 3);
    page.change(|cx, document| cx.set_data(first_child(cx, document, "t"), "abcd"));
    assert_eq!(cells(&page).len(), 5);
    let before = cells(&page);
    set(&mut page, "s", "dy", "5");
    assert_ne!(cells(&page), before);
}

/// Anything changed in an `<svg>`'s content takes it again whole, attributes
/// that aren't style (`x`, `viewBox`) included, and lays out the same as a
/// new tree (`check`).
#[test]
fn an_svg_takes_its_content_again() {
    let mut page = Page::new(
        "<svg id=s width=100 height=50><g id=g><rect id=r width=10 height=10 /></g></svg>",
    );
    let before = svg_content(&page.view()).expect("svg content");
    set(&mut page, "r", "x", "20");
    let moved = svg_content(&page.view()).unwrap();
    assert_ne!(before, moved, "the rect moved");
    // A new shape.
    page.change(|cx, document| {
        let circle = cx.create_element(document, svg("circle"));
        cx.set_attribute(circle, "r", "5");
        cx.append_child(find(cx, document, "g"), circle).unwrap();
    });
    let layout::SvgNodeKind::Container(children) =
        &svg_content(&page.view()).unwrap().children[0].kind
    else {
        panic!("the group");
    };
    assert_eq!(children.len(), 2);
    // A descendant's style.
    set_style(&mut page, "r", "display: none");
    // The `viewBox` gives the box a ratio: its size changes.
    page.change(|cx, document| {
        let s = find(cx, document, "s");
        cx.remove_attribute(s, "height");
        cx.set_attribute(s, "viewBox", "0 0 2 1");
    });
    // Its natural size is its `width` attribute's even where its CSS
    // isn't: the attribute alone changing changes its size.
    set_style(&mut page, "s", "display: block; width: auto; height: auto");
    let size = |page: &Page| {
        let view = page.view();
        fragments(&view, page)["s"].size
    };
    assert_eq!(size(&page), SizeF::new(100.0, 50.0));
    set(&mut page, "s", "width", "60");
    assert_eq!(size(&page), SizeF::new(60.0, 30.0));
}

/// What is in an `<svg>` is captured again whatever changes in it: an
/// attribute that is geometry (not style), a presentation attribute, a
/// class, an element added, text in a `<text>`, its `viewBox`; each lays
/// out as building it all again does.
#[test]
fn a_change_in_an_svg_captures_it_again() {
    let mut page = Page::new(
        "<style>.big { stroke-width: 9 }</style>\
         <svg id=s width=200 height=100><g id=g><rect id=r width=10 height=10 fill=red/>\
         <polygon id=p points='0,0 10,0 10,10'/><text id=t x=5 y=50>a</text></g></svg><p>after</p>",
    );
    set(&mut page, "p", "points", "0,0 50,0 50,50");
    set(&mut page, "r", "width", "80");
    set(&mut page, "r", "class", "big");
    page.change(|cx, document| {
        let circle = cx.create_element(document, svg("circle"));
        cx.set_attribute(circle, "r", "20");
        cx.append_child(find(cx, document, "g"), circle).unwrap();
    });
    page.change(|cx, document| cx.set_data(first_child(cx, document, "t"), "longer text"));
    set(&mut page, "s", "viewBox", "0 0 50 25");
}

/// What draws with what an id found follows it, in another `<svg>` too,
/// and only it is captured again: a gradient's stop, its attribute, its
/// `href`, an id given later, one before it in tree order, one gone; a
/// `<clipPath>` an HTML box and an `<svg>` are clipped to; a `<use>`'s
/// copy (each lays out as building it all again does).
#[test]
fn what_an_id_found_is_followed() {
    let mut page = Page::new(
        "<svg id=defs width=0 height=0><linearGradient id=g1><stop id=s stop-color=red />\
         <stop offset=1 stop-color=lime /></linearGradient><linearGradient id=g3 href='#g4'/>\
         <linearGradient id=g4><stop stop-color=lime /></linearGradient>\
         <clipPath id=c><rect id=cr width=10 height=10 /></clipPath><rect id=r width=5 height=5 /></svg>\
         <svg id=a width=100 height=50><rect id=plain width=10 height=10 /></svg>\
         <svg id=b width=100 height=50><rect width=10 height=10 fill='url(#g1)'/>\
         <rect width=10 height=10 fill='url(#later) lime'/><rect width=10 height=10 fill='url(#g3)'/></svg>\
         <div id=clipped style='width: 40px; height: 40px; background: blue; clip-path: url(#c)'></div>\
         <svg id=u width=100 height=50 style='clip-path: url(#c)'><use href=#r /></svg>",
    );
    let captured = |update: BoxTreeUpdate| update.svg_captured;
    // Nothing found by id in it: only it.
    assert_eq!(captured(set(&mut page, "plain", "width", "20")), 1);
    // What `#b` found (`#defs` is captured for its own content).
    assert_eq!(captured(set(&mut page, "s", "stop-color", "blue")), 2);
    set(&mut page, "g1", "x2", "0.5");
    set(&mut page, "g3", "href", "#g1");
    let update = page.change(|cx, document| {
        let later = cx.create_element(document, svg("linearGradient"));
        cx.set_attribute(later, "id", "later");
        cx.set_attribute(later, "href", "#g4");
        cx.append_child(find(cx, document, "defs"), later).unwrap();
    });
    assert_eq!(captured(update), 2);
    // Another element with the id first, then none.
    page.change(|cx, document| {
        let first = cx.create_element(document, svg("radialGradient"));
        cx.set_attribute(first, "id", "g1");
        let defs = find(cx, document, "defs");
        cx.insert(defs, first, cx.first_child(defs));
    });
    page.change(|cx, document| {
        let defs = find(cx, document, "defs");
        cx.remove(cx.first_child(defs).unwrap());
        cx.remove(find(cx, document, "g1"));
    });
    // The `<clipPath>`: the HTML box's and `#u`'s clip.
    assert_eq!(
        captured(set(&mut page, "cr", "width", "30")),
        2,
        "`#defs` and `#u`"
    );
    // A `<use>`'s copy.
    set(&mut page, "r", "fill", "lime");
}

/// An out-of-flow box in inline content is a child of the box holding that
/// content: its new inset lays its containing block, above that box, out
/// again.
#[test]
fn an_absolute_box_in_inline_content_lays_its_containing_block_out_again() {
    let mut page = Page::new(
        "<div id=cb style='position: relative'>
           <p id=text style='font: 20px/1 Ahem'>A<span id=abs style='position: absolute;
             top: 5px'></span></p>
         </div>",
    );
    set_style(&mut page, "abs", "position: absolute; top: 25px");
    let view = page.view();
    let found = fragments(&view, &page);
    let link = found["cb"]
        .children
        .iter()
        .find(|link| Arc::ptr_eq(&link.fragment, &found["abs"]))
        .expect("the containing block holds it");
    assert_eq!(link.offset.y, 25.0);
}

/// Where box `id` is in its parent's fragment, after the last layout.
fn offset_in(page: &Page, parent: &str, id: &str) -> gfx::PointF {
    let view = page.view();
    let found = fragments(&view, page);
    found[parent]
        .children
        .iter()
        .find(|link| Arc::ptr_eq(&link.fragment, &found[id]))
        .map(|link| link.offset)
        .expect("a child")
}

/// A box that put floats into the block formatting context it shares is
/// laid out again whenever its parent is, not taken from taffy's cache,
/// which wouldn't put them there: what comes after keeps avoiding them.
#[test]
fn a_box_holding_floats_puts_them_there_again() {
    let mut page = Page::new(
        "<style>body { margin: 0 }</style>
         <div id=box style='width: 300px'>
           <div id=wrapper><div style='float: left; width: 100px; height: 50px'></div></div>
           <div id=bfc style='overflow: hidden; height: 10px'></div>
         </div>",
    );
    assert_eq!(offset_in(&page, "box", "bfc").x, 100.0);
    set_style(&mut page, "bfc", "overflow: hidden; height: 20px");
    assert_eq!(offset_in(&page, "box", "bfc").x, 100.0, "still beside it");
}

/// A box next to floats is laid out again whenever its parent is: the
/// floats around it may have changed, which taffy's cache doesn't know.
#[test]
fn a_box_next_to_floats_follows_them() {
    let mut page = Page::new(
        "<style>body { margin: 0 }</style>
         <div id=box style='width: 300px'>
           <div id=float style='float: left; width: 100px; height: 50px'></div>
           <div id=wrapper><div id=bfc style='overflow: hidden; height: 10px'></div></div>
         </div>",
    );
    assert_eq!(offset_in(&page, "wrapper", "bfc").x, 100.0);
    set_style(
        &mut page,
        "float",
        "float: left; width: 150px; height: 50px",
    );
    assert_eq!(offset_in(&page, "wrapper", "bfc").x, 150.0);
}

/// Lines beside a float that grows are broken again beside it: the floats
/// are part of what their cache keys on.
#[test]
fn lines_follow_a_float_that_changes() {
    let mut page = Page::new(
        "<style>body { margin: 0 }</style>
         <div style='width: 200px'>
           <div id=float style='float: left; width: 100px; height: 20px'></div>
           <p id=text style='font: 20px/1 Ahem; margin: 0'>XX XX XX XX XX XX</p>
         </div>",
    );
    let starts = |page: &Page| {
        let view = page.view();
        fragments(&view, page)["text"]
            .lines
            .iter()
            .map(|line| line.rect.x)
            .collect::<Vec<f32>>()
    };
    assert_eq!(starts(&page), [100.0, 0.0, 0.0]);
    set_style(
        &mut page,
        "float",
        "float: left; width: 100px; height: 40px",
    );
    assert_eq!(starts(&page), [100.0, 100.0, 0.0]);
}

/// A block that comes to float after inline content is built again as a
/// float in that content's line, beside it, not a block between two
/// anonymous ones.
#[test]
fn a_block_that_floats_goes_into_the_line_before_it() {
    let mut page = Page::new(
        "<style>body { margin: 0 }</style>
         <div id=d style='font: 20px/1 Ahem; width: 200px'>XX<div id=b
           style='width: 40px; height: 20px'></div>YY</div>",
    );
    set_style(&mut page, "b", "float: left; width: 40px; height: 20px");
    let view = page.view();
    let lines = &fragments(&view, &page)["d"].lines;
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].rect.x, 40.0);
}

/// A float that comes after inline content once a block between them is
/// gone goes into the anonymous block made for that content, beside its
/// line: its box is kept, not dropped with the box it was a child of.
#[test]
fn a_float_goes_into_the_line_a_removed_block_was_between() {
    let mut page = Page::new(
        "<style>body { margin: 0 }</style>
         <div id=outer style='width: 200px'><div style='height: 50px'></div><div
           style='display: inline-block; width: 100px; height: 150px'></div><div id=gone
           style='height: 100px'></div><div id=float
           style='float: left; width: 100px; height: 150px'></div></div>",
    );
    set_style(&mut page, "gone", "display: none");
    let view = page.view();
    let anonymous = fragments(&view, &page)["outer"].children[1]
        .fragment
        .clone();
    let float = anonymous
        .children
        .iter()
        .find(|child| child.fragment.floating)
        .expect("the float is in the anonymous block");
    assert_eq!((float.offset.x, float.offset.y), (0.0, 0.0));
    assert_eq!(anonymous.lines[0].rect.x, 100.0);
}

/// A pseudo-element that comes with a class is built in its element's
/// content, a block's and an inline element's, and goes with it; its
/// `attr()` text follows the attribute. Each update lays out as building
/// the tree again does (`Page::check`).
#[test]
fn pseudo_elements_follow_their_rules_and_attributes() {
    let mut page = Page::new(
        "<style>body { margin: 0 } div { font: 20px/1 Ahem; width: 400px }
           .on::before { content: 'A' attr(data-x) } .block::after { content: ''; display: block; height: 10px }</style>
         <div id=d data-x=B>XX<span id=s data-x=C>YY</span></div>",
    );
    let width = |page: &Page| {
        let view = page.view();
        fragments(&view, page)["d"].lines[0].rect.width
    };
    assert_eq!(width(&page), 80.0);
    set(&mut page, "d", "class", "on");
    assert_eq!(width(&page), 120.0);
    // Styled again to the same pseudo-element styles: nothing to do.
    assert_eq!(set(&mut page, "d", "title", "t"), update(0, 0, 0));
    set(&mut page, "s", "class", "on");
    assert_eq!(width(&page), 160.0);
    set(&mut page, "s", "data-x", "CCC");
    assert_eq!(width(&page), 200.0);
    set(&mut page, "d", "class", "on block");
    let view = page.view();
    assert_eq!(fragments(&view, &page)["d"].size.height, 30.0);
    set(&mut page, "d", "class", "");
    assert_eq!(width(&page), 160.0);
}

/// The marker text of the element whose `id` is `id`.
fn marker(page: &Page, id: &str) -> String {
    page.tree
        .counter_text_for_testing(page.id(id), PseudoId::Marker)
        .unwrap_or_default()
}

/// The `::before` counter text of the element whose `id` is `id`.
fn before(page: &Page, id: &str) -> String {
    page.tree
        .counter_text_for_testing(page.id(id), PseudoId::Before)
        .unwrap_or_default()
}

/// The list items' markers follow the list: an item inserted or removed
/// numbers those after it again, as does a new `counter-set`, `value` or
/// `start`; a new `list-style-type` or `-position` makes the markers
/// again. Only the items whose marker changed are collected again (each
/// update lays out as building the tree again does, `Page::check`).
#[test]
fn markers_follow_their_list() {
    let mut page = Page::new(
        "<style>ol { font: 20px/1 Ahem }</style>
         <ol id=ol><li id=a>A<li id=b>B<li id=c>C</ol><p id=p>Text</p>",
    );
    let markers = |page: &Page| -> Vec<String> {
        let items = page.read(|cx, document| {
            let mut items = Vec::new();
            let mut child = cx.first_child(find(cx, document, "ol"));
            while let Some(c) = child {
                if cx.kind(c) == dom::NodeKind::Element {
                    items.push(cx.node_id(c));
                }
                child = cx.next_sibling(c);
            }
            items
        });
        items
            .into_iter()
            .map(|item| {
                page.tree
                    .counter_text_for_testing(item, PseudoId::Marker)
                    .unwrap_or_default()
            })
            .collect()
    };
    assert_eq!(markers(&page), ["1. ", "2. ", "3. "]);
    // The new item and its marker, and the markers of the two after it,
    // whose content is collected again.
    let inserted = page.change(|cx, document| {
        let li = cx.create_element(document, html("li"));
        let (ol, b) = (find(cx, document, "ol"), find(cx, document, "b"));
        cx.pre_insert(ol, li, Some(b)).unwrap();
    });
    assert_eq!(markers(&page), ["1. ", "2. ", "3. ", "4. "]);
    assert_eq!(inserted.built, 4, "{inserted:?}");
    page.change(|cx, document| cx.remove(find(cx, document, "b")));
    assert_eq!(markers(&page), ["1. ", "2. ", "3. "]);
    set_style(&mut page, "c", "counter-set: list-item 7");
    assert_eq!(markers(&page), ["1. ", "2. ", "7. "]);
    set_style(&mut page, "c", "");
    set(&mut page, "c", "value", "7");
    assert_eq!(markers(&page), ["1. ", "2. ", "7. "]);
    set(&mut page, "ol", "start", "5");
    assert_eq!(markers(&page), ["5. ", "6. ", "7. "]);
    set_style(&mut page, "ol", "list-style-type: upper-alpha");
    assert_eq!(markers(&page), ["E. ", "F. ", "G. "]);
    set_style(&mut page, "ol", "list-style-position: inside");
    let view = page.view();
    let a = &fragments(&view, &page)["a"];
    assert_eq!(a.lines[0].boxes[0].pseudo, Some(PseudoId::Marker));
    // Nothing the counters depend on: no marker is made again.
    assert_eq!(
        page.change(|cx, document| cx.set_data(first_child(cx, document, "p"), "Words")),
        update(0, 1, 0)
    );
}

/// Taking away the last element with a counter in a box, the counters
/// after it are worked out again: the box no longer has any, but the
/// document does.
#[test]
fn counters_follow_an_element_taken_away() {
    let mut page = Page::new(
        "<style>body { counter-reset: h } h2::before { counter-increment: h; content: counter(h) }</style>
         <div id=wrap><h2 id=x></h2></div><h2 id=y></h2>",
    );
    let y = page.id("y");
    assert_eq!(
        page.tree
            .counter_text_for_testing(y, PseudoId::Before)
            .as_deref(),
        Some("2")
    );
    page.change(|cx, document| cx.remove(find(cx, document, "x")));
    assert_eq!(
        page.tree
            .counter_text_for_testing(y, PseudoId::Before)
            .as_deref(),
        Some("1")
    );
}

/// Changes the page with `change`, and checks how many times the whole
/// document's counters were worked out again, and lists numbered.
fn counted(
    page: &mut Page,
    change: impl FnOnce(&mut Page),
    what: &str,
    passes: usize,
    lists: usize,
) {
    let before = (
        page.tree.counter_passes_for_testing(),
        page.tree.lists_numbered_for_testing().0,
    );
    change(page);
    let after = (
        page.tree.counter_passes_for_testing(),
        page.tree.lists_numbered_for_testing().0,
    );
    assert_eq!(
        (after.0 - before.0, after.1 - before.1),
        (passes, lists),
        "{what}: whole document passes, lists numbered"
    );
}

/// The lists number their own items (Blink's `ListItemOrdinal`), with no
/// pass over the whole document: a list is numbered again when an item or
/// an element in it comes, goes, is put under `display: none` or out of
/// it, or changes its part in the `list-item` counter (`counter-
/// increment`, `value`), and only when one of its markers shows a number.
/// Another list isn't, nor is one for its markers' `list-style-type`
/// (the numbers are the same), and the markers show the new numbers.
/// (Debug builds check every marker against the whole document's pass.)
#[test]
fn lists_number_their_own_items() {
    let mut page = Page::new(
        "<style>ol, p, ul { font: 20px/1 Ahem } .none { list-style: none }</style>
         <div id=box><ol id=ol><li id=a>A<li id=b>B</ol><ol><li id=x>X</ol>
         <ul id=plain class=none><li>P</ul><p id=p>Text</p></div>
         <ul id=hidden style='display: none'><li>Hidden</ul>",
    );
    assert!(page.tree.lists_numbered_for_testing().1);
    counted(
        &mut page,
        |page| {
            page.change(|cx, document| cx.set_data(first_child(cx, document, "p"), "Words"));
        },
        "text",
        0,
        0,
    );
    counted(
        &mut page,
        |page| {
            set_style(page, "box", "padding: 5px");
        },
        "a box's size",
        0,
        0,
    );
    counted(
        &mut page,
        |page| {
            set_style(page, "a", "color: red");
        },
        "a color",
        0,
        0,
    );
    counted(
        &mut page,
        |page| {
            set_style(page, "hidden", "display: none; color: red");
        },
        "a list with no boxes",
        0,
        0,
    );
    counted(
        &mut page,
        |page| {
            set_style(page, "a", "list-style-type: upper-alpha");
        },
        "a list-style-type",
        0,
        0,
    );
    assert_eq!(marker(&page, "a"), "A. ");
    counted(
        &mut page,
        |page| {
            set_style(
                page,
                "a",
                "list-style-type: upper-alpha; counter-increment: list-item 3",
            );
        },
        "a counter-increment",
        0,
        1,
    );
    assert_eq!(marker(&page, "b"), "4. ");
    counted(
        &mut page,
        |page| {
            page.change(|cx, document| {
                let li = cx.create_element(document, html("li"));
                let (ol, b) = (find(cx, document, "ol"), find(cx, document, "b"));
                cx.pre_insert(ol, li, Some(b)).unwrap();
            });
        },
        "an item coming",
        0,
        1,
    );
    assert_eq!(marker(&page, "b"), "5. ");
    counted(
        &mut page,
        |page| {
            page.change(|cx, document| {
                let b = find(cx, document, "b");
                cx.remove(cx.previous_sibling(b).unwrap());
            });
        },
        "an item going",
        0,
        1,
    );
    assert_eq!(marker(&page, "b"), "4. ");
    counted(
        &mut page,
        |page| {
            page.change(|cx, document| {
                let div = cx.create_element(document, html("div"));
                cx.append_child(find(cx, document, "ol"), div).unwrap();
            });
        },
        "an element in it with no counter",
        0,
        1,
    );
    counted(
        &mut page,
        |page| {
            set_style(page, "b", "display: none");
        },
        "under display: none",
        0,
        1,
    );
    counted(
        &mut page,
        |page| {
            set_style(page, "b", "");
        },
        "out of display: none",
        0,
        1,
    );
    assert_eq!(marker(&page, "b"), "4. ");
    counted(
        &mut page,
        |page| {
            set(page, "b", "value", "9");
        },
        "a value",
        0,
        1,
    );
    assert_eq!(marker(&page, "b"), "9. ");
    assert_eq!(marker(&page, "x"), "1. ");
    // An item with no boxes counts nothing.
    page.change(|cx, document| {
        cx.remove_attribute(find(cx, document, "b"), "value");
    });
    assert_eq!(marker(&page, "b"), "4. ");
    counted(
        &mut page,
        |page| {
            set_style(page, "a", "display: none");
        },
        "an item before it under display: none",
        0,
        1,
    );
    assert_eq!(marker(&page, "b"), "1. ");
    // Nor do the items in an element with no boxes.
    page.change(|cx, document| {
        let div = element(cx, document, "div", &[("style", "display: none")]);
        let li = cx.create_element(document, html("li"));
        cx.append_child(div, li).unwrap();
        let (ol, b) = (find(cx, document, "ol"), find(cx, document, "b"));
        cx.pre_insert(ol, div, Some(b)).unwrap();
    });
    assert_eq!(marker(&page, "b"), "1. ");
    counted(
        &mut page,
        |page| {
            page.change(|cx, document| {
                let li = cx.create_element(document, html("li"));
                cx.append_child(find(cx, document, "plain"), li).unwrap();
            });
        },
        "an item of a list whose markers show no number",
        0,
        0,
    );
}

/// A marker whose `content` is strings shows the new ones when its style
/// changes, with no counter worked out.
#[test]
fn a_markers_strings_follow_its_style() {
    let mut page = Page::new(
        "<style>ol { font: 20px/1 Ahem } ::marker { content: 'A' } .t ::marker { content: 'B' }</style>
         <ol id=ol><li id=a>Item</ol>",
    );
    assert_eq!(marker(&page, "a"), "A");
    counted(
        &mut page,
        |page| {
            set(page, "ol", "class", "t");
        },
        "a marker's strings",
        0,
        0,
    );
    assert_eq!(marker(&page, "a"), "B");
}

/// The other counters are worked out over the whole document only when
/// what takes part in them changes (Blink's `MarkCountersDirty`): an
/// element changing or showing one coming, going, or changing its part.
#[test]
fn other_counters_are_worked_out_when_they_change() {
    let mut page = Page::new(
        "<style>body { counter-reset: h } h2::before { counter-increment: h; content: counter(h) }
           .x::before { content: counter(h) } ol, p { font: 20px/1 Ahem }</style>
         <div id=box><h2 id=a></h2><h2 id=b></h2><p id=p>Text</p><ol id=ol><li>I</ol></div>",
    );
    assert_eq!(before(&page, "b"), "2");
    counted(
        &mut page,
        |page| {
            page.change(|cx, document| {
                let li = cx.create_element(document, html("li"));
                cx.append_child(find(cx, document, "ol"), li).unwrap();
            });
        },
        "a list item",
        0,
        1,
    );
    counted(
        &mut page,
        |page| {
            page.change(|cx, document| {
                let h2 = cx.create_element(document, html("h2"));
                let (bx, b) = (find(cx, document, "box"), find(cx, document, "b"));
                cx.pre_insert(bx, h2, Some(b)).unwrap();
            });
        },
        "an element with a counter coming",
        1,
        0,
    );
    assert_eq!(before(&page, "b"), "3");
    counted(
        &mut page,
        |page| {
            set(page, "p", "class", "x");
        },
        "a ::before showing one",
        1,
        0,
    );
    assert_eq!(before(&page, "p"), "3");
    counted(
        &mut page,
        |page| {
            set_style(page, "a", "display: none");
        },
        "one going",
        1,
        0,
    );
    assert_eq!(before(&page, "b"), "2");
}

/// A use of `list-item` the lists can't number one by one as css-lists 3
/// has it: the whole document's pass numbers the lists then, as before.
/// An item outside a list counts on the list before it; another element
/// changing it, or a `::before` showing it. (An item after a list in its
/// own list counts on its own list's: the lists number it.)
#[test]
fn other_uses_of_list_item_are_numbered_by_the_whole_document() {
    let cases = [
        (
            "<ol><li>1</ol><li id=t style='list-style-type: decimal'>stray",
            "2. ",
        ),
        (
            "<ol><li>A<div style='counter-increment: list-item 5'></div><li id=t>B</ol>",
            "7. ",
        ),
        (
            "<style>.c::before { content: counter(list-item) }</style><ol><li class=c>A<li id=t>B</ol>",
            "2. ",
        ),
        (
            "<style>.c::before { content: ''; counter-increment: list-item 4 }</style><ol><li class=c>A<li id=t>B</ol>",
            "6. ",
        ),
    ];
    for (html, text) in cases {
        let page = Page::new(&format!("<style>ol {{ font: 20px/1 Ahem }}</style>{html}"));
        assert!(!page.tree.lists_numbered_for_testing().1, "{html}");
        assert_eq!(marker(&page, "t"), text, "{html}");
    }
    let page = Page::new(
        "<style>ol { font: 20px/1 Ahem }</style><ol><li>A</li><ul><li>U<li>V</ul><li id=t>B</ol>",
    );
    assert!(page.tree.lists_numbered_for_testing().1);
    assert_eq!(marker(&page, "t"), "2. ");
    // Coming in an update. (A page at a time: one token a thread.)
    drop(page);
    let mut page =
        Page::new("<style>ol { font: 20px/1 Ahem }</style><div id=box><ol><li id=a>A</ol></div>");
    assert!(page.tree.lists_numbered_for_testing().1);
    page.change(|cx, document| {
        let li = element(
            cx,
            document,
            "li",
            &[("id", "t"), ("style", "list-style-type: decimal")],
        );
        cx.append_child(find(cx, document, "box"), li).unwrap();
    });
    assert!(!page.tree.lists_numbered_for_testing().1);
    assert_eq!(marker(&page, "t"), "2. ");
    // Coming with a pseudo-element's new style.
    drop(page);
    let mut page = Page::new(
        "<style>ol { font: 20px/1 Ahem } .c::before { content: ''; counter-increment: list-item 4 }</style>
         <ol><li id=a>A<li id=t>B</ol>",
    );
    set(&mut page, "a", "class", "c");
    assert!(!page.tree.lists_numbered_for_testing().1);
    assert_eq!(marker(&page, "t"), "6. ");
    // Coming with a new style.
    drop(page);
    let mut page = Page::new(
        "<style>ol { font: 20px/1 Ahem } .c { counter-increment: list-item 5 }</style>
         <ol><li>A<div id=d></div><li id=t>B</ol>",
    );
    assert!(page.tree.lists_numbered_for_testing().1);
    set(&mut page, "d", "class", "c");
    assert!(!page.tree.lists_numbered_for_testing().1);
    assert_eq!(marker(&page, "t"), "7. ");
}

/// The first list in a document without counters numbers its items: the
/// content built again has counters, so they are worked out.
#[test]
fn a_first_list_has_markers() {
    let mut page = Page::new("<style>ol { font: 20px/1 Ahem }</style><div id=d>Text</div>");
    let li = page.change(|cx, document| {
        let ol = cx.create_element(document, html("ol"));
        let li = cx.create_element(document, html("li"));
        cx.append_child(ol, li).unwrap();
        cx.append_child(find(cx, document, "d"), ol).unwrap();
    });
    assert!(li.built > 0);
    let item = page.read(|cx, document| {
        let ol = cx.last_child(find(cx, document, "d")).unwrap();
        cx.node_id(cx.first_child(ol).unwrap())
    });
    assert_eq!(
        page.tree
            .counter_text_for_testing(item, PseudoId::Marker)
            .as_deref(),
        Some("1. ")
    );
}

/// What style changes mean for scroll anchoring, as an update notes them:
/// a suppression trigger on the element, going out of flow on its
/// scroller (the root element for the viewport), `overflow-anchor`, and
/// what was built again; and when the whole tree is built again.
#[test]
fn style_changes_are_noted_for_scroll_anchoring() {
    let mut page = Page::new(
        "<html id=html><body id=body><div id=s style='overflow: auto; height: 50px'>\
         <p id=p>a</p><span id=i>b</span></div><p id=q>c</p></body></html>",
    );
    page.tree.take_scroll_anchoring_changes();
    let (html, s, p, i, q) = (
        page.id("html"),
        page.id("s"),
        page.id("p"),
        page.id("i"),
        page.id("q"),
    );
    set_style(&mut page, "p", "padding: 1px");
    assert_eq!(page.tree.take_scroll_anchoring_changes().disabled, [p]);
    set_style(&mut page, "i", "margin-left: 2px");
    assert_eq!(page.tree.take_scroll_anchoring_changes().disabled, [i]);
    set_style(&mut page, "q", "overflow-anchor: none");
    assert_eq!(
        page.tree.take_scroll_anchoring_changes().overflow_anchor,
        [q]
    );
    // Out of flow: its own box changes, and its scroller is noted.
    set_style(&mut page, "p", "padding: 1px; position: absolute");
    let changes = page.tree.take_scroll_anchoring_changes();
    assert!(changes.disabled.contains(&s), "{changes:?}");
    set_style(&mut page, "q", "overflow-anchor: none; position: fixed");
    let changes = page.tree.take_scroll_anchoring_changes();
    assert!(changes.disabled.contains(&html), "{changes:?}");
    set_style(&mut page, "q", "display: inline-block");
    assert_eq!(page.tree.take_scroll_anchoring_changes().rebuilt, [q]);
    // Built again whole: each element's new style is compared with its
    // old one.
    set_style(&mut page, "body", "overflow: hidden");
    let changes = page.tree.take_scroll_anchoring_changes();
    assert!(changes.rebuilt.is_empty(), "{changes:?}");
    set_style(
        &mut page,
        "s",
        "overflow: auto; height: 60px; overflow-anchor: none",
    );
    page.tree.take_scroll_anchoring_changes();
    page.change(|cx, document| {
        cx.set_attribute(find(cx, document, "body"), "style", "");
        cx.set_attribute(
            find(cx, document, "s"),
            "style",
            "overflow: auto; height: 70px",
        );
    });
    let changes = page.tree.take_scroll_anchoring_changes();
    assert!(changes.disabled.contains(&s), "{changes:?}");
}

/// Counters through a `display: contents` element (its pseudo-elements
/// and children count in its place), and an element taking part in them
/// coming inside another.
#[test]
fn counters_reach_through_what_has_no_box() {
    let mut page = Page::new(
        "<style>body { counter-reset: h } h2::before { counter-increment: h; content: counter(h) } \
         .c::before { content: 'x' counter(h); counter-increment: h }</style>\
         <div id=box><div id=c class=c style='display: contents'><h2 id=a></h2></div><h2 id=b></h2></div>",
    );
    assert_eq!(before(&page, "c"), "x1");
    assert_eq!(before(&page, "b"), "3");
    counted(
        &mut page,
        |page| {
            page.change(|cx, document| {
                let div = cx.create_element(document, html("div"));
                let h2 = cx.create_element(document, html("h2"));
                cx.append_child(div, h2).unwrap();
                let (bx, b) = (find(cx, document, "box"), find(cx, document, "b"));
                cx.pre_insert(bx, div, Some(b)).unwrap();
            });
        },
        "an element with a counter coming in another",
        1,
        0,
    );
    assert_eq!(before(&page, "b"), "4");
}

/// A replaced element's marker and what is in it take no part in the
/// counters, as it shows neither: a list item `<img>` whose `::marker`
/// has `content`, outside or inside, has no marker, though it counts as an
/// item; a counter in a `<canvas>`'s fallback isn't counted.
#[test]
fn a_replaced_element_has_no_marker_nor_counters_in_it() {
    let page = Page::new(
        "<style>body { counter-reset: h } img { display: list-item } img::marker { content: '[m]' } \
         h2::before { counter-increment: h; content: counter(h) }</style>\
         <ol><img id=a alt=a><img id=b alt=b style='list-style-position: inside'><li id=c>c</li></ol>\
         <canvas><h2 id=fallback>x</h2></canvas><h2 id=d>d</h2>",
    );
    assert_eq!(marker(&page, "a"), "");
    assert_eq!(marker(&page, "b"), "");
    assert_eq!(marker(&page, "c"), "3. ");
    assert_eq!(before(&page, "fallback"), "");
    assert_eq!(before(&page, "d"), "1");
}

/// An element built again whose counter's text changes too: its content
/// is the new box's, collected with it (not its old box's, given up).
#[test]
fn a_counter_in_what_is_built_again_is_in_the_new_box() {
    let mut page = Page::new(
        "<style>body { counter-reset: h } h2::before { counter-increment: h; content: counter(h) }</style>\
         <div id=box><h2 id=a>a</h2><h2 id=b>b</h2></div>",
    );
    page.change(|cx, document| {
        let h2 = cx.create_element(document, html("h2"));
        let (bx, a, b) = (
            find(cx, document, "box"),
            find(cx, document, "a"),
            find(cx, document, "b"),
        );
        cx.pre_insert(bx, h2, Some(a)).unwrap();
        cx.set_attribute(b, "style", "display: inline-block");
    });
    assert_eq!(before(&page, "b"), "3");
}

/// An element taking part in the counters moved into another document is
/// gone from this one's: the counters after it are worked out again.
#[test]
fn counters_follow_an_element_moved_to_another_document() {
    let mut page = Page::new(
        "<style>body { counter-reset: h } h2::before { counter-increment: h; content: counter(h) }</style>\
         <h2 id=x></h2><h2 id=y></h2>",
    );
    page.change(|cx, document| {
        let other = cx.create_document(true);
        cx.append_child(other, find(cx, document, "x")).unwrap();
    });
    assert_eq!(before(&page, "y"), "1");
}

/// Style containment coming or going scopes the counters again, on an
/// element with no counter of its own as on one with.
#[test]
fn counters_follow_style_containment() {
    let mut page = Page::new(
        "<style>body { counter-reset: n } p::before { counter-increment: n; content: counter(n) }</style>
         <p></p><div id=c><p id=b></p></div><div id=d style='counter-increment: n'><p id=e></p></div>",
    );
    assert_eq!(
        (before(&page, "b"), before(&page, "e")),
        ("2".into(), "4".into())
    );
    set_style(&mut page, "c", "contain: style");
    assert_eq!(before(&page, "b"), "1");
    // `d`'s own increment is outside: to 3, and `e`'s a new counter.
    set_style(&mut page, "d", "counter-increment: n; contain: style");
    assert_eq!(before(&page, "e"), "1");
    set_style(&mut page, "c", "");
    assert_eq!(before(&page, "b"), "2");
}

/// Gives the element whose `id` is `id` an open shadow root with `html`
/// in it.
fn attach_shadow(page: &mut Page, id: &str, html: &str) -> BoxTreeUpdate {
    page.change(|cx, document| {
        crate::page::attach_shadow(cx, find(cx, document, id), html);
    })
}

/// Whether the element whose `id` is `id` is laid out: its first child's
/// text.
fn shown(page: &Page, id: &str) -> bool {
    page.tree
        .is_rendered(page.read(|cx, document| cx.node_id(first_child(cx, document, id))))
}

/// A shadow tree changed, a host given one, what is assigned to a slot
/// changed, a slot's fallback came or went, `display: contents` came or
/// went: each step lays out as a tree built from scratch.
#[test]
fn shadow_trees_and_slots_follow_the_dom() {
    let mut page = Page::new(
        "<div id=host style='width: 300px'>\
         <span id=s slot=a>light</span><span id=t slot=b>other</span>text</div>\
         <div id=later>later <b>light</b></div>",
    );
    // Shadow trees, given to a host: its children go into the slots.
    attach_shadow(
        &mut page,
        "host",
        "<div id=da style='height: 10px'><slot name=a></slot></div>\
         <p id=db><slot id=slot-b name=b>fallback</slot></p><slot id=default></slot>",
    );
    attach_shadow(&mut page, "later", "<i>shadow</i>");
    // A child into another slot, in another box.
    set(&mut page, "s", "slot", "b");
    // The last one assigned to a slot goes: its fallback shows.
    page.change(|cx, document| cx.remove(find(cx, document, "s")));
    page.change(|cx, document| cx.remove(find(cx, document, "t")));
    // A slot renamed: the text in the default slot moves.
    set(&mut page, "default", "name", "b");
    // In the shadow tree: inserted, changed, taken out.
    page.change(|cx, document| {
        let p = cx.create_element(document, html("p"));
        let text = cx.create_text(document, "new");
        cx.append_child(p, text).unwrap();
        cx.append_child(find(cx, document, "da"), p).unwrap();
    });
    set_style(&mut page, "da", "height: 30px");
    set_style(&mut page, "db", "display: none");
    page.change(|cx, document| cx.remove(find(cx, document, "da")));
    // `display: contents` on and off, with a child that changes.
    set_style(&mut page, "db", "display: contents");
    set_style(&mut page, "slot-b", "display: block");
    set_style(&mut page, "db", "display: block");
}

/// A child of a host whose box goes in a slot's container: inserted,
/// changed and taken out there, not in the host's own content.
#[test]
fn a_slotted_child_updates_in_its_slots_box() {
    let mut page =
        Page::new("<div id=host><p id=p>light</p>text <span id=span>inline</span></div>");
    attach_shadow(
        &mut page,
        "host",
        "<section id=outer><div id=inner><slot></slot></div></section>",
    );
    set_style(&mut page, "p", "color: red");
    // Changes that leave what is assigned where it was: a text, an inline
    // element laid out otherwise.
    set_style(&mut page, "span", "font-size: 30px");
    fn text<'gc>(cx: &Cx<'_, 'gc>, document: NodeRef<'gc>) -> NodeRef<'gc> {
        cx.previous_sibling(find(cx, document, "span")).unwrap()
    }
    page.change(|cx, document| cx.append_data(text(cx, document), "more "));
    // The host built again, with a child changed in the same update.
    page.change(|cx, document| {
        cx.set_attribute(find(cx, document, "host"), "style", "display: flex");
        cx.append_data(text(cx, document), "and more ");
    });
    page.change(|cx, document| {
        let more = cx.create_text(document, " more");
        cx.append_child(find(cx, document, "p"), more).unwrap();
    });
    page.change(|cx, document| {
        let q = cx.create_element(document, html("q"));
        cx.append_child(find(cx, document, "host"), q).unwrap();
    });
    page.change(|cx, document| cx.remove(find(cx, document, "p")));
}

/// A document with no counters whose first list is in a shadow tree: the
/// markers come with it.
#[test]
fn a_first_list_in_a_shadow_tree_has_markers() {
    let mut page = Page::new("<div id=host><ol id=list></ol></div>");
    attach_shadow(&mut page, "host", "<slot></slot>");
    fn item<'gc>(cx: &Cx<'_, 'gc>, document: NodeRef<'gc>) -> NodeRef<'gc> {
        let li = cx.create_element(document, html("li"));
        let text = cx.create_text(document, "item");
        cx.append_child(li, text).unwrap();
        li
    }
    page.change(|cx, document| {
        cx.append_child(find(cx, document, "list"), item(cx, document))
            .unwrap();
    });
    // Right in a shadow root: the box it is in is the host's, whose own
    // children have no list.
    drop(page);
    let mut page = Page::new("<div id=host></div>");
    attach_shadow(&mut page, "host", "<p>shadow</p>");
    page.change(|cx, document| {
        let root = cx.shadow_root(find(cx, document, "host")).unwrap();
        cx.append_child(root, item(cx, document)).unwrap();
    });
}

/// A host built again, with its shadow tree changed in the same update:
/// no flag is left in the shadow tree.
#[test]
fn a_host_built_again_clears_its_shadow_trees_flags() {
    let mut page = Page::new("<div id=outer><div id=host>light</div></div>");
    attach_shadow(&mut page, "host", "<p id=inside>shadow</p>");
    page.change(|cx, document| {
        cx.set_attribute(find(cx, document, "inside"), "style", "width: 10px");
        cx.set_attribute(find(cx, document, "outer"), "style", "display: flex");
    });
}

/// A marker in a shadow tree that shows another number, in the update that
/// builds its host again: nothing is collected in what is built again.
#[test]
fn a_marker_in_a_host_built_again() {
    let mut page = Page::new("<ol><li id=first>a</li><div id=host></div></ol>");
    attach_shadow(&mut page, "host", "<div><li>b</li></div>");
    page.change(|cx, document| {
        cx.remove(find(cx, document, "first"));
        cx.set_attribute(find(cx, document, "host"), "style", "display: flex");
    });
}

/// A host's shadow tree is laid out in place of its children: a slot's
/// assigned nodes where the slot is, a child no slot takes not at all;
/// each change of the flat tree updates the boxes as building them again
/// would (`check`).
#[test]
fn a_shadow_tree_is_laid_out_in_place_of_the_hosts_children() {
    let mut page = Page::new(
        "<!doctype html><div id=host><b id=a slot=s>a</b><b id=b>b</b><b id=c slot=none>c</b></div><p id=after>after</p>",
    );
    assert!(shown(&page, "c"), "no shadow tree yet");
    page.change(|cx, document| {
        let host = find(cx, document, "host");
        let root = cx.attach_shadow(host, open(dom::SlotAssignmentMode::Named));
        let inner = element(cx, document, "p", &[("id", "inner")]);
        cx.append_child(inner, cx.create_text(document, "in"))
            .unwrap();
        cx.append_child(root, inner).unwrap();
        cx.append_child(
            root,
            element(cx, document, "slot", &[("id", "s"), ("name", "s")]),
        )
        .unwrap();
        cx.append_child(root, element(cx, document, "slot", &[("id", "default")]))
            .unwrap();
        let z = element(cx, document, "slot", &[("id", "z"), ("name", "z")]);
        let fallback = element(cx, document, "i", &[("id", "fb")]);
        cx.append_child(fallback, cx.create_text(document, "fallback"))
            .unwrap();
        cx.append_child(z, fallback).unwrap();
        cx.append_child(root, z).unwrap();
    });
    for id in ["inner", "a", "b", "fb", "after"] {
        assert!(shown(&page, id), "#{id}");
    }
    assert!(!shown(&page, "c"), "taken by no slot");
    // Assigned: rendered; the fallback, no longer.
    set(&mut page, "c", "slot", "z");
    assert!(shown(&page, "c"));
    assert!(!shown(&page, "fb"));
    // A slot gone: what it had, with it.
    page.change(|cx, document| cx.remove(find(cx, document, "s")));
    assert!(!shown(&page, "a"));
    // A slot renamed to the name: it gets it.
    set(&mut page, "default", "name", "s");
    assert!(shown(&page, "a"));
    assert!(!shown(&page, "b"));
    // In the shadow tree: an element in, a text out.
    page.change(|cx, document| {
        let inner = find(cx, document, "inner");
        let late = element(cx, document, "u", &[("id", "late")]);
        cx.append_child(late, cx.create_text(document, "late"))
            .unwrap();
        cx.append_child(inner, late).unwrap();
    });
    assert!(shown(&page, "late"));
    page.change(|cx, document| {
        let inner = find(cx, document, "inner");
        cx.set_text_content(inner, "");
    });
    // The host's text: the default slot's (none now): not rendered; a
    // default slot again: rendered.
    page.change(|cx, document| {
        let host = find(cx, document, "host");
        cx.append_child(host, cx.create_text(document, "loose"))
            .unwrap();
    });
    page.change(|cx, document| {
        let host = find(cx, document, "host");
        let root = cx.shadow_root(host).unwrap();
        cx.append_child(root, element(cx, document, "slot", &[("id", "again")]))
            .unwrap();
    });
    assert!(shown(&page, "b"));
    // Out of the document with its host, and back.
    page.change(|cx, document| cx.remove(find(cx, document, "host")));
    page.change(|cx, document| {
        let host = cx.create_element(document, html("section"));
        let root = cx.attach_shadow(host, open(dom::SlotAssignmentMode::Manual));
        let slot = element(cx, document, "slot", &[("id", "manual")]);
        cx.append_child(root, slot).unwrap();
        let m = element(cx, document, "em", &[("id", "m")]);
        cx.append_child(m, cx.create_text(document, "m")).unwrap();
        cx.append_child(host, m).unwrap();
        cx.assign(slot, &[m]);
        let body = cx.parent(find(cx, document, "after")).unwrap();
        cx.append_child(body, host).unwrap();
    });
    assert!(shown(&page, "m"));
}

/// Nested hosts: a slot's assigned node a host itself, a slot assigned to
/// another slot.
#[test]
fn nested_shadow_trees_are_laid_out_through_their_slots() {
    let mut page = Page::new("<!doctype html><div id=outer><span id=x>x</span></div>");
    page.change(|cx, document| {
        let outer = find(cx, document, "outer");
        let root = cx.attach_shadow(outer, open(dom::SlotAssignmentMode::Named));
        let inner = element(cx, document, "div", &[("id", "inner")]);
        cx.append_child(inner, element(cx, document, "slot", &[("id", "s1")]))
            .unwrap();
        cx.append_child(root, inner).unwrap();
        let inner_root = cx.attach_shadow(inner, open(dom::SlotAssignmentMode::Named));
        let wrap = element(cx, document, "p", &[("id", "wrap")]);
        cx.append_child(wrap, element(cx, document, "slot", &[("id", "s2")]))
            .unwrap();
        cx.append_child(inner_root, wrap).unwrap();
    });
    assert!(shown(&page, "x"), "#x");
    set(&mut page, "x", "slot", "nope");
    assert!(!shown(&page, "x"));
}

/// A manual slot's fallback, laid out, then a node assigned to it: the
/// fallback goes.
#[test]
fn a_fallback_goes_when_a_node_is_assigned() {
    let mut page = Page::new("<!doctype html><div id=host><span id=assigned>x</span></div>");
    page.change(|cx, document| {
        let host = find(cx, document, "host");
        let root = cx.attach_shadow(host, open(dom::SlotAssignmentMode::Manual));
        let slot = element(cx, document, "slot", &[("id", "slot")]);
        let fallback = element(cx, document, "i", &[("id", "fb")]);
        cx.append_child(fallback, cx.create_text(document, "fallback"))
            .unwrap();
        cx.append_child(slot, fallback).unwrap();
        cx.append_child(root, slot).unwrap();
    });
    assert!(shown(&page, "fb") && !shown(&page, "assigned"));
    page.change(|cx, document| {
        let slot = find(cx, document, "slot");
        cx.assign(slot, &[find(cx, document, "assigned")]);
    });
    assert!(!shown(&page, "fb") && shown(&page, "assigned"));
}

/// The same, the fallback a text, the node assigned an empty element.
#[test]
fn a_text_fallback_goes_when_an_empty_node_is_assigned() {
    let mut page = Page::new("<!doctype html><div id=host><span id=assigned></span></div>");
    page.change(|cx, document| {
        let host = find(cx, document, "host");
        let root = cx.attach_shadow(host, open(dom::SlotAssignmentMode::Manual));
        let slot = element(cx, document, "slot", &[("id", "slot")]);
        cx.append_child(slot, cx.create_text(document, "fallback"))
            .unwrap();
        cx.append_child(root, slot).unwrap();
    });
    assert!(shown(&page, "slot"));
    let update = page.change(|cx, document| {
        let slot = find(cx, document, "slot");
        cx.assign(slot, &[find(cx, document, "assigned")]);
    });
    assert!(!shown(&page, "slot"), "the fallback's text");
    // It built nothing, shaped nothing: the renderer lays out again for
    // the document's flags (`FrameView`), not for these.
    assert_eq!(update.built + update.shaped, 0);
}

/// An inline element right in a shadow root (no box, its parent no
/// element): a change of it is found in the host's box.
#[test]
fn a_change_right_in_a_shadow_root_is_the_hosts() {
    let mut page = Page::new("<!doctype html><div id=host></div>");
    page.change(|cx, document| {
        let host = find(cx, document, "host");
        let root = cx.attach_shadow(host, open(dom::SlotAssignmentMode::Named));
        let span = element(cx, document, "span", &[("id", "span")]);
        cx.append_child(span, cx.create_text(document, "first"))
            .unwrap();
        cx.append_child(root, span).unwrap();
    });
    assert!(shown(&page, "span"));
    page.change(|cx, document| {
        let span = find(cx, document, "span");
        cx.set_text_content(span, "second, longer");
    });
    assert!(shown(&page, "span"));
    set_style(&mut page, "span", "display: block");
}

/// A list in a shadow tree, its items slotted into it: numbered through
/// the flat tree, again as one comes.
#[test]
fn slotted_items_are_their_slots_lists() {
    let mut page =
        Page::new("<!doctype html><div id=host><li id=one>a</li><li id=two>b</li></div>");
    page.change(|cx, document| {
        let host = find(cx, document, "host");
        let root = cx.attach_shadow(host, open(dom::SlotAssignmentMode::Named));
        let list = element(cx, document, "ol", &[("id", "list")]);
        cx.append_child(list, element(cx, document, "slot", &[]))
            .unwrap();
        cx.append_child(root, list).unwrap();
    });
    assert_eq!(
        (marker(&page, "one"), marker(&page, "two")),
        ("1. ".into(), "2. ".into())
    );
    page.change(|cx, document| {
        let host = find(cx, document, "host");
        let zero = element(cx, document, "li", &[("id", "zero")]);
        cx.append_child(zero, cx.create_text(document, "z"))
            .unwrap();
        cx.insert(host, zero, cx.first_child(host));
    });
    assert_eq!(
        (
            marker(&page, "zero"),
            marker(&page, "one"),
            marker(&page, "two")
        ),
        ("1. ".into(), "2. ".into(), "3. ".into())
    );
    set(&mut page, "list", "start", "5");
    assert_eq!(marker(&page, "two"), "7. ");
}

/// A table's rows and cells come and go, its cells' text and spans
/// change, and each time it lays out as a new tree does; a cell kept laid
/// out moves its aligned content as far as it should, once.
#[test]
fn tables_follow_the_dom() {
    let mut page = Page::new(
        "<table><tbody id=body><tr id=r1><td id=a>A</td><td id=b>B B</td></tr>\
           <tr id=r2><td id=c style='height: 60px'>C</td></tr></tbody></table>",
    );
    // A row.
    page.change(|cx, document| {
        let tr = cx.create_element(document, html("tr"));
        let td = cx.create_element(document, html("td"));
        let text = cx.create_text(document, "New cell");
        cx.append_child(td, text).unwrap();
        cx.append_child(tr, td).unwrap();
        cx.pre_insert(find(cx, document, "body"), tr, None).unwrap();
    });
    // A cell's text, and a cell.
    page.change(|cx, document| cx.set_data(first_child(cx, document, "a"), "Longer text"));
    page.change(|cx, document| cx.remove(find(cx, document, "b")));
    // A span: the cell is built again.
    set(&mut page, "a", "colspan", "2");
    set(&mut page, "a", "rowspan", "2");
    // Its row's height, laid out again: the content goes back to the middle.
    set_style(&mut page, "r1", "height: 100px");
    set_style(&mut page, "c", "height: 20px");
    // Text and a cell straight in a row, then a cell in the row group.
    page.change(|cx, document| {
        let text = cx.create_text(document, "loose");
        cx.append_child(find(cx, document, "r1"), text).unwrap();
        let td = cx.create_element(document, html("td"));
        cx.append_child(find(cx, document, "body"), td).unwrap();
    });
}

/// A cell's `colspan` and `rowspan`, and a column's `span`, are its
/// box's: a change builds it again. A table's `cellpadding` styles its
/// cells again.
#[test]
fn a_span_builds_its_box_again() {
    let mut page = Page::new(
        "<table id=t><colgroup><col id=col></colgroup><tr><td id=a>a<td>b<tr><td>c<td>d</table>",
    );
    for (id, name, value) in [
        ("a", "colspan", "2"),
        ("a", "rowspan", "2"),
        ("col", "span", "2"),
        ("a", "colspan", "1"),
    ] {
        let changed = set(&mut page, id, name, value);
        assert!(changed.built > 0, "{name}={value}: {changed:?}");
    }
    let changed = set(&mut page, "t", "cellpadding", "5");
    assert!(changed.built + changed.restyled > 0, "{changed:?}");
}

/// Anonymous table boxes inherit from the box they are in: a change only
/// painted makes them again, with the new style.
#[test]
fn anonymous_table_boxes_take_their_parents_style() {
    let mut page = Page::new(
        "<div id=t style='display: table; color: red'>Text</div>\
         <div id=d style='color: red'>A <span id=s style='color: red'>\
           <span style='display: table-cell'>cell</span></span></div>",
    );
    set_style(&mut page, "t", "display: table; color: blue");
    set_style(&mut page, "d", "color: blue");
    set_style(&mut page, "s", "color: green");
}

/// A cell laid out again (its text changed) has its content moved by its
/// `vertical-align` again, from where its layout put it: its lines, and
/// its blocks.
#[test]
fn a_cell_laid_out_again_is_aligned_again() {
    let mut page = Page::new(
        "<table><tr><td id=a style='vertical-align: middle'>A</td>\
           <td style='vertical-align: middle'><div id=block>A</div></td>\
           <td style='height: 80px'>B</td></tr></table>",
    );
    for id in ["a", "block"] {
        page.change(|cx, document| cx.set_data(first_child(cx, document, id), "Changed"));
        page.change(|cx, document| cx.set_data(first_child(cx, document, id), "Again"));
    }
}

/// A column's style (what only paints too: the table's fragment carries
/// it) and `span` change, a column comes into a group, and each time the
/// table lays out as a new tree does.
#[test]
fn columns_follow_the_dom() {
    let mut page = Page::new(
        "<table><colgroup id=g><col id=c style='width: 30px'></colgroup>\
           <tr><td>A</td><td>B</td><td>C</td></tr></table>",
    );
    set_style(&mut page, "c", "width: 60px; background-color: red");
    set_style(&mut page, "c", "width: 60px; background-color: blue");
    set(&mut page, "c", "span", "2");
    page.change(|cx, document| {
        let col = element(cx, document, "col", &[("style", "width: 5px")]);
        cx.append_child(find(cx, document, "g"), col).unwrap();
    });
    let blue = |view: &BoxFragment| -> bool {
        fn find(f: &BoxFragment) -> bool {
            f.columns.iter().any(|c| {
                c.style.background_color
                    == computed_style::StyleColor::Rgba(gfx::Color::rgb(0, 0, 255))
            }) || f.children.iter().any(|c| find(&c.fragment))
        }
        find(view)
    };
    assert!(blue(&page.view()));
}

/// A table's borders collapsing and coming apart again, and a cell's
/// border changing in between: each time it lays out as a new tree does.
#[test]
fn collapsed_borders_follow_the_dom() {
    let mut page = Page::new(
        "<table id=t style='border: 4px solid'><tr><td id=a style='border: 2px solid'>A</td>\
           <td id=b>B</td></tr></table>",
    );
    set_style(
        &mut page,
        "t",
        "border: 4px solid; border-collapse: collapse",
    );
    set_style(&mut page, "a", "border: 10px solid");
    set_style(&mut page, "a", "border: 10px solid red");
    // Narrower than the table's and the other cell's: its collapsed border
    // stays the same.
    set_style(
        &mut page,
        "t",
        "border: 20px solid; border-collapse: collapse",
    );
    set_style(&mut page, "b", "border: 20px solid");
    set_style(&mut page, "a", "border: 3px solid");
    set_style(&mut page, "t", "border: 4px solid");
}

/// A collapsed border's color changing: the table's fragment, which
/// carries the borders, has it.
#[test]
fn a_collapsed_borders_color_follows_the_dom() {
    let mut page = Page::new(
        "<table style='border-collapse: collapse'><tr><td id=a style='border: 2px solid red'>A</td>\
           </tr></table>",
    );
    set_style(&mut page, "a", "border: 2px solid blue");
    fn find(f: &BoxFragment) -> Option<gfx::Color> {
        f.collapsed_borders
            .as_ref()
            .map(|b| b.edges[0].color)
            .or_else(|| f.children.iter().find_map(|c| find(&c.fragment)))
    }
    assert_eq!(find(&page.view()), Some(gfx::Color::rgb(0, 0, 255)));
}

/// The fragment of the node `node` in `f`'s tree.
fn fragment_of(f: &BoxFragment, node: gfx::NodeId) -> Option<&BoxFragment> {
    if f.node == Some(node) {
        return Some(f);
    }
    f.children
        .iter()
        .find_map(|c| fragment_of(&c.fragment, node))
}

/// A form control's own size follows its attributes, its font and its
/// type, and a text area's text stays out of its content: each time it
/// lays out as a new tree does.
#[test]
fn form_controls_follow_the_dom() {
    let mut page = Page::without_shadow_trees(
        "<input id=field><textarea id=area>a value</textarea>         <button id=button style='height: 40px'>Go</button>
         <button id=block style='display: block'>Go</button>",
    );
    let width = |page: &Page, id: &str| fragment_of(&page.view(), page.id(id)).unwrap().size.width;
    assert_eq!(width(&page, "field"), 166.0);
    set(&mut page, "field", "size", "1");
    assert_eq!(width(&page, "field"), 33.0);
    set_style(&mut page, "field", "font-size: 20px");
    assert_eq!(width(&page, "field"), 46.0);
    set(&mut page, "field", "type", "range");
    assert_eq!(width(&page, "field"), 129.0);
    set(&mut page, "area", "cols", "10");
    assert_eq!(width(&page, "area"), 91.0);
    page.change(|cx, document| cx.set_data(first_child(cx, document, "area"), "another value"));
    page.change(|cx, document| {
        let span = cx.create_element(document, html("span"));
        let more = cx.create_text(document, "more");
        cx.append_child(span, more).unwrap();
        cx.append_child(find(cx, document, "button"), span).unwrap();
    });
    // Restyled, a block button keeps its fit-content width.
    set_style(&mut page, "block", "display: block; padding: 0");
}

/// A text control's UA shadow tree (which the DOM keeps up to date as the
/// value, the placeholder and the type change) lays out as a new tree
/// does after each change.
#[test]
fn text_controls_follow_their_ua_shadow_trees() {
    let mut page = Page::without_shadow_trees(
        "<input id=field placeholder=Hint><textarea id=area>a value</textarea>",
    );
    page.change(ensure_shadow_trees);
    // Each change, the trees made up to date after it.
    fn change(page: &mut Page, change: impl for<'gc> FnOnce(&Cx<'_, 'gc>, NodeRef<'gc>)) {
        page.change(|cx, document| {
            change(cx, document);
            ensure_shadow_trees(cx, document);
        });
    }
    change(&mut page, |cx, document| {
        cx.set_attribute(find(cx, document, "field"), "value", "Typed")
    });
    change(&mut page, |cx, document| {
        cx.remove_attribute(find(cx, document, "field"), "placeholder");
    });
    change(&mut page, |cx, document| {
        cx.set_attribute(find(cx, document, "field"), "type", "checkbox")
    });
    change(&mut page, |cx, document| {
        cx.set_attribute(find(cx, document, "field"), "type", "password")
    });
    // Into a decorated field, a button and back.
    for ty in ["search", "number", "submit", "button", "text"] {
        change(&mut page, |cx, document| {
            cx.set_attribute(find(cx, document, "field"), "type", ty)
        });
    }
    change(&mut page, |cx, document| {
        let field = find(cx, document, "field");
        cx.set_attribute(field, "type", "search");
        cx.set_attribute(field, "value", "");
    });
    change(&mut page, |cx, document| {
        cx.set_data(
            first_child(cx, document, "area"),
            "a longer value that wraps in the text area",
        )
    });
}

/// A control's UA shadow tree, when it comes and as it follows the
/// control's value, placeholder and type, and its native look's states
/// (painted again: its fragment, nothing built or shaped): the update
/// lays out as a build does.
#[test]
fn a_controls_tree_and_native_look_follow_it() {
    let mut page = Page::without_shadow_trees(
        "<!doctype html><input id=t value=a><input id=c type=checkbox><textarea id=a>x</textarea>",
    );
    page.change(|cx, d| {
        for id in ["t", "c", "a"] {
            cx.ensure_shadow_tree(find(cx, d, id));
        }
    });
    page.change(|cx, d| cx.set_input_value(find(cx, d, "t"), "typed").unwrap());
    set(&mut page, "t", "placeholder", "p");
    page.change(|cx, d| cx.set_input_value(find(cx, d, "t"), "").unwrap());
    page.change(|cx, d| cx.set_textarea_value(find(cx, d, "a"), "two\nlines"));
    set(&mut page, "t", "type", "submit");
    set(&mut page, "t", "type", "search");
    let checked = page.change(|cx, d| cx.set_checked(find(cx, d, "c"), true));
    assert_eq!(checked, update(0, 0, 1), "its native look, painted again");
    let disabled = set(&mut page, "c", "disabled", "");
    assert_eq!((disabled.built, disabled.shaped), (0, 0));
}

/// A state a native look shows (checked, hovered, disabled) changes its
/// box's fragment, as a new tree has it, without laying it out again; a
/// part of a control's UA shadow tree (a range's thumb) follows its
/// control's.
#[test]
fn native_looks_follow_their_controls_states() {
    let mut page = Page::without_shadow_trees(
        // The range's color is its own, not the UA's disabled one: its
        // thumb's style doesn't change with its state.
        "<input type=checkbox id=box><input type=range id=range style='color: red'>
         <button id=button>Go</button>",
    );
    page.change(ensure_shadow_trees);
    // The range's thumb, a part of its UA shadow tree, by an id.
    page.change(|cx, document| {
        let root = cx
            .shadow_root(find(cx, document, "range"))
            .expect("a UA shadow tree");
        let mut thumb = None;
        crate::page::each_node(cx, root, &mut |node| {
            if cx
                .shadow_pseudo_id(node)
                .is_some_and(|pseudo| pseudo == "-webkit-slider-thumb")
            {
                thumb = Some(node);
            }
        });
        cx.set_attribute(thumb.expect("a thumb"), "id", "thumb");
    });
    let state = |page: &Page, id: &str| {
        let view = page.view();
        fragments(&view, page)[id]
            .control_state
            .expect("a native look")
    };
    assert!(!state(&page, "box").checked);
    page.change(|cx, document| cx.set_checked(find(cx, document, "box"), true));
    assert!(state(&page, "box").checked);
    assert_eq!(page.layout.laid_out, 0, "nothing laid out again");
    page.change(|cx, document| cx.set_hover_element(document, Some(find(cx, document, "button"))));
    assert!(state(&page, "button").hovered);
    set(&mut page, "range", "disabled", "");
    assert!(state(&page, "range").disabled);
    // Its thumb shows its range's states, not its own.
    assert!(state(&page, "thumb").disabled);
}

/// A menu list follows its options: a longer label widens it, another
/// selected option shows, `multiple` makes it a list box and back; each
/// as built anew.
#[test]
fn menu_lists_follow_their_options() {
    let mut page = Page::without_shadow_trees(
        "<select id=select><option id=a>A</option><option id=b>B</option></select>",
    );
    page.change(ensure_shadow_trees);
    let width = |page: &Page| {
        let view = page.view();
        fragments(&view, page)["select"].size.width
    };
    let narrow = width(&page);
    page.change(|cx, document| {
        cx.set_attribute(find(cx, document, "a"), "label", "A much longer label");
        ensure_shadow_trees(cx, document);
    });
    assert!(width(&page) > narrow + 50.0);
    page.change(|cx, document| {
        cx.set_attribute(find(cx, document, "b"), "selected", "");
        ensure_shadow_trees(cx, document);
    });
    for value in [Some(""), None] {
        page.change(|cx, document| {
            let select = find(cx, document, "select");
            match value {
                Some(value) => cx.set_attribute(select, "multiple", value),
                None => {
                    cx.remove_attribute(select, "multiple");
                }
            }
            ensure_shadow_trees(cx, document);
        });
    }
}

/// A list box follows its `size` and its options' sizes, each as built
/// anew.
#[test]
fn list_boxes_follow_their_size_and_options() {
    let mut page = Page::without_shadow_trees(
        "<select id=select size=4><option id=a>A</option><option id=b>B</option></select>",
    );
    page.change(ensure_shadow_trees);
    let height = |page: &Page| {
        let view = page.view();
        fragments(&view, page)["select"].size.height
    };
    let four = height(&page);
    page.change(|cx, document| {
        cx.set_attribute(find(cx, document, "select"), "size", "2");
        ensure_shadow_trees(cx, document);
    });
    assert_eq!(height(&page), (four - 2.0) / 2.0 + 2.0);
    // A taller option: every row as tall.
    set_style(&mut page, "a", "font-size: 30px");
    assert!(height(&page) > four);
}

/// The border box of the element whose `id` is `id`, laid out.
fn border_box(page: &mut Page, id: &str) -> gfx::RectF {
    let element = page.id(id);
    let view = page.tree.layout(page.viewport);
    layout::element_box(&view, &Default::default(), &Default::default(), element)
        .expect("a box")
        .border_box
}

/// An `<img>` is replaced: no box of its own content, inline as an atomic
/// inline, sized by its image once its request has one (its boxes built
/// again, as the request changes), by its `width` keeping its ratio.
#[test]
fn an_image_is_replaced_and_built_again_as_its_image_comes() {
    let mut page = Page::new(
        "<!doctype html><body style='margin: 0'>\
         <p id=p style='margin: 0'><img id=i style='vertical-align: top'><img id=w width=30 style='vertical-align: top'></p>",
    );
    page.request_images();
    page.change(|cx, document| {
        cx.set_attribute(find(cx, document, "i"), "src", "a.png");
        cx.set_attribute(find(cx, document, "w"), "src", "a.png");
    });
    assert_eq!(page.request_images(), [format!("{BASE_URL}a.png")]);
    // Loading: no image to size it by.
    page.update();
    assert_eq!(border_box(&mut page, "i").width, 0.0);
    let built = page.image_fetched("a.png", Some(raster(20, 10)));
    assert_eq!(built.built, 2, "each image's box");
    assert_eq!(
        border_box(&mut page, "i"),
        gfx::RectF::new(0.0, 0.0, 20.0, 10.0)
    );
    // Its `width`, the height by its ratio; after the first, on its line.
    assert_eq!(
        border_box(&mut page, "w"),
        gfx::RectF::new(20.0, 0.0, 30.0, 15.0)
    );
    // An SVG image the host made, sized by its natural dimensions: a width
    // and a ratio.
    set(&mut page, "i", "src", "b.svg");
    page.request_images();
    let natural = gfx::svg::SvgNaturalSize {
        width: Some(40.0),
        height: None,
        ratio: Some(4.0),
    };
    page.image_fetched("b.svg", Some(dom::Image::Svg(3, natural)));
    assert_eq!(
        border_box(&mut page, "i"),
        gfx::RectF::new(0.0, 0.0, 40.0, 10.0)
    );
}

/// A line clamp follows its changes, laid out again or built again in
/// part: its number of lines, text added in a block inside it (its
/// ellipsis is shaped ahead there too), and a `-webkit-box` item's flexing.
/// (`check` compares each with the page built anew.)
#[test]
fn line_clamps_and_webkit_boxes_follow_changes() {
    let mut page = Page::new(
        "<style>body { font: 20px/1 Ahem } .c { display: -webkit-box; -webkit-box-orient: vertical;
           -webkit-line-clamp: 2; width: 60px }</style>
         <div id=c class=c><div><div id=d>XX XX XX</div></div>XX</div>
         <div style='display: -webkit-box; width: 300px'><div id=i style='-webkit-box-flex: 1'>X</div><div>X</div></div>",
    );
    set_style(&mut page, "c", "-webkit-line-clamp: 3");
    set_style(&mut page, "c", "-webkit-line-clamp: 1");
    page.change(|cx, document| {
        let text = cx.create_text(document, " XX");
        cx.append_child(find(cx, document, "d"), text).unwrap();
    });
    set_style(&mut page, "i", "-webkit-box-flex: 2");
}

/// A `transform` coming or going changes the containing block of the
/// out-of-flow boxes inside, deeper in than its children too: laid out
/// again, as a containment's change is. (`check` compares each with the
/// page built anew.)
#[test]
fn containing_blocks_follow_transforms_and_containment() {
    let mut page = Page::new(
        "<div id=c style='margin-top: 50px; height: 10px'><div><div style='position: absolute; top: 0'>X</div><div style='position: fixed; top: 0'>X</div></div></div>",
    );
    set_style(
        &mut page,
        "c",
        "margin-top: 50px; height: 10px; transform: translate(0)",
    );
    set_style(
        &mut page,
        "c",
        "margin-top: 50px; height: 10px; transform: translate(5px)",
    );
    set_style(&mut page, "c", "margin-top: 50px; height: 10px");
    set_style(
        &mut page,
        "c",
        "margin-top: 50px; height: 10px; contain: paint",
    );
    set_style(&mut page, "c", "margin-top: 50px; height: 10px");
    set_style(
        &mut page,
        "c",
        "margin-top: 50px; height: 10px; filter: blur(1px)",
    );
}

/// What clips to a `<clipPath>` follows it (each step checked against the
/// page built anew): a shape's attribute, a shape coming, the
/// `<clipPath>`'s own `transform`, an id given later, another `url()`,
/// for a box and for an SVG element alike.
#[test]
fn clip_paths_follow_their_clip_path_elements() {
    let mut page = Page::new(
        "<style>.on ~ clipPath > rect { transform: translate(5px, 0) }</style>
         <svg id=defs width=0 height=0><clipPath id=c><rect id=r width=10 height=10 /></clipPath>
           <g id=g></g><clipPath id=d><rect width=10 height=10 /></clipPath></svg>
         <div id=box style='width: 100px; height: 50px; clip-path: url(#c)'></div>
         <div id=sibling style='width: 100px; height: 50px; clip-path: url(#d)'></div>
         <div id=late style='width: 100px; height: 50px; clip-path: url(#later)'></div>
         <svg width=100 height=100><g style='clip-path: url(#c)'><rect width=50 height=50 /></g></svg>",
    );
    let clip = |page: &Page, id: &str| {
        let f = fragment_of(&page.view(), page.id(id))
            .expect("a box")
            .clone();
        f.clip_path.as_ref().map(|c| {
            c[0].iter()
                .filter_map(|p| p.bounds())
                .fold(gfx::RectF::new(0.0, 0.0, 0.0, 0.0), |a, b| a.union(&b))
        })
    };
    set(&mut page, "r", "width", "30");
    assert_eq!(
        clip(&page, "box"),
        Some(gfx::RectF::new(0.0, 0.0, 30.0, 10.0))
    );
    page.change(|cx, document| {
        let rect = cx.create_element(document, svg("rect"));
        cx.set_attribute(rect, "y", "20");
        cx.set_attribute(rect, "width", "5");
        cx.set_attribute(rect, "height", "5");
        cx.append_child(find(cx, document, "c"), rect).unwrap();
    });
    assert_eq!(
        clip(&page, "box"),
        Some(gfx::RectF::new(0.0, 0.0, 30.0, 25.0))
    );
    set(&mut page, "c", "transform", "translate(1 2)");
    assert_eq!(
        clip(&page, "box"),
        Some(gfx::RectF::new(1.0, 2.0, 30.0, 25.0))
    );
    assert_eq!(clip(&page, "late"), None);
    set(&mut page, "c", "id", "later");
    assert_eq!(
        clip(&page, "late"),
        Some(gfx::RectF::new(1.0, 2.0, 30.0, 25.0))
    );
    assert_eq!(clip(&page, "box"), None);
    set_style(
        &mut page,
        "box",
        "width: 100px; height: 50px; clip-path: inset(1px)",
    );
    assert_eq!(
        clip(&page, "box"),
        Some(gfx::RectF::new(1.0, 1.0, 98.0, 48.0))
    );
    set_style(
        &mut page,
        "box",
        "width: 100px; height: 50px; clip-path: url(#later)",
    );
    assert_eq!(
        clip(&page, "box"),
        Some(gfx::RectF::new(1.0, 2.0, 30.0, 25.0))
    );
    // A child styled again alone, the `<clipPath>` not.
    assert_eq!(
        clip(&page, "sibling"),
        Some(gfx::RectF::new(0.0, 0.0, 10.0, 10.0))
    );
    set(&mut page, "g", "class", "on");
    assert_eq!(
        clip(&page, "sibling"),
        Some(gfx::RectF::new(5.0, 0.0, 10.0, 10.0))
    );
}

/// An out-of-flow box's parent has it where it is in tree order, before the
/// child after it, with the fragment its containing block holds: made
/// again whenever that is, though the parent wasn't laid out again.
#[test]
fn an_out_of_flow_box_is_where_its_parent_has_it() {
    let mut page = Page::new(
        "<div id=cb style='position: relative'><div id=parent>
           <p>A</p><div id=abs style='position: absolute'></div><p>B</p>
         </div><div id=after></div></div>",
    );
    for style in [
        "position: absolute",
        "position: absolute; width: 10px",
        "position: absolute; width: 10px; color: blue",
    ] {
        set_style(&mut page, "abs", style);
        let view = page.view();
        let fragments = fragments(&view, &page);
        let (cb, parent) = (&fragments["cb"], &fragments["parent"]);
        assert_eq!(parent.out_of_flow.len(), 1, "{style}");
        let place = &parent.out_of_flow[0];
        assert_eq!(place.before, 1);
        let held = &cb.children.last().expect("its out-of-flow box").fragment;
        assert!(Arc::ptr_eq(&place.fragment, held), "{style}");
        assert!(Arc::ptr_eq(held, &fragments["abs"]));
    }
    // Laid out again by its containing block alone, taller for what comes
    // after its parent: its parent made again with its new fragment (the
    // harness checks the tree against one built from scratch).
    set_style(&mut page, "abs", "position: absolute; height: 50%");
    set_style(&mut page, "after", "height: 100px");
}

/// A box that stops being the containing block of a fixed box in it (its
/// `transform` gone), which another out-of-flow box beside it is then laid
/// out with: each is where its parent has it with the fragment made last
/// (the harness checks the tree against one built from scratch).
#[test]
fn out_of_flow_boxes_hoisted_elsewhere_are_where_their_parents_have_them() {
    let mut page = Page::new(
        "<style>#t { position: absolute; top: 100px; left: 100px }
           #f { position: fixed; top: 150px; width: 10px; height: 10px }</style>
         <body><div id=t style='transform: translateX(4px)'><div style='position: absolute'></div><div id=f></div></div>",
    );
    set_style(&mut page, "t", "");
    let view = page.view();
    let fragments = fragments(&view, &page);
    let held = |id: &str| {
        Arc::ptr_eq(
            &fragments[id],
            &view
                .children
                .iter()
                .find(|c| Arc::ptr_eq(&c.fragment, &fragments[id]))
                .expect("the viewport's")
                .fragment,
        )
    };
    assert!(held("t") && held("f"));
    let places = &fragments["t"].out_of_flow;
    assert_eq!(places.len(), 2);
    assert!(Arc::ptr_eq(&places[1].fragment, &fragments["f"]));
}

/// What a selection goes over (Blink's `LayoutSelection` walks the nodes
/// with a layout object): text with something left after white space
/// processing, a `<br>`, a replaced element; and as the DOM changes.
#[test]
fn what_a_selection_goes_over() {
    let mut page = Page::new(
        "<p id=a>ab</p>\n<p id=hidden style='display: none'>x</p><div id=d><br id=br><span id=s>  </span>\
         <img id=img>c</div><p id=empty></p><div><span id=anonymous>e</span><p>f</p></div>",
    );
    let rendered = |page: &Page, id: &str, child: bool| {
        let node = page.read(|cx, document| {
            let element = find(cx, document, id);
            cx.node_id(if child {
                cx.first_child(element).unwrap()
            } else {
                element
            })
        });
        page.tree.is_rendered(node)
    };
    let between =
        page.read(|cx, document| cx.node_id(cx.next_sibling(find(cx, document, "a")).unwrap()));
    assert!(rendered(&page, "a", true));
    assert!(
        !page.tree.is_rendered(between),
        "white space between blocks"
    );
    assert!(!rendered(&page, "hidden", true));
    assert!(rendered(&page, "br", false));
    assert!(rendered(&page, "img", false));
    assert!(!rendered(&page, "s", true), "spaces that collapse away");
    assert!(!rendered(&page, "d", false), "a block isn't");
    assert!(!rendered(&page, "empty", false));
    assert!(rendered(&page, "anonymous", true), "in an anonymous block");
    // Shown, and its text with it; hidden again.
    set_style(&mut page, "hidden", "");
    assert!(rendered(&page, "hidden", true));
    set_style(&mut page, "hidden", "display: none");
    assert!(!rendered(&page, "hidden", true));
    // Spaces that are kept now.
    set_style(&mut page, "s", "white-space: pre");
    assert!(rendered(&page, "s", true));
    // A text node moved to another block, and removed.
    let text = page.read(|cx, document| cx.root(first_child(cx, document, "a")));
    let id = page.dom.read(|cx| cx.node_id(text.get(cx.gc)));
    page.change(|cx, document| {
        cx.append_child(find(cx, document, "d"), text.get(cx.gc))
            .unwrap();
    });
    assert!(page.tree.is_rendered(id));
    page.change(|cx, _| cx.remove(text.get(cx.gc)));
    assert!(!page.tree.is_rendered(id));
}
