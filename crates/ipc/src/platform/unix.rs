//! What macOS and Linux have in common that a process at an end of a
//! channel needs and neither's layer is the place for.

/// Ends this process now, with `status` (Chromium's
/// `base::Process::TerminateCurrentProcessImmediately`): nothing is
/// unwound, no handler registered for the exit runs, and nothing buffered
/// is flushed. For a child whose host is gone, from its IO thread: its
/// other threads are wherever they are, one of them perhaps in a loop
/// that never ends, and whatever runs at an ordinary exit would run
/// beside them, or wait for them.
pub fn terminate_current_process_immediately(status: i32) -> ! {
    // SAFETY: `_exit` takes any status, doesn't return, and reads and
    // writes none of this process's memory: it is the system call that
    // ends the process, which is the kernel taking every thread off its
    // processor and closing what the process had open. No code of the
    // process's runs after the call, so nothing is left to see a value
    // that was never dropped.
    unsafe { libc::_exit(status) }
}

/// The bytes free on the file system `path` is on, for this user (Chromium's
/// `base::SysInfo::AmountOfFreeDiskSpace`): the blocks `statvfs` says are
/// available to one who isn't the superuser, times their size.
pub fn available_disk_space(path: &std::path::Path) -> std::io::Result<u64> {
    let stat = rustix::fs::statvfs(path)?;
    Ok(stat.f_bavail.saturating_mul(stat.f_frsize))
}
