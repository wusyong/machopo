//! One Mach message, put together and taken apart by hand: the kernel's
//! structures are packed to 4 bytes (`#pragma pack(4)`), so each field is
//! written and read at its place in a buffer of words, not through a
//! `#[repr(C)]` struct that would lay a descriptor out otherwise.
//!
//! A message here is the header; when it carries rights, the count of its
//! descriptors and one port descriptor a right; then its body: a length
//! and that many bytes, padded to a word. Out-of-line memory is never
//! sent, and a message that brings any, or a descriptor of another kind,
//! is destroyed whole and refused.
//!
//! Every message received comes with the kernel's word on who sent it
//! (the audit trailer): a pid the sender can't choose, 0 for the kernel
//! itself.

use std::ops::Range;
use std::time::Duration;

use mach2::message::{
    MACH_MSG_PORT_DESCRIPTOR, MACH_MSG_SUCCESS, MACH_MSG_TIMEOUT_NONE, MACH_MSG_TYPE_COPY_SEND,
    MACH_MSG_TYPE_MAKE_SEND_ONCE, MACH_MSG_TYPE_MOVE_RECEIVE, MACH_MSG_TYPE_MOVE_SEND,
    MACH_MSG_TYPE_MOVE_SEND_ONCE, MACH_MSGH_BITS_COMPLEX, MACH_RCV_MSG, MACH_RCV_TIMED_OUT,
    MACH_RCV_TIMEOUT, MACH_RCV_TOO_LARGE, MACH_SEND_MSG, MACH_SEND_TIMED_OUT, MACH_SEND_TIMEOUT,
    audit_token_t, mach_msg, mach_msg_destroy, mach_msg_header_t, mach_msg_return_t,
    mach_msg_type_name_t,
};
use mach2::port::{MACH_PORT_NULL, mach_port_t};

use super::port::{ReceiveRight, Right, SendOnceRight, SendRight};

/// The header: bits, size, remote port, local port, voucher, id.
const HEADER_WORDS: usize = 6;
/// A port descriptor: name, pad, and a word of pad, disposition and type.
const PORT_DESCRIPTOR_WORDS: usize = 3;
/// What follows a message in the buffer it was received into: the
/// trailer with the audit token (type, size, sequence number, the old
/// security token's two words, the audit token's eight).
const TRAILER_WORDS: usize = 13;
/// Where in the trailer the audit token is.
const AUDIT_TOKEN_AT: usize = 5;
/// `MACH_RCV_TRAILER_ELEMENTS(MACH_RCV_TRAILER_AUDIT)`: ask for the
/// trailer up to the audit token.
const RCV_AUDIT_TRAILER: i32 = 3 << 24;
/// The most rights one message carries here.
pub const MAX_RIGHTS: usize = 16;

#[link(name = "bsm")]
unsafe extern "C" {
    fn audit_token_to_pid(token: audit_token_t) -> i32;
}

/// A buffer for one message, aligned as its header is.
pub struct Buffer {
    words: Vec<u32>,
    /// The longest body it is for.
    body: usize,
}

impl Buffer {
    /// A buffer that holds a message whose body is `body` bytes, with all
    /// the rights one may carry, and its trailer.
    pub fn for_body(body: usize) -> Self {
        let fixed = HEADER_WORDS
            .saturating_add(1)
            .saturating_add(MAX_RIGHTS.saturating_mul(PORT_DESCRIPTOR_WORDS))
            .saturating_add(1)
            .saturating_add(TRAILER_WORDS);
        Self {
            words: vec![0; fixed.saturating_add(body.div_ceil(4))],
            body,
        }
    }

    fn bytes(&self) -> &[u8] {
        // SAFETY: the words are initialized, a `u32`'s bytes are bytes,
        // and the length in bytes is what the vector holds.
        unsafe {
            std::slice::from_raw_parts(
                self.words.as_ptr().cast(),
                std::mem::size_of_val(&*self.words),
            )
        }
    }

    fn bytes_mut(&mut self) -> &mut [u8] {
        let len = std::mem::size_of_val(&*self.words);
        // SAFETY: as `bytes`, and any bytes are a `u32`.
        unsafe { std::slice::from_raw_parts_mut(self.words.as_mut_ptr().cast(), len) }
    }

    /// The bytes of a message received into this buffer.
    pub fn body(&self, received: &Received) -> &[u8] {
        self.bytes().get(received.body.clone()).unwrap_or(&[])
    }
}

/// Who a message is sent to.
pub enum To<'a> {
    /// A port this task keeps its right to.
    Port(&'a SendRight),
    /// The one reply a send-once right is for.
    Reply(SendOnceRight),
}

/// Why a message wasn't sent. Its rights were let go with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendError {
    /// More bytes or rights than a message holds here.
    TooLarge,
    /// The time given ran out with the port's queue still full.
    TimedOut,
    /// The kernel refused: the port is dead, most often.
    Kernel(mach_msg_return_t),
}

/// Sends one message: `body` and `rights` to `to`, saying it is `id`.
/// With `reply`, the message carries a send-once right to that port for
/// the answer. `timeout` is how long to wait for room in the port's
/// queue; `None` waits as long as it takes.
pub fn send(
    buffer: &mut Buffer,
    to: To<'_>,
    reply: Option<&ReceiveRight>,
    id: i32,
    body: &[u8],
    rights: Vec<Right>,
    timeout: Option<Duration>,
) -> Result<(), SendError> {
    let body_len = u32::try_from(body.len()).map_err(|_| SendError::TooLarge)?;
    if rights.len() > MAX_RIGHTS || body.len() > buffer.body {
        // Not sent: the rights go with the vector, each let go.
        return Err(SendError::TooLarge);
    }
    let complex = !rights.is_empty();
    let (remote, remote_disposition) = match to {
        To::Port(right) => (right.name(), MACH_MSG_TYPE_COPY_SEND),
        To::Reply(right) => (right.into_raw(), MACH_MSG_TYPE_MOVE_SEND_ONCE),
    };
    let (local, local_disposition) = match reply {
        Some(port) => (port.name(), MACH_MSG_TYPE_MAKE_SEND_ONCE),
        None => (MACH_PORT_NULL, 0),
    };
    let mut words = Vec::with_capacity(
        HEADER_WORDS
            .saturating_add(2)
            .saturating_add(rights.len().saturating_mul(3)),
    );
    let bits = remote_disposition
        | (local_disposition << 8)
        | if complex { MACH_MSGH_BITS_COMPLEX } else { 0 };
    // The size is filled in once it is known.
    words.extend([bits, 0, remote, local, MACH_PORT_NULL, id.cast_unsigned()]);
    if complex {
        words.push(u32::try_from(rights.len()).map_err(|_| SendError::TooLarge)?);
        for right in rights {
            // From here the names are the message's: sent, they are the
            // kernel's; not sent, `mach_msg_destroy` lets them go.
            let (name, disposition) = match right {
                Right::Send(right) => (right.into_raw(), MACH_MSG_TYPE_MOVE_SEND),
                Right::Receive(right) => (right.into_raw(), MACH_MSG_TYPE_MOVE_RECEIVE),
            };
            // The last word: 16 bits of pad, the disposition, the type.
            words.extend([
                name,
                0,
                (disposition << 16) | (MACH_MSG_PORT_DESCRIPTOR << 24),
            ]);
        }
    }
    words.push(body_len);
    let before_body = words.len();
    let total = before_body.saturating_add(body.len().div_ceil(4));
    let size = total
        .checked_mul(4)
        .and_then(|size| u32::try_from(size).ok());
    let fits = total.saturating_add(TRAILER_WORDS) <= buffer.words.len();
    let (Some(size), true) = (size, fits) else {
        // Not sent: the rights the message was given are let go.
        discard(&mut words);
        return Err(SendError::TooLarge);
    };
    if let Some(size_word) = words.get_mut(1) {
        *size_word = size;
    }
    let Some((head, _)) = buffer.words.split_at_mut_checked(before_body) else {
        discard(&mut words);
        return Err(SendError::TooLarge);
    };
    head.copy_from_slice(&words);
    // The body's last word is padded with zeroes.
    if let Some(last) = buffer.words.get_mut(total.saturating_sub(1)) {
        *last = 0;
    }
    let start = before_body.saturating_mul(4);
    if let Some(into) = buffer
        .bytes_mut()
        .get_mut(start..start.saturating_add(body.len()))
    {
        into.copy_from_slice(body);
    }
    let (option, wait) = match timeout {
        Some(timeout) => (
            MACH_SEND_MSG | MACH_SEND_TIMEOUT,
            u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX),
        ),
        None => (MACH_SEND_MSG, MACH_MSG_TIMEOUT_NONE),
    };
    let header = buffer.words.as_mut_ptr().cast::<mach_msg_header_t>();
    // SAFETY: the buffer holds a whole message of `size` bytes, aligned
    // as a header is; the rights it names were this task's to give.
    let result = unsafe {
        mach_msg(
            header,
            option,
            size,
            0,
            MACH_PORT_NULL,
            wait,
            MACH_PORT_NULL,
        )
    };
    match result {
        MACH_MSG_SUCCESS => Ok(()),
        failed => {
            // SAFETY: the message wasn't sent: what it names (the rights
            // it was to move, the send-once right it was to use) is
            // still this task's, and this lets each go once. A right it
            // only copied (`To::Port`) is left as it was.
            unsafe { mach_msg_destroy(header) };
            Err(if failed == MACH_SEND_TIMED_OUT {
                SendError::TimedOut
            } else {
                SendError::Kernel(failed)
            })
        }
    }
}

/// Lets go of the rights a message that is never handed to the kernel
/// was given.
fn discard(words: &mut [u32]) {
    // SAFETY: `words` is a whole message as `send` lays it out, never
    // sent: its rights are this task's, each let go once here.
    unsafe { mach_msg_destroy(words.as_mut_ptr().cast()) };
}

/// Who sent a message, by the kernel's audit token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sender {
    token: [u32; 8],
}

impl Sender {
    /// The sender's process; 0 for the kernel.
    pub fn pid(&self) -> i32 {
        // SAFETY: a call with a value, which reads one of its words.
        unsafe { audit_token_to_pid(audit_token_t { val: self.token }) }
    }
}

/// A message received. Its rights are this task's now.
#[derive(Debug)]
pub struct Received {
    /// What the sender said the message is.
    pub id: i32,
    /// Where the sender wants the answer, if it asked for one.
    pub reply: Option<SendOnceRight>,
    pub rights: Vec<Right>,
    pub sender: Sender,
    /// Where in the buffer its body is ([`Buffer::body`]).
    body: Range<usize>,
}

/// Why no message was received.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiveError {
    /// The time given ran out.
    TimedOut,
    /// A message longer than the buffer: taken off the port and destroyed.
    TooLarge,
    /// A message that isn't laid out as [`send`] lays one out: destroyed,
    /// with whatever it brought.
    Malformed,
    Kernel(mach_msg_return_t),
}

/// Receives one message on `port` (a receive right's name, or a port
/// set's), waiting `timeout` for one (`None`: as long as it takes).
pub fn receive(
    buffer: &mut Buffer,
    port: mach_port_t,
    timeout: Option<Duration>,
) -> Result<Received, ReceiveError> {
    let (option, wait) = match timeout {
        Some(timeout) => (
            MACH_RCV_MSG | MACH_RCV_TIMEOUT,
            u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX),
        ),
        None => (MACH_RCV_MSG, MACH_MSG_TIMEOUT_NONE),
    };
    let capacity = u32::try_from(std::mem::size_of_val(&*buffer.words)).unwrap_or(u32::MAX);
    let header = buffer.words.as_mut_ptr().cast::<mach_msg_header_t>();
    // SAFETY: the buffer is `capacity` bytes, aligned as a header is; the
    // kernel writes no more than that.
    let result = unsafe {
        mach_msg(
            header,
            option | RCV_AUDIT_TRAILER,
            0,
            capacity,
            port,
            wait,
            MACH_PORT_NULL,
        )
    };
    match result {
        MACH_MSG_SUCCESS => {}
        MACH_RCV_TIMED_OUT => return Err(ReceiveError::TimedOut),
        // Without `MACH_RCV_LARGE` the kernel has destroyed it.
        MACH_RCV_TOO_LARGE => return Err(ReceiveError::TooLarge),
        failed => return Err(ReceiveError::Kernel(failed)),
    }
    match parse(&buffer.words) {
        Some(parsed) => Ok(parsed.own()),
        None => {
            // SAFETY: a message the kernel just wrote whole into the
            // buffer; nothing of it was taken yet, so each right and any
            // memory it brought is let go once here.
            unsafe { mach_msg_destroy(header) };
            Err(ReceiveError::Malformed)
        }
    }
}

/// A message's parts, its rights still only names: none is owned until
/// the whole message is known to be one of ours.
struct Parsed {
    id: i32,
    reply: mach_port_t,
    rights: Vec<(mach_port_t, mach_msg_type_name_t)>,
    token: [u32; 8],
    body: Range<usize>,
}

impl Parsed {
    fn own(self) -> Received {
        let rights = self
            .rights
            .into_iter()
            .map(|(name, disposition)| {
                if disposition == MACH_MSG_TYPE_MOVE_RECEIVE {
                    // SAFETY: a receive right the message moved to this
                    // task, named nowhere else.
                    Right::Receive(unsafe { ReceiveRight::from_raw(name) })
                } else {
                    // SAFETY: a send right the message gave this task
                    // (`parse` let through no other kind), which this
                    // value now lets go of.
                    Right::Send(unsafe { SendRight::from_raw(name) })
                }
            })
            .collect();
        // SAFETY: the reply port of a message received is a send-once
        // right the sender made for the answer; `parse` let through no
        // other disposition.
        let reply =
            (self.reply != MACH_PORT_NULL).then(|| unsafe { SendOnceRight::from_raw(self.reply) });
        Received {
            id: self.id,
            reply,
            rights,
            sender: Sender { token: self.token },
            body: self.body,
        }
    }
}

fn parse(words: &[u32]) -> Option<Parsed> {
    let (&[bits, size, reply, _local, _voucher, id], mut rest) =
        words.split_first_chunk::<HEADER_WORDS>()?;
    let size_words = usize::try_from(size).ok()?.div_ceil(4);
    // After a receive the header's remote port is the sender's reply
    // right, and the low byte of the bits how it came.
    let reply_disposition = bits & 0xff;
    if reply != MACH_PORT_NULL && reply_disposition != MACH_MSG_TYPE_MOVE_SEND_ONCE {
        return None;
    }
    let mut at = HEADER_WORDS;
    let mut rights = Vec::new();
    if bits & MACH_MSGH_BITS_COMPLEX != 0 {
        let (&count, after) = rest.split_first()?;
        rest = after;
        at = at.checked_add(1)?;
        if usize::try_from(count).ok()? > MAX_RIGHTS {
            return None;
        }
        for _ in 0..count {
            let (&[name, _pad, kind], after) = rest.split_first_chunk::<PORT_DESCRIPTOR_WORDS>()?;
            rest = after;
            at = at.checked_add(PORT_DESCRIPTOR_WORDS)?;
            let disposition = (kind >> 16) & 0xff;
            let sent_as_right =
                disposition == MACH_MSG_TYPE_MOVE_SEND || disposition == MACH_MSG_TYPE_MOVE_RECEIVE;
            if kind >> 24 != MACH_MSG_PORT_DESCRIPTOR || !sent_as_right {
                return None;
            }
            rights.push((name, disposition));
        }
    }
    // The kernel's own messages (notifications) have no body of this
    // layout: what is left of them is no body at all.
    let body = match rest.split_first() {
        Some((&len, _)) if at < size_words => {
            let start = at.checked_add(1)?.checked_mul(4)?;
            let end = start.checked_add(usize::try_from(len).ok()?)?;
            if end > size_words.checked_mul(4)? {
                return None;
            }
            start..end
        }
        _ => 0..0,
    };
    // The trailer follows the message.
    let trailer = words.get(size_words..)?;
    let token = *trailer.get(AUDIT_TOKEN_AT..)?.first_chunk::<8>()?;
    Some(Parsed {
        id: id.cast_signed(),
        reply,
        rights,
        token,
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: i32 = 0x1234;
    const SOON: Option<Duration> = Some(Duration::from_millis(500));

    fn own_pid() -> i32 {
        std::process::id().cast_signed()
    }

    /// A port and a send right to it.
    fn port() -> (ReceiveRight, SendRight) {
        let receive = ReceiveRight::new().unwrap();
        let send = receive.make_send().unwrap();
        (receive, send)
    }

    #[test]
    fn a_messages_bytes_arrive_as_sent_from_the_process_that_sent_them() {
        let (receive, to) = port();
        let mut buffer = Buffer::for_body(4096);
        for len in [0usize, 1, 3, 4, 5, 127, 4096] {
            let body: Vec<u8> = (0..len)
                .map(|i| (i as u8).wrapping_mul(7).wrapping_add(1))
                .collect();
            send(&mut buffer, To::Port(&to), None, ID, &body, vec![], None).unwrap();
            let received = receive_one(&mut buffer, &receive);
            assert_eq!(received.id, ID);
            assert_eq!(buffer.body(&received), &body[..], "{len} bytes");
            assert!(received.reply.is_none());
            assert!(received.rights.is_empty());
            assert_eq!(received.sender.pid(), own_pid());
        }
    }

    fn receive_one(buffer: &mut Buffer, on: &ReceiveRight) -> Received {
        receive(buffer, on.name(), SOON).unwrap()
    }

    #[test]
    fn nothing_to_receive_times_out() {
        let (receive, _to) = port();
        let mut buffer = Buffer::for_body(64);
        let none = super::receive(&mut buffer, receive.name(), Some(Duration::from_millis(20)));
        assert_eq!(none.unwrap_err(), ReceiveError::TimedOut);
    }

    #[test]
    fn rights_cross_in_order_and_are_the_receivers() {
        let (receive, to) = port();
        let (first_port, first_send) = port();
        let (second_port, second_send) = port();
        let third_port = ReceiveRight::new().unwrap();
        let third_send = third_port.make_send().unwrap();
        let mut buffer = Buffer::for_body(64);
        let rights = vec![
            Right::Send(first_send),
            Right::Receive(third_port),
            Right::Send(second_send),
        ];
        send(
            &mut buffer,
            To::Port(&to),
            None,
            ID,
            b"with rights",
            rights,
            None,
        )
        .unwrap();
        let mut received = receive_one(&mut buffer, &receive);
        assert_eq!(buffer.body(&received), b"with rights");
        assert_eq!(received.rights.len(), 3);
        // Each is the right it was: a message sent to one comes out of
        // the port it was a right to.
        let Right::Send(to_second) = received.rights.pop().unwrap() else {
            panic!("the third was a send right");
        };
        let Right::Receive(third) = received.rights.pop().unwrap() else {
            panic!("the second was a receive right");
        };
        let Right::Send(to_first) = received.rights.pop().unwrap() else {
            panic!("the first was a send right");
        };
        for (to, from, body) in [
            (&to_first, &first_port, &b"one"[..]),
            (&to_second, &second_port, b"two"),
            (&third_send, &third, b"three"),
        ] {
            send(&mut buffer, To::Port(to), None, ID, body, vec![], None).unwrap();
            let received = receive_one(&mut buffer, from);
            assert_eq!(buffer.body(&received), body);
        }
    }

    #[test]
    fn an_answer_goes_to_the_port_the_asker_named_once() {
        let (server, to_server) = port();
        let answers = ReceiveRight::new().unwrap();
        let mut buffer = Buffer::for_body(64);
        send(
            &mut buffer,
            To::Port(&to_server),
            Some(&answers),
            ID,
            b"ask",
            vec![],
            None,
        )
        .unwrap();
        let asked = receive_one(&mut buffer, &server);
        let reply = asked.reply.expect("a right to answer with");
        send(
            &mut buffer,
            To::Reply(reply),
            None,
            ID + 1,
            b"answer",
            vec![],
            None,
        )
        .unwrap();
        let answer = receive_one(&mut buffer, &answers);
        assert_eq!((answer.id, buffer.body(&answer)), (ID + 1, &b"answer"[..]));
        assert!(answer.reply.is_none());
    }

    #[test]
    fn an_answer_never_sent_is_told_by_the_kernel() {
        let (server, to_server) = port();
        let answers = ReceiveRight::new().unwrap();
        let mut buffer = Buffer::for_body(64);
        send(
            &mut buffer,
            To::Port(&to_server),
            Some(&answers),
            ID,
            b"ask",
            vec![],
            None,
        )
        .unwrap();
        let asked = receive_one(&mut buffer, &server);
        drop(asked);
        // The right to answer went unused: the kernel says so, as itself.
        let told = receive_one(&mut buffer, &answers);
        assert_eq!(told.id, mach2::notify::MACH_NOTIFY_SEND_ONCE);
        assert_eq!(told.sender.pid(), 0);
        assert_eq!(buffer.body(&told), b"");
    }

    #[test]
    fn a_message_to_a_dead_port_isnt_sent_and_its_rights_are_let_go() {
        let (receive, to) = port();
        drop(receive);
        let (carried_port, carried) = port();
        let mut buffer = Buffer::for_body(64);
        let sent = send(
            &mut buffer,
            To::Port(&to),
            None,
            ID,
            b"",
            vec![Right::Send(carried)],
            None,
        );
        assert!(matches!(sent, Err(SendError::Kernel(_))), "{sent:?}");
        // The right the message carried was let go: with no send right
        // left, the kernel tells the port so when asked.
        assert!(no_senders(&carried_port));
    }

    /// Whether no send right to `port` is left: asks the kernel to say so
    /// on the port itself, which it does at once when there is none.
    fn no_senders(port: &ReceiveRight) -> bool {
        assert!(port.tell_when_no_senders(0));
        let mut buffer = Buffer::for_body(64);
        match receive(&mut buffer, port.name(), Some(Duration::from_millis(50))) {
            Ok(told) => told.id == mach2::notify::MACH_NOTIFY_NO_SENDERS && told.sender.pid() == 0,
            Err(_) => false,
        }
    }

    #[test]
    fn rights_received_and_dropped_are_let_go() {
        let (receive, to) = port();
        let (carried_port, carried) = port();
        let mut buffer = Buffer::for_body(64);
        send(
            &mut buffer,
            To::Port(&to),
            None,
            ID,
            b"",
            vec![Right::Send(carried)],
            None,
        )
        .unwrap();
        assert!(!no_senders(&carried_port), "the message holds it");
        let received = receive_one(&mut buffer, &receive);
        drop(received);
        assert!(no_senders(&carried_port));
        // A receive right dropped is a dead port: a send to it fails.
        let moved = ReceiveRight::new().unwrap();
        let to_moved = moved.make_send().unwrap();
        send(
            &mut buffer,
            To::Port(&to),
            None,
            ID,
            b"",
            vec![Right::Receive(moved)],
            None,
        )
        .unwrap();
        drop(receive_one(&mut buffer, &receive));
        let sent = send(
            &mut buffer,
            To::Port(&to_moved),
            None,
            ID,
            b"",
            vec![],
            None,
        );
        assert!(matches!(sent, Err(SendError::Kernel(_))), "{sent:?}");
    }

    #[test]
    fn more_than_a_message_holds_isnt_sent() {
        let (receive, to) = port();
        let mut buffer = Buffer::for_body(16);
        assert_eq!(
            send(&mut buffer, To::Port(&to), None, ID, &[0; 17], vec![], None),
            Err(SendError::TooLarge)
        );
        assert_eq!(
            send(&mut buffer, To::Port(&to), None, ID, &[7; 16], vec![], None),
            Ok(())
        );
        let received = receive_one(&mut buffer, &receive);
        assert_eq!(buffer.body(&received), &[7; 16]);
        // Too many rights: none of them is sent, each is let go.
        let (carried_port, _) = port();
        let rights = (0..=MAX_RIGHTS)
            .map(|_| Right::Send(carried_port.make_send().unwrap()))
            .collect();
        assert_eq!(
            send(&mut buffer, To::Port(&to), None, ID, b"", rights, None),
            Err(SendError::TooLarge)
        );
        assert!(no_senders(&carried_port));
        // As many as one holds are.
        let rights = (0..MAX_RIGHTS)
            .map(|_| Right::Send(carried_port.make_send().unwrap()))
            .collect();
        assert_eq!(
            send(&mut buffer, To::Port(&to), None, ID, &[1; 16], rights, None),
            Ok(())
        );
        let received = receive_one(&mut buffer, &receive);
        assert_eq!(received.rights.len(), MAX_RIGHTS);
        assert_eq!(buffer.body(&received), &[1; 16]);
    }

    #[test]
    fn a_message_longer_than_the_buffer_is_refused_and_gone() {
        let (receive, to) = port();
        let mut large = Buffer::for_body(4096);
        send(
            &mut large,
            To::Port(&to),
            None,
            ID,
            &[1; 4096],
            vec![],
            None,
        )
        .unwrap();
        send(&mut large, To::Port(&to), None, ID, b"next", vec![], None).unwrap();
        let mut small = Buffer::for_body(64);
        assert_eq!(
            super::receive(&mut small, receive.name(), SOON).unwrap_err(),
            ReceiveError::TooLarge
        );
        // It was taken off the port: the next one is next.
        let received = receive_one(&mut small, &receive);
        assert_eq!(small.body(&received), b"next");
    }

    #[test]
    fn a_full_port_refuses_a_send_that_wont_wait() {
        let (receive, to) = port();
        let mut buffer = Buffer::for_body(64);
        let mut sent = 0;
        let full = loop {
            match send(
                &mut buffer,
                To::Port(&to),
                None,
                ID,
                b"x",
                vec![],
                Some(Duration::ZERO),
            ) {
                Ok(()) => sent += 1,
                Err(error) => break error,
            }
            assert!(sent < 10_000, "a port's queue has an end");
        };
        assert_eq!(full, SendError::TimedOut);
        assert!(sent >= 1);
        // Read one: room for one more.
        receive_one(&mut buffer, &receive);
        assert_eq!(
            send(
                &mut buffer,
                To::Port(&to),
                None,
                ID,
                b"x",
                vec![],
                Some(Duration::ZERO)
            ),
            Ok(())
        );
    }

    /// A buffer that holds any message these tests make.
    fn large_enough() -> Buffer {
        Buffer::for_body(4096)
    }

    /// A message as bytes of this test's own making, sent as they are.
    fn send_raw(to: &SendRight, mut words: Vec<u32>) {
        words[1] = (words.len() * 4) as u32;
        words[2] = to.name();
        // SAFETY: a whole message of the size its header says; what it
        // names is the test's to give.
        let result = unsafe {
            mach_msg(
                words.as_mut_ptr().cast(),
                MACH_SEND_MSG,
                words[1],
                0,
                MACH_PORT_NULL,
                MACH_MSG_TIMEOUT_NONE,
                MACH_PORT_NULL,
            )
        };
        assert_eq!(result, MACH_MSG_SUCCESS);
    }

    #[test]
    fn a_message_laid_out_otherwise_is_refused_with_what_it_brought() {
        let (receive, to) = port();
        let mut buffer = Buffer::for_body(256);
        let plain = MACH_MSG_TYPE_COPY_SEND;
        // A body said to be longer than the message.
        send_raw(&to, vec![plain, 0, 0, 0, 0, 1, 9, 0xaaaa_aaaa]);
        assert_eq!(
            super::receive(&mut buffer, receive.name(), SOON).unwrap_err(),
            ReceiveError::Malformed
        );
        // Exactly as long as it says is one.
        send_raw(&to, vec![plain, 0, 0, 0, 0, 1, 4, 0xaaaa_aaaa]);
        let received = receive_one(&mut buffer, &receive);
        assert_eq!(buffer.body(&received), &[0xaa; 4]);
        // Out-of-line memory: refused, and unmapped.
        let memory = vec![0x5a_u8; 1 << 16];
        let address = memory.as_ptr() as u64;
        send_raw(
            &to,
            vec![
                plain | MACH_MSGH_BITS_COMPLEX,
                0,
                0,
                0,
                0,
                1,
                1,
                address as u32,
                (address >> 32) as u32,
                // deallocate, copy (virtual), pad, type (out-of-line).
                (1 << 8) | (1 << 24),
                1 << 16,
                0,
            ],
        );
        assert_eq!(
            super::receive(&mut buffer, receive.name(), SOON).unwrap_err(),
            ReceiveError::Malformed
        );
        // A right that is neither a send nor a receive right (a
        // send-once right, made from the port itself): refused, let go.
        let (carried_port, _) = port();
        send_raw(
            &to,
            vec![
                plain | MACH_MSGH_BITS_COMPLEX,
                0,
                0,
                0,
                0,
                1,
                1,
                carried_port.name(),
                0,
                (MACH_MSG_TYPE_MAKE_SEND_ONCE << 16) | (MACH_MSG_PORT_DESCRIPTOR << 24),
                0,
            ],
        );
        assert_eq!(
            super::receive(&mut buffer, receive.name(), SOON).unwrap_err(),
            ReceiveError::Malformed
        );
        // Let go, it was never used: the kernel tells the port so.
        let told = receive_one(&mut buffer, &carried_port);
        assert_eq!(told.id, mach2::notify::MACH_NOTIFY_SEND_ONCE);
        // A send right, refused with the rest of a message whose body is
        // wrong: let go too.
        let carried = carried_port.make_send().unwrap();
        send_raw(
            &to,
            vec![
                plain | MACH_MSGH_BITS_COMPLEX,
                0,
                0,
                0,
                0,
                1,
                1,
                carried.into_raw(),
                0,
                (MACH_MSG_TYPE_MOVE_SEND << 16) | (MACH_MSG_PORT_DESCRIPTOR << 24),
                99,
            ],
        );
        assert_eq!(
            super::receive(&mut buffer, receive.name(), SOON).unwrap_err(),
            ReceiveError::Malformed
        );
        assert!(no_senders(&carried_port));
        // Rights out of line (an array of them, by address): refused, and
        // the right in the array let go.
        let names = [carried_port.make_send().unwrap().into_raw()];
        let address = names.as_ptr() as u64;
        send_raw(
            &to,
            vec![
                plain | MACH_MSGH_BITS_COMPLEX,
                0,
                0,
                0,
                0,
                1,
                1,
                address as u32,
                (address >> 32) as u32,
                // deallocate, copy (virtual), disposition, type
                // (out-of-line ports).
                (1 << 8) | (MACH_MSG_TYPE_MOVE_SEND << 16) | (2 << 24),
                1,
                0,
            ],
        );
        assert_eq!(
            super::receive(&mut buffer, receive.name(), SOON).unwrap_err(),
            ReceiveError::Malformed
        );
        assert!(no_senders(&carried_port));
        // More rights than a message of ours carries: all let go.
        let mut many = vec![
            plain | MACH_MSGH_BITS_COMPLEX,
            0,
            0,
            0,
            0,
            1,
            (MAX_RIGHTS + 1) as u32,
        ];
        for _ in 0..=MAX_RIGHTS {
            let right = carried_port.make_send().unwrap().into_raw();
            many.extend([
                right,
                0,
                (MACH_MSG_TYPE_MOVE_SEND << 16) | (MACH_MSG_PORT_DESCRIPTOR << 24),
            ]);
        }
        many.push(0);
        send_raw(&to, many);
        assert_eq!(
            super::receive(&mut large_enough(), receive.name(), SOON).unwrap_err(),
            ReceiveError::Malformed
        );
        assert!(no_senders(&carried_port));
        // A reply port that isn't a right to answer once (a send right
        // the sender made to its own port): refused, let go.
        let mut with_reply = vec![
            plain | (mach2::message::MACH_MSG_TYPE_MAKE_SEND << 8),
            0,
            0,
            0,
            0,
            1,
            0,
        ];
        with_reply[3] = carried_port.name();
        send_raw(&to, with_reply);
        assert_eq!(
            super::receive(&mut buffer, receive.name(), SOON).unwrap_err(),
            ReceiveError::Malformed
        );
        assert!(no_senders(&carried_port));
        // And the port still takes good messages.
        send(&mut buffer, To::Port(&to), None, ID, b"good", vec![], None).unwrap();
        let received = receive_one(&mut buffer, &receive);
        assert_eq!(buffer.body(&received), b"good");
    }
}
