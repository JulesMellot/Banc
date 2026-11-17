//! What the agent keeps on disk: its enrollment, in `agent.json` in the
//! data folder. The machine key is not there: it is in the secret store.

use std::path::{Path, PathBuf};

use banc_platform::secrets::{SecretError, SecretStore};
use banc_proto::enroll::ServerInfo;
use banc_proto::keys::{PublicKey, SecretKey};
use banc_proto::machine::{MachineRole, PersonRole};
use serde::{Deserialize, Serialize};

const STATE_FILE: &str = "agent.json";
/// Name of the machine key in the secret store.
pub const MACHINE_KEY: &str = "machine-key";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Enrolled {
    /// Server base URL, from the invitation.
    pub server: String,
    pub machine_id: i64,
    pub name: String,
    pub machine_role: MachineRole,
    pub person_role: PersonRole,
    /// The team's admin key, checked against the invitation's fingerprint.
    /// Every manifest must be signed by it (phase 2).
    pub admin_key: PublicKey,
    pub server_info: ServerInfo,
}

#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("cannot read or write {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path} is unreadable")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("secret store")]
    Secret(#[from] SecretError),
    #[error("the machine key in the secret store is malformed")]
    BadKey,
}

pub fn load(dir: &Path) -> Result<Option<Enrolled>, StateError> {
    let path = dir.join(STATE_FILE);
    let text = match std::fs::read(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(StateError::Io { path, source }),
    };
    serde_json::from_slice(&text)
        .map(Some)
        .map_err(|source| StateError::Json { path, source })
}

/// Written in one step: a crash leaves the old file or the new one.
pub fn save(dir: &Path, enrolled: &Enrolled) -> Result<(), StateError> {
    let path = dir.join(STATE_FILE);
    let json = serde_json::to_vec_pretty(enrolled).map_err(|source| StateError::Json {
        path: path.clone(),
        source,
    })?;
    banc_platform::fs::write_private(&path, &json).map_err(|source| StateError::Io { path, source })
}

pub fn load_key(secrets: &SecretStore) -> Result<Option<SecretKey>, StateError> {
    let Some(bytes) = secrets.get(MACHINE_KEY)? else {
        return Ok(None);
    };
    let bytes: [u8; 32] = bytes.try_into().map_err(|_| StateError::BadKey)?;
    Ok(Some(SecretKey::from_bytes(&bytes)))
}

pub fn save_key(secrets: &SecretStore, key: &SecretKey) -> Result<(), StateError> {
    Ok(secrets.set(MACHINE_KEY, &key.to_bytes())?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enrolled() -> Enrolled {
        Enrolled {
            server: "https://banc.example.com".to_owned(),
            machine_id: 3,
            name: "Station 3".to_owned(),
            machine_role: MachineRole::Workstation,
            person_role: PersonRole::Editor,
            admin_key: SecretKey::from_bytes(&[1; 32]).public(),
            server_info: ServerInfo {
                version: "0.1.0".to_owned(),
                endpoint_id: SecretKey::from_bytes(&[2; 32]).public(),
                relay_url: Some("https://banc.example.com".to_owned()),
                direct_addrs: vec!["203.0.113.5:443".parse().unwrap()],
            },
        }
    }

    #[test]
    fn state_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(load(tmp.path()).unwrap(), None);
        save(tmp.path(), &enrolled()).unwrap();
        assert_eq!(load(tmp.path()).unwrap(), Some(enrolled()));
    }

    #[test]
    fn corrupt_state_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(STATE_FILE), b"{").unwrap();
        assert!(matches!(load(tmp.path()), Err(StateError::Json { .. })));
    }

    #[test]
    fn key_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let secrets = SecretStore::in_dir(tmp.path());
        assert!(load_key(&secrets).unwrap().is_none());
        let key = SecretKey::generate();
        save_key(&secrets, &key).unwrap();
        assert_eq!(load_key(&secrets).unwrap().unwrap().public(), key.public());
        secrets.set(MACHINE_KEY, b"short").unwrap();
        assert!(matches!(load_key(&secrets), Err(StateError::BadKey)));
    }
}
