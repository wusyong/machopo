//! `getComputedStyle` through a frame's layout of the tests' (`Fake`): a
//! new, live declaration each call; its values brought up to date first
//! (the style only, but for what layout decides, from the element's box);
//! the pseudo-element it is for; read only.

use std::rc::Rc;

use dom::{NodeRoot, ObjectRef, ObjectRoot};
use dom_api::{ApiError, MemberKind};

use crate::exception;
use crate::geometry::{block, fake, id, page};
use crate::{Arg, Ret, TestDom, This, call_on};

/// The document's window, made now.
fn window(dom: &mut TestDom, nodes: &[Rc<NodeRoot>]) -> Rc<ObjectRoot> {
    let document = nodes[0].clone();
    Rc::new(dom.mutate(|cx| {
        let window = cx.create_window(document.get(cx.gc));
        cx.root_object(ObjectRef::Window(window))
    }))
}

/// `window.getComputedStyle(element, pseudo)`.
fn computed(
    dom: &mut TestDom,
    window: &Rc<ObjectRoot>,
    element: &Rc<NodeRoot>,
    pseudo: Option<&'static str>,
) -> Result<Rc<ObjectRoot>, ApiError> {
    let mut args = vec![Arg::Node(element.clone())];
    if let Some(pseudo) = pseudo {
        args.push(Arg::Str(pseudo));
    }
    let ret = call_on(
        dom,
        This::Object(window.clone()),
        "Window",
        "getComputedStyle",
        MemberKind::Method,
        &args,
    )
    .0?;
    match ret {
        Ret::Object(object) => Ok(Rc::new(object)),
        other => panic!("not an object: {other:?}"),
    }
}

fn member(
    dom: &mut TestDom,
    style: &Rc<ObjectRoot>,
    name: &str,
    kind: MemberKind,
    args: &[Arg],
) -> Result<Ret, ApiError> {
    call_on(
        dom,
        This::Object(style.clone()),
        crate::css_style::interface_of(name),
        name,
        kind,
        args,
    )
    .0
}

fn value(dom: &mut TestDom, style: &Rc<ObjectRoot>, property: &'static str) -> String {
    member(
        dom,
        style,
        "getPropertyValue",
        MemberKind::Method,
        &[Arg::Str(property)],
    )
    .unwrap()
    .string()
    .to_string()
}

/// A value of what layout doesn't decide brings the style up to date, one
/// of what it does the layout, from the element's box; without a box, the
/// computed value. Each call makes a new declaration.
#[test]
fn values_bring_style_or_layout_up_to_date() {
    let (mut dom, nodes, log) = page(false, &["div"]);
    let window = window(&mut dom, &nodes);
    let div = nodes[3].clone();
    let div_id = id(&dom, &div);
    let style = computed(&mut dom, &window, &div, None).unwrap();
    let again = computed(&mut dom, &window, &div, None).unwrap();
    assert!(!dom.read(|cx| {
        let (ObjectRef::CssStyleDeclaration(a), ObjectRef::CssStyleDeclaration(b)) =
            (style.get(cx.gc), again.get(cx.gc))
        else {
            panic!("declarations");
        };
        elise::Gc::ptr_eq(a, b)
    }));
    assert!(log.borrow().is_empty(), "nothing read yet");
    assert_eq!(value(&mut dom, &style, "display"), "inline");
    assert_eq!(*log.borrow(), ["style"]);
    log.borrow_mut().clear();
    assert_eq!(
        value(&mut dom, &style, "width"),
        "auto",
        "no box: the computed value"
    );
    assert_eq!(*log.borrow(), ["style", "layout"]);
    fake(&dom, |fake| {
        fake.boxes
            .insert(div_id, block(0.0, 0.0, 30.0, 20.0, [1.0; 4]));
    });
    assert_eq!(value(&mut dom, &style, "width"), "28px", "the content box");
    assert_eq!(value(&mut dom, &style, "height"), "18px");
    // A logical property as its physical one; a shorthand from its
    // longhands.
    assert_eq!(value(&mut dom, &style, "inline-size"), "28px");
    assert_eq!(value(&mut dom, &style, "margin"), "0px");
    assert_eq!(value(&mut dom, &style, "not-a-property"), "");
    // Named properties too.
    let named = member(
        &mut dom,
        &style,
        "getter",
        MemberKind::Method,
        &[Arg::Str("width")],
    )
    .unwrap();
    assert_eq!(named.string().to_string(), "28px");
}

/// `length` and `item()` list every computable longhand; the declaration
/// has no text or priorities of its own and no rule.
#[test]
fn it_lists_the_computable_properties() {
    let (mut dom, nodes, _) = page(false, &["div"]);
    let window = window(&mut dom, &nodes);
    let style = computed(&mut dom, &window, &nodes[3], None).unwrap();
    let length = match member(&mut dom, &style, "length", MemberKind::Getter, &[]).unwrap() {
        Ret::U32(length) => length,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        length as usize,
        style_resolver::computable_properties().len()
    );
    let first = member(
        &mut dom,
        &style,
        "item",
        MemberKind::Method,
        &[Arg::Number(0.0)],
    )
    .unwrap();
    assert_eq!(
        first.string().to_string(),
        style_resolver::computable_properties()[0]
    );
    let past = member(
        &mut dom,
        &style,
        "item",
        MemberKind::Method,
        &[Arg::Number(f64::from(length))],
    )
    .unwrap();
    assert_eq!(past.string().to_string(), "");
    let text = member(&mut dom, &style, "cssText", MemberKind::Getter, &[]).unwrap();
    assert_eq!(text.string().to_string(), "");
    let priority = member(
        &mut dom,
        &style,
        "getPropertyPriority",
        MemberKind::Method,
        &[Arg::Str("color")],
    )
    .unwrap();
    assert_eq!(priority.string().to_string(), "");
    assert!(matches!(
        member(&mut dom, &style, "parentRule", MemberKind::Getter, &[]).unwrap(),
        Ret::Null
    ));
}

/// Every member that changes the declarations throws
/// `NoModificationAllowedError`, and changes nothing.
#[test]
fn it_is_read_only() {
    let (mut dom, nodes, _) = page(false, &["div"]);
    let window = window(&mut dom, &nodes);
    let style = computed(&mut dom, &window, &nodes[3], None).unwrap();
    let cases: [(&str, MemberKind, Vec<Arg>); 6] = [
        ("cssText", MemberKind::Setter, vec![Arg::Str("color: red")]),
        (
            "setProperty",
            MemberKind::Method,
            vec![Arg::Str("color"), Arg::Str("red")],
        ),
        (
            "removeProperty",
            MemberKind::Method,
            vec![Arg::Str("color")],
        ),
        ("cssFloat", MemberKind::Setter, vec![Arg::Str("left")]),
        (
            "setter",
            MemberKind::Method,
            vec![Arg::Str("color"), Arg::Str("red")],
        ),
        (
            "setter",
            MemberKind::Method,
            vec![Arg::Str("backgroundColor"), Arg::Str("red")],
        ),
    ];
    for (name, kind, args) in cases {
        let error = member(&mut dom, &style, name, kind, &args).unwrap_err();
        assert_eq!(exception(error), "NoModificationAllowedError", "{name}");
    }
    let inline = dom.read(|cx| {
        cx.get_attribute(nodes[3].get(cx.gc), "style")
            .map(|s| s.to_string())
    });
    assert_eq!(inline, None, "the element's own untouched");
    assert_eq!(value(&mut dom, &style, "color"), "rgb(0, 0, 0)");
}

/// The pseudo-element argument: none or not starting with a colon is the
/// element's; `::before` and `:before` (CSS 2's take one colon) the
/// pseudo-element's, its style without rules if none matched; a known one
/// machopo doesn't have, or a newer one with one colon, no style at all; any
/// other a `SyntaxError`. An element in no document has none.
#[test]
fn the_pseudo_element_argument() {
    let (mut dom, nodes, _) = page(false, &["div"]);
    let window = window(&mut dom, &nodes);
    let div = nodes[3].clone();
    let div_id = id(&dom, &div);
    fake(&dom, |fake| {
        fake.positions
            .insert(div_id, computed_style::Position::Absolute);
    });
    for pseudo in [None, Some("before"), Some("")] {
        let style = computed(&mut dom, &window, &div, pseudo).unwrap();
        assert_eq!(
            value(&mut dom, &style, "position"),
            "absolute",
            "{pseudo:?}"
        );
    }
    for pseudo in ["::before", ":before", "::AFTER", "::marker"] {
        let style = computed(&mut dom, &window, &div, Some(pseudo)).unwrap();
        // A pseudo-element no rule matches: what cascading nothing gives
        // it (`position` doesn't inherit: static).
        assert_eq!(value(&mut dom, &style, "position"), "static", "{pseudo}");
    }
    for pseudo in ["::first-line", ":marker", ":selection", "::backdrop"] {
        let style = computed(&mut dom, &window, &div, Some(pseudo)).unwrap();
        assert_eq!(value(&mut dom, &style, "position"), "", "{pseudo}");
        let length = member(&mut dom, &style, "length", MemberKind::Getter, &[]).unwrap();
        assert!(matches!(length, Ret::U32(0)), "{pseudo}");
    }
    for pseudo in ["::nope", ":hover", "::before(", "::-webkit-thing"] {
        let error = computed(&mut dom, &window, &div, Some(pseudo)).unwrap_err();
        assert_eq!(exception(error), "SyntaxError", "{pseudo}");
    }
    // `::selection`: the element's style with its highlight's (none
    // matched: the initial, a transparent background).
    let selection = computed(&mut dom, &window, &div, Some("::selection")).unwrap();
    assert_eq!(
        value(&mut dom, &selection, "background-color"),
        "rgba(0, 0, 0, 0)"
    );
    // `::placeholder` of no control: what cascading nothing gives it.
    let placeholder = computed(&mut dom, &window, &div, Some("::placeholder")).unwrap();
    assert_eq!(value(&mut dom, &placeholder, "position"), "static");
    // Taken out of its document: no style.
    let style = computed(&mut dom, &window, &div, None).unwrap();
    dom.mutate(|cx| cx.remove(div.get(cx.gc)));
    assert_eq!(value(&mut dom, &style, "position"), "");
}
