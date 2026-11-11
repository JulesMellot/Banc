//! Serveur de Banc.
//!
//! Phase 0 : le serveur lit sa configuration, ouvre ses journaux et sa base,
//! puis attend l'arrêt. L'API arrive en phase 1.

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
            // Les journaux ne sont pas forcément ouverts : l'erreur va aussi
            // sur la sortie d'erreur, lue par le gestionnaire de services.
            tracing::error!("arrêt sur erreur : {error:#}");
            let _ = writeln!(std::io::stderr(), "{NAME} : {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> anyhow::Result<()> {
    // `lexopt` répète le message dans la cause de l'erreur : on ne garde
    // que le texte.
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
