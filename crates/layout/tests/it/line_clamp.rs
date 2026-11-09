//! Line clamp: a `-webkit-box` with a vertical `-webkit-line-clamp` ends
//! with its clamped line, which ends with an ellipsis, and hides what
//! comes after it.

use layout::BoxFragment;

use crate::page::lay_out;

/// Each line of `fragment`: whether it is hidden, and where each run
/// starts (the ellipsis is a run of its own).
fn lines(fragment: &BoxFragment) -> Vec<(bool, Vec<f32>)> {
    fragment
        .lines
        .iter()
        .map(|line| (line.hidden, line.runs.iter().map(|r| r.x).collect()))
        .collect()
}

const CLAMP: &str = "<style>body { margin: 0; font: 20px/1 Ahem }
    .clamp { display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 2;
      width: 60px }</style>";

/// The second of four lines ends with the ellipsis, after its text, though
/// it doesn't overflow; the lines after it are hidden; the box is as tall
/// as two lines, and its padding and border. A height of its own stays.
/// Fewer lines are as they are.
#[test]
fn ends_with_the_clamped_line() {
    let laid = lay_out(&format!(
        "{CLAMP}<div id=a class=clamp>XX XX XX XX</div>
         <div id=b class=clamp style='padding: 5px; border: 3px solid'>XX XX XX XX</div>
         <div id=c class=clamp style='height: 70px'>XX XX XX XX</div>
         <div id=d class=clamp style='-webkit-line-clamp: 5'>XX XX XX XX</div>
         <div id=e class=clamp style='min-height: 50px'>XX XX XX XX</div>
         <div id=f class=clamp style='max-height: 30px'>XX XX XX XX</div>
         <div id=g class=clamp>XXXX XX XX</div>"
    ));
    assert_eq!(laid.rect("a").height, 40.0);
    assert_eq!(
        lines(laid.fragment("a")),
        [
            (false, vec![0.0]),
            (false, vec![0.0, 40.0]),
            (true, vec![0.0]),
            (true, vec![0.0]),
        ]
    );
    assert_eq!(laid.rect("b").height, 56.0);
    assert_eq!(laid.rect("c").height, 70.0);
    assert!(laid.fragment("c").lines[2].hidden);
    assert_eq!(laid.rect("d").height, 80.0);
    assert!(
        lines(laid.fragment("d"))
            .iter()
            .all(|(hidden, runs)| !hidden && runs.len() == 1)
    );
    // Its `min-height` and `max-height` still hold.
    assert_eq!(laid.rect("e").height, 50.0);
    assert_eq!(laid.rect("f").height, 30.0);
    // A first line that overflows isn't truncated: only the clamped line
    // ends with an ellipsis.
    assert_eq!(
        lines(laid.fragment("g"))[..2],
        [(false, vec![0.0]), (false, vec![0.0, 40.0])]
    );
    assert_eq!(laid.fragment("g").lines[0].runs[0].hidden_from, None);
}

/// Lines are counted through the blocks in the box's block formatting
/// context, in order: the clamped one is in a child, and the blocks after
/// it are hidden. A block establishing its own isn't counted into, nor
/// hidden before the clamped line. A measured clamp container (a flex
/// item) is as tall too.
#[test]
fn counts_the_lines_of_its_block_formatting_context() {
    let laid = lay_out(&format!(
        "{CLAMP}<div id=a class=clamp style='-webkit-line-clamp: 3'><div>X<br>X</div><div id=b>X<br>X</div><div id=c>X</div></div>
         <div id=d class=clamp><div id=bfc style='overflow: hidden'>X<br>X<br>X</div><div style='display: flow-root'>X</div><div id=e>X<br>X<br>X</div></div>
         <div style='display: flex; align-items: start'><div id=f class=clamp>XX XX XX</div></div>"
    ));
    assert_eq!(laid.rect("a").height, 60.0);
    assert_eq!(
        lines(laid.fragment("b")),
        [(false, vec![0.0, 20.0]), (true, vec![0.0])]
    );
    assert!(laid.fragment("c").hidden);
    assert!(!laid.fragment("b").hidden);
    // The `overflow: hidden` block's three lines aren't counted, nor the
    // `flow-root`'s one.
    assert_eq!(laid.rect("d").height, 120.0);
    assert!(!laid.fragment("bfc").hidden);
    assert!(laid.fragment("bfc").lines.iter().all(|line| !line.hidden));
    assert_eq!(
        lines(laid.fragment("e")),
        [
            (false, vec![0.0]),
            (false, vec![0.0, 20.0]),
            (true, vec![0.0])
        ]
    );
    assert_eq!(laid.rect("f").height, 40.0);
}

/// A `-webkit-inline-box` clamps as an inline-block.
#[test]
fn an_inline_box_clamps_too() {
    let laid = lay_out(&format!(
        "{CLAMP}<div>A<span id=a class=clamp style='display: -webkit-inline-box'>XX XX XX</span>
           <span id=b class=clamp style='display: -webkit-inline-box; height: 70px'>XX XX XX</span></div>"
    ));
    assert_eq!(laid.rect("a").height, 40.0);
    // A height of its own stays.
    assert_eq!(laid.rect("b").height, 70.0);
    assert!(laid.fragment("a").lines[2].hidden);
}

/// With nothing after the clamped line the clamp does nothing: no
/// ellipsis. A blank clamped line has the ellipsis where it starts.
#[test]
fn ends_with_an_ellipsis_only_before_more() {
    let laid = lay_out(&format!(
        "{CLAMP}<div id=a class=clamp>XX XX</div>
         <div id=b class=clamp style='white-space: pre'>XX\n\nXX</div>"
    ));
    assert_eq!(
        lines(laid.fragment("a")),
        [(false, vec![0.0]), (false, vec![0.0])]
    );
    assert_eq!(
        lines(laid.fragment("b")),
        [(false, vec![0.0]), (false, vec![0.0]), (true, vec![0.0])]
    );
}

/// The blocks around the clamped line end with it too, and their padding
/// and border: the clamp container's height is theirs and its own.
#[test]
fn the_blocks_around_the_clamped_line_end_with_it() {
    let laid = lay_out(&format!(
        "{CLAMP}<div id=a class=clamp style='padding: 1px'>X<div id=b style='padding: 2px; border: 3px solid'>X<br>X</div>X</div>"
    ));
    // Line 1 at 1, `b` at 21, its line at 26 to 46, and its padding and
    // border to 51; the container's padding to 52.
    assert_eq!(laid.rect("b").y, 21.0);
    assert_eq!(laid.rect("b").height, 30.0);
    assert_eq!(laid.rect("a").height, 52.0);
}

/// The ellipsis is placed before the line is aligned: a right-aligned line
/// moves left by its width, as far as there is room.
#[test]
fn an_aligned_line_makes_room_for_the_ellipsis() {
    let laid = lay_out(&format!(
        "{CLAMP}<div id=a class=clamp style='width: 140px; text-align: right'>XX XX XX XX XX</div>
         <div id=b class=clamp style='width: 140px; text-align: center'>XX XX XX XX XX</div>"
    ));
    // `XX XX` 40px from the right, then 20px, and the ellipsis.
    assert_eq!(lines(laid.fragment("a"))[1], (false, vec![20.0, 120.0]));
    // 20px from either side: half the ellipsis less on the left.
    assert_eq!(lines(laid.fragment("b"))[1], (false, vec![10.0, 110.0]));
}
