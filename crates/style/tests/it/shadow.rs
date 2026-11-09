//! Shadow trees through the flat tree: a slot's assigned nodes inherit
//! from it, its fallback when it has none, a shadow tree's elements from
//! the host; the document's sheets match none of a shadow tree's elements
//! (but a `<use>`'s copy's, which match its `<use>`'s tree's);
//! a change in a shadow tree, of a slot or of an assignment styles again
//! what it should. Each tree's own sheets (its `<style>`s and what it
//! adopts), `:host` and `::slotted()` and their order in the cascade,
//! `display: contents`.

use dom::{QualName, ShadowRootInit, ShadowRootMode, SlotAssignmentMode, names};

use crate::{Page, find, open_init};

fn font_size(page: &mut Page, id: &str) -> Option<f32> {
    let Page {
        dom,
        document,
        token,
        ..
    } = page;
    dom.read(|cx| {
        let element = find(cx, document.get(cx.gc), id).expect("an element with the id");
        cx.computed_style(element, token)
            .map(|style| style.font.size)
    })
}

fn color_red(page: &mut Page, id: &str) -> bool {
    let Page {
        dom,
        document,
        token,
        ..
    } = page;
    dom.read(|cx| {
        let element = find(cx, document.get(cx.gc), id).expect("an element with the id");
        cx.computed_style(element, token)
            .is_some_and(|style| style.color == gfx::Color::rgb(255, 0, 0))
    })
}

/// A host whose light children are `#a` (slot `s`), `#b` and `#c` (slot
/// `none`, which no slot has), and a shadow tree of `<p id=inner>`, a slot
/// `s` and a default slot each with its own font size, a slot `z` with a
/// fallback `<em id=fb>`.
fn hosted() -> Page {
    let mut page = Page::new(
        "<!doctype html><style>p, em { color: red } span { font-size: 7px }</style>\
         <div id=host style='font-size: 10px'><b id=a slot=s>a</b><b id=b>b</b><b id=c slot=none>c</b></div>",
    );
    page.with("host", |cx, host| {
        let document = cx.document_of(host);
        let root = cx.attach_shadow(host, open_init());
        let element = |name: &str, attributes: &[(&str, &str)]| {
            let element = cx.create_element(document, QualName::html(name));
            for (name, value) in attributes {
                cx.set_attribute(element, name, value);
            }
            element
        };
        cx.append_child(root, element("p", &[("id", "inner")]))
            .unwrap();
        cx.append_child(
            root,
            element(
                "slot",
                &[("id", "s"), ("name", "s"), ("style", "font-size: 30px")],
            ),
        )
        .unwrap();
        cx.append_child(
            root,
            element("slot", &[("id", "default"), ("style", "font-size: 40px")]),
        )
        .unwrap();
        let z = element(
            "slot",
            &[("id", "z"), ("name", "z"), ("style", "font-size: 50px")],
        );
        cx.append_child(z, element("em", &[("id", "fb")])).unwrap();
        cx.append_child(root, z).unwrap();
    });
    page.recalc();
    page
}

#[test]
fn assigned_nodes_inherit_from_their_slots() {
    let mut page = hosted();
    assert_eq!(font_size(&mut page, "inner"), Some(10.0), "from the host");
    assert_eq!(font_size(&mut page, "a"), Some(30.0));
    assert_eq!(font_size(&mut page, "b"), Some(40.0));
    assert_eq!(
        font_size(&mut page, "fb"),
        Some(50.0),
        "a fallback, from its slot"
    );
    // The document's sheets: the light tree's, not the shadow tree's.
    assert!(!color_red(&mut page, "inner"));
    assert!(!color_red(&mut page, "fb"));
}

#[test]
fn changes_in_and_around_the_shadow_tree_style_again() {
    let mut page = hosted();
    // In the shadow tree: its marks reach the document through the host.
    page.with("inner", |cx, inner| {
        cx.set_attribute(inner, "style", "font-size: 11px")
    });
    page.recalc();
    assert_eq!(font_size(&mut page, "inner"), Some(11.0));
    // A slot's style: its assigned nodes'.
    page.with("s", |cx, slot| {
        cx.set_attribute(slot, "style", "font-size: 31px")
    });
    page.recalc();
    assert_eq!(font_size(&mut page, "a"), Some(31.0));
    // Assigned elsewhere: from the other slot.
    page.with("a", |cx, a| cx.set_attribute(a, "slot", "z"));
    page.recalc();
    assert_eq!(font_size(&mut page, "a"), Some(50.0));
    // An assigned node's own change, under its slot.
    page.with("a", |cx, a| cx.set_attribute(a, "style", "font-size: 12px"));
    page.recalc();
    assert_eq!(font_size(&mut page, "a"), Some(12.0));
    // A host's child coming: its slot's.
    page.with("host", |cx, host| {
        let document = cx.document_of(host);
        let d = cx.create_element(document, QualName::html("b"));
        cx.set_attribute(d, "id", "d");
        cx.append_child(host, d).unwrap();
    });
    page.recalc();
    assert_eq!(font_size(&mut page, "d"), Some(40.0));
    // A slot renamed: the nodes go to the next of the name.
    page.with("c", |cx, c| cx.set_attribute(c, "slot", "s"));
    page.recalc();
    assert_eq!(font_size(&mut page, "c"), Some(31.0));
    // An element new in the shadow tree.
    page.with("inner", |cx, inner| {
        let document = cx.document_of(inner);
        let late = cx.create_element(document, QualName::html("i"));
        cx.set_attribute(late, "id", "late");
        cx.append_child(inner, late).unwrap();
    });
    page.recalc();
    assert_eq!(font_size(&mut page, "late"), Some(11.0));
}

/// The manual mode: a node assigned before it is the host's child gets its
/// slot's style when it comes (DOM signals no slot change then).
#[test]
fn a_manually_assigned_node_coming_into_the_host_is_its_slots() {
    let mut page =
        Page::new("<!doctype html><div id=host style='font-size: 10px'></div><b id=m>m</b>");
    page.with("host", |cx, host| {
        let document = cx.document_of(host);
        let root = cx.attach_shadow(
            host,
            ShadowRootInit {
                mode: ShadowRootMode::Open,
                delegates_focus: false,
                slot_assignment: SlotAssignmentMode::Manual,
                clonable: false,
                serializable: false,
            },
        );
        let slot = cx.create_element(document, QualName::html("slot"));
        cx.set_attribute(slot, "id", "manual");
        cx.set_attribute(slot, "style", "font-size: 33px");
        cx.append_child(root, slot).unwrap();
        let m = find(cx, document, "m").unwrap();
        cx.assign(slot, &[m]);
    });
    page.recalc();
    page.with("host", |cx, host| {
        let m = find(cx, cx.document_of(host), "m").unwrap();
        cx.append_child(host, m).unwrap();
    });
    page.recalc();
    assert_eq!(font_size(&mut page, "m"), Some(33.0));
}

/// A shadow root attached to a styled host: its children are its shadow
/// tree's from then on.
#[test]
fn a_shadow_root_attached_styles_its_tree() {
    let mut page =
        Page::new("<!doctype html><div id=host style='font-size: 9px'><span id=x>x</span></div>");
    assert_eq!(
        font_size(&mut page, "x"),
        Some(9.0),
        "from the host, before"
    );
    page.with("host", |cx, host| {
        let document = cx.document_of(host);
        let root = cx.attach_shadow(host, open_init());
        let slot = cx.create_element(document, QualName::html("slot"));
        cx.set_attribute(slot, "style", "font-size: 20px");
        cx.append_child(root, slot).unwrap();
    });
    page.recalc();
    assert_eq!(font_size(&mut page, "x"), Some(20.0));
}

/// A `<style>` with `css` added to the document, styled: the ids of the
/// elements styled again, sorted.
fn add_sheet(page: &mut Page, css: &str) -> Vec<String> {
    page.with("host", |cx, host| {
        let document = cx.document_of(host);
        let style = cx.create_element(document, QualName::html("style"));
        let text = cx.create_text(document, css);
        cx.append_child(style, text).unwrap();
        cx.insert(cx.parent(host).unwrap(), style, Some(host));
    });
    page.recalc();
    let Page { dom, engine, .. } = page;
    let mut ids: Vec<String> = dom.read(|cx| {
        engine
            .restyled_for_testing()
            .iter()
            .filter_map(|&id| cx.node_by_id(id))
            .map(|element| {
                cx.attribute_value_by_id(element, names::ID)
                    .map_or_else(String::new, |id| id.borrow().to_str().into_owned())
            })
            .collect()
    });
    ids.sort();
    ids
}

/// The document's changed rules find its own tree's elements: a host's
/// children its slots show, not its shadow tree's, though they have the
/// feature.
#[test]
fn the_documents_changed_rules_find_its_own_tree() {
    let mut page = hosted();
    page.with("inner", |cx, inner| cx.set_attribute(inner, "class", "a"));
    page.with("a", |cx, a| cx.set_attribute(a, "class", "a"));
    page.recalc();
    // The new `<style>` itself (`""`).
    assert_eq!(add_sheet(&mut page, ".a { color: blue }"), ["", "a"]);
}

/// A `<style>` with `css` put before the host (which it styles again by
/// position, with a sibling combinator among the rules).
fn add_rules(page: &mut Page, css: &str) {
    add_sheet(page, css);
}

/// The sibling sets of an assigned node reach its later siblings that
/// another slot shows: one after its own in the shadow tree, and one
/// before (which the walk had passed: marked, and a walk after).
#[test]
fn sibling_sets_reach_siblings_in_another_slot() {
    let mut page = hosted();
    // `#a`'s slot is before `#b`'s. (Not the host, which the new `<style>`
    // moved: no rule matched its siblings by position before.)
    assert_eq!(add_sheet(&mut page, "#a + * { font-size: 3px }"), ["", "b"]);
    assert_eq!(font_size(&mut page, "b"), Some(3.0));
    // `#c` to the first slot, `#b`'s after it.
    page.with("c", |cx, c| cx.set_attribute(c, "slot", "s"));
    page.recalc();
    // (Not the `<style>` before the new one: a `+` sees what follows.)
    assert_eq!(
        add_sheet(&mut page, "#b + * { font-size: 4px }"),
        ["", "c", "host"]
    );
    assert_eq!(font_size(&mut page, "c"), Some(4.0));
    // A class's, and what the sets don't say (every later sibling).
    // (More specific than `#b + *`.)
    add_rules(
        &mut page,
        "#b.x + * { font-size: 5px } :is(#a.y, .z) ~ b { font-size: 6px }",
    );
    page.with("b", |cx, b| cx.set_attribute(b, "class", "x"));
    page.recalc();
    assert_eq!(font_size(&mut page, "c"), Some(5.0));
    page.with("b", |cx, b| cx.set_attribute(b, "class", "q"));
    page.recalc();
    // `#a`'s change alone: `#b` styled again by it.
    page.with("a", |cx, a| cx.set_attribute(a, "class", "y"));
    page.recalc();
    assert_eq!(font_size(&mut page, "b"), Some(6.0));
}

/// An assigned node's sibling sets mark only the siblings they reach and
/// find (as far as a run of `+`), and every later one for what the sets
/// don't say.
#[test]
fn an_assigned_nodes_sets_mark_what_they_reach() {
    let mut page = hosted();
    // `#c` shown too, after `#b`.
    page.with("c", |cx, c| cx.set_attribute(c, "slot", "s"));
    page.recalc();
    add_rules(
        &mut page,
        ".x + * { font-size: 3px } .y ~ i { font-size: 4px } :is(.z, .w) ~ * { font-size: 5px }",
    );
    page.with("a", |cx, a| cx.set_attribute(a, "class", "x"));
    page.recalc();
    assert_eq!(page.restyled(), ["a", "b"], "one on, not `#c`");
    page.with("a", |cx, a| cx.set_attribute(a, "class", "y"));
    page.recalc();
    assert_eq!(page.restyled(), ["a", "b"], "`#b` back; no `<i>`");
    page.with("a", |cx, a| cx.set_attribute(a, "class", "z"));
    page.recalc();
    assert_eq!(page.restyled(), ["a", "b", "c"], "every later one");
}

/// A host's child coming styles what matches by position in every slot:
/// the slots' assigned nodes change, all of them styled again.
#[test]
fn a_hosts_child_coming_styles_by_position_in_every_slot() {
    let mut page = hosted();
    add_rules(&mut page, "b:nth-child(3) { font-size: 3px }");
    page.recalc();
    assert_ne!(font_size(&mut page, "b"), Some(3.0));
    // A new first child: `#b` is third now, flagged by none.
    page.with("host", |cx, host| {
        let document = cx.document_of(host);
        let i = cx.create_element(document, QualName::html("i"));
        let first = cx.first_child(host);
        cx.insert(host, i, first);
    });
    page.recalc();
    assert_eq!(font_size(&mut page, "b"), Some(3.0));
}

fn px(px: f32) -> computed_style::Size {
    computed_style::Size::LengthPercentage(computed_style::LengthPercentage::Px(px))
}

const RED: gfx::Color = gfx::Color::rgb(255, 0, 0);
const BLUE: gfx::Color = gfx::Color::rgb(0, 0, 255);

/// A shadow tree's sheets: its own elements', not the document's; `:host`
/// is its host's, `::slotted()` what its slots are assigned (not the
/// fallback); its plain rules match neither.
mod shadow_sheets {
    use super::*;

    const SHADOW: &str = "<style>p { width: 1px } :host { width: 2px } :host(.big) { height: 9px } \
        ::slotted(b) { width: 3px } div { width: 4px } em { width: 5px }</style>\
        <p id=inner>p</p><slot id=s></slot><slot name=none><em id=fb>fallback</em></slot>";

    fn page() -> Page {
        Page::with_shadows(
            "<!doctype html><div id=host><b id=light>b</b><p id=lp>p</p></div><p id=outside>p</p>",
            &[("host", SHADOW)],
        )
    }

    #[test]
    fn a_shadow_trees_sheets_style_its_host_and_what_its_slots_show() {
        let mut page = page();
        assert_eq!(page.style("inner").width, px(1.0), "its own element");
        assert_eq!(page.style("host").width, px(2.0), ":host, not `div`");
        assert_eq!(page.style("light").width, px(3.0), "::slotted(b)");
        assert_eq!(
            page.style("lp").width,
            computed_style::Size::Auto,
            "a light `p`: not its tree's"
        );
        assert_eq!(page.style("outside").width, computed_style::Size::Auto);
        assert_eq!(
            page.style("fb").width,
            px(5.0),
            "a fallback: its tree's, not slotted"
        );
        assert_ne!(page.style("host").height, px(9.0));
    }

    /// The host's class: `:host(.big)` and a rule below it in the tree
    /// (`:host(.big) p`), by the shadow tree's sets.
    #[test]
    fn the_hosts_change_styles_what_its_shadow_trees_rules_find() {
        let mut page = Page::with_shadows(
            "<!doctype html><div id=host></div>",
            &[(
                "host",
                "<style>:host(.big) { height: 9px } :host(.big) p { color: red } .on span { color: blue }</style>\
                <p id=inner>p</p><div id=d><span id=sp>s</span></div>",
            )],
        );
        page.with("host", |cx, host| cx.set_attribute(host, "class", "big"));
        page.recalc();
        assert_eq!(page.style("host").height, px(9.0));
        assert_eq!(page.style("inner").color, RED);
        // A class only the shadow tree's sheet uses, of one of its elements.
        page.with("d", |cx, d| cx.set_attribute(d, "class", "on"));
        page.recalc();
        assert_eq!(page.style("sp").color, BLUE);
    }

    /// A shadow tree's `<style>` that changes, comes or goes styles its
    /// tree, its host and what its slots show again.
    #[test]
    fn a_shadow_trees_sheet_that_changes_styles_it_again() {
        let mut page = page();
        page.with("inner", |cx, inner| {
            let style = cx.first_child(cx.parent(inner).unwrap()).unwrap();
            cx.set_text_content(
                style,
                "p { width: 7px } :host { width: 8px } ::slotted(b) { width: 6px }",
            );
        });
        page.recalc();
        assert_eq!(page.style("inner").width, px(7.0));
        assert_eq!(page.style("host").width, px(8.0));
        assert_eq!(page.style("light").width, px(6.0));
        page.with("inner", |cx, inner| {
            let style = cx.first_child(cx.parent(inner).unwrap()).unwrap();
            cx.remove(style);
        });
        page.recalc();
        assert_eq!(page.style("inner").width, computed_style::Size::Auto);
        assert_eq!(page.style("host").width, computed_style::Size::Auto);
        assert_eq!(page.style("light").width, computed_style::Size::Auto);
        // One that comes, in a subtree.
        page.with("inner", |cx, inner| {
            let fragment = html::parser::parse_fragment_in(
                cx,
                inner,
                "<i><style>p { width: 5px }</style></i>",
                false,
            );
            cx.append_child(inner, fragment).unwrap();
        });
        page.recalc();
        assert_eq!(page.style("inner").width, px(5.0));
    }

    /// Shadow roots of one `<style>` text share its parsed contents and
    /// its index.
    #[test]
    fn shadow_trees_of_one_text_share_an_index() {
        let mut page = Page::with_shadows(
            "<!doctype html><div id=a></div><div id=b></div>",
            &[
                ("a", "<style>p { width: 1px }</style><p id=pa>a</p>"),
                ("b", "<style>p { width: 1px }</style><p id=pb>b</p>"),
            ],
        );
        assert_eq!(page.style("pa").width, px(1.0));
        assert_eq!(page.style("pb").width, px(1.0));
        assert_eq!(page.engine.shadow_indexes_for_testing(), 1);
    }

    /// A shadow tree's `<link>`, once its sheet came.
    #[test]
    fn a_shadow_trees_link_applies_once_given() {
        let mut page = Page::with_shadows(
            "<!doctype html><div id=host></div>",
            &[(
                "host",
                "<link id=l rel=stylesheet href=a.css><p id=inner>p</p>",
            )],
        );
        {
            let Page { dom, document, .. } = &mut page;
            dom.mutate(|cx| {
                let link = find(cx, document.get(cx.gc), "l").unwrap();
                cx.set_link_sheet(link, crate::sheet("p { width: 4px }"), "a.css", true);
            });
        }
        page.recalc();
        assert_eq!(page.style("inner").width, px(4.0));
    }

    /// To its own tree's `:host` rules the host has no ancestors (`body
    /// :host` matches nothing); an alternate `<link>` doesn't apply.
    #[test]
    fn the_host_is_featureless_to_its_trees_rules() {
        let mut page = Page::with_shadows(
            "<!doctype html><div id=host></div>",
            &[(
                "host",
                "<style>body :host { width: 9px } :host { height: 3px }</style>\
                <link id=alt rel='alternate stylesheet' href=a.css><p id=inner>p</p>",
            )],
        );
        assert_eq!(page.style("host").height, px(3.0));
        assert_eq!(page.style("host").width, computed_style::Size::Auto);
        {
            let Page { dom, document, .. } = &mut page;
            dom.mutate(|cx| {
                let link = find(cx, document.get(cx.gc), "alt").unwrap();
                cx.set_link_sheet(link, crate::sheet("p { width: 4px }"), "a.css", true);
            });
        }
        page.recalc();
        assert_eq!(page.style("inner").width, computed_style::Size::Auto);
    }

    /// A shadow tree's sheet whose media come to match: the names its
    /// rules use are noted from then on.
    #[test]
    fn a_shadow_sheets_names_count_once_its_media_match() {
        let mut page = Page::with_shadows(
            "<!doctype html><div id=host></div>",
            &[(
                "host",
                "<style media='(min-width: 100px)'>.on p { width: 4px }</style><div id=d><p id=inner>p</p></div>",
            )],
        );
        page.engine.set_viewport(gfx::SizeF::new(200.0, 100.0), 1.0);
        page.recalc();
        page.with("d", |cx, d| cx.set_attribute(d, "class", "on"));
        page.recalc();
        assert_eq!(page.style("inner").width, px(4.0));
    }

    /// A shadow tree's `:has()`: a change below styles what it matches.
    #[test]
    fn a_shadow_trees_has_styles_what_it_matches() {
        let mut page = Page::with_shadows(
            "<!doctype html><div id=host></div>",
            &[(
                "host",
                "<style>div:has(.x) { width: 3px }</style><div id=d><p id=inner>p</p></div>",
            )],
        );
        page.with("inner", |cx, inner| cx.set_attribute(inner, "class", "x"));
        page.recalc();
        assert_eq!(page.style("d").width, px(3.0));
    }

    /// A shadow tree's sibling combinator left of a descendant one: an
    /// element coming before styles what is below its later siblings.
    #[test]
    fn a_shadow_trees_siblings_then_descendants() {
        let mut page = Page::with_shadows(
            "<!doctype html><div id=host></div>",
            &[(
                "host",
                "<style>.a + div span { width: 3px }</style><div id=d><span id=sp>s</span></div>",
            )],
        );
        page.with("d", |cx, d| {
            let document = cx.document_of(d);
            let a = cx.create_element(document, QualName::html("i"));
            cx.set_attribute(a, "class", "a");
            cx.insert(cx.parent(d).unwrap(), a, Some(d));
        });
        page.recalc();
        assert_eq!(page.style("sp").width, px(3.0));
    }

    /// A `<style id=id>` of `css` appended to `host`'s shadow root,
    /// styled: the ids of the elements styled again, sorted.
    fn add_to_shadow(page: &mut Page, host: &str, id: &str, css: &str) -> Vec<String> {
        page.with(host, |cx, host| {
            let root = cx.shadow_root(host).expect("a host");
            let document = cx.document_of(host);
            let style = cx.create_element(document, QualName::html("style"));
            cx.set_attribute(style, "id", id);
            let text = cx.create_text(document, css);
            cx.append_child(style, text).unwrap();
            cx.append_child(root, style).unwrap();
        });
        page.recalc();
        page.restyled()
    }

    /// The light tree, the shadow tree with two slots, and an element of
    /// the document's tree with the same class as one of the shadow
    /// tree's.
    fn changing() -> Page {
        Page::with_shadows(
            "<!doctype html><div id=host><i id=light slot=a><b id=deep class=k></b></i><i id=other></i></div>\
             <p id=doc class=k></p>",
            &[(
                "host",
                "<style id=sheet>p { margin: 1px }</style><p id=s1 class=k><span id=s1a></span></p>\
                <slot id=slot-a name=a></slot><slot id=slot-b></slot><div id=s2></div>",
            )],
        )
    }

    /// A shadow tree's rule that comes styles what its features find in
    /// that tree, not the document's elements with them; a `:host` rule,
    /// the host alone (its style the same: nothing below); a `::slotted()`
    /// rule, what its slots are assigned. (Not the element before each new
    /// `<style>`: no rule of the tree matches by position.)
    #[test]
    fn a_shadow_trees_rules_find_its_tree_its_host_and_what_is_slotted() {
        let mut page = changing();
        assert_eq!(
            add_to_shadow(&mut page, "host", "c", ".k { width: 1px }"),
            ["c", "s1"]
        );
        assert_eq!(page.style("s1").width, px(1.0));
        assert_eq!(
            add_to_shadow(&mut page, "host", "h", ":host(.nope) { width: 1px }"),
            ["h", "host"]
        );
        assert_eq!(
            add_to_shadow(&mut page, "host", "sl", "::slotted(i) { height: 2px }"),
            ["light", "other", "sl"]
        );
        assert_eq!(page.style("light").height, px(2.0));
        // A sibling's later siblings in the tree.
        assert_eq!(
            add_to_shadow(&mut page, "host", "sb", "#slot-b + * { height: 3px }"),
            ["s2", "sb"]
        );
    }

    /// What no feature finds in a shadow tree's rules styles that tree
    /// again, its host and what its slots are assigned (Blink's), not the
    /// host's other descendants unless what they inherit changed, nor the
    /// document; so do its sheets kept in another order.
    #[test]
    fn everything_of_a_shadow_tree_stops_at_its_tree() {
        let mut page = changing();
        page.with("s1", |cx, s1| {
            let sheet = cx.first_child(cx.parent(s1).unwrap()).unwrap();
            let text = cx.create_text(cx.document_of(s1), " * { width: 7px }");
            cx.append_child(sheet, text).unwrap();
        });
        page.recalc();
        assert_eq!(
            page.restyled(),
            [
                "host", "light", "other", "s1", "s1a", "s2", "sheet", "slot-a", "slot-b"
            ]
        );
        assert_eq!(page.style("s2").width, px(7.0));
        // Two sheets the other way round.
        add_to_shadow(&mut page, "host", "x2", "p { margin: 2px }");
        page.with("s1", |cx, s1| {
            let root = cx.parent(s1).unwrap();
            let x2 = cx.last_child(root).unwrap();
            let first = cx.first_child(root);
            cx.insert(root, x2, first);
        });
        page.recalc();
        assert_eq!(
            page.restyled(),
            [
                "host", "light", "other", "s1", "s1a", "s2", "sheet", "slot-a", "slot-b", "x2"
            ]
        );
        assert_eq!(
            page.style("s1").margin.top,
            computed_style::LengthPercentageOrAuto::LengthPercentage(
                computed_style::LengthPercentage::Px(1.0)
            )
        );
    }

    /// A `::slotted()` rule of a tree whose slot is assigned a slot of
    /// another tree reaches what is assigned to that one too.
    #[test]
    fn slotted_rules_reach_through_a_slot_assigned_to_a_slot() {
        let mut page = Page::with_shadows(
            "<!doctype html><div id=outer><i id=light></i></div>",
            &[(
                "outer",
                "<style>slot { display: block }</style><div id=inner><slot id=s1></slot></div>",
            )],
        );
        page.with("inner", |cx, inner| {
            let root = cx.attach_shadow(inner, open_init());
            let slot = cx.create_element(cx.document_of(inner), QualName::html("slot"));
            cx.set_attribute(slot, "id", "s2");
            cx.append_child(root, slot).unwrap();
        });
        page.recalc();
        assert_eq!(
            add_to_shadow(&mut page, "inner", "st", "::slotted(i) { height: 3px }"),
            ["light", "s1", "st"]
        );
        assert_eq!(page.style("light").height, px(3.0));
    }

    /// A host's or a slotted element's change styles again what the
    /// shadow tree's rules find of it alone: `:host(.big)` the host,
    /// `:host(.big) p` the `p`s below it too, `::slotted(.on)` the element.
    #[test]
    fn a_hosts_and_a_slotted_elements_change_style_what_finds_them() {
        let mut page = Page::with_shadows(
            "<!doctype html><div id=host><i id=light></i></div>",
            &[(
                "host",
                "<style>:host(.big) { height: 9px } :host(.huge) p { width: 3px } ::slotted(.on) { width: 2px }</style>\
                <p id=inner>p</p><div id=d></div><slot id=s></slot>",
            )],
        );
        let mut changed = |id: &str, class: &str| {
            page.with(id, |cx, element| cx.set_attribute(element, "class", class));
            page.recalc();
            page.restyled()
        };
        assert_eq!(changed("host", "big"), ["host"]);
        assert_eq!(changed("host", "big huge"), ["host", "inner"]);
        assert_eq!(changed("light", "on"), ["light"]);
        assert_eq!(page.style("light").width, px(2.0));
        assert_eq!(page.style("inner").width, px(3.0));
    }

    /// A shadow tree's `<style media>`, and an `@media` in its sheet,
    /// follow the viewport.
    #[test]
    fn a_shadow_trees_media_follow_the_viewport() {
        let mut page = Page::with_shadows(
            "<!doctype html><div id=host></div>",
            &[(
                "host",
                "<style media='(min-width: 100px)'>p { width: 4px }</style>\
                 <style>@media (min-width: 100px) { i { width: 5px } }</style><p id=inner>p</p><i id=it>i</i>",
            )],
        );
        assert_eq!(page.style("inner").width, computed_style::Size::Auto);
        assert_eq!(page.style("it").width, computed_style::Size::Auto);
        page.engine.set_viewport(gfx::SizeF::new(200.0, 100.0), 1.0);
        page.recalc();
        assert_eq!(page.style("inner").width, px(4.0));
        assert_eq!(page.style("it").width, px(5.0));
    }

    /// A shadow tree's viewport units follow the viewport.
    #[test]
    fn a_shadow_trees_viewport_units_follow_the_viewport() {
        let mut page = Page::with_shadows(
            "<!doctype html><div id=light style='width: 100vw'></div><div id=host></div>",
            &[("host", "<style>p { width: 100vw }</style><p id=inner>p</p>")],
        );
        page.engine.set_viewport(gfx::SizeF::new(50.0, 50.0), 1.0);
        page.recalc();
        assert_eq!(page.style("inner").width, px(50.0));
        page.engine.set_viewport(gfx::SizeF::new(100.0, 100.0), 1.0);
        page.recalc();
        assert_eq!(page.style("light").width, px(100.0));
        assert_eq!(page.style("inner").width, px(100.0));
    }
}

/// The style of the root of the copy of the `<use>` whose `id` is `id`
/// (in the document, or in `host`'s shadow tree).
fn copy_style(page: &mut Page, id: &str) -> std::sync::Arc<computed_style::ComputedStyle> {
    let Page {
        dom,
        document,
        token,
        ..
    } = page;
    dom.read(|cx| {
        let user = find(cx, document.get(cx.gc), id).expect("a `<use>` with the id");
        let copy = cx.use_copy(user).expect("a copy");
        cx.computed_style(copy, token).expect("styled").clone()
    })
}

/// A `<use>`'s copy matches the rules of the `<use>`'s tree as itself
/// (`.lime`, `#r`), not as in its original's place (`defs rect`), and
/// inherits from the `<use>`; a copy in a shadow tree, that tree's rules.
#[test]
fn a_use_copy_matches_its_uses_trees_rules() {
    use computed_style::{LengthPercentage, StyleColor, SvgPaint};
    use gfx::Color;
    let mut page = Page::new(
        "<!doctype html><style>.lime { stroke: lime } defs rect { stroke-width: 9px } \
         #r { opacity: 0.5 } rect { fill: red }</style>\
         <svg><defs><rect id=r class=lime></rect></defs><use id=u href=#r style='color: blue'></use></svg>\
         <div id=host></div>\
         <div id=held><style>rect { stroke-width: 3px }</style>\
         <svg><rect id=sr></rect><use id=su href=#sr></use></svg></div>",
    );
    page.with("host", |cx, host| {
        let root = cx.attach_shadow(host, open_init());
        let held = cx.next_sibling(host).unwrap();
        while let Some(child) = cx.first_child(held) {
            cx.append_child(root, child).unwrap();
        }
        cx.update_use_trees();
    });
    page.recalc();
    let copy = copy_style(&mut page, "u");
    let lime = StyleColor::Rgba(Color::rgb(0, 255, 0));
    assert_eq!(copy.svg.stroke, SvgPaint::Color(lime), "its class");
    assert_eq!(copy.opacity, 0.5, "its id");
    assert_eq!(
        copy.svg.stroke_width,
        LengthPercentage::Px(1.0),
        "not in `<defs>`"
    );
    let red = SvgPaint::Color(StyleColor::Rgba(Color::rgb(255, 0, 0)));
    assert_eq!(copy.svg.fill, red, "the document's rule");
    assert_eq!(copy.color, Color::rgb(0, 0, 255), "from the `<use>`");
    let shadowed = copy_style(&mut page, "su");
    assert_eq!(
        shadowed.svg.stroke_width,
        LengthPercentage::Px(3.0),
        "its tree's"
    );
    assert_ne!(shadowed.svg.fill, red, "not the document's");
}

const GREEN: gfx::Color = gfx::Color::rgb(0, 128, 0);

fn size_px(v: f32) -> computed_style::Size {
    computed_style::Size::LengthPercentage(computed_style::LengthPercentage::Px(v))
}

/// A shadow tree's `<style>` matches only its tree's elements, and the
/// document's only the document tree's.
#[test]
fn a_tree_scopes_sheets_match_its_own_elements() {
    let mut page = Page::with_shadows(
        "<style>p { color: red } .x { width: 5px }</style>
         <div id=host><p id=light class=x></p></div>",
        &[(
            "host",
            "<style>p { color: green } .x { height: 7px }</style>\
             <p id=inner class=x></p><slot></slot>",
        )],
    );
    let inner = page.style("inner");
    assert_eq!(inner.color, GREEN);
    assert_eq!(
        inner.width,
        computed_style::Size::Auto,
        "not the document's .x"
    );
    assert_eq!(inner.height, size_px(7.0));
    let light = page.style("light");
    assert_eq!(light.color, RED, "not the shadow tree's p");
    assert_eq!(light.width, size_px(5.0));
    assert_eq!(light.height, computed_style::Size::Auto);
}

/// What a shadow root adopts cascades after its `<style>`s, and applies to
/// its tree only.
#[test]
fn a_shadow_roots_adopted_sheets() {
    let mut page = Page::with_shadows(
        "<div id=host><p id=light></p></div>",
        &[(
            "host",
            "<style>p { color: red }</style><p id=inner></p><slot></slot>",
        )],
    );
    page.with("host", |cx, host| {
        let root = cx.shadow_root(host).unwrap();
        let sheet = cx.create_style_sheet(
            cx.document_of(host),
            crate::sheet("p { color: green }"),
            "",
            false,
        );
        cx.set_adopted_style_sheet(root, 0, sheet);
    });
    page.recalc();
    assert_eq!(page.style("inner").color, GREEN);
    assert_eq!(page.style("light").color, gfx::Color::BLACK);
}

/// A shadow root's children inherit from the host; a host's child from
/// the slot it is assigned to, through slots in slots, or from the host if
/// it is assigned to none. A slot is `display: contents`.
#[test]
fn styles_inherit_along_the_flat_tree() {
    let mut page = Page::with_shadows(
        "<style>#outer-host { color: blue }</style>
         <div id=outer-host><i id=deep slot=x></i><b id=unassigned slot=none></b></div>",
        &[
            (
                "outer-host",
                "<span id=shadow-child></span>\
                 <div id=inner-host><slot id=outer-slot name=x slot=y></slot></div>",
            ),
            (
                "inner-host",
                "<style>slot { color: green }</style><slot id=inner-slot name=y></slot>",
            ),
        ],
    );
    assert_eq!(page.style("shadow-child").color, BLUE);
    assert_eq!(page.style("unassigned").color, BLUE);
    assert_eq!(page.style("inner-slot").color, GREEN);
    // The outer slot is assigned to the inner one, and inherits from it.
    assert_eq!(page.style("outer-slot").color, GREEN);
    assert_eq!(page.style("deep").color, GREEN);
    assert_eq!(
        page.style("inner-slot").display,
        computed_style::Display::Contents
    );
}

/// A host's child assigned to no slot is styled again as it or its host
/// changes, and as it is assigned to a slot or no longer is.
#[test]
fn a_child_assigned_to_no_slot_follows_its_changes() {
    let mut page = Page::with_shadows(
        "<style>#host { color: blue } .red { color: red }</style>
         <div id=host><b id=b slot=none></b></div>",
        &[(
            "host",
            "<style>slot { color: green }</style><slot name=x></slot>",
        )],
    );
    assert_eq!(page.style("b").color, BLUE);
    page.change(|cx, document| {
        cx.set_attribute(find(cx, document, "b").unwrap(), "class", "red");
    });
    assert_eq!(page.style("b").color, gfx::Color::rgb(255, 0, 0));
    page.change(|cx, document| {
        let b = find(cx, document, "b").unwrap();
        cx.remove_attribute(b, "class");
        cx.set_attribute(find(cx, document, "host").unwrap(), "style", "color: black");
    });
    assert_eq!(page.style("b").color, gfx::Color::BLACK);
    page.change(|cx, document| {
        cx.set_attribute(find(cx, document, "b").unwrap(), "slot", "x");
    });
    assert_eq!(page.style("b").color, GREEN);
    page.change(|cx, document| {
        cx.set_attribute(find(cx, document, "b").unwrap(), "slot", "none");
    });
    assert_eq!(page.style("b").color, gfx::Color::BLACK);
}

/// A slot's fallback comes into the flat tree as what was assigned to it
/// leaves: styled again then, though its style was worked out before it
/// went (what it inherits changed meanwhile; WPT's
/// `shadow-fallback-dynamic`).
#[test]
fn a_slots_fallback_is_styled_as_it_shows() {
    let mut page = Page::with_shadows(
        "<style>.red { color: rgb(255, 0, 0) }</style><div id=host></div>",
        &[("host", "<slot><i id=fallback></i></slot>")],
    );
    assert_eq!(page.style("fallback").color, gfx::Color::BLACK);
    page.change(|cx, document| {
        let host = find(cx, document, "host").unwrap();
        let light = cx.create_element(document, QualName::html("b"));
        cx.append_child(host, light).unwrap();
    });
    page.change(|cx, document| {
        cx.set_attribute(find(cx, document, "host").unwrap(), "class", "red");
    });
    page.change(|cx, document| {
        let host = find(cx, document, "host").unwrap();
        let light = cx.first_child(host).unwrap();
        cx.remove(light);
    });
    assert_eq!(page.style("fallback").color, gfx::Color::rgb(255, 0, 0));
}

/// The same `<style>` text in many shadow trees has one index; a text no
/// shadow root has any more is dropped.
#[test]
fn the_same_text_is_parsed_once() {
    let mut page = Page::with_shadows(
        "<div id=a></div><div id=b></div><div id=c></div>",
        &[
            ("a", "<style>p { color: green }</style><p id=pa></p>"),
            ("b", "<style>p { color: green }</style><p id=pb></p>"),
            ("c", "<style>p { color: red }</style><p id=pc></p>"),
        ],
    );
    assert_eq!(page.style("pa").color, GREEN);
    assert_eq!(page.style("pb").color, GREEN);
    assert_eq!(page.engine.shadow_scopes_for_testing(), 3);
    assert_eq!(page.engine.shadow_indexes_for_testing(), 2);
    page.with("c", |cx, c| {
        let root = cx.shadow_root(c).unwrap();
        let style = cx.first_child(root).unwrap();
        let text = cx.first_child(style).unwrap();
        cx.append_data(text, " ");
    });
    page.recalc();
    assert_eq!(page.engine.shadow_scopes_for_testing(), 3);
    assert_eq!(page.engine.shadow_indexes_for_testing(), 2);
}

/// A host's child goes where its slot's box would: a flex container's
/// child is a flex item.
#[test]
fn a_slotted_child_of_a_flex_host_is_a_flex_item() {
    let mut page = Page::with_shadows(
        "<div id=host style='display: flex'><span id=item></span></div>",
        &[("host", "<div style='display: contents'><slot></slot></div>")],
    );
    assert_eq!(page.style("item").display, computed_style::Display::Block);
}

/// `display: contents` is its computed value (the root element's is
/// `block`); its children are blockified in the flex container it is in,
/// as are its pseudo-elements, and take `z-index` as flex items.
#[test]
fn display_contents() {
    use computed_style::Display;
    let mut page = Page::new(
        "<!DOCTYPE html><style>
           html, #c { display: contents }
           #c::before { content: 'x' }
           #item { z-index: 3 }
         </style>
         <div style='display: flex'><span id=c><span id=item></span></span></div>
         <div><span id=inline-contents style='display: contents'><span id=inline></span></span></div>",
    );
    let Page {
        dom,
        document,
        token,
        ..
    } = &page;
    let root = dom.read(|cx| {
        let root = cx.first_child(document.get(cx.gc));
        let root = std::iter::successors(root, |node| cx.next_sibling(*node))
            .find(|node| cx.kind(*node) == dom::NodeKind::Element)
            .unwrap();
        cx.computed_style(root, token).unwrap().display
    });
    assert_eq!(root, Display::Block);
    assert_eq!(page.style("c").display, Display::Contents);
    let item = page.style("item");
    assert_eq!(item.display, Display::Block);
    assert_eq!(item.effective_z_index, Some(3));
    assert_eq!(page.style("inline").display, Display::Inline);
    assert_eq!(
        page.pseudo("c").unwrap().before.unwrap().display,
        Display::Block
    );
}

/// `:host` and `:host()` match the host with its shadow tree's sheets, and
/// nothing else: no element in the shadow tree, none in the document; in a
/// complex selector, `:host` is where the shadow tree's elements are.
#[test]
fn host_rules_match_the_host() {
    let mut page = Page::with_shadows(
        "<div id=host class=wide><p id=light></p></div><p id=doc></p>",
        &[(
            "host",
            "<style>
               :host { color: green }
               :host(.wide) { width: 5px }
               :host(.narrow) { width: 1px }
               :host p { height: 7px }
               :host(.wide) > div > p { margin-top: 3px }
               :host, p { font-size: 20px }
             </style>
             <div><p id=inner></p></div><slot></slot>",
        )],
    );
    let host = page.style("host");
    assert_eq!(host.color, GREEN);
    assert_eq!(host.width, size_px(5.0));
    assert_eq!(host.font.size, 20.0);
    let inner = page.style("inner");
    assert_eq!(inner.height, size_px(7.0));
    assert_eq!(
        inner.margin.top,
        computed_style::LengthPercentageOrAuto::LengthPercentage(
            computed_style::LengthPercentage::Px(3.0)
        )
    );
    assert_eq!(inner.font.size, 20.0);
    // The light child inherits from the default slot, and from the host.
    let light = page.style("light");
    assert_eq!(
        light.height,
        computed_style::Size::Auto,
        "not the shadow tree's `p`"
    );
    assert_eq!(light.color, GREEN, "inherited");
    assert_eq!(page.style("doc").color, gfx::Color::BLACK);
}

/// `::slotted()` matches the elements assigned to the tree's slots (through
/// slots in slots), not their descendants, not a fallback, not one
/// assigned to no slot.
#[test]
fn slotted_rules_match_what_is_assigned() {
    let mut page = Page::with_shadows(
        "<div id=outer><span id=deep slot=x><b id=inside></b></span>\
           <i id=unassigned slot=none></i></div>",
        &[
            (
                "outer",
                "<style>::slotted(span) { color: green } ::slotted(*) { width: 3px }</style>\
                 <div id=inner-host><slot id=outer-slot name=x slot=y></slot></div>\
                 <slot name=fb><span id=fallback></span></slot>",
            ),
            (
                "inner-host",
                "<style>::slotted(*) { height: 9px } ::slotted(slot) { width: 99px }</style>\
                 <slot name=y></slot>",
            ),
        ],
    );
    let deep = page.style("deep");
    assert_eq!(deep.color, GREEN);
    assert_eq!(deep.width, size_px(3.0));
    // The inner tree's, through the outer slot, which is no slottable of
    // its own.
    assert_eq!(deep.height, size_px(9.0));
    assert_eq!(page.style("outer-slot").width, computed_style::Size::Auto);
    assert_eq!(page.style("inside").width, computed_style::Size::Auto);
    assert_eq!(page.style("fallback").width, computed_style::Size::Auto);
    assert_eq!(page.style("unassigned").width, computed_style::Size::Auto);
}

/// The ancestors the bloom filter has for an element of a shadow tree are
/// its tree's, up to the shadow root (the host's don't count for its own
/// tree's rules); a `::slotted()` rule, whose ancestors are the slot's,
/// not the assigned element's, isn't passed over by it; nor is the light
/// tree's after the shadow tree's, by what that pushed.
#[test]
fn the_bloom_filter_follows_each_trees_ancestors() {
    let mut page = Page::with_shadows(
        "<style>.outer p { width: 7px } .outer .light { height: 8px }</style>\
         <div class=outer><div id=host><p id=light class=light></p></div></div>",
        &[(
            "host",
            "<style>.wrap p { width: 3px } .outer p { height: 99px }\
               .in ::slotted(p) { color: green }</style>\
             <div class=wrap><p id=inner></p></div>\
             <div class=in><slot></slot></div>",
        )],
    );
    assert_eq!(page.style("inner").width, size_px(3.0));
    assert_eq!(page.style("inner").height, computed_style::Size::Auto);
    assert_eq!(page.style("light").color, GREEN);
    assert_eq!(page.style("light").height, size_px(8.0));
    assert_eq!(page.style("light").width, size_px(7.0));
}

/// Declarations of other trees: for normal ones the outer tree's win, for
/// `!important` ones the inner tree's, whatever their specificity; a
/// `style` attribute is its element's tree's.
#[test]
fn the_outer_trees_normal_and_the_inner_trees_important_win() {
    let mut page = Page::with_shadows(
        "<style>
           #host { color: red; width: 1px !important }
           #slotted { color: blue; width: 2px !important }
         </style>
         <div id=host style='height: 4px'><span id=slotted></span></div>",
        &[(
            "host",
            "<style>
               :host(#host.x), :host(div#host) { color: green; width: 8px !important; height: 8px }
               ::slotted(#slotted) { color: green; width: 9px !important }
             </style><slot></slot>",
        )],
    );
    let host = page.style("host");
    assert_eq!(host.color, RED, "the document's normal");
    assert_eq!(host.width, size_px(8.0), "the shadow tree's important");
    assert_eq!(host.height, size_px(4.0), "the style attribute");
    let slotted = page.style("slotted");
    assert_eq!(slotted.color, BLUE);
    assert_eq!(slotted.width, size_px(9.0));
}

/// `:host::before` gives the host a `::before`.
#[test]
fn a_hosts_pseudo_elements() {
    let mut page = Page::with_shadows(
        "<div id=host></div>",
        &[(
            "host",
            "<style>:host::before { content: 'x'; color: green }</style>",
        )],
    );
    page.recalc();
    assert_eq!(page.pseudo("host").unwrap().before.unwrap().color, GREEN);
}

/// `:host` and `::slotted()` match nothing in `querySelector`.
#[test]
fn query_selector_has_no_host() {
    let mut page = Page::with_shadows(
        "<div id=host><span></span></div>",
        &[("host", "<slot></slot>")],
    );
    for selector in [":host", ":host(div)", "::slotted(span)"] {
        let query = style::SelectorQuery::parse(selector).expect("it parses");
        assert!(
            !page.with("host", |cx, host| query.matches(cx, host)),
            "{selector}"
        );
    }
}

/// The host has no ancestors or siblings to its shadow tree's `:host`
/// rules; `::slotted()` takes `::before`.
#[test]
fn the_host_is_featureless_and_slotted_takes_before() {
    let mut page = Page::with_shadows(
        "<div class=container><div id=host><span id=slotted></span></div></div>",
        &[(
            "host",
            "<style>.container :host { color: red } :host { width: 2px }
               ::slotted(span)::before { content: 'x'; color: green }</style><slot></slot>",
        )],
    );
    let host = page.style("host");
    assert_eq!(host.color, gfx::Color::BLACK);
    assert_eq!(host.width, size_px(2.0));
    assert_eq!(page.pseudo("slotted").unwrap().before.unwrap().color, GREEN);
}

/// An element that is a host and is assigned to a slot: its own tree is
/// first in the shadow-including tree order, then the slots' trees, then
/// its shadow tree, so `::slotted()` beats `:host` for normal declarations
/// and `:host` wins for `!important` ones, though both trees are one
/// deep.
#[test]
fn slotted_comes_before_host() {
    let mut page = Page::with_shadows(
        "<div id=outer><div id=both></div></div>",
        &[
            (
                "outer",
                "<style>::slotted(*) { color: green; width: 1px !important }</style><slot></slot>",
            ),
            (
                "both",
                "<style>:host { color: red; width: 2px !important }</style>",
            ),
        ],
    );
    let both = page.style("both");
    assert_eq!(both.color, GREEN);
    assert_eq!(both.width, size_px(2.0));
}
