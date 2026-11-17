//! Talking to the agent over its local endpoint.

use std::path::PathBuf;

use banc_proto::control;
use banc_proto::local::{AgentStatus, InviteInput, Request, Response};

use crate::error::{CommandError, CommandResult};

/// The agent's endpoint: `BANC_AGENT_SOCKET` for development, the system
/// one otherwise.
fn endpoint() -> PathBuf {
    std::env::var_os("BANC_AGENT_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(banc_platform::paths::agent_socket)
}

async fn ask(request: &Request) -> CommandResult<Response> {
    let mut stream = banc_platform::local::connect(&endpoint())
        .await
        .map_err(|e| CommandError::new("agent_unreachable", e))?;
    control::write_frame(&mut stream, request)
        .await
        .map_err(|e| CommandError::new("agent_unreachable", e))?;
    control::read_frame::<_, Response>(&mut stream)
        .await
        .map_err(|e| CommandError::new("agent_unreachable", e))?
        .ok_or_else(|| CommandError::new("agent_unreachable", "the agent closed the connection"))
}

fn status_of(response: Response) -> CommandResult<AgentStatus> {
    match response {
        Response::Status(status) => Ok(status),
        Response::Error { code, message } => Err(CommandError { code, message }),
    }
}

/// The agent's state. `agent_unreachable` when the service is not installed
/// or not running.
#[tauri::command]
pub async fn agent_status() -> CommandResult<AgentStatus> {
    status_of(ask(&Request::Status).await?)
}

/// Joins a team. `link` or `server` and `key`.
#[tauri::command]
pub async fn enroll(
    link: Option<String>,
    server: Option<String>,
    key: Option<String>,
    name: Option<String>,
) -> CommandResult<AgentStatus> {
    let invite = match (link, server, key) {
        (Some(link), _, _) => InviteInput::Link { link },
        (None, Some(server), Some(key)) => InviteInput::Key { server, key },
        _ => {
            return Err(CommandError::new(
                "invalid_invite",
                "a link or a server and a key",
            ))
        }
    };
    let name = name.filter(|name| !name.trim().is_empty());
    status_of(ask(&Request::Enroll { invite, name }).await?)
}

/// Installs and starts the agent service, after the OS has asked for the
/// administrator password. The agent binary sits next to the application's.
#[tauri::command]
pub async fn install_agent() -> CommandResult<()> {
    let program = agent_binary()?;
    let result = tokio::task::spawn_blocking(move || {
        banc_platform::elevate::run(
            &program,
            &[
                std::ffi::OsStr::new("service"),
                std::ffi::OsStr::new("install"),
            ],
        )
    })
    .await
    .map_err(|e| CommandError::new("internal", e))?;
    result.map_err(|e| match e {
        banc_platform::elevate::ElevateError::Declined => CommandError::new("declined", e),
        other => CommandError::new("install_failed", other),
    })
}

/// `BANC_AGENT_BIN` for development, otherwise next to this executable.
fn agent_binary() -> CommandResult<PathBuf> {
    if let Some(path) = std::env::var_os("BANC_AGENT_BIN") {
        return Ok(PathBuf::from(path));
    }
    let exe = std::env::current_exe().map_err(|e| CommandError::new("internal", e))?;
    let name = format!("banc-agent{}", std::env::consts::EXE_SUFFIX);
    let path = exe
        .parent()
        .map(|dir| dir.join(&name))
        .ok_or_else(|| CommandError::new("agent_missing", "no folder for the application"))?;
    if path.exists() {
        Ok(path)
    } else {
        Err(CommandError::new("agent_missing", path.display()))
    }
}

/// The computer name, proposed as the machine name.
#[tauri::command]
pub fn hostname() -> String {
    banc_platform::machine::hostname()
}

/// What a link contains, shown before joining: the server and the admin key
/// fingerprint.
#[derive(serde::Serialize)]
pub struct LinkSummary {
    server: String,
    fingerprint: String,
}

#[tauri::command]
pub fn read_link(link: String) -> CommandResult<LinkSummary> {
    let invite = banc_proto::invite::Invite::parse_link(&link)
        .map_err(|e| CommandError::new("invalid_invite", e))?;
    Ok(LinkSummary {
        server: invite.server,
        fingerprint: invite.fingerprint.to_string(),
    })
}
