//! Shared memory by sections of the page file (Chromium's
//! `platform_shared_memory_region_win.cc`). A region is made once, at the
//! length it keeps: such a section can't be grown or shrunk, so a peer
//! that holds one can't pull memory from under whoever mapped it.
//!
//! Two kinds are made, the two a channel uses (Chromium's
//! `ReadOnlySharedMemoryRegion` and `UnsafeSharedMemoryRegion`): one its
//! maker writes and the peer can only read, and one the peer writes. The
//! first's handle has the right to map for reading alone: the kernel
//! refuses to map it writable. And it can't be made into one with more:
//! the section's own list of who may open it allows nobody, and denies
//! its owner the changing of the list, so the rights a handle to it has
//! are all it will ever have. (Chromium's list is empty: it has its
//! sandbox to see that a renderer isn't the owner.)
//!
//! A handle a peer gives is mapped as it is, with no look at what kind
//! of section it is: only a child is given handles, by its host.

use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{DuplicateHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Security::{
    ACL, AddAccessDeniedAce, InitializeAcl, InitializeSecurityDescriptor, SECURITY_ATTRIBUTES,
    SECURITY_DESCRIPTOR, SID, SID_IDENTIFIER_AUTHORITY, SetSecurityDescriptorDacl,
};
use windows_sys::Win32::System::Memory::{
    CreateFileMappingW, FILE_MAP_READ, FILE_MAP_WRITE, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile,
    PAGE_READWRITE, UnmapViewOfFile,
};
use windows_sys::Win32::System::SystemInformation::{GetSystemInfo, SYSTEM_INFO};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

use crate::core::region::Memory;

/// `SECURITY_DESCRIPTOR_REVISION`, `ACL_REVISION` and `WRITE_DAC` (the
/// right to change an object's list) of `<winnt.h>`.
const SECURITY_DESCRIPTOR_REVISION: u32 = 1;
const ACL_REVISION: u32 = 2;
const WRITE_DAC: u32 = 0x4_0000;

/// A view of a region in this process, unmapped when dropped.
#[derive(Debug)]
pub struct Mapping {
    address: usize,
    len: usize,
}

// SAFETY: an address range of this process's, the same from every
// thread; what is read and written through `Memory` are bytes, copied
// whole.
unsafe impl Send for Mapping {}
// SAFETY: as above; two threads copying at once is the race the peer's
// own writes already are.
unsafe impl Sync for Mapping {}

// SAFETY: the range is mapped from `map` to `drop` and never moves; it is
// readable, and writable when it was mapped so, which whoever puts it in
// a pool did (`create` with `peer_writes` false, `map` with `writable`).
unsafe impl Memory for Mapping {
    fn ptr(&self) -> *mut u8 {
        std::ptr::with_exposed_provenance_mut(self.address)
    }

    fn len(&self) -> usize {
        self.len
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: this value's own view, mapped by `map` and unmapped
        // once.
        unsafe {
            UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                Value: self.ptr().cast(),
            })
        };
    }
}

/// The length of a page.
fn page() -> usize {
    // SAFETY: all zeroes is a `SYSTEM_INFO`.
    let mut info: SYSTEM_INFO = unsafe { std::mem::zeroed() };
    // SAFETY: the call writes `info`, alive for the call; it doesn't fail.
    unsafe { GetSystemInfo(&raw mut info) };
    usize::try_from(info.dwPageSize).unwrap_or(usize::MAX)
}

/// A new section of `len` bytes, which nothing can open: every handle to
/// it comes from the one returned. `None` when the kernel refuses.
fn section(len: usize) -> Option<OwnedHandle> {
    let size = u64::try_from(len).ok()?;
    let high = u32::try_from(size >> 32).ok()?;
    let low = u32::try_from(size & u64::from(u32::MAX)).ok()?;
    // The list's header and its one entry, in words (the kernel wants
    // it aligned so).
    let mut list = [0u32; 16];
    let acl: *mut ACL = list.as_mut_ptr().cast();
    let list_len = u32::try_from(size_of_val(&list)).ok()?;
    // "Owner rights" (S-1-3-4): whoever owns the object, as such.
    let mut owner = SID {
        Revision: 1,
        SubAuthorityCount: 1,
        IdentifierAuthority: SID_IDENTIFIER_AUTHORITY {
            Value: [0, 0, 0, 0, 0, 3],
        },
        SubAuthority: [4],
    };
    // SAFETY: all zeroes is a descriptor, before the calls below fill it.
    let mut descriptor: SECURITY_DESCRIPTOR = unsafe { std::mem::zeroed() };
    // SAFETY: each call writes the value it is given, alive for the call
    // (`acl` is `list`, of `list_len` bytes, which has room for the one
    // entry); the descriptor points to `list`, which outlives its use
    // below.
    let described = unsafe {
        // A list that allows nobody: no handle to the section is given
        // more rights than the handle it is duplicated from has. An
        // object's owner may change its list all the same, unless the
        // list says otherwise, and a process of the same user is the
        // owner: the one entry says otherwise.
        InitializeAcl(acl, list_len, ACL_REVISION) != 0
            && AddAccessDeniedAce(acl, ACL_REVISION, WRITE_DAC, (&raw mut owner).cast()) != 0
            && InitializeSecurityDescriptor(
                (&raw mut descriptor).cast(),
                SECURITY_DESCRIPTOR_REVISION,
            ) != 0
            && SetSecurityDescriptorDacl((&raw mut descriptor).cast(), 1, acl, 0) != 0
    };
    if !described {
        return None;
    }
    let attributes = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).ok()?,
        lpSecurityDescriptor: (&raw mut descriptor).cast(),
        bInheritHandle: 0,
    };
    // SAFETY: `attributes` and what it points to are alive for the call;
    // no file (the page file's memory) and no name.
    let handle = unsafe {
        CreateFileMappingW(
            INVALID_HANDLE_VALUE,
            &raw const attributes,
            PAGE_READWRITE,
            high,
            low,
            null(),
        )
    };
    if handle.is_null() {
        return None;
    }
    // SAFETY: the call succeeded: a new handle, this process's alone.
    Some(unsafe { OwnedHandle::from_raw_handle(handle) })
}

/// Another handle to what `handle` is, with `rights` alone, which a child
/// doesn't inherit. `None` when the kernel refuses: `handle` has less.
fn with_rights(handle: &OwnedHandle, rights: u32) -> Option<OwnedHandle> {
    let mut out: HANDLE = null_mut();
    // SAFETY: `handle` is open for the call, both processes are this one,
    // and `out` is alive for the call.
    let made = unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            handle.as_raw_handle(),
            GetCurrentProcess(),
            &raw mut out,
            rights,
            0,
            0,
        )
    };
    if made == 0 {
        return None;
    }
    // SAFETY: the call succeeded: a new handle, this process's alone.
    Some(unsafe { OwnedHandle::from_raw_handle(out) })
}

/// Maps the first `len` bytes of the region `handle` is, to write when
/// `writable`. `None` when the kernel refuses: the handle isn't a region,
/// is shorter, or is one that may only be read and `writable` asks for
/// more.
pub fn map(handle: &OwnedHandle, len: usize, writable: bool) -> Option<Mapping> {
    // A length of zero asks the kernel for all the section has.
    if len == 0 {
        return None;
    }
    let access = if writable {
        FILE_MAP_READ | FILE_MAP_WRITE
    } else {
        FILE_MAP_READ
    };
    // SAFETY: the kernel picks the address; nothing of this process's is
    // replaced. `handle` is open for the call.
    let view = unsafe { MapViewOfFile(handle.as_raw_handle(), access, 0, 0, len) };
    (!view.Value.is_null()).then(|| Mapping {
        address: view.Value.expose_provenance(),
        len,
    })
}

/// Makes a region of `len` bytes (whole pages): this process's mapping of
/// it, and the handle that gives a peer its own. With `peer_writes` the
/// peer may write it and this process's mapping is to read; without, this
/// process's mapping is to write and the peer's handle lets it only read.
pub fn create(len: usize, peer_writes: bool) -> Option<(Mapping, OwnedHandle)> {
    // The kernel makes a section of any length and maps whole pages of
    // it: a region is made at lengths that are whole pages already, so
    // anything else isn't a region.
    if len == 0 || len.checked_rem(page())? != 0 {
        return None;
    }
    let whole = section(len)?;
    // The handle the kernel gave has every right to the section; the one
    // that leaves this function has the rights to map it and no other,
    // and the first is let go here.
    if peer_writes {
        let mapping = map(&whole, len, false)?;
        return Some((
            mapping,
            with_rights(&whole, FILE_MAP_READ | FILE_MAP_WRITE)?,
        ));
    }
    let mapping = map(&whole, len, true)?;
    Some((mapping, with_rights(&whole, FILE_MAP_READ)?))
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::System::Memory::{PAGE_PROTECTION_FLAGS, VirtualProtect};
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use windows_sys::Win32::System::Threading::GetProcessHandleCount;

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

    #[test]
    fn what_the_maker_writes_the_peers_handle_reads() {
        let (mine, handle) = create(LEN, false).unwrap();
        assert_eq!(mine.len(), LEN);
        assert_eq!(read(&mine, 0), 0, "new memory is zeroes");
        assert_eq!(read(&mine, LEN - 1), 0);
        write(&mine, 0, 0x11);
        let theirs = map(&handle, LEN, false).unwrap();
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
        assert!(map(&handle, LEN, true).is_none());
        assert!(map(&handle, LEN, false).is_some());
    }

    #[test]
    fn a_handle_to_read_cant_be_made_one_to_write() {
        // What a peer with the handle could try: another handle to the
        // same section, asking for more than the one it was given has.
        const SECTION_ALL_ACCESS: u32 = 0xF_001F;
        let (_mine, handle) = create(LEN, false).unwrap();
        for more in [
            FILE_MAP_READ | FILE_MAP_WRITE,
            FILE_MAP_WRITE,
            SECTION_ALL_ACCESS,
            // To put itself on the section's list first.
            WRITE_DAC,
        ] {
            assert!(with_rights(&handle, more).is_none(), "{more:#x}");
        }
        // The same or less it may have.
        let same = with_rights(&handle, FILE_MAP_READ).unwrap();
        assert!(map(&same, LEN, true).is_none());
        assert!(map(&same, LEN, false).is_some());
        // And the handle to write has the rights to map, no other.
        let (_mine, handle) = create(LEN, true).unwrap();
        assert!(with_rights(&handle, FILE_MAP_READ | FILE_MAP_WRITE).is_some());
        assert!(with_rights(&handle, SECTION_ALL_ACCESS).is_none());
        assert!(with_rights(&handle, WRITE_DAC).is_none());
        // Not even the ones the handle the kernel first gave has besides
        // (to ask about the section, to read its list, to delete it).
        for (_mine, handle) in [create(LEN, true), create(LEN, false)].map(Option::unwrap) {
            for besides in [0x1, 0x1_0000, 0x2_0000] {
                assert!(with_rights(&handle, besides).is_none(), "{besides:#x}");
            }
        }
    }

    /// Whether the kernel lets `mapping` be made writable where it is.
    fn can_be_made_writable(mapping: &Mapping) -> bool {
        let mut before: PAGE_PROTECTION_FLAGS = 0;
        // SAFETY: changes the protection of the test's own mapping, or
        // fails; nothing reads it meanwhile. `before` is alive for the
        // call.
        unsafe {
            VirtualProtect(
                mapping.ptr().cast(),
                mapping.len(),
                PAGE_READWRITE,
                &raw mut before,
            ) != 0
        }
    }

    #[test]
    fn a_mapping_to_read_stays_one() {
        // The peer's mapping of what it may only read; the maker's own of
        // what the peer writes; a mapping to read of what may be written.
        let (mine, handle) = create(LEN, false).unwrap();
        let theirs = map(&handle, LEN, false).unwrap();
        assert!(!can_be_made_writable(&theirs));
        assert!(
            can_be_made_writable(&mine),
            "the maker's own was writable already"
        );
        let (mine, handle) = create(LEN, true).unwrap();
        assert!(!can_be_made_writable(&mine));
        assert!(!can_be_made_writable(&map(&handle, LEN, false).unwrap()));
        assert!(can_be_made_writable(&map(&handle, LEN, true).unwrap()));
    }

    #[test]
    fn what_the_peer_writes_the_maker_reads() {
        let (mine, handle) = create(LEN, true).unwrap();
        let theirs = map(&handle, LEN, true).unwrap();
        write(&theirs, 5, 0x44);
        write(&theirs, LEN - 1, 0x55);
        assert_eq!(read(&mine, 5), 0x44);
        assert_eq!(read(&mine, LEN - 1), 0x55);
        // A peer may map what it writes to read as well.
        let again = map(&handle, LEN, false).unwrap();
        assert_eq!(read(&again, 5), 0x44);
    }

    #[test]
    fn a_region_keeps_its_memory_while_anything_maps_it() {
        let (mine, handle) = create(LEN, true).unwrap();
        let theirs = map(&handle, LEN, true).unwrap();
        write(&theirs, 0, 0x66);
        drop(handle);
        drop(mine);
        assert_eq!(read(&theirs, 0), 0x66);
        write(&theirs, 1, 0x77);
        assert_eq!(read(&theirs, 1), 0x77);
    }

    #[test]
    fn no_more_than_a_region_has_is_mapped() {
        let (_mine, handle) = create(LEN, true).unwrap();
        assert!(map(&handle, LEN + 1, true).is_none());
        assert!(map(&handle, 2 * LEN, false).is_none());
        // Nor all of it by asking for nothing.
        assert!(map(&handle, 0, true).is_none());
        assert!(map(&handle, 0, false).is_none());
        // Less is: its start.
        let part = map(&handle, LEN / 4, true).unwrap();
        assert_eq!(part.len(), LEN / 4);
        write(&part, LEN / 4 - 1, 0x12);
        let whole = map(&handle, LEN, false).unwrap();
        assert_eq!(read(&whole, LEN / 4 - 1), 0x12);
    }

    #[test]
    fn a_region_is_made_only_at_a_length_of_whole_pages() {
        assert!(create(LEN + 1, false).is_none());
        assert!(create(LEN + 1, true).is_none());
        assert!(create(1, false).is_none());
        assert!(create(0, false).is_none());
        assert!(create(0, true).is_none());
        for len in [page(), LEN, 4 * LEN, 1 << 20] {
            for peer_writes in [false, true] {
                let (mine, handle) = create(len, peer_writes).unwrap();
                assert_eq!(mine.len(), len);
                assert_eq!(map(&handle, len, false).unwrap().len(), len);
                // The section is that long and no longer.
                assert!(map(&handle, len + page(), false).is_none());
            }
        }
    }

    #[test]
    fn what_isnt_a_region_isnt_mapped() {
        let file = std::fs::File::open(std::env::current_exe().unwrap()).unwrap();
        let handle = OwnedHandle::from(file);
        assert!(map(&handle, LEN, false).is_none());
        assert!(map(&handle, LEN, true).is_none());
    }

    #[test]
    fn a_call_the_kernel_refuses_gives_no_handle() {
        // More than the system has to give.
        assert!(section(1 << 46).is_none());
        assert!(create(1 << 46, false).is_none());
        assert!(section(LEN).is_some());
    }

    #[test]
    fn a_child_inherits_no_handle_to_a_region() {
        use windows_sys::Win32::Foundation::{GetHandleInformation, HANDLE_FLAG_INHERIT};

        for peer_writes in [false, true] {
            let (_mine, handle) = create(LEN, peer_writes).unwrap();
            let mut flags = 0;
            // SAFETY: `handle` is open and `flags` alive for the call.
            assert!(unsafe { GetHandleInformation(handle.as_raw_handle(), &raw mut flags) } != 0);
            assert_eq!(flags & HANDLE_FLAG_INHERIT, 0);
        }
    }

    /// How many handles this process has open.
    fn handles() -> u32 {
        let mut count = 0;
        // SAFETY: `count` is alive for the call.
        assert!(unsafe { GetProcessHandleCount(GetCurrentProcess(), &raw mut count) } != 0);
        count
    }

    #[test]
    fn a_region_let_go_leaves_no_handle() {
        // Other tests open and close handles meanwhile, a few at a time,
        // not thousands.
        let before = handles();
        for i in 0..4000 {
            let (mine, handle) = create(LEN, i % 2 == 0).unwrap();
            assert!(map(&handle, LEN, false).is_some());
            drop(mine);
        }
        let more = handles().saturating_sub(before);
        assert!(more < 1000, "{more} handles more after 4000 regions");
        // And kept, they are counted: the measure sees what it is meant
        // to.
        let kept: Vec<_> = (0..4000).map(|_| create(LEN, false).unwrap()).collect();
        let more = handles().saturating_sub(before);
        assert!(more >= 3000, "4000 regions kept are only {more} handles");
        // One each: the handle the kernel first gave isn't kept too.
        assert!(more < 5000, "4000 regions kept are {more} handles");
        drop(kept);
    }

    /// The address space this process has left, in bytes.
    fn address_space_left() -> u64 {
        // SAFETY: all zeroes is a `MEMORYSTATUSEX`, but for its length.
        let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
        status.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
        // SAFETY: `status` says its own length and is alive for the call.
        assert!(unsafe { GlobalMemoryStatusEx(&raw mut status) } != 0);
        status.ullAvailVirtual
    }

    #[test]
    fn a_mapping_dropped_is_unmapped() {
        // 4 GiB mapped and dropped, 16 MiB at a time: were the mappings
        // kept, the process's address space would shrink by that much.
        // (Other tests map and unmap meanwhile, megabytes of it, not
        // gigabytes.)
        const BIG: usize = 16 << 20;
        let (mine, handle) = create(BIG, true).unwrap();
        let before = address_space_left();
        for _ in 0..256 {
            let mapping = map(&handle, BIG, true).unwrap();
            assert_eq!(mapping.len(), BIG);
        }
        let used = before.saturating_sub(address_space_left());
        assert!(used < 1 << 30, "{used} bytes of address space gone");
        // And kept, it does shrink: the measure sees what it is meant to.
        let kept: Vec<Mapping> = (0..256).map(|_| map(&handle, BIG, true).unwrap()).collect();
        let used = before.saturating_sub(address_space_left());
        assert!(
            used >= 3 << 30,
            "256 mappings kept took only {used} bytes of address space"
        );
        drop(kept);
        drop(mine);
    }

    #[test]
    fn a_pool_and_a_table_work_over_real_regions() {
        use crate::core::region::{Pool, Table};

        let (mine, handle) = create(LEN, false).unwrap();
        let mut pool = Pool::new(LEN);
        pool.add(7, mine).unwrap();
        let mut table = Table::new();
        assert!(table.insert(7, map(&handle, LEN, false).unwrap()));
        let bytes: Vec<u8> = (0..LEN).map(|i| (i as u8).wrapping_mul(7)).collect();
        assert_eq!(pool.put(&bytes), Some(7));
        let mut got = Vec::new();
        table.read(7, LEN as u32, &mut got).unwrap();
        assert!(got == bytes);
    }
}
