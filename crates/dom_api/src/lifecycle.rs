//! A document's loading as its host drives it (HTML's "the end" of
//! parsing, https://html.spec.whatwg.org/#the-end): its readiness and the
//! events it fires, `readystatechange`, `DOMContentLoaded` and the
//! window's `load` (whose target is the document); its unloading as its
//! frame navigates on (`pagehide`, `unload`); and the window's
//! `unhandledrejection` (HTML's "notify about rejected promises").

use dom::names::event;
use dom::{
    Cx, DocumentReadyState, Event, EventFlags, LoadProgress, NavigationMilestone, NodeRef,
    NodeRoot, TargetRef, TargetRoot,
};
use elise::Persistent;
use std::time::Instant;

use crate::typed::ScriptHost;
use crate::{dispatch, fire_event};

/// What `check_completed` found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadCheck {
    /// Nothing delays its `load` any more: its window's `load` task is
    /// queued.
    Queued,
    /// It isn't parsed yet, or a request of its own or a child frame
    /// delays its `load`.
    Waits,
    /// Only images delay its `load`, those its style found now among them:
    /// its host fetches them (one that fails as it is asked for, a revoked
    /// blob URL's, delays it no more, and it may check again).
    WaitsForImages,
}

/// Whether `document` (a frame's) has loaded, HTML's "the end" once it is
/// parsed (Blink's `Document::CheckCompleted` and `ShouldComplete`): its
/// window's `load` task queued if nothing delays it. What does: a request
/// its host fetches for it (`Cx::add_load_request`, a `<link>`'s sheet's),
/// a child frame loading (a navigation on its way, `Cx::frame_navigating`,
/// or its document not loaded), its own requests that delay its load event
/// (an `<img>`'s, an external script's), and the images backgrounds wait
/// for. Its style is brought up to date through `layout` first, if parsed
/// and nothing else delays it: the backgrounds its style finds delay it
/// too (Blink finds them in the style recalc before `load`).
pub fn check_completed<'gc>(
    cx: &Cx<'_, 'gc>,
    layout: Option<&mut dyn crate::FrameLayout>,
    document: NodeRef<'gc>,
) -> LoadCheck {
    if cx.load_progress(document) != LoadProgress::Parsed
        || cx.has_load_requests(document)
        || children_loading(cx, document)
    {
        return LoadCheck::Waits;
    }
    if let Some(layout) = layout {
        layout.update_style_and_layout(cx);
    }
    // Its images that delay it (not a lazy one's, as in Chrome), and the
    // backgrounds' (Chrome's too).
    if cx.background_images_loading() || cx.load_event_delayed(document) {
        return LoadCheck::WaitsForImages;
    }
    // HTML's "the end": complete and the window's `load` are a task of the
    // DOM manipulation task source, after the element tasks queued before
    // it (a script's `error` for an empty `src`, an image's `load`); its
    // iframe still delays its parent's `load` meanwhile.
    cx.queue_window_load(document);
    LoadCheck::Queued
}

/// Whether a child frame of `document`'s delays its `load` (HTML: an
/// iframe delays its document's load event; Blink's
/// `AllDescendantsAreComplete`): its navigation on its way, or its
/// document not loaded.
fn children_loading<'gc>(cx: &Cx<'_, 'gc>, document: NodeRef<'gc>) -> bool {
    cx.child_content_documents(document)
        .into_iter()
        .any(|child| cx.frame_navigating(child) || cx.load_progress(child) != LoadProgress::Loaded)
}

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
    /// Its parser aborted, after interactive (HTML's "abort a parser":
    /// its body stopped short, or its load was stopped): complete, with
    /// no `load`.
    Aborted,
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
            DocumentReadiness::Loaded | DocumentReadiness::Aborted => {
                cx.set_ready_state(node, DocumentReadyState::Complete);
                mark(NavigationMilestone::DomComplete);
            }
            // Parsed from now on: its `load` may come (`check_completed`).
            DocumentReadiness::ContentLoaded => cx.set_content_loaded(node),
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
        DocumentReadiness::Interactive | DocumentReadiness::Aborted => {
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

/// `document`'s load ended, right after its window's `load` in the same
/// task: it delays its parent's `load` no more (`Cx::set_load_ended`); a
/// child frame's iframe fires `load` (HTML's "iframe load event steps";
/// Chrome's `LocalDOMWindow::DispatchLoadEvent` calls the owner's
/// `DispatchLoad` right after the window's), unless script took the
/// iframe away meanwhile; then its parent may have loaded. The event is
/// dispatched from here, the child's realm current: what of it is a realm's
/// is the parent's all the same (its wrappers, its listeners' calls, their
/// errors, `window.event`).
pub(crate) fn end_load<H: ScriptHost>(host: &mut H, document: &NodeRoot) {
    let owner = host.mutate(|cx| {
        let document = document.get(cx.gc);
        cx.set_load_ended(document);
        if cx.is_discarded(document) {
            return None;
        }
        cx.frame_owner(document).map(|owner| cx.root(owner))
    });
    let Some(owner) = owner else {
        return;
    };
    fire_event(
        host,
        &TargetRoot::Node(owner.clone()),
        event::LOAD,
        false,
        false,
    );
    let parent = host.read(|cx| cx.root(cx.document_of(owner.get(cx.gc))));
    crate::frame::check_parent_completed(host, &parent);
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
    // HTML's unload counter: no child frame made, no navigation, as they
    // fire.
    host.mutate(|cx| cx.change_unload_counter(document.get(cx.gc), true));
    fire_event(host, &window, event::PAGEHIDE, false, false);
    let start = Instant::now();
    fire_event(host, &window, event::UNLOAD, false, false);
    let end = Instant::now();
    host.mutate(|cx| cx.change_unload_counter(document.get(cx.gc), false));
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
            cx.target_context(window),
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
        let event = cx.create_promise_rejection_event(
            event::UNHANDLEDREJECTION,
            Some(cx.window_context(window)),
        );
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
