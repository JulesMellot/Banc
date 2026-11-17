//! Secrets: the machine key of the agent, the admin key of the application.
//!
//! macOS: the keychain (System for the service, the user's login keychain
//! for the application). Windows: the Credential Manager of the account
//! that runs the process. Linux: files readable by their owner only, in the
//! data folder; a system service on Linux has no keychain.
//!
//! [`SecretStore::in_dir`] keeps files in a chosen folder, on every OS: for
//! development and tests.

use std::path::{Path, PathBuf};

use crate::paths::{PathError, Scope};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as os;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as os;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as os;

/// Service name of the keychain entries.
#[cfg(any(target_os = "macos", target_os = "windows"))]
const SERVICE: &str = "Banc";

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("keychain error: {0}")]
    Keychain(String),
    #[error("secret file {path}")]
    File {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Path(#[from] PathError),
    #[error("invalid secret name `{0}`")]
    Name(String),
}

#[derive(Debug, Clone)]
pub struct SecretStore {
    backend: Backend,
}

#[derive(Debug, Clone)]
enum Backend {
    Files(PathBuf),
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    Keychain(Scope),
}

impl SecretStore {
    /// The OS store for this scope.
    pub fn os(scope: Scope) -> Result<Self, SecretError> {
        Ok(Self {
            backend: os::backend(scope)?,
        })
    }

    /// Files in `dir`, mode 0600.
    pub fn in_dir(dir: impl Into<PathBuf>) -> Self {
        Self {
            backend: Backend::Files(dir.into()),
        }
    }

    pub fn get(&self, name: &str) -> Result<Option<Vec<u8>>, SecretError> {
        check_name(name)?;
        match &self.backend {
            Backend::Files(dir) => read_file(&dir.join(name)),
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            Backend::Keychain(scope) => os::get(*scope, SERVICE, name),
        }
    }

    pub fn set(&self, name: &str, secret: &[u8]) -> Result<(), SecretError> {
        check_name(name)?;
        match &self.backend {
            Backend::Files(dir) => {
                let path = dir.join(name);
                crate::fs::write_private(&path, secret)
                    .map_err(|source| SecretError::File { path, source })
            }
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            Backend::Keychain(scope) => os::set(*scope, SERVICE, name, secret),
        }
    }

    pub fn delete(&self, name: &str) -> Result<(), SecretError> {
        check_name(name)?;
        match &self.backend {
            Backend::Files(dir) => {
                let path = dir.join(name);
                match std::fs::remove_file(&path) {
                    Err(source) if source.kind() != std::io::ErrorKind::NotFound => {
                        Err(SecretError::File { path, source })
                    }
                    _ => Ok(()),
                }
            }
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            Backend::Keychain(scope) => os::delete(*scope, SERVICE, name),
        }
    }
}

/// A name is also a file name: lowercase letters, digits and dashes.
fn check_name(name: &str) -> Result<(), SecretError> {
    let valid = !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if valid {
        Ok(())
    } else {
        Err(SecretError::Name(name.to_owned()))
    }
}

fn read_file(path: &Path) -> Result<Option<Vec<u8>>, SecretError> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(SecretError::File {
            path: path.to_path_buf(),
            source,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SecretStore::in_dir(tmp.path().join("secrets"));
        assert_eq!(store.get("machine-key").unwrap(), None);
        store.set("machine-key", b"secret").unwrap();
        assert_eq!(
            store.get("machine-key").unwrap().as_deref(),
            Some(&b"secret"[..])
        );
        store.delete("machine-key").unwrap();
        store.delete("machine-key").unwrap();
        assert_eq!(store.get("machine-key").unwrap(), None);
    }

    #[test]
    fn names_checked() {
        let store = SecretStore::in_dir("/nowhere");
        for name in ["", "../x", "Machine", "a/b", "a.b"] {
            assert!(
                matches!(store.get(name), Err(SecretError::Name(_))),
                "{name}"
            );
        }
    }
}
