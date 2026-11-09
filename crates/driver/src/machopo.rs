//! A session's machopo: `machopo --automation-pipe`, driven through its standard
//! input and output with `protocol::automation`'s messages, each followed
//! by a NUL. A thread reads what machopo sends; answers are matched to
//! commands by id, and events wait in order until a command looks for
//! them. Which tabs' renderers have gone is kept from the events as they
//! go by (`Target.targetCrashed`, until the tab navigates again).

use std::collections::{HashSet, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command as Process, Stdio};
use std::time::{Duration, Instant};

use base::SequenceNumber;
use crossbeam_channel::{Receiver, RecvTimeoutError, unbounded};
use protocol::automation::{self, Command, Event, EventKind, Message, Method};
use serde_json::Value;

use crate::Options;
use crate::error::{Failure, Result};

/// How long machopo has to end once asked to, before it is killed.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(5);

pub struct Machopo {
    child: Child,
    input: Option<ChildStdin>,
    messages: Receiver<Message>,
    /// Command ids, from 1 (`int`s, as CDP's are).
    ids: SequenceNumber<u32>,
    events: VecDeque<Event>,
    /// The targets whose renderer has gone.
    crashed: HashSet<String>,
}

impl Machopo {
    pub fn start(options: &Options) -> std::result::Result<Machopo, String> {
        let mut child = Process::new(&options.binary)
            .arg("--automation-pipe")
            .args(&options.binary_args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("starting {}: {e}", options.binary.display()))?;
        let input = child.stdin.take();
        let output = child.stdout.take().expect("piped");
        let (tx, messages) = unbounded();
        std::thread::Builder::new()
            .name("MachopoPipeReader".into())
            .spawn(move || {
                let mut output = BufReader::new(output);
                let mut bytes = Vec::new();
                loop {
                    bytes.clear();
                    match output.read_until(0, &mut bytes) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                    if bytes.last() == Some(&0) {
                        bytes.pop();
                    }
                    let text = String::from_utf8_lossy(&bytes);
                    match Message::from_json(&text) {
                        Some(message) => {
                            if tx.send(message).is_err() {
                                break;
                            }
                        }
                        None => {
                            tracing::warn!(%text, "a message from machopo the driver doesn't know")
                        }
                    }
                }
            })
            .map_err(|e| format!("starting the pipe's reader: {e}"))?;
        Ok(Machopo {
            child,
            input,
            messages,
            ids: SequenceNumber::starting_at(1),
            events: VecDeque::new(),
            crashed: HashSet::new(),
        })
    }

    /// Sends `method`, for the page session `session` or the browser's;
    /// its answer: the result, or the CDP error. Events that come first
    /// wait. `disconnected` if machopo has ended.
    pub fn call(
        &mut self,
        session: Option<&str>,
        method: Method,
    ) -> Result<std::result::Result<Value, automation::Error>> {
        Ok(self
            .call_until(session, method, None)?
            .expect("no deadline to pass"))
    }

    /// [`call`](Self::call), giving up at `deadline` (`None`: never):
    /// `None` then. An answer that comes later is dropped.
    pub fn call_until(
        &mut self,
        session: Option<&str>,
        method: Method,
        deadline: Option<Instant>,
    ) -> Result<Option<std::result::Result<Value, automation::Error>>> {
        let id = i32::try_from(self.ids.get_next()).expect("fewer than 2^31 commands");
        let command = Command {
            id,
            session_id: session.map(str::to_owned),
            method,
        };
        let mut bytes = command.to_json().into_bytes();
        bytes.push(0);
        let input = self.input.as_mut().ok_or_else(Failure::disconnected)?;
        input
            .write_all(&bytes)
            .and_then(|()| input.flush())
            .map_err(|_| Failure::disconnected())?;
        loop {
            let message = match deadline {
                Some(deadline) => self
                    .messages
                    .recv_timeout(deadline.saturating_duration_since(Instant::now())),
                None => self
                    .messages
                    .recv()
                    .map_err(|_| RecvTimeoutError::Disconnected),
            };
            match message {
                Ok(Message::Response(response)) if response.id == id => {
                    return Ok(Some(response.result));
                }
                Ok(Message::Response(response)) => {
                    tracing::warn!(id = response.id, "an answer to no command");
                }
                Ok(Message::Event(event)) => self.keep(event),
                // A page's, from V8's inspector: the driver asks for none.
                Ok(Message::Raw { .. }) => {}
                Err(RecvTimeoutError::Timeout) => return Ok(None),
                Err(RecvTimeoutError::Disconnected) => return Err(Failure::disconnected()),
            }
        }
    }

    /// [`call`](Self::call), a CDP error being an `unknown error`.
    pub fn call_ok(&mut self, session: Option<&str>, method: Method) -> Result<Value> {
        let name = method.name().to_owned();
        self.call(session, method)?.map_err(|e| {
            Failure::new(
                webdriver::error::ErrorStatus::UnknownError,
                format!("{name}: {e}"),
            )
        })
    }

    fn keep(&mut self, event: Event) {
        match &event.event {
            EventKind::TargetCrashed { target_id, .. } => {
                self.crashed.insert(target_id.clone());
            }
            // Navigating again launches another renderer.
            EventKind::FrameStartedNavigating { frame_id, .. } => {
                self.crashed.remove(frame_id);
            }
            _ => {}
        }
        self.events.push_back(event);
    }

    /// Whether `target`'s renderer has gone, as far as machopo has told.
    pub fn has_crashed(&mut self, target: &str) -> bool {
        // What has come so far.
        while let Ok(message) = self.messages.try_recv() {
            match message {
                Message::Event(event) => self.keep(event),
                Message::Response(response) => {
                    tracing::warn!(id = response.id, "an answer to no command");
                }
                Message::Raw { .. } => {}
            }
        }
        self.crashed.contains(target)
    }

    /// The first event come so far (the kept ones first) that `wanted`
    /// picks, without waiting for more; those looked at are dropped.
    pub fn already<T>(&mut self, mut wanted: impl FnMut(&Event) -> Option<T>) -> Option<T> {
        while let Ok(message) = self.messages.try_recv() {
            match message {
                Message::Event(event) => self.keep(event),
                Message::Response(response) => {
                    tracing::warn!(id = response.id, "an answer to no command");
                }
                Message::Raw { .. } => {}
            }
        }
        while let Some(event) = self.events.pop_front() {
            if let Some(found) = wanted(&event) {
                return Some(found);
            }
        }
        None
    }

    /// Drops the events kept so far: what a command waits for comes after.
    pub fn forget_events(&mut self) {
        self.events.clear();
    }

    /// Waits until `wanted` picks an event (the kept ones first), each one
    /// looked at being dropped; `None` if none did by `deadline` (`None`:
    /// no deadline). `disconnected` if machopo has ended.
    pub fn wait_for<T>(
        &mut self,
        deadline: Option<Instant>,
        mut wanted: impl FnMut(&Event) -> Option<T>,
    ) -> Result<Option<T>> {
        while let Some(event) = self.events.pop_front() {
            if let Some(found) = wanted(&event) {
                return Ok(Some(found));
            }
        }
        loop {
            let message = match deadline {
                Some(deadline) => self
                    .messages
                    .recv_timeout(deadline.saturating_duration_since(Instant::now())),
                None => self
                    .messages
                    .recv()
                    .map_err(|_| RecvTimeoutError::Disconnected),
            };
            match message {
                Ok(Message::Event(event)) => {
                    self.keep(event);
                    let event = self.events.pop_back().expect("just kept");
                    if let Some(found) = wanted(&event) {
                        return Ok(Some(found));
                    }
                }
                Ok(Message::Response(response)) => {
                    tracing::warn!(id = response.id, "an answer to no command");
                }
                Ok(Message::Raw { .. }) => {}
                Err(RecvTimeoutError::Timeout) => return Ok(None),
                Err(RecvTimeoutError::Disconnected) => return Err(Failure::disconnected()),
            }
        }
    }

    /// Asks machopo to end (`Browser.close`), and waits for it to; kills it if
    /// it doesn't in time.
    pub fn close(mut self) {
        let _ = self.call(None, Method::BrowserClose);
        drop(self.input.take());
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "now plus five seconds: within any clock's range"
        )]
        let deadline = Instant::now() + CLOSE_TIMEOUT;
        while Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        tracing::warn!("machopo didn't end in time: killing it");
    }
}

impl Drop for Machopo {
    fn drop(&mut self) {
        if let Ok(None) = self.child.try_wait() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}
