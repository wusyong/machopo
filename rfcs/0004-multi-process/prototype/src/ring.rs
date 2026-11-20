//! A single-producer, single-consumer ring of bytes in shared memory, as
//! RFC 0004's second step would have (after shmem-ipc's `ringbuf.rs`): the
//! region holds one atomic count of the bytes in the ring and a flag the
//! reader sets before it sleeps; each side keeps its own position in its
//! own memory, where the other can't reach it. All the other side can do
//! to the count is make it a wrong value in range (garbage read, or unread
//! bytes written over): every load is checked against the capacity.
//!
//! The prototype measures speed. Framing is a `u32` length before each
//! message; the checks a real one needs beyond the count's range (a
//! length limit per destination, sequence numbers) aren't here.

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use crate::shm::Mapping;

/// The count, the flag and padding: each on a cache line of its own.
const HEADER: usize = 256;
const COUNT_AT: usize = 0;
const SLEEPING_AT: usize = 128;

#[derive(Debug)]
pub struct Corrupt;

struct Shared {
    mapping: Mapping,
    capacity: usize,
}

impl Shared {
    fn new(mapping: Mapping) -> Self {
        let capacity = mapping.len() - HEADER;
        Self { mapping, capacity }
    }

    fn count(&self) -> &AtomicUsize {
        // SAFETY: inside the mapping, aligned (a mapping is page-aligned),
        // and an atomic may be shared with whoever else maps the region.
        unsafe { &*self.mapping.ptr().add(COUNT_AT).cast::<AtomicUsize>() }
    }

    fn sleeping(&self) -> &AtomicU32 {
        // SAFETY: as `count`.
        unsafe { &*self.mapping.ptr().add(SLEEPING_AT).cast::<AtomicU32>() }
    }

    /// The count, if it is one the ring can have.
    fn checked_count(&self, order: Ordering) -> Result<usize, Corrupt> {
        let count = self.count().load(order);
        if count <= self.capacity { Ok(count) } else { Err(Corrupt) }
    }

    fn data(&self) -> *mut u8 {
        // SAFETY: the header is inside the mapping.
        unsafe { self.mapping.ptr().add(HEADER) }
    }

    /// Copies `bytes` in at `pos`, wrapping.
    fn put(&self, pos: usize, bytes: &[u8]) {
        let first = bytes.len().min(self.capacity - pos);
        // SAFETY: `pos < capacity` and `bytes.len() <= capacity`, so both
        // pieces are inside the data area, which doesn't overlap `bytes`.
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), self.data().add(pos), first);
            std::ptr::copy_nonoverlapping(bytes.as_ptr().add(first), self.data(), bytes.len() - first);
        }
    }

    /// Copies `len` bytes at `pos` out to `into`, wrapping.
    fn take(&self, pos: usize, len: usize, into: *mut u8) {
        let first = len.min(self.capacity - pos);
        // SAFETY: as `put`; `into` has room for `len` bytes (the callers').
        unsafe {
            std::ptr::copy_nonoverlapping(self.data().add(pos), into, first);
            std::ptr::copy_nonoverlapping(self.data(), into.add(first), len - first);
        }
    }
}

pub struct RingWriter {
    shared: Shared,
    pos: usize,
}

impl RingWriter {
    pub fn new(mapping: Mapping) -> Self {
        Self {
            shared: Shared::new(mapping),
            pos: 0,
        }
    }

    /// Writes one message if there is room. `Ok(Some(wake))`: written, and
    /// `wake` says the reader sleeps and must be woken. `Ok(None)`: full.
    pub fn write(&mut self, payload: &[u8]) -> Result<Option<bool>, Corrupt> {
        let needed = 4 + payload.len();
        assert!(needed <= self.shared.capacity, "a message larger than the ring");
        let count = self.shared.checked_count(Ordering::Acquire)?;
        if self.shared.capacity - count < needed {
            return Ok(None);
        }
        let len = u32::try_from(payload.len()).unwrap().to_ne_bytes();
        self.shared.put(self.pos, &len);
        self.shared.put((self.pos + 4) % self.shared.capacity, payload);
        Ok(Some(self.commit(needed)))
    }

    /// Writes as many of `bytes` as fit, with no framing (a byte pipe):
    /// how many, and whether the reader must be woken.
    pub fn write_bytes(&mut self, bytes: &[u8]) -> Result<(usize, bool), Corrupt> {
        let count = self.shared.checked_count(Ordering::Acquire)?;
        let fits = bytes.len().min(self.shared.capacity - count);
        if fits == 0 {
            return Ok((0, false));
        }
        self.shared.put(self.pos, &bytes[..fits]);
        Ok((fits, self.commit(fits)))
    }

    fn commit(&mut self, written: usize) -> bool {
        self.pos = (self.pos + written) % self.shared.capacity;
        self.shared.count().fetch_add(written, Ordering::SeqCst);
        // The reader set the flag and then looked at the count again
        // (both `SeqCst`): either it saw these bytes, or we see its flag.
        // Taking the flag means one wake for one sleep.
        self.shared.sleeping().swap(0, Ordering::SeqCst) == 1
    }
}

pub struct RingReader {
    shared: Shared,
    pos: usize,
}

impl RingReader {
    pub fn new(mapping: Mapping) -> Self {
        Self {
            shared: Shared::new(mapping),
            pos: 0,
        }
    }

    /// Reads one message into `into`, if a whole one is there.
    pub fn read(&mut self, into: &mut Vec<u8>) -> Result<bool, Corrupt> {
        let count = self.shared.checked_count(Ordering::Acquire)?;
        if count < 4 {
            return Ok(false);
        }
        let mut len = [0u8; 4];
        self.shared.take(self.pos, 4, len.as_mut_ptr());
        let len = u32::from_ne_bytes(len) as usize;
        // The writer adds a whole message to the count at once: a length
        // past what is there is the writer's lie.
        if len > count - 4 {
            return Err(Corrupt);
        }
        into.clear();
        into.reserve(len);
        self.shared.take((self.pos + 4) % self.shared.capacity, len, into.as_mut_ptr());
        // SAFETY: `take` wrote `len` bytes into the reserved space.
        unsafe { into.set_len(len) };
        self.consume(4 + len);
        Ok(true)
    }

    /// Reads up to `max` bytes with no framing, appended to `into`.
    pub fn read_bytes(&mut self, into: &mut Vec<u8>, max: usize) -> Result<usize, Corrupt> {
        let count = self.shared.checked_count(Ordering::Acquire)?;
        let len = count.min(max);
        if len == 0 {
            return Ok(0);
        }
        let start = into.len();
        into.reserve(len);
        // SAFETY: room for `len` more bytes was reserved.
        self.shared.take(self.pos, len, unsafe { into.as_mut_ptr().add(start) });
        // SAFETY: `take` wrote them.
        unsafe { into.set_len(start + len) };
        self.consume(len);
        Ok(len)
    }

    fn consume(&mut self, read: usize) {
        self.pos = (self.pos + read) % self.shared.capacity;
        self.shared.count().fetch_sub(read, Ordering::SeqCst);
    }

    /// Before sleeping: sets the flag and looks again. True if the ring is
    /// still empty: sleep until the writer's wake. False: there is
    /// something to read (a wake may come all the same, to be ignored).
    pub fn prepare_sleep(&self) -> bool {
        self.shared.sleeping().store(1, Ordering::SeqCst);
        if self.shared.count().load(Ordering::SeqCst) == 0 {
            return true;
        }
        self.shared.sleeping().store(0, Ordering::SeqCst);
        false
    }
}
