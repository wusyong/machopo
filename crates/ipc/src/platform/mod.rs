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
//! `terminate_current_process_immediately`, which machopo's processes
//! also end with once they have wound down.
//!
//! A process driven through a pipe, as Chrome's `--remote-debugging-pipe`
//! is, gets its two ends the same way, apart from the channel: whatever
//! starts it hands them over with [`spawn_with_pipe`], or as Chrome's
//! clients do, and it takes them with [`inherited_pipe`].
//!
//! Beside the channel, one thing of the system's that the rest asks for:
//! how much a disk has free ([`available_disk_space`], for the HTTP
//! cache's size), a system call like the others, in the one place that
//! makes them.

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
mod pipe;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
pub mod windows;

#[cfg(unix)]
pub use pipe::{INPUT_FD, OUTPUT_FD};
pub use pipe::{IO_PIPES_SWITCH, InheritError, PipeEnd, inherited_pipe, spawn_with_pipe};

#[cfg(target_os = "linux")]
pub use linux::channel::{Channel, child_gone, inherited, spawn};
#[cfg(target_os = "macos")]
pub use macos::channel::{Channel, child_gone, inherited, spawn};
#[cfg(unix)]
pub use unix::{available_disk_space, on_shutdown_signals, terminate_current_process_immediately};
#[cfg(windows)]
pub use windows::channel::{
    Channel, child_gone, inherited, inherited_file, spawn, spawn_handing, spawn_in,
    spawn_reading_errors,
};

/// A child [`spawn`] started: the standard library's, but on Windows,
/// where a child is started here with what the standard library can't
/// say (which handles it inherits, its sandbox), and can't be made one of
/// the standard library's.
#[cfg(not(windows))]
pub use std::process::Child;
#[cfg(windows)]
pub use sunaba::windows::Child;

#[cfg(target_os = "linux")]
pub use sunaba::linux::Namespaces as Sandbox;
/// What a child is started in ([`spawn_in`]), where part of a process's
/// sandbox is given by whoever starts it: on Windows all of it, on Linux
/// its own user and network namespaces (it enters the rest itself).
/// Elsewhere a process enters its own, and there is nothing to start it
/// with.
#[cfg(windows)]
pub use sunaba::windows::Policy as Sandbox;
#[cfg(not(any(windows, target_os = "linux")))]
#[derive(Clone, Debug)]
pub enum Sandbox {}

/// [`spawn_in`], with what the child writes to its standard error read
/// from the reader, not written where this process's goes.
#[cfg(not(windows))]
pub fn spawn_reading_errors(
    command: &mut std::process::Command,
    sandbox: Option<&Sandbox>,
) -> std::io::Result<(Child, Channel, std::io::PipeReader)> {
    let (reader, writer) = std::io::pipe()?;
    command.stderr(writer);
    let spawned = spawn_in(command, sandbox);
    // The command keeps no end of the pipe: the reader reads the end
    // when the child goes.
    command.stderr(std::process::Stdio::inherit());
    let (child, channel) = spawned?;
    Ok((child, channel, reader))
}

/// [`spawn`], where a sandbox isn't the starter's to give.
#[cfg(not(windows))]
pub fn spawn_in(
    command: &mut std::process::Command,
    sandbox: Option<&Sandbox>,
) -> std::io::Result<(Child, Channel)> {
    #[cfg(target_os = "linux")]
    if let Some(namespaces) = sandbox {
        let namespaces = namespaces.clone();
        // SAFETY: the closure runs in the child between fork and exec,
        // where only what is async-signal-safe may be done: `enter` makes
        // system calls on what it was made with, allocating nothing.
        unsafe {
            std::os::unix::process::CommandExt::pre_exec(command, move || namespaces.enter());
        }
    }
    #[cfg(not(target_os = "linux"))]
    if let Some(sandbox) = sandbox {
        match *sandbox {}
    }
    spawn(command)
}
#[cfg(windows)]
pub use windows::disk::available_disk_space;
#[cfg(windows)]
pub use windows::{on_shutdown_signals, terminate_current_process_immediately};
