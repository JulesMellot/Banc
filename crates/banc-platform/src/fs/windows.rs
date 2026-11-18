use std::fs::{File, OpenOptions};
use std::path::Path;

/// On Windows the file inherits the folder's permissions. The server does
/// not run on Windows; the workstations use the keychain.
pub(super) fn create_private(path: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
}

/// Files inherit the permissions of the application's folder.
pub(super) fn set_installed_mode(_path: &Path, _executable: bool) -> std::io::Result<()> {
    Ok(())
}
