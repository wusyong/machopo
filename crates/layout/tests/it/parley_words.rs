//! Our change to parley (the `machopo` branch of our fork, next to machopo at
//! `../third_party/parley`; see its README.machopo.md) that shapes text a word
//! at a time and keeps what shaping a word gave, with fonts the fork's own
//! tests don't have (those make theirs, a rule or two each): Lato has a
//! contextual alternate reaching across a space, and is shaped whole; some
//! of the WPT's take a space with the glyph beside it, and the words
//! around it are shaped again as one. In a debug build every text shaped a
//! word at a time is also shaped whole and compared.

use std::sync::Arc;

use parley::fontique::{Blob, Collection, CollectionOptions, SourceCache};
use parley::{FontContext, FontFamily, FontFamilyName, Layout, LayoutContext, StyleProperty};

static AHEM: &[u8] = include_bytes!("../../../../resources/test_fonts/Ahem.ttf");
static LATO: &[u8] = include_bytes!("../../../../resources/test_fonts/Lato-Medium.ttf");

type Contexts = (FontContext, LayoutContext<u32>);

/// A font context with the fonts in `files` only.
fn contexts_with(files: &[Vec<u8>]) -> Contexts {
    let mut collection = Collection::new(CollectionOptions {
        shared: false,
        system_fonts: false,
    });
    for file in files {
        collection.register_fonts(Blob::new(Arc::new(file.clone())), None);
    }
    let fonts = FontContext {
        collection,
        source_cache: SourceCache::default(),
    };
    (fonts, LayoutContext::new())
}

type Styles<'a> = &'a [(std::ops::Range<usize>, StyleProperty<'static, u32>)];

/// `text` in 20px `family`, with `styles` over parts of it.
fn build(
    fonts: &mut FontContext,
    cx: &mut LayoutContext<u32>,
    family: &str,
    text: &str,
    styles: Styles<'_>,
) -> Layout<u32> {
    let mut builder = cx.ranged_builder(fonts, text, 1.0, true);
    builder.push_default(StyleProperty::FontFamily(FontFamily::Single(
        FontFamilyName::named(family),
    )));
    builder.push_default(StyleProperty::FontSize(20.0));
    for (range, style) in styles {
        builder.push(style.clone(), range.clone());
    }
    builder.build(text)
}

/// How many words laying out `text` took from those shaped before, and how
/// many it shaped and kept.
fn words(cx: &mut Contexts, family: &str, text: &str, styles: Styles<'_>) -> (usize, usize) {
    let (reused, shaped) = cx.1.words_reused_and_shaped();
    build(&mut cx.0, &mut cx.1, family, text, styles);
    let (now_reused, now_shaped) = cx.1.words_reused_and_shaped();
    (now_reused - reused, now_shaped - shaped)
}

#[test]
fn text_in_lato_is_shaped_whole() {
    let mut cx = contexts_with(&[AHEM.to_vec(), LATO.to_vec()]);
    assert_eq!(words(&mut cx, "Lato", "aa bb aa", &[]), (0, 0));
    // So is a text with a word in it.
    let lato = StyleProperty::FontFamily(FontFamily::Single(FontFamilyName::named("Lato")));
    assert_eq!(words(&mut cx, "Ahem", "aa bb aa", &[(3..5, lato)]), (0, 0));
    // Without one, its words are kept.
    let mut cx = contexts_with(&[AHEM.to_vec(), LATO.to_vec()]);
    assert_eq!(words(&mut cx, "Ahem", "aa bb aa", &[]), (2, 3));
}

/// A font file of the WPT's, at `path` in it.
fn wpt_font(path: &str) -> Vec<u8> {
    let path = format!(
        "{}/../../../third_party/wpt/{path}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

#[test]
fn words_beside_a_space_a_font_shapes_with_them_are_shaped_again_as_one() {
    for (path, text, joined) in [
        // A substitution taking `ि` (or what it becomes) and a space after
        // it: shaping puts it before its consonant, never before a space.
        (
            "fonts/noto/NotoSansDevanagari-Regular.ttf",
            "\u{915}\u{93f} \u{916} \u{915}\u{93f}",
            0,
        ),
        // Kerning with a space: `T` after one, `V` before one.
        (
            "css/css-fonts/support/fonts/Exo2-SemiBold.otf",
            "aa bb aa",
            0,
        ),
        ("css/css-fonts/support/fonts/Exo2-SemiBold.otf", "aa Ta", 1),
        ("css/css-fonts/support/fonts/Exo2-SemiBold.otf", "aV aa", 1),
        // Neither.
        ("fonts/Lato-Bold.ttf", "aa bb aa", 0),
        ("css/css-fonts/support/fonts/Rochester.otf", "aa bb aa", 0),
    ] {
        let mut cx = contexts_with(&[wpt_font(path)]);
        let family = cx.0.collection.family_names().next().unwrap().to_owned();
        let (reused, shaped) = words(&mut cx, &family, text, &[]);
        assert!(reused + shaped > 0, "{path}: {text}");
        assert_eq!(cx.1.words_joined(), joined, "{path}: {text}");
    }
}
