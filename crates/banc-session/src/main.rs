//! Banc session helper: system tray icon and Resolve bridge.
//!
//! Phase 0: the helper opens its logs and exits. The icon and its event loop
//! (`tray-icon` + `tao`) arrive in phase 1.

mod args;

use std::io::Write;
use std::process::ExitCode;

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
        None => paths::log_dir(Scope::User)?,
    };
    let mut log = LogConfig::new(log_dir, NAME);
    log.stderr = options.stderr;
    let _log_guard = logging::init(&log)?;

    tracing::info!(version = VERSION, "session helper started");
    tracing::info!("nothing to do before phase 1, exiting");
    Ok(())
}

fn print(text: &str) -> anyhow::Result<()> {
    std::io::stdout().write_all(text.as_bytes())?;
    Ok(())
}
