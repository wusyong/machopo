//! Which selectors an element is matched against: only those in the
//! buckets of its id, classes and name (and the rest's), which must give
//! every element the style matching every selector gives; and few of them.

use computed_style::{ComputedStyle, LengthPercentage, Size};
use style::SelectorQuery;

use crate::Page;

/// Every element's `id` and computed style in tree order, and how many
/// selectors were matched in full, with `adopted` as the document's
/// adopted sheets; matching every selector if `linear`.
fn run(html: &str, adopted: &[&str], linear: bool) -> (Vec<Styled>, usize) {
    let mut page = Page::parsed(html);
    page.engine.set_linear_matching_for_testing(linear);
    page.with_document(|cx, document| {
        for (at, css) in adopted.iter().enumerate() {
            let sheet = cx.create_style_sheet(document, crate::sheet(css), "", false);
            cx.set_adopted_style_sheet(document, at, sheet);
        }
    });
    page.recalc();
    let Page {
        dom,
        document,
        token,
        engine,
        ..
    } = &page;
    let styles = dom.read(|cx| {
        let mut out = Vec::new();
        crate::each_element(cx, document.get(cx.gc), &mut |element| {
            out.push((
                crate::id_of(cx, element),
                cx.computed_style(element, token)
                    .map(|style| (**style).clone()),
            ));
        });
        out
    });
    (styles, engine.matched_selectors_for_testing())
}

type Styled = (Option<String>, Option<ComputedStyle>);

/// The buckets give each element the style every selector gives.
fn same_as_linear(html: &str, adopted: &[&str]) -> Vec<Styled> {
    let (buckets, matched) = run(html, adopted, false);
    let (linear, matched_linearly) = run(html, adopted, true);
    // Linearly is every selector against every element.
    assert!(
        matched_linearly > matched * 2,
        "{matched_linearly} against {matched}"
    );
    assert_eq!(buckets.len(), linear.len());
    for (index, (buckets, linear)) in buckets.iter().zip(&linear).enumerate() {
        assert_eq!(buckets, linear, "element {index}");
    }
    buckets
}

/// The style of the element with `id`.
fn style<'a>(styles: &'a [Styled], id: &str) -> &'a ComputedStyle {
    styles
        .iter()
        .find(|(of, _)| of.as_deref() == Some(id))
        .and_then(|(_, style)| style.as_ref())
        .unwrap_or_else(|| panic!("#{id}'s style"))
}

fn px(v: f32) -> Size {
    Size::LengthPercentage(LengthPercentage::Px(v))
}

const SHEETS: &str = "<style>
  #a { width: 1px } .b { height: 2px } div { margin-left: 3px } * { padding-top: 1px }
  [data-x] { width: 4px } [data-x=y] { width: 5px }
  .c.d { width: 6px } .d { min-width: 7px !important }
  span:is(.e, .f) { width: 8px } :where(.g) { width: 9px } p:not(.h) { width: 10px }
  .i:has(> .j) { width: 11px } .k > .l { width: 12px } .k .m { width: 13px }
  .n + .o { width: 14px } .n ~ .p { height: 15px }
  .Upper { width: 16px } #Mixed { height: 17px } DIV.v { min-height: 1px }
  SECTION { min-height: 2px }
  .w, #w { height: 50px } div.w { height: 51px }
  section, .q, #r { width: 18px } .q, .s { height: 19px }
  li:first-child { width: 20px } li:nth-child(2n) { height: 21px }
  input:checked { width: 22px } a:link { width: 23px } a { height: 1px }
  svg clipPath { width: 24px } svg clippath { height: 25px } svg rect.t { width: 26px }
</style>
<style>.b { height: 30px } .c { width: 31px } * { padding-bottom: 2px }</style>";

const BODY: &str = "
  <div id=a class='b c d v'></div>
  <div data-x=y class=Upper></div><div class=upper id=mixed></div><div id=Mixed></div>
  <span class=e></span><span class='f g'></span><p></p><p class=h></p>
  <div class=i><span class=j></span></div>
  <div class=k><div class=l></div><div><div class=m></div></div></div>
  <div class=n></div><div class=o></div><div class=p></div><div id=w class=w></div>
  <section></section><div class='q s'></div><div id=r></div>
  <ul><li></li><li></li><li class=t></li></ul>
  <input type=checkbox checked><a href=u></a><a></a>
  <svg><clipPath></clipPath><rect class=t></rect></svg>";

/// In a standards mode document, a quirks mode one, and with adopted
/// sheets after the document's own.
#[test]
fn the_buckets_give_the_same_styles() {
    let styles = same_as_linear(&format!("<!DOCTYPE html>{SHEETS}{BODY}"), &[]);
    // Not just the same, but styled, the same in both: `#a`'s width is
    // from the document's first sheet, its height from the second (later
    // wins); a rule counts with its most specific matching selector
    // (`#w`, not `.w`, against `div.w`).
    assert_eq!(style(&styles, "a").width, px(1.0));
    assert_eq!(style(&styles, "a").height, px(30.0));
    assert_eq!(style(&styles, "w").height, px(50.0));
    same_as_linear(&format!("{SHEETS}{BODY}"), &[]);
    let styles = same_as_linear(
        &format!("<!DOCTYPE html>{SHEETS}{BODY}"),
        &[
            ".b { height: 40px } #r { width: 41px }",
            "div { width: 42px }",
        ],
    );
    // The adopted sheets come after the document's.
    assert_eq!(style(&styles, "a").height, px(40.0));
    assert_eq!(style(&styles, "r").width, px(41.0));
}

const ANCESTOR_SHEET: &str = "<style>
  .a p { width: 1px } .a .b p { height: 2px } section p { min-width: 3px }
  #x .y { width: 4px } .c > .d .e { height: 5px } :is(.f) p { min-height: 6px }
  .Upper p { height: 7px } svg g rect { width: 8px } svg clipPath rect { height: 9px }
  div.g p.h { width: 10px } .gone p { width: 11px } .k .k .k { height: 12px }
</style>";

/// Elements whose ancestors come and go between them: after a subtree,
/// what was pushed for it is popped (`.gone` doesn't count for later
/// elements), and what a later element's parent chain has is pushed.
const ANCESTOR_BODY: &str = "
  <div class=gone><div class=a><div class=b><p id=p1></p></div></div></div>
  <div class=a><p id=p2></p><span><p id=p3></p></span></div><p id=p4></p>
  <section><div><p id=p5></p></div></section>
  <div id=x><div><span class=y id=y></span></div></div><span class=y id=y2></span>
  <div class=c><div class=d><div><span class=e id=e></span></div></div></div>
  <div class=f><p id=p6></p></div><div class=upper><p id=p7></p></div>
  <svg><g><rect id=r1></rect></g><clipPath><rect id=r2></rect></clipPath></svg>
  <div class=g><p class=h id=h></p></div>
  <div class=k><div class=k><div class=k id=k></div></div></div>";

/// The ancestor bloom filter passes over only what wouldn't match: every
/// element gets the style matching every selector gives, in a standards
/// mode document and a quirks mode one (where `.Upper` matches `upper`);
/// and it passes over some.
#[test]
fn the_bloom_filter_gives_the_same_styles() {
    let styles = same_as_linear(
        &format!("<!DOCTYPE html>{ANCESTOR_SHEET}{ANCESTOR_BODY}"),
        &[],
    );
    assert_eq!(style(&styles, "p1").height, px(2.0));
    assert_eq!(style(&styles, "p3").width, px(1.0));
    assert_eq!(style(&styles, "p4").width, Size::Auto);
    assert_eq!(style(&styles, "y").width, px(4.0));
    assert_eq!(style(&styles, "y2").width, Size::Auto);
    assert_eq!(style(&styles, "e").height, px(5.0));
    assert_eq!(style(&styles, "r1").width, px(8.0));
    assert_eq!(style(&styles, "r2").height, px(9.0));
    assert_eq!(style(&styles, "h").width, px(10.0));
    assert_eq!(style(&styles, "k").height, px(12.0));
    let quirks = same_as_linear(&format!("{ANCESTOR_SHEET}{ANCESTOR_BODY}"), &[]);
    assert_eq!(style(&quirks, "p7").height, px(7.0));
    // `.zz p` against `p`s with no `.zz` above them.
    assert_eq!(extra_matched(".zz p { width: 1px }", "<p></p><p></p>"), 0);
}

/// `:has()`'s filter of an element's subtree (made the second time one
/// of its `:has()`s is matched) has its descendants' names: a later
/// `:has()` that should match isn't rejected by it.
#[test]
fn has_s_filter_has_the_subtree_s_names() {
    let (styles, _) = run(
        "<!DOCTYPE html><style>.box:has(.a) { width: 1px } .box:has(.b) { height: 2px }
           .box:has(#c) { width: 3px } .box:has(em) { min-height: 4px }</style>
         <div class=box id=box><span><i class=b id=c></i></span></div>",
        &[],
        false,
    );
    assert_eq!(style(&styles, "box").width, px(3.0));
    assert_eq!(style(&styles, "box").height, px(2.0));
}

/// How many selectors a document with `rules` matches in full, beyond one
/// without them.
fn extra_matched(rules: &str, body: &str) -> usize {
    let (_, with) = run(
        &format!("<!DOCTYPE html><style>{rules}</style>{body}"),
        &[],
        false,
    );
    let (_, without) = run(&format!("<!DOCTYPE html><style></style>{body}"), &[], false);
    with - without
}

/// Of a thousand rules, an element is matched against those in its own
/// buckets only.
#[test]
fn few_selectors_are_matched() {
    let thousand = |rule: &dyn Fn(usize) -> String| (0..1000).map(rule).collect::<String>();
    let body = "<div id=i7 class='c7 x'></div>";
    assert_eq!(
        extra_matched(&thousand(&|n| format!(".c{n} {{ width: 1px }}")), body),
        1
    );
    assert_eq!(
        extra_matched(&thousand(&|n| format!("#i{n} {{ width: 1px }}")), body),
        1
    );
    // The rightmost compound's: `.x .cN` is in `cN`'s bucket; and with no
    // ancestor of class `x`, the bloom filter passes over it.
    assert_eq!(
        extra_matched(&thousand(&|n| format!(".x .c{n} {{ width: 1px }}")), body),
        0
    );
    assert_eq!(
        extra_matched(
            &thousand(&|n| format!(".x .c{n} {{ width: 1px }}")),
            "<section class=x><div class=c7></div></section>"
        ),
        1
    );
    // Nor once what had it is behind: it was popped.
    assert_eq!(
        extra_matched(
            &thousand(&|n| format!(".x .c{n} {{ width: 1px }}")),
            "<section class=x><i></i></section><div class=c7></div>"
        ),
        0
    );
    assert_eq!(
        extra_matched(
            &thousand(&|n| format!("t{n} {{ width: 1px }}")),
            "<t7></t7><div></div>"
        ),
        1
    );
    // A class's case doesn't matter to the buckets; classes that differ
    // only in case look in the bucket once.
    assert_eq!(
        extra_matched(
            &thousand(&|n| format!(".C{n} {{ width: 1px }}")),
            "<div class='c7 C7'></div>"
        ),
        1
    );
    // A selector two buckets give is matched once, whatever came between.
    assert_eq!(
        extra_matched(
            &format!(
                "{} .y {{ width: 1px }}",
                thousand(&|n| format!(".c{n} {{ width: 1px }}"))
            ),
            "<div class='c7 y C7'></div>"
        ),
        2
    );
    // What has none of those is matched against every element: `html`,
    // `head`, `style`, `body` and these two.
    assert_eq!(
        extra_matched("[data-a] { width: 1px }", "<div></div><p></p>"),
        6
    );
}

/// A rule for a pseudo-element is in its element's bucket (`.a::before`
/// in `a`'s, as Blink's `FindBestRuleSetAndAdd`), matched only against the
/// elements that may have it, and gives them a pseudo-element style; the
/// element's own style doesn't take its declarations. A selector query
/// with a pseudo-element matches no element (not a `SyntaxError`).
#[test]
fn pseudo_element_rules_are_in_their_elements_buckets() {
    let body = "<div id=one class=a></div><div id=two></div><div id=three></div><p id=four></p>";
    let html = format!(
        "<style>.a::before {{ content: 'x'; color: red }} p::after {{ color: blue }}</style>{body}"
    );
    // The UA's rules are matched either way, `<style>` too; `.z` is in a
    // bucket no element looks in.
    let (_, without) = run(
        &format!("<style>.z {{ color: red }}</style>{body}"),
        &[],
        false,
    );
    let mut page = Page::new(&html);
    // `.a::before` for `#one`, `p::after` for `#four`.
    assert_eq!(page.engine.matched_selectors_for_testing() - without, 2);
    let found: Vec<(&str, bool, bool)> = ["one", "two", "three", "four"]
        .into_iter()
        .map(|id| {
            let pseudo = page.pseudo(id);
            let has = |pseudo_id| {
                pseudo
                    .as_ref()
                    .is_some_and(|styles| styles.get(pseudo_id).is_some())
            };
            (
                id,
                has(computed_style::PseudoId::Before),
                has(computed_style::PseudoId::After),
            )
        })
        .collect();
    assert_eq!(
        found,
        [
            ("one", true, false),
            ("two", false, false),
            ("three", false, false),
            ("four", false, true),
        ]
    );
    // The pseudo-element's `color`, not its element's.
    assert_ne!(
        page.style("one").color,
        page.pseudo("one").unwrap().before.unwrap().color
    );
    let query = SelectorQuery::parse("p::after").expect("a valid selector");
    assert!(!page.with("four", |cx, four| query.matches(cx, four)));
    assert!(SelectorQuery::parse("div:before").is_some());
}

/// A list item's `::marker` has a style, with the user-agent's `white-space:
/// pre`, whether a rule of the page matches it or not; any other element's
/// has none. Only the properties that apply to a marker cascade: its
/// `display` is its position's (an outside one's `inline-block`, an inside
/// one's `inline`), it has no `text-indent`, and the list properties are
/// its list item's, which it inherits.
#[test]
fn a_list_items_marker_has_a_style() {
    let html = "<style>::marker { color: red; display: block; font-size: 20px;
          list-style-type: decimal; text-shadow: 1px 1px; text-decoration: underline }
          #b::marker { content: 'x' attr(data-x) }</style>
        <ul style='text-indent: 5px'><li id=a><li id=b data-x=y style='list-style-position: inside'></ul>
        <div id=div></div>";
    let mut page = Page::new(html);
    let mut marker = |id: &str| page.pseudo(id).and_then(|styles| styles.marker);
    let a = marker("a").expect("a list item's marker");
    assert_eq!(a.color, gfx::Color::rgb(255, 0, 0));
    assert_eq!(a.font.size, 20.0);
    // Outside: laid out as an inline-block, whatever its `display`.
    assert_eq!(a.display, computed_style::Display::InlineBlock);
    assert_eq!(
        a.rare_inherited.list_style_type,
        computed_style::ListStyleType::Disc
    );
    assert_eq!(
        a.white_space_collapse,
        computed_style::WhiteSpaceCollapse::Preserve
    );
    assert_eq!(a.rare.content, computed_style::Content::Normal);
    // `text-shadow` applies, a decoration doesn't (Blink's).
    assert_eq!(a.rare_inherited.text_shadow.len(), 1);
    assert!(a.rare_inherited.applied_text_decorations.is_empty());
    let b = marker("b").unwrap();
    // Inside: an inline box; never its list item's indent.
    assert_eq!(b.display, computed_style::Display::Inline);
    assert_eq!(
        b.rare_inherited.text_indent,
        computed_style::LengthPercentage::Px(0.0)
    );
    assert_eq!(
        b.rare.content,
        computed_style::Content::Items(
            [
                computed_style::ContentItem::String("x".into()),
                computed_style::ContentItem::Attr {
                    name: "data-x".into(),
                    value: "y".into()
                }
            ]
            .into()
        )
    );
    assert!(marker("div").is_none());
    let query = SelectorQuery::parse("li::marker").expect("a valid selector");
    assert!(SelectorQuery::parse("li:marker").is_none());
    assert!(!page.with("a", |cx, li| query.matches(cx, li)));
}
