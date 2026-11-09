//! What styles an element: the UA stylesheet, `<style>` sheets in order,
//! adopted sheets after them, the `style` attribute, inheritance, `rem`;
//! selectors by tag, class (numbers: case-sensitively in standards mode,
//! not in quirks mode), id, attribute, combinators (through the ancestor
//! bloom filter) and structural pseudo-classes; `::before` with `attr()`;
//! the values each property computes to, and what gets ignored.

use std::collections::BTreeMap;
use std::sync::Arc;

use computed_style::{
    BackgroundAttachment, BackgroundBox, BackgroundImage, BackgroundRepeat, BackgroundSize,
    BoxSizing, ComputedStyle, Content, ContentItem, Display, FlexBasis, GradientItem, GradientKind,
    ItemAlignment, LengthPercentage, LengthPercentageOrAuto, LengthPercentageOrNone,
    LinearDirection, Position2D, PositionComponent, PseudoId, RadialShape, RadialSize, Sides, Size,
    StyleColor, VerticalSide,
};
use css::{StyleSheetContents, UrlContext};
use gfx::Color;

use crate::Page;

const RED: Color = Color::rgb(255, 0, 0);
const GREEN: Color = Color::rgb(0, 128, 0);
const BLUE: Color = Color::rgb(0, 0, 255);

/// Parses `html`, runs style and returns the computed style of every
/// element with an `id`, or none for one not styled.
fn styles(html: &str) -> BTreeMap<String, Option<ComputedStyle>> {
    styles_adopting(html, Vec::new(), None)
}

/// [`styles`], with `adopted` as the document's adopted stylesheets and
/// `base` its URL.
fn styles_adopting(
    html: &str,
    adopted: Vec<Arc<StyleSheetContents>>,
    base: Option<url::Url>,
) -> BTreeMap<String, Option<ComputedStyle>> {
    let mut page = Page::parsed(html);
    if let Some(base) = &base {
        page.with_document(|cx, document| cx.set_document_url(document, base.as_str()));
    }
    page.set_url_context(base.map(UrlContext::of_sheet));
    page.with_document(|cx, document| {
        for (at, contents) in adopted.into_iter().enumerate() {
            let sheet = cx.create_style_sheet(document, contents, "", false);
            cx.set_adopted_style_sheet(document, at, sheet);
        }
    });
    page.recalc();
    owned(page.all_styles())
}

fn owned(
    styles: BTreeMap<String, Option<Arc<ComputedStyle>>>,
) -> BTreeMap<String, Option<ComputedStyle>> {
    styles
        .into_iter()
        .map(|(id, style)| (id, style.map(|style| (*style).clone())))
        .collect()
}

fn style_of<'a>(
    styles: &'a BTreeMap<String, Option<ComputedStyle>>,
    id: &str,
) -> &'a ComputedStyle {
    styles[id]
        .as_ref()
        .unwrap_or_else(|| panic!("#{id} has no style"))
}

/// `#id`'s width, which these tests give as `auto` or a length.
fn width(styles: &BTreeMap<String, Option<ComputedStyle>>, id: &str) -> LengthPercentageOrAuto {
    match style_of(styles, id).width {
        Size::Auto => LengthPercentageOrAuto::Auto,
        Size::LengthPercentage(v) => LengthPercentageOrAuto::LengthPercentage(v),
        other => panic!("#{id}'s width is {other:?}"),
    }
}

fn size_px(v: f32) -> Size {
    Size::LengthPercentage(LengthPercentage::Px(v))
}

fn px(v: f32) -> LengthPercentageOrAuto {
    LengthPercentageOrAuto::LengthPercentage(LengthPercentage::Px(v))
}

#[test]
fn the_ua_sheet_author_sheets_and_the_style_attribute_cascade() {
    let mut page = Page::new(
        "<!doctype html><style>p { color: red } .a { color: blue }</style>\
         <style>#x { color: green } p { display: inline }</style>\
         <p id=p1>1</p><p id=p2 class=a>2</p><p id=p3 class=a>3</p>\
         <p id=p4 class=a style='color: rgb(0, 128, 0) !important'>4</p>\
         <span id=s>s</span>",
    );
    assert_eq!(page.style("p1").color, RED);
    // A class beats a tag.
    assert_eq!(page.style("p2").color, BLUE);
    // The `style` attribute beats the rules, at its importance.
    assert_eq!(page.style("p4").color, GREEN);
    // The later sheet's rule wins over the UA sheet's `display: block`.
    assert_eq!(page.style("p1").display, Display::Inline);
    // The UA sheet alone.
    assert_eq!(page.style("s").display, Display::Inline);
    page.with("p3", |cx, p| cx.set_attribute(p, "id", "x"));
    page.recalc();
    assert_eq!(page.style("x").color, GREEN, "an id beats a class");
}

#[test]
fn styles_inherit_and_rem_is_the_roots() {
    let mut page = Page::new(
        "<!doctype html><style>html { font-size: 20px } p { font-size: 2rem } \
         div { color: blue }</style><div id=d><span id=s>t</span><p id=p>p</p></div>",
    );
    assert_eq!(page.style("s").color, BLUE, "inherited");
    assert_eq!(page.style("p").font.size, 40.0);
}

/// The root's own `rem` is the initial font size each time it is styled,
/// not its last one: styled again, a root of `2rem` stays 32px. Its
/// `::before` sees the root's as it is now.
#[test]
fn the_roots_own_rem_is_the_initial_font_size() {
    let mut page = Page::new(
        "<!doctype html><html id=r><style>:root { font-size: 2rem } \
         :root::before { content: 'x'; width: 1rem } div { width: 10rem }</style><div id=d></div>",
    );
    assert_eq!(page.style("r").font.size, 32.0);
    let before = page.pseudo("r").and_then(|pseudo| pseudo.before.clone());
    assert_eq!(
        before.expect("a ::before").width,
        size_px(32.0),
        "the root's of this style"
    );
    page.change_at("r", |cx, root| {
        cx.set_attribute(root, "style", "color: green")
    });
    assert_eq!(page.style("r").font.size, 32.0, "styled again");
    assert_eq!(page.style("d").width, size_px(320.0));
    let before = page.pseudo("r").and_then(|pseudo| pseudo.before.clone());
    assert_eq!(before.expect("a ::before").width, size_px(32.0));
}

#[test]
fn selectors_match_by_their_kinds() {
    let mut page = Page::new(
        "<!doctype html><style>\
         div p { color: red } div > b { color: green } i + u { color: blue } \
         em:first-child { color: red } [title=t] { color: green } \
         s:not(.n) { color: blue } q:is(.m) { color: red } P.big { color: green } \
         .Foo { color: blue } .bar { color: red } a:link { color: green } EM { width: 2px } \
         </style>\
         <div><section><p id=p>p</p></section><b id=b>b</b></div><b id=b2>b</b>\
         <p id=p2>p</p><i>i</i><u id=u>u</u><span><em id=e>e</em><em id=e2>e</em></span>\
         <span id=t title=t>t</span><s id=s1>s</s><s id=s2 class=n>s</s><q id=q class=m>q</q>\
         <p id=big class=big>p</p><span id=f class=foo>f</span><span id=g class=Bar>g</span><a id=a href=x>a</a>",
    );
    let default = page.style("p2").color;
    assert_eq!(page.style("p").color, RED, "a descendant");
    assert_eq!(page.style("b").color, GREEN, "a child");
    assert_eq!(page.style("b2").color, default, "not a child of a div");
    assert_eq!(page.style("u").color, BLUE, "an adjacent sibling");
    assert_eq!(page.style("e").color, RED);
    assert_eq!(page.style("e2").color, default, "not the first child");
    assert_eq!(page.style("t").color, GREEN, "an attribute");
    assert_eq!(page.style("s1").color, BLUE);
    assert_ne!(page.style("s2").color, BLUE, ":not()");
    assert_eq!(page.style("q").color, RED, ":is()");
    assert_eq!(page.style("big").color, GREEN, "an HTML tag in any case");
    assert_eq!(
        page.style("e2").width,
        computed_style::Size::LengthPercentage(computed_style::LengthPercentage::Px(2.0)),
        "by its tag's bucket"
    );
    assert_ne!(page.style("f").color, BLUE, "classes are case-sensitive");
    assert_ne!(page.style("g").color, RED, "either way");
    assert_eq!(page.style("a").color, GREEN, ":link");
}

#[test]
fn before_takes_its_attribute() {
    let mut page = Page::new(
        "<!doctype html><style>p::before { content: attr(data-x) } \
         p::after { color: red }</style><p id=p data-x=hello>t</p><p id=q>t</p>",
    );
    let pseudo = page.pseudo("p").expect("pseudo-elements styled");
    let before = pseudo.get(PseudoId::Before).expect("::before");
    match &before.rare.content {
        Content::Items(items) => assert!(matches!(
            &items[..],
            [ContentItem::Attr { value, .. }] if &**value == "hello"
        )),
        other => panic!("{other:?}"),
    }
    assert_eq!(pseudo.get(PseudoId::After).unwrap().color, RED);
    assert!(page.pseudo("q").is_some(), "::after matches it too");
}

#[test]
fn the_ua_sheets_rules_match_by_ancestors() {
    // Its compounds name HTML's namespace (its default one), which the
    // ancestor bloom filter has to have for each ancestor.
    let mut page = Page::new(
        "<!doctype html><section><h1 id=h>x</h1></section><ol><li><ol id=o><li>y</ol></ol>\
         <ul><li><ul id=u><li>z</ul></ul>",
    );
    assert_eq!(page.style("h").font.size, 24.0, ":is(section) h1");
    assert_eq!(
        page.style("o").margin.top,
        computed_style::LengthPercentageOrAuto::LengthPercentage(
            computed_style::LengthPercentage::Px(0.0)
        ),
        "a list in a list"
    );
    assert_eq!(
        page.style("u").rare_inherited.list_style_type,
        computed_style::ListStyleType::Circle
    );
}

#[test]
fn selectors_and_specificity() {
    let s = styles(
        "<style>
           #x { width: 30px }
           .a { width: 20px }
           div { width: 10px }
           .a.b { width: 40px }
           .b { width: 50px }
           section > p { width: 60px }
           section p + p { width: 70px }
         </style>
         <div id=type></div>
         <div id=class class=a></div>
         <div id=x class=a></div>
         <div id=two class='a b'></div>
         <section><p id=child></p><p id=sibling></p><div><p id=deep></p></div></section>",
    );
    assert_eq!(width(&s, "type"), px(10.0));
    assert_eq!(width(&s, "class"), px(20.0));
    // Id beats class even though it comes first.
    assert_eq!(width(&s, "x"), px(30.0));
    // `.a.b` beats the later `.b`: specificity before order.
    assert_eq!(width(&s, "two"), px(40.0));
    assert_eq!(width(&s, "child"), px(60.0));
    assert_eq!(width(&s, "sibling"), px(70.0));
    assert_eq!(width(&s, "deep"), LengthPercentageOrAuto::Auto);
}

#[test]
fn order_importance_and_the_style_attribute() {
    let s = styles(
        "<style>
           div { width: 1px }
           div { width: 2px }
           #imp { width: 3px !important }
           .imp { width: 4px !important }
           #attr { width: 5px }
           #attr-imp { width: 6px !important }
           #mixed { height: 1px }
           .mixed { height: 2px; min-height: 3px !important }
         </style>
         <div id=later></div>
         <div id=imp class=imp></div>
         <div id=attr style='width: 7px'></div>
         <div id=attr-imp style='width: 8px'></div>
         <div id=attr-both style='width: 9px !important; width: 10px'></div>
         <div id=mixed class=mixed></div>",
    );
    assert_eq!(width(&s, "later"), px(2.0));
    // Among important declarations, specificity still decides.
    assert_eq!(width(&s, "imp"), px(3.0));
    // The style attribute beats any selector...
    assert_eq!(width(&s, "attr"), px(7.0));
    // ...but not an important declaration...
    assert_eq!(width(&s, "attr-imp"), px(6.0));
    // ...unless it is important itself.
    assert_eq!(width(&s, "attr-both"), px(9.0));
    // A block's important declarations don't lift its normal ones.
    assert_eq!(style_of(&s, "mixed").height, size_px(1.0));
    assert_eq!(style_of(&s, "mixed").min_height, px(3.0));
}

/// A document's adopted stylesheets cascade as author sheets after its own,
/// in their order (css-cascade's order of appearance), and each `url()`
/// resolves against its sheet's base URL, or the document's.
#[test]
fn adopted_sheets_cascade_after_the_documents_own() {
    // No base URL: its `url()`s stay as written.
    let first = StyleSheetContents::parse(
        "#later, #order, #specific { width: 2px }
         #important { width: 3px !important }
         #own-url { background-image: url(img/a.png) }",
        None,
    );
    let second = StyleSheetContents::parse(
        "#order { width: 4px }
         #sheet-url { background-image: url(img/b.png) }",
        Some(UrlContext::of_sheet(
            url::Url::parse("http://other.test/css/").unwrap(),
        )),
    );
    let s = styles_adopting(
        "<style>
           #later, #order { width: 1px }
           div#specific { width: 1px }
         </style>
         <div id=later></div>
         <div id=order></div>
         <div id=specific></div>
         <div id=important style='width: 5px'></div>
         <div id=own-url></div>
         <div id=sheet-url></div>",
        vec![Arc::new(first), Arc::new(second)],
        Some(url::Url::parse("http://example.test/dir/page.html").unwrap()),
    );
    assert_eq!(width(&s, "later"), px(2.0));
    assert_eq!(width(&s, "order"), px(4.0));
    // Specificity before order, as for any author sheet.
    assert_eq!(width(&s, "specific"), px(1.0));
    assert_eq!(width(&s, "important"), px(3.0));
    let image = |id: &str| style_of(&s, id).backgrounds.background_image[0].clone();
    assert_eq!(image("own-url"), BackgroundImage::Url("img/a.png".into()));
    assert_eq!(
        image("sheet-url"),
        BackgroundImage::Url("http://other.test/css/img/b.png".into())
    );
    // Fetched for the sheet they were parsed in; one left as written, for
    // no one.
    let referrer = |id: &str| match image(id) {
        BackgroundImage::Url(url) => url.referrer,
        image => panic!("{image:?}"),
    };
    assert_eq!(
        referrer("sheet-url").as_deref(),
        Some("http://other.test/css/")
    );
    assert_eq!(referrer("own-url"), None);
}

#[test]
fn user_agent_stylesheet() {
    let s = styles(
        "<!DOCTYPE html><html id=html><head id=head><title id=title>t</title>
           <style id=style>head { display: block } noscript { display: block }</style>
         </head>
         <body id=body><div id=div></div><span id=span></span><p id=p hidden></p>
           <noscript id=noscript></noscript><script id=script></script></body></html>",
    );
    let display = |id| style_of(&s, id).display;
    assert_eq!(display("html"), Display::Block);
    assert_eq!(display("body"), Display::Block);
    assert_eq!(display("div"), Display::Block);
    assert_eq!(display("span"), Display::Inline);
    assert_eq!(display("title"), Display::None);
    assert_eq!(display("style"), Display::None);
    assert_eq!(display("script"), Display::None);
    assert_eq!(display("p"), Display::None);
    // Author rules override UA rules...
    assert_eq!(display("head"), Display::Block);
    // ...but not UA `!important` ones.
    assert_eq!(display("noscript"), Display::None);
}

/// The UA stylesheet's rules are HTML's, as in Chrome 152: an SVG
/// element named as an HTML one (a `<button>` in an `<svg>`) takes none of
/// them, nor does `hidden` hide it; SVG's own rules (the viewports' clip,
/// the focus ring) still apply. A page's rules are any namespace's.
#[test]
fn user_agent_rules_are_html_elements_and_svgs_own() {
    let html = "<!DOCTYPE html><style>button { color: green }</style><svg id=svg>
           <button id=button></button><input id=input></input>
           <select id=select></select><textarea id=textarea></textarea>
           <address id=address></address><rect id=rect hidden></rect>
           <svg id=inner></svg><foreignObject id=foreign></foreignObject>
           <circle id=circle tabindex=0></circle>
         </svg><button id=html-button></button><p id=html-p hidden></p>
         <address id=html-address></address>";
    let mut page = Page::parsed(html);
    page.with("circle", |cx, circle| {
        let document = cx.document_of(circle);
        let window = cx
            .window_of(document)
            .unwrap_or_else(|| cx.create_window(document));
        cx.set_page_focused(window, true);
        cx.set_focused_element(document, Some(circle), true);
    });
    page.recalc();
    let s = owned(page.all_styles());
    let value = |id: &str, property: &str| {
        style_resolver::computed_value(style_of(&s, id), property).expect("a supported property")
    };
    for id in ["button", "input", "select", "textarea"] {
        assert_eq!(value(id, "border-top-style"), "none", "{id}");
        assert_eq!(value(id, "padding-left"), "0px", "{id}");
    }
    assert_eq!(value("button", "color"), "rgb(0, 128, 0)");
    assert_eq!(value("html-button", "border-top-style"), "outset");
    assert_eq!(value("html-address", "font-style"), "italic");
    assert_eq!(value("address", "font-style"), "normal");
    assert_eq!(value("rect", "display"), "inline");
    assert_eq!(value("html-p", "display"), "none");
    for id in ["svg", "inner", "foreign"] {
        assert_eq!(value(id, "overflow-x"), "hidden", "{id}");
    }
    assert_eq!(value("circle", "outline-style"), "auto");
}

#[test]
fn root_element_is_blockified() {
    let s = styles("<html id=root style='display: inline'><body id=body style='display: inline'>");
    assert_eq!(style_of(&s, "root").display, Display::Block);
    assert_eq!(style_of(&s, "body").display, Display::Inline);
}

/// An out-of-flow box is blockified, but keeps the `display` it had: its
/// static position is where that box would have been.
#[test]
fn out_of_flow_boxes_keep_their_original_display() {
    let s = styles(
        "<span id=abs style='position: absolute'></span>
         <span id=fixed style='display: inline-block; position: fixed'></span>
         <span id=static></span>",
    );
    let both = |id: &str| {
        let style = style_of(&s, id);
        (style.display, style.original_display)
    };
    assert_eq!(both("abs"), (Display::Block, Display::Inline));
    assert_eq!(both("fixed"), (Display::Block, Display::InlineBlock));
    assert_eq!(both("static"), (Display::Inline, Display::Inline));
}

#[test]
fn flex_items_are_blockified() {
    let s = styles(
        "<div style='display: flex'>
           <span id=span></span><b id=none style='display: none'></b>
           <p id=flex style='display: flex'><span id=grandchild></span></p>
         </div>
         <span style='display: flex'><span id=in-inline-flex-parent></span></span>
         <div><span id=outside></span></div>",
    );
    assert_eq!(style_of(&s, "span").display, Display::Block);
    assert_eq!(style_of(&s, "none").display, Display::None);
    assert_eq!(style_of(&s, "flex").display, Display::Flex);
    // Only the children: the flex item's own children are not.
    assert_eq!(style_of(&s, "grandchild").display, Display::Block);
    assert_eq!(
        style_of(&s, "in-inline-flex-parent").display,
        Display::Block
    );
    assert_eq!(style_of(&s, "outside").display, Display::Inline);
}

#[test]
fn atomic_inlines_are_blockified_as_items_and_as_the_root() {
    let s = styles(
        "<span id=inline-block style='display: inline-block'></span>
         <div style='display: flex'><span id=flex-item style='display: inline-block'></span></div>
         <div style='display: grid'><span id=grid-item style='display: INLINE-BLOCK'></span></div>
         <span id=inline-flex style='display: inline-flex'><span id=flex-child></span></span>
         <span id=inline-grid style='display: inline-grid'><span id=grid-child></span></span>
         <div style='display: flex'><span id=flex-in-flex style='display: inline-flex'></span>
           <span id=grid-in-flex style='display: inline-grid'></span></div>",
    );
    assert_eq!(style_of(&s, "inline-block").display, Display::InlineBlock);
    assert_eq!(style_of(&s, "flex-item").display, Display::Block);
    assert_eq!(style_of(&s, "grid-item").display, Display::Block);
    // Inline-level flex and grid containers: their children are items.
    assert_eq!(style_of(&s, "inline-flex").display, Display::InlineFlex);
    assert_eq!(style_of(&s, "inline-grid").display, Display::InlineGrid);
    assert_eq!(style_of(&s, "flex-child").display, Display::Block);
    assert_eq!(style_of(&s, "grid-child").display, Display::Block);
    // Blockified, they are block-level containers.
    assert_eq!(style_of(&s, "flex-in-flex").display, Display::Flex);
    assert_eq!(style_of(&s, "grid-in-flex").display, Display::Grid);
    let s = styles("<html id=root style='display: inline-block'></html>");
    assert_eq!(style_of(&s, "root").display, Display::Block);
    let s = styles("<html id=root style='display: inline-grid'></html>");
    assert_eq!(style_of(&s, "root").display, Display::Grid);
}

#[test]
fn flex_properties() {
    use computed_style::{ContentAlignment, FlexDirection, FlexWrap, ItemAlignment};
    let s = styles(
        "<div id=initial></div>
         <div id=longhands style='flex-direction: column-reverse; flex-wrap: wrap-reverse;
           flex-grow: 2.5; flex-shrink: 0; flex-basis: 50%; order: -2;
           justify-content: space-evenly; align-content: flex-end; align-items: first baseline;
           align-self: self-end; row-gap: 1em; column-gap: normal'></div>
         <div id=flow style='flex-flow: wrap column; gap: 3px 4%'></div>
         <div id=flow-wrap-only style='flex-flow: wrap; gap: 5px'></div>
         <div id=invalid style='flex-grow: -1; flex-shrink: 1px; order: 1.5; justify-content: left;
           align-items: last baseline; align-self: space-between; row-gap: -1px;
           flex-basis: stretch; flex-direction: row-reverse; flex-direction: inherit-not'></div>",
    );
    let initial = style_of(&s, "initial");
    assert_eq!(initial.flex_direction, FlexDirection::Row);
    assert_eq!(initial.flex_wrap, FlexWrap::Nowrap);
    assert_eq!(
        (initial.flex_grow, initial.flex_shrink, initial.flex_basis),
        (0.0, 1.0, FlexBasis::Size(Size::Auto))
    );
    assert_eq!(initial.order, 0);
    assert_eq!(initial.justify_content, ContentAlignment::Normal);
    assert_eq!(initial.align_content, ContentAlignment::Normal);
    assert_eq!(initial.align_items, ItemAlignment::Normal);
    assert_eq!(initial.align_self, None);
    assert_eq!(initial.row_gap, LengthPercentage::ZERO);
    assert_eq!(initial.column_gap, LengthPercentage::ZERO);

    let l = style_of(&s, "longhands");
    assert_eq!(l.flex_direction, FlexDirection::ColumnReverse);
    assert_eq!(l.flex_wrap, FlexWrap::WrapReverse);
    assert_eq!((l.flex_grow, l.flex_shrink), (2.5, 0.0));
    assert_eq!(
        l.flex_basis,
        FlexBasis::Size(Size::LengthPercentage(LengthPercentage::Percent(50.0)))
    );
    assert_eq!(l.order, -2);
    assert_eq!(l.justify_content, ContentAlignment::SpaceEvenly);
    assert_eq!(l.align_content, ContentAlignment::FlexEnd);
    assert_eq!(l.align_items, ItemAlignment::Baseline);
    assert_eq!(l.align_self, Some(ItemAlignment::SelfEnd));
    assert_eq!(l.row_gap, LengthPercentage::Px(16.0));
    assert_eq!(l.column_gap, LengthPercentage::ZERO);

    let f = style_of(&s, "flow");
    assert_eq!(
        (f.flex_direction, f.flex_wrap),
        (FlexDirection::Column, FlexWrap::Wrap)
    );
    assert_eq!(
        (f.row_gap, f.column_gap),
        (LengthPercentage::Px(3.0), LengthPercentage::Percent(4.0))
    );
    let f = style_of(&s, "flow-wrap-only");
    assert_eq!(
        (f.flex_direction, f.flex_wrap),
        (FlexDirection::Row, FlexWrap::Wrap)
    );
    assert_eq!(
        (f.row_gap, f.column_gap),
        (LengthPercentage::Px(5.0), LengthPercentage::Px(5.0))
    );

    // Every invalid one is dropped; the valid `row-reverse` stays.
    let i = style_of(&s, "invalid");
    assert_eq!(
        (
            i.flex_grow,
            i.flex_shrink,
            i.order,
            i.justify_content,
            i.align_items,
            i.align_self,
            i.row_gap,
            i.flex_basis
        ),
        (
            0.0,
            1.0,
            0,
            ContentAlignment::Normal,
            ItemAlignment::Normal,
            None,
            LengthPercentage::ZERO,
            FlexBasis::Size(Size::Auto)
        )
    );
    assert_eq!(i.flex_direction, FlexDirection::RowReverse);
}

#[test]
fn flex_shorthand() {
    let s = styles(
        "<style>
           #none { flex: 2 2 2px; flex: none }
           #auto { flex: auto }
           #one { flex: 1 }
           #grow-shrink { flex: 2 0 }
           #zeros { flex: 0 0 0 }
           #basis-first { flex: 10px 3 }
           #basis-only { flex: 5em }
           #initial { flex: 4; flex: initial }
           #invalid { flex: 3; flex: 1 2 3 }
           #negative { flex: -1 }
         </style>
         <div id=none></div><div id=auto></div><div id=one></div><div id=grow-shrink></div>
         <div id=zeros></div><div id=basis-first></div><div id=basis-only></div>
         <div id=initial></div><div id=invalid></div><div id=negative></div>",
    );
    let flex = |id: &str| {
        let style = style_of(&s, id);
        (style.flex_grow, style.flex_shrink, style.flex_basis)
    };
    let percent = |p| FlexBasis::Size(Size::LengthPercentage(LengthPercentage::Percent(p)));
    let auto = FlexBasis::Size(Size::Auto);
    let px = |v| FlexBasis::Size(size_px(v));
    assert_eq!(flex("none"), (0.0, 0.0, auto));
    assert_eq!(flex("auto"), (1.0, 1.0, auto));
    assert_eq!(flex("one"), (1.0, 1.0, percent(0.0)));
    assert_eq!(flex("grow-shrink"), (2.0, 0.0, percent(0.0)));
    assert_eq!(flex("zeros"), (0.0, 0.0, px(0.0)));
    assert_eq!(flex("basis-first"), (3.0, 1.0, px(10.0)));
    assert_eq!(flex("basis-only"), (1.0, 1.0, px(80.0)));
    assert_eq!(flex("initial"), (0.0, 1.0, auto));
    assert_eq!(flex("invalid"), (3.0, 1.0, percent(0.0)));
    assert_eq!(flex("negative"), (0.0, 1.0, auto));
}

#[test]
fn values() {
    let s = styles(
        "<div id=all style='
            display: block; box-sizing: border-box;
            width: 50%; height: 2em; min-width: 1rem; min-height: 10px;
            max-width: 100px; max-height: none;
            margin: 1px 2px 3px; padding: 4px 5px;
            background-color: #00ff0080'></div>
         <div id=one style='margin: 7px; padding: 0'></div>
         <div id=two style='margin: 1px auto'></div>
         <div id=four style='margin: 1px 2px 3px -4px'></div>",
    );
    let all = style_of(&s, "all");
    assert_eq!(all.display, Display::Block);
    assert_eq!(all.box_sizing, BoxSizing::BorderBox);
    assert_eq!(
        all.width,
        Size::LengthPercentage(LengthPercentage::Percent(50.0))
    );
    // No `font-size` yet: `em` and `rem` are 16px.
    assert_eq!(all.height, size_px(32.0));
    assert_eq!(all.min_width, px(16.0));
    assert_eq!(all.min_height, px(10.0));
    assert_eq!(
        all.max_width,
        LengthPercentageOrNone::LengthPercentage(LengthPercentage::Px(100.0))
    );
    assert_eq!(all.max_height, LengthPercentageOrNone::None);
    assert_eq!(
        all.margin,
        Sides {
            top: px(1.0),
            right: px(2.0),
            bottom: px(3.0),
            left: px(2.0)
        }
    );
    assert_eq!(
        all.padding,
        Sides {
            top: LengthPercentage::Px(4.0),
            right: LengthPercentage::Px(5.0),
            bottom: LengthPercentage::Px(4.0),
            left: LengthPercentage::Px(5.0),
        }
    );
    assert_eq!(
        all.background_color,
        StyleColor::Rgba(Color::rgba(0, 255, 0, 128))
    );
    assert_eq!(style_of(&s, "one").margin, Sides::all(px(7.0)));
    assert_eq!(
        style_of(&s, "one").padding,
        Sides::all(LengthPercentage::ZERO)
    );
    let auto = LengthPercentageOrAuto::Auto;
    assert_eq!(
        style_of(&s, "two").margin,
        Sides {
            top: px(1.0),
            right: auto,
            bottom: px(1.0),
            left: auto
        }
    );
    assert_eq!(
        style_of(&s, "four").margin,
        Sides {
            top: px(1.0),
            right: px(2.0),
            bottom: px(3.0),
            left: px(-4.0)
        }
    );
}

#[test]
fn css_wide_keywords() {
    let s = styles(
        "<style>div { width: 5px; padding: 3px }</style>
         <div id=parent style='width: 40px; padding: 2px'>
           <div id=inherit style='width: inherit; padding: inherit'></div>
           <div id=initial style='width: initial; padding: initial'></div>
           <div id=unset style='width: unset; margin: unset'></div>
         </div>",
    );
    assert_eq!(width(&s, "inherit"), px(40.0));
    assert_eq!(
        style_of(&s, "inherit").padding,
        Sides::all(LengthPercentage::Px(2.0))
    );
    assert_eq!(width(&s, "initial"), LengthPercentageOrAuto::Auto);
    assert_eq!(
        style_of(&s, "initial").padding,
        Sides::all(LengthPercentage::ZERO)
    );
    // Nothing supported is inherited yet, so `unset` is `initial`.
    assert_eq!(width(&s, "unset"), LengthPercentageOrAuto::Auto);
}

#[test]
fn what_is_ignored() {
    let s = styles(
        "<style>
           div { width: 1px; color: red; display: ruby; height: 2px; padding: 5px; padding: -1px }
           @media print { div { width: 99px } }
           @unknown { div { width: 99px } }
           div::before { width: 98px }
           div, p:in-range { width: 97px }
           div { margin: 1px 2px 3px 4px 5px; min-width: auto 10px }
         </style>
         <style type=text/foo>div { width: 96px }</style>
         <style type=TEXT/CSS>div { max-width: 3px }</style>
         <div id=d></div>
         <template><style>div { width: 95px }</style><div id=in-template></div></template>",
    );
    let d = style_of(&s, "d");
    // Unsupported properties and values are dropped; the rest of the
    // block still applies.
    assert_eq!(d.width, size_px(1.0));
    assert_eq!(d.height, size_px(2.0));
    assert_eq!(d.display, Display::Block);
    // An invalid declaration doesn't override an earlier valid one.
    assert_eq!(d.padding, Sides::all(LengthPercentage::Px(5.0)));
    assert_eq!(d.margin, Sides::all(px(0.0)));
    assert_eq!(d.min_width, LengthPercentageOrAuto::Auto);
    // `type=text/css` is case-insensitive; other types are not CSS.
    assert_eq!(
        d.max_width,
        LengthPercentageOrNone::LengthPercentage(LengthPercentage::Px(3.0))
    );
    // Template contents are not in the document: not styled, and their
    // `<style>` doesn't apply.
    assert_eq!(s["in-template"], None);
}

#[test]
fn quirks_mode_matches_classes_and_ids_case_insensitively() {
    let html = "<style>.Foo { width: 1px } #E { width: 2px }</style>
                <div id=d class=foo></div><div id=e></div>";
    let quirks = styles(html);
    assert_eq!(width(&quirks, "d"), px(1.0));
    assert_eq!(width(&quirks, "e"), px(2.0));
    let standards = styles(&format!("<!DOCTYPE html>{html}"));
    assert_eq!(width(&standards, "d"), LengthPercentageOrAuto::Auto);
    assert_eq!(width(&standards, "e"), LengthPercentageOrAuto::Auto);
}

/// `[|name]` is the attribute in the null namespace only: SVG's
/// `xlink:href` is in XLink's; `[*|name]` is in any.
#[test]
fn attribute_selectors_with_a_namespace() {
    let s = styles(
        "<!DOCTYPE html><style>[|href] { width: 1px } [*|href] { height: 2px }</style>
         <svg><a id=x xlink:href=u></a><a id=n href=u></a></svg>",
    );
    assert_eq!(width(&s, "x"), LengthPercentageOrAuto::Auto);
    assert_eq!(style_of(&s, "x").height, size_px(2.0));
    assert_eq!(width(&s, "n"), px(1.0));
}

#[test]
fn recalc_replaces_styles() {
    let mut page = Page::new("<style>.wide { width: 9px }</style><div id=d></div>");
    assert_eq!(page.style("d").width, Size::Auto);
    page.with("d", |cx, d| cx.set_attribute(d, "class", "wide"));
    page.recalc();
    assert_eq!(page.style("d").width, size_px(9.0));
}

#[test]
fn system_fonts_compute_to_the_system_ui_font() {
    let s = styles(
        "<div style='font: italic bold 20px/3 serif'>
           <div id=control style='font: -webkit-small-control'></div>
           <div id=menu style='font: menu'></div>
           <div id=caption style='font: caption'></div>
           <div id=set-after style='font: menu; font-size: 30px'></div>
         </div>",
    );
    let style = |id| style_of(&s, id);
    let system_ui = [computed_style::FontFamily::Generic(
        computed_style::GenericFamily::SystemUi,
    )];
    for (id, size) in [("control", 40.0 / 3.0), ("menu", 12.0), ("caption", 16.0)] {
        let style = style(id);
        assert_eq!(style.font.size, size, "{id}");
        assert_eq!(*style.font.family, system_ui, "{id}");
        // The rest reset, not inherited.
        assert_eq!(style.font.weight, 400.0, "{id}");
        assert_eq!(style.font.style, computed_style::FontStyle::Normal, "{id}");
        assert_eq!(
            style.line_height,
            computed_style::LineHeight::Normal,
            "{id}"
        );
    }
    assert_eq!(style("set-after").font.size, 30.0);
    assert_eq!(*style("set-after").font.family, system_ui);
}

#[test]
fn system_colors_are_the_engines() {
    // The platform's, as the renderer sets them; each apart, so that one
    // taking another's shows.
    let colors = gfx::SystemColors {
        button_face: Color::rgb(1, 0, 0),
        button_text: Color::rgb(2, 0, 0),
        field: Color::rgb(3, 0, 0),
        gray_text: Color::rgb(4, 0, 0),
        highlight: Color::rgb(5, 0, 0),
        ..gfx::SystemColors::DEFAULT
    };
    let mut page = Page::parsed(
        "<style>
           #a { color: ButtonText; background-color: buttonface; border: 1px solid graytext }
           #b { outline-color: highlight; box-shadow: 1px 1px field }
         </style>
         <div id=a><span id=child></span></div><div id=b></div>",
    );
    page.engine.set_system_colors(colors);
    page.recalc();
    let s = owned(page.all_styles());
    let style = |id| style_of(&s, id);
    assert_eq!(style("a").color, colors.button_text);
    assert_eq!(
        style("a").background_color,
        StyleColor::Rgba(colors.button_face)
    );
    assert_eq!(
        style("a").border_color.top,
        StyleColor::Rgba(colors.gray_text)
    );
    // Inherited as the color it computed to.
    assert_eq!(style("child").color, colors.button_text);
    assert_eq!(style("b").outline_color, StyleColor::Rgba(colors.highlight));
    assert_eq!(
        style("b").box_shadow[0].color,
        StyleColor::Rgba(colors.field)
    );
}

#[test]
fn color_inherits_and_currentcolor_resolves_late() {
    let s = styles(
        "<div id=parent style='color: green; background-color: currentcolor'>
           <div id=child style='border: 1px solid'></div>
           <div id=unset style='color: blue; width: 5px'><div id=grandchild style='color: unset'></div></div>
           <div id=current style='color: currentcolor'></div>
           <div id=initial style='color: initial'></div>
           <div id=inherit-border style='color: blue; border-color: inherit'></div>
         </div>",
    );
    let green = Color::rgb(0, 128, 0);
    let style = |id| style_of(&s, id);
    assert_eq!(style("parent").color, green);
    assert_eq!(
        style("parent").resolve(style("parent").background_color),
        green
    );
    // Inherited, and only `color` is.
    assert_eq!(style("child").color, green);
    assert_eq!(
        style("child").background_color,
        StyleColor::Rgba(Color::TRANSPARENT)
    );
    assert_eq!(style("grandchild").width, Size::Auto);
    assert_eq!(style("grandchild").color, Color::rgb(0, 0, 255));
    assert_eq!(style("current").color, green);
    assert_eq!(style("initial").color, Color::BLACK);
    // `border-color` defaults to `currentcolor`, resolved with the
    // element's own color...
    assert_eq!(
        style("child").border_color,
        Sides::all(StyleColor::CurrentColor)
    );
    // ...and `inherit` copies the keyword, not the parent's green.
    let inherit = style("inherit-border");
    assert_eq!(inherit.border_color, Sides::all(StyleColor::CurrentColor));
    assert_eq!(
        inherit.resolve(inherit.border_color.top),
        Color::rgb(0, 0, 255)
    );
}

#[test]
fn borders() {
    let s = styles(
        "<div id=shorthand style='border: 2px solid red'></div>
         <div id=no-style style='border-width: 7px; border-color: red'></div>
         <div id=keywords style='border-style: solid; border-width: thin medium thick 1em'></div>
         <div id=side style='border-top: solid; border-left: 4px blue solid; border-right: 9px'></div>
         <div id=mixed style='border-style: solid none; border-width: 3px; border-color: red blue'></div>
         <div id=invalid style='border-width: 10%; border-top-style: wavy; border: 1px 2px solid'></div>",
    );
    let red = StyleColor::Rgba(Color::rgb(255, 0, 0));
    let blue = StyleColor::Rgba(Color::rgb(0, 0, 255));
    let solid = computed_style::BorderStyle::Solid;
    let none = computed_style::BorderStyle::None;
    let shorthand = style_of(&s, "shorthand");
    assert_eq!(shorthand.border_width, Sides::all(2.0));
    assert_eq!(shorthand.border_style, Sides::all(solid));
    assert_eq!(shorthand.border_color, Sides::all(red));
    // A width without a style computes to 0.
    assert_eq!(style_of(&s, "no-style").border_width, Sides::all(0.0));
    assert_eq!(
        style_of(&s, "keywords").border_width,
        Sides {
            top: 1.0,
            right: 3.0,
            bottom: 5.0,
            left: 16.0
        }
    );
    // Omitted parts of a side shorthand reset to their initial values.
    let side = style_of(&s, "side");
    assert_eq!(
        side.border_width,
        Sides {
            top: 3.0,
            right: 0.0,
            bottom: 0.0,
            left: 4.0
        }
    );
    assert_eq!(
        side.border_style,
        Sides {
            top: solid,
            right: none,
            bottom: none,
            left: solid
        }
    );
    assert_eq!(side.border_color.top, StyleColor::CurrentColor);
    assert_eq!(side.border_color.left, blue);
    let mixed = style_of(&s, "mixed");
    assert_eq!(
        mixed.border_width,
        Sides {
            top: 3.0,
            right: 0.0,
            bottom: 3.0,
            left: 0.0
        }
    );
    assert_eq!(
        mixed.border_color,
        Sides {
            top: red,
            right: blue,
            bottom: red,
            left: blue
        }
    );
    // Percentages, unsupported styles and two widths are all invalid.
    assert_eq!(style_of(&s, "invalid").border_style, Sides::all(none));
}

/// The background images to load are those of rendered elements only:
/// not of one that is `display: none`, nor of one in such a subtree
/// (Chrome loads neither); once shown, it is styled again and its image
/// is asked for then.
#[test]
fn background_images_are_asked_for_where_rendered() {
    let mut page = Page::new(
        "<div style='background: url(shown.png)'></div>
         <div id=outer style='display: none'>
           <div style='background: url(inner.png)'></div>
         </div>
         <div style='display: none; background: url(none.png)'></div>",
    );
    let taken = |page: &mut Page| -> Vec<String> {
        page.engine
            .take_background_image_urls()
            .into_iter()
            .map(|(_, image)| String::from(&*image.url))
            .collect()
    };
    assert_eq!(taken(&mut page), ["shown.png"]);
    page.with("outer", |cx, outer| cx.set_attribute(outer, "style", ""));
    page.recalc();
    assert_eq!(taken(&mut page), ["inner.png"]);
}

#[test]
fn background_shorthand_resets_every_longhand() {
    let s = styles(
        "<div id=color style='background: green'></div>
         <div id=none style='background-color: red; background: none'></div>
         <div id=image style='background-color: green; background: url(x.png)'></div>
         <div id=layers style='background: url(a.png) right 10px top / 20px no-repeat fixed
                                content-box, linear-gradient(red, blue) padding-box border-box
                                yellow'></div>",
    );
    let green = StyleColor::Rgba(Color::rgb(0, 128, 0));
    assert_eq!(style_of(&s, "color").background_color, green);
    assert_eq!(
        style_of(&s, "none").background_color,
        StyleColor::Rgba(Color::TRANSPARENT)
    );
    // Left out, the color is reset.
    let image = style_of(&s, "image");
    assert_eq!(image.background_color, StyleColor::Rgba(Color::TRANSPARENT));
    // No base URL to resolve against: as written.
    assert_eq!(
        *image.backgrounds.background_image,
        [BackgroundImage::Url("x.png".into())]
    );

    let layers = style_of(&s, "layers");
    assert_eq!(
        layers.background_color,
        StyleColor::Rgba(Color::rgb(255, 255, 0))
    );
    let [top, bottom] = &layers.background_layers()[..] else {
        panic!("two layers")
    };
    assert_eq!(top.image, BackgroundImage::Url("a.png".into()));
    assert_eq!(
        top.position_x,
        PositionComponent {
            from_end: true,
            offset: LengthPercentage::Px(10.0)
        }
    );
    assert_eq!(top.position_y, PositionComponent::START);
    assert_eq!(
        top.size,
        BackgroundSize::Explicit(px(20.0), LengthPercentageOrAuto::Auto)
    );
    assert_eq!(
        top.repeat,
        (BackgroundRepeat::NoRepeat, BackgroundRepeat::NoRepeat)
    );
    assert_eq!(top.attachment, BackgroundAttachment::Fixed);
    // One box is both.
    assert_eq!(
        (top.origin, top.clip),
        (BackgroundBox::ContentBox, BackgroundBox::ContentBox)
    );
    assert!(matches!(bottom.image, BackgroundImage::Gradient(_)));
    // What a layer left out is reset.
    assert_eq!(bottom.size, BackgroundSize::AUTO);
    assert_eq!(bottom.attachment, BackgroundAttachment::Scroll);
    assert_eq!(
        (bottom.origin, bottom.clip),
        (BackgroundBox::PaddingBox, BackgroundBox::BorderBox)
    );
}

#[test]
fn background_lists_repeat_to_the_number_of_images() {
    let s = styles(
        "<div id=x style='background-image: url(a), url(b), url(c);
                          background-repeat: no-repeat, repeat-x;
                          background-size: cover'></div>",
    );
    let layers = style_of(&s, "x").background_layers();
    assert_eq!(layers.len(), 3);
    let (no, x) = (
        (BackgroundRepeat::NoRepeat, BackgroundRepeat::NoRepeat),
        (BackgroundRepeat::Repeat, BackgroundRepeat::NoRepeat),
    );
    assert_eq!(
        layers.iter().map(|l| l.repeat).collect::<Vec<_>>(),
        [no, x, no]
    );
    assert!(layers.iter().all(|l| l.size == BackgroundSize::Cover));
    // The style keeps the lists as declared.
    assert_eq!(style_of(&s, "x").backgrounds.background_repeat.len(), 2);
}

/// A `url()` resolves as it is parsed (Blink's `CSSUrlData`): a linked
/// sheet's against the sheet's URL, a `<style>`'s and a `style`
/// attribute's against the document's base URL of then. Another base URL
/// later (`pushState`, a `<base>`) changes none of them, as in Chrome, and
/// styles nothing again; a `style` attribute set after resolves against
/// it, as does a `<style>` whose text changes.
#[test]
fn urls_resolve_as_they_are_parsed() {
    let mut page = Page::parsed(
        "<link rel=stylesheet href=css/sheet.css>
         <style id=sheet>#inline { background-image: url(img/a.png) }
           #anim { animation: k 100s } @keyframes k { from, to { background-image: url(img/k.png) } }
           #clip { clip-path: url(clip.svg#c) }
           #clip-local { clip-path: url(#c); background-image: url(img/l.png) }</style>
         <style>#kept { background-image: url(img/g.png) }</style>
         <div id=inline></div>
         <div id=attribute></div>
         <div id=linked></div>
         <div id=var></div>
         <div id=anim></div>
         <div id=absolute></div>
         <div id=kept></div>
         <div id=clip></div>
         <div id=clip-local></div>
         <div id=host></div>
         <svg><rect id=rect></rect><rect id=local fill='url(#p)'></rect></svg>",
    );
    let set_style = |page: &mut Page, id: &str, style: &str| {
        page.with(id, |cx, element| cx.set_attribute(element, "style", style));
    };
    page.with_document(|cx, document| {
        cx.set_document_url(document, "http://example.test/dir/page.html");
    });
    set_style(&mut page, "attribute", "background: url(b.png)");
    set_style(
        &mut page,
        "absolute",
        "background-image: url(http://other.test/c.png)",
    );
    let set_fill = |page: &mut Page, fill: &str| {
        page.with("rect", |cx, rect| cx.set_attribute(rect, "fill", fill));
    };
    set_fill(&mut page, "url(paint.svg#p) red");
    page.with("host", |cx, host| {
        crate::attach_shadow(
            cx,
            host,
            "<style id=changing>#in { background-image: url(in.png) }</style>
             <style>#in-kept { background-image: url(in-kept.png) }</style>
             <div id=in></div><div id=in-kept></div>",
        );
    });
    let fill = |styles: &BTreeMap<String, Option<ComputedStyle>>| {
        style_resolver::computed_value(style_of(styles, "rect"), "fill").unwrap()
    };
    let sheet_url = url::Url::parse("http://example.test/dir/css/sheet.css").unwrap();
    page.with_document(|cx, document| {
        let mut link = None;
        crate::each_element(cx, document, &mut |element| {
            if link.is_none() && cx.stylesheet_link_href(element).is_some() {
                link = Some(element);
            }
        });
        let contents = StyleSheetContents::parse(
            "#linked { background-image: url(../img/d.png) }
             #var { --image: url(../img/v.png); background-image: var(--image) }",
            Some(UrlContext::of_sheet(sheet_url.clone())),
        );
        cx.set_link_sheet(
            link.expect("the <link>"),
            Arc::new(contents),
            sheet_url.as_str(),
            true,
        );
    });
    let recalc = |page: &mut Page, base: &str| {
        page.set_url_context(Some(UrlContext::of_sheet(url::Url::parse(base).unwrap())));
        page.recalc();
        owned(page.all_styles())
    };
    let image = |styles: &BTreeMap<String, Option<ComputedStyle>>, id: &str| {
        style_of(styles, id).backgrounds.background_image[0].clone()
    };
    let url = |u: &str| BackgroundImage::Url(u.into());
    let s = recalc(&mut page, "http://example.test/dir/page.html");
    assert_eq!(
        image(&s, "inline"),
        url("http://example.test/dir/img/a.png")
    );
    assert_eq!(image(&s, "attribute"), url("http://example.test/dir/b.png"));
    assert_eq!(
        image(&s, "linked"),
        url("http://example.test/dir/img/d.png")
    );
    assert_eq!(image(&s, "absolute"), url("http://other.test/c.png"));
    // Substituted for `var()`: against where the `var()` is.
    assert_eq!(image(&s, "var"), url("http://example.test/dir/img/v.png"));
    assert_eq!(image(&s, "anim"), url("http://example.test/dir/img/k.png"));
    assert_eq!(image(&s, "in"), url("http://example.test/dir/in.png"));
    // A presentation attribute's, as it is parsed for the cascade; a
    // fragment alone stays as written (an element of the document).
    assert_eq!(
        style_resolver::computed_value(style_of(&s, "local"), "fill").unwrap(),
        "url(\"#p\")"
    );
    let clip = |id| style_resolver::computed_value(style_of(&s, id), "clip-path").unwrap();
    assert_eq!(clip("clip"), "url(\"http://example.test/dir/clip.svg#c\")");
    // Alone, or with another `url()` to resolve in its block.
    assert_eq!(clip("clip-local"), "url(\"#c\")");
    assert_eq!(
        fill(&s),
        "url(\"http://example.test/dir/paint.svg#p\") rgb(255, 0, 0)"
    );

    page.with_document(|cx, document| cx.set_document_url(document, "http://moved.test/"));
    let moved = recalc(&mut page, "http://moved.test/");
    assert_eq!(moved, s, "what was parsed keeps what it resolved to");

    let text = |page: &mut Page, id: &str| -> dom::NodeRoot {
        page.with(id, |cx, style| {
            elise::Persistent::new(cx.gc, cx.first_child(style).unwrap())
        })
    };
    let (sheet, changing) = (text(&mut page, "sheet"), text(&mut page, "changing"));
    set_style(&mut page, "attribute", "background: url(e.png)");
    set_fill(&mut page, "url(other.svg#p) red");
    page.with_document(|cx, _| {
        cx.replace_data(
            sheet.get(cx.gc),
            0,
            10_000,
            "#inline { background-image: url(img/f.png) }",
        );
        cx.replace_data(
            changing.get(cx.gc),
            0,
            10_000,
            "#in { background-image: url(in2.png) }",
        );
    });
    let changed = recalc(&mut page, "http://moved.test/");
    assert_eq!(image(&changed, "attribute"), url("http://moved.test/e.png"));
    assert_eq!(
        image(&changed, "inline"),
        url("http://moved.test/img/f.png")
    );
    assert_eq!(image(&changed, "in"), url("http://moved.test/in2.png"));
    // A `<style>` whose text didn't change keeps its sheet, in a shadow tree
    // too.
    assert_eq!(
        image(&changed, "kept"),
        url("http://example.test/dir/img/g.png")
    );
    assert_eq!(
        image(&changed, "in-kept"),
        url("http://example.test/dir/in-kept.png")
    );
    assert_eq!(
        fill(&changed),
        "url(\"http://moved.test/other.svg#p\") rgb(255, 0, 0)"
    );
    assert_eq!(
        image(&changed, "linked"),
        url("http://example.test/dir/img/d.png")
    );
}

#[test]
fn gradients_compute_their_lengths_and_defaults() {
    let s = styles(
        "<div id=linear style='font-size: 10px;
           background-image: linear-gradient(red, 2em, blue 50%)'></div>
         <div id=turn style='background-image: repeating-linear-gradient(0.5turn, red, blue)'></div>
         <div id=radial style='font-size: 10px;
           background-image: radial-gradient(2em at right 1em top, red, blue)'></div>
         <div id=bad style='background-image: url(ok.png);
           background-image: linear-gradient(red)'></div>",
    );
    let gradient = |id: &str| match &style_of(&s, id).backgrounds.background_image[0] {
        BackgroundImage::Gradient(gradient) => (**gradient).clone(),
        other => panic!("#{id}: {other:?}"),
    };
    let red = StyleColor::Rgba(Color::rgb(255, 0, 0));
    let blue = StyleColor::Rgba(Color::rgb(0, 0, 255));
    let linear = gradient("linear");
    assert_eq!(
        linear.kind,
        GradientKind::Linear(LinearDirection::To(None, Some(VerticalSide::Bottom)))
    );
    assert!(!linear.repeating);
    assert_eq!(
        *linear.items,
        [
            GradientItem::Stop(red, None),
            GradientItem::Hint(LengthPercentage::Px(20.0)),
            GradientItem::Stop(blue, Some(LengthPercentage::Percent(50.0))),
        ]
    );
    let turn = gradient("turn");
    assert!(turn.repeating);
    assert_eq!(
        turn.kind,
        GradientKind::Linear(LinearDirection::Angle(180.0))
    );
    assert_eq!(
        gradient("radial").kind,
        GradientKind::Radial {
            shape: RadialShape::Circle,
            size: RadialSize::Circle(20.0),
            position: Position2D {
                x: PositionComponent {
                    from_end: true,
                    offset: LengthPercentage::Px(10.0),
                },
                y: PositionComponent::START,
            },
        }
    );
    // An invalid gradient drops the declaration.
    assert_eq!(
        *style_of(&s, "bad").backgrounds.background_image,
        [BackgroundImage::Url("ok.png".into())]
    );
}

#[test]
fn pseudo_classes() {
    let s = styles(
        "<style>
           a:link { width: 1px }
           a:any-link { height: 2px }
           a:visited, div:hover, div:focus, div:active { width: 99px }
           :is(.x, .y) { width: 3px }
           :where(#w) { width: 4px }
           .w { width: 5px }
           div:has(> .child) { width: 6px }
           div:not(.x):first-child { height: 7px }
         </style>
         <div id=first-div><a id=link href=x></a><a id=anchor></a><area id=area href=y></div>
         <div id=x class=x></div>
         <div id=w class=w></div>
         <div id=has><p class=child></p></div>",
    );
    assert_eq!(width(&s, "link"), px(1.0));
    assert_eq!(style_of(&s, "link").height, size_px(2.0));
    // `:link` needs an `href`; nothing is visited, hovered, focused or
    // active.
    assert_eq!(width(&s, "anchor"), LengthPercentageOrAuto::Auto);
    assert_eq!(width(&s, "first-div"), LengthPercentageOrAuto::Auto);
    assert_eq!(width(&s, "x"), px(3.0));
    // `:where()` has no specificity, so the later-but-weaker `.w` wins.
    assert_eq!(width(&s, "w"), px(5.0));
    assert_eq!(width(&s, "has"), px(6.0));
    assert_eq!(style_of(&s, "first-div").height, size_px(7.0));
    assert_eq!(style_of(&s, "x").height, Size::Auto);
}

#[test]
fn user_agent_box_rules() {
    let s = styles(
        "<body id=body><p id=p></p><ul id=ul><li><ul id=nested></ul></ul>
         <a id=a href=x></a><hr id=hr><blockquote id=bq></blockquote>",
    );
    let margin = |id| style_of(&s, id).margin;
    assert_eq!(margin("body"), Sides::all(px(8.0)));
    assert_eq!(
        margin("p"),
        Sides {
            top: px(16.0),
            right: px(0.0),
            bottom: px(16.0),
            left: px(0.0)
        }
    );
    assert_eq!(style_of(&s, "ul").padding.left, LengthPercentage::Px(40.0));
    assert_eq!(margin("nested").top, px(0.0));
    assert_eq!(margin("bq").left, px(40.0));
    assert_eq!(style_of(&s, "a").color, Color::from_hex(0x0000ee));
    let hr = style_of(&s, "hr");
    assert_eq!(hr.border_width, Sides::all(1.0));
    assert_eq!(hr.resolve(hr.border_color.top), Color::rgb(128, 128, 128));
    assert_eq!(hr.margin.left, LengthPercentageOrAuto::Auto);
}

#[test]
fn font_properties() {
    use computed_style::{FontFamily, FontStyle, GenericFamily, LineHeight, TextAlign, WhiteSpace};
    let s = styles(
        "<html id=html style='font-size: 20px'>
         <div id=parent style='font-size: 10px; line-height: 1.5; text-align: center'>
           <div id=em style='font-size: 2em; margin-top: 1em; width: 1rem'></div>
           <div id=percent style='font-size: 150%'></div>
           <div id=keyword style='font-size: large'></div>
           <div id=larger style='font-size: larger'></div>
           <div id=inherited></div>
           <div id=lh-em style='font-size: 20px; line-height: 2em'></div>
           <div id=lh-percent style='font-size: 20px; line-height: 150%'></div>
           <div id=shorthand style='font: italic bold 12px/30px \"Ahem Custom\", Ahem, monospace'></div>
           <div id=shorthand-reset style='font-weight: 900; line-height: 5; font: 12px serif'></div>
           <div id=weights style='font-weight: 300'><b id=bolder></b><i id=italic></i></div>
           <div id=invalid style='font: bold; font-size: -1px; line-height: -2'></div>
           <div id=ws style='white-space: pre-line'></div>
         </div>",
    );
    let style = |id| style_of(&s, id);
    // `em` in `font-size` is the parent's; elsewhere it is the element's.
    assert_eq!(style("em").font.size, 20.0);
    assert_eq!(style("em").margin.top, px(20.0));
    // `rem` is the root's font size.
    assert_eq!(style("em").width, size_px(20.0));
    assert_eq!(style("percent").font.size, 15.0);
    assert_eq!(style("keyword").font.size, 18.0);
    assert_eq!(style("larger").font.size, 12.0);
    // Inherited: the number stays a number, so it scales with the child.
    assert_eq!(style("inherited").font.size, 10.0);
    assert_eq!(style("inherited").line_height, LineHeight::Number(1.5));
    assert_eq!(style("inherited").text_align, TextAlign::Center);
    assert_eq!(style("lh-em").line_height, LineHeight::Px(40.0));
    assert_eq!(style("lh-percent").line_height, LineHeight::Px(30.0));
    let shorthand = style("shorthand");
    assert_eq!(shorthand.font.style, FontStyle::Italic);
    assert_eq!(shorthand.font.weight, 700.0);
    assert_eq!(shorthand.font.size, 12.0);
    assert_eq!(shorthand.line_height, LineHeight::Px(30.0));
    assert_eq!(
        &shorthand.font.family[..],
        [
            FontFamily::Named("Ahem Custom".into()),
            FontFamily::Named("Ahem".into()),
            FontFamily::Generic(GenericFamily::Monospace),
        ]
    );
    // The shorthand resets what it doesn't mention.
    assert_eq!(style("shorthand-reset").font.weight, 400.0);
    assert_eq!(style("shorthand-reset").line_height, LineHeight::Normal);
    // `b` is `bolder` in the UA stylesheet: 300 -> 400. `i` is italic.
    assert_eq!(style("bolder").font.weight, 400.0);
    assert_eq!(style("italic").font.style, FontStyle::Italic);
    let invalid = style("invalid");
    assert_eq!((invalid.font.size, invalid.font.weight), (10.0, 400.0));
    let ws = style("ws");
    assert_eq!(
        (ws.white_space_collapse, ws.text_wrap_mode),
        WhiteSpace::PreLine.longhands()
    );
}

/// Each element with a `text-decoration-line` adds a decoration of its
/// own to its ancestors' (the decorating box's): its lines, style,
/// thickness, `text-underline-offset`, and color, `currentcolor` its own
/// `color` (what a descendant's `color` doesn't change).
#[test]
fn text_decorations_propagate() {
    use computed_style::{
        AppliedTextDecoration, LengthPercentage, TextDecorationLine, TextDecorationStyle,
        TextDecorationThickness,
    };
    let s = styles(
        "<div id=u style='text-decoration: underline; color: red'>
           <span id=child style='text-decoration: line-through wavy 3px blue; text-underline-offset: 2px; color: green'></span>
           <div id=block style='color: green'></div>
         </div>
         <div id=none style='text-decoration: underline overline; text-decoration: none'></div>
         <a id=link href=x></a>",
    );
    let applied = |id| {
        style_of(&s, id)
            .rare_inherited
            .applied_text_decorations
            .to_vec()
    };
    let lines = |id| applied(id).iter().map(|d| d.lines).collect::<Vec<_>>();
    let underline = TextDecorationLine {
        underline: true,
        ..TextDecorationLine::NONE
    };
    let red_underline = AppliedTextDecoration {
        lines: underline,
        style: TextDecorationStyle::Solid,
        color: gfx::Color::rgb(255, 0, 0),
        thickness: TextDecorationThickness::Auto,
        underline_offset: None,
    };
    assert_eq!(applied("u"), [red_underline]);
    // Not inherited, but the ancestors' decorations still apply.
    assert!(!style_of(&s, "child").text_decoration_line.underline);
    assert_eq!(
        applied("child"),
        [
            red_underline,
            AppliedTextDecoration {
                lines: TextDecorationLine {
                    line_through: true,
                    ..TextDecorationLine::NONE
                },
                style: TextDecorationStyle::Wavy,
                color: gfx::Color::rgb(0, 0, 255),
                thickness: TextDecorationThickness::Length(LengthPercentage::Px(3.0)),
                underline_offset: Some(LengthPercentage::Px(2.0)),
            }
        ]
    );
    assert_eq!(applied("block"), [red_underline]);
    assert_eq!(lines("none"), []);
    assert_eq!(lines("link"), [underline]);
}

/// An ancestor's decorations stop at an atomic inline, a float and an
/// out-of-flow box (with what's inside them), which keep their own; a flex
/// item is blockified first and gets them, and a control's `inline` is
/// checked before the theme makes it `inline-block` (as Blink).
#[test]
fn text_decorations_stop_at_atomic_inlines() {
    use computed_style::TextDecorationLine;
    let s = styles(
        "<u id=u>
           <span id=inline></span>
           <span id=inline-block style='display: inline-block'><span id=inside></span></span>
           <span id=own style='display: inline-block; text-decoration: overline'></span>
           <span id=inline-flex style='display: inline-flex'></span>
           <span id=inline-grid style='display: inline-grid'></span>
           <span id=inline-table style='display: inline-table'></span>
           <span id=float style='float: left'></span>
           <span id=absolute style='position: absolute'></span>
           <span id=fixed style='position: fixed'></span>
           <span id=relative style='position: relative'></span>
           <button id=button></button>
           <button id=inline-button style='display: inline'></button>
           <span style='display: flex'><span id=item style='display: inline-block'></span></span>
         </u>",
    );
    let applied = |id| {
        style_of(&s, id)
            .rare_inherited
            .applied_text_decorations
            .iter()
            .map(|d| d.lines)
            .collect::<Vec<_>>()
    };
    let underline = TextDecorationLine {
        underline: true,
        ..TextDecorationLine::NONE
    };
    for id in ["u", "inline", "relative", "inline-button", "item"] {
        assert_eq!(applied(id), [underline], "#{id}");
    }
    for id in [
        "inline-block",
        "inside",
        "inline-flex",
        "inline-grid",
        "inline-table",
        "float",
        "absolute",
        "fixed",
        "button",
    ] {
        assert_eq!(applied(id), [], "#{id}");
    }
    assert_eq!(
        applied("own"),
        [TextDecorationLine {
            overline: true,
            ..TextDecorationLine::NONE
        }]
    );
}

#[test]
fn user_agent_text_rules() {
    use computed_style::{FontFamily, GenericFamily, WhiteSpace};
    let s = styles(
        "<body><h1 id=h1></h1><section><h1 id=section-h1></h1></section>
         <pre id=pre></pre><code id=code></code><small id=small></small><mark id=mark></mark>",
    );
    let h1 = style_of(&s, "h1");
    assert_eq!((h1.font.size, h1.font.weight), (32.0, 700.0));
    // Heading margins are in the heading's own `em` now.
    assert_eq!(h1.margin.top, px(0.67 * 32.0));
    assert_eq!(style_of(&s, "section-h1").font.size, 24.0);
    let pre = style_of(&s, "pre");
    assert_eq!(
        (pre.white_space_collapse, pre.text_wrap_mode),
        WhiteSpace::Pre.longhands()
    );
    assert_eq!(
        &style_of(&s, "code").font.family[..],
        [FontFamily::Generic(GenericFamily::Monospace)]
    );
    assert!((style_of(&s, "small").font.size - 16.0 / 1.2).abs() < 1e-4);
    // `Mark` and `MarkText`.
    let mark = style_of(&s, "mark");
    assert_eq!(
        (mark.background_color, mark.color),
        (StyleColor::Rgba(Color::rgb(255, 255, 0)), Color::BLACK)
    );
}

#[test]
fn effect_properties() {
    use computed_style::{Overflow, TransformOperation};
    let s = styles(
        "<div id=opacity style='opacity: 0.25'></div>
         <div id=percent style='opacity: 150%'></div>
         <div id=negative style='opacity: -2'></div>
         <div id=one style='overflow: hidden'></div>
         <div id=mixed style='overflow: visible hidden'></div>
         <div id=scroll style='overflow: hidden; overflow: scroll'></div>
         <div id=auto style='overflow: auto clip'></div>
         <div id=overlay style='overflow: overlay'></div>
         <div id=clip style='overflow: clip'></div>
         <div id=clip-x style='overflow: clip visible'></div>
         <img id=img>
         <div id=transform style='transform: translate(10px, 50%) scale(2) translateY(1em) scaleX(3) matrix(1, 0, 0, 1, 5, 6)'></div>
         <div id=none style='transform: translate(1px); transform: none'></div>
         <div id=translate-x style='transform: translate(7px)'></div>
         <div id=rotate style='transform: rotate(0.25turn) rotateZ(0) skew(10deg) skewX(0) skewY(1rad)'></div>
         <div id=invalid style='transform: translate3d(1px, 2px, 3px) rotate(45deg)'></div>",
    );
    let style = |id| style_of(&s, id);
    assert_eq!(style("opacity").opacity, 0.25);
    assert!(style("opacity").is_stacking_context());
    // Clamped.
    assert_eq!(style("percent").opacity, 1.0);
    assert_eq!(style("negative").opacity, 0.0);
    assert_eq!(
        (style("one").overflow_x, style("one").overflow_y),
        (Overflow::Hidden, Overflow::Hidden)
    );
    // `visible` next to `hidden` becomes `auto`: it clips too, and can
    // scroll.
    assert_eq!(
        (style("mixed").overflow_x, style("mixed").overflow_y),
        (Overflow::Auto, Overflow::Hidden)
    );
    assert_eq!(style("scroll").overflow_y, Overflow::Scroll);
    assert!(style("scroll").overflow_y.user_scrollable());
    // `clip` next to an axis that scrolls is `hidden`; neither is
    // scrollable by the user.
    assert_eq!(
        (style("auto").overflow_x, style("auto").overflow_y),
        (Overflow::Auto, Overflow::Hidden)
    );
    // Alone, or next to `visible`, it stays: it clips without being a
    // scroll container, one axis only if the other is `visible`.
    assert_eq!(
        (style("clip").overflow_x, style("clip").overflow_y),
        (Overflow::Clip, Overflow::Clip)
    );
    assert_eq!(
        (style("clip-x").overflow_x, style("clip-x").overflow_y),
        (Overflow::Clip, Overflow::Visible)
    );
    assert!(style("clip").clips_overflow() && !style("clip").is_scroll_container());
    assert!(style("one").is_scroll_container());
    // A replaced element clips its content (the UA stylesheet).
    assert_eq!(style("img").overflow_x, Overflow::Clip);
    assert!(!Overflow::Hidden.user_scrollable());
    // `overlay` is `auto`.
    assert_eq!(style("overlay").overflow_y, Overflow::Auto);
    let px = |v| LengthPercentage::Px(v);
    assert_eq!(
        &style("transform").transform[..],
        [
            TransformOperation::Translate(px(10.0), LengthPercentage::Percent(50.0)),
            TransformOperation::Scale(2.0, 2.0),
            TransformOperation::Translate(LengthPercentage::Percent(0.0), px(16.0)),
            TransformOperation::Scale(3.0, 1.0),
            TransformOperation::Matrix([1.0, 0.0, 0.0, 1.0, 5.0, 6.0]),
        ]
    );
    // `translate(x)` doesn't move along y.
    assert_eq!(
        &style("translate-x").transform[..],
        [TransformOperation::Translate(
            px(7.0),
            LengthPercentage::Percent(0.0)
        )]
    );
    assert!(style("none").transform.is_empty());
    assert!(!style("none").is_stacking_context());
    assert_eq!(
        &*style("rotate").transform,
        [
            TransformOperation::Rotate(90.0),
            TransformOperation::Rotate(0.0),
            TransformOperation::Skew(10.0, 0.0),
            TransformOperation::SkewX(0.0),
            TransformOperation::SkewY(1f32.to_degrees()),
        ]
    );
    // Only 2D functions: a 3D one drops the whole declaration.
    assert!(style("invalid").transform.is_empty());
}

/// Layout and paint containment make a stacking context, on a box they
/// apply to: not a non-atomic inline box, nor a table row or column.
#[test]
fn containment_makes_stacking_contexts() {
    let s = styles(
        "<div id=paint style='contain: paint'></div><div id=layout style='contain: layout'></div>
         <div id=size style='contain: size style'></div><span id=inline style='contain: paint'></span>
         <span id=inline-block style='display: inline-block; contain: layout'></span>
         <table><tr id=row style='contain: paint'><td id=cell style='contain: paint'></td></tr>
           <col id=col style='contain: layout'></table>",
    );
    let stacking = |id| style_of(&s, id).is_stacking_context();
    assert!(
        stacking("paint") && stacking("layout") && stacking("inline-block") && stacking("cell")
    );
    assert!(!stacking("size") && !stacking("inline") && !stacking("row") && !stacking("col"));
}

/// `clip-path` makes a stacking context (css-masking 1), `clip` doesn't.
#[test]
fn clip_paths_make_stacking_contexts() {
    let s = styles(
        "<div id=path style='clip-path: inset(1px)'></div><div id=box style='clip-path: content-box'></div>
         <div id=clip style='position: absolute; clip: rect(0, 1px, 1px, 0)'></div>",
    );
    let stacking = |id| style_of(&s, id).is_stacking_context();
    assert!(stacking("path") && stacking("box"));
    assert!(!stacking("clip"));
}

/// `filter` (any function) and `isolation: isolate` make stacking
/// contexts (filter-effects 1, compositing 1).
#[test]
fn filters_and_isolation_make_stacking_contexts() {
    let s = styles(
        "<div id=filter style='filter: opacity(1)'></div><div id=isolate style='isolation: isolate'></div>
         <div id=none style='filter: none; isolation: auto'></div>",
    );
    let stacking = |id| style_of(&s, id).is_stacking_context();
    assert!(stacking("filter") && stacking("isolate"));
    assert!(!stacking("none"));
}

#[test]
fn img_width_and_height_are_presentational_hints() {
    let s = styles(
        "<style>#c { width: 20px }</style>
         <img id=a width=40 height=30>
         <img id=b width=50% height=junk>
         <img id=c width=40>
         <img id=d width=40 style='width: 10px'>
         <div id=e width=40></div>",
    );
    let px = |v| LengthPercentageOrAuto::LengthPercentage(LengthPercentage::Px(v));
    assert_eq!(width(&s, "a"), px(40.0));
    assert_eq!(style_of(&s, "a").height, size_px(30.0));
    assert_eq!(
        width(&s, "b"),
        LengthPercentageOrAuto::LengthPercentage(LengthPercentage::Percent(50.0))
    );
    assert_eq!(style_of(&s, "b").height, Size::Auto);
    // Any author rule or `style` attribute beats a hint, whatever its
    // specificity.
    assert_eq!(width(&s, "c"), px(20.0));
    assert_eq!(width(&s, "d"), px(10.0));
    // Only `<img>` and `<iframe>` map them.
    assert_eq!(width(&s, "e"), LengthPercentageOrAuto::Auto);
}

/// An iframe: the UA stylesheet's `border: 2px inset`; `width` and
/// `height` as an image's; `frameborder` other than a nonzero integer takes
/// the border's width off (Blink's), which an author rule still beats.
#[test]
fn iframe_border_and_presentational_hints() {
    use computed_style::BorderStyle;
    let s = styles(
        "<style>#rule { border-width: 3px }</style>
         <iframe id=plain width=40 height=30></iframe>
         <iframe id=zero frameborder=0></iframe>
         <iframe id=no frameborder=no></iframe>
         <iframe id=one frameborder=1></iframe>
         <iframe id=rule frameborder=0></iframe>",
    );
    let plain = style_of(&s, "plain");
    assert_eq!(plain.border_style, Sides::all(BorderStyle::Inset));
    assert_eq!(plain.border_width, Sides::all(2.0));
    assert_eq!(plain.border_color, Sides::all(StyleColor::CurrentColor));
    assert_eq!(
        width(&s, "plain"),
        LengthPercentageOrAuto::LengthPercentage(LengthPercentage::Px(40.0))
    );
    assert_eq!(plain.height, size_px(30.0));
    for (id, border) in [("zero", 0.0), ("no", 0.0), ("one", 2.0), ("rule", 3.0)] {
        assert_eq!(style_of(&s, id).border_width, Sides::all(border), "#{id}");
    }
}

/// Every flex property's value, to compare.
fn flex_values(s: &ComputedStyle) -> String {
    format!(
        "{:?}",
        (
            s.flex_direction,
            s.flex_wrap,
            s.flex_grow,
            s.flex_shrink,
            s.flex_basis,
            s.order,
            s.justify_content,
            s.align_content,
            s.align_items,
            s.align_self,
            s.row_gap,
            s.column_gap,
        )
    )
}

#[test]
fn flex_properties_take_css_wide_keywords() {
    const SET: &str = "flex-direction: column; flex-wrap: wrap; flex: 2 3 4px; order: 5;
        justify-content: center; align-content: space-around; align-items: flex-end;
        align-self: baseline; gap: 6px 7px";
    const NAMES: [&str; 12] = [
        "flex-direction",
        "flex-wrap",
        "flex-grow",
        "flex-shrink",
        "flex-basis",
        "order",
        "justify-content",
        "align-content",
        "align-items",
        "align-self",
        "row-gap",
        "column-gap",
    ];
    let keyword = |k: &str| NAMES.map(|name| format!("{name}: {k}")).join("; ");
    let s = styles(&format!(
        "<div id=parent style='{SET}'>
           <p id=inherit style='{}'></p>
           <p id=initial style='{SET}; {}'></p>
           <p id=unset style='{SET}; {}'></p>
         </div>",
        keyword("inherit"),
        keyword("initial"),
        keyword("unset"),
    ));
    let flex = |id: &str| flex_values(style_of(&s, id));
    let initial = flex_values(&ComputedStyle::initial());
    assert_ne!(flex("parent"), initial);
    assert_eq!(flex("inherit"), flex("parent"));
    assert_eq!(flex("initial"), initial);
    // None of them is inherited.
    assert_eq!(flex("unset"), initial);
}

#[test]
fn z_index_applies_to_flex_items() {
    let s = styles(
        "<style>.z { z-index: 3 }</style>
         <div id=block class=z><p id=in-block class=z></p></div>
         <div style='display: flex; z-index: 7'>
           <p id=item class=z></p><p id=auto style='z-index: 2; z-index: auto'></p>
           <p id=negative style='z-index: -4'></p><p id=invalid style='z-index: 1.5'></p>
           <p id=inherit style='z-index: inherit'></p>
           <p id=in-item><span id=deeper class=z></span></p>
         </div>",
    );
    let z = |id: &str| {
        let style = style_of(&s, id);
        (style.z_index, style.effective_z_index)
    };
    // Computed everywhere; in effect on flex items only.
    assert_eq!(z("block"), (Some(3), None));
    assert_eq!(z("in-block"), (Some(3), None));
    assert_eq!(z("item"), (Some(3), Some(3)));
    assert_eq!(z("auto"), (None, None));
    assert_eq!(z("negative"), (Some(-4), Some(-4)));
    assert_eq!(z("invalid"), (None, None));
    // `inherit` takes the flex container's, which doesn't apply to it.
    assert_eq!(z("inherit"), (Some(7), Some(7)));
    assert_eq!(z("deeper"), (Some(3), None));
    assert!(style_of(&s, "item").is_stacking_context());
    assert!(!style_of(&s, "block").is_stacking_context());
    assert_eq!(style_of(&s, "negative").stack_level(), -4);
    assert_eq!(style_of(&s, "block").stack_level(), 0);
}

#[test]
fn position_insets_and_z_index_on_positioned_boxes() {
    use computed_style::{LengthPercentageOrAuto, Position};
    let s = styles(
        "<div id=parent style='position: relative; inset: 1px 2% 3px auto; z-index: 4'>
           <p id=inherit style='position: inherit; top: inherit; z-index: inherit'></p>
           <p id=sticky style='position: absolute; position: -webkit-sticky'></p>
           <p id=fixed style='position: fixed'></p>
           <p id=reset style='position: relative; position: static; z-index: 2'></p>
           <p id=one style='inset: 5px; left: auto'></p>
           <p id=absolute style='position: absolute; display: inline'></p>
         </div>",
    );
    let px = |v| LengthPercentageOrAuto::LengthPercentage(LengthPercentage::Px(v));
    let parent = style_of(&s, "parent");
    assert_eq!(parent.position, Position::Relative);
    assert_eq!(
        [
            parent.inset.top,
            parent.inset.right,
            parent.inset.bottom,
            parent.inset.left
        ],
        [
            px(1.0),
            LengthPercentageOrAuto::LengthPercentage(LengthPercentage::Percent(2.0)),
            px(3.0),
            LengthPercentageOrAuto::Auto
        ]
    );
    // `z-index` applies to a positioned box, which is a stacking context
    // with one; with `z-index: auto` it is still stacked.
    assert_eq!(parent.effective_z_index, Some(4));
    assert!(parent.is_stacking_context());
    let inherit = style_of(&s, "inherit");
    assert_eq!(
        (
            inherit.position,
            inherit.inset.top,
            inherit.effective_z_index
        ),
        (Position::Relative, px(1.0), Some(4))
    );
    // `-webkit-sticky` is `sticky` (Chrome's alias). A sticky box is in
    // flow, positioned and a stacking context even with `z-index: auto`;
    // a fixed one is out of flow, and stacked without being one.
    let sticky = style_of(&s, "sticky");
    assert_eq!(sticky.position, Position::Sticky);
    assert!(!sticky.is_out_of_flow() && sticky.is_containing_block());
    assert!(sticky.is_stacking_context());
    let fixed = style_of(&s, "fixed");
    assert!(fixed.is_out_of_flow() && fixed.is_fixed());
    assert!(fixed.is_stacked() && !fixed.is_stacking_context());
    let reset = style_of(&s, "reset");
    assert_eq!(
        (reset.position, reset.effective_z_index),
        (Position::Static, None)
    );
    assert!(!reset.is_stacked());
    // An out-of-flow box is blockified (css-display), as a flex item is.
    let absolute = style_of(&s, "absolute");
    assert_eq!(absolute.position, Position::Absolute);
    assert_eq!(absolute.display, computed_style::Display::Block);
    assert!(absolute.is_out_of_flow() && absolute.is_containing_block());
    let one = style_of(&s, "one");
    assert_eq!(
        [
            one.inset.top,
            one.inset.right,
            one.inset.bottom,
            one.inset.left
        ],
        [px(5.0), px(5.0), px(5.0), LengthPercentageOrAuto::Auto]
    );
}

#[test]
fn intrinsic_size_keywords() {
    let s = styles(
        "<div id=min style='width: min-content; height: max-content'></div>
         <div id=fit style='width: FIT-CONTENT; height: fit-content'></div>
         <div id=basis style='flex-basis: max-content'></div>
         <div id=content style='flex-basis: content'></div>
         <div id=flex-content style='flex: content'></div>
         <div id=invalid style='width: 1px; width: content; width: stretch;
           width: fit-content(10px); width: -webkit-fit-content; height: 2px;
           height: -webkit-fill-available; min-width: 3px; min-width: min-content;
           max-width: 4px; max-width: max-content; margin-left: 5px; margin-left: fit-content'></div>",
    );
    let min = style_of(&s, "min");
    assert_eq!(
        (min.width, min.height),
        (Size::MinContent, Size::MaxContent)
    );
    let fit = style_of(&s, "fit");
    assert_eq!(
        (fit.width, fit.height),
        (Size::FitContent, Size::FitContent)
    );
    assert_eq!(
        style_of(&s, "basis").flex_basis,
        FlexBasis::Size(Size::MaxContent)
    );
    assert_eq!(style_of(&s, "content").flex_basis, FlexBasis::Content);
    let flex = style_of(&s, "flex-content");
    assert_eq!(
        (flex.flex_grow, flex.flex_shrink, flex.flex_basis),
        (1.0, 1.0, FlexBasis::Content)
    );
    // `content` is only `flex-basis`'s; `stretch`, `fit-content()` and the
    // prefixed keywords aren't supported, nor any keyword on `min-*`,
    // `max-*` or margins: each of them is dropped.
    let i = style_of(&s, "invalid");
    assert_eq!((i.width, i.height), (size_px(1.0), size_px(2.0)));
    assert_eq!(i.min_width, px(3.0));
    assert_eq!(
        i.max_width,
        LengthPercentageOrNone::LengthPercentage(LengthPercentage::Px(4.0))
    );
    assert_eq!(i.margin.left, px(5.0));
}

#[test]
fn grid_properties() {
    use computed_style::{GridAutoFlow, GridLine, GridTrack, RepeatCount, TrackBreadth, TrackSize};
    let s = styles(
        "<div id=initial></div>
         <div id=g style='display: grid;
           grid-template-columns: 100px 1fr minmax(2em, max-content) fit-content(50%)
             repeat(2, auto min-content);
           grid-template-rows: repeat(auto-fill, minmax(10px, 1fr)) 20px;
           grid-auto-columns: 30px 2fr; grid-auto-rows: max-content;
           grid-auto-flow: column dense;
           grid-row: 2 / span 3; grid-column: -1;
           justify-items: center; justify-self: end'></div>
         <div id=area style='grid-area: 1 / 2 / 3 / 4'></div>
         <div id=area-short style='grid-area: span 2 / 5'></div>
         <div id=none style='grid-template-columns: 10px; grid-template-columns: none'></div>
         <div id=invalid style='grid-template-columns: 10px; grid-template-columns: [a] [b] 20px;
           grid-template-columns: 1fr repeat(auto-fit, 10px); grid-template-columns: -5px;
           grid-template-columns: minmax(1fr, 10px); grid-row-start: 2; grid-row-start: 0;
           grid-row-start: span; grid-row-start: span 0; grid-auto-flow: row column;
           justify-self: left'></div>",
    );
    let px = |v| TrackBreadth::LengthPercentage(LengthPercentage::Px(v));
    let breadth = TrackSize::Breadth;
    let initial = style_of(&s, "initial");
    assert!(initial.rare.grid_template_columns.tracks.is_empty());
    assert!(initial.rare.grid_template_areas.is_none());
    assert_eq!(*initial.rare.grid_auto_rows, [breadth(TrackBreadth::Auto)]);
    assert_eq!(initial.rare.grid_auto_flow, GridAutoFlow::Row);
    assert_eq!(
        (
            initial.rare.grid_row_start.clone(),
            initial.rare.grid_column_end.clone()
        ),
        (GridLine::Auto, GridLine::Auto)
    );
    assert_eq!(initial.justify_items, ItemAlignment::Normal);
    assert_eq!(initial.justify_self, None);

    let g = style_of(&s, "g");
    assert_eq!(g.display, Display::Grid);
    assert_eq!(
        *g.rare.grid_template_columns.tracks,
        [
            GridTrack::Size(breadth(px(100.0))),
            GridTrack::Size(breadth(TrackBreadth::Flex(1.0))),
            GridTrack::Size(TrackSize::MinMax(px(32.0), TrackBreadth::MaxContent)),
            GridTrack::Size(TrackSize::FitContent(LengthPercentage::Percent(50.0))),
            GridTrack::Repeat(
                RepeatCount::Count(2),
                [
                    breadth(TrackBreadth::Auto),
                    breadth(TrackBreadth::MinContent)
                ]
                .into(),
                [].into()
            ),
        ]
    );
    assert_eq!(
        *g.rare.grid_template_rows.tracks,
        [
            GridTrack::Repeat(
                RepeatCount::AutoFill,
                [TrackSize::MinMax(px(10.0), TrackBreadth::Flex(1.0))].into(),
                [].into()
            ),
            GridTrack::Size(breadth(px(20.0))),
        ]
    );
    assert_eq!(
        *g.rare.grid_auto_columns,
        [breadth(px(30.0)), breadth(TrackBreadth::Flex(2.0))]
    );
    assert_eq!(*g.rare.grid_auto_rows, [breadth(TrackBreadth::MaxContent)]);
    assert_eq!(g.rare.grid_auto_flow, GridAutoFlow::ColumnDense);
    assert_eq!(
        (g.rare.grid_row_start.clone(), g.rare.grid_row_end.clone()),
        (GridLine::Line(2), GridLine::Span(3))
    );
    assert_eq!(
        (
            g.rare.grid_column_start.clone(),
            g.rare.grid_column_end.clone()
        ),
        (GridLine::Line(-1), GridLine::Auto)
    );
    assert_eq!(g.justify_items, ItemAlignment::Center);
    assert_eq!(g.justify_self, Some(ItemAlignment::End));

    let a = style_of(&s, "area");
    assert_eq!(
        (
            a.rare.grid_row_start.clone(),
            a.rare.grid_column_start.clone(),
            a.rare.grid_row_end.clone(),
            a.rare.grid_column_end.clone()
        ),
        (
            GridLine::Line(1),
            GridLine::Line(2),
            GridLine::Line(3),
            GridLine::Line(4)
        )
    );
    let a = style_of(&s, "area-short");
    assert_eq!(
        (
            a.rare.grid_row_start.clone(),
            a.rare.grid_column_start.clone(),
            a.rare.grid_row_end.clone(),
            a.rare.grid_column_end.clone()
        ),
        (
            GridLine::Span(2),
            GridLine::Line(5),
            GridLine::Auto,
            GridLine::Auto
        )
    );
    assert!(
        style_of(&s, "none")
            .rare
            .grid_template_columns
            .tracks
            .is_empty()
    );

    // Two line name sets in a row, a flexible track next to an auto
    // repeat, negative sizes, a flexible minimum, line 0, `span` alone and
    // `span 0` are dropped.
    let i = style_of(&s, "invalid");
    assert_eq!(
        *i.rare.grid_template_columns.tracks,
        [GridTrack::Size(breadth(px(10.0)))]
    );
    assert_eq!(i.rare.grid_row_start.clone(), GridLine::Line(2));
    assert_eq!(i.rare.grid_auto_flow, GridAutoFlow::Row);
    assert_eq!(i.justify_self, None);
}

#[test]
fn place_and_gap_shorthands() {
    use computed_style::ContentAlignment;
    let s = styles(
        "<div id=one style='place-items: center; place-self: start; place-content: space-between'></div>
         <div id=two style='place-items: end stretch; place-self: auto center;
           place-content: center flex-end; grid-gap: 3px 4px'></div>
         <div id=legacy style='grid-row-gap: 5px; grid-column-gap: 6px'></div>",
    );
    let one = style_of(&s, "one");
    assert_eq!(
        (one.align_items, one.justify_items),
        (ItemAlignment::Center, ItemAlignment::Center)
    );
    assert_eq!(
        (one.align_self, one.justify_self),
        (Some(ItemAlignment::Start), Some(ItemAlignment::Start))
    );
    assert_eq!(
        (one.align_content, one.justify_content),
        (
            ContentAlignment::SpaceBetween,
            ContentAlignment::SpaceBetween
        )
    );
    let two = style_of(&s, "two");
    assert_eq!(
        (two.align_items, two.justify_items),
        (ItemAlignment::End, ItemAlignment::Stretch)
    );
    assert_eq!(
        (two.align_self, two.justify_self),
        (None, Some(ItemAlignment::Center))
    );
    assert_eq!(
        (two.align_content, two.justify_content),
        (ContentAlignment::Center, ContentAlignment::FlexEnd)
    );
    assert_eq!(
        (two.row_gap, two.column_gap),
        (LengthPercentage::Px(3.0), LengthPercentage::Px(4.0))
    );
    let legacy = style_of(&s, "legacy");
    assert_eq!(
        (legacy.row_gap, legacy.column_gap),
        (LengthPercentage::Px(5.0), LengthPercentage::Px(6.0))
    );
}

#[test]
fn grid_items_are_blockified_with_their_z_index() {
    let s = styles(
        "<div style='display: grid'><span id=item style='z-index: 2'></span></div>
         <span id=outside style='z-index: 2'></span>",
    );
    let item = style_of(&s, "item");
    assert_eq!(item.display, Display::Block);
    assert_eq!(item.effective_z_index, Some(2));
    let outside = style_of(&s, "outside");
    assert_eq!(outside.display, Display::Inline);
    assert_eq!(outside.effective_z_index, None);
}

/// Every grid property's value, to compare.
fn grid_values(s: &ComputedStyle) -> String {
    format!(
        "{:?}",
        (
            &s.rare.grid_template_columns,
            &s.rare.grid_template_rows,
            &s.rare.grid_template_areas,
            &s.rare.grid_auto_columns,
            &s.rare.grid_auto_rows,
            s.rare.grid_auto_flow,
            &s.rare.grid_row_start,
            &s.rare.grid_row_end,
            &s.rare.grid_column_start,
            &s.rare.grid_column_end,
            s.justify_items,
            s.justify_self,
        )
    )
}

#[test]
fn grid_properties_take_css_wide_keywords() {
    const SET: &str = "grid-template-columns: 1px 2fr; grid-template-rows: repeat(2, 3px);
        grid-auto-columns: 4px; grid-auto-rows: 5px; grid-auto-flow: column dense;
        grid-area: 1 / 2 / 3 / 4; justify-items: center; justify-self: end;
        grid-template-areas: \"a\"";
    const NAMES: [&str; 12] = [
        "grid-template-columns",
        "grid-template-rows",
        "grid-template-areas",
        "grid-auto-columns",
        "grid-auto-rows",
        "grid-auto-flow",
        "grid-row-start",
        "grid-row-end",
        "grid-column-start",
        "grid-column-end",
        "justify-items",
        "justify-self",
    ];
    let keyword = |k: &str| NAMES.map(|name| format!("{name}: {k}")).join("; ");
    let s = styles(&format!(
        "<div id=parent style='{SET}'>
           <p id=inherit style='{}'></p>
           <p id=initial style='{SET}; {}'></p>
           <p id=unset style='{SET}; {}'></p>
         </div>",
        keyword("inherit"),
        keyword("initial"),
        keyword("unset"),
    ));
    let grid = |id: &str| grid_values(style_of(&s, id));
    let initial = grid_values(&ComputedStyle::initial());
    assert_ne!(grid("parent"), initial);
    assert_eq!(grid("inherit"), grid("parent"));
    assert_eq!(grid("initial"), initial);
    // None of them is inherited.
    assert_eq!(grid("unset"), initial);
}

#[test]
fn grid_names_areas_and_grid_template() {
    use computed_style::{GridLine, GridTrack, TrackBreadth, TrackSize};
    use std::sync::Arc;
    let s = styles(
        "<div id=areas style='grid-template-areas: \"head head\" \"nav main\"'></div>
         <div id=bad-areas style='grid-template-areas: \"a\"; grid-template-areas: \"a b\" \"b a\"'></div>
         <div id=template style='grid-template: [top] \"head head\" 50px [mid]
           [nav-top] \"nav main\" 1fr [bottom] / [left] 100px [gutter] 1fr'></div>
         <div id=rows-columns style='grid-template-areas: \"x\"; grid-template: 10px / 20px 30px'></div>
         <div id=bad-template style='grid-template: \"a\" / 10px; grid-template: \"a\" / repeat(2, 10px);
           grid-template: \"a b\" \"c\"; grid-template: [x] / 10px; grid-template: \"a\" 10px 20px'></div>
         <div id=template-auto style='grid-template: \"a\" \"b\" 10px'></div>
         <div id=template-none style='grid-template: 10px / 20px; grid-template: none'></div>
         <div id=row style='grid-row: head; grid-column: col 2 / span nav'></div>
         <div id=area style='grid-area: main'></div>
         <div id=area-two style='grid-area: a / b'></div>
         <div id=area-number style='grid-area: 2 / b'></div>",
    );
    let name = |n: &str| -> Arc<str> { Arc::from(n) };
    let areas = style_of(&s, "areas")
        .rare
        .grid_template_areas
        .clone()
        .unwrap();
    assert_eq!((areas.rows, areas.columns), (2, 2));
    assert_eq!(
        areas
            .areas
            .iter()
            .map(|a| (
                &*a.name,
                a.row_start,
                a.row_end,
                a.column_start,
                a.column_end
            ))
            .collect::<Vec<_>>(),
        [
            ("head", 1, 2, 1, 3),
            ("nav", 2, 3, 1, 2),
            ("main", 2, 3, 2, 3)
        ]
    );
    // The invalid one doesn't override the valid one.
    assert_eq!(
        style_of(&s, "bad-areas")
            .rare
            .grid_template_areas
            .as_ref()
            .unwrap()
            .areas
            .len(),
        1
    );

    // Rows from the strings, with their sizes and the names around them:
    // `[mid]` and `[nav-top]` name the same line.
    let t = style_of(&s, "template");
    assert_eq!(t.rare.grid_template_areas.as_ref().unwrap().areas.len(), 3);
    let px = |v| TrackBreadth::LengthPercentage(LengthPercentage::Px(v));
    assert_eq!(
        *t.rare.grid_template_rows.tracks,
        [
            GridTrack::Size(TrackSize::Breadth(px(50.0))),
            GridTrack::Size(TrackSize::Breadth(TrackBreadth::Flex(1.0))),
        ]
    );
    assert_eq!(
        *t.rare.grid_template_rows.line_names,
        [
            [name("top")].into(),
            [name("mid"), name("nav-top")].into(),
            [name("bottom")].into()
        ]
    );
    assert_eq!(
        *t.rare.grid_template_columns.line_names,
        [
            [name("left")].into(),
            [name("gutter")].into(),
            Arc::from([])
        ]
    );
    // A row without a size is `auto`.
    assert_eq!(
        *style_of(&s, "template-auto").rare.grid_template_rows.tracks,
        [
            GridTrack::Size(TrackSize::Breadth(TrackBreadth::Auto)),
            GridTrack::Size(TrackSize::Breadth(px(10.0))),
        ]
    );
    // No `repeat()` after areas, rows of the same length, names only
    // around a row, one size per row.
    let bad = style_of(&s, "bad-template");
    assert_eq!(bad.rare.grid_template_areas.as_ref().unwrap().columns, 1);
    assert!(matches!(
        *bad.rare.grid_template_columns.tracks,
        [GridTrack::Size(_)]
    ));
    assert!(bad.rare.grid_template_rows.line_names.is_empty());
    // Rows and columns: no areas, whatever came before.
    let rc = style_of(&s, "rows-columns");
    assert!(rc.rare.grid_template_areas.is_none());
    assert_eq!(rc.rare.grid_template_columns.tracks.len(), 2);
    let none = style_of(&s, "template-none");
    assert!(none.rare.grid_template_rows.tracks.is_empty());
    assert!(none.rare.grid_template_columns.tracks.is_empty());

    // A name alone is the omitted end's too; anything else, `auto`.
    let row = style_of(&s, "row");
    assert_eq!(
        (
            row.rare.grid_row_start.clone(),
            row.rare.grid_row_end.clone()
        ),
        (GridLine::Ident(name("head")), GridLine::Ident(name("head")))
    );
    assert_eq!(
        (
            row.rare.grid_column_start.clone(),
            row.rare.grid_column_end.clone()
        ),
        (
            GridLine::Named(name("col"), 2),
            GridLine::NamedSpan(name("nav"), 1)
        )
    );
    let lines = |id: &str| {
        let s = style_of(&s, id);
        [
            s.rare.grid_row_start.clone(),
            s.rare.grid_column_start.clone(),
            s.rare.grid_row_end.clone(),
            s.rare.grid_column_end.clone(),
        ]
    };
    let main = GridLine::Ident(name("main"));
    assert_eq!(
        lines("area"),
        [main.clone(), main.clone(), main.clone(), main]
    );
    let (a, b) = (GridLine::Ident(name("a")), GridLine::Ident(name("b")));
    assert_eq!(lines("area-two"), [a.clone(), b.clone(), a, b.clone()]);
    assert_eq!(
        lines("area-number"),
        [GridLine::Line(2), b.clone(), GridLine::Auto, b]
    );
}

/// `cursor`'s images' URLs resolve against the base URL, but an empty one
/// and a fragment, which stay as written (Chrome's).
#[test]
fn a_cursors_urls_resolve_against_the_base_url() {
    let styles = styles_adopting(
        "<style>#a { cursor: url(img/a.cur) 1 2, url(''), url(#c), pointer }</style><div id=a></div>",
        Vec::new(),
        Some(url::Url::parse("http://example.test/dir/page.html").unwrap()),
    );
    let urls: Vec<&str> = style_of(&styles, "a")
        .rare_inherited
        .cursor
        .images
        .iter()
        .map(|image| match &image.image {
            BackgroundImage::Url(url) => &*url.url,
            image => panic!("{image:?}"),
        })
        .collect();
    assert_eq!(urls, ["http://example.test/dir/img/a.cur", "", "#c"]);
}

/// `border-image-source`'s `url()` resolves against the base URL, as a
/// background image's does: in the shorthand, and set alone.
#[test]
fn a_border_images_url_resolves_against_the_base_url() {
    let styles = styles_adopting(
        "<style>#a { border-image: url(img/a.png) 30 } #b { border-image-source: url(/b.png) }\
         </style><div id=a></div><div id=b></div>",
        Vec::new(),
        Some(url::Url::parse("http://example.test/dir/page.html").unwrap()),
    );
    assert_eq!(
        style_of(&styles, "a").rare.border_image.source,
        BackgroundImage::Url("http://example.test/dir/img/a.png".into())
    );
    assert_eq!(
        style_of(&styles, "b").rare.border_image.source,
        BackgroundImage::Url("http://example.test/b.png".into())
    );
}

/// An `image-set()`'s `url()`s and strings resolve against the base URL,
/// in each property that takes one.
#[test]
fn an_image_sets_urls_resolve_against_the_base_url() {
    let styles = styles_adopting(
        "<style>#a { background: image-set(url(img/a.png) 1x, 'b.png' 2x) }\
         #b { border-image-source: image-set(url(/c.png) 1x); cursor: image-set('d.cur'), auto }\
         </style><div id=a></div><div id=b></div>",
        Vec::new(),
        Some(url::Url::parse("http://example.test/dir/page.html").unwrap()),
    );
    let urls = |image: &BackgroundImage| -> Vec<String> {
        let BackgroundImage::Set(options) = image else {
            panic!("{image:?}");
        };
        options
            .iter()
            .map(|option| match &option.image {
                BackgroundImage::Url(url) => url.url.to_string(),
                image => panic!("{image:?}"),
            })
            .collect()
    };
    let a = style_of(&styles, "a");
    assert_eq!(
        urls(&a.backgrounds.background_image[0]),
        [
            "http://example.test/dir/img/a.png",
            "http://example.test/dir/b.png"
        ]
    );
    let b = style_of(&styles, "b");
    assert_eq!(
        urls(&b.rare.border_image.source),
        ["http://example.test/c.png"]
    );
    assert_eq!(
        urls(&b.rare_inherited.cursor.images[0].image),
        ["http://example.test/dir/d.cur"]
    );
}
