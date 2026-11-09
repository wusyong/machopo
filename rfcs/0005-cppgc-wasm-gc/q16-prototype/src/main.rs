//! Q16: wasm GC objects (structs and arrays laid out by a type table) as
//! cppgc objects, next to DOM nodes, without wasmtime. See `wasm.rs` for
//! the objects; this file checks them:
//!
//! - `cycles`: wasm objects and nodes in cycles, i31s and objects in the
//!   same `anyref` fields; everything comes back as written, and the
//!   cycles are collected once unrooted (a standalone heap, forced GCs).
//! - `fuzz-incremental`, `fuzz-concurrent`: a random graph mutated between
//!   the steps of an incremental GC (a detached `CppHeap` and cppgc's
//!   testing API), on the main thread only or with all marking on
//!   concurrent marker threads; a shadow graph says what must stay alive.
//! - `fuzz-standalone`: the same on a standalone heap that schedules its
//!   own GCs, run as tasks between batches of mutations.
//! - `size`: bytes per object, from the process's memory.
//!
//! Which objects are alive: the reachable ones must hold what the shadow
//! graph says (a collected object's memory is reused or overwritten by
//! cppgc's free list), and a weak table of every object (`observer`, a
//! `WeakObjectTable` never entered) must hold exactly the reachable ones
//! after full GCs.
//! - `all`: all of the above.

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use v8::cppgc::standalone::StandaloneHeap;
use v8::cppgc::testing::{self, DetachedHeap};
use v8::cppgc::wasm_gc::WeakObjectTable;
use v8::cppgc::{AllocationHandle, EmbedderStackState, HeapCreateParams, Persistent, UnsafePtr};

mod wasm;

use wasm::{A, Any, FINALIZED, Node, Obj, P, Roots, S, node_ptr};

/// The fuzzed graph's backbone: `ROOTS` arrays of `SLOTS` elements, each
/// filled with a new object, so that the graph stays at about `TARGET`
/// reachable objects and marking takes many steps. Long arrays also get
/// written while marking is partway through them.
const ROOTS: usize = 8;
const SLOTS: u32 = 4_096;
const TARGET: usize = 40_000;
/// Root slots that move, after the backbone's.
const MOVING_ROOTS: usize = 8;

fn grow(graph: &mut Graph, handle: &AllocationHandle, rng: &mut Rng) {
    for root in 0..ROOTS {
        let id = graph.shadows.len();
        // SAFETY: rooted at once.
        let array = unsafe { Obj::new(handle, A, SLOTS + 1, id as u32) };
        graph.add(
            Shadow::A(vec![AnyShadow::I31(id as i32); 1]),
            Ptr::Wasm(array),
        );
        graph.set_root(root, Some(id));
        for slot in 1..=SLOTS {
            let child = loop {
                let child = graph.alloc(handle, rng);
                if !graph.is_node(child) {
                    break child;
                }
            };
            if let Shadow::A(elems) = &mut graph.shadows[id] {
                elems.push(AnyShadow::Obj(child));
            }
            set_any(graph, array, slot, AnyShadow::Obj(child));
        }
    }
    let mut live = graph.reachable();
    for _ in 0..50_000 {
        graph.mutate(handle, rng, &mut live);
    }
}

/// xorshift64*, so runs are repeatable without a dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum AnyShadow {
    Null,
    I31(i32),
    Obj(usize),
}

/// What each object holds, as far as the program knows. An array's
/// element 0 is its id.
#[derive(Clone, Debug)]
enum Shadow {
    S {
        a: Option<usize>,
        b: AnyShadow,
        n: Option<usize>,
    },
    A(Vec<AnyShadow>),
    P {
        x: f64,
        v: i32,
    },
    Node {
        wasm: Option<usize>,
    },
}

enum Ptr {
    Wasm(Obj),
    Node(UnsafePtr<Node>),
}

/// The objects (index = id), their shadows, and the roots. The pointers
/// are kept for every object ever made, but only those the shadow graph
/// reaches from the roots are used: the others may be gone.
struct Graph {
    shadows: Vec<Shadow>,
    ptrs: Vec<Ptr>,
    /// What each root slot holds.
    roots: [Option<usize>; ROOTS + MOVING_ROOTS],
    root_set: Persistent<Roots>,
    observer: NonNull<WeakObjectTable>,
}

impl Graph {
    fn new(handle: &AllocationHandle) -> Self {
        // SAFETY: rooted at once; the observer, in a root slot, before the
        // next allocation.
        let root_set = Persistent::new(&unsafe { wasm::new_roots(handle) });
        let observer = unsafe { WeakObjectTable::new(handle) };
        unsafe { root_set.get().unwrap().table.assign(observer.as_ptr()) };
        Self {
            shadows: Vec::new(),
            ptrs: Vec::new(),
            roots: [None; ROOTS + MOVING_ROOTS],
            root_set,
            observer,
        }
    }

    fn add(&mut self, shadow: Shadow, ptr: Ptr) {
        // SAFETY: both alive; on the heap's thread.
        unsafe {
            match &ptr {
                Ptr::Wasm(obj) => WeakObjectTable::add_wasm(self.observer, obj.raw()),
                Ptr::Node(node) => WeakObjectTable::add_host(self.observer, node),
            }
        }
        self.shadows.push(shadow);
        self.ptrs.push(ptr);
    }

    fn set_root(&mut self, index: usize, id: Option<usize>) {
        self.roots[index] = id;
        let target = self.wasm_raw(id);
        // SAFETY: the target is alive; on the heap's thread.
        unsafe { self.root_set.get().unwrap().slots[index].assign(target) };
    }

    fn wasm(&self, id: usize) -> Obj {
        match &self.ptrs[id] {
            Ptr::Wasm(obj) => *obj,
            Ptr::Node(_) => panic!("{id} is a node"),
        }
    }

    fn node(&self, id: usize) -> &UnsafePtr<Node> {
        match &self.ptrs[id] {
            Ptr::Node(ptr) => ptr,
            Ptr::Wasm(_) => panic!("{id} is a wasm object"),
        }
    }

    fn ptr(&self, id: usize) -> *const c_void {
        match &self.ptrs[id] {
            Ptr::Wasm(obj) => obj.as_ptr(),
            Ptr::Node(node) => node_ptr(node),
        }
    }

    /// What a `WasmMemberSlot` takes.
    fn wasm_raw(&self, id: Option<usize>) -> *mut v8::cppgc::wasm_gc::WasmGcObject {
        id.map_or(std::ptr::null_mut(), |id| self.wasm(id).raw().as_ptr())
    }

    fn opt_ptr(&self, id: Option<usize>) -> *const c_void {
        id.map_or(std::ptr::null(), |id| self.ptr(id))
    }

    fn is_node(&self, id: usize) -> bool {
        matches!(self.ptrs[id], Ptr::Node(_))
    }

    /// A new, unlinked object of a random kind.
    fn alloc(&mut self, handle: &AllocationHandle, rng: &mut Rng) -> usize {
        let id = self.shadows.len();
        // SAFETY (allocations): kept in `ptrs`, used only while reachable.
        let (shadow, ptr) = match rng.below(4) {
            0 => (
                Shadow::S {
                    a: None,
                    b: AnyShadow::Null,
                    n: None,
                },
                Ptr::Wasm(unsafe { Obj::new(handle, S, 0, id as u32) }),
            ),
            1 => {
                let len = 2 + rng.below(7) as u32;
                let mut elems = vec![AnyShadow::Null; len as usize];
                elems[0] = AnyShadow::I31(id as i32);
                (
                    Shadow::A(elems),
                    Ptr::Wasm(unsafe { Obj::new(handle, A, len, id as u32) }),
                )
            }
            2 => (
                Shadow::P { x: 0.0, v: 0 },
                Ptr::Wasm(unsafe { Obj::new(handle, P, 0, id as u32) }),
            ),
            _ => (
                Shadow::Node { wasm: None },
                Ptr::Node(unsafe { wasm::new_node(handle, id as u32) }),
            ),
        };
        self.add(shadow, ptr);
        id
    }

    fn reachable(&self) -> Vec<usize> {
        let mut seen = vec![false; self.shadows.len()];
        let mut stack: Vec<usize> = self.roots.iter().flatten().copied().collect();
        let mut out = Vec::new();
        while let Some(id) = stack.pop() {
            if std::mem::replace(&mut seen[id], true) {
                continue;
            }
            out.push(id);
            let any = |a: &AnyShadow| match a {
                AnyShadow::Obj(id) => Some(*id),
                _ => None,
            };
            match &self.shadows[id] {
                Shadow::S { a, b, n } => {
                    stack.extend(a.iter().chain(any(b).iter()).chain(n.iter()))
                }
                Shadow::A(elems) => stack.extend(elems.iter().filter_map(any)),
                Shadow::P { .. } => {}
                Shadow::Node { wasm } => stack.extend(wasm.iter()),
            }
        }
        out
    }

    /// One mutation, as a wasm program (or the DOM) would make it, among
    /// `live` (reachable at the start of the batch, so not collected).
    fn mutate(&mut self, handle: &AllocationHandle, rng: &mut Rng, live: &mut Vec<usize>) {
        // Keep the reachable graph around `TARGET` objects: below it, more
        // new objects and fewer cut links; above it, the other way round.
        let big = live.len() > TARGET;
        let cut = if big { 40 } else { 10 };
        // Sometimes a new object, which the write below may link in.
        if rng.chance(if big { 5 } else { 40 }) || live.is_empty() {
            let id = self.alloc(handle, rng);
            live.push(id);
            let free = (ROOTS..ROOTS + MOVING_ROOTS).find(|i| self.roots[*i].is_none());
            if let (Some(index), false) = (free, self.is_node(id)) {
                self.set_root(index, Some(id));
                return;
            }
        }
        let src = if rng.chance(30) {
            self.roots[rng.below(ROOTS)].unwrap()
        } else {
            live[rng.below(live.len())]
        };
        let wasm_target = |g: &Self, rng: &mut Rng| -> Option<usize> {
            (0..4)
                .map(|_| live[rng.below(live.len())])
                .find(|id| !g.is_node(*id))
        };
        let node_target = |g: &Self, rng: &mut Rng| -> Option<usize> {
            (0..4)
                .map(|_| live[rng.below(live.len())])
                .find(|id| g.is_node(*id))
        };
        let any_value = |g: &Self, rng: &mut Rng| -> AnyShadow {
            match rng.below(3) {
                0 => AnyShadow::Null,
                // Edge values too: 0 (encoded 1, cppgc's sentinel), -1, the
                // extremes of i31.
                1 => AnyShadow::I31(
                    [0, -1, 1, (1 << 30) - 1, -(1 << 30), rng.next() as i32 >> 1][rng.below(6)],
                ),
                _ => wasm_target(g, rng).map_or(AnyShadow::Null, AnyShadow::Obj),
            }
        };
        let mut shadow = self.shadows[src].clone();
        // SAFETY (writes): `src` and every target are reachable, so alive;
        // on the heap's thread.
        match &mut shadow {
            Shadow::S { a, b, n } => {
                let obj = self.wasm(src);
                match rng.below(3) {
                    0 => {
                        *a = if rng.chance(cut) {
                            None
                        } else {
                            wasm_target(self, rng)
                        };
                        unsafe { obj.set_ref(4, a.map(|id| self.wasm(id))) };
                    }
                    1 => {
                        *b = any_value(self, rng);
                        set_any(self, obj, 2, *b);
                    }
                    _ => {
                        *n = if rng.chance(cut) {
                            None
                        } else {
                            node_target(self, rng)
                        };
                        unsafe { obj.set_node(12, n.map(|id| self.node(id))) };
                    }
                }
            }
            Shadow::A(elems) => {
                let index = 1 + rng.below(elems.len() - 1);
                elems[index] = any_value(self, rng);
                set_any(self, self.wasm(src), index as u32, elems[index]);
            }
            Shadow::P { x, v } => {
                *x = rng.next() as f64;
                *v = rng.next() as i32;
                let obj = self.wasm(src);
                obj.set_f64(0, *x);
                obj.set_i32(8, *v);
            }
            Shadow::Node { wasm } => {
                *wasm = if rng.chance(cut) {
                    None
                } else {
                    wasm_target(self, rng)
                };
                let node = unsafe { self.node(src).as_ref() };
                unsafe { node.wasm.assign(self.wasm_raw(*wasm)) };
            }
        }
        self.shadows[src] = shadow;
        // Now and then a moving root moves or goes.
        if rng.chance(2) {
            let index = ROOTS + rng.below(MOVING_ROOTS);
            let target = wasm_target(self, rng);
            self.set_root(index, target);
        }
    }

    /// Every reachable object holds what the shadow says.
    fn check(&self) -> usize {
        let reachable = self.reachable();
        for &id in &reachable {
            // SAFETY (reads): reachable, so alive; no GC during the check.
            match &self.shadows[id] {
                Shadow::Node { wasm } => {
                    let node = unsafe { self.node(id).as_ref() };
                    assert_eq!(node.id(), id as u32);
                    let got = unsafe { node.wasm.get() }
                        .map_or(std::ptr::null(), |o| o.as_ptr() as *const c_void);
                    assert_eq!(got, self.opt_ptr(*wasm), "node {id}");
                }
                shadow => {
                    let obj = self.wasm(id);
                    assert_eq!(obj.id(), id as u32, "object {id}");
                    match shadow {
                        Shadow::S { a, b, n } => {
                            assert_eq!(obj.ty(), S);
                            let got =
                                unsafe { obj.get_ref(4) }.map_or(std::ptr::null(), Obj::as_ptr);
                            assert_eq!(got, self.opt_ptr(*a), "struct {id}.a");
                            assert_any(self, obj, 2, *b, || format!("struct {id}.b"));
                            assert_eq!(
                                unsafe { obj.get_node(12) },
                                self.opt_ptr(*n),
                                "struct {id}.n"
                            );
                        }
                        Shadow::A(elems) => {
                            assert_eq!(obj.ty(), A);
                            assert_eq!(obj.len() as usize, elems.len());
                            for (index, elem) in elems.iter().enumerate() {
                                assert_any(self, obj, index as u32, *elem, || {
                                    format!("array {id}[{index}]")
                                });
                            }
                        }
                        Shadow::P { x, v } => {
                            assert_eq!(obj.ty(), P);
                            assert_eq!(obj.f64(0).to_bits(), x.to_bits());
                            assert_eq!(obj.i32(8), *v);
                        }
                        Shadow::Node { .. } => unreachable!(),
                    }
                }
            }
        }
        reachable.len()
    }

    /// After full GCs: exactly the reachable objects are left, and every
    /// unreachable node was finalized, once.
    fn check_exact(&self) {
        let reachable = self.check();
        // SAFETY: rooted; on the heap's thread.
        let left = unsafe { WeakObjectTable::len(self.observer) };
        assert_eq!(left, reachable, "objects left after full GCs");
        let nodes = (0..self.shadows.len())
            .filter(|id| self.is_node(*id))
            .count();
        let live_nodes = self
            .reachable()
            .into_iter()
            .filter(|id| self.is_node(*id))
            .count();
        let mut finalized = FINALIZED.lock().unwrap().clone();
        let count = finalized.len();
        finalized.sort();
        finalized.dedup();
        assert_eq!(finalized.len(), count, "a node was finalized twice");
        assert_eq!(count, nodes - live_nodes, "nodes finalized");
    }
}

/// The index of an `AnyRef` slot: struct field 2 is at offset 8; array
/// element `i` at `4 * i`.
fn any_offset(obj: Obj, index: u32) -> u32 {
    if obj.ty() == S { 8 } else { 4 * index }
}

fn set_any(g: &Graph, obj: Obj, index: u32, value: AnyShadow) {
    let value = match value {
        AnyShadow::Null => Any::Null,
        AnyShadow::I31(i) => Any::I31(i),
        AnyShadow::Obj(id) => Any::Obj(g.wasm(id)),
    };
    // SAFETY: the target is reachable, so alive; on the heap's thread.
    unsafe { obj.set_any(any_offset(obj, index), value) }
}

fn assert_any(g: &Graph, obj: Obj, index: u32, want: AnyShadow, what: impl Fn() -> String) {
    // SAFETY: `obj` is reachable; no GC meanwhile.
    let got = unsafe { obj.get_any(any_offset(obj, index)) };
    let ok = match (got, want) {
        (Any::Null, AnyShadow::Null) => true,
        (Any::I31(a), AnyShadow::I31(b)) => a == b,
        (Any::Obj(o), AnyShadow::Obj(id)) => o.as_ptr() == g.ptr(id),
        _ => false,
    };
    assert!(ok, "{}: want {want:?}", what());
}

/// The renderer's queue for a standalone heap's tasks (as in Q18).
#[derive(Default)]
struct Queue(Mutex<Vec<(Instant, v8::Task)>>);

impl Queue {
    fn run_due(&self) -> usize {
        let mut ran = 0;
        loop {
            let task = {
                let mut tasks = self.0.lock().unwrap();
                let now = Instant::now();
                match tasks.iter().position(|(due, _)| *due <= now) {
                    Some(index) => tasks.remove(index).1,
                    None => return ran,
                }
            };
            task.run();
            ran += 1;
        }
    }
}

struct Runner(Arc<Queue>);

impl v8::PlatformImpl for Runner {
    fn post_task(&self, _: *mut std::ffi::c_void, task: v8::Task) {
        self.0.0.lock().unwrap().push((Instant::now(), task));
    }

    fn post_non_nestable_task(&self, _: *mut std::ffi::c_void, task: v8::Task) {
        self.post_task(std::ptr::null_mut(), task);
    }

    fn post_delayed_task(&self, _: *mut std::ffi::c_void, task: v8::Task, delay: f64) {
        let due = Instant::now() + Duration::from_secs_f64(delay);
        self.0.0.lock().unwrap().push((due, task));
    }

    fn post_non_nestable_delayed_task(&self, _: *mut std::ffi::c_void, task: v8::Task, delay: f64) {
        self.post_delayed_task(std::ptr::null_mut(), task, delay);
    }
}

fn standalone_heap(queue: &Arc<Queue>) -> StandaloneHeap {
    StandaloneHeap::new(
        v8::V8::get_current_platform(),
        HeapCreateParams::default(),
        Runner(queue.clone()),
    )
}

fn cycles() {
    FINALIZED.lock().unwrap().clear();
    let queue = Arc::new(Queue::default());
    let heap = standalone_heap(&queue);
    let handle = heap.allocation_handle();
    let i31s = [0, -1, 1, (1 << 30) - 1, -(1 << 30)];
    let graph = Graph::new(handle);
    let roots = graph.root_set.get().unwrap();
    // SAFETY (whole block): every pointer is rooted or stored before the
    // next GC, which only `collect_garbage` runs; on the heap's thread.
    unsafe {
        let s1 = Obj::new(handle, S, 0, 1);
        let s2 = Obj::new(handle, S, 0, 2);
        let arr = Obj::new(handle, A, 8, 3);
        let node = wasm::new_node(handle, 4);
        for obj in [s1, s2, arr] {
            WeakObjectTable::add_wasm(graph.observer, obj.raw());
        }
        WeakObjectTable::add_host(graph.observer, &node);
        roots.slots[0].assign(arr.raw().as_ptr());
        // s1 <-> s2, s1 -> node -> s2 (a listener), s2 -> arr -> s1, i31s.
        s1.set_ref(4, Some(s2));
        s2.set_ref(4, Some(s1));
        s1.set_node(12, Some(&node));
        node.as_ref().wasm.assign(s2.raw().as_ptr());
        s2.set_any(8, Any::Obj(arr));
        arr.set_any(4, Any::Obj(s1));
        for (index, value) in i31s.iter().enumerate() {
            arr.set_any(4 * (2 + index as u32), Any::I31(*value));
        }
        heap.collect_garbage();
        assert_eq!(WeakObjectTable::len(graph.observer), 4);
        assert!(FINALIZED.lock().unwrap().is_empty());
        assert_eq!(arr.id(), 3);
        let Any::Obj(got) = arr.get_any(4) else {
            panic!()
        };
        assert_eq!(got, s1);
        for (index, value) in i31s.iter().enumerate() {
            let Any::I31(got) = arr.get_any(4 * (2 + index as u32)) else {
                panic!("arr[{}] is not an i31", 2 + index)
            };
            assert_eq!(got, *value);
        }
        assert!(matches!(arr.get_any(4 * 7), Any::Null));
        assert_eq!(s1.get_node(12), node_ptr(&node));
        assert_eq!(node.as_ref().wasm.get(), Some(s2.raw()));
        roots.slots[0].assign(std::ptr::null_mut());
        heap.collect_garbage();
        assert_eq!(WeakObjectTable::len(graph.observer), 0);
    }
    assert_eq!(*FINALIZED.lock().unwrap(), [4]);
    println!("cycles: ok (two structs, an array, a node; i31s {i31s:?} read back)");
}

/// Randomly mutates between the steps of incremental GCs on a detached
/// `CppHeap`; with `concurrent`, all marking runs on marker threads while
/// the main thread mutates.
fn fuzz_detached(concurrent: bool, seed: u64) {
    FINALIZED.lock().unwrap().clear();
    let heap = DetachedHeap::new(concurrent);
    let handle = heap.allocation_handle();
    let mut graph = Graph::new(handle);
    let mut rng = Rng(seed);
    grow(&mut graph, handle, &mut rng);
    let mut steps = 0;
    let mut most = 0;
    for _ in 0..40 {
        testing::start_incremental_gc(&heap);
        let mut gc_steps = 0;
        if concurrent {
            testing::set_main_thread_marking(&heap, false);
        }
        loop {
            let mut live = graph.reachable();
            most = most.max(live.len());
            for _ in 0..50 {
                graph.mutate(handle, &mut rng, &mut live);
            }
            steps += 1;
            gc_steps += 1;
            // With main-thread marking off, a step may never report the
            // marking done (work the barrier hands the main thread stays
            // there); the final pause finishes it.
            if testing::marking_step(&heap) || gc_steps == 100 {
                break;
            }
        }
        if concurrent {
            // The final pause marks on this thread.
            testing::set_main_thread_marking(&heap, true);
        }
        testing::finalize_gc(&heap);
        let reachable = graph.check();
        // SAFETY: rooted; on the heap's thread.
        let left = unsafe { WeakObjectTable::len(graph.observer) };
        assert!(
            left >= reachable,
            "the observer lost live objects: {left} < {reachable}"
        );
    }
    // SAFETY: nothing unrooted on the stack.
    unsafe {
        heap.collect_garbage_for_testing(EmbedderStackState::NoHeapPointers);
        heap.collect_garbage_for_testing(EmbedderStackState::NoHeapPointers);
    }
    graph.check_exact();
    println!(
        "fuzz-{}: ok, seed {seed}: 40 GCs, {steps} marking steps with 50 mutations before each, \
         {} objects, at most {most} reachable",
        if concurrent {
            "concurrent"
        } else {
            "incremental"
        },
        graph.shadows.len(),
    );
}

/// The same on a standalone heap: cppgc decides when to collect, and its
/// tasks run between batches of mutations.
fn fuzz_standalone(seed: u64) {
    FINALIZED.lock().unwrap().clear();
    let queue = Arc::new(Queue::default());
    let heap = standalone_heap(&queue);
    let handle = heap.allocation_handle();
    let mut graph = Graph::new(handle);
    let mut rng = Rng(seed);
    grow(&mut graph, handle, &mut rng);
    let mut tasks = 0;
    let mut most = 0;
    let mut least_left = usize::MAX;
    for _ in 0..4_000 {
        let mut live = graph.reachable();
        most = most.max(live.len());
        for _ in 0..50 {
            graph.mutate(handle, &mut rng, &mut live);
        }
        // Garbage, so cppgc collects often (not in the observer).
        for _ in 0..20 {
            // SAFETY: dropped at once.
            unsafe { Obj::new(handle, A, 64, u32::MAX) };
        }
        tasks += queue.run_due();
        let reachable = graph.check();
        // SAFETY: rooted; on the heap's thread.
        let left = unsafe { WeakObjectTable::len(graph.observer) };
        assert!(
            left >= reachable,
            "the observer lost live objects: {left} < {reachable}"
        );
        least_left = least_left.min(left);
    }
    let made = graph.shadows.len();
    // SAFETY: nothing unrooted on the stack.
    unsafe {
        heap.collect_garbage();
        heap.collect_garbage();
    }
    graph.check_exact();
    println!(
        "fuzz-standalone: ok, seed {seed}: 4,000 batches of 50 mutations, {tasks} GC tasks, \
         {made} objects, at most {most} reachable; the observer held as few as {least_left} \
         between batches (cppgc collecting on its own)",
    );
}

fn rss_bytes() -> f64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .expect("ps");
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<f64>()
        .unwrap()
        * 1024.0
}

/// Bytes per object: a million of each, chained from one root.
fn size() {
    let queue = Arc::new(Queue::default());
    let heap = standalone_heap(&queue);
    let handle = heap.allocation_handle();
    let graph = Graph::new(handle);
    let roots = graph.root_set.get().unwrap();
    const N: usize = 1_000_000;
    for (name, ty, len, payload) in [
        ("struct {i32, ref, anyref, externref}", S, 0, 16),
        ("array of 8 anyref", A, 8, 32),
    ] {
        let before = rss_bytes();
        // SAFETY: each object is linked from the rooted chain before the
        // next allocation, and no GC task runs during the loop.
        unsafe {
            let first = Obj::new(handle, ty, len, 0);
            roots.slots[0].assign(first.raw().as_ptr());
            let mut last = first;
            for i in 1..N {
                let next = Obj::new(handle, ty, len, i as u32);
                if ty == S {
                    last.set_ref(4, Some(next));
                } else {
                    last.set_any(4, Any::Obj(next));
                }
                last = next;
            }
        }
        let per = (rss_bytes() - before) / N as f64;
        println!(
            "size: {name}: about {per:.0} bytes each ({payload} of fields, cppgc's 8-byte \
             header, the 8-byte `WasmGcObject` header, 16-alignment)",
        );
        // SAFETY: on the heap's thread; nothing unrooted on the stack.
        unsafe {
            roots.slots[0].assign(std::ptr::null_mut());
            heap.collect_garbage();
        }
    }
}

fn main() {
    let platform = v8::new_default_platform(0, false).make_shared();
    v8::V8::initialize_platform(platform.clone());
    v8::cppgc::initialize_process(platform);
    // A detached `CppHeap` (the fuzz modes) is V8's; the standalone heap
    // doesn't need this.
    v8::V8::initialize();
    wasm::init();
    let seed = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0x9e37_79b9_7f4a_7c15);
    match std::env::args().nth(1).as_deref() {
        Some("cycles") => cycles(),
        Some("fuzz-incremental") => fuzz_detached(false, seed),
        Some("fuzz-concurrent") => fuzz_detached(true, seed),
        Some("fuzz-standalone") => fuzz_standalone(seed),
        Some("size") => size(),
        Some("all") => {
            cycles();
            fuzz_detached(false, seed);
            fuzz_detached(true, seed);
            fuzz_standalone(seed);
            size();
        }
        _ => eprintln!(
            "usage: q16-prototype cycles|fuzz-incremental|fuzz-concurrent|fuzz-standalone|size|all [seed]"
        ),
    }
}
