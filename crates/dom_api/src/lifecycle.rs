//! A document's loading as its host drives it (HTML's "the end" of
//! parsing, https://html.spec.whatwg.org/#the-end): its readiness and the
//! events it fires, `readystatechange`, `DOMContentLoaded` and the
//! window's `load` (whose target is the document); its unloading as its
//! frame navigates on (`pagehide`, `unload`); and the window's
//! `unhandledrejection` (HTML's "notify about rejected promises").

use dom::names::event;
use dom::{
    DocumentReadyState, Event, EventFlags, NavigationMilestone, NodeRoot, TargetRef, TargetRoot,
};
use elise::Persistent;
use std::time::Instant;

use crate::typed::ScriptHost;
use crate::{dispatch, fire_event};

/// Where a document's loading is (`document_ready`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentReadiness {
    /// Parsed, its parser-blocking scripts run: interactive (its deferred
    /// scripts run next).
    Interactive,
    /// Its deferred scripts run: `DOMContentLoaded`.
    ContentLoaded,
    /// Loaded (nothing more to wait for): complete, then the window's
    /// `load`.
    Loaded,
}

/// HTML's "the end" at `readiness` for `document` (a window's): its
/// readiness updated (`readystatechange` at it); or `DOMContentLoaded` at
/// it (bubbling); or, complete, `load` at its window with it as the target
/// (HTML's legacy target override). The host performs a microtask
/// checkpoint after each event, as each is a task's.
pub fn document_ready<H: ScriptHost>(
    host: &mut H,
    document: &NodeRoot,
    readiness: DocumentReadiness,
) {
    let (target, window) = host.read(|cx| {
        let node = document.get(cx.gc);
        let window = cx
            .window_of(node)
            .map(|window| cx.root_target(TargetRef::Window(window)));
        // Navigation Timing's: the readiness's time, as it changes (Blink's
        // `DocumentTiming::MarkDomInteractive`, `MarkDomComplete`).
        let mark = |milestone| {
            if let Some(window) = cx.window_of(node) {
                cx.mark_navigation(window, milestone);
            }
        };
        match readiness {
            DocumentReadiness::Interactive => {
                cx.set_ready_state(node, DocumentReadyState::Interactive);
                mark(NavigationMilestone::DomInteractive);
            }
            DocumentReadiness::Loaded => {
                cx.set_ready_state(node, DocumentReadyState::Complete);
                mark(NavigationMilestone::DomComplete);
            }
            DocumentReadiness::ContentLoaded => {}
        }
        (cx.root_target(TargetRef::Node(node)), window)
    });
    // An event's start and end around its dispatch (Blink's
    // `DocumentLoadTiming`).
    let mark = |host: &mut H, milestone| {
        host.read(|cx| {
            if let Some(window) = cx.window_of(document.get(cx.gc)) {
                cx.mark_navigation(window, milestone);
            }
        });
    };
    match readiness {
        DocumentReadiness::Interactive => {
            fire_event(host, &target, event::READYSTATECHANGE, false, false);
        }
        DocumentReadiness::ContentLoaded => {
            mark(host, NavigationMilestone::DomContentLoadedEventStart);
            fire_event(host, &target, event::DOM_CONTENT_LOADED, true, false);
            mark(host, NavigationMilestone::DomContentLoadedEventEnd);
        }
        DocumentReadiness::Loaded => {
            fire_event(host, &target, event::READYSTATECHANGE, false, false);
            if let Some(window) = window {
                mark(host, NavigationMilestone::LoadEventStart);
                fire_load(host, &window, &target);
                mark(host, NavigationMilestone::LoadEventEnd);
            }
            // HTML's completely loaded: its `load` is over.
            host.read(|cx| cx.set_completely_loaded(document.get(cx.gc)));
            // Navigation Timing's "queue the navigation timing entry": its
            // observers have it once its `load` is over.
            crate::performance::load_ended(host, document);
        }
    }
}

/// HTML's "unload a document" of `document` (a window's), as its frame
/// navigates on: `pagehide` at its window (a plain event: machopo has no
/// `PageTransitionEvent`, whose `persisted` would be false), then
/// `unload`; when the `unload` started and ended (HTML's "unload timing
/// info"). The host performs a microtask checkpoint after each. Then the
/// images it used are let go (`release_document_images`).
pub fn unload_document<H: ScriptHost>(
    host: &mut H,
    document: &NodeRoot,
) -> Option<(Instant, Instant)> {
    let window = host.read(|cx| {
        cx.window_of(document.get(cx.gc))
            .map(|window| cx.root_target(TargetRef::Window(window)))
    })?;
    fire_event(host, &window, event::PAGEHIDE, false, false);
    let start = Instant::now();
    fire_event(host, &window, event::UNLOAD, false, false);
    let end = Instant::now();
    // It shuts down: the images it used go, but those others use.
    host.mutate(|cx| cx.release_document_images(document.get(cx.gc)));
    Some((start, end))
}

/// The window's `load`, its target the document (HTML's "fire an event
/// named load at window, with legacy target override flag set"): the
/// window alone is its path.
fn fire_load<H: ScriptHost>(host: &mut H, window: &TargetRoot, document: &TargetRoot) {
    let event = host.mutate(|cx| {
        let window = window.get(cx.gc);
        let event = cx.create_event(
            event::LOAD,
            EventFlags {
                trusted: true,
                ..EventFlags::default()
            },
            cx.target_window(window),
        );
        Persistent::new(cx.gc, event)
    });
    crate::event_target::dispatch_with_target(host, window, document, &event);
}

/// HTML's "notify about rejected promises" for one promise: an
/// `unhandledrejection` (a `PromiseRejectionEvent`, cancelable) at
/// `window`, its promise and reason the host's values; whether no listener
/// canceled it (then the host logs it).
pub fn report_unhandled_rejection<H: ScriptHost>(
    host: &mut H,
    window: &Persistent<dom::Window>,
    promise: H::Value,
    reason: H::Value,
) -> bool {
    let mut values = Some((promise, reason));
    let event: Persistent<Event> = host.mutate_storing(|cx, store| {
        let window = window.get(cx.gc);
        let event = cx.create_promise_rejection_event(event::UNHANDLEDREJECTION, Some(window));
        let data = cx
            .promise_rejection_data(event)
            .expect("a rejection's data");
        let (promise, reason) = values.take().expect("stored once");
        store(cx.promise_edge(data), promise);
        store(cx.reason_edge(data), reason);
        Persistent::new(cx.gc, event)
    });
    let target = host.read(|cx| cx.root_target(TargetRef::Window(window.get(cx.gc))));
    dispatch(host, &target, &event)
}
