//! Service système de Banc : installations, transferts, tunnel.
//!
//! Phase 0 : le service démarre, ouvre ses journaux et attend l'arrêt sans
//! rien faire. C'est l'état de repos que mesure `cargo xtask idle-bench`.

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

    let log_dir = match options.log_dir {
        Some(dir) => dir,
        None => paths::log_dir(Scope::System)?,
    };
    let mut log = LogConfig::new(log_dir, NAME);
    log.stderr = options.stderr;
    let _log_guard = logging::init(&log)?;

    // Un seul fil pour tout l'agent : c'est ce qui tient la cible de mémoire.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("démarrage du runtime")?;

    tracing::info!(version = VERSION, "agent démarré");
    runtime
        .block_on(banc_platform::signal::shutdown())
        .context("attente du signal d'arrêt")?;
    tracing::info!("agent arrêté");
    Ok(())
}

fn print(text: &str) -> anyhow::Result<()> {
    std::io::stdout().write_all(text.as_bytes())?;
    Ok(())
}
