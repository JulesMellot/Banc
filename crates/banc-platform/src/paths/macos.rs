use std::path::PathBuf;

use super::{required, Env, PathError, Scope};

pub(super) fn log_dir(scope: Scope, env: Env<'_>) -> Result<PathBuf, PathError> {
    match scope {
        Scope::System => Ok(PathBuf::from("/Library/Logs/Banc")),
        Scope::User => Ok(required(env, "HOME")?.join("Library/Logs/Banc")),
    }
}

#[cfg(test)]
mod tests {
    use super::super::fake_env;
    use super::*;

    #[test]
    fn journaux_du_service() {
        let dir = log_dir(Scope::System, &fake_env(&[])).unwrap();
        assert_eq!(dir, PathBuf::from("/Library/Logs/Banc"));
    }

    #[test]
    fn journaux_de_session() {
        let env = fake_env(&[("HOME", "/Users/lea")]);
        let dir = log_dir(Scope::User, &env).unwrap();
        assert_eq!(dir, PathBuf::from("/Users/lea/Library/Logs/Banc"));
    }

    #[test]
    fn journaux_de_session_sans_home() {
        let err = log_dir(Scope::User, &fake_env(&[])).unwrap_err();
        assert_eq!(err, PathError::MissingEnv("HOME"));
    }
}
