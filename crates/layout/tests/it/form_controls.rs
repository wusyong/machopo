//! Form controls' own sizes and baselines, Chrome 152's with the same Lato
//! (13.333px, the UA stylesheet's): a text field's from its `size` and its
//! font, a text area's from `cols` and `rows`, a slider's, a check box's
//! and a radio button's; in a block sized as replaced elements are (an
//! `auto` width is their own), stretched in a grid or a flex container; a
//! button's content vertically centered in it, its `auto` width
//! fit-content in a block.

use std::collections::BTreeMap;
use std::sync::Arc;

use gfx::{NodeId, PointF, RectF};
use layout::BoxFragment;

use crate::page::Page;

/// Border boxes in LayoutView coordinates, by `id` attribute; an element's
/// descendants' (its UA shadow tree's inner editor and placeholder) as
/// `id/0`, `id/1`, `id/0/1`..., and the first line's of each as
/// `id/line`, `id/0/line`.
fn lay_out(html: &str) -> BTreeMap<String, RectF> {
    let laid = crate::page::lay_out(html);
    let mut rects = BTreeMap::new();
    collect(&laid.view, PointF::new(0.0, 0.0), &laid.ids, &mut rects);
    rects
}

fn collect(
    fragment: &Arc<BoxFragment>,
    origin: PointF,
    ids: &BTreeMap<NodeId, String>,
    out: &mut BTreeMap<String, RectF>,
) {
    if let Some(id) = fragment.node.and_then(|n| ids.get(&n)) {
        let size = fragment.size;
        out.insert(
            id.clone(),
            RectF::new(origin.x, origin.y, size.width, size.height),
        );
        if let Some(line) = fragment.lines.first() {
            out.insert(
                format!("{id}/line"),
                RectF::new(
                    origin.x + line.rect.x,
                    origin.y + line.rect.y,
                    line.rect.width,
                    line.rect.height,
                ),
            );
        }
        descendants(fragment, origin, id, out);
    }
    for child in &fragment.children {
        let at = PointF::new(origin.x + child.offset.x, origin.y + child.offset.y);
        collect(&child.fragment, at, ids, out);
    }
}

/// `fragment`'s children's boxes and first lines, as `prefix/index`, and
/// theirs.
fn descendants(
    fragment: &Arc<BoxFragment>,
    origin: PointF,
    prefix: &str,
    out: &mut BTreeMap<String, RectF>,
) {
    for (index, child) in fragment.children.iter().enumerate() {
        let at = PointF::new(origin.x + child.offset.x, origin.y + child.offset.y);
        let size = child.fragment.size;
        let name = format!("{prefix}/{index}");
        out.insert(
            name.clone(),
            RectF::new(at.x, at.y, size.width, size.height),
        );
        if let Some(line) = child.fragment.lines.first() {
            out.insert(
                format!("{name}/line"),
                RectF::new(
                    at.x + line.rect.x,
                    at.y + line.rect.y,
                    line.rect.width,
                    line.rect.height,
                ),
            );
        }
        descendants(&child.fragment, at, &name, out);
    }
}

/// [`assert_size`] to within a hundredth of a pixel: Chrome's are in
/// 64ths (its `LayoutUnit`), machopo's text isn't rounded.
#[track_caller]
fn assert_size_near(rects: &BTreeMap<String, RectF>, id: &str, width: f32, height: f32) {
    let rect = rects[id];
    assert!(
        (rect.width - width).abs() < 0.01 && (rect.height - height).abs() < 0.01,
        "#{id}: {} x {}, not {width} x {height}",
        rect.width,
        rect.height
    );
}

#[track_caller]
fn assert_size(rects: &BTreeMap<String, RectF>, id: &str, width: f32, height: f32) {
    let rect = rects[id];
    assert_eq!((rect.width, rect.height), (width, height), "#{id}");
}

#[test]
fn text_controls_are_sized_by_their_attributes_and_font() {
    let r = lay_out(
        "<input id=default><input size=1 id=size-1><input size=10 id=size-10>
         <input size=0 id=size-0><input size=' 3x' id=size-3>
         <input style='font-size: 20px' id=font-20><input style='font-size: 16px' id=font-16>
         <input style='font-size: 10.4px' id=font-10.4><input type=search id=search>
         <input type=number id=number><input type=password id=password>
         <input type=bogus id=bogus>
         <textarea id=textarea>text that isn't its content</textarea>
         <textarea cols=10 rows=4 id=textarea-10x4></textarea>
         <textarea cols=0 rows=x id=textarea-defaults></textarea>",
    );
    // `size` average characters plus the widest's room past one: 20 × 7
    // + 25 − 7 = 158 at 13.333px, inside 1px 2px of padding and a 2px
    // border; as high as a line.
    assert_size(&r, "default", 166.0, 22.0);
    assert_size(&r, "size-1", 33.0, 22.0);
    assert_size(&r, "size-10", 96.0, 22.0);
    // Not above 0: the default; leading digits count.
    assert_size(&r, "size-0", 166.0, 22.0);
    assert_size(&r, "size-3", 47.0, 22.0);
    assert_size(&r, "font-20", 238.0, 30.0);
    // Its line's ascent and descent each rounded: 16 + 3 at 16px (not
    // 19.2); the widest character's room of the font size as it is.
    assert_size(&r, "font-16", 192.0, 25.0);
    assert_size(&r, "font-10.4", 128.0, 18.0);
    // A search field's `border-box` holds the same.
    for id in ["search", "number", "password", "bogus"] {
        assert_size(&r, id, 166.0, 22.0);
    }
    // `cols` characters and a scrollbar's 15px, `rows` lines; its children
    // aren't laid out.
    assert_size(&r, "textarea", 161.0, 38.0);
    assert_size(&r, "textarea-10x4", 91.0, 70.0);
    assert_size(&r, "textarea-defaults", 161.0, 38.0);
}

#[test]
fn sliders_check_boxes_and_radio_buttons_have_the_themes_size() {
    let r = lay_out(
        "<input type=range id=range><input type=checkbox id=checkbox>
         <input type=radio id=radio><input type=range style='appearance: none' id=range-none>
         <input type=range style='width: 50px' id=range-50>",
    );
    assert_size(&r, "range", 129.0, 16.0);
    assert_size(&r, "checkbox", 13.0, 13.0);
    assert_size(&r, "radio", 13.0, 13.0);
    assert_size(&r, "range-none", 129.0, 16.0);
    assert_size(&r, "range-50", 50.0, 16.0);
}

#[test]
fn controls_are_sized_as_replaced_elements() {
    let r = lay_out(
        "<input style='display: block' id=block>
         <input style='width: 100%; display: block' id=percent>
         <div style='display: flex; width: 50px'><input id=in-flex></div>
         <div style='display: flex; width: 50px'><textarea id=textarea-in-flex></textarea></div>
         <div style='display: flex; width: 20px'><input type=range id=range-in-flex></div>
         <div style='display: grid; grid-template-columns: 300px'><input id=in-grid></div>
         <div style='display: grid; grid-template-columns: 300px'><input type=range id=range-in-grid></div>
         <div style='display: flex; flex-direction: column; width: 300px'><input id=in-column></div>",
    );
    // An `auto` width is its own, not the containing block's.
    assert_size(&r, "block", 166.0, 22.0);
    // A percentage width (of its content box, 784px) doesn't change its
    // height (no ratio).
    assert_size(&r, "percent", 792.0, 22.0);
    // A flex item's automatic minimum is its own size, but a text area's,
    // which scrolls (`overflow: auto`): 0.
    assert_size(&r, "in-flex", 166.0, 22.0);
    assert_eq!(r["textarea-in-flex"].width, 50.0);
    assert_size(&r, "range-in-flex", 129.0, 16.0);
    // A grid and a flex container stretch it as any box (Chrome's; not as
    // a replaced element).
    assert_size(&r, "in-grid", 300.0, 22.0);
    assert_size(&r, "range-in-grid", 296.0, 16.0);
    assert_size(&r, "in-column", 300.0, 22.0);
}

/// Where each control sits on a line: its baseline, from the top of its
/// border box, against a 0×0 inline-block's bottom margin edge (the
/// line's baseline).
#[test]
fn controls_baselines() {
    let r = lay_out(
        "<div style='font: 20px/40px Lato'>
           <span style='display: inline-block' id=z></span>
           <input id=text><input type=checkbox id=checkbox><input type=radio id=radio>
           <input type=range id=range><button id=button>Go</button><button id=empty></button>
           <input style='height: 40px' id=tall><input style='height: 10px' id=short>
         </div>
         <div style='font: 20px/40px Lato'>
           <span style='display: inline-block' id=z3></span>
           <input type=checkbox style='appearance: none; width: 13px; height: 13px' id=css-checkbox>
           <input type=range style='appearance: none' id=css-range>
         </div>
         <div style='font: 20px/40px Lato'>
           <span style='display: inline-block' id=z2></span><textarea id=textarea></textarea>
         </div>",
    );
    let baseline = r["z"].bottom();
    let above = |id: &str| baseline - r[id].y;
    // Its text's, a line of Lato at 13.333px (ascent 13) under 3px.
    assert_eq!(above("text"), 16.0);
    assert_eq!(above("button"), 16.0);
    // On its border box's bottom, its margin below.
    assert_eq!(above("checkbox"), 13.0);
    assert_eq!(above("radio"), 13.0);
    assert_eq!(above("range"), 16.0);
    // One drawn as CSS is an empty inline-block, its bottom margin edge
    // on the baseline; a slider's is its border box's bottom all the same.
    let above_3 = |id: &str| r["z3"].bottom() - r[id].y;
    assert_eq!(above_3("css-checkbox"), 16.0);
    assert_eq!(above_3("css-range"), 16.0);
    // An empty button's content box's top.
    assert_eq!(above("empty"), 3.0);
    // A text field's line is centered in it: (40 − 16) / 2 + 13 + 3; above
    // its top where it doesn't fit.
    assert_eq!(above("tall"), 28.0);
    assert_eq!(above("short"), 13.0);
    // A text area's bottom (it clips its overflow).
    assert_eq!(r["textarea"].bottom(), r["z2"].bottom());
}

#[test]
fn a_buttons_content_is_centered_in_it() {
    let r = lay_out(
        "<button style='height: 40px' id=tall><span style='display: inline-block' id=tall-text>Go</span></button>
         <button style='height: 40px; display: block' id=block><span style='display: inline-block' id=block-text>Go</span></button>
         <button style='width: 100px' id=wide><span style='display: inline-block' id=wide-text>Go</span></button>
         <button style='height: 40px; display: flex; align-items: start' id=flex><span style='display: inline-block' id=flex-text>Go</span></button>
         <button style='height: 40px' id=blocks><div style='height: 10px' id=first></div><div style='height: 10px'></div></button>
         <button style='height: 10px' id=small><span style='display: inline-block' id=small-text>Go</span></button>",
    );
    let from_top = |button: &str, content: &str| r[content].y - r[button].y;
    // (34 − 16) / 2 below the content box's top.
    assert_eq!(from_top("tall", "tall-text"), 12.0);
    assert_eq!(from_top("block", "block-text"), 12.0);
    // Centered across too (`text-align: center`).
    let wide = r["wide"];
    let text = r["wide-text"];
    assert!(
        ((text.x - wide.x) - (wide.right() - text.right())).abs() < 0.01,
        "{wide:?} {text:?}"
    );
    // A flex button lays its content out as flex does: at its start.
    assert_eq!(from_top("flex", "flex-text"), 3.0);
    // Its block children as one: (34 − 20) / 2.
    assert_eq!(from_top("blocks", "first"), 10.0);
    // Where it doesn't fit, at the top.
    assert_eq!(from_top("small", "small-text"), 3.0);
}

/// In a block a button's `auto` width is fit-content, whatever its
/// `display`, and it is a flow root (Chrome 152): beside a float, around
/// its own; a flex or grid container stretches it, and out of flow it is
/// sized as any box.
#[test]
fn a_button_in_a_block_is_fit_content_and_a_flow_root() {
    let r = lay_out(
        "<style>
           body { margin: 0; width: 400px }
           button, input { padding: 0; border: 0; margin: 0 }
           .w { display: inline-block; width: 50px; height: 20px }
           .f { float: left; width: 100px; height: 50px }
         </style>
         <button style='display: block' id=block><span class=w></span></button>
         <button style='display: flex' id=flex><span class=w></span><span class=w></span></button>
         <button style='display: grid' id=grid><span class=w></span></button>
         <input type=submit value='' style='display: block' id=submit>
         <button style='display: block; width: 50%' id=percent><span class=w></span></button>
         <div style='width: 30px'><button style='display: block' id=narrow><span class=w></span> <span class=w></span></button></div>
         <div style='display: flex; flex-direction: column'><button id=column><span class=w></span></button></div>
         <div style='display: grid'><button id=in-grid><span class=w></span></button></div>
         <div style='position: relative; height: 20px'><button style='position: absolute; left: 0; right: 0' id=absolute><span class=w></span></button></div>
         <div><div class=f></div><button style='display: block' id=beside><span class=w></span></button></div>
         <div style='clear: both' id=holder><button style='display: block' id=around><div class=f></div></button></div>",
    );
    for (id, width) in [
        ("block", 50.0),
        ("flex", 100.0),
        ("grid", 50.0),
        ("submit", 0.0),
        ("percent", 200.0),
        // Its min-content width, wider than the room.
        ("narrow", 50.0),
        ("column", 400.0),
        ("in-grid", 400.0),
        ("absolute", 400.0),
    ] {
        assert_eq!(r[id].width, width, "#{id}");
    }
    assert_eq!(r["beside"].x, 100.0);
    assert_eq!(r["around"].height, 50.0);
    assert_eq!(r["holder"].height, 50.0);
}

/// A check box's, a slider's, a color well's, a file field's and a date
/// field's `::before` is rendered in its box, as a `<button>`'s; a text
/// control's, a `<select>`'s, an `<input>` button's and an image button's
/// isn't (Chrome 152).
#[test]
fn which_controls_render_their_pseudo_elements() {
    let r = lay_out(
        "<style>
           .t { appearance: none; display: block; margin: 0; padding: 0; border: 0; height: auto; min-height: 0 }
           .t::before { content: ''; display: block; height: 30px }
         </style>
         <input type=checkbox class=t id=checkbox><input type=radio class=t id=radio>
         <input type=color class=t id=color><input type=file class=t id=file>
         <input type=date class=t id=date><button class=t id=button></button>
         <input type=button class=t id=input-button><input type=submit class=t id=submit>
         <input type=image class=t id=image><select class=t id=select></select>
         <div class=t id=div></div>",
    );
    for id in [
        "checkbox", "radio", "color", "file", "date", "button", "div",
    ] {
        assert_eq!(r[id].height, 30.0, "#{id}");
    }
    // An `<input>` button's label's line, a menu list's inner element's
    // (Chrome's is 15px high), not its `::before`.
    for id in ["input-button", "submit", "select"] {
        assert_eq!(r[id].height, 16.0, "#{id}");
    }
    assert_eq!(r["image"].height, 0.0);
}

/// A text control's value and placeholder (its UA shadow tree's inner
/// editor and placeholder) are laid out in the control's own size, which
/// they don't change: at the content box's start, a text field's line in
/// its middle (even where it doesn't fit), a long value past its end (the
/// inner editor clips it), the placeholder only while the value is empty;
/// a password field's characters are discs (Chrome 152).
#[test]
fn a_text_controls_value_is_laid_out_in_its_size() {
    let long = "word ".repeat(30);
    let discs = "\u{2022}".repeat(6);
    let r = lay_out(&format!(
        "<input id=value value=Hello><input id=long value='{long}'>
         <input id=tall style='height: 40px' value=x><input id=small style='height: 6px' value=x>
         <input id=placeholder placeholder=Hint><input id=both placeholder=Hint value=v>
         <textarea id=area>Its value</textarea>
         <input type=password id=password value=secret><input id=discs value={discs}>
         <input id=plain value=secret><input id=empty>
         <input type=number id=number>"
    ));
    let from = |id: &str, child: &str| {
        let (outer, inner) = (r[id], r[child]);
        (
            inner.x - outer.x,
            inner.y - outer.y,
            inner.width,
            inner.height,
        )
    };
    for id in ["value", "long", "placeholder", "both", "password"] {
        assert_size(&r, id, 166.0, 22.0);
    }
    // Its border and padding off, as wide as the content box, a line high.
    assert_eq!(from("value", "value/0"), (4.0, 3.0, 158.0, 16.0));
    assert!(r["value/0/line"].width > 0.0);
    assert_eq!(from("long", "long/0").2, 158.0);
    assert!(r["long/0/line"].width > 158.0);
    // (40 − 16) / 2 below the content box's top; (6 − 16) / 2 above it.
    assert_size(&r, "tall", 166.0, 46.0);
    assert_eq!(from("tall", "tall/0").1, 3.0 + 12.0);
    assert_eq!(from("small", "small/0").1, 3.0 - 5.0);
    // The placeholder, then the empty inner editor.
    assert_eq!(
        from("placeholder", "placeholder/0"),
        (4.0, 3.0, 158.0, 16.0)
    );
    assert_eq!(from("placeholder", "placeholder/1").3, 0.0);
    assert!(!r.contains_key("both/1"), "the placeholder isn't laid out");
    assert_size(&r, "area", 161.0, 38.0);
    assert_eq!(from("area", "area/0").0, 3.0);
    assert!(!r.contains_key("area/1"), "its children aren't laid out");
    // Empty, without a placeholder: a line all the same (an editable
    // block's), which the spin button beside it is as high as.
    assert_eq!(from("empty", "empty/0"), (4.0, 3.0, 158.0, 16.0));
    assert_eq!(from("number", "number/0/0/0").3, 16.0);
    assert_eq!(from("number", "number/0/1").3, 16.0);
    assert_eq!(r["password/0/line"].width, r["discs/0/line"].width);
    assert_ne!(r["password/0/line"].width, r["plain/0/line"].width);
}

/// A flex container aligns a text field by its text's baseline.
#[test]
fn a_flex_container_aligns_a_text_field_by_its_baseline() {
    let r = lay_out(
        "<div style='display: flex; align-items: baseline; font: 20px/1 Ahem'>           <span style='display: inline-block' id=x>x</span><input id=field>
         <input id=tall style='height: 40px' value=x></div>",
    );
    // Ahem's baseline is 16px down its 20px line; the field's, 16px
    // down its border box; a tall one's, its line in the middle, 3 +
    // (40 − 16) / 2 + 13.
    assert_eq!(r["field"].y - r["x"].y, 0.0);
    assert_eq!(r["tall"].y - r["x"].y, 16.0 - 28.0);
}

/// A text area's value that ends in a newline has a line after it, where
/// the caret goes (Chrome 152's `scrollHeight`: Blink adds a placeholder
/// `<br>`), though a forced break ending other content makes none.
#[test]
fn a_text_areas_final_newline_has_a_line_after_it() {
    let r = lay_out(
        "<textarea id=one>a</textarea><textarea id=two>a\n</textarea>
         <textarea id=three>a\n\n</textarea>",
    );
    let line = r["one/0"].height;
    assert!(line > 0.0);
    assert_eq!(r["two/0"].height, 2.0 * line);
    assert_eq!(r["three/0"].height, 3.0 * line);
}

/// A text area's children are its value, not its content: no text.
#[test]
fn a_text_areas_children_arent_laid_out() {
    let view = Page::without_shadow_trees("<textarea>a value</textarea>")
        .lay_out()
        .view;
    // The text runs on lines (the body's line holds the text area).
    fn lines(fragment: &BoxFragment) -> usize {
        fragment
            .lines
            .iter()
            .map(|line| line.runs.len())
            .sum::<usize>()
            + fragment
                .children
                .iter()
                .map(|child| lines(&child.fragment))
                .sum::<usize>()
    }
    assert_eq!(lines(&view), 0, "{}", view.dump());
}

/// A form control's `display: inline` box is an atomic inline, as an
/// inline-block's: it has its size, beside the next on the line.
#[test]
fn an_inline_control_is_an_atomic_inline() {
    // Without `appearance` (with one, the theme makes it `inline-block`),
    // still sized as a control (Chrome 152).
    let r = lay_out(
        "<style>.n { appearance: none; display: inline }</style>
         <input class=n id=a><input class=n id=b><textarea class=n id=area></textarea>
         <button class=n id=button>Go</button>",
    );
    assert_size(&r, "a", 166.0, 22.0);
    assert_eq!(r["b"].y, r["a"].y);
    assert_eq!(r["b"].x, r["a"].right());
    assert_size(&r, "area", 161.0, 38.0);
    assert_eq!(r["button"].height, 22.0);
}

/// An `<input>` button is as big as its label (Chrome 152 with the same
/// Lato): its value, its type's, or a line of nothing; a search field's
/// and a number field's value leave room for their part, the clear
/// button (12px and 1px before it) and the spin button (15px), there
/// while hidden, the placeholder too.
#[test]
fn an_input_buttons_label_and_a_fields_part() {
    let r = lay_out(
        "<input type=submit id=submit><input type=reset id=reset><input type=button id=empty>
         <input type=button value=Go id=go><input type=submit value='  Send  ' id=send>
         <input type=button value='1&#10;2' id=lines>
         <input type=search value=x id=search><input type=search placeholder=Find id=find>
         <input type=number value=5 id=number>
         <input type=button value=g style='display: grid; height: 40px' id=grid-input>
         <button style='display: grid; height: 40px' id=grid-button>g</button>",
    );
    assert_size_near(&r, "submit", 57.15625, 22.0);
    assert_size_near(&r, "reset", 48.65625, 22.0);
    assert_size(&r, "empty", 16.0, 22.0);
    assert_size_near(&r, "go", 33.234375, 22.0);
    assert_size_near(&r, "send", 58.703125, 22.0);
    assert_size_near(&r, "lines", 23.734375, 38.0);
    for id in ["search", "find", "number"] {
        assert_size(&r, id, 166.0, 22.0);
    }
    let x = |id: &str| r[id].x;
    // The decoration container, its editing view port with the inner
    // editor, then the part.
    assert_eq!(r["search/0"].width, 158.0);
    assert_eq!(r["search/0/0"].width, 158.0 - 13.0);
    assert_eq!(r["search/0/0/0"].width, 158.0 - 13.0);
    assert_eq!(
        (x("search/0/1") - x("search"), r["search/0/1"].width),
        (4.0 + 145.0 + 1.0, 12.0)
    );
    assert_eq!(r["search/0/1"].height, 12.0);
    assert_eq!(r["find/0/0/0"].width, 145.0, "the placeholder");
    assert_eq!(r["number/0/0"].width, 158.0 - 15.0);
    assert_eq!(
        (r["number/0/1"].width, r["number/0/1"].height),
        (15.0, 16.0)
    );
    // An `<input>` button's label is in its middle whatever its `display`
    // ((40 − 6 − 16) / 2 below its content box's top); a `<button>`'s
    // content is laid out as its `display` says.
    assert_eq!(r["grid-input/line"].y - r["grid-input"].y, 3.0 + 9.0);
    assert_eq!(r["grid-button/0/line"].y - r["grid-button"].y, 3.0);
}

/// A text field's inner editor clips its value at the field's content box
/// (Blink's inner editor style); a text area's doesn't, the text area
/// scrolls.
#[test]
fn a_text_fields_inner_editor_clips_its_value() {
    let laid = crate::page::lay_out("<input id=field value=text><textarea id=area>text</textarea>");
    let (view, ids) = (laid.view, laid.ids);
    fn editor_clips(
        fragment: &Arc<BoxFragment>,
        ids: &BTreeMap<NodeId, String>,
        out: &mut BTreeMap<String, bool>,
    ) {
        if let Some(id) = fragment.node.and_then(|n| ids.get(&n)) {
            out.insert(
                id.clone(),
                fragment.children[0].fragment.style.clips_overflow(),
            );
        }
        for child in &fragment.children {
            editor_clips(&child.fragment, ids, out);
        }
    }
    let mut clips = BTreeMap::new();
    editor_clips(&view, &ids, &mut clips);
    assert!(clips["field"]);
    assert!(!clips["area"]);
}

/// A menu list is as wide as its widest option's label (in its font,
/// rounded up), whichever it shows, and with a native look a line of its
/// font high whatever its `line-height` (without, its `line-height`'s),
/// with the native look's room: 4px and 16px across, 1px above and below
/// the line. Its inner element holds that room, cut where the select is
/// narrower, and is centered where it is taller (at its top where it is
/// shorter). Chrome 152's, the same Lato.
#[test]
fn a_menu_list_is_as_wide_as_its_widest_option() {
    let r = lay_out(
        "<div><select id=empty></select></div>
         <div><select id=one><option>A</option></select></div>
         <div><select id=long><option>A</option><option>A much longer option</option></select></div>
         <div><select id=shown><option>A</option><option selected>A much longer option</option></select></div>
         <div><select id=big style='font-size: 20px'><option>Abc</option></select></div>
         <div><select id=narrow style='width: 100px'><option>A much longer option than fits</option></select></div>
         <div><select id=styled style='border: 1px solid red; background: yellow'><option>Styled</option></select></div>
         <div><select id=padded style='padding: 5px'><option>Pad</option></select></div>
         <div><select id=tall style='height: 40px'><option>Tall</option></select></div>
         <div><select id=short style='height: 10px'><option>Short</option></select></div>
         <div><select id=spaced><option label='  a   b '>Text</option></select></div>
         <div><select id=grouped><optgroup label=G><option>Grouped</option></optgroup></select></div>
         <div><select id=multiple multiple size=1><option>One</option><option>Two</option></select></div>
         <div><select id=unselected><option disabled>Off</option></select></div>
         <div><select id=line-height style='line-height: 30px'><option>Lh</option></select></div>
         <div><select id=none style='appearance: none'><option>A</option><option>A much longer option</option></select></div>
         <div><select id=none-line-height style='appearance: none; line-height: 30px'><option>None</option></select></div>",
    );
    for (id, width, height) in [
        ("empty", 22.0, 20.0),
        ("one", 32.0, 20.0),
        ("long", 149.0, 20.0),
        ("shown", 149.0, 20.0),
        ("big", 57.0, 28.0),
        ("narrow", 100.0, 20.0),
        ("styled", 59.0, 20.0),
        ("padded", 55.0, 30.0),
        ("tall", 42.0, 40.0),
        ("short", 54.0, 10.0),
        // Its label as it is, spaces too, though they aren't shown.
        ("spaced", 57.0, 20.0),
        // Indented in its group.
        ("grouped", 87.0, 20.0),
        // As wide as "2 selected" too.
        ("multiple", 82.0, 20.0),
        ("unselected", 42.0, 20.0),
        ("line-height", 37.0, 20.0),
        // Chrome's is a pixel shorter, its font's line a pixel shorter
        // there than its `line-height: normal`'s; machopo's are the same.
        ("none", 129.0, 18.0),
        ("none-line-height", 35.0, 32.0),
    ] {
        assert_size(&r, id, width, height);
    }
    // The inner element: 4px in from the border, 16px short of it, 1px
    // above and below its line.
    let (one, inner) = (r["one"], r["one/0"]);
    assert_eq!(
        (inner.x - one.x, inner.y - one.y, inner.width, inner.height),
        (5.0, 1.0, 10.0, 18.0)
    );
    assert_eq!(r["narrow/0"].width, 78.0);
    let none = (r["none"], r["none/0"]);
    assert_eq!((none.1.x - none.0.x, none.1.y - none.0.y), (1.0, 1.0));
    // Centered in a taller one, at the top of a shorter one.
    assert_eq!(r["tall/0"].y - r["tall"].y, 1.0 + (38.0 - 18.0) / 2.0);
    assert_eq!(r["short/0"].y - r["short"].y, 1.0);
}

/// A menu list's baseline is its text's, centered in a taller one: in a
/// line it ends 2px lower than the 16px text beside it, a 40px one 12px.
/// Chrome 152's end 2.5px and 12.5px lower: its text is half a pixel
/// higher in the menu list.
#[test]
fn a_menu_lists_baseline_is_its_texts() {
    let r = lay_out(
        "<div style='font-size: 16px'><span id=text style='display: inline-block'>base</span>
           <select id=select><option>xyz</option></select></div>
         <div style='font-size: 16px'><span id=tall-text style='display: inline-block'>base</span>
           <select id=tall style='height: 40px'><option>xyz</option></select></div>",
    );
    let bottom = |id: &str| r[id].y + r[id].height;
    assert_eq!(bottom("select") - bottom("text"), 2.0);
    assert_eq!(bottom("tall") - bottom("tall-text"), 12.0);
}

/// A list box (`multiple`, or a `size` above 1) lays its options out as
/// blocks, `size` rows high (4 without one) of its tallest row (an
/// option, an optgroup's label; with none, a line and 1px), as wide as
/// its widest and a scrollbar's 15px; an option in a group indented 20px,
/// an `<hr>` half a line of room. Chrome 152's, the same Lato.
#[test]
fn a_list_box_is_size_rows_high() {
    let r = lay_out(
        "<div><select id=size4 size=4><option id=one>One</option><option>Two</option></select></div>
         <div><select id=empty size=4></select></div>
         <div><select id=multiple multiple><option>One</option><option>Two</option><option>Three</option>
           <option>Four</option><option>Five</option></select></div>
         <div><select id=grouped size=6><optgroup label=' Group ' id=group><option id=in>In</option></optgroup>
           <option id=out>Out</option><hr id=hr><option id=after>After hr</option></select></div>
         <div><select id=big size=3 style='font-size: 20px'><option id=abc>Abc</option></select></div>
         <div><select id=size2 size=2><option>One</option><option>Two</option><option>Three</option></select></div>
         <div><select id=tall size=3><option style='font-size: 30px' id=tall-option>Big</option>
           <option id=small>small</option></select></div>
         <div><select id=none size=3 style='appearance: none'><option>One</option></select></div>
         <div><select id=big-label size=3><optgroup label=G style='font-size: 20px'>
           <option style='font-size: 13.333333px'>In</option></optgroup></select></div>
         <div><select id=fixed size=3 style='height: 50px'><option>One</option></select></div>
         <div><select id=min-height size=3 style='min-height: 80px'><option>One</option></select></div>",
    );
    for (id, width, height) in [
        ("size4", 46.09375, 70.0),
        ("empty", 17.0, 70.0),
        ("multiple", 55.0, 70.0),
        ("grouped", 66.328125, 104.0),
        ("big", 55.40625, 77.0),
        ("size2", 55.0, 36.0),
        ("tall", 63.15625, 113.0),
        ("none", 46.09375, 53.0),
        // An optgroup's label is a row too.
        ("big-label", 50.15625, 77.0),
        // Its own `height`, `min-height`.
        ("fixed", 46.09375, 50.0),
        ("min-height", 46.09375, 80.0),
    ] {
        assert_size_near(&r, id, width, height);
    }
    let at = |id: &str, select: &str| {
        let (a, b) = (r[id], r[select]);
        (a.x - b.x, a.y - b.y, a.height)
    };
    assert_eq!(at("one", "size4"), (1.0, 1.0, 17.0));
    assert_eq!(at("group", "grouped"), (1.0, 1.0, 34.0));
    assert_eq!(at("in", "grouped"), (1.0, 18.0, 17.0));
    assert_eq!(at("out", "grouped"), (1.0, 35.0, 17.0));
    // Below the `<hr>`'s 0.5em (6.65625 in Chrome's 64ths of a pixel).
    let (_, after, _) = at("after", "grouped");
    assert!((after - (52.0 + 20.0 / 3.0)).abs() < 0.01, "{after}");
    assert_eq!(at("abc", "big").2, 25.0);
    assert_eq!(at("small", "tall"), (1.0, 38.0, 17.0));
    // The options are as wide as the list box but its scrollbar.
    assert_size_near(&r, "one", 46.09375 - 2.0 - 15.0, 17.0);
}
