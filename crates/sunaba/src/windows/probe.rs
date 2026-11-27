//! A policy's test: a process started under the policy that tries one
//! thing and says what came of it. What a sandbox refuses can only be
//! seen from inside one, so a test starts one of these for each thing.
//!
//! The started process's `main` hands [`attempt_and_exit`] its arguments,
//! once it has lowered itself as it would for real:
//!
//! ```text
//! nothing
//! read PATH | list PATH | write PATH | remove PATH | stat PATH
//! connect ADDRESS:PORT            (TCP)
//! listen                          (TCP, any port of the loopback)
//! run PATH ARGUMENT...            (a program, started and waited for)
//! process PID                     (opened to read its memory)
//! handle VALUE                    (whether it is a handle to a device here)
//! integrity                       (this process's integrity level)
//! container                       (1 in an app container, 0 outside)
//! mitigation NUMBER               (the flags of that mitigation policy)
//! thread ATTEMPT...               (the attempt, on a new thread)
//! words WORD...                   (how many arguments it was given)
//! length WORD                     (how many bytes the argument is)
//! env NAME                        (how many bytes the variable is)
//! sleep SECONDS
//! abort
//! ```
//!
//! and prints one line: `ok` and a number (bytes read, entries listed, a
//! level, flags), or `error` and the system's error code (5 is access
//! denied; a socket's are Winsock's, and 10107 is Winsock not starting).
//! The sandbox refuses by failing the call: nothing ends the process, but
//! a handle that isn't one once the process has lowered itself.

use std::io::{Read as _, Write as _};
use std::os::windows::io::{BorrowedHandle, RawHandle};

use windows_sys::Win32::Foundation::{GetHandleInformation, SetHandleInformation};
use windows_sys::Win32::Networking::WinSock::{WSADATA, WSAStartup};
use windows_sys::Win32::Storage::FileSystem::{FILE_TYPE_CHAR, GetFileType};
use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_VM_READ};

use super::{Mitigation, in_app_container, integrity, mitigation, owned, this_process};

/// `HANDLE_FLAG_INHERIT`.
const INHERIT: u32 = 1;

/// Tries what `arguments` say, prints the outcome and ends the process: 0
/// when the outcome was printed, 2 when the arguments make no sense.
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

fn attempt(arguments: &[String]) -> std::io::Result<usize> {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["nothing"] => Ok(0),
        ["read", path] => std::fs::read(path).map(|bytes| bytes.len()),
        ["list", path] => std::fs::read_dir(path).map(Iterator::count),
        ["write", path] => std::fs::write(path, b"written").map(|()| 0),
        ["remove", path] => std::fs::remove_file(path).map(|()| 0),
        ["stat", path] => std::fs::metadata(path).map(|_| 0),
        // Connected, and a byte each way: one the other end sends back.
        ["connect", address] => {
            winsock()?;
            let mut stream = std::net::TcpStream::connect(address)?;
            stream.write_all(b"x")?;
            let mut byte = [0u8; 1];
            stream.read(&mut byte)
        }
        ["listen"] => {
            winsock()?;
            std::net::TcpListener::bind("127.0.0.1:0").map(|_| 0)
        }
        ["run", path, rest @ ..] => std::process::Command::new(path)
            .args(rest)
            .status()
            .map(|status| usize::from(!status.success())),
        ["process", id] => {
            let id: u32 = id.parse().map_err(|_| invalid())?;
            // SAFETY: opens a handle or fails; nothing is read through it.
            let process = unsafe { OpenProcess(PROCESS_VM_READ, 0, id) };
            if process.is_null() {
                return Err(std::io::Error::last_os_error());
            }
            drop(owned(process));
            Ok(0)
        }
        ["handle", value] => {
            let value: usize = value.parse().map_err(|_| invalid())?;
            let handle = std::ptr::with_exposed_provenance_mut(value);
            let mut flags = 0u32;
            // SAFETY: this is the test's point: whether a number is a
            // handle in this process, and to what. The calls read nothing
            // through it and write `flags`; a number that is no handle is
            // an error, or ends a process that asked for that.
            let device = unsafe {
                GetHandleInformation(handle, &raw mut flags) != 0
                    && GetFileType(handle) == FILE_TYPE_CHAR
            };
            // Another handle of this process's that happens to have the
            // number isn't the one asked about.
            if device {
                Ok(1)
            } else {
                // `ERROR_INVALID_HANDLE`.
                Err(std::io::Error::from_raw_os_error(6))
            }
        }
        ["integrity"] => integrity(this_process()).and_then(number),
        ["container"] => in_app_container(this_process()).map(usize::from),
        ["mitigation", which] => {
            let which = match which.parse::<i32>().map_err(|_| invalid())? {
                3 => Mitigation::StrictHandleCheck,
                4 => Mitigation::SystemCallDisable,
                6 => Mitigation::ExtensionPointDisable,
                8 => Mitigation::Signature,
                9 => Mitigation::FontDisable,
                10 => Mitigation::ImageLoad,
                13 => Mitigation::ChildProcess,
                14 => Mitigation::SideChannelIsolation,
                _ => return Err(invalid()),
            };
            mitigation(this_process(), which).and_then(number)
        }
        // The rest, on a thread made after the process lowered itself.
        ["thread", rest @ ..] => {
            let rest: Vec<String> = rest.iter().map(|word| (*word).to_owned()).collect();
            std::thread::Builder::new()
                .spawn(move || attempt(&rest))?
                .join()
                .unwrap_or_else(|_| Err(invalid()))
        }
        ["words", rest @ ..] => Ok(rest.len()),
        ["length", word] => Ok(word.len()),
        ["env", name] => std::env::var_os(name)
            .map(|value| value.len())
            // `ERROR_ENVVAR_NOT_FOUND`.
            .ok_or_else(|| std::io::Error::from_raw_os_error(203)),
        ["sleep", seconds] => {
            let seconds = seconds.parse().map_err(|_| invalid())?;
            std::thread::sleep(std::time::Duration::from_secs(seconds));
            Ok(0)
        }
        // Ends the process as a program that gives up does.
        ["abort"] => std::process::abort(),
        // Commits that many MiB, and gives them back: a refusal is
        // `ERROR_NOT_ENOUGH_MEMORY`, and the process goes on.
        ["allocate", megabytes] => {
            let megabytes: usize = megabytes.parse().map_err(|_| invalid())?;
            let bytes = megabytes.checked_mul(1 << 20).ok_or_else(invalid)?;
            let mut memory: Vec<u8> = Vec::new();
            memory
                .try_reserve_exact(bytes)
                .map_err(|_| std::io::Error::from_raw_os_error(8))?;
            memory.resize(bytes, 1);
            Ok(megabytes)
        }
        _ => Err(invalid()),
    }
}

/// Winsock started, or why it wasn't: the standard library starts it too
/// as a socket is first made, and ends the process when it can't.
fn winsock() -> std::io::Result<()> {
    // SAFETY: all-zero is a valid `WSADATA` (numbers, and pointers that
    // aren't read), which the call writes.
    let mut data: WSADATA = unsafe { std::mem::zeroed() };
    // SAFETY: as above; version 2.2.
    match unsafe { WSAStartup(0x0202, &raw mut data) } {
        0 => Ok(()),
        error => Err(std::io::Error::from_raw_os_error(error)),
    }
}

fn number(value: u32) -> std::io::Result<usize> {
    usize::try_from(value).map_err(|_| invalid())
}

/// Whether processes this one starts may inherit `handle`: what
/// [`super::Command::inherit`] needs of a handle it is given.
pub fn set_inheritable(handle: BorrowedHandle<'_>, inheritable: bool) -> std::io::Result<()> {
    use std::os::windows::io::AsRawHandle as _;
    let flags = if inheritable { INHERIT } else { 0 };
    // SAFETY: `handle` is open for the call, which changes one flag.
    if unsafe { SetHandleInformation(handle.as_raw_handle(), INHERIT, flags) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// A handle's value, as a child is told of it.
pub fn value(handle: RawHandle) -> usize {
    handle.expose_provenance()
}
