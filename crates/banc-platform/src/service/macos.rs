use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::{LAUNCHD_LABEL, SESSION_LABEL, UPDATER_LABEL};

fn plist_path() -> PathBuf {
    Path::new("/Library/LaunchDaemons").join(format!("{LAUNCHD_LABEL}.plist"))
}

fn updater_plist_path() -> PathBuf {
    Path::new("/Library/LaunchDaemons").join(format!("{UPDATER_LABEL}.plist"))
}

/// In `/Library/LaunchAgents`: for every user who logs in.
fn session_plist_path() -> PathBuf {
    Path::new("/Library/LaunchAgents").join(format!("{SESSION_LABEL}.plist"))
}

pub(super) fn install(program: &Path) -> std::io::Result<()> {
    // Replace a previous registration.
    let _ = launchctl(&["bootout", &format!("system/{LAUNCHD_LABEL}")]);
    let path = plist_path();
    std::fs::write(&path, super::launchd_plist(program))?;
    launchctl(&["bootstrap", "system", &path.to_string_lossy()])
}

pub(super) fn uninstall() -> std::io::Result<()> {
    if let Some(user) = console_user() {
        let _ = launchctl(&["bootout", &format!("gui/{user}/{SESSION_LABEL}")]);
    }
    let _ = std::fs::remove_file(session_plist_path());
    let _ = launchctl(&["bootout", &format!("system/{LAUNCHD_LABEL}")]);
    match std::fs::remove_file(plist_path()) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// `bootout` returns once the job has stopped. Already stopped is fine.
pub(super) fn stop() -> std::io::Result<()> {
    let _ = launchctl(&["bootout", &format!("system/{LAUNCHD_LABEL}")]);
    Ok(())
}

pub(super) fn start() -> std::io::Result<()> {
    launchctl(&["bootstrap", "system", &plist_path().to_string_lossy()])
}

/// `KeepAlive` already restarts the service. Installs the tray icon for the
/// next logins, and starts it now for the user at the screen.
pub(super) fn keep_running(session: &Path) -> std::io::Result<()> {
    let path = session_plist_path();
    let text = super::session_plist(session);
    if std::fs::read_to_string(&path).is_ok_and(|current| current == text) {
        return Ok(());
    }
    std::fs::create_dir_all("/Library/LaunchAgents")?;
    std::fs::write(&path, text)?;
    if let Some(user) = console_user() {
        let domain = format!("gui/{user}");
        let _ = launchctl(&["bootout", &format!("{domain}/{SESSION_LABEL}")]);
        launchctl(&["bootstrap", &domain, &path.to_string_lossy()])?;
    }
    Ok(())
}

/// The user logged in at the screen, `None` at the login window.
fn console_user() -> Option<u32> {
    let output = Command::new("/usr/bin/stat")
        .args(["-f", "%u", "/dev/console"])
        .output()
        .ok()?;
    let user: u32 = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .ok()?;
    (user != 0).then_some(user)
}

pub(super) fn launch_updater(program: &Path, args: &[OsString]) -> std::io::Result<()> {
    let _ = launchctl(&["bootout", &format!("system/{UPDATER_LABEL}")]);
    let path = updater_plist_path();
    std::fs::write(&path, super::updater_plist(program, args))?;
    launchctl(&["bootstrap", "system", &path.to_string_lossy()])
}

/// Removes the job. launchd ends this process with it.
pub(super) fn finish_updater() {
    let _ = std::fs::remove_file(updater_plist_path());
    let _ = launchctl(&["bootout", &format!("system/{UPDATER_LABEL}")]);
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
