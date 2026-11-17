//! Keys, signatures and fingerprints.
//!
//! Every machine is identified by an Ed25519 key, which is also its iroh
//! identifier. The admin key, kept on the admin's workstation, signs the
//! manifest; invitations carry its fingerprint.

use std::fmt;
use std::str::FromStr;

pub use iroh_base::{PublicKey, SecretKey, Signature};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::text;

/// BLAKE3 key derivation context of the fingerprint. Never changes: every
/// enrolled workstation has stored a fingerprint computed with it.
const FINGERPRINT_CONTEXT: &str = "banc 2026-11 admin key fingerprint";

/// Fingerprint of the admin public key, carried by the invitation link
/// (`a=`). 128 bits: enough against a second preimage, short enough for a
/// link.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fingerprint([u8; 16]);

impl Fingerprint {
    pub fn of(key: &PublicKey) -> Self {
        let hash = blake3::derive_key(FINGERPRINT_CONTEXT, key.as_bytes());
        let mut bytes = [0; 16];
        bytes.copy_from_slice(&hash[..16]);
        Self(bytes)
    }

    /// Whether `key` is the key this fingerprint was computed from.
    pub fn matches(&self, key: &PublicKey) -> bool {
        *self == Self::of(key)
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&text::base32(&self.0))
    }
}

impl fmt::Debug for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fingerprint({self})")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("malformed fingerprint")]
pub struct FingerprintError;

impl FromStr for Fingerprint {
    type Err = FingerprintError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        text::base32_exact(s).map(Self).ok_or(FingerprintError)
    }
}

impl Serialize for Fingerprint {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Fingerprint {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

/// A signature, written in JSON as hexadecimal text. `iroh_base::Signature`
/// serializes as an array of 64 numbers.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Sig(pub Signature);

impl fmt::Debug for Sig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Sig({})", text::hex(&self.0.to_bytes()))
    }
}

impl Serialize for Sig {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&text::hex(&self.0.to_bytes()))
    }
}

impl<'de> Deserialize<'de> for Sig {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        let bytes = text::hex_exact::<{ Signature::LENGTH }>(&text)
            .ok_or_else(|| serde::de::Error::custom("malformed signature"))?;
        Ok(Self(Signature::from_bytes(&bytes)))
    }
}

/// Signature of `payload` under a domain separation label, so that a
/// signature made for one purpose is never valid for another.
pub fn sign(secret: &SecretKey, domain: &str, payload: &[u8]) -> Sig {
    Sig(secret.sign(&domain_message(domain, payload)))
}

/// Checks a signature made with [`sign`].
pub fn verify(key: &PublicKey, domain: &str, payload: &[u8], sig: &Sig) -> bool {
    key.verify(&domain_message(domain, payload), &sig.0).is_ok()
}

fn domain_message(domain: &str, payload: &[u8]) -> Vec<u8> {
    let mut message = Vec::with_capacity(domain.len() + 1 + payload.len());
    message.extend_from_slice(domain.as_bytes());
    message.push(0);
    message.extend_from_slice(payload);
    message
}

/// Domain of the admin key registration.
const ADMIN_KEY_DOMAIN: &str = "banc/admin-key/1";

/// Sent by the admin's Banc application to register the team's admin public
/// key on the server. The signature proves that the application holds the
/// private key. The private key itself never leaves the admin's workstation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminKeyRegistration {
    pub public_key: PublicKey,
    pub signature: Sig,
}

impl AdminKeyRegistration {
    pub fn signed(secret: &SecretKey) -> Self {
        let public_key = secret.public();
        Self {
            public_key,
            signature: sign(secret, ADMIN_KEY_DOMAIN, public_key.as_bytes()),
        }
    }

    pub fn is_valid(&self) -> bool {
        verify(
            &self.public_key,
            ADMIN_KEY_DOMAIN,
            self.public_key.as_bytes(),
            &self.signature,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(seed: u8) -> SecretKey {
        SecretKey::from_bytes(&[seed; 32])
    }

    #[test]
    fn fingerprint_is_stable_and_specific() {
        let a = key(1).public();
        let b = key(2).public();
        assert_eq!(Fingerprint::of(&a), Fingerprint::of(&a));
        assert_ne!(Fingerprint::of(&a), Fingerprint::of(&b));
        assert!(Fingerprint::of(&a).matches(&a));
        assert!(!Fingerprint::of(&a).matches(&b));
    }

    #[test]
    fn fingerprint_text_round_trip() {
        let fingerprint = Fingerprint::of(&key(1).public());
        let text = fingerprint.to_string();
        assert_eq!(text.len(), 26);
        assert_eq!(text.parse::<Fingerprint>(), Ok(fingerprint));
        assert_eq!(
            text.to_ascii_uppercase().parse::<Fingerprint>(),
            Ok(fingerprint)
        );
        assert_eq!("abc".parse::<Fingerprint>(), Err(FingerprintError));

        let json = serde_json::to_string(&fingerprint).unwrap();
        assert_eq!(json, format!("\"{text}\""));
        assert_eq!(
            serde_json::from_str::<Fingerprint>(&json).unwrap(),
            fingerprint
        );
    }

    #[test]
    fn signature_bound_to_its_domain() {
        let secret = key(3);
        let sig = sign(&secret, "banc/test/1", b"payload");
        assert!(verify(&secret.public(), "banc/test/1", b"payload", &sig));
        assert!(!verify(&secret.public(), "banc/other/1", b"payload", &sig));
        assert!(!verify(&secret.public(), "banc/test/1", b"payloaD", &sig));
        assert!(!verify(&key(4).public(), "banc/test/1", b"payload", &sig));
    }

    #[test]
    fn signature_json_is_hex() {
        let sig = sign(&key(3), "banc/test/1", b"x");
        let json = serde_json::to_string(&sig).unwrap();
        assert_eq!(json.len(), 2 + 128);
        assert_eq!(serde_json::from_str::<Sig>(&json).unwrap(), sig);
        assert!(serde_json::from_str::<Sig>("\"abcd\"").is_err());
    }

    #[test]
    fn admin_key_registration() {
        let registration = AdminKeyRegistration::signed(&key(5));
        assert!(registration.is_valid());

        let mut forged = registration.clone();
        forged.public_key = key(6).public();
        assert!(!forged.is_valid());
    }
}
