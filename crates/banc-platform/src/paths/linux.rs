use std::path::PathBuf;

use super::{required, Env, PathError, Scope};

pub(super) fn log_dir(scope: Scope, env: Env<'_>) -> Result<PathBuf, PathError> {
    match scope {
        Scope::System => Ok(PathBuf::from("/var/log/banc")),
        Scope::User => {
            // An empty XDG variable counts as a missing one.
            let state = match required(env, "XDG_STATE_HOME") {
                Ok(dir) => dir,
                Err(_) => required(env, "HOME")?.join(".local/state"),
            };
            Ok(state.join("banc/logs"))
        }
    }
}

pub(super) const AGENT_SOCKET: &str = "/run/banc/agent.sock";

pub(super) fn state_dir(scope: Scope, env: Env<'_>) -> Result<PathBuf, PathError> {
    match scope {
        Scope::System => Ok(PathBuf::from("/var/lib/banc")),
        Scope::User => {
            let data = match required(env, "XDG_DATA_HOME") {
                Ok(dir) => dir,
                Err(_) => required(env, "HOME")?.join(".local/share"),
            };
            Ok(data.join("banc"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::fake_env;
    use super::*;

    #[test]
    fn service_logs() {
        let dir = log_dir(Scope::System, &fake_env(&[])).unwrap();
        assert_eq!(dir, PathBuf::from("/var/log/banc"));
    }

    #[test]
    fn session_logs_in_xdg_state_home() {
        let env = fake_env(&[("XDG_STATE_HOME", "/data/state"), ("HOME", "/home/lea")]);
        let dir = log_dir(Scope::User, &env).unwrap();
        assert_eq!(dir, PathBuf::from("/data/state/banc/logs"));
    }

    #[test]
    fn session_logs_without_xdg() {
        let env = fake_env(&[("XDG_STATE_HOME", ""), ("HOME", "/home/lea")]);
        let dir = log_dir(Scope::User, &env).unwrap();
        assert_eq!(dir, PathBuf::from("/home/lea/.local/state/banc/logs"));
    }

    #[test]
    fn state_folders() {
        let env = fake_env(&[("HOME", "/home/lea")]);
        assert_eq!(
            state_dir(Scope::System, &env).unwrap(),
            PathBuf::from("/var/lib/banc")
        );
        assert_eq!(
            state_dir(Scope::User, &env).unwrap(),
            PathBuf::from("/home/lea/.local/share/banc")
        );
        let env = fake_env(&[("XDG_DATA_HOME", "/data"), ("HOME", "/home/lea")]);
        assert_eq!(
            state_dir(Scope::User, &env).unwrap(),
            PathBuf::from("/data/banc")
        );
    }

    #[test]
    fn session_logs_without_home() {
        let err = log_dir(Scope::User, &fake_env(&[])).unwrap_err();
        assert_eq!(err, PathError::MissingEnv("HOME"));
    }
}
