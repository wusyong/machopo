//! A elise heap marking with V8's major GCs as cppgc's does in Chrome's
//! unified heap (Blink's `UnifiedHeapController` and `V8EmbedderRootsHandler`),
//! through the fork's `v8::machopo`: V8 starts the heap's marking, advances it
//! with each of its steps, asks whether it is done and ends it
//! ([`EmbedderHeap`], [`register_embedder_heap`]).
//!
//! As cppgc's objects: a wrapper holds its object's address, tagged by the
//! object's type ([`wrap`], [`unwrap`]); V8's marking threads hand the
//! object over as they mark the wrapper (`elise::WrapperLocal`); an object's
//! reference to its wrapper is V8's `TracedReference`, in the object (a
//! `elise::JsEdge`: [`set_wrapper`], [`wrapper`]), which the heap's marker
//! threads mark into V8's worklists through the sink this module gives the
//! heap's marking (`elise::JsEdgeSink`). The reference is droppable: V8's
//! scavenge drops an unmodified wrapper nothing else holds, and this
//! module's roots handler resets the reference
//! (`reclaim_unmodified_wrappers`).
//!
//! A heap snapshot (`HeapProfiler.takeHeapSnapshot`) has the heap's objects
//! too, as Blink's `V8EmbedderGraphBuilder` adds its DOM's: each object a
//! node named by its type, merged with its wrapper's, with edges to the
//! objects and JS values it refers to, and its detachedness as the heap
//! tells it ([`EmbedderHeap::snapshot`], [`HeapGraph`]).
//!
//! The `unsafe` is the objects' addresses and references, read from other
//! threads; the argument is in `crates/bindings/soundness.md`.

use std::collections::HashMap;
use std::ffi::c_void;
use std::mem::{align_of, size_of};
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, ThreadId};
use std::time::Duration;

use v8::machopo::{
    EmbedderGraphRegistration, EmbedderMarkingRegistration, RootHandle, RootsHandlerRegistration,
};

/// `T`'s wrappers' tag: the fork's first, plus the type's index. Every
/// call of a member reads it.
#[inline]
fn tag<T: elise::HeapType>() -> u16 {
    tag_of(elise::type_index::<T>())
}

/// The wrappers' tag of the type numbered `index` (`elise::type_index`): the
/// index is below `elise::MAX_TYPES`, which the fork's range holds (asserted
/// below), so masked it needs no check.
#[inline]
fn tag_of(index: u32) -> u16 {
    #[expect(clippy::cast_possible_truncation, reason = "MAX_TYPES - 1 is 4095")]
    const MASK: u32 = (elise::MAX_TYPES - 1) as u32;
    const _: () = assert!(
        elise::MAX_TYPES.is_power_of_two()
            && v8::machopo::FIRST_TAG as usize + elise::MAX_TYPES - 1
                == v8::machopo::LAST_TAG as usize
    );
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "masked below MAX_TYPES, which FIRST_TAG's range holds (asserted above)"
    )]
    let tag = v8::machopo::FIRST_TAG + (index & MASK) as u16;
    tag
}

/// The heap's side of a major GC's marking, on the isolate's thread: what
/// V8 calls at its marking's start, steps and end.
pub trait EmbedderHeap: 'static {
    /// A major GC's marking starts (`elise::Heap::start_marking`): the heap's
    /// marker gives `sink` the JS edges of what it marks, which V8 marks;
    /// V8's threads hand the wrapped objects over through what it returns
    /// (none if the heap is going).
    fn start(&self, sink: Arc<dyn elise::JsEdgeSink>) -> Option<elise::WrapperMarker>;
    /// Marks for up to `budget`, what V8's step leaves after cppgc's (all
    /// that is left in the `atomic` pause): whether nothing is left.
    fn advance(&self, budget: Duration, atomic: bool) -> bool;
    /// Both sides are done, in the pause (`elise::Heap::finish_marking`): the
    /// heap's used bytes after it, its live ones, which V8 sets its next
    /// limits from.
    fn end(&self) -> usize;
    /// A heap snapshot's turn to tell of the heap, after V8's GC: the
    /// heap's objects added to `graph` (`HeapGraph::add_heap`, with the
    /// heap's `elise::Heap::walk`). Nothing by default.
    fn snapshot(&self, graph: &mut HeapGraph<'_, '_, '_, '_>) {
        let _ = graph;
    }
}

/// The heap's marking as V8 calls it: the hand-over, for any thread, and
/// the heap and its isolate, for the isolate's.
struct Marking {
    /// Shared with the heap snapshot's builder (`Graph`), the other of
    /// V8's two registrations that call the heap; only on the isolate's
    /// thread, whatever reaches it.
    heap: Rc<dyn EmbedderHeap>,
    marker: Mutex<Option<elise::WrapperMarker>>,
    running: Mutex<Option<Running>>,
    isolate: IsolatePtr,
    thread: ThreadId,
}

// SAFETY: V8's marking threads reach only `marker` (behind a mutex; a
// `WrapperMarker` is `Send + Sync`) and the locals; `heap` only through
// `Marking::heap`, which checks it is on the isolate's thread, the one that
// made it, and its `Rc` is cloned and dropped only there (as the
// registration is made and dropped).
unsafe impl Sync for Marking {}

impl Marking {
    fn heap(&self) -> &dyn EmbedderHeap {
        on_thread(self.thread);
        &*self.heap
    }

    fn marker(&self) -> MutexGuard<'_, Option<elise::WrapperMarker>> {
        self.marker.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn running(&self) -> MutexGuard<'_, Option<Running>> {
        self.running.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Whether a marking V8 drives still runs, for the locals its threads and
/// the heap's take of it: false once V8 ended it. Checked in debug builds
/// only: a local used after its marking is the protocol broken, with no
/// symptom of its own until much later (an object handed over then is
/// never marked, and is freed while its wrapper lives; a reference marked
/// then lands in V8's next marking, and keeps garbage).
#[derive(Clone)]
struct Running(Arc<AtomicBool>);

impl Running {
    fn new() -> Self {
        Self(Arc::new(AtomicBool::new(true)))
    }

    fn end(&self) {
        // A check, not a synchronization: V8's end comes after every
        // thread's use, or the protocol is broken anyway.
        self.0.store(false, Ordering::Relaxed);
    }

    fn check(&self, what: &str) {
        debug_assert!(
            self.0.load(Ordering::Relaxed),
            "{what} after its marking ended"
        );
    }
}

/// A V8 marking thread's list of objects to hand over, and the marking it
/// was made in.
struct Local {
    local: elise::WrapperLocal,
    running: Running,
}

impl Drop for Local {
    fn drop(&mut self) {
        // What it still holds is published to a marking that is over.
        if !self.local.is_empty() && !thread::panicking() {
            self.running.check("a local holding objects goes");
        }
    }
}

thread_local! {
    /// This thread's id, read once: `thread::current()` counts a reference
    /// to the thread's handle each time, and V8's scavenge resets a root
    /// (`Roots::reset_root`) for each wrapper it drops.
    static THREAD: ThreadId = thread::current().id();
}

fn on_thread(thread: ThreadId) {
    assert_eq!(
        THREAD.with(|id| *id),
        thread,
        "V8 calls the embedder heap on the isolate's thread"
    );
}

impl v8::machopo::EmbedderMarking for Marking {
    type Local = Local;

    fn start(&self) {
        let running = Running::new();
        let marker = self.heap().start(Arc::new(TracedSink {
            isolate: self.isolate,
            running: running.clone(),
        }));
        *self.marker() = marker;
        *self.running() = Some(running);
    }

    fn advance(&self, max_duration_ms: f64, in_atomic_pause: bool) -> bool {
        self.heap()
            .advance(budget(max_duration_ms), in_atomic_pause)
    }

    fn is_done(&self) -> bool {
        on_thread(self.thread);
        drained(self.marker().as_ref())
    }

    fn end(&self) -> usize {
        let used = self.heap().end();
        *self.marker() = None;
        if let Some(running) = self.running().take() {
            running.end();
        }
        used
    }

    fn new_local(&self) -> Option<Local> {
        let local = self.marker().as_ref()?.local();
        let running = self.running().clone()?;
        Some(Local { local, running })
    }

    fn hand(local: &mut Local, object: NonNull<c_void>) {
        local.running.check("V8 hands an object over");
        // SAFETY: V8 hands over the object of a wrapper it marked with one
        // of the fork's tags, in the marking that made the local: only
        // `wrap` wraps with those, with the address of a live object of the
        // isolate's heap (`elise::Mutation::engine_address`), which the
        // wrapper keeps alive (an object of another heap the marker
        // ignores).
        unsafe { local.local.hand(object.cast()) };
    }

    fn publish(local: &mut Local) {
        if !local.local.is_empty() {
            local.running.check("V8 publishes objects");
        }
        local.local.publish();
    }

    fn is_local_empty(local: &Local) -> bool {
        local.local.is_empty()
    }
}

/// Whether the heap's marker has nothing left of what V8's threads handed
/// it and published (V8's parallel markers publish theirs as they finish,
/// in the pause, and V8 then asks this; its main thread's unpublished
/// local it counts itself); true if the heap doesn't mark.
fn drained(marker: Option<&elise::WrapperMarker>) -> bool {
    marker.is_none_or(elise::WrapperMarker::is_drained)
}

/// V8's step's time, in ms: NaN and negative times are none; infinite or
/// too long ones, no bound.
fn budget(max_ms: f64) -> Duration {
    Duration::try_from_secs_f64(max_ms / 1000.0).unwrap_or(if max_ms > 0.0 {
        Duration::MAX
    } else {
        Duration::ZERO
    })
}

/// Resets an object's reference to a wrapper V8's scavenge dropped: on V8's
/// threads in parallel (`try_reset_root`, as Blink's `BlinkRootsHandler`
/// does), or on the isolate's (`reset_root`).
struct Roots {
    thread: ThreadId,
}

impl Roots {
    /// Resets `object`'s JS edge that refers to what `handle` does: whether
    /// one did.
    ///
    /// # Safety
    ///
    /// In V8's scavenge, on the isolate's thread or in its parallel reset
    /// of `handle` (`RootHandle::reset_if_same`'s contract); `object` is
    /// what a wrapper V8 dropped held, with one of the fork's tags.
    unsafe fn reset(object: NonNull<c_void>, handle: &RootHandle) -> bool {
        // SAFETY: as the caller promises: an object of the isolate's heap
        // (`wrap`), alive (old; the heap frees it only in a major GC, which
        // frees its dropped wrapper too), not in an operation (V8 scavenges
        // between them, the isolate's thread waiting in the scavenge), whose
        // reference to the wrapper is one of its JS edges; its type's entry
        // read atomically, from any thread.
        let edges = unsafe { elise::js_edges_of(object.cast()) };
        edges.into_iter().any(|edge| {
            // SAFETY: a live object's JS edge, its `TracedReference`
            // (`set_wrapper`), written on the isolate's thread before the
            // scavenge began (V8's posting of its job orders that before
            // its threads' reads); reset as the caller promises.
            unsafe { handle.reset_if_same(slot(edge.as_ref())) }
        })
    }
}

impl v8::machopo::RootsHandler for Roots {
    fn reset_root(&self, object: NonNull<c_void>, handle: &RootHandle) {
        on_thread(self.thread);
        // SAFETY: on the isolate's thread, in its scavenge; V8's dropped
        // wrapper's object.
        unsafe { Self::reset(object, handle) };
    }

    fn try_reset_root(&self, object: NonNull<c_void>, handle: &RootHandle) -> bool {
        // SAFETY: in V8's parallel reset of `handle`; V8's dropped wrapper's
        // object. A reference to it is one of the object's JS edges (V8
        // found it through the object's wrapper), so one is reset.
        unsafe { Self::reset(object, handle) }
    }
}

/// A heap registered with an isolate's `CppHeap`, and its roots handler:
/// unregistered when dropped, which must be before the isolate is. A GC V8
/// runs then is finished first, with the heap (its marking ends as in any
/// GC: the heap must still be whole, as `EmbedderHeap` holds it), as
/// Chrome finishes its GC before it detaches a `CppHeap`.
pub struct EmbedderRegistration {
    // In this order: the snapshots' builder goes, the marking finishes the
    // GC that runs, then the roots handler goes.
    _graph: EmbedderGraphRegistration,
    _marking: EmbedderMarkingRegistration,
    _roots: RootsHandlerRegistration,
}

/// Registers `heap` as the isolate's embedder heap, which marks with its
/// major GCs from the next one on, the roots handler that resets its
/// objects' references to the wrappers V8's scavenges drop, and the builder
/// of its part of the isolate's heap snapshots.
///
/// # Safety
///
/// The registration is dropped on the isolate's thread, before the isolate
/// is.
///
/// # Panics
///
/// If the isolate has no `CppHeap`.
pub unsafe fn register_embedder_heap(
    isolate: &mut v8::Isolate,
    heap: Box<dyn EmbedderHeap>,
) -> EmbedderRegistration {
    let thread = thread::current().id();
    let heap: Rc<dyn EmbedderHeap> = Rc::from(heap);
    let marking = Box::new(Marking {
        heap: heap.clone(),
        marker: Mutex::new(None),
        running: Mutex::new(None),
        isolate: IsolatePtr::new(isolate),
        thread,
    });
    // SAFETY: all dropped with the registration, as the caller promises.
    unsafe {
        EmbedderRegistration {
            _graph: v8::machopo::add_build_embedder_graph_callback(
                isolate,
                Box::new(Graph { heap, thread }),
            ),
            _marking: v8::machopo::set_embedder_marking(isolate, marking),
            _roots: v8::machopo::set_roots_handler(isolate, Box::new(Roots { thread })),
        }
    }
}

/// An isolate's state that is its embedder heap: V8's GC reaches it through
/// the isolate's slot (`set_isolate_state_with_embedder_heap`), as `&mut`,
/// on the isolate's thread, from a call that took the isolate whole (as it
/// allocates or steps), so when no borrow of it is held.
pub trait EmbedderHeapState: 'static {
    /// As `EmbedderHeap::start`.
    fn start(&mut self, sink: Arc<dyn elise::JsEdgeSink>) -> Option<elise::WrapperMarker>;
    /// As `EmbedderHeap::advance`.
    fn advance(&mut self, budget: Duration, atomic: bool) -> bool;
    /// As `EmbedderHeap::end`.
    fn end(&mut self) -> usize;
    /// As `EmbedderHeap::snapshot`.
    fn snapshot(&mut self, graph: &mut HeapGraph<'_, '_, '_, '_>) {
        let _ = graph;
    }
}

/// The embedder heap that is the isolate's state, a `T`, reached through
/// its slot.
struct StateHeap<T> {
    isolate: IsolatePtr,
    _state: std::marker::PhantomData<fn() -> T>,
}

impl<T: EmbedderHeapState> StateHeap<T> {
    #[expect(
        clippy::mut_from_ref,
        reason = "the state isn't `self`'s: the isolate's, which no other borrow holds during V8's call (the \
                  safety comment)"
    )]
    fn state(&self) -> &mut T {
        // SAFETY: on the isolate's thread (`Marking::heap` and
        // `Graph::build` check), which lives while its registration does
        // (dropped before the isolate).
        let isolate = unsafe { v8::Isolate::from_raw_isolate_ptr_unchecked(self.isolate.0) };
        // SAFETY: the state is a `T` from its registration on (set first)
        // until it is dropped (after the registration,
        // `drop_isolate_state`). V8 calls the embedder heap only from a
        // call that took the isolate whole (an allocation, a step, a report
        // of the used size, a heap snapshot, the registration's drop), and
        // never from its own call to it (a snapshot's builder calls V8
        // only for what neither allocates nor calls back): no borrow of the
        // state is held then (`isolate_state_mut` and `StateScope` each
        // hold the isolate, or a scope of it, as long as theirs), and this
        // one ends with the call.
        unsafe { crate::isolate_state::unchecked::<T>(&isolate).as_mut() }
    }
}

impl<T: EmbedderHeapState> EmbedderHeap for StateHeap<T> {
    fn start(&self, sink: Arc<dyn elise::JsEdgeSink>) -> Option<elise::WrapperMarker> {
        self.state().start(sink)
    }

    fn advance(&self, budget: Duration, atomic: bool) -> bool {
        self.state().advance(budget, atomic)
    }

    fn end(&self) -> usize {
        self.state().end()
    }

    fn snapshot(&self, graph: &mut HeapGraph<'_, '_, '_, '_>) {
        self.state().snapshot(graph);
    }
}

/// Gives `isolate` the embedder's state (`set_isolate_state`), which is its
/// embedder heap (`register_embedder_heap`, reaching it through the slot):
/// the registration goes with the state, before it, which only the
/// isolate's owner drops, as the isolate goes (`drop_isolate_state`, which
/// `V8PerIsolateData`'s drop calls), or never. What a binding without
/// `unsafe` registers a heap with.
///
/// # Panics
///
/// If the isolate has a state already, or no `CppHeap`.
pub fn set_isolate_state_with_embedder_heap<T: EmbedderHeapState>(
    isolate: &mut v8::Isolate,
    state: T,
) {
    // Before the registration: one made then dropped (the state can't be
    // set) would unset the isolate's heap, whoever's.
    assert!(
        !crate::isolate_state::has_isolate_state(isolate),
        "an isolate's state is set once"
    );
    crate::set_isolate_state(isolate, state);
    let heap = StateHeap::<T> {
        isolate: IsolatePtr::new(isolate),
        _state: std::marker::PhantomData,
    };
    // SAFETY: the registration is kept in the isolate's state's slot, which
    // only `drop_isolate_state` drops: on the isolate's thread, before the
    // isolate goes (its contract), as the registration needs. Never
    // dropped (the isolate's owner never calls it), the registration leaks
    // with the state, and the boxes V8 points to with it.
    let registration = unsafe { register_embedder_heap(isolate, Box::new(heap)) };
    crate::isolate_state::keep_registration(isolate, registration);
}

/// Whether an object of the heap is part of the page, for a heap snapshot
/// (V8's `EmbedderGraph::Node::Detachedness`): DevTools' Memory panel marks
/// the detached ones (Blink: a node out of every document's tree).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Detachedness {
    #[default]
    Unknown,
    Attached,
    Detached,
}

impl From<Detachedness> for v8::machopo::Detachedness {
    fn from(detachedness: Detachedness) -> Self {
        match detachedness {
            Detachedness::Unknown => Self::Unknown,
            Detachedness::Attached => Self::Attached,
            Detachedness::Detached => Self::Detached,
        }
    }
}

/// What the snapshot calls the node standing for the heap's roots
/// (`Persistent`s), which holds what they hold (as Chrome's cppgc graph
/// builder's "C++ Persistent roots").
const ROOTS: &str = "(Persistent roots)";

/// A heap snapshot's graph, as the embedder heap is told of it
/// (`EmbedderHeap::snapshot`), with a handle scope of the isolate's: V8's
/// GC can't run while it is built (the snapshot holds it off), and
/// nothing here allocates in V8's heap.
pub struct HeapGraph<'a, 's, 'i, 'g> {
    scope: &'a mut v8::PinScope<'s, 'i, ()>,
    graph: &'a mut v8::machopo::EmbedderGraph<'g>,
}

impl<'s, 'g> HeapGraph<'_, 's, '_, 'g> {
    /// Adds the heap's objects, from its walk (of the isolate's embedder
    /// heap), as Blink's `V8EmbedderGraphBuilder` adds its DOM's: a node
    /// for each, named by its type and sized as the heap counts it, merged
    /// with its wrapper's (V8's `WrapperNode`: the snapshot shows the two
    /// as one); an edge for each of its edges to another, named by its
    /// field (weak for a weak one; an ephemeron's value held by its key);
    /// an edge to each JS value its JS edges hold but its wrapper; a root
    /// node holding what the heap's roots hold. Each object's detachedness
    /// is what `detachedness` answers.
    pub fn add_heap<'gc>(
        &mut self,
        walk: &elise::Walk<'gc>,
        mut detachedness: impl FnMut(elise::HeapObject<'gc>) -> Detachedness,
    ) {
        let mut nodes = HashMap::with_capacity(walk.len());
        for object in walk.objects() {
            let wrapper =
                wrapper_of(self.scope, object).map(|wrapper| self.graph.v8_node(wrapper.into()));
            let node = self.graph.add_node(v8::machopo::EmbedderNode {
                name: object.type_name(),
                size: object.size(),
                address: Some(object.address().cast()),
                detachedness: detachedness(object).into(),
                wrapper,
                is_root: false,
            });
            nodes.insert(object.address(), node);
        }
        // Every edge's object is one of the walk's (what the heap holds).
        let node = |object: elise::HeapObject<'gc>| nodes.get(&object.address()).copied();
        for object in walk.objects() {
            let Some(from) = node(object) else {
                continue;
            };
            for edge in object.edges() {
                let Some(to) = node(edge.to) else {
                    continue;
                };
                match edge.kind {
                    elise::EdgeKind::Strong => self.graph.add_edge(from, to, edge.name),
                    elise::EdgeKind::Weak => self.graph.add_weak_edge(from, to, edge.name),
                    elise::EdgeKind::Ephemeron { key } => {
                        self.graph.add_weak_edge(from, to, edge.name);
                        if let Some(key) = node(key) {
                            self.graph.add_edge(key, to, edge.name);
                        }
                    }
                }
            }
            let wrapper = wrapper_of(self.scope, object);
            for (name, edge) in object.js_edges() {
                let Some(value) = strong_value(self.scope, edge) else {
                    continue;
                };
                // The wrapper is the object's node itself.
                if wrapper.is_some_and(|wrapper| wrapper == value) {
                    continue;
                }
                let to = self.graph.v8_node(value);
                self.graph.add_edge(from, to, Some(name));
            }
        }
        let mut roots = walk.roots().filter_map(node).peekable();
        if roots.peek().is_some() {
            let root = self.graph.add_node(v8::machopo::EmbedderNode {
                name: ROOTS,
                size: 0,
                address: None,
                detachedness: v8::machopo::Detachedness::Unknown,
                wrapper: None,
                is_root: true,
            });
            for to in roots {
                self.graph.add_edge(root, to, None);
            }
        }
    }
}

/// `object`'s wrapper: the object one of its JS edges holds that wraps it
/// (`wrap`).
fn wrapper_of<'s>(
    scope: &mut v8::PinScope<'s, '_, ()>,
    object: elise::HeapObject<'_>,
) -> Option<v8::Local<'s, v8::Object>> {
    let tag = tag_of(object.type_index());
    object.js_edges().find_map(|(_, edge)| {
        let value = v8::Local::<v8::Object>::try_from(strong_value(scope, edge)?).ok()?;
        let wrapped = v8::machopo::unwrap(scope, value, tag)?;
        (wrapped.cast() == object.address()).then_some(value)
    })
}

/// Builds the heap's part of a heap snapshot (V8's
/// `BuildEmbedderGraphCallback`, Blink's `V8EmbedderGraphBuilder`): the
/// embedder heap's `snapshot`, on the isolate's thread.
struct Graph {
    heap: Rc<dyn EmbedderHeap>,
    thread: ThreadId,
}

impl v8::machopo::BuildEmbedderGraph for Graph {
    fn build(&self, isolate: &mut v8::Isolate, graph: &mut v8::machopo::EmbedderGraph<'_>) {
        on_thread(self.thread);
        v8::scope!(let scope, isolate);
        self.heap.snapshot(&mut HeapGraph { scope, graph });
    }
}

/// A callback's result as a holder of the isolate's state may write it:
/// the setters of V8's `ReturnValue` that allocate nothing (a handle's
/// value, the oddballs, a Smi). Its others need no scope yet allocate a
/// heap number (`set_double`, an integer past a Smi), which V8's GC may
/// follow: out of the state's reach.
pub struct OpReturn<'o, 'r> {
    rv: &'o mut v8::ReturnValue<'r, v8::Value>,
}

/// A callback's result, as its front end keeps it across its operation:
/// whole only to who holds the isolate (`full`), which no holder of its
/// state does; else as `OpReturn`.
pub struct ReturnSlot<'r> {
    rv: v8::ReturnValue<'r, v8::Value>,
}

impl<'r> ReturnSlot<'r> {
    pub fn new(rv: v8::ReturnValue<'r, v8::Value>) -> Self {
        Self { rv }
    }

    /// The result whole (its setters may allocate), for one holding the
    /// isolate: a borrow of the state holds it (`StateScope`), so not in
    /// an operation.
    #[inline]
    pub fn full<'a>(
        &'a mut self,
        _isolate: &mut v8::Isolate,
    ) -> &'a mut v8::ReturnValue<'r, v8::Value> {
        &mut self.rv
    }

    /// The result by its setters that allocate nothing.
    #[inline]
    pub fn op(&mut self) -> OpReturn<'_, 'r> {
        OpReturn { rv: &mut self.rv }
    }
}

/// The largest integer a Smi holds (31 bits with pointer compression, which
/// machopo's V8 has on every platform).
const SMI_MAX: u32 = (1 << 30) - 1;

impl<'o, 'r> OpReturn<'o, 'r> {
    pub fn new(rv: &'o mut v8::ReturnValue<'r, v8::Value>) -> Self {
        Self { rv }
    }

    /// The value of a handle.
    #[inline]
    pub fn set(&mut self, value: v8::Local<'_, v8::Value>) {
        self.rv.set(value);
    }

    #[inline]
    pub fn set_bool(&mut self, value: bool) {
        self.rv.set_bool(value);
    }

    #[inline]
    pub fn set_null(&mut self) {
        self.rv.set_null();
    }

    #[inline]
    pub fn set_undefined(&mut self) {
        self.rv.set_undefined();
    }

    /// V8's empty string: one of its roots (a constant with pointer
    /// compression's static roots), no string made.
    #[inline]
    pub fn set_empty_string(&mut self) {
        self.rv.set_empty_string();
    }

    /// `value` as a Smi, if one holds it: whether it did.
    #[inline]
    pub fn set_smi_u32(&mut self, value: u32) -> bool {
        if value > SMI_MAX {
            return false;
        }
        self.rv.set_uint32(value);
        true
    }

    /// `value` as a Smi, if one holds it: whether it did.
    #[inline]
    pub fn set_smi_i32(&mut self, value: i32) -> bool {
        if value.unsigned_abs() > SMI_MAX {
            return false;
        }
        self.rv.set_int32(value);
        true
    }
}

/// What a holder of the isolate's state does with V8 (`NoGc`): reading and
/// writing what its embedder heap's objects refer to, and returning values
/// from handles; none allocates in V8's heap or calls back, so V8's GC
/// (which reaches the state) can't run meanwhile. Each is the module's
/// function of the same name; the argument for each is in
/// `crates/bindings/soundness.md`.
impl<'s> crate::NoGc<'_, 's, '_> {
    /// `unwrap_object`: reads the wrapper's embedder field.
    #[inline]
    pub fn unwrap<'gc, T: elise::HeapType>(
        &mut self,
        cx: &elise::Mutation<'gc>,
        object: v8::Local<'_, v8::Object>,
    ) -> Option<elise::Gc<'gc, T>> {
        unwrap_object(self.scope, cx, object)
    }

    /// `unwrap_receiver`: reads a checked receiver's embedder field.
    #[inline]
    pub fn unwrap_receiver<'gc, T: elise::HeapType>(
        &mut self,
        cx: &elise::Mutation<'gc>,
        this: crate::Receiver<'_>,
    ) -> Option<elise::Gc<'gc, T>> {
        unwrap_receiver(self.scope, cx, this)
    }

    /// `wrap`: writes the wrapper's embedder field (and the fork's pointer
    /// table's entry).
    #[inline]
    pub fn wrap<'gc, T: elise::HeapType>(
        &mut self,
        cx: &elise::Mutation<'gc>,
        wrapper: v8::Local<'_, v8::Object>,
        object: elise::Gc<'gc, T>,
    ) {
        wrap(self.scope, cx, wrapper, object);
    }

    /// `wrapper`: a local handle of what the edge refers to.
    #[inline]
    pub fn wrapper(&mut self, edge: &elise::JsEdge) -> Option<v8::Local<'s, v8::Object>> {
        wrapper(self.scope, edge)
    }

    /// `strong_value`.
    #[inline]
    pub fn strong_value(&mut self, edge: &elise::JsEdge) -> Option<v8::Local<'s, v8::Value>> {
        strong_value(self.scope, edge)
    }

    /// `strong_resolver`.
    #[inline]
    pub fn strong_resolver(
        &mut self,
        edge: &elise::JsEdge,
    ) -> Option<v8::Local<'s, v8::PromiseResolver>> {
        strong_resolver(self.scope, edge)
    }

    /// `set_wrapper`: writes the edge's traced handle.
    #[inline]
    pub fn set_wrapper(&mut self, edge: &elise::JsEdge, wrapper: v8::Local<'s, v8::Object>) {
        set_wrapper(self.scope, edge, wrapper);
    }

    /// `set_strong`.
    #[inline]
    pub fn set_strong(&mut self, edge: &elise::JsEdge, object: v8::Local<'s, v8::Object>) {
        set_strong(self.scope, edge, object);
    }

    /// `set_strong_value`.
    #[inline]
    pub fn set_strong_value(&mut self, edge: &elise::JsEdge, value: v8::Local<'s, v8::Value>) {
        set_strong_value(self.scope, edge, value);
    }

    /// `return_wrapper`: the callback's result from the edge's traced
    /// handle, no local handle made.
    #[inline]
    pub fn return_wrapper(&mut self, rv: &mut OpReturn<'_, '_>, edge: &elise::JsEdge) -> bool {
        return_wrapper(self.scope, rv.rv, edge)
    }

    /// The callback's result from a global handle, no local handle made.
    #[inline]
    pub fn return_global<T>(&mut self, rv: &mut OpReturn<'_, '_>, global: &v8::Global<T>)
    where
        for<'a> v8::Local<'a, T>: Into<v8::Local<'a, v8::Value>>,
    {
        rv.rv.set_global(self.scope, global);
    }

    /// A local handle of a global one's value (a cached template, a name's
    /// string).
    #[inline]
    pub fn local<T>(&mut self, global: &v8::Global<T>) -> v8::Local<'s, T> {
        v8::Local::new(self.scope, global)
    }

    /// A local handle of a weak one's value, if it still has it (a cached
    /// string).
    #[inline]
    pub fn weak_local<T>(&mut self, weak: &v8::Weak<T>) -> Option<v8::Local<'s, T>> {
        weak.to_local(self.scope)
    }

    /// A string's ASCII characters as V8 lends them in place
    /// (`LentString::ascii`): no copy, valid while no GC runs.
    #[inline]
    pub fn ascii<'a>(&mut self, string: &'a crate::LentString<'_>) -> Option<&'a str> {
        string.ascii(self.scope)
    }

    /// The callback's result from a weak handle, if it still has its value.
    #[inline]
    pub fn return_weak<T>(&mut self, rv: &mut OpReturn<'_, '_>, weak: &v8::Weak<T>) -> bool
    where
        for<'a> v8::Local<'a, T>: Into<v8::Local<'a, v8::Value>>,
    {
        rv.rv.set_weak(self.scope, weak)
    }
}

/// What a holder of the isolate's state borrowed from a callback's info
/// does with V8 (`InfoNoGc`): `NoGc`'s reads that need the isolate alone
/// (the same functions, the same arguments in
/// `crates/bindings/soundness.md`), and the callback's result.
impl<'s> crate::InfoNoGc<'_, 's> {
    /// `unwrap_object`: reads the wrapper's embedder field.
    #[inline]
    pub fn unwrap<'gc, T: elise::HeapType>(
        &mut self,
        cx: &elise::Mutation<'gc>,
        object: v8::Local<'_, v8::Object>,
    ) -> Option<elise::Gc<'gc, T>> {
        unwrap_object(self.isolate, cx, object)
    }

    /// `unwrap_receiver`: reads a checked receiver's embedder field.
    #[inline]
    pub fn unwrap_receiver<'gc, T: elise::HeapType>(
        &mut self,
        cx: &elise::Mutation<'gc>,
        this: crate::Receiver<'_>,
    ) -> Option<elise::Gc<'gc, T>> {
        unwrap_receiver(self.isolate, cx, this)
    }

    /// `wrap`: writes the wrapper's embedder field (and the fork's pointer
    /// table's entry).
    #[inline]
    pub fn wrap<'gc, T: elise::HeapType>(
        &mut self,
        cx: &elise::Mutation<'gc>,
        wrapper: v8::Local<'_, v8::Object>,
        object: elise::Gc<'gc, T>,
    ) {
        wrap(self.isolate, cx, wrapper, object);
    }

    /// `set_wrapper`: writes the edge's traced handle.
    #[inline]
    pub fn set_wrapper(&mut self, edge: &elise::JsEdge, wrapper: v8::Local<'s, v8::Object>) {
        set_wrapper(self.isolate, edge, wrapper);
    }

    /// `set_strong`.
    #[inline]
    pub fn set_strong(&mut self, edge: &elise::JsEdge, object: v8::Local<'s, v8::Object>) {
        set_strong(self.isolate, edge, object);
    }

    /// `set_strong_value`.
    #[inline]
    pub fn set_strong_value(&mut self, edge: &elise::JsEdge, value: v8::Local<'s, v8::Value>) {
        set_strong_value(self.isolate, edge, value);
    }

    /// `refers_to`: reads the edge's traced handle's object.
    #[inline]
    pub fn refers_to(&mut self, edge: &elise::JsEdge, value: v8::Local<'s, v8::Value>) -> bool {
        refers_to(edge, value)
    }

    /// The callback's result, as a holder of the state may set it.
    #[inline]
    pub fn ret(&mut self) -> OpReturn<'_, 's> {
        OpReturn { rv: &mut self.rv }
    }

    /// `return_wrapper`: the callback's result from the edge's traced
    /// handle, no local handle made.
    #[inline]
    pub fn return_wrapper(&mut self, edge: &elise::JsEdge) -> bool {
        return_wrapper(self.isolate, &mut self.rv, edge)
    }

    /// A local handle of a global one's value (a cached boilerplate), with
    /// no scope made: a handle of the callback's own handle scope
    /// (`HandleScope::Extend`'s block isn't V8's heap: no GC).
    #[inline(always)]
    pub fn local<T>(&mut self, global: &v8::Global<T>) -> v8::Local<'s, T> {
        // SAFETY: in a callback of V8's on the isolate's thread (an
        // `InfoNoGc` is of a callback's info); the handle has the info's
        // lifetime.
        unsafe { v8::Local::new_in_callback(self.isolate, global) }
    }

    /// The current context's global object (its global proxy), its handle
    /// left in the callback's own handle scope: the current realm of a
    /// callback with no receiver to find it by (Blink's callbacks'
    /// `GetCurrentContext()`).
    #[inline]
    pub fn current_global(&mut self) -> v8::Local<'s, v8::Object> {
        // SAFETY: on the isolate's thread, in the callback the info is
        // of: V8 entered its function's context and opened its handle
        // scope before calling it, and `'s` is the info's.
        unsafe { v8::Context::current_global(self.isolate) }
    }

    /// `strong_value`: a local handle of what the edge refers to, left in
    /// the callback's own handle scope (no scope made: V8 allocates a
    /// handle's slot, not on its heap).
    #[inline]
    pub fn strong_value(&mut self, edge: &elise::JsEdge) -> Option<v8::Local<'s, v8::Value>> {
        // SAFETY: on the isolate's thread, the heap's (the only one that
        // writes the edge), in the callback the info is of, whose handle
        // scope V8 opened before calling it; `'s` is the info's.
        unsafe { value_slot(edge).get_in_callback(self.isolate) }
    }

    /// As `return_wrapper`, of an edge of any value (`set_strong_value`:
    /// a listener's callback, an event's `detail`).
    #[inline]
    pub fn return_value(&mut self, edge: &elise::JsEdge) -> bool {
        // SAFETY: on the isolate's thread, the heap's (the only one that
        // writes the edge), as `return_wrapper`'s.
        unsafe { self.rv.set_traced(self.isolate, value_slot(edge)) }
    }

    /// The callback's result from a global handle, no local handle made.
    #[inline]
    pub fn return_global<T>(&mut self, global: &v8::Global<T>)
    where
        for<'a> v8::Local<'a, T>: Into<v8::Local<'a, v8::Value>>,
    {
        self.rv.set_global(self.isolate, global);
    }

    /// The callback's result from a weak handle, if it still has its value.
    #[inline]
    pub fn return_weak<T>(&mut self, weak: &v8::Weak<T>) -> bool
    where
        for<'a> v8::Local<'a, T>: Into<v8::Local<'a, v8::Value>>,
    {
        self.rv.set_weak(self.isolate, weak)
    }

    /// A string's ASCII characters as V8 lends them in place
    /// (`LentString::ascii`): no copy, valid while no GC runs.
    #[inline]
    pub fn ascii<'a>(&mut self, string: &'a crate::LentString<'_>) -> Option<&'a str> {
        string.ascii(self.isolate)
    }
}

/// Tells V8 the embedder heap's used bytes are `used` now: it counts them
/// with cppgc's, and on growth may start, advance or finish a major GC now
/// (calling the embedder heap), so only between its operations. A major
/// GC's end sets them itself (`EmbedderHeap::end`).
pub fn report_embedder_used_size(isolate: &mut v8::Isolate, used: usize) {
    v8::machopo::report_embedder_used_size(isolate, used);
}

/// Wraps `wrapper` with `object` of the isolate's embedder heap (in an
/// operation of it, `cx`), tagged by `T`: V8 hands the object over as it
/// marks the wrapper, which keeps it alive.
///
/// # Panics
///
/// Unless `wrapper` is an API wrapper (an object of a `FunctionTemplate`'s)
/// wrapping nothing yet; or as `elise::Mutation::engine_address` (the heap
/// isn't driven, or a `T` has no JS edge).
pub fn wrap<'gc, T: elise::HeapType>(
    isolate: &mut v8::Isolate,
    cx: &elise::Mutation<'gc>,
    wrapper: v8::Local<'_, v8::Object>,
    object: elise::Gc<'gc, T>,
) {
    assert!(
        wrapper.is_api_wrapper() && !v8::Object::is_wrapping(isolate, wrapper),
        "a wrapper of the heap's is an API wrapper wrapping nothing yet"
    );
    let address = cx.engine_address(object);
    // SAFETY: the address of a live `T` of the heap, which doesn't move
    // (old, as a type with a JS edge is in a driven heap) and lives while
    // the wrapper does (V8 hands it over as it marks the wrapper), as the
    // embedder marking (`Marking::hand`) takes it.
    unsafe { v8::machopo::wrap(isolate, wrapper, address.cast(), tag::<T>()) };
}

/// The `T` `value` wraps, if it is a wrapper of one (`wrap`), read in an
/// operation of the isolate's embedder heap, `cx`; marked if a marking
/// runs (`elise::Mutation::from_engine`).
///
/// # Panics
///
/// If `cx` is another heap's (the object isn't in its cage).
pub fn unwrap<'gc, T: elise::HeapType>(
    isolate: &mut v8::Isolate,
    cx: &elise::Mutation<'gc>,
    value: v8::Local<'_, v8::Value>,
) -> Option<elise::Gc<'gc, T>> {
    unwrap_object(isolate, cx, v8::Local::<v8::Object>::try_from(value).ok()?)
}

/// `unwrap` of what is an object already (a receiver): no check that it
/// is one.
pub fn unwrap_object<'gc, T: elise::HeapType>(
    isolate: &mut v8::Isolate,
    cx: &elise::Mutation<'gc>,
    object: v8::Local<'_, v8::Object>,
) -> Option<elise::Gc<'gc, T>> {
    let address = v8::machopo::unwrap(isolate, object, tag::<T>())?;
    // SAFETY: only `wrap` wraps with `T`'s tag, with the address of a live
    // `T` of the isolate's heap, which the wrapper (alive: script holds it)
    // keeps alive; `from_engine` checks it is `cx`'s heap.
    Some(unsafe { cx.from_engine::<T>(address.cast()) })
}

/// `unwrap_object` of a call's receiver that V8 checked is an object of
/// one of the isolate's templates (`Receiver`), so an API wrapper, whose
/// map isn't read again (V8's header's `Object::Unwrap` doesn't either).
/// The wrapper's tag is still compared: one wrapping nothing yet, or
/// another type's object, is `None`, as `unwrap_object` says.
///
/// # Panics
///
/// As `unwrap_object`.
#[inline]
pub fn unwrap_receiver<'gc, T: elise::HeapType>(
    isolate: &mut v8::Isolate,
    cx: &elise::Mutation<'gc>,
    this: crate::Receiver<'_>,
) -> Option<elise::Gc<'gc, T>> {
    if cfg!(debug_assertions) {
        RECEIVER_UNWRAPS.set(RECEIVER_UNWRAPS.get().wrapping_add(1));
    }
    // SAFETY: an API wrapper, as a `Receiver` is (made only where V8
    // checked it); this V8 has no sandbox (the fork's function exists only
    // without it).
    let address = unsafe { v8::machopo::unwrap_receiver(isolate, this.object(), tag::<T>()) }?;
    // SAFETY: as `unwrap_object`'s: only `wrap` wraps with `T`'s tag, with
    // the address of a live `T` of the isolate's heap, which the wrapper
    // keeps alive; `from_engine` checks it is `cx`'s heap.
    Some(unsafe { cx.from_engine::<T>(address.cast()) })
}

thread_local! {
    /// The receivers unwrapped as V8 checked them (`unwrap_receiver`), on
    /// this thread, for the tests. Counted in debug builds only.
    static RECEIVER_UNWRAPS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// The receivers this thread unwrapped as V8 checked them
/// (`unwrap_receiver`): what tells a member's call skips the map's check.
/// `None` in release builds. For tests.
pub fn receiver_unwraps_for_testing() -> Option<u64> {
    cfg!(debug_assertions).then(|| RECEIVER_UNWRAPS.get())
}

/// Whether `value` is a wrapper of a `T` (`wrap`): what `unwrap` reads, but
/// without an operation of the heap, for a binding to check its arguments
/// before it runs one.
pub fn is_wrapper<T: elise::HeapType>(
    isolate: &mut v8::Isolate,
    value: v8::Local<'_, v8::Value>,
) -> bool {
    v8::Local::<v8::Object>::try_from(value)
        .is_ok_and(|object| v8::machopo::unwrap(isolate, object, tag::<T>()).is_some())
}

type Slot = v8::cppgc::TracedSlot<v8::Object>;

// A JS edge is the C++ `TracedReference` it holds, alone: 8 bytes, the size
// of a `TracedSlot` (whose one non-zero-sized field is all of it).
const _: () = assert!(size_of::<Slot>() == size_of::<elise::JsEdge>());
const _: () = assert!(align_of::<Slot>() <= align_of::<elise::JsEdge>());

/// `edge` as the `TracedReference` it holds.
fn slot(edge: &elise::JsEdge) -> &Slot {
    // SAFETY: a JS edge's 8 aligned bytes, made empty (0, an empty
    // `TracedReference`) and then only written by this module; it never
    // moves (its object is old). A `TracedSlot` is those bytes alone
    // (asserted above), with interior mutability, as the edge.
    unsafe { &*edge.as_ptr().cast::<Slot>() }
}

/// `edge`'s bytes as a slot of any value: the same C++ object
/// (`TracedReference<Value>`), typed by what `set_strong_value` stores.
fn value_slot(edge: &elise::JsEdge) -> &v8::cppgc::TracedSlot<v8::Value> {
    // SAFETY: as `slot`'s; `TracedSlot`'s type parameter is a phantom, so
    // the layout is `Slot`'s (asserted above).
    unsafe { &*edge.as_ptr().cast::<v8::cppgc::TracedSlot<v8::Value>>() }
}

/// Points `edge` (an object's of the isolate's embedder heap, in one of its
/// operations) at `wrapper`, droppable: V8's scavenge may drop the wrapper
/// if no one else holds it and it is unmodified, and resets `edge`. V8
/// marks the wrapper at once while marking.
pub fn set_wrapper(
    isolate: &v8::Isolate,
    edge: &elise::JsEdge,
    wrapper: v8::Local<'_, v8::Object>,
) {
    // SAFETY: on the isolate's thread, the heap's (the only one that
    // writes the edge); the edge is in its final place (its object is old
    // and never moves).
    unsafe { slot(edge).set_droppable(isolate, wrapper) };
}

/// Points `edge` (an object's of the isolate's embedder heap, in one of its
/// operations) at `object`, strongly (not droppable): V8's scavenges keep
/// it, as what it refers to may be held by nothing else (a listener's
/// callback, Blink's `TraceWrapperV8Reference`); V8's major GCs keep it
/// while the edge's object lives. V8 marks it at once while marking.
pub fn set_strong(isolate: &v8::Isolate, edge: &elise::JsEdge, object: v8::Local<'_, v8::Object>) {
    // SAFETY: as `set_wrapper`'s: on the isolate's thread, the heap's (the
    // only one that writes the edge); the edge is in its final place (its
    // object is old and never moves).
    unsafe { slot(edge).set(isolate, Some(object)) };
}

/// Whether `edge` refers to the object `value` is a handle of (the same
/// one), read from the edge's traced handle with no local made of it.
#[inline]
pub fn refers_to(edge: &elise::JsEdge, value: v8::Local<'_, v8::Value>) -> bool {
    // SAFETY: as `wrapper`'s: on the isolate's thread, the heap's.
    unsafe { value_slot(edge).refers_to(value) }
}

/// What a strong edge refers to (`set_strong`), if anything: read as a
/// wrapper's is.
pub fn strong<'s>(
    scope: &v8::PinScope<'s, '_, ()>,
    edge: &elise::JsEdge,
) -> Option<v8::Local<'s, v8::Object>> {
    wrapper(scope, edge)
}

/// Points `edge` (an object's of the isolate's embedder heap, in one of its
/// operations) at `value`, any JS value, strongly, as `set_strong` does an
/// object (an error event's error, a timer's arguments: Blink's
/// `TraceWrapperV8Reference<v8::Value>`).
pub fn set_strong_value(
    isolate: &v8::Isolate,
    edge: &elise::JsEdge,
    value: v8::Local<'_, v8::Value>,
) {
    // SAFETY: as `set_strong`'s.
    unsafe { value_slot(edge).set(isolate, Some(value)) };
}

/// What a strong edge of any value refers to (`set_strong_value`), if
/// anything.
pub fn strong_value<'s>(
    scope: &v8::PinScope<'s, '_, ()>,
    edge: &elise::JsEdge,
) -> Option<v8::Local<'s, v8::Value>> {
    // SAFETY: as `wrapper`'s.
    unsafe { value_slot(edge).get(scope) }
}

/// The promise resolver a strong edge refers to (`set_strong` of a
/// `PromiseResolver`: an animation's `ready`, Blink's
/// `ScriptPromiseProperty`), if it refers to a promise.
pub fn strong_resolver<'s>(
    scope: &v8::PinScope<'s, '_, ()>,
    edge: &elise::JsEdge,
) -> Option<v8::Local<'s, v8::PromiseResolver>> {
    let value = strong_value(scope, edge)?;
    if !value.is_promise() {
        return None;
    }
    // SAFETY: V8's `Promise::Resolver` is the promise itself (its `Cast`
    // checks only `IsPromise`, and `GetPromise` gives back the same
    // object), so a handle to a promise is a handle to its resolver.
    Some(unsafe { v8::Local::cast_unchecked(value) })
}

/// The wrapper `edge` refers to, if any.
pub fn wrapper<'s>(
    scope: &v8::PinScope<'s, '_, ()>,
    edge: &elise::JsEdge,
) -> Option<v8::Local<'s, v8::Object>> {
    // SAFETY: on the isolate's thread, the heap's (the only one that writes
    // the edge).
    unsafe { slot(edge).get(scope) }
}

/// Returns the wrapper `edge` refers to from the callback (`rv`), with no
/// local handle made for it (V8's `ReturnValue::Set(const
/// BasicTracedReference&)`, as Blink returns a node's wrapper): `false`,
/// returning nothing, if it has none.
#[inline]
pub fn return_wrapper(
    isolate: &v8::Isolate,
    rv: &mut v8::ReturnValue<'_, v8::Value>,
    edge: &elise::JsEdge,
) -> bool {
    // SAFETY: on the isolate's thread, the heap's (the only one that writes
    // the edge).
    unsafe { rv.set_traced(isolate, slot(edge)) }
}

/// An isolate's pointer, for a `TracedMarker` made on another thread.
#[derive(Clone, Copy)]
struct IsolatePtr(v8::UnsafeRawIsolatePtr);

// SAFETY: only passed to the fork's `TracedMarker`, which reads the
// isolate's heap and worklists on the thread it is made on (as cppgc's
// concurrent markers do).
unsafe impl Send for IsolatePtr {}
// SAFETY: as above.
unsafe impl Sync for IsolatePtr {}

impl IsolatePtr {
    fn new(isolate: &v8::Isolate) -> Self {
        // SAFETY: the pointer is only used while the isolate lives (a
        // marking of its).
        Self(unsafe { isolate.as_raw_isolate_ptr() })
    }
}

/// What the heap's marker gives the JS edges of what it marks to, in a
/// marking V8 drives: their references, marked into V8's worklists. Made
/// at V8's start; elise calls it only from that marking's threads, and drops
/// their locals before the marking is done.
struct TracedSink {
    isolate: IsolatePtr,
    running: Running,
}

impl elise::JsEdgeSink for TracedSink {
    fn local(&self) -> Box<dyn elise::JsEdgeLocal> {
        self.running.check("the heap's marker takes a local");
        // SAFETY: V8 runs the marking the sink was made at the start of:
        // the heap's marker, which calls this, runs in it, and drops the
        // local (its run's or step's end) before it is done, which V8's end
        // waits for.
        let marker = unsafe { v8::machopo::TracedMarker::new(self.isolate.0) };
        Box::new(TracedLocal {
            marker,
            running: self.running.clone(),
        })
    }
}

struct TracedLocal {
    marker: v8::machopo::TracedMarker,
    running: Running,
}

impl elise::JsEdgeLocal for TracedLocal {
    unsafe fn mark(&mut self, edge: NonNull<elise::JsEdge>) {
        self.running.check("the heap's marker marks a reference");
        // SAFETY: as the heap promises, a live object's JS edge, which is a
        // `TracedReference` V8 reads atomically (`GetSlotThreadSafe`), as
        // the heap's thread writes it (`SetSlotThreadSafe`).
        self.marker.mark(slot(unsafe { edge.as_ref() }));
    }
}

impl Drop for TracedLocal {
    fn drop(&mut self) {
        // Its marker publishes what it holds to V8's worklists next.
        if !thread::panicking() {
            self.running.check("the heap's marker publishes references");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::c_void;
    use std::ptr::NonNull;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    use elise::JsEdgeSink;
    use v8::machopo::EmbedderMarking;

    use super::{
        IsolatePtr, Local, Marking, Running, TracedLocal, TracedSink, budget, drained, on_thread,
    };

    #[derive(elise::GcObject)]
    struct Wrapped {
        wrapper: elise::JsEdge,
    }

    /// An engine that marks none of the JS edges it is given.
    struct NoEdges;

    impl elise::JsEdgeSink for NoEdges {
        fn local(&self) -> Box<dyn elise::JsEdgeLocal> {
            Box::new(NoEdges)
        }
    }

    impl elise::JsEdgeLocal for NoEdges {
        unsafe fn mark(&mut self, _: NonNull<elise::JsEdge>) {}
    }

    /// What `is_done` tells V8: an object handed over and published (as a
    /// parallel marker of V8's does as it finishes) is left to mark until
    /// the heap marks it.
    #[test]
    fn a_published_hand_over_is_left_to_mark() {
        let mut heap = elise::Heap::with_platform(
            elise::HeapOptions {
                driven: true,
                ..elise::HeapOptions::default()
            },
            Arc::new(elise::InlinePlatform),
        );
        let address = heap.mutate(|cx| cx.engine_address(cx.alloc(WrappedInit { wrapper: () })));
        let marker = heap.start_marking(Arc::new(NoEdges));
        let mut local = marker.local();
        // SAFETY: an object of the heap, alive: no GC ran since.
        unsafe { local.hand(address) };
        assert!(drained(Some(&marker)), "not published yet");
        local.publish();
        assert!(!drained(Some(&marker)));
        while !heap.advance_marking(true, Duration::MAX) {}
        assert!(drained(Some(&marker)));
        heap.finish_marking();
        assert!(drained(None));
    }

    /// A driven heap marking, with an object of it.
    fn marking() -> (elise::Heap, elise::WrapperMarker, NonNull<c_void>) {
        let mut heap = elise::Heap::with_platform(
            elise::HeapOptions {
                driven: true,
                ..elise::HeapOptions::default()
            },
            Arc::new(elise::InlinePlatform),
        );
        let address = heap.mutate(|cx| cx.engine_address(cx.alloc(WrappedInit { wrapper: () })));
        let marker = heap.start_marking(Arc::new(NoEdges));
        (heap, marker, address.cast())
    }

    // Debug builds check that no local outlives its marking.

    #[test]
    #[should_panic(expected = "V8 hands an object over after its marking ended")]
    fn a_hand_over_after_the_marking_is_caught() {
        let (_heap, marker, object) = marking();
        let running = Running::new();
        let mut local = Local {
            local: marker.local(),
            running: running.clone(),
        };
        running.end();
        Marking::hand(&mut local, object);
    }

    #[test]
    #[should_panic(expected = "V8 publishes objects after its marking ended")]
    fn objects_published_after_the_marking_are_caught() {
        let (_heap, marker, object) = marking();
        let running = Running::new();
        let mut local = Local {
            local: marker.local(),
            running: running.clone(),
        };
        Marking::hand(&mut local, object);
        running.end();
        Marking::publish(&mut local);
    }

    #[test]
    #[should_panic(expected = "a local holding objects goes after its marking ended")]
    fn objects_left_after_the_marking_are_caught() {
        let (_heap, marker, object) = marking();
        let running = Running::new();
        let mut local = Local {
            local: marker.local(),
            running: running.clone(),
        };
        Marking::hand(&mut local, object);
        running.end();
        drop(local);
    }

    /// An empty local goes after its marking (V8's main thread's, deleted
    /// in its pause after the heap's end): nothing is lost.
    #[test]
    fn an_empty_local_may_go_after_the_marking() {
        let (_heap, marker, object) = marking();
        let running = Running::new();
        let mut local = Local {
            local: marker.local(),
            running: running.clone(),
        };
        Marking::hand(&mut local, object);
        Marking::publish(&mut local);
        running.end();
        Marking::publish(&mut local);
        drop(local);
    }

    /// A heap as V8 drives it.
    struct Driven(std::rc::Rc<std::cell::RefCell<elise::Heap>>);

    impl super::EmbedderHeap for Driven {
        fn start(&self, sink: Arc<dyn elise::JsEdgeSink>) -> Option<elise::WrapperMarker> {
            Some(self.0.borrow_mut().start_marking(sink))
        }

        fn advance(&self, budget: Duration, atomic: bool) -> bool {
            self.0.borrow_mut().advance_marking(atomic, budget)
        }

        fn end(&self) -> usize {
            self.0.borrow_mut().finish_marking()
        }
    }

    /// V8's start and end: a local of a marking that ended is one.
    #[test]
    #[should_panic(expected = "V8 hands an object over after its marking ended")]
    fn a_local_of_an_ended_marking_is_caught() {
        let heap = std::rc::Rc::new(std::cell::RefCell::new(elise::Heap::with_platform(
            elise::HeapOptions {
                driven: true,
                ..elise::HeapOptions::default()
            },
            Arc::new(elise::InlinePlatform),
        )));
        let object = heap
            .borrow_mut()
            .mutate(|cx| cx.engine_address(cx.alloc(WrappedInit { wrapper: () })))
            .cast();
        let marking = Marking {
            heap: std::rc::Rc::new(Driven(heap)),
            marker: std::sync::Mutex::new(None),
            running: std::sync::Mutex::new(None),
            // Unused: nothing the heap marks has a JS edge to give V8.
            isolate: IsolatePtr(v8::UnsafeRawIsolatePtr::null()),
            thread: std::thread::current().id(),
        };
        marking.start();
        let mut local = marking.new_local().expect("a marking runs");
        while !marking.advance(f64::INFINITY, true) {}
        marking.end();
        Marking::hand(&mut local, object);
    }

    #[test]
    #[should_panic(expected = "the heap's marker takes a local after its marking ended")]
    fn the_heaps_local_after_the_marking_is_caught() {
        let running = Running::new();
        running.end();
        let sink = TracedSink {
            isolate: IsolatePtr(v8::UnsafeRawIsolatePtr::null()),
            running,
        };
        let _ = sink.local();
    }

    /// A `TracedLocal` of `isolate`'s, whose marking then ended.
    fn traced_local(isolate: &v8::Isolate) -> TracedLocal {
        let running = Running::new();
        let local = TracedLocal {
            // SAFETY: outside a marking, but it marks nothing (the check
            // stops it first) and publishes nothing: V8's worklists, the
            // collector's, are there for the isolate's life.
            marker: unsafe { v8::machopo::TracedMarker::new(IsolatePtr::new(isolate).0) },
            running: running.clone(),
        };
        running.end();
        local
    }

    #[test]
    #[should_panic(expected = "the heap's marker marks a reference after its marking ended")]
    fn a_reference_marked_after_the_marking_is_caught() {
        crate::initialize();
        let mut data = crate::V8PerIsolateData::new();
        let mut local = traced_local(data.isolate());
        // SAFETY: not read: the check stops it first.
        unsafe { elise::JsEdgeLocal::mark(&mut local, NonNull::dangling()) };
    }

    #[test]
    #[should_panic(expected = "the heap's marker publishes references after its marking ended")]
    fn references_published_after_the_marking_are_caught() {
        crate::initialize();
        let mut data = crate::V8PerIsolateData::new();
        drop(traced_local(data.isolate()));
    }

    /// The check of the thread V8 calls the embedder heap on: this
    /// thread's id passes here, and fails on another thread.
    #[test]
    fn the_isolates_thread_is_checked() {
        let here = thread::current().id();
        on_thread(here);
        let elsewhere = thread::spawn(move || on_thread(here)).join();
        let message = elsewhere.expect_err("another thread fails the check");
        let message = message
            .downcast_ref::<String>()
            .map(String::as_str)
            .unwrap_or_default();
        assert!(
            message.contains("V8 calls the embedder heap on the isolate's thread"),
            "{message}"
        );
    }

    #[test]
    fn v8s_step_time_is_a_duration() {
        assert_eq!(budget(5.0), Duration::from_millis(5));
        assert_eq!(budget(0.25), Duration::from_micros(250));
        assert_eq!(budget(0.0), Duration::ZERO);
        assert_eq!(budget(-1.0), Duration::ZERO);
        assert_eq!(budget(f64::NAN), Duration::ZERO);
        assert_eq!(budget(f64::INFINITY), Duration::MAX);
        assert_eq!(budget(f64::MAX), Duration::MAX);
    }
}
