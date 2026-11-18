//! Brings the workstation to what the signed manifest asks for.
//!
//! Wakes up when the server announces a manifest, when the connection comes
//! back, and, while packages are unfinished, every [`crate::PACKAGE_RETRY`].
//! At rest nothing here runs. The server is not trusted: the manifest must
//! carry the admin's signature and each file must match the hash it signs.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use banc_proto::control;
use banc_proto::keys::PublicKey;
use banc_proto::machine::Os;
use banc_proto::manifest::{
    Manifest, Package, PackageBuild, PackageFile, PackageKind, SignedManifest,
};
use banc_proto::packages::{short_error, Answer, Request, WorkstationReport, ALPN, MAX_MANIFEST};
use iroh::endpoint::{Connection, Endpoint, RecvStream};
use serde::{Deserialize, Serialize};

use crate::install::{self, InstallError};
use crate::packages::{self, Action, LocalPackage, LocalState, Machine};
use crate::state::Enrolled;
use crate::{control as server, lock, Shared};

const STATE_FILE: &str = "packages.json";
const DOWNLOADS_DIR: &str = "downloads";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// A package that failed is tried again after this time, or with a new
/// manifest.
const FAILURE_PAUSE: Duration = Duration::from_secs(3600);

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("the server is unreachable: {0}")]
    Network(String),
    #[error("the server does not have this file")]
    FileMissing,
    #[error("the file received does not match the signed manifest")]
    HashMismatch,
    #[error("{0}")]
    Install(#[from] InstallError),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("the package state is unreadable: {0}")]
    State(#[from] serde_json::Error),
}

fn network(error: impl std::fmt::Display) -> SyncError {
    SyncError::Network(error.to_string())
}

/// What the agent remembers about packages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stored {
    /// The key the manifest below was checked against.
    pub admin_key: PublicKey,
    /// The last manifest accepted, kept to work without the server.
    pub manifest: Option<SignedManifest>,
    /// What Banc installed here.
    pub local: LocalState,
}

impl Stored {
    fn empty(admin_key: PublicKey) -> Self {
        Self {
            admin_key,
            manifest: None,
            local: LocalState::new(),
        }
    }

    /// The state for this team. After joining a team with another admin
    /// key, its manifest counts for nothing; what Banc installed stays known.
    fn for_team(mut self, admin_key: PublicKey) -> Self {
        if self.admin_key != admin_key {
            self.admin_key = admin_key;
            self.manifest = None;
        }
        self
    }

    /// The manifest applied here, checked again: the file on disk is not
    /// trusted more than the network.
    fn applied(&self) -> Option<Manifest> {
        self.manifest
            .as_ref()
            .and_then(|signed| signed.open(&self.admin_key, None).ok())
    }
}

fn load(dir: &Path, admin_key: PublicKey) -> Result<Stored, SyncError> {
    match std::fs::read(dir.join(STATE_FILE)) {
        Ok(text) => Ok(serde_json::from_slice::<Stored>(&text)?.for_team(admin_key)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Stored::empty(admin_key)),
        Err(error) => Err(error.into()),
    }
}

fn save(dir: &Path, stored: &Stored) -> Result<(), SyncError> {
    let json = serde_json::to_vec_pretty(stored)?;
    banc_platform::fs::write_private(&dir.join(STATE_FILE), &json)?;
    Ok(())
}

/// Takes the newest acceptable manifest. A refused one changes nothing: the
/// workstation keeps applying the last good manifest, and says why.
pub fn accept(stored: &mut Stored, signed: SignedManifest) -> Result<bool, String> {
    let current = stored.applied().map(|manifest| manifest.version);
    match signed.open(&stored.admin_key, current) {
        Ok(manifest) => {
            let new = current != Some(manifest.version);
            stored.manifest = Some(signed);
            Ok(new)
        }
        Err(error) => Err(error.to_string()),
    }
}

/// What the package work keeps between two passes.
#[derive(Default)]
struct Attempts {
    failures: HashMap<String, (String, Instant)>,
}

impl Attempts {
    fn paused(&self, id: &str) -> bool {
        self.failures
            .get(id)
            .is_some_and(|(_, at)| at.elapsed() < FAILURE_PAUSE)
    }

    fn errors(&self) -> HashMap<String, String> {
        self.failures
            .iter()
            .map(|(id, (error, _))| (id.clone(), error.clone()))
            .collect()
    }
}

/// Runs for as long as the machine is enrolled.
pub async fn run(endpoint: &Endpoint, enrolled: &Enrolled, shared: &Arc<Shared>) {
    let mut attempts = Attempts::default();
    loop {
        let unfinished = match pass(endpoint, enrolled, shared, &mut attempts).await {
            Ok(unfinished) => unfinished,
            Err(error) => {
                tracing::info!("packages: {error}");
                true
            }
        };
        if unfinished {
            tokio::select! {
                () = shared.package_wake.notified() => {}
                () = tokio::time::sleep(shared.package_retry) => {}
            }
        } else {
            shared.package_wake.notified().await;
        }
    }
}

fn this_machine(resolve: Option<&str>) -> Option<Machine<'_>> {
    Some(Machine {
        os: Os::from_str(banc_platform::machine::OS).ok()?,
        arch: banc_platform::machine::ARCH,
        resolve,
    })
}

/// One look at the manifest. Returns whether work remains.
async fn pass(
    endpoint: &Endpoint,
    enrolled: &Enrolled,
    shared: &Arc<Shared>,
    attempts: &mut Attempts,
) -> Result<bool, SyncError> {
    let dir = &shared.state_dir;
    let mut stored = load(dir, enrolled.admin_key)?;
    apply_confirmations(&mut stored, shared);

    // The server may be away: the last manifest still applies.
    let connection = connect(endpoint, enrolled).await;
    let mut refused = None;
    let mut reachable = false;
    if let Ok(connection) = &connection {
        match fetch_manifest(connection).await {
            Ok(Some(signed)) => {
                reachable = true;
                match accept(&mut stored, signed) {
                    Ok(true) => {
                        attempts.failures.clear();
                        tracing::info!("new manifest accepted");
                    }
                    Ok(false) => {}
                    Err(reason) => {
                        tracing::error!("manifest refused: {reason}");
                        refused = Some(short_error(&reason));
                    }
                }
            }
            Ok(None) => reachable = true,
            Err(error) => tracing::info!("fetching the manifest: {error}"),
        }
    }
    save(dir, &stored)?;

    let Some(manifest) = stored.applied() else {
        publish(
            shared,
            WorkstationReport {
                refused,
                ..WorkstationReport::default()
            },
        );
        return Ok(!reachable);
    };

    let resolve = shared.inventory().resolve.map(|install| install.version);
    let Some(machine) = this_machine(resolve.as_deref()) else {
        return Ok(false);
    };
    let report = |stored: &Stored, attempts: &Attempts| {
        let mut report = packages::report(&manifest, machine, &stored.local, &attempts.errors());
        report.refused.clone_from(&refused);
        report
    };
    publish(shared, report(&stored, attempts));

    let planned = packages::plan(&manifest, machine, &stored.local);
    let resolve_open = (shared.resolve_open)();
    for action in planned.runnable(resolve_open) {
        if matches!(action, Action::Manual { .. }) || attempts.paused(action.id()) {
            continue;
        }
        let id = action.id().to_owned();
        let done = perform(
            action,
            &manifest,
            machine,
            &mut stored,
            shared,
            connection.as_ref().ok(),
        )
        .await;
        match done {
            Ok(()) => {
                attempts.failures.remove(&id);
            }
            // The network comes back by itself: nothing to hold against the package.
            Err(SyncError::Network(reason)) => {
                tracing::info!(package = %id, "package delayed: {reason}");
            }
            Err(error) => {
                tracing::error!(package = %id, "package not installed: {error}");
                attempts
                    .failures
                    .insert(id, (error.to_string(), Instant::now()));
            }
        }
        save(dir, &stored)?;
        publish(shared, report(&stored, attempts));
    }
    if let Ok(connection) = connection {
        connection.close(0u32.into(), b"done");
    }

    // Manual packages wait for a person, who wakes this work up.
    let remaining = packages::plan(&manifest, machine, &stored.local);
    Ok(remaining
        .actions
        .iter()
        .any(|action| !matches!(action, Action::Manual { .. })))
}

fn publish(shared: &Shared, report: WorkstationReport) {
    shared.report.send_if_modified(|current| {
        let changed = current.as_ref() != Some(&report);
        *current = Some(report);
        changed
    });
}

/// Records the manual packages a person confirmed, if the manifest still
/// asks for them.
fn apply_confirmations(stored: &mut Stored, shared: &Shared) {
    let confirmed = std::mem::take(&mut *lock(&shared.confirmed));
    let Some(manifest) = stored.applied() else {
        return;
    };
    for id in confirmed {
        let package = manifest.packages.iter().find(|package| package.id == id);
        let manual = package.filter(|package| {
            package
                .builds
                .iter()
                .any(|build| build.kind == PackageKind::Manual)
        });
        if let Some(package) = manual {
            tracing::info!(package = %id, version = %package.version, "manual package confirmed");
            stored.local.insert(
                id,
                LocalPackage {
                    version: package.version.clone(),
                    installed_from: None,
                    kind: Some(PackageKind::Manual),
                    files: Vec::new(),
                },
            );
        }
    }
}

async fn perform(
    action: &Action,
    manifest: &Manifest,
    machine: Machine<'_>,
    stored: &mut Stored,
    shared: &Arc<Shared>,
    connection: Option<&Connection>,
) -> Result<(), SyncError> {
    if let Action::Remove { id } = action {
        return remove(id, stored, shared).await;
    }
    let id = action.id();
    let package = manifest.packages.iter().find(|package| package.id == id);
    let found = package.and_then(|package| {
        let build = package.build_for(machine.os, machine.arch)?;
        Some((package, build, build.file.as_ref()?))
    });
    let Some((package, build, file)) = found else {
        return Err(InstallError::NotInstallable.into());
    };
    let connection = connection.ok_or_else(|| network("no connection"))?;
    let downloads = shared.state_dir.join(DOWNLOADS_DIR);
    // Private: an installer runs from here with the service's rights.
    banc_platform::fs::create_private_dir(&downloads)?;
    let path = download(connection, file, &downloads).await?;
    let installed = put_in_place(package, build, file, &path, stored, shared).await;
    let _ = std::fs::remove_file(&path);
    installed
}

async fn put_in_place(
    package: &Package,
    build: &PackageBuild,
    file: &PackageFile,
    path: &Path,
    stored: &mut Stored,
    shared: &Arc<Shared>,
) -> Result<(), SyncError> {
    let owned = stored
        .local
        .get(&package.id)
        .map(|local| local.files.clone())
        .unwrap_or_default();
    let work = shared.state_dir.join(DOWNLOADS_DIR);
    let places = shared.places.clone();
    let (task_build, task_path, name) = (build.clone(), path.to_path_buf(), file.name.clone());
    tracing::info!(package = %package.id, version = %package.version, "installing");
    let files = tokio::task::spawn_blocking(move || {
        install::install(&task_build, &task_path, &name, &work, &owned, &places)
    })
    .await
    .map_err(|error| std::io::Error::other(error.to_string()))??;
    tracing::info!(package = %package.id, version = %package.version, "installed");
    stored.local.insert(
        package.id.clone(),
        LocalPackage {
            version: package.version.clone(),
            installed_from: Some(file.blake3.clone()),
            kind: Some(build.kind),
            files,
        },
    );
    Ok(())
}

async fn remove(id: &str, stored: &mut Stored, shared: &Arc<Shared>) -> Result<(), SyncError> {
    let Some(local) = stored.local.get(id).cloned() else {
        return Ok(());
    };
    if local.files.is_empty() {
        // An installer's files are known to its vendor only.
        tracing::warn!(package = %id, "dropped from the manifest: left installed");
    } else {
        let places = shared.places.clone();
        let (kind, files) = (local.kind, local.files);
        tokio::task::spawn_blocking(move || install::remove(kind, &files, &places))
            .await
            .map_err(|error| std::io::Error::other(error.to_string()))??;
        tracing::info!(package = %id, "removed");
    }
    stored.local.remove(id);
    Ok(())
}

async fn connect(endpoint: &Endpoint, enrolled: &Enrolled) -> Result<Connection, SyncError> {
    let addr = server::server_addr(enrolled);
    tokio::time::timeout(CONNECT_TIMEOUT, endpoint.connect(addr, ALPN))
        .await
        .map_err(|_| network("timed out"))?
        .map_err(network)
}

/// Sends a request. `None` when the server does not have what was asked.
async fn ask(
    connection: &Connection,
    request: &Request,
) -> Result<Option<(u64, RecvStream)>, SyncError> {
    let (mut send, mut recv) = connection.open_bi().await.map_err(network)?;
    control::write_frame(&mut send, request)
        .await
        .map_err(network)?;
    send.finish().map_err(network)?;
    let answer = control::read_frame::<_, Answer>(&mut recv)
        .await
        .map_err(network)?;
    match answer {
        Some(Answer::Found { size }) => Ok(Some((size, recv))),
        Some(Answer::NotFound) => Ok(None),
        None => Err(network("no answer")),
    }
}

async fn fetch_manifest(connection: &Connection) -> Result<Option<SignedManifest>, SyncError> {
    let Some((size, mut recv)) = ask(connection, &Request::Manifest).await? else {
        return Ok(None);
    };
    if size > MAX_MANIFEST {
        return Err(network("manifest too large"));
    }
    let mut body = vec![0; usize::try_from(size).map_err(network)?];
    recv.read_exact(&mut body).await.map_err(network)?;
    Ok(Some(serde_json::from_slice(&body)?))
}

/// Downloads a file. It only gets its real name, which installers rely on,
/// once its hash matches the signed manifest; anything else is deleted.
async fn download(
    connection: &Connection,
    file: &PackageFile,
    dir: &Path,
) -> Result<PathBuf, SyncError> {
    let request = Request::File {
        blake3: file.blake3.clone(),
    };
    let Some((size, mut recv)) = ask(connection, &request).await? else {
        return Err(SyncError::FileMissing);
    };
    if size != file.size {
        return Err(SyncError::HashMismatch);
    }
    let partial = dir.join(format!("{}.part", file.blake3));
    let result = receive(&mut recv, size, &partial, &file.blake3).await;
    if result.is_err() {
        let _ = std::fs::remove_file(&partial);
        return Err(result.err().unwrap_or(SyncError::HashMismatch));
    }
    let path = dir.join(&file.name);
    std::fs::rename(&partial, &path)?;
    Ok(path)
}

async fn receive(
    recv: &mut RecvStream,
    size: u64,
    path: &Path,
    blake3: &str,
) -> Result<(), SyncError> {
    let mut output = std::fs::File::create(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut remaining = size;
    let mut buffer = vec![0; 64 * 1024];
    while remaining > 0 {
        let wanted = buffer
            .len()
            .min(usize::try_from(remaining).unwrap_or(buffer.len()));
        let read = recv.read(&mut buffer[..wanted]).await.map_err(network)?;
        let Some(read) = read.filter(|read| *read > 0) else {
            return Err(network("the transfer stopped early"));
        };
        hasher.update(&buffer[..read]);
        output.write_all(&buffer[..read])?;
        remaining -= read as u64;
    }
    output.sync_all()?;
    if hasher.finalize().to_hex().as_str() == blake3 {
        Ok(())
    } else {
        Err(SyncError::HashMismatch)
    }
}

#[cfg(test)]
mod tests {
    use banc_proto::keys::SecretKey;

    use super::*;

    fn key(seed: u8) -> SecretKey {
        SecretKey::from_bytes(&[seed; 32])
    }

    fn signed(version: u64, seed: u8) -> SignedManifest {
        let manifest = Manifest {
            version,
            resolve: None,
            packages: Vec::new(),
        };
        SignedManifest::sign(&key(seed), &manifest).unwrap()
    }

    #[test]
    fn a_newer_manifest_replaces_the_current_one() {
        let mut stored = Stored::empty(key(1).public());
        assert_eq!(accept(&mut stored, signed(1, 1)), Ok(true));
        assert_eq!(accept(&mut stored, signed(1, 1)), Ok(false), "same one");
        assert_eq!(accept(&mut stored, signed(2, 1)), Ok(true));
        assert_eq!(stored.applied().unwrap().version, 2);
    }

    #[test]
    fn a_refused_manifest_changes_nothing() {
        let mut stored = Stored::empty(key(1).public());
        accept(&mut stored, signed(5, 1)).unwrap();
        assert!(accept(&mut stored, signed(6, 2)).is_err(), "another key");
        assert!(accept(&mut stored, signed(4, 1)).is_err(), "older");
        let mut changed = signed(7, 1);
        changed.payload = changed.payload.replace("\"version\":7", "\"version\":8");
        assert!(accept(&mut stored, changed).is_err(), "changed on the way");
        assert_eq!(stored.applied().unwrap().version, 5);
    }

    #[test]
    fn a_manifest_changed_on_disk_is_not_applied() {
        let mut stored = Stored::empty(key(1).public());
        accept(&mut stored, signed(5, 1)).unwrap();
        if let Some(manifest) = &mut stored.manifest {
            manifest.payload = manifest
                .payload
                .replace("\"packages\":[]", "\"packages\":[ ]");
        }
        assert_eq!(stored.applied(), None);
    }

    #[test]
    fn another_team_starts_from_its_own_manifests() {
        let mut stored = Stored::empty(key(1).public());
        accept(&mut stored, signed(9, 1)).unwrap();
        stored.local.insert(
            "font".to_owned(),
            LocalPackage {
                version: "1".to_owned(),
                installed_from: Some("ab".repeat(32)),
                kind: Some(PackageKind::Font),
                files: Vec::new(),
            },
        );
        let mut next = stored.for_team(key(2).public());
        assert_eq!(next.manifest, None);
        assert!(next.local.contains_key("font"), "still known as Banc's");
        assert_eq!(accept(&mut next, signed(1, 2)), Ok(true));
    }

    #[test]
    fn the_state_survives_a_restart() {
        let tmp = tempfile::tempdir().unwrap();
        let mut stored = Stored::empty(key(1).public());
        accept(&mut stored, signed(3, 1)).unwrap();
        save(tmp.path(), &stored).unwrap();
        assert_eq!(load(tmp.path(), key(1).public()).unwrap(), stored);
    }

    #[test]
    fn a_failure_waits_before_another_attempt() {
        let mut attempts = Attempts::default();
        assert!(!attempts.paused("font"));
        attempts
            .failures
            .insert("font".to_owned(), ("broken".to_owned(), Instant::now()));
        assert!(attempts.paused("font"));
        assert_eq!(attempts.errors()["font"], "broken");
    }
}
