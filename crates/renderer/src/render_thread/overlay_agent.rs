//! DevTools' `Overlay` domain as the renderer answers it (Blink's
//! `InspectorOverlayAgent`, its `NodeHighlightTool` and
//! `SearchingForNodeTool`): a node highlighted on the page, its content,
//! padding, border and margin boxes and a label with its name and size;
//! the inspect mode, where the mouse over the page highlights what it is
//! over, and a click asks DevTools to inspect it.
//!
//! As Chrome's, in two halves. What is to be drawn is worked out as Blink's
//! `InspectorHighlight::AsProtocolValue` has it (`highlight`: the boxes'
//! paths with their colors, the element's info), the JSON Chrome hands its
//! overlay page; machopo draws that itself (`paint`), where Chrome's
//! overlay page runs devtools-frontend's `inspector_overlay`. The drawing
//! is a layer of its own the render thread adds over the page's as it
//! commits, in the viewport's space: a highlight that moves draws only
//! that layer again.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use base::numerics::f32_from_f64;
use compositor_paint::{DisplayItemList, PaintFlags, PaintOp};
use crossbeam_channel::Sender;
use dom::{Cx, NodeId, NodeKind, NodeRef};
use dom_api::DevToolsPage;
use gfx::{Color, PointF, RRectF, RectF, SizeF};
use protocol::renderer::RendererHostMsg;
use serde_json::{Value, json};

use super::dom_agent::{DomAgent, quad_json, with_layout};
use crate::frame_view::Frame;

/// A color of a highlight's configuration (DevTools' `DOM.RGBA`), as given:
/// its alpha kept as a fraction, which the paths' colors are serialized
/// with.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Rgba {
    r: u8,
    g: u8,
    b: u8,
    a: f32,
}

impl Rgba {
    const TRANSPARENT: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 0.0,
    };

    /// `value`, a `DOM.RGBA`; transparent for none (Blink's `ParseColor`).
    fn parse(value: &Value) -> Self {
        let channel = |name: &str| {
            let channel = value[name].as_f64().unwrap_or(0.0).clamp(0.0, 255.0);
            base::numerics::saturated_cast::<u8, f64>(channel.round())
        };
        if !value.is_object() {
            return Self::TRANSPARENT;
        }
        Self {
            r: channel("r"),
            g: channel("g"),
            b: channel("b"),
            a: f32_from_f64(value["a"].as_f64().unwrap_or(1.0).clamp(0.0, 1.0)),
        }
    }

    /// Blink's `Color::kTransparent`: no color at all, not only no alpha
    /// (a transparent red still goes in the paths, as Blink's does).
    fn is_transparent(self) -> bool {
        self == Self::TRANSPARENT
    }

    /// As CSS serializes it (Blink's `Color::SerializeAsCSSColor`):
    /// `rgb(…)` when opaque, else `rgba(…)`.
    fn css(self) -> String {
        if self.a >= 1.0 {
            format!("rgb({}, {}, {})", self.r, self.g, self.b)
        } else {
            format!("rgba({}, {}, {}, {})", self.r, self.g, self.b, self.a)
        }
    }
}

/// DevTools' `Overlay.HighlightConfig`, the parts machopo draws (Blink's
/// `InspectorHighlightConfig`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HighlightConfig {
    show_info: bool,
    show_styles: bool,
    show_rulers: bool,
    show_extension_lines: bool,
    show_accessibility_info: bool,
    color_format: String,
    content: Rgba,
    content_outline: Rgba,
    padding: Rgba,
    border: Rgba,
    margin: Rgba,
}

impl HighlightConfig {
    /// `config`, a command's `highlightConfig` (Blink's
    /// `HighlightConfigFromInspectorObject` and `ToHighlightConfig`).
    fn parse(config: &Value) -> Result<Self, String> {
        if !config.is_object() {
            return Err("Internal error: highlight configuration parameter is missing".to_owned());
        }
        let color_format = config["colorFormat"].as_str().unwrap_or("hex").to_owned();
        if !matches!(color_format.as_str(), "rgb" | "hex" | "hsl" | "hwb") {
            return Err("Unknown color format".to_owned());
        }
        Ok(Self {
            show_info: config["showInfo"].as_bool().unwrap_or(false),
            show_styles: config["showStyles"].as_bool().unwrap_or(false),
            show_rulers: config["showRulers"].as_bool().unwrap_or(false),
            show_extension_lines: config["showExtensionLines"].as_bool().unwrap_or(false),
            show_accessibility_info: config["showAccessibilityInfo"].as_bool().unwrap_or(true),
            color_format,
            content: Rgba::parse(&config["contentColor"]),
            content_outline: Rgba::TRANSPARENT,
            padding: Rgba::parse(&config["paddingColor"]),
            border: Rgba::parse(&config["borderColor"]),
            margin: Rgba::parse(&config["marginColor"]),
        })
    }

    /// Blink's `InspectorHighlight::DefaultConfig`, what
    /// `getHighlightObjectForTest` highlights with.
    fn default_for_test() -> Self {
        let rgba = |r, g, b| Rgba { r, g, b, a: 0.0 };
        Self {
            show_info: true,
            show_styles: false,
            show_rulers: true,
            show_extension_lines: true,
            show_accessibility_info: true,
            color_format: "hex".to_owned(),
            content: rgba(255, 0, 0),
            content_outline: rgba(128, 0, 0),
            padding: rgba(0, 255, 0),
            border: rgba(0, 0, 255),
            margin: rgba(255, 255, 255),
        }
    }
}

/// What the overlay shows: a node highlighted, with how (Blink's inspect
/// tool).
#[derive(Debug, Clone)]
struct Shown {
    node: NodeId,
    config: HighlightConfig,
    /// Shown by the inspect mode, under the mouse: it goes as the mode
    /// does.
    inspecting: bool,
}

/// A session's `Overlay`.
#[derive(Default)]
struct Session {
    enabled: bool,
    /// Its inspect mode, if on: how what the mouse is over is highlighted,
    /// and whether in user-agent shadow trees too.
    inspect: Option<(HighlightConfig, bool)>,
    /// What `setPausedInDebuggerMessage` asked to show.
    paused_message: Option<String>,
}

/// Blink's `InspectorOverlayAgent`, for every session of the page; one
/// overlay, which the last command shows.
pub(crate) struct OverlayAgent {
    to_browser: Sender<RendererHostMsg>,
    dom_agent: Rc<DomAgent>,
    sessions: RefCell<HashMap<String, Session>>,
    shown: RefCell<Option<Shown>>,
    /// Counts the changes of what is shown: the render thread asks for a
    /// frame once it moves.
    version: Cell<u64>,
    /// What was last drawn (the highlight, `highlight`'s, and the paused
    /// message), to draw again only when it changes.
    drawn: RefCell<Option<Value>>,
    /// The overlay's layer's id, the same for each drawing: the compositor
    /// takes a new drawing as the same layer's new content.
    layer_id: compositor::LayerId,
}

impl OverlayAgent {
    pub(crate) fn new(to_browser: Sender<RendererHostMsg>, dom_agent: Rc<DomAgent>) -> Self {
        Self {
            to_browser,
            dom_agent,
            sessions: RefCell::new(HashMap::new()),
            shown: RefCell::new(None),
            version: Cell::new(0),
            drawn: RefCell::new(None),
            layer_id: compositor::LayerId::next(),
        }
    }

    fn send(&self, session: &str, message: &Value) {
        let _ = self.to_browser.send(RendererHostMsg::DevToolsMessage {
            session: session.to_owned(),
            message: message.to_string(),
        });
    }

    fn tell(&self, session: &str, method: &str, params: Value) {
        self.send(session, &json!({ "method": method, "params": params }));
    }

    fn show(&self, shown: Option<Shown>) {
        *self.shown.borrow_mut() = shown;
        self.version.set(self.version.get().wrapping_add(1));
    }

    /// Whether something is highlighted: the render thread draws it at
    /// each frame, following the node as it moves.
    pub(crate) fn showing(&self) -> bool {
        self.shown.borrow().is_some()
    }

    /// The count of what was shown's changes.
    pub(crate) fn version(&self) -> u64 {
        self.version.get()
    }

    /// Whether a session is in the inspect mode: the mouse over the page
    /// is the overlay's, not the page's.
    pub(crate) fn inspecting(&self) -> bool {
        self.sessions
            .borrow()
            .values()
            .any(|session| session.enabled && session.inspect.is_some())
    }

    /// `session` has gone: what it showed goes with it.
    pub(crate) fn detach(&self, session: &str) {
        if self
            .sessions
            .borrow_mut()
            .remove(session)
            .is_some_and(|gone| gone.paused_message.is_some())
        {
            self.version.set(self.version.get().wrapping_add(1));
        }
        if !self.inspecting()
            && self
                .shown
                .borrow()
                .as_ref()
                .is_some_and(|shown| shown.inspecting)
        {
            self.show(None);
        }
    }

    /// The DOM is going: what it showed of it goes.
    pub(crate) fn dom_gone(&self) {
        self.show(None);
        *self.drawn.borrow_mut() = None;
    }

    /// Answers `message`, a command of `session`'s, if it is one of the
    /// agent's: whether it was.
    pub(crate) fn handle(&self, session: &str, message: &str, page: &mut dyn DevToolsPage) -> bool {
        let Ok(command) = serde_json::from_str::<Value>(message) else {
            return false;
        };
        let Some(method) = command.get("method").and_then(Value::as_str) else {
            return false;
        };
        let Some(method) = method.strip_prefix("Overlay.") else {
            return false;
        };
        if !COMMANDS.contains(&method) && !method.starts_with("setShow") {
            return false;
        }
        let id = command.get("id").cloned().unwrap_or(Value::Null);
        let params = command.get("params").cloned().unwrap_or_else(|| json!({}));
        // A command with no `highlightConfig` where the protocol says one
        // must be is refused before it reaches Blink's agent, as an invalid
        // parameter.
        if method == "highlightNode" && !params["highlightConfig"].is_object() {
            let error = json!({
                "code": -32602,
                "message": "Invalid parameters",
                "data": "Failed to deserialize params.highlightConfig - BINDINGS: mandatory field missing",
            });
            self.send(session, &json!({ "id": id, "error": error }));
            return true;
        }
        let answer = self.answer(session, method, &params, page);
        let message = match answer {
            Ok(result) => json!({ "id": id, "result": result }),
            Err(error) => json!({ "id": id, "error": { "code": -32000, "message": error } }),
        };
        self.send(session, &message);
        true
    }

    fn answer(
        &self,
        name: &str,
        method: &str,
        params: &Value,
        page: &mut dyn DevToolsPage,
    ) -> Result<Value, String> {
        match method {
            "enable" => {
                if !self.dom_agent.enabled(name) {
                    return Err("DOM should be enabled first".to_owned());
                }
                self.sessions
                    .borrow_mut()
                    .entry(name.to_owned())
                    .or_default()
                    .enabled = true;
                Ok(json!({}))
            }
            "disable" => {
                self.sessions.borrow_mut().remove(name);
                if self
                    .shown
                    .borrow()
                    .as_ref()
                    .is_some_and(|shown| shown.inspecting)
                {
                    self.show(None);
                }
                Ok(json!({}))
            }
            "highlightNode" => {
                let node = self.dom_agent.node_param(name, params, page)?;
                let config = HighlightConfig::parse(&params["highlightConfig"])?;
                self.show(Some(Shown {
                    node,
                    config,
                    inspecting: false,
                }));
                Ok(json!({}))
            }
            "hideHighlight" => {
                self.show(None);
                Ok(json!({}))
            }
            "setInspectMode" => {
                let mode = params["mode"].as_str().unwrap_or_default();
                if !matches!(
                    mode,
                    "none" | "searchForNode" | "searchForUAShadowDOM" | "captureAreaScreenshot"
                ) {
                    return Err(format!("Unknown mode \"{mode}\" was provided."));
                }
                let config = if params["highlightConfig"].is_object() {
                    HighlightConfig::parse(&params["highlightConfig"])?
                } else {
                    HighlightConfig::parse(&json!({}))?
                };
                let inspect = matches!(mode, "searchForNode" | "searchForUAShadowDOM")
                    .then(|| (config, mode == "searchForUAShadowDOM"));
                self.sessions
                    .borrow_mut()
                    .entry(name.to_owned())
                    .or_default()
                    .inspect = inspect;
                if !self.inspecting()
                    && self
                        .shown
                        .borrow()
                        .as_ref()
                        .is_some_and(|shown| shown.inspecting)
                {
                    self.show(None);
                }
                Ok(json!({}))
            }
            "getHighlightObjectForTest" => {
                let id = params["nodeId"].as_i64().unwrap_or(0);
                let node = self
                    .dom_agent
                    .node_param(name, &json!({ "nodeId": id }), page)?;
                let mut config = HighlightConfig::default_for_test();
                config.show_styles = params["includeStyle"].as_bool().unwrap_or(false);
                config.show_accessibility_info =
                    params["showAccessibilityInfo"].as_bool().unwrap_or(true);
                config.color_format = params["colorFormat"].as_str().unwrap_or("hex").to_owned();
                let highlight = with_layout(page, node, |cx, frame, element, document| {
                    Some(highlight(cx, frame, element, document, &config, true))
                })
                .unwrap_or_else(|| json!({}));
                Ok(json!({ "highlight": highlight }))
            }
            "setPausedInDebuggerMessage" => {
                self.sessions
                    .borrow_mut()
                    .entry(name.to_owned())
                    .or_default()
                    .paused_message = params["message"].as_str().map(str::to_owned);
                self.version.set(self.version.get().wrapping_add(1));
                Ok(json!({}))
            }
            // The other overlays machopo doesn't draw: asked to show none,
            // what is asked is so; asked to show one, an error that says
            // so, as `Network`'s commands machopo can't do are answered.
            _ => {
                let asks = params.as_object().is_some_and(|params| {
                    params.values().any(|value| match value {
                        Value::Bool(on) => *on,
                        Value::Array(items) => !items.is_empty(),
                        Value::Object(_) => true,
                        _ => false,
                    })
                });
                if asks {
                    Err(format!("Overlay.{method}: machopo can't show it"))
                } else {
                    Ok(json!({}))
                }
            }
        }
    }

    /// The mouse moved to `point` (CSS px of the viewport) in the inspect
    /// mode (Blink's `SearchingForNodeTool::HandleMouseMove`): what it is
    /// over highlighted, and the session told (`nodeHighlightRequested`,
    /// with the path to it sent).
    pub(crate) fn mouse_moved(&self, point: PointF, page: &mut dyn DevToolsPage) {
        let Some((name, config, user_agent)) = self.inspecting_session() else {
            return;
        };
        let Some(node) = super::dom_agent::node_at(page, point, false, user_agent) else {
            return;
        };
        if self
            .shown
            .borrow()
            .as_ref()
            .is_some_and(|shown| shown.node == node)
        {
            return;
        }
        self.show(Some(Shown {
            node,
            config,
            inspecting: true,
        }));
        if let Some(id) = self.dom_agent.push_node(&name, node, page.dom()) {
            self.tell(
                &name,
                "Overlay.nodeHighlightRequested",
                json!({ "nodeId": id }),
            );
        }
    }

    /// A click at `point` in the inspect mode (Blink's `Inspect`): DevTools
    /// is asked to inspect what it is over (`inspectNodeRequested`).
    pub(crate) fn clicked(&self, point: PointF, page: &mut dyn DevToolsPage) {
        let Some((name, _, user_agent)) = self.inspecting_session() else {
            return;
        };
        if let Some(node) = super::dom_agent::node_at(page, point, false, user_agent) {
            self.tell(
                &name,
                "Overlay.inspectNodeRequested",
                json!({ "backendNodeId": node.get() }),
            );
        }
    }

    /// The session whose inspect mode is on, if one's is, and its mode.
    fn inspecting_session(&self) -> Option<(String, HighlightConfig, bool)> {
        self.sessions.borrow().iter().find_map(|(name, session)| {
            let (config, user_agent) = session.inspect.as_ref().filter(|_| session.enabled)?;
            Some((name.clone(), config.clone(), *user_agent))
        })
    }

    /// The overlay's layer for this frame, if it moved or what it shows
    /// changed since it was last drawn (`Some(None)`: there is none now),
    /// else `None`: the layout of `frame` is up to date.
    pub(crate) fn layer(
        &self,
        cx: &Cx<'_, '_>,
        frame: &mut Frame,
    ) -> Option<Option<compositor::PictureLayer>> {
        // The message of an enabled session's (Chrome shows the last
        // command's: one, as DevTools has one session that sends it).
        let paused = self
            .sessions
            .borrow()
            .values()
            .filter(|session| session.enabled)
            .find_map(|session| session.paused_message.clone());
        let shown = self.shown.borrow().clone();
        let highlight = shown.and_then(|shown| {
            let element = cx.node_by_id(shown.node)?;
            let document = cx.node_id(cx.document_of(element));
            Some(highlight(
                cx,
                frame,
                element,
                document,
                &shown.config,
                shown.config.show_info,
            ))
        });
        let drawing = (highlight.is_some() || paused.is_some())
            .then(|| json!({ "highlight": highlight, "paused": paused }));
        if *self.drawn.borrow() == drawing {
            return None;
        }
        *self.drawn.borrow_mut() = drawing;
        if highlight.is_none() && paused.is_none() {
            return Some(None);
        }
        let viewport = frame.viewport_size();
        let mut shape = |text: &str, size, bold| frame.shape_label(text, size, bold);
        let mut list = DisplayItemList::new();
        // Script paused: the page dimmed, the highlight over it, the
        // message over both (Chrome's `PausedInDebuggerTool` is the
        // overlay page's while paused; a highlight shows over it).
        if paused.is_some() {
            list.push_op(PaintOp::DrawRect {
                rect: RectF::new(0.0, 0.0, viewport.width, viewport.height),
                flags: PaintFlags::fill(PAUSED_DIM),
            });
        }
        if let Some(highlight) = &highlight {
            paint(&mut list, highlight, viewport, &mut shape);
        }
        if let Some(message) = &paused {
            paint_paused_message(&mut list, message, viewport, &mut shape);
        }
        list.finalize();
        Some(Some(compositor::PictureLayer {
            id: self.layer_id,
            bounds: viewport,
            display_list: Arc::new(list),
            contents_opaque: false,
            background_color: Color::TRANSPARENT,
            invalidation: None,
            properties: compositor::LayerProperties {
                fixed: true,
                ..Default::default()
            },
        }))
    }
}

/// The commands the agent answers, besides the `setShow…` ones; another of
/// `Overlay`'s is the inspector's (V8 has none), which answers it isn't
/// there.
const COMMANDS: &[&str] = &[
    "enable",
    "disable",
    "highlightNode",
    "hideHighlight",
    "setInspectMode",
    "getHighlightObjectForTest",
    "setPausedInDebuggerMessage",
];

/// What is to be drawn of `element` (of `document`) highlighted with
/// `config`, as Blink's `InspectorHighlight::AsProtocolValue` has it: its
/// content, padding, border and margin boxes as paths in the top-level
/// frame's viewport, each with its fill color and name, and, with
/// `element_info`, its name and size (the accessibility and contrast
/// Chrome's has, machopo doesn't work out).
fn highlight<'gc>(
    cx: &Cx<'_, 'gc>,
    frame: &mut Frame,
    element: NodeRef<'gc>,
    document: NodeId,
    config: &HighlightConfig,
    element_info: bool,
) -> Value {
    let node = cx.node_id(element);
    let mut paths = Vec::new();
    if let Some(quads) = frame.box_quads_in_top(document, node) {
        for (quad, fill, outline, name) in [
            (
                quads.content,
                config.content,
                config.content_outline,
                "content",
            ),
            (quads.padding, config.padding, Rgba::TRANSPARENT, "padding"),
            (quads.border, config.border, Rgba::TRANSPARENT, "border"),
            (quads.margin, config.margin, Rgba::TRANSPARENT, "margin"),
        ] {
            let points = quad_json(&quad);
            let mut path = json!({
                "path": [
                    "M", points[0], points[1],
                    "L", points[2], points[3],
                    "L", points[4], points[5],
                    "L", points[6], points[7],
                    "Z",
                ],
                "fillColor": fill.css(),
                "name": name,
            });
            if !outline.is_transparent() {
                path["outlineColor"] = outline.css().into();
            }
            paths.push(path);
        }
    }
    let mut value = json!({
        "paths": paths,
        "showRulers": config.show_rulers,
        "showExtensionLines": config.show_extension_lines,
        "showAccessibilityInfo": config.show_accessibility_info,
        "colorFormat": config.color_format,
    });
    if element_info && cx.kind(element) == NodeKind::Element {
        let mut info = json!({
            "tagName": cx.node_name(element).to_str().to_lowercase(),
            "idValue": cx
                .attribute_value_by_id(element, dom::names::ID)
                .map(|id| id.borrow().to_str().into_owned())
                .unwrap_or_default(),
        });
        let mut classes: Vec<String> = Vec::new();
        if let Some(names) = cx.attribute_value_by_id(element, dom::names::CLASS) {
            for class in names.borrow().to_str().split_ascii_whitespace() {
                if !classes.iter().any(|known| known == class) {
                    classes.push(class.to_owned());
                }
            }
        }
        if !classes.is_empty() {
            info["className"] = classes
                .iter()
                .map(|class| format!(".{class}"))
                .collect::<String>()
                .into();
        }
        let rects = frame
            .view
            .view_of(document)
            .map(|view| view.client_rects(node))
            .unwrap_or_default();
        if !rects.is_empty() {
            let bounds = layout::bounding_rect(&rects);
            info["nodeWidth"] = number(bounds.width).into();
            info["nodeHeight"] = number(bounds.height).into();
        }
        if config.show_styles {
            info["style"] = style_info(cx, frame, element);
        }
        info["showAccessibilityInfo"] = config.show_accessibility_info.into();
        value["elementInfo"] = info;
    }
    value
}

/// What the label shows of `element`'s computed style (Blink's
/// `BuildStyleInfo`): the color and font for an element with text of its
/// own, then its padding, margin and background color; the colors as
/// `#RRGGBBAA` (Blink's `ToHEXA`), whatever the format asked (the overlay
/// page writes them in it), and as fractions.
fn style_info<'gc>(cx: &Cx<'_, 'gc>, frame: &mut Frame, element: NodeRef<'gc>) -> Value {
    let has_text = std::iter::successors(cx.first_child(element), |&child| cx.next_sibling(child))
        .any(|child| {
            cx.kind(child) == NodeKind::Text
                && cx.text_data(child).is_some_and(|data| {
                    !data
                        .borrow()
                        .to_str()
                        .trim_matches(|c: char| c.is_ascii_whitespace())
                        .is_empty()
                })
        });
    let mut names = Vec::new();
    if has_text {
        names.extend(["color", "font-family", "font-size", "line-height"]);
    }
    names.extend(["padding", "margin", "background-color"]);
    let values = dom_api::computed_values_for_devtools(cx, frame, element, &names);
    let mut style = json!({});
    for (name, value) in names.into_iter().zip(values) {
        if value.is_empty() {
            continue;
        }
        if matches!(name, "color" | "background-color") {
            // The color's channels as fractions too, which the label's
            // swatch reads (Blink's `-unclamped-rgba`).
            let color = parse_css_color(&value);
            let fraction = |channel: u8| f64::from(channel) / 255.0;
            style[format!("{name}-unclamped-rgba")] = json!([
                fraction(color.r),
                fraction(color.g),
                fraction(color.b),
                fraction(color.a)
            ]);
            style[name] = format!(
                "#{:02X}{:02X}{:02X}{:02X}",
                color.r, color.g, color.b, color.a
            )
            .into();
            continue;
        }
        style[name] = value.into();
    }
    style
}

/// `value` as Blink's `String::Number` writes a size: a whole number with
/// no fraction, else at most six significant digits.
fn number(value: f32) -> String {
    let value = f64::from(value);
    if value == value.trunc() {
        return format!("{value:.0}");
    }
    let whole = value.abs().trunc();
    let digits = if whole >= 1.0 {
        base::numerics::saturated_cast::<usize, f64>(whole.log10().floor()).saturating_add(1)
    } else {
        0
    };
    let decimals = 6_usize.saturating_sub(digits);
    let written = format!("{value:.decimals$}");
    written
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

/// A color a highlight's path has (`Rgba::css`'s), or a computed one, read
/// back to paint; `#RRGGBBAA` too (`style_info`'s).
fn parse_css_color(text: &str) -> Color {
    if let Some(hex) = text.strip_prefix('#')
        && hex.len() == 8
        && let Ok(value) = u32::from_str_radix(hex, 16)
    {
        let [r, g, b, a] = value.to_be_bytes();
        return Color::rgba(r, g, b, a);
    }
    let inner = text
        .strip_prefix("rgba(")
        .or_else(|| text.strip_prefix("rgb("))
        .and_then(|rest| rest.strip_suffix(')'));
    let Some(inner) = inner else {
        return Color::TRANSPARENT;
    };
    let parts: Vec<f32> = inner
        .split(',')
        .filter_map(|part| part.trim().parse().ok())
        .collect();
    let channel = |at: usize| {
        base::numerics::saturated_cast::<u8, f32>(parts.get(at).copied().unwrap_or(0.0))
    };
    let alpha = parts.get(3).copied().unwrap_or(1.0);
    Color::rgba(
        channel(0),
        channel(1),
        channel(2),
        base::numerics::saturated_cast::<u8, f32>((alpha * 255.0).round()),
    )
}

/// The label's room: around its content, between a name and its value,
/// between its rows; a color's swatch.
const PADDING: f32 = 8.0;
const GAP: f32 = 8.0;
const ROW_GAP: f32 = 4.0;
const SWATCH: f32 = 10.0;

/// The label's colors, as Chrome's overlay has them: the tag's purple, the
/// id's and the classes' blue, the size's gray.
const TAG_COLOR: Color = Color::rgb(0x88, 0x12, 0x80);
const ID_COLOR: Color = Color::rgb(0x1a, 0x1a, 0xa6);
const SIZE_COLOR: Color = Color::rgb(0x5f, 0x63, 0x68);

/// Script paused's overlay, as `inspector_overlay`'s `tool_paused.css` has
/// it: the page under 31% black, the message's box pale yellow with a gray
/// border.
const PAUSED_DIM: Color = Color::rgba(0, 0, 0, 79);
const PAUSED_BOX: Color = Color::rgb(255, 255, 194);
const PAUSED_BORDER: Color = Color::rgb(202, 202, 202);
const PAUSED_TEXT_SIZE: f32 = 13.0;
const SWATCH_BORDER: Color = Color::rgb(0xda, 0xdc, 0xe0);
const LABEL_SIZE: f32 = 12.0;

/// Draws `highlight` (`highlight`'s value) into `list`, over a viewport of `viewport`
/// CSS px (Chrome's overlay page draws the same JSON with
/// `inspector_overlay`'s `drawHighlight`): each path filled with its color
/// and outlined with its outline's; with the element's info, a label below
/// the border box (above if it doesn't fit), kept in the viewport, its
/// text laid out by `shape` (text, size, bold).
fn paint(
    list: &mut DisplayItemList,
    highlight: &Value,
    viewport: SizeF,
    shape: &mut dyn FnMut(&str, f32, bool) -> Option<gfx::ShapedText>,
) {
    // Around every path's points, rounded (the overlay page's `bounds`, which
    // its `buildPath` grows): what the label goes by.
    let mut bounds: Option<RectF> = None;
    // The path before each, inside it, which its fill leaves out.
    let mut inner: Option<Vec<PointF>> = None;
    for path in highlight["paths"].as_array().into_iter().flatten() {
        let mut points = Vec::new();
        let mut commands = path["path"].as_array().into_iter().flatten();
        while let Some(command) = commands.next() {
            if matches!(command.as_str(), Some("M" | "L")) {
                let x = f32_from_f64(commands.next().and_then(Value::as_f64).unwrap_or(0.0));
                let y = f32_from_f64(commands.next().and_then(Value::as_f64).unwrap_or(0.0));
                points.push(PointF::new(x, y));
            }
        }
        for point in &points {
            let (x, y) = (point.x.round(), point.y.round());
            bounds = Some(match bounds {
                None => RectF::new(x, y, 0.0, 0.0),
                Some(around) => {
                    let left = around.x.min(x);
                    let top = around.y.min(y);
                    let right = (around.x + around.width).max(x);
                    let bottom = (around.y + around.height).max(y);
                    RectF::new(left, top, right - left, bottom - top)
                }
            });
        }
        let shape_path = gfx::Path::polygons([&points]);
        // Each box's ring only, the box inside it left out, as the overlay
        // page draws them (from the margin in, each cutting out the next:
        // `drawHighlight`'s `destination-out`).
        let ring = match &inner {
            Some(inside) => {
                gfx::Path::polygons([&points, inside]).with_fill_rule(gfx::FillRule::EvenOdd)
            }
            None => shape_path.clone(),
        };
        let fill = parse_css_color(path["fillColor"].as_str().unwrap_or_default());
        if fill.a > 0 {
            list.push_op(PaintOp::DrawPath {
                path: ring,
                flags: PaintFlags::fill(fill),
            });
        }
        if let Some(outline) = path["outlineColor"].as_str() {
            list.push_op(PaintOp::DrawPath {
                path: shape_path,
                flags: PaintFlags::stroke(parse_css_color(outline), 1.0),
            });
        }
        inner = Some(points);
    }
    if let (Some(info), Some(bounds)) = (highlight.get("elementInfo"), bounds) {
        let format = highlight["colorFormat"].as_str().unwrap_or("hex");
        paint_label(list, info, format, bounds, viewport, shape);
    }
}

/// Script paused's message (`setPausedInDebuggerMessage`'s), as the overlay
/// page's `PausedInDebuggerTool` draws it: in a pale yellow box at the
/// viewport's top, in its middle. Chrome's has buttons to go on and step
/// after it; DevTools has them too, machopo's has none.
fn paint_paused_message(
    list: &mut DisplayItemList,
    message: &str,
    viewport: SizeF,
    shape: &mut dyn FnMut(&str, f32, bool) -> Option<gfx::ShapedText>,
) {
    const TOP: f32 = 10.0;
    const HEIGHT: f32 = 22.0;
    const SIDE: f32 = 5.0;
    let Some(text) = shape(message, PAUSED_TEXT_SIZE, false) else {
        return;
    };
    let width = text.width + 2.0 * SIDE;
    let x = ((viewport.width - width) / 2.0).max(0.0);
    let rect = RectF::new(x, TOP, width, HEIGHT);
    list.push_op(PaintOp::DrawRect {
        rect,
        flags: PaintFlags::fill(PAUSED_BOX),
    });
    list.push_op(PaintOp::DrawRect {
        rect,
        flags: PaintFlags::stroke(PAUSED_BORDER, 1.0),
    });
    let baseline = TOP + (HEIGHT + text.metrics.ascent - text.metrics.descent) / 2.0;
    draw_text(
        list,
        text,
        PointF::new(x + SIDE, baseline),
        Color::BLACK,
        rect,
    );
}

/// The element's label (Chrome's tooltip, `inspector_overlay`'s
/// `createElementDescription`): its name, id and classes, and its size;
/// with its style, a row each for its color and background (with a
/// swatch), font, padding and margin, those that say something.
fn paint_label(
    list: &mut DisplayItemList,
    info: &Value,
    format: &str,
    bounds: RectF,
    viewport: SizeF,
    shape: &mut dyn FnMut(&str, f32, bool) -> Option<gfx::ShapedText>,
) {
    let mut parts: Vec<(String, Color, bool)> = vec![(
        info["tagName"].as_str().unwrap_or_default().to_owned(),
        TAG_COLOR,
        true,
    )];
    let id = info["idValue"].as_str().unwrap_or_default();
    if !id.is_empty() {
        parts.push((format!("#{id}"), ID_COLOR, true));
    }
    if let Some(classes) = info["className"].as_str() {
        parts.push((classes.to_owned(), ID_COLOR, true));
    }
    let shaped: Vec<(gfx::ShapedText, Color)> = parts
        .iter()
        .filter_map(|(text, color, bold)| Some((shape(text, LABEL_SIZE, *bold)?, *color)))
        .collect();
    let Some((first, _)) = shaped.first() else {
        return;
    };
    let ascent = first.metrics.ascent;
    let line = ascent + first.metrics.descent;
    // The size at the header's end, apart from the name.
    let size_text = match (info["nodeWidth"].as_str(), info["nodeHeight"].as_str()) {
        (Some(width), Some(height)) => shape(&format!("{width}\u{d7}{height}"), LABEL_SIZE, false),
        _ => None,
    };
    let name_width: f32 = shaped.iter().map(|(text, _)| text.width).sum();
    let size_width = size_text
        .as_ref()
        .map_or(0.0, |text| text.width + 2.0 * GAP);
    let header_width = name_width + size_width;
    let rows: Vec<(gfx::ShapedText, gfx::ShapedText, Option<Color>)> =
        style_rows(&info["style"], format)
            .into_iter()
            .filter_map(|(name, value, swatch)| {
                Some((
                    shape(name, LABEL_SIZE, false)?,
                    shape(&value, LABEL_SIZE, false)?,
                    swatch,
                ))
            })
            .collect();
    let swatch_room = |swatch: &Option<Color>| if swatch.is_some() { SWATCH + 4.0 } else { 0.0 };
    let rows_width = rows
        .iter()
        .map(|(name, value, swatch)| name.width + 2.0 * GAP + swatch_room(swatch) + value.width)
        .fold(0.0, f32::max);
    let content_width = header_width.max(rows_width);
    #[expect(clippy::cast_precision_loss, reason = "a handful of rows")]
    let rows_height = if rows.is_empty() {
        0.0
    } else {
        ROW_GAP + rows.len() as f32 * (line + ROW_GAP)
    };
    let size = SizeF::new(
        content_width + 2.0 * PADDING,
        line + rows_height + 2.0 * PADDING,
    );
    let Some(place) = place_label(bounds, size, viewport) else {
        return;
    };
    let (x, y) = (place.origin.x, place.origin.y);
    let rect = RectF::new(x, y, size.width, size.height);
    list.push_op(PaintOp::DrawRRect {
        rrect: RRectF::uniform(rect.offset(0.0, 1.0), 3.0),
        flags: PaintFlags::fill(Color::rgba(0, 0, 0, 40)),
    });
    list.push_op(PaintOp::DrawRRect {
        rrect: RRectF::uniform(rect, 3.0),
        flags: PaintFlags::fill(Color::WHITE),
    });
    // The arrow at the element: under the label above it, over the label
    // below it.
    if let Some((arrow_x, above)) = place.arrow {
        let (base, tip) = if above {
            (y + size.height, y + size.height + ARROW_HALF_WIDTH)
        } else {
            (y, y - ARROW_HALF_WIDTH)
        };
        let triangle = [
            PointF::new(arrow_x, base),
            PointF::new(arrow_x + 2.0 * ARROW_HALF_WIDTH, base),
            PointF::new(arrow_x + ARROW_HALF_WIDTH, tip),
        ];
        list.push_op(PaintOp::DrawPath {
            path: gfx::Path::polygons([triangle]),
            flags: PaintFlags::fill(Color::WHITE),
        });
    }
    let mut at = x + PADDING;
    let baseline = y + PADDING + ascent;
    for (text, color) in shaped {
        let width = text.width;
        draw_text(list, text, PointF::new(at, baseline), color, rect);
        at += width;
    }
    let end = x + PADDING + content_width;
    if let Some(size) = size_text {
        let at = end - size.width;
        draw_text(list, size, PointF::new(at, baseline), SIZE_COLOR, rect);
    }
    // The rows, the name at the start, the value (after its swatch) at the
    // end.
    let mut baseline = baseline + ROW_GAP + line + ROW_GAP;
    for (name, value, swatch) in rows {
        draw_text(
            list,
            name,
            PointF::new(x + PADDING, baseline),
            SIZE_COLOR,
            rect,
        );
        let value_at = end - value.width;
        if let Some(color) = swatch {
            let swatch = RectF::new(value_at - SWATCH - 4.0, baseline - SWATCH, SWATCH, SWATCH);
            list.push_op(PaintOp::DrawRect {
                rect: swatch,
                flags: PaintFlags::fill(color),
            });
            list.push_op(PaintOp::DrawRect {
                rect: swatch,
                flags: PaintFlags::stroke(SWATCH_BORDER, 1.0),
            });
        }
        draw_text(
            list,
            value,
            PointF::new(value_at, baseline),
            Color::BLACK,
            rect,
        );
        baseline += line + ROW_GAP;
    }
}

/// The arrow's half width, and how far the label stays from the viewport's
/// sides (`drawElementTitle`'s `arrowHalfWidth`, `pageMargin`).
const ARROW_HALF_WIDTH: f32 = 8.0;
const PAGE_MARGIN: f32 = 2.0;

/// Where a label of `size` goes for an element `bounds` covers (all its
/// boxes'), in a viewport of `viewport`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct LabelPlace {
    origin: PointF,
    /// The arrow's left edge, and whether the label is above the element
    /// (the arrow under it, pointing down); none when it is hidden.
    arrow: Option<(f32, bool)>,
}

/// Where the overlay page puts the label (`inspector_overlay`'s
/// `drawElementTitle`): above the element if it fits, else below it; its
/// left edge at the element's, kept in the viewport, the arrow at the
/// element's left (in its middle if the element is narrow); none when it
/// would overlap the element without being inside it.
fn place_label(bounds: RectF, size: SizeF, viewport: SizeF) -> Option<LabelPlace> {
    let arrow_width = 2.0 * ARROW_HALF_WIDTH;
    let arrow_inset = ARROW_HALF_WIDTH + 2.0;
    let container_min_x = PAGE_MARGIN + arrow_inset;
    let container_max_x = viewport.width - PAGE_MARGIN - arrow_inset - arrow_width;
    // `constrainNumber`: `value` kept from `min` to `max`, `min` winning.
    let constrain = |value: f32, min: f32, max: f32| {
        if value < min {
            min
        } else if value > max {
            max
        } else {
            value
        }
    };
    let (min_x, max_x) = (bounds.x, bounds.x + bounds.width);
    let (min_y, max_y) = (bounds.y, bounds.y + bounds.height);
    let arrow_x = if max_x - min_x < arrow_width + 2.0 * arrow_inset {
        (min_x + max_x) * 0.5 - ARROW_HALF_WIDTH
    } else {
        let from_left = min_x + arrow_inset;
        let from_right = max_x - arrow_inset - arrow_width;
        if from_left > container_min_x && from_left < container_max_x {
            from_left
        } else {
            constrain(container_min_x, from_left, from_right)
        }
    };
    let arrow_hidden = arrow_x < container_min_x || arrow_x > container_max_x;
    let x = constrain(
        arrow_x - arrow_inset,
        PAGE_MARGIN,
        viewport.width - size.width - PAGE_MARGIN,
    );
    let mut y = min_y - ARROW_HALF_WIDTH - size.height;
    let mut above = true;
    if y < 0.0 {
        y = (viewport.height - size.height).min(max_y + ARROW_HALF_WIDTH);
        above = false;
    } else if min_y > viewport.height {
        y = viewport.height - ARROW_HALF_WIDTH - size.height;
    }
    let includes = x >= min_x && x + size.width <= max_x && y >= min_y && y + size.height <= max_y;
    let overlaps = x < max_x && x + size.width > min_x && y < max_y && y + size.height > min_y;
    if overlaps && !includes {
        return None;
    }
    Some(LabelPlace {
        origin: PointF::new(x, y),
        arrow: (!arrow_hidden && !includes).then_some((arrow_x, above)),
    })
}

/// The label's rows of an element's style (`elementInfo.style`), as
/// `inspector_overlay`'s `createElementDescription` has them: name, value
/// and the swatch of a color, its value written in `format`. A transparent
/// color, a font of no size, and margin and padding of none say nothing.
fn style_rows(style: &Value, format: &str) -> Vec<(&'static str, String, Option<Color>)> {
    let text = |name: &str| style[name].as_str().filter(|value| !value.is_empty());
    let mut rows = Vec::new();
    for (name, property) in [
        ("Color", "color"),
        ("Font", ""),
        ("Background", "background-color"),
    ] {
        if property.is_empty() {
            if let (Some(size), Some(family)) = (text("font-size"), text("font-family"))
                && size != "0px"
            {
                rows.push(("Font", format!("{size} {family}"), None));
            }
            continue;
        }
        if let Some(hexa) = text(property) {
            let color = parse_css_color(hexa);
            if color.a > 0 {
                rows.push((name, format_color(color, format), Some(color)));
            }
        }
    }
    for (name, property) in [("Margin", "margin"), ("Padding", "padding")] {
        if let Some(value) = text(property).filter(|value| *value != "0px") {
            rows.push((name, value.to_owned(), None));
        }
    }
    rows
}

/// `color` as the overlay page writes a color in `format`
/// (`formatColor`): `#RRGGBB`, with its alpha only if it has one, or
/// `rgb(r g b / a)`, `hsl(…)`, `hwb(…)`.
fn format_color(color: Color, format: &str) -> String {
    let [r, g, b, a] =
        [color.r, color.g, color.b, color.a].map(|channel| f64::from(channel) / 255.0);
    let alpha = if color.a == 255 {
        String::new()
    } else {
        format!(" / {}", (a * 100.0).round() / 100.0)
    };
    let percent = |value: f64| (value * 100.0).round();
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let hue = || {
        let delta = max - min;
        let hue = if delta == 0.0 {
            0.0
        } else if max == r {
            ((g - b) / delta).rem_euclid(6.0)
        } else if max == g {
            (b - r) / delta + 2.0
        } else {
            (r - g) / delta + 4.0
        };
        (hue * 60.0).round()
    };
    match format {
        "rgb" => format!("rgb({} {} {}{alpha})", color.r, color.g, color.b),
        "hsl" => {
            let lightness = (max + min) / 2.0;
            let saturation = if max == min {
                0.0
            } else {
                (max - min) / (1.0 - (2.0 * lightness - 1.0).abs())
            };
            format!(
                "hsl({}deg {} {}{alpha})",
                hue(),
                percent(saturation),
                percent(lightness)
            )
        }
        "hwb" => format!(
            "hwb({}deg {} {}{alpha})",
            hue(),
            percent(min),
            percent(1.0 - max)
        ),
        _ if color.a == 255 => format!("#{:02X}{:02X}{:02X}", color.r, color.g, color.b),
        _ => format!(
            "#{:02X}{:02X}{:02X}{:02X}",
            color.r, color.g, color.b, color.a
        ),
    }
}

/// `text`'s glyphs drawn from `origin` (its start, on the baseline) in
/// `color`, within `bounds`.
fn draw_text(
    list: &mut DisplayItemList,
    text: gfx::ShapedText,
    origin: PointF,
    color: Color,
    bounds: RectF,
) {
    for run in text.runs {
        list.push_op(PaintOp::DrawGlyphs {
            font: run.font,
            size: run.size,
            glyphs: run.glyphs.into(),
            positions: run
                .positions
                .iter()
                .map(|p| PointF::new(origin.x + p.x, origin.y + p.y))
                .collect(),
            rotations: Vec::new().into(),
            flags: PaintFlags::fill(color),
            embolden: run.embolden,
            skew: run.skew,
            variations: run.variations,
            bounds,
        });
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{Color, format_color, style_rows};

    /// A color as the overlay page writes it in each format
    /// (`inspector_overlay`'s `formatColor`).
    #[test]
    fn colors_are_written_as_the_overlay_writes_them() {
        let red = Color::rgba(255, 0, 0, 255);
        let half = Color::rgba(0, 0, 255, 128);
        let cases = [
            (red, "hex", "#FF0000"),
            (half, "hex", "#0000FF80"),
            (red, "rgb", "rgb(255 0 0)"),
            (half, "rgb", "rgb(0 0 255 / 0.5)"),
            (red, "hsl", "hsl(0deg 100 50)"),
            (Color::rgba(0, 128, 0, 255), "hsl", "hsl(120deg 100 25)"),
            (half, "hwb", "hwb(240deg 0 0 / 0.5)"),
            (Color::rgba(128, 128, 128, 255), "hwb", "hwb(0deg 50 50)"),
        ];
        for (color, format, written) in cases {
            assert_eq!(
                format_color(color, format),
                written,
                "{color:?} in {format}"
            );
        }
    }

    /// The label as the overlay page places it: above the element, its left
    /// edge at the element's, the arrow under it; below an element too near
    /// the top; kept in the viewport; the arrow in the middle of a narrow
    /// element; inside a large element with no arrow; none where it would
    /// overlap the element.
    #[test]
    fn labels_are_placed_as_chrome_places_them() {
        use super::{LabelPlace, place_label};
        use gfx::{PointF, RectF, SizeF};
        let viewport = SizeF::new(800.0, 600.0);
        let label = SizeF::new(100.0, 30.0);
        let at = |x, y, arrow| {
            Some(LabelPlace {
                origin: PointF::new(x, y),
                arrow,
            })
        };
        assert_eq!(
            place_label(RectF::new(100.0, 200.0, 150.0, 90.0), label, viewport),
            at(100.0, 162.0, Some((110.0, true)))
        );
        assert_eq!(
            place_label(RectF::new(100.0, 10.0, 150.0, 90.0), label, viewport),
            at(100.0, 108.0, Some((110.0, false)))
        );
        assert_eq!(
            place_label(RectF::new(750.0, 200.0, 40.0, 90.0), label, viewport),
            at(698.0, 162.0, Some((760.0, true)))
        );
        assert_eq!(
            place_label(RectF::new(0.0, 0.0, 800.0, 600.0), label, viewport),
            at(2.0, 570.0, None)
        );
        // Below a tall element, overlapping it: none.
        assert_eq!(
            place_label(RectF::new(0.0, 20.0, 100.0, 570.0), label, viewport),
            None
        );
        // A narrow element: the arrow in its middle.
        assert_eq!(
            place_label(RectF::new(300.0, 200.0, 20.0, 20.0), label, viewport),
            at(292.0, 162.0, Some((302.0, true)))
        );
    }

    /// The label's rows: color, font, background, margin, padding, in that
    /// order; those that say nothing (a transparent color, a font of no
    /// size, no margin or padding) left out.
    #[test]
    fn rows_are_those_that_say_something() {
        let style = json!({
            "color": "#FF0000FF",
            "font-family": "Ahem",
            "font-size": "12px",
            "padding": "3px",
            "margin": "1px 2px",
            "background-color": "#0000FF80",
        });
        let rows: Vec<(&str, String, Option<Color>)> = style_rows(&style, "hex");
        assert_eq!(
            rows,
            [
                (
                    "Color",
                    "#FF0000".to_owned(),
                    Some(Color::rgba(255, 0, 0, 255))
                ),
                ("Font", "12px Ahem".to_owned(), None),
                (
                    "Background",
                    "#0000FF80".to_owned(),
                    Some(Color::rgba(0, 0, 255, 128))
                ),
                ("Margin", "1px 2px".to_owned(), None),
                ("Padding", "3px".to_owned(), None),
            ]
        );
        let quiet = json!({
            "color": "#FF000000",
            "font-family": "Ahem",
            "font-size": "0px",
            "padding": "0px",
            "margin": "0px",
            "background-color": "#00000000",
        });
        assert!(style_rows(&quiet, "hex").is_empty());
    }
}
