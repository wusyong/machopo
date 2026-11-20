//! The locks and counts a channel's threads share: std's, or, built with
//! `--cfg ipc_loom` or `--cfg ipc_shuttle`, loom's or shuttle's, so that those
//! check the code that uses them (loom every interleaving of a small
//! model; shuttle random schedules of a larger one). Everything a
//! channel's threads share goes through here.

#[cfg(ipc_loom)]
pub(crate) use loom::sync::{Arc, Mutex, MutexGuard};
#[cfg(ipc_shuttle)]
pub(crate) use shuttle::sync::{Arc, Mutex, MutexGuard};
#[cfg(not(any(ipc_loom, ipc_shuttle)))]
pub(crate) use std::sync::{Arc, Mutex, MutexGuard};

/// Locks `mutex`, through a poisoning: what a channel's lock guards is
/// whole at every unlock.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
