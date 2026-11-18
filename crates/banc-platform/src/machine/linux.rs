use super::ResolveFact;

pub(super) fn os_version() -> Option<String> {
    let text = std::fs::read_to_string("/etc/os-release").ok()?;
    super::key_value(&text, "PRETTY_NAME").map(str::to_owned)
}

/// The Linux installer puts Resolve in `/opt/resolve`. To be checked on a
/// real machine (section 11 of the plan): the desktop entries give the
/// edition, the read-me the version.
const FILES: &[&str] = &[
    "/usr/share/applications/com.blackmagicdesign.resolve.desktop",
    "/usr/share/applications/DaVinciResolve.desktop",
    "/opt/resolve/docs/ReadMe.html",
    "/opt/resolve/docs/DaVinci Resolve Studio ReadMe.html",
    "/opt/resolve/docs/DaVinci Resolve ReadMe.html",
];

pub(super) fn resolve_facts() -> Vec<ResolveFact> {
    if !std::path::Path::new("/opt/resolve/bin/resolve").exists() {
        return Vec::new();
    }
    FILES
        .iter()
        .filter_map(|path| {
            let text = std::fs::read_to_string(path).ok()?;
            Some(ResolveFact::InstallFile {
                name: (*path).to_owned(),
                text,
            })
        })
        .collect()
}

/// `pgrep` answers 0 when a process has this exact name, 1 when none has.
pub(super) fn resolve_running() -> Option<bool> {
    let status = std::process::Command::new("pgrep")
        .args(["-x", "resolve"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()?;
    match status.code() {
        Some(0) => Some(true),
        Some(1) => Some(false),
        _ => None,
    }
}
