//! What an element's own style is cascaded from, for DevTools
//! (`StyleEngine::explain`): the rules it matches, the UA's and the
//! author's, in cascade order with the winner last (an `!important`
//! block where its normal declarations are), each with the selectors of
//! its list it matches; none of its pseudo-elements' or of rules it
//! doesn't match; its presentational hints and its `style` attribute's
//! declarations apart. A shadow tree's element its tree's rules, a host
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
