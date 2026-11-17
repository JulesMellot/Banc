use std::path::Path;
use std::process::Command;

use super::SYSTEMD_UNIT;

const UNIT_DIR: &str = "/etc/systemd/system";

pub(super) fn install(program: &Path) -> std::io::Result<()> {
    let path = Path::new(UNIT_DIR).join(SYSTEMD_UNIT);
    std::fs::write(&path, super::systemd_unit(program))?;
    systemctl(&["daemon-reload"])?;
    systemctl(&["enable", "--now", SYSTEMD_UNIT])?;
    // A unit that was already running keeps the old binary otherwise.
    systemctl(&["restart", SYSTEMD_UNIT])
}

pub(super) fn uninstall() -> std::io::Result<()> {
    // Not installed: nothing to stop.
    let _ = systemctl(&["disable", "--now", SYSTEMD_UNIT]);
    match std::fs::remove_file(Path::new(UNIT_DIR).join(SYSTEMD_UNIT)) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    systemctl(&["daemon-reload"])
}

pub(super) fn run(main: impl FnOnce() -> u32 + Send + 'static) -> std::io::Result<u32> {
    Ok(main())
}

fn systemctl(args: &[&str]) -> std::io::Result<()> {
    let status = Command::new("systemctl").args(args).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "systemctl {} failed: {status}",
            args.join(" ")
        )))
    }
}
