//! Starts and stops the server's parts: HTTP API, iroh endpoint with the
//! control channel, relay.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::Context;
use banc_proto::control;
use banc_proto::enroll::ServerInfo;
use banc_proto::keys::{PublicKey, SecretKey};
use iroh::endpoint::presets;
use iroh::protocol::Router as IrohRouter;
use iroh::{Endpoint, RelayMap, RelayMode, RelayUrl};

use crate::auth;
use crate::config::Config;
use crate::control::ControlProtocol;
use crate::state::{AppState, Clock, SetupToken};
use crate::{api, db, relay, store};

const VERSION: &str = env!("CARGO_PKG_VERSION");
/// File where the first-admin token is written, in `data_dir`.
const SETUP_TOKEN_FILE: &str = "setup-token";

/// A started server.
pub struct Running {
    pub state: Arc<AppState>,
    pub http_addr: SocketAddr,
    pub endpoint_id: PublicKey,
    /// Local address of the relay, `None` when it is disabled.
    pub relay_addr: Option<SocketAddr>,
    http: tokio::task::JoinHandle<std::io::Result<()>>,
    http_stop: tokio::sync::oneshot::Sender<()>,
    iroh: IrohRouter,
    relay: Option<iroh_relay::server::Server>,
}

/// Starts everything. Listening addresses with port 0 get a free port,
/// for tests.
pub async fn start(config: &Config, clock: Clock) -> anyhow::Result<Running> {
    let database = config.database_path();
    let conn = db::open(&database)
        .with_context(|| format!("opening the database {}", database.display()))?;
    let secret = load_or_create_key(&config.server_key_path())?;
    let endpoint_id = secret.public();

    // The relay starts first: the endpoint connects to it.
    let relay_listener = if config.relay.enabled {
        Some(config.relay.listen)
    } else {
        None
    };

    // The configuration wins over an address learned earlier.
    let public_url = match &config.public_url {
        Some(url) => Some(url.clone()),
        None => store::public_url(&conn)?,
    };
    let relay_follows_public_url = config.relay.enabled && config.relay.url.is_none();
    let relay_url = config.relay_url().or_else(|| {
        relay_follows_public_url
            .then(|| public_url.clone())
            .flatten()
    });
    let server_info = ServerInfo {
        version: VERSION.to_owned(),
        endpoint_id,
        relay_url,
        direct_addrs: config.iroh.public_addrs.clone(),
    };
    let state = Arc::new(AppState::new(
        conn,
        clock,
        public_url,
        server_info,
        relay_follows_public_url,
        config.http.admin_ui.clone(),
    ));
    prepare_setup(&state, &config.data_dir)?;

    let relay = match relay_listener {
        Some(listen) => Some(
            relay::spawn(listen, state.clone(), endpoint_id)
                .await
                .context("starting the relay")?,
        ),
        None => None,
    };
    let relay_addr = relay.as_ref().and_then(|relay| relay.http_addr());
    if let Some(addr) = relay_addr {
        // Set once, on a state nobody else holds yet.
        let _ = state.relay_addr.set(addr);
    }

    // The server reaches its own relay locally; agents use the public URL.
    // Both lead to the same relay process.
    let relay_mode = match relay_addr {
        Some(addr) => {
            let url = RelayUrl::from_str(&local_relay_url(addr)).context("local relay address")?;
            RelayMode::Custom(RelayMap::from(url))
        }
        None => RelayMode::Disabled,
    };
    let endpoint = Endpoint::builder(presets::Minimal)
        .secret_key(secret)
        .alpns(vec![control::ALPN.to_vec()])
        .relay_mode(relay_mode)
        .clear_ip_transports()
        .bind_addr(config.iroh.listen)
        .context("iroh address")?
        .bind()
        .await
        .context("starting the iroh endpoint")?;
    let iroh = IrohRouter::builder(endpoint)
        .accept(control::ALPN, ControlProtocol::new(state.clone()))
        .spawn();

    let listener = tokio::net::TcpListener::bind(config.http.listen)
        .await
        .with_context(|| format!("listening on {}", config.http.listen))?;
    let http_addr = listener.local_addr()?;
    let (http_stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let app = api::router(state.clone());
    let http = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
    });

    tracing::info!(
        http = %http_addr,
        iroh = ?iroh.endpoint().bound_sockets(),
        relay = ?relay_addr,
        endpoint_id = %endpoint_id,
        "server listening"
    );
    Ok(Running {
        state,
        http_addr,
        endpoint_id,
        relay_addr,
        http,
        http_stop,
        iroh,
        relay,
    })
}

impl Running {
    /// Addresses of the iroh endpoint, for tests that connect directly.
    pub fn iroh_addrs(&self) -> Vec<SocketAddr> {
        self.iroh.endpoint().bound_sockets()
    }

    pub async fn shutdown(self) -> anyhow::Result<()> {
        let _ = self.http_stop.send(());
        self.state.connections.close_all();
        self.iroh
            .shutdown()
            .await
            .map_err(|e| anyhow::anyhow!("stopping the iroh endpoint: {e}"))?;
        if let Some(relay) = self.relay {
            relay
                .shutdown()
                .await
                .map_err(|e| anyhow::anyhow!("stopping the relay: {e}"))?;
        }
        self.http
            .await
            .context("HTTP task")?
            .context("HTTP server")?;
        Ok(())
    }
}

/// Where the server reaches its own relay. A relay listening on every
/// interface, as in a container, is reached over the loopback.
fn local_relay_url(addr: SocketAddr) -> String {
    let ip = match addr.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        ip => ip,
    };
    format!("http://{}", SocketAddr::new(ip, addr.port()))
}

/// The server's iroh key, created at the first start. The VPS has no
/// keychain: the key lives in a file readable by the server's account only.
fn load_or_create_key(path: &Path) -> anyhow::Result<SecretKey> {
    match std::fs::read_to_string(path) {
        Ok(text) => SecretKey::from_str(text.trim())
            .map_err(|e| anyhow::anyhow!("server key {} unreadable: {e}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let bytes: [u8; 32] = auth::random_bytes()?;
            let key = SecretKey::from_bytes(&bytes);
            let text = data_encoding::HEXLOWER.encode(&key.to_bytes());
            banc_platform::fs::write_private(path, text.as_bytes())
                .with_context(|| format!("writing the server key {}", path.display()))?;
            tracing::info!(file = %path.display(), "server key created");
            Ok(key)
        }
        Err(error) => {
            Err(error).with_context(|| format!("reading the server key {}", path.display()))
        }
    }
}

/// Without an admin, writes a one-time token for creating the first one.
/// The token goes to a file readable by the server's account only, never
/// to the logs.
fn prepare_setup(state: &AppState, data_dir: &Path) -> anyhow::Result<()> {
    if store::admin_count(&state.db())? > 0 {
        return Ok(());
    }
    let bytes: [u8; 24] = auth::random_bytes()?;
    let token = data_encoding::BASE32_NOPAD
        .encode(&bytes)
        .to_ascii_lowercase();
    let file = data_dir.join(SETUP_TOKEN_FILE);
    banc_platform::fs::write_private(&file, format!("{token}\n").as_bytes())
        .with_context(|| format!("writing {}", file.display()))?;
    *state.setup() = Some(SetupToken {
        hash: auth::token_hash(token.as_bytes()),
        file: Some(file.clone()),
    });
    tracing::warn!(
        file = %file.display(),
        url = state.public_url().as_deref().unwrap_or("not known yet"),
        "no admin yet: open the web admin and enter the token from this file"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_relay_address() {
        for (listen, url) in [
            ("127.0.0.1:3340", "http://127.0.0.1:3340"),
            ("0.0.0.0:3340", "http://127.0.0.1:3340"),
            ("[::]:3340", "http://[::1]:3340"),
            ("10.0.0.2:3340", "http://10.0.0.2:3340"),
        ] {
            assert_eq!(local_relay_url(listen.parse().unwrap()), url, "{listen}");
        }
    }
}
