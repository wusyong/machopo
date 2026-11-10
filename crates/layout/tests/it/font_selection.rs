//! Which font each part of a text is shaped in, with the built-in Ahem
//! (every glyph a 1em square, but no `€`) and Lato (one face, Medium, so
//! italic is synthesized): a character the first font of the list has no
//! glyph for takes the next one's, and the text after it the first one's
//! again; each family list and each style picks from its own. (In a debug
//! build every font picked without the whole query is checked against it.)

use layout::{BoxFragment, TextContext};

use crate::page::{Page, find};

/// The text runs of `html`'s lines, in order: each one's font (its data's
/// id), skew and advance.
fn runs(html: &str) -> Vec<(u64, f32, f32)> {
    let html = format!("<style>html, body {{ margin: 0 }}</style>{html}");
    let view = crate::page::lay_out(&html).view;
    fn collect(fragment: &BoxFragment, out: &mut Vec<(u64, f32, f32)>) {
        for line in &fragment.lines {
            for run in &line.runs {
                out.push((run.font.id, run.skew, run.advance));
            }
        }
        for child in &fragment.children {
            collect(&child.fragment, out);
        }
    }
    let mut out = Vec::new();
    collect(&view, &mut out);
    out
}

/// The fonts of `runs`, as the index of each one's first appearance.
fn fonts(runs: &[(u64, f32, f32)]) -> Vec<usize> {
    let mut seen = Vec::new();
    runs.iter()
        .map(|&(id, ..)| {
            seen.iter().position(|&s| s == id).unwrap_or_else(|| {
                seen.push(id);
                seen.len() - 1
            })
        })
        .collect()
}

/// A character the first font has no glyph for is in the next one; the
/// characters after it are in the first again, not in the one before them.
#[test]
fn a_missing_glyph_falls_back_and_back_again() {
    let runs = runs("<div style='font: 20px/1 Ahem, Lato'>XX\u{20AC}X\u{20AC}\u{20AC}XX</div>");
    assert_eq!(fonts(&runs), [0, 1, 0, 1, 0]);
    let advances: Vec<f32> = runs
        .iter()
        .map(|&(_, _, advance)| advance.round())
        .collect();
    assert_eq!(advances[0], 40.0, "Ahem: {advances:?}");
    assert_eq!(advances[2], 20.0, "Ahem: {advances:?}");
    assert_eq!(advances[4], 40.0, "Ahem: {advances:?}");
    assert_eq!(advances[1], 12.0, "Lato: {advances:?}");
}

/// An element with other families picks from its own list, and the text
/// after it from the first list again: one with a box (its edges split
/// the shaping), and one with none (`display: contents`), shaped with the
/// text around it.
#[test]
fn each_family_list_picks_its_own() {
    for display in ["inline", "contents"] {
        let runs = runs(&format!(
            "<div style='font: 20px/1 Ahem'>XX<span style='display: {display}; font-family: Lato'>XX</span>XX</div>"
        ));
        assert_eq!(fonts(&runs), [0, 1, 0], "{display}");
    }
}

/// A generic family is the families it stands for: in the tests, Lato.
#[test]
fn a_generic_family_is_its_families() {
    let runs = runs(
        "<div style='font: 20px/1 Ahem'>XX<span style='font-family: Lato'>XX</span>XX<span style='font-family: serif'>XX</span></div>",
    );
    assert_eq!(fonts(&runs), [0, 1, 0, 1]);
}

/// An italic element takes the font's italic, synthesized as a skew since
/// Lato has none; the text after it, upright again, has none. With a box
/// and without.
#[test]
fn each_style_picks_its_own() {
    for display in ["inline", "contents"] {
        let runs = runs(&format!(
            "<div style='font: 20px/1 Lato'>ab<i style='display: {display}'>ab</i>ab</div>"
        ));
        assert_eq!(fonts(&runs), [0, 0, 0], "{display}");
        let skews: Vec<f32> = runs.iter().map(|&(_, skew, _)| skew).collect();
        assert_eq!(skews[0], 0.0, "{display}: {skews:?}");
        assert_ne!(skews[1], 0.0, "{display}: {skews:?}");
        assert_eq!(skews[2], 0.0, "{display}: {skews:?}");
    }
}

/// A `font-family` value's names are looked up in the fonts when text is
/// first shaped in it, whichever span has it: the block, an inline
/// element, or a run in an element with no box (`display: contents`).
/// Text in a value seen before looks none up, in another block or another
/// kind of span too, and neither does an update that shapes nothing.
#[test]
fn a_font_family_value_is_looked_up_once() {
    let mut page = Page::new(
        "<div id=d>XX <span id=s>XX</span> <span id=c style='display: contents'>XX</span></div>         <div id=e>XX</div>",
    );
    let mut names = |id: &str, style: &str| {
        let update = page.change(|cx, document| {
            cx.set_attribute(find(cx, document, id), "style", style);
        });
        (update.shaped, update.font_family_names)
    };
    // The list's own names, and those every list ends in.
    let (shaped, one) = names("d", "font-family: f1");
    assert_eq!(shaped, 1);
    assert!(one > 0);
    assert_eq!(names("d", "font-family: f1, f2"), (1, one + 1));
    assert_eq!(names("s", "font-family: g1, g2, g3"), (1, one + 2));
    assert_eq!(
        names("c", "display: contents; font-family: h1, h2, h3, h4"),
        (1, one + 3)
    );
    // Seen before: the same value, not the same style.
    assert_eq!(names("e", "font-family: f1, f2; font-size: 30px"), (1, 0));
    assert_eq!(names("e", "font-family: g1, g2, g3"), (1, 0));
    assert_eq!(names("s", "font-family: h1, h2, h3, h4"), (1, 0));
    assert_eq!(names("c", "display: contents; font-family: f1"), (1, 0));
    assert_eq!(names("d", "font-family: f1; font-size: 30px"), (1, 0));
    // The same names in another order are another value.
    assert_eq!(names("e", "font-family: f2, f1"), (1, one + 1));
    // A generic family is no name to look up: `serif` is at most the names
    // every list ends in, `monospace` none on any platform (`cursive` and
    // the like are a name of their own where the fonts are Linux's).
    assert_eq!(names("e", "font-family: k1, serif, monospace"), (1, one));
    assert_eq!(page.update().font_family_names, 0);
}

/// Adding fonts forgets what the `font-family` values were looked up as:
/// a name that was no family's may be one of theirs.
#[test]
fn adding_fonts_looks_the_family_names_up_again() {
    let exo = || {
        let path = format!(
            "{}/../../../third_party/wpt/css/css-fonts/support/fonts/Exo2-SemiBold.otf",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
    };
    let name = TextContext::new(true)
        .register_fonts_for_testing(exo())
        .remove(0);
    let mut page = Page::new(&format!(
        "<div id=d style=\"font: 20px/1 '{name}', Ahem\">XX</div>"
    ));
    fn run(page: &mut Page) -> (u64, f32) {
        let view = page.lay_out().view;
        let mut out = Vec::new();
        fn collect(fragment: &BoxFragment, out: &mut Vec<(u64, f32)>) {
            for line in &fragment.lines {
                out.extend(line.runs.iter().map(|run| (run.font.id, run.advance)));
            }
            for child in &fragment.children {
                collect(&child.fragment, out);
            }
        }
        collect(&view, &mut out);
        assert_eq!(out.len(), 1, "{out:?}");
        out[0]
    }
    let (ahem, advance) = run(&mut page);
    assert_eq!(advance, 40.0);
    assert_eq!(
        page.text.register_fonts_for_testing(exo()),
        std::slice::from_ref(&name)
    );
    let update = page.change(|cx, document| {
        let style = format!("font: 30px/1 '{name}', Ahem");
        cx.set_attribute(find(cx, document, "d"), "style", &style);
    });
    assert!(update.font_family_names > 0);
    let (font, advance) = run(&mut page);
    assert_ne!(font, ahem);
    assert_ne!(advance, 60.0);
}

/// A line with nothing on it is as high as the block's font makes it,
/// like the lines with text around it: the block's families are looked
/// up for it too, not only for the text.
#[test]
fn a_line_with_nothing_on_it_is_in_the_blocks_font() {
    let heights = |family: &str| -> Vec<f32> {
        let laid = crate::page::lay_out(&format!(
            "<div id=d style='font: 40px {family}; white-space: pre'>x\n\nx</div>"
        ));
        laid.lines("d")
            .iter()
            .map(|line| line.size().height)
            .collect()
    };
    let lato = heights("Lato");
    assert_eq!(lato.len(), 3, "{lato:?}");
    assert_eq!(lato[1], lato[0], "{lato:?}");
    let ahem = heights("Ahem");
    assert_eq!(ahem, [40.0, 40.0, 40.0]);
    assert_ne!(lato[1], ahem[1]);
}

/// A canvas's text is in its font's families, looked up as a page's are.
#[test]
fn a_canvass_text_is_in_its_families() {
    use computed_style::{CanvasFont, CanvasTextStyle, Direction, FontFamily, FontKerning};
    let mut text = TextContext::new(true);
    let mut width = |family: &str| {
        let style = CanvasTextStyle {
            font: CanvasFont {
                family: std::sync::Arc::new([FontFamily::Named(family.into())]),
                size: 20.0,
                ..CanvasFont::default()
            },
            direction: Direction::Ltr,
            letter_spacing: 0.0,
            word_spacing: 0.0,
            kerning: FontKerning::Auto,
        };
        layout::shape_canvas_text(&mut text, "XX", &style).width
    };
    assert_eq!(width("Ahem"), 40.0);
    let lato = width("Lato");
    assert_ne!(lato, 40.0);
    // And again, from what the names were looked up as.
    assert_eq!(width("Ahem"), 40.0);
    assert_eq!(width("Lato"), lato);
}
