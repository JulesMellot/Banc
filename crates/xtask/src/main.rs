//! Banc development commands: `cargo xtask <command>`.
//!
//! `check` runs the same checks as CI. The workflows call
//! `cargo xtask check <step>`, so that each check has a single definition.

mod architecture;
mod bench;
mod check;
mod cmd;
mod commits;
mod dist;

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context;
use clap::{Parser, Subcommand};

use crate::cmd::{root, run, say};

#[derive(Parser)]
#[command(name = "cargo xtask", about = "Commandes de développement de Banc")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Lance les contrôles de la CI (tous, ou seulement les étapes citées)
    Check {
        /// Étapes à lancer, dans l'ordre donné
        steps: Vec<check::Step>,
        /// Référence git de départ pour les étapes `commits` et `secrets`
        #[arg(long, default_value = "origin/main")]
        base: String,
    },
    /// Contrôle un message de commit dans un fichier (crochet commit-msg)
    CommitMsg { file: PathBuf },
    /// Contrôle le titre d'une PR
    PrTitle { title: String },
    /// Réécrit migrations/schema.sql à partir des migrations
    Schema,
    /// Active les crochets git du dépôt (.githooks)
    Hooks,
    /// Lance le serveur avec dev/serveur.toml
    DevServer,
    /// Lance l'agent au premier plan, journaux dans .dev/
    DevAgent,
    /// Lance le frontend en mode développement
    DevUi,
    /// Compile les binaires en mode release dans dist/<cible>/
    Dist {
        /// Cible Rust (répétable). Par défaut : la machine courante
        #[arg(long = "target")]
        targets: Vec<String>,
    },
    /// Mesure la mémoire et le CPU de l'agent au repos (Linux)
    IdleBench {
        /// Durée de la mesure
        #[arg(long, default_value_t = 10)]
        seconds: u64,
    },
}

fn main() -> ExitCode {
    match execute() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            say(&format!("erreur : {error:#}"));
            ExitCode::FAILURE
        }
    }
}

fn execute() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let root = root()?;
    match cli.command {
        Command::Check { steps, base } => check::run_steps(&root, &steps, &base),
        Command::CommitMsg { file } => {
            let message = std::fs::read_to_string(&file)
                .with_context(|| format!("lecture de {}", file.display()))?;
            report("message de commit", commits::check_message(&message))
        }
        Command::PrTitle { title } => report("titre de PR", commits::check_subject(&title)),
        Command::Schema => {
            let mut command = cmd::cargo(&["test", "-p", "banc-server", "--test", "migrations"]);
            command.env("BANC_WRITE_SCHEMA", "1");
            run(command.current_dir(&root))?;
            say("migrations/schema.sql réécrit, relis le diff");
            Ok(())
        }
        Command::Hooks => {
            let mut command = cmd::tool("git");
            command.args(["config", "core.hooksPath", ".githooks"]);
            run(command.current_dir(&root))?;
            say("crochets git activés (.githooks)");
            Ok(())
        }
        Command::DevServer => {
            let args = ["run", "-p", "banc-server", "--"];
            let mut command = cmd::cargo(&args);
            command.args(["--config", "dev/serveur.toml", "--stderr"]);
            run(command.current_dir(&root))
        }
        Command::DevAgent => {
            let mut command = cmd::cargo(&["run", "-p", "banc-agent", "--"]);
            command.args(["--log-dir", ".dev/journaux", "--stderr"]);
            run(command.current_dir(&root))
        }
        Command::DevUi => {
            let ui = root.join("ui");
            let mut install = cmd::tool("pnpm");
            run(install.arg("install").current_dir(&ui))?;
            let mut dev = cmd::tool("pnpm");
            run(dev.arg("dev").current_dir(&ui))
        }
        Command::Dist { targets } => dist::dist(&root, &targets),
        Command::IdleBench { seconds } => bench::idle_bench(&root, seconds),
    }
}

/// Prints the errors of a convention check and fails if there are any.
fn report(what: &str, errors: Vec<String>) -> anyhow::Result<()> {
    if errors.is_empty() {
        return Ok(());
    }
    for error in &errors {
        say(&format!("  - {error}"));
    }
    say(commits::HELP);
    anyhow::bail!("{what} refusé")
}
