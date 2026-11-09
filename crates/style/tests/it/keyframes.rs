//! Which `@keyframes` an element's `animation-name` finds: by name, in its
//! tree scope and outward.

use dom::QualName;
use style_resolver::computed_value;

use crate::Page;

/// The first keyframe's declarations of the `@keyframes` `name` that
/// `#id` finds, if it finds one: what an animation of it there gives
/// `top` (the only property these keyframes have), its end filled from
/// the start. A host's animation is its shadow tree's `:host` rule's,
/// which finds that tree's keyframes first.
fn found(page: &mut Page, id: &str, name: &str) -> Option<String> {
    // The name as a longhand: one like `both` would be the shorthand's
    // fill mode.
    let animation = format!("animation: 1s -1s forwards; animation-name: {name}");
    page.with(id, |cx, element| match cx.shadow_root(element) {
        Some(root) => {
            let document = cx.document_of(element);
            let style = cx.create_element(document, QualName::html("style"));
            let text = cx.create_text(document, &format!(":host {{ {animation} }}"));
            cx.append_child(style, text).unwrap();
            cx.append_child(root, style).unwrap();
        }
        None => cx.set_attribute(element, "style", &animation),
    });
    page.recalc();
    let top = computed_value(&page.style(id), "top").expect("a longhand");
    page.with(id, |cx, element| match cx.shadow_root(element) {
        Some(root) => cx.remove(cx.last_child(root).unwrap()),
        None => {
            cx.remove_attribute(element, "style");
        }
    });
    page.recalc();
    (top != "auto").then(|| format!("top: {top};"))
}

#[test]
fn the_last_keyframes_of_a_name_wins() {
    let mut page = Page::new(
        "<style>@keyframes a { to { top: 1px } } @keyframes a { to { top: 2px } }
         @keyframes b { to { top: 3px } }</style>
         <style>@keyframes b { to { top: 4px } }
         @media not all { @keyframes a { to { top: 5px } } }
         @supports (not (display: block)) { @keyframes b { to { top: 6px } } }</style>
         <div id=t></div>",
    );
    assert_eq!(found(&mut page, "t", "a").as_deref(), Some("top: 2px;"));
    assert_eq!(found(&mut page, "t", "b").as_deref(), Some("top: 4px;"));
    assert_eq!(
        found(&mut page, "t", "A"),
        None,
        "names match case-sensitively"
    );
    assert_eq!(found(&mut page, "t", "c"), None);
}

#[test]
fn keyframes_are_found_in_the_tree_scope_and_outward() {
    let mut page = Page::with_shadows(
        "<style>@keyframes outer { to { top: 1px } } @keyframes both { to { top: 2px } }</style>
         <div id=host></div>",
        &[
            (
                "host",
                "<style>@keyframes both { to { top: 3px } } @keyframes inner { to { top: 4px } }\
                 </style><div id=inner-host></div><p id=in-shadow></p>",
            ),
            (
                "inner-host",
                "<style>@keyframes deepest { to { top: 5px } } @keyframes dup { to { top: 6px } }\
                 </style><style>@keyframes dup { to { top: 7px } }</style><p id=deep></p>",
            ),
        ],
    );
    assert_eq!(
        found(&mut page, "in-shadow", "both").as_deref(),
        Some("top: 3px;")
    );
    assert_eq!(
        found(&mut page, "in-shadow", "inner").as_deref(),
        Some("top: 4px;")
    );
    assert_eq!(
        found(&mut page, "in-shadow", "outer").as_deref(),
        Some("top: 1px;")
    );
    assert_eq!(
        found(&mut page, "deep", "inner").as_deref(),
        Some("top: 4px;")
    );
    assert_eq!(
        found(&mut page, "deep", "outer").as_deref(),
        Some("top: 1px;")
    );
    assert_eq!(
        found(&mut page, "deep", "deepest").as_deref(),
        Some("top: 5px;")
    );
    assert_eq!(
        found(&mut page, "deep", "dup").as_deref(),
        Some("top: 7px;"),
        "the later <style>'s"
    );
    assert_eq!(
        found(&mut page, "in-shadow", "deepest"),
        None,
        "not a scope inside"
    );
    // The host's own shadow tree's, for its `:host` rules, before its own
    // tree's.
    assert_eq!(
        found(&mut page, "host", "both").as_deref(),
        Some("top: 3px;")
    );
    assert_eq!(
        found(&mut page, "host", "inner").as_deref(),
        Some("top: 4px;")
    );
    assert_eq!(found(&mut page, "host", "deepest"), None);
}
