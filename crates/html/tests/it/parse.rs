//! The parser's own behavior, beyond what the tree-construction tests cover.

use dom::{Dom, QualName, QuirksMode};

use crate::harness::{context_named, dump, dump_fragment};

fn parse(input: &str) -> (String, QuirksMode) {
    let mut dom = Dom::new();
    let document = html::parser::parse_document(&mut dom, input, true);
    let quirks = dom.read(|cx| cx.quirks_mode(document.root().get(cx.gc)));
    (dump(&mut dom, &document), quirks)
}

#[test]
fn parses_a_document() {
    let (dump, quirks) = parse("<!DOCTYPE html><title>t</title><p class=a>one<p>two");
    assert_eq!(
        dump,
        "\
| <!DOCTYPE html>
| <html>
|   <head>
|     <title>
|       \"t\"
|   <body>
|     <p>
|       class=\"a\"
|       \"one\"
|     <p>
|       \"two\"
"
    );
    assert_eq!(quirks, QuirksMode::NoQuirks);
}

/// A processing instruction in the dump: html5lib's `<?target data>`. The
/// parser makes a comment of `<?…>` (HTML's bogus comment), so only script
/// makes one.
#[test]
fn dumps_processing_instructions() {
    let mut dom = Dom::new();
    let document = html::parser::parse_document(&mut dom, "<?x y?>", true);
    dom.mutate(|cx| {
        let document = document.root().get(cx.gc);
        let pi = cx.create_processing_instruction(document, "t", "d e");
        cx.append_child(document, pi).unwrap();
    });
    assert_eq!(
        dump(&mut dom, &document),
        "\
| <!-- ?x y? -->
| <html>
|   <head>
|   <body>
| <?t d e>
"
    );
}

/// A CDATA section in the dump: html5lib's `<![CDATA[data]]>`. Only script
/// makes one, in an XML document.
#[test]
fn dumps_cdata_sections() {
    let mut dom = Dom::new();
    let document = dom.mutate(|cx| {
        let document = cx.create_document(false);
        let root = cx.create_element(document, QualName::html("r"));
        cx.append_child(document, root).unwrap();
        let cdata = cx.create_cdata_section(document, "a<b");
        cx.append_child(root, cdata).unwrap();
        cx.root(document)
    });
    assert_eq!(
        html::parser::dump_children(&mut dom, &document),
        "| <r>\n|   <![CDATA[a<b]]>\n"
    );
}

/// Where the fragment's context is, for the form element pointer.
#[derive(Clone, Copy, PartialEq)]
enum Form {
    None,
    /// The context is in a `<form>`.
    Ancestor,
    /// The context is a `<form>`.
    Context,
    /// The context is a `<template>` in a `<form>` (the form's template).
    FormsTemplate,
}

/// Parses `input` as a fragment with a `<div>` context (or a `<form>`) in
/// a document in `quirks` mode. Returns the dump of the result, and
/// whether the context and its document were left as they were and the
/// fragment is the document's, in no tree.
fn parse_fragment(input: &str, quirks: QuirksMode, form: Form) -> (String, bool) {
    let mut dom = Dom::new();
    let name = match form {
        Form::Context => "form",
        Form::FormsTemplate => "template",
        Form::None | Form::Ancestor => "div",
    };
    let (context, document) = context_named(&mut dom, QualName::html(name), quirks);
    if matches!(form, Form::Ancestor | Form::FormsTemplate) {
        dom.mutate(|cx| {
            let context = context.get(cx.gc);
            let form = cx.create_element(cx.document_of(context), QualName::html("form"));
            cx.append_child(form, context).unwrap();
        });
    }
    let fragment = html::parser::parse_fragment(&mut dom, &context, input, true);
    let untouched = dom.read(|cx| {
        let (context, document) = (context.get(cx.gc), document.get(cx.gc));
        let fragment = fragment.root().get(cx.gc);
        cx.first_child(context).is_none()
            && cx.first_child(document).is_none()
            && cx.kind(fragment) == dom::NodeKind::DocumentFragment
            && cx.parent(fragment).is_none()
            && elise::Gc::ptr_eq(cx.document_of(fragment), document)
            && cx.quirks_mode(document) == quirks
    });
    (dump(&mut dom, &fragment), untouched)
}

/// A context that is or is in a `<form>` sets the form element pointer, so
/// a `<form>` in the fragment is a nested form, and ignored; not a
/// `<template>`'s (whatwg/html#12257, Chromium's).
#[test]
fn a_fragment_in_a_form_ignores_forms() {
    let input = "<form><i>x</i></form>";
    let (outside, _) = parse_fragment(input, QuirksMode::NoQuirks, Form::None);
    assert_eq!(outside, "| <form>\n|   <i>\n|     \"x\"\n");
    for form in [Form::Ancestor, Form::Context] {
        let (inside, untouched) = parse_fragment(input, QuirksMode::NoQuirks, form);
        assert!(untouched);
        assert_eq!(inside, "| <i>\n|   \"x\"\n");
    }
    let (template, _) = parse_fragment(input, QuirksMode::NoQuirks, Form::FormsTemplate);
    assert_eq!(template, outside);
}

/// With scripting enabled, `<noscript>` contents are raw text: a
/// `<noscript>` context's (the tokenizer starts in RAWTEXT) and a
/// `<noscript>` element's in the fragment (the tree builder's flag); with
/// it disabled, they are markup.
#[test]
fn noscript_follows_the_scripting_flag() {
    assert_eq!(
        dump_fragment("noscript", "<b>x</b>", true),
        "| \"<b>x</b>\"\n"
    );
    assert_eq!(
        dump_fragment("noscript", "<b>x</b>", false),
        "| <b>\n|   \"x\"\n"
    );
    let input = "<noscript><b>x</b></noscript>";
    assert_eq!(
        dump_fragment("div", input, true),
        "| <noscript>\n|   \"<b>x</b>\"\n"
    );
    assert_eq!(
        dump_fragment("div", input, false),
        "| <noscript>\n|   <b>\n|     \"x\"\n"
    );
}

/// HTML's "create an element for a token": a form-associated element made
/// while the form element pointer is set gets that form as its owner, with
/// the parser inserted flag, though the form isn't its ancestor
/// (`<div><form></div><input>`); not one with a `form` attribute, nor one
/// made after `</form>` cleared the pointer.
#[test]
fn associates_controls_with_the_form_element_pointer() {
    let mut dom = Dom::new();
    let document = html::parser::parse_document(
        &mut dom,
        "<div><form id=f></div><input id=a><input id=b form=x><select id=c></select>\
         <form id=g></form><input id=d>",
        true,
    );
    dom.read(|cx| {
        let document = document.root().get(cx.gc);
        let by_id = |id: &str| {
            cx.element_by_id(document, id)
                .unwrap_or_else(|| panic!("no #{id}"))
        };
        let owner = |id: &str| cx.form_owner(by_id(id)).map(|form| cx.node_id(form));
        let form = cx.node_id(by_id("f"));
        assert_eq!(owner("a"), Some(form));
        assert!(cx.script_flags(by_id("a")).parser_inserted);
        assert_eq!(owner("b"), None, "a form attribute");
        assert_eq!(owner("c"), Some(form));
        assert_eq!(owner("d"), None, "after </form>");
        assert!(!cx.script_flags(by_id("d")).parser_inserted);
    });
}
