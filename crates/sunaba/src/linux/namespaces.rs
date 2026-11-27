//! A child's own user and network namespaces (Chromium's
//! `NamespaceSandbox`, without its PID namespace), made by whoever starts
//! the child, between its fork and its exec: the child has no network
//! interface but loopback, and whatever it may do as root it may only
//! within namespaces of its own.
//!
//! Its user and group ids map to themselves: the child is the same user
//! inside as out, not root, so the exec takes every capability the new
//! user namespace gave it (the kernel recomputes them for a user that
//! isn't root, from a program that has none of its own); all but
//! `CAP_SYS_CHROOT`, where it is kept for the child's own chroot
//! ([`Namespaces::keeping_chroot`]), which [`enter`](super::enter) makes
//! and then drops every capability.
//!
//! Unprivileged user namespaces aren't everywhere: a kernel may be built
//! or set without them, and Ubuntu (since 23.10) lets only programs its
//! AppArmor profiles name make them. [`Namespaces::available`] tries.

use std::ffi::CStr;
use std::io;

/// What a child's namespaces are made with: its maps, written before the
/// fork, so that what runs between the fork and the exec allocates
/// nothing.
#[derive(Clone, Debug)]
pub struct Namespaces {
    uid_map: Vec<u8>,
    gid_map: Vec<u8>,
    keeps_chroot: bool,
}

/// `CAP_SYS_CHROOT`'s number (`<linux/capability.h>`).
pub(super) const CAP_SYS_CHROOT: u32 = 18;

/// `capget` and `capset`'s header, for the version of 64-bit sets
/// (`_LINUX_CAPABILITY_VERSION_3`).
#[repr(C)]
pub(super) struct CapabilityHeader {
    pub version: u32,
    pub pid: libc::c_int,
}

/// One half (32 capabilities) of each of a thread's three sets.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct CapabilityData {
    pub effective: u32,
    pub permitted: u32,
    pub inheritable: u32,
}

pub(super) const CAPABILITY_VERSION_3: u32 = 0x2008_0522;

/// The calling thread's sets, or `None` when the kernel won't say.
pub(super) fn capabilities() -> Option<[CapabilityData; 2]> {
    let mut header = CapabilityHeader {
        version: CAPABILITY_VERSION_3,
        pid: 0,
    };
    let mut data = [CapabilityData::default(); 2];
    // SAFETY: the header and two halves, as the version says the kernel
    // writes, locals alive for the call.
    let got = unsafe { libc::syscall(libc::SYS_capget, &raw mut header, data.as_mut_ptr()) };
    (got == 0).then_some(data)
}

/// Sets the calling thread's sets to `data`.
pub(super) fn set_capabilities(data: &[CapabilityData; 2]) -> io::Result<()> {
    let mut header = CapabilityHeader {
        version: CAPABILITY_VERSION_3,
        pid: 0,
    };
    // SAFETY: the header and two halves, as the version says the kernel
    // reads, alive for the call.
    if unsafe { libc::syscall(libc::SYS_capset, &raw mut header, data.as_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

impl Default for Namespaces {
    fn default() -> Self {
        Self::new()
    }
}

impl Namespaces {
    /// For a child of this process, with this process's user and group.
    pub fn new() -> Self {
        // SAFETY: neither call can fail or touches memory of the caller's.
        let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
        Self {
            uid_map: format!("{uid} {uid} 1\n").into_bytes(),
            gid_map: format!("{gid} {gid} 1\n").into_bytes(),
            keeps_chroot: false,
        }
    }

    /// The child keeps `CAP_SYS_CHROOT` alone through its exec (as an
    /// ambient capability), for a policy's chroot: it may chroot within
    /// its own namespaces until [`enter`](super::enter) drops it with
    /// every other.
    pub fn keeping_chroot(mut self) -> Self {
        self.keeps_chroot = true;
        self
    }

    /// Moves the calling process into a new user namespace and a new
    /// network namespace, its ids mapped to themselves. For a child
    /// between fork and exec: it makes system calls on what `self` holds
    /// and nothing else (no allocation, no lock), as is all that may be
    /// done there. The kernel refuses a process of more than one thread.
    pub fn enter(&self) -> io::Result<()> {
        // SAFETY: a system call that takes flags alone.
        if unsafe { libc::unshare(libc::CLONE_NEWUSER | libc::CLONE_NEWNET) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // A process that isn't privileged where it came from maps its
        // group only once it has given up setting supplementary ones.
        write_all(c"/proc/self/setgroups", b"deny")?;
        write_all(c"/proc/self/uid_map", &self.uid_map)?;
        write_all(c"/proc/self/gid_map", &self.gid_map)?;
        if self.keeps_chroot {
            // Inheritable and then ambient: the one set a program with no
            // capabilities of its own keeps through an exec, for a user
            // that isn't root.
            let mut sets = capabilities().ok_or_else(io::Error::last_os_error)?;
            sets[0].inheritable = 1 << CAP_SYS_CHROOT;
            sets[1].inheritable = 0;
            set_capabilities(&sets)?;
            let raise = libc::c_ulong::from(CAP_SYS_CHROOT);
            // SAFETY: integers alone.
            let raised = unsafe {
                libc::prctl(
                    libc::PR_CAP_AMBIENT,
                    libc::PR_CAP_AMBIENT_RAISE as libc::c_ulong,
                    raise,
                    0 as libc::c_ulong,
                    0 as libc::c_ulong,
                )
            };
            if raised != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }

    /// Whether a child of this process can be given its namespaces here:
    /// tried in a child of its own, which ends at once (this process
    /// touches no namespace).
    pub fn available(&self) -> bool {
        // SAFETY: the child makes only system calls (`enter`'s, and
        // `_exit`) before it ends: what is safe in a child of a process of
        // many threads, whose locks it may have been forked holding.
        let pid = unsafe { libc::fork() };
        if pid == 0 {
            let code = if self.enter().is_ok() { 0 } else { 1 };
            // SAFETY: ends the child at once, running nothing of this
            // process's (no destructor, no exit handler).
            unsafe { libc::_exit(code) };
        }
        if pid < 0 {
            return false;
        }
        let mut status = 0;
        loop {
            // SAFETY: waits for the child just made, into a local.
            let waited = unsafe { libc::waitpid(pid, &raw mut status, 0) };
            if waited == pid {
                break;
            }
            if waited < 0 && io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
                return false;
            }
        }
        libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0
    }
}

/// Writes `bytes` to the file at `path` in one `write`, as the kernel's
/// files under `/proc/self` take them.
fn write_all(path: &CStr, bytes: &[u8]) -> io::Result<()> {
    // SAFETY: `path` is a C string alive for the call.
    let fd = unsafe { libc::open(path.as_ptr(), libc::O_WRONLY | libc::O_CLOEXEC) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `fd` was just opened; `bytes` is alive for the call and is
    // read as far as its length.
    let written = unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) };
    let error = io::Error::last_os_error();
    // SAFETY: `fd` is this function's, closed once.
    unsafe { libc::close(fd) };
    match usize::try_from(written) {
        Ok(written) if written == bytes.len() => Ok(()),
        Ok(_) => Err(io::Error::from(io::ErrorKind::WriteZero)),
        Err(_) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_childs_maps_are_its_own_ids() {
        let namespaces = Namespaces::new();
        // SAFETY: neither call can fail.
        let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
        assert_eq!(namespaces.uid_map, format!("{uid} {uid} 1\n").into_bytes());
        assert_eq!(namespaces.gid_map, format!("{gid} {gid} 1\n").into_bytes());
    }

    #[test]
    fn a_process_of_many_threads_is_refused() {
        // The test harness's: this one and the others'.
        let other =
            std::thread::spawn(|| std::thread::park_timeout(std::time::Duration::from_secs(1)));
        assert!(Namespaces::new().enter().is_err());
        other.thread().unpark();
        other.join().unwrap();
    }
}
