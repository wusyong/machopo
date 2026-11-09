//! Atomic inlines: `inline-block` boxes on lines, sized to fit their
//! content, on the baseline, and placed by the lines they are on.

use gfx::{RectF, SizeF};
use layout::AtomicInline;

use crate::page::{Laid, lay_out};

impl Laid {
    /// The baselines of the runs on `id`'s lines, in LayoutView
    /// coordinates, line by line.
    fn baselines(&self, id: &str) -> Vec<Vec<f32>> {
        let origin = self.origin(id);
        self.fragment(id)
            .lines
            .iter()
            .map(|line| line.runs.iter().map(|r| origin.y + r.baseline).collect())
            .collect()
    }
}

const AHEM: &str = "<style>body { margin: 0 } div { font: 20px/1 Ahem }
    span { display: inline-block }</style>";

#[test]
fn sized_to_fit_their_content() {
    let laid = lay_out(&format!(
        "{AHEM}<div>A<span id=a>BB</span>C<span id=b style='width: 60px'>D</span>E<span id=c
           style='padding: 4px; border: 2px solid; margin: 0 5px'>F</span></div>
         <div style='width: 100px'><span id=wraps>XX XX XX</span></div>
         <div style='width: 10px'><span id=narrow>XX XX</span></div>
         <div style='width: 200px'><span id=half style='width: 50%'>X</span></div>
         <div style='width: 100px'><span id=text-margins style='margin: 0 10px'>XX XX XX</span></div>
         <div style='width: 100px'><span id=block-margins style='margin: 0 10px'><div>XX XX XX</div></span></div>
         <div><span id=fits><span style='margin: 0 10px'>XX</span></span></div>"
    ));
    // As wide as its text, on the line after `A`; 6px down, as `#c`'s
    // padding and border above its baseline make the line taller.
    assert_eq!(laid.rect("a"), RectF::new(20.0, 6.0, 40.0, 20.0));
    assert_eq!(laid.rect("b"), RectF::new(80.0, 6.0, 60.0, 20.0));
    // Its border box after its left margin.
    assert_eq!(laid.rect("c"), RectF::new(165.0, 0.0, 32.0, 32.0));
    // No wider than the room: its text wraps inside it, `XX XX` filling
    // the first line.
    assert_eq!(laid.rect("wraps").size(), SizeF::new(100.0, 40.0));
    // No narrower than its longest word.
    assert_eq!(laid.rect("narrow").size(), SizeF::new(40.0, 40.0));
    // A percentage of the line's containing block.
    assert_eq!(laid.rect("half").width, 100.0);
    // In the room its margins leave: 80px, a line for each `XX`.
    assert_eq!(laid.rect("text-margins").size(), SizeF::new(80.0, 60.0));
    assert_eq!(laid.rect("block-margins").size(), SizeF::new(80.0, 60.0));
    // What it adds to the max-content width of an inline-block around it:
    // its own, and its margins.
    assert_eq!(laid.rect("fits").width, 60.0);
}

/// An intrinsic size keyword for its `width` sizes it from its content,
/// whatever the room (Blink's `ComputeInlineSizeFromKeyword`): `min-content`
/// as its longest word, `max-content` as its line unbroken, `fit-content`
/// as `auto`. Its min-content and max-content contributions are that size
/// too, and a float on a line is sized the same way.
#[test]
fn intrinsic_size_keywords() {
    let laid = lay_out(&format!(
        "{AHEM}<div style='width: 300px'><span id=min style='width: min-content'>XX XXX</span></div>
         <div style='width: 300px'><span id=min-block style='width: min-content'><div>XX XXX</div></span></div>
         <div style='width: 50px'><span id=max style='width: max-content'>XX XXX</span></div>
         <div style='width: 100px'><span id=fit style='width: fit-content'>XX XXX X</span></div>
         <div><span id=around><span style='width: min-content'>XX XXX</span></span></div>
         <div style='width: 300px'>A<span id=float style='float: left; width: min-content'>XX XXX</span></div>"
    ));
    assert_eq!(laid.rect("min").size(), SizeF::new(60.0, 40.0));
    assert_eq!(laid.rect("min-block").size(), SizeF::new(60.0, 40.0));
    // Wider than the room.
    assert_eq!(laid.rect("max").size(), SizeF::new(120.0, 20.0));
    // The room, between its min-content and max-content widths.
    assert_eq!(laid.rect("fit").size(), SizeF::new(100.0, 40.0));
    // The max-content width of the inline-block around it is its
    // min-content width.
    assert_eq!(laid.rect("around").width, 60.0);
    assert_eq!(laid.rect("float").size(), SizeF::new(60.0, 40.0));
}

#[test]
fn on_the_baseline() {
    let laid = lay_out(&format!(
        "{AHEM}<div id=lines>X<span id=two>A<br>B</span>Y</div>
         <div id=clips>X<span id=clipped style='overflow: hidden'>A<br>B</span>Y</div>
         <div id=clip>X<span id=clip-only style='overflow: clip'>A<br>B</span>Y</div>
         <div id=empty>X<span id=nothing style='height: 30px'></span>Y</div>
         <div id=blocks>X<span id=nested><div>A</div><div>B</div></span>Y</div>
         <div id=margin>X<span id=below style='margin-top: 10px'>A</span>Y</div>
         <div id=out-of-flow>X<span style='position: relative'><div>A</div><div style='position: absolute; top: 50px'>B</div></span>Y</div>
         <div id=only-oof>X<span id=oof style='height: 30px; position: relative'><div style='position: absolute'>B</div></span>Y</div>
         <div id=oof-after>X<span style='position: relative'>A<br><b style='position: absolute'>B</b></span>Y</div>
         <div id=break>X<span><br></span>Y</div>
         <div id=nested-atomic>X<span><span>A</span></span>Y</div>"
    ));
    // Its last line's baseline on the line's: 36px down, and the line as
    // tall as it.
    let top = laid.rect("lines").y;
    assert_eq!(laid.rect("two"), RectF::new(20.0, top, 20.0, 40.0));
    assert_eq!(laid.baselines("lines")[0], [top + 36.0, top + 36.0]);
    // A scroll container, or one with no line: its bottom margin edge.
    let top = laid.rect("clips").y;
    assert_eq!(laid.rect("clipped").y, top);
    assert_eq!(laid.baselines("clips")[0], [top + 40.0, top + 40.0]);
    // `overflow: clip` isn't a scroll container: its last line's, as
    // with `visible` (Blink's `IsOverflowVisibleOrClip`).
    let top = laid.rect("clip").y;
    assert_eq!(laid.baselines("clip")[0], [top + 36.0, top + 36.0]);
    let top = laid.rect("empty").y;
    assert_eq!(laid.baselines("empty")[0], [top + 30.0, top + 30.0]);
    // The last line of its last block.
    let top = laid.rect("blocks").y;
    assert_eq!(laid.rect("nested").height, 40.0);
    assert_eq!(laid.baselines("blocks")[0], [top + 36.0, top + 36.0]);
    // Its margin box on the line: the baseline below its top margin.
    let top = laid.rect("margin").y;
    assert_eq!(laid.rect("below").y, top + 10.0);
    assert_eq!(laid.baselines("margin")[0], [top + 26.0, top + 26.0]);
    // An out-of-flow last child has no say, even laid out already (it is
    // the containing block): the last in-flow one's.
    let top = laid.rect("out-of-flow").y;
    assert_eq!(laid.baselines("out-of-flow")[0], [top + 16.0, top + 16.0]);
    // A line with only out-of-flow boxes is empty: it gives no baseline
    // (Blink's empty line box), so one with nothing else has none, its
    // bottom margin edge on the line's; one after the last line of text
    // leaves that line's. A forced break alone isn't empty.
    let top = laid.rect("only-oof").y;
    assert_eq!(laid.rect("oof").y, top);
    assert_eq!(laid.baselines("only-oof")[0], [top + 30.0, top + 30.0]);
    let top = laid.rect("oof-after").y;
    assert_eq!(laid.rect("oof-after").height, 20.0);
    assert_eq!(laid.baselines("oof-after")[0], [top + 16.0, top + 16.0]);
    assert_eq!(laid.rect("break").height, 20.0);
    // Nor is an atomic inline alone: the one inside gives its baseline.
    assert_eq!(laid.rect("nested-atomic").height, 20.0);
}

#[test]
fn placed_whole_by_their_lines() {
    let laid = lay_out(&format!(
        "{AHEM}<div id=wrap style='width: 100px'>XX <span id=moved>YYYY</span> Z</div>"
    ));
    // Too wide for what is left of the first line: on the next, whole.
    let top = laid.rect("wrap").y;
    assert_eq!(laid.rect("moved"), RectF::new(0.0, top + 20.0, 80.0, 20.0));
    // It is the block's child, which the second line paints first,
    // before `Z`.
    let wrap = &laid.fragments["wrap"];
    assert_eq!(wrap.children.len(), 1);
    assert_eq!(
        wrap.lines[1].atomic_inlines,
        [AtomicInline {
            child: 0,
            paint_before: 0,
            after_boxes: 0,
            hidden: false,
        }]
    );
    assert!(wrap.lines[0].atomic_inlines.is_empty());
}

#[test]
fn in_an_inline_element_and_an_anonymous_block() {
    let laid = lay_out(&format!(
        "{AHEM}<div id=outer>X<b id=b style='padding: 0 10px'>Y<span id=in>Z</span></b><div>after</div></div>"
    ));
    // After `X`, the inline element's padding and `Y`; it is in the
    // anonymous block around the first run, which holds it.
    assert_eq!(laid.rect("in").x, 50.0);
    let outer = &laid.fragments["outer"];
    assert!(outer.children[0].fragment.node.is_none());
    assert_eq!(outer.children[0].fragment.children.len(), 1);
}

#[test]
fn text_that_fits_exactly_stays_on_one_line() {
    // Lato's advances aren't whole pixels: the width it fits in comes back
    // through its border box and padding with float noise.
    let laid = lay_out(
        "<p style='font: 16px sans-serif'>x<span id=fit style='display: inline-block;
           padding: 4px; border: 2px solid; margin: 0 5px'>About us</span>y</p>",
    );
    assert_eq!(laid.fragments["fit"].lines.len(), 1);
}

#[test]
fn inline_flex_and_grid_containers() {
    let laid = lay_out(&format!(
        "{AHEM}<div>A<b id=flex style='display: inline-flex; margin: 0 10px'><i>XX</i><i>Y</i></b>Z</div>
         <div>A<b id=grid style='display: inline-grid; grid-template-columns: auto auto'><i>XX</i><i>Y</i></b>Z</div>
         <div id=column>A<b id=col style='display: inline-flex; flex-direction: column'><i>X</i><i>Y</i></b>Z</div>
         <div style='width: 100px'><b id=wraps style='display: inline-flex; margin: 0 10px;
           flex-wrap: wrap'><i>XX</i><i>YY</i></b></div>"
    ));
    // As wide as their items, after the margin.
    let flex = laid.rect("flex");
    assert_eq!((flex.x, flex.size()), (30.0, SizeF::new(60.0, 20.0)));
    assert_eq!(laid.rect("grid").size(), SizeF::new(60.0, 20.0));
    // On its first baseline: the first item's, in a column.
    let top = laid.rect("column").y;
    assert_eq!(laid.rect("col").size(), SizeF::new(20.0, 40.0));
    assert_eq!(laid.baselines("column")[0], [top + 16.0, top + 16.0]);
    // Its margins taken off the room once: both items fit on its line.
    assert_eq!(laid.rect("wraps").size(), SizeF::new(80.0, 20.0));
}

#[test]
fn what_resolves_against_its_containing_block_and_its_own_width() {
    let laid = lay_out(&format!(
        "{AHEM}<div id=clipping>X<span id=has-clipper><div style='overflow: hidden; height: 30px;
           margin-bottom: 5px'>A</div></span>Y</div>
         <div><span style='height: 100px'>X<span id=half style='height: 50%'>Y</span></span></div>
         <div><span style='height: 200px; max-height: 100px'>X<span id=clamped
           style='height: 100%'>Y</span></span></div>
         <div style='display: flex; height: 100px'><div>X<span id=stretched
           style='height: 50%'>Y</span></div></div>
         <div><b id=gaps style='display: inline-grid; grid-template-columns: 40px 40px;
           column-gap: 20%'><i id=first></i><i id=second></i></b></div>"
    ));
    // A block in it that clips: its bottom margin edge, not its line.
    let top = laid.rect("clipping").y;
    assert_eq!(laid.baselines("clipping")[0], [top + 35.0, top + 35.0]);
    // Percentages of its containing block's height, when definite: its
    // style's, between its minimum and maximum.
    assert_eq!(laid.rect("half").height, 50.0);
    assert_eq!(laid.rect("clamped").height, 100.0);
    // Or the height layout gives it: a flex item stretched to its line.
    assert_eq!(laid.rect("stretched").height, 50.0);
    // A grid's percentage gap, of the width its tracks give it: 16px.
    assert_eq!(laid.rect("gaps").width, 80.0);
    assert_eq!(laid.rect("second").x - laid.rect("first").x, 56.0);
}

/// An iframe is a replaced element (Blink's `LayoutIFrame`): with no
/// natural size or ratio it is 300×150 (CSS's default object size), plus
/// the UA stylesheet's 2px border; `width` and `height` are presentational
/// hints and `frameborder=0` takes the border off; a percentage width and a
/// grid area size it, the other axis staying 150; its children make no
/// boxes.
#[test]
fn iframes_are_replaced_elements() {
    let laid = lay_out(
        "<body style='margin: 0'>
           <iframe id=plain></iframe>
           <iframe id=sized width=100 height=50 frameborder=0></iframe>
           <iframe id=half style='display: block; width: 50%'></iframe>
           <div style='display: flex'><iframe id=item></iframe></div>
           <div style='display: grid; grid-template: 200px / 400px'><iframe id=cell></iframe></div>
           <iframe id=fallback>text the parser keeps</iframe>",
    );
    assert_eq!(laid.rect("plain").size(), SizeF::new(304.0, 154.0));
    assert_eq!(laid.rect("sized").size(), SizeF::new(100.0, 50.0));
    assert_eq!(laid.rect("half").size(), SizeF::new(404.0, 154.0));
    assert_eq!(laid.rect("item").size(), SizeF::new(304.0, 154.0));
    // No natural size: stretched, unlike an image (css-align 3).
    assert_eq!(laid.rect("cell").size(), SizeF::new(400.0, 200.0));
    assert!(laid.fragments["fallback"].children.is_empty());
    assert!(laid.fragments["fallback"].lines.is_empty());
}

/// A canvas is a replaced element (Blink's `LayoutHTMLCanvas`) as big as
/// its bitmap: its `width` and `height` attributes, 300×150 by default,
/// which aren't presentational hints; a CSS size on one axis keeps its
/// ratio; its fallback content makes no boxes; one 0 wide has no ratio.
#[test]
fn canvases_are_replaced_elements_as_big_as_their_bitmaps() {
    let laid = lay_out(
        "<body style='margin: 0'>
           <canvas id=plain></canvas>
           <canvas id=sized width=100 height=50></canvas>
           <canvas id=wide width=100 height=50 style='width: 200px'></canvas>
           <canvas id=styled width=100 height=50 style='width: 30px; height: 40px'></canvas>
           <canvas id=bad width=-4 height=x></canvas>
           <canvas id=empty width=0 height=50 style='width: 20px'></canvas>
           <div style='display: flex'><canvas id=item width=60 height=30></canvas></div>
           <canvas id=fallback width=10 height=10><p>Your browser has no canvas</p></canvas>",
    );
    assert_eq!(laid.rect("plain").size(), SizeF::new(300.0, 150.0));
    assert_eq!(laid.rect("sized").size(), SizeF::new(100.0, 50.0));
    assert_eq!(laid.rect("wide").size(), SizeF::new(200.0, 100.0));
    assert_eq!(laid.rect("styled").size(), SizeF::new(30.0, 40.0));
    assert_eq!(laid.rect("bad").size(), SizeF::new(300.0, 150.0));
    // No ratio: the height is the natural one.
    assert_eq!(laid.rect("empty").size(), SizeF::new(20.0, 50.0));
    assert_eq!(laid.rect("item").size(), SizeF::new(60.0, 30.0));
    assert_eq!(laid.rect("fallback").size(), SizeF::new(10.0, 10.0));
    assert!(laid.fragments["fallback"].children.is_empty());
    assert!(laid.fragments["fallback"].lines.is_empty());
}
