//! What Windows has for the process itself.

use windows_sys::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};

/// Ends this process now, with `status` (Chromium's
/// `base::Process::TerminateCurrentProcessImmediately`, which calls
/// `TerminateProcess` on itself): nothing is unwound, no DLL is told the
/// process is detaching, and nothing buffered is flushed, as `_exit` on
/// macOS and Linux.
pub fn terminate_current_process_immediately(status: i32) -> ! {
    // SAFETY: `GetCurrentProcess` returns a pseudo-handle, a constant that
    // needs no closing and is always open. `TerminateProcess` on it reads
    // and writes none of this process's memory: the kernel ends every
    // thread and closes what the process had open, and no code of the
    // process's runs after it, so nothing is left to see a value that was
    // never dropped.
    unsafe {
        TerminateProcess(GetCurrentProcess(), status.cast_unsigned());
    }
    // It doesn't return when it ends this process; if it ever fails, the
    // process ends all the same, as Chromium's does.
    std::process::abort()
}
