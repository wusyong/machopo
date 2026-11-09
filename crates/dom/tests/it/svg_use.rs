//! SVG's `<use>`: its copy, in a closed UA shadow tree, made before style
//! (`update_use_trees`) of the SVG elements and texts of what it references
//! (those it doesn't draw too, not a `<script>`, nothing outside SVG), with
//! the `<use>`s in it copied in turn into theirs; `href` and SVG 1.1's
//! `xlink:href`; made again as its `href` changes, as an id finds another
//! element, as what it copied changes, and not otherwise; nothing for a
//! reference to no element or outside SVG, for a cycle, for more than
//! `MAX_COPIES` elements; taken away and forgotten once it leaves; its
//! elements no parts of a host.

use dom::{Cx, Dom, NodeRef, QualName};

const SVG: &str = "http://www.w3.org/2000/svg";

fn svg_name(local: &str) -> QualName<'_> {
    QualName {
        namespace: Some(SVG),
        prefix: None,
        local,
    }
}

/// A document with a window, and an `<svg>` in its `<body>`: the `<svg>`.
fn page<'gc>(cx: &Cx<'_, 'gc>) -> NodeRef<'gc> {
    let document = cx.create_document(true);
    cx.create_window(document);
    let html = cx.create_element(document, QualName::html("html"));
    cx.append_child(document, html).unwrap();
    let body = cx.create_element(document, QualName::html("body"));
    cx.append_child(html, body).unwrap();
    let svg = cx.create_element(document, svg_name("svg"));
    cx.append_child(body, svg).unwrap();
    svg
}

/// An SVG element `name` with `attributes`, appended to `parent`.
fn add<'gc>(
    cx: &Cx<'_, 'gc>,
    parent: NodeRef<'gc>,
    name: &str,
    attributes: &[(&str, &str)],
) -> NodeRef<'gc> {
    let element = cx.create_element(cx.document_of(parent), svg_name(name));
    for (name, value) in attributes {
        cx.set_attribute(element, name, value);
    }
    cx.append_child(parent, element).unwrap();
    element
}

/// Whether `node` is `expected`.
fn same<'gc>(node: Option<NodeRef<'gc>>, expected: NodeRef<'gc>) -> bool {
    node.is_some_and(|node| elise::Gc::ptr_eq(node, expected))
}

/// `node`'s subtree as markup-like text: elements by name with their ids,
/// texts by data, a `<use>`'s copy after `->`.
fn shape<'gc>(cx: &Cx<'_, 'gc>, node: NodeRef<'gc>) -> String {
    let mut out = String::new();
    if let Some(data) = cx.data(node) {
        out.push_str(&format!("{:?}", data.to_str()));
        return out;
    }
    out.push_str(&cx.node_name(node).to_str().to_lowercase());
    if let Some(id) = cx.attribute_value_ns(node, None, "id") {
        out.push_str(&format!("#{}", id.borrow().to_str()));
    }
    let mut children = Vec::new();
    let mut child = cx.first_child(node);
    while let Some(at) = child {
        children.push(shape(cx, at));
        child = cx.next_sibling(at);
    }
    if !children.is_empty() {
        out.push_str(&format!("({})", children.join(" ")));
    }
    if let Some(copy) = cx.use_copy(node) {
        out.push_str(&format!("->{}", shape(cx, copy)));
    }
    out
}

#[test]
fn a_use_copies_what_it_references_into_a_closed_shadow_tree() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let svg = page(cx);
        let defs = add(cx, svg, "defs", &[]);
        let g = add(cx, defs, "g", &[("id", "g"), ("fill", "lime")]);
        add(cx, g, "rect", &[("width", "10")]);
        add(cx, g, "defs", &[]);
        let text = add(cx, g, "text", &[]);
        cx.append_child(text, cx.create_text(cx.document_of(svg), "hi"))
            .unwrap();
        add(cx, g, "script", &[]);
        let foreign = add(cx, g, "foreignObject", &[]);
        let div = cx.create_element(cx.document_of(svg), QualName::html("div"));
        cx.append_child(foreign, div).unwrap();
        let user = add(cx, svg, "use", &[("href", "#g")]);
        let old = add(cx, svg, "use", &[]);
        let xlink = QualName {
            namespace: Some("http://www.w3.org/1999/xlink"),
            prefix: Some("xlink"),
            local: "href",
        };
        cx.set_attribute_ns(old, xlink, "#g");
        // Nothing until the copies are made, before style.
        assert!(cx.use_copy(user).is_none());
        cx.update_use_trees();
        let copied = "g#g(rect defs text(\"hi\") foreignobject)";
        assert_eq!(shape(cx, user), format!("use->{copied}"));
        assert_eq!(shape(cx, old), format!("use->{copied}"));
        // A closed UA shadow root: script never gets it.
        let root = cx.user_agent_shadow_root(user).expect("a UA shadow root");
        assert!(same(cx.shadow_root(user), root));
        // The copy's elements are in it, of the `<use>` there.
        let copy = cx.use_copy(user).unwrap();
        assert!(same(cx.use_tree_host(copy), user));
        assert!(same(cx.outermost_use(copy), user));
        assert!(cx.use_tree_host(g).is_none());
        // SVG 1.1's changed too.
        let xlink = QualName {
            namespace: Some("http://www.w3.org/1999/xlink"),
            prefix: Some("xlink"),
            local: "href",
        };
        cx.set_attribute_ns(old, xlink, "#missing");
        cx.update_use_trees();
        assert_eq!(shape(cx, old), "use");
        // Its ids are its tree's: the document finds the originals.
        assert!(
            cx.element_by_id(cx.document_of(svg), "g")
                .is_some_and(|found| elise::Gc::ptr_eq(found, g))
        );
    });
}

#[test]
fn a_use_of_nothing_copies_nothing() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let svg = page(cx);
        let document = cx.document_of(svg);
        let p = cx.create_element(document, QualName::html("p"));
        cx.set_attribute(p, "id", "p");
        cx.append_child(svg, p).unwrap();
        let script = add(cx, svg, "script", &[("id", "s")]);
        let users = [
            add(cx, svg, "use", &[("href", "#missing")]),
            add(cx, svg, "use", &[("href", "#p")]),
            add(cx, svg, "use", &[("href", "#s")]),
            add(cx, svg, "use", &[("href", "other.svg#p")]),
            add(cx, svg, "use", &[]),
        ];
        cx.update_use_trees();
        for user in users {
            assert!(cx.use_copy(user).is_none(), "{}", shape(cx, user));
        }
        let _ = script;
    });
}

#[test]
fn a_use_in_a_copy_copies_too_but_not_in_a_cycle() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let svg = page(cx);
        let defs = add(cx, svg, "defs", &[]);
        add(cx, defs, "rect", &[("id", "r")]);
        let g = add(cx, defs, "g", &[("id", "g")]);
        add(cx, g, "use", &[("href", "#r")]);
        // Itself, and what it is in.
        add(cx, g, "use", &[("href", "#g")]);
        let user = add(cx, svg, "use", &[("id", "u"), ("href", "#g")]);
        let of_use = add(cx, svg, "use", &[("href", "#u")]);
        let itself = add(cx, svg, "use", &[("id", "self"), ("href", "#self")]);
        // Through the `<use>` whose copy it is in.
        let g2 = add(cx, defs, "g", &[("id", "g2")]);
        add(cx, g2, "use", &[("href", "#u2")]);
        let outer = add(cx, svg, "use", &[("id", "u2"), ("href", "#g2")]);
        cx.update_use_trees();
        let copied = "g#g(use->rect#r use)";
        assert_eq!(shape(cx, user), format!("use#u->{copied}"));
        assert_eq!(shape(cx, of_use), format!("use->use#u->{copied}"));
        assert_eq!(shape(cx, itself), "use#self");
        assert_eq!(shape(cx, outer), "use#u2->g#g2(use)");
        // The deepest copy's `<use>` outside any copy.
        let inner = cx.use_copy(cx.use_copy(of_use).unwrap()).unwrap();
        assert!(same(cx.outermost_use(inner), of_use));
    });
}

#[test]
fn a_use_of_too_many_elements_copies_nothing() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let svg = page(cx);
        let defs = add(cx, svg, "defs", &[]);
        // 2^14 rects in the last: more than `MAX_COPIES`.
        add(cx, defs, "rect", &[("id", "l0")]);
        for level in 1..=14 {
            let g = add(cx, defs, "g", &[("id", &format!("l{level}"))]);
            for _ in 0..2 {
                add(cx, g, "use", &[("href", &format!("#l{}", level - 1))]);
            }
        }
        let small = add(cx, svg, "use", &[("href", "#l3")]);
        let big = add(cx, svg, "use", &[("href", "#l14")]);
        cx.update_use_trees();
        assert!(cx.use_copy(small).is_some());
        assert!(cx.use_copy(big).is_none());
        const { assert!(dom::MAX_COPIES < 1 << 14) };
    });
}

#[test]
fn a_use_follows_what_it_references() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let svg = page(cx);
        let defs = add(cx, svg, "defs", &[]);
        let r = add(cx, defs, "rect", &[("id", "r")]);
        let g = add(cx, defs, "g", &[("id", "g")]);
        let circle = add(cx, g, "circle", &[]);
        let user = add(cx, svg, "use", &[("href", "#r")]);
        let other = add(cx, svg, "use", &[("href", "#g")]);
        let later = add(cx, svg, "use", &[("href", "#later")]);
        cx.update_use_trees();
        assert_eq!(shape(cx, user), "use->rect#r");
        let copy = |user| cx.use_copy(user).unwrap();
        let (kept, kept_other) = (copy(user), copy(other));
        // Its target's attribute: copied again.
        cx.set_attribute(r, "width", "5");
        cx.update_use_trees();
        assert!(!elise::Gc::ptr_eq(copy(user), kept));
        assert_eq!(
            cx.attribute_value_ns(copy(user), None, "width")
                .map(|value| value.borrow().to_str().into_owned()),
            Some("5".to_owned())
        );
        // What wasn't copied changed: the copies stay.
        assert!(elise::Gc::ptr_eq(copy(other), kept_other));
        // Something in what it copied: a child, a text.
        let kept = copy(other);
        add(cx, circle, "title", &[]);
        cx.update_use_trees();
        assert_eq!(shape(cx, other), "use->g#g(circle(title))");
        assert!(!elise::Gc::ptr_eq(copy(other), kept));
        let title = cx.first_child(circle).unwrap();
        let text = cx.create_text(cx.document_of(svg), "a");
        cx.append_child(title, text).unwrap();
        cx.update_use_trees();
        let kept = copy(other);
        cx.set_data(text, "b");
        cx.update_use_trees();
        assert_eq!(shape(cx, other), "use->g#g(circle(title(\"b\")))");
        assert!(!elise::Gc::ptr_eq(copy(other), kept));
        cx.remove(cx.first_child(circle).unwrap());
        cx.update_use_trees();
        assert_eq!(shape(cx, other), "use->g#g(circle)");
        // An element given the id later, another `href`.
        let ellipse = add(cx, defs, "ellipse", &[]);
        cx.update_use_trees();
        assert_eq!(shape(cx, later), "use");
        cx.set_attribute(ellipse, "id", "later");
        cx.update_use_trees();
        assert_eq!(shape(cx, later), "use->ellipse#later");
        cx.set_attribute(user, "href", "#g");
        cx.update_use_trees();
        assert_eq!(shape(cx, user), "use->g#g(circle)");
        // An element with the id before it in tree order.
        let first = cx.create_element(cx.document_of(svg), svg_name("line"));
        cx.set_attribute(first, "id", "later");
        cx.insert(defs, first, Some(r));
        cx.update_use_trees();
        assert_eq!(shape(cx, later), "use->line#later");
        // The id taken away: nothing.
        cx.remove_attribute(first, "id");
        cx.set_attribute(cx.next_sibling(g).unwrap(), "id", "gone");
        cx.update_use_trees();
        assert_eq!(shape(cx, later), "use");
        // Its target leaves.
        cx.remove(g);
        cx.update_use_trees();
        assert_eq!(shape(cx, user), "use");
        assert_eq!(shape(cx, other), "use");
    });
}

#[test]
fn a_use_that_leaves_is_forgotten() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let svg = page(cx);
        let r = add(cx, svg, "rect", &[("id", "r")]);
        let user = add(cx, svg, "use", &[("href", "#r")]);
        cx.update_use_trees();
        assert!(cx.use_copy(user).is_some());
        cx.remove(user);
        cx.update_use_trees();
        assert!(cx.use_copy(user).is_none());
        // Out of the document, it follows nothing.
        cx.set_attribute(r, "width", "1");
        cx.update_use_trees();
        assert!(cx.use_copy(user).is_none());
        // Back, it copies again.
        cx.append_child(svg, user).unwrap();
        cx.update_use_trees();
        assert_eq!(shape(cx, user), "use->rect#r");
    });
}

#[test]
fn a_copys_elements_are_no_parts() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let svg = page(cx);
        add(
            cx,
            svg,
            "rect",
            &[("id", "r"), ("pseudo", "-webkit-slider-thumb")],
        );
        let user = add(cx, svg, "use", &[("href", "#r")]);
        cx.update_use_trees();
        let copy = cx.use_copy(user).unwrap();
        assert!(cx.shadow_pseudo_id(copy).is_none());
    });
}

#[test]
fn a_use_moved_into_a_shadow_tree_copies_from_it() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let svg = page(cx);
        let document = cx.document_of(svg);
        add(cx, svg, "rect", &[("id", "r")]);
        let user = add(cx, svg, "use", &[("href", "#r")]);
        let host = cx.create_element(document, QualName::html("div"));
        cx.append_child(cx.parent(svg).unwrap(), host).unwrap();
        let root = cx.attach_shadow(
            host,
            dom::ShadowRootInit {
                mode: dom::ShadowRootMode::Open,
                delegates_focus: false,
                slot_assignment: dom::SlotAssignmentMode::Named,
                clonable: false,
                serializable: false,
            },
        );
        let inner = cx.create_element(document, svg_name("svg"));
        cx.append_child(root, inner).unwrap();
        add(cx, inner, "circle", &[("id", "r")]);
        cx.update_use_trees();
        assert_eq!(shape(cx, user), "use->rect#r");
        // Out and in again before the copies are made: its ids are the
        // shadow tree's.
        cx.remove(user);
        cx.append_child(inner, user).unwrap();
        cx.update_use_trees();
        assert_eq!(shape(cx, user), "use->circle#r");
    });
}

#[test]
fn a_cycle_is_looked_for_up_to_the_first_element_outside_svg() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let svg = page(cx);
        let document = cx.document_of(svg);
        // A host with the id of what its shadow tree's `<use>` references:
        // not a cycle (Blink's walk stops at the shadow root).
        let host = cx.create_element(document, QualName::html("div"));
        cx.set_attribute(host, "id", "d");
        cx.append_child(cx.parent(svg).unwrap(), host).unwrap();
        let root = cx.attach_shadow(
            host,
            dom::ShadowRootInit {
                mode: dom::ShadowRootMode::Open,
                delegates_focus: false,
                slot_assignment: dom::SlotAssignmentMode::Named,
                clonable: false,
                serializable: false,
            },
        );
        let inner = cx.create_element(document, svg_name("svg"));
        cx.append_child(root, inner).unwrap();
        add(cx, inner, "rect", &[("id", "d")]);
        let user = add(cx, inner, "use", &[("href", "#d")]);
        cx.update_use_trees();
        assert_eq!(shape(cx, user), "use->rect#d");
    });
}
