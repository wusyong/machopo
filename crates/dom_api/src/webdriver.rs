//! WebDriver's work on the DOM, in no engine's terms: the values a
//! script's result and arguments are (elements and windows as `NodeId`s,
//! so nothing here keeps a node alive: the renderer makes references of
//! them and checks them, the engine its objects), and what the renderer
//! asks of a document (`WebDriverQuery`): finding elements, reading them,
//! where they are in the viewport and what is hit there; and what the
//! browser's automated checks read (`Probe`). The renderer has
//! the protocol (references, frames); the engine runs these in its
//! operations (`ScriptEngine::webdriver_query`). Chromium splits it alike:
//! chromedriver's references are backend node ids, and its DOM work is the
//! DOM's API (its atoms) in the page.

use dom::{NodeId, NodeKind, NodeRef, NodeRoot};
use gfx::{PointF, RectF, SizeF};

use crate::ApiCx;
use crate::typed::ScriptHost;

/// A value in JSON's shape, with WebDriver's references as ids.
#[derive(Debug, Clone, PartialEq)]
pub enum WebDriverValue {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<WebDriverValue>),
    /// An object's own enumerable properties, in order.
    Object(Vec<(String, WebDriverValue)>),
    /// An element.
    Element(NodeId),
    /// A window (its `WindowProxy`), by its document.
    Window(NodeId),
}

/// Why an engine's WebDriver work failed: script threw, or the value can't
/// be cloned (a cycle): WebDriver's "javascript error", with the message.
#[derive(Debug, Clone, PartialEq)]
pub struct WebDriverScriptError(pub String);

/// A location strategy: `css selector`, `tag name`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementLocator {
    Css,
    TagName,
}

/// What the renderer asks of a document for WebDriver, its elements by
/// their `NodeId`s.
#[derive(Debug, Clone, PartialEq)]
pub enum WebDriverQuery<'a> {
    /// Whether the element is still one of the document's.
    Element(NodeId),
    /// Find Element(s): what `querySelectorAll` (or `getElementsByTagName`)
    /// finds among the document's descendants, or `from`'s.
    Find {
        using: ElementLocator,
        value: &'a str,
        from: Option<NodeId>,
    },
    /// Get Element Rect: its `getBoundingClientRect()` moved by the
    /// viewport's scroll offset, in the document's CSS px.
    Rect(NodeId),
    /// Get Element Tag Name: its qualified name as it is (`tagName` but
    /// for HTML's uppercasing, as WebDriver's tests and Chrome have it).
    TagName(NodeId),
    /// The element's attribute `name`, if it has one (HTML's lowercasing).
    Attribute(NodeId, &'a str),
    /// Element Click's point: the element scrolled into view first
    /// (`scroll_into_view`), WebDriver's in-view center point of the
    /// element, where what is hit on top is it or in it, in the top-level
    /// frame's viewport (where the browser's click goes), through the
    /// iframes around a child frame's.
    ClickPoint(NodeId),
    /// Perform Actions' element origin: the middle of its first box
    /// clamped to its frame's viewport, floored, in that viewport.
    InViewCenter(NodeId),
    /// Take Element Screenshot's: the element scrolled into view first,
    /// the part of its box in view, in the top-level frame's viewport.
    ScreenshotRect(NodeId),
    /// Perform Actions' viewport: the frame's, where it is in the top-level
    /// one and its size.
    Viewport,
    /// An automated check's (a scenario's `RendererMsg::QueryElement` and
    /// `ScrollIntoViewIfNeeded`; DevTools' `DOM.querySelector` and
    /// `DOM.getBoxModel`): the document's first element matching a
    /// selector, its box in the top-level frame's viewport; none for a
    /// selector that doesn't parse.
    Probe(&'a str),
}

/// What a query found.
#[derive(Debug, Clone, PartialEq)]
pub enum WebDriverAnswer {
    Element,
    Elements(Vec<NodeId>),
    Rect(RectF),
    Point(PointF),
    Text(Option<String>),
    Probed(Option<Probed>),
}

/// The element an automated check found.
#[derive(Debug, Clone, PartialEq)]
pub struct Probed {
    pub element: NodeId,
    /// The union of its border boxes (`getBoundingClientRect()`), laid
    /// out first, in the top-level frame's viewport; none if it has no
    /// box (or its frame isn't shown).
    pub rect: Option<RectF>,
    /// Its `textContent`.
    pub text: String,
}

/// Why a query failed, as WebDriver's errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebDriverQueryError {
    /// The element has gone, or isn't in the document now: "stale element
    /// reference".
    Stale,
    /// Not an element: "no such element".
    NoSuchElement,
    InvalidSelector(String),
    /// "element not interactable": not in view (nothing scrolls it there
    /// yet).
    NotInteractable,
    /// "element click intercepted": something else is on top there.
    ClickIntercepted,
    /// "move target out of bounds": no box.
    OutOfBounds,
    /// No frame shows the document.
    NotShown,
    /// "unable to capture screen": nothing of the element is in view to
    /// take (WebDriver's Take Element Screenshot, as chromedriver answers).
    NothingToCapture,
}

/// The element `id` names in `document`.
fn element<'gc>(
    cx: &ApiCx<'_, 'gc>,
    document: NodeRef<'gc>,
    id: NodeId,
) -> Result<NodeRef<'gc>, WebDriverQueryError> {
    let node = cx.node_by_id(id).ok_or(WebDriverQueryError::Stale)?;
    if cx.kind(node) != NodeKind::Element {
        return Err(WebDriverQueryError::NoSuchElement);
    }
    if !cx.is_connected(node) || !elise::Gc::ptr_eq(cx.document_of(node), document) {
        return Err(WebDriverQueryError::Stale);
    }
    Ok(node)
}

/// Answers `query` of `document`, laying it out as a member would.
pub fn answer<'gc>(
    cx: &ApiCx<'_, 'gc>,
    document: NodeRef<'gc>,
    query: &WebDriverQuery<'_>,
) -> Result<WebDriverAnswer, WebDriverQueryError> {
    match *query {
        WebDriverQuery::Element(id) => element(cx, document, id).map(|_| WebDriverAnswer::Element),
        WebDriverQuery::Find { using, value, from } => {
            let root = match from {
                Some(from) => element(cx, document, from)?,
                None => document,
            };
            let found = match using {
                ElementLocator::Css => crate::query::select_all(cx, root, value)
                    .map_err(|error| WebDriverQueryError::InvalidSelector(error.message()))?,
                ElementLocator::TagName => crate::collection::elements_by_tag_name(cx, root, value),
            };
            Ok(WebDriverAnswer::Elements(
                found.into_iter().map(|node| cx.node_id(node)).collect(),
            ))
        }
        WebDriverQuery::Rect(id) => {
            let element = element(cx, document, id)?;
            let bounds = crate::geometry::bounding_client_rect(cx, element);
            let scroll =
                viewport(cx, document).map_or(PointF::new(0.0, 0.0), |viewport| viewport.scroll);
            Ok(WebDriverAnswer::Rect(bounds.offset(scroll.x, scroll.y)))
        }
        WebDriverQuery::TagName(id) => {
            let element = element(cx, document, id)?;
            let name = cx.element_name(element).expect("an element");
            let local = dom::names::get(name.local);
            Ok(WebDriverAnswer::Text(Some(
                if name.prefix == dom::names::NONE {
                    local.to_string()
                } else {
                    format!("{}:{local}", dom::names::get(name.prefix))
                },
            )))
        }
        WebDriverQuery::Attribute(id, name) => {
            let element = element(cx, document, id)?;
            Ok(WebDriverAnswer::Text(
                crate::element::attribute_named(cx, element, name).map(|value| value.to_string()),
            ))
        }
        WebDriverQuery::ClickPoint(id) => {
            let element = element(cx, document, id)?;
            scroll_into_view(cx, document, element);
            let center = in_view_center(cx, document, element)
                .ok_or(WebDriverQueryError::NotInteractable)?;
            // On top through the frames: the deepest element there.
            let top = cx.top_document(document);
            let on_top = crate::geometry::deepest_element_at(cx, top, center)
                .is_some_and(|hit| is_inclusive_ancestor(cx, element, cx.node_id(hit)));
            if !on_top {
                return Err(WebDriverQueryError::ClickIntercepted);
            }
            Ok(WebDriverAnswer::Point(center))
        }
        WebDriverQuery::InViewCenter(id) => {
            let element = element(cx, document, id)?;
            let rect = *crate::geometry::client_rects_of(cx, element)
                .first()
                .ok_or(WebDriverQueryError::OutOfBounds)?;
            let port = viewport(cx, document)
                .ok_or(WebDriverQueryError::OutOfBounds)?
                .size;
            let (x0, x1) = (rect.x, rect.x + rect.width);
            let (y0, y1) = (rect.y, rect.y + rect.height);
            let (left, right) = (x0.min(x1).max(0.0), x0.max(x1).min(port.width));
            let (top, bottom) = (y0.min(y1).max(0.0), y0.max(y1).min(port.height));
            Ok(WebDriverAnswer::Point(PointF::new(
                ((left + right) / 2.0).floor(),
                ((top + bottom) / 2.0).floor(),
            )))
        }
        WebDriverQuery::ScreenshotRect(id) => {
            let element = element(cx, document, id)?;
            scroll_into_view(cx, document, element);
            let bounds = crate::geometry::bounding_client_rect(cx, element);
            let port = viewport(cx, document)
                .ok_or(WebDriverQueryError::NotShown)?
                .size;
            in_view(bounds, port)
                .and_then(|rect| in_top_view(cx, document, rect))
                .map(WebDriverAnswer::Rect)
                .ok_or(WebDriverQueryError::NothingToCapture)
        }
        WebDriverQuery::Viewport => {
            let size = viewport(cx, document)
                .ok_or(WebDriverQueryError::NotShown)?
                .size;
            let origin = point_in_top(cx, document, PointF::new(0.0, 0.0))
                .ok_or(WebDriverQueryError::NotShown)?;
            Ok(WebDriverAnswer::Rect(RectF::new(
                origin.x,
                origin.y,
                size.width,
                size.height,
            )))
        }
        WebDriverQuery::Probe(selector) => {
            let found = crate::query::query_selector(cx, document, selector)
                .ok()
                .flatten();
            Ok(WebDriverAnswer::Probed(found.map(|element| {
                let rects = crate::geometry::client_rects_of(cx, element);
                Probed {
                    element: cx.node_id(element),
                    rect: (!rects.is_empty())
                        .then(|| crate::geometry::bounding_rect(&rects))
                        .and_then(|rect| rect_in_top(cx, document, rect)),
                    text: cx
                        .text_content(element)
                        .map(|text| text.to_string())
                        .unwrap_or_default(),
                }
            })))
        }
    }
}

fn viewport<'gc>(cx: &ApiCx<'_, 'gc>, document: NodeRef<'gc>) -> Option<gfx::Viewport> {
    cx.with_layout(document, |layout| layout.viewport())
}

/// `point` (CSS px of `document`'s frame's viewport) in the top-level
/// frame's viewport: through the iframes around a child frame's.
fn point_in_top<'gc>(cx: &ApiCx<'_, 'gc>, document: NodeRef<'gc>, point: PointF) -> Option<PointF> {
    let top = cx.top_document(document);
    if elise::Gc::ptr_eq(top, document) {
        return Some(point);
    }
    let id = cx.node_id(document);
    cx.with_layout(top, |layout| layout.point_from_frame(id, point))
        .flatten()
}

/// `rect` (CSS px of `document`'s frame's viewport) in the top-level
/// frame's viewport, its corners moved (an iframe's transform's rotation
/// isn't kept).
fn rect_in_top<'gc>(cx: &ApiCx<'_, 'gc>, document: NodeRef<'gc>, rect: RectF) -> Option<RectF> {
    let from = point_in_top(cx, document, PointF::new(rect.x, rect.y))?;
    let to = point_in_top(
        cx,
        document,
        PointF::new(rect.x + rect.width, rect.y + rect.height),
    )?;
    let (left, right) = (from.x.min(to.x), from.x.max(to.x));
    let (top, bottom) = (from.y.min(to.y), from.y.max(to.y));
    Some(RectF::new(left, top, right - left, bottom - top))
}

/// The part of `rect` (`document`'s frame's viewport, already in it) in the
/// top-level frame's viewport, there; none if none of it is.
fn in_top_view<'gc>(cx: &ApiCx<'_, 'gc>, document: NodeRef<'gc>, rect: RectF) -> Option<RectF> {
    let top = cx.top_document(document);
    if elise::Gc::ptr_eq(top, document) {
        return Some(rect);
    }
    let rect = rect_in_top(cx, document, rect)?;
    in_view(rect, viewport(cx, top)?.size)
}

/// Whether `node` is `ancestor` or in it, through the hosts of the shadow
/// trees it is in (a text control's editor is the control's) and the
/// iframes a child frame's document is in (a hit in a child frame is in
/// its iframe).
fn is_inclusive_ancestor<'gc>(cx: &ApiCx<'_, 'gc>, ancestor: NodeRef<'gc>, node: NodeId) -> bool {
    cx.node_by_id(node).is_some_and(|node| {
        std::iter::successors(Some(node), |&at| {
            cx.parent(at).or_else(|| cx.shadow_host(at)).or_else(|| {
                (cx.kind(at) == NodeKind::Document)
                    .then(|| cx.frame_owner(at))
                    .flatten()
            })
        })
        .any(|at| elise::Gc::ptr_eq(at, ancestor))
    })
}

/// The part of `rect` (the viewport's CSS px) in a viewport of `port`;
/// none if none of it is.
fn in_view(rect: RectF, port: SizeF) -> Option<RectF> {
    let (left, top) = (rect.x.max(0.0), rect.y.max(0.0));
    let right = (rect.x + rect.width).min(port.width);
    let bottom = (rect.y + rect.height).min(port.height);
    (right > left && bottom > top).then(|| RectF::new(left, top, right - left, bottom - top))
}

/// WebDriver's in-view center point of `element`, in the top-level
/// frame's viewport: the center of the part of its first box in its
/// frame's viewport and in the top-level one (through the iframes around
/// a child frame's), floored, if the element is in view there: hit testing
/// at it in its frame finds the element or something in it, under
/// whatever is on top ("a member of its own pointer-interactable paint
/// tree").
fn in_view_center<'gc>(
    cx: &ApiCx<'_, 'gc>,
    document: NodeRef<'gc>,
    element: NodeRef<'gc>,
) -> Option<PointF> {
    let first = *crate::geometry::client_rects_of(cx, element).first()?;
    let port = viewport(cx, document)?.size;
    let rect = in_top_view(cx, document, in_view(first, port)?)?;
    let center = PointF::new(
        (rect.x + rect.width / 2.0).floor(),
        (rect.y + rect.height / 2.0).floor(),
    );
    // The point in its frame's viewport.
    let top = cx.top_document(document);
    let at = if elise::Gc::ptr_eq(top, document) {
        center
    } else {
        let id = cx.node_id(document);
        cx.with_layout(top, |layout| layout.point_in_frame(id, center))??
    };
    cx.with_layout(document, |layout| layout.elements_at(at))?
        .into_iter()
        .any(|hit| is_inclusive_ancestor(cx, element, hit))
        .then_some(center)
}

/// WebDriver's "scroll into view" of `element`, unless its in-view center
/// point is in view already: `block: end`, `inline: nearest`, in each
/// scrolling box around it through the frames around its own (as
/// `scrollIntoView` with those options; ChromeDriver's).
fn scroll_into_view<'gc>(cx: &ApiCx<'_, 'gc>, document: NodeRef<'gc>, element: NodeRef<'gc>) {
    if in_view_center(cx, document, element).is_some() {
        return;
    }
    let params = gfx::ScrollIntoViewParams {
        block: gfx::ScrollAlignment::End,
        inline: gfx::ScrollAlignment::Nearest,
        nearest_only: false,
    };
    cx.with_layout(element, |layout| {
        layout.scroll_into_view(cx, element, params)
    });
}

/// What an action of WebDriver's on an element failed with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebDriverActionError {
    /// The element is gone, or not one: as `WebDriverQueryError`'s.
    Query(WebDriverQueryError),
    /// "element not interactable".
    NotInteractable(&'static str),
    /// "invalid element state".
    InvalidElementState(&'static str),
    /// "unsupported operation".
    Unsupported(&'static str),
}

/// Whether Element Send Keys sets `element`'s value rather than typing:
/// WebDriver's non-typeable form controls (color, the date and time types,
/// range).
fn is_non_typeable<'gc>(cx: &ApiCx<'_, 'gc>, element: NodeRef<'gc>) -> bool {
    use dom::InputKind;
    matches!(
        cx.input_kind(element),
        Some(
            InputKind::Color
                | InputKind::Date
                | InputKind::DatetimeLocal
                | InputKind::Month
                | InputKind::Range
                | InputKind::Time
                | InputKind::Week
        )
    )
}

/// Element Send Keys before its keys, of `document`'s element `id`: a
/// file input is "unsupported operation" (machopo has no files to pick);
/// another scrolled into view (`scroll_into_view`); one that isn't
/// keyboard-interactable (a focusable area, the `<body>`, the
/// document's element) is "element not interactable" (the driver asks
/// again for the implicit wait); the focus taken if it hasn't it, a text
/// control's cursor then at its end. A non-typeable control's value is set
/// to `text` here (`false`: no keys; read-only or disabled, "element not
/// interactable"). `true`: the driver types the keys.
pub fn send_keys<H: ScriptHost>(
    host: &mut H,
    document: &NodeRoot,
    id: NodeId,
    text: &str,
) -> Result<bool, WebDriverActionError> {
    let element = host.mutate(|cx| {
        element(cx, document.get(cx.gc), id)
            .map(|node| cx.root(node))
            .map_err(WebDriverActionError::Query)
    })?;
    let (file, interactable, had_focus) = host.mutate(|cx| {
        let node = element.get(cx.gc);
        let file = cx.input_kind(node) == Some(dom::InputKind::File);
        let root = cx
            .parent(node)
            .is_some_and(|parent| cx.kind(parent) == NodeKind::Document);
        let body =
            cx.qualified_name(node) == Some(dom::QualifiedName::html(dom::names::local::BODY));
        let interactable = body || root || crate::focus::is_focusable_area(cx, node);
        let had_focus = cx
            .focused_element(cx.document_of(node))
            .is_some_and(|focused| elise::Gc::ptr_eq(focused, node));
        (file, interactable, had_focus)
    });
    if file {
        return Err(WebDriverActionError::Unsupported(
            "machopo has no files to pick",
        ));
    }
    host.mutate(|cx| scroll_into_view(cx, document.get(cx.gc), element.get(cx.gc)));
    if !interactable {
        return Err(WebDriverActionError::NotInteractable(
            "the element can't take the keys",
        ));
    }
    if !had_focus {
        crate::focus::focusing_steps(host, document, Some(&element), None);
    }
    let non_typeable = host.read(|cx| is_non_typeable(cx, element.get(cx.gc)));
    if non_typeable {
        let mutable = host.mutate(|cx| {
            let node = element.get(cx.gc);
            let mutable = crate::editing::is_mutable(cx, node);
            if mutable {
                // As script sets it: sanitized.
                let _ = cx.set_input_value(node, text);
            }
            mutable
        });
        return if mutable {
            Ok(false)
        } else {
            Err(WebDriverActionError::NotInteractable(
                "the control is read-only or disabled",
            ))
        };
    }
    if !had_focus {
        host.mutate(|cx| {
            let node = element.get(cx.gc);
            if crate::editing::is_text_field(cx, node) {
                let end =
                    u32::try_from(cx.text_control_value(node).utf16_len()).unwrap_or(u32::MAX);
                cx.set_selection_range(node, end, end, dom::SelectionDirection::None);
            }
        });
    }
    Ok(true)
}

/// WebDriver's editable element, as far as machopo has them: a mutable form
/// control element, a `<textarea>` or an `<input>` of a type that holds a
/// value the user enters (not a button, checkbox, radio or hidden one),
/// neither read-only nor disabled. machopo has no `contenteditable`.
fn is_editable<'gc>(cx: &ApiCx<'_, 'gc>, element: NodeRef<'gc>) -> bool {
    use dom::InputKind;
    let control = cx.is_textarea(element)
        || cx.input_kind(element).is_some_and(|kind| {
            !matches!(
                kind,
                InputKind::Hidden
                    | InputKind::Checkbox
                    | InputKind::Radio
                    | InputKind::Submit
                    | InputKind::Image
                    | InputKind::Reset
                    | InputKind::Button
            )
        });
    control && crate::editing::is_mutable(cx, element)
}

/// Element Clear of `document`'s element `id`: "invalid element state"
/// unless it is editable; scrolled into view (`scroll_into_view`);
/// "element not interactable" unless its first box is in the viewport
/// then (an empty box counts). Then, unless it is empty already (a file input's files: machopo has
/// none), WebDriver's "clear a resettable element": the focusing steps,
/// its value emptied as script would (no change left for the user to
/// commit), `input` and `change`, the unfocusing steps.
pub fn clear<H: ScriptHost>(
    host: &mut H,
    document: &NodeRoot,
    id: NodeId,
) -> Result<(), WebDriverActionError> {
    let element = host.mutate(|cx| {
        let document = document.get(cx.gc);
        let node = element(cx, document, id).map_err(WebDriverActionError::Query)?;
        if !is_editable(cx, node) {
            return Err(WebDriverActionError::InvalidElementState(
                "the element isn't an editable form control",
            ));
        }
        scroll_into_view(cx, document, node);
        let port = viewport(cx, document).map(|viewport| viewport.size);
        let in_view = crate::geometry::client_rects_of(cx, node)
            .first()
            .zip(port)
            .is_some_and(|(rect, port)| {
                rect.x + rect.width >= 0.0
                    && rect.y + rect.height >= 0.0
                    && rect.x <= port.width
                    && rect.y <= port.height
            });
        if !in_view {
            return Err(WebDriverActionError::NotInteractable(
                "the element isn't in view",
            ));
        }
        Ok(cx.root(node))
    })?;
    let empty = host.read(|cx| {
        let node = element.get(cx.gc);
        cx.input_kind(node) == Some(dom::InputKind::File) || cx.text_control_value(node).is_empty()
    });
    if empty {
        return Ok(());
    }
    crate::focus::focusing_steps(host, document, Some(&element), None);
    host.mutate(|cx| {
        let node = element.get(cx.gc);
        if cx.is_textarea(node) {
            cx.set_textarea_value(node, "");
        } else {
            let _ = cx.set_input_value(node, "");
        }
        cx.forget_edit(node);
    });
    crate::editing::fire_plain_input(host, &element);
    crate::editing::fire_change(host, &element);
    // `blur()` fails at nothing.
    let _ = crate::focus::blur(host, element);
    Ok(())
}
