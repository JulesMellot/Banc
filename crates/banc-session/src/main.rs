//! Banc session helper: system tray icon and Resolve bridge.
//!
//! Phase 1: the tray icon on macOS and Windows, with the agent's state and
//! a way to open the window. On Linux there is no tray icon yet: the helper
//! waits for the end of the session. The Resolve bridge arrives in phase 4.

mod args;
mod text;

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::Context;
use banc_platform::logging::{self, LogConfig};
use banc_platform::paths::{self, Scope};
use banc_platform::tray::{self, Tray, TrayError};
use banc_proto::control;
use banc_proto::local::{AgentStatus, Request, Response};

use crate::text::Text;

const NAME: &str = env!("CARGO_PKG_NAME");
const VERSION: &str = env!("CARGO_PKG_VERSION");
/// How often the icon asks the agent for its state.
const REFRESH: Duration = Duration::from_secs(60);
/// The tray icon, 32×32 RGBA.
static ICON: &[u8] = include_bytes!("../icons/tray-32.rgba");

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

    let socket = options.socket.unwrap_or_else(paths::agent_socket);
    let text = Text::load();
    let icon = Tray {
        icon_rgba: ICON.to_vec(),
        icon_size: 32,
        open_label: text.get("tray.open"),
        quit_label: text.get("tray.quit"),
    };
    let status = move || {
        let socket = socket.clone();
        let text = Text::load();
        async move { text.status_line(agent_status(&socket).await.as_ref()) }
    };
    match tray::run(icon, REFRESH, status, open_window) {
        Ok(()) => {}
        Err(TrayError::Unsupported) => {
            tracing::info!("no tray icon on this system, waiting for the end of the session");
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .context("starting the runtime")?;
            runtime
                .block_on(banc_platform::signal::shutdown())
                .context("waiting for the shutdown signal")?;
        }
        Err(error) => return Err(error.into()),
    }
    tracing::info!("session helper stopped");
    Ok(())
}

/// The agent's state, `None` if it does not answer.
async fn agent_status(socket: &std::path::Path) -> Option<AgentStatus> {
    let mut stream = banc_platform::local::connect(socket).await.ok()?;
    control::write_frame(&mut stream, &Request::Status)
        .await
        .ok()?;
    match control::read_frame::<_, Response>(&mut stream)
        .await
        .ok()??
    {
        Response::Status(status) => Some(status),
        Response::Error { .. } => None,
    }
}

/// Opens the window: `BANC_APP_BIN`, or the application next to this
/// program.
fn open_window() {
    let program = std::env::var_os("BANC_APP_BIN")
        .map(PathBuf::from)
        .or_else(|| {
            let exe = std::env::current_exe().ok()?;
            Some(
                exe.parent()?
                    .join(format!("banc-app{}", std::env::consts::EXE_SUFFIX)),
            )
        });
    let Some(program) = program else {
        return;
    };
    if let Err(error) = std::process::Command::new(&program).spawn() {
        tracing::error!(program = %program.display(), "cannot open the window: {error}");
    }
}

fn print(text: &str) -> anyhow::Result<()> {
    std::io::stdout().write_all(text.as_bytes())?;
    Ok(())
}
