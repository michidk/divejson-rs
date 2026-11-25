//! The structural checks serde's typed pass cannot make: a JSON member name twice in one
//! object (§9), which `serde_json` resolves silently inside maps, and `null`, which typed
//! `Option` fields would read as absence (§5.4).

use alloc::collections::BTreeSet;
use alloc::string::String;
use core::fmt;

use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};

/// Walks any JSON value, rejecting duplicate member names everywhere and `null` outside
/// `extensions` (whose content is any JSON, §5.5).
#[derive(Clone, Copy)]
pub(crate) struct Strict {
    pub(crate) in_extensions: bool,
}

impl<'de> DeserializeSeed<'de> for Strict {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Strict {
    type Value = ();

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON value")
    }

    fn visit_bool<E>(self, _: bool) -> Result<(), E> {
        Ok(())
    }

    fn visit_i64<E>(self, _: i64) -> Result<(), E> {
        Ok(())
    }

    fn visit_u64<E>(self, _: u64) -> Result<(), E> {
        Ok(())
    }

    fn visit_f64<E>(self, _: f64) -> Result<(), E> {
        Ok(())
    }

    fn visit_str<E>(self, _: &str) -> Result<(), E> {
        Ok(())
    }

    fn visit_unit<E: de::Error>(self) -> Result<(), E> {
        if self.in_extensions {
            Ok(())
        } else {
            Err(E::custom(
                "null is not a DiveJSON value; an unrecorded member is omitted (spec §5.4)",
            ))
        }
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        while seq.next_element_seed(self)?.is_some() {}
        Ok(())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let mut seen = BTreeSet::new();
        while let Some(key) = map.next_key::<String>()? {
            let child = Self {
                in_extensions: self.in_extensions || key == "extensions",
            };
            map.next_value_seed(child)?;
            if let Some(key) = seen.replace(key) {
                return Err(de::Error::custom(format_args!(
                    "duplicate member name `{key}` (spec §9)"
                )));
            }
        }
        Ok(())
    }
}
