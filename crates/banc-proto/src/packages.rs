//! Delivery of the manifest and of package files to the agents.
//!
//! The agent opens an iroh connection with [`ALPN`] and one bidirectional
//! stream per request: a [`Request`] frame, an [`Answer`] frame, then the raw
//! bytes announced by the answer. The iroh handshake proves the machine key.
//! Nothing that travels here is trusted: the manifest carries the admin's
//! signature, and each file is checked against the hash the manifest signs.

use serde::{Deserialize, Serialize};

/// iroh protocol identifier of the package delivery.
pub const ALPN: &[u8] = b"banc/packages/1";

/// Largest signed manifest accepted.
pub const MAX_MANIFEST: u64 = 4 * 1024 * 1024;

/// Longest error text kept in a report, in bytes.
pub const MAX_ERROR: usize = 160;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    /// The latest published manifest.
    Manifest,
    /// A file listed in the latest published manifest.
    File { blake3: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Answer {
    /// `size` bytes follow.
    Found {
        size: u64,
    },
    NotFound,
}

/// Where one package stands on a workstation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageState {
    Installed,
    /// Waiting for its turn, or for Resolve to close.
    Pending,
    Failed,
    /// Waiting for a person to follow the instructions.
    Manual,
    /// Required, but the manifest has no build for this machine.
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageReport {
    pub id: String,
    /// The version present on the workstation.
    #[serde(default)]
    pub version: Option<String>,
    pub state: PackageState,
    /// Why it failed, at most [`MAX_ERROR`] bytes.
    #[serde(default)]
    pub error: Option<String>,
    /// What a person has to do, for a manual package.
    #[serde(default)]
    pub instructions: Option<String>,
}

/// Where a workstation stands against the manifest.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkstationReport {
    /// The manifest applied here, `None` before the first one.
    pub manifest_version: Option<u64>,
    /// Why the latest manifest was refused.
    #[serde(default)]
    pub refused: Option<String>,
    /// The Resolve version is not the required one: projects stay closed.
    #[serde(default)]
    pub resolve_mismatch: bool,
    /// The version the team requires.
    #[serde(default)]
    pub resolve_required: Option<String>,
    pub packages: Vec<PackageReport>,
}

impl WorkstationReport {
    /// Whether a project may be opened on this workstation.
    pub fn can_open_projects(&self) -> bool {
        !self.resolve_mismatch
    }
}

/// `text` cut to at most [`MAX_ERROR`] bytes, on a character boundary.
pub fn short_error(text: &str) -> String {
    let mut end = text.len().min(MAX_ERROR);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::{write_frame, MAX_FRAME};
    use crate::manifest::{MAX_INSTRUCTIONS, MAX_PACKAGES};

    #[test]
    fn errors_are_cut_between_characters() {
        assert_eq!(short_error("short"), "short");
        let long = "é".repeat(MAX_ERROR);
        let cut = short_error(&long);
        assert!(cut.len() <= MAX_ERROR);
        assert!(cut.chars().all(|c| c == 'é'));
    }

    #[test]
    fn a_mismatch_keeps_projects_closed() {
        let report = WorkstationReport {
            resolve_mismatch: true,
            ..WorkstationReport::default()
        };
        assert!(!report.can_open_projects());
        assert!(WorkstationReport::default().can_open_projects());
    }

    #[tokio::test]
    async fn the_largest_report_fits_in_one_frame() {
        let packages = (0..MAX_PACKAGES)
            .map(|_| PackageReport {
                id: "a".repeat(64),
                version: Some("1".repeat(64)),
                state: PackageState::Manual,
                error: None,
                instructions: Some("x".repeat(MAX_INSTRUCTIONS)),
            })
            .collect();
        let report = WorkstationReport {
            manifest_version: Some(u64::MAX),
            refused: Some("x".repeat(MAX_ERROR)),
            resolve_mismatch: true,
            resolve_required: Some("21.1.0".to_owned()),
            packages,
        };
        let mut buffer = Vec::new();
        write_frame(&mut buffer, &report).await.unwrap();
        assert!(buffer.len() <= MAX_FRAME);
    }
}
