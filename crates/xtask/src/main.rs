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
#[command(name = "cargo xtask", about = "Banc development commands")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Runs the CI checks (all of them, or only the steps listed)
    Check {
        /// Steps to run, in the given order
        steps: Vec<check::Step>,
        /// Starting git reference for the `commits` and `secrets` steps
        #[arg(long, default_value = "origin/main")]
        base: String,
    },
    /// Checks a commit message in a file (commit-msg hook)
    CommitMsg { file: PathBuf },
    /// Checks a PR title
    PrTitle { title: String },
    /// Rewrites migrations/schema.sql from the migrations
    Schema,
    /// Enables the repository's git hooks (.githooks)
    Hooks,
    /// Runs the server with dev/server.toml
    DevServer,
    /// Runs the agent in the foreground, logs in .dev/
    DevAgent,
    /// Runs the frontend in development mode
    DevUi,
    /// Builds the release binaries into dist/<target>/
    Dist {
        /// Rust target (repeatable). Default: the current machine
        #[arg(long = "target")]
        targets: Vec<String>,
    },
    /// Measures the agent's memory and CPU at rest (Linux)
    IdleBench {
        /// Measurement duration
        #[arg(long, default_value_t = 10)]
        seconds: u64,
    },
}

fn main() -> ExitCode {
    match execute() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            say(&format!("error: {error:#}"));
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
                .with_context(|| format!("reading {}", file.display()))?;
            report("commit message", commits::check_message(&message))
        }
        Command::PrTitle { title } => report("PR title", commits::check_subject(&title)),
        Command::Schema => {
            let mut command = cmd::cargo(&["test", "-p", "banc-server", "--test", "migrations"]);
            command.env("BANC_WRITE_SCHEMA", "1");
            run(command.current_dir(&root))?;
            say("migrations/schema.sql rewritten, review the diff");
            Ok(())
        }
        Command::Hooks => {
            let mut command = cmd::tool("git");
            command.args(["config", "core.hooksPath", ".githooks"]);
            run(command.current_dir(&root))?;
            say("git hooks enabled (.githooks)");
            Ok(())
        }
        Command::DevServer => {
            let args = ["run", "-p", "banc-server", "--"];
            let mut command = cmd::cargo(&args);
            command.args(["--config", "dev/server.toml", "--stderr"]);
            run(command.current_dir(&root))
        }
        Command::DevAgent => {
            let mut command = cmd::cargo(&["run", "-p", "banc-agent", "--"]);
            command.args(["--log-dir", ".dev/logs", "--stderr"]);
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
    anyhow::bail!("{what} refused")
}
