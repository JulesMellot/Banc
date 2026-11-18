//! Bridge to DaVinci Resolve: version detection, Python script calls, fake
//! for tests.
//!
//! Phase 1: which Resolve is installed. `banc-platform` reads the raw facts
//! where each OS keeps them; this crate decides the edition and the
//! version. The sources are to be confirmed on real machines (section 11 of
//! the plan): each parser is tolerant and returns nothing rather than a
//! guess.

use banc_platform::machine::{self, ResolveFact};
use banc_proto::machine::{ResolveEdition, ResolveInstall};

/// Digits of the build number at the end of Blackmagic's long version.
const BUILD_DIGITS: usize = 4;

/// The Resolve installed on this machine, if any.
pub fn installed() -> Option<ResolveInstall> {
    detect(&machine::resolve_facts())
}

/// Combines the facts: the first version found, Studio if any fact says so.
///
/// The edition stays unknown otherwise: Resolve Studio installs under the
/// plain name "DaVinci Resolve" on macOS and Windows, so a name without
/// "Studio" does not prove the free edition.
pub fn detect(facts: &[ResolveFact]) -> Option<ResolveInstall> {
    let mut version = None;
    let mut studio = false;
    for fact in facts {
        let (found, is_studio) = match fact {
            ResolveFact::InfoPlist(xml) => from_info_plist(xml),
            ResolveFact::InstalledProgram { name, version } if is_resolve_itself(name) => {
                (clean_version(version), name.contains("Studio"))
            }
            ResolveFact::InstalledProgram { .. } => (None, false),
            ResolveFact::InstallFile { name, text } => from_install_file(name, text),
        };
        studio |= is_studio;
        if version.is_none() {
            version = found;
        }
    }
    let edition = studio.then_some(ResolveEdition::Studio);
    version.map(|version| ResolveInstall { edition, version })
}

/// Resolve itself, not a program installed with it: "DaVinci Resolve Control
/// Panels" has its own version numbers.
fn is_resolve_itself(name: &str) -> bool {
    matches!(name.trim(), "DaVinci Resolve" | "DaVinci Resolve Studio")
}

/// Version and edition from an `Info.plist`.
fn from_info_plist(xml: &str) -> (Option<String>, bool) {
    let version = plist_string(xml, "CFBundleShortVersionString")
        .or_else(|| plist_string(xml, "CFBundleVersion"))
        .and_then(clean_version);
    let studio = ["CFBundleName", "CFBundleDisplayName", "CFBundleExecutable"]
        .iter()
        .filter_map(|key| plist_string(xml, key))
        .any(|name| name.contains("Studio"));
    (version, studio)
}

/// The `<string>` that follows `<key>name</key>`.
fn plist_string<'a>(xml: &'a str, key: &str) -> Option<&'a str> {
    let marker = format!("<key>{key}</key>");
    let after = &xml[xml.find(&marker)? + marker.len()..];
    let after = after.trim_start().strip_prefix("<string>")?;
    Some(after[..after.find("</string>")?].trim())
}

/// Version and edition from a text of the Linux installation: the first
/// "DaVinci Resolve [Studio] <version>" it contains.
fn from_install_file(name: &str, text: &str) -> (Option<String>, bool) {
    let mut studio = name.contains("Studio") || text.contains("DaVinci Resolve Studio");
    let mut rest = text;
    while let Some(at) = rest.find("DaVinci Resolve") {
        let mut after = &rest[at + "DaVinci Resolve".len()..];
        if let Some(stripped) = after.strip_prefix(" Studio") {
            studio = true;
            after = stripped;
        }
        if let Some(version) = after.strip_prefix(' ').and_then(leading_version) {
            return (Some(version), studio);
        }
        rest = after;
    }
    (None, studio)
}

/// `21.1.2b4 …` gives `21.1.2`.
fn leading_version(text: &str) -> Option<String> {
    let end = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(text.len());
    clean_version(&text[..end])
}

/// Digits separated by dots, at least `major.minor`, without trailing dot
/// and without the build number.
fn clean_version(text: &str) -> Option<String> {
    let text = text.trim().trim_end_matches('.');
    let parts: Vec<&str> = text.split('.').collect();
    let valid = parts.len() >= 2
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
    valid.then(|| without_build(&parts))
}

/// Blackmagic glues a 4-digit build to the patch number: `21.0.40005` is
/// 21.0.4 build 5, and `21.1.00014` is 21.1.0 build 14.
fn without_build(parts: &[&str]) -> String {
    match parts {
        [major, minor, patch_build] if patch_build.len() > BUILD_DIGITS => {
            let patch = &patch_build[..patch_build.len() - BUILD_DIGITS];
            format!("{major}.{minor}.{patch}")
        }
        _ => parts.join("."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plist(name: &str, version: &str) -> ResolveFact {
        ResolveFact::InfoPlist(format!(
            "<?xml version=\"1.0\"?>\n<plist version=\"1.0\"><dict>\n\
             \t<key>CFBundleExecutable</key>\n\t<string>Resolve</string>\n\
             \t<key>CFBundleName</key>\n\t<string>{name}</string>\n\
             \t<key>CFBundleShortVersionString</key>\n\t<string>{version}</string>\n\
             </dict></plist>"
        ))
    }

    fn studio(version: &str) -> Option<ResolveInstall> {
        Some(ResolveInstall {
            edition: Some(ResolveEdition::Studio),
            version: version.to_owned(),
        })
    }

    fn unknown_edition(version: &str) -> Option<ResolveInstall> {
        Some(ResolveInstall {
            edition: None,
            version: version.to_owned(),
        })
    }

    fn program(name: &str, version: &str) -> ResolveFact {
        ResolveFact::InstalledProgram {
            name: name.to_owned(),
            version: version.to_owned(),
        }
    }

    #[test]
    fn nothing_installed() {
        assert_eq!(detect(&[]), None);
    }

    #[test]
    fn mac_bundle() {
        assert_eq!(
            detect(&[plist("DaVinci Resolve Studio", "21.1")]),
            studio("21.1")
        );
        assert_eq!(
            detect(&[plist("DaVinci Resolve", "21.0.4")]),
            unknown_edition("21.0.4")
        );
        assert_eq!(detect(&[plist("DaVinci Resolve", "unknown")]), None);
    }

    #[test]
    fn windows_programs() {
        let facts = [program("DaVinci Resolve Studio", "21.1.0.12")];
        assert_eq!(detect(&facts), studio("21.1.0.12"));
    }

    /// What the registry of a Windows 11 PC with Resolve Studio 21.1 build 14
    /// holds, in the order most likely to mislead.
    #[test]
    fn windows_companion_programs_are_ignored() {
        let facts = [
            program("DaVinci Resolve Control Panels", "2.3.5.0"),
            program("DaVinci Resolve Renderer", "21.1.00014"),
            program("DaVinci Resolve", "21.1.00014"),
        ];
        assert_eq!(detect(&facts), unknown_edition("21.1.0"));
        assert_eq!(detect(&facts[..2]), None);
    }

    #[test]
    fn linux_files() {
        let facts = [
            ResolveFact::InstallFile {
                name: "/usr/share/applications/com.blackmagicdesign.resolve.desktop".to_owned(),
                text:
                    "[Desktop Entry]\nName=DaVinci Resolve Studio\nExec=/opt/resolve/bin/resolve\n"
                        .to_owned(),
            },
            ResolveFact::InstallFile {
                name: "/opt/resolve/docs/ReadMe.html".to_owned(),
                text: "<html><title>DaVinci Resolve 21.1.2 Read Me</title>".to_owned(),
            },
        ];
        assert_eq!(detect(&facts), studio("21.1.2"));
    }

    #[test]
    fn versions() {
        assert_eq!(clean_version("21.1"), Some("21.1".to_owned()));
        assert_eq!(clean_version(" 21.1.2. "), Some("21.1.2".to_owned()));
        assert_eq!(clean_version("21.1.00014"), Some("21.1.0".to_owned()));
        assert_eq!(clean_version("21.0.40005"), Some("21.0.4".to_owned()));
        assert_eq!(clean_version("21.0.120005"), Some("21.0.12".to_owned()));
        assert_eq!(clean_version("21.1.0014"), Some("21.1.0014".to_owned()));
        for bad in ["21", "", "21..1", "v21.1", "21.1b"] {
            assert_eq!(clean_version(bad), None, "{bad}");
        }
        assert_eq!(
            leading_version("21.1.2b4 Read Me"),
            Some("21.1.2".to_owned())
        );
        let (version, studio) = from_install_file(
            "x",
            "About DaVinci Resolve. DaVinci Resolve Studio 19.1 notes",
        );
        assert_eq!(version.as_deref(), Some("19.1"));
        assert!(studio);
    }

    #[test]
    fn plist_reading() {
        let xml = "<key>A</key>\n  <string> one </string><key>B</key><integer>2</integer>";
        assert_eq!(plist_string(xml, "A"), Some("one"));
        assert_eq!(plist_string(xml, "B"), None);
        assert_eq!(plist_string(xml, "C"), None);
    }
}
