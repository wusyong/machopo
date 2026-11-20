//! Shared memory by Mach memory entries: a port stands for the memory, with
//! no name and no descriptor. A read-only entry can't be mapped writable.

use std::collections::HashMap;

use mach2::kern_return::{KERN_SUCCESS, kern_return_t};
use mach2::port::{MACH_PORT_NULL, mach_port_t};
use mach2::traps::mach_task_self;
use mach2::vm::{mach_make_memory_entry_64, mach_vm_deallocate, mach_vm_map};
use mach2::vm_inherit::VM_INHERIT_NONE;
use mach2::vm_prot::{VM_PROT_READ, VM_PROT_WRITE};
use mach2::vm_statistics::VM_FLAGS_ANYWHERE;

use crate::sys::MAP_MEM_NAMED_CREATE;

/// A mapping of a region in this task. Never a `&[u8]`: the other side may
/// write it at any time, so it is read and written through pointers.
pub struct Mapping {
    address: u64,
    len: usize,
}

// SAFETY: an address and a length; what is there is reached by raw pointer
// copies and atomics alone.
unsafe impl Send for Mapping {}
unsafe impl Sync for Mapping {}

impl Mapping {
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn ptr(&self) -> *mut u8 {
        self.address as *mut u8
    }

    /// Copies `len` bytes at `offset` out, if they are in the region.
    pub fn copy_out(&self, offset: usize, len: usize, into: &mut Vec<u8>) -> bool {
        let Some(end) = offset.checked_add(len) else {
            return false;
        };
        if end > self.len {
            return false;
        }
        into.clear();
        into.reserve(len);
        // SAFETY: `offset..end` is inside the mapping, `into` has room for
        // `len` bytes, and the two don't overlap. What the bytes are is
        // the other side's business: any value is a valid `u8`.
        unsafe {
            std::ptr::copy_nonoverlapping(self.ptr().add(offset), into.as_mut_ptr(), len);
            into.set_len(len);
        }
        true
    }

    /// Copies `bytes` in at `offset` (which the caller knows is inside).
    pub fn copy_in(&self, offset: usize, bytes: &[u8]) {
        assert!(offset.checked_add(bytes.len()).is_some_and(|end| end <= self.len));
        // SAFETY: inside the mapping, which is mapped writable by whoever
        // calls this, and not overlapping `bytes`.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), self.ptr().add(offset), bytes.len()) };
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: this mapping's own range.
        unsafe { mach_vm_deallocate(mach_task_self(), self.address, self.len as u64) };
    }
}

/// A new region of at least `size` bytes: its entry (a send right, to map
/// or hand over) and the size the kernel gave it.
pub fn create(size: usize) -> (mach_port_t, usize) {
    let mut actual = size as u64;
    let mut entry = MACH_PORT_NULL;
    // SAFETY: a call that writes a size and a name.
    let kr = unsafe {
        mach_make_memory_entry_64(
            mach_task_self(),
            &mut actual,
            0,
            MAP_MEM_NAMED_CREATE | VM_PROT_READ | VM_PROT_WRITE,
            &mut entry,
            MACH_PORT_NULL,
        )
    };
    assert_eq!(kr, KERN_SUCCESS, "mach_make_memory_entry_64");
    (entry, usize::try_from(actual).unwrap())
}

/// Maps `len` bytes of `entry`.
pub fn map(entry: mach_port_t, len: usize, writable: bool) -> Result<Mapping, kern_return_t> {
    let mut address = 0u64;
    let protection = if writable { VM_PROT_READ | VM_PROT_WRITE } else { VM_PROT_READ };
    // SAFETY: the kernel picks the address; the entry is a right we hold.
    let kr = unsafe {
        mach_vm_map(
            mach_task_self(),
            &mut address,
            len as u64,
            0,
            VM_FLAGS_ANYWHERE,
            entry,
            0,
            0,
            protection,
            protection,
            VM_INHERIT_NONE,
        )
    };
    if kr == KERN_SUCCESS { Ok(Mapping { address, len }) } else { Err(kr) }
}

/// A read-only entry for the memory `mapping` maps (as Chromium's
/// `ConvertToReadOnly`: an entry made from a mapping, with read
/// permission alone).
pub fn read_only_entry(mapping: &Mapping) -> mach_port_t {
    let mut size = mapping.len as u64;
    let mut entry = MACH_PORT_NULL;
    // SAFETY: a call on this task's own mapping.
    let kr = unsafe {
        mach_make_memory_entry_64(
            mach_task_self(),
            &mut size,
            mapping.address,
            VM_PROT_READ,
            &mut entry,
            MACH_PORT_NULL,
        )
    };
    assert_eq!(kr, KERN_SUCCESS, "mach_make_memory_entry_64 (read-only)");
    entry
}

/// The regions a peer may name, by the numbers their maker gave them: the
/// same table at whichever end checks.
#[derive(Default)]
pub struct RegionTable {
    regions: HashMap<u32, Mapping>,
}

impl RegionTable {
    pub fn insert(&mut self, number: u32, mapping: Mapping) {
        self.regions.insert(number, mapping);
    }

    pub fn get(&self, number: u32) -> Option<&Mapping> {
        self.regions.get(&number)
    }

    /// `len` bytes at `offset` of region `number`, copied out: `Err` says
    /// why the peer's message is a bad one.
    pub fn read(&self, number: u32, offset: u32, len: u32, into: &mut Vec<u8>) -> Result<(), &'static str> {
        let mapping = self.regions.get(&number).ok_or("a region that isn't the peer's")?;
        if mapping.copy_out(offset as usize, len as usize, into) {
            Ok(())
        } else {
            Err("a range outside its region")
        }
    }
}
