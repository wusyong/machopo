//! `element.style` and its `CSSStyleDeclaration` through their members:
//! the declarations are the element's one copy, the `style` attribute
//! serialized from them when read.

use std::rc::Rc;

use dom::{NodeRoot, ObjectRoot, QualName};
use dom_api::MemberKind;

use crate::{Arg, Ret, TestDom, This, call_on, get, set};

fn element(dom: &mut TestDom, style: Option<&str>) -> Rc<NodeRoot> {
    Rc::new(dom.mutate(|cx| {
        let doc = cx.create_document(true);
        let element = cx.create_element(doc, QualName::html("div"));
        if let Some(style) = style {
            cx.set_attribute(element, "style", style);
        }
        cx.root(element)
    }))
}

fn declaration(dom: &mut TestDom, element: &Rc<NodeRoot>) -> Rc<ObjectRoot> {
    match get(dom, element, "HTMLElement", "style") {
        Ret::Object(object) => Rc::new(object),
        other => panic!("{other:?}"),
    }
}

/// The interface of the declaration's member `name`: `CSSStyleProperties`'
/// own, or `CSSStyleDeclaration`'s.
pub(crate) fn interface_of(name: &str) -> &'static str {
    match name {
        "cssFloat" | "getter" | "setter" | "names" => "CSSStyleProperties",
        _ => "CSSStyleDeclaration",
    }
}

fn call(
    dom: &mut TestDom,
    style: &Rc<ObjectRoot>,
    name: &str,
    kind: MemberKind,
    args: &[Arg],
) -> Ret {
    call_on(
        dom,
        This::Object(style.clone()),
        interface_of(name),
        name,
        kind,
        args,
    )
    .0
    .unwrap()
}

fn string(ret: Ret) -> String {
    ret.string().to_string()
}

fn attribute(dom: &mut TestDom, element: &Rc<NodeRoot>) -> Option<String> {
    dom.read(|cx| {
        cx.get_attribute(element.get(cx.gc), "style")
            .map(|value| value.to_string())
    })
}

#[test]
fn the_declarations_come_from_the_attribute() {
    let mut dom = TestDom::new();
    let element = element(&mut dom, Some("color: red; margin-top: 2px !important"));
    let style = declaration(&mut dom, &element);
    assert!(dom.read(
        |cx| match (style.get(cx.gc), declaration_again(cx, &element)) {
            (dom::ObjectRef::CssStyleDeclaration(a), b) => elise::Gc::ptr_eq(a, b),
            _ => false,
        }
    ));
    assert!(matches!(
        call(&mut dom, &style, "length", MemberKind::Getter, &[]),
        Ret::U32(2)
    ));
    assert_eq!(
        string(call(
            &mut dom,
            &style,
            "item",
            MemberKind::Method,
            &[Arg::Number(1.0)]
        )),
        "margin-top"
    );
    assert_eq!(
        string(call(
            &mut dom,
            &style,
            "item",
            MemberKind::Method,
            &[Arg::Number(5.0)]
        )),
        "",
        "past the end"
    );
    let value = |dom: &mut TestDom, name| {
        string(call(
            dom,
            &style,
            "getPropertyValue",
            MemberKind::Method,
            &[Arg::Str(name)],
        ))
    };
    assert_eq!(value(&mut dom, "color"), "red");
    assert_eq!(
        string(call(
            &mut dom,
            &style,
            "getPropertyPriority",
            MemberKind::Method,
            &[Arg::Str("margin-top")]
        )),
        "important"
    );
    assert_eq!(
        string(call(&mut dom, &style, "cssText", MemberKind::Getter, &[])),
        "color: red; margin-top: 2px !important;"
    );
    // `setAttribute` replaces them.
    dom.mutate(|cx| cx.set_attribute(element.get(cx.gc), "style", "color: blue"));
    assert_eq!(value(&mut dom, "color"), "blue");
    assert_eq!(value(&mut dom, "margin-top"), "");
    dom.mutate(|cx| {
        cx.remove_attribute(element.get(cx.gc), "style");
    });
    assert_eq!(value(&mut dom, "color"), "", "none with no attribute");
}

fn declaration_again<'gc>(
    cx: &dom::Cx<'_, 'gc>,
    element: &Rc<NodeRoot>,
) -> elise::Gc<'gc, dom::CssStyleDeclaration> {
    cx.style_declaration(element.get(cx.gc))
}

#[test]
fn changes_write_the_attribute_when_it_is_read() {
    let mut dom = TestDom::new();
    let element = element(&mut dom, None);
    let style = declaration(&mut dom, &element);
    call(
        &mut dom,
        &style,
        "setProperty",
        MemberKind::Method,
        &[Arg::Str("color"), Arg::Str("red")],
    );
    // No attribute was written yet: it is written as it is read, added.
    assert!(dom.read(|cx| cx.attribute_count(element.get(cx.gc))) == 1);
    assert_eq!(
        attribute(&mut dom, &element).as_deref(),
        Some("color: red;")
    );
    call(
        &mut dom,
        &style,
        "setProperty",
        MemberKind::Method,
        &[Arg::Str("width"), Arg::Str("3px"), Arg::Str("important")],
    );
    assert_eq!(
        attribute(&mut dom, &element).as_deref(),
        Some("color: red; width: 3px !important;")
    );
    // An unknown property or a bad value changes nothing.
    call(
        &mut dom,
        &style,
        "setProperty",
        MemberKind::Method,
        &[Arg::Str("nonsense"), Arg::Str("1")],
    );
    call(
        &mut dom,
        &style,
        "setProperty",
        MemberKind::Method,
        &[Arg::Str("width"), Arg::Str("wide")],
    );
    assert_eq!(
        attribute(&mut dom, &element).as_deref(),
        Some("color: red; width: 3px !important;")
    );
    assert_eq!(
        string(call(
            &mut dom,
            &style,
            "removeProperty",
            MemberKind::Method,
            &[Arg::Str("color")]
        )),
        "red"
    );
    assert_eq!(
        attribute(&mut dom, &element).as_deref(),
        Some("width: 3px !important;")
    );
    // A null value removes it.
    call(
        &mut dom,
        &style,
        "setProperty",
        MemberKind::Method,
        &[Arg::Str("width"), Arg::Null],
    );
    assert_eq!(attribute(&mut dom, &element).as_deref(), Some(""));
    call_on(
        &mut dom,
        This::Object(style.clone()),
        "CSSStyleDeclaration",
        "cssText",
        MemberKind::Setter,
        &[Arg::Str("float: left")],
    )
    .0
    .unwrap();
    assert_eq!(
        string(call(&mut dom, &style, "cssFloat", MemberKind::Getter, &[])),
        "left"
    );
    // `[PutForwards=cssText]`.
    set(
        &mut dom,
        &element,
        "HTMLElement",
        "style",
        Arg::Str("color: green"),
    )
    .unwrap();
    assert_eq!(
        attribute(&mut dom, &element).as_deref(),
        Some("color: green;")
    );
}

#[test]
fn camel_cased_and_dashed_names_are_named_properties() {
    let mut dom = TestDom::new();
    let element = element(&mut dom, Some("background-color: red"));
    let style = declaration(&mut dom, &element);
    for name in ["backgroundColor", "background-color"] {
        assert_eq!(
            string(call(
                &mut dom,
                &style,
                "getter",
                MemberKind::Method,
                &[Arg::Str(name)]
            )),
            "red",
            "{name}"
        );
    }
    assert!(matches!(
        call(
            &mut dom,
            &style,
            "getter",
            MemberKind::Method,
            &[Arg::Str("nothing")]
        ),
        Ret::Null
    ));
    assert!(matches!(
        call(
            &mut dom,
            &style,
            "setter",
            MemberKind::Method,
            &[Arg::Str("marginLeft"), Arg::Str("4px")]
        ),
        Ret::Bool(true)
    ));
    assert!(matches!(
        call(
            &mut dom,
            &style,
            "setter",
            MemberKind::Method,
            &[Arg::Str("nothing"), Arg::Str("4px")]
        ),
        Ret::Bool(false)
    ));
    assert_eq!(
        attribute(&mut dom, &element).as_deref(),
        Some("background-color: red; margin-left: 4px;")
    );
}

/// A change takes the declarations out and puts them back: in place while
/// no one else holds them, a copy while the style phase does.
#[test]
fn a_change_copies_only_what_another_holds() {
    let mut dom = TestDom::new();
    let element = element(&mut dom, Some("color: red"));
    let style = declaration(&mut dom, &element);
    let held = dom.mutate(|cx| cx.inline_style(element.get(cx.gc)).unwrap());
    call(
        &mut dom,
        &style,
        "setProperty",
        MemberKind::Method,
        &[Arg::Str("color"), Arg::Str("blue")],
    );
    assert_eq!(
        held.property_value("color"),
        "red",
        "what was held is as it was"
    );
    let now = dom.mutate(|cx| cx.inline_style(element.get(cx.gc)).unwrap());
    assert!(!std::sync::Arc::ptr_eq(&held, &now));
    drop(held);
    let address = std::sync::Arc::as_ptr(&now);
    drop(now);
    call(
        &mut dom,
        &style,
        "setProperty",
        MemberKind::Method,
        &[Arg::Str("color"), Arg::Str("green")],
    );
    let again = dom.mutate(|cx| cx.inline_style(element.get(cx.gc)).unwrap());
    assert_eq!(std::sync::Arc::as_ptr(&again), address, "changed in place");
    assert_eq!(again.property_value("color"), "green");
}

/// The declarations parsed from the attribute are kept (the same until it
/// changes), and a member that changes nothing leaves the attribute as
/// written.
#[test]
fn parsed_declarations_are_kept() {
    let mut dom = TestDom::new();
    let element = element(&mut dom, Some("color:red"));
    let style = declaration(&mut dom, &element);
    let (a, b) = dom.mutate(|cx| {
        let element = element.get(cx.gc);
        (
            cx.inline_style(element).unwrap(),
            cx.inline_style(element).unwrap(),
        )
    });
    assert!(std::sync::Arc::ptr_eq(&a, &b), "parsed once");
    call(
        &mut dom,
        &style,
        "setProperty",
        MemberKind::Method,
        &[Arg::Str("nonsense"), Arg::Str("1")],
    );
    call(
        &mut dom,
        &style,
        "removeProperty",
        MemberKind::Method,
        &[Arg::Str("width")],
    );
    assert_eq!(
        attribute(&mut dom, &element).as_deref(),
        Some("color:red"),
        "as written"
    );
}

/// What `cssText` and `setProperty` set resolves its `url()`s against the
/// element's document's URL as it is set (the base URL asked only of a
/// value with one): none left to resolve after, either way.
#[test]
fn urls_set_resolve_against_the_document() {
    let mut dom = TestDom::new();
    let element = element(&mut dom, None);
    dom.mutate(|cx| {
        let document = cx.document_of(element.get(cx.gc));
        cx.set_document_url(document, "https://example.test/dir/page.html");
    });
    let style = declaration(&mut dom, &element);
    let unresolved = |dom: &mut TestDom| {
        dom.mutate(|cx| {
            cx.inline_style(element.get(cx.gc))
                .is_some_and(|block| block.has_unresolved_urls())
        })
    };
    call(
        &mut dom,
        &style,
        "cssText",
        MemberKind::Setter,
        &[Arg::Str("color: red; background-image: url(a.png)")],
    );
    assert!(!unresolved(&mut dom), "cssText's resolved");
    call(
        &mut dom,
        &style,
        "setProperty",
        MemberKind::Method,
        &[Arg::Str("cursor"), Arg::Str("url(c.png), auto")],
    );
    assert!(!unresolved(&mut dom), "setProperty's resolved");
    // One with none: as it was set.
    call(
        &mut dom,
        &style,
        "setProperty",
        MemberKind::Method,
        &[Arg::Str("width"), Arg::Str("10px")],
    );
    assert_eq!(
        string(call(
            &mut dom,
            &style,
            "getPropertyValue",
            MemberKind::Method,
            &[Arg::Str("width")]
        )),
        "10px"
    );
}
