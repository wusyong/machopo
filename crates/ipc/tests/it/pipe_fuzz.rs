//! What the fuzzer runs on a data pipe across a channel
//! (`fuzz/fuzz_targets/pipe.rs` includes this file): a writer's end and a
//! reader's end driven by the input, the pipe's messages between them as
//! the bytes they are on a channel, in order. With a reader that keeps to
//! the protocol, the reader never holds more than the pipe's capacity,
//! what it reads is what was written, in order, and neither end takes the
//! other's messages for bad ones. With reports the input makes up, the
//! writer takes exactly those that claim no more than it sent. The tests
//! here replay `fuzz/regressions/pipe/`.

use std::collections::VecDeque;

use bytes::Bytes;
use ipc::core::control::Control;
use ipc::core::frame::BadMessage;
use ipc::core::pipe::{Incoming, MAX_PIPE_CHUNK, Outgoing, ReadReport};
use ipc::data_pipe::{self, Consumer, Producer, Read, TryWriteError};

fn encoded(control: &Control<'_>) -> Vec<u8> {
    let mut payload = Vec::new();
    control.encode(&mut payload);
    payload
}

/// The byte at `at` of what the writer has to send.
fn source(at: usize) -> u8 {
    (at as u8).wrapping_mul(13).wrapping_add((at >> 8) as u8)
}

struct Ends {
    capacity: u32,
    total: usize,
    // The writer's end.
    outgoing: Outgoing,
    pipe: u32,
    sent: usize,
    writing: bool,
    /// What the writer has sent that no report it took covers.
    unread: u64,
    // Between them, as payloads.
    to_reader: VecDeque<Vec<u8>>,
    to_writer: VecDeque<Vec<u8>>,
    // The reader's end.
    incoming: Incoming<Producer>,
    consumer: Option<Consumer>,
    report: ReadReport,
    delivered: usize,
    done: bool,
    /// The input made up a report the writer took: the reader's own
    /// reports no longer add up, so only the writer's checks hold.
    forged: bool,
}

impl Ends {
    fn new(capacity: u32, total: usize) -> Self {
        let mut outgoing = Outgoing::new();
        // A pipe before it, so its number isn't the first.
        let earlier = outgoing.open(1).unwrap();
        assert!(outgoing.end(earlier));
        let pipe = outgoing.open(capacity).unwrap();
        let mut ends = Self {
            capacity,
            total,
            outgoing,
            pipe,
            sent: 0,
            writing: true,
            unread: 0,
            to_reader: VecDeque::new(),
            to_writer: VecDeque::new(),
            incoming: Incoming::new(),
            consumer: None,
            report: ReadReport::new(capacity),
            delivered: 0,
            done: false,
            forged: false,
        };
        ends.to_reader
            .push_back(encoded(&Control::PipeOpened { pipe, capacity }));
        ends
    }

    fn room(&mut self) -> usize {
        let room = self.outgoing.room(self.pipe);
        let expected = match self.writing {
            true => (u64::from(self.capacity.max(1)) - self.unread).min(MAX_PIPE_CHUNK as u64),
            false => 0,
        };
        assert_eq!(room as u64, expected, "the writer's room");
        room
    }

    /// `false` ends the run: the channel was closed over a bad message.
    fn step(&mut self, op: u8, arg: u8) -> bool {
        match op % 7 {
            0 => {
                let len = usize::from(arg)
                    .min(self.room())
                    .min(self.total - self.sent);
                if self.writing && len > 0 {
                    assert!(self.outgoing.sent(self.pipe, len));
                    assert!(!self.outgoing.sent(self.pipe, self.capacity as usize + 1));
                    let bytes: Vec<u8> = (self.sent..self.sent + len).map(source).collect();
                    self.to_reader.push_back(encoded(&Control::PipeData {
                        pipe: self.pipe,
                        bytes: &bytes,
                    }));
                    self.sent += len;
                    self.unread += len as u64;
                }
            }
            1 => {
                if self.writing && self.sent == self.total {
                    assert!(self.outgoing.end(self.pipe));
                    self.writing = false;
                    self.to_reader
                        .push_back(encoded(&Control::PipeEnded { pipe: self.pipe }));
                }
            }
            2 => {
                let Some(payload) = self.to_reader.pop_front() else {
                    return true;
                };
                match Control::decode(&payload).unwrap() {
                    Control::PipeOpened { pipe, capacity } => {
                        let (producer, consumer) = data_pipe::create(capacity.max(1) as usize);
                        self.incoming.opened(pipe, producer).unwrap();
                        self.consumer = Some(consumer);
                    }
                    Control::PipeData { pipe, bytes } => {
                        if let Some(producer) = self.incoming.get(pipe).unwrap() {
                            match producer.try_write(Bytes::copy_from_slice(bytes)) {
                                Ok(()) => {}
                                // Only a writer given room by a made-up
                                // report sends past the capacity.
                                Err(TryWriteError::Full) => {
                                    assert!(
                                        self.forged,
                                        "the reader holds more than the pipe does"
                                    );
                                    return false;
                                }
                                Err(TryWriteError::Closed) => {
                                    panic!("a pipe still open, its consumer gone")
                                }
                            }
                        }
                    }
                    Control::PipeEnded { pipe } => drop(self.incoming.ended(pipe).unwrap()),
                    other => panic!("the writer sent {other:?}"),
                }
            }
            3 => {
                let Some(consumer) = &mut self.consumer else {
                    return true;
                };
                match consumer.read(usize::from(arg) + 1) {
                    Read::Data(bytes) => {
                        for (i, byte) in bytes.iter().enumerate() {
                            assert_eq!(
                                *byte,
                                source(self.delivered + i),
                                "byte {} of the pipe",
                                self.delivered + i
                            );
                        }
                        self.delivered += bytes.len();
                        if let Some(count) = self.report.read(bytes.len()) {
                            self.to_writer.push_back(encoded(&Control::PipeRead {
                                pipe: self.pipe,
                                count,
                            }));
                        }
                    }
                    Read::Empty => {}
                    Read::Done => {
                        assert_eq!(self.delivered, self.total, "the end came before all of it");
                        self.done = true;
                    }
                }
            }
            4 => {
                let Some(payload) = self.to_writer.pop_front() else {
                    return true;
                };
                match Control::decode(&payload).unwrap() {
                    Control::PipeRead { pipe, count } => {
                        let known = pipe <= self.pipe;
                        let fits = !self.writing || pipe != self.pipe || count <= self.unread;
                        let expected = match (known, fits) {
                            (false, _) => Err(BadMessage::UnknownPipe { pipe }),
                            (true, false) => Err(BadMessage::PipeOverRead { pipe }),
                            (true, true) => Ok(()),
                        };
                        assert_eq!(self.outgoing.peer_read(pipe, count), expected);
                        if expected.is_err() {
                            assert!(self.forged, "a reader of ours was taken for a bad one");
                            return false;
                        }
                        if self.writing && pipe == self.pipe {
                            self.unread -= count;
                        }
                    }
                    Control::PipeCancelled { pipe } => {
                        assert_eq!(
                            self.outgoing.peer_cancelled(pipe),
                            Ok(self.writing && pipe == self.pipe)
                        );
                        if pipe == self.pipe {
                            self.writing = false;
                        }
                    }
                    other => panic!("the reader sent {other:?}"),
                }
            }
            5 => {
                // The reader's user wants no more.
                if self.consumer.take().is_some() {
                    drop(self.incoming.cancel(self.pipe));
                    self.to_writer
                        .push_back(encoded(&Control::PipeCancelled { pipe: self.pipe }));
                }
            }
            _ => {
                // A report nobody read for: of this pipe, the one before,
                // or one never opened.
                let pipe = self.pipe + u32::from(arg >> 6) - 1;
                let count = u64::from(arg & 0x3f);
                self.forged = true;
                self.to_writer
                    .push_back(encoded(&Control::PipeRead { pipe, count }));
            }
        }
        self.room();
        true
    }
}

/// One input: the pipe's capacity, how much there is to send, then steps
/// (an operation and an argument each).
pub fn run(data: &[u8]) {
    let [capacity, total, steps @ ..] = data else {
        return;
    };
    let mut ends = Ends::new(u32::from(*capacity % 70), usize::from(*total) * 3);
    for step in steps.as_chunks::<2>().0 {
        if !ends.step(step[0], step[1]) {
            return;
        }
    }
    if ends.forged {
        return;
    }
    // Whatever is left, a reader that keeps to the protocol gets all of
    // it: nothing is stuck.
    if ends.consumer.is_none() {
        return;
    }
    let mut turns = 0;
    while !ends.done {
        for op in [0, 1, 2, 3, 4] {
            assert!(ends.step(op, 0xff));
        }
        turns += 1;
        assert!(
            turns < 10_000,
            "stuck with {} of {} delivered",
            ends.delivered,
            ends.total
        );
    }
    assert_eq!(ends.delivered, ends.total);
}

#[test]
fn seeds_run() {
    run(&[]);
    run(&[8]);
    for capacity in [0u8, 1, 2, 3, 8, 64, 69] {
        for total in [0u8, 1, 5, 40] {
            // Nothing but the drain; then each operation round and round
            // with a few arguments; then a cancel and a forged report at
            // each point.
            run(&[capacity, total]);
            for arg in [0u8, 1, 7, 0x40, 0x85, 0xff] {
                let mut input = vec![capacity, total];
                for round in 0..30u8 {
                    for op in [0u8, 2, 3, 4, 1] {
                        input.extend([op, arg.wrapping_add(round)]);
                    }
                    run(&input);
                    run(&[&input[..], &[5, 0, 0, arg, 2, 0, 4, 0, 2, 0]].concat());
                    run(&[&input[..], &[6, arg, 4, 0, 0, 0xff, 2, 0, 0, 0xff, 2, 0]].concat());
                }
            }
        }
    }
}

/// The fuzzer's finds, kept with their fixes.
#[test]
fn regressions_run() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fuzz/regressions/pipe");
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|extension| extension == "bin") {
            run(&std::fs::read(&path).unwrap());
        }
    }
}
