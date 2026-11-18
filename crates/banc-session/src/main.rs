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
use std::time::{Duration, Instant, SystemTime};

use anyhow::Context;
use banc_platform::logging::{self, LogConfig};
use banc_platform::paths::{self, Scope};
use banc_platform::tray::{self, Tray, TrayError};
use banc_proto::control;
use banc_proto::local::{AgentStatus, Request, Response};

use crate::text::Text;

const NAME: &str = env!("CARGO_PKG_NAME");
const VERSION: &str = banc_proto::VERSION;
/// How often the icon asks the agent for its state.
const REFRESH: Duration = Duration::from_secs(60);
/// How long a new helper waits for the previous one to end.
const LOCK_WAIT: Duration = Duration::from_secs(5);
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
    let Some(_instance) = single_instance()? else {
        tracing::debug!("the session helper is already running");
        return Ok(());
    };
    tracing::info!(version = VERSION, "session helper started");

    let socket = options.socket.unwrap_or_else(paths::agent_socket);
    let text = Text::load();
    let icon = Tray {
        icon_rgba: ICON.to_vec(),
        icon_size: 32,
        open_label: text.get("tray.open"),
        quit_label: text.get("tray.quit"),
    };
    let started_from = program_date();
    let status = move || {
        restart_if_updated(started_from);
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

/// One tray icon per session. `None` if another helper holds the lock. The
/// operating system releases it when the process ends, however it ends.
fn single_instance() -> anyhow::Result<Option<std::fs::File>> {
    let dir = paths::state_dir(Scope::User)?;
    std::fs::create_dir_all(&dir).context("creating the data folder")?;
    let file = std::fs::File::create(dir.join("session.lock")).context("creating the lock")?;
    let start = Instant::now();
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(Some(file)),
            Err(std::fs::TryLockError::WouldBlock) if start.elapsed() < LOCK_WAIT => {
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(std::fs::TryLockError::WouldBlock) => return Ok(None),
            Err(std::fs::TryLockError::Error(error)) => {
                return Err(error).context("locking the session helper");
            }
        }
    }
}

/// When the program on disk was written.
fn program_date() -> Option<SystemTime> {
    std::fs::metadata(std::env::current_exe().ok()?)
        .ok()?
        .modified()
        .ok()
}

/// An update replaced the program on disk: starts the new one.
fn restart_if_updated(started_from: Option<SystemTime>) {
    let Some(now) = program_date() else {
        // Being replaced right now: seen at the next refresh.
        return;
    };
    if started_from.is_some_and(|date| date != now) {
        tracing::info!("the session helper was updated, restarting");
        if let Err(error) = banc_platform::process::restart() {
            tracing::error!("restarting the session helper: {error}");
        }
    }
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
