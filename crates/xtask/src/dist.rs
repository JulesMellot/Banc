//! `cargo xtask dist`: release binaries, sorted by target.
//!
//! Installers are added here in phase 7.

use std::path::Path;

use anyhow::Context;

use crate::cmd::{cargo, output, run, say, tool};

const BINARIES: &[&str] = &["banc-agent", "banc-session", "banc-server"];

pub fn dist(root: &Path, targets: &[String]) -> anyhow::Result<()> {
    if targets.is_empty() {
        return build(root, &host_target()?, false);
    }
    for target in targets {
        build(root, target, true)?;
    }
    Ok(())
}

/// The current machine's target, read from `rustc -vV`.
fn host_target() -> anyhow::Result<String> {
    let version = output(tool("rustc").arg("-vV"))?;
    parse_host(&version)
        .map(str::to_owned)
        .context("cible de la machine absente de `rustc -vV`")
}

fn parse_host(rustc_version: &str) -> Option<&str> {
    rustc_version
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
}

fn build(root: &Path, target: &str, cross: bool) -> anyhow::Result<()> {
    let mut command = cargo(&["build", "--release", "--locked"]);
    for binary in BINARIES {
        command.args(["-p", binary]);
    }
    let mut built = root.join("target");
    if cross {
        command.args(["--target", target]);
        built.push(target);
    }
    built.push("release");
    run(command.current_dir(root))?;

    let dist = root.join("dist").join(target);
    std::fs::create_dir_all(&dist).with_context(|| format!("création de {}", dist.display()))?;
    let suffix = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    for binary in BINARIES {
        let file = format!("{binary}{suffix}");
        let from = built.join(&file);
        std::fs::copy(&from, dist.join(&file))
            .with_context(|| format!("copie de {}", from.display()))?;
    }
    say(&format!("binaires dans {}", dist.display()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cible_de_la_machine() {
        let version = "rustc 1.97.0\nbinary: rustc\nhost: aarch64-apple-darwin\nrelease: 1.97.0\n";
        assert_eq!(parse_host(version), Some("aarch64-apple-darwin"));
        assert_eq!(parse_host("rustc 1.97.0\n"), None);
    }
}
