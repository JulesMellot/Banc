//! `cargo xtask update-e2e`: a real update on this machine.
//!
//! Installs a first version as a real system service, then publishes on a
//! local download site: a working version, one signed with the wrong key,
//! and a broken one. Needs administrator rights for `--run`, which is why
//! building and running are two steps.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{bail, Context};
use banc_platform::paths::{self, Scope};
use banc_proto::keys::SecretKey;

use crate::cmd::{cargo, output, run, say};
use crate::release;

const PORT: u16 = 18480;
const FIRST: &str = "1.0.0";
const WORKING: &str = "1.0.1";
const FORGED: &str = "1.0.2";
const BROKEN: &str = "1.0.3";
/// The test key: it signs nothing but these throwaway versions.
const TEST_KEY: [u8; 32] = [42; 32];

fn work_dir(root: &Path) -> PathBuf {
    root.join("target/update-e2e")
}

fn exe(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

/// The installed application, as on a real workstation.
fn install_root() -> PathBuf {
    if cfg!(target_os = "macos") {
        PathBuf::from("/Applications/BancE2E.app")
    } else if cfg!(windows) {
        PathBuf::from(r"C:\Program Files\BancE2E")
    } else {
        PathBuf::from("/opt/banc-e2e")
    }
}

fn program_prefix() -> &'static str {
    if cfg!(target_os = "macos") {
        "Contents/MacOS/"
    } else {
        ""
    }
}

fn installed(name: &str) -> PathBuf {
    install_root().join(format!("{}{}", program_prefix(), exe(name)))
}

/// Builds the same programs under each version number.
pub fn build(root: &Path) -> anyhow::Result<()> {
    let work = work_dir(root);
    let public = release::public_hex(&SecretKey::from_bytes(&TEST_KEY));
    for version in [FIRST, WORKING] {
        let mut command = cargo(&[
            "build",
            "--locked",
            "-p",
            "banc-agent",
            "-p",
            "banc-session",
        ]);
        command
            .env("BANC_VERSION", version)
            .env("BANC_RELEASE_PUBLIC_KEY", &public)
            .env("BANC_UPDATE_SITE", format!("http://127.0.0.1:{PORT}"))
            .env("BANC_UPDATE_CHECK_SECS", "3")
            .env("CARGO_TARGET_DIR", work.join("target"));
        run(command.current_dir(root))?;
        let dir = work.join("builds").join(version);
        std::fs::create_dir_all(&dir)?;
        for name in ["banc-agent", "banc-session"] {
            std::fs::copy(
                work.join("target/debug").join(exe(name)),
                dir.join(exe(name)),
            )?;
        }
    }
    Ok(())
}

/// A static site with the paths of GitHub releases.
fn serve(site: PathBuf) -> anyhow::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", PORT))?;
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let _ = answer(stream, &site);
        }
    });
    Ok(())
}

fn answer(mut stream: std::net::TcpStream, site: &Path) -> std::io::Result<()> {
    let mut request = [0; 2048];
    let read = stream.read(&mut request)?;
    let request = String::from_utf8_lossy(&request[..read]);
    let path = request.split_whitespace().nth(1).unwrap_or("");
    let file = if path == "/latest/download/update.json" {
        Some(site.join("update.json"))
    } else {
        path.rsplit('/')
            .next()
            .filter(|_| path.starts_with("/download/"))
            .map(|asset| site.join("assets").join(asset))
    };
    let body = file.and_then(|file| std::fs::read(file).ok());
    let status = if body.is_some() {
        "200 OK"
    } else {
        "404 Not Found"
    };
    let body = body.unwrap_or_default();
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(&body)
}

/// Publishes `programs` as `version`, signed with `key`.
fn publish(
    site: &Path,
    version: &str,
    programs: [(&str, PathBuf); 2],
    key: &SecretKey,
) -> anyhow::Result<()> {
    let files: Vec<_> = programs
        .into_iter()
        .map(|(name, source)| (format!("{}{}", program_prefix(), exe(name)), source))
        .collect();
    let payload = site.join("payload");
    let _ = std::fs::remove_dir_all(&payload);
    let described = release::write_payload(&files, &release::platform(), &payload)?;
    let assets = site.join("assets");
    std::fs::create_dir_all(&assets)?;
    for file in &described {
        std::fs::copy(
            payload.join("assets").join(&file.asset),
            assets.join(&file.asset),
        )?;
    }
    release::sign_manifest(&payload, version.parse()?, key, &site.join("update.json"))?;
    say(&format!("published {version}"));
    Ok(())
}

/// The version the running agent reports, `None` if it does not answer.
fn running_version() -> Option<String> {
    let text = output(Command::new(installed("banc-agent")).arg("status")).ok()?;
    let status: serde_json::Value = serde_json::from_str(&text).ok()?;
    status["version"].as_str().map(str::to_owned)
}

fn wait_for(what: &str, limit: Duration, done: impl Fn() -> bool) -> anyhow::Result<()> {
    let start = Instant::now();
    while start.elapsed() < limit {
        if done() {
            say(&format!("ok: {what} ({}s)", start.elapsed().as_secs()));
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    bail!("timed out waiting for: {what}")
}

fn version_on_disk() -> anyhow::Result<String> {
    Ok(
        output(Command::new(installed("banc-agent")).arg("--version"))?
            .trim()
            .to_owned(),
    )
}

fn expect_on_disk(version: &str) -> anyhow::Result<()> {
    let found = version_on_disk()?;
    if found != format!("banc-agent {version}") {
        bail!("expected version {version} on disk, found `{found}`");
    }
    Ok(())
}

fn update_folder() -> PathBuf {
    let mut name = install_root()
        .file_name()
        .unwrap_or_default()
        .to_os_string();
    name.push(".update");
    install_root().with_file_name(name)
}

fn scenario(work: &Path) -> anyhow::Result<()> {
    let site = work.join("site");
    let builds = work.join("builds");
    let key = SecretKey::from_bytes(&TEST_KEY);
    let build_of = |version: &str, name: &str| builds.join(version).join(exe(name));
    serve(site.clone())?;

    // 1. A first version, as the installer leaves it.
    for name in ["banc-agent", "banc-session"] {
        let target = installed(name);
        std::fs::create_dir_all(target.parent().context("no parent")?)?;
        std::fs::copy(build_of(FIRST, name), &target)?;
    }
    run(Command::new(installed("banc-agent")).args(["service", "install"]))?;
    wait_for("the first version runs", Duration::from_secs(60), || {
        running_version().as_deref() == Some(FIRST)
    })?;

    // 2. A working version replaces it, with nobody touching the machine.
    let programs = |version: &str| {
        [
            ("banc-agent", build_of(version, "banc-agent")),
            ("banc-session", build_of(version, "banc-session")),
        ]
    };
    publish(&site, WORKING, programs(WORKING), &key)?;
    wait_for("the update is installed", Duration::from_secs(240), || {
        running_version().as_deref() == Some(WORKING)
    })?;
    expect_on_disk(WORKING)?;
    wait_for("the update has finished", Duration::from_secs(90), || {
        !update_folder().join("pending.json").exists() && !update_folder().join("previous").exists()
    })?;

    // 3. A version signed with another key is ignored.
    let forger = SecretKey::from_bytes(&[7; 32]);
    publish(&site, FORGED, programs(WORKING), &forger)?;
    std::thread::sleep(Duration::from_secs(20));
    if running_version().as_deref() != Some(WORKING) {
        bail!("a forged release changed the running version");
    }
    expect_on_disk(WORKING)?;
    say("ok: the forged release is ignored");

    // 4. A broken version: the "agent" is a program that exits at once.
    let failed = paths::state_dir(Scope::System)?.join("update-failed");
    publish(
        &site,
        BROKEN,
        [
            ("banc-agent", build_of(WORKING, "banc-session")),
            ("banc-session", build_of(WORKING, "banc-session")),
        ],
        &key,
    )?;
    wait_for(
        "the broken version is refused",
        Duration::from_secs(420),
        || std::fs::read_to_string(&failed).is_ok_and(|text| text.trim() == BROKEN),
    )?;
    wait_for(
        "the working version is back",
        Duration::from_secs(120),
        || running_version().as_deref() == Some(WORKING),
    )?;
    expect_on_disk(WORKING)?;
    Ok(())
}

/// Runs the scenario, then removes everything from the machine.
pub fn run_scenario(root: &Path) -> anyhow::Result<()> {
    let result = scenario(&work_dir(root));
    if result.is_err() {
        print_logs();
    }
    let _ = Command::new(installed("banc-agent"))
        .args(["service", "uninstall"])
        .status();
    let _ = std::fs::remove_dir_all(install_root());
    let _ = std::fs::remove_dir_all(update_folder());
    result?;
    say("update scenario passed");
    Ok(())
}

/// The agent's logs, to understand a failure in CI.
fn print_logs() {
    let Ok(dir) = paths::log_dir(Scope::System) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    for entry in entries.flatten() {
        if let Ok(text) = std::fs::read_to_string(entry.path()) {
            say(&format!("--- {}\n{text}", entry.path().display()));
        }
    }
}
