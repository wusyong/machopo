//! A probe of overlapped named pipes for the Windows layer of the channel:
//! the host (no arguments) runs every section and prints what it found;
//! `--child <mode> ...` is the same program started by the host with its
//! end of a pipe inherited.

#![cfg(windows)]

mod others;

use std::process::{Child, Command};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_IO_PENDING, GENERIC_READ, GENERIC_WRITE, GetLastError, HANDLE,
    HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, SetHandleInformation, WAIT_OBJECT_0,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, OPEN_EXISTING,
    PIPE_ACCESS_DUPLEX, ReadFile, SECURITY_ANONYMOUS, SECURITY_SQOS_PRESENT, WriteFile,
};
use windows_sys::Win32::System::IO::{
    CancelIoEx, CreateIoCompletionPort, GetOverlappedResult, GetQueuedCompletionStatus, OVERLAPPED,
};
use windows_sys::Win32::System::Memory::{
    CreateFileMappingW, FILE_MAP_ALL_ACCESS, MapViewOfFile, PAGE_READWRITE,
};
use windows_sys::Win32::System::Pipes::{
    CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
};
use windows_sys::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

const KIB: usize = 1 << 10;
const MIB: usize = 1 << 20;
/// What a small message is here: a note of a few fields.
const NOTE: usize = 64;

fn last_error() -> u32 {
    // SAFETY: reads this thread's last error.
    unsafe { GetLastError() }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// Both ends of a new pipe, overlapped, and its name. Nobody else can be
/// the client: one instance, and the name's first.
fn pair(buffer: u32) -> (HANDLE, HANDLE, Vec<u16>) {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    let name = wide(&format!(
        r"\\.\pipe\machopo.probe.{}.{}.{nanos}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    // SAFETY: `name` is a NUL-terminated string that outlives both calls.
    unsafe {
        let server = CreateNamedPipeW(
            name.as_ptr(),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            buffer,
            buffer,
            0,
            null(),
        );
        assert!(server != INVALID_HANDLE_VALUE, "pipe: {}", last_error());
        let client = CreateFileW(
            name.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            0,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_ANONYMOUS,
            null_mut(),
        );
        assert!(client != INVALID_HANDLE_VALUE, "client: {}", last_error());
        (server, client, name)
    }
}

fn event() -> HANDLE {
    // SAFETY: no name, no attributes; manual reset, as overlapped I/O asks.
    let event = unsafe { CreateEventW(null(), 1, 0, null()) };
    assert!(!event.is_null());
    event
}

/// One end, read and written one operation at a time, each waited for.
struct Io {
    handle: HANDLE,
    event: HANDLE,
}

impl Io {
    fn new(handle: HANDLE) -> Self {
        Self {
            handle,
            event: event(),
        }
    }

    fn overlapped(&self) -> OVERLAPPED {
        OVERLAPPED {
            hEvent: self.event,
            // SAFETY: all zeroes is an `OVERLAPPED`.
            ..unsafe { std::mem::zeroed() }
        }
    }

    /// Waits for `overlapped`'s operation, started just now with `started`.
    fn finish(&self, started: i32, overlapped: &OVERLAPPED, done: &mut u32) -> Result<usize, u32> {
        if started == 0 {
            let error = last_error();
            if error != ERROR_IO_PENDING {
                return Err(error);
            }
            // SAFETY: `overlapped` is this operation's, alive until it is over.
            if unsafe { GetOverlappedResult(self.handle, overlapped, done, 1) } == 0 {
                return Err(last_error());
            }
        }
        Ok(*done as usize)
    }

    fn read(&self, buffer: &mut [u8]) -> Result<usize, u32> {
        let mut overlapped = self.overlapped();
        let mut done = 0;
        // SAFETY: `buffer` and `overlapped` outlive the operation, which
        // `finish` waits for.
        let started = unsafe {
            ReadFile(
                self.handle,
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                &mut done,
                &mut overlapped,
            )
        };
        self.finish(started, &overlapped, &mut done)
    }

    /// As `read`, but gives up after `millis`: `Err(0)`.
    fn read_within(&self, buffer: &mut [u8], millis: u32) -> Result<usize, u32> {
        let mut overlapped = self.overlapped();
        let mut done = 0;
        // SAFETY: as `read`; a read that timed out is cancelled and then
        // waited for before `buffer` and `overlapped` go.
        unsafe {
            if ReadFile(
                self.handle,
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                &mut done,
                &mut overlapped,
            ) != 0
            {
                return Ok(done as usize);
            }
            let error = last_error();
            if error != ERROR_IO_PENDING {
                return Err(error);
            }
            let timed_out = WaitForSingleObject(self.event, millis) != WAIT_OBJECT_0;
            if timed_out {
                CancelIoEx(self.handle, &overlapped);
            }
            let finished = GetOverlappedResult(self.handle, &overlapped, &mut done, 1) != 0;
            match (finished, timed_out) {
                (true, _) => Ok(done as usize),
                (false, true) => Err(0),
                (false, false) => Err(last_error()),
            }
        }
    }

    fn read_exact(&self, buffer: &mut [u8], at_most: usize) -> Result<(), u32> {
        let mut at = 0;
        while at < buffer.len() {
            let end = (at + at_most).min(buffer.len());
            at += self.read(&mut buffer[at..end])?;
        }
        Ok(())
    }

    /// Writes all of `bytes`, `chunk` at a time.
    fn write_all(&self, bytes: &[u8], chunk: usize) -> Result<(), u32> {
        for part in bytes.chunks(chunk) {
            let mut at = 0;
            while at < part.len() {
                let mut overlapped = self.overlapped();
                let mut done = 0;
                // SAFETY: as `read`.
                let started = unsafe {
                    WriteFile(
                        self.handle,
                        part[at..].as_ptr(),
                        (part.len() - at) as u32,
                        &mut done,
                        &mut overlapped,
                    )
                };
                at += self.finish(started, &overlapped, &mut done)?;
            }
        }
        Ok(())
    }
}

/// Starts this program again as the child `mode`, which inherits
/// `handles` and is told their values after `mode`.
fn spawn(mode: &str, handles: &[HANDLE], rest: &[usize]) -> Child {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args(["--child", mode]);
    for &handle in handles {
        // SAFETY: a handle of this process's.
        let set = unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) };
        assert!(set != 0, "inherit: {}", last_error());
        command.arg((handle as usize).to_string());
    }
    command.args(rest.iter().map(usize::to_string));
    let child = command.spawn().unwrap();
    for &handle in handles {
        // SAFETY: as above; the child has its own now.
        unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) };
    }
    child
}

fn close(handle: HANDLE) {
    // SAFETY: a handle of this process's, not used again.
    unsafe { CloseHandle(handle) };
}

/// A section of `len` bytes, and it mapped to read and write.
fn section(len: usize) -> (HANDLE, *mut u8) {
    // SAFETY: a section of the page file, with no name.
    let handle = unsafe {
        CreateFileMappingW(
            INVALID_HANDLE_VALUE,
            null(),
            PAGE_READWRITE,
            (len as u64 >> 32) as u32,
            len as u32,
            null(),
        )
    };
    assert!(!handle.is_null(), "section: {}", last_error());
    (handle, map(handle, len))
}

fn map(section: HANDLE, len: usize) -> *mut u8 {
    // SAFETY: maps wherever the kernel finds room.
    let view = unsafe { MapViewOfFile(section, FILE_MAP_ALL_ACCESS, 0, 0, len) };
    assert!(!view.Value.is_null(), "map: {}", last_error());
    view.Value.cast()
}

// ---------------------------------------------------------------------
// The child.

fn child(mode: &str, args: &[usize]) -> i32 {
    match mode {
        // Never touches its end.
        "exit" => 7,
        "sleep" => {
            std::thread::sleep(Duration::from_secs(60));
            0
        }
        // Reads `size` bytes, `read` at a time, and answers with `reply`.
        "inline" => {
            let &[pipe, size, reply, read] = args else {
                return 2;
            };
            let io = Io::new(pipe as HANDLE);
            let mut buffer = vec![0u8; size];
            let answer = vec![1u8; reply];
            while io.read_exact(&mut buffer, read).is_ok() {
                if io.write_all(&answer, usize::MAX).is_err() {
                    break;
                }
            }
            0
        }
        // Reads a note, copies `size` bytes out of the section, answers.
        "region" => {
            let &[pipe, region, size] = args else {
                return 2;
            };
            let io = Io::new(pipe as HANDLE);
            let from = map(region as HANDLE, size);
            let mut buffer = vec![0u8; size];
            let mut note = [0u8; NOTE];
            while io.read_exact(&mut note, NOTE).is_ok() {
                // SAFETY: `from` is `size` bytes mapped, `buffer` is `size`
                // bytes; the host writes only between an answer and its
                // next note.
                unsafe { std::ptr::copy_nonoverlapping(from, buffer.as_mut_ptr(), size) };
                std::hint::black_box(&buffer);
                if io.write_all(&[1], usize::MAX).is_err() {
                    break;
                }
            }
            0
        }
        _ => 2,
    }
}

// ---------------------------------------------------------------------
// What a pipe does.

fn name_of(error: u32) -> &'static str {
    match error {
        0 => "ok",
        109 => "ERROR_BROKEN_PIPE",
        231 => "ERROR_PIPE_BUSY",
        233 => "ERROR_NO_DATA",
        995 => "ERROR_OPERATION_ABORTED",
        996 => "ERROR_IO_INCOMPLETE",
        997 => "ERROR_IO_PENDING",
        _ => "?",
    }
}

/// A thread starts a write that can't finish (nobody reads), then is
/// over or stays. What became of the write, and what the reader gets.
fn a_writers_thread_ends(port: bool, thread_stays: bool) {
    let (server, client, _) = pair(4 * KIB as u32);
    let completion = if port {
        // SAFETY: a new port, with `server` on it.
        let completion = unsafe { CreateIoCompletionPort(server, null_mut(), 1, 0) };
        assert!(!completion.is_null());
        Some(completion)
    } else {
        None
    };
    // Leaked: the kernel has both until the write is over, whatever
    // becomes of this function.
    let bytes: &'static [u8] = vec![7u8; MIB].leak();
    let overlapped: &'static mut OVERLAPPED = Box::leak(Box::new(OVERLAPPED {
        hEvent: if port { null_mut() } else { event() },
        // SAFETY: all zeroes is an `OVERLAPPED`.
        ..unsafe { std::mem::zeroed() }
    }));
    let overlapped: *mut OVERLAPPED = overlapped;

    let (server_n, overlapped_n) = (server as usize, overlapped as usize);
    let (stay, staying) = std::sync::mpsc::channel::<()>();
    let writer = std::thread::spawn(move || {
        // SAFETY: `bytes` and the `OVERLAPPED` are leaked.
        let started = unsafe {
            WriteFile(
                server_n as HANDLE,
                bytes.as_ptr(),
                bytes.len() as u32,
                null_mut(),
                overlapped_n as *mut OVERLAPPED,
            )
        };
        let error = if started == 0 { last_error() } else { 0 };
        if thread_stays {
            let _ = staying.recv();
        }
        error
    });
    let mut writer = Some(writer);
    let started = if thread_stays {
        std::thread::sleep(Duration::from_millis(100));
        ERROR_IO_PENDING
    } else {
        writer.take().unwrap().join().unwrap()
    };
    std::thread::sleep(Duration::from_millis(100));

    let state = |overlapped: *mut OVERLAPPED| {
        let mut done = 0;
        // SAFETY: the write's own `OVERLAPPED`; doesn't wait.
        if unsafe { GetOverlappedResult(server, overlapped, &mut done, 0) } != 0 {
            (0, done)
        } else {
            (last_error(), done)
        }
    };
    let (before, _) = state(overlapped);

    let reader = Io::new(client);
    let mut buffer = vec![0u8; 64 * KIB];
    let mut got = 0;
    while got < bytes.len() {
        match reader.read_within(&mut buffer, 300) {
            Ok(n) => got += n,
            Err(_) => break,
        }
    }
    // The write's own completion is the writer's thread's to note.
    std::thread::sleep(Duration::from_millis(50));
    let (after, written) = state(overlapped);
    let packet = completion.map(|completion| {
        let (mut done, mut key, mut which) = (0, 0, null_mut());
        // SAFETY: out parameters of this frame's.
        let ok =
            unsafe { GetQueuedCompletionStatus(completion, &mut done, &mut key, &mut which, 300) };
        if which.is_null() {
            "no packet".to_owned()
        } else if ok != 0 {
            format!("packet: {done} bytes")
        } else {
            format!("packet: {}", name_of(last_error()))
        }
    });
    println!(
        "  {:<22} {:<16} started: {:<17} after 100 ms: {:<24} reader got {:>7} of {}; then: {} ({written} written){}",
        if port { "completion port" } else { "event" },
        if thread_stays {
            "thread stays"
        } else {
            "thread ends"
        },
        name_of(started),
        name_of(before),
        got,
        bytes.len(),
        name_of(after),
        packet
            .map(|packet| format!("; {packet}"))
            .unwrap_or_default(),
    );
    drop(stay);
    if let Some(writer) = writer {
        let _ = writer.join();
    }
    close(client);
    close(server);
}

fn a_second_client() {
    let (server, client, name) = pair(4 * KIB as u32);
    // SAFETY: `name` is NUL-terminated.
    let second = unsafe {
        CreateFileW(
            name.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            0,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED,
            null_mut(),
        )
    };
    if second == INVALID_HANDLE_VALUE {
        println!("  a second client: refused, {}", name_of(last_error()));
    } else {
        println!("  a second client: CONNECTED");
        close(second);
    }
    // And a second server of the same name.
    // SAFETY: as `pair`.
    let again = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE,
            1,
            4096,
            4096,
            0,
            null(),
        )
    };
    if again == INVALID_HANDLE_VALUE {
        println!("  a second instance of the name: refused, {}", last_error());
    } else {
        println!("  a second instance of the name: MADE");
        close(again);
    }
    close(client);
    close(server);
}

/// How soon the host's read ends once its child is gone.
fn a_child_that_goes(mode: &str, kill: bool) {
    let mut times = Vec::new();
    let mut error = 0;
    for _ in 0..5 {
        let (server, client, _) = pair(4 * KIB as u32);
        let mut child = spawn(mode, &[client], &[]);
        close(client);
        let io = Io::new(server);
        let from = Instant::now();
        if kill {
            // Let it start first.
            std::thread::sleep(Duration::from_millis(200));
        }
        let from = if kill {
            let now = Instant::now();
            child.kill().unwrap();
            now
        } else {
            from
        };
        error = io.read(&mut [0u8; 16]).err().unwrap_or(0);
        times.push(from.elapsed());
        let _ = child.wait();
        close(server);
    }
    times.sort();
    println!(
        "  {:<44} the host's read ends with {} ({error}); {:?} to {:?}",
        if kill {
            "a child killed:"
        } else {
            "a child that exits without touching its end:"
        },
        name_of(error),
        times[0],
        times[times.len() - 1],
    );
}

// ---------------------------------------------------------------------
// Timings.

struct Stats {
    median: f64,
    p90: f64,
}

fn stats(mut micros: Vec<f64>) -> Stats {
    micros.sort_by(f64::total_cmp);
    Stats {
        median: micros[micros.len() / 2],
        p90: micros[micros.len() * 9 / 10],
    }
}

fn rounds(size: usize) -> usize {
    match size {
        s if s <= NOTE => 20_000,
        s if s <= 64 * KIB => 3000,
        s if s <= MIB => 400,
        _ => 60,
    }
}

/// `size` bytes to the child in the pipe, written `chunk` at a time and
/// read `read` at a time, until its answer of `reply` bytes is back.
fn inline(buffer: usize, size: usize, chunk: usize, read: usize, reply: usize) -> Stats {
    let (server, client, _) = pair(buffer as u32);
    let mut child = spawn("inline", &[client], &[size, reply, read]);
    close(client);
    let io = Io::new(server);
    let bytes = vec![5u8; size];
    let mut answer = vec![0u8; reply];
    let mut times = Vec::new();
    let n = rounds(size);
    for i in 0..n + n / 10 {
        let from = Instant::now();
        io.write_all(&bytes, chunk).unwrap();
        io.read_exact(&mut answer, reply).unwrap();
        // The first tenth warms up.
        if i >= n / 10 {
            times.push(from.elapsed().as_secs_f64() * 1e6);
        }
    }
    close(server);
    let _ = child.wait();
    stats(times)
}

/// `size` bytes to the child by a region kept and used again: copied in,
/// a note sent, copied out by the child, its answer back.
fn region(buffer: usize, size: usize) -> Stats {
    let (server, client, _) = pair(buffer as u32);
    let (section, to) = section(size);
    let mut child = spawn("region", &[client, section], &[size]);
    close(client);
    let io = Io::new(server);
    let bytes = vec![5u8; size];
    let note = [3u8; NOTE];
    let mut times = Vec::new();
    let n = rounds(size);
    for i in 0..n + n / 10 {
        let from = Instant::now();
        // SAFETY: `to` is `size` bytes mapped; the child reads them only
        // between the note and its answer.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), to, size) };
        io.write_all(&note, usize::MAX).unwrap();
        io.read_exact(&mut [0u8; 1], 1).unwrap();
        if i >= n / 10 {
            times.push(from.elapsed().as_secs_f64() * 1e6);
        }
    }
    close(server);
    let _ = child.wait();
    close(section);
    stats(times)
}

fn label(bytes: usize) -> String {
    if bytes == usize::MAX {
        "whole".into()
    } else if bytes >= MIB {
        format!("{} MiB", bytes / MIB)
    } else if bytes >= KIB {
        format!("{} KiB", bytes / KIB)
    } else {
        format!("{bytes} B")
    }
}

fn timings() {
    let buffers = [4 * KIB, 64 * KIB, MIB];

    println!("\n## A note there and back ({NOTE} bytes each way), µs: median (p90)");
    for buffer in buffers {
        let s = inline(buffer, NOTE, usize::MAX, NOTE, NOTE);
        println!(
            "  pipe buffer {:>7}: {:>7.1} ({:.1})",
            label(buffer),
            s.median,
            s.p90
        );
    }

    println!("\n## Large bytes to the child, until its one-byte answer, µs: median (p90)");
    println!(
        "  {:>7} | {:>11} | {:>7} | {:>7} | {:>20}",
        "bytes", "pipe buffer", "written", "read", "in the pipe"
    );
    for size in [64 * KIB, MIB, 8 * MIB] {
        for buffer in buffers {
            for chunk in [64 * KIB, MIB, usize::MAX] {
                if chunk != usize::MAX && chunk >= size {
                    continue;
                }
                for read in [64 * KIB, MIB] {
                    if read > size && read != 64 * KIB {
                        continue;
                    }
                    let s = inline(buffer, size, chunk, read, 1);
                    println!(
                        "  {:>7} | {:>11} | {:>7} | {:>7} | {:>10.1} ({:.1})",
                        label(size),
                        label(buffer),
                        label(chunk),
                        label(read),
                        s.median,
                        s.p90
                    );
                }
            }
        }
        let s = region(4 * KIB, size);
        println!(
            "  {:>7} | by a region kept and used again (a note in the pipe)  | {:>10.1} ({:.1})",
            label(size),
            s.median,
            s.p90
        );
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--child") {
        if args[2] == "interprocess" {
            std::process::exit(others::child(&args[3..]));
        }
        let numbers: Vec<usize> = args[3..].iter().map(|n| n.parse().unwrap()).collect();
        std::process::exit(child(&args[2], &numbers));
    }
    let only = args.get(1).map(String::as_str);

    if only.is_none_or(|only| only == "behaviour") {
        println!("## A write still going when the thread that started it is over");
        a_writers_thread_ends(false, true);
        a_writers_thread_ends(false, false);
        a_writers_thread_ends(true, true);
        a_writers_thread_ends(true, false);

        println!("\n## Who else can have the pipe");
        a_second_client();

        println!("\n## A child that goes");
        a_child_that_goes("exit", false);
        a_child_that_goes("sleep", true);
    }
    if only.is_none_or(|only| only == "timings") {
        timings();
    }
    if only.is_none_or(|only| only == "compare") {
        others::compare();
    }
}
