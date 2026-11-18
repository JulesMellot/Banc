//! Phase 1, server side, end to end in one process: first admin, admin key,
//! invitation, enrollment over HTTP, control channel over iroh, relay,
//! revocation.
//!
//! The HTTP API is called through the router itself; the iroh parts use real
//! endpoints on 127.0.0.1.

#![cfg(test)]

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use banc_proto::control::{self, close, AgentMessage, ServerMessage};
use banc_proto::enroll::{EnrollRequest, EnrollResponse, ENROLL_PATH, INFO_PATH};
use banc_proto::invite::Invite;
use banc_proto::keys::{AdminKeyRegistration, SecretKey};
use banc_proto::machine::{Inventory, Os, ResolveEdition, ResolveInstall};
use banc_server::config::Config;
use banc_server::server::{self, Running};
use iroh::endpoint::{presets, ConnectionError};
use iroh::{Endpoint, EndpointAddr, RelayMap, RelayMode, RelayUrl, Watcher};
use serde_json::{json, Value};
use tower::ServiceExt;

const START: i64 = 1_800_000_000;
const PASSWORD: &str = "correct horse battery staple";

struct Harness {
    _dir: tempfile::TempDir,
    data_dir: std::path::PathBuf,
    server: Running,
    time: Arc<AtomicI64>,
}

impl Harness {
    async fn start(relay: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().join("data");
        let text = format!(
            r#"
            data_dir = '{}'
            public_url = "http://localhost:8080"
            [http]
            listen = "127.0.0.1:0"
            [iroh]
            listen = "127.0.0.1:0"
            [relay]
            enabled = {relay}
            listen = "127.0.0.1:0"
            "#,
            data_dir.display()
        );
        let config = Config::parse(&text, dir.path()).unwrap();
        let time = Arc::new(AtomicI64::new(START));
        let clock_time = time.clone();
        let clock = Arc::new(move || clock_time.load(Ordering::SeqCst));
        let server = server::start(&config, clock).await.unwrap();
        Self {
            _dir: dir,
            data_dir,
            server,
            time,
        }
    }

    fn advance(&self, seconds: i64) {
        self.time.fetch_add(seconds, Ordering::SeqCst);
    }

    async fn call(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
        cookie: Option<&str>,
    ) -> (StatusCode, Option<String>, Value) {
        let mut request = Request::builder().method(method).uri(path);
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        let body = match body {
            Some(body) => {
                request = request.header(header::CONTENT_TYPE, "application/json");
                Body::from(body.to_string())
            }
            None => Body::empty(),
        };
        let router = banc_server::api::router(self.server.state.clone());
        let response = router.oneshot(request.body(body).unwrap()).await.unwrap();
        let status = response.status();
        let set_cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .map(|value| value.to_str().unwrap().to_owned());
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        let json = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        (status, set_cookie, json)
    }

    /// Creates the first admin with the token file; returns the cookie.
    async fn first_admin(&self) -> String {
        let token = std::fs::read_to_string(self.data_dir.join("setup-token")).unwrap();
        let body = json!({ "token": token.trim(), "name": "jules", "password": PASSWORD });
        let (status, cookie, json) = self
            .call(Method::POST, "/api/v1/admin/setup", Some(body), None)
            .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["name"], "jules");
        cookie_pair(&cookie.unwrap())
    }

    async fn register_admin_key(&self, cookie: &str, secret: &SecretKey) -> Value {
        let body = serde_json::to_value(AdminKeyRegistration::signed(secret)).unwrap();
        let (status, _, json) = self
            .call(
                Method::PUT,
                "/api/v1/admin/admin-key",
                Some(body),
                Some(cookie),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        json
    }

    async fn invite(&self, cookie: &str) -> Invite {
        let body = json!({ "user_name": "Léa", "person_role": "editor" });
        let (status, _, json) = self
            .call(
                Method::POST,
                "/api/v1/admin/invites",
                Some(body),
                Some(cookie),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        let invite = Invite::parse_link(json["link"].as_str().unwrap()).unwrap();
        let from_key = Invite::from_server_and_key(
            json["server"].as_str().unwrap(),
            json["key"].as_str().unwrap(),
        )
        .unwrap();
        assert_eq!(invite, from_key);
        invite
    }

    async fn enroll(&self, invite: &Invite, machine: &SecretKey) -> (StatusCode, Value) {
        let request = EnrollRequest::signed(
            machine,
            invite.token,
            "Station 3".to_owned(),
            Os::Macos,
            "aarch64".to_owned(),
            "0.1.0".to_owned(),
        );
        let body = serde_json::to_value(request).unwrap();
        let (status, _, json) = self.call(Method::POST, ENROLL_PATH, Some(body), None).await;
        (status, json)
    }

    async fn machines(&self, cookie: &str) -> Vec<Value> {
        let (status, _, json) = self
            .call(Method::GET, "/api/v1/admin/machines", None, Some(cookie))
            .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        json.as_array().unwrap().clone()
    }

    /// Everything up to an enrolled machine.
    async fn enrolled(&self, machine: &SecretKey) -> (String, EnrollResponse) {
        let cookie = self.first_admin().await;
        self.register_admin_key(&cookie, &SecretKey::generate())
            .await;
        let invite = self.invite(&cookie).await;
        let (status, json) = self.enroll(&invite, machine).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        (cookie, serde_json::from_value(json).unwrap())
    }

    fn direct_addr(&self) -> EndpointAddr {
        let mut addr = EndpointAddr::new(self.server.endpoint_id);
        for socket in self.server.iroh_addrs() {
            addr = addr.with_ip_addr(socket);
        }
        addr
    }
}

/// `name=value` of a `Set-Cookie` header.
fn cookie_pair(set_cookie: &str) -> String {
    set_cookie.split(';').next().unwrap().to_owned()
}

async fn agent_endpoint(secret: SecretKey, relay: Option<RelayUrl>) -> Endpoint {
    let mode = match relay {
        Some(url) => RelayMode::Custom(RelayMap::from(url)),
        None => RelayMode::Disabled,
    };
    Endpoint::builder(presets::Minimal)
        .secret_key(secret)
        .relay_mode(mode)
        .bind()
        .await
        .unwrap()
}

fn inventory(version: &str) -> Inventory {
    Inventory {
        hostname: "station-3.local".to_owned(),
        os_version: "macOS 26.1".to_owned(),
        resolve: Some(ResolveInstall {
            edition: Some(ResolveEdition::Studio),
            version: version.to_owned(),
        }),
    }
}

/// Connects, says hello and returns the streams with the welcome.
async fn say_hello(
    endpoint: &Endpoint,
    addr: EndpointAddr,
) -> (
    iroh::endpoint::Connection,
    iroh::endpoint::SendStream,
    iroh::endpoint::RecvStream,
    ServerMessage,
) {
    let connection = endpoint.connect(addr, control::ALPN).await.unwrap();
    let (mut send, mut recv) = connection.open_bi().await.unwrap();
    let hello = AgentMessage::Hello {
        agent_version: "0.1.0".to_owned(),
        inventory: inventory("21.1"),
    };
    control::write_frame(&mut send, &hello).await.unwrap();
    let welcome = control::read_frame(&mut recv).await.unwrap().unwrap();
    (connection, send, recv, welcome)
}

/// Close code of a connection closed by the server.
async fn close_code(connection: &iroh::endpoint::Connection) -> u64 {
    let closed = tokio::time::timeout(Duration::from_secs(10), connection.closed())
        .await
        .unwrap();
    match closed {
        ConnectionError::ApplicationClosed(close) => close.error_code.into_inner(),
        other => panic!("unexpected end: {other:?}"),
    }
}

/// Waits until the admin API says the machine is online, or not.
async fn wait_online(harness: &Harness, cookie: &str, online: bool) -> Value {
    for _ in 0..100 {
        let machines = harness.machines(cookie).await;
        if machines[0]["online"] == online {
            return machines[0].clone();
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("machine never became online={online}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn enrollment_then_control_channel_then_revocation() {
    let harness = Harness::start(false).await;
    let machine = SecretKey::generate();

    let (status, _, info) = harness.call(Method::GET, INFO_PATH, None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(info["setup_required"], true);
    assert_eq!(info["endpoint_id"], harness.server.endpoint_id.to_string());

    let cookie = harness.first_admin().await;
    assert!(!harness.data_dir.join("setup-token").exists());
    let (_, _, info) = harness.call(Method::GET, INFO_PATH, None, None).await;
    assert_eq!(info["setup_required"], false);

    let admin_secret = SecretKey::generate();
    let key = harness.register_admin_key(&cookie, &admin_secret).await;
    assert_eq!(key["public_key"], admin_secret.public().to_string());

    // The invitation carries the fingerprint of the admin key, and the
    // enrollment answer the key itself: the workstation can check one
    // against the other.
    let invite = harness.invite(&cookie).await;
    assert!(invite.fingerprint.matches(&admin_secret.public()));
    assert_eq!(invite.server, "http://localhost:8080");
    let (status, json) = harness.enroll(&invite, &machine).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let enrolled: EnrollResponse = serde_json::from_value(json).unwrap();
    assert_eq!(enrolled.admin_key, admin_secret.public());
    assert_eq!(enrolled.server.endpoint_id, harness.server.endpoint_id);
    assert_eq!(enrolled.server.relay_url, None);

    // A token is single use.
    let (status, json) = harness.enroll(&invite, &SecretKey::generate()).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(json["error"], "invite_used");

    let machines = harness.machines(&cookie).await;
    assert_eq!(machines.len(), 1);
    assert_eq!(machines[0]["name"], "Station 3");
    assert_eq!(machines[0]["online"], false);
    assert_eq!(machines[0]["resolve"], Value::Null);

    // Control channel: hello, welcome, heartbeat.
    let endpoint = agent_endpoint(machine.clone(), None).await;
    let (connection, mut send, _recv, welcome) = say_hello(&endpoint, harness.direct_addr()).await;
    assert_eq!(
        welcome,
        ServerMessage::Welcome {
            machine_id: enrolled.machine_id,
            heartbeat_secs: 300,
            packages: true,
        }
    );
    let online = wait_online(&harness, &cookie, true).await;
    assert_eq!(online["resolve"]["version"], "21.1");
    assert_eq!(online["hostname"], "station-3.local");
    assert_eq!(online["last_seen_at"], START);

    harness.advance(300);
    control::write_frame(&mut send, &AgentMessage::Heartbeat)
        .await
        .unwrap();
    control::write_frame(
        &mut send,
        &AgentMessage::Report {
            inventory: inventory("21.2"),
        },
    )
    .await
    .unwrap();
    for _ in 0..100 {
        let machines = harness.machines(&cookie).await;
        if machines[0]["resolve"]["version"] == "21.2" {
            assert_eq!(machines[0]["last_seen_at"], START + 300);
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        harness.machines(&cookie).await[0]["resolve"]["version"],
        "21.2"
    );

    // Revocation cuts the connection at once, and the machine can no longer
    // come back.
    let path = format!("/api/v1/admin/machines/{}/revoke", enrolled.machine_id);
    let (status, _, _) = harness.call(Method::POST, &path, None, Some(&cookie)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(close_code(&connection).await, u64::from(close::REVOKED));
    let revoked = wait_online(&harness, &cookie, false).await;
    assert_eq!(revoked["revoked_at"], START + 300);

    let again = endpoint
        .connect(harness.direct_addr(), control::ALPN)
        .await
        .unwrap();
    assert_eq!(close_code(&again).await, u64::from(close::UNKNOWN_MACHINE));

    endpoint.close().await;
    harness.server.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unknown_machine_refused_on_the_control_channel() {
    let harness = Harness::start(false).await;
    let endpoint = agent_endpoint(SecretKey::generate(), None).await;
    let connection = endpoint
        .connect(harness.direct_addr(), control::ALPN)
        .await
        .unwrap();
    assert_eq!(
        close_code(&connection).await,
        u64::from(close::UNKNOWN_MACHINE)
    );
    endpoint.close().await;
    harness.server.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn protocol_errors_close_the_connection() {
    let harness = Harness::start(false).await;
    let machine = SecretKey::generate();
    harness.enrolled(&machine).await;
    let endpoint = agent_endpoint(machine, None).await;

    // A heartbeat before the hello.
    let connection = endpoint
        .connect(harness.direct_addr(), control::ALPN)
        .await
        .unwrap();
    let (mut send, _recv) = connection.open_bi().await.unwrap();
    control::write_frame(&mut send, &AgentMessage::Heartbeat)
        .await
        .unwrap();
    assert_eq!(close_code(&connection).await, u64::from(close::PROTOCOL));

    // A second hello.
    let (connection, mut send, _recv, _) = say_hello(&endpoint, harness.direct_addr()).await;
    let hello = AgentMessage::Hello {
        agent_version: "0.1.0".to_owned(),
        inventory: Inventory::default(),
    };
    control::write_frame(&mut send, &hello).await.unwrap();
    assert_eq!(close_code(&connection).await, u64::from(close::PROTOCOL));

    endpoint.close().await;
    harness.server.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn relay_serves_the_team_only() {
    let harness = Harness::start(true).await;
    let relay_addr = harness.server.relay_addr.unwrap();
    let relay_url: RelayUrl = format!("http://{relay_addr}").parse().unwrap();

    // A stranger is turned away by the relay itself.
    let stranger = agent_endpoint(SecretKey::generate(), Some(relay_url.clone())).await;
    let mut denied = None;
    for _ in 0..200 {
        let status = stranger.home_relay_status().get();
        denied = status
            .iter()
            .find_map(|relay| relay.auth_denied_reason().map(str::to_owned));
        if denied.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(denied.as_deref(), Some("not a member of this team"));
    stranger.close().await;

    // An enrolled machine reaches the server through the relay alone.
    let machine = SecretKey::generate();
    harness.enrolled(&machine).await;
    let endpoint = agent_endpoint(machine, Some(relay_url.clone())).await;
    let by_relay = EndpointAddr::new(harness.server.endpoint_id).with_relay_url(relay_url);
    let (_connection, _send, _recv, welcome) = say_hello(&endpoint, by_relay).await;
    assert!(matches!(welcome, ServerMessage::Welcome { .. }));

    endpoint.close().await;
    harness.server.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admin_api_needs_a_session() {
    let harness = Harness::start(false).await;
    for (method, path) in [
        (Method::GET, "/api/v1/admin/me"),
        (Method::GET, "/api/v1/admin/machines"),
        (Method::GET, "/api/v1/admin/invites"),
        (Method::GET, "/api/v1/admin/admin-key"),
        (Method::POST, "/api/v1/admin/machines/1/revoke"),
    ] {
        let (status, _, json) = harness.call(method, path, None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{path}");
        assert_eq!(json["error"], "unauthorized");
        let (status, _, _) = harness
            .call(Method::GET, path, None, Some("banc_session=AAAA"))
            .await;
        assert!(status == StatusCode::UNAUTHORIZED || status == StatusCode::METHOD_NOT_ALLOWED);
    }

    // Setup: wrong token, weak password, then a second attempt once done.
    let body = json!({ "token": "nope", "name": "jules", "password": PASSWORD });
    let (status, _, json) = harness
        .call(Method::POST, "/api/v1/admin/setup", Some(body), None)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(json["error"], "bad_setup_token");
    let token = std::fs::read_to_string(harness.data_dir.join("setup-token")).unwrap();
    let body = json!({ "token": token.trim(), "name": "jules", "password": "short" });
    let (status, _, json) = harness
        .call(Method::POST, "/api/v1/admin/setup", Some(body), None)
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["error"], "weak_password");
    harness.first_admin().await;
    let body = json!({ "token": token.trim(), "name": "paul", "password": PASSWORD });
    let (status, _, _) = harness
        .call(Method::POST, "/api/v1/admin/setup", Some(body), None)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Login.
    let body = json!({ "name": "jules", "password": "wrong password!" });
    let (status, cookie, json) = harness
        .call(Method::POST, "/api/v1/admin/login", Some(body), None)
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(json["error"], "bad_credentials");
    assert!(cookie.is_none());
    let body = json!({ "name": "nobody", "password": PASSWORD });
    let (status, _, _) = harness
        .call(Method::POST, "/api/v1/admin/login", Some(body), None)
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let body = json!({ "name": "jules", "password": PASSWORD });
    let (status, set_cookie, _) = harness
        .call(Method::POST, "/api/v1/admin/login", Some(body), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    let set_cookie = set_cookie.unwrap();
    assert!(set_cookie.contains("HttpOnly"), "{set_cookie}");
    assert!(set_cookie.contains("SameSite=Strict"), "{set_cookie}");
    let cookie = cookie_pair(&set_cookie);
    let (status, _, json) = harness
        .call(Method::GET, "/api/v1/admin/me", None, Some(&cookie))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["name"], "jules");

    // The session expires after 12 hours.
    harness.advance(12 * 3600);
    let (status, _, _) = harness
        .call(Method::GET, "/api/v1/admin/me", None, Some(&cookie))
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Logout ends the session.
    let body = json!({ "name": "jules", "password": PASSWORD });
    let (_, set_cookie, _) = harness
        .call(Method::POST, "/api/v1/admin/login", Some(body), None)
        .await;
    let cookie = cookie_pair(&set_cookie.unwrap());
    let (status, _, _) = harness
        .call(Method::POST, "/api/v1/admin/logout", None, Some(&cookie))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _, _) = harness
        .call(Method::GET, "/api/v1/admin/me", None, Some(&cookie))
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    harness.server.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admin_key_rules() {
    let harness = Harness::start(false).await;
    let cookie = harness.first_admin().await;

    // No invitation without an admin key.
    let body = json!({ "user_name": "Léa", "person_role": "editor" });
    let (status, _, json) = harness
        .call(
            Method::POST,
            "/api/v1/admin/invites",
            Some(body),
            Some(&cookie),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(json["error"], "admin_key_missing");

    // A registration signed by another key is refused.
    let mut forged = AdminKeyRegistration::signed(&SecretKey::generate());
    forged.public_key = SecretKey::generate().public();
    let body = serde_json::to_value(forged).unwrap();
    let (status, _, json) = harness
        .call(
            Method::PUT,
            "/api/v1/admin/admin-key",
            Some(body),
            Some(&cookie),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["error"], "bad_signature");

    let first = SecretKey::generate();
    harness.register_admin_key(&cookie, &first).await;
    // Registering the same key again changes nothing.
    harness.register_admin_key(&cookie, &first).await;
    let invite = harness.invite(&cookie).await;

    // Replacing it needs an explicit request, and makes pending invitations
    // obsolete.
    let second = SecretKey::generate();
    let body = serde_json::to_value(AdminKeyRegistration::signed(&second)).unwrap();
    let (status, _, json) = harness
        .call(
            Method::PUT,
            "/api/v1/admin/admin-key",
            Some(body.clone()),
            Some(&cookie),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(json["error"], "admin_key_exists");
    let mut replace = body;
    replace["replace"] = json!(true);
    let (status, _, json) = harness
        .call(
            Method::PUT,
            "/api/v1/admin/admin-key",
            Some(replace),
            Some(&cookie),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{json}");

    let (status, json) = harness.enroll(&invite, &SecretKey::generate()).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(json["error"], "invite_obsolete");
    let (_, _, invites) = harness
        .call(Method::GET, "/api/v1/admin/invites", None, Some(&cookie))
        .await;
    assert_eq!(invites[0]["status"], "obsolete");

    harness.server.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn enrollment_refusals() {
    let harness = Harness::start(false).await;
    let cookie = harness.first_admin().await;
    harness
        .register_admin_key(&cookie, &SecretKey::generate())
        .await;

    // Signed by another key than the one it enrolls.
    let invite = harness.invite(&cookie).await;
    let mut request = EnrollRequest::signed(
        &SecretKey::generate(),
        invite.token,
        "Station".to_owned(),
        Os::Linux,
        "x86_64".to_owned(),
        "0.1.0".to_owned(),
    );
    request.machine_key = SecretKey::generate().public();
    let body = serde_json::to_value(request).unwrap();
    let (status, _, json) = harness
        .call(Method::POST, ENROLL_PATH, Some(body), None)
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["error"], "bad_signature");

    // Malformed JSON.
    let (status, _, json) = harness
        .call(Method::POST, ENROLL_PATH, Some(json!({ "token": 1 })), None)
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["error"], "bad_request");

    // Revoked, then expired invitations.
    let (_, _, invites) = harness
        .call(Method::GET, "/api/v1/admin/invites", None, Some(&cookie))
        .await;
    let id = invites[0]["id"].as_i64().unwrap();
    let path = format!("/api/v1/admin/invites/{id}/revoke");
    let (status, _, _) = harness.call(Method::POST, &path, None, Some(&cookie)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, json) = harness.enroll(&invite, &SecretKey::generate()).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(json["error"], "invite_revoked");

    let invite = harness.invite(&cookie).await;
    harness.advance(7 * 24 * 3600);
    let (status, json) = harness.enroll(&invite, &SecretKey::generate()).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(json["error"], "invite_expired");

    harness.server.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn server_key_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let text = format!(
        "data_dir = '{}'\npublic_url = \"http://localhost:8080\"\n\
         [http]\nlisten = \"127.0.0.1:0\"\n[iroh]\nlisten = \"127.0.0.1:0\"\n\
         [relay]\nenabled = false\n",
        dir.path().join("data").display()
    );
    let config = Config::parse(&text, dir.path()).unwrap();
    let first = server::start(&config, Arc::new(|| START)).await.unwrap();
    let id = first.endpoint_id;
    first.shutdown().await.unwrap();
    let second = server::start(&config, Arc::new(|| START)).await.unwrap();
    assert_eq!(second.endpoint_id, id);
    second.shutdown().await.unwrap();
}
