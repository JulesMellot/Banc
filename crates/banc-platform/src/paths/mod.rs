//! Emplacements des fichiers de Banc sur chaque OS.

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

/// Qui écrit : le service système ou un processus de la session utilisateur.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// `banc-agent` et `banc-server`, lancés par le gestionnaire de services.
    System,
    /// `banc-session` et la fenêtre, lancés dans la session de l'utilisateur.
    User,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PathError {
    #[error("variable d'environnement {0} absente ou vide")]
    MissingEnv(&'static str),
}

/// Lecture d'une variable d'environnement, injectable dans les tests.
type Env<'a> = &'a dyn Fn(&str) -> Option<OsString>;

/// Dossier des journaux de Banc.
pub fn log_dir(scope: Scope) -> Result<PathBuf, PathError> {
    os::log_dir(scope, &|name| std::env::var_os(name))
}

/// Variable d'environnement obligatoire, refusée si elle est vide.
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
