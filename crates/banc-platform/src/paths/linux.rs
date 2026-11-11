use std::path::PathBuf;

use super::{required, Env, PathError, Scope};

pub(super) fn log_dir(scope: Scope, env: Env<'_>) -> Result<PathBuf, PathError> {
    match scope {
        Scope::System => Ok(PathBuf::from("/var/log/banc")),
        Scope::User => {
            // Une variable XDG vide vaut une variable absente.
            let state = match required(env, "XDG_STATE_HOME") {
                Ok(dir) => dir,
                Err(_) => required(env, "HOME")?.join(".local/state"),
            };
            Ok(state.join("banc/logs"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::fake_env;
    use super::*;

    #[test]
    fn journaux_du_service() {
        let dir = log_dir(Scope::System, &fake_env(&[])).unwrap();
        assert_eq!(dir, PathBuf::from("/var/log/banc"));
    }

    #[test]
    fn journaux_de_session_dans_xdg_state_home() {
        let env = fake_env(&[("XDG_STATE_HOME", "/data/state"), ("HOME", "/home/lea")]);
        let dir = log_dir(Scope::User, &env).unwrap();
        assert_eq!(dir, PathBuf::from("/data/state/banc/logs"));
    }

    #[test]
    fn journaux_de_session_sans_xdg() {
        let env = fake_env(&[("XDG_STATE_HOME", ""), ("HOME", "/home/lea")]);
        let dir = log_dir(Scope::User, &env).unwrap();
        assert_eq!(dir, PathBuf::from("/home/lea/.local/state/banc/logs"));
    }

    #[test]
    fn journaux_de_session_sans_home() {
        let err = log_dir(Scope::User, &fake_env(&[])).unwrap_err();
        assert_eq!(err, PathError::MissingEnv("HOME"));
    }
}
