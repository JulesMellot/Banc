use apple_native_keyring_store::keychain::{Cred, MacKeychainDomain};
use keyring_core::Error;

use super::{Backend, SecretError};
use crate::paths::{self, Scope};

/// Files readable by root only for the service, under
/// `/Library/Application Support/Banc/secrets`. The login keychain for the
/// application.
pub(super) fn backend(scope: Scope) -> Result<Backend, SecretError> {
    match scope {
        Scope::System => Ok(Backend::Files(paths::state_dir(scope)?.join("secrets"))),
        Scope::User => Ok(Backend::Keychain(scope)),
    }
}

fn entry(_scope: Scope, service: &str, name: &str) -> Result<keyring_core::Entry, SecretError> {
    Cred::build(MacKeychainDomain::User, service, name).map_err(keychain)
}

pub(super) fn get(scope: Scope, service: &str, name: &str) -> Result<Option<Vec<u8>>, SecretError> {
    match entry(scope, service, name)?.get_secret() {
        Ok(secret) => Ok(Some(secret)),
        Err(Error::NoEntry) => Ok(None),
        Err(error) => Err(keychain(error)),
    }
}

pub(super) fn set(
    scope: Scope,
    service: &str,
    name: &str,
    secret: &[u8],
) -> Result<(), SecretError> {
    entry(scope, service, name)?
        .set_secret(secret)
        .map_err(keychain)
}

pub(super) fn delete(scope: Scope, service: &str, name: &str) -> Result<(), SecretError> {
    match entry(scope, service, name)?.delete_credential() {
        Ok(()) | Err(Error::NoEntry) => Ok(()),
        Err(error) => Err(keychain(error)),
    }
}

fn keychain(error: Error) -> SecretError {
    SecretError::Keychain(error.to_string())
}
