use apple_native_keyring_store::keychain::{Cred, MacKeychainDomain};
use keyring_core::Error;

use super::{Backend, SecretError};
use crate::paths::Scope;

pub(super) fn backend(scope: Scope) -> Result<Backend, SecretError> {
    Ok(Backend::Keychain(scope))
}

/// The System keychain for the service (it runs as root), the login
/// keychain for the application.
fn entry(scope: Scope, service: &str, name: &str) -> Result<keyring_core::Entry, SecretError> {
    let domain = match scope {
        Scope::System => MacKeychainDomain::System,
        Scope::User => MacKeychainDomain::User,
    };
    Cred::build(domain, service, name).map_err(keychain)
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
