//! What the fuzzer runs on the regions (`fuzz/fuzz_targets/region.rs`
//! includes this file): a pool and a table driven by the input against a
//! plain model of them, and the input read as a payload that names
//! regions and as one of the core's own messages. Nothing a peer sends
//! may panic or reach outside a region (the fuzzer runs with ASan), and a
//! region is never written while what was put in it is still out. The
//! tests here replay `fuzz/regressions/region/`.

use std::collections::BTreeMap;
use std::sync::Arc;

use bytes::Bytes;
use ipc::codec::{self, NoRegions};
use ipc::core::control::Control;
use ipc::core::frame::BadMessage;
use ipc::core::region::{
    AddError, HeapMemory, MAX_REGION_LEN, MIN_REGION_LEN, Pool, Table, class_len,
};
use serde::{Deserialize, Serialize};

type Shared = Arc<HeapMemory>;

/// What the model knows of a region: its length, what is out in it, and
/// whether it is out to a taker that hasn't lent it to a message.
struct Known {
    len: usize,
    out: Option<Vec<u8>>,
    held: bool,
}

const LIMIT: usize = 4 * MIN_REGION_LEN;

/// The pool and the table over the same regions, and the model.
struct Ends {
    pool: Pool<Shared>,
    table: Table<Shared>,
    known: BTreeMap<u32, Known>,
}

impl Ends {
    fn new() -> Self {
        Self {
            pool: Pool::new(LIMIT),
            table: Table::new(),
            known: BTreeMap::new(),
        }
    }

    fn total(&self) -> usize {
        self.known.values().map(|known| known.len).sum()
    }

    /// The shortest free region that holds `len` bytes, by the model.
    fn fit(&self, len: usize) -> Option<usize> {
        self.known
            .values()
            .filter(|known| known.out.is_none() && known.len >= len)
            .map(|known| known.len)
            .min()
    }

    fn step(&mut self, op: u8, number: u32, amount: usize) {
        match op % 8 {
            0 => {
                // Lengths around the two smallest classes, and not classes.
                let len = match amount % 5 {
                    0 => MIN_REGION_LEN,
                    1 => 2 * MIN_REGION_LEN,
                    2 => MIN_REGION_LEN + 1,
                    3 => MIN_REGION_LEN / 2,
                    _ => amount % (3 * MIN_REGION_LEN),
                };
                let memory = Arc::new(HeapMemory::new(len));
                let expected = if self.known.contains_key(&number) {
                    Err(AddError::Duplicate)
                } else if class_len(len) != Some(len) {
                    Err(AddError::NotAClass)
                } else if self.known.values().map(|known| known.len).sum::<usize>() + len > LIMIT {
                    Err(AddError::OverLimit)
                } else {
                    Ok(())
                };
                assert_eq!(self.pool.add(number, memory.clone()), expected);
                if expected.is_ok() {
                    assert!(self.table.insert(number, memory));
                    self.known.insert(
                        number,
                        Known {
                            len,
                            out: None,
                            held: false,
                        },
                    );
                }
            }
            1 => {
                let bytes: Vec<u8> = (0..amount % (2 * MIN_REGION_LEN + 2))
                    .map(|i| (i as u8) ^ op)
                    .collect();
                let fit = self.fit(bytes.len());
                match self.pool.put(&bytes) {
                    Some(placed) => {
                        let known = self
                            .known
                            .get_mut(&placed)
                            .expect("a region the pool was never given");
                        assert!(known.out.is_none(), "written while what was in it is out");
                        assert_eq!(Some(known.len), fit, "not the shortest that holds it");
                        known.out = Some(bytes);
                    }
                    None => assert_eq!(fit, None, "a free region holds it"),
                }
            }
            2 => {
                // Out, and not to a taker that has yet to send it.
                let was_out = self
                    .known
                    .get(&number)
                    .map(|known| known.out.is_some() && !known.held);
                let expected = match was_out {
                    None => Err(BadMessage::UnknownRegion { number }),
                    Some(false) => Err(BadMessage::RegionNotOut { number }),
                    Some(true) => Ok(()),
                };
                assert_eq!(self.pool.released(number), expected);
                if expected.is_ok() {
                    self.known.get_mut(&number).unwrap().out = None;
                }
            }
            3 => {
                let free = self
                    .known
                    .get(&number)
                    .is_some_and(|known| known.out.is_none());
                assert_eq!(self.pool.remove(number).is_some(), free);
                if free {
                    assert!(self.table.remove(number).is_some());
                    self.known.remove(&number);
                }
            }
            4 => {
                let len = u32::try_from(amount).unwrap_or(u32::MAX);
                let mut read = vec![0x77];
                let expected = match self.known.get(&number) {
                    None => Err(BadMessage::UnknownRegion { number }),
                    Some(known) if amount > known.len => {
                        Err(BadMessage::OutsideRegion { number, len })
                    }
                    Some(_) => Ok(()),
                };
                assert_eq!(self.table.read(number, len, &mut read), expected);
                match expected {
                    Ok(()) => assert_eq!(read.len(), 1 + amount),
                    Err(_) => assert_eq!(read, [0x77]),
                }
            }
            5 => {
                let fit = self.fit(amount);
                match self.pool.take(amount) {
                    Some(taken) => {
                        let known = self.known.get_mut(&taken).unwrap();
                        assert!(known.out.is_none());
                        assert_eq!(Some(known.len), fit);
                        // Whoever took it wrote nothing: what it holds is
                        // what the model has for it from now on.
                        let mut held = Vec::new();
                        self.table.read(taken, known.len as u32, &mut held).unwrap();
                        known.out = Some(held);
                        known.held = true;
                    }
                    None => assert_eq!(fit, None),
                }
            }
            6 => {
                let was_out = self
                    .known
                    .get(&number)
                    .is_some_and(|known| known.out.is_some());
                assert_eq!(self.pool.give_back(number), was_out);
                if was_out {
                    let known = self.known.get_mut(&number).unwrap();
                    known.out = None;
                    known.held = false;
                }
            }
            _ => {
                // Lent to a message by its taker, once: still out, and
                // the peer's to give back from now.
                let held = self.known.get(&number).is_some_and(|known| known.held);
                assert_eq!(self.pool.lend(number), held);
                if held {
                    self.known.get_mut(&number).unwrap().held = false;
                }
            }
        }
        self.check();
    }

    /// What is out is still what was put, and the pool counts what the
    /// model does.
    fn check(&mut self) {
        let pool = &self.pool;
        assert_eq!(pool.total(), self.total());
        assert!(pool.total() <= LIMIT);
        let free: BTreeMap<u32, usize> = pool.free().collect();
        for (number, known) in &self.known {
            assert_eq!(
                free.get(number).copied(),
                known.out.is_none().then_some(known.len)
            );
            if let Some(bytes) = &known.out {
                let mut read = Vec::new();
                self.table
                    .read(*number, bytes.len() as u32, &mut read)
                    .unwrap();
                assert!(
                    read == *bytes,
                    "region {number} changed under what was out in it"
                );
            }
        }
        assert_eq!(
            free.len(),
            self.known
                .values()
                .filter(|known| known.out.is_none())
                .count()
        );
        self.table.take_read();
    }
}

/// A message with bytes at several depths, as a peer's payload is read.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Message {
    id: u32,
    body: Bytes,
    parts: Vec<Part>,
    extra: Option<Box<Message>>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum Part {
    None,
    Bytes(Bytes),
    Named { name: String, bytes: Bytes },
    More(Vec<Part>),
}

/// The input as a payload: whatever it names, reading it stays inside
/// the regions; and what it reads as writes and reads back the same.
fn as_payload(data: &[u8]) {
    let mut table = Table::<Shared>::new();
    for (number, len) in [(0, MIN_REGION_LEN), (1, 2 * MIN_REGION_LEN)] {
        table.insert(number, Arc::new(HeapMemory::new(len)));
    }
    let Ok(message) = codec::decode::<Message>(data, &mut table) else {
        return;
    };
    let mut pool = Pool::<Shared>::new(MAX_REGION_LEN);
    let mut peer = Table::new();
    for number in 0..4 {
        let memory = Arc::new(HeapMemory::new(2 * MIN_REGION_LEN));
        pool.add(number, memory.clone()).unwrap();
        peer.insert(number, memory);
    }
    let mut payload = Vec::new();
    codec::encode(&message, &mut pool, &mut payload).unwrap();
    assert_eq!(
        codec::decode::<Message>(&payload, &mut peer).as_ref(),
        Ok(&message)
    );
    payload.clear();
    codec::encode(&message, &mut NoRegions, &mut payload).unwrap();
    assert_eq!(
        codec::decode::<Message>(&payload, &mut NoRegions),
        Ok(message)
    );
}

/// One input: as a payload, as one of the core's messages, and as steps
/// (an operation, a region's number, an amount in three bytes).
pub fn run(data: &[u8]) {
    as_payload(data);
    if let Ok(control) = Control::decode(data) {
        let mut again = Vec::new();
        control.encode(&mut again);
        assert_eq!(again, data);
    }
    let mut ends = Ends::new();
    for step in data.as_chunks::<5>().0.iter().take(64) {
        let amount = usize::from(step[2]) | usize::from(step[3]) << 8 | usize::from(step[4]) << 16;
        ends.step(step[0], u32::from(step[1] % 6), amount);
    }
}

#[test]
fn seeds_run() {
    run(&[]);
    // Each operation, on regions there and not.
    let mut steps = Vec::new();
    for round in 0..40u8 {
        for op in 0..7u8 {
            let amount =
                (usize::from(round) * 7919 + usize::from(op) * 30011) % (3 * MIN_REGION_LEN);
            steps.extend([
                op,
                round.wrapping_add(op),
                amount as u8,
                (amount >> 8) as u8,
                (amount >> 16) as u8,
            ]);
            run(&steps);
        }
    }
    // Payloads: good, naming regions, cut, and with bytes changed.
    let message = Message {
        id: 7,
        body: Bytes::from(vec![5; MIN_REGION_LEN]),
        parts: vec![
            Part::None,
            Part::Bytes(Bytes::from_static(b"small")),
            Part::More(vec![Part::Named {
                name: "n".into(),
                bytes: Bytes::from(vec![6; MIN_REGION_LEN + 9]),
            }]),
        ],
        extra: Some(Box::new(Message {
            id: 8,
            body: Bytes::new(),
            parts: vec![],
            extra: None,
        })),
    };
    let mut pool = Pool::<Shared>::new(MAX_REGION_LEN);
    pool.add(0, Arc::new(HeapMemory::new(MIN_REGION_LEN)))
        .unwrap();
    pool.add(1, Arc::new(HeapMemory::new(2 * MIN_REGION_LEN)))
        .unwrap();
    let mut payload = Vec::new();
    codec::encode(&message, &mut pool, &mut payload).unwrap();
    assert!(payload.len() < 100);
    run(&payload);
    for at in 0..payload.len() {
        run(&payload[..at]);
        for flip in [0x01, 0x80, 0xff] {
            let mut broken = payload.clone();
            broken[at] ^= flip;
            run(&broken);
        }
    }
    let mut control = Vec::new();
    Control::Released(vec![1, 2, 3]).encode(&mut control);
    run(&control);
    control.clear();
    Control::Granted {
        number: 1,
        len: 1 << 16,
        writable: true,
    }
    .encode(&mut control);
    run(&control);
}

/// The fuzzer's finds, kept with their fixes.
#[test]
fn regressions_run() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fuzz/regressions/region");
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|extension| extension == "bin") {
            run(&std::fs::read(&path).unwrap());
        }
    }
}
