//! The guests' wasm GC objects in the DOM's heap: a store of the C guest
//! (which calls the DOM, and has no GC heap) and a store of a module with
//! GC types (`WAT`, its object in a global), in one `StoreSet`. The GCs the
//! guest's DOM work makes due run in its calls (through its store) and keep
//! the other store's object, and a call with none due runs none (one due
//! runs in the next call that may allocate); that store's own GCs keep the nodes the guest
//! holds; none runs while another store is lent; the DOM's full GC between
//! calls keeps both.

use dom_wasm::WasmDom;
use wasmtime::{Instance, Module, Store};

use crate::guest::{Data, Guest, Set, engine, set};

const WAT: &str = r#"
    (module
        (type $s (struct (field (mut i64)) (field (mut anyref))))
        (global $g (mut anyref) (ref.null any))
        (func (export "set_global") (param $v i64)
            (global.set $g (struct.new $s (local.get $v) (ref.null any))))
        (func (export "global") (result i64)
            (struct.get $s 0 (ref.cast (ref $s) (global.get $g))))
        ;; `n` structs, garbage.
        (func (export "allocate") (param $n i32)
            (block $done
                (loop $more
                    (br_if $done (i32.eqz (local.get $n)))
                    (drop (struct.new $s (i64.const 0) (ref.null any)))
                    (local.set $n (i32.sub (local.get $n) (i32.const 1)))
                    (br $more))))
    )
"#;

/// A store of `WAT` in `set`, its global's struct holding `value`.
struct Objects {
    set: Set,
    index: usize,
    instance: Instance,
}

impl Objects {
    fn new(set: &Set, value: i64) -> Self {
        let module = Module::new(engine(), WAT).unwrap();
        let index = set
            .borrow_mut()
            .insert(Store::new(engine(), Data::new()))
            .unwrap();
        let instance = set
            .borrow()
            .with(index, |mut cx| {
                let instance = Instance::new(&mut cx, &module, &[]).unwrap();
                let set_global = instance
                    .get_typed_func::<i64, ()>(&mut cx, "set_global")
                    .unwrap();
                set_global.call(&mut cx, value).unwrap();
                instance
            })
            .unwrap();
        Self {
            set: set.clone(),
            index,
            instance,
        }
    }

    /// The value of the struct its global holds.
    fn global(&self) -> i64 {
        let instance = self.instance;
        self.set
            .borrow()
            .with(self.index, |mut cx| {
                let global = instance.get_typed_func::<(), i64>(&mut cx, "global")?;
                global.call(&mut cx, ())
            })
            .unwrap()
            .unwrap()
    }

    /// It allocates `n` structs.
    fn allocate(&self, n: i32) {
        let instance = self.instance;
        self.set
            .borrow()
            .with(self.index, |mut cx| {
                let allocate = instance.get_typed_func::<i32, ()>(&mut cx, "allocate")?;
                allocate.call(&mut cx, n)
            })
            .unwrap()
            .unwrap();
    }
}

/// The guest runs `case` until the DOM's heap ran a scavenge since
/// `scavenges`.
fn until_a_scavenge(dom: &WasmDom, guest: &mut Guest, case: &str) {
    let scavenges = dom.heap_stats().scavenges;
    for _ in 0..100_000 {
        if dom.heap_stats().scavenges > scavenges {
            return;
        }
        guest.run(case);
    }
    panic!("no scavenge");
}

#[test]
fn the_guests_dom_work_gcs_and_keeps_another_stores_objects() {
    let dom = WasmDom::new();
    let set = set(&dom);
    let objects = Objects::new(&set, 7);
    let mut guest = Guest::new(&set);
    until_a_scavenge(&dom, &mut guest, "tree");
    assert_eq!(objects.global(), 7);
    // A call with no GC due runs none.
    let stats = dom.heap_stats();
    guest.run("strings");
    let after = dom.heap_stats();
    assert_eq!((after.gcs, after.scavenges), (stats.gcs, stats.scavenges));
}

/// The guest's DOM work while the objects' store is lent, until a GC is
/// due (none runs).
fn until_due(dom: &WasmDom, set: &Set, objects: &Objects, guest: &mut Guest) {
    set.borrow()
        .with(objects.index, |_| {
            for _ in 0..100_000 {
                if dom.gc_due() {
                    return;
                }
                guest.run("tree");
            }
            panic!("no GC due");
        })
        .unwrap();
}

/// A GC due runs in the guest's next host call that may allocate: a
/// member's (`read-kept` calls one), `create-document` (`stale` calls it,
/// then a member that fails first).
#[test]
fn a_gc_due_runs_in_the_next_host_call() {
    let dom = WasmDom::new();
    let set = set(&dom);
    let objects = Objects::new(&set, 7);
    let mut guest = Guest::new(&set);
    guest.run("keep");
    until_due(&dom, &set, &objects, &mut guest);
    let scavenges = dom.heap_stats().scavenges;
    guest.run("read-kept");
    assert!(dom.heap_stats().scavenges > scavenges, "a member's");
    until_due(&dom, &set, &objects, &mut guest);
    let scavenges = dom.heap_stats().scavenges;
    guest.try_run("stale").unwrap_err();
    assert!(dom.heap_stats().scavenges > scavenges, "create-document's");
    assert_eq!(objects.global(), 7);
}

#[test]
fn a_stores_gcs_keep_the_nodes_the_guest_holds() {
    let dom = WasmDom::new();
    let set = set(&dom);
    let objects = Objects::new(&set, 7);
    let mut guest = Guest::new(&set);
    guest.run("keep");
    let scavenges = dom.heap_stats().scavenges;
    objects.allocate(100_000);
    assert!(dom.heap_stats().scavenges > scavenges);
    guest.run("read-kept");
    assert_eq!(guest.held(), 3);
    assert_eq!(objects.global(), 7);
}

#[test]
fn no_gc_runs_while_another_store_is_lent() {
    let dom = WasmDom::new();
    let set = set(&dom);
    let objects = Objects::new(&set, 7);
    let mut guest = Guest::new(&set);
    let scavenges = dom.heap_stats().scavenges;
    // The guest's DOM work, the objects' store lent meanwhile: no GC.
    set.borrow()
        .with(objects.index, |_| {
            for _ in 0..2000 {
                guest.run("tree");
            }
        })
        .unwrap();
    assert_eq!(dom.heap_stats().scavenges, scavenges);
    until_a_scavenge(&dom, &mut guest, "tree");
    assert_eq!(objects.global(), 7);
}

#[test]
fn the_doms_full_gc_between_calls_keeps_both() {
    let dom = WasmDom::new();
    let set = set(&dom);
    let objects = Objects::new(&set, 7);
    let mut guest = Guest::new(&set);
    guest.run("keep");
    let gcs = dom.heap_stats().gcs;
    dom.collect();
    assert_eq!(dom.heap_stats().gcs, gcs + 1);
    guest.run("read-kept");
    assert_eq!(objects.global(), 7);
}
