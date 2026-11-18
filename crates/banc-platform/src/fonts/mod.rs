//! Fonts installed for every user of the workstation.

use std::path::{Path, PathBuf};

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

/// Where the system keeps the font called `file_name`.
pub fn path_of(file_name: &str) -> std::io::Result<PathBuf> {
    Ok(os::folder()?.join(file_name))
}

/// Installs a font file and tells the system about it. Returns where it is.
pub fn install(source: &Path, file_name: &str) -> std::io::Result<PathBuf> {
    let target = path_of(file_name)?;
    if let Some(folder) = target.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let temporary = target.with_extension("banc-new");
    std::fs::copy(source, &temporary)?;
    crate::fs::set_installed_mode(&temporary, false)?;
    std::fs::rename(&temporary, &target)?;
    os::registered(&target)?;
    Ok(target)
}

/// Removes a font that Banc installed.
pub fn remove(installed: &Path) -> std::io::Result<()> {
    os::unregistered(installed)?;
    match std::fs::remove_file(installed) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}
