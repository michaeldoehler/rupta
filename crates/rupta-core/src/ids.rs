//! Identifiers.
//!
//! Database ids are 64-bit integers. They are serialized as strings in the
//! public API so that JavaScript clients do not lose precision.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A 64-bit id that serializes as a decimal string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Id(pub i64);

impl Serialize for Id {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for Id {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Id, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Num(i64),
            Str(String),
        }
        match Raw::deserialize(d)? {
            Raw::Num(n) => Ok(Id(n)),
            Raw::Str(s) => s.parse().map(Id).map_err(serde::de::Error::custom),
        }
    }
}

impl std::fmt::Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_strings_on_the_wire() {
        assert_eq!(serde_json::to_string(&Id(9007199254740993)).unwrap(), "\"9007199254740993\"");
        assert_eq!(serde_json::from_str::<Id>("\"42\"").unwrap(), Id(42));
        assert_eq!(serde_json::from_str::<Id>("42").unwrap(), Id(42));
    }
}
