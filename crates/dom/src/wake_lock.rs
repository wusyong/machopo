//! A navigator's `wakeLock` (Blink's `modules/wake_lock/wake_lock.h`): the
//! same object each time, which the navigator keeps. Its active locks
//! (Blink's `WakeLockManager`) are `WakeLockSentinel`s, event targets of
//! their own (`EventTargetImpl`'s `wake_lock`), in the order granted: it
//! keeps them until they are released. When they are is `dom_api`'s.

use std::cell::Cell;

use elise::{Gc, GcObject, HeapVector, JsEdge, Member};

use crate::{Cx, EventTargetImpl, Navigator, Window, sites};

#[derive(GcObject)]
pub struct WakeLock {
    window: Member<Window>,
    /// Its active locks, in the order granted.
    sentinels: Member<HeapVector<EventTargetImpl>>,
    wrapper: JsEdge,
}

/// A `WakeLockSentinel`'s data: the wake lock it was granted by, and its
/// `[[Released]]`.
#[derive(GcObject)]
pub struct WakeLockSentinelData {
    wake_lock: Member<WakeLock>,
    released: Cell<bool>,
}

impl<'gc> Cx<'_, 'gc> {
    /// `navigator`'s `wakeLock`, the same each time.
    pub fn navigator_wake_lock(&self, navigator: Gc<'gc, Navigator>) -> Gc<'gc, WakeLock> {
        if let Some(wake_lock) = navigator.wake_lock.get(self.gc) {
            return wake_lock;
        }
        let wake_lock = self.gc.alloc_in(
            &sites::OTHER,
            WakeLockInit {
                window: Some(self.navigator_window(navigator)),
                sentinels: None,
                wrapper: (),
            },
        );
        navigator.wake_lock.set(self.gc, Some(wake_lock));
        wake_lock
    }

    /// `window`'s `navigator.wakeLock`, if script made it.
    pub fn window_wake_lock(&self, window: Gc<'gc, Window>) -> Option<Gc<'gc, WakeLock>> {
        window.navigator.get(self.gc)?.wake_lock.get(self.gc)
    }

    pub fn wake_lock_window(&self, wake_lock: Gc<'gc, WakeLock>) -> Gc<'gc, Window> {
        wake_lock.window.get(self.gc).expect("a wake lock's window")
    }

    /// A new lock of `wake_lock`'s, granted: a `WakeLockSentinel`, active
    /// until it is released.
    pub fn acquire_wake_lock(&self, wake_lock: Gc<'gc, WakeLock>) -> Gc<'gc, EventTargetImpl> {
        let target = self.create_event_target();
        let data = self.gc.alloc_in(
            &sites::OTHER,
            WakeLockSentinelDataInit {
                wake_lock: Some(wake_lock),
                released: Cell::new(false),
            },
        );
        target.wake_lock.set(self.gc, Some(data));
        let sentinels = match wake_lock.sentinels.get(self.gc) {
            Some(sentinels) => sentinels,
            None => {
                let sentinels = HeapVector::new(self.gc, 1);
                wake_lock.sentinels.set(self.gc, Some(sentinels));
                sentinels
            }
        };
        sentinels.push(self.gc, target);
        target
    }

    pub fn is_wake_lock_sentinel(&self, target: Gc<'gc, EventTargetImpl>) -> bool {
        target.wake_lock.get(self.gc).is_some()
    }

    /// The window of the wake lock that granted `target`; none for another
    /// target.
    pub fn wake_lock_sentinel_window(
        &self,
        target: Gc<'gc, EventTargetImpl>,
    ) -> Option<Gc<'gc, Window>> {
        let wake_lock = target.wake_lock.get(self.gc)?.wake_lock.get(self.gc)?;
        Some(self.wake_lock_window(wake_lock))
    }

    /// Whether `sentinel` was released.
    pub fn wake_lock_sentinel_released(&self, sentinel: Gc<'gc, EventTargetImpl>) -> bool {
        sentinel
            .wake_lock
            .get(self.gc)
            .is_some_and(|data| data.released.get())
    }

    /// The screen wake lock's "release a wake lock" of `sentinel`'s, its
    /// first steps: no longer active, released. Whether it was active.
    pub fn release_wake_lock(&self, sentinel: Gc<'gc, EventTargetImpl>) -> bool {
        let Some(data) = sentinel.wake_lock.get(self.gc) else {
            return false;
        };
        if data.released.replace(true) {
            return false;
        }
        if let Some(sentinels) = data
            .wake_lock
            .get(self.gc)
            .and_then(|lock| lock.sentinels.get(self.gc))
            && let Some(index) = (0..sentinels.len()).find(|&index| {
                sentinels
                    .get(self.gc, index)
                    .is_some_and(|s| Gc::ptr_eq(s, sentinel))
            })
        {
            sentinels.remove(self.gc, index);
        }
        true
    }

    /// The task of `wake_lock`'s permission for the request whose promise
    /// `promise` names, queued in its window: asked for, or `answered`.
    pub fn queue_wake_lock_task(&self, wake_lock: Gc<'gc, WakeLock>, promise: u64, answered: bool) {
        let window = self.wake_lock_window(wake_lock);
        let document = self.window_document(window);
        let data = promise.wrapping_shl(1) | u64::from(answered);
        self.push_element_task(
            window,
            document,
            crate::names::NONE,
            crate::window::WAKE_LOCK,
            data,
        );
    }

    /// `wake_lock`'s active locks, in the order granted.
    pub fn wake_lock_sentinels(
        &self,
        wake_lock: Gc<'gc, WakeLock>,
    ) -> Vec<Gc<'gc, EventTargetImpl>> {
        wake_lock
            .sentinels
            .get(self.gc)
            .map_or_else(Vec::new, |sentinels| {
                (0..sentinels.len())
                    .filter_map(|index| sentinels.get(self.gc, index))
                    .collect()
            })
    }

    pub(crate) fn wake_lock_wrapper_edge(&self, wake_lock: Gc<'gc, WakeLock>) -> &'gc JsEdge {
        &wake_lock.into_ref().wrapper
    }
}
