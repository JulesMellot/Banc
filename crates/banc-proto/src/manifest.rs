//! The manifest: what every workstation of the team must have.
//!
//! The admin's application signs it with the admin key, which never leaves
//! the admin's workstation. The server stores and relays it without being
//! able to change it: an agent reads nothing of a manifest before checking
//! its signature against the key recorded at enrollment.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::keys::{self, PublicKey, SecretKey, Sig};
use crate::machine::Os;
use crate::text;

/// Domain of the manifest signature.
const MANIFEST_DOMAIN: &str = "banc/manifest/1";
/// Longest package identifier or version text.
const MAX_NAME: usize = 64;
/// Architectures a package can be built for.
const ARCHES: [&str; 2] = ["x86_64", "aarch64"];

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ManifestError {
    #[error("the manifest signature is not valid")]
    BadSignature,
    #[error("malformed manifest: {0}")]
    Malformed(String),
    #[error("manifest version {found} is older than version {current}, already applied")]
    Older { found: u64, current: u64 },
}

fn malformed<T>(message: impl Into<String>) -> Result<T, ManifestError> {
    Err(ManifestError::Malformed(message.into()))
}

/// How a package gets onto a workstation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageKind {
    Copy,
    Pkg,
    Msi,
    Exe,
    Deb,
    Rpm,
    Run,
    Font,
    /// Installed by a person, following the admin's instructions.
    Manual,
}

impl PackageKind {
    /// Whether this kind of installer exists on `os`.
    pub fn runs_on(self, os: Os) -> bool {
        match self {
            Self::Copy | Self::Font | Self::Manual => true,
            Self::Pkg => os == Os::Macos,
            Self::Msi | Self::Exe => os == Os::Windows,
            Self::Deb | Self::Rpm | Self::Run => os == Os::Linux,
        }
    }
}

/// The file of a build, identified by its content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageFile {
    /// BLAKE3 of the content, in hexadecimal.
    pub blake3: String,
    pub size: u64,
}

/// One variant of a package, for an OS and possibly an architecture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageBuild {
    pub os: Os,
    /// `x86_64` or `aarch64`. `None`: any architecture of this OS.
    #[serde(default)]
    pub arch: Option<String>,
    pub kind: PackageKind,
    /// `None` only for a manual package.
    #[serde(default)]
    pub file: Option<PackageFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Package {
    pub id: String,
    pub version: String,
    /// A required package is installed on every workstation.
    pub required: bool,
    pub builds: Vec<PackageBuild>,
}

impl Package {
    /// The build for this machine: its exact architecture first, then a
    /// build for any architecture.
    pub fn build_for(&self, os: Os, arch: &str) -> Option<&PackageBuild> {
        let of_os = || self.builds.iter().filter(|build| build.os == os);
        of_os()
            .find(|build| build.arch.as_deref() == Some(arch))
            .or_else(|| of_os().find(|build| build.arch.is_none()))
    }

    fn check(&self) -> Result<(), ManifestError> {
        let id = &self.id;
        if !is_package_id(id) {
            return malformed(format!("package id `{id}`"));
        }
        if self.version.is_empty() || self.version.len() > MAX_NAME {
            return malformed(format!("version of package `{id}`"));
        }
        if self.builds.is_empty() {
            return malformed(format!("package `{id}` has no build"));
        }
        let mut seen = HashSet::new();
        for build in &self.builds {
            if !seen.insert((build.os, build.arch.as_deref())) {
                return malformed(format!("package `{id}` has the same build twice"));
            }
            check_build(id, build)?;
        }
        Ok(())
    }
}

fn check_build(id: &str, build: &PackageBuild) -> Result<(), ManifestError> {
    if !build.kind.runs_on(build.os) {
        return malformed(format!(
            "package `{id}`: {:?} does not exist on {}",
            build.kind, build.os
        ));
    }
    if let Some(arch) = &build.arch {
        if !ARCHES.contains(&arch.as_str()) {
            return malformed(format!("package `{id}`: architecture `{arch}`"));
        }
    }
    match &build.file {
        Some(file) if text::hex_exact::<32>(&file.blake3).is_none() => {
            malformed(format!("package `{id}`: file hash"))
        }
        None if build.kind != PackageKind::Manual => {
            malformed(format!("package `{id}`: file missing"))
        }
        _ => Ok(()),
    }
}

/// Lowercase letters, digits and dashes.
fn is_package_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_NAME
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The Resolve version every workstation must run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequiredResolve {
    /// `21.1` accepts every `21.1.x`, `21.1.2` only that one.
    pub version: String,
}

impl RequiredResolve {
    /// Whether the installed version is the required one. A number missing
    /// from the installed version counts as 0.
    pub fn accepts(&self, installed: &str) -> bool {
        let mut installed = installed.split('.');
        self.version
            .split('.')
            .all(|part| same_number(part, installed.next().unwrap_or("0")))
    }

    fn check(&self) -> Result<(), ManifestError> {
        let parts: Vec<_> = self.version.split('.').collect();
        let valid = (2..=3).contains(&parts.len())
            && parts
                .iter()
                .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
        if valid {
            Ok(())
        } else {
            malformed(format!("Resolve version `{}`", self.version))
        }
    }
}

fn same_number(a: &str, b: &str) -> bool {
    a.trim_start_matches('0') == b.trim_start_matches('0')
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Grows with every publication.
    pub version: u64,
    /// `None`: the team has not chosen a version yet.
    #[serde(default)]
    pub resolve: Option<RequiredResolve>,
    pub packages: Vec<Package>,
}

impl Manifest {
    fn check(&self) -> Result<(), ManifestError> {
        if self.version == 0 {
            return malformed("version 0");
        }
        if let Some(resolve) = &self.resolve {
            resolve.check()?;
        }
        let mut ids = HashSet::new();
        for package in &self.packages {
            package.check()?;
            if !ids.insert(package.id.as_str()) {
                return malformed(format!("package `{}` listed twice", package.id));
            }
        }
        Ok(())
    }
}

/// A manifest as it travels: the exact bytes that were signed, and the
/// signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedManifest {
    pub payload: String,
    pub signature: Sig,
}

impl SignedManifest {
    pub fn sign(admin_key: &SecretKey, manifest: &Manifest) -> Result<Self, ManifestError> {
        manifest.check()?;
        let payload = serde_json::to_string(manifest)
            .map_err(|error| ManifestError::Malformed(error.to_string()))?;
        let signature = keys::sign(admin_key, MANIFEST_DOMAIN, payload.as_bytes());
        Ok(Self { payload, signature })
    }

    /// The manifest, once its signature has been checked against the team's
    /// admin key. `current` is the version already applied here: an older
    /// manifest is refused, so that a server cannot bring one back.
    pub fn open(
        &self,
        admin_key: &PublicKey,
        current: Option<u64>,
    ) -> Result<Manifest, ManifestError> {
        if !keys::verify(
            admin_key,
            MANIFEST_DOMAIN,
            self.payload.as_bytes(),
            &self.signature,
        ) {
            return Err(ManifestError::BadSignature);
        }
        let manifest: Manifest = serde_json::from_str(&self.payload)
            .map_err(|error| ManifestError::Malformed(error.to_string()))?;
        manifest.check()?;
        match current {
            Some(current) if manifest.version < current => Err(ManifestError::Older {
                found: manifest.version,
                current,
            }),
            _ => Ok(manifest),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(seed: u8) -> SecretKey {
        SecretKey::from_bytes(&[seed; 32])
    }

    fn file() -> Option<PackageFile> {
        Some(PackageFile {
            blake3: "ab".repeat(32),
            size: 10,
        })
    }

    fn build(os: Os, arch: Option<&str>, kind: PackageKind) -> PackageBuild {
        PackageBuild {
            os,
            arch: arch.map(str::to_owned),
            kind,
            file: file(),
        }
    }

    fn package(id: &str, builds: Vec<PackageBuild>) -> Package {
        Package {
            id: id.to_owned(),
            version: "1.0".to_owned(),
            required: true,
            builds,
        }
    }

    fn manifest(packages: Vec<Package>) -> Manifest {
        Manifest {
            version: 12,
            resolve: Some(RequiredResolve {
                version: "21.1".to_owned(),
            }),
            packages,
        }
    }

    fn font() -> Package {
        package(
            "font-inter",
            vec![build(Os::Macos, None, PackageKind::Font)],
        )
    }

    fn is_malformed(manifest: &Manifest) -> bool {
        matches!(
            SignedManifest::sign(&key(1), manifest),
            Err(ManifestError::Malformed(_))
        )
    }

    #[test]
    fn signed_round_trip() {
        let manifest = manifest(vec![font()]);
        let signed = SignedManifest::sign(&key(1), &manifest).unwrap();
        let json = serde_json::to_string(&signed).unwrap();
        let read: SignedManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(read.open(&key(1).public(), None).unwrap(), manifest);
    }

    #[test]
    fn a_manifest_changed_on_the_server_is_refused() {
        let mut signed = SignedManifest::sign(&key(1), &manifest(vec![font()])).unwrap();
        signed.payload = signed.payload.replace("font-inter", "font-evil");
        assert_eq!(
            signed.open(&key(1).public(), None),
            Err(ManifestError::BadSignature)
        );
    }

    #[test]
    fn another_key_is_refused() {
        let signed = SignedManifest::sign(&key(1), &manifest(vec![font()])).unwrap();
        assert_eq!(
            signed.open(&key(2).public(), None),
            Err(ManifestError::BadSignature)
        );
    }

    #[test]
    fn a_signature_for_another_purpose_is_refused() {
        let payload = serde_json::to_string(&manifest(vec![font()])).unwrap();
        let signed = SignedManifest {
            signature: keys::sign(&key(1), "banc/update/1", payload.as_bytes()),
            payload,
        };
        assert_eq!(
            signed.open(&key(1).public(), None),
            Err(ManifestError::BadSignature)
        );
    }

    #[test]
    fn an_older_manifest_is_refused() {
        let signed = SignedManifest::sign(&key(1), &manifest(vec![font()])).unwrap();
        let public = key(1).public();
        assert!(signed.open(&public, Some(11)).is_ok());
        assert!(signed.open(&public, Some(12)).is_ok(), "delivered again");
        assert_eq!(
            signed.open(&public, Some(13)),
            Err(ManifestError::Older {
                found: 12,
                current: 13
            })
        );
    }

    #[test]
    fn a_well_signed_but_malformed_manifest_is_refused() {
        let mut bad = manifest(vec![font()]);
        bad.packages[0].id = "../evil".to_owned();
        let payload = serde_json::to_string(&bad).unwrap();
        let signed = SignedManifest {
            signature: keys::sign(&key(1), MANIFEST_DOMAIN, payload.as_bytes()),
            payload,
        };
        assert!(matches!(
            signed.open(&key(1).public(), None),
            Err(ManifestError::Malformed(_))
        ));
    }

    #[test]
    fn malformed_manifests() {
        let mut zero = manifest(vec![]);
        zero.version = 0;
        assert!(is_malformed(&zero));

        let mut resolve = manifest(vec![]);
        resolve.resolve = Some(RequiredResolve {
            version: "latest".to_owned(),
        });
        assert!(is_malformed(&resolve));

        assert!(is_malformed(&manifest(vec![font(), font()])), "same id");
        assert!(is_malformed(&manifest(vec![package("no-build", vec![])])));

        let mut version = font();
        version.version = String::new();
        assert!(is_malformed(&manifest(vec![version])));
    }

    #[test]
    fn malformed_builds() {
        let bad_builds = [
            vec![build(Os::Windows, None, PackageKind::Pkg)],
            vec![build(Os::Macos, None, PackageKind::Msi)],
            vec![build(Os::Macos, None, PackageKind::Deb)],
            vec![build(Os::Macos, Some("sparc"), PackageKind::Copy)],
            vec![
                build(Os::Macos, None, PackageKind::Copy),
                build(Os::Macos, None, PackageKind::Font),
            ],
            vec![PackageBuild {
                file: None,
                ..build(Os::Macos, None, PackageKind::Copy)
            }],
            vec![PackageBuild {
                file: Some(PackageFile {
                    blake3: "short".to_owned(),
                    size: 1,
                }),
                ..build(Os::Macos, None, PackageKind::Copy)
            }],
        ];
        for builds in bad_builds {
            let manifest = manifest(vec![package("plugin", builds.clone())]);
            assert!(is_malformed(&manifest), "{builds:?}");
        }
    }

    #[test]
    fn a_manual_package_needs_no_file() {
        let manual = PackageBuild {
            file: None,
            ..build(Os::Windows, None, PackageKind::Manual)
        };
        let manifest = manifest(vec![package("licence", vec![manual])]);
        assert!(SignedManifest::sign(&key(1), &manifest).is_ok());
    }

    #[test]
    fn package_ids() {
        assert!(is_package_id("boris-continuum-2025"));
        for bad in ["", "Boris", "a b", "a/b", "a.b", &"a".repeat(65)] {
            assert!(!is_package_id(bad), "{bad}");
        }
    }

    #[test]
    fn the_exact_architecture_comes_first() {
        let plugin = package(
            "plugin",
            vec![
                build(Os::Macos, None, PackageKind::Copy),
                build(Os::Macos, Some("aarch64"), PackageKind::Pkg),
            ],
        );
        let kind = |os, arch| plugin.build_for(os, arch).map(|build| build.kind);
        assert_eq!(kind(Os::Macos, "aarch64"), Some(PackageKind::Pkg));
        assert_eq!(kind(Os::Macos, "x86_64"), Some(PackageKind::Copy));
        assert_eq!(kind(Os::Windows, "x86_64"), None);
    }

    #[test]
    fn resolve_versions() {
        let required = |version: &str| RequiredResolve {
            version: version.to_owned(),
        };
        assert!(required("21.1").accepts("21.1"));
        assert!(required("21.1").accepts("21.1.0"));
        assert!(required("21.1").accepts("21.1.3"));
        assert!(!required("21.1").accepts("21.0.4"));
        assert!(!required("21.1").accepts("21.10.0"));
        assert!(!required("21.1").accepts("22.1.0"));
        assert!(required("21.1.2").accepts("21.1.2"));
        assert!(!required("21.1.2").accepts("21.1.3"));
        assert!(!required("21.1.2").accepts("21.1"));
        assert!(required("21.1.0").accepts("21.1"));
    }
}
