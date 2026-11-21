//! One end of a channel between two processes, whole (the place of Mojo's
//! `Channel` on its IO thread together with what `mojo/core` does for
//! messages, shared buffers and data pipes): the core's parts put to work
//! over a [`Platform`]. A [`Link`] is what any thread sends with; an
//! [`Endpoint`] is the IO thread's, whose [`Endpoint::run`] is that
//! thread's loop until the channel is over.
//!
//! The order of what the loop does is here once, for every platform and
//! for both kinds of process, because the core's guarantees rest on it: a
//! region is told of before a message names it and given back once what
//! named it was read; a pipe is opened before a message names it, and its
//! bytes go only as far as its reader has room.
//!
//! Only one end of a channel makes regions (the one that may attach
//! handles, a Browser): the numbers of both directions' regions are its
//! own.

use std::sync::{Arc, Mutex, MutexGuard};

use bytes::Bytes;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::codec::{self, Place};
use crate::core::channel::{self, Gone, Receiver, SendError as ChannelError, Sender, Transport};
use crate::core::control::{CONTROL_DESTINATION, Control};
use crate::core::frame::{BadMessage, Message};
use crate::core::pipe::{Incoming, Outgoing, ReadReport};
use crate::core::region::{Memory, Pool, Table, class_len};
use crate::data_pipe::{self, Consumer, Producer, Read, ReadObserver, TryWriteError};

/// What a platform's wait came back with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// Bytes from the peer (and the handles that came with them) were
    /// added to what the wait was given.
    Read,
    /// The transport has room again.
    Writable,
    /// [`Transport::wake`] was called.
    Woken,
    /// The peer is gone.
    PeerGone,
}

/// What a platform does for an endpoint, and nothing else: it moves bytes
/// and handles, waits, and maps memory. What to make of any of it is the
/// endpoint's.
pub trait Platform: Transport + Send + Sync + 'static
where
    Self::Handle: Send,
{
    /// A mapping of shared memory.
    type Memory: Memory + Send + 'static;

    /// Waits until there is something for the IO thread to do, and says
    /// what. Only the IO thread calls it.
    fn wait(&self, bytes: &mut Vec<u8>, handles: &mut Vec<Self::Handle>) -> Event;

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
struct Out<P: Platform>
where
    P::Handle: Send,
{
    /// The regions this end writes into.
    pool: Pool<P::Memory>,
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

struct Inner<P: Platform>
where
    P::Handle: Send,
{
    sender: Sender<P>,
    out: Mutex<Out<P>>,
    config: Config,
}

impl<P: Platform> Inner<P>
where
    P::Handle: Send,
{
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

/// Places a message's large bytes in the pool, making a region for them
/// when no free one is long enough and this end may.
struct Placing<'a, P: Platform>
where
    P::Handle: Send,
{
    inner: &'a Inner<P>,
    pool: &'a mut Pool<P::Memory>,
    next_region: &'a mut u32,
    /// What this message placed, to take back if it isn't sent.
    placed: Vec<u32>,
}

impl<P: Platform> Placing<'_, P>
where
    P::Handle: Send,
{
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
        self.pool.add(number, memory).ok()?;
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

impl<P: Platform> Place for Placing<'_, P>
where
    P::Handle: Send,
{
    fn place(&mut self, bytes: &[u8]) -> Option<u32> {
        let number = match self.pool.put(bytes) {
            Some(number) => number,
            None => {
                self.grow(bytes.len())?;
                self.pool.put(bytes)?
            }
        };
        self.placed.push(number);
        Some(number)
    }

    fn unplace(&mut self, number: u32) {
        self.pool.give_back(number);
        self.placed.retain(|placed| *placed != number);
    }
}

/// An endpoint's sending side, for any thread.
pub struct Link<P: Platform>(Arc<Inner<P>>)
where
    P::Handle: Send;

impl<P: Platform> Clone for Link<P>
where
    P::Handle: Send,
{
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<P: Platform> Link<P>
where
    P::Handle: Send,
{
    /// Sends `message` to the peer's `destination`, without waiting. Its
    /// large bytes go by a region when there is one.
    pub fn send<T: Serialize + ?Sized>(
        &self,
        destination: u32,
        message: &T,
    ) -> Result<(), SendError> {
        let mut out = self.0.out();
        let Out {
            pool,
            next_region,
            scratch,
            ..
        } = &mut *out;
        scratch.clear();
        let mut placing = Placing {
            inner: &self.0,
            pool,
            next_region,
            placed: Vec::new(),
        };
        codec::encode(message, &mut placing, scratch).map_err(|_| SendError::Refused)?;
        let sent = self.0.sender.send(destination, scratch, Vec::new());
        if sent.is_err() {
            // The peer gets no message that names what was placed for
            // this one: nobody will read it or give it back.
            for number in std::mem::take(&mut placing.placed) {
                placing.unplace(number);
            }
        }
        Ok(sent?)
    }

    /// Opens a pipe to the peer that holds at most `capacity` unread
    /// bytes, fed from `source`: its number, for the message that names
    /// it, which is to be sent after.
    pub fn open_pipe(&self, source: Consumer, capacity: u32) -> Result<u32, SendError> {
        let mut source = source;
        let mut out = self.0.out();
        let pipe = out.pipes.open(capacity).ok_or(SendError::Refused)?;
        if let Err(error) = self
            .0
            .control(&Control::PipeOpened { pipe, capacity }, Vec::new())
        {
            out.pipes.end(pipe);
            return Err(error);
        }
        // The IO thread sends what the source has, each time it has more.
        let sender = self.0.sender.clone();
        source.watch(move || sender.transport().wake());
        out.feeding.push((pipe, source));
        drop(out);
        self.0.platform().wake();
        Ok(pipe)
    }

    /// Asks for a region of `len` bytes or more for the peer to write its
    /// large bytes into. Only for the end that makes regions.
    pub fn grant_to_peer(&self, len: usize) {
        self.0.out().to_grant.push(len);
        self.0.platform().wake();
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
}

/// Tells a pipe's writer how much of it was read, and that its reader is
/// gone.
struct Reporter<P: Platform>
where
    P::Handle: Send,
{
    inner: Arc<Inner<P>>,
    pipe: u32,
    report: ReadReport,
    done: bool,
}

impl<P: Platform> ReadObserver for Reporter<P>
where
    P::Handle: Send,
{
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

impl<P: Platform> Drop for Reporter<P>
where
    P::Handle: Send,
{
    fn drop(&mut self) {
        if !self.done {
            let _ = self
                .inner
                .control(&Control::PipeCancelled { pipe: self.pipe }, Vec::new());
        }
    }
}

/// A message for the layer above, on the IO thread.
pub struct Received<'a, P: Platform>
where
    P::Handle: Send,
{
    destination: u32,
    payload: &'a [u8],
    table: &'a mut Table<P::Memory>,
    arrived: &'a mut Vec<(u32, Consumer)>,
}

impl<P: Platform> Received<'_, P>
where
    P::Handle: Send,
{
    /// Which of this end's destinations it is for.
    pub const fn destination(&self) -> u32 {
        self.destination
    }

    /// The message as the type its destination takes; its large bytes
    /// copied out of the peer's regions. An error is a bad message: the
    /// handler returns it, and the channel is closed.
    pub fn decode<T: DeserializeOwned>(&mut self) -> Result<T, BadMessage> {
        codec::decode(self.payload, self.table)
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

/// What the layer above does with a message.
pub trait Handler<P: Platform>
where
    P::Handle: Send,
{
    /// A message from the peer, on the IO thread. An error closes the
    /// channel.
    fn message(&mut self, message: Received<'_, P>) -> Result<(), BadMessage>;
}

/// An endpoint's receiving side: the IO thread's.
pub struct Endpoint<P: Platform>
where
    P::Handle: Send,
{
    inner: Arc<Inner<P>>,
    receiver: Receiver<P>,
    /// The regions the peer writes into.
    table: Table<P::Memory>,
    /// The pipes the peer writes, and where their bytes go.
    incoming: Incoming<Producer>,
    /// The reading ends of pipes the peer opened, until a message names
    /// them.
    arrived: Vec<(u32, Consumer)>,
}

/// An endpoint over `platform`: its sending side, for any thread, and its
/// receiving side, for the IO thread.
pub fn endpoint<P: Platform>(platform: P, config: Config) -> (Link<P>, Endpoint<P>)
where
    P::Handle: Send,
{
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
        incoming: Incoming::new(),
        arrived: Vec::new(),
    };
    (Link(inner), endpoint)
}

impl<P: Platform> Endpoint<P>
where
    P::Handle: Send,
{
    /// The IO thread's loop: waits on the platform and does what there is
    /// to do, until the channel is over; then says why.
    pub fn run(mut self, mut handler: impl Handler<P>) -> Gone {
        let mut bytes = Vec::new();
        let mut handles = Vec::new();
        loop {
            bytes.clear();
            let event = self.inner.platform().wait(&mut bytes, &mut handles);
            if let Some(gone) = self.turn(event, &bytes, &mut handles, &mut handler) {
                return gone;
            }
        }
    }

    /// One turn of the loop, for what one wait came back with. `Some` when
    /// the channel is over: the endpoint is done.
    pub fn turn(
        &mut self,
        event: Event,
        bytes: &[u8],
        handles: &mut Vec<P::Handle>,
        handler: &mut impl Handler<P>,
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
        drop(out);
        self.inner.platform().shut();
    }

    fn read(
        &mut self,
        bytes: &[u8],
        handles: &mut Vec<P::Handle>,
        handler: &mut impl Handler<P>,
    ) -> Option<Gone> {
        let Self {
            inner,
            receiver,
            table,
            incoming,
            arrived,
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
        let mut out = self.inner.out();
        let number = out.next_region;
        let after = number.checked_add(1)?;
        let (memory, handle) = self.inner.platform().create(class, true)?;
        if !self.table.insert(number, memory) {
            return None;
        }
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

/// One of the core's own messages from the peer.
fn control<P: Platform>(
    inner: &Arc<Inner<P>>,
    table: &mut Table<P::Memory>,
    incoming: &mut Incoming<Producer>,
    arrived: &mut Vec<(u32, Consumer)>,
    mut message: Message<'_, P::Handle>,
) -> Result<(), BadMessage>
where
    P::Handle: Send,
{
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
        Control::PipeEnded { pipe } => incoming.ended(pipe).map(drop),
        Control::PipeCancelled { pipe } => {
            // Its source goes at the feeding that follows: a pipe that
            // isn't open has no room and is let go there.
            inner.out().pipes.peer_cancelled(pipe).map(drop)
        }
    }
}
