//! Banc server.
//!
//! Reads its configuration, opens its logs, starts the API, the iroh
//! endpoint and the relay, then runs until the shutdown signal.

mod args;

use std::io::Write;
use std::process::ExitCode;

use anyhow::Context;
use banc_platform::logging::{self, LogConfig};
use banc_platform::paths::{self, Scope};
use banc_server::config::Config;
use banc_server::{server, state};

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

    let config = Config::load(&options.config)?;
    let log_dir = match &config.logs.dir {
        Some(dir) => dir.clone(),
        None => paths::log_dir(Scope::System)?,
    };
    let mut log = LogConfig::new(log_dir, NAME);
    log.level = config.logs.level.into();
    log.keep_files = usize::from(config.logs.keep_days);
    log.stderr = options.stderr;
    let _log_guard = logging::init(&log)?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .context("starting the runtime")?;
    runtime.block_on(async {
        let running = server::start(&config, state::system_clock()).await?;
        tracing::info!(version = VERSION, "server started");
        banc_platform::signal::shutdown()
            .await
            .context("waiting for the shutdown signal")?;
        tracing::info!("stopping");
        running.shutdown().await
    })?;
    tracing::info!("server stopped");
    Ok(())
}

fn print(text: &str) -> anyhow::Result<()> {
    std::io::stdout().write_all(text.as_bytes())?;
    Ok(())
}
