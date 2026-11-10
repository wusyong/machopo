//! A DOM its engine owns (a page's): taken out of the thread's place for
//! the engine's own work, lent back for the guests' turns. The guests
//! reach it only while it is lent; the heap's GCs outside (the engine's
//! safepoints) keep what the stores hold.

use std::panic::{AssertUnwindSafe, catch_unwind};

use dom_wasm::WasmDom;

use crate::guest::{Guest, set};
use crate::shared::Objects;

#[test]
fn a_guest_reaches_the_dom_only_while_it_is_lent() {
    let wasm = WasmDom::new();
    let mut dom = Some(wasm.take());
    // The engine's own work on it, with no guest.
    let made = dom.as_mut().unwrap().mutate(|cx| {
        let document = cx.create_document(true);
        cx.root(document)
    });
    let (set, mut guest) = wasm.lend(&mut dom, || {
        let set = set(&wasm);
        let mut guest = Guest::new(&set);
        guest.run("tree");
        guest.run("keep");
        (set, guest)
    });
    assert!(dom.is_some(), "handed back");
    // Not lent: the guest's call fails, and its store goes on after.
    let failed = guest.try_run("tree").unwrap_err();
    assert!(format!("{failed:?}").contains("DOM is gone"), "{failed:?}");
    wasm.lend(&mut dom, || guest.run("read-kept"));
    drop(made);
    wasm.lend(&mut dom, || {
        drop(guest);
        drop(set);
    });
}

/// The engine's GCs of the DOM it holds keep the nodes a guest holds and
/// another store's objects.
#[test]
fn the_engines_gcs_keep_what_the_stores_hold() {
    let wasm = WasmDom::new();
    let mut dom = Some(wasm.take());
    let (set, objects, mut guest) = wasm.lend(&mut dom, || {
        let set = set(&wasm);
        let objects = Objects::new(&set, 7);
        let mut guest = Guest::new(&set);
        guest.run("keep");
        (set, objects, guest)
    });
    // A full GC at the engine's safepoint: the set gives the stores' roots.
    let collect = |dom: &mut Option<dom::Dom>| {
        let dom = dom.as_mut().unwrap();
        let gcs = dom.stats().gcs;
        dom.collect();
        assert_eq!(dom.stats().gcs, gcs + 1);
    };
    collect(&mut dom);
    wasm.lend(&mut dom, || {
        guest.run("read-kept");
        assert_eq!(objects.global(), 7);
        guest.run("release-kept");
        assert_eq!(guest.held(), 0);
    });
    collect(&mut dom);
    wasm.lend(&mut dom, || {
        assert_eq!(objects.global(), 7);
        // The stores go while the DOM is lent, the set with them: a store
        // with a GC heap lets go of the DOM's heap as it drops.
        drop(guest);
        drop(objects);
        drop(set);
    });
}

/// A turn that unwinds hands the DOM back.
#[test]
fn a_lend_that_unwinds_hands_the_dom_back() {
    let wasm = WasmDom::new();
    let mut dom = Some(wasm.take());
    let unwound = catch_unwind(AssertUnwindSafe(|| {
        wasm.lend(&mut dom, || panic!("in a turn"));
    }));
    assert!(unwound.is_err());
    assert!(dom.is_some());
    wasm.lend(&mut dom, || ());
}

#[test]
#[should_panic(expected = "the DOM is here already")]
fn a_dom_here_isnt_lent_again() {
    let wasm = WasmDom::new();
    let mut other = Some(dom::Dom::new());
    wasm.lend(&mut other, || ());
}

/// A store dropped with the DOM away (not as its engine should) leaves
/// its table, which goes with the `WasmDom`, before the DOM does.
#[test]
fn a_store_dropped_with_the_dom_away_leaves_its_table() {
    let wasm = WasmDom::new();
    let mut dom = Some(wasm.take());
    let (set, guest) = wasm.lend(&mut dom, || {
        let set = set(&wasm);
        let mut guest = Guest::new(&set);
        guest.run("keep");
        (set, guest)
    });
    drop(guest);
    wasm.lend(&mut dom, || drop(set));
    // The table's holder is a root of the DOM's heap: first.
    drop(wasm);
    drop(dom);
}
