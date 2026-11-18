//! The connection to the server, kept open while the agent runs.
//!
//! Hello with the inventory, then a heartbeat every interval asked by the
//! server, or a report when the inventory changed. Reconnects after a cut,
//! waiting longer each time, up to 5 minutes. Stops for good when the
//! server says the machine is revoked.

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use banc_proto::control::{self, close, AgentMessage, ServerMessage};
use banc_proto::local::Connection as ConnectionState;
use banc_proto::machine::Inventory;
use iroh::endpoint::{ConnectionError, Endpoint};
use iroh::{EndpointAddr, RelayUrl};

use crate::state::Enrolled;
use crate::Shared;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const FIRST_RETRY: Duration = Duration::from_secs(1);
const LAST_RETRY: Duration = Duration::from_secs(300);
/// A connection that lasted this long resets the waiting time.
const STABLE: Duration = Duration::from_secs(60);

/// How a session with the server ended.
#[derive(Debug, PartialEq, Eq)]
enum End {
    Closed,
    Revoked,
}

/// Keeps the connection up. Returns only when the machine is revoked.
pub async fn maintain(endpoint: &Endpoint, enrolled: &Enrolled, shared: &Arc<Shared>) {
    let addr = server_addr(enrolled);
    let mut wait = FIRST_RETRY;
    loop {
        shared.set_connection(ConnectionState::Connecting);
        let started = tokio::time::Instant::now();
        match session(endpoint, addr.clone(), shared).await {
            Ok(End::Revoked) => {
                tracing::warn!("the server no longer knows this machine: revoked");
                shared.set_connection(ConnectionState::Revoked);
                return;
            }
            Ok(End::Closed) => tracing::info!("connection to the server closed"),
            Err(error) => tracing::info!("connection to the server failed: {error:#}"),
        }
        shared.set_connection(ConnectionState::Connecting);
        if started.elapsed() >= STABLE {
            wait = FIRST_RETRY;
        }
        tokio::time::sleep(wait).await;
        wait = next_wait(wait);
    }
}

fn next_wait(wait: Duration) -> Duration {
    (wait * 2).min(LAST_RETRY)
}

/// The server's iroh address: its key, its relay, its direct addresses.
pub fn server_addr(enrolled: &Enrolled) -> EndpointAddr {
    let info = &enrolled.server_info;
    let mut addr = EndpointAddr::new(info.endpoint_id);
    if let Some(url) = info
        .relay_url
        .as_deref()
        .and_then(|url| RelayUrl::from_str(url).ok())
    {
        addr = addr.with_relay_url(url);
    }
    for socket in &info.direct_addrs {
        addr = addr.with_ip_addr(*socket);
    }
    addr
}

async fn session(
    endpoint: &Endpoint,
    addr: EndpointAddr,
    shared: &Arc<Shared>,
) -> anyhow::Result<End> {
    let connection = tokio::time::timeout(CONNECT_TIMEOUT, endpoint.connect(addr, control::ALPN))
        .await
        .map_err(|_| anyhow::anyhow!("timed out"))??;
    let (mut send, mut recv) = connection.open_bi().await?;
    let mut inventory = shared.inventory();
    let hello = AgentMessage::Hello {
        agent_version: banc_proto::VERSION.to_owned(),
        inventory: inventory.clone(),
    };
    control::write_frame(&mut send, &hello).await?;
    let welcome = tokio::select! {
        welcome = tokio::time::timeout(CONNECT_TIMEOUT, control::read_frame::<_, ServerMessage>(&mut recv)) => welcome,
        reason = connection.closed() => return Ok(end_of(&reason)),
    };
    let heartbeat = match welcome {
        Ok(Ok(Some(ServerMessage::Welcome {
            heartbeat_secs,
            machine_id,
        }))) => {
            tracing::info!(machine = machine_id, "connected to the server");
            Duration::from_secs(heartbeat_secs.clamp(10, 3600))
        }
        Ok(Ok(None)) => return Ok(end_of(&connection.closed().await)),
        Ok(Err(error)) => return Err(error.into()),
        Err(_) => anyhow::bail!("no welcome from the server"),
    };
    shared.set_connection(ConnectionState::Connected);

    let mut ticker = tokio::time::interval(heartbeat);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // The first tick is immediate: the hello already counts.
    ticker.tick().await;
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                let current = shared.inventory();
                let message = next_message(&inventory, &current);
                inventory = current;
                control::write_frame(&mut send, &message).await?;
            }
            reason = connection.closed() => return Ok(end_of(&reason)),
        }
    }
}

/// A report when something changed, a heartbeat otherwise.
fn next_message(previous: &Inventory, current: &Inventory) -> AgentMessage {
    if previous == current {
        AgentMessage::Heartbeat
    } else {
        AgentMessage::Report {
            inventory: current.clone(),
        }
    }
}

fn end_of(reason: &ConnectionError) -> End {
    match reason {
        ConnectionError::ApplicationClosed(closed) => {
            let code = closed.error_code.into_inner();
            if code == u64::from(close::REVOKED) || code == u64::from(close::UNKNOWN_MACHINE) {
                End::Revoked
            } else {
                End::Closed
            }
        }
        _ => End::Closed,
    }
}

#[cfg(test)]
mod tests {
    use banc_proto::machine::{ResolveEdition, ResolveInstall};

    use super::*;

    #[test]
    fn waiting_time_doubles_up_to_five_minutes() {
        let mut wait = FIRST_RETRY;
        let mut waits = Vec::new();
        for _ in 0..12 {
            waits.push(wait.as_secs());
            wait = next_wait(wait);
        }
        assert_eq!(waits, [1, 2, 4, 8, 16, 32, 64, 128, 256, 300, 300, 300]);
    }

    #[test]
    fn report_only_when_the_inventory_changes() {
        let before = Inventory::default();
        assert_eq!(next_message(&before, &before), AgentMessage::Heartbeat);
        let after = Inventory {
            resolve: Some(ResolveInstall {
                edition: ResolveEdition::Studio,
                version: "21.1".to_owned(),
            }),
            ..Inventory::default()
        };
        assert_eq!(
            next_message(&before, &after),
            AgentMessage::Report {
                inventory: after.clone()
            }
        );
    }
}
