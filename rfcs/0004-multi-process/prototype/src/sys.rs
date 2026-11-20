//! What mach2 0.6.0 doesn't declare, and the two sandbox calls.

use std::ffi::{c_char, c_int};

use mach2::kern_return::kern_return_t;
use mach2::message::{audit_token_t, mach_msg_type_name_t};
use mach2::port::{mach_port_name_t, mach_port_t};

pub const MAP_MEM_NAMED_CREATE: c_int = 0x02_0000;
pub const MACH_PORT_LIMITS_INFO: c_int = 1;
pub const MACH_PORT_QLIMIT_LARGE: u32 = 1024;
/// `MACH_RCV_TRAILER_TYPE(MACH_MSG_TRAILER_FORMAT_0) |
/// MACH_RCV_TRAILER_ELEMENTS(MACH_RCV_TRAILER_AUDIT)`.
pub const RCV_AUDIT_TRAILER: c_int = 3 << 24;
/// The size of `mach_msg_audit_trailer_t`.
pub const AUDIT_TRAILER_SIZE: usize = 52;

unsafe extern "C" {
    pub fn mach_port_request_notification(
        task: mach_port_t,
        name: mach_port_name_t,
        msgid: c_int,
        sync: u32,
        notify: mach_port_t,
        notify_poly: mach_msg_type_name_t,
        previous: *mut mach_port_t,
    ) -> kern_return_t;
    pub fn mach_port_set_attributes(
        task: mach_port_t,
        name: mach_port_name_t,
        flavor: c_int,
        info: *const u32,
        count: u32,
    ) -> kern_return_t;
    pub fn sandbox_init(profile: *const c_char, flags: u64, errorbuf: *mut *mut c_char) -> c_int;
    pub fn sandbox_free_error(errorbuf: *mut c_char);
}

#[link(name = "bsm")]
unsafe extern "C" {
    pub fn audit_token_to_pid(token: audit_token_t) -> libc::pid_t;
}
