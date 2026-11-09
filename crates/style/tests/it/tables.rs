//! The table properties (parsed, serialized, computed, inherited), the
//! user-agent stylesheet's table rules, and the tables' presentational
//! hints, as `getComputedStyle` reads them. The expected text is Chrome's.

use computed_style::ComputedStyle;
use dom::QualName;
use style_resolver::computed_value;

use crate::Page;

/// A function reading `property`'s computed value of the element `id` in
/// `html`: a shorthand's (`border-spacing`) from its longhands'.
fn values(html: &str) -> impl Fn(&str, &str) -> String {
    let styles = crate::styles(html);
    move |id, property| {
        let style = styles
            .get(id)
            .unwrap_or_else(|| panic!("#{id} has a style"));
        computed_value(style, property)
            .or_else(|| {
                let values: Vec<(&str, String)> = style_resolver::shorthand_longhands(property)?
                    .into_iter()
                    .map(|name| (name, computed_value(style, name).unwrap()))
                    .collect();
                style_resolver::shorthand_value(property, &values)
            })
            .expect("a supported property")
    }
}

/// `border-spacing` is Chrome's two longhands' shorthand: one length when
/// they are the same, nothing while only one is set.
#[test]
fn border_spacing_is_a_shorthand() {
    let css_text = |css: &str| style_resolver::CssPropertyValueSet::parse(css).css_text();
    assert_eq!(
        css_text("border-spacing: 3px 5px"),
        "border-spacing: 3px 5px;"
    );
    assert_eq!(
        css_text("-webkit-border-horizontal-spacing: 3px"),
        "-webkit-border-horizontal-spacing: 3px;"
    );
    assert_eq!(
        css_text("-webkit-border-horizontal-spacing: 3px; -webkit-border-vertical-spacing: 3px"),
        "border-spacing: 3px;"
    );
    let block = style_resolver::CssPropertyValueSet::parse("border-spacing: 3px 5px");
    assert_eq!(
        block.property_value("-webkit-border-vertical-spacing"),
        "5px"
    );
    let block = style_resolver::CssPropertyValueSet::parse("-webkit-border-vertical-spacing: 5px");
    assert_eq!(block.property_value("border-spacing"), "");
    let v = values(
        "<div id=a style='border-spacing: 1px 2px'><span id=b \
         style='-webkit-border-horizontal-spacing: 3px'></span>\
         <span id=c style='border-spacing: 5px; border-spacing: unset'></span></div>",
    );
    assert_eq!(v("c", "border-spacing"), "1px 2px", "unset inherits both");
    assert_eq!(v("a", "-webkit-border-horizontal-spacing"), "1px");
    assert_eq!(v("a", "-webkit-border-vertical-spacing"), "2px");
    assert_eq!(v("b", "border-spacing"), "3px 2px", "inherited apart");
}

/// Initial values; `border-spacing`'s lengths absolute (a negative `calc()`
/// held to 0), one when both are the same; every one but `table-layout`
/// inherited.
#[test]
fn the_table_properties_compute_and_inherit() {
    let initial = ComputedStyle::initial();
    for (property, expected) in [
        ("border-collapse", "separate"),
        ("-webkit-border-horizontal-spacing", "0px"),
        ("-webkit-border-vertical-spacing", "0px"),
        ("table-layout", "auto"),
        ("caption-side", "top"),
        ("empty-cells", "show"),
    ] {
        assert_eq!(
            computed_value(&initial, property).as_deref(),
            Some(expected),
            "{property}"
        );
    }
    let v = values(
        r#"<div id=parent style="font-size: 40px; border-collapse: collapse;
            border-spacing: calc(10px + 0.5em) calc(10px - 0.5em); table-layout: fixed;
            caption-side: bottom; empty-cells: hide">
          <div id=child></div>
          <div id=same style="border-spacing: 1em 40px"></div>
          <div id=unset style="border-spacing: unset; table-layout: unset;
            border-collapse: unset"></div>
          <div id=inherit style="border-spacing: 1px; table-layout: inherit"></div>
          <div id=initial style="border-spacing: initial; empty-cells: initial"></div>
        </div>"#,
    );
    assert_eq!(v("parent", "border-spacing"), "30px 0px");
    assert_eq!(v("parent", "table-layout"), "fixed");
    assert_eq!(v("child", "border-collapse"), "collapse");
    assert_eq!(v("child", "border-spacing"), "30px 0px");
    assert_eq!(v("child", "caption-side"), "bottom");
    assert_eq!(v("child", "empty-cells"), "hide");
    assert_eq!(v("child", "table-layout"), "auto", "not inherited");
    assert_eq!(v("same", "border-spacing"), "40px");
    assert_eq!(v("unset", "border-spacing"), "30px 0px");
    assert_eq!(v("unset", "table-layout"), "auto");
    assert_eq!(v("unset", "border-collapse"), "collapse");
    assert_eq!(v("inherit", "table-layout"), "fixed");
    assert_eq!(v("initial", "border-spacing"), "0px");
    assert_eq!(v("initial", "empty-cells"), "show");
}

/// HTML's and Chrome's table rules.
#[test]
fn the_user_agent_stylesheet_has_tables() {
    let v = values(
        r#"<table id=table><caption id=caption>c</caption>
          <colgroup id=colgroup><col id=col></colgroup>
          <thead id=thead><tr id=tr><th id=th>h</th></tr></thead>
          <tbody id=tbody><tr><td id=td>d</td></tr></tbody>
          <tfoot id=tfoot></tfoot></table>"#,
    );
    for (id, display) in [
        ("table", "table"),
        ("caption", "table-caption"),
        ("colgroup", "table-column-group"),
        ("col", "table-column"),
        ("thead", "table-header-group"),
        ("tbody", "table-row-group"),
        ("tfoot", "table-footer-group"),
        ("tr", "table-row"),
        ("th", "table-cell"),
        ("td", "table-cell"),
    ] {
        assert_eq!(v(id, "display"), display, "#{id}");
    }
    assert_eq!(v("table", "border-spacing"), "2px");
    assert_eq!(v("table", "border-collapse"), "separate");
    assert_eq!(v("table", "box-sizing"), "border-box");
    // Chrome's: `currentColor`, not HTML's `gray`.
    for id in ["table", "tbody", "tr", "td"] {
        assert_eq!(v(id, "border-top-color"), "rgb(0, 0, 0)", "#{id}");
    }
    assert_eq!(v("caption", "text-align"), "center");
    assert_eq!(v("th", "font-weight"), "700");
    assert_eq!(v("th", "text-align"), "center");
    assert_eq!(v("td", "text-align"), "start");
    // A row group's `middle`, inherited by its rows and cells.
    assert_eq!(v("tbody", "vertical-align"), "middle");
    assert_eq!(v("tr", "vertical-align"), "middle");
    assert_eq!(v("td", "vertical-align"), "middle");
    assert_eq!(v("td", "padding-top"), "1px");
    assert_eq!(v("th", "padding-left"), "1px");
}

/// `text-indent: initial` on the table; a row put in the table itself,
/// with the DOM, is `middle` too.
#[test]
fn a_table_resets_text_indent_and_its_own_rows_are_middle() {
    let mut page = Page::parsed(
        "<div style='text-indent: 5px; vertical-align: top'>
          <table id=table><tbody id=tbody></tbody></table></div>",
    );
    page.with("table", |cx, table| {
        let tr = cx.create_element(cx.document_of(table), QualName::html("tr"));
        cx.set_attribute(tr, "id", "tr");
        cx.append_child(table, tr).unwrap();
    });
    page.recalc();
    assert_eq!(
        computed_value(&page.style("table"), "text-indent").as_deref(),
        Some("0px")
    );
    assert_eq!(
        computed_value(&page.style("tr"), "vertical-align").as_deref(),
        Some("middle")
    );
}

/// `<table>`'s: `width` (not zero), `height`, `border` (1 when it isn't a
/// non-negative integer; `outset` when not zero), `cellspacing` (no
/// percentage), `align`.
#[test]
fn the_table_hints() {
    let v = values(
        r#"<table id=a width=300 height=50% border cellspacing=5 align=CENTER></table>
          <table id=b width=0 border=0 cellspacing=10% align=left></table>
          <table id=c width=40% border=3px cellspacing=1.5 align=right></table>
          <table id=d border=-2 align=middle></table>
          <table id=e border=x style="border-spacing: 7px; width: 9px"></table>"#,
    );
    assert_eq!(v("a", "width"), "300px");
    assert_eq!(v("a", "height"), "50%");
    assert_eq!(v("a", "border-top-width"), "1px");
    assert_eq!(v("a", "border-left-style"), "outset");
    assert_eq!(v("a", "border-spacing"), "5px");
    assert_eq!(v("a", "margin-left"), "auto");
    assert_eq!(v("a", "margin-right"), "auto");
    assert_eq!(v("a", "float"), "none");
    assert_eq!(v("b", "width"), "auto", "zero is ignored");
    assert_eq!(v("b", "border-top-width"), "0px");
    assert_eq!(v("b", "border-top-style"), "none");
    assert_eq!(v("b", "border-spacing"), "2px", "no percentage");
    assert_eq!(v("b", "float"), "left");
    assert_eq!(v("c", "width"), "40%");
    assert_eq!(v("c", "border-right-width"), "3px");
    assert_eq!(v("c", "border-bottom-style"), "outset");
    assert_eq!(v("c", "border-spacing"), "1.5px");
    assert_eq!(v("c", "float"), "right");
    assert_eq!(v("d", "border-top-width"), "1px");
    assert_eq!(v("d", "float"), "none");
    assert_eq!(v("d", "margin-left"), "0px");
    assert_eq!(v("e", "border-top-width"), "1px");
    // Author declarations win.
    assert_eq!(v("e", "border-spacing"), "7px");
    assert_eq!(v("e", "width"), "9px");
}

/// A cell's own (`width` and `height` not zero, `nowrap`), and its
/// table's: `border`'s `1px inset` in the cell's color (not the table's
/// `border-color`, which Chrome 152's cells inherit: a bug, WPT's
/// `table-border-color-not-inherited`), `cellpadding`.
#[test]
fn the_cell_hints() {
    let v = values(
        r#"<table border=2 cellpadding=4 style="border-color: red">
            <tr style="color: blue"><td id=a width=30 height=0 nowrap>a</td>
              <th id=b width=0% height=20%>b</th></tr>
          </table>
          <table cellpadding=-1><tbody><tr><td id=c>c</td></tr></tbody></table>
          <table border=0 cellpadding=0><tfoot><tr><td id=d>d</td></tr></tfoot></table>
          <table cellpadding=6><tr><td id=e style="padding: 2px">e</td></tr></table>"#,
    );
    assert_eq!(v("a", "width"), "30px");
    assert_eq!(v("a", "height"), "auto", "zero is ignored");
    assert_eq!(v("a", "text-wrap-mode"), "nowrap");
    assert_eq!(v("a", "white-space-collapse"), "collapse");
    assert_eq!(v("b", "width"), "auto");
    assert_eq!(v("b", "height"), "20%");
    assert_eq!(v("b", "text-wrap-mode"), "wrap");
    for id in ["a", "b"] {
        assert_eq!(v(id, "border-top-width"), "1px", "#{id}");
        assert_eq!(v(id, "border-left-style"), "inset", "#{id}");
        assert_eq!(v(id, "border-bottom-color"), "rgb(0, 0, 255)", "#{id}");
        assert_eq!(v(id, "padding-right"), "4px", "#{id}");
    }
    // Not a non-negative integer: the user-agent stylesheet's.
    assert_eq!(v("c", "padding-top"), "1px");
    assert_eq!(v("c", "border-top-style"), "none");
    assert_eq!(v("d", "padding-bottom"), "0px");
    assert_eq!(v("d", "border-top-style"), "none");
    assert_eq!(v("e", "padding-left"), "2px", "author declarations win");
}

/// The table a cell takes `cellpadding` from is its row's parent, or its
/// row group's: not a table further out, nor one its row isn't in, nor
/// the one it is in without a row.
#[test]
fn a_cell_takes_its_own_tables_hints() {
    let mut page = Page::parsed(
        "<table cellpadding=9 border=1><tr><td>
          <table><tr><td id=inner>i</td></tr></table>
          <div id=div cellpadding=9 border=1></div></td></tr></table>
          <table cellpadding=9 border=1><tbody id=tbody></tbody></table>",
    );
    let tbody = page.root("tbody");
    page.with("div", |cx, div| {
        // A row in a `<div>` in the table's cell, a cell in a `<div>`, and
        // a cell in a row group.
        let document = cx.document_of(div);
        let element = |name, id: Option<&str>| {
            let element = cx.create_element(document, QualName::html(name));
            if let Some(id) = id {
                cx.set_attribute(element, "id", id);
            }
            element
        };
        let tr = element("tr", None);
        let td = element("td", Some("in-div-row"));
        cx.append_child(tr, td).unwrap();
        cx.append_child(div, tr).unwrap();
        let td = element("td", Some("in-div"));
        cx.append_child(div, td).unwrap();
        let td = element("td", Some("in-tbody"));
        cx.append_child(tbody.get(cx.gc), td).unwrap();
    });
    page.recalc();
    for id in ["inner", "in-div-row", "in-div", "in-tbody"] {
        let style = page.style(id);
        let value = |property| computed_value(&style, property).expect("a supported property");
        assert_eq!(value("padding-top"), "1px", "#{id}");
        assert_eq!(value("border-top-style"), "none", "#{id}");
    }
}

/// The rows', row groups' and cells' `valign` and `align`: HTML's
/// keywords, anything else parsed as the property's CSS; a row's
/// `height`; `<col width>` and `<colgroup width>`.
#[test]
fn the_row_and_column_hints() {
    let v = values(
        r#"<table>
          <colgroup id=colgroup width=40><col id=col width=0><col id=col2 width=25%></colgroup>
          <thead id=thead valign=BOTTOM align=Middle>
            <tr id=tr valign=text-top align=end height=0><td id=td>x</td></tr></thead>
          <tbody id=tbody valign=baseline align=justify>
            <tr id=tr2 valign=10px align=absmiddle height=40><td id=td2 valign=top align=left>x</td>
              <td id=td3 valign=bogus align=RIGHT>x</td></tr></tbody></table>"#,
    );
    assert_eq!(v("thead", "vertical-align"), "bottom");
    assert_eq!(v("thead", "text-align"), "center");
    assert_eq!(v("tr", "vertical-align"), "text-top");
    assert_eq!(v("tr", "text-align"), "end");
    assert_eq!(v("tr", "height"), "0px", "zero is a row's height");
    assert_eq!(v("td", "vertical-align"), "text-top", "inherited");
    assert_eq!(v("td", "text-align"), "end", "inherited");
    assert_eq!(v("tbody", "vertical-align"), "baseline");
    assert_eq!(
        v("tbody", "text-align"),
        "start",
        "machopo has no `justify`"
    );
    assert_eq!(v("tr2", "vertical-align"), "10px");
    assert_eq!(v("tr2", "text-align"), "center");
    assert_eq!(v("tr2", "height"), "40px");
    assert_eq!(v("td2", "vertical-align"), "top");
    assert_eq!(v("td2", "text-align"), "left");
    assert_eq!(v("td3", "vertical-align"), "10px", "the row's, inherited");
    assert_eq!(v("td3", "text-align"), "right");
    assert_eq!(v("col", "width"), "0px");
    assert_eq!(v("col2", "width"), "25%");
    assert_eq!(v("colgroup", "width"), "40px", "Chrome's, not HTML's");
}
