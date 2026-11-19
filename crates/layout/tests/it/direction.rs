//! `direction: rtl` in layout: a paragraph's direction is its block's
//! (not its first strong character's), lines start on the right; flex
//! items and table columns go from the right; out-of-flow boxes' static
//! positions are on the right.

use crate::page::lay_out;

const AHEM: &str = "<style>body { margin: 0; font: 20px/1 Ahem } table { border-spacing: 0 }
    td { padding: 0 }</style>";

/// An `rtl` block's lines start on the right; an `ltr` one's on the
/// left, whatever its text's first strong character.
#[test]
fn a_paragraphs_direction_is_its_blocks() {
    let laid = lay_out(&format!(
        "{AHEM}<div id=rtl style='width: 200px; direction: rtl'>XX</div>
         <div id=ltr style='width: 200px'>&#x5d0;&#x5d0;</div>"
    ));
    assert_eq!(laid.lines("rtl")[0].x, 160.0);
    assert_eq!(laid.lines("ltr")[0].x, 0.0);
}

/// Flex items and table columns go from the right.
#[test]
fn items_and_columns_go_from_the_right() {
    let laid = lay_out(&format!(
        "{AHEM}<div style='display: flex; width: 200px; direction: rtl'>
           <div id=f1 style='width: 50px; height: 10px'></div>
           <div id=f2 style='width: 50px; height: 10px'></div></div>
         <table style='direction: rtl'><tr><td id=c1>X</td><td id=c2>XX</td></tr></table>"
    ));
    assert_eq!(laid.rect("f1").x, 150.0);
    assert_eq!(laid.rect("f2").x, 100.0);
    assert_eq!(laid.rect("c1").x, 40.0);
    assert_eq!(laid.rect("c2").x, 0.0);
}

/// An out-of-flow box's static position is at its container's
/// inline-start, the right: one block-level in inline content, below its
/// line; a table's own.
#[test]
fn static_positions_are_on_the_right() {
    let laid = lay_out(&format!(
        "{AHEM}<div style='position: relative; width: 200px; direction: rtl'>XX<div id=inline
           style='position: absolute; width: 30px; height: 10px'></div></div>
         <div style='position: relative'><div style='display: table; direction: rtl;
           width: 100px'><div id=table style='position: absolute; width: 10px;
           height: 10px'></div><div style='display: table-cell'>X</div></div></div>"
    ));
    assert_eq!(laid.rect("inline").x, 170.0);
    assert_eq!(laid.rect("table").x, 90.0);
}

const BOXES: &str = "<style>body { margin: 0; font: 20px/1 Ahem }
    i { display: inline-block; width: 10px; height: 10px }</style>";

/// An element's `unicode-bidi` and `direction` reorder what it holds:
/// an embedding, an isolate and an override (inline, or a block's over its
/// lines), and `plaintext` an isolate of its first strong character's
/// direction, as the bidi controls they are; `normal` adds nothing, and
/// the neutral inline-blocks take the text's direction around them.
#[test]
fn unicode_bidi_reorders_by_the_element() {
    let laid = lay_out(&format!(
        "{BOXES}<div>X<span style='direction: rtl; unicode-bidi: embed'><i id=e1></i><i
           id=e2></i></span>X</div>
         <div>X<span style='direction: rtl; unicode-bidi: isolate'><i id=i1></i><i
           id=i2></i></span>X</div>
         <div>X<span style='direction: rtl; unicode-bidi: bidi-override'><i id=o1></i><i
           id=o2></i></span>X</div>
         <div>X<span style='direction: rtl; unicode-bidi: isolate-override'><i
           id=io1></i><i id=io2></i></span>X</div>
         <div>X<span style='direction: rtl'><i id=n1></i><i id=n2></i></span>X</div>
         <div>X<span style='unicode-bidi: plaintext'>&#x200f;<i id=p1></i><i
           id=p2></i></span>X</div>
         <div>X<span style='direction: rtl; unicode-bidi: bidi-override'>X<i id=ox1></i>X<i
           id=ox2></i>X</span></div>
         <div>X<span style='direction: rtl; unicode-bidi: isolate-override'>X<i
           id=iox1></i>X<i id=iox2></i>X</span></div>
         <div style='unicode-bidi: bidi-override'>&#x200f;<i id=b1></i><i
           id=b2></i>&#x200f;</div>
         <div>&#x200f;<i id=r1></i><i id=r2></i>&#x200f;</div>"
    ));
    let x = |id: &str| laid.rect(id).x;
    for (first, second) in [
        ("e1", "e2"),
        ("i1", "i2"),
        ("o1", "o2"),
        ("io1", "io2"),
        ("p1", "p2"),
    ] {
        assert_eq!((x(first), x(second)), (30.0, 20.0), "{first} reversed");
    }
    assert_eq!((x("n1"), x("n2")), (20.0, 30.0));
    // Left-to-right text overridden too, where an embedding keeps it.
    assert_eq!((x("ox1"), x("ox2")), (70.0, 40.0));
    assert_eq!((x("iox1"), x("iox2")), (70.0, 40.0));
    assert_eq!((x("b1"), x("b2")), (0.0, 10.0), "overridden left to right");
    assert_eq!((x("r1"), x("r2")), (10.0, 0.0));
}

/// A block's `unicode-bidi: plaintext` takes its direction from its first
/// strong character: its line starts on the right.
#[test]
fn plaintext_takes_its_first_strong_characters() {
    let laid = lay_out(&format!(
        "{BOXES}<div id=plain style='width: 200px; unicode-bidi: plaintext'>&#x200f;XX</div>"
    ));
    assert_eq!(laid.lines("plain")[0].x, 160.0);
}

/// An inline box's left edge (margin, border and padding) is before its
/// content and its right edge after it, whatever bidi does with them: an
/// `rtl` element with left-to-right content it doesn't embed, one that
/// embeds its content (its start edge after the embedding's initiator),
/// and an empty one. An inline-level out-of-flow box's static position is
/// where it goes among them.
#[test]
fn edges_go_on_their_sides() {
    let laid = lay_out(&format!(
        "{BOXES}<div>X<span style='direction: rtl; margin: 0 3px 0 10px; padding-right: 5px'><i
           id=normal></i></span><i id=after_normal></i></div>
         <div>X<span style='direction: rtl; unicode-bidi: embed; padding: 0 4px 0 10px'><i
           id=embed1></i><i id=embed2></i></span><i id=after_embed></i></div>
         <div>X<span style='direction: rtl; padding: 0 5px 0 7px'></span><i
           id=after_empty></i></div>
         <div>X<span style='direction: rtl; padding-left: 10px'><b id=abs
           style='position: absolute; width: 10px; height: 10px'></b>X</span></div>"
    ));
    let x = |id: &str| laid.rect(id).x;
    assert_eq!((x("normal"), x("after_normal")), (30.0, 48.0));
    assert_eq!(
        (x("embed2"), x("embed1"), x("after_embed")),
        (30.0, 40.0, 54.0)
    );
    assert_eq!(x("after_empty"), 32.0);
    assert_eq!(x("abs"), 30.0);
}

/// An `rtl` box wrapped over lines has its start edge (its right) on its
/// first line and its end edge (its left) on its last, whatever the
/// direction of its content (CSS 2 § 8.6); the white space ending its
/// first line is removed, before the edge.
#[test]
fn a_wrapped_rtl_box_starts_on_its_right() {
    let laid = lay_out(&format!(
        "{BOXES}<div id=wrap style='width: 60px'><span style='direction: rtl; border-style: solid;
           border-width: 0 5px 0 3px'>XX XX</span><i id=after></i></div>"
    ));
    let boxes = crate::inline::inline_boxes(&laid.view);
    let sides: Vec<Vec<(f32, f32, bool, bool)>> = boxes
        .iter()
        .map(|line| line.iter().map(|b| (b.0.x, b.0.width, b.1, b.2)).collect())
        .collect();
    assert_eq!(
        sides,
        [
            vec![(0.0, 45.0, false, true)],
            vec![(0.0, 43.0, true, false)]
        ]
    );
    assert_eq!(laid.rect("after").x, 43.0, "after the box's left edge only");
    let first = &laid.fragment("wrap").lines[0];
    assert_eq!((first.runs[0].x, first.runs[0].advance), (0.0, 40.0));
}

/// The bidi controls an element adds are not content: an element holding
/// nothing (but an out-of-flow box) leaves its line empty, with no height,
/// and one ending with a forced break makes no line after it.
#[test]
fn bidi_controls_are_not_content() {
    let laid = lay_out(&format!(
        "{BOXES}<div id=empty><span dir=rtl><b style='position: absolute'></b></span></div>
         <div id=ends><span dir=rtl>X<br></span></div>"
    ));
    assert_eq!(laid.rect("empty").height, 0.0);
    assert_eq!(laid.rect("ends").height, 20.0);
}

/// A forced break ends the embeddings around it, and they go on after it:
/// what follows is still in them. An override of the block's too, and a
/// preserved newline's inside a run of text. They open again outermost
/// first: an `ltr` isolate in an `rtl` one is still `ltr`.
#[test]
fn embeddings_go_on_past_a_forced_break() {
    let laid = lay_out(&format!(
        "{BOXES}<div><span dir=rtl>X<br><i id=a1></i><i id=a2></i></span></div>
         <div style='unicode-bidi: bidi-override'>&#x200f;<br>&#x200f;<i id=b1></i><i
           id=b2></i>&#x200f;</div>
         <div style='white-space: pre'><span dir=rtl>X\nX<i id=c1></i><i
           id=c2></i></span></div>
         <div><span dir=rtl><span dir=ltr>X<br><i id=d1></i><i
           id=d2></i></span></span></div>"
    ));
    let x = |id: &str| laid.rect(id).x;
    assert_eq!((x("a1"), x("a2")), (10.0, 0.0));
    assert_eq!((x("c1"), x("c2")), (10.0, 0.0));
    assert_eq!((x("d1"), x("d2")), (0.0, 10.0));
    assert_eq!((x("b1"), x("b2")), (0.0, 10.0));
}
