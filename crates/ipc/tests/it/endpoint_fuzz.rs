//! What the fuzzer runs on a whole endpoint
//! (`fuzz/fuzz_targets/endpoint.rs` includes this file): a host's end,
//! and a child's end that sends whatever the input says, as frames the
//! channel takes: to any destination, the core's own among them, with
//! handles or without, while the host sends large bytes, opens pipes and
//! grants regions of its own accord. Whatever comes, the host doesn't
//! panic or read outside a region (the fuzzer runs with ASan), and once
//! it says the channel is over it hands nothing more up and sends
//! nothing. The tests here replay `fuzz/regressions/endpoint/`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use bytes::Bytes;
use ipc::core::channel::{self, Gone, MAX_QUEUED_BYTES};
use ipc::core::control::CONTROL_DESTINATION;
use ipc::core::frame::BadMessage;
use ipc::core::region::{HeapMemory, MIN_REGION_LEN};
use ipc::data_pipe::{self, Consumer};
use ipc::endpoint::{self, Config, Event, Handler, Received, SendError};
use ipc::local::{self, Local};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
enum Msg {
    Text(String),
    Body(Bytes),
    Stream {
        pipe: u32,
    },
    Many(Vec<Self>),
    /// A pipe as a value: what a child writes for one is a number of no
    /// pipe of its own (it may open none).
    Piped(Consumer),
}

#[derive(Default)]
struct Counter {
    handed_up: usize,
}

impl Handler<Local> for Counter {
    fn message(&mut self, mut message: Received<'_, Local>) -> Result<(), BadMessage> {
        assert!(message.destination() < 3);
        let msg = message.decode::<Msg>()?;
        if let Msg::Stream { pipe } = msg {
            message.take_pipe(pipe).ok_or(BadMessage::Rejected)?;
        }
        self.handed_up += 1;
        Ok(())
    }
}

/// One input: steps of an operation, an argument, a length and that many
/// bytes.
pub fn run(data: &[u8]) {
    let (host_end, child_end) = local::pair(None);
    let (host, mut endpoint) = endpoint::endpoint(
        host_end,
        Config {
            destinations: 3,
            may_attach: true,
            peer_may_attach: false,
            peer_may_open_pipes: false,
            region_limit: 4 * MIN_REGION_LEN,
            peer_region_limit: 4 * MIN_REGION_LEN,
            queue_limit: MAX_QUEUED_BYTES,
        },
    );
    // The test is the host's loop: it turns the end by hand, for its
    // wakes too.
    let woken = Arc::new(AtomicBool::new(false));
    let flag = woken.clone();
    host.on_wake(Arc::new(move || flag.store(true, Ordering::SeqCst)));
    // The child's end: a bare channel, which may do whatever its input
    // says (what it may do is the host's to decide).
    let (child, _) = channel::channel(
        child_end,
        channel::Config {
            destinations: 3,
            may_attach: true,
            peer_may_attach: true,
            queue_limit: MAX_QUEUED_BYTES,
        },
    );
    let mut counter = Counter::default();
    let mut gone: Option<Gone> = None;
    let mut handed_up_when_gone = 0;
    let mut sources = Vec::new();
    let mut rest = data;
    while let [op, arg, len, after @ ..] = rest {
        let (bytes, after) = after.split_at(usize::from(*len).min(after.len()));
        rest = after;
        match op % 8 {
            // The child sends a frame: to a destination of the host's, one
            // past them, or the core's.
            0..=3 => {
                let destination = match arg % 5 {
                    4 => CONTROL_DESTINATION,
                    n => u32::from(n),
                };
                let handles = (0..arg >> 6)
                    .map(|_| Arc::new(HeapMemory::new(MIN_REGION_LEN)).into())
                    .collect();
                // To a channel the host shut, nothing goes.
                let _ = child.send(destination, bytes, handles);
            }
            // The host sends, of its own accord.
            4 => {
                let body = Bytes::from(vec![*arg; usize::from(*arg) * 1024]);
                let sent = host.send(
                    u32::from(*arg % 3),
                    &Msg::Many(vec![Msg::Body(body), Msg::Text("t".into())]),
                );
                assert_eq!(
                    sent.is_err(),
                    gone.is_some(),
                    "{sent:?} with the channel {gone:?}"
                );
            }
            // A pipe in a message, opened by the message itself.
            5 if arg % 3 == 2 => {
                let (mut source, consumer) = data_pipe::create(usize::from(*arg).max(1) * 64);
                let sent = host.send(
                    0,
                    &Msg::Many(vec![Msg::Text("with a pipe".into()), Msg::Piped(consumer)]),
                );
                assert_eq!(
                    sent.is_err(),
                    gone.is_some(),
                    "{sent:?} with the channel {gone:?}"
                );
                let written = source.try_write(Bytes::copy_from_slice(bytes));
                // Not sent: the pipe went with the message.
                if sent.is_err() && !bytes.is_empty() {
                    assert_eq!(written, Err(data_pipe::TryWriteError::Closed));
                }
                if arg % 2 == 0 {
                    sources.push(source);
                }
            }
            5 => {
                let (mut source, consumer) = data_pipe::create(1 << 16);
                match host.open_pipe(consumer, u32::from(*arg)) {
                    Ok(pipe) => {
                        assert!(gone.is_none());
                        let _ = host.send(0, &Msg::Stream { pipe });
                        let _ = source.try_write(Bytes::copy_from_slice(bytes));
                        if arg % 2 == 0 {
                            sources.push(source);
                        }
                    }
                    Err(error) => assert!(matches!(error, SendError::Gone(_)) && gone.is_some()),
                }
            }
            6 => host.grant_to_peer(usize::from(*arg) * 1024),
            _ => sources.clear(),
        }
        // The host's IO thread does all there is to do.
        let mut read = Vec::new();
        let mut handles = Vec::new();
        loop {
            let event = match woken.swap(false, Ordering::SeqCst) {
                true => Event::Woken,
                false => match host.platform().poll(&mut read, &mut handles) {
                    Some(event) => event,
                    None => break,
                },
            };
            if gone.is_none() {
                gone = endpoint.turn(event, &read, &mut handles, &mut counter);
                if gone.is_some() {
                    handed_up_when_gone = counter.handed_up;
                }
            }
            read.clear();
            handles.clear();
        }
        if gone.is_some() {
            assert_eq!(
                counter.handed_up, handed_up_when_gone,
                "handed up after the channel was over"
            );
        }
        let (total, free) = host.regions();
        assert!(free <= total && total <= 4 * MIN_REGION_LEN);
    }
}

#[test]
fn seeds_run() {
    use ipc::core::control::Control;

    run(&[]);
    let step = |op: u8, arg: u8, bytes: &[u8]| {
        let mut step = vec![op, arg, bytes.len() as u8];
        step.extend_from_slice(bytes);
        step
    };
    let message = |msg: &Msg| postcard::to_allocvec(msg).unwrap();
    let control = |control: Control<'_>| {
        let mut payload = Vec::new();
        control.encode(&mut payload);
        payload
    };
    // What a child of ours sends, with the host busy; then each kind of
    // the core's messages, right and wrong; then each of those with every
    // byte changed.
    let good = [
        step(4, 100, &[]),
        step(5, 40, b"the start of a body"),
        step(6, 64, &[]),
        step(0, 0, &message(&Msg::Text("hello".into()))),
        step(
            0,
            1,
            &message(&Msg::Many(vec![Msg::Body(Bytes::from_static(b"small"))])),
        ),
        step(0, 4, &control(Control::Released(vec![0]))),
        step(0, 4, &control(Control::PipeRead { pipe: 0, count: 10 })),
        step(4, 200, &[]),
        step(0, 4, &control(Control::PipeCancelled { pipe: 0 })),
        step(7, 0, &[]),
    ];
    run(&good.concat());
    let wrong = [
        step(0, 3, &message(&Msg::Text("no such destination".into()))),
        step(0, 0x40, &message(&Msg::Text("with a handle".into()))),
        step(0, 0, &[0xff, 0xff, 0xff]),
        step(0, 0, &message(&Msg::Stream { pipe: 9 })),
        step(0, 4, &control(Control::Released(vec![7]))),
        step(0, 4, &control(Control::Released(vec![0, 0]))),
        step(
            0,
            0x44,
            &control(Control::Granted {
                number: 1,
                len: 1 << 16,
                writable: true,
            }),
        ),
        step(
            0,
            4,
            &control(Control::Granted {
                number: 1,
                len: 1 << 16,
                writable: false,
            }),
        ),
        step(
            0,
            4,
            &control(Control::PipeOpened {
                pipe: 0,
                capacity: 100,
            }),
        ),
        step(
            0,
            4,
            &control(Control::PipeData {
                pipe: 0,
                bytes: b"x",
            }),
        ),
        step(
            0,
            4,
            &control(Control::PipeRead {
                pipe: 0,
                count: 1 << 40,
            }),
        ),
        step(0, 4, &control(Control::PipeRead { pipe: 5, count: 0 })),
        step(0, 4, &control(Control::PipeEnded { pipe: 0 })),
        step(0, 4, &control(Control::PipeCancelled { pipe: 3 })),
        step(0, 4, &[]),
    ];
    for one in &wrong {
        run(&[&good[..3].concat()[..], one, &good[3..].concat()[..]].concat());
        for at in 3..one.len() {
            let mut changed = one.clone();
            changed[at] ^= 0x41;
            run(&[&good[..3].concat()[..], &changed, &good[3..].concat()[..]].concat());
        }
    }
}

/// The fuzzer's finds, kept with their fixes.
#[test]
fn regressions_run() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fuzz/regressions/endpoint");
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|extension| extension == "bin") {
            run(&std::fs::read(&path).unwrap());
        }
    }
}
