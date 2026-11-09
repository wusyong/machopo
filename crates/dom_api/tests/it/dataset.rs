//! `dataset` and its `DOMStringMap`'s named properties through their
//! special operations.

use std::rc::Rc;

use dom::{NodeRoot, ObjectRoot, QualName};
use dom_api::{ApiError, MemberKind};

use crate::{Arg, Ret, TestDom, This, call_on, exception, get};

fn element(dom: &mut TestDom) -> Rc<NodeRoot> {
    Rc::new(dom.mutate(|cx| {
        let doc = cx.create_document(true);
        let element = cx.create_element(doc, QualName::html("div"));
        cx.set_attribute(element, "data-foo-bar", "1");
        cx.set_attribute(element, "data-Upper", "no");
        cx.set_attribute(element, "data-", "empty");
        cx.set_attribute_ns(
            element,
            QualName {
                namespace: Some("urn:x"),
                prefix: None,
                local: "data-ns",
            },
            "no",
        );
        cx.set_attribute(element, "id", "i");
        cx.root(element)
    }))
}

fn dataset(dom: &mut TestDom, element: &Rc<NodeRoot>) -> Rc<ObjectRoot> {
    match get(dom, element, "HTMLElement", "dataset") {
        Ret::Object(object) => Rc::new(object),
        other => panic!("{other:?}"),
    }
}

fn special(
    dom: &mut TestDom,
    map: &Rc<ObjectRoot>,
    name: &str,
    args: &[Arg],
) -> Result<Ret, ApiError> {
    call_on(
        dom,
        This::Object(map.clone()),
        "DOMStringMap",
        name,
        MemberKind::Method,
        args,
    )
    .0
}

fn attribute(dom: &mut TestDom, element: &Rc<NodeRoot>, name: &str) -> Option<String> {
    dom.read(|cx| {
        cx.get_attribute(element.get(cx.gc), name)
            .map(|value| value.to_string())
    })
}

#[test]
fn names_and_values_come_from_data_attributes() {
    let mut dom = TestDom::new();
    let element = element(&mut dom);
    let map = dataset(&mut dom, &element);
    match special(&mut dom, &map, "names", &[]).unwrap() {
        Ret::Strings(names) => {
            let names: Vec<String> = names.iter().map(ToString::to_string).collect();
            assert_eq!(names, ["fooBar", ""]);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        special(&mut dom, &map, "getter", &[Arg::Str("fooBar")])
            .unwrap()
            .string()
            .to_string(),
        "1"
    );
    assert_eq!(
        special(&mut dom, &map, "getter", &[Arg::Str("")])
            .unwrap()
            .string()
            .to_string(),
        "empty"
    );
    for missing in ["foo-bar", "upper", "Upper", "ns", "id"] {
        assert!(
            matches!(
                special(&mut dom, &map, "getter", &[Arg::Str(missing)]).unwrap(),
                Ret::Null
            ),
            "{missing}"
        );
    }
}

#[test]
fn the_setter_and_deleter_change_the_attributes() {
    let mut dom = TestDom::new();
    let element = element(&mut dom);
    let map = dataset(&mut dom, &element);
    special(
        &mut dom,
        &map,
        "setter",
        &[Arg::Str("newOne"), Arg::Str("v")],
    )
    .unwrap();
    assert_eq!(
        attribute(&mut dom, &element, "data-new-one").as_deref(),
        Some("v")
    );
    special(
        &mut dom,
        &map,
        "setter",
        &[Arg::Str("fooBar"), Arg::Str("2")],
    )
    .unwrap();
    assert_eq!(
        attribute(&mut dom, &element, "data-foo-bar").as_deref(),
        Some("2")
    );
    let error = special(&mut dom, &map, "setter", &[Arg::Str("a-b"), Arg::Str("v")]).unwrap_err();
    assert_eq!(exception(error), "SyntaxError");
    let error = special(&mut dom, &map, "setter", &[Arg::Str("a b"), Arg::Str("v")]).unwrap_err();
    assert_eq!(exception(error), "InvalidCharacterError");
    special(&mut dom, &map, "deleter", &[Arg::Str("fooBar")]).unwrap();
    assert_eq!(attribute(&mut dom, &element, "data-foo-bar"), None);
    // A name no attribute can be deletes nothing.
    special(&mut dom, &map, "deleter", &[Arg::Str("new-one")]).unwrap();
    assert_eq!(
        attribute(&mut dom, &element, "data-new-one").as_deref(),
        Some("v")
    );
}
