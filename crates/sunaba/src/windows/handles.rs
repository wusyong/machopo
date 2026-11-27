//! The handles a process has: those [`super::lower`] closes of its own
//! (Chromium's `HandleCloserAgent`), and what whoever started a process
//! sees of them ([`handles`]).

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt as _;
use std::os::windows::io::{
    AsHandle as _, AsRawHandle as _, BorrowedHandle, IntoRawHandle as _, OwnedHandle,
};
use std::ptr::{null, null_mut};

use windows_sys::Wdk::Foundation::{NtQueryObject, ObjectTypeInformation};
use windows_sys::Wdk::System::Threading::{NtQueryInformationProcess, ProcessHandleInformation};
use windows_sys::Win32::Foundation::{
    CloseHandle, DUPLICATE_SAME_ACCESS, DuplicateHandle, HANDLE, HANDLE_FLAG_PROTECT_FROM_CLOSE,
    STATUS_INFO_LENGTH_MISMATCH, SetHandleInformation,
};
use windows_sys::Win32::Storage::FileSystem::{FILE_TYPE_PIPE, GetFileType};
use windows_sys::Win32::System::Threading::{
    CreateEventW, GetCurrentProcessorNumber, GetCurrentThread, OpenProcess, PROCESS_DUP_HANDLE,
    PROCESS_QUERY_INFORMATION, SetThreadAffinityMask,
};

use super::{Error, check, owned, this_process};

/// The device the system's random numbers are seeded from: the first
/// random number a process asks for opens it (`cryptbase.dll`, for
/// `RtlGenRandom`), and nothing reads it again; the numbers after come
/// from the process's own generator.
const KERNEL_SECURITY_DEVICE: &str = r"\Device\KsecDD";

/// `ObjectNameInformation`, which `windows-sys` doesn't name.
const OBJECT_NAME_INFORMATION: i32 = 1;

/// `PROCESS_HANDLE_SNAPSHOT_INFORMATION` (`ntpsapi.h`, not in the SDK's
/// headers), in words: a count and a word of padding, then that many
/// `PROCESS_HANDLE_TABLE_ENTRY_INFO`s of five words each (the handle's
/// value, its object's count of handles and of pointers, then four
/// 32-bit numbers: its access, its type's index, its attributes and
/// padding).
const SNAPSHOT_HEADER: usize = 2;
const ENTRY_WORDS: usize = 5;
/// More words than a process's handles take: a snapshot asking for more
/// is refused.
const SNAPSHOT_MOST: usize = 1 << 24;

/// A handle of a process's, by its value, and how many handles its
/// object has.
#[derive(Clone, Copy)]
struct Entry {
    value: usize,
    count: usize,
}

/// `process`'s handles, as the system lists them; `None` when it lists
/// none.
fn snapshot(process: BorrowedHandle<'_>) -> Option<Vec<Entry>> {
    let mut words = vec![0_usize; 1 << 12];
    loop {
        let bytes = u32::try_from(words.len().checked_mul(size_of::<usize>())?).ok()?;
        let mut needed = 0_u32;
        // SAFETY: `process` is open; the buffer is this function's,
        // writable for the length passed, and aligned for the words the
        // class writes; `needed` is a local.
        let status = unsafe {
            NtQueryInformationProcess(
                process.as_raw_handle(),
                ProcessHandleInformation,
                words.as_mut_ptr().cast(),
                bytes,
                &raw mut needed,
            )
        };
        if status == STATUS_INFO_LENGTH_MISMATCH {
            // Handles made between the two calls take more: room for a
            // few more than were asked for.
            let asked = usize::try_from(needed).ok()?.div_ceil(size_of::<usize>());
            let more = asked.max(words.len()).checked_mul(2)?;
            if more > SNAPSHOT_MOST {
                return None;
            }
            words.resize(more, 0);
            continue;
        }
        if status != 0 {
            return None;
        }
        break;
    }
    let count = *words.first()?;
    let (entries, _) = words.get(SNAPSHOT_HEADER..)?.as_chunks::<ENTRY_WORDS>();
    if entries.len() < count {
        return None;
    }
    Some(
        entries
            .iter()
            .take(count)
            .map(|entry| Entry {
                value: entry[0],
                count: entry[1],
            })
            .collect(),
    )
}

/// Asks objects, through their handles, of the two things answered as
/// a `UNICODE_STRING` and its text (a type's name, an object's), in one
/// buffer kept for every answer.
struct Asker {
    /// As much as the longest answer takes (a `UNICODE_STRING` is at
    /// most 65,535 bytes), aligned for the string's pointer.
    words: Vec<u64>,
}

impl Asker {
    fn new() -> Self {
        Self {
            words: vec![0; (1 << 16) / 8 + 4],
        }
    }

    /// The kind of object `handle` is (`File`, `Event`, `Key`…).
    fn kind(&mut self, handle: BorrowedHandle<'_>) -> Option<OsString> {
        self.ask(handle, ObjectTypeInformation)
    }

    /// The object's name: a file's is its device's (`\Device\…`), and
    /// one with none has an empty one. `None` for a pipe, which isn't
    /// asked: the question waits for any read of the pipe that is
    /// waiting, by this process or another that has it.
    fn name(&mut self, handle: BorrowedHandle<'_>, kind: &OsString) -> Option<OsString> {
        // SAFETY: `handle` is open; a handle that isn't a file's is an
        // error.
        if kind == "File" && unsafe { GetFileType(handle.as_raw_handle()) } == FILE_TYPE_PIPE {
            return None;
        }
        self.ask(handle, OBJECT_NAME_INFORMATION)
    }

    fn ask(&mut self, handle: BorrowedHandle<'_>, class: i32) -> Option<OsString> {
        let bytes = u32::try_from(self.words.len().checked_mul(8)?).ok()?;
        // SAFETY: `handle` is open; the buffer is this value's, writable
        // for the length passed and aligned for the string the class
        // writes at its start (the text follows it, in the buffer).
        let status = unsafe {
            NtQueryObject(
                handle.as_raw_handle(),
                class,
                self.words.as_mut_ptr().cast(),
                bytes,
                null_mut(),
            )
        };
        if status != 0 {
            return None;
        }
        // The string: its length in bytes (16 bits), then, a word on,
        // where its text is, which is in the buffer. Read as numbers:
        // where the text is is checked against the buffer before any of
        // it is read.
        let length = usize::try_from(*self.words.first()? & 0xFFFF).ok()?;
        let at = usize::try_from(*self.words.get(1)?).ok()?;
        if length == 0 || at == 0 {
            return Some(OsString::new());
        }
        let start = at.checked_sub(self.words.as_ptr().addr())?;
        let end = start.checked_add(length)?;
        let text: Vec<u8> = self
            .words
            .iter()
            .flat_map(|word| word.to_ne_bytes())
            .skip(start)
            .take(length)
            .collect();
        if text.len() != length || end > bytes.try_into().ok()? {
            return None;
        }
        let units: Vec<u16> = text
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&pair| u16::from_ne_bytes(pair))
            .collect();
        Some(OsString::from_wide(&units))
    }
}

/// Closes this process's handles to [`KERNEL_SECURITY_DEVICE`], which
/// it opened making a random number before it lowered itself, and has no
/// use for: a device of the kernel's that a renderer has no reason to
/// reach, and Chromium closes in its renderers. Each slot a handle was
/// in is then filled with a copy of one event's handle, kept for the
/// process's life, so that a handle made later doesn't take the value,
/// and code that still has it uses an event, not what was made later.
///
/// On the process's first thread, as it lowers itself, when it is alone
/// (a renderer's is): no other thread uses or closes a handle meanwhile,
/// or makes one in a slot. A process with others isn't harmed: a handle
/// closed while it is asked about is an error (handles aren't checked
/// yet), and is skipped; one in its slot made before the copy leaves the
/// slot as any other.
pub(super) fn close_kernel_security_device() -> Result<(), Error> {
    let entries = snapshot(this_process())
        .ok_or_else(|| Error::other("NtQueryInformationProcess", "no handles listed"))?;
    let mut asker = Asker::new();
    let mut event: Option<OwnedHandle> = None;
    for entry in entries {
        // SAFETY: the value is one of this process's handles, listed just
        // now; borrowed only for the questions, which take it as a number
        // the system looks up (one closed meanwhile is an error), and
        // closed only after, here.
        let handle = unsafe {
            BorrowedHandle::borrow_raw(std::ptr::with_exposed_provenance_mut(entry.value))
        };
        let Some(kind) = asker.kind(handle) else {
            continue;
        };
        if kind != "File"
            || asker.name(handle, &kind).as_deref() != Some(KERNEL_SECURITY_DEVICE.as_ref())
        {
            continue;
        }
        // Made before the first handle is closed, so that it doesn't
        // take the slot itself.
        let event = match event {
            Some(ref event) => event,
            None => event.insert(dummy_event()?),
        };
        close_and_fill(entry.value, event)?;
    }
    // The copies are of it: it is kept with them.
    if let Some(event) = event {
        let _ = event.into_raw_handle();
    }
    Ok(())
}

/// An event no one waits for or sets.
fn dummy_event() -> Result<OwnedHandle, Error> {
    // SAFETY: no attributes or name; a new event's handle or none.
    let event = unsafe { CreateEventW(null(), 0, 0, null()) };
    check("CreateEventW", !event.is_null())?;
    Ok(owned(event))
}

/// Closes the handle `value` and fills its slot with a copy of `event`'s
/// handle. The process's free slots are kept a list for each processor,
/// and a handle is made from the list of the processor its thread is on:
/// the thread stays on one processor from the close to the copies.
fn close_and_fill(value: usize, event: &OwnedHandle) -> Result<(), Error> {
    // SAFETY: neither takes an argument; the thread's handle is a
    // pseudo-handle, never closed.
    let (thread, processor) = unsafe { (GetCurrentThread(), GetCurrentProcessorNumber()) };
    // The processor's number in its group, below 64.
    let pinned = 1_usize.checked_shl(processor).map_or(0, |mask| {
        // SAFETY: the thread is this one; the mask is one processor of
        // its group. The mask it had is returned, 0 on a failure.
        unsafe { SetThreadAffinityMask(thread, mask) }
    });
    let handle: HANDLE = std::ptr::with_exposed_provenance_mut(value);
    // SAFETY: `handle` is one of this process's, to the device, which
    // nothing reads again once it has seeded the process's generator; its
    // protection from closing, if it has any, is taken off, and it is
    // closed once, here. Code that still has the number then finds the
    // copy in its slot, or nothing (an error).
    let closed = unsafe {
        SetHandleInformation(handle, HANDLE_FLAG_PROTECT_FROM_CLOSE, 0);
        CloseHandle(handle)
    };
    let closed = check("CloseHandle", closed != 0);
    if closed.is_ok() {
        fill(value, event);
    }
    if pinned != 0 {
        // SAFETY: as above, with the mask the thread had.
        unsafe { SetThreadAffinityMask(thread, pinned) };
    }
    closed
}

/// Copies `event`'s handle until a copy is the value `slot`, which it
/// keeps, for good, and closes the others; it gives up after a few, or
/// once a copy's value is past the slot's (the free slot taken first is
/// the lowest). Whether it was filled isn't an error: the handle is
/// closed either way.
fn fill(slot: usize, event: &OwnedHandle) {
    let mut others = Vec::new();
    for _ in 0..16 {
        let mut copy: HANDLE = null_mut();
        let me = this_process().as_raw_handle();
        // SAFETY: from and to this process's pseudo-handle; `event` is
        // open; the new handle is written to a local.
        let made = unsafe {
            DuplicateHandle(
                me,
                event.as_raw_handle(),
                me,
                &raw mut copy,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            )
        };
        if made == 0 {
            break;
        }
        if copy.addr() == slot {
            return;
        }
        let past = copy.addr() > slot;
        others.push(owned(copy));
        if past {
            break;
        }
    }
}

/// A handle a process has, as another process sees it ([`handles`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Handle {
    /// The handle's value in the process.
    pub value: usize,
    /// The kind of object (`File`, `Event`, `Key`…); `None` for a handle
    /// that can't be copied to be asked about.
    pub kind: Option<OsString>,
    /// The object's name: a file's is its device's (`\Device\…`), and
    /// one with none has an empty one. `None` for a pipe, which isn't
    /// asked, or a handle that can't be copied.
    pub name: Option<OsString>,
    /// How many handles the object has, in every process.
    pub count: usize,
}

/// The handles the process `id` has, as its starter sees them: for
/// whoever started a process to see what it holds. `None` for a process
/// that isn't there or isn't this user's.
pub fn handles(id: u32) -> Option<Vec<Handle>> {
    // SAFETY: opens a handle or fails; nothing is read or written.
    let process = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_DUP_HANDLE, 0, id) };
    if process.is_null() {
        return None;
    }
    let process = owned(process);
    let entries = snapshot(process.as_handle())?;
    let mut asker = Asker::new();
    Some(
        entries
            .into_iter()
            .map(|entry| {
                let copy = copy_of(&process, entry.value);
                let kind = copy.as_ref().and_then(|copy| asker.kind(copy.as_handle()));
                let name = copy
                    .as_ref()
                    .zip(kind.as_ref())
                    .and_then(|(copy, kind)| asker.name(copy.as_handle(), kind));
                Handle {
                    value: entry.value,
                    kind,
                    name,
                    count: entry.count,
                }
            })
            .collect(),
    )
}

/// A copy, in this process, of `process`'s handle `value`, with the same
/// access; `None` for one the system doesn't copy.
fn copy_of(process: &OwnedHandle, value: usize) -> Option<OwnedHandle> {
    let mut copy: HANDLE = null_mut();
    // SAFETY: `process` is open with the right to copy its handles; the
    // value is a number, which the call looks up in that process's
    // table (one that is no handle there is an error); the copy is
    // written to a local, and is this process's.
    let made = unsafe {
        DuplicateHandle(
            process.as_raw_handle(),
            std::ptr::with_exposed_provenance_mut(value),
            this_process().as_raw_handle(),
            &raw mut copy,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        )
    };
    (made != 0).then(|| owned(copy))
}
