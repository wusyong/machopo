//! A profile's test: a process that enters the profile first thing, tries
//! one thing, and says what came of it. What a sandbox refuses can only be
//! seen from inside one, and a process can't leave to try the next thing,
//! so a test runs one of these for each.
//!
//! A program's `main` hands [`run`] its profile and its arguments:
//!
//! ```text
//! nothing
//! entered                         (1 in a sandbox, 0 outside)
//! read PATH | list PATH | write PATH | remove PATH | stat PATH
//! chmod PATH                      (to rw-------)
//! connect ADDRESS:PORT            (TCP)
//! listen                          (TCP, any port of the loopback)
//! unix PATH                       (a Unix socket, connected)
//! run PATH                        (a program, started and waited for)
//! fork                            (a child that ends at once)
//! signal PID                      (signal 0: whether it could be sent)
//! process PID                     (what the kernel says of the process)
//! lookup NAME                     (a Mach service, asked of launchd)
//! sysctl NAME                     (read)
//! shm NAME                        (a POSIX shared memory object, made)
//! wait                            (until standard input ends)
//! thread ATTEMPT...               (the attempt, on a new thread)
//! abort
//! ```
//!
//! and prints one line: `ok` and a number (bytes read, entries listed), or
//! `error` and the `errno` (for `lookup`, launchd's return code). The sandbox refuses by failing the call, most
//! often with `EPERM`: nothing ends the process.

use std::ffi::{CString, c_char, c_int};
use std::io::Write;

use super::{Profile, enter, entered};

/// Enters `profile`, tries what `arguments` say, prints the outcome and
/// ends the process: 0 when the outcome was printed, 2 when the sandbox
/// wasn't entered or the arguments make no sense.
pub fn run(profile: &Profile, arguments: impl IntoIterator<Item = String>) -> ! {
    let arguments: Vec<String> = arguments.into_iter().collect();
    if let Err(error) = enter(profile) {
        eprintln!("sunaba: {error}");
        std::process::exit(2);
    }
    attempt_and_exit(arguments)
}

/// [`run`] without the entering, for a program that enters the way it
/// does for real (what it does before, its own switches) and is then to
/// be tested: tries what `arguments` say, prints the outcome and ends the
/// process.
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

fn invalid() -> std::io::Error {
    std::io::Error::from(std::io::ErrorKind::InvalidInput)
}

fn c_string(word: &str) -> std::io::Result<CString> {
    CString::new(word).map_err(|_| invalid())
}

/// launchd's return code as the error (`error N`).
fn mach(code: c_int) -> std::io::Result<usize> {
    if code == 0 {
        Ok(0)
    } else {
        Err(std::io::Error::from_raw_os_error(code))
    }
}

unsafe extern "C" {
    static bootstrap_port: libc::mach_port_t;
    fn bootstrap_look_up(
        bootstrap: libc::mach_port_t,
        name: *const c_char,
        port: *mut libc::mach_port_t,
    ) -> c_int;
}

fn attempt(arguments: &[String]) -> std::io::Result<usize> {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["nothing"] => Ok(0),
        ["entered"] => Ok(usize::from(entered())),
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
        ["connect", address] => std::net::TcpStream::connect(address).map(|_| 0),
        ["listen"] => std::net::TcpListener::bind("127.0.0.1:0").map(|_| 0),
        ["unix", path] => std::os::unix::net::UnixStream::connect(path).map(|_| 0),
        ["run", path] => std::process::Command::new(path)
            .stdout(std::process::Stdio::null())
            .status()
            .map(|_| 0),
        ["fork"] => {
            // SAFETY: the child does nothing but end, with the one call
            // that is safe in a forked child of a threaded process; the
            // parent waits for that child alone.
            unsafe {
                match libc::fork() {
                    -1 => Err(std::io::Error::last_os_error()),
                    0 => libc::_exit(0),
                    child => {
                        libc::waitpid(child, std::ptr::null_mut(), 0);
                        Ok(0)
                    }
                }
            }
        }
        ["signal", pid] => {
            let pid: libc::pid_t = pid.parse().map_err(|_| invalid())?;
            // SAFETY: integers alone; signal 0 is sent to nothing, and
            // only says whether one could be.
            if unsafe { libc::kill(pid, 0) } == 0 {
                Ok(0)
            } else {
                Err(std::io::Error::last_os_error())
            }
        }
        ["process", pid] => {
            let pid: libc::pid_t = pid.parse().map_err(|_| invalid())?;
            // SAFETY: all-zero is a valid `proc_bsdinfo` (integers and
            // byte arrays); the kernel writes at most the size it is told,
            // which is the structure's.
            let filled = unsafe {
                let mut info: libc::proc_bsdinfo = std::mem::zeroed();
                libc::proc_pidinfo(
                    pid,
                    libc::PROC_PIDTBSDINFO,
                    0,
                    (&raw mut info).cast(),
                    c_int::try_from(size_of::<libc::proc_bsdinfo>()).unwrap_or(0),
                )
            };
            match usize::try_from(filled) {
                Ok(filled) if filled > 0 => Ok(filled),
                _ => Err(std::io::Error::last_os_error()),
            }
        }
        ["lookup", name] => {
            let name = c_string(name)?;
            let mut port: libc::mach_port_t = 0;
            // SAFETY: the process's bootstrap port as it started with, a C
            // string, and a place for one port name; a port that comes
            // back is the process's until it ends, which is next.
            mach(unsafe { bootstrap_look_up(bootstrap_port, name.as_ptr(), &raw mut port) })
        }
        ["sysctl", name] => {
            let name = c_string(name)?;
            let mut length: libc::size_t = 0;
            // SAFETY: a C string, no place for the value (its length is
            // all that is asked), a place for the length, nothing to set.
            let result = unsafe {
                libc::sysctlbyname(
                    name.as_ptr(),
                    std::ptr::null_mut(),
                    &raw mut length,
                    std::ptr::null_mut(),
                    0,
                )
            };
            if result == 0 {
                Ok(length)
            } else {
                Err(std::io::Error::last_os_error())
            }
        }
        ["shm", name] => {
            let name = c_string(name)?;
            // SAFETY: a C string and integers; what is made is unlinked
            // and closed at once.
            unsafe {
                let fd = libc::shm_open(name.as_ptr(), libc::O_RDWR | libc::O_CREAT, 0o600);
                if fd < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                libc::shm_unlink(name.as_ptr());
                libc::close(fd);
            }
            Ok(0)
        }
        // For whoever started the process to look at it meanwhile.
        ["wait"] => std::io::copy(&mut std::io::stdin(), &mut std::io::sink()).map(|_| 0),
        // The rest, on a thread made after entering.
        ["thread", rest @ ..] => {
            let rest: Vec<String> = rest.iter().map(|word| (*word).to_owned()).collect();
            std::thread::Builder::new()
                .spawn(move || attempt(&rest))?
                .join()
                .unwrap_or_else(|_| Err(invalid()))
        }
        // Ends the process as a program that gives up does.
        ["abort"] => std::process::abort(),
        _ => Err(invalid()),
    }
}
