//! A data pipe across the channel (the place of Mojo's data pipe, which is
//! a ring in shared memory; here its bytes are messages): the writer's end
//! sends the pipe's bytes a piece at a time, never more than the pipe's
//! capacity past what the reader said it read, and the reader says so
//! each time it has read half the capacity. So a reader that stops
//! reading stops its writer, with no state the two share but the messages
//! ([`Control`](super::control::Control)'s `Pipe*`).
//!
//! A pipe is a number its writer's end gave it, each higher than the
//! last and none used twice. That is how either end tells a message for a
//! pipe that is over (the reader cancelled while bytes were on their way,
//! the writer ended while a report was: let go) from one for a pipe that
//! never was (a bad message).

use super::credit;
use super::frame::BadMessage;

/// The most of a pipe's bytes in one message: past this a message costs
/// more to copy than a second one costs to send, and what is behind it on
/// the channel (input, frames) waits longer.
pub const MAX_PIPE_CHUNK: usize = 64 << 10;

/// What a pipe's writer may send: its capacity, less what it sent that
/// the reader hasn't said it read.
#[derive(Debug)]
struct Window {
    capacity: u64,
    unread: u64,
}

/// The pipes this end writes (the sending half): their numbers and their
/// credit.
#[derive(Debug, Default)]
pub struct Outgoing {
    /// The next pipe's number: one past the highest given out.
    next: u32,
    open: Vec<(u32, Window)>,
}

impl Outgoing {
    pub fn new() -> Self {
        Self::default()
    }

    fn window(&mut self, pipe: u32) -> Option<&mut Window> {
        self.open
            .iter_mut()
            .find(|(number, _)| *number == pipe)
            .map(|(_, window)| window)
    }

    /// A new pipe whose reader holds at most `capacity` unread bytes (at
    /// least one): its number, for the message that opens it. `None` when
    /// the numbers are spent.
    pub fn open(&mut self, capacity: u32) -> Option<u32> {
        let pipe = self.next;
        // The last number there is opens no pipe: nothing is past it for
        // `next` to be.
        self.next = pipe.checked_add(1)?;
        let window = Window {
            capacity: u64::from(capacity.max(1)),
            unread: 0,
        };
        self.open.push((pipe, window));
        Some(pipe)
    }

    /// How many of the pipe's bytes may be sent now, in one message: none
    /// for a pipe that is over.
    pub fn room(&mut self, pipe: u32) -> usize {
        let room = self
            .window(pipe)
            .map_or(0, |window| credit::room(window.capacity, window.unread));
        usize::try_from(room).map_or(MAX_PIPE_CHUNK, |room| room.min(MAX_PIPE_CHUNK))
    }

    /// `len` of the pipe's bytes are being sent. `false`, and nothing
    /// counted, when that is more than there is room for: they aren't to
    /// be sent.
    pub fn sent(&mut self, pipe: u32, len: usize) -> bool {
        let Some(window) = self.window(pipe) else {
            return false;
        };
        let after = u64::try_from(len)
            .ok()
            .and_then(|len| credit::sent(window.capacity, window.unread, len));
        match after {
            Some(unread) => {
                window.unread = unread;
                true
            }
            None => false,
        }
    }

    /// The writer is done with the pipe, or lets it go. `false` when it
    /// was over already.
    pub fn end(&mut self, pipe: u32) -> bool {
        let before = self.open.len();
        self.open.retain(|(number, _)| *number != pipe);
        self.open.len() != before
    }

    /// A message about a pipe this end never opened is a bad one.
    fn known(&self, pipe: u32) -> Result<(), BadMessage> {
        if pipe < self.next {
            Ok(())
        } else {
            Err(BadMessage::UnknownPipe { pipe })
        }
    }

    /// The peer said it read `count` more of the pipe's bytes. More than
    /// it was sent is a bad message; a report for a pipe that is over is
    /// let go (it crossed the pipe's end).
    pub fn peer_read(&mut self, pipe: u32, count: u64) -> Result<(), BadMessage> {
        self.known(pipe)?;
        let Some(window) = self.window(pipe) else {
            return Ok(());
        };
        match credit::read(window.unread, count) {
            Some(unread) => {
                window.unread = unread;
                Ok(())
            }
            None => Err(BadMessage::PipeOverRead { pipe }),
        }
    }

    /// The peer wants no more of the pipe: whether it was still open (its
    /// writer is to stop).
    pub fn peer_cancelled(&mut self, pipe: u32) -> Result<bool, BadMessage> {
        self.known(pipe)?;
        Ok(self.end(pipe))
    }

    /// The pipes still open.
    pub fn open_pipes(&self) -> impl Iterator<Item = u32> {
        self.open.iter().map(|(pipe, _)| *pipe)
    }
}

/// When a pipe's reader says how much it read: once what it hasn't said
/// reaches half the capacity.
#[derive(Debug)]
pub struct ReadReport {
    threshold: u64,
    unreported: u64,
}

impl ReadReport {
    /// For a pipe of `capacity` bytes.
    pub fn new(capacity: u32) -> Self {
        Self {
            threshold: credit::threshold(u64::from(capacity.max(1))),
            unreported: 0,
        }
    }

    /// `count` more bytes were read: the count to report now, if any.
    pub fn read(&mut self, count: usize) -> Option<u64> {
        let count = u64::try_from(count).unwrap_or(u64::MAX);
        let (unreported, report) = credit::report(self.threshold, self.unreported, count);
        self.unreported = unreported;
        report
    }
}

/// The pipes the peer writes (the receiving half), each with what this
/// end keeps for it (`P`: where its bytes go).
#[derive(Debug)]
pub struct Incoming<P> {
    /// One past the highest number opened.
    next: u32,
    open: Vec<(u32, P)>,
}

impl<P> Default for Incoming<P> {
    fn default() -> Self {
        Self {
            next: 0,
            open: Vec::new(),
        }
    }
}

impl<P> Incoming<P> {
    pub fn new() -> Self {
        Self::default()
    }

    /// The peer opened a pipe. Its number must be higher than every one
    /// before: a number used again would make a pipe that is over one
    /// that isn't.
    pub fn opened(&mut self, pipe: u32, with: P) -> Result<(), BadMessage> {
        if pipe < self.next {
            return Err(BadMessage::PipeReopened { pipe });
        }
        // The last number there is opens no pipe: nothing is past it for
        // `next` to be.
        self.next = pipe
            .checked_add(1)
            .ok_or(BadMessage::PipeReopened { pipe })?;
        self.open.push((pipe, with));
        Ok(())
    }

    /// What is kept for a pipe the peer sent bytes of. `None` for a pipe
    /// that is over (this end cancelled it while they were on their way):
    /// they are let go. A pipe never opened is a bad message.
    pub fn get(&mut self, pipe: u32) -> Result<Option<&mut P>, BadMessage> {
        if pipe >= self.next {
            return Err(BadMessage::UnknownPipe { pipe });
        }
        Ok(self
            .open
            .iter_mut()
            .find(|(number, _)| *number == pipe)
            .map(|(_, with)| with))
    }

    /// The peer ended the pipe: what was kept for it, unless it was over
    /// already.
    pub fn ended(&mut self, pipe: u32) -> Result<Option<P>, BadMessage> {
        if pipe >= self.next {
            return Err(BadMessage::UnknownPipe { pipe });
        }
        Ok(self.cancel(pipe))
    }

    /// This end wants no more of the pipe: what was kept for it, unless
    /// it was over already.
    pub fn cancel(&mut self, pipe: u32) -> Option<P> {
        let i = self.open.iter().position(|(number, _)| *number == pipe)?;
        Some(self.open.swap_remove(i).1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_writer_sends_no_more_than_the_reader_has_room_for() {
        let mut pipes = Outgoing::new();
        let pipe = pipes.open(100).unwrap();
        assert_eq!(pipes.room(pipe), 100);
        assert!(pipes.sent(pipe, 60));
        assert_eq!(pipes.room(pipe), 40);
        assert!(!pipes.sent(pipe, 41), "one more than there is room for");
        assert_eq!(pipes.room(pipe), 40, "and nothing was counted");
        assert!(pipes.sent(pipe, 40));
        assert_eq!(pipes.room(pipe), 0);
        assert!(!pipes.sent(pipe, 1));
        assert!(pipes.sent(pipe, 0));
        // The reader read some: that much room again.
        assert_eq!(pipes.peer_read(pipe, 30), Ok(()));
        assert_eq!(pipes.room(pipe), 30);
        assert_eq!(pipes.peer_read(pipe, 70), Ok(()));
        assert_eq!(pipes.room(pipe), 100);
    }

    #[test]
    fn a_report_of_more_than_was_sent_is_a_bad_message() {
        let mut pipes = Outgoing::new();
        let pipe = pipes.open(100).unwrap();
        assert_eq!(
            pipes.peer_read(pipe, 1),
            Err(BadMessage::PipeOverRead { pipe })
        );
        assert!(pipes.sent(pipe, 50));
        assert_eq!(
            pipes.peer_read(pipe, 51),
            Err(BadMessage::PipeOverRead { pipe })
        );
        assert_eq!(
            pipes.peer_read(pipe, u64::MAX),
            Err(BadMessage::PipeOverRead { pipe })
        );
        assert_eq!(pipes.room(pipe), 50, "none of it gave room");
        assert_eq!(pipes.peer_read(pipe, 50), Ok(()));
        assert_eq!(pipes.peer_read(pipe, 0), Ok(()));
    }

    #[test]
    fn one_message_holds_at_most_a_chunk() {
        let mut pipes = Outgoing::new();
        let pipe = pipes.open(512 << 10).unwrap();
        assert_eq!(pipes.room(pipe), MAX_PIPE_CHUNK);
        for _ in 0..7 {
            assert!(pipes.sent(pipe, MAX_PIPE_CHUNK));
        }
        assert_eq!(pipes.room(pipe), MAX_PIPE_CHUNK);
        assert!(pipes.sent(pipe, MAX_PIPE_CHUNK - 1));
        assert_eq!(pipes.room(pipe), 1);
        // A pipe of no capacity holds a byte.
        let small = pipes.open(0).unwrap();
        assert_eq!(pipes.room(small), 1);
        // The largest: its room is still a chunk.
        let large = pipes.open(u32::MAX).unwrap();
        assert_eq!(pipes.room(large), MAX_PIPE_CHUNK);
    }

    #[test]
    fn pipes_have_numbers_of_their_own_and_credit_of_their_own() {
        let mut pipes = Outgoing::new();
        let first = pipes.open(10).unwrap();
        let second = pipes.open(20).unwrap();
        assert_eq!((first, second), (0, 1));
        assert!(pipes.sent(first, 10));
        assert_eq!(pipes.room(first), 0);
        assert_eq!(pipes.room(second), 20);
        assert_eq!(
            pipes.peer_read(second, 1),
            Err(BadMessage::PipeOverRead { pipe: second })
        );
        assert_eq!(pipes.open_pipes().collect::<Vec<_>>(), [0, 1]);
        assert!(pipes.end(first));
        assert_eq!(pipes.open_pipes().collect::<Vec<_>>(), [1]);
        assert_eq!(pipes.open(5), Some(2), "a number isn't used again");
        // Ending one in the middle leaves those on both sides of it.
        let third = pipes.open(30).unwrap();
        assert!(pipes.sent(second, 7));
        assert!(pipes.end(2));
        assert_eq!(pipes.open_pipes().collect::<Vec<_>>(), [second, third]);
        assert_eq!(pipes.room(second), 13);
        assert_eq!(pipes.room(third), 30);
    }

    #[test]
    fn a_message_for_a_pipe_that_is_over_is_let_go_and_one_never_opened_is_bad() {
        let mut pipes = Outgoing::new();
        assert_eq!(
            pipes.peer_read(0, 0),
            Err(BadMessage::UnknownPipe { pipe: 0 })
        );
        assert_eq!(
            pipes.peer_cancelled(0),
            Err(BadMessage::UnknownPipe { pipe: 0 })
        );
        let pipe = pipes.open(10).unwrap();
        assert!(pipes.sent(pipe, 10));
        assert_eq!(
            pipes.peer_read(pipe + 1, 0),
            Err(BadMessage::UnknownPipe { pipe: pipe + 1 })
        );
        assert_eq!(
            pipes.peer_cancelled(pipe + 1),
            Err(BadMessage::UnknownPipe { pipe: pipe + 1 })
        );
        // The writer ended it; the reader's report was on its way.
        assert!(pipes.end(pipe));
        assert!(!pipes.end(pipe));
        assert_eq!(pipes.peer_read(pipe, 5), Ok(()));
        assert_eq!(
            pipes.peer_read(pipe, 5000),
            Ok(()),
            "whatever it says: there is nothing to give room in"
        );
        assert_eq!(pipes.peer_cancelled(pipe), Ok(false));
        assert_eq!(pipes.room(pipe), 0);
        assert!(!pipes.sent(pipe, 1));
        assert!(!pipes.sent(pipe, 0));
        // The reader cancels one that is open: its writer is to stop.
        let pipe = pipes.open(10).unwrap();
        assert_eq!(pipes.peer_cancelled(pipe), Ok(true));
        assert_eq!(pipes.peer_cancelled(pipe), Ok(false));
        assert_eq!(pipes.room(pipe), 0);
        assert_eq!(pipes.open_pipes().count(), 0);
    }

    #[test]
    fn a_reader_reports_each_half_of_the_capacity_it_reads() {
        let mut report = ReadReport::new(100);
        assert_eq!(report.read(49), None);
        assert_eq!(
            report.read(1),
            Some(50),
            "half, with what wasn't reported before"
        );
        assert_eq!(report.read(10), None, "counted from none again");
        assert_eq!(report.read(0), None);
        assert_eq!(report.read(100), Some(110));
        assert_eq!(report.read(50), Some(50));
        // An odd capacity's half is rounded down; the smallest pipes
        // report every byte.
        let mut report = ReadReport::new(7);
        assert_eq!(report.read(2), None);
        assert_eq!(report.read(1), Some(3));
        for capacity in [0, 1, 2, 3] {
            let mut report = ReadReport::new(capacity);
            assert_eq!(report.read(1), Some(1), "capacity {capacity}");
            assert_eq!(
                report.read(0),
                None,
                "capacity {capacity}: nothing read, nothing to say"
            );
        }
    }

    #[test]
    fn a_writer_out_of_room_always_has_a_report_coming() {
        // Whatever the capacity and however the reader's reads are cut:
        // once the writer is out of room and the reader has read all it
        // was sent, what the reader reported gives room again.
        for capacity in [1u32, 2, 3, 7, 64, 1000] {
            for step in [1usize, 2, 3, 5, 64, 999] {
                let mut pipes = Outgoing::new();
                let pipe = pipes.open(capacity).unwrap();
                let mut report = ReadReport::new(capacity);
                let mut held = 0usize;
                for _ in 0..50 {
                    let room = pipes.room(pipe);
                    assert!(pipes.sent(pipe, room));
                    held += room;
                    assert!(
                        held <= capacity as usize,
                        "the reader holds more than the pipe does"
                    );
                    while held > 0 {
                        let count = step.min(held);
                        held -= count;
                        if let Some(reported) = report.read(count) {
                            pipes.peer_read(pipe, reported).unwrap();
                        }
                    }
                    assert!(
                        pipes.room(pipe) > 0,
                        "capacity {capacity}, reads of {step}: stuck"
                    );
                }
            }
        }
    }

    #[test]
    fn the_reading_end_tells_a_pipe_that_is_over_from_one_that_never_was() {
        let mut pipes = Incoming::<&str>::new();
        assert_eq!(pipes.get(0), Err(BadMessage::UnknownPipe { pipe: 0 }));
        assert_eq!(pipes.ended(0), Err(BadMessage::UnknownPipe { pipe: 0 }));
        assert_eq!(pipes.opened(3, "three"), Ok(()));
        assert_eq!(pipes.opened(5, "five"), Ok(()));
        assert_eq!(pipes.get(3), Ok(Some(&mut "three")));
        assert_eq!(pipes.get(5), Ok(Some(&mut "five")));
        // Numbers under the highest that no pipe had: over, as far as
        // this end can tell.
        assert_eq!(pipes.get(4), Ok(None));
        assert_eq!(pipes.get(6), Err(BadMessage::UnknownPipe { pipe: 6 }));
        // This end cancels: bytes on their way are let go, and so is the
        // end.
        assert_eq!(pipes.cancel(3), Some("three"));
        assert_eq!(pipes.cancel(3), None);
        assert_eq!(pipes.get(3), Ok(None));
        assert_eq!(pipes.ended(3), Ok(None));
        assert_eq!(
            pipes.get(5),
            Ok(Some(&mut "five")),
            "the other is as it was"
        );
        // The peer ends one.
        assert_eq!(pipes.ended(5), Ok(Some("five")));
        assert_eq!(pipes.ended(5), Ok(None));
        assert_eq!(pipes.get(5), Ok(None));
    }

    #[test]
    fn a_pipes_number_isnt_opened_twice() {
        let mut pipes = Incoming::<u8>::new();
        assert_eq!(pipes.opened(2, 0), Ok(()));
        assert_eq!(
            pipes.opened(2, 1),
            Err(BadMessage::PipeReopened { pipe: 2 })
        );
        assert_eq!(
            pipes.opened(1, 1),
            Err(BadMessage::PipeReopened { pipe: 1 })
        );
        assert_eq!(pipes.get(2), Ok(Some(&mut 0)), "the first is the one kept");
        assert_eq!(pipes.cancel(2), Some(0));
        assert_eq!(
            pipes.opened(2, 1),
            Err(BadMessage::PipeReopened { pipe: 2 }),
            "over isn't free"
        );
        assert_eq!(pipes.opened(3, 1), Ok(()));
        assert_eq!(
            pipes.opened(u32::MAX, 1),
            Err(BadMessage::PipeReopened { pipe: u32::MAX })
        );
        assert_eq!(pipes.opened(u32::MAX - 1, 1), Ok(()));
        assert_eq!(
            pipes.get(u32::MAX),
            Err(BadMessage::UnknownPipe { pipe: u32::MAX })
        );
    }
}
