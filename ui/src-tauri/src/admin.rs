//! The admin key, on the admin's workstation only.
//!
//! Generated here, kept in the user's keychain, written down on paper as a
//! recovery code. The server only ever receives the public key, with a
//! signature proving the application holds the private one.

use std::sync::Mutex;

use banc_platform::paths::Scope;
use banc_platform::secrets::SecretStore;
use banc_proto::admin_key;
use banc_proto::keys::{AdminKeyRegistration, SecretKey};
use banc_proto::manifest::{Manifest, SignedManifest};
use serde::Serialize;

use crate::error::{CommandError, CommandResult};
use crate::server::PublishSession;

/// Name of the admin key in the keychain.
const SECRET_NAME: &str = "admin-key";

/// A key generated but not confirmed yet: it is only stored once the admin
/// has proved they wrote the recovery code down.
#[derive(Default)]
pub struct PendingKey(Mutex<Option<SecretKey>>);

/// The manifest the admin is reading. What gets signed is this copy, never
/// something sent back by the page.
#[derive(Default)]
pub struct PendingManifest(Mutex<Option<PublishSession>>);

impl PendingManifest {
    fn replace(&self, session: Option<PublishSession>) -> Option<PublishSession> {
        let mut pending = self.0.lock().unwrap_or_else(|p| p.into_inner());
        std::mem::replace(&mut pending, session)
    }
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct KeyInfo {
    pub fingerprint: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct NewKey {
    pub fingerprint: String,
    pub recovery_code: String,
}

fn store() -> CommandResult<SecretStore> {
    SecretStore::os(Scope::User).map_err(|e| CommandError::new("keychain", e))
}

pub fn load(store: &SecretStore) -> CommandResult<Option<SecretKey>> {
    let bytes = store
        .get(SECRET_NAME)
        .map_err(|e| CommandError::new("keychain", e))?;
    let Some(bytes) = bytes else {
        return Ok(None);
    };
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| CommandError::new("keychain", "malformed admin key"))?;
    Ok(Some(SecretKey::from_bytes(&bytes)))
}

fn info(key: &SecretKey) -> KeyInfo {
    KeyInfo {
        fingerprint: admin_key::fingerprint(key).to_string(),
    }
}

/// The admin key held by this workstation, if any.
#[tauri::command]
pub fn admin_key_status() -> CommandResult<Option<KeyInfo>> {
    Ok(load(&store()?)?.as_ref().map(info))
}

/// Generates a key and returns its recovery code. Nothing is stored yet.
#[tauri::command]
pub fn admin_key_generate(pending: tauri::State<'_, PendingKey>) -> CommandResult<NewKey> {
    if load(&store()?)?.is_some() {
        return Err(CommandError::new(
            "admin_key_present",
            "this workstation already holds a key",
        ));
    }
    let key = admin_key::generate();
    let answer = NewKey {
        fingerprint: admin_key::fingerprint(&key).to_string(),
        recovery_code: admin_key::recovery_code(&key),
    };
    *pending.0.lock().unwrap_or_else(|p| p.into_inner()) = Some(key);
    Ok(answer)
}

/// The admin types the last group of the code back: the key is stored.
#[tauri::command]
pub fn admin_key_confirm(
    pending: tauri::State<'_, PendingKey>,
    last_group: String,
) -> CommandResult<KeyInfo> {
    confirm(&store()?, &pending.0, &last_group)
}

pub fn confirm(
    store: &SecretStore,
    pending: &Mutex<Option<SecretKey>>,
    last_group: &str,
) -> CommandResult<KeyInfo> {
    let mut pending = pending.lock().unwrap_or_else(|p| p.into_inner());
    let Some(key) = pending.as_ref() else {
        return Err(CommandError::new("no_pending_key", "generate a key first"));
    };
    let code = admin_key::recovery_code(key);
    let expected = code.rsplit('-').next().unwrap_or_default();
    if !last_group.trim().eq_ignore_ascii_case(expected) {
        return Err(CommandError::new(
            "wrong_confirmation",
            "the last group does not match",
        ));
    }
    store
        .set(SECRET_NAME, &key.to_bytes())
        .map_err(|e| CommandError::new("keychain", e))?;
    let answer = info(key);
    *pending = None;
    tracing::info!(fingerprint = %answer.fingerprint, "admin key created");
    Ok(answer)
}

/// Puts back a key from its recovery code, on a new workstation.
#[tauri::command]
pub fn admin_key_restore(code: String) -> CommandResult<KeyInfo> {
    restore(&store()?, &code)
}

pub fn restore(store: &SecretStore, code: &str) -> CommandResult<KeyInfo> {
    if load(store)?.is_some() {
        return Err(CommandError::new(
            "admin_key_present",
            "this workstation already holds a key",
        ));
    }
    let key = admin_key::from_recovery_code(code)
        .map_err(|e| CommandError::new("bad_recovery_code", e))?;
    store
        .set(SECRET_NAME, &key.to_bytes())
        .map_err(|e| CommandError::new("keychain", e))?;
    tracing::info!("admin key restored from its recovery code");
    Ok(info(&key))
}

/// Registers the public key on the server, with an admin account of the
/// web admin. `replace` replaces another registered key.
#[tauri::command]
pub async fn admin_key_register(
    server: String,
    name: String,
    password: String,
    replace: bool,
) -> CommandResult<KeyInfo> {
    let key = load(&store()?)?
        .ok_or_else(|| CommandError::new("no_admin_key", "create or restore the key first"))?;
    let server = banc_proto::invite::normalize_server(&server)
        .map_err(|e| CommandError::new("bad_server", e))?;
    crate::server::register_admin_key(&server, &name, &password, &key, replace).await?;
    Ok(info(&key))
}

/// What the server proposes to publish, for the admin to read.
#[derive(Debug, Serialize)]
pub struct Proposal {
    pub manifest: Manifest,
    /// Whether it differs from what is published.
    pub changed: bool,
}

/// Reads what the server proposes to publish. The admin key proves who is
/// asking: no password to type.
#[tauri::command]
pub async fn manifest_prepare(
    pending: tauri::State<'_, PendingManifest>,
    server: String,
) -> CommandResult<Proposal> {
    let key = load(&store()?)?
        .ok_or_else(|| CommandError::new("no_admin_key", "create or restore the key first"))?;
    let server = banc_proto::invite::normalize_server(&server)
        .map_err(|e| CommandError::new("bad_server", e))?;
    let (session, changed) = crate::server::prepare_publication(&server, &key).await?;
    let manifest = session.manifest.clone();
    pending.replace(changed.then_some(session));
    Ok(Proposal { manifest, changed })
}

/// Signs the manifest the admin has just read, and publishes it.
#[tauri::command]
pub async fn manifest_publish(pending: tauri::State<'_, PendingManifest>) -> CommandResult<u64> {
    let session = pending
        .replace(None)
        .ok_or_else(|| CommandError::new("no_pending_manifest", "read the manifest first"))?;
    let key = load(&store()?)?
        .ok_or_else(|| CommandError::new("no_admin_key", "create or restore the key first"))?;
    let signed = SignedManifest::sign(&key, &session.manifest)
        .map_err(|e| CommandError::new("bad_manifest", e))?;
    crate::server::publish(&session, &signed).await?;
    tracing::info!(version = session.manifest.version, "manifest published");
    Ok(session.manifest.version)
}

/// The admin decided not to publish.
#[tauri::command]
pub fn manifest_cancel(pending: tauri::State<'_, PendingManifest>) {
    pending.replace(None);
}

/// What the server answers about its registered key.
pub fn registration(key: &SecretKey, replace: bool) -> serde_json::Value {
    let mut body = serde_json::to_value(AdminKeyRegistration::signed(key)).unwrap_or_default();
    body["replace"] = serde_json::Value::Bool(replace);
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirm_needs_the_last_group() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SecretStore::in_dir(tmp.path());
        let pending = Mutex::new(None);
        assert_eq!(
            confirm(&store, &pending, "x").unwrap_err().code,
            "no_pending_key"
        );

        let key = SecretKey::from_bytes(&[4; 32]);
        let code = admin_key::recovery_code(&key);
        let last = code.rsplit('-').next().unwrap().to_owned();
        *pending.lock().unwrap() = Some(key.clone());
        assert_eq!(
            confirm(&store, &pending, "AAAAA").unwrap_err().code,
            "wrong_confirmation"
        );
        assert!(load(&store).unwrap().is_none());

        let info = confirm(&store, &pending, &last.to_ascii_lowercase()).unwrap();
        assert_eq!(info.fingerprint, admin_key::fingerprint(&key).to_string());
        assert_eq!(load(&store).unwrap().unwrap().public(), key.public());
        assert!(pending.lock().unwrap().is_none());
    }

    #[test]
    fn restore_from_the_code() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SecretStore::in_dir(tmp.path());
        let key = SecretKey::from_bytes(&[5; 32]);
        assert_eq!(
            restore(&store, "nope").unwrap_err().code,
            "bad_recovery_code"
        );
        restore(&store, &admin_key::recovery_code(&key)).unwrap();
        assert_eq!(load(&store).unwrap().unwrap().public(), key.public());
        // Never overwrites a key already held.
        let other = admin_key::recovery_code(&SecretKey::from_bytes(&[6; 32]));
        assert_eq!(
            restore(&store, &other).unwrap_err().code,
            "admin_key_present"
        );
    }

    #[test]
    fn registration_is_signed() {
        let key = SecretKey::from_bytes(&[7; 32]);
        let body = registration(&key, true);
        assert_eq!(body["replace"], true);
        let parsed: AdminKeyRegistration = serde_json::from_value(body).unwrap();
        assert!(parsed.is_valid());
        assert_eq!(parsed.public_key, key.public());
    }
}
