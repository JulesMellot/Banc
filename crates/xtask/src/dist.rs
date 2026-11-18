//! `cargo xtask dist`: release binaries, sorted by target.
//! `cargo xtask bundle-app`: the window with the agent and the session
//! helper beside it, as an installer and as the files of an update.

use std::path::Path;

use anyhow::Context;

use crate::cmd::{cargo, output, run, say, tool};
use crate::release;

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

/// Builds the agent and the session helper, places them where Tauri
/// expects its side programs, then bundles the window for this machine.
pub fn bundle_app(
    root: &Path,
    version: Option<&str>,
    payload: Option<&Path>,
) -> anyhow::Result<()> {
    let target = host_target()?;
    let mut build = cargo(&[
        "build",
        "--release",
        "--locked",
        "-p",
        "banc-agent",
        "--bin",
        "banc-agent",
        "-p",
        "banc-session",
        "--bin",
        "banc-session",
    ]);
    if let Some(version) = version {
        build.env("BANC_VERSION", version);
    }
    run(build.current_dir(root))?;
    let ui = root.join("ui");
    let binaries = ui.join("src-tauri/binaries");
    std::fs::create_dir_all(&binaries)?;
    let suffix = std::env::consts::EXE_SUFFIX;
    for binary in ["banc-agent", "banc-session"] {
        let from = root.join(format!("target/release/{binary}{suffix}"));
        let to = binaries.join(format!("{binary}-{target}{suffix}"));
        std::fs::copy(&from, &to)
            .with_context(|| format!("copying {} to {}", from.display(), to.display()))?;
    }
    for args in [&["install", "--frozen-lockfile"][..], &["build"]] {
        run(tool("pnpm").args(args).current_dir(&ui))?;
    }
    let mut tauri = tool("pnpm");
    tauri.args(["tauri", "build", "--config", "src-tauri/bundle.conf.json"]);
    if let Some(version) = version {
        // A file: the quotes of inline JSON do not survive `pnpm.cmd`.
        let config = ui.join("src-tauri/target/version.conf.json");
        std::fs::create_dir_all(ui.join("src-tauri/target"))?;
        std::fs::write(&config, format!("{{\"version\":\"{version}\"}}"))?;
        tauri.arg("--config").arg(&config);
    }
    run(tauri.current_dir(&ui))?;
    if let Some(payload) = payload {
        let files = release::bundled_files(root)?;
        release::write_payload(&files, &release::platform(), payload)?;
    }
    say(&format!(
        "\nBundles in {}",
        ui.join("src-tauri/target/release/bundle").display()
    ));
    Ok(())
}

/// The current machine's target, read from `rustc -vV`.
fn host_target() -> anyhow::Result<String> {
    let version = output(tool("rustc").arg("-vV"))?;
    parse_host(&version)
        .map(str::to_owned)
        .context("host target missing from `rustc -vV`")
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
    std::fs::create_dir_all(&dist).with_context(|| format!("creating {}", dist.display()))?;
    let suffix = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    for binary in BINARIES {
        let file = format!("{binary}{suffix}");
        let from = built.join(&file);
        std::fs::copy(&from, dist.join(&file))
            .with_context(|| format!("copying {}", from.display()))?;
    }
    say(&format!("binaries in {}", dist.display()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_target() {
        let version = "rustc 1.97.0\nbinary: rustc\nhost: aarch64-apple-darwin\nrelease: 1.97.0\n";
        assert_eq!(parse_host(version), Some("aarch64-apple-darwin"));
        assert_eq!(parse_host("rustc 1.97.0\n"), None);
    }
}
