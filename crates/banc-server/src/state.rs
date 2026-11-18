//! State shared by the HTTP API, the control channel and the relay.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, RwLock};

use banc_proto::enroll::ServerInfo;
use rusqlite::Connection;
use tokio::sync::watch;

use crate::control::Registry;

/// Current time in Unix seconds. Replaced in tests.
pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

pub fn system_clock() -> Clock {
    Arc::new(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| {
                i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
            })
    })
}

pub struct AppState {
    db: Mutex<Connection>,
    clock: Clock,
    /// Base URL written into invitation links, and how agents reach the
    /// server over iroh.
    public: RwLock<Public>,
    /// The relay is served at the public address.
    relay_follows_public_url: bool,
    /// One-time token that allows creating the first admin, `None` once an
    /// admin exists.
    pub setup: Mutex<Option<SetupToken>>,
    /// Open control connections.
    pub connections: Registry,
    /// Built web admin, served on every path outside the API.
    pub admin_ui: Option<PathBuf>,
    /// Local address of the relay, once it has started.
    pub relay_addr: OnceLock<SocketAddr>,
    /// Where the content of the package files is kept.
    pub packages_dir: PathBuf,
    /// Version of the latest manifest: connected agents follow it.
    pub manifest_version: watch::Sender<u64>,
}

struct Public {
    url: Option<String>,
    info: ServerInfo,
}

/// The first-admin token and the file it was written to.
pub struct SetupToken {
    pub hash: [u8; 32],
    pub file: Option<PathBuf>,
}

impl AppState {
    pub fn new(
        db: Connection,
        clock: Clock,
        public_url: Option<String>,
        server_info: ServerInfo,
        relay_follows_public_url: bool,
        admin_ui: Option<PathBuf>,
        packages_dir: PathBuf,
    ) -> Self {
        let latest = crate::store::latest_manifest(&db)
            .ok()
            .flatten()
            .map_or(0, |manifest| manifest.version);
        Self {
            db: Mutex::new(db),
            clock,
            public: RwLock::new(Public {
                url: public_url,
                info: server_info,
            }),
            relay_follows_public_url,
            setup: Mutex::new(None),
            connections: Registry::default(),
            admin_ui,
            relay_addr: OnceLock::new(),
            packages_dir,
            manifest_version: watch::Sender::new(latest),
        }
    }

    /// The server's public address, `None` until it is known.
    pub fn public_url(&self) -> Option<String> {
        self.public
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .url
            .clone()
    }

    pub fn server_info(&self) -> ServerInfo {
        self.public
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .info
            .clone()
    }

    /// Starts using a public address learned while running.
    pub fn adopt_public_url(&self, url: String) {
        let mut public = self
            .public
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if self.relay_follows_public_url {
            public.info.relay_url = Some(url.clone());
        }
        public.url = Some(url);
    }

    pub fn now(&self) -> i64 {
        (self.clock)()
    }

    /// The database. A thread that panicked while holding it leaves SQLite
    /// in a consistent state (transactions roll back on drop), so the lock
    /// is taken back.
    pub fn db(&self) -> MutexGuard<'_, Connection> {
        self.db
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn setup(&self) -> MutexGuard<'_, Option<SetupToken>> {
        self.setup
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
