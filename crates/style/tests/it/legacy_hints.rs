//! The presentational hints Chrome has for `<body>`, `<font>`, `<hr>` and
//! the tables' colors and backgrounds, the legacy colors they parse, and a
//! document's link colors (`<body link>`, `-webkit-link`), as
//! `getComputedStyle` reads them. The expected text is Chrome 152's, but
//! for what only layout gives (an `auto` margin or height reads as `auto`
//! here) and the default font family (the platform's in Chrome).

use style_resolver::computed_value;

/// `properties`' computed values of each element `ids` names, joined by
/// spaces, each element's after `id:`, the elements' by ` | `.
fn read(html: &str, ids: &[&str], properties: &[&str]) -> String {
    let styles = crate::styles(html);
    ids.iter()
        .map(|id| {
            let style = styles
                .get(*id)
                .unwrap_or_else(|| panic!("#{id} has a style"));
            let values: Vec<String> = properties
                .iter()
                .map(|property| computed_value(style, property).expect("a supported property"))
                .collect();
            format!("{id}:{}", values.join(" "))
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

/// HTML's rules for parsing a legacy colour value, through `bgcolor`.
#[test]
fn legacy_colors() {
    let colors = [
        "chucknorris",
        "abc",
        "#abc",
        "#abcd",
        "123456789",
        "#1234567",
        "  red ",
        "zz",
        "\u{1F600}",
        "#",
        "0",
        "fffffffff",
        "transparent",
        "",
        // Outside the BMP, two zeros; 128 at most; leading zeros of all
        // three parts dropped; the last eight digits of a longer part.
        "a\u{1F600}b",
        "0000000000000000000000000000000000000000000000000000000000000000\
         0000000000000000000000000000000000000000000000000000000000000000f",
        "#00ff00ff00ff",
        "1234567890abcdef1234567890ab",
    ];
    let html: String = colors
        .iter()
        .enumerate()
        .map(|(i, color)| format!("<table id=c{i} bgcolor=\"{color}\"></table>"))
        .collect();
    let ids: Vec<String> = (0..colors.len()).map(|i| format!("c{i}")).collect();
    let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
    assert_eq!(
        read(&html, &ids, &["background-color"]),
        "c0:rgb(192, 0, 0) | c1:rgb(10, 11, 12) | c2:rgb(170, 187, 204) | \
         c3:rgb(171, 205, 0) | c4:rgb(18, 69, 120) | c5:rgb(18, 69, 112) | \
         c6:rgb(255, 0, 0) | c7:rgb(0, 0, 0) | c8:rgb(0, 0, 0) | c9:rgb(0, 0, 0) | \
         c10:rgb(0, 0, 0) | c11:rgb(255, 255, 255) | c12:rgba(0, 0, 0, 0) | \
         c13:rgba(0, 0, 0, 0) | c14:rgb(160, 11, 0) | c15:rgb(0, 0, 0) | \
         c16:rgb(255, 255, 255) | c17:rgb(52, 205, 120)"
    );
}

/// `<font>`'s legacy size (1 to 7, or relative to 3, held to that range),
/// color and face.
#[test]
fn font_hints() {
    let html = "<font id=f1 size=1>a</font><font id=f2 size='+2'>a</font>\
        <font id=f3 size=' -1'>a</font><font id=f4 size=9>a</font>\
        <font id=f5 size='-9'>a</font><font id=f6 size='+0'>a</font>\
        <font id=f7 size='3x'>a</font><font id=f8 size=x>a</font>\
        <font id=f9 color=chucknorris face='Times, serif'>a</font>\
        <font id=f10 color='#abc' face=''>a</font><font id=f11 color=transparent>a</font>";
    assert_eq!(
        read(
            html,
            &["f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8"],
            &["font-size"]
        ),
        "f1:10px | f2:24px | f3:13px | f4:48px | f5:10px | f6:16px | f7:16px | f8:16px"
    );
    assert_eq!(
        read(html, &["f9", "f10", "f11"], &["color", "font-family"]),
        "f9:rgb(192, 0, 0) Times, serif | f10:rgb(170, 187, 204) serif | f11:rgb(0, 0, 0) serif"
    );
}

/// `<hr>`'s hints as Chrome has them: `size` a height (its integer less
/// two; one or less, or none, takes the bottom border off), `color` the
/// border's and the background's color with a solid border, `noshade`
/// gray ones when there is no `color`, `align` the margins, `width`.
#[test]
fn hr_hints() {
    let html = "<hr id=h1 size=5px><hr id=h2 size=x><hr id=h3 size=' 4 '><hr id=h4 size=1>\
        <hr id=h5 width=0><hr id=h6 width='50%' align=RIGHT><hr id=h7 align=left>\
        <hr id=h8 color=bogus><hr id=h9 color=blue noshade><hr id=h10 noshade>";
    assert_eq!(
        read(
            html,
            &["h1", "h2", "h3", "h4"],
            &["height", "border-bottom-width"]
        ),
        "h1:3px 1px | h2:auto 0px | h3:2px 1px | h4:auto 0px"
    );
    assert_eq!(
        read(
            html,
            &["h5", "h6", "h7"],
            &["width", "margin-left", "margin-right"]
        ),
        "h5:0px auto auto | h6:50% auto 0px | h7:auto 0px auto"
    );
    assert_eq!(
        read(
            html,
            &["h8", "h9", "h10", "h1"],
            &[
                "border-top-style",
                "border-top-color",
                "background-color",
                "color"
            ]
        ),
        "h8:solid rgb(176, 0, 0) rgb(176, 0, 0) rgb(128, 128, 128) | \
         h9:solid rgb(0, 0, 255) rgb(0, 0, 255) rgb(128, 128, 128) | \
         h10:solid rgb(128, 128, 128) rgb(128, 128, 128) rgb(128, 128, 128) | \
         h1:inset rgb(128, 128, 128) rgba(0, 0, 0, 0) rgb(128, 128, 128)"
    );
}

/// `<body>`'s background, text color and margins (`marginwidth` and
/// `leftmargin` the left and right ones, `marginheight` and `topmargin`
/// the top and bottom ones; `rightmargin` and `bottommargin` nothing).
#[test]
fn body_hints() {
    let body = |attributes: &str| {
        read(
            &format!("<body id=b {attributes}>"),
            &["b"],
            &[
                "margin-top",
                "margin-right",
                "margin-bottom",
                "margin-left",
                "background-color",
                "color",
                "background-image",
            ],
        )
    };
    assert_eq!(
        body("marginwidth=7 bgcolor=yellow text=green background=' a.png '"),
        "b:8px 7px 8px 7px rgb(255, 255, 0) rgb(0, 128, 0) url(\"a.png\")"
    );
    assert_eq!(
        body("leftmargin=7 topmargin=3"),
        "b:3px 7px 3px 7px rgba(0, 0, 0, 0) rgb(0, 0, 0) none"
    );
    assert_eq!(
        body("marginheight='10%' rightmargin=5 bottommargin=5 background=' '"),
        "b:10% 8px 10% 8px rgba(0, 0, 0, 0) rgb(0, 0, 0) none"
    );
    // A value that isn't a length sets nothing; the later of two wins.
    assert_eq!(
        body("marginwidth=x"),
        "b:8px 8px 8px 8px rgba(0, 0, 0, 0) rgb(0, 0, 0) none"
    );
    assert_eq!(
        body("marginwidth=1 leftmargin=2"),
        "b:8px 2px 8px 2px rgba(0, 0, 0, 0) rgb(0, 0, 0) none"
    );
}

/// The tables' `bgcolor` and `background`, a column's too; `bordercolor`,
/// which makes a table's border and its cells' solid and of its color, and
/// which its sections and rows inherit. An author's `border-color` on the
/// table isn't its cells' (Chrome 152's cells inherit it: a bug, WPT's
/// `table-border-color-not-inherited`).
#[test]
fn table_colors_and_backgrounds() {
    let html = "<table id=t bgcolor=red background=t.png><colgroup id=cg bgcolor=lime \
        background=cg.png><col id=col bgcolor=blue></colgroup><thead id=th bgcolor=green>\
        <tr id=tr bgcolor=yellow background=' r.png '><td id=td bgcolor=navy background=''>x\
        </td></tr></thead></table>";
    assert_eq!(
        read(
            html,
            &["t", "cg", "col", "th", "tr", "td"],
            &["background-color", "background-image"]
        ),
        "t:rgb(255, 0, 0) url(\"t.png\") | cg:rgb(0, 255, 0) url(\"cg.png\") | \
         col:rgb(0, 0, 255) none | th:rgb(0, 128, 0) none | \
         tr:rgb(255, 255, 0) url(\"r.png\") | td:rgb(0, 0, 128) none"
    );
    let table = |attributes: &str| {
        let html = format!(
            "<table id=t {attributes}><tbody id=tb><tr id=tr><td id=td style='color: green'>x\
             </td></tr></tbody></table>"
        );
        read(
            &html,
            &["t", "tb", "tr", "td"],
            &["border-top-style", "border-top-width", "border-top-color"],
        )
    };
    assert_eq!(
        table("border=1"),
        "t:outset 1px rgb(0, 0, 0) | tb:none 0px rgb(0, 0, 0) | tr:none 0px rgb(0, 0, 0) | \
         td:inset 1px rgb(0, 128, 0)"
    );
    assert_eq!(
        table("border=1 style='border-color: blue'"),
        "t:outset 1px rgb(0, 0, 255) | tb:none 0px rgb(0, 0, 255) | \
         tr:none 0px rgb(0, 0, 255) | td:inset 1px rgb(0, 128, 0)"
    );
    assert_eq!(
        table("border=2 bordercolor=red"),
        "t:solid 2px rgb(255, 0, 0) | tb:none 0px rgb(255, 0, 0) | \
         tr:none 0px rgb(255, 0, 0) | td:solid 1px rgb(255, 0, 0)"
    );
    // Without a border, only the color.
    assert_eq!(
        table("bordercolor=red"),
        "t:none 0px rgb(255, 0, 0) | tb:none 0px rgb(255, 0, 0) | \
         tr:none 0px rgb(255, 0, 0) | td:none 0px rgb(0, 128, 0)"
    );
    // An empty one isn't one.
    assert_eq!(
        table("border=1 bordercolor=''"),
        "t:outset 1px rgb(0, 0, 0) | tb:none 0px rgb(0, 0, 0) | tr:none 0px rgb(0, 0, 0) | \
         td:inset 1px rgb(0, 128, 0)"
    );
}

/// A document's link colors: its `<body>`'s `link` and `alink`, what
/// `-webkit-link` and `-webkit-activelink` are, in the UA stylesheet's
/// links and anywhere a page uses them; the defaults without them.
#[test]
fn link_colors() {
    let html = |body: &str| {
        format!(
            "<body {body}><a id=a href=x>a</a><div id=d style='color: -webkit-link; \
             background-color: -webkit-activelink'></div>"
        )
    };
    assert_eq!(
        read(
            &html("link=red alink=lime vlink=blue"),
            &["a", "d"],
            &["color", "background-color"]
        ),
        "a:rgb(255, 0, 0) rgba(0, 0, 0, 0) | d:rgb(255, 0, 0) rgb(0, 255, 0)"
    );
    assert_eq!(
        read(
            &html("link=transparent"),
            &["a", "d"],
            &["color", "background-color"]
        ),
        "a:rgb(0, 0, 238) rgba(0, 0, 0, 0) | d:rgb(0, 0, 238) rgb(255, 0, 0)"
    );
    // Another element's `link` isn't the document's.
    assert_eq!(
        read(
            "<body><div link=red><a id=a href=x>a</a></div>",
            &["a"],
            &["color"]
        ),
        "a:rgb(0, 0, 238)"
    );
}
