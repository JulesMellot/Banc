//! State shared by the HTTP API, the control channel and the relay.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use banc_proto::enroll::ServerInfo;
use rusqlite::Connection;

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
    /// Base URL written into invitation links.
    pub public_url: String,
    /// How agents reach the server over iroh.
    pub server_info: ServerInfo,
    /// One-time token that allows creating the first admin, `None` once an
    /// admin exists.
    pub setup: Mutex<Option<SetupToken>>,
    /// Open control connections.
    pub connections: Registry,
    /// Built web admin, served on every path outside the API.
    pub admin_ui: Option<PathBuf>,
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
        public_url: String,
        server_info: ServerInfo,
        admin_ui: Option<PathBuf>,
    ) -> Self {
        Self {
            db: Mutex::new(db),
            clock,
            public_url,
            server_info,
            setup: Mutex::new(None),
            connections: Registry::default(),
            admin_ui,
        }
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
