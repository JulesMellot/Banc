use std::path::Path;
use std::process::Command;

use super::LAUNCHD_LABEL;

fn plist_path() -> std::path::PathBuf {
    Path::new("/Library/LaunchDaemons").join(format!("{LAUNCHD_LABEL}.plist"))
}

pub(super) fn install(program: &Path) -> std::io::Result<()> {
    // Replace a previous registration.
    let _ = launchctl(&["bootout", &format!("system/{LAUNCHD_LABEL}")]);
    let path = plist_path();
    std::fs::write(&path, super::launchd_plist(program))?;
    launchctl(&["bootstrap", "system", &path.to_string_lossy()])
}

pub(super) fn uninstall() -> std::io::Result<()> {
    let _ = launchctl(&["bootout", &format!("system/{LAUNCHD_LABEL}")]);
    match std::fs::remove_file(plist_path()) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

pub(super) fn run(main: impl FnOnce() -> u32 + Send + 'static) -> std::io::Result<u32> {
    Ok(main())
}

fn launchctl(args: &[&str]) -> std::io::Result<()> {
    let status = Command::new("/bin/launchctl").args(args).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "launchctl {} failed: {status}",
            args.join(" ")
        )))
    }
}
