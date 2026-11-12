use std::path::PathBuf;

use super::{required, Env, PathError, Scope};

pub(super) fn log_dir(scope: Scope, env: Env<'_>) -> Result<PathBuf, PathError> {
    let name = match scope {
        Scope::System => "PROGRAMDATA",
        Scope::User => "LOCALAPPDATA",
    };
    Ok(required(env, name)?.join("Banc").join("logs"))
}

#[cfg(test)]
mod tests {
    use super::super::fake_env;
    use super::*;

    #[test]
    fn service_logs() {
        let env = fake_env(&[("PROGRAMDATA", r"C:\ProgramData")]);
        let dir = log_dir(Scope::System, &env).unwrap();
        assert_eq!(dir, PathBuf::from(r"C:\ProgramData\Banc\logs"));
    }

    #[test]
    fn session_logs() {
        let env = fake_env(&[("LOCALAPPDATA", r"C:\Users\lea\AppData\Local")]);
        let dir = log_dir(Scope::User, &env).unwrap();
        assert_eq!(dir, PathBuf::from(r"C:\Users\lea\AppData\Local\Banc\logs"));
    }

    #[test]
    fn logs_without_variable() {
        let err = log_dir(Scope::User, &fake_env(&[])).unwrap_err();
        assert_eq!(err, PathError::MissingEnv("LOCALAPPDATA"));
    }
}
