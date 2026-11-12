//! Locations of Banc's files on each OS.

use std::ffi::OsString;
use std::path::PathBuf;

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

/// Who writes: the system service or a process in the user session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// `banc-agent` and `banc-server`, started by the service manager.
    System,
    /// `banc-session` and the window, started in the user's session.
    User,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PathError {
    #[error("variable d'environnement {0} absente ou vide")]
    MissingEnv(&'static str),
}

/// Reads an environment variable, injectable in tests.
type Env<'a> = &'a dyn Fn(&str) -> Option<OsString>;

/// Banc's log folder.
pub fn log_dir(scope: Scope) -> Result<PathBuf, PathError> {
    os::log_dir(scope, &|name| std::env::var_os(name))
}

/// Required environment variable, refused if empty.
fn required(env: Env<'_>, name: &'static str) -> Result<PathBuf, PathError> {
    match env(name) {
        Some(value) if !value.is_empty() => Ok(PathBuf::from(value)),
        _ => Err(PathError::MissingEnv(name)),
    }
}

#[cfg(test)]
fn fake_env(vars: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<OsString> {
    move |name| {
        vars.iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| OsString::from(value))
    }
}
