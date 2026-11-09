//! What shaping a text gave is kept, for a layout with the same text, styled
//! the same in what changes shaping, with its inline elements' edges in
//! the same places. (In a debug build every kept shaping is also shaped
//! again and compared.)

use layout::TextContext;

/// How many layouts of `html`'s blocks took a kept shaping, building its
/// box tree once.
fn reused(html: &str) -> usize {
    let mut text = TextContext::new(true);
    build(html, &mut text);
    text.shapings_reused_for_testing()
}

/// Builds `html`'s box tree with `text`.
pub(crate) fn build(html: &str, text: &mut TextContext) {
    crate::page::build(html, text);
}

#[test]
fn alike_text_takes_the_shaping_of_the_first() {
    let style = "<style>p { font: 20px/1 Lato }</style>";
    assert_eq!(
        reused(&format!("{style}<p>Some words</p><p>Some words</p>")),
        1
    );
    // Colors don't change shaping; a line height doesn't either.
    assert_eq!(
        reused(&format!(
            "{style}<p>Some words</p><p style='color: red; line-height: 3'>Some words</p>"
        )),
        1
    );
    // Other text, another size, a bolder weight, other letter spacing:
    // shaped again.
    assert_eq!(
        reused(&format!("{style}<p>Some words</p><p>Other words</p>")),
        0
    );
    assert_eq!(
        reused(&format!(
            "{style}<p>Some words</p><p style='font-size: 30px'>Some words</p>"
        )),
        0
    );
    assert_eq!(
        reused(&format!(
            "{style}<p>Some words</p><p style='font-weight: bold'>Some words</p>"
        )),
        0
    );
    assert_eq!(
        reused(&format!(
            "{style}<p>Some words</p><p style='letter-spacing: 2px'>Some words</p>"
        )),
        0
    );
    // An inline element's edges split the shaping: in the same places, it
    // is taken, with each character's own style; elsewhere, not.
    assert_eq!(
        reused(&format!(
            "{style}<p>Some <b style='font-weight: normal'>wor</b>ds</p><p>Some <i style='font-style: normal; color: red'>wor</i>ds</p>"
        )),
        1
    );
    assert_eq!(
        reused(&format!(
            "{style}<p>Some words</p><p>Some <span>wor</span>ds</p>"
        )),
        0
    );
    // Text styled otherwise in parts, with no edges between: each part
    // takes its own style.
    assert_eq!(
        reused(&format!(
            "{style}<p>Some words</p><p><span style='display: contents; color: red'>Some</span> words</p>"
        )),
        1
    );
}

/// Adding fonts forgets the shapings kept, and the words: font selection
/// may pick the new fonts.
#[test]
fn adding_fonts_forgets_what_shaping_gave() {
    let mut text = TextContext::new(true);
    let html = "<p style='font: 20px/1 Ahem'>aa aa</p>";
    build(html, &mut text);
    assert_eq!(text.words_reused_and_shaped_for_testing(), (1, 2));
    text.register_fonts_for_testing(
        include_bytes!("../../../../resources/test_fonts/Lato-Medium.ttf").to_vec(),
    );
    build(html, &mut text);
    assert_eq!(text.shapings_reused_for_testing(), 0);
    assert_eq!(text.words_reused_and_shaped_for_testing(), (2, 4));
}
