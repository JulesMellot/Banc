use std::path::{Path, PathBuf};

/// macOS picks up the files of this folder by itself.
pub(super) fn folder() -> std::io::Result<PathBuf> {
    Ok(PathBuf::from("/Library/Fonts"))
}

pub(super) fn registered(_font: &Path) -> std::io::Result<()> {
    Ok(())
}

pub(super) fn unregistered(_font: &Path) -> std::io::Result<()> {
    Ok(())
}
