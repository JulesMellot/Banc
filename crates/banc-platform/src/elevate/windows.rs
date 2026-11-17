use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_CANCELLED};
use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

use super::ElevateError;

fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(std::iter::once(0)).collect()
}

/// Quotes one argument the way the C runtime splits a command line.
fn quote(arg: &OsStr) -> String {
    let arg = arg.to_string_lossy();
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg.into_owned();
    }
    let mut out = String::from('"');
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.push_str(&"\\".repeat(backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.push_str(&"\\".repeat(backslashes));
                out.push(c);
                backslashes = 0;
            }
        }
    }
    out.push_str(&"\\".repeat(backslashes * 2));
    out.push('"');
    out
}

/// The "runas" verb shows the UAC prompt.
pub(super) fn run(program: &Path, args: &[&OsStr]) -> Result<(), ElevateError> {
    let parameters: Vec<String> = args.iter().map(|arg| quote(arg)).collect();
    let verb = wide(OsStr::new("runas"));
    let file = wide(program.as_os_str());
    let parameters = wide(OsStr::new(&parameters.join(" ")));
    // SAFETY: zero is a valid initial value for this plain C structure.
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = parameters.as_ptr();
    info.nShow = SW_HIDE;
    // SAFETY: the strings outlive the call; `info` is fully initialized.
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_CANCELLED as i32) {
            return Err(ElevateError::Declined);
        }
        return Err(error.into());
    }
    let mut code: u32 = 1;
    // SAFETY: SEE_MASK_NOCLOSEPROCESS gives a process handle, closed below.
    unsafe {
        WaitForSingleObject(info.hProcess, INFINITE);
        GetExitCodeProcess(info.hProcess, &mut code);
        CloseHandle(info.hProcess);
    }
    if code == 0 {
        Ok(())
    } else {
        Err(ElevateError::Failed(code as i32))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_line_quoting() {
        assert_eq!(quote(OsStr::new("install")), "install");
        assert_eq!(quote(OsStr::new("a b")), "\"a b\"");
        assert_eq!(quote(OsStr::new(r#"say "hi""#)), r#""say \"hi\"""#);
        assert_eq!(quote(OsStr::new(r"C:\dir\")), r#"C:\dir\"#);
        assert_eq!(quote(OsStr::new(r"C:\my dir\")), r#""C:\my dir\\""#);
    }
}
