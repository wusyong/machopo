//! A policy's test: a process that enters the policy first thing, tries
//! one thing, and says what came of it. What a sandbox refuses can only be
//! seen from inside one, and a process can't leave to try the next thing,
//! so a test runs one of these for each.
//!
//! A program's `main` hands [`run`] its policy and its arguments:
//!
//! ```text
//! nothing
//! read PATH | list PATH | write PATH | remove PATH | stat PATH
//! chmod PATH | truncate PATH      (to rw-------; opened read-only with O_TRUNC)
//! field PATH NAME                 (the number after `NAME:` in the file)
//! syscall NUMBER [ARGUMENT...]
//! thread ATTEMPT...               (the attempt, on a new thread)
//! abort
//! ```
//!
//! and prints one line: `ok` and a number (bytes read, entries listed, the
//! system call's result), or `error` and the `errno`. A refused system
//! call ends the process by `SIGSYS`, with the handler's line on stderr.

use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;

use super::{Policy, enter};

/// The system calls this module's own code makes, for a policy that is
/// only being tested to start from: what a Rust program's start and end
/// need, the standard library's file calls (Landlock decides which paths)
/// and a thread's.
pub fn baseline() -> Policy {
    let mut policy = Policy::new();
    policy.allow(&[
        libc::SYS_read,
        libc::SYS_write,
        libc::SYS_close,
        libc::SYS_openat,
        libc::SYS_fstat,
        libc::SYS_newfstatat,
        libc::SYS_statx,
        libc::SYS_lseek,
        libc::SYS_getdents64,
        libc::SYS_unlinkat,
        libc::SYS_fcntl,
        libc::SYS_mmap,
        libc::SYS_munmap,
        libc::SYS_mprotect,
        libc::SYS_madvise,
        libc::SYS_brk,
        libc::SYS_futex,
        libc::SYS_rt_sigaction,
        libc::SYS_rt_sigprocmask,
        libc::SYS_rt_sigreturn,
        libc::SYS_sigaltstack,
        libc::SYS_set_robust_list,
        libc::SYS_rseq,
        libc::SYS_getrandom,
        libc::SYS_sched_getaffinity,
        libc::SYS_gettid,
        libc::SYS_exit,
        libc::SYS_exit_group,
    ]);
    policy.allow_threads();
    #[cfg(target_arch = "x86_64")]
    policy.allow(&[libc::SYS_unlink, libc::SYS_poll]);
    policy
}

/// Enters `policy`, tries what `arguments` say, prints the outcome and
/// ends the process: 0 when the outcome was printed, 2 when the sandbox
/// wasn't entered or the arguments make no sense.
pub fn run(policy: &Policy, arguments: impl IntoIterator<Item = String>) -> ! {
    let arguments: Vec<String> = arguments.into_iter().collect();
    no_core_dumps();
    if let Err(error) = enter(policy) {
        eprintln!("sunaba: {error}");
        std::process::exit(2);
    }
    attempt_and_exit(arguments)
}

/// [`run`] without the entering, for a program that enters the way it
/// does for real (what it does before, its own switches) and is then to
/// be tested: tries what `arguments` say, prints the outcome and ends the
/// process. Call [`no_core_dumps`] before entering.
pub fn attempt_and_exit(arguments: impl IntoIterator<Item = String>) -> ! {
    let arguments: Vec<String> = arguments.into_iter().collect();
    let line = match attempt(&arguments) {
        Ok(number) => format!("ok {number}"),
        Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => {
            eprintln!("sunaba: no such attempt: {arguments:?}");
            std::process::exit(2);
        }
        Err(error) => format!("error {}", error.raw_os_error().unwrap_or(0)),
    };
    let mut out = std::io::stdout();
    let printed = writeln!(out, "{line}").and_then(|()| out.flush());
    std::process::exit(if printed.is_ok() { 0 } else { 2 });
}

/// A refused system call ends the process the way a crash does, and a
/// test's every such process would leave a core dump behind.
pub fn no_core_dumps() {
    // SAFETY: `PR_SET_DUMPABLE` takes integers alone. That it may fail
    // only leaves the dumps.
    unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) };
}

fn invalid() -> std::io::Error {
    std::io::Error::from(std::io::ErrorKind::InvalidInput)
}

fn attempt(arguments: &[String]) -> std::io::Result<usize> {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["nothing"] => Ok(0),
        ["read", path] => std::fs::read(path).map(|bytes| bytes.len()),
        ["list", path] => std::fs::read_dir(path).map(Iterator::count),
        ["write", path] => std::fs::write(path, b"written").map(|()| 0),
        ["remove", path] => std::fs::remove_file(path).map(|()| 0),
        ["stat", path] => std::fs::metadata(path).map(|_| 0),
        // The file's mode to `rw-------`.
        ["chmod", path] => {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map(|()| 0)
        }
        // Opened for reading with `O_TRUNC`: what a kernel before
        // Landlock's third ABI doesn't count as writing.
        ["truncate", path] => std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_TRUNC)
            .open(path)
            .map(|_| 0),
        // The number after `name:` on a line of `path` (`/proc`'s status
        // files).
        ["field", path, name] => std::fs::read_to_string(path)?
            .lines()
            .find_map(|line| {
                line.strip_prefix(name)?
                    .strip_prefix(':')?
                    .trim()
                    .parse()
                    .ok()
            })
            .ok_or_else(invalid),
        // The rest, on a thread made after entering.
        ["thread", rest @ ..] => {
            let rest: Vec<String> = rest.iter().map(|word| (*word).to_owned()).collect();
            std::thread::Builder::new()
                .spawn(move || attempt(&rest))?
                .join()
                .unwrap_or_else(|_| Err(invalid()))
        }
        ["syscall", number, rest @ ..] => system_call(number, rest),
        // Ends the process as a program that gives up does.
        ["abort"] => std::process::abort(),
        _ => Err(invalid()),
    }
}

/// System call `number` with up to six arguments, the rest 0: its result,
/// or its `errno`.
fn system_call(number: &str, arguments: &[&str]) -> std::io::Result<usize> {
    let number: libc::c_long = number.parse().map_err(|_| invalid())?;
    let mut six = [0 as libc::c_long; 6];
    if arguments.len() > six.len() {
        return Err(invalid());
    }
    for (slot, argument) in six.iter_mut().zip(arguments) {
        *slot = argument.parse().map_err(|_| invalid())?;
    }
    let [a, b, c, d, e, f] = six;
    // SAFETY: this is the test's whole point, and it is only as sound as
    // its caller's arguments: the kernel is asked for whatever they say,
    // in a process that exists to make this one call and end. The tests
    // pass numbers and flags, no pointers but null.
    let result = unsafe { libc::syscall(number, a, b, c, d, e, f) };
    usize::try_from(result).map_err(|_| std::io::Error::last_os_error())
}
