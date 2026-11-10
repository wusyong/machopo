//! Our change to parley (the `machopo` branch of our fork, next to machopo at
//! `../third_party/parley`; see its README.machopo.md) that shares a layout's
//! shaped text (`Layout::shaped_text`): a glyph run's `GlyphSpan` walks
//! its glyphs and clusters from it once the layout has moved on, breaking
//! the lines again leaves a kept one as it was, and the kept shapings
//! share it.
//!
//! Ahem at 20px: every glyph, space included, is 20px wide.

use std::sync::Arc;

use parley::fontique::{Blob, Collection, CollectionOptions, SourceCache};
use parley::{
    Alignment, AlignmentOptions, FontContext, FontFamily, FontFamilyName, Glyph, Layout,
    LayoutContext, PositionedLayoutItem, ShapedText, StyleProperty, TabStops,
};

static AHEM: &[u8] = include_bytes!("../../../../resources/test_fonts/Ahem.ttf");

/// A font context with Ahem only, and a layout context whose kept shapings
/// layouts built with it share.
fn contexts() -> (FontContext, LayoutContext<u32>) {
    let mut collection = Collection::new(CollectionOptions {
        shared: false,
        system_fonts: false,
    });
    collection.register_fonts(Blob::new(Arc::new(AHEM)), None);
    let fonts = FontContext {
        collection,
        source_cache: SourceCache::default(),
    };
    (fonts, LayoutContext::new())
}

type Styles<'a> = &'a [(std::ops::Range<usize>, StyleProperty<'static, u32>)];

/// `text` in 20px Ahem, with `styles` over parts of it.
fn build(
    contexts: &mut (FontContext, LayoutContext<u32>),
    text: &str,
    styles: Styles<'_>,
) -> Layout<u32> {
    let mut layout = Layout::new();
    build_into(contexts, &mut layout, text, styles);
    layout
}

/// `build`'s layout, built into `layout` (a layout used again).
fn build_into(
    (fonts, cx): &mut (FontContext, LayoutContext<u32>),
    layout: &mut Layout<u32>,
    text: &str,
    styles: Styles<'_>,
) {
    let mut builder = cx.ranged_builder(fonts, text, 1.0, true);
    builder.push_default(StyleProperty::FontFamily(FontFamily::Single(
        FontFamilyName::named("Ahem"),
    )));
    builder.push_default(StyleProperty::FontSize(20.0));
    builder.push_default(StyleProperty::TabStops(Some(TabStops {
        interval: 160.0,
        min_advance: 10.0,
    })));
    for (range, style) in styles {
        builder.push(style.clone(), range.clone());
    }
    builder.build_into(layout, text);
}

/// Each glyph run's glyphs from its span and from the run.
fn glyphs(layout: &Layout<u32>) -> Vec<(Vec<Glyph>, Vec<Glyph>)> {
    layout
        .lines()
        .flat_map(|line| line.items())
        .filter_map(|item| match item {
            PositionedLayoutItem::GlyphRun(run) => Some((
                run.span().glyphs(layout.shaped_text()).collect(),
                run.glyphs().collect(),
            )),
            PositionedLayoutItem::InlineBox(_) => None,
        })
        .collect()
}

type Clusters = Vec<(std::ops::Range<usize>, f32)>;

/// Each glyph run's clusters (text and advance) and where their text is,
/// from its span and from the run's clusters of its style.
#[expect(clippy::type_complexity, reason = "test output")]
fn clusters(
    layout: &Layout<u32>,
) -> Vec<(
    (Clusters, Option<(usize, usize)>),
    (Clusters, Option<(usize, usize)>),
)> {
    layout
        .lines()
        .flat_map(|line| line.items())
        .filter_map(|item| match item {
            PositionedLayoutItem::GlyphRun(run) => {
                let span = run.span();
                let shaped = layout.shaped_text();
                let from_span: Clusters = span
                    .clusters(shaped)
                    .map(|c| (c.text_range, c.advance))
                    .collect();
                let from_run: Clusters = run
                    .run()
                    .visual_clusters()
                    .filter(|c| c.style_index() == run.style_index())
                    .map(|c| (c.text_range(), c.advance()))
                    .collect();
                let text = from_run.first().map(|(first, _)| {
                    let end = from_run.iter().map(|(text, _)| text.end).max();
                    (first.start, end.unwrap_or(first.end))
                });
                Some(((from_span, span.text_range(shaped)), (from_run, text)))
            }
            PositionedLayoutItem::InlineBox(_) => None,
        })
        .collect()
}

/// The advance of each shaped cluster of `shaped` (a cluster of one
/// glyph holds it itself).
fn advances(shaped: &ShapedText) -> Vec<f32> {
    shaped
        .shaped_clusters()
        .iter()
        .map(|cluster| cluster.advance)
        .collect()
}

#[test]
fn a_spans_glyphs_are_its_runs() {
    let cx = &mut contexts();
    // Letter spacing, a justified line, right to left (an override: Ahem
    // has no right-to-left letters), and a glyph run that is part of a
    // shaped run (styled otherwise only in what doesn't change shaping).
    let mut layout = build(
        cx,
        "aa bb cc \u{202e}xyz\u{202c} dd ee",
        &[
            (0..2, StyleProperty::LetterSpacing(3.0)),
            (4..5, StyleProperty::Underline(true)),
        ],
    );
    layout.set_alignment(Alignment::Justify, AlignmentOptions::default());
    layout.break_all_lines(Some(250.0));
    let runs = glyphs(&layout);
    assert!(runs.len() >= 3, "{runs:?}");
    for (span, run) in &runs {
        assert_eq!(span, run);
    }
    // The letter spacing and the line's justification (a space after it)
    // are in the glyphs.
    assert_eq!(runs[0].1[0].advance, 23.0);
    assert!(runs[1].1[0].advance > 20.0, "{runs:?}");
    // The override's run, from the right (after its control character's
    // glyph).
    let mut ids = |text: &str| -> Vec<u32> {
        let mut layout = build(cx, text, &[]);
        layout.break_all_lines(None);
        glyphs(&layout)[0].1.iter().map(|glyph| glyph.id).collect()
    };
    let mut reversed = ids("zyx");
    assert!(runs.iter().any(|(span, _)| {
        let span: Vec<u32> = span.iter().map(|glyph| glyph.id).collect();
        span.ends_with(&reversed)
    }));
    reversed.reverse();
    assert_eq!(reversed, ids("xyz"));
}

#[test]
fn a_spans_clusters_are_its_runs() {
    let cx = &mut contexts();
    // As `a_spans_glyphs_are_its_runs`, with a newline (a cluster without
    // glyphs) and white space hanging past a line.
    let text = "aa bb cc \u{202e}xyz\u{202c} dd ee            ff\ngg";
    let mut layout = build(
        cx,
        text,
        &[
            (0..2, StyleProperty::LetterSpacing(3.0)),
            (4..5, StyleProperty::Underline(true)),
        ],
    );
    layout.set_alignment(Alignment::Justify, AlignmentOptions::default());
    layout.break_all_lines(Some(250.0));
    let runs = clusters(&layout);
    assert!(runs.len() >= 4, "{runs:?}");
    for (span, run) in &runs {
        assert_eq!(span, run);
    }
    // The letter spacing and the justification are in the advances; the
    // underlined "b" is a run of its own in the middle of a shaped run
    // (whose other style's clusters are all of that run's on the line).
    assert_eq!(runs[0].0.0[0], (0..1, 23.0));
    assert!(runs[1].0.0[0].1 > 20.0, "{runs:?}");
    assert_eq!(runs[2].0, (vec![(4..5, 20.0)], Some((4, 5))));
    // The override's text starts on the right, with what ends it; the
    // newline is a cluster.
    let (pop, newline) = (text.find('\u{202c}').unwrap(), text.find('\n').unwrap());
    assert!(
        runs.iter().any(|((span, range), _)| {
            *range == Some((pop, pop + 3)) && span.last().is_some_and(|(x, _)| x.start == pop - 3)
        }),
        "{runs:?}"
    );
    assert!(
        runs.iter()
            .any(|((span, _), _)| span.iter().any(|(text, _)| text.start == newline)),
        "{runs:?}"
    );
}

#[test]
fn breaking_lines_again_leaves_a_kept_shaped_text_as_it_was() {
    let cx = &mut contexts();
    // The tab after "aaaa b" (120px) reaches 160px; on the second line, after
    // "b" (20px), it does too: 40px, then 140px.
    let mut layout = build(cx, "aaaa b\tc", &[]);
    layout.break_all_lines(Some(1000.0));
    let kept = Arc::clone(layout.shaped_text());
    let wide = advances(&kept);
    assert_eq!(wide[6], 40.0);
    layout.break_all_lines(Some(100.0));
    assert_eq!(advances(layout.shaped_text())[6], 140.0);
    assert_eq!(advances(&kept), wide);
    assert!(!Arc::ptr_eq(&kept, layout.shaped_text()));
}

#[test]
fn a_kept_shaping_is_shared() {
    let cx = &mut contexts();
    let first = build(cx, "aa bb", &[]);
    let alike = build(cx, "aa bb", &[]);
    assert!(Arc::ptr_eq(first.shaped_text(), alike.shaped_text()));
    // Styled otherwise where it doesn't change shaping: a copy with its own
    // styles.
    let underlined = build(cx, "aa bb", &[(3..5, StyleProperty::Underline(true))]);
    assert!(!Arc::ptr_eq(first.shaped_text(), underlined.shaped_text()));
    assert_eq!(
        advances(first.shaped_text()),
        advances(underlined.shaped_text())
    );
    let style = |layout: &Layout<u32>| -> Vec<u16> {
        let shaped = layout.shaped_text();
        shaped
            .characters()
            .iter()
            .map(|c| c.style_index)
            .chain(shaped.shaped_clusters().iter().map(|c| c.style_index))
            .collect()
    };
    assert_eq!(style(&first), [0; 10]);
    assert_eq!(style(&underlined), [0, 0, 0, 1, 1, 0, 0, 0, 1, 1]);
}

#[test]
fn a_layout_built_again_leaves_a_kept_shaped_text_as_it_was() {
    let cx = &mut contexts();
    let mut layout = build(cx, "aa bb", &[]);
    // Held by the kept shapings too.
    let kept = Arc::clone(layout.shaped_text());
    build_into(cx, &mut layout, "ccc", &[]);
    assert_eq!(kept.characters().len(), 5);
    assert_eq!(layout.shaped_text().characters().len(), 3);
    // Built again, alike, it shares the kept shaping again.
    build_into(cx, &mut layout, "aa bb", &[]);
    assert!(Arc::ptr_eq(&kept, layout.shaped_text()));
    // Cleared, it lets go of it (and of the fonts it holds).
    layout.clear();
    assert!(layout.shaped_text().characters().is_empty());
    assert_eq!(kept.characters().len(), 5);
}
