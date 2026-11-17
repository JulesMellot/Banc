use std::sync::Arc;

use keyring_core::api::CredentialStoreApi;
use keyring_core::Error;
use windows_native_keyring_store::Store;

use super::{Backend, SecretError};
use crate::paths::Scope;

/// The Credential Manager of the account running the process: LocalSystem
/// for the service, the user for the application.
pub(super) fn backend(scope: Scope) -> Result<Backend, SecretError> {
    Ok(Backend::Keychain(scope))
}

fn entry(service: &str, name: &str) -> Result<keyring_core::Entry, SecretError> {
    let store: Arc<Store> = Store::new().map_err(keychain)?;
    store.build(service, name, None).map_err(keychain)
}

pub(super) fn get(_: Scope, service: &str, name: &str) -> Result<Option<Vec<u8>>, SecretError> {
    match entry(service, name)?.get_secret() {
        Ok(secret) => Ok(Some(secret)),
        Err(Error::NoEntry) => Ok(None),
        Err(error) => Err(keychain(error)),
    }
}

pub(super) fn set(_: Scope, service: &str, name: &str, secret: &[u8]) -> Result<(), SecretError> {
    entry(service, name)?.set_secret(secret).map_err(keychain)
}

pub(super) fn delete(_: Scope, service: &str, name: &str) -> Result<(), SecretError> {
    match entry(service, name)?.delete_credential() {
        Ok(()) | Err(Error::NoEntry) => Ok(()),
        Err(error) => Err(keychain(error)),
    }
}

fn keychain(error: Error) -> SecretError {
    SecretError::Keychain(error.to_string())
}
