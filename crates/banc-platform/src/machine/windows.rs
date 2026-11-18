use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ,
    KEY_WOW64_64KEY, RRF_RT_REG_SZ,
};

use super::ResolveFact;

const CURRENT_VERSION: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";

pub(super) fn os_version() -> Option<String> {
    let product = string_value(HKEY_LOCAL_MACHINE, CURRENT_VERSION, "ProductName")?;
    let build: u32 = string_value(HKEY_LOCAL_MACHINE, CURRENT_VERSION, "CurrentBuildNumber")
        .and_then(|build| build.parse().ok())
        .unwrap_or(0);
    // Windows 11 still says "Windows 10" in ProductName.
    let product = if build >= 22000 {
        product.replacen("Windows 10", "Windows 11", 1)
    } else {
        product
    };
    match string_value(HKEY_LOCAL_MACHINE, CURRENT_VERSION, "DisplayVersion") {
        Some(display) => Some(format!("{product} {display}")),
        None => Some(product),
    }
}

/// Installed programs whose name starts with "DaVinci Resolve".
pub(super) fn resolve_facts() -> Vec<ResolveFact> {
    subkeys(HKEY_LOCAL_MACHINE, UNINSTALL)
        .into_iter()
        .filter_map(|key| {
            let path = format!(r"{UNINSTALL}\{key}");
            let name = string_value(HKEY_LOCAL_MACHINE, &path, "DisplayName")?;
            if !name.starts_with("DaVinci Resolve") {
                return None;
            }
            let version = string_value(HKEY_LOCAL_MACHINE, &path, "DisplayVersion")?;
            Some(ResolveFact::InstalledProgram { name, version })
        })
        .collect()
}

/// Asks `tasklist` for the processes with Resolve's image name.
pub(super) fn resolve_running() -> Option<bool> {
    let output = std::process::Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq Resolve.exe", "/FO", "CSV", "/NH"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
    Some(text.contains("\"resolve.exe\""))
}

fn wide(text: &str) -> Vec<u16> {
    std::ffi::OsStr::new(text)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn string_value(root: HKEY, path: &str, name: &str) -> Option<String> {
    let path = wide(path);
    let name = wide(name);
    let mut buffer = vec![0u16; 512];
    let mut size = (buffer.len() * 2) as u32;
    // SAFETY: the buffers are valid and `size` is their length in bytes.
    let status = unsafe {
        RegGetValueW(
            root,
            path.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    let chars = (size as usize / 2).saturating_sub(1);
    buffer.truncate(chars);
    Some(OsString::from_wide(&buffer).to_string_lossy().into_owned())
}

fn subkeys(root: HKEY, path: &str) -> Vec<String> {
    let path = wide(path);
    let mut key: HKEY = std::ptr::null_mut();
    // SAFETY: valid NUL-terminated path; the key is closed below.
    let status =
        unsafe { RegOpenKeyExW(root, path.as_ptr(), 0, KEY_READ | KEY_WOW64_64KEY, &mut key) };
    if status != ERROR_SUCCESS {
        return Vec::new();
    }
    let mut names = Vec::new();
    for index in 0.. {
        let mut buffer = vec![0u16; 256];
        let mut len = buffer.len() as u32;
        // SAFETY: `buffer` holds `len` characters.
        let status = unsafe {
            RegEnumKeyExW(
                key,
                index,
                buffer.as_mut_ptr(),
                &mut len,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if status != ERROR_SUCCESS {
            break;
        }
        buffer.truncate(len as usize);
        names.push(OsString::from_wide(&buffer).to_string_lossy().into_owned());
    }
    // SAFETY: opened above.
    unsafe {
        RegCloseKey(key);
    }
    names
}
