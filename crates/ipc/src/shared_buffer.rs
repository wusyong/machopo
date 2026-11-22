//! Bytes that cross a channel in shared memory, copied by neither end on
//! the way (Chromium's `mojo_base::BigBuffer` when it is shared memory,
//! with what `base::WritableSharedMemoryMapping` and
//! `base::ReadOnlySharedMemoryMapping` are to the two ends): a
//! [`Writer`] is a region of the channel's taken to be written in place,
//! a tile drawn there; the [`SharedBuffer`] it becomes goes in a message,
//! which names the region; and the buffer the other end reads the message
//! as is that region still, to copy out of once, to where the bytes are
//! wanted (a GPU's buffer). The region is written again when that buffer
//! is dropped.
//!
//! A buffer is also plain bytes of the process's own, for an end with no
//! region free and for a channel within one process: whoever takes one
//! out of a message asks which it is only to save a copy.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use bytes::Bytes;
use serde::de::{self, Deserialize, Deserializer};
use serde::ser::{self, Serialize, Serializer};

use crate::core::region::{self, Memory};

/// A region's mapping, whatever platform made it.
pub(crate) type Mapped = Arc<dyn Memory + Send + Sync>;

/// The channel's end a region is of: what hears that a region taken
/// isn't wanted, and that one read may be written again.
pub(crate) trait Owner: Send + Sync {
    /// A region taken and not sent is the pool's again.
    fn give_back(&self, number: u32);

    /// What the peer wrote in its region `number` was read: the peer is
    /// told.
    fn read(&self, number: u32);
}

/// Which end an owner is: the address of what it shares.
pub(crate) fn owner_id(owner: &Arc<dyn Owner>) -> usize {
    Arc::as_ptr(owner).cast::<()>().addr()
}

/// A region out of its pool to whoever holds this: given back when it
/// goes, unless a message took it.
struct Lent {
    memory: Mapped,
    number: u32,
    len: usize,
    owner: Arc<dyn Owner>,
    /// A message names the region: it is the peer's to say it was read.
    sent: AtomicBool,
}

impl Drop for Lent {
    fn drop(&mut self) {
        if !*self.sent.get_mut() {
            self.owner.give_back(self.number);
        }
    }
}

/// A region of a channel's, taken to be written in place
/// ([`Link::take_region`](crate::endpoint::Link::take_region)): the
/// taker's alone until it is made a [`SharedBuffer`]. Dropped, the region
/// is free again.
pub struct Writer(Lent);

impl Writer {
    pub(crate) fn new(memory: Mapped, number: u32, len: usize, owner: Arc<dyn Owner>) -> Self {
        debug_assert!(len <= memory.len());
        Self(Lent {
            memory,
            number,
            len,
            owner,
            sent: AtomicBool::new(false),
        })
    }

    /// The bytes asked for, to write: what they hold is whatever was
    /// written there before.
    pub fn bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: the region is one of a pool's, so writable, and `len`
        // was checked against it when it was taken. It is out of the pool
        // to this value alone and no message has named it, so nothing
        // else in this process reaches it until `self` is consumed, and
        // `&mut self` makes this the one slice; the peer's mapping of a
        // region this end writes is read only (the system's, for a
        // region a Browser writes; a Browser's own code, for one it gave
        // a renderer).
        unsafe { region::bytes_mut(&*self.0.memory, self.0.len) }
    }

    /// What was written.
    pub fn bytes(&self) -> &[u8] {
        // SAFETY: as in `bytes_mut`: nothing but this value reaches the
        // region, and `&self` keeps `bytes_mut` from being called while
        // the slice lives, so nothing writes it.
        unsafe { region::bytes(&*self.0.memory, self.0.len) }
    }
}

/// A region of the peer's a message named: what the peer wrote there,
/// until this goes and the peer is told it may write again.
struct Held {
    memory: Mapped,
    number: u32,
    len: usize,
    owner: Arc<dyn Owner>,
}

impl Drop for Held {
    fn drop(&mut self) {
        self.owner.read(self.number);
    }
}

/// Bytes for a message, shared with whoever made them rather than copied
/// into it: in a region of the channel's, or the process's own.
#[derive(Clone)]
pub struct SharedBuffer(Repr);

#[derive(Clone)]
enum Repr {
    /// The process's own memory.
    Own(Bytes),
    /// A region this end wrote, for the message that takes it to the
    /// peer. A clone is the same region: it crosses once.
    Lent(Arc<Lent>),
    /// A region the peer wrote, as a message brought it.
    Held(Arc<Held>),
}

impl SharedBuffer {
    /// How many bytes.
    pub fn len(&self) -> usize {
        match &self.0 {
            Repr::Own(bytes) => bytes.len(),
            Repr::Lent(lent) => lent.len,
            Repr::Held(held) => held.len,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The bytes, when they are the process's own. `None` for a region:
    /// its bytes are only copied out ([`SharedBuffer::copy_to`]), because
    /// whoever wrote them still can.
    pub const fn as_bytes(&self) -> Option<&Bytes> {
        match &self.0 {
            Repr::Own(bytes) => Some(bytes),
            Repr::Lent(_) | Repr::Held(_) => None,
        }
    }

    /// The `into.len()` bytes from `offset`, copied out. `false`, with
    /// nothing copied, when they aren't all in the buffer, or the buffer
    /// is a region this end wrote and gave to a message: that one is the
    /// peer's to read, and may be another taker's to write by now.
    pub fn copy_to(&self, offset: usize, into: &mut [u8]) -> bool {
        let within = offset
            .checked_add(into.len())
            .is_some_and(|end| end <= self.len());
        if !within {
            return false;
        }
        match &self.0 {
            Repr::Own(bytes) => {
                let from = bytes.get(offset..).and_then(|rest| rest.get(..into.len()));
                from.is_some_and(|from| {
                    into.copy_from_slice(from);
                    true
                })
            }
            Repr::Lent(_) => false,
            Repr::Held(held) => region::copy_out(&*held.memory, offset, into),
        }
    }
}

/// Runs of `N` bytes of a buffer, by value ([`SharedBuffer::chunks`]).
pub struct Chunks<'a, const N: usize>(ChunksOf<'a, N>);

enum ChunksOf<'a, const N: usize> {
    Own(std::slice::Iter<'a, [u8; N]>),
    Region(region::ChunksOut<'a, N>),
}

impl<const N: usize> Iterator for Chunks<'_, N> {
    type Item = [u8; N];

    #[inline]
    fn next(&mut self) -> Option<[u8; N]> {
        match &mut self.0 {
            ChunksOf::Own(chunks) => chunks.next().copied(),
            ChunksOf::Region(chunks) => chunks.next(),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match &self.0 {
            ChunksOf::Own(chunks) => chunks.size_hint(),
            ChunksOf::Region(chunks) => chunks.size_hint(),
        }
    }
}

impl<const N: usize> ExactSizeIterator for Chunks<'_, N> {}

impl SharedBuffer {
    /// The `count` runs of `N` bytes from `offset`, each by value as it
    /// is asked for: how the bytes go, once, to somewhere that is only
    /// written (a GPU's mapped buffer, which gives no slice to copy
    /// into). `None` wherever [`SharedBuffer::copy_to`] copies nothing.
    pub fn chunks<const N: usize>(&self, offset: usize, count: usize) -> Option<Chunks<'_, N>> {
        let end = count.checked_mul(N)?.checked_add(offset)?;
        if end > self.len() {
            return None;
        }
        match &self.0 {
            Repr::Own(bytes) => {
                let (chunks, _) = bytes.get(offset..end)?.as_chunks::<N>();
                Some(Chunks(ChunksOf::Own(chunks.iter())))
            }
            Repr::Lent(_) => None,
            Repr::Held(held) => region::chunks_out(&*held.memory, offset, count)
                .map(|c| Chunks(ChunksOf::Region(c))),
        }
    }
}

impl Default for SharedBuffer {
    fn default() -> Self {
        Self(Repr::Own(Bytes::new()))
    }
}

impl From<Bytes> for SharedBuffer {
    fn from(bytes: Bytes) -> Self {
        Self(Repr::Own(bytes))
    }
}

/// What was written in the region, for a message: nothing writes it from
/// here on.
impl From<Writer> for SharedBuffer {
    fn from(writer: Writer) -> Self {
        Self(Repr::Lent(Arc::new(writer.0)))
    }
}

impl fmt::Debug for SharedBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match &self.0 {
            Repr::Own(_) => "own",
            Repr::Lent(_) => "a region to send",
            Repr::Held(_) => "a region of the peer's",
        };
        f.debug_struct("SharedBuffer")
            .field("len", &self.len())
            .field("in", &kind)
            .finish()
    }
}

/// What a serializer is told a buffer in a region is (a newtype struct of
/// this name), and what a deserializer is asked for a buffer by: how this
/// crate's codec knows one, in a value of any type.
pub(crate) const LENT_NAME: &str = "$ipc::shared_buffer::Lent";
pub(crate) const BUFFER_NAME: &str = "$ipc::shared_buffer::SharedBuffer";

/// A region on its way into a message: whose it is, its number, and how
/// many of its bytes.
pub(crate) type Lending = (usize, u32, usize);

thread_local! {
    /// The region a value being serialized is giving to its message, for
    /// the codec, and the one the codec has for the value it is reading:
    /// serde's data model has no place for either, so they go beside it,
    /// for the length of one call on the one thread making it.
    static LENDING: Cell<Option<Lending>> = const { Cell::new(None) };
    static ARRIVING: RefCell<Option<SharedBuffer>> = const { RefCell::new(None) };
}

/// The region a value being serialized is giving to its message.
pub(crate) fn take_lending() -> Option<Lending> {
    LENDING.take()
}

/// Put back by a codec whose channel the region isn't of: the value
/// keeps it.
pub(crate) fn set_lending(lending: Lending) {
    LENDING.set(Some(lending));
}

impl SharedBuffer {
    /// The first `len` bytes of the peer's region `number`, as a message
    /// brought them: `owner` hears when the buffer goes.
    pub(crate) fn held(memory: Mapped, number: u32, len: usize, owner: Arc<dyn Owner>) -> Self {
        debug_assert!(len <= memory.len());
        Self(Repr::Held(Arc::new(Held {
            memory,
            number,
            len,
            owner,
        })))
    }
}

/// The peer's region `number` as the buffer a message brought, for the
/// value the codec is about to hand its visitor.
pub(crate) fn set_arriving(memory: Mapped, number: u32, len: usize, owner: Arc<dyn Owner>) {
    let buffer = SharedBuffer::held(memory, number, len, owner);
    ARRIVING.with(|arriving| *arriving.borrow_mut() = Some(buffer));
}

pub(crate) fn take_arriving() -> Option<SharedBuffer> {
    ARRIVING.with(|arriving| arriving.borrow_mut().take())
}

/// What a serializer that isn't the codec gets to write of a region:
/// nothing.
struct OnlyInAMessage;

impl Serialize for OnlyInAMessage {
    fn serialize<S: Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        Err(ser::Error::custom(
            "a buffer in a region is serialized only into a message of its channel",
        ))
    }
}

/// Bytes written as bytes.
struct Raw<'a>(&'a [u8]);

impl Serialize for Raw<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(self.0)
    }
}

/// The process's own bytes are written as any bytes are. A region this
/// end wrote goes to the message of its own channel that takes it, once:
/// any other serializer, another channel and a second message are
/// refused, and the first two leave the region the buffer's. A region
/// the peer wrote isn't sent on.
impl Serialize for SharedBuffer {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let lent = match &self.0 {
            Repr::Own(bytes) => return Raw(bytes).serialize(serializer),
            Repr::Lent(lent) => lent,
            Repr::Held(_) => {
                return Err(ser::Error::custom("a region of the peer's isn't sent on"));
            }
        };
        if lent.sent.swap(true, Ordering::AcqRel) {
            return Err(ser::Error::custom("a region crosses once"));
        }
        set_lending((owner_id(&lent.owner), lent.number, lent.len));
        let written = serializer.serialize_newtype_struct(LENT_NAME, &OnlyInAMessage);
        // Not taken: the serializer wasn't its channel's codec.
        if take_lending().is_some() {
            lent.sent.store(false, Ordering::Release);
        }
        written
    }
}

/// The bytes a message holds, or the region it names, which only this
/// crate's codec has to give.
impl<'de> Deserialize<'de> for SharedBuffer {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Arrived;

        impl<'de> de::Visitor<'de> for Arrived {
            type Value = SharedBuffer;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("bytes, or a region an ipc message names")
            }

            fn visit_bytes<E: de::Error>(self, bytes: &[u8]) -> Result<SharedBuffer, E> {
                Ok(Bytes::copy_from_slice(bytes).into())
            }

            fn visit_byte_buf<E: de::Error>(self, bytes: Vec<u8>) -> Result<SharedBuffer, E> {
                Ok(Bytes::from(bytes).into())
            }

            fn visit_u32<E: de::Error>(self, _number: u32) -> Result<SharedBuffer, E> {
                take_arriving()
                    .ok_or_else(|| E::custom("a region is read only out of an ipc message"))
            }

            // A deserializer that isn't the codec: the bytes are there.
            fn visit_newtype_struct<D: Deserializer<'de>>(
                self,
                deserializer: D,
            ) -> Result<SharedBuffer, D::Error> {
                deserializer.deserialize_byte_buf(self)
            }
        }

        deserializer.deserialize_newtype_struct(BUFFER_NAME, Arrived)
    }
}
