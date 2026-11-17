//! Enrollment: a new machine trades an invitation token for a place in the
//! team.
//!
//! The agent sends [`EnrollRequest`] to [`ENROLL_PATH`] over HTTPS. The
//! request is signed with the machine key, which proves the agent holds it:
//! that key becomes the machine's identity, on the server and on iroh.
//! The response carries the admin public key; the agent checks it against the
//! fingerprint of the invitation before storing anything.

use std::net::SocketAddr;

use serde::{Deserialize, Serialize};

use crate::invite::InviteToken;
use crate::keys::{self, PublicKey, SecretKey, Sig};
use crate::machine::{MachineRole, Os, PersonRole};

/// HTTP path of enrollment.
pub const ENROLL_PATH: &str = "/api/v1/enroll";
/// HTTP path of the public server information.
pub const INFO_PATH: &str = "/api/v1/info";

const ENROLL_DOMAIN: &str = "banc/enroll/1";

/// Longest machine name accepted.
pub const MAX_NAME_LEN: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnrollRequest {
    pub token: InviteToken,
    /// The machine's public key, also its iroh identifier.
    pub machine_key: PublicKey,
    /// Name shown in the admin, the computer name by default.
    pub name: String,
    pub os: Os,
    /// `x86_64` or `aarch64`.
    pub arch: String,
    pub agent_version: String,
    /// Signature of the token and the machine key, with the machine key.
    pub signature: Sig,
}

impl EnrollRequest {
    pub fn signed(
        secret: &SecretKey,
        token: InviteToken,
        name: String,
        os: Os,
        arch: String,
        agent_version: String,
    ) -> Self {
        let machine_key = secret.public();
        let signature = keys::sign(secret, ENROLL_DOMAIN, &payload(&token, &machine_key));
        Self {
            token,
            machine_key,
            name,
            os,
            arch,
            agent_version,
            signature,
        }
    }

    /// Whether the signature was made with `machine_key` for this token.
    pub fn is_signed(&self) -> bool {
        keys::verify(
            &self.machine_key,
            ENROLL_DOMAIN,
            &payload(&self.token, &self.machine_key),
            &self.signature,
        )
    }
}

fn payload(token: &InviteToken, machine_key: &PublicKey) -> Vec<u8> {
    let mut payload = Vec::with_capacity(64);
    payload.extend_from_slice(token.as_bytes());
    payload.extend_from_slice(machine_key.as_bytes());
    payload
}

/// Checks a machine name: not empty, not too long, no control character.
/// Returns it without surrounding spaces.
pub fn clean_name(name: &str) -> Option<&str> {
    let name = name.trim();
    let valid = !name.is_empty()
        && name.chars().count() <= MAX_NAME_LEN
        && !name.chars().any(char::is_control);
    valid.then_some(name)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnrollResponse {
    pub machine_id: i64,
    pub machine_role: MachineRole,
    pub person_role: PersonRole,
    /// The team's admin key. The agent checks it against the invitation's
    /// fingerprint and refuses to go on if they differ.
    pub admin_key: PublicKey,
    pub server: ServerInfo,
}

/// How to reach the server over iroh. Public: served at [`INFO_PATH`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerInfo {
    pub version: String,
    /// The server's iroh identifier.
    pub endpoint_id: PublicKey,
    /// The team's iroh relay, `None` if the server runs without one.
    pub relay_url: Option<String>,
    /// Addresses where the server's iroh endpoint can be reached directly.
    pub direct_addrs: Vec<SocketAddr>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(secret: &SecretKey) -> EnrollRequest {
        EnrollRequest::signed(
            secret,
            InviteToken::from_bytes([9; 32]),
            "Station 3".to_owned(),
            Os::Macos,
            "aarch64".to_owned(),
            "0.1.0".to_owned(),
        )
    }

    #[test]
    fn signed_request_is_valid() {
        let request = request(&SecretKey::from_bytes(&[1; 32]));
        assert!(request.is_signed());
    }

    #[test]
    fn signature_bound_to_the_token_and_the_key() {
        let secret = SecretKey::from_bytes(&[1; 32]);

        let mut other_token = request(&secret);
        other_token.token = InviteToken::from_bytes([8; 32]);
        assert!(!other_token.is_signed());

        // Someone who intercepted a request cannot enroll their own key with
        // the same token and signature.
        let mut other_key = request(&secret);
        other_key.machine_key = SecretKey::from_bytes(&[2; 32]).public();
        assert!(!other_key.is_signed());
    }

    #[test]
    fn request_json_round_trip() {
        let request = request(&SecretKey::from_bytes(&[1; 32]));
        let json = serde_json::to_string(&request).unwrap();
        let back: EnrollRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back, request);
        assert!(back.is_signed());
    }

    #[test]
    fn machine_names() {
        assert_eq!(clean_name("  Station 3 "), Some("Station 3"));
        assert_eq!(clean_name("Poste d'étalonnage"), Some("Poste d'étalonnage"));
        assert_eq!(clean_name(""), None);
        assert_eq!(clean_name("   "), None);
        assert_eq!(clean_name("a\nb"), None);
        assert_eq!(
            clean_name(&"é".repeat(MAX_NAME_LEN)).map(str::len),
            Some(128)
        );
        assert_eq!(clean_name(&"x".repeat(MAX_NAME_LEN + 1)), None);
    }
}
