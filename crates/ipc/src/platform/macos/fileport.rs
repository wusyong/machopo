//! Open files as ports (Chromium's `ChannelMac`, which sends a descriptor
//! as a fileport): a Mach message carries rights to ports, not
//! descriptors, so a file goes as a port that stands for it, which the
//! receiving task makes a descriptor of its own from. The port holds the
//! open file itself, as a descriptor does: open as it was opened (to read,
//! for the files a channel sends), however its path changes, until the
//! last right to the port is let go.

use std::fs::File;
use std::os::fd::{AsRawFd, FromRawFd};

use mach2::port::{MACH_PORT_NULL, mach_port_t};

use super::port::SendRight;

// The kernel's (libsystem_kernel), private but stable: `<sys/fileport.h>`
// isn't in the SDK. Chromium's Mojo and XPC send descriptors by them.
unsafe extern "C" {
    fn fileport_makeport(fd: libc::c_int, port: *mut mach_port_t) -> libc::c_int;
    fn fileport_makefd(port: mach_port_t) -> libc::c_int;
}

/// A port that stands for `file`, as it is open here: a send right of
/// this task's, which a message may move to another. The port has the
/// open file of its own: `file` may be closed.
pub fn make_port(file: &File) -> Option<SendRight> {
    let mut name = MACH_PORT_NULL;
    // SAFETY: `file`'s descriptor is open for the call; the port's name
    // is written to a local alive for it.
    let result = unsafe { fileport_makeport(file.as_raw_fd(), &raw mut name) };
    if result != 0 || name == MACH_PORT_NULL {
        return None;
    }
    // SAFETY: the call succeeded: `name` is a new send right, this task's
    // alone.
    Some(unsafe { SendRight::from_raw(name) })
}

/// The file `port` stands for, as a descriptor of this task's own (closed
/// on exec, as the kernel makes it). `None` when the port isn't a
/// fileport: a region's memory entry, say.
pub fn make_file(port: &SendRight) -> Option<File> {
    // SAFETY: a call on a right this task holds, which makes a new
    // descriptor or none; the right stays the caller's.
    let fd = unsafe { fileport_makefd(port.name()) };
    if fd < 0 {
        return None;
    }
    // SAFETY: the call made `fd` for this call alone: nothing else owns
    // it.
    Some(unsafe { File::from_raw_fd(fd) })
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Seek, Write};

    use super::*;
    use crate::platform::macos::memory;

    /// A file of its own with `bytes` in it, removed at once: what is
    /// open of it stays.
    fn scratch(bytes: &[u8]) -> File {
        let path = std::env::temp_dir().join(format!(
            "machopo-ipc-fileport-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(&path, bytes).unwrap();
        let file = File::open(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        file
    }

    #[test]
    fn a_files_port_makes_a_file_open_as_it_was_and_closed_on_exec() {
        let file = scratch(b"fileport");
        let port = make_port(&file).unwrap();
        drop(file);
        let mut made = make_file(&port).unwrap();
        // The port goes on standing for it: another descriptor of the same.
        let mut again = make_file(&port).unwrap();
        drop(port);
        let mut bytes = String::new();
        made.read_to_string(&mut bytes).unwrap();
        assert_eq!(bytes, "fileport");
        // The same open file: its offset is shared.
        let mut rest = String::new();
        again.read_to_string(&mut rest).unwrap();
        assert_eq!(rest, "");
        again.rewind().unwrap();
        // Open to read, as it was: not to write.
        assert!(made.write_all(b"x").is_err());
        let flags = rustix::io::fcntl_getfd(&made).unwrap();
        assert!(flags.contains(rustix::io::FdFlags::CLOEXEC));
    }

    #[test]
    fn a_regions_handle_is_no_file_and_a_files_port_no_region() {
        let (_, region) = memory::create(16 << 10, false).unwrap();
        assert!(make_file(&region).is_none());
        let port = make_port(&scratch(b"not memory")).unwrap();
        assert!(memory::map(&port, 16 << 10, false).is_none());
    }
}
