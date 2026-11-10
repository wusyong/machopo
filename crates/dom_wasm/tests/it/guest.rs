//! A C guest (`tests/guests/dom.c`, built by `ci.py guests`) calls
//! the DOM through `machopo:dom`: each case runs in a store of its
//! own (a trap ends a store), and says what went wrong, or traps. The
//! stores are a `StoreSet`'s, borrowing the DOM's heap, and async (as a
//! page's are): each call is a future polled here until it is done, in one
//! lend of the store.

use std::cell::RefCell;
use std::future::Future;
use std::path::Path;
use std::pin::pin;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::task::{Context, Poll, Waker};

use dom_wasm::{GuestClock, GuestClockView, Output, WasmDom, WasmStore, WasmWasi};
use wasmtime::component::{Component, Linker, TypedFunc};
use wasmtime::{Config, Engine, Store, StoreSet};
use wasmtime_wasi::{WasiCtxView, WasiView};

/// A store's data: WASI 0.3's (for the guest's libc) and the DOM's.
pub(crate) struct Data {
    wasi: WasmWasi,
    pub(crate) dom: WasmStore,
    /// The time the guest's clock reads (nanoseconds), the test's to move.
    pub(crate) time: Arc<AtomicU64>,
    /// The lines the guest printed.
    pub(crate) printed: Arc<Mutex<Vec<(Output, String)>>>,
}

impl Data {
    pub(crate) fn new() -> Self {
        // Not at zero, nor on the clock's resolution.
        let time = Arc::new(AtomicU64::new(1_234_567_890));
        let printed = Arc::new(Mutex::new(Vec::new()));
        let (now, sink) = (time.clone(), printed.clone());
        Self {
            wasi: WasmWasi::new(
                move || now.load(Ordering::Relaxed),
                Arc::new(move |output, line: &str| {
                    sink.lock().unwrap().push((output, line.to_owned()));
                }),
            ),
            dom: WasmStore::new(),
            time,
            printed,
        }
    }
}

impl WasiView for Data {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        self.wasi.view()
    }
}

impl GuestClockView for Data {
    fn guest_clock(&mut self) -> &mut GuestClock {
        self.wasi.clock()
    }
}

/// Polls `future` until it is done: nothing a test's guest waits for comes
/// from outside its store, so each poll moves it on (one that never does
/// fails the test rather than hang it).
pub(crate) fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    for _ in 0..1_000_000 {
        if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
            return output;
        }
    }
    panic!("the guest's call never finished");
}

/// The engine (`dom_wasm::configure`d), made once.
pub(crate) fn engine() -> &'static Engine {
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    ENGINE.get_or_init(|| {
        let mut config = Config::new();
        dom_wasm::configure(&mut config);
        Engine::new(&config).unwrap()
    })
}

/// The linker and the guest, made once (the component compiles once for
/// every test).
fn guest() -> &'static (Linker<Data>, Component) {
    static GUEST: OnceLock<(Linker<Data>, Component)> = OnceLock::new();
    GUEST.get_or_init(|| {
        let engine = engine();
        let mut linker = Linker::new(engine);
        dom_wasm::add_wasi_to_linker(&mut linker).unwrap();
        dom_wasm::add_to_linker(&mut linker, |data: &Data| &data.dom).unwrap();
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/guests/dom.wasm");
        let component = Component::from_file(engine, path).unwrap();
        (linker, component)
    })
}

/// The thread's DOM's stores.
pub(crate) type Set = Rc<RefCell<StoreSet<Data>>>;

pub(crate) fn set(dom: &WasmDom) -> Set {
    Rc::new(RefCell::new(dom.store_set(engine()).unwrap()))
}

/// A store of the guest, in a set, and its `run`; removed from the set as
/// it goes.
pub(crate) struct Guest {
    pub(crate) set: Set,
    pub(crate) index: usize,
    run: TypedFunc<(String,), (String,)>,
    pub(crate) sleep: TypedFunc<(u64,), ()>,
}

impl Guest {
    /// A new store of the guest, in the thread's DOM.
    pub(crate) fn new(set: &Set) -> Self {
        let (linker, component) = guest();
        let index = set
            .borrow_mut()
            .insert(Store::new(engine(), Data::new()))
            .unwrap();
        let (run, sleep) = set
            .borrow()
            .with(index, |mut cx| {
                let instance = block_on(linker.instantiate_async(&mut cx, component)).unwrap();
                (
                    instance.get_typed_func(&mut cx, "run").unwrap(),
                    instance.get_typed_func(&mut cx, "sleep").unwrap(),
                )
            })
            .unwrap();
        Self {
            set: set.clone(),
            index,
            run,
            sleep,
        }
    }

    /// Runs `case`: what went wrong (empty if nothing), or the trap.
    pub(crate) fn try_run(&mut self, case: &str) -> wasmtime::Result<String> {
        let run = self.run;
        let (failure,) = self.set.borrow().with(self.index, |mut cx| {
            block_on(run.call_async(&mut cx, (case.to_owned(),)))
        })??;
        Ok(failure)
    }

    pub(crate) fn run(&mut self, case: &str) {
        let failure = self
            .try_run(case)
            .unwrap_or_else(|trap| panic!("{case} trapped: {trap:?}"));
        assert!(failure.is_empty(), "{case}: {failure}");
    }

    pub(crate) fn held(&self) -> usize {
        self.set
            .borrow()
            .with(self.index, |cx| cx.data().dom.held())
            .unwrap()
    }
}

impl Drop for Guest {
    fn drop(&mut self) {
        let _ = self.set.borrow_mut().remove(self.index);
    }
}

/// Making, inserting, walking and removing nodes; a node is the same
/// number each time; the guest let each go.
#[test]
fn a_guest_builds_and_walks_a_tree() {
    let dom = WasmDom::new();
    let set = set(&dom);
    let mut guest = Guest::new(&set);
    guest.run("tree");
    assert_eq!(guest.held(), 0);
}

/// Strings both ways: attributes (ASCII and not), names, a text's data,
/// `[LegacyNullToEmptyString]`'s none, `textContent`'s.
#[test]
fn strings_cross_both_ways() {
    let dom = WasmDom::new();
    let set = set(&dom);
    Guest::new(&set).run("strings");
}

/// The DOM's errors are their codes; `this` of another interface is a
/// `TypeError` (code 1), not a trap: the store goes on.
#[test]
fn errors_are_codes_and_another_interfaces_this_a_type_error() {
    let dom = WasmDom::new();
    let set = set(&dom);
    let mut guest = Guest::new(&set);
    guest.run("errors");
    guest.run("type-errors");
    guest.run("tree");
}

/// A released node's slot gives the next node another number.
#[test]
fn a_released_handles_slot_gives_another_number() {
    let dom = WasmDom::new();
    let set = set(&dom);
    Guest::new(&set).run("release");
}

/// What the guest keeps is held, past a GC, until it releases it; then
/// the store holds nothing.
#[test]
fn kept_handles_hold_their_nodes_until_released() {
    let dom = WasmDom::new();
    let set = set(&dom);
    let mut guest = Guest::new(&set);
    guest.run("keep");
    assert_eq!(guest.held(), 3);
    dom.collect();
    guest.run("read-kept");
    guest.run("release-kept");
    assert_eq!(guest.held(), 0);
}

/// A handle that isn't one of the store's traps the call: released, never
/// given, 0, released again; so does a string that isn't UTF-8.
#[test]
fn a_handle_not_the_stores_and_a_string_not_utf8_trap() {
    let dom = WasmDom::new();
    let set = set(&dom);
    for case in [
        "stale",
        "never-given",
        "zero",
        "release-twice",
        "release-never-given",
        "bad-utf8",
    ] {
        let mut guest = Guest::new(&set);
        let trap = guest.try_run(case).expect_err(case);
        let message = format!("{trap:?}");
        if case == "bad-utf8" {
            assert!(
                message.contains("utf-8") || message.contains("UTF-8"),
                "{case}: {message}"
            );
        } else {
            assert!(
                message.contains("no handle of this store"),
                "{case}: {message}"
            );
        }
    }
}

/// Two stores of one DOM: each its own numbers, each's store holding its
/// own; the second's (not the main one) in the shared entries, which it
/// leaves as it goes; the first going leaves the second's.
#[test]
fn stores_have_their_own_handles() {
    let dom = WasmDom::new();
    let set = set(&dom);
    let mut first = Guest::new(&set);
    let mut second = Guest::new(&set);
    first.run("keep");
    second.run("keep");
    assert_eq!((first.held(), second.held()), (3, 3));
    assert_eq!(dom.shared_entries(), 3, "the second store's");
    drop(first);
    assert_eq!(second.held(), 3);
    second.run("read-kept");
    drop(second);
    assert_eq!(dom.shared_entries(), 0);
    let mut third = Guest::new(&set);
    third.run("keep");
    assert_eq!(dom.shared_entries(), 3, "no store is main after the first");
    third.run("release-kept");
    assert_eq!((third.held(), dom.shared_entries()), (0, 0));
}

/// A store outliving its DOM calls into none (its calls trap), and leaves
/// the thread's next DOM alone as it goes: the next DOM's first store has
/// the same number its first had.
#[test]
fn a_store_outliving_its_dom_leaves_the_next_one_alone() {
    let dom = WasmDom::new();
    let old_set = set(&dom);
    let mut old = Guest::new(&old_set);
    old.run("keep");
    drop(dom);
    let trap = old
        .try_run("tree")
        .expect_err("a call into a DOM that's gone");
    assert!(
        format!("{trap:?}").contains("the store's DOM is gone"),
        "{trap:?}"
    );
    let dom = WasmDom::new();
    let set = set(&dom);
    // Its DOM is gone still: the thread's new one isn't it.
    let trap = old
        .try_run("tree")
        .expect_err("a call into a DOM that's gone");
    assert!(
        format!("{trap:?}").contains("the store's DOM is gone"),
        "{trap:?}"
    );
    let mut new = Guest::new(&set);
    new.run("keep");
    drop(old);
    assert_eq!(new.held(), 3);
    new.run("read-kept");
}

/// An element's own interface's members (an input's value and checked,
/// its style), told by the handle's interface: another element for `this`
/// is a `TypeError`. Objects that aren't nodes (a class list, a style's
/// declarations, a collection, a node list): a handle each, the same each
/// time; their members, with any number of strings and an optional boolean.
#[test]
fn a_guest_holds_objects_of_any_interface() {
    let dom = WasmDom::new();
    let set = set(&dom);
    for case in ["input", "class-list", "style", "lists"] {
        let mut guest = Guest::new(&set);
        guest.run(case);
    }
}

/// A handle to an object that isn't a node, where a node is taken, traps:
/// a binding's mistake, as a handle that isn't the store's.
#[test]
fn an_object_that_isnt_a_node_isnt_taken_as_one() {
    let dom = WasmDom::new();
    let set = set(&dom);
    let mut guest = Guest::new(&set);
    let trap = guest.try_run("object-as-node").expect_err("it traps");
    let message = format!("{trap:?}");
    assert!(
        message.contains("is a DOMTokenList's handle, not a node's"),
        "{message}"
    );
}
