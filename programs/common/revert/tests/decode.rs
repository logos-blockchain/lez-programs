use std::{collections::BTreeMap, fmt};

use program_revert::decode_instruction;
use risc0_zkvm::serde::{from_slice, to_vec, Error};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Unit;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Newtype(String);

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Pair(String, String);

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum Nested {
    Unit,
    Newtype(Box<Newtype>),
    Tuple(Option<String>, Vec<String>),
    Struct {
        map: BTreeMap<String, String>,
        pair: Pair,
        array: [String; 2],
    },
}

fn assert_compatible<T: Serialize + DeserializeOwned + PartialEq + fmt::Debug>(value: T) {
    let words = to_vec(&value).expect("encode fixture");
    let old: T = from_slice(&words).expect("upstream decoder");
    let new: T = decode_instruction(&words).expect("bounded decoder");
    assert_eq!(old, value);
    assert_eq!(new, value);
}

#[test]
fn leaf_values_preserve_the_risc0_format() {
    assert_compatible((true, false, 'λ', (), Unit));
    assert_compatible((u8::MAX, u16::MAX, u32::MAX, u64::MAX, u128::MAX));
    assert_compatible((i8::MIN, i16::MIN, i32::MIN, i64::MIN, i128::MIN));
    assert_compatible((i8::MAX, i16::MAX, i32::MAX, i64::MAX, i128::MAX));
    assert_compatible((f32::MIN, f32::MAX, f64::MIN, f64::MAX));
    assert_compatible((None::<u32>, Some(u32::MAX), [0u32; 8]));
}

#[test]
fn strings_preserve_padding_and_trailing_word_acceptance() {
    for value in ["", "a", "ab", "abc", "abcd", "abcde", "λ🦀"] {
        assert_compatible(value.to_owned());
        let mut words = to_vec(value).expect("encode string");
        words.push(u32::MAX);
        assert_eq!(decode_instruction::<String>(&words).expect("string"), value);
        assert_eq!(from_slice::<String, _>(&words).expect("upstream"), value);
    }
    // Nonzero padding is accepted by the pinned decoder too.
    let words = [1, u32::from_ne_bytes([b'a', 0xff, 0xff, 0xff])];
    assert_eq!(decode_instruction::<String>(&words).expect("string"), "a");
    assert_eq!(from_slice::<String, _>(&words).expect("upstream"), "a");
}

#[test]
fn containers_preserve_the_risc0_format() {
    assert_compatible(Nested::Unit);
    assert_compatible(Nested::Newtype(Box::new(Newtype("name".into()))));
    assert_compatible(Nested::Tuple(
        Some("optional".into()),
        vec!["first".into(), "second".into()],
    ));
    assert_compatible(Nested::Struct {
        map: BTreeMap::from([("key".into(), "value".into())]),
        pair: Pair("a".into(), "b".into()),
        array: ["c".into(), "d".into()],
    });
    assert_compatible(Vec::<String>::new());
    assert_compatible(BTreeMap::<String, String>::new());
}

#[test]
fn truncated_string_lengths_return_errors_before_allocation() {
    for length in [1, 4, 5, 0x8000_0000, u32::MAX] {
        assert!(matches!(
            decode_instruction::<String>(&[length]),
            Err(Error::DeserializeUnexpectedEnd)
        ));
    }
    assert!(matches!(
        decode_instruction::<String>(&[5, u32::from_ne_bytes(*b"abcd")]),
        Err(Error::DeserializeUnexpectedEnd)
    ));
}

#[test]
fn every_container_checks_nested_string_lengths() {
    const MARKER: &str = "length-prefix-marker";

    fn assert_rejected<T: Serialize + DeserializeOwned>(value: T) {
        let mut words = to_vec(&value).expect("encode fixture");
        let marker_len = u32::try_from(MARKER.len()).expect("marker length");
        let length = words
            .iter_mut()
            .find(|word| **word == marker_len)
            .expect("fixture contains the unique marker length");
        *length = u32::MAX;
        assert!(matches!(
            decode_instruction::<T>(&words),
            Err(Error::DeserializeUnexpectedEnd)
        ));
    }

    assert_rejected(Some(MARKER.to_owned()));
    assert_rejected(vec![MARKER.to_owned()]);
    assert_rejected([MARKER.to_owned()]);
    assert_rejected(Newtype(MARKER.into()));
    assert_rejected(Pair("first".into(), MARKER.into()));
    assert_rejected(BTreeMap::from([(MARKER.to_owned(), String::new())]));
    assert_rejected(BTreeMap::from([(String::new(), MARKER.to_owned())]));
    assert_rejected(Nested::Newtype(Box::new(Newtype(MARKER.into()))));
    assert_rejected(Nested::Tuple(Some(MARKER.into()), vec![]));
    assert_rejected(Nested::Struct {
        map: BTreeMap::new(),
        pair: Pair("a".into(), "b".into()),
        array: ["first".into(), MARKER.into()],
    });
}

// Exercise Serde's packed-byte representation without adding a test dependency.
#[derive(Debug, PartialEq)]
struct Bytes(Vec<u8>);

impl Serialize for Bytes {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(&self.0)
    }
}

impl<'de> Deserialize<'de> for Bytes {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct BytesVisitor;

        impl serde::de::Visitor<'_> for BytesVisitor {
            type Value = Bytes;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a packed byte buffer")
            }

            fn visit_byte_buf<E: serde::de::Error>(self, value: Vec<u8>) -> Result<Bytes, E> {
                Ok(Bytes(value))
            }
        }

        deserializer.deserialize_byte_buf(BytesVisitor)
    }
}

#[test]
fn packed_byte_buffers_preserve_encoding_and_check_lengths() {
    for length in 0..=9 {
        assert_compatible(Bytes(vec![0xff; length]));
    }
    for length in [1, 4, 5, 0x8000_0000, u32::MAX] {
        assert!(matches!(
            decode_instruction::<Bytes>(&[length]),
            Err(Error::DeserializeUnexpectedEnd)
        ));
    }
}

#[test]
fn other_invalid_values_keep_upstream_errors() {
    fn assert_same_error<T: DeserializeOwned>(words: &[u32]) {
        let old = from_slice::<T, _>(words).err().expect("upstream rejects");
        let new = decode_instruction::<T>(words)
            .err()
            .expect("bounded decoder rejects");
        assert_eq!(new.to_string(), old.to_string());
    }

    assert_same_error::<String>(&[]);
    assert_same_error::<String>(&[1, u32::from_ne_bytes([0xff; 4])]);
    assert_same_error::<bool>(&[2]);
    assert_same_error::<u8>(&[256]);
    assert_same_error::<i8>(&[128]);
    assert_same_error::<u64>(&[0]);
    assert_same_error::<char>(&[0xd800]);
    assert_same_error::<Option<u32>>(&[2]);
    assert_same_error::<Nested>(&[u32::MAX]);
    assert_same_error::<Vec<u32>>(&[2, 0]);
}
