//! What each operating system does for a channel (Chromium's
//! `mojo/core/platform` and `base/memory/platform_shared_memory_region_*`):
//! it moves bytes and handles, waits, and makes and maps shared memory.
//! There is no judgement here: what to make of anything a peer sent is the
//! core's. Each is thin enough to read line by line, which is how it is
//! checked: no tool sees through a system call.

#[cfg(target_os = "macos")]
pub mod macos;
