//! Phase 1 end to end: a real server and a real agent in one process.
//!
//! The agent enrolls through its local endpoint, as the interface will ask
//! it to, connects to the server over iroh, reports its inventory, comes
//! back after a restart with its stored key, and stops when revoked.

#![cfg(test)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use banc_agent::install::{Fonts, Places};
use banc_agent::{Config, InventorySource, ResolveOpen};
use banc_platform::paths::ResolveDirs;
use banc_platform::secrets::SecretStore;
use banc_proto::control;
use banc_proto::invite::{Invite, InviteToken};
use banc_proto::keys::{Fingerprint, SecretKey};
use banc_proto::local::{Connection, InviteInput, Request, Response};
use banc_proto::machine::{Inventory, MachineRole, Os, PersonRole, ResolveEdition, ResolveInstall};
use banc_proto::manifest::{
    Manifest, Package, PackageBuild, PackageFile, PackageKind, Place, RequiredResolve,
    SignedManifest,
};
use banc_proto::packages::{PackageState, WorkstationReport};
use banc_server::config::Config as ServerConfig;
use banc_server::server::{self, Running};
use banc_server::store;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

const NOW: i64 = 1_800_000_000;

struct Server {
    _dir: tempfile::TempDir,
    running: Running,
    admin_key: SecretKey,
}

impl Server {
    async fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        // The agent dials the address the server announces: pick the port
        // before starting.
        let port = std::net::UdpSocket::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let text = format!(
            "data_dir = '{}'\npublic_url = \"http://localhost:8080\"\n\
             [http]\nlisten = \"127.0.0.1:0\"\n\
             [iroh]\nlisten = \"127.0.0.1:{port}\"\npublic_addrs = [\"127.0.0.1:{port}\"]\n\
             [relay]\nenabled = false\n",
            dir.path().join("data").display()
        );
        let config = ServerConfig::parse(&text, dir.path()).unwrap();
        let running = server::start(&config, Arc::new(|| NOW)).await.unwrap();
        let admin_key = SecretKey::generate();
        {
            let mut db = running.state.db();
            store::create_admin(&db, "jules", "hash", NOW).unwrap();
            store::set_admin_key(&mut db, &admin_key.public(), 1, NOW).unwrap();
        }
        Self {
            _dir: dir,
            running,
            admin_key,
        }
    }

    /// A pending invitation, as the admin would send it.
    fn invite(&self, token: u8) -> Invite {
        let token = InviteToken::from_bytes([token; 32]);
        let new = store::NewInvite {
            token_hash: &banc_server::auth::token_hash(token.as_bytes()),
            user_name: "Léa",
            person_role: PersonRole::Editor,
            machine_role: MachineRole::Workstation,
            admin_key: &self.admin_key.public(),
            created_by: 1,
        };
        store::create_invite(&self.running.state.db(), &new, NOW).unwrap();
        Invite {
            server: format!("http://{}", self.running.http_addr),
            token,
            fingerprint: Fingerprint::of(&self.admin_key.public()),
        }
    }

    fn machines(&self) -> Vec<store::MachineRow> {
        store::list_machines(&self.running.state.db()).unwrap()
    }
}

impl Server {
    /// A server as a hosting platform runs it: one HTTP port, carrying the
    /// API and the relay, and no direct iroh address given to the agents.
    async fn start_relay_only() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let text = format!(
            "data_dir = '{}'\npublic_url = \"http://localhost:{port}\"\n\
             [http]\nlisten = \"127.0.0.1:{port}\"\n\
             [iroh]\nlisten = \"127.0.0.1:0\"\n\
             [relay]\nlisten = \"127.0.0.1:0\"\n",
            dir.path().join("data").display()
        );
        let config = ServerConfig::parse(&text, dir.path()).unwrap();
        let running = server::start(&config, Arc::new(|| NOW)).await.unwrap();
        let admin_key = SecretKey::generate();
        {
            let mut db = running.state.db();
            store::create_admin(&db, "jules", "hash", NOW).unwrap();
            store::set_admin_key(&mut db, &admin_key.public(), 1, NOW).unwrap();
        }
        Self {
            _dir: dir,
            running,
            admin_key,
        }
    }
}

struct Agent {
    task: JoinHandle<anyhow::Result<()>>,
    stop: oneshot::Sender<()>,
    socket: PathBuf,
}

fn inventory() -> InventorySource {
    Arc::new(|| Inventory {
        hostname: "station-3".to_owned(),
        os_version: "Test OS 1".to_owned(),
        resolve: Some(ResolveInstall {
            edition: Some(ResolveEdition::Studio),
            version: "21.1".to_owned(),
        }),
    })
}

fn places(dir: &Path) -> Places {
    let root = dir.join("machine");
    Places {
        resolve: ResolveDirs {
            ofx: root.join("ofx"),
            lut: root.join("lut"),
            fusion_templates: root.join("templates"),
            scripts: root.join("scripts"),
        },
        fonts: Fonts::Folder(root.join("fonts")),
    }
}

impl Agent {
    fn start(dir: &Path) -> Self {
        Self::start_with(dir, Arc::new(|| false))
    }

    fn start_with(dir: &Path, resolve_open: ResolveOpen) -> Self {
        let socket = banc_platform::local::endpoint_in(dir, "agent");
        let config = Config {
            state_dir: dir.join("state"),
            secrets: SecretStore::in_dir(dir.join("state/secrets")),
            socket: socket.clone(),
            inventory: inventory(),
            places: places(dir),
            resolve_open,
            package_retry: Duration::from_millis(200),
        };
        let (stop, stopped) = oneshot::channel::<()>();
        let task = tokio::spawn(banc_agent::run(config, async {
            let _ = stopped.await;
        }));
        Self { task, stop, socket }
    }

    async fn ask(&self, request: &Request) -> Response {
        for _ in 0..100 {
            if let Ok(mut stream) = banc_platform::local::connect(&self.socket).await {
                control::write_frame(&mut stream, request).await.unwrap();
                return control::read_frame(&mut stream).await.unwrap().unwrap();
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("the agent does not answer");
    }

    async fn wait_connection(&self, expected: Connection) {
        for _ in 0..200 {
            if let Response::Status(status) = self.ask(&Request::Status).await {
                if status.enrollment.as_ref().map(|e| e.connection) == Some(expected) {
                    return;
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("the agent never reached {expected:?}");
    }

    async fn stop(self) {
        let _ = self.stop.send(());
        self.task.await.unwrap().unwrap();
    }
}

fn enroll_link(invite: &Invite) -> Request {
    Request::Enroll {
        invite: InviteInput::Link {
            link: invite.link(),
        },
        name: Some("Station 3".to_owned()),
    }
}

fn error_code(response: &Response) -> &str {
    match response {
        Response::Error { code, .. } => code,
        Response::Status(_) => "",
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn enroll_connect_restart_revoke() {
    let server = Server::start().await;
    let dir = tempfile::tempdir().unwrap();
    let agent = Agent::start(dir.path());

    let Response::Status(status) = agent.ask(&Request::Status).await else {
        panic!("status expected");
    };
    assert_eq!(status.enrollment, None);
    assert_eq!(status.inventory.hostname, "station-3");

    let invite = server.invite(1);
    let answer = agent.ask(&enroll_link(&invite)).await;
    let Response::Status(status) = answer else {
        panic!("enrollment refused: {answer:?}");
    };
    let enrollment = status.enrollment.unwrap();
    assert_eq!(enrollment.name, "Station 3");
    assert_eq!(enrollment.person_role, PersonRole::Editor);
    assert_eq!(enrollment.admin_fingerprint, invite.fingerprint);

    agent.wait_connection(Connection::Connected).await;
    let machines = server.machines();
    assert_eq!(machines.len(), 1);
    assert_eq!(machines[0].hostname.as_deref(), Some("station-3"));
    assert_eq!(machines[0].resolve.as_ref().unwrap().version, "21.1");
    let key = machines[0].public_key.parse().unwrap();
    assert!(server.running.state.connections.is_connected(&key));

    // A second enrollment is refused.
    let again = agent.ask(&enroll_link(&server.invite(2))).await;
    assert_eq!(error_code(&again), "already_enrolled");

    // After a restart the agent comes back with its stored key.
    agent.stop().await;
    let agent = Agent::start(dir.path());
    agent.wait_connection(Connection::Connected).await;
    assert_eq!(server.machines().len(), 1);

    // Revocation: the agent learns it and stops trying.
    let id = server.machines()[0].id;
    let key = store::revoke_machine(&server.running.state.db(), id, 1, NOW)
        .unwrap()
        .unwrap();
    server.running.state.connections.disconnect(&key);
    agent.wait_connection(Connection::Revoked).await;

    agent.stop().await;
    server.running.shutdown().await.unwrap();
}

/// Enrolls a new agent, then revokes it. Returns the revoked machine's key.
async fn enrolled_then_revoked(server: &Server, agent: &Agent) -> String {
    let answer = agent.ask(&enroll_link(&server.invite(1))).await;
    assert!(matches!(answer, Response::Status(_)), "{answer:?}");
    agent.wait_connection(Connection::Connected).await;
    let machine = server.machines().remove(0);
    let key = store::revoke_machine(&server.running.state.db(), machine.id, 1, NOW)
        .unwrap()
        .unwrap();
    server.running.state.connections.disconnect(&key);
    agent.wait_connection(Connection::Revoked).await;
    machine.public_key
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_revoked_machine_joins_again_with_a_new_invitation() {
    let server = Server::start().await;
    let dir = tempfile::tempdir().unwrap();
    let agent = Agent::start(dir.path());
    let old_key = enrolled_then_revoked(&server, &agent).await;

    let answer = agent.ask(&enroll_link(&server.invite(2))).await;
    assert!(matches!(answer, Response::Status(_)), "{answer:?}");
    agent.wait_connection(Connection::Connected).await;

    // A new identity: the revoked key stays revoked.
    let machines = server.machines();
    assert_eq!(machines.len(), 2);
    let active: Vec<_> = machines.iter().filter(|m| m.revoked_at.is_none()).collect();
    assert_eq!(active.len(), 1);
    assert_ne!(active[0].public_key, old_key);

    // Back in a team, a further enrollment is refused again.
    let again = agent.ask(&enroll_link(&server.invite(3))).await;
    assert_eq!(error_code(&again), "already_enrolled");

    // The new identity survives a restart.
    agent.stop().await;
    let agent = Agent::start(dir.path());
    agent.wait_connection(Connection::Connected).await;

    agent.stop().await;
    server.running.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_machine_revoked_before_a_restart_can_still_join_again() {
    let server = Server::start().await;
    let dir = tempfile::tempdir().unwrap();
    let agent = Agent::start(dir.path());
    enrolled_then_revoked(&server, &agent).await;

    agent.stop().await;
    let agent = Agent::start(dir.path());
    agent.wait_connection(Connection::Revoked).await;

    let answer = agent.ask(&enroll_link(&server.invite(2))).await;
    assert!(matches!(answer, Response::Status(_)), "{answer:?}");
    agent.wait_connection(Connection::Connected).await;

    agent.stop().await;
    server.running.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_invitation_leaves_a_revoked_machine_as_it_was() {
    let server = Server::start().await;
    let dir = tempfile::tempdir().unwrap();
    let agent = Agent::start(dir.path());
    enrolled_then_revoked(&server, &agent).await;

    // An invitation nobody issued.
    let mut forged = server.invite(2);
    forged.token = InviteToken::from_bytes([9; 32]);
    let answer = agent.ask(&enroll_link(&forged)).await;
    assert!(matches!(answer, Response::Error { .. }), "{answer:?}");
    agent.wait_connection(Connection::Revoked).await;
    assert_eq!(server.machines().len(), 1);

    agent.stop().await;
    server.running.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn connects_through_the_relay_on_the_http_port() {
    let server = Server::start_relay_only().await;
    assert!(server.running.state.server_info().direct_addrs.is_empty());
    let dir = tempfile::tempdir().unwrap();
    let agent = Agent::start(dir.path());

    let answer = agent.ask(&enroll_link(&server.invite(1))).await;
    assert!(matches!(answer, Response::Status(_)), "{answer:?}");
    agent.wait_connection(Connection::Connected).await;

    let key = server.machines()[0].public_key.parse().unwrap();
    assert!(server.running.state.connections.is_connected(&key));

    agent.stop().await;
    server.running.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn enrollment_refusals_leave_nothing_behind() {
    let server = Server::start().await;
    let dir = tempfile::tempdir().unwrap();
    let agent = Agent::start(dir.path());

    // Not a link.
    let request = Request::Enroll {
        invite: InviteInput::Link {
            link: "https://example.com".to_owned(),
        },
        name: None,
    };
    assert_eq!(error_code(&agent.ask(&request).await), "invalid_invite");

    // A token the server does not know.
    let mut unknown = server.invite(1);
    unknown.token = InviteToken::from_bytes([9; 32]);
    assert_eq!(
        error_code(&agent.ask(&enroll_link(&unknown)).await),
        "invite_unknown"
    );

    // A server whose admin key is not the invitation's: refused by the
    // agent even though the server accepted.
    let mut forged = server.invite(2);
    forged.fingerprint = Fingerprint::of(&SecretKey::generate().public());
    assert_eq!(
        error_code(&agent.ask(&enroll_link(&forged)).await),
        "admin_key_mismatch"
    );

    // The server cannot be reached.
    let mut offline = server.invite(3);
    offline.server = "http://127.0.0.1:9".to_owned();
    assert_eq!(
        error_code(&agent.ask(&enroll_link(&offline)).await),
        "server_unreachable"
    );

    let Response::Status(status) = agent.ask(&Request::Status).await else {
        panic!("status expected");
    };
    assert_eq!(status.enrollment, None);
    assert!(!dir.path().join("state/agent.json").exists());
    assert!(!dir.path().join("state/secrets/machine-key").exists());

    // An invitation from the right server still works.
    let answer = agent.ask(&enroll_link(&server.invite(4))).await;
    assert!(matches!(answer, Response::Status(_)), "{answer:?}");

    agent.stop().await;
    server.running.shutdown().await.unwrap();
}

// ---------------------------------------------------------------- phase 2

fn this_os() -> Os {
    banc_platform::machine::OS.parse().unwrap()
}

/// A zip archive, as an admin would upload it.
fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, content) in entries {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(content).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

impl Server {
    /// Puts a file in the package store.
    fn store_file(&self, name: &str, content: &[u8]) -> PackageFile {
        let blake3 = blake3::hash(content).to_hex().to_string();
        let dir = &self.running.state.packages_dir;
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(&blake3), content).unwrap();
        let row = store::PackageFileRow {
            blake3: blake3.clone(),
            name: name.to_owned(),
            size: content.len() as u64,
            uploaded_at: NOW,
        };
        store::add_package_file(&self.running.state.db(), &row, 1).unwrap();
        PackageFile {
            blake3,
            size: content.len() as u64,
            name: name.to_owned(),
        }
    }

    /// Signs and publishes a manifest, as the admin's application does.
    fn publish(&self, version: u64, resolve: Option<&str>, packages: Vec<Package>) {
        let manifest = Manifest {
            version,
            resolve: resolve.map(|version| RequiredResolve {
                version: version.to_owned(),
            }),
            packages,
        };
        let signed = SignedManifest::sign(&self.admin_key, &manifest).unwrap();
        store::publish_manifest(&mut self.running.state.db(), version, &signed, Some(1), NOW)
            .unwrap();
        self.running.state.manifest_version.send_replace(version);
    }

    fn report(&self) -> Option<WorkstationReport> {
        store::machine_packages(&self.running.state.db())
            .unwrap()
            .into_iter()
            .next()
            .map(|line| line.report)
    }

    async fn wait_report(&self, what: &str, done: impl Fn(&WorkstationReport) -> bool) {
        for _ in 0..400 {
            if self.report().is_some_and(|report| done(&report)) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("never happened: {what}, last report: {:?}", self.report());
    }
}

fn package(id: &str, kind: PackageKind, place: Option<Place>, file: PackageFile) -> Package {
    Package {
        id: id.to_owned(),
        version: "1.0".to_owned(),
        required: true,
        builds: vec![PackageBuild {
            os: this_os(),
            arch: None,
            kind,
            file: Some(file),
            place,
            args: Vec::new(),
            instructions: None,
        }],
    }
}

fn state_of(report: &WorkstationReport, id: &str) -> Option<PackageState> {
    report
        .packages
        .iter()
        .find(|package| package.id == id)
        .map(|package| package.state)
}

fn all_installed(report: &WorkstationReport, version: u64) -> bool {
    report.manifest_version == Some(version)
        && report
            .packages
            .iter()
            .all(|package| package.state == PackageState::Installed)
}

/// An enrolled and connected agent.
async fn joined(server: &Server, dir: &Path, resolve_open: ResolveOpen) -> Agent {
    let agent = Agent::start_with(dir, resolve_open);
    let answer = agent.ask(&enroll_link(&server.invite(9))).await;
    assert!(matches!(answer, Response::Status(_)), "{answer:?}");
    agent.wait_connection(Connection::Connected).await;
    agent
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_signed_manifest_installs_plugin_font_lut_and_preset() {
    let server = Server::start().await;
    let dir = tempfile::tempdir().unwrap();
    let agent = joined(&server, dir.path(), Arc::new(|| false)).await;
    let machine = dir.path().join("machine");

    let plugin = archive(&[("Glow.ofx.bundle/Contents/Info.plist", b"plist")]);
    let plugin = server.store_file("glow.zip", &plugin);
    let font = server.store_file("Inter.ttf", b"font");
    let lut = server.store_file("Team.cube", b"lut");
    let preset = server.store_file("Lower third.setting", b"preset");
    server.publish(
        1,
        None,
        vec![
            package("glow", PackageKind::Copy, Some(Place::Ofx), plugin),
            package("inter", PackageKind::Font, None, font),
            package("team-lut", PackageKind::Copy, Some(Place::Lut), lut),
            package(
                "lower-third",
                PackageKind::Copy,
                Some(Place::FusionTemplates),
                preset,
            ),
        ],
    );

    server
        .wait_report("everything installed", |report| {
            report.packages.len() == 4 && all_installed(report, 1)
        })
        .await;
    assert!(machine
        .join("ofx/Glow.ofx.bundle/Contents/Info.plist")
        .exists());
    assert_eq!(
        std::fs::read(machine.join("fonts/Inter.ttf")).unwrap(),
        b"font"
    );
    assert!(machine.join("lut/Team.cube").exists());
    assert!(machine.join("templates/Lower third.setting").exists());

    // The window sees the same state.
    let Response::Status(status) = agent.ask(&Request::Status).await else {
        panic!("status expected");
    };
    assert_eq!(status.packages.unwrap().packages.len(), 4);

    // A package dropped from the manifest goes away, the others stay.
    let kept = store::latest_manifest(&server.running.state.db())
        .unwrap()
        .unwrap();
    let kept: Manifest = serde_json::from_str(&kept.signed.payload).unwrap();
    let kept = kept
        .packages
        .into_iter()
        .filter(|package| package.id != "glow")
        .collect();
    server.publish(2, None, kept);
    server
        .wait_report("the plugin removed", |report| {
            report.packages.len() == 3 && all_installed(report, 2)
        })
        .await;
    for _ in 0..100 {
        if !machine.join("ofx/Glow.ofx.bundle").exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(!machine.join("ofx/Glow.ofx.bundle").exists());
    assert!(machine.join("fonts/Inter.ttf").exists());
    agent.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_manifest_changed_in_the_server_database_is_refused() {
    let server = Server::start().await;
    let dir = tempfile::tempdir().unwrap();
    let agent = joined(&server, dir.path(), Arc::new(|| false)).await;
    let machine = dir.path().join("machine");

    let good = server.store_file("Good.cube", b"good");
    server.publish(
        1,
        None,
        vec![package("good", PackageKind::Copy, Some(Place::Lut), good)],
    );
    server
        .wait_report("the first manifest", |report| {
            report.packages.len() == 1 && all_installed(report, 1)
        })
        .await;

    // Someone with access to the server adds a package to the manifest.
    let evil = server.store_file("Evil.cube", b"evil");
    {
        let db = server.running.state.db();
        let latest = store::latest_manifest(&db).unwrap().unwrap();
        let mut manifest: Manifest = serde_json::from_str(&latest.signed.payload).unwrap();
        manifest
            .packages
            .push(package("evil", PackageKind::Copy, Some(Place::Lut), evil));
        manifest.version = 2;
        db.execute(
            "INSERT INTO manifests (version, payload, signature, published_at)
             VALUES (2, ?1, ?2, 0)",
            (
                serde_json::to_string(&manifest).unwrap(),
                serde_json::to_string(&latest.signed.signature).unwrap(),
            ),
        )
        .unwrap();
    }
    server.running.state.manifest_version.send_replace(2);

    server
        .wait_report("the refusal", |report| report.refused.is_some())
        .await;
    let report = server.report().unwrap();
    assert_eq!(
        report.manifest_version,
        Some(1),
        "the good one still applies"
    );
    assert_eq!(state_of(&report, "evil"), None);
    assert!(machine.join("lut/Good.cube").exists());
    assert!(!machine.join("lut/Evil.cube").exists());
    agent.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_file_that_does_not_match_its_hash_is_refused() {
    let server = Server::start().await;
    let dir = tempfile::tempdir().unwrap();
    let agent = joined(&server, dir.path(), Arc::new(|| false)).await;
    let machine = dir.path().join("machine");

    let lut = server.store_file("Team.cube", b"the real lut");
    // The file is replaced on the server, with the same size.
    std::fs::write(
        server.running.state.packages_dir.join(&lut.blake3),
        b"a forged lut",
    )
    .unwrap();
    server.publish(
        1,
        None,
        vec![package(
            "team-lut",
            PackageKind::Copy,
            Some(Place::Lut),
            lut,
        )],
    );

    server
        .wait_report("the failure", |report| {
            state_of(report, "team-lut") == Some(PackageState::Failed)
        })
        .await;
    assert!(!machine.join("lut/Team.cube").exists());
    let downloads = dir.path().join("state/downloads");
    assert_eq!(std::fs::read_dir(downloads).unwrap().count(), 0, "deleted");
    agent.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nothing_is_installed_while_resolve_is_open() {
    let server = Server::start().await;
    let dir = tempfile::tempdir().unwrap();
    let open = Arc::new(AtomicBool::new(true));
    let flag = open.clone();
    let agent = joined(
        &server,
        dir.path(),
        Arc::new(move || flag.load(Ordering::SeqCst)),
    )
    .await;
    let installed = dir.path().join("machine/lut/Team.cube");

    let lut = server.store_file("Team.cube", b"lut");
    server.publish(
        1,
        Some("21.1"),
        vec![package(
            "team-lut",
            PackageKind::Copy,
            Some(Place::Lut),
            lut,
        )],
    );
    server
        .wait_report("the package waiting", |report| {
            state_of(report, "team-lut") == Some(PackageState::Pending)
        })
        .await;
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert!(!installed.exists(), "Resolve is open");

    open.store(false, Ordering::SeqCst);
    server
        .wait_report("the package installed", |report| {
            state_of(report, "team-lut") == Some(PackageState::Installed)
        })
        .await;
    assert!(installed.exists());
    assert!(
        server.report().unwrap().can_open_projects(),
        "right version"
    );
    agent.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_wrong_resolve_version_closes_projects() {
    let server = Server::start().await;
    let dir = tempfile::tempdir().unwrap();
    let agent = joined(&server, dir.path(), Arc::new(|| false)).await;

    server.publish(1, Some("22.0"), Vec::new());
    server
        .wait_report("the mismatch", |report| report.resolve_mismatch)
        .await;
    let report = server.report().unwrap();
    assert!(!report.can_open_projects());
    assert_eq!(report.resolve_required.as_deref(), Some("22.0"));
    agent.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_manual_package_waits_for_a_person() {
    let server = Server::start().await;
    let dir = tempfile::tempdir().unwrap();
    let agent = joined(&server, dir.path(), Arc::new(|| false)).await;

    let manual = Package {
        id: "licence".to_owned(),
        version: "1.0".to_owned(),
        required: true,
        builds: vec![PackageBuild {
            os: this_os(),
            arch: None,
            kind: PackageKind::Manual,
            file: None,
            place: None,
            args: Vec::new(),
            instructions: Some("Ask the admin for the dongle.".to_owned()),
        }],
    };
    server.publish(1, None, vec![manual]);
    server
        .wait_report("the instructions", |report| {
            state_of(report, "licence") == Some(PackageState::Manual)
        })
        .await;

    let confirm = Request::ConfirmManual {
        id: "licence".to_owned(),
    };
    assert!(matches!(agent.ask(&confirm).await, Response::Status(_)));
    server
        .wait_report("the confirmation", |report| {
            state_of(report, "licence") == Some(PackageState::Installed)
        })
        .await;
    agent.stop().await;
}
