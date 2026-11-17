//! Running a program with administrator rights, after the OS has asked the
//! user for them: the administrator password prompt on macOS, the UAC
//! prompt on Windows, `pkexec` on Linux.
//!
//! The only use: installing the agent service from the window, the one time
//! Banc asks for the administrator password (section 1 of the plan).

use std::ffi::OsStr;
use std::path::Path;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as os;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as os;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as os;

#[derive(Debug, thiserror::Error)]
pub enum ElevateError {
    #[error("the user declined to give administrator rights")]
    Declined,
    #[error("the program failed with code {0}")]
    Failed(i32),
    #[error("cannot start the program")]
    Io(#[from] std::io::Error),
}

/// Runs `program` with `args` as administrator and waits for it. Blocks:
/// call it off the async threads.
pub fn run(program: &Path, args: &[&OsStr]) -> Result<(), ElevateError> {
    os::run(program, args)
}

/// Quotes a word for `/bin/sh`.
pub fn shell_quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}

/// Quotes a text for an AppleScript string literal.
pub fn applescript_quote(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', r"\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting() {
        assert_eq!(
            shell_quote("/Applications/Banc.app"),
            "'/Applications/Banc.app'"
        );
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
        assert_eq!(applescript_quote(r#"a "b" \c"#), r#""a \"b\" \\c""#);
    }
}
