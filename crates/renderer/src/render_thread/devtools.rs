//! DevTools' sessions of the page (Blink's `DevToolsAgent` and
//! `DevToolsSession`): the browser attaches one for each of its own, hands
//! it the commands it doesn't answer itself, and passes its answers and
//! events on. The renderer's `Page`, `Network` and `Log` agents answer what
//! they have (`page_agent`, `network_agent`, `log_agent`); the engine's
//! inspector answers the rest (V8's: `Runtime`, `Debugger`...), what it
//! hasn't that it isn't there.

use std::rc::Rc;
use std::sync::Arc;

use dom_api::{DevToolsQueued, EngineSettings, FrameId, ScriptEngine};
use protocol::renderer::RendererHostMsg;
use url::Url;

use super::RenderThread;
use super::page_agent::FrameInfo;
use crate::task_queue::DevToolsQueue;

/// JSON-RPC's "method not found", as CDP answers a method no one has.
const METHOD_NOT_FOUND: i64 = -32601;

impl RenderThread {
    /// What the browser put in the DevTools queue, in order: what script
    /// paused or interrupted didn't take already.
    pub(super) fn run_devtools_queue(&mut self) {
        while let Some(queued) = self.devtools_queue.try_next() {
            match queued {
                DevToolsQueued::Attach(session) => self.devtools_attach(session),
                DevToolsQueued::Detach(session) => self.devtools_detach(&session),
                DevToolsQueued::Message { session, message } => {
                    self.devtools_message(&session, &message);
                }
                // The main queue has the shutdown.
                DevToolsQueued::Quit => {}
            }
        }
    }

    pub(super) fn devtools_attach(&mut self, session: String) {
        if let Some(engine) = &mut self.engine {
            engine.devtools_attach(session.clone(), devtools_send(&self.to_browser, &session));
        }
        self.devtools_sessions.retain(|known| *known != session);
        self.devtools_sessions.push(session);
    }

    pub(super) fn devtools_detach(&mut self, session: &str) {
        if let Some(engine) = &mut self.engine {
            engine.devtools_detach(session);
        }
        self.page_agent.detach(session);
        self.network_agent.detach(session);
        self.log_agent.detach(session);
        self.dom_agent.detach(session);
        if let Some(engine) = &mut self.engine {
            engine
                .dom_mut()
                .set_dom_changes_wanted(self.dom_agent.any_enabled());
        }
        self.devtools_sessions.retain(|known| known != session);
        self.update_stacks_wanted();
    }

    /// Tells the `Page` agent the frames now, while there are sessions:
    /// what changed goes to those that turned `Page` on.
    pub(super) fn tell_frames(&mut self) {
        if !self.devtools_sessions.is_empty() {
            let frames = self.frame_infos();
            self.page_agent.update(frames);
            if self.dom_agent.any_enabled() {
                let frames = self.dom_frames();
                if let Some(engine) = &mut self.engine {
                    self.dom_agent.set_frames(engine.dom_mut(), frames);
                }
            }
        }
    }

    /// Tells the `DOM` agent's sessions of the DOM's changes since.
    pub(super) fn tell_dom_changes(&mut self) {
        if let Some(engine) = &mut self.engine {
            self.dom_agent.take_changes(engine.dom_mut());
        }
    }

    /// The frames, parents first, as DevTools' `Page.Frame`s tell them.
    /// Before the main frame's first document commits, the main frame
    /// with none, as Chrome's tab has its frame from the start (its
    /// initial empty document: no URL, an opaque origin).
    fn frame_infos(&mut self) -> Vec<FrameInfo> {
        if self.document.is_some() {
            return self.document_frame_infos();
        }
        let mut frames = Vec::new();
        if !self.devtools_main_frame_token.is_empty() {
            frames.push(FrameInfo {
                id: self.devtools_main_frame_token.clone(),
                parent_id: None,
                // No navigation's: theirs start at 1, so its first
                // document's is told as navigated.
                loader_id: "0".to_owned(),
                document: 0,
                name: None,
                initial: true,
                navigating: false,
                url: String::new(),
                origin: "://".to_owned(),
                mime_type: "text/html".to_owned(),
                secure_context: secure_context("", None),
                source: Rc::from(""),
                resources: Rc::default(),
                creation_stack: None,
            });
        }
        frames
    }

    /// Answers `message` with the agent whose it is, or hands it to the
    /// engine's inspector (made now if it isn't yet: the page's first
    /// document hasn't come), as a task of its own: script it runs
    /// (`Runtime.evaluate`) is followed by a microtask checkpoint, as any
    /// task's. One that none takes is answered that the method isn't
    /// there.
    pub(super) fn devtools_message(&mut self, session: &str, message: &str) {
        // The agents', with the frames as they are now.
        self.tell_frames();
        if self.page_agent.handle(session, message)
            || self.network_agent.handle(session, message)
            || self.log_agent.handle(session, message)
        {
            // The stacks they want from now on (`Page.enable`,
            // `Network.setAttachDebugStack`).
            self.update_stacks_wanted();
            return;
        }
        if self.engine.is_none() {
            self.engine = Some(self.make_engine());
        }
        if let Some(engine) = &mut self.engine
            && self
                .dom_agent
                .handle(session, message, &mut *engine.devtools_page())
        {
            return;
        }
        let handled = self
            .engine
            .as_mut()
            .is_some_and(|engine| engine.dispatch_devtools_message(session, message));
        if handled {
            // What its script left for the page (its results, warnings).
            self.send_harness_results();
            self.log_console_warnings();
            return;
        }
        let Some((id, method)) = command_of(message) else {
            return;
        };
        let answer = serde_json::json!({
            "id": id,
            "error": {
                "code": METHOD_NOT_FOUND,
                "message": format!("'{method}' wasn't found"),
            },
        });
        let _ = self.to_browser.send(RendererHostMsg::DevToolsMessage {
            session: session.to_owned(),
            message: answer.to_string(),
        });
    }
}

impl RenderThread {
    /// The page's engine, as its first document (or DevTools' first
    /// command for its inspector) comes: its `console` printed
    /// (`print_console`), its inspector taking from the DevTools queue
    /// while script is paused or busy, and attached to the browser's
    /// sessions, its frames named as DevTools names them.
    pub(super) fn make_engine(&self) -> Box<dyn ScriptEngine> {
        let mut engine = (self.script_engine.engine)(EngineSettings {
            web_tests: self.web_tests,
        });
        // `fetch()` and blob URLs at the call, on the thread's handles.
        engine.set_load_sink(Box::new(super::documents::RendererLoadSink {
            to_browser: self.to_browser.clone(),
            ids: self.next_request_id.clone(),
            blob_urls: self.blob_urls.clone(),
        }));
        // What every frame's `navigator` answers, a new iframe's too.
        engine
            .dom_mut()
            .set_agent(&self.user_agent, self.automation);
        engine.set_console_sink(Box::new(|message| {
            let kind = if message.uncaught {
                "uncaught"
            } else {
                "console"
            };
            super::print_console(kind, &message.text, &message.url, message.line)
        }));
        let interrupter = engine.set_devtools_queue(Box::new(DevToolsQueueHandle {
            queue: self.devtools_queue.clone(),
            page_agent: self.page_agent.clone(),
            network_agent: self.network_agent.clone(),
            log_agent: self.log_agent.clone(),
            dom_agent: self.dom_agent.clone(),
            to_browser: self.to_browser.clone(),
        }));
        self.devtools_queue.set_interrupter(interrupter);
        engine.set_devtools_frame_ids(self.devtools_frame_ids(engine.top_frame()));
        for session in &self.devtools_sessions {
            engine.devtools_attach(session.clone(), devtools_send(&self.to_browser, session));
        }
        engine.set_stacks_wanted(
            self.network_agent.wants_stacks(),
            self.page_agent.any_enabled(),
        );
        engine
    }

    /// What DevTools calls the engine's frames: the top-level one the
    /// browser's main frame token (none until told), a child one as
    /// WebDriver does (`frame_id`).
    pub(super) fn devtools_frame_ids(&self, top: FrameId) -> dom_api::DevToolsFrameIds {
        let main = self.devtools_main_frame_token.clone();
        let prefix = self.webdriver_state.prefix;
        Box::new(move |frame| {
            if frame == top {
                (!main.is_empty()).then(|| main.clone())
            } else {
                Some(super::webdriver_commands::child_frame_id(prefix, frame))
            }
        })
    }

    /// The loop waits for its next task (`idle`), or has one: the engine's
    /// inspector tells the two apart in a profile.
    pub(super) fn set_idle(&mut self, idle: bool) {
        if let Some(engine) = &mut self.engine {
            engine.set_idle(idle);
        }
    }

    /// The warnings the engine's script earned since (Blink's
    /// `ExecutionContext::AddConsoleMessage`): the page's log has them.
    pub(super) fn log_console_warnings(&mut self) {
        let Some(engine) = &mut self.engine else {
            return;
        };
        for text in engine.dom_mut().take_console_warnings() {
            self.log_agent
                .add(super::log_agent::LogEntry::javascript_warning(text));
        }
    }
}

/// Where a session's answers and events from the engine's inspector go:
/// to the browser, as the session's.
fn devtools_send(
    to_browser: &crossbeam_channel::Sender<RendererHostMsg>,
    session: &str,
) -> dom_api::DevToolsSend {
    let to_browser = to_browser.clone();
    let session = session.to_owned();
    Box::new(move |message| {
        let _ = to_browser.send(RendererHostMsg::DevToolsMessage {
            session: session.clone(),
            message,
        });
    })
}

/// The DevTools queue and the `Page`, `Network` and `Log` agents, as the
/// engine's inspector takes from them while script is paused or busy. The
/// agents are the render thread's too, shared (`Rc`): the inspector answers
/// with them while the render thread is inside a call of the engine's,
/// which it can't be borrowed in.
struct DevToolsQueueHandle {
    queue: Arc<DevToolsQueue>,
    page_agent: Rc<super::page_agent::PageAgent>,
    network_agent: Rc<super::network_agent::NetworkAgent>,
    log_agent: Rc<super::log_agent::LogAgent>,
    dom_agent: Rc<super::dom_agent::DomAgent>,
    // PROTOTYPE: for `Prototype.pausedPage`'s answer.
    to_browser: crossbeam_channel::Sender<RendererHostMsg>,
}

impl dom_api::DevToolsQueue for DevToolsQueueHandle {
    fn next(&self) -> DevToolsQueued {
        self.queue.wait_next()
    }

    fn take_interrupting(&self) -> Vec<(String, String)> {
        self.queue.take_interrupting()
    }

    fn put_back(&self, queued: Vec<DevToolsQueued>) {
        self.queue.put_back(queued);
    }

    fn answer_here(
        &self,
        session: &str,
        message: &str,
        page: &mut dyn dom_api::DevToolsPage,
    ) -> bool {
        if let Some((id, method)) = command_of(message)
            && method == "Prototype.pausedPage"
        {
            let result = paused_page_prototype(page);
            let _ = self.to_browser.send(RendererHostMsg::DevToolsMessage {
                session: session.to_owned(),
                message: serde_json::json!({ "id": id, "result": result }).to_string(),
            });
            return true;
        }
        self.page_agent.handle(session, message)
            || self.network_agent.handle(session, message)
            || self.log_agent.handle(session, message)
            || self.dom_agent.handle(session, message, page)
    }

    fn paused(&self, page: &mut dyn dom_api::DevToolsPage) {
        self.dom_agent.take_changes(page.dom());
    }
}

// PROTOTYPE: what DevTools' agents would answer from while script is
// paused: the top-level document's elements, counted, and its root's box,
// style and layout brought up to date first.
fn paused_page_prototype(page: &mut dyn dom_api::DevToolsPage) -> serde_json::Value {
    use dom_api::FrameLayout;
    let (dom, layout) = page.dom_and_layout();
    let Some(layout) = layout else {
        return serde_json::json!({ "layout": false });
    };
    dom.read(|cx| {
        layout.update_style_and_layout(cx);
        let Some(frame) = layout
            .as_any_mut()
            .downcast_mut::<crate::frame_view::Frame>()
        else {
            return serde_json::json!({ "layout": "not a frame" });
        };
        let document = frame.view.document().get(cx.gc);
        let mut elements = 0;
        let mut root = None;
        let mut stack = vec![document];
        while let Some(node) = stack.pop() {
            if cx.kind(node) == dom::NodeKind::Element {
                elements = usize::saturating_add(elements, 1);
                if root.is_none() {
                    root = Some(node);
                }
            }
            let mut child = cx.first_child(node);
            while let Some(next) = child {
                stack.push(next);
                child = cx.next_sibling(next);
            }
        }
        let height = root
            .and_then(|root| frame.element_box(cx.node_id(root)))
            .map(|element| element.border_box.height);
        serde_json::json!({ "layout": true, "elements": elements, "rootHeight": height })
    })
}

/// The `id` and `method` of a command, as JSON.
fn command_of(message: &str) -> Option<(serde_json::Value, String)> {
    let command: serde_json::Value = serde_json::from_str(message).ok()?;
    let id = command.get("id")?.clone();
    let method = command.get("method")?.as_str()?.to_owned();
    Some((id, method))
}

/// `Page.SecureContextType` of a document at `url`, in a frame whose parent
/// is `parent`'s (Blink's `SecureContextMode`): an insecure parent's
/// children aren't; `about:` and `data:` URLs (potentially trustworthy, per
/// Secure Contexts) are as their parent (or, alone, secure); HTTPS and
/// files are, and HTTP only on the machine itself.
pub(super) fn secure_context(url: &str, parent: Option<&'static str>) -> &'static str {
    if let Some(parent) = parent
        && parent != "Secure"
        && parent != "SecureLocalhost"
    {
        return "InsecureAncestor";
    }
    let Ok(url) = Url::parse(url) else {
        return "InsecureScheme";
    };
    match url.scheme() {
        "about" | "data" => parent.unwrap_or("Secure"),
        "https" | "wss" | "file" => "Secure",
        "http" | "ws" => match url.host() {
            Some(url::Host::Domain(host))
                if host == "localhost" || host.ends_with(".localhost") =>
            {
                "SecureLocalhost"
            }
            Some(url::Host::Ipv4(ip)) if ip.is_loopback() => "SecureLocalhost",
            Some(url::Host::Ipv6(ip)) if ip.is_loopback() => "SecureLocalhost",
            _ => "InsecureScheme",
        },
        _ => "InsecureScheme",
    }
}
