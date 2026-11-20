//! The prototype's channel against two crates that do the same job:
//! `ipc-channel` (Servo's; Mach ports on macOS) and `interprocess` (a
//! local socket, here a Unix domain socket). Each carries the same
//! messages between the host and a child that serves them the same way,
//! so what differs is the transport.

use std::io::{BufReader, Read, Write};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use interprocess::local_socket::traits::{Listener as _, Stream as _};
#[allow(deprecated, reason = "the halves are what a `BufReader` can own")]
use interprocess::local_socket::{GenericFilePath, ListenerOptions, RecvHalf, SendHalf, Stream, ToFsName};
use ipc_channel::ipc::{
    self, IpcBytesReceiver, IpcBytesSender, IpcOneShotServer, IpcReceiver, IpcSender, IpcSharedMemory,
};
use protocol::viz::CompositorFrame;

use super::{Channel, ChannelError, D_ECHO, Host, Times, size_name};

/// The first byte of a message on a link: what the child does with it.
/// Send the message back as it is.
const OP_ECHO: u8 = 0;
/// Take the bytes in; answer their length.
const OP_SINK: u8 = 1;
/// Decode the rest as a frame; answer its length.
const OP_FRAME: u8 = 2;
/// End.
const OP_EXIT: u8 = 3;
/// `ipc-channel` only: take as many frames as the next four bytes say from
/// the typed channel, answering each.
const OP_TYPED_FRAMES: u8 = 4;
/// `ipc-channel` only: take as many shared memories as the next four bytes
/// say, copy each into own memory, answering its length.
const OP_SHARED_MEMORIES: u8 = 5;

/// One end of a transport that carries whole messages.
trait Link {
    fn send(&mut self, message: &[u8]);
    /// The next message, or `false` once the other end is gone.
    fn recv(&mut self, into: &mut Vec<u8>) -> bool;
}

/// The prototype's control channel.
struct MachLink<'a>(&'a mut Channel);

impl Link for MachLink<'_> {
    fn send(&mut self, message: &[u8]) {
        self.0.send(D_ECHO, message).unwrap();
    }

    fn recv(&mut self, into: &mut Vec<u8>) -> bool {
        match self.0.recv(None) {
            Ok(received) => {
                *into = received.payload;
                true
            }
            Err(ChannelError::Disconnected) => false,
            Err(error) => panic!("the Mach link: {error:?}"),
        }
    }
}

/// `ipc-channel`'s byte channels, one each way.
struct IpcChannelLink {
    send: IpcBytesSender,
    recv: IpcBytesReceiver,
}

impl Link for IpcChannelLink {
    fn send(&mut self, message: &[u8]) {
        self.send.send(message).unwrap();
    }

    fn recv(&mut self, into: &mut Vec<u8>) -> bool {
        match self.recv.recv() {
            Ok(message) => {
                *into = message;
                true
            }
            Err(_) => false,
        }
    }
}

/// A local socket, each message after its length. Reads go through a
/// buffer, so a small message's length and bytes are one `read`; a small
/// message is written with its length in one `write`.
#[allow(deprecated, reason = "the halves are what a `BufReader` can own")]
struct SocketLink {
    send: SendHalf,
    recv: BufReader<RecvHalf>,
    out: Vec<u8>,
}

impl SocketLink {
    /// `buffer`: the socket's send and receive buffers, if not the
    /// system's (8 KiB each on macOS).
    #[allow(deprecated, reason = "the halves are what a `BufReader` can own")]
    fn new(stream: Stream, buffer: Option<usize>) -> Self {
        if let Some(bytes) = buffer {
            use std::os::fd::{AsFd, AsRawFd};
            let Stream::UdSocket(socket) = &stream;
            let fd = socket.as_fd().as_raw_fd();
            let bytes = libc::c_int::try_from(bytes).unwrap();
            for option in [libc::SO_SNDBUF, libc::SO_RCVBUF] {
                // SAFETY: a call with an open socket and an int of the
                // size it is told.
                let failed = unsafe {
                    libc::setsockopt(fd, libc::SOL_SOCKET, option, (&raw const bytes).cast(), size_of::<libc::c_int>() as u32)
                };
                assert_eq!(failed, 0, "setsockopt: {}", std::io::Error::last_os_error());
            }
        }
        let (recv, send) = stream.split();
        Self { send, recv: BufReader::with_capacity(64 << 10, recv), out: Vec::new() }
    }
}

impl Link for SocketLink {
    fn send(&mut self, message: &[u8]) {
        let len = u32::try_from(message.len()).unwrap().to_ne_bytes();
        if message.len() <= 64 << 10 {
            self.out.clear();
            self.out.extend_from_slice(&len);
            self.out.extend_from_slice(message);
            self.send.write_all(&self.out).unwrap();
        } else {
            self.send.write_all(&len).unwrap();
            self.send.write_all(message).unwrap();
        }
    }

    fn recv(&mut self, into: &mut Vec<u8>) -> bool {
        let mut len = [0u8; 4];
        if self.recv.read_exact(&mut len).is_err() {
            return false;
        }
        // Not cleared first: only what the message is longer by is zeroed.
        into.resize(u32::from_ne_bytes(len) as usize, 0);
        self.recv.read_exact(into).is_ok()
    }
}

// ---------------------------------------------------------------- child

/// What `ipc-channel`'s child hands the host once: its ends of the byte
/// channels, and of the typed ones.
type Setup = (IpcBytesSender, IpcBytesReceiver, IpcSender<CompositorFrame>, IpcSender<IpcSharedMemory>);

struct Typed {
    frames: IpcReceiver<CompositorFrame>,
    memories: IpcReceiver<IpcSharedMemory>,
}

fn serve(link: &mut dyn Link, typed: Option<&Typed>) {
    let mut message = Vec::new();
    let mut scratch = Vec::new();
    let count = |message: &[u8]| u32::from_ne_bytes(message[1..5].try_into().unwrap());
    while link.recv(&mut message) {
        match message[0] {
            OP_ECHO => link.send(&message),
            OP_SINK => {
                std::hint::black_box(&message);
                link.send(&(message.len() as u64).to_ne_bytes());
            }
            OP_FRAME => {
                let frame: CompositorFrame = postcard::from_bytes(&message[1..]).expect("a frame");
                std::hint::black_box(&frame);
                link.send(&(message.len() as u64).to_ne_bytes());
            }
            OP_TYPED_FRAMES => {
                for _ in 0..count(&message) {
                    let frame = typed.unwrap().frames.recv().expect("a frame");
                    std::hint::black_box(&frame);
                    link.send(&0u64.to_ne_bytes());
                }
            }
            OP_SHARED_MEMORIES => {
                for _ in 0..count(&message) {
                    let memory = typed.unwrap().memories.recv().expect("a shared memory");
                    scratch.clear();
                    scratch.extend_from_slice(&memory);
                    std::hint::black_box(&scratch);
                    link.send(&(scratch.len() as u64).to_ne_bytes());
                }
            }
            OP_EXIT => return,
            other => panic!("op {other}"),
        }
    }
}

/// The child of the Mach link, its channel made as any child's is.
pub fn serve_mach(channel: &mut Channel) {
    serve(&mut MachLink(channel), None);
}

/// `--type=ipc-channel <name>` and `--type=interprocess <path>`.
pub fn child_main(kind: &str, arg: &str) {
    match kind {
        "ipc-channel" => {
            let (to_child, from_host) = ipc::bytes_channel().unwrap();
            let (to_host, from_child) = ipc::bytes_channel().unwrap();
            let (frames_to_child, frames) = ipc::channel::<CompositorFrame>().unwrap();
            let (memories_to_child, memories) = ipc::channel::<IpcSharedMemory>().unwrap();
            let setup: IpcSender<Setup> = IpcSender::connect(arg.to_owned()).expect("the host's server");
            setup.send((to_child, from_child, frames_to_child, memories_to_child)).unwrap();
            serve(&mut IpcChannelLink { send: to_host, recv: from_host }, Some(&Typed { frames, memories }));
        }
        "interprocess" => {
            let name = arg.to_fs_name::<GenericFilePath>().unwrap();
            let stream = Stream::connect(name).expect("the host's socket");
            let buffer = std::env::var("MP_SOCKET_BUFFER").ok().map(|bytes| bytes.parse().unwrap());
            serve(&mut SocketLink::new(stream, buffer), None);
        }
        other => panic!("child {other}"),
    }
}

// ----------------------------------------------------------------- host

fn spawn(kind: &str, arg: &str) -> Child {
    Command::new(std::env::current_exe().unwrap())
        .arg(format!("--type={kind}"))
        .arg(arg)
        .spawn()
        .expect("the child starts")
}

fn ack(link: &mut dyn Link, scratch: &mut Vec<u8>) {
    assert!(link.recv(scratch));
    assert_eq!(scratch.len(), 8);
}

/// The same measurements over any link.
fn measure(name: &str, link: &mut dyn Link, frame: &CompositorFrame) {
    println!("\n-- {name} --");
    let mut scratch = Vec::new();
    for size in [64usize, 1024, 16 << 10] {
        let mut payload = vec![7u8; size];
        payload[0] = OP_ECHO;
        let times = Times::measure(20_000, 2_000, || {
            link.send(&payload);
            assert!(link.recv(&mut scratch));
        });
        println!("round trip, {size:>6} bytes each way:        {times}");
    }

    let mut payload = vec![7u8; 64];
    payload[0] = OP_ECHO;
    let mut times: Vec<Duration> = (0..3_100)
        .map(|_| {
            std::thread::sleep(Duration::from_millis(1));
            let start = Instant::now();
            link.send(&payload);
            assert!(link.recv(&mut scratch));
            start.elapsed()
        })
        .skip(100)
        .collect();
    times.sort_unstable();
    println!("round trip, 64 bytes, receiver asleep:    {}", Times(times));

    let mut encoded = Vec::new();
    let times = Times::measure(5_000, 500, || {
        encoded.clear();
        encoded.push(OP_FRAME);
        encoded = postcard::to_extend(frame, std::mem::take(&mut encoded)).unwrap();
        link.send(&encoded);
        ack(link, &mut scratch);
    });
    println!("a frame: encode, send, decode, ack:       {times}");

    for size in [64usize << 10, 1 << 20, 8 << 20] {
        let mut data = super::pattern(3, size);
        data[0] = OP_SINK;
        let runs = if size >= 8 << 20 { 100 } else { 500 };
        let times = Times::measure(runs, 20, || {
            link.send(&data);
            ack(link, &mut scratch);
        });
        println!("{:>7} in the message, until acked:      {times}", size_name(size));
    }
}

pub fn timings(host: &Host) {
    println!("\n== the prototype's channel against ipc-channel and interprocess ==");
    let frame = super::frames().into_iter().find(|(name, _)| name == "46-quads").unwrap().1;

    // The prototype: Mach messages, the child not sandboxed (the others
    // can't be: both look a name up that a sandbox would have to allow).
    let mut peer = host.spawn("link", &[]);
    measure("prototype (Mach messages)", &mut MachLink(&mut peer.channel), &frame);
    peer.channel.send(D_ECHO, &[OP_EXIT]).unwrap();
    let _ = peer.process.wait();

    // ipc-channel: its own rendezvous (a one-shot server by name).
    let (server, name) = IpcOneShotServer::<Setup>::new().unwrap();
    let mut child = spawn("ipc-channel", &name);
    let (_, (to_child, from_child, frames, memories)) = server.accept().unwrap();
    let mut link = IpcChannelLink { send: to_child, recv: from_child };
    measure("ipc-channel (Mach messages)", &mut link, &frame);
    let mut scratch = Vec::new();
    let (runs, warm_up) = (5_000u32, 500u32);
    let mut command = vec![OP_TYPED_FRAMES];
    command.extend_from_slice(&(runs + warm_up).to_ne_bytes());
    link.send(&command);
    let times = Times::measure(runs as usize, warm_up as usize, || {
        frames.send(frame.clone()).unwrap();
        ack(&mut link, &mut scratch);
    });
    println!("a frame by its typed channel (cloned first): {times}");
    let times = Times::measure(runs as usize, warm_up as usize, || {
        std::hint::black_box(frame.clone());
    });
    println!("  of which the clone:                     {times}");
    for size in [64usize << 10, 1 << 20, 8 << 20] {
        let data = super::pattern(3, size);
        let (runs, warm_up) = (if size >= 8 << 20 { 100u32 } else { 500 }, 20u32);
        let mut command = vec![OP_SHARED_MEMORIES];
        command.extend_from_slice(&(runs + warm_up).to_ne_bytes());
        link.send(&command);
        let times = Times::measure(runs as usize, warm_up as usize, || {
            memories.send(IpcSharedMemory::from_bytes(&data)).unwrap();
            ack(&mut link, &mut scratch);
        });
        println!("{:>7} as an IpcSharedMemory, until copied out and acked: {times}", size_name(size));
    }
    link.send(&[OP_EXIT]);
    let _ = child.wait();

    // interprocess: a local socket at a path, with the system's buffers
    // and with larger ones.
    for buffer in [None, Some(1usize << 20)] {
        let path = std::env::temp_dir().join(format!("mp-prototype-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let listener = ListenerOptions::new()
            .name(path.as_os_str().to_fs_name::<GenericFilePath>().unwrap())
            .create_sync()
            .expect("a socket to listen on");
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.arg("--type=interprocess").arg(&path);
        if let Some(bytes) = buffer {
            command.env("MP_SOCKET_BUFFER", bytes.to_string());
        }
        let mut child = command.spawn().expect("the child starts");
        let mut link = SocketLink::new(listener.accept().expect("the child connects"), buffer);
        let name = match buffer {
            None => "interprocess (Unix domain socket)".to_owned(),
            Some(bytes) => format!("interprocess (Unix domain socket, {} buffers)", size_name(bytes)),
        };
        measure(&name, &mut link, &frame);
        link.send(&[OP_EXIT]);
        let _ = child.wait();
        let _ = std::fs::remove_file(&path);
    }
}
