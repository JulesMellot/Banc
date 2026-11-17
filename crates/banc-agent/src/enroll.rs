//! Enrollment, agent side: one signed HTTPS request.

use std::time::Duration;

use banc_proto::enroll::{EnrollRequest, EnrollResponse, ENROLL_PATH};
use banc_proto::invite::Invite;
use banc_proto::keys::SecretKey;
use banc_proto::machine::Os;

#[derive(Debug, thiserror::Error)]
pub enum EnrollError {
    #[error("the server refused: {message}")]
    Refused { code: String, message: String },
    #[error("the server cannot be reached: {0}")]
    Unreachable(String),
    #[error("unexpected answer from the server: {0}")]
    BadAnswer(String),
    /// The server answered with an admin key that is not the one of the
    /// invitation: wrong server, or a compromised one. Nothing is kept.
    #[error("the server's admin key does not match the invitation")]
    AdminKeyMismatch,
    #[error("unknown operating system {0}")]
    UnknownOs(String),
}

impl EnrollError {
    /// Stable code for the interface.
    pub fn code(&self) -> String {
        match self {
            Self::Refused { code, .. } => code.clone(),
            Self::Unreachable(_) => "server_unreachable".to_owned(),
            Self::BadAnswer(_) => "bad_answer".to_owned(),
            Self::AdminKeyMismatch => "admin_key_mismatch".to_owned(),
            Self::UnknownOs(_) => "unknown_os".to_owned(),
        }
    }
}

/// The HTTP client, with rustls and ring.
pub fn http_client() -> Result<reqwest::Client, EnrollError> {
    // Already installed by an earlier call: fine.
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(concat!("banc-agent/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| EnrollError::Unreachable(e.to_string()))
}

/// Generates the machine key and enrolls it. The key is returned only if
/// the server accepted it and proved it belongs to the invitation's team.
pub async fn enroll(
    client: &reqwest::Client,
    invite: &Invite,
    name: &str,
) -> Result<(SecretKey, EnrollResponse), EnrollError> {
    let os: Os = banc_platform::machine::OS
        .parse()
        .map_err(|_| EnrollError::UnknownOs(banc_platform::machine::OS.to_owned()))?;
    let key = SecretKey::generate();
    let request = EnrollRequest::signed(
        &key,
        invite.token,
        name.to_owned(),
        os,
        banc_platform::machine::ARCH.to_owned(),
        env!("CARGO_PKG_VERSION").to_owned(),
    );
    let response = client
        .post(format!("{}{ENROLL_PATH}", invite.server))
        .json(&request)
        .send()
        .await
        .map_err(|e| EnrollError::Unreachable(e.to_string()))?;
    let status = response.status();
    let body = response
        .bytes()
        .await
        .map_err(|e| EnrollError::Unreachable(e.to_string()))?;
    if !status.is_success() {
        #[derive(serde::Deserialize)]
        struct Refusal {
            error: String,
            message: String,
        }
        let refusal: Refusal = serde_json::from_slice(&body)
            .map_err(|_| EnrollError::BadAnswer(format!("HTTP {status}")))?;
        return Err(EnrollError::Refused {
            code: refusal.error,
            message: refusal.message,
        });
    }
    let answer: EnrollResponse =
        serde_json::from_slice(&body).map_err(|e| EnrollError::BadAnswer(e.to_string()))?;
    check_admin_key(invite, &answer)?;
    Ok((key, answer))
}

/// The admin key given by the server must be the one whose fingerprint the
/// invitation carries.
pub fn check_admin_key(invite: &Invite, answer: &EnrollResponse) -> Result<(), EnrollError> {
    if invite.fingerprint.matches(&answer.admin_key) {
        Ok(())
    } else {
        Err(EnrollError::AdminKeyMismatch)
    }
}

#[cfg(test)]
mod tests {
    use banc_proto::enroll::ServerInfo;
    use banc_proto::invite::InviteToken;
    use banc_proto::keys::Fingerprint;
    use banc_proto::machine::{MachineRole, PersonRole};

    use super::*;

    #[test]
    fn admin_key_must_match_the_invitation() {
        let admin = SecretKey::from_bytes(&[1; 32]).public();
        let invite = Invite {
            server: "https://banc.example.com".to_owned(),
            token: InviteToken::from_bytes([3; 32]),
            fingerprint: Fingerprint::of(&admin),
        };
        let mut answer = EnrollResponse {
            machine_id: 1,
            machine_role: MachineRole::Workstation,
            person_role: PersonRole::Editor,
            admin_key: admin,
            server: ServerInfo {
                version: "0".to_owned(),
                endpoint_id: SecretKey::from_bytes(&[2; 32]).public(),
                relay_url: None,
                direct_addrs: Vec::new(),
            },
        };
        assert!(check_admin_key(&invite, &answer).is_ok());
        answer.admin_key = SecretKey::from_bytes(&[9; 32]).public();
        assert!(matches!(
            check_admin_key(&invite, &answer),
            Err(EnrollError::AdminKeyMismatch)
        ));
    }
}
