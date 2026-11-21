//! The same bytes there and back over the `interprocess` crate's local
//! socket (on Windows a named pipe of its own making), beside this
//! probe's pipe: `cargo run --release -- compare`.

use std::io::{Read, Write};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{GenericNamespaced, ListenerOptions, Stream};

use crate::{KIB, MIB, NOTE, Stats, inline, label, rounds, stats};

/// Reads `size` bytes and answers with `reply`, until the host is gone.
pub fn child(args: &[String]) -> i32 {
    let [name, size, reply] = args else {
        return 2;
    };
    let (size, reply): (usize, usize) = (size.parse().unwrap(), reply.parse().unwrap());
    let name = name.as_str().to_ns_name::<GenericNamespaced>().unwrap();
    let mut stream = Stream::connect(name).unwrap();
    let mut buffer = vec![0u8; size];
    let answer = vec![1u8; reply];
    while stream.read_exact(&mut buffer).is_ok() {
        if stream.write_all(&answer).is_err() {
            break;
        }
    }
    0
}

fn there_and_back(size: usize, reply: usize) -> Stats {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let name = format!(
        "machopo.probe.others.{}.{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let listener = ListenerOptions::new()
        .name(name.as_str().to_ns_name::<GenericNamespaced>().unwrap())
        .create_sync()
        .unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--child", "interprocess", &name])
        .args([size.to_string(), reply.to_string()])
        .spawn()
        .unwrap();
    let mut stream = listener.accept().unwrap();
    let bytes = vec![5u8; size];
    let mut answer = vec![0u8; reply];
    let mut times = Vec::new();
    let n = rounds(size);
    for i in 0..n + n / 10 {
        let from = Instant::now();
        stream.write_all(&bytes).unwrap();
        stream.read_exact(&mut answer).unwrap();
        if i >= n / 10 {
            times.push(from.elapsed().as_secs_f64() * 1e6);
        }
    }
    drop(stream);
    let _ = child.wait();
    stats(times)
}

pub fn compare() {
    println!("\n## Beside `interprocess` (its local socket, blocking), µs: median (p90)");
    println!(
        "  {:>7} | {:>18} | {:>22} | {:>22}",
        "bytes", "interprocess", "this pipe, whole", "this pipe, 64 KiB each"
    );
    let cell = |s: Stats| format!("{:.1} ({:.1})", s.median, s.p90);
    for (size, reply) in [(NOTE, NOTE), (64 * KIB, 1), (MIB, 1), (8 * MIB, 1)] {
        let theirs = there_and_back(size, reply);
        let whole = inline(64 * KIB, size, usize::MAX, size, reply);
        let parts = inline(64 * KIB, size, 64 * KIB, 64 * KIB, reply);
        println!(
            "  {:>7} | {:>18} | {:>22} | {:>22}",
            label(size),
            cell(theirs),
            cell(whole),
            cell(parts)
        );
    }
}
