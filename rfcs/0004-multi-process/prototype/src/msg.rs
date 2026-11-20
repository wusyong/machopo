//! A Mach message, put together and taken apart by hand: the kernel's
//! structures are packed to 4 bytes (`#pragma pack(4)`), which `#[repr(C)]`
//! ones aren't once a descriptor has a pointer in it, so every field is
//! written and read at its offset.
//!
//! One message: the header; if it carries rights or out-of-line memory, the
//! descriptor count and the descriptors; then `destination: u32`,
//! `length: u32` and the payload, padded to 4 bytes.

use mach2::kern_return::{KERN_SUCCESS, kern_return_t};
use mach2::message::{
    MACH_MSG_OOL_DESCRIPTOR, MACH_MSG_PORT_DESCRIPTOR, MACH_MSG_SUCCESS, MACH_MSG_TIMEOUT_NONE,
    MACH_MSG_TYPE_COPY_SEND, MACH_MSG_VIRTUAL_COPY, MACH_MSGH_BITS_COMPLEX, MACH_RCV_LARGE,
    MACH_RCV_MSG, MACH_RCV_TIMED_OUT, MACH_RCV_TIMEOUT, MACH_RCV_TOO_LARGE, MACH_SEND_MSG,
    MACH_SEND_TIMEOUT, audit_token_t, mach_msg, mach_msg_header_t, mach_msg_type_name_t,
};
use mach2::port::{MACH_PORT_NULL, mach_port_t};
use mach2::traps::mach_task_self;
use mach2::vm::mach_vm_deallocate;

use crate::sys::{AUDIT_TRAILER_SIZE, RCV_AUDIT_TRAILER, audit_token_to_pid};

const HEADER: usize = 24;
const PORT_DESCRIPTOR: usize = 12;
const OOL_DESCRIPTOR: usize = 16;

/// A buffer for one message, aligned as the header wants.
pub struct MsgBuf {
    words: Vec<u64>,
    len: usize,
}

impl MsgBuf {
    pub fn with_capacity(bytes: usize) -> Self {
        Self {
            words: vec![0; bytes.div_ceil(8)],
            len: 0,
        }
    }

    fn capacity(&self) -> usize {
        self.words.len() * 8
    }

    fn reserve(&mut self, bytes: usize) {
        if bytes > self.capacity() {
            self.words.resize(bytes.div_ceil(8), 0);
        }
    }

    fn ptr(&mut self) -> *mut u8 {
        self.words.as_mut_ptr().cast()
    }

    fn bytes(&self) -> &[u8] {
        // SAFETY: the words are initialized, and a `u64`'s bytes are bytes.
        unsafe { std::slice::from_raw_parts(self.words.as_ptr().cast(), self.capacity()) }
    }

    fn bytes_mut(&mut self) -> &mut [u8] {
        let capacity = self.capacity();
        // SAFETY: as `bytes`.
        unsafe { std::slice::from_raw_parts_mut(self.words.as_mut_ptr().cast(), capacity) }
    }

    fn put_u32(&mut self, at: usize, value: u32) {
        self.bytes_mut()[at..at + 4].copy_from_slice(&value.to_ne_bytes());
    }

    fn put_u64(&mut self, at: usize, value: u64) {
        self.bytes_mut()[at..at + 8].copy_from_slice(&value.to_ne_bytes());
    }

    fn u32_at(&self, at: usize) -> Option<u32> {
        let bytes = self.bytes().get(at..at.checked_add(4)?)?;
        Some(u32::from_ne_bytes(bytes.try_into().ok()?))
    }

    fn u64_at(&self, at: usize) -> Option<u64> {
        let bytes = self.bytes().get(at..at.checked_add(8)?)?;
        Some(u64::from_ne_bytes(bytes.try_into().ok()?))
    }
}

/// What to send.
pub struct Outgoing<'a> {
    /// `msgh_id`.
    pub id: i32,
    pub destination: u32,
    pub payload: &'a [u8],
    /// Rights to hand over, each with how (`MACH_MSG_TYPE_MOVE_RECEIVE`,
    /// `COPY_SEND`, `MAKE_SEND`).
    pub ports: &'a [(mach_port_t, mach_msg_type_name_t)],
    /// Memory to send out of line (copy-on-write, not copied into the
    /// message).
    pub ool: Option<&'a [u8]>,
}

/// Sends `message` to `remote` (a send right, or a send-once right with
/// `remote_disposition` `MOVE_SEND_ONCE`), naming `reply` as its reply
/// port if any. `timeout_ms` `None` waits for room in the port's queue;
/// `Some(0)` never waits (`MACH_SEND_TIMED_OUT` when it is full).
pub fn send(
    buf: &mut MsgBuf,
    remote: mach_port_t,
    remote_disposition: mach_msg_type_name_t,
    reply: Option<(mach_port_t, mach_msg_type_name_t)>,
    message: &Outgoing<'_>,
    timeout_ms: Option<u32>,
) -> Result<(), kern_return_t> {
    let complex = !message.ports.is_empty() || message.ool.is_some();
    let descriptors = message.ports.len() * PORT_DESCRIPTOR
        + if message.ool.is_some() { OOL_DESCRIPTOR } else { 0 };
    let inline_at = HEADER + if complex { 4 + descriptors } else { 0 };
    let size = (inline_at + 8 + message.payload.len()).next_multiple_of(4);
    buf.reserve(size);
    let (reply_port, reply_disposition) = reply.unwrap_or((MACH_PORT_NULL, 0));
    let mut bits = remote_disposition | (reply_disposition << 8);
    if complex {
        bits |= MACH_MSGH_BITS_COMPLEX;
    }
    buf.put_u32(0, bits);
    buf.put_u32(4, u32::try_from(size).expect("a message under 4 GiB"));
    buf.put_u32(8, remote);
    buf.put_u32(12, reply_port);
    buf.put_u32(16, MACH_PORT_NULL);
    buf.put_u32(20, message.id.cast_unsigned());
    let mut at = HEADER;
    if complex {
        let count = message.ports.len() + usize::from(message.ool.is_some());
        buf.put_u32(at, u32::try_from(count).unwrap());
        at += 4;
        for &(port, disposition) in message.ports {
            buf.put_u32(at, port);
            buf.put_u32(at + 4, 0);
            // pad2: 16, disposition: 8, type: 8.
            buf.put_u32(at + 8, (disposition << 16) | (MACH_MSG_PORT_DESCRIPTOR << 24));
            at += PORT_DESCRIPTOR;
        }
        if let Some(ool) = message.ool {
            buf.put_u64(at, ool.as_ptr() as u64);
            // deallocate: 8, copy: 8, pad1: 8, type: 8.
            buf.put_u32(
                at + 8,
                (MACH_MSG_VIRTUAL_COPY << 8) | (MACH_MSG_OOL_DESCRIPTOR << 24),
            );
            buf.put_u32(at + 12, u32::try_from(ool.len()).expect("under 4 GiB"));
            at += OOL_DESCRIPTOR;
        }
    }
    buf.put_u32(at, message.destination);
    buf.put_u32(
        at + 4,
        u32::try_from(message.payload.len()).expect("under 4 GiB"),
    );
    buf.bytes_mut()[at + 8..at + 8 + message.payload.len()].copy_from_slice(message.payload);
    let (option, timeout) = match timeout_ms {
        Some(ms) => (MACH_SEND_MSG | MACH_SEND_TIMEOUT, ms),
        None => (MACH_SEND_MSG, MACH_MSG_TIMEOUT_NONE),
    };
    // SAFETY: the buffer holds a whole message of `size` bytes; the rights
    // it names are the caller's to give.
    let kr = unsafe {
        mach_msg(
            buf.ptr().cast::<mach_msg_header_t>(),
            option,
            u32::try_from(size).unwrap(),
            0,
            MACH_PORT_NULL,
            timeout,
            MACH_PORT_NULL,
        )
    };
    if kr == MACH_MSG_SUCCESS { Ok(()) } else { Err(kr) }
}

/// Out-of-line memory a message brought: the receiver's to unmap.
pub struct OolMemory {
    address: u64,
    len: usize,
}

impl OolMemory {
    pub fn len(&self) -> usize {
        self.len
    }

    /// Copies it out. (Ours alone: the kernel gave this task its own
    /// copy-on-write mapping.)
    pub fn to_vec(&self) -> Vec<u8> {
        // SAFETY: the kernel mapped `len` bytes at `address` for this
        // message, and nothing else has them until `drop`.
        unsafe { std::slice::from_raw_parts(self.address as *const u8, self.len) }.to_vec()
    }
}

impl Drop for OolMemory {
    fn drop(&mut self) {
        // SAFETY: the mapping is this message's.
        unsafe { mach_vm_deallocate(mach_task_self(), self.address, self.len as u64) };
    }
}

/// A message received.
pub struct Incoming {
    pub id: i32,
    /// The reply port's right the sender gave, if any (a send-once right).
    pub reply: mach_port_t,
    pub destination: u32,
    pub payload: Vec<u8>,
    /// The rights it carried, now this task's.
    pub ports: Vec<mach_port_t>,
    pub ool: Option<OolMemory>,
    /// The sender's pid, by the kernel's audit token: 0 is the kernel.
    pub sender_pid: i32,
}

pub enum RecvError {
    TimedOut,
    Kernel(kern_return_t),
    /// A message that isn't one of ours.
    Malformed(&'static str),
}

/// Receives one message on `port` (a receive right or a port set).
pub fn recv(buf: &mut MsgBuf, port: mach_port_t, timeout_ms: Option<u32>) -> Result<Incoming, RecvError> {
    let (mut option, timeout) = match timeout_ms {
        Some(ms) => (MACH_RCV_MSG | MACH_RCV_TIMEOUT, ms),
        None => (MACH_RCV_MSG, MACH_MSG_TIMEOUT_NONE),
    };
    option |= MACH_RCV_LARGE | RCV_AUDIT_TRAILER;
    loop {
        let capacity = u32::try_from(buf.capacity()).unwrap();
        // SAFETY: the buffer is `capacity` bytes, aligned for a header.
        let kr = unsafe {
            mach_msg(
                buf.ptr().cast::<mach_msg_header_t>(),
                option,
                0,
                capacity,
                port,
                timeout,
                MACH_PORT_NULL,
            )
        };
        match kr {
            MACH_MSG_SUCCESS => break,
            MACH_RCV_TOO_LARGE => {
                // With `MACH_RCV_LARGE` the message stays queued and the
                // header says how big it is.
                let needed = buf.u32_at(4).unwrap_or(0) as usize + AUDIT_TRAILER_SIZE;
                buf.reserve(needed.max(buf.capacity() * 2));
            }
            MACH_RCV_TIMED_OUT => return Err(RecvError::TimedOut),
            kr => return Err(RecvError::Kernel(kr)),
        }
    }
    parse(buf)
}

fn parse(buf: &mut MsgBuf) -> Result<Incoming, RecvError> {
    let malformed = RecvError::Malformed;
    let bits = buf.u32_at(0).ok_or(malformed("no header"))?;
    let size = buf.u32_at(4).ok_or(malformed("no header"))? as usize;
    // After a receive the sender's reply right is the header's remote port.
    let reply = buf.u32_at(8).unwrap_or(MACH_PORT_NULL);
    let id = buf.u32_at(20).unwrap_or(0).cast_signed();
    buf.len = size;
    let mut at = HEADER;
    let mut ports = Vec::new();
    let mut ool = None;
    if bits & MACH_MSGH_BITS_COMPLEX != 0 {
        let count = buf.u32_at(at).ok_or(malformed("no descriptor count"))?;
        at += 4;
        for _ in 0..count {
            let kind = buf.u32_at(at + 8).ok_or(malformed("a cut descriptor"))? >> 24;
            if kind == MACH_MSG_PORT_DESCRIPTOR {
                ports.push(buf.u32_at(at).unwrap());
                at += PORT_DESCRIPTOR;
            } else if kind == MACH_MSG_OOL_DESCRIPTOR {
                let address = buf.u64_at(at).ok_or(malformed("a cut descriptor"))?;
                let len = buf.u32_at(at + 12).ok_or(malformed("a cut descriptor"))? as usize;
                ool = Some(OolMemory { address, len });
                at += OOL_DESCRIPTOR;
            } else {
                return Err(malformed("a descriptor of another kind"));
            }
        }
    }
    // The trailer follows the message, rounded up to 4 bytes.
    let trailer = size.next_multiple_of(4);
    let mut token = audit_token_t::default();
    for (i, value) in token.val.iter_mut().enumerate() {
        *value = buf.u32_at(trailer + 20 + i * 4).ok_or(malformed("no audit trailer"))?;
    }
    // SAFETY: a call with a value, into libbsm.
    let sender_pid = unsafe { audit_token_to_pid(token) };
    // The kernel's notifications have no destination and payload of ours.
    let (destination, payload) = if at + 8 <= size {
        let destination = buf.u32_at(at).unwrap();
        let len = buf.u32_at(at + 4).unwrap() as usize;
        let start = at + 8;
        let end = start.checked_add(len).filter(|&end| end <= size);
        match end {
            Some(end) => (destination, buf.bytes()[start..end].to_vec()),
            None => return Err(malformed("a payload longer than its message")),
        }
    } else {
        (u32::MAX, Vec::new())
    };
    Ok(Incoming {
        id,
        reply,
        destination,
        payload,
        ports,
        ool,
        sender_pid,
    })
}

/// Drops a send (or send-once, or dead-name) right.
pub fn deallocate(port: mach_port_t) {
    if port != MACH_PORT_NULL {
        // SAFETY: a call on this task's own name.
        unsafe { mach2::mach_port::mach_port_deallocate(mach_task_self(), port) };
    }
}

/// A new port: its receive right.
pub fn allocate_receive() -> mach_port_t {
    let mut port = MACH_PORT_NULL;
    // SAFETY: a call that writes one name.
    let kr = unsafe {
        mach2::mach_port::mach_port_allocate(
            mach_task_self(),
            mach2::port::MACH_PORT_RIGHT_RECEIVE,
            &mut port,
        )
    };
    assert_eq!(kr, KERN_SUCCESS, "mach_port_allocate");
    port
}

/// A send right to `receive`, this task's receive right, under the same
/// name.
pub fn insert_send(receive: mach_port_t) {
    // SAFETY: a call on this task's own receive right.
    let kr = unsafe {
        mach2::mach_port::mach_port_insert_right(
            mach_task_self(),
            receive,
            receive,
            mach2::message::MACH_MSG_TYPE_MAKE_SEND,
        )
    };
    assert_eq!(kr, KERN_SUCCESS, "mach_port_insert_right");
}

/// Drops a receive right (the port dies: its senders' rights become dead
/// names).
pub fn drop_receive(port: mach_port_t) {
    // SAFETY: a call on this task's own name.
    unsafe {
        mach2::mach_port::mach_port_mod_refs(
            mach_task_self(),
            port,
            mach2::port::MACH_PORT_RIGHT_RECEIVE,
            -1,
        )
    };
}

pub const COPY_SEND: mach_msg_type_name_t = MACH_MSG_TYPE_COPY_SEND;
