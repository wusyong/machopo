//! Q15's fallback: while a wasm Store runs, a GC (V8's, say, from a JS
//! listener the wasm code dispatched to) keeps everything the Store
//! allocated or was handed alive, without knowing what its stack holds.
//! Our rusty_v8 fork's `WeakObjectTable` holds those objects: weakly while
//! the Store isn't running, strongly while it is.
//!
//! Simulated: a few Stores, each with a holder (its table and its globals,
//! which stand for wasm's own roots: tables, globals); a stack of running
//! frames, each with a "wasm stack" of values that the GC never sees (a
//! Rust `Vec`); a document of DOM nodes. A running Store allocates, reads
//! and writes fields, gets nodes from the host, sets listeners, calls into
//! another Store (nesting), returns. GCs advance step by step between the
//! operations and finish at any point, also while Stores run, with no stack
//! scanning. Afterwards everything reachable from the document, the
//! globals and every frame's stack must be intact; at the end, with no
//! Store running, the weak tables must hold exactly the live objects.
//!
//! - `fuzz-incremental`, `fuzz-concurrent`: marking on the main thread, or
//!   all of it on marker threads.
//! - `cost`: what the table costs (adding, entering, the second trace).
//! - `all`.

use std::collections::HashSet;
use std::ffi::CStr;
use std::ptr::NonNull;
use std::time::{Duration, Instant};

use v8::cppgc::testing::{self, DetachedHeap};
use v8::cppgc::wasm_gc::{WasmMemberSlot, WeakObjectTable, WeakObjectTableSlot};
use v8::cppgc::{
    AllocationHandle, EmbedderStackState, GarbageCollected, MemberSlot, Persistent, UnsafePtr,
    Visitor, make_garbage_collected_on,
};

#[path = "../../q16-prototype/src/wasm.rs"]
#[allow(dead_code)]
mod wasm;

use wasm::{A, Any, FINALIZED, Node, Obj, P, S, node_ptr};

const STORES: usize = 3;
const GLOBALS: usize = 8;
const DOC: usize = 64;
/// A frame's stack never holds more.
const STACK: usize = 32;
const BALLAST: usize = 100_000;
/// Each Store's first globals hold arrays of `BACKBONE_LEN` objects, so
/// that marking the program's own objects takes many steps too: a Store
/// may read an object out of an array marking hasn't reached, clear the
/// element, and hold the object on its stack only.
const BACKBONE: usize = 4;
const BACKBONE_LEN: u32 = 1_024;

fn grow(world: &mut World, handle: &AllocationHandle, rng: &mut Rng) {
    for store in 0..STORES {
        world.enter(store);
        for global in 0..BACKBONE {
            let array = world.new_wasm_of(handle, store, A, 1 + BACKBONE_LEN);
            world.set_global(store, global, Some(array));
            for index in 1..=BACKBONE_LEN as usize {
                let child = world.new_wasm(handle, rng, store);
                if let Shadow::A(elems) = &mut world.shadows[array] {
                    elems[index] = AnyShadow::Obj(child);
                }
                // SAFETY: both alive (no GC while growing).
                unsafe {
                    world
                        .wasm(array)
                        .set_any(4 * index as u32, Any::Obj(world.wasm(child)))
                };
            }
        }
        world.exit();
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

/// A Store's holder (`StoreHolder` in RFC 0001): its table and its roots.
struct Holder {
    table: WeakObjectTableSlot,
    globals: [WasmMemberSlot; GLOBALS],
}

// SAFETY: `trace` visits every slot.
unsafe impl GarbageCollected for Holder {
    fn trace(&self, visitor: &mut Visitor) {
        // SAFETY (all): fields of this object, from its trace.
        unsafe { self.table.trace(visitor) };
        for slot in &self.globals {
            unsafe { slot.trace(visitor) };
        }
    }

    fn get_name(&self) -> &'static CStr {
        c"Holder"
    }
}

/// The document: the nodes it holds, a table of every object, never
/// entered, that says which objects are left after full GCs, and ballast
/// (a long chain of objects outside the program) so that marking takes
/// many steps.
struct Document {
    nodes: [MemberSlot<Node>; DOC],
    observer: WeakObjectTableSlot,
    ballast: WasmMemberSlot,
}

// SAFETY: `trace` visits every slot.
unsafe impl GarbageCollected for Document {
    fn trace(&self, visitor: &mut Visitor) {
        // SAFETY (all): fields of this object, from its trace.
        for slot in &self.nodes {
            unsafe { slot.trace(visitor) };
        }
        unsafe { self.observer.trace(visitor) };
        unsafe { self.ballast.trace(visitor) };
    }

    fn get_name(&self) -> &'static CStr {
        c"Document"
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum AnyShadow {
    Null,
    I31(i32),
    Obj(usize),
}

/// What each object holds. An array's element 0 is its id.
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

struct Store {
    holder: Persistent<Holder>,
    table: NonNull<WeakObjectTable>,
    globals: [Option<usize>; GLOBALS],
    /// The nodes it was handed.
    handed: HashSet<usize>,
}

struct Frame {
    store: usize,
    /// The wasm stack: values the GC never sees.
    stack: Vec<usize>,
}

struct World {
    shadows: Vec<Shadow>,
    ptrs: Vec<Ptr>,
    /// The Store that allocated each wasm object (nodes have none).
    owner: Vec<Option<usize>>,
    stores: Vec<Store>,
    frames: Vec<Frame>,
    doc: Persistent<Document>,
    doc_nodes: [Option<usize>; DOC],
    observer: NonNull<WeakObjectTable>,
    /// Instrumentation: `false` makes handing a node to a Store skip its
    /// table (to show that the rule is needed).
    record_handed: bool,
}

impl World {
    fn new(handle: &AllocationHandle) -> Self {
        // SAFETY (allocations): each is rooted or stored in a rooted object
        // before the next allocation.
        let doc = Persistent::new(&unsafe {
            make_garbage_collected_on(
                handle,
                Document {
                    nodes: std::array::from_fn(|_| MemberSlot::empty()),
                    observer: WeakObjectTableSlot::empty(),
                    ballast: WasmMemberSlot::empty(),
                },
            )
        });
        let observer = unsafe { WeakObjectTable::new(handle) };
        unsafe { doc.get().unwrap().observer.assign(observer.as_ptr()) };
        // The ballast: arrays of 8 `anyref`s, each pointing to the next.
        unsafe {
            let first = Obj::new(handle, A, 8, u32::MAX);
            doc.get().unwrap().ballast.assign(first.raw().as_ptr());
            let mut last = first;
            for _ in 0..BALLAST {
                let next = Obj::new(handle, A, 8, u32::MAX);
                last.set_any(4, Any::Obj(next));
                last = next;
            }
        }
        let stores = (0..STORES)
            .map(|_| {
                let holder = Persistent::new(&unsafe {
                    make_garbage_collected_on(
                        handle,
                        Holder {
                            table: WeakObjectTableSlot::empty(),
                            globals: std::array::from_fn(|_| WasmMemberSlot::empty()),
                        },
                    )
                });
                let table = unsafe { WeakObjectTable::new(handle) };
                unsafe { holder.get().unwrap().table.assign(table.as_ptr()) };
                Store {
                    holder,
                    table,
                    globals: [None; GLOBALS],
                    handed: HashSet::new(),
                }
            })
            .collect();
        Self {
            shadows: Vec::new(),
            ptrs: Vec::new(),
            owner: Vec::new(),
            stores,
            frames: Vec::new(),
            doc,
            doc_nodes: [None; DOC],
            observer,
            record_handed: true,
        }
    }

    fn wasm(&self, id: usize) -> Obj {
        match &self.ptrs[id] {
            Ptr::Wasm(obj) => *obj,
            Ptr::Node(_) => panic!("{id} is a node"),
        }
    }

    fn node(&self, id: usize) -> &UnsafePtr<Node> {
        match &self.ptrs[id] {
            Ptr::Node(node) => node,
            Ptr::Wasm(_) => panic!("{id} is a wasm object"),
        }
    }

    /// The objects the program can still reach: from the document, the
    /// Stores' globals and every running frame's stack.
    fn reachable(&self) -> Vec<usize> {
        let mut seen = vec![false; self.shadows.len()];
        let mut stack: Vec<usize> = self.doc_nodes.iter().flatten().copied().collect();
        for store in &self.stores {
            stack.extend(store.globals.iter().flatten());
        }
        for frame in &self.frames {
            stack.extend(&frame.stack);
        }
        let mut out = Vec::new();
        while let Some(id) = stack.pop() {
            if std::mem::replace(&mut seen[id], true) {
                continue;
            }
            out.push(id);
            match &self.shadows[id] {
                Shadow::S { a, b, n } => {
                    stack.extend(a.iter().chain(n.iter()));
                    if let AnyShadow::Obj(id) = b {
                        stack.push(*id);
                    }
                }
                Shadow::A(elems) => stack.extend(elems.iter().filter_map(|e| match e {
                    AnyShadow::Obj(id) => Some(*id),
                    _ => None,
                })),
                Shadow::P { .. } => {}
                Shadow::Node { wasm } => stack.extend(wasm.iter()),
            }
        }
        out
    }

    fn new_wasm(&mut self, handle: &AllocationHandle, rng: &mut Rng, store: usize) -> usize {
        match rng.below(3) {
            0 => self.new_wasm_of(handle, store, S, 0),
            1 => self.new_wasm_of(handle, store, A, 2 + rng.below(7) as u32),
            _ => self.new_wasm_of(handle, store, P, 0),
        }
    }

    fn new_wasm_of(&mut self, handle: &AllocationHandle, store: usize, ty: u32, len: u32) -> usize {
        let id = self.shadows.len();
        // SAFETY (allocation): pushed on the running frame's stack (or
        // stored in a reachable object), which keeps it alive through the
        // Store's table.
        let obj = unsafe { Obj::new(handle, ty, len, id as u32) };
        let shadow = match ty {
            S => Shadow::S {
                a: None,
                b: AnyShadow::Null,
                n: None,
            },
            A => {
                let mut elems = vec![AnyShadow::Null; len as usize];
                elems[0] = AnyShadow::I31(id as i32);
                Shadow::A(elems)
            }
            _ => Shadow::P { x: 0.0, v: 0 },
        };
        // SAFETY: both alive; on the heap's thread.
        unsafe {
            WeakObjectTable::add_wasm(self.stores[store].table, obj.raw());
            WeakObjectTable::add_wasm(self.observer, obj.raw());
        }
        self.shadows.push(shadow);
        self.ptrs.push(Ptr::Wasm(obj));
        self.owner.push(Some(store));
        id
    }

    fn new_node(&mut self, handle: &AllocationHandle) -> usize {
        let id = self.shadows.len();
        // SAFETY: handed to the running Store or put in the document at
        // once.
        let node = unsafe { wasm::new_node(handle, id as u32) };
        // SAFETY: both alive; on the heap's thread.
        unsafe { WeakObjectTable::add_host(self.observer, &node) };
        self.shadows.push(Shadow::Node { wasm: None });
        self.ptrs.push(Ptr::Node(node));
        self.owner.push(None);
        id
    }

    /// The host hands a node to the running Store: it goes in the Store's
    /// table too, or the Store's stack would be all that holds it.
    fn hand(&mut self, node: usize) {
        let store = self.frames.last().unwrap().store;
        if self.record_handed {
            // SAFETY: both alive; on the heap's thread.
            unsafe { WeakObjectTable::add_host(self.stores[store].table, self.node(node)) };
            self.stores[store].handed.insert(node);
        }
        self.frames.last_mut().unwrap().stack.push(node);
    }

    fn enter(&mut self, store: usize) {
        // SAFETY: the table is alive (its holder is rooted).
        unsafe { WeakObjectTable::enter(self.stores[store].table) };
        self.frames.push(Frame {
            store,
            stack: Vec::new(),
        });
    }

    fn exit(&mut self) {
        let frame = self.frames.pop().unwrap();
        // SAFETY: as `enter`.
        unsafe { WeakObjectTable::exit(self.stores[frame.store].table) };
    }

    /// One random operation: by the host when no Store runs, else by the
    /// running Store (the top frame), on the values of its stack.
    fn step(&mut self, handle: &AllocationHandle, rng: &mut Rng) {
        let Some(frame) = self.frames.last() else {
            match rng.below(4) {
                0 | 1 => self.enter(rng.below(STORES)),
                2 => {
                    let slot = rng.below(DOC);
                    let node = self.new_node(handle);
                    self.set_doc(slot, Some(node));
                }
                _ => self.set_doc(rng.below(DOC), None),
            }
            return;
        };
        let store = frame.store;
        let stack = frame.stack.clone();
        let pick = |rng: &mut Rng, want: &dyn Fn(usize) -> bool| -> Option<usize> {
            (0..4)
                .filter_map(|_| stack.get(rng.below(stack.len().max(1))).copied())
                .find(|id| want(*id))
        };
        // `S`, `A`, `P`, `N` (a node) or `W` (any wasm object).
        let is_kind = |id: usize, kind: u8| {
            matches!(
                (&self.shadows[id], kind),
                (Shadow::S { .. }, b'S' | b'W')
                    | (Shadow::A(_), b'A' | b'W')
                    | (Shadow::P { .. }, b'P' | b'W')
                    | (Shadow::Node { .. }, b'N')
            )
        };
        // SAFETY (field accesses below): every object used is on the
        // running frame's stack or reachable from it, so alive; on the
        // heap's thread.
        match rng.below(26) {
            // A new wasm object.
            0..=3 => {
                let id = self.new_wasm(handle, rng, store);
                self.push(id);
            }
            // Drop a value.
            4 | 5 => {
                let frame = self.frames.last_mut().unwrap();
                if !frame.stack.is_empty() {
                    let index = rng.below(frame.stack.len());
                    frame.stack.swap_remove(index);
                }
            }
            // `global.get`.
            6 => {
                if let Some(id) = self.stores[store].globals[rng.below(GLOBALS)] {
                    self.push(id);
                }
            }
            // `global.set` (not the backbone's globals).
            7 => {
                let target = if rng.chance(20) {
                    None
                } else {
                    pick(rng, &|id| is_kind(id, b'W'))
                };
                self.set_global(store, BACKBONE + rng.below(GLOBALS - BACKBONE), target);
            }
            // `struct.get` / `array.get`.
            8..=10 => {
                let Some(src) = pick(rng, &|id| is_kind(id, b'S') || is_kind(id, b'A')) else {
                    return;
                };
                match self.shadows[src].clone() {
                    Shadow::S { a, b, n } => match rng.below(3) {
                        0 => a.into_iter().for_each(|id| self.push(id)),
                        1 => {
                            if let AnyShadow::Obj(id) = b {
                                self.push(id)
                            }
                        }
                        // A node from a wasm field: the host hands it over.
                        _ => n.into_iter().for_each(|id| self.hand(id)),
                    },
                    Shadow::A(elems) => {
                        if let AnyShadow::Obj(id) = elems[rng.below(elems.len())] {
                            self.push(id);
                        }
                    }
                    _ => unreachable!(),
                }
            }
            // `struct.set` / `array.set`.
            11..=14 => {
                let Some(src) = pick(rng, &|id| is_kind(id, b'S') || is_kind(id, b'A')) else {
                    return;
                };
                let any = |rng: &mut Rng| match rng.below(3) {
                    0 => AnyShadow::Null,
                    1 => AnyShadow::I31([0, -1, (1 << 30) - 1, -(1 << 30)][rng.below(4)]),
                    _ => pick(rng, &|id| is_kind(id, b'W')).map_or(AnyShadow::Null, AnyShadow::Obj),
                };
                let obj = self.wasm(src);
                let mut shadow = self.shadows[src].clone();
                match &mut shadow {
                    Shadow::S { a, b, n } => match rng.below(3) {
                        0 => {
                            *a = pick(rng, &|id| is_kind(id, b'W'));
                            unsafe { obj.set_ref(4, a.map(|id| self.wasm(id))) };
                        }
                        1 => {
                            *b = any(rng);
                            unsafe { obj.set_any(8, self.any(*b)) };
                        }
                        _ => {
                            *n = pick(rng, &|id| is_kind(id, b'N'));
                            unsafe { obj.set_node(12, n.map(|id| self.node(id))) };
                        }
                    },
                    Shadow::A(elems) => {
                        let index = 1 + rng.below(elems.len() - 1);
                        elems[index] = any(rng);
                        unsafe { obj.set_any(4 * index as u32, self.any(elems[index])) };
                    }
                    _ => unreachable!(),
                }
                self.shadows[src] = shadow;
            }
            // A listener: a node on the stack gets one of the Store's
            // objects (`addEventListener`).
            15 => {
                let (Some(node), target) = (
                    pick(rng, &|id| is_kind(id, b'N')),
                    pick(rng, &|id| is_kind(id, b'W')),
                ) else {
                    return;
                };
                self.shadows[node] = Shadow::Node { wasm: target };
                let raw = target.map_or(std::ptr::null_mut(), |id| self.wasm(id).raw().as_ptr());
                unsafe { self.node(node).as_ref().wasm.assign(raw) };
            }
            // `document.createElement`, sometimes attached.
            16 => {
                let node = self.new_node(handle);
                if rng.chance(30) {
                    self.set_doc(rng.below(DOC), Some(node));
                }
                self.hand(node);
            }
            // `document.querySelector`-ish: a node of the document.
            17 => {
                if let Some(node) = self.doc_nodes[rng.below(DOC)] {
                    self.hand(node);
                }
            }
            // A call into a Store (maybe the same one).
            18 => {
                if self.frames.len() < 6 {
                    self.enter(rng.below(STORES));
                }
            }
            // Take an object out of the heap: read a field (or an element)
            // onto the stack and clear it, so the stack is all that holds
            // it. Often from a global's array, which marking may not have
            // reached yet.
            20..=24 => {
                let src = if rng.chance(50) {
                    self.stores[store].globals[rng.below(BACKBONE)]
                } else {
                    pick(rng, &|id| is_kind(id, b'S') || is_kind(id, b'A'))
                };
                let Some(src) = src else { return };
                let obj = self.wasm(src);
                let mut shadow = self.shadows[src].clone();
                let taken = match &mut shadow {
                    Shadow::S { a, .. } => {
                        let taken = a.take();
                        unsafe { obj.set_ref(4, None) };
                        taken
                    }
                    Shadow::A(elems) => {
                        let index = 1 + rng.below(elems.len() - 1);
                        let AnyShadow::Obj(id) = elems[index] else {
                            return;
                        };
                        elems[index] = AnyShadow::Null;
                        unsafe { obj.set_any(4 * index as u32, Any::Null) };
                        Some(id)
                    }
                    _ => None,
                };
                self.shadows[src] = shadow;
                if let Some(id) = taken {
                    self.push(id);
                }
            }
            // Return.
            _ => self.exit(),
        }
        let frame = self.frames.last_mut();
        if let Some(frame) = frame
            && frame.stack.len() > STACK
        {
            frame.stack.remove(0);
        }
    }

    fn push(&mut self, id: usize) {
        self.frames.last_mut().unwrap().stack.push(id);
    }

    fn any(&self, value: AnyShadow) -> Any {
        match value {
            AnyShadow::Null => Any::Null,
            AnyShadow::I31(i) => Any::I31(i),
            AnyShadow::Obj(id) => Any::Obj(self.wasm(id)),
        }
    }

    fn set_global(&mut self, store: usize, index: usize, target: Option<usize>) {
        self.stores[store].globals[index] = target;
        let raw = target.map_or(std::ptr::null_mut(), |id| self.wasm(id).raw().as_ptr());
        // SAFETY: the target is alive; on the heap's thread.
        unsafe { self.stores[store].holder.get().unwrap().globals[index].assign(raw) };
    }

    fn set_doc(&mut self, slot: usize, node: Option<usize>) {
        self.doc_nodes[slot] = node;
        let doc = self.doc.get().unwrap();
        // SAFETY: the node is alive; on the heap's thread.
        unsafe {
            match node {
                Some(id) => doc.nodes[slot].assign(self.node(id)),
                None => doc.nodes[slot].clear(),
            }
        }
    }

    /// Every object the program can reach is intact.
    fn check(&self) -> usize {
        let reachable = self.reachable();
        let finalized: HashSet<u32> = FINALIZED.lock().unwrap().iter().copied().collect();
        let ptr = |id: Option<usize>| match id {
            None => std::ptr::null(),
            Some(id) => match &self.ptrs[id] {
                Ptr::Wasm(obj) => obj.as_ptr(),
                Ptr::Node(node) => node_ptr(node),
            },
        };
        let any_ok = |got: Any, want: AnyShadow| match (got, want) {
            (Any::Null, AnyShadow::Null) => true,
            (Any::I31(a), AnyShadow::I31(b)) => a == b,
            (Any::Obj(o), AnyShadow::Obj(id)) => o.as_ptr() == ptr(Some(id)),
            _ => false,
        };
        for &id in &reachable {
            // SAFETY (reads): reachable, so alive (that is the claim under
            // test); no GC during the check.
            match &self.shadows[id] {
                Shadow::Node { wasm } => {
                    assert!(
                        !finalized.contains(&(id as u32)),
                        "node {id} on a stack was finalized"
                    );
                    let node = unsafe { self.node(id).as_ref() };
                    assert_eq!(node.id(), id as u32);
                    let got = unsafe { node.wasm.get() }
                        .map_or(std::ptr::null(), |o| o.as_ptr() as *const _);
                    assert_eq!(got, ptr(*wasm), "node {id}");
                }
                shadow => {
                    let obj = self.wasm(id);
                    // The header first: a collected object's is where
                    // cppgc's free list writes.
                    let ty = match shadow {
                        Shadow::S { .. } => S,
                        Shadow::A(_) => A,
                        _ => P,
                    };
                    assert_eq!(obj.ty(), ty, "object {id}'s type");
                    assert_eq!(obj.id(), id as u32, "object {id}");
                    match shadow {
                        Shadow::S { a, b, n } => {
                            let got =
                                unsafe { obj.get_ref(4) }.map_or(std::ptr::null(), Obj::as_ptr);
                            assert_eq!(got, ptr(*a), "struct {id}.a");
                            assert!(any_ok(unsafe { obj.get_any(8) }, *b), "struct {id}.b");
                            assert_eq!(unsafe { obj.get_node(12) }, ptr(*n), "struct {id}.n");
                        }
                        Shadow::A(elems) => {
                            assert_eq!(obj.len() as usize, elems.len());
                            for (index, elem) in elems.iter().enumerate() {
                                let got = unsafe { obj.get_any(4 * index as u32) };
                                assert!(any_ok(got, *elem), "array {id}[{index}]");
                            }
                        }
                        Shadow::P { x, v } => {
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

    /// With no Store running, after full GCs: the tables hold exactly the
    /// live objects (each Store's: those it allocated and the nodes it was
    /// handed).
    fn check_exact(&self) {
        assert!(self.frames.is_empty());
        let reachable = self.check();
        let live: HashSet<usize> = self.reachable().into_iter().collect();
        // SAFETY (lengths): rooted tables; on the heap's thread.
        assert_eq!(
            unsafe { WeakObjectTable::len(self.observer) },
            reachable,
            "observer"
        );
        for (index, store) in self.stores.iter().enumerate() {
            let owned = live
                .iter()
                .filter(|id| self.owner[**id] == Some(index))
                .count();
            let handed = store.handed.iter().filter(|id| live.contains(id)).count();
            assert_eq!(
                unsafe { WeakObjectTable::len(store.table) },
                owned + handed,
                "store {index}'s table"
            );
        }
    }
}

/// GCs advanced step by step between the program's operations, each
/// finished wherever the program is (Stores running or not).
fn fuzz(concurrent: bool, seed: u64, record_handed: bool) {
    FINALIZED.lock().unwrap().clear();
    let heap = DetachedHeap::new(concurrent);
    let handle = heap.allocation_handle();
    let mut world = World::new(handle);
    world.record_handed = record_handed;
    let mut rng = Rng(seed);
    grow(&mut world, handle, &mut rng);
    let (mut steps, mut running_at_end, mut most) = (0, 0, 0);
    for _ in 0..300 {
        testing::start_incremental_gc(&heap);
        if concurrent {
            testing::set_main_thread_marking(&heap, false);
        }
        let mut gc_steps = 0;
        loop {
            for _ in 0..rng.below(40) {
                world.step(handle, &mut rng);
            }
            steps += 1;
            gc_steps += 1;
            if testing::marking_step(&heap) || gc_steps == 30 {
                break;
            }
        }
        for _ in 0..rng.below(40) {
            world.step(handle, &mut rng);
        }
        if concurrent {
            testing::set_main_thread_marking(&heap, true);
        }
        if !world.frames.is_empty() {
            running_at_end += 1;
        }
        testing::finalize_gc(&heap);
        most = most.max(world.check());
    }
    while !world.frames.is_empty() {
        world.exit();
    }
    // SAFETY: nothing unrooted on the stack.
    unsafe {
        heap.collect_garbage_for_testing(EmbedderStackState::NoHeapPointers);
        heap.collect_garbage_for_testing(EmbedderStackState::NoHeapPointers);
    }
    world.check_exact();
    println!(
        "fuzz-{}: ok, seed {seed}: 300 GCs ({running_at_end} finished while a Store ran), \
         {steps} marking steps, {} objects, at most {most} reachable",
        if concurrent {
            "concurrent"
        } else {
            "incremental"
        },
        world.shadows.len(),
    );
}

fn time<R>(f: impl FnOnce() -> R) -> (Duration, R) {
    let begin = Instant::now();
    let r = f();
    (begin.elapsed(), r)
}

/// What the table costs.
fn cost() {
    let heap = DetachedHeap::new(false);
    let handle = heap.allocation_handle();
    let world = World::new(handle);
    let roots = world.doc.get().unwrap();
    let _ = roots;
    let table = world.stores[0].table;
    const N: u32 = 1_000_000;
    // Allocation, with and without recording in the table (no GC runs:
    // nothing is collected during the loops, so nothing needs rooting).
    for record in [false, true, false, true] {
        let (took, ()) = time(|| {
            for i in 0..N {
                // SAFETY: not used after the loop; no GC runs in it.
                let obj = unsafe { Obj::new(handle, S, 0, i) };
                if record {
                    // SAFETY: both alive; on the heap's thread.
                    unsafe { WeakObjectTable::add_wasm(table, obj.raw()) };
                }
            }
        });
        println!(
            "cost: struct.new{}: {:.1} ns each",
            if record { " + recording" } else { "" },
            took.as_nanos() as f64 / N as f64
        );
        // SAFETY: nothing unrooted on the stack.
        unsafe { heap.collect_garbage_for_testing(EmbedderStackState::NoHeapPointers) };
    }
    // Entering and leaving, with no GC marking.
    let (took, ()) = time(|| {
        for _ in 0..N {
            // SAFETY: the table is rooted.
            unsafe {
                WeakObjectTable::enter(table);
                WeakObjectTable::exit(table);
            }
        }
    });
    println!(
        "cost: enter + exit: {:.1} ns",
        took.as_nanos() as f64 / N as f64
    );
    // The second trace: a table of `n` live objects, entered after it was
    // traced weakly in the same GC.
    for n in [10_000u32, 100_000] {
        let keep = world.stores[1].holder.get().unwrap();
        let table = world.stores[1].table;
        // SAFETY: each object is linked from a rooted chain (global 0)
        // before the next allocation.
        unsafe {
            let first = Obj::new(handle, S, 0, 0);
            keep.globals[0].assign(first.raw().as_ptr());
            WeakObjectTable::add_wasm(table, first.raw());
            let mut last = first;
            for i in 1..n {
                let next = Obj::new(handle, S, 0, i);
                last.set_ref(4, Some(next));
                WeakObjectTable::add_wasm(table, next.raw());
                last = next;
            }
        }
        for enter in [false, true] {
            testing::start_incremental_gc(&heap);
            let mut marking = Duration::ZERO;
            let mut entered = false;
            loop {
                let (took, done) = time(|| testing::marking_step(&heap));
                marking += took;
                if done {
                    break;
                }
                if enter && !entered {
                    // SAFETY: rooted.
                    unsafe { WeakObjectTable::enter(table) };
                    entered = true;
                }
            }
            let (finalize, ()) = time(|| testing::finalize_gc(&heap));
            if entered {
                // SAFETY: as above.
                unsafe { WeakObjectTable::exit(table) };
            }
            println!(
                "cost: GC with a {n}-entry table{}: marking {marking:.2?}, final pause {finalize:.2?}",
                if enter { ", entered mid-marking" } else { "" },
            );
        }
        // SAFETY: on the heap's thread.
        unsafe { keep.globals[0].assign(std::ptr::null_mut()) };
    }
    // SAFETY: on the heap's thread.
    println!("cost: a table entry is 4 bytes (a compressed pointer), plus the vector's slack");
}

fn main() {
    let platform = v8::new_default_platform(0, false).make_shared();
    v8::V8::initialize_platform(platform.clone());
    v8::cppgc::initialize_process(platform);
    v8::V8::initialize();
    wasm::init();
    let seed = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0x9e37_79b9_7f4a_7c15);
    match std::env::args().nth(1).as_deref() {
        Some("fuzz-incremental") => fuzz(false, seed, true),
        Some("fuzz-concurrent") => fuzz(true, seed, true),
        // Handing nodes over without recording them: expected to fail.
        Some("fuzz-unrecorded") => fuzz(false, seed, false),
        Some("cost") => cost(),
        Some("all") => {
            fuzz(false, seed, true);
            fuzz(true, seed, true);
            cost();
        }
        _ => eprintln!(
            "usage: q15-prototype fuzz-incremental|fuzz-concurrent|fuzz-unrecorded|cost|all [seed]"
        ),
    }
}
