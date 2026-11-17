//! Errors sent to the window: a stable code, which the interface turns into
//! French text (`error.<code>`), and an English message for the logs.

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct CommandError {
    pub code: String,
    pub message: String,
}

impl CommandError {
    pub fn new(code: &str, message: impl std::fmt::Display) -> Self {
        Self {
            code: code.to_owned(),
            message: message.to_string(),
        }
    }
}

pub type CommandResult<T> = Result<T, CommandError>;
