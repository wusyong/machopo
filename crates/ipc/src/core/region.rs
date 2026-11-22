//! Shared memory as the core sees it (Chromium's `BigBuffer` over
//! `base::*SharedMemoryRegion`, kept and used again where Chromium makes
//! one a time): a region is a number its maker gave it and a length of
//! memory, whatever platform mapped it. A [`Pool`] is the regions this end
//! writes large data into, one piece of data to a region; a [`Table`] is
//! the regions the peer writes into, read by number and checked. What says
//! a region may be written again is a message
//! ([`Control`](super::control::Control)), so nothing in the shared memory
//! itself is state both ends write.
//!
//! Memory the peer may be writing is never a `&[u8]`: bytes come out of
//! it through pointers, into memory of this end's own ([`copy_out`]). A
//! region this end writes is a slice only for whoever took it out of the
//! pool, and only until it is sent ([`bytes_mut`]): the peer reads it, and
//! writes nothing.

use std::marker::PhantomData;
use std::sync::Arc;

pub use super::fit::{MAX_REGION_LEN, MIN_REGION_LEN, class_len};
use super::fit::{Slot, admits, best_fit};
use super::frame::BadMessage;

/// A length of memory another process may see too.
///
/// # Safety
///
/// For as long as the value lives, `ptr()` is valid for reads of `len()`
/// bytes, and for writes of them when the value is put in a [`Pool`];
/// neither changes. Nothing else in this process holds a reference into
/// the memory.
#[expect(clippy::len_without_is_empty, reason = "a region is never empty")]
pub unsafe trait Memory {
    fn ptr(&self) -> *mut u8;
    fn len(&self) -> usize;
}

// SAFETY: the `Arc` keeps the memory it shares alive and unmoved; the
// promises are the inner value's.
unsafe impl<M: Memory> Memory for Arc<M> {
    fn ptr(&self) -> *mut u8 {
        (**self).ptr()
    }

    fn len(&self) -> usize {
        (**self).len()
    }
}

/// Memory of this process's own heap standing in for a mapping: what the
/// two ends of a channel within one process share, and what the core's
/// tests run on (under Miri too).
pub struct HeapMemory {
    ptr: *mut [u8],
}

impl HeapMemory {
    pub fn new(len: usize) -> Self {
        Self {
            ptr: Box::into_raw(vec![0; len].into_boxed_slice()),
        }
    }
}

impl Drop for HeapMemory {
    fn drop(&mut self) {
        // SAFETY: `ptr` came from `Box::into_raw` in `new` and is given
        // back only here.
        drop(unsafe { Box::from_raw(self.ptr) });
    }
}

// SAFETY: the allocation is this value's own; what is read and written
// through `Memory` are bytes, copied whole.
unsafe impl Send for HeapMemory {}
// SAFETY: as above; two ends copying at once is the race a mapping shared
// with another process has, and both copy bytes.
unsafe impl Sync for HeapMemory {}

// SAFETY: the box lives until `drop`, is `len()` bytes, and is reached
// only through `ptr`.
unsafe impl Memory for HeapMemory {
    fn ptr(&self) -> *mut u8 {
        self.ptr.cast()
    }

    fn len(&self) -> usize {
        self.ptr.len()
    }
}

/// Why a region didn't join a [`Pool`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddError {
    /// The pool has a region of that number.
    Duplicate,
    /// Not a length regions are made at ([`class_len`]).
    NotAClass,
    /// More than the pool may hold in all.
    OverLimit,
}

/// The first `into.len()` bytes from `offset` of `memory`, copied out.
/// `false`, and nothing copied, when they aren't all in it. What the peer
/// writes meanwhile comes out as whatever bytes were there: the copy is
/// this end's own from then on.
pub fn copy_out<M: Memory + ?Sized>(memory: &M, offset: usize, into: &mut [u8]) -> bool {
    let within = offset
        .checked_add(into.len())
        .is_some_and(|end| end <= memory.len());
    if !within {
        return false;
    }
    // SAFETY: `offset + into.len()` is no more than the memory's length,
    // which is readable; `into` is this process's own, apart from it.
    unsafe {
        std::ptr::copy_nonoverlapping(memory.ptr().add(offset), into.as_mut_ptr(), into.len());
    }
    true
}

/// Runs of `N` bytes of a memory one after another, each copied out as
/// it is asked for ([`chunks_out`]): for whoever takes bytes by value
/// (a destination that is only written to, and so gives no slice to copy
/// into).
pub struct ChunksOut<'a, const N: usize> {
    /// The next run: with the `left` runs from it, all in the memory.
    next: *const u8,
    left: usize,
    memory: PhantomData<&'a ()>,
}

/// The `count` runs of `N` bytes from `offset` of `memory`. `None` when
/// they aren't all in it.
pub fn chunks_out<M: Memory + ?Sized, const N: usize>(
    memory: &M,
    offset: usize,
    count: usize,
) -> Option<ChunksOut<'_, N>> {
    let end = count.checked_mul(N)?.checked_add(offset)?;
    if end > memory.len() {
        return None;
    }
    Some(ChunksOut {
        // SAFETY: `offset` is no more than the memory's length.
        next: unsafe { memory.ptr().add(offset) },
        left: count,
        memory: PhantomData,
    })
}

impl<const N: usize> Iterator for ChunksOut<'_, N> {
    type Item = [u8; N];

    #[inline]
    fn next(&mut self) -> Option<[u8; N]> {
        self.left = self.left.checked_sub(1)?;
        // SAFETY: `chunks_out` checked that the runs counted from `next`
        // are in the memory, which lives and stays readable for `'a`;
        // one was left, so `N` bytes are there to read, and the pointer
        // after them is in the memory or at its end. Bytes need no
        // alignment and any are valid.
        let chunk = unsafe {
            let chunk = self.next.cast::<[u8; N]>().read_unaligned();
            self.next = self.next.add(N);
            chunk
        };
        Some(chunk)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.left, Some(self.left))
    }
}

impl<const N: usize> ExactSizeIterator for ChunksOut<'_, N> {}

/// The first `len` bytes of `memory`, to write and to read back.
///
/// # Safety
///
/// `memory` is writable and `len` is no more than its length. For as
/// long as the slice lives nothing else in this process reads or writes
/// the memory, and nothing outside it writes there: the region is out of
/// its [`Pool`] to the caller alone ([`Pool::take`]) and not yet sent
/// ([`Pool::lend`]), and the peer's mapping is one it only reads by.
#[expect(
    clippy::mut_from_ref,
    reason = "the memory is behind a pointer; what makes the slice the only way to it is the caller's promise"
)]
pub unsafe fn bytes_mut<M: Memory + ?Sized>(memory: &M, len: usize) -> &mut [u8] {
    debug_assert!(len <= memory.len());
    // SAFETY: the caller's promises: `len` bytes that are valid to read
    // and write, reached by nothing else while the slice lives. The bytes
    // are initialized: a mapping's are zero or what was written.
    unsafe { std::slice::from_raw_parts_mut(memory.ptr(), len) }
}

/// The first `len` bytes of `memory`, to read.
///
/// # Safety
///
/// As [`bytes_mut`], but for other slices this one made: for as long as
/// the slice lives nothing writes the memory at all.
pub unsafe fn bytes<M: Memory + ?Sized>(memory: &M, len: usize) -> &[u8] {
    debug_assert!(len <= memory.len());
    // SAFETY: the caller's promises: `len` bytes that are valid to read
    // and that nothing writes while the slice lives.
    unsafe { std::slice::from_raw_parts(memory.ptr(), len) }
}

/// The regions this end writes large data into (the sending half of
/// Chromium's `BigBuffer`, with the regions kept): each holds one piece of
/// data at a time, and is written again only once the peer said it had
/// read it, or whoever took it gave it back.
pub struct Pool<M> {
    /// What the search reads; `regions[i]` is slot `i`'s.
    slots: Vec<Slot>,
    regions: Vec<(u32, M)>,
    /// `held[i]`: slot `i` is out to a taker that hasn't sent it
    /// ([`Pool::take`]). The peer hasn't heard of what is in it, so it
    /// isn't the peer's to say it was read.
    held: Vec<bool>,
    total: usize,
    limit: usize,
}

impl<M: Memory> Pool<M> {
    /// A pool that holds `limit` bytes of regions at most.
    pub const fn new(limit: usize) -> Self {
        Self {
            slots: Vec::new(),
            regions: Vec::new(),
            held: Vec::new(),
            total: 0,
            limit,
        }
    }

    fn index(&self, number: u32) -> Option<usize> {
        self.regions.iter().position(|(n, _)| *n == number)
    }

    /// A region joins, free. `memory` is writable.
    pub fn add(&mut self, number: u32, memory: M) -> Result<(), AddError> {
        let len = memory.len();
        if self.index(number).is_some() {
            return Err(AddError::Duplicate);
        }
        if class_len(len) != Some(len) {
            return Err(AddError::NotAClass);
        }
        if !admits(self.total, self.limit, len) {
            return Err(AddError::OverLimit);
        }
        self.total = self.total.saturating_add(len);
        self.slots.push(Slot { len, out: false });
        self.regions.push((number, memory));
        self.held.push(false);
        Ok(())
    }

    /// The bytes of regions held.
    pub const fn total(&self) -> usize {
        self.total
    }

    /// A free region of `len` bytes or more, the shortest there is, now
    /// out to the caller, to write as it likes: its number. `None` when
    /// no free one is long enough. It is the caller's until it is lent
    /// ([`Pool::lend`]) or given back; the peer saying it read it frees
    /// nothing.
    pub fn take(&mut self, len: usize) -> Option<u32> {
        let i = best_fit(&self.slots, len)?;
        self.slots.get_mut(i)?.out = true;
        *self.held.get_mut(i)? = true;
        Some(self.regions.get(i)?.0)
    }

    /// A message is about to name the region `number`, which its taker
    /// wrote: from now it is out as one bytes were put in, the peer's to
    /// say it read. `false` when no taker holds it (it was lent already,
    /// or never taken).
    pub fn lend(&mut self, number: u32) -> bool {
        let held = self.index(number).and_then(|i| self.held.get_mut(i));
        match held {
            Some(held) if *held => {
                *held = false;
                true
            }
            _ => false,
        }
    }

    /// `bytes` copied into a free region, now out: its number. `None` when
    /// no free one is long enough.
    pub fn put(&mut self, bytes: &[u8]) -> Option<u32> {
        let i = best_fit(&self.slots, bytes.len())?;
        let (number, memory) = self.regions.get(i)?;
        debug_assert!(bytes.len() <= memory.len());
        // SAFETY: `best_fit` returns a slot whose length, the region's, is
        // at least `bytes.len()` (proved), and the region is writable for
        // its length; `bytes` is this process's own memory, apart from it.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), memory.ptr(), bytes.len()) };
        self.slots.get_mut(i)?.out = true;
        Some(*number)
    }

    /// The memory of a region of this pool.
    pub fn memory(&self, number: u32) -> Option<&M> {
        Some(&self.regions.get(self.index(number)?)?.1)
    }

    /// Whoever took the region is done with it: free again. `false` when
    /// it wasn't out.
    pub fn give_back(&mut self, number: u32) -> bool {
        let Some(i) = self.index(number) else {
            return false;
        };
        match (self.slots.get_mut(i), self.held.get_mut(i)) {
            (Some(slot), Some(held)) if slot.out => {
                slot.out = false;
                *held = false;
                true
            }
            _ => false,
        }
    }

    /// The peer said it had read what was in the region: free again. A
    /// region that isn't this pool's, that nothing is out in, or that its
    /// taker still writes (no message named it: freed, the next taker
    /// would write the same memory) is the peer's mistake.
    pub fn released(&mut self, number: u32) -> Result<(), BadMessage> {
        let Some(i) = self.index(number) else {
            return Err(BadMessage::UnknownRegion { number });
        };
        if self.held.get(i).copied().unwrap_or(false) {
            return Err(BadMessage::RegionNotOut { number });
        }
        if self.give_back(number) {
            Ok(())
        } else {
            Err(BadMessage::RegionNotOut { number })
        }
    }

    /// The free regions, as (number, length): what there is to let go of.
    pub fn free(&self) -> impl Iterator<Item = (u32, usize)> {
        self.slots
            .iter()
            .zip(&self.regions)
            .filter(|(slot, _)| !slot.out)
            .map(|(slot, (number, _))| (*number, slot.len))
    }

    /// A free region leaves the pool. `None` when it isn't there or is
    /// out: what the peer has yet to read isn't taken from under it.
    pub fn remove(&mut self, number: u32) -> Option<M> {
        let i = self.index(number)?;
        if self.slots.get(i)?.out {
            return None;
        }
        let slot = self.slots.swap_remove(i);
        self.held.swap_remove(i);
        self.total = self.total.saturating_sub(slot.len);
        Some(self.regions.swap_remove(i).1)
    }
}

/// The regions the peer writes into, by the numbers they were made with
/// (the receiving half): the check of what a message names. The peer can
/// still write what it named, so the bytes are copied out before anything
/// looks at them.
pub struct Table<M> {
    regions: Vec<(u32, M)>,
    /// The regions read since [`Table::take_read`], for the message that
    /// gives them back.
    read: Vec<u32>,
}

impl<M> Default for Table<M> {
    fn default() -> Self {
        Self {
            regions: Vec::new(),
            read: Vec::new(),
        }
    }
}

impl<M: Memory> Table<M> {
    pub fn new() -> Self {
        Self::default()
    }

    /// A region the peer may name from now on. `false`, and the region
    /// isn't kept, when the number is taken.
    pub fn insert(&mut self, number: u32, memory: M) -> bool {
        if self.regions.iter().any(|(n, _)| *n == number) {
            return false;
        }
        self.regions.push((number, memory));
        true
    }

    /// The memory of region `number`, which a message says holds `len`
    /// bytes: checked, and not read. Whoever reads it says when it may be
    /// written again; [`Table::take_read`] doesn't.
    pub fn named(&self, number: u32, len: u32) -> Result<&M, BadMessage> {
        let Some((_, memory)) = self.regions.iter().find(|(n, _)| *n == number) else {
            return Err(BadMessage::UnknownRegion { number });
        };
        match usize::try_from(len) {
            Ok(count) if count <= memory.len() => Ok(memory),
            _ => Err(BadMessage::OutsideRegion { number, len }),
        }
    }

    /// The first `len` bytes of region `number`, copied to the end of
    /// `into`.
    pub fn read(&mut self, number: u32, len: u32, into: &mut Vec<u8>) -> Result<(), BadMessage> {
        let memory = self.named(number, len)?;
        // `named` checked it against the region's length, a `usize`.
        let count = usize::try_from(len).unwrap_or(0);
        into.reserve(count);
        let spare = into.spare_capacity_mut();
        debug_assert!(count <= spare.len());
        // SAFETY: `count` is no more than the region's length, which is
        // readable, and no more than the spare capacity just reserved;
        // the vector is this process's own, apart from the region.
        unsafe { std::ptr::copy_nonoverlapping(memory.ptr(), spare.as_mut_ptr().cast(), count) };
        let filled = into.len().saturating_add(count);
        // SAFETY: the `count` bytes after the old length were just
        // written, within the capacity.
        unsafe { into.set_len(filled) };
        self.read.push(number);
        Ok(())
    }

    /// The regions read since the last call, in the order read.
    pub fn take_read(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.read)
    }

    /// A region the peer may no longer name.
    pub fn remove(&mut self, number: u32) -> Option<M> {
        let i = self.regions.iter().position(|(n, _)| *n == number)?;
        Some(self.regions.swap_remove(i).1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KIB: usize = 1 << 10;

    /// A pool and a table over the same two regions, as the two ends of a
    /// channel have them: 64 KiB is number 1, 256 KiB number 2.
    fn ends() -> (Pool<Arc<HeapMemory>>, Table<Arc<HeapMemory>>) {
        let mut pool = Pool::new(1 << 20);
        let mut table = Table::new();
        for (number, len) in [(1, 64 * KIB), (2, 256 * KIB)] {
            let memory = Arc::new(HeapMemory::new(len));
            pool.add(number, memory.clone()).unwrap();
            assert!(table.insert(number, memory));
        }
        (pool, table)
    }

    fn pattern(len: usize, salt: u8) -> Vec<u8> {
        (0..len)
            .map(|i| (i as u8).wrapping_mul(31).wrapping_add(salt))
            .collect()
    }

    #[test]
    fn a_region_is_made_at_the_smallest_class_that_holds_the_bytes() {
        assert_eq!(class_len(0), Some(64 * KIB));
        assert_eq!(class_len(64 * KIB), Some(64 * KIB));
        assert_eq!(class_len(64 * KIB + 1), Some(128 * KIB));
        assert_eq!(class_len(3 << 20), Some(4 << 20));
        assert_eq!(class_len(MAX_REGION_LEN), Some(MAX_REGION_LEN));
        assert_eq!(class_len(MAX_REGION_LEN + 1), None);
    }

    #[test]
    fn what_is_put_in_a_region_is_what_the_other_end_reads() {
        let (mut pool, mut table) = ends();
        let bytes = pattern(100 * KIB, 7);
        let number = pool.put(&bytes).unwrap();
        let mut read = vec![0xee];
        table.read(number, bytes.len() as u32, &mut read).unwrap();
        assert_eq!(read[0], 0xee, "what the vector had stays");
        assert_eq!(&read[1..], &bytes[..]);
    }

    #[test]
    fn bytes_go_in_the_shortest_free_region_that_holds_them() {
        let (mut pool, _) = ends();
        assert_eq!(pool.put(&pattern(10 * KIB, 0)), Some(1));
        // Number 1 is out: the next piece, however short, goes in 2.
        assert_eq!(pool.put(&pattern(KIB, 0)), Some(2));
        assert_eq!(pool.put(&pattern(KIB, 0)), None);
        let (mut pool, _) = ends();
        assert_eq!(pool.put(&pattern(64 * KIB + 1, 0)), Some(2));
        assert_eq!(pool.put(&pattern(64 * KIB + 1, 0)), None);
        assert_eq!(pool.put(&pattern(64 * KIB, 0)), Some(1));
        // Whatever order they joined in: the longest first, the shortest
        // in the middle.
        let mut pool = Pool::new(1 << 20);
        for (number, len) in [(1, 256 * KIB), (2, 64 * KIB), (3, 128 * KIB)] {
            pool.add(number, HeapMemory::new(len)).unwrap();
        }
        assert_eq!(pool.put(&pattern(KIB, 0)), Some(2));
        assert_eq!(pool.put(&pattern(KIB, 0)), Some(3));
        assert_eq!(pool.put(&pattern(KIB, 0)), Some(1));
    }

    #[test]
    fn a_region_out_isnt_written_until_the_peer_read_it() {
        let (mut pool, mut table) = ends();
        let first = pattern(200 * KIB, 1);
        assert_eq!(pool.put(&first), Some(2));
        assert_eq!(
            pool.put(&pattern(200 * KIB, 2)),
            None,
            "the one region that long is out"
        );
        let mut read = Vec::new();
        table.read(2, first.len() as u32, &mut read).unwrap();
        assert_eq!(read, first, "the refused bytes didn't touch it");
        assert_eq!(table.take_read(), [2]);
        assert_eq!(table.take_read(), [0u32; 0]);
        assert_eq!(pool.released(2), Ok(()));
        assert_eq!(pool.put(&pattern(200 * KIB, 2)), Some(2));
    }

    #[test]
    fn giving_back_what_isnt_out_is_a_bad_message() {
        let (mut pool, _) = ends();
        assert_eq!(
            pool.released(2),
            Err(BadMessage::RegionNotOut { number: 2 })
        );
        assert_eq!(
            pool.released(9),
            Err(BadMessage::UnknownRegion { number: 9 })
        );
        assert_eq!(pool.put(&pattern(KIB, 0)), Some(1));
        assert_eq!(pool.released(1), Ok(()));
        assert_eq!(
            pool.released(1),
            Err(BadMessage::RegionNotOut { number: 1 }),
            "given back twice"
        );
        // None of it freed or took anything.
        assert_eq!(
            pool.free().collect::<Vec<_>>(),
            [(1, 64 * KIB), (2, 256 * KIB)]
        );
    }

    #[test]
    fn a_taken_region_is_its_takers_until_given_back() {
        let (mut pool, mut table) = ends();
        let number = pool.take(256 * KIB).unwrap();
        assert_eq!(number, 2);
        assert_eq!(pool.take(256 * KIB), None);
        let memory = pool.memory(number).unwrap();
        assert_eq!(memory.len(), 256 * KIB);
        // SAFETY: the region is 256 KiB and writable; nothing else reads
        // or writes it during the write.
        unsafe { memory.ptr().add(256 * KIB - 1).write(0x5a) };
        let mut read = Vec::new();
        table.read(number, (256 * KIB) as u32, &mut read).unwrap();
        assert_eq!(read.last(), Some(&0x5a));
        assert!(pool.give_back(number));
        assert!(!pool.give_back(number));
        assert!(!pool.give_back(77));
        assert_eq!(pool.take(256 * KIB), Some(2));
        assert!(pool.memory(77).is_none());
    }

    #[test]
    fn a_region_its_taker_holds_is_the_takers_alone_until_it_is_lent() {
        let (mut pool, _) = ends();
        let number = pool.take(100 * KIB).unwrap();
        assert_eq!(number, 2);
        // No message named it: the peer saying it read it frees nothing
        // (the next taker would write where this one does).
        assert_eq!(
            pool.released(2),
            Err(BadMessage::RegionNotOut { number: 2 })
        );
        assert_eq!(pool.take(100 * KIB), None);
        assert!(pool.remove(2).is_none());
        // Lent to a message, once: from then the peer's to say it read.
        assert!(pool.lend(2));
        assert!(!pool.lend(2));
        assert_eq!(pool.take(100 * KIB), None, "still out");
        assert_eq!(pool.released(2), Ok(()));
        assert!(!pool.lend(2), "free");
        assert!(!pool.lend(1), "never taken");
        assert!(!pool.lend(77));
        // One bytes were put in has no taker.
        assert_eq!(pool.put(&[1]), Some(1));
        assert!(!pool.lend(1));
        assert_eq!(pool.released(1), Ok(()));
        // Given back by its taker, it is as any free one: whoever it goes
        // to next isn't held to the first taker's hold.
        assert_eq!(pool.take(1), Some(1));
        assert!(pool.give_back(1));
        assert!(!pool.lend(1));
        assert_eq!(pool.put(&[1]), Some(1));
        assert_eq!(pool.released(1), Ok(()));
    }

    #[test]
    fn a_takers_hold_stays_its_regions_when_another_leaves() {
        let mut pool = Pool::new(1 << 20);
        for number in 1..=3 {
            pool.add(number, HeapMemory::new(64 * KIB)).unwrap();
        }
        assert_eq!(pool.put(&[1]), Some(1));
        assert_eq!(pool.put(&[2]), Some(2));
        assert_eq!(pool.take(1), Some(3));
        assert_eq!(pool.released(1), Ok(()));
        // 1 leaves; 3 takes its place in the vectors, held still, and 2
        // is not.
        assert!(pool.remove(1).is_some());
        assert_eq!(
            pool.released(3),
            Err(BadMessage::RegionNotOut { number: 3 })
        );
        assert_eq!(pool.released(2), Ok(()));
        assert!(!pool.lend(2));
        assert!(pool.lend(3));
        assert_eq!(pool.released(3), Ok(()));
        // A region that joins after isn't held by what was.
        assert_eq!(pool.take(1), Some(3));
        assert!(pool.remove(2).is_some());
        pool.add(4, HeapMemory::new(64 * KIB)).unwrap();
        assert!(!pool.lend(4));
        assert!(pool.lend(3));
    }

    #[test]
    fn a_takers_slice_is_the_bytes_the_other_end_reads() {
        let (mut pool, table) = ends();
        let number = pool.take(100 * KIB).unwrap();
        let memory = pool.memory(number).unwrap();
        // SAFETY: the region is out to this test alone, writable, and
        // longer than 100 KiB; nothing reads it while the slices live.
        let written = unsafe {
            let slice = bytes_mut(memory, 100 * KIB);
            assert_eq!(slice.len(), 100 * KIB);
            slice.copy_from_slice(&pattern(100 * KIB, 4));
            bytes(memory, 100 * KIB).to_vec()
        };
        assert_eq!(written, pattern(100 * KIB, 4));

        // What a message names is checked, and not read.
        let named = table.named(number, (100 * KIB) as u32).unwrap();
        assert_eq!(named.len(), 256 * KIB);
        assert_eq!(
            table.named(number, (256 * KIB + 1) as u32).err(),
            Some(BadMessage::OutsideRegion {
                number,
                len: (256 * KIB + 1) as u32
            })
        );
        assert_eq!(
            table.named(9, 1).err(),
            Some(BadMessage::UnknownRegion { number: 9 })
        );
        // Any part of it copies out; nothing that isn't all inside does.
        let mut part = [0; 7];
        assert!(copy_out(named, 100 * KIB - 7, &mut part));
        assert_eq!(part, written[100 * KIB - 7..]);
        assert!(copy_out(named, 256 * KIB - 7, &mut part));
        assert_eq!(part, [0; 7], "never written");
        part = [5; 7];
        assert!(!copy_out(named, 256 * KIB - 6, &mut part));
        assert!(!copy_out(named, usize::MAX, &mut part));
        assert_eq!(part, [5; 7]);
        assert!(copy_out(named, 256 * KIB, &mut []));
        assert!(!copy_out(named, 256 * KIB + 1, &mut []));
        // By value, a run at a time.
        let runs: Vec<[u8; 3]> = chunks_out(named, 1, 5).unwrap().collect();
        assert_eq!(runs.as_flattened(), &written[1..16]);
        let mut last = chunks_out::<_, 4>(named, 256 * KIB - 8, 2).unwrap();
        assert_eq!((last.len(), last.next()), (2, Some([0; 4])));
        assert_eq!((last.len(), last.next()), (1, Some([0; 4])));
        assert_eq!((last.len(), last.next()), (0, None));
        assert_eq!(last.next(), None);
        assert!(chunks_out::<_, 4>(named, 256 * KIB - 7, 2).is_none());
        assert!(chunks_out::<_, 4>(named, 0, usize::MAX).is_none());
        assert!(chunks_out::<_, 4>(named, usize::MAX, 1).is_none());
        assert_eq!(chunks_out::<_, 4>(named, 256 * KIB, 0).unwrap().count(), 0);
    }

    #[test]
    fn a_region_that_isnt_the_peers_or_is_shorter_isnt_read() {
        let (_, mut table) = ends();
        let mut read = vec![1, 2, 3];
        assert_eq!(
            table.read(3, 1, &mut read),
            Err(BadMessage::UnknownRegion { number: 3 })
        );
        let over = (64 * KIB + 1) as u32;
        assert_eq!(
            table.read(1, over, &mut read),
            Err(BadMessage::OutsideRegion {
                number: 1,
                len: over
            })
        );
        assert_eq!(
            table.read(1, u32::MAX, &mut read),
            Err(BadMessage::OutsideRegion {
                number: 1,
                len: u32::MAX
            })
        );
        assert_eq!(read, [1, 2, 3]);
        assert!(
            table.take_read().is_empty(),
            "nothing was read, nothing to give back"
        );
        // All of a region, and none of it, are in it.
        assert_eq!(table.read(1, (64 * KIB) as u32, &mut read), Ok(()));
        assert_eq!(read.len(), 3 + 64 * KIB);
        assert_eq!(table.read(1, 0, &mut read), Ok(()));
        assert_eq!(table.take_read(), [1, 1]);
    }

    #[test]
    fn a_region_removed_from_the_table_cant_be_named() {
        let (_, mut table) = ends();
        assert!(table.remove(1).is_some());
        assert!(table.remove(1).is_none());
        assert_eq!(
            table.read(1, 1, &mut Vec::new()),
            Err(BadMessage::UnknownRegion { number: 1 })
        );
        assert_eq!(table.read(2, 1, &mut Vec::new()), Ok(()));
        assert!(
            !table.insert(2, Arc::new(HeapMemory::new(64 * KIB))),
            "the number is taken"
        );
        assert_eq!(
            table.read(2, (256 * KIB) as u32, &mut Vec::new()),
            Ok(()),
            "and still the first one's"
        );
        assert!(table.insert(1, Arc::new(HeapMemory::new(64 * KIB))));
    }

    #[test]
    fn a_pool_takes_only_regions_it_may() {
        let mut pool = Pool::new(200 * KIB);
        assert_eq!(pool.add(1, HeapMemory::new(64 * KIB)), Ok(()));
        assert_eq!(
            pool.add(1, HeapMemory::new(64 * KIB)),
            Err(AddError::Duplicate)
        );
        assert_eq!(
            pool.add(2, HeapMemory::new(64 * KIB + 1)),
            Err(AddError::NotAClass)
        );
        assert_eq!(
            pool.add(2, HeapMemory::new(32 * KIB)),
            Err(AddError::NotAClass)
        );
        assert_eq!(
            pool.add(2, HeapMemory::new(256 * KIB)),
            Err(AddError::OverLimit)
        );
        assert_eq!(pool.total(), 64 * KIB);
        assert_eq!(pool.add(2, HeapMemory::new(128 * KIB)), Ok(()));
        assert_eq!(pool.total(), 192 * KIB);
        assert_eq!(
            pool.add(3, HeapMemory::new(64 * KIB)),
            Err(AddError::OverLimit)
        );
        // Exactly the limit is under it.
        let mut pool = Pool::new(128 * KIB);
        assert_eq!(pool.add(1, HeapMemory::new(64 * KIB)), Ok(()));
        assert_eq!(pool.add(2, HeapMemory::new(64 * KIB)), Ok(()));
    }

    #[test]
    fn only_a_free_region_leaves_the_pool() {
        let (mut pool, _) = ends();
        assert_eq!(pool.put(&pattern(KIB, 0)), Some(1));
        assert!(pool.remove(1).is_none(), "the peer has yet to read it");
        assert!(pool.remove(5).is_none());
        assert_eq!(pool.free().collect::<Vec<_>>(), [(2, 256 * KIB)]);
        assert_eq!(pool.remove(2).map(|m| m.len()), Some(256 * KIB));
        assert_eq!(pool.total(), 64 * KIB);
        assert_eq!(pool.free().count(), 0);
        // What is left is still found by its number.
        assert_eq!(pool.released(1), Ok(()));
        assert_eq!(pool.remove(1).map(|m| m.len()), Some(64 * KIB));
        assert_eq!(pool.total(), 0);
        assert_eq!(pool.put(&pattern(KIB, 0)), None);
    }

    #[test]
    fn removing_a_region_keeps_the_others_numbers_and_states() {
        let mut pool = Pool::new(1 << 20);
        for number in 1..=3 {
            pool.add(number, HeapMemory::new(64 * KIB)).unwrap();
        }
        assert_eq!(pool.put(&[1]), Some(1));
        assert_eq!(pool.put(&[2]), Some(2));
        assert_eq!(pool.released(1), Ok(()));
        // 1 leaves; 3 takes its place in the vectors, 2 is still out.
        assert!(pool.remove(1).is_some());
        assert_eq!(pool.free().collect::<Vec<_>>(), [(3, 64 * KIB)]);
        assert_eq!(pool.put(&[3]), Some(3));
        assert_eq!(pool.put(&[4]), None);
        assert_eq!(pool.released(2), Ok(()));
        assert_eq!(pool.released(3), Ok(()));
    }
}
