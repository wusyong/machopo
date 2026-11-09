//! The matched properties cache: an element whose declarations, parent's
//! inherited values and environment are those of one cascaded before
//! takes that cascade, adjusted for where it is, shared: the same style
//! object. (In a debug build every cascade the cache gives is also worked
//! out again and compared, in every test.)

use std::sync::Arc;

use computed_style::ComputedStyle;
use dom::QualName;
use gfx::SizeF;

use crate::Page;

/// How many cascades the cache keeps, and has given so far.
fn cache(page: &Page) -> (usize, usize) {
    page.engine.matched_properties_for_testing()
}

/// Appends a `name` element with `attributes` to the element with id
/// `parent`, styles the page, and returns how many cascades the cache
/// gave for it.
fn append(page: &mut Page, parent: &str, name: &str, attributes: &[(&str, &str)]) -> usize {
    let before = cache(page).1;
    page.with(parent, |cx, parent| {
        let element = cx.create_element(cx.document_of(parent), QualName::html(name));
        for (name, value) in attributes {
            cx.set_attribute(element, name, value);
        }
        cx.append_child(parent, element).unwrap();
    });
    page.recalc();
    cache(page).1 - before
}

/// An element whose rules, parent and environment are another's takes the
/// other's cascade. A cascade is kept once it is looked for again: one of
/// an element alike no other isn't.
#[test]
fn an_element_like_one_styled_takes_its_cascade() {
    let mut page = Page::new(
        "<style>.a { color: red; padding: 2px }</style>
         <div id=box><div class=a id=first></div></div>",
    );
    let kept = cache(&page).0;
    assert_eq!(
        append(&mut page, "box", "div", &[("class", "a"), ("id", "second")]),
        0
    );
    assert_eq!(cache(&page).0, kept + 1, "looked for again: kept");
    assert_eq!(
        append(&mut page, "box", "div", &[("class", "a"), ("id", "third")]),
        1
    );
    assert_eq!(*page.style("third"), *page.style("first"));
}

/// A parent with other inherited values gives another cascade (`em`s,
/// `currentcolor` and the inherited values themselves come of them), which
/// is kept in place of the first; one with the same takes it.
#[test]
fn another_parents_inherited_values_cascade_again() {
    let mut page = Page::new(
        "<style>.red { color: red } .blue { color: blue } .k { padding: 1px }</style>
         <section class=red><p class=k id=red></p></section><section class=blue id=blue></section>",
    );
    assert_eq!(
        append(&mut page, "blue", "p", &[("class", "k"), ("id", "one")]),
        0
    );
    assert_ne!(page.style("one").color, page.style("red").color);
    assert_eq!(
        append(&mut page, "blue", "p", &[("class", "k"), ("id", "two")]),
        1
    );
}

/// A `style` attribute is each element's own: what it gives isn't kept.
#[test]
fn a_style_attribute_isnt_kept() {
    let mut page = Page::new("<div id=box></div>");
    for _ in 0..3 {
        assert_eq!(
            append(&mut page, "box", "div", &[("style", "margin: 1px")]),
            0
        );
    }
}

/// A property that isn't inherited, taken with `inherit`, takes the
/// parent's value: such a cascade isn't kept.
#[test]
fn inherit_of_a_property_not_inherited_isnt_kept() {
    let mut page = Page::new(
        "<style>.w { width: inherit }</style>
         <div id=wide style='width: 20px'></div><div id=narrow style='width: 10px'></div>",
    );
    assert_eq!(
        append(&mut page, "wide", "div", &[("class", "w"), ("id", "a")]),
        0
    );
    assert_eq!(
        append(&mut page, "narrow", "div", &[("class", "w"), ("id", "b")]),
        0
    );
    assert_eq!(
        append(&mut page, "wide", "div", &[("class", "w"), ("id", "c")]),
        0
    );
    assert_eq!(page.style("b").width, page.style("narrow").width);
    assert_eq!(page.style("c").width, page.style("wide").width);
}

/// The presentational hints are the element's, made for it: alike ones
/// give the same cascade, others another.
#[test]
fn presentational_hints_count_by_their_values() {
    let mut page = Page::new("<div id=box></div>");
    assert_eq!(
        append(&mut page, "box", "img", &[("width", "10"), ("id", "a")]),
        0
    );
    assert_eq!(append(&mut page, "box", "img", &[("width", "10")]), 0);
    assert_eq!(append(&mut page, "box", "img", &[("width", "10")]), 1);
    assert_eq!(
        append(&mut page, "box", "img", &[("width", "20"), ("id", "b")]),
        0
    );
    assert_ne!(page.style("a").width, page.style("b").width);
}

/// The viewport is part of what a cascade is looked for with: a `vw` is
/// of the viewport as it is now, and the cascades of the new one are
/// kept.
#[test]
fn another_viewport_cascades_again() {
    let mut page =
        Page::new("<style>.v { width: 10vw }</style><div id=box><div class=v id=a></div></div>");
    let px = |v| computed_style::Size::LengthPercentage(computed_style::LengthPercentage::Px(v));
    for (width, expected) in [(1000.0, 100.0), (500.0, 50.0)] {
        let hits = cache(&page).1;
        page.set_viewport(SizeF::new(width, 400.0), 1.0);
        page.recalc();
        assert_eq!(cache(&page).1, hits, "a cascade of another viewport");
        assert_eq!(page.style("a").width, px(expected));
    }
    assert_eq!(
        append(&mut page, "box", "div", &[("class", "v"), ("id", "b")]),
        1
    );
    assert_eq!(page.style("b").width, px(50.0));
}

/// When the document's sheets change, the cache lets go of what the old
/// ones' rules gave: of the elements styled again (their rules changed),
/// each looked for once, it keeps none.
#[test]
fn new_document_sheets_let_go_of_the_old_cascades() {
    let mut page =
        Page::new("<style id=sheet>.a { color: red }</style><div class=a></div><p class=a></p>");
    page.with("sheet", |cx, sheet| {
        let text = cx.first_child(sheet).unwrap();
        cx.set_data(text, ".a { color: blue }");
    });
    assert_eq!(cache(&page).0, 1, "the old rules'");
    page.recalc();
    assert_eq!(cache(&page).0, 0);
}

/// Each shadow tree's rules are its own, though they sort alike (the same
/// tree order and place in their sheets): another tree's cascade isn't
/// taken.
#[test]
fn rules_of_other_shadow_trees_cascade_apart() {
    let mut page = Page::new("<div id=one></div><div id=two></div>");
    for (id, color) in [("one", "red"), ("two", "blue")] {
        let markup = format!("<style>.x {{ color: {color} }}</style><p class=x id={id}-p></p>");
        page.with(id, |cx, host| {
            crate::attach_shadow(cx, host, &markup);
        });
    }
    page.recalc();
    assert_ne!(page.style("one-p").color, page.style("two-p").color);
}

/// The root's font size is part of what a cascade is looked for with: a
/// `rem` is of the root's as it is now, though the parent's values are
/// the same (two elements alike: the cascade of the old size is kept).
#[test]
fn another_root_font_size_cascades_again() {
    let mut page = Page::new(
        "<style>html.big { font-size: 20px } body { font-size: 16px } .r { width: 2rem }</style>
         <html id=root><body><p class=r id=p></p><p class=r></p></body></html>",
    );
    page.with("root", |cx, root| cx.set_attribute(root, "class", "big"));
    page.recalc();
    assert_eq!(
        page.style("p").width,
        computed_style::Size::LengthPercentage(computed_style::LengthPercentage::Px(40.0))
    );
}

/// Elements alike share their style, the cache's (once it keeps one: the
/// second time it is looked for); in a flex container, where they are
/// blockified, they share another; another class, another.
#[test]
fn elements_alike_share_their_style() {
    let mut page = Page::new(
        "<style>.i { width: 1px } .x { width: 2px }</style>
         <div id=block><span id=a class=i></span><span id=b class=i></span>\
         <span id=c class=i></span><span id=d class=x></span></div>
         <div id=flex style='display: flex'><span id=e class=i></span>\
         <span id=f class=i></span><span id=g class=i></span></div>",
    );
    let style = |page: &mut Page, id: &str| page.style(id);
    let (b, c) = (style(&mut page, "b"), style(&mut page, "c"));
    assert!(Arc::ptr_eq(&b, &c), "alike");
    assert!(!Arc::ptr_eq(&c, &style(&mut page, "d")), "another class");
    let (f, g) = (style(&mut page, "f"), style(&mut page, "g"));
    assert!(Arc::ptr_eq(&f, &g), "alike in the flex container");
    assert!(!Arc::ptr_eq(&c, &g), "blockified there");
    assert_eq!(c.display, computed_style::Display::Inline);
    assert_eq!(g.display, computed_style::Display::Block);
}

/// An element styled again whose style comes out the same keeps its old
/// style object, which its children's cache entries and its painted items
/// compare by address.
#[test]
fn the_same_style_again_keeps_the_old_one() {
    let mut page =
        Page::new("<style>.y span { color: red }</style><div id=a><span id=b></span></div>");
    let (a, b) = (page.style("a"), page.style("b"));
    page.with("a", |cx, a| cx.set_attribute(a, "class", "y"));
    page.recalc();
    assert!(page.restyled().contains(&"a".to_owned()), "styled again");
    assert!(Arc::ptr_eq(&a, &page.style("a")), "the same values: kept");
    assert!(!Arc::ptr_eq(&b, &page.style("b")), "another color");
}

/// Pseudo-elements alike share their style too, adjusted for what they are
/// (a marker's `display` by its list item's position, `::before`'s
/// `content: normal` as `none`); `attr()` in `content` takes each
/// element's own value.
#[test]
fn pseudo_elements_alike_share_their_style() {
    let mut page = Page::new(
        "<style>li::before { color: red } p::before { content: attr(data-x) }</style>
         <ul><li id=a></li><li id=b></li><li id=c></li></ul>
         <p id=x data-x=1></p><p id=y data-x=2></p><p id=z data-x=3></p>",
    );
    let pseudo = |page: &mut Page, id: &str, pseudo: computed_style::PseudoId| {
        page.pseudo(id)
            .and_then(|styles| styles.get(pseudo).cloned())
            .expect("styled")
    };
    use computed_style::PseudoId::{Before, Marker};
    let (b, c) = (
        pseudo(&mut page, "b", Marker),
        pseudo(&mut page, "c", Marker),
    );
    assert!(Arc::ptr_eq(&b, &c), "markers alike");
    assert_eq!(c.display, computed_style::Display::InlineBlock);
    let (b, c) = (
        pseudo(&mut page, "b", Before),
        pseudo(&mut page, "c", Before),
    );
    assert!(Arc::ptr_eq(&b, &c), "::before alike");
    assert_eq!(c.rare.content, computed_style::Content::None);
    let (y, z) = (
        pseudo(&mut page, "y", Before),
        pseudo(&mut page, "z", Before),
    );
    assert!(!Arc::ptr_eq(&y, &z));
    let value = |style: &ComputedStyle| match &style.rare.content {
        computed_style::Content::Items(items) => match &items[..] {
            [computed_style::ContentItem::Attr { value, .. }] => value.to_string(),
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    };
    assert_eq!((value(&y), value(&z)), ("2".to_owned(), "3".to_owned()));
}

/// `attr()`'s name is ASCII lowercased for an HTML element in an HTML
/// document, as its attributes' are; not in an XHTML one (`text/html`
/// alone: machopo parses XHTML as HTML, Chrome as XML).
#[test]
fn attr_names_are_lowercased_in_html_documents_alone() {
    let content = |content_type: &str| {
        let mut page =
            Page::parsed("<style>p::before { content: attr(DATA-X) }</style><p id=p data-x=1></p>");
        page.with_document(|cx, document| cx.set_document_content_type(document, content_type));
        page.recalc();
        let before = page
            .pseudo("p")
            .and_then(|styles| styles.get(computed_style::PseudoId::Before).cloned())
            .expect("styled");
        match &before.rare.content {
            computed_style::Content::Items(items) => match &items[..] {
                [computed_style::ContentItem::Attr { value, .. }] => value.to_string(),
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(content("text/html"), "1");
    assert_eq!(content("application/xhtml+xml"), "");
}
