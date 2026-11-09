//! `DOMImplementation` (https://dom.spec.whatwg.org/#interface-domimplementation):
//! `document.implementation`, which makes doctypes and other documents
//! (Blink's `DOMImplementation`). The documents it makes have no browsing
//! context: no window, nothing rendered, no script run in them; their
//! wrappers are of the current realm.

use dom::names::{self, local};
use dom::{DomImplementation, NodeRef};
use elise::Gc;

use crate::exception::ExceptionName;
use crate::names_check::{self, NameContext};
use crate::{ApiCx, ApiError};

/// `createDocumentType(name, publicId, systemId)`: a doctype of its
/// document; an `InvalidCharacterError` for a name with ASCII whitespace,
/// NULL or `>`.
pub(crate) fn create_document_type<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, DomImplementation>,
    name: &str,
    public_id: &str,
    system_id: &str,
) -> Result<NodeRef<'gc>, ApiError> {
    if !names_check::is_valid_doctype_name(name) {
        return Err(ApiError::dom(ExceptionName::InvalidCharacter));
    }
    let document = cx.dom_implementation_document(this);
    Ok(cx.create_doctype(document, name, public_id, system_id))
}

/// `createDocument(namespace, qualifiedName, doctype)`: a new XML document
/// (`XMLDocument`) with `doctype` and an element of that name (none for
/// the empty name), of `this`'s document's relevant global; its content
/// type by the namespace (XHTML's `application/xhtml+xml`, SVG's
/// `image/svg+xml`, else `application/xml`).
pub(crate) fn create_document<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, DomImplementation>,
    namespace: Option<&str>,
    qualified_name: &str,
    doctype: Option<NodeRef<'gc>>,
) -> Result<NodeRef<'gc>, ApiError> {
    let name = if qualified_name.is_empty() {
        None
    } else {
        Some(names_check::validate_and_extract(
            namespace,
            qualified_name,
            NameContext::Element,
        )?)
    };
    let document = cx.create_document(false);
    cx.set_document_relevant_window(
        document,
        cx.document_relevant_window(cx.dom_implementation_document(this)),
    );
    cx.set_xml_document_interface(document);
    cx.set_document_content_type(
        document,
        match namespace {
            Some("http://www.w3.org/1999/xhtml") => "application/xhtml+xml",
            Some("http://www.w3.org/2000/svg") => "image/svg+xml",
            _ => "application/xml",
        },
    );
    if let Some(doctype) = doctype {
        cx.append_child(document, doctype)?;
    }
    if let Some(name) = name {
        let element = cx.create_element(document, name);
        cx.append_child(document, element)?;
    }
    Ok(document)
}

/// `createHTMLDocument(title)`: a new HTML document of `this`'s
/// document's relevant global with a doctype, and `<html>` holding
/// `<head>` (with a `<title>` of `title` if it is given) and `<body>`.
pub(crate) fn create_html_document<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, DomImplementation>,
    title: Option<&str>,
) -> Result<NodeRef<'gc>, ApiError> {
    let document = cx.create_document(true);
    cx.set_document_relevant_window(
        document,
        cx.document_relevant_window(cx.dom_implementation_document(this)),
    );
    let doctype = cx.create_doctype(document, "html", "", "");
    cx.insert(document, doctype, None);
    let element = |local| {
        cx.create_element_named(
            document,
            dom::NameIds {
                namespace: names::HTML_NAMESPACE,
                prefix: names::NONE,
                local,
            },
        )
    };
    let html = element(local::HTML);
    cx.insert(document, html, None);
    let head = element(local::HEAD);
    cx.insert(html, head, None);
    if let Some(title) = title {
        let element = element(local::TITLE);
        cx.insert(head, element, None);
        cx.insert(element, cx.create_text(document, title), None);
    }
    cx.insert(html, element(local::BODY), None);
    Ok(document)
}

/// `hasFeature()`: true (historical).
pub(crate) fn has_feature<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, DomImplementation>,
) -> Result<bool, ApiError> {
    Ok(true)
}
