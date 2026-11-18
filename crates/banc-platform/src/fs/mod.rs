//! Files that only the owner of the process may read.

use std::io::Write;
use std::path::Path;

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use unix as os;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as os;

/// Writes a file readable by its owner only, in a single step: the content
/// goes to a temporary file in the same folder, which is then renamed. A
/// crash never leaves a half-written file.
///
/// For the server's own key, on the VPS, which has no OS keychain. The
/// workstations keep their secrets in the keychain.
pub fn write_private(path: &Path, content: &[u8]) -> std::io::Result<()> {
    let dir = match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    };
    std::fs::create_dir_all(dir)?;
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    let temporary = dir.join(name);
    let mut file = os::create_private(&temporary)?;
    file.write_all(content)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&temporary, path)
}

/// Permissions of a file of an installed application.
pub fn set_installed_mode(path: &Path, executable: bool) -> std::io::Result<()> {
    os::set_installed_mode(path, executable)
}

/// Makes `path` an empty folder that only the system and administrators can
/// enter. Whatever was there is removed first, including a folder prepared
/// by someone else: what is about to run from it must not be replaceable by
/// a local user.
pub fn create_private_dir(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_dir_all(path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    os::create_private_dir(path)
}

/// Creates a symbolic link, as found inside macOS bundles. Windows refuses:
/// a package made for it does not contain any.
pub fn symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    os::symlink(target, link)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_private_folder_starts_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("work/downloads");
        create_private_dir(&dir).unwrap();
        std::fs::write(dir.join("left-over"), b"x").unwrap();
        create_private_dir(&dir).unwrap();
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        std::fs::write(dir.join("usable"), b"x").unwrap();
    }

    #[test]
    fn written_then_replaced() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("keys/server.key");
        write_private(&path, b"first").unwrap();
        write_private(&path, b"second").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        assert!(!tmp.path().join("keys/server.key.tmp").exists());
    }
}
