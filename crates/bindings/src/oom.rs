//! What a process does when V8 runs out of memory (Blink's `v8_oom.cc`).

use std::ffi::{CStr, c_char};
use std::io::Write as _;

/// The exit code of a process whose V8 ran out of memory: its heap's
/// limit, or what the system would give it. The same on every platform,
/// and 8 bits, all an exit code carries on Linux and macOS (Chromium's
/// renderer raises an exception code on Windows instead, for its crash
/// reporter, which machopo has none of).
pub const OUT_OF_MEMORY_EXIT: u8 = 6;

/// V8's out-of-memory handler, every isolate's (Blink's
/// `ReportV8OOMError`): says so on stderr, then ends the process at once
/// with [`OUT_OF_MEMORY_EXIT`]. Without it V8 prints its stack and aborts,
/// which a sandboxed renderer on Windows can't do (the stack needs a DLL it
/// may no longer load) and its browser takes for a crash. Nothing else
/// runs first, no unwinding and no exit handler: V8 can't go on, and
/// little memory is left, so this allocates none. On a thread of the
/// browser's process (`--single-process`) the whole browser ends with it,
/// as Chromium's single process does.
///
/// V8 calls it on a thread with the isolate; one of its own threads
/// without one runs out by V8's way still (a process-wide handler, V8's
/// `SetFatalMemoryErrorCallback`, isn't in the `v8` crate).
pub(crate) unsafe extern "C" fn out_of_memory(location: *const c_char, details: &v8::OomDetails) {
    // SAFETY: V8 gives a NUL-terminated string that outlives the call, or
    // null, for both (`OOMDetails`, and `FatalProcessOutOfMemory`'s
    // `location`).
    let (location, detail) = unsafe { (text(location), text(details.detail)) };
    let what: &[u8] = if details.is_heap_oom {
        b"its heap's limit"
    } else {
        b"the memory it could have"
    };
    // Stderr is unbuffered: each piece is written as it is, nothing kept.
    let mut stderr = std::io::stderr().lock();
    let pieces: [&[u8]; 5] = [b"V8 ran out of memory: ", what, b" (", location, b")"];
    for piece in pieces {
        let _ = stderr.write_all(piece);
    }
    if !detail.is_empty() {
        let _ = stderr.write_all(b": ");
        let _ = stderr.write_all(detail);
    }
    let _ = stderr.write_all(b"\n");
    ipc::platform::terminate_current_process_immediately(i32::from(OUT_OF_MEMORY_EXIT))
}

/// The bytes of a C string, without its NUL; none for null.
///
/// # Safety
///
/// `text` is null, or a NUL-terminated string that outlives `'a`.
unsafe fn text<'a>(text: *const c_char) -> &'a [u8] {
    if text.is_null() {
        return b"";
    }
    // SAFETY: not null, and the caller promises the rest.
    unsafe { CStr::from_ptr(text) }.to_bytes()
}
