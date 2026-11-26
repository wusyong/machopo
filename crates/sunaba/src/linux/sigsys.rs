//! What happens to a system call the policy doesn't name: seccomp sends
//! the thread `SIGSYS` in place of making it (`SECCOMP_RET_TRAP`), and the
//! handler here writes which call it was to stderr and ends the process
//! (Chromium's `CrashSIGSYS`). The sandbox doesn't rest on the handler:
//! the call is already not made, and a process that swaps the handler for
//! one that returns gets a system call that did nothing.

use std::ffi::{c_int, c_uint, c_void};
use std::io;

use super::names;

/// `siginfo_t` as the kernel fills it for `SIGSYS`, on the 64-bit
/// architectures (`struct siginfo`'s `_sigsys`): `libc` has no reader for
/// these fields.
#[repr(C)]
struct SigsysInfo {
    signal: c_int,
    errno: c_int,
    code: c_int,
    call_address: *mut c_void,
    system_call: c_int,
    arch: c_uint,
}

/// `si_code` when seccomp sent the signal.
const SYS_SECCOMP: c_int = 1;

/// A system call number no policy names (and no kernel has): made from the
/// handler, where `SIGSYS` is blocked, seccomp traps it, and the kernel
/// ends a process it can't deliver a trap to, as killed by `SIGSYS`.
const NO_SUCH_CALL: libc::c_long = 0x0fff_ffff;

pub(super) fn install() -> io::Result<()> {
    // SAFETY: zeroed is a valid `sigaction` (no flags, empty mask, no
    // restorer: libc's `sigaction` sets one).
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    let handler: extern "C" fn(c_int, *mut libc::siginfo_t, *mut c_void) = on_sigsys;
    action.sa_sigaction = handler as usize;
    // Not `SA_NODEFER`: the handler ends the process by a trap that can't
    // be delivered. `SA_ONSTACK`: the refused call may be the one a thread
    // makes when its stack has run out.
    action.sa_flags = libc::SA_SIGINFO | libc::SA_ONSTACK;
    // SAFETY: `action` is a whole `sigaction` with a handler of the
    // `SA_SIGINFO` signature that only makes async-signal-safe calls; the
    // old action isn't asked for.
    if unsafe { libc::sigaction(libc::SIGSYS, &action, std::ptr::null_mut()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Async-signal-safe: no allocation, no lock; `write` and the call that
/// ends the process are all it asks of the kernel.
extern "C" fn on_sigsys(_signal: c_int, info: *mut libc::siginfo_t, _context: *mut c_void) {
    // SAFETY: the kernel hands an `SA_SIGINFO` handler a valid `siginfo_t`
    // which, for `SIGSYS`, has `SigsysInfo`'s fields (the layout is the
    // kernel's ABI on 64-bit Linux); only plain integers are read.
    let (code, call) = unsafe {
        let info = info.cast::<SigsysInfo>();
        ((*info).code, (*info).system_call)
    };
    let mut message = Message::new();
    if code == SYS_SECCOMP {
        message.text(b"sunaba: system call ");
        message.number(call);
        if let Some(name) = names::name(i64::from(call)) {
            message.text(b" (");
            message.text(name.as_bytes());
            message.text(b")");
        }
        message.text(b" isn't allowed in this sandbox\n");
    } else {
        message.text(b"sunaba: SIGSYS that seccomp didn't send\n");
    }
    // SAFETY: `write` is async-signal-safe and reads `length` bytes of
    // `message.bytes`, which is that long or longer. That stderr may be
    // closed, or `write` refused, changes nothing below.
    unsafe { libc::write(2, message.bytes.as_ptr().cast(), message.length) };
    // SAFETY: a system call with no arguments that no filter allows:
    // seccomp traps it, `SIGSYS` is blocked here, so the kernel ends the
    // process. If it comes back (no filter after all), `abort` does.
    unsafe {
        libc::syscall(NO_SUCH_CALL);
        libc::abort();
    }
}

/// The handler's line, built in place.
struct Message {
    bytes: [u8; 96],
    length: usize,
}

impl Message {
    fn new() -> Self {
        Self {
            bytes: [0; 96],
            length: 0,
        }
    }

    /// What fits of `text`.
    fn text(&mut self, text: &[u8]) {
        for byte in text {
            let Some(slot) = self.bytes.get_mut(self.length) else {
                return;
            };
            *slot = *byte;
            self.length = self.length.saturating_add(1);
        }
    }

    fn number(&mut self, number: c_int) {
        if number < 0 {
            self.text(b"-");
        }
        let mut rest = number.unsigned_abs();
        let mut digits = [0u8; 10];
        let mut count = 0usize;
        loop {
            // A remainder of 10 is under 10.
            let digit = (rest % 10) as u8;
            if let Some(slot) = digits.get_mut(count) {
                *slot = b'0'.saturating_add(digit);
            }
            count = count.saturating_add(1);
            rest /= 10;
            if rest == 0 {
                break;
            }
        }
        for digit in digits.iter().take(count).rev() {
            self.text(&[*digit]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(build: impl FnOnce(&mut Message)) -> String {
        let mut message = Message::new();
        build(&mut message);
        String::from_utf8(message.bytes[..message.length].to_vec()).unwrap()
    }

    #[test]
    fn a_number_is_written_in_decimal() {
        assert_eq!(line(|m| m.number(0)), "0");
        assert_eq!(line(|m| m.number(41)), "41");
        assert_eq!(line(|m| m.number(c_int::MAX)), "2147483647");
        assert_eq!(line(|m| m.number(c_int::MIN)), "-2147483648");
    }

    #[test]
    fn a_line_too_long_is_cut_not_overrun() {
        let long = line(|m| {
            for _ in 0..20 {
                m.text(b"0123456789");
            }
        });
        assert_eq!(long.len(), 96);
    }
}
