//! A message on a channel as bytes (Chromium's `mojo::core::Channel::
//! Message`): a fixed header, then the payload.
//!
//! | bytes | field |
//! | --- | --- |
//! | 0..4 | the payload's length |
//! | 4..8 | the destination |
//! | 8..16 | the sequence number |
//! | 16..20 | how many handles go with it |
//!
//! All little-endian. An end numbers what it sends from 0 up by one; the
//! other end takes nothing out of that order.

use std::collections::VecDeque;
use std::fmt;

use base::SequenceNumber;

/// The header's length.
pub const HEADER_LEN: usize = 20;

/// The longest payload (Chromium's `kMaximumMessageSize` is as much).
/// Large bytes go beside a message, not in it; this is for what doesn't
/// (a script's source in a string), and a peer that says more is broken.
pub const MAX_PAYLOAD_LEN: u32 = 128 << 20;

/// The most handles one message brings: a region or two is what one does.
pub const MAX_HANDLES: u32 = 16;

/// What a peer did that a working one doesn't: its channel is closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadMessage {
    /// A payload longer than [`MAX_PAYLOAD_LEN`].
    TooLong { len: u32 },
    /// Not the sequence number after the last one's.
    OutOfOrder { expected: u64, got: u64 },
    /// Handles from an end that may attach none.
    HandlesNotAllowed,
    /// More handles than [`MAX_HANDLES`] with one message, or waiting for
    /// messages that don't claim them.
    TooManyHandles,
    /// A message whose handles didn't come with it.
    MissingHandles { expected: u32, got: usize },
    /// The message after the last sequence number there is.
    SequenceSpent,
    /// A destination this end has nothing at.
    UnknownDestination { destination: u32 },
    /// A payload whoever it was for could make nothing of.
    Rejected,
    /// A value nested deeper than a message's may be.
    TooDeep,
    /// A region that isn't one of this peer's.
    UnknownRegion { number: u32 },
    /// More bytes than the region named has.
    OutsideRegion { number: u32, len: u32 },
    /// A region given back that nothing was sent in.
    RegionNotOut { number: u32 },
}

impl fmt::Display for BadMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLong { len } => write!(f, "a payload of {len} bytes"),
            Self::OutOfOrder { expected, got } => {
                write!(f, "message {got} where {expected} was due")
            }
            Self::HandlesNotAllowed => f.write_str("handles from an end that may attach none"),
            Self::TooManyHandles => f.write_str("too many handles"),
            Self::MissingHandles { expected, got } => {
                write!(f, "{got} handles where {expected} were due")
            }
            Self::SequenceSpent => f.write_str("a message after the last sequence number"),
            Self::UnknownDestination { destination } => {
                write!(f, "a message for destination {destination}")
            }
            Self::Rejected => f.write_str("a payload its destination refused"),
            Self::TooDeep => f.write_str("a value nested too deep"),
            Self::UnknownRegion { number } => write!(f, "region {number}, which isn't this peer's"),
            Self::OutsideRegion { number, len } => {
                write!(f, "{len} bytes of region {number}, which is shorter")
            }
            Self::RegionNotOut { number } => {
                write!(f, "region {number} given back with nothing sent in it")
            }
        }
    }
}

impl std::error::Error for BadMessage {}

/// Why a message wasn't written: this end's own mistake, found before the
/// peer would close the channel over it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodeError {
    /// A payload longer than [`MAX_PAYLOAD_LEN`].
    TooLong { len: usize },
    /// More handles than [`MAX_HANDLES`].
    TooManyHandles { handles: usize },
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLong { len } => write!(f, "a payload of {len} bytes"),
            Self::TooManyHandles { handles } => write!(f, "{handles} handles with one message"),
        }
    }
}

impl std::error::Error for EncodeError {}

/// A message's header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Header {
    len: u32,
    destination: u32,
    sequence: u64,
    handles: u32,
}

impl Header {
    fn write(self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.len.to_le_bytes());
        out.extend_from_slice(&self.destination.to_le_bytes());
        out.extend_from_slice(&self.sequence.to_le_bytes());
        out.extend_from_slice(&self.handles.to_le_bytes());
    }

    fn read(bytes: &[u8; HEADER_LEN]) -> Self {
        let [
            l0,
            l1,
            l2,
            l3,
            d0,
            d1,
            d2,
            d3,
            s0,
            s1,
            s2,
            s3,
            s4,
            s5,
            s6,
            s7,
            h0,
            h1,
            h2,
            h3,
        ] = *bytes;
        Self {
            len: u32::from_le_bytes([l0, l1, l2, l3]),
            destination: u32::from_le_bytes([d0, d1, d2, d3]),
            sequence: u64::from_le_bytes([s0, s1, s2, s3, s4, s5, s6, s7]),
            handles: u32::from_le_bytes([h0, h1, h2, h3]),
        }
    }
}

/// The sending end's side of the framing: it numbers the messages.
#[derive(Debug, Default)]
pub struct Encoder {
    sequence: SequenceNumber<u64>,
}

impl Encoder {
    pub const fn new() -> Self {
        Self {
            sequence: SequenceNumber::new(),
        }
    }

    /// Appends the next message to `out`: its header, then `payload`.
    /// `handles` is how many go with it, which the platform's layer sends
    /// along. Nothing is appended, and no number is used, if it can't be
    /// sent.
    pub fn encode(
        &mut self,
        destination: u32,
        payload: &[u8],
        handles: usize,
        out: &mut Vec<u8>,
    ) -> Result<(), EncodeError> {
        let len = u32::try_from(payload.len())
            .ok()
            .filter(|&len| len <= MAX_PAYLOAD_LEN)
            .ok_or(EncodeError::TooLong { len: payload.len() })?;
        let handles = u32::try_from(handles)
            .ok()
            .filter(|&handles| handles <= MAX_HANDLES)
            .ok_or(EncodeError::TooManyHandles { handles })?;
        Header {
            len,
            destination,
            sequence: self.sequence.get_next(),
            handles,
        }
        .write(out);
        out.extend_from_slice(payload);
        Ok(())
    }
}

/// A message read whole: its payload still where it was read.
#[derive(Debug)]
pub struct Message<'a, H> {
    pub destination: u32,
    pub sequence: u64,
    pub payload: &'a [u8],
    pub handles: Vec<H>,
}

/// The receiving end's side of the framing. It is fed what the platform's
/// layer read, in whatever pieces it came (a socket is a stream; a Mach
/// message is one message whole), and gives out each message once it is
/// all there, having checked its header as soon as that was.
///
/// After a [`BadMessage`] it takes nothing more and holds nothing: the
/// bytes and the handles it had are dropped, and every later call gives
/// the same error.
#[derive(Debug)]
pub struct Decoder<H> {
    /// The start of a message whose rest hasn't come. It grows with what
    /// has come, never with what a header says will.
    partial: Vec<u8>,
    /// Handles read and not yet claimed by a message, in the order read.
    handles: VecDeque<H>,
    /// The sequence number due next; `None` once the last was taken.
    next_sequence: Option<u64>,
    /// The peer's end may attach handles.
    peer_may_attach: bool,
    failed: Option<BadMessage>,
}

impl<H> Decoder<H> {
    pub const fn new(peer_may_attach: bool) -> Self {
        Self {
            partial: Vec::new(),
            handles: VecDeque::new(),
            next_sequence: Some(0),
            peer_may_attach,
            failed: None,
        }
    }

    /// Takes in `bytes` and `handles` as read, and gives `sink` each
    /// message they complete, in order.
    pub fn feed(
        &mut self,
        bytes: &[u8],
        handles: impl IntoIterator<Item = H>,
        mut sink: impl FnMut(Message<'_, H>),
    ) -> Result<(), BadMessage> {
        self.try_feed(bytes, handles, |message| {
            sink(message);
            Ok(())
        })
    }

    /// `feed` with a `sink` that may refuse a message: that message is
    /// then the bad one, and none after it is given out.
    pub fn try_feed(
        &mut self,
        bytes: &[u8],
        handles: impl IntoIterator<Item = H>,
        mut sink: impl FnMut(Message<'_, H>) -> Result<(), BadMessage>,
    ) -> Result<(), BadMessage> {
        // Taken before anything is checked, so that a failure drops them
        // with the rest.
        self.handles.extend(handles);
        let result = self.feed_checked(bytes, &mut sink);
        if let Err(bad) = result {
            self.failed = Some(bad);
            self.partial = Vec::new();
            self.handles.clear();
        }
        result
    }

    fn feed_checked(
        &mut self,
        bytes: &[u8],
        sink: &mut dyn FnMut(Message<'_, H>) -> Result<(), BadMessage>,
    ) -> Result<(), BadMessage> {
        if let Some(bad) = self.failed {
            return Err(bad);
        }
        if !self.peer_may_attach && !self.handles.is_empty() {
            return Err(BadMessage::HandlesNotAllowed);
        }
        // More than any messages in flight could claim: one a message's
        // worth for the message being read and one for the next.
        if self.handles.len() > (MAX_HANDLES as usize).saturating_mul(2) {
            return Err(BadMessage::TooManyHandles);
        }
        if self.partial.is_empty() {
            // Read where the bytes are; only an unfinished message's
            // start is kept.
            let rest = self.take_messages(bytes, sink)?;
            self.partial.extend_from_slice(rest);
        } else {
            let mut partial = std::mem::take(&mut self.partial);
            partial.extend_from_slice(bytes);
            let rest = self.take_messages(&partial, sink)?.len();
            let taken = partial.len().saturating_sub(rest);
            partial.drain(..taken);
            self.partial = partial;
        }
        Ok(())
    }

    /// Gives `sink` each whole message at the start of `bytes`; what is
    /// left is the start of one that isn't whole, its header checked if
    /// it is there.
    fn take_messages<'a>(
        &mut self,
        mut bytes: &'a [u8],
        sink: &mut dyn FnMut(Message<'_, H>) -> Result<(), BadMessage>,
    ) -> Result<&'a [u8], BadMessage> {
        while let Some((header, after)) = bytes.split_first_chunk::<HEADER_LEN>() {
            let header = Header::read(header);
            if header.len > MAX_PAYLOAD_LEN {
                return Err(BadMessage::TooLong { len: header.len });
            }
            let expected = self.next_sequence.ok_or(BadMessage::SequenceSpent)?;
            if header.sequence != expected {
                return Err(BadMessage::OutOfOrder {
                    expected,
                    got: header.sequence,
                });
            }
            if header.handles != 0 && !self.peer_may_attach {
                return Err(BadMessage::HandlesNotAllowed);
            }
            if header.handles > MAX_HANDLES {
                return Err(BadMessage::TooManyHandles);
            }
            let Some((payload, rest)) = usize::try_from(header.len)
                .ok()
                .and_then(|len| after.split_at_checked(len))
            else {
                // Not all there yet.
                break;
            };
            // A message's handles are read with its first bytes, so they
            // are here by its last.
            let count = usize::try_from(header.handles).unwrap_or(usize::MAX);
            if self.handles.len() < count {
                return Err(BadMessage::MissingHandles {
                    expected: header.handles,
                    got: self.handles.len(),
                });
            }
            let handles = self.handles.drain(..count).collect();
            self.next_sequence = expected.checked_add(1);
            sink(Message {
                destination: header.destination,
                sequence: header.sequence,
                payload,
                handles,
            })?;
            bytes = rest;
        }
        Ok(bytes)
    }

    /// How many bytes of an unfinished message are held.
    pub fn buffered(&self) -> usize {
        self.partial.len()
    }

    /// How many handles are held for messages not yet whole.
    pub fn handles_held(&self) -> usize {
        self.handles.len()
    }
}

#[cfg(test)]
impl<H> Decoder<H> {
    /// One whose next message is `sequence`.
    fn expecting(sequence: u64, peer_may_attach: bool) -> Self {
        Self {
            next_sequence: Some(sequence),
            ..Self::new(peer_may_attach)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::core::fake;

    /// A handle that says which it is, and counts itself when dropped (as
    /// a real one is closed).
    #[derive(Debug)]
    struct TestHandle(u32, Arc<AtomicUsize>);

    impl Drop for TestHandle {
        #[expect(
            clippy::disallowed_methods,
            reason = "a test's count of drops: a few thousand"
        )]
        fn drop(&mut self) {
            self.1.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// What a message was, to compare.
    type Seen = (u32, u64, Vec<u8>, Vec<u32>);

    fn seen(message: Message<'_, TestHandle>) -> Seen {
        let handles = message.handles.iter().map(|handle| handle.0).collect();
        (
            message.destination,
            message.sequence,
            message.payload.to_vec(),
            handles,
        )
    }

    fn header(len: u32, destination: u32, sequence: u64, handles: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        Header {
            len,
            destination,
            sequence,
            handles,
        }
        .write(&mut bytes);
        bytes
    }

    /// Three messages as one run of bytes: one with a handle, one empty,
    /// one with two handles.
    fn three() -> (Vec<u8>, Vec<Seen>) {
        let mut encoder = Encoder::new();
        let mut bytes = Vec::new();
        encoder.encode(7, b"hello", 1, &mut bytes).unwrap();
        encoder.encode(0, b"", 0, &mut bytes).unwrap();
        encoder.encode(u32::MAX, &[9; 300], 2, &mut bytes).unwrap();
        let expected = vec![
            (7, 0, b"hello".to_vec(), vec![10]),
            (0, 1, Vec::new(), vec![]),
            (u32::MAX, 2, vec![9; 300], vec![11, 12]),
        ];
        (bytes, expected)
    }

    fn handles(
        numbers: impl IntoIterator<Item = u32>,
        dropped: &Arc<AtomicUsize>,
    ) -> Vec<TestHandle> {
        numbers
            .into_iter()
            .map(|number| TestHandle(number, dropped.clone()))
            .collect()
    }

    #[test]
    fn the_header_is_twenty_bytes_little_endian() {
        let mut bytes = Vec::new();
        Encoder::new()
            .encode(0x0403_0201, b"xy", 3, &mut bytes)
            .unwrap();
        assert_eq!(
            bytes,
            [
                2, 0, 0, 0, 1, 2, 3, 4, 0, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, b'x', b'y'
            ]
        );
        assert_eq!(bytes.len(), HEADER_LEN + 2);
        let read = Header::read(bytes.first_chunk().unwrap());
        assert_eq!(
            read,
            Header {
                len: 2,
                destination: 0x0403_0201,
                sequence: 0,
                handles: 3
            }
        );
        let far = header(1, 2, 0x0807_0605_0403_0201, 4);
        assert_eq!(far[8..16], [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(
            Header::read(far.first_chunk().unwrap()).sequence,
            0x0807_0605_0403_0201
        );
    }

    /// Messages come out as they went in, numbered from 0, each with its
    /// own handles, wherever the bytes were cut.
    #[test]
    fn messages_come_out_whole_wherever_the_bytes_are_cut() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let (bytes, expected) = three();
        // All at once.
        let mut decoder = Decoder::new(true);
        let mut got = Vec::new();
        decoder
            .feed(&bytes, handles(10..=12, &dropped), |message| {
                got.push(seen(message))
            })
            .unwrap();
        assert_eq!(got, expected);
        assert_eq!((decoder.buffered(), decoder.handles_held()), (0, 0));
        // In two pieces, cut at every place; each message's handles come
        // with the piece its first byte is in.
        let starts = [0, HEADER_LEN + 5, 2 * HEADER_LEN + 5];
        for cut in 0..=bytes.len() {
            let mut decoder = Decoder::new(true);
            let mut got = Vec::new();
            let with = |range: std::ops::Range<usize>| {
                let numbers = [(starts[0], vec![10]), (starts[2], vec![11, 12])]
                    .into_iter()
                    .filter(|(start, _)| range.contains(start))
                    .flat_map(|(_, numbers)| numbers);
                handles(numbers, &dropped)
            };
            decoder
                .feed(&bytes[..cut], with(0..cut), |message| {
                    got.push(seen(message))
                })
                .unwrap();
            decoder
                .feed(&bytes[cut..], with(cut..bytes.len()), |message| {
                    got.push(seen(message))
                })
                .unwrap();
            assert_eq!(got, expected, "cut at {cut}");
            assert_eq!((decoder.buffered(), decoder.handles_held()), (0, 0));
        }
        // A byte at a time.
        let mut decoder = Decoder::new(true);
        let mut got = Vec::new();
        for (at, byte) in bytes.iter().enumerate() {
            let with = match at {
                0 => handles([10], &dropped),
                at if at == starts[2] => handles([11, 12], &dropped),
                _ => Vec::new(),
            };
            decoder
                .feed(&[*byte], with, |message| got.push(seen(message)))
                .unwrap();
        }
        assert_eq!(got, expected);
    }

    /// A message is given out as soon as its last byte is in, and the
    /// start of the next is kept, no more.
    #[test]
    fn only_an_unfinished_message_is_kept() {
        let (bytes, expected) = three();
        let mut decoder = Decoder::<TestHandle>::new(true);
        let dropped = Arc::new(AtomicUsize::new(0));
        let mut got = Vec::new();
        // The first message and half of the second's header.
        let cut = HEADER_LEN + 5 + 10;
        decoder
            .feed(&bytes[..cut], handles([10], &dropped), |message| {
                got.push(seen(message))
            })
            .unwrap();
        assert_eq!(got, expected[..1]);
        assert_eq!(decoder.buffered(), 10);
        assert_eq!(decoder.handles_held(), 0);
    }

    /// The header is checked when it is there, before its payload is: a
    /// length past the limit is refused at once.
    #[test]
    fn a_length_past_the_limit_is_refused_by_the_header_alone() {
        let mut decoder = Decoder::<TestHandle>::new(true);
        let none = |_: Message<'_, TestHandle>| panic!("no message");
        assert_eq!(
            decoder.feed(&header(MAX_PAYLOAD_LEN, 0, 0, 0), [], none),
            Ok(())
        );
        assert_eq!(decoder.buffered(), HEADER_LEN);
        let mut decoder = Decoder::<TestHandle>::new(true);
        let over = MAX_PAYLOAD_LEN + 1;
        assert_eq!(
            decoder.feed(&header(over, 0, 0, 0), [], none),
            Err(BadMessage::TooLong { len: over })
        );
        assert_eq!(decoder.buffered(), 0);
    }

    /// What is kept grows with what came, not with what the header says
    /// will: a peer that says 128 MiB and sends ten bytes costs ten bytes.
    #[test]
    fn a_length_that_lies_costs_no_memory() {
        let mut decoder = Decoder::<TestHandle>::new(true);
        let mut bytes = header(MAX_PAYLOAD_LEN, 0, 0, 0);
        bytes.extend_from_slice(&[1; 10]);
        decoder.feed(&bytes, [], |_| panic!("no message")).unwrap();
        assert_eq!(decoder.buffered(), HEADER_LEN + 10);
        assert!(
            decoder.partial.capacity() < 1024,
            "{} bytes kept",
            decoder.partial.capacity()
        );
    }

    #[test]
    fn a_message_out_of_order_is_bad() {
        let none = |_: Message<'_, TestHandle>| panic!("no message");
        // One skipped.
        let mut decoder = Decoder::<TestHandle>::new(true);
        assert_eq!(
            decoder.feed(&header(0, 0, 1, 0), [], none),
            Err(BadMessage::OutOfOrder {
                expected: 0,
                got: 1
            })
        );
        // One again.
        let mut decoder = Decoder::<TestHandle>::new(true);
        let mut count = 0;
        decoder
            .feed(&header(0, 0, 0, 0), [], |_| count += 1)
            .unwrap();
        assert_eq!(
            decoder.feed(&header(0, 0, 0, 0), [], none),
            Err(BadMessage::OutOfOrder {
                expected: 1,
                got: 0
            })
        );
        assert_eq!(count, 1);
        // The messages before a bad one in the same bytes are given out.
        let mut decoder = Decoder::<TestHandle>::new(true);
        let mut bytes = header(0, 5, 0, 0);
        bytes.extend(header(0, 6, 2, 0));
        let mut got = Vec::new();
        assert_eq!(
            decoder.feed(&bytes, [], |message| got.push(message.destination)),
            Err(BadMessage::OutOfOrder {
                expected: 1,
                got: 2
            })
        );
        assert_eq!(got, [5]);
    }

    /// An end that may attach nothing leaves this end no handle: not by a
    /// header that claims one, not by one sent beside a header that
    /// claims none.
    #[test]
    fn an_end_that_may_not_attach_leaves_no_handle() {
        let none = |_: Message<'_, TestHandle>| panic!("no message");
        let dropped = Arc::new(AtomicUsize::new(0));
        let mut decoder = Decoder::new(false);
        assert_eq!(
            decoder.feed(&header(0, 0, 0, 1), handles([1], &dropped), none),
            Err(BadMessage::HandlesNotAllowed)
        );
        assert_eq!(decoder.handles_held(), 0);
        assert_eq!(dropped.load(Ordering::Relaxed), 1);

        let mut decoder = Decoder::new(false);
        assert_eq!(
            decoder.feed(&header(0, 0, 0, 0), handles([1, 2], &dropped), none),
            Err(BadMessage::HandlesNotAllowed)
        );
        assert_eq!(decoder.handles_held(), 0);
        assert_eq!(dropped.load(Ordering::Relaxed), 3);

        // A header that claims one that didn't come is as bad.
        let mut decoder = Decoder::<TestHandle>::new(false);
        assert_eq!(
            decoder.feed(&header(0, 0, 0, 1), [], none),
            Err(BadMessage::HandlesNotAllowed)
        );

        // With none it is a message like any other.
        let mut decoder = Decoder::<TestHandle>::new(false);
        let mut count = 0;
        decoder
            .feed(&header(0, 0, 0, 0), [], |_| count += 1)
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn too_many_handles_are_bad() {
        let none = |_: Message<'_, TestHandle>| panic!("no message");
        let dropped = Arc::new(AtomicUsize::new(0));
        // With one message: as many as the limit are taken.
        let mut decoder = Decoder::new(true);
        let mut got = 0;
        decoder
            .feed(
                &header(0, 0, 0, MAX_HANDLES),
                handles(0..MAX_HANDLES, &dropped),
                |message| {
                    got = message.handles.len();
                },
            )
            .unwrap();
        assert_eq!(got, MAX_HANDLES as usize);
        let mut decoder = Decoder::new(true);
        assert_eq!(
            decoder.feed(
                &header(0, 0, 0, MAX_HANDLES + 1),
                handles(0..=MAX_HANDLES, &dropped),
                none
            ),
            Err(BadMessage::TooManyHandles)
        );
        // Beside messages that don't claim them: they don't pile up.
        let mut decoder = Decoder::new(true);
        let before = dropped.load(Ordering::Relaxed);
        assert_eq!(
            decoder.feed(&[], handles(0..MAX_HANDLES * 2, &dropped), none),
            Ok(())
        );
        assert_eq!(decoder.handles_held(), (MAX_HANDLES * 2) as usize);
        assert_eq!(
            decoder.feed(&[], handles([0], &dropped), none),
            Err(BadMessage::TooManyHandles)
        );
        assert_eq!(decoder.handles_held(), 0);
        assert_eq!(
            dropped.load(Ordering::Relaxed) - before,
            (MAX_HANDLES * 2 + 1) as usize
        );
    }

    #[test]
    fn a_message_without_its_handles_is_bad() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let mut decoder = Decoder::new(true);
        assert_eq!(
            decoder.feed(&header(0, 0, 0, 2), handles([1], &dropped), |_| panic!(
                "no message"
            )),
            Err(BadMessage::MissingHandles {
                expected: 2,
                got: 1
            })
        );
        assert_eq!(dropped.load(Ordering::Relaxed), 1);
    }

    /// After a bad message nothing more is taken or kept, and the answer
    /// stays the same.
    #[test]
    fn nothing_follows_a_bad_message() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let mut decoder = Decoder::new(true);
        // A good start of a message, kept; then a bad one after it.
        let mut bytes = header(4, 0, 0, 0);
        bytes.extend_from_slice(&[1, 2]);
        decoder
            .feed(&bytes, handles([1], &dropped), |_| panic!("no message"))
            .unwrap();
        assert_eq!(
            (decoder.buffered(), decoder.handles_held()),
            (HEADER_LEN + 2, 1)
        );
        let mut rest = vec![3, 4];
        rest.extend(header(0, 0, 9, 0));
        let mut count = 0;
        let bad = BadMessage::OutOfOrder {
            expected: 1,
            got: 9,
        };
        assert_eq!(decoder.feed(&rest, [], |_| count += 1), Err(bad));
        assert_eq!(count, 1);
        assert_eq!((decoder.buffered(), decoder.handles_held()), (0, 0));
        assert_eq!(decoder.partial.capacity(), 0);
        assert_eq!(dropped.load(Ordering::Relaxed), 1);
        // A good message now is refused the same way, its handles dropped.
        assert_eq!(
            decoder.feed(&header(0, 0, 1, 0), handles([2], &dropped), |_| panic!(
                "no message"
            )),
            Err(bad)
        );
        assert_eq!((decoder.buffered(), decoder.handles_held()), (0, 0));
        assert_eq!(dropped.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn the_last_sequence_number_is_the_last_message() {
        let mut decoder = Decoder::<TestHandle>::expecting(u64::MAX, true);
        let mut count = 0;
        decoder
            .feed(&header(0, 0, u64::MAX, 0), [], |_| count += 1)
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(
            decoder.feed(&header(0, 0, 0, 0), [], |_| panic!("no message")),
            Err(BadMessage::SequenceSpent)
        );
    }

    /// The sending end refuses what the receiving end would, and a
    /// message refused takes no number.
    #[test]
    fn the_encoder_refuses_what_the_peer_would() {
        let mut encoder = Encoder::new();
        let mut bytes = Vec::new();
        let handles = MAX_HANDLES as usize + 1;
        assert_eq!(
            encoder.encode(0, b"", handles, &mut bytes),
            Err(EncodeError::TooManyHandles { handles })
        );
        assert!(bytes.is_empty());
        encoder
            .encode(0, b"", MAX_HANDLES as usize, &mut bytes)
            .unwrap();
        assert_eq!(Header::read(bytes.first_chunk().unwrap()).sequence, 0);
    }

    /// Too slow under Miri: the payload is 128 MiB (never touched: the
    /// length is refused first).
    #[test]
    #[cfg_attr(miri, ignore)]
    fn the_encoder_refuses_a_payload_past_the_limit() {
        let mut encoder = Encoder::new();
        let mut bytes = Vec::new();
        let payload = vec![0u8; MAX_PAYLOAD_LEN as usize + 1];
        assert_eq!(
            encoder.encode(0, &payload, 0, &mut bytes),
            Err(EncodeError::TooLong { len: payload.len() })
        );
        assert!(bytes.is_empty());
        // One at the limit goes, and is the first.
        encoder.encode(0, &payload[1..], 0, &mut bytes).unwrap();
        let header = Header::read(bytes.first_chunk().unwrap());
        assert_eq!((header.len, header.sequence), (MAX_PAYLOAD_LEN, 0));
    }

    /// Two threads and a transport that cuts the bytes where it likes:
    /// what one end sends the other gets, each once, in order.
    #[test]
    fn messages_cross_a_transport_in_order() {
        let count: u32 = if cfg!(miri) { 40 } else { 2_000 };
        let dropped = Arc::new(AtomicUsize::new(0));
        for (seed, longest) in [(1, 1), (2, 7), (3, 64), (4, 4096)] {
            let (mut sender, mut receiver) = fake::one_way::<TestHandle>(seed, longest);
            let for_sender = dropped.clone();
            let payload = |number: u32| vec![number as u8; (number as usize * 37) % 301];
            let sending = std::thread::spawn(move || {
                let mut encoder = Encoder::new();
                let mut bytes = Vec::new();
                for number in 0..count {
                    bytes.clear();
                    let with = handles((0..number % 3).map(|nth| number * 10 + nth), &for_sender);
                    encoder
                        .encode(number, &payload(number), with.len(), &mut bytes)
                        .unwrap();
                    sender.write(&bytes, with);
                }
            });
            let mut decoder = Decoder::new(true);
            let mut next = 0u32;
            while let Some((bytes, with)) = receiver.read() {
                decoder
                    .feed(&bytes, with, |message| {
                        assert_eq!(message.destination, next);
                        assert_eq!(message.sequence, u64::from(next));
                        assert_eq!(message.payload, payload(next));
                        let numbers: Vec<u32> =
                            message.handles.iter().map(|handle| handle.0).collect();
                        assert_eq!(
                            numbers,
                            (0..next % 3).map(|nth| next * 10 + nth).collect::<Vec<_>>()
                        );
                        next += 1;
                    })
                    .unwrap();
            }
            sending.join().unwrap();
            assert_eq!(next, count);
            assert_eq!((decoder.buffered(), decoder.handles_held()), (0, 0));
        }
    }
}
