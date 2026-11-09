//! Which font each part of a text is shaped in, with the built-in Ahem
//! (every glyph a 1em square, but no `€`) and Lato (one face, Medium, so
//! italic is synthesized): a character the first font of the list has no
//! glyph for takes the next one's, and the text after it the first one's
//! again; each family list and each style picks from its own. (In a debug
//! build every font picked without the whole query is checked against it.)

use layout::BoxFragment;

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

/// Each span of an inline formatting context shaped gives parley its font
/// family list, every name of which fontique looks up: the block's, an
/// inline element's, and a run's in an element with no box (`display:
/// contents`); text in the style of the span it is in gives none of its
/// own. Text beside a block, in an anonymous block, gives its block's list
/// twice: as the anonymous block's, and as its run's, whose style is the
/// block's, not the anonymous block's. An update that shapes nothing gives
/// none.
#[test]
fn each_span_gives_parley_its_family_names() {
    let mut page = Page::new(
        "<div id=d>XX <span id=s style='font-family: f1'>XX</span> \
         <span id=c style='display: contents; font-family: f1'>XX</span></div>\
         <div id=b>XX<div style='display: flex'></div></div><div id=e>XX</div>",
    );
    let mut names = |id: &str, style: &str| {
        let update = page.change(|cx, document| {
            cx.set_attribute(find(cx, document, id), "style", style);
        });
        (update.shaped, update.font_family_names)
    };
    let (shaped, d) = names("d", "font-size: 30px");
    assert_eq!(shaped, 1);
    assert!(d > 0);
    assert_eq!(names("s", "font-family: f1, f2"), (1, d + 1));
    assert_eq!(
        names("c", "display: contents; font-family: f1, f2, f3"),
        (1, d + 3)
    );
    let (_, e) = names("e", "font-size: 30px");
    assert_eq!(names("b", "font-size: 30px"), (1, 2 * e));
    assert_eq!(page.update().font_family_names, 0);
}
