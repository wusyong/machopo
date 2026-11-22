//! A channel's end over a Unix stream socket (Chromium's `ChannelPosix`):
//! the core's messages are bytes in a stream, written as far as the
//! socket takes them and read as they come, and the core cuts the
//! messages out of them. Handles are descriptors (a region's memfd), sent
//! with the bytes they belong to (`SCM_RIGHTS`).
//!
//! The IO thread waits with one epoll on all its channels' sockets and on
//! an eventfd other threads wake it by. Level-triggered: each wait reads
//! one socket once, and what is left is told of again. "Writable" is
//! asked for only once a write was refused, by the thread it was refused
//! to, and no longer once it came: a socket has room nearly always.
//!
//! The socket's buffers are the kernel's own: a larger one changes
//! nothing that was measured, and how large one may be asked for
//! differs between kernels.

use std::collections::VecDeque;
use std::io::{self, IoSlice, IoSliceMut};
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use rustix::event::epoll::{self, EventData, EventFlags};
use rustix::event::{EventfdFlags, eventfd};
use rustix::io::{Errno, FdFlags, fcntl_dupfd_cloexec, fcntl_setfd};
use rustix::net::sockopt::{socket_domain, socket_type};
use rustix::net::{
    AddressFamily, RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, ReturnFlags,
    SendAncillaryBuffer, SendAncillaryMessage, SendFlags, Shutdown, SocketType, recvmsg, sendmsg,
    shutdown,
};

use super::memory::{self, Mapping};
use crate::core::channel::{Transport, Wake, WriteError};
use crate::core::frame::MAX_HANDLES;
use crate::endpoint::{Event, Key, Platform, Waited, Waiter};

/// The most descriptors one message brings ([`MAX_HANDLES`]): what the
/// room for a message's control data is for, sending and receiving.
const HANDLES: usize = MAX_HANDLES as usize;
/// Room for a message's control data. Not what holds a message to
/// [`HANDLES`]: room is rounded up, and one or two more fit.
const CONTROL_LEN: usize = rustix::cmsg_space!(ScmRights(HANDLES));

/// How much one read takes. Past the longest message that goes in the
/// channel itself with its large bytes left in (the codec puts them in a
/// region from 64 KiB), so such a message is read at once; measured no
/// slower than 64 KiB.
const READ_LEN: usize = 256 << 10;

/// How many events one `epoll_wait` takes. They are told of one a wait,
/// so this is also how many reads an interrupt may wait behind.
const EVENTS: usize = 16;

/// The eventfd's place among the keys an epoll tells of: no channel's
/// (theirs are 32 bits).
const INTERRUPT: u64 = u64::MAX;

/// The environment variable a child finds its end's descriptor by. Not an
/// argument: a child may be a program with arguments of its own to make
/// sense of.
const INHERITED_FD: &str = "MACHOPO_IPC_FD";

/// Standard input, output and error: no end is handed over as one of
/// them.
const FIRST_FREE_FD: i32 = 3;

/// Starts `command` as a child that has the other end of a new channel,
/// which it takes with [`inherited`]. The child inherits its end's
/// descriptor under the number it has here, told by an environment
/// variable; only that child does (close-on-exec is taken off the
/// descriptor in the child alone, after the fork).
///
/// `command` is spent: what was added to it stays, and started again it
/// would hand over whatever has that number by then.
pub fn spawn(command: &mut Command) -> io::Result<(Child, Channel)> {
    let (ours, theirs) = UnixStream::pair()?;
    let mut theirs = OwnedFd::from(theirs);
    if theirs.as_raw_fd() < FIRST_FREE_FD {
        // This process was started with one of the three closed.
        theirs = fcntl_dupfd_cloexec(&theirs, FIRST_FREE_FD)?;
    }
    let number = theirs.as_raw_fd();
    command.env(INHERITED_FD, number.to_string());
    // SAFETY: the closure runs in the child between fork and exec, where
    // only what is async-signal-safe may be called (another thread may
    // have held any lock at the fork): it makes one `fcntl` system call
    // and allocates nothing (an error is made of its number alone).
    // `number` is open in the child: `theirs` is alive in this process
    // until `spawn` below has returned, so it was at the fork.
    unsafe {
        command.pre_exec(move || {
            // SAFETY: open (above), and borrowed only for this call.
            let fd = BorrowedFd::borrow_raw(number);
            Ok(fcntl_setfd(fd, FdFlags::empty())?)
        });
    }
    let child = command.spawn()?;
    // The child's end is the child's alone from here: when it goes, this
    // end reads the end of the stream.
    drop(theirs);
    Ok((child, Channel::new(OwnedFd::from(ours))))
}

/// In a process started by [`spawn`]: its end of the channel. `None`
/// when the parent left none, and from the second call on.
///
/// The variable stays in this process's environment, and what the process
/// starts sees it: taking it out is sound only with no other thread, which
/// isn't this function's to know. The number is closed in such a process
/// (the descriptor is close-on-exec again), so it finds no end there.
pub fn inherited() -> Option<Channel> {
    static TAKEN: AtomicBool = AtomicBool::new(false);
    let number: i32 = std::env::var(INHERITED_FD).ok()?.parse().ok()?;
    if number < FIRST_FREE_FD {
        return None;
    }
    // SAFETY: borrowed for the two calls below, which only ask what the
    // descriptor is; a number that isn't open is an error from each.
    let named = unsafe { BorrowedFd::borrow_raw(number) };
    if socket_domain(named).ok()? != AddressFamily::UNIX
        || socket_type(named).ok()? != SocketType::STREAM
    {
        return None;
    }
    if TAKEN.swap(true, Ordering::SeqCst) {
        return None;
    }
    // SAFETY: the descriptor `spawn` left open in this process for it to
    // take: a Unix stream socket (just asked) under the number `spawn`
    // wrote. Nothing else in this process was given the number, and it is
    // taken once (`TAKEN`), so this value alone closes it.
    let fd = unsafe { OwnedFd::from_raw_fd(number) };
    // Whatever this process starts doesn't have it.
    fcntl_setfd(&fd, FdFlags::CLOEXEC).ok()?;
    Some(Channel::new(fd))
}

/// Nothing: a child's end is the child's alone (the host's copy is closed
/// once the child is started), so the kernel tells the host when a child
/// goes, whether or not it took its end.
pub fn child_gone(_pid: u32) {}

/// The waiter an end is served by, and what the end wants of it.
struct Watch {
    /// The epoll the end's socket is in, and the key it is told of as.
    /// The descriptor is shared, not borrowed: a writer asks for room
    /// from its own thread, whenever the waiter goes.
    served: Option<(Arc<OwnedFd>, Key)>,
    /// A write was refused: the socket is watched for room.
    wants_room: bool,
}

/// What an end is, to its own value and to the waiter of the IO thread
/// that serves it.
struct End {
    socket: OwnedFd,
    shut: AtomicBool,
    /// Never held while the core is called: a write is called with the
    /// core's own lock held, and takes this one inside it.
    watch: Mutex<Watch>,
}

impl End {
    fn watch(&self) -> MutexGuard<'_, Watch> {
        // Nothing panics while holding it.
        self.watch
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// What the socket is watched for now.
    fn interest(wants_room: bool) -> EventFlags {
        if wants_room {
            EventFlags::IN | EventFlags::OUT
        } else {
            EventFlags::IN
        }
    }

    /// Whether the socket is watched for room from now, by the waiter the
    /// end is served by. An end no waiter serves yet is watched once one
    /// does.
    fn want_room(&self, wants_room: bool) {
        let mut watch = self.watch();
        watch.wants_room = wants_room;
        if let Some((epoll, key)) = &watch.served {
            // Refused only for a socket no longer in the epoll (removed
            // meanwhile): nothing waits on it.
            let _ = epoll::modify(
                epoll,
                &self.socket,
                EventData::new_u64(u64::from(*key)),
                Self::interest(wants_room),
            );
        }
    }
}

/// A channel's end.
// An `Arc`: the IO thread's waiter reaches the end too, to read its
// socket while other threads write it.
pub struct Channel(Arc<End>);

impl Channel {
    /// The end `socket` is: a Unix stream socket. Whether it blocks is
    /// nothing here: every read and write says it doesn't wait.
    fn new(socket: OwnedFd) -> Self {
        Self(Arc::new(End {
            socket,
            shut: AtomicBool::new(false),
            watch: Mutex::new(Watch {
                served: None,
                wants_room: false,
            }),
        }))
    }

    /// Both ends of a new channel in this process.
    pub fn pair() -> io::Result<(Self, Self)> {
        let (one, other) = UnixStream::pair()?;
        Ok((Self::new(one.into()), Self::new(other.into())))
    }
}

impl Transport for Channel {
    type Handle = OwnedFd;

    fn write(&self, bytes: &[u8], handles: &mut Vec<OwnedFd>) -> Result<usize, WriteError> {
        if bytes.is_empty() {
            // Descriptors go with a byte: a write of none takes none.
            return Ok(0);
        }
        // More than a message brings: one the core never makes.
        if handles.len() > HANDLES {
            return Err(WriteError::Closed);
        }
        let mut space = [MaybeUninit::uninit(); CONTROL_LEN];
        loop {
            let borrowed: Vec<BorrowedFd<'_>> = handles.iter().map(AsFd::as_fd).collect();
            let mut control = SendAncillaryBuffer::new(&mut space);
            if !borrowed.is_empty() && !control.push(SendAncillaryMessage::ScmRights(&borrowed)) {
                return Err(WriteError::Closed);
            }
            // No signal for a peer that is gone: the error says so.
            let flags = SendFlags::DONTWAIT | SendFlags::NOSIGNAL;
            match sendmsg(&self.0.socket, &[IoSlice::new(bytes)], &mut control, flags) {
                Ok(written) => {
                    // The kernel has its own references to what the
                    // descriptors are, from the message's first byte:
                    // these are closed.
                    handles.clear();
                    return Ok(written);
                }
                Err(Errno::INTR) => {}
                Err(Errno::AGAIN) => {
                    // Room is told of even if it came between the
                    // refusal and this: the epoll is level-triggered.
                    self.0.want_room(true);
                    return Err(WriteError::WouldBlock);
                }
                Err(_) => return Err(WriteError::Closed),
            }
        }
    }
}

impl Platform for Channel {
    type Memory = Mapping;
    type Waiter = EpollWaiter;

    fn map(&self, handle: OwnedFd, len: usize, writable: bool) -> Option<Mapping> {
        // The mapping keeps the memory; the descriptor is closed here.
        memory::map(handle.as_fd(), len, writable)
    }

    fn create(&self, len: usize, peer_writes: bool) -> Option<(Mapping, OwnedFd)> {
        memory::create(len, peer_writes)
    }

    fn shut(&self) {
        self.0.shut.store(true, Ordering::SeqCst);
        // Both ways: the peer reads the end of the stream, and its
        // writes are refused. Nothing to do about a refusal.
        let _ = shutdown(&self.0.socket, Shutdown::Both);
    }
}

/// What a process's IO thread waits with: an epoll of its channels'
/// sockets and of an eventfd to be woken by.
pub struct EpollWaiter {
    epoll: Arc<OwnedFd>,
    interrupt: Arc<OwnedFd>,
    /// What the last `epoll_wait` told of and no wait has yet.
    ready: VecDeque<(u64, EventFlags)>,
    /// Where a read's bytes are, lent to whoever waits.
    buffer: Vec<u8>,
    channels: Vec<(Key, Arc<End>)>,
}

impl Waiter<Channel> for EpollWaiter {
    fn new() -> Option<Self> {
        let epoll = epoll::create(epoll::CreateFlags::CLOEXEC).ok()?;
        let interrupt = eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK).ok()?;
        epoll::add(
            &epoll,
            &interrupt,
            EventData::new_u64(INTERRUPT),
            EventFlags::IN,
        )
        .ok()?;
        Some(Self {
            epoll: Arc::new(epoll),
            interrupt: Arc::new(interrupt),
            ready: VecDeque::with_capacity(EVENTS),
            buffer: vec![0; READ_LEN],
            channels: Vec::new(),
        })
    }

    fn interrupter(&self) -> Wake {
        let interrupt = self.interrupt.clone();
        Arc::new(move || {
            // The eventfd counts: however many calls, one wait reads
            // them all. Refused only when the count is at its most,
            // which is a wake asked for already.
            let _ = rustix::io::write(&*interrupt, &1u64.to_ne_bytes());
        })
    }

    fn add(&mut self, key: Key, channel: &Channel) -> bool {
        let end = &channel.0;
        if end.shut.load(Ordering::SeqCst) {
            return false;
        }
        let mut watch = end.watch();
        // One waiter an end: its socket is read by one thread.
        if watch.served.is_some() {
            return false;
        }
        // What came to the socket before now is still in it, and is
        // told of: level-triggered.
        let added = epoll::add(
            &self.epoll,
            &end.socket,
            EventData::new_u64(u64::from(key)),
            End::interest(watch.wants_room),
        )
        .is_ok();
        if added {
            watch.served = Some((self.epoll.clone(), key));
            drop(watch);
            self.channels.push((key, end.clone()));
        }
        added
    }

    fn remove(&mut self, key: Key) {
        let Some(at) = self.channels.iter().position(|(served, _)| *served == key) else {
            return;
        };
        let (_, end) = self.channels.swap_remove(at);
        end.watch().served = None;
        let _ = epoll::delete(&self.epoll, &end.socket);
        // What was told of it already and no wait has yet names a key
        // no channel has: keys aren't used again.
    }

    fn wait<R>(&mut self, then: impl FnOnce(Waited<'_, OwnedFd>) -> R) -> R {
        let mut handles = Vec::new();
        let (key, event, len) = loop {
            let Some((told, flags)) = self.ready.pop_front() else {
                // On the stack: what an epoll tells of holds a pointer's
                // worth, which would keep the waiter to one thread.
                let mut events = [MaybeUninit::<epoll::Event>::uninit(); EVENTS];
                // Interrupted by a signal, or refused: waited again.
                if let Ok((told, _)) = epoll::wait(&*self.epoll, &mut events[..], None) {
                    self.ready
                        .extend(told.iter().map(|event| (event.data.u64(), event.flags)));
                }
                continue;
            };
            if told == INTERRUPT {
                // Every wake asked for until now, as one.
                let mut count = [0; 8];
                let _ = rustix::io::read(&*self.interrupt, &mut count);
                return then(Waited::Interrupted);
            }
            let Some((key, end)) = self
                .channels
                .iter()
                .find(|(key, _)| u64::from(*key) == told)
                .map(|(key, end)| (*key, &**end))
            else {
                continue;
            };
            if flags.contains(EventFlags::OUT) {
                // Told once: the socket has room nearly always, and is
                // watched for it again when a write is next refused.
                end.want_room(false);
                // What else it was told of is told of again:
                // level-triggered.
                break (key, Event::Writable, 0);
            }
            // Anything else is read: that the peer is gone is what a
            // read says once all it sent has been read.
            let mut space = [MaybeUninit::uninit(); CONTROL_LEN];
            let mut control = RecvAncillaryBuffer::new(&mut space);
            let read = recvmsg(
                &end.socket,
                &mut [IoSliceMut::new(&mut self.buffer)],
                &mut control,
                // What comes in isn't left to whatever this process
                // starts.
                RecvFlags::DONTWAIT | RecvFlags::CMSG_CLOEXEC,
            );
            let read = match read {
                Ok(read) => read,
                // Read already (an event told of twice), or a signal.
                Err(Errno::AGAIN | Errno::INTR) => continue,
                Err(_) => break (key, Event::PeerGone, 0),
            };
            for message in control.drain() {
                if let RecvAncillaryMessage::ScmRights(descriptors) = message {
                    handles.extend(descriptors);
                }
            }
            // More than a message brings (one read has one write's, and
            // those that didn't fit the room were closed by the kernel):
            // from a peer that is none.
            if read.flags.contains(ReturnFlags::CTRUNC) || handles.len() > HANDLES {
                handles.clear();
                break (key, Event::PeerGone, 0);
            }
            if read.bytes == 0 {
                // The end of the stream.
                break (key, Event::PeerGone, 0);
            }
            break (key, Event::Read, read.bytes);
        };
        then(Waited::Channel {
            key,
            event,
            bytes: self.buffer.get(..len).unwrap_or_default(),
            handles: &mut handles,
        })
    }
}

#[cfg(test)]
mod tests {
    use rustix::io::fcntl_getfd;

    use super::*;
    use crate::core::region::Memory;

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

    /// A waiter that serves `channels`, as keys from 0.
    fn serving(channels: &[&Channel]) -> EpollWaiter {
        let mut waiter = EpollWaiter::new().unwrap();
        for (key, channel) in channels.iter().enumerate() {
            assert!(waiter.add(key as Key, channel));
        }
        waiter
    }

    /// What a wait told of: nothing of a channel's for an interrupt.
    #[derive(Debug, PartialEq)]
    enum Told {
        Interrupted,
        Channel(Key, Event, Vec<u8>, usize),
    }

    fn next(waiter: &mut EpollWaiter) -> Told {
        waiter.wait(|waited| match waited {
            Waited::Interrupted => Told::Interrupted,
            Waited::Channel {
                key,
                event,
                bytes,
                handles,
            } => Told::Channel(key, event, bytes.to_vec(), handles.len()),
        })
    }

    fn read(key: Key, bytes: &[u8]) -> Told {
        Told::Channel(key, Event::Read, bytes.to_vec(), 0)
    }

    fn told(key: Key, event: Event) -> Told {
        Told::Channel(key, event, Vec::new(), 0)
    }

    /// Nothing waits to be told of: an interrupt is all there is, twice
    /// (what an `epoll_wait` took along with the first would be told of
    /// before the second).
    fn nothing_to_tell(waiter: &mut EpollWaiter) {
        for _ in 0..2 {
            waiter.interrupter()();
            assert_eq!(next(waiter), Told::Interrupted);
        }
    }

    /// Writes until the socket takes no more: how much it took.
    fn fill(channel: &Channel) -> usize {
        let chunk = [0x5a; 4096];
        let mut written = 0;
        loop {
            match channel.write(&chunk, &mut Vec::new()) {
                Ok(taken) => written += taken,
                Err(WriteError::WouldBlock) => return written,
                Err(WriteError::Closed) => panic!("closed"),
            }
        }
    }

    /// Reads `len` bytes of what was written to the channel `key`.
    fn drain(waiter: &mut EpollWaiter, key: Key, mut len: usize) {
        while len > 0 {
            match next(waiter) {
                Told::Channel(told, Event::Read, bytes, 0) if told == key => len -= bytes.len(),
                other => panic!("{other:?}"),
            }
        }
    }

    /// How many descriptors this process has for the file `handle` is.
    /// Not asked by number: a number closed is another test's the next
    /// moment. A file is its device and its inode: another file system's
    /// (sockets') has the same inode now and then.
    fn descriptors_of(file: (u64, u64)) -> usize {
        use std::os::unix::fs::MetadataExt;
        std::fs::read_dir("/proc/self/fd")
            .unwrap()
            .filter_map(|entry| std::fs::metadata(entry.unwrap().path()).ok())
            .filter(|found| (found.dev(), found.ino()) == file)
            .count()
    }

    #[test]
    fn what_one_end_writes_the_other_reads_in_order() {
        within(|| {
            let (host, child) = Channel::pair().unwrap();
            let mut hosts = serving(&[&host]);
            let mut childs = serving(&[&child]);
            assert_eq!(host.write(b"to the child", &mut Vec::new()), Ok(12));
            assert_eq!(next(&mut childs), read(0, b"to the child"));
            assert_eq!(child.write(b"to the host", &mut Vec::new()), Ok(11));
            assert_eq!(next(&mut hosts), read(0, b"to the host"));
            // A stream: two writes may be one read, and one longer than
            // a read takes is several, the bytes the same in order.
            let long: Vec<u8> = (0..READ_LEN * 3 + 17)
                .map(|i| (i as u8).wrapping_mul(3))
                .collect();
            let (mut written, mut got) = (0, Vec::new());
            while got.len() < long.len() {
                if written < long.len() {
                    match host.write(&long[written..], &mut Vec::new()) {
                        Ok(taken) => {
                            assert!(taken > 0);
                            written += taken;
                        }
                        Err(WriteError::WouldBlock) => {}
                        Err(WriteError::Closed) => panic!("closed"),
                    }
                }
                match next(&mut childs) {
                    Told::Channel(0, Event::Read, bytes, 0) => {
                        assert!(bytes.len() <= READ_LEN);
                        got.extend(bytes);
                    }
                    other => panic!("{other:?}"),
                }
            }
            assert!(got == long);
        });
    }

    #[test]
    fn nothing_written_is_nothing_taken() {
        let (host, _child) = Channel::pair().unwrap();
        let (_mapping, handle) = memory::create(64 << 10, false).unwrap();
        let mut handles = vec![handle];
        assert_eq!(host.write(&[], &mut handles), Ok(0));
        assert_eq!(handles.len(), 1, "a handle goes with a byte");
    }

    #[test]
    fn handles_go_with_their_message_and_leave_the_writers_hands() {
        within(|| {
            let (host, child) = Channel::pair().unwrap();
            let mut childs = serving(&[&child]);
            let (mapping, handle) = memory::create(64 << 10, false).unwrap();
            let stat = rustix::fs::fstat(&handle).unwrap();
            let file = (stat.st_dev, stat.st_ino);
            assert_eq!(descriptors_of(file), 1);
            let mut handles = vec![handle];
            assert_eq!(host.write(b"with a region", &mut handles), Ok(13));
            assert!(handles.is_empty());
            // Closed here: the kernel has what the message carries.
            assert_eq!(descriptors_of(file), 0);
            let mapped = childs.wait(|waited| match waited {
                Waited::Channel {
                    key: 0,
                    event: Event::Read,
                    bytes,
                    handles,
                } => {
                    assert_eq!((bytes, handles.len()), (&b"with a region"[..], 1));
                    let handle = handles.pop().unwrap();
                    // Not left to whatever this process starts.
                    assert_eq!(fcntl_getfd(&handle), Ok(FdFlags::CLOEXEC));
                    child.map(handle, 64 << 10, false)
                }
                _ => panic!("not a read"),
            });
            // The region the handle is: what the maker writes is read
            // there.
            // SAFETY: inside both mappings; nothing else touches them.
            unsafe {
                mapping.ptr().write(0x42);
                assert_eq!(mapped.unwrap().ptr().read(), 0x42);
            }
        });
    }

    #[test]
    fn several_handles_come_in_the_order_sent() {
        within(|| {
            let (host, child) = Channel::pair().unwrap();
            let mut childs = serving(&[&child]);
            let mut handles = Vec::new();
            for len in 1..=HANDLES {
                handles.push(memory::create(len * 4096, true).unwrap().1);
            }
            assert_eq!(host.write(b"x", &mut handles), Ok(1));
            childs.wait(|waited| match waited {
                Waited::Channel { handles, .. } => {
                    let lens: Vec<i64> = handles
                        .iter()
                        .map(|handle| rustix::fs::fstat(handle).unwrap().st_size)
                        .collect();
                    let sent: Vec<i64> = (1..=HANDLES as i64).map(|len| len * 4096).collect();
                    assert_eq!(lens, sent);
                }
                Waited::Interrupted => panic!("interrupted"),
            });
        });
    }

    #[test]
    fn more_handles_than_a_message_brings_arent_sent() {
        let (host, _child) = Channel::pair().unwrap();
        let mut handles = Vec::new();
        for _ in 0..=HANDLES {
            handles.push(memory::create(4096, true).unwrap().1);
        }
        assert_eq!(host.write(b"x", &mut handles), Err(WriteError::Closed));
    }

    #[test]
    fn more_handles_than_a_message_brings_are_the_peer_gone() {
        within(|| {
            // One more than a message brings, which the room for control
            // data still holds; and far more than it holds.
            for count in [HANDLES + 1, HANDLES * 4] {
                let (host, child) = Channel::pair().unwrap();
                let mut hosts = serving(&[&host]);
                // By hand: the channel's own write sends no such message.
                let fds: Vec<OwnedFd> = (0..count)
                    .map(|_| memory::create(4096, true).unwrap().1)
                    .collect();
                let borrowed: Vec<BorrowedFd<'_>> = fds.iter().map(AsFd::as_fd).collect();
                let mut space =
                    [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(HANDLES * 4))];
                let mut control = SendAncillaryBuffer::new(&mut space);
                assert!(control.push(SendAncillaryMessage::ScmRights(&borrowed)));
                sendmsg(
                    &child.0.socket,
                    &[IoSlice::new(b"too many")],
                    &mut control,
                    SendFlags::empty(),
                )
                .unwrap();
                assert_eq!(next(&mut hosts), told(0, Event::PeerGone), "{count}");
            }
        });
    }

    #[test]
    fn room_is_told_of_once_after_a_write_was_refused_and_not_otherwise() {
        within(|| {
            let (host, child) = Channel::pair().unwrap();
            let mut hosts = serving(&[&host]);
            let mut childs = serving(&[&child]);
            // A socket with room is told of for nothing.
            assert_eq!(host.write(b"fits", &mut Vec::new()), Ok(4));
            assert_eq!(next(&mut childs), read(0, b"fits"));
            nothing_to_tell(&mut hosts);
            // Refused, and still full: nothing yet.
            let written = fill(&host);
            assert!(written > 0);
            nothing_to_tell(&mut hosts);
            // The peer reads: there is room, told once.
            drain(&mut childs, 0, written);
            assert_eq!(next(&mut hosts), told(0, Event::Writable));
            nothing_to_tell(&mut hosts);
            // And again only after the next refusal.
            assert_eq!(host.write(b"more", &mut Vec::new()), Ok(4));
            assert_eq!(next(&mut childs), read(0, b"more"));
            nothing_to_tell(&mut hosts);
            let written = fill(&host);
            drain(&mut childs, 0, written);
            assert_eq!(next(&mut hosts), told(0, Event::Writable));
        });
    }

    #[test]
    fn a_write_refused_before_a_waiter_serves_the_end_is_told_of_after() {
        within(|| {
            let (host, child) = Channel::pair().unwrap();
            let mut childs = serving(&[&child]);
            let written = fill(&host);
            let mut hosts = serving(&[&host]);
            nothing_to_tell(&mut hosts);
            drain(&mut childs, 0, written);
            assert_eq!(next(&mut hosts), told(0, Event::Writable));
        });
    }

    #[test]
    fn room_and_bytes_at_once_are_both_told_of() {
        within(|| {
            let (host, child) = Channel::pair().unwrap();
            let mut hosts = serving(&[&host]);
            let mut childs = serving(&[&child]);
            let written = fill(&host);
            assert_eq!(child.write(b"meanwhile", &mut Vec::new()), Ok(9));
            drain(&mut childs, 0, written);
            let mut events = vec![next(&mut hosts), next(&mut hosts)];
            events.sort_by_key(|event| matches!(event, Told::Channel(_, Event::Read, ..)));
            assert_eq!(events, [told(0, Event::Writable), read(0, b"meanwhile")]);
            nothing_to_tell(&mut hosts);
        });
    }

    #[test]
    fn a_peer_gone_is_told_after_what_it_sent() {
        within(|| {
            let (host, child) = Channel::pair().unwrap();
            let mut hosts = serving(&[&host]);
            assert_eq!(child.write(b"last words", &mut Vec::new()), Ok(10));
            drop(child);
            assert_eq!(next(&mut hosts), read(0, b"last words"));
            assert_eq!(next(&mut hosts), told(0, Event::PeerGone));
            // No signal ends the process: the write is refused.
            assert_eq!(
                host.write(b"anyone?", &mut Vec::new()),
                Err(WriteError::Closed)
            );
        });
    }

    #[test]
    fn an_end_shut_is_its_peer_told_and_nothing_more_written() {
        within(|| {
            let (host, child) = Channel::pair().unwrap();
            let mut childs = serving(&[&child]);
            assert_eq!(host.write(b"before", &mut Vec::new()), Ok(6));
            host.shut();
            assert_eq!(next(&mut childs), read(0, b"before"));
            assert_eq!(next(&mut childs), told(0, Event::PeerGone));
            assert_eq!(
                host.write(b"after", &mut Vec::new()),
                Err(WriteError::Closed)
            );
            assert_eq!(
                child.write(b"back", &mut Vec::new()),
                Err(WriteError::Closed)
            );
            // A shut end isn't one to wait on.
            assert!(!EpollWaiter::new().unwrap().add(0, &host));
        });
    }

    #[test]
    fn a_waiter_tells_its_channels_apart_and_a_removed_one_no_longer() {
        within(|| {
            let pairs: Vec<_> = (0..3).map(|_| Channel::pair().unwrap()).collect();
            let mut waiter = serving(&[&pairs[0].0, &pairs[1].0, &pairs[2].0]);
            for at in [2, 0, 1] {
                let bytes = [at as u8; 5];
                assert_eq!(pairs[at].1.write(&bytes, &mut Vec::new()), Ok(5));
                assert_eq!(next(&mut waiter), read(at as Key, &bytes));
            }
            // All three at once, and one removed with its bytes there
            // (told of by the epoll already, if the first wait took the
            // three together): the other two are told of, and no more.
            for (_, peer) in &pairs {
                assert_eq!(peer.write(b"all", &mut Vec::new()), Ok(3));
            }
            let mut heard = vec![next(&mut waiter)];
            waiter.remove(1);
            while !(heard.contains(&read(0, b"all")) && heard.contains(&read(2, b"all"))) {
                heard.push(next(&mut waiter));
                assert!(heard.len() <= 3, "{heard:?}");
            }
            assert_eq!(heard.len(), if heard[0] == read(1, b"all") { 3 } else { 2 });
            nothing_to_tell(&mut waiter);
            assert_eq!(pairs[1].1.write(b"unheard", &mut Vec::new()), Ok(7));
            nothing_to_tell(&mut waiter);
            // Its refused writes ask nothing of the waiter it left.
            fill(&pairs[1].0);
            // Another waiter may serve it, and reads what waited.
            let mut other = serving(&[&pairs[1].0]);
            assert!(matches!(
                next(&mut other),
                Told::Channel(0, Event::Read, _, 0)
            ));
        });
    }

    /// How many descriptors the waiter's epoll watches.
    fn watched(waiter: &EpollWaiter) -> usize {
        let number = waiter.epoll.as_raw_fd();
        let info = std::fs::read_to_string(format!("/proc/self/fdinfo/{number}")).unwrap();
        info.lines().filter(|line| line.starts_with("tfd:")).count()
    }

    #[test]
    fn a_removed_end_is_out_of_the_epoll() {
        let (host, _child) = Channel::pair().unwrap();
        let (other, _peer) = Channel::pair().unwrap();
        let mut waiter = serving(&[&host, &other]);
        // The two, and what it is interrupted by.
        assert_eq!(watched(&waiter), 3);
        waiter.remove(0);
        assert_eq!(watched(&waiter), 2);
        waiter.remove(0);
        waiter.remove(7);
        assert_eq!(watched(&waiter), 2);
    }

    #[test]
    fn an_end_has_one_waiter() {
        let (host, _child) = Channel::pair().unwrap();
        let mut one = serving(&[&host]);
        assert!(!EpollWaiter::new().unwrap().add(0, &host));
        assert!(!one.add(1, &host));
        one.remove(0);
        assert!(EpollWaiter::new().unwrap().add(0, &host));
    }

    #[test]
    fn an_interrupt_ends_a_wait_once_however_often_it_is_asked() {
        within(|| {
            let (host, child) = Channel::pair().unwrap();
            let mut hosts = serving(&[&host]);
            let interrupt = hosts.interrupter();
            for _ in 0..5 {
                interrupt();
            }
            assert_eq!(next(&mut hosts), Told::Interrupted);
            // Not again: the next wait is for the channel.
            assert_eq!(child.write(b"then", &mut Vec::new()), Ok(4));
            assert_eq!(next(&mut hosts), read(0, b"then"));
            // From another thread, a wait under way.
            let waker = std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(50));
                interrupt();
            });
            assert_eq!(next(&mut hosts), Told::Interrupted);
            waker.join().unwrap();
        });
    }

    #[test]
    fn a_waiter_gone_leaves_its_ends_writers_nothing_to_break() {
        let (host, _child) = Channel::pair().unwrap();
        drop(serving(&[&host]));
        // The epoll is the end's too while it names it: the write asks
        // it for room and is refused as any other.
        fill(&host);
        assert_eq!(
            host.write(b"x", &mut Vec::new()),
            Err(WriteError::WouldBlock)
        );
    }
}
