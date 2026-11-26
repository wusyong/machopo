//! A pipe a process is driven through, as Chrome's
//! `--remote-debugging-pipe` is (Chromium's `DevToolsPipeHandler`): the
//! process reads what it is told from one end and writes to the other,
//! file descriptors 3 and 4 on macOS and Linux, and on Windows the two
//! handles `--remote-debugging-io-pipes=<read>,<write>` names (Chromium's
//! `AdoptPipes`). What is said on it is the caller's.
//!
//! The process takes its ends with [`inherited_pipe`]; whatever starts it
//! hands them over with [`spawn_with_pipe`] (Chromium's
//! `LaunchOptions::fds_to_remap`, and `handles_to_inherit` on Windows), or
//! as Chrome's clients do (Node puts its fourth and fifth stdio at 3 and
//! 4; Python's `pass_fds` and a shell's `3<&N 4>&M` do as much). Once
//! taken, the ends are this process's alone: nothing it starts has them,
//! so a client hears the pipe end when this process does.

use std::fmt;
use std::io::{self, PipeReader, PipeWriter};
use std::process::{Child, Command};

/// The descriptor the process reads (Chromium's `kReadFD`).
#[cfg(unix)]
pub const INPUT_FD: i32 = 3;
/// The descriptor the process writes (Chromium's `kWriteFD`).
#[cfg(unix)]
pub const OUTPUT_FD: i32 = 4;

/// On Windows, the switch that names the two handles, Chrome's.
pub const IO_PIPES_SWITCH: &str = "--remote-debugging-io-pipes";

/// One of the pipe's two ends, as the driven process has them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipeEnd {
    /// What it reads: descriptor 3, or the first handle named.
    Input,
    /// What it writes: descriptor 4, or the second handle named.
    Output,
}

/// Why [`inherited_pipe`] has no pipe to give.
#[derive(Debug)]
pub enum InheritError {
    /// macOS and Linux: nothing is open at that end's descriptor.
    Missing(PipeEnd),
    /// What is at that end is neither a pipe nor a socket (on Windows, no
    /// pipe).
    NotAPipe(PipeEnd),
    /// Windows: no handles were named.
    NoHandles,
    /// Windows: what names the handles isn't two numbers, by a comma.
    BadHandles,
    /// The ends were taken already, by an earlier call.
    Taken,
    /// The system wouldn't keep them from what this process starts.
    Io(io::Error),
}

impl fmt::Display for InheritError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(end) => write!(f, "nothing is open at {}", end.name()),
            Self::NotAPipe(end) => write!(f, "{} is no pipe", end.name()),
            Self::NoHandles => write!(f, "no {IO_PIPES_SWITCH}=<read>,<write> names the pipe"),
            Self::BadHandles => write!(f, "{IO_PIPES_SWITCH} wants two handles: <read>,<write>"),
            Self::Taken => write!(f, "the pipe was taken already"),
            Self::Io(error) => write!(f, "the pipe couldn't be kept to this process: {error}"),
        }
    }
}

impl std::error::Error for InheritError {}

impl PipeEnd {
    fn name(self) -> &'static str {
        match (self, cfg!(windows)) {
            (Self::Input, false) => "descriptor 3 (the pipe's input)",
            (Self::Output, false) => "descriptor 4 (the pipe's output)",
            (Self::Input, true) => "the first handle (the pipe's input)",
            (Self::Output, true) => "the second handle (the pipe's output)",
        }
    }
}

/// In a process started with a pipe: its ends, the one it reads and the
/// one it writes, once. `io_pipes` is `--remote-debugging-io-pipes`'
/// value, which the caller parsed: Windows alone looks at it, and macOS
/// and Linux read 3 and 4 whatever it says, as Chromium does. Asked for a
/// second time, whatever came of the first, it is [`InheritError::Taken`].
///
/// Both are checked to be pipes (or, on macOS and Linux, sockets: Node's
/// stdio pipes are a socket pair) before they are taken, and once taken
/// they are kept from whatever this process starts: close-on-exec, or not
/// inheritable. Chromium reads whatever is there; a process here started
/// without its pipe says so instead.
///
/// On macOS and Linux it is to be called before this process opens
/// anything: with 3 and 4 closed at its start, what it opens next would
/// be given those numbers, and taken for the pipe.
pub fn inherited_pipe(io_pipes: Option<&str>) -> Result<(PipeReader, PipeWriter), InheritError> {
    imp::inherited_pipe(io_pipes)
}

/// Starts `command` with a new pipe: the child reads what is written to
/// the [`PipeWriter`] and what it writes comes out of the [`PipeReader`]
/// (it takes its ends with [`inherited_pipe`]). On macOS and Linux its ends
/// are its descriptors 3 and 4; on Windows they are inherited handles,
/// which `--remote-debugging-io-pipes` at the end of `command`'s arguments
/// names. No other child has them: on Windows, one is started at a time
/// with handles made inheritable for it, this and the channel's
/// [`super::spawn`] alike.
///
/// `command` is spent: what was added to it stays, and on macOS and Linux
/// it hands over numbers that are closed once it has started.
pub fn spawn_with_pipe(command: &mut Command) -> io::Result<(Child, PipeWriter, PipeReader)> {
    imp::spawn_with_pipe(command)
}

#[cfg(unix)]
mod imp {
    use std::io::{self, PipeReader, PipeWriter};
    use std::os::fd::{AsFd as _, AsRawFd as _, BorrowedFd, FromRawFd as _, OwnedFd, RawFd};
    use std::os::unix::process::CommandExt as _;
    use std::process::{Child, Command};
    use std::sync::atomic::{AtomicBool, Ordering};

    use rustix::fs::{FileType, fstat};
    use rustix::io::{Errno, FdFlags, fcntl_dupfd_cloexec, fcntl_setfd};

    use super::{INPUT_FD, InheritError, OUTPUT_FD, PipeEnd};

    /// The first number past the pipe's: a child's ends are moved here
    /// before they are put at 3 and 4, so that neither is in the other's
    /// way.
    const PAST_THE_PIPE: RawFd = OUTPUT_FD + 1;

    pub(super) fn inherited_pipe(
        _io_pipes: Option<&str>,
    ) -> Result<(PipeReader, PipeWriter), InheritError> {
        static TAKEN: AtomicBool = AtomicBool::new(false);
        // Asked once, whatever comes of it: once 3 and 4 are taken, or
        // found to be no pipe, what is at those numbers later is whatever
        // this process opened there.
        if TAKEN.swap(true, Ordering::SeqCst) {
            return Err(InheritError::Taken);
        }
        check(INPUT_FD, PipeEnd::Input)?;
        check(OUTPUT_FD, PipeEnd::Output)?;
        // SAFETY: 3 and 4 are open (just asked), and are what this
        // process was started with for it to take: open at its start and
        // given to nothing else in it, which the caller sees to by taking
        // them first. They are asked for once (`TAKEN`), so these values
        // alone close them.
        let (input, output) = unsafe {
            (
                OwnedFd::from_raw_fd(INPUT_FD),
                OwnedFd::from_raw_fd(OUTPUT_FD),
            )
        };
        // What this process starts doesn't have them.
        for fd in [&input, &output] {
            fcntl_setfd(fd, FdFlags::CLOEXEC).map_err(|e| InheritError::Io(e.into()))?;
        }
        Ok((PipeReader::from(input), PipeWriter::from(output)))
    }

    /// Whether `number` is open, and a pipe or a socket.
    fn check(number: RawFd, end: PipeEnd) -> Result<(), InheritError> {
        // SAFETY: borrowed for the one call below, which only asks what
        // the descriptor is; a number that isn't open is an error from it.
        let fd = unsafe { BorrowedFd::borrow_raw(number) };
        match fstat(fd) {
            Ok(stat) => match FileType::from_raw_mode(stat.st_mode) {
                FileType::Fifo | FileType::Socket => Ok(()),
                _ => Err(InheritError::NotAPipe(end)),
            },
            Err(Errno::BADF) => Err(InheritError::Missing(end)),
            Err(error) => Err(InheritError::Io(error.into())),
        }
    }

    pub(super) fn spawn_with_pipe(
        command: &mut Command,
    ) -> io::Result<(Child, PipeWriter, PipeReader)> {
        // Both close-on-exec, as `std::io::pipe` makes them.
        let (their_input, our_writer) = io::pipe()?;
        let (our_reader, their_output) = io::pipe()?;
        let their_input = past_the_pipe(their_input.into())?;
        let their_output = past_the_pipe(their_output.into())?;
        let (input, output) = (their_input.as_raw_fd(), their_output.as_raw_fd());
        // SAFETY: the closure runs in the child between fork and exec,
        // where only what is async-signal-safe may be called (another
        // thread may have held any lock at the fork): it makes two `dup2`
        // system calls and allocates nothing (an error is made of its
        // number alone). `input` and `output` are open in the child: their
        // owners live in this process until `spawn` below has returned, so
        // they were at the fork. Both are past 4, so neither is the other's
        // target, and each copy at 3 and 4 is a new descriptor, without
        // close-on-exec, which the exec keeps; the originals close there.
        unsafe {
            command.pre_exec(move || {
                for (from, to) in [(input, INPUT_FD), (output, OUTPUT_FD)] {
                    if libc::dup2(from, to) == -1 {
                        return Err(io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
        let child = command.spawn()?;
        // The child's ends are the child's alone from here: when it goes,
        // the reader reads the end.
        drop((their_input, their_output));
        Ok((child, our_writer, our_reader))
    }

    /// `fd`, at a number past the pipe's (a copy, close-on-exec, if it
    /// isn't): a process started with 3 or 4 closed makes its pipes there.
    fn past_the_pipe(fd: OwnedFd) -> io::Result<OwnedFd> {
        if fd.as_raw_fd() >= PAST_THE_PIPE {
            return Ok(fd);
        }
        Ok(fcntl_dupfd_cloexec(fd.as_fd(), PAST_THE_PIPE)?)
    }
}

#[cfg(windows)]
mod imp {
    use std::io::{self, PipeReader, PipeWriter};
    use std::os::windows::io::{AsHandle as _, AsRawHandle as _, FromRawHandle as _, OwnedHandle};
    use std::process::{Child, Command};
    use std::sync::atomic::{AtomicBool, Ordering};

    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::Storage::FileSystem::{FILE_TYPE_PIPE, GetFileType};

    use super::super::windows::channel::{inherited_by_children, starting_a_child};
    use super::{IO_PIPES_SWITCH, InheritError, PipeEnd};

    pub(super) fn inherited_pipe(
        io_pipes: Option<&str>,
    ) -> Result<(PipeReader, PipeWriter), InheritError> {
        static TAKEN: AtomicBool = AtomicBool::new(false);
        let value = io_pipes.ok_or(InheritError::NoHandles)?;
        let (input, output) = value.split_once(',').ok_or(InheritError::BadHandles)?;
        // Asked once, whatever comes of it, as on macOS and Linux.
        if TAKEN.swap(true, Ordering::SeqCst) {
            return Err(InheritError::Taken);
        }
        let input = named(input, PipeEnd::Input)?;
        let output = named(output, PipeEnd::Output)?;
        if input == output {
            return Err(InheritError::BadHandles);
        }
        // SAFETY: what started this process, which decides how it starts,
        // put these two handles in it for this and named them; each was
        // just seen to be a pipe, they are two, and `TAKEN` sees that they
        // are owned once.
        let (input, output) = unsafe {
            (
                OwnedHandle::from_raw_handle(input),
                OwnedHandle::from_raw_handle(output),
            )
        };
        // Inherited, they would be inherited again by this process's own
        // children.
        for handle in [&input, &output] {
            if !inherited_by_children(handle.as_handle(), false) {
                return Err(InheritError::Io(io::Error::last_os_error()));
            }
        }
        Ok((PipeReader::from(input), PipeWriter::from(output)))
    }

    /// The handle `value` names, if it is a pipe of this process's.
    fn named(value: &str, end: PipeEnd) -> Result<HANDLE, InheritError> {
        let value: usize = value.parse().map_err(|_| InheritError::BadHandles)?;
        let handle: HANDLE = std::ptr::with_exposed_provenance_mut(value);
        // A value that names nothing, or something that isn't a pipe, is
        // said so by the call. (Asked of what is no handle, it says so.)
        // SAFETY: the call only looks the value up.
        if unsafe { GetFileType(handle) } != FILE_TYPE_PIPE {
            return Err(InheritError::NotAPipe(end));
        }
        Ok(handle)
    }

    pub(super) fn spawn_with_pipe(
        command: &mut Command,
    ) -> io::Result<(Child, PipeWriter, PipeReader)> {
        // Neither inheritable, as `std::io::pipe` makes them.
        let (their_input, our_writer) = io::pipe()?;
        let (our_reader, their_output) = io::pipe()?;
        let (their_input, their_output) = (
            OwnedHandle::from(their_input),
            OwnedHandle::from(their_output),
        );
        let values = [&their_input, &their_output].map(|h| h.as_raw_handle().expose_provenance());
        command.arg(format!("{IO_PIPES_SWITCH}={},{}", values[0], values[1]));
        let child = {
            let _one_at_a_time = starting_a_child();
            for handle in [&their_input, &their_output] {
                if !inherited_by_children(handle.as_handle(), true) {
                    return Err(io::Error::last_os_error());
                }
            }
            let child = command.spawn();
            // Closed with the lock held: the child's are the only ones
            // left, so the reader reads the end when the child goes, and
            // no child started after has them.
            drop((their_input, their_output));
            child?
        };
        Ok((child, our_writer, our_reader))
    }
}
