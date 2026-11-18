//! Banc system service: installs, transfers, tunnel.
//!
//! Phase 1: the machine key, enrollment requested through the local
//! endpoint, the iroh connection to the server with its heartbeat, the
//! inventory (computer, OS, DaVinci Resolve).
//!
//! At rest the agent waits: on its local endpoint, on its connection to the
//! server, and on a 5-minute timer for the heartbeat. Nothing else runs.

pub mod control;
pub mod enroll;
pub mod local;
pub mod packages;
pub mod state;
pub mod update;

use std::future::Future;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::Context;
use banc_platform::local::LocalListener;
use banc_platform::secrets::SecretStore;
use banc_proto::enroll::EnrollResponse;
use banc_proto::keys::{Fingerprint, SecretKey};
use banc_proto::local::{AgentStatus, Connection as ConnectionState, Enrollment};
use banc_proto::machine::Inventory;
use iroh::endpoint::presets;
use iroh::{Endpoint, RelayMap, RelayMode, RelayUrl};
use tokio::sync::watch;

use crate::state::{Enrolled, StateError};

/// Reads the inventory. Replaced in tests.
pub type InventorySource = Arc<dyn Fn() -> Inventory + Send + Sync>;

/// The machine's real inventory. The computer name and the OS version are
/// read once; Resolve at every call, since it can be installed or updated
/// while the agent runs.
pub fn system_inventory() -> InventorySource {
    let hostname = banc_platform::machine::hostname();
    let os_version = banc_platform::machine::os_version();
    Arc::new(move || Inventory {
        hostname: hostname.clone(),
        os_version: os_version.clone(),
        resolve: banc_resolve::installed(),
    })
}

pub struct Config {
    /// Where `agent.json` lives.
    pub state_dir: PathBuf,
    pub secrets: SecretStore,
    /// Local endpoint for the interface.
    pub socket: PathBuf,
    pub inventory: InventorySource,
}

/// State shared by the local endpoint and the connection to the server.
pub struct Shared {
    state_dir: PathBuf,
    secrets: SecretStore,
    inventory: InventorySource,
    enrollment: watch::Sender<Option<(Enrolled, SecretKey)>>,
    connection: Mutex<ConnectionState>,
    enrolling: Mutex<bool>,
}

/// Held during an enrollment.
pub struct EnrollmentGuard<'a>(&'a Shared);

impl Drop for EnrollmentGuard<'_> {
    fn drop(&mut self) {
        *lock(&self.0.enrolling) = false;
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Shared {
    fn new(config: &Config, enrolled: Option<(Enrolled, SecretKey)>) -> Self {
        Self {
            state_dir: config.state_dir.clone(),
            secrets: config.secrets.clone(),
            inventory: config.inventory.clone(),
            enrollment: watch::Sender::new(enrolled),
            connection: Mutex::new(ConnectionState::Connecting),
            enrolling: Mutex::new(false),
        }
    }

    pub fn inventory(&self) -> Inventory {
        (self.inventory)()
    }

    pub fn enrolled(&self) -> Option<Enrolled> {
        self.enrollment
            .borrow()
            .as_ref()
            .map(|(enrolled, _)| enrolled.clone())
    }

    pub fn set_connection(&self, state: ConnectionState) {
        *lock(&self.connection) = state;
    }

    pub fn status(&self) -> AgentStatus {
        let connection = *lock(&self.connection);
        AgentStatus {
            version: banc_proto::VERSION.to_owned(),
            enrollment: self.enrolled().map(|enrolled| Enrollment {
                server: enrolled.server,
                machine_id: enrolled.machine_id,
                name: enrolled.name,
                machine_role: enrolled.machine_role,
                person_role: enrolled.person_role,
                admin_fingerprint: Fingerprint::of(&enrolled.admin_key),
                connection,
            }),
            inventory: self.inventory(),
        }
    }

    /// `None` if another enrollment is running.
    pub fn begin_enrollment(&self) -> Option<EnrollmentGuard<'_>> {
        let mut enrolling = lock(&self.enrolling);
        if *enrolling {
            return None;
        }
        *enrolling = true;
        Some(EnrollmentGuard(self))
    }

    /// Stores the key, then the state, then starts the connection.
    pub fn complete_enrollment(
        &self,
        server: &str,
        name: &str,
        key: SecretKey,
        answer: EnrollResponse,
    ) -> Result<(), StateError> {
        let enrolled = Enrolled {
            server: server.to_owned(),
            machine_id: answer.machine_id,
            name: name.to_owned(),
            machine_role: answer.machine_role,
            person_role: answer.person_role,
            admin_key: answer.admin_key,
            server_info: answer.server,
        };
        state::save_key(&self.secrets, &key)?;
        state::save(&self.state_dir, &enrolled)?;
        tracing::info!(
            machine = enrolled.machine_id,
            server = %enrolled.server,
            admin_key = %Fingerprint::of(&enrolled.admin_key),
            "machine enrolled"
        );
        self.enrollment.send_replace(Some((enrolled, key)));
        Ok(())
    }
}

/// Runs the agent until `shutdown` completes.
pub async fn run(config: Config, shutdown: impl Future<Output = ()>) -> anyhow::Result<()> {
    let enrolled = load(&config)?;
    if enrolled.is_none() {
        tracing::info!("not enrolled yet, waiting for an invitation");
    }
    let shared = Arc::new(Shared::new(&config, enrolled));
    let listener = LocalListener::bind(&config.socket)
        .with_context(|| format!("listening on {}", config.socket.display()))?;
    let local = tokio::spawn(local::serve(listener, shared.clone()));

    tokio::pin!(shutdown);
    let mut receiver = shared.enrollment.subscribe();
    let mut endpoint = None;
    let result = loop {
        let current = receiver.borrow_and_update().clone();
        let Some((enrolled, key)) = current else {
            tokio::select! {
                changed = receiver.changed() => {
                    if changed.is_err() {
                        break Ok(());
                    }
                    continue;
                }
                () = &mut shutdown => break Ok(()),
            }
        };
        let bound = match bind(key, &enrolled).await {
            Ok(bound) => bound,
            Err(error) => break Err(error),
        };
        endpoint = Some(bound.clone());
        tokio::select! {
            () = control::maintain(&bound, &enrolled, &shared) => {
                // Revoked: nothing left to do but wait.
                (&mut shutdown).await;
                break Ok(());
            }
            () = &mut shutdown => break Ok(()),
        }
    };
    local.abort();
    if let Some(endpoint) = endpoint {
        endpoint.close().await;
    }
    result
}

/// The enrollment and key on disk. A state without its key is ignored: the
/// machine enrolls again.
fn load(config: &Config) -> anyhow::Result<Option<(Enrolled, SecretKey)>> {
    let Some(enrolled) = state::load(&config.state_dir)? else {
        return Ok(None);
    };
    match state::load_key(&config.secrets)? {
        Some(key) => {
            tracing::info!(machine = enrolled.machine_id, server = %enrolled.server, "enrolled");
            Ok(Some((enrolled, key)))
        }
        None => {
            tracing::error!("enrollment found without its machine key: enroll again");
            Ok(None)
        }
    }
}

/// The machine's iroh endpoint, with the team's relay as home relay.
async fn bind(key: SecretKey, enrolled: &Enrolled) -> anyhow::Result<Endpoint> {
    let relay = enrolled
        .server_info
        .relay_url
        .as_deref()
        .map(RelayUrl::from_str)
        .transpose()
        .context("relay URL from the server")?;
    let relay_mode = match relay {
        Some(url) => RelayMode::Custom(RelayMap::from(url)),
        None => RelayMode::Disabled,
    };
    Endpoint::builder(presets::Minimal)
        .secret_key(key)
        .relay_mode(relay_mode)
        .bind()
        .await
        .context("starting the iroh endpoint")
}
