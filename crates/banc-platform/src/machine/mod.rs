//! What the machine is: name, OS version, installed DaVinci Resolve.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as os;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as os;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as os;

/// `macos`, `windows` or `linux`.
pub const OS: &str = std::env::consts::OS;
/// `x86_64` or `aarch64`.
pub const ARCH: &str = std::env::consts::ARCH;

/// Computer name, without the `.local` that macOS adds.
pub fn hostname() -> String {
    let name = gethostname::gethostname().to_string_lossy().into_owned();
    name.strip_suffix(".local").unwrap_or(&name).to_owned()
}

/// Human-readable OS version, `macOS 26.1`, `Windows 11 Pro 25H2`,
/// `Debian GNU/Linux 13 (trixie)`. Empty when unknown.
pub fn os_version() -> String {
    os::os_version().unwrap_or_default()
}

/// Raw facts about the installed DaVinci Resolve, read where each OS keeps
/// them. `banc-resolve` turns them into an edition and a version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveFact {
    /// macOS: the application bundle's `Info.plist`, as XML.
    InfoPlist(String),
    /// Windows: an entry of the installed programs list.
    InstalledProgram { name: String, version: String },
    /// Linux: a text file of the installation (desktop entry, read-me).
    InstallFile { name: String, text: String },
}

/// Everything found. Empty when Resolve is not installed.
pub fn resolve_facts() -> Vec<ResolveFact> {
    os::resolve_facts()
}

/// Whether DaVinci Resolve is running. When the OS cannot tell, the answer
/// is yes: the caller is about to do something that must wait for Resolve.
pub fn resolve_running() -> bool {
    os::resolve_running().unwrap_or(true)
}

/// Value of `KEY=value` or `KEY="value"` in an `os-release` style file.
pub fn key_value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().find_map(|line| {
        let value = line.strip_prefix(key)?.strip_prefix('=')?;
        Some(value.trim().trim_matches('"'))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_release() {
        let text =
            "NAME=\"Debian GNU/Linux\"\nPRETTY_NAME=\"Debian GNU/Linux 13 (trixie)\"\nID=debian\n";
        assert_eq!(
            key_value(text, "PRETTY_NAME"),
            Some("Debian GNU/Linux 13 (trixie)")
        );
        assert_eq!(key_value(text, "ID"), Some("debian"));
        assert_eq!(key_value(text, "NAM"), None);
        assert_eq!(key_value(text, "VERSION"), None);
    }

    #[test]
    fn this_machine() {
        assert!(!hostname().is_empty());
        assert!(["macos", "windows", "linux"].contains(&OS));
        // Reading must not fail on a machine without Resolve.
        let _ = resolve_facts();
        let _ = resolve_running();
        let _ = os_version();
    }
}
