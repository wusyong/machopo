//! A platform within one process: the two ends of a channel on two
//! threads, what one writes handed to the other as it is, and regions
//! that are this process's own memory. It is what the endpoint's tests and
//! fuzzer run on (under Miri too), with nothing of an operating system's
//! under it.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use crate::core::channel::{Transport, WriteError};
use crate::core::region::{HeapMemory, Memory};
use crate::endpoint::{Event, Key, Platform, Waited, Waiter, Wake};

/// A region, as a handle and as a mapping: the memory itself, shared.
pub type Region = Arc<HeapMemory>;

/// What one end has waiting for it.
#[derive(Default)]
struct Side {
    /// What the peer wrote, a write at a time.
    packets: VecDeque<(Vec<u8>, Vec<Region>)>,
    /// Their bytes.
    waiting: usize,
    /// The peer's end was shut or dropped.
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
    /// Called when there is something new for this end: what wakes the
    /// thread that waits on it.
    arrived: Option<Wake>,
}

/// Both ends', behind one lock: each end's writes touch the other's.
struct Shared {
    sides: Mutex<[Side; 2]>,
    /// How many bytes may wait unread at an end before a write to it is
    /// refused; `None` for any number.
    room: Option<usize>,
}

/// One end of a channel within a process, as whoever reaches it: the
/// channel's own value, or a waiter's.
#[derive(Clone)]
struct End {
    shared: Arc<Shared>,
    side: usize,
}

/// One end of a channel within a process. Dropped, its peer is told it is
/// gone.
pub struct Local(End);

/// The two ends of a channel. `room` is how many bytes may wait unread
/// at an end before a write to it is refused (`None`: any number); a
/// write to an end with nothing waiting is always taken.
pub fn pair(room: Option<usize>) -> (Local, Local) {
    let shared = Arc::new(Shared {
        sides: Mutex::new([Side::default(), Side::default()]),
        room,
    });
    let end = |side| {
        Local(End {
            shared: shared.clone(),
            side,
        })
    };
    (end(0), end(1))
}

impl End {
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

    /// What there is for this end to do, if anything, and whom to tell
    /// of what taking it changed.
    fn next(
        &self,
        bytes: &mut Vec<u8>,
        handles: &mut Vec<Region>,
    ) -> (Option<Event>, Option<Wake>) {
        let mut sides = self.sides();
        let (mine, theirs) = self.split(&mut sides);
        if let Some((packet, with)) = mine.packets.pop_front() {
            mine.waiting = mine.waiting.saturating_sub(packet.len());
            bytes.extend_from_slice(&packet);
            handles.extend(with);
            let tell = if std::mem::take(&mut mine.writer_waits) {
                theirs.writable = true;
                theirs.arrived.clone()
            } else {
                None
            };
            return (Some(Event::Read), tell);
        }
        if std::mem::take(&mut mine.writable) {
            return (Some(Event::Writable), None);
        }
        (mine.peer_gone.then_some(Event::PeerGone), None)
    }

    fn shut(&self) {
        let mut sides = self.sides();
        let theirs = self.split(&mut sides).1;
        theirs.peer_gone = true;
        let tell = theirs.arrived.clone();
        drop(sides);
        // Not under the lock: it wakes a thread that takes it.
        if let Some(tell) = tell {
            tell();
        }
    }
}

impl Local {
    /// The bytes this end wrote, in all.
    pub fn written(&self) -> usize {
        let mut sides = self.0.sides();
        self.0.split(&mut sides).0.written
    }

    /// The regions this end made, in all.
    pub fn created(&self) -> usize {
        let mut sides = self.0.sides();
        self.0.split(&mut sides).0.created
    }

    /// What there is for this end to do, if anything, without waiting:
    /// for a test that turns an endpoint by hand.
    pub fn poll(&self, bytes: &mut Vec<u8>, handles: &mut Vec<Region>) -> Option<Event> {
        let (event, tell) = self.0.next(bytes, handles);
        if let Some(tell) = tell {
            tell();
        }
        event
    }
}

impl Transport for Local {
    type Handle = Region;

    fn write(&self, bytes: &[u8], handles: &mut Vec<Region>) -> Result<usize, WriteError> {
        let mut sides = self.0.sides();
        let (mine, theirs) = self.0.split(&mut sides);
        if mine.peer_gone {
            return Err(WriteError::Closed);
        }
        let after = theirs.waiting.saturating_add(bytes.len());
        if theirs.waiting > 0 && self.0.shared.room.is_some_and(|room| after > room) {
            theirs.writer_waits = true;
            return Err(WriteError::WouldBlock);
        }
        theirs.waiting = after;
        mine.written = mine.written.saturating_add(bytes.len());
        theirs
            .packets
            .push_back((bytes.to_vec(), std::mem::take(handles)));
        let tell = theirs.arrived.clone();
        drop(sides);
        if let Some(tell) = tell {
            tell();
        }
        Ok(bytes.len())
    }
}

impl Platform for Local {
    type Memory = Region;
    type Waiter = LocalWaiter;

    fn map(&self, handle: Region, len: usize, _writable: bool) -> Option<Region> {
        (handle.len() == len).then_some(handle)
    }

    fn create(&self, len: usize, _peer_writes: bool) -> Option<(Region, Region)> {
        let mut sides = self.0.sides();
        let mine = self.0.split(&mut sides).0;
        mine.created = mine.created.saturating_add(1);
        drop(sides);
        let region = Arc::new(HeapMemory::new(len));
        Some((region.clone(), region))
    }

    fn shut(&self) {
        self.0.shut();
    }
}

impl Drop for Local {
    fn drop(&mut self) {
        self.0.shut();
    }
}

/// What a waiter's thread sleeps on: set by whatever has something for
/// it.
#[derive(Default)]
struct Signal {
    /// Something arrived at one of the ends waited on.
    arrived: bool,
    /// The interrupter was called.
    interrupted: bool,
}

/// What an IO thread waits on [`Local`] ends with.
pub struct LocalWaiter {
    ends: Vec<(Key, End)>,
    signal: Arc<(Mutex<Signal>, Condvar)>,
    /// Where the last wait found something: the next begins after it, so
    /// one busy channel doesn't keep the others waiting.
    after: usize,
}

fn signal(signal: &(Mutex<Signal>, Condvar), set: impl FnOnce(&mut Signal)) {
    set(&mut signal
        .0
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()));
    signal.1.notify_all();
}

impl Waiter<Local> for LocalWaiter {
    fn new() -> Option<Self> {
        Some(Self {
            ends: Vec::new(),
            signal: Arc::default(),
            after: 0,
        })
    }

    fn interrupter(&self) -> Wake {
        let shared = self.signal.clone();
        Arc::new(move || signal(&shared, |signal| signal.interrupted = true))
    }

    fn add(&mut self, key: Key, channel: &Local) -> bool {
        let end = channel.0.clone();
        let shared = self.signal.clone();
        let arrived: Wake = Arc::new(move || signal(&shared, |signal| signal.arrived = true));
        let mut sides = end.sides();
        end.split(&mut sides).0.arrived = Some(arrived);
        drop(sides);
        self.ends.push((key, end));
        // What arrived before it was waited on is there to find.
        signal(&self.signal, |signal| signal.arrived = true);
        true
    }

    fn remove(&mut self, key: Key) {
        self.ends.retain(|(served, end)| {
            if *served == key {
                let mut sides = end.sides();
                end.split(&mut sides).0.arrived = None;
            }
            *served != key
        });
    }

    fn wait<R>(&mut self, then: impl FnOnce(Waited<'_, Region>) -> R) -> R {
        let mut bytes = Vec::new();
        let mut handles = Vec::new();
        loop {
            {
                let mut signal = self
                    .signal
                    .0
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                while !signal.interrupted && !signal.arrived {
                    signal = self
                        .signal
                        .1
                        .wait(signal)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                }
                if std::mem::take(&mut signal.interrupted) {
                    return then(Waited::Interrupted);
                }
                // Cleared before the ends are looked at: what arrives
                // while looking sets it again.
                signal.arrived = false;
            }
            let count = self.ends.len();
            for step in 0..count {
                let at = self
                    .after
                    .wrapping_add(step)
                    .checked_rem(count)
                    .unwrap_or(0);
                let Some((key, end)) = self.ends.get(at) else {
                    continue;
                };
                let (event, tell) = end.next(&mut bytes, &mut handles);
                if let Some(tell) = tell {
                    tell();
                }
                if let Some(event) = event {
                    self.after = at.wrapping_add(1);
                    // There may be more, here or at another end.
                    signal(&self.signal, |signal| signal.arrived = true);
                    return then(Waited::Channel {
                        key: *key,
                        event,
                        bytes: &bytes,
                        handles: &mut handles,
                    });
                }
            }
        }
    }
}
