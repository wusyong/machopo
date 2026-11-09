//! The fonts text was laid out in (`platform_fonts`, for DevTools'
//! `getPlatformFontsForNode`): each font of the runs asked about, in the
//! order it first comes, its family and PostScript name from its `name`
//! table, and the glyphs it drew of those runs (white space that collapsed
//! away draws none); the runs not asked about not counted.

use layout::{PlatformFont, platform_fonts};

use crate::page::{Page, VIEWPORT, find};

#[test]
fn fonts_are_counted_by_their_glyphs() {
    let mut page = Page::build(
        "<style>div { font: 20px Ahem } b { font: 20px Lato }</style>\
         <div id=a>ab  <b>cde</b> f</div><div id=other>xyz</div>",
    );
    let other = page.read(|cx, document| cx.node_id(find(cx, document, "other")));
    let view = page.tree.layout(VIEWPORT);
    let ahem = |glyph_count| PlatformFont {
        family_name: "Ahem".to_owned(),
        postscript_name: "Ahem".to_owned(),
        glyph_count,
    };
    let fonts = platform_fonts(&view, &|run| run.node != Some(other));
    assert_eq!(
        fonts,
        [
            // `ab ` (one space left of the two), then ` f`.
            ahem(5),
            PlatformFont {
                family_name: "Lato".to_owned(),
                postscript_name: "Lato-Medium".to_owned(),
                glyph_count: 3,
            },
        ]
    );
    assert_eq!(
        platform_fonts(&view, &|run| run.node == Some(other)),
        [ahem(3)]
    );
    assert!(platform_fonts(&view, &|_| false).is_empty());
}

/// Fonts of text in two blocks: the first block's first.
#[test]
fn fonts_come_in_tree_order() {
    let mut page = Page::build(
        "<style>p { font: 20px Lato } p + p { font: 20px Ahem }</style><div><p>a</p><p>b</p></div>",
    );
    let view = page.tree.layout(VIEWPORT);
    let names: Vec<String> = platform_fonts(&view, &|_| true)
        .into_iter()
        .map(|font| font.family_name)
        .collect();
    assert_eq!(names, ["Lato", "Ahem"]);
}
