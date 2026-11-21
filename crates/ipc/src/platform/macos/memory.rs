//! Shared memory by Mach memory entries (Chromium's
//! `platform_shared_memory_region_apple.cc`). A region is made once, at
//! the length it keeps: an entry can't be grown or shrunk, so a peer that
//! holds one can't pull memory from under whoever mapped it.
//!
//! Two kinds are made, the two a channel uses (Chromium's
//! `ReadOnlySharedMemoryRegion` and `UnsafeSharedMemoryRegion`): one its
//! maker writes and the peer can only read, and one the peer writes. The
//! first's handle is an entry with read permission alone: the kernel
//! refuses to map it writable.

use mach2::kern_return::KERN_SUCCESS;
use mach2::mach_port::mach_port_deallocate;
use mach2::port::{MACH_PORT_NULL, mach_port_t};
use mach2::traps::mach_task_self;
use mach2::vm::{mach_make_memory_entry_64, mach_vm_deallocate, mach_vm_map};
use mach2::vm_inherit::VM_INHERIT_NONE;
use mach2::vm_prot::{VM_PROT_READ, VM_PROT_WRITE, vm_prot_t};
use mach2::vm_statistics::VM_FLAGS_ANYWHERE;

use crate::core::region::Memory;

/// `MAP_MEM_NAMED_CREATE` (`<mach/memory_object_types.h>`), which mach2
/// doesn't declare: make new memory for the entry, not an entry for
/// memory there already is.
const MAP_MEM_NAMED_CREATE: vm_prot_t = 0x02_0000;

/// A send right this task holds, let go when dropped: a region's handle.
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
}

impl Drop for SendRight {
    fn drop(&mut self) {
        // SAFETY: the name is a right this value owns, let go once.
        unsafe { mach_port_deallocate(mach_task_self(), self.0) };
    }
}

/// A mapping of a region in this task, unmapped when dropped.
#[derive(Debug)]
pub struct Mapping {
    address: u64,
    len: usize,
}

// SAFETY: an address range of this task's, the same from every thread;
// what is read and written through `Memory` are bytes, copied whole.
unsafe impl Send for Mapping {}
// SAFETY: as above; two threads copying at once is the race the peer's
// own writes already are.
unsafe impl Sync for Mapping {}

// SAFETY: the range is mapped from `map` to `drop` and never moves; it is
// readable, and writable when it was mapped so, which whoever puts it in
// a pool did (`create` with `peer_writes` false, `map` with `writable`).
unsafe impl Memory for Mapping {
    fn ptr(&self) -> *mut u8 {
        std::ptr::with_exposed_provenance_mut(usize::try_from(self.address).unwrap_or(0))
    }

    fn len(&self) -> usize {
        self.len
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: this value's own range, mapped by `map` and unmapped
        // once; `len` came from a `u64` there.
        unsafe { mach_vm_deallocate(mach_task_self(), self.address, self.len as u64) };
    }
}

/// A memory entry: for `len` new bytes (`from` is `None`), or for the
/// memory mapped at `from`, with `permission`. `None` when the kernel
/// refuses, or gives an entry of another length.
fn entry(len: usize, from: Option<u64>, permission: vm_prot_t) -> Option<SendRight> {
    let wanted = u64::try_from(len).ok()?;
    let mut size = wanted;
    let mut name = MACH_PORT_NULL;
    let flags = match from {
        Some(_) => permission,
        None => MAP_MEM_NAMED_CREATE | permission,
    };
    // SAFETY: the call reads `size` and writes it and `name`, both alive
    // for the call; `from` is this task's own mapping or nothing.
    let result = unsafe {
        mach_make_memory_entry_64(
            mach_task_self(),
            &raw mut size,
            from.unwrap_or(0),
            flags,
            &raw mut name,
            MACH_PORT_NULL,
        )
    };
    if result != KERN_SUCCESS {
        return None;
    }
    // SAFETY: the call succeeded: `name` is a new send right, this
    // task's alone.
    let right = unsafe { SendRight::from_raw(name) };
    // The kernel rounds a length up to whole pages: a region is made at
    // lengths that are whole pages already, so anything else isn't the
    // region asked for.
    (size == wanted).then_some(right)
}

/// Maps the first `len` bytes of the region `handle` is, to write when
/// `writable`. `None` when the kernel refuses: the handle isn't a region,
/// is shorter, or is one that may only be read and `writable` asks for
/// more.
pub fn map(handle: &SendRight, len: usize, writable: bool) -> Option<Mapping> {
    let size = u64::try_from(len).ok()?;
    let protection = if writable {
        VM_PROT_READ | VM_PROT_WRITE
    } else {
        VM_PROT_READ
    };
    let mut address = 0u64;
    // SAFETY: the kernel picks the address (`VM_FLAGS_ANYWHERE`) and
    // writes it to `address`, alive for the call; nothing of this task's
    // is replaced.
    let result = unsafe {
        mach_vm_map(
            mach_task_self(),
            &raw mut address,
            size,
            0,
            VM_FLAGS_ANYWHERE,
            handle.name(),
            0,
            0,
            protection,
            // The most the mapping may ever be given: a mapping to read
            // can't be made writable later.
            protection,
            VM_INHERIT_NONE,
        )
    };
    (result == KERN_SUCCESS).then_some(Mapping { address, len })
}

/// Makes a region of `len` bytes (whole pages): this task's mapping of
/// it, and the handle that gives a peer its own. With `peer_writes` the
/// peer may write it and this task's mapping is to read; without, this
/// task's mapping is to write and the peer's handle lets it only read.
pub fn create(len: usize, peer_writes: bool) -> Option<(Mapping, SendRight)> {
    let whole = entry(len, None, VM_PROT_READ | VM_PROT_WRITE)?;
    if peer_writes {
        let mapping = map(&whole, len, false)?;
        return Some((mapping, whole));
    }
    let mapping = map(&whole, len, true)?;
    // An entry for the same memory with read permission alone (as
    // Chromium's `ConvertToReadOnly`); the one that may be written is let
    // go here, so no right to write it leaves this function.
    let read_only = entry(len, Some(mapping.address), VM_PROT_READ)?;
    Some((mapping, read_only))
}

#[cfg(test)]
mod tests {
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

    /// Whether `name` is still a right in this task: letting go of a name
    /// that isn't one fails. (Used on names just let go, before anything
    /// else is made that could take the name.)
    fn let_go_already(name: mach_port_t) -> bool {
        // SAFETY: a name the test owned and let go; if the call succeeds
        // the test fails.
        unsafe { mach_port_deallocate(mach_task_self(), name) != KERN_SUCCESS }
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

    /// Whether the kernel lets `mapping` be made writable where it is.
    fn can_be_made_writable(mapping: &Mapping) -> bool {
        // SAFETY: changes the protection of the test's own mapping, or
        // fails; nothing reads it meanwhile.
        let result = unsafe {
            mach2::vm::mach_vm_protect(
                mach_task_self(),
                mapping.address,
                mapping.len as u64,
                0,
                VM_PROT_READ | VM_PROT_WRITE,
            )
        };
        result == KERN_SUCCESS
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
        for len in [LEN, 4 * LEN, 1 << 20] {
            for peer_writes in [false, true] {
                let (mine, handle) = create(len, peer_writes).unwrap();
                assert_eq!(mine.len(), len);
                assert_eq!(map(&handle, len, false).unwrap().len(), len);
            }
        }
    }

    #[test]
    fn a_call_the_kernel_refuses_gives_no_handle() {
        // No memory there to make an entry for.
        assert!(entry(LEN, Some(16), VM_PROT_READ).is_none());
        assert!(entry(LEN, None, VM_PROT_READ | VM_PROT_WRITE).is_some());
    }

    #[test]
    fn what_isnt_a_region_isnt_mapped() {
        // SAFETY: this task's own right to itself, given one more
        // reference by the call, which the value lets go of.
        let task = unsafe { SendRight::from_raw(mach2::mach_init::mach_thread_self()) };
        assert!(map(&task, LEN, false).is_none());
    }

    #[test]
    fn a_handle_dropped_is_a_right_let_go() {
        let (mine, handle) = create(LEN, false).unwrap();
        let name = handle.name();
        drop(handle);
        assert!(let_go_already(name));
        drop(mine);
        // Moved into a message, it isn't let go by the value.
        let (_mine, handle) = create(LEN, true).unwrap();
        let name = handle.into_raw();
        assert!(!let_go_already(name), "it was this task's until now");
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
        assert_eq!(result, KERN_SUCCESS);
        info.virtual_size
    }

    #[test]
    fn a_mapping_dropped_is_unmapped() {
        // 4 GiB mapped and dropped, 16 MiB at a time: were the mappings
        // kept, the task's address space would grow by that much. (Other
        // tests map and unmap meanwhile, megabytes of it, not gigabytes.)
        const BIG: usize = 16 << 20;
        let (mine, handle) = create(BIG, true).unwrap();
        let before = virtual_size();
        for _ in 0..256 {
            let mapping = map(&handle, BIG, true).unwrap();
            assert_eq!(mapping.len(), BIG);
        }
        let grew = virtual_size().saturating_sub(before);
        assert!(grew < 1 << 30, "the address space grew by {grew} bytes");
        // And kept, it does grow: the measure sees what it is meant to.
        let kept: Vec<Mapping> = (0..256).map(|_| map(&handle, BIG, true).unwrap()).collect();
        let grew = virtual_size().saturating_sub(before);
        assert!(
            grew >= 3 << 30,
            "256 mappings kept grew the address space by only {grew} bytes"
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
