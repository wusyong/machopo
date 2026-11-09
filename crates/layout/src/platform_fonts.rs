//! The fonts text was laid out in, for DevTools' `CSS` agent
//! (`getPlatformFontsForNode`; Blink's
//! `InspectorCSSAgent::CollectPlatformFontsForLayoutObject`, which counts
//! the glyphs of each `ShapeResult`'s runs by their font): each font's
//! family and PostScript name, read from its `name` table, and how many
//! glyphs of the runs asked about it drew.

use gfx::FontData;
use read_fonts::tables::name::{NameId, NameRecord};
use read_fonts::{FontRef, TableProvider};

use crate::{BoxFragment, TextFragment};

/// A font some text was laid out in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformFont {
    /// Its typographic family's name (the `name` table's 16, else 1), as
    /// the platform's font APIs give it (Chrome's `familyName`).
    pub family_name: String,
    pub postscript_name: String,
    pub glyph_count: usize,
}

/// The fonts of the runs in `layout_view`'s tree that `counts` picks, in
/// the order each first comes, with the glyphs of those runs each drew.
pub fn platform_fonts(
    layout_view: &BoxFragment,
    counts: &dyn Fn(&TextFragment) -> bool,
) -> Vec<PlatformFont> {
    let mut fonts: Vec<(FontData, usize)> = Vec::new();
    let mut fragments = vec![layout_view];
    while let Some(fragment) = fragments.pop() {
        for run in fragment
            .lines
            .iter()
            .flat_map(|line| &line.runs)
            .filter(|run| counts(run))
        {
            let mut glyphs = 0usize;
            run.for_each_glyph(usize::MAX, |_, _| glyphs = glyphs.saturating_add(1));
            match fonts.iter_mut().find(|(font, _)| *font == run.font) {
                Some((_, count)) => *count = count.saturating_add(glyphs),
                None => fonts.push((run.font.clone(), glyphs)),
            }
        }
        // In tree order: the last child is popped last.
        fragments.extend(fragment.children.iter().rev().map(|child| &*child.fragment));
    }
    fonts
        .into_iter()
        .map(|(font, glyph_count)| {
            let name = |ids: &[NameId]| font_name(&font, ids).unwrap_or_default();
            PlatformFont {
                family_name: name(&[NameId::TYPOGRAPHIC_FAMILY_NAME, NameId::FAMILY_NAME]),
                postscript_name: name(&[NameId::POSTSCRIPT_NAME]),
                glyph_count,
            }
        })
        .collect()
}

/// The first of `ids` `font`'s `name` table has: its Unicode record in
/// English, else its Macintosh one in English (some of macOS's own fonts,
/// Times's, have only those), else its first.
fn font_name(font: &FontData, ids: &[NameId]) -> Option<String> {
    let face = FontRef::from_index((*font.bytes).as_ref(), font.index).ok()?;
    let table = face.name().ok()?;
    let data = table.string_data();
    ids.iter().find_map(|&id| {
        let records: Vec<&NameRecord> = table
            .name_record()
            .iter()
            .filter(|record| record.name_id() == id)
            .collect();
        let record = records
            .iter()
            .find(|record| unicode_english(record))
            .or_else(|| records.iter().find(|record| mac_english(record)))
            .or_else(|| records.first())?;
        let text: String = record.string(data).ok()?.chars().collect();
        (!text.is_empty()).then_some(text)
    })
}

/// An English record of a Unicode encoding: Windows' 0x409, or the Unicode
/// platform's (which has no languages).
fn unicode_english(record: &NameRecord) -> bool {
    record.is_unicode() && (record.platform_id() == 0 || record.language_id() == 0x409)
}

/// A Macintosh record in English (its language 0).
fn mac_english(record: &NameRecord) -> bool {
    record.platform_id() == 1 && record.language_id() == 0
}
