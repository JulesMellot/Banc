use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    RegDeleteKeyValueW, RegSetKeyValueW, HKEY_LOCAL_MACHINE, REG_SZ,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_FONTCHANGE,
};

/// The fonts Windows loads when a user logs in.
const FONTS_KEY: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts";

fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(std::iter::once(0)).collect()
}

pub(super) fn folder() -> std::io::Result<PathBuf> {
    let windows =
        std::env::var_os("WINDIR").ok_or_else(|| std::io::Error::other("WINDIR is not set"))?;
    Ok(PathBuf::from(windows).join("Fonts"))
}

/// Name of the font's entry in the registry.
fn entry(font: &Path) -> Vec<u16> {
    let stem = font.file_stem().unwrap_or_default().to_string_lossy();
    let opentype = font
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("otf"));
    let kind = if opentype { "OpenType" } else { "TrueType" };
    wide(OsStr::new(&format!("{stem} ({kind})")))
}

/// Tells the programs that are running that the fonts changed.
fn announce() {
    // SAFETY: a broadcast without pointers; the timeout bounds the wait.
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_FONTCHANGE,
            0,
            0,
            SMTO_ABORTIFHUNG,
            1000,
            std::ptr::null_mut(),
        );
    }
}

pub(super) fn registered(font: &Path) -> std::io::Result<()> {
    let key = wide(OsStr::new(FONTS_KEY));
    let name = entry(font);
    let value = wide(font.file_name().unwrap_or_default());
    let size = u32::try_from(value.len() * 2).map_err(std::io::Error::other)?;
    // SAFETY: the three strings end with a zero and outlive the call, and
    // `size` is the length of `value` in bytes.
    let status = unsafe {
        RegSetKeyValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            name.as_ptr(),
            REG_SZ,
            value.as_ptr().cast(),
            size,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(std::io::Error::from_raw_os_error(status.cast_signed()));
    }
    announce();
    Ok(())
}

pub(super) fn unregistered(font: &Path) -> std::io::Result<()> {
    let key = wide(OsStr::new(FONTS_KEY));
    let name = entry(font);
    // SAFETY: both strings end with a zero and outlive the call.
    let status = unsafe { RegDeleteKeyValueW(HKEY_LOCAL_MACHINE, key.as_ptr(), name.as_ptr()) };
    if status != ERROR_SUCCESS && status != ERROR_FILE_NOT_FOUND {
        return Err(std::io::Error::from_raw_os_error(status.cast_signed()));
    }
    announce();
    Ok(())
}
