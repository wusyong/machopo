//! How far shaping with a font reaches across a space, in our change to
//! parley that shapes text a word at a time (see `parley_words`): a font
//! whose rules take a space only with the glyph beside it is shaped a word
//! at a time, and where a space and the glyph beside it are taken together
//! the words around it are shaped again as one; a font whose rules reach
//! further is shaped whole. The fonts are made here, a rule or two each,
//! and every text is also shaped whole and compared.

use write_fonts::FontBuilder;
use write_fonts::tables::cmap::Cmap;
use write_fonts::tables::gdef::Gdef;
use write_fonts::tables::gpos::{
    AnchorTable, BaseArray, BaseRecord, Class1Record, Class2Record, ComponentRecord,
    CursivePosFormat1, EntryExitRecord, LigatureArray, LigatureAttach, Mark2Array, Mark2Record,
    MarkArray, MarkBasePosFormat1, MarkLigPosFormat1, MarkMarkPosFormat1, MarkRecord, PairPos,
    PairSet, PairValueRecord, PositionChainContext, PositionLookup, PositionSequenceContext,
    ValueRecord,
};
use write_fonts::tables::gsub::{
    Gsub, Ligature, LigatureSet, LigatureSubstFormat1, MultipleSubstFormat1,
    ReverseChainSingleSubstFormat1, Sequence, SingleSubst, SubstitutionChainContext,
    SubstitutionLookup, SubstitutionSequenceContext,
};
use write_fonts::tables::head::Head;
use write_fonts::tables::hhea::Hhea;
use write_fonts::tables::hmtx::{Hmtx, LongMetric};
use write_fonts::tables::layout::{
    ChainedClassSequenceRule, ChainedClassSequenceRuleSet, ChainedSequenceContext,
    ChainedSequenceRule, ChainedSequenceRuleSet, ClassDef, ClassSequenceRule, ClassSequenceRuleSet,
    CoverageTable, Feature, FeatureList, FeatureRecord, LangSys, Lookup, LookupFlag, LookupList,
    Script, ScriptList, ScriptRecord, SequenceContext, SequenceLookupRecord, SequenceRule,
    SequenceRuleSet,
};
use write_fonts::tables::maxp::Maxp;
use write_fonts::tables::name::{Name, NameRecord};
use write_fonts::tables::os2::Os2;
use write_fonts::tables::post::Post;
use write_fonts::types::{FWord, GlyphId, GlyphId16, NameId, Tag, UfWord};

use parley::LayoutContext;

use crate::parley_words::{AHEM, build, contexts_with};

/// The characters of the fonts made here, glyph 1 on (0 is `.notdef`):
/// each a 1000-unit advance, in an em of 1000.
const CHARS: [char; 5] = [' ', 'a', 'b', 'T', '\u{301}'];

/// How many glyphs follow without characters, for substitutions to make:
/// each a 300-unit advance.
const EXTRAS: u16 = 300;

/// The glyph of `ch`.
fn g(ch: char) -> GlyphId16 {
    let index = CHARS.iter().position(|&c| c == ch).unwrap();
    GlyphId16::new(u16::try_from(index).unwrap() + 1)
}

/// The `index`th glyph without a character.
fn extra(index: u16) -> GlyphId16 {
    assert!(index < EXTRAS);
    GlyphId16::new(u16::try_from(CHARS.len()).unwrap() + 1 + index)
}

/// What a font made here shapes with.
#[derive(Default)]
struct Rules {
    gsub: Vec<SubstitutionLookup>,
    gpos: Vec<PositionLookup>,
    /// The GSUB lookups `ccmp` applies (the rest only in contexts), if not
    /// all of them.
    ccmp: Option<Vec<u16>>,
    /// GSUB lookups `dlig` applies, a feature off unless asked for.
    dlig: Vec<u16>,
    /// The `kern` table's pairs.
    kern: Vec<(GlyphId16, GlyphId16, i16)>,
    /// A `kern` table of classes instead, kerning `T` and a space.
    kern_classes: bool,
    /// Whether GDEF tells U+0301 a mark.
    marks: bool,
    /// Whether the font has an AAT table.
    aat: bool,
    /// Its em.
    em: Em,
}

/// The em of a font made here.
#[derive(Default, Clone, Copy)]
enum Em {
    #[default]
    Thousand,
    /// 0 units: its glyphs would come out at NaN.
    Zero,
    /// No `head`, as an Apple bitmap font has `bhed` instead (macOS's
    /// GB18030 Bitmap): an em of 0 too.
    Missing,
}

/// A font named `Made` shaping with `rules`.
fn font(rules: Rules) -> Vec<u8> {
    let glyphs = u16::try_from(CHARS.len()).unwrap() + 1 + EXTRAS;
    let advance = |glyph: u16| {
        if glyph == 0 || usize::from(glyph) > CHARS.len() {
            300
        } else {
            1000
        }
    };
    let mut builder = FontBuilder::new();
    match rules.em {
        Em::Thousand | Em::Zero => {
            builder
                .add_table(&Head {
                    units_per_em: if matches!(rules.em, Em::Zero) {
                        0
                    } else {
                        1000
                    },
                    ..Default::default()
                })
                .unwrap();
        }
        Em::Missing => {}
    }
    builder
        .add_table(&Hhea {
            ascender: FWord::new(800),
            descender: FWord::new(-200),
            advance_width_max: UfWord::new(1000),
            number_of_h_metrics: glyphs,
            ..Default::default()
        })
        .unwrap()
        .add_table(&Maxp::new(glyphs))
        .unwrap()
        .add_table(&Hmtx::new(
            (0..glyphs)
                .map(|glyph| LongMetric::new(advance(glyph), 0))
                .collect(),
            Vec::new(),
        ))
        .unwrap()
        .add_table(
            &Cmap::from_mappings(CHARS.iter().map(|&ch| (ch, GlyphId::from(g(ch))))).unwrap(),
        )
        .unwrap()
        .add_table(&Name::new(
            [(1, "Made"), (2, "Regular"), (4, "Made"), (6, "Made")]
                .into_iter()
                .map(|(id, name)| {
                    NameRecord::new(3, 1, 0x409, NameId::new(id), name.to_owned().into())
                })
                .collect(),
        ))
        .unwrap()
        .add_table(&Os2 {
            us_weight_class: 400,
            us_width_class: 5,
            s_typo_ascender: 800,
            s_typo_descender: -200,
            us_win_ascent: 800,
            us_win_descent: 200,
            ..Default::default()
        })
        .unwrap()
        .add_table(&Post::default())
        .unwrap();
    if rules.marks {
        let classes = ClassDef::from_iter(
            CHARS
                .iter()
                .map(|&ch| (g(ch), if ch == '\u{301}' { 3 } else { 1 })),
        );
        builder
            .add_table(&Gdef::new(Some(classes), None, None, None))
            .unwrap();
    }
    if !rules.gsub.is_empty() {
        let count = u16::try_from(rules.gsub.len()).unwrap();
        let mut features = vec![(*b"ccmp", rules.ccmp.unwrap_or_else(|| (0..count).collect()))];
        if !rules.dlig.is_empty() {
            features.push((*b"dlig", rules.dlig));
        }
        builder
            .add_table(&Gsub::new(
                scripts(features.len()),
                feature_list(features),
                LookupList::new(rules.gsub),
            ))
            .unwrap();
    }
    if !rules.gpos.is_empty() {
        let count = u16::try_from(rules.gpos.len()).unwrap();
        builder
            .add_table(&write_fonts::tables::gpos::Gpos::new(
                scripts(1),
                feature_list(vec![(*b"kern", (0..count).collect())]),
                LookupList::new(rules.gpos),
            ))
            .unwrap();
    }
    if !rules.kern.is_empty() {
        builder.add_raw(Tag::new(b"kern"), kern_table(rules.kern));
    }
    if rules.kern_classes {
        builder.add_raw(Tag::new(b"kern"), kern_classes_table());
    }
    if rules.aat {
        // Version 2, no chains: harfrust shapes with it, and nothing happens.
        builder.add_raw(Tag::new(b"morx"), vec![0, 2, 0, 0, 0, 0, 0, 0]);
    }
    builder.build()
}

/// `DFLT` and `latn`, each with features `0..count`.
fn scripts(count: usize) -> ScriptList {
    let features: Vec<u16> = (0..u16::try_from(count).unwrap()).collect();
    ScriptList::new(
        [*b"DFLT", *b"latn"]
            .into_iter()
            .map(|tag| {
                ScriptRecord::new(
                    Tag::new(&tag),
                    Script::new(Some(LangSys::new(features.clone())), Vec::new()),
                )
            })
            .collect(),
    )
}

fn feature_list(features: Vec<([u8; 4], Vec<u16>)>) -> FeatureList {
    FeatureList::new(
        features
            .into_iter()
            .map(|(tag, lookups)| FeatureRecord::new(Tag::new(&tag), Feature::new(None, lookups)))
            .collect(),
    )
}

/// A `kern` table of one format 0 subtable with `pairs`.
fn kern_table(mut pairs: Vec<(GlyphId16, GlyphId16, i16)>) -> Vec<u8> {
    pairs.sort();
    let count = u16::try_from(pairs.len()).unwrap();
    let mut table = Vec::new();
    let mut push = |value: u16| table.extend_from_slice(&value.to_be_bytes());
    // Version 0, one subtable.
    push(0);
    push(1);
    // Version 0, its length, horizontal kerning, format 0.
    push(0);
    push(14 + 6 * count);
    push(1);
    push(count);
    let entry_selector = u16::try_from(count.max(1).ilog2()).unwrap();
    let search_range = 6 * (1 << entry_selector);
    push(search_range);
    push(entry_selector);
    push(6 * count - search_range);
    for (left, right, value) in pairs {
        push(left.to_u16());
        push(right.to_u16());
        push(u16::from_be_bytes(value.to_be_bytes()));
    }
    table
}

/// A `kern` table of one format 2 subtable, kerning `T` and a space by
/// -300: one row (`T`), of two columns (every other glyph, the space).
fn kern_classes_table() -> Vec<u8> {
    let mut table = Vec::new();
    let mut push = |value: u16| table.extend_from_slice(&value.to_be_bytes());
    // Version 0, one subtable.
    push(0);
    push(1);
    // Version 0, its length, horizontal kerning, format 2.
    push(0);
    push(30);
    push(0x0201);
    // The row's width in bytes, and where the classes and the values start
    // in the subtable.
    push(4);
    push(14);
    push(20);
    push(26);
    // The left classes: `T` is the row at the values' start.
    push(g('T').to_u16());
    push(1);
    push(26);
    // The right classes: the space is the second column.
    push(g(' ').to_u16());
    push(1);
    push(2);
    // The values.
    push(0);
    push(u16::from_be_bytes((-300_i16).to_be_bytes()));
    table
}

fn coverage(glyphs: &[GlyphId16]) -> CoverageTable {
    let mut glyphs = glyphs.to_vec();
    glyphs.sort();
    CoverageTable::format_1(glyphs)
}

fn classes(classes: &[(GlyphId16, u16)]) -> ClassDef {
    ClassDef::from_iter(classes.iter().copied())
}

fn moved(x_advance: i16) -> ValueRecord {
    ValueRecord::new().with_x_advance(x_advance)
}

/// Kerning `first` and `second` by -300, with `flag`.
fn pair(first: GlyphId16, second: GlyphId16, flag: LookupFlag) -> PositionLookup {
    PositionLookup::Pair(Lookup::new(
        flag,
        vec![PairPos::format_1(
            coverage(&[first]),
            vec![PairSet::new(vec![PairValueRecord::new(
                second,
                moved(-300),
                ValueRecord::new(),
            )])],
        )],
    ))
}

/// Substituting `from` with `to`.
fn single(from: GlyphId16, to: GlyphId16) -> SubstitutionLookup {
    SubstitutionLookup::Single(Lookup::new(
        LookupFlag::empty(),
        vec![SingleSubst::format_2(coverage(&[from]), vec![to])],
    ))
}

/// A ligature of `components` making `to`, with `flag`.
fn ligature(components: &[GlyphId16], to: GlyphId16, flag: LookupFlag) -> SubstitutionLookup {
    SubstitutionLookup::Ligature(Lookup::new(
        flag,
        vec![LigatureSubstFormat1::new(
            coverage(&components[..1]),
            vec![LigatureSet::new(vec![Ligature::new(
                to,
                components[1..].to_vec(),
            )])],
        )],
    ))
}

/// The second lookup applied at `index` of a context.
fn second_at(index: u16) -> Vec<SequenceLookupRecord> {
    vec![SequenceLookupRecord::new(index, 1)]
}

/// A substitution context; lookup 1 makes a space `extra(0)`, in context only.
fn in_context(context: SubstitutionLookup) -> Rules {
    Rules {
        gsub: vec![context, single(g(' '), extra(0))],
        ccmp: Some(vec![0]),
        ..Default::default()
    }
}

fn substitution_context(context: SequenceContext) -> SubstitutionLookup {
    SubstitutionLookup::Contextual(Lookup::new(
        LookupFlag::empty(),
        vec![SubstitutionSequenceContext::from(context)],
    ))
}

fn substitution_chain(context: ChainedSequenceContext) -> SubstitutionLookup {
    SubstitutionLookup::ChainContextual(Lookup::new(
        LookupFlag::empty(),
        vec![SubstitutionChainContext::from(context)],
    ))
}

fn anchor() -> AnchorTable {
    AnchorTable::format_1(0, 0)
}

/// How laying out each of `texts` in a font with `rules` went, after laying
/// out `texts` once, in the font's words shaped again as one: `None` if the
/// text was shaped whole. Each text shapes as it does whole, glyph for glyph.
fn joined(rules: Rules, texts: &[&str]) -> Vec<Option<usize>> {
    let mut cx = contexts_with(&[font(rules)]);
    // The same fonts, for the same font ids.
    let mut whole = LayoutContext::new();
    whole.set_shape_words(false);
    texts
        .iter()
        .map(|text| {
            let (words_before, joined_before) =
                (cx.1.words_reused_and_shaped(), cx.1.words_joined());
            let by_words = build(&mut cx.0, &mut cx.1, "Made", text, &[]);
            let by_whole = build(&mut cx.0, &mut whole, "Made", text, &[]);
            assert!(
                **by_words.shaped_text() == **by_whole.shaped_text(),
                "{text:?} shapes otherwise a word at a time"
            );
            (cx.1.words_reused_and_shaped() != words_before)
                .then(|| cx.1.words_joined() - joined_before)
        })
        .collect()
}

/// `joined` of texts that are shaped whole.
const WHOLE: [Option<usize>; 2] = [None, None];

#[test]
fn a_font_without_rules_for_a_space_shapes_words_on_their_own() {
    assert_eq!(
        joined(Rules::default(), &["aa bb aa", "a\u{301} \u{301}b"]),
        [Some(0), Some(0)]
    );
}

/// A font without an em isn't taken (its glyphs would be at NaN): text
/// asking for it falls back, as for a font that isn't there.
#[test]
fn a_font_without_an_em_is_not_taken() {
    for em in [Em::Zero, Em::Missing] {
        let rules = Rules {
            em,
            ..Default::default()
        };
        let (mut fonts, mut cx) = contexts_with(&[font(rules), AHEM.to_vec()]);
        let families: Vec<_> = fonts.collection.family_names().collect();
        assert_eq!(families, ["Ahem"]);
        let made = build(&mut fonts, &mut cx, "Made", "aa bb", &[]);
        let missing = build(&mut fonts, &mut cx, "Missing", "aa bb", &[]);
        assert!(**made.shaped_text() == **missing.shaped_text());
    }
    // Made with an em, it is taken.
    let (mut fonts, _) = contexts_with(&[font(Rules::default())]);
    assert_eq!(
        fonts.collection.family_names().collect::<Vec<_>>(),
        ["Made"]
    );
}

#[test]
fn a_kerning_pair_with_a_space_joins_the_word_beside_it() {
    let none = LookupFlag::empty();
    // `aT` and the space after it.
    let rules = Rules {
        gpos: vec![pair(g('T'), g(' '), none)],
        ..Default::default()
    };
    assert_eq!(joined(rules, &["aT bb aT", "aa bb"]), [Some(1), Some(0)]);
    // The space and `Tb` after it.
    let rules = Rules {
        gpos: vec![pair(g(' '), g('T'), none)],
        ..Default::default()
    };
    assert_eq!(joined(rules, &["aa Tb", "Ta bb"]), [Some(1), Some(0)]);
    // In the `kern` table.
    let rules = Rules {
        kern: vec![(g('T'), g(' '), -300)],
        ..Default::default()
    };
    assert_eq!(joined(rules, &["aT bb", "aa bb"]), [Some(1), Some(0)]);
    let rules = Rules {
        kern: vec![(g(' '), g('T'), -300)],
        ..Default::default()
    };
    assert_eq!(joined(rules, &["aa Tb", "Ta bb"]), [Some(1), Some(0)]);
}

#[test]
fn a_kerning_pair_of_classes_with_a_space_joins_the_word_beside_it() {
    // Class 1 of the first, `T`, and class 1 of the second, the space.
    let class_pair = |values: [i16; 2], first: ClassDef| {
        PositionLookup::Pair(Lookup::new(
            LookupFlag::empty(),
            vec![PairPos::format_2(
                coverage(&[g('T'), g('a')]),
                first,
                classes(&[(g(' '), 1)]),
                vec![
                    Class1Record::new(vec![
                        Class2Record::new(moved(0), ValueRecord::new()),
                        Class2Record::new(moved(0), ValueRecord::new()),
                    ]),
                    Class1Record::new(vec![
                        Class2Record::new(moved(values[0]), ValueRecord::new()),
                        Class2Record::new(moved(values[1]), ValueRecord::new()),
                    ]),
                ],
            )],
        ))
    };
    let rules = Rules {
        gpos: vec![class_pair([0, -300], classes(&[(g('T'), 1)]))],
        ..Default::default()
    };
    assert_eq!(joined(rules, &["aT bb", "aa bb"]), [Some(1), Some(0)]);
    // A pair that moves nothing doesn't count.
    let rules = Rules {
        gpos: vec![class_pair([0, 0], classes(&[(g('T'), 1)]))],
        ..Default::default()
    };
    assert_eq!(joined(rules, &["aT bb", "aa bb"]), [Some(0), Some(0)]);
    // Class 0 of the first is what the coverage has and the classes leave
    // out: `a`.
    let rules = Rules {
        gpos: vec![PositionLookup::Pair(Lookup::new(
            LookupFlag::empty(),
            vec![PairPos::format_2(
                coverage(&[g('T'), g('a')]),
                classes(&[(g('T'), 1)]),
                classes(&[(g(' '), 1)]),
                vec![
                    Class1Record::new(vec![
                        Class2Record::new(moved(0), ValueRecord::new()),
                        Class2Record::new(moved(-300), ValueRecord::new()),
                    ]),
                    Class1Record::new(vec![
                        Class2Record::new(moved(0), ValueRecord::new()),
                        Class2Record::new(moved(0), ValueRecord::new()),
                    ]),
                ],
            )],
        ))],
        ..Default::default()
    };
    assert_eq!(joined(rules, &["Ta bb", "aT bb"]), [Some(1), Some(0)]);
    // Class 0 of the second is every glyph but the classes': text in the
    // font is shaped whole.
    let rules = Rules {
        gpos: vec![PositionLookup::Pair(Lookup::new(
            LookupFlag::empty(),
            vec![PairPos::format_2(
                coverage(&[g(' ')]),
                classes(&[(g(' '), 1)]),
                classes(&[(g('T'), 1)]),
                vec![
                    Class1Record::new(vec![
                        Class2Record::new(moved(0), ValueRecord::new()),
                        Class2Record::new(moved(0), ValueRecord::new()),
                    ]),
                    Class1Record::new(vec![
                        Class2Record::new(moved(-300), ValueRecord::new()),
                        Class2Record::new(moved(0), ValueRecord::new()),
                    ]),
                ],
            )],
        ))],
        ..Default::default()
    };
    assert_eq!(joined(rules, &["aa bb", "aT bb"]), WHOLE);
}

#[test]
fn a_space_and_marks_take_their_neighbours_where_marks_are_skipped() {
    let skip = LookupFlag::IGNORE_MARKS;
    // A word ending in a mark: kerning `T` and the space skips it.
    let rules = Rules {
        gpos: vec![pair(g('T'), g(' '), skip)],
        marks: true,
        ..Default::default()
    };
    assert_eq!(
        joined(rules, &["aT\u{301} bb", "aa\u{301} bb"]),
        [Some(2), Some(2)]
    );
    // The `kern` table's pairs skip marks.
    let rules = Rules {
        kern: vec![(g('T'), g(' '), -300)],
        marks: true,
        ..Default::default()
    };
    assert_eq!(
        joined(rules, &["aT\u{301} bb", "aa bb"]),
        [Some(2), Some(0)]
    );
    // A mark a substitution makes of `T`, from one character: GDEF tells it.
    let rules = Rules {
        gsub: vec![SubstitutionLookup::Multiple(Lookup::new(
            LookupFlag::empty(),
            vec![MultipleSubstFormat1::new(
                coverage(&[g('T')]),
                vec![Sequence::new(vec![g('T'), g('\u{301}')])],
            )],
        ))],
        gpos: vec![pair(g('T'), g(' '), skip)],
        marks: true,
        ..Default::default()
    };
    assert_eq!(joined(rules, &["aT bb", "aa bb"]), [Some(1), Some(0)]);
    // Kerning a space with its own mark, which stays in its word.
    let rules = Rules {
        gpos: vec![pair(g(' '), g('\u{301}'), LookupFlag::empty())],
        marks: true,
        ..Default::default()
    };
    assert_eq!(
        joined(rules, &["aa \u{301}bb", "aa bb"]),
        [Some(0), Some(0)]
    );
    let rules = Rules {
        kern: vec![(g(' '), g('\u{301}'), -300)],
        marks: true,
        ..Default::default()
    };
    assert_eq!(
        joined(rules, &["aa \u{301}bb", "aa bb"]),
        [Some(0), Some(0)]
    );
}

#[test]
fn a_substitution_with_a_space_joins_the_words_on_both_sides() {
    let none = LookupFlag::empty();
    // `T` and the space become one glyph, which `b` after it kerns with.
    let rules = Rules {
        gsub: vec![ligature(&[g('T'), g(' ')], extra(0), none)],
        gpos: vec![pair(extra(0), g('b'), none)],
        ..Default::default()
    };
    assert_eq!(joined(rules, &["aT bb", "aa bb"]), [Some(2), Some(0)]);
    // The space and `T`, which `a` before it kerns with.
    let rules = Rules {
        gsub: vec![ligature(&[g(' '), g('T')], extra(0), none)],
        gpos: vec![pair(g('a'), extra(0), none)],
        ..Default::default()
    };
    assert_eq!(joined(rules, &["aa Tb", "aa bb"]), [Some(1), Some(0)]);
    // A space before `T` becomes `extra(0)`, which `a` kerns with.
    let mut rules = in_context(substitution_chain(ChainedSequenceContext::format_3(
        Vec::new(),
        vec![coverage(&[g(' ')])],
        vec![coverage(&[g('T')])],
        second_at(0),
    )));
    rules.gpos = vec![pair(g('a'), extra(0), none)];
    assert_eq!(joined(rules, &["aa Tb", "aa bb"]), [Some(1), Some(0)]);
}

#[test]
fn what_substitutions_make_of_the_glyph_beside_a_space_counts_too() {
    // A space after `a` becomes `extra(0)`, and then `a` becomes `extra(1)`.
    let rules = Rules {
        gsub: vec![
            substitution_chain(ChainedSequenceContext::format_3(
                vec![coverage(&[g('a')])],
                vec![coverage(&[g(' ')])],
                Vec::new(),
                second_at(0),
            )),
            single(g(' '), extra(0)),
            single(g('a'), extra(1)),
        ],
        ccmp: Some(vec![0, 2]),
        ..Default::default()
    };
    assert_eq!(joined(rules, &["a bb", "bb bb"]), [Some(2), Some(0)]);
    // A space before `b` becomes `extra(0)`, and then `b` becomes `extra(1)`.
    let rules = Rules {
        gsub: vec![
            substitution_chain(ChainedSequenceContext::format_3(
                Vec::new(),
                vec![coverage(&[g(' ')])],
                vec![coverage(&[g('b')])],
                second_at(0),
            )),
            single(g(' '), extra(0)),
            single(g('b'), extra(1)),
        ],
        ccmp: Some(vec![0, 2]),
        ..Default::default()
    };
    assert_eq!(joined(rules, &["aa b", "aa aa"]), [Some(1), Some(0)]);
}

#[test]
fn a_space_in_a_context_of_two_joins_the_words_on_both_sides() {
    // Each takes `T` and the space after it, and makes the space `extra(0)`.
    let space_at = second_at(1);
    for context in [
        substitution_context(SequenceContext::format_1(
            coverage(&[g('T')]),
            vec![Some(SequenceRuleSet::new(vec![SequenceRule::new(
                vec![g(' ')],
                space_at.clone(),
            )]))],
        )),
        substitution_context(SequenceContext::format_2(
            coverage(&[g('T')]),
            classes(&[(g('T'), 1), (g(' '), 2)]),
            vec![
                None,
                Some(ClassSequenceRuleSet::new(vec![ClassSequenceRule::new(
                    vec![2],
                    space_at.clone(),
                )])),
            ],
        )),
        substitution_context(SequenceContext::format_3(
            vec![coverage(&[g('T')]), coverage(&[g(' ')])],
            space_at.clone(),
        )),
        substitution_chain(ChainedSequenceContext::format_1(
            coverage(&[g(' ')]),
            vec![Some(ChainedSequenceRuleSet::new(vec![
                ChainedSequenceRule::new(vec![g('T')], Vec::new(), Vec::new(), second_at(0)),
            ]))],
        )),
        substitution_chain(ChainedSequenceContext::format_2(
            coverage(&[g(' ')]),
            classes(&[(g('T'), 1)]),
            classes(&[(g(' '), 1)]),
            ClassDef::from_iter([]),
            vec![
                None,
                Some(ChainedClassSequenceRuleSet::new(vec![
                    ChainedClassSequenceRule::new(vec![1], Vec::new(), Vec::new(), second_at(0)),
                ])),
            ],
        )),
        substitution_chain(ChainedSequenceContext::format_3(
            vec![coverage(&[g('T')])],
            vec![coverage(&[g(' ')])],
            Vec::new(),
            second_at(0),
        )),
    ]
    .into_iter()
    .enumerate()
    {
        let mut rules = in_context(context.1);
        rules.gpos = vec![pair(extra(0), g('b'), LookupFlag::empty())];
        assert_eq!(
            joined(rules, &["aT bb", "aa bb"]),
            [Some(2), Some(0)],
            "{}",
            context.0
        );
    }
    // A reverse chain applies itself, by its feature: what it makes of the
    // space is taken for a space, which `b` after any space kerns with.
    let mut rules = in_context(SubstitutionLookup::Reverse(Lookup::new(
        LookupFlag::empty(),
        vec![ReverseChainSingleSubstFormat1::new(
            coverage(&[g(' ')]),
            vec![coverage(&[g('T')])],
            Vec::new(),
            vec![extra(0)],
        )],
    )));
    rules.gpos = vec![pair(extra(0), g('b'), LookupFlag::empty())];
    assert_eq!(
        joined(rules, &["aT bb", "aa bb", "aa aa"]),
        [Some(2), Some(1), Some(0)]
    );
}

#[test]
fn a_space_in_a_longer_context_shapes_text_whole() {
    for context in [
        // `a T space`.
        substitution_chain(ChainedSequenceContext::format_3(
            vec![coverage(&[g('T')]), coverage(&[g('a')])],
            vec![coverage(&[g(' ')])],
            Vec::new(),
            second_at(0),
        )),
        // `space T b`: the lookahead counts.
        substitution_chain(ChainedSequenceContext::format_3(
            Vec::new(),
            vec![coverage(&[g(' ')])],
            vec![coverage(&[g('T')]), coverage(&[g('b')])],
            second_at(0),
        )),
        substitution_context(SequenceContext::format_3(
            vec![
                coverage(&[g('a')]),
                coverage(&[g('T')]),
                coverage(&[g(' ')]),
            ],
            second_at(2),
        )),
    ] {
        assert_eq!(joined(in_context(context), &["aT bb", "aa Tb"]), WHOLE);
    }
    // A ligature of three, which a context of two applies.
    let rules = Rules {
        gsub: vec![
            substitution_context(SequenceContext::format_3(
                vec![coverage(&[g('T')]), coverage(&[g(' ')])],
                second_at(1),
            )),
            ligature(&[g(' '), g('a'), g('b')], extra(0), LookupFlag::empty()),
        ],
        ccmp: Some(vec![0]),
        ..Default::default()
    };
    assert_eq!(joined(rules, &["aT bb", "aa bb"]), WHOLE);
    // A ligature of three.
    let rules = Rules {
        gsub: vec![ligature(
            &[g('a'), g(' '), g('b')],
            extra(0),
            LookupFlag::empty(),
        )],
        ..Default::default()
    };
    assert_eq!(joined(rules, &["aa bb", "Ta Tb"]), WHOLE);
}

#[test]
fn what_a_feature_makes_of_a_space_is_a_space() {
    // `ccmp` makes every space `extra(0)`, which kerns with `T`.
    let rules = Rules {
        gsub: vec![single(g(' '), extra(0))],
        gpos: vec![pair(extra(0), g('T'), LookupFlag::empty())],
        ..Default::default()
    };
    assert_eq!(joined(rules, &["aa Tb", "aa bb"]), [Some(1), Some(0)]);
}

#[test]
fn a_ligature_of_a_space_and_its_mark_joins_the_words_on_both_sides() {
    // A space and its mark become `extra(0)`, which `b` after it kerns with.
    let rules = Rules {
        gsub: vec![ligature(
            &[g(' '), g('\u{301}')],
            extra(0),
            LookupFlag::empty(),
        )],
        gpos: vec![pair(extra(0), g('b'), LookupFlag::empty())],
        marks: true,
        ..Default::default()
    };
    assert_eq!(
        joined(rules, &["aa \u{301}bb cc", "aa bb", " \u{301}bb cc"]),
        [Some(2), Some(0), Some(1)]
    );
}

#[test]
fn rules_reaching_further_shape_text_whole() {
    let rule_fonts = [
        // A lookup skipping base glyphs may skip a space.
        Rules {
            gsub: vec![ligature(
                &[g('a'), g('b')],
                extra(0),
                LookupFlag::IGNORE_BASE_GLYPHS,
            )],
            ..Default::default()
        },
        Rules {
            gpos: vec![pair(g('a'), g('b'), LookupFlag::IGNORE_BASE_GLYPHS)],
            ..Default::default()
        },
        // Cursive attachment of a space.
        Rules {
            gpos: vec![PositionLookup::Cursive(Lookup::new(
                LookupFlag::empty(),
                vec![CursivePosFormat1::new(
                    coverage(&[g(' ')]),
                    vec![EntryExitRecord::new(Some(anchor()), Some(anchor()))],
                )],
            ))],
            ..Default::default()
        },
        // A space as a mark.
        Rules {
            gpos: vec![PositionLookup::MarkToBase(Lookup::new(
                LookupFlag::empty(),
                vec![MarkBasePosFormat1::new(
                    coverage(&[g(' ')]),
                    coverage(&[g('a')]),
                    MarkArray::new(vec![MarkRecord::new(0, anchor())]),
                    BaseArray::new(vec![BaseRecord::new(vec![Some(anchor())])]),
                )],
            ))],
            ..Default::default()
        },
        Rules {
            gpos: vec![PositionLookup::MarkToLig(Lookup::new(
                LookupFlag::empty(),
                vec![MarkLigPosFormat1::new(
                    coverage(&[g(' ')]),
                    coverage(&[g('a')]),
                    MarkArray::new(vec![MarkRecord::new(0, anchor())]),
                    LigatureArray::new(vec![LigatureAttach::new(vec![ComponentRecord::new(
                        vec![Some(anchor())],
                    )])]),
                )],
            ))],
            ..Default::default()
        },
        Rules {
            gpos: vec![PositionLookup::MarkToMark(Lookup::new(
                LookupFlag::empty(),
                vec![MarkMarkPosFormat1::new(
                    coverage(&[g(' ')]),
                    coverage(&[g('\u{301}')]),
                    MarkArray::new(vec![MarkRecord::new(0, anchor())]),
                    Mark2Array::new(vec![Mark2Record::new(vec![Some(anchor())])]),
                )],
            ))],
            ..Default::default()
        },
        Rules {
            gpos: vec![PositionLookup::MarkToMark(Lookup::new(
                LookupFlag::empty(),
                vec![MarkMarkPosFormat1::new(
                    coverage(&[g('\u{301}')]),
                    coverage(&[g(' ')]),
                    MarkArray::new(vec![MarkRecord::new(0, anchor())]),
                    Mark2Array::new(vec![Mark2Record::new(vec![Some(anchor())])]),
                )],
            ))],
            ..Default::default()
        },
        // A positioning context of three.
        Rules {
            gpos: vec![
                PositionLookup::Contextual(Lookup::new(
                    LookupFlag::empty(),
                    vec![PositionSequenceContext::from(SequenceContext::format_3(
                        vec![
                            coverage(&[g('a')]),
                            coverage(&[g(' ')]),
                            coverage(&[g('b')]),
                        ],
                        second_at(1),
                    ))],
                )),
                pair(g(' '), g('b'), LookupFlag::empty()),
            ],
            ..Default::default()
        },
        Rules {
            gpos: vec![PositionLookup::ChainContextual(Lookup::new(
                LookupFlag::empty(),
                vec![PositionChainContext::from(
                    ChainedSequenceContext::format_3(
                        vec![coverage(&[g('a')])],
                        vec![coverage(&[g(' ')])],
                        vec![coverage(&[g('b')])],
                        Vec::new(),
                    ),
                )],
            ))],
            ..Default::default()
        },
        // A `kern` table of classes.
        Rules {
            kern_classes: true,
            ..Default::default()
        },
        // AAT, which harfrust shapes with.
        Rules {
            aat: true,
            ..Default::default()
        },
    ];
    for rules in rule_fonts {
        assert_eq!(joined(rules, &["aa bb", "aT Tb"]), WHOLE);
    }
}

#[test]
fn a_feature_off_unless_asked_for_is_not_looked_into() {
    let rules = Rules {
        gsub: vec![
            single(g('b'), g('b')),
            ligature(&[g('a'), g(' '), g('b')], extra(0), LookupFlag::empty()),
        ],
        ccmp: Some(vec![0]),
        dlig: vec![1],
        ..Default::default()
    };
    assert_eq!(joined(rules, &["aa bb", "bb aa"]), [Some(0), Some(0)]);
}

#[test]
fn a_font_with_many_glyphs_beside_a_space_shapes_text_whole() {
    let kerned = |count: u16| Rules {
        kern: (0..count)
            .map(|index| (extra(index), g(' '), -100))
            .collect(),
        ..Default::default()
    };
    assert_eq!(joined(kerned(256), &["aa bb"]), [Some(0)]);
    assert_eq!(joined(kerned(257), &["aa bb"]), [None]);
}
