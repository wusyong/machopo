//! A document's window (HTML's `Window`, Blink's `LocalDOMWindow`), a
//! realm's global object, and its `Location`. A window is an event target
//! (its listeners its own) and the parent of its document in an event's
//! path; its document knows it (`DocumentData`), and its URL is the
//! document's.

use std::cell::{Cell, RefCell};

use elise::{Gc, GcObject, HeapVector, JsEdge, LocalExternal, Member};

use crate::{
    Cx, DocumentData, DocumentDataInit, DocumentReadyState, DomString, Listener, NameId, NodeKind,
    NodeRef, sites,
};

/// A window: its document, its location once asked for, its listeners and
/// its time origin.
/// The `hashchange`s queued: their URLs before and after.
type HashChanges = Vec<(DomString, DomString)>;

/// The blob reads queued: each promise's number and what it resolves with.
type BlobReads = Vec<(u64, crate::BlobRead)>;

#[derive(GcObject)]
pub struct Window {
    document: Member<crate::Node>,
    location: Member<Location>,
    pub(crate) history: Member<crate::History>,
    /// Its `navigator`, once asked for.
    pub(crate) navigator: Member<crate::Navigator>,
    /// Its `crypto`, once asked for.
    pub(crate) crypto: Member<crate::Crypto>,
    /// Its `performance`, once asked for, and its document's navigation
    /// timing, once its host gave it (`performance`; taken and put back).
    pub(crate) performance: Member<crate::EventTargetImpl>,
    pub(crate) navigation_timing: LocalExternal<Cell<Option<Box<crate::NavigationTiming>>>>,
    /// The `hashchange`s queued, their URLs before and after, in order
    /// (`history`'s `queue_hash_change`); taken and put back.
    pub(crate) hash_changes: LocalExternal<Cell<HashChanges>>,
    /// The blob reads queued (`text()`, `arrayBuffer()`, `bytes()`), by
    /// their promises' numbers, what each resolves with (`blob`); taken
    /// and put back.
    pub(crate) blob_reads: LocalExternal<Cell<BlobReads>>,
    pub(crate) listeners: Member<HeapVector<Listener>>,
    /// Its time origin (HR-Time's), in milliseconds since the DOM was made:
    /// what its events' times count from.
    time_origin: Cell<f64>,
    /// Its timers (HTML's map of active timers, Blink's
    /// `DOMTimerCoordinator`), in the order they were made: few at a time,
    /// found by a scan.
    timers: Member<HeapVector<Timer>>,
    /// The last timer's id (HTML's: ids go up from 1, a WebIDL `long`).
    last_timer_id: Cell<i32>,
    /// Its animation frame callbacks (HTML's map of them, Blink's
    /// `FrameRequestCallbackCollection`), in the order requested.
    animation_frames: Member<HeapVector<AnimationFrame>>,
    /// The element tasks queued for its document's elements, in order
    /// (`queue_element_task`).
    element_tasks: Member<HeapVector<ElementTask>>,
    /// The messages posted to it waiting to be delivered, in order
    /// (`message`'s posted message task source).
    pub(crate) posted_messages: Member<HeapVector<crate::PostedMessage>>,
    /// Its message ports enabled and entangled (`message`): alive while
    /// messages may come, their queues looked at.
    pub(crate) message_ports: Member<HeapVector<crate::EventTargetImpl>>,
    /// The UA's own timers of its document's elements (`add_native_timer`),
    /// in the order made.
    native_timers: Member<HeapVector<NativeTimer>>,
    /// Its intersection observers, weakly, in the order they were made; and
    /// those waiting for the task that delivers their entries, in the
    /// order they got their first (`intersection`).
    pub(crate) intersection_observers: Member<HeapVector<crate::intersection::ObserverRef>>,
    pub(crate) pending_intersection_observers: Member<HeapVector<crate::IntersectionObserver>>,
    /// Its resize observers, weakly, in the order they were made
    /// (`resize`).
    pub(crate) resize_observers: Member<HeapVector<crate::resize::ResizeObserverRef>>,
    /// The last animation frame callback's handle (from 1, a `long`).
    last_animation_frame: Cell<i32>,
    /// Its idle callbacks (`idle`), in the order requested, and the last
    /// one's handle (from 1, an `unsigned long`).
    pub(crate) idle_callbacks: Member<HeapVector<crate::IdleCallback>>,
    /// Its `fetch()`s in flight (`fetch`), in the order made.
    pub(crate) fetches: Member<HeapVector<crate::FetchRecord>>,
    /// Its document's `<img>`s whose requests wait for their images
    /// (`image`): alive till their image comes, nothing else holding them
    /// (a `new Image()`), as their task holds them after; Blink's
    /// `ImageLoader::HasPendingActivity`.
    pub(crate) pending_images: Member<HeapVector<crate::Node>>,
    pub(crate) last_idle_callback: Cell<u32>,
    /// The nesting level of the timer whose callback runs, 0 out of one
    /// (HTML's "timer nesting level").
    nesting: Cell<u8>,
    /// Whether it reports an exception now (HTML's "in error reporting
    /// mode"): one thrown meanwhile isn't reported again.
    reporting: Cell<bool>,
    /// Whether its page has the system's focus, and whether a focus from
    /// script shows a ring (`:focus-visible`): the host's, which it gives
    /// each window (Blink's `FocusController`), here for its members.
    page_focused: Cell<bool>,
    focus_visible: Cell<bool>,
    /// Whether its page is hidden (HTML's visibility state, the host's,
    /// which it gives each window).
    page_hidden: Cell<bool>,
    /// HTML's last activation timestamp (`now_since`'s milliseconds):
    /// positive infinity until user input activates it, negative infinity
    /// once its transient activation is consumed.
    last_activation: Cell<f64>,
    /// Its `customElements`, once asked for.
    pub(crate) custom_elements: Member<crate::CustomElementRegistry>,
    /// HTML's current event (`window.event`): the event whose listener of
    /// its realm runs, set by the engine around the call.
    current_event: Member<crate::Event>,
    /// The global proxy (V8's): the engine's reference, which the DOM never
    /// reads.
    wrapper: JsEdge,
}

/// A timer of a window (HTML's, Blink's `DOMTimer`): when it is due, how
/// it repeats, its nesting level, and its callback and arguments, the JS
/// engine's (a function; an array), which only the engine reads and
/// writes.
#[derive(GcObject)]
pub struct Timer {
    id: Cell<i32>,
    /// When it is due, in milliseconds since its window's time origin.
    due: Cell<f64>,
    /// Its task's number (`tasks`), as it was set or set again.
    number: Cell<u64>,
    /// An interval's milliseconds (`setInterval`); none for a timeout.
    interval: Cell<Option<f64>>,
    nesting: Cell<u8>,
    callback: JsEdge,
    arguments: JsEdge,
}

/// An animation frame callback of a window (`requestAnimationFrame`'s):
/// its handle, and its callback, the JS engine's (a function), which only
/// the engine reads and writes.
#[derive(GcObject)]
pub struct AnimationFrame {
    handle: Cell<i32>,
    callback: JsEdge,
}

/// An element task (HTML's "queue an element task"; Blink's `EnqueueEvent`):
/// an event to fire at an element, a text control's `select`; or an
/// `<img>`'s step (`ImageTaskAction`: its `load` or `error`, a `decode()`
/// promise settled), `action` and `data` its kind and word.
#[derive(GcObject)]
pub struct ElementTask {
    element: Member<crate::Node>,
    /// The event target a task of no element's is of (a `FileReader`'s
    /// read), its element its window's document.
    target: Member<crate::EventTargetImpl>,
    event_type: Cell<NameId>,
    action: Cell<u8>,
    data: Cell<u64>,
    /// When it was queued (the DOM's clock) and its number (`tasks`).
    queued: Cell<f64>,
    number: Cell<u64>,
}

/// `ElementTask::action`'s.
const FIRE: u8 = 0;
const IMAGE_LOAD: u8 = 1;
const IMAGE_ERROR: u8 = 2;
const IMAGE_DECODE: u8 = 3;
const IMAGE_REJECT: u8 = 4;
const CANVAS_BLOB: u8 = 5;
const SCRIPT: u8 = 6;
pub(crate) const HASH_CHANGE: u8 = 7;
pub(crate) const FILE_READ: u8 = 8;
pub(crate) const PERFORMANCE_TIMELINE: u8 = 9;
pub(crate) const GEOLOCATION: u8 = 10;
pub(crate) const WAKE_LOCK: u8 = 11;
const WINDOW_LOAD: u8 = 12;
pub(crate) const BLOB_READ: u8 = 13;

/// What an element task does (`Cx::take_element_task`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElementTaskKind {
    /// An event of the type fired at the element.
    Fire(NameId),
    Image(crate::ImageTaskAction),
    /// A canvas's `toBlob` callback called (`Cx::take_canvas_blob`), of the
    /// call the number names.
    CanvasBlob(u64),
    /// A script element's external script run, its turn come
    /// (`Cx::take_ready_script`).
    Script,
    /// A `hashchange` fired at the window (its document the task's
    /// element), from the first URL to the second.
    HashChange(DomString, DomString),
    /// A step of a `FileReader`'s read (the task's target), of the read the
    /// number names (`Cx::file_reader_reading`).
    FileRead(u64, crate::ReadStep),
    /// The task delivering a window's `performance`'s observers' entries
    /// (the task's target): Performance Timeline's.
    PerformanceTimeline,
    /// A step of the window's `navigator.geolocation`'s.
    Geolocation(crate::GeolocationStep),
    /// The window's `navigator.wakeLock`'s permission asked for the
    /// request whose promise the number names, or its answer.
    WakeLockPermission {
        promise: u64,
        answered: bool,
    },
    /// The window's `load` (HTML's "the end": a task of the DOM
    /// manipulation task source, so the element tasks queued before it
    /// run first), its document the task's element.
    WindowLoad,
    /// A blob read's promise, of the number, resolved (`Cx::take_blob_read`).
    BlobRead(u64),
}

/// A timer of the UA's own for an element (Blink's `TaskRunnerTimer`s of
/// an element's frame: a spin button's repeat), or for an event target of
/// no element (`AbortSignal.timeout()`'s: its element the window's
/// document): what it does, a word for it, and when it is due
/// (milliseconds since its window's time origin).
#[derive(GcObject)]
pub struct NativeTimer {
    element: Member<crate::Node>,
    target: Member<crate::EventTargetImpl>,
    action: Cell<u8>,
    data: Cell<u64>,
    due: Cell<f64>,
    /// Its task's number (`tasks`).
    number: Cell<u64>,
}

/// What a native timer does when it is due.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeAction {
    /// A number's spin button's repeated step, of the press its data
    /// numbers.
    SpinRepeat,
    /// `AbortSignal.timeout()`'s signal (the timer's target) aborts.
    AbortTimeout,
}

impl NativeAction {
    fn raw(self) -> u8 {
        match self {
            Self::SpinRepeat => 0,
            Self::AbortTimeout => 1,
        }
    }

    fn from_raw(raw: u8) -> Self {
        match raw {
            0 => Self::SpinRepeat,
            _ => Self::AbortTimeout,
        }
    }
}

/// A timer's id, when it is due, its nesting level and its interval.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimerState {
    pub id: i32,
    pub due: f64,
    pub nesting: u8,
    pub interval: Option<f64>,
}

/// A window's `Location`: its document's URL, read through the window.
#[derive(GcObject)]
pub struct Location {
    window: Member<Window>,
    wrapper: JsEdge,
}

/// HTML's transient activation duration, in milliseconds (Blink's
/// `kActivationLifespan`).
const TRANSIENT_ACTIVATION_DURATION: f64 = 5000.0;

impl<'gc> Cx<'_, 'gc> {
    /// A new window of `document` (which has none), its time origin now.
    ///
    /// # Panics
    ///
    /// If `document` isn't a document, or has a window already.
    pub fn create_window(&self, document: NodeRef<'gc>) -> Gc<'gc, Window> {
        assert_eq!(
            self.kind(document),
            NodeKind::Document,
            "a window's document"
        );
        let data = self.ensure_document_data(document);
        assert!(
            data.window.get(self.gc).is_none(),
            "a document has one window"
        );
        let window = self.gc.alloc_in(
            &sites::OTHER,
            WindowInit {
                document: Some(document),
                location: None,
                history: None,
                navigator: None,
                crypto: None,
                performance: None,
                navigation_timing: LocalExternal::new(Cell::new(None)),
                hash_changes: LocalExternal::new(Cell::new(Vec::new())),
                blob_reads: LocalExternal::new(Cell::new(Vec::new())),
                listeners: None,
                time_origin: Cell::new(self.origin.elapsed().as_secs_f64() * 1000.0),
                timers: None,
                last_timer_id: Cell::new(0),
                animation_frames: None,
                element_tasks: None,
                posted_messages: None,
                message_ports: None,
                native_timers: None,
                intersection_observers: None,
                pending_intersection_observers: None,
                resize_observers: None,
                last_animation_frame: Cell::new(0),
                idle_callbacks: None,
                fetches: None,
                pending_images: None,
                last_idle_callback: Cell::new(0),
                nesting: Cell::new(0),
                reporting: Cell::new(false),
                page_focused: Cell::new(false),
                focus_visible: Cell::new(true),
                page_hidden: Cell::new(false),
                last_activation: Cell::new(f64::INFINITY),
                custom_elements: None,
                current_event: None,
                wrapper: (),
            },
        );
        data.window.set(self.gc, Some(window));
        window
    }

    /// HTML's reuse of `window` for `document` (a child frame's initial
    /// `about:blank` navigated to a document of its origin: its window, and
    /// what is on it, stays; Blink's `DocumentLoader` reusing the
    /// `LocalDOMWindow`): the window's document is `document` now, the one
    /// before no window's.
    pub fn reuse_window(&self, window: Gc<'gc, Window>, document: NodeRef<'gc>) {
        let old = self.window_document(window);
        if let Some(data) = self.document_data(old) {
            data.window.set(self.gc, None);
        }
        let data = self.ensure_document_data(document);
        assert!(
            data.window.get(self.gc).is_none(),
            "a document has one window"
        );
        window.document.set(self.gc, Some(document));
        data.window.set(self.gc, Some(window));
        // The new document's entry has its own state.
        self.set_window_history_state(window, None);
    }

    /// `window` activated (HTML's activation notification steps, for each
    /// window they reach: `dom_api`'s go through the frame tree): its
    /// last activation is now, by its own clock.
    pub fn notify_activation(&self, window: Gc<'gc, Window>) {
        window.last_activation.set(self.now_since(Some(window)));
    }

    /// Whether `window` has transient activation (HTML's: activated less
    /// than the transient activation duration ago, Blink's 5 seconds).
    pub fn has_transient_activation(&self, window: Gc<'gc, Window>) -> bool {
        let last = window.last_activation.get();
        let now = self.now_since(Some(window));
        last <= now && now < last + TRANSIENT_ACTIVATION_DURATION
    }

    /// `window`'s current event (HTML's), if a listener of its realm runs.
    pub fn window_current_event(&self, window: Gc<'gc, Window>) -> Option<Gc<'gc, crate::Event>> {
        window.current_event.get(self.gc)
    }

    /// Sets `window`'s current event (HTML's "inner invoke": around a
    /// listener's call, the one before it again after).
    pub fn set_window_current_event(
        &self,
        window: Gc<'gc, Window>,
        event: Option<Gc<'gc, crate::Event>>,
    ) {
        window.current_event.set(self.gc, event);
    }

    /// Whether `window` has sticky activation (HTML's: user input has
    /// activated it).
    pub fn has_sticky_activation(&self, window: Gc<'gc, Window>) -> bool {
        window.last_activation.get() != f64::INFINITY
    }

    /// `window`'s activation consumed (HTML's "consume user activation",
    /// for each window of the page: `dom_api`'s): unless it was never
    /// activated, its last activation is minus infinity, its sticky
    /// activation kept.
    pub fn consume_activation(&self, window: Gc<'gc, Window>) {
        if window.last_activation.get() != f64::INFINITY {
            window.last_activation.set(f64::NEG_INFINITY);
        }
    }

    /// `window`'s time origin as a Unix time, in milliseconds (HR-Time's
    /// `timeOrigin`, not coarsened).
    pub fn window_unix_time_origin(&self, window: Gc<'gc, Window>) -> f64 {
        self.changes.unix_origin.get() + window.time_origin.get()
    }

    /// Sets `window`'s time origin to `origin` (a navigation's start, the
    /// host's): its events' and timers' times count from it. One before the
    /// DOM was made (a navigation that started before its engine) is
    /// negative: the network's times of its navigation count from it too.
    pub fn set_window_time_origin(&self, window: Gc<'gc, Window>, origin: std::time::Instant) {
        let ms = match origin.checked_duration_since(self.origin) {
            Some(after) => after.as_secs_f64() * 1000.0,
            None => -self.origin.saturating_duration_since(origin).as_secs_f64() * 1000.0,
        };
        window.time_origin.set(ms);
    }

    /// HTML's "queue an element task" to fire an event of `event_type` at
    /// `element`: in its document's window, which runs it in a task of its
    /// own (`take_element_task`); none if its document has no window
    /// (Blink's event queue is its execution context's).
    pub fn queue_element_task(&self, element: NodeRef<'gc>, event_type: NameId) {
        let Some(window) = self.window_of(self.document_of(element)) else {
            return;
        };
        self.push_element_task(window, element, event_type, FIRE, 0);
    }

    /// `queue_element_task` of `canvas`'s `toBlob` callback, of the call
    /// `id` names: whether it was queued (its document has a window).
    pub(crate) fn queue_canvas_blob_task(&self, canvas: NodeRef<'gc>, id: u64) -> bool {
        let Some(window) = self.window_of(self.document_of(canvas)) else {
            return false;
        };
        self.push_element_task(window, canvas, crate::names::event::LOAD, CANVAS_BLOB, id);
        true
    }

    /// `queue_element_task` of a script element's external script, its
    /// turn come, in the window of `document`, the one it was prepared in:
    /// its load event waits for it, wherever the element went (the task
    /// runs nothing then). None without a window: nothing waits.
    pub(crate) fn queue_script_task(&self, element: NodeRef<'gc>, document: NodeRef<'gc>) {
        let Some(window) = self.window_of(document) else {
            return;
        };
        self.push_element_task(window, element, crate::names::event::LOAD, SCRIPT, 0);
    }

    /// `queue_element_task` of an `<img>`'s step.
    pub(crate) fn queue_element_task_with(
        &self,
        element: NodeRef<'gc>,
        action: crate::ImageTaskAction,
    ) {
        let Some(window) = self.window_of(self.document_of(element)) else {
            return;
        };
        let (kind, data) = match action {
            crate::ImageTaskAction::Event {
                load: true,
                generation,
            } => (IMAGE_LOAD, generation),
            crate::ImageTaskAction::Event {
                load: false,
                generation,
            } => (IMAGE_ERROR, generation),
            crate::ImageTaskAction::Decode { promise } => (IMAGE_DECODE, promise),
            crate::ImageTaskAction::Reject { promise } => (IMAGE_REJECT, promise),
        };
        self.push_element_task(window, element, crate::names::event::LOAD, kind, data);
    }

    pub(crate) fn push_element_task(
        &self,
        window: Gc<'gc, Window>,
        element: NodeRef<'gc>,
        event_type: NameId,
        action: u8,
        data: u64,
    ) {
        self.push_task(window, element, None, event_type, action, data);
    }

    /// An element task of `target`'s, of no element (its element `window`'s
    /// document).
    pub(crate) fn push_target_task(
        &self,
        window: Gc<'gc, Window>,
        target: Gc<'gc, crate::EventTargetImpl>,
        action: u8,
        data: u64,
    ) {
        let document = self.window_document(window);
        self.push_task(
            window,
            document,
            Some(target),
            crate::names::NONE,
            action,
            data,
        );
    }

    fn push_task(
        &self,
        window: Gc<'gc, Window>,
        element: NodeRef<'gc>,
        target: Option<Gc<'gc, crate::EventTargetImpl>>,
        event_type: NameId,
        action: u8,
        data: u64,
    ) {
        let (queued, number) = self.stamp_queued_task();
        let task = self.gc.alloc_in(
            &sites::OTHER,
            ElementTaskInit {
                element: Some(element),
                target,
                event_type: Cell::new(event_type),
                action: Cell::new(action),
                data: Cell::new(data),
                queued: Cell::new(queued),
                number: Cell::new(number),
            },
        );
        let tasks = match window.element_tasks.get(self.gc) {
            Some(tasks) => tasks,
            None => {
                let tasks = HeapVector::new(self.gc, 1);
                window.element_tasks.set(self.gc, Some(tasks));
                tasks
            }
        };
        tasks.push(self.gc, task);
    }

    /// A native timer of `element`'s, doing `action` with `data` in
    /// `delay` milliseconds from `from` (since its window's time origin;
    /// none: now): in its document's window (none without one). One a
    /// native timer adds as it runs counts from the time it ran at, as an
    /// interval does.
    pub fn add_native_timer(
        &self,
        element: NodeRef<'gc>,
        action: NativeAction,
        data: u64,
        delay: f64,
        from: Option<f64>,
    ) {
        let Some(window) = self.window_of(self.document_of(element)) else {
            return;
        };
        self.push_native_timer(
            window,
            element,
            None,
            action,
            data,
            from.unwrap_or_else(|| self.now_since(Some(window))) + delay,
        );
    }

    /// A native timer of `window`'s for `target`, an event target of no
    /// element, doing `action` in `delay` milliseconds from now.
    pub fn add_target_timer(
        &self,
        window: Gc<'gc, Window>,
        target: Gc<'gc, crate::EventTargetImpl>,
        action: NativeAction,
        delay: f64,
    ) {
        let document = self.window_document(window);
        let due = self.now_since(Some(window)) + delay;
        self.push_native_timer(window, document, Some(target), action, 0, due);
    }

    fn push_native_timer(
        &self,
        window: Gc<'gc, Window>,
        element: NodeRef<'gc>,
        target: Option<Gc<'gc, crate::EventTargetImpl>>,
        action: NativeAction,
        data: u64,
        due: f64,
    ) {
        let timer = self.gc.alloc_in(
            &sites::OTHER,
            NativeTimerInit {
                element: Some(element),
                target,
                action: Cell::new(action.raw()),
                data: Cell::new(data),
                due: Cell::new(due),
                number: Cell::new(self.next_task_number()),
            },
        );
        let timers = match window.native_timers.get(self.gc) {
            Some(timers) => timers,
            None => {
                let timers = HeapVector::new(self.gc, 1);
                window.native_timers.set(self.gc, Some(timers));
                timers
            }
        };
        timers.push(self.gc, timer);
    }

    /// `window`'s native timer due by `now` to run next (the earliest, of
    /// those the first made), taken out: its element, action and data, and
    /// its target if it is of no element.
    pub fn take_due_native_timer(
        &self,
        window: Gc<'gc, Window>,
        now: f64,
    ) -> Option<(
        NodeRef<'gc>,
        NativeAction,
        u64,
        Option<Gc<'gc, crate::EventTargetImpl>>,
    )> {
        let timers = window.native_timers.get(self.gc)?;
        let (index, timer) = self.first_native_timer(window)?;
        if timer.due.get() > now {
            return None;
        }
        timers.remove(self.gc, index);
        Some((
            timer.element.get(self.gc).expect("a timer's element"),
            NativeAction::from_raw(timer.action.get()),
            timer.data.get(),
            timer.target.get(self.gc),
        ))
    }

    /// Whether `window` has an element task queued.
    pub fn has_element_task(&self, window: Gc<'gc, Window>) -> bool {
        window
            .element_tasks
            .get(self.gc)
            .is_some_and(|tasks| !tasks.is_empty())
    }

    /// `window`'s first element task taken out, if the host's pass lets it
    /// run (`tasks`): its element, what it does, and the event target it is
    /// of if it is of no element's.
    pub fn take_element_task(
        &self,
        window: Gc<'gc, Window>,
    ) -> Option<(
        NodeRef<'gc>,
        ElementTaskKind,
        Option<Gc<'gc, crate::EventTargetImpl>>,
    )> {
        let tasks = window.element_tasks.get(self.gc)?;
        let task = tasks.get(self.gc, 0)?;
        if !self.task_may_run(task.number.get()) {
            return None;
        }
        tasks.remove(self.gc, 0);
        let data = task.data.get();
        let kind = match task.action.get() {
            HASH_CHANGE => {
                let (old, new) = self
                    .take_hash_change(window)
                    .expect("a hashchange queued with its task");
                ElementTaskKind::HashChange(old, new)
            }
            IMAGE_LOAD | IMAGE_ERROR => ElementTaskKind::Image(crate::ImageTaskAction::Event {
                load: task.action.get() == IMAGE_LOAD,
                generation: data,
            }),
            IMAGE_DECODE => {
                ElementTaskKind::Image(crate::ImageTaskAction::Decode { promise: data })
            }
            IMAGE_REJECT => {
                ElementTaskKind::Image(crate::ImageTaskAction::Reject { promise: data })
            }
            CANVAS_BLOB => ElementTaskKind::CanvasBlob(data),
            SCRIPT => ElementTaskKind::Script,
            FILE_READ => ElementTaskKind::FileRead(
                data >> 2,
                match data & 3 {
                    0 => crate::ReadStep::LoadStart,
                    1 => crate::ReadStep::Progress,
                    _ => crate::ReadStep::Load,
                },
            ),
            PERFORMANCE_TIMELINE => ElementTaskKind::PerformanceTimeline,
            GEOLOCATION => ElementTaskKind::Geolocation(match data & 3 {
                0 => {
                    crate::GeolocationStep::Timer(u32::try_from(data >> 2).expect("a request's id"))
                }
                1 => crate::GeolocationStep::Permission { answered: false },
                _ => crate::GeolocationStep::Permission { answered: true },
            }),
            WAKE_LOCK => ElementTaskKind::WakeLockPermission {
                promise: data >> 1,
                answered: data & 1 == 1,
            },
            WINDOW_LOAD => ElementTaskKind::WindowLoad,
            BLOB_READ => ElementTaskKind::BlobRead(data),
            _ => ElementTaskKind::Fire(task.event_type.get()),
        };
        Some((
            task.element.get(self.gc).expect("a task's element"),
            kind,
            task.target.get(self.gc),
        ))
    }

    /// `document`'s window, if it has one.
    pub fn window_of(&self, document: NodeRef<'gc>) -> Option<Gc<'gc, Window>> {
        self.document_data(document)?.window.get(self.gc)
    }

    /// `document`'s relevant global: its window, or for one of none the
    /// window whose script made it (`set_document_relevant_window`).
    pub fn document_relevant_window(&self, document: NodeRef<'gc>) -> Option<Gc<'gc, Window>> {
        let data = self.document_data(document)?;
        data.window
            .get(self.gc)
            .or_else(|| data.relevant_window.get(self.gc))
    }

    /// `document`, which has no window of its own, made by script of
    /// `window` (or of a document whose relevant global it is).
    pub fn set_document_relevant_window(
        &self,
        document: NodeRef<'gc>,
        window: Option<Gc<'gc, Window>>,
    ) {
        self.ensure_document_data(document)
            .relevant_window
            .set(self.gc, window);
    }

    pub fn window_document(&self, window: Gc<'gc, Window>) -> NodeRef<'gc> {
        window.document.get(self.gc).expect("a window's document")
    }

    /// `window`'s `Location`, made at the first ask (the same after).
    pub fn window_location(&self, window: Gc<'gc, Window>) -> Gc<'gc, Location> {
        if let Some(location) = window.location.get(self.gc) {
            return location;
        }
        let location = self.gc.alloc_in(
            &sites::OTHER,
            LocationInit {
                window: Some(window),
                wrapper: (),
            },
        );
        window.location.set(self.gc, Some(location));
        location
    }

    pub fn location_window(&self, location: Gc<'gc, Location>) -> Gc<'gc, Window> {
        location.window.get(self.gc).expect("a location's window")
    }

    /// `window`'s time origin, in milliseconds since the DOM was made.
    pub fn window_time_origin(&self, window: Gc<'gc, Window>) -> f64 {
        window.time_origin.get()
    }

    /// Milliseconds since `window`'s time origin, or since the DOM was made
    /// with none: an event's time (`timeStamp`'s, before it is coarsened).
    pub fn now_since(&self, window: Option<Gc<'gc, Window>>) -> f64 {
        let now = self.origin.elapsed().as_secs_f64() * 1000.0;
        window.map_or(now, |window| now - window.time_origin.get())
    }

    /// `document`'s URL, if it has one (a document a window's: what its
    /// `location` reads).
    pub fn document_url(&self, document: NodeRef<'gc>) -> Option<DomString> {
        let data = self.document_data(document)?;
        let url = data.into_ref().url.get().borrow();
        (!url.is_empty()).then(|| url.clone())
    }

    /// Whether something (an `<img>`'s request) delays `document`'s load
    /// event: its host waits to fire it.
    pub fn load_event_delayed(&self, document: NodeRef<'gc>) -> bool {
        self.document_data(document)
            .is_some_and(|data| data.load_delay.get() > 0)
    }

    /// One more (`delays`) or one fewer of what delays `document`'s load
    /// event: one without a window's data delays nothing.
    pub(crate) fn change_load_delay(&self, document: NodeRef<'gc>, delays: bool) {
        let Some(data) = self.document_data(document) else {
            return;
        };
        let count = data.load_delay.get();
        data.load_delay.set(if delays {
            count.checked_add(1).expect("fewer than 2^32 delays")
        } else {
            count.saturating_sub(1)
        });
    }

    /// `document`'s readiness: complete if it has no data (none was set).
    pub fn ready_state(&self, document: NodeRef<'gc>) -> DocumentReadyState {
        match self
            .document_data(document)
            .map(|data| data.ready_state.get())
        {
            Some(0) => DocumentReadyState::Loading,
            Some(1) => DocumentReadyState::Interactive,
            _ => DocumentReadyState::Complete,
        }
    }

    pub fn set_ready_state(&self, document: NodeRef<'gc>, state: DocumentReadyState) {
        self.ensure_document_data(document)
            .ready_state
            .set(ready_state_bits(state));
    }

    /// Sets `document`'s URL.
    pub fn set_document_url(&self, document: NodeRef<'gc>, url: &str) {
        let data = self.ensure_document_data(document);
        *data.into_ref().url.get().borrow_mut() = DomString::new(url);
        self.base_url_stale(document);
    }

    /// `document`'s content type (`contentType`): what its host gave, else
    /// `text/html` for an HTML document and `application/xml` for another
    /// (DOM's default for one script makes).
    pub fn document_content_type(&self, document: NodeRef<'gc>) -> DomString {
        let given = self
            .document_data(document)
            .map(|data| data.into_ref().content_type.get().borrow().clone())
            .filter(|given| !given.is_empty());
        given.unwrap_or_else(|| {
            DomString::new(if self.is_html_document(document) {
                "text/html"
            } else {
                "application/xml"
            })
        })
    }

    pub fn set_document_content_type(&self, document: NodeRef<'gc>, content_type: &str) {
        let data = self.ensure_document_data(document);
        *data.into_ref().content_type.get().borrow_mut() = DomString::new(content_type);
    }

    /// `document`'s character encoding's name (`characterSet`): what its
    /// host decoded it as, else `UTF-8`.
    pub fn document_character_set(&self, document: NodeRef<'gc>) -> DomString {
        self.document_data(document)
            .map(|data| data.into_ref().character_set.get().borrow().clone())
            .filter(|given| !given.is_empty())
            .unwrap_or_else(|| DomString::new("UTF-8"))
    }

    pub fn set_document_character_set(&self, document: NodeRef<'gc>, name: &str) {
        let data = self.ensure_document_data(document);
        *data.into_ref().character_set.get().borrow_mut() = DomString::new(name);
    }

    /// `document.currentScript`.
    pub fn current_script(&self, document: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        self.document_data(document)?.current_script.get(self.gc)
    }

    /// `document`'s currently executing script is `script` now (HTML's
    /// "execute the script element": a script in a shadow tree is none);
    /// the one before, to put back after.
    pub fn set_current_script(
        &self,
        document: NodeRef<'gc>,
        script: Option<NodeRef<'gc>>,
    ) -> Option<NodeRef<'gc>> {
        let data = self.ensure_document_data(document);
        let before = data.current_script.get(self.gc);
        data.current_script.set(self.gc, script);
        before
    }

    /// `document`'s data, made if it has none.
    pub(crate) fn ensure_document_data(&self, document: NodeRef<'gc>) -> Gc<'gc, DocumentData> {
        if let Some(data) = self.document_data(document) {
            return data;
        }
        let data = self
            .gc
            .alloc_beside(document, &sites::OTHER, new_document_data());
        document.data.set(self.gc, Some(data.erase()));
        data
    }

    /// A new timer of `window`, due `timeout` milliseconds from now (HTML's
    /// timer initialization steps: a negative timeout is 0; made by a
    /// timer nested deeper than 5, at least 4 ms), repeating every
    /// `timeout` if `repeat`, its nesting level one more than the timer
    /// running now. Its callback and arguments are the engine's to set.
    pub fn add_timer(&self, window: Gc<'gc, Window>, timeout: f64, repeat: bool) -> Gc<'gc, Timer> {
        let running = window.nesting.get();
        let timeout = clamp_timeout(timeout, running);
        let id = window
            .last_timer_id
            .get()
            .checked_add(1)
            .expect("fewer than 2^31 timers a window");
        window.last_timer_id.set(id);
        let timer = self.gc.alloc_in(
            &sites::OTHER,
            TimerInit {
                id: Cell::new(id),
                due: Cell::new(self.now_since(Some(window)) + timeout),
                number: Cell::new(self.next_task_number()),
                interval: Cell::new(repeat.then_some(timeout)),
                nesting: Cell::new(running.saturating_add(1)),
                callback: (),
                arguments: (),
            },
        );
        let timers = match window.timers.get(self.gc) {
            Some(timers) => timers,
            None => {
                let timers = HeapVector::new(self.gc, 1);
                window.timers.set(self.gc, Some(timers));
                timers
            }
        };
        timers.push(self.gc, timer);
        timer
    }

    /// A new animation frame callback of `window` (`requestAnimationFrame`),
    /// its handle one more than the last; its callback is the engine's to
    /// set.
    pub fn add_animation_frame(&self, window: Gc<'gc, Window>) -> Gc<'gc, AnimationFrame> {
        let handle = window
            .last_animation_frame
            .get()
            .checked_add(1)
            .expect("fewer than 2^31 animation frame callbacks a window");
        window.last_animation_frame.set(handle);
        let frame = self.gc.alloc_in(
            &sites::OTHER,
            AnimationFrameInit {
                handle: Cell::new(handle),
                callback: (),
            },
        );
        let frames = match window.animation_frames.get(self.gc) {
            Some(frames) => frames,
            None => {
                let frames = HeapVector::new(self.gc, 1);
                window.animation_frames.set(self.gc, Some(frames));
                frames
            }
        };
        frames.push(self.gc, frame);
        frame
    }

    pub fn animation_frame_handle(&self, frame: Gc<'gc, AnimationFrame>) -> i32 {
        frame.handle.get()
    }

    /// `frame`'s reference to its callback, which only the JS engine reads
    /// and writes.
    pub fn animation_frame_callback_edge(&self, frame: Gc<'gc, AnimationFrame>) -> &'gc JsEdge {
        &frame.into_ref().callback
    }

    /// The handles of `window`'s animation frame callbacks, in the order
    /// requested: what a frame runs (HTML's "getting the keys" of the map:
    /// one requested meanwhile waits for the next frame).
    pub fn animation_frame_handles(&self, window: Gc<'gc, Window>) -> Vec<i32> {
        window
            .animation_frames
            .get(self.gc)
            .map_or_else(Vec::new, |frames| {
                (0..frames.len())
                    .filter_map(|index| frames.get(self.gc, index))
                    .map(|frame| frame.handle.get())
                    .collect()
            })
    }

    /// Whether `window` has an animation frame callback: what asks for a
    /// frame.
    pub fn has_animation_frames(&self, window: Gc<'gc, Window>) -> bool {
        window
            .animation_frames
            .get(self.gc)
            .is_some_and(|frames| !frames.is_empty())
    }

    /// `window`'s animation frame callback `handle` taken out, if it has it
    /// (`cancelAnimationFrame`, and a frame running it).
    pub fn take_animation_frame(
        &self,
        window: Gc<'gc, Window>,
        handle: i32,
    ) -> Option<Gc<'gc, AnimationFrame>> {
        let frames = window.animation_frames.get(self.gc)?;
        let index = (0..frames.len()).find(|&index| {
            frames
                .get(self.gc, index)
                .is_some_and(|frame| frame.handle.get() == handle)
        })?;
        let frame = frames.get(self.gc, index);
        frames.remove(self.gc, index);
        frame
    }

    /// Whether `window`'s page has the system's focus.
    pub fn page_focused(&self, window: Gc<'gc, Window>) -> bool {
        window.page_focused.get()
    }

    /// Whether a focus from script shows a ring (until the mouse gives
    /// one).
    pub fn focus_visible(&self, window: Gc<'gc, Window>) -> bool {
        window.focus_visible.get()
    }

    /// Whether `window`'s page is hidden (its document's visibility state
    /// `hidden`).
    pub fn page_hidden(&self, window: Gc<'gc, Window>) -> bool {
        window.page_hidden.get()
    }

    pub fn set_page_hidden(&self, window: Gc<'gc, Window>, hidden: bool) {
        window.page_hidden.set(hidden);
    }

    pub(crate) fn set_window_page_focused(&self, window: Gc<'gc, Window>, focused: bool) {
        window.page_focused.set(focused);
    }

    pub fn set_focus_visible(&self, window: Gc<'gc, Window>, visible: bool) {
        window.focus_visible.set(visible);
    }

    /// `at` in milliseconds since `window`'s time origin.
    pub fn time_at(&self, window: Gc<'gc, Window>, at: std::time::Instant) -> f64 {
        let since = at.saturating_duration_since(self.origin);
        since.as_secs_f64() * 1000.0 - window.time_origin.get()
    }

    /// `clearTimeout`, `clearInterval`: `window`'s timer `id` removed, if it
    /// has one.
    pub fn remove_timer(&self, window: Gc<'gc, Window>, id: i32) {
        let Some(timers) = window.timers.get(self.gc) else {
            return;
        };
        let found = (0..timers.len()).find(|&index| {
            timers
                .get(self.gc, index)
                .is_some_and(|timer| timer.id.get() == id)
        });
        if let Some(index) = found {
            timers.remove(self.gc, index);
        }
    }

    /// When `window`'s next timer is due, in milliseconds since its time
    /// origin; none with no timer.
    pub fn next_due(&self, window: Gc<'gc, Window>) -> Option<f64> {
        // The UA's own timers too (`add_native_timer`), and the idle
        // callbacks' timeouts (`idle`), which run as timers do.
        let native = window.native_timers.get(self.gc).and_then(|timers| {
            (0..timers.len())
                .filter_map(|index| timers.get(self.gc, index))
                .map(|timer| timer.due.get())
                .reduce(f64::min)
        });
        [
            self.next_script_timer_due(window),
            native,
            self.next_idle_timeout(window),
        ]
        .into_iter()
        .flatten()
        .reduce(f64::min)
    }

    /// When `window`'s next script timer (`setTimeout`'s, `setInterval`'s)
    /// is due, in milliseconds since its time origin; none with none.
    pub fn next_script_timer_due(&self, window: Gc<'gc, Window>) -> Option<f64> {
        window.timers.get(self.gc).and_then(|timers| {
            (0..timers.len())
                .filter_map(|index| timers.get(self.gc, index))
                .map(|timer| timer.due.get())
                .reduce(f64::min)
        })
    }

    /// `window`'s timer due by `now` (milliseconds since its time origin)
    /// to run next: the earliest due, of those the first made (HTML's
    /// order; the timers are in the order made), and the nesting level it
    /// runs at. A timeout is taken out; an
    /// interval stays, due again an interval after `now`, one level deeper
    /// (HTML's timer task, which initializes it again at its level, Blink's
    /// `DOMTimer::Fired`: past 5, at least 4 ms, so that a host running the
    /// timers due by one time ends). None if none is due.
    pub fn take_due(&self, window: Gc<'gc, Window>, now: f64) -> Option<(Gc<'gc, Timer>, u8)> {
        let timers = window.timers.get(self.gc)?;
        let (index, timer) = self.first_timer(window)?;
        if timer.due.get() > now {
            return None;
        }
        let running = timer.nesting.get();
        match timer.interval.get() {
            Some(interval) => {
                let interval = clamp_timeout(interval, running);
                timer.nesting.set(running.saturating_add(1));
                timer.interval.set(Some(interval));
                timer.due.set(now + interval);
                // Set again: a task numbered after those queued meanwhile.
                timer.number.set(self.next_task_number());
            }
            None => {
                timers.remove(self.gc, index);
            }
        }
        Some((timer, running))
    }

    /// `window`'s timer to run first, of those the host's pass lets run:
    /// the earliest due, ties by number (the order set; an interval's
    /// number is its latest).
    fn first_timer(&self, window: Gc<'gc, Window>) -> Option<(usize, Gc<'gc, Timer>)> {
        let timers = window.timers.get(self.gc)?;
        let mut first: Option<(usize, Gc<'gc, Timer>)> = None;
        for index in 0..timers.len() {
            let timer = timers.get(self.gc, index).expect("below the length");
            if !self.task_may_run(timer.number.get()) {
                continue;
            }
            let key = (timer.due.get(), timer.number.get());
            if first.is_none_or(|(_, best)| {
                key.0
                    .total_cmp(&best.due.get())
                    .then(key.1.cmp(&best.number.get()))
                    .is_lt()
            }) {
                first = Some((index, timer));
            }
        }
        first
    }

    /// `window`'s native timer to run first (`first_timer`'s order).
    fn first_native_timer(&self, window: Gc<'gc, Window>) -> Option<(usize, Gc<'gc, NativeTimer>)> {
        let timers = window.native_timers.get(self.gc)?;
        let mut first: Option<(usize, Gc<'gc, NativeTimer>)> = None;
        for index in 0..timers.len() {
            let timer = timers.get(self.gc, index).expect("below the length");
            if !self.task_may_run(timer.number.get()) {
                continue;
            }
            let key = (timer.due.get(), timer.number.get());
            if first.is_none_or(|(_, best)| {
                key.0
                    .total_cmp(&best.due.get())
                    .then(key.1.cmp(&best.number.get()))
                    .is_lt()
            }) {
                first = Some((index, timer));
            }
        }
        first
    }

    /// When `window`'s script timer to run first is due (since its time
    /// origin), and its number (`tasks`).
    pub(crate) fn next_due_timer(&self, window: Gc<'gc, Window>) -> Option<(f64, u64)> {
        self.first_timer(window)
            .map(|(_, timer)| (timer.due.get(), timer.number.get()))
    }

    /// The same of `window`'s native timers.
    pub(crate) fn next_due_native_timer(&self, window: Gc<'gc, Window>) -> Option<(f64, u64)> {
        self.first_native_timer(window)
            .map(|(_, timer)| (timer.due.get(), timer.number.get()))
    }

    /// When `window`'s first element task was queued (the DOM's clock), and
    /// its number, if the host's pass lets it run (`tasks`).
    pub(crate) fn next_element_task(&self, window: Gc<'gc, Window>) -> Option<(f64, u64)> {
        let task = window.element_tasks.get(self.gc)?.get(self.gc, 0)?;
        let number = task.number.get();
        self.task_may_run(number)
            .then(|| (task.queued.get(), number))
    }

    /// HTML's "the end"'s window `load`: a task of the DOM manipulation
    /// task source queued in `document`'s window, after the element tasks
    /// queued so far (`ElementTaskKind::WindowLoad`); none without one.
    pub fn queue_window_load(&self, document: NodeRef<'gc>) {
        let Some(window) = self.window_of(document) else {
            return;
        };
        self.push_element_task(window, document, crate::names::event::LOAD, WINDOW_LOAD, 0);
    }

    pub fn timer_state(&self, timer: Gc<'gc, Timer>) -> TimerState {
        TimerState {
            id: timer.id.get(),
            due: timer.due.get(),
            nesting: timer.nesting.get(),
            interval: timer.interval.get(),
        }
    }

    /// `timer`'s reference to its callback, and to its arguments' array,
    /// which only the JS engine reads and writes.
    pub fn timer_callback_edge(&self, timer: Gc<'gc, Timer>) -> &'gc JsEdge {
        &timer.into_ref().callback
    }

    pub fn timer_arguments_edge(&self, timer: Gc<'gc, Timer>) -> &'gc JsEdge {
        &timer.into_ref().arguments
    }

    /// The nesting level of the timer whose callback runs in `window` (0
    /// out of one); set as one runs and back after.
    pub fn timer_nesting(&self, window: Gc<'gc, Window>) -> u8 {
        window.nesting.get()
    }

    pub fn set_timer_nesting(&self, window: Gc<'gc, Window>, nesting: u8) {
        window.nesting.set(nesting);
    }

    /// Whether `window` reports an exception now (HTML's "in error
    /// reporting mode").
    pub fn reporting(&self, window: Gc<'gc, Window>) -> bool {
        window.reporting.get()
    }

    pub fn set_reporting(&self, window: Gc<'gc, Window>, reporting: bool) {
        window.reporting.set(reporting);
    }

    pub(crate) fn window_wrapper_edge(&self, window: Gc<'gc, Window>) -> &'gc JsEdge {
        &window.into_ref().wrapper
    }

    pub(crate) fn location_wrapper_edge(&self, location: Gc<'gc, Location>) -> &'gc JsEdge {
        &location.into_ref().wrapper
    }
}

/// A timer's timeout, made by a timer of nesting level `running` (0 out
/// of one): not negative, nor NaN; at least 4 ms past a level of 5.
fn clamp_timeout(timeout: f64, running: u8) -> f64 {
    let timeout = if timeout > 0.0 { timeout } else { 0.0 };
    if running > 5 {
        timeout.max(4.0)
    } else {
        timeout
    }
}

/// A document's data with nothing in it yet.
pub(crate) fn new_document_data<'gc>() -> DocumentDataInit<'gc> {
    DocumentDataInit {
        template_document: None,
        rare: None,
        window: None,
        relevant_window: None,
        url: LocalExternal::new(RefCell::new(DomString::new(""))),
        content_type: LocalExternal::new(RefCell::new(DomString::new(""))),
        character_set: LocalExternal::new(RefCell::new(DomString::new(""))),
        current_script: None,
        ready_state: Cell::new(ready_state_bits(DocumentReadyState::Complete)),
        ids: elise::External::new(std::cell::Cell::new(None)),
        hover: None,
        active: None,
        focused: None,
        implementation: None,
        style_names: None,
        adopted: None,
        sheet_changed_roots: None,
        changed_sheets: None,
        forms: None,
        load_delay: Cell::new(0),
        responsive_images: None,
        lazy_load_observer: None,
        auto_sizes_observer: None,
        canvas_fonts: elise::External::new(Cell::new(None)),
        frame_owner: None,
        discarded: Cell::new(false),
        focused_frame: None,
        live_ranges: None,
        node_iterators: None,
        selection: None,
        in_order_scripts: None,
        style_sheets: None,
        sheet_texts: elise::External::new(Cell::new(None)),
        media_query_lists: None,
        kept_media_query_lists: None,
        media_environment: elise::External::new(Cell::new(None)),
        animation_time: Cell::new(None),
        animated: None,
        animation_events: elise::External::new(RefCell::new(Vec::new())),
        animation_objects: None,
        kept_animation_objects: None,
        effect_objects: None,
        kept_effect_objects: None,
        timeline: None,
        detached_animations: elise::External::new(RefCell::new(Vec::new())),
        target_element: None,
        completely_loaded: Cell::new(false),
        base_url: LocalExternal::new(RefCell::new(None)),
        about_base_url: LocalExternal::new(RefCell::new(None)),
        content_language: LocalExternal::new(RefCell::new(None)),
        autofocus_candidates: elise::External::new(Cell::new(Vec::new())),
        autofocus_processed: Cell::new(false),
    }
}

fn ready_state_bits(state: DocumentReadyState) -> u8 {
    match state {
        DocumentReadyState::Loading => 0,
        DocumentReadyState::Interactive => 1,
        DocumentReadyState::Complete => 2,
    }
}
