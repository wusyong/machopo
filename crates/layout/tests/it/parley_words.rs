//! Our change to parley (the `machopo` branch of our fork, next to machopo at
//! `../third_party/parley`; see its README.machopo.md) that shapes text a word
//! at a time and keeps what shaping a word gave: text is split before each
//! space and after it, and a word is shaped again only with other fonts
//! asked for (families, width, weight, style) or other shaping options. It
//! does so only where shaping a word on its own gives what shaping the
//! text whole does: left to right, and with fonts whose rules take a space
//! with the glyph beside it at most, where words around a space taken so
//! are shaped again as one (Lato has a contextual alternate reaching across
//! a space, and is shaped whole; Ahem has no rules; `parley_space` makes
//! fonts with one rule or two). In a debug build every text shaped a word
//! at a time is also shaped whole and compared.

use std::borrow::Cow;
use std::sync::Arc;

use parley::fontique::{Blob, Collection, CollectionOptions, SourceCache};
use parley::{
    FontContext, FontFamily, FontFamilyName, FontFeatures, FontVariations, FontWeight, Language,
    Layout, LayoutContext, StyleProperty,
};

pub(crate) static AHEM: &[u8] = include_bytes!("../../../../resources/test_fonts/Ahem.ttf");
static LATO: &[u8] = include_bytes!("../../../../resources/test_fonts/Lato-Medium.ttf");

type Contexts = (FontContext, LayoutContext<u32>);

/// A font context with Ahem and Lato only.
fn contexts() -> Contexts {
    contexts_with(&[AHEM.to_vec(), LATO.to_vec()])
}

/// A font context with the fonts in `files` only.
pub(crate) fn contexts_with(files: &[Vec<u8>]) -> Contexts {
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
pub(crate) fn build(
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

/// `words` of `text` in Ahem, all of it with `style`.
fn ahem_styled(
    cx: &mut Contexts,
    text: &str,
    style: StyleProperty<'static, u32>,
) -> (usize, usize) {
    words(cx, "Ahem", text, &[(0..text.len(), style)])
}

#[test]
fn a_word_is_shaped_once() {
    let mut cx = contexts();
    // `aa`, ` ` and `bb` are shaped; the rest are taken.
    assert_eq!(words(&mut cx, "Ahem", "aa bb aa bb aa", &[]), (6, 3));
    // Another text takes the words shaped before.
    assert_eq!(words(&mut cx, "Ahem", "bb aa cc", &[]), (4, 1));
}

#[test]
fn text_is_split_before_and_after_each_space() {
    // `aa`, ` `, ` `, `bb`.
    assert_eq!(words(&mut contexts(), "Ahem", "aa  bb", &[]), (1, 3));
    // A space's combining mark stays with it: `aa`, ` \u{301}bb`.
    assert_eq!(words(&mut contexts(), "Ahem", "aa \u{301}bb", &[]), (0, 2));
}

#[test]
fn a_long_word_is_not_kept() {
    let a32 = "a".repeat(32);
    let a33 = "a".repeat(33);
    assert_eq!(
        words(&mut contexts(), "Ahem", &format!("{a32} {a32}"), &[]),
        (1, 2)
    );
    // Only the space is kept.
    assert_eq!(
        words(&mut contexts(), "Ahem", &format!("{a33} {a33}"), &[]),
        (0, 1)
    );
}

#[test]
fn a_word_asking_for_other_fonts_or_options_is_shaped_again() {
    let bold = StyleProperty::FontWeight(FontWeight::BOLD);
    assert_eq!(
        words(&mut contexts(), "Ahem", "aa aa", &[(3..5, bold)]),
        (0, 3)
    );
    // A color asks for the same fonts.
    let color = StyleProperty::Brush(1);
    assert_eq!(
        words(&mut contexts(), "Ahem", "aa aa", &[(3..5, color)]),
        (1, 2)
    );
    let mut cx = contexts();
    assert_eq!(words(&mut cx, "Ahem", "aa", &[]), (0, 1));
    let larger = StyleProperty::FontSize(30.0);
    assert_eq!(ahem_styled(&mut cx, "aa", larger), (0, 1));
    let no_ligatures =
        StyleProperty::FontFeatures(FontFeatures::Source(Cow::Borrowed("\"liga\" 0")));
    assert_eq!(ahem_styled(&mut cx, "aa", no_ligatures), (0, 1));
    let weight_axis =
        StyleProperty::FontVariations(FontVariations::Source(Cow::Borrowed("\"wght\" 500")));
    assert_eq!(ahem_styled(&mut cx, "aa", weight_axis), (0, 1));
    let turkish = StyleProperty::Locale(Some(Language::parse("tr").unwrap()));
    assert_eq!(ahem_styled(&mut cx, "aa", turkish), (0, 1));
}

#[test]
fn a_word_in_another_script_is_shaped_again() {
    // Ahem has no Greek: font selection takes it anyway, the first family.
    let mut cx = contexts_with(&[AHEM.to_vec()]);
    assert_eq!(words(&mut cx, "Ahem", "ab 12", &[]), (0, 3));
    // ` 12` takes the script of the Greek before it.
    assert_eq!(words(&mut cx, "Ahem", "\u{3b1}\u{3b2} 12", &[]), (0, 3));
}

/// Distinct words: `0` to `count - 1`.
fn numbers(count: usize) -> String {
    (0..count)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn the_words_kept_start_again_once_there_are_too_many() {
    let mut cx = contexts();
    // 2^15 words with the space.
    words(&mut cx, "Ahem", &numbers((1 << 15) - 1), &[]);
    assert_eq!(words(&mut cx, "Ahem", "0", &[]), (1, 0));
    // One more: the cache starts again with it.
    assert_eq!(words(&mut cx, "Ahem", "x", &[]), (0, 1));
    assert_eq!(words(&mut cx, "Ahem", "0 0", &[]), (1, 2));
}

#[test]
fn the_words_kept_start_again_with_the_ids_of_options() {
    let mut cx = contexts();
    let size = |i: u16| StyleProperty::FontSize(f32::from(i) + 1.0);
    // 2^12 sets of options take every id.
    for i in 0..1 << 12 {
        assert_eq!(ahem_styled(&mut cx, "aa", size(i)), (0, 1));
    }
    // Another takes one given out before: no word kept with it is taken.
    assert_eq!(ahem_styled(&mut cx, "aa", size(1 << 12)), (0, 1));
    // The first takes a new one.
    assert_eq!(ahem_styled(&mut cx, "aa aa", size(0)), (1, 2));
}

#[test]
fn the_words_kept_start_again_with_the_ids_of_font_styles() {
    let mut cx = contexts();
    let weight = |i: u16| StyleProperty::FontWeight(FontWeight::new(1.0 + f32::from(i) / 8.0));
    // 2^12 font styles take every id.
    for i in 0..1 << 12 {
        assert_eq!(ahem_styled(&mut cx, "aa", weight(i)), (0, 1));
    }
    // Another (bold enough to embolden) takes one given out before: no word
    // kept with it is taken.
    assert_eq!(ahem_styled(&mut cx, "aa", weight(1 << 12)), (0, 1));
    // The first takes a new one.
    assert_eq!(ahem_styled(&mut cx, "aa aa", weight(0)), (1, 2));
}

#[test]
fn text_is_shaped_whole_where_words_would_shape_otherwise() {
    // Lato shapes a space together with what is around it.
    assert_eq!(words(&mut contexts(), "Lato", "aa bb aa", &[]), (0, 0));
    // So does a text with a word in it.
    let lato = StyleProperty::FontFamily(FontFamily::Single(FontFamilyName::named("Lato")));
    assert_eq!(
        words(&mut contexts(), "Ahem", "aa bb aa", &[(3..5, lato)]),
        (0, 0)
    );
    // A feature that is off unless asked for: what a font does with it isn't
    // looked into.
    let small_caps = StyleProperty::FontFeatures(FontFeatures::Source(Cow::Borrowed("\"smcp\"")));
    assert_eq!(
        words(&mut contexts(), "Ahem", "aa bb aa", &[(0..8, small_caps)]),
        (0, 0)
    );
    // Right to left: the words of a run go the other way.
    assert_eq!(
        words(
            &mut contexts(),
            "Ahem",
            "\u{5d0}\u{5d1} \u{5d0}\u{5d1}",
            &[]
        ),
        (0, 0)
    );
    // Arabic, even left to right: harfrust's Arabic shaper looks at the
    // text around a run. (Ahem has no Arabic: font selection takes it
    // anyway, the first family.)
    assert_eq!(
        words(
            &mut contexts_with(&[AHEM.to_vec()]),
            "Ahem",
            "\u{202d}\u{628}\u{628} \u{628}\u{628}\u{202c}",
            &[]
        ),
        (0, 0)
    );
    // A font met after runs of others were put together: the segment is
    // shaped whole, without them.
    let bold = StyleProperty::FontWeight(FontWeight::BOLD);
    let lato = StyleProperty::FontFamily(FontFamily::Single(FontFamilyName::named("Lato")));
    assert_eq!(
        words(
            &mut contexts(),
            "Ahem",
            "aa bb cc",
            &[(3..5, bold), (6..8, lato)]
        ),
        (0, 0)
    );
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

#[test]
fn words_are_shaped_again_once_cleared() {
    let mut cx = contexts();
    assert_eq!(words(&mut cx, "Ahem", "aa", &[]), (0, 1));
    cx.1.clear_shapings();
    assert_eq!(words(&mut cx, "Ahem", "aa bb", &[]), (0, 3));
}

/// Text in many scripts, so that fonts fall back inside words, with
/// repeated words.
const TEXTS: &[&str] = &[
    "The quick brown fox jumps over the lazy dog. AV To Wa \u{201c}Yes,\u{201d} 1/2 fi fl ffi -- a  b\tc. The fox, the dog: the end.",
    "\u{41f}\u{440}\u{438}\u{432}\u{435}\u{442} \u{43c}\u{438}\u{440} \u{41f}\u{440}\u{438}\u{432}\u{435}\u{442}. \u{393}\u{3b5}\u{3b9}\u{3ac} \u{3c3}\u{3bf}\u{3c5}. \u{4f60}\u{597d}\u{ff0c}\u{4e16}\u{754c} \u{4f60}\u{597d}. \u{3053}\u{3093}\u{306b}\u{3061}\u{306f} \u{4e16}\u{754c}. \u{c548}\u{b155} \u{c138}\u{c0c1} \u{c548}\u{b155}.",
    "\u{928}\u{92e}\u{938}\u{94d}\u{924}\u{947} \u{926}\u{941}\u{928}\u{93f}\u{92f}\u{93e} \u{928}\u{92e}\u{938}\u{94d}\u{924}\u{947}. \u{e2a}\u{e27}\u{e31}\u{e2a}\u{e14}\u{e35} \u{e0a}\u{e32}\u{e27}\u{e42}\u{e25}\u{e01}. \u{1f600} \u{1f44d}\u{1f3fd} a\u{1f600}b \u{1f600}",
    "\u{5e9}\u{5dc}\u{5d5}\u{5dd} hello \u{5e2}\u{5d5}\u{5dc}\u{5dd}. \u{645}\u{631}\u{62d}\u{628}\u{627} hello world. a\u{301} e\u{301}e x\u{20dd} a\u{200d}b a\u{200b}b \u{301}a",
];

/// With each of the system's font families, text shaped a word at a time
/// is what shaping it whole gives (in a release build too).
#[test]
fn system_fonts_shape_words_as_whole_text() {
    let mut fonts = FontContext::new();
    let families: Vec<String> = fonts.collection.family_names().map(str::to_owned).collect();
    let mut words = LayoutContext::new();
    let mut whole = LayoutContext::new();
    whole.set_shape_words(false);
    for family in &families {
        for text in TEXTS {
            let by_words = build(&mut fonts, &mut words, family, text, &[]);
            let by_whole = build(&mut fonts, &mut whole, family, text, &[]);
            assert!(
                **by_words.shaped_text() == **by_whole.shaped_text(),
                "{family}: {text}"
            );
        }
    }
    eprintln!(
        "{} families; words taken, shaped: {:?}",
        families.len(),
        words.words_reused_and_shaped()
    );
}
