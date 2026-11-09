//! Presentational hints: HTML's attributes that map to CSS, below every
//! author rule; a cell's that are its table's, styled again when its
//! table's change; a document's link colors from its `<body>`; an editing
//! host's `overflow-wrap`.

use computed_style::{LengthPercentage, ListStyleType, Size};
use gfx::Color;

use crate::Page;

const GREEN: Color = Color::rgb(0, 128, 0);

fn px(px: f32) -> Size {
    Size::LengthPercentage(LengthPercentage::Px(px))
}

#[test]
fn attributes_map_to_css_below_the_rules() {
    let mut page = Page::new(
        "<!doctype html><style>.w { width: 7px }</style>\
         <table id=t width=300 cellpadding=4 border=2><tr id=r valign=top>\
         <td id=c width=50 nowrap>x</td><td id=w class=w width=60>y</td></tr></table>\
         <ol id=o type=I start=3><li id=l value=9>a</ol><font id=f color=green size=5>f</font>\
         <hr id=h width=40 noshade>",
    );
    assert_eq!(page.style("t").width, px(300.0));
    assert_eq!(page.style("c").width, px(50.0));
    assert_eq!(page.style("w").width, px(7.0), "a rule wins over a hint");
    assert_eq!(
        page.style("c").padding.left,
        LengthPercentage::Px(4.0),
        "its table's cellpadding"
    );
    assert_eq!(page.style("c").border_width.left, 1.0, "its table's border");
    assert_eq!(
        page.style("o").rare_inherited.list_style_type,
        ListStyleType::UpperRoman
    );
    assert_eq!(page.style("o").rare.counter_reset[0].value, Some(2));
    assert_eq!(page.style("l").rare.counter_set[0].value, 9);
    assert_eq!(page.style("f").color, GREEN);
    assert_eq!(page.style("h").width, px(40.0));
}

/// `contenteditable` true or plaintext-only wraps long words
/// (`overflow-wrap: break-word`, which descendants inherit), below the
/// rules; false or another value doesn't; styled again as it changes.
#[test]
fn an_editing_hosts_long_words_wrap() {
    use computed_style::OverflowWrap;
    let mut page = Page::new(
        "<!doctype html><style>.n { overflow-wrap: normal }</style>\
         <div id=t contenteditable><i id=in>x</i></div><div id=p contenteditable=PlainText-Only></div>\
         <div id=f contenteditable=false></div><div id=b contenteditable=bogus></div>\
         <div id=r class=n contenteditable></div><svg><g id=g contenteditable></g></svg>",
    );
    let wrap = |page: &mut Page, id: &str| page.style(id).rare_inherited.overflow_wrap;
    for (id, want) in [
        ("t", OverflowWrap::BreakWord),
        ("in", OverflowWrap::BreakWord),
        ("p", OverflowWrap::BreakWord),
        ("f", OverflowWrap::Normal),
        ("b", OverflowWrap::Normal),
        ("r", OverflowWrap::Normal),
        ("g", OverflowWrap::Normal),
    ] {
        assert_eq!(wrap(&mut page, id), want, "{id}");
    }
    page.with("t", |cx, t| cx.set_attribute(t, "contenteditable", "false"));
    page.with("f", |cx, f| cx.set_attribute(f, "contenteditable", ""));
    page.recalc();
    assert_eq!(wrap(&mut page, "t"), OverflowWrap::Normal);
    assert_eq!(wrap(&mut page, "in"), OverflowWrap::Normal);
    assert_eq!(wrap(&mut page, "f"), OverflowWrap::BreakWord);
}

#[test]
fn a_tables_attributes_style_its_cells_again() {
    let mut page = Page::new(
        "<!doctype html><table id=t cellpadding=1><tbody><tr><td id=c>x</td></tr></tbody></table>",
    );
    assert_eq!(page.style("c").padding.left, LengthPercentage::Px(1.0));
    page.with("t", |cx, t| cx.set_attribute(t, "cellpadding", "6"));
    page.recalc();
    assert_eq!(page.style("c").padding.left, LengthPercentage::Px(6.0));
    page.with("t", |cx, t| cx.set_attribute(t, "border", "3"));
    page.recalc();
    assert_eq!(page.style("c").border_width.left, 1.0);
}

#[test]
fn a_bodys_link_is_the_documents_link_color() {
    let mut page = Page::new("<!doctype html><body id=b link=green><a id=a href=x>a</a>");
    assert_eq!(page.style("a").color, GREEN);
    page.with("b", |cx, b| cx.set_attribute(b, "link", "#0000ff"));
    page.recalc();
    assert_eq!(page.style("a").color, Color::rgb(0, 0, 255));
}

/// The matched properties cache keys on the attributes the hints are of:
/// elements whose rules are the same but whose hints aren't don't share a
/// style.
#[test]
fn hints_key_the_matched_properties_cache() {
    let mut page = Page::new(
        "<!doctype html><table><tr><td id=a width=50>a<td id=b width=60>b<td id=c width=50>c</table>",
    );
    assert_eq!(page.style("a").width, px(50.0));
    assert_eq!(page.style("b").width, px(60.0));
    assert_eq!(page.style("c").width, px(50.0));
}

/// Only HTML elements have HTML's hints.
#[test]
fn another_namespaces_element_has_no_hints() {
    let mut page = Page::new("<!doctype html><div id=d></div>");
    page.with("d", |cx, d| {
        let document = cx.owner_document(d).unwrap();
        let cell = cx.create_element(
            document,
            dom::QualName {
                namespace: Some("urn:x"),
                prefix: None,
                local: "td",
            },
        );
        cx.set_attribute(cell, "id", "x");
        cx.set_attribute(cell, "width", "50");
        cx.append_child(d, cell).unwrap();
    });
    page.recalc();
    assert_eq!(page.style("x").width, Size::Auto);
}

/// A cell's table is its row's parent, or its row group's: a cell put
/// right in a row group (by the DOM; the parser makes a row) has none.
#[test]
fn a_cell_outside_a_row_has_no_table() {
    let mut page = Page::new(
        "<!doctype html><table cellpadding=4><tbody id=g><tr><td id=c>x</td></tr></tbody></table>",
    );
    page.with("g", |cx, g| {
        let document = cx.owner_document(g).unwrap();
        let cell = cx.create_element(
            document,
            dom::QualName {
                namespace: Some("http://www.w3.org/1999/xhtml"),
                prefix: None,
                local: "td",
            },
        );
        cx.set_attribute(cell, "id", "out");
        cx.append_child(g, cell).unwrap();
    });
    page.recalc();
    assert_eq!(page.style("c").padding.left, LengthPercentage::Px(4.0));
    assert_eq!(
        page.style("out").padding.left,
        LengthPercentage::Px(1.0),
        "the UA stylesheet's, not its table's"
    );
}

/// An SVG element's presentation attributes: the properties' values (a
/// number a length), below the rules; the geometry ones only on the
/// elements that have them (`r` on a `<circle>`, not a `<rect>`); a
/// `transform`, a `<path>`'s `d`; an animation element's `fill` its own;
/// one that doesn't parse ignored; a change styles it again.
#[test]
fn svg_presentation_attributes() {
    use computed_style::{StyleColor, SvgPaint};
    let mut page = Page::new(
        "<!doctype html><style>.s { stroke-width: 7px }</style>\
         <svg><circle id=c r=5 fill=green stroke-width=3 class=s></circle>\
         <rect id=r r=5 width=10 x=2 fill=nonsense transform='translate(1 2)'></rect>\
         <path id=p d='M0 0 L5 5'></path><animate id=a fill=freeze></animate><g id=g></g></svg>",
    );
    let circle = page.style("c");
    assert_eq!(circle.svg.r, LengthPercentage::Px(5.0));
    assert_eq!(circle.svg.fill, SvgPaint::Color(StyleColor::Rgba(GREEN)));
    assert_eq!(
        circle.svg.stroke_width,
        LengthPercentage::Px(7.0),
        "a rule wins over a presentation attribute"
    );
    let rect = page.style("r");
    assert_eq!(rect.svg.r, LengthPercentage::Px(0.0), "no `r` on a rect");
    assert_eq!(rect.width, px(10.0));
    assert_eq!(rect.svg.x, LengthPercentage::Px(2.0));
    let none = page.style("g").svg.fill.clone();
    assert_eq!(rect.svg.fill, none, "nonsense ignored");
    assert!(!rect.transform.is_empty());
    assert!(page.style("p").svg.d.is_some());
    assert_eq!(
        page.style("a").svg.fill,
        none,
        "an animation's `fill` isn't the property"
    );
    page.with("c", |cx, c| cx.set_attribute(c, "r", "9"));
    page.recalc();
    assert_eq!(page.style("c").svg.r, LengthPercentage::Px(9.0));
}
