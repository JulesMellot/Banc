//! Phase 1 end to end: a real server and a real agent in one process.
//!
//! The agent enrolls through its local endpoint, as the interface will ask
//! it to, connects to the server over iroh, reports its inventory, comes
//! back after a restart with its stored key, and stops when revoked.

#![cfg(test)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use banc_agent::{Config, InventorySource};
use banc_platform::secrets::SecretStore;
use banc_proto::control;
use banc_proto::invite::{Invite, InviteToken};
use banc_proto::keys::{Fingerprint, SecretKey};
use banc_proto::local::{Connection, InviteInput, Request, Response};
use banc_proto::machine::{Inventory, MachineRole, PersonRole, ResolveEdition, ResolveInstall};
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
            edition: ResolveEdition::Studio,
            version: "21.1".to_owned(),
        }),
    })
}

impl Agent {
    fn start(dir: &Path) -> Self {
        let socket = banc_platform::local::endpoint_in(dir, "agent");
        let config = Config {
            state_dir: dir.join("state"),
            secrets: SecretStore::in_dir(dir.join("state/secrets")),
            socket: socket.clone(),
            inventory: inventory(),
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
