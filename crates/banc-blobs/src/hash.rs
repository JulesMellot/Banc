//! The BLAKE3 hash that names a file.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The BLAKE3 hash of a file's content, written as 64 lowercase hex digits,
/// like the hashes of the manifest.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Hash([u8; 32]);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("not a BLAKE3 hash: 64 lowercase hex digits expected")]
pub struct InvalidHash;

impl Hash {
    /// The hash of `data`.
    pub fn of(data: &[u8]) -> Self {
        Self(*blake3::hash(data).as_bytes())
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl From<blake3::Hash> for Hash {
    fn from(hash: blake3::Hash) -> Self {
        Self(*hash.as_bytes())
    }
}

impl From<Hash> for blake3::Hash {
    fn from(hash: Hash) -> Self {
        blake3::Hash::from_bytes(hash.0)
    }
}

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hash({self})")
    }
}

impl FromStr for Hash {
    type Err = InvalidHash;

    /// Lowercase only: the text is also a file name in the store, and one
    /// hash must have one name.
    fn from_str(text: &str) -> Result<Self, InvalidHash> {
        fn digit(byte: u8) -> Result<u8, InvalidHash> {
            match byte {
                b'0'..=b'9' => Ok(byte - b'0'),
                b'a'..=b'f' => Ok(byte - b'a' + 10),
                _ => Err(InvalidHash),
            }
        }
        let text = text.as_bytes();
        if text.len() != 64 {
            return Err(InvalidHash);
        }
        let mut bytes = [0; 32];
        let (pairs, _) = text.as_chunks::<2>();
        for (byte, [high, low]) in bytes.iter_mut().zip(pairs) {
            *byte = digit(*high)? << 4 | digit(*low)?;
        }
        Ok(Self(bytes))
    }
}

impl Serialize for Hash {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Hash {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hash_reads_back_its_own_text() {
        let hash = Hash::of(b"banc");
        assert_eq!(hash.to_string(), blake3::hash(b"banc").to_hex().as_str());
        assert_eq!(hash.to_string().parse(), Ok(hash));
    }

    #[test]
    fn only_lowercase_hex_of_the_right_length_is_a_hash() {
        let text = Hash::of(b"banc").to_string();
        assert_eq!(text.to_uppercase().parse::<Hash>(), Err(InvalidHash));
        assert_eq!(text[..63].parse::<Hash>(), Err(InvalidHash));
        assert_eq!(format!("{text}0").parse::<Hash>(), Err(InvalidHash));
        assert_eq!("../".repeat(21).parse::<Hash>(), Err(InvalidHash));
    }

    #[test]
    fn a_hash_travels_as_text() {
        let hash = Hash::of(b"banc");
        let json = serde_json::to_string(&hash).unwrap();
        assert_eq!(json, format!("\"{hash}\""));
        assert_eq!(serde_json::from_str::<Hash>(&json).unwrap(), hash);
        assert!(serde_json::from_str::<Hash>("\"zz\"").is_err());
    }
}
