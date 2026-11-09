//! WebDriver in the renderer (`RendererMsg::WebDriver`, with
//! `RendererFlags::automation` only): what the renderer keeps for it (its
//! prefix of frame ids and element references, the scripts that haven't
//! settled) and the protocol's errors; `webdriver_commands` does each command's
//! work in the frame the driver names.

use std::collections::HashMap;
use std::hash::{BuildHasher, RandomState};

use compositor::DocumentId;
use protocol::renderer::RendererHostMsg;
use protocol::webdriver::{ErrorKind, WebDriverCommand, WebDriverError, WebDriverValue};

use super::RenderThread;

/// HTML's boolean attributes, as WebDriver's Get Element Attribute lists
/// them (Selenium's): "true" when the element has one, whatever its value.
pub(super) const BOOLEAN_ATTRIBUTES: &[&str] = &[
    "allowfullscreen",
    "allowpaymentrequest",
    "allowusermedia",
    "async",
    "autofocus",
    "autoplay",
    "checked",
    "compact",
    "complete",
    "controls",
    "declare",
    "default",
    "defaultchecked",
    "defaultselected",
    "defer",
    "disabled",
    "ended",
    "formnovalidate",
    "hidden",
    "indeterminate",
    "iscontenteditable",
    "ismap",
    "itemscope",
    "loop",
    "multiple",
    "muted",
    "nohref",
    "nomodule",
    "noresize",
    "noshade",
    "novalidate",
    "nowrap",
    "open",
    "paused",
    "playsinline",
    "pubdate",
    "readonly",
    "required",
    "reversed",
    "scoped",
    "seamless",
    "seeking",
    "selected",
    "truespeed",
    "typemustmatch",
    "willvalidate",
];

/// What the renderer keeps for WebDriver.
pub(super) struct WebDriverState {
    /// This renderer's, in each of its references and frame ids.
    pub(super) prefix: u64,
    /// The scripts that haven't settled, by request, with the document
    /// they run in.
    pub(super) scripts: HashMap<u64, DocumentId>,
    /// The documents references were made in, with the frame each was in:
    /// a reference to one of them that isn't its frame's document now is
    /// stale in that frame, and "no such element" in any other, as is any
    /// other.
    pub(super) documents: HashMap<DocumentId, dom_api::FrameId>,
}

/// A renderer's prefix of its child frames' ids: random enough that no two
/// renderers share one.
pub(super) fn new_frame_id_prefix() -> u64 {
    RandomState::new().hash_one(std::time::SystemTime::now())
}

impl WebDriverState {
    /// With the renderer's `prefix` (its frame ids' too).
    pub(super) fn new(prefix: u64) -> Self {
        Self {
            prefix,
            scripts: HashMap::new(),
            documents: HashMap::new(),
        }
    }
}

pub(super) fn error(kind: ErrorKind, message: impl Into<String>) -> WebDriverError {
    WebDriverError::new(kind, message)
}

pub(super) fn stale() -> WebDriverError {
    error(
        ErrorKind::StaleElementReference,
        "the element is no longer in its frame's document",
    )
}

impl RenderThread {
    /// `RendererMsg::WebDriver`, with `RendererFlags::automation` only: in
    /// the frame `frame` names, the top-level one if none.
    pub(super) fn webdriver(
        &mut self,
        request_id: u64,
        frame: Option<&str>,
        command: WebDriverCommand,
    ) {
        if !self.automation {
            tracing::warn!("WebDriver without RendererFlags::automation; ignored");
            return;
        }
        self.run_webdriver(request_id, frame, command);
    }

    pub(super) fn reply_webdriver(
        &self,
        request_id: u64,
        result: Result<WebDriverValue, WebDriverError>,
    ) {
        let _ = self
            .to_browser
            .send(RendererHostMsg::WebDriverReply { request_id, result });
    }
}
