//! Shared memory by sealed memfds (Chromium's
//! `platform_shared_memory_region_posix.cc`). A region is made once, at
//! the length it keeps: its descriptor is sealed against shrinking and
//! growing, so a peer that holds one can't pull memory from under
//! whoever mapped it (touching a mapping past a file's end isn't an
//! error, it is the process killed).
//!
//! Two kinds are made, the two a channel uses (Chromium's
//! `ReadOnlySharedMemoryRegion` and `UnsafeSharedMemoryRegion`): one its
//! maker writes and the peer can only read, and one the peer writes. The
//! first is sealed against writing from then on, once its maker has
//! mapped it: the seal is the file's, so no descriptor for it, however
//! it was come by, maps it writable or writes it. (Chromium hands out a
//! descriptor opened to read, which `/proc` opens again to write, and
//! has its sandbox keep a renderer from `/proc`.)
//!
//! Where the kernel knows how, a region can't be run either
//! (`MFD_NOEXEC_SEAL`): a peer that may write it could otherwise write a
//! program there and execute it.

use std::io;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd};

use memmap2::{MmapOptions, MmapRaw};
use rustix::fs::{MemfdFlags, SealFlags, fcntl_add_seals, fcntl_get_seals, fstat, ftruncate};
use rustix::io::Errno;

use crate::core::region::Memory;

/// A mapping of a region in this process, unmapped when dropped.
#[derive(Debug)]
pub struct Mapping(MmapRaw);

// SAFETY: the range is mapped from `map` to `drop` and never moves; it is
// readable, and writable when it was mapped so, which whoever puts it in
// a pool did (`create` with `peer_writes` false, `map` with `writable`).
// It stays within its file: the file is sealed against shrinking, which
// `map` sees to and `create` does.
unsafe impl Memory for Mapping {
    fn ptr(&self) -> *mut u8 {
        self.0.as_mut_ptr()
    }

    fn len(&self) -> usize {
        self.0.len()
    }
}

/// Maps the first `len` bytes of the region `handle` is, to write when
/// `writable`. `None` when it isn't a region that long (a file that could
/// still be made shorter is no region: its mapping could be left past the
/// end), or is one that may only be read and `writable` asks for more.
pub fn map(handle: BorrowedFd<'_>, len: usize, writable: bool) -> Option<Mapping> {
    if len == 0 {
        return None;
    }
    // Sealed first, measured after: the length read can't go down.
    let seals = fcntl_get_seals(handle).ok()?;
    if !seals.contains(SealFlags::SHRINK) {
        return None;
    }
    let size = u64::try_from(fstat(handle).ok()?.st_size).ok()?;
    if u64::try_from(len).ok()? > size {
        return None;
    }
    let mut options = MmapOptions::new();
    options.len(len);
    let mapped = if writable {
        options.map_raw(handle.as_raw_fd())
    } else {
        options.map_raw_read_only(handle.as_raw_fd())
    };
    mapped.ok().map(Mapping)
}

/// A new memfd that can be sealed: one that can't be run where the
/// kernel knows the flag, and a plain one where it doesn't (Linux before
/// 6.3 refuses the flag as an invalid argument). Not remembered: regions
/// are made seldom, and the second call is all an old kernel costs.
fn memfd(make: impl Fn(MemfdFlags) -> io::Result<OwnedFd>) -> io::Result<OwnedFd> {
    let flags = MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING;
    match make(flags | MemfdFlags::NOEXEC_SEAL) {
        Err(error) if error.raw_os_error() == Some(Errno::INVAL.raw_os_error()) => make(flags),
        made => made,
    }
}

/// Makes a region of `len` bytes: this process's mapping of it, and the
/// handle that gives a peer its own. With `peer_writes` the peer may
/// write it and this process's mapping is to read; without, this
/// process's mapping is to write and the handle, from here on, lets
/// nobody map it to write.
pub fn create(len: usize, peer_writes: bool) -> Option<(Mapping, OwnedFd)> {
    let fd = memfd(|flags| Ok(rustix::fs::memfd_create("machopo-region", flags)?)).ok()?;
    ftruncate(&fd, u64::try_from(len).ok()?).ok()?;
    fcntl_add_seals(&fd, SealFlags::SHRINK | SealFlags::GROW).ok()?;
    // This process's mapping before the seal against writing: the seal
    // leaves the mappings there are, and refuses new ones.
    let mapping = map(fd.as_fd(), len, !peer_writes)?;
    let last = if peer_writes {
        SealFlags::SEAL
    } else {
        SealFlags::FUTURE_WRITE | SealFlags::SEAL
    };
    fcntl_add_seals(&fd, last).ok()?;
    Some((mapping, fd))
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use rustix::fs::{Mode, OFlags, fchmod, memfd_create, open};
    use rustix::mm::{MprotectFlags, mprotect};

    use super::*;

    const LEN: usize = 64 << 10;

    fn write(mapping: &Mapping, at: usize, byte: u8) {
        assert!(at < mapping.len());
        // SAFETY: inside the mapping, which the test mapped to write;
        // nothing else touches it during the write.
        unsafe { mapping.ptr().add(at).write(byte) };
    }

    fn read(mapping: &Mapping, at: usize) -> u8 {
        assert!(at < mapping.len());
        // SAFETY: inside the mapping, which is readable.
        unsafe { mapping.ptr().add(at).read() }
    }

    /// The peer's end of a handle: this process again.
    fn peer(handle: &OwnedFd, writable: bool) -> Option<Mapping> {
        map(handle.as_fd(), LEN, writable)
    }

    /// The same file opened anew to read and write, as whoever holds a
    /// descriptor for it can through `/proc`.
    fn reopened(handle: &OwnedFd) -> io::Result<OwnedFd> {
        let path = format!("/proc/self/fd/{}", handle.as_raw_fd());
        Ok(open(path, OFlags::RDWR | OFlags::CLOEXEC, Mode::empty())?)
    }

    #[test]
    fn what_the_maker_writes_the_peers_handle_reads() {
        let (mine, handle) = create(LEN, false).unwrap();
        assert_eq!(mine.len(), LEN);
        assert_eq!(read(&mine, 0), 0, "new memory is zeroes");
        assert_eq!(read(&mine, LEN - 1), 0);
        write(&mine, 0, 0x11);
        let theirs = peer(&handle, false).unwrap();
        assert_ne!(mine.ptr(), theirs.ptr());
        assert_eq!(read(&theirs, 0), 0x11);
        // Written after the handle was made and after it was mapped: the
        // same memory, not a copy of what it held then.
        write(&mine, LEN - 1, 0x22);
        write(&mine, 0, 0x33);
        assert_eq!(read(&theirs, LEN - 1), 0x22);
        assert_eq!(read(&theirs, 0), 0x33);
    }

    #[test]
    fn a_handle_to_read_cant_be_mapped_to_write() {
        let (_mine, handle) = create(LEN, false).unwrap();
        assert!(peer(&handle, true).is_none());
        assert!(peer(&handle, false).is_some());
    }

    #[test]
    fn a_handle_to_read_cant_be_written_however_it_is_held() {
        let (mine, handle) = create(LEN, false).unwrap();
        assert_eq!(rustix::io::write(&handle, b"x"), Err(Errno::PERM));
        // Opened anew to write: the seal is the file's, not the
        // descriptor's.
        if let Ok(again) = reopened(&handle) {
            assert_eq!(rustix::io::write(&again, b"x"), Err(Errno::PERM));
            assert!(map(again.as_fd(), LEN, true).is_none());
        }
        assert_eq!(read(&mine, 0), 0);
    }

    fn can_be_made_writable(mapping: &Mapping) -> bool {
        // SAFETY: the mapping's own range; a mapping made writable is
        // one the test fails on, and nothing is written.
        unsafe {
            mprotect(
                mapping.ptr().cast(),
                mapping.len(),
                MprotectFlags::READ | MprotectFlags::WRITE,
            )
        }
        .is_ok()
    }

    #[test]
    fn the_peers_mapping_to_read_stays_one() {
        let (_mine, handle) = create(LEN, false).unwrap();
        assert!(!can_be_made_writable(&peer(&handle, false).unwrap()));
    }

    /// How the kernel has `mapping`: `r--s`, `rw-s`.
    fn protection(mapping: &Mapping) -> String {
        let start = format!("{:x}-", mapping.ptr().addr());
        let maps = std::fs::read_to_string("/proc/self/maps").unwrap();
        let line = maps.lines().find(|line| line.starts_with(&start)).unwrap();
        line.split(' ').nth(1).unwrap().to_owned()
    }

    #[test]
    fn each_end_maps_to_write_only_what_it_writes() {
        let (mine, handle) = create(LEN, false).unwrap();
        assert_eq!(protection(&mine), "rw-s");
        assert_eq!(protection(&peer(&handle, false).unwrap()), "r--s");
        let (mine, handle) = create(LEN, true).unwrap();
        assert_eq!(protection(&mine), "r--s");
        assert_eq!(protection(&peer(&handle, true).unwrap()), "rw-s");
        assert_eq!(protection(&peer(&handle, false).unwrap()), "r--s");
    }

    #[test]
    fn what_the_peer_writes_the_maker_reads() {
        let (mine, handle) = create(LEN, true).unwrap();
        let theirs = peer(&handle, true).unwrap();
        write(&theirs, 5, 0x44);
        assert_eq!(read(&mine, 5), 0x44);
        write(&theirs, LEN - 1, 0x55);
        assert_eq!(read(&mine, LEN - 1), 0x55);
    }

    #[test]
    fn a_region_keeps_its_length_and_its_seals() {
        for peer_writes in [false, true] {
            let (_mine, handle) = create(LEN, peer_writes).unwrap();
            assert!(ftruncate(&handle, 0).is_err(), "shrunk");
            assert!(ftruncate(&handle, LEN as u64 / 2).is_err(), "shrunk");
            assert!(ftruncate(&handle, LEN as u64 * 2).is_err(), "grown");
            assert_eq!(fstat(&handle).unwrap().st_size, LEN as i64);
            // No seal is added after: one against writing would end
            // the peer's writing.
            for seal in [SealFlags::WRITE, SealFlags::FUTURE_WRITE] {
                assert_eq!(fcntl_add_seals(&handle, seal), Err(Errno::PERM));
            }
            let seals = fcntl_get_seals(&handle).unwrap();
            assert!(seals.contains(SealFlags::SHRINK | SealFlags::GROW | SealFlags::SEAL));
            assert_eq!(seals.contains(SealFlags::FUTURE_WRITE), !peer_writes);
            assert!(!seals.contains(SealFlags::WRITE));
        }
    }

    /// Whether this kernel has memfds that can't be run (Linux 6.3).
    fn kernel_seals_exec() -> bool {
        memfd_create("probe", MemfdFlags::CLOEXEC | MemfdFlags::NOEXEC_SEAL).is_ok()
    }

    #[test]
    fn a_region_cant_be_run_where_the_kernel_can_say_so() {
        if !kernel_seals_exec() {
            return;
        }
        for peer_writes in [false, true] {
            let (_mine, handle) = create(LEN, peer_writes).unwrap();
            assert!(fcntl_get_seals(&handle).unwrap().contains(SealFlags::EXEC));
            assert_eq!(
                fchmod(&handle, Mode::from_raw_mode(0o777)),
                Err(Errno::PERM)
            );
            assert_eq!(
                fstat(&handle).unwrap().st_mode & 0o111,
                0,
                "nobody may execute it"
            );
        }
    }

    #[test]
    fn a_kernel_that_doesnt_know_the_flag_is_asked_again_without_it() {
        let asked = RefCell::new(Vec::new());
        let old_kernel = |flags: MemfdFlags| {
            asked.borrow_mut().push(flags);
            if flags.contains(MemfdFlags::NOEXEC_SEAL) {
                return Err(io::Error::from_raw_os_error(Errno::INVAL.raw_os_error()));
            }
            Ok(memfd_create("old", flags)?)
        };
        let fd = memfd(old_kernel).unwrap();
        let plain = MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING;
        assert_eq!(*asked.borrow(), [plain | MemfdFlags::NOEXEC_SEAL, plain]);
        // What it gave can be sealed.
        fcntl_add_seals(&fd, SealFlags::SHRINK).unwrap();

        // Any other refusal is the answer: nothing is asked twice.
        let asked = RefCell::new(0);
        let refuses = |_| {
            *asked.borrow_mut() += 1;
            Err(io::Error::from_raw_os_error(Errno::MFILE.raw_os_error()))
        };
        assert!(memfd(refuses).is_err());
        assert_eq!(*asked.borrow(), 1);

        // And a kernel that knows the flag is asked once, with it.
        let asked = RefCell::new(Vec::new());
        let new_kernel = |flags: MemfdFlags| {
            asked.borrow_mut().push(flags);
            Ok(memfd_create("new", flags & !MemfdFlags::NOEXEC_SEAL)?)
        };
        memfd(new_kernel).unwrap();
        assert_eq!(*asked.borrow(), [plain | MemfdFlags::NOEXEC_SEAL]);
    }

    #[test]
    fn a_region_keeps_its_memory_while_anything_maps_it() {
        let (mine, handle) = create(LEN, false).unwrap();
        let theirs = peer(&handle, false).unwrap();
        drop(handle);
        write(&mine, 9, 0x66);
        assert_eq!(read(&theirs, 9), 0x66);
        drop(mine);
        assert_eq!(read(&theirs, 9), 0x66);
    }

    #[test]
    fn no_more_than_a_region_has_is_mapped() {
        let (_mine, handle) = create(LEN, true).unwrap();
        for writable in [false, true] {
            assert!(map(handle.as_fd(), LEN + 1, writable).is_none());
            assert!(map(handle.as_fd(), LEN * 2, writable).is_none());
            assert_eq!(
                map(handle.as_fd(), LEN / 2, writable).unwrap().len(),
                LEN / 2
            );
            assert!(map(handle.as_fd(), 0, writable).is_none());
        }
        assert!(create(0, false).is_none());
        assert!(create(0, true).is_none());
    }

    #[test]
    fn what_isnt_a_region_isnt_mapped() {
        // A file long enough that could still be made shorter.
        let flags = MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING;
        let unsealed = memfd_create("unsealed", flags).unwrap();
        ftruncate(&unsealed, LEN as u64).unwrap();
        assert!(map(unsealed.as_fd(), LEN, false).is_none());
        // Sealed, it is one.
        fcntl_add_seals(&unsealed, SealFlags::SHRINK).unwrap();
        assert!(map(unsealed.as_fd(), LEN, false).is_some());
        // What has no seals at all, and what isn't a file.
        let null = std::fs::File::open("/dev/null").unwrap();
        assert!(map(null.as_fd(), LEN, false).is_none());
        let (socket, _other) = std::os::unix::net::UnixStream::pair().unwrap();
        assert!(map(socket.as_fd(), LEN, false).is_none());
    }

    /// How many mappings of the file `handle` is this process has.
    fn mappings_of(handle: &OwnedFd) -> usize {
        // A line of `maps` is: range, protection, offset, device, inode,
        // path. A memfd's inode is its own.
        let inode = fstat(handle).unwrap().st_ino.to_string();
        let maps = std::fs::read_to_string("/proc/self/maps").unwrap();
        maps.lines()
            .filter(|line| line.split_whitespace().nth(4) == Some(&inode))
            .filter(|line| line.contains("memfd:"))
            .count()
    }

    #[test]
    fn a_mapping_dropped_is_unmapped() {
        let (mine, handle) = create(LEN, true).unwrap();
        assert_eq!(mappings_of(&handle), 1);
        let kept = peer(&handle, false).unwrap();
        assert_eq!(mappings_of(&handle), 2);
        for _ in 0..64 {
            drop(peer(&handle, true).unwrap());
        }
        assert_eq!(mappings_of(&handle), 2);
        drop(kept);
        assert_eq!(mappings_of(&handle), 1);
        drop(mine);
        assert_eq!(mappings_of(&handle), 0);
    }

    #[test]
    fn a_pool_and_a_table_work_over_real_regions() {
        use crate::core::region::{Pool, Table};

        let (mine, handle) = create(LEN, false).unwrap();
        let mut pool = Pool::new(LEN);
        pool.add(7, mine).unwrap();
        let mut table = Table::new();
        assert!(table.insert(7, peer(&handle, false).unwrap()));
        let bytes: Vec<u8> = (0..LEN).map(|i| (i as u8).wrapping_mul(7)).collect();
        assert_eq!(pool.put(&bytes), Some(7));
        let mut got = Vec::new();
        table.read(7, LEN as u32, &mut got).unwrap();
        assert!(got == bytes);
    }
}
