//! What an element's own style is cascaded from, for DevTools
//! (`StyleEngine::explain`): the rules it matches, the UA's and the
//! author's, in cascade order with the winner last (an `!important`
//! block where its normal declarations are), each with the selectors of
//! its list it matches; none of rules it doesn't match; its
//! pseudo-elements' the same way, each apart; its presentational hints and
//! its `style` attribute's declarations apart. A shadow tree's element its tree's rules, a host
//! its shadow tree's `:host` rules, a slotted one its slot's tree's
//! `::slotted()`. Nothing for a node that isn't an element of the engine's
//! document's; nothing kept of the explaining.

use css::stylesheet::Origin;
use style::Explained;

use crate::{Page, find};

/// `explain` of the element whose id is `id`.
fn explain(page: &mut Page, id: &str) -> Option<Explained> {
    let Page {
        dom,
        document,
        engine,
        ..
    } = page;
    dom.read(|cx| {
        let element = find(cx, document.get(cx.gc), id).unwrap_or_else(|| panic!("no #{id}"));
        engine.explain(cx, element)
    })
}

/// Each rule's selector text and the selectors it matches, the author's
/// only (the UA's are many, and change).
fn author_rules(explained: &Explained) -> Vec<(String, Vec<usize>)> {
    explained
        .rules
        .iter()
        .filter(|rule| rule.origin == Origin::Author)
        .map(|rule| (rule.rule.selector_text(), rule.matching_selectors.clone()))
        .collect()
}

#[test]
fn rules_come_in_cascade_order() {
    let mut page = Page::new(
        "<!doctype html><style>\
         #x { color: red !important }\
         div { color: green }\
         .a, p, div.a { color: blue }\
         span { color: gray }\
         div::before { content: 'b' }\
         .a { margin: 0 }\
         body > .a { padding: 0 }\
         div::after, .a:empty, #x.a { color: black }\
         </style>\
         <div id=x class=a style='color: pink'>x</div><font id=f color=red>f</font>",
    );
    let explained = explain(&mut page, "x").expect("an element");
    assert_eq!(
        author_rules(&explained),
        [
            // Specificity first, then where it comes; the `!important`
            // one at its normal place, its specificity's.
            ("div".to_owned(), vec![0]),
            (".a".to_owned(), vec![0]),
            (".a, p, div.a".to_owned(), vec![0, 2]),
            // Its parent in the bloom filter.
            ("body > .a".to_owned(), vec![0]),
            ("#x".to_owned(), vec![0]),
            // Its pseudo-element's selector isn't its own.
            ("div::after, .a:empty, #x.a".to_owned(), vec![2]),
        ]
    );
    // The UA's come first: `div { display: block }` among them.
    let first_author = explained
        .rules
        .iter()
        .position(|rule| rule.origin == Origin::Author)
        .unwrap();
    assert!(first_author > 0);
    assert!(
        explained.rules[..first_author]
            .iter()
            .all(|rule| rule.origin == Origin::UserAgent)
    );
    assert!(
        explained.rules[..first_author]
            .iter()
            .any(|rule| rule.rule.selector_text().contains("div"))
    );
    assert_eq!(
        explained
            .inline
            .as_deref()
            .map(css::CssPropertyValueSet::css_text),
        Some("color: pink;".to_owned())
    );
    assert!(explained.presentation.is_none());
    let font = explain(&mut page, "f").expect("an element");
    assert_eq!(
        font.presentation
            .as_deref()
            .map(css::CssPropertyValueSet::css_text),
        Some("color: rgb(255, 0, 0);".to_owned())
    );

    // Neither for an element with neither.
    drop(page);
    let mut page = Page::new("<!doctype html><p id=p>p</p>");
    let explained = explain(&mut page, "p").expect("an element");
    assert!(author_rules(&explained).is_empty());
    assert!(explained.inline.is_none() && explained.presentation.is_none());
}

#[test]
fn shadow_trees_explain_their_rules() {
    let mut page = Page::with_shadows(
        "<!doctype html><style>b { color: red } div { color: red }</style>\
         <div id=host><b id=light slot=s>b</b></div>",
        &[(
            "host",
            "<style>:host { color: blue } ::slotted(b) { color: green } \
             i, em { color: gray }</style><i id=inner>i</i><slot name=s></slot>",
        )],
    );
    let host = explain(&mut page, "host").expect("the host");
    assert_eq!(
        author_rules(&host),
        // The inner tree's normal declarations lose to the outer's.
        [(":host".to_owned(), vec![0]), ("div".to_owned(), vec![0])]
    );
    let inner = explain(&mut page, "inner").expect("a shadow tree's element");
    assert_eq!(author_rules(&inner), [("i, em".to_owned(), vec![0])]);
    let light = explain(&mut page, "light").expect("a slotted element");
    assert_eq!(
        author_rules(&light),
        [
            ("::slotted(b)".to_owned(), vec![0]),
            ("b".to_owned(), vec![0])
        ]
    );
}

#[test]
fn explaining_keeps_nothing() {
    let mut page = Page::new(
        "<!doctype html><style>p:nth-child(2) { color: red } .a { color: blue }</style>\
         <div id=d><p id=a class=a>a</p><p id=b>b</p></div><template id=t><p id=c>c</p></template>",
    );
    let style = page.style("b");
    let matched = page.engine.matched_selectors_for_testing();
    assert_eq!(
        author_rules(&explain(&mut page, "b").unwrap()),
        [("p:nth-child(2)".to_owned(), vec![0])]
    );
    // Its matching isn't the walk's; its style is the same one.
    assert_eq!(page.engine.matched_selectors_for_testing(), matched);
    assert!(std::sync::Arc::ptr_eq(&page.style("b"), &style));
    page.recalc();
    assert!(page.restyled().is_empty());
    // Not an element of the document's tree: of another, of none.
    assert!(explain(&mut page, "c").is_none());
    let loose = {
        let Page {
            dom,
            document,
            engine,
            ..
        } = &mut page;
        dom.mutate(|cx| {
            let a = find(cx, document.get(cx.gc), "a").unwrap();
            cx.remove(a);
            engine.explain(cx, a).is_none()
        })
    };
    assert!(loose);
    // Connected, but in another document, which another engine styles.
    let other = page.dom.mutate(|cx| {
        let other = cx.create_document(true);
        let html = cx.create_element(other, dom::QualName::html("html"));
        cx.append_child(other, html).unwrap();
        page.engine.explain(cx, html).is_none()
    });
    assert!(other);
    let text = {
        let Page {
            dom,
            document,
            engine,
            ..
        } = &mut page;
        dom.read(|cx| {
            let b = find(cx, document.get(cx.gc), "b").unwrap();
            engine.explain(cx, cx.first_child(b).unwrap()).is_none()
        })
    };
    assert!(text);
}

/// A sheet adopted twice: its rule twice, as it cascades, each with the
/// selector that matches once.
#[test]
fn a_rule_in_two_places_is_listed_twice() {
    let mut page = Page::new("<!doctype html><p id=p class=a>p</p>");
    let document = &page.document;
    page.dom.mutate(|cx| {
        let document = document.get(cx.gc);
        let sheet = cx.create_style_sheet(
            document,
            std::sync::Arc::new(css::StyleSheetContents::parse(".a { color: red }", None)),
            "",
            false,
        );
        cx.set_adopted_style_sheet(document, 0, sheet);
        cx.set_adopted_style_sheet(document, 1, sheet);
    });
    page.recalc();
    assert_eq!(
        author_rules(&explain(&mut page, "p").unwrap()),
        [(".a".to_owned(), vec![0]), (".a".to_owned(), vec![0])]
    );
}

/// The sheets (their ids) of the author rules, by selector.
fn author_sheets(explained: &Explained) -> Vec<(String, Option<u64>)> {
    explained
        .rules
        .iter()
        .filter(|rule| rule.origin == Origin::Author)
        .map(|rule| (rule.rule.selector_text(), rule.sheet))
        .collect()
}

/// The ids of the sheets of the elements `selector` finds, in tree order
/// (a shadow host's shadow tree's before its children).
fn sheet_ids(page: &mut Page, selector: &str) -> Vec<u64> {
    let document = &page.document;
    page.dom.mutate(|cx| {
        let document = document.get(cx.gc);
        let mut ids = Vec::new();
        let mut at = cx.first_child(document);
        let mut all = Vec::new();
        while let Some(node) = at {
            all.push(node);
            let shadow = cx.shadow_root(node).and_then(|root| cx.first_child(root));
            at = shadow.or_else(|| cx.first_child(node)).or_else(|| {
                let mut up = Some(node);
                while let Some(here) = up {
                    if let Some(next) = cx.next_sibling(here) {
                        return Some(next);
                    }
                    // Out of a shadow tree: its host's children, or on
                    // from its host.
                    if let Some(host) = cx.parent(here).and_then(|parent| cx.shadow_host(parent)) {
                        if let Some(child) = cx.first_child(host) {
                            return Some(child);
                        }
                        up = Some(host);
                        continue;
                    }
                    up = cx.parent(here);
                }
                None
            });
        }
        for node in all {
            if cx.kind(node) == dom::NodeKind::Element
                && cx.node_name(node).to_str().eq_ignore_ascii_case(selector)
                && let Some(sheet) = cx.owner_sheet(node)
            {
                ids.push(cx.style_sheet_id(sheet));
            }
        }
        ids
    })
}

/// Each rule's sheet: of the small sheets one index has, each its own;
/// two `<style>`s of the same text (one parse, the same rules) each theirs;
/// an adopted sheet; once a `<style>` is taken out and put back (the same
/// to style, another object), the new one.
#[test]
fn rules_know_their_sheets() {
    let mut page = Page::new(
        "<!doctype html><style>p { color: red }</style><style>.a { color: blue }</style>\
         <style>#p { margin: 0 }</style><style>#p { margin: 0 }</style><p id=p class=a>p</p>",
    );
    let ids = sheet_ids(&mut page, "style");
    assert_eq!(ids.len(), 4);
    let explained = explain(&mut page, "p").unwrap();
    assert_eq!(
        author_sheets(&explained),
        [
            ("p".to_owned(), Some(ids[0])),
            (".a".to_owned(), Some(ids[1])),
            ("#p".to_owned(), Some(ids[2])),
            ("#p".to_owned(), Some(ids[3])),
        ]
    );
    assert!(explained.rules.iter().filter(|rule| rule.origin == Origin::UserAgent).all(|rule| rule.sheet.is_none()));

    // Adopted, after the document's own.
    let document = &page.document;
    let adopted = page.dom.mutate(|cx| {
        let document = document.get(cx.gc);
        let sheet = cx.create_style_sheet(
            document,
            std::sync::Arc::new(css::StyleSheetContents::parse(".a { padding: 0 }", None)),
            "",
            false,
        );
        cx.set_adopted_style_sheet(document, 0, sheet);
        cx.style_sheet_id(sheet)
    });
    page.recalc();
    let explained = explain(&mut page, "p").unwrap();
    let sheets = author_sheets(&explained);
    // After the `<style>`'s `.a`, by where it comes (its specificity, the
    // same, below the `#p`s').
    let style_a = sheets.iter().position(|sheet| *sheet == (".a".to_owned(), Some(ids[1])));
    let adopted_a = sheets.iter().position(|sheet| *sheet == (".a".to_owned(), Some(adopted)));
    assert!(style_a.is_some() && adopted_a == style_a.map(|at| at + 1), "{sheets:?}");

    // The first `<style>` taken out and put back: another object of the
    // same contents.
    page.with("p", |cx, p| {
        // The `<style>`s are the `<head>`'s.
        let html = cx.parent(cx.parent(p).unwrap()).unwrap();
        let parent = cx.first_child(html).unwrap();
        let style = cx.first_child(parent).unwrap();
        let next = cx.next_sibling(style).unwrap();
        cx.remove(style);
        cx.insert(parent, style, Some(next));
    });
    page.recalc();
    let again = sheet_ids(&mut page, "style");
    assert_ne!(again[0], ids[0]);
    let explained = explain(&mut page, "p").unwrap();
    assert_eq!(author_sheets(&explained)[0], ("p".to_owned(), Some(again[0])));
}

/// A large sheet (an index of its own), a small one after it, and a shadow
/// tree's sheets: a rule
/// of its tree's, a host's `:host` rule, a slotted element's `::slotted()`
/// rule, each of its root's sheet (one that doesn't apply not counted).
#[test]
fn large_and_shadow_sheets_are_known() {
    let mut big = String::from("<!doctype html><style>");
    for at in 0..600 {
        big.push_str(&format!(".x{at} {{ color: red }} "));
    }
    // A small sheet after it, of an index of its own that starts there.
    big.push_str(
        "#host { color: blue }</style><style>#host { padding: 1px }</style>\
         <div id=host><b id=light slot=s>b</b></div>",
    );
    let mut page = Page::with_shadows(
        &big,
        &[(
            "host",
            "<style media=print>i { color: black }</style><style>:host { color: green }</style>\
             <style>::slotted(b) { color: gray } i { color: pink }</style>\
             <i id=inner>i</i><slot name=s></slot>",
        )],
    );
    let [document_sheet, small_sheet, _print, host_sheet, inner_sheet] =
        sheet_ids(&mut page, "style")[..]
    else {
        panic!("five sheets");
    };
    let host = explain(&mut page, "host").unwrap();
    assert_eq!(
        author_sheets(&host),
        [
            (":host".to_owned(), Some(host_sheet)),
            ("#host".to_owned(), Some(document_sheet)),
            ("#host".to_owned(), Some(small_sheet))
        ]
    );
    let inner = explain(&mut page, "inner").unwrap();
    assert_eq!(author_sheets(&inner), [("i".to_owned(), Some(inner_sheet))]);
    let light = explain(&mut page, "light").unwrap();
    assert_eq!(author_sheets(&light)[0], ("::slotted(b)".to_owned(), Some(inner_sheet)));
}

/// Each pseudo-element's rules the same way, apart from the element's own:
/// its `::before`, `::after`, `::marker` and `::selection`, each with the
/// selectors of its list it matches as that pseudo-element and its sheet.
#[test]
fn pseudo_elements_rules_are_apart() {
    let mut page = Page::new(
        "<!doctype html><style>\
         .a::before { content: 'b' }\
         div::after, .a::before, .a { color: blue }\
         .a::marker { color: green }\
         .a::selection { color: pink }\
         </style><div id=x class=a>x</div>",
    );
    let explained = explain(&mut page, "x").expect("an element");
    let author = |rules: &[style::ExplainedRule]| -> Vec<(String, Vec<usize>, bool)> {
        rules
            .iter()
            .filter(|rule| rule.origin == Origin::Author)
            .map(|rule| {
                (
                    rule.rule.selector_text(),
                    rule.matching_selectors.clone(),
                    rule.sheet.is_some(),
                )
            })
            .collect()
    };
    assert_eq!(
        author(&explained.rules),
        [("div::after, .a::before, .a".to_owned(), vec![2], true)]
    );
    let [before, after, marker, selection] = &explained.pseudo;
    assert_eq!(
        author(before),
        [
            (".a::before".to_owned(), vec![0], true),
            ("div::after, .a::before, .a".to_owned(), vec![1], true)
        ]
    );
    assert_eq!(
        author(after),
        [("div::after, .a::before, .a".to_owned(), vec![0], true)]
    );
    assert_eq!(author(marker), [(".a::marker".to_owned(), vec![0], true)]);
    assert_eq!(
        author(selection),
        [(".a::selection".to_owned(), vec![0], true)]
    );
}
