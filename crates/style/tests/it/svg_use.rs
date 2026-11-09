//! SVG's `<use>`: the styles of the copy it draws of what it references
//! (inheriting from the `<use>`, matching rules within the copy), and that
//! a change to what it references, or to the reference, styles it again.
//! The expected values are Chrome 152's, read from the elements of the
//! `<use>`'s shadow tree.

use std::sync::Arc;

use computed_style::ComputedStyle;
use dom::{Cx, LocalToken, NodeId, NodeKind, NodeRef, QualName};
use style_resolver::computed_value;

use crate::{Page, by_id, id_of};

/// `html`, parsed, its `<use>`s' copies made, styled.
fn page(html: &str) -> Page {
    Page::with_ua_trees(html)
}

/// `f` in an operation, with the element whose `id` is `id` outside the
/// copies (which have their originals' ids).
fn with<R>(
    page: &mut Page,
    id: &str,
    f: impl for<'gc> FnOnce(&Cx<'_, 'gc>, NodeRef<'gc>) -> R,
) -> R {
    page.with_document(|cx, document| {
        let element = by_id(cx, document, id).unwrap_or_else(|| panic!("no #{id}"));
        f(cx, element)
    })
}

/// The root of the copy of the `<use>` `id`, as its node and its style.
fn instance(page: &mut Page, id: &str) -> Option<(NodeId, Arc<ComputedStyle>)> {
    let Page {
        dom,
        document,
        token,
        ..
    } = page;
    dom.read(|cx| {
        let user = by_id(cx, document.get(cx.gc), id).unwrap_or_else(|| panic!("no #{id}"));
        let copy = cx.use_copy(user)?;
        Some((cx.node_id(copy), cx.computed_style(copy, token)?.clone()))
    })
}

/// The copy `id` draws, one line per element (indented by depth): the
/// copied element's id and `properties`' computed values; a nested
/// `<use>`'s copy follows it, marked `->`.
fn copy(page: &mut Page, id: &str, properties: &[&str]) -> String {
    let Page {
        dom,
        document,
        token,
        ..
    } = page;
    dom.read(|cx| {
        let user = by_id(cx, document.get(cx.gc), id).unwrap_or_else(|| panic!("no #{id}"));
        let mut out = String::new();
        if let Some(copy) = cx.use_copy(user) {
            describe(cx, token, copy, properties, 0, &mut out);
        }
        out
    })
}

fn describe<'gc>(
    cx: &Cx<'_, 'gc>,
    token: &LocalToken,
    element: NodeRef<'gc>,
    properties: &[&str],
    depth: usize,
    out: &mut String,
) {
    assert_eq!(cx.kind(element), NodeKind::Element);
    let id = id_of(cx, element).unwrap_or_default();
    let style = cx.computed_style(element, token).expect("a copy is styled");
    let values: Vec<String> = properties
        .iter()
        .map(|property| computed_value(style, property).unwrap())
        .collect();
    out.push_str(&format!(
        "{}{id} {}\n",
        "  ".repeat(depth),
        values.join(" ")
    ));
    let mut child = cx.first_child(element);
    while let Some(node) = child {
        if cx.kind(node) == NodeKind::Element {
            describe(cx, token, node, properties, depth + 1, out);
        }
        child = cx.next_sibling(node);
    }
    if cx.is_svg_use(element)
        && let Some(used) = cx.use_copy(element)
    {
        out.push_str(&format!("{}->\n", "  ".repeat(depth + 1)));
        describe(cx, token, used, properties, depth + 1, out);
    }
}

/// The styles of every element of every copy in the document, in tree
/// order (a nested `<use>`'s copy after it).
fn copy_styles(page: &mut Page) -> Vec<ComputedStyle> {
    fn walk<'gc>(
        cx: &Cx<'_, 'gc>,
        token: &LocalToken,
        node: NodeRef<'gc>,
        in_copy: bool,
        out: &mut Vec<ComputedStyle>,
    ) {
        if in_copy && cx.kind(node) == NodeKind::Element {
            out.push((**cx.computed_style(node, token).expect("styled")).clone());
        }
        if cx.is_svg_use(node)
            && let Some(copy) = cx.use_copy(node)
        {
            walk(cx, token, copy, true, out);
        }
        let mut child = cx.first_child(node);
        while let Some(at) = child {
            walk(cx, token, at, in_copy, out);
            child = cx.next_sibling(at);
        }
    }
    let Page {
        dom,
        document,
        token,
        ..
    } = page;
    dom.read(|cx| {
        let mut out = Vec::new();
        walk(cx, token, document.get(cx.gc), false, &mut out);
        out
    })
}

/// Changes the DOM with `change`, makes the copies again and styles it
/// again, and checks each copy is what a new engine styling everything
/// gives.
fn change(page: &mut Page, change: impl for<'gc> FnOnce(&Cx<'_, 'gc>, NodeRef<'gc>)) {
    page.with_document(|cx, document| {
        change(cx, document);
        cx.update_use_trees();
    });
    page.recalc();
    let before = copy_styles(page);
    page.check();
    assert_eq!(
        before,
        copy_styles(page),
        "a <use>'s copy: what styling everything gives"
    );
}

/// An SVG element named `local`, with id `id`.
fn svg_element<'gc>(
    cx: &Cx<'_, 'gc>,
    document: NodeRef<'gc>,
    local: &str,
    id: &str,
) -> NodeRef<'gc> {
    let element = cx.create_element(
        document,
        QualName {
            namespace: Some("http://www.w3.org/2000/svg"),
            prefix: None,
            local,
        },
    );
    cx.set_attribute(element, "id", id);
    element
}

/// The copy inherits from the `<use>`, not from where what it copies is;
/// its own presentation attributes and rules still apply.
#[test]
fn a_copy_inherits_from_its_use() {
    let mut page = page(
        "<style>.x { stroke-width: 5px }</style>
        <svg><defs stroke=red>
          <g id=g fill=green><rect id=r class=x /><circle id=c fill=blue /></g>
        </defs>
        <use id=u href=#g stroke=yellow opacity=0.5 /></svg>",
    );
    assert_eq!(
        copy(
            &mut page,
            "u",
            &["fill", "stroke", "stroke-width", "opacity"]
        ),
        "g rgb(0, 128, 0) rgb(255, 255, 0) 1px 1\n\
         \x20 r rgb(0, 128, 0) rgb(255, 255, 0) 5px 1\n\
         \x20 c rgb(0, 0, 255) rgb(255, 255, 0) 1px 1\n",
        "opacity isn't inherited"
    );
    // The original keeps its own.
    let Page {
        dom,
        document,
        token,
        ..
    } = &mut page;
    let stroke = dom.read(|cx| {
        let r = by_id(cx, document.get(cx.gc), "r").unwrap();
        computed_value(cx.computed_style(r, token).unwrap(), "stroke").unwrap()
    });
    assert_eq!(stroke, "rgb(255, 0, 0)");
}

/// A copy's rules match in the copy: its root has no parent or siblings
/// (Chrome's shadow tree), so `defs rect` matches the original only.
#[test]
fn rules_match_within_the_copy() {
    let mut page = page(
        "<style>
          rect { fill: red }
          defs rect, svg > g > rect { fill: blue }
          g > rect { stroke: green }
          rect:first-child { stroke-width: 3px }
          rect + rect { opacity: 0.5 }
        </style>
        <svg><defs><g><rect id=a /><rect id=b /></g></defs>
        <use id=ua href=#a /><use id=ub href=#b /></svg>",
    );
    let properties = ["fill", "stroke", "stroke-width", "opacity"];
    assert_eq!(
        copy(&mut page, "ua", &properties),
        "a rgb(255, 0, 0) none 3px 1\n"
    );
    assert_eq!(
        copy(&mut page, "ub", &properties),
        "b rgb(255, 0, 0) none 3px 1\n"
    );
    // Below the root, the copy's tree is the original's, up to the root.
    drop(page);
    let mut page = self::page(
        "<style>g > rect { stroke: green } rect + rect { opacity: 0.5 } defs rect { fill: blue }</style>
        <svg><defs><g id=g><rect id=a /><rect id=b /></g></defs><use id=u href=#g /></svg>",
    );
    assert_eq!(
        copy(&mut page, "u", &["stroke", "opacity", "fill"]),
        "g none 1 rgb(0, 0, 0)\n  a rgb(0, 128, 0) 1 rgb(0, 0, 0)\n  b rgb(0, 128, 0) 0.5 rgb(0, 0, 0)\n"
    );
}

#[test]
fn xlink_href_is_followed_after_href() {
    let mut page = page(
        "<svg xmlns:xlink='http://www.w3.org/1999/xlink'>
          <rect id=a /><rect id=b />
          <use id=old xlink:href=#a /><use id=both href=#b xlink:href=#a />
          <use id=other href=a /><use id=nothing /><use id=encoded href=#%62 /></svg>",
    );
    assert_eq!(copy(&mut page, "old", &[]), "a \n");
    assert_eq!(copy(&mut page, "both", &[]), "b \n");
    assert_eq!(copy(&mut page, "other", &[]), "", "not a fragment");
    assert_eq!(copy(&mut page, "nothing", &[]), "");
    assert_eq!(copy(&mut page, "encoded", &[]), "b \n", "percent-decoded");
}

/// The first element with the id in tree order, as `getElementById`.
#[test]
fn the_first_element_with_the_id_is_used() {
    let mut page =
        page("<svg><use id=u href=#x /><rect id=x fill=green /><rect id=x fill=red /></svg>");
    assert_eq!(copy(&mut page, "u", &["fill"]), "x rgb(0, 128, 0)\n");
}

#[test]
fn nested_uses_are_copied_with_their_copies() {
    let mut page = page(
        "<svg><defs>
          <rect id=r />
          <g id=g fill=green><use id=inner href=#r /></g>
        </defs><use id=u href=#g /></svg>",
    );
    assert_eq!(
        copy(&mut page, "u", &["fill"]),
        "g rgb(0, 128, 0)\n  inner rgb(0, 128, 0)\n    ->\n    r rgb(0, 128, 0)\n"
    );
}

/// A `<use>` in what it references, directly or through other `<use>`s,
/// draws nothing at the point the cycle closes.
#[test]
fn cycles_draw_nothing() {
    let mut page = page(
        "<svg><use id=self href=#self />
        <g id=a><use id=in-a href=#a /></g>
        <g id=b><use id=b-to-c href=#c /></g>
        <g id=c><use id=c-to-b href=#b /></g>
        <use id=outside href=#b /></svg>",
    );
    assert_eq!(copy(&mut page, "self", &[]), "");
    assert_eq!(copy(&mut page, "in-a", &[]), "");
    // Inside b's copy, c's `<use>` would copy b again.
    assert_eq!(
        copy(&mut page, "outside", &[]),
        "b \n  b-to-c \n    ->\n    c \n      c-to-b \n"
    );
    // In c's copy, b's `<use>` would copy b, which b-to-c is in.
    assert_eq!(copy(&mut page, "b-to-c", &[]), "c \n  c-to-b \n");
    // A `<use>` that references what it is in, from a copy, copies it
    // once: the copy is in the `<use>`'s copy, not in the element.
    drop(page);
    let mut page = self::page(
        "<svg><defs><g id=a><g id=x><use id=n href=#a /></g></g></defs>
        <use id=top href=#x /></svg>",
    );
    assert_eq!(copy(&mut page, "n", &[]), "");
    assert_eq!(
        copy(&mut page, "top", &[]),
        "x \n  n \n    ->\n    a \n      x \n        n \n"
    );
    // A `<use>` in a copy that references the `<use>` the copy is in.
    drop(page);
    let mut page = self::page(
        "<svg><defs><g id=g><use id=inner href=#outer /></g></defs>
        <use id=outer href=#g /></svg>",
    );
    assert_eq!(copy(&mut page, "outer", &[]), "g \n  inner \n");
}

/// Many `<use>`s, each referencing a group of the one before twice over,
/// would copy 2^40 elements: past a limit, the `<use>` draws nothing, and
/// copying stops there.
#[test]
fn too_many_copies_draw_nothing() {
    let mut markup = String::from("<svg><defs><g id=g0><rect /><rect /></g>");
    for i in 1..40 {
        let previous = i - 1;
        markup.push_str(&format!(
            "<g id=g{i}><use href=#g{previous} /><use href=#g{previous} /></g>"
        ));
    }
    markup.push_str("</defs><use id=small href=#g3 /><use id=huge href=#g39 /></svg>");
    let mut page = page(&markup);
    assert!(instance(&mut page, "small").is_some());
    assert!(instance(&mut page, "huge").is_none());
}

#[test]
fn a_change_in_what_is_copied_styles_the_copy_again() {
    let mut page = page(
        "<svg><defs><g id=g><rect id=r fill=red /></g></defs>
        <use id=u href=#g /><rect id=other /></svg>",
    );
    let before = instance(&mut page, "u").unwrap();
    // Something it doesn't copy: the copy stays.
    change(&mut page, |cx, document| {
        let other = by_id(cx, document, "other").unwrap();
        cx.set_attribute(other, "fill", "blue");
    });
    let after = instance(&mut page, "u").unwrap();
    assert!(before.0 == after.0 && Arc::ptr_eq(&before.1, &after.1));
    change(&mut page, |cx, document| {
        let r = by_id(cx, document, "r").unwrap();
        cx.set_attribute(r, "fill", "green");
    });
    assert_eq!(
        copy(&mut page, "u", &["fill"]),
        "g rgb(0, 0, 0)\n  r rgb(0, 128, 0)\n"
    );
    // A new child of what it copies.
    change(&mut page, |cx, document| {
        let g = by_id(cx, document, "g").unwrap();
        let circle = svg_element(cx, document, "circle", "new");
        cx.append_child(g, circle).unwrap();
    });
    assert_eq!(copy(&mut page, "u", &[]), "g \n  r \n  new \n");
    change(&mut page, |cx, document| {
        cx.remove(by_id(cx, document, "r").unwrap());
    });
    assert_eq!(copy(&mut page, "u", &[]), "g \n  new \n");
}

#[test]
fn a_change_to_the_use_styles_the_copy_again() {
    let mut page = page(
        "<style>.green { fill: green }</style>
        <svg><rect id=a /><circle id=b /><use id=u href=#a /></svg>",
    );
    change(&mut page, |cx, document| {
        let u = by_id(cx, document, "u").unwrap();
        cx.set_attribute(u, "class", "green");
    });
    assert_eq!(copy(&mut page, "u", &["fill"]), "a rgb(0, 128, 0)\n");
    change(&mut page, |cx, document| {
        let u = by_id(cx, document, "u").unwrap();
        cx.set_attribute(u, "href", "#b");
    });
    assert_eq!(copy(&mut page, "u", &["fill"]), "b rgb(0, 128, 0)\n");
}

/// The reference finds another element, or one at all, with a change
/// elsewhere in the document (Blink's `IdTargetObserver`).
#[test]
fn a_change_to_what_the_reference_finds_styles_the_copy_again() {
    let mut page = page("<svg id=svg><use id=u href=#later /><rect id=a /><rect id=b /></svg>");
    assert_eq!(copy(&mut page, "u", &[]), "");
    change(&mut page, |cx, document| {
        let b = by_id(cx, document, "b").unwrap();
        cx.set_attribute(b, "id", "later");
    });
    assert_eq!(copy(&mut page, "u", &[]), "later \n");
    // An element with the id before it in tree order.
    change(&mut page, |cx, document| {
        let parent = by_id(cx, document, "svg").unwrap();
        let a = by_id(cx, document, "a").unwrap();
        let first = svg_element(cx, document, "circle", "later");
        cx.insert(parent, first, Some(a));
    });
    let name = with(&mut page, "u", |cx, u| {
        cx.node_name(cx.use_copy(u).unwrap()).to_string()
    });
    assert_eq!(name, "circle");
    // The element goes: the `<use>` draws nothing.
    change(&mut page, |cx, document| {
        cx.remove(by_id(cx, document, "later").unwrap());
    });
    change(&mut page, |cx, document| {
        cx.remove(by_id(cx, document, "later").unwrap());
    });
    assert_eq!(copy(&mut page, "u", &[]), "");
}

/// A `<use>` taken out of the document and put back is styled anew.
#[test]
fn a_use_put_back_is_copied_again() {
    let mut page = page("<svg id=svg><rect id=a fill=green /><use id=u href=#a /></svg>");
    let u = with(&mut page, "u", |cx, u| elise::Persistent::new(cx.gc, u));
    change(&mut page, |cx, _| cx.remove(u.get(cx.gc)));
    change(&mut page, |cx, document| {
        let svg = by_id(cx, document, "svg").unwrap();
        cx.append_child(svg, u.get(cx.gc)).unwrap();
    });
    assert_eq!(copy(&mut page, "u", &["fill"]), "a rgb(0, 128, 0)\n");
}
