//! Every field a struct deserializes, read from serde itself.
//!
//! A derived `Deserialize` hands its field list to `deserialize_struct`, so a
//! deserializer that records that list and then refuses learns every field -
//! including the ones its `Serialize` leaves out when they are empty or
//! `None`, which no amount of serializing a default can show. The editor's
//! derived inspector reads it (`editor_registry::derived_component_info`):
//! before, `text.align`, `sprite.fill` and every other field skipped by its
//! default were simply absent from the Inspector.
//!
//! The list is serde's, so it can carry a field's **aliases** beside its
//! name; a reader that needs the canonical name checks it round-trips.

use serde::de::{self, Deserialize, Deserializer, Visitor};
use std::cell::Cell;

/// The fields `T` deserializes, or `None` when `T` is not a plain struct (a
/// newtype, an enum, a map, or a struct with a `#[serde(flatten)]` field,
/// which serde reads as a map).
pub fn field_names<T: for<'de> Deserialize<'de>>() -> Option<&'static [&'static str]> {
    let seen = Cell::new(None);
    let _ = T::deserialize(Capture(&seen));
    seen.get()
}

struct Capture<'a>(&'a Cell<Option<&'static [&'static str]>>);

impl<'de> Deserializer<'de> for Capture<'_> {
    type Error = de::value::Error;

    fn deserialize_any<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, Self::Error> {
        Err(de::Error::custom("not a struct"))
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        fields: &'static [&'static str],
        _visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.0.set(Some(fields));
        Err(de::Error::custom("fields captured"))
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map enum identifier ignored_any
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    #[allow(dead_code)]
    struct Plain {
        a: f32,
        #[serde(default, rename = "bee")]
        b: Option<String>,
        #[serde(default, skip_serializing)]
        c: Vec<u8>,
    }

    #[derive(Deserialize)]
    #[allow(dead_code)]
    struct Wrapper(f32);

    #[test]
    fn lists_every_field_by_its_serialized_name() {
        assert_eq!(field_names::<Plain>(), Some(&["a", "bee", "c"][..]));
    }

    #[test]
    fn is_none_for_what_is_not_a_struct() {
        assert_eq!(field_names::<Wrapper>(), None);
        assert_eq!(field_names::<f32>(), None);
    }
}
