use std::fs::{File, OpenOptions};
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows_sys::Win32::Storage::FileSystem::CreateDirectoryW;

/// Full control for the system, administrators and the folder's owner, and
/// nothing inherited from the parent folder.
const PRIVATE_DIR: &str = "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;OW)";

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

pub(super) fn create_private_dir(path: &Path) -> std::io::Result<()> {
    let rules: Vec<u16> = PRIVATE_DIR.encode_utf16().chain([0]).collect();
    let name: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    // SAFETY: `rules` ends with a zero, and `descriptor` receives a buffer
    // released below.
    let converted = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            rules.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            std::ptr::null_mut(),
        )
    };
    if converted == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    // SAFETY: `name` ends with a zero and `attributes` outlives the call.
    let created = unsafe { CreateDirectoryW(name.as_ptr(), &attributes) };
    let result = if created == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    };
    // SAFETY: the buffer comes from the conversion above and is not used again.
    unsafe { LocalFree(descriptor) };
    result
}

pub(super) fn symlink(_target: &Path, _link: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "symbolic links are not installed on Windows",
    ))
}
