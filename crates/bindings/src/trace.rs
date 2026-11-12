//! A Perfetto trace of the process: V8's own trace events (GC phases,
//! deopts) and the embedder's spans in one file, as Chrome's
//! trace has V8's next to Blink's (Chromium's `TracingControllerImpl` and
//! Perfetto's track events; V8 is built with Perfetto, as in Chromium).
//!
//! [`start`] has to come before V8 starts ([`crate::initialize`]): V8
//! registers its events only if Perfetto is up by then. Without a trace,
//! Perfetto stays down and each of V8's trace points costs a load and a
//! branch.
//!
//! [`mark`] and [`measure`] write the page's User Timing as Chrome's trace
//! has it (Blink's `UserTiming`, category `blink.user_timing`).
//!
//! [`enter_script`] counts the embedder's entries into script on a thread,
//! for spans named as Chrome's are (the outermost call is a `FunctionCall`).

use std::cell::Cell;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::platform;

/// The categories recorded: those Chrome is traced with to compare the two
/// (`scripts/speedometer.py`), so that both traces have the same of V8's
/// events, `machopo` for the embedder's spans and `blink.user_timing` for the
/// page's marks and measures. V8's GC phases are in
/// `devtools.timeline,disabled-by-default-v8.gc`, recorded with
/// `devtools.timeline`, as in Chrome; cppgc's (`CppGC.AtomicMark` and the
/// like: the isolate's `CppHeap`'s phases, which the elise heap marks in, as
/// Oilpan's in Chrome) are in `cppgc`. `v8.memory` has V8's heap's sizes and
/// limits as counters (the elise heap's in the global ones), which say why
/// V8 starts a full GC when it does. Not `disabled-by-default-v8.gc` on its
/// own: what it adds is V8's speeds, and its embedder heap's speed is the
/// empty `CppHeap`'s, which would mislead.
const CATEGORIES: &[&str] = &[
    "toplevel",
    "blink",
    "v8",
    "cc",
    "viz",
    "benchmark",
    "loading",
    "devtools.timeline",
    "cppgc",
    "v8.memory",
    "machopo",
    "blink.user_timing",
];

/// The ring buffer the trace goes through, written out every second: what
/// a second of tracing never fills, with room to spare (a Speedometer
/// suite's whole trace is a few MB). Were it to overflow, the oldest events
/// of that second would be lost, and the trace's stats would say so.
const BUFFER_KB: u32 = 64 * 1024;

/// Why [`start`] didn't.
#[derive(Debug)]
pub enum StartError {
    /// V8 has started: it wouldn't record its events.
    V8Started,
    /// The file can't be written, its path isn't UTF-8, or a trace was
    /// already started.
    File,
}

/// Starts recording a trace of the process into `file`, as Perfetto's
/// protobuf (Perfetto's UI and trace processor read it). Before V8 starts;
/// [`stop`] writes the end.
pub fn start(file: &Path) -> Result<(), StartError> {
    if platform::is_initialized() {
        return Err(StartError::V8Started);
    }
    if v8::machopo_trace::start(file, CATEGORIES, BUFFER_KB) {
        RECORDING.store(true, Ordering::Relaxed);
        Ok(())
    } else {
        Err(StartError::File)
    }
}

/// Stops the trace: the rest of it is written and the file closed. Does
/// nothing if none was started.
pub fn stop() {
    RECORDING.store(false, Ordering::Relaxed);
    v8::machopo_trace::stop();
}

/// The trace as the DOM's heap's (`elise::TracingController`; gin's
/// `V8Platform::GetTracingController`, a static, as cppgc's in Chrome): the
/// heap's phases, and its workers' runs of its jobs, as spans on the thread
/// that runs each.
pub fn gc_tracing_controller() -> &'static dyn elise::TracingController {
    &GcTracing
}

struct GcTracing;

impl elise::TracingController for GcTracing {
    /// Whether a trace records ([`start`] to [`stop`]): the flag the heap
    /// reads before each phase, so that a phase costs it a load and a branch
    /// with no trace, as a span of machopo's own does (cppgc reads its
    /// category's enabled flag so,
    /// `TracingController::GetCategoryGroupEnabled`). The trace's categories
    /// are fixed, so recording at all is recording `machopo`'s.
    fn enabled(&self) -> &AtomicBool {
        &RECORDING
    }

    fn begin(&self, name: &'static str) {
        self::begin(name);
    }

    fn end(&self) {
        self::end();
    }
}

static RECORDING: AtomicBool = AtomicBool::new(false);

/// Begins a span named `name` on this thread's track; [`end`] ends the
/// innermost one. A load and a branch with no trace.
pub fn begin(name: &str) {
    v8::machopo_trace::begin(name);
}

/// Ends the innermost span [`begin`] began on this thread.
pub fn end() {
    v8::machopo_trace::end();
}

/// A User Timing mark named `name`: an instant on this thread's track,
/// `ago` nanoseconds before now (negative: after), as Blink writes a
/// `performance.mark()`. A load and a branch with no trace.
pub fn mark(name: &str, ago: i64) {
    v8::machopo_trace::mark(name, ago);
}

/// A User Timing measure named `name`: a slice on a track of its own under
/// this thread's, from `start_ago` to `end_ago` nanoseconds before now
/// (negative: after), as Blink writes a `performance.measure()`. A load
/// and a branch with no trace.
pub fn measure(name: &str, start_ago: i64, end_ago: i64) {
    v8::machopo_trace::measure(name, start_ago, end_ago);
}

thread_local! {
    /// How many entries into script ([`enter_script`]) this thread is in.
    static SCRIPT_DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// The embedder's entry into script (a call, a construction, a script's or
/// a module's evaluation, a microtask checkpoint), for as long as it lives
/// (Blink's `MicrotasksScope` depth, which `V8ScriptRunner` reads).
pub struct ScriptEntry {
    outermost: bool,
}

impl ScriptEntry {
    /// Whether no script was on this thread's stack before it: a call is
    /// Chrome's `FunctionCall` only then, not when script calls back into
    /// script through the DOM (a listener `click()` runs).
    pub fn is_outermost(&self) -> bool {
        self.outermost
    }
}

impl Drop for ScriptEntry {
    fn drop(&mut self) {
        SCRIPT_DEPTH.with(|depth| {
            depth.set(
                depth
                    .get()
                    .checked_sub(1)
                    .expect("an entry leaves after it entered"),
            )
        });
    }
}

/// Whether script is on this thread's stack: an entry ([`enter_script`])
/// is alive (HTML's "JavaScript execution context stack" isn't empty).
pub fn in_script() -> bool {
    SCRIPT_DEPTH.with(|depth| depth.get() > 0)
}

/// Enters script on this thread until the entry is dropped.
pub fn enter_script() -> ScriptEntry {
    // Each entry is frames on this thread's stack: far fewer than u32::MAX.
    let depth = SCRIPT_DEPTH.with(|depth| {
        depth.replace(
            depth
                .get()
                .checked_add(1)
                .expect("script nests no deeper than the stack"),
        )
    });
    ScriptEntry {
        outermost: depth == 0,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{StartError, enter_script, in_script, start};

    /// Only an entry with none around it is the outermost; once the inner
    /// ones are gone, the next is inner to the outer one still, and after
    /// it, outermost again.
    #[test]
    fn the_outermost_entry_into_script() {
        assert!(!in_script());
        let outer = enter_script();
        assert!(outer.is_outermost());
        assert!(in_script());
        let inner = enter_script();
        assert!(!inner.is_outermost());
        drop(inner);
        assert!(in_script());
        assert!(!enter_script().is_outermost());
        drop(outer);
        assert!(!in_script());
        assert!(enter_script().is_outermost());
    }

    /// Once V8 has started it wouldn't record its events: no trace, and
    /// the file isn't touched.
    #[test]
    fn a_trace_starts_only_before_v8() {
        crate::initialize();
        let file = Path::new("never-written.pftrace");
        assert!(matches!(start(file), Err(StartError::V8Started)));
        assert!(!file.exists());
    }
}
