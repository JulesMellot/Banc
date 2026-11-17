//! `cargo xtask idle-bench`: agent memory and CPU at rest.
//!
//! Targets from section 10 of the plan: under 15 MB of RAM and CPU at 0%.
//! Two measurements: the agent alone, not enrolled, then the same agent
//! enrolled and connected to a server with its relay (the `bench_server`
//! example of `banc-agent`), which is the normal state of a workstation.
//! Then the session helper (target: 10 MB), connected to that agent. On
//! Linux it has no tray icon yet, so this is its floor; macOS and Windows
//! are measured by hand (docs/measurements.md).
//! The measurement reads `/proc`, so it only runs on Linux for now.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{bail, Context};

use crate::cmd::{cargo, output, run, say, tool};

/// Maximum resident memory of the agent at rest, in kilobytes.
const MAX_RSS_KB: u64 = 15 * 1024;
/// Same for the session helper.
const MAX_SESSION_RSS_KB: u64 = 10 * 1024;
/// Maximum CPU at rest, in thousandths of elapsed time (1 = 0.1%).
const MAX_CPU_PER_MILLE: u64 = 1;
/// Time given to a process to settle before a measurement.
const WARM_UP: Duration = Duration::from_secs(2);
/// After its enrollment the agent sets up its connection, its relay and
/// its network report: that is start-up work, not rest.
const SETTLE_AFTER_ENROLLMENT: Duration = Duration::from_secs(10);
/// Longest wait for the agent to connect after its enrollment.
const CONNECT_LIMIT: Duration = Duration::from_secs(30);

#[derive(Debug, PartialEq, Eq)]
struct Measure {
    rss_kb: u64,
    cpu_ticks: u64,
}

/// Kills the process on exit, even on error.
struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        // The process may already be dead: then there is nothing to do.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub fn idle_bench(root: &Path, seconds: u64) -> anyhow::Result<()> {
    if seconds == 0 {
        bail!("the measurement duration must be at least 1 second");
    }
    if !Path::new("/proc/self/stat").exists() {
        bail!("idle-bench reads /proc: the measurement only runs on Linux for now");
    }
    let build = [
        "build",
        "--release",
        "--locked",
        "-p",
        "banc-agent",
        "--bin",
        "banc-agent",
        "--example",
        "bench_server",
        "-p",
        "banc-session",
        "--bin",
        "banc-session",
    ];
    run(cargo(&build).current_dir(root))?;
    let ticks_per_second: u64 = output(tool("getconf").arg("CLK_TCK"))?
        .trim()
        .parse()
        .context("CLK_TCK value")?;

    let work = root.join("target/idle-bench");
    match std::fs::remove_dir_all(&work) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
        _ => {}
    }
    std::fs::create_dir_all(&work)?;
    let binary = root.join("target/release/banc-agent");
    let socket = work.join("agent.sock");
    let agent = Command::new(&binary)
        .arg("--log-dir")
        .arg(work.join("logs"))
        .arg("--state-dir")
        .arg(work.join("agent"))
        .arg("--socket")
        .arg(&socket)
        .stdin(Stdio::null())
        .spawn()
        .context("starting banc-agent")?;
    let agent = KillOnDrop(agent);
    let pid = agent.0.id();

    let mut failures = Vec::new();
    let alone = measure_period(pid, "banc-agent", seconds, "not enrolled")?;
    failures.extend(check(&alone, MAX_RSS_KB, seconds, ticks_per_second));

    // Enrolled and connected.
    let mut server = Command::new(root.join("target/release/examples/bench_server"))
        .arg(work.join("server"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .context("starting bench_server")?;
    let stdout = server.stdout.take().context("bench_server output")?;
    let server = KillOnDrop(server);
    let mut link = String::new();
    BufReader::new(stdout)
        .read_line(&mut link)
        .context("reading the invitation link")?;
    let link = link.trim();
    if !link.starts_with("banc://join?") {
        bail!("bench_server printed no invitation link");
    }
    let enroll = output(
        Command::new(&binary)
            .args(["enroll", link, "--socket"])
            .arg(&socket),
    )
    .context("enrolling the agent")?;
    if !enroll.contains("\"enrollment\"") {
        bail!("enrollment failed: {enroll}");
    }
    wait_connected(&binary, &socket)?;
    std::thread::sleep(SETTLE_AFTER_ENROLLMENT);
    let enrolled = measure_period(pid, "banc-agent", seconds, "enrolled and connected")?;
    failures.extend(check(&enrolled, MAX_RSS_KB, seconds, ticks_per_second));

    let session = Command::new(root.join("target/release/banc-session"))
        .arg("--log-dir")
        .arg(work.join("logs"))
        .arg("--socket")
        .arg(&socket)
        .stdin(Stdio::null())
        .spawn()
        .context("starting banc-session")?;
    let session = KillOnDrop(session);
    let measured = measure_period(session.0.id(), "banc-session", seconds, "next to the agent")?;
    failures.extend(check(
        &measured,
        MAX_SESSION_RSS_KB,
        seconds,
        ticks_per_second,
    ));
    drop(session);
    drop(agent);
    drop(server);

    if !failures.is_empty() {
        bail!("{}", failures.join("; "));
    }
    say("targets at rest met");
    Ok(())
}

/// Waits for `banc-agent status` to report the connection.
fn wait_connected(binary: &Path, socket: &Path) -> anyhow::Result<()> {
    let start = Instant::now();
    while start.elapsed() < CONNECT_LIMIT {
        let status = output(
            Command::new(binary)
                .arg("status")
                .arg("--socket")
                .arg(socket),
        )?;
        if status.contains("\"connection\": \"connected\"") {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    bail!("the agent did not connect to the server within {CONNECT_LIMIT:?}")
}

/// Waits, then measures memory and the CPU used over `seconds`.
fn measure_period(pid: u32, name: &str, seconds: u64, label: &str) -> anyhow::Result<Measure> {
    std::thread::sleep(WARM_UP);
    let before = measure(pid)?;
    say(&format!(
        "measuring {name} at rest, {label}, for {seconds} s..."
    ));
    std::thread::sleep(Duration::from_secs(seconds));
    let after = measure(pid)?;
    Ok(Measure {
        rss_kb: after.rss_kb,
        cpu_ticks: after.cpu_ticks.saturating_sub(before.cpu_ticks),
    })
}

/// Prints the measure; returns what exceeds the targets.
fn check(measure: &Measure, max_rss_kb: u64, seconds: u64, ticks_per_second: u64) -> Vec<String> {
    let allowed = allowed_ticks(seconds, ticks_per_second);
    let cpu_percent = measure.cpu_ticks as f64 * 100.0 / (seconds * ticks_per_second) as f64;
    say(&format!(
        "  resident memory: {:.1} MB (target: under {} MB)",
        measure.rss_kb as f64 / 1024.0,
        max_rss_kb / 1024
    ));
    say(&format!(
        "  CPU: {cpu_percent:.2}% ({} tick(s) in {seconds} s, {allowed} allowed)",
        measure.cpu_ticks
    ));
    let mut failures = Vec::new();
    if measure.rss_kb >= max_rss_kb {
        failures.push("memory target exceeded at rest".to_owned());
    }
    if measure.cpu_ticks > allowed {
        failures.push("CPU used at rest".to_owned());
    }
    failures
}

/// Number of CPU ticks allowed over the measurement period.
fn allowed_ticks(seconds: u64, ticks_per_second: u64) -> u64 {
    seconds * ticks_per_second * MAX_CPU_PER_MILLE / 1000
}

fn measure(pid: u32) -> anyhow::Result<Measure> {
    let read = |file: &str| {
        let path = format!("/proc/{pid}/{file}");
        std::fs::read_to_string(&path).with_context(|| format!("reading {path} (process stopped?)"))
    };
    Ok(Measure {
        rss_kb: parse_rss_kb(&read("status")?).context("VmRSS missing from /proc/<pid>/status")?,
        cpu_ticks: parse_cpu_ticks(&read("stat")?).context("format of /proc/<pid>/stat")?,
    })
}

/// The `VmRSS:     1234 kB` line of `/proc/<pid>/status`.
fn parse_rss_kb(status: &str) -> Option<u64> {
    let line = status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))?;
    line.trim().strip_suffix("kB")?.trim().parse().ok()
}

/// Sum of user and system times from `/proc/<pid>/stat`, in ticks.
fn parse_cpu_ticks(stat: &str) -> Option<u64> {
    // The process name, in parentheses, may contain spaces: fields are
    // counted after the last closing parenthesis.
    let (_, fields) = stat.rsplit_once(')')?;
    let mut fields = fields.split_whitespace();
    // After the name come the state, then ten fields, then utime and stime.
    let user: u64 = fields.nth(11)?.parse().ok()?;
    let system: u64 = fields.next()?.parse().ok()?;
    Some(user + system)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resident_memory() {
        let status = "Name:\tbanc-agent\nVmPeak:\t   12000 kB\nVmRSS:\t    2816 kB\nThreads:\t2\n";
        assert_eq!(parse_rss_kb(status), Some(2816));
        assert_eq!(parse_rss_kb("Name:\tbanc-agent\n"), None);
    }

    #[test]
    fn cpu_time() {
        let stat = "4242 (banc agent (x)) S 1 4242 4242 0 -1 4194304 150 0 0 0 7 3 0 0 20 0 2 0 123456 1000000 700\n";
        assert_eq!(parse_cpu_ticks(stat), Some(10));
        assert_eq!(parse_cpu_ticks("4242 (banc-agent) S 1 2"), None);
        assert_eq!(parse_cpu_ticks(""), None);
    }

    #[test]
    fn ticks_allowed() {
        assert_eq!(allowed_ticks(10, 100), 1);
        assert_eq!(allowed_ticks(60, 100), 6);
        assert_eq!(allowed_ticks(5, 100), 0);
    }
}
