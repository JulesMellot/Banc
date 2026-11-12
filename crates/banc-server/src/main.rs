//! Banc server.
//!
//! Phase 0: the server reads its configuration, opens its logs and its
//! database, then waits for shutdown. The API arrives in phase 1.

mod args;

use std::io::Write;
use std::process::ExitCode;

use anyhow::Context;
use banc_platform::logging::{self, LogConfig};
use banc_platform::paths::{self, Scope};
use banc_server::config::Config;
use banc_server::db;

const NAME: &str = env!("CARGO_PKG_NAME");
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // The logs are not necessarily open: the error also goes to
            // standard error, which the service manager reads.
            tracing::error!("arrêt sur erreur : {error:#}");
            let _ = writeln!(std::io::stderr(), "{NAME} : {error:#}");
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

    let database = config.database_path();
    let conn = db::open(&database)
        .with_context(|| format!("ouverture de la base {}", database.display()))?;
    tracing::info!(version = VERSION, base = %database.display(), "serveur démarré");

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("démarrage du runtime")?;
    runtime
        .block_on(banc_platform::signal::shutdown())
        .context("attente du signal d'arrêt")?;

    drop(conn);
    tracing::info!("serveur arrêté");
    Ok(())
}

fn print(text: &str) -> anyhow::Result<()> {
    std::io::stdout().write_all(text.as_bytes())?;
    Ok(())
}
