//! The team's iroh relay, run inside the server process.
//!
//! Only the server itself and enrolled, non-revoked machines may use it:
//! the relay carries the team's traffic and nobody else's.

use std::net::SocketAddr;
use std::sync::Arc;

use banc_proto::keys::PublicKey;
use iroh_relay::server::{Access, AccessControl, ClientRequest, RelayConfig, Server, ServerConfig};

use crate::state::AppState;
use crate::store;

pub struct TeamAccess {
    state: Arc<AppState>,
    server_id: PublicKey,
}

impl std::fmt::Debug for TeamAccess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TeamAccess")
    }
}

impl TeamAccess {
    fn decide(&self, endpoint: &PublicKey) -> Access {
        if *endpoint == self.server_id {
            return Access::Allow;
        }
        match store::active_machine(&self.state.db(), endpoint) {
            Ok(Some(_)) => Access::Allow,
            Ok(None) => {
                tracing::debug!(key = %endpoint.fmt_short(), "relay refused: unknown machine");
                Access::Deny {
                    reason: Some("not a member of this team".to_owned()),
                }
            }
            Err(error) => {
                tracing::error!("relay access check: {error:#}");
                Access::Deny { reason: None }
            }
        }
    }
}

impl AccessControl for TeamAccess {
    async fn on_connect(&self, request: &ClientRequest) -> Access {
        self.decide(&request.endpoint_id())
    }
}

/// Starts the relay on `listen`, plain HTTP: TLS is terminated in front.
pub async fn spawn(
    listen: SocketAddr,
    state: Arc<AppState>,
    server_id: PublicKey,
) -> anyhow::Result<Server> {
    let mut relay = RelayConfig::new(listen);
    relay.access = Arc::new(TeamAccess { state, server_id });
    let mut config = ServerConfig::default();
    config.relay = Some(relay);
    Ok(Server::spawn(config).await?)
}
