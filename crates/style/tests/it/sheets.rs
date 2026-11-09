//! The document's sheets: `<style>`s and the `<link>`s whose sheets the
//! renderer gave (`dom::Cx::set_link_sheet`), in tree order; which of
//! them apply (CSS's `type`, not `disabled`, the preferred set and the
//! alternates, `media` as the viewport is); what to load
//! (`dom::Cx::stylesheet_link_href`).

use gfx::{Color, SizeF};

use crate::Page;

const RED: Color = Color::rgb(255, 0, 0);
const GREEN: Color = Color::rgb(0, 128, 0);
const BLUE: Color = Color::rgb(0, 0, 255);

/// `css` as the sheet of the `<link>` whose `id` is `id`.
fn give(page: &mut Page, id: &str, css: &str) {
    crate::give_link_sheet(page, id, css);
    page.recalc();
}

/// An SVG `<style>`'s sheet is the document's too, and changes as its
/// text does.
#[test]
fn an_svg_style_applies() {
    let mut page =
        Page::new("<!doctype html><svg><style id=s>p { color: green }</style></svg><p id=p>a</p>");
    assert_eq!(page.style("p").color, GREEN);
    page.with("s", |cx, style| {
        let text = cx.first_child(style).unwrap();
        cx.set_data(text, "p { color: blue }");
    });
    page.recalc();
    assert_eq!(page.style("p").color, BLUE);
}

#[test]
fn a_links_sheet_applies_in_tree_order_once_given() {
    let mut page = Page::new(
        "<!doctype html><style>p { color: red }</style><link id=l rel=stylesheet href=a.css>\
         <style>p.late { color: blue }</style><p id=p>a</p><p id=q class=late>b</p>",
    );
    assert_eq!(page.style("p").color, RED, "not loaded yet");
    give(&mut page, "l", "p { color: green }");
    assert_eq!(page.style("p").color, GREEN, "after the first <style>");
    assert_eq!(page.style("q").color, BLUE, "before the second");
}

#[test]
fn what_doesnt_apply() {
    let mut page = Page::new(
        "<!doctype html><link id=alt rel='alternate stylesheet' href=a.css>\
         <link id=off rel=stylesheet href=b.css disabled>\
         <link id=text rel=stylesheet href=c.css type=text/plain><link id=icon rel=icon href=d.css>\
         <style type=text/plain>p { color: red }</style>\
         <style media=print>p { color: red }</style><p id=p>a</p>",
    );
    for id in ["alt", "off", "text", "icon"] {
        give(&mut page, id, "p { color: red }");
    }
    assert_ne!(page.style("p").color, RED);
    let hrefs = page.dom.read(|cx| {
        let document = page.document.get(cx.gc);
        ["alt", "off", "text", "icon"]
            .map(|id| cx.stylesheet_link_href(crate::by_id(cx, document, id).unwrap()))
    });
    assert_eq!(
        hrefs,
        [Some("a.css".to_owned()), None, None, None],
        "an alternate one loads"
    );
}

/// The preferred set: the first title that isn't an alternate's, then
/// the sheets with that title or none, and the alternates with that
/// title.
#[test]
fn the_preferred_set_applies() {
    let mut page = Page::new(
        "<!doctype html><link id=alt rel='alternate stylesheet' title=x href=a.css>\
         <style title=y>p { color: blue }</style><style title=x>p { color: red }</style>\
         <link id=also rel='alternate stylesheet' title=y href=b.css><p id=p>a</p>",
    );
    give(&mut page, "alt", "p { color: red }");
    assert_eq!(page.style("p").color, BLUE, "y is the preferred set");
    give(&mut page, "also", "p { color: green }");
    assert_eq!(
        page.style("p").color,
        GREEN,
        "an alternate of the preferred set"
    );
}

/// A sheet's `media` as the environment is: another viewport makes the
/// sheets again.
#[test]
fn a_sheets_media_follows_the_viewport() {
    let mut page = Page::new(
        "<!doctype html><style media='(min-width: 500px)'>p { color: green }</style>\
         <link id=l rel=stylesheet href=a.css media='(max-width: 300px)'><p id=p>a</p>",
    );
    page.engine.set_viewport(SizeF::new(400.0, 300.0), 1.0);
    give(&mut page, "l", "p { color: blue }");
    assert_ne!(page.style("p").color, GREEN);
    assert_ne!(page.style("p").color, BLUE);
    page.engine.set_viewport(SizeF::new(600.0, 300.0), 1.0);
    page.recalc();
    assert_eq!(page.style("p").color, GREEN);
    page.engine.set_viewport(SizeF::new(200.0, 300.0), 1.0);
    page.recalc();
    assert_eq!(page.style("p").color, BLUE);
    page.with("l", |cx, link| cx.set_attribute(link, "media", "print"));
    page.recalc();
    assert_ne!(page.style("p").color, BLUE, "its media changed");
}

/// What CSSOM does to an element's sheet styles again: a rule put in, a
/// rule changed, its media, `disabled`; the next text is a new sheet,
/// without them.
#[test]
fn a_sheets_edits_apply() {
    use dom::{RuleParent, Rules};
    let mut page = Page::new("<!doctype html><style id=s>p { color: red }</style><p id=p>a</p>");
    assert_eq!(page.style("p").color, RED);
    page.with("s", |cx, style| {
        let sheet = cx.owner_sheet(style).unwrap();
        let edited = cx
            .style_sheet_contents(sheet)
            .insert_rule("p { color: green }", 1)
            .unwrap();
        cx.css_rule_inserted(RuleParent::Sheet(sheet), Rules::Sheet(edited), 1);
    });
    page.recalc();
    assert_eq!(page.style("p").color, GREEN, "a rule put in");
    page.with("s", |cx, style| {
        let sheet = cx.owner_sheet(style).unwrap();
        let rule = cx.rule_at(RuleParent::Sheet(sheet), 1).unwrap();
        let block = css::CssPropertyValueSet::parse("color: blue");
        let changed = cx.css_rule(rule).as_style().unwrap().with_style(block);
        cx.set_css_rule(rule, css::CssRule::Style(std::sync::Arc::new(changed)));
    });
    page.recalc();
    assert_eq!(page.style("p").color, BLUE, "a rule changed");
    page.with("s", |cx, style| {
        let sheet = cx.owner_sheet(style).unwrap();
        let print = css::media::MediaQueryList::parse("print");
        cx.set_style_sheet_media(sheet, "print", std::sync::Arc::new(print));
    });
    page.recalc();
    assert_ne!(page.style("p").color, BLUE, "its media");
    page.with("s", |cx, style| {
        let sheet = cx.owner_sheet(style).unwrap();
        cx.set_style_sheet_media(
            sheet,
            "",
            std::sync::Arc::new(css::media::MediaQueryList::parse("")),
        );
        cx.set_style_sheet_disabled(sheet, true);
    });
    page.recalc();
    assert_ne!(page.style("p").color, BLUE, "disabled");
    page.with("s", |cx, style| {
        let text = cx.first_child(style).unwrap();
        cx.set_data(text, "p { color: red }");
    });
    page.recalc();
    assert_eq!(page.style("p").color, RED, "a new sheet");
}
