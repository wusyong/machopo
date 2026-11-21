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

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "macos")]
pub use macos::channel::{Channel, child_gone, inherited, spawn};
