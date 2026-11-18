//! Server side of the control channel (`banc_proto::control`).
//!
//! Only enrolled, non-revoked machines are served: the iroh handshake proves
//! the machine key, the database says whether it belongs to the team.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use banc_proto::control::{self, close, AgentMessage, ServerMessage, HEARTBEAT_INTERVAL};
use banc_proto::keys::PublicKey;
use iroh::endpoint::{Connection, RecvStream, SendStream};
use iroh::protocol::{AcceptError, ProtocolHandler};
use tokio::sync::watch;

use crate::state::AppState;
use crate::store;

/// Time allowed for the agent to open its stream and say hello.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

/// A machine that misses three heartbeats in a row is disconnected.
const SILENCE_LIMIT: Duration = Duration::from_secs(HEARTBEAT_INTERVAL.as_secs() * 3);

/// The open control connections, by machine key.
#[derive(Debug, Default)]
pub struct Registry {
    connections: Mutex<HashMap<PublicKey, Connection>>,
}

impl Registry {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<PublicKey, Connection>> {
        self.connections
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Records a connection. An older connection of the same machine is
    /// closed: an agent only keeps one.
    fn register(&self, key: PublicKey, connection: Connection) {
        if let Some(old) = self.lock().insert(key, connection) {
            old.close(close::PROTOCOL.into(), b"replaced by a new connection");
        }
    }

    /// Forgets a connection, unless it has already been replaced.
    fn unregister(&self, key: &PublicKey, stable_id: usize) {
        let mut connections = self.lock();
        if connections
            .get(key)
            .is_some_and(|connection| connection.stable_id() == stable_id)
        {
            connections.remove(key);
        }
    }

    pub fn is_connected(&self, key: &PublicKey) -> bool {
        self.lock().contains_key(key)
    }

    /// Cuts the connection of a revoked machine.
    pub fn disconnect(&self, key: &PublicKey) {
        if let Some(connection) = self.lock().remove(key) {
            connection.close(close::REVOKED.into(), b"machine revoked");
        }
    }

    pub fn close_all(&self) {
        for (_, connection) in self.lock().drain() {
            connection.close(close::SHUTDOWN.into(), b"server shutting down");
        }
    }
}

#[derive(Clone)]
pub struct ControlProtocol {
    state: Arc<AppState>,
}

impl ControlProtocol {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }
}

impl std::fmt::Debug for ControlProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ControlProtocol")
    }
}

impl ProtocolHandler for ControlProtocol {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        serve(&self.state, connection).await;
        Ok(())
    }
}

/// Why a control connection ended.
#[derive(Debug)]
enum End {
    /// The agent closed it, or the network dropped.
    Closed,
    /// Closed by the server with this code and reason.
    Refused(u32, &'static str),
}

async fn serve(state: &AppState, connection: Connection) {
    let key = connection.remote_id();
    let machine = match store::active_machine(&state.db(), &key) {
        Ok(Some(machine)) => machine,
        Ok(None) => {
            tracing::info!(key = %key.fmt_short(), "control connection refused: unknown machine");
            connection.close(close::UNKNOWN_MACHINE.into(), b"unknown machine");
            return;
        }
        Err(error) => {
            tracing::error!("control connection: database error: {error:#}");
            connection.close(close::SHUTDOWN.into(), b"server error");
            return;
        }
    };

    // Before the welcome: a manifest published from now on is announced.
    let manifests = state.manifest_version.subscribe();
    match handshake(state, &connection, machine.id).await {
        Ok((send, mut recv)) => {
            state.connections.register(key, connection.clone());
            tracing::info!(machine = machine.id, name = %machine.name, "machine connected");
            let end = tokio::select! {
                end = follow(state, &mut recv, machine.id) => end,
                () = announce(send, manifests) => End::Closed,
            };
            state.connections.unregister(&key, connection.stable_id());
            if let End::Refused(code, reason) = end {
                connection.close(code.into(), reason.as_bytes());
            }
            tracing::info!(machine = machine.id, ?end, "machine disconnected");
        }
        Err(End::Refused(code, reason)) => {
            tracing::info!(machine = machine.id, reason, "control handshake failed");
            connection.close(code.into(), reason.as_bytes());
        }
        Err(End::Closed) => {}
    }
}

/// The agent's stream and hello, then the server's welcome.
async fn handshake(
    state: &AppState,
    connection: &Connection,
    machine_id: i64,
) -> Result<(SendStream, RecvStream), End> {
    let timed_out = End::Refused(close::PROTOCOL, "handshake timed out");
    let (mut send, mut recv) = tokio::time::timeout(HANDSHAKE_TIMEOUT, connection.accept_bi())
        .await
        .map_err(|_| End::Refused(close::PROTOCOL, "handshake timed out"))?
        .map_err(|_| End::Closed)?;
    let hello = tokio::time::timeout(HANDSHAKE_TIMEOUT, control::read_frame(&mut recv))
        .await
        .map_err(|_| timed_out)?;
    let (agent_version, inventory) = match hello {
        Ok(Some(AgentMessage::Hello {
            agent_version,
            inventory,
        })) => (agent_version, inventory),
        Ok(None) => return Err(End::Closed),
        Ok(Some(_)) | Err(_) => return Err(End::Refused(close::PROTOCOL, "hello expected")),
    };
    let recorded = store::record_hello(
        &mut state.db(),
        machine_id,
        &agent_version,
        &inventory,
        state.now(),
    );
    if let Err(error) = recorded {
        tracing::error!(machine = machine_id, "recording the hello: {error:#}");
        return Err(End::Refused(close::SHUTDOWN, "server error"));
    }
    let welcome = ServerMessage::Welcome {
        machine_id,
        heartbeat_secs: HEARTBEAT_INTERVAL.as_secs(),
        packages: true,
    };
    control::write_frame(&mut send, &welcome)
        .await
        .map_err(|_| End::Closed)?;
    Ok((send, recv))
}

/// Tells the agent about each new manifest, until the connection is cut.
async fn announce(mut send: SendStream, mut manifests: watch::Receiver<u64>) {
    while manifests.changed().await.is_ok() {
        let version = *manifests.borrow_and_update();
        let notice = ServerMessage::ManifestPublished { version };
        if control::write_frame(&mut send, &notice).await.is_err() {
            return;
        }
    }
    // The server is stopping: `follow` ends with the connection.
    std::future::pending::<()>().await;
}

/// Heartbeats and reports until the connection ends.
async fn follow(state: &AppState, recv: &mut RecvStream, machine_id: i64) -> End {
    loop {
        let message = match tokio::time::timeout(SILENCE_LIMIT, control::read_frame(recv)).await {
            Err(_) => return End::Refused(close::PROTOCOL, "no heartbeat"),
            Ok(Ok(Some(message))) => message,
            Ok(Ok(None)) => return End::Closed,
            Ok(Err(control::FrameError::Io(_))) => return End::Closed,
            Ok(Err(_)) => return End::Refused(close::PROTOCOL, "malformed message"),
        };
        let now = state.now();
        let result = match &message {
            AgentMessage::Heartbeat => store::touch_machine(&state.db(), machine_id, now),
            AgentMessage::Report { inventory } => {
                store::record_report(&mut state.db(), machine_id, inventory, now)
            }
            AgentMessage::Packages { report } => {
                store::record_packages(&state.db(), machine_id, report, now)
            }
            AgentMessage::Hello { .. } => return End::Refused(close::PROTOCOL, "second hello"),
        };
        if let Err(error) = result {
            tracing::error!(
                machine = machine_id,
                "recording a control message: {error:#}"
            );
        }
    }
}
