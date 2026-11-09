//! WebDriver's work in the renderer for the DOM's document: what each
//! command asks of the document is `dom_api`'s
//! (`ScriptEngine::webdriver_query`), its script and its properties the
//! engine's, in the frame the driver names (the engine's `FrameId`, with
//! this renderer's prefix).
//!
//! A reference names where its element is rather than standing for an
//! entry in a table: this renderer's prefix, the document's `DocumentId`
//! and the element's `NodeId` (chromedriver's are made the same way, of a
//! frame, a document and a backend node id; so are Ladybird's). The new
//! DOM's weak id table finds the node again, so a reference keeps nothing
//! alive. Another renderer's prefix (another tab's, or this tab's before a
//! crash) is "no such element", as WebDriver has it for a reference its
//! navigable never saw, and so is another frame's document; one of a
//! document the frame no longer shows is stale. Document ids are never
//! reused, so a reference from before a navigation is stale, never another
//! element.

use compositor::DocumentId;
use dom::NodeId;
use dom_api::FrameId;
use dom_api::{
    ElementLocator, FrameLayout, Probed, WebDriverAnswer, WebDriverQuery, WebDriverQueryError,
};
use gfx::{PointF, RectF};
use protocol::renderer::{QueriedElement, RendererHostMsg};
use protocol::webdriver::{ErrorKind, Locator, WebDriverCommand, WebDriverError, WebDriverValue};

use super::RenderThread;
use super::documents::engine_frame;
use super::webdriver::{BOOLEAN_ATTRIBUTES, error, stale};

/// A child frame an automated check went into: its id, its iframe, and
/// the iframe's rect in the top-level viewport.
type ProbedFrame = (FrameId, NodeId, Option<RectF>);

/// A query's failure as WebDriver's error.
fn query_error(failure: WebDriverQueryError) -> WebDriverError {
    match failure {
        WebDriverQueryError::Stale => stale(),
        WebDriverQueryError::NoSuchElement => error(ErrorKind::NoSuchElement, "not an element"),
        WebDriverQueryError::InvalidSelector(message) => error(ErrorKind::InvalidSelector, message),
        WebDriverQueryError::NotInteractable => error(
            ErrorKind::ElementNotInteractable,
            "the element isn't in view",
        ),
        WebDriverQueryError::ClickIntercepted => error(
            ErrorKind::ElementClickIntercepted,
            "another element would get the click",
        ),
        WebDriverQueryError::OutOfBounds => error(
            ErrorKind::MoveTargetOutOfBounds,
            "the element has no box to move to",
        ),
        WebDriverQueryError::NotShown => error(ErrorKind::NoSuchWindow, "the frame isn't shown"),
        WebDriverQueryError::NothingToCapture => error(
            ErrorKind::UnableToCaptureScreen,
            "nothing of the element is in view",
        ),
    }
}

fn action_error(failure: dom_api::WebDriverActionError) -> WebDriverError {
    match failure {
        dom_api::WebDriverActionError::Query(failure) => query_error(failure),
        dom_api::WebDriverActionError::NotInteractable(message) => {
            error(ErrorKind::ElementNotInteractable, message)
        }
        dom_api::WebDriverActionError::InvalidElementState(message) => {
            error(ErrorKind::InvalidElementState, message)
        }
        dom_api::WebDriverActionError::Unsupported(message) => {
            error(ErrorKind::UnsupportedOperation, message)
        }
    }
}

fn script_error(failure: dom_api::WebDriverScriptError) -> WebDriverError {
    error(ErrorKind::JavascriptError, failure.0)
}

fn rect_value(x: f32, y: f32, width: f32, height: f32) -> WebDriverValue {
    WebDriverValue::Object(vec![
        ("x".into(), WebDriverValue::Number(f64::from(x))),
        ("y".into(), WebDriverValue::Number(f64::from(y))),
        ("width".into(), WebDriverValue::Number(f64::from(width))),
        ("height".into(), WebDriverValue::Number(f64::from(height))),
    ])
}

fn point_value(point: gfx::PointF) -> WebDriverValue {
    WebDriverValue::Object(vec![
        ("x".into(), WebDriverValue::Number(f64::from(point.x))),
        ("y".into(), WebDriverValue::Number(f64::from(point.y))),
    ])
}

impl RenderThread {
    /// `RendererMsg::WebDriver` for the DOM's document.
    pub(super) fn run_webdriver(
        &mut self,
        request_id: u64,
        frame: Option<&str>,
        command: WebDriverCommand,
    ) {
        let result = self.webdriver_command(request_id, frame, command);
        match result {
            Ok(None) => {}
            Ok(Some(value)) => self.reply_webdriver(request_id, Ok(value)),
            Err(e) => self.reply_webdriver(request_id, Err(e)),
        }
        self.request_next_frame_if_dom_changed();
        self.send_harness_results();
    }

    fn webdriver_command(
        &mut self,
        request_id: u64,
        frame: Option<&str>,
        command: WebDriverCommand,
    ) -> Result<Option<WebDriverValue>, WebDriverError> {
        let cx = self.webdriver_context(frame)?;
        Ok(Some(match command {
            WebDriverCommand::ExecuteScript {
                body,
                args,
                is_async,
            } => {
                let args = args
                    .iter()
                    .map(|arg| self.value_in(cx, arg))
                    .collect::<Result<Vec<_>, _>>()?;
                let engine = self
                    .engine
                    .as_mut()
                    .ok_or_else(|| error(ErrorKind::NoSuchWindow, "no document"))?;
                engine
                    .execute_webdriver_script(cx.frame, request_id, &body, &args, is_async)
                    .map_err(script_error)?;
                self.webdriver_state.scripts.insert(request_id, cx.document);
                self.webdriver_state.documents.insert(cx.document, cx.frame);
                // Answered once it settles: maybe already.
                self.send_webdriver_results();
                return Ok(None);
            }
            WebDriverCommand::FindElements { using, value, from } => {
                let from = from
                    .as_deref()
                    .map(|from| self.element(cx, from))
                    .transpose()?;
                let using = match using {
                    Locator::Css => ElementLocator::Css,
                    Locator::TagName => ElementLocator::TagName,
                };
                let WebDriverAnswer::Elements(found) = self.query(
                    cx.frame,
                    &WebDriverQuery::Find {
                        using,
                        value: &value,
                        from,
                    },
                )?
                else {
                    unreachable!("Find answers with elements")
                };
                WebDriverValue::Array(
                    found
                        .into_iter()
                        .map(|element| WebDriverValue::Element(self.reference(cx, element)))
                        .collect(),
                )
            }
            WebDriverCommand::ElementRect { element } => {
                let element = self.element(cx, &element)?;
                let WebDriverAnswer::Rect(rect) =
                    self.query(cx.frame, &WebDriverQuery::Rect(element))?
                else {
                    unreachable!("Rect answers with a rect")
                };
                rect_value(rect.x, rect.y, rect.width, rect.height)
            }
            WebDriverCommand::ElementTagName { element } => {
                let element = self.element(cx, &element)?;
                text(self.query(cx.frame, &WebDriverQuery::TagName(element))?)
            }
            WebDriverCommand::ElementAttribute { element, name } => {
                let element = self.element(cx, &element)?;
                let value = text(self.query(cx.frame, &WebDriverQuery::Attribute(element, &name))?);
                // A boolean attribute's "true" if it has it, whatever its
                // value.
                if BOOLEAN_ATTRIBUTES.contains(&name.to_ascii_lowercase().as_str())
                    && value != WebDriverValue::Null
                {
                    WebDriverValue::String("true".into())
                } else {
                    value
                }
            }
            WebDriverCommand::ElementProperty { element, name } => {
                let element = self.element(cx, &element)?;
                let engine = self
                    .engine
                    .as_mut()
                    .ok_or_else(|| error(ErrorKind::NoSuchWindow, "no document"))?;
                let value = engine
                    .webdriver_element_property(cx.frame, element, &name)
                    .map_err(script_error)?;
                self.value_out(cx, value)?
            }
            WebDriverCommand::SwitchToFrame { index, element } => {
                self.switch_to_frame(cx, index, element.as_deref())?
            }
            WebDriverCommand::ElementClickPoint {
                element,
                in_view_center_only,
            } => {
                let element = self.element(cx, &element)?;
                let query = if in_view_center_only {
                    WebDriverQuery::InViewCenter(element)
                } else {
                    WebDriverQuery::ClickPoint(element)
                };
                let WebDriverAnswer::Point(point) = self.query(cx.frame, &query)? else {
                    unreachable!("a click point answers with a point")
                };
                point_value(point)
            }
            WebDriverCommand::ElementScreenshotRect { element } => {
                let element = self.element(cx, &element)?;
                let WebDriverAnswer::Rect(rect) =
                    self.query(cx.frame, &WebDriverQuery::ScreenshotRect(element))?
                else {
                    unreachable!("ScreenshotRect answers with a rect")
                };
                rect_value(rect.x, rect.y, rect.width, rect.height)
            }
            WebDriverCommand::FrameViewport => {
                let WebDriverAnswer::Rect(rect) =
                    self.query(cx.frame, &WebDriverQuery::Viewport)?
                else {
                    unreachable!("Viewport answers with a rect")
                };
                rect_value(rect.x, rect.y, rect.width, rect.height)
            }
            WebDriverCommand::ElementSendKeys { element, text } => {
                let element = self.element(cx, &element)?;
                let engine = self
                    .engine
                    .as_mut()
                    .ok_or_else(|| error(ErrorKind::NoSuchWindow, "no document"))?;
                let typed = engine
                    .webdriver_send_keys(cx.frame, element, &text)
                    .map_err(action_error)?;
                WebDriverValue::Bool(typed)
            }
            WebDriverCommand::ElementClear { element } => {
                let element = self.element(cx, &element)?;
                let engine = self
                    .engine
                    .as_mut()
                    .ok_or_else(|| error(ErrorKind::NoSuchWindow, "no document"))?;
                engine
                    .webdriver_clear(cx.frame, element)
                    .map_err(action_error)?;
                WebDriverValue::Null
            }
        }))
    }

    /// The frame `frame` names (the top-level one if none): "no such
    /// window" once it has gone (its iframe removed, or its parent's
    /// document gone).
    fn webdriver_context(&mut self, frame: Option<&str>) -> Result<Context, WebDriverError> {
        let gone = || error(ErrorKind::NoSuchWindow, format!("no frame {frame:?}"));
        let engine = self
            .engine
            .as_ref()
            .ok_or_else(|| error(ErrorKind::NoSuchWindow, "the tab has no document"))?;
        let top = engine.top_frame();
        let id = match frame {
            None => top,
            Some(id) => self.frame_id_of(id).ok_or_else(gone)?,
        };
        let (_, next) = engine_frame(&mut self.engine)
            .ok_or_else(|| error(ErrorKind::NoSuchWindow, "the tab has no document"))?;
        let document = if id == top {
            next.view.document_id()
        } else {
            next.view.child_mut(id).ok_or_else(gone)?.document_id()
        };
        Ok(Context {
            frame: id,
            top,
            document,
        })
    }

    /// The id of child frame `frame`, for the browser and the driver: this
    /// renderer's prefix, and the engine's number for it (the same as its
    /// documents change, as WebDriver's browsing context is).
    pub(super) fn frame_id(&self, frame: FrameId) -> String {
        child_frame_id(self.webdriver_state.prefix, frame)
    }

    /// The child frame `id` names, if it is one of this renderer's.
    fn frame_id_of(&self, id: &str) -> Option<FrameId> {
        let (prefix, number) = id.split_once('.')?;
        let number = number.strip_prefix('f')?;
        let (prefix, number) = (u64::from_str_radix(prefix, 16).ok()?, number.parse().ok()?);
        (prefix == self.webdriver_state.prefix).then_some(FrameId(number))
    }

    /// Switch To Frame's work in the page: the child frame of the context's
    /// frame by its index in its window's (`window[index]`) or by its iframe
    /// (an element of the context's document), or the context's own id
    /// (`null` for the top-level one: Switch To Parent Frame's check that
    /// the frame it goes back to is still there). "No such frame" for none.
    fn switch_to_frame(
        &mut self,
        cx: Context,
        index: Option<u16>,
        element: Option<&str>,
    ) -> Result<WebDriverValue, WebDriverError> {
        let no_frame = || error(ErrorKind::NoSuchFrame, "no such frame");
        let owner = match (index, element) {
            // Only an iframe has a content document: machopo has no `<frame>`.
            (_, Some(element)) => self.element(cx, element)?,
            (Some(index), None) => {
                let document = self.document_of(cx.frame).ok_or_else(no_frame)?;
                let engine = self.engine.as_mut().ok_or_else(no_frame)?;
                engine
                    .dom_mut()
                    .read(|cx| {
                        let window = cx.window_of(document.get(cx.gc))?;
                        let child = *dom_api::child_windows(cx, window).get(usize::from(index))?;
                        cx.frame_owner(cx.window_document(child))
                            .map(|owner| cx.node_id(owner))
                    })
                    .ok_or_else(no_frame)?
            }
            (None, None) => {
                return Ok(if cx.frame == cx.top {
                    WebDriverValue::Null
                } else {
                    WebDriverValue::String(self.frame_id(cx.frame))
                });
            }
        };
        let (_, next) = engine_frame(&mut self.engine).ok_or_else(no_frame)?;
        let frame = next.view.frame_owned_by(owner).ok_or_else(no_frame)?;
        Ok(WebDriverValue::String(self.frame_id(frame)))
    }

    /// The document `frame` shows now.
    fn document_of(&mut self, frame: FrameId) -> Option<dom::NodeRoot> {
        let top = self.engine.as_ref()?.top_frame();
        let (_, next) = engine_frame(&mut self.engine)?;
        if frame == top {
            return Some(next.view.document().clone());
        }
        Some(next.view.child_mut(frame)?.document().clone())
    }

    fn query(
        &mut self,
        frame: FrameId,
        query: &WebDriverQuery<'_>,
    ) -> Result<WebDriverAnswer, WebDriverError> {
        let engine = self
            .engine
            .as_mut()
            .ok_or_else(|| error(ErrorKind::NoSuchWindow, "no document"))?;
        engine.webdriver_query(frame, query).map_err(query_error)
    }

    /// The element reference `id` is to, in the context's document: stale
    /// for one of a document its frame showed before, "no such element"
    /// for any other's.
    fn element(&mut self, cx: Context, id: &str) -> Result<NodeId, WebDriverError> {
        let unknown = || error(ErrorKind::NoSuchElement, format!("no element {id:?}"));
        let mut parts = id.split('.');
        let (Some(prefix), Some(of), Some(node), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(unknown());
        };
        let (Ok(prefix), Ok(of), Ok(node)) = (
            u64::from_str_radix(prefix, 16),
            of.parse::<u64>(),
            node.parse::<u32>(),
        ) else {
            return Err(unknown());
        };
        if prefix != self.webdriver_state.prefix {
            return Err(unknown());
        }
        let of = DocumentId(of);
        if of != cx.document {
            // A document the frame showed before.
            return Err(
                if self.webdriver_state.documents.get(&of) == Some(&cx.frame) {
                    stale()
                } else {
                    unknown()
                },
            );
        }
        let element = NodeId::from_raw(node);
        self.query(cx.frame, &WebDriverQuery::Element(element))?;
        Ok(element)
    }

    /// A reference to `element`, of the context's document.
    fn reference(&mut self, cx: Context, element: NodeId) -> String {
        self.webdriver_state.documents.insert(cx.document, cx.frame);
        format!(
            "{:016x}.{}.{}",
            self.webdriver_state.prefix,
            cx.document.0,
            element.get()
        )
    }

    /// An engine's value, its elements made references and its windows
    /// WebDriver's (the top-level one's) or a frame's (a child frame's:
    /// WebDriver's "serialize a WindowProxy"; stale once its frame has
    /// gone).
    fn value_out(
        &mut self,
        cx: Context,
        value: dom_api::WebDriverValue,
    ) -> Result<WebDriverValue, WebDriverError> {
        use dom_api::WebDriverValue as Engine;
        Ok(match value {
            Engine::Null => WebDriverValue::Null,
            Engine::Bool(b) => WebDriverValue::Bool(b),
            Engine::Number(n) => WebDriverValue::Number(n),
            Engine::String(s) => WebDriverValue::String(s),
            Engine::Array(items) => WebDriverValue::Array(
                items
                    .into_iter()
                    .map(|item| self.value_out(cx, item))
                    .collect::<Result<_, _>>()?,
            ),
            Engine::Object(properties) => WebDriverValue::Object(
                properties
                    .into_iter()
                    .map(|(name, value)| Ok((name, self.value_out(cx, value)?)))
                    .collect::<Result<_, WebDriverError>>()?,
            ),
            Engine::Element(element) => {
                // One out of the document now (a script may make one): stale.
                self.query(cx.frame, &WebDriverQuery::Element(element))?;
                WebDriverValue::Element(self.reference(cx, element))
            }
            Engine::Window(document) => {
                let gone = || {
                    error(
                        ErrorKind::StaleElementReference,
                        "the window's frame has gone",
                    )
                };
                let (_, next) = engine_frame(&mut self.engine).ok_or_else(gone)?;
                let view = next.view.view_of(document).ok_or_else(gone)?;
                match view.child_frame() {
                    None => WebDriverValue::Window,
                    Some(frame) => WebDriverValue::Frame(self.frame_id(frame)),
                }
            }
        })
    }

    /// A value for the engine, its references found: a window's by its
    /// frame's document (any frame of the tab's: they are all the top-level
    /// one's descendants).
    fn value_in(
        &mut self,
        cx: Context,
        value: &WebDriverValue,
    ) -> Result<dom_api::WebDriverValue, WebDriverError> {
        use dom_api::WebDriverValue as Engine;
        Ok(match value {
            WebDriverValue::Null => Engine::Null,
            WebDriverValue::Bool(b) => Engine::Bool(*b),
            WebDriverValue::Number(n) => Engine::Number(*n),
            WebDriverValue::String(s) => Engine::String(s.clone()),
            WebDriverValue::Array(items) => Engine::Array(
                items
                    .iter()
                    .map(|item| self.value_in(cx, item))
                    .collect::<Result<_, _>>()?,
            ),
            WebDriverValue::Object(properties) => Engine::Object(
                properties
                    .iter()
                    .map(|(name, value)| Ok((name.clone(), self.value_in(cx, value)?)))
                    .collect::<Result<_, WebDriverError>>()?,
            ),
            WebDriverValue::Element(id) => Engine::Element(self.element(cx, id)?),
            WebDriverValue::Window => {
                let (dom, next) = engine_frame(&mut self.engine)
                    .ok_or_else(|| error(ErrorKind::NoSuchWindow, "the tab has no document"))?;
                let document = next.view.document().clone();
                Engine::Window(dom.read(|cx| cx.node_id(document.get(cx.gc))))
            }
            WebDriverValue::Frame(id) => {
                let no_frame = || error(ErrorKind::NoSuchFrame, format!("no frame {id:?}"));
                let frame = self.frame_id_of(id).ok_or_else(no_frame)?;
                let (dom, next) = engine_frame(&mut self.engine).ok_or_else(no_frame)?;
                let document = next
                    .view
                    .child_mut(frame)
                    .ok_or_else(no_frame)?
                    .document()
                    .clone();
                Engine::Window(dom.read(|cx| cx.node_id(document.get(cx.gc))))
            }
        })
    }

    /// `RendererMsg::QueryElement` for the DOM's document: its first
    /// element matching `selector`, in the child frame `frames` pick
    /// (`probe`), its rect in the top-level viewport and its text.
    pub(super) fn run_query_element(&mut self, query_id: u64, frames: &[String], selector: &str) {
        let element = self
            .probe(frames, selector)
            .map(|(_, probed)| QueriedElement {
                rect: probed.rect,
                text: probed.text,
            });
        let _ = self
            .to_browser
            .send(RendererHostMsg::QueryElementResult { query_id, element });
    }

    /// `RendererMsg::ScrollIntoViewIfNeeded` for the DOM's document:
    /// if the element's box's center is out of the top-level viewport, or
    /// out of the box of an iframe around it (its frame's viewport hides
    /// it), it is scrolled into view once next laid out, at the next main
    /// frame: in its frame, then each iframe around it in the frame around
    /// that, out to the top-level one.
    pub(super) fn run_scroll_into_view_if_needed(&mut self, frames: &[String], selector: &str) {
        let Some((
            owners,
            Probed {
                element,
                rect: Some(rect),
                ..
            },
        )) = self.probe(frames, selector)
        else {
            return;
        };
        let Some((_, next)) = engine_frame(&mut self.engine) else {
            return;
        };
        let port = next.viewport().size;
        let center = PointF::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
        let inside = |rect: RectF| {
            (rect.x..rect.x + rect.width).contains(&center.x)
                && (rect.y..rect.y + rect.height).contains(&center.y)
        };
        if inside(RectF::new(0.0, 0.0, port.width, port.height))
            && owners
                .iter()
                .all(|&(_, _, iframe)| iframe.is_some_and(inside))
        {
            return;
        }
        // The element in its frame, then each iframe in the frame before.
        let mut target = element;
        for &(frame, owner, _) in owners.iter().rev() {
            if let Some(view) = next.view.child_mut(frame) {
                view.scroll_into_view_after_layout(target);
            }
            target = owner;
        }
        next.view.scroll_into_view_after_layout(target);
        self.request_main_frame();
    }

    /// An automated check's element: the first matching `selector` in the
    /// child frame of the first iframe matching each of `frames` in turn,
    /// from the top-level frame; with those frames and their iframes (and
    /// the iframes' rects in the top-level viewport), the outermost first.
    fn probe(&mut self, frames: &[String], selector: &str) -> Option<(Vec<ProbedFrame>, Probed)> {
        let mut frame = self.engine.as_ref()?.top_frame();
        let mut owners = Vec::with_capacity(frames.len());
        for iframe in frames {
            let WebDriverAnswer::Probed(Some(found)) =
                self.query(frame, &WebDriverQuery::Probe(iframe)).ok()?
            else {
                return None;
            };
            let (_, next) = engine_frame(&mut self.engine)?;
            frame = next.view.frame_owned_by(found.element)?;
            owners.push((frame, found.element, found.rect));
        }
        match self.query(frame, &WebDriverQuery::Probe(selector)).ok()? {
            WebDriverAnswer::Probed(found) => Some((owners, found?)),
            other => unreachable!("Probe answers with what it found, not {other:?}"),
        }
    }

    /// From the top of the event loop: answers the scripts that settled,
    /// and those whose document has gone, which never will.
    pub(super) fn send_webdriver_results(&mut self) {
        if self.webdriver_state.scripts.is_empty() {
            return;
        }
        let settled = self
            .engine
            .as_mut()
            .map(|engine| engine.take_webdriver_results())
            .unwrap_or_default();
        for (request_id, result) in settled {
            let Some(document) = self.webdriver_state.scripts.remove(&request_id) else {
                continue;
            };
            let result = result.map_err(script_error).and_then(|value| {
                let cx = self.script_context(document).ok_or_else(|| {
                    error(
                        ErrorKind::JavascriptError,
                        "the document went away before the script settled",
                    )
                })?;
                self.value_out(cx, value)
            });
            self.reply_webdriver(request_id, result);
        }
        let gone: Vec<u64> = self
            .webdriver_state
            .scripts
            .clone()
            .into_iter()
            .filter(|&(_, document)| self.script_context(document).is_none())
            .map(|(request_id, _)| request_id)
            .collect();
        for request_id in gone {
            self.webdriver_state.scripts.remove(&request_id);
            self.reply_webdriver(
                request_id,
                Err(error(
                    ErrorKind::JavascriptError,
                    "the document went away before the script settled",
                )),
            );
        }
    }

    /// The context a script started in `document` is in, if its frame
    /// still shows it.
    fn script_context(&mut self, document: DocumentId) -> Option<Context> {
        let frame = *self.webdriver_state.documents.get(&document)?;
        let top = self.engine.as_ref()?.top_frame();
        let (_, next) = engine_frame(&mut self.engine)?;
        let shown = if frame == top {
            next.view.document_id()
        } else {
            next.view.child_mut(frame)?.document_id()
        };
        (shown == document).then_some(Context {
            frame,
            top,
            document,
        })
    }
}

/// The frame WebDriver's work is in (its current browsing context): the
/// engine's, the top-level one's (to tell them apart), and the id of the
/// document it shows now (what references name).
#[derive(Clone, Copy)]
struct Context {
    frame: FrameId,
    top: FrameId,
    document: DocumentId,
}

/// A text answer as WebDriver's value: null for none.
fn text(answer: WebDriverAnswer) -> WebDriverValue {
    match answer {
        WebDriverAnswer::Text(Some(text)) => WebDriverValue::String(text),
        WebDriverAnswer::Text(None) => WebDriverValue::Null,
        other => unreachable!("a text query answers with text, not {other:?}"),
    }
}

/// The id of child frame `frame` of a renderer whose prefix is `prefix`
/// (`RenderThread::frame_id`): WebDriver's, and DevTools'. The `f`
/// keeps it apart from a document's token (`prefix.<document>`, an
/// element's reference without its node), whose numbers are counted apart
/// and meet the frames'.
pub(super) fn child_frame_id(prefix: u64, frame: FrameId) -> String {
    format!("{prefix:016x}.f{}", frame.0)
}
