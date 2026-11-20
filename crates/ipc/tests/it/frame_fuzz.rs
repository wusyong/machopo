//! What the fuzzer runs on the framing (`fuzz/fuzz_targets/frame.rs`
//! includes this file): bytes as a peer might send them, cut into pieces
//! and with handles beside them as the input says. Nothing a peer sends
//! may panic, and what the decoder gives out must hold whatever it was
//! given. The tests here replay `fuzz/regressions/`.

use std::cell::Cell;
use std::rc::Rc;

use ipc::core::frame::{BadMessage, Decoder, HEADER_LEN, MAX_HANDLES, MAX_PAYLOAD_LEN, Message};

/// A handle that counts itself in while it is alive.
struct Counted(Rc<Cell<usize>>);

impl Counted {
    fn new(alive: &Rc<Cell<usize>>) -> Self {
        alive.set(alive.get() + 1);
        Self(alive.clone())
    }
}

impl Drop for Counted {
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
}

/// What came out of a run: each message's destination, sequence number,
/// payload and number of handles, and how the run ended.
type Outcome = (Vec<(u32, u64, Vec<u8>, usize)>, Result<(), BadMessage>);

/// Feeds `stream` in the pieces `cuts` says (each a piece's length less
/// one, round and round; none: all at once), with as many handles beside
/// each piece as `handles` says for it.
fn feed(stream: &[u8], may_attach: bool, cuts: &[u8], handles: &[u8]) -> Outcome {
    let alive = Rc::new(Cell::new(0usize));
    let mut decoder = Decoder::<Counted>::new(may_attach);
    let mut messages = Vec::new();
    let mut rest = stream;
    let mut piece = 0usize;
    let mut fed = 0usize;
    let result = loop {
        let len = match cuts.is_empty() {
            true => rest.len(),
            false => (usize::from(cuts[piece % cuts.len()]) + 1).min(rest.len()),
        };
        let (bytes, after) = rest.split_at(len);
        let with = match handles.is_empty() {
            true => 0,
            false => usize::from(handles[piece % handles.len()] % 4),
        };
        fed += len;
        let before = messages.len();
        let result = decoder.feed(
            bytes,
            (0..with).map(|_| Counted::new(&alive)),
            |message: Message<'_, Counted>| {
                messages.push((
                    message.destination,
                    message.sequence,
                    message.payload.to_vec(),
                    message.handles.len(),
                ));
            },
        );
        // A handle from an end that may attach none ends it there.
        assert!(may_attach || with == 0 || result.is_err());
        // In order from 0, one at a time.
        for (nth, message) in messages.iter().enumerate().skip(before) {
            assert_eq!(message.1, nth as u64);
            assert!(message.2.len() <= MAX_PAYLOAD_LEN as usize);
            assert!(message.3 <= MAX_HANDLES as usize);
            assert!(
                may_attach || message.3 == 0,
                "a handle from an end that may attach none"
            );
        }
        // No more is kept than came, nor more handles than two messages'.
        assert!(decoder.buffered() <= fed);
        assert!(decoder.handles_held() <= MAX_HANDLES as usize * 2);
        assert!(may_attach || decoder.handles_held() == 0);
        // The handles alive are the decoder's alone: each message's were
        // dropped with it.
        assert_eq!(alive.get(), decoder.handles_held());
        if result.is_err() {
            assert_eq!((decoder.buffered(), decoder.handles_held()), (0, 0));
            // And it stays so.
            let again = decoder.feed(after, [Counted::new(&alive)], |_| {
                panic!("a message after a bad one")
            });
            assert_eq!(again, result);
            assert_eq!(alive.get(), 0);
            break result;
        }
        rest = after;
        piece += 1;
        if rest.is_empty() {
            break result;
        }
    };
    // Every message's bytes were in the stream.
    let given: usize = messages
        .iter()
        .map(|message| HEADER_LEN + message.2.len())
        .sum();
    assert!(given <= stream.len());
    drop(decoder);
    assert_eq!(alive.get(), 0, "a handle outlived the decoder");
    (messages, result)
}

/// One input: a byte of flags, how many of the next bytes are the cuts
/// and how many the handles, then the stream.
pub fn run(data: &[u8]) {
    let Some((&flags, data)) = data.split_first() else {
        return;
    };
    let Some((&counts, data)) = data.split_first() else {
        return;
    };
    let may_attach = flags & 1 != 0;
    let (cuts, data) = data.split_at(usize::from(counts & 0x0f).min(data.len()));
    let (handles, stream) = data.split_at(usize::from(counts >> 4).min(data.len()));
    // The bytes alone: however they are cut, the same messages come out
    // and the run ends the same way.
    let whole = feed(stream, may_attach, &[], &[]);
    let cut = feed(stream, may_attach, cuts, &[]);
    assert_eq!(whole, cut);
    // With handles beside them: the checks in `feed`.
    let _ = feed(stream, may_attach, cuts, handles);
}

/// A stream of `count` good messages, for seeds.
fn good_stream(count: u32) -> Vec<u8> {
    let mut encoder = ipc::core::frame::Encoder::new();
    let mut stream = Vec::new();
    for number in 0..count {
        let payload = vec![number as u8; (number as usize * 13) % 50];
        encoder.encode(number, &payload, 0, &mut stream).unwrap();
    }
    stream
}

#[test]
fn seeds_run() {
    for may_attach in [0u8, 1] {
        for cuts in [&[][..], &[0], &[3, 0, 19], &[255]] {
            for handles in [&[][..], &[1], &[0, 0, 2]] {
                let mut input = vec![may_attach, (cuts.len() | handles.len() << 4) as u8];
                input.extend_from_slice(cuts);
                input.extend_from_slice(handles);
                let stream = good_stream(12);
                // Good, cut short, and with each byte of the first two
                // headers changed.
                run(&[&input[..], &stream].concat());
                run(&[&input[..], &stream[..stream.len() - 7]].concat());
                for at in 0..HEADER_LEN * 2 {
                    let mut broken = stream.clone();
                    broken[at] ^= 0x81;
                    run(&[&input[..], &broken].concat());
                }
            }
        }
    }
    run(&[]);
    run(&[1]);
    run(&[1, 0xff]);
}

/// The fuzzer's finds, kept with their fixes.
#[test]
fn regressions_run() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fuzz/regressions");
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|extension| extension == "bin") {
            run(&std::fs::read(&path).unwrap());
        }
    }
}
