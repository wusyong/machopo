//! `Element`'s attributes: by qualified name (lowercased for an HTML
//! element in an HTML document only), by namespace, the reflected `id` and
//! `className`, and their errors.

use dom_api::MemberKind;

use crate::{Arg, Ret, TestDom, document, exception, get, method, rc, set};

const SVG: &str = "http://www.w3.org/2000/svg";
const XLINK: &str = "http://www.w3.org/1999/xlink";

type Root = std::rc::Rc<dom::NodeRoot>;

fn element(dom: &mut TestDom, doc: &Root, name: &'static str) -> Root {
    rc(method(dom, doc, "Document", "createElement", &[Arg::Str(name)]).unwrap())
}

fn attribute(dom: &mut TestDom, this: &Root, name: &'static str) -> Ret {
    method(dom, this, "Element", "getAttribute", &[Arg::Str(name)]).unwrap()
}

fn has(dom: &mut TestDom, this: &Root, name: &'static str) -> bool {
    matches!(
        method(dom, this, "Element", "hasAttribute", &[Arg::Str(name)]).unwrap(),
        Ret::Bool(true)
    )
}

#[test]
fn an_html_elements_attribute_names_are_lowercased_in_an_html_document() {
    let mut dom = TestDom::new();
    let doc = document(&mut dom, true);
    let div = element(&mut dom, &doc, "div");
    assert!(matches!(
        method(&mut dom, &div, "Element", "hasAttributes", &[]),
        Ok(Ret::Bool(false))
    ));
    method(
        &mut dom,
        &div,
        "Element",
        "setAttribute",
        &[Arg::Str("DATA-X"), Arg::Str("1")],
    )
    .unwrap();
    assert_eq!(attribute(&mut dom, &div, "data-x").string(), "1");
    assert_eq!(attribute(&mut dom, &div, "Data-X").string(), "1");
    assert!(has(&mut dom, &div, "DATA-x"));
    assert!(matches!(
        method(&mut dom, &div, "Element", "hasAttributes", &[]),
        Ok(Ret::Bool(true))
    ));
    let found = method(
        &mut dom,
        &div,
        "Element",
        "getAttributeNS",
        &[Arg::Null, Arg::Str("data-x")],
    );
    assert_eq!(found.unwrap().string(), "1");
    method(
        &mut dom,
        &div,
        "Element",
        "removeAttribute",
        &[Arg::Str("Data-x")],
    )
    .unwrap();
    assert!(matches!(attribute(&mut dom, &div, "data-x"), Ret::Null));
    assert!(!has(&mut dom, &div, "data-x"));
}

/// Not lowercased: an element of another namespace in an HTML document,
/// and an HTML element in an XML document.
#[test]
fn other_elements_attribute_names_keep_their_case() {
    let mut dom = TestDom::new();
    let html = document(&mut dom, true);
    let xml = document(&mut dom, false);
    let svg = rc(method(
        &mut dom,
        &html,
        "Document",
        "createElementNS",
        &[Arg::Str(SVG), Arg::Str("svg")],
    )
    .unwrap());
    let in_xml = rc(method(
        &mut dom,
        &xml,
        "Document",
        "createElementNS",
        &[Arg::Str("http://www.w3.org/1999/xhtml"), Arg::Str("div")],
    )
    .unwrap());
    for this in [&svg, &in_xml] {
        method(
            &mut dom,
            this,
            "Element",
            "setAttribute",
            &[Arg::Str("viewBox"), Arg::Str("0 0 1 1")],
        )
        .unwrap();
        assert_eq!(attribute(&mut dom, this, "viewBox").string(), "0 0 1 1");
        assert!(matches!(attribute(&mut dom, this, "viewbox"), Ret::Null));
        assert!(!has(&mut dom, this, "viewbox"));
        method(
            &mut dom,
            this,
            "Element",
            "removeAttribute",
            &[Arg::Str("viewbox")],
        )
        .unwrap();
        assert!(has(&mut dom, this, "viewBox"));
    }
}

#[test]
fn set_attribute_refuses_an_invalid_name() {
    let mut dom = TestDom::new();
    let doc = document(&mut dom, true);
    let div = element(&mut dom, &doc, "div");
    for name in ["", "a b", "a=b", "a/b", "a>", "a\0"] {
        let error = method(
            &mut dom,
            &div,
            "Element",
            "setAttribute",
            &[Arg::Str(name), Arg::Str("x")],
        )
        .unwrap_err();
        assert_eq!(exception(error), "InvalidCharacterError", "{name:?}");
    }
    for name in ["1x", "a<b", "-", "a:b"] {
        method(
            &mut dom,
            &div,
            "Element",
            "setAttribute",
            &[Arg::Str(name), Arg::Str("x")],
        )
        .unwrap();
        assert!(has(&mut dom, &div, name), "{name:?}");
    }
}

#[test]
fn namespaced_attributes_validate_and_are_found_by_namespace() {
    let mut dom = TestDom::new();
    let doc = document(&mut dom, true);
    let a = element(&mut dom, &doc, "a");
    let set_ns = |dom: &mut TestDom, namespace: Arg, qualified: &'static str| {
        method(
            dom,
            &a,
            "Element",
            "setAttributeNS",
            &[namespace, Arg::Str(qualified), Arg::Str("v")],
        )
    };
    set_ns(&mut dom, Arg::Str(XLINK), "xl:HREF").unwrap();
    let get_ns = |dom: &mut TestDom, local: &'static str| {
        method(
            dom,
            &a,
            "Element",
            "getAttributeNS",
            &[Arg::Str(XLINK), Arg::Str(local)],
        )
        .unwrap()
    };
    // Not lowercased: a namespace's names are exact.
    assert_eq!(get_ns(&mut dom, "HREF").string(), "v");
    assert!(matches!(get_ns(&mut dom, "href"), Ret::Null));
    // `getAttribute` lowercases its name for an HTML element in an HTML
    // document: an uppercase namespaced name isn't found that way.
    assert!(matches!(attribute(&mut dom, &a, "xl:HREF"), Ret::Null));
    set_ns(&mut dom, Arg::Str(XLINK), "xl:show").unwrap();
    assert_eq!(attribute(&mut dom, &a, "XL:Show").string(), "v");
    let has_ns = method(
        &mut dom,
        &a,
        "Element",
        "hasAttributeNS",
        &[Arg::Str(XLINK), Arg::Str("HREF")],
    );
    assert!(matches!(has_ns, Ok(Ret::Bool(true))));
    for (namespace, qualified, name) in [
        (Arg::Null, "x:a", "NamespaceError"),
        (Arg::Str(XLINK), "a b", "InvalidCharacterError"),
        (Arg::Str(XLINK), "a=b", "InvalidCharacterError"),
        (Arg::Str(XLINK), "xmlns", "NamespaceError"),
    ] {
        let error = set_ns(&mut dom, namespace, qualified).unwrap_err();
        assert_eq!(exception(error), name, "{qualified:?}");
    }
    method(
        &mut dom,
        &a,
        "Element",
        "removeAttributeNS",
        &[Arg::Str(XLINK), Arg::Str("HREF")],
    )
    .unwrap();
    assert!(matches!(get_ns(&mut dom, "HREF"), Ret::Null));
    let has_ns = method(
        &mut dom,
        &a,
        "Element",
        "hasAttributeNS",
        &[Arg::Str(XLINK), Arg::Str("HREF")],
    );
    assert!(matches!(has_ns, Ok(Ret::Bool(false))));
}

/// `id` and `className` reflect their attributes in no namespace: the empty
/// string when there is none.
#[test]
fn id_and_class_name_reflect_their_attributes() {
    let mut dom = TestDom::new();
    let doc = document(&mut dom, true);
    let div = element(&mut dom, &doc, "div");
    assert_eq!(get(&mut dom, &div, "Element", "id").string(), "");
    assert_eq!(get(&mut dom, &div, "Element", "className").string(), "");
    set(&mut dom, &div, "Element", "id", Arg::Str("main")).unwrap();
    set(&mut dom, &div, "Element", "className", Arg::Str("a b")).unwrap();
    assert_eq!(attribute(&mut dom, &div, "id").string(), "main");
    assert_eq!(attribute(&mut dom, &div, "class").string(), "a b");
    method(
        &mut dom,
        &div,
        "Element",
        "setAttribute",
        &[Arg::Str("ID"), Arg::Str("other")],
    )
    .unwrap();
    assert_eq!(get(&mut dom, &div, "Element", "id").string(), "other");
    // A value in another namespace with the same local name isn't it.
    method(
        &mut dom,
        &div,
        "Element",
        "setAttributeNS",
        &[Arg::Str(XLINK), Arg::Str("class"), Arg::Str("no")],
    )
    .unwrap();
    assert_eq!(get(&mut dom, &div, "Element", "className").string(), "a b");
    // Nor when it is the element's first attribute of that local name.
    let span = element(&mut dom, &doc, "span");
    method(
        &mut dom,
        &span,
        "Element",
        "setAttributeNS",
        &[Arg::Str(XLINK), Arg::Str("id"), Arg::Str("no")],
    )
    .unwrap();
    assert_eq!(get(&mut dom, &span, "Element", "id").string(), "");
    set(&mut dom, &span, "Element", "id", Arg::Str("yes")).unwrap();
    assert_eq!(get(&mut dom, &span, "Element", "id").string(), "yes");
}

/// A string result the DOM holds (an attribute's value, a text's data, a
/// name) is given borrowed, not cloned: as many references to it as the
/// DOM itself holds.
#[test]
fn strings_the_dom_holds_are_given_borrowed() {
    let mut dom = TestDom::new();
    let doc = document(&mut dom, true);
    let p = element(&mut dom, &doc, "p");
    method(
        &mut dom,
        &p,
        "Element",
        "setAttribute",
        &[Arg::Str("id"), Arg::Str("value")],
    )
    .unwrap();
    // The DOM's own references to `id`'s value, and to the name `p`.
    let held = |dom: &mut TestDom| {
        dom.mutate(|cx| {
            let element = p.get(cx.gc);
            let refs = |string: &dom::DomString| string.reference_count();
            let value = refs(&cx.attribute_value(element, "id").unwrap().borrow());
            let name = refs(&dom::names::string(cx.element_name(element).unwrap().local));
            (value, name)
        })
    };
    let (value, name) = held(&mut dom);
    for (member, kind, args, refs) in [
        (
            "getAttribute",
            MemberKind::Method,
            vec![Arg::Str("id")],
            value,
        ),
        (
            "getAttributeNS",
            MemberKind::Method,
            vec![Arg::Null, Arg::Str("id")],
            value,
        ),
        ("id", MemberKind::Getter, vec![], value),
        ("localName", MemberKind::Getter, vec![], name),
    ] {
        let (ret, hooks) = crate::call_with_hooks(&mut dom, &p, "Element", member, kind, &args);
        assert!(ret.is_ok());
        assert_eq!(hooks.string_refs, Some(refs), "{member}");
    }
    // `tagName`: the uppercased name, made once and held by the thread's
    // names, which hand the front their string (one reference more while
    // given, as `name` above counts).
    let (tag, hooks) =
        crate::call_with_hooks(&mut dom, &p, "Element", "tagName", MemberKind::Getter, &[]);
    assert!(matches!(tag.unwrap(), Ret::Str(tag) if tag == "P"));
    let given = dom.mutate(|cx| {
        let id = cx.node_name_id(p.get(cx.gc));
        dom::names::string(id).reference_count()
    });
    assert_eq!(hooks.string_refs, Some(given));
    // A text's data.
    let text = rc(method(
        &mut dom,
        &doc,
        "Document",
        "createTextNode",
        &[Arg::Str("data")],
    )
    .unwrap());
    let data = dom.mutate(|cx| {
        let data = cx.text_data(text.get(cx.gc)).unwrap();
        data.borrow().reference_count()
    });
    let (_, hooks) = crate::call_with_hooks(
        &mut dom,
        &text,
        "CharacterData",
        "data",
        MemberKind::Getter,
        &[],
    );
    assert_eq!(hooks.string_refs, Some(data));
}
