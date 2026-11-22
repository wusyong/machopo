//! What each operating system does for a channel (Chromium's
//! `mojo/core/platform` and `base/memory/platform_shared_memory_region_*`):
//! it moves bytes and handles, waits, and makes and maps shared memory.
//! There is no judgement here: what to make of anything a peer sent is the
//! core's. Each is thin enough to read line by line, which is how it is
//! checked: no tool sees through a system call.

//!
//! Whatever the system, a host and its child get their ends the same
//! way: the host starts the child with [`spawn`] and has its own end; the
//! child takes its with [`inherited`]; and when the child exits, the host
//! says so with [`child_gone`], for the systems where the kernel doesn't.
//! [`Channel`] is the end: an [`crate::endpoint::Platform`].
//!
//! A child doesn't outlive its host. On Windows the kernel sees to it
//! (`spawn` puts the child in a job); on macOS and Linux the child does,
//! once its channel says the host is gone, with
//! `terminate_current_process_immediately`.

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
pub mod windows;

#[cfg(target_os = "linux")]
pub use linux::channel::{Channel, child_gone, inherited, spawn};
#[cfg(target_os = "macos")]
pub use macos::channel::{Channel, child_gone, inherited, spawn};
#[cfg(unix)]
pub use unix::terminate_current_process_immediately;
#[cfg(windows)]
pub use windows::channel::{Channel, child_gone, inherited, spawn};
