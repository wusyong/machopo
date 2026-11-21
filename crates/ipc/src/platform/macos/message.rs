//! One Mach message, put together and taken apart by hand: the kernel's
//! structures are packed to 4 bytes (`#pragma pack(4)`), so each field is
//! written and read at its place in a buffer of words, not through a
//! `#[repr(C)]` struct that would lay a descriptor out otherwise.
//!
//! A message here is the header; when it carries rights or its body is
//! out of line, the count of its descriptors, one port descriptor a
//! right and at most one of memory; then its body: a length and that
//! many bytes, padded to a word. A body longer than the buffer is for
//! goes out of line where the buffer allows it ([`Buffer::with_out_of_line`],
//! as Chromium's `ChannelMac` does): the kernel maps the sender's pages
//! into the receiver, copy on write, where an inline body is copied in and
//! out. A message that brings a descriptor of another kind, memory where
//! none is allowed or more than is allowed, is destroyed whole and
//! refused.
//!
//! Every message received comes with the kernel's word on who sent it
//! (the audit trailer): a pid the sender can't choose, 0 for the kernel
//! itself.

use std::ops::Range;
use std::time::Duration;

use mach2::message::{
    MACH_MSG_OOL_DESCRIPTOR, MACH_MSG_PORT_DESCRIPTOR, MACH_MSG_SUCCESS, MACH_MSG_TIMEOUT_NONE,
    MACH_MSG_TYPE_COPY_SEND, MACH_MSG_TYPE_MAKE_SEND_ONCE, MACH_MSG_TYPE_MOVE_RECEIVE,
    MACH_MSG_TYPE_MOVE_SEND, MACH_MSG_TYPE_MOVE_SEND_ONCE, MACH_MSG_VIRTUAL_COPY,
    MACH_MSGH_BITS_COMPLEX, MACH_RCV_MSG, MACH_RCV_TIMED_OUT, MACH_RCV_TIMEOUT, MACH_RCV_TOO_LARGE,
    MACH_SEND_MSG, MACH_SEND_TIMED_OUT, MACH_SEND_TIMEOUT, audit_token_t, mach_msg,
    mach_msg_destroy, mach_msg_header_t, mach_msg_return_t, mach_msg_type_name_t,
};
use mach2::port::{MACH_PORT_NULL, mach_port_t};
use mach2::traps::mach_task_self;
use mach2::vm::mach_vm_deallocate;

use super::port::{ReceiveRight, Right, SendOnceRight, SendRight};

/// The header: bits, size, remote port, local port, voucher, id.
const HEADER_WORDS: usize = 6;
/// A port descriptor: name, pad, and a word of pad, disposition and type.
const PORT_DESCRIPTOR_WORDS: usize = 3;
/// A descriptor of out-of-line memory: the address in two words, a word
/// of flags and type, the size.
const OOL_DESCRIPTOR_WORDS: usize = 4;
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
    /// The longest body that goes in the message itself.
    body: usize,
    /// The longest body that goes out of line; 0 when none does.
    out_of_line: usize,
    /// The port the last message refused as malformed came to.
    refused_at: mach_port_t,
}

impl Buffer {
    /// A buffer that holds a message whose body is `body` bytes, with all
    /// the rights one may carry, and its trailer.
    pub fn for_body(body: usize) -> Self {
        let fixed = HEADER_WORDS
            .saturating_add(1)
            .saturating_add(MAX_RIGHTS.saturating_mul(PORT_DESCRIPTOR_WORDS))
            .saturating_add(OOL_DESCRIPTOR_WORDS)
            .saturating_add(1)
            .saturating_add(TRAILER_WORDS);
        Self {
            words: vec![0; fixed.saturating_add(body.div_ceil(4))],
            body,
            out_of_line: 0,
            refused_at: MACH_PORT_NULL,
        }
    }

    /// A buffer for messages whose bodies of up to `inline` bytes go in
    /// the message itself and longer ones, up to `most` bytes, out of
    /// line. A message received with more out of line than `most` is
    /// refused.
    pub fn with_out_of_line(inline: usize, most: usize) -> Self {
        Self {
            out_of_line: most,
            ..Self::for_body(inline)
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

    /// The port the message a receive just refused
    /// ([`ReceiveError::Malformed`]) came to: which of a set's.
    pub const fn refused_at(&self) -> mach_port_t {
        self.refused_at
    }

    /// The bytes of a message received into this buffer.
    pub fn body<'a>(&'a self, received: &'a Received) -> &'a [u8] {
        match &received.body {
            Body::Inline(range) => self.bytes().get(range.clone()).unwrap_or(&[]),
            // SAFETY: the kernel mapped `len` bytes at `address` into this
            // task for this message alone, a copy of the sender's pages
            // that the sender can't reach; they stay mapped until
            // `received` is dropped, which the borrow outlasts.
            Body::OutOfLine(mapped) => unsafe {
                std::slice::from_raw_parts(
                    std::ptr::with_exposed_provenance(usize::try_from(mapped.address).unwrap_or(0)),
                    mapped.len,
                )
            },
        }
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
    /// The port it came to: one of a set's, when it was received on a
    /// set.
    pub port: mach_port_t,
    /// Where its body is ([`Buffer::body`]).
    body: Body,
}

#[derive(Debug)]
enum Body {
    /// In the buffer.
    Inline(Range<usize>),
    OutOfLine(Mapped),
}

/// Memory a message brought out of line: this task's own, unmapped when
/// dropped.
#[derive(Debug)]
struct Mapped {
    address: u64,
    len: usize,
}

impl Drop for Mapped {
    fn drop(&mut self) {
        // SAFETY: the range the kernel mapped for the message, unmapped
        // once; `len` came from a `u32`.
        unsafe { mach_vm_deallocate(mach_task_self(), self.address, self.len as u64) };
    }
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
    match parse(&buffer.words, buffer.out_of_line) {
        Some(parsed) => Ok(parsed.own()),
        None => {
            // The header's local port: the one it came to.
            buffer.refused_at = buffer.words.get(3).copied().unwrap_or(MACH_PORT_NULL);
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
    port: mach_port_t,
    body: Range<usize>,
    /// The address and length of memory it brought out of line.
    out_of_line: Option<(u64, usize)>,
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
        let body = match self.out_of_line {
            Some((address, len)) => Body::OutOfLine(Mapped { address, len }),
            None => Body::Inline(self.body),
        };
        Received {
            id: self.id,
            reply,
            rights,
            sender: Sender { token: self.token },
            port: self.port,
            body,
        }
    }
}

/// `out_of_line` is the most memory a message may bring out of line.
fn parse(words: &[u32], out_of_line: usize) -> Option<Parsed> {
    let (&[bits, size, reply, port, _voucher, id], mut rest) =
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
    let mut memory = None;
    if bits & MACH_MSGH_BITS_COMPLEX != 0 {
        let (&count, after) = rest.split_first()?;
        rest = after;
        at = at.checked_add(1)?;
        for _ in 0..count {
            // Whatever its kind, a descriptor's kind is the top byte of
            // its third word.
            let kind = rest.get(2)? >> 24;
            if kind == MACH_MSG_PORT_DESCRIPTOR {
                let (&[name, _pad, flags], after) =
                    rest.split_first_chunk::<PORT_DESCRIPTOR_WORDS>()?;
                rest = after;
                at = at.checked_add(PORT_DESCRIPTOR_WORDS)?;
                let disposition = (flags >> 16) & 0xff;
                let sent_as_right = disposition == MACH_MSG_TYPE_MOVE_SEND
                    || disposition == MACH_MSG_TYPE_MOVE_RECEIVE;
                if !sent_as_right || rights.len() >= MAX_RIGHTS {
                    return None;
                }
                rights.push((name, disposition));
            } else if kind == MACH_MSG_OOL_DESCRIPTOR {
                let (&[low, high, _flags, len], after) =
                    rest.split_first_chunk::<OOL_DESCRIPTOR_WORDS>()?;
                rest = after;
                at = at.checked_add(OOL_DESCRIPTOR_WORDS)?;
                let len = usize::try_from(len).ok()?;
                // One piece of memory, no more than this end takes.
                if memory.is_some() || len > out_of_line {
                    return None;
                }
                memory = Some((u64::from(low) | (u64::from(high) << 32), len));
            } else {
                return None;
            }
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
    // A body is in the message or out of line, not both.
    if memory.is_some() && !body.is_empty() {
        return None;
    }
    // The trailer follows the message.
    let trailer = words.get(size_words..)?;
    let token = *trailer.get(AUDIT_TOKEN_AT..)?.first_chunk::<8>()?;
    Some(Parsed {
        id: id.cast_signed(),
        reply,
        rights,
        token,
        port,
        body,
        out_of_line: memory,
    })
}

/// Sends one message to `to` without waiting, its body out of line when
/// it is longer than the buffer holds inline, and with a copy of each of
/// `rights`: sent or not, the rights given stay the caller's.
/// [`SendError::TimedOut`] says the port's queue is full.
pub fn try_send(
    buffer: &mut Buffer,
    to: &SendRight,
    id: i32,
    body: &[u8],
    rights: &[SendRight],
) -> Result<(), SendError> {
    let inline = body.len() <= buffer.body;
    if rights.len() > MAX_RIGHTS || (!inline && body.len() > buffer.out_of_line) {
        return Err(SendError::TooLarge);
    }
    let body_len = u32::try_from(body.len()).map_err(|_| SendError::TooLarge)?;
    let descriptors = rights.len().saturating_add(usize::from(!inline));
    let complex = descriptors > 0;
    let mut words = Vec::with_capacity(
        HEADER_WORDS
            .saturating_add(2)
            .saturating_add(rights.len().saturating_mul(PORT_DESCRIPTOR_WORDS))
            .saturating_add(OOL_DESCRIPTOR_WORDS),
    );
    let bits = MACH_MSG_TYPE_COPY_SEND | if complex { MACH_MSGH_BITS_COMPLEX } else { 0 };
    words.extend([
        bits,
        0,
        to.name(),
        MACH_PORT_NULL,
        MACH_PORT_NULL,
        id.cast_unsigned(),
    ]);
    if complex {
        words.push(u32::try_from(descriptors).map_err(|_| SendError::TooLarge)?);
        for right in rights {
            // A copy: the caller's own right is its own whatever comes of
            // the send.
            words.extend([
                right.name(),
                0,
                (MACH_MSG_TYPE_COPY_SEND << 16) | (MACH_MSG_PORT_DESCRIPTOR << 24),
            ]);
        }
        if !inline {
            let address = body.as_ptr().expose_provenance() as u64;
            // The low and high halves of the address.
            #[expect(
                clippy::cast_possible_truncation,
                reason = "each half of a `u64` is a `u32`"
            )]
            let (low, high) = (address as u32, (address >> 32) as u32);
            // Not deallocated from this task (the bytes are the caller's),
            // and copied on write: what the receiver gets is the bytes as
            // they are now.
            words.extend([
                low,
                high,
                (MACH_MSG_VIRTUAL_COPY << 8) | (MACH_MSG_OOL_DESCRIPTOR << 24),
                body_len,
            ]);
        }
    }
    words.push(if inline { body_len } else { 0 });
    let before_body = words.len();
    let inline_words = if inline { body.len().div_ceil(4) } else { 0 };
    let total = before_body.saturating_add(inline_words);
    let size = total
        .checked_mul(4)
        .and_then(|size| u32::try_from(size).ok());
    let fits = total.saturating_add(TRAILER_WORDS) <= buffer.words.len();
    let (Some(size), true) = (size, fits) else {
        return Err(SendError::TooLarge);
    };
    if let Some(size_word) = words.get_mut(1) {
        *size_word = size;
    }
    let Some((head, _)) = buffer.words.split_at_mut_checked(before_body) else {
        return Err(SendError::TooLarge);
    };
    head.copy_from_slice(&words);
    if inline {
        // The body's last word is padded with zeroes.
        if let Some(last) = buffer
            .words
            .get_mut(total.saturating_sub(1))
            .filter(|_| inline_words > 0)
        {
            *last = 0;
        }
        let start = before_body.saturating_mul(4);
        if let Some(into) = buffer
            .bytes_mut()
            .get_mut(start..start.saturating_add(body.len()))
        {
            into.copy_from_slice(body);
        }
    }
    let header = buffer.words.as_mut_ptr().cast::<mach_msg_header_t>();
    // SAFETY: the buffer holds a whole message of `size` bytes, aligned
    // as a header is; the rights it names are the caller's, copied; the
    // memory it names is `body`, alive for the call, which the kernel
    // copies (on write) and leaves mapped here.
    let result = unsafe {
        mach_msg(
            header,
            MACH_SEND_MSG | MACH_SEND_TIMEOUT,
            size,
            0,
            MACH_PORT_NULL,
            0,
            MACH_PORT_NULL,
        )
    };
    match result {
        MACH_MSG_SUCCESS => Ok(()),
        failed => {
            // SAFETY: a send that fails hands the message back as if it
            // had been received: a reference more to each right it named
            // and its memory mapped anew. This lets go of exactly those,
            // and leaves what the caller holds as it was.
            unsafe { mach_msg_destroy(header) };
            Err(if failed == MACH_SEND_TIMED_OUT {
                SendError::TimedOut
            } else {
                SendError::Kernel(failed)
            })
        }
    }
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

    #[test]
    fn a_long_body_goes_out_of_line_and_arrives_the_same() {
        let (receive, to) = port();
        let mut buffer = Buffer::with_out_of_line(64, 1 << 20);
        for len in [0usize, 64, 65, 4096, 100_000, 1 << 20] {
            let body: Vec<u8> = (0..len)
                .map(|i| (i as u8).wrapping_mul(13).wrapping_add(5))
                .collect();
            try_send(&mut buffer, &to, ID, &body, &[]).unwrap();
            let received = receive_one(&mut buffer, &receive);
            assert!(buffer.body(&received) == &body[..], "{len} bytes");
            assert_eq!(
                matches!(received.body, Body::OutOfLine(_)),
                len > 64,
                "{len} bytes"
            );
            assert_eq!(received.port, receive.name());
        }
        // Longer than this end sends at all.
        assert_eq!(
            try_send(&mut buffer, &to, ID, &vec![0; (1 << 20) + 1], &[]),
            Err(SendError::TooLarge)
        );
    }

    #[test]
    fn what_is_sent_out_of_line_is_the_bytes_as_they_were_when_sent() {
        let (receive, to) = port();
        let mut buffer = Buffer::with_out_of_line(64, 1 << 20);
        let mut body = vec![1u8; 200_000];
        try_send(&mut buffer, &to, ID, &body, &[]).unwrap();
        // The sender writes over them after: the receiver's are its own.
        body.fill(2);
        let received = receive_one(&mut buffer, &receive);
        assert!(buffer.body(&received).iter().all(|byte| *byte == 1));
        assert!(
            body.iter().all(|byte| *byte == 2),
            "and the sender's are still the sender's"
        );
    }

    #[test]
    fn out_of_line_memory_is_taken_only_where_allowed_and_as_much() {
        let (receive, to) = port();
        let mut sending = Buffer::with_out_of_line(64, 1 << 20);
        let body = vec![7u8; 300_000];
        // A buffer that takes none.
        try_send(&mut sending, &to, ID, &body, &[]).unwrap();
        let mut inline_only = Buffer::for_body(64);
        assert_eq!(
            super::receive(&mut inline_only, receive.name(), SOON).unwrap_err(),
            ReceiveError::Malformed
        );
        // One that takes less; exactly as much is taken.
        try_send(&mut sending, &to, ID, &body, &[]).unwrap();
        let mut smaller = Buffer::with_out_of_line(64, 299_999);
        assert_eq!(
            super::receive(&mut smaller, receive.name(), SOON).unwrap_err(),
            ReceiveError::Malformed
        );
        try_send(&mut sending, &to, ID, &body, &[]).unwrap();
        let mut exact = Buffer::with_out_of_line(64, 300_000);
        let received = receive_one(&mut exact, &receive);
        assert_eq!(exact.body(&received).len(), 300_000);
    }

    /// The address space this task has mapped, in bytes.
    fn virtual_size() -> u64 {
        use mach2::task_info::{
            MACH_TASK_BASIC_INFO, MACH_TASK_BASIC_INFO_COUNT, mach_task_basic_info,
        };

        let mut info = mach_task_basic_info::default();
        let mut count = MACH_TASK_BASIC_INFO_COUNT;
        // SAFETY: the call writes at most `count` words of `info`, which
        // is that long and alive for the call.
        let result = unsafe {
            mach2::task::task_info(
                mach_task_self(),
                MACH_TASK_BASIC_INFO,
                (&raw mut info).cast(),
                &raw mut count,
            )
        };
        assert_eq!(result, mach2::kern_return::KERN_SUCCESS);
        info.virtual_size
    }

    #[test]
    fn memory_received_out_of_line_is_unmapped_with_its_message() {
        let (receive, to) = port();
        let mut buffer = Buffer::with_out_of_line(64, 16 << 20);
        let body = vec![3u8; 16 << 20];
        let before = virtual_size();
        // 2 GiB in all, received and dropped, then refused.
        for _ in 0..64 {
            try_send(&mut buffer, &to, ID, &body, &[]).unwrap();
            drop(receive_one(&mut buffer, &receive));
            try_send(&mut buffer, &to, ID, &body, &[]).unwrap();
            let mut refusing = Buffer::for_body(64);
            assert_eq!(
                super::receive(&mut refusing, receive.name(), SOON).unwrap_err(),
                ReceiveError::Malformed
            );
        }
        let grew = virtual_size().saturating_sub(before);
        assert!(grew < 256 << 20, "the address space grew by {grew} bytes");
    }

    #[test]
    fn a_send_that_finds_the_queue_full_leaves_everything_as_it_was() {
        let (receive, to) = port();
        let (carried_port, carried) = port();
        let mut buffer = Buffer::with_out_of_line(64, 16 << 20);
        let mut filled = 0;
        while try_send(&mut buffer, &to, ID, b"fill", &[]).is_ok() {
            filled += 1;
            assert!(filled < 10_000);
        }
        let body = vec![9u8; 16 << 20];
        let rights = [carried];
        let before = virtual_size();
        for _ in 0..64 {
            assert_eq!(
                try_send(&mut buffer, &to, ID, &body, &rights),
                Err(SendError::TimedOut)
            );
        }
        // Neither the memory the kernel mapped back nor the references
        // it gave back are kept.
        let grew = virtual_size().saturating_sub(before);
        assert!(grew < 256 << 20, "the address space grew by {grew} bytes");
        assert!(
            !no_senders(&carried_port),
            "the caller's right is still its own"
        );
        // With room, the same call goes, and the right is still the
        // caller's after: the receiver has one of its own.
        while super::receive(&mut buffer, receive.name(), Some(Duration::from_millis(20))).is_ok() {
        }
        assert_eq!(try_send(&mut buffer, &to, ID, &body, &rights), Ok(()));
        let mut received = receive_one(&mut buffer, &receive);
        assert!(buffer.body(&received) == &body[..]);
        let Some(Right::Send(theirs)) = received.rights.pop() else {
            panic!("a send right");
        };
        drop(rights);
        assert!(
            !no_senders(&carried_port),
            "the receiver's is one of its own"
        );
        drop(theirs);
        drop(received);
        assert!(no_senders(&carried_port));
    }

    #[test]
    fn the_kernel_says_when_a_full_port_has_room_and_when_a_port_dies() {
        use super::super::port::PortSet;

        let (receive, to) = port();
        let notify = ReceiveRight::new().unwrap();
        let mut buffer = Buffer::for_body(64);
        while try_send(&mut buffer, &to, ID, b"fill", &[]).is_ok() {}
        assert!(to.tell_when_sendable(&notify));
        let quiet = Some(Duration::from_millis(50));
        assert_eq!(
            super::receive(&mut buffer, notify.name(), quiet).unwrap_err(),
            ReceiveError::TimedOut
        );
        receive_one(&mut buffer, &receive);
        let told = receive_one(&mut buffer, &notify);
        assert_eq!(
            (told.id, told.sender.pid()),
            (mach2::notify::MACH_NOTIFY_SEND_POSSIBLE, 0)
        );
        assert_eq!(try_send(&mut buffer, &to, ID, b"room", &[]), Ok(()));
        // A queue made as long as one can be holds many more.
        let (long, to_long) = port();
        assert!(long.raise_queue_limit());
        let mut held = 0;
        while try_send(&mut buffer, &to_long, ID, b"fill", &[]).is_ok() {
            held += 1;
            assert!(held < 10_000);
        }
        assert_eq!(held, 1024);
        // A port's death is told to whoever asked, on a set as well as on
        // the port; the notification's own reference is let go.
        let set = PortSet::new().unwrap();
        assert!(set.add(&notify));
        assert!(to.tell_when_dead(&notify));
        assert_eq!(
            super::receive(&mut buffer, set.name(), quiet).unwrap_err(),
            ReceiveError::TimedOut
        );
        drop(receive);
        let told = super::receive(&mut buffer, set.name(), SOON).unwrap();
        assert_eq!(
            (told.id, told.sender.pid()),
            (mach2::notify::MACH_NOTIFY_DEAD_NAME, 0)
        );
        assert_eq!(
            told.port,
            notify.name(),
            "which of the set's ports it came to"
        );
        to.dead_name_told();
        assert!(matches!(
            try_send(&mut buffer, &to, ID, b"", &[]),
            Err(SendError::Kernel(_))
        ));
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
        // Memory out of line is one piece, with no body in the message
        // beside it: two pieces, and a piece with a body, are refused by
        // a buffer that would take either alone.
        let mut taking = Buffer::with_out_of_line(256, 1 << 20);
        let ool = |memory: &[u8]| {
            let address = memory.as_ptr() as u64;
            [
                address as u32,
                (address >> 32) as u32,
                (1 << 8) | (1 << 24),
                memory.len() as u32,
            ]
        };
        let mut one = vec![plain | MACH_MSGH_BITS_COMPLEX, 0, 0, 0, 0, 1, 1];
        one.extend(ool(&memory));
        one.push(0);
        send_raw(&to, one.clone());
        let received = receive_one(&mut taking, &receive);
        assert_eq!(taking.body(&received).len(), 1 << 16);
        drop(received);
        let mut two = vec![plain | MACH_MSGH_BITS_COMPLEX, 0, 0, 0, 0, 1, 2];
        two.extend(ool(&memory));
        two.extend(ool(&memory));
        two.push(0);
        send_raw(&to, two);
        assert_eq!(
            super::receive(&mut taking, receive.name(), SOON).unwrap_err(),
            ReceiveError::Malformed
        );
        let mut both = one;
        *both.last_mut().unwrap() = 4;
        both.push(0xaaaa_aaaa);
        send_raw(&to, both);
        assert_eq!(
            super::receive(&mut taking, receive.name(), SOON).unwrap_err(),
            ReceiveError::Malformed
        );
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
