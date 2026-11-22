//! The core's own messages, at a destination no layer above has
//! ([`CONTROL_DESTINATION`]): what hands a region over and says one may be
//! written again ([`super::region`]), and a pipe's bytes and its credit
//! ([`super::pipe`]). They are bytes laid out by hand, a tag and
//! little-endian fields, so the core needs no serializer to read what a
//! peer sent it.

use super::frame::BadMessage;

/// The destination of the core's own messages.
pub const CONTROL_DESTINATION: u32 = u32::MAX;

/// One of the core's own messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Control<'a> {
    /// The regions the sender has read what was in: each may be written
    /// again.
    Released(Vec<u32>),
    /// A region the receiver has from now on, its handle with the
    /// message: `writable` for one the receiver writes into (its pool's),
    /// otherwise one it reads (its table's).
    Granted {
        number: u32,
        len: u32,
        writable: bool,
    },
    /// The sender had no free region for `len` bytes and would have one
    /// more: for the end that makes regions, which grants it or doesn't.
    Wanted { len: u32 },
    /// A pipe the sender will write: the receiver holds at most `capacity`
    /// of its bytes unread.
    PipeOpened { pipe: u32, capacity: u32 },
    /// The pipe's next bytes.
    PipeData { pipe: u32, bytes: &'a [u8] },
    /// The pipe's reader read `count` more bytes: its writer may send
    /// that many more.
    PipeRead { pipe: u32, count: u64 },
    /// The pipe's writer is done: what was sent is all of it.
    PipeEnded { pipe: u32 },
    /// The pipe's reader wants no more of it.
    PipeCancelled { pipe: u32 },
}

const RELEASED: u8 = 0;
const GRANTED: u8 = 1;
const PIPE_OPENED: u8 = 2;
const PIPE_DATA: u8 = 3;
const PIPE_READ: u8 = 4;
const PIPE_ENDED: u8 = 5;
const PIPE_CANCELLED: u8 = 6;
const WANTED: u8 = 7;

fn word(bytes: &[u8]) -> Result<u32, BadMessage> {
    bytes
        .try_into()
        .map(u32::from_le_bytes)
        .map_err(|_| BadMessage::Rejected)
}

impl<'a> Control<'a> {
    /// The message's payload, after what `out` has.
    pub fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Self::Released(numbers) => {
                out.push(RELEASED);
                for number in numbers {
                    out.extend_from_slice(&number.to_le_bytes());
                }
            }
            Self::Granted {
                number,
                len,
                writable,
            } => {
                out.push(GRANTED);
                out.extend_from_slice(&number.to_le_bytes());
                out.extend_from_slice(&len.to_le_bytes());
                out.push(u8::from(*writable));
            }
            Self::Wanted { len } => {
                out.push(WANTED);
                out.extend_from_slice(&len.to_le_bytes());
            }
            Self::PipeOpened { pipe, capacity } => {
                out.push(PIPE_OPENED);
                out.extend_from_slice(&pipe.to_le_bytes());
                out.extend_from_slice(&capacity.to_le_bytes());
            }
            Self::PipeData { pipe, bytes } => {
                out.push(PIPE_DATA);
                out.extend_from_slice(&pipe.to_le_bytes());
                out.extend_from_slice(bytes);
            }
            Self::PipeRead { pipe, count } => {
                out.push(PIPE_READ);
                out.extend_from_slice(&pipe.to_le_bytes());
                out.extend_from_slice(&count.to_le_bytes());
            }
            Self::PipeEnded { pipe } => {
                out.push(PIPE_ENDED);
                out.extend_from_slice(&pipe.to_le_bytes());
            }
            Self::PipeCancelled { pipe } => {
                out.push(PIPE_CANCELLED);
                out.extend_from_slice(&pipe.to_le_bytes());
            }
        }
    }

    /// How many handles come with the message.
    pub const fn handles(&self) -> usize {
        match self {
            Self::Granted { .. } => 1,
            _ => 0,
        }
    }

    /// A payload read back: anything [`Control::encode`] doesn't write is
    /// a bad message. A pipe's bytes stay where the payload is.
    pub fn decode(payload: &'a [u8]) -> Result<Self, BadMessage> {
        let (&tag, rest) = payload.split_first().ok_or(BadMessage::Rejected)?;
        if tag == RELEASED {
            return match rest.as_chunks() {
                (numbers, []) => Ok(Self::Released(
                    numbers.iter().copied().map(u32::from_le_bytes).collect(),
                )),
                _ => Err(BadMessage::Rejected),
            };
        }
        // Every other message begins with a number: a region's, a pipe's
        // or a length.
        let (first, rest) = rest.split_at_checked(4).ok_or(BadMessage::Rejected)?;
        let first = word(first)?;
        match (tag, rest) {
            (GRANTED, &[a, b, c, d, writable @ (0 | 1)]) => Ok(Self::Granted {
                number: first,
                len: u32::from_le_bytes([a, b, c, d]),
                writable: writable == 1,
            }),
            (PIPE_OPENED, &[a, b, c, d]) => Ok(Self::PipeOpened {
                pipe: first,
                capacity: u32::from_le_bytes([a, b, c, d]),
            }),
            (PIPE_DATA, bytes) => Ok(Self::PipeData { pipe: first, bytes }),
            (PIPE_READ, count) => Ok(Self::PipeRead {
                pipe: first,
                count: count
                    .try_into()
                    .map(u64::from_le_bytes)
                    .map_err(|_| BadMessage::Rejected)?,
            }),
            (WANTED, []) => Ok(Self::Wanted { len: first }),
            (PIPE_ENDED, []) => Ok(Self::PipeEnded { pipe: first }),
            (PIPE_CANCELLED, []) => Ok(Self::PipeCancelled { pipe: first }),
            _ => Err(BadMessage::Rejected),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cores_own_messages_read_back() {
        let messages = [
            Control::Released(vec![]),
            Control::Released(vec![1, u32::MAX, 0x0102_0304]),
            Control::Granted {
                number: 7,
                len: 1 << 16,
                writable: true,
            },
            Control::Granted {
                number: u32::MAX,
                len: 1 << 30,
                writable: false,
            },
            Control::Wanted { len: 0 },
            Control::Wanted { len: u32::MAX },
            Control::PipeOpened {
                pipe: 3,
                capacity: 512 << 10,
            },
            Control::PipeData {
                pipe: 3,
                bytes: b"",
            },
            Control::PipeData {
                pipe: u32::MAX,
                bytes: b"some of a body",
            },
            Control::PipeRead { pipe: 3, count: 0 },
            Control::PipeRead {
                pipe: 4,
                count: u64::MAX,
            },
            Control::PipeEnded { pipe: 5 },
            Control::PipeCancelled { pipe: 6 },
        ];
        for control in &messages {
            let mut payload = vec![];
            control.encode(&mut payload);
            assert_eq!(Control::decode(&payload).as_ref(), Ok(control));
            assert_eq!(
                control.handles(),
                usize::from(matches!(control, Control::Granted { .. }))
            );
        }
        // No two kinds are written alike.
        let tags: Vec<u8> = messages
            .iter()
            .map(|control| {
                let mut payload = vec![];
                control.encode(&mut payload);
                payload[0]
            })
            .collect();
        assert_eq!(tags, [0, 0, 1, 1, 7, 7, 2, 3, 3, 4, 4, 5, 6]);
    }

    #[test]
    fn each_field_is_where_it_is_written() {
        let payload = |control: Control<'_>| {
            let mut payload = vec![0xaa];
            control.encode(&mut payload);
            payload
        };
        assert_eq!(
            payload(Control::Released(vec![0x0102_0304])),
            [0xaa, 0, 4, 3, 2, 1],
            "after what was there"
        );
        assert_eq!(
            payload(Control::Granted {
                number: 0x0102_0304,
                len: 0x0506_0708,
                writable: true
            }),
            [0xaa, 1, 4, 3, 2, 1, 8, 7, 6, 5, 1]
        );
        assert_eq!(
            payload(Control::Wanted { len: 0x0102_0304 }),
            [0xaa, 7, 4, 3, 2, 1]
        );
        assert_eq!(
            payload(Control::PipeOpened {
                pipe: 0x0102_0304,
                capacity: 0x0506_0708
            }),
            [0xaa, 2, 4, 3, 2, 1, 8, 7, 6, 5]
        );
        assert_eq!(
            payload(Control::PipeData {
                pipe: 0x0102_0304,
                bytes: &[9, 8]
            }),
            [0xaa, 3, 4, 3, 2, 1, 9, 8]
        );
        assert_eq!(
            payload(Control::PipeRead {
                pipe: 0x0102_0304,
                count: 0x0506_0708_090a_0b0c
            }),
            [0xaa, 4, 4, 3, 2, 1, 0x0c, 0x0b, 0x0a, 9, 8, 7, 6, 5]
        );
        assert_eq!(
            payload(Control::PipeEnded { pipe: 0x0102_0304 }),
            [0xaa, 5, 4, 3, 2, 1]
        );
        assert_eq!(
            payload(Control::PipeCancelled { pipe: 0x0102_0304 }),
            [0xaa, 6, 4, 3, 2, 1]
        );
    }

    #[test]
    fn a_control_message_nobody_writes_is_a_bad_message() {
        let bad: [&[u8]; 22] = [
            &[],
            &[8],
            &[8, 0, 0, 0, 0],
            &[0xff, 0, 0, 0, 0],
            // Wanted: short, long.
            &[7, 0, 0, 0],
            &[7, 0, 0, 0, 0, 0],
            // Released: not whole numbers.
            &[0, 1],
            &[0, 1, 2, 3, 4, 5],
            // Granted: short, long, a flag that is neither.
            &[1],
            &[1, 0, 0, 0, 0, 0, 0, 0, 0],
            &[1, 0, 0, 0, 0, 0, 0, 0, 0, 2],
            &[1, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0],
            // PipeOpened: short, long.
            &[2, 0, 0, 0, 0, 0, 0, 0],
            &[2, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            // PipeData: no pipe.
            &[3, 0, 0, 0],
            // PipeRead: short, long.
            &[4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            &[4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            // PipeEnded, PipeCancelled: short, long.
            &[5, 0, 0, 0],
            &[5, 0, 0, 0, 0, 0],
            &[6, 0, 0, 0],
            &[6, 0, 0, 0, 0, 0],
            &[6],
        ];
        for payload in bad {
            assert_eq!(
                Control::decode(payload),
                Err(BadMessage::Rejected),
                "{payload:?}"
            );
        }
    }
}
