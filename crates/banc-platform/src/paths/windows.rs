use std::path::PathBuf;

use super::{required, Env, PathError, ResolveDirs, Scope};

pub(super) fn log_dir(scope: Scope, env: Env<'_>) -> Result<PathBuf, PathError> {
    let name = match scope {
        Scope::System => "PROGRAMDATA",
        Scope::User => "LOCALAPPDATA",
    };
    Ok(required(env, name)?.join("Banc").join("logs"))
}

pub(super) const AGENT_SOCKET: &str = r"\\.\pipe\banc-agent";

pub(super) fn state_dir(scope: Scope, env: Env<'_>) -> Result<PathBuf, PathError> {
    let name = match scope {
        Scope::System => "PROGRAMDATA",
        Scope::User => "LOCALAPPDATA",
    };
    Ok(required(env, name)?.join("Banc"))
}

pub(super) fn resolve_dirs(env: Env<'_>) -> Result<ResolveDirs, PathError> {
    let resolve = required(env, "PROGRAMDATA")?
        .join("Blackmagic Design")
        .join("DaVinci Resolve");
    Ok(ResolveDirs {
        ofx: required(env, "CommonProgramFiles")?
            .join("OFX")
            .join("Plugins"),
        lut: resolve.join("Support").join("LUT"),
        fusion_templates: resolve.join("Fusion").join("Templates"),
        scripts: resolve.join("Fusion").join("Scripts"),
    })
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
    fn state_folders() {
        let env = fake_env(&[("PROGRAMDATA", r"C:\ProgramData")]);
        assert_eq!(
            state_dir(Scope::System, &env).unwrap(),
            PathBuf::from(r"C:\ProgramData\Banc")
        );
        assert_eq!(super::AGENT_SOCKET, r"\\.\pipe\banc-agent");
    }

    #[test]
    fn resolve_folders() {
        let env = fake_env(&[
            ("PROGRAMDATA", r"C:\ProgramData"),
            ("CommonProgramFiles", r"C:\Program Files\Common Files"),
        ]);
        let dirs = resolve_dirs(&env).unwrap();
        assert_eq!(
            dirs.ofx,
            PathBuf::from(r"C:\Program Files\Common Files\OFX\Plugins")
        );
        assert_eq!(
            dirs.lut,
            PathBuf::from(r"C:\ProgramData\Blackmagic Design\DaVinci Resolve\Support\LUT")
        );
    }

    #[test]
    fn logs_without_variable() {
        let err = log_dir(Scope::User, &fake_env(&[])).unwrap_err();
        assert_eq!(err, PathError::MissingEnv("LOCALAPPDATA"));
    }
}
