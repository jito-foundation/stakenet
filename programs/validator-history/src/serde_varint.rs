// Copied from solana/sdk/program/src/serde_varint.rs
//! Integers that serialize to variable size.

#![allow(clippy::arithmetic_side_effects)]
use {
    serde::{
        de::{Error as _, SeqAccess, Visitor},
        ser::SerializeTuple,
        Deserializer, Serializer,
    },
    std::{fmt, marker::PhantomData},
};

pub trait VarInt: Sized {
    fn visit_seq<'de, A>(seq: A) -> Result<Self, A::Error>
    where
        A: SeqAccess<'de>;

    fn serialize<S>(self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer;
}

struct VarIntVisitor<T> {
    phantom: PhantomData<T>,
}

impl<'de, T> Visitor<'de> for VarIntVisitor<T>
where
    T: VarInt,
{
    type Value = T;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a VarInt")
    }

    fn visit_seq<A>(self, seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        T::visit_seq(seq)
    }
}

pub fn serialize<S, T>(value: &T, serializer: S) -> Result<S::Ok, S::Error>
where
    T: Copy + VarInt,
    S: Serializer,
{
    (*value).serialize(serializer)
}

pub fn deserialize<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: VarInt,
{
    deserializer.deserialize_tuple(
        (std::mem::size_of::<T>() * 8 + 6) / 7,
        VarIntVisitor {
            phantom: PhantomData,
        },
    )
}

macro_rules! impl_var_int {
    ($type:ty) => {
        impl VarInt for $type {
            fn visit_seq<'de, A>(mut seq: A) -> Result<Self, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut out = 0;
                let mut shift = 0u32;
                while shift < <$type>::BITS {
                    let byte = match seq.next_element::<u8>()? {
                        None => return Err(A::Error::custom("Invalid Sequence")),
                        Some(byte) => byte,
                    };
                    out |= ((byte & 0x7F) as Self) << shift;
                    if byte & 0x80 == 0 {
                        // Last byte should not have been truncated when it was
                        // shifted to the left above.
                        if (out >> shift) as u8 != byte {
                            return Err(A::Error::custom("Last Byte Truncated"));
                        }
                        // Last byte can be zero only if there was only one
                        // byte and the output is also zero.
                        if byte == 0u8 && (shift != 0 || out != 0) {
                            return Err(A::Error::custom("Invalid Trailing Zeros"));
                        }
                        return Ok(out);
                    }
                    shift += 7;
                }
                Err(A::Error::custom("Left Shift Overflows"))
            }

            fn serialize<S>(mut self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                let bits = <$type>::BITS - self.leading_zeros();
                let num_bytes = ((bits + 6) / 7).max(1) as usize;
                let mut seq = serializer.serialize_tuple(num_bytes)?;
                while self >= 0x80 {
                    let byte = ((self & 0x7F) | 0x80) as u8;
                    seq.serialize_element(&byte)?;
                    self >>= 7;
                }
                seq.serialize_element(&(self as u8))?;
                seq.end()
            }
        }
    };
}

impl_var_int!(u16);
impl_var_int!(u32);
impl_var_int!(u64);
