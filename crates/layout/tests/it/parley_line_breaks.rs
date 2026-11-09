//! Our change to parley (the `machopo` branch of our fork; see its
//! README.machopo.md) that looks line break opportunities up in a table for
//! characters up to U+017F and from U+2000 to U+203F (Latin-1, Latin
//! Extended-A and the commonest punctuation), under the default rules,
//! giving ICU4X only the
//! words the table can't decide (with a character outside it, or beside a
//! quotation mark whose opportunities depend on more than its neighbour),
//! and that can leave word boundaries out. In a debug build every text
//! looked up is also given to ICU4X whole and compared.

use layout::TextContext;
use parley::Layout;

use crate::parley_words::{build, contexts_with};

static AHEM: &[u8] = include_bytes!("../../../../resources/test_fonts/Ahem.ttf");

/// How many texts building `html`'s box tree looked up in the table, and
/// how many words it gave to ICU4X.
fn looked_up(html: &str) -> (usize, usize) {
    let mut text = TextContext::new(true);
    crate::shaping::build(html, &mut text);
    text.line_breaks_looked_up_for_testing()
}

#[test]
fn text_in_the_table_is_looked_up() {
    assert_eq!(
        looked_up("<p>Le chat dort sur le canapé, près de la fenêtre (à 5 h).</p>"),
        (1, 0)
    );
    assert_eq!(
        looked_up("<p>L’enfant — son cœur… à Łódź, 5 ‰, un čaj, note†1.</p>"),
        (1, 0)
    );
    // The euro sign is outside the table; the word after it is looked up,
    // but not when the sign ends the word before.
    assert_eq!(looked_up("<p>Il coûte 5 €. Bien.</p>"), (1, 1));
    assert_eq!(looked_up("<p>Il coûte 5 €… Bien.</p>"), (1, 1));
    assert_eq!(looked_up("<p>€ 5</p>"), (1, 2));
    // An opening quotation mark keeps the word after it on its line
    // depending on what is before the mark; a closing one, on what is after.
    assert_eq!(looked_up("<p>Il dit « oui ».</p>"), (1, 2));
    assert_eq!(looked_up("<p>Il dit «\u{a0}oui\u{a0}».</p>"), (1, 0));
}

#[test]
fn text_with_other_rules_is_given_to_icu4x_whole() {
    assert_eq!(
        looked_up("<p style='word-break: break-all'>Le chat dort.</p>"),
        (0, 0)
    );
    assert_eq!(
        looked_up("<p>Le <span style='word-break: break-all'>chat</span> dort.</p>"),
        (0, 0)
    );
}

/// The clusters marked as word boundaries in `layout`, and its lines'
/// ranges when they break at every opportunity.
fn boundaries_and_lines(layout: &mut Layout<u32>) -> (Vec<usize>, Vec<std::ops::Range<usize>>) {
    layout.break_all_lines(Some(0.0));
    let boundaries = layout
        .lines()
        .flat_map(|line| line.runs().collect::<Vec<_>>())
        .flat_map(|run| run.clusters().collect::<Vec<_>>())
        .filter(|cluster| cluster.is_word_boundary())
        .map(|cluster| cluster.text_range().start)
        .collect();
    let lines = layout.lines().map(|line| line.text_range()).collect();
    (boundaries, lines)
}

#[test]
fn word_boundaries_are_left_out_when_turned_off() {
    let (mut fonts, mut cx) = contexts_with(&[AHEM.to_vec()]);
    // Thai has no dictionary here: every grapheme starts a word.
    let text = "Le chat (gris) dort. L’enfant aussi. ภาษาไทย";
    let (boundaries, lines) =
        boundaries_and_lines(&mut build(&mut fonts, &mut cx, "Ahem", text, &[]));
    assert!(!boundaries.is_empty());
    cx.set_word_boundaries(false);
    let (no_boundaries, same_lines) =
        boundaries_and_lines(&mut build(&mut fonts, &mut cx, "Ahem", text, &[]));
    assert_eq!(no_boundaries, Vec::<usize>::new());
    // Line break opportunities are where they were.
    assert_eq!(same_lines, lines);
    assert!(lines.len() > 1);
}
