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

#[cfg(test)]
mod tests {
    use super::*;

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
