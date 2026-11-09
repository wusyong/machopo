//! A window's idle callbacks (https://w3c.github.io/requestidlecallback/,
//! Blink's `ScriptedIdleTaskController`): `requestIdleCallback`'s, in the
//! order requested, each with its handle, whether an idle period started
//! since it was requested (HTML's list of runnable idle callbacks), when
//! its timeout runs it, and its callback, the JS engine's. When an idle
//! period starts and ends is the host's (Blink's scheduler's
//! `IdleHelper`); an `IdleDeadline` is what a callback gets, its period's
//! end on its window's clock.

use std::cell::Cell;

use elise::{Gc, GcObject, HeapVector, JsEdge, Member};

use crate::{Cx, Window, sites};

/// A callback `requestIdleCallback` requested.
#[derive(GcObject)]
pub struct IdleCallback {
    /// Its handle (from 1, a WebIDL `unsigned long`).
    handle: Cell<u32>,
    /// Whether an idle period started since it was requested: it may run
    /// in the one running; those requested meanwhile wait for the next.
    runnable: Cell<bool>,
    /// When its `timeout` runs it if no idle period has (milliseconds
    /// since its window's time origin); infinite without one.
    due: Cell<f64>,
    /// Its timeout's task's number (`tasks`).
    number: Cell<u64>,
    callback: JsEdge,
}

/// What an idle callback gets (`IdleDeadline`): its window, on whose clock
/// `deadline` is, and whether its timeout ran it.
#[derive(GcObject)]
pub struct IdleDeadline {
    window: Member<Window>,
    deadline: Cell<f64>,
    did_timeout: Cell<bool>,
    wrapper: JsEdge,
}

impl<'gc> Cx<'_, 'gc> {
    /// A new idle callback of `window`, its handle one more than the
    /// last, its timeout due `timeout` milliseconds from now if that is
    /// more than 0; its callback is the engine's to set.
    pub fn add_idle_callback(
        &self,
        window: Gc<'gc, Window>,
        timeout: u32,
    ) -> Gc<'gc, IdleCallback> {
        let handle = window
            .last_idle_callback
            .get()
            .checked_add(1)
            .expect("fewer than 2^32 idle callbacks a window");
        window.last_idle_callback.set(handle);
        let due = if timeout > 0 {
            self.now_since(Some(window)) + f64::from(timeout)
        } else {
            f64::INFINITY
        };
        let callback = self.gc.alloc_in(
            &sites::OTHER,
            IdleCallbackInit {
                handle: Cell::new(handle),
                runnable: Cell::new(false),
                due: Cell::new(due),
                number: Cell::new(self.next_task_number()),
                callback: (),
            },
        );
        let callbacks = match window.idle_callbacks.get(self.gc) {
            Some(callbacks) => callbacks,
            None => {
                let callbacks = HeapVector::new(self.gc, 1);
                window.idle_callbacks.set(self.gc, Some(callbacks));
                callbacks
            }
        };
        callbacks.push(self.gc, callback);
        callback
    }

    pub fn idle_callback_handle(&self, callback: Gc<'gc, IdleCallback>) -> u32 {
        callback.handle.get()
    }

    /// `callback`'s reference to its function, which only the JS engine
    /// reads and writes.
    pub fn idle_callback_edge(&self, callback: Gc<'gc, IdleCallback>) -> &'gc JsEdge {
        &callback.into_ref().callback
    }

    /// `cancelIdleCallback(handle)`: `window`'s callback of `handle`
    /// taken out, if it has it.
    pub fn cancel_idle_callback(&self, window: Gc<'gc, Window>, handle: u32) {
        if let Some(index) =
            self.idle_callback_index(window, |callback| callback.handle.get() == handle)
        {
            self.remove_idle_callback(window, index);
        }
    }

    /// Whether `window` has an idle callback waiting.
    pub fn has_idle_callbacks(&self, window: Gc<'gc, Window>) -> bool {
        window
            .idle_callbacks
            .get(self.gc)
            .is_some_and(|callbacks| !callbacks.is_empty())
    }

    /// HTML's "start an idle period" for `window`: every callback
    /// requested so far may run in it; those its callbacks request wait
    /// for the next.
    pub fn start_idle_period(&self, window: Gc<'gc, Window>) {
        let Some(callbacks) = window.idle_callbacks.get(self.gc) else {
            return;
        };
        for callback in (0..callbacks.len()).filter_map(|index| callbacks.get(self.gc, index)) {
            callback.runnable.set(true);
        }
    }

    /// Whether one of `window`'s callbacks may run in the idle period
    /// that started.
    pub fn has_runnable_idle_callback(&self, window: Gc<'gc, Window>) -> bool {
        self.idle_callback_index(window, |callback| callback.runnable.get())
            .is_some()
    }

    /// The first of `window`'s callbacks that may run in the idle period
    /// running, taken out (HTML's "invoke idle callbacks").
    pub fn take_runnable_idle_callback(
        &self,
        window: Gc<'gc, Window>,
    ) -> Option<Gc<'gc, IdleCallback>> {
        let index = self.idle_callback_index(window, |callback| callback.runnable.get())?;
        Some(self.remove_idle_callback(window, index))
    }

    /// When `window`'s next idle callback's timeout is due (milliseconds
    /// since its time origin), if one has one.
    pub fn next_idle_timeout(&self, window: Gc<'gc, Window>) -> Option<f64> {
        let callbacks = window.idle_callbacks.get(self.gc)?;
        (0..callbacks.len())
            .filter_map(|index| callbacks.get(self.gc, index))
            .map(|callback| callback.due.get())
            .filter(|due| due.is_finite())
            .reduce(f64::min)
    }

    /// `window`'s callback whose timeout is due by `now`, taken out: the
    /// earliest, of those the first requested (HTML's "invoke idle
    /// callback timeout algorithm").
    pub fn take_due_idle_timeout(
        &self,
        window: Gc<'gc, Window>,
        now: f64,
    ) -> Option<Gc<'gc, IdleCallback>> {
        let (index, due, _) = self.first_idle_timeout(window)?;
        (due <= now).then(|| self.remove_idle_callback(window, index))
    }

    /// When `window`'s idle callback timeout to run first is due (since its
    /// time origin), and its number (`tasks`).
    pub(crate) fn next_due_idle_timeout(&self, window: Gc<'gc, Window>) -> Option<(f64, u64)> {
        self.first_idle_timeout(window)
            .map(|(_, due, number)| (due, number))
    }

    /// `window`'s idle callback whose timeout runs first, of those with one
    /// the host's pass lets run: the earliest due, ties by number.
    fn first_idle_timeout(&self, window: Gc<'gc, Window>) -> Option<(usize, f64, u64)> {
        let callbacks = window.idle_callbacks.get(self.gc)?;
        let mut first: Option<(usize, f64, u64)> = None;
        for index in 0..callbacks.len() {
            let callback = callbacks.get(self.gc, index).expect("below the length");
            let (due, number) = (callback.due.get(), callback.number.get());
            if !due.is_finite() || !self.task_may_run(number) {
                continue;
            }
            if first.is_none_or(|(_, best, best_number)| {
                due.total_cmp(&best).then(number.cmp(&best_number)).is_lt()
            }) {
                first = Some((index, due, number));
            }
        }
        first
    }

    fn idle_callback_index(
        &self,
        window: Gc<'gc, Window>,
        found: impl Fn(Gc<'gc, IdleCallback>) -> bool,
    ) -> Option<usize> {
        let callbacks = window.idle_callbacks.get(self.gc)?;
        (0..callbacks.len()).find(|&index| callbacks.get(self.gc, index).is_some_and(&found))
    }

    fn remove_idle_callback(&self, window: Gc<'gc, Window>, index: usize) -> Gc<'gc, IdleCallback> {
        window
            .idle_callbacks
            .get(self.gc)
            .expect("a window's idle callbacks")
            .remove(self.gc, index)
    }

    /// A new `IdleDeadline` of `window`'s, its idle period ending at
    /// `deadline` (milliseconds since the window's time origin), its
    /// callback run by its timeout if `did_timeout`.
    pub fn create_idle_deadline(
        &self,
        window: Gc<'gc, Window>,
        deadline: f64,
        did_timeout: bool,
    ) -> Gc<'gc, IdleDeadline> {
        self.gc.alloc_in(
            &sites::OTHER,
            IdleDeadlineInit {
                window: Some(window),
                deadline: Cell::new(deadline),
                did_timeout: Cell::new(did_timeout),
                wrapper: (),
            },
        )
    }

    /// `deadline`'s window, the end of its idle period, and whether its
    /// callback's timeout ran it.
    pub fn idle_deadline(&self, deadline: Gc<'gc, IdleDeadline>) -> (Gc<'gc, Window>, f64, bool) {
        (
            deadline
                .window
                .get(self.gc)
                .expect("an idle deadline's window"),
            deadline.deadline.get(),
            deadline.did_timeout.get(),
        )
    }

    pub(crate) fn idle_deadline_wrapper_edge(
        &self,
        deadline: Gc<'gc, IdleDeadline>,
    ) -> &'gc JsEdge {
        &deadline.into_ref().wrapper
    }
}
