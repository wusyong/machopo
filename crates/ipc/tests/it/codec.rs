//! A message's value through `ipc::codec`: large bytes go by a region and
//! come back the same, everything else is postcard's bytes untouched, and
//! what a peer could send that isn't a value is refused, not followed.

use std::collections::BTreeMap;
use std::sync::Arc;

use bytes::Bytes;
use ipc::codec::{self, EncodeError, MAX_DEPTH, NoRegions, Place};
use ipc::core::frame::BadMessage;
use ipc::core::region::{HeapMemory, MIN_REGION_LEN, Pool, Table};
use serde::{Deserialize, Serialize};

const KIB: usize = 1 << 10;

type Shared = Arc<HeapMemory>;

/// The two ends' views of the same regions: 64 KiB is number 1, 256 KiB
/// number 2, 1 MiB number 3.
fn ends() -> (Pool<Shared>, Table<Shared>) {
    let mut pool = Pool::new(8 << 20);
    let mut table = Table::new();
    for (number, len) in [(1, 64 * KIB), (2, 256 * KIB), (3, 1 << 20)] {
        let memory = Arc::new(HeapMemory::new(len));
        pool.add(number, memory.clone()).unwrap();
        assert!(table.insert(number, memory));
    }
    (pool, table)
}

fn pattern(len: usize, salt: u8) -> Bytes {
    (0..len)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(salt))
        .collect()
}

/// Every shape serde has, with bytes at several depths.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Everything {
    flag: bool,
    small: (i8, i16, i32, i64, i128),
    unsigned: (u8, u16, u32, u64, u128),
    floats: (f32, f64),
    letter: char,
    text: String,
    unit: (),
    marker: Marker,
    wrapped: Wrapper,
    pair: Pair,
    body: Bytes,
    maybe: Option<Bytes>,
    nothing: Option<Bytes>,
    list: Vec<Shape>,
    map: BTreeMap<String, Bytes>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Marker;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Wrapper(Bytes);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Pair(u32, Bytes);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum Shape {
    Unit,
    Newtype(Bytes),
    Tuple(u8, Bytes),
    Struct { name: String, bytes: Bytes },
}

fn everything(body: Bytes, in_list: Bytes) -> Everything {
    Everything {
        flag: true,
        small: (-1, -300, -70_000, -5_000_000_000, -(1 << 100)),
        unsigned: (200, 60_000, 4_000_000_000, 1 << 60, 1 << 120),
        floats: (1.5, -2.25),
        letter: '字',
        text: "text".into(),
        unit: (),
        marker: Marker,
        wrapped: Wrapper(Bytes::from_static(b"wrapped")),
        pair: Pair(9, Bytes::from_static(b"pair")),
        body,
        maybe: Some(Bytes::from_static(b"maybe")),
        nothing: None,
        list: vec![
            Shape::Unit,
            Shape::Newtype(Bytes::from_static(b"newtype")),
            Shape::Tuple(3, in_list),
            Shape::Struct {
                name: "struct".into(),
                bytes: Bytes::from_static(b"in a struct"),
            },
        ],
        map: BTreeMap::from([("key".into(), Bytes::from_static(b"value"))]),
    }
}

/// The same value with no run of bytes in it: what the wrapper must leave
/// exactly as postcard writes it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct NoBytes {
    flag: bool,
    numbers: (i8, i16, i32, i64, i128, u8, u16, u32, u64, u128, f32, f64),
    letter: char,
    text: String,
    unit: (),
    marker: Marker,
    wrapped: Count,
    maybe: Option<Count>,
    nothing: Option<u8>,
    list: Vec<Plain>,
    map: BTreeMap<String, Vec<u16>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Count(u32);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum Plain {
    Unit,
    Newtype(String),
    Tuple(u8, i64),
    Struct { name: String, on: bool },
}

fn no_bytes() -> NoBytes {
    NoBytes {
        flag: true,
        numbers: (
            -1,
            -300,
            -70_000,
            -5_000_000_000,
            -(1 << 100),
            200,
            60_000,
            4_000_000_000,
            1 << 60,
            1 << 120,
            1.5,
            -2.25,
        ),
        letter: '字',
        text: "text".into(),
        unit: (),
        marker: Marker,
        wrapped: Count(77),
        maybe: Some(Count(78)),
        nothing: None,
        list: vec![
            Plain::Unit,
            Plain::Newtype("newtype".into()),
            Plain::Tuple(3, -4),
            Plain::Struct {
                name: "struct".into(),
                on: true,
            },
        ],
        map: BTreeMap::from([("key".into(), vec![1, 2, 3])]),
    }
}

fn encoded<T: Serialize>(value: &T, regions: &mut dyn Place) -> Vec<u8> {
    let mut payload = Vec::new();
    codec::encode(value, regions, &mut payload).unwrap();
    payload
}

#[test]
fn a_value_without_bytes_is_written_as_postcard_writes_it() {
    let value = no_bytes();
    let (mut pool, mut table) = ends();
    let payload = encoded(&value, &mut pool);
    assert_eq!(payload, postcard::to_allocvec(&value).unwrap());
    assert_eq!(codec::decode::<NoBytes>(&payload, &mut table), Ok(value));
    assert_eq!(pool.free().count(), 3, "nothing was placed");
}

#[test]
fn every_shape_of_value_comes_back_with_its_bytes_in_the_message() {
    let value = everything(pattern(64 * KIB - 1, 1), pattern(100, 2));
    let (mut pool, mut table) = ends();
    let payload = encoded(&value, &mut pool);
    assert_eq!(
        pool.free().count(),
        3,
        "all of it is under what goes by a region"
    );
    assert!(payload.len() > 64 * KIB);
    assert_eq!(
        codec::decode::<Everything>(&payload, &mut table),
        Ok(value.clone())
    );
    assert!(table.take_read().is_empty());
    // The same without regions at either end.
    assert_eq!(encoded(&value, &mut NoRegions), payload);
    assert_eq!(
        codec::decode::<Everything>(&payload, &mut NoRegions),
        Ok(value)
    );
}

#[test]
fn large_bytes_go_by_a_region_wherever_they_are_nested() {
    let body = pattern(MIN_REGION_LEN, 3);
    let in_list = pattern(200 * KIB, 4);
    let value = everything(body, in_list);
    let (mut pool, mut table) = ends();
    let payload = encoded(&value, &mut pool);
    assert!(
        payload.len() < 2 * KIB,
        "{} bytes: the large ones aren't in it",
        payload.len()
    );
    assert_eq!(
        pool.free().collect::<Vec<_>>(),
        [(3, 1 << 20)],
        "the two shortest that hold them are out"
    );
    assert_eq!(codec::decode::<Everything>(&payload, &mut table), Ok(value));
    assert_eq!(table.take_read(), [1, 2], "in the order the value has them");
}

#[test]
fn a_region_is_written_again_once_the_peer_read_it() {
    let (mut pool, mut table) = ends();
    for salt in 0..4 {
        let value = Wrapper(pattern(900 * KIB, salt));
        let payload = encoded(&value, &mut pool);
        assert_eq!(
            payload.len(),
            5,
            "a variant, the number 3, and a length in three bytes"
        );
        assert_eq!(codec::decode::<Wrapper>(&payload, &mut table), Ok(value));
        for number in table.take_read() {
            pool.released(number).unwrap();
        }
    }
}

#[test]
fn bytes_no_free_region_holds_stay_in_the_message() {
    let (mut pool, mut table) = ends();
    let first = Wrapper(pattern(900 * KIB, 1));
    let second = Wrapper(pattern(900 * KIB, 2));
    let in_region = encoded(&first, &mut pool);
    let inline = encoded(&second, &mut pool);
    assert!(inline.len() > 900 * KIB, "the one region that long is out");
    assert_eq!(codec::decode::<Wrapper>(&inline, &mut table), Ok(second));
    assert!(table.take_read().is_empty());
    assert_eq!(
        codec::decode::<Wrapper>(&in_region, &mut table),
        Ok(first),
        "and wasn't written over"
    );
    // Longer than any region there is: in the message too.
    let (mut pool, mut table) = ends();
    let long = Wrapper(pattern((1 << 20) + 1, 3));
    let payload = encoded(&long, &mut pool);
    assert_eq!(pool.free().count(), 3);
    assert_eq!(codec::decode::<Wrapper>(&payload, &mut table), Ok(long));
}

#[test]
fn a_payload_is_written_after_what_the_vector_has() {
    let (mut pool, mut table) = ends();
    let mut payload = vec![0xaa, 0xbb];
    codec::encode(&Count(5), &mut pool, &mut payload).unwrap();
    assert_eq!(payload, [0xaa, 0xbb, 5]);
    assert_eq!(
        codec::decode::<Count>(&payload[2..], &mut table),
        Ok(Count(5))
    );
}

/// A value that places its bytes and then refuses to serialize.
struct FailsAfter(Bytes);

impl Serialize for FailsAfter {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeTuple;
        let mut tuple = serializer.serialize_tuple(2)?;
        tuple.serialize_element(&self.0)?;
        tuple.serialize_element(&Refuses)?;
        tuple.end()
    }
}

struct Refuses;

impl Serialize for Refuses {
    fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom("refused"))
    }
}

#[test]
fn a_value_that_doesnt_serialize_leaves_nothing_placed_or_written() {
    let (mut pool, _) = ends();
    let mut payload = vec![1, 2, 3];
    let value = FailsAfter(pattern(100 * KIB, 0));
    assert_eq!(
        codec::encode(&value, &mut pool, &mut payload),
        Err(EncodeError)
    );
    assert_eq!(payload, [1, 2, 3]);
    assert_eq!(
        pool.free().count(),
        3,
        "the region its bytes went in is free again"
    );
}

/// What a hostile peer writes: the same variants, any numbers.
#[derive(Serialize)]
enum Forged {
    #[expect(
        dead_code,
        reason = "keeps `Region` the second variant, as the codec writes it"
    )]
    Inline(Bytes),
    Region(u32, u32),
    Third(Bytes),
}

fn forged(value: &Forged) -> Vec<u8> {
    postcard::to_allocvec(value).unwrap()
}

#[test]
fn a_region_that_isnt_the_peers_or_is_shorter_is_a_bad_message() {
    let (mut pool, mut table) = ends();
    let honest = encoded(&Wrapper(pattern(64 * KIB, 0)), &mut pool);
    assert_eq!(
        honest,
        forged(&Forged::Region(1, 64 * KIB as u32)),
        "what an honest end writes is this"
    );
    assert!(codec::decode::<Wrapper>(&honest, &mut table).is_ok());

    let unknown = forged(&Forged::Region(4, 1));
    assert_eq!(
        codec::decode::<Wrapper>(&unknown, &mut table),
        Err(BadMessage::UnknownRegion { number: 4 })
    );
    let over = 64 * KIB as u32 + 1;
    let outside = forged(&Forged::Region(1, over));
    assert_eq!(
        codec::decode::<Wrapper>(&outside, &mut table),
        Err(BadMessage::OutsideRegion {
            number: 1,
            len: over
        })
    );
    // An end with no regions is named none.
    assert_eq!(
        codec::decode::<Wrapper>(&honest, &mut NoRegions),
        Err(BadMessage::UnknownRegion { number: 1 })
    );
    // Neither variant, though what follows would read as bytes.
    let third = forged(&Forged::Third(Bytes::from_static(b"bytes")));
    assert_eq!(
        codec::decode::<Wrapper>(&third, &mut table),
        Err(BadMessage::Rejected)
    );
    // The first thing wrong is what is reported.
    #[derive(Debug, PartialEq, Deserialize)]
    struct Two(Bytes, Bytes);
    let mut both = forged(&Forged::Region(9, 1));
    both.extend(forged(&Forged::Region(1, over)));
    assert_eq!(
        codec::decode::<Two>(&both, &mut table),
        Err(BadMessage::UnknownRegion { number: 9 })
    );
}

#[test]
fn a_payload_that_isnt_one_whole_value_is_a_bad_message() {
    let (mut pool, mut table) = ends();
    let value = everything(pattern(300, 1), pattern(100 * KIB, 2));
    let payload = encoded(&value, &mut pool);
    let mut longer = payload.clone();
    longer.push(0);
    assert_eq!(
        codec::decode::<Everything>(&longer, &mut table),
        Err(BadMessage::Rejected)
    );
    for cut in [0, 1, payload.len() / 2, payload.len() - 1] {
        assert_eq!(
            codec::decode::<Everything>(&payload[..cut], &mut table),
            Err(BadMessage::Rejected),
            "cut at {cut}"
        );
    }
    assert_eq!(codec::decode::<Everything>(&payload, &mut table), Ok(value));
    // Bytes said to be longer than the message is.
    assert_eq!(
        codec::decode::<Wrapper>(&[0, 0xff, 0xff, 0xff, 0x7f, 1], &mut table),
        Err(BadMessage::Rejected)
    );
}

/// A value that holds itself, in each way serde nests one.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum Nested {
    Leaf,
    List(Vec<Self>),
    Maybe(Option<Box<Self>>),
    Boxed(Box<Self>),
    Field { inner: Box<Self> },
    Pair(u8, Box<Self>),
    Map(BTreeMap<u8, Self>),
}

fn nested(levels: usize, wrap: impl Fn(Nested) -> Nested) -> Nested {
    (0..levels).fold(Nested::Leaf, |inner, _| wrap(inner))
}

#[test]
fn a_value_nested_too_deep_is_refused_before_the_stack_is() {
    type Wrap = fn(Nested) -> Nested;
    let wraps: [(&str, usize, Wrap); 6] = [
        // Each wrap is this many levels: the enum, and what its variant
        // holds.
        ("list", 2, |n| Nested::List(vec![n])),
        ("option", 2, |n| Nested::Maybe(Some(Box::new(n)))),
        ("newtype variant", 1, |n| Nested::Boxed(Box::new(n))),
        ("struct variant", 1, |n| Nested::Field {
            inner: Box::new(n),
        }),
        ("tuple variant", 1, |n| Nested::Pair(0, Box::new(n))),
        ("map", 2, |n| Nested::Map(BTreeMap::from([(0, n)]))),
    ];
    for (name, levels, wrap) in wraps {
        // The leaf is an enum too: one level.
        let most = (MAX_DEPTH - 1) / levels;
        let fits = nested(most, wrap);
        let payload = encoded(&fits, &mut NoRegions);
        assert_eq!(
            codec::decode::<Nested>(&payload, &mut NoRegions),
            Ok(fits),
            "{name}"
        );
        let payload = encoded(&nested(most + 1, wrap), &mut NoRegions);
        assert_eq!(
            codec::decode::<Nested>(&payload, &mut NoRegions),
            Err(BadMessage::TooDeep),
            "{name}"
        );
    }
    // Far past the limit, as bytes (a value that deep can't be built or
    // dropped on this stack either): refused without going there.
    let one = encoded(&Nested::List(vec![Nested::Leaf]), &mut NoRegions);
    let (wrap, leaf) = one.split_at(one.len() - 1);
    let mut payload = wrap.repeat(1_000_000);
    payload.extend(leaf);
    assert_eq!(
        codec::decode::<Nested>(&payload, &mut NoRegions),
        Err(BadMessage::TooDeep)
    );
}

#[test]
fn levels_are_counted_down_as_well_as_up() {
    // Wide, not deep: far more nested values than MAX_DEPTH, none deep.
    let wide = Nested::List(
        (0..10 * MAX_DEPTH)
            .map(|_| Nested::List(vec![Nested::Maybe(None)]))
            .collect(),
    );
    let payload = encoded(&wide, &mut NoRegions);
    assert_eq!(codec::decode::<Nested>(&payload, &mut NoRegions), Ok(wide));
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Wide(Vec<(u8, Count)>, Vec<Option<Count>>, BTreeMap<u8, Marker>);
    let wide = Wide(
        (0..200).map(|i| (i, Count(1))).collect(),
        (0..200).map(|i| Some(Count(i))).collect(),
        (0..200).map(|i| (i, Marker)).collect(),
    );
    let payload = encoded(&wide, &mut NoRegions);
    assert_eq!(codec::decode::<Wide>(&payload, &mut NoRegions), Ok(wide));
}
