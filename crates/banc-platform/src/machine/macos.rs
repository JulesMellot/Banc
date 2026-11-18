use std::process::Command;

use super::ResolveFact;

pub(super) fn os_version() -> Option<String> {
    let output = Command::new("/usr/bin/sw_vers")
        .arg("-productVersion")
        .output()
        .ok()?;
    let version = String::from_utf8(output.stdout).ok()?;
    Some(format!("macOS {}", version.trim()))
}

/// Both editions install into `/Applications/DaVinci Resolve`; the bundle's
/// name tells them apart. To be checked on a real machine (section 11).
const BUNDLES: &[&str] = &[
    "/Applications/DaVinci Resolve/DaVinci Resolve.app",
    "/Applications/DaVinci Resolve Studio/DaVinci Resolve Studio.app",
];

pub(super) fn resolve_facts() -> Vec<ResolveFact> {
    BUNDLES
        .iter()
        .filter_map(|bundle| info_plist(&format!("{bundle}/Contents/Info.plist")))
        .map(ResolveFact::InfoPlist)
        .collect()
}

/// The plist as XML: `plutil` converts a binary plist.
fn info_plist(path: &str) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    if !bytes.starts_with(b"bplist") {
        return String::from_utf8(bytes).ok();
    }
    let output = Command::new("/usr/bin/plutil")
        .args(["-convert", "xml1", "-o", "-", path])
        .output()
        .ok()?;
    String::from_utf8(output.stdout).ok()
}

/// `pgrep` answers 0 when a process has this exact name, 1 when none has.
pub(super) fn resolve_running() -> Option<bool> {
    let status = std::process::Command::new("/usr/bin/pgrep")
        .args(["-x", "Resolve"])
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
