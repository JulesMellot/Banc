//! `cargo xtask idle-bench` : mémoire et CPU de l'agent au repos.
//!
//! Cibles de la section 10 du plan : moins de 15 Mo de RAM et CPU à 0 %.
//! La mesure lit `/proc`, elle ne tourne donc que sous Linux pour l'instant.
//! L'assistant de session (cible : 10 Mo) s'ajoute quand il reste en vie,
//! en phase 1.

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use anyhow::{bail, Context};

use crate::cmd::{cargo, output, run, say, tool};

/// Mémoire résidente maximale de l'agent au repos, en kilo-octets.
const MAX_RSS_KB: u64 = 15 * 1024;
/// CPU maximal au repos, en millièmes du temps écoulé (1 = 0,1 %).
const MAX_CPU_PER_MILLE: u64 = 1;
/// Temps laissé à l'agent pour finir de démarrer avant la mesure.
const WARM_UP: Duration = Duration::from_secs(2);

#[derive(Debug, PartialEq, Eq)]
struct Measure {
    rss_kb: u64,
    cpu_ticks: u64,
}

/// Tue le processus à la sortie, même en cas d'erreur.
struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        // Le processus peut déjà être mort : il n'y a alors rien à faire.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub fn idle_bench(root: &Path, seconds: u64) -> anyhow::Result<()> {
    if seconds == 0 {
        bail!("la durée de mesure doit valoir au moins 1 seconde");
    }
    if !Path::new("/proc/self/stat").exists() {
        bail!("idle-bench lit /proc : la mesure ne tourne que sous Linux pour l'instant");
    }
    run(cargo(&["build", "--release", "--locked", "-p", "banc-agent"]).current_dir(root))?;
    let ticks_per_second: u64 = output(tool("getconf").arg("CLK_TCK"))?
        .trim()
        .parse()
        .context("valeur de CLK_TCK")?;

    let logs = root.join("target/idle-bench/journaux");
    let agent = Command::new(root.join("target/release/banc-agent"))
        .arg("--log-dir")
        .arg(&logs)
        .stdin(Stdio::null())
        .spawn()
        .context("lancement de banc-agent")?;
    let agent = KillOnDrop(agent);
    let pid = agent.0.id();

    std::thread::sleep(WARM_UP);
    let before = measure(pid)?;
    say(&format!(
        "mesure de banc-agent au repos pendant {seconds} s..."
    ));
    std::thread::sleep(Duration::from_secs(seconds));
    let after = measure(pid)?;
    drop(agent);

    let cpu_ticks = after.cpu_ticks.saturating_sub(before.cpu_ticks);
    let allowed_ticks = allowed_ticks(seconds, ticks_per_second);
    let cpu_percent = cpu_ticks as f64 * 100.0 / (seconds * ticks_per_second) as f64;
    say(&format!(
        "mémoire résidente : {:.1} Mo (cible : moins de {} Mo)",
        after.rss_kb as f64 / 1024.0,
        MAX_RSS_KB / 1024
    ));
    say(&format!(
        "CPU : {cpu_percent:.2} % ({cpu_ticks} tic(s) en {seconds} s, {allowed_ticks} admis)"
    ));

    if after.rss_kb >= MAX_RSS_KB {
        bail!("l'agent dépasse la cible de mémoire au repos");
    }
    if cpu_ticks > allowed_ticks {
        bail!("l'agent consomme du CPU au repos");
    }
    say("cibles au repos tenues");
    Ok(())
}

/// Nombre de tics de CPU admis sur la durée de mesure.
fn allowed_ticks(seconds: u64, ticks_per_second: u64) -> u64 {
    seconds * ticks_per_second * MAX_CPU_PER_MILLE / 1000
}

fn measure(pid: u32) -> anyhow::Result<Measure> {
    let read = |file: &str| {
        let path = format!("/proc/{pid}/{file}");
        std::fs::read_to_string(&path)
            .with_context(|| format!("lecture de {path} (agent arrêté ?)"))
    };
    Ok(Measure {
        rss_kb: parse_rss_kb(&read("status")?).context("VmRSS absent de /proc/<pid>/status")?,
        cpu_ticks: parse_cpu_ticks(&read("stat")?).context("format de /proc/<pid>/stat")?,
    })
}

/// Ligne `VmRSS:     1234 kB` de `/proc/<pid>/status`.
fn parse_rss_kb(status: &str) -> Option<u64> {
    let line = status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))?;
    line.trim().strip_suffix("kB")?.trim().parse().ok()
}

/// Somme des temps utilisateur et système de `/proc/<pid>/stat`, en tics.
fn parse_cpu_ticks(stat: &str) -> Option<u64> {
    // Le nom du processus, entre parenthèses, peut contenir des espaces :
    // les champs se comptent après la dernière parenthèse fermante.
    let (_, fields) = stat.rsplit_once(')')?;
    let mut fields = fields.split_whitespace();
    // Après le nom viennent l'état puis dix champs, et enfin utime et stime.
    let user: u64 = fields.nth(11)?.parse().ok()?;
    let system: u64 = fields.next()?.parse().ok()?;
    Some(user + system)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memoire_residente() {
        let status = "Name:\tbanc-agent\nVmPeak:\t   12000 kB\nVmRSS:\t    2816 kB\nThreads:\t2\n";
        assert_eq!(parse_rss_kb(status), Some(2816));
        assert_eq!(parse_rss_kb("Name:\tbanc-agent\n"), None);
    }

    #[test]
    fn temps_cpu() {
        let stat = "4242 (banc agent (x)) S 1 4242 4242 0 -1 4194304 150 0 0 0 7 3 0 0 20 0 2 0 123456 1000000 700\n";
        assert_eq!(parse_cpu_ticks(stat), Some(10));
        assert_eq!(parse_cpu_ticks("4242 (banc-agent) S 1 2"), None);
        assert_eq!(parse_cpu_ticks(""), None);
    }

    #[test]
    fn tics_admis() {
        assert_eq!(allowed_ticks(10, 100), 1);
        assert_eq!(allowed_ticks(60, 100), 6);
        assert_eq!(allowed_ticks(5, 100), 0);
    }
}
