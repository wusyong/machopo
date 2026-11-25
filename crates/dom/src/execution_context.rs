//! A realm's execution context: what its script runs with that isn't its
//! document's (HTML's environment settings object and the queues of its
//! event loop, Blink's `ExecutionContext`). Its time origin, its timers and
//! the UA's own, its task queue, the messages posted to it and its ports,
//! its `fetch()`s and blob reads, its `crypto` and `performance`: what a
//! worker's global has as much as a window. A window holds one (Blink's
//! `LocalDOMWindow` is one instead, inheriting the class); its listeners and
//! its wrapper stay the window's, as an event target's (Blink's
//! `EventTargetData` and `ScriptWrappable` are `EventTarget`'s, not the
//! execution context's).

use std::cell::Cell;

use elise::{Gc, GcObject, HeapVector, LocalExternal, Member};

use url::Url;

use crate::{Cx, ElementTask, NativeTimer, NodeRef, Timer, Window, sites};

/// The blob reads queued: each promise's number and what it resolves with.
pub(crate) type BlobReads = Vec<(u64, crate::BlobRead)>;

#[derive(GcObject)]
pub struct ExecutionContext {
    /// The global it is the execution context of, a window's: what
    /// answers for it what its global's document has (its base URL, its
    /// origin), as Blink's `LocalDOMWindow` answers `ExecutionContext::
    /// BaseURL()`. The DOM's own: no function gives it out, or its
    /// document, which would give the window (`window_of`) to a member of
    /// an interface a worker has too.
    window: Member<Window>,
    /// Or a worker's (`worker_global`), which answers its URL and origin
    /// itself, as Blink's `WorkerGlobalScope` does.
    worker: Member<crate::WorkerGlobalScope>,
    /// Its time origin (HR-Time's), in milliseconds since the DOM was made:
    /// what its events' and timers' times count from.
    pub(crate) time_origin: Cell<f64>,
    /// Its timers (HTML's map of active timers, Blink's
    /// `DOMTimerCoordinator`), in the order they were made: few at a time,
    /// found by a scan.
    pub(crate) timers: Member<HeapVector<Timer>>,
    /// The last timer's id (HTML's: ids go up from 1, a WebIDL `long`).
    pub(crate) last_timer_id: Cell<i32>,
    /// The nesting level of the timer whose callback runs, 0 out of one
    /// (HTML's "timer nesting level").
    pub(crate) nesting: Cell<u8>,
    /// The UA's own timers (`add_native_timer`, `add_target_timer`), in the
    /// order made.
    pub(crate) native_timers: Member<HeapVector<NativeTimer>>,
    /// The tasks queued for its elements and event targets, in order
    /// (`queue_element_task`, `push_target_task`).
    pub(crate) element_tasks: Member<HeapVector<ElementTask>>,
    /// The messages posted to it waiting to be delivered, in order
    /// (`message`'s posted message task source).
    pub(crate) posted_messages: Member<HeapVector<crate::PostedMessage>>,
    /// Its message ports enabled and entangled (`message`): alive while
    /// messages may come, their queues looked at.
    pub(crate) message_ports: Member<HeapVector<crate::EventTargetImpl>>,
    /// Its `fetch()`s in flight (`fetch`), in the order made.
    pub(crate) fetches: Member<HeapVector<crate::FetchRecord>>,
    /// Its `Worker`s not terminated (`worker`): alive till then, as their
    /// workers run till then (Chrome's `DedicatedWorker::
    /// HasPendingActivity`), found by their workers' numbers.
    pub(crate) workers: Member<HeapVector<crate::EventTargetImpl>>,
    /// The blob reads queued (`text()`, `arrayBuffer()`, `bytes()`), by
    /// their promises' numbers, what each resolves with (`blob`); taken
    /// and put back.
    pub(crate) blob_reads: LocalExternal<Cell<BlobReads>>,
    /// Its `crypto`, once asked for.
    pub(crate) crypto: Member<crate::Crypto>,
    /// Its `performance`, once asked for (`performance`).
    pub(crate) performance: Member<crate::EventTargetImpl>,
    /// Whether it reports an exception now (HTML's "in error reporting
    /// mode"): one thrown meanwhile isn't reported again.
    pub(crate) reporting: Cell<bool>,
}

impl<'gc> Cx<'_, 'gc> {
    /// A new execution context, its time origin now: `set_context_global`
    /// gives it its global.
    pub(crate) fn create_execution_context(&self) -> Gc<'gc, ExecutionContext> {
        self.gc.alloc_in(
            &sites::OTHER,
            ExecutionContextInit {
                window: None,
                worker: None,
                time_origin: Cell::new(self.origin.elapsed().as_secs_f64() * 1000.0),
                timers: None,
                last_timer_id: Cell::new(0),
                nesting: Cell::new(0),
                native_timers: None,
                element_tasks: None,
                posted_messages: None,
                message_ports: None,
                fetches: None,
                workers: None,
                blob_reads: LocalExternal::new(Cell::new(Vec::new())),
                crypto: None,
                performance: None,
                reporting: Cell::new(false),
            },
        )
    }

    /// Makes `window` the global `context` is the execution context of.
    pub(crate) fn set_context_global(
        &self,
        context: Gc<'gc, ExecutionContext>,
        window: Gc<'gc, Window>,
    ) {
        context.window.set(self.gc, Some(window));
    }

    /// Makes worker's global `global` the global `context` is the execution
    /// context of.
    pub(crate) fn set_context_worker_global(
        &self,
        context: Gc<'gc, ExecutionContext>,
        global: Gc<'gc, crate::WorkerGlobalScope>,
    ) {
        context.worker.set(self.gc, Some(global));
    }

    /// The document of `context`'s global, if it has one (a window's): the
    /// DOM's own, as the global is.
    fn context_document(&self, context: Gc<'gc, ExecutionContext>) -> Option<NodeRef<'gc>> {
        let window = context.window.get(self.gc)?;
        Some(self.window_document(window))
    }

    /// The number of `context`'s global's document, if it has one (a
    /// window's), for the host to know it by (the loader a worker it makes
    /// goes through): a number, not the document, which stays the DOM's
    /// own.
    pub fn context_document_id(&self, context: Gc<'gc, ExecutionContext>) -> Option<crate::NodeId> {
        self.context_document(context)
            .map(|document| self.node_id(document))
    }

    /// `context`'s API base URL (HTML's): its global's document's base
    /// URL, a worker's global's URL; none without one.
    pub fn context_base_url(&self, context: Gc<'gc, ExecutionContext>) -> Option<Url> {
        if let Some(global) = context.worker.get(self.gc) {
            return Some(self.worker_global_url(global).clone());
        }
        self.document_base_url(self.context_document(context)?)
    }

    /// `context`'s creation URL (HTML's): its global's document's URL, a
    /// worker's global's; none without one.
    pub fn context_url(&self, context: Gc<'gc, ExecutionContext>) -> Option<crate::DomString> {
        if let Some(global) = context.worker.get(self.gc) {
            return Some(crate::DomString::new(
                self.worker_global_url(global).as_str(),
            ));
        }
        self.document_url(self.context_document(context)?)
    }

    /// `context`'s origin (HTML's): its global's document's
    /// (`document_origin`), a worker's global's; none without one.
    pub fn context_origin(&self, context: Gc<'gc, ExecutionContext>) -> Option<url::Origin> {
        if let Some(global) = context.worker.get(self.gc) {
            return Some(self.worker_global_origin(global).clone());
        }
        self.document_origin(self.context_document(context)?)
    }

    /// `context`'s global, as an event target: what a message posted to
    /// it is for.
    pub(crate) fn context_target(
        &self,
        context: Gc<'gc, ExecutionContext>,
    ) -> crate::TargetRef<'gc> {
        match context.worker.get(self.gc) {
            Some(global) => crate::TargetRef::WorkerGlobal(global),
            None => {
                crate::TargetRef::Window(self.context_window(context).expect("a context's global"))
            }
        }
    }

    /// The window `context` is the execution context of, if its global is
    /// one: the DOM's own (`window`).
    pub(crate) fn context_window(
        &self,
        context: Gc<'gc, ExecutionContext>,
    ) -> Option<Gc<'gc, Window>> {
        context.window.get(self.gc)
    }

    /// The worker's global `context` is the execution context of, if its
    /// global is one.
    pub fn context_worker_global(
        &self,
        context: Gc<'gc, ExecutionContext>,
    ) -> Option<Gc<'gc, crate::WorkerGlobalScope>> {
        context.worker.get(self.gc)
    }

    /// The time since `context`'s time origin (the DOM's start with none),
    /// in milliseconds: an event's time stamp.
    pub fn now_in(&self, context: Option<Gc<'gc, ExecutionContext>>) -> f64 {
        let now = self.origin.elapsed().as_secs_f64() * 1000.0;
        context.map_or(now, |context| now - context.time_origin.get())
    }

    /// `context`'s time origin, in milliseconds since the DOM was made.
    pub fn context_time_origin(&self, context: Gc<'gc, ExecutionContext>) -> f64 {
        context.time_origin.get()
    }

    /// `url` stands for `data` now, for as long as `context`'s realm lives
    /// (`BlobUrlRequest::Register`, for its global's document, or for a
    /// worker's global).
    pub fn register_context_blob_url(
        &self,
        context: Gc<'gc, ExecutionContext>,
        url: String,
        data: crate::BlobData,
    ) {
        let document = match context.worker.get(self.gc) {
            Some(_) => None,
            None => match self.context_document(context) {
                Some(document) => Some(self.node_id(document)),
                None => return,
            },
        };
        self.request_blob_url(crate::BlobUrlRequest::Register {
            document,
            url,
            data,
        });
    }

    /// `window`'s execution context.
    pub fn window_context(&self, window: Gc<'gc, Window>) -> Gc<'gc, ExecutionContext> {
        window
            .context
            .get(self.gc)
            .expect("a window has its execution context")
    }
}
