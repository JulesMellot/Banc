use std::path::PathBuf;

use super::{required, Env, PathError, ResolveDirs, Scope};

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

pub(super) fn resolve_dirs(_env: Env<'_>) -> Result<ResolveDirs, PathError> {
    let support = PathBuf::from("/Library/Application Support/Blackmagic Design/DaVinci Resolve");
    Ok(ResolveDirs {
        ofx: PathBuf::from("/Library/OFX/Plugins"),
        lut: support.join("LUT"),
        fusion_templates: support.join("Fusion/Templates"),
        scripts: support.join("Fusion/Scripts"),
    })
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
    fn resolve_folders() {
        let dirs = resolve_dirs(&fake_env(&[])).unwrap();
        assert_eq!(dirs.ofx, PathBuf::from("/Library/OFX/Plugins"));
        assert_eq!(
            dirs.lut,
            PathBuf::from("/Library/Application Support/Blackmagic Design/DaVinci Resolve/LUT")
        );
    }

    #[test]
    fn session_logs_without_home() {
        let err = log_dir(Scope::User, &fake_env(&[])).unwrap_err();
        assert_eq!(err, PathError::MissingEnv("HOME"));
    }
}
