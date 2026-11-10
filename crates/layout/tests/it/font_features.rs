//! What the font properties do to the text: the OpenType features
//! `font-variant-*`, `font-kerning`, `text-rendering` and
//! `font-feature-settings` ask for, the width `font-stretch` picks, the
//! axes `font-variation-settings` and `font-optical-sizing` set (which the
//! glyphs are drawn at too), and what `font-synthesis` stops synthesizing.
//! With the built-in Lato (which has `liga`, `tnum`, `sups`, kerning), and
//! WPT's test fonts for small caps, widths and axes (read from the WPT
//! checkout: their licenses don't say machopo may ship them).

use layout::{BoxFragment, TextContext, TextFragment};

use crate::page::Page;

/// A font of WPT's `css/css-fonts/support/fonts/`.
fn wpt_font(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/../../../third_party/wpt/css/css-fonts/support/fonts/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// The text runs of `html`'s lines, in order, laid out with the built-in
/// fonts and `fonts` (WPT's, by file name); and the family names those
/// hold, in order.
fn runs_with(html: &str, fonts: &[&str]) -> (Vec<TextFragment>, Vec<String>) {
    let mut cx = TextContext::new(true);
    let families = fonts
        .iter()
        .flat_map(|name| cx.register_fonts_for_testing(wpt_font(name)))
        .collect();
    let html = format!("<style>html, body {{ margin: 0 }}</style>{html}");
    let view = Page::build_with_text(&html, cx).lay_out().view;
    fn collect(fragment: &BoxFragment, out: &mut Vec<TextFragment>) {
        for line in &fragment.lines {
            out.extend(line.runs.iter().cloned());
        }
        for child in &fragment.children {
            collect(&child.fragment, out);
        }
    }
    let mut out = Vec::new();
    collect(&view, &mut out);
    (out, families)
}

fn runs(html: &str) -> Vec<TextFragment> {
    runs_with(html, &[]).0
}

/// Each run's glyph ids.
fn glyphs(runs: &[TextFragment]) -> Vec<Vec<u32>> {
    runs.iter()
        .map(|run| run.glyphs().map(|(id, _)| id).collect())
        .collect()
}

/// `text` in Lato at 40px, one block per `css`: each one's glyphs and
/// advance.
fn lato(text: &str, css: &[&str]) -> Vec<(Vec<u32>, f32)> {
    let html: String = css
        .iter()
        .map(|css| format!("<div style='font: 40px Lato; {css}'>{text}</div>"))
        .collect();
    runs(&html)
        .iter()
        .map(|run| (run.glyphs().map(|(id, _)| id).collect(), run.advance))
        .collect()
}

/// The common ligatures: on by default (Lato's `fi`), off for
/// `font-variant-ligatures: none` or `no-common-ligatures`,
/// `font-feature-settings: "liga" 0`, `text-rendering: optimizeSpeed`
/// (but where `common-ligatures` asks for them) and a letter spacing.
#[test]
fn ligatures() {
    let fi = lato(
        "fi",
        &[
            "",
            "font-variant-ligatures: none",
            "font-variant-ligatures: no-common-ligatures",
            "font-feature-settings: \"liga\" 0",
            "text-rendering: optimizeSpeed",
            "text-rendering: optimizeSpeed; font-variant-ligatures: common-ligatures",
            "text-rendering: optimizeLegibility",
            "font-variant-ligatures: none; font-feature-settings: \"liga\" 1",
        ],
    );
    let count: Vec<usize> = fi.iter().map(|(glyphs, _)| glyphs.len()).collect();
    assert_eq!(count, [1, 2, 2, 2, 2, 1, 1, 1]);
    // An element's own, in a line of its parent's.
    let runs = runs(
        "<div style='font: 40px Lato'>fi<span style='font-variant-ligatures: none'>fi</span></div>",
    );
    let count: Vec<usize> = glyphs(&runs).iter().map(Vec::len).collect();
    assert_eq!(count, [1, 2]);
}

/// `font-kerning: none` (and `optimizeSpeed`'s) turns Lato's kerning off,
/// `normal` keeps it.
#[test]
fn kerning() {
    let av = lato(
        "AVAV",
        &[
            "",
            "font-kerning: none",
            "font-kerning: normal",
            "text-rendering: optimizeSpeed",
            "text-rendering: optimizeSpeed; font-kerning: normal",
        ],
    );
    let advance: Vec<f32> = av.iter().map(|(_, advance)| *advance).collect();
    assert!(advance[1] > advance[0], "kerned closer: {advance:?}");
    assert_eq!(advance[2], advance[0]);
    assert_eq!(advance[3], advance[1]);
    assert_eq!(advance[4], advance[0]);
}

/// Numerals: Lato's digits are tabular, `proportional-nums` gives them
/// their own advances, `oldstyle-nums` and `super` their own glyphs.
#[test]
fn numerals_and_position() {
    let one = lato(
        "1",
        &[
            "",
            "font-variant-numeric: proportional-nums",
            "font-variant-numeric: oldstyle-nums",
            "font-variant-position: super",
        ],
    );
    let zero = lato("0", &["", "font-variant-numeric: proportional-nums"]);
    assert_eq!(one[0].1, zero[0].1, "tabular by default");
    assert_ne!(one[1].1, zero[1].1, "proportional");
    assert_ne!(one[2].0, one[0].0, "oldstyle");
    assert_ne!(one[3].0, one[0].0, "superior");
    // `font-feature-settings` comes last: it can turn one back off.
    let back = lato(
        "1",
        &["font-variant-numeric: proportional-nums; font-feature-settings: \"pnum\" 0"],
    );
    assert_eq!(back[0].1, one[0].1);
}

/// `font-synthesis`: Lato has one face (Medium, upright), so `bold` is
/// emboldened and `italic` slanted, unless `font-synthesis` says not to.
#[test]
fn synthesis() {
    let runs = runs(
        "<div style='font: bold italic 20px Lato'>x</div>\
         <div style='font: bold italic 20px Lato; font-synthesis: none'>x</div>\
         <div style='font: bold italic 20px Lato; font-synthesis: style'>x</div>",
    );
    let synthesized: Vec<(bool, bool)> = runs
        .iter()
        .map(|run| (run.embolden, run.skew != 0.0))
        .collect();
    assert_eq!(synthesized, [(true, true), (false, false), (false, true)]);
}

/// Small caps and the like are the font's: WPT's `FontWithFeatures2` has
/// a glyph for each feature on one of its capitals; a font without them
/// (Lato) keeps its glyphs (no small caps synthesized).
#[test]
fn caps_and_other_variants_take_the_fonts_glyphs() {
    let (_, families) = runs_with("", &["FontWithFeatures2.otf"]);
    let family = &families[0];
    let text = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let blocks: String = [
        "",
        "font-variant-caps: small-caps",
        "font-variant-caps: all-small-caps",
        "font-variant-numeric: slashed-zero",
        "font-variant-alternates: historical-forms",
    ]
    .iter()
    .map(|css| format!("<div style='font: 20px \"{family}\"; {css}'>{text}</div>"))
    .collect();
    let (runs, _) = runs_with(&blocks, &["FontWithFeatures2.otf"]);
    let glyphs = glyphs(&runs);
    for (i, what) in ["smcp", "c2sc", "zero", "hist"].iter().enumerate() {
        assert_ne!(glyphs[i + 1], glyphs[0], "{what}");
    }
    assert_ne!(glyphs[2], glyphs[1], "all-small-caps has c2sc too");
    let lato = lato("a", &["", "font-variant-caps: small-caps"]);
    assert_eq!(lato[0].0, lato[1].0, "none synthesized");
}

/// `font-stretch` picks the face of the nearest width (Exo 2's condensed,
/// normal and expanded faces).
#[test]
fn stretch_picks_the_width() {
    let fonts = [
        "Exo2-SemiBold.otf",
        "Exo2-SemiBoldCondensed.otf",
        "Exo2-SemiBoldExpanded.otf",
    ];
    let (_, families) = runs_with("", &fonts);
    let family = &families[0];
    let (runs, _) = runs_with(
        &format!(
            "<div style='font: 600 20px \"{family}\"'>x</div>\
             <div style='font: 600 condensed 20px \"{family}\"'>x</div>\
             <div style='font: 600 20px \"{family}\"; font-stretch: 130%'>x</div>"
        ),
        &fonts,
    );
    let ids: Vec<u64> = runs.iter().map(|run| run.font.id).collect();
    assert_ne!(ids[0], ids[1]);
    assert_ne!(ids[0], ids[2]);
    assert_ne!(ids[1], ids[2]);
    // An element's own.
    let (spans, _) = runs_with(
        &format!(
            "<div style='font: 600 20px \"{family}\"'>x<span style='font-stretch: condensed'>x</span></div>"
        ),
        &fonts,
    );
    assert_ne!(spans[1].font.id, spans[0].font.id);
}

/// A variable font's axes, as the glyphs are drawn at: `wght` for the
/// weight, `font-variation-settings`' (later winning over the weight's),
/// and `opsz` at the font size for `font-optical-sizing: auto` unless the
/// settings set it; none for a font without axes (Lato).
#[test]
fn a_variable_fonts_axes() {
    let (_, families) = runs_with("", &["RobotoExtremo-VF.subset.ttf"]);
    let family = &families[0];
    let (runs, _) = runs_with(
        &format!(
            "<div style='font: 20px \"{family}\"'>text</div>\
             <div style='font: 700 20px \"{family}\"'>text</div>\
             <div style='font: 700 20px \"{family}\"; font-variation-settings: \"wght\" 300'>text</div>\
             <div style='font: 20px \"{family}\"; font-optical-sizing: none'>text</div>\
             <div style='font: 20px \"{family}\"; font-variation-settings: \"opsz\" 40'>text</div>\
             <div style='font: 20px Lato'>text</div>\
             <div style='font: 700 20px \"{family}\"; font-optical-sizing: none'>text</div>             <div style='font: 20px \"{family}\"; font-optical-sizing: none; font-variation-settings: \"opsz\" 20'>text</div>"
        ),
        &["RobotoExtremo-VF.subset.ttf"],
    );
    let axis = |run: &TextFragment, tag: &[u8; 4]| {
        run.variations
            .iter()
            .find(|v| &v.tag == tag)
            .map(|v| v.value)
    };
    assert_eq!(axis(&runs[0], b"opsz"), Some(20.0), "the font size");
    assert_eq!(axis(&runs[1], b"wght"), Some(700.0));
    assert_eq!(axis(&runs[2], b"wght"), Some(300.0), "the settings win");
    assert_eq!(axis(&runs[3], b"opsz"), None);
    assert_eq!(axis(&runs[4], b"opsz"), Some(40.0));
    assert!(runs[5].variations.is_empty(), "no axes");
    // Off: no `opsz`, where the weight gives the run axes.
    assert_eq!(axis(&runs[6], b"wght"), Some(700.0));
    assert_eq!(axis(&runs[6], b"opsz"), None);
    // Shaped with them: the weight's advance differs, and the optical
    // size's (a layout of the same text with optical sizing off isn't
    // given the first's shaping).
    assert_ne!(runs[0].advance, runs[1].advance);
    assert_ne!(runs[1].advance, runs[2].advance, "the settings' weight");
    assert_ne!(runs[0].advance, runs[3].advance);
    assert_ne!(runs[0].advance, runs[4].advance, "the settings' size");
    assert_ne!(runs[3].advance, runs[4].advance);
    assert_eq!(runs[0].advance, runs[7].advance, "as the axis set to it");
    // An element's own, in a line of its parent's: the variation settings
    // and optical sizing it sets, each part shaped with its own.
    let (spans, _) = runs_with(
        &format!(
            "<div style='font: 20px \"{family}\"'>text<span style='font-optical-sizing: none'>text</span>\
             <span style='font-variation-settings: \"wght\" 900'>text</span></div>"
        ),
        &["RobotoExtremo-VF.subset.ttf"],
    );
    assert_eq!(spans.len(), 3);
    assert_eq!(spans[0].advance, runs[0].advance);
    assert_eq!(spans[1].advance, runs[3].advance, "shaped without opsz");
    assert_eq!(axis(&spans[1], b"opsz"), None);
    assert_eq!(axis(&spans[2], b"wght"), Some(900.0));
    assert_ne!(spans[2].advance, spans[0].advance, "shaped at it");
}
