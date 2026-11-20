//! A control channel's end: this task's receive port and a send right to
//! the peer's. Whether an end may attach rights to its messages is the
//! end's own permission, set by whoever made the channel: a message with
//! rights from an end without it is a bad message.

use mach2::kern_return::KERN_SUCCESS;
use mach2::message::{MACH_MSG_TYPE_MAKE_SEND_ONCE, MACH_SEND_INVALID_DEST, mach_msg_type_name_t};
use mach2::notify::{MACH_NOTIFY_DEAD_NAME, MACH_NOTIFY_NO_SENDERS};
use mach2::port::{MACH_PORT_NULL, mach_port_t};
use mach2::traps::mach_task_self;

use crate::msg::{self, Incoming, MsgBuf, Outgoing, RecvError};
use crate::sys;

/// `msgh_id` of the channel's messages.
pub const MESSAGE_ID: i32 = 0x4d50;

#[derive(Debug)]
pub enum ChannelError {
    /// The peer has gone (the kernel said so, or a send found its port
    /// dead).
    Disconnected,
    /// The peer sent what it may not.
    BadMessage(&'static str),
    TimedOut,
}

pub struct Received {
    pub destination: u32,
    pub payload: Vec<u8>,
    pub ports: Vec<mach_port_t>,
    pub ool: Option<msg::OolMemory>,
}

pub struct Channel {
    receive: mach_port_t,
    send: mach_port_t,
    may_attach: bool,
    peer_may_attach: bool,
    /// The peer's pid, if messages are checked against it.
    peer_pid: Option<i32>,
    buf: MsgBuf,
    /// Notifications that didn't come from the kernel.
    pub forged_notifications: u32,
}

impl Channel {
    /// An end of `receive` (this task's receive right) and `send` (a send
    /// right to the peer's port). Asks the kernel to say when the peer's
    /// port dies (dead-name, on the send right) and when no send right to
    /// ours is left (no-senders): both come as messages on `receive`.
    pub fn new(receive: mach_port_t, send: mach_port_t, may_attach: bool, peer_may_attach: bool) -> Self {
        for (name, id) in [(send, MACH_NOTIFY_DEAD_NAME), (receive, MACH_NOTIFY_NO_SENDERS)] {
            let mut previous = MACH_PORT_NULL;
            // SAFETY: calls on this task's own names; the notification
            // goes to a send-once right made from our receive right.
            let kr = unsafe {
                sys::mach_port_request_notification(
                    mach_task_self(),
                    name,
                    id,
                    // Dead-name: 1 tells even if it is dead already.
                    // No-senders: told once no send right is left and at
                    // least this many were ever made; 0 would tell at once
                    // for a port whose first send right the rendezvous
                    // hasn't made yet.
                    1,
                    receive,
                    MACH_MSG_TYPE_MAKE_SEND_ONCE,
                    &mut previous,
                )
            };
            assert_eq!(kr, KERN_SUCCESS, "mach_port_request_notification({id})");
            msg::deallocate(previous);
        }
        Self {
            receive,
            send,
            may_attach,
            peer_may_attach,
            peer_pid: None,
            buf: MsgBuf::with_capacity(64 * 1024),
            forged_notifications: 0,
        }
    }

    /// Lets up to 1024 messages wait in this end's port (5 by default).
    pub fn raise_queue_limit(&self) {
        let limit = sys::MACH_PORT_QLIMIT_LARGE;
        // SAFETY: a call on this task's own receive right, with one word.
        let kr = unsafe {
            sys::mach_port_set_attributes(mach_task_self(), self.receive, sys::MACH_PORT_LIMITS_INFO, &limit, 1)
        };
        assert_eq!(kr, KERN_SUCCESS, "mach_port_set_attributes");
    }

    pub fn expect_peer(&mut self, pid: i32) {
        self.peer_pid = Some(pid);
    }

    pub fn receive_port(&self) -> mach_port_t {
        self.receive
    }

    pub fn send(&mut self, destination: u32, payload: &[u8]) -> Result<(), ChannelError> {
        self.send_with(destination, payload, &[], None, None)
    }

    /// `timeout_ms` `Some(0)`: never waits for room in the peer's queue
    /// (`TimedOut` when it is full); `None` waits.
    pub fn send_with(
        &mut self,
        destination: u32,
        payload: &[u8],
        ports: &[(mach_port_t, mach_msg_type_name_t)],
        ool: Option<&[u8]>,
        timeout_ms: Option<u32>,
    ) -> Result<(), ChannelError> {
        assert!(
            self.may_attach || ports.is_empty(),
            "this end may not attach rights"
        );
        let message = Outgoing {
            id: MESSAGE_ID,
            destination,
            payload,
            ports,
            ool,
        };
        match msg::send(&mut self.buf, self.send, msg::COPY_SEND, None, &message, timeout_ms) {
            Ok(()) => Ok(()),
            Err(MACH_SEND_INVALID_DEST) => Err(ChannelError::Disconnected),
            Err(mach2::message::MACH_SEND_TIMED_OUT) => Err(ChannelError::TimedOut),
            Err(kr) => panic!("mach_msg send: {kr:#x}"),
        }
    }

    /// Sends rights whatever this end's permission: what a misbehaving
    /// peer would do.
    pub fn send_forbidden(&mut self, destination: u32, ports: &[(mach_port_t, mach_msg_type_name_t)]) {
        let message = Outgoing {
            id: MESSAGE_ID,
            destination,
            payload: &[],
            ports,
            ool: None,
        };
        let _ = msg::send(&mut self.buf, self.send, msg::COPY_SEND, None, &message, None);
    }

    /// Sends a message with a notification's id: what a peer forging the
    /// kernel's would do.
    pub fn send_forged_notification(&mut self) {
        let message = Outgoing {
            id: MACH_NOTIFY_DEAD_NAME,
            destination: 0,
            payload: &[],
            ports: &[],
            ool: None,
        };
        let _ = msg::send(&mut self.buf, self.send, msg::COPY_SEND, None, &message, None);
    }

    pub fn recv(&mut self, timeout_ms: Option<u32>) -> Result<Received, ChannelError> {
        loop {
            let incoming = match msg::recv(&mut self.buf, self.receive, timeout_ms) {
                Ok(incoming) => incoming,
                Err(RecvError::TimedOut) => return Err(ChannelError::TimedOut),
                Err(RecvError::Kernel(kr)) => panic!("mach_msg receive: {kr:#x}"),
                Err(RecvError::Malformed(why)) => return Err(ChannelError::BadMessage(why)),
            };
            let Incoming {
                id,
                reply,
                destination,
                payload,
                ports,
                ool,
                sender_pid,
            } = incoming;
            msg::deallocate(reply);
            if id == MACH_NOTIFY_DEAD_NAME || id == MACH_NOTIFY_NO_SENDERS {
                // Only the kernel's count: a peer can send a message with
                // the same id.
                if sender_pid == 0 {
                    return Err(ChannelError::Disconnected);
                }
                self.forged_notifications += 1;
                continue;
            }
            let bad = |why| {
                for &port in &ports {
                    msg::deallocate(port);
                }
                Err(ChannelError::BadMessage(why))
            };
            if id != MESSAGE_ID {
                return bad("a message of another id");
            }
            if self.peer_pid.is_some_and(|pid| pid != sender_pid) {
                return bad("a message from another process");
            }
            if !self.peer_may_attach && (!ports.is_empty() || ool.is_some()) {
                return bad("rights from an end that may not attach them");
            }
            return Ok(Received {
                destination,
                payload,
                ports,
                ool,
            });
        }
    }
}

/// Both ends of a channel, made by one task: `(ours, theirs)`, where
/// `theirs` is the rights to hand to the peer: its receive right (to move)
/// and a send right to ours (to make from our receive right).
pub struct ChannelPair {
    pub our_receive: mach_port_t,
    pub our_send: mach_port_t,
    pub their_receive: mach_port_t,
}

impl ChannelPair {
    pub fn new() -> Self {
        let our_receive = msg::allocate_receive();
        let their_receive = msg::allocate_receive();
        // A send right to their port, under the same name while we hold
        // the receive right; ours to keep once that moves away.
        msg::insert_send(their_receive);
        Self {
            our_receive,
            our_send: their_receive,
            their_receive,
        }
    }
}
