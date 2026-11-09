//! Selector queries on `dom` (`SelectorQuery`): `querySelector`'s
//! first and `querySelectorAll`'s every match below the root in tree
//! order (the fast paths' too: a lone id, class or type), `matches` and
//! `closest` with `:scope`, quirks mode's classes, an invalid list, a
//! pseudo-element matching nothing; a selector of compounds matched
//! compound by compound as its full match does, and matched in full only
//! where they say it may match (the debug build's count).

use dom::{Cx, NodeRef, names};
use style::SelectorQuery;

use crate::{Page, by_id};

fn ids<'gc>(cx: &Cx<'_, 'gc>, nodes: &[NodeRef<'gc>]) -> Vec<String> {
    nodes
        .iter()
        .map(|&node| {
            cx.attribute_value_by_id(node, names::ID)
                .map(|id| id.borrow().to_str().into_owned())
                .unwrap_or_default()
        })
        .collect()
}

const PAGE: &str = "<!doctype html><div id=a class='x Y'><p id=b class=x>t</p>\
    <span id=c><p id=d class=y>u</p></span></div><p id=e>v</p><section id=f><p id=g>w</p></section>";

#[test]
fn queries_find_in_tree_order() {
    let page = Page::new(PAGE);
    let document = &page.document;
    page.dom.read(|cx| {
        let document = document.get(cx.gc);
        let all = |text: &str| {
            ids(
                cx,
                &SelectorQuery::parse(text)
                    .expect("valid")
                    .query_all(cx, document),
            )
        };
        assert_eq!(all("p"), ["b", "d", "e", "g"], "a type");
        assert_eq!(
            all("P"),
            ["b", "d", "e", "g"],
            "an HTML element's type in any case"
        );
        assert_eq!(all(".x"), ["a", "b"], "a class");
        assert_eq!(all(".y"), ["d"], "case-sensitive");
        assert_eq!(all("#d"), ["d"]);
        assert_eq!(all("div p"), ["b", "d"]);
        assert_eq!(
            all("div > p, section p"),
            ["b", "g"],
            "a list, in tree order"
        );
        assert_eq!(all("span ~ *, #a + p"), ["e"]);
        assert!(
            all("p::before").is_empty(),
            "a pseudo-element matches nothing"
        );
        let first = SelectorQuery::parse("p").unwrap().query_first(cx, document);
        assert_eq!(ids(cx, &first.into_iter().collect::<Vec<_>>()), ["b"]);
        let first = SelectorQuery::parse("#g")
            .unwrap()
            .query_first(cx, document);
        assert_eq!(
            ids(cx, &first.into_iter().collect::<Vec<_>>()),
            ["g"],
            "by the id table"
        );
        // Below an element: not the element itself; `:scope` is it.
        let a = by_id(cx, document, "a").unwrap();
        let below = |text: &str| ids(cx, &SelectorQuery::parse(text).unwrap().query_all(cx, a));
        assert_eq!(below("*"), ["b", "c", "d"]);
        assert_eq!(below("div p"), ["b", "d"], "the root's ancestors count");
        assert_eq!(below(":scope > p"), ["b"]);
        assert_eq!(below("#d"), ["d"], "an id below an element: a walk");
        assert!(below("#a").is_empty(), "not the root's own id");
        assert!(
            SelectorQuery::parse("#a")
                .unwrap()
                .query_first(cx, a)
                .is_none()
        );
    });
}

#[test]
fn matches_and_closest() {
    let page = Page::new(PAGE);
    let document = &page.document;
    page.dom.read(|cx| {
        let document = document.get(cx.gc);
        let d = by_id(cx, document, "d").unwrap();
        let query = |text: &str| SelectorQuery::parse(text).unwrap();
        assert!(query("span > .y").matches(cx, d));
        assert!(!query("div > .y").matches(cx, d));
        assert!(query(":scope").matches(cx, d));
        let closest = |text: &str| query(text).closest(cx, d).map(|node| ids(cx, &[node]));
        assert_eq!(closest("div"), Some(vec!["a".to_owned()]));
        assert_eq!(
            closest("p"),
            Some(vec!["d".to_owned()]),
            "the element itself"
        );
        assert_eq!(closest("section"), None);
    });
}

#[test]
fn quirks_mode_matches_classes_in_any_case() {
    let page = Page::new("<p id=p class=Foo>a</p>");
    let document = &page.document;
    page.dom.read(|cx| {
        let document = document.get(cx.gc);
        let found = SelectorQuery::parse(".foo")
            .unwrap()
            .query_all(cx, document);
        assert_eq!(ids(cx, &found), ["p"]);
        let found = SelectorQuery::parse("p.FOO")
            .unwrap()
            .query_all(cx, document);
        assert_eq!(ids(cx, &found), ["p"]);
        // A lone class (the fast path) written in another case.
        let found = SelectorQuery::parse(".FOO")
            .unwrap()
            .query_all(cx, document);
        assert_eq!(ids(cx, &found), ["p"]);
    });
}

#[test]
fn an_invalid_list_is_none() {
    assert!(SelectorQuery::parse("p >").is_none());
    assert!(SelectorQuery::parse("").is_none());
    assert!(SelectorQuery::parse("::selection:hover").is_none());
    assert!(SelectorQuery::parse("p, div").is_some());
}

/// A selector of compounds joined by descendants (or children) matched
/// compound by compound: ancestors above the root match the first ones; a
/// child combinator, another class or a pseudo-class matched in full on
/// what the compounds find; exact attributes as written, `i` without case,
/// those HTML compares without case (`type`) in full; a sibling
/// combinator, a list, matched in full.
#[test]
fn compounds_match_as_the_full_match() {
    let page = Page::new(
        "<!doctype html><main id=m class=top><ul id=u data-k=List>\
         <li id=l1 data-id=1 class='item done'><input id=i1 type=checkbox></li>\
         <li id=l2 data-id=2 class=item><b id=b2 class=item></b></li>\
         <li id=l3 data-id=22 class=done></li></ul></main><li id=l4 data-id=2></li>",
    );
    let document = &page.document;
    page.dom.read(|cx| {
        let document = document.get(cx.gc);
        let all = |root, text: &str| {
            ids(
                cx,
                &SelectorQuery::parse(text)
                    .expect("valid")
                    .query_all(cx, root),
            )
        };
        let list = by_id(cx, document, "u").expect("the list");
        assert_eq!(all(document, r#"[data-id="2"]"#), ["l2", "l4"]);
        assert_eq!(all(document, r#"ul [data-id="2"]"#), ["l2"]);
        assert_eq!(
            all(list, r#"main li[data-id="2"]"#),
            ["l2"],
            "main above the root"
        );
        assert_eq!(all(list, ".top .item"), ["l1", "l2", "b2"]);
        assert_eq!(
            all(document, "ul > .item"),
            ["l1", "l2"],
            "a child, in full"
        );
        assert_eq!(all(document, "li.item.done"), ["l1"], "two classes");
        assert_eq!(all(document, "li:not(.done)"), ["l2", "l4"]);
        assert_eq!(
            all(document, r#"[data-k="list"]"#),
            Vec::<String>::new(),
            "as written"
        );
        assert_eq!(all(document, r#"[data-k="list" i]"#), ["u"]);
        assert_eq!(
            all(document, r#"[type="CHECKBOX"]"#),
            ["i1"],
            "HTML's type without case"
        );
        assert_eq!(
            all(document, "#L1"),
            Vec::<String>::new(),
            "an id as written"
        );
        assert_eq!(all(document, "li + li"), ["l2", "l3"], "a sibling, in full");
        assert_eq!(all(document, "b, #l3"), ["b2", "l3"], "a list, in full");
        let first = SelectorQuery::parse(r#"li[data-id="2"]"#)
            .unwrap()
            .query_first(cx, document);
        assert_eq!(ids(cx, &first.into_iter().collect::<Vec<_>>()), ["l2"]);
    });
}

/// Quirks mode's ids and classes match without case, compound by compound
/// too.
#[test]
fn compounds_match_quirks_modes_ids_and_classes() {
    let quirks = Page::new("<div id=Q class=Big><p id=p class=x></p></div>");
    let document = &quirks.document;
    quirks.dom.read(|cx| {
        let document = document.get(cx.gc);
        let all = |text: &str| {
            ids(
                cx,
                &SelectorQuery::parse(text)
                    .expect("valid")
                    .query_all(cx, document),
            )
        };
        assert_eq!(all("#q"), ["Q"], "quirks mode's id without case");
        assert_eq!(all(".big .X"), ["p"], "and its classes");
    });
}

/// A selector of compounds is matched in full only where its compounds
/// say it may match, and not at all when they say all of it.
#[test]
fn compounds_leave_the_full_match_to_what_they_find() {
    let Some(_) = style::full_matches_for_testing() else {
        // Counted in debug builds only.
        return;
    };
    let mut markup = String::from("<!doctype html><ul>");
    for index in 0..50 {
        markup.push_str(&format!("<li data-id={index} class=item><b></b></li>"));
    }
    markup.push_str("</ul>");
    let page = Page::new(&markup);
    let document = &page.document;
    page.dom.read(|cx| {
        let document = document.get(cx.gc);
        let count = |text: &str| {
            let before = style::full_matches_for_testing().unwrap();
            let found = SelectorQuery::parse(text)
                .expect("valid")
                .query_all(cx, document)
                .len();
            (found, style::full_matches_for_testing().unwrap() - before)
        };
        assert_eq!(count(r#"[data-id="7"]"#), (1, 0));
        assert_eq!(count(r#"ul li[data-id="7"]"#), (1, 0));
        assert_eq!(count(".item b"), (50, 0));
        assert_eq!(
            count(r#"ul > [data-id="7"]"#),
            (1, 1),
            "the one the compounds find"
        );
        assert_eq!(count("li + li"), (49, 104), "each element: no compounds");
    });
}

/// A subtree whose bloom filter misses what a selector's compounds need
/// (an attribute's name, a class) is skipped, its elements not matched;
/// one it has is walked.
#[test]
fn a_bloom_filter_skips_the_subtrees_without_what_it_takes() {
    let Some(_) = style::compound_elements_for_testing() else {
        // Counted in debug builds only.
        return;
    };
    let mut markup = String::from("<!doctype html><body><div id=big>");
    for _ in 0..200 {
        markup.push_str("<p><b></b></p>");
    }
    markup.push_str("</div><ul id=list>");
    for index in 0..5 {
        markup.push_str(&format!("<li data-id={index} class=item></li>"));
    }
    markup.push_str("</ul></body>");
    let page = Page::new(&markup);
    let document = &page.document;
    page.dom.read(|cx| {
        let document = document.get(cx.gc);
        let walk = |root, text: &str| {
            let before = style::compound_elements_for_testing().unwrap();
            let found = SelectorQuery::parse(text)
                .expect("valid")
                .query_all(cx, root);
            (
                ids(cx, &found),
                style::compound_elements_for_testing().unwrap() - before,
            )
        };
        let (found, walked) = walk(document, r#"[data-id="3"]"#);
        assert!(found.len() == 1 && walked < 20, "{walked} elements");
        let (found, walked) = walk(document, "ul .item");
        assert!(found.len() == 5 && walked < 20, "{walked} elements");
        let big = by_id(cx, document, "big").expect("the big div");
        let (found, walked) = walk(big, ".item");
        assert!(found.is_empty() && walked == 0, "nothing below the root");
        let (found, walked) = walk(document, "p b");
        assert!(
            found.len() == 200 && walked > 400,
            "no attribute: all of it"
        );
    });
}

/// An element given a `style` attribute by CSSOM, written back lazily:
/// `[style]` finds it before the attribute is read, whatever its subtree's
/// bloom filter has.
#[test]
fn a_lazily_written_style_attribute_is_found() {
    let mut page = Page::new("<!doctype html><div><p id=p>p</p></div>");
    page.with("p", |cx, p| {
        let mut block = cx.take_inline_style(p);
        std::sync::Arc::make_mut(&mut block).set_property("color", "red", "");
        cx.set_inline_style(p, block);
    });
    let document = &page.document;
    page.dom.read(|cx| {
        let document = document.get(cx.gc);
        let found = SelectorQuery::parse("div [style]")
            .unwrap()
            .query_all(cx, document);
        assert_eq!(ids(cx, &found), ["p"]);
    });
}
