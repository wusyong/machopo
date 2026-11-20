//! RFC 0004's prototype: see README.md.

#[cfg(not(target_os = "macos"))]
compile_error!("the prototype is macOS's");

mod channel;
mod msg;
mod rendezvous;
mod ring;
mod shm;
mod sys;

use std::process::{Child, Command};
use std::time::{Duration, Instant};

use mach2::message::{MACH_MSG_TYPE_MAKE_SEND, MACH_MSG_TYPE_MOVE_RECEIVE};
use serde::{Deserialize, Serialize};

use channel::{Channel, ChannelError, ChannelPair};

/// Destinations on a control channel.
const D_CMD: u32 = 0;
const D_ECHO: u32 = 1;
/// Bytes to take in (inline, or out of line), answered with `D_ACK`.
const D_SINK: u32 = 2;
/// Bytes in an own region, by `(number, offset, len)`: copied out,
/// answered with `D_ACK`.
const D_REGION: u32 = 3;
const D_REPLY: u32 = 5;
/// How many bytes a `D_SINK`, `D_REGION` or `D_FRESH` message brought.
const D_ACK: u32 = 6;
/// The reader of a ring sleeps on its control channel: wake up.
const D_WAKE: u32 = 4;
/// Bytes to take in and say nothing.
const D_DROP: u32 = 9;
/// A frame, encoded: decoded, answered with `D_ACK`.
const D_FRAME: u32 = 10;
/// Bytes in a region made for this message alone (the message carries its
/// read-only entry; the payload is the length): mapped, copied out,
/// unmapped, answered with `D_ACK`.
const D_FRESH: u32 = 8;
/// On the direct channel between two children: pixels in a region.
const D_PIXELS: u32 = 7;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum Misbehave {
    /// Name a region number the receiver doesn't have for this peer.
    UnknownRegion,
    /// Name a range that runs past the region.
    OutOfRange,
    /// Attach a right, from an end that may not.
    AttachRight,
    /// Send a message with the id of the kernel's dead-name notification,
    /// then a good one.
    ForgeNotification,
}

#[derive(Debug, Serialize, Deserialize)]
enum Cmd {
    /// Answer `Reply::Hello`.
    Hello,
    /// Try what a sandbox should and shouldn't let through.
    Probe,
    /// A region of this process's own (the message carries its entry),
    /// to map and keep under `number`.
    Region { number: u32, len: usize, writable: bool },
    /// A region the peer on the direct channel may name as `number` (the
    /// message carries a read-only entry). Answers `Reply::PeerRegion`.
    PeerRegion { number: u32, len: usize },
    /// The direct channel to another child, `peer_pid` (the message
    /// carries this end's receive right and a send right to the peer's).
    PeerChannel { peer_pid: i32, may_attach: bool, peer_may_attach: bool },
    /// Fill `len` bytes at `offset` of own region `number` from `seed`
    /// and tell the peer on the direct channel where they are.
    WriteAndTell { number: u32, offset: u32, len: u32, seed: u8, misbehave: Option<Misbehave> },
    /// Take one message from the direct channel and answer
    /// `Reply::Direct`.
    ServeDirect,
    /// Two rings (the message carries their entries): one to read, one to
    /// write.
    Rings { len: usize },
    /// Read `count` messages from the ring and write each back
    /// (`frames`: decode each as a frame and write its length back).
    RingEcho { count: u32, spin: u32, frames: bool },
    /// Read `count` messages from the ring (`frames`: decode each as a
    /// frame), then answer `Reply::Done`.
    RingSink { count: u32, spin: u32, frames: bool },
    /// Read `bytes` bytes from the ring as a pipe, then answer
    /// `Reply::Done`.
    RingPipe { bytes: usize, spin: u32 },
    /// Let 1024 messages wait in this end's port, not 5.
    RaiseQueueLimit,
    /// Ask the rendezvous again: a second time gets nothing.
    FetchAgain,
    /// End.
    Exit,
}

#[derive(Debug, Serialize, Deserialize)]
enum Reply {
    FetchedAgain(String),
    Hello { pid: u32 },
    Probe(Vec<(String, String)>),
    /// The kernel's answer to mapping the read-only entry writable.
    PeerRegion { writable_map: i32 },
    Direct { outcome: String, checksum: u64, forged_notifications: u32 },
    Done,
}

fn checksum(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn pattern(seed: u8, len: usize) -> Vec<u8> {
    (0..len).map(|i| seed.wrapping_add(i as u8).wrapping_mul(31)).collect()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.iter().find_map(|arg| arg.strip_prefix("--type=")) {
        Some(kind) => child_main(kind, args.iter().any(|arg| arg == "--sandbox")),
        None => host_main(),
    }
}

// ---------------------------------------------------------------- child

/// The profile a child applies before anything else: what RFC 0004's
/// "為 sandbox 留的條件" lists, and nothing more.
fn sandbox_profile(host_pid: u32) -> String {
    let exe = std::env::current_exe().unwrap();
    format!(
        r#"(version 1)
(deny default)
(allow mach-lookup (global-name "{}"))
(allow file-read* (subpath "/System/Library/Fonts"))
(allow file-read* (literal "{}"))
"#,
        rendezvous::name_for(host_pid),
        exe.display()
    )
}

fn enter_sandbox(host_pid: u32) {
    let profile = std::ffi::CString::new(sandbox_profile(host_pid)).unwrap();
    let mut error = std::ptr::null_mut();
    // SAFETY: a call with a C string and a place for an error string.
    let failed = unsafe { sys::sandbox_init(profile.as_ptr(), 0, &mut error) };
    if failed != 0 {
        // SAFETY: on failure the call left a C string there.
        let why = unsafe { std::ffi::CStr::from_ptr(error) }.to_string_lossy().into_owned();
        // SAFETY: the error string is the call's to free.
        unsafe { sys::sandbox_free_error(error) };
        eprintln!("sandbox_init: {why}");
        std::process::exit(5);
    }
}

fn probe(sandboxed: bool) -> Vec<(String, String)> {
    use std::io::Read;
    fn outcome<T>(result: std::io::Result<T>, ok: impl Fn(T) -> String) -> String {
        match result {
            Ok(value) => format!("ok ({})", ok(value)),
            Err(error) => format!("refused ({:?})", error.kind()),
        }
    }
    let read = |path: &str| {
        outcome(std::fs::File::open(path).and_then(|mut file| {
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).map(|_| bytes.len())
        }), |len| format!("{len} bytes"))
    };
    let mut other = mach2::port::MACH_PORT_NULL;
    let other_name = c"com.apple.system.notification_center";
    // SAFETY: a call with a C string and a place for one name.
    let other_kr = unsafe {
        mach2::bootstrap::bootstrap_look_up(mach2::bootstrap::bootstrap_port, other_name.as_ptr(), &mut other)
    };
    let mut size = 16384u64;
    let mut entry = mach2::port::MACH_PORT_NULL;
    // SAFETY: a call that writes a size and a name.
    let shm_kr = unsafe {
        mach2::vm::mach_make_memory_entry_64(
            mach2::traps::mach_task_self(),
            &mut size,
            0,
            sys::MAP_MEM_NAMED_CREATE | mach2::vm_prot::VM_PROT_READ | mach2::vm_prot::VM_PROT_WRITE,
            &mut entry,
            mach2::port::MACH_PORT_NULL,
        )
    };
    let home = std::env::var("HOME").unwrap_or_default();
    vec![
        ("sandboxed".into(), sandboxed.to_string()),
        ("read a system font".into(), read("/System/Library/Fonts/Helvetica.ttc")),
        ("read /etc/passwd".into(), read("/etc/passwd")),
        ("list the home folder".into(), outcome(std::fs::read_dir(&home).map(Iterator::count), |n| format!("{n} entries"))),
        ("create a file in /tmp".into(), {
            let path = format!("/tmp/mp-prototype-probe-{}", std::process::id());
            let created = outcome(std::fs::File::create(&path), |_| "created".into());
            let _ = std::fs::remove_file(&path);
            created
        }),
        ("connect to 127.0.0.1:9".into(), outcome(std::net::TcpStream::connect("127.0.0.1:9"), |_| "connected".into())),
        ("start /usr/bin/true".into(), outcome(Command::new("/usr/bin/true").status(), |status| status.to_string())),
        ("look another service up".into(), format!("kern_return {other_kr:#x}")),
        ("make a memory entry".into(), format!("kern_return {shm_kr:#x}")),
    ]
}

fn child_main(kind: &str, sandbox: bool) {
    let host_pid = std::os::unix::process::parent_id();
    // First of all: everything after runs inside.
    if sandbox {
        enter_sandbox(host_pid);
    }
    let ports = match rendezvous::fetch(host_pid) {
        Ok(ports) if ports.len() == 2 => ports,
        other => {
            eprintln!("child {kind}: no ports from the rendezvous: {other:?}");
            std::process::exit(3);
        }
    };
    // A renderer's end may attach nothing; the host's may.
    let may_attach = kind != "renderer";
    let mut channel = Channel::new(ports[0], ports[1], may_attach, true);
    channel.expect_peer(host_pid.cast_signed());
    let mut own = shm::RegionTable::default();
    let mut peers = shm::RegionTable::default();
    let mut direct: Option<Channel> = None;
    let mut rings: Option<(ring::RingReader, ring::RingWriter)> = None;
    let mut scratch = Vec::new();
    let ack = |channel: &mut Channel, len: usize| channel.send(D_ACK, &(len as u64).to_ne_bytes()).unwrap();
    loop {
        let received = match channel.recv(None) {
            Ok(received) => received,
            Err(ChannelError::Disconnected) => std::process::exit(0),
            Err(error) => {
                eprintln!("child {kind}: {error:?}");
                std::process::exit(4);
            }
        };
        match received.destination {
            D_ECHO => channel.send(D_ECHO, &received.payload).unwrap(),
            D_WAKE => {}
            D_DROP => {
                std::hint::black_box(&received.payload);
            }
            D_FRAME => {
                let frame: protocol::viz::CompositorFrame = postcard::from_bytes(&received.payload).expect("a frame");
                std::hint::black_box(&frame);
                ack(&mut channel, received.payload.len());
            }
            D_SINK => {
                // Inline bytes are in our memory already (the receive
                // copied them out of the message buffer); out-of-line
                // ones are copied here.
                let ool = received.ool.as_ref().map_or(0, |ool| {
                    scratch = std::hint::black_box(ool.to_vec());
                    scratch.len()
                });
                ack(&mut channel, std::hint::black_box(&received.payload).len() + ool);
            }
            D_REGION => {
                let (number, offset, len): (u32, u32, u32) = postcard::from_bytes(&received.payload).unwrap();
                own.read(number, offset, len, &mut scratch).expect("a range of an own region");
                ack(&mut channel, std::hint::black_box(&scratch).len());
            }
            D_FRESH => {
                let len = u64::from_ne_bytes(received.payload[..8].try_into().unwrap()) as usize;
                let mapping = shm::map(received.ports[0], len, false).expect("the region maps");
                msg::deallocate(received.ports[0]);
                assert!(mapping.copy_out(0, len, &mut scratch));
                drop(mapping);
                ack(&mut channel, std::hint::black_box(&scratch).len());
            }
            D_CMD => match postcard::from_bytes(&received.payload).expect("a command") {
                Cmd::Hello => reply(&mut channel, &Reply::Hello { pid: std::process::id() }),
                Cmd::Probe => reply(&mut channel, &Reply::Probe(probe(sandbox))),
                Cmd::Region { number, len, writable } => {
                    let mapping = shm::map(received.ports[0], len, writable).expect("the region maps");
                    msg::deallocate(received.ports[0]);
                    own.insert(number, mapping);
                    reply(&mut channel, &Reply::Done);
                }
                Cmd::PeerRegion { number, len } => {
                    // A read-only entry must not map writable.
                    let writable_map = shm::map(received.ports[0], len, true).err().unwrap_or(0);
                    let mapping = shm::map(received.ports[0], len, false).expect("the region maps read-only");
                    msg::deallocate(received.ports[0]);
                    peers.insert(number, mapping);
                    reply(&mut channel, &Reply::PeerRegion { writable_map });
                }
                Cmd::PeerChannel { peer_pid, may_attach, peer_may_attach } => {
                    let mut end = Channel::new(received.ports[0], received.ports[1], may_attach, peer_may_attach);
                    end.expect_peer(peer_pid);
                    direct = Some(end);
                    reply(&mut channel, &Reply::Done);
                }
                Cmd::WriteAndTell { number, offset, len, seed, misbehave } => {
                    let direct = direct.as_mut().expect("a direct channel");
                    let region = own.get(number).expect("the region");
                    region.copy_in(offset as usize, &pattern(seed, len as usize));
                    let told = match misbehave {
                        Some(Misbehave::UnknownRegion) => (number + 100, offset, len),
                        Some(Misbehave::OutOfRange) => (number, u32::try_from(region.len()).unwrap() - 1, len),
                        _ => (number, offset, len),
                    };
                    let payload = postcard::to_allocvec(&told).unwrap();
                    match misbehave {
                        Some(Misbehave::AttachRight) => {
                            let right = (direct.receive_port(), MACH_MSG_TYPE_MAKE_SEND);
                            direct.send_forbidden(D_PIXELS, &[right]);
                        }
                        Some(Misbehave::ForgeNotification) => {
                            direct.send_forged_notification();
                            direct.send(D_PIXELS, &payload).unwrap();
                        }
                        _ => direct.send(D_PIXELS, &payload).unwrap(),
                    }
                    reply(&mut channel, &Reply::Done);
                }
                Cmd::ServeDirect => {
                    let direct = direct.as_mut().expect("a direct channel");
                    let mut pixels = Vec::new();
                    let outcome = match direct.recv(Some(2000)) {
                        Ok(received) if received.destination == D_PIXELS => {
                            match postcard::from_bytes::<(u32, u32, u32)>(&received.payload) {
                                Ok((number, offset, len)) => match peers.read(number, offset, len, &mut pixels) {
                                    Ok(()) => "ok".to_owned(),
                                    Err(why) => format!("bad message: {why}"),
                                },
                                Err(_) => "bad message: not a region reference".to_owned(),
                            }
                        }
                        Ok(received) => format!("bad message: destination {}", received.destination),
                        Err(error) => format!("{error:?}"),
                    };
                    reply(
                        &mut channel,
                        &Reply::Direct {
                            outcome,
                            checksum: checksum(&pixels),
                            forged_notifications: direct.forged_notifications,
                        },
                    );
                }
                Cmd::Rings { len } => {
                    let reader = ring::RingReader::new(shm::map(received.ports[0], len, true).unwrap());
                    let writer = ring::RingWriter::new(shm::map(received.ports[1], len, true).unwrap());
                    msg::deallocate(received.ports[0]);
                    msg::deallocate(received.ports[1]);
                    rings = Some((reader, writer));
                    reply(&mut channel, &Reply::Done);
                }
                Cmd::RingEcho { count, spin, frames } => {
                    let (reader, writer) = rings.as_mut().expect("rings");
                    for _ in 0..count {
                        ring_read(reader, &mut channel, spin, &mut scratch);
                        if frames {
                            let frame: protocol::viz::CompositorFrame = postcard::from_bytes(&scratch).expect("a frame");
                            std::hint::black_box(&frame);
                            ring_write(writer, &mut channel, &(scratch.len() as u64).to_ne_bytes());
                        } else {
                            ring_write(writer, &mut channel, &scratch);
                        }
                    }
                }
                Cmd::RingSink { count, spin, frames } => {
                    let (reader, _) = rings.as_mut().expect("rings");
                    for _ in 0..count {
                        ring_read(reader, &mut channel, spin, &mut scratch);
                        if frames {
                            let frame: protocol::viz::CompositorFrame = postcard::from_bytes(&scratch).expect("a frame");
                            std::hint::black_box(&frame);
                        } else {
                            std::hint::black_box(&scratch);
                        }
                    }
                    reply(&mut channel, &Reply::Done);
                }
                Cmd::RingPipe { bytes, spin } => {
                    let (reader, _) = rings.as_mut().expect("rings");
                    let mut total = 0;
                    while total < bytes {
                        scratch.clear();
                        total += ring_read_bytes(reader, &mut channel, spin, &mut scratch, 64 * 1024);
                        std::hint::black_box(&scratch);
                    }
                    reply(&mut channel, &Reply::Done);
                }
                Cmd::RaiseQueueLimit => {
                    channel.raise_queue_limit();
                    reply(&mut channel, &Reply::Done);
                }
                Cmd::FetchAgain => {
                    let again = format!("{:?}", rendezvous::fetch(host_pid).map(|ports| ports.len()));
                    reply(&mut channel, &Reply::FetchedAgain(again));
                }
                Cmd::Exit => std::process::exit(0),
            },
            other => panic!("child {kind}: destination {other}"),
        }
    }
}

/// Waits for the writer's wake on the control channel.
fn sleep_until_woken(channel: &mut Channel) {
    let received = channel.recv(None).expect("a wake");
    assert_eq!(received.destination, D_WAKE, "something else while a ring's reader sleeps");
}

/// Reads one message from the ring, looking `spin` times before it sleeps
/// on the control channel (`u32::MAX`: never sleeps).
fn ring_read(reader: &mut ring::RingReader, channel: &mut Channel, spin: u32, into: &mut Vec<u8>) {
    loop {
        let mut spins = 0;
        loop {
            if reader.read(into).expect("a sound ring") {
                return;
            }
            if spins == spin {
                break;
            }
            spins = spins.saturating_add(1);
            std::hint::spin_loop();
        }
        if spin != u32::MAX && reader.prepare_sleep() {
            sleep_until_woken(channel);
        }
    }
}

fn ring_read_bytes(
    reader: &mut ring::RingReader,
    channel: &mut Channel,
    spin: u32,
    into: &mut Vec<u8>,
    max: usize,
) -> usize {
    loop {
        let mut spins = 0;
        loop {
            let read = reader.read_bytes(into, max).expect("a sound ring");
            if read > 0 {
                return read;
            }
            if spins == spin {
                break;
            }
            spins = spins.saturating_add(1);
            std::hint::spin_loop();
        }
        if spin != u32::MAX && reader.prepare_sleep() {
            sleep_until_woken(channel);
        }
    }
}

/// Writes one message to the ring, waiting (spinning) while it is full,
/// and wakes the reader if it sleeps.
fn ring_write(writer: &mut ring::RingWriter, channel: &mut Channel, payload: &[u8]) {
    loop {
        match writer.write(payload).expect("a sound ring") {
            Some(wake) => {
                if wake {
                    channel.send(D_WAKE, &[]).unwrap();
                }
                return;
            }
            None => std::hint::spin_loop(),
        }
    }
}

fn reply(channel: &mut Channel, reply: &Reply) {
    channel.send(D_REPLY, &postcard::to_allocvec(reply).unwrap()).unwrap();
}

// ----------------------------------------------------------------- host

struct Host {
    rendezvous: rendezvous::Server,
}

struct Peer {
    process: Child,
    channel: Channel,
}

impl Host {
    fn spawn(&self, kind: &str, extra: &[&str]) -> Peer {
        let pair = ChannelPair::new();
        let process = Command::new(std::env::current_exe().unwrap())
            .arg(format!("--type={kind}"))
            .args(extra)
            .spawn()
            .expect("the child starts");
        self.rendezvous.register(
            process.id(),
            vec![
                (pair.their_receive, MACH_MSG_TYPE_MOVE_RECEIVE),
                (pair.our_receive, MACH_MSG_TYPE_MAKE_SEND),
            ],
        );
        let mut channel = Channel::new(pair.our_receive, pair.our_send, true, kind != "renderer");
        channel.expect_peer(process.id().cast_signed());
        Peer { process, channel }
    }
}

impl Peer {
    fn command(&mut self, cmd: &Cmd) {
        self.command_with(cmd, &[]);
    }

    fn command_with(&mut self, cmd: &Cmd, ports: &[(mach2::port::mach_port_t, u32)]) {
        self.channel
            .send_with(D_CMD, &postcard::to_allocvec(cmd).unwrap(), ports, None, None)
            .unwrap();
    }

    fn reply(&mut self) -> Reply {
        loop {
            let received = self.channel.recv(Some(60_000)).expect("a reply");
            // A wake that crossed a reader that didn't sleep after all.
            if received.destination == D_WAKE {
                continue;
            }
            assert_eq!(received.destination, D_REPLY);
            return postcard::from_bytes(&received.payload).expect("a reply");
        }
    }
}

fn host_main() {
    let host = Host {
        rendezvous: rendezvous::Server::start().expect("the rendezvous server checks in"),
    };
    println!("host {}: rendezvous {}", std::process::id(), rendezvous::name_for(std::process::id()));
    three_processes(&host);
    a_child_dies(&host);
    channel_timings(&host);
    ring_timings(&host);
    frame_timings(&host);
    crossbeam_timings();
    asleep_timings(&host);
}

/// A round trip when the other side has gone to sleep waiting, as it has
/// between a page's messages: each run comes a millisecond after the one
/// before (the wait isn't timed).
fn asleep_timings(host: &Host) {
    println!("\n== a round trip of 64 bytes to a receiver that has gone to sleep ==");
    let pause = Duration::from_millis(1);
    let payload = vec![7u8; 64];

    let mut renderer = host.spawn("renderer", &["--sandbox"]);
    renderer.command(&Cmd::Hello);
    renderer.reply();
    let mut measure = |what: &str, run: &mut dyn FnMut()| {
        let mut times: Vec<Duration> = (0..3_100)
            .map(|_| {
                std::thread::sleep(pause);
                let start = Instant::now();
                run();
                start.elapsed()
            })
            .skip(100)
            .collect();
        times.sort_unstable();
        println!("{what} {}", Times(times));
    };
    let (to_renderer, from_browser) = crossbeam_channel::unbounded::<Vec<u8>>();
    let (to_browser, from_renderer) = crossbeam_channel::unbounded::<Vec<u8>>();
    let thread = std::thread::spawn(move || {
        for message in from_browser {
            to_browser.send(message).unwrap();
        }
    });
    let mut moved = Some(payload.clone());
    measure("crossbeam, two threads:           ", &mut || {
        to_renderer.send(moved.take().unwrap()).unwrap();
        moved = Some(from_renderer.recv().unwrap());
    });
    drop(to_renderer);
    thread.join().unwrap();
    measure("Mach messages, two processes:     ", &mut || {
        renderer.channel.send(D_ECHO, &payload).unwrap();
        renderer.channel.recv(Some(1000)).unwrap();
    });
    renderer.command(&Cmd::Exit);
    let _ = renderer.process.wait();
}

/// What the single-process mode sends between a renderer's thread and the
/// browser's: the message itself, moved, with nothing encoded or copied.
enum Threaded {
    Echo(Vec<u8>),
    Drop([u8; 64]),
    Frame(Box<protocol::viz::CompositorFrame>),
    Bytes(std::sync::Arc<[u8]>, std::ops::Range<usize>),
    Hello,
}

/// The same measurements between two threads of one process over
/// crossbeam channels, as machopo's single-process mode is.
fn crossbeam_timings() {
    println!("\n== one process: two threads and crossbeam channels ==");
    let (to_renderer, from_browser) = crossbeam_channel::unbounded::<Threaded>();
    let (to_browser, from_renderer) = crossbeam_channel::unbounded::<Threaded>();
    let renderer = std::thread::Builder::new()
        .name("renderer".into())
        .spawn(move || {
            for message in from_browser {
                match message {
                    Threaded::Echo(_) | Threaded::Frame(_) | Threaded::Hello => {
                        to_browser.send(std::hint::black_box(message)).unwrap();
                    }
                    Threaded::Drop(bytes) => {
                        std::hint::black_box(bytes);
                    }
                    Threaded::Bytes(bytes, range) => {
                        std::hint::black_box(&bytes[range]);
                    }
                }
            }
        })
        .unwrap();

    for size in [64, 16 * 1024] {
        let mut payload = Some(vec![7u8; size]);
        let times = Times::measure(20_000, 2_000, || {
            to_renderer.send(Threaded::Echo(payload.take().unwrap())).unwrap();
            let Threaded::Echo(back) = from_renderer.recv().unwrap() else { panic!() };
            payload = Some(back);
        });
        println!("round trip, {size:>5} bytes (the buffer moved):             {times}");
    }

    let count = 10_000;
    let times = Times::measure(30, 3, || {
        for _ in 0..count {
            to_renderer.send(Threaded::Drop([7; 64])).unwrap();
        }
        to_renderer.send(Threaded::Hello).unwrap();
        from_renderer.recv().unwrap();
    });
    println!("{count} messages of 64 bytes one way:                       {times}");

    for (name, frame) in frames() {
        let mut frame = Some(Box::new(frame));
        let times = Times::measure(5_000, 500, || {
            to_renderer.send(Threaded::Frame(frame.take().unwrap())).unwrap();
            let Threaded::Frame(back) = from_renderer.recv().unwrap() else { panic!() };
            frame = Some(back);
        });
        println!("frame {name:<20} there and back (moved):         {times}");
    }

    for size in [64 << 10, 1 << 20, 8 << 20] {
        let data: std::sync::Arc<[u8]> = pattern(3, size).into();
        let times = Times::measure(500, 20, || {
            to_renderer.send(Threaded::Bytes(data.clone(), 0..size)).unwrap();
            to_renderer.send(Threaded::Hello).unwrap();
            from_renderer.recv().unwrap();
        });
        println!("{:>7} to the renderer (shared, not copied), and its answer: {times}", size_name(size));
    }

    let body: std::sync::Arc<[u8]> = pattern(5, 16 << 20).into();
    let times = Times::measure(30, 3, || {
        for start in (0..body.len()).step_by(64 * 1024) {
            to_renderer.send(Threaded::Bytes(body.clone(), start..start + 64 * 1024)).unwrap();
        }
        to_renderer.send(Threaded::Hello).unwrap();
        from_renderer.recv().unwrap();
    });
    println!("16 MiB as a pipe, 64 KiB pieces (shared, not copied):          {times}");
    drop(to_renderer);
    renderer.join().unwrap();
}

/// The frames machopo's compositor submitted for real pages (`frames/`,
/// captured 2026-11-20 at 1280x800: a long book, a documentation page, an
/// app, `about:demo`), and the largest with its quads and resources
/// repeated, for what a much heavier page would send.
fn frames() -> Vec<(String, protocol::viz::CompositorFrame)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("frames");
    let mut frames: Vec<(String, protocol::viz::CompositorFrame)> = ["15-quads", "21-quads", "30-quads", "46-quads"]
        .iter()
        .map(|name| {
            let json = std::fs::read(dir.join(format!("{name}.json"))).expect("a captured frame");
            ((*name).to_owned(), serde_json::from_slice(&json).expect("a frame"))
        })
        .collect();
    let largest = frames.last().unwrap().1.clone();
    for times in [25, 100] {
        let mut frame = largest.clone();
        let pass = &mut frame.render_pass_list[0];
        let (quads, states) = (pass.quad_list.clone(), pass.shared_quad_state_list.clone());
        for _ in 1..times {
            pass.quad_list.extend_from_slice(&quads);
            pass.shared_quad_state_list.extend_from_slice(&states);
            frame.resource_list.extend(largest.resource_list.iter().cloned());
        }
        frames.push((format!("46 x {times} (made up)"), frame));
    }
    frames
}

fn frame_timings(host: &Host) {
    println!("\n== frames: postcard, and to the renderer until it has decoded one and said so ==");
    let mut renderer = host.spawn("renderer", &["--sandbox"]);
    renderer.command(&Cmd::Hello);
    renderer.reply();
    let ring_len = (4 << 20) + 16384;
    let (to_renderer, len) = shm::create(ring_len);
    let (from_renderer, _) = shm::create(ring_len);
    let mut writer = ring::RingWriter::new(shm::map(to_renderer, len, true).unwrap());
    let mut reader = ring::RingReader::new(shm::map(from_renderer, len, true).unwrap());
    renderer.command_with(&Cmd::Rings { len }, &[(to_renderer, msg::COPY_SEND), (from_renderer, msg::COPY_SEND)]);
    assert!(matches!(renderer.reply(), Reply::Done));
    let mut scratch = Vec::new();
    for (name, frame) in frames() {
        let pass = &frame.render_pass_list[0];
        let encoded = postcard::to_allocvec(&frame).unwrap();
        println!(
            "{name}: {} quads, {} quad states, {} resources; {} bytes encoded ({} as JSON)",
            pass.quad_list.len(),
            pass.shared_quad_state_list.len(),
            frame.resource_list.len(),
            encoded.len(),
            serde_json::to_vec(&frame).unwrap().len(),
        );
        let (runs, warm_up) = (5_000u32, 500u32);
        let times = Times::measure(runs as usize, warm_up as usize, || {
            std::hint::black_box(postcard::to_allocvec(std::hint::black_box(&frame)).unwrap());
        });
        println!("  encode:                               {times}");
        let times = Times::measure(runs as usize, warm_up as usize, || {
            let frame: protocol::viz::CompositorFrame = postcard::from_bytes(std::hint::black_box(&encoded)).unwrap();
            std::hint::black_box(frame);
        });
        println!("  decode:                               {times}");
        let times = Times::measure(runs as usize, warm_up as usize, || {
            let encoded = postcard::to_allocvec(&frame).unwrap();
            renderer.channel.send(D_FRAME, &encoded).unwrap();
            while renderer.channel.recv(Some(1000)).unwrap().destination == D_WAKE {}
        });
        println!("  encode, Mach message, decode, ack:    {times}");
        for (spin, what) in [(SPIN_A_WHILE, "looks a while"), (0, "sleeps at once")] {
            renderer.command(&Cmd::RingEcho { count: runs + warm_up, spin, frames: true });
            let channel = &mut renderer.channel;
            let times = Times::measure(runs as usize, warm_up as usize, || {
                let encoded = postcard::to_allocvec(&frame).unwrap();
                ring_write(&mut writer, channel, &encoded);
                ring_read(&mut reader, channel, spin, &mut scratch);
            });
            println!("  the same by ring, {what:<14}:     {times}");
        }
    }
    renderer.command(&Cmd::Exit);
    let _ = renderer.process.wait();
}

/// How long a reader looks at an empty ring before it sleeps, in the
/// "looks a while" runs: about 150 µs on this machine (measured below).
const SPIN_A_WHILE: u32 = 20_000;

/// The second step's ring against the control channel: small messages
/// there and back and in a burst, and a body's bytes as a pipe.
fn ring_timings(host: &Host) {
    println!("\n== a ring in shared memory against Mach messages ==");
    let mut renderer = host.spawn("renderer", &["--sandbox"]);
    renderer.command(&Cmd::Hello);
    renderer.reply();
    let ring_len = (1 << 20) + 16384;
    let (to_renderer, len) = shm::create(ring_len);
    let (from_renderer, _) = shm::create(ring_len);
    let mut writer = ring::RingWriter::new(shm::map(to_renderer, len, true).unwrap());
    let mut reader = ring::RingReader::new(shm::map(from_renderer, len, true).unwrap());
    renderer.command_with(&Cmd::Rings { len }, &[(to_renderer, msg::COPY_SEND), (from_renderer, msg::COPY_SEND)]);
    assert!(matches!(renderer.reply(), Reply::Done));
    let mut scratch = Vec::new();
    let looking = Times::measure(200, 20, || {
        for _ in 0..SPIN_A_WHILE {
            assert!(!reader.read(&mut scratch).unwrap());
            std::hint::spin_loop();
        }
    });
    println!("looking at an empty ring {SPIN_A_WHILE} times:                          {looking}");

    for size in [64, 16 * 1024] {
        let payload = vec![7u8; size];
        let (runs, warm_up) = (20_000u32, 2_000u32);
        let times = Times::measure(runs as usize, warm_up as usize, || {
            renderer.channel.send(D_ECHO, &payload).unwrap();
            // A wake may be left from a ring's reader that didn't sleep
            // after all.
            while renderer.channel.recv(Some(1000)).unwrap().destination == D_WAKE {}
        });
        println!("round trip, {size:>5} bytes, Mach messages:                     {times}");
        for (spin, what) in [
            (u32::MAX, "ring, both ends never sleep:       "),
            (SPIN_A_WHILE, "ring, ends look a while then sleep:"),
            (0, "ring, ends sleep at once:          "),
        ] {
            renderer.command(&Cmd::RingEcho { count: runs + warm_up, spin, frames: false });
            let channel = &mut renderer.channel;
            let times = Times::measure(runs as usize, warm_up as usize, || {
                ring_write(&mut writer, channel, &payload);
                ring_read(&mut reader, channel, spin, &mut scratch);
            });
            println!("round trip, {size:>5} bytes, {what} {times}");
        }
    }

    // A burst one way: until the renderer has taken the last.
    let (count, payload) = (10_000u32, vec![7u8; 64]);
    let burst_by_mach = |renderer: &mut Peer| {
        Times::measure(30, 3, || {
            for _ in 0..count {
                renderer.channel.send(D_DROP, &payload).unwrap();
            }
            renderer.command(&Cmd::Hello);
            renderer.reply();
        })
    };
    println!("{count} messages of 64 bytes one way, Mach, 5 may wait:        {}", burst_by_mach(&mut renderer));
    renderer.command(&Cmd::RaiseQueueLimit);
    renderer.reply();
    println!("{count} messages of 64 bytes one way, Mach, 1024 may wait:     {}", burst_by_mach(&mut renderer));
    for (spin, what) in [(SPIN_A_WHILE, "looks a while"), (0, "sleeps at once")] {
        let times = Times::measure(30, 3, || {
            renderer.command(&Cmd::RingSink { count, spin, frames: false });
            for _ in 0..count {
                ring_write(&mut writer, &mut renderer.channel, &payload);
            }
            renderer.reply();
        });
        println!("{count} messages of 64 bytes one way, ring, reader {what:<14}: {times}");
    }

    // A body as a pipe: 16 MiB, until the renderer has copied the last out.
    let body = pattern(5, 16 << 20);
    let times = Times::measure(30, 3, || {
        for chunk in body.chunks(64 * 1024) {
            renderer.channel.send(D_DROP, chunk).unwrap();
        }
        renderer.command(&Cmd::Hello);
        renderer.reply();
    });
    println!("16 MiB as a pipe, 64 KiB Mach messages:                        {times}");
    for (spin, what) in [(SPIN_A_WHILE, "looks a while"), (0, "sleeps at once")] {
        let times = Times::measure(30, 3, || {
            renderer.command(&Cmd::RingPipe { bytes: body.len(), spin });
            let mut rest = &body[..];
            while !rest.is_empty() {
                let (written, wake) = writer.write_bytes(rest).expect("a sound ring");
                if wake {
                    renderer.channel.send(D_WAKE, &[]).unwrap();
                }
                if written == 0 {
                    std::hint::spin_loop();
                }
                rest = &rest[written..];
            }
            renderer.reply();
        });
        println!("16 MiB as a pipe, a 1 MiB ring, reader {what:<14}:          {times}");
    }
    renderer.command(&Cmd::Exit);
    let _ = renderer.process.wait();
}

/// Sorted times: the least, the median, the 99th percentile and the mean.
struct Times(Vec<Duration>);

impl Times {
    fn measure(runs: usize, warm_up: usize, mut run: impl FnMut()) -> Self {
        for _ in 0..warm_up {
            run();
        }
        let mut times: Vec<Duration> = (0..runs)
            .map(|_| {
                let start = Instant::now();
                run();
                start.elapsed()
            })
            .collect();
        times.sort_unstable();
        Self(times)
    }

    fn micros(duration: Duration) -> String {
        format!("{:.2}", duration.as_secs_f64() * 1e6)
    }
}

impl std::fmt::Display for Times {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let times = &self.0;
        let mean = times.iter().sum::<Duration>() / u32::try_from(times.len()).unwrap();
        write!(
            f,
            "min {:>8} median {:>8} p99 {:>8} mean {:>8} µs",
            Self::micros(times[0]),
            Self::micros(times[times.len() / 2]),
            Self::micros(times[times.len() * 99 / 100]),
            Self::micros(mean),
        )
    }
}

fn size_name(bytes: usize) -> String {
    if bytes >= 1 << 20 { format!("{} MiB", bytes >> 20) } else { format!("{} KiB", bytes >> 10) }
}

fn await_ack(channel: &mut Channel, expected: usize) {
    let received = channel.recv(Some(10_000)).expect("an ack");
    assert_eq!(received.destination, D_ACK);
    assert_eq!(u64::from_ne_bytes(received.payload[..8].try_into().unwrap()) as usize, expected);
}

/// The control channel's cost: a small message there and back, and a large
/// piece of data sent each way the design could (in the message, out of
/// line, in a region kept, in a region made for it).
fn channel_timings(host: &Host) {
    println!("\n== the control channel (Mach messages), host <-> sandboxed renderer ==");
    let mut renderer = host.spawn("renderer", &["--sandbox"]);
    renderer.command(&Cmd::Hello);
    renderer.reply();
    let channel = &mut renderer.channel;
    for size in [16, 64, 1024, 16 * 1024] {
        let payload = vec![7u8; size];
        let times = Times::measure(20_000, 2_000, || {
            channel.send(D_ECHO, &payload).unwrap();
            channel.recv(Some(1000)).unwrap();
        });
        println!("round trip, {size:>6} bytes each way:        {times}");
    }

    // A region kept for large data, as a renderer's pool would be.
    let (entry, region_len) = shm::create(8 << 20);
    let ours = shm::map(entry, region_len, true).unwrap();
    let command = postcard::to_allocvec(&Cmd::Region { number: 9, len: region_len, writable: false }).unwrap();
    channel.send_with(D_CMD, &command, &[(shm::read_only_entry(&ours), msg::COPY_SEND)], None, None).unwrap();
    channel.recv(Some(1000)).unwrap();

    println!("large data to the renderer, until it has copied it into its own memory and said so:");
    for size in [64 << 10, 1 << 20, 8 << 20] {
        let data = pattern(3, size);
        let runs = if size >= 8 << 20 { 100 } else { 500 };
        let name = size_name(size);
        let times = Times::measure(runs, 20, || {
            channel.send(D_SINK, &data).unwrap();
            await_ack(channel, size);
        });
        println!("  {name:>7} in the message:              {times}");
        let times = Times::measure(runs, 20, || {
            channel.send_with(D_SINK, &[], &[], Some(&data), None).unwrap();
            await_ack(channel, size);
        });
        println!("  {name:>7} out of line:                 {times}");
        let reference = postcard::to_allocvec(&(9u32, 0u32, u32::try_from(size).unwrap())).unwrap();
        let times = Times::measure(runs, 20, || {
            ours.copy_in(0, &data);
            channel.send(D_REGION, &reference).unwrap();
            await_ack(channel, size);
        });
        println!("  {name:>7} in a region kept:            {times}");
        let times = Times::measure(runs, 20, || {
            let (entry, len) = shm::create(size);
            let mapping = shm::map(entry, len, true).unwrap();
            mapping.copy_in(0, &data);
            let read_only = shm::read_only_entry(&mapping);
            channel
                .send_with(D_FRESH, &(size as u64).to_ne_bytes(), &[(read_only, msg::COPY_SEND)], None, None)
                .unwrap();
            await_ack(channel, size);
            msg::deallocate(read_only);
            msg::deallocate(entry);
        });
        println!("  {name:>7} in a region made for it:     {times}");
    }
    renderer.command(&Cmd::Exit);
    let _ = renderer.process.wait();
}

/// The host, a sandboxed renderer and a second kind of child: each child's
/// channel to the host, a direct channel between the children, and a
/// region the host made that one writes and the other reads by number.
fn three_processes(host: &Host) {
    println!("\n== three processes ==");
    let mut plain = host.spawn("renderer", &[]);
    plain.command(&Cmd::Probe);
    let Reply::Probe(outside) = plain.reply() else { panic!() };
    plain.command(&Cmd::Exit);
    let _ = plain.process.wait();

    let mut renderer = host.spawn("renderer", &["--sandbox"]);
    renderer.command(&Cmd::Probe);
    let Reply::Probe(inside) = renderer.reply() else { panic!() };
    println!("{:<28} {:<34} {}", "", "no sandbox", "sandbox");
    for ((what, outside), (_, inside)) in outside.iter().zip(&inside) {
        println!("{what:<28} {outside:<34} {inside}");
    }

    let mut gpu = host.spawn("gpu", &[]);
    // The direct channel: both ports made here, each end handed over.
    let (to_renderer, to_gpu) = (msg::allocate_receive(), msg::allocate_receive());
    msg::insert_send(to_renderer);
    msg::insert_send(to_gpu);
    renderer.command_with(
        &Cmd::PeerChannel { peer_pid: gpu.process.id().cast_signed(), may_attach: false, peer_may_attach: true },
        &[(to_renderer, MACH_MSG_TYPE_MOVE_RECEIVE), (to_gpu, msg::COPY_SEND)],
    );
    gpu.command_with(
        &Cmd::PeerChannel { peer_pid: renderer.process.id().cast_signed(), may_attach: true, peer_may_attach: false },
        &[(to_gpu, MACH_MSG_TYPE_MOVE_RECEIVE), (to_renderer, msg::COPY_SEND)],
    );
    msg::deallocate(to_renderer);
    msg::deallocate(to_gpu);
    assert!(matches!(renderer.reply(), Reply::Done));
    assert!(matches!(gpu.reply(), Reply::Done));

    // A region the host makes: writable for the renderer, read-only for
    // the other child, under the same number.
    let (entry, len) = shm::create(1 << 20);
    let ours = shm::map(entry, len, true).unwrap();
    let read_only = shm::read_only_entry(&ours);
    renderer.command_with(&Cmd::Region { number: 1, len, writable: true }, &[(entry, msg::COPY_SEND)]);
    assert!(matches!(renderer.reply(), Reply::Done));
    gpu.command_with(&Cmd::PeerRegion { number: 1, len }, &[(read_only, msg::COPY_SEND)]);
    let Reply::PeerRegion { writable_map } = gpu.reply() else { panic!() };
    println!("region of {len} bytes; mapping the read-only entry writable: kern_return {writable_map:#x}");
    msg::deallocate(entry);
    msg::deallocate(read_only);

    let cases = [
        (None, "a good reference"),
        (Some(Misbehave::UnknownRegion), "an unknown region"),
        (Some(Misbehave::OutOfRange), "a range past the region"),
        (Some(Misbehave::AttachRight), "a right attached"),
        (Some(Misbehave::ForgeNotification), "a forged notification, then a good reference"),
    ];
    for (seed, (misbehave, what)) in cases.into_iter().enumerate() {
        let seed = seed as u8 + 1;
        let (offset, len) = (4096 * u32::from(seed), 300_000);
        renderer.command(&Cmd::WriteAndTell { number: 1, offset, len, seed, misbehave });
        assert!(matches!(renderer.reply(), Reply::Done));
        gpu.command(&Cmd::ServeDirect);
        let Reply::Direct { outcome, checksum: theirs, forged_notifications } = gpu.reply() else { panic!() };
        let expected = checksum(&pattern(seed, len as usize));
        let pixels = if outcome == "ok" {
            if theirs == expected { ", the bytes match" } else { ", THE BYTES DIFFER" }
        } else {
            ""
        };
        println!("{what}: {outcome}{pixels} (forged notifications ignored so far: {forged_notifications})");
    }
    renderer.command(&Cmd::FetchAgain);
    println!("the renderer asks the rendezvous a second time: {:?}", renderer.reply());
    println!(
        "a process with nothing registered asks: {:?}",
        rendezvous::fetch(std::process::id()).map(|ports| ports.len())
    );
    println!("rendezvous requests refused: {:?}", host.rendezvous.refused());
    renderer.command(&Cmd::Exit);
    gpu.command(&Cmd::Exit);
    let _ = (renderer.process.wait(), gpu.process.wait());
}

fn a_child_dies(host: &Host) {
    println!("\n== a child dies ==");
    let mut renderer = host.spawn("renderer", &["--sandbox"]);
    renderer.command(&Cmd::Hello);
    println!("renderer says {:?}", renderer.reply());
    let start = Instant::now();
    renderer.channel.send(D_ECHO, b"ping").unwrap();
    let echoed = renderer.channel.recv(Some(1000)).unwrap();
    println!("echo {:?} in {:?}", String::from_utf8_lossy(&echoed.payload), start.elapsed());
    renderer.process.kill().unwrap();
    let start = Instant::now();
    let gone = renderer.channel.recv(Some(5000));
    println!("after kill: {:?} in {:?}", gone.err(), start.elapsed());
    let _ = renderer.process.wait();
    std::thread::sleep(Duration::from_millis(10));
}
