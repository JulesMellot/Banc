//! Banc system service: installs, transfers, tunnel.
//!
//! Phase 0: the service starts, opens its logs and waits for shutdown without
//! doing anything. This is the idle state that `cargo xtask idle-bench` measures.

mod args;

use std::io::Write;
use std::process::ExitCode;

use anyhow::Context;
use banc_platform::logging::{self, LogConfig};
use banc_platform::paths::{self, Scope};

const NAME: &str = env!("CARGO_PKG_NAME");
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // The logs are not necessarily open: the error also goes to
            // standard error, which the service manager reads.
            tracing::error!("stopped on error: {error:#}");
            let _ = writeln!(std::io::stderr(), "{NAME}: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> anyhow::Result<()> {
    // `lexopt` repeats the message in the error's cause: keep only the
    // text.
    let command = args::parse(lexopt::Parser::from_env()).map_err(|e| anyhow::anyhow!("{e}"))?;
    let options = match command {
        args::Command::Run(options) => options,
        args::Command::Help => return print(args::HELP),
        args::Command::Version => return print(&format!("{NAME} {VERSION}\n")),
    };

    let log_dir = match options.log_dir {
        Some(dir) => dir,
        None => paths::log_dir(Scope::System)?,
    };
    let mut log = LogConfig::new(log_dir, NAME);
    log.stderr = options.stderr;
    let _log_guard = logging::init(&log)?;

    // A single thread for the whole agent: this is what keeps the memory target.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("starting the runtime")?;

    tracing::info!(version = VERSION, "agent started");
    runtime
        .block_on(banc_platform::signal::shutdown())
        .context("waiting for the shutdown signal")?;
    tracing::info!("agent stopped");
    Ok(())
}

fn print(text: &str) -> anyhow::Result<()> {
    std::io::stdout().write_all(text.as_bytes())?;
    Ok(())
}
