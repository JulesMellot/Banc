//! The agent's local endpoint: requests from the interface.

use std::sync::Arc;

use banc_platform::local::{LocalListener, ServerStream};
use banc_proto::control;
use banc_proto::invite::Invite;
use banc_proto::local::{InviteInput, Request, Response};

use crate::enroll::{self, EnrollError};
use crate::Shared;

/// Answers requests until the listener fails.
pub async fn serve(mut listener: LocalListener, shared: Arc<Shared>) {
    loop {
        match listener.accept().await {
            Ok(stream) => {
                let shared = shared.clone();
                tokio::spawn(async move {
                    if let Err(error) = handle(stream, &shared).await {
                        tracing::debug!("local request: {error:#}");
                    }
                });
            }
            Err(error) => {
                tracing::error!("local endpoint: {error}");
                return;
            }
        }
    }
}

async fn handle(mut stream: ServerStream, shared: &Arc<Shared>) -> anyhow::Result<()> {
    let Some(request) = control::read_frame::<_, Request>(&mut stream).await? else {
        return Ok(());
    };
    let response = answer(request, shared).await;
    control::write_frame(&mut stream, &response).await?;
    Ok(())
}

fn error(code: &str, message: impl Into<String>) -> Response {
    Response::Error {
        code: code.to_owned(),
        message: message.into(),
    }
}

pub async fn answer(request: Request, shared: &Arc<Shared>) -> Response {
    match request {
        Request::Status => Response::Status(shared.status()),
        Request::Enroll { invite, name } => enroll_request(invite, name, shared).await,
    }
}

async fn enroll_request(
    input: InviteInput,
    name: Option<String>,
    shared: &Arc<Shared>,
) -> Response {
    let invite = match &input {
        InviteInput::Link { link } => Invite::parse_link(link),
        InviteInput::Key { server, key } => Invite::from_server_and_key(server, key),
    };
    let invite = match invite {
        Ok(invite) => invite,
        Err(e) => return error("invalid_invite", e.to_string()),
    };
    let name = name.unwrap_or_else(banc_platform::machine::hostname);
    let Some(name) = banc_proto::enroll::clean_name(&name).map(str::to_owned) else {
        return error("bad_name", "invalid machine name");
    };
    // One enrollment at a time, and only once.
    let Some(_guard) = shared.begin_enrollment() else {
        return error("busy", "an enrollment is already running");
    };
    if !shared.can_enroll() {
        return error("already_enrolled", "this machine already belongs to a team");
    }
    tracing::info!(server = %invite.server, name, "enrolling");
    let result = async {
        let client = enroll::http_client()?;
        enroll::enroll(&client, &invite, &name).await
    }
    .await;
    match result {
        Ok((key, answer)) => match shared.complete_enrollment(&invite.server, &name, key, answer) {
            Ok(()) => Response::Status(shared.status()),
            Err(e) => {
                tracing::error!("saving the enrollment: {e:#}");
                error("storage", format!("{e:#}"))
            }
        },
        Err(e) => {
            if matches!(e, EnrollError::AdminKeyMismatch) {
                tracing::error!(server = %invite.server, "enrollment refused: {e}");
            } else {
                tracing::info!("enrollment failed: {e}");
            }
            error(&e.code(), e.to_string())
        }
    }
}
