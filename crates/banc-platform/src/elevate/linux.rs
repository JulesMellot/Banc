use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

use super::ElevateError;

/// pkexec returns 126 when the user dismisses the prompt, 127 when not
/// authorized.
pub(super) fn run(program: &Path, args: &[&OsStr]) -> Result<(), ElevateError> {
    let status = Command::new("pkexec").arg(program).args(args).status()?;
    match status.code() {
        Some(0) => Ok(()),
        Some(126 | 127) => Err(ElevateError::Declined),
        Some(code) => Err(ElevateError::Failed(code)),
        None => Err(ElevateError::Failed(-1)),
    }
}
