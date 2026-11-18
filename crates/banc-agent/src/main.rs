//! Banc system service: installs, transfers, tunnel.
//!
//! Without a command, runs the agent. The other commands talk to the
//! running agent through its local endpoint, or register the service.

mod args;

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::Context;
use banc_agent::update::{self, apply};
use banc_platform::logging::{self, LogConfig};
use banc_platform::paths::{self, Scope};
use banc_platform::secrets::SecretStore;
use banc_proto::control;
use banc_proto::local::{InviteInput, Request, Response};

const NAME: &str = env!("CARGO_PKG_NAME");
const VERSION: &str = banc_proto::VERSION;

fn main() -> ExitCode {
    match start() {
        Ok(0) => ExitCode::SUCCESS,
        Ok(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        Err(error) => {
            // The logs are not necessarily open: the error also goes to
            // standard error, which the service manager reads.
            tracing::error!("stopped on error: {error:#}");
            let _ = writeln!(std::io::stderr(), "{NAME}: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn start() -> anyhow::Result<u32> {
    // `lexopt` repeats the message in the error's cause: keep only the
    // text.
    let command = args::parse(lexopt::Parser::from_env()).map_err(|e| anyhow::anyhow!("{e}"))?;
    match command {
        args::Command::Run(options) if options.service => {
            let code = banc_platform::service::run(move || match run(options) {
                Ok(()) => 0,
                Err(error) => {
                    tracing::error!("stopped on error: {error:#}");
                    1
                }
            })?;
            Ok(code)
        }
        args::Command::Run(options) => run(options).map(|()| 0),
        args::Command::Enroll {
            invite,
            name,
            socket,
        } => {
            let invite = match invite {
                args::Invite::Link(link) => InviteInput::Link { link },
                args::Invite::Key { server, key } => InviteInput::Key { server, key },
            };
            ask(socket, Request::Enroll { invite, name })
        }
        args::Command::Status { socket } => ask(socket, Request::Status),
        args::Command::InstallService => {
            let program = std::env::current_exe().context("locating the agent binary")?;
            banc_platform::service::install(&program).context("installing the service")?;
            print("service installed and started\n")?;
            Ok(0)
        }
        args::Command::UninstallService => {
            banc_platform::service::uninstall().context("removing the service")?;
            print("service removed\n")?;
            Ok(0)
        }
        args::Command::ApplyUpdate(pending) => apply_update(&pending),
        args::Command::Help => print(args::HELP).map(|()| 0),
        args::Command::Version => print(&format!("{NAME} {VERSION}\n")).map(|()| 0),
    }
}

fn run(options: args::Options) -> anyhow::Result<()> {
    let log_dir = match options.log_dir {
        Some(dir) => dir,
        None => paths::log_dir(Scope::System)?,
    };
    let mut log = LogConfig::new(log_dir, NAME);
    log.stderr = options.stderr;
    let _log_guard = logging::init(&log)?;

    // A folder given on the command line holds the secrets too: for
    // development, without touching the system keychain.
    let (state_dir, secrets) = match options.state_dir {
        Some(dir) => (dir.clone(), SecretStore::in_dir(dir.join("secrets"))),
        None => (
            paths::state_dir(Scope::System)?,
            SecretStore::os(Scope::System)?,
        ),
    };
    let config = banc_agent::Config {
        state_dir,
        secrets,
        socket: options.socket.unwrap_or_else(paths::agent_socket),
        inventory: banc_agent::system_inventory(),
        places: banc_agent::system_places()?,
        resolve_open: std::sync::Arc::new(banc_platform::machine::resolve_running),
        package_retry: banc_agent::PACKAGE_RETRY,
    };

    // A single thread for the whole agent: this is what keeps the memory target.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("starting the runtime")?;

    tracing::info!(version = VERSION, "agent started");
    let updates = if options.service {
        keep_running();
        update::Settings::for_this_machine(config.state_dir.clone())?
    } else {
        None
    };
    runtime.block_on(async {
        if let Some(settings) = updates {
            tokio::spawn(update::watch(settings));
        }
        banc_agent::run(config, async {
            if let Err(error) = banc_platform::signal::shutdown().await {
                tracing::error!("waiting for the shutdown signal: {error}");
            }
        })
        .await
    })?;
    tracing::info!("agent stopped");
    Ok(())
}

/// Restart after a failure, tray icon at login. A machine works without
/// them: a problem here is only logged.
fn keep_running() {
    let session = std::env::current_exe().map(|program| {
        program.with_file_name(format!("banc-session{}", std::env::consts::EXE_SUFFIX))
    });
    match session {
        Ok(session) if session.exists() => {
            if let Err(error) = banc_platform::service::keep_running(&session) {
                tracing::warn!("setting up the automatic start: {error}");
            }
        }
        Ok(session) => tracing::debug!(session = %session.display(), "no session helper"),
        Err(error) => tracing::warn!("locating the session helper: {error}"),
    }
}

/// The service as an update sees it.
struct SystemService {
    socket: PathBuf,
    runtime: tokio::runtime::Runtime,
}

impl apply::Supervisor for SystemService {
    fn stop(&self) -> std::io::Result<()> {
        banc_platform::service::stop()
    }

    fn start(&self) -> std::io::Result<()> {
        banc_platform::service::start()
    }

    fn running_version(&self) -> Option<String> {
        self.runtime.block_on(async {
            let ask = async {
                let mut stream = banc_platform::local::connect(&self.socket).await.ok()?;
                control::write_frame(&mut stream, &Request::Status)
                    .await
                    .ok()?;
                match control::read_frame::<_, Response>(&mut stream)
                    .await
                    .ok()??
                {
                    Response::Status(status) => Some(status.version),
                    Response::Error { .. } => None,
                }
            };
            tokio::time::timeout(Duration::from_secs(5), ask)
                .await
                .ok()
                .flatten()
        })
    }

    fn wait(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// Runs from a copy of the old agent, started by the service.
fn apply_update(pending: &std::path::Path) -> anyhow::Result<u32> {
    let log = LogConfig::new(paths::log_dir(Scope::System)?, "banc-agent-update");
    let _log_guard = logging::init(&log)?;
    let text = std::fs::read(pending).context("reading the update")?;
    let pending: apply::Pending = serde_json::from_slice(&text).context("reading the update")?;
    tracing::info!(version = %pending.version, from = VERSION, "applying an update");
    let service = SystemService {
        socket: paths::agent_socket(),
        runtime: tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("starting the runtime")?,
    };
    let outcome = apply::apply(
        &pending,
        &apply::Work::for_root(&pending.root),
        &service,
        apply::Timing::REAL,
        &update::failed_file(&paths::state_dir(Scope::System)?),
    );
    tracing::info!(?outcome, "update finished");
    banc_platform::service::finish_updater();
    Ok(0)
}

/// Sends one request to the running agent and prints the answer.
fn ask(socket: Option<PathBuf>, request: Request) -> anyhow::Result<u32> {
    let socket = socket.unwrap_or_else(paths::agent_socket);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("starting the runtime")?;
    let response = runtime.block_on(async {
        let mut stream = banc_platform::local::connect(&socket)
            .await
            .with_context(|| format!("the agent does not answer on {}", socket.display()))?;
        control::write_frame(&mut stream, &request).await?;
        control::read_frame::<_, Response>(&mut stream)
            .await?
            .context("the agent closed the connection")
    })?;
    let failed = matches!(response, Response::Error { .. });
    print(&format!("{}\n", serde_json::to_string_pretty(&response)?))?;
    Ok(u32::from(failed))
}

fn print(text: &str) -> anyhow::Result<()> {
    std::io::stdout().write_all(text.as_bytes())?;
    Ok(())
}
