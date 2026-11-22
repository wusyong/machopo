//! One end of a channel between two processes, whole (the place of Mojo's
//! `Channel` on its IO thread together with what `mojo/core` does for
//! messages, shared buffers and data pipes): the core's parts put to work
//! over a [`Platform`]. A [`Link`] is what any thread sends with; an
//! [`Endpoint`] is the IO thread's. A process has one IO thread, whose
//! loop is an [`IoLoop`]: it waits on every channel the process has at
//! once (a platform's [`Waiter`]) and turns the endpoint of the one
//! something came for (Chromium's IO thread and its `MessagePumpForIO`).
//!
//! The order of what an endpoint does in a turn is here once, for every
//! platform and for both kinds of process, because the core's guarantees
//! rest on it: a region is told of before a message names it and given
//! back once what named it was read; a pipe is opened before a message
//! names it, and its bytes go only as far as its reader has room.
//!
//! Only one end of a channel makes regions (the one that may attach
//! handles, a Browser): the numbers of both directions' regions are its
//! own.

use std::sync::{Arc, Mutex, MutexGuard};

use bytes::Bytes;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::codec::{self, Place, Resolve};
pub use crate::core::channel::Wake;
use crate::core::channel::{self, Gone, Receiver, SendError as ChannelError, Sender, Transport};
use crate::core::control::{CONTROL_DESTINATION, Control};
use crate::core::frame::{BadMessage, Message};
use crate::core::pipe::{Incoming, Outgoing, ReadReport};
use crate::core::region::{Memory, Pool, Table, class_len};
use crate::data_pipe::{self, Consumer, Producer, Read, ReadObserver, TryWriteError};
use crate::shared_buffer::{self, Mapped, Owner, Writer};

/// What there is for an endpoint to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// Bytes from the peer, and the handles that came with them.
    Read,
    /// The transport has room again.
    Writable,
    /// The end's wake was called ([`Sender::on_wake`]): it has something
    /// to do that nothing came from the peer for (a queue to write, a
    /// pipe to feed, a close).
    Woken,
    /// The peer is gone.
    PeerGone,
}

/// What a platform does for one channel's end, and nothing else: it moves
/// bytes and handles and maps memory. Waiting is not a channel's: a
/// process's one IO thread waits on all its channels together, with the
/// platform's [`Waiter`]. What to make of anything is the endpoint's.
///
/// One thing a platform must see to itself: a handle it hands up is one
/// the kernel moved into this process for the message, never a value the
/// peer wrote. Where handles cross as values in the bytes (a system
/// where the sender duplicates them into the receiver), an end whose
/// peer may attach none looks at no value and hands no handle up, so
/// the core refuses the message that says it has one: a value is a
/// handle of this process's own, of the peer's choosing.
pub trait Platform: Transport<Handle: Send> + Send + Sync + Sized + 'static {
    /// A mapping of shared memory. Shared between the threads of a
    /// process: whoever holds bytes that crossed in a region keeps its
    /// mapping, on a thread of its own.
    type Memory: Memory + Send + Sync + 'static;

    /// What an IO thread waits on this platform's channels with.
    type Waiter: Waiter<Self>;

    /// Maps `len` bytes of the region a handle from the peer is, to write
    /// when `writable`. `None` when the handle isn't such a region.
    fn map(&self, handle: Self::Handle, len: usize, writable: bool) -> Option<Self::Memory>;

    /// The channel is over at this end: the transport is shut, so the
    /// peer's platform says this end is gone. Called once, by the IO
    /// thread.
    fn shut(&self);

    /// Makes a region of `len` bytes: this end's mapping of it, and the
    /// handle that gives the peer its own, to write when `peer_writes` and
    /// only to read otherwise.
    fn create(&self, len: usize, peer_writes: bool) -> Option<(Self::Memory, Self::Handle)>;
}

/// Which of an IO thread's channels: the loop's name for one.
pub type Key = u32;

/// What a wait came back with.
pub enum Waited<'a, H> {
    /// The [`Waiter::interrupter`] was called.
    Interrupted,
    /// Something for the channel `key`: [`Event::Read`], with the bytes
    /// lent and the handles that came with them; [`Event::Writable`]; or
    /// [`Event::PeerGone`]. The bytes are lent, not handed over, so a
    /// platform that has them in memory of its own (a message mapped out
    /// of line) copies them nowhere.
    Channel {
        key: Key,
        event: Event,
        bytes: &'a [u8],
        handles: &'a mut Vec<H>,
    },
}

/// What an IO thread waits with: all of a process's channels at once (a
/// port set, an epoll, a completion port), and a way to be woken. One a
/// thread, used by that thread alone.
pub trait Waiter<P: Platform>: Sized {
    /// `None` when the system gives none.
    fn new() -> Option<Self>;

    /// What ends [`Waiter::wait`] from another thread: a wait under way
    /// returns [`Waited::Interrupted`], and so does the next if none is.
    fn interrupter(&self) -> Wake;

    /// `channel` is waited on from now, as `key`. `false` when it can't
    /// be (it is shut already).
    fn add(&mut self, key: Key, channel: &P) -> bool;

    /// The channel `key` is no longer waited on.
    fn remove(&mut self, key: Key);

    /// Waits until there is something to do, then has `then` do it.
    fn wait<R>(&mut self, then: impl FnOnce(Waited<'_, P::Handle>) -> R) -> R;
}

/// What an endpoint is made with.
#[derive(Debug, Clone, Copy)]
pub struct Config {
    /// How many destinations the layer above has at this end.
    pub destinations: u32,
    /// This end may attach handles, and so is the one that makes regions.
    pub may_attach: bool,
    /// The peer's end may.
    pub peer_may_attach: bool,
    /// The peer may open pipes to this end. Each is memory this end keeps
    /// for it, so a peer that isn't trusted may open none.
    pub peer_may_open_pipes: bool,
    /// The most bytes of regions this end writes into.
    pub region_limit: usize,
    /// The most bytes of regions this end makes for the peer to write
    /// into, asked for by either end. Only for the end that makes
    /// regions.
    pub peer_region_limit: usize,
    /// The most bytes that wait to be written
    /// ([`channel::MAX_QUEUED_BYTES`]).
    pub queue_limit: usize,
}

/// Why something wasn't sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendError {
    /// The channel is over.
    Gone(Gone),
    /// This end's own mistake (a value that doesn't serialize, a message
    /// too long): nothing was sent, and the channel goes on.
    Refused,
}

impl From<ChannelError> for SendError {
    fn from(error: ChannelError) -> Self {
        match error {
            ChannelError::Gone(gone) => Self::Gone(gone),
            ChannelError::Encode(_) | ChannelError::HandlesNotAllowed => Self::Refused,
        }
    }
}

/// What the sending threads and the IO thread share, behind one lock: a
/// message's large bytes are placed, the regions they went in are told
/// of, and the message is sent, with nothing of another thread's between.
struct Out<P: Platform> {
    /// The regions this end writes into. Each mapping is shared with
    /// whoever took the region to write in place, who may hold it past
    /// the channel's end.
    pool: Pool<Arc<P::Memory>>,
    /// The next region's number, when this end makes them.
    next_region: u32,
    /// The pipes this end writes, and where their bytes come from.
    pipes: Outgoing,
    feeding: Vec<(u32, Consumer)>,
    /// Regions asked for the peer to write into, for the IO thread to make
    /// (their mappings are its table's).
    to_grant: Vec<usize>,
    /// Where a message is put together, kept from one to the next.
    scratch: Vec<u8>,
}

struct Inner<P: Platform> {
    sender: Sender<P>,
    out: Mutex<Out<P>>,
    config: Config,
}

impl<P: Platform> Inner<P> {
    fn out(&self) -> MutexGuard<'_, Out<P>> {
        // A panic while holding it (an allocation failing) leaves the
        // channel no worse than the panic did.
        self.out
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn platform(&self) -> &P {
        self.sender.transport()
    }

    fn control(&self, control: &Control<'_>, handles: Vec<P::Handle>) -> Result<(), SendError> {
        let mut payload = Vec::new();
        control.encode(&mut payload);
        Ok(self.sender.send(CONTROL_DESTINATION, &payload, handles)?)
    }
}

impl<P: Platform> Owner for Inner<P> {
    fn give_back(&self, number: u32) {
        self.out().pool.give_back(number);
    }

    fn read(&self, number: u32) {
        // To a channel that is over there is nobody to tell.
        let _ = self.control(&Control::Released(vec![number]), Vec::new());
    }
}

/// Which end `inner` is, as a region's taker knows its owner
/// ([`shared_buffer::owner_id`]).
fn id_of<P: Platform>(inner: &Inner<P>) -> usize {
    std::ptr::from_ref(inner).cast::<()>().addr()
}

/// Places a message's large bytes in the pool, making a region for them
/// when no free one is long enough and this end may.
struct Placing<'a, P: Platform> {
    inner: &'a Inner<P>,
    pool: &'a mut Pool<Arc<P::Memory>>,
    next_region: &'a mut u32,
    pipes: &'a mut Outgoing,
    feeding: &'a mut Vec<(u32, Consumer)>,
    /// What this message placed, and the pipes opened for it, to take
    /// back if it isn't sent.
    placed: Vec<u32>,
    piped: Vec<u32>,
}

/// Opens a pipe to the peer that holds at most `capacity` unread bytes,
/// fed from `source`: told to the peer now, so before whatever names it.
/// With `out`'s lock held.
fn open_pipe<P: Platform>(
    inner: &Inner<P>,
    pipes: &mut Outgoing,
    feeding: &mut Vec<(u32, Consumer)>,
    mut source: Consumer,
    capacity: u32,
) -> Result<u32, SendError> {
    let pipe = pipes.open(capacity).ok_or(SendError::Refused)?;
    if let Err(error) = inner.control(&Control::PipeOpened { pipe, capacity }, Vec::new()) {
        pipes.end(pipe);
        return Err(error);
    }
    // The IO thread sends what the source has, each time it has more:
    // watching tells at once of what it has already, and the turn that
    // makes waits for `out`, so it finds the source here.
    let sender = inner.sender.clone();
    source.watch(move || sender.wake());
    feeding.push((pipe, source));
    Ok(pipe)
}

impl<P: Platform> Placing<'_, P> {
    /// A new region that holds `len` bytes, told to the peer: `None` when
    /// this end makes none, has its fill, or the platform has none to
    /// give.
    fn grow(&mut self, len: usize) -> Option<()> {
        if !self.inner.config.may_attach {
            return None;
        }
        let class = class_len(len)?;
        if self.pool.total().checked_add(class)? > self.inner.config.region_limit {
            return None;
        }
        let number = *self.next_region;
        let after = number.checked_add(1)?;
        let (memory, handle) = self.inner.platform().create(class, false)?;
        self.pool.add(number, Arc::new(memory)).ok()?;
        *self.next_region = after;
        let granted = Control::Granted {
            number,
            len: u32::try_from(class).ok()?,
            writable: false,
        };
        // A channel that is over sends nothing; the message these bytes
        // are for won't go either.
        self.inner.control(&granted, vec![handle]).ok()
    }
}

impl<P: Platform> Place for Placing<'_, P> {
    fn place(&mut self, bytes: &[u8]) -> Option<u32> {
        let number = match self.pool.put(bytes) {
            Some(number) => number,
            None if self.inner.config.may_attach => {
                self.grow(bytes.len())?;
                self.pool.put(bytes)?
            }
            None => {
                // These go in the message. The end that makes regions is
                // told there was none free for them: it may grant one
                // more, for the next that long. Nothing waits for it.
                if let Ok(len) = u32::try_from(bytes.len()) {
                    let _ = self.inner.control(&Control::Wanted { len }, Vec::new());
                }
                return None;
            }
        };
        self.placed.push(number);
        Some(number)
    }

    fn unplace(&mut self, number: u32) {
        self.pool.give_back(number);
        self.placed.retain(|placed| *placed != number);
    }

    fn lend(&mut self, owner: usize, number: u32) -> bool {
        if owner != id_of(self.inner) || !self.pool.lend(number) {
            return false;
        }
        self.placed.push(number);
        true
    }

    fn pipe(&mut self, mut source: Consumer) -> Option<u32> {
        // As long as the pipe it reads: the peer keeps as much.
        let capacity = u32::try_from(source.capacity()).ok()?;
        let pipe = open_pipe(self.inner, self.pipes, self.feeding, source, capacity).ok()?;
        self.piped.push(pipe);
        Some(pipe)
    }

    fn unpipe(&mut self, number: u32) {
        self.piped.retain(|piped| *piped != number);
        // The peer was told of the pipe: it is told there is nothing to
        // come. The source goes, and whoever writes it hears.
        self.feeding.retain(|(pipe, _)| *pipe != number);
        if self.pipes.end(number) {
            let _ = self
                .inner
                .control(&Control::PipeEnded { pipe: number }, Vec::new());
        }
    }
}

/// An endpoint's sending side, for any thread.
pub struct Link<P: Platform>(Arc<Inner<P>>);

impl<P: Platform> Clone for Link<P> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<P: Platform> Link<P> {
    /// Sends `message` to the peer's `destination`, without waiting. Its
    /// large bytes go by a region when there is one (when there is none,
    /// the end that makes them makes one, and the other asks it for one
    /// for the next time), and the data pipes
    /// in it ([`Consumer`]) go with it: each is opened to the peer before
    /// the message, and fed from then on. A message that isn't sent ends
    /// the pipes it had.
    pub fn send<T: Serialize + ?Sized>(
        &self,
        destination: u32,
        message: &T,
    ) -> Result<(), SendError> {
        let mut out = self.0.out();
        let Out {
            pool,
            next_region,
            pipes,
            feeding,
            scratch,
            ..
        } = &mut *out;
        scratch.clear();
        let mut placing = Placing {
            inner: &self.0,
            pool,
            next_region,
            pipes,
            feeding,
            placed: Vec::new(),
            piped: Vec::new(),
        };
        codec::encode(message, &mut placing, scratch).map_err(|_| SendError::Refused)?;
        let sent = self.0.sender.send(destination, scratch, Vec::new());
        if sent.is_err() {
            // The peer gets no message that names what was placed for
            // this one, or the pipes opened for it: nobody will read
            // them or give them back.
            for number in std::mem::take(&mut placing.placed) {
                placing.unplace(number);
            }
            for number in std::mem::take(&mut placing.piped) {
                placing.unpipe(number);
            }
        }
        Ok(sent?)
    }

    /// Opens a pipe to the peer that holds at most `capacity` unread
    /// bytes, fed from `source`: its number, for the message that names
    /// it, which is to be sent after.
    pub fn open_pipe(&self, source: Consumer, capacity: u32) -> Result<u32, SendError> {
        let mut out = self.0.out();
        let Out { pipes, feeding, .. } = &mut *out;
        open_pipe(&self.0, pipes, feeding, source, capacity)
    }

    /// Asks for a region of `len` bytes or more for the peer to write its
    /// large bytes into. Only for the end that makes regions; one past
    /// what it makes for its peer ([`Config::peer_region_limit`]) isn't
    /// made.
    pub fn grant_to_peer(&self, len: usize) {
        self.0.out().to_grant.push(len);
        self.0.sender.wake();
    }

    /// A free region of this end's to write `len` bytes in place, for a
    /// message of this channel to take to the peer as a
    /// [`SharedBuffer`](shared_buffer::SharedBuffer): nothing is copied
    /// on the way. `None` when no free region is that long; none is made
    /// for it here. What was to be written there is then sent as bytes
    /// of the sender's own, which is what asks for one more.
    pub fn take_region(&self, len: usize) -> Option<Writer> {
        let mut out = self.0.out();
        let number = out.pool.take(len)?;
        let Some(memory) = out.pool.memory(number).cloned() else {
            out.pool.give_back(number);
            return None;
        };
        drop(out);
        let memory: Mapped = memory;
        Some(Writer::new(memory, number, len, self.0.clone()))
    }

    /// Closes the channel: the IO thread's loop ends.
    pub fn close(&self) {
        self.0.sender.close();
    }

    /// The bytes of regions this end writes into, and how many of them
    /// are free.
    pub fn regions(&self) -> (usize, usize) {
        let out = self.0.out();
        (out.pool.total(), out.pool.free().map(|(_, len)| len).sum())
    }

    /// The platform under the channel.
    pub fn platform(&self) -> &P {
        self.0.platform()
    }

    /// Has `wake` called when this end has something for its IO thread to
    /// do. A loop does this for the ends it serves; this is for whoever
    /// turns an endpoint by hand in a loop's place.
    pub fn on_wake(&self, wake: Wake) {
        self.0.sender.on_wake(wake);
    }
}

/// Tells a pipe's writer how much of it was read, and that its reader is
/// gone.
struct Reporter<P: Platform> {
    inner: Arc<Inner<P>>,
    pipe: u32,
    report: ReadReport,
    done: bool,
}

impl<P: Platform> ReadObserver for Reporter<P> {
    fn read(&mut self, count: usize) {
        if let Some(count) = self.report.read(count) {
            // To a channel that is over there is nobody to tell.
            let _ = self.inner.control(
                &Control::PipeRead {
                    pipe: self.pipe,
                    count,
                },
                Vec::new(),
            );
        }
    }

    fn done(&mut self) {
        self.done = true;
    }
}

impl<P: Platform> Drop for Reporter<P> {
    fn drop(&mut self) {
        if !self.done {
            let _ = self
                .inner
                .control(&Control::PipeCancelled { pipe: self.pipe }, Vec::new());
        }
    }
}

/// A message for the layer above, on the IO thread.
pub struct Received<'a, P: Platform> {
    destination: u32,
    payload: &'a [u8],
    inner: &'a Arc<Inner<P>>,
    table: &'a mut Table<Arc<P::Memory>>,
    arrived: &'a mut Vec<(u32, Consumer)>,
}

impl<P: Platform> Received<'_, P> {
    /// Which of this end's destinations it is for.
    pub const fn destination(&self) -> u32 {
        self.destination
    }

    /// The message as the type its destination takes; its large bytes
    /// copied out of the peer's regions, but for those the type takes as
    /// a [`SharedBuffer`](shared_buffer::SharedBuffer), whose regions
    /// are the peer's to write again when the buffers go. An error is a
    /// bad message: the handler returns it, and the channel is closed.
    pub fn decode<T: DeserializeOwned>(&mut self) -> Result<T, BadMessage> {
        codec::decode(
            self.payload,
            &mut Arriving {
                inner: self.inner,
                table: self.table,
                arrived: self.arrived,
            },
        )
    }

    /// The reading end of a pipe the peer opened, by the number a message
    /// names it with. `None` when no such pipe is waiting to be taken: a
    /// bad message.
    pub fn take_pipe(&mut self, pipe: u32) -> Option<Consumer> {
        let i = self
            .arrived
            .iter()
            .position(|(number, _)| *number == pipe)?;
        Some(self.arrived.swap_remove(i).1)
    }
}

/// What a message's value is read out of: the peer's regions, and the
/// pipes it opened that no message has named yet.
struct Arriving<'a, P: Platform> {
    inner: &'a Arc<Inner<P>>,
    table: &'a mut Table<Arc<P::Memory>>,
    arrived: &'a mut Vec<(u32, Consumer)>,
}

impl<P: Platform> Resolve for Arriving<'_, P> {
    fn resolve(&mut self, number: u32, len: u32, into: &mut Vec<u8>) -> Result<(), BadMessage> {
        self.table.read(number, len, into)
    }

    fn hold(&mut self, number: u32, len: u32) -> Result<(), BadMessage> {
        let memory: Mapped = self.table.named(number, len)?.clone();
        let len = usize::try_from(len).map_err(|_| BadMessage::OutsideRegion { number, len })?;
        shared_buffer::set_arriving(memory, number, len, self.inner.clone());
        Ok(())
    }

    fn pipe(&mut self, number: u32) -> Result<Consumer, BadMessage> {
        let i = self
            .arrived
            .iter()
            .position(|(pipe, _)| *pipe == number)
            .ok_or(BadMessage::UnknownPipe { pipe: number })?;
        Ok(self.arrived.swap_remove(i).1)
    }
}

/// What the layer above does with a message.
///
/// It runs on the IO thread, which every channel of the process waits
/// behind: it may do work (check a message, hand it on), but it doesn't
/// wait for anything, and what it does for one message doesn't grow
/// without bound with what the peer sent.
pub trait Handler<P: Platform> {
    /// A message from the peer, on the IO thread. An error closes the
    /// channel.
    fn message(&mut self, message: Received<'_, P>) -> Result<(), BadMessage>;

    /// The channel is over, and why: told once, on the IO thread, after
    /// which the handler is dropped.
    fn gone(&mut self, gone: Gone) {
        let _ = gone;
    }
}

/// An endpoint's receiving side: the IO thread's.
pub struct Endpoint<P: Platform> {
    inner: Arc<Inner<P>>,
    receiver: Receiver<P>,
    /// The regions the peer writes into. Each mapping is shared with
    /// whoever holds a buffer a message brought in it.
    table: Table<Arc<P::Memory>>,
    /// The bytes of the regions this end made for the peer to write
    /// into.
    granted: usize,
    /// The pipes the peer writes, and where their bytes go.
    incoming: Incoming<Producer>,
    /// The reading ends of pipes the peer opened, until a message names
    /// them.
    arrived: Vec<(u32, Consumer)>,
}

/// An endpoint over `platform`: its sending side, for any thread, and its
/// receiving side, for the IO thread.
pub fn endpoint<P: Platform>(platform: P, config: Config) -> (Link<P>, Endpoint<P>) {
    let (sender, receiver) = channel::channel(
        platform,
        channel::Config {
            destinations: config.destinations,
            may_attach: config.may_attach,
            peer_may_attach: config.peer_may_attach,
            queue_limit: config.queue_limit,
        },
    );
    let inner = Arc::new(Inner {
        sender,
        out: Mutex::new(Out {
            pool: Pool::new(config.region_limit),
            next_region: 0,
            pipes: Outgoing::new(),
            feeding: Vec::new(),
            to_grant: Vec::new(),
            scratch: Vec::new(),
        }),
        config,
    });
    let endpoint = Endpoint {
        inner: inner.clone(),
        receiver,
        table: Table::new(),
        granted: 0,
        incoming: Incoming::new(),
        arrived: Vec::new(),
    };
    (Link(inner), endpoint)
}

impl<P: Platform> Endpoint<P> {
    /// The platform's end this endpoint is over.
    pub fn platform(&self) -> &P {
        self.inner.platform()
    }

    /// Closes the channel at this end, for a loop that stops with the
    /// channel still open: the peer hears of it.
    fn abandon(&mut self) {
        self.inner.sender.close();
        self.over();
    }

    /// One turn of the loop, for what one wait came back with. `Some` when
    /// the channel is over: the endpoint is done.
    pub fn turn(
        &mut self,
        event: Event,
        bytes: &[u8],
        handles: &mut Vec<P::Handle>,
        handler: &mut (impl Handler<P> + ?Sized),
    ) -> Option<Gone> {
        let gone = match event {
            Event::Read => self.read(bytes, handles, handler),
            Event::Writable => None,
            Event::Woken => self.receiver.woken(),
            Event::PeerGone => self.receiver.peer_gone(),
        };
        if gone.is_some() {
            self.over();
            return gone;
        }
        self.grant();
        self.feed_pipes();
        self.inner.sender.flush();
        None
    }

    /// The channel is over: what this end held for it goes.
    fn over(&mut self) {
        let mut out = self.inner.out();
        // The sources' writers see their reader gone.
        out.feeding.clear();
        out.to_grant.clear();
        // Nothing more is sent: the regions kept to send by are unmapped
        // now, not when the last of this end's senders is dropped (a tab
        // keeps the host of a renderer that went until it is loaded
        // again).
        out.pool = Pool::new(self.inner.config.region_limit);
        drop(out);
        self.inner.platform().shut();
    }

    fn read(
        &mut self,
        bytes: &[u8],
        handles: &mut Vec<P::Handle>,
        handler: &mut (impl Handler<P> + ?Sized),
    ) -> Option<Gone> {
        let Self {
            inner,
            receiver,
            table,
            incoming,
            arrived,
            ..
        } = self;
        let gone = receiver.feed(bytes, handles.drain(..), |message| {
            if message.destination == CONTROL_DESTINATION {
                return control(inner, table, incoming, arrived, message);
            }
            // Only the core's own messages come with handles.
            if !message.handles.is_empty() {
                return Err(BadMessage::Rejected);
            }
            handler.message(Received {
                destination: message.destination,
                payload: message.payload,
                inner,
                table,
                arrived,
            })
        });
        if gone.is_some() {
            return gone;
        }
        // What this batch read out of the peer's regions may be written
        // again.
        let read = table.take_read();
        if !read.is_empty() {
            let _ = inner.control(&Control::Released(read), Vec::new());
        }
        None
    }

    /// Makes the regions asked for the peer to write into.
    fn grant(&mut self) {
        let lens = std::mem::take(&mut self.inner.out().to_grant);
        for len in lens {
            let _ = self.grant_one(len);
        }
    }

    fn grant_one(&mut self, len: usize) -> Option<()> {
        if !self.inner.config.may_attach {
            return None;
        }
        let class = class_len(len)?;
        let granted_len = u32::try_from(class).ok()?;
        // Whoever asked, the peer has only so much: a peer that asks
        // without end is given no more.
        let granted = self.granted.checked_add(class)?;
        if granted > self.inner.config.peer_region_limit {
            return None;
        }
        let mut out = self.inner.out();
        let number = out.next_region;
        let after = number.checked_add(1)?;
        let (memory, handle) = self.inner.platform().create(class, true)?;
        if !self.table.insert(number, Arc::new(memory)) {
            return None;
        }
        self.granted = granted;
        out.next_region = after;
        let granted = Control::Granted {
            number,
            len: granted_len,
            writable: true,
        };
        // Sent with the lock held: no message of another thread's is
        // numbered between a region's making and its telling.
        self.inner.control(&granted, vec![handle]).ok()
    }

    /// Sends what the pipes' sources have, as far as their readers have
    /// room.
    fn feed_pipes(&mut self) {
        let mut out = self.inner.out();
        let Out { pipes, feeding, .. } = &mut *out;
        feeding.retain_mut(|(pipe, source)| {
            let pipe = *pipe;
            loop {
                let room = pipes.room(pipe);
                if room == 0 {
                    // Out of room, or cancelled by its reader.
                    return pipes.open_pipes().any(|open| open == pipe);
                }
                match source.read(room) {
                    Read::Data(bytes) => {
                        let counted = pipes.sent(pipe, bytes.len());
                        debug_assert!(counted, "a read of no more than the room");
                        let data = Control::PipeData {
                            pipe,
                            bytes: &bytes,
                        };
                        if self.inner.control(&data, Vec::new()).is_err() {
                            return false;
                        }
                    }
                    Read::Empty => return true,
                    Read::Done => {
                        pipes.end(pipe);
                        let _ = self.inner.control(&Control::PipeEnded { pipe }, Vec::new());
                        return false;
                    }
                }
            }
        });
    }
}

/// An endpoint a loop serves, and what the layer above does with its
/// messages.
struct Served<P: Platform> {
    key: Key,
    endpoint: Endpoint<P>,
    handler: Box<dyn Handler<P> + Send>,
}

/// What other threads leave for a loop.
struct Pending<P: Platform> {
    /// Endpoints to serve from now on.
    joining: Vec<(Endpoint<P>, Box<dyn Handler<P> + Send>)>,
    /// The channels whose ends were woken.
    woken: Vec<Key>,
    stop: bool,
}

struct LoopShared<P: Platform> {
    interrupt: Wake,
    pending: Mutex<Pending<P>>,
}

impl<P: Platform> LoopShared<P> {
    fn pending(&self) -> MutexGuard<'_, Pending<P>> {
        // Held for a push or a take.
        self.pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// A process's IO thread's loop: every channel the process has, waited on
/// together.
pub struct IoLoop<P: Platform> {
    waiter: P::Waiter,
    served: Vec<Served<P>>,
    next_key: Key,
    shared: Arc<LoopShared<P>>,
}

/// A loop, for the threads that aren't its own: gives it endpoints to
/// serve, and stops it.
pub struct LoopHandle<P: Platform>(Arc<LoopShared<P>>);

impl<P: Platform> Clone for LoopHandle<P> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<P: Platform> LoopHandle<P> {
    /// Has the loop serve `endpoint` from now on: its messages go to
    /// `handler`, which is told when the channel is over.
    pub fn serve(&self, endpoint: Endpoint<P>, handler: impl Handler<P> + Send + 'static) {
        self.0.pending().joining.push((endpoint, Box::new(handler)));
        (self.0.interrupt)();
    }

    /// Stops the loop: [`IoLoop::run`] returns, the channels it served
    /// closed at this end.
    pub fn stop(&self) {
        self.0.pending().stop = true;
        (self.0.interrupt)();
    }
}

impl<P: Platform> IoLoop<P> {
    /// A loop that serves nothing yet, and its handle. `None` when the
    /// platform has nothing to wait with.
    pub fn new() -> Option<(Self, LoopHandle<P>)> {
        let waiter = P::Waiter::new()?;
        let shared = Arc::new(LoopShared {
            interrupt: waiter.interrupter(),
            pending: Mutex::new(Pending {
                joining: Vec::new(),
                woken: Vec::new(),
                stop: false,
            }),
        });
        let io = Self {
            waiter,
            served: Vec::new(),
            next_key: 0,
            shared: shared.clone(),
        };
        Some((io, LoopHandle(shared)))
    }

    /// The IO thread's loop, until it is stopped.
    pub fn run(mut self) {
        loop {
            let (joining, woken, stop) = {
                let mut pending = self.shared.pending();
                (
                    std::mem::take(&mut pending.joining),
                    std::mem::take(&mut pending.woken),
                    pending.stop,
                )
            };
            if stop {
                let served = self
                    .served
                    .drain(..)
                    .map(|served| (served.endpoint, served.handler));
                for (mut endpoint, mut handler) in served.chain(joining) {
                    endpoint.abandon();
                    handler.gone(Gone::Closed);
                }
                return;
            }
            let mut turns = woken;
            for (endpoint, handler) in joining {
                turns.extend(self.join(endpoint, handler));
            }
            // An end woken has something to do that nothing came from
            // the peer for (a queue to write, a pipe to feed, a close).
            for key in turns {
                let gone = turn(&mut self.served, key, Event::Woken, &[], &mut Vec::new());
                self.leave(gone);
            }
            let Self { waiter, served, .. } = &mut self;
            let gone = waiter.wait(|waited| match waited {
                Waited::Interrupted => None,
                Waited::Channel {
                    key,
                    event,
                    bytes,
                    handles,
                } => turn(served, key, event, bytes, handles),
            });
            self.leave(gone);
        }
    }

    /// Serves `endpoint` from now: its key, for the turn joining gives it
    /// (what its end did before it joined woke nobody).
    fn join(
        &mut self,
        mut endpoint: Endpoint<P>,
        mut handler: Box<dyn Handler<P> + Send>,
    ) -> Option<Key> {
        let key = self.next_key;
        // Keys aren't used again: a wake for a channel that left names
        // nothing.
        let next = key.checked_add(1);
        let added = next.is_some() && self.waiter.add(key, endpoint.platform());
        let (Some(next), true) = (next, added) else {
            endpoint.abandon();
            handler.gone(Gone::Closed);
            return None;
        };
        self.next_key = next;
        let shared = self.shared.clone();
        endpoint.inner.sender.on_wake(Arc::new(move || {
            shared.pending().woken.push(key);
            (shared.interrupt)();
        }));
        self.served.push(Served {
            key,
            endpoint,
            handler,
        });
        Some(key)
    }

    /// The channel `gone` names is over: it is served no more.
    fn leave(&mut self, gone: Option<Key>) {
        if let Some(key) = gone {
            self.waiter.remove(key);
            self.served.retain(|served| served.key != key);
        }
    }
}

/// Turns the endpoint `key` is, if the loop still serves it: its key
/// again when its channel is over (its handler was told).
fn turn<P: Platform>(
    served: &mut [Served<P>],
    key: Key,
    event: Event,
    bytes: &[u8],
    handles: &mut Vec<P::Handle>,
) -> Option<Key> {
    let served = served.iter_mut().find(|served| served.key == key)?;
    let gone = served
        .endpoint
        .turn(event, bytes, handles, &mut *served.handler)?;
    served.handler.gone(gone);
    Some(key)
}

/// One of the core's own messages from the peer.
fn control<P: Platform>(
    inner: &Arc<Inner<P>>,
    table: &mut Table<Arc<P::Memory>>,
    incoming: &mut Incoming<Producer>,
    arrived: &mut Vec<(u32, Consumer)>,
    mut message: Message<'_, P::Handle>,
) -> Result<(), BadMessage> {
    let control = Control::decode(message.payload)?;
    if message.handles.len() != control.handles() {
        return Err(BadMessage::Rejected);
    }
    match control {
        Control::Released(numbers) => {
            let mut out = inner.out();
            numbers
                .into_iter()
                .try_for_each(|number| out.pool.released(number))
        }
        Control::Granted {
            number,
            len,
            writable,
        } => {
            let handle = message.handles.pop().ok_or(BadMessage::Rejected)?;
            let len = usize::try_from(len).map_err(|_| BadMessage::Rejected)?;
            let memory = inner
                .platform()
                .map(handle, len, writable)
                .ok_or(BadMessage::Rejected)?;
            let memory = Arc::new(memory);
            let kept = if writable {
                inner.out().pool.add(number, memory).is_ok()
            } else {
                table.insert(number, memory)
            };
            if kept {
                Ok(())
            } else {
                Err(BadMessage::Rejected)
            }
        }
        Control::Wanted { len } => {
            // The end that makes the regions asks nobody.
            if !inner.config.may_attach {
                return Err(BadMessage::Rejected);
            }
            // Made at the end of this turn, if the peer may have more.
            let len = usize::try_from(len).map_err(|_| BadMessage::Rejected)?;
            inner.out().to_grant.push(len);
            Ok(())
        }
        Control::PipeOpened { pipe, capacity } => {
            if !inner.config.peer_may_open_pipes {
                return Err(BadMessage::Rejected);
            }
            let (producer, mut consumer) =
                data_pipe::create(usize::try_from(capacity).unwrap_or(usize::MAX));
            incoming.opened(pipe, producer)?;
            consumer.observe(Box::new(Reporter {
                inner: inner.clone(),
                pipe,
                report: ReadReport::new(capacity),
                done: false,
            }));
            arrived.push((pipe, consumer));
            Ok(())
        }
        Control::PipeData { pipe, bytes } => {
            let Some(producer) = incoming.get(pipe)? else {
                return Ok(());
            };
            match producer.try_write(Bytes::copy_from_slice(bytes)) {
                Ok(()) => Ok(()),
                Err(TryWriteError::Full) => Err(BadMessage::PipeOverrun { pipe }),
                // Its reader went: the cancel is on its way to the peer.
                Err(TryWriteError::Closed) => {
                    incoming.cancel(pipe);
                    Ok(())
                }
            }
        }
        Control::PipeRead { pipe, count } => inner.out().pipes.peer_read(pipe, count),
        Control::PipeEnded { pipe } => {
            incoming.ended(pipe)?;
            // Ended with no message having named it (the one it was
            // opened for didn't go): nothing will take it.
            arrived.retain(|(number, _)| *number != pipe);
            Ok(())
        }
        Control::PipeCancelled { pipe } => {
            // Its source goes at the feeding that follows: a pipe that
            // isn't open has no room and is let go there.
            inner.out().pipes.peer_cancelled(pipe).map(drop)
        }
    }
}
