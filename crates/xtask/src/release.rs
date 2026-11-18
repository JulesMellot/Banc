//! Automatic releases: the next version number, the files of a version, and
//! the signed description that workstations read.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use banc_proto::keys::SecretKey;
use banc_proto::update::{Release, ReleaseFile, SignedRelease, Version};

use crate::cmd::{output, say, tool};

/// Environment variable that holds the private release key, in hexadecimal.
pub const KEY_VARIABLE: &str = "BANC_RELEASE_KEY";
/// The public key built into the workstation programs.
pub const PUBLIC_KEY_FILE: &str = "crates/banc-proto/release-key.pub";
/// First version published.
const FIRST: Version = Version {
    major: 0,
    minor: 1,
    patch: 0,
};

/// What a commit changes for the people using Banc.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Bump {
    Patch,
    Minor,
    Major,
}

/// Reads `type(scope)!: description`. Only what changes the product counts:
/// documentation, CI and tests do not make a version.
fn bump_of(subject: &str) -> Option<Bump> {
    let (head, _) = subject.split_once(':')?;
    let breaking = head.ends_with('!');
    let kind = head.trim_end_matches('!');
    let kind = kind.split('(').next().unwrap_or(kind);
    match kind {
        "feat" | "fix" | "perf" | "refactor" if breaking => Some(Bump::Major),
        "feat" => Some(Bump::Minor),
        "fix" | "perf" | "refactor" => Some(Bump::Patch),
        _ => None,
    }
}

fn apply_bump(version: Version, bump: Bump) -> Version {
    // Before 1.0, a breaking change is a minor version.
    let bump = if version.major == 0 && bump == Bump::Major {
        Bump::Minor
    } else {
        bump
    };
    match bump {
        Bump::Major => Version {
            major: version.major + 1,
            minor: 0,
            patch: 0,
        },
        Bump::Minor => Version {
            minor: version.minor + 1,
            patch: 0,
            ..version
        },
        Bump::Patch => Version {
            patch: version.patch + 1,
            ..version
        },
    }
}

/// The highest `vX.Y.Z` tag.
fn latest_tag(tags: &str) -> Option<Version> {
    tags.lines()
        .filter_map(|tag| tag.trim().strip_prefix('v')?.parse().ok())
        .max()
}

/// The version to publish, `None` if nothing in `subjects` needs one.
fn next_version(latest: Option<Version>, subjects: &str) -> Option<Version> {
    let bump = subjects.lines().filter_map(bump_of).max()?;
    Some(match latest {
        Some(version) => apply_bump(version, bump),
        None => FIRST,
    })
}

/// Prints the version to publish, nothing if none is needed. In a workflow,
/// `GITHUB_OUTPUT` receives `version=<version>`.
pub fn print_next_version(root: &Path) -> anyhow::Result<()> {
    let tags = output(tool("git").args(["tag", "--list", "v*"]).current_dir(root))?;
    let latest = latest_tag(&tags);
    let range = latest.map_or_else(|| "HEAD".to_owned(), |version| format!("v{version}..HEAD"));
    let subjects = output(
        tool("git")
            .args(["log", "--format=%s", &range])
            .current_dir(root),
    )?;
    let next = next_version(latest, &subjects);
    let text = next.map(|version| version.to_string()).unwrap_or_default();
    say(&format!("next version: {}", or_none(&text)));
    if let Some(path) = std::env::var_os("GITHUB_OUTPUT") {
        let mut file = std::fs::OpenOptions::new().append(true).open(path)?;
        writeln!(file, "version={text}")?;
    }
    writeln!(std::io::stdout(), "{text}")?;
    Ok(())
}

fn or_none(text: &str) -> &str {
    if text.is_empty() {
        "none"
    } else {
        text
    }
}

/// `macos-aarch64`, `windows-x86_64`.
pub fn platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

/// Whether a file of an installed application is a program.
fn is_executable(path: &str) -> bool {
    path.starts_with("Contents/MacOS/") || path.ends_with(".exe") || !path.contains('/')
}

/// Every file under `dir`, with its path relative to `dir`.
fn walk(dir: &Path, prefix: &str, found: &mut Vec<(String, PathBuf)>) -> anyhow::Result<()> {
    let mut entries = std::fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = format!("{prefix}{name}");
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            bail!("{path} is a symbolic link: an update only carries files");
        }
        if kind.is_dir() {
            walk(&entry.path(), &format!("{path}/"), found)?;
        } else {
            found.push((path, entry.path()));
        }
    }
    Ok(())
}

/// Copies the files of this platform into `out/assets`, named after their
/// content, and describes them in `out/files-<platform>.json`.
pub fn write_payload(
    files: &[(String, PathBuf)],
    platform: &str,
    out: &Path,
) -> anyhow::Result<Vec<ReleaseFile>> {
    let assets = out.join("assets");
    std::fs::create_dir_all(&assets)?;
    let mut described = Vec::new();
    for (path, source) in files {
        let content =
            std::fs::read(source).with_context(|| format!("reading {}", source.display()))?;
        let hash = blake3::hash(&content).to_hex().to_string();
        let asset = format!("{platform}.{}", &hash[..32]);
        std::fs::write(assets.join(&asset), &content)?;
        described.push(ReleaseFile {
            path: path.clone(),
            asset,
            size: content.len() as u64,
            blake3: hash,
            executable: is_executable(path),
        });
    }
    std::fs::write(
        out.join(format!("files-{platform}.json")),
        serde_json::to_vec_pretty(&described)?,
    )?;
    Ok(described)
}

/// The installed files of the application just bundled on this machine.
pub fn bundled_files(root: &Path) -> anyhow::Result<Vec<(String, PathBuf)>> {
    let release = root.join("ui/src-tauri/target/release");
    if cfg!(target_os = "macos") {
        let mut files = Vec::new();
        walk(&release.join("bundle/macos/Banc.app"), "", &mut files)?;
        return Ok(files);
    }
    let suffix = std::env::consts::EXE_SUFFIX;
    Ok(vec![
        (
            format!("banc-app{suffix}"),
            release.join(format!("banc-app{suffix}")),
        ),
        (
            format!("banc-agent{suffix}"),
            root.join(format!("target/release/banc-agent{suffix}")),
        ),
        (
            format!("banc-session{suffix}"),
            root.join(format!("target/release/banc-session{suffix}")),
        ),
    ])
}

pub fn parse_secret(text: &str) -> anyhow::Result<SecretKey> {
    let text = text.trim();
    if text.len() != 64 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("the release key must be 64 hexadecimal characters");
    }
    let mut bytes = [0; 32];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)?;
    }
    Ok(SecretKey::from_bytes(&bytes))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn public_hex(secret: &SecretKey) -> String {
    hex(secret.public().as_bytes())
}

/// Creates a release key: the private half in `out`, readable by its owner,
/// the public half in the repository.
pub fn new_key(root: &Path, out: &Path) -> anyhow::Result<()> {
    if out.exists() {
        bail!("{} already exists", out.display());
    }
    let secret = SecretKey::generate();
    banc_platform::fs::write_private(out, hex(&secret.to_bytes()).as_bytes())?;
    std::fs::write(
        root.join(PUBLIC_KEY_FILE),
        format!("{}\n", public_hex(&secret)),
    )?;
    say(&format!("private key in {}", out.display()));
    Ok(())
}

/// Assembles the signed description from the payloads found under `dir`.
pub fn sign_manifest(
    dir: &Path,
    version: Version,
    secret: &SecretKey,
    out: &Path,
) -> anyhow::Result<()> {
    let mut found = Vec::new();
    walk(dir, "", &mut found)?;
    let mut platforms = BTreeMap::new();
    for (path, source) in found {
        let name = path.rsplit('/').next().unwrap_or(&path);
        let Some(platform) = name
            .strip_prefix("files-")
            .and_then(|name| name.strip_suffix(".json"))
        else {
            continue;
        };
        let files: Vec<ReleaseFile> = serde_json::from_slice(&std::fs::read(&source)?)
            .with_context(|| format!("reading {}", source.display()))?;
        platforms.insert(platform.to_owned(), files);
    }
    if platforms.is_empty() {
        bail!("no platform found in {}", dir.display());
    }
    let release = Release { version, platforms };
    let signed = SignedRelease::sign(secret, &release)?;
    std::fs::write(out, serde_json::to_vec(&signed)?)?;
    say(&format!(
        "{} signed for {} platform(s)",
        out.display(),
        release.platforms.len()
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(text: &str) -> Version {
        text.parse().unwrap()
    }

    #[test]
    fn what_makes_a_version() {
        assert_eq!(bump_of("fix(resolve): read the version"), Some(Bump::Patch));
        assert_eq!(bump_of("perf: go faster"), Some(Bump::Patch));
        assert_eq!(bump_of("refactor(agent): split it"), Some(Bump::Patch));
        assert_eq!(bump_of("feat(agent): update itself"), Some(Bump::Minor));
        assert_eq!(
            bump_of("feat(proto)!: change the format"),
            Some(Bump::Major)
        );
        for none in [
            "docs: explain",
            "ci: faster",
            "test(agent): cover",
            "build: bump",
            "chore: tidy",
            "Merge pull request #7 from x/y",
        ] {
            assert_eq!(bump_of(none), None, "{none}");
        }
    }

    #[test]
    fn the_biggest_change_decides() {
        let latest = Some(version("1.2.3"));
        assert_eq!(
            next_version(latest, "fix: a\nfeat: b\ndocs: c\n"),
            Some(version("1.3.0"))
        );
        assert_eq!(next_version(latest, "fix: a\n"), Some(version("1.2.4")));
        assert_eq!(
            next_version(latest, "feat!: a\nfix: b\n"),
            Some(version("2.0.0"))
        );
    }

    #[test]
    fn nothing_to_publish() {
        assert_eq!(
            next_version(Some(version("1.2.3")), "docs: a\nci: b\n"),
            None
        );
        assert_eq!(next_version(Some(version("1.2.3")), ""), None);
        assert_eq!(next_version(None, "docs: a\n"), None);
    }

    #[test]
    fn first_version() {
        assert_eq!(next_version(None, "fix: a\n"), Some(version("0.1.0")));
    }

    #[test]
    fn breaking_before_one_is_a_minor_version() {
        assert_eq!(
            next_version(Some(version("0.4.2")), "feat!: a\n"),
            Some(version("0.5.0"))
        );
    }

    #[test]
    fn latest_tag_is_compared_as_numbers() {
        let tags = "v0.9.0\nv0.10.0\nnot-a-version\nv1.2\n";
        assert_eq!(latest_tag(tags), Some(version("0.10.0")));
        assert_eq!(latest_tag(""), None);
    }

    #[test]
    fn programs_are_recognized() {
        for program in ["Contents/MacOS/banc-agent", "banc-agent.exe", "banc-agent"] {
            assert!(is_executable(program), "{program}");
        }
        for data in ["Contents/Info.plist", "Contents/Resources/icon.icns"] {
            assert!(!is_executable(data), "{data}");
        }
    }

    #[test]
    fn payload_then_signed_description() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("banc-agent");
        std::fs::write(&source, b"agent").unwrap();
        let out = tmp.path().join("payload-linux");
        let files = vec![("banc-agent".to_owned(), source)];
        let described = write_payload(&files, "linux-x86_64", &out).unwrap();
        assert_eq!(described[0].size, 5);
        assert!(out.join("assets").join(&described[0].asset).exists());

        let secret = SecretKey::from_bytes(&[3; 32]);
        let manifest = tmp.path().join("update.json");
        sign_manifest(tmp.path(), version("1.2.3"), &secret, &manifest).unwrap();
        let signed: SignedRelease =
            serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
        let release = signed.open(&secret.public()).unwrap();
        assert_eq!(release.version, version("1.2.3"));
        assert_eq!(release.files("linux-x86_64").unwrap(), &described[..]);
    }

    #[test]
    fn no_payload_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let secret = SecretKey::from_bytes(&[3; 32]);
        let out = tmp.path().join("update.json");
        assert!(sign_manifest(tmp.path(), version("1.2.3"), &secret, &out).is_err());
    }

    #[test]
    fn secret_key_text() {
        let secret = SecretKey::from_bytes(&[7; 32]);
        let parsed = parse_secret(&format!("{}\n", hex(&secret.to_bytes()))).unwrap();
        assert_eq!(parsed.public(), secret.public());
        assert!(parse_secret("abcd").is_err());
        assert!(parse_secret(&"z".repeat(64)).is_err());
    }

    #[test]
    fn files_are_listed_with_their_relative_path() {
        let tmp = tempfile::tempdir().unwrap();
        let mut found = Vec::new();
        std::fs::create_dir(tmp.path().join("Contents")).unwrap();
        std::fs::write(tmp.path().join("Contents/Info.plist"), b"x").unwrap();
        walk(tmp.path(), "", &mut found).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, "Contents/Info.plist");
    }
}
