//! Markup in and out of the tree (HTML's "DOM parsing and serialization"):
//! `innerHTML`, `outerHTML` and `insertAdjacentHTML`. The parsing (HTML's
//! fragment parsing algorithm, in the member's operation) and serializing
//! are `html`'s; this is the DOM side, Blink's `Element` members and
//! `editing/serializers/serialization.cc`.
//!
//! An XML document's markup would be XML's (its parser and serializer),
//! which machopo doesn't have: the members there are a `NotSupportedError`.

use dom::{NodeKind, NodeRef, QualName, names};

use crate::exception::{DomException, ExceptionName};
use crate::typed::DomStr;
use crate::{ApiCx, ApiError};

/// HTML's "scripting is enabled" for `node`: its document has a window (a
/// browsing context). It decides whether `<noscript>` holds markup or raw
/// text, both ways.
fn scripting_enabled<'gc>(cx: &ApiCx<'_, 'gc>, node: NodeRef<'gc>) -> bool {
    cx.window_of(cx.document_of(node)).is_some()
}

fn error(name: ExceptionName, message: String) -> ApiError {
    ApiError::Dom(DomException { name, message })
}

/// The `NotSupportedError` of markup in an XML document (`what` is the
/// member); `Ok` in an HTML one.
pub(crate) fn html_only<'gc>(
    cx: &ApiCx<'_, 'gc>,
    node: NodeRef<'gc>,
    what: &str,
) -> Result<(), ApiError> {
    if cx.is_html_document(cx.document_of(node)) {
        return Ok(());
    }
    Err(error(
        ExceptionName::NotSupported,
        format!("{what}: markup in an XML document needs an XML parser, which there isn't"),
    ))
}

/// HTML's fragment parsing algorithm with `context`, into a new fragment
/// of `context`'s node document; its `<script>`s never run.
pub(crate) fn parse_fragment<'gc>(
    cx: &ApiCx<'_, 'gc>,
    context: NodeRef<'gc>,
    markup: &str,
) -> NodeRef<'gc> {
    html::parser::parse_fragment_in(cx, context, markup, scripting_enabled(cx, context))
}

/// A new `<body>` of `node`'s document: the context of markup inserted
/// where there is no element to be its context.
pub(crate) fn body_of<'gc>(cx: &ApiCx<'_, 'gc>, node: NodeRef<'gc>) -> NodeRef<'gc> {
    cx.create_element(cx.document_of(node), QualName::html("body"))
}

/// `innerHTML`'s getter: the HTML fragment serialization of `this`'s
/// children (a `<template>`'s contents).
pub(crate) fn inner_html<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    html_only(cx, this, "innerHTML")?;
    let markup = html::parser::serialize_children(cx, this, scripting_enabled(cx, this));
    Ok(DomStr::fresh(markup))
}

/// `innerHTML`'s setter: `value` parsed with `this` as the context, and
/// `this`'s children (a `<template>`'s contents) replaced with it.
pub(crate) fn set_inner_html<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    html_only(cx, this, "innerHTML")?;
    let fragment = parse_fragment(cx, this, value);
    let parent = cx.template_content(this).unwrap_or(this);
    cx.replace_all(parent, Some(fragment));
    Ok(())
}

/// `ShadowRoot`'s `innerHTML` setter: `value` parsed with its host as
/// the context, and `root`'s children replaced with it.
pub(crate) fn set_shadow_inner_html<'gc>(
    cx: &ApiCx<'_, 'gc>,
    root: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    let host = cx.shadow_host(root).expect("a shadow root has its host");
    html_only(cx, host, "innerHTML")?;
    let fragment = parse_fragment(cx, host, value);
    cx.replace_all(root, Some(fragment));
    Ok(())
}

/// `outerHTML`'s getter: the serialization of `this` itself.
pub(crate) fn outer_html<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    html_only(cx, this, "outerHTML")?;
    let markup = html::parser::serialize_node(cx, this, scripting_enabled(cx, this));
    Ok(DomStr::fresh(markup))
}

/// `outerHTML`'s setter: nothing with no parent; a document parent is a
/// `NoModificationAllowedError`; else `value` parsed with the parent as
/// the context (a new `<body>` for a fragment) replaces `this`.
pub(crate) fn set_outer_html<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    html_only(cx, this, "outerHTML")?;
    let Some(parent) = cx.parent(this) else {
        return Ok(());
    };
    let context = match cx.kind(parent) {
        NodeKind::Document => {
            return Err(error(
                ExceptionName::NoModificationAllowed,
                "outerHTML: the element's parent is a document".to_owned(),
            ));
        }
        NodeKind::DocumentFragment => body_of(cx, this),
        _ => parent,
    };
    let fragment = parse_fragment(cx, context, value);
    cx.replace_with_new(this, fragment);
    Ok(())
}

/// Where `insertAdjacentHTML` puts what it parses.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Position {
    BeforeBegin,
    AfterBegin,
    BeforeEnd,
    AfterEnd,
}

/// `insertAdjacentHTML(position, string)`: `string` parsed with `this`
/// (`afterbegin`, `beforeend`) or its parent (`beforebegin`, `afterend`;
/// none or a document is a `NoModificationAllowedError`) as the context,
/// a new `<body>` for one that isn't an element or is an HTML document's
/// `<html>`; then inserted there (DOM's "pre-insert": it may throw). Any
/// other position is a `SyntaxError`.
pub(crate) fn insert_adjacent_html<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    position: &str,
    string: &str,
) -> Result<(), ApiError> {
    let position = match position.to_ascii_lowercase().as_str() {
        "beforebegin" => Position::BeforeBegin,
        "afterbegin" => Position::AfterBegin,
        "beforeend" => Position::BeforeEnd,
        "afterend" => Position::AfterEnd,
        _ => {
            return Err(error(
                ExceptionName::Syntax,
                format!("insertAdjacentHTML: '{position}' is not a valid position"),
            ));
        }
    };
    html_only(cx, this, "insertAdjacentHTML")?;
    let context = if matches!(position, Position::BeforeBegin | Position::AfterEnd) {
        match cx.parent(this) {
            Some(parent) if cx.kind(parent) != NodeKind::Document => parent,
            _ => {
                return Err(error(
                    ExceptionName::NoModificationAllowed,
                    "insertAdjacentHTML: the element has no parent element".to_owned(),
                ));
            }
        }
    } else {
        this
    };
    let is_html_root = cx.element_name(context).is_some_and(|name| {
        name.namespace == names::HTML_NAMESPACE && name.local == names::local::HTML
    }) && cx.is_html_document(cx.document_of(context));
    let context = if cx.kind(context) != NodeKind::Element || is_html_root {
        body_of(cx, this)
    } else {
        context
    };
    let fragment = parse_fragment(cx, context, string);
    let parent = cx.parent(this);
    match position {
        Position::BeforeBegin => {
            cx.pre_insert(parent.expect("checked"), fragment, Some(this))?;
        }
        Position::AfterBegin => {
            cx.pre_insert(this, fragment, cx.first_child(this))?;
        }
        Position::BeforeEnd => {
            cx.pre_insert(this, fragment, None)?;
        }
        Position::AfterEnd => {
            cx.pre_insert(parent.expect("checked"), fragment, cx.next_sibling(this))?;
        }
    }
    Ok(())
}

/// `setHTMLUnsafe(html)` (HTML's "unsafely set HTML"): `html` parsed with
/// `this` as the context, its `<template shadowrootmode>`s attaching
/// declarative shadow roots, and `this`'s children (a `<template>`'s
/// contents) replaced with it. An XML document's isn't supported.
pub(crate) fn set_html_unsafe<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    html: &str,
) -> Result<(), ApiError> {
    html_only(cx, this, "setHTMLUnsafe")?;
    let fragment =
        html::parser::parse_fragment_unsafe_in(cx, this, html, scripting_enabled(cx, this));
    let parent = cx.template_content(this).unwrap_or(this);
    cx.replace_all(parent, Some(fragment));
    Ok(())
}

/// `ShadowRoot`'s `setHTMLUnsafe(html)`: as `Element`'s, its host the
/// context, its own children replaced.
pub(crate) fn set_shadow_html_unsafe<'gc>(
    cx: &ApiCx<'_, 'gc>,
    root: NodeRef<'gc>,
    html: &str,
) -> Result<(), ApiError> {
    let host = cx.shadow_host(root).expect("a shadow root has its host");
    html_only(cx, host, "setHTMLUnsafe")?;
    let fragment =
        html::parser::parse_fragment_unsafe_in(cx, host, html, scripting_enabled(cx, host));
    cx.replace_all(root, Some(fragment));
    Ok(())
}

/// `Document.parseHTMLUnsafe(html)`: a new HTML document parsed from
/// `html`, scripting disabled (it has no browsing context), its
/// `<template shadowrootmode>`s attaching declarative shadow roots.
pub(crate) fn parse_html_unsafe<'gc, O: crate::typed::OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    html: &str,
) -> Result<NodeRef<'gc>, ApiError> {
    let document = html::parser::parse_document_unsafe_in(cx, html);
    cx.set_document_relevant_window(document, op.current_window(cx));
    Ok(document)
}

/// `getHTML(options)`: the HTML fragment serialization of `this`'s children
/// (a `<template>`'s contents), the shadow roots of `this` and its
/// descendants written as `<template shadowrootmode>`s if serializable
/// (with `serializableShadowRoots`) or among `shadowRoots`.
pub(crate) fn get_html<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    options: crate::typed::GetHtmlOptions<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    let context = cx.shadow_host(this).unwrap_or(this);
    html_only(cx, context, "getHTML")?;
    let shadows = html::parser::ShadowRoots {
        serializable: options.serializable_shadow_roots,
        listed: &options.shadow_roots,
    };
    let markup =
        html::parser::serialize_children_with(cx, this, scripting_enabled(cx, context), shadows);
    Ok(DomStr::fresh(markup))
}
