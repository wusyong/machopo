//! What a change styles again, and only that: an element alone when no
//! selector asks its changed name of an ancestor, its subtree when one
//! does, its children when what they inherit changed, its siblings when a
//! selector matches by position (`+`, `~`, `:first-child`), its parent's
//! `:empty`, everything with `:has()` or a new root font size; and that
//! what comes out is what styling everything again with a new engine gives
//! (`Page::change`).

use std::sync::Arc;

use css::StyleSheetContents;
use dom::{Cx, NodeRef, QualName};
use gfx::Color;

use crate::{Page, focus, restyled};

const RED: Color = Color::rgb(255, 0, 0);
const BLUE: Color = Color::rgb(0, 0, 255);

fn px(px: f32) -> computed_style::Size {
    computed_style::Size::LengthPercentage(computed_style::LengthPercentage::Px(px))
}

#[test]
fn a_name_no_selector_asks_of_an_ancestor_styles_the_element_alone() {
    let mut page = Page::new(
        "<!doctype html><style>.a { width: 10px }</style>\
         <div id=d><span id=s>s</span></div>",
    );
    let child = page.style("s");
    page.with("d", |cx, d| cx.set_attribute(d, "class", "a"));
    page.recalc();
    assert_eq!(page.style("d").width, px(10.0));
    assert!(
        Arc::ptr_eq(&child, &page.style("s")),
        "not inherited, not asked of"
    );
}

#[test]
fn a_name_asked_of_an_ancestor_styles_the_subtree() {
    let mut page = Page::new(
        "<!doctype html><style>.a span { color: red }</style>\
         <div id=d><p><span id=s>s</span></p></div>",
    );
    page.with("d", |cx, d| cx.set_attribute(d, "class", "a"));
    page.recalc();
    assert_eq!(page.style("s").color, RED);
    page.with("d", |cx, d| cx.set_attribute(d, "title", "t"));
    page.recalc();
    assert_eq!(page.style("s").color, RED, "kept");
}

#[test]
fn what_children_inherit_styles_them_again() {
    let mut page = Page::new(
        "<!doctype html><style>.a { color: blue }</style>\
         <div id=d><p><span id=s>s</span></p></div>",
    );
    page.with("d", |cx, d| cx.set_attribute(d, "class", "a"));
    page.recalc();
    assert_eq!(page.style("s").color, BLUE, "two levels down");
}

#[test]
fn the_next_sibling_by_position() {
    // No `~` here: it would style every later sibling.
    let mut page = Page::new(
        "<!doctype html><style>.a + p { color: red }</style>\
         <div><p id=first>1</p><p id=second>2</p></div>",
    );
    page.with("first", |cx, p| cx.set_attribute(p, "class", "a"));
    page.recalc();
    assert_eq!(page.style("second").color, RED, "+");
}

#[test]
fn every_later_sibling_by_position() {
    let mut page = Page::new(
        "<!doctype html><style>.b ~ i { color: blue }</style>\
         <div><u id=u>u</u><i id=i1>1</i><i id=i2>2</i></div>",
    );
    page.with("u", |cx, u| cx.set_attribute(u, "class", "b"));
    page.recalc();
    assert_eq!(page.style("i1").color, BLUE, "~");
    assert_eq!(page.style("i2").color, BLUE, "~, not only the next");
}

#[test]
fn children_that_came_restyle_those_matched_by_position() {
    let mut page = Page::new(
        "<!doctype html><style>p:first-child { width: 5px } p:nth-child(2) { width: 7px }</style>\
         <div id=d><p id=first>1</p><p id=second>2</p></div>",
    );
    assert_eq!(page.style("first").width, px(5.0));
    assert_eq!(page.style("second").width, px(7.0));
    // A new first child: the old one isn't any more.
    page.with("d", |cx, d| {
        let document = cx.document_of(d);
        let p = cx.create_element(document, dom::QualName::html("p"));
        let first = cx.first_child(d);
        cx.insert(d, p, first);
    });
    page.recalc();
    assert_eq!(page.style("first").width, px(7.0), "the next, flagged");
    // Two siblings on, flagged by none: its parent's selector flags say.
    assert_ne!(page.style("second").width, px(7.0));
}

#[test]
fn a_parents_empty_changes_with_its_children() {
    let mut page =
        Page::new("<!doctype html><style>div:empty { color: red }</style><div id=d></div>");
    assert_eq!(page.style("d").color, RED);
    page.with("d", |cx, d| {
        let text = cx.create_text(cx.document_of(d), "t");
        cx.append_child(d, text).unwrap();
    });
    page.recalc();
    assert_ne!(page.style("d").color, RED);
}

#[test]
fn has_styles_everything() {
    let mut page = Page::new(
        "<!doctype html><style>div:has(.x) { color: red }</style>\
         <div id=d><p><span id=s>s</span></p></div>",
    );
    page.with("s", |cx, s| cx.set_attribute(s, "class", "x"));
    page.recalc();
    assert_eq!(page.style("d").color, RED);
}

#[test]
fn a_new_root_font_size_styles_everything() {
    let mut page = Page::new(
        // What `<div>` gives `<p>` to inherit doesn't change with the root.
        "<!doctype html><style>p { width: 1rem }</style>\
         <div style='font-size: 10px'><p id=p>p</p></div>",
    );
    page.with("p", |cx, p| {
        let root = cx
            .parent(cx.parent(cx.parent(p).unwrap()).unwrap())
            .unwrap();
        cx.set_attribute(root, "style", "font-size: 20px");
    });
    page.recalc();
    assert_eq!(page.style("p").width, px(20.0));
}

/// `:hover` and `:active` follow the document's chains: an element entering
/// one is styled again, its subtree only when a selector asks the state of
/// an ancestor, its next sibling by position; leaving it styles it back.
#[test]
fn hover_and_active_style_the_chain() {
    let mut page = Page::new(
        "<!doctype html><style>#d:hover { width: 10px } p:active { width: 20px } \
         .h:hover span { color: red } .h:hover + i { color: blue }</style>\
         <div id=d><p id=p><span id=s>s</span></p></div><div id=h class=h><span id=t>t</span></div><i id=i>i</i>",
    );
    let child = page.style("s");
    page.with("s", |cx, s| {
        let document = cx.document_of(s);
        cx.set_hover_element(document, Some(s));
        cx.set_active_element(document, Some(cx.parent(s).unwrap()));
    });
    page.recalc();
    assert_eq!(
        page.style("d").width,
        px(10.0),
        "an ancestor of the hover element"
    );
    assert_eq!(page.style("p").width, px(20.0), ":active");
    assert!(
        Arc::ptr_eq(&child, &page.style("s")),
        "no selector asks `#d:hover` of an ancestor"
    );
    assert_ne!(page.style("t").color, RED);
    // The div, not the span in it: the span isn't in the chain.
    page.with("h", |cx, h| {
        cx.set_hover_element(cx.document_of(h), Some(h))
    });
    page.recalc();
    assert_eq!(
        page.style("t").color,
        RED,
        "asked of an ancestor: its subtree"
    );
    assert_eq!(page.style("i").color, BLUE, "the next sibling");
    assert_eq!(page.style("d").width, computed_style::Size::Auto, "left");
    page.with("t", |cx, t| {
        let document = cx.document_of(t);
        cx.set_hover_element(document, None);
        cx.set_active_element(document, None);
    });
    page.recalc();
    assert_ne!(page.style("t").color, RED);
    assert_ne!(page.style("i").color, BLUE);
    assert_eq!(page.style("p").width, computed_style::Size::Auto);
}

/// `:focus`, `:focus-within` and `:focus-visible` follow the focused
/// element while the page has the system's focus; `:focus-within` asked
/// of an ancestor styles below it.
#[test]
fn focus_styles_its_chain() {
    let mut page = Page::new(
        "<!doctype html><style>#e:focus { width: 10px } #e:focus-visible { height: 5px } \
         #d:focus-within span { color: red }</style>\
         <div id=d><p id=e tabindex=0>e</p><span id=s>s</span></div>",
    );
    page.with("e", |cx, e| {
        let document = cx.document_of(e);
        let window = cx
            .window_of(document)
            .unwrap_or_else(|| cx.create_window(document));
        cx.set_page_focused(window, true);
        cx.set_focused_element(document, Some(e), true);
    });
    page.recalc();
    assert_eq!(page.style("e").width, px(10.0));
    assert_eq!(page.style("e").height, px(5.0));
    assert_eq!(
        page.style("s").color,
        RED,
        ":focus-within asked of an ancestor"
    );
    // No ring.
    page.with("e", |cx, e| {
        let document = cx.document_of(e);
        cx.set_focused_element(document, None, true);
        cx.set_focused_element(document, Some(e), false);
    });
    page.recalc();
    assert_eq!(page.style("e").width, px(10.0));
    assert_eq!(
        page.style("e").height,
        computed_style::Size::Auto,
        "no :focus-visible"
    );
    // The page losing the system's focus: nothing matches.
    page.with("e", |cx, e| {
        let document = cx.document_of(e);
        cx.set_page_focused(cx.window_of(document).expect("a window"), false);
    });
    page.recalc();
    assert_eq!(page.style("e").width, computed_style::Size::Auto);
    assert_ne!(page.style("s").color, RED);
}

/// `:defined`: an undefined custom element's name doesn't match until it
/// is custom (an upgrade), a failed one never, a plain element always;
/// the state's change styles it again, asked of an ancestor its subtree.
#[test]
fn defined_follows_the_custom_element_state() {
    use dom::CustomElementState as State;
    let mut page = Page::new(
        "<!doctype html><style>:defined { width: 10px } x-a:not(:defined) { width: 5px } \
         x-a:defined span { color: red }</style>\
         <div id=d>d</div><x-a id=x><span id=s>s</span></x-a><x-a id=f>f</x-a>",
    );
    assert_eq!(page.style("d").width, px(10.0));
    assert_eq!(page.style("x").width, px(5.0), "undefined");
    assert_ne!(page.style("s").color, RED);
    page.with("x", |cx, x| cx.set_custom_element_state(x, State::Failed));
    page.recalc();
    assert_eq!(page.style("x").width, px(5.0), "failed: still not defined");
    page.with("x", |cx, x| cx.set_custom_element_state(x, State::Custom));
    page.with("f", |cx, f| {
        cx.set_custom_element_state(f, State::Precustomized)
    });
    page.recalc();
    assert_eq!(page.style("x").width, px(10.0), "custom");
    assert_eq!(page.style("s").color, RED, "asked of an ancestor");
    assert_eq!(page.style("f").width, px(5.0), "precustomized: not defined");
}

/// `:defined` asked only of an ancestor (no rule's own element): its change
/// is still one style reads.
#[test]
fn defined_asked_only_of_an_ancestor() {
    use dom::CustomElementState as State;
    let mut page = Page::new(
        "<!doctype html><style>x-a:defined span { color: red }</style>\
         <x-a id=x><span id=s>s</span></x-a>",
    );
    assert_ne!(page.style("s").color, RED);
    page.with("x", |cx, x| cx.set_custom_element_state(x, State::Custom));
    page.recalc();
    assert_eq!(page.style("s").color, RED);
}

/// A blocking `wheel` (or `mousewheel`) listener makes its element and
/// what is below it inside one (`inside_blocking_wheel_event_handler`,
/// inherited), styled again as it comes and goes; a passive one doesn't;
/// the window's and the document's count for its root element.
#[test]
fn a_blocking_wheel_listener_styles_its_subtree() {
    use dom::names::event;
    use dom::{ListenerFlags, TargetRef};
    let mut page = Page::new("<!doctype html><div id=a><p id=b>b</p></div><div id=c></div>");
    let inside = |page: &mut Page| {
        ["a", "b", "c"].map(|id| page.style(id).inside_blocking_wheel_event_handler)
    };
    assert_eq!(inside(&mut page), [false; 3]);
    let listener = page.with("a", |cx, a| {
        elise::Persistent::new(
            cx.gc,
            cx.add_listener(TargetRef::Node(a), event::WHEEL, ListenerFlags::default()),
        )
    });
    page.with("c", |cx, c| {
        let passive = ListenerFlags {
            passive: true,
            ..ListenerFlags::default()
        };
        cx.add_listener(TargetRef::Node(c), event::MOUSEWHEEL, passive);
    });
    page.recalc();
    assert_eq!(inside(&mut page), [true, true, false]);
    page.with("a", |cx, a| {
        cx.remove_listener(TargetRef::Node(a), listener.get(cx.gc))
    });
    page.recalc();
    assert_eq!(inside(&mut page), [false; 3]);
    // The window's, as the document's, is the root's.
    let document = page.document.clone();
    let on_window = page.dom.mutate(|cx| {
        let window = cx.create_window(document.get(cx.gc));
        let listener = cx.add_listener(
            TargetRef::Window(window),
            event::WHEEL,
            ListenerFlags::default(),
        );
        (
            elise::Persistent::new(cx.gc, window),
            elise::Persistent::new(cx.gc, listener),
        )
    });
    page.recalc();
    assert_eq!(inside(&mut page), [true; 3]);
    page.dom.mutate(|cx| {
        cx.remove_listener(
            TargetRef::Window(on_window.0.get(cx.gc)),
            on_window.1.get(cx.gc),
        )
    });
    page.recalc();
    assert_eq!(inside(&mut page), [false; 3]);
    page.dom.mutate(|cx| {
        cx.add_listener(
            TargetRef::Node(document.get(cx.gc)),
            event::MOUSEWHEEL,
            ListenerFlags::default(),
        );
    });
    page.recalc();
    assert_eq!(inside(&mut page), [true; 3]);
    drop(on_window);
}

fn set_attribute(page: &mut Page, id: &str, name: &str, value: &str) -> Vec<String> {
    restyled(page, |cx, document| {
        let element = crate::by_id(cx, document, id).expect("an element with the id");
        cx.set_attribute(element, name, value);
    })
}

fn set_class(page: &mut Page, id: &str, class: &str) -> Vec<String> {
    set_attribute(page, id, "class", class)
}

/// A descendant invalidation set: what a change of an ancestor's feature
/// styles again below it, the elements with the subject's feature (Blink's
/// `RuleFeatureSet`).
mod descendant_sets {
    use super::*;

    const TREE: &str = "<div id=a><p id=b><span id=c></span><i id=d></i></p><em id=e></em></div>";

    fn tree(css: &str) -> Page {
        Page::new(&format!("<!DOCTYPE html><style>{css}</style>{TREE}"))
    }

    /// A subject with no feature to find it by: the whole subtree.
    #[test]
    fn a_subject_without_features_styles_the_subtree() {
        let mut page = tree(".x > * { width: 1px }");
        assert_eq!(set_class(&mut page, "a", "x"), ["a", "b", "c", "d", "e"]);
    }

    /// The subject's feature: only the elements with it.
    #[test]
    fn a_subjects_feature_finds_what_to_style() {
        let mut page = tree(".x i { width: 1px }");
        assert_eq!(set_class(&mut page, "a", "x"), ["a", "d"]);
        assert_eq!(page.style("d").width, px(1.0));
        // A name no rule uses: the DOM doesn't note it.
        assert!(set_class(&mut page, "a", "x z").is_empty());
    }

    /// Each of an `:is()`'s selectors' subjects.
    #[test]
    fn a_subject_list_styles_each_of_its_subjects() {
        let mut page = tree(".x :is(span, em) { width: 1px }");
        assert_eq!(set_class(&mut page, "a", "x"), ["a", "c", "e"]);
        assert_eq!(page.style("e").width, px(1.0));
    }

    /// An id, an attribute, and an id or class the subject has.
    #[test]
    fn ids_attributes_and_classes_find_their_subjects() {
        let mut page = Page::new(
            "<!DOCTYPE html><style>#on .k { width: 1px } [data-x] #d { height: 1px }</style>\
             <div id=a><p id=b class=k></p><i id=d></i></div>",
        );
        assert_eq!(set_attribute(&mut page, "a", "data-x", ""), ["a", "d"]);
        assert_eq!(set_attribute(&mut page, "a", "id", "on"), ["b", "on"]);
        assert_eq!(page.style("b").width, px(1.0));
    }

    /// A link's `href` a rule uses of an ancestor (`a:any-link span`).
    #[test]
    fn a_links_href_styles_below() {
        let mut page = Page::new(
            // What the UA's link styles change, the same each way: what
            // the children inherit doesn't change.
            "<!DOCTYPE html><style>a { color: black; text-decoration: none; cursor: auto } \
             a:any-link span { width: 1px }</style><a id=a><span id=s></span><i id=i></i></a>",
        );
        assert_eq!(set_attribute(&mut page, "a", "href", "x"), ["a", "s"]);
        assert_eq!(page.style("s").width, px(1.0));
    }

    /// The walk goes down for a set, not for the debug build's check alone.
    #[test]
    fn the_walk_goes_down_for_a_set() {
        let mut page = tree(".x i { width: 1px }");
        page.engine.set_verify_for_testing(false);
        assert_eq!(set_class(&mut page, "a", "x"), ["a", "d"]);
        assert_eq!(page.style("d").width, px(1.0));
    }

    /// A state rules use of an ancestor (`:hover`): what below it they
    /// match.
    #[test]
    fn a_state_styles_what_rules_below_match() {
        let mut page = tree("p:hover i { width: 1px }");
        let restyled = restyled(&mut page, |cx, document| {
            let b = crate::by_id(cx, document, "b").unwrap();
            cx.set_hover_element(document, Some(b));
        });
        assert!(restyled.contains(&"d".to_owned()), "{restyled:?}");
        assert!(!restyled.contains(&"c".to_owned()), "{restyled:?}");
        assert_eq!(page.style("d").width, px(1.0));
    }

    /// `lang` and `dir`: what matches below depends on them whatever the
    /// rules.
    #[test]
    fn lang_styles_the_subtree() {
        let mut page = tree(":lang(fr) { width: 1px }");
        assert_eq!(
            set_attribute(&mut page, "a", "lang", "fr"),
            ["a", "b", "c", "d", "e"]
        );
        assert_eq!(page.style("c").width, px(1.0));
    }

    /// A shadow tree's rules that use their host's class (`:host(.x) .b`):
    /// what they match in its shadow tree (each of its elements for a
    /// subject with no feature), not its light DOM children for themselves.
    #[test]
    fn a_hosts_class_styles_what_its_shadow_trees_rules_match() {
        let mut page = Page::with_shadows(
            "<!DOCTYPE html><div id=host><i id=light></i></div>",
            &[(
                "host",
                "<style>:host(.x) .b { width: 1px } :host(.y) * { height: 2px }</style>\
                 <p id=sa class=b></p><p id=sb></p><slot></slot>",
            )],
        );
        assert_eq!(change_class(&mut page, "host", "x"), ["host", "sa"]);
        assert_eq!(page.styles()["sa"].width, px(1.0));
        // A subject with no feature: each element of its shadow tree, and
        // what is assigned to its slot as it inherits from it (a slot is
        // `display: contents`).
        assert_eq!(
            change_class(&mut page, "host", "y"),
            ["host", "light", "sa", "sb"]
        );
    }

    /// A slot's class a `::slotted()` rule uses: what is assigned to it (as
    /// the slot's subtree, which is not the sets').
    #[test]
    fn a_slots_class_styles_what_is_assigned_to_it() {
        let mut page = Page::with_shadows(
            "<!DOCTYPE html><div id=host><i id=light></i></div>",
            &[(
                "host",
                "<style>slot.x::slotted(i) { width: 1px }</style><p id=sp></p><slot id=slot></slot>",
            )],
        );
        let restyled = change_class(&mut page, "slot", "x");
        assert!(restyled.contains(&"light".to_owned()), "{restyled:?}");
        assert!(!restyled.contains(&"sp".to_owned()), "{restyled:?}");
        assert_eq!(page.styles()["light"].width, px(1.0));
    }
}

/// Sibling invalidation sets: what a change of a feature styles again of
/// the later siblings, as far as the rules reach.
mod sibling_sets {
    use super::*;

    const LIST: &str = "<div id=list><p id=a><span id=a1></span></p><p id=b><span id=b1></span></p>\
        <p id=c><span id=c1></span></p><div id=d><span id=d1></span></div></div>";

    fn list(css: &str) -> Page {
        Page::new(&format!("<!DOCTYPE html><style>{css}</style>{LIST}"))
    }

    /// As far as its run of `+`, each that is what the set looks for.
    #[test]
    fn adjacent_styles_as_many_as_its_run() {
        let mut page = list(".x + p { width: 1px }");
        assert_eq!(set_class(&mut page, "a", "x"), ["a", "b"]);
        drop(page);
        let mut page = list(".x + p + p { width: 1px }");
        assert_eq!(set_class(&mut page, "a", "x"), ["a", "b", "c"]);
        assert_eq!(page.style("c").width, px(1.0));
    }

    /// The changed element's subtree stays: no rule uses its class of
    /// elements below.
    #[test]
    fn later_siblings_style_those_that_match_not_their_children() {
        let mut page = list(".x ~ p { width: 1px }");
        assert_eq!(set_class(&mut page, "a", "x"), ["a", "b", "c"]);
        drop(page);
        let mut page = list(".x ~ * { width: 1px }");
        assert_eq!(set_class(&mut page, "a", "x"), ["a", "b", "c", "d"]);
    }

    #[test]
    fn a_subject_with_nothing_to_find_styles_whole_subtrees() {
        let mut page = list(".x ~ div * { width: 1px }");
        assert_eq!(set_class(&mut page, "a", "x"), ["a", "d1"]);
    }

    #[test]
    fn an_attribute_and_an_id_have_sets_too() {
        let mut page = list("[data-on] ~ div { width: 1px }");
        assert_eq!(set_attribute(&mut page, "a", "data-on", ""), ["a", "d"]);
        drop(page);
        let mut page = list("#on + p span { width: 1px }");
        assert_eq!(set_attribute(&mut page, "a", "id", "on"), ["b1", "on"]);
    }

    /// What the sets don't say styles every later sibling (the changed one
    /// alone), or all of them for a position counted from the end.
    #[test]
    fn what_the_sets_dont_say_styles_the_siblings_after() {
        let mut page = list(":is(.x, .y) ~ p { width: 1px }");
        let restyled = set_class(&mut page, "b", "x");
        for id in ["b", "c", "d"] {
            assert!(restyled.contains(&id.to_owned()), "{restyled:?}");
        }
        assert!(!restyled.contains(&"a".to_owned()), "{restyled:?}");
        drop(page);
        let mut page = list("p:nth-child(2 of .x) { width: 1px }");
        let restyled = set_class(&mut page, "b", "x");
        assert!(restyled.contains(&"c".to_owned()), "{restyled:?}");
    }

    /// What the sets don't say styles the siblings as children coming or
    /// going would: those after the change (the changed one alone), and
    /// the earlier ones too for a position counted from the end (marked
    /// for the walk after the first); not their children,
    /// which a sibling's new style takes down if it changes what they
    /// inherit (and a descendant combinator right of the sibling one is
    /// the sets' `sibling_then_descendant`).
    #[test]
    fn what_the_sets_dont_say_styles_the_siblings_after_or_all() {
        let mut page = list(":is(.x, .y) ~ p { width: 1px }");
        assert_eq!(change_class(&mut page, "b", "x"), ["b", "c", "d"]);
        drop(page);
        let mut page = list("p:nth-child(2 of .x) { width: 1px }");
        assert_eq!(change_class(&mut page, "b", "x"), ["b", "c", "d"]);
        drop(page);
        let mut page = list("p:nth-last-child(1 of .x) { width: 1px }");
        let restyled = change_class(&mut page, "c", "x");
        assert!(restyled.contains(&"a".to_owned()), "{restyled:?}");
        // An earlier sibling that matched no longer does.
        drop(page);
        let mut page = list("p:nth-last-child(1 of .x) { width: 1px }");
        change_class(&mut page, "b", "x");
        assert_eq!(page.style("b").width, px(1.0));
        change_class(&mut page, "c", "x");
        assert_eq!(page.style("b").width, computed_style::Size::Auto);
        assert_eq!(page.style("c").width, px(1.0));
    }

    /// Left of a descendant combinator, it is an ancestor's.
    #[test]
    fn an_ancestors_feature_before_siblings_styles_below() {
        let mut page = list(".x p ~ div { width: 1px }");
        let restyled = set_class(&mut page, "list", "x");
        assert!(restyled.contains(&"d".to_owned()), "{restyled:?}");
        assert_eq!(page.style("d").width, px(1.0));
    }

    /// An element's children changed, so may its `:empty`: what follows it
    /// that the sets find.
    #[test]
    fn an_emptiness_styles_the_siblings_its_sets_find() {
        let append_text = |page: &mut Page, id: &str| {
            page.change_at(id, |cx, element| {
                let text = cx.create_text(cx.document_of(element), "t");
                cx.append_child(element, text).unwrap();
            })
        };
        let mut page = list(":empty + p { width: 1px }");
        // Nothing after it: never matched as `:empty`, nothing to style.
        assert_eq!(append_text(&mut page, "a1"), Vec::<String>::new());
        drop(page);
        let mut page = Page::new(
            "<!DOCTYPE html><style>:empty + p { width: 1px }</style>\
             <div><p id=a></p><p id=b></p><p id=c></p></div>",
        );
        assert_eq!(page.style("b").width, px(1.0));
        assert_eq!(append_text(&mut page, "a"), ["a", "b"]);
        assert_eq!(page.style("b").width, computed_style::Size::Auto);
        drop(page);
        // Nested in a list.
        let mut page = Page::new(
            "<!DOCTYPE html><style>:is(:empty, .x) + p { width: 1px }</style>\
             <div><p id=a></p><p id=b></p><p id=c></p></div>",
        );
        let restyled = append_text(&mut page, "a");
        assert!(restyled.contains(&"b".to_owned()), "{restyled:?}");
        drop(page);
        // Nested with the sibling combinator.
        let mut page = Page::new(
            "<!DOCTYPE html><style>p:is(:empty + *) { width: 1px }</style>\
             <div><p id=a></p><p id=b></p></div>",
        );
        let restyled = append_text(&mut page, "a");
        assert!(restyled.contains(&"b".to_owned()), "{restyled:?}");
    }

    /// The pseudo-classes of an attribute: a link's `href`, a language's
    /// `lang`.
    #[test]
    fn a_link_and_a_language_have_sets_too() {
        let mut page = Page::new(
            "<!DOCTYPE html><style>a:any-link + p { width: 1px } :lang(fr) + p { height: 1px }</style>\
             <div><a id=a></a><p id=b></p><span id=c></span><p id=d></p></div>",
        );
        assert_eq!(set_attribute(&mut page, "a", "href", "x"), ["a", "b"]);
        assert_eq!(set_attribute(&mut page, "c", "lang", "fr"), ["c", "d"]);
    }

    /// The siblings it looks for by an id or an attribute.
    #[test]
    fn the_siblings_are_found_by_an_id_or_an_attribute() {
        let mut page = list(".x ~ #c { width: 1px } .x ~ [title] { height: 1px }");
        set_attribute(&mut page, "d", "title", "t");
        assert_eq!(set_class(&mut page, "a", "x"), ["a", "c", "d"]);
    }

    /// A list nested left of a sibling combinator: what it uses left of
    /// its own combinators is an ancestor's.
    #[test]
    fn a_nested_lists_ancestor_styles_below() {
        let mut page = list(":is(.y p) ~ div { width: 1px }");
        let restyled = set_class(&mut page, "list", "y");
        assert!(restyled.contains(&"d".to_owned()), "{restyled:?}");
    }

    /// The subject's nested list's ancestor (`p:is(.y *)`): what below it
    /// has the subject's features.
    #[test]
    fn the_subjects_nested_ancestor_styles_what_below_matches() {
        let mut page = list("p:is(.y *) { width: 1px }");
        assert_eq!(set_class(&mut page, "list", "y"), ["a", "b", "c", "list"]);
    }

    /// An element in: the later siblings as far as the run of `+` from it
    /// (the second after it, not only the next: the parent's selector
    /// flags'), or all of them for `~`.
    #[test]
    fn an_element_in_styles_the_siblings_its_position_reaches() {
        let mut page = Page::new(
            "<!DOCTYPE html><style>.x + * + .c { width: 1px }</style>\
             <div id=list><p id=a></p><p id=c class=c></p></div>",
        );
        restyled(&mut page, |cx, document| {
            let list = crate::by_id(cx, document, "list").unwrap();
            let x = cx.create_element(document, dom::QualName::html("p"));
            cx.set_attribute(x, "class", "x");
            cx.insert(list, x, cx.first_child(list));
        });
        assert_eq!(page.style("c").width, px(1.0));
        drop(page);
        let mut page = Page::new(
            "<!DOCTYPE html><style>.x ~ .c { width: 1px }</style>\
             <div id=list><p id=a></p><p id=b></p><p id=c class=c></p></div>",
        );
        restyled(&mut page, |cx, document| {
            let list = crate::by_id(cx, document, "list").unwrap();
            let x = cx.create_element(document, dom::QualName::html("p"));
            cx.set_attribute(x, "class", "x");
            cx.insert(list, x, cx.first_child(list));
        });
        assert_eq!(page.style("c").width, px(1.0));
    }

    /// A change of position (an element in) reaches as far as a run of
    /// `+` with no `~` among the rules.
    #[test]
    fn a_change_of_position_reaches_along_a_run_of_adjacents() {
        let mut page = Page::new(
            "<!DOCTYPE html><style>.x + * + .c { width: 1px }</style>\
             <div id=list><p id=a></p><p id=b></p><p id=c class=c></p></div>",
        );
        restyled(&mut page, |cx, document| {
            let list = crate::by_id(cx, document, "list").unwrap();
            let first = cx.create_element(document, dom::QualName::html("p"));
            cx.set_attribute(first, "class", "x");
            cx.insert(list, first, cx.first_child(list));
        });
        assert_eq!(page.style("b").width, computed_style::Size::Auto);
        assert_eq!(page.style("a").width, computed_style::Size::Auto);
        drop(page);
        // `.x`, `a`, `b`: `.c` is two after `.x` no longer.
        let mut page = Page::new(
            "<!DOCTYPE html><style>.x + * + .c { width: 1px }</style>\
             <div id=list><p id=a class=x></p><p id=c class=c></p></div>",
        );
        assert_eq!(page.style("c").width, computed_style::Size::Auto);
        restyled(&mut page, |cx, document| {
            let list = crate::by_id(cx, document, "list").unwrap();
            let middle = cx.create_element(document, dom::QualName::html("p"));
            let c = crate::by_id(cx, document, "c").unwrap();
            cx.insert(list, middle, Some(c));
        });
        assert_eq!(page.style("c").width, px(1.0));
        // Taken out again: two after no longer.
        restyled(&mut page, |cx, document| {
            let list = crate::by_id(cx, document, "list").unwrap();
            let a = crate::by_id(cx, document, "a").unwrap();
            let first = cx.create_element(document, dom::QualName::html("i"));
            cx.insert(list, first, Some(a));
            cx.remove(a);
        });
        assert_eq!(page.style("c").width, computed_style::Size::Auto);
    }

    /// A dropdown's toggle: what follows it that the rules look for, the
    /// content itself and the label's spans, not the rest of them.
    #[test]
    fn a_state_styles_the_siblings_and_descendants_the_sets_find() {
        let mut page = Page::new(
            "<!DOCTYPE html><style>#t:checked ~ .content { width: 1px }
             #t:checked ~ .label div span span:first-child { height: 1px }</style>
             <div><input id=t type=checkbox>
             <label id=l class=label><div id=ld><span id=s1><span id=s2></span><span id=s3></span></span></div><b id=lb></b></label>
             <ul id=c class=content><li id=c1></li></ul><p id=after></p></div>",
        );
        let restyled = page.change_at("t", |cx, t| cx.set_checked(t, true));
        assert_eq!(restyled, ["c", "s1", "s2", "s3", "t"]);
    }

    /// Among a host's children: all of them, as a structural change takes
    /// them.
    #[test]
    fn a_hosts_children_are_styled_again() {
        let mut page = Page::with_shadows(
            &format!("<!DOCTYPE html><style>.x + p {{ width: 1px }}</style>{LIST}"),
            &[("list", "<slot></slot>")],
        );
        let restyled = change_class(&mut page, "a", "x");
        assert!(restyled.contains(&"b".to_owned()), "{restyled:?}");
    }

    /// Below a host the set finds: its children that have what it looks
    /// for.
    #[test]
    fn a_hosts_descendants_take_the_descendant_part() {
        let mut page = Page::with_shadows(
            &format!("<!DOCTYPE html><style>.x ~ div span {{ width: 1px }}</style>{LIST}"),
            &[("d", "<slot></slot>")],
        );
        assert_eq!(change_class(&mut page, "a", "x"), ["a", "d1"]);
    }

    /// A shadow root's children: all of them, with a structural rule in
    /// its tree.
    #[test]
    fn a_shadow_roots_children_are_styled_again() {
        let mut page = Page::with_shadows(
            "<!DOCTYPE html><div id=host></div>",
            &[(
                "host",
                "<style>.x + p { width: 1px }</style><p id=s1></p><p id=s2></p>",
            )],
        );
        let restyled = change_class(&mut page, "s1", "x");
        assert!(restyled.contains(&"s2".to_owned()), "{restyled:?}");
    }

    /// A state the sets don't say: as children coming or going (the later
    /// siblings, not their children).
    #[test]
    fn a_state_the_sets_dont_say_styles_the_siblings_after() {
        let mut page = Page::new(
            "<!DOCTYPE html><style>:is(:checked, .y) ~ p { width: 1px }</style>\
             <div><input id=t type=checkbox><p id=a><span id=a1></span></p></div>",
        );
        let restyled = page.change_at("t", |cx, t| cx.set_checked(t, true));
        assert_eq!(restyled, ["a", "t"]);
    }

    /// A shadow tree's sibling change doesn't reach the host's children
    /// assigned to no slot: styled after the flat tree's children, they
    /// aren't its siblings.
    #[test]
    fn a_shadow_trees_siblings_arent_what_is_assigned_to_no_slot() {
        let mut page = Page::with_shadows(
            "<!DOCTYPE html><div id=host><b id=b slot=none></b></div>",
            &[(
                "host",
                "<style>.x ~ * { width: 1px }</style><p id=s0></p><p id=s1></p>",
            )],
        );
        let restyled = change_class(&mut page, "s0", "x");
        assert!(restyled.contains(&"s1".to_owned()), "{restyled:?}");
        assert!(!restyled.contains(&"b".to_owned()), "{restyled:?}");
    }

    /// A slot's earlier sibling, for what is assigned to it
    /// (`.x ~ slot::slotted(span)`): as children coming or going.
    #[test]
    fn a_slots_sibling_styles_what_is_assigned_to_it() {
        let mut page = Page::with_shadows(
            "<!DOCTYPE html><div id=host><span id=in></span></div>",
            &[(
                "host",
                "<style>.x ~ slot::slotted(span) { width: 1px }</style><p id=s0></p><slot></slot>",
            )],
        );
        let restyled = change_class(&mut page, "s0", "x");
        assert!(restyled.contains(&"in".to_owned()), "{restyled:?}");
        drop(page);
        // Under an element of the shadow tree.
        let mut page = Page::with_shadows(
            "<!DOCTYPE html><div id=host><span id=in></span></div>",
            &[(
                "host",
                "<style>.x ~ slot::slotted(span) { width: 1px }</style>\
                 <div><p id=s0></p><slot></slot></div>",
            )],
        );
        let restyled = change_class(&mut page, "s0", "x");
        assert!(restyled.contains(&"in".to_owned()), "{restyled:?}");
    }
}

/// What a change of the document's sheets styles again: what the rules of
/// the sheets that came and went may match, by their features (Blink's
/// `InvalidateForRuleSetChanges`); everything when no feature says.
mod sheet_changes {
    use super::*;

    /// A `<style id=id>` with `css` appended to the `<head id=head>` (whose
    /// children only sheets are: none of the others moves).
    fn add_sheet(page: &mut Page, id: &str, css: &str) -> Vec<String> {
        restyled(page, |cx, document| {
            let parent = crate::by_id(cx, document, "head").expect("a <head id=head>");
            let style = cx.create_element(document, dom::QualName::html("style"));
            cx.set_attribute(style, "id", id);
            let text = cx.create_text(document, css);
            cx.append_child(style, text).unwrap();
            cx.append_child(parent, style).unwrap();
        })
    }

    fn remove(page: &mut Page, id: &str) -> Vec<String> {
        restyled(page, |cx, document| {
            let element = crate::by_id(cx, document, id).expect("an element with the id");
            cx.remove(element);
        })
    }

    /// What an added sheet's rules may match: by a class (in either case:
    /// a document in no quirks mode matches as written), an id, an
    /// attribute, a tag, `::before`'s element, an `:is()`'s subjects; an
    /// ancestor's subtree, a sibling's later siblings; nothing for a sheet
    /// whose media don't match.
    #[test]
    fn an_added_sheet_styles_what_its_rules_may_match() {
        const HTML: &str = "<!DOCTYPE html><head id=head></head><body id=body>\
             <div id=a class=a><p id=a1><i id=a2></i></p></div>\
             <div id=b title=x><p id=b1></p></div>\
             <section id=c></section><p id=d></p><p id=e class=Big></p>";
        let mut page = Page::new(HTML);
        assert_eq!(
            add_sheet(&mut page, "s0", "@media print { p { color: red } }"),
            ["s0"]
        );
        // Or a `<style>`'s own.
        let print = restyled(&mut page, |cx, document| {
            let parent = crate::by_id(cx, document, "head").expect("a <head id=head>");
            let style = cx.create_element(document, dom::QualName::html("style"));
            cx.set_attribute(style, "id", "print");
            cx.set_attribute(style, "media", "print");
            let text = cx.create_text(document, "p { color: red }");
            cx.append_child(style, text).unwrap();
            cx.append_child(parent, style).unwrap();
        });
        assert_eq!(print, ["print"]);
        assert_eq!(
            add_sheet(&mut page, "big", ".Big { width: 9px }"),
            ["big", "e"]
        );
        assert_eq!(page.style("e").width, px(9.0));
        // What it passes on goes down to the children.
        assert_eq!(
            add_sheet(&mut page, "s1", ".a { color: red }"),
            ["a", "a1", "a2", "s1"]
        );
        assert_eq!(page.style("a2").color, RED);
        assert_eq!(add_sheet(&mut page, "s2", "#d { width: 1px }"), ["d", "s2"]);
        assert_eq!(
            add_sheet(&mut page, "s3", "[title] { width: 2px }"),
            ["b", "s3"]
        );
        assert_eq!(
            add_sheet(&mut page, "s4", "section::before { content: 'x' }"),
            ["c", "s4"]
        );
        assert!(
            page.pseudo("c")
                .is_some_and(|pseudo| pseudo.before.is_some())
        );
        assert_eq!(
            add_sheet(&mut page, "s5", ":is(#e, section) { height: 3px }"),
            ["c", "e", "s5"]
        );
        // No feature of its own: the subtree of an ancestor's.
        assert_eq!(
            add_sheet(&mut page, "s6", ".a > * { height: 4px }"),
            ["a", "a1", "a2", "s6"]
        );
        assert_eq!(page.style("a1").height, px(4.0));
        drop(page);
        // Or a sibling's later siblings: one for `+`, every one for `~`,
        // below them for a descendant right of it (each on a page of its
        // own: a sheet coming after one with `+` styles the sheets before
        // it again, by position).
        for (css, expected) in [
            ("#c + * { height: 5px }", &["d", "s"][..]),
            ("#c ~ * { height: 5px }", &["d", "e", "s"]),
            ("#a + * > * { height: 5px }", &["b1", "s"]),
        ] {
            let mut page = Page::new(HTML);
            assert_eq!(add_sheet(&mut page, "s", css), expected, "{css}");
            assert_eq!(page.style(expected[0]).height, px(5.0), "{css}");
        }
    }

    /// The rules of an `@media` or `@supports` whose condition holds count,
    /// an id in any case; a `:host` rule (a shadow tree's) in the
    /// document's sheet matches nothing.
    #[test]
    fn rules_in_conditions_and_rules_of_shadow_trees() {
        let mut page = Page::new(
            "<!DOCTYPE html><head id=head></head><body id=body>\
             <p id=a></p><p id=b></p><p id=Up></p><div id=d><p id=d1></p></div>",
        );
        assert_eq!(
            add_sheet(&mut page, "s1", "@media all { #a { width: 1px } }"),
            ["a", "s1"]
        );
        assert_eq!(page.style("a").width, px(1.0));
        assert_eq!(
            add_sheet(
                &mut page,
                "s2",
                "@supports (display: block) { #b { width: 2px } }"
            ),
            ["b", "s2"]
        );
        assert_eq!(
            add_sheet(&mut page, "s3", "#UP { width: 3px } #up { height: 3px }"),
            ["Up", "s3"]
        );
        assert_eq!(add_sheet(&mut page, "s4", ":host { width: 4px }"), ["s4"]);
        // A `:host` left of the subject: found by its subject, as in a
        // shadow tree's sheet (every `p`, though none matches here).
        assert_eq!(
            add_sheet(&mut page, "s5", ":host(#d) p { width: 5px }"),
            ["Up", "a", "b", "d1", "s5"]
        );
    }

    /// What no feature finds styles everything again: a subject and the
    /// rest with none (`*`, `:root`, a state, `:not()`), `:has()`,
    /// `@keyframes`; and sheets kept in another order.
    #[test]
    fn what_no_feature_finds_styles_everything_again() {
        let html = "<!DOCTYPE html><html id=root><head id=head></head><body id=body><p id=one></p><div id=two></div>";
        for css in [
            "* { width: 1px }",
            ":root { width: 1px }",
            ":hover { width: 1px }",
            ":not(.a) { width: 1px }",
            "p:has(i) { width: 1px }",
            "@keyframes k { to { width: 1px } }",
        ] {
            let mut page = Page::new(html);
            let restyled = add_sheet(&mut page, "s", css);
            assert_eq!(
                restyled,
                ["body", "head", "one", "root", "s", "two"],
                "{css}"
            );
        }
        let mut page = Page::new(
            "<!DOCTYPE html><html id=root><head id=head><style id=s1>p { width: 1px }</style>\
             <style id=s2>p { width: 2px }</style></head><body><p id=one></p><div id=two></div>",
        );
        let moved = restyled(&mut page, |cx, document| {
            let head = crate::by_id(cx, document, "head").unwrap();
            let s1 = crate::by_id(cx, document, "s1").unwrap();
            cx.append_child(head, s1).unwrap();
        });
        assert!(
            moved.contains(&"two".to_owned()),
            "another order: {moved:?}"
        );
        assert_eq!(page.style("one").width, px(1.0));
    }

    /// A sheet that goes styles what its rules matched; one with `:has()`,
    /// everything.
    #[test]
    fn a_removed_sheet_styles_what_its_rules_matched() {
        let mut page = Page::new(
            "<!DOCTYPE html><style id=s>.a { width: 1px }</style>\
             <body><p id=one class=a></p><div id=two></div>",
        );
        assert_eq!(remove(&mut page, "s"), ["one"]);
        assert_eq!(page.style("one").width, computed_style::Size::Auto);
        drop(page);
        let mut page = Page::new(
            "<!DOCTYPE html><html id=root><style id=s>p:has(i) { width: 1px }</style>\
             <body id=body><p id=one><i></i></p><div id=two></div>",
        );
        // The `<head>` and the `<i>` too (`""`).
        assert_eq!(
            remove(&mut page, "s"),
            ["", "", "body", "one", "root", "two"]
        );
    }

    /// A `<style>` whose text changed is another sheet: what the old rules
    /// and the new match; a kept one is left alone, and a change of its
    /// element that leaves its sheet as it was styles nothing.
    #[test]
    fn an_edited_sheet_styles_what_its_old_and_new_rules_match() {
        let mut page = Page::new(
            "<!DOCTYPE html><style id=k>.k { width: 3px }</style><style id=s>.a { width: 1px }</style>\
             <body><p id=one class=a></p><p id=two class=b></p><p id=three class=k></p>",
        );
        let edited = restyled(&mut page, |cx, document| {
            let style = crate::by_id(cx, document, "s").unwrap();
            cx.set_text_content(style, ".b { width: 2px }");
        });
        assert_eq!(edited, ["one", "two"]);
        assert_eq!(page.style("one").width, computed_style::Size::Auto);
        assert_eq!(page.style("two").width, px(2.0));
        // The same sheet; and no rule uses `title`.
        assert!(set_attribute(&mut page, "s", "title", "").is_empty());
    }

    /// A sheet's `media` that stops matching: its rules' elements; a
    /// viewport across one of its queries, everything.
    #[test]
    fn a_sheets_media_styles_what_its_rules_match() {
        let mut page = Page::new(
            "<!DOCTYPE html><html id=root><style id=s>.a { width: 1px } @media (min-width: 100px) { .b { width: 2px } }</style>\
             <body id=body><p id=one class=a></p><p id=two class=b></p>",
        );
        assert_eq!(page.style("one").width, px(1.0));
        // (Not the `<style>`: no rule uses `media`.)
        assert_eq!(set_attribute(&mut page, "s", "media", "print"), ["one"]);
        assert_eq!(page.style("one").width, computed_style::Size::Auto);
        assert_eq!(set_attribute(&mut page, "s", "media", "all"), ["one"]);
        page.engine.set_viewport(gfx::SizeF::new(200.0, 100.0), 1.0);
        let resized = restyled(&mut page, |_, _| {});
        // The `<head>` too (`""`).
        assert_eq!(resized, ["", "body", "one", "root", "s", "two"]);
        assert_eq!(page.style("two").width, px(2.0));
    }

    /// A `<link>`'s sheet that came: what its rules match.
    #[test]
    fn a_linked_sheet_styles_what_its_rules_match() {
        let mut page = Page::new(
            "<!DOCTYPE html><link id=l rel=stylesheet href=a.css><body><p id=one class=a></p><p id=two></p>",
        );
        crate::give_link_sheet(&mut page, "l", ".a { width: 1px }");
        assert_eq!(restyled(&mut page, |_, _| {}), ["one"]);
        assert_eq!(page.style("one").width, px(1.0));
    }
}

/// What the DOM notes of a change is what style reads (`dom::
/// StyleNames`): a name some rule uses, those of a sheet that came, an
/// attribute `attr()` read, a pseudo-class's attribute; nothing else.
mod noted_names {
    use super::*;

    fn content(page: &mut Page, id: &str) -> Option<computed_style::Content> {
        page.pseudo(id).and_then(|pseudo| {
            pseudo
                .before
                .as_ref()
                .map(|before| before.rare.content.clone())
        })
    }

    #[test]
    fn an_attribute_attr_reads_is_noted() {
        let mut page = Page::new(
            "<!doctype html><style>p::before { content: attr(data-x) }</style><p id=p data-x=a>p</p>",
        );
        let before = content(&mut page, "p");
        assert_eq!(set_attribute(&mut page, "p", "data-x", "b"), ["p"]);
        assert_ne!(content(&mut page, "p"), before);
        assert!(
            set_attribute(&mut page, "p", "data-y", "b").is_empty(),
            "one it doesn't read"
        );
    }

    #[test]
    fn a_name_of_a_sheet_that_came_is_noted() {
        let mut page = Page::new("<!doctype html><head id=head></head><p id=p>p</p>");
        assert!(set_class(&mut page, "p", "n").is_empty());
        restyled(&mut page, |cx, document| {
            let head = crate::by_id(cx, document, "head").unwrap();
            let style = cx.create_element(document, dom::QualName::html("style"));
            let text = cx.create_text(document, ".m { width: 1px }");
            cx.append_child(style, text).unwrap();
            cx.append_child(head, style).unwrap();
        });
        assert_eq!(set_class(&mut page, "p", "m"), ["p"]);
        assert_eq!(page.style("p").width, px(1.0));
    }

    /// `css`'s page, styled, then the DOM given no names (as if style
    /// missed some), and `#p`'s class set to `x`.
    #[cfg(debug_assertions)]
    fn class_passed_over(css: &str) {
        let mut page = Page::new(&format!(
            "<!doctype html><style>{css}</style><div id=p><i>i</i></div>"
        ));
        let document = page.document.clone();
        page.dom
            .mutate(|cx| cx.set_style_names(document.get(cx.gc), dom::StyleNames::default()));
        set_class(&mut page, "p", "x");
    }

    /// The debug build's check of what the DOM didn't note: the element's
    /// own style.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "styles otherwise")]
    fn the_debug_build_checks_a_change_passed_over() {
        class_passed_over(".x { width: 1px }");
    }

    /// And what the sets would have found of others.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "styles others again")]
    fn the_debug_build_checks_what_a_change_passed_over_finds() {
        class_passed_over(".x i { width: 1px }");
    }

    #[test]
    fn a_pseudo_classes_attribute_is_noted() {
        let mut page = Page::new(
            "<!doctype html><style>details:open { width: 5px }</style><details id=d><summary>s</summary></details>",
        );
        assert_eq!(set_attribute(&mut page, "d", "open", ""), ["d"]);
        assert_eq!(page.style("d").width, px(5.0));
    }
}

/// `::selection` rules: the element's highlight from its own and its
/// parent's (highlight inheritance), a property its rules don't declare
/// its parent's; one with no rule shares its parent's. A change of what a
/// `::selection` rule asks of the element styles it again, its children
/// for what they inherit: not its subtree.
mod selection {
    use computed_style::StyleColor;

    use super::*;

    fn highlight(page: &mut Page, id: &str) -> Option<computed_style::HighlightStyle> {
        page.style(id).rare_inherited.highlight.as_deref().cloned()
    }

    #[test]
    fn a_highlight_inherits_and_cascades() {
        let mut page = Page::new(
            "<!DOCTYPE html><style>#a::selection { background-color: rgb(0, 128, 0); color: rgb(0, 0, 255) }\
             #b::selection { color: rgb(255, 0, 0) }</style><div id=a><p id=b><span id=c></span></p></div><i id=d></i>",
        );
        let green = StyleColor::Rgba(Color::rgb(0, 128, 0));
        let a = highlight(&mut page, "a").expect("its own");
        assert_eq!(
            (a.background_color, a.color),
            (green, StyleColor::Rgba(Color::rgb(0, 0, 255)))
        );
        let b = highlight(&mut page, "b").expect("its own");
        assert_eq!(
            (b.background_color, b.color),
            (green, StyleColor::Rgba(Color::rgb(255, 0, 0))),
            "its parent's background"
        );
        let c = highlight(&mut page, "c").expect("inherited");
        assert_eq!(c, b);
        assert!(
            highlight(&mut page, "d").is_none(),
            "none matched it or an ancestor"
        );
    }

    #[test]
    fn a_class_change_styles_the_element_and_what_inherits() {
        let mut page = Page::new(
            "<!DOCTYPE html><style>.x::selection { background-color: rgb(0, 128, 0) }</style>\
             <div id=a><p id=b><span id=c></span></p></div><em id=e></em>",
        );
        assert!(highlight(&mut page, "c").is_none());
        // Not `e`, nor everything: the element's own rule.
        assert_eq!(set_class(&mut page, "a", "x"), ["a", "b", "c"]);
        assert_eq!(
            highlight(&mut page, "c").map(|highlight| highlight.background_color),
            Some(StyleColor::Rgba(Color::rgb(0, 128, 0))),
            "inherited by the subtree"
        );
        // A class no `::selection` rule asks of: nothing.
        assert!(set_class(&mut page, "b", "y").is_empty());
    }

    /// A change a `::selection` rule asks of: the element alone if it
    /// doesn't make it match (its highlight the same, nothing inherits a
    /// new one), of an ancestor the descendants the subject's feature finds,
    /// as an element's own rule's; not the subtree, as a UA shadow tree's
    /// part's.
    #[test]
    fn a_change_that_matches_nothing_styles_the_element_alone() {
        let mut page = Page::new(
            "<!DOCTYPE html><style>.x.y::selection { background-color: rgb(0, 128, 0) }\
             .x .z::selection { color: rgb(0, 128, 0) }</style>\
             <div id=a><p id=b><span id=c class=z></span></p></div>",
        );
        assert_eq!(
            set_class(&mut page, "a", "x"),
            ["a", "c"],
            "the descendants with `z`"
        );
    }

    /// A sheet with a `::selection` rule that matches nothing, added: the
    /// elements its rule's subject's feature finds, not their subtrees.
    #[test]
    fn an_added_sheets_selection_rule_styles_its_subject() {
        let mut page = Page::new(
            "<!DOCTYPE html><head id=head></head><body id=body>\
             <div id=a class=x><p id=b><span id=c></span></p></div>",
        );
        let restyled = restyled(&mut page, |cx, document| {
            let parent = crate::by_id(cx, document, "head").expect("a <head id=head>");
            let style = cx.create_element(document, dom::QualName::html("style"));
            cx.set_attribute(style, "id", "s");
            let text = cx.create_text(document, ".x.nothing::selection { color: red }");
            cx.append_child(style, text).unwrap();
            cx.append_child(parent, style).unwrap();
        });
        assert_eq!(restyled, ["a", "s"]);
    }
}

/// The element whose `id` is `id` in `document`'s tree or a shadow tree in
/// it.
fn at<'gc>(cx: &Cx<'_, 'gc>, document: NodeRef<'gc>, id: &str) -> NodeRef<'gc> {
    crate::find(cx, document, id).unwrap_or_else(|| panic!("no #{id}"))
}

/// A new HTML `name` element of `document`, with `id` if given.
fn element<'gc>(
    cx: &Cx<'_, 'gc>,
    document: NodeRef<'gc>,
    name: &str,
    id: Option<&str>,
) -> NodeRef<'gc> {
    let element = cx.create_element(document, QualName::html(name));
    if let Some(id) = id {
        cx.set_attribute(element, "id", id);
    }
    element
}

/// `document`'s `<body>`: its root element's last child.
fn body<'gc>(cx: &Cx<'_, 'gc>, document: NodeRef<'gc>) -> NodeRef<'gc> {
    let html = cx.last_child(document).expect("<html>");
    cx.last_child(html).expect("<body>")
}

/// [`Page::change`]s the page by appending a `<style>` with id `id` and
/// text `css` to `parent` (its `<body>` if `None`, else the shadow root of
/// the element with that id).
fn add_style_to(page: &mut Page, parent: Option<&str>, id: &str, css: &str) -> Vec<String> {
    page.change(|cx, document| {
        let parent = match parent {
            None => body(cx, document),
            Some(host) => cx.shadow_root(at(cx, document, host)).expect("a host"),
        };
        let style = element(cx, document, "style", Some(id));
        let text = cx.create_text(document, css);
        cx.append_child(style, text).unwrap();
        cx.append_child(parent, style).unwrap();
    })
}

/// [`Page::change`]s the page by appending a `<style>` with id `id` and
/// text `css` to its `<body>`.
fn add_sheet(page: &mut Page, id: &str, css: &str) -> Vec<String> {
    add_style_to(page, None, id, css)
}

/// [`Page::change`]s the class of the element with id `id` to `class`.
fn change_class(page: &mut Page, id: &str, class: &str) -> Vec<String> {
    page.change_at(id, |cx, element| cx.set_attribute(element, "class", class))
}

/// [`Page::change`]s the attribute `name` of the element with id `id` to
/// `value`.
fn change_attribute(page: &mut Page, id: &str, name: &str, value: &str) -> Vec<String> {
    page.change_at(id, |cx, element| cx.set_attribute(element, name, value))
}

/// A sheet object, rooted.
type Sheet = elise::Persistent<dom::CssStyleSheet>;

/// A constructed sheet of `contents`, rooted.
fn constructed(page: &mut Page, contents: &Arc<StyleSheetContents>) -> Sheet {
    page.with_document(|cx, document| {
        elise::Persistent::new(
            cx.gc,
            cx.create_style_sheet(document, contents.clone(), "", false),
        )
    })
}

/// [`Page::change`]s the document's adopted sheets to sheet objects of
/// `contents`: one made for each contents not in `made` yet, the same one
/// again for one that is.
fn adopt(
    page: &mut Page,
    made: &mut Vec<(Arc<StyleSheetContents>, Sheet)>,
    contents: &[Arc<StyleSheetContents>],
) -> Vec<String> {
    let mut sheets = Vec::new();
    for contents in contents {
        let sheet = match made.iter().find(|(made, _)| Arc::ptr_eq(made, contents)) {
            Some((_, sheet)) => sheet.clone(),
            None => {
                let sheet = constructed(page, contents);
                made.push((contents.clone(), sheet.clone()));
                sheet
            }
        };
        sheets.push(sheet);
    }
    page.change(|cx, document| {
        while !cx.adopted_style_sheets(document).is_empty() {
            cx.remove_last_adopted_style_sheet(document);
        }
        for (index, sheet) in sheets.iter().enumerate() {
            cx.set_adopted_style_sheet(document, index, sheet.get(cx.gc));
        }
    })
}

/// The ids of the elements styled with a blocking `wheel` listener over
/// them.
fn inside_blocking_wheel(page: &mut Page) -> Vec<String> {
    page.styles()
        .iter()
        .filter(|(_, style)| style.inside_blocking_wheel_event_handler)
        .map(|(id, _)| id.clone())
        .collect()
}

const LIST: &str = "<!DOCTYPE html><style>.big { width: 50px } .big span { height: 5px }</style>
    <div id=a><span id=a1></span></div>
    <div id=b><span id=b1></span></div>
    <div id=c><span id=c1></span></div>";

#[test]
fn nothing_changed_styles_nothing() {
    let mut page = Page::new(LIST);
    assert_eq!(page.change(|_, _| {}), Vec::<String>::new());
}

#[test]
fn a_changed_attribute_styles_the_element_and_its_subtree_again() {
    let mut page = Page::new(LIST);
    assert_eq!(change_class(&mut page, "b", "big"), ["b", "b1"]);
}

/// A change in a rare inherited property (one of the group most elements
/// share with their parent) styles the children again, as any inherited
/// one does; a non-inherited one doesn't.
#[test]
fn a_changed_rare_inherited_property_styles_the_children_again() {
    let mut page = Page::new(LIST);
    let restyled = change_attribute(&mut page, "a", "style", "text-indent: 5px");
    assert_eq!(restyled, ["a", "a1"]);
    assert_eq!(
        page.styles()["a1"].rare_inherited.text_indent,
        computed_style::LengthPercentage::Px(5.0)
    );
    let restyled = change_attribute(&mut page, "c", "style", "min-width: 5px");
    assert_eq!(restyled, ["c"]);
}

/// A cell's presentational hints are its table's `cellpadding`, `border`
/// and `bordercolor` too: the table's changing styles the cells again.
#[test]
fn a_tables_attribute_styles_its_cells_again() {
    let mut page = Page::new(
        "<!DOCTYPE html><table id=table><tbody id=tbody><tr id=tr><td id=td>x</td></tr></tbody></table>",
    );
    for (name, value) in [
        ("cellpadding", "7"),
        ("border", "1"),
        ("bordercolor", "red"),
    ] {
        let restyled = change_attribute(&mut page, "table", name, value);
        assert_eq!(restyled, ["table", "tbody", "td", "tr"], "{value}");
    }
    let td = &page.styles()["td"];
    assert_eq!(td.padding.top, computed_style::LengthPercentage::Px(7.0));
    assert_eq!(td.border_width.left, 1.0);
    assert_eq!(td.border_style.left, computed_style::BorderStyle::Solid);
}

/// A table's `bordercolor` is its cells' border color: changing it styles
/// them again, though their rows, with a border color of their own, don't
/// change.
#[test]
fn a_tables_bordercolor_styles_its_cells_again() {
    let mut page = Page::new(
        "<!DOCTYPE html><style>tr { border-color: black }</style><table id=table border=1 \
         bordercolor=red><tbody id=tbody><tr id=tr><td id=td>x</td></tr></tbody></table>",
    );
    let restyled = change_attribute(&mut page, "table", "bordercolor", "blue");
    assert!(restyled.contains(&"td".to_owned()), "{restyled:?}");
    assert_eq!(
        page.styles()["td"].border_color.top,
        computed_style::StyleColor::Rgba(gfx::Color::rgb(0, 0, 255))
    );
}

/// The `<body>`'s `link` is the document's link color: changing it styles
/// every element again (a link's color is `-webkit-link`), and the links
/// take it.
#[test]
fn the_bodys_link_styles_everything_again() {
    let mut page =
        Page::new("<!DOCTYPE html><body id=body><p id=p><a id=a href=x>a</a></p><div id=d></div>");
    let restyled = change_attribute(&mut page, "body", "link", "red");
    assert_eq!(restyled, ["a", "body", "d", "p"]);
    assert_eq!(page.styles()["a"].color, gfx::Color::rgb(255, 0, 0));
    // Another attribute of it styles nothing: no rule reads it, so the
    // DOM doesn't note it.
    let restyled = change_attribute(&mut page, "body", "title", "t");
    assert_eq!(restyled, Vec::<String>::new());
}

/// The page taking or losing the focus styles everything again if some
/// selector has `:window-inactive`, in the document or a shadow tree, and
/// nothing otherwise.
#[test]
fn the_pages_focus_styles_everything_again_if_a_rule_asks() {
    fn page_focused<'gc>(cx: &Cx<'_, 'gc>, document: NodeRef<'gc>) {
        let window = cx
            .window_of(document)
            .unwrap_or_else(|| cx.create_window(document));
        cx.set_page_focused(window, true);
    }
    let mut page = Page::new("<!DOCTYPE html><body id=body><p id=p>p</p>");
    let restyled = page.change(page_focused);
    assert!(restyled.is_empty(), "{restyled:?}");

    drop(page);
    let mut page = Page::with_shadows(
        "<!DOCTYPE html><body id=body><div id=h></div><p id=p>p</p>",
        &[(
            "h",
            "<style>span:window-inactive { color: rgb(0, 0, 255) }</style><span id=s>s</span>",
        )],
    );
    assert_eq!(page.styles()["s"].color, gfx::Color::rgb(0, 0, 255));
    let restyled = page.change(page_focused);
    assert!(restyled.contains(&"s".to_owned()), "{restyled:?}");
    assert!(restyled.contains(&"p".to_owned()), "{restyled:?}");
    assert_eq!(page.styles()["s"].color, gfx::Color::rgb(0, 0, 0));
}

#[test]
fn an_inserted_element_is_styled_and_nothing_else() {
    let mut page = Page::new(LIST);
    let restyled = page.change_at("b", |cx, b| {
        let new = element(cx, cx.document_of(b), "i", Some("new"));
        cx.append_child(b, new).unwrap();
    });
    assert_eq!(restyled, ["new"]);
}

/// With a selector on the shape of the tree, what matches the siblings of
/// what came, went or changed may change: those siblings are styled again,
/// as what matching found they depend on says (the first and last for
/// `:first-child`, the next for `+`), not the parent.
#[test]
fn structural_selectors_style_the_siblings_again() {
    let html =
        "<!DOCTYPE html><style>li:first-child { width: 10px } .x + li { height: 3px }</style>
        <ul id=list><li id=one></li><li id=two></li></ul><p id=other></p>";
    let mut page = Page::new(html);
    let restyled = page.change(|cx, document| {
        let first = element(cx, document, "li", Some("zero"));
        cx.insert(
            at(cx, document, "list"),
            first,
            Some(at(cx, document, "one")),
        );
    });
    assert_eq!(restyled, ["one", "two", "zero"], "not #other");
    // A class on one, which `.x + li` looks at from its next sibling (by
    // its sibling invalidation set); an attribute moves no child, so
    // `:first-child` takes none.
    assert_eq!(change_class(&mut page, "one", "x"), ["one", "two"]);
    drop(page);
    // Without such selectors, the siblings stay.
    let mut page = Page::new(LIST);
    let restyled = page.change_at("a", |cx, a| cx.remove(a));
    assert_eq!(restyled, Vec::<String>::new());
}

/// `:empty` changes with the text in an element, which has no style of its
/// own.
#[test]
fn text_that_changes_empty_styles_its_parent_again() {
    let html =
        "<!DOCTYPE html><style>div:empty { width: 10px }</style><div id=e></div><div id=f></div>";
    let mut page = Page::new(html);
    page.with("e", |cx, e| {
        let text = cx.create_text(cx.document_of(e), "");
        cx.append_child(e, text).unwrap();
    });
    page.recalc();
    let restyled = page.change_at("e", |cx, e| {
        cx.set_data(cx.first_child(e).unwrap(), "now not empty");
    });
    assert_eq!(restyled, ["e"]);
}

/// `:has()` looks down from an element: a change anywhere may change what
/// matches anywhere.
#[test]
fn has_styles_everything_again() {
    let html = "<!DOCTYPE html><style>div:has(.deep) { width: 10px }</style>
        <div id=outer><p id=middle><span id=deep></span></p></div><p id=other></p>";
    let mut page = Page::new(html);
    let restyled = change_class(&mut page, "deep", "deep");
    assert_eq!(restyled, ["deep", "middle", "other", "outer"]);
}

/// A `<style>` whose text changed: its old rules went and its new ones
/// came, and what either matches is styled again.
#[test]
fn a_changed_stylesheet_styles_what_its_rules_match() {
    let html =
        "<!DOCTYPE html><style id=sheet>p { width: 1px }</style><p id=one></p><div id=two></div>";
    let mut page = Page::new(html);
    let set_text = |page: &mut Page, css: &'static str| {
        page.change_at("sheet", |cx, sheet| {
            cx.set_data(cx.first_child(sheet).expect("a text child"), css);
        })
    };
    assert_eq!(set_text(&mut page, "p { width: 2px }"), ["one"]);
    assert_eq!(set_text(&mut page, "div { width: 2px }"), ["one", "two"]);
}

/// The adopted stylesheets tell whether they changed by being other
/// contents (another `Arc`): the same ones again style nothing; other ones
/// (even with the same text), one more or one fewer style what their rules
/// match; another order, everything.
#[test]
fn other_adopted_contents_style_what_their_rules_match() {
    let html = "<!DOCTYPE html><p id=one></p><div id=two></div>";
    let mut page = Page::new(html);
    let mut made = Vec::new();
    let (a, b) = (
        crate::sheet("p { width: 1px }"),
        crate::sheet("p { width: 2px }"),
    );
    let everything = ["one", "two"];
    assert_eq!(
        adopt(&mut page, &mut made, std::slice::from_ref(&a)),
        ["one"]
    );
    assert_eq!(page.styles()["one"].width, px(1.0));
    assert_eq!(
        adopt(&mut page, &mut made, std::slice::from_ref(&a)),
        Vec::<String>::new()
    );
    assert_eq!(
        adopt(&mut page, &mut made, &[a.clone(), b.clone()]),
        ["one"]
    );
    assert_eq!(page.styles()["one"].width, px(2.0));
    assert_eq!(
        adopt(&mut page, &mut made, &[b.clone(), a.clone()]),
        everything
    );
    assert_eq!(page.styles()["one"].width, px(1.0));
    // An edit makes new contents, with the rules it kept: only the rule
    // it added is new.
    let edited = Arc::new(a.insert_rule("div { width: 3px }", 1).unwrap());
    assert_eq!(adopt(&mut page, &mut made, &[b.clone(), edited]), ["two"]);
    assert_eq!(page.styles()["two"].width, px(3.0));
    assert_eq!(
        adopt(&mut page, &mut made, &[b, crate::sheet("p { width: 1px }")]),
        everything
    );
    assert_eq!(adopt(&mut page, &mut made, &[]), ["one"]);
    assert_eq!(page.styles()["one"].width, computed_style::Size::Auto);
}

/// What an adopted sheet's selectors depend on counts as a `<style>`'s
/// does: a structural selector only there still styles siblings again.
#[test]
fn an_adopted_sheets_selectors_count_for_invalidation() {
    let mut page = Page::new("<!DOCTYPE html><div id=list><p id=a></p><p id=b></p></div>");
    adopt(
        &mut page,
        &mut Vec::new(),
        &[crate::sheet(".x + p { width: 1px }")],
    );
    let restyled = change_class(&mut page, "a", "x");
    assert!(restyled.contains(&"b".to_string()), "{restyled:?}");
    assert_eq!(page.styles()["b"].width, px(1.0));
}

/// `rem` is the root's font size: when it changes, every element may change.
#[test]
fn another_root_font_size_styles_everything_again() {
    let html = "<!DOCTYPE html><html id=root><style>p { width: 2rem }</style><p id=one></p><div id=two></div></html>";
    let mut page = Page::new(html);
    let restyled = change_attribute(&mut page, "root", "style", "font-size: 10px");
    assert_eq!(restyled, ["one", "root", "two"]);
    assert_eq!(page.styles()["one"].width, px(20.0));
}

/// Each kind of selector that looks at siblings or the shape of the tree,
/// alone in its stylesheet (so no other kind makes it count): a change that
/// makes a sibling match otherwise styles it again.
#[test]
fn each_structural_selector_styles_the_siblings_again() {
    #[derive(Clone, Copy)]
    enum Change {
        /// A class on the first `<p>`.
        Class(&'static str),
        /// A new `<p>` before it.
        InsertFirst,
        /// The last `<p>` gone.
        RemoveLast,
    }
    use Change::*;
    let cases = [
        (".x + p { width: 1px }", Class("x")),
        (".x ~ p { width: 1px }", Class("x")),
        ("p:nth-child(2) { width: 1px }", InsertFirst),
        ("p:last-child { width: 1px }", RemoveLast),
        (":is(.x + p) { width: 1px }", Class("x")),
        (":where(.x ~ p) { width: 1px }", Class("x")),
        ("p:not(.x + p) { width: 1px }", Class("x")),
        ("p:nth-child(1 of .y) { width: 1px }", Class("y")),
    ];
    for (css, change) in cases {
        let mut page = Page::new(&format!(
            "<!DOCTYPE html><style>{css}</style>
             <div id=list><p id=a></p><p id=b class=y></p><p id=c></p></div>"
        ));
        let restyled = page.change(|cx, document| {
            let a = at(cx, document, "a");
            match change {
                Class(class) => cx.set_attribute(a, "class", class),
                InsertFirst => {
                    let new = element(cx, document, "p", None);
                    cx.insert(at(cx, document, "list"), new, Some(a));
                }
                RemoveLast => cx.remove(at(cx, document, "c")),
            }
        });
        assert!(
            restyled.contains(&"b".to_string()),
            "{css}: the sibling styled again, not only {restyled:?}"
        );
    }
}

/// The elements whose boxes have to take a new style (layout tree flags),
/// by id, clearing every node's flags as layout does.
fn style_changed(page: &mut Page) -> Vec<String> {
    fn collect<'gc>(cx: &Cx<'_, 'gc>, node: NodeRef<'gc>, out: &mut Vec<String>) {
        if cx
            .layout_tree_flags(node)
            .contains(dom::LayoutTreeFlags::STYLE_CHANGED)
            && let Some(id) = crate::id_of(cx, node)
        {
            out.push(id);
        }
        cx.clear_layout_tree_flags(node);
        let mut child = cx.first_child(node);
        while let Some(at) = child {
            collect(cx, at, out);
            child = cx.next_sibling(at);
        }
    }
    page.with_document(|cx, document| {
        let mut out = Vec::new();
        collect(cx, document, &mut out);
        out
    })
}

/// Styled again with the same values, an element's boxes have nothing to
/// take: only the elements whose style changed are flagged for layout.
#[test]
fn only_other_values_flag_the_boxes() {
    let mut page = Page::new(LIST);
    assert!(style_changed(&mut page).contains(&"b".to_owned()), "new");
    change_class(&mut page, "b", "big");
    assert_eq!(style_changed(&mut page), ["b", "b1"]);
    // A class no rule uses: nothing, the DOM doesn't note it.
    let restyled = change_class(&mut page, "b", "big other");
    assert_eq!(restyled, Vec::<String>::new());
    assert_eq!(style_changed(&mut page), Vec::<String>::new());
}

#[test]
fn a_flex_containers_children_follow_its_display() {
    use computed_style::Display;
    let mut page = Page::new("<div id=d><span id=s></span></div>");
    for (style, want) in [
        ("display: flex", (Display::Flex, Display::Block)),
        ("", (Display::Block, Display::Inline)),
    ] {
        let restyled = change_attribute(&mut page, "d", "style", style);
        assert_eq!(restyled, ["d", "s"], "style={style:?}");
        let styles = page.styles();
        assert_eq!((styles["d"].display, styles["s"].display), want);
    }
}

#[test]
fn a_flex_items_z_index_follows_its_parent() {
    let mut page = Page::new("<div id=d><p id=p style='z-index: 1'></p></div>");
    for (style, want) in [("display: flex", Some(1)), ("", None)] {
        change_attribute(&mut page, "d", "style", style);
        assert_eq!(
            page.styles()["p"].effective_z_index,
            want,
            "style={style:?}"
        );
    }
}

/// Each document has its own engine (Blink's `StyleEngine` belongs to its
/// `Document`), sharing the UA stylesheet: styling one document leaves the
/// other's engine as it was, so styling that again with nothing changed
/// styles nothing. One engine for both would see other stylesheets each
/// time and style everything again.
#[test]
fn each_document_has_its_own_engine() {
    fn style_of(
        dom: &mut dom::Dom,
        token: &dom::LocalToken,
        document: &dom::NodeRoot,
    ) -> Arc<computed_style::ComputedStyle> {
        dom.read(|cx| {
            let mut found = None;
            crate::each_element(cx, document.get(cx.gc), &mut |element| {
                // Each document's `<p>`, its one element with an id.
                if found.is_none() && crate::id_of(cx, element).is_some() {
                    found = cx.computed_style(element, token).cloned();
                }
            });
            found.expect("a styled <p>")
        })
    }
    /// Whether styling `document` with `engine` styles anything again.
    fn restyles(
        engine: &mut style::StyleEngine,
        dom: &mut dom::Dom,
        token: &mut dom::LocalToken,
        document: &dom::NodeRoot,
    ) -> bool {
        dom.mutate(|cx| engine.recalc_style(cx, token, document.get(cx.gc)));
        !engine.restyled_for_testing().is_empty()
    }

    let mut dom = dom::Dom::new();
    let mut token = dom::LocalToken::new();
    let first = html::parser::parse_document(
        &mut dom,
        "<style>p { width: 1px }</style><p id=p></p>",
        false,
    )
    .root()
    .clone();
    let second = html::parser::parse_document(
        &mut dom,
        "<style>p { width: 2px }</style><p id=q></p>",
        false,
    )
    .root()
    .clone();
    let mut engine = style::StyleEngine::new();
    let mut other = style::StyleEngine::new();
    dom.mutate(|cx| engine.recalc_style(cx, &mut token, first.get(cx.gc)));
    dom.mutate(|cx| other.recalc_style(cx, &mut token, second.get(cx.gc)));
    assert!(
        !restyles(&mut engine, &mut dom, &mut token, &first),
        "nothing changed"
    );
    assert_ne!(
        style_of(&mut dom, &token, &first),
        style_of(&mut dom, &token, &second)
    );

    // One engine for both: the second's sheets replace the first's.
    dom.mutate(|cx| engine.recalc_style(cx, &mut token, second.get(cx.gc)));
    assert!(restyles(&mut engine, &mut dom, &mut token, &first));
    drop((first, second));
}

/// A focus state change styles the element again (with its subtree) only
/// if some rule uses that state: none for a page whose rules don't.
#[test]
fn a_focus_change_styles_again_only_what_rules_use() {
    let focus_shown = |css: &str, visible: bool| {
        let html = format!(
            "<!DOCTYPE html><style>{css}</style>\
             <div id=a><div id=b><span id=b1></span></div><span id=c></span></div>"
        );
        let mut page = Page::new(&html);
        page.change(|cx, document| {
            focus(cx, document, Some(at(cx, document, "b")), true, visible);
        })
    };
    let focus_visibly = |css: &str| focus_shown(css, true);
    // The focus from a click: no rule uses `:focus`.
    assert_eq!(
        focus_shown(".x { width: 1px }", false),
        Vec::<String>::new()
    );
    // The UA stylesheet's focus ring uses `:focus-visible`: the element
    // alone, which no rule for elements below uses it of.
    assert_eq!(focus_visibly(".x { width: 1px }"), ["b"]);
    assert_eq!(focus_visibly(":focus { width: 1px }"), ["b"]);
    assert_eq!(focus_visibly(":focus-visible { width: 1px }"), ["b"]);
    // Every ancestor's `:focus-within` changed: each of them is styled
    // again, which no rule for elements below uses it of.
    assert_eq!(focus_visibly("div:focus-within { width: 1px }"), ["a", "b"]);
    // Across siblings: the next one, by its sibling invalidation set (not
    // the element's subtree: no rule uses its state of elements below).
    assert_eq!(focus_visibly(":focus + span { height: 1px }"), ["b", "c"]);
    // Without the system's focus no state changes, whatever the rules.
    let mut page =
        Page::new("<!DOCTYPE html><style>:focus { width: 1px }</style><div id=b tabindex=0></div>");
    let restyled = page.change(|cx, document| {
        focus(cx, document, Some(at(cx, document, "b")), false, true);
    });
    assert_eq!(restyled, Vec::<String>::new());
}

/// What the focus states match: `:focus` the focused element, `:focus-within`
/// it and its ancestors, `:focus-visible` it if shown.
#[test]
fn the_focus_pseudo_classes_match() {
    let mut page = Page::new(
        "<!DOCTYPE html><style>
           :focus { width: 1px }
           :focus-within { height: 2px }
           :focus-visible { width: 3px }
         </style><div id=a><div id=b></div></div>",
    );
    page.change(|cx, document| focus(cx, document, Some(at(cx, document, "b")), true, false));
    let styles = page.styles();
    let (a, b) = (&styles["a"], &styles["b"]);
    // `:focus`'s, not the later `:focus-visible`'s: not shown.
    assert_eq!(b.width, px(1.0));
    assert_eq!(b.height, px(2.0));
    assert_eq!(a.height, px(2.0));
    assert_ne!(a.width, px(1.0));
    page.change(|cx, document| focus(cx, document, Some(at(cx, document, "b")), true, true));
    assert_eq!(page.styles()["b"].width, px(3.0), "shown");
}

/// `:hover` and `:active` match the hover and active elements and their
/// ancestors; changing them styles again only if some rule uses the state,
/// the elements whose state changed that matching looked at it of (not
/// `#b1`: `div:hover` isn't a `<span>`'s), their subtrees only if a rule
/// for elements below uses it.
#[test]
fn hover_and_active_match_and_style_only_what_rules_use() {
    let mut page = Page::new(
        "<!DOCTYPE html><style>
           div:hover { width: 1px }
           span:active { height: 2px }
         </style><div id=a><div id=b><span id=b1></span></div><span id=c></span></div>",
    );
    let restyled = page.change(|cx, document| {
        cx.set_hover_element(document, Some(at(cx, document, "b1")));
    });
    assert_eq!(restyled, ["a", "b"]);
    let styles = page.styles();
    assert_eq!(styles["a"].width, px(1.0));
    assert_eq!(styles["b"].width, px(1.0));
    assert_ne!(styles["c"].width, px(1.0));
    page.change(|cx, document| cx.set_active_element(document, Some(at(cx, document, "b1"))));
    assert_eq!(page.styles()["b1"].height, px(2.0));
    assert_ne!(page.styles()["c"].height, px(2.0));
    page.change(|cx, document| cx.set_hover_element(document, None));
    assert_ne!(page.styles()["a"].width, px(1.0));

    drop(page);
    // No rule uses `:hover` (the UA stylesheet's `:link:active` uses
    // `:active`): nothing is styled again.
    let mut page = Page::new("<!DOCTYPE html><style>.x { width: 1px }</style><div id=a></div>");
    let restyled = page.change(|cx, document| {
        cx.set_hover_element(document, Some(at(cx, document, "a")));
    });
    assert_eq!(restyled, Vec::<String>::new());
}

/// `:read-write`, `:read-only`, `:required` and `:optional` match the
/// text controls' states (their `readonly` and `required`), and
/// `:read-write` an editable element by its `contenteditable` (styled
/// again with its subtree as the attribute changes); a state change styles
/// again only if a rule uses it.
#[test]
fn the_form_states_match_and_style_only_what_rules_use() {
    let mut page = Page::new(
        "<!DOCTYPE html><style>
           :read-write { width: 1px }
           input:read-only { height: 2px }
           :required { order: 3 }
           :optional { flex-grow: 4 }
         </style><input id=a readonly required><input id=b><div id=e><p id=e1></p></div>",
    );
    let restyled = page.change(|cx, document| {
        let a = at(cx, document, "a");
        cx.remove_attribute(a, "readonly");
        cx.remove_attribute(a, "required");
        let b = at(cx, document, "b");
        cx.set_attribute(b, "readonly", "");
        cx.set_attribute(b, "required", "");
    });
    assert_eq!(restyled, ["a", "b"]);
    let styles = page.styles();
    assert_eq!(styles["a"].width, px(1.0));
    assert_ne!(styles["a"].height, px(2.0));
    assert_eq!(styles["a"].flex_grow, 4.0);
    assert_ne!(styles["b"].width, px(1.0));
    assert_eq!(styles["b"].height, px(2.0), "not read-write is read-only");
    assert_eq!(styles["b"].order, 3);
    assert_ne!(styles["e1"].width, px(1.0));
    let restyled = change_attribute(&mut page, "e", "contenteditable", "");
    assert_eq!(restyled, ["e", "e1"]);
    assert_eq!(page.styles()["e1"].width, px(1.0), "editable");

    drop(page);
    // `:read-only` alone depends on whether it is read-write too.
    let mut page = Page::new(
        "<!DOCTYPE html><style>input:read-only { height: 2px }</style><input id=a readonly>",
    );
    assert_eq!(page.styles()["a"].height, px(2.0));
    let restyled = page.change_at("a", |cx, a| {
        cx.remove_attribute(a, "readonly");
    });
    assert_eq!(restyled, ["a"]);
    assert_ne!(page.styles()["a"].height, px(2.0));

    drop(page);
    // No rule uses them: nothing is styled again.
    let mut page = Page::new("<!DOCTYPE html><style>.x { width: 1px }</style><input id=a>");
    let restyled = change_attribute(&mut page, "a", "required", "");
    assert_eq!(restyled, Vec::<String>::new());
}

/// `:target` matches the document's target element, the element alone,
/// while it is in that document; changing it styles again the two
/// elements, only if a rule uses it (their subtrees only if a rule for
/// elements below does).
#[test]
fn target_matches_the_target_element() {
    let mut page = Page::new(
        "<!DOCTYPE html><style>:target { width: 1px }</style>
         <div id=a><span id=a1></span></div><div id=b></div>",
    );
    let restyled = page.change(|cx, document| {
        cx.set_target_element(document, Some(at(cx, document, "a")));
    });
    assert_eq!(restyled, ["a"]);
    assert_eq!(page.styles()["a"].width, px(1.0));
    assert_ne!(page.styles()["a1"].width, px(1.0));
    let restyled = page.change(|cx, document| {
        cx.set_target_element(document, Some(at(cx, document, "b")));
    });
    assert_eq!(restyled, ["a", "b"]);
    assert_eq!(page.styles()["b"].width, px(1.0));

    let target = style::SelectorQuery::parse(":target").unwrap();
    let b = page.root("b");
    page.with_document(|cx, _| {
        let other = cx.create_document(true);
        cx.adopt_node(b.get(cx.gc), other);
    });
    let matches = |page: &mut Page| page.with_document(|cx, _| target.matches(cx, b.get(cx.gc)));
    assert!(!matches(&mut page), "in another document");
    page.with_document(|cx, document| {
        let b = b.get(cx.gc);
        cx.adopt_node(b, document);
        cx.append_child(body(cx, document), b).unwrap();
    });
    assert!(matches(&mut page), "back in its document");

    drop(b);
    drop(page);
    let mut page = Page::new("<!DOCTYPE html><style>.x { width: 1px }</style><div id=a></div>");
    let restyled = page.change(|cx, document| {
        cx.set_target_element(document, Some(at(cx, document, "a")));
    });
    assert_eq!(restyled, Vec::<String>::new());
}

/// `:lang()` matches each element's language by extended filtering (RFC
/// 4647): a range matches its tag and longer ones, case aside, `*` any
/// subtag (first or later), a missing subtag skipped but not past a
/// singleton; one of a list; a string as an identifier. Not an unknown
/// language, even for `*`, nor a tag with a subtag over eight characters.
#[test]
fn lang_matches_the_language() {
    let mut page = Page::new(
        r#"<!DOCTYPE html><style>
           :lang(en) { width: 1px }
           :lang("*-DE") { height: 2px }
           :lang(fr, ja-JP) { order: 3 }
           :lang(de-DE) { flex-grow: 4 }
           :lang(zh-TW) { opacity: 0.5 }
           :lang("de-*-DE") { flex-shrink: 5 }
           :lang("*") { z-index: 6 }
         </style>
         <div id=a lang=EN-us><span id=a1></span></div>
         <div id=b lang=de-Latn-DE></div>
         <div id=c lang=ja-jp></div>
         <div id=d lang=zh-x-TW></div>
         <div id=e lang=en-toolongsubtag></div>
         <div id=f lang=""></div>
         <div id=g></div>"#,
    );
    let styles = page.styles();
    assert_eq!(styles["a"].width, px(1.0));
    assert_eq!(styles["a1"].width, px(1.0), "inherited");
    assert_eq!(styles["b"].height, px(2.0), "a wildcard");
    assert_eq!(styles["b"].flex_grow, 4.0, "skipping Latn");
    assert_eq!(styles["b"].flex_shrink, 5.0, "a later wildcard");
    assert_eq!(styles["a"].z_index, Some(6), "any language");
    assert_eq!(styles["c"].order, 3, "one of the list");
    assert_eq!(styles["d"].opacity, 1.0, "not past a singleton");
    assert_ne!(styles["e"].width, px(1.0), "not a well-formed tag");
    for id in ["f", "g"] {
        assert_ne!(styles[id].width, px(1.0), "#{id}: unknown");
        assert_ne!(styles[id].order, 3, "#{id}: unknown");
        assert_eq!(styles[id].z_index, None, "#{id}: unknown, even for *");
    }
    // The document's content language (its response's, or a `<meta>`'s):
    // everything styled again, what has no `lang` around of it.
    let restyled = page.change(|cx, document| cx.set_content_language(document, Some("fr")));
    assert_eq!(restyled.len(), styles.len(), "everything: {restyled:?}");
    assert_eq!(page.styles()["g"].order, 3, "the content language");
    assert_ne!(page.styles()["a"].order, 3);
}

/// A shadow tree's element with no `lang` of its own is of its host's
/// language (HTML's: the host's `lang` counts for the tree in it).
#[test]
fn a_shadow_trees_elements_are_of_their_hosts_language() {
    let mut page = Page::with_shadows(
        "<!DOCTYPE html><div id=host lang=fr></div>",
        &[(
            "host",
            "<style>:lang(fr) { order: 3 }</style><span id=in></span>",
        )],
    );
    assert_eq!(page.style("in").order, 3);
}

const COMPONENT: &str = "<!DOCTYPE html><div id=doc></div>
    <div id=host><i id=light slot=a></i></div>";

const SHADOW: &str = "<style id=sheet>.big { width: 50px } .big span { height: 5px }
        slot[name=a] { color: green } slot[name=b] { color: blue }</style>\
    <div id=s1><span id=s1a></span></div><div id=s2></div>\
    <slot id=slot-a name=a></slot><slot id=slot-b name=b></slot>";

/// A change in a shadow tree styles what it would in the document's.
#[test]
fn a_change_in_a_shadow_tree_styles_only_what_it_changed() {
    let mut page = Page::with_shadows(COMPONENT, &[("host", SHADOW)]);
    let collected = scopes_collected(&page);
    assert_eq!(change_class(&mut page, "s1", "big"), ["s1", "s1a"]);
    assert_eq!(change_class(&mut page, "doc", "big"), ["doc"]);
    assert_eq!(
        scopes_collected(&page),
        collected,
        "no tree scope was marked"
    );
}

/// How many times the engine has collected a tree scope's sheets.
fn scopes_collected(page: &Page) -> usize {
    page.engine.collected_tree_scopes_for_testing()
}

/// A shadow tree's `<style>` changed: that tree scope's sheets are
/// collected again (and no other's), and what the rules of its old text
/// and its new one may match in its tree is styled again (its `span`, its
/// slots), not its host (no `:host` rule); what is assigned to a slot
/// whose style comes out the same isn't (it inherits nothing new).
#[test]
fn a_shadow_trees_style_changed() {
    let mut page = Page::with_shadows(
        "<!DOCTYPE html><div id=doc></div>
         <div id=host><i id=light slot=a></i></div><div id=other></div>",
        &[
            ("host", SHADOW),
            ("other", "<style>p { color: red }</style><p id=o></p>"),
        ],
    );
    let collected = scopes_collected(&page);
    let restyled = page.change_at("sheet", |cx, sheet| {
        let text = cx.create_text(cx.document_of(sheet), "slot { color: red }");
        cx.append_child(sheet, text).unwrap();
    });
    assert_eq!(restyled, ["s1a", "slot-a", "slot-b"]);
    assert_eq!(
        scopes_collected(&page),
        collected + 1,
        "only #host's shadow root"
    );
    // A `<style>` that comes into the tree, and its `media`.
    let restyled = add_style_to(&mut page, Some("other"), "new-sheet", "p { color: green }");
    assert_eq!(restyled, ["new-sheet", "o"]);
    // The `<style>` itself isn't: no rule reads its `media`.
    let restyled = change_attribute(&mut page, "new-sheet", "media", "print");
    assert_eq!(restyled, ["o"]);
    assert_eq!(scopes_collected(&page), collected + 3);
}

/// A child's `slot` attribute, or a slot's `name`, changed: what moved
/// from one slot to another (or to none) is styled again, with its new
/// slot's style.
#[test]
fn what_moves_between_slots_is_styled_again() {
    let mut page = Page::with_shadows(COMPONENT, &[("host", SHADOW)]);
    assert_eq!(change_attribute(&mut page, "light", "slot", "b"), ["light"]);
    assert_eq!(page.styles()["light"].color, gfx::Color::rgb(0, 0, 255));
    let restyled = change_attribute(&mut page, "slot-b", "name", "c");
    assert_eq!(restyled, ["light", "slot-b"]);
    assert_eq!(page.styles()["light"].color, gfx::Color::BLACK);
}

/// A sheet a shadow root adopts changed: its tree scope's sheets are
/// collected again, and no other's; once it lets go of the sheet, a change
/// to it collects nothing.
#[test]
fn a_shadow_roots_adopted_sheet_changed() {
    let mut page = Page::with_shadows(COMPONENT, &[("host", SHADOW)]);
    let sheet = constructed(&mut page, &crate::sheet("span { width: 1px }"));
    page.with("host", |cx, host| {
        cx.set_adopted_style_sheet(cx.shadow_root(host).unwrap(), 0, sheet.get(cx.gc));
    });
    page.recalc();
    let collected = scopes_collected(&page);
    let restyled = page.change(|cx, _| {
        cx.replace_style_sheet_contents(sheet.get(cx.gc), crate::sheet("span { width: 2px }"));
    });
    assert_eq!(restyled, ["s1a"], "its `span`");
    assert_eq!(scopes_collected(&page), collected + 1, "not the document's");
    page.change_at("host", |cx, host| {
        cx.remove_last_adopted_style_sheet(cx.shadow_root(host).unwrap());
    });
    let collected = scopes_collected(&page);
    let restyled = page.change(|cx, _| {
        cx.replace_style_sheet_contents(sheet.get(cx.gc), crate::sheet("span { width: 3px }"));
    });
    assert!(restyled.is_empty());
    assert_eq!(scopes_collected(&page), collected, "no one adopts it");
}

/// Collecting every tree scope's sheets at each recalc gives the same
/// styles as collecting only those the DOM marked.
#[test]
fn collecting_every_tree_scope_gives_the_same_styles() {
    let mut page = Page::with_shadows(COMPONENT, &[("host", SHADOW)]);
    page.engine.set_collect_every_tree_scope_for_testing(true);
    let collected = scopes_collected(&page);
    let restyled = change_class(&mut page, "s1", "big");
    assert_eq!(restyled, ["s1", "s1a"], "the same sheets: nothing more");
    assert_eq!(scopes_collected(&page), collected + 2);
}

/// A `<style>` whose text comes in pieces between two styles is styled
/// with once its sheets are collected.
#[test]
fn a_style_is_parsed_when_its_sheets_are_collected() {
    let mut page =
        Page::new("<!DOCTYPE html><style id=s></style><style id=t></style><div id=d></div>");
    let restyled = page.change_at("s", |cx, style| {
        for rule in [
            "#d { width: 1px }",
            " #d { width: 2px }",
            " #d { height: 3px }",
        ] {
            let text = cx.create_text(cx.document_of(style), rule);
            cx.append_child(style, text).unwrap();
        }
    });
    assert_eq!(restyled, ["d"]);
    page.change_at("t", |cx, other| {
        let text = cx.create_text(cx.document_of(other), "#d { color: red }");
        cx.append_child(other, text).unwrap();
    });
}

/// A loaded `<link>`'s sheet no longer applies once it is `disabled`, or
/// an alternate stylesheet, and again once it isn't.
#[test]
fn a_loaded_links_sheet_follows_its_attributes() {
    let mut page =
        Page::new("<!DOCTYPE html><link rel=stylesheet href=a.css id=link><div id=d></div>");
    crate::give_link_sheet(&mut page, "link", "#d { width: 5px }");
    page.recalc();
    let width = |page: &mut Page| page.styles()["d"].width;
    assert_eq!(width(&mut page), px(5.0));
    for (name, value) in [("disabled", ""), ("rel", "alternate stylesheet")] {
        change_attribute(&mut page, "link", name, value);
        assert_ne!(width(&mut page), px(5.0), "{name}");
        page.change_at("link", |cx, link| {
            if name == "rel" {
                cx.set_attribute(link, name, "stylesheet");
            } else {
                cx.remove_attribute(link, name);
            }
        });
        assert_eq!(width(&mut page), px(5.0), "{name} undone");
    }
}

/// Structural selectors in a shadow tree: what came into it, at its root
/// too (which keeps its children's selector flags), styles its siblings
/// again.
#[test]
fn structural_selectors_in_a_shadow_tree() {
    let mut page = Page::with_shadows(
        "<!DOCTYPE html><div id=host></div>",
        &[(
            "host",
            "<style>:first-child { width: 10px } li:first-child { height: 3px }
               ul:nth-child(2) { height: 4px }</style>\
             <ul id=list><li id=one></li></ul><p id=last></p>",
        )],
    );
    assert_eq!(page.style("list").height, px(4.0));
    let restyled = page.change(|cx, document| {
        let list = at(cx, document, "list");
        let li = element(cx, document, "li", None);
        cx.insert(list, li, cx.first_child(list));
    });
    assert!(restyled.contains(&"one".to_owned()), "{restyled:?}");
    let restyled = page.change(|cx, document| {
        let root = cx.shadow_root(at(cx, document, "host")).unwrap();
        let p = element(cx, document, "p", None);
        cx.insert(root, p, cx.first_child(root));
    });
    assert!(restyled.contains(&"list".to_owned()), "{restyled:?}");
    assert_eq!(page.style("list").height, computed_style::Size::Auto);
}

/// The host's attributes change what its `:host()` rules match, and what
/// `:host()` in its shadow tree's selectors does: it and its shadow tree
/// are styled again.
#[test]
fn host_rules_follow_the_host() {
    let mut page = Page::with_shadows(
        "<!DOCTYPE html><div id=host><i id=light></i></div>",
        &[(
            "host",
            "<style>:host(.on) { color: green } :host(.on) p { width: 3px }
               slot[name=a]::slotted(*) { width: 5px }</style>\\
             <p id=inner></p><slot id=a name=a></slot><slot id=b></slot>",
        )],
    );
    let restyled = change_class(&mut page, "host", "on");
    assert_eq!(restyled, ["a", "b", "host", "inner", "light"]);
    // Into the slot whose `::slotted()` rule it matches.
    let restyled = change_attribute(&mut page, "light", "slot", "a");
    assert_eq!(restyled, ["light"]);
    assert_eq!(page.styles()["light"].width, px(5.0));
}

/// A state that a shadow tree's `:host()` or `::slotted()` rules use
/// styles again the host, or what is assigned to its slots, when it
/// changes there; elsewhere, nothing.
#[test]
fn host_and_slotted_rules_states_are_theirs() {
    let mut page = Page::with_shadows(
        "<!DOCTYPE html><div id=host><i id=light></i></div><p id=doc></p>",
        &[(
            "host",
            "<style>:host(:hover) { color: green } ::slotted(:focus) { color: blue }</style>\\
             <slot></slot>",
        )],
    );
    let hover = |page: &mut Page, id: &'static str| {
        page.change(|cx, document| cx.set_hover_element(document, Some(at(cx, document, id))))
    };
    let focus_on = |page: &mut Page, id: &'static str| {
        page.change(|cx, document| focus(cx, document, Some(at(cx, document, id)), true, false))
    };
    let restyled = hover(&mut page, "doc");
    assert!(restyled.is_empty(), "{restyled:?}");
    let restyled = hover(&mut page, "host");
    assert_eq!(restyled, ["host", "light"], "the host, and what inherits");
    let restyled = focus_on(&mut page, "doc");
    assert!(restyled.is_empty(), "{restyled:?}");
    let restyled = focus_on(&mut page, "light");
    assert_eq!(restyled, ["light"]);
}

/// A state that `:host()` uses in a rule for the shadow tree's elements
/// is the host's too: when it changes there, the host and its shadow tree
/// are styled again.
#[test]
fn a_host_state_in_the_shadow_trees_rules() {
    let mut page = Page::with_shadows(
        "<!DOCTYPE html><div id=host></div>",
        &[(
            "host",
            "<style>:host(:hover) p { color: green }</style><p id=inner></p>",
        )],
    );
    let restyled = page.change(|cx, document| {
        cx.set_hover_element(document, Some(at(cx, document, "host")));
    });
    assert_eq!(restyled, ["host", "inner"]);
}

/// A list box's focus styles its options again (the UA's `select:
/// -internal-list-box:focus option:checked` and the like), as an option's
/// own `:checked` styles it; a menu list's focus, or another element's,
/// styles nothing again (no other rule uses `:focus`).
#[test]
fn a_list_boxs_focus_styles_its_options_again() {
    let mut page = Page::new(
        "<!DOCTYPE html><select id=list multiple><option id=a>A</option><option id=b>B</option></select>
         <select id=menu><option id=c>C</option></select><div id=d tabindex=0><span id=s></span></div>",
    );
    let restyled = page.change_at("a", |cx, a| cx.set_option_selected(a, true));
    assert!(restyled.contains(&"a".to_owned()), "{restyled:?}");
    let focus_on = |page: &mut Page, id: &'static str| {
        page.change(|cx, document| focus(cx, document, Some(at(cx, document, id)), true, false))
    };
    let restyled = focus_on(&mut page, "list");
    assert!(restyled.contains(&"a".to_owned()), "{restyled:?}");
    // The list box's options again as it loses its focus; not the menu
    // list's.
    let restyled = focus_on(&mut page, "menu");
    assert!(restyled.contains(&"a".to_owned()), "{restyled:?}");
    assert!(
        !restyled.iter().any(|id| id == "menu" || id == "c"),
        "{restyled:?}"
    );
    assert_eq!(focus_on(&mut page, "d"), Vec::<String>::new());
}

/// An element styled again takes its children along only if what they
/// take of it changed: what they inherit, its `display`, or a value one
/// takes with `inherit`; or if rules for elements below use what changed.
mod for_children {
    use super::*;

    const TREE: &str = "<div id=a><div id=b><span id=c></span></div></div>";

    #[test]
    fn its_own_values_style_it_alone() {
        let mut page = Page::new(&format!(
            "<!DOCTYPE html><style>.wide {{ width: 5px }}</style>{TREE}"
        ));
        assert_eq!(change_class(&mut page, "a", "wide"), ["a"]);
    }

    #[test]
    fn what_they_inherit_styles_them_too() {
        let mut page = Page::new(&format!(
            "<!DOCTYPE html><style>.red {{ color: red }} #b {{ color: blue }}</style>{TREE}"
        ));
        // `#b` sets its own color: its child inherits what it had.
        assert_eq!(change_class(&mut page, "a", "red"), ["a", "b"]);
    }

    #[test]
    fn a_value_taken_with_inherit_styles_them_too() {
        let mut page = Page::new(&format!(
            "<!DOCTYPE html><style>.far {{ margin-left: 5px }} #b {{ margin-left: inherit }}</style>{TREE}"
        ));
        assert_eq!(change_class(&mut page, "a", "far"), ["a", "b"]);
    }

    #[test]
    fn display_contents_passes_a_display_on() {
        let mut page = Page::new(&format!(
            "<!DOCTYPE html><style>.flex {{ display: flex }} #b {{ display: contents }}</style>{TREE}"
        ));
        // `#c`'s box is in `#a`'s: blockified in a flex container.
        assert_eq!(change_class(&mut page, "a", "flex"), ["a", "b", "c"]);
        assert_eq!(page.styles()["c"].display, computed_style::Display::Block);
    }

    /// `display: none` coming or going styles the element and its
    /// children (what they inherit may change), not what is below them:
    /// their values are the same, and whether the animations below run is
    /// the whole subtree's to style again only once there are animations.
    #[test]
    fn display_none_coming_or_going_styles_the_subtree() {
        let mut page = Page::new(&format!(
            "<!DOCTYPE html><style>.hidden {{ display: none }}</style>{TREE}"
        ));
        assert_eq!(change_class(&mut page, "a", "hidden"), ["a", "b"]);
        assert_eq!(change_class(&mut page, "a", ""), ["a", "b"]);
    }

    #[test]
    fn another_root_font_size_styles_every_rem() {
        let mut page = Page::new(
            "<!DOCTYPE html><html id=r><style>.big { font-size: 20px }
               #b { font-size: 10px } #c { width: 2rem }</style>
             <body><div id=a><div id=b><span id=c></span></div></div></body></html>",
        );
        let restyled = change_class(&mut page, "r", "big");
        assert!(restyled.contains(&"c".to_owned()), "{restyled:?}");
        assert_eq!(page.styles()["c"].width, px(40.0));
    }

    /// An attribute rules use of an ancestor: what below it they match
    /// (its descendant invalidation set, Blink's), not the whole subtree.
    #[test]
    fn an_attribute_rules_below_use_styles_what_they_match_below() {
        let mut page = Page::new(&format!(
            "<!DOCTYPE html><style>[data-x] span {{ width: 1px }}</style>{TREE}"
        ));
        // One no rule uses: nothing.
        assert_eq!(
            change_attribute(&mut page, "a", "data-y", "1"),
            Vec::<String>::new()
        );
        assert_eq!(change_attribute(&mut page, "a", "data-x", "1"), ["a", "c"]);
    }

    /// A class rules use of an ancestor, as it comes and as it goes: what
    /// below it they match.
    #[test]
    fn a_class_rules_below_use_styles_what_they_match_below() {
        let mut page = Page::new(&format!(
            "<!DOCTYPE html><style>.big span {{ width: 1px }}</style>{TREE}"
        ));
        assert_eq!(change_class(&mut page, "a", "big"), ["a", "c"]);
        assert_eq!(change_class(&mut page, "a", "small"), ["a", "c"], "removed");
        // Classes no rule uses: nothing.
        assert_eq!(
            change_class(&mut page, "a", "small other"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn an_id_rules_below_use_styles_the_subtree_as_it_goes_too() {
        let mut page = Page::new(
            "<!DOCTYPE html><style>#x span { width: 1px }</style>
             <div id=a><div id=b><span id=c></span></div></div>",
        );
        let b = page.root("b");
        let set = |page: &mut Page, id: &'static str| {
            page.change(|cx, _| cx.set_attribute(b.get(cx.gc), "id", id))
        };
        let restyled = set(&mut page, "x");
        assert!(restyled.contains(&"c".to_owned()), "{restyled:?}");
        assert_eq!(page.styles()["c"].width, px(1.0));
        let restyled = set(&mut page, "z");
        assert!(restyled.contains(&"c".to_owned()), "{restyled:?}: x went");
        assert_ne!(page.styles()["c"].width, px(1.0));
    }

    #[test]
    fn a_class_in_its_own_is_styles_it_alone() {
        let mut page = Page::new(&format!(
            "<!DOCTYPE html><style>:is(.big, .wide) {{ width: 1px }}</style>{TREE}"
        ));
        assert_eq!(change_class(&mut page, "a", "big"), ["a"]);
    }

    #[test]
    fn a_state_only_a_shadow_trees_own_rules_use_styles_nothing() {
        let mut page = Page::with_shadows(
            "<!DOCTYPE html><div id=h></div>",
            &[(
                "h",
                "<style>.x:hover span { width: 1px }</style><div class=x><span id=s></span></div>",
            )],
        );
        let restyled = page.change(|cx, document| {
            cx.set_hover_element(document, Some(at(cx, document, "h")));
        });
        assert_eq!(restyled, Vec::<String>::new());
    }

    #[test]
    fn a_state_rules_below_use_styles_the_subtree() {
        let mut page = Page::new(&format!(
            "<!DOCTYPE html><style>div:hover span {{ width: 1px }}</style>{TREE}"
        ));
        let restyled = page.change(|cx, document| {
            cx.set_hover_element(document, Some(at(cx, document, "b")));
        });
        assert_eq!(restyled, ["a", "b", "c"]);
        assert_eq!(page.styles()["c"].width, px(1.0));
    }
}

/// Elements deep in two subtrees styled again in one recalc: the bloom
/// filter is brought to each one's ancestors as it is matched, with
/// nothing pushed for what was walked past, what the first one's pushed
/// popped for the second, and what the second's parent chain has pushed.
#[test]
fn the_bloom_filter_follows_what_is_styled_again() {
    let mut page = Page::new(
        "<!DOCTYPE html><style>.outer .mid .on { width: 5px } .other .on { height: 6px }</style>
         <div class=outer><div class=mid><div><span id=deep></span></div></div></div>
         <div class=other><div><div><span id=far></span></div></div></div>",
    );
    let restyled = page.change(|cx, document| {
        for id in ["deep", "far"] {
            cx.set_attribute(at(cx, document, id), "class", "on");
        }
    });
    assert_eq!(restyled, ["deep", "far"]);
    let styles = page.styles();
    assert_eq!(styles["deep"].width, px(5.0));
    assert_eq!(styles["far"].height, px(6.0));
}

/// Another document base URL (`pushState`, a `<base>`) styles nothing
/// again: what was parsed (a `<style>`, a `style` attribute, a
/// presentation attribute) keeps what its `url()`s resolved to, as in
/// Chrome, whose `Document::UpdateBaseURL` leaves style alone.
#[test]
fn another_base_url_styles_nothing_again() {
    let mut page = Page::new(
        "<!DOCTYPE html><style>#a { background-image: url(a.png) }</style>
         <div id=a></div><div id=b style='background-image: url(b.png)'></div>
         <svg><rect id=r fill='url(g.svg#g)'/></svg>",
    );
    let base = |path: &str| url::Url::parse(&format!("https://example.com/{path}")).unwrap();
    page.set_url_context(Some(css::UrlContext::of_sheet(base("one/"))));
    page.recalc();
    let before = page.styles();
    page.set_url_context(Some(css::UrlContext::of_sheet(base("two/"))));
    page.recalc();
    let after = page.styles();
    let restyled: Vec<&str> = after
        .iter()
        .filter(|(id, style)| !Arc::ptr_eq(&before[*id], style))
        .map(|(id, _)| id.as_str())
        .collect();
    assert!(restyled.is_empty(), "{restyled:?}");
}

/// The option the keys of a `multiple` list box moved to without
/// selecting it styles itself again as it becomes it, or stops being it
/// (`:-internal-multi-select-focus`, the UA's focus ring).
#[test]
fn a_multi_select_focus_styles_its_option_again() {
    let mut page = Page::new(
        "<!DOCTYPE html><select id=list multiple><option id=a>A</option><option id=b>B</option></select>",
    );
    let end = |page: &mut Page, non_contiguous: bool| {
        page.change(|cx, document| {
            let list = at(cx, document, "list");
            let mut state = cx.list_box_state(list);
            state.end = Some(cx.node_id(at(cx, document, "a")));
            state.non_contiguous = non_contiguous;
            cx.set_list_box_state(list, state);
        })
    };
    assert_eq!(end(&mut page, true), ["a"]);
    assert_eq!(end(&mut page, false), ["a"]);
}

/// `:open` matches a select whose dropdown is shown (its state), and an
/// open `<details>` and `<dialog>` (their attribute), not another element
/// with the attribute; what it and its children look like follows as it
/// changes.
#[test]
fn open_matches_and_styles_again() {
    let mut page = Page::new(
        "<!DOCTYPE html><style>:open { font-size: 7px } :open > span { font-size: 3px }</style>
         <select id=s><option id=o>A</option></select>
         <details id=d><summary id=su>S</summary><span id=d1></span></details>
         <dialog id=g><span id=g1></span></dialog><div id=v open><span id=v1></span></div>",
    );
    let size = |page: &mut Page, id: &str| page.styles()[id].font.size;
    assert_eq!(size(&mut page, "v"), 16.0, "no `open` of a div's");
    let restyled = page.change_at("s", |cx, s| cx.set_select_popup_open(s, true));
    assert!(restyled.contains(&"s".to_owned()), "{restyled:?}");
    assert_eq!(size(&mut page, "s"), 7.0);
    for (id, child) in [("d", "d1"), ("g", "g1")] {
        let restyled = change_attribute(&mut page, id, "open", "");
        assert!(restyled.contains(&id.to_owned()), "{restyled:?}");
        assert_eq!(size(&mut page, child), 3.0, "{child}");
        page.change_at(id, |cx, element| {
            cx.remove_attribute(element, "open");
        });
        assert_eq!(size(&mut page, id), 16.0, "{id} closed");
    }
}

/// A `<details>` opening or closing styles again what rules below it use
/// its `:open` for, though it doesn't change itself: its attribute is a
/// feature of those rules, as `[open]` would be.
#[test]
fn opening_a_details_styles_what_rules_below_use_it_for() {
    let mut page = Page::new(
        "<!DOCTYPE html><style>:open > summary { font-size: 3px }</style>
         <details id=d><summary id=su>S</summary></details>",
    );
    let restyled = change_attribute(&mut page, "d", "open", "");
    assert_eq!(restyled, ["d", "su"]);
    assert_eq!(page.styles()["su"].font.size, 3.0);
}

/// A change among an element's children styles again only the children
/// what matching found depend on it, as its selector flags say.
mod siblings {
    use super::*;

    const LIST: &str = "<div id=list><p id=a></p><p id=b></p><p id=c></p><p id=d></p></div>";

    fn page(css: &str) -> Page {
        Page::new(&format!(
            "<!DOCTYPE html><style>{css}</style>{LIST}<div id=other><p id=o></p></div>"
        ))
    }

    fn insert_before(page: &mut Page, before: Option<&'static str>) -> Vec<String> {
        page.change(|cx, document| {
            let new = element(cx, document, "p", Some("new"));
            let before = before.map(|id| at(cx, document, id));
            cx.insert(at(cx, document, "list"), new, before);
        })
    }

    fn remove(page: &mut Page, id: &str) -> Vec<String> {
        page.change_at(id, |cx, node| cx.remove(node))
    }

    /// Text appended to the element with id `id`.
    fn append_text(page: &mut Page, id: &str) -> Vec<String> {
        page.change_at(id, |cx, element| {
            let text = cx.create_text(cx.document_of(element), "t");
            cx.append_child(element, text).unwrap();
        })
    }

    #[test]
    fn last_child_styles_the_first_and_the_last_around_the_change() {
        let mut page = page("p:last-child { width: 1px }");
        assert_eq!(insert_before(&mut page, None), ["a", "d", "new"]);
    }

    /// Text between them (whitespace in the markup): the elements around
    /// the change are the ones flagged.
    #[test]
    fn the_elements_around_the_change_are_flagged_past_text() {
        let mut page = Page::new(
            "<!DOCTYPE html><style>p:last-child { width: 1px }</style>\
             <div id=list> <p id=a></p> <p id=b></p> </div>",
        );
        assert_eq!(insert_before(&mut page, None), ["a", "b", "new"]);
        assert_eq!(page.styles()["b"].width, computed_style::Size::Auto);
    }

    /// A position its pseudo-element's rule takes: flagged as that rule is
    /// matched.
    #[test]
    fn a_pseudo_elements_position_styles_it_too() {
        let mut page = page("p:last-child::after { content: 'x' }");
        let restyled = insert_before(&mut page, None);
        assert!(restyled.contains(&"d".to_owned()), "{restyled:?}");
    }

    #[test]
    fn adjacent_styles_the_next_one() {
        let mut page = page(".x + p { width: 1px }");
        assert_eq!(insert_before(&mut page, Some("b")), ["b", "new"]);
    }

    #[test]
    fn two_adjacent_style_the_next_two() {
        let mut page = page(".x + p + p { width: 1px }");
        assert_eq!(insert_before(&mut page, Some("b")), ["b", "c", "new"]);
    }

    #[test]
    fn later_siblings_style_every_one_after() {
        let mut page = page(".x ~ p { width: 1px }");
        assert_eq!(insert_before(&mut page, Some("b")), ["b", "c", "d", "new"]);
        assert_eq!(remove(&mut page, "c"), ["d"]);
    }

    /// Nested in a list, positions and sibling combinators alike: those
    /// after the change.
    #[test]
    fn a_nested_position_styles_every_one_after() {
        for css in [
            "p:not(:nth-child(2n)) { width: 1px }",
            ":is(#new + p) { width: 1px }",
        ] {
            let mut page = page(css);
            assert_eq!(
                insert_before(&mut page, Some("b")),
                ["b", "c", "d", "new"],
                "{css}"
            );
        }
    }

    /// A host's position its shadow tree's rules take (`:host(…)`): how
    /// far they reach counts too, though its parent's tree has no rules.
    #[test]
    fn a_hosts_position_its_shadow_tree_takes() {
        let mut page = Page::with_shadows(
            &format!("<!DOCTYPE html>{LIST}"),
            &[("c", "<style>:host(:nth-child(3)) { width: 1px }</style>")],
        );
        let restyled = insert_before(&mut page, Some("b"));
        assert!(restyled.contains(&"c".to_owned()), "{restyled:?}");
    }

    #[test]
    fn a_hosts_emptiness_its_shadow_tree_takes() {
        let mut page = Page::with_shadows(
            "<!DOCTYPE html><div id=e></div>",
            &[("e", "<style>:host(:empty) { width: 1px }</style>")],
        );
        let restyled = append_text(&mut page, "e");
        assert!(restyled.contains(&"e".to_owned()), "{restyled:?}");
    }

    /// A position left of `+`: the sibling after the one whose position
    /// changed, wherever that is after the change.
    #[test]
    fn a_position_left_of_adjacent_styles_every_one_after() {
        for css in [
            ":first-child + p { width: 1px }",
            ":not(:first-child) + p { width: 1px }",
        ] {
            let mut page = page(css);
            assert_eq!(
                insert_before(&mut page, Some("a")),
                ["a", "b", "c", "d", "new"],
                "{css}"
            );
        }
    }

    #[test]
    fn a_position_from_the_end_styles_them_all() {
        let mut page = page("p:nth-last-child(2) { width: 1px }");
        assert_eq!(
            insert_before(&mut page, Some("d")),
            ["a", "b", "c", "d", "new"]
        );
    }

    #[test]
    fn empty_styles_the_parent() {
        let mut page =
            Page::new("<!DOCTYPE html><style>div:empty { width: 1px }</style><div id=e></div>");
        assert_eq!(append_text(&mut page, "e"), ["e"]);
    }

    /// (`selectors` flags the parent as it tests the rightmost compound's
    /// position, before the rest: `#other > p:first-child` flags it too.)
    #[test]
    fn a_parent_none_of_whose_children_a_rule_looks_at_styles_none() {
        let mut page = page("span:first-child { width: 1px }");
        assert_eq!(insert_before(&mut page, Some("b")), ["new"]);
    }
}

/// A `wheel` listener coming or going styles its element again, and what is
/// below it only where whether it is inside a blocking one changed
/// (`inside_blocking_wheel_event_handler`, inherited): a passive one, or
/// one below an element inside one already, changes nothing else. The root
/// element counts its document's.
#[test]
fn a_blocking_wheel_listener_styles_what_it_is_over() {
    use dom::names::event;
    use dom::{ListenerFlags, TargetRef};
    let mut page = Page::new(LIST);
    let blocking = ListenerFlags::default();
    let passive = ListenerFlags {
        passive: true,
        ..ListenerFlags::default()
    };
    let add = |page: &mut Page, id: &'static str, name, flags| {
        page.change_with(|cx, document| {
            let target = TargetRef::Node(at(cx, document, id));
            elise::Persistent::new(cx.gc, cx.add_listener(target, name, flags))
        })
    };
    add(&mut page, "a", event::WHEEL, passive);
    assert!(inside_blocking_wheel(&mut page).is_empty(), "a passive one");
    let (on_a, restyled) = add(&mut page, "a", event::MOUSEWHEEL, blocking);
    assert_eq!(restyled, ["a", "a1"]);
    assert_eq!(inside_blocking_wheel(&mut page), ["a", "a1"]);
    let (_, restyled) = add(&mut page, "a1", event::WHEEL, blocking);
    assert!(!restyled.contains(&"a".to_owned()), "{restyled:?}");
    page.change_at("a", |cx, a| {
        cx.remove_listener(TargetRef::Node(a), on_a.get(cx.gc));
    });
    assert_eq!(inside_blocking_wheel(&mut page), ["a1"]);

    // A listener taken off by its handle, and an event handler, come and
    // go too.
    let (on_b, _) = add(&mut page, "b", event::WHEEL, blocking);
    assert_eq!(inside_blocking_wheel(&mut page), ["a1", "b", "b1"]);
    page.change_at("b", |cx, b| {
        cx.remove_listener(TargetRef::Node(b), on_b.get(cx.gc));
    });
    assert_eq!(inside_blocking_wheel(&mut page), ["a1"]);
    page.change_at("b", |cx, b| {
        cx.set_compiled_handler(TargetRef::Node(b), event::WHEEL);
    });
    assert_eq!(inside_blocking_wheel(&mut page), ["a1", "b", "b1"]);
    page.change_at("b", |cx, b| {
        cx.remove_event_handler(TargetRef::Node(b), event::WHEEL);
    });
    assert_eq!(inside_blocking_wheel(&mut page), ["a1"]);

    // The root element counts its document's and window's.
    let (on_document, restyled) = page.change_with(|cx, document| {
        let listener = cx.add_listener(TargetRef::Node(document), event::WHEEL, blocking);
        elise::Persistent::new(cx.gc, listener)
    });
    assert!(restyled.contains(&"b1".to_owned()), "{restyled:?}");
    assert_eq!(inside_blocking_wheel(&mut page).len(), 6);
    page.change(|cx, document| {
        cx.remove_listener(TargetRef::Node(document), on_document.get(cx.gc));
    });
    assert_eq!(inside_blocking_wheel(&mut page), ["a1"]);
    page.change(|cx, document| {
        let window = cx.create_window(document);
        cx.add_listener(TargetRef::Window(window), event::WHEEL, blocking);
    });
    assert_eq!(inside_blocking_wheel(&mut page).len(), 6);
}

/// A shadow root's blocking `wheel` listener is over its tree: its elements
/// at the top take it, and what is below them inherits it; a host's is over
/// its shadow tree too, as the flat tree goes.
#[test]
fn a_shadow_roots_wheel_listener_is_over_its_tree() {
    use dom::names::event;
    use dom::{ListenerFlags, TargetRef};
    let mut page = Page::with_shadows(
        "<div id=host><i id=slotted></i></div><div id=other></div>",
        &[("host", "<p id=top><span id=deep></span><slot></slot></p>")],
    );
    let (on_root, _) = page.change_with(|cx, document| {
        let root = cx.shadow_root(at(cx, document, "host")).unwrap();
        let listener = cx.add_listener(
            TargetRef::Node(root),
            event::WHEEL,
            ListenerFlags::default(),
        );
        elise::Persistent::new(cx.gc, listener)
    });
    assert_eq!(inside_blocking_wheel(&mut page), ["deep", "slotted", "top"]);
    page.change(|cx, document| {
        let root = cx.shadow_root(at(cx, document, "host")).unwrap();
        cx.remove_listener(TargetRef::Node(root), on_root.get(cx.gc));
    });
    assert!(inside_blocking_wheel(&mut page).is_empty());
    page.change_at("host", |cx, host| {
        cx.add_listener(
            TargetRef::Node(host),
            event::WHEEL,
            ListenerFlags::default(),
        );
    });
    assert_eq!(
        inside_blocking_wheel(&mut page),
        ["deep", "host", "slotted", "top"]
    );
}

/// The document's rules match its own tree: a host's children, which its
/// slots show, are styled again; its shadow tree's elements aren't, though
/// they have the feature.
#[test]
fn the_document_s_rules_find_its_own_tree() {
    let mut page = Page::with_shadows(
        "<!DOCTYPE html><body><div id=host><i id=light class=a></i></div>",
        &[("host", "<slot></slot><i id=inner class=a></i>")],
    );
    assert_eq!(
        add_sheet(&mut page, "s", ".a { width: 1px }"),
        ["light", "s"]
    );
}

/// A shadow tree's `:host` rule that comes styles its host again, and
/// nothing else; a `::slotted()` rule, what is assigned to its slots (and
/// a slot assigned to one, with what is assigned to it).
#[test]
fn host_and_slotted_rules_style_the_host_and_what_is_assigned() {
    let mut page = Page::with_shadows(
        "<!DOCTYPE html><div id=host><i id=light slot=a><b id=deep></b></i><i id=other></i></div>",
        &[("host", SHADOW)],
    );
    // Matching nothing, so that the host's style is the same: a host
    // whose style changed styles its children again.
    assert_eq!(
        add_style_to(&mut page, Some("host"), "h", ":host(.nope) { width: 1px }"),
        ["h", "host"]
    );
    assert_eq!(
        add_style_to(
            &mut page,
            Some("host"),
            "sl",
            "::slotted(i) { height: 2px }"
        ),
        ["light", "sl"]
    );
    assert_eq!(page.styles()["light"].height, px(2.0));
}

/// A constructed sheet edited through CSSOM (`insertRule`, `deleteRule`)
/// keeps the rules it didn't touch: what the rule it added or removed may
/// match is styled again, in each tree scope that adopts it (the
/// document's and a shadow root's).
#[test]
fn an_edited_sheet_styles_what_its_edit_may_match() {
    let mut page = Page::with_shadows(
        "<!DOCTYPE html><body><p id=p></p><div id=d></div><span id=host></span>",
        &[("host", "<p id=sp></p><div id=sd></div><i id=si></i>")],
    );
    let first = crate::sheet("p { width: 1px }");
    let sheet = constructed(&mut page, &first);
    page.with("host", |cx, host| {
        let document = cx.document_of(host);
        cx.set_adopted_style_sheet(document, 0, sheet.get(cx.gc));
        cx.set_adopted_style_sheet(cx.shadow_root(host).unwrap(), 0, sheet.get(cx.gc));
    });
    page.recalc();
    let inserted = Arc::new(first.insert_rule("div { width: 2px }", 1).unwrap());
    let restyled = page.change(|cx, _| {
        cx.replace_style_sheet_contents(sheet.get(cx.gc), inserted.clone());
    });
    assert_eq!(restyled, ["d", "sd"]);
    let deleted = Arc::new(inserted.delete_rule(0).unwrap());
    let restyled = page.change(|cx, _| {
        cx.replace_style_sheet_contents(sheet.get(cx.gc), deleted.clone());
    });
    assert_eq!(restyled, ["p", "sp"]);
    assert_eq!(page.styles()["sp"].width, computed_style::Size::Auto);
    assert_eq!(page.styles()["sd"].width, px(2.0));
}

/// A shadow tree's sheets kept in another order: each of its elements is
/// styled again.
#[test]
fn a_shadow_trees_sheets_in_another_order_style_all_of_it() {
    let mut page = Page::with_shadows(
        "<!DOCTYPE html><div id=host></div>",
        &[(
            "host",
            "<style id=x1>p { width: 1px }</style><style id=x2>p { width: 2px }</style>\
             <p id=sp></p><div id=sd></div>",
        )],
    );
    let restyled = page.change(|cx, document| {
        let root = cx.shadow_root(at(cx, document, "host")).unwrap();
        cx.insert(root, at(cx, document, "x2"), Some(at(cx, document, "x1")));
    });
    assert_eq!(restyled, ["host", "sd", "sp", "x1", "x2"]);
    assert_eq!(page.styles()["sp"].width, px(1.0));
}

/// A `<style>` whose sheet is disabled (`HTMLStyleElement.disabled`) isn't
/// applied, and what its rules matched is styled again each way. A new
/// sheet isn't disabled: its text changed, or it was inserted again.
#[test]
fn a_disabled_style_elements_sheet_isnt_applied() {
    let mut page = Page::new("<!DOCTYPE html><p id=one></p><div id=two></div>");
    assert_eq!(add_sheet(&mut page, "s", "p { width: 1px }"), ["one", "s"]);
    let disabled = |page: &mut Page, disabled: bool| {
        page.change_at("s", |cx, style| {
            cx.set_style_sheet_disabled(cx.owner_sheet(style).expect("a sheet"), disabled);
        })
    };
    let is_disabled = |page: &mut Page| {
        page.with("s", |cx, style| {
            cx.style_sheet_disabled(cx.owner_sheet(style).expect("a sheet"))
        })
    };
    assert_eq!(disabled(&mut page, true), ["one"]);
    assert_eq!(page.styles()["one"].width, computed_style::Size::Auto);
    assert_eq!(disabled(&mut page, true), Vec::<String>::new());
    assert_eq!(disabled(&mut page, false), ["one"]);
    assert_eq!(page.styles()["one"].width, px(1.0));
    // Its text: a new sheet.
    disabled(&mut page, true);
    let restyled = page.change_at("s", |cx, style| {
        let text = cx.create_text(cx.document_of(style), " div { width: 2px }");
        cx.append_child(style, text).unwrap();
    });
    assert_eq!(restyled, ["one", "two"]);
    assert!(!is_disabled(&mut page));
    assert_eq!(page.styles()["two"].width, px(2.0));
    // Taken out and put back: a new sheet.
    disabled(&mut page, true);
    page.change_at("s", |cx, style| {
        let body = cx.parent(style).expect("in <body>");
        cx.remove(style);
        cx.append_child(body, style).unwrap();
    });
    assert!(!is_disabled(&mut page));
    assert_eq!(page.styles()["one"].width, px(1.0));
}

/// `count` rules of their own classes, none of which the page has: a
/// sheet large enough to be a set of its own.
fn large_sheet(count: usize) -> String {
    (0..count)
        .map(|i| format!(".large{i} {{ width: 1px }}\n"))
        .collect()
}

/// How many style rules were put in the document's sets, and how many
/// sets were made, in styling `page` again after the text of the
/// `<style>` with id `id` became `css`.
fn sets_made(page: &mut Page, id: &str, css: &str) -> (usize, usize) {
    page.with(id, |cx, style| {
        cx.set_data(cx.first_child(style).expect("a text child"), css);
    });
    indexed_in_recalc(page)
}

/// How many style rules were put in the document's sets, and how many
/// sets were made, in styling `page` again.
fn indexed_in_recalc(page: &mut Page) -> (usize, usize) {
    let before = page.engine.document_rules_indexed_for_testing();
    page.recalc();
    let after = page.engine.document_rules_indexed_for_testing();
    (after.0 - before.0, after.1 - before.1)
}

/// A large sheet's rules are a set of their own, which a change of
/// another sheet leaves alone; the small ones between two large ones are
/// one set, made again (all of it) when one of them changes, and the sets
/// of the other runs are taken again. The sets cascade in the sheets'
/// order.
#[test]
fn a_large_sheet_is_a_set_of_its_own() {
    let html = format!(
        "<!DOCTYPE html><style id=s1>p {{ height: 1px }}</style>\
         <style id=s2>p {{ height: 2px }} #two {{ width: 3px }}</style>\
         <style id=big>{} p {{ height: 3px }} .large5 {{ height: 9px }}</style>\
         <style id=s3>div {{ height: 4px }}</style>\
         <p id=one></p><div id=two class=large5></div>",
        large_sheet(600)
    );
    let mut page = Page::new(&html);
    assert_eq!(page.engine.document_sets_for_testing(), 3);
    let styles = page.styles();
    assert_eq!(styles["one"].height, px(3.0), "the large sheet's, later");
    assert_eq!(styles["two"].height, px(9.0), "a class over a name");
    assert_eq!(styles["two"].width, px(3.0), "an id over a class");
    let made = sets_made(&mut page, "s3", "div { height: 5px } p { width: 6px }");
    assert_eq!(made, (2, 1), "the last run's set only");
    let made = sets_made(&mut page, "s1", "p { height: 7px }");
    assert_eq!(made, (3, 1), "the first run's: both its sheets'");
    let made = sets_made(&mut page, "big", &large_sheet(700));
    assert_eq!(made, (700, 1), "the large sheet's, the runs' taken again");
    let styles = page.styles();
    assert_eq!(
        styles["one"].height,
        px(2.0),
        "the second sheet's, now last"
    );
    assert_eq!(styles["one"].width, px(6.0));
    // What styling everything again gives.
    page.change(|_, _| {});
}

/// However many small sheets there are, next to each other they are one
/// set: an element looks in its buckets once.
#[test]
fn small_sheets_are_one_set() {
    let sheets: String = (0..300)
        .map(|i| format!("<style>.c{i} {{ width: {i}px }}</style>"))
        .collect();
    let mut page = Page::new(&format!("<!DOCTYPE html>{sheets}<p id=p class=c42></p>"));
    assert_eq!(page.engine.document_sets_for_testing(), 1);
    assert_eq!(page.styles()["p"].width, px(42.0));
}

/// A large sheet's `media` decides whether its set cascades: when the
/// viewport makes it come out otherwise, the set is taken (or left out)
/// without being made again.
#[test]
fn a_large_sheets_media_takes_its_set_or_leaves_it_out() {
    let html = format!(
        "<!DOCTYPE html><style media=\"(min-width: 500px)\">{} p {{ width: 5px }}</style>\
         <style>p {{ height: 1px }}</style><p id=p></p>",
        large_sheet(600)
    );
    let mut page = Page::new(&html);
    // Styled first with no viewport: it doesn't apply.
    assert_eq!(page.styles()["p"].width, computed_style::Size::Auto);
    assert_eq!(page.engine.document_sets_for_testing(), 1);
    assert!(page.set_viewport(gfx::SizeF::new(600.0, 300.0), 1.0));
    let made = indexed_in_recalc(&mut page);
    assert_eq!(page.styles()["p"].width, px(5.0));
    assert_eq!(page.engine.document_sets_for_testing(), 2);
    assert_eq!(
        made,
        (601, 1),
        "its set made the first time it applies, the run's taken again"
    );
    assert!(page.set_viewport(gfx::SizeF::new(400.0, 300.0), 1.0));
    let made = indexed_in_recalc(&mut page);
    assert_eq!(page.styles()["p"].width, computed_style::Size::Auto);
    assert_eq!(page.engine.document_sets_for_testing(), 1);
    assert_eq!(made, (0, 0));
    assert!(page.set_viewport(gfx::SizeF::new(600.0, 300.0), 1.0));
    let made = indexed_in_recalc(&mut page);
    assert_eq!(page.styles()["p"].width, px(5.0));
    assert_eq!(made, (0, 0), "its set and features taken again");
}

/// What a change of an element's class styles again below it comes from
/// the rules of every set, each set's features merged.
#[test]
fn every_sets_rules_count_for_invalidation() {
    let html = format!(
        "<!DOCTYPE html><style>.a .x {{ width: 1px }}</style>\
         <style>{} .b .x {{ width: 2px }}</style>\
         <style>.c .x {{ width: 3px }}</style>\
         <div id=p><span id=x class=x></span><span id=y></span></div>",
        large_sheet(600)
    );
    let mut page = Page::new(&html);
    // How many sets the sheets make is `a_large_sheet_is_a_set_of_its_own`'s;
    // whatever they make, every rule counts.
    for (class, width) in [("a", 1.0), ("b", 2.0), ("c", 3.0)] {
        let restyled = change_class(&mut page, "p", class);
        assert_eq!(restyled, ["p", "x"], "{class}");
        assert_eq!(page.styles()["x"].width, px(width), "{class}");
    }
}

/// An edit of a `<style>`'s sheet through script's object for it styles
/// what the rules it added or took may match, as a change of its text
/// does; its media and `disabled`, what all of its rules match.
#[test]
fn an_edit_through_a_style_elements_sheet_styles_what_it_may_match() {
    let mut page = Page::new(
        "<!DOCTYPE html><style id=sheet>p { width: 1px }</style>\
         <p id=one></p><div id=two></div><span id=three></span>",
    );
    let sheet = page.with("sheet", |cx, style| {
        elise::Persistent::new(cx.gc, cx.owner_sheet(style).expect("a sheet"))
    });
    let restyled = page.change(|cx, _| {
        let sheet = sheet.get(cx.gc);
        let inserted = cx
            .style_sheet_contents(sheet)
            .insert_rule("div { width: 2px }", 1)
            .unwrap();
        cx.css_rule_inserted(
            dom::RuleParent::Sheet(sheet),
            dom::Rules::Sheet(inserted),
            1,
        );
    });
    assert_eq!(restyled, ["two"]);
    assert_eq!(page.styles()["two"].width, px(2.0));
    let media = |page: &mut Page, text: &'static str| {
        page.change(|cx, _| {
            let list = Arc::new(css::media::MediaQueryList::parse(text));
            cx.set_style_sheet_media(sheet.get(cx.gc), text, list);
        })
    };
    assert_eq!(media(&mut page, "print"), ["one", "two"]);
    assert_eq!(page.styles()["one"].width, computed_style::Size::Auto);
    media(&mut page, "");
    let restyled = page.change(|cx, _| cx.set_style_sheet_disabled(sheet.get(cx.gc), true));
    assert_eq!(restyled, ["one", "two"]);
    assert_eq!(page.styles()["two"].width, computed_style::Size::Auto);
}

/// The preferred style sheet set (HTML's, as Chrome 152 has it): the title
/// of the first sheet with one that isn't an alternate `<link>`'s, which
/// stays the document's. A sheet with another title doesn't apply, nor an
/// alternate one without a title; an alternate one with the preferred
/// set's does. A shadow tree's titles don't count.
#[test]
fn only_the_preferred_sets_sheets_apply() {
    let mut page = Page::with_shadows(
        "<!DOCTYPE html><style>#p1 { width: 1px }</style>\
         <link id=early rel='alternate stylesheet' title=second href=e.css>\
         <style id=first title=first>#p2 { width: 2px }</style>\
         <style id=second title=second>#p3 { width: 3px }</style>\
         <style title=first>#p4 { width: 4px }</style>\
         <link id=alt rel='alternate stylesheet' title=first href=a.css>\
         <link id=untitled rel='alternate stylesheet' href=b.css>\
         <p id=p1></p><p id=p2></p><p id=p3></p><p id=p4></p><p id=p5></p><p id=p6></p>\
         <p id=p7></p><div id=host></div>",
        &[(
            "host",
            "<style title=zzz>#s { width: 8px }</style><span id=s></span>",
        )],
    );
    for (id, css) in [
        ("early", "#p7 { width: 7px }"),
        ("alt", "#p5 { width: 5px }"),
        ("untitled", "#p6 { width: 6px }"),
    ] {
        page.change_at(id, |cx, link| {
            cx.set_link_sheet(link, crate::sheet(css), "a.css", true);
        });
    }
    let widths = |page: &mut Page| -> Vec<String> {
        let styles = page.styles();
        ["p1", "p2", "p3", "p4", "p5", "p6", "p7", "s"]
            .iter()
            .map(|id| format!("{:?}", styles[*id].width))
            .collect()
    };
    let auto = format!("{:?}", computed_style::Size::Auto);
    let applied = |widths: &[Option<f32>]| -> Vec<String> {
        widths
            .iter()
            .map(|width| width.map_or(auto.clone(), |width| format!("{:?}", px(width))))
            .collect()
    };
    let expected = applied(&[
        Some(1.0),
        Some(2.0),
        None,
        Some(4.0),
        Some(5.0),
        None,
        None,
        Some(8.0),
    ]);
    assert_eq!(widths(&mut page), expected);
    // The first titled sheet gone, the set stays: its sheets still apply,
    // the second's still don't.
    // (Styled again without `change`'s check: a new engine's set would be
    // the one the document has now.)
    page.with("first", |cx, first| cx.remove(first));
    page.recalc();
    page.with("second", |cx, second| {
        cx.set_data(cx.first_child(second).unwrap(), "#p3 { width: 30px }");
    });
    page.recalc();
    let expected = applied(&[
        Some(1.0),
        None,
        None,
        Some(4.0),
        Some(5.0),
        None,
        None,
        Some(8.0),
    ]);
    assert_eq!(widths(&mut page), expected);
}

/// What the selectors of every set depend on counts, each set's merged: a
/// child coming before the first one styles its siblings again for a
/// `:first-child` rule of a set after a large sheet's.
#[test]
fn every_sets_selectors_count_for_what_changes_style() {
    let html = format!(
        "<!DOCTYPE html><style>{}</style><style>li:first-child {{ width: 10px }}</style>\
         <ul id=list><li id=one></li><li id=two></li></ul>",
        large_sheet(600)
    );
    let mut page = Page::new(&html);
    // How many sets the sheets make is `a_large_sheet_is_a_set_of_its_own`'s;
    // whatever they make, every rule counts.
    let restyled = page.change(|cx, document| {
        let first = element(cx, document, "li", Some("zero"));
        cx.insert(
            at(cx, document, "list"),
            first,
            Some(at(cx, document, "one")),
        );
    });
    assert_eq!(restyled, ["one", "two", "zero"]);
    assert_eq!(page.styles()["one"].width, computed_style::Size::Auto);
}

/// A run of small sheets is made again when a media query of a rule in it
/// comes out otherwise, not taken again.
#[test]
fn a_runs_media_rules_follow_the_viewport() {
    let html = format!(
        "<!DOCTYPE html><style>{}</style>\
         <style>@media (min-width: 500px) {{ p {{ height: 5px }} }}</style><p id=p></p>",
        large_sheet(600)
    );
    let mut page = Page::new(&html);
    assert_eq!(page.styles()["p"].height, computed_style::Size::Auto);
    assert!(page.set_viewport(gfx::SizeF::new(600.0, 300.0), 1.0));
    let made = indexed_in_recalc(&mut page);
    assert_eq!(page.styles()["p"].height, px(5.0));
    assert_eq!(
        made.0, 1,
        "the run's set made again, the large sheet's kept"
    );
}
