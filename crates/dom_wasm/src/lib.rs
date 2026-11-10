//! The DOM (`dom`, `dom_api`) for wasm guests, on wasmtime:
//! the host functions of `machopo:dom` (`wit`), each member's typed
//! (`front`), and each store's handles to the DOM's nodes (`handles`)
//! (Blink's `bindings/core/v8` is the closest: the engine's side of the
//! DOM's interfaces). No renderer runs it yet: machopo has no wasm documents.
//!
//! A thread's wasm engine has one DOM (`WasmDom`), whose heap no JS engine
//! drives: its own GC, on elise's pool of workers. The guests' wasm GC
//! objects live in it too: the engine (`configure`) has its stores borrow
//! the DOM's heap (wasmtime's `EliseHeapProvider`, from the thread-local
//! below), and they are a `StoreSet`'s (`WasmDom::store_set`), which gives
//! the heap their roots. Each guest's store has a `WasmStore` in its data,
//! its handles' table in the DOM's state. The state is the thread's (a
//! thread-local the `WasmDom` installs), not a store's: a store's data must
//! be `Send` (an async store's is), the DOM's heap is the thread's, and the
//! host functions are the linker's.
//!
//! No `unsafe` (`#![forbid(unsafe_code)]`).

#![forbid(unsafe_code)]
#![cfg_attr(
    test,
    allow(
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap,
        reason = "tests"
    )
)]

use std::cell::RefCell;
use std::sync::Arc;

use dom::Dom;
use dom_api::MemberKind;
use wasmtime::component::Linker;
use wasmtime::{Config, EliseHeapProvider, Engine, StoreContextMut, StoreSet};

mod engine;
mod front;
mod handles;
mod turn;
mod wasi;
pub mod wit;

pub use engine::WasmEngine;
pub use front::{TYPE_ERROR, exception_code};
pub use turn::poll_until_idle;
pub use wasi::{
    CLOCK_RESOLUTION, GuestClock, GuestClockView, Output, OutputSink, WasmWasi, add_wasi_to_linker,
};

use handles::Tables;

/// A thread's DOM and its stores' handles.
struct State {
    /// Which of the thread's `WasmDom`s it is: a store outliving its DOM
    /// leaves the next one alone.
    serial: u64,
    // Before the DOM: its holders are roots of the DOM's heap.
    tables: RefCell<Tables>,
    /// The DOM, while it is here: an engine that owns it (a page's) takes
    /// it (`WasmDom::take`) and lends it for each turn of its guests
    /// (`WasmDom::lend`), as the page's own work needs the `Dom` itself.
    dom: Option<RefCell<Dom>>,
}

impl State {
    /// The DOM, which is here (`Slot::of` gave the state).
    fn dom(&self) -> &RefCell<Dom> {
        self.dom.as_ref().expect("the DOM, lent")
    }
}

/// The thread's DOM, if it has one, and how many it made (each one's
/// serial).
struct Slot {
    made: u64,
    state: Option<State>,
}

impl Slot {
    /// The state of the store `key`, if the thread's DOM is still its,
    /// and here (not with its engine, between its guests' turns).
    fn of(&self, key: Key) -> Option<&State> {
        self.state
            .as_ref()
            .filter(|state| state.serial == key.dom && state.dom.is_some())
    }
}

thread_local! {
    static STATE: RefCell<Slot> = const { RefCell::new(Slot { made: 0, state: None }) };
}

fn no_store() -> wasmtime::Error {
    wasmtime::format_err!("the store's DOM is gone")
}

/// Sets `config` up for the thread's DOM's guests: wasm's GC, its objects
/// in the DOM's heap (elise, the stores borrowing the heap the thread's
/// `WasmDom` lends).
pub fn configure(config: &mut Config) {
    config.wasm_function_references(true);
    config.wasm_gc(true);
    config.collector(wasmtime::Collector::Elise);
    config.elise_heap_provider(Arc::new(DomHeap));
}

/// Lends the thread's DOM's heap to its stores, the DOM's serial its key:
/// as Blink finds cppgc's heap through the thread's `ThreadState`. A
/// store whose DOM is gone, or on another thread, finds none.
struct DomHeap;

impl EliseHeapProvider for DomHeap {
    fn key(&self) -> wasmtime::Result<u64> {
        STATE
            .with_borrow(|slot| slot.state.as_ref().map(|state| state.serial))
            .ok_or_else(no_store)
    }

    fn with_heap(&self, key: u64, f: &mut dyn FnMut(&mut elise::Heap)) -> wasmtime::Result<()> {
        STATE.with(|slot| {
            let busy = || wasmtime::format_err!("the DOM is in use");
            let slot = slot.try_borrow().map_err(|_| busy())?;
            let state = slot
                .state
                .as_ref()
                .filter(|state| state.serial == key)
                .ok_or_else(no_store)?;
            let dom = state.dom.as_ref().ok_or_else(no_store)?;
            let mut dom = dom.try_borrow_mut().map_err(|_| busy())?;
            f(dom.heap_mut());
            Ok(())
        })
    }
}

/// After a host function's DOM operation: the GC the heap is due to run,
/// through the store, whose wasm frames' roots wasmtime gives (the heap's
/// own safepoints don't run it while a store is lent: it can't see them).
fn collect_if_due<T>(cx: &mut StoreContextMut<'_, T>) -> wasmtime::Result<()> {
    let due = STATE.with_borrow(|slot| {
        slot.state
            .as_ref()
            .and_then(|state| state.dom.as_ref())
            .is_some_and(|dom| dom.try_borrow().is_ok_and(|dom| dom.should_collect()))
    });
    if due {
        cx.gc(None)?;
    }
    Ok(())
}

/// The thread's DOM for wasm guests (Blink's `V8PerIsolateData`'s DOM, a
/// thread's): made with the engine, gone with it.
pub struct WasmDom(());

impl WasmDom {
    /// # Panics
    ///
    /// If the thread has one already.
    pub fn new() -> Self {
        STATE.with_borrow_mut(|slot| {
            assert!(slot.state.is_none(), "a thread's one new DOM");
            slot.made = slot.made.checked_add(1).expect("2^64 DOMs");
            slot.state = Some(State {
                serial: slot.made,
                tables: RefCell::default(),
                dom: Some(RefCell::new(Dom::new())),
            });
        });
        Self(())
    }

    /// Takes the DOM out, for the engine that owns it: the page's work
    /// (style, layout, the parser) uses the `Dom` itself, and the guests
    /// reach it only in `lend`. Without it here a guest's call fails, a
    /// store can't be made, and one dropped leaves its table behind (and,
    /// with a GC heap, a root of the DOM's heap): the stores and their set
    /// are made and dropped in `lend`.
    ///
    /// # Panics
    ///
    /// If it is out already, or in use.
    pub fn take(&self) -> Dom {
        STATE.with_borrow_mut(|slot| {
            let state = slot.state.as_mut().expect("the thread's DOM");
            state.dom.take().expect("the DOM, here").into_inner()
        })
    }

    /// Puts the DOM its engine holds (`dom`, taken with `take`) back for
    /// `f`, a turn of its guests or the making and dropping of their
    /// stores, and hands it back after (if `f` unwinds too).
    ///
    /// # Panics
    ///
    /// If `dom` holds none, or the DOM is here already.
    pub fn lend<R>(&self, dom: &mut Option<Dom>, f: impl FnOnce() -> R) -> R {
        /// Hands the DOM back as it goes.
        struct Lent<'a>(&'a mut Option<Dom>);
        impl Drop for Lent<'_> {
            fn drop(&mut self) {
                *self.0 = STATE.with_borrow_mut(|slot| {
                    let state = slot.state.as_mut()?;
                    Some(state.dom.take()?.into_inner())
                });
            }
        }
        let lent = dom.take().expect("the engine's DOM, not lent");
        STATE.with_borrow_mut(|slot| {
            let state = slot.state.as_mut().expect("the thread's DOM");
            assert!(state.dom.is_none(), "the DOM is here already");
            state.dom = Some(RefCell::new(lent));
        });
        let _lent = Lent(dom);
        f()
    }

    /// The DOM's heap's counts.
    pub fn heap_stats(&self) -> elise::HeapStats {
        with_state(|state| state.dom().borrow().stats())
    }

    /// Whether the DOM's heap is due to run a GC, for the tests.
    pub fn gc_due(&self) -> bool {
        with_state(|state| state.dom().borrow().should_collect())
    }

    /// A full GC of the DOM's heap, between calls.
    pub fn collect(&self) {
        with_state(|state| state.dom().borrow_mut().collect());
    }

    /// A set for the DOM's guests' stores (of an engine `configure` set
    /// up): the stores borrow its heap through it.
    ///
    /// # Errors
    ///
    /// If `engine` isn't `configure`d, or the DOM has a set already.
    pub fn store_set<T: 'static>(&self, engine: &Engine) -> wasmtime::Result<StoreSet<T>> {
        StoreSet::new(engine)
    }

    /// The entries of the stores' handles shared in the DOM (each of a node
    /// a store but the main one holds), for the tests.
    pub fn shared_entries(&self) -> usize {
        with_state(|state| state.tables.borrow().shared_entries())
    }
}

impl Default for WasmDom {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for WasmDom {
    fn drop(&mut self) {
        // The stores' tables first: their holders are the heap's roots.
        if let Some(state) = STATE.with_borrow_mut(|slot| slot.state.take()) {
            drop(state.tables);
        }
    }
}

fn with_state<R>(f: impl FnOnce(&State) -> R) -> R {
    STATE.with_borrow(|slot| f(slot.state.as_ref().expect("the thread's DOM")))
}

/// A store's number in its DOM, and the DOM's serial.
#[derive(Clone, Copy)]
struct Key {
    number: u32,
    dom: u64,
}

/// A store's place in the thread's DOM: its data holds it, and its table
/// goes with it. Its number isn't used again.
pub struct WasmStore {
    key: Key,
}

impl WasmStore {
    /// # Panics
    ///
    /// With no `WasmDom` on the thread, or its DOM not here (an engine's,
    /// outside `WasmDom::lend`).
    pub fn new() -> Self {
        let key = with_state(|state| {
            let mut tables = state.tables.borrow_mut();
            let number = state.dom().borrow_mut().mutate(|cx| tables.add_store(cx));
            Key {
                number,
                dom: state.serial,
            }
        });
        Self { key }
    }

    /// This store's handle to `node`, for its guest (the page's document,
    /// which its `start` takes): the same each time, as a member's result.
    ///
    /// # Errors
    ///
    /// If the store's DOM is gone, or away.
    pub fn give(&self, node: &dom::NodeRoot) -> wasmtime::Result<u32> {
        let number = self.key.number;
        STATE.with_borrow(|slot| {
            let state = slot.of(self.key).ok_or_else(no_store)?;
            let mut tables = state.tables.borrow_mut();
            tables.store(number).ok_or_else(no_store)?;
            Ok(state
                .dom()
                .borrow_mut()
                .mutate(|cx| tables.give(cx, number, node.get(cx.gc).into())))
        })
    }

    /// How many nodes the store holds, for the tests: 0 once its DOM is
    /// gone.
    pub fn held(&self) -> usize {
        STATE.with_borrow(|slot| {
            slot.of(self.key).map_or(0, |state| {
                state
                    .tables
                    .borrow()
                    .store(self.key.number)
                    .map_or(0, handles::Handles::held)
            })
        })
    }
}

impl Default for WasmStore {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for WasmStore {
    fn drop(&mut self) {
        STATE.with_borrow(|slot| {
            if let Some(state) = slot.of(self.key) {
                state.tables.borrow_mut().remove_store(self.key.number);
            }
        });
    }
}

/// Adds `machopo:dom`'s functions to `linker`, for stores whose
/// `WasmStore` `store` finds in their data.
///
/// # Errors
///
/// If one is in the linker already.
pub fn add_to_linker<T: 'static>(
    linker: &mut Linker<T>,
    store: fn(&T) -> &WasmStore,
) -> wasmtime::Result<()> {
    let mut instance = linker.instance(wit::INTERFACE)?;
    instance.func_wrap(
        "create-document",
        move |mut cx: StoreContextMut<'_, T>, (): ()| {
            let key = store(cx.data()).key;
            let number = key.number;
            let handle = STATE.with_borrow(|slot| -> wasmtime::Result<u32> {
                let state = slot.of(key).ok_or_else(no_store)?;
                let mut tables = state.tables.borrow_mut();
                tables.store(number).ok_or_else(no_store)?;
                Ok(state.dom().borrow_mut().mutate(|cx| {
                    let document = cx.create_document(true);
                    tables.give(cx, number, document.into())
                }))
            })?;
            collect_if_due(&mut cx)?;
            Ok((handle,))
        },
    )?;
    instance.func_wrap(
        "release",
        move |cx: StoreContextMut<'_, T>, (handle,): (u32,)| {
            let key = store(cx.data()).key;
            let number = key.number;
            STATE.with_borrow(|slot| {
                let state = slot.of(key).ok_or_else(no_store)?;
                let mut tables = state.tables.borrow_mut();
                let handles = tables.store(number).ok_or_else(no_store)?;
                if handles.interface(handle).is_none() {
                    return Err(wasmtime::format_err!(
                        "{handle:#x} is no handle of this store"
                    ));
                }
                // No allocation: no safepoint.
                state
                    .dom()
                    .borrow_mut()
                    .read(|cx| tables.release(cx, number, handle));
                Ok(())
            })
        },
    )?;
    // The plain members only (`wit::is_plain`): the others take or give
    // what a guest can't hold yet (an object besides a node, a
    // dictionary); and of them the frozen ones (`wit::FROZEN`).
    macro_rules! member {
        ($interface:ident, $member:ident, $kind:ident, $path:ty, object) => {};
        ($interface:ident, $member:ident, $kind:ident, $path:ty, plain) => {
            let name = wit::function_name(
                stringify!($interface),
                stringify!($member),
                MemberKind::$kind,
            );
            if wit::is_frozen(&name) {
                front::add::<T, $path>(&mut instance, &name, store)?;
            }
        };
    }
    dom_api::for_each_member!(member);
    Ok(())
}
