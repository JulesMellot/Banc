//! Banc system service: installs, transfers, tunnel.
//!
//! Without a command, runs the agent. The other commands talk to the
//! running agent through its local endpoint, or register the service.

mod args;

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context;
use banc_platform::logging::{self, LogConfig};
use banc_platform::paths::{self, Scope};
use banc_platform::secrets::SecretStore;
use banc_proto::control;
use banc_proto::local::{InviteInput, Request, Response};

const NAME: &str = env!("CARGO_PKG_NAME");
const VERSION: &str = env!("CARGO_PKG_VERSION");

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
    };

    // A single thread for the whole agent: this is what keeps the memory target.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("starting the runtime")?;

    tracing::info!(version = VERSION, "agent started");
    runtime.block_on(banc_agent::run(config, async {
        if let Err(error) = banc_platform::signal::shutdown().await {
            tracing::error!("waiting for the shutdown signal: {error}");
        }
    }))?;
    tracing::info!("agent stopped");
    Ok(())
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
