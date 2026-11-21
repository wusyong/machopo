//! Shared memory as the core sees it (Chromium's `BigBuffer` over
//! `base::*SharedMemoryRegion`, kept and used again where Chromium makes
//! one a time): a region is a number its maker gave it and a length of
//! memory, whatever platform mapped it. A [`Pool`] is the regions this end
//! writes large data into, one piece of data to a region; a [`Table`] is
//! the regions the peer writes into, read by number and checked. What says
//! a region may be written again is a message ([`Control`]), so nothing in
//! the shared memory itself is state both ends write.
//!
//! The memory is never a `&[u8]`: the peer may be writing it. Bytes go in
//! and come out through pointers, into memory of this end's own.

use std::sync::Arc;

pub use super::fit::{MAX_REGION_LEN, MIN_REGION_LEN, class_len};
use super::fit::{Slot, admits, best_fit};
use super::frame::BadMessage;

/// The destination of the core's own messages ([`Control`]): no layer
/// above has one there.
pub const CONTROL_DESTINATION: u32 = u32::MAX;

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

/// The regions this end writes large data into (the sending half of
/// Chromium's `BigBuffer`, with the regions kept): each holds one piece of
/// data at a time, and is written again only once the peer said it had
/// read it, or whoever took it gave it back.
pub struct Pool<M> {
    /// What the search reads; `regions[i]` is slot `i`'s.
    slots: Vec<Slot>,
    regions: Vec<(u32, M)>,
    total: usize,
    limit: usize,
}

impl<M: Memory> Pool<M> {
    /// A pool that holds `limit` bytes of regions at most.
    pub const fn new(limit: usize) -> Self {
        Self {
            slots: Vec::new(),
            regions: Vec::new(),
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
        Ok(())
    }

    /// The bytes of regions held.
    pub const fn total(&self) -> usize {
        self.total
    }

    /// A free region of `len` bytes or more, the shortest there is, now
    /// out: its number. `None` when no free one is long enough.
    pub fn take(&mut self, len: usize) -> Option<u32> {
        let i = best_fit(&self.slots, len)?;
        self.slots.get_mut(i)?.out = true;
        Some(self.regions.get(i)?.0)
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
        let slot = self.index(number).and_then(|i| self.slots.get_mut(i));
        match slot {
            Some(slot) if slot.out => {
                slot.out = false;
                true
            }
            _ => false,
        }
    }

    /// The peer said it had read what was in the region: free again. A
    /// region that isn't this pool's, or that nothing is out in, is the
    /// peer's mistake.
    pub fn released(&mut self, number: u32) -> Result<(), BadMessage> {
        if self.index(number).is_none() {
            return Err(BadMessage::UnknownRegion { number });
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

    /// The first `len` bytes of region `number`, copied to the end of
    /// `into`.
    pub fn read(&mut self, number: u32, len: u32, into: &mut Vec<u8>) -> Result<(), BadMessage> {
        let Some((_, memory)) = self.regions.iter().find(|(n, _)| *n == number) else {
            return Err(BadMessage::UnknownRegion { number });
        };
        let count = match usize::try_from(len) {
            Ok(count) if count <= memory.len() => count,
            _ => return Err(BadMessage::OutsideRegion { number, len }),
        };
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

/// The core's own messages, at [`CONTROL_DESTINATION`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Control {
    /// The regions the sender has read what was in: each may be written
    /// again.
    Released(Vec<u32>),
    /// A region the receiver has from now on, its handle with the
    /// message: `writable` for one the receiver writes into (its
    /// [`Pool`]'s), otherwise one it reads (its [`Table`]'s).
    Granted {
        number: u32,
        len: u32,
        writable: bool,
    },
}

const RELEASED: u8 = 0;
const GRANTED: u8 = 1;

impl Control {
    /// The message's payload, after what `out` has.
    pub fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Self::Released(numbers) => {
                out.push(RELEASED);
                for number in numbers {
                    out.extend_from_slice(&number.to_le_bytes());
                }
            }
            Self::Granted {
                number,
                len,
                writable,
            } => {
                out.push(GRANTED);
                out.extend_from_slice(&number.to_le_bytes());
                out.extend_from_slice(&len.to_le_bytes());
                out.push(u8::from(*writable));
            }
        }
    }

    /// How many handles come with the message.
    pub const fn handles(&self) -> usize {
        match self {
            Self::Released(_) => 0,
            Self::Granted { .. } => 1,
        }
    }

    /// A payload read back: anything [`Control::encode`] doesn't write is
    /// a bad message.
    pub fn decode(payload: &[u8]) -> Result<Self, BadMessage> {
        let word = |bytes: &[u8]| {
            bytes
                .try_into()
                .map(u32::from_le_bytes)
                .map_err(|_| BadMessage::Rejected)
        };
        match payload.split_first() {
            Some((&RELEASED, rest)) => match rest.as_chunks() {
                (numbers, []) => Ok(Self::Released(
                    numbers.iter().copied().map(u32::from_le_bytes).collect(),
                )),
                _ => Err(BadMessage::Rejected),
            },
            Some((&GRANTED, rest)) => match rest.split_at_checked(8) {
                Some((words, &[writable @ (0 | 1)])) => {
                    let (number, len) = words.split_at(4);
                    Ok(Self::Granted {
                        number: word(number)?,
                        len: word(len)?,
                        writable: writable == 1,
                    })
                }
                _ => Err(BadMessage::Rejected),
            },
            _ => Err(BadMessage::Rejected),
        }
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

    #[test]
    fn the_cores_own_messages_read_back() {
        for control in [
            Control::Released(vec![]),
            Control::Released(vec![1, u32::MAX, 0x0102_0304]),
            Control::Granted {
                number: 7,
                len: 1 << 16,
                writable: true,
            },
            Control::Granted {
                number: u32::MAX,
                len: 1 << 30,
                writable: false,
            },
        ] {
            let mut payload = vec![];
            control.encode(&mut payload);
            assert_eq!(Control::decode(&payload), Ok(control.clone()));
            assert_eq!(
                control.handles(),
                usize::from(matches!(control, Control::Granted { .. }))
            );
        }
        let mut payload = vec![0xaa];
        Control::Released(vec![0x0102_0304]).encode(&mut payload);
        assert_eq!(
            payload,
            [0xaa, 0, 4, 3, 2, 1],
            "after what was there, little-endian"
        );
    }

    #[test]
    fn a_control_message_nobody_writes_is_a_bad_message() {
        let bad: [&[u8]; 9] = [
            &[],
            &[2],
            &[0, 1],
            &[0, 1, 2, 3, 4, 5],
            &[1],
            &[1, 0, 0, 0, 0, 0, 0, 0, 0],
            &[1, 0, 0, 0, 0, 0, 0, 0, 0, 2],
            &[1, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0],
            &[0xff, 0, 0, 0, 0],
        ];
        for payload in bad {
            assert_eq!(
                Control::decode(payload),
                Err(BadMessage::Rejected),
                "{payload:?}"
            );
        }
    }
}
