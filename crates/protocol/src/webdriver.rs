//! WebDriver's work in a renderer: running script, its values
//! as JSON, element references, finding elements and reading them. The
//! browser hands it over as `RendererMsg::WebDriver` and gets
//! `RendererHostMsg::WebDriverReply` back. Chromium's chromedriver does
//! this work in JS it injects into the page (`call_function.js`, through
//! `Runtime.callFunctionOn`); machopo's renderer does it natively, as
//! Ladybird's WebContent does.
//!
//! Values cross in JSON's shape, with what WebDriver's JSON has references
//! for kept apart: an element as its reference's id, and a child frame's
//! window as its frame's, which only the renderer reads; the tab's window
//! as just that, since the renderer doesn't know its tab, and the browser,
//! which does, gives its handle.
//!
//! The work is done in a frame of the tab: the top-level one, or a child
//! frame by its id, which the renderer gives (`SwitchToFrame`) and which
//! stays the frame's as it navigates (Chromium's frame token).

use base::numerics::saturated_cast;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The key of a web element reference (WebDriver's "web element
/// identifier").
pub const ELEMENT_KEY: &str = "element-6066-11e4-a52e-4f735466cecf";
/// The key of a shadow root reference.
pub const SHADOW_ROOT_KEY: &str = "shadow-6066-11e4-a52e-4f735466cecf";
/// The key of a web window reference.
pub const WINDOW_KEY: &str = "window-fcc6-11e5-b4f8-330a88ab9d7f";
/// The key of a web frame reference.
pub const FRAME_KEY: &str = "frame-075b-4da1-b6ba-e579c2d3230a";

/// A value in JSON's shape, with references apart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WebDriverValue {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<WebDriverValue>),
    /// An object's properties, in order.
    Object(Vec<(String, WebDriverValue)>),
    /// A web element reference: its id.
    Element(String),
    /// The tab's top-level window.
    Window,
    /// A child frame's window (a web frame reference): its frame's id.
    Frame(String),
}

/// What the renderer does for WebDriver, in a frame of the tab.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WebDriverCommand {
    /// Execute Script (`is_async`: Execute Async Script): `body` as a
    /// function body, called with `args`; its result once it settles.
    ExecuteScript {
        body: String,
        args: Vec<WebDriverValue>,
        is_async: bool,
    },
    /// Find Element(s): the elements `value` picks, in tree order, among
    /// the document's descendants or `from`'s.
    FindElements {
        using: Locator,
        value: String,
        from: Option<String>,
    },
    /// Get Element Rect: `{x, y, width, height}`.
    ElementRect { element: String },
    /// Get Element Tag Name.
    ElementTagName { element: String },
    /// Get Element Attribute: a string, or null.
    ElementAttribute { element: String, name: String },
    /// Get Element Property.
    ElementProperty { element: String, name: String },
    /// Switch To Frame: the id of the child frame `window[index]` is, or of
    /// the iframe `element`'s; with neither, of the frame itself (`Null`
    /// for the top-level one), for checking it is still there.
    SwitchToFrame {
        index: Option<u16>,
        element: Option<String>,
    },
    /// Element Click's work before the click: scrolls the element into
    /// view; `{x, y}`, its in-view center point in the top-level
    /// viewport's CSS px, where the pointer goes. With
    /// `in_view_center_only`, Perform Actions' element origin: WebDriver's
    /// in-view center point, in the CSS px of the viewport of the
    /// element's frame (outside it if the element is), the page not
    /// scrolled and what else would get a click not checked; "move target
    /// out of bounds" if the element has no box.
    ElementClickPoint {
        element: String,
        in_view_center_only: bool,
    },
    /// Take Element Screenshot's: scrolls the element into view;
    /// `{x, y, width, height}`, the part of it in the top-level viewport,
    /// in its CSS px.
    ElementScreenshotRect { element: String },
    /// Perform Actions' viewport: `{x, y, width, height}`, the frame's
    /// viewport, where it is in the top-level one (CSS px) and its size.
    FrameViewport,
    /// Element Send Keys' work in the page: the element scrolled into view,
    /// "element not interactable" unless it is keyboard-interactable; the
    /// focus taken if it hasn't it (a text control's cursor then after its
    /// value). `true` if the driver types `text`'s keys; `false` if the
    /// page took it (a non-typeable control's value set to it); "unsupported
    /// operation" for a file input (machopo has no files).
    ElementSendKeys { element: String, text: String },
    /// Element Clear: "invalid element state" unless the element is
    /// editable; scrolled into view, "element not interactable" unless its
    /// box is in the viewport then; emptied as WebDriver's "clear a
    /// resettable element" says (focus, `input`, `change`, blur).
    ElementClear { element: String },
}

/// A location strategy machopo has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Locator {
    /// `css selector`.
    Css,
    /// `tag name`: as `getElementsByTagName`.
    TagName,
}

impl Locator {
    /// The strategy WebDriver names `using`: `None` for one it has that machopo
    /// doesn't (`link text`, `partial link text`, `xpath`), an error for
    /// one it doesn't have.
    pub fn from_name(using: &str) -> Result<Option<Locator>, WebDriverError> {
        match using {
            "css selector" => Ok(Some(Locator::Css)),
            "tag name" => Ok(Some(Locator::TagName)),
            "link text" | "partial link text" | "xpath" => Ok(None),
            other => Err(WebDriverError::new(
                ErrorKind::InvalidArgument,
                format!("no location strategy {other:?}"),
            )),
        }
    }
}

/// A WebDriver error, as the renderer or the browser answers it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebDriverError {
    pub kind: ErrorKind,
    pub message: String,
}

impl WebDriverError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

/// The WebDriver errors machopo's work answers with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorKind {
    InvalidArgument,
    InvalidSelector,
    ElementClickIntercepted,
    ElementNotInteractable,
    JavascriptError,
    NoSuchElement,
    NoSuchFrame,
    NoSuchShadowRoot,
    NoSuchWindow,
    StaleElementReference,
    UnableToCaptureScreen,
    UnsupportedOperation,
    MoveTargetOutOfBounds,
    InvalidElementState,
}

impl ErrorKind {
    /// WebDriver's error code.
    pub fn code(self) -> &'static str {
        match self {
            ErrorKind::InvalidArgument => "invalid argument",
            ErrorKind::InvalidSelector => "invalid selector",
            ErrorKind::ElementClickIntercepted => "element click intercepted",
            ErrorKind::ElementNotInteractable => "element not interactable",
            ErrorKind::JavascriptError => "javascript error",
            ErrorKind::NoSuchElement => "no such element",
            ErrorKind::NoSuchFrame => "no such frame",
            ErrorKind::NoSuchShadowRoot => "no such shadow root",
            ErrorKind::NoSuchWindow => "no such window",
            ErrorKind::StaleElementReference => "stale element reference",
            ErrorKind::UnableToCaptureScreen => "unable to capture screen",
            ErrorKind::UnsupportedOperation => "unsupported operation",
            ErrorKind::MoveTargetOutOfBounds => "move target out of bounds",
            ErrorKind::InvalidElementState => "invalid element state",
        }
    }

    /// The kind with WebDriver's error `code`.
    pub fn from_code(code: &str) -> Option<ErrorKind> {
        [
            ErrorKind::InvalidArgument,
            ErrorKind::InvalidSelector,
            ErrorKind::ElementClickIntercepted,
            ErrorKind::ElementNotInteractable,
            ErrorKind::JavascriptError,
            ErrorKind::NoSuchElement,
            ErrorKind::NoSuchFrame,
            ErrorKind::NoSuchShadowRoot,
            ErrorKind::NoSuchWindow,
            ErrorKind::StaleElementReference,
            ErrorKind::UnableToCaptureScreen,
            ErrorKind::UnsupportedOperation,
            ErrorKind::MoveTargetOutOfBounds,
            ErrorKind::InvalidElementState,
        ]
        .into_iter()
        .find(|kind| kind.code() == code)
    }
}

/// The one key of a reference object, and its id.
fn reference(object: &Map<String, Value>) -> Option<(&str, &Value)> {
    [ELEMENT_KEY, SHADOW_ROOT_KEY, WINDOW_KEY, FRAME_KEY]
        .into_iter()
        .find_map(|key| object.get(key).map(|id| (key, id)))
}

impl WebDriverValue {
    /// As WebDriver's JSON, the tab's window being `window` (its handle).
    pub fn to_json(&self, window: &str) -> Value {
        match self {
            WebDriverValue::Null => Value::Null,
            WebDriverValue::Bool(b) => Value::Bool(*b),
            // A number JSON can't have (NaN, ±Infinity) is null, as
            // `JSON.stringify` makes it.
            WebDriverValue::Number(n) => {
                if n.fract() == 0.0 && n.abs() < 9_007_199_254_740_992.0 {
                    // Integral: an integer, as JSON from JS prints it
                    // (exactly: an `i64` holds every integer under 2^53).
                    Value::from(saturated_cast::<i64, _>(*n))
                } else {
                    serde_json::Number::from_f64(*n).map_or(Value::Null, Value::Number)
                }
            }
            WebDriverValue::String(s) => Value::String(s.clone()),
            WebDriverValue::Array(items) => {
                Value::Array(items.iter().map(|item| item.to_json(window)).collect())
            }
            WebDriverValue::Object(properties) => Value::Object(
                properties
                    .iter()
                    .map(|(name, value)| (name.clone(), value.to_json(window)))
                    .collect(),
            ),
            WebDriverValue::Element(id) => {
                let mut object = Map::new();
                object.insert(ELEMENT_KEY.into(), Value::String(id.clone()));
                Value::Object(object)
            }
            WebDriverValue::Window => {
                let mut object = Map::new();
                object.insert(WINDOW_KEY.into(), Value::String(window.to_owned()));
                Value::Object(object)
            }
            WebDriverValue::Frame(id) => {
                let mut object = Map::new();
                object.insert(FRAME_KEY.into(), Value::String(id.clone()));
                Value::Object(object)
            }
        }
    }

    /// WebDriver's "JSON deserialize" up to the references: `json` with an
    /// element's and a frame's kept as their ids, for the renderer to find,
    /// and the tab's window (handle `window`) as itself. Another window's
    /// handle is "no such window" (its window is another renderer's); a
    /// reference whose id isn't a string, "invalid argument". Machopo has no
    /// shadow roots.
    pub fn from_json(json: &Value, window: &str) -> Result<WebDriverValue, WebDriverError> {
        Ok(match json {
            Value::Null => WebDriverValue::Null,
            Value::Bool(b) => WebDriverValue::Bool(*b),
            Value::Number(n) => WebDriverValue::Number(n.as_f64().unwrap_or(f64::NAN)),
            Value::String(s) => WebDriverValue::String(s.clone()),
            Value::Array(items) => WebDriverValue::Array(
                items
                    .iter()
                    .map(|item| Self::from_json(item, window))
                    .collect::<Result<_, _>>()?,
            ),
            Value::Object(object) => match reference(object) {
                Some((key, id)) => {
                    let Value::String(id) = id else {
                        return Err(WebDriverError::new(
                            ErrorKind::InvalidArgument,
                            format!("a reference's id is a string, not {id}"),
                        ));
                    };
                    match key {
                        ELEMENT_KEY => WebDriverValue::Element(id.clone()),
                        WINDOW_KEY if id == window => WebDriverValue::Window,
                        WINDOW_KEY => {
                            return Err(WebDriverError::new(
                                ErrorKind::NoSuchWindow,
                                format!("no window {id:?}"),
                            ));
                        }
                        FRAME_KEY => WebDriverValue::Frame(id.clone()),
                        _ => {
                            return Err(WebDriverError::new(
                                ErrorKind::NoSuchShadowRoot,
                                format!("no shadow root {id:?}"),
                            ));
                        }
                    }
                }
                None => WebDriverValue::Object(
                    object
                        .iter()
                        .map(|(name, value)| Ok((name.clone(), Self::from_json(value, window)?)))
                        .collect::<Result<_, WebDriverError>>()?,
                ),
            },
        })
    }
}
