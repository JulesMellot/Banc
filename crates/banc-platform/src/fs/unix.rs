use std::fs::{File, OpenOptions};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
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

pub(super) fn set_installed_mode(path: &Path, executable: bool) -> std::io::Result<()> {
    let mode = if executable { 0o755 } else { 0o644 };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

pub(super) fn create_private_dir(path: &Path) -> std::io::Result<()> {
    std::fs::DirBuilder::new().mode(0o700).create(path)
}

pub(super) fn symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
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

    #[test]
    fn a_private_folder_is_closed_to_others() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("private");
        super::super::create_private_dir(&dir).unwrap();
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
    }

    #[test]
    fn installed_modes() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("program");
        std::fs::write(&path, b"x").unwrap();
        for (executable, expected) in [(true, 0o755), (false, 0o644)] {
            super::super::set_installed_mode(&path, executable).unwrap();
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, expected);
        }
    }
}
