//! A command's arguments, decoded from the MessagePack the page sends
//! (RFC 0050).
//!
//! The page's object keys are strings, so a map keyed by integers
//! (`HashMap<i64, T>`) arrives keyed by `"3"`, `"7"`, which `rmp_serde` won't
//! read as integers. [`decode_args`] decodes strictly first; only when that
//! fails does it decode again, reading a map key that's a string of digits as
//! the integer its type asks for. Nothing but map keys is read more leniently,
//! and when both fail the strict error is the one returned.

use rmpv::Value;
use serde::de::{
    self, DeserializeOwned, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor,
};

type Error = rmpv::ext::Error;

/// Decode a command's argument tuple: strictly, then (on failure) with map
/// keys read from strings of digits. The strict error when both fail.
pub fn decode_args<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, rmp_serde::decode::Error> {
    let strict = match rmp_serde::from_slice::<T>(bytes) {
        Ok(value) => return Ok(value),
        Err(e) => e,
    };
    let Ok(value) = rmpv::decode::read_value(&mut &bytes[..]) else {
        return Err(strict);
    };
    T::deserialize(Lenient(value)).map_err(|_| strict)
}

/// A value whose map keys may be integers sent as strings; everything else
/// is `rmpv`'s, strict.
struct Lenient(Value);

impl<'de> Deserializer<'de> for Lenient {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.0 {
            Value::Array(items) => {
                let len = items.len();
                let mut seq = Seq(items.into_iter());
                let out = visitor.visit_seq(&mut seq)?;
                if seq.0.len() == 0 {
                    Ok(out)
                } else {
                    Err(de::Error::invalid_length(len, &"fewer elements in array"))
                }
            }
            Value::Map(entries) => {
                let len = entries.len();
                let mut map = Map {
                    entries: entries.into_iter(),
                    value: None,
                };
                let out = visitor.visit_map(&mut map)?;
                if map.entries.len() == 0 {
                    Ok(out)
                } else {
                    Err(de::Error::invalid_length(len, &"fewer elements in map"))
                }
            }
            other => other.deserialize_any(visitor),
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.0 {
            Value::Nil => visitor.visit_none(),
            other => visitor.visit_some(Lenient(other)),
        }
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        match self.0 {
            // MessagePack's own extension values are `rmpv`'s to read.
            ext @ Value::Ext(..) => ext.deserialize_newtype_struct(name, visitor),
            other => visitor.visit_newtype_struct(Lenient(other)),
        }
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.0.deserialize_enum(name, variants, visitor)
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.0.deserialize_unit_struct(name, visitor)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf unit seq tuple tuple_struct map struct identifier ignored_any
    }
}

struct Seq(std::vec::IntoIter<Value>);

impl<'de> SeqAccess<'de> for Seq {
    type Error = Error;

    fn next_element_seed<S: DeserializeSeed<'de>>(
        &mut self,
        seed: S,
    ) -> Result<Option<S::Value>, Error> {
        self.0
            .next()
            .map(|item| seed.deserialize(Lenient(item)))
            .transpose()
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.0.len())
    }
}

struct Map {
    entries: std::vec::IntoIter<(Value, Value)>,
    value: Option<Value>,
}

impl<'de> MapAccess<'de> for Map {
    type Error = Error;

    fn next_key_seed<S: DeserializeSeed<'de>>(
        &mut self,
        seed: S,
    ) -> Result<Option<S::Value>, Error> {
        let Some((key, value)) = self.entries.next() else {
            return Ok(None);
        };
        self.value = Some(value);
        seed.deserialize(Key(key)).map(Some)
    }

    fn next_value_seed<S: DeserializeSeed<'de>>(&mut self, seed: S) -> Result<S::Value, Error> {
        let value = self
            .value
            .take()
            .ok_or_else(|| de::Error::custom("a map value asked for before its key"))?;
        seed.deserialize(Lenient(value))
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.entries.len())
    }
}

/// A map key: a string asked for as an integer is parsed as one (exactly
/// what `str::parse` reads for the type); anything else is a [`Lenient`]
/// value's.
struct Key(Value);

macro_rules! integer_key {
    ($($method:ident $ty:ty => $visit:ident),+ $(,)?) => {$(
        fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
            match self.0 {
                Value::String(s) if s.is_str() => {
                    let text = s.as_str().unwrap_or_default();
                    let n: $ty = text.parse().map_err(|_| {
                        de::Error::invalid_value(de::Unexpected::Str(text), &stringify!($ty))
                    })?;
                    visitor.$visit(n)
                }
                other => Lenient(other).$method(visitor),
            }
        }
    )+};
}

impl<'de> Deserializer<'de> for Key {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        Lenient(self.0).deserialize_any(visitor)
    }

    integer_key! {
        deserialize_i8 i8 => visit_i8,
        deserialize_i16 i16 => visit_i16,
        deserialize_i32 i32 => visit_i32,
        deserialize_i64 i64 => visit_i64,
        deserialize_i128 i128 => visit_i128,
        deserialize_u8 u8 => visit_u8,
        deserialize_u16 u16 => visit_u16,
        deserialize_u32 u32 => visit_u32,
        deserialize_u64 u64 => visit_u64,
        deserialize_u128 u128 => visit_u128,
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        match self.0 {
            ext @ Value::Ext(..) => ext.deserialize_newtype_struct(name, visitor),
            // `struct LineId(i64)` as a key: its integer, read as a key.
            other => visitor.visit_newtype_struct(Key(other)),
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        Lenient(self.0).deserialize_option(visitor)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        Lenient(self.0).deserialize_enum(name, variants, visitor)
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        Lenient(self.0).deserialize_unit_struct(name, visitor)
    }

    serde::forward_to_deserialize_any! {
        bool f32 f64 char str string bytes byte_buf unit seq tuple tuple_struct
        map struct identifier ignored_any
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};

    use serde::Deserialize;
    use serde_json::json;

    use super::*;

    /// What the page sends: `encode([...args])`, an object's keys as strings.
    fn page(args: serde_json::Value) -> Vec<u8> {
        rmp_serde::to_vec(&args).unwrap()
    }

    #[test]
    fn integer_keys_are_read_from_the_page_s_strings() {
        let (by_line,): (HashMap<i64, u32>,) =
            decode_args(&page(json!([{"3": 2, "-7": 1}]))).unwrap();
        assert_eq!(by_line, HashMap::from([(3, 2), (-7, 1)]));
        let (sorted,): (BTreeMap<u32, String>,) =
            decode_args(&page(json!([{"2": "b", "1": "a"}]))).unwrap();
        assert_eq!(
            sorted.into_iter().collect::<Vec<_>>(),
            [(1, "a".into()), (2, "b".into())]
        );
        // Past 2^53: the digits, so nothing is lost.
        let (wide,): (HashMap<u64, bool>,) =
            decode_args(&page(json!([{"9007199254740993": true}]))).unwrap();
        assert_eq!(wide, HashMap::from([(9_007_199_254_740_993, true)]));
    }

    #[derive(Deserialize, Debug, PartialEq)]
    struct Order {
        id: i64,
        qty_by_line: HashMap<i64, u32>,
        notes: Option<HashMap<u16, String>>,
    }

    #[derive(Deserialize, Debug, PartialEq, Eq, Hash)]
    struct LineId(i64);

    #[test]
    fn a_map_nested_anywhere_and_a_newtype_key_are_read_too() {
        let bytes = page(json!([
            {"id": 1, "qty_by_line": {"3": 2}, "notes": {"9": "rush"}},
            [{"4": 1}, {}],
            {"5": "five"},
        ]));
        let (order, list, newtype): (Order, Vec<HashMap<i32, u8>>, HashMap<LineId, String>) =
            decode_args(&bytes).unwrap();
        assert_eq!(order.qty_by_line, HashMap::from([(3, 2)]));
        assert_eq!(order.notes, Some(HashMap::from([(9, "rush".into())])));
        assert_eq!(list, vec![HashMap::from([(4, 1)]), HashMap::new()]);
        assert_eq!(newtype, HashMap::from([(LineId(5), "five".into())]));
        // Struct fields keep their names, and a struct sent as an array reads too.
        let (order,): (Order,) = decode_args(&page(json!([[2, {"1": 1}, null]]))).unwrap();
        assert_eq!(order.id, 2);
        assert_eq!(order.notes, None);
    }

    #[test]
    fn anything_but_digits_in_range_is_the_strict_error() {
        let strict = |bytes: &[u8]| {
            rmp_serde::from_slice::<(HashMap<u8, u8>,)>(bytes)
                .unwrap_err()
                .to_string()
        };
        for key in ["3.0", " 3", "three", "256", "-1", ""] {
            let bytes = page(json!([{ key: 1 }]));
            let err = decode_args::<(HashMap<u8, u8>,)>(&bytes).unwrap_err();
            assert_eq!(err.to_string(), strict(&bytes), "{key:?}");
        }
    }

    #[test]
    fn a_string_where_an_integer_field_is_expected_stays_an_error() {
        #[derive(Deserialize, Debug)]
        #[allow(dead_code)]
        struct Line {
            qty: u32,
        }
        assert!(decode_args::<(Line,)>(&page(json!([{"qty": "3"}]))).is_err());
        assert!(decode_args::<(u32,)>(&page(json!(["3"]))).is_err());
        assert!(decode_args::<(Vec<i64>,)>(&page(json!([["1"]]))).is_err());
    }

    #[test]
    fn a_call_that_decodes_today_decodes_the_same() {
        let bytes = rmp_serde::to_vec(&(HashMap::from([(3i64, 2u32)]), "x", Some(5u8))).unwrap();
        let (map, text, n): (HashMap<i64, u32>, String, Option<u8>) = decode_args(&bytes).unwrap();
        assert_eq!(
            (map, text.as_str(), n),
            (HashMap::from([(3, 2)]), "x", Some(5))
        );
        let (s,): (HashMap<String, u8>,) = decode_args(&page(json!([{"3": 1}]))).unwrap();
        assert_eq!(s, HashMap::from([("3".into(), 1)]));
    }
}
