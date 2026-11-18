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
/// Domain of the request that reads the draft with the admin key.
const DRAFT_DOMAIN: &str = "banc/manifest-draft/1";
/// How far a draft request's clock may be from the server's, in seconds.
const DRAFT_REQUEST_SKEW: u64 = 300;
/// Where the application reads the draft, with the admin key.
pub const DRAFT_PATH: &str = "/api/v1/manifest/draft";
/// Where the application publishes a signed manifest.
pub const PUBLISH_PATH: &str = "/api/v1/manifest";
/// Longest package identifier or version text.
const MAX_NAME: usize = 64;
/// Architectures a package can be built for.
const ARCHES: [&str; 2] = ["x86_64", "aarch64"];
/// Most packages in a manifest: a workstation's report must fit in one
/// control frame.
pub const MAX_PACKAGES: usize = 100;
/// Longest file name of a build.
const MAX_FILE_NAME: usize = 128;
/// Longest instructions of a manual package, in bytes.
pub const MAX_INSTRUCTIONS: usize = 300;
/// Most arguments of an installer, and the longest one.
const MAX_ARGS: usize = 16;
const MAX_ARG: usize = 256;

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

/// Where a `copy` package goes. Each OS has its own folder for each place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Place {
    /// OFX plugins (`.ofx.bundle`).
    Ofx,
    Lut,
    /// Fusion templates and macros, for every user of the workstation.
    FusionTemplates,
    /// Resolve scripts, for every user of the workstation.
    Scripts,
}

/// The file of a build, identified by its content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageFile {
    /// BLAKE3 of the content, in hexadecimal.
    pub blake3: String,
    pub size: u64,
    /// Name given to the file on the workstation. A `.zip` is unpacked by
    /// `copy` and `font`.
    pub name: String,
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
    /// Where a `copy` package goes.
    #[serde(default)]
    pub place: Option<Place>,
    /// Arguments of an `exe` or `run` installer, for a silent installation.
    #[serde(default)]
    pub args: Vec<String>,
    /// What a person has to do, for a `manual` package.
    #[serde(default)]
    pub instructions: Option<String>,
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
    if build.place.is_some() != (build.kind == PackageKind::Copy) {
        return malformed(format!("package `{id}`: place"));
    }
    let takes_args = matches!(build.kind, PackageKind::Exe | PackageKind::Run);
    let args_valid = build.args.len() <= MAX_ARGS
        && build
            .args
            .iter()
            .all(|arg| arg.len() <= MAX_ARG && !arg.contains('\0'));
    if !args_valid || (!takes_args && !build.args.is_empty()) {
        return malformed(format!("package `{id}`: arguments"));
    }
    let manual = build.kind == PackageKind::Manual;
    let instructions_valid = build
        .instructions
        .as_ref()
        .is_some_and(|text| !text.trim().is_empty() && text.len() <= MAX_INSTRUCTIONS);
    if manual != instructions_valid || (!manual && build.instructions.is_some()) {
        return malformed(format!("package `{id}`: instructions"));
    }
    match &build.file {
        Some(_) if manual => malformed(format!("package `{id}`: a manual package has no file")),
        Some(file) if text::hex_exact::<32>(&file.blake3).is_none() => {
            malformed(format!("package `{id}`: file hash"))
        }
        Some(file) if !is_file_name(&file.name) => malformed(format!("package `{id}`: file name")),
        None if !manual => malformed(format!("package `{id}`: file missing")),
        _ => Ok(()),
    }
}

/// A plain file name: it can never lead out of the folder it is written to.
pub fn is_file_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_FILE_NAME
        && !name.starts_with('.')
        && !name.ends_with(['.', ' '])
        && !name.chars().any(|c| {
            c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        })
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
    /// Whether the manifest can be signed and applied.
    pub fn check(&self) -> Result<(), ManifestError> {
        if self.version == 0 {
            return malformed("version 0");
        }
        if self.packages.len() > MAX_PACKAGES {
            return malformed("too many packages");
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

/// Asks the server for the draft. The admin key proves who is asking, so the
/// application needs no password.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftRequest {
    /// Unix time of the request.
    pub at: i64,
    pub signature: Sig,
}

impl DraftRequest {
    pub fn signed(admin_key: &SecretKey, at: i64) -> Self {
        Self {
            at,
            signature: keys::sign(admin_key, DRAFT_DOMAIN, at.to_string().as_bytes()),
        }
    }

    /// Signed by this key, and recent: an old request cannot be sent again.
    pub fn is_valid(&self, admin_key: &PublicKey, now: i64) -> bool {
        self.at.abs_diff(now) <= DRAFT_REQUEST_SKEW
            && keys::verify(
                admin_key,
                DRAFT_DOMAIN,
                self.at.to_string().as_bytes(),
                &self.signature,
            )
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
            name: "plugin.zip".to_owned(),
        })
    }

    fn build(os: Os, arch: Option<&str>, kind: PackageKind) -> PackageBuild {
        let manual = kind == PackageKind::Manual;
        PackageBuild {
            os,
            arch: arch.map(str::to_owned),
            kind,
            file: if manual { None } else { file() },
            place: (kind == PackageKind::Copy).then_some(Place::Ofx),
            args: Vec::new(),
            instructions: manual.then(|| "Ask the admin for the licence.".to_owned()),
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
                    name: "plugin.zip".to_owned(),
                }),
                ..build(Os::Macos, None, PackageKind::Copy)
            }],
            vec![PackageBuild {
                file: Some(PackageFile {
                    blake3: "ab".repeat(32),
                    size: 1,
                    name: "../../evil".to_owned(),
                }),
                ..build(Os::Macos, None, PackageKind::Copy)
            }],
            vec![PackageBuild {
                place: None,
                ..build(Os::Macos, None, PackageKind::Copy)
            }],
            vec![PackageBuild {
                place: Some(Place::Lut),
                ..build(Os::Macos, None, PackageKind::Font)
            }],
            vec![PackageBuild {
                args: vec!["/S".to_owned()],
                ..build(Os::Macos, None, PackageKind::Pkg)
            }],
            vec![PackageBuild {
                args: vec!["a\0b".to_owned()],
                ..build(Os::Windows, None, PackageKind::Exe)
            }],
            vec![PackageBuild {
                instructions: None,
                ..build(Os::Macos, None, PackageKind::Manual)
            }],
            vec![PackageBuild {
                instructions: Some("x".repeat(MAX_INSTRUCTIONS + 1)),
                ..build(Os::Macos, None, PackageKind::Manual)
            }],
            vec![PackageBuild {
                instructions: Some("Not for an installer".to_owned()),
                ..build(Os::Macos, None, PackageKind::Pkg)
            }],
            vec![PackageBuild {
                file: file(),
                ..build(Os::Macos, None, PackageKind::Manual)
            }],
        ];
        for builds in bad_builds {
            let manifest = manifest(vec![package("plugin", builds.clone())]);
            assert!(is_malformed(&manifest), "{builds:?}");
        }
    }

    #[test]
    fn a_manual_package_needs_no_file() {
        let manual = build(Os::Windows, None, PackageKind::Manual);
        let manifest = manifest(vec![package("licence", vec![manual])]);
        assert!(SignedManifest::sign(&key(1), &manifest).is_ok());
    }

    #[test]
    fn an_installer_takes_signed_arguments() {
        let silent = PackageBuild {
            args: vec!["/S".to_owned(), "/NORESTART".to_owned()],
            ..build(Os::Windows, None, PackageKind::Exe)
        };
        let manifest = manifest(vec![package("plugin", vec![silent])]);
        assert!(SignedManifest::sign(&key(1), &manifest).is_ok());
    }

    #[test]
    fn too_many_packages_are_refused() {
        let packages = (0..=MAX_PACKAGES)
            .map(|n| {
                package(
                    &format!("font-{n}"),
                    vec![build(Os::Macos, None, PackageKind::Font)],
                )
            })
            .collect();
        assert!(is_malformed(&manifest(packages)));
    }

    #[test]
    fn file_names_stay_in_their_folder() {
        for good in ["Inter-Regular.ttf", "Boris FX 2025.pkg", "plugin.ofx.zip"] {
            assert!(is_file_name(good), "{good}");
        }
        for bad in [
            "",
            ".",
            "..",
            ".hidden",
            "a/b",
            r"a\b",
            "C:evil",
            "a?",
            "trailing.",
            "trailing ",
            "new\nline",
        ] {
            assert!(!is_file_name(bad), "{bad}");
        }
        assert!(!is_file_name(&"a".repeat(MAX_FILE_NAME + 1)));
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

    #[test]
    fn a_draft_request_is_tied_to_the_key_and_the_time() {
        let key = SecretKey::from_bytes(&[3; 32]);
        let request = DraftRequest::signed(&key, 1_000);
        assert!(request.is_valid(&key.public(), 1_000));
        assert!(request.is_valid(&key.public(), 1_300));
        assert!(!request.is_valid(&key.public(), 1_301), "too old");
        assert!(!request.is_valid(&key.public(), 699), "from the future");
        let other = SecretKey::from_bytes(&[4; 32]);
        assert!(!request.is_valid(&other.public(), 1_000));
        let moved = DraftRequest {
            at: 1_001,
            ..request
        };
        assert!(!moved.is_valid(&key.public(), 1_000), "the time is signed");
    }
}
