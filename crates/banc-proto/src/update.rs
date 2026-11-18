//! Signed releases of the workstation programs.
//!
//! The release workflow publishes one `update.json`, signed with the release
//! key. That key is kept off the VPS: neither the server nor anyone who
//! controls the download site can make a workstation run a program.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::keys::{self, PublicKey, SecretKey, Sig};
use crate::text;

/// Domain of the release signature.
const UPDATE_DOMAIN: &str = "banc/update/1";

/// The public release key built into the programs, in hexadecimal.
/// `BANC_RELEASE_PUBLIC_KEY` replaces it at build time, for the update tests.
const RELEASE_KEY: &str = match option_env!("BANC_RELEASE_PUBLIC_KEY") {
    Some(key) => key,
    None => include_str!("../release-key.pub"),
};

/// The public key that releases must be signed with.
pub fn release_key() -> Result<PublicKey, UpdateError> {
    parse_public_key(RELEASE_KEY)
}

/// A public key written in hexadecimal.
pub fn parse_public_key(text: &str) -> Result<PublicKey, UpdateError> {
    let bytes = text::hex_exact::<32>(text.trim()).ok_or(UpdateError::BadKey)?;
    PublicKey::from_bytes(&bytes).map_err(|_| UpdateError::BadKey)
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UpdateError {
    #[error("malformed release key")]
    BadKey,
    #[error("the release signature is not valid")]
    BadSignature,
    #[error("malformed release: {0}")]
    Malformed(String),
    #[error("malformed version `{0}`")]
    BadVersion(String),
}

/// `major.minor.patch`, compared number by number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Version {
    /// Development builds are `0.0.0`: they never update themselves.
    pub fn is_development(&self) -> bool {
        self.major == 0 && self.minor == 0 && self.patch == 0
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl FromStr for Version {
    type Err = UpdateError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let bad = || UpdateError::BadVersion(text.to_owned());
        let number = |part: &str| {
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                return Err(bad());
            }
            part.parse().map_err(|_| bad())
        };
        let mut parts = text.split('.');
        let (Some(major), Some(minor), Some(patch), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(bad());
        };
        Ok(Self {
            major: number(major)?,
            minor: number(minor)?,
            patch: number(patch)?,
        })
    }
}

impl Serialize for Version {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Version {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

/// One file of an installed application.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseFile {
    /// Inside the application's folder, with forward slashes.
    pub path: String,
    /// Name of the file on the download site.
    pub asset: String,
    pub size: u64,
    /// BLAKE3 of the content, in hexadecimal.
    pub blake3: String,
    pub executable: bool,
}

/// What a version contains, per platform (`macos-aarch64`, `windows-x86_64`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    pub version: Version,
    pub platforms: BTreeMap<String, Vec<ReleaseFile>>,
}

impl Release {
    fn check(&self) -> Result<(), UpdateError> {
        for file in self.platforms.values().flatten() {
            let malformed = |what: &str| {
                Err(UpdateError::Malformed(format!(
                    "{what} of file `{}`",
                    file.path
                )))
            };
            if !is_safe_path(&file.path) {
                return malformed("path");
            }
            if !is_safe_asset(&file.asset) {
                return malformed("asset");
            }
            if text::hex_exact::<32>(&file.blake3).is_none() {
                return malformed("hash");
            }
        }
        Ok(())
    }

    /// The files of a platform, `None` if this version does not have it.
    pub fn files(&self, platform: &str) -> Option<&[ReleaseFile]> {
        self.platforms.get(platform).map(Vec::as_slice)
    }
}

/// Relative, forward slashes, and never leaves the application's folder.
fn is_safe_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains('\\')
        && !path.contains(':')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn is_safe_asset(asset: &str) -> bool {
    !asset.is_empty()
        && !asset.starts_with('.')
        && asset
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

/// The published file: the exact bytes that were signed, and the signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedRelease {
    pub payload: String,
    pub signature: Sig,
}

impl SignedRelease {
    pub fn sign(secret: &SecretKey, release: &Release) -> Result<Self, UpdateError> {
        release.check()?;
        let payload =
            serde_json::to_string(release).map_err(|e| UpdateError::Malformed(e.to_string()))?;
        let signature = keys::sign(secret, UPDATE_DOMAIN, payload.as_bytes());
        Ok(Self { payload, signature })
    }

    /// The release, once its signature has been checked. Nothing in the
    /// payload is read before that.
    pub fn open(&self, key: &PublicKey) -> Result<Release, UpdateError> {
        if !keys::verify(key, UPDATE_DOMAIN, self.payload.as_bytes(), &self.signature) {
            return Err(UpdateError::BadSignature);
        }
        let release: Release = serde_json::from_str(&self.payload)
            .map_err(|e| UpdateError::Malformed(e.to_string()))?;
        release.check()?;
        Ok(release)
    }
}

/// Whether a workstation on `current` should install `release`. Only a newer
/// version is accepted, so an old signed release cannot be replayed to bring
/// back a known flaw. A version that failed here is not tried again.
pub fn should_install(current: Version, release: Version, failed: Option<Version>) -> bool {
    !current.is_development() && release > current && failed != Some(release)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(text: &str) -> Version {
        text.parse().unwrap()
    }

    fn file(path: &str) -> ReleaseFile {
        ReleaseFile {
            path: path.to_owned(),
            asset: "macos-aarch64.banc-agent".to_owned(),
            size: 3,
            blake3: "ab".repeat(32),
            executable: true,
        }
    }

    fn release(path: &str) -> Release {
        Release {
            version: version("1.2.3"),
            platforms: BTreeMap::from([("macos-aarch64".to_owned(), vec![file(path)])]),
        }
    }

    fn secret(seed: u8) -> SecretKey {
        SecretKey::from_bytes(&[seed; 32])
    }

    #[test]
    fn built_in_key_is_valid() {
        release_key().unwrap();
    }

    #[test]
    fn versions_are_compared_as_numbers() {
        assert!(version("1.10.0") > version("1.9.9"));
        assert!(version("2.0.0") > version("1.99.99"));
        assert_eq!(version("1.2.3").to_string(), "1.2.3");
        for bad in [
            "", "1", "1.2", "1.2.3.4", "1.2.x", "v1.2.3", "1..3", "+1.2.3",
        ] {
            assert!(bad.parse::<Version>().is_err(), "{bad}");
        }
    }

    #[test]
    fn only_a_newer_version_is_installed() {
        let current = version("1.2.3");
        assert!(should_install(current, version("1.2.4"), None));
        assert!(!should_install(current, current, None));
        assert!(!should_install(current, version("1.2.2"), None));
    }

    #[test]
    fn a_failed_version_is_skipped_until_a_newer_one() {
        let current = version("1.2.3");
        let failed = Some(version("1.2.4"));
        assert!(!should_install(current, version("1.2.4"), failed));
        assert!(should_install(current, version("1.2.5"), failed));
    }

    #[test]
    fn development_builds_never_update() {
        assert!(!should_install(version("0.0.0"), version("9.0.0"), None));
    }

    #[test]
    fn signed_release_round_trip() {
        let signed =
            SignedRelease::sign(&secret(1), &release("Contents/MacOS/banc-agent")).unwrap();
        let json = serde_json::to_string(&signed).unwrap();
        let read: SignedRelease = serde_json::from_str(&json).unwrap();
        let opened = read.open(&secret(1).public()).unwrap();
        assert_eq!(opened, release("Contents/MacOS/banc-agent"));
        assert_eq!(opened.files("windows-x86_64"), None);
    }

    #[test]
    fn another_key_is_refused() {
        let signed = SignedRelease::sign(&secret(1), &release("banc-agent")).unwrap();
        assert_eq!(
            signed.open(&secret(2).public()),
            Err(UpdateError::BadSignature)
        );
    }

    #[test]
    fn a_changed_payload_is_refused() {
        let mut signed = SignedRelease::sign(&secret(1), &release("banc-agent")).unwrap();
        signed.payload = signed.payload.replace("1.2.3", "9.9.9");
        assert_eq!(
            signed.open(&secret(1).public()),
            Err(UpdateError::BadSignature)
        );
    }

    #[test]
    fn a_signature_for_another_purpose_is_refused() {
        let payload = serde_json::to_string(&release("banc-agent")).unwrap();
        let signed = SignedRelease {
            signature: keys::sign(&secret(1), "banc/manifest/1", payload.as_bytes()),
            payload,
        };
        assert_eq!(
            signed.open(&secret(1).public()),
            Err(UpdateError::BadSignature)
        );
    }

    #[test]
    fn paths_stay_inside_the_application() {
        for good in ["banc-agent.exe", "Contents/MacOS/banc-agent"] {
            assert!(is_safe_path(good), "{good}");
        }
        for bad in [
            "",
            "/etc/passwd",
            "../banc-agent",
            "Contents/../../x",
            "a//b",
            "./a",
            r"..\x",
            r"C:\Windows\x",
            "C:/Windows/x",
        ] {
            assert!(!is_safe_path(bad), "{bad}");
            assert!(
                matches!(
                    SignedRelease::sign(&secret(1), &release(bad)),
                    Err(UpdateError::Malformed(_))
                ),
                "{bad}"
            );
        }
    }

    #[test]
    fn asset_names_are_plain() {
        assert!(is_safe_asset("windows-x86_64.banc-agent.exe"));
        for bad in ["", "../x", "a/b", ".hidden", "a b", "a?b=c"] {
            assert!(!is_safe_asset(bad), "{bad}");
        }
    }
}
