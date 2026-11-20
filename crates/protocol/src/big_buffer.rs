//! [`BigBuffer`]: the bytes a message carries.

use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

use bytes::Bytes;
use serde::de::{self, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Bytes in a message across the process boundary (Chromium's
/// `mojo_base::BigBuffer`): a response's body, a blob's content, a frame's
/// pixels. Every such field is one, however small its bytes usually are,
/// so that which bytes may leave a message for shared memory is a
/// property of the type, not of who remembered to ask for it.
///
/// Within one process it is a `Bytes`: cloned by a count, sliced without
/// a copy. It writes itself as one run of bytes (`serialize_bytes`), the
/// one place a channel between processes has to look to put a large one
/// elsewhere than in the message.
#[derive(Clone, Default, PartialEq, Eq, Hash)]
pub struct BigBuffer(Bytes);

impl BigBuffer {
    pub const fn new() -> Self {
        Self(Bytes::new())
    }

    pub const fn from_static(bytes: &'static [u8]) -> Self {
        Self(Bytes::from_static(bytes))
    }

    /// A copy of `bytes`.
    pub fn copy_from_slice(bytes: &[u8]) -> Self {
        Self(Bytes::copy_from_slice(bytes))
    }

    /// The same bytes as a `Bytes`, not copied.
    pub fn as_bytes(&self) -> &Bytes {
        &self.0
    }

    /// The same bytes as a `Bytes`, not copied.
    pub fn into_bytes(self) -> Bytes {
        self.0
    }
}

impl Deref for BigBuffer {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.0
    }
}

impl AsRef<[u8]> for BigBuffer {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

/// Its length and no more: a megabyte of pixels in a log helps nobody.
impl fmt::Debug for BigBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BigBuffer({} bytes)", self.0.len())
    }
}

impl From<Bytes> for BigBuffer {
    fn from(bytes: Bytes) -> Self {
        Self(bytes)
    }
}

impl From<BigBuffer> for Bytes {
    fn from(buffer: BigBuffer) -> Self {
        buffer.0
    }
}

/// The buffer's own allocation if nothing else holds it, a copy if
/// something does.
impl From<BigBuffer> for Vec<u8> {
    fn from(buffer: BigBuffer) -> Self {
        Self::from(buffer.0)
    }
}

/// The vector's own allocation, not a copy.
impl From<Vec<u8>> for BigBuffer {
    fn from(bytes: Vec<u8>) -> Self {
        Self(Bytes::from(bytes))
    }
}

/// The same allocation, held by one more count.
impl From<Arc<[u8]>> for BigBuffer {
    fn from(bytes: Arc<[u8]>) -> Self {
        Self(Bytes::from_owner(bytes))
    }
}

/// A copy of the array.
impl<const N: usize> From<[u8; N]> for BigBuffer {
    fn from(bytes: [u8; N]) -> Self {
        Self::copy_from_slice(&bytes)
    }
}

impl From<&'static [u8]> for BigBuffer {
    fn from(bytes: &'static [u8]) -> Self {
        Self::from_static(bytes)
    }
}

impl<const N: usize> From<&'static [u8; N]> for BigBuffer {
    fn from(bytes: &'static [u8; N]) -> Self {
        Self::from_static(bytes)
    }
}

impl From<&'static str> for BigBuffer {
    fn from(text: &'static str) -> Self {
        Self::from_static(text.as_bytes())
    }
}

impl From<String> for BigBuffer {
    fn from(text: String) -> Self {
        Self(Bytes::from(text))
    }
}

impl PartialEq<[u8]> for BigBuffer {
    fn eq(&self, other: &[u8]) -> bool {
        *self.0 == *other
    }
}

impl PartialEq<&[u8]> for BigBuffer {
    fn eq(&self, other: &&[u8]) -> bool {
        *self.0 == **other
    }
}

impl<const N: usize> PartialEq<[u8; N]> for BigBuffer {
    fn eq(&self, other: &[u8; N]) -> bool {
        *self.0 == other[..]
    }
}

impl<const N: usize> PartialEq<&[u8; N]> for BigBuffer {
    fn eq(&self, other: &&[u8; N]) -> bool {
        *self.0 == other[..]
    }
}

impl PartialEq<&str> for BigBuffer {
    fn eq(&self, other: &&str) -> bool {
        *self.0 == *other.as_bytes()
    }
}

impl PartialEq<Vec<u8>> for BigBuffer {
    fn eq(&self, other: &Vec<u8>) -> bool {
        *self.0 == other[..]
    }
}

impl Serialize for BigBuffer {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(&self.0)
    }
}

impl<'de> Deserialize<'de> for BigBuffer {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct BytesVisitor;

        impl<'de> Visitor<'de> for BytesVisitor {
            type Value = BigBuffer;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("bytes")
            }

            fn visit_bytes<E: de::Error>(self, bytes: &[u8]) -> Result<BigBuffer, E> {
                Ok(BigBuffer::copy_from_slice(bytes))
            }

            fn visit_byte_buf<E: de::Error>(self, bytes: Vec<u8>) -> Result<BigBuffer, E> {
                Ok(BigBuffer::from(bytes))
            }

            // A format with no bytes of its own (JSON) writes them as a
            // sequence of numbers.
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<BigBuffer, A::Error> {
                // What the sender says is coming isn't trusted with an
                // allocation: the vector grows as the bytes do come.
                let mut bytes = Vec::new();
                while let Some(byte) = seq.next_element::<u8>()? {
                    bytes.push(byte);
                }
                Ok(BigBuffer::from(bytes))
            }
        }

        deserializer.deserialize_byte_buf(BytesVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Made from what already holds the bytes, it is those bytes, not a
    /// copy of them; so is a clone of it, and the `Bytes` it gives back.
    #[test]
    fn it_shares_the_bytes_it_is_made_from() {
        let vec = vec![1u8, 2, 3];
        let at = vec.as_ptr();
        let buffer = BigBuffer::from(vec);
        assert_eq!(buffer.as_ptr(), at);
        assert_eq!(buffer.clone().as_ptr(), at);
        assert_eq!(buffer.as_bytes().as_ptr(), at);
        assert_eq!(buffer.into_bytes().as_ptr(), at);

        let arc: Arc<[u8]> = Arc::from([4u8, 5]);
        let buffer = BigBuffer::from(arc.clone());
        assert_eq!(buffer.as_ptr(), arc.as_ptr());
        assert_eq!(&buffer[..], [4, 5]);
        // Held for as long as the buffer is.
        assert_eq!(Arc::strong_count(&arc), 2);
        drop(buffer);
        assert_eq!(Arc::strong_count(&arc), 1);

        let bytes = Bytes::from_static(b"abc");
        assert_eq!(BigBuffer::from(bytes.clone()).as_ptr(), bytes.as_ptr());
    }

    /// It is written as one run of bytes, not a sequence of numbers each
    /// on its own: what a channel between processes looks for.
    #[test]
    fn it_is_serialized_as_one_run_of_bytes() {
        use serde::ser::Impossible;

        struct OnlyBytes;
        #[derive(Debug)]
        struct NotBytes;
        impl fmt::Display for NotBytes {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("not bytes")
            }
        }
        impl std::error::Error for NotBytes {}
        impl serde::ser::Error for NotBytes {
            fn custom<T: fmt::Display>(_: T) -> Self {
                NotBytes
            }
        }
        macro_rules! refuse {
            ($($name:ident($ty:ty))*) => {$(
                fn $name(self, _: $ty) -> Result<Vec<u8>, NotBytes> {
                    Err(NotBytes)
                }
            )*};
        }
        impl Serializer for OnlyBytes {
            type Ok = Vec<u8>;
            type Error = NotBytes;
            type SerializeSeq = Impossible<Vec<u8>, NotBytes>;
            type SerializeTuple = Impossible<Vec<u8>, NotBytes>;
            type SerializeTupleStruct = Impossible<Vec<u8>, NotBytes>;
            type SerializeTupleVariant = Impossible<Vec<u8>, NotBytes>;
            type SerializeMap = Impossible<Vec<u8>, NotBytes>;
            type SerializeStruct = Impossible<Vec<u8>, NotBytes>;
            type SerializeStructVariant = Impossible<Vec<u8>, NotBytes>;

            fn serialize_bytes(self, bytes: &[u8]) -> Result<Vec<u8>, NotBytes> {
                Ok(bytes.to_vec())
            }
            refuse! {
                serialize_bool(bool) serialize_i8(i8) serialize_i16(i16)
                serialize_i32(i32) serialize_i64(i64) serialize_u8(u8)
                serialize_u16(u16) serialize_u32(u32) serialize_u64(u64)
                serialize_f32(f32) serialize_f64(f64) serialize_char(char)
                serialize_str(&str) serialize_unit_struct(&'static str)
            }
            fn serialize_none(self) -> Result<Vec<u8>, NotBytes> {
                Err(NotBytes)
            }
            fn serialize_some<T: ?Sized + Serialize>(self, _: &T) -> Result<Vec<u8>, NotBytes> {
                Err(NotBytes)
            }
            fn serialize_unit(self) -> Result<Vec<u8>, NotBytes> {
                Err(NotBytes)
            }
            fn serialize_unit_variant(
                self,
                _: &'static str,
                _: u32,
                _: &'static str,
            ) -> Result<Vec<u8>, NotBytes> {
                Err(NotBytes)
            }
            fn serialize_newtype_struct<T: ?Sized + Serialize>(
                self,
                _: &'static str,
                _: &T,
            ) -> Result<Vec<u8>, NotBytes> {
                Err(NotBytes)
            }
            fn serialize_newtype_variant<T: ?Sized + Serialize>(
                self,
                _: &'static str,
                _: u32,
                _: &'static str,
                _: &T,
            ) -> Result<Vec<u8>, NotBytes> {
                Err(NotBytes)
            }
            fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, NotBytes> {
                Err(NotBytes)
            }
            fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, NotBytes> {
                Err(NotBytes)
            }
            fn serialize_tuple_struct(
                self,
                _: &'static str,
                _: usize,
            ) -> Result<Self::SerializeTupleStruct, NotBytes> {
                Err(NotBytes)
            }
            fn serialize_tuple_variant(
                self,
                _: &'static str,
                _: u32,
                _: &'static str,
                _: usize,
            ) -> Result<Self::SerializeTupleVariant, NotBytes> {
                Err(NotBytes)
            }
            fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, NotBytes> {
                Err(NotBytes)
            }
            fn serialize_struct(
                self,
                _: &'static str,
                _: usize,
            ) -> Result<Self::SerializeStruct, NotBytes> {
                Err(NotBytes)
            }
            fn serialize_struct_variant(
                self,
                _: &'static str,
                _: u32,
                _: &'static str,
                _: usize,
            ) -> Result<Self::SerializeStructVariant, NotBytes> {
                Err(NotBytes)
            }
        }

        let written = BigBuffer::from(vec![7u8, 8, 9]).serialize(OnlyBytes);
        assert_eq!(written.ok(), Some(vec![7, 8, 9]));
    }

    /// It comes back the same from a format that has no bytes of its own.
    #[test]
    fn it_round_trips_through_json() {
        let buffer = BigBuffer::from(vec![0u8, 255, 16]);
        let json = serde_json::to_string(&buffer).unwrap();
        assert_eq!(json, "[0,255,16]");
        assert_eq!(serde_json::from_str::<BigBuffer>(&json).unwrap(), buffer);
        let empty = serde_json::to_string(&BigBuffer::new()).unwrap();
        assert_eq!(serde_json::from_str::<BigBuffer>(&empty).unwrap().len(), 0);
    }

    #[test]
    fn its_debug_is_its_length() {
        assert_eq!(
            format!("{:?}", BigBuffer::from(vec![0u8; 5])),
            "BigBuffer(5 bytes)"
        );
    }
}
