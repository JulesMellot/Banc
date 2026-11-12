//! Repository rule: everything that depends on the OS lives in `banc-platform`.
//!
//! The check walks the other `banc-*` crates and refuses OS `cfg`s,
//! `std::os::<os>` modules and per-target dependencies.

use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::Context;

const PLATFORM_CRATE: &str = "banc-platform";

#[derive(Debug, PartialEq, Eq)]
pub struct Violation {
    pub path: PathBuf,
    pub line: usize,
    pub text: String,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.path.display(), self.line, self.text)
    }
}

/// Walks `crates/banc-*` except `banc-platform`.
pub fn scan(root: &Path) -> anyhow::Result<Vec<Violation>> {
    let crates = root.join("crates");
    let mut files = Vec::new();
    for entry in std::fs::read_dir(&crates).context("lecture de crates/")? {
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if path.is_dir() && name.starts_with("banc-") && name != PLATFORM_CRATE {
            collect(&path, &mut files)?;
        }
    }
    files.sort();

    let mut violations = Vec::new();
    for file in files {
        let is_manifest = file.file_name().is_some_and(|name| name == "Cargo.toml");
        let text = std::fs::read_to_string(&file)
            .with_context(|| format!("lecture de {}", file.display()))?;
        for (index, line) in text.lines().enumerate() {
            let refused = if is_manifest {
                manifest_line_is_os_specific(line)
            } else {
                rust_line_is_os_specific(line)
            };
            if refused {
                violations.push(Violation {
                    path: file.strip_prefix(root).unwrap_or(&file).to_path_buf(),
                    line: index + 1,
                    text: line.trim().to_owned(),
                });
            }
        }
    }
    Ok(violations)
}

/// Adds the folder's `.rs` and `Cargo.toml` files, skipping `target/`.
fn collect(dir: &Path, files: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("lecture de {}", dir.display()))? {
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if path.is_dir() {
            if name != "target" {
                collect(&path, files)?;
            }
        } else if name == "Cargo.toml" || name.ends_with(".rs") {
            files.push(path);
        }
    }
    Ok(())
}

fn rust_line_is_os_specific(line: &str) -> bool {
    let line = line.trim_start();
    if line.starts_with("//") {
        return false;
    }
    const OS_MODULES: &[&str] = &[
        "std::os::unix",
        "std::os::windows",
        "std::os::linux",
        "std::os::macos",
        "std::os::darwin",
    ];
    if OS_MODULES.iter().any(|module| line.contains(module)) {
        return true;
    }
    const OS_WORDS: &[&str] = &[
        "target_os",
        "target_family",
        "target_vendor",
        "unix",
        "windows",
    ];
    line.contains("cfg") && OS_WORDS.iter().any(|word| contains_word(line, word))
}

fn manifest_line_is_os_specific(line: &str) -> bool {
    line.trim_start().starts_with("[target.")
}

/// True if `word` appears with no letter, digit or `_` attached around it.
fn contains_word(text: &str, word: &str) -> bool {
    let is_word_char = |c: char| c.is_alphanumeric() || c == '_';
    text.match_indices(word).any(|(start, _)| {
        let before = text[..start].chars().next_back();
        let after = text[start + word.len()..].chars().next();
        !before.is_some_and(is_word_char) && !after.is_some_and(is_word_char)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lignes_rust_refusees() {
        for line in [
            "#[cfg(target_os = \"macos\")]",
            "    #[cfg(any(target_os = \"linux\", target_os = \"macos\"))]",
            "#[cfg(unix)]",
            "#[cfg(not(windows))]",
            "#[cfg_attr(windows, path = \"win.rs\")]",
            "if cfg!(windows) {",
            "let mac = cfg!(target_os = \"macos\");",
            "#[cfg(target_family = \"unix\")]",
            "use std::os::unix::fs::PermissionsExt;",
            "use std::os::windows::process::CommandExt;",
        ] {
            assert!(rust_line_is_os_specific(line), "{line}");
        }
    }

    #[test]
    fn lignes_rust_acceptees() {
        for line in [
            "#[cfg(test)]",
            "#[cfg(feature = \"faux\")]",
            "// #[cfg(windows)] se trouve dans banc-platform",
            "let windows = config.windows_drive;",
            "let config = load(\"unix.toml\");",
            "#[cfg(debug_assertions)] let unixtime = now();",
            "use std::os::raw::c_int;",
        ] {
            assert!(!rust_line_is_os_specific(line), "{line}");
        }
    }

    #[test]
    fn lignes_de_manifeste() {
        assert!(manifest_line_is_os_specific(
            "[target.'cfg(windows)'.dependencies]"
        ));
        assert!(manifest_line_is_os_specific(
            "[target.x86_64-pc-windows-msvc.dependencies]"
        ));
        assert!(!manifest_line_is_os_specific("[dependencies]"));
        assert!(!manifest_line_is_os_specific("target = \"x\""));
    }

    #[test]
    fn parcours_des_crates_sauf_platform() {
        let tmp = tempfile::tempdir().unwrap();
        let write = |path: &str, text: &str| {
            let path = tmp.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        write(
            "crates/banc-platform/src/lib.rs",
            "#[cfg(windows)]\nmod w;\n",
        );
        write("crates/xtask/src/main.rs", "let w = cfg!(windows);\n");
        write("crates/banc-agent/src/main.rs", "fn main() {}\n");
        write(
            "crates/banc-agent/src/deep/os.rs",
            "\n#[cfg(unix)]\nfn f() {}\n",
        );
        write("crates/banc-agent/target/debug/x.rs", "#[cfg(unix)]\n");
        write(
            "crates/banc-session/Cargo.toml",
            "[target.'cfg(windows)'.dependencies]\n",
        );

        let violations = scan(tmp.path()).unwrap();
        let found: Vec<String> = violations
            .iter()
            .map(|v| format!("{}:{}", v.path.display(), v.line).replace('\\', "/"))
            .collect();
        assert_eq!(
            found,
            [
                "crates/banc-agent/src/deep/os.rs:2",
                "crates/banc-session/Cargo.toml:1",
            ]
        );
    }
}
