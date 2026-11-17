use std::path::PathBuf;

use super::{required, Env, PathError, Scope};

pub(super) fn log_dir(scope: Scope, env: Env<'_>) -> Result<PathBuf, PathError> {
    match scope {
        Scope::System => Ok(PathBuf::from("/Library/Logs/Banc")),
        Scope::User => Ok(required(env, "HOME")?.join("Library/Logs/Banc")),
    }
}

pub(super) const AGENT_SOCKET: &str = "/var/run/banc/agent.sock";

pub(super) fn state_dir(scope: Scope, env: Env<'_>) -> Result<PathBuf, PathError> {
    match scope {
        Scope::System => Ok(PathBuf::from("/Library/Application Support/Banc")),
        Scope::User => Ok(required(env, "HOME")?.join("Library/Application Support/Banc")),
    }
}

#[cfg(test)]
mod tests {
    use super::super::fake_env;
    use super::*;

    #[test]
    fn service_logs() {
        let dir = log_dir(Scope::System, &fake_env(&[])).unwrap();
        assert_eq!(dir, PathBuf::from("/Library/Logs/Banc"));
    }

    #[test]
    fn session_logs() {
        let env = fake_env(&[("HOME", "/Users/lea")]);
        let dir = log_dir(Scope::User, &env).unwrap();
        assert_eq!(dir, PathBuf::from("/Users/lea/Library/Logs/Banc"));
    }

    #[test]
    fn state_folders() {
        let env = fake_env(&[("HOME", "/Users/lea")]);
        assert_eq!(
            state_dir(Scope::System, &env).unwrap(),
            PathBuf::from("/Library/Application Support/Banc")
        );
        assert_eq!(
            state_dir(Scope::User, &env).unwrap(),
            PathBuf::from("/Users/lea/Library/Application Support/Banc")
        );
    }

    #[test]
    fn session_logs_without_home() {
        let err = log_dir(Scope::User, &fake_env(&[])).unwrap_err();
        assert_eq!(err, PathError::MissingEnv("HOME"));
    }
}
