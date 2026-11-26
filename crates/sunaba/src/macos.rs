//! macOS: the kernel's sandbox (Seatbelt, `sandbox(7)`) under a profile
//! the process hands it (Chromium's `sandbox/mac`: `Seatbelt`).
//!
//! A profile is a program in the sandbox's own language that says what
//! the process may still ask for: files by path, Mach services by name,
//! `sysctl`s by name, and so on. [`enter`] is for the start of `main`: the
//! profile is the whole process's from then on, whatever threads it has,
//! and that of every process it starts.

pub mod probe;

use std::ffi::{CStr, CString, c_char, c_int};
use std::fmt;

/// What a process may still ask of the system once it has entered: the
/// profile's text, and the values of the parameters it reads with
/// `(param "NAME")` (a path found at run time, a process's id).
#[derive(Clone, Debug)]
pub struct Profile {
    source: String,
    parameters: Vec<(String, String)>,
}

impl Profile {
    /// `source` in the sandbox's profile language, starting with
    /// `(version 1)`.
    pub fn new(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            parameters: Vec::new(),
        }
    }

    /// What `(param "name")` is in the profile. A value is never part of
    /// the profile's text, so nothing in it is read as a rule.
    pub fn parameter(&mut self, name: impl Into<String>, value: impl Into<String>) -> &mut Self {
        self.parameters.push((name.into(), value.into()));
        self
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn parameters(&self) -> &[(String, String)] {
        &self.parameters
    }
}

/// Why the process isn't in the sandbox. A process that gets one of these
/// is to end, not to go on outside.
#[derive(Debug)]
pub enum Error {
    /// The profile or a parameter has a NUL in it.
    Nul,
    /// The system wouldn't take the profile: it doesn't compile (the
    /// message says where), or the process is in a sandbox already.
    Refused(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Nul => write!(
                f,
                "the sandbox's profile or a parameter of it has a NUL in it"
            ),
            Self::Refused(why) => write!(f, "the system refused the sandbox's profile: {why}"),
        }
    }
}

impl std::error::Error for Error {}

// libsystem_sandbox's, part of libSystem. `sandbox_init_with_parameters`
// is in no header (Chromium and Firefox declare it the same way);
// `sandbox_free_error` is `sandbox.h`'s.
unsafe extern "C" {
    fn sandbox_init_with_parameters(
        profile: *const c_char,
        flags: u64,
        parameters: *const *const c_char,
        error: *mut *mut c_char,
    ) -> c_int;
    fn sandbox_free_error(error: *mut c_char);
    fn sandbox_check(pid: libc::pid_t, operation: *const c_char, filter: c_int, ...) -> c_int;
}

/// Enters `profile`'s sandbox, for good: this process and every process
/// it starts. Call it first thing in `main`.
///
/// On an error the process isn't restricted and is to end.
pub fn enter(profile: &Profile) -> Result<(), Error> {
    let source = CString::new(profile.source.as_str()).map_err(|_| Error::Nul)?;
    let mut strings = Vec::new();
    for (name, value) in &profile.parameters {
        strings.push(CString::new(name.as_str()).map_err(|_| Error::Nul)?);
        strings.push(CString::new(value.as_str()).map_err(|_| Error::Nul)?);
    }
    // Names and values in turn, then a null.
    let mut parameters: Vec<*const c_char> = strings.iter().map(|string| string.as_ptr()).collect();
    parameters.push(std::ptr::null());
    let mut message: *mut c_char = std::ptr::null_mut();
    // SAFETY: `source` and every string of `parameters` are C strings that
    // live past the call, the array ends with a null as the function
    // expects, and `message` is a place for one pointer. The function
    // reads them and keeps none.
    let result = unsafe {
        sandbox_init_with_parameters(source.as_ptr(), 0, parameters.as_ptr(), &raw mut message)
    };
    let why = if message.is_null() {
        std::io::Error::last_os_error().to_string()
    } else {
        // SAFETY: on a failure the function leaves a C string it
        // allocated, which is copied here and freed once, by the function
        // it is to be freed with.
        unsafe {
            let why = CStr::from_ptr(message).to_string_lossy().trim().to_owned();
            sandbox_free_error(message);
            why
        }
    };
    if result == 0 {
        Ok(())
    } else {
        Err(Error::Refused(why))
    }
}

/// Whether this process is in a sandbox, by whatever means it got there.
pub fn entered() -> bool {
    sandboxed(std::process::id())
}

/// Whether the process `pid` is in a sandbox, by the kernel's account:
/// for whoever started a process to see that it entered. Only of a
/// process that is running: the kernel answers for one that isn't there
/// as for one in a sandbox.
pub fn sandboxed(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    // SAFETY: with no operation named the function reads nothing but the
    // process id, and answers whether that process has a sandbox at all
    // (1), or not (0).
    unsafe { sandbox_check(pid, std::ptr::null(), 0) == 1 }
}
