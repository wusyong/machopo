//! Rights to Mach ports this task holds, each let go when dropped: a
//! handle that is forgotten is a port that never dies, and a peer that is
//! never told its channel is over.

use mach2::kern_return::{KERN_SUCCESS, kern_return_t};
use mach2::mach_port::{
    mach_port_allocate, mach_port_deallocate, mach_port_insert_right, mach_port_mod_refs,
};
use mach2::message::{MACH_MSG_TYPE_MAKE_SEND, MACH_MSG_TYPE_MAKE_SEND_ONCE, mach_msg_type_name_t};
use mach2::notify::{MACH_NOTIFY_DEAD_NAME, MACH_NOTIFY_NO_SENDERS, MACH_NOTIFY_SEND_POSSIBLE};
use mach2::port::{
    MACH_PORT_NULL, MACH_PORT_QLIMIT_LARGE, MACH_PORT_RIGHT_PORT_SET, MACH_PORT_RIGHT_RECEIVE,
    mach_port_t,
};
use mach2::traps::mach_task_self;

// What mach2 doesn't declare (`<mach/mach_port.h>`).
unsafe extern "C" {
    fn mach_port_request_notification(
        task: mach_port_t,
        name: mach_port_t,
        id: i32,
        sync: u32,
        notify: mach_port_t,
        notify_disposition: mach_msg_type_name_t,
        previous: *mut mach_port_t,
    ) -> kern_return_t;
    fn mach_port_insert_member(
        task: mach_port_t,
        name: mach_port_t,
        set: mach_port_t,
    ) -> kern_return_t;
    fn mach_port_set_attributes(
        task: mach_port_t,
        name: mach_port_t,
        flavor: i32,
        info: *const u32,
        count: u32,
    ) -> kern_return_t;
}

/// `MACH_PORT_LIMITS_INFO` (`<mach/port.h>`): a port's queue limit, one
/// word.
const MACH_PORT_LIMITS_INFO: i32 = 1;

/// Asks the kernel for a notification about `name`, sent to `notify` as
/// a message whose id is `id`. What an earlier request for the same left
/// is let go.
fn request(name: mach_port_t, id: i32, sync: u32, notify: &ReceiveRight) -> bool {
    let mut previous = MACH_PORT_NULL;
    // SAFETY: a call on names this task holds, the notification going to
    // a send-once right the kernel makes from `notify`; `previous` is
    // alive for the call.
    let result = unsafe {
        mach_port_request_notification(
            mach_task_self(),
            name,
            id,
            sync,
            notify.name(),
            MACH_MSG_TYPE_MAKE_SEND_ONCE,
            &raw mut previous,
        )
    };
    if previous != MACH_PORT_NULL {
        // SAFETY: the right an earlier request left, handed back to this
        // task to let go.
        unsafe { mach_port_deallocate(mach_task_self(), previous) };
    }
    result == KERN_SUCCESS
}

/// A send right: what a message is sent to, and what a region's handle is.
#[derive(Debug)]
pub struct SendRight(mach_port_t);

impl SendRight {
    /// Takes over a send right.
    ///
    /// # Safety
    ///
    /// `name` is a send right in this task that nothing else will let go
    /// of.
    pub const unsafe fn from_raw(name: mach_port_t) -> Self {
        Self(name)
    }

    /// The right's name in this task.
    pub const fn name(&self) -> mach_port_t {
        self.0
    }

    /// The right, no longer let go by this value: for a message that
    /// moves it to the peer.
    pub fn into_raw(self) -> mach_port_t {
        let name = self.0;
        std::mem::forget(self);
        name
    }

    /// Asks the kernel to tell `notify` (`MACH_NOTIFY_DEAD_NAME`) when
    /// the port this is a right to dies. `false` when it is dead
    /// already: there is nothing to wait for. The notification comes
    /// with one more reference to the name, which
    /// [`SendRight::dead_name_told`] lets go.
    pub fn tell_when_dead(&self, notify: &ReceiveRight) -> bool {
        request(self.0, MACH_NOTIFY_DEAD_NAME, 1, notify)
    }

    /// Lets go of the reference a dead-name notification about this
    /// right came with.
    pub fn dead_name_told(&self) {
        // SAFETY: the reference the notification gave this task for the
        // name, let go once; the value's own is left.
        unsafe { mach_port_deallocate(mach_task_self(), self.0) };
    }

    /// Asks the kernel to tell `notify` (`MACH_NOTIFY_SEND_POSSIBLE`)
    /// once the port has room for a message: at once, if it has. For
    /// after a send that found its queue full: room that came between
    /// the send and the asking is told of too.
    pub fn tell_when_sendable(&self, notify: &ReceiveRight) -> bool {
        request(self.0, MACH_NOTIFY_SEND_POSSIBLE, 1, notify)
    }
}

impl Drop for SendRight {
    fn drop(&mut self) {
        // SAFETY: the name is a right this value owns, let go once.
        unsafe { mach_port_deallocate(mach_task_self(), self.0) };
    }
}

/// A send-once right: what one reply is sent to. Dropped unused, the
/// kernel tells the port's receiver that no reply is coming.
#[derive(Debug)]
pub struct SendOnceRight(mach_port_t);

impl SendOnceRight {
    /// Takes over a send-once right.
    ///
    /// # Safety
    ///
    /// `name` is a send-once right in this task that nothing else will
    /// use or let go of.
    pub const unsafe fn from_raw(name: mach_port_t) -> Self {
        Self(name)
    }

    /// The right, no longer let go by this value: for the message that
    /// uses it up.
    pub fn into_raw(self) -> mach_port_t {
        let name = self.0;
        std::mem::forget(self);
        name
    }
}

impl Drop for SendOnceRight {
    fn drop(&mut self) {
        // SAFETY: the name is a right this value owns, let go once.
        unsafe { mach_port_deallocate(mach_task_self(), self.0) };
    }
}

/// A receive right: a port, as the one that reads it. Dropped, the port
/// dies: send rights to it become dead names.
#[derive(Debug)]
pub struct ReceiveRight(mach_port_t);

impl ReceiveRight {
    /// A new port.
    pub fn new() -> Option<Self> {
        let mut name = MACH_PORT_NULL;
        // SAFETY: the call writes one name, to a variable alive for it.
        let result =
            unsafe { mach_port_allocate(mach_task_self(), MACH_PORT_RIGHT_RECEIVE, &raw mut name) };
        (result == KERN_SUCCESS).then_some(Self(name))
    }

    /// Takes over a receive right.
    ///
    /// # Safety
    ///
    /// `name` is a receive right in this task that nothing else will let
    /// go of.
    pub const unsafe fn from_raw(name: mach_port_t) -> Self {
        Self(name)
    }

    /// The right's name in this task.
    pub const fn name(&self) -> mach_port_t {
        self.0
    }

    /// The right, no longer let go by this value: for a message that
    /// moves it to the peer.
    pub fn into_raw(self) -> mach_port_t {
        let name = self.0;
        std::mem::forget(self);
        name
    }

    /// A send right to this port.
    pub fn make_send(&self) -> Option<SendRight> {
        // SAFETY: a call on this task's own receive right; it adds one
        // send right under the same name, which the value returned owns.
        let result = unsafe {
            mach_port_insert_right(mach_task_self(), self.0, self.0, MACH_MSG_TYPE_MAKE_SEND)
        };
        (result == KERN_SUCCESS).then_some(SendRight(self.0))
    }

    /// Asks the kernel to send this port a message (`MACH_NOTIFY_NO_SENDERS`)
    /// once no send right to it is left and at least `made` were ever
    /// made: at once, if that is so already. `made` 0 tells of a port
    /// nobody was given a right to yet; 1 waits for one to have been.
    pub fn tell_when_no_senders(&self, made: u32) -> bool {
        request(self.0, MACH_NOTIFY_NO_SENDERS, made, self)
    }

    /// Lets as many messages wait in this port as a port can hold (1024,
    /// where a new one holds 5).
    pub fn raise_queue_limit(&self) -> bool {
        let limit = MACH_PORT_QLIMIT_LARGE;
        // SAFETY: a call on this task's own receive right, reading one
        // word alive for the call.
        let result = unsafe {
            mach_port_set_attributes(
                mach_task_self(),
                self.0,
                MACH_PORT_LIMITS_INFO,
                &raw const limit,
                1,
            )
        };
        result == KERN_SUCCESS
    }
}

impl Drop for ReceiveRight {
    fn drop(&mut self) {
        // SAFETY: the name is a receive right this value owns, let go
        // once.
        unsafe { mach_port_mod_refs(mach_task_self(), self.0, MACH_PORT_RIGHT_RECEIVE, -1) };
    }
}

/// A right a message carries from one task to another.
#[derive(Debug)]
pub enum Right {
    Send(SendRight),
    Receive(ReceiveRight),
}

/// A set of ports received on as one: a message to any of them comes out
/// of a receive on the set.
#[derive(Debug)]
pub struct PortSet(mach_port_t);

impl PortSet {
    pub fn new() -> Option<Self> {
        let mut name = MACH_PORT_NULL;
        // SAFETY: the call writes one name, to a variable alive for it.
        let result = unsafe {
            mach_port_allocate(mach_task_self(), MACH_PORT_RIGHT_PORT_SET, &raw mut name)
        };
        (result == KERN_SUCCESS).then_some(Self(name))
    }

    /// The set's name in this task: what to receive on.
    pub const fn name(&self) -> mach_port_t {
        self.0
    }

    /// Adds `port` to the set. It leaves when it dies.
    pub fn add(&self, port: &ReceiveRight) -> bool {
        // SAFETY: a call on this task's own receive right and set.
        let result = unsafe { mach_port_insert_member(mach_task_self(), port.name(), self.0) };
        result == KERN_SUCCESS
    }
}

impl Drop for PortSet {
    fn drop(&mut self) {
        // SAFETY: the name is a set this value owns, let go once; its
        // members stay their owners'.
        unsafe { mach_port_mod_refs(mach_task_self(), self.0, MACH_PORT_RIGHT_PORT_SET, -1) };
    }
}
