//! HTML's form pseudo-classes matched and styled again as the form state
//! they read changes: `:checked` (and a sibling's rule, `:checked +
//! label`), `:indeterminate` (a checkbox's, a radio button's group's, a
//! progress's), `:disabled` and `:enabled` (a fieldset's but its first
//! legend's), `:required` and `:optional`, `:read-write` and `:read-only`
//! (an editable element's, its subtree styled again as an ancestor's
//! `contenteditable` changes), `:placeholder-shown` (an input's value, a textarea's: its children's
//! text until set), `:default` (a `checked` attribute, a form's default
//! button). The inputs have no native look, whose sizes would be theirs.

use computed_style::Size;

use crate::Page;

fn px(value: f32) -> Size {
    Size::LengthPercentage(computed_style::LengthPercentage::Px(value))
}

fn width(page: &mut Page, id: &str) -> Size {
    page.style(id).width
}

#[test]
fn checked_and_its_sibling() {
    let mut page = Page::new(
        "<!doctype html><style>input { appearance: none } :checked { width: 1px } :checked + label { width: 2px } #c:checked span { height: 3px }</style>\
         <input type=checkbox id=c><label id=l>l</label><input type=radio name=r id=r1 checked><input type=radio name=r id=r2>\
         <input id=t checked><label id=tl>t</label>",
    );
    // A child (script can give an input one): a state asked of an
    // ancestor.
    page.with("c", |cx, c| {
        let span = cx.create_element(cx.document_of(c), dom::QualName::html("span"));
        cx.set_attribute(span, "id", "cs");
        cx.append_child(c, span).unwrap();
    });
    page.recalc();
    assert_eq!(width(&mut page, "c"), Size::Auto);
    assert_eq!(width(&mut page, "r1"), px(1.0));
    // A text input's checkedness isn't `:checked`.
    assert_eq!(width(&mut page, "t"), Size::Auto);
    page.with("c", |cx, c| cx.set_checked(c, true));
    page.recalc();
    assert_eq!(width(&mut page, "c"), px(1.0));
    assert_eq!(page.style("cs").height, px(3.0), "below");
    assert_eq!(width(&mut page, "l"), px(2.0), "the next sibling");
    // A radio button checked: the other unchecked, styled again.
    page.with("r2", |cx, r2| cx.set_checked(r2, true));
    page.recalc();
    assert_eq!(
        (width(&mut page, "r1"), width(&mut page, "r2")),
        (Size::Auto, px(1.0))
    );
    // A type: the attribute's change.
    page.with("t", |cx, t| cx.set_attribute(t, "type", "checkbox"));
    page.recalc();
    assert_eq!(width(&mut page, "t"), px(1.0));
    assert_eq!(width(&mut page, "tl"), px(2.0), "its type, for the sibling");
}

#[test]
fn indeterminate() {
    let mut page = Page::new(
        "<!doctype html><style>input { appearance: none } :indeterminate { width: 1px } progress:indeterminate + span { height: 3px }</style>\
         <input type=checkbox id=c><input type=radio name=g id=a><input type=radio name=g id=b>\
         <input type=radio id=alone checked><progress id=p></progress><progress id=q value=1></progress><span id=qs>s</span>\
         <form id=f></form><input type=radio name=h id=y><input type=radio name=h id=z form=f checked><input name=h id=x checked>",
    );
    assert_eq!(width(&mut page, "c"), Size::Auto);
    assert_eq!(
        (width(&mut page, "a"), width(&mut page, "b")),
        (px(1.0), px(1.0)),
        "none of the group checked"
    );
    assert_eq!(width(&mut page, "alone"), Size::Auto);
    assert_eq!(
        (width(&mut page, "p"), width(&mut page, "q")),
        (px(1.0), Size::Auto)
    );
    page.with("c", |cx, c| cx.set_indeterminate(c, true));
    // One of the group checked: the other too.
    page.with("a", |cx, a| cx.set_checked(a, true));
    page.recalc();
    assert_eq!(width(&mut page, "c"), px(1.0));
    assert_eq!(
        (width(&mut page, "a"), width(&mut page, "b")),
        (Size::Auto, Size::Auto)
    );
    // Leaving the group (its name): the other's group has none checked.
    page.with("a", |cx, a| cx.set_attribute(a, "name", "other"));
    page.recalc();
    assert_eq!(width(&mut page, "b"), px(1.0));
    // Removed from the tree.
    page.with("b", |cx, b| cx.set_attribute(b, "name", "other"));
    page.recalc();
    assert_eq!(width(&mut page, "b"), Size::Auto);
    page.with("a", |cx, a| cx.remove(a));
    page.recalc();
    assert_eq!(width(&mut page, "b"), px(1.0));
    page.with("q", |cx, q| cx.remove_attribute(q, "value"));
    page.recalc();
    assert_eq!(width(&mut page, "q"), px(1.0));
    assert_eq!(
        page.style("qs").height,
        px(3.0),
        "its value, for the sibling"
    );
    // A checked one coming into the tree's group.
    page.with("b", |cx, b| {
        let radio = cx.create_element(cx.document_of(b), dom::QualName::html("input"));
        cx.set_attribute(radio, "type", "radio");
        cx.set_attribute(radio, "name", "other");
        cx.set_attribute(radio, "checked", "");
        cx.append_child(cx.parent(b).unwrap(), radio).unwrap();
    });
    page.recalc();
    assert_eq!(width(&mut page, "b"), Size::Auto);
    // Its owner (the form it names): its group another.
    assert_eq!(width(&mut page, "y"), px(1.0));
    page.with("z", |cx, z| cx.remove_attribute(z, "form"));
    page.recalc();
    assert_eq!(width(&mut page, "y"), Size::Auto);
    // Its type: a checked text input becoming one of the group.
    page.with("z", |cx, z| cx.set_checked(z, false));
    page.recalc();
    assert_eq!(width(&mut page, "y"), px(1.0));
    page.with("x", |cx, x| cx.set_attribute(x, "type", "radio"));
    page.recalc();
    assert_eq!(width(&mut page, "y"), Size::Auto);
}

/// Radio buttons the parser's fast path makes, of their type from the
/// start (`create_html_element_with_attributes`), are radio buttons seen:
/// a checked one coming into their group styles the others again, in a
/// document that had none before.
#[test]
fn radio_buttons_made_with_their_attributes_are_seen() {
    let mut page = Page::new(
        "<!doctype html><style>input { appearance: none } :indeterminate { width: 1px }</style>\
         <div id=d></div>",
    );
    fn radio<'gc>(cx: &dom::Cx<'_, 'gc>, d: dom::NodeRef<'gc>, id: &str, checked: bool) {
        let mut attributes = vec![("type", "radio"), ("name", "g"), ("id", id)];
        if checked {
            attributes.push(("checked", ""));
        }
        let attributes: Vec<_> = attributes
            .into_iter()
            .map(|(name, value)| (dom::names::intern(name), value))
            .collect();
        let input = cx.create_html_element_with_attributes(
            cx.document_of(d),
            dom::names::local::INPUT,
            &attributes,
        );
        cx.append_child(d, input).unwrap();
    }
    page.with("d", |cx, d| {
        radio(cx, d, "a", false);
        radio(cx, d, "b", false);
    });
    page.recalc();
    assert_eq!(
        (width(&mut page, "a"), width(&mut page, "b")),
        (px(1.0), px(1.0)),
        "none of the group checked"
    );
    page.with("d", |cx, d| radio(cx, d, "c", true));
    page.recalc();
    assert_eq!(
        (width(&mut page, "a"), width(&mut page, "b")),
        (Size::Auto, Size::Auto)
    );
}

#[test]
fn disabled_and_enabled() {
    let mut page = Page::new(
        "<!doctype html><style>input { appearance: none } :disabled { width: 1px } :enabled { width: 2px } fieldset:disabled span { height: 3px } \
         #i:disabled + i { height: 4px }</style>\
         <fieldset id=f><legend><input id=l></legend><div><input id=i><i id=ii>i</i><button id=b>b</button><span id=sp>s</span></div></fieldset>\
         <select id=s></select><textarea id=t disabled></textarea><div id=d disabled></div>",
    );
    assert_eq!(width(&mut page, "i"), px(2.0));
    assert_eq!(width(&mut page, "f"), px(2.0));
    assert_eq!(width(&mut page, "s"), px(2.0));
    assert_eq!(width(&mut page, "t"), px(1.0));
    assert_eq!(width(&mut page, "d"), Size::Auto, "not a form control");
    page.with("f", |cx, f| cx.set_attribute(f, "disabled", ""));
    page.recalc();
    assert_eq!(width(&mut page, "f"), px(1.0));
    assert_eq!(width(&mut page, "i"), px(1.0), "in the fieldset");
    assert_eq!(width(&mut page, "b"), px(1.0));
    assert_eq!(width(&mut page, "l"), px(2.0), "in its first legend");
    assert_eq!(page.style("sp").height, px(3.0), "asked of an ancestor");
    assert_eq!(
        page.style("ii").height,
        px(4.0),
        "a sibling's, by the fieldset"
    );
    page.with("t", |cx, t| cx.remove_attribute(t, "disabled"));
    page.with("f", |cx, f| cx.remove_attribute(f, "disabled"));
    page.recalc();
    assert_eq!(
        (width(&mut page, "i"), width(&mut page, "t")),
        (px(2.0), px(2.0))
    );
}

#[test]
fn required_optional_read_write_read_only() {
    let mut page = Page::new(
        "<!doctype html><style>input { appearance: none } :required { width: 1px } :optional { width: 2px } \
         :read-write { height: 1px } :read-only { height: 2px }</style>\
         <input id=t required><input id=o><input type=hidden id=h required><input type=checkbox id=c>\
         <select id=s></select><textarea id=a></textarea><input id=r readonly><input id=d disabled><div id=v></div>",
    );
    assert_eq!(width(&mut page, "t"), px(1.0));
    assert_eq!(width(&mut page, "o"), px(2.0));
    assert_eq!(
        width(&mut page, "h"),
        Size::Auto,
        "`required` doesn't apply"
    );
    assert_eq!(width(&mut page, "c"), px(2.0));
    assert_eq!(width(&mut page, "s"), px(2.0));
    let height = |page: &mut Page, id: &str| page.style(id).height;
    assert_eq!(height(&mut page, "t"), px(1.0));
    assert_eq!(height(&mut page, "a"), px(1.0));
    assert_eq!(height(&mut page, "c"), px(2.0), "`readonly` doesn't apply");
    assert_eq!(height(&mut page, "r"), px(2.0));
    assert_eq!(height(&mut page, "d"), px(2.0), "disabled");
    assert_eq!(height(&mut page, "v"), px(2.0));
    page.with("o", |cx, o| cx.set_attribute(o, "required", ""));
    page.with("r", |cx, r| cx.remove_attribute(r, "readonly"));
    page.with("t", |cx, t| cx.set_attribute(t, "type", "range"));
    page.recalc();
    assert_eq!(width(&mut page, "o"), px(1.0));
    assert_eq!(height(&mut page, "r"), px(1.0));
    assert_eq!(
        (width(&mut page, "t"), height(&mut page, "t")),
        (Size::Auto, px(2.0)),
        "a range"
    );
}

/// An element that is editable or an editing host matches `:read-write`
/// (an `<input>` or `<textarea>` by its own state only); an ancestor's
/// `contenteditable` changing styles what is below it again.
#[test]
fn editable_elements_are_read_write() {
    let mut page = Page::new(
        "<!doctype html><style>input { appearance: none } :read-write { height: 1px } :read-only { height: 2px }</style>\
         <div id=host contenteditable><p id=p><span id=s>x</span></p><b id=off contenteditable=false>y</b>\
         <input id=r readonly></div><div id=plain><i id=i>z</i></div>\
         <div id=kept contenteditable style='overflow-wrap: normal'><i id=k>w</i></div>",
    );
    let height = |page: &mut Page, id: &str| page.style(id).height;
    for (id, want) in [
        ("host", 1.0),
        ("p", 1.0),
        ("s", 1.0),
        ("off", 2.0),
        ("r", 2.0),
        ("plain", 2.0),
        ("i", 2.0),
    ] {
        assert_eq!(height(&mut page, id), px(want), "{id}");
    }
    page.with("host", |cx, host| {
        cx.set_attribute(host, "contenteditable", "false")
    });
    page.with("plain", |cx, plain| {
        cx.set_attribute(plain, "contenteditable", "plaintext-only")
    });
    // What it inherits stays (its `overflow-wrap` is the rule's): the
    // attribute itself restyles what is below.
    page.with("kept", |cx, kept| {
        cx.set_attribute(kept, "contenteditable", "false")
    });
    page.recalc();
    for (id, want) in [
        ("host", 2.0),
        ("p", 2.0),
        ("s", 2.0),
        ("plain", 1.0),
        ("i", 1.0),
        ("kept", 2.0),
        ("k", 2.0),
    ] {
        assert_eq!(height(&mut page, id), px(want), "{id}");
    }
}

#[test]
fn placeholder_shown() {
    let mut page = Page::new(
        "<!doctype html><style>input { appearance: none } :placeholder-shown { width: 1px } :placeholder-shown + span { height: 3px }</style>\
         <input id=p placeholder=x><input id=v placeholder=x value=v><span id=vs>s</span><input id=n>\
         <input type=checkbox id=c placeholder=x value=\"\">",
    );
    assert_eq!(width(&mut page, "p"), px(1.0));
    assert_eq!(width(&mut page, "v"), Size::Auto);
    assert_eq!(width(&mut page, "n"), Size::Auto);
    assert_eq!(
        width(&mut page, "c"),
        Size::Auto,
        "`placeholder` doesn't apply"
    );
    // The value, set: not an attribute's change.
    page.with("p", |cx, p| cx.set_input_value(p, "typed").unwrap());
    page.recalc();
    assert_eq!(width(&mut page, "p"), Size::Auto);
    page.with("v", |cx, v| cx.set_attribute(v, "value", ""));
    page.recalc();
    assert_eq!(width(&mut page, "v"), px(1.0));
    assert_eq!(
        page.style("vs").height,
        px(3.0),
        "its value, for the sibling"
    );
    // Reset: the attribute's again.
    page.with("p", |cx, p| {
        let form = cx.create_element(cx.document_of(p), dom::QualName::html("form"));
        cx.append_child(cx.parent(p).unwrap(), form).unwrap();
        cx.append_child(form, p).unwrap();
    });
    page.recalc();
    assert_eq!(width(&mut page, "p"), Size::Auto);
    page.with("p", |cx, p| {
        cx.reset_form_controls(cx.form_owner(p).unwrap())
    });
    page.recalc();
    assert_eq!(width(&mut page, "p"), px(1.0));
}

#[test]
fn a_textareas_placeholder_shown() {
    let mut page = Page::new(
        "<!doctype html><style>input { appearance: none } :placeholder-shown { width: 1px } :placeholder-shown + span { height: 3px }</style>\
         <form><textarea id=t placeholder=x></textarea><span id=ts>s</span></form>",
    );
    assert_eq!(width(&mut page, "t"), px(1.0));
    assert_eq!(page.style("ts").height, px(3.0));
    // Its children's text, inserted, changed and removed: its value while
    // it isn't set (the sibling sees each).
    page.with("t", |cx, t| {
        let text = cx.create_text(t, "a");
        cx.append_child(t, text).unwrap();
    });
    page.recalc();
    assert_eq!(width(&mut page, "t"), Size::Auto);
    assert_eq!(page.style("ts").height, Size::Auto);
    page.with("t", |cx, t| cx.set_data(cx.first_child(t).unwrap(), ""));
    page.recalc();
    assert_eq!(page.style("ts").height, px(3.0), "a text child's data");
    page.with("t", |cx, t| cx.set_data(cx.first_child(t).unwrap(), "b"));
    page.recalc();
    assert_eq!(page.style("ts").height, Size::Auto);
    page.with("t", |cx, t| cx.remove(cx.first_child(t).unwrap()));
    page.recalc();
    assert_eq!(page.style("ts").height, px(3.0), "a child removed");
    // Set: its own, the children no longer.
    page.with("t", |cx, t| cx.set_textarea_value(t, "v"));
    page.recalc();
    assert_eq!(page.style("ts").height, Size::Auto);
    // Reset: its children's (none) again.
    page.with("t", |cx, t| {
        cx.reset_form_controls(cx.form_owner(t).unwrap())
    });
    page.recalc();
    assert_eq!(page.style("ts").height, px(3.0));
    page.with("t", |cx, t| {
        cx.set_range_text(t, "x", None, dom::SelectionMode::Preserve);
    });
    page.recalc();
    assert_eq!(page.style("ts").height, Size::Auto, "`setRangeText()`'s");
}

#[test]
fn default() {
    let mut page = Page::new(
        "<!doctype html><style>input { appearance: none } :default { width: 1px } :default + span { height: 3px }</style>\
         <form id=f><input type=checkbox id=c checked><span id=cs>s</span><input type=radio id=r><button type=button id=n>n</button>\
         <input type=submit id=s1><button id=s2>b</button></form><button id=lone>l</button>",
    );
    assert_eq!(width(&mut page, "c"), px(1.0));
    assert_eq!(width(&mut page, "r"), Size::Auto);
    assert_eq!(width(&mut page, "n"), Size::Auto);
    assert_eq!(
        (width(&mut page, "s1"), width(&mut page, "s2")),
        (px(1.0), Size::Auto),
        "the first submit button"
    );
    assert_eq!(width(&mut page, "lone"), Size::Auto, "no form");
    // A submit button before it.
    page.with("n", |cx, n| cx.set_attribute(n, "type", "submit"));
    page.recalc();
    assert_eq!(
        (width(&mut page, "n"), width(&mut page, "s1")),
        (px(1.0), Size::Auto)
    );
    // It leaves.
    page.with("n", |cx, n| cx.remove(n));
    page.recalc();
    assert_eq!(width(&mut page, "s1"), px(1.0));
    // A type: another.
    page.with("s1", |cx, s1| cx.set_attribute(s1, "type", "reset"));
    page.with("c", |cx, c| cx.remove_attribute(c, "checked"));
    page.recalc();
    assert_eq!(
        (width(&mut page, "s1"), width(&mut page, "s2")),
        (Size::Auto, px(1.0))
    );
    assert_eq!(width(&mut page, "c"), Size::Auto);
    assert_eq!(
        page.style("cs").height,
        Size::Auto,
        "its `checked`, for the sibling"
    );
    page.with("r", |cx, r| cx.set_attribute(r, "checked", ""));
    page.recalc();
    assert_eq!(width(&mut page, "r"), px(1.0));
}

#[test]
fn an_options_checked_default_and_disabled() {
    // The select a list box (no menu list's look): its options styled as
    // they are.
    let mut page = Page::new(
        "<!doctype html><style>select { appearance: none } :checked { width: 1px } :default { height: 2px } \
         optgroup:disabled { max-height: 3px } select { color: GrayText !important }</style>\
         <fieldset id=f><select id=s size=3><option id=a>a<option id=b selected>b<optgroup id=g><option id=c>c</optgroup>\
         </select></fieldset>",
    );
    assert_eq!(
        (width(&mut page, "a"), width(&mut page, "b")),
        (Size::Auto, px(1.0))
    );
    assert_eq!(page.style("b").height, px(2.0));
    // Its selectedness: the other unselected, styled again.
    page.with("a", |cx, a| cx.set_option_selected(a, true));
    page.recalc();
    assert_eq!(
        (width(&mut page, "a"), width(&mut page, "b")),
        (px(1.0), Size::Auto)
    );
    assert_eq!(page.style("b").height, px(2.0), "its attribute still");
    // Its `selected` attribute.
    page.with("c", |cx, c| cx.set_attribute(c, "selected", ""));
    page.recalc();
    assert_eq!(page.style("c").height, px(2.0));
    assert_eq!(
        (width(&mut page, "c"), width(&mut page, "a")),
        (px(1.0), Size::Auto),
        "not dirty: it selects it"
    );
    // `a`, dirty: its attribute doesn't.
    page.with("a", |cx, a| cx.set_attribute(a, "selected", ""));
    page.recalc();
    assert_eq!(
        (width(&mut page, "a"), page.style("a").height),
        (Size::Auto, px(2.0))
    );
    // Disabled by its select, the select's fieldset: the optgroup noted
    // (no rule of the UA's has one below a disabled select, and the
    // select's `color`, which the UA's `select:disabled` changes, is the
    // author's: nothing it inherits changes).
    let disabled = |page: &mut Page, id: &str| {
        page.style(id).max_height != computed_style::LengthPercentageOrNone::None
    };
    assert!(!disabled(&mut page, "g"));
    page.with("s", |cx, s| cx.set_attribute(s, "disabled", ""));
    page.recalc();
    assert!(disabled(&mut page, "g"));
    page.with("s", |cx, s| cx.remove_attribute(s, "disabled"));
    page.recalc();
    assert!(!disabled(&mut page, "g"));
    page.with("f", |cx, f| cx.set_attribute(f, "disabled", ""));
    page.recalc();
    assert!(disabled(&mut page, "g"));
}

#[test]
fn a_select_shown_as_a_list_box() {
    // `:-internal-list-box` is the UA stylesheet's own: its rule's
    // `overflow-y: scroll`.
    let mut page = Page::new(
        "<!doctype html><select id=s><option>a</select><select id=m multiple><option>b</select>",
    );
    let scrolls =
        |page: &mut Page, id: &str| page.style(id).overflow_y == computed_style::Overflow::Scroll;
    assert!(!scrolls(&mut page, "s"));
    assert!(scrolls(&mut page, "m"));
    page.with("s", |cx, s| cx.set_attribute(s, "size", "3"));
    page.with("m", |cx, m| cx.set_attribute(m, "size", "1"));
    page.recalc();
    assert!(scrolls(&mut page, "s"));
    assert!(
        !scrolls(&mut page, "m"),
        "`multiple` with `size=1`: a menu list"
    );
}

#[test]
fn a_selects_open_popup() {
    let mut page = Page::new(
        "<!doctype html><style>select { appearance: none } :open { width: 1px } :open + span { height: 2px }</style>\
         <select id=s><option>a</select><span id=n>n</span><details id=d open>d</details>",
    );
    assert_eq!(width(&mut page, "s"), Size::Auto);
    assert_eq!(width(&mut page, "d"), px(1.0), "an open details still");
    page.with("s", |cx, s| cx.set_select_popup_open(s, true));
    page.recalc();
    assert_eq!(width(&mut page, "s"), px(1.0));
    assert_eq!(page.style("n").height, px(2.0), "the sibling");
    page.with("s", |cx, s| cx.set_select_popup_open(s, false));
    page.recalc();
    assert_eq!(width(&mut page, "s"), Size::Auto);
}

/// A list box's end moved to its option of text `id` by the keys, with
/// Ctrl (`non_contiguous`) or not.
fn end<'gc>(cx: &dom::Cx<'_, 'gc>, select: dom::NodeRef<'gc>, id: &str, non_contiguous: bool) {
    let option = cx
        .list_of_options(select)
        .into_iter()
        .find(|o| cx.option_text(*o) == id)
        .unwrap();
    let mut state = cx.list_box_state(select);
    state.end = Some(cx.node_id(option));
    state.non_contiguous = non_contiguous;
    cx.set_list_box_state(select, state);
}

#[test]
fn a_list_boxs_multi_select_focus() {
    // The UA's `select:-internal-list-box:focus-visible
    // option:-internal-multi-select-focus` outline.
    let mut page =
        Page::new("<!doctype html><select id=s multiple><option id=a>a<option id=b>b</select>");
    page.with("s", |cx, s| {
        let document = cx.document_of(s);
        let window = cx
            .window_of(document)
            .unwrap_or_else(|| cx.create_window(document));
        cx.set_page_focused(window, true);
        cx.set_focused_element(document, Some(s), true);
    });
    page.recalc();
    let outlined = |page: &mut Page, id: &str| {
        page.style(id).outline_style
            != computed_style::OutlineStyle::Line(computed_style::BorderStyle::None)
    };
    assert!(!outlined(&mut page, "b"));
    page.with("s", |cx, s| end(cx, s, "b", true));
    page.recalc();
    assert!(outlined(&mut page, "b") && !outlined(&mut page, "a"));
    page.with("s", |cx, s| end(cx, s, "a", true));
    page.recalc();
    assert!(
        outlined(&mut page, "a") && !outlined(&mut page, "b"),
        "moved: both noted"
    );
    page.with("s", |cx, s| end(cx, s, "a", false));
    page.recalc();
    assert!(!outlined(&mut page, "a"));
}
