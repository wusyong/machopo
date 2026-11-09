//! Form controls' style: the UA stylesheet's rules (the computed values
//! Chrome 152 gives each control, as `getComputedStyle` reads them; the
//! system colors are a web test's, the font family `system-ui`, which is
//! Chrome's own platform's there), and which native look `appearance`
//! comes to for each control and the author's styles.

use std::collections::BTreeMap;

use computed_style::{ComputedStyle, EffectiveAppearance};
use dom::{Cx, NodeKind, NodeRef, QualName, ShadowRootInit, ShadowRootMode, SlotAssignmentMode};
use style_resolver::computed_value;

use crate::{Page, focus, styles};

#[track_caller]
fn assert_computed(styles: &BTreeMap<String, ComputedStyle>, id: &str, expected: &[(&str, &str)]) {
    let style = &styles[id];
    for &(property, value) in expected {
        assert_eq!(
            computed_value(style, property).as_deref(),
            Some(value),
            "#{id} {property}"
        );
    }
}

#[test]
fn the_ua_rules_give_controls_chromes_style() {
    let s = styles(
        "<input id=text><input type=search id=search><input type=checkbox id=checkbox>
         <input type=radio id=radio><input type=range id=range><input type=color id=color>
         <input type=submit id=submit><button id=button>Go</button><textarea id=textarea></textarea>
         <select id=select><option>a</option></select><input type=date id=date>
         <input type=file id=file><input type=image id=image>
         <input type=hidden id=hidden><div id=div></div>",
    );
    // What every control has (but a check box's margins).
    for id in ["text", "checkbox", "button", "textarea", "select"] {
        assert_computed(
            &s,
            id,
            &[
                ("display", "inline-block"),
                ("font-size", "13.3333px"),
                ("font-weight", "400"),
                ("font-style", "normal"),
                ("line-height", "normal"),
                ("letter-spacing", "normal"),
                ("word-spacing", "0px"),
                ("text-transform", "none"),
                ("text-indent", "0px"),
            ],
        );
    }
    for id in ["text", "button", "textarea", "select"] {
        assert_computed(&s, id, &[("margin-top", "0px"), ("margin-left", "0px")]);
    }
    assert_computed(
        &s,
        "text",
        &[
            ("appearance", "auto"),
            ("font-family", "system-ui"),
            ("color", "rgb(0, 0, 0)"),
            ("background-color", "rgb(255, 255, 255)"),
            ("border-top-width", "2px"),
            ("border-top-style", "inset"),
            ("border-top-color", "rgb(118, 118, 118)"),
            ("border-left-width", "2px"),
            ("padding-top", "1px"),
            ("padding-left", "2px"),
            ("box-sizing", "content-box"),
            ("text-align", "start"),
            // Chrome's is `clip`, which machopo computes to `hidden`: none.
            ("overflow-x", "visible"),
        ],
    );
    assert_computed(&s, "search", &[("box-sizing", "border-box")]);
    assert_computed(
        &s,
        "checkbox",
        &[
            ("background-color", "rgba(0, 0, 0, 0)"),
            ("border-top-width", "0px"),
            ("border-top-style", "none"),
            ("padding-top", "0px"),
            ("margin-top", "3px"),
            ("margin-right", "3px"),
            ("margin-bottom", "3px"),
            ("margin-left", "4px"),
            ("box-sizing", "border-box"),
            ("overflow-x", "visible"),
        ],
    );
    assert_computed(
        &s,
        "radio",
        &[("margin-bottom", "0px"), ("margin-left", "5px")],
    );
    assert_computed(
        &s,
        "range",
        &[
            ("color", "rgb(16, 16, 16)"),
            ("border-top-style", "none"),
            ("padding-left", "0px"),
            ("margin-top", "2px"),
            ("margin-left", "2px"),
            ("overflow-x", "visible"),
        ],
    );
    assert_computed(
        &s,
        "color",
        &[
            ("background-color", "rgb(239, 239, 239)"),
            ("border-top-width", "1px"),
            ("border-top-style", "solid"),
        ],
    );
    for id in ["submit", "button"] {
        assert_computed(
            &s,
            id,
            &[
                ("color", "rgb(0, 0, 0)"),
                ("background-color", "rgb(239, 239, 239)"),
                ("border-top-width", "2px"),
                ("border-top-style", "outset"),
                ("border-top-color", "rgb(118, 118, 118)"),
                ("padding-top", "1px"),
                ("padding-left", "6px"),
                ("padding-right", "6px"),
                ("text-align", "center"),
                ("box-sizing", "border-box"),
            ],
        );
    }
    assert_computed(
        &s,
        "submit",
        &[
            ("white-space-collapse", "preserve"),
            ("text-wrap-mode", "nowrap"),
        ],
    );
    assert_computed(
        &s,
        "button",
        &[
            ("white-space-collapse", "collapse"),
            ("text-wrap-mode", "wrap"),
            ("overflow-x", "visible"),
        ],
    );
    assert_computed(
        &s,
        "textarea",
        &[
            ("font-family", "monospace"),
            ("background-color", "rgb(255, 255, 255)"),
            ("border-top-width", "1px"),
            ("border-top-style", "solid"),
            ("border-top-color", "rgb(118, 118, 118)"),
            ("padding-top", "2px"),
            ("padding-left", "2px"),
            ("box-sizing", "content-box"),
            ("white-space-collapse", "preserve"),
            ("text-wrap-mode", "wrap"),
            ("overflow-wrap", "break-word"),
            ("overflow-x", "auto"),
        ],
    );
    assert_computed(
        &s,
        "select",
        &[
            ("background-color", "rgb(255, 255, 255)"),
            ("border-top-width", "1px"),
            ("border-top-style", "solid"),
            ("padding-top", "0px"),
            ("box-sizing", "border-box"),
            ("align-items", "center"),
            ("white-space-collapse", "preserve"),
            ("text-wrap-mode", "nowrap"),
        ],
    );
    assert_computed(
        &s,
        "date",
        &[
            ("font-family", "monospace"),
            ("padding-top", "0px"),
            ("padding-left", "1px"),
            ("padding-right", "0px"),
        ],
    );
    for id in ["file", "image"] {
        assert_computed(
            &s,
            id,
            &[
                ("appearance", "none"),
                ("background-color", "rgba(0, 0, 0, 0)"),
                ("border-top-style", "none"),
                ("padding-top", "0px"),
            ],
        );
    }
    assert_computed(
        &s,
        "file",
        &[
            ("white-space-collapse", "preserve"),
            ("text-wrap-mode", "nowrap"),
            ("align-items", "baseline"),
        ],
    );
    assert_computed(&s, "hidden", &[("display", "none"), ("appearance", "none")]);
    assert_computed(&s, "div", &[("appearance", "none"), ("font-size", "16px")]);
}

/// A disabled control's colors, faded; a disabled select is half
/// transparent too, and its options gray.
#[test]
fn disabled_controls_are_faded() {
    let s = styles(
        "<input disabled id=disabled><button disabled id=button-disabled>x</button>
         <select disabled id=select-disabled><option id=option>A</option></select>",
    );
    assert_computed(
        &s,
        "disabled",
        &[
            ("color", "rgb(84, 84, 84)"),
            ("background-color", "rgba(239, 239, 239, 0.3)"),
            ("border-top-color", "rgba(118, 118, 118, 0.3)"),
        ],
    );
    assert_computed(
        &s,
        "button-disabled",
        &[
            ("color", "rgba(16, 16, 16, 0.3)"),
            ("background-color", "rgba(239, 239, 239, 0.3)"),
            ("border-top-color", "rgba(118, 118, 118, 0.3)"),
        ],
    );
    assert_computed(
        &s,
        "select-disabled",
        &[
            ("opacity", "0.7"),
            ("color", "rgb(128, 128, 128)"),
            ("border-top-color", "rgba(118, 118, 118, 0.3)"),
        ],
    );
    assert_computed(&s, "option", &[("color", "rgb(128, 128, 128)")]);
}

/// Each control's native look from `appearance: auto`, and what the other
/// keywords and the author's styles make of it.
#[test]
fn appearance_comes_to_each_controls_native_look() {
    use EffectiveAppearance::*;
    let s = styles(
        "<input id=text><input type=PASSWORD id=password><input type=bogus id=bogus>
         <input type=search id=search><input type=Checkbox id=checkbox><input type=radio id=radio>
         <input type=range id=range><input type=color id=color><input type=reset id=reset>
         <button id=button></button><textarea id=textarea></textarea>
         <select id=select></select><select size=1 id=size-1></select>
         <select size=' 3x' id=size-3></select><select multiple id=multiple></select>
         <select multiple size=1 id=multiple-1></select>
         <meter style='appearance: auto' id=meter></meter>
         <progress style='appearance: auto' id=progress></progress>
         <input type=file id=file><input type=image id=image><div id=div></div>
         <div style='appearance: auto' id=div-auto></div>
         <input style='appearance: none' id=none>
         <input type=checkbox style='appearance: radio' id=compat>
         <input type=search style='appearance: textfield' id=search-textfield>
         <input style='appearance: searchfield' id=text-searchfield>
         <select style='appearance: menulist-button' id=menulist-button></select>
         <input style='appearance: menulist-button' id=text-menulist-button>
         <select style='appearance: base-select' id=base-select></select>
         <input style='appearance: base-select' id=text-base-select>",
    );
    for (id, part) in [
        ("text", TextField),
        ("password", TextField),
        ("bogus", TextField),
        ("search", SearchField),
        ("checkbox", Checkbox),
        ("radio", Radio),
        ("range", SliderHorizontal),
        ("color", SquareButton),
        ("reset", PushButton),
        ("button", PushButton),
        ("textarea", TextArea),
        ("select", Menulist),
        ("size-1", Menulist),
        ("size-3", Listbox),
        ("multiple", Listbox),
        ("multiple-1", Menulist),
        ("meter", Meter),
        ("progress", ProgressBar),
        ("file", None),
        ("image", None),
        ("div", None),
        ("div-auto", None),
        ("none", None),
        // A compat keyword is `auto`.
        ("compat", Checkbox),
        ("search-textfield", TextField),
        ("text-searchfield", TextField),
        ("menulist-button", MenulistButton),
        ("text-menulist-button", TextField),
        ("base-select", None),
        ("text-base-select", TextField),
    ] {
        assert_eq!(s[id].effective_appearance, part, "#{id}");
    }
}

#[test]
fn an_authors_background_or_border_draws_a_control_as_css_does() {
    use EffectiveAppearance::*;
    let s = styles(
        "<style>
           .bg { background-image: none }
           .border { border-left-width: 2px }
           .radius { border-top-left-radius: 1px }
           .logical { border-inline-start-color: red }
           .important { background-color: red !important }
           .color { color: red; padding: 9px; box-shadow: 1px 1px red }
         </style>
         <button class=bg id=button-bg></button><input class=border id=text-border>
         <input type=search class=radius id=search-radius><textarea class=logical id=textarea-logical></textarea>
         <input type=color class=important id=color-important>
         <input type=reset style='background: none' id=reset-inline>
         <select class=bg id=select-bg></select><select class=bg multiple id=listbox-bg></select>
         <meter class=border style='appearance: auto' id=meter-border></meter>
         <progress class=bg style='appearance: auto' id=progress-bg></progress>
         <input type=checkbox class=bg id=checkbox-bg><input type=radio class=border id=radio-border>
         <input type=range class=bg id=range-bg><button class=color id=button-color></button>
         <input disabled id=disabled>",
    );
    for (id, part) in [
        ("button-bg", None),
        ("text-border", None),
        ("search-radius", None),
        ("textarea-logical", None),
        ("color-important", None),
        ("reset-inline", None),
        // A menu list keeps its arrow.
        ("select-bg", MenulistButton),
        ("listbox-bg", None),
        ("meter-border", None),
        ("progress-bg", None),
        // Check boxes, radio buttons and sliders stay native.
        ("checkbox-bg", Checkbox),
        ("radio-border", Radio),
        ("range-bg", SliderHorizontal),
        // Other properties don't count, nor do the UA's own (a disabled
        // control's colors).
        ("button-color", PushButton),
        ("disabled", TextField),
    ] {
        assert_eq!(s[id].effective_appearance, part, "#{id}");
    }
}

#[test]
fn a_pseudo_element_has_no_native_look() {
    let mut page = Page::new(
        "<style>button::before { content: 'x'; appearance: auto }</style><button id=b></button>",
    );
    let before = page
        .pseudo("b")
        .and_then(|pseudo| pseudo.before.clone())
        .expect("the button has a ::before");
    assert_eq!(before.effective_appearance, EffectiveAppearance::None);
    assert_eq!(
        page.style("b").effective_appearance,
        EffectiveAppearance::PushButton
    );
}

/// The UA rules' system colors are the platform's; a text field's border is
/// a fixed gray, not `ButtonBorder`.
#[test]
fn the_ua_rules_follow_the_platforms_system_colors() {
    let colors = gfx::SystemColors {
        button_face: gfx::Color::rgb(1, 0, 0),
        button_text: gfx::Color::rgb(2, 0, 0),
        button_border: gfx::Color::rgb(3, 0, 0),
        field: gfx::Color::rgb(4, 0, 0),
        field_text: gfx::Color::rgb(5, 0, 0),
        ..gfx::SystemColors::DEFAULT
    };
    let mut page = Page::parsed("<input id=text><button id=button></button>");
    page.engine.set_system_colors(colors);
    page.recalc();
    let s = owned(&mut page);
    assert_computed(
        &s,
        "text",
        &[
            ("color", "rgb(5, 0, 0)"),
            ("background-color", "rgb(4, 0, 0)"),
            ("border-top-color", "rgb(118, 118, 118)"),
        ],
    );
    assert_computed(
        &s,
        "button",
        &[
            ("color", "rgb(2, 0, 0)"),
            ("background-color", "rgb(1, 0, 0)"),
            ("border-top-color", "rgb(3, 0, 0)"),
        ],
    );
}

/// A check box's and a radio button's `auto` sizes are the theme's 13px;
/// one drawn as CSS (`appearance: none`) has none.
#[test]
fn check_boxes_and_radio_buttons_are_the_themes_size() {
    use computed_style::{LengthPercentage, Size};
    let s = styles(
        "<input type=checkbox id=checkbox><input type=radio id=radio>
         <input type=checkbox style='width: 20px' id=wide>
         <input type=radio style='height: 50%' id=tall>
         <input type=checkbox style='appearance: none' id=none>
         <input type=range id=range>",
    );
    let px = |v| Size::LengthPercentage(LengthPercentage::Px(v));
    for id in ["checkbox", "radio"] {
        assert_eq!((s[id].width, s[id].height), (px(13.0), px(13.0)), "#{id}");
    }
    assert_eq!((s["wide"].width, s["wide"].height), (px(20.0), px(13.0)));
    assert_eq!(
        (s["tall"].width, s["tall"].height),
        (
            px(13.0),
            Size::LengthPercentage(LengthPercentage::Percent(50.0))
        )
    );
    assert_eq!(
        (s["none"].width, s["none"].height),
        (Size::Auto, Size::Auto)
    );
    assert_eq!(
        (s["range"].width, s["range"].height),
        (Size::Auto, Size::Auto)
    );
}

/// An element with an `appearance` is no table part and not `inline`
/// (Chrome 152): `table` and `list-item` are `block`, `inline`,
/// `inline-table` and the table-internal values `inline-block`; a control
/// drawn as CSS keeps its own, and so does one styled by the author, whose
/// `appearance` is still `auto`.
#[test]
fn what_has_an_appearance_is_no_table_part() {
    let s = styles(
        "<input style='display: table' id=table><select style='display: table-cell' id=cell></select>
         <textarea style='display: inline-table' id=inline-table></textarea>
         <button style='display: inline' id=inline></button>
         <select style='display: list-item' id=list-item></select>
         <input style='display: table-caption; border: 0' id=styled>
         <span style='appearance: auto' id=span></span>
         <input style='display: flex' id=flex>
         <input type=file style='display: table-caption' id=file>
         <input style='display: table-cell; appearance: none' id=none>
         <div style='display: table-cell' id=div></div>",
    );
    for (id, display) in [
        ("table", "block"),
        ("cell", "inline-block"),
        ("inline-table", "inline-block"),
        ("inline", "inline-block"),
        ("list-item", "block"),
        ("styled", "inline-block"),
        ("span", "inline-block"),
        ("flex", "flex"),
        ("file", "table-caption"),
        ("none", "table-cell"),
        ("div", "table-cell"),
    ] {
        assert_computed(&s, id, &[("display", display)]);
    }
}

/// A change to the DOM between two recalcs, given the controls by `id`
/// (and the document as `#document`).
type Change<'a> = &'a dyn for<'gc> Fn(&Cx<'_, 'gc>, &BTreeMap<String, NodeRef<'gc>>);

/// `html`'s text controls with an `id`, each given a shadow tree of
/// `mode` with a placeholder element (`pseudo="-webkit-input-placeholder"`,
/// as a control's UA shadow tree has it), and the placeholders' computed
/// styles by their control's `id` after `change` (the first recalc's,
/// without one).
fn placeholder_styles_after(
    html: &str,
    mode: ShadowRootMode,
    change: Option<Change<'_>>,
) -> BTreeMap<String, ComputedStyle> {
    part_styles_after(html, mode, "-webkit-input-placeholder", change)
}

/// [`placeholder_styles_after`] for the element of shadow pseudo id `id`.
/// A shadow tree that isn't the UA's gives its elements `appearance: auto`
/// (its own sheet), as an author's might.
fn part_styles_after(
    html: &str,
    mode: ShadowRootMode,
    id: &str,
    change: Option<Change<'_>>,
) -> BTreeMap<String, ComputedStyle> {
    /// The controls with an id in `node`'s subtree.
    fn controls<'gc>(
        cx: &Cx<'_, 'gc>,
        node: NodeRef<'gc>,
        out: &mut BTreeMap<String, NodeRef<'gc>>,
    ) {
        if cx.kind(node) == NodeKind::Element
            && ["INPUT", "TEXTAREA", "SELECT", "OPTGROUP"]
                .contains(&&*cx.node_name(node).to_string())
            && let Some(id) = crate::id_of(cx, node)
        {
            out.insert(id, node);
        }
        let mut child = cx.first_child(node);
        while let Some(at) = child {
            controls(cx, at, out);
            child = cx.next_sibling(at);
        }
    }
    let mut page = Page::parsed(html);
    let placeholders: BTreeMap<String, elise::Persistent<dom::Node>> =
        page.with_document(|cx, document| {
            let mut all = BTreeMap::new();
            controls(cx, document, &mut all);
            all.into_iter()
                .map(|(key, control)| {
                    let root = cx.attach_shadow(
                        control,
                        ShadowRootInit {
                            mode,
                            delegates_focus: false,
                            slot_assignment: SlotAssignmentMode::Named,
                            clonable: false,
                            serializable: false,
                        },
                    );
                    let placeholder = cx.create_element(document, QualName::html("div"));
                    cx.set_attribute(placeholder, "pseudo", id);
                    cx.append_child(root, placeholder).unwrap();
                    if mode != ShadowRootMode::UserAgent {
                        let sheet = cx.create_element(document, QualName::html("style"));
                        let css = cx.create_text(document, "div { appearance: auto }");
                        cx.append_child(sheet, css).unwrap();
                        cx.append_child(root, sheet).unwrap();
                    }
                    (key, elise::Persistent::new(cx.gc, placeholder))
                })
                .collect()
        });
    page.recalc();
    if let Some(change) = change {
        page.with_document(|cx, document| {
            let mut all = BTreeMap::new();
            controls(cx, document, &mut all);
            all.insert("#document".into(), document);
            change(cx, &all);
        });
        page.recalc();
    }
    let Page { dom, token, .. } = &mut page;
    dom.read(|cx| {
        placeholders
            .iter()
            .map(|(id, placeholder)| {
                let style = cx
                    .computed_style(placeholder.get(cx.gc), token)
                    .expect("styled");
                (id.clone(), (**style).clone())
            })
            .collect()
    })
}

fn placeholder_styles(html: &str, mode: ShadowRootMode) -> BTreeMap<String, ComputedStyle> {
    placeholder_styles_after(html, mode, None)
}

/// A UA shadow tree's placeholder takes the UA's `::placeholder` rules
/// (Chrome 152's computed values) and the author's for its control,
/// either name, the control matched as the element the pseudo-element is
/// of; a password field's text is masked, the author can't unmask it.
#[test]
fn the_placeholder_takes_its_controls_placeholder_rules() {
    let s = placeholder_styles(
        "<style>
           #red::placeholder { color: red }
           .form input::-webkit-input-placeholder { background-color: blue }
           #other::placeholder { color: green }
           input { -webkit-text-security: square }
         </style>
         <input id=plain style='font-size: 20px; text-align: right'>
         <input id=red><div class=form><input id=in-form></div>
         <input type=password id=password>
         <textarea id=area></textarea>",
        ShadowRootMode::UserAgent,
    );
    assert_computed(
        &s,
        "plain",
        &[
            ("color", "rgb(117, 117, 117)"),
            ("display", "block"),
            ("overflow-x", "hidden"),
            ("white-space-collapse", "preserve"),
            ("text-wrap-mode", "nowrap"),
            ("overflow-wrap", "normal"),
            ("pointer-events", "none"),
            ("line-height", "normal"),
            ("-webkit-text-security", "none"),
            // Inherited from the control.
            ("font-size", "20px"),
            ("text-align", "right"),
        ],
    );
    assert_computed(
        &s,
        "red",
        &[
            ("color", "rgb(255, 0, 0)"),
            ("background-color", "rgba(0, 0, 0, 0)"),
        ],
    );
    assert_computed(
        &s,
        "in-form",
        &[
            ("color", "rgb(117, 117, 117)"),
            ("background-color", "rgb(0, 0, 255)"),
        ],
    );
    assert_computed(
        &s,
        "area",
        &[
            ("white-space-collapse", "preserve"),
            ("text-wrap-mode", "wrap"),
            ("overflow-wrap", "break-word"),
            ("overflow-x", "visible"),
        ],
    );
    let controls = styles(
        "<style>input { -webkit-text-security: square }</style><input type=password id=password><input id=text>",
    );
    assert_computed(&controls, "password", &[("-webkit-text-security", "disc")]);
    assert_computed(&controls, "text", &[("-webkit-text-security", "square")]);
}

/// The same element in a shadow tree that isn't the UA's is no
/// pseudo-element: no `::placeholder` rule matches it.
#[test]
fn only_a_ua_shadow_trees_element_is_a_placeholder() {
    let s = placeholder_styles(
        "<style>#red::placeholder { color: red }</style><input id=red>",
        ShadowRootMode::Open,
    );
    // The control's own color, inherited.
    assert_computed(
        &s,
        "red",
        &[("color", "rgb(0, 0, 0)"), ("overflow-x", "visible")],
    );
}

/// The control styled again (an attribute changed) styles its placeholder
/// again.
#[test]
fn the_placeholder_follows_its_control() {
    fn change<'gc>(cx: &Cx<'_, 'gc>, controls: &BTreeMap<String, NodeRef<'gc>>) {
        cx.set_attribute(controls["a"], "class", "on");
    }
    let s = placeholder_styles_after(
        "<style>input.on::placeholder { color: lime }</style><input id=a><input id=b>",
        ShadowRootMode::UserAgent,
        Some(&change),
    );
    assert_computed(&s, "a", &[("color", "rgb(0, 255, 0)")]);
    assert_computed(&s, "b", &[("color", "rgb(117, 117, 117)")]);
}

/// A part's rule that looks at its host's earlier siblings: a change of
/// one styles the part again too (the sibling invalidation sets don't
/// cover parts; it takes what children coming or going would).
#[test]
fn a_parts_rule_across_its_hosts_siblings_styles_it_again() {
    fn change<'gc>(cx: &Cx<'_, 'gc>, controls: &BTreeMap<String, NodeRef<'gc>>) {
        cx.set_attribute(controls["a"], "class", "on");
    }
    let s = placeholder_styles_after(
        "<style>.on ~ input::placeholder { color: lime }</style><div><input id=a><input id=b></div>",
        ShadowRootMode::UserAgent,
        Some(&change),
    );
    assert_computed(&s, "b", &[("color", "rgb(0, 255, 0)")]);
}

/// A rule for a UA shadow tree's part that a changed sheet brings styles
/// the part again (its control's subtree, the shadow tree in it).
#[test]
fn a_changed_sheet_styles_the_parts_its_rules_match() {
    fn change<'gc>(cx: &Cx<'_, 'gc>, controls: &BTreeMap<String, NodeRef<'gc>>) {
        fn sheet<'gc>(cx: &Cx<'_, 'gc>, node: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
            if cx.is_style_element(node) {
                return Some(node);
            }
            let mut child = cx.first_child(node);
            while let Some(at) = child {
                if let Some(found) = sheet(cx, at) {
                    return Some(found);
                }
                child = cx.next_sibling(at);
            }
            None
        }
        let text = sheet(cx, controls["#document"])
            .and_then(|style| cx.first_child(style))
            .expect("the sheet's text");
        cx.set_data(text, "#a::placeholder { color: lime }");
    }
    let s = placeholder_styles_after(
        "<style>/* */</style><input id=a><input id=b>",
        ShadowRootMode::UserAgent,
        Some(&change),
    );
    assert_computed(&s, "a", &[("color", "rgb(0, 255, 0)")]);
    assert_computed(&s, "b", &[("color", "rgb(117, 117, 117)")]);
}

/// A menu list's inner element takes its select's look and alignment,
/// which aren't inherited: selects alike but for those don't share their
/// inner elements' styles (the matched properties cache doesn't keep them).
#[test]
fn a_menu_lists_inner_element_follows_its_own_select() {
    let s = part_styles_after(
        "<select id=a></select><select id=b></select><select id=c style='appearance: none'></select>",
        ShadowRootMode::UserAgent,
        "-internal-menulist-inner",
        None,
    );
    assert_computed(&s, "b", &[("margin-left", "4px"), ("margin-right", "16px")]);
    assert_computed(&s, "c", &[("margin-left", "0px"), ("margin-right", "0px")]);
}

/// A search field's clear button and a number field's spin button (their
/// UA shadow trees' parts) have their native look, and the theme's size
/// where theirs is `auto`: a clear button a square of its font size
/// (Chrome 152's), a spin button 15px wide. They show (`opacity`) while
/// the field can be edited and has the pointer or the focus.
#[test]
fn the_parts_have_their_native_look_and_size() {
    // An editable field is enabled and read-write: the DOM's states of an
    // `<input>`.
    fn hover<'gc>(cx: &Cx<'_, 'gc>, controls: &BTreeMap<String, NodeRef<'gc>>) {
        cx.set_hover_element(controls["#document"], Some(controls["hovered"]));
    }
    let html = "<style>#none::-webkit-search-cancel-button { appearance: none }
                  #given::-webkit-search-cancel-button { width: 3px }</style>
         <input type=search id=plain><input type=search id=big style='font-size: 40px'>
         <input type=search id=small style='font-size: 6px'><input type=search id=none>
         <input type=search id=given><input type=search id=hovered>";
    let s = part_styles_after(
        html,
        ShadowRootMode::UserAgent,
        "-webkit-search-cancel-button",
        Some(&hover),
    );
    assert_eq!(
        s["plain"].effective_appearance,
        EffectiveAppearance::SearchFieldCancelButton
    );
    assert_computed(
        &s,
        "plain",
        &[
            ("width", "12px"),
            ("height", "12px"),
            ("opacity", "0"),
            ("display", "block"),
            ("margin-left", "1px"),
            ("flex-shrink", "0"),
        ],
    );
    assert_computed(&s, "big", &[("width", "24px"), ("height", "24px")]);
    assert_computed(&s, "small", &[("width", "8px")]);
    assert_eq!(s["none"].effective_appearance, EffectiveAppearance::None);
    assert_computed(&s, "none", &[("width", "auto")]);
    assert_computed(&s, "given", &[("width", "3px"), ("height", "12px")]);
    assert_computed(
        &s,
        "hovered",
        &[("opacity", "1"), ("pointer-events", "auto")],
    );

    let html = "<input type=number id=plain><input type=number id=hovered>
         <input type=number id=disabled disabled><input type=number id=readonly readonly>";
    fn hover_all<'gc>(cx: &Cx<'_, 'gc>, controls: &BTreeMap<String, NodeRef<'gc>>) {
        hover(cx, controls);
        for id in ["disabled", "readonly"] {
            cx.set_hover_element(controls["#document"], Some(controls[id]));
        }
    }
    let s = part_styles_after(
        html,
        ShadowRootMode::UserAgent,
        "-webkit-inner-spin-button",
        Some(&hover),
    );
    assert_eq!(
        s["plain"].effective_appearance,
        EffectiveAppearance::InnerSpinButton
    );
    assert_computed(
        &s,
        "plain",
        &[
            ("width", "15px"),
            ("height", "auto"),
            ("align-self", "stretch"),
            ("opacity", "0"),
        ],
    );
    assert_computed(&s, "hovered", &[("opacity", "1")]);
    let s = part_styles_after(
        html,
        ShadowRootMode::UserAgent,
        "-webkit-inner-spin-button",
        Some(&hover_all),
    );
    // Only one element has the pointer: the last one asked, a field that
    // can't be edited.
    assert_computed(&s, "readonly", &[("opacity", "0")]);
    // A range's thumb: 16px square.
    let s = part_styles_after(
        "<input type=range id=thumb><input type=range id=small style='font-size: 6px'>",
        ShadowRootMode::UserAgent,
        "-webkit-slider-thumb",
        None,
    );
    assert_eq!(
        s["thumb"].effective_appearance,
        EffectiveAppearance::SliderThumbHorizontal
    );
    assert_computed(
        &s,
        "thumb",
        &[("width", "16px"), ("height", "16px"), ("display", "block")],
    );
    assert_computed(&s, "small", &[("width", "16px")]);
    // In a shadow tree that isn't the UA's, no part, whatever its
    // `appearance`.
    let s = part_styles_after(
        html,
        ShadowRootMode::Open,
        "-webkit-inner-spin-button",
        None,
    );
    assert_eq!(s["plain"].effective_appearance, EffectiveAppearance::None);
    assert_computed(&s, "plain", &[("appearance", "auto"), ("width", "auto")]);
}

/// The UA shadow trees' inner parts take their style from the UA
/// stylesheet by their shadow pseudo ids (no selector of a page's names
/// them): a text field's inner editor a block whose line doesn't wrap and
/// is cut at its edge, a text area's a block wrapped as the text area
/// says; the room a decoration container leaves its part; a menu list's
/// inner element a flex item that may be narrower than its text; an
/// optgroup's label a row of a list box.
#[test]
fn the_inner_parts_take_the_ua_stylesheets_style() {
    let html = "<input id=field style='text-overflow: ellipsis'><textarea id=area></textarea>
         <select id=select style='text-overflow: ellipsis'></select>
         <select multiple><optgroup id=group label=G></optgroup></select>";
    let editor = part_styles_after(
        html,
        ShadowRootMode::UserAgent,
        "-internal-inner-editor",
        None,
    );
    assert_computed(
        &editor,
        "field",
        &[
            ("display", "block"),
            ("white-space-collapse", "preserve"),
            ("text-wrap-mode", "nowrap"),
            ("overflow-wrap", "normal"),
            ("overflow-x", "hidden"),
            ("text-overflow", "ellipsis"),
        ],
    );
    assert_computed(
        &editor,
        "area",
        &[
            ("display", "block"),
            ("text-wrap-mode", "wrap"),
            ("overflow-x", "visible"),
        ],
    );
    let view_port = part_styles_after(
        html,
        ShadowRootMode::UserAgent,
        "-internal-editing-view-port",
        None,
    );
    assert_computed(
        &view_port,
        "field",
        &[
            ("display", "block"),
            ("flex-grow", "1"),
            ("min-width", "0px"),
        ],
    );
    let inner = part_styles_after(
        html,
        ShadowRootMode::UserAgent,
        "-internal-menulist-inner",
        None,
    );
    assert_computed(
        &inner,
        "select",
        &[
            ("display", "block"),
            ("flex-grow", "1"),
            ("flex-shrink", "1"),
            ("flex-basis", "auto"),
            ("min-width", "0px"),
            ("overflow-x", "hidden"),
            ("text-overflow", "ellipsis"),
        ],
    );
    let label = part_styles_after(
        html,
        ShadowRootMode::UserAgent,
        "-internal-optgroup-label",
        None,
    );
    assert_computed(
        &label,
        "group",
        &[
            ("padding-left", "2px"),
            ("padding-bottom", "1px"),
            ("min-height", "16px"),
        ],
    );
    // A page's element of those ids in an open shadow tree takes none.
    let open = part_styles_after(html, ShadowRootMode::Open, "-internal-inner-editor", None);
    assert_computed(&open, "field", &[("white-space-collapse", "collapse")]);
}

/// A menu list's inner element has its native look's room (4px left of
/// its text, 16px right, 1px above and below; none without a native look,
/// a styled one's too) and `auto` block margins while the menu list
/// centers it; the menu list's own `line-height` is `normal` with a
/// native look (Blink's `MenuListInnerElement` and `LayoutTheme::
/// AdjustMenuListStyle`).
#[test]
fn a_menu_lists_inner_element_has_its_room() {
    let html = "<select id=plain style='line-height: 30px'></select>
         <select id=none style='appearance: none; line-height: 30px'></select>
         <select id=styled style='background: red'></select>
         <select id=start style='align-items: flex-start'></select>";
    let s = part_styles_after(
        html,
        ShadowRootMode::UserAgent,
        "-internal-menulist-inner",
        None,
    );
    for id in ["plain", "styled"] {
        assert_computed(
            &s,
            id,
            &[
                ("margin-left", "4px"),
                ("margin-right", "16px"),
                ("padding-top", "1px"),
                ("padding-bottom", "1px"),
                ("margin-top", "auto"),
                ("margin-bottom", "auto"),
                ("align-self", "start"),
            ],
        );
    }
    assert_computed(
        &s,
        "none",
        &[
            ("margin-left", "0px"),
            ("margin-right", "0px"),
            ("padding-top", "0px"),
            ("margin-top", "auto"),
            ("line-height", "30px"),
        ],
    );
    assert_computed(
        &s,
        "start",
        &[
            ("margin-left", "4px"),
            ("margin-top", "0px"),
            ("align-self", "auto"),
        ],
    );
    // An open shadow tree's element of that id has no room.
    let open = part_styles_after(html, ShadowRootMode::Open, "-internal-menulist-inner", None);
    assert_computed(
        &open,
        "plain",
        &[("margin-left", "0px"), ("padding-top", "0px")],
    );
    let selects = styles(
        "<select id=plain style='line-height: 30px'></select>
         <select id=none style='appearance: none; line-height: 30px'></select>
         <select id=styled style='background: red; line-height: 30px'></select>",
    );
    assert_computed(&selects, "plain", &[("line-height", "normal")]);
    assert_computed(&selects, "styled", &[("line-height", "normal")]);
    assert_computed(&selects, "none", &[("line-height", "30px")]);
}

/// A menu list's text keeps its white space (`pre`), a list box's doesn't
/// wrap (`nowrap`: `:-internal-list-box`, `multiple` or a `size` above 1,
/// but `multiple` with `size=1`).
#[test]
fn the_ua_rules_tell_menu_lists_from_list_boxes() {
    let s = styles(
        "<select id=menu></select><select id=size-1 size=1></select>
         <select id=size-4 size=4></select><select id=multiple multiple></select>
         <select id=multiple-1 multiple size=1></select><select id=size-x size=x></select>
         <select id=disabled disabled><option id=option>A</option></select>",
    );
    for id in ["menu", "size-1", "multiple-1", "size-x"] {
        assert_computed(
            &s,
            id,
            &[
                ("white-space-collapse", "preserve"),
                ("text-wrap-mode", "nowrap"),
            ],
        );
    }
    for id in ["size-4", "multiple"] {
        assert_computed(
            &s,
            id,
            &[
                ("white-space-collapse", "collapse"),
                ("text-wrap-mode", "nowrap"),
            ],
        );
    }
}

/// A list box's options (Chrome's UA rules): blocks of `padding: 0 2px
/// 1px`, a line high at least, `line-height: normal` whatever the list
/// box's, indented 20px in a group; a selected one gray, and while the
/// list box has the focus `SelectedItem`; gray text in a disabled one; an
/// `<hr>` without a line. The list box: rounded, scrolled up and down.
#[test]
fn a_list_boxs_options_show_their_selection() {
    let mut page = Page::parsed(
        "<select id=list multiple style='line-height: 30px'><option id=plain>A</option>
           <option id=selected>B</option><optgroup id=group label=G><option id=grouped>C</option>
           </optgroup><hr id=hr></select>
         <select id=focused multiple><option id=focused-selected>D</option></select>
         <select id=disabled multiple disabled><option id=disabled-selected>E</option></select>
         <select id=menu><option id=menu-option>F</option></select>",
    );
    for id in ["selected", "focused-selected", "disabled-selected"] {
        page.with(id, |cx, option| cx.set_option_selected(option, true));
    }
    page.with("focused", |cx, list| {
        focus(cx, cx.document_of(list), Some(list), true, false)
    });
    page.recalc();
    let s = owned(&mut page);
    assert_computed(
        &s,
        "list",
        &[
            ("border-top-left-radius", "2px"),
            ("overflow-x", "hidden"),
            ("overflow-y", "scroll"),
            ("align-items", "flex-start"),
            ("vertical-align", "text-bottom"),
        ],
    );
    assert_computed(
        &s,
        "plain",
        &[
            ("display", "block"),
            ("padding-top", "0px"),
            ("padding-left", "2px"),
            ("padding-bottom", "1px"),
            ("min-height", "16px"),
            ("line-height", "normal"),
            ("font-weight", "400"),
            ("background-color", "rgba(0, 0, 0, 0)"),
        ],
    );
    assert_computed(
        &s,
        "selected",
        &[
            ("background-color", "rgb(206, 206, 206)"),
            ("color", "rgb(16, 16, 16)"),
        ],
    );
    assert_computed(
        &s,
        "group",
        &[("font-weight", "700"), ("line-height", "normal")],
    );
    assert_computed(
        &s,
        "grouped",
        &[("padding-left", "20px"), ("font-weight", "400")],
    );
    assert_computed(
        &s,
        "hr",
        &[
            ("border-top-style", "none"),
            ("margin-top", "6.66667px"),
            ("margin-bottom", "0px"),
        ],
    );
    assert_computed(
        &s,
        "focused-selected",
        &[
            ("background-color", "rgb(25, 103, 210)"),
            ("color", "rgb(255, 255, 255)"),
        ],
    );
    assert_computed(
        &s,
        "disabled-selected",
        &[
            ("background-color", "rgb(206, 206, 206)"),
            ("color", "rgb(128, 128, 128)"),
        ],
    );
    // A menu list's options are no list box's.
    assert_computed(
        &s,
        "menu-option",
        &[("background-color", "rgba(0, 0, 0, 0)")],
    );
}

/// The option of a `multiple` list box the keys moved to without
/// selecting it (`:-internal-multi-select-focus`) has a focus ring, inside
/// it, while the list box shows its focus (`:focus-visible`); not after a
/// press focused it.
#[test]
fn a_list_boxs_multi_select_focus_has_a_ring() {
    for visible in [true, false] {
        let mut page = Page::parsed(
            "<select id=list multiple><option id=focus>A</option><option id=other>B</option></select>",
        );
        page.with("list", |cx, list| {
            // The keys moved the list box's end to it, with Ctrl: not
            // selected.
            let option = cx.first_child(list).unwrap();
            let mut state = cx.list_box_state(list);
            state.end = Some(cx.node_id(option));
            state.non_contiguous = true;
            cx.set_list_box_state(list, state);
            focus(cx, cx.document_of(list), Some(list), true, visible);
        });
        page.recalc();
        let s = owned(&mut page);
        let ring = if visible { "auto" } else { "none" };
        assert_computed(&s, "focus", &[("outline-style", ring)]);
        assert_computed(&s, "other", &[("outline-style", "none")]);
        if visible {
            assert_computed(&s, "focus", &[("outline-offset", "-1px")]);
        }
    }
}

/// The page's styles, owned.
fn owned(page: &mut Page) -> BTreeMap<String, ComputedStyle> {
    page.styles()
        .into_iter()
        .map(|(id, style)| (id, (*style).clone()))
        .collect()
}
