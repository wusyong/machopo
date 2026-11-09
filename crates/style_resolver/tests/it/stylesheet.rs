//! A constructed stylesheet's contents as CSSOM sees them: parsing, each
//! rule's `selectorText`, `cssText` and `conditionText` (Chrome's), a media
//! list's text, and the edits `insertRule`, `deleteRule` and a changed rule
//! make, in the sheet or in a group rule, each new contents sharing the
//! rules it didn't touch.

use std::sync::Arc;
use style_resolver::UrlContext;

use style_resolver::{
    CssPropertyValueSet, CssRule, InsertRuleError, MediaQueryList, StyleSheetContents,
};

fn style(rule: &CssRule) -> &Arc<style_resolver::StyleRule> {
    rule.as_style().expect("a style rule")
}

fn selector_texts(rules: &[CssRule]) -> Vec<String> {
    rules
        .iter()
        .map(|rule| style(rule).selector_text())
        .collect()
}

fn css_texts(rules: &[CssRule]) -> Vec<String> {
    rules.iter().map(CssRule::css_text).collect()
}

#[test]
fn parsing_keeps_style_media_and_supports_rules_and_drops_other_at_rules() {
    let contents = StyleSheetContents::parse(
        "@import url(a.css); .a { color: red } @media screen { .b { color: blue } } \
         ::unknown { color: red } @font-face { font-family: x } \
         @supports (display: grid) { .c { } } .d { }",
        None,
    );
    assert_eq!(
        css_texts(contents.rules()),
        [
            ".a { color: red; }",
            "@media screen {\n  .b { color: blue; }\n}",
            "@supports (display: grid) {\n  .c { }\n}",
            ".d { }",
        ]
    );
}

#[test]
fn rules_serialize_as_chrome_writes_them() {
    let contents = StyleSheetContents::parse(
        "div{margin:10px;padding:0} p,.foo{} div>p{color:red !important} \
         [type=checkbox]:checked{} :not(.a) li{}",
        None,
    );
    assert_eq!(
        css_texts(contents.rules()),
        [
            "div { margin: 10px; padding: 0px; }",
            "p, .foo { }",
            "div > p { color: red !important; }",
            "[type=\"checkbox\"]:checked { }",
            ":not(.a) li { }",
        ]
    );
}

/// The pseudo-classes as written: `:lang()`'s ranges each an identifier
/// or a string, as it was; an invalid range (a number) drops the rule.
#[test]
fn pseudo_classes_serialize_as_written() {
    let contents = StyleSheetContents::parse(
        ":LANG(en,\"*-CH\" ) {} :lang(1) {} input:READ-ONLY:required {} \
         :read-write:optional {} :target {}",
        None,
    );
    assert_eq!(
        css_texts(contents.rules()),
        [
            ":lang(en, \"*-CH\") { }",
            "input:read-only:required { }",
            ":read-write:optional { }",
            ":target { }",
        ]
    );
}

/// Blink's `AppendCSSTextForItems`: each rule on a line of its own,
/// indented by two spaces (what is in them isn't indented again).
#[test]
fn group_rules_serialize_as_chrome_writes_them() {
    let contents = StyleSheetContents::parse(
        "@MEDIA SCREEN AND (MIN-WIDTH:10EM),print{.a{color:red}.b{}} \
         @media print {} @media {} \
         @supports   (display:grid)   and (not (color: nonsense)) { @media (color) { .c {} } }",
        None,
    );
    assert_eq!(
        css_texts(contents.rules()),
        [
            "@media screen and (min-width: 10em), print {\n  .a { color: red; }\n  .b { }\n}",
            "@media print {\n}",
            "@media  {\n}",
            "@supports (display:grid) and (not (color: nonsense)) {\n  \
             @media (color) {\n  .c { }\n}\n}",
        ]
    );
    let conditions: Vec<_> = contents
        .rules()
        .iter()
        .map(|rule| rule.condition_text().unwrap())
        .collect();
    assert_eq!(
        conditions,
        [
            "screen and (min-width: 10em), print",
            "print",
            "",
            "(display:grid) and (not (color: nonsense))",
        ]
    );
    assert_eq!(
        StyleSheetContents::parse(".a {}", None).rules()[0].condition_text(),
        None
    );
}

/// mediaqueries-4's serialization, as Chrome writes it: lower case, `all
/// and` left out before a condition, ranges and ratios spaced out, what
/// isn't known as written, `not all` for what doesn't parse.
#[test]
fn media_queries_serialize_as_chrome_writes_them() {
    for (text, written) in [
        ("", ""),
        ("all", "all"),
        ("ALL AND (color)", "(color)"),
        (
            "only screen and (max-width:100px)",
            "only screen and (max-width: 100px)",
        ),
        ("not print", "not print"),
        ("not all and (monochrome)", "not all and (monochrome)"),
        ("(min-aspect-ratio:16/9)", "(min-aspect-ratio: 16 / 9)"),
        ("(400px<=width<=700px)", "(400px <= width <= 700px)"),
        ("(width>=600px)", "(width >= 600px)"),
        (
            "((color) and (hover)) or (fancy)",
            "((color) and (hover)) or (fancy)",
        ),
        ("not (color)", "not (color)"),
        ("(min-resolution: 2DPPX)", "(min-resolution: 2dppx)"),
        (
            "(prefers-color-scheme: DARK)",
            "(prefers-color-scheme: dark)",
        ),
        ("screen, !nonsense, print", "screen, not all, print"),
        ("(fancy: 1)", "(fancy: 1)"),
        ("foo(bar)", "foo(bar)"),
        // WPT's `serialize-media-rule.html` and `MediaList-parsing.html`.
        ("spEech", "speech"),
        ("not screen", "not screen"),
        ("screen and (cOLor)", "screen and (color)"),
        ("  screen  ,  print  ", "screen, print"),
        ("NOT  PRINT", "not print"),
        ("bad$", "not all"),
    ] {
        assert_eq!(MediaQueryList::parse(text).to_css(), written, "{text:?}");
    }
}

#[test]
fn media_lists_take_and_lose_media() {
    let list = MediaQueryList::parse("screen");
    let added = list.with_medium("PRINT").unwrap();
    assert_eq!(added.to_css(), "screen, print");
    assert!(added.with_medium("print").is_none(), "already there");
    assert!(list.with_medium("a, b").is_none(), "not one medium");
    assert_eq!(added.without_medium("screen").unwrap().to_css(), "print");
    assert!(added.without_medium("tv").is_none(), "not there");
    assert_eq!(
        added
            .without_medium("print")
            .unwrap()
            .queries()
            .collect::<Vec<_>>(),
        ["screen"]
    );
}

#[test]
fn a_rule_takes_another_selector_only_if_it_parses() {
    let contents = StyleSheetContents::parse("div { color: red }", None);
    let rule = style(&contents.rules()[0]);
    let changed = rule.with_selector_text("p,  .foo").expect("valid");
    assert_eq!(changed.selector_text(), "p, .foo");
    assert_eq!(changed.css_text(), "p, .foo { color: red; }");
    assert!(rule.with_selector_text("p {").is_none());
    assert!(rule.with_selector_text("").is_none());
    assert!(rule.with_selector_text("::unknown").is_none());
}

#[test]
fn a_rule_takes_other_declarations() {
    let contents = StyleSheetContents::parse("div { color: red }", None);
    let rule = style(&contents.rules()[0]);
    let mut block = rule.style().clone();
    block.set_property("margin", "42px", "");
    let changed = rule.with_style(block);
    assert_eq!(changed.css_text(), "div { color: red; margin: 42px; }");
    assert_eq!(
        rule.css_text(),
        "div { color: red; }",
        "the old rule as it was"
    );
}

#[test]
fn a_media_rule_takes_other_media() {
    let contents = StyleSheetContents::parse("@media screen { .a {} }", None);
    let rule = &contents.rules()[0];
    let changed = rule.with_media(MediaQueryList::parse("print"));
    assert_eq!(changed.css_text(), "@media print {\n  .a { }\n}");
    // The same rules in it.
    assert!(changed.child_rules().unwrap()[0].same(&rule.child_rules().unwrap()[0]));
}

#[test]
fn insert_rule_checks_the_index_then_parses_one_rule() {
    let contents = StyleSheetContents::parse(".a {} .b {}", None);
    let inserted = contents.insert_rule("  .new { color: red }  ", 1).unwrap();
    assert_eq!(selector_texts(inserted.rules()), [".a", ".new", ".b"]);
    assert_eq!(selector_texts(contents.rules()), [".a", ".b"], "unchanged");
    // The rules it didn't touch are the same ones.
    assert!(inserted.rules()[0].same(&contents.rules()[0]));
    assert!(inserted.rules()[2].same(&contents.rules()[1]));
    assert_eq!(
        selector_texts(contents.insert_rule(".end {}", 2).unwrap().rules()),
        [".a", ".b", ".end"]
    );
    // Group rules too.
    let media = contents.insert_rule("@media screen { .x {} }", 0).unwrap();
    assert_eq!(media.rules()[0].css_text(), "@media screen {\n  .x { }\n}");
    let supports = contents
        .insert_rule("@supports (color: red) {}", 0)
        .unwrap();
    assert_eq!(
        supports.rules()[0].condition_text().unwrap(),
        "(color: red)"
    );

    let error = |rule: &str, index: usize| contents.insert_rule(rule, index).err();
    assert_eq!(error(".x {}", 3), Some(InsertRuleError::IndexSize));
    // The index first, as Blink checks it.
    assert_eq!(error("not a rule", 3), Some(InsertRuleError::IndexSize));
    for rule in [
        "",
        "not a rule",
        ".x {} .y {}",
        ".x { color: red } garbage",
        "@import url(a.css);",
        "@font-face { font-family: x }",
        "@supports nonsense { .x {} }",
        "::unknown {}",
    ] {
        assert_eq!(error(rule, 0), Some(InsertRuleError::Syntax), "{rule:?}");
    }
}

#[test]
fn a_group_rule_inserts_deletes_and_replaces_its_rules() {
    let contents = StyleSheetContents::parse("@media all { .a {} .b {} }", None);
    let media = &contents.rules()[0];
    let inserted = media
        .insert_rule("@supports (color: red) { .c {} }", 1)
        .unwrap();
    assert_eq!(
        inserted.css_text(),
        "@media all {\n  .a { }\n  @supports (color: red) {\n  .c { }\n}\n  .b { }\n}"
    );
    assert!(inserted.child_rules().unwrap()[0].same(&media.child_rules().unwrap()[0]));
    assert_eq!(
        media.insert_rule(".x {}", 3).err(),
        Some(InsertRuleError::IndexSize)
    );
    assert_eq!(
        media.insert_rule("nonsense", 0).err(),
        Some(InsertRuleError::Syntax)
    );

    let deleted = media.delete_rule(0).unwrap();
    assert_eq!(selector_texts(deleted.child_rules().unwrap()), [".b"]);
    assert!(media.delete_rule(2).is_none());

    let old = media.child_rules().unwrap()[1].clone();
    let new = CssRule::Style(Arc::new(
        style(&old).with_style(CssPropertyValueSet::parse("color: red")),
    ));
    let replaced = media.replace_rule(&old, new.clone()).unwrap();
    assert!(replaced.child_rules().unwrap()[1].same(&new));
    assert!(
        media.replace_rule(&new, old).is_none(),
        "not one of its rules"
    );
    // The media rule in the sheet's contents: replaced in turn.
    let sheet = contents.replace_rule(media, replaced.clone()).unwrap();
    assert!(sheet.rules()[0].same(&replaced));
}

#[test]
fn delete_rule_needs_a_rule_at_the_index() {
    let contents = StyleSheetContents::parse(".a {} .b {} .c {}", None);
    let deleted = contents.delete_rule(1).unwrap();
    assert_eq!(selector_texts(deleted.rules()), [".a", ".c"]);
    assert!(deleted.rules()[1].same(&contents.rules()[2]));
    assert!(contents.delete_rule(3).is_none());
    assert!(StyleSheetContents::parse("", None).delete_rule(0).is_none());
}

#[test]
fn replace_rule_finds_the_same_rule() {
    let contents = StyleSheetContents::parse(".a {} .b {}", None);
    let old = contents.rules()[1].clone();
    let new = CssRule::Style(Arc::new(
        style(&old).with_style(CssPropertyValueSet::parse("color: red")),
    ));
    let replaced = contents.replace_rule(&old, new.clone()).unwrap();
    assert!(replaced.rules()[1].same(&new));
    assert!(replaced.rules()[0].same(&contents.rules()[0]));
    // A rule taken out (another sheet's, or an older version's) isn't
    // found.
    let other = StyleSheetContents::parse(".b {}", None);
    assert!(contents.replace_rule(&other.rules()[0], new).is_none());
}

#[test]
fn edits_keep_the_base_url() {
    let url = url::Url::parse("https://example.com/styles/").unwrap();
    let contents = StyleSheetContents::parse(".a {}", Some(UrlContext::of_sheet(url.clone())));
    assert_eq!(contents.base_url(), Some(&url));
    assert_eq!(
        contents.insert_rule(".b {}", 0).unwrap().base_url(),
        Some(&url)
    );
    assert_eq!(contents.delete_rule(0).unwrap().base_url(), Some(&url));
    assert_eq!(StyleSheetContents::parse(".a {}", None).base_url(), None);
}

/// An edit resolves the `url()`s of the rule it puts in, not of every rule
/// again: `insertRule` into a large sheet (a CSS-in-JS library's, a rule
/// at a time) costs the same whatever its size.
#[test]
fn an_edit_resolves_only_its_rules_urls() {
    let resolved = || style_resolver::rules_resolved_for_testing().expect("a debug build");
    let url = url::Url::parse("https://example.com/styles/").unwrap();
    let text: String = (0..1000)
        .map(|i| format!(".r{i} {{ background-image: url(r{i}.png) }}\n"))
        .collect();
    let contents = StyleSheetContents::parse(&text, Some(UrlContext::of_sheet(url)));
    let before = resolved();
    let inserted = contents
        .insert_rule(".new { background-image: url(new.png) }", 1000)
        .unwrap();
    assert_eq!(resolved(), before + 1);
    assert!(!style(&inserted.rules()[1000]).block.has_unresolved_urls());
    let deleted = inserted.delete_rule(0).unwrap();
    assert_eq!(resolved(), before + 1, "nothing put in");
    let old = deleted.rules()[5].clone();
    let block = CssPropertyValueSet::parse("background-image: url(other.png)");
    assert!(block.has_unresolved_urls());
    let new = CssRule::Style(Arc::new(style(&old).with_style(block)));
    let replaced = deleted.replace_rule(&old, new).unwrap();
    assert_eq!(resolved(), before + 2);
    assert!(!style(&replaced.rules()[5]).block.has_unresolved_urls());
    // The rules it didn't touch are the same ones.
    assert!(Arc::ptr_eq(
        style(&contents.rules()[7]),
        style(&replaced.rules()[6])
    ));
    // Without a base URL, nothing is.
    let unbased = StyleSheetContents::parse(".a {}", None);
    let before = resolved();
    let inserted = unbased
        .insert_rule(".b { background-image: url(b.png) }", 0)
        .unwrap();
    assert_eq!(resolved(), before);
    assert!(style(&inserted.rules()[0]).block.has_unresolved_urls());
}

/// `::placeholder` and any `::-webkit-` pseudo-element parse and read back
/// as written (the latter in lowercase), after `::slotted()` too (Chrome
/// 152's); after `::placeholder` nothing, after a `::-webkit-` one a state
/// (`:hover`). `::-internal-` and `::-moz-` ones don't parse.
#[test]
fn internal_pseudo_classes_are_the_ua_stylesheets() {
    // A page's sheet drops a rule with one.
    let contents = StyleSheetContents::parse(
        "select:-internal-list-box { } select { } :-INTERNAL-list-box { }          option:-internal-multi-select-focus { }",
        None,
    );
    assert_eq!(selector_texts(contents.rules()), ["select"]);
}

/// Which selectors with `::selection` or `:window-inactive` are valid, as
/// Chrome 152 takes them (`CSS.supports('selector(…)')`, `querySelector`
/// and a style rule agree): `:window-inactive` anywhere a pseudo-class
/// goes, and after `::selection` or a UA part, not after another
/// pseudo-element; nothing but it after `::selection`; `::selection` last,
/// not after `::slotted()`; not `::-moz-selection`.
#[test]
fn selection_selectors_are_chromes() {
    let cases = [
        (":window-inactive", true),
        ("a:window-inactive", true),
        (":is(:window-inactive)", true),
        ("::before:window-inactive", false),
        ("::marker:window-inactive", false),
        ("::selection:window-inactive", true),
        ("::selection:window-inactive:window-inactive", true),
        ("::-webkit-scrollbar:window-inactive", true),
        ("::selection:focus", false),
        ("::selection:active", false),
        ("::selection:hover:window-inactive", false),
        ("::selection::before", false),
        ("::before::selection", false),
        ("::placeholder::selection", false),
        ("::selection .a", false),
        ("::slotted(span)::selection", false),
        ("a::selection", true),
        (":host::selection", true),
        ("::-moz-selection", false),
        ("a, ::-moz-selection", false),
        ("::selection:not(:window-inactive)", true),
    ];
    for (selector, valid) in cases {
        assert_eq!(
            css::selector::parse_selector_list(selector).is_some(),
            valid,
            "{selector}"
        );
        // `@supports` takes no `::-webkit-scrollbar` at all: Blink's
        // scrollbar parts aren't among the UA parts machopo knows.
        if !selector.contains("scrollbar") {
            assert_eq!(
                css::selector::supports_selector(selector),
                valid,
                "{selector}"
            );
        }
        let contents = StyleSheetContents::parse(&format!("{selector} {{ }}"), None);
        assert_eq!(contents.rules().len(), usize::from(valid), "{selector}");
    }
    // Read back in lowercase.
    let contents = StyleSheetContents::parse("::SELECTION { }", None);
    assert_eq!(selector_texts(contents.rules()), ["::selection"]);
}

#[test]
fn placeholder_selectors_read_back_as_written() {
    let contents = StyleSheetContents::parse(
        "input::placeholder { } ::-WEBKIT-input-placeholder { } \
         .a:hover::placeholder { } ::slotted(input)::placeholder { } \
         ::placeholder:hover { } input::placeholder::before { } \
         input::-webkit-outer-spin-button { } ::-webkit-inner-spin-button:hover { } \
         ::-internal-foo { } ::-moz-foo { } ::-webkit- { } ::-w\u{e9}bkit-foo { }",
        None,
    );
    assert_eq!(
        selector_texts(contents.rules()),
        [
            "input::placeholder",
            "::-webkit-input-placeholder",
            ".a:hover::placeholder",
            "::slotted(input)::placeholder",
            "input::-webkit-outer-spin-button",
            "::-webkit-inner-spin-button:hover",
        ]
    );
}

/// What DevTools lists of a rule's selectors: each one's text and its
/// specificity, ids, classes and elements apart.
#[test]
fn selectors_are_listed_with_their_specificity() {
    let contents = StyleSheetContents::parse("#a .b > p, div::before, :is(#x, y) .c.d { }", None);
    assert_eq!(
        style(&contents.rules()[0]).selectors_for_inspector(),
        [
            ("#a .b > p".to_owned(), [1, 1, 1]),
            ("div::before".to_owned(), [0, 0, 2]),
            (":is(#x, y) .c.d".to_owned(), [1, 2, 0]),
        ]
    );
}
