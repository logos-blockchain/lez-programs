//! Slice-bounded decoding for the pinned RISC Zero instruction format.
//!
//! RISC Zero 3.0.5 allocates strings/byte buffers from their length prefix before
//! checking for truncated input. Delegate leaf values only after checking their
//! encoded extent. Containers must recurse through this decoder so nested strings
//! receive the same check. The format and acceptance of trailing words are unchanged.

use risc0_zkvm::serde::{Deserializer as Risc0Deserializer, Error};
use serde::de::{
    DeserializeOwned, DeserializeSeed, Deserializer, EnumAccess, IntoDeserializer, MapAccess,
    SeqAccess, VariantAccess, Visitor,
};

/// Decode RISC Zero instruction words without allocating from unchecked byte lengths.
///
/// Preserves the pinned RISC Zero encoding, including padding and trailing words.
/// Actual input size still determines memory and cycle requirements.
///
/// # Errors
/// Returns the RISC Zero decoding error for malformed or truncated input, including
/// a string or byte-buffer length that exceeds the available instruction words.
pub fn decode_instruction<T: DeserializeOwned>(words: &[u32]) -> Result<T, Error> {
    T::deserialize(&mut Decoder { words })
}

struct Decoder<'de> {
    words: &'de [u32],
}

impl<'de> Decoder<'de> {
    fn take_words(&mut self, count: usize) -> Result<&'de [u32], Error> {
        let (value, rest) = self
            .words
            .split_at_checked(count)
            .ok_or(Error::DeserializeUnexpectedEnd)?;
        self.words = rest;
        Ok(value)
    }

    fn word(&mut self) -> Result<u32, Error> {
        let (&value, rest) = self
            .words
            .split_first()
            .ok_or(Error::DeserializeUnexpectedEnd)?;
        self.words = rest;
        Ok(value)
    }

    fn length(&mut self) -> Result<usize, Error> {
        usize::try_from(self.word()?).map_err(|_| Error::DeserializeUnexpectedEnd)
    }

    fn take_byte_words(&mut self) -> Result<&'de [u32], Error> {
        let &length = self.words.first().ok_or(Error::DeserializeUnexpectedEnd)?;
        let length = usize::try_from(length).map_err(|_| Error::DeserializeUnexpectedEnd)?;
        let count = length
            .div_ceil(size_of::<u32>())
            .checked_add(1)
            .ok_or(Error::DeserializeUnexpectedEnd)?;
        // Include the prefix and word padding. No allocation happens until the
        // entire byte payload is known to be present in the instruction frame.
        self.take_words(count)
    }
}

macro_rules! delegate_leaf {
    ($($method:ident($take:ident $(, $count:expr)?)),* $(,)?) => {
        $(
            fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
                let words = self.$take($($count)?)?;
                Risc0Deserializer::new(words).$method(visitor)
            }
        )*
    };
}

impl<'de> Deserializer<'de> for &mut Decoder<'de> {
    type Error = Error;

    delegate_leaf! {
        deserialize_bool(take_words, 1),
        deserialize_i8(take_words, 1),
        deserialize_i16(take_words, 1),
        deserialize_i32(take_words, 1),
        deserialize_i64(take_words, 2),
        deserialize_i128(take_words, 4),
        deserialize_u8(take_words, 1),
        deserialize_u16(take_words, 1),
        deserialize_u32(take_words, 1),
        deserialize_u64(take_words, 2),
        deserialize_u128(take_words, 4),
        deserialize_f32(take_words, 1),
        deserialize_f64(take_words, 2),
        deserialize_char(take_words, 1),
        deserialize_unit(take_words, 0),
        deserialize_str(take_byte_words),
        deserialize_string(take_byte_words),
        deserialize_bytes(take_byte_words),
        deserialize_byte_buf(take_byte_words),
    }

    serde::forward_to_deserialize_any! { identifier ignored_any }

    fn is_human_readable(&self) -> bool {
        false
    }

    fn deserialize_any<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, Error> {
        Err(Error::NotSupported)
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.word()? {
            0 => visitor.visit_none(),
            1 => visitor.visit_some(self),
            _ => Err(Error::DeserializeBadOption),
        }
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.deserialize_unit(visitor)
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        let remaining = self.length()?;
        self.deserialize_tuple(remaining, visitor)
    }

    fn deserialize_tuple<V: Visitor<'de>>(
        self,
        remaining: usize,
        visitor: V,
    ) -> Result<V::Value, Error> {
        visitor.visit_seq(Compound {
            decoder: self,
            remaining,
        })
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.deserialize_tuple(len, visitor)
    }

    fn deserialize_map<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        let remaining = self.length()?;
        visitor.visit_map(Compound {
            decoder: self,
            remaining,
        })
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.deserialize_tuple(fields.len(), visitor)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        visitor.visit_enum(self)
    }
}

struct Compound<'a, 'de> {
    decoder: &'a mut Decoder<'de>,
    remaining: usize,
}

impl<'de> SeqAccess<'de> for Compound<'_, 'de> {
    type Error = Error;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Error> {
        let Some(remaining) = self.remaining.checked_sub(1) else {
            return Ok(None);
        };
        self.remaining = remaining;
        seed.deserialize(&mut *self.decoder).map(Some)
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.remaining)
    }
}

impl<'de> MapAccess<'de> for Compound<'_, 'de> {
    type Error = Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Error> {
        self.next_element_seed(seed)
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Error> {
        seed.deserialize(&mut *self.decoder)
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.remaining)
    }
}

impl<'de> EnumAccess<'de> for &mut Decoder<'de> {
    type Error = Error;
    type Variant = Self;

    fn variant_seed<V: DeserializeSeed<'de>>(self, seed: V) -> Result<(V::Value, Self), Error> {
        let tag = self.word()?;
        let variant = seed.deserialize(tag.into_deserializer())?;
        Ok((variant, self))
    }
}

impl<'de> VariantAccess<'de> for &mut Decoder<'de> {
    type Error = Error;

    fn unit_variant(self) -> Result<(), Error> {
        Ok(())
    }

    fn newtype_variant_seed<V: DeserializeSeed<'de>>(self, seed: V) -> Result<V::Value, Error> {
        seed.deserialize(self)
    }

    fn tuple_variant<V: Visitor<'de>>(self, len: usize, visitor: V) -> Result<V::Value, Error> {
        self.deserialize_tuple(len, visitor)
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.deserialize_tuple(fields.len(), visitor)
    }
}
