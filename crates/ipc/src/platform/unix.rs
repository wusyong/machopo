//! What macOS and Linux have in common that a process at an end of a
//! channel needs and neither's layer is the place for.

use std::io::Read as _;
use std::os::fd::{AsRawFd as _, IntoRawFd as _};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicI32, Ordering};

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

/// The signals that end machopo as closing it does (Chromium's
/// `InstallShutdownSignalHandlers`: SIGTERM, SIGINT, SIGHUP).
const SHUTDOWN_SIGNALS: [libc::c_int; 3] = [libc::SIGTERM, libc::SIGINT, libc::SIGHUP];

/// The end the handler writes each signal's number to: none (-1) until
/// [`on_shutdown_signals`].
static SIGNAL_PIPE: AtomicI32 = AtomicI32::new(-1);

/// Calls `on_signal` with each SIGTERM, SIGINT and SIGHUP this process gets
/// from now on, on a thread of its own (Chromium's `ShutdownDetector`):
/// the handler only writes the signal's number to a socket the thread
/// reads, which is about all a handler may do (it runs on whatever thread
/// the signal lands on, between any two instructions). A signal the process
/// was started ignoring (`nohup`, a shell's background job) stays ignored,
/// as Chromium's does. Once in a process: an error after.
pub fn on_shutdown_signals(on_signal: impl Fn(i32) + Send + 'static) -> std::io::Result<()> {
    let (mut read, write) = UnixStream::pair()?;
    // A handler never waits: were the socket full (signals faster than
    // the thread reads them), one is dropped.
    write.set_nonblocking(true)?;
    if SIGNAL_PIPE
        .compare_exchange(-1, write.as_raw_fd(), Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err(std::io::Error::other(
            "the shutdown signals are handled already",
        ));
    }
    // Open for as long as the process: a handler may write to it any time.
    let _ = write.into_raw_fd();
    std::thread::Builder::new()
        .name("ShutdownDetector".into())
        .spawn(move || {
            let mut signal = [0u8];
            loop {
                match read.read(&mut signal) {
                    Ok(1) => on_signal(i32::from(signal[0])),
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    // The write end is never closed.
                    _ => break,
                }
            }
        })?;
    for signal in SHUTDOWN_SIGNALS {
        install(signal)?;
    }
    Ok(())
}

/// `handle` for `signal`, unless the process was started ignoring it.
fn install(signal: libc::c_int) -> std::io::Result<()> {
    // SAFETY: both structures are zeroed locals (all zeroes is a valid
    // `sigaction`: no flags, an empty mask, the default action) that live
    // until the calls return; `sigaction` only reads the one and writes
    // the other. `handle` is an `extern "C" fn(c_int)`, the handler
    // `sa_sigaction` takes without `SA_SIGINFO`, and does only what a
    // handler may (`handle`'s SAFETY).
    unsafe {
        let mut old: libc::sigaction = std::mem::zeroed();
        if libc::sigaction(signal, std::ptr::null(), &raw mut old) != 0 {
            return Err(std::io::Error::last_os_error());
        }
        if old.sa_sigaction == libc::SIG_IGN {
            return Ok(());
        }
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = handle as extern "C" fn(libc::c_int) as libc::sighandler_t;
        // A system call it interrupts goes on, as without it.
        action.sa_flags = libc::SA_RESTART;
        libc::sigemptyset(&raw mut action.sa_mask);
        if libc::sigaction(signal, &raw const action, std::ptr::null_mut()) != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

/// The handler: the signal's number, to the socket.
extern "C" fn handle(signal: libc::c_int) {
    // The three signals' numbers are small.
    let byte = u8::try_from(signal).unwrap_or(u8::MAX);
    let errno = errno_location();
    // SAFETY: `write` and reading and writing `errno` are what a signal
    // handler may do (POSIX's async-signal-safe functions); the socket is
    // open for as long as the process (`on_shutdown_signals` installs this
    // after it is set, and never closes it), and the byte is a local that
    // lives until `write` returns. `errno` is this thread's: it is put back
    // as it was, for the code the signal interrupted, which may read it.
    unsafe {
        let saved = *errno;
        libc::write(
            SIGNAL_PIPE.load(Ordering::Acquire),
            (&raw const byte).cast(),
            1,
        );
        *errno = saved;
    }
}

/// This thread's `errno`.
fn errno_location() -> *mut libc::c_int {
    // SAFETY: the C library's own accessor for the calling thread's
    // `errno`, which takes nothing and is always valid to call; the
    // address it gives is the thread's for as long as the thread.
    #[cfg(target_os = "linux")]
    unsafe {
        libc::__errno_location()
    }
    // SAFETY: as Linux's, macOS's name for it.
    #[cfg(target_os = "macos")]
    unsafe {
        libc::__error()
    }
}
