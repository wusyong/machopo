//! A platform within one process: the two ends of a channel on two
//! threads, what one writes handed to the other as it is, and regions
//! that are this process's own memory. It is what the endpoint's tests and
//! fuzzer run on (under Miri too), with nothing of an operating system's
//! under it.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use crate::core::channel::{Transport, WriteError};
use crate::core::region::{HeapMemory, Memory};
use crate::endpoint::{Event, Platform};

/// A region, as a handle and as a mapping: the memory itself, shared.
pub type Region = Arc<HeapMemory>;

/// What one end has waiting for it.
#[derive(Default)]
struct Side {
    /// What the peer wrote, a write at a time.
    packets: VecDeque<(Vec<u8>, Vec<Region>)>,
    /// Their bytes.
    waiting: usize,
    /// [`Transport::wake`] was called at this end.
    woken: bool,
    /// The peer's end was dropped.
    peer_gone: bool,
    /// A write to this end was refused for want of room: the peer is told
    /// when there is some.
    writer_waits: bool,
    /// This end was refused, and there is room now.
    writable: bool,
    /// The bytes this end wrote, in all.
    written: usize,
    /// The regions this end made, in all.
    created: usize,
}

/// Both ends', behind one lock: each end's writes touch the other's.
struct Shared {
    sides: Mutex<[Side; 2]>,
    changed: Condvar,
    /// How many bytes may wait unread at an end before a write to it is
    /// refused; `None` for any number.
    room: Option<usize>,
}

/// One end of a channel within a process.
pub struct Local {
    shared: Arc<Shared>,
    side: usize,
}

/// The two ends of a channel. `room` is how many bytes may wait unread
/// at an end before a write to it is refused (`None`: any number); a
/// write to an end with nothing waiting is always taken.
pub fn pair(room: Option<usize>) -> (Local, Local) {
    let shared = Arc::new(Shared {
        sides: Mutex::new([Side::default(), Side::default()]),
        changed: Condvar::new(),
        room,
    });
    (
        Local {
            shared: shared.clone(),
            side: 0,
        },
        Local { shared, side: 1 },
    )
}

impl Local {
    fn sides(&self) -> MutexGuard<'_, [Side; 2]> {
        self.shared
            .sides
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// This end's side and the peer's.
    fn split<'a>(&self, sides: &'a mut [Side; 2]) -> (&'a mut Side, &'a mut Side) {
        let [first, second] = sides;
        if self.side == 0 {
            (first, second)
        } else {
            (second, first)
        }
    }

    /// The bytes this end wrote, in all.
    pub fn written(&self) -> usize {
        let mut sides = self.sides();
        self.split(&mut sides).0.written
    }

    /// The regions this end made, in all.
    pub fn created(&self) -> usize {
        let mut sides = self.sides();
        self.split(&mut sides).0.created
    }

    /// What there is for this end to do, if anything: [`Platform::wait`]
    /// without the wait.
    pub fn poll(&self, bytes: &mut Vec<u8>, handles: &mut Vec<Region>) -> Option<Event> {
        self.next(&mut self.sides(), bytes, handles)
    }

    fn next(
        &self,
        sides: &mut [Side; 2],
        bytes: &mut Vec<u8>,
        handles: &mut Vec<Region>,
    ) -> Option<Event> {
        let (mine, theirs) = self.split(sides);
        if std::mem::take(&mut mine.woken) {
            return Some(Event::Woken);
        }
        if let Some((packet, with)) = mine.packets.pop_front() {
            mine.waiting = mine.waiting.saturating_sub(packet.len());
            bytes.extend_from_slice(&packet);
            handles.extend(with);
            if std::mem::take(&mut mine.writer_waits) {
                theirs.writable = true;
                self.shared.changed.notify_all();
            }
            return Some(Event::Read);
        }
        if std::mem::take(&mut mine.writable) {
            return Some(Event::Writable);
        }
        mine.peer_gone.then_some(Event::PeerGone)
    }
}

impl Transport for Local {
    type Handle = Region;

    fn write(&self, bytes: &[u8], handles: &mut Vec<Region>) -> Result<usize, WriteError> {
        let mut sides = self.sides();
        let (mine, theirs) = self.split(&mut sides);
        if mine.peer_gone {
            return Err(WriteError::Closed);
        }
        let after = theirs.waiting.saturating_add(bytes.len());
        if theirs.waiting > 0 && self.shared.room.is_some_and(|room| after > room) {
            theirs.writer_waits = true;
            return Err(WriteError::WouldBlock);
        }
        theirs.waiting = after;
        mine.written = mine.written.saturating_add(bytes.len());
        theirs
            .packets
            .push_back((bytes.to_vec(), std::mem::take(handles)));
        drop(sides);
        self.shared.changed.notify_all();
        Ok(bytes.len())
    }

    fn wake(&self) {
        let mut sides = self.sides();
        self.split(&mut sides).0.woken = true;
        drop(sides);
        self.shared.changed.notify_all();
    }
}

impl Platform for Local {
    type Memory = Region;

    fn wait(&self, bytes: &mut Vec<u8>, handles: &mut Vec<Region>) -> Event {
        let mut sides = self.sides();
        loop {
            if let Some(event) = self.next(&mut sides, bytes, handles) {
                return event;
            }
            sides = self
                .shared
                .changed
                .wait(sides)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }

    fn map(&self, handle: Region, len: usize, _writable: bool) -> Option<Region> {
        (handle.len() == len).then_some(handle)
    }

    fn create(&self, len: usize, _peer_writes: bool) -> Option<(Region, Region)> {
        let mut sides = self.sides();
        let mine = self.split(&mut sides).0;
        mine.created = mine.created.saturating_add(1);
        drop(sides);
        let region = Arc::new(HeapMemory::new(len));
        Some((region.clone(), region))
    }

    fn shut(&self) {
        let mut sides = self.sides();
        self.split(&mut sides).1.peer_gone = true;
        drop(sides);
        self.shared.changed.notify_all();
    }
}

impl Drop for Local {
    fn drop(&mut self) {
        self.shut();
    }
}
