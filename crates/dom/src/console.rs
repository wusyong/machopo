//! The messages for the page's console the DOM's operations leave (Blink's
//! `ExecutionContext::AddConsoleMessage`): the host logs them after each
//! task (`Dom::take_console_notes`). One said where the script running is
//! (Blink's `ConsoleMessage` captures its `SourceLocation`) has that
//! script's URL and line once the operation is over: the engine places it
//! (`Dom::place_console_notes`), as V8 allocates and an operation may not.

use crate::{Cx, Dom, Microtasks};

/// Where a message comes from (DevTools' `Log.LogEntry`'s `source`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleSource {
    JavaScript,
    Security,
    Other,
}

/// How grave it is (`Log.LogEntry`'s `level`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleLevel {
    Warning,
    Error,
}

/// A message for the page's console.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsoleNote {
    pub source: ConsoleSource,
    pub level: ConsoleLevel,
    pub text: String,
    /// The resource it is of: the script's or the document's URL; empty
    /// for none.
    pub url: String,
    /// The line of `url` (0-based, as DevTools tells it), if it says one.
    pub line: Option<u32>,
}

/// A message, and whether the engine is yet to place it at the script
/// running.
#[derive(Default)]
pub(crate) struct ConsoleNotes {
    notes: std::cell::RefCell<Vec<(ConsoleNote, bool)>>,
}

impl<'gc> Cx<'_, 'gc> {
    /// A warning for the page's console about what script asked (Blink's
    /// `ExecutionContext::AddConsoleMessage`, a JavaScript one at the
    /// warning level), of no place, for the host to log.
    pub fn warn_console(&self, text: String) {
        self.add_console_note(ConsoleNote {
            source: ConsoleSource::JavaScript,
            level: ConsoleLevel::Warning,
            text,
            url: String::new(),
            line: None,
        });
    }

    /// `note` for the page's console, for the host to log.
    pub fn add_console_note(&self, note: ConsoleNote) {
        self.changes.console.notes.borrow_mut().push((note, false));
    }

    /// `note` for the page's console, at the script running as the
    /// operation ends, if one is (its URL and line; else `note`'s as it
    /// is: the document's URL, say): the engine places it then.
    pub fn add_console_note_at_script(&self, note: ConsoleNote) {
        self.changes.console.notes.borrow_mut().push((note, true));
        self.changes
            .microtasks
            .set(self.changes.microtasks.get() | Microtasks::STACKS);
    }
}

impl Dom {
    /// The messages for the page's console since the last ask, in order.
    pub fn take_console_notes(&self) -> Vec<ConsoleNote> {
        std::mem::take(&mut *self.changes.console.notes.borrow_mut())
            .into_iter()
            .map(|(note, _)| note)
            .collect()
    }

    /// Whether a message waits to be placed at the script running.
    pub fn console_notes_unplaced(&self) -> bool {
        self.changes
            .console
            .notes
            .borrow()
            .iter()
            .any(|(_, unplaced)| *unplaced)
    }

    /// The messages waiting are at `at` (the script running's URL and
    /// 0-based line), or where they said with no script running.
    pub fn place_console_notes(&self, at: Option<(String, u32)>) {
        for (note, unplaced) in self.changes.console.notes.borrow_mut().iter_mut() {
            if std::mem::take(unplaced)
                && let Some((url, line)) = &at
            {
                note.url.clone_from(url);
                note.line = Some(*line);
            }
        }
    }
}
