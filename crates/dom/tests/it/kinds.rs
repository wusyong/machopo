//! The node kinds besides documents, elements and texts: comments,
//! processing instructions, doctypes and fragments; a document's quirks
//! mode, a script's flags, a template's contents; where each kind may go
//! (DOM's "ensure pre-insertion validity"), a fragment's insertion, and
//! cloning.

use dom::{Dom, DomError, NodeKind, QualName, QuirksMode, ScriptFlags};

use crate::children;

/// A node is still 56 bytes, a 64-byte slot with its header, with its
/// flag word and its id: its name is one number.
#[test]
fn a_node_fits_its_slot() {
    assert_eq!(std::mem::size_of::<dom::Node>(), 56);
}

/// `nodeType` of every kind of node: the nine DOM gives (a CDATA section
/// is a text node whose type is its own).
#[test]
fn every_kind_has_its_node_type() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let element = cx.create_element(document, QualName::html("p"));
        cx.set_attribute(element, "title", "t");
        let types = [
            element,
            cx.attr_node(element, "title")
                .expect("its attribute's node"),
            cx.create_text(document, "text"),
            cx.create_cdata_section(document, "cdata"),
            cx.create_processing_instruction(document, "target", "data"),
            cx.create_comment(document, "comment"),
            document,
            cx.create_doctype(document, "html", "", ""),
            cx.create_document_fragment(document),
        ]
        .map(|node| cx.node_type(node));
        assert_eq!(types, [1, 2, 3, 4, 7, 8, 9, 10, 11]);
    });
}

#[test]
fn each_kind_has_its_type_name_and_data() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let comment = cx.create_comment(document, "a comment");
        let pi = cx.create_processing_instruction(document, "xml-stylesheet", "href=x");
        let doctype = cx.create_doctype(document, "html", "-//W3C//DTD", "about:legacy");
        let fragment = cx.create_document_fragment(document);
        let kinds =
            [comment, pi, doctype, fragment].map(|node| (cx.kind(node), cx.node_type(node)));
        assert_eq!(
            kinds,
            [
                (NodeKind::Comment, 8),
                (NodeKind::ProcessingInstruction, 7),
                (NodeKind::DocumentType, 10),
                (NodeKind::DocumentFragment, 11),
            ]
        );
        let names = [comment, pi, doctype, fragment].map(|node| cx.node_name(node).to_string());
        assert_eq!(
            names,
            ["#comment", "xml-stylesheet", "html", "#document-fragment"]
        );
        assert_eq!(cx.data(comment).unwrap(), "a comment");
        assert_eq!(cx.data(pi).unwrap(), "href=x");
        assert_eq!(cx.target(pi).unwrap(), "xml-stylesheet");
        assert_eq!(cx.target(comment), None);
        let ids = cx.doctype(doctype).unwrap();
        assert_eq!(
            (ids.name, ids.public_id, ids.system_id),
            ("html".into(), "-//W3C//DTD".into(), "about:legacy".into())
        );
        assert_eq!(cx.doctype(comment), None);
        assert!(
            NodeKind::Comment.is_character_data() && !NodeKind::DocumentType.is_character_data()
        );
        // Each kind's owner is the document.
        for node in [comment, pi, doctype, fragment] {
            assert!(elise::Gc::ptr_eq(
                cx.owner_document(node).unwrap(),
                document
            ));
        }
    });
}

/// `textContent`: a document's and a doctype's none; a comment's and a
/// processing instruction's data; a fragment's descendant texts, not its
/// comments.
#[test]
fn text_content_by_kind() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let doctype = cx.create_doctype(document, "html", "", "");
        let fragment = cx.create_document_fragment(document);
        let p = cx.create_element(document, QualName::html("p"));
        cx.insert(fragment, cx.create_text(document, "a"), None);
        cx.insert(fragment, cx.create_comment(document, "not this"), None);
        cx.insert(fragment, p, None);
        cx.insert(p, cx.create_text(document, "b"), None);
        assert_eq!(cx.text_content(fragment).unwrap(), "ab");
        assert_eq!(cx.text_content(document), None);
        assert_eq!(cx.text_content(doctype), None);
        let comment = cx.create_comment(document, "c");
        assert_eq!(cx.text_content(comment).unwrap(), "c");
        cx.set_text_content(comment, "d");
        assert_eq!(cx.data(comment).unwrap(), "d");
        cx.set_text_content(fragment, "e");
        assert_eq!(children(cx, fragment), ["#text"]);
        cx.set_text_content(doctype, "ignored");
        assert_eq!(cx.doctype(doctype).unwrap().name, "html");
    });
}

/// A document's quirks mode, an element's script flags and integration
/// point: each its own bits, the kind and HTML-ness kept.
#[test]
fn a_documents_and_an_elements_flags() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        assert_eq!(cx.quirks_mode(document), QuirksMode::NoQuirks);
        for mode in [
            QuirksMode::Quirks,
            QuirksMode::LimitedQuirks,
            QuirksMode::NoQuirks,
        ] {
            cx.set_quirks_mode(document, mode);
            assert_eq!(cx.quirks_mode(document), mode);
            assert!(cx.is_html_document(document));
            assert_eq!(cx.kind(document), NodeKind::Document);
        }
        let xml = cx.create_document(false);
        cx.set_quirks_mode(xml, QuirksMode::Quirks);
        assert!(!cx.is_html_document(xml));

        // One script makes is non-blocking.
        let script = cx.create_element(document, QualName::html("script"));
        assert_eq!(
            cx.script_flags(script),
            ScriptFlags {
                non_blocking: true,
                ..ScriptFlags::default()
            }
        );
        let flags = ScriptFlags {
            parser_inserted: true,
            already_started: false,
            non_blocking: false,
        };
        cx.set_script_flags(script, flags);
        assert_eq!(cx.script_flags(script), flags);
        cx.set_script_flags(
            script,
            ScriptFlags {
                parser_inserted: false,
                already_started: true,
                non_blocking: true,
            },
        );
        assert!(
            cx.script_flags(script).already_started
                && !cx.script_flags(script).parser_inserted
                && cx.script_flags(script).non_blocking
        );
        // The parser's isn't; nor is another element anything but a
        // `<link>`'s parser-inserted.
        let parsers = cx.create_element_by_parser(document, QualName::html("script"));
        assert_eq!(cx.script_flags(parsers), ScriptFlags::default());
        let annotation = cx.create_element(
            document,
            QualName {
                namespace: Some("http://www.w3.org/1998/Math/MathML"),
                prefix: None,
                local: "annotation-xml",
            },
        );
        assert!(!cx.is_integration_point(annotation));
        cx.set_integration_point(annotation);
        assert!(cx.is_integration_point(annotation));
        assert_eq!(cx.script_flags(annotation), ScriptFlags::default());
        assert_eq!(cx.kind(script), NodeKind::Element);
        assert_eq!(cx.node_name(script), "SCRIPT");
    });
}

/// An HTML `<template>` has its contents: a fragment of its document's
/// inert template document (one a document, its own for itself), moved to
/// the new document's as the template is adopted; another element has
/// none.
#[test]
fn a_templates_contents_are_its_template_documents() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let template = cx.create_element(document, QualName::html("template"));
        let content = cx.template_content(template).unwrap();
        assert_eq!(cx.kind(content), NodeKind::DocumentFragment);
        let inert = cx.owner_document(content).unwrap();
        assert!(!elise::Gc::ptr_eq(inert, document));
        assert!(elise::Gc::ptr_eq(cx.template_document(document), inert));
        assert!(elise::Gc::ptr_eq(cx.template_document(inert), inert));
        assert!(cx.is_html_document(inert));
        let second = cx.create_element(document, QualName::html("template"));
        assert!(elise::Gc::ptr_eq(
            cx.owner_document(cx.template_content(second).unwrap())
                .unwrap(),
            inert
        ));
        assert!(
            cx.template_content(cx.create_element(document, QualName::html("div")))
                .is_none()
        );
        let svg_template = cx.create_element(
            document,
            QualName {
                namespace: Some("http://www.w3.org/2000/svg"),
                prefix: None,
                local: "template",
            },
        );
        assert!(cx.template_content(svg_template).is_none());
        // An XML document's template document is an XML one.
        let xml = cx.create_document(false);
        let xml_template = cx.create_element(xml, QualName::html("template"));
        let xml_inert = cx
            .owner_document(cx.template_content(xml_template).unwrap())
            .unwrap();
        assert!(!cx.is_html_document(xml_inert));

        let other = cx.create_document(true);
        let body = cx.create_element(other, QualName::html("body"));
        cx.insert(body, template, None);
        assert!(elise::Gc::ptr_eq(
            cx.owner_document(template).unwrap(),
            other
        ));
        let moved = cx.owner_document(content).unwrap();
        assert!(elise::Gc::ptr_eq(moved, cx.template_document(other)));
        assert!(!elise::Gc::ptr_eq(moved, inert));
    });
}

/// Where each kind may go: a doctype only in a document, before its
/// element, once; a text not in a document; a fragment in a document only
/// with at most one element child and no text; a comment anywhere.
#[test]
fn each_kind_goes_where_dom_lets_it() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let html = cx.create_element(document, QualName::html("html"));
        let doctype = || cx.create_doctype(document, "html", "", "");
        let err = Err(DomError::HierarchyRequest);
        assert_eq!(
            cx.pre_insert(html, doctype(), None).map(drop),
            err,
            "a doctype in an element"
        );
        assert_eq!(
            cx.pre_insert(document, cx.create_text(document, "t"), None)
                .map(drop),
            err
        );
        let comment = cx.create_comment(document, "c");
        let in_comment = cx.create_comment(document, "d");
        assert_eq!(
            cx.pre_insert(comment, in_comment, None).map(drop),
            err,
            "a node in a comment"
        );
        assert!(
            cx.pre_insert(document, comment, None).is_ok(),
            "a comment in a document"
        );
        cx.pre_insert(document, html, None).unwrap();
        assert_eq!(
            cx.pre_insert(document, doctype(), None).map(drop),
            err,
            "a doctype after the element"
        );
        let first = doctype();
        cx.pre_insert(document, first, Some(comment)).unwrap();
        assert_eq!(
            cx.pre_insert(document, doctype(), Some(comment)).map(drop),
            err,
            "a second doctype"
        );
        let other = cx.create_element(document, QualName::html("body"));
        assert_eq!(
            cx.pre_insert(document, other, None).map(drop),
            err,
            "a second element"
        );
        assert_eq!(children(cx, document), ["html", "#comment", "HTML"]);

        // An element before the doctype.
        let fresh = cx.create_document(true);
        let doctype = cx.create_doctype(fresh, "html", "", "");
        cx.pre_insert(fresh, doctype, None).unwrap();
        let element = cx.create_element(fresh, QualName::html("html"));
        assert_eq!(cx.pre_insert(fresh, element, Some(doctype)).map(drop), err);
        let before = cx.create_comment(fresh, "before");
        cx.pre_insert(fresh, before, Some(doctype)).unwrap();
        assert_eq!(
            cx.pre_insert(fresh, element, Some(before)).map(drop),
            err,
            "an element before a node before the doctype"
        );
        cx.pre_insert(fresh, element, None).unwrap();
        let fragment = cx.create_document_fragment(fresh);
        cx.insert(
            fragment,
            cx.create_element(fresh, QualName::html("body")),
            None,
        );
        assert_eq!(
            cx.pre_insert(fresh, fragment, None).map(drop),
            err,
            "a fragment's element beside the element"
        );
        assert_eq!(children(cx, fresh), ["#comment", "html", "HTML"]);

        // Fragments into a document: one element, no text.
        let fresh = cx.create_document(true);
        let fragment = cx.create_document_fragment(fresh);
        cx.insert(
            fragment,
            cx.create_element(fresh, QualName::html("a")),
            None,
        );
        cx.insert(
            fragment,
            cx.create_element(fresh, QualName::html("b")),
            None,
        );
        assert_eq!(
            cx.pre_insert(fresh, fragment, None).map(drop),
            err,
            "two elements"
        );
        let fragment = cx.create_document_fragment(fresh);
        cx.insert(fragment, cx.create_text(fresh, "t"), None);
        assert_eq!(
            cx.pre_insert(fresh, fragment, None).map(drop),
            err,
            "a text"
        );
        let fragment = cx.create_document_fragment(fresh);
        cx.insert(fragment, cx.create_comment(fresh, "c"), None);
        cx.insert(
            fragment,
            cx.create_element(fresh, QualName::html("html")),
            None,
        );
        cx.pre_insert(fresh, fragment, None).unwrap();
        assert_eq!(children(cx, fresh), ["#comment", "HTML"]);
        assert!(
            cx.first_child(fragment).is_none(),
            "the fragment left empty"
        );
    });
}

/// A fragment inserted in an element puts its children there, in order,
/// before the reference child.
#[test]
fn a_fragments_children_are_inserted_in_order() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let div = cx.create_element(document, QualName::html("div"));
        let last = cx.create_element(document, QualName::html("i"));
        cx.append_child(div, last).unwrap();
        let fragment = cx.create_document_fragment(document);
        for name in ["a", "b", "c"] {
            cx.insert(
                fragment,
                cx.create_element(document, QualName::html(name)),
                None,
            );
        }
        assert!(elise::Gc::ptr_eq(
            cx.pre_insert(div, fragment, Some(last)).unwrap(),
            fragment
        ));
        assert_eq!(children(cx, div), ["A", "B", "C", "I"]);
        assert!(cx.first_child(fragment).is_none());
        assert!(elise::Gc::ptr_eq(
            cx.parent(cx.first_child(div).unwrap()).unwrap(),
            div
        ));

        // Another document's fragment: its children are adopted.
        let other = cx.create_document(true);
        let fragment = cx.create_document_fragment(other);
        let from_other = cx.create_element(other, QualName::html("p"));
        cx.insert(fragment, from_other, None);
        cx.pre_insert(div, fragment, None).unwrap();
        assert!(elise::Gc::ptr_eq(
            cx.owner_document(from_other).unwrap(),
            document
        ));
    });
}

#[test]
fn appended_data_continues_a_text() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let text = cx.create_text(document, "a");
        cx.append_data(text, "bé");
        assert_eq!(cx.data(text).unwrap(), "abé");
    });
}

/// A clone: the node's kind, name, attributes or data; its descendants
/// and a template's contents if deep; in the given document.
#[test]
fn a_clone_copies_the_node_and_if_deep_its_descendants() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let div = cx.create_element(document, QualName::html("div"));
        cx.set_attribute(div, "class", "x");
        cx.append_child(div, cx.create_text(document, "t")).unwrap();
        cx.append_child(div, cx.create_comment(document, "c"))
            .unwrap();
        let template = cx.create_element(document, QualName::html("template"));
        cx.insert(
            cx.template_content(template).unwrap(),
            cx.create_element(document, QualName::html("b")),
            None,
        );
        cx.append_child(div, template).unwrap();

        let shallow = cx.clone_node(div, None, false);
        assert_eq!(cx.node_name(shallow), "DIV");
        assert_eq!(cx.get_attribute(shallow, "class").unwrap(), "x");
        assert!(cx.first_child(shallow).is_none());

        let deep = cx.clone_node(div, None, true);
        assert_eq!(children(cx, deep), ["#text", "#comment", "TEMPLATE"]);
        assert_eq!(cx.text_content(deep).unwrap(), "t");
        let copied = cx.last_child(deep).unwrap();
        assert!(!elise::Gc::ptr_eq(copied, template));
        let content = cx.template_content(copied).unwrap();
        assert_eq!(children(cx, content), ["B"]);

        let other = cx.create_document(false);
        let moved = cx.clone_node(
            cx.create_doctype(document, "html", "p", "s"),
            Some(other),
            false,
        );
        assert!(elise::Gc::ptr_eq(cx.owner_document(moved).unwrap(), other));
        assert_eq!(cx.doctype(moved).unwrap().system_id, "s");
        let pi = cx.clone_node(
            cx.create_processing_instruction(document, "t", "d"),
            None,
            false,
        );
        cx.set_quirks_mode(document, QuirksMode::LimitedQuirks);
        let copy = cx.clone_node(document, None, false);
        assert_eq!(cx.quirks_mode(copy), QuirksMode::LimitedQuirks);
        assert!(cx.is_html_document(copy) && !elise::Gc::ptr_eq(copy, document));
        assert_eq!(
            (cx.target(pi).unwrap(), cx.data(pi).unwrap()),
            ("t".into(), "d".into())
        );
    });
}
