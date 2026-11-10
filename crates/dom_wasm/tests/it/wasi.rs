//! The guest's WASI: a wait is a deadline of the store's clock, which the
//! embedder reads and runs the store again at, the guest's task kept in
//! the store between the turns; the clock reads to its resolution; the
//! output comes a line at a time.

use std::pin::pin;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::task::Poll;

use dom_wasm::{CLOCK_RESOLUTION, GuestClockView, Output, WasmDom, poll_until_idle};

use crate::guest::{Guest, set};

/// The time the guest's clock starts at (`Data::new`).
const START: u64 = 1_234_567_890;

impl Guest {
    /// A turn of the store that starts `sleep(nanos)`: whether it finished.
    fn start_sleep(&self, nanos: u64) -> bool {
        let sleep = self.sleep;
        self.set
            .borrow()
            .with(self.index, |cx| {
                let turn = pin!(cx.run_concurrent(async |accessor| {
                    sleep.call_concurrent(accessor, (nanos,)).await
                }));
                match poll_until_idle(turn) {
                    Poll::Ready(result) => {
                        result.unwrap().unwrap();
                        true
                    }
                    Poll::Pending => false,
                }
            })
            .unwrap()
    }

    /// A turn of the store with nothing new for it, as the embedder runs
    /// one when a deadline has come: the due waits woken, then polled
    /// until nothing moves. Whether any wait was due.
    fn turn(&self) -> bool {
        self.set
            .borrow()
            .with(self.index, |mut cx| {
                let due = cx.data_mut().guest_clock().wake_due();
                let turn = pin!(cx.run_concurrent(async |_| std::future::pending::<()>().await));
                assert!(poll_until_idle(turn).is_pending());
                due
            })
            .unwrap()
    }

    fn next_deadline(&self) -> Option<u64> {
        self.set
            .borrow()
            .with(self.index, |mut cx| {
                cx.data_mut().guest_clock().next_deadline()
            })
            .unwrap()
    }

    fn set_time(&self, time: u64) {
        self.set
            .borrow()
            .with(self.index, |cx| {
                cx.data().time.store(time, Ordering::Relaxed)
            })
            .unwrap();
    }

    /// The lines the guest printed, which outlive it.
    fn printed(&self) -> Arc<Mutex<Vec<(Output, String)>>> {
        self.set
            .borrow()
            .with(self.index, |cx| cx.data().printed.clone())
            .unwrap()
    }
}

/// A sleeping guest stays in its store between turns, with the deadline
/// the embedder is to run it at; it wakes in the turn after the time has
/// come, not before.
#[test]
fn a_wait_is_a_deadline_the_guest_wakes_after() {
    let dom = WasmDom::new();
    let set = set(&dom);
    let mut guest = Guest::new(&set);
    assert_eq!(guest.next_deadline(), None);
    assert!(!guest.start_sleep(5_000_000), "it waits");
    assert_eq!(guest.next_deadline(), Some(START + 5_000_000));
    // The guest takes calls while its task sleeps.
    guest.run("is-asleep");

    // A turn before the deadline wakes nothing.
    guest.set_time(START + 4_999_999);
    assert!(!guest.turn());
    guest.run("is-asleep");
    assert_eq!(guest.next_deadline(), Some(START + 5_000_000));

    // At the deadline the wait is over, but the task only moves in a turn.
    guest.set_time(START + 5_000_000);
    guest.run("is-asleep");
    assert!(guest.turn());
    guest.run("is-awake");
    assert_eq!(guest.next_deadline(), None);
    assert!(!guest.turn(), "nothing left to wake");
}

/// A wait for no time ends in the turn that starts it.
#[test]
fn a_wait_already_over_doesnt_wait() {
    let dom = WasmDom::new();
    let set = set(&dom);
    let mut guest = Guest::new(&set);
    assert!(guest.start_sleep(0));
    guest.run("is-awake");
    assert_eq!(guest.next_deadline(), None);
}

/// Two stores' waits are their own: each wakes at its deadline.
#[test]
fn each_store_has_its_waits() {
    let dom = WasmDom::new();
    let set = set(&dom);
    let mut first = Guest::new(&set);
    let mut second = Guest::new(&set);
    assert!(!first.start_sleep(1_000_000));
    assert!(!second.start_sleep(2_000_000));
    assert_eq!(first.next_deadline(), Some(START + 1_000_000));
    assert_eq!(second.next_deadline(), Some(START + 2_000_000));
    first.set_time(START + 1_000_000);
    assert!(first.turn());
    first.run("is-awake");
    assert!(!second.turn(), "its clock hasn't moved");
    second.run("is-asleep");
}

/// The clock the guest reads is down to its resolution.
#[test]
fn the_guests_clock_is_coarse() {
    assert_eq!(CLOCK_RESOLUTION, 100_000);
    let dom = WasmDom::new();
    let set = set(&dom);
    let mut guest = Guest::new(&set);
    guest.run("coarse-clock");
}

/// What the guest prints comes a line at a time, each from its output, as
/// it is written; a last line with no newline comes when the output closes
/// (the store goes).
#[test]
fn the_guests_output_comes_in_lines() {
    let dom = WasmDom::new();
    let set = set(&dom);
    let mut guest = Guest::new(&set);
    let printed = guest.printed();
    guest.run("print");
    let lines = [
        (Output::Stdout, "to stdout".to_owned()),
        (Output::Stderr, "to stderr".to_owned()),
        (Output::Stderr, "no newline".to_owned()),
    ];
    assert_eq!(*printed.lock().unwrap(), lines[..2]);
    drop(guest);
    assert_eq!(*printed.lock().unwrap(), lines);
}
