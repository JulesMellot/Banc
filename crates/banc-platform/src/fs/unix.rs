use std::fs::{File, OpenOptions};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

/// Mode 0600, set at creation: the file is never readable by others, even
/// for an instant.
pub(super) fn create_private(path: &Path) -> std::io::Result<File> {
    // A leftover temporary file may have other permissions.
    match std::fs::remove_file(path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn owner_only() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("secret");
        super::super::write_private(&path, b"x").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
