use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub(super) fn folder() -> std::io::Result<PathBuf> {
    Ok(PathBuf::from("/usr/local/share/fonts/banc"))
}

/// Refreshes the font cache. Without `fontconfig` nothing reads the cache.
fn refresh() {
    let refreshed = Command::new("fc-cache")
        .arg("-f")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if let Err(error) = refreshed {
        tracing::debug!("fc-cache: {error}");
    }
}

pub(super) fn registered(_font: &Path) -> std::io::Result<()> {
    refresh();
    Ok(())
}

pub(super) fn unregistered(_font: &Path) -> std::io::Result<()> {
    refresh();
    Ok(())
}
