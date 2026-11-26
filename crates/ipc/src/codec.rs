//! A message's value to and from a payload's bytes (the place of Mojo's
//! generated serialization and its message context): postcard, with two
//! things only this layer can do. A run of bytes
//! ([`serde::Serializer::serialize_bytes`], which is how a `BigBuffer`
//! writes itself) of [`MIN_REGION_LEN`] or more goes into a region kept
//! for it when there is a free one, and the payload holds the region's
//! number and the length. And a value read back may nest only
//! [`MAX_DEPTH`] deep: postcard has no limit of its own, and a peer's
//! value nested deeper than the stack is long ends the process, which no
//! handler catches. And a data pipe's consumer
//! ([`crate::data_pipe::Consumer`]) in a value goes with it: a pipe over
//! the channel is opened for it and the payload holds the pipe's number,
//! which reads back as the consumer of the pipe the other end made.
//! And an open file ([`crate::file::ReadOnlyFile`]) in a value goes with
//! it: its handle is one of the message's, and the payload holds which.
//!
//! Both are wrappers around serde's serializer and deserializer that pass
//! everything else through, so the regions are arguments, not state
//! somewhere a value's `serialize` has to find.

use std::cell::RefCell;
use std::fmt;
use std::fs::File;

use serde::de::{
    self, DeserializeOwned, DeserializeSeed, Deserializer, EnumAccess, MapAccess, SeqAccess,
    VariantAccess, Visitor,
};
use serde::ser::{self, Serialize, Serializer};

use crate::core::frame::BadMessage;
use crate::core::region::{MIN_REGION_LEN, Memory, Pool, Table};
use crate::data_pipe::{self, CONSUMER_NAME, Consumer};
use crate::file::{self as open_file, FILE_NAME};
use crate::shared_buffer::{self, BUFFER_NAME, LENT_NAME};

/// How deep a value read from a peer may nest: every sequence, map,
/// struct, tuple, enum, option and newtype is a level. machopo's messages
/// nest a dozen deep by their types; what goes deeper is a value that
/// holds itself (a WebDriver value's lists of lists).
pub const MAX_DEPTH: usize = 128;

/// Where large bytes go on their way out.
pub trait Place {
    /// `bytes` copied to a region the peer reads: its number. `None`
    /// leaves them in the message.
    fn place(&mut self, bytes: &[u8]) -> Option<u32>;

    /// The message `number` was placed for isn't going: the region is
    /// free again.
    fn unplace(&mut self, number: u32);

    /// The message names the region `number`, which `owner`'s taker
    /// wrote in place: out from now as one bytes were placed in. `false`
    /// when it isn't a region of this end's that a taker holds.
    fn lend(&mut self, owner: usize, number: u32) -> bool {
        let _ = (owner, number);
        false
    }

    /// Opens a pipe to the peer fed from `source`: its number. `None`
    /// when this end opens none (the value doesn't serialize, and the
    /// source is let go).
    fn pipe(&mut self, source: Consumer) -> Option<u32> {
        drop(source);
        None
    }

    /// The message the pipe `number` was opened for isn't going: the pipe
    /// is ended, and its source let go.
    fn unpipe(&mut self, number: u32) {
        let _ = number;
    }

    /// Attaches `file`'s handle to the message: which of its handles it
    /// is. `None` when this end attaches none (the value doesn't
    /// serialize, and the file goes). Nothing is taken back if the value
    /// fails after: the handles go with the message that isn't sent.
    fn file(&mut self, file: File) -> Option<u32> {
        drop(file);
        None
    }
}

/// Where large bytes are on their way in.
pub trait Resolve {
    /// The first `len` bytes of the peer's region `number`, copied to the
    /// end of `into`.
    fn resolve(&mut self, number: u32, len: u32, into: &mut Vec<u8>) -> Result<(), BadMessage>;

    /// The peer's region `number`, which the message says holds `len`
    /// bytes, left for the value being read
    /// (`shared_buffer::set_arriving`): not copied.
    fn hold(&mut self, number: u32, len: u32) -> Result<(), BadMessage> {
        let _ = len;
        Err(BadMessage::UnknownRegion { number })
    }

    /// The reading end of the pipe `number` the peer opened, which no
    /// message has named yet.
    fn pipe(&mut self, number: u32) -> Result<Consumer, BadMessage> {
        Err(BadMessage::UnknownPipe { pipe: number })
    }

    /// The file that is the message's handle `index`, which no part of
    /// the value has taken yet.
    fn file(&mut self, index: u32) -> Result<File, BadMessage> {
        Err(BadMessage::UnknownHandle { index })
    }
}

impl<M: Memory> Place for Pool<M> {
    fn place(&mut self, bytes: &[u8]) -> Option<u32> {
        self.put(bytes)
    }

    fn unplace(&mut self, number: u32) {
        self.give_back(number);
    }
}

impl<M: Memory> Resolve for Table<M> {
    fn resolve(&mut self, number: u32, len: u32, into: &mut Vec<u8>) -> Result<(), BadMessage> {
        self.read(number, len, into)
    }
}

/// An end with no regions: everything goes in the message, and a message
/// that names a region is a bad one.
pub struct NoRegions;

impl Place for NoRegions {
    fn place(&mut self, _: &[u8]) -> Option<u32> {
        None
    }

    fn unplace(&mut self, _: u32) {}
}

impl Resolve for NoRegions {
    fn resolve(&mut self, number: u32, _: u32, _: &mut Vec<u8>) -> Result<(), BadMessage> {
        Err(BadMessage::UnknownRegion { number })
    }
}

/// A value that couldn't be written: its own `serialize` refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncodeError;

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a value that doesn't serialize")
    }
}

impl std::error::Error for EncodeError {}

/// `value` as a payload, after what `out` has; its large bytes in
/// `regions`, and its pipes opened there. When it can't be written, `out`
/// is as it was, nothing stays placed, and the pipes opened for it are
/// ended (the value has lost them all the same).
pub fn encode<T: Serialize + ?Sized>(
    value: &T,
    regions: &mut dyn Place,
    out: &mut Vec<u8>,
) -> Result<(), EncodeError> {
    let before = out.len();
    let placing = RefCell::new(Placing {
        regions,
        placed: Vec::new(),
        piped: Vec::new(),
    });
    let wrapped = Wrapped {
        value,
        placing: &placing,
    };
    let written = postcard::to_extend(&wrapped, Tail(out)).map(drop);
    let Placing {
        regions,
        placed,
        piped,
    } = placing.into_inner();
    if written.is_err() {
        for number in placed {
            regions.unplace(number);
        }
        for number in piped {
            regions.unpipe(number);
        }
        out.truncate(before);
    }
    written.map_err(|_| EncodeError)
}

/// The end of a vector, as what postcard writes to: the vector stays the
/// caller's whether the value serializes or not.
struct Tail<'a>(&'a mut Vec<u8>);

impl Extend<u8> for Tail<'_> {
    fn extend<I: IntoIterator<Item = u8>>(&mut self, bytes: I) {
        self.0.extend(bytes);
    }
}

/// A payload as the value it is; its large bytes copied out of `regions`.
/// A payload that isn't one value of `T` and nothing more is a bad
/// message.
pub fn decode<T: DeserializeOwned>(
    payload: &[u8],
    regions: &mut dyn Resolve,
) -> Result<T, BadMessage> {
    let mut resolving = Resolving {
        regions,
        depth: 0,
        bad: None,
    };
    let mut postcard = postcard::Deserializer::from_bytes(payload);
    let value = T::deserialize(De {
        inner: &mut postcard,
        resolving: &mut resolving,
    });
    match (value, postcard.finalize()) {
        (Ok(value), Ok([])) => Ok(value),
        _ => Err(resolving.bad.unwrap_or(BadMessage::Rejected)),
    }
}

// How a run of bytes is written: an enum of these two variants, in the
// serializer's own encoding of one.
const NAME: &str = "BigBuffer";
const VARIANTS: &[&str] = &["Inline", "Region"];
const INLINE: u32 = 0;
const REGION: u32 = 1;

/// What one `encode` placed, to take back if the value fails after.
struct Placing<'r> {
    regions: &'r mut dyn Place,
    placed: Vec<u32>,
    piped: Vec<u32>,
}

// A `RefCell`, not `&mut`: serde hands a nested value to
// `Serialize::serialize(&self)`, so what the levels share can only be
// reached through `&`. It is borrowed once per run of large bytes.
type PlacingRef<'a, 'r> = &'a RefCell<Placing<'r>>;

/// A value that serializes through [`Ser`] wherever it is nested.
struct Wrapped<'a, 'r, T: ?Sized> {
    value: &'a T,
    placing: PlacingRef<'a, 'r>,
}

impl<T: Serialize + ?Sized> Serialize for Wrapped<'_, '_, T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.value.serialize(Ser {
            inner: serializer,
            placing: self.placing,
        })
    }
}

/// Bytes written as bytes, past the wrapper.
struct Raw<'a>(&'a [u8]);

impl Serialize for Raw<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(self.0)
    }
}

/// The serializer under it, but for runs of bytes.
struct Ser<'a, 'r, S> {
    inner: S,
    placing: PlacingRef<'a, 'r>,
}

macro_rules! pass_through {
    ($($method:ident($ty:ty))*) => {$(
        fn $method(self, v: $ty) -> Result<S::Ok, S::Error> {
            self.inner.$method(v)
        }
    )*};
}

impl<'a, 'r, S: Serializer> Serializer for Ser<'a, 'r, S> {
    type Ok = S::Ok;
    type Error = S::Error;
    type SerializeSeq = Compound<'a, 'r, S::SerializeSeq>;
    type SerializeTuple = Compound<'a, 'r, S::SerializeTuple>;
    type SerializeTupleStruct = Compound<'a, 'r, S::SerializeTupleStruct>;
    type SerializeTupleVariant = Compound<'a, 'r, S::SerializeTupleVariant>;
    type SerializeMap = Compound<'a, 'r, S::SerializeMap>;
    type SerializeStruct = Compound<'a, 'r, S::SerializeStruct>;
    type SerializeStructVariant = Compound<'a, 'r, S::SerializeStructVariant>;

    pass_through! {
        serialize_bool(bool) serialize_i8(i8) serialize_i16(i16) serialize_i32(i32)
        serialize_i64(i64) serialize_i128(i128) serialize_u8(u8) serialize_u16(u16)
        serialize_u32(u32) serialize_u64(u64) serialize_u128(u128) serialize_f32(f32)
        serialize_f64(f64) serialize_char(char) serialize_str(&str)
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<S::Ok, S::Error> {
        if v.len() >= MIN_REGION_LEN
            && let Ok(len) = u32::try_from(v.len())
        {
            let mut placing = self.placing.borrow_mut();
            if let Some(number) = placing.regions.place(v) {
                placing.placed.push(number);
                drop(placing);
                let mut region = self
                    .inner
                    .serialize_tuple_variant(NAME, REGION, "Region", 2)?;
                ser::SerializeTupleVariant::serialize_field(&mut region, &number)?;
                ser::SerializeTupleVariant::serialize_field(&mut region, &len)?;
                return ser::SerializeTupleVariant::end(region);
            }
        }
        self.inner
            .serialize_newtype_variant(NAME, INLINE, "Inline", &Raw(v))
    }

    fn serialize_none(self) -> Result<S::Ok, S::Error> {
        self.inner.serialize_none()
    }

    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<S::Ok, S::Error> {
        self.inner.serialize_some(&Wrapped {
            value,
            placing: self.placing,
        })
    }

    fn serialize_unit(self) -> Result<S::Ok, S::Error> {
        self.inner.serialize_unit()
    }

    fn serialize_unit_struct(self, name: &'static str) -> Result<S::Ok, S::Error> {
        self.inner.serialize_unit_struct(name)
    }

    fn serialize_unit_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
    ) -> Result<S::Ok, S::Error> {
        self.inner.serialize_unit_variant(name, index, variant)
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        name: &'static str,
        value: &T,
    ) -> Result<S::Ok, S::Error> {
        // A region written in place, which left its number beside the
        // value: written as bytes placed in a region are, with nothing
        // placed.
        if name == LENT_NAME {
            let lending = shared_buffer::take_lending()
                .ok_or_else(|| ser::Error::custom("no region to send"))?;
            let (owner, number, len) = lending;
            let mut placing = self.placing.borrow_mut();
            let len = u32::try_from(len).ok();
            let (Some(len), true) = (len, placing.regions.lend(owner, number)) else {
                // The value keeps its region.
                shared_buffer::set_lending(lending);
                return Err(ser::Error::custom("not a region of this channel's"));
            };
            placing.placed.push(number);
            drop(placing);
            let mut region = self
                .inner
                .serialize_tuple_variant(NAME, REGION, "Region", 2)?;
            ser::SerializeTupleVariant::serialize_field(&mut region, &number)?;
            ser::SerializeTupleVariant::serialize_field(&mut region, &len)?;
            return ser::SerializeTupleVariant::end(region);
        }
        // A data pipe's consumer, which left its pipe beside the value:
        // the pipe is opened to the peer, and its number is what is
        // written. Another type that calls itself by the name left none.
        if name == CONSUMER_NAME {
            let source = data_pipe::take_crossing()
                .ok_or_else(|| ser::Error::custom("no data pipe to send"))?;
            let mut placing = self.placing.borrow_mut();
            let number = placing
                .regions
                .pipe(source)
                .ok_or_else(|| ser::Error::custom("this end opens no pipe"))?;
            placing.piped.push(number);
            drop(placing);
            return self.inner.serialize_newtype_struct(name, &number);
        }
        // An open file, which left another handle to it beside the value:
        // the handle goes with the message, and which of its handles it
        // is is what is written.
        if name == FILE_NAME {
            let file =
                open_file::take_crossing().ok_or_else(|| ser::Error::custom("no file to send"))?;
            let index = self
                .placing
                .borrow_mut()
                .regions
                .file(file)
                .ok_or_else(|| ser::Error::custom("this end attaches no files"))?;
            return self.inner.serialize_newtype_struct(name, &index);
        }
        self.inner.serialize_newtype_struct(
            name,
            &Wrapped {
                value,
                placing: self.placing,
            },
        )
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<S::Ok, S::Error> {
        self.inner.serialize_newtype_variant(
            name,
            index,
            variant,
            &Wrapped {
                value,
                placing: self.placing,
            },
        )
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq, S::Error> {
        Ok(Compound {
            inner: self.inner.serialize_seq(len)?,
            placing: self.placing,
        })
    }

    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, S::Error> {
        Ok(Compound {
            inner: self.inner.serialize_tuple(len)?,
            placing: self.placing,
        })
    }

    fn serialize_tuple_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct, S::Error> {
        Ok(Compound {
            inner: self.inner.serialize_tuple_struct(name, len)?,
            placing: self.placing,
        })
    }

    fn serialize_tuple_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleVariant, S::Error> {
        Ok(Compound {
            inner: self
                .inner
                .serialize_tuple_variant(name, index, variant, len)?,
            placing: self.placing,
        })
    }

    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap, S::Error> {
        Ok(Compound {
            inner: self.inner.serialize_map(len)?,
            placing: self.placing,
        })
    }

    fn serialize_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStruct, S::Error> {
        Ok(Compound {
            inner: self.inner.serialize_struct(name, len)?,
            placing: self.placing,
        })
    }

    fn serialize_struct_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStructVariant, S::Error> {
        Ok(Compound {
            inner: self
                .inner
                .serialize_struct_variant(name, index, variant, len)?,
            placing: self.placing,
        })
    }

    fn is_human_readable(&self) -> bool {
        self.inner.is_human_readable()
    }
}

/// A sequence, tuple, map or struct being written: each part goes through
/// [`Ser`].
struct Compound<'a, 'r, C> {
    inner: C,
    placing: PlacingRef<'a, 'r>,
}

impl<C: ser::SerializeSeq> ser::SerializeSeq for Compound<'_, '_, C> {
    type Ok = C::Ok;
    type Error = C::Error;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), C::Error> {
        let placing = self.placing;
        self.inner.serialize_element(&Wrapped { value, placing })
    }

    fn end(self) -> Result<C::Ok, C::Error> {
        self.inner.end()
    }
}

impl<C: ser::SerializeTuple> ser::SerializeTuple for Compound<'_, '_, C> {
    type Ok = C::Ok;
    type Error = C::Error;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), C::Error> {
        let placing = self.placing;
        self.inner.serialize_element(&Wrapped { value, placing })
    }

    fn end(self) -> Result<C::Ok, C::Error> {
        self.inner.end()
    }
}

impl<C: ser::SerializeTupleStruct> ser::SerializeTupleStruct for Compound<'_, '_, C> {
    type Ok = C::Ok;
    type Error = C::Error;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), C::Error> {
        let placing = self.placing;
        self.inner.serialize_field(&Wrapped { value, placing })
    }

    fn end(self) -> Result<C::Ok, C::Error> {
        self.inner.end()
    }
}

impl<C: ser::SerializeTupleVariant> ser::SerializeTupleVariant for Compound<'_, '_, C> {
    type Ok = C::Ok;
    type Error = C::Error;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), C::Error> {
        let placing = self.placing;
        self.inner.serialize_field(&Wrapped { value, placing })
    }

    fn end(self) -> Result<C::Ok, C::Error> {
        self.inner.end()
    }
}

impl<C: ser::SerializeMap> ser::SerializeMap for Compound<'_, '_, C> {
    type Ok = C::Ok;
    type Error = C::Error;

    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), C::Error> {
        let placing = self.placing;
        self.inner.serialize_key(&Wrapped {
            value: key,
            placing,
        })
    }

    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), C::Error> {
        let placing = self.placing;
        self.inner.serialize_value(&Wrapped { value, placing })
    }

    fn end(self) -> Result<C::Ok, C::Error> {
        self.inner.end()
    }
}

impl<C: ser::SerializeStruct> ser::SerializeStruct for Compound<'_, '_, C> {
    type Ok = C::Ok;
    type Error = C::Error;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), C::Error> {
        let placing = self.placing;
        self.inner.serialize_field(key, &Wrapped { value, placing })
    }

    fn skip_field(&mut self, key: &'static str) -> Result<(), C::Error> {
        self.inner.skip_field(key)
    }

    fn end(self) -> Result<C::Ok, C::Error> {
        self.inner.end()
    }
}

impl<C: ser::SerializeStructVariant> ser::SerializeStructVariant for Compound<'_, '_, C> {
    type Ok = C::Ok;
    type Error = C::Error;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), C::Error> {
        let placing = self.placing;
        self.inner.serialize_field(key, &Wrapped { value, placing })
    }

    fn skip_field(&mut self, key: &'static str) -> Result<(), C::Error> {
        self.inner.skip_field(key)
    }

    fn end(self) -> Result<C::Ok, C::Error> {
        self.inner.end()
    }
}

/// What one `decode` reads through: the regions, how deep it is, and what
/// was wrong, when it is more than "not this type".
struct Resolving<'r> {
    regions: &'r mut dyn Resolve,
    depth: usize,
    bad: Option<BadMessage>,
}

impl Resolving<'_> {
    /// One level in. Past [`MAX_DEPTH`] the value is refused.
    fn enter<E: de::Error>(&mut self) -> Result<(), E> {
        if self.depth >= MAX_DEPTH {
            self.bad.get_or_insert(BadMessage::TooDeep);
            return Err(E::custom("nested too deep"));
        }
        self.depth = self.depth.saturating_add(1);
        Ok(())
    }

    fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }
}

/// The deserializer under it, but for runs of bytes, and counting levels.
struct De<'a, 'r, D> {
    inner: D,
    resolving: &'a mut Resolving<'r>,
}

macro_rules! leaves {
    ($($method:ident)*) => {$(
        fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, D::Error> {
            self.inner.$method(visitor)
        }
    )*};
}

/// A level: the visitor's nested values come back through [`De`], one
/// deeper.
macro_rules! level {
    ($self:ident, $visitor:ident, |$inner:ident, $wrapped:ident| $call:expr) => {{
        let resolving = $self.resolving;
        resolving.enter()?;
        let $inner = $self.inner;
        let $wrapped = Vis {
            visitor: $visitor,
            resolving: &mut *resolving,
        };
        let value = $call;
        resolving.leave();
        value
    }};
}

impl<'de, D: Deserializer<'de>> Deserializer<'de> for De<'_, '_, D> {
    type Error = D::Error;

    leaves! {
        deserialize_bool deserialize_i8 deserialize_i16 deserialize_i32 deserialize_i64
        deserialize_i128 deserialize_u8 deserialize_u16 deserialize_u32 deserialize_u64
        deserialize_u128 deserialize_f32 deserialize_f64 deserialize_char deserialize_str
        deserialize_string deserialize_unit deserialize_identifier
    }

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, D::Error> {
        level!(self, visitor, |inner, wrapped| inner
            .deserialize_any(wrapped))
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, D::Error> {
        level!(self, visitor, |inner, wrapped| inner
            .deserialize_ignored_any(wrapped))
    }

    fn deserialize_bytes<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, D::Error> {
        self.inner.deserialize_enum(
            NAME,
            VARIANTS,
            Run {
                visitor,
                resolving: self.resolving,
                hold: false,
            },
        )
    }

    fn deserialize_byte_buf<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, D::Error> {
        self.deserialize_bytes(visitor)
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, D::Error> {
        level!(self, visitor, |inner, wrapped| inner
            .deserialize_option(wrapped))
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, D::Error> {
        self.inner.deserialize_unit_struct(name, visitor)
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, D::Error> {
        // A data pipe's consumer: the number the peer wrote is the pipe
        // it opened, whose reading end goes beside the number to the
        // visitor that asked by this name.
        if name == CONSUMER_NAME {
            let number = <u32 as de::Deserialize>::deserialize(self.inner)?;
            let consumer = match self.resolving.regions.pipe(number) {
                Ok(consumer) => consumer,
                Err(bad) => {
                    self.resolving.bad.get_or_insert(bad);
                    return Err(de::Error::custom("no such pipe"));
                }
            };
            data_pipe::set_crossing(consumer);
            let value = visitor.visit_u32(number);
            // A visitor that isn't a consumer's took none: it goes.
            drop(data_pipe::take_crossing());
            return value;
        }
        // An open file: the index the peer wrote is which of the
        // message's handles it is, which goes beside the index to the
        // visitor that asked by this name.
        if name == FILE_NAME {
            let index = <u32 as de::Deserialize>::deserialize(self.inner)?;
            let file = match self.resolving.regions.file(index) {
                Ok(file) => file,
                Err(bad) => {
                    self.resolving.bad.get_or_insert(bad);
                    return Err(de::Error::custom("no such handle"));
                }
            };
            open_file::set_crossing(file);
            let value = visitor.visit_u32(index);
            // A visitor that isn't a file's took none: it goes.
            drop(open_file::take_crossing());
            return value;
        }
        // Bytes whose region isn't copied out of: the region goes beside
        // its number to the visitor that asked by this name.
        if name == BUFFER_NAME {
            return self.inner.deserialize_enum(
                NAME,
                VARIANTS,
                Run {
                    visitor,
                    resolving: self.resolving,
                    hold: true,
                },
            );
        }
        level!(self, visitor, |inner, wrapped| inner
            .deserialize_newtype_struct(name, wrapped))
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, D::Error> {
        level!(self, visitor, |inner, wrapped| inner
            .deserialize_seq(wrapped))
    }

    fn deserialize_tuple<V: Visitor<'de>>(
        self,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, D::Error> {
        level!(self, visitor, |inner, wrapped| inner
            .deserialize_tuple(len, wrapped))
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, D::Error> {
        level!(self, visitor, |inner, wrapped| inner
            .deserialize_tuple_struct(name, len, wrapped))
    }

    fn deserialize_map<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, D::Error> {
        level!(self, visitor, |inner, wrapped| inner
            .deserialize_map(wrapped))
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, D::Error> {
        level!(self, visitor, |inner, wrapped| inner
            .deserialize_struct(name, fields, wrapped))
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, D::Error> {
        level!(self, visitor, |inner, wrapped| inner
            .deserialize_enum(name, variants, wrapped))
    }

    fn is_human_readable(&self) -> bool {
        self.inner.is_human_readable()
    }
}

/// A seed whose value is read through [`De`].
struct Seed<'a, 'r, S> {
    seed: S,
    resolving: &'a mut Resolving<'r>,
}

impl<'de, S: DeserializeSeed<'de>> DeserializeSeed<'de> for Seed<'_, '_, S> {
    type Value = S::Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<S::Value, D::Error> {
        self.seed.deserialize(De {
            inner: deserializer,
            resolving: self.resolving,
        })
    }
}

/// A visitor whose nested values are read through [`De`].
struct Vis<'a, 'r, V> {
    visitor: V,
    resolving: &'a mut Resolving<'r>,
}

macro_rules! visits {
    ($($method:ident($ty:ty))*) => {$(
        fn $method<E: de::Error>(self, v: $ty) -> Result<V::Value, E> {
            self.visitor.$method(v)
        }
    )*};
}

impl<'de, V: Visitor<'de>> Visitor<'de> for Vis<'_, '_, V> {
    type Value = V::Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.visitor.expecting(f)
    }

    visits! {
        visit_bool(bool) visit_i8(i8) visit_i16(i16) visit_i32(i32) visit_i64(i64)
        visit_i128(i128) visit_u8(u8) visit_u16(u16) visit_u32(u32) visit_u64(u64)
        visit_u128(u128) visit_f32(f32) visit_f64(f64) visit_char(char) visit_str(&str)
        visit_borrowed_str(&'de str) visit_string(String) visit_bytes(&[u8])
        visit_borrowed_bytes(&'de [u8]) visit_byte_buf(Vec<u8>)
    }

    fn visit_none<E: de::Error>(self) -> Result<V::Value, E> {
        self.visitor.visit_none()
    }

    fn visit_unit<E: de::Error>(self) -> Result<V::Value, E> {
        self.visitor.visit_unit()
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<V::Value, D::Error> {
        self.visitor.visit_some(De {
            inner: deserializer,
            resolving: self.resolving,
        })
    }

    fn visit_newtype_struct<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<V::Value, D::Error> {
        self.visitor.visit_newtype_struct(De {
            inner: deserializer,
            resolving: self.resolving,
        })
    }

    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<V::Value, A::Error> {
        self.visitor.visit_seq(Access {
            inner: seq,
            resolving: self.resolving,
        })
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<V::Value, A::Error> {
        self.visitor.visit_map(Access {
            inner: map,
            resolving: self.resolving,
        })
    }

    fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<V::Value, A::Error> {
        self.visitor.visit_enum(Access {
            inner: data,
            resolving: self.resolving,
        })
    }
}

/// A sequence, map, enum or variant being read: each part goes through
/// [`De`].
struct Access<'a, 'r, A> {
    inner: A,
    resolving: &'a mut Resolving<'r>,
}

impl<'de, A: SeqAccess<'de>> SeqAccess<'de> for Access<'_, '_, A> {
    type Error = A::Error;

    fn next_element_seed<S: DeserializeSeed<'de>>(
        &mut self,
        seed: S,
    ) -> Result<Option<S::Value>, A::Error> {
        self.inner.next_element_seed(Seed {
            seed,
            resolving: &mut *self.resolving,
        })
    }

    fn size_hint(&self) -> Option<usize> {
        self.inner.size_hint()
    }
}

impl<'de, A: MapAccess<'de>> MapAccess<'de> for Access<'_, '_, A> {
    type Error = A::Error;

    fn next_key_seed<S: DeserializeSeed<'de>>(
        &mut self,
        seed: S,
    ) -> Result<Option<S::Value>, A::Error> {
        self.inner.next_key_seed(Seed {
            seed,
            resolving: &mut *self.resolving,
        })
    }

    fn next_value_seed<S: DeserializeSeed<'de>>(&mut self, seed: S) -> Result<S::Value, A::Error> {
        self.inner.next_value_seed(Seed {
            seed,
            resolving: &mut *self.resolving,
        })
    }

    fn size_hint(&self) -> Option<usize> {
        self.inner.size_hint()
    }
}

impl<'a, 'r, 'de, A: EnumAccess<'de>> EnumAccess<'de> for Access<'a, 'r, A> {
    type Error = A::Error;
    type Variant = Access<'a, 'r, A::Variant>;

    fn variant_seed<S: DeserializeSeed<'de>>(
        self,
        seed: S,
    ) -> Result<(S::Value, Self::Variant), A::Error> {
        let resolving = self.resolving;
        let (value, variant) = self.inner.variant_seed(Seed {
            seed,
            resolving: &mut *resolving,
        })?;
        Ok((
            value,
            Access {
                inner: variant,
                resolving,
            },
        ))
    }
}

impl<'de, A: VariantAccess<'de>> VariantAccess<'de> for Access<'_, '_, A> {
    type Error = A::Error;

    fn unit_variant(self) -> Result<(), A::Error> {
        self.inner.unit_variant()
    }

    fn newtype_variant_seed<S: DeserializeSeed<'de>>(self, seed: S) -> Result<S::Value, A::Error> {
        self.inner.newtype_variant_seed(Seed {
            seed,
            resolving: self.resolving,
        })
    }

    fn tuple_variant<V: Visitor<'de>>(self, len: usize, visitor: V) -> Result<V::Value, A::Error> {
        self.inner.tuple_variant(
            len,
            Vis {
                visitor,
                resolving: self.resolving,
            },
        )
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, A::Error> {
        self.inner.struct_variant(
            fields,
            Vis {
                visitor,
                resolving: self.resolving,
            },
        )
    }
}

/// A run of bytes being read for `visitor`: in the message, or in a
/// region.
struct Run<'a, 'r, V> {
    visitor: V,
    resolving: &'a mut Resolving<'r>,
    /// A region is handed over as it is, not copied out of.
    hold: bool,
}

impl<'de, V: Visitor<'de>> Visitor<'de> for Run<'_, '_, V> {
    type Value = V::Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("bytes, in the message or in a region")
    }

    fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<V::Value, A::Error> {
        match data.variant::<u32>()? {
            (INLINE, variant) => variant.newtype_variant_seed(Inline(self.visitor)),
            (REGION, variant) => variant.tuple_variant(2, self),
            _ => Err(de::Error::custom("neither in the message nor in a region")),
        }
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<V::Value, A::Error> {
        let (Some(number), Some(len)) = (seq.next_element::<u32>()?, seq.next_element::<u32>()?)
        else {
            return Err(de::Error::custom("a region's number and a length"));
        };
        if self.hold {
            return match self.resolving.regions.hold(number, len) {
                Ok(()) => {
                    let value = self.visitor.visit_u32(number);
                    // A visitor that isn't a buffer's took none: it goes.
                    drop(shared_buffer::take_arriving());
                    value
                }
                Err(bad) => {
                    self.resolving.bad.get_or_insert(bad);
                    Err(de::Error::custom(bad))
                }
            };
        }
        let mut bytes = Vec::new();
        match self.resolving.regions.resolve(number, len, &mut bytes) {
            Ok(()) => self.visitor.visit_byte_buf(bytes),
            Err(bad) => {
                self.resolving.bad.get_or_insert(bad);
                Err(de::Error::custom(bad))
            }
        }
    }
}

/// Bytes read as bytes, past the wrapper.
struct Inline<V>(V);

impl<'de, V: Visitor<'de>> DeserializeSeed<'de> for Inline<V> {
    type Value = V::Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<V::Value, D::Error> {
        deserializer.deserialize_bytes(self.0)
    }
}
