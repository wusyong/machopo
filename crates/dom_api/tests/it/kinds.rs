//! The interfaces of the node kinds besides documents, elements and texts:
//! a doctype's name and ids, a processing instruction's target, a
//! comment's data (`CharacterData`'s), a fragment's `ParentNode` members;
//! the `Node` members of each.

use std::rc::Rc;

use dom::QualName;

use crate::{Ret, TestDom, document, get, rc};

#[test]
fn a_doctypes_and_a_processing_instructions_members() {
    let mut dom = TestDom::new();
    let doc = document(&mut dom, true);
    let (doctype, pi, comment) = dom.mutate(|cx| {
        let document = doc.get(cx.gc);
        (
            Rc::new(cx.root(cx.create_doctype(
                document,
                "html",
                "-//W3C//DTD XHTML 1.0//EN",
                "about:x",
            ))),
            Rc::new(cx.root(cx.create_processing_instruction(
                document,
                "xml-stylesheet",
                "href=a",
            ))),
            Rc::new(cx.root(cx.create_comment(document, "hé"))),
        )
    });
    assert_eq!(
        get(&mut dom, &doctype, "DocumentType", "name").string(),
        "html"
    );
    assert_eq!(
        get(&mut dom, &doctype, "DocumentType", "publicId").string(),
        "-//W3C//DTD XHTML 1.0//EN"
    );
    assert_eq!(
        get(&mut dom, &doctype, "DocumentType", "systemId").string(),
        "about:x"
    );
    assert!(matches!(
        get(&mut dom, &doctype, "Node", "nodeType"),
        Ret::U32(10)
    ));
    assert!(matches!(
        get(&mut dom, &doctype, "Node", "textContent"),
        Ret::Null
    ));
    assert_eq!(
        get(&mut dom, &pi, "ProcessingInstruction", "target").string(),
        "xml-stylesheet"
    );
    assert_eq!(
        get(&mut dom, &pi, "CharacterData", "data").string(),
        "href=a"
    );
    assert_eq!(
        get(&mut dom, &pi, "Node", "nodeName").string(),
        "xml-stylesheet"
    );
    assert!(matches!(
        get(&mut dom, &comment, "Node", "nodeType"),
        Ret::U32(8)
    ));
    assert_eq!(
        get(&mut dom, &comment, "Node", "nodeName").string(),
        "#comment"
    );
    assert_eq!(
        get(&mut dom, &comment, "CharacterData", "data").string(),
        "hé"
    );
    assert!(matches!(
        get(&mut dom, &comment, "CharacterData", "length"),
        Ret::U32(2)
    ));
}

#[test]
fn a_fragments_parent_node_members() {
    let mut dom = TestDom::new();
    let doc = document(&mut dom, true);
    let fragment = dom.mutate(|cx| {
        let document = doc.get(cx.gc);
        let fragment = cx.create_document_fragment(document);
        cx.insert(fragment, cx.create_text(document, "t"), None);
        for name in ["a", "b"] {
            cx.insert(
                fragment,
                cx.create_element(document, QualName::html(name)),
                None,
            );
        }
        Rc::new(cx.root(fragment))
    });
    assert!(matches!(
        get(&mut dom, &fragment, "Node", "nodeType"),
        Ret::U32(11)
    ));
    assert_eq!(
        get(&mut dom, &fragment, "Node", "nodeName").string(),
        "#document-fragment"
    );
    assert!(matches!(
        get(&mut dom, &fragment, "DocumentFragment", "childElementCount"),
        Ret::U32(2)
    ));
    let first = rc(get(
        &mut dom,
        &fragment,
        "DocumentFragment",
        "firstElementChild",
    ));
    assert_eq!(get(&mut dom, &first, "Element", "localName").string(), "a");
    let last = rc(get(
        &mut dom,
        &fragment,
        "DocumentFragment",
        "lastElementChild",
    ));
    assert_eq!(get(&mut dom, &last, "Element", "localName").string(), "b");
    assert_eq!(
        get(&mut dom, &fragment, "Node", "textContent").string(),
        "t"
    );
}
