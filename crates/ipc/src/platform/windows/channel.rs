//! A channel's end over a named pipe (Chromium's
//! `mojo/core/channel_win.cc`): overlapped reads and writes, whose
//! completions come to the completion port of the IO thread that serves
//! the end ([`IocpWaiter`]), one port for all of a process's channels.
//!
//! The port is not a convenience. A write that can't finish at once goes
//! on in the kernel after `write` returns, and Windows cancels what a
//! thread started when the thread is over, unless the handle is on a
//! completion port: any thread writes here, and a thread that is over
//! must not take its message with it. So an end writes nothing before
//! its IO thread serves it: until then every write is refused as one
//! that would block, and the core keeps the message.
//!
//! A write is of [`RECORD_MAX`] bytes at most, and one goes on at a
//! time. What the kernel is done with when the call returns is over
//! then, and leaves nothing at the port: with room in the pipe any
//! thread writes one message after another without the IO thread's
//! having to wake for each (Chromium's waits for the port every time;
//! measured, that is half as fast for one channel and a seventh as fast
//! for four). Only what the kernel couldn't finish at once goes on, and
//! holds its end, until its completion is taken off the port.
//!
//! The pipe carries bytes alone, so the handles that go with a message
//! go as values, and only from a host to its child: before each of a
//! host's writes is a short header of this layer's own, with how many
//! bytes follow, how many handles, and the value each handle has in the
//! child's process, where the host duplicated it. What a child writes
//! has no header: its bytes are the core's as they are, so a host makes
//! nothing of a child's bytes but what the core does, and has no way to
//! take a value for a handle.

use std::collections::VecDeque;
use std::hash::{BuildHasher, RandomState};
use std::io;
use std::os::windows::io::{AsHandle, AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle};
use std::process::{Child, Command};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use base::AtomicSequenceNumber;
use windows_sys::Win32::Foundation::{
    DUPLICATE_CLOSE_SOURCE, DUPLICATE_SAME_ACCESS, DuplicateHandle, ERROR_IO_PENDING,
    ERROR_PIPE_CONNECTED, GetLastError, HANDLE, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE,
    SetHandleInformation,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_APPEND_DATA, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED,
    FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_TYPE_PIPE, GetFileType, OPEN_EXISTING,
    PIPE_ACCESS_DUPLEX, ReadFile, SECURITY_ANONYMOUS, SECURITY_SQOS_PRESENT,
    SetFileCompletionNotificationModes, WriteFile,
};
use windows_sys::Win32::System::IO::{
    CancelIoEx, CreateIoCompletionPort, GetQueuedCompletionStatus, OVERLAPPED,
    PostQueuedCompletionStatus,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
    PIPE_TYPE_BYTE,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, INFINITE, PROCESS_DUP_HANDLE};

use super::job;
use super::memory::{self, Mapping};
use crate::core::channel::{Transport, WriteError};
use crate::core::frame::MAX_HANDLES;
use crate::endpoint::{Event, Key, Platform, Waited, Waiter, Wake};

/// The most one write puts in the pipe, a host's header and all: what
/// the pipe's buffer holds, so a write into an empty pipe is done at
/// once. The rest of a longer message stays with the core until the
/// write is over. It is also what a read may bring: one write.
pub const RECORD_MAX: usize = 64 << 10;
/// The pipe's buffer, each way.
const PIPE_BUFFER: u32 = 64 << 10;
/// A host's header: how many bytes follow it, and how many handles'
/// values come before them.
const HEADER_LEN: usize = 8;
/// A handle's value in a header.
const VALUE_LEN: usize = 8;
/// The most handles' values a header has.
const VALUES_MAX: usize = MAX_HANDLES as usize;
/// `FILE_SKIP_COMPLETION_PORT_ON_SUCCESS` (`<winbase.h>`): a read or a
/// write that is done when its call returns leaves nothing at the port.
const SKIP_PORT_ON_SUCCESS: u8 = 1;
/// How many times in a row the IO thread takes up what its ends have
/// read already before it looks at the port: an end whose reads are
/// always done at once doesn't keep the others' completions waiting.
const TURNS_BEFORE_THE_PORT: usize = 8;
/// How many names are tried for a pipe before there is none.
const NAMES_TRIED: usize = 8;
/// What `CreateNamedPipeW` is told a client's `WaitNamedPipe` waits by
/// default: nothing here calls that.
const DEFAULT_WAIT_MS: u32 = 5000;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // Nothing panics while holding any of them.
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn last_error() -> u32 {
    // SAFETY: reads this thread's last error.
    unsafe { GetLastError() }
}

fn zeroed_overlapped() -> OVERLAPPED {
    // SAFETY: all zeroes is an `OVERLAPPED`, with no event.
    unsafe { std::mem::zeroed() }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Read,
    Write,
}

/// What a read or a write has the kernel's hands on while it goes on:
/// where the kernel says how it went, and the bytes. An end has one of
/// each kind, made with it and freed with it, and each stays where it
/// is: a completion names its operation by the address of `overlapped`.
///
/// While one goes on, the kernel writes `overlapped` (and a read's
/// `buffer`) whenever it likes: nothing here refers to either then.
#[repr(C)]
struct Operation {
    /// First, so its address is the operation's.
    overlapped: OVERLAPPED,
    kind: Kind,
    /// The end this is an operation of: set once, before the end is
    /// anyone else's. An operation going on holds the end, by one count
    /// of its `Arc`, until its completion is taken off the port: the
    /// end, and so this memory, outlives the kernel's use of it.
    end: *const End,
    buffer: Box<[u8]>,
}

fn operation(kind: Kind) -> *mut Operation {
    Box::into_raw(Box::new(Operation {
        overlapped: zeroed_overlapped(),
        kind,
        end: null(),
        buffer: vec![0; RECORD_MAX].into_boxed_slice(),
    }))
}

/// The sending side, under one lock: one write goes on at a time.
struct Writing {
    /// An IO thread serves this end: its pipe is on that thread's port.
    joined: bool,
    /// A write was started and its completion not yet taken off the
    /// port.
    going: bool,
    /// How many bytes the write going on is to put in the pipe.
    len: usize,
    /// A write was refused since the last time there was room: the IO
    /// thread says when there is room again.
    blocked: bool,
}

/// What the IO thread alone uses: the bytes read and not yet lent are
/// the read operation's buffer from `start` to `end`.
struct Reading {
    start: usize,
    end: usize,
}

/// What an end is, to its own value, to the waiter of the IO thread
/// that serves it, and to its operations going on.
struct End {
    /// `None` once the channel is shut. Locked after `writing` and
    /// `reading` wherever both are.
    pipe: Mutex<Option<OwnedHandle>>,
    /// A host's: its child's process, to duplicate handles into and for
    /// nothing else. `None` for a child's end, and once shut.
    peer: Mutex<Option<OwnedHandle>>,
    /// This is a host's end: what it writes has headers, what it reads
    /// has none.
    host: bool,
    writing: Mutex<Writing>,
    reading: Mutex<Reading>,
    /// Touched only while it isn't going on: by the IO thread.
    read: *mut Operation,
    /// Touched only while it isn't going on: under `writing`.
    write: *mut Operation,
}

// SAFETY: the two pointers are this end's own allocations. What they
// point to is used by one thread at a time (the read's by the IO thread,
// the write's under `writing`), and by none while the kernel has it.
unsafe impl Send for End {}
// SAFETY: as above.
unsafe impl Sync for End {}

impl Drop for End {
    fn drop(&mut self) {
        // SAFETY: made by `operation`, freed once, here. Neither goes
        // on: one that did would hold this end.
        unsafe {
            drop(Box::from_raw(self.read));
            drop(Box::from_raw(self.write));
        }
    }
}

/// What the bytes read so far have for the IO thread.
enum Next {
    /// A record's bytes: `len` of them at this address of the read's
    /// buffer, which isn't read into until they were lent.
    Lend(*const u8, usize),
    /// Nothing until more is read.
    More,
    /// What no host writes.
    Bad,
}

/// How a read that was started stands.
enum Started {
    /// It goes on: its completion comes to the port.
    Going,
    /// Done at once: what it read is in the buffer, and nothing comes
    /// to the port.
    Done,
    /// The pipe has no more to give.
    Over,
}

fn u32_at(bytes: &[u8], at: usize) -> Option<usize> {
    let end = at.checked_add(4)?;
    usize::try_from(u32::from_le_bytes(bytes.get(at..end)?.try_into().ok()?)).ok()
}

impl End {
    /// The count an operation about to go on holds this end by.
    fn hold(self: &Arc<Self>) {
        let _ = Arc::into_raw(Arc::clone(self));
    }

    /// Takes back the count of an operation that didn't start.
    fn let_go(self: &Arc<Self>) {
        // SAFETY: the count `hold` just added, which no completion will
        // come to take.
        unsafe { Arc::decrement_strong_count(Arc::as_ptr(self)) };
    }

    fn shut(&self) {
        let pipe = lock(&self.pipe).take();
        if let Some(pipe) = &pipe {
            // What is going on is cancelled, whichever thread started
            // it: each still leaves its completion at the port.
            // SAFETY: `pipe` is open for the call.
            unsafe { CancelIoEx(pipe.as_raw_handle(), null()) };
        }
        drop(pipe);
        drop(lock(&self.peer).take());
    }

    /// Makes of the bytes read what there is to make. For the IO
    /// thread, with no read going on.
    fn next(&self, handles: &mut Vec<OwnedHandle>) -> Next {
        let mut reading = lock(&self.reading);
        // SAFETY: no read is going on, and only this thread starts one.
        let buffer = unsafe { &(*self.read).buffer };
        let unread = buffer.get(reading.start..reading.end).unwrap_or_default();
        if self.host {
            // A child's bytes are the core's as they are.
            if unread.is_empty() {
                return Next::More;
            }
            reading.start = reading.end;
            return Next::Lend(unread.as_ptr(), unread.len());
        }
        let (Some(len), Some(count)) = (u32_at(unread, 0), u32_at(unread, 4)) else {
            return Next::More;
        };
        let header = HEADER_LEN.saturating_add(count.saturating_mul(VALUE_LEN));
        let whole = header.saturating_add(len);
        // A record is never empty, and never more than one write makes.
        if len == 0 || count > VALUES_MAX || whole > RECORD_MAX {
            return Next::Bad;
        }
        let Some(record) = unread.get(..whole) else {
            return Next::More;
        };
        let (values, bytes) = record
            .get(HEADER_LEN..)
            .unwrap_or_default()
            .split_at(count.saturating_mul(VALUE_LEN));
        for value in values.as_chunks::<VALUE_LEN>().0 {
            let Ok(value) = usize::try_from(u64::from_le_bytes(*value)) else {
                return Next::Bad;
            };
            let handle: HANDLE = std::ptr::with_exposed_provenance_mut(value);
            // Neither is a handle a process is given.
            if handle.is_null() || handle == INVALID_HANDLE_VALUE {
                return Next::Bad;
            }
            // SAFETY: the peer is the host, which is trusted: it put
            // this handle in this process for this message, and tells of
            // it once.
            handles.push(unsafe { OwnedHandle::from_raw_handle(handle) });
        }
        reading.start = reading.start.saturating_add(whole);
        Next::Lend(bytes.as_ptr(), bytes.len())
    }

    /// Starts a read for what the buffer has room for. For the IO
    /// thread, with no read going on.
    fn read(self: &Arc<Self>) -> Started {
        let mut reading = lock(&self.reading);
        // SAFETY: no read is going on, and only this thread starts one.
        let operation = unsafe { &mut *self.read };
        // What is unread (the start of a record) goes to the buffer's
        // start, so the rest of its record fits.
        operation.buffer.copy_within(reading.start..reading.end, 0);
        reading.end = reading.end.saturating_sub(reading.start);
        reading.start = 0;
        let Some(room) = operation.buffer.get_mut(reading.end..) else {
            return Started::Over;
        };
        let Ok(len) = u32::try_from(room.len()) else {
            return Started::Over;
        };
        let pipe = lock(&self.pipe);
        let Some(pipe) = pipe.as_ref() else {
            return Started::Over;
        };
        if len == 0 {
            return Started::Over;
        }
        operation.overlapped = zeroed_overlapped();
        let mut read = 0;
        // Held before the call: a read that goes on may be over, and
        // its completion taken, before the call is back.
        self.hold();
        // SAFETY: `pipe` is open and `read` alive for the call. `room`
        // and the `OVERLAPPED` are the read operation's, which stays
        // where it is and, if the read goes on, is held by the count
        // just taken until the read's completion is taken off the port;
        // nothing refers to either until then.
        let done = unsafe {
            ReadFile(
                pipe.as_raw_handle(),
                room.as_mut_ptr(),
                len,
                &raw mut read,
                &raw mut operation.overlapped,
            ) != 0
        };
        if !done && last_error() == ERROR_IO_PENDING {
            return Started::Going;
        }
        // Over with the call, one way or the other: no completion
        // comes for it.
        self.let_go();
        let read = usize::try_from(read).unwrap_or(0);
        // A read that failed, or brought nothing, is the pipe's end.
        if !done || read == 0 {
            return Started::Over;
        }
        reading.end = reading.end.saturating_add(read);
        Started::Done
    }
}

/// A channel's end.
// An `Arc`: the IO thread's waiter reaches the end too, and so does
// each of its operations while it goes on.
pub struct Channel(Arc<End>);

impl Drop for Channel {
    fn drop(&mut self) {
        // The end may be held a while yet (by its waiter, by what goes
        // on): its handles aren't.
        self.0.shut();
    }
}

/// Another handle to the process `process` is, with the right to
/// duplicate handles into it and no other.
fn to_duplicate_into(process: BorrowedHandle<'_>) -> Option<OwnedHandle> {
    let mut out: HANDLE = null_mut();
    // SAFETY: `process` is open for the call, both tables are this
    // process's, and `out` is alive for the call.
    let made = unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            process.as_raw_handle(),
            GetCurrentProcess(),
            &raw mut out,
            PROCESS_DUP_HANDLE,
            0,
            0,
        )
    };
    // SAFETY: the call succeeded: a new handle, this process's alone.
    (made != 0).then(|| unsafe { OwnedHandle::from_raw_handle(out) })
}

/// Puts `handle` in the process `peer` is, with the rights it has here:
/// its value there.
fn give(peer: &OwnedHandle, handle: &OwnedHandle) -> Option<u64> {
    let mut out: HANDLE = null_mut();
    // SAFETY: both handles are open for the call, and `out` is alive
    // for it.
    let made = unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            handle.as_raw_handle(),
            peer.as_raw_handle(),
            &raw mut out,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        )
    };
    if made == 0 {
        return None;
    }
    u64::try_from(out.expose_provenance()).ok()
}

/// Closes, in the process `peer` is, the handles `give` put there for a
/// message that didn't go.
fn take_back(peer: &OwnedHandle, values: &[u64]) {
    for value in values {
        let Ok(value) = usize::try_from(*value) else {
            continue;
        };
        // SAFETY: `peer` is open for the call. The value is one `give`
        // just made in that process, which the peer was never told of;
        // with no target, the call only closes it there.
        unsafe {
            DuplicateHandle(
                peer.as_raw_handle(),
                std::ptr::with_exposed_provenance_mut(value),
                null_mut(),
                null_mut(),
                0,
                0,
                DUPLICATE_CLOSE_SOURCE,
            )
        };
    }
}

impl Channel {
    /// A host's end: `pipe` is the server's end of [`pipe`], and `child`
    /// the process that has the client's. `None` when handles can't be
    /// duplicated into that process.
    pub fn host(pipe: OwnedHandle, child: BorrowedHandle<'_>) -> Option<Self> {
        Some(Self::new(pipe, Some(to_duplicate_into(child)?)))
    }

    /// A child's end: `pipe` is the client's end of [`pipe`].
    pub fn child(pipe: OwnedHandle) -> Self {
        Self::new(pipe, None)
    }

    fn new(pipe: OwnedHandle, peer: Option<OwnedHandle>) -> Self {
        let end = Arc::new(End {
            pipe: Mutex::new(Some(pipe)),
            host: peer.is_some(),
            peer: Mutex::new(peer),
            writing: Mutex::new(Writing {
                joined: false,
                going: false,
                len: 0,
                blocked: false,
            }),
            reading: Mutex::new(Reading { start: 0, end: 0 }),
            read: operation(Kind::Read),
            write: operation(Kind::Write),
        });
        let held = Arc::into_raw(Arc::clone(&end));
        // SAFETY: nothing else has the operations yet. The pointer is
        // the `Arc`'s own, and the count it came with is given back at
        // once: an operation holds the end only while it goes on.
        unsafe {
            (*end.read).end = held;
            (*end.write).end = held;
            Arc::decrement_strong_count(held);
        }
        Self(end)
    }
}

impl Transport for Channel {
    type Handle = OwnedHandle;

    fn write(&self, bytes: &[u8], handles: &mut Vec<OwnedHandle>) -> Result<usize, WriteError> {
        let end = &self.0;
        let mut writing = lock(&end.writing);
        let pipe = lock(&end.pipe);
        let Some(pipe) = pipe.as_ref() else {
            return Err(WriteError::Closed);
        };
        if bytes.is_empty() {
            return Ok(0);
        }
        // Not before an IO thread has the pipe on its port, and one at a
        // time.
        if !writing.joined || writing.going {
            writing.blocked = true;
            return Err(WriteError::WouldBlock);
        }
        // SAFETY: no write is going on, and `writing` is held: the
        // operation is this call's alone.
        let operation = unsafe { &mut *end.write };
        let mut values = Vec::new();
        let peer = lock(&end.peer);
        let header = if end.host {
            HEADER_LEN.saturating_add(handles.len().saturating_mul(VALUE_LEN))
        } else {
            0
        };
        let taken = bytes.len().min(RECORD_MAX.saturating_sub(header));
        let whole = header.saturating_add(taken);
        let (Some(record), Ok(len), true) = (
            operation.buffer.get_mut(..whole),
            u32::try_from(whole),
            handles.len() <= VALUES_MAX && taken > 0,
        ) else {
            // More handles than a message has.
            return Err(WriteError::Closed);
        };
        if end.host {
            let (Some(peer), Ok(taken), Ok(count)) = (
                peer.as_ref(),
                u32::try_from(taken),
                u32::try_from(handles.len()),
            ) else {
                return Err(WriteError::Closed);
            };
            for handle in handles.iter() {
                let Some(value) = give(peer, handle) else {
                    // The child is gone.
                    take_back(peer, &values);
                    return Err(WriteError::Closed);
                };
                values.push(value);
            }
            let mut head = Vec::with_capacity(header);
            head.extend_from_slice(&taken.to_le_bytes());
            head.extend_from_slice(&count.to_le_bytes());
            for value in &values {
                head.extend_from_slice(&value.to_le_bytes());
            }
            record
                .get_mut(..header)
                .unwrap_or_default()
                .copy_from_slice(&head);
        }
        record
            .get_mut(header..)
            .unwrap_or_default()
            .copy_from_slice(bytes.get(..taken).unwrap_or_default());
        operation.overlapped = zeroed_overlapped();
        let mut written = 0;
        // Set before the call: a write that goes on may be over, and
        // its completion taken, before the call is back.
        writing.going = true;
        writing.len = whole;
        end.hold();
        // SAFETY: `pipe` is open and `written` alive for the call.
        // `record` and the `OVERLAPPED` are the write operation's, which
        // stays where it is and, if the write goes on, is held by the
        // count just taken until the write's completion is taken off
        // the port; nothing refers to either until then.
        let done = unsafe {
            WriteFile(
                pipe.as_raw_handle(),
                record.as_ptr(),
                len,
                &raw mut written,
                &raw mut operation.overlapped,
            ) != 0
        };
        if done || last_error() != ERROR_IO_PENDING {
            // Over with the call, one way or the other: no completion
            // comes for it.
            writing.going = false;
            end.let_go();
            // A pipe takes all of a write or fails it.
            if !done || written != len {
                if let Some(peer) = peer.as_ref() {
                    take_back(peer, &values);
                }
                return Err(WriteError::Closed);
            }
        }
        // A host's are in the child now, and these were this end's; a
        // child has no way to give one, and lets go of what it was
        // handed.
        handles.clear();
        Ok(taken)
    }
}

impl Platform for Channel {
    type Memory = Mapping;
    type Waiter = IocpWaiter;

    fn map(&self, handle: OwnedHandle, len: usize, writable: bool) -> Option<Mapping> {
        memory::map(&handle, len, writable)
    }

    fn create(&self, len: usize, peer_writes: bool) -> Option<(Mapping, OwnedHandle)> {
        memory::create(len, peer_writes)
    }

    fn shut(&self) {
        self.0.shut();
    }
}

/// Both ends of a new pipe and its name: the server's and the client's,
/// each for reads and writes that don't wait, and neither inherited by a
/// child. Nothing else can have the client's: the pipe has one instance,
/// which the client made here is connected to.
fn named_pipe() -> Option<(OwnedHandle, OwnedHandle, Vec<u16>)> {
    static NEXT: AtomicSequenceNumber<u32> = AtomicSequenceNumber::<u32>::new();
    // Another process that has the name first, or opens it between the
    // two calls below, can only be in the way: then both are made
    // again, under another name.
    for _ in 0..NAMES_TRIED {
        let name: Vec<u16> = format!(
            r"\\.\pipe\machopo.{}.{}.{:016x}",
            std::process::id(),
            NEXT.get_next(),
            // Seeded by the system: a name isn't guessed before it is
            // used.
            RandomState::new().hash_one(0u8),
        )
        .encode_utf16()
        .chain(Some(0))
        .collect();
        // SAFETY: `name` ends in a zero and outlives the call. Fails if
        // the name is anyone's already; no second instance; nobody from
        // another machine.
        let server = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                PIPE_BUFFER,
                PIPE_BUFFER,
                DEFAULT_WAIT_MS,
                null(),
            )
        };
        if server == INVALID_HANDLE_VALUE {
            continue;
        }
        // SAFETY: the call succeeded: a new handle, this process's alone.
        let server = unsafe { OwnedHandle::from_raw_handle(server) };
        // SAFETY: as above. Without the right to append, which for a
        // pipe is the right to make another instance of it; and the
        // server can't act as whoever holds this end.
        let client = unsafe {
            CreateFileW(
                name.as_ptr(),
                (FILE_GENERIC_READ | FILE_GENERIC_WRITE) & !FILE_APPEND_DATA,
                0,
                null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_ANONYMOUS,
                null_mut(),
            )
        };
        if client == INVALID_HANDLE_VALUE {
            continue;
        }
        // SAFETY: the call succeeded: a new handle, this process's alone.
        let client = unsafe { OwnedHandle::from_raw_handle(client) };
        // The server's end has a client, and it can only be the one
        // just made: asked to wait for one, it says one is there.
        // SAFETY: `server` is open for the call.
        let connected = unsafe { ConnectNamedPipe(server.as_raw_handle(), null_mut()) } == 0
            && last_error() == ERROR_PIPE_CONNECTED;
        if connected {
            return Some((server, client, name));
        }
    }
    None
}

/// Both ends of a new pipe: the server's, for [`Channel::host`], and the
/// client's, for the child's [`Channel::child`]. `None` when the system
/// gives none.
pub fn pipe() -> Option<(OwnedHandle, OwnedHandle)> {
    named_pipe().map(|(server, client, _)| (server, client))
}

/// The variable a child started by [`spawn`] finds its end's handle in:
/// the handle's value, which is the same in the child as in its host.
/// (A child's arguments aren't this layer's to add to.)
pub const CHANNEL_VARIABLE: &str = "MACHOPO_IPC_CHANNEL";

/// Whether a child this process starts inherits `handle`.
fn inherited_by_children(handle: &OwnedHandle, inherited: bool) -> bool {
    let flag = if inherited { HANDLE_FLAG_INHERIT } else { 0 };
    // SAFETY: `handle` is open for the call.
    unsafe { SetHandleInformation(handle.as_raw_handle(), HANDLE_FLAG_INHERIT, flag) != 0 }
}

/// Starts `command` as a child that has the other end of a new channel,
/// which it takes with [`inherited`]. The child inherits its end, as
/// every child this process starts meanwhile would: one is started here
/// at a time, and the end is inherited only while it is. A process
/// started elsewhere in that time has the end too; nothing here can
/// keep it from that.
///
/// The child doesn't outlive this process, however either of them is
/// doing (`job`): hearing that the channel is over is only enough for a
/// child that isn't stuck. Chromium likewise puts each child in a job.
pub fn spawn(command: &mut Command) -> io::Result<(Child, Channel)> {
    static STARTING: Mutex<()> = Mutex::new(());
    let (server, client) = pipe().ok_or_else(|| io::Error::other("no pipe could be made"))?;
    let value = client.as_raw_handle().expose_provenance();
    let spawned = {
        let _one_at_a_time = lock(&STARTING);
        if !inherited_by_children(&client, true) {
            return Err(io::Error::last_os_error());
        }
        command.env(CHANNEL_VARIABLE, value.to_string());
        let spawned = command.spawn();
        // The command is the caller's: a child it starts by itself
        // later is told of no end.
        command.env_remove(CHANNEL_VARIABLE);
        // This process's own client end goes here, with the lock held:
        // the child's is the only one, so the child going is the pipe
        // broken.
        drop(client);
        spawned
    };
    let mut child = spawned?;
    // It has been told nothing yet, so it isn't stuck in anything: were
    // this process to end before this, the child would hear that and go.
    if let Err(error) = job::end_with_this_process(child.as_handle()) {
        // It would be left running when this process is gone.
        let _ = child.kill();
        return Err(error);
    }
    let Some(channel) = Channel::host(server, child.as_handle()) else {
        // No way to give it handles: the child would wait on a host
        // that can't be one.
        let _ = child.kill();
        return Err(io::Error::last_os_error());
    };
    Ok((child, channel))
}

/// In a process started by [`spawn`]: its end of the channel, once.
/// `None` when the parent gave none, or it was taken already. It takes
/// the variable out of the process's environment.
pub fn inherited() -> Option<Channel> {
    static TAKEN: AtomicBool = AtomicBool::new(false);
    if TAKEN.swap(true, Ordering::SeqCst) {
        return None;
    }
    let value = std::env::var(CHANNEL_VARIABLE).ok()?;
    // A child of this process's own is told of no end: it has none.
    // SAFETY: on Windows the environment is changed by a call of the
    // system's that any thread may make while others read it, and this
    // module is built for Windows alone.
    unsafe { std::env::remove_var(CHANNEL_VARIABLE) };
    end_named(&value)
}

/// The end the variable's `value` names, if it names a pipe of this
/// process's.
fn end_named(value: &str) -> Option<Channel> {
    let value: usize = value.parse().ok()?;
    let handle: HANDLE = std::ptr::with_exposed_provenance_mut(value);
    // A variable set by anyone else names nothing, or something that
    // isn't a pipe. (Asked of what is no handle, the call says so.)
    // SAFETY: the call only looks the value up.
    if unsafe { GetFileType(handle) } != FILE_TYPE_PIPE {
        return None;
    }
    // SAFETY: the host, which is trusted, put this handle in this
    // process for this and says so once; it was just seen to be a pipe,
    // and `inherited` sees that it is owned once.
    let pipe = unsafe { OwnedHandle::from_raw_handle(handle) };
    // Inherited, it would be inherited again by this process's own
    // children.
    inherited_by_children(&pipe, false).then(|| Channel::child(pipe))
}

/// Nothing: a child gone has let go of its end, taken or not, and this
/// end's read fails for it.
pub const fn child_gone(_pid: u32) {}

/// What other threads end a waiter's wait by.
struct Interrupt {
    port: OwnedHandle,
    /// Asked for and not yet told: looked at before every wait, and one
    /// completion is posted for it however many times it was asked.
    asked: AtomicBool,
}

/// What a process's IO thread waits with: one completion port, which the
/// pipe of every end it serves is on.
///
/// When it is dropped with reads and writes still going on (they were
/// cancelled when their ends were shut), nobody takes their completions:
/// their ends' memory is never freed, which is what keeps the kernel
/// from writing memory that was.
pub struct IocpWaiter {
    interrupt: Arc<Interrupt>,
    channels: Vec<(Key, Arc<End>)>,
    /// The ends with a read over: bytes to lend, or the next read to
    /// start.
    read: VecDeque<Key>,
    /// The ends to say have room again.
    writable: VecDeque<Key>,
    /// How many times in a row something was taken up from `read`
    /// without a look at the port.
    turns: usize,
}

impl Waiter<Channel> for IocpWaiter {
    fn new() -> Option<Self> {
        // SAFETY: makes a new port, for one thread to wait on.
        let port = unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, null_mut(), 0, 1) };
        if port.is_null() {
            return None;
        }
        Some(Self {
            interrupt: Arc::new(Interrupt {
                // SAFETY: the call succeeded: a new handle, this
                // process's alone.
                port: unsafe { OwnedHandle::from_raw_handle(port) },
                asked: AtomicBool::new(false),
            }),
            channels: Vec::new(),
            read: VecDeque::new(),
            writable: VecDeque::new(),
            turns: 0,
        })
    }

    fn interrupter(&self) -> Wake {
        let interrupt = self.interrupt.clone();
        Arc::new(move || {
            if !interrupt.asked.swap(true, Ordering::SeqCst) {
                // A completion that is nobody's operation: it only ends
                // the wait.
                // SAFETY: the port is open as long as `interrupt` is.
                unsafe { PostQueuedCompletionStatus(interrupt.port.as_raw_handle(), 0, 0, null()) };
            }
        })
    }

    fn add(&mut self, key: Key, channel: &Channel) -> bool {
        let end = &channel.0;
        let Ok(port_key) = usize::try_from(key) else {
            return false;
        };
        {
            let pipe = lock(&end.pipe);
            let Some(pipe) = pipe.as_ref() else {
                return false;
            };
            // SAFETY: both handles are open for the call.
            let port = unsafe {
                CreateIoCompletionPort(
                    pipe.as_raw_handle(),
                    self.interrupt.port.as_raw_handle(),
                    port_key,
                    0,
                )
            };
            // On a port already (an end joins one thread, once), or not
            // a handle a port takes.
            if port.is_null() {
                return false;
            }
            // Before anything is read or written: what is done at once
            // is over at once, with no completion to wait for.
            // SAFETY: `pipe` is open for the call.
            let set = unsafe {
                SetFileCompletionNotificationModes(pipe.as_raw_handle(), SKIP_PORT_ON_SUCCESS)
            };
            if set == 0 {
                return false;
            }
        }
        let mut writing = lock(&end.writing);
        writing.joined = true;
        // What was refused before now can be written.
        if std::mem::take(&mut writing.blocked) {
            self.writable.push_back(key);
        }
        drop(writing);
        self.channels.push((key, end.clone()));
        // What came before now is still in the pipe: the first read
        // finds it.
        self.read.push_back(key);
        true
    }

    fn remove(&mut self, key: Key) {
        self.channels.retain(|(served, _)| *served != key);
        self.read.retain(|served| *served != key);
        self.writable.retain(|served| *served != key);
    }

    fn wait<R>(&mut self, then: impl FnOnce(Waited<'_, OwnedHandle>) -> R) -> R {
        let mut handles = Vec::new();
        let (key, event) = loop {
            if self.interrupt.asked.swap(false, Ordering::SeqCst) {
                return then(Waited::Interrupted);
            }
            if let Some(key) = self.writable.pop_front() {
                break (key, Event::Writable);
            }
            // What the ends have read already, an end at a time and
            // not for ever: the port's turn comes too.
            let waiting = !self.read.is_empty();
            if let Some(&key) = self.read.front()
                && self.turns < TURNS_BEFORE_THE_PORT
            {
                // (An end no longer served isn't in the queue.)
                let Some(end) = self.served(key) else {
                    self.read.pop_front();
                    continue;
                };
                // Shut at this end: what it read, and that it reads no
                // more, is nobody's to hear.
                if lock(&end.pipe).is_none() {
                    self.read.pop_front();
                    continue;
                }
                self.turns = self.turns.saturating_add(1);
                match end.next(&mut handles) {
                    Next::Lend(bytes, len) => {
                        // The next turn is another end's, if another
                        // has something.
                        self.read.rotate_left(1);
                        // SAFETY: `len` bytes of the end's read buffer,
                        // which `end` keeps alive. No read is going on,
                        // and only this thread starts one, after this
                        // call: nothing writes them while they are lent.
                        let bytes = unsafe { std::slice::from_raw_parts(bytes, len) };
                        return then(Waited::Channel {
                            key,
                            event: Event::Read,
                            bytes,
                            handles: &mut handles,
                        });
                    }
                    Next::More => match end.read() {
                        Started::Going => {
                            self.read.pop_front();
                        }
                        // Its bytes are lent in the next turn.
                        Started::Done => {}
                        Started::Over => break (key, Event::PeerGone),
                    },
                    Next::Bad => break (key, Event::PeerGone),
                }
                continue;
            }
            self.turns = 0;
            let mut transferred = 0;
            let mut port_key = 0;
            let mut overlapped: *mut OVERLAPPED = null_mut();
            // SAFETY: the port is open, and the three are alive for the
            // call.
            let done = unsafe {
                GetQueuedCompletionStatus(
                    self.interrupt.port.as_raw_handle(),
                    &raw mut transferred,
                    &raw mut port_key,
                    &raw mut overlapped,
                    // Only a look, with ends waiting for their turn.
                    if waiting { 0 } else { INFINITE },
                ) != 0
            };
            // An interrupt's (the flag says whether it is still to be
            // told), or nothing was there to take.
            if overlapped.is_null() {
                continue;
            }
            let operation: *mut Operation = overlapped.cast();
            // SAFETY: only the pipes of this waiter's ends are on its
            // port, and nothing posts to it but its interrupter, with no
            // `OVERLAPPED`: this is the completion of an operation of
            // one of those ends, whose address is its `OVERLAPPED`'s.
            // The two fields never change once the end is made. The
            // operation held its end by one count since it started, and
            // each has one completion: the count is taken over here.
            let (kind, end) = unsafe { ((*operation).kind, Arc::from_raw((*operation).end)) };
            let transferred = usize::try_from(transferred).unwrap_or(usize::MAX);
            let key = match kind {
                Kind::Write => {
                    let mut writing = lock(&end.writing);
                    writing.going = false;
                    let whole = done && transferred == writing.len;
                    let blocked = std::mem::take(&mut writing.blocked);
                    drop(writing);
                    let Some(key) = self.serving(port_key, &end) else {
                        continue;
                    };
                    // A pipe takes all of a write or fails it.
                    if !whole {
                        break (key, Event::PeerGone);
                    }
                    if blocked {
                        break (key, Event::Writable);
                    }
                    continue;
                }
                Kind::Read => {
                    let Some(key) = self.serving(port_key, &end) else {
                        continue;
                    };
                    key
                }
            };
            // A read that failed, or brought nothing, is the pipe's end.
            if !done || transferred == 0 {
                break (key, Event::PeerGone);
            }
            let mut reading = lock(&end.reading);
            reading.end = reading.end.saturating_add(transferred);
            drop(reading);
            self.read.push_back(key);
        };
        if event == Event::PeerGone {
            // Told once: its other operation's failing says no more.
            self.remove(key);
        }
        then(Waited::Channel {
            key,
            event,
            bytes: &[],
            handles: &mut handles,
        })
    }
}

impl IocpWaiter {
    fn served(&self, key: Key) -> Option<Arc<End>> {
        self.channels
            .iter()
            .find(|(served, _)| *served == key)
            .map(|(_, end)| end.clone())
    }

    /// The key `end` is served as, if it still is and isn't shut: what
    /// its operations end with after that is nobody's to hear.
    fn serving(&self, port_key: usize, end: &Arc<End>) -> Option<Key> {
        let key = Key::try_from(port_key).ok()?;
        let served = self.served(key)?;
        (Arc::ptr_eq(&served, end) && lock(&end.pipe).is_some()).then_some(key)
    }
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::Foundation::{ERROR_PIPE_BUSY, GetHandleInformation};
    use windows_sys::Win32::System::Threading::GetProcessHandleCount;

    use super::*;

    /// Runs a test that waits on a channel, which waits as long as it
    /// takes: a wait that never ends is the test failing, said so.
    fn within(test: impl FnOnce() + Send + 'static) {
        let (done, finished) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            test();
            let _ = done.send(());
        });
        match finished.recv_timeout(std::time::Duration::from_secs(10)) {
            Ok(()) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                panic!("stuck: a wait on the channel never ended")
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => panic!("failed (above)"),
        }
    }

    /// An end and the waiter of the thread that serves it, as key 7.
    struct Served {
        channel: Channel,
        waiter: IocpWaiter,
    }

    const KEY: Key = 7;

    impl Served {
        fn new(channel: Channel) -> Self {
            let mut waiter = IocpWaiter::new().unwrap();
            assert!(waiter.add(KEY, &channel));
            Self { channel, waiter }
        }

        /// What the next wait comes back with: `None` when it was
        /// interrupted.
        fn next(&mut self) -> Option<(Event, Vec<u8>, usize)> {
            self.waiter.wait(|waited| match waited {
                Waited::Interrupted => None,
                Waited::Channel {
                    key,
                    event,
                    bytes,
                    handles,
                } => {
                    assert_eq!(key, KEY);
                    Some((event, bytes.to_vec(), handles.len()))
                }
            })
        }

        fn write(&self, bytes: &[u8]) -> Result<usize, WriteError> {
            self.channel.write(bytes, &mut Vec::new())
        }

        /// Writes all of `bytes`, a write at a time.
        fn send(&mut self, bytes: &[u8]) {
            let mut left = bytes;
            while !left.is_empty() {
                match self.write(left) {
                    Ok(taken) => left = &left[taken..],
                    Err(WriteError::WouldBlock) => {
                        assert_eq!(self.next().unwrap().0, Event::Writable);
                    }
                    Err(WriteError::Closed) => panic!("closed"),
                }
            }
        }

        /// The next `len` bytes read, however many reads they come in.
        fn receive(&mut self, len: usize) -> Vec<u8> {
            let mut got = Vec::new();
            while got.len() < len {
                let (event, bytes, handles) = self.next().unwrap();
                assert_eq!((event, handles), (Event::Read, 0));
                got.extend(bytes);
            }
            got
        }
    }

    fn this_process() -> BorrowedHandle<'static> {
        // SAFETY: the pseudo-handle for this process, good for as long
        // as the process is.
        unsafe { BorrowedHandle::borrow_raw(GetCurrentProcess()) }
    }

    /// Both ends in this process, not yet served: the host's child is
    /// this process.
    fn channels() -> (Channel, Channel) {
        let (server, client) = pipe().unwrap();
        (
            Channel::host(server, this_process()).unwrap(),
            Channel::child(client),
        )
    }

    fn ends() -> (Served, Served) {
        let (host, child) = channels();
        (Served::new(host), Served::new(child))
    }

    /// An end that reads as a child does, and its peer, which writes
    /// what the test gives it as it is: a host of the test's making.
    fn by_hand() -> (Served, Served) {
        let (server, client) = pipe().unwrap();
        (
            Served::new(Channel::child(server)),
            Served::new(Channel::child(client)),
        )
    }

    /// What the next wait comes back with when nothing is to come: the
    /// wait is interrupted a while after it began, having taken off the
    /// port what was there.
    fn later(served: &mut Served) -> Option<(Event, Vec<u8>, usize)> {
        let interrupt = served.waiter.interrupter();
        let from_elsewhere = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            interrupt();
        });
        let told = served.next();
        from_elsewhere.join().unwrap();
        told
    }

    /// What a waiter of several ends says next.
    fn told(waiter: &mut IocpWaiter) -> Option<(Key, Event, Vec<u8>)> {
        waiter.wait(|waited| match waited {
            Waited::Interrupted => None,
            Waited::Channel {
                key, event, bytes, ..
            } => Some((key, event, bytes.to_vec())),
        })
    }

    /// As `later`, for a waiter of several ends.
    fn told_later(waiter: &mut IocpWaiter) -> Option<(Key, Event, Vec<u8>)> {
        let interrupt = waiter.interrupter();
        let from_elsewhere = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            interrupt();
        });
        let told = told(waiter);
        from_elsewhere.join().unwrap();
        told
    }

    fn read(bytes: &[u8], handles: usize) -> Option<(Event, Vec<u8>, usize)> {
        Some((Event::Read, bytes.to_vec(), handles))
    }

    fn pattern(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i as u8).wrapping_mul(3)).collect()
    }

    /// A host's record, as it is in the pipe.
    fn record(len: u32, values: &[u64], bytes: &[u8]) -> Vec<u8> {
        let mut record = len.to_le_bytes().to_vec();
        record.extend((values.len() as u32).to_le_bytes());
        for value in values {
            record.extend(value.to_le_bytes());
        }
        record.extend(bytes);
        record
    }

    /// How many handles this process has open.
    fn handle_count() -> u32 {
        let mut count = 0;
        // SAFETY: `count` is alive for the call.
        assert!(unsafe { GetProcessHandleCount(GetCurrentProcess(), &raw mut count) } != 0);
        count
    }

    #[test]
    fn what_one_end_writes_the_other_is_lent() {
        within(|| {
            let (mut host, mut child) = ends();
            // Each fits one write, a host's header and all.
            for len in [1usize, 2, 4096, RECORD_MAX - HEADER_LEN] {
                let bytes = pattern(len);
                assert_eq!(host.write(&bytes), Ok(len));
                assert!(child.next() == read(&bytes, 0), "{len} bytes to the child");
                host.send(&bytes);
                assert!(child.receive(len) == bytes, "{len} bytes again");
                child.send(&bytes);
                assert!(host.receive(len) == bytes, "{len} bytes to the host");
            }
        });
    }

    #[test]
    fn a_long_message_goes_a_write_at_a_time_and_comes_in_order() {
        within(|| {
            // A write takes what one write holds and no more: less for a
            // host, by its header.
            let (host, child) = ends();
            let long = pattern(1 << 20);
            assert_eq!(host.write(&long), Ok(RECORD_MAX - HEADER_LEN));
            assert_eq!(child.write(&long), Ok(RECORD_MAX));
        });
        within(|| {
            let (mut host, mut child) = ends();
            for len in [RECORD_MAX, RECORD_MAX + 1, 1 << 20] {
                let bytes = pattern(len);
                let got = std::thread::scope(|scope| {
                    let reader = scope.spawn(|| child.receive(len));
                    host.send(&bytes);
                    reader.join().unwrap()
                });
                assert!(got == bytes, "{len} bytes to the child");
                let got = std::thread::scope(|scope| {
                    let reader = scope.spawn(|| host.receive(len));
                    child.send(&bytes);
                    reader.join().unwrap()
                });
                assert!(got == bytes, "{len} bytes to the host");
            }
        });
    }

    #[test]
    fn writes_go_one_after_another_while_the_pipe_has_room() {
        within(|| {
            let (mut host, mut child) = ends();
            // Each is over when its call is back: none waits for its
            // end's thread to take anything off the port.
            let mut to_child = Vec::new();
            let mut to_host = Vec::new();
            for i in 0..20u8 {
                assert_eq!(host.write(&[i; 100]), Ok(100));
                assert_eq!(child.write(&[i; 50]), Ok(50));
                to_child.extend([i; 100]);
                to_host.extend([i; 50]);
            }
            assert!(child.receive(2000) == to_child);
            assert!(host.receive(1000) == to_host);
            // And they left nothing at the port: what is told next is
            // what comes next.
            assert_eq!(child.write(b"after"), Ok(5));
            assert_eq!(host.next(), read(b"after", 0));
            assert_eq!(host.write(b"and back"), Ok(8));
            assert_eq!(child.next(), read(b"and back", 0));
        });
    }

    #[test]
    fn one_write_goes_on_at_a_time_and_room_is_told_to_who_was_refused() {
        within(|| {
            let (mut host, mut child) = ends();
            let bytes = pattern(60_000);
            // The pipe takes the first at once.
            assert_eq!(host.write(&bytes), Ok(60_000));
            // It has no room for the second, which goes on until the
            // child reads.
            assert_eq!(host.write(&bytes), Ok(60_000));
            assert_eq!(host.write(b"third"), Err(WriteError::WouldBlock));
            assert!(child.receive(60_000) == bytes);
            assert_eq!(host.next().unwrap().0, Event::Writable);
            assert_eq!(host.write(b"third"), Ok(5));
            assert!(child.receive(60_000) == bytes);
            assert_eq!(child.next(), read(b"third", 0));
        });
    }

    #[test]
    fn a_write_nobody_was_refused_after_tells_of_no_room() {
        within(|| {
            let (mut host, mut child) = ends();
            assert_eq!(host.write(b"one"), Ok(3));
            assert_eq!(child.next(), read(b"one", 0));
            assert_eq!(child.write(b"back"), Ok(4));
            // The write's completion is taken and said nothing of.
            assert_eq!(host.next(), read(b"back", 0));
            assert_eq!(host.write(b"two"), Ok(3));
            assert_eq!(child.next(), read(b"two", 0));
        });
    }

    #[test]
    fn an_end_writes_nothing_before_a_thread_serves_it() {
        within(|| {
            let (host, child) = channels();
            assert_eq!(
                host.write(b"early", &mut Vec::new()),
                Err(WriteError::WouldBlock)
            );
            let mut child = Served::new(child);
            // Refused, it is told of room once it is served; then the
            // write goes.
            let mut host = Served::new(host);
            assert_eq!(host.next().unwrap().0, Event::Writable);
            assert_eq!(host.write(b"early"), Ok(5));
            assert_eq!(child.next(), read(b"early", 0));
            // An end nothing was refused of is told nothing on joining.
            assert_eq!(child.write(b"back"), Ok(4));
            assert_eq!(host.next(), read(b"back", 0));
        });
    }

    #[test]
    fn nothing_is_written_of_no_bytes() {
        within(|| {
            let (mut host, mut child) = ends();
            assert_eq!(host.write(b""), Ok(0));
            assert_eq!(child.write(b""), Ok(0));
            // No write goes on for it.
            assert_eq!(host.write(b"something"), Ok(9));
            assert_eq!(child.next(), read(b"something", 0));
            assert_eq!(child.write(b"else"), Ok(4));
            assert_eq!(host.next(), read(b"else", 0));
        });
    }

    #[test]
    fn handles_go_with_their_message_and_leave_the_writers_hands() {
        within(|| {
            let (host, mut child) = ends();
            let (mapping, handle) = memory::create(RECORD_MAX, false).unwrap();
            let (_other, second) = memory::create(RECORD_MAX, true).unwrap();
            let mut handles = vec![handle, second];
            assert_eq!(host.channel.write(b"with regions", &mut handles), Ok(12));
            assert!(handles.is_empty());
            let Served { channel, waiter } = &mut child;
            let (mapped, writable) = waiter.wait(|waited| {
                let Waited::Channel {
                    event,
                    bytes,
                    handles,
                    ..
                } = waited
                else {
                    panic!("an interrupt");
                };
                assert_eq!(
                    (event, bytes, handles.len()),
                    (Event::Read, &b"with regions"[..], 2)
                );
                // In the order they were given, each with the rights it
                // had.
                let second = handles.pop().unwrap();
                let first = handles.pop().unwrap();
                for handle in [&first, &second] {
                    let mut flags = 0;
                    // SAFETY: `handle` is open and `flags` alive for
                    // the call.
                    let asked = unsafe {
                        GetHandleInformation(handle.as_raw_handle(), &raw mut flags) != 0
                    };
                    assert!(asked);
                    assert_eq!(flags & HANDLE_FLAG_INHERIT, 0, "a child would inherit it");
                }
                assert!(memory::map(&first, RECORD_MAX, true).is_none());
                (
                    channel.map(first, RECORD_MAX, false),
                    channel.map(second, RECORD_MAX, true).is_some(),
                )
            });
            assert!(writable);
            // The region the handle is: what the maker writes is read
            // there.
            // SAFETY: inside both mappings; nothing else touches them.
            unsafe {
                crate::core::region::Memory::ptr(&mapping).write(0x42);
                assert_eq!(
                    crate::core::region::Memory::ptr(&mapped.unwrap()).read(),
                    0x42
                );
            }
        });
    }

    #[test]
    fn a_message_that_goes_leaves_no_handle_behind() {
        // Held by the test's own thread: one that is stuck doesn't keep
        // the others from counting.
        let _alone = super::super::counting_handles();
        within(|| {
            let (mut host, mut child) = ends();
            let before = handle_count();
            for _ in 0..2000 {
                let (_mapping, handle) = memory::create(4096, false).unwrap();
                let mut handles = vec![handle];
                loop {
                    match host.channel.write(b"a region", &mut handles) {
                        Ok(_) => break,
                        Err(WriteError::WouldBlock) => {
                            assert_eq!(host.next().unwrap().0, Event::Writable);
                        }
                        Err(WriteError::Closed) => panic!("closed"),
                    }
                }
                // The child's own is let go with the wait.
                assert_eq!(child.next(), read(b"a region", 1));
            }
            let more = handle_count().saturating_sub(before);
            assert!(more < 500, "{more} handles more after 2000 messages");
        });
    }

    #[test]
    fn a_message_that_doesnt_go_leaves_no_handle_in_the_child() {
        // Held by the test's own thread: one that is stuck doesn't keep
        // the others from counting.
        let _alone = super::super::counting_handles();
        within(|| {
            let (host, child) = ends();
            drop(child);
            let before = handle_count();
            // The child's process is this one: a handle left there is
            // one more here.
            let mut kept = Vec::new();
            for _ in 0..2000 {
                let (_mapping, handle) = memory::create(4096, false).unwrap();
                let mut handles = vec![handle];
                assert_eq!(
                    host.channel.write(b"to nobody", &mut handles),
                    Err(WriteError::Closed)
                );
                // Still the writer's.
                assert_eq!(handles.len(), 1);
                kept.extend(handles);
            }
            let more = handle_count().saturating_sub(before);
            // One each, the writer's own: none more in the child. (Other
            // tests open and close a few meanwhile.)
            assert!(
                (1500..3000).contains(&more),
                "{more} handles more for 2000 kept"
            );
        });
    }

    #[test]
    fn a_child_gives_no_handle_and_a_host_takes_no_value_for_one() {
        within(|| {
            let (mut host, mut child) = ends();
            // A child handed one lets go of it: there is nowhere to put
            // it.
            let (_mapping, handle) = memory::create(RECORD_MAX, false).unwrap();
            let mut handles = vec![handle];
            assert_eq!(child.channel.write(b"mine", &mut handles), Ok(4));
            assert!(handles.is_empty());
            assert_eq!(host.next(), read(b"mine", 0));
            // What a child writes that looks like a host's header, with
            // a value that is a handle of the host's own, is bytes.
            let own = host
                .channel
                .0
                .pipe
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .as_raw_handle();
            let forged = record(5, &[own.expose_provenance() as u64], b"bytes");
            child.send(&forged);
            assert_eq!(host.next(), read(&forged, 0));
            // And the handle it named is still the host's.
            assert_eq!(host.write(b"still open"), Ok(10));
            assert_eq!(child.next(), read(b"still open", 0));
        });
    }

    #[test]
    fn each_of_a_hosts_writes_is_lent_once_in_order() {
        within(|| {
            let (mut host, mut reader) = by_hand();
            // Three in one read.
            let mut three = record(1, &[], b"a");
            three.extend(record(2, &[], b"bc"));
            three.extend(record(3, &[], b"def"));
            host.send(&three);
            assert_eq!(reader.next(), read(b"a", 0));
            assert_eq!(reader.next(), read(b"bc", 0));
            assert_eq!(reader.next(), read(b"def", 0));
            // The longest there is, twice: neither read has room for
            // both.
            let longest = pattern(RECORD_MAX - HEADER_LEN);
            let whole = record((RECORD_MAX - HEADER_LEN) as u32, &[], &longest);
            for _ in 0..2 {
                let got = std::thread::scope(|scope| {
                    let reading = scope.spawn(|| reader.next());
                    host.send(&whole);
                    reading.join().unwrap()
                });
                assert!(got == read(&longest, 0));
            }
        });
    }

    #[test]
    fn a_record_that_comes_in_pieces_is_lent_whole() {
        within(|| {
            let (mut host, mut reader) = by_hand();
            let whole = record(6, &[], b"pieces");
            let got = std::thread::scope(|scope| {
                let reading = scope.spawn(|| reader.next());
                // A byte at a time, the reader reading between.
                for byte in &whole {
                    host.send(std::slice::from_ref(byte));
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                reading.join().unwrap()
            });
            assert_eq!(got, read(b"pieces", 0));
            // And what follows a record in the same read isn't lost
            // while the rest of it is waited for.
            let mut two = record(1, &[], b"x");
            two.extend(&whole[..5]);
            host.send(&two);
            assert_eq!(reader.next(), read(b"x", 0));
            let got = std::thread::scope(|scope| {
                let reading = scope.spawn(|| reader.next());
                std::thread::sleep(std::time::Duration::from_millis(50));
                host.send(&whole[5..]);
                reading.join().unwrap()
            });
            assert_eq!(got, read(b"pieces", 0));
        });
    }

    #[test]
    fn a_header_no_host_writes_is_the_peer_gone() {
        let too_long = (RECORD_MAX - HEADER_LEN + 1) as u32;
        let cases: [(&str, Vec<u8>); 6] = [
            ("no bytes", record(0, &[], b"")),
            ("more than a write", record(too_long, &[], b"")),
            ("more with its values", {
                let mut it = (RECORD_MAX as u32 - 16).to_le_bytes().to_vec();
                it.extend(2u32.to_le_bytes());
                it
            }),
            ("too many handles", {
                let mut it = 1u32.to_le_bytes().to_vec();
                it.extend(17u32.to_le_bytes());
                it
            }),
            ("a value of nothing", record(1, &[0], b"x")),
            ("a value of no handle", record(1, &[u64::MAX], b"x")),
        ];
        for (what, bytes) in cases {
            within(move || {
                let (mut host, mut reader) = by_hand();
                // What came before it is lent.
                let mut all = record(4, &[], b"good");
                all.extend(bytes);
                host.send(&all);
                assert_eq!(reader.next(), read(b"good", 0), "{what}");
                assert_eq!(reader.next().unwrap().0, Event::PeerGone, "{what}");
            });
        }
        // And the most a header may say is taken.
        within(|| {
            let (mut host, mut reader) = by_hand();
            let most = pattern(RECORD_MAX - HEADER_LEN);
            let whole = record((RECORD_MAX - HEADER_LEN) as u32, &[], &most);
            let got = std::thread::scope(|scope| {
                let reading = scope.spawn(|| reader.next());
                host.send(&whole);
                reading.join().unwrap()
            });
            assert!(got == read(&most, 0));
        });
    }

    #[test]
    fn a_wait_is_interrupted_once_however_many_times_it_was_asked() {
        within(|| {
            let (mut host, mut child) = ends();
            let interrupt = host.waiter.interrupter();
            for _ in 0..20 {
                interrupt();
            }
            assert_eq!(host.next(), None);
            // The next thing told is what comes next.
            assert_eq!(child.write(b"after"), Ok(5));
            assert_eq!(host.next(), read(b"after", 0));
            interrupt();
            assert_eq!(host.next(), None);
            // From another thread, with the wait under way.
            let from_elsewhere = std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(50));
                interrupt();
            });
            assert_eq!(host.next(), None);
            from_elsewhere.join().unwrap();
            // One completion was posted for the twenty: nothing is left
            // to end a later wait early.
            child.send(b"later");
            assert_eq!(host.next(), read(b"later", 0));
            assert_eq!(host.write(b"and back"), Ok(8));
            assert_eq!(child.next(), read(b"and back", 0));
        });
    }

    #[test]
    fn one_waiter_tells_its_channels_apart() {
        within(|| {
            let mut waiter = IocpWaiter::new().unwrap();
            let mut peers = Vec::new();
            for key in [3, 9, 4] {
                let (host, child) = channels();
                assert!(waiter.add(key, &host));
                peers.push((key, host, Served::new(child)));
            }
            let next = |waiter: &mut IocpWaiter| {
                waiter.wait(|waited| match waited {
                    Waited::Interrupted => None,
                    Waited::Channel {
                        key, event, bytes, ..
                    } => Some((key, event, bytes.to_vec())),
                })
            };
            // Written before it was waited on, and after.
            let (late_host, mut late) = {
                let (host, child) = channels();
                let child = Served::new(child);
                assert_eq!(child.write(b"early"), Ok(5));
                assert!(waiter.add(1, &host));
                (host, child)
            };
            assert_eq!(next(&mut waiter), Some((1, Event::Read, b"early".to_vec())));
            for (key, _, child) in &peers {
                assert_eq!(child.write(&[*key as u8]), Ok(1));
                assert_eq!(
                    next(&mut waiter),
                    Some((*key, Event::Read, vec![*key as u8]))
                );
            }
            // Each is written by its own end.
            for (key, host, child) in &mut peers {
                assert_eq!(host.write(&[*key as u8, 1], &mut Vec::new()), Ok(2));
                assert_eq!(child.next(), read(&[*key as u8, 1], 0));
            }
            // One peer gone is that channel's alone, told once.
            let (key, host, child) = peers.remove(1);
            drop(child);
            assert_eq!(next(&mut waiter), Some((key, Event::PeerGone, vec![])));
            // No longer served, what its operations end with is nothing.
            waiter.remove(key);
            drop(host);
            late.send(b"late");
            assert_eq!(next(&mut waiter), Some((1, Event::Read, b"late".to_vec())));
            // An end shut can't join, nor one that has.
            let (host, _child) = channels();
            host.shut();
            assert!(!waiter.add(2, &host));
            assert!(!waiter.add(5, &late_host));
            assert!(!IocpWaiter::new().unwrap().add(5, &late_host));
        });
    }

    #[test]
    fn an_end_removed_is_told_of_no_more() {
        within(|| {
            let mut waiter = IocpWaiter::new().unwrap();
            let (gone_host, gone_child) = channels();
            let (host, child) = channels();
            assert!(waiter.add(1, &gone_host));
            assert!(waiter.add(2, &host));
            let gone_child = Served::new(gone_child);
            let child = Served::new(child);
            // Refused of a write while one goes on, and with bytes to
            // read: both would be told.
            let bytes = pattern(60_000);
            assert_eq!(gone_host.write(&bytes, &mut Vec::new()), Ok(60_000));
            assert_eq!(gone_host.write(&bytes, &mut Vec::new()), Ok(60_000));
            assert_eq!(
                gone_host.write(b"b", &mut Vec::new()),
                Err(WriteError::WouldBlock)
            );
            assert_eq!(gone_child.write(b"unheard"), Ok(7));
            waiter.remove(1);
            // The write that went on is over once its peer has read: its
            // completion is at the port, for nobody.
            let mut gone_child = gone_child;
            assert!(gone_child.receive(60_000) == bytes);
            // And one refused before it joined, which is told of room
            // on joining: not once it has left.
            let (refused, _its_child) = channels();
            assert_eq!(
                refused.write(b"early", &mut Vec::new()),
                Err(WriteError::WouldBlock)
            );
            assert!(waiter.add(3, &refused));
            waiter.remove(3);
            assert_eq!(child.write(b"heard"), Ok(5));
            let told = waiter.wait(|waited| match waited {
                Waited::Interrupted => None,
                Waited::Channel {
                    key, event, bytes, ..
                } => Some((key, event, bytes.to_vec())),
            });
            assert_eq!(told, Some((2, Event::Read, b"heard".to_vec())));
        });
    }

    #[test]
    fn an_end_shut_sends_nothing_and_its_peer_is_told() {
        within(|| {
            let (mut host, mut child) = ends();
            // With a read of its own going on when it shuts.
            assert_eq!(child.write(b"first"), Ok(5));
            assert_eq!(host.next(), read(b"first", 0));
            assert_eq!(later(&mut host), None);
            assert_eq!(host.write(b"before"), Ok(6));
            host.channel.shut();
            assert_eq!(host.write(b"after"), Err(WriteError::Closed));
            // Its handles go at once, whatever holds the end yet: the
            // pipe, and its child's process.
            assert!(lock(&host.channel.0.pipe).is_none());
            assert!(lock(&host.channel.0.peer).is_none());
            // What was sent before still comes; then the pipe's end.
            assert_eq!(child.next(), read(b"before", 0));
            assert_eq!(child.next().unwrap().0, Event::PeerGone);
            // Told once: what its operations end with after is nothing,
            // and neither is anything of the end that shut itself.
            let _ = child.write(b"to nobody");
            assert_eq!(later(&mut child), None);
            // Its own read was cancelled: taken off the port, it says
            // nothing.
            assert_eq!(later(&mut host), None);
        });
    }

    #[test]
    fn an_end_shut_before_its_first_wait_is_told_nothing() {
        within(|| {
            // The read joining set down for it is never started.
            let (mut host, mut child) = ends();
            host.channel.shut();
            assert_eq!(later(&mut host), None);
            assert_eq!(child.next().unwrap().0, Event::PeerGone);
        });
    }

    #[test]
    fn an_end_dropped_is_an_end_gone() {
        within(|| {
            let (mut host, child) = ends();
            // The value alone: its waiter still has the end.
            let Served { channel, waiter } = child;
            drop(channel);
            assert_eq!(host.next().unwrap().0, Event::PeerGone);
            drop(waiter);
            assert_eq!(host.write(b"to nobody"), Err(WriteError::Closed));
        });
        // With a write going on as well as the read, both fail: told
        // once all the same.
        within(|| {
            let (mut host, child) = ends();
            let bytes = pattern(60_000);
            assert_eq!(host.write(&bytes), Ok(60_000));
            assert_eq!(host.write(&bytes), Ok(60_000));
            assert_eq!(later(&mut host), None);
            drop(child);
            assert_eq!(host.next().unwrap().0, Event::PeerGone);
            assert_eq!(later(&mut host), None);
        });
    }

    #[test]
    fn a_host_holds_its_child_only_to_give_it_handles() {
        use windows_sys::Win32::System::Threading::GetExitCodeProcess;

        let (host, _child) = channels();
        let peer = lock(&host.0.peer);
        let mut code = 0;
        // SAFETY: the handle is open and `code` alive for the call.
        let asked =
            unsafe { GetExitCodeProcess(peer.as_ref().unwrap().as_raw_handle(), &raw mut code) };
        assert_eq!(
            asked, 0,
            "the handle has the right to ask about the process"
        );
    }

    #[test]
    fn a_write_outlives_the_thread_that_started_it() {
        within(|| {
            let (host, mut child) = ends();
            let bytes = pattern(60_000);
            assert_eq!(host.write(&bytes), Ok(60_000));
            let Served { channel, .. } = &host;
            // The pipe is full: this one goes on in the kernel after
            // its thread is over.
            std::thread::scope(|scope| {
                scope
                    .spawn(|| assert_eq!(channel.write(&bytes, &mut Vec::new()), Ok(60_000)))
                    .join()
                    .unwrap();
            });
            assert!(child.receive(60_000) == bytes);
            assert!(child.receive(60_000) == bytes);
        });
    }

    #[test]
    fn channels_let_go_leave_no_handle() {
        // Held by the test's own thread: one that is stuck doesn't keep
        // the others from counting.
        let _alone = super::super::counting_handles();
        within(|| {
            let before = handle_count();
            // Served by a waiter that outlives them, and by ones that
            // don't: with a read going on each time.
            let mut waiter = IocpWaiter::new().unwrap();
            for key in 0..300 {
                let (host, child) = channels();
                assert!(waiter.add(key, &host));
                let mut child = Served::new(child);
                assert_eq!(host.write(b"once", &mut Vec::new()), Ok(4));
                assert_eq!(child.next(), read(b"once", 0));
                // Shut by the value's drop, with no thread to hear of
                // what was cancelled.
                waiter.remove(key);
            }
            let more = handle_count().saturating_sub(before);
            assert!(more < 100, "{more} handles more after 300 channels");
            // And kept, they are counted: the measure sees what it is
            // meant to (two ends, a process and a port each).
            let kept: Vec<_> = (0..300).map(|_| ends()).collect();
            let more = handle_count().saturating_sub(before);
            assert!(more >= 900, "300 channels kept are only {more} handles");
            drop(kept);
        });
    }

    #[test]
    fn what_goes_on_holds_its_end_until_its_completion_is_taken() {
        // A read.
        within(|| {
            let (host, _child) = channels();
            let mut waiter = IocpWaiter::new().unwrap();
            assert!(waiter.add(1, &host));
            let end = Arc::downgrade(&host.0);
            // Its first read goes on: nothing is there to read.
            assert_eq!(told_later(&mut waiter), None);
            // Let go of by its value and by the waiter's list, the end
            // is there yet.
            waiter.remove(1);
            drop(host);
            assert!(end.upgrade().is_some());
            // Cancelled when it was shut, the read's completion is taken
            // off the port by the next wait, and the end goes.
            assert_eq!(told_later(&mut waiter), None);
            assert!(end.upgrade().is_none());
        });
        // A write.
        within(|| {
            let (host, _child) = channels();
            let mut waiter = IocpWaiter::new().unwrap();
            assert!(waiter.add(1, &host));
            let end = Arc::downgrade(&host.0);
            let bytes = pattern(60_000);
            // The second finds the pipe full. (No read was started: the
            // waiter hasn't waited.)
            assert_eq!(host.write(&bytes, &mut Vec::new()), Ok(60_000));
            assert_eq!(host.write(&bytes, &mut Vec::new()), Ok(60_000));
            waiter.remove(1);
            drop(host);
            assert!(end.upgrade().is_some());
            assert_eq!(told_later(&mut waiter), None);
            assert!(end.upgrade().is_none());
        });
    }

    #[test]
    fn what_is_done_at_once_holds_nothing() {
        within(|| {
            let (host, child) = channels();
            let mut waiter = IocpWaiter::new().unwrap();
            assert!(waiter.add(1, &host));
            let mut child = Served::new(child);
            let end = Arc::downgrade(&host.0);
            // Written at once, and read at once: the bytes were there.
            assert_eq!(host.write(b"there", &mut Vec::new()), Ok(5));
            assert_eq!(child.next(), read(b"there", 0));
            assert_eq!(child.write(b"back"), Ok(4));
            assert_eq!(told(&mut waiter), Some((1, Event::Read, b"back".to_vec())));
            // Nothing of the end's goes on: it goes with its value.
            waiter.remove(1);
            drop(host);
            assert!(end.upgrade().is_none());
        });
    }

    #[test]
    fn an_end_with_much_to_lend_doesnt_keep_the_others_waiting() {
        within(|| {
            let mut waiter = IocpWaiter::new().unwrap();
            // One whose peer writes records by hand, and one more.
            let (server, client) = pipe().unwrap();
            let mut busy_peer = Served::new(Channel::child(server));
            let busy = Channel::child(client);
            let (quiet, quiet_peer) = channels();
            assert!(waiter.add(1, &busy));
            assert!(waiter.add(2, &quiet));
            let quiet_peer = Served::new(quiet_peer);
            // Both reads go on: nothing is there yet.
            assert_eq!(told_later(&mut waiter), None);
            // A thousand records in one read for the first; then a
            // message for the second, whose read's completion waits at
            // the port behind it.
            let mut many = Vec::new();
            for i in 0..1000u32 {
                many.extend(record(4, &[], &i.to_le_bytes()));
            }
            busy_peer.send(&many);
            assert_eq!(quiet_peer.write(b"quiet"), Ok(5));
            let mut lent = Vec::new();
            let before = loop {
                match told(&mut waiter) {
                    Some((1, Event::Read, bytes)) => lent.push(bytes),
                    Some((2, Event::Read, bytes)) => {
                        assert_eq!(bytes, b"quiet");
                        break lent.len();
                    }
                    other => panic!("{other:?}"),
                }
            };
            // The port is looked at after so many turns, and the turn
            // after that is the other end's.
            assert!(
                before <= TURNS_BEFORE_THE_PORT + 1,
                "the second end waited for {before} of the first's records"
            );
            // And the first's all come, in order.
            while lent.len() < 1000 {
                match told(&mut waiter) {
                    Some((1, Event::Read, bytes)) => lent.push(bytes),
                    other => panic!("{other:?}"),
                }
            }
            for (i, bytes) in lent.iter().enumerate() {
                assert_eq!(bytes, &(i as u32).to_le_bytes());
            }
        });
    }

    #[test]
    fn a_variable_names_an_end_only_when_it_names_a_pipe() {
        let file = std::fs::File::open(std::env::current_exe().unwrap()).unwrap();
        let a_file = file.as_raw_handle().expose_provenance().to_string();
        for value in ["", "0", "nothing", "-1", "4294967292", &a_file] {
            assert!(end_named(value).is_none(), "{value:?}");
        }
        // Not taken for an end, the file is still open.
        assert!(file.metadata().is_ok());
    }

    #[test]
    fn an_end_is_taken_once_and_no_word_of_it_is_left() {
        use std::os::windows::io::IntoRawHandle;

        // (The one test that takes this process's end.)
        let inheritable = |handle: HANDLE| {
            let mut flags = 0;
            // SAFETY: `handle` is open and `flags` alive for the call.
            assert!(unsafe { GetHandleInformation(handle, &raw mut flags) } != 0);
            flags & HANDLE_FLAG_INHERIT != 0
        };
        let (_server, client) = pipe().unwrap();
        // As a child finds it: inherited, and so to be inherited again.
        assert!(inherited_by_children(&client, true));
        let handle = client.into_raw_handle();
        assert!(inheritable(handle));
        let value = handle.expose_provenance().to_string();
        // SAFETY: no other test reads or writes this variable.
        unsafe { std::env::set_var(CHANNEL_VARIABLE, &value) };
        let end = inherited().unwrap();
        assert!(!inheritable(handle), "its own children would inherit it");
        assert!(std::env::var_os(CHANNEL_VARIABLE).is_none());
        // Told of it again, it is taken no second time.
        // SAFETY: as above.
        unsafe { std::env::set_var(CHANNEL_VARIABLE, &value) };
        assert!(inherited().is_none());
        // SAFETY: as above.
        unsafe { std::env::remove_var(CHANNEL_VARIABLE) };
        drop(end);
    }

    #[test]
    fn a_pipes_ends_are_these_two_alone() {
        let (server, client, name) = named_pipe().unwrap();
        for handle in [&server, &client] {
            let mut flags = 0;
            // SAFETY: `handle` is open and `flags` alive for the call.
            assert!(unsafe { GetHandleInformation(handle.as_raw_handle(), &raw mut flags) } != 0);
            assert_eq!(flags & HANDLE_FLAG_INHERIT, 0, "a child would inherit it");
        }
        // SAFETY: `name` ends in a zero and outlives the call.
        let second = unsafe {
            CreateFileW(
                name.as_ptr(),
                FILE_GENERIC_READ,
                0,
                null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                null_mut(),
            )
        };
        assert_eq!(second, INVALID_HANDLE_VALUE);
        assert_eq!(last_error(), ERROR_PIPE_BUSY);
        // Each pipe has a name of its own.
        let (_server, _client, other) = named_pipe().unwrap();
        assert_ne!(name, other);
    }
}
