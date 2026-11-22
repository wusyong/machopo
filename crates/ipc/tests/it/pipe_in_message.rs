//! A data pipe's consumer in a message's value: `ipc::codec` opens a pipe
//! for it wherever it is nested and writes the pipe's number, and reads
//! the number back as the consumer the other end has for it. Nothing else
//! serializes one, and a value that isn't sent loses its pipes cleanly.

use std::collections::BTreeMap;

use bytes::Bytes;
use ipc::codec::{self, EncodeError, NoRegions, Place, Resolve};
use ipc::core::frame::BadMessage;
use ipc::data_pipe::{self, Consumer, Producer, Read, TryWriteError};
use serde::{Deserialize, Serialize};

/// A pipe with `bytes` written and its writing end kept.
fn pipe_with(bytes: &'static [u8]) -> (Producer, Consumer) {
    let (mut producer, consumer) = data_pipe::create(64);
    producer.try_write(Bytes::from_static(bytes)).unwrap();
    (producer, consumer)
}

fn read_all(consumer: &mut Consumer) -> Vec<u8> {
    let mut got = Vec::new();
    while let Read::Data(bytes) = consumer.read(usize::MAX) {
        got.extend_from_slice(&bytes);
    }
    got
}

/// An end that opens pipes: keeps each source under the number it gave,
/// from 10, and what was taken back.
#[derive(Default)]
struct Opening {
    open: Vec<(u32, Consumer)>,
    taken_back: Vec<u32>,
    /// Opens no more than this many.
    limit: Option<usize>,
}

impl Place for Opening {
    fn place(&mut self, _: &[u8]) -> Option<u32> {
        None
    }

    fn unplace(&mut self, _: u32) {}

    fn pipe(&mut self, source: Consumer) -> Option<u32> {
        if self.limit.is_some_and(|limit| self.open.len() >= limit) {
            return None;
        }
        let number = 10 + self.open.len() as u32;
        self.open.push((number, source));
        Some(number)
    }

    fn unpipe(&mut self, number: u32) {
        self.open.retain(|(open, _)| *open != number);
        self.taken_back.push(number);
    }
}

/// The other end: the consumers it has for the pipes it was told of.
#[derive(Default)]
struct Arrived(Vec<(u32, Consumer)>);

impl Resolve for Arrived {
    fn resolve(&mut self, number: u32, _: u32, _: &mut Vec<u8>) -> Result<(), BadMessage> {
        Err(BadMessage::UnknownRegion { number })
    }

    fn pipe(&mut self, number: u32) -> Result<Consumer, BadMessage> {
        let i = self
            .0
            .iter()
            .position(|(pipe, _)| *pipe == number)
            .ok_or(BadMessage::UnknownPipe { pipe: number })?;
        Ok(self.0.swap_remove(i).1)
    }
}

/// Consumers at every kind of place a value has.
#[derive(Serialize, Deserialize)]
struct Carrying {
    name: String,
    body: Consumer,
    maybe: Option<Consumer>,
    none: Option<Consumer>,
    list: Vec<Consumer>,
    map: BTreeMap<String, Consumer>,
    shape: Shape,
    after: u32,
}

#[derive(Serialize, Deserialize)]
enum Shape {
    Newtype(Consumer),
    Struct { pipe: Consumer },
}

#[test]
fn a_consumer_goes_as_its_pipes_number_wherever_it_is_nested() {
    let mut writers = Vec::new();
    let mut pipe = |bytes| {
        let (producer, consumer) = pipe_with(bytes);
        writers.push(producer);
        consumer
    };
    let value = Carrying {
        name: "a document".into(),
        body: pipe(b"body"),
        maybe: Some(pipe(b"maybe")),
        none: None,
        list: vec![pipe(b"first"), pipe(b"second")],
        map: BTreeMap::from([("k".to_owned(), pipe(b"in a map"))]),
        shape: Shape::Struct {
            pipe: pipe(b"in a variant"),
        },
        after: 7,
    };
    let mut opening = Opening::default();
    let mut payload = Vec::new();
    codec::encode(&value, &mut opening, &mut payload).unwrap();
    // Each was opened, in the order the value has them, and the payload
    // is short: numbers, not pipes.
    let numbers: Vec<u32> = opening.open.iter().map(|(number, _)| *number).collect();
    assert_eq!(numbers, [10, 11, 12, 13, 14, 15]);
    assert!(payload.len() < 40, "{}", payload.len());
    // The value gave its pipes up: what is left of each is a pipe that
    // is over, and dropping it closes nothing.
    let Carrying { mut body, .. } = value;
    assert_eq!(body.read(64), Read::Done);
    drop(body);
    assert_eq!(
        writers[0].try_write(Bytes::from_static(b"!")),
        Ok(()),
        "the pipe is still open: its source is the end's now"
    );

    // The other end has a consumer for each number, of pipes of its own.
    let mut arrived = Arrived::default();
    let mut theirs = Vec::new();
    for (number, word) in [
        (10, &b"BODY"[..]),
        (11, b"MAYBE"),
        (12, b"FIRST"),
        (13, b"SECOND"),
        (14, b"IN A MAP"),
        (15, b"IN A VARIANT"),
    ] {
        let (mut producer, consumer) = data_pipe::create(64);
        producer.try_write(Bytes::copy_from_slice(word)).unwrap();
        theirs.push(producer);
        arrived.0.push((number, consumer));
    }
    let mut back: Carrying = codec::decode(&payload, &mut arrived).unwrap();
    assert!(arrived.0.is_empty(), "every one was taken");
    assert_eq!((back.name.as_str(), back.after), ("a document", 7));
    assert_eq!(read_all(&mut back.body), b"BODY");
    assert_eq!(read_all(back.maybe.as_mut().unwrap()), b"MAYBE");
    assert!(back.none.is_none());
    assert_eq!(read_all(&mut back.list[0]), b"FIRST");
    assert_eq!(read_all(&mut back.list[1]), b"SECOND");
    assert_eq!(read_all(back.map.get_mut("k").unwrap()), b"IN A MAP");
    let Shape::Struct { mut pipe } = back.shape else {
        panic!("another shape");
    };
    assert_eq!(read_all(&mut pipe), b"IN A VARIANT");
}

#[test]
fn a_consumer_crosses_once() {
    let (_writer, consumer) = pipe_with(b"once");
    let mut opening = Opening::default();
    let mut payload = Vec::new();
    codec::encode(&consumer, &mut opening, &mut payload).unwrap();
    assert_eq!(opening.open.len(), 1);
    // Again: the value has no pipe to give.
    let before = payload.clone();
    assert_eq!(
        codec::encode(&consumer, &mut opening, &mut payload),
        Err(EncodeError)
    );
    assert_eq!(payload, before);
    assert_eq!(opening.open.len(), 1);
}

/// A value that fails to serialize after its pipes were opened.
#[derive(Serialize)]
struct ThenFails {
    first: Consumer,
    second: Consumer,
    #[serde(serialize_with = "refuse")]
    last: u8,
}

fn refuse<S: serde::Serializer>(_: &u8, _: S) -> Result<S::Ok, S::Error> {
    Err(serde::ser::Error::custom("refused"))
}

#[test]
fn a_value_that_doesnt_serialize_ends_the_pipes_opened_for_it() {
    let (first_writer, first) = pipe_with(b"1");
    let (second_writer, second) = pipe_with(b"2");
    let mut opening = Opening::default();
    let mut payload = vec![0xaa];
    assert_eq!(
        codec::encode(
            &ThenFails {
                first,
                second,
                last: 0
            },
            &mut opening,
            &mut payload
        ),
        Err(EncodeError)
    );
    assert_eq!(payload, [0xaa]);
    assert_eq!(opening.taken_back, [10, 11]);
    assert!(opening.open.is_empty());
    // Their sources went with them: whoever writes hears.
    for mut writer in [first_writer, second_writer] {
        assert_eq!(
            writer.try_write(Bytes::from_static(b"x")),
            Err(TryWriteError::Closed)
        );
    }
}

#[test]
fn an_end_that_opens_no_pipe_sends_no_value_with_one() {
    // An end with no pipes at all.
    let (mut writer, consumer) = pipe_with(b"nowhere");
    let mut payload = Vec::new();
    assert_eq!(
        codec::encode(&consumer, &mut NoRegions, &mut payload),
        Err(EncodeError)
    );
    assert!(payload.is_empty());
    assert_eq!(
        writer.try_write(Bytes::from_static(b"x")),
        Err(TryWriteError::Closed),
        "the pipe went with the value that couldn't be sent"
    );
    // An end that ran out after the first: the first is taken back.
    let (_a, first) = pipe_with(b"a");
    let (_b, second) = pipe_with(b"b");
    let mut opening = Opening {
        limit: Some(1),
        ..Default::default()
    };
    assert_eq!(
        codec::encode(&(first, second), &mut opening, &mut payload),
        Err(EncodeError)
    );
    assert_eq!(opening.taken_back, [10]);
}

#[test]
fn a_number_that_is_no_pipes_is_a_bad_message() {
    let (_writer, consumer) = pipe_with(b"x");
    let mut opening = Opening::default();
    let mut payload = Vec::new();
    codec::encode(&(1u8, consumer), &mut opening, &mut payload).unwrap();
    // The other end was told of no such pipe.
    let mut arrived = Arrived::default();
    assert_eq!(
        codec::decode::<(u8, Consumer)>(&payload, &mut arrived).err(),
        Some(BadMessage::UnknownPipe { pipe: 10 })
    );
    // Or has it, and the message names it twice.
    let (_theirs, consumer) = data_pipe::create(64);
    arrived.0.push((10, consumer));
    let twice = postcard::to_allocvec(&(10u32, 10u32)).unwrap();
    assert_eq!(
        codec::decode::<(Consumer, Consumer)>(&twice, &mut arrived).err(),
        Some(BadMessage::UnknownPipe { pipe: 10 })
    );
    // An end with no pipes names none.
    assert_eq!(
        codec::decode::<(u8, Consumer)>(&payload, &mut NoRegions).err(),
        Some(BadMessage::UnknownPipe { pipe: 10 })
    );
}

#[test]
fn only_the_codec_serializes_a_consumer() {
    let (mut writer, mut consumer) = pipe_with(b"kept");
    // Another serializer is refused, and the value keeps its pipe.
    assert!(serde_json::to_string(&consumer).is_err());
    assert!(postcard::to_allocvec(&consumer).is_err());
    assert_eq!(read_all(&mut consumer), b"kept");
    writer.try_write(Bytes::from_static(b"still")).unwrap();
    assert_eq!(read_all(&mut consumer), b"still");
    // And nothing reads one but out of a message.
    assert!(serde_json::from_str::<Consumer>("10").is_err());
    assert!(postcard::from_bytes::<Consumer>(&[10]).is_err());
    // What the refused value left behind isn't the next value's: a
    // message with no pipe in it has none.
    let mut opening = Opening::default();
    let mut payload = Vec::new();
    codec::encode(&"plain", &mut opening, &mut payload).unwrap();
    assert!(opening.open.is_empty());
}

/// Something else that calls itself what a consumer does.
struct Impostor;

impl Serialize for Impostor {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_newtype_struct("$ipc::data_pipe::Consumer", &10u32)
    }
}

impl<'de> Deserialize<'de> for Impostor {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Any;
        impl<'de> serde::de::Visitor<'de> for Any {
            type Value = Impostor;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("anything")
            }
            fn visit_u32<E>(self, _: u32) -> Result<Impostor, E> {
                Ok(Impostor)
            }
        }
        deserializer.deserialize_newtype_struct("$ipc::data_pipe::Consumer", Any)
    }
}

#[test]
fn a_type_that_takes_a_consumers_name_gets_no_pipe() {
    // Sending: it left no pipe to open.
    let mut opening = Opening::default();
    let mut payload = Vec::new();
    assert_eq!(
        codec::encode(&Impostor, &mut opening, &mut payload),
        Err(EncodeError)
    );
    assert!(opening.open.is_empty());
    // Receiving: the pipe it named is taken from the end and let go, and
    // isn't left for the next consumer to be read.
    let (mut theirs, consumer) = data_pipe::create(64);
    let mut arrived = Arrived(vec![(10, consumer)]);
    let named = postcard::to_allocvec(&10u32).unwrap();
    assert!(codec::decode::<Impostor>(&named, &mut arrived).is_ok());
    assert_eq!(
        theirs.try_write(Bytes::from_static(b"x")),
        Err(TryWriteError::Closed)
    );
    assert!(serde_json::from_str::<Consumer>("10").is_err());
}
