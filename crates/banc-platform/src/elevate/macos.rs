use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

use super::{applescript_quote, shell_quote, ElevateError};

/// `do shell script … with administrator privileges`: macOS shows its own
/// password prompt. Error -128 is the user's cancel.
pub(super) fn run(program: &Path, args: &[&OsStr]) -> Result<(), ElevateError> {
    let mut command = shell_quote(&program.to_string_lossy());
    for arg in args {
        command.push(' ');
        command.push_str(&shell_quote(&arg.to_string_lossy()));
    }
    let script = format!(
        "do shell script {} with administrator privileges",
        applescript_quote(&command)
    );
    let output = Command::new("/usr/bin/osascript")
        .args(["-e", &script])
        .output()?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("-128") {
        Err(ElevateError::Declined)
    } else {
        Err(ElevateError::Failed(output.status.code().unwrap_or(-1)))
    }
}
