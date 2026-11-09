//! `Document`'s members of the tree (https://dom.spec.whatwg.org/#interface-document).

use dom::names::{self, local};
use dom::{DomImplementation, DomString, NodeKind, NodeRef, QualName, QualifiedName};
use elise::Gc;

use crate::exception::ExceptionName;
use crate::names_check::{self, NameContext};
use crate::typed::{Created, DomStr};
use crate::{ApiCx, ApiError, custom_elements, is_element, lowercase};

const HTML_NAMESPACE: &str = "http://www.w3.org/1999/xhtml";

/// `readyState`: its readiness (complete for a document no parser made).
pub(crate) fn ready_state<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<crate::typed::DomStr<'gc>, ApiError> {
    let state = match cx.ready_state(this) {
        dom::DocumentReadyState::Loading => "loading",
        dom::DocumentReadyState::Interactive => "interactive",
        dom::DocumentReadyState::Complete => "complete",
    };
    Ok(crate::typed::DomStr::Owned(dom::DomString::new(state)))
}

/// `URL` and `documentURI`: its URL (`about:blank` for one with none).
pub(crate) fn url<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<crate::typed::DomStr<'gc>, ApiError> {
    Ok(crate::typed::DomStr::Owned(
        cx.document_url(this)
            .filter(|url| !url.to_str().is_empty())
            .unwrap_or_else(|| dom::DomString::new("about:blank")),
    ))
}

/// `defaultView`: its browsing context's window, if it has one.
pub(crate) fn default_view<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<elise::Gc<'gc, dom::Window>>, ApiError> {
    Ok(cx.window_of(this))
}

/// `compatMode`: `BackCompat` in quirks mode, else `CSS1Compat`.
pub(crate) fn compat_mode<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<crate::typed::DomStr<'gc>, ApiError> {
    let mode = match cx.quirks_mode(this) {
        dom::QuirksMode::Quirks => "BackCompat",
        _ => "CSS1Compat",
    };
    Ok(crate::typed::DomStr::Owned(dom::DomString::new(mode)))
}

/// `documentElement`: its element child.
pub(crate) fn document_element<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    crate::parent_node::first_element_child(cx, this)
}

/// `createElement`: the name checked, lowercased in an HTML document; an
/// HTML element there, one in no namespace in an XML document. `id` the
/// name's number if the front end found it as it is (lowercase): in an
/// HTML document the element made by it, nothing interned again (but for
/// a name with a hyphen, which may be a custom element's).
pub(crate) fn create_element<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    (local_name, id): (&str, Option<dom::NameId>),
) -> Result<Created<'gc>, ApiError> {
    if !names_check::is_valid_element_local_name(local_name) {
        return Err(ApiError::dom(ExceptionName::InvalidCharacter));
    }
    let html = cx.is_html_document(this);
    if html
        && let Some(local) = id
        && !local_name.contains('-')
    {
        let ids = dom::NameIds {
            namespace: names::HTML_NAMESPACE,
            prefix: names::NONE,
            local,
        };
        let element = cx.create_element_named(this, ids);
        debug_assert!(is_element(cx, element));
        return Ok(Created::Element(element));
    }
    let local = if html {
        lowercase(local_name)
    } else {
        local_name.into()
    };
    // An XHTML document's elements are HTML's too, their names as given.
    let namespace = html || cx.document_content_type(this).to_str() == "application/xhtml+xml";
    let name = QualName {
        namespace: namespace.then_some(HTML_NAMESPACE),
        prefix: None,
        local: &local,
    };
    let element = cx.create_element(this, name);
    debug_assert!(is_element(cx, element));
    custom_or(cx, this, element)
}

/// `element` (just made), or, if it is undefined (a custom element's
/// name: `create_element` tells it, one byte read on the path of every
/// other element) and its name is defined, the custom element to
/// construct after the operation instead: `element` let go of, garbage
/// beside a constructor's call.
fn custom_or<'gc>(
    cx: &ApiCx<'_, 'gc>,
    document: NodeRef<'gc>,
    element: NodeRef<'gc>,
) -> Result<Created<'gc>, ApiError> {
    if cx.custom_element_state(element) == dom::CustomElementState::Undefined {
        return Ok(custom_elements::to_construct(cx, document, element));
    }
    Ok(Created::Element(element))
}

/// `createElementNS`: "validate and extract", then the element.
pub(crate) fn create_element_ns<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    namespace: Option<&str>,
    qualified_name: &str,
) -> Result<Created<'gc>, ApiError> {
    let name = names_check::validate_and_extract(namespace, qualified_name, NameContext::Element)?;
    let element = cx.create_element(this, name);
    custom_or(cx, this, element)
}

pub(crate) fn create_text_node<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    data: &str,
) -> Result<NodeRef<'gc>, ApiError> {
    Ok(cx.create_text(this, data))
}

/// What `createEvent` makes of an interface's name: DOM's table, ASCII
/// case-insensitively, of the interfaces machopo has.
enum CreatedEvent {
    Plain,
    Ui,
    Mouse,
    Keyboard,
    Focus,
    Custom,
    Text,
    Message,
}

/// `createEvent(interface)`: a new event, not initialized (an
/// `init*Event` initializes it), of the interface DOM's table gives the
/// name, in this document's realm; another name (of the table's
/// interfaces machopo hasn't, or none of its) is a `NotSupportedError`.
pub(crate) fn create_event<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    interface: &str,
) -> Result<elise::Gc<'gc, dom::Event>, ApiError> {
    let kind = match lowercase(interface).as_ref() {
        "event" | "events" | "htmlevents" | "svgevents" => CreatedEvent::Plain,
        "uievent" | "uievents" => CreatedEvent::Ui,
        "mouseevent" | "mouseevents" => CreatedEvent::Mouse,
        "keyboardevent" => CreatedEvent::Keyboard,
        "focusevent" => CreatedEvent::Focus,
        "customevent" => CreatedEvent::Custom,
        "textevent" => CreatedEvent::Text,
        "messageevent" => CreatedEvent::Message,
        _ => return Err(ApiError::dom(ExceptionName::NotSupported)),
    };
    let event = cx.create_uninitialized_event(cx.window_of(this));
    match kind {
        CreatedEvent::Plain => {}
        CreatedEvent::Ui => cx.set_ui_event_data(event, None, 0),
        CreatedEvent::Mouse => {
            cx.set_mouse_event_data(event, None, &dom::MouseEventInfo::default(), None)
        }
        CreatedEvent::Keyboard => cx.set_keyboard_event_data(
            event,
            None,
            &dom::KeyboardEventInfo {
                detail: 0,
                key: dom::DomString::new(""),
                code: dom::DomString::new(""),
                location: 0,
                repeat: false,
                is_composing: false,
                modifiers: 0,
                char_code: 0,
                key_code: 0,
            },
        ),
        CreatedEvent::Focus => cx.set_focus_event_data(event, None, 0, None),
        CreatedEvent::Custom => cx.set_custom_event_data(event),
        CreatedEvent::Text => cx.set_text_event_data(event, None, ""),
        CreatedEvent::Message => cx.set_message_event_data(
            event,
            dom::MessageEventInit {
                origin: dom::DomString::new(""),
                last_event_id: dom::DomString::new(""),
                source: None,
                ports: Vec::new(),
            },
        ),
    }
    Ok(event)
}

/// The document's SVG root element: its document element, if that is an
/// SVG `svg`.
fn svg_root<'gc>(cx: &dom::Cx<'_, 'gc>, document: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
    let mut child = cx.first_child(document);
    while let Some(at) = child {
        if cx.kind(at) == NodeKind::Element {
            return (cx.qualified_name(at) == Some(QualifiedName::svg(local::SVG))).then_some(at);
        }
        child = cx.next_sibling(at);
    }
    None
}

/// `svg`'s first SVG `title` child.
fn svg_title<'gc>(cx: &dom::Cx<'_, 'gc>, svg: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
    std::iter::successors(cx.first_child(svg), |&child| cx.next_sibling(child))
        .find(|&child| cx.qualified_name(child) == Some(QualifiedName::svg(local::TITLE)))
}

/// HTML's "the title element": the first HTML `title` in the document, in
/// tree order.
fn title_element<'gc>(cx: &dom::Cx<'_, 'gc>, document: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
    let mut node = cx.first_child(document);
    while let Some(at) = node {
        if cx.qualified_name(at) == Some(QualifiedName::html(local::TITLE)) {
            return Some(at);
        }
        node = cx.following_within(at, document);
    }
    None
}

/// HTML's `document.title`, what the frame tells the browser (Blink's
/// `Document::title`): the child text content (its `Text` children's
/// data, `CDATASection`s too) of an SVG document element's first SVG
/// `title` child, else of the title element, ASCII whitespace stripped and
/// collapsed; empty without one.
pub fn document_title<'gc>(cx: &dom::Cx<'_, 'gc>, document: NodeRef<'gc>) -> String {
    let source = match svg_root(cx, document) {
        Some(svg) => svg_title(cx, svg),
        None => title_element(cx, document),
    };
    let Some(source) = source else {
        return String::new();
    };
    let text: String =
        std::iter::successors(cx.first_child(source), |&child| cx.next_sibling(child))
            .filter(|&child| cx.kind(child) == NodeKind::Text)
            .filter_map(|child| cx.data(child))
            .map(|data| data.to_string())
            .collect();
    text.split_ascii_whitespace().collect::<Vec<_>>().join(" ")
}

/// `title`.
pub(crate) fn title<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(DomString::new(&document_title(cx, this))))
}

/// Setting `title`: with an SVG document element, its first SVG `title`
/// child (one made and inserted first if there is none); with an HTML
/// one, the title element (one made and appended to the head element if
/// there is none; nothing without a head either); in either, its children
/// replaced by `value` ("string replace all"). Nothing for another
/// document element, or none.
pub(crate) fn set_title<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    let title = if let Some(svg) = svg_root(cx, this) {
        match svg_title(cx, svg) {
            Some(title) => title,
            None => {
                let name = QualName {
                    namespace: Some("http://www.w3.org/2000/svg"),
                    prefix: None,
                    local: "title",
                };
                let title = cx.create_element(this, name);
                cx.pre_insert(svg, title, cx.first_child(svg))
                    .map_err(ApiError::from)?;
                title
            }
        }
    } else if document_element(cx, this)?.is_some_and(|element| {
        cx.qualified_name(element)
            .is_some_and(|name| name.namespace() == names::HTML_NAMESPACE)
    }) {
        match title_element(cx, this) {
            Some(title) => title,
            None => {
                let Some(head) = head(cx, this)? else {
                    return Ok(());
                };
                let title = cx.create_element(this, QualName::html("title"));
                cx.append_child(head, title).map_err(ApiError::from)?;
                title
            }
        }
    } else {
        return Ok(());
    };
    cx.set_text_content(title, value);
    Ok(())
}

/// `implementation`: the same each time.
pub(crate) fn implementation<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Gc<'gc, DomImplementation>, ApiError> {
    Ok(cx.dom_implementation(this))
}

/// `createCDATASection(data)`: a `NotSupportedError` in an HTML document,
/// an `InvalidCharacterError` for data with `]]>`.
pub(crate) fn create_cdata_section<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    data: &str,
) -> Result<NodeRef<'gc>, ApiError> {
    if cx.is_html_document(this) {
        return Err(ApiError::dom(ExceptionName::NotSupported));
    }
    if data.contains("]]>") {
        return Err(ApiError::dom(ExceptionName::InvalidCharacter));
    }
    Ok(cx.create_cdata_section(this, data))
}

pub(crate) fn create_comment<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    data: &str,
) -> Result<NodeRef<'gc>, ApiError> {
    Ok(cx.create_comment(this, data))
}

/// `new DocumentFragment()`: a fragment of the current realm's document.
pub(crate) fn construct_document_fragment<'gc, O: crate::typed::OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
) -> Result<NodeRef<'gc>, ApiError> {
    let document = crate::character_data::realm_document(op, cx)?;
    Ok(cx.create_document_fragment(document))
}

/// `new Document()`: an XML document (not an HTML one) at `about:blank`,
/// in no browsing context (as `createHTMLDocument`'s).
pub(crate) fn construct_document<'gc, O: crate::typed::OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
) -> Result<NodeRef<'gc>, ApiError> {
    crate::character_data::realm_document(op, cx)?;
    let document = cx.create_document(false);
    cx.set_document_relevant_window(document, op.current_window(cx));
    cx.set_document_url(document, "about:blank");
    Ok(document)
}

pub(crate) fn create_document_fragment<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<NodeRef<'gc>, ApiError> {
    Ok(cx.create_document_fragment(this))
}

/// `createProcessingInstruction(target, data)`: an `InvalidCharacterError`
/// for a target that isn't an XML `Name` or data with `?>`.
pub(crate) fn create_processing_instruction<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    target: &str,
    data: &str,
) -> Result<NodeRef<'gc>, ApiError> {
    if !names_check::is_xml_name(target) || data.contains("?>") {
        return Err(ApiError::dom(ExceptionName::InvalidCharacter));
    }
    Ok(cx.create_processing_instruction(this, target, data))
}

/// The `NotSupportedError` of a shadow root or a document cloned or
/// imported.
fn not_supported() -> ApiError {
    ApiError::dom(ExceptionName::NotSupported)
}

/// `importNode(node, deep)`: a copy of `node` of this document; a document
/// or a shadow root is a `NotSupportedError`.
pub(crate) fn import_node<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    node: NodeRef<'gc>,
    deep: bool,
) -> Result<NodeRef<'gc>, ApiError> {
    if cx.kind(node) == NodeKind::Document || cx.is_shadow_root(node) {
        return Err(not_supported());
    }
    Ok(cx.clone_node(node, Some(this), deep))
}

/// `adoptNode(node)`: `node` out of its parent and this document's, with
/// its descendants; a document is a `NotSupportedError`, a shadow root a
/// `HierarchyRequestError`.
pub(crate) fn adopt_node<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    node: NodeRef<'gc>,
) -> Result<NodeRef<'gc>, ApiError> {
    if cx.kind(node) == NodeKind::Document {
        return Err(not_supported());
    }
    if cx.is_shadow_root(node) {
        return Err(ApiError::dom(ExceptionName::HierarchyRequest));
    }
    cx.adopt_node(node, this);
    Ok(node)
}

/// `cloneNode(deep)`: a copy, of its node document, not in any tree; a
/// shadow root is a `NotSupportedError`.
pub(crate) fn clone_node<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    deep: bool,
) -> Result<NodeRef<'gc>, ApiError> {
    if cx.is_shadow_root(this) {
        return Err(not_supported());
    }
    Ok(cx.clone_node(this, None, deep))
}

/// Whether `node` is an HTML element named `local`.
fn is_html_named<'gc>(cx: &ApiCx<'_, 'gc>, node: NodeRef<'gc>, local: names::NameId) -> bool {
    cx.element_name(node)
        .is_some_and(|name| name.namespace == names::HTML_NAMESPACE && name.local == local)
}

/// The first of the document element's children `matches` takes, if the
/// document element is an HTML `<html>` (HTML's "the body element", "the
/// head element").
fn html_child<'gc>(
    cx: &ApiCx<'_, 'gc>,
    document: NodeRef<'gc>,
    matches: impl Fn(NodeRef<'gc>) -> bool,
) -> Option<NodeRef<'gc>> {
    let html = crate::parent_node::first_element_child(cx, document)
        .ok()
        .flatten()
        .filter(|&html| is_html_named(cx, html, local::HTML))?;
    let mut child = cx.first_child(html);
    while let Some(at) = child {
        if matches(at) {
            return Some(at);
        }
        child = cx.next_sibling(at);
    }
    None
}

/// `body`: the `<html>`'s first `<body>` or `<frameset>` child.
pub(crate) fn body<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(html_child(cx, this, |at| {
        is_html_named(cx, at, local::BODY) || is_html_named(cx, at, local::FRAMESET)
    }))
}

/// Setting `body`: a `<body>` or `<frameset>` (else a
/// `HierarchyRequestError`) in place of the body, or appended to the
/// document element (none: a `HierarchyRequestError`).
pub(crate) fn set_body<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: Option<NodeRef<'gc>>,
) -> Result<(), ApiError> {
    let Some(value) = value.filter(|&value| {
        is_html_named(cx, value, local::BODY) || is_html_named(cx, value, local::FRAMESET)
    }) else {
        return Err(ApiError::dom(ExceptionName::HierarchyRequest));
    };
    match body(cx, this)? {
        Some(old) if elise::Gc::ptr_eq(old, value) => Ok(()),
        Some(old) => {
            let parent = cx
                .parent(old)
                .expect("the body is the document element's child");
            cx.replace_child(parent, value, old)?;
            Ok(())
        }
        None => match crate::parent_node::first_element_child(cx, this)? {
            Some(root) => {
                cx.append_child(root, value)?;
                Ok(())
            }
            None => Err(ApiError::dom(ExceptionName::HierarchyRequest)),
        },
    }
}

/// `head`: the `<html>`'s first `<head>` child.
pub(crate) fn head<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(html_child(cx, this, |at| {
        is_html_named(cx, at, local::HEAD)
    }))
}

/// `doctype`: its doctype child, if it has one.
pub(crate) fn doctype<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    let mut child = cx.first_child(this);
    while let Some(at) = child {
        if cx.kind(at) == NodeKind::DocumentType {
            return Ok(Some(at));
        }
        child = cx.next_sibling(at);
    }
    Ok(None)
}

/// `getElementsByName(name)`: a live list of the document's HTML elements
/// whose `name` is `name`.
pub(crate) fn get_elements_by_name<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    name: &str,
) -> Result<Gc<'gc, dom::NodeList>, ApiError> {
    Ok(cx.elements_by_name_list(this, name))
}

/// `links`: the document's `a` and `area` elements with an `href`.
pub(crate) fn links<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Gc<'gc, dom::HtmlCollection>, ApiError> {
    Ok(cx.document_links(this, false))
}

/// `images`, `embeds` and `plugins` (the same), `scripts`: the
/// document's `img`, `embed` and `script` elements, the same each time.
pub(crate) fn images<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Gc<'gc, dom::HtmlCollection>, ApiError> {
    Ok(cx.kept_collection(this, dom::KeptCollection::Images))
}

pub(crate) fn embeds<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Gc<'gc, dom::HtmlCollection>, ApiError> {
    Ok(cx.kept_collection(this, dom::KeptCollection::Embeds))
}

pub(crate) fn scripts<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Gc<'gc, dom::HtmlCollection>, ApiError> {
    Ok(cx.kept_collection(this, dom::KeptCollection::Scripts))
}

/// `anchors`: the document's `a` elements with a `name`.
pub(crate) fn anchors<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Gc<'gc, dom::HtmlCollection>, ApiError> {
    Ok(cx.document_links(this, true))
}

/// `characterSet`, `charset` and `inputEncoding`: its encoding's name.
pub(crate) fn character_set<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(cx.document_character_set(this)))
}

pub(crate) fn content_type<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(cx.document_content_type(this)))
}

/// `currentScript`: the classic script element its host runs now.
pub(crate) fn current_script<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(cx.current_script(this))
}

/// HTML's "the html element": the document element, if it is an HTML
/// `html`.
fn html_element<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
    crate::geometry::document_element(cx, this)
        .filter(|&root| cx.qualified_name(root) == Some(dom::QualifiedName::html(local::HTML)))
}

/// `dir`: the html element's `dir`, limited to `ltr`, `rtl` and `auto`
/// (ASCII case-insensitively; else empty); empty without one.
pub(crate) fn dir<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<DomStr<'gc>, ApiError> {
    let value = html_element(cx, this)
        .and_then(|html| cx.attribute_value_by_id(html, local::DIR))
        .map(|value| value.borrow().to_str().to_ascii_lowercase())
        .filter(|value| matches!(value.as_str(), "ltr" | "rtl" | "auto"))
        .unwrap_or_default();
    Ok(DomStr::Owned(value.into()))
}

/// Setting `dir` sets the html element's; without one, nothing.
pub(crate) fn set_dir<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    if let Some(html) = html_element(cx, this) {
        cx.set_attribute(html, "dir", value);
    }
    Ok(())
}

/// Whether the document's page is hidden: one without a window is (HTML's
/// visibility state starts hidden, and only a browsing context's document
/// follows its page; Blink's `Document::IsPageVisible` is false without a
/// frame).
fn page_hidden<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> bool {
    cx.window_of(this)
        .is_none_or(|window| cx.page_hidden(window))
}

/// `visibilityState`: `hidden` while its page is, else `visible`.
pub(crate) fn visibility_state<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(
        if page_hidden(cx, this) {
            "hidden"
        } else {
            "visible"
        }
        .into(),
    ))
}

pub(crate) fn hidden<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<bool, ApiError> {
    Ok(page_hidden(cx, this))
}

/// `prerendering`: no document is (machopo doesn't prerender).
pub(crate) fn prerendering<'gc>(_: &ApiCx<'_, 'gc>, _: NodeRef<'gc>) -> Result<bool, ApiError> {
    Ok(false)
}

/// The page `top` (a top-level document) is in was hidden or shown (the
/// browser's `WasHidden`, `WasShown`; Blink's `Page::SetVisibilityState`):
/// every window of its frame tree takes the state, then each document, in
/// tree order, runs HTML's "update the visibility state": `visibilitychange`
/// (bubbling, not cancelable) fires at it. Nothing if it was already so.
/// Before that, the windows' page visibility observers (Blink's
/// `PageVisibilityObserver`s, as `Page::SetVisibilityState` tells them
/// first): hidden, their wake locks are released; shown, their
/// geolocations start the requests made while hidden.
pub fn set_page_visible<H: crate::typed::ScriptHost>(
    host: &mut H,
    top: &dom::NodeRoot,
    visible: bool,
) {
    let (documents, windows) = host.mutate(|cx| {
        let top = top.get(cx.gc);
        if cx
            .window_of(top)
            .is_none_or(|window| cx.page_hidden(window) == !visible)
        {
            return (Vec::new(), Vec::new());
        }
        let documents = crate::documents_in_tree_order(cx, top);
        let mut windows = Vec::new();
        for &document in &documents {
            if let Some(window) = cx.window_of(document) {
                cx.set_page_hidden(window, !visible);
                if visible {
                    crate::geolocation::page_shown(cx, window);
                } else {
                    windows.push(elise::Persistent::new(cx.gc, window));
                }
            }
        }
        (
            documents
                .into_iter()
                .map(|document| cx.root(document))
                .collect(),
            windows,
        )
    });
    for window in &windows {
        crate::wake_lock::page_hidden(host, window);
    }
    for document in documents {
        crate::event_target::fire_event(
            host,
            &dom::TargetRoot::Node(document),
            names::event::VISIBILITYCHANGE,
            true,
            false,
        );
    }
}
