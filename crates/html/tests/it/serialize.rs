//! HTML's fragment serializing: `serialize_children` (`innerHTML`) and
//! `serialize_node` (`outerHTML`).

use dom::{Cx, Dom, NodeRef, QualName};

const HTML: &str = "http://www.w3.org/1999/xhtml";
const SVG: &str = "http://www.w3.org/2000/svg";
const XMLNS: &str = "http://www.w3.org/2000/xmlns/";

/// Parses `input` as a `<div>`'s contents and serializes them back.
fn round_trip(input: &str, scripting: bool) -> String {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let context = cx.create_element(document, QualName::html("div"));
        let fragment = html::parser::parse_fragment_in(cx, context, input, scripting);
        html::parser::serialize_children(cx, fragment, scripting)
    })
}

/// A name in `namespace` (none if empty), with `prefix`.
fn name<'a>(namespace: &'a str, prefix: Option<&'a str>, local: &'a str) -> QualName<'a> {
    QualName {
        namespace: (!namespace.is_empty()).then_some(namespace),
        prefix,
        local,
    }
}

/// Builds with `build` and serializes what it returns: its children and
/// itself.
fn serialized(build: impl for<'gc> FnOnce(&Cx<'_, 'gc>) -> NodeRef<'gc>) -> (String, String) {
    serialized_with(true, build)
}

fn serialized_with(
    scripting: bool,
    build: impl for<'gc> FnOnce(&Cx<'_, 'gc>) -> NodeRef<'gc>,
) -> (String, String) {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let node = build(cx);
        (
            html::parser::serialize_children(cx, node, scripting),
            html::parser::serialize_node(cx, node, scripting),
        )
    })
}

/// A `<name>` (in `namespace`) holding the text `text`, in a new document.
fn element_with_text<'gc>(
    cx: &Cx<'_, 'gc>,
    namespace: &str,
    local: &str,
    text: &str,
) -> NodeRef<'gc> {
    let document = cx.create_document(true);
    let element = cx.create_element(document, name(namespace, None, local));
    let text = cx.create_text(document, text);
    cx.append_child(element, text).unwrap();
    element
}

#[test]
fn serializes_elements_text_and_comments() {
    let markup = "<p class=\"a\" id=\"b\">one <i>two</i></p><!--three--><ul><li>four</li></ul>";
    assert_eq!(round_trip(markup, true), markup);
}

/// Text escapes `&`, U+00A0, `<` and `>`; an attribute value escapes `&`,
/// U+00A0, `"`, `<` and `>`, but not `'`.
#[test]
fn escapes_text_and_attribute_values() {
    assert_eq!(
        round_trip(
            "<p title='a&amp;b&quot;c&lt;d&gt;e&nbsp;f&apos;'>1 &amp; 2 &lt; 3 &gt; 0&nbsp;\"'</p>",
            true
        ),
        "<p title=\"a&amp;b&quot;c&lt;d&gt;e&nbsp;f'\">1 &amp; 2 &lt; 3 &gt; 0&nbsp;\"'</p>"
    );
}

/// The text of HTML `<script>`, `<style>`, `<xmp>`, `<iframe>`, `<noembed>`,
/// `<noframes>` and `<plaintext>` is written as it is; `<noscript>`'s only
/// with scripting enabled.
#[test]
fn raw_text_elements_are_not_escaped() {
    for name in ["script", "style", "xmp", "iframe", "noembed", "noframes"] {
        let markup = format!("<{name}>a < b && c</{name}>");
        assert_eq!(round_trip(&markup, true), markup);
    }
    assert_eq!(
        round_trip("<plaintext>a < b", true),
        "<plaintext>a < b</plaintext>"
    );
    for scripting in [true, false] {
        let (inner, outer) = serialized_with(scripting, |cx| {
            element_with_text(cx, HTML, "noscript", "a < b")
        });
        let text = if scripting { "a < b" } else { "a &lt; b" };
        assert_eq!(
            (inner.as_str(), outer.as_str()),
            (text, &*format!("<noscript>{text}</noscript>"))
        );
    }
}

/// A `<template>` serializes its contents (its children are its contents'
/// in the parsed tree).
#[test]
fn a_template_serializes_its_contents() {
    let markup = "<template><b>x</b><template><i>y</i></template></template>";
    assert_eq!(round_trip(markup, true), markup);
    let (inner, _) = serialized(|cx| {
        let document = cx.create_document(true);
        let context = cx.create_element(document, QualName::html("div"));
        let fragment =
            html::parser::parse_fragment_in(cx, context, "<template><b>x</b></template>", true);
        cx.first_child(fragment).unwrap()
    });
    assert_eq!(inner, "<b>x</b>");
}

/// A void element has no end tag, and nothing inside it is written: its
/// `innerHTML` is empty even if script gave it children.
#[test]
fn void_elements_have_no_contents() {
    assert_eq!(
        round_trip("<p>a<br>b<img src=x><input></p><hr><col>", true),
        "<p>a<br>b<img src=\"x\"><input></p><hr>"
    );
    let (inner, outer) = serialized(|cx| {
        let document = cx.create_document(true);
        let br = cx.create_element(document, QualName::html("br"));
        let text = cx.create_text(document, "hidden");
        cx.append_child(br, text).unwrap();
        let i = cx.create_element(document, QualName::html("i"));
        cx.append_child(br, i).unwrap();
        br
    });
    assert_eq!((inner.as_str(), outer.as_str()), ("", "<br>"));
    // A `br` in another namespace isn't void.
    let (inner, outer) = serialized(|cx| element_with_text(cx, SVG, "br", "shown"));
    assert_eq!(
        (inner.as_str(), outer.as_str()),
        ("shown", "<br>shown</br>")
    );
}

/// HTML, SVG and MathML elements are written by their local name (case
/// kept), others by their qualified name. Attributes in no namespace and
/// the XML, XMLNS and XLink ones as HTML says; others by qualified name.
#[test]
fn names_follow_their_namespace() {
    assert_eq!(
        round_trip(
            "<svg viewBox=\"0 0 1 1\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"><foreignObject><a xlink:href=\"#x\" xml:lang=\"en\"></a></foreignObject></svg><math><mi>x</mi></math>",
            true
        ),
        "<svg viewBox=\"0 0 1 1\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"><foreignObject><a xlink:href=\"#x\" xml:lang=\"en\"></a></foreignObject></svg><math><mi>x</mi></math>"
    );
    let (_, outer) = serialized(|cx| {
        let document = cx.create_document(true);
        let element = cx.create_element(document, name("urn:x", Some("p"), "thing"));
        for (attribute, value) in [
            (name("", None, "plain"), "1"),
            (name("urn:y", Some("q"), "other"), "2"),
            (name("urn:y", None, "bare"), "3"),
            (name(XMLNS, Some("xmlns"), "q"), "urn:y"),
            (name(XMLNS, None, "xmlns"), "urn:x"),
        ] {
            cx.set_attribute_ns(element, attribute, value);
        }
        let child = cx.create_element(document, name(HTML, Some("h"), "span"));
        cx.append_child(element, child).unwrap();
        element
    });
    assert_eq!(
        outer,
        "<p:thing plain=\"1\" q:other=\"2\" bare=\"3\" xmlns:q=\"urn:y\" xmlns=\"urn:x\"><span></span></p:thing>"
    );
}

/// The text inside a non-HTML element named like a raw text one (an SVG
/// `<style>`) is escaped, whether it is the node serialized or a parent in
/// it.
#[test]
fn only_html_raw_text_elements_are_raw() {
    let (inner, outer) = serialized(|cx| element_with_text(cx, SVG, "style", "a < b"));
    assert_eq!(
        (inner.as_str(), outer.as_str()),
        ("a &lt; b", "<style>a &lt; b</style>")
    );
    let (inner, _) = serialized(|cx| element_with_text(cx, HTML, "style", "a < b"));
    assert_eq!(inner, "a < b");
    // Inside an HTML element, an SVG `<style>`'s text is still escaped.
    let (inner, _) = serialized(|cx| {
        let div = element_with_text(cx, HTML, "div", "");
        let svg_style = element_with_text(cx, SVG, "style", "c < d");
        cx.append_child(div, svg_style).unwrap();
        div
    });
    assert_eq!(inner, "<style>c &lt; d</style>");
}

/// A document's doctype, processing instructions and CDATA sections (text
/// to HTML's serializer).
#[test]
fn serializes_doctypes_processing_instructions_and_cdata() {
    let mut dom = Dom::new();
    let serialized = dom.mutate(|cx| {
        let document = cx.create_document(true);
        let doctype = cx.create_doctype(document, "html", "", "");
        cx.append_child(document, doctype).unwrap();
        let pi = cx.create_processing_instruction(document, "t", "d e");
        cx.append_child(document, pi).unwrap();
        let html = cx.create_element(document, QualName::html("html"));
        cx.append_child(document, html).unwrap();
        let xml = cx.create_document(false);
        let cdata = cx.create_cdata_section(xml, "a<b");
        cx.append_child(html, cdata).unwrap();
        html::parser::serialize_children(cx, document, true)
    });
    assert_eq!(serialized, "<!DOCTYPE html><?t d e><html>a&lt;b</html>");
}
