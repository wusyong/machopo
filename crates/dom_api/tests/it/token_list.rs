//! `classList` and its `DOMTokenList` through their members.

use std::rc::Rc;

use dom::{NodeRoot, ObjectRoot, QualName};
use dom_api::MemberKind;

use crate::{Arg, Ret, TestDom, This, call, call_on, exception, get, set};

fn element(dom: &mut TestDom, class: Option<&str>) -> Rc<NodeRoot> {
    Rc::new(dom.mutate(|cx| {
        let doc = cx.create_document(true);
        let element = cx.create_element(doc, QualName::html("div"));
        if let Some(class) = class {
            cx.set_attribute(element, "class", class);
        }
        cx.root(element)
    }))
}

fn class_list(dom: &mut TestDom, element: &Rc<NodeRoot>) -> Rc<ObjectRoot> {
    match get(dom, element, "Element", "classList") {
        Ret::Object(object) => Rc::new(object),
        other => panic!("{other:?}"),
    }
}

fn list_call(
    dom: &mut TestDom,
    list: &Rc<ObjectRoot>,
    name: &str,
    kind: MemberKind,
    args: &[Arg],
) -> Result<Ret, dom_api::ApiError> {
    call_on(
        dom,
        This::Object(list.clone()),
        "DOMTokenList",
        name,
        kind,
        args,
    )
    .0
}

fn class(dom: &mut TestDom, element: &Rc<NodeRoot>) -> Option<String> {
    dom.read(|cx| {
        cx.get_attribute(element.get(cx.gc), "class")
            .map(|value| value.to_string())
    })
}

fn string(ret: Ret) -> String {
    ret.string().to_string()
}

#[test]
fn class_list_is_the_same_list_each_time() {
    let mut dom = TestDom::new();
    let element = element(&mut dom, Some("a"));
    let first = class_list(&mut dom, &element);
    let again = class_list(&mut dom, &element);
    assert!(dom.read(|cx| match (first.get(cx.gc), again.get(cx.gc)) {
        (dom::ObjectRef::DomTokenList(a), dom::ObjectRef::DomTokenList(b)) =>
            elise::Gc::ptr_eq(a, b),
        _ => false,
    }));
}

#[test]
fn the_tokens_are_the_attribute_as_a_set() {
    let mut dom = TestDom::new();
    let element = element(&mut dom, Some("  b a\tb  "));
    let list = class_list(&mut dom, &element);
    assert!(matches!(
        list_call(&mut dom, &list, "length", MemberKind::Getter, &[]),
        Ok(Ret::U32(2))
    ));
    let item = |dom: &mut TestDom, index: f64| {
        list_call(
            dom,
            &list,
            "item",
            MemberKind::Method,
            &[Arg::Number(index)],
        )
        .unwrap()
    };
    assert_eq!(string(item(&mut dom, 0.0)), "b");
    assert_eq!(string(item(&mut dom, 1.0)), "a");
    assert!(matches!(item(&mut dom, 2.0), Ret::Null));
    for (token, has) in [("a", true), ("b", true), ("c", false), ("A", false)] {
        assert!(
            matches!(list_call(&mut dom, &list, "contains", MemberKind::Method, &[Arg::Str(token)]), Ok(Ret::Bool(b)) if b == has),
            "{token}"
        );
    }
    // `value` and the stringifier are the attribute as it is.
    assert_eq!(
        string(list_call(&mut dom, &list, "value", MemberKind::Getter, &[]).unwrap()),
        "  b a\tb  "
    );
    assert_eq!(
        string(list_call(&mut dom, &list, "toString", MemberKind::Method, &[]).unwrap()),
        "  b a\tb  "
    );
}

#[test]
fn add_and_remove_write_the_set_back() {
    let mut dom = TestDom::new();
    let element = element(&mut dom, Some(" a  a "));
    let list = class_list(&mut dom, &element);
    list_call(
        &mut dom,
        &list,
        "add",
        MemberKind::Method,
        &[Arg::Str("b"), Arg::Str("a"), Arg::Str("c")],
    )
    .unwrap();
    assert_eq!(class(&mut dom, &element).as_deref(), Some("a b c"));
    list_call(
        &mut dom,
        &list,
        "remove",
        MemberKind::Method,
        &[Arg::Str("a"), Arg::Str("z")],
    )
    .unwrap();
    assert_eq!(class(&mut dom, &element).as_deref(), Some("b c"));
    // No arguments still writes the set back.
    let element2 = self::element(&mut dom, Some(" x  x"));
    let list2 = class_list(&mut dom, &element2);
    list_call(&mut dom, &list2, "add", MemberKind::Method, &[]).unwrap();
    assert_eq!(class(&mut dom, &element2).as_deref(), Some("x"));
    // An empty set on an element with no attribute adds none.
    let bare = self::element(&mut dom, None);
    let bare_list = class_list(&mut dom, &bare);
    list_call(
        &mut dom,
        &bare_list,
        "remove",
        MemberKind::Method,
        &[Arg::Str("a")],
    )
    .unwrap();
    assert_eq!(class(&mut dom, &bare), None);
    list_call(
        &mut dom,
        &bare_list,
        "add",
        MemberKind::Method,
        &[Arg::Str("n")],
    )
    .unwrap();
    assert_eq!(class(&mut dom, &bare).as_deref(), Some("n"));
}

#[test]
fn a_bad_token_throws_before_any_change() {
    let mut dom = TestDom::new();
    let element = element(&mut dom, Some("a"));
    let list = class_list(&mut dom, &element);
    for (args, name) in [
        (vec![Arg::Str("b"), Arg::Str("")], "SyntaxError"),
        (
            vec![Arg::Str("b"), Arg::Str("c d")],
            "InvalidCharacterError",
        ),
    ] {
        for method in ["add", "remove"] {
            let error = list_call(&mut dom, &list, method, MemberKind::Method, &args).unwrap_err();
            assert_eq!(exception(error), name, "{method}");
        }
        assert_eq!(class(&mut dom, &element).as_deref(), Some("a"));
    }
    let error = list_call(
        &mut dom,
        &list,
        "toggle",
        MemberKind::Method,
        &[Arg::Str("")],
    )
    .unwrap_err();
    assert_eq!(exception(error), "SyntaxError");
    let error = list_call(
        &mut dom,
        &list,
        "replace",
        MemberKind::Method,
        &[Arg::Str("a"), Arg::Str(" ")],
    )
    .unwrap_err();
    assert_eq!(exception(error), "InvalidCharacterError");
    // Both tokens for emptiness before either for whitespace.
    let error = list_call(
        &mut dom,
        &list,
        "replace",
        MemberKind::Method,
        &[Arg::Str(" "), Arg::Str("")],
    )
    .unwrap_err();
    assert_eq!(exception(error), "SyntaxError");
    let error = list_call(
        &mut dom,
        &list,
        "supports",
        MemberKind::Method,
        &[Arg::Str("a")],
    )
    .unwrap_err();
    assert!(
        matches!(error, dom_api::ApiError::Type(_)),
        "class has no supported tokens"
    );
}

#[test]
fn toggle_takes_force() {
    let mut dom = TestDom::new();
    let element = element(&mut dom, Some("a"));
    let list = class_list(&mut dom, &element);
    let toggle = |dom: &mut TestDom, args: &[Arg]| match list_call(
        dom,
        &list,
        "toggle",
        MemberKind::Method,
        args,
    ) {
        Ok(Ret::Bool(value)) => value,
        other => panic!("{other:?}"),
    };
    assert!(!toggle(&mut dom, &[Arg::Str("a")]));
    assert_eq!(class(&mut dom, &element).as_deref(), Some(""));
    assert!(toggle(&mut dom, &[Arg::Str("a")]));
    assert_eq!(class(&mut dom, &element).as_deref(), Some("a"));
    // `force` true keeps it, false removes none; undefined is no force.
    assert!(toggle(&mut dom, &[Arg::Str("a"), Arg::Bool(true)]));
    assert!(!toggle(&mut dom, &[Arg::Str("b"), Arg::Bool(false)]));
    assert_eq!(class(&mut dom, &element).as_deref(), Some("a"));
    assert!(!toggle(&mut dom, &[Arg::Str("a"), Arg::Undefined]));
    assert!(toggle(&mut dom, &[Arg::Str("a"), Arg::Undefined]));
    assert!(!toggle(&mut dom, &[Arg::Str("a")]));
    assert!(toggle(&mut dom, &[Arg::Str("b"), Arg::Bool(true)]));
    assert!(!toggle(&mut dom, &[Arg::Str("b"), Arg::Bool(false)]));
    assert_eq!(class(&mut dom, &element).as_deref(), Some(""));
}

#[test]
fn replace_puts_the_new_token_in_the_first_place() {
    let mut dom = TestDom::new();
    let element = element(&mut dom, Some("a b c b"));
    let list = class_list(&mut dom, &element);
    let replace = |dom: &mut TestDom, token: &'static str, new: &'static str| match list_call(
        dom,
        &list,
        "replace",
        MemberKind::Method,
        &[Arg::Str(token), Arg::Str(new)],
    ) {
        Ok(Ret::Bool(value)) => value,
        other => panic!("{other:?}"),
    };
    assert!(!replace(&mut dom, "z", "y"));
    assert_eq!(
        class(&mut dom, &element).as_deref(),
        Some("a b c b"),
        "unchanged"
    );
    assert!(replace(&mut dom, "c", "a"));
    assert_eq!(class(&mut dom, &element).as_deref(), Some("a b"));
    assert!(replace(&mut dom, "b", "d"));
    assert_eq!(class(&mut dom, &element).as_deref(), Some("a d"));
    // The new token after the old one: the old one's place.
    assert!(replace(&mut dom, "a", "d"));
    assert_eq!(class(&mut dom, &element).as_deref(), Some("d"));
}

#[test]
fn value_and_class_list_set_the_attribute() {
    let mut dom = TestDom::new();
    let element = element(&mut dom, None);
    let list = class_list(&mut dom, &element);
    assert_eq!(
        string(list_call(&mut dom, &list, "value", MemberKind::Getter, &[]).unwrap()),
        ""
    );
    list_call(
        &mut dom,
        &list,
        "value",
        MemberKind::Setter,
        &[Arg::Str("p q")],
    )
    .ok();
    assert_eq!(class(&mut dom, &element).as_deref(), Some("p q"));
    assert!(matches!(
        list_call(&mut dom, &list, "length", MemberKind::Getter, &[]),
        Ok(Ret::U32(2))
    ));
    // `[PutForwards=value]`.
    set(&mut dom, &element, "Element", "classList", Arg::Str("r")).unwrap();
    assert_eq!(class(&mut dom, &element).as_deref(), Some("r"));
    assert!(matches!(
        call(&mut dom, &element, "Element", "className", MemberKind::Getter, &[]),
        Ok(Ret::Str(value)) if value == "r"
    ));
}

/// A class no one has asked for yet is in the set: `contains` makes the
/// classes before it looks for the name.
#[test]
fn contains_finds_a_class_not_made_yet() {
    let mut dom = TestDom::new();
    let element = element(&mut dom, Some("never-seen-before-class other"));
    let list = class_list(&mut dom, &element);
    assert!(matches!(
        list_call(
            &mut dom,
            &list,
            "contains",
            MemberKind::Method,
            &[Arg::Str("never-seen-before-class")]
        ),
        Ok(Ret::Bool(true))
    ));
}
