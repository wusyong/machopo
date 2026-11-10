//! DevTools' `Log` domain as the renderer answers it (Blink's
//! `InspectorLogAgent` over the page's `ConsoleMessageStorage`): the
//! messages the page's loads give (a response of 400 or more, a load that
//! failed) and the warnings script's calls earn (`javascript_warning`),
//! kept whether or not DevTools is there, the newest 1000, and
//! sent to each session that turned `Log` on, the kept ones first. Each is
//! printed to stderr as the console's messages are, as Chrome prints them
//! with `--enable-logging=stderr`. A main frame's new document starts
//! afresh.
//!
//! A session also hears, on its own, of what it asked of `Network` that machopo
//! can't do: Chrome has no such warnings, but machopo answers those commands
//! as errors DevTools' front end doesn't show.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;

use crossbeam_channel::Sender;
use protocol::devtools_network::wall_time;
use protocol::renderer::RendererHostMsg;
use serde_json::{Value, json};

/// The most messages kept (Blink's `kMaxConsoleMessageCount`).
const MAX_ENTRIES: usize = 1000;

/// A message, as DevTools' `Log.LogEntry` tells it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LogEntry {
    /// `Log.LogEntry`'s `source`: `network`, `other`...
    pub(crate) source: &'static str,
    /// `verbose`, `info`, `warning` or `error`.
    pub(crate) level: &'static str,
    pub(crate) text: String,
    /// The resource it is about; empty for none.
    pub(crate) url: String,
    /// The request it is about, as DevTools' `Network` calls it.
    pub(crate) network_request_id: Option<String>,
    /// Milliseconds since the Unix epoch (`Runtime.Timestamp`).
    pub(crate) timestamp: f64,
}

impl LogEntry {
    /// A load's message (`source` `network`, `error`), now.
    pub(crate) fn network(text: String, url: String, network_request_id: String) -> Self {
        Self {
            source: "network",
            level: "error",
            text,
            url,
            network_request_id: Some(network_request_id),
            timestamp: wall_time() * 1000.0,
        }
    }

    /// A warning about what script asked (`source` `javascript`,
    /// `warning`), now.
    pub(crate) fn javascript_warning(text: String) -> Self {
        Self {
            source: "javascript",
            level: "warning",
            text,
            url: String::new(),
            network_request_id: None,
            timestamp: wall_time() * 1000.0,
        }
    }

    /// A warning that raster panicked drawing `what` (a tile, a canvas)
    /// of the page at `url`, which is left without it (`source`
    /// `rendering`, `warning`), now: Blink's console messages of
    /// `kRendering`, as of a context that was lost.
    pub(crate) fn raster_failed(
        what: RasterFailed,
        failure: &raster::RasterFailure,
        url: &str,
    ) -> Self {
        let raster::RasterFailure { message, count } = failure;
        let left = match what {
            RasterFailed::Tile => "A part of the page was left blank",
            RasterFailed::Canvas => "What a canvas drew was left out",
        };
        Self {
            source: "rendering",
            level: "warning",
            text: format!("{left}: drawing it failed ({message}). It has happened {count} times."),
            url: url.to_owned(),
            network_request_id: None,
            timestamp: wall_time() * 1000.0,
        }
    }

    fn to_json(&self) -> Value {
        let mut entry = json!({
            "source": self.source,
            "level": self.level,
            "text": self.text,
            "timestamp": self.timestamp,
        });
        if !self.url.is_empty() {
            entry["url"] = self.url.as_str().into();
        }
        if let Some(id) = &self.network_request_id {
            entry["networkRequestId"] = id.as_str().into();
        }
        entry
    }
}

/// What raster failed to draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RasterFailed {
    /// A tile of a layer, on the compositor's side.
    Tile,
    /// What a canvas's 2D context recorded.
    Canvas,
}

/// Blink's `InspectorLogAgent`, for every session of the page, and the
/// page's messages.
pub(crate) struct LogAgent {
    to_browser: Sender<RendererHostMsg>,
    /// The sessions that turned `Log` on.
    enabled: RefCell<Vec<String>>,
    /// The newest messages, oldest first.
    entries: RefCell<VecDeque<LogEntry>>,
    /// How many older ones were dropped since the last clear (Blink's
    /// `ConsoleMessageStorage::ExpiredCount`).
    expired: Cell<usize>,
}

impl LogAgent {
    pub(crate) fn new(to_browser: Sender<RendererHostMsg>) -> Self {
        Self {
            to_browser,
            enabled: RefCell::new(Vec::new()),
            entries: RefCell::new(VecDeque::new()),
            expired: Cell::new(0),
        }
    }

    fn send(&self, session: &str, message: &Value) {
        let _ = self.to_browser.send(RendererHostMsg::DevToolsMessage {
            session: session.to_owned(),
            message: message.to_string(),
        });
    }

    fn tell(&self, session: &str, entry: &Value) {
        self.send(
            session,
            &json!({ "method": "Log.entryAdded", "params": { "entry": entry } }),
        );
    }

    /// A message of the page's: printed, kept, and told to every session
    /// that turned `Log` on.
    pub(crate) fn add(&self, entry: LogEntry) {
        if entry.url.is_empty() {
            eprintln!("[console] {}", entry.text);
        } else {
            eprintln!("[console] {} ({})", entry.text, entry.url);
        }
        let json = entry.to_json();
        {
            let mut entries = self.entries.borrow_mut();
            if entries.len() >= MAX_ENTRIES {
                entries.pop_front();
                self.expired.set(self.expired.get().saturating_add(1));
            }
            entries.push_back(entry);
        }
        for session in self.enabled.borrow().iter() {
            self.tell(session, &json);
        }
    }

    /// Warns `session`, if it turned `Log` on, of what it asked that machopo
    /// can't do (`text`); not kept, not printed: it is that session's.
    pub(crate) fn warn(&self, session: &str, text: String) {
        if self.enabled.borrow().iter().any(|on| on == session) {
            let entry = LogEntry {
                source: "other",
                level: "warning",
                text,
                url: String::new(),
                network_request_id: None,
                timestamp: wall_time() * 1000.0,
            };
            self.tell(session, &entry.to_json());
        }
    }

    /// The main frame has a new document: the messages kept go (Blink's
    /// `Page::DidCommitLoad` clears the storage).
    pub(crate) fn main_frame_navigated(&self) {
        self.clear();
    }

    fn clear(&self) {
        self.entries.borrow_mut().clear();
        self.expired.set(0);
    }

    /// `session` has gone.
    pub(crate) fn detach(&self, session: &str) {
        self.enabled.borrow_mut().retain(|on| on != session);
    }

    /// Answers `message`, a command of `session`'s, if it is one of the
    /// agent's: whether it was.
    pub(crate) fn handle(&self, session: &str, message: &str) -> bool {
        let Ok(command) = serde_json::from_str::<Value>(message) else {
            return false;
        };
        let Some(method) = command.get("method").and_then(Value::as_str) else {
            return false;
        };
        match method {
            "Log.enable"
            | "Log.disable"
            | "Log.clear"
            | "Log.startViolationsReport"
            | "Log.stopViolationsReport" => {}
            _ => return false,
        }
        match method {
            "Log.enable" => {
                let already = self.enabled.borrow().iter().any(|on| on == session);
                if !already {
                    self.enabled.borrow_mut().push(session.to_owned());
                    self.replay(session);
                }
            }
            "Log.disable" => self.detach(session),
            "Log.clear" => self.clear(),
            // machopo reports no violations (long tasks, forced layouts...).
            _ => {}
        }
        // After the kept messages, as Blink's agent sends them while
        // enabling.
        let id = command.get("id").cloned().unwrap_or(Value::Null);
        self.send(session, &json!({ "id": id, "result": {} }));
        true
    }

    /// The messages kept, to `session`, which just turned `Log` on: first
    /// how many were dropped, if any (as Blink's agent does).
    fn replay(&self, session: &str) {
        let expired = self.expired.get();
        if expired > 0 {
            let text = if expired == 1 {
                "1 log entry is not shown.".to_owned()
            } else {
                format!("{expired} log entries are not shown.")
            };
            self.tell(
                session,
                &json!({ "source": "other", "level": "warning", "text": text, "timestamp": 0 }),
            );
        }
        for entry in self.entries.borrow().iter() {
            self.tell(session, &entry.to_json());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(text: &str) -> LogEntry {
        LogEntry::network(text.to_owned(), String::new(), "0.1".to_owned())
    }

    /// What `session` was sent: the entries' texts, then `answered` for
    /// the answer.
    fn sent(from: &crossbeam_channel::Receiver<RendererHostMsg>) -> Vec<String> {
        from.try_iter()
            .map(|msg| match msg {
                RendererHostMsg::DevToolsMessage { message, .. } => {
                    let message: Value = serde_json::from_str(&message).unwrap();
                    match message["params"]["entry"]["text"].as_str() {
                        Some(text) => text.to_owned(),
                        None => "answered".to_owned(),
                    }
                }
                other => panic!("{other:?}"),
            })
            .collect()
    }

    fn command(method: &str) -> String {
        json!({ "id": 1, "method": method }).to_string()
    }

    /// A raster failure is a warning of the page's, of `rendering`, with
    /// the page's URL, what was left out and how many times.
    #[test]
    fn a_raster_failure_is_a_rendering_warning() {
        let failure = raster::RasterFailure {
            message: "attempt to subtract with overflow".into(),
            count: 4,
        };
        let tile = LogEntry::raster_failed(RasterFailed::Tile, &failure, "https://a.test/");
        assert_eq!(
            (tile.source, tile.level, tile.url.as_str()),
            ("rendering", "warning", "https://a.test/")
        );
        assert_eq!(
            tile.text,
            "A part of the page was left blank: drawing it failed \
             (attempt to subtract with overflow). It has happened 4 times."
        );
        assert_eq!(tile.network_request_id, None);
        let canvas = LogEntry::raster_failed(RasterFailed::Canvas, &failure, "");
        assert_eq!(
            canvas.text,
            "What a canvas drew was left out: drawing it failed \
             (attempt to subtract with overflow). It has happened 4 times."
        );
        // Told as any entry is.
        let (to_browser, from) = crossbeam_channel::unbounded();
        let agent = LogAgent::new(to_browser);
        assert!(agent.handle("s", &command("Log.enable")));
        sent(&from);
        agent.add(tile.clone());
        assert_eq!(sent(&from), [tile.text]);
    }

    /// The newest 1000 are kept, told before `Log.enable`'s answer after
    /// how many were dropped; `Log.clear` drops them all, and the count.
    #[test]
    fn the_newest_are_kept_and_replayed() {
        let (to_browser, from) = crossbeam_channel::unbounded();
        let agent = LogAgent::new(to_browser);
        for n in 0..1002 {
            agent.add(entry(&n.to_string()));
        }
        assert!(agent.handle("s", &command("Log.enable")));
        let replayed = sent(&from);
        assert_eq!(replayed.len(), 1002, "the count, 1000, the answer");
        assert_eq!(replayed[0], "2 log entries are not shown.");
        assert_eq!(replayed[1], "2");
        assert_eq!(replayed[1000], "1001");
        assert_eq!(replayed[1001], "answered");
        // Told as they come, while on; not once off.
        agent.add(entry("live"));
        assert_eq!(sent(&from), ["live"]);
        assert!(agent.handle("s", &command("Log.disable")));
        agent.add(entry("off"));
        assert_eq!(sent(&from), ["answered"]);

        assert!(agent.handle("s", &command("Log.clear")));
        agent.add(entry("after"));
        assert!(agent.handle("s", &command("Log.enable")));
        assert_eq!(sent(&from), ["answered", "after", "answered"]);
        // One dropped is told so.
        agent.main_frame_navigated();
        for n in 0..1001 {
            agent.add(entry(&n.to_string()));
        }
        assert!(agent.handle("t", &command("Log.enable")));
        let told = sent(&from);
        // The live ones `s` still hears of, then `t`'s.
        assert_eq!(told[1001], "1 log entry is not shown.");
    }
}
